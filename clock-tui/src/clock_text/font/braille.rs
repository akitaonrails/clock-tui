use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use super::Font;

/// Draws text as a dot bitmap packed into braille characters. Each terminal
/// cell holds a 2x4 grid of dots, so "12:34:56" fits in 14x2 cells at scale 1.
/// Glyphs are drawn `scale` dots per pixel, with a `scale`-dot gap between them.
pub struct BrailleFont {
    pub scale: u16,
}

const GLYPH_ROWS: u16 = 5;
const CELL_DOTS_X: u16 = 2;
const CELL_DOTS_Y: u16 = 4;
const BRAILLE_BASE: u32 = 0x2800;

/// Bit for the dot at (x, y) within one braille cell, per the Unicode layout.
const DOT_BITS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

fn glyph(c: char) -> Option<[&'static str; 5]> {
    match c {
        '0' => Some(["###", "#.#", "#.#", "#.#", "###"]),
        '1' => Some(["##.", ".#.", ".#.", ".#.", "###"]),
        '2' => Some(["###", "..#", "###", "#..", "###"]),
        '3' => Some(["###", "..#", "###", "..#", "###"]),
        '4' => Some(["#.#", "#.#", "###", "..#", "..#"]),
        '5' => Some(["###", "#..", "###", "..#", "###"]),
        '6' => Some(["###", "#..", "###", "#.#", "###"]),
        '7' => Some(["###", "..#", "..#", "..#", "..#"]),
        '8' => Some(["###", "#.#", "###", "#.#", "###"]),
        '9' => Some(["###", "#.#", "###", "..#", "###"]),
        ':' => Some([".", "#", ".", "#", "."]),
        '.' => Some([".", ".", ".", ".", "#"]),
        '-' => Some(["...", "...", "###", "...", "..."]),
        _ => None,
    }
}

impl BrailleFont {
    pub fn new(scale: u16) -> Self {
        Self {
            scale: scale.max(1),
        }
    }

    /// The largest scale whose output fits `area`, or `None` when even
    /// scale 1 does not fit.
    pub fn fitting(text: &str, area: Rect) -> Option<Self> {
        let fits = |scale| {
            let (width, height) = Self::new(scale).text_size(text);
            width <= area.width && height <= area.height
        };
        if !fits(1) {
            return None;
        }
        let mut scale = 1;
        while fits(scale + 1) {
            scale += 1;
        }
        Some(Self::new(scale))
    }

    /// Width and height of the text in dots.
    fn dot_size(&self, text: &str) -> (u16, u16) {
        let height = GLYPH_ROWS.saturating_mul(self.scale);
        let mut width: u16 = 0;
        for (i, g) in text.chars().filter_map(glyph).enumerate() {
            if i > 0 {
                width = width.saturating_add(self.scale);
            }
            width = width.saturating_add((g[0].len() as u16).saturating_mul(self.scale));
        }
        (width, height)
    }
}

impl Font for BrailleFont {
    fn get_char(&self, _c: char) -> Option<&[crate::clock_text::point::Point]> {
        None // Glyphs are packed across cell boundaries in draw_str
    }

    fn get_char_width(&self) -> u16 {
        (3 * self.scale).div_ceil(CELL_DOTS_X)
    }

    fn get_char_height(&self) -> u16 {
        (GLYPH_ROWS * self.scale).div_ceil(CELL_DOTS_Y)
    }

    fn text_size(&self, s: &str) -> (u16, u16) {
        let (width, height) = self.dot_size(s);
        (width.div_ceil(CELL_DOTS_X), height.div_ceil(CELL_DOTS_Y))
    }

    fn draw_char(&self, c: char, x: u16, y: u16, style: Style, buf: &mut Buffer) {
        let area = Rect::new(
            x,
            y,
            buf.area.right().saturating_sub(x),
            buf.area.bottom().saturating_sub(y),
        );
        self.draw_str(&c.to_string(), area, style, buf);
    }

    fn draw_str(&self, s: &str, area: Rect, style: Style, buf: &mut Buffer) {
        let (dot_width, dot_height) = self.dot_size(s);
        let (cols, rows) = self.text_size(s);
        let grid_width = (cols * CELL_DOTS_X) as usize;
        let grid_height = (rows * CELL_DOTS_Y) as usize;
        // Spread the slack dots evenly so the text sits centered in its cells.
        let offset_x = (grid_width - dot_width as usize) / 2;
        let offset_y = (grid_height - dot_height as usize) / 2;
        let scale = self.scale as usize;

        let mut dots = vec![false; grid_width * grid_height];
        let mut left = offset_x;
        for g in s.chars().filter_map(glyph) {
            for (row, line) in g.iter().enumerate() {
                for (col, pixel) in line.chars().enumerate() {
                    if pixel != '#' {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = left + col * scale + dx;
                            let y = offset_y + row * scale + dy;
                            dots[y * grid_width + x] = true;
                        }
                    }
                }
            }
            left += (g[0].len() + 1) * scale;
        }

        for cell_y in 0..rows.min(area.height) {
            for cell_x in 0..cols.min(area.width) {
                let mut bits = 0u8;
                for (dy, row_bits) in DOT_BITS.iter().enumerate() {
                    for (dx, bit) in row_bits.iter().enumerate() {
                        let x = (cell_x * CELL_DOTS_X) as usize + dx;
                        let y = (cell_y * CELL_DOTS_Y) as usize + dy;
                        if dots[y * grid_width + x] {
                            bits |= bit;
                        }
                    }
                }
                if bits == 0 {
                    continue;
                }
                if let Some(ch) = char::from_u32(BRAILLE_BASE + bits as u32) {
                    let (x, y) = (area.x + cell_x, area.y + cell_y);
                    if x < buf.area.right() && y < buf.area.bottom() {
                        buf[(x, y)].set_char(ch).set_style(style);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_text_packs_into_two_rows() {
        let font = BrailleFont::new(1);

        assert_eq!(font.text_size("12:34:56"), (14, 2));
        assert_eq!(font.text_size("12:34"), (9, 2));
        assert_eq!(font.text_size(""), (0, 2));
    }

    #[test]
    fn fitting_picks_largest_scale_that_fits() {
        assert_eq!(
            BrailleFont::fitting("12:34", Rect::new(0, 0, 9, 2)).map(|f| f.scale),
            Some(1)
        );
        assert_eq!(
            BrailleFont::fitting("12:34", Rect::new(0, 0, 30, 6)).map(|f| f.scale),
            Some(3)
        );
        assert!(BrailleFont::fitting("12:34", Rect::new(0, 0, 8, 2)).is_none());
    }

    #[test]
    fn draw_str_centers_dots_vertically_in_cells() {
        let font = BrailleFont::new(1);
        let area = Rect::new(0, 0, 2, 2);
        let mut buf = Buffer::empty(area);

        font.draw_str("1", area, Style::default(), &mut buf);

        // Five glyph rows in eight dot rows leave one blank dot row on top.
        assert_eq!(buf[(0, 0)].symbol(), "\u{28b2}");
        assert_eq!(buf[(0, 1)].symbol(), "\u{281a}");
    }

    #[test]
    fn draw_str_respects_render_area() {
        let font = BrailleFont::new(1);
        let area = Rect::new(0, 0, 4, 1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));

        font.draw_str("88:88", area, Style::default(), &mut buf);

        for y in 0..buf.area.bottom() {
            for x in 0..buf.area.right() {
                if !area.contains((x, y).into()) {
                    assert_eq!(buf[(x, y)].symbol(), " ");
                }
            }
        }
    }
}
