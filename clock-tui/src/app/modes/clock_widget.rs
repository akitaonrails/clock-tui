use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph, Widget, Wrap},
};

use crate::config::{
    ClockWidgetConfig, ClockWidgetPopupActionConfig, WidgetPosition, DEFAULT_WIDGET_REFRESH_SECS,
    DEFAULT_WIDGET_TIMEOUT_SECS,
};

const DEFAULT_REFRESH: Duration = Duration::from_secs(DEFAULT_WIDGET_REFRESH_SECS);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(DEFAULT_WIDGET_TIMEOUT_SECS);
const SQUARE_TERMINAL_WIDGETS: usize = 2;
const WIDE_TERMINAL_WIDGETS: usize = 4;
const ULTRAWIDE_TERMINAL_WIDGETS: usize = 6;

// Terminal cells are generally about twice as tall as they are wide in pixels.
// Use this to approximate visual aspect ratio from character-cell dimensions.
const CELL_HEIGHT_TO_WIDTH_RATIO: f32 = 2.0;
const WIDE_TERMINAL_ASPECT: f32 = 1.5;
const ULTRAWIDE_TERMINAL_ASPECT: f32 = 2.2;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PROCESS_GROUP_KILL_GRACE: Duration = Duration::from_millis(100);
const MAX_WIDGET_OUTPUT_BYTES: usize = 64 * 1024;
const CLOCK_GLYPH_COLUMNS: usize = 6;
const CLOCK_GLYPH_ROWS: usize = 5;
const CLOCK_CHARACTER_SPACING: u16 = 2;
const CLOCK_HORIZONTAL_MARGIN: u16 = 4;
const CLOCK_HEADER_VERTICAL_PADDING: u16 = 4;
const CLOCK_NO_HEADER_VERTICAL_PADDING: u16 = 2;
// When bottom-positioned widgets coexist with the widget row, the row keeps at
// least this many rows so the columns stay usable.
const MIN_WIDGET_ROW_HEIGHT: u16 = 8;
// A bottom widget that can't get at least this many rows is hidden instead of
// rendering an unusable sliver.
const MIN_BOTTOM_WIDGET_HEIGHT: u16 = 3;
const WIDGET_THEME_ENV: &str = "TCLOCK_WIDGET_THEME";
const POPUP_MAX_WIDTH: u16 = 110;
const POPUP_MAX_HEIGHT: u16 = 30;
const POPUP_MARGIN: u16 = 2;

pub(crate) struct ClockWidgets {
    widgets: Vec<ClockWidget>,
    tx: Sender<WidgetMessage>,
    rx: Receiver<WidgetMessage>,
    cancel: Arc<AtomicBool>,
    viewports: Vec<Rect>,
    active_widget: Option<usize>,
    themes: Vec<String>,
    theme_index: usize,
    groups: Vec<String>,
    group_index: usize,
    popup_tx: Sender<PopupMessage>,
    popup_rx: Receiver<PopupMessage>,
    popup: Option<WidgetPopup>,
    popup_viewport: Rect,
    popup_cancel: Option<Arc<AtomicBool>>,
    popup_handle: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClockTheme {
    pub(crate) clock_style: Style,
    pub(crate) text_style: Style,
    pub(crate) widget_style: Style,
    pub(crate) widget_title_style: Style,
    pub(crate) widget_marker_style: Style,
}

impl ClockTheme {
    pub(crate) fn named(name: &str, base_style: Style) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "evangelion" => Self::evangelion(base_style),
            "nerv" => Self::nerv(base_style),
            _ => Self::default_theme(base_style),
        }
    }

    fn default_theme(base_style: Style) -> Self {
        Self {
            clock_style: base_style,
            text_style: base_style,
            widget_style: base_style,
            widget_title_style: base_style
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
            widget_marker_style: base_style
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
        }
    }

    fn evangelion(base_style: Style) -> Self {
        // Original Evangelion-inspired palette: purple title, NERV orange
        // labels, muted lavender base, EVA green accent.
        Self {
            clock_style: base_style
                .fg(Color::Indexed(171))
                .add_modifier(Modifier::BOLD),
            text_style: base_style.fg(Color::Indexed(208)),
            widget_style: base_style.fg(Color::Indexed(103)),
            widget_title_style: base_style
                .fg(Color::Indexed(99))
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
            widget_marker_style: base_style
                .fg(Color::Indexed(118))
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
        }
    }

    fn nerv(base_style: Style) -> Self {
        // Screenshot-matched NERV palette: red monitor clock, orange interface
        // text on a dark substrate, with neon green active marks.
        Self {
            clock_style: base_style
                .fg(Color::Indexed(196))
                .add_modifier(Modifier::BOLD),
            text_style: base_style.fg(Color::Indexed(214)),
            widget_style: base_style.fg(Color::Indexed(172)),
            widget_title_style: base_style
                .fg(Color::Indexed(214))
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
            widget_marker_style: base_style
                .fg(Color::Indexed(48))
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
        }
    }
}

struct ClockWidget {
    title: Option<String>,
    command: Vec<String>,
    popup_actions: Vec<WidgetPopupAction>,
    refresh: Duration,
    timeout: Duration,
    position: WidgetPosition,
    group: Option<String>,
    output: String,
    running: bool,
    visible: bool,
    scroll: u16,
    next_run: Instant,
    rerun_requested: bool,
    handle: Option<JoinHandle<()>>,
}

struct WidgetMessage {
    index: usize,
    output: String,
}

#[derive(Clone)]
struct WidgetPopupAction {
    key: char,
    label: Option<String>,
    title: Option<String>,
    command: Vec<String>,
    timeout: Duration,
}

struct WidgetPopup {
    widget_index: usize,
    action_index: usize,
    title: String,
    output: String,
    scroll: u16,
}

struct PopupMessage {
    widget_index: usize,
    action_index: usize,
    output: String,
}

