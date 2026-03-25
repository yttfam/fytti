/// SWF file header.
#[derive(Debug, Clone)]
pub struct SwfHeader {
    pub version: u8,
    pub file_length: u32,
    pub frame_width: f32,  // in pixels (twips / 20)
    pub frame_height: f32,
    pub frame_rate: f32,
    pub frame_count: u16,
}

/// A parsed SWF file.
#[derive(Debug)]
pub struct Swf {
    pub header: SwfHeader,
    pub tags: Vec<Tag>,
}

/// SWF tag types we care about.
#[derive(Debug, Clone)]
pub enum Tag {
    SetBackgroundColor(Color),
    DefineShape(DefineShape),
    DefineBitmap(DefineBitmap),
    DefineSprite(DefineSprite),
    PlaceObject(PlaceObject),
    RemoveObject { depth: u16 },
    ShowFrame,
    DefineText(DefineText),
    End,
    Unknown { tag_code: u16, length: usize },
}

/// An embedded bitmap image.
#[derive(Debug, Clone)]
pub struct DefineBitmap {
    pub id: u16,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // decoded RGBA pixels
}

/// DefineSprite — a MovieClip with its own nested timeline.
#[derive(Debug, Clone)]
pub struct DefineSprite {
    pub id: u16,
    pub frame_count: u16,
    pub tags: Vec<Tag>,
}

/// RGBA color.
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub fn to_f32(&self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

/// A 2D affine transform matrix.
#[derive(Debug, Clone, Copy)]
pub struct Matrix {
    pub a: f32,  // scale x
    pub b: f32,  // rotate skew 1
    pub c: f32,  // rotate skew 0
    pub d: f32,  // scale y
    pub tx: f32, // translate x (pixels)
    pub ty: f32, // translate y (pixels)
}

impl Default for Matrix {
    fn default() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }
}

impl Matrix {
    pub fn transform(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }
}

/// Fill style for a shape.
#[derive(Debug, Clone)]
pub enum FillStyle {
    Solid(Color),
    LinearGradient {
        matrix: Matrix,
        colors: Vec<(u8, Color)>,
    },
    RadialGradient {
        matrix: Matrix,
        colors: Vec<(u8, Color)>,
    },
    Bitmap {
        character_id: u16,
        matrix: Matrix,
        repeating: bool,
        smoothed: bool,
    },
}

/// Line style for a shape.
#[derive(Debug, Clone)]
pub struct LineStyle {
    pub width: f32, // in pixels (twips / 20)
    pub color: Color,
}

/// A shape edge record.
#[derive(Debug, Clone)]
pub enum ShapeEdge {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CurveTo { cx: f32, cy: f32, ax: f32, ay: f32 }, // quadratic bezier
}

/// A shape path — a sequence of edges with a fill and/or line style.
#[derive(Debug, Clone)]
pub struct ShapePath {
    pub fill: Option<usize>,  // index into fill_styles
    pub line: Option<usize>,  // index into line_styles
    pub edges: Vec<ShapeEdge>,
}

/// DefineShape tag — defines a reusable shape character.
#[derive(Debug, Clone)]
pub struct DefineShape {
    pub id: u16,
    pub bounds: Rect,
    pub fill_styles: Vec<FillStyle>,
    pub line_styles: Vec<LineStyle>,
    pub paths: Vec<ShapePath>,
}

/// PlaceObject tag — places/modifies a character on the display list.
#[derive(Debug, Clone)]
pub struct PlaceObject {
    pub depth: u16,
    pub character_id: Option<u16>,
    pub matrix: Option<Matrix>,
    pub is_move: bool,
}

/// DefineText tag.
#[derive(Debug, Clone)]
pub struct DefineText {
    pub id: u16,
    pub bounds: Rect,
    pub text: String, // simplified — real SWF text is glyph-based
    pub color: Color,
    pub size: f32,
    pub x: f32,
    pub y: f32,
}

/// Rectangle.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

impl Rect {
    pub fn width(&self) -> f32 { self.x_max - self.x_min }
    pub fn height(&self) -> f32 { self.y_max - self.y_min }
}
