use std::sync::OnceLock;

use chrono::DateTime;
use chrono::Duration;
use chrono::Local;
use chrono::LocalResult;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::NaiveTime;
use chrono::TimeZone;
use chrono_tz::Tz;
use clap::Subcommand;
use crossterm::event::KeyCode;
use ratatui::{
    style::{Color, Style},
    Frame,
};
use regex::Regex;

use self::modes::Clock;
use self::modes::Countdown;
use self::modes::DurationFormat;
use self::modes::Pause;
use self::modes::Stopwatch;
use self::modes::Timer;

pub mod modes;

#[derive(Debug, Subcommand)]
pub enum Mode {
    /// The clock mode displays the current time, the default mode.
    Clock {
        /// Custom timezone, for example "America/New_York"; uses the local timezone if not specified
        #[arg(short = 'z', long, value_parser = parse_timezone)]
        timezone: Option<Tz>,
    },
    /// The timer mode displays the remaining time until the timer is finished.
    Timer {
        /// Initial duration for timer, value can be 10s for 10 seconds, 1m for 1 minute, etc.
        /// Also accepts multiple duration values and runs the timers sequentially, eg. 25m 5m.
        /// Falls back to `[timer] durations` from the config, then 5m.
        #[arg(short, long = "duration", value_parser = parse_duration, num_args = 1..)]
        durations: Vec<Duration>,

        /// Restart the timer when timer is over
        #[arg(long, short, action)]
        repeat: bool,

        /// Auto quit when time is up
        #[arg(long = "quit", short = 'Q', action)]
        auto_quit: bool,

        /// Command to run when the timer ends
        #[arg(long, short, num_args = 1.., allow_hyphen_values = true)]
        execute: Vec<String>,
    },
    /// The stopwatch mode displays the elapsed time since it was started.
    Stopwatch,
    /// The countdown timer mode shows the duration to a specific time
    Countdown {
        /// The target time to countdown to, eg. "2023-01-01", "20:00", "2022-12-25 20:00:00" or "2022-12-25T20:00:00-04:00"
        #[arg(long, short, value_parser = parse_datetime)]
        time: DateTime<Local>,

        /// Continue counting down after passing the target time
        #[arg(long = "continue", short = 'C', action)]
        continue_on_zero: bool,

        /// Reverse the countdown, a.k.a. countup
        #[arg(long, short, action)]
        reverse: bool,
    },
}

/// Display options accepted by every mode, before or after the mode name.
///
/// Each flag is optional. When absent, the value comes from the mode's config
/// section, then `[default]`, then the built-in default for that mode.
#[derive(clap::Args, Debug, Default, Clone, PartialEq, Eq)]
pub struct DisplayArgs {
    /// Header text shown above the digits. Timer mode accepts one title per duration.
    #[arg(short = 'T', long = "title", value_name = "TITLE", num_args = 1.., action = clap::ArgAction::Append, global = true)]
    pub titles: Vec<String>,

    /// Show seconds (overrides `show_seconds = false` in the config)
    #[arg(long, action, global = true, overrides_with = "no_seconds")]
    pub seconds: bool,

    /// Hide seconds; every mode then shows hours and minutes only
    #[arg(short = 'S', long, action, global = true, overrides_with = "seconds")]
    pub no_seconds: bool,

    /// Show fractional seconds
    #[arg(short = 'm', long, action, global = true, overrides_with = "no_millis")]
    pub millis: bool,

    /// Hide fractional seconds
    #[arg(short = 'M', long, action, global = true, overrides_with = "millis")]
    pub no_millis: bool,

    /// Show the date line (clock mode)
    #[arg(long, action, global = true, overrides_with = "no_date")]
    pub date: bool,

    /// Hide the date line (clock mode)
    #[arg(short = 'D', long, action, global = true, overrides_with = "date")]
    pub no_date: bool,

    /// Start paused (timer and stopwatch modes)
    #[arg(short = 'P', long, action, global = true)]
    pub paused: bool,
}

impl DisplayArgs {
    fn show_seconds(&self) -> Option<bool> {
        flag_pair(self.seconds, self.no_seconds)
    }

    fn show_millis(&self) -> Option<bool> {
        flag_pair(self.millis, self.no_millis)
    }

    fn show_date(&self) -> Option<bool> {
        flag_pair(self.date, self.no_date)
    }

