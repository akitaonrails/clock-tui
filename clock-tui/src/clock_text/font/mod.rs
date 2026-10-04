pub mod braille;
pub mod bricks;

use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use super::{point::Point, CHARACTER_SPACING};

pub trait Font {
    fn get_char(&self, c: char) -> Option<&[Point]>;
    fn get_char_width(&self) -> u16;
    fn get_char_height(&self) -> u16;

    /// Width and height in cells of `s` drawn with this font.
    fn text_size(&self, s: &str) -> (u16, u16) {
        let char_count = s.chars().count() as u16;
        let height = self.get_char_height();

        if char_count == 0 {
            return (0, height);
        }

        let char_width = self.get_char_width().saturating_add(CHARACTER_SPACING);
        let width = char_count
            .saturating_mul(char_width)
            .saturating_sub(CHARACTER_SPACING);

        (width, height)
    }

    fn draw_char(&self, c: char, x: u16, y: u16, style: Style, buf: &mut Buffer) {
        if let Some(points) = self.get_char(c) {
            for point in points {
                let x = x.saturating_add(point.0);
                let y = y.saturating_add(point.1);
                if x < buf.area.right() && y < buf.area.bottom() {
                    buf[(x, y)].set_style(style);
                }
            }
        }
    }

    fn draw_str(&self, s: &str, area: Rect, style: Style, buf: &mut Buffer) {
        let mut x = area.x;
        let y = area.y;
        for c in s.chars() {
            if x.saturating_add(self.get_char_width()) > area.right() {
                break;
            }
            self.draw_char(c, x, y, style, buf);
            x = x
                .saturating_add(self.get_char_width())
                .saturating_add(CHARACTER_SPACING);
        }
    }
}
