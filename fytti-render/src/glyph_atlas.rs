use std::collections::HashMap;
use cosmic_text::{CacheKey, FontSystem, SwashCache, SwashContent};

/// A cached glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct CachedGlyph {
    /// Position in atlas texture (pixels)
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub width: u32,
    pub height: u32,
    /// Offset from pen position to top-left of glyph bitmap
    pub offset_x: i32,
    pub offset_y: i32,
}

/// A row-packed glyph atlas.
pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA
    cache: HashMap<CacheKey, CachedGlyph>,
    /// Current packing position
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    pub dirty: bool,
    /// Region that needs re-upload
    pub dirty_min_y: u32,
    pub dirty_max_y: u32,
}

impl GlyphAtlas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
            cache: HashMap::with_capacity(256),
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            dirty: false,
            dirty_min_y: height,
            dirty_max_y: 0,
        }
    }

    /// Get a cached glyph, or rasterize and cache it.
    pub fn get_or_insert(
        &mut self,
        cache_key: CacheKey,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
    ) -> Option<CachedGlyph> {
        if let Some(glyph) = self.cache.get(&cache_key) {
            return Some(*glyph);
        }

        // Rasterize the glyph
        let image = match swash_cache.get_image(font_system, cache_key) {
            Some(img) => img,
            None => return None,
        };

        let glyph_w = image.placement.width;
        let glyph_h = image.placement.height;

        if glyph_w == 0 || glyph_h == 0 {
            // Whitespace or empty glyph — cache a zero-size entry
            let glyph = CachedGlyph {
                atlas_x: 0,
                atlas_y: 0,
                width: 0,
                height: 0,
                offset_x: image.placement.left,
                offset_y: image.placement.top,
            };
            self.cache.insert(cache_key, glyph);
            return Some(glyph);
        }

        // Check if we need to wrap to next row
        if self.cursor_x + glyph_w + 1 > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        // Check if atlas is full
        if self.cursor_y + glyph_h > self.height {
            // Atlas full — could grow or evict, for now just fail
            return None;
        }

        let ax = self.cursor_x;
        let ay = self.cursor_y;

        // Copy glyph bitmap into atlas
        match image.content {
            SwashContent::Mask => {
                // Alpha mask — expand to RGBA (white + alpha)
                for row in 0..glyph_h {
                    for col in 0..glyph_w {
                        let src = (row * glyph_w + col) as usize;
                        let dst_x = (ax + col) as usize;
                        let dst_y = (ay + row) as usize;
                        let dst = (dst_y * self.width as usize + dst_x) * 4;
                        if src < image.data.len() && dst + 3 < self.pixels.len() {
                            let a = image.data[src];
                            self.pixels[dst] = 255;
                            self.pixels[dst + 1] = 255;
                            self.pixels[dst + 2] = 255;
                            self.pixels[dst + 3] = a;
                        }
                    }
                }
            }
            SwashContent::Color => {
                // Full RGBA
                for row in 0..glyph_h {
                    for col in 0..glyph_w {
                        let src = ((row * glyph_w + col) * 4) as usize;
                        let dst_x = (ax + col) as usize;
                        let dst_y = (ay + row) as usize;
                        let dst = (dst_y * self.width as usize + dst_x) * 4;
                        if src + 3 < image.data.len() && dst + 3 < self.pixels.len() {
                            self.pixels[dst] = image.data[src];
                            self.pixels[dst + 1] = image.data[src + 1];
                            self.pixels[dst + 2] = image.data[src + 2];
                            self.pixels[dst + 3] = image.data[src + 3];
                        }
                    }
                }
            }
            SwashContent::SubpixelMask => {
                // Treat as grayscale mask (take first channel)
                for row in 0..glyph_h {
                    for col in 0..glyph_w {
                        let src = ((row * glyph_w + col) * 3) as usize;
                        let dst_x = (ax + col) as usize;
                        let dst_y = (ay + row) as usize;
                        let dst = (dst_y * self.width as usize + dst_x) * 4;
                        if src < image.data.len() && dst + 3 < self.pixels.len() {
                            let a = image.data[src];
                            self.pixels[dst] = 255;
                            self.pixels[dst + 1] = 255;
                            self.pixels[dst + 2] = 255;
                            self.pixels[dst + 3] = a;
                        }
                    }
                }
            }
        }

        // Track dirty region
        self.dirty = true;
        self.dirty_min_y = self.dirty_min_y.min(ay);
        self.dirty_max_y = self.dirty_max_y.max(ay + glyph_h);

        // Advance cursor
        self.cursor_x += glyph_w + 1;
        self.row_height = self.row_height.max(glyph_h);

        let glyph = CachedGlyph {
            atlas_x: ax,
            atlas_y: ay,
            width: glyph_w,
            height: glyph_h,
            offset_x: image.placement.left,
            offset_y: image.placement.top,
        };
        self.cache.insert(cache_key, glyph);
        Some(glyph)
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        self.dirty_min_y = self.height;
        self.dirty_max_y = 0;
    }
}