impl ClockWidgets {
    pub(crate) fn new(configs: Vec<ClockWidgetConfig>, themes: Vec<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (popup_tx, popup_rx) = mpsc::channel();
        let now = Instant::now();
        let cancel = Arc::new(AtomicBool::new(false));
        let themes = normalize_themes(themes);
        let groups = collect_groups(&configs);
        let widgets = configs
            .into_iter()
            .map(|config| {
                let timeout = duration_or_default(config.timeout_secs, DEFAULT_TIMEOUT);
                let popup_actions = config
                    .popup_actions
                    .into_iter()
                    .map(|action| popup_action(action, &config.command, timeout))
                    .collect();
                ClockWidget {
                    title: config.title,
                    command: config.command,
                    popup_actions,
                    refresh: duration_or_default(config.refresh_secs, DEFAULT_REFRESH),
                    timeout,
                    position: config.position,
                    group: normalize_group(config.group.as_deref()),
                    output: "Loading...".to_string(),
                    running: false,
                    visible: false,
                    scroll: 0,
                    next_run: now,
                    rerun_requested: false,
                    handle: None,
                }
            })
            .collect();

        Self {
            widgets,
            tx,
            rx,
            cancel,
            viewports: Vec::new(),
            active_widget: None,
            themes,
            theme_index: 0,
            groups,
            group_index: 0,
            popup_tx,
            popup_rx,
            popup: None,
            popup_viewport: Rect::default(),
            popup_cancel: None,
            popup_handle: None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    /// Remove every widget from the active layout. Running commands may finish,
    /// but `tick` will not schedule another refresh while they remain hidden.
    pub(crate) fn hide_all(&mut self) {
        for widget in &mut self.widgets {
            widget.visible = false;
        }
        self.viewports.fill(Rect::default());
        self.active_widget = None;
    }

    pub(crate) fn tick(&mut self) {
        let now = Instant::now();
        let mut finished_handles = Vec::new();
        let mut popup_finished = false;

        while let Ok(message) = self.rx.try_recv() {
            if let Some(widget) = self.widgets.get_mut(message.index) {
                if !widget.rerun_requested {
                    widget.output = message.output;
                }
                widget.clamp_scroll_to_output();
                widget.running = false;
                widget.next_run = if widget.rerun_requested {
                    widget.rerun_requested = false;
                    now
                } else {
                    now + widget.refresh
                };
                if let Some(handle) = widget.handle.take() {
                    finished_handles.push(handle);
                }
            }
        }

        for handle in finished_handles {
            let _ = handle.join();
        }

        while let Ok(message) = self.popup_rx.try_recv() {
            if let Some(popup) = self.popup.as_mut() {
                if popup.widget_index == message.widget_index
                    && popup.action_index == message.action_index
                {
                    popup.output = message.output;
                    popup.clamp_scroll(self.popup_viewport);
                    popup_finished = true;
                }
            }
        }
        if popup_finished {
            if let Some(handle) = self.popup_handle.take() {
                let _ = handle.join();
            }
            self.popup_cancel = None;
        }

        let theme = self.current_theme().to_string();
        let current_group = self.current_group().map(str::to_string);
        for (index, widget) in self.widgets.iter_mut().enumerate() {
            let in_active_group = match widget.group.as_deref() {
                None => true,
                Some(group) => current_group.as_deref() == Some(group),
            };
            if !widget.visible || !in_active_group || widget.running || now < widget.next_run {
                continue;
            }

            widget.running = true;
            let command = widget.command.clone();
            let timeout = widget.timeout;
            let theme = theme.clone();
            let tx = self.tx.clone();
            let cancel = self.cancel.clone();

            widget.handle = Some(thread::spawn(move || {
                let output = run_command(command, timeout, cancel, theme);
                let _ = tx.send(WidgetMessage { index, output });
            }));
        }
    }

    pub(crate) fn cycle_theme(&mut self) {
        if self.themes.len() <= 1 {
            return;
        }

        self.theme_index = (self.theme_index + 1) % self.themes.len();
        let now = Instant::now();
        for widget in &mut self.widgets {
            if widget.visible {
                widget.output = "Loading...".to_string();
                widget.scroll = 0;
            }
            if widget.running {
                widget.rerun_requested = true;
            } else {
                widget.next_run = now;
            }
        }
    }

    /// Switch to the next widget group. Widgets outside the new group stop
    /// refreshing (they are no longer visible) but keep their last output, so
    /// coming back to a group is instant instead of showing "Loading...".
    pub(crate) fn cycle_group(&mut self) {
        if self.groups.len() <= 1 {
            return;
        }

        self.group_index = (self.group_index + 1) % self.groups.len();
        self.active_widget = None;
    }

    fn current_group(&self) -> Option<&str> {
        self.groups.get(self.group_index).map(String::as_str)
    }

    /// A widget is eligible for layout when it has no group (always shown) or
    /// its group is the active one.
    fn in_active_group(&self, widget: &ClockWidget) -> bool {
        match widget.group.as_deref() {
            None => true,
            Some(group) => Some(group) == self.current_group(),
        }
    }

    #[cfg(test)]
    pub(crate) fn current_group_for_test(&self) -> Option<&str> {
        self.current_group()
    }

    fn current_theme(&self) -> &str {
        self.themes
            .get(self.theme_index)
            .map(String::as_str)
            .unwrap_or("default")
    }

    pub(crate) fn current_clock_theme(&self, base_style: Style) -> ClockTheme {
        ClockTheme::named(self.current_theme(), base_style)
    }

    #[cfg(test)]
    pub(crate) fn current_theme_for_test(&self) -> &str {
        self.current_theme()
    }

    pub(crate) fn render(
        &mut self,
        area: Rect,
        terminal_area: Rect,
        buf: &mut Buffer,
        theme: ClockTheme,
    ) {
        self.viewports = vec![Rect::default(); self.widgets.len()];
        for widget in &mut self.widgets {
            widget.visible = false;
        }
        if area.height == 0 || area.width == 0 {
            return;
        }

        let row_indices: Vec<usize> = self
            .widgets
            .iter()
            .enumerate()
            .filter(|(_, widget)| {
                widget.position == WidgetPosition::Auto && self.in_active_group(widget)
            })
            .map(|(index, _)| index)
            .collect();
        let bottom_indices: Vec<usize> = self
            .widgets
            .iter()
            .enumerate()
            .filter(|(_, widget)| {
                widget.position == WidgetPosition::Bottom && self.in_active_group(widget)
            })
            .map(|(index, _)| index)
            .collect();

        let row_count = visible_widget_count(terminal_area, row_indices.len());

        // Bottom band: full-width widgets stacked under the row, each just tall
        // enough for its content. If a row is present it keeps a minimum height.
        let bottom_budget = if row_count > 0 {
            area.height.saturating_sub(MIN_WIDGET_ROW_HEIGHT)
        } else {
            area.height
        };
        let mut bottom_heights = Vec::new();
        let mut bottom_total: u16 = 0;
        for &index in &bottom_indices {
            let widget = &self.widgets[index];
            let needed = bottom_widget_height(&widget.title(), &widget.output, area.width);
            let granted = needed.min(bottom_budget.saturating_sub(bottom_total));
            if granted < MIN_BOTTOM_WIDGET_HEIGHT {
                bottom_heights.push((index, 0));
                continue;
            }
            bottom_heights.push((index, granted));
            bottom_total = bottom_total.saturating_add(granted);
        }

        let row_area = Rect {
            height: area.height - bottom_total,
            ..area
        };
        let mut bottom_y = area.y + row_area.height;

        if row_count > 0 && row_area.height > 0 {
            let constraints = (0..row_count)
                .map(|_| Constraint::Ratio(1, row_count as u32))
                .collect::<Vec<_>>();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(row_area);

            for (&index, chunk) in row_indices.iter().take(row_count).zip(chunks.iter()) {
                let viewport = padded_widget_area(*chunk);
                self.viewports[index] = viewport;
                let widget = &mut self.widgets[index];
                widget.visible = true;
                widget.render(viewport, buf, theme);
            }
        }

        for (index, height) in bottom_heights {
            if height == 0 {
                continue;
            }
            let band = Rect {
                x: area.x,
                y: bottom_y,
                width: area.width,
                height,
            };
            bottom_y = bottom_y.saturating_add(height);
            let viewport = padded_widget_area(band);
            self.viewports[index] = viewport;
            let widget = &mut self.widgets[index];
            widget.visible = true;
            widget.render(viewport, buf, theme);
        }
    }

    pub(crate) fn render_popup(
        &mut self,
        terminal_area: Rect,
        buf: &mut Buffer,
        theme: ClockTheme,
    ) {
        let Some(popup) = self.popup.as_mut() else {
            self.popup_viewport = Rect::default();
            return;
        };
        let area = centered_popup_area(terminal_area);
        if area.width == 0 || area.height == 0 {
            self.popup_viewport = Rect::default();
            return;
        }

        Clear.render(area, buf);
        let block = Block::bordered()
            .title(format!(" {} ", popup.title))
            .title_style(theme.widget_title_style)
            .title_bottom(" Esc close · ↑/↓ scroll ")
            .style(theme.widget_style);
        let inner = block.inner(area);
        self.popup_viewport = inner;
        popup.clamp_scroll(inner);
        block.render(area, buf);

        Paragraph::new(Text::from(ansi_lines(&popup.output, theme.widget_style)))
            .style(theme.widget_style)
            .scroll((popup.scroll, 0))
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }

    /// Run the popup action bound to `key` for a visible widget.
    ///
    /// The last mouse-scrolled widget wins when it defines the key; otherwise
    /// the first visible widget in config order wins. The clock knows nothing
    /// about action semantics — it only executes the configured command.
    pub(crate) fn open_popup_action(&mut self, key: char) -> bool {
        if self.popup.is_some() {
            return false;
        }
        let Some((widget_index, action_index)) = self.popup_action_target(key) else {
            return false;
        };

        let widget = &self.widgets[widget_index];
        let action = &widget.popup_actions[action_index];
        let title = popup_title(widget, action);
        let command = action.command.clone();
        let timeout = action.timeout;
        let theme = self.current_theme().to_string();
        let tx = self.popup_tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));

        self.popup = Some(WidgetPopup {
            widget_index,
            action_index,
            title,
            output: "Loading...".to_string(),
            scroll: 0,
        });
        self.popup_viewport = Rect::default();
        self.popup_cancel = Some(cancel.clone());
        self.popup_handle = Some(thread::spawn(move || {
            let output = run_command(command, timeout, cancel, theme);
            let _ = tx.send(PopupMessage {
                widget_index,
                action_index,
                output,
            });
        }));
        true
    }

