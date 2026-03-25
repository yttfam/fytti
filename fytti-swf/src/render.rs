use std::collections::HashMap;

use crate::types::*;
use fytti_render::display_list::{color_to_f32, DisplayList, DrawCmd, PathEdge};

/// Display list entry — a placed character at a depth.
#[derive(Debug, Clone)]
struct DisplayEntry {
    character_id: u16,
    matrix: Matrix,
}

/// SWF playback state. Call advance_frame() to step through the timeline.
pub struct SwfPlayer {
    pub swf: Swf,
    /// Character dictionary: id → index into swf.tags where DefineShape lives
    characters: HashMap<u16, usize>,
    /// Current display list (depth → entry)
    display_list: HashMap<u16, DisplayEntry>,
    /// Current tag index (playhead)
    tag_pos: usize,
    /// Current frame number
    pub frame: u16,
    /// Background color
    pub bg_color: Color,
}

impl SwfPlayer {
    pub fn new(swf: Swf) -> Self {
        let mut characters = HashMap::new();
        let bg_color = Color::rgb(255, 255, 255);

        // Build character dictionary
        for (i, tag) in swf.tags.iter().enumerate() {
            match tag {
                Tag::DefineShape(shape) => { characters.insert(shape.id, i); }
                Tag::DefineSprite(sprite) => { characters.insert(sprite.id, i); }
                Tag::DefineText(text) => { characters.insert(text.id, i); }
                _ => {}
            }
        }

        let mut player = SwfPlayer {
            swf,
            characters,
            display_list: HashMap::new(),
            tag_pos: 0,
            frame: 0,
            bg_color,
        };

        // Advance to first ShowFrame to set up initial display list
        player.advance_frame();
        player
    }

    /// Advance one frame. Processes tags until the next ShowFrame.
    /// Returns false if we've reached the end.
    pub fn advance_frame(&mut self) -> bool {
        loop {
            if self.tag_pos >= self.swf.tags.len() {
                // Loop back to start
                self.tag_pos = 0;
                self.display_list.clear();
                self.frame = 0;
            }

            let tag = self.swf.tags[self.tag_pos].clone();
            self.tag_pos += 1;

            match tag {
                Tag::SetBackgroundColor(color) => {
                    self.bg_color = color;
                }
                Tag::PlaceObject(po) => {
                    if let Some(char_id) = po.character_id {
                        let matrix = po.matrix.unwrap_or_default();
                        self.display_list.insert(po.depth, DisplayEntry {
                            character_id: char_id,
                            matrix,
                        });
                    } else if po.is_move {
                        // Move existing entry
                        if let Some(entry) = self.display_list.get_mut(&po.depth) {
                            if let Some(m) = po.matrix {
                                entry.matrix = m;
                            }
                        }
                    }
                }
                Tag::RemoveObject { depth } => {
                    self.display_list.remove(&depth);
                }
                Tag::ShowFrame => {
                    self.frame += 1;
                    return true;
                }
                Tag::End => {
                    return false;
                }
                _ => {}
            }
        }
    }

    /// Render the current display list to a Fytti DisplayList.
    pub fn render(&self, width: u32, height: u32) -> DisplayList {
        let mut dl = DisplayList::new(width, height);
        dl.clear_color = self.bg_color.to_f32();

        // Scale SWF coordinates to viewport
        let sx = width as f32 / self.swf.header.frame_width;
        let sy = height as f32 / self.swf.header.frame_height;

        // Render in depth order
        let mut depths: Vec<u16> = self.display_list.keys().copied().collect();
        depths.sort();

        for depth in depths {
            let entry = &self.display_list[&depth];
            if let Some(&tag_idx) = self.characters.get(&entry.character_id) {
                match &self.swf.tags[tag_idx] {
                    Tag::DefineShape(shape) => {
                        self.render_shape(shape, &entry.matrix, sx, sy, &mut dl);
                    }
                    Tag::DefineSprite(sprite) => {
                        self.render_sprite(sprite, &entry.matrix, sx, sy, &mut dl);
                    }
                    Tag::DefineText(text) => {
                        let (tx, ty) = entry.matrix.transform(text.x, text.y);
                        dl.commands.push(DrawCmd::Text {
                            text: text.text.clone(),
                            x: tx * sx,
                            y: ty * sy,
                            size: text.size * sy.min(sx),
                            color: text.color.to_f32(),
                        });
                    }
                    _ => {}
                }
            }
        }

        dl
    }

