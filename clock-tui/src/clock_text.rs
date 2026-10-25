use ratatui::{style::Style, widgets::Widget};

use crate::clock_text::font::Font;

pub mod font;
pub mod point;

pub(crate) const CHARACTER_SPACING: u16 = 2;

#[derive(Clone)]
pub struct ClockText<'a> {
    pub text: String,
    pub font: &'a dyn Font,
    pub style: Style,
}

impl<'a> ClockText<'a> {
    pub fn new(text: String, font: &'a dyn Font, style: Style) -> ClockText<'a> {
        ClockText { text, font, style }
    }
    pub fn size(&self) -> (u16, u16) {
        self.font.text_size(&self.text)
    }
}

impl<'a> Widget for ClockText<'a> {
    fn render(self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        self.font.draw_str(&self.text, area, self.style, buf);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Style;

    use super::*;
    use crate::clock_text::font::bricks::BricksFont;

    #[test]
    fn empty_clock_text_has_zero_width() {
        let font = BricksFont::new(1);
        let text = ClockText::new(String::new(), &font, Style::default());

        assert_eq!(text.size(), (0, 5));
    }
}