    pub(crate) fn close_popup(&mut self) {
        if let Some(cancel) = self.popup_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.popup_handle.take() {
            let _ = handle.join();
        }
        while self.popup_rx.try_recv().is_ok() {}
        self.popup = None;
        self.popup_viewport = Rect::default();
    }

    pub(crate) fn has_popup_open(&self) -> bool {
        self.popup.is_some()
    }

    pub(crate) fn scroll_popup(&mut self, delta: i16) {
        if let Some(popup) = self.popup.as_mut() {
            popup.scroll_by(delta);
            popup.clamp_scroll(self.popup_viewport);
        }
    }

    pub(crate) fn scroll_at(&mut self, column: u16, row: u16, delta: i16) {
        if self.popup.is_some() {
            self.scroll_popup(delta);
            return;
        }
        if let Some(index) = self.hit_test(column, row) {
            self.active_widget = Some(index);
            if let Some(widget) = self.widgets.get_mut(index) {
                widget.scroll_by(delta);
            }
        }
    }

    pub(crate) fn scroll_active_to_top(&mut self) {
        if let Some(popup) = self.popup.as_mut() {
            popup.scroll = 0;
            return;
        }
        if let Some(widget) = self
            .active_widget
            .and_then(|index| self.widgets.get_mut(index))
        {
            widget.scroll = 0;
        }
    }

    pub(crate) fn scroll_active_to_bottom(&mut self) {
        if let Some(popup) = self.popup.as_mut() {
            popup.scroll = popup.max_scroll(self.popup_viewport);
            return;
        }
        if let Some(index) = self.active_widget {
            if let (Some(widget), Some(area)) =
                (self.widgets.get_mut(index), self.viewports.get(index))
            {
                widget.scroll = widget.max_scroll(*area);
            }
        }
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<usize> {
        self.viewports
            .iter()
            .position(|area| rect_contains(*area, column, row))
    }

    fn popup_action_target(&self, key: char) -> Option<(usize, usize)> {
        self.active_widget
            .filter(|&index| self.widgets.get(index).is_some_and(|widget| widget.visible))
            .and_then(|index| {
                self.widgets[index]
                    .popup_actions
                    .iter()
                    .position(|action| action.key == key)
                    .map(|action_index| (index, action_index))
            })
            .or_else(|| {
                self.widgets
                    .iter()
                    .enumerate()
                    .filter(|(_, widget)| widget.visible)
                    .find_map(|(widget_index, widget)| {
                        widget
                            .popup_actions
                            .iter()
                            .position(|action| action.key == key)
                            .map(|action_index| (widget_index, action_index))
                    })
            })
    }
}

impl Drop for ClockWidgets {
    fn drop(&mut self) {
        self.close_popup();
        self.cancel.store(true, Ordering::Relaxed);
        for widget in &mut self.widgets {
            if let Some(handle) = widget.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl ClockWidget {
    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: ClockTheme) {
        self.clamp_scroll(area);
        let title = self.title();
        let paragraph = Paragraph::new(widget_text(&title, &self.output, theme))
            .style(theme.widget_style)
            .scroll((self.scroll, 0))
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }

    fn scroll_by(&mut self, delta: i16) {
        if delta.is_negative() {
            self.scroll = self.scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(delta as u16);
        }
    }

    fn clamp_scroll(&mut self, area: Rect) {
        self.scroll = self.scroll.min(self.max_scroll(area));
    }

    fn clamp_scroll_to_output(&mut self) {
        let line_count = widget_text_height(&self.title(), &self.output, 1);
        self.scroll = self.scroll.min(line_count.saturating_sub(1));
    }

    fn max_scroll(&self, area: Rect) -> u16 {
        widget_text_height(&self.title(), &self.output, area.width).saturating_sub(area.height)
    }

    fn title(&self) -> String {
        self.title
            .clone()
            .or_else(|| self.command.first().cloned())
            .unwrap_or_else(|| "widget".to_string())
    }
}

impl WidgetPopup {
    fn scroll_by(&mut self, delta: i16) {
        if delta.is_negative() {
            self.scroll = self.scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(delta as u16);
        }
    }

    fn clamp_scroll(&mut self, area: Rect) {
        self.scroll = self.scroll.min(self.max_scroll(area));
    }

    fn max_scroll(&self, area: Rect) -> u16 {
        widget_text_height("", &self.output, area.width).saturating_sub(area.height)
    }
}

fn popup_action(
    config: ClockWidgetPopupActionConfig,
    widget_command: &[String],
    widget_timeout: Duration,
) -> WidgetPopupAction {
    let mut command = if config.command.is_empty() {
        widget_command.to_vec()
    } else {
        config.command
    };
    command.extend(config.args);

    WidgetPopupAction {
        key: config.key,
        label: normalize_optional_text(config.label),
        title: normalize_optional_text(config.title),
        command,
        timeout: config
            .timeout_secs
            .map(|secs| duration_or_default(secs, DEFAULT_TIMEOUT))
            .unwrap_or(widget_timeout),
    }
}

fn popup_title(widget: &ClockWidget, action: &WidgetPopupAction) -> String {
    if let Some(title) = &action.title {
        return title.clone();
    }

    let base = widget
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .or_else(|| {
            widget.command.first().map(|command| {
                Path::new(command)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(command)
                    .to_string()
            })
        })
        .unwrap_or_else(|| "widget".to_string());

    match &action.label {
        Some(label) => format!("{base} · {label}"),
        None => base,
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn centered_popup_area(area: Rect) -> Rect {
    let horizontal_margin = POPUP_MARGIN.saturating_mul(2);
    let vertical_margin = POPUP_MARGIN.saturating_mul(2);
    let width = area
        .width
        .saturating_sub(horizontal_margin)
        .min(POPUP_MAX_WIDTH);
    let height = area
        .height
        .saturating_sub(vertical_margin)
        .min(POPUP_MAX_HEIGHT);

    Rect {
        x: area.x.saturating_add(area.width.saturating_sub(width) / 2),
        y: area
            .y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    }
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn widget_text_height(title: &str, output: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    let total = widget_text(
        title,
        output,
        ClockTheme::named("default", Style::default()),
    )
    .lines
    .iter()
    .fold(0usize, |total, line| {
        total.saturating_add(line.width().max(1).div_ceil(width))
    });

    total.min(u16::MAX as usize) as u16
}

/// Height a bottom-positioned widget needs to show its full output inside a
/// full-width band: wrapped text height at the band's content width, plus the
/// vertical padding `padded_widget_area` will trim.
fn bottom_widget_height(title: &str, output: &str, band_width: u16) -> u16 {
    let x_padding = u16::from(band_width > 2);
    let content_width = band_width.saturating_sub(x_padding * 2).max(1);
    widget_text_height(title, output, content_width).saturating_add(2)
}

fn padded_widget_area(area: Rect) -> Rect {
    let x_padding = u16::from(area.width > 2);
    let y_padding = u16::from(area.height > 2);

    Rect {
        x: area.x + x_padding,
        y: area.y + y_padding,
        width: area.width.saturating_sub(x_padding * 2),
        height: area.height.saturating_sub(y_padding * 2),
    }
}

fn duration_or_default(secs: u64, default: Duration) -> Duration {
    if secs == 0 {
        default
    } else {
        Duration::from_secs(secs)
    }
}

fn normalize_themes(themes: Vec<String>) -> Vec<String> {
    themes
        .into_iter()
        .map(|theme| theme.trim().to_string())
        .filter(|theme| !theme.is_empty())
        .collect()
}

/// Trim a configured group name, treating blank as "no group".
fn normalize_group(group: Option<&str>) -> Option<String> {
    group
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(str::to_string)
}

/// Group cycle order is the order groups first appear in the config, so the
/// first grouped widget decides which group is shown at startup.
fn collect_groups(configs: &[ClockWidgetConfig]) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    for config in configs {
        if let Some(group) = normalize_group(config.group.as_deref()) {
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
    }
    groups
}

fn run_command(
    command: Vec<String>,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
    theme: String,
) -> String {
    if command.is_empty() {
        return "[error] missing command".to_string();
    }

    let mut child_command = Command::new(&command[0]);
    child_command
        .args(&command[1..])
        .env(WIDGET_THEME_ENV, theme)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        child_command.process_group(0);
    }

    let mut child = match child_command.spawn() {
        Ok(child) => child,
        Err(error) => return format!("[error] failed to start {}: {}", command[0], error),
    };

    let stdout = child
        .stdout
        .take()
        .map(|mut stdout| thread::spawn(move || read_to_end(&mut stdout)));
    let stderr = child
        .stderr
        .take()
        .map(|mut stderr| thread::spawn(move || read_to_end(&mut stderr)));

    let started_at = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            terminate_child(&mut child);
            let _ = child.wait();
            let _ = join_output(stdout);
            let _ = join_output(stderr);
            return "[cancelled]".to_string();
        }

        match child.try_wait() {
            Ok(Some(_status)) => {
                let status = match child.wait() {
                    Ok(status) => status,
                    Err(error) => return format!("[error] failed to wait for command: {}", error),
                };
                let stdout = join_output(stdout);
                let stderr = join_output(stderr);
                return format_output(status.success(), stdout, stderr);
            }
            Ok(None) if started_at.elapsed() >= timeout => {
                terminate_child(&mut child);
                let _ = child.wait();
                let _ = join_output(stdout);
                let _ = join_output(stderr);
                return format!("[timeout] command exceeded {}s", timeout.as_secs());
            }
            Ok(None) => thread::sleep(WORKER_POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                return format!("[error] failed to wait for command: {}", error);
            }
        }
    }
}

fn read_to_end(reader: &mut impl Read) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = [0; 8 * 1024];

    loop {
        let bytes_read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(bytes_read) => bytes_read,
        };

        let remaining = MAX_WIDGET_OUTPUT_BYTES.saturating_sub(output.len());
        if remaining > 0 {
            output.extend_from_slice(&buffer[..bytes_read.min(remaining)]);
        }
    }

