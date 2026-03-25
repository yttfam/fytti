// cssparser is a dependency for future use (proper tokenization)
// Currently using manual parsing for simplicity
use fytti_html::{Document, NodeData, NodeId};
use std::collections::HashMap;

// ── Types ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthOrAuto {
    Px(f32),
    Em(f32),
    Percent(f32),
    Auto,
}

impl Default for LengthOrAuto {
    fn default() -> Self {
        LengthOrAuto::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Display {
    Block,
    Inline,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Color,
    pub background_color: Color,
    pub font_size: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
    pub width: LengthOrAuto,
    pub height: LengthOrAuto,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        ComputedStyle {
            display: Display::Block,
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            font_size: 16.0,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            width: LengthOrAuto::Auto,
            height: LengthOrAuto::Auto,
        }
    }
}

pub type StyleMap = HashMap<NodeId, ComputedStyle>;

// ── Selectors (simple) ──

#[derive(Debug, Clone)]
struct Selector {
    tag: Option<String>,
    class: Option<String>,
    id: Option<String>,
    /// Optional ancestor context: "div p" → ancestor=Some(Selector{tag:"div"}), tag:"p"
    ancestor: Option<Box<Selector>>,
}

impl Selector {
    fn specificity(&self) -> (u32, u32, u32) {
        let a = if self.id.is_some() { 1 } else { 0 };
        let b = if self.class.is_some() { 1 } else { 0 };
        let c = if self.tag.is_some() { 1 } else { 0 };
        let (pa, pb, pc) = self
            .ancestor
            .as_ref()
            .map(|s| s.specificity())
            .unwrap_or((0, 0, 0));
        (a + pa, b + pb, c + pc)
    }

    fn matches(&self, doc: &Document, node: NodeId) -> bool {
        if !self.matches_simple(doc, node) {
            return false;
        }
        if let Some(ref ancestor_sel) = self.ancestor {
            // Check if any ancestor matches
            for anc in doc.ancestors(node) {
                if ancestor_sel.matches(doc, anc) {
                    return true;
                }
            }
            return false;
        }
        true
    }

    fn matches_simple(&self, doc: &Document, node: NodeId) -> bool {
        let n = doc.node(node);
        let el = match &n.data {
            NodeData::Element(el) => el,
            _ => return false,
        };
        if let Some(ref tag) = self.tag {
            if el.name.local.as_ref() != tag.as_str() {
                return false;
            }
        }
        if let Some(ref class) = self.class {
            let classes = el.attributes.get("class").map(|s| s.as_str()).unwrap_or("");
            if !classes.split_whitespace().any(|c| c == class) {
                return false;
            }
        }
        if let Some(ref id) = self.id {
            let node_id = el.attributes.get("id").map(|s| s.as_str()).unwrap_or("");
            if node_id != id {
                return false;
            }
        }
        true
    }
}

fn parse_selector(s: &str) -> Option<Selector> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let mut iter = parts.iter().rev();
    let mut sel = parse_simple_selector(iter.next()?)?;
    for part in iter {
        let ancestor = parse_simple_selector(part)?;
        sel.ancestor = Some(Box::new(ancestor));
    }
    Some(sel)
}

fn parse_simple_selector(s: &str) -> Option<Selector> {
    let mut tag = None;
    let mut class = None;
    let mut id = None;

    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let mut remaining = s;

    // Parse tag name (if doesn't start with . or #)
    if !remaining.starts_with('.') && !remaining.starts_with('#') {
        let end = remaining
            .find(|c: char| c == '.' || c == '#')
            .unwrap_or(remaining.len());
        tag = Some(remaining[..end].to_string());
        remaining = &remaining[end..];
    }

    // Parse class and id parts
    while !remaining.is_empty() {
        if remaining.starts_with('.') {
            remaining = &remaining[1..];
            let end = remaining
                .find(|c: char| c == '.' || c == '#')
                .unwrap_or(remaining.len());
            class = Some(remaining[..end].to_string());
            remaining = &remaining[end..];
        } else if remaining.starts_with('#') {
            remaining = &remaining[1..];
            let end = remaining
                .find(|c: char| c == '.' || c == '#')
                .unwrap_or(remaining.len());
            id = Some(remaining[..end].to_string());
            remaining = &remaining[end..];
        } else {
            break;
        }
    }

    Some(Selector {
        tag,
        class,
        id,
        ancestor: None,
    })
}

// ── CSS Rule Parsing ──

#[derive(Debug)]
struct Rule {
    selector: Selector,
    declarations: Vec<Declaration>,
}

#[derive(Debug, Clone)]
struct Declaration {
    property: String,
    value: String,
}