    fn render_sprite(
        &self,
        sprite: &DefineSprite,
        parent_matrix: &Matrix,
        sx: f32,
        sy: f32,
        dl: &mut DisplayList,
    ) {
        // Build sprite's character dictionary (shapes defined inside the sprite)
        let mut sprite_chars: HashMap<u16, usize> = HashMap::new();
        for (i, tag) in sprite.tags.iter().enumerate() {
            match tag {
                Tag::DefineShape(s) => { sprite_chars.insert(s.id, i); }
                Tag::DefineSprite(s) => { sprite_chars.insert(s.id, i); }
                _ => {}
            }
        }

        // Walk sprite tags to first ShowFrame, building display list
        let mut sprite_dl: HashMap<u16, DisplayEntry> = HashMap::new();
        for tag in &sprite.tags {
            match tag {
                Tag::PlaceObject(po) => {
                    if let Some(char_id) = po.character_id {
                        let local_matrix = po.matrix.unwrap_or_default();
                        // Combine parent and local transforms
                        let combined = Matrix {
                            a: parent_matrix.a * local_matrix.a + parent_matrix.c * local_matrix.b,
                            b: parent_matrix.b * local_matrix.a + parent_matrix.d * local_matrix.b,
                            c: parent_matrix.a * local_matrix.c + parent_matrix.c * local_matrix.d,
                            d: parent_matrix.b * local_matrix.c + parent_matrix.d * local_matrix.d,
                            tx: parent_matrix.a * local_matrix.tx + parent_matrix.c * local_matrix.ty + parent_matrix.tx,
                            ty: parent_matrix.b * local_matrix.tx + parent_matrix.d * local_matrix.ty + parent_matrix.ty,
                        };
                        sprite_dl.insert(po.depth, DisplayEntry {
                            character_id: char_id,
                            matrix: combined,
                        });
                    }
                }
                Tag::RemoveObject { depth } => { sprite_dl.remove(depth); }
                Tag::ShowFrame => break,
                _ => {}
            }
        }

        // Render sprite's display list
        let mut depths: Vec<u16> = sprite_dl.keys().copied().collect();
        depths.sort();
        for depth in depths {
            let entry = &sprite_dl[&depth];
            // Look up in sprite's local chars first, then global
            let tag_opt = sprite_chars.get(&entry.character_id)
                .map(|&i| &sprite.tags[i])
                .or_else(|| self.characters.get(&entry.character_id).map(|&i| &self.swf.tags[i]));

            if let Some(tag) = tag_opt {
                match tag {
                    Tag::DefineShape(shape) => {
                        self.render_shape(shape, &entry.matrix, sx, sy, dl);
                    }
                    Tag::DefineSprite(nested) => {
                        self.render_sprite(nested, &entry.matrix, sx, sy, dl);
                    }
                    _ => {}
                }
            }
        }
    }

    fn render_shape(
        &self,
        shape: &DefineShape,
        matrix: &Matrix,
        sx: f32,
        sy: f32,
        dl: &mut DisplayList,
    ) {
        for path in &shape.paths {
            let fill = path.fill.and_then(|i| shape.fill_styles.get(i));
            let line = path.line.and_then(|i| shape.line_styles.get(i));

            if path.edges.is_empty() {
                continue;
            }

            // Transform edges to screen coordinates
            let mut edges = Vec::with_capacity(path.edges.len());
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = f32::MIN;
            let mut max_y = f32::MIN;

            for edge in &path.edges {
                let transformed = match edge {
                    ShapeEdge::MoveTo(x, y) => {
                        let (tx, ty) = matrix.transform(*x, *y);
                        let px = tx * sx;
                        let py = ty * sy;
                        min_x = min_x.min(px); min_y = min_y.min(py);
                        max_x = max_x.max(px); max_y = max_y.max(py);
                        PathEdge::MoveTo(px, py)
                    }
                    ShapeEdge::LineTo(x, y) => {
                        let (tx, ty) = matrix.transform(*x, *y);
                        let px = tx * sx;
                        let py = ty * sy;
                        min_x = min_x.min(px); min_y = min_y.min(py);
                        max_x = max_x.max(px); max_y = max_y.max(py);
                        PathEdge::LineTo(px, py)
                    }
                    ShapeEdge::CurveTo { cx, cy, ax, ay } => {
                        let (tcx, tcy) = matrix.transform(*cx, *cy);
                        let (tax, tay) = matrix.transform(*ax, *ay);
                        let pcx = tcx * sx; let pcy = tcy * sy;
                        let pax = tax * sx; let pay = tay * sy;
                        min_x = min_x.min(pcx).min(pax);
                        min_y = min_y.min(pcy).min(pay);
                        max_x = max_x.max(pcx).max(pax);
                        max_y = max_y.max(pcy).max(pay);
                        PathEdge::CurveTo { cx: pcx, cy: pcy, ax: pax, ay: pay }
                    }
                };
                edges.push(transformed);
            }

            let w = max_x - min_x;
            let h = max_y - min_y;
            if w < 0.1 && h < 0.1 { continue; }

            // Emit filled path
            if let Some(fill_style) = fill {
                let color = match fill_style {
                    FillStyle::Solid(c) => c.to_f32(),
                    FillStyle::LinearGradient { colors, .. } => {
                        // Use midpoint color as approximation for path fill
                        if colors.len() >= 2 {
                            let mid = colors.len() / 2;
                            colors[mid].1.to_f32()
                        } else {
                            colors.first().map(|(_, c)| c.to_f32()).unwrap_or([1.0; 4])
                        }
                    }
                    FillStyle::RadialGradient { colors, .. } => {
                        colors.first().map(|(_, c)| c.to_f32()).unwrap_or([1.0; 4])
                    }
                };

                dl.commands.push(DrawCmd::FillPath {
                    edges: edges.clone(),
                    color,
                    bounds: [min_x, min_y, w, h],
                });
            }

            // Emit stroked path
            if let Some(ls) = line {
                // For strokes, emit a FillPath with the line color
                // Proper stroke would need path offsetting — for now use the same path
                dl.commands.push(DrawCmd::FillPath {
                    edges,
                    color: ls.color.to_f32(),
                    bounds: [min_x, min_y, w, h],
                });
            }
        }
    }
}

/// Convenience: parse SWF bytes and render frame 1 to a display list.
pub fn swf_to_display_list(data: &[u8], width: u32, height: u32) -> Result<DisplayList, String> {
    let swf = crate::parse_swf(data)?;
    let player = SwfPlayer::new(swf);
    Ok(player.render(width, height))
}