    output
}

fn join_output(handle: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        terminate_process_group(child.id());
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
        }
    }

    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

#[cfg(unix)]
fn terminate_process_group(pid: u32) {
    let process_group = format!("-{}", pid);
    if silent_kill("-TERM", &process_group) {
        thread::sleep(PROCESS_GROUP_KILL_GRACE);
        let _ = silent_kill("-KILL", &process_group);
    }
}

#[cfg(unix)]
fn silent_kill(signal: &str, process_group: &str) -> bool {
    Command::new("kill")
        .arg(signal)
        .arg(process_group)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn terminate_process_group(_pid: u32) {}

fn format_output(success: bool, stdout: Vec<u8>, stderr: Vec<u8>) -> String {
    let stdout = normalize_output(&String::from_utf8_lossy(&stdout));
    let stderr = normalize_output(&String::from_utf8_lossy(&stderr));

    if success {
        if stdout.is_empty() {
            "[ok]".to_string()
        } else {
            stdout
        }
    } else if stderr.is_empty() {
        format!("[error] {}", stdout)
    } else {
        format!("[error] {}", stderr)
    }
}

fn normalize_output(output: &str) -> String {
    output.replace('\r', "").trim_end().to_string()
}

fn widget_text(title: &str, output: &str, theme: ClockTheme) -> Text<'static> {
    // An explicitly empty title (`title = ""`) suppresses the title line so the
    // command output fully owns the widget area (e.g. self-rendered headers).
    let mut lines = if title.is_empty() {
        Vec::new()
    } else {
        vec![Line::from(vec![
            Span::styled("● ".to_string(), theme.widget_marker_style),
            Span::styled(title.to_string(), theme.widget_title_style),
        ])]
    };

    lines.extend(ansi_lines(output, theme.widget_style));
    Text::from(lines)
}

fn ansi_lines(output: &str, base_style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut spans = Vec::new();
    let mut text = String::new();
    let mut style = base_style;
    let mut chars = output.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\n' {
            push_span(&mut spans, &mut text, style);
            lines.push(Line::from(spans));
            spans = Vec::new();
        } else if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    let mut sequence = String::new();
                    let mut final_byte = None;
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            final_byte = Some(next);
                            break;
                        }
                        sequence.push(next);
                    }

                    if final_byte == Some('m') {
                        push_span(&mut spans, &mut text, style);
                        style = apply_sgr_sequence(style, base_style, &sequence);
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }

                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if ch == '\t' {
            text.push_str("    ");
        } else if !ch.is_control() {
            // Widget output can include logs or other externally influenced
            // text. Never pass raw C0/C1 controls through the terminal backend.
            text.push(ch);
        }
    }

    push_span(&mut spans, &mut text, style);
    lines.push(Line::from(spans));
    lines
}

fn push_span(spans: &mut Vec<Span<'static>>, text: &mut String, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(std::mem::take(text), style));
    }
}

