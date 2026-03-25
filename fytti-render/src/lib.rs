pub mod display_list;
pub mod glyph_atlas;
pub mod gpu;

use cosmic_text::{Attrs, Buffer, Color as CosmicColor, FontSystem, Metrics, Shaping, SwashCache};
use fytti_css::{Color, ComputedStyle, StyleMap};
use fytti_html::Document;
use fytti_layout::{BoxType, LayoutBox, Rect, TextMeasure};
use tiny_skia::{Paint, PathBuilder, Pixmap, Transform};

pub struct Renderer {
    pub pixmap: Pixmap,
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Reusable buffer for pixel conversion (avoids per-frame allocation)
    pixel_buf: Vec<u32>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        let pixmap = Pixmap::new(width, height).expect("failed to create pixmap");
        let pixel_buf = vec![0u32; (width * height) as usize];
        Renderer {
            pixmap,
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            pixel_buf,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if w != self.pixmap.width() || h != self.pixmap.height() {
            self.pixmap = Pixmap::new(w, h).expect("failed to create pixmap");
            self.pixel_buf.resize((w * h) as usize, 0);
        }
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Clear with a background color
    pub fn clear(&mut self, color: Color) {
        self.pixmap
            .fill(tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a));
    }

    /// Paint the full layout tree
    pub fn paint(
        &mut self,
        layout: &LayoutBox,
        doc: &Document,
        styles: &StyleMap,
    ) {
        self.paint_box(layout, doc, styles);
    }

    fn paint_box(
        &mut self,
        layout_box: &LayoutBox,
        doc: &Document,
        styles: &StyleMap,
    ) {
        // Paint background
        if let Some(node) = layout_box.node {
            let style = styles.get(&node).cloned().unwrap_or_default();
            if style.background_color.a > 0 {
                self.fill_rect(layout_box.rect, style.background_color);
            }
        }

        // Paint text
        if let BoxType::Text(ref text) = layout_box.box_type {
            // Find the style from the nearest element ancestor
            let style = self.find_text_style(layout_box, doc, styles);
            self.draw_text(
                text,
                layout_box.rect.x,
                layout_box.rect.y,
                style.font_size,
                style.color,
                layout_box.rect.width,
            );
        }

        // Paint children
        for child in &layout_box.children {
            self.paint_box(child, doc, styles);
        }
    }

    fn find_text_style(
        &self,
        layout_box: &LayoutBox,
        _doc: &Document,
        styles: &StyleMap,
    ) -> ComputedStyle {
        // Text boxes don't have a node; look at parent chain
        // For now, check if the text box has a node or walk up
        if let Some(node) = layout_box.node {
            return styles.get(&node).cloned().unwrap_or_default();
        }
        // Default: inherited from wherever
        ComputedStyle::default()
    }

    /// Direct fill_rect for WASM host backend
    pub fn fill_rect_direct(&mut self, rect: Rect, color: Color) {
        self.fill_rect(rect, color);
    }

    /// Direct draw_line for WASM host backend
    pub fn draw_line_direct(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;

        let mut pb = PathBuilder::new();
        pb.move_to(x1, y1);
        pb.line_to(x2, y2);
        if let Some(path) = pb.finish() {
            let stroke = tiny_skia::Stroke {
                width,
                ..Default::default()
            };
            self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// Blit raw RGBA bitmap data onto the pixmap with nearest-neighbor scaling.
    pub fn blit_bitmap_direct(&mut self, data: &[u8], src_w: u32, src_h: u32, x: f32, y: f32, w: f32, h: f32) {
        let dx = x.max(0.0) as usize;
        let dy = y.max(0.0) as usize;
        let dw = w as usize;
        let dh = h as usize;
        let pw = self.pixmap.width() as usize;
        let ph = self.pixmap.height() as usize;
        let pixels = self.pixmap.data_mut();

        for row in 0..dh {
            let fy = dy + row;
            if fy >= ph { break; }
            let src_y = (row * src_h as usize / dh.max(1)).min(src_h as usize - 1);
            for col in 0..dw {
                let fx = dx + col;
                if fx >= pw { break; }
                let src_x = (col * src_w as usize / dw.max(1)).min(src_w as usize - 1);
                let si = (src_y * src_w as usize + src_x) * 4;
                let di = (fy * pw + fx) * 4;
                if si + 3 < data.len() && di + 3 < pixels.len() {
                    let sa = data[si + 3] as f32 / 255.0;
                    if sa > 0.0 {
                        let inv = 1.0 - sa;
                        pixels[di] = (data[si] as f32 * sa + pixels[di] as f32 * inv) as u8;
                        pixels[di+1] = (data[si+1] as f32 * sa + pixels[di+1] as f32 * inv) as u8;
                        pixels[di+2] = (data[si+2] as f32 * sa + pixels[di+2] as f32 * inv) as u8;
                        pixels[di+3] = 255;
                    }
                }
            }
        }
    }

    /// Render a filled vector path directly onto the pixmap.
    pub fn fill_path_direct(&mut self, edges: &[crate::display_list::PathEdge], color: Color) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;

        let mut pb = PathBuilder::new();
        for edge in edges {
            match edge {
                crate::display_list::PathEdge::MoveTo(x, y) => pb.move_to(*x, *y),
                crate::display_list::PathEdge::LineTo(x, y) => pb.line_to(*x, *y),
                crate::display_list::PathEdge::CurveTo { cx, cy, ax, ay } => pb.quad_to(*cx, *cy, *ax, *ay),
            }
        }
        pb.close();

        if let Some(path) = pb.finish() {
            self.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
        }
    }

    /// Direct draw_text for WASM host backend
    pub fn draw_text_direct(&mut self, text: &str, x: f32, y: f32, font_size: f32, color: Color) {
        self.draw_text(text, x, y, font_size, color, self.pixmap.width() as f32);
    }

    /// Draw a filled ellipse (or circle if rx == ry).
    pub fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;

        // Approximate ellipse with 4 cubic beziers
        // Magic number: kappa = 4 * (sqrt(2) - 1) / 3 ≈ 0.5522847498
        let k = 0.5522848;
        let kx = rx * k;
        let ky = ry * k;

        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy - ry);
        pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
        pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
        pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
        pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
        pb.close();

