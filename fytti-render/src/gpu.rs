use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

use cosmic_text::{Attrs, Buffer, Color as CosmicColor, FontSystem, Metrics, Shaping, SwashCache};

use crate::display_list::{DisplayList, DrawCmd};
use crate::glyph_atlas::GlyphAtlas;

/// GPU-accelerated 2D renderer using wgpu.
///
/// Rects and lines are drawn as instanced quads in a single draw call.
/// Text is rasterized on CPU via cosmic-text, uploaded as a texture, and
/// composited as a full-screen overlay.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,

    // Rect pipeline
    rect_pipeline: wgpu::RenderPipeline,
    viewport_buf: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,

    // Text overlay pipeline
    text_pipeline: wgpu::RenderPipeline,
    text_texture: wgpu::Texture,
    text_bind_group: wgpu::BindGroup,
    text_bind_group_layout: wgpu::BindGroupLayout,
    text_sampler: wgpu::Sampler,

    // Glyph atlas pipeline
    glyph_pipeline: wgpu::RenderPipeline,
    glyph_atlas: GlyphAtlas,
    glyph_atlas_texture: wgpu::Texture,
    glyph_atlas_bind_group: wgpu::BindGroup,
    glyph_instances: Vec<GlyphInstance>,
    glyph_gpu_buf: Option<wgpu::Buffer>,
    glyph_gpu_buf_capacity: usize,
    last_glyph_count: u32,

    // Image pipeline (reuses text pipeline shader — textured quad)
    image_textures: HashMap<u32, (wgpu::Texture, wgpu::BindGroup)>,

    // Text shaping (cosmic-text for layout, atlas for rendering)
    font_system: FontSystem,
    swash_cache: SwashCache,

    // Text shaping cache — avoid re-shaping identical text strings
    text_shape_cache: HashMap<u64, Vec<GlyphInstance>>,

    // Legacy text overlay (kept for fallback)
    text_pixels: Vec<u8>,

    // Reusable buffers (avoid per-frame allocations)
    rect_instances: Vec<RectInstance>,
    text_sub_buf: Vec<u8>,
    rect_gpu_buf: Option<wgpu::Buffer>,
    rect_gpu_buf_capacity: usize,

    // Dirty tracking — skip CPU work if display list unchanged
    last_dl_hash: u64,
    last_rect_count: u32,
    last_has_text: bool,
    last_clear_color: [f32; 4],

    width: u32,
    height: u32,
}

// Instance data: pos(2) + size(2) + color(4) + color2(4) + mode_extra(4) = 16 floats = 64 bytes
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct RectInstance {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    color2: [f32; 4],        // second color (for gradients)
    mode_and_extra: [f32; 4], // [mode, extra.x, extra.y, _pad]
}

// Glyph instance: pos(2) + size(2) + uv_rect(4) + color(4) = 12 floats = 48 bytes
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphInstance {
    pos: [f32; 2],     // screen position
    size: [f32; 2],    // screen size
    uv_rect: [f32; 4], // atlas UV: [x, y, w, h] in texels
    color: [f32; 4],   // tint color
}

impl RectInstance {
    fn solid(pos: [f32; 2], size: [f32; 2], color: [f32; 4]) -> Self {
        Self { pos, size, color, color2: [0.0; 4], mode_and_extra: [0.0; 4] }
    }

    fn gradient(pos: [f32; 2], size: [f32; 2], c1: [f32; 4], c2: [f32; 4], vertical: bool) -> Self {
        Self { pos, size, color: c1, color2: c2, mode_and_extra: [1.0, if vertical { 1.0 } else { 0.0 }, 0.0, 0.0] }
    }

    fn ellipse(pos: [f32; 2], size: [f32; 2], color: [f32; 4]) -> Self {
        Self { pos, size, color, color2: [0.0; 4], mode_and_extra: [2.0, 0.0, 0.0, 0.0] }
    }