fn apply_sgr_sequence(mut style: Style, base_style: Style, sequence: &str) -> Style {
    // Numeric SGR values are standardized ANSI escape codes. Keep the raw values
    // here so the match mirrors terminal documentation (0 reset, 30-37 fg, etc.).
    let values = parse_sgr_values(sequence);
    let mut idx = 0;

    while idx < values.len() {
        match values[idx] {
            0 => style = base_style,
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            21 | 24 => style = remove_sgr_modifier(style, Modifier::UNDERLINED),
            22 => style = remove_sgr_modifier(style, Modifier::BOLD | Modifier::DIM),
            23 => style = remove_sgr_modifier(style, Modifier::ITALIC),
            27 => style = remove_sgr_modifier(style, Modifier::REVERSED),
            29 => style = remove_sgr_modifier(style, Modifier::CROSSED_OUT),
            30..=37 => style = style.fg(ansi_color(values[idx], false)),
            39 => style.fg = base_style.fg,
            40..=47 => style = style.bg(ansi_color(values[idx] - 10, false)),
            49 => style.bg = base_style.bg,
            90..=97 => style = style.fg(ansi_color(values[idx] - 60, true)),
            100..=107 => style = style.bg(ansi_color(values[idx] - 70, true)),
            38 if values.get(idx + 1) == Some(&5) => {
                if let Some(color) = values
                    .get(idx + 2)
                    .and_then(|value| u8::try_from(*value).ok())
                {
                    style = style.fg(Color::Indexed(color));
                    idx += 2;
                }
            }
            38 if values.get(idx + 1) == Some(&2) => {
                if let (Some(r), Some(g), Some(b)) = (
                    values
                        .get(idx + 2)
                        .and_then(|value| u8::try_from(*value).ok()),
                    values
                        .get(idx + 3)
                        .and_then(|value| u8::try_from(*value).ok()),
                    values
                        .get(idx + 4)
                        .and_then(|value| u8::try_from(*value).ok()),
                ) {
                    style = style.fg(Color::Rgb(r, g, b));
                    idx += 4;
                }
            }
            48 if values.get(idx + 1) == Some(&5) => {
                if let Some(color) = values
                    .get(idx + 2)
                    .and_then(|value| u8::try_from(*value).ok())
                {
                    style = style.bg(Color::Indexed(color));
                    idx += 2;
                }
            }
            48 if values.get(idx + 1) == Some(&2) => {
                if let (Some(r), Some(g), Some(b)) = (
                    values
                        .get(idx + 2)
                        .and_then(|value| u8::try_from(*value).ok()),
                    values
                        .get(idx + 3)
                        .and_then(|value| u8::try_from(*value).ok()),
                    values
                        .get(idx + 4)
                        .and_then(|value| u8::try_from(*value).ok()),
                ) {
                    style = style.bg(Color::Rgb(r, g, b));
                    idx += 4;
                }
            }
            _ => {}
        }
        idx += 1;
    }

    style
}

fn remove_sgr_modifier(mut style: Style, modifier: Modifier) -> Style {
    style.add_modifier.remove(modifier);
    style.sub_modifier.insert(modifier);
    style
}

fn parse_sgr_values(sequence: &str) -> Vec<u16> {
    if sequence.is_empty() {
        return vec![0];
    }

    sequence
        .split(';')
        .map(|value| value.parse::<u16>().unwrap_or(0))
        .collect()
}

fn ansi_color(value: u16, bright: bool) -> Color {
    match (value, bright) {
        (30, false) => Color::Black,
        (31, false) => Color::Red,
        (32, false) => Color::Green,
        (33, false) => Color::Yellow,
        (34, false) => Color::Blue,
        (35, false) => Color::Magenta,
        (36, false) => Color::Cyan,
        (37, false) => Color::Gray,
        (30, true) => Color::DarkGray,
        (31, true) => Color::LightRed,
        (32, true) => Color::LightGreen,
        (33, true) => Color::LightYellow,
        (34, true) => Color::LightBlue,
        (35, true) => Color::LightMagenta,
        (36, true) => Color::LightCyan,
        (37, true) => Color::White,
        _ => Color::Reset,
    }
}

pub(crate) fn visible_widget_count(area: Rect, configured_count: usize) -> usize {
    configured_count.min(max_widgets_for_area(area))
}

pub(crate) fn max_widgets_for_area(area: Rect) -> usize {
    let height = area.height.max(1) as f32;
    let aspect = area.width as f32 / (height * CELL_HEIGHT_TO_WIDTH_RATIO);

    if aspect < WIDE_TERMINAL_ASPECT {
        SQUARE_TERMINAL_WIDGETS
    } else if aspect < ULTRAWIDE_TERMINAL_ASPECT {
        WIDE_TERMINAL_WIDGETS
    } else {
        ULTRAWIDE_TERMINAL_WIDGETS
    }
}

pub(crate) fn clock_size_for_area(text_len: usize, area: Rect, has_header: bool) -> u16 {
    if text_len == 0 {
        return 1;
    }

    let spacing = CLOCK_CHARACTER_SPACING.saturating_mul(text_len.saturating_sub(1) as u16);
    let width_budget =
        area.width
            .saturating_sub(spacing.saturating_add(CLOCK_HORIZONTAL_MARGIN)) as usize;
    let width_size = width_budget / (CLOCK_GLYPH_COLUMNS * text_len);

    let height_budget = area
        .height
        .saturating_sub(clock_vertical_padding(has_header)) as usize;
    let height_size = height_budget / CLOCK_GLYPH_ROWS;

    width_size.min(height_size).max(1) as u16
}

pub(crate) fn clock_height_for_size(size: u16, has_header: bool) -> u16 {
    size.saturating_mul(CLOCK_GLYPH_ROWS as u16)
        .saturating_add(clock_vertical_padding(has_header))
}