    fn start_paused(&self) -> Option<bool> {
        self.paused.then_some(true)
    }
}

/// Collapse a `--flag` / `--no-flag` pair into an explicit choice, or `None`
/// when neither was given. clap's `overrides_with` guarantees at most one is set.
fn flag_pair(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

/// Built-in display defaults for one mode, used when neither the command line
/// nor the config says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModeDefaults {
    show_date: bool,
    show_seconds: bool,
    show_millis: bool,
}

const CLOCK_DEFAULTS: ModeDefaults = ModeDefaults {
    show_date: true,
    show_seconds: true,
    show_millis: false,
};
const TIMER_DEFAULTS: ModeDefaults = ModeDefaults {
    show_date: false,
    show_seconds: true,
    show_millis: true,
};
const STOPWATCH_DEFAULTS: ModeDefaults = ModeDefaults {
    show_date: false,
    show_seconds: true,
    show_millis: true,
};
const COUNTDOWN_DEFAULTS: ModeDefaults = ModeDefaults {
    show_date: false,
    show_seconds: true,
    show_millis: false,
};

/// Display options after precedence has been applied for one mode.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayOptions {
    titles: Vec<String>,
    show_date: bool,
    show_seconds: bool,
    show_millis: bool,
    start_paused: bool,
}

impl DisplayOptions {
    /// Precedence, highest first: command-line flag, the mode's config
    /// section, `[default]`, then the built-in default for the mode.
    /// `mode_titles` lets the timer pass its per-duration `titles` list.
    fn resolve(
        args: &DisplayArgs,
        mode: Option<&DisplayConfig>,
        default: Option<&DisplayConfig>,
        builtin: ModeDefaults,
        mode_titles: Vec<String>,
    ) -> Self {
        let pick = |cli: Option<bool>, get: fn(&DisplayConfig) -> Option<bool>, fallback: bool| {
            cli.or_else(|| mode.and_then(get))
                .or_else(|| default.and_then(get))
                .unwrap_or(fallback)
        };
        let titles = if !args.titles.is_empty() {
            args.titles.clone()
        } else if !mode_titles.is_empty() {
            mode_titles
        } else {
            mode.and_then(|c| c.title.clone())
                .or_else(|| default.and_then(|c| c.title.clone()))
                .into_iter()
                .collect()
        };

        Self {
            titles,
            show_date: pick(args.show_date(), |c| c.show_date, builtin.show_date),
            show_seconds: pick(
                args.show_seconds(),
                |c| c.show_seconds,
                builtin.show_seconds,
            ),
            show_millis: pick(args.show_millis(), |c| c.show_millis, builtin.show_millis),
            start_paused: pick(args.start_paused(), |c| c.start_paused, false),
        }
    }

    fn title(&self) -> Option<String> {
        self.titles.first().cloned()
    }

    fn duration_format(&self) -> DurationFormat {
        DurationFormat::from_display(self.show_seconds, self.show_millis)
    }
}

use crate::config::{Config, DisplayConfig, TimerConfig};

const DEFAULT_CLOCK_SIZE: u16 = 1;
const DEFAULT_TIMER_WORK_MINUTES: i64 = 25;
const DEFAULT_TIMER_BREAK_MINUTES: i64 = 5;
const WIDGET_THEME_ENV: &str = "TCLOCK_WIDGET_THEME";

#[derive(clap::Parser, Default)]
#[command(name = "tclock", about = "A clock app in terminal", long_about = None)]
pub struct App {
    #[command(subcommand)]
    pub mode: Option<Mode>,
    /// Foreground color of the clock, possible values are:
    ///     a) Any one of: Black, Red, Green, Yellow, Blue, Magenta, Cyan, Gray, DarkGray, LightRed, LightGreen, LightYellow, LightBlue, LightMagenta, LightCyan, White.
    ///     b) Hexadecimal color code: #RRGGBB.
    #[arg(short, long, value_parser = parse_color, global = true)]
    pub color: Option<Color>,
    /// Size of the clock, should be a positive integer (>=1).
    #[arg(short, long, value_parser = parse_size, global = true)]
    pub size: Option<u16>,

    /// Initial clock/widget theme, for example "default", "evangelion", or "nerv". Falls back to TCLOCK_WIDGET_THEME, then config.
    #[arg(long, value_parser = parse_theme_name, global = true)]
    pub theme: Option<String>,

