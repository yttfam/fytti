use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

mod input;
mod registry;

use fytti_render::gpu::GpuRenderer;
use fytti_render::Renderer;

fn load_html(input: &str) -> String {
    if input.starts_with("http://") || input.starts_with("https://") {
        eprintln!("Fetching {input}...");
        reqwest::blocking::Client::builder()
            .user_agent("Fytti/0.1")
            .build()
            .expect("http client")
            .get(input)
            .send()
            .expect("fetch failed")
            .text()
            .expect("body decode failed")
    } else {
        std::fs::read_to_string(input)
            .unwrap_or_else(|e| panic!("Failed to read {input}: {e}"))
    }
}

/// Render HTML to a software renderer (headless).
fn render_html_headless(html: &str, width: u32, height: u32) -> Renderer {
    let doc = fytti_html::parse(html);
    let styles = fytti_css::resolve(&doc);

    let mut renderer = Renderer::new(width, height);

    let body = doc.body();
    let body_style = styles.get(&body).cloned().unwrap_or_default();
    renderer.clear(body_style.background_color);

    let layout = fytti_layout::layout(&doc, &styles, width as f32, height as f32, &mut renderer);
    renderer.paint(&layout, &doc, &styles);

    renderer
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut input = "test.html".to_string();
    let mut png_out: Option<String> = None;
    let mut width: u32 = 900;
    let mut height: u32 = 700;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--png" => {
                i += 1;
                png_out = Some(args.get(i).expect("--png requires output path").clone());
            }
            "--width" => {
                i += 1;
                width = args.get(i).expect("--width requires value").parse().expect("invalid width");
            }
            "--height" => {
                i += 1;
                height = args.get(i).expect("--height requires value").parse().expect("invalid height");
            }
            other => {
                input = other.to_string();
            }
        }
        i += 1;
    }

    // Headless PNG mode
    if let Some(ref out_path) = png_out {
        if input.ends_with(".wasm") {
            eprintln!("Headless WASM rendering not yet supported");
            std::process::exit(1);
        }
        if input.ends_with(".swf") {
            let data = std::fs::read(&input).expect("Failed to read SWF");
            let dl = fytti_swf::swf_to_display_list(&data, width, height)
                .expect("SWF parse failed");
            eprintln!("SWF: {} draw commands", dl.commands.len());
            // Render display list to software pixmap
            let mut renderer = Renderer::new(width, height);
            let bg = dl.clear_color;
            renderer.clear(fytti_css::Color::rgba(
                (bg[0] * 255.0) as u8, (bg[1] * 255.0) as u8,
                (bg[2] * 255.0) as u8, (bg[3] * 255.0) as u8,
            ));
            for cmd in &dl.commands {
                match cmd {
                    fytti_render::display_list::DrawCmd::FillRect { x, y, w, h, color } => {
                        renderer.fill_rect_direct(
                            fytti_layout::Rect { x: *x, y: *y, width: *w, height: *h },
                            fytti_css::Color::rgba(
                                (color[0] * 255.0) as u8, (color[1] * 255.0) as u8,
                                (color[2] * 255.0) as u8, (color[3] * 255.0) as u8,
                            ),
                        );
                    }
                    fytti_render::display_list::DrawCmd::FillEllipse { cx, cy, rx, ry, color } => {
                        renderer.fill_ellipse(*cx, *cy, *rx, *ry, fytti_css::Color::rgba(
                            (color[0] * 255.0) as u8, (color[1] * 255.0) as u8,
                            (color[2] * 255.0) as u8, (color[3] * 255.0) as u8,
                        ));
                    }
                    fytti_render::display_list::DrawCmd::Text { text, x, y, size, color } => {
                        renderer.draw_text_direct(text, *x, *y, *size, fytti_css::Color::rgba(
                            (color[0] * 255.0) as u8, (color[1] * 255.0) as u8,
                            (color[2] * 255.0) as u8, (color[3] * 255.0) as u8,
                        ));
                    }
                    fytti_render::display_list::DrawCmd::FillPath { edges, color, .. } => {
                        renderer.fill_path_direct(edges, fytti_css::Color::rgba(
                            (color[0] * 255.0) as u8, (color[1] * 255.0) as u8,
                            (color[2] * 255.0) as u8, (color[3] * 255.0) as u8,
                        ));
                    }
                    _ => {}
                }
            }
            renderer.save_png(out_path).expect("save PNG failed");
            eprintln!("Saved {width}x{height} → {out_path}");
            return;
        }
        let html = load_html(&input);
        let renderer = render_html_headless(&html, width, height);
        renderer.save_png(out_path).expect("save PNG failed");
        eprintln!("Saved {width}x{height} → {out_path}");
        return;
    }

    // Window mode
    let app_name = input.clone();
    let mode = if input.ends_with(".wasm") {
        Mode::Wasm(input)
    } else if input.ends_with(".swf") {
        let data = std::fs::read(&input)
            .unwrap_or_else(|e| panic!("Failed to read {input}: {e}"));
        Mode::Swf(data)
    } else {
        Mode::Html(load_html(&input))
    };

    // Announce to Hermytt registry (fire-and-forget, fails silently if Hermytt isn't running)
    let token = std::env::var("HERMYTT_KEY").unwrap_or_default();
    let _registry = registry::start(&token, &[app_name]);

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = App {
        mode,
        window: None,
        surface: None,
        sw_renderer: None,
        gpu: None,
        wasm_app: None,
        swf_player: None,
        fps: FpsCounter::new(),
        win_width: width,
        win_height: height,
    };
    event_loop.run_app(&mut app).expect("event loop failed");
}