fn parse_stylesheet(css: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut pos = 0;
    let bytes = css.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace and comments
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Find selector (everything before '{')
        let sel_start = pos;
        while pos < bytes.len() && bytes[pos] != b'{' {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let selector_str = css[sel_start..pos].trim();
        pos += 1; // skip '{'

        // Find declarations (everything before '}')
        let decl_start = pos;
        let mut depth = 1;
        while pos < bytes.len() && depth > 0 {
            if bytes[pos] == b'{' {
                depth += 1;
            } else if bytes[pos] == b'}' {
                depth -= 1;
            }
            if depth > 0 {
                pos += 1;
            }
        }
        let decl_str = &css[decl_start..pos];
        pos += 1; // skip '}'

        if let Some(selector) = parse_selector(selector_str) {
            let declarations = parse_declarations(decl_str);
            rules.push(Rule {
                selector,
                declarations,
            });
        }
    }

    rules
}

fn parse_declarations(css: &str) -> Vec<Declaration> {
    css.split(';')
        .filter_map(|decl| {
            let decl = decl.trim();
            if decl.is_empty() {
                return None;
            }
            let colon = decl.find(':')?;
            let property = decl[..colon].trim().to_lowercase();
            let value = decl[colon + 1..].trim().to_string();
            Some(Declaration { property, value })
        })
        .collect()
}

// ── Value Parsing ──

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();

    // Named colors
    match s {
        "black" => return Some(Color::rgb(0, 0, 0)),
        "white" => return Some(Color::rgb(255, 255, 255)),
        "red" => return Some(Color::rgb(255, 0, 0)),
        "green" => return Some(Color::rgb(0, 128, 0)),
        "blue" => return Some(Color::rgb(0, 0, 255)),
        "yellow" => return Some(Color::rgb(255, 255, 0)),
        "transparent" => return Some(Color::TRANSPARENT),
        "gray" | "grey" => return Some(Color::rgb(128, 128, 128)),
        _ => {}
    }

    // Hex colors
    if s.starts_with('#') {
        let hex = &s[1..];
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some(Color::rgb(r, g, b))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Color::rgba(r, g, b, a))
            }
            _ => None,
        };
    }

    // rgb(r, g, b) / rgba(r, g, b, a)
    if s.starts_with("rgb") {
        let inner = s
            .trim_start_matches("rgba(")
            .trim_start_matches("rgb(")
            .trim_end_matches(')');
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() >= 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            let a = if parts.len() >= 4 {
                (parts[3].trim().parse::<f32>().ok()? * 255.0) as u8
            } else {
                255
            };
            return Some(Color::rgba(r, g, b, a));
        }
    }

    None
}

fn parse_length(s: &str) -> Option<f32> {
    let s = s.trim();
    if s == "0" {
        return Some(0.0);
    }
    if let Some(px) = s.strip_suffix("px") {
        return px.trim().parse().ok();
    }
    // Bare number → treat as px
    s.parse().ok()
}

fn parse_length_or_auto(s: &str) -> LengthOrAuto {
    let s = s.trim();
    if s == "auto" {
        return LengthOrAuto::Auto;
    }
    if let Some(pct) = s.strip_suffix('%') {
        if let Ok(v) = pct.trim().parse::<f32>() {
            return LengthOrAuto::Percent(v);
        }
    }
    if let Some(em) = s.strip_suffix("em") {
        if let Ok(v) = em.trim().parse::<f32>() {
            return LengthOrAuto::Em(v);
        }
    }
    if let Some(v) = parse_length(s) {
        return LengthOrAuto::Px(v);
    }
    LengthOrAuto::Auto
}

fn parse_display(s: &str) -> Option<Display> {
    match s.trim() {
        "block" => Some(Display::Block),
        "inline" => Some(Display::Inline),
        "none" => Some(Display::None),
        _ => Some(Display::Block), // fallback
    }
}

// ── Apply Declarations to Style ──