    #[command(flatten)]
    pub display: DisplayArgs,

    #[arg(skip)]
    clock: Option<Clock>,
    #[arg(skip)]
    timer: Option<Timer>,
    #[arg(skip)]
    stopwatch: Option<Stopwatch>,
    #[arg(skip)]
    countdown: Option<Countdown>,
}

impl App {
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = Some(mode);
        self.init_app();
    }

    pub fn init_app(&mut self) {
        // Load config
        let config = Config::load();
        let default_config = config.as_ref().map(|c| &c.default);
        let default_display = default_config.map(|c| &c.display);

        self.clock = None;
        self.timer = None;
        self.stopwatch = None;
        self.countdown = None;

        // default mode
        if self.mode.is_none() {
            self.mode = Some(default_mode(config.as_ref()));
        }

        // set default color and size
        if self.color.is_none() {
            self.color = default_config
                .map(|c| parse_color(&c.color).unwrap_or(Color::Green))
                .or(Some(Color::Green));
        }
        if self.size.is_none() {
            self.size = default_config
                .map(|c| c.size)
                .filter(|size| *size > 0)
                .or(Some(DEFAULT_CLOCK_SIZE));
        }

        let style = Style::default().fg(self.color.unwrap_or(Color::Green));
        let size = self.size.unwrap_or(DEFAULT_CLOCK_SIZE);

        match self.mode.as_ref().expect("mode is set above") {
            Mode::Clock { timezone } => {
                let clock_config = config.as_ref().map(|c| &c.clock);
                let display = DisplayOptions::resolve(
                    &self.display,
                    clock_config.map(|c| &c.display),
                    default_display,
                    CLOCK_DEFAULTS,
                    Vec::new(),
                );
                let widget_themes = resolve_widget_themes(
                    self.theme.as_deref(),
                    std::env::var(WIDGET_THEME_ENV).ok().as_deref(),
                    clock_config
                        .map(|c| c.widget_themes.clone())
                        .unwrap_or_default(),
                );
                self.clock = Some(Clock::new(
                    size,
                    style,
                    display.title(),
                    display.show_date,
                    display.show_millis,
                    display.show_seconds,
                    timezone.or_else(|| clock_config.and_then(|c| c.timezone)),
                    clock_config.map(|c| c.widgets.clone()).unwrap_or_default(),
                    widget_themes,
                ));
            }
            Mode::Timer {
                durations,
                repeat,
                auto_quit,
                execute,
            } => {
                let timer_config = config.as_ref().map(|c| &c.timer);
                let display = DisplayOptions::resolve(
                    &self.display,
                    timer_config.map(|c| &c.display),
                    default_display,
                    TIMER_DEFAULTS,
                    timer_config.map(|c| c.titles.clone()).unwrap_or_default(),
                );
                let durations = if durations.is_empty() {
                    timer_config
                        .map(configured_timer_durations)
                        .unwrap_or_else(default_timer_config_durations)
                } else {
                    durations.clone()
                };
                let execute = if execute.is_empty() {
                    timer_config.map(|c| c.execute.clone()).unwrap_or_default()
                } else {
                    execute.clone()
                };
                self.timer = Some(Timer::new(
                    size,
                    style,
                    durations,
                    display.titles.clone(),
                    *repeat || timer_config.map(|c| c.repeat).unwrap_or(false),
                    display.duration_format(),
                    display.start_paused,
                    *auto_quit || timer_config.map(|c| c.auto_quit).unwrap_or(false),
                    execute,
                ));
            }
            Mode::Stopwatch => {
                let display = DisplayOptions::resolve(
                    &self.display,
                    config.as_ref().map(|c| &c.stopwatch.display),
                    default_display,
                    STOPWATCH_DEFAULTS,
                    Vec::new(),
                );
                self.stopwatch = Some(Stopwatch::new(
                    size,
                    style,
                    display.title(),
                    display.duration_format(),
                    display.start_paused,
                ));
            }
            Mode::Countdown {
                time,
                continue_on_zero,
                reverse,
            } => {
                let countdown_config = config.as_ref().map(|c| &c.countdown);
                let display = DisplayOptions::resolve(
                    &self.display,
                    countdown_config.map(|c| &c.display),
                    default_display,
                    COUNTDOWN_DEFAULTS,
                    Vec::new(),
                );
                self.countdown = Some(Countdown {
                    size,
                    style,
                    time: *time,
                    title: display.title(),
                    continue_on_zero: *continue_on_zero
                        || countdown_config
                            .map(|c| c.continue_on_zero)
                            .unwrap_or(false),
                    reverse: *reverse || countdown_config.map(|c| c.reverse).unwrap_or(false),
                    format: display.duration_format(),
                })
            }
        }
    }

    pub fn ui(&mut self, f: &mut Frame) {
        if let Some(ref mut w) = self.clock {
            w.render(f.area(), f.buffer_mut());
        } else if let Some(ref w) = self.timer {
            f.render_widget(w, f.area());
        } else if let Some(ref w) = self.stopwatch {
            f.render_widget(w, f.area());
        } else if let Some(ref w) = self.countdown {
            f.render_widget(w, f.area());
        }
    }

    pub fn tick(&mut self) {
        if let Some(ref mut w) = self.clock {
            w.tick();
        }
    }

    pub fn on_key(&mut self, key: KeyCode) {
        if let Some(w) = self.clock.as_mut() {
            match key {
                KeyCode::Char('T') => w.cycle_widget_theme(),
                KeyCode::Char('g') => w.cycle_widget_group(),
                KeyCode::Char('z') => w.toggle_widgets(),
                KeyCode::Home => w.scroll_active_widget_to_top(),
                KeyCode::End => w.scroll_active_widget_to_bottom(),
                _ => {}
            }
        } else if let Some(w) = self.timer.as_mut() {
            handle_key(w, key);
        } else if let Some(w) = self.stopwatch.as_mut() {
            handle_key(w, key);
        }
    }

    pub fn open_widget_popup_action(&mut self, key: KeyCode) -> bool {
        let KeyCode::Char(key) = key else {
            return false;
        };
        self.clock
            .as_mut()
            .is_some_and(|clock| clock.open_widget_popup_action(key))
    }

    pub fn on_widget_popup_key(&mut self, key: KeyCode) {
        let Some(clock) = self.clock.as_mut() else {
            return;
        };
        match key {
            KeyCode::Esc => clock.close_widget_popup(),
            KeyCode::Up => clock.scroll_widget_popup(-1),
            KeyCode::Down => clock.scroll_widget_popup(1),
            KeyCode::PageUp => clock.scroll_widget_popup(-10),
            KeyCode::PageDown => clock.scroll_widget_popup(10),
            KeyCode::Home => clock.scroll_active_widget_to_top(),
            KeyCode::End => clock.scroll_active_widget_to_bottom(),
            _ => {}
        }
    }

    pub fn on_mouse_scroll(&mut self, column: u16, row: u16, delta: i16) {
        if let Some(w) = self.clock.as_mut() {
            w.scroll_widget_at(column, row, delta);
        }
    }

    pub fn has_widget_popup_open(&self) -> bool {
        self.clock
            .as_ref()
            .is_some_and(Clock::has_widget_popup_open)
    }

    pub fn is_ended(&self) -> bool {
        if let Some(ref w) = self.timer {
            return w.is_finished();
        }
        false
    }

    pub fn on_exit(&self) {
        if let Some(ref w) = self.stopwatch {
            println!("Stopwatch time: {}", w.get_display_time());
        }
    }
}

