/// A draw command collected during a frame.
#[derive(Debug, Clone)]
pub enum DrawCmd {
    Clear([f32; 4]),
    FillRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: [f32; 4],
        width: f32,
    },
    Text {
        text: String,
        x: f32,
        y: f32,
        size: f32,
        color: [f32; 4],
    },
    /// Draw an image. image_id references the texture cache.
    Image {
        image_id: u32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// Draw raw RGBA bitmap data at a position.
    BitmapRaw {
        data: Vec<u8>,
        src_width: u32,
        src_height: u32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// Fill an ellipse (circle if rx == ry).
    FillEllipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        color: [f32; 4],
    },
    /// Stroke an ellipse outline.
    StrokeEllipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        color: [f32; 4],
        width: f32,
    },
    /// Filled vector path (CPU-rasterized, uploaded as texture).
    FillPath {
        /// Path edges: (type, x1, y1, x2, y2) — type: 0=moveto, 1=lineto, 2=curveto(cx,cy,ax,ay)
        edges: Vec<PathEdge>,
        color: [f32; 4],
        bounds: [f32; 4], // x, y, w, h
    },
    /// Fill a rect with a linear gradient.
    LinearGradient {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color_start: [f32; 4],
        color_end: [f32; 4],
        /// 0.0 = horizontal (left→right), 1.0 = vertical (top→bottom)
        vertical: bool,
    },
}

/// Collected draw commands for one frame.
#[derive(Debug, Clone)]
pub struct DisplayList {
    pub clear_color: [f32; 4],
    pub commands: Vec<DrawCmd>,
    pub width: u32,
    pub height: u32,
}

impl DisplayList {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            clear_color: [0.0, 0.0, 0.0, 1.0],
            commands: Vec::with_capacity(512),
            width,
            height,
        }
    }

    pub fn reset(&mut self, width: u32, height: u32) {
        self.clear_color = [0.0, 0.0, 0.0, 1.0];
        self.commands.clear();
        self.width = width;
        self.height = height;
    }
}

impl DisplayList {
    /// Fast hash of the display list contents for dirty checking.
    /// Not cryptographic — just needs to detect changes between frames.
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
        let prime: u64 = 0x100000001b3;

        // Hash clear color
        for &c in &self.clear_color {
            h ^= c.to_bits() as u64;
            h = h.wrapping_mul(prime);
        }

        // Hash command count + types + key values
        h ^= self.commands.len() as u64;
        h = h.wrapping_mul(prime);

        for cmd in &self.commands {
            match cmd {
                DrawCmd::Clear(c) => {
                    h ^= 1;
                    for v in c { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
                DrawCmd::FillRect { x, y, w, h: rh, color } => {
                    h ^= 2;
                    h ^= x.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= w.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= rh.to_bits() as u64; h = h.wrapping_mul(prime);
                    for v in color { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
                DrawCmd::Line { x1, y1, x2, y2, color, width } => {
                    h ^= 3;
                    h ^= x1.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y1.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= x2.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y2.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= width.to_bits() as u64; h = h.wrapping_mul(prime);
                    for v in color { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
                DrawCmd::Text { x, y, size, .. } => {
                    h ^= 4;
                    h ^= x.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= size.to_bits() as u64; h = h.wrapping_mul(prime);
                    // Skip text content — position+size change is the common case
                }
                DrawCmd::FillEllipse { cx, cy, rx, ry, color } => {
                    h ^= 5;
                    h ^= cx.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= cy.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= rx.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= ry.to_bits() as u64; h = h.wrapping_mul(prime);
                    for v in color { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
                DrawCmd::StrokeEllipse { cx, cy, rx, ry, color, width } => {
                    h ^= 6;
                    h ^= cx.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= cy.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= rx.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= ry.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= width.to_bits() as u64; h = h.wrapping_mul(prime);
                    for v in color { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
                DrawCmd::Image { image_id, x, y, w, h: ih } => {
                    h ^= 7;
                    h ^= *image_id as u64; h = h.wrapping_mul(prime);
                    h ^= x.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= w.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= ih.to_bits() as u64; h = h.wrapping_mul(prime);
                }
                DrawCmd::BitmapRaw { src_width, src_height, x, y, w, h: bh, .. } => {
                    h ^= 10;
                    h ^= *src_width as u64; h = h.wrapping_mul(prime);
                    h ^= *src_height as u64; h = h.wrapping_mul(prime);
                    h ^= x.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= w.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= bh.to_bits() as u64; h = h.wrapping_mul(prime);
                }
                DrawCmd::FillPath { edges, color, bounds } => {
                    h ^= 9;
                    h ^= edges.len() as u64; h = h.wrapping_mul(prime);
                    for v in bounds { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                    for v in color { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
                DrawCmd::LinearGradient { x, y, w, h: gh, color_start, color_end, vertical } => {
                    h ^= 8;
                    h ^= x.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= y.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= w.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= gh.to_bits() as u64; h = h.wrapping_mul(prime);
                    h ^= *vertical as u64; h = h.wrapping_mul(prime);
                    for v in color_start { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                    for v in color_end { h ^= v.to_bits() as u64; h = h.wrapping_mul(prime); }
                }
            }
        }
        h
    }
}

/// A path edge for vector rendering.
#[derive(Debug, Clone, Copy)]
pub enum PathEdge {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// Quadratic bezier: control point, then anchor point.
    CurveTo { cx: f32, cy: f32, ax: f32, ay: f32 },
}

/// Helper: convert u8 RGBA to f32 RGBA.
pub fn color_to_f32(r: u8, g: u8, b: u8, a: u8) -> [f32; 4] {
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}
