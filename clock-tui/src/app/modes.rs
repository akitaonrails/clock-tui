mod clock;
mod clock_widget;
mod countdown;
mod pause;
mod stopwatch;
mod timer;

use std::cmp::min;
use std::fmt::Write as _;
use std::time::Instant;

use crate::clock_text::ClockText;
use chrono::Duration;
pub(crate) use clock::Clock;
pub(crate) use countdown::Countdown;
pub(crate) use pause::Pause;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Span,
    widgets::{Paragraph, Widget},
};
pub(crate) use stopwatch::Stopwatch;
pub(crate) use timer::Timer;

pub(crate) const PAUSED_FOOTER: &str = "PAUSED (press <SPACE> to resume)";
const FLASH_PERIOD_MILLIS: i64 = 1000;
const FLASH_ON_MILLIS: i64 = 500;

#[derive(Copy, Clone)]
pub(crate) enum DurationFormat {
    /// Hours, minutes, seconds, deciseconds
    HourMinSecDeci,
    /// Hours, minutes, seconds
    HourMinSec,
}

fn format_duration(duration: Duration, format: DurationFormat) -> String {
    let is_neg = duration < Duration::zero();
    let duration = if is_neg { -duration } else { duration };

    let millis = duration.num_milliseconds();
    let seconds = millis / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let mut result = String::new();

    fn append_number(s: &mut String, num: i64) {
        if s.is_empty() {
            let _ = write!(s, "{}", num);
        } else {
            let _ = write!(s, "{:02}", num);
        }
    }

    if days > 0 {
        let _ = write!(result, "{}:", days);
    }
    if hours > 0 {
        append_number(&mut result, hours % 24);
        result.push(':');
    }
    append_number(&mut result, minutes % 60);
    result.push(':');

    if is_neg {
        result.insert(0, '-');
    }
    match format {
        DurationFormat::HourMinSecDeci => {
            let _ = write!(result, "{:02}.{}", seconds % 60, (millis % 1000) / 100);
        }
        DurationFormat::HourMinSec => {
            let _ = write!(result, "{:02}", seconds % 60);
        }
    }

    result
}

fn elapsed_since(started_at: Instant) -> Duration {
    Duration::from_std(started_at.elapsed()).unwrap_or(Duration::MAX)
}

fn should_flash(duration: Duration) -> bool {
    duration.num_milliseconds().abs() % FLASH_PERIOD_MILLIS < FLASH_ON_MILLIS
}

fn render_centered(
    area: Rect,
    buf: &mut Buffer,
    text: &ClockText,
    header: Option<String>,
    footer: Option<String>,
    label_style: Style,
) {
    let text_size = text.size();
    let mut text_area = Rect {
        x: area.x + (area.width.saturating_sub(text_size.0)) / 2,
        y: area.y + (area.height.saturating_sub(text_size.1)) / 2,
        width: min(text_size.0, area.width),
        height: min(text_size.1, area.height),
    };

    if header.is_some() && area.top() + 2 == text_area.top() && text_area.bottom() < area.bottom() {
        text_area.y += 1;
    }

    text.clone().render(text_area, buf);

    let render_text_center = |text: &str, top: u16, buf: &mut Buffer| {
        let text_len = text.len() as u16;
        let paragrahp = Paragraph::new(Span::from(text)).style(label_style);

        let para_area = Rect {
            x: area.left() + (area.width.saturating_sub(text_len)) / 2,
            y: top,
            width: min(text_len, area.width),
            height: min(1, area.height),
        };
        paragrahp.render(para_area, buf);
    };

    if let Some(text) = header {
        if area.top() + 2 <= text_area.top() {
            render_text_center(text.as_str(), text_area.top() - 2, buf);
        }
    }

    if let Some(text) = footer {
        if area.bottom() >= text_area.bottom() + 2 {
            render_text_center(text.as_str(), text_area.bottom() + 1, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_supports_deciseconds() {
        assert_eq!(
            format_duration(
                Duration::milliseconds(65_432),
                DurationFormat::HourMinSecDeci
            ),
            "1:05.4"
        );
    }

    #[test]
    fn format_duration_supports_no_fractional_seconds() {
        assert_eq!(
            format_duration(Duration::seconds(65), DurationFormat::HourMinSec),
            "1:05"
        );
    }

    #[test]
    fn format_duration_supports_hours_days_and_negative_values() {
        let duration =
            Duration::days(1) + Duration::hours(2) + Duration::minutes(3) + Duration::seconds(4);

        assert_eq!(
            format_duration(duration, DurationFormat::HourMinSecDeci),
            "1:02:03:04.0"
        );
        assert_eq!(
            format_duration(-Duration::seconds(65), DurationFormat::HourMinSecDeci),
            "-1:05.0"
        );
    }

    #[test]
    fn should_flash_uses_first_half_of_each_second() {
        assert!(should_flash(Duration::milliseconds(-499)));
        assert!(!should_flash(Duration::milliseconds(-500)));
    }

    #[test]
    fn render_centered_applies_header_footer_style() {
        use crate::clock_text::font::bricks::BricksFont;
        use ratatui::style::Color;

        let area = ratatui::layout::Rect::new(0, 0, 40, 12);
        let mut buffer = Buffer::empty(area);
        let font = BricksFont::new(1);
        let text = ClockText::new("12".to_string(), &font, Style::default());

        render_centered(
            area,
            &mut buffer,
            &text,
            Some("Header".to_string()),
            Some("Footer".to_string()),
            Style::default().fg(Color::LightYellow),
        );

        let header_x = area.x + (area.width - "Header".len() as u16) / 2;
        assert_eq!(buffer[(header_x, 1)].style().fg, Some(Color::LightYellow));
    }
}