fn handle_key<T: Pause>(widget: &mut T, key: KeyCode) {
    if let KeyCode::Char(' ') = key {
        widget.toggle_paused()
    }
}

/// The mode to start in when none was given on the command line: `[default]
/// mode` from the config, falling back to the clock. Mode-specific values
/// (durations, countdown target) come from the config too; display options
/// are resolved later in `init_app`, so they are not repeated here.
fn default_mode(config: Option<&Config>) -> Mode {
    let mode_name = config.map(|c| c.default.mode.as_str()).unwrap_or("clock");
    match mode_name {
        "timer" => Mode::Timer {
            durations: Vec::new(),
            repeat: false,
            auto_quit: false,
            execute: Vec::new(),
        },
        "stopwatch" => Mode::Stopwatch,
        "countdown" => Mode::Countdown {
            time: config
                .and_then(|c| c.countdown.time.as_deref())
                .and_then(|t| parse_datetime(t).ok())
                .unwrap_or_else(Local::now),
            continue_on_zero: false,
            reverse: false,
        },
        _ => Mode::Clock { timezone: None },
    }
}

fn default_timer_config_durations() -> Vec<Duration> {
    vec![
        Duration::minutes(DEFAULT_TIMER_WORK_MINUTES),
        Duration::minutes(DEFAULT_TIMER_BREAK_MINUTES),
    ]
}

