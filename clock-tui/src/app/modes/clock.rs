use crate::clock_text::font::bricks::BricksFont;
use crate::clock_text::ClockText;
use crate::config::ClockWidgetConfig;
use chrono::{Local, NaiveDateTime, Utc};
use chrono_tz::Tz;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
};

use super::clock_widget::{clock_height_for_size, clock_size_for_area, ClockTheme, ClockWidgets};
use super::render_centered;

pub(crate) struct Clock {
    pub size: u16,
    pub style: Style,
    pub title: Option<String>,
    pub show_date: bool,
    pub show_millis: bool,
    pub show_secs: bool,
    pub timezone: Option<Tz>,
    widgets_visible: bool,
    widgets: ClockWidgets,
}

impl Clock {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        size: u16,
        style: Style,
        title: Option<String>,
        show_date: bool,
        show_millis: bool,
        show_secs: bool,
        timezone: Option<Tz>,
        widgets: Vec<ClockWidgetConfig>,
        widget_themes: Vec<String>,
    ) -> Self {
        Self {
            size,
            style,
            title,
            show_date,
            show_millis,
            show_secs,
            timezone,
            widgets_visible: true,
            widgets: ClockWidgets::new(widgets, widget_themes),
        }
    }

    pub(crate) fn tick(&mut self) {
        self.widgets.tick();
    }

    pub(crate) fn scroll_widget_at(&mut self, column: u16, row: u16, delta: i16) {
        self.widgets.scroll_at(column, row, delta);
    }

    pub(crate) fn scroll_widget_popup(&mut self, delta: i16) {
        self.widgets.scroll_popup(delta);
    }

    pub(crate) fn scroll_active_widget_to_top(&mut self) {
        self.widgets.scroll_active_to_top();
    }

    pub(crate) fn scroll_active_widget_to_bottom(&mut self) {
        self.widgets.scroll_active_to_bottom();
    }

    pub(crate) fn cycle_widget_theme(&mut self) {
        self.widgets.cycle_theme();
    }

    pub(crate) fn cycle_widget_group(&mut self) {
        self.widgets.cycle_group();
    }

    pub(crate) fn toggle_widgets(&mut self) {
        self.widgets_visible = !self.widgets_visible;
        if !self.widgets_visible {
            self.widgets.hide_all();
        }
    }

    pub(crate) fn open_widget_popup_action(&mut self, key: char) -> bool {
        self.widgets.open_popup_action(key)
    }

    pub(crate) fn close_widget_popup(&mut self) {
        self.widgets.close_popup();
    }

    pub(crate) fn has_widget_popup_open(&self) -> bool {
        self.widgets.has_popup_open()
    }

    pub(crate) fn current_theme(&self) -> ClockTheme {
        self.widgets.current_clock_theme(self.style)
    }

    #[cfg(test)]
    pub(crate) fn current_widget_theme_for_test(&self) -> &str {
        self.widgets.current_theme_for_test()
    }

    #[cfg(test)]
    pub(crate) fn current_theme_for_test(&self) -> ClockTheme {
        self.current_theme()
    }

    #[cfg(test)]
    pub(crate) fn widgets_visible_for_test(&self) -> bool {
        self.widgets_visible
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let now = if let Some(ref tz) = self.timezone {
            Utc::now().with_timezone(tz).naive_local()
        } else {
            Local::now().naive_local()
        };
        let time_str = if !self.show_secs {
            now.format("%H:%M").to_string()
        } else if self.show_millis {
            now.format("%H:%M:%S%.1f").to_string()
        } else {
            now.format("%H:%M:%S").to_string()
        };
        let time_str = time_str.as_str();
        let date = self
            .show_date
            .then(|| format_clock_header(now, self.timezone));
        let header = clock_header(self.title.as_deref(), date);

        if self.widgets.is_empty() || !self.widgets_visible {
            self.render_clock(area, buf, time_str, header, self.size);
        } else {
            let layout = clock_widgets_layout(area, time_str.chars().count(), header.is_some());

            self.render_clock(layout.clock_area, buf, time_str, header, layout.clock_size);
            let theme = self.current_theme();
            self.widgets.render(layout.widgets_area, area, buf, theme);
            self.widgets.render_popup(area, buf, theme);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClockWidgetsLayout {
    clock_area: Rect,
    widgets_area: Rect,
    clock_size: u16,
}

fn clock_widgets_layout(area: Rect, text_len: usize, has_header: bool) -> ClockWidgetsLayout {
    let clock_height_budget = clock_height_budget(area.height);
    let sizing_area = Rect {
        height: clock_height_budget,
        ..area
    };
    let clock_size = clock_size_for_area(text_len, sizing_area, has_header);
    let clock_height = clock_height_for_size(clock_size, has_header)
        .min(clock_height_budget)
        .min(area.height);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(clock_height), Constraint::Min(0)])
        .split(area);

    ClockWidgetsLayout {
        clock_area: chunks[0],
        widgets_area: chunks[1],
        clock_size,
    }
}

