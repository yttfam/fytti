use std::collections::VecDeque;

use fytti_render::display_list::{color_to_f32, DisplayList, DrawCmd};
use wytti_host::{Color, HostBackend, InputEvent, Rect, ResourceId};


/// Fytti's implementation of the Wytti HostBackend trait.
/// Collects draw commands into a DisplayList for GPU rendering.
pub struct FyttiBackend {
    pub display_list: DisplayList,
    pub events: VecDeque<InputEvent>,
    pub title: String,
    pub frame_requested: bool,
    pub presented: bool,
    width: u32,
    height: u32,
    last_mouse: (f32, f32),
    mouse_moved: bool,
}

impl FyttiBackend {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            display_list: DisplayList::new(width, height),
            events: VecDeque::new(),
            title: String::from("Fytti"),
            frame_requested: false,
            presented: false,
            width,
            height,
            last_mouse: (0.0, 0.0),
            mouse_moved: false,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    pub fn push_event(&mut self, event: InputEvent) {
        // Track mouse position for poll_mouse
        if let InputEvent::MouseMove { x, y } = &event {
            self.last_mouse = (*x, *y);
            self.mouse_moved = true;
        }
        self.events.push_back(event);
    }
}

fn wytti_color(c: Color) -> [f32; 4] {
    color_to_f32(c.r, c.g, c.b, c.a)
}

impl HostBackend for FyttiBackend {
    fn clear(&mut self, color: Color) {
        self.display_list.reset(self.width, self.height);
        self.display_list.clear_color = wytti_color(color);
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.display_list.commands.push(DrawCmd::FillRect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            color: wytti_color(color),
        });
    }

    fn stroke_rect(&mut self, rect: Rect, color: Color, width: f32) {
        let c = wytti_color(color);
        let r = rect;
        // Top
        self.display_list.commands.push(DrawCmd::FillRect { x: r.x, y: r.y, w: r.w, h: width, color: c });
        // Bottom
        self.display_list.commands.push(DrawCmd::FillRect { x: r.x, y: r.y + r.h - width, w: r.w, h: width, color: c });
        // Left
        self.display_list.commands.push(DrawCmd::FillRect { x: r.x, y: r.y, w: width, h: r.h, color: c });
        // Right
        self.display_list.commands.push(DrawCmd::FillRect { x: r.x + r.w - width, y: r.y, w: width, h: r.h, color: c });
    }

    fn draw_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
        self.display_list.commands.push(DrawCmd::Line {
            x1, y1, x2, y2,
            color: wytti_color(color),
            width,
        });
    }

    fn draw_text(&mut self, text: &str, x: f32, y: f32, size: f32, _font: ResourceId, color: Color) {
        self.display_list.commands.push(DrawCmd::Text {
            text: text.to_string(),
            x, y, size,
            color: wytti_color(color),
        });
    }

    fn gradient_rect(&mut self, rect: Rect, color1: Color, color2: Color, vertical: bool) {
        self.display_list.commands.push(DrawCmd::LinearGradient {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            color_start: wytti_color(color1),
            color_end: wytti_color(color2),
            vertical,
        });
    }

    fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
        self.display_list.commands.push(DrawCmd::FillEllipse {
            cx, cy, rx, ry,
            color: wytti_color(color),
        });
    }

    fn poll_mouse(&mut self) -> Option<(f32, f32)> {
        if self.mouse_moved {
            self.mouse_moved = false;
            Some(self.last_mouse)
        } else {
            None
        }
    }

    fn draw_image(&mut self, _image: ResourceId, _x: f32, _y: f32, _w: f32, _h: f32) {
        // TODO: positioned quad rendering
    }

    fn present(&mut self) {
        self.presented = true;
    }

    fn get_width(&self) -> u32 {
        self.width
    }

    fn get_height(&self) -> u32 {
        self.height
    }

    fn poll_event(&mut self) -> Option<InputEvent> {
        self.events.pop_front()
    }

    fn load_font(&mut self, _name: &str) -> ResourceId {
        ResourceId(1)
    }

    fn load_image(&mut self, _url: &str) -> ResourceId {
        ResourceId::INVALID
    }

    fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    fn request_frame(&mut self) {
        self.frame_requested = true;
    }

    fn clipboard_read(&mut self) -> Option<String> {
        None
    }

    fn clipboard_write(&mut self, _text: &str) {}
}