fn configured_timer_durations(config: &TimerConfig) -> Vec<Duration> {
    let durations = config
        .durations
        .iter()
        .filter_map(|duration| parse_duration(duration).ok())
        .collect::<Vec<_>>();

    if durations.is_empty() {
        default_timer_config_durations()
    } else {
        durations
    }
}

fn duration_regex() -> &'static Regex {
    static DURATION_REGEX: OnceLock<Regex> = OnceLock::new();
    DURATION_REGEX.get_or_init(|| Regex::new(r"^(\d+)([smhdSMHD])$").expect("valid duration regex"))
}

fn hex_color_regex() -> &'static Regex {
    static HEX_COLOR_REGEX: OnceLock<Regex> = OnceLock::new();
    HEX_COLOR_REGEX.get_or_init(|| Regex::new(r"^#([0-9a-f]{6})$").expect("valid color regex"))
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let cap = duration_regex()
        .captures(s)
        .ok_or_else(|| format!("{} is not a valid duration", s))?;

    let num = cap
        .get(1)
        .expect("duration regex captures number")
        .as_str()
        .parse::<i64>()
        .map_err(|_| format!("Duration is too large: {}", s))?;
    let unit = cap.get(2).unwrap().as_str().to_lowercase();

    let duration = match unit.as_str() {
        "s" => Duration::try_seconds(num),
        "m" => Duration::try_minutes(num),
        "h" => Duration::try_hours(num),
        "d" => Duration::try_days(num),
        _ => return Err(format!("Invalid duration: {}", s)),
    };

    duration.ok_or_else(|| format!("Duration is too large: {}", s))
}

fn parse_size(s: &str) -> Result<u16, String> {
    let size = s
        .parse::<u16>()
        .map_err(|_| format!("Invalid clock size: {}", s))?;

    if size == 0 {
        Err("Clock size must be at least 1".to_string())
    } else {
        Ok(size)
    }
}

fn parse_theme_name(value: &str) -> Result<String, String> {
    let theme = value.trim();
    if theme.is_empty() {
        Err("theme must not be empty".to_string())
    } else {
        Ok(theme.to_string())
    }
}

fn resolve_widget_themes(
    cli_theme: Option<&str>,
    env_theme: Option<&str>,
    configured_themes: Vec<String>,
) -> Vec<String> {
    let requested = cli_theme
        .or(env_theme)
        .map(str::trim)
        .filter(|theme| !theme.is_empty());
    let Some(requested) = requested else {
        return configured_themes;
    };

    let mut themes = Vec::with_capacity(configured_themes.len().max(1));
    themes.push(requested.to_string());
    for theme in configured_themes {
        if !theme.trim().eq_ignore_ascii_case(requested) {
            themes.push(theme);
        }
    }
    themes
}

fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.to_lowercase();
    match s.as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" => Ok(Color::Gray),
        "darkgray" => Ok(Color::DarkGray),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        "white" => Ok(Color::White),
        s => {
            let cap = hex_color_regex()
                .captures(s)
                .ok_or_else(|| format!("Invalid color: {}", s))?;
            let hex = cap.get(1).expect("color regex captures hex value").as_str();
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|error| format!("Invalid red channel in color {}: {}", s, error))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|error| format!("Invalid green channel in color {}: {}", s, error))?;
            let b = u8::from_str_radix(&hex[4..], 16)
                .map_err(|error| format!("Invalid blue channel in color {}: {}", s, error))?;
            Ok(Color::Rgb(r, g, b))
        }
    }
}

fn local_datetime(date_time: NaiveDateTime) -> Result<DateTime<Local>, String> {
    match Local.from_local_datetime(&date_time) {
        LocalResult::Single(date_time) => Ok(date_time),
        LocalResult::Ambiguous(_, _) => Err(format!("Ambiguous local time: {}", date_time)),
        LocalResult::None => Err(format!("Invalid local time: {}", date_time)),
    }
}