enum Mode {
    Html(String),
    Wasm(String),
    Swf(Vec<u8>),
}

struct FpsCounter {
    frame_count: u32,
    last_tick: Instant,
    current_fps: f64,
}

impl FpsCounter {
    fn new() -> Self {
        Self { frame_count: 0, last_tick: Instant::now(), current_fps: 0.0 }
    }
    fn tick(&mut self) {
        self.frame_count += 1;
        let elapsed = self.last_tick.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            self.current_fps = self.frame_count as f64 / elapsed;
            self.frame_count = 0;
            self.last_tick = Instant::now();
        }
    }
    fn fps_string(&self) -> String {
        format!("{:.0} fps", self.current_fps)
    }
}

struct App {
    mode: Mode,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    sw_renderer: Option<Renderer>,
    gpu: Option<GpuRenderer>,
    wasm_app: Option<fytti_wasm::WasmApp>,
    swf_player: Option<fytti_swf::render::SwfPlayer>,
    fps: FpsCounter,
    win_width: u32,
    win_height: u32,
}

impl App {
    fn render_html(&mut self, html: &str) {
        let window = match self.window.as_ref() {
            Some(w) => w,
            None => return,
        };

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let doc = fytti_html::parse(html);
        let styles = fytti_css::resolve(&doc);

        let renderer = self.sw_renderer.get_or_insert_with(|| Renderer::new(width, height));
        renderer.resize(width, height);

        let body = doc.body();
        let body_style = styles.get(&body).cloned().unwrap_or_default();
        renderer.clear(body_style.background_color);

        let layout = fytti_layout::layout(&doc, &styles, width as f32, height as f32, renderer);
        renderer.paint(&layout, &doc, &styles);

        if let Some(surface) = self.surface.as_mut() {
            surface
                .resize(NonZeroU32::new(width).unwrap(), NonZeroU32::new(height).unwrap())
                .expect("resize failed");
            let mut buffer = surface.buffer_mut().expect("buffer_mut failed");
            let pixels = renderer.pixels_as_u32();
            buffer[..pixels.len()].copy_from_slice(pixels);
            buffer.present().expect("present failed");
        }
    }

    fn render_swf_frame(&mut self) {
        let player = match self.swf_player.as_mut() {
            Some(p) => p,
            None => return,
        };

        let size = self.window.as_ref().unwrap().inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let dl = player.render(width, height);
        player.advance_frame(); // advance timeline

        let gpu = match self.gpu.as_mut() {
            Some(g) => g,
            None => return,
        };
        gpu.resize(width, height);
        gpu.render(&dl);

        self.fps.tick();
        let fps_text = self.fps.fps_string();
        if let Some(w) = self.window.as_ref() {
            w.set_title(&format!("Fytti SWF — {fps_text}"));
        }

        // Keep animating if multi-frame
        if player.swf.header.frame_count > 1 {
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }
    }

    fn render_wasm_frame(&mut self) {
        let wasm = match self.wasm_app.as_mut() {
            Some(w) => w,
            None => return,
        };

        let size = self.window.as_ref().unwrap().inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        wasm.resize(width, height);

        match wasm.run_frame() {
            Ok(_) => {}
            Err(e) => {
                eprintln!("WASM frame error: {e}");
            }
        };

        let wants_more = wasm.frame_requested();

        self.fps.tick();
        let fps_text = self.fps.fps_string();

        let gpu = match self.gpu.as_mut() {
            Some(g) => g,
            None => return,
        };
        gpu.resize(width, height);
        let rendered = gpu.render(wasm.display_list());

        let base_title = wasm.title().to_string();
        if let Some(w) = self.window.as_ref() {
            w.set_title(&format!("{base_title} — {fps_text}"));
        }

        // Only keep spinning if the scene is actually changing
        if wants_more && rendered {
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("Fytti")
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.win_width as f64,
                self.win_height as f64,
            ));

        let window = Arc::new(event_loop.create_window(attrs).expect("create window failed"));
        let size = window.inner_size();

        match &self.mode {
            Mode::Html(_) => {
                let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
                let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
                self.surface = Some(surface);
            }
            Mode::Wasm(path) => {
                let gpu = pollster::block_on(GpuRenderer::new(window.clone()));
                self.gpu = Some(gpu);

                let runner = fytti_wasm::WasmRunner::new().expect("WASM runner failed");
                let wasm_app = runner
                    .load(path, size.width.max(1), size.height.max(1))
                    .expect("WASM load failed");
                self.wasm_app = Some(wasm_app);
            }
            Mode::Swf(ref data) => {
                let gpu = pollster::block_on(GpuRenderer::new(window.clone()));
                self.gpu = Some(gpu);

                let swf = fytti_swf::parse_swf(data).expect("SWF parse failed");
                eprintln!(
                    "SWF: {}x{} @ {}fps, {} frames",
                    swf.header.frame_width, swf.header.frame_height,
                    swf.header.frame_rate, swf.header.frame_count,
                );
                self.swf_player = Some(fytti_swf::render::SwfPlayer::new(swf));
            }
        }

        self.window = Some(window.clone());
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Forward input events to WASM guest + wake the render loop
        if let Some(ref mut wasm) = self.wasm_app {
            if let Some(input_event) = input::convert_event(&event) {
                wasm.push_event(input_event);
                // Wake up — guest might need to handle this event
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => match &self.mode {
                Mode::Html(html) => {
                    let html = html.clone();
                    self.render_html(&html);
                }
                Mode::Wasm(_) => self.render_wasm_frame(),
                Mode::Swf(_) => self.render_swf_frame(),
            },
            WindowEvent::Resized(_) => {
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}
