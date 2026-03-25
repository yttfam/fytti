use fytti_css::{ComputedStyle, Display, LengthOrAuto, StyleMap};
use fytti_html::{Document, NodeData, NodeId};

// ── Layout Types ──

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub rect: Rect,
    pub node: Option<NodeId>,
    pub box_type: BoxType,
    pub children: Vec<LayoutBox>,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub enum BoxType {
    Block,
    Text(String),
    Anonymous,
}

/// Trait for measuring text dimensions
pub trait TextMeasure {
    /// Measure text, returns (width, height) given max_width and font_size
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> (f32, f32);
}

// ── Layout Algorithm ──

pub fn layout(
    doc: &Document,
    styles: &StyleMap,
    viewport_width: f32,
    _viewport_height: f32,
    measurer: &mut dyn TextMeasure,
) -> LayoutBox {
    let body = doc.body();
    let body_style = styles.get(&body).cloned().unwrap_or_default();

    let content_width = viewport_width - body_style.margin_left - body_style.margin_right;

    let mut root = LayoutBox {
        rect: Rect {
            x: body_style.margin_left,
            y: body_style.margin_top,
            width: content_width,
            height: 0.0,
        },
        node: Some(body),
        box_type: BoxType::Block,
        children: Vec::new(),
    };

    let mut cursor_y: f32 = body_style.padding_top;

    for &child in &doc.node(body).children {
        let child_boxes = layout_node(doc, styles, child, content_width - body_style.padding_left - body_style.padding_right, measurer);
        for mut child_box in child_boxes {
            child_box.rect.x += body_style.padding_left;
            child_box.rect.y += cursor_y;
            let ox = child_box.rect.x;
            let oy = child_box.rect.y;
            offset_children(&mut child_box, ox, oy);
            cursor_y += child_box.rect.height;
            root.children.push(child_box);
        }
    }

    root.rect.height = cursor_y + body_style.padding_bottom;

    root
}

fn offset_children(layout_box: &mut LayoutBox, _parent_x: f32, _parent_y: f32) {
    // Children positions are relative; convert to absolute during painting
    // Actually, let's make positions absolute during layout
    for child in &mut layout_box.children {
        child.rect.x += layout_box.rect.x;
        child.rect.y += layout_box.rect.y;
        offset_children(child, child.rect.x, child.rect.y);
    }
}

fn layout_node(
    doc: &Document,
    styles: &StyleMap,
    node: NodeId,
    available_width: f32,
    measurer: &mut dyn TextMeasure,
) -> Vec<LayoutBox> {
    let n = doc.node(node);
    let style = styles.get(&node).cloned().unwrap_or_default();

    match &n.data {
        NodeData::Element(_) => {
            if style.display == Display::None {
                return Vec::new();
            }
            layout_block_element(doc, styles, node, &style, available_width, measurer)
        }
        NodeData::Text(text) => {
            let text = text.trim();
            if text.is_empty() {
                return Vec::new();
            }
            layout_text(text, &style, available_width, measurer)
        }
        _ => Vec::new(),
    }
}

fn layout_block_element(
    doc: &Document,
    styles: &StyleMap,
    node: NodeId,
    style: &ComputedStyle,
    available_width: f32,
    measurer: &mut dyn TextMeasure,
) -> Vec<LayoutBox> {
    let content_width = match style.width {
        LengthOrAuto::Px(w) => w,
        LengthOrAuto::Percent(p) => available_width * p / 100.0,
        _ => available_width - style.margin_left - style.margin_right,
    };

    let inner_width = content_width - style.padding_left - style.padding_right;

    let mut children = Vec::new();
    let mut cursor_y: f32 = style.padding_top;

    for &child in &doc.node(node).children {
        let child_boxes = layout_node(doc, styles, child, inner_width, measurer);
        for mut child_box in child_boxes {
            child_box.rect.x += style.padding_left + style.margin_left;
            child_box.rect.y = cursor_y;
            cursor_y += child_box.rect.height;
            children.push(child_box);
        }
    }

    let content_height = match style.height {
        LengthOrAuto::Px(h) => h,
        _ => cursor_y + style.padding_bottom,
    };

    let total_height = content_height + style.margin_top + style.margin_bottom;

    let mut layout_box = LayoutBox {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: content_width,
            height: total_height,
        },
        node: Some(node),
        box_type: BoxType::Block,
        children,
    };

    // Offset children to account for margin_top
    for child in &mut layout_box.children {
        child.rect.y += style.margin_top;
    }

    vec![layout_box]
}

fn layout_text(
    text: &str,
    style: &ComputedStyle,
    available_width: f32,
    measurer: &mut dyn TextMeasure,
) -> Vec<LayoutBox> {
    let (_, height) = measurer.measure(text, style.font_size, available_width);

    vec![LayoutBox {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: available_width,
            height,
        },
        node: None,
        box_type: BoxType::Text(text.to_string()),
        children: Vec::new(),
    }]
}