fn clock_vertical_padding(has_header: bool) -> u16 {
    if has_header {
        CLOCK_HEADER_VERTICAL_PADDING
    } else {
        CLOCK_NO_HEADER_VERTICAL_PADDING
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn max_widgets_follows_terminal_aspect_ratio() {
        assert_eq!(max_widgets_for_area(Rect::new(0, 0, 100, 50)), 2);
        assert_eq!(max_widgets_for_area(Rect::new(0, 0, 160, 45)), 4);
        assert_eq!(max_widgets_for_area(Rect::new(0, 0, 240, 50)), 6);
    }

    #[test]
    fn visible_widgets_never_exceed_configured_count() {
        let area = Rect::new(0, 0, 240, 50);
        assert_eq!(visible_widget_count(area, 0), 0);
        assert_eq!(visible_widget_count(area, 3), 3);
        assert_eq!(visible_widget_count(area, 10), 6);
    }

    #[test]
    fn empty_title_suppresses_title_line() {
        let untitled = widget_text("", "hello\nworld", default_clock_theme());
        assert_eq!(untitled.lines.len(), 2);

        let titled = widget_text("Health", "hello\nworld", default_clock_theme());
        assert_eq!(titled.lines.len(), 3);

        assert_eq!(widget_text_height("", "one\ntwo", 80), 2);
    }

    #[test]
    fn widget_title_distinguishes_empty_omitted_and_missing() {
        let widgets = ClockWidgets::new(
            vec![
                ClockWidgetConfig {
                    title: Some(String::new()),
                    command: vec!["self-header".to_string()],
                    popup_actions: Vec::new(),
                    refresh_secs: DEFAULT_WIDGET_REFRESH_SECS,
                    timeout_secs: DEFAULT_WIDGET_TIMEOUT_SECS,
                    position: WidgetPosition::Auto,
                    group: None,
                },
                ClockWidgetConfig {
                    title: None,
                    command: vec!["fallback-command".to_string()],
                    popup_actions: Vec::new(),
                    refresh_secs: DEFAULT_WIDGET_REFRESH_SECS,
                    timeout_secs: DEFAULT_WIDGET_TIMEOUT_SECS,
                    position: WidgetPosition::Auto,
                    group: None,
                },
                ClockWidgetConfig {
                    title: None,
                    command: Vec::new(),
                    popup_actions: Vec::new(),
                    refresh_secs: DEFAULT_WIDGET_REFRESH_SECS,
                    timeout_secs: DEFAULT_WIDGET_TIMEOUT_SECS,
                    position: WidgetPosition::Auto,
                    group: None,
                },
            ],
            default_themes(),
        );

        assert_eq!(widgets.widgets[0].title(), "");
        assert_eq!(widgets.widgets[1].title(), "fallback-command");
        assert_eq!(widgets.widgets[2].title(), "widget");
    }

    #[test]
    fn bottom_widget_renders_full_width_under_the_row() {
        let mut widgets = ClockWidgets::new(
            vec![
                widget_config("one"),
                widget_config("two"),
                bottom_widget_config("health"),
            ],
            default_themes(),
        );
        widgets.widgets[2].output = numbered_lines(4);
        let area = Rect::new(0, 2, 80, 38);
        let terminal_area = Rect::new(0, 0, 80, 40);
        let mut buffer = Buffer::empty(terminal_area);

        widgets.render(area, terminal_area, &mut buffer, default_clock_theme());

        // row widgets side by side on top
        assert!(widgets.widgets[0].visible);
        assert!(widgets.widgets[1].visible);
        assert_eq!(widgets.viewports[0].y, widgets.viewports[1].y);
        assert!(widgets.viewports[1].x > widgets.viewports[0].x);

        // bottom widget: full band width, below the row, sized to content
        assert!(widgets.widgets[2].visible);
        let bottom = widgets.viewports[2];
        assert!(bottom.y > widgets.viewports[0].y);
        // band width minus padding
        assert_eq!(bottom.width, area.width - 2);
        // title + 4 output lines
        assert_eq!(bottom.height, 5);
        // band ends at the bottom edge of the widget area
        assert_eq!(bottom.y + bottom.height + 1, area.y + area.height);
    }

    #[test]
    fn bottom_widgets_do_not_count_against_row_widget_limit() {
        let mut configs = (0..ULTRAWIDE_TERMINAL_WIDGETS)
            .map(|index| widget_config(&format!("row-{index}")))
            .collect::<Vec<_>>();
        configs.push(bottom_widget_config("health"));
        let mut widgets = ClockWidgets::new(configs, default_themes());
        widgets.widgets[ULTRAWIDE_TERMINAL_WIDGETS].output = "ok".to_string();
        let area = Rect::new(0, 0, 240, 50);
        let mut buffer = Buffer::empty(area);

        widgets.render(area, area, &mut buffer, default_clock_theme());

        assert_eq!(widgets.widgets.len(), ULTRAWIDE_TERMINAL_WIDGETS + 1);
        assert!(widgets
            .widgets
            .iter()
            .take(ULTRAWIDE_TERMINAL_WIDGETS)
            .all(|widget| widget.visible));
        assert!(widgets.widgets[ULTRAWIDE_TERMINAL_WIDGETS].visible);
        assert!(widgets.viewports[ULTRAWIDE_TERMINAL_WIDGETS].y > widgets.viewports[0].y);
    }

    #[test]
    fn bottom_widget_alone_takes_band_without_row() {
        let mut widgets = ClockWidgets::new(vec![bottom_widget_config("health")], default_themes());
        widgets.widgets[0].output = numbered_lines(3);
        let area = Rect::new(0, 0, 60, 30);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 30));

        widgets.render(area, area, &mut buffer, default_clock_theme());

        assert!(widgets.widgets[0].visible);
        let viewport = widgets.viewports[0];
        assert_eq!(viewport.height, 4); // title + 3 lines
        assert_eq!(viewport.y + viewport.height + 1, area.height);
    }

    #[test]
    fn bottom_widget_is_capped_by_min_row_height() {
        let mut widgets = ClockWidgets::new(
            vec![widget_config("one"), bottom_widget_config("health")],
            default_themes(),
        );
        widgets.widgets[1].output = numbered_lines(100);
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 20));

        widgets.render(area, area, &mut buffer, default_clock_theme());

        // the row keeps its minimum height even with oversized bottom output
        assert!(widgets.viewports[0].height >= MIN_WIDGET_ROW_HEIGHT - 2);
        let bottom = widgets.viewports[1];
        assert!(bottom.height <= area.height - MIN_WIDGET_ROW_HEIGHT);
        assert!(widgets.widgets[1].visible);
    }

    #[test]
    fn hidden_bottom_widget_does_not_run_or_hit_test() {
        let mut widgets = ClockWidgets::new(
            vec![widget_config("one"), bottom_widget_config("health")],
            default_themes(),
        );
        widgets.widgets[1].output = numbered_lines(5);
        // area too small to grant the bottom widget its minimum height
        let area = Rect::new(0, 0, 40, 9);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 9));

        widgets.render(area, area, &mut buffer, default_clock_theme());

        assert!(!widgets.widgets[1].visible);
        assert_eq!(widgets.viewports[1], Rect::default());
        assert_eq!(widgets.hit_test(5, 5), Some(0));
    }

    #[test]
    fn widget_hit_testing_uses_last_rendered_viewports() {
        let mut widgets = ClockWidgets::new(
            vec![widget_config("one"), widget_config("two")],
            default_themes(),
        );
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 10));

        widgets.render(
            Rect::new(0, 2, 40, 8),
            Rect::new(0, 0, 40, 10),
            &mut buffer,
            default_clock_theme(),
        );

        assert_eq!(widgets.viewports.len(), 2);
        assert_eq!(widgets.hit_test(1, 3), Some(0));
        assert_eq!(widgets.hit_test(21, 3), Some(1));
        assert_eq!(widgets.hit_test(0, 0), None);
    }

    #[test]
    fn widget_scroll_clamps_and_home_end_target_active_widget() {
        let mut widgets = ClockWidgets::new(
            vec![widget_config("one"), widget_config("two")],
            default_themes(),
        );
        widgets.widgets[0].output = numbered_lines(20);
        widgets.widgets[1].output = numbered_lines(20);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 8));
        widgets.render(
            Rect::new(0, 0, 40, 8),
            Rect::new(0, 0, 40, 8),
            &mut buffer,
            default_clock_theme(),
        );

        widgets.scroll_at(1, 1, 50);
        widgets.render(
            Rect::new(0, 0, 40, 8),
            Rect::new(0, 0, 40, 8),
            &mut buffer,
            default_clock_theme(),
        );
        assert_eq!(widgets.active_widget, Some(0));
        assert_eq!(
            widgets.widgets[0].scroll,
            widgets.widgets[0].max_scroll(widgets.viewports[0])
        );
        assert_eq!(widgets.widgets[1].scroll, 0);

        widgets.scroll_active_to_top();
        assert_eq!(widgets.widgets[0].scroll, 0);
        widgets.scroll_active_to_bottom();
        assert_eq!(
            widgets.widgets[0].scroll,
            widgets.widgets[0].max_scroll(widgets.viewports[0])
        );
    }

    #[test]
    fn widget_text_height_saturates_for_large_wrapped_lines() {
        let output = "x".repeat(u16::MAX as usize + 1);

        assert_eq!(widget_text_height("huge", &output, 1), u16::MAX);
    }

    #[test]
    fn clock_size_fits_width_and_height() {
        let area = Rect::new(0, 0, 80, 20);
        let size = clock_size_for_area(8, area, true);
        let width = 8 * (CLOCK_GLYPH_COLUMNS as u16 * size + CLOCK_CHARACTER_SPACING)
            - CLOCK_CHARACTER_SPACING;
        let height = clock_height_for_size(size, true);

        assert!(width <= area.width);
        assert!(height <= area.height);
    }

    #[test]
    fn groups_cycle_in_first_appearance_order() {
        let mut widgets = ClockWidgets::new(
            vec![
                grouped_widget_config("weather", Some("weather")),
                grouped_widget_config("github", Some("github")),
                grouped_widget_config("forecast", Some("weather")),
            ],
            default_themes(),
        );

        // First group in config order is the one shown at startup.
        assert_eq!(widgets.current_group_for_test(), Some("weather"));

        widgets.cycle_group();
        assert_eq!(widgets.current_group_for_test(), Some("github"));

        // Only two distinct groups, so it wraps back rather than visiting
        // "weather" twice for the two weather widgets.
        widgets.cycle_group();
        assert_eq!(widgets.current_group_for_test(), Some("weather"));
    }

    #[test]
    fn render_shows_active_group_and_always_shows_ungrouped() {
        let mut widgets = ClockWidgets::new(
            vec![
                grouped_widget_config("weather", Some("weather")),
                grouped_widget_config("github", Some("github")),
                grouped_widget_config("calendar", None),
            ],
            default_themes(),
        );
        let area = Rect::new(0, 0, 80, 30);
        let mut buffer = Buffer::empty(area);

        widgets.render(area, area, &mut buffer, default_clock_theme());
        assert!(widgets.widgets[0].visible, "active group is rendered");
        assert!(!widgets.widgets[1].visible, "inactive group is hidden");
        assert!(widgets.widgets[2].visible, "ungrouped is always rendered");

        widgets.cycle_group();
        widgets.render(area, area, &mut buffer, default_clock_theme());
        assert!(!widgets.widgets[0].visible);
        assert!(widgets.widgets[1].visible);
        assert!(widgets.widgets[2].visible);
    }

    #[test]
    fn hidden_group_keeps_its_output_across_a_cycle() {
        let mut widgets = ClockWidgets::new(
            vec![
                grouped_widget_config("weather", Some("weather")),
                grouped_widget_config("github", Some("github")),
            ],
            default_themes(),
        );
        widgets.widgets[0].output = "22C sunny".to_string();

        widgets.cycle_group();
        widgets.cycle_group();

        // Unlike a theme cycle, switching groups must not reset output to
        // "Loading..." — returning to a group should be instant.
        assert_eq!(widgets.widgets[0].output, "22C sunny");
    }

    #[test]
    fn cycling_group_before_render_does_not_refresh_previous_group() {
        let mut widgets = ClockWidgets::new(
            vec![
                grouped_widget_config("weather", Some("weather")),
                grouped_widget_config("github", Some("github")),
            ],
            default_themes(),
        );

        widgets.widgets[0].visible = true;
        assert_eq!(widgets.current_group_for_test(), Some("weather"));

        widgets.cycle_group();
        widgets.tick();

        assert_eq!(widgets.current_group_for_test(), Some("github"));
        assert!(!widgets.widgets[0].running);
        assert!(!widgets.widgets[1].running);
    }

    #[test]
    fn hiding_all_widgets_stops_refreshes_and_preserves_layout_state() {
        let mut weather = grouped_widget_config("weather", Some("weather"));
        weather.popup_actions = vec![popup_action_config('d', &["details"])];
        let mut widgets = ClockWidgets::new(
            vec![weather, grouped_widget_config("github", Some("github"))],
            default_themes(),
        );
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        widgets.render(area, area, &mut buffer, default_clock_theme());
        widgets.widgets[0].output = "22C sunny".to_string();

        widgets.hide_all();
        widgets.tick();

        assert!(widgets.widgets.iter().all(|widget| !widget.visible));
        assert!(widgets.widgets.iter().all(|widget| !widget.running));
        assert!(widgets
            .viewports
            .iter()
            .all(|area| *area == Rect::default()));
        assert_eq!(widgets.active_widget, None);
        assert!(!widgets.open_popup_action('d'));
        assert_eq!(widgets.current_group_for_test(), Some("weather"));
        assert_eq!(widgets.widgets[0].output, "22C sunny");

        widgets.render(area, area, &mut buffer, default_clock_theme());
        assert!(widgets.widgets[0].visible);
        assert!(!widgets.widgets[1].visible);
        assert_eq!(widgets.widgets[0].output, "22C sunny");
    }

    #[test]
    fn cycling_is_inert_without_at_least_two_groups() {
        let mut widgets = ClockWidgets::new(
            vec![widget_config("a"), widget_config("b")],
            default_themes(),
        );
        let area = Rect::new(0, 0, 80, 30);
        let mut buffer = Buffer::empty(area);

        widgets.cycle_group();

        assert_eq!(widgets.current_group_for_test(), None);
        widgets.render(area, area, &mut buffer, default_clock_theme());
        assert!(widgets.widgets.iter().all(|widget| widget.visible));
    }

    #[test]
    fn blank_group_is_treated_as_ungrouped() {
        let widgets = ClockWidgets::new(
            vec![
                grouped_widget_config("spaces", Some("   ")),
                grouped_widget_config("real", Some("  github  ")),
            ],
            default_themes(),
        );

        assert_eq!(widgets.widgets[0].group, None);
        assert_eq!(widgets.widgets[1].group.as_deref(), Some("github"));
        assert_eq!(widgets.groups, vec!["github".to_string()]);
    }

    fn widget_config(title: &str) -> ClockWidgetConfig {
        ClockWidgetConfig {
            title: Some(title.to_string()),
            command: vec![title.to_string()],
            popup_actions: Vec::new(),
            refresh_secs: DEFAULT_WIDGET_REFRESH_SECS,
            timeout_secs: DEFAULT_WIDGET_TIMEOUT_SECS,
            position: WidgetPosition::Auto,
            group: None,
        }
    }

    fn popup_action_config(key: char, args: &[&str]) -> ClockWidgetPopupActionConfig {
        ClockWidgetPopupActionConfig {
            key,
            label: Some("details".to_string()),
            title: None,
            command: Vec::new(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            timeout_secs: None,
        }
    }

    fn grouped_widget_config(title: &str, group: Option<&str>) -> ClockWidgetConfig {
        ClockWidgetConfig {
            group: group.map(str::to_string),
            ..widget_config(title)
        }
    }

    fn default_themes() -> Vec<String> {
        vec![
            "default".to_string(),
            "evangelion".to_string(),
            "nerv".to_string(),
        ]
    }

    fn default_clock_theme() -> ClockTheme {
        ClockTheme::named("default", Style::default())
    }

    #[test]
    fn command_receives_current_widget_theme_env() {
        let output = run_command(
            vec![
                "sh".to_string(),
                "-c".to_string(),
                "printf %s \"$TCLOCK_WIDGET_THEME\"".to_string(),
            ],
            DEFAULT_TIMEOUT,
            Arc::new(AtomicBool::new(false)),
            "nerv".to_string(),
        );

        assert_eq!(output, "nerv");
    }

    #[test]
    fn popup_actions_are_generic_commands_with_widget_defaults() {
        let mut config = widget_config("health");
        config.command = vec!["printf".to_string()];
        config.timeout_secs = 7;
        config.popup_actions = vec![popup_action_config('d', &["failure details"])];

        let widgets = ClockWidgets::new(vec![config], default_themes());
        let action = &widgets.widgets[0].popup_actions[0];

        assert_eq!(action.key, 'd');
        assert_eq!(action.command, vec!["printf", "failure details"]);
        assert_eq!(action.timeout, Duration::from_secs(7));
        assert_eq!(popup_title(&widgets.widgets[0], action), "health · details");
    }

    #[test]
    fn popup_action_can_replace_command_and_timeout() {
        let mut config = widget_config("health");
        config.popup_actions = vec![ClockWidgetPopupActionConfig {
            key: 'x',
            label: None,
            title: Some("Custom diagnostics".to_string()),
            command: vec!["printf".to_string(), "%s".to_string()],
            args: vec!["alternate".to_string()],
            timeout_secs: Some(3),
        }];

        let widgets = ClockWidgets::new(vec![config], default_themes());
        let action = &widgets.widgets[0].popup_actions[0];

        assert_eq!(action.command, vec!["printf", "%s", "alternate"]);
        assert_eq!(action.timeout, Duration::from_secs(3));
        assert_eq!(
            popup_title(&widgets.widgets[0], action),
            "Custom diagnostics"
        );
    }

    #[test]
    fn visible_widget_popup_action_runs_and_closes() {
        let mut config = widget_config("health");
        config.command = vec!["printf".to_string()];
        config.popup_actions = vec![popup_action_config('d', &["failed unit: example.service"])];
        let mut widgets = ClockWidgets::new(vec![config], default_themes());
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        widgets.render(area, area, &mut buffer, default_clock_theme());

        assert!(widgets.open_popup_action('d'));
        assert!(widgets.has_popup_open());
        let deadline = Instant::now() + Duration::from_secs(2);
        while widgets
            .popup
            .as_ref()
            .is_some_and(|popup| popup.output == "Loading...")
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
            widgets.tick();
        }

        assert_eq!(
            widgets.popup.as_ref().map(|popup| popup.output.as_str()),
            Some("failed unit: example.service")
        );
        widgets.render_popup(area, &mut buffer, default_clock_theme());
        assert!(widgets.popup_viewport.width > 0);

        widgets.close_popup();
        assert!(!widgets.has_popup_open());
    }

    #[test]
    fn active_widget_wins_when_popup_keys_overlap() {
        let mut first = widget_config("first");
        first.popup_actions = vec![popup_action_config('d', &["first"])];
        let mut second = widget_config("second");
        second.popup_actions = vec![popup_action_config('d', &["second"])];
        let mut widgets = ClockWidgets::new(vec![first, second], default_themes());
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        widgets.render(area, area, &mut buffer, default_clock_theme());

        widgets.scroll_at(60, 5, 0);
        assert!(widgets.open_popup_action('d'));

        assert_eq!(
            widgets.popup.as_ref().map(|popup| popup.widget_index),
            Some(1)
        );
    }

    #[test]
    fn popup_area_is_centered_and_bounded() {
        assert_eq!(
            centered_popup_area(Rect::new(10, 5, 200, 60)),
            Rect::new(55, 20, POPUP_MAX_WIDTH, POPUP_MAX_HEIGHT)
        );
        assert_eq!(
            centered_popup_area(Rect::new(0, 0, 80, 24)),
            Rect::new(2, 2, 76, 20)
        );
    }

    #[test]
    fn cycling_theme_refreshes_visible_widgets_and_ignores_single_theme() {
        let mut widgets = ClockWidgets::new(
            vec![widget_config("one"), widget_config("hidden")],
            default_themes(),
        );
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        widgets.render(area, area, &mut buffer, default_clock_theme());
        widgets.widgets[0].output = "old".to_string();
        widgets.widgets[0].next_run = Instant::now() + Duration::from_secs(60);
        widgets.widgets[1].visible = false;
        widgets.widgets[1].output = "old hidden".to_string();
        widgets.widgets[1].next_run = Instant::now() + Duration::from_secs(60);

        widgets.cycle_theme();

        assert_eq!(widgets.current_theme(), "evangelion");
        assert_eq!(widgets.widgets[0].output, "Loading...");
        assert!(widgets.widgets[0].next_run <= Instant::now());
        assert_eq!(widgets.widgets[1].output, "old hidden");
        assert!(widgets.widgets[1].next_run <= Instant::now());

        let mut single = ClockWidgets::new(vec![widget_config("one")], vec!["only".to_string()]);
        single.render(area, area, &mut buffer, default_clock_theme());
        single.widgets[0].output = "old".to_string();
        single.cycle_theme();
        assert_eq!(single.current_theme(), "only");
        assert_eq!(single.widgets[0].output, "old");
    }

    fn bottom_widget_config(title: &str) -> ClockWidgetConfig {
        ClockWidgetConfig {
            position: WidgetPosition::Bottom,
            ..widget_config(title)
        }
    }

    fn numbered_lines(count: usize) -> String {
        (0..count)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn clock_size_leaves_vertical_breathing_without_header() {
        let area = Rect::new(0, 0, 500, 25);
        let size = clock_size_for_area(8, area, false);
        let height = clock_height_for_size(size, false);

        assert!(height <= area.height);
        assert_eq!(size, 4);
    }

    #[test]
    fn clock_size_handles_tiny_areas() {
        assert_eq!(clock_size_for_area(8, Rect::new(0, 0, 1, 1), true), 1);
    }

    #[test]
    fn widget_output_preserves_ansi_for_rendering() {
        let output = format_output(true, b"\x1b[2mok\x1b[0m\r\n".to_vec(), Vec::new());

        assert_eq!(output, "\x1b[2mok\x1b[0m");
    }

    #[test]
    fn widget_output_reader_caps_large_outputs() {
        let input = vec![b'a'; MAX_WIDGET_OUTPUT_BYTES + 1024];
        let mut reader = Cursor::new(input);

        let output = read_to_end(&mut reader);

        assert_eq!(output.len(), MAX_WIDGET_OUTPUT_BYTES);
    }

    #[test]
    fn ansi_lines_render_sgr_as_styled_spans() {
        let lines = ansi_lines(
            "\x1b[2m19 projects checked\x1b[0m\n\x1b[36m\x1b[1mrepo/name\x1b[0m",
            Style::default(),
        );

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "19 projects checked");
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[1].spans[0].content, "repo/name");
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Cyan));
        assert!(lines[1].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn ansi_reset_restores_themed_widget_base_style() {
        let base_style = Style::default().fg(Color::LightCyan);
        let lines = ansi_lines("base \x1b[31mred\x1b[0m base", base_style);

        assert_eq!(lines[0].spans[0].style.fg, Some(Color::LightCyan));
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
        assert_eq!(lines[0].spans[2].style.fg, Some(Color::LightCyan));
    }

    #[test]
    fn widget_title_chrome_uses_clock_theme_styles() {
        let text = widget_text("Health", "ok", ClockTheme::named("nerv", Style::default()));

        assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::Indexed(48)));
        assert_eq!(text.lines[0].spans[1].style.fg, Some(Color::Indexed(214)));
        assert_eq!(text.lines[1].spans[0].style.fg, Some(Color::Indexed(172)));
    }

    #[test]
    fn evangelion_theme_preserves_original_nerv_palette() {
        let text = widget_text(
            "Health",
            "ok",
            ClockTheme::named("evangelion", Style::default()),
        );

        assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::Indexed(118)));
        assert_eq!(text.lines[0].spans[1].style.fg, Some(Color::Indexed(99)));
        assert_eq!(text.lines[1].spans[0].style.fg, Some(Color::Indexed(103)));
    }

    #[test]
    fn ansi_lines_skip_osc_terminated_by_st() {
        let lines = ansi_lines(
            "\x1b]8;;https://example.test\x1b\\repo/name\x1b]8;;\x1b\\ ok",
            Style::default(),
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "repo/name ok");
    }

    #[test]
    fn ansi_lines_strip_raw_control_characters() {
        let lines = ansi_lines("left\x07\x08\tcenter\u{009b}right", Style::default());

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "left    centerright");
    }

    #[test]
    fn ansi_lines_render_common_sgr_attributes() {
        let lines = ansi_lines(
            "\x1b[3;4;9;42;48;5;196mstyled\x1b[23;24;29;49m plain",
            Style::default(),
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "styled");
        assert_eq!(lines[0].spans[0].style.bg, Some(Color::Indexed(196)));
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::ITALIC | Modifier::UNDERLINED | Modifier::CROSSED_OUT));
        assert_eq!(lines[0].spans[1].content, " plain");
        assert_eq!(lines[0].spans[1].style.bg, None);
        assert!(!lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::ITALIC | Modifier::UNDERLINED | Modifier::CROSSED_OUT));
    }
}
