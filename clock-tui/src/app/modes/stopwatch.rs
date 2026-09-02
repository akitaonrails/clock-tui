use std::time::Instant;

use crate::clock_text::font::bricks::BricksFont;
use crate::clock_text::ClockText;
use chrono::Duration;
use ratatui::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::app::modes::pause::Pause;

use super::{elapsed_since, format_duration, render_centered, DurationFormat, PAUSED_FOOTER};

pub struct Stopwatch {
    pub size: u16,
    pub style: Style,
    pub title: Option<String>,
    format: DurationFormat,
    duration: Duration,
    started_at: Option<Instant>,
}

impl Stopwatch {
    pub(crate) fn new(
        size: u16,
        style: Style,
        title: Option<String>,
        format: DurationFormat,
        paused: bool,
    ) -> Self {
        Self {
            size,
            style,
            title,
            format,
            duration: Duration::zero(),
            started_at: (!paused).then(Instant::now),
        }
    }

    pub(crate) fn total_time(&self) -> Duration {
        if let Some(start_at) = self.started_at {
            self.duration + elapsed_since(start_at)
        } else {
            self.duration
        }
    }

    pub fn get_display_time(&self) -> String {
        format_duration(self.total_time(), DurationFormat::HourMinSecDeci)
    }
}

impl Widget for &Stopwatch {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let time_str = format_duration(self.total_time(), self.format);
        let font = BricksFont::new(self.size);
        let text = ClockText::new(time_str, &font, self.style);
        let footer = if self.is_paused() {
            Some(PAUSED_FOOTER.to_string())
        } else {
            None
        };
        render_centered(
            area,
            buf,
            &text,
            self.title.clone(),
            footer,
            Style::default(),
        );
    }
}

impl Pause for Stopwatch {
    fn is_paused(&self) -> bool {
        self.started_at.is_none()
    }

    fn pause(&mut self) {
        if let Some(start_at) = self.started_at {
            self.duration += elapsed_since(start_at);
            self.started_at = None;
        }
    }

    fn resume(&mut self) {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopwatch_can_start_paused() {
        let stopwatch = Stopwatch::new(
            1,
            Style::default(),
            Some("Focus".to_string()),
            DurationFormat::HourMin,
            true,
        );

        assert!(stopwatch.is_paused());
        assert_eq!(stopwatch.total_time(), Duration::zero());
        assert_eq!(stopwatch.title.as_deref(), Some("Focus"));
    }

    #[test]
    fn stopwatch_starts_running_by_default() {
        let stopwatch =
            Stopwatch::new(1, Style::default(), None, DurationFormat::HourMinSec, false);

        assert!(!stopwatch.is_paused());
    }
}