        if let Some(path) = pb.finish() {
            self.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Stroke an ellipse outline.
    pub fn stroke_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: Color, width: f32) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;

        let k = 0.5522848;
        let kx = rx * k;
        let ky = ry * k;

        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy - ry);
        pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
        pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
        pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
        pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
        pb.close();

        if let Some(path) = pb.finish() {
            let stroke = tiny_skia::Stroke { width, ..Default::default() };
            self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// Draw a quadratic bezier curve.
    pub fn stroke_quad(&mut self, x0: f32, y0: f32, cx: f32, cy: f32, x1: f32, y1: f32, color: Color, width: f32) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;

        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.quad_to(cx, cy, x1, y1);

        if let Some(path) = pb.finish() {
            let stroke = tiny_skia::Stroke { width, ..Default::default() };
            self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// Draw a cubic bezier curve.
    pub fn stroke_cubic(&mut self, x0: f32, y0: f32, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x1: f32, y1: f32, color: Color, width: f32) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;

        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.cubic_to(cx1, cy1, cx2, cy2, x1, y1);

        if let Some(path) = pb.finish() {
            let stroke = tiny_skia::Stroke { width, ..Default::default() };
            self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = false;

        let r = match tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
            Some(r) => r,
            None => return,
        };
        let path = PathBuilder::from_rect(r);
        self.pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font_size: f32,
        color: Color,
        max_width: f32,
    ) {
        let line_height = font_size * 1.4;
        let metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let text_color = CosmicColor::rgba(color.r, color.g, color.b, color.a);

        buffer.draw(&mut self.font_system, &mut self.swash_cache, text_color, |cx, cy, w, h, buf_color| {
            let px = x as i32 + cx;
            let py = y as i32 + cy;

            let pm_w = self.pixmap.width() as i32;
            let pm_h = self.pixmap.height() as i32;

            // Draw each pixel of the glyph
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    let fx = px + dx;
                    let fy = py + dy;
                    if fx >= 0 && fy >= 0 && fx < pm_w && fy < pm_h {
                        let alpha = buf_color.a();
                        if alpha > 0 {
                            let idx = (fy as usize * pm_w as usize + fx as usize) * 4;
                            let pixels = self.pixmap.data_mut();
                            if idx + 3 < pixels.len() {
                                // Simple alpha blend
                                let a = alpha as f32 / 255.0;
                                let inv_a = 1.0 - a;
                                pixels[idx] = (buf_color.r() as f32 * a + pixels[idx] as f32 * inv_a) as u8;
                                pixels[idx + 1] = (buf_color.g() as f32 * a + pixels[idx + 1] as f32 * inv_a) as u8;
                                pixels[idx + 2] = (buf_color.b() as f32 * a + pixels[idx + 2] as f32 * inv_a) as u8;
                                pixels[idx + 3] = 255;
                            }
                        }
                    }
                }
            }
        });
    }

    /// Get pixels as RGBA bytes
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Get pixels as 0x00RRGGBB u32 slice for softbuffer (zero-alloc after first frame)
    pub fn pixels_as_u32(&mut self) -> &[u32] {
        let data = self.pixmap.data();
        for (i, px) in data.chunks_exact(4).enumerate() {
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            self.pixel_buf[i] = (r << 16) | (g << 8) | b;
        }
        &self.pixel_buf
    }

    /// Save the current pixmap as a PNG file.
    pub fn save_png(&self, path: &str) -> Result<(), String> {
        self.pixmap
            .save_png(path)
            .map_err(|e| format!("Failed to save PNG: {e}"))
    }
}

// ── TextMeasure impl ──

impl TextMeasure for Renderer {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> (f32, f32) {
        let line_height = font_size * 1.4;
        let metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut total_height = 0.0f32;
        let mut max_w = 0.0f32;
        for run in buffer.layout_runs() {
            max_w = max_w.max(run.line_w);
            total_height = total_height.max(run.line_y + line_height);
        }

        if total_height == 0.0 {
            total_height = line_height;
        }

        (max_w, total_height)
    }
}