fn apply_declaration(style: &mut ComputedStyle, decl: &Declaration) {
    match decl.property.as_str() {
        "display" => {
            if let Some(d) = parse_display(&decl.value) {
                style.display = d;
            }
        }
        "color" => {
            if let Some(c) = parse_color(&decl.value) {
                style.color = c;
            }
        }
        "background-color" | "background" => {
            if let Some(c) = parse_color(&decl.value) {
                style.background_color = c;
            }
        }
        "font-size" => {
            if let Some(v) = parse_length(&decl.value) {
                style.font_size = v;
            }
        }
        "margin" => {
            if let Some(v) = parse_length(&decl.value) {
                style.margin_top = v;
                style.margin_right = v;
                style.margin_bottom = v;
                style.margin_left = v;
            }
        }
        "margin-top" => {
            if let Some(v) = parse_length(&decl.value) {
                style.margin_top = v;
            }
        }
        "margin-right" => {
            if let Some(v) = parse_length(&decl.value) {
                style.margin_right = v;
            }
        }
        "margin-bottom" => {
            if let Some(v) = parse_length(&decl.value) {
                style.margin_bottom = v;
            }
        }
        "margin-left" => {
            if let Some(v) = parse_length(&decl.value) {
                style.margin_left = v;
            }
        }
        "padding" => {
            if let Some(v) = parse_length(&decl.value) {
                style.padding_top = v;
                style.padding_right = v;
                style.padding_bottom = v;
                style.padding_left = v;
            }
        }
        "padding-top" => {
            if let Some(v) = parse_length(&decl.value) {
                style.padding_top = v;
            }
        }
        "padding-right" => {
            if let Some(v) = parse_length(&decl.value) {
                style.padding_right = v;
            }
        }
        "padding-bottom" => {
            if let Some(v) = parse_length(&decl.value) {
                style.padding_bottom = v;
            }
        }
        "padding-left" => {
            if let Some(v) = parse_length(&decl.value) {
                style.padding_left = v;
            }
        }
        "width" => {
            style.width = parse_length_or_auto(&decl.value);
        }
        "height" => {
            style.height = parse_length_or_auto(&decl.value);
        }
        _ => {} // ignore unknown properties
    }
}

// ── UA Defaults ──

fn ua_default(tag: &str) -> ComputedStyle {
    let mut s = ComputedStyle::default();
    match tag {
        "h1" => {
            s.font_size = 32.0;
            s.margin_top = 21.0;
            s.margin_bottom = 21.0;
        }
        "h2" => {
            s.font_size = 24.0;
            s.margin_top = 19.0;
            s.margin_bottom = 19.0;
        }
        "h3" => {
            s.font_size = 19.0;
            s.margin_top = 18.0;
            s.margin_bottom = 18.0;
        }
        "p" => {
            s.margin_top = 16.0;
            s.margin_bottom = 16.0;
        }
        "body" => {
            s.margin_top = 8.0;
            s.margin_right = 8.0;
            s.margin_bottom = 8.0;
            s.margin_left = 8.0;
        }
        "span" | "a" | "strong" | "em" | "b" | "i" => {
            s.display = Display::Inline;
        }
        "head" | "style" | "script" | "meta" | "title" | "link" => {
            s.display = Display::None;
        }
        _ => {}
    }
    s
}

// ── Style Resolution ──

pub fn resolve(doc: &Document) -> StyleMap {
    let mut styles = StyleMap::new();

    // Collect CSS from <style> elements
    let mut all_css = String::new();
    collect_style_elements(doc, doc.root(), &mut all_css);
    let rules = parse_stylesheet(&all_css);

    // Sort rules by specificity
    let mut indexed_rules: Vec<(usize, &Rule)> = rules.iter().enumerate().collect();
    indexed_rules.sort_by_key(|(i, r)| (r.selector.specificity(), *i));

    // Resolve styles for all nodes
    resolve_node(doc, doc.root(), &indexed_rules, &mut styles, None);

    styles
}

fn collect_style_elements(doc: &Document, node: NodeId, css: &mut String) {
    if doc.tag_name(node) == Some("style") {
        css.push_str(&doc.text_content(node));
        css.push('\n');
        return;
    }
    for &child in &doc.node(node).children {
        collect_style_elements(doc, child, css);
    }
}

fn resolve_node(
    doc: &Document,
    node: NodeId,
    rules: &[(usize, &Rule)],
    styles: &mut StyleMap,
    parent_style: Option<&ComputedStyle>,
) {
    let n = doc.node(node);

    let style = match &n.data {
        NodeData::Element(el) => {
            let tag = el.name.local.as_ref();

            // Start with UA defaults
            let mut style = ua_default(tag);

            // Inherit from parent
            if let Some(ps) = parent_style {
                style.color = ps.color;
                style.font_size = ps.font_size;
            }

            // Apply matching rules (already sorted by specificity)
            for (_, rule) in rules {
                if rule.selector.matches(doc, node) {
                    for decl in &rule.declarations {
                        apply_declaration(&mut style, decl);
                    }
                }
            }

            // Apply inline styles (highest priority)
            if let Some(inline) = el.attributes.get("style") {
                let decls = parse_declarations(inline);
                for decl in &decls {
                    apply_declaration(&mut style, decl);
                }
            }

            style
        }
        NodeData::Text(_) => {
            // Text nodes inherit from parent
            parent_style.cloned().unwrap_or_default()
        }
        _ => ComputedStyle::default(),
    };

    styles.insert(node, style.clone());

    for &child in &n.children {
        resolve_node(doc, child, rules, styles, Some(&style));
    }
}
