use crate::types::*;
use std::io::Read;

/// Parse a SWF file from bytes.
pub fn parse_swf(data: &[u8]) -> Result<Swf, String> {
    if data.len() < 8 {
        return Err("file too small".into());
    }

    let sig = &data[0..3];
    let version = data[3];
    let file_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Decompress if needed
    let body = match sig {
        b"FWS" => data[8..].to_vec(),
        b"CWS" => {
            // zlib compressed
            let mut decoder = flate2::read::ZlibDecoder::new(&data[8..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map_err(|e| format!("zlib: {e}"))?;
            decompressed
        }
        b"ZWS" => {
            // LZMA compressed — not supported yet
            return Err("LZMA-compressed SWF not supported".into());
        }
        _ => return Err(format!("invalid SWF signature: {:?}", sig)),
    };

    let mut reader = BitReader::new(&body);

    // Frame rect (bit-packed)
    let header_rect = reader.read_rect()?;
    let frame_rate = reader.read_u16()? as f32 / 256.0;
    let frame_count = reader.read_u16()?;

    let header = SwfHeader {
        version,
        file_length,
        frame_width: header_rect.width(),
        frame_height: header_rect.height(),
        frame_rate,
        frame_count,
    };

    // Parse tags
    let mut tags = Vec::new();
    loop {
        let tag = reader.read_tag(version)?;
        let is_end = matches!(tag, Tag::End);
        tags.push(tag);
        if is_end {
            break;
        }
    }

    Ok(Swf { header, tags })
}

/// Bit-level reader for SWF's packed binary format.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,       // byte position
    bit_pos: u8,      // bits remaining in current byte (8 = fresh byte)
    current: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit_pos: 0, current: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        self.align();
        if self.pos >= self.data.len() {
            return Err("unexpected EOF".into());
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let lo = self.read_u8()? as u16;
        let hi = self.read_u8()? as u16;
        Ok(lo | (hi << 8))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let a = self.read_u8()? as u32;
        let b = self.read_u8()? as u32;
        let c = self.read_u8()? as u32;
        let d = self.read_u8()? as u32;
        Ok(a | (b << 8) | (c << 16) | (d << 24))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        Ok(self.read_u16()? as i16)
    }

    fn align(&mut self) {
        self.bit_pos = 0;
    }

    fn read_ub(&mut self, n: u8) -> Result<u32, String> {
        if n == 0 { return Ok(0); }
        let mut result = 0u32;
        let mut bits_left = n;
        while bits_left > 0 {
            if self.bit_pos == 0 {
                if self.pos >= self.data.len() {
                    return Err("unexpected EOF in bits".into());
                }
                self.current = self.data[self.pos];
                self.pos += 1;
                self.bit_pos = 8;
            }
            let take = bits_left.min(self.bit_pos);
            let shift = self.bit_pos - take;
            let mask = ((1u16 << take) - 1) as u8;
            let bits = (self.current >> shift) & mask;
            result = (result << take) | bits as u32;
            self.bit_pos -= take;
            bits_left -= take;
        }
        Ok(result)
    }

    fn read_sb(&mut self, n: u8) -> Result<i32, String> {
        let val = self.read_ub(n)?;
        // Sign extend
        if n > 0 && val & (1 << (n - 1)) != 0 {
            Ok(val as i32 | !((1i32 << n) - 1))
        } else {
            Ok(val as i32)
        }
    }

    fn read_fb(&mut self, n: u8) -> Result<f32, String> {
        let val = self.read_sb(n)?;
        Ok(val as f32 / 65536.0)
    }

    fn read_rect(&mut self) -> Result<Rect, String> {
        let nbits = self.read_ub(5)? as u8;
        let x_min = self.read_sb(nbits)? as f32 / 20.0; // twips to pixels
        let x_max = self.read_sb(nbits)? as f32 / 20.0;
        let y_min = self.read_sb(nbits)? as f32 / 20.0;
        let y_max = self.read_sb(nbits)? as f32 / 20.0;
        self.align();
        Ok(Rect { x_min, y_min, x_max, y_max })
    }

    fn read_matrix(&mut self) -> Result<Matrix, String> {
        let mut m = Matrix::default();
        // HasScale
        let has_scale = self.read_ub(1)? != 0;
        if has_scale {
            let nbits = self.read_ub(5)? as u8;
            m.a = self.read_fb(nbits)?;
            m.d = self.read_fb(nbits)?;
        }
        // HasRotate
        let has_rotate = self.read_ub(1)? != 0;
        if has_rotate {
            let nbits = self.read_ub(5)? as u8;
            m.b = self.read_fb(nbits)?;
            m.c = self.read_fb(nbits)?;
        }
        // Translate
        let nbits = self.read_ub(5)? as u8;
        m.tx = self.read_sb(nbits)? as f32 / 20.0;
        m.ty = self.read_sb(nbits)? as f32 / 20.0;
        self.align();
        Ok(m)
    }

    fn read_rgb(&mut self) -> Result<Color, String> {
        let r = self.read_u8()?;
        let g = self.read_u8()?;
        let b = self.read_u8()?;
        Ok(Color::rgb(r, g, b))
    }

    fn read_rgba(&mut self) -> Result<Color, String> {
        let r = self.read_u8()?;
        let g = self.read_u8()?;
        let b = self.read_u8()?;
        let a = self.read_u8()?;
        Ok(Color::rgba(r, g, b, a))
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn skip(&mut self, n: usize) {
        self.align();
        self.pos = (self.pos + n).min(self.data.len());
    }

    fn read_tag(&mut self, version: u8) -> Result<Tag, String> {
        self.align();
        if self.remaining() < 2 {
            return Ok(Tag::End);
        }

        let tag_code_and_length = self.read_u16()?;
        let tag_code = tag_code_and_length >> 6;
        let mut length = (tag_code_and_length & 0x3F) as usize;

        // Long tag header
        if length == 0x3F {
            length = self.read_u32()? as usize;
        }

        let start_pos = self.pos;

        let tag = match tag_code {
            0 => Tag::End,
            1 => Tag::ShowFrame,
            9 => {
                // SetBackgroundColor
                let color = self.read_rgb()?;
                Tag::SetBackgroundColor(color)
            }
            2 | 22 | 32 | 83 => {
                // DefineShape (1/2/3/4)
                let uses_rgba = tag_code >= 32;
                match self.parse_define_shape(uses_rgba) {
                    Ok(shape) => Tag::DefineShape(shape),
                    Err(_) => {
                        self.pos = start_pos + length;
                        Tag::Unknown { tag_code, length }
                    }
                }
            }
            4 | 26 | 70 => {
                // PlaceObject / PlaceObject2 / PlaceObject3
                match self.parse_place_object(tag_code, length, start_pos) {
                    Ok(po) => Tag::PlaceObject(po),
                    Err(_) => {
                        self.pos = start_pos + length;
                        Tag::Unknown { tag_code, length }
                    }
                }
            }
            6 | 21 | 35 => {
                // DefineBitsJPEG / DefineBitsJPEG2 / DefineBitsJPEG3
                match self.parse_define_bits_jpeg(tag_code, length, start_pos) {
                    Ok(bmp) => Tag::DefineBitmap(bmp),
                    Err(_) => {
                        self.pos = start_pos + length;
                        Tag::Unknown { tag_code, length }
                    }
                }
            }
            20 | 36 => {
                // DefineBitsLossless / DefineBitsLossless2
                match self.parse_define_bits_lossless(tag_code, length, start_pos) {
                    Ok(bmp) => Tag::DefineBitmap(bmp),
                    Err(_) => {
                        self.pos = start_pos + length;
                        Tag::Unknown { tag_code, length }
                    }
                }
            }
            39 => {
                // DefineSprite — MovieClip with nested timeline
                match self.parse_define_sprite(version) {
                    Ok(sprite) => Tag::DefineSprite(sprite),
                    Err(_) => {
                        self.pos = start_pos + length;
                        Tag::Unknown { tag_code, length }
                    }
                }
            }
            5 | 28 => {
                // RemoveObject / RemoveObject2
                if tag_code == 28 {
                    let depth = self.read_u16()?;
                    Tag::RemoveObject { depth }
                } else {
                    let _char_id = self.read_u16()?;
                    let depth = self.read_u16()?;
                    Tag::RemoveObject { depth }
                }
            }
            _ => {
                self.skip(length);
                Tag::Unknown { tag_code, length }
            }
        };

        // Ensure we consumed exactly `length` bytes
        let consumed = self.pos - start_pos;
        if consumed < length {
            self.skip(length - consumed);
        }

        Ok(tag)
    }

    fn parse_define_bits_jpeg(
        &mut self,
        tag_code: u16,
        length: usize,
        start_pos: usize,
    ) -> Result<DefineBitmap, String> {
        let id = self.read_u16()?;

        let alpha_offset = if tag_code == 35 {
            // DefineBitsJPEG3: has alpha data offset
            Some(self.read_u32()? as usize)
        } else {
            None
        };

        let jpeg_start = self.pos;
        let jpeg_len = if let Some(ao) = alpha_offset {
            ao
        } else {
            length - (self.pos - start_pos)
        };

        // Read JPEG data — skip erroneous header bytes if present
        let mut jpeg_data = self.data[jpeg_start..jpeg_start + jpeg_len].to_vec();
        // SWF sometimes prepends FF D9 FF D8 (EOI + SOI) — strip it
        if jpeg_data.len() > 4 && jpeg_data[0] == 0xFF && jpeg_data[1] == 0xD9 {
            jpeg_data = jpeg_data[2..].to_vec();
        }
        self.pos = jpeg_start + jpeg_len;

        // Decode JPEG
        let img = image::load_from_memory_with_format(&jpeg_data, image::ImageFormat::Jpeg)
            .map_err(|e| format!("JPEG decode: {e}"))?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut pixels = rgba.into_raw();

        // Apply alpha channel for DefineBitsJPEG3
        if tag_code == 35 {
            let alpha_len = length - (self.pos - start_pos);
            let alpha_compressed = &self.data[self.pos..self.pos + alpha_len];
            self.pos += alpha_len;

            let mut alpha = Vec::new();
            let mut decoder = flate2::read::ZlibDecoder::new(alpha_compressed);
            let _ = decoder.read_to_end(&mut alpha);

            // Apply alpha to each pixel
            for (i, &a) in alpha.iter().enumerate() {
                let idx = i * 4 + 3;
                if idx < pixels.len() {
                    pixels[idx] = a;
                }
            }
        }

        Ok(DefineBitmap { id, width: w, height: h, data: pixels })
    }

    fn parse_define_bits_lossless(
        &mut self,
        tag_code: u16,
        length: usize,
        start_pos: usize,
    ) -> Result<DefineBitmap, String> {
        let has_alpha = tag_code == 36;
        let id = self.read_u16()?;
        let format = self.read_u8()?;
        let width = self.read_u16()? as u32;
        let height = self.read_u16()? as u32;

        let color_table_size = if format == 3 {
            self.read_u8()? as usize + 1
        } else {
            0
        };

        // Rest is zlib-compressed pixel data
        let compressed_len = length - (self.pos - start_pos);
        let compressed = &self.data[self.pos..self.pos + compressed_len];
        self.pos += compressed_len;

        let mut decompressed = Vec::new();
        let mut decoder = flate2::read::ZlibDecoder::new(compressed);
        decoder.read_to_end(&mut decompressed).map_err(|e| format!("zlib: {e}"))?;

        let mut pixels = vec![0u8; (width * height * 4) as usize];

        match format {
            3 => {
                // Colormapped: color table + 1-byte-per-pixel indices
                let entry_size = if has_alpha { 4 } else { 3 };
                let table_bytes = color_table_size * entry_size;
                if decompressed.len() < table_bytes {
                    return Err("lossless: data too short for color table".into());
                }
                let (table, indices) = decompressed.split_at(table_bytes);

                // Rows are padded to 4-byte boundaries
                let row_stride = ((width as usize + 3) / 4) * 4;
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let idx = indices.get(y * row_stride + x).copied().unwrap_or(0) as usize;
                        let dst = (y * width as usize + x) * 4;
                        if idx < color_table_size && dst + 3 < pixels.len() {
                            let src = idx * entry_size;
                            pixels[dst] = table.get(src).copied().unwrap_or(0);
                            pixels[dst + 1] = table.get(src + 1).copied().unwrap_or(0);
                            pixels[dst + 2] = table.get(src + 2).copied().unwrap_or(0);
                            pixels[dst + 3] = if has_alpha {
                                table.get(src + 3).copied().unwrap_or(255)
                            } else {
                                255
                            };
                        }
                    }
                }
            }
            4 => {
                // 15-bit RGB (rare)
                return Err("lossless: 15-bit format not supported".into());
            }
            5 => {
                // 24-bit RGB (tag 20) or 32-bit ARGB (tag 36)
                if has_alpha {
                    // ARGB premultiplied
                    for i in 0..(width * height) as usize {
                        let si = i * 4;
                        let di = i * 4;
                        if si + 3 < decompressed.len() && di + 3 < pixels.len() {
                            let a = decompressed[si];
                            pixels[di] = decompressed[si + 1]; // R
                            pixels[di + 1] = decompressed[si + 2]; // G
                            pixels[di + 2] = decompressed[si + 3]; // B
                            pixels[di + 3] = a;
                        }
                    }
                } else {
                    // 0RGB (pad byte + RGB)
                    for i in 0..(width * height) as usize {
                        let si = i * 4;
                        let di = i * 4;
                        if si + 3 < decompressed.len() && di + 3 < pixels.len() {
                            pixels[di] = decompressed[si + 1];
                            pixels[di + 1] = decompressed[si + 2];
                            pixels[di + 2] = decompressed[si + 3];
                            pixels[di + 3] = 255;
                        }
                    }
                }
            }
            _ => {
                return Err(format!("lossless: unknown format {format}"));
            }
        }

        Ok(DefineBitmap { id, width, height, data: pixels })
    }

    fn parse_define_sprite(&mut self, version: u8) -> Result<DefineSprite, String> {
        let id = self.read_u16()?;
        let frame_count = self.read_u16()?;

        // Parse sub-tag stream until End tag
        let mut tags = Vec::new();
        loop {
            let tag = self.read_tag(version)?;
            let is_end = matches!(tag, Tag::End);
            tags.push(tag);
            if is_end { break; }
        }

        Ok(DefineSprite { id, frame_count, tags })
    }

    fn parse_define_shape(&mut self, rgba: bool) -> Result<DefineShape, String> {
        let id = self.read_u16()?;
        let bounds = self.read_rect()?;

        // Fill styles
        let mut fill_styles = Vec::new();
        let fill_count = self.read_u8()? as usize;
        // Extended count not handled for simplicity
        for _ in 0..fill_count {
            let fill_type = self.read_u8()?;
            match fill_type {
                0x00 => {
                    // Solid fill
                    let color = if rgba { self.read_rgba()? } else { self.read_rgb()? };
                    fill_styles.push(FillStyle::Solid(color));
                }
                0x10 | 0x12 => {
                    // Linear/radial gradient
                    let matrix = self.read_matrix()?;
                    let num_stops = self.read_u8()? as usize;
                    let mut colors = Vec::with_capacity(num_stops);
                    for _ in 0..num_stops {
                        let ratio = self.read_u8()?;
                        let color = if rgba { self.read_rgba()? } else { self.read_rgb()? };
                        colors.push((ratio, color));
                    }
                    if fill_type == 0x10 {
                        fill_styles.push(FillStyle::LinearGradient { matrix, colors });
                    } else {
                        fill_styles.push(FillStyle::RadialGradient { matrix, colors });
                    }
                }
                0x40 | 0x41 | 0x42 | 0x43 => {
                    // Bitmap fill
                    let character_id = self.read_u16()?;
                    let matrix = self.read_matrix()?;
                    let repeating = fill_type == 0x40 || fill_type == 0x42;
                    let smoothed = fill_type == 0x42 || fill_type == 0x43;
                    fill_styles.push(FillStyle::Bitmap {
                        character_id, matrix, repeating, smoothed,
                    });
                }
                0x13 => {
                    // Focal radial gradient — parse like radial + extra focal point
                    let matrix = self.read_matrix()?;
                    let num_stops = self.read_u8()? as usize;
                    let mut colors = Vec::with_capacity(num_stops);
                    for _ in 0..num_stops {
                        let ratio = self.read_u8()?;
                        let color = if rgba { self.read_rgba()? } else { self.read_rgb()? };
                        colors.push((ratio, color));
                    }
                    let _focal_point = self.read_i16()?; // fixed 8.8
                    fill_styles.push(FillStyle::RadialGradient { matrix, colors });
                }
                _ => {
                    return Err(format!("unsupported fill type: {fill_type:#x}"));
                }
            }
        }

        // Line styles
        let mut line_styles = Vec::new();
        let line_count = self.read_u8()? as usize;
        for _ in 0..line_count {
            let width = self.read_u16()? as f32 / 20.0;
            let color = if rgba { self.read_rgba()? } else { self.read_rgb()? };
            line_styles.push(LineStyle { width, color });
        }

        // Shape records (edge records)
        let paths = self.parse_shape_records(&fill_styles, &line_styles)?;

        Ok(DefineShape {
            id,
            bounds,
            fill_styles,
            line_styles,
            paths,
        })
    }

    fn parse_shape_records(
        &mut self,
        _fill_styles: &[FillStyle],
        _line_styles: &[LineStyle],
    ) -> Result<Vec<ShapePath>, String> {
        let num_fill_bits = self.read_ub(4)? as u8;
        let num_line_bits = self.read_ub(4)? as u8;

        let mut paths = Vec::new();
        let mut current_path = ShapePath {
            fill: None,
            line: None,
            edges: Vec::new(),
        };
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut cur_fill0: Option<usize> = None;
        let mut cur_fill1: Option<usize> = None;
        let mut cur_line: Option<usize> = None;
        let mut nfb = num_fill_bits;
        let mut nlb = num_line_bits;

        loop {
            let type_flag = self.read_ub(1)?;

            if type_flag == 0 {
                // Non-edge record
                let flags = self.read_ub(5)?;
                if flags == 0 {
                    // EndShape
                    if !current_path.edges.is_empty() {
                        current_path.fill = cur_fill0.or(cur_fill1);
                        current_path.line = cur_line;
                        paths.push(current_path);
                    }
                    break;
                }

                // StyleChange record
                if !current_path.edges.is_empty() {
                    current_path.fill = cur_fill0.or(cur_fill1);
                    current_path.line = cur_line;
                    paths.push(current_path.clone());
                    current_path.edges.clear();
                }

                if flags & 0x01 != 0 {
                    // MoveTo
                    let nbits = self.read_ub(5)? as u8;
                    x = self.read_sb(nbits)? as f32 / 20.0;
                    y = self.read_sb(nbits)? as f32 / 20.0;
                    current_path.edges.push(ShapeEdge::MoveTo(x, y));
                }
                if flags & 0x02 != 0 {
                    // FillStyle0
                    let idx = self.read_ub(nfb)? as usize;
                    cur_fill0 = if idx > 0 { Some(idx - 1) } else { None };
                }
                if flags & 0x04 != 0 {
                    // FillStyle1
                    let idx = self.read_ub(nfb)? as usize;
                    cur_fill1 = if idx > 0 { Some(idx - 1) } else { None };
                }
                if flags & 0x08 != 0 {
                    // LineStyle
                    let idx = self.read_ub(nlb)? as usize;
                    cur_line = if idx > 0 { Some(idx - 1) } else { None };
                }
                if flags & 0x10 != 0 {
                    // NewStyles — not supported in DefineShape1
                    return Err("NewStyles not supported".into());
                }
            } else {
                // Edge record
                let straight = self.read_ub(1)?;
                let nbits = self.read_ub(4)? as u8 + 2;

                if straight != 0 {
                    // StraightEdge
                    let general = self.read_ub(1)?;
                    if general != 0 {
                        let dx = self.read_sb(nbits)? as f32 / 20.0;
                        let dy = self.read_sb(nbits)? as f32 / 20.0;
                        x += dx;
                        y += dy;
                    } else {
                        let vert = self.read_ub(1)?;
                        if vert != 0 {
                            let dy = self.read_sb(nbits)? as f32 / 20.0;
                            y += dy;
                        } else {
                            let dx = self.read_sb(nbits)? as f32 / 20.0;
                            x += dx;
                        }
                    }
                    current_path.edges.push(ShapeEdge::LineTo(x, y));
                } else {
                    // CurvedEdge
                    let cx_delta = self.read_sb(nbits)? as f32 / 20.0;
                    let cy_delta = self.read_sb(nbits)? as f32 / 20.0;
                    let ax_delta = self.read_sb(nbits)? as f32 / 20.0;
                    let ay_delta = self.read_sb(nbits)? as f32 / 20.0;
                    let cx = x + cx_delta;
                    let cy = y + cy_delta;
                    let ax = cx + ax_delta;
                    let ay = cy + ay_delta;
                    current_path.edges.push(ShapeEdge::CurveTo { cx, cy, ax, ay });
                    x = ax;
                    y = ay;
                }
            }
        }

        self.align();
        Ok(paths)
    }

    fn parse_place_object(
        &mut self,
        tag_code: u16,
        length: usize,
        start_pos: usize,
    ) -> Result<PlaceObject, String> {
        if tag_code == 4 {
            // PlaceObject1
            let character_id = self.read_u16()?;
            let depth = self.read_u16()?;
            let matrix = self.read_matrix()?;
            Ok(PlaceObject {
                depth,
                character_id: Some(character_id),
                matrix: Some(matrix),
                is_move: false,
            })
        } else {
            // PlaceObject2/3
            let flags = self.read_u8()?;
            let _flags2 = if tag_code == 70 { self.read_u8()? } else { 0 };
            let depth = self.read_u16()?;

            let is_move = flags & 0x01 != 0;
            let has_character = flags & 0x02 != 0;
            let has_matrix = flags & 0x04 != 0;
            // has_color_transform = flags & 0x08
            // has_ratio = flags & 0x10
            // has_name = flags & 0x20
            // has_clip_depth = flags & 0x40
            // has_clip_actions = flags & 0x80

            let character_id = if has_character { Some(self.read_u16()?) } else { None };
            let matrix = if has_matrix { Some(self.read_matrix()?) } else { None };

            // Skip remaining fields
            let consumed = self.pos - start_pos;
            if consumed < length {
                self.skip(length - consumed);
            }

            Ok(PlaceObject {
                depth,
                character_id,
                matrix,
                is_move,
            })
        }
    }
}