fn clock_height_budget(area_height: u16) -> u16 {
    if area_height == 0 {
        0
    } else {
        (area_height / 2).max(1)
    }
}

/// The single header line above the clock digits: the title, the date, or
/// both joined with a middle dot.
fn clock_header(title: Option<&str>, date: Option<String>) -> Option<String> {
    let title = title.map(str::trim).filter(|title| !title.is_empty());
    match (title, date) {
        (Some(title), Some(date)) => Some(format!("{title} · {date}")),
        (Some(title), None) => Some(title.to_string()),
        (None, date) => date,
    }
}

fn format_clock_header(now: NaiveDateTime, timezone: Option<Tz>) -> String {
    let mut title = now.format("%A, %B %-d %Y").to_string();
    if let Some(tz) = timezone {
        title.push(' ');
        title.push_str(tz.name());
    }
    title
}

impl Clock {
    fn render_clock(
        &self,
        area: Rect,
        buf: &mut Buffer,
        time_str: &str,
        header: Option<String>,
        size: u16,
    ) {
        let font = BricksFont::new(size);
        let theme = self.current_theme();
        let text = ClockText::new(time_str.to_string(), &font, theme.clock_style);
        render_centered(area, buf, &text, header, None, theme.text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn widgets_get_extra_height_when_width_limits_square_clock() {
        let layout = clock_widgets_layout(Rect::new(0, 0, 80, 80), 8, true);

        assert_eq!(layout.clock_size, 1);
        assert_eq!(layout.clock_area.height, 9);
        assert_eq!(layout.widgets_area.height, 71);
    }

    #[test]
    fn wide_clock_height_is_capped_to_top_half() {
        let layout = clock_widgets_layout(Rect::new(0, 0, 500, 50), 8, true);

        assert_eq!(layout.clock_size, 4);
        assert!(layout.clock_area.height <= 25);
        assert_eq!(layout.clock_area.height, 24);
        assert_eq!(layout.widgets_area.height, 26);
    }

    #[test]
    fn clock_layout_leaves_vertical_breathing_without_header() {
        let layout = clock_widgets_layout(Rect::new(0, 0, 500, 50), 8, false);

        assert_eq!(layout.clock_size, 4);
        assert_eq!(layout.clock_area.height, 22);
        assert_eq!(layout.widgets_area.height, 28);
    }

    #[test]
    fn clock_layout_handles_tiny_areas() {
        let layout = clock_widgets_layout(Rect::new(0, 0, 10, 1), 8, true);

        assert_eq!(layout.clock_area.height, 1);
        assert_eq!(layout.widgets_area.height, 0);
    }

    #[test]
    fn clock_header_uses_friendly_date_format() {
        let now =
            NaiveDateTime::parse_from_str("2026-06-07 01:41:24", "%Y-%m-%d %H:%M:%S").unwrap();

        assert_eq!(format_clock_header(now, None), "Sunday, June 7 2026");
        assert_eq!(
            format_clock_header(now, Some(chrono_tz::America::New_York)),
            "Sunday, June 7 2026 America/New_York"
        );
    }

    #[test]
    fn clock_header_combines_title_and_date() {
        assert_eq!(clock_header(None, None), None);
        assert_eq!(
            clock_header(None, Some("Sunday".to_string())),
            Some("Sunday".to_string())
        );
        assert_eq!(clock_header(Some("Focus"), None), Some("Focus".to_string()));
        assert_eq!(clock_header(Some("  "), None), None);
        assert_eq!(
            clock_header(Some("Focus"), Some("Sunday".to_string())),
            Some("Focus · Sunday".to_string())
        );
    }

    #[test]
    fn clock_theme_cycle_updates_clock_palette() {
        let mut clock = Clock::new(
            1,
            Style::default().fg(Color::Green),
            None,
            true,
            false,
            true,
            None,
            Vec::new(),
            vec!["nerv".to_string(), "default".to_string()],
        );

        assert_eq!(clock.current_widget_theme_for_test(), "nerv");
        assert_eq!(
            clock.current_theme_for_test().clock_style.fg,
            Some(Color::Indexed(196))
        );
        assert_eq!(
            clock.current_theme_for_test().text_style.fg,
            Some(Color::Indexed(214))
        );

        clock.cycle_widget_theme();

        assert_eq!(clock.current_widget_theme_for_test(), "default");
        assert_eq!(
            clock.current_theme_for_test().clock_style.fg,
            Some(Color::Green)
        );
    }
}