    fn stroke_ellipse(pos: [f32; 2], size: [f32; 2], color: [f32; 4], stroke_w: f32) -> Self {
        // stroke_w normalized: fraction of radius used for stroke
        Self { pos, size, color, color2: [0.0; 4], mode_and_extra: [3.0, stroke_w, 0.0, 0.0] }
    }
}

impl GpuRenderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).expect("create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // ── Rect pipeline ──

        let rect_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
        });

        let viewport_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("viewport"),
            contents: bytemuck::cast_slice(&[width as f32, height as f32]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_bg"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buf.as_entire_binding(),
            }],
        });

        let rect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rect_pipeline_layout"),
                bind_group_layouts: &[&viewport_layout],
                push_constant_ranges: &[],
            });

        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&rect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &rect_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RectInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        // pos
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                        // size
                        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
                        // color
                        wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x4 },
                        // color2
                        wgpu::VertexAttribute { offset: 32, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
                        // mode_and_extra
                        wgpu::VertexAttribute { offset: 48, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &rect_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Text overlay pipeline ──

        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("text_overlay.wgsl").into()),
        });

        let text_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("text_bg_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let text_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("text_pipeline_layout"),
                bind_group_layouts: &[&text_bind_group_layout],
                push_constant_ranges: &[],
            });

        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text_pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let text_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let text_texture = Self::create_text_texture(&device, width, height);
        let text_bind_group =
            Self::create_text_bind_group(&device, &text_bind_group_layout, &text_texture, &text_sampler);

        let text_pixels = vec![0u8; (width * height * 4) as usize];

        // ── Glyph atlas pipeline ──

        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glyph.wgsl").into()),
        });

        let atlas_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas_bg_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let glyph_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyph_pipeline_layout"),
            bind_group_layouts: &[&viewport_layout, &atlas_bind_group_layout],
            push_constant_ranges: &[],
        });

        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&glyph_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &glyph_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },  // pos
                        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },  // size
                        wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x4 }, // uv_rect
                        wgpu::VertexAttribute { offset: 32, shader_location: 3, format: wgpu::VertexFormat::Float32x4 }, // color
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &glyph_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let atlas_w = 2048;
        let atlas_h = 2048;
        let glyph_atlas = GlyphAtlas::new(atlas_w, atlas_h);
        let glyph_atlas_texture = Self::create_text_texture(&device, atlas_w, atlas_h);
        let glyph_atlas_bind_group = Self::create_text_bind_group(&device, &atlas_bind_group_layout, &glyph_atlas_texture, &text_sampler);

        GpuRenderer {
            device,
            queue,
            surface,
            surface_config,
            rect_pipeline,
            viewport_buf,
            viewport_bind_group,
            text_pipeline,
            text_texture,
            text_bind_group,
            text_bind_group_layout,
            text_sampler,
            glyph_pipeline,
            glyph_atlas,
            glyph_atlas_texture,
            glyph_atlas_bind_group,
            glyph_instances: Vec::with_capacity(512),
            glyph_gpu_buf: None,
            glyph_gpu_buf_capacity: 0,
            last_glyph_count: 0,
            image_textures: HashMap::new(),
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            text_shape_cache: HashMap::with_capacity(64),
            text_pixels,
            rect_instances: Vec::with_capacity(1024),
            text_sub_buf: Vec::with_capacity(64 * 1024),
            rect_gpu_buf: None,
            rect_gpu_buf_capacity: 0,
            last_dl_hash: 0,
            last_rect_count: 0,
            last_has_text: false,
            last_clear_color: [0.0; 4],
            width,
            height,
        }
    }

    /// Load an image from raw RGBA bytes and cache it as a GPU texture.
    /// Returns the image_id for use in DrawCmd::Image.
    pub fn load_image_rgba(&mut self, image_id: u32, rgba: &[u8], width: u32, height: u32) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image_bg"),
            layout: &self.text_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.text_sampler),
                },
            ],
        });

        self.image_textures.insert(image_id, (texture, bind_group));
    }

    /// Load an image from a file path (PNG, JPEG, WebP).
    pub fn load_image_file(&mut self, image_id: u32, path: &str) -> Result<(), String> {
        let img = image::open(path).map_err(|e| format!("Failed to load image {path}: {e}"))?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        self.load_image_rgba(image_id, &rgba, w, h);
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if w == self.width && h == self.height {
            return;
        }
        self.width = w;
        self.height = h;
        self.surface_config.width = w;
        self.surface_config.height = h;
        self.surface.configure(&self.device, &self.surface_config);

        self.queue
            .write_buffer(&self.viewport_buf, 0, bytemuck::cast_slice(&[w as f32, h as f32]));

        self.text_texture = Self::create_text_texture(&self.device, w, h);
        self.text_bind_group = Self::create_text_bind_group(
            &self.device,
            &self.text_bind_group_layout,
            &self.text_texture,
            &self.text_sampler,
        );
        self.text_pixels.resize((w * h * 4) as usize, 0);
        self.text_pixels.fill(0);
        self.last_dl_hash = 0;
        self.text_shape_cache.clear(); // positions change on resize
    }

    /// Render a display list to the screen.
    /// Skips everything if the display list hasn't changed.
    /// Returns true if a frame was actually rendered (dirty).
    pub fn render(&mut self, dl: &DisplayList) -> bool {
        // Dirty check — skip everything if display list unchanged
        let dl_hash = dl.content_hash();
        let dirty = dl_hash != self.last_dl_hash;

        if !dirty {
            return false;
        }
        self.last_dl_hash = dl_hash;

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return false;
            }
            Err(e) => {
                eprintln!("wgpu surface error: {e}");
                return false;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.process_display_list(dl);
        self.submit_render_pass(&view);
        frame.present();
        true
    }

    /// Process display list into GPU-ready data (rect instances + text rasterization).
    /// Only called when the display list has changed.
    fn process_display_list(&mut self, dl: &DisplayList) {
        self.last_clear_color = dl.clear_color;
        self.rect_instances.clear();
        self.glyph_instances.clear();
        let mut has_text = false;

        // Clear text/path overlay
        self.text_pixels.fill(0);

        for cmd in &dl.commands {
            match cmd {
                DrawCmd::Clear(_) => {} // handled by render pass clear
                DrawCmd::FillRect { x, y, w, h, color } => {
                    self.rect_instances.push(RectInstance::solid([*x, *y], [*w, *h], *color));
                }
                DrawCmd::Line { x1, y1, x2, y2, color, width } => {
                    let dx = x2 - x1;
                    let dy = y2 - y1;
                    if (dx * dx + dy * dy) < 0.01 { continue; }
                    let half = width / 2.0;
                    if dy.abs() < dx.abs() {
                        let min_x = x1.min(*x2);
                        let min_y = y1.min(*y2) - half;
                        self.rect_instances.push(RectInstance::solid([min_x, min_y], [dx.abs(), width.max(1.0)], *color));
                    } else {
                        let min_x = x1.min(*x2) - half;
                        let min_y = y1.min(*y2);
                        self.rect_instances.push(RectInstance::solid([min_x, min_y], [width.max(1.0), dy.abs()], *color));
                    }
                }
                DrawCmd::Text { text, x, y, size, color } => {
                    has_text = true;
                    self.shape_text_to_glyphs(text, *x, *y, *size, *color);
                }
                DrawCmd::FillEllipse { cx, cy, rx, ry, color } => {
                    // Single quad — GPU SDF does the ellipse shape
                    self.rect_instances.push(RectInstance::ellipse(
                        [cx - rx, cy - ry], [rx * 2.0, ry * 2.0], *color,
                    ));
                }
                DrawCmd::StrokeEllipse { cx, cy, rx, ry, color, width } => {
                    // Single quad — GPU SDF does the stroke
                    let half = width / 2.0;
                    let outer_rx = rx + half;
                    let outer_ry = ry + half;
                    let stroke_norm = width / outer_rx.max(outer_ry); // normalized stroke width
                    self.rect_instances.push(RectInstance::stroke_ellipse(
                        [cx - outer_rx, cy - outer_ry], [outer_rx * 2.0, outer_ry * 2.0], *color, stroke_norm,
                    ));
                }
                DrawCmd::FillPath { edges, color, bounds } => {
                    has_text = true; // reuse text overlay for path rasterization
                    self.rasterize_path(edges, *color, *bounds);
                }
                DrawCmd::Image { .. } => {
                    // TODO: positioned textured quad
                }
                DrawCmd::LinearGradient { x, y, w, h, color_start, color_end, vertical } => {
                    // Single quad — GPU shader interpolates colors
                    self.rect_instances.push(RectInstance::gradient(
                        [*x, *y], [*w, *h], *color_start, *color_end, *vertical,
                    ));
                }
            }
        }

        // Upload rect instances — reuse GPU buffer when possible
        if !self.rect_instances.is_empty() {
            let byte_size = self.rect_instances.len() * std::mem::size_of::<RectInstance>();
            if byte_size > self.rect_gpu_buf_capacity {
                let new_cap = byte_size.next_power_of_two().max(4096);
                self.rect_gpu_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("rect_instances"),
                    size: new_cap as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.rect_gpu_buf_capacity = new_cap;
            }
            if let Some(ref buf) = self.rect_gpu_buf {
                self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&self.rect_instances));
            }
        }

        // Upload glyph atlas (only dirty rows)
        if self.glyph_atlas.dirty {
            let min_y = self.glyph_atlas.dirty_min_y;
            let max_y = self.glyph_atlas.dirty_max_y.min(self.glyph_atlas.height);
            if max_y > min_y {
                let atlas_w = self.glyph_atlas.width;
                let row_bytes = atlas_w as usize * 4;
                let offset = min_y as usize * row_bytes;
                let size = (max_y - min_y) as usize * row_bytes;
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.glyph_atlas_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: min_y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &self.glyph_atlas.pixels[offset..offset + size],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(atlas_w * 4),
                        rows_per_image: Some(max_y - min_y),
                    },
                    wgpu::Extent3d {
                        width: atlas_w,
                        height: max_y - min_y,
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.glyph_atlas.clear_dirty();
        }

        // Upload glyph instances
        if !self.glyph_instances.is_empty() {
            let byte_size = self.glyph_instances.len() * std::mem::size_of::<GlyphInstance>();
            if byte_size > self.glyph_gpu_buf_capacity {
                let new_cap = byte_size.next_power_of_two().max(4096);
                self.glyph_gpu_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("glyph_instances"),
                    size: new_cap as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.glyph_gpu_buf_capacity = new_cap;
            }
            if let Some(ref buf) = self.glyph_gpu_buf {
                self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&self.glyph_instances));
            }
        }

        // Upload text/path overlay texture (full screen, only if paths were rasterized)
        if has_text {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.text_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.text_pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.width * 4),
                    rows_per_image: Some(self.height),
                },
                wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        self.last_rect_count = self.rect_instances.len() as u32;
        self.last_glyph_count = self.glyph_instances.len() as u32;
        self.last_has_text = has_text;
    }

    /// Submit the GPU render pass using cached state.
    fn submit_render_pass(&mut self, view: &wgpu::TextureView) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let cc = self.last_clear_color;
            let clear = wgpu::Color {
                r: cc[0] as f64, g: cc[1] as f64, b: cc[2] as f64, a: cc[3] as f64,
            };

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            if self.last_rect_count > 0 {
                if let Some(ref buf) = self.rect_gpu_buf {
                    pass.set_pipeline(&self.rect_pipeline);
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..4, 0..self.last_rect_count);
                }
            }

            // Draw vector path overlay (CPU-rasterized paths via text overlay texture)
            if self.last_has_text {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &self.text_bind_group, &[]);
                pass.draw(0..4, 0..1);
            }

            // Draw glyphs (atlas-based text)
            if self.last_glyph_count > 0 {
                if let Some(ref buf) = self.glyph_gpu_buf {
                    pass.set_pipeline(&self.glyph_pipeline);
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    pass.set_bind_group(1, &self.glyph_atlas_bind_group, &[]);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..4, 0..self.last_glyph_count);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Shape text and emit glyph instances using the atlas + shaping cache.
    fn shape_text_to_glyphs(&mut self, text: &str, x: f32, y: f32, font_size: f32, color: [f32; 4]) {
        // Hash the text command for cache lookup (position-independent — color applied later)
        let mut h: u64 = 0xcbf29ce484222325;
        let p: u64 = 0x100000001b3;
        for b in text.bytes() { h ^= b as u64; h = h.wrapping_mul(p); }
        h ^= font_size.to_bits() as u64; h = h.wrapping_mul(p);
        h ^= x.to_bits() as u64; h = h.wrapping_mul(p);
        h ^= y.to_bits() as u64; h = h.wrapping_mul(p);
        h ^= self.width as u64; h = h.wrapping_mul(p);

        if let Some(cached_instances) = self.text_shape_cache.get(&h) {
            // Reuse cached glyph positions, just update color
            for gi in cached_instances {
                let mut g = *gi;
                g.color = color;
                self.glyph_instances.push(g);
            }
            return;
        }

        // Shape from scratch
        let line_height = font_size * 1.4;
        let metrics = Metrics::new(font_size, line_height);
        let max_width = (self.width as f32 - x).max(1.0);

        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut new_instances = Vec::new();

        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0., 0.), 1.0);

                if let Some(cached) = self.glyph_atlas.get_or_insert(
                    physical.cache_key,
                    &mut self.font_system,
                    &mut self.swash_cache,
                ) {
                    if cached.width == 0 || cached.height == 0 {
                        continue;
                    }

                    let gx = x + physical.x as f32 + cached.offset_x as f32;
                    let gy = y + run.line_y + physical.y as f32 - cached.offset_y as f32;

                    let gi = GlyphInstance {
                        pos: [gx, gy],
                        size: [cached.width as f32, cached.height as f32],
                        uv_rect: [
                            cached.atlas_x as f32,
                            cached.atlas_y as f32,
                            cached.width as f32,
                            cached.height as f32,
                        ],
                        color,
                    };
                    new_instances.push(gi);
                    self.glyph_instances.push(gi);
                }
            }
        }

        self.text_shape_cache.insert(h, new_instances);
    }

    /// Rasterize a filled vector path onto the text overlay pixmap.
    fn rasterize_path(&mut self, edges: &[crate::display_list::PathEdge], color: [f32; 4], bounds: [f32; 4]) {
        use tiny_skia::{Paint, PathBuilder, FillRule, Transform, Pixmap};

        let bx = bounds[0].max(0.0) as i32;
        let by = bounds[1].max(0.0) as i32;
        let bw = bounds[2].ceil() as u32 + 2;
        let bh = bounds[3].ceil() as u32 + 2;

        if bw < 1 || bh < 1 || bw > 4096 || bh > 4096 { return; }

        // Build tiny-skia path
        let mut pb = PathBuilder::new();
        for edge in edges {
            match edge {
                crate::display_list::PathEdge::MoveTo(x, y) => {
                    pb.move_to(*x - bx as f32, *y - by as f32);
                }
                crate::display_list::PathEdge::LineTo(x, y) => {
                    pb.line_to(*x - bx as f32, *y - by as f32);
                }
                crate::display_list::PathEdge::CurveTo { cx, cy, ax, ay } => {
                    pb.quad_to(*cx - bx as f32, *cy - by as f32, *ax - bx as f32, *ay - by as f32);
                }
            }
        }
        pb.close();

        let path = match pb.finish() {
            Some(p) => p,
            None => return,
        };

        // Rasterize to a small pixmap
        if let Some(mut pixmap) = Pixmap::new(bw, bh) {
            let mut paint = Paint::default();
            paint.set_color_rgba8(
                (color[0] * 255.0) as u8,
                (color[1] * 255.0) as u8,
                (color[2] * 255.0) as u8,
                (color[3] * 255.0) as u8,
            );
            paint.anti_alias = true;

            pixmap.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);

            // Composite onto text overlay buffer
            let pw = self.width as usize;
            let ph = self.height as usize;
            let src = pixmap.data();
            for row in 0..bh as usize {
                let dy = by as usize + row;
                if dy >= ph { break; }
                for col in 0..bw as usize {
                    let dx = bx as usize + col;
                    if dx >= pw { break; }
                    let si = (row * bw as usize + col) * 4;
                    let di = (dy * pw + dx) * 4;
                    if si + 3 < src.len() && di + 3 < self.text_pixels.len() {
                        let sa = src[si + 3] as f32 / 255.0;
                        if sa > 0.0 {
                            let inv = 1.0 - sa;
                            self.text_pixels[di] = (src[si] as f32 + self.text_pixels[di] as f32 * inv) as u8;
                            self.text_pixels[di+1] = (src[si+1] as f32 + self.text_pixels[di+1] as f32 * inv) as u8;
                            self.text_pixels[di+2] = (src[si+2] as f32 + self.text_pixels[di+2] as f32 * inv) as u8;
                            self.text_pixels[di+3] = ((sa * 255.0) + self.text_pixels[di+3] as f32 * inv).min(255.0) as u8;
                        }
                    }
                }
            }
        }
    }

    /// Rasterize text to the overlay buffer. Returns (x, y, w, h) bounding box.
    fn rasterize_text(&mut self, text: &str, x: f32, y: f32, font_size: f32, color: [f32; 4]) -> (u32, u32, u32, u32) {
        let line_height = font_size * 1.4;
        let metrics = Metrics::new(font_size, line_height);
        let max_width = (self.width as f32 - x).max(1.0);

        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        // Compute bounding box from layout runs
        let mut max_w = 0.0f32;
        let mut max_h = 0.0f32;
        for run in buffer.layout_runs() {
            max_w = max_w.max(run.line_w);
            max_h = max_h.max(run.line_y + line_height);
        }

        let bx = (x as u32).min(self.width);
        let by = (y as u32).min(self.height);
        let bw = (max_w.ceil() as u32 + 2).min(self.width - bx);
        let bh = (max_h.ceil() as u32 + 2).min(self.height - by);

        // Clear only the bounding box region
        let pw = self.width as usize;
        for row in by..(by + bh).min(self.height) {
            let start = (row as usize * pw + bx as usize) * 4;
            let end = start + bw as usize * 4;
            if end <= self.text_pixels.len() {
                self.text_pixels[start..end].fill(0);
            }
        }

        let text_color = CosmicColor::rgba(
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
            (color[3] * 255.0) as u8,
        );

        let pw_i = self.width as i32;
        let ph_i = self.height as i32;
        let pixels = &mut self.text_pixels;

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            text_color,
            |cx, cy, w, h, buf_color| {
                let px = x as i32 + cx;
                let py = y as i32 + cy;
                for dy in 0..h as i32 {
                    for dx in 0..w as i32 {
                        let fx = px + dx;
                        let fy = py + dy;
                        if fx >= 0 && fy >= 0 && fx < pw_i && fy < ph_i {
                            let alpha = buf_color.a();
                            if alpha > 0 {
                                let idx = (fy as usize * pw_i as usize + fx as usize) * 4;
                                if idx + 3 < pixels.len() {
                                    let a = alpha as f32 / 255.0;
                                    let inv_a = 1.0 - a;
                                    pixels[idx] = (buf_color.r() as f32 * a + pixels[idx] as f32 * inv_a) as u8;
                                    pixels[idx + 1] = (buf_color.g() as f32 * a + pixels[idx + 1] as f32 * inv_a) as u8;
                                    pixels[idx + 2] = (buf_color.b() as f32 * a + pixels[idx + 2] as f32 * inv_a) as u8;
                                    pixels[idx + 3] = (alpha as f32 + pixels[idx + 3] as f32 * inv_a).min(255.0) as u8;
                                }
                            }
                        }
                    }
                }
            },
        );

        (bx, by, bw, bh)
    }

    fn create_text_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text_overlay"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn create_text_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        texture: &wgpu::Texture,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