fn parse_datetime(s: &str) -> Result<DateTime<Local>, String> {
    let s = s.trim();
    let today = Local::now().date_naive();

    let time = NaiveTime::parse_from_str(s, "%H:%M");
    if let Ok(time) = time {
        let time = NaiveDateTime::new(today, time);
        return local_datetime(time);
    }

    let time = NaiveTime::parse_from_str(s, "%H:%M:%S");
    if let Ok(time) = time {
        let time = NaiveDateTime::new(today, time);
        return local_datetime(time);
    }

    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d");
    if let Ok(date) = date {
        let time = NaiveDateTime::new(date, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        return local_datetime(time);
    }

    let date_time = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S");
    if let Ok(date_time) = date_time {
        return local_datetime(date_time);
    }

    let rfc_time = DateTime::parse_from_rfc3339(s);
    if let Ok(rfc_time) = rfc_time {
        return Ok(rfc_time.with_timezone(&Local));
    }

    Err("Invalid time format".to_string())
}

fn parse_timezone(s: &str) -> Result<Tz, String> {
    s.parse::<Tz>().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ClockWidgetConfig, ClockWidgetPopupActionConfig, WidgetPosition};
    use clap::Parser;
    use ratatui::{buffer::Buffer, layout::Rect};

    #[test]
    fn parse_duration_accepts_supported_units() {
        assert_eq!(parse_duration("10s").unwrap(), Duration::seconds(10));
        assert_eq!(parse_duration("5M").unwrap(), Duration::minutes(5));
        assert_eq!(parse_duration("2h").unwrap(), Duration::hours(2));
        assert_eq!(parse_duration("1D").unwrap(), Duration::days(1));
    }

    #[test]
    fn parse_duration_rejects_invalid_or_overflowing_values() {
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("999999999999999999999999999999999999999d").is_err());
    }

    #[test]
    fn configured_timer_durations_falls_back_when_all_values_are_invalid() {
        let config = TimerConfig {
            durations: vec!["bad".to_string()],
            ..TimerConfig::default()
        };

        assert_eq!(
            configured_timer_durations(&config),
            default_timer_config_durations()
        );
    }

    #[test]
    fn parse_size_rejects_zero() {
        assert_eq!(parse_size("1"), Ok(1));
        assert!(parse_size("0").is_err());
    }

    #[test]
    fn parse_color_accepts_names_and_hex_values() {
        assert_eq!(parse_color("LightCyan"), Ok(Color::LightCyan));
        assert_eq!(parse_color("#e63946"), Ok(Color::Rgb(230, 57, 70)));
        assert!(parse_color("#xyzxyz").is_err());
    }

    #[test]
    fn command_line_definition_passes_clap_self_checks() {
        // Catches duplicate short/long flags across the global display options
        // and every subcommand. clap only runs these asserts in debug builds,
        // so a collision would otherwise ship silently in release binaries.
        use clap::CommandFactory;
        App::command().debug_assert();
    }

    #[test]
    fn app_accepts_theme_option() {
        let app = App::try_parse_from(["tclock", "--theme", "nerv"]).unwrap();

        assert_eq!(app.theme.as_deref(), Some("nerv"));
        assert!(App::try_parse_from(["tclock", "--theme", ""]).is_err());
    }

    fn display_config(
        title: Option<&str>,
        show_seconds: Option<bool>,
        show_millis: Option<bool>,
    ) -> DisplayConfig {
        DisplayConfig {
            title: title.map(str::to_string),
            show_date: None,
            show_seconds,
            show_millis,
            start_paused: None,
        }
    }

    #[test]
    fn display_flags_are_accepted_by_every_mode_before_or_after_the_mode() {
        let app = App::try_parse_from(["tclock", "stopwatch", "--no-seconds", "--title", "Focus"])
            .unwrap();
        assert!(matches!(app.mode, Some(Mode::Stopwatch)));
        assert_eq!(app.display.show_seconds(), Some(false));
        assert_eq!(app.display.titles, vec!["Focus"]);

        let app =
            App::try_parse_from(["tclock", "--no-millis", "-P", "timer", "-d", "10m"]).unwrap();
        assert!(matches!(app.mode, Some(Mode::Timer { .. })));
        assert_eq!(app.display.show_millis(), Some(false));
        assert_eq!(app.display.start_paused(), Some(true));

        let app = App::try_parse_from(["tclock", "clock", "-D", "-S", "--color", "red"]).unwrap();
        assert_eq!(app.display.show_date(), Some(false));
        assert_eq!(app.display.show_seconds(), Some(false));
        assert_eq!(app.color, Some(Color::Red));
    }

    #[test]
    fn later_display_flag_wins_over_its_opposite() {
        let app = App::try_parse_from(["tclock", "--millis", "--no-millis"]).unwrap();
        assert_eq!(app.display.show_millis(), Some(false));

        let app = App::try_parse_from(["tclock", "--no-seconds", "--seconds"]).unwrap();
        assert_eq!(app.display.show_seconds(), Some(true));

        let app = App::try_parse_from(["tclock"]).unwrap();
        assert_eq!(app.display.show_seconds(), None);
        assert_eq!(app.display.show_millis(), None);
        assert_eq!(app.display.show_date(), None);
        assert_eq!(app.display.start_paused(), None);
    }

    #[test]
    fn timer_titles_accept_repeated_and_multi_value_forms() {
        let app = App::try_parse_from([
            "tclock", "timer", "-d", "25m", "5m", "--title", "Focus", "Break",
        ])
        .unwrap();
        assert_eq!(app.display.titles, vec!["Focus", "Break"]);

        let app = App::try_parse_from(["tclock", "timer", "--title", "Focus", "--title", "Break"])
            .unwrap();
        assert_eq!(app.display.titles, vec!["Focus", "Break"]);
    }

    #[test]
    fn display_precedence_is_cli_then_mode_then_default_then_builtin() {
        let mode = display_config(Some("Mode title"), Some(false), None);
        let default = display_config(Some("Default title"), Some(true), Some(false));

        // Built-in defaults only.
        let display = DisplayOptions::resolve(
            &DisplayArgs::default(),
            None,
            None,
            STOPWATCH_DEFAULTS,
            Vec::new(),
        );
        assert_eq!(display.titles, Vec::<String>::new());
        assert!(display.show_seconds);
        assert!(display.show_millis);
        assert!(!display.start_paused);

        // `[default]` fills what the mode section leaves unset.
        let display = DisplayOptions::resolve(
            &DisplayArgs::default(),
            Some(&mode),
            Some(&default),
            STOPWATCH_DEFAULTS,
            Vec::new(),
        );
        assert_eq!(display.titles, vec!["Mode title"]);
        assert!(!display.show_seconds);
        assert!(!display.show_millis);

        // A command-line flag beats both config layers.
        let args = DisplayArgs {
            seconds: true,
            titles: vec!["CLI".to_string()],
            ..DisplayArgs::default()
        };
        let display = DisplayOptions::resolve(
            &args,
            Some(&mode),
            Some(&default),
            STOPWATCH_DEFAULTS,
            Vec::new(),
        );
        assert_eq!(display.titles, vec!["CLI"]);
        assert!(display.show_seconds);
        assert_eq!(display.duration_format(), DurationFormat::HourMinSec);

        // Timer `titles` beat a single `title`, and the first title is the header.
        let display = DisplayOptions::resolve(
            &DisplayArgs::default(),
            Some(&mode),
            Some(&default),
            TIMER_DEFAULTS,
            vec!["Work".to_string(), "Rest".to_string()],
        );
        assert_eq!(display.titles, vec!["Work", "Rest"]);
        assert_eq!(display.title().as_deref(), Some("Work"));
    }

    #[test]
    fn default_mode_reads_the_config_and_falls_back_to_clock() {
        let config: Config = toml::from_str("[default]\nmode = \"stopwatch\"").unwrap();
        assert!(matches!(default_mode(Some(&config)), Mode::Stopwatch));

        let config: Config = toml::from_str("[default]\nmode = \"timer\"").unwrap();
        assert!(matches!(
            default_mode(Some(&config)),
            Mode::Timer { durations, .. } if durations.is_empty()
        ));

        assert!(matches!(default_mode(None), Mode::Clock { timezone: None }));
    }

    #[test]
    fn parse_datetime_accepts_dates_and_rejects_invalid_values() {
        assert!(parse_datetime("2026-01-01").is_ok());
        assert!(parse_datetime("not a date").is_err());
    }

    #[test]
    fn parse_timezone_reports_invalid_names() {
        assert!(parse_timezone("America/New_York").is_ok());
        assert!(parse_timezone("Nowhere/Missing").is_err());
    }

    #[test]
    fn uppercase_t_cycles_clock_theme_and_lowercase_t_does_not() {
        let clock = Clock::new(
            DEFAULT_CLOCK_SIZE,
            Style::default(),
            None,
            true,
            false,
            true,
            None,
            Vec::new(),
            vec!["default".to_string(), "nerv".to_string()],
        );
        let mut app = App {
            mode: Some(Mode::Clock { timezone: None }),
            clock: Some(clock),
            ..App::default()
        };

        app.on_key(KeyCode::Char('T'));
        let clock = app.clock.as_ref().expect("clock mode remains active");
        assert_eq!(clock.current_widget_theme_for_test(), "nerv");
        assert_eq!(
            clock.current_theme_for_test().clock_style.fg,
            Some(Color::Indexed(196))
        );

        app.on_key(KeyCode::Char('t'));
        assert_eq!(
            app.clock
                .as_ref()
                .expect("clock mode remains active")
                .current_widget_theme_for_test(),
            "nerv"
        );
    }

    #[test]
    fn z_toggles_the_clock_only_layout() {
        let clock = Clock::new(
            DEFAULT_CLOCK_SIZE,
            Style::default(),
            None,
            true,
            false,
            true,
            None,
            Vec::new(),
            vec!["default".to_string()],
        );
        let mut app = App {
            mode: Some(Mode::Clock { timezone: None }),
            clock: Some(clock),
            ..App::default()
        };

        assert!(app.clock.as_ref().unwrap().widgets_visible_for_test());
        app.on_key(KeyCode::Char('z'));
        assert!(!app.clock.as_ref().unwrap().widgets_visible_for_test());
        app.on_key(KeyCode::Char('z'));
        assert!(app.clock.as_ref().unwrap().widgets_visible_for_test());
    }

    #[test]
    fn clock_routes_arbitrary_widget_popup_keys_and_escape_closes() {
        let widget = ClockWidgetConfig {
            title: Some("Diagnostics".to_string()),
            command: vec!["printf".to_string()],
            popup_actions: vec![ClockWidgetPopupActionConfig {
                key: 'x',
                label: Some("inspect".to_string()),
                title: None,
                command: Vec::new(),
                args: vec!["details".to_string()],
                timeout_secs: None,
            }],
            refresh_secs: 900,
            timeout_secs: 30,
            position: WidgetPosition::Auto,
            group: None,
        };
        let mut clock = Clock::new(
            DEFAULT_CLOCK_SIZE,
            Style::default(),
            None,
            true,
            false,
            true,
            None,
            vec![widget],
            vec!["default".to_string()],
        );
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        clock.render(area, &mut buffer);
        let mut app = App {
            mode: Some(Mode::Clock { timezone: None }),
            clock: Some(clock),
            ..App::default()
        };

        assert!(app.open_widget_popup_action(KeyCode::Char('x')));
        assert!(app.has_widget_popup_open());
        app.on_widget_popup_key(KeyCode::Esc);
        assert!(!app.has_widget_popup_open());
        assert!(!app.open_widget_popup_action(KeyCode::F(1)));
    }

    #[test]
    fn theme_precedence_reorders_widget_themes() {
        assert_eq!(
            resolve_widget_themes(
                None,
                None,
                vec![
                    "default".to_string(),
                    "evangelion".to_string(),
                    "nerv".to_string()
                ]
            ),
            vec!["default", "evangelion", "nerv"]
        );
        assert_eq!(
            resolve_widget_themes(
                None,
                Some("nerv"),
                vec![
                    "default".to_string(),
                    "evangelion".to_string(),
                    "nerv".to_string()
                ]
            ),
            vec!["nerv", "default", "evangelion"]
        );
        assert_eq!(
            resolve_widget_themes(
                Some("default"),
                Some("nerv"),
                vec![
                    "default".to_string(),
                    "evangelion".to_string(),
                    "nerv".to_string()
                ]
            ),
            vec!["default", "evangelion", "nerv"]
        );
        assert_eq!(
            resolve_widget_themes(Some("eva"), None, vec!["default".to_string()]),
            vec!["eva", "default"]
        );
    }
}
