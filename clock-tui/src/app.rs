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
        /// Do not show date
        #[arg(short = 'D', long, action)]
        no_date: bool,
        /// Do not show seconds
        #[arg(short = 'S', long, action)]
        no_seconds: bool,
        /// Show fractional seconds
        #[arg(short, long, action)]
        millis: bool,
    },
    /// The timer mode displays the remaining time until the timer is finished.
    Timer {
        /// Initial duration for timer, value can be 10s for 10 seconds, 1m for 1 minute, etc.
        /// Also accepts multiple duration values and runs the timers sequentially, eg. 25m 5m
        #[arg(short, long = "duration", value_parser = parse_duration, num_args = 1.., default_value = "5m")]
        durations: Vec<Duration>,

        /// Set the title for the timer; accepts multiple titles corresponding to each duration
        #[arg(short, long = "title", num_args = 0..)]
        titles: Vec<String>,

        /// Restart the timer when timer is over
        #[arg(long, short, action)]
        repeat: bool,

        /// Hide fractional seconds
        #[arg(long = "no-millis", short = 'M', action)]
        no_millis: bool,

        /// Start the timer paused
        #[arg(long = "paused", short = 'P', action)]
        paused: bool,

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

        /// Title or description for countdown show in header
        #[arg(long, short = 'T')]
        title: Option<String>,

        /// Continue counting down after passing the target time
        #[arg(long = "continue", short = 'c', action)]
        continue_on_zero: bool,

        /// Reverse the countdown, a.k.a. countup
        #[arg(long, short, action)]
        reverse: bool,

        /// Show fractional seconds
        #[arg(short, long, action)]
        millis: bool,
    },
}

use crate::config::{Config, TimerConfig};

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
    #[arg(short, long, value_parser = parse_color)]
    pub color: Option<Color>,
    /// Size of the clock, should be a positive integer (>=1).
    #[arg(short, long, value_parser = parse_size)]
    pub size: Option<u16>,

    /// Initial clock/widget theme, for example "default", "evangelion", or "nerv". Falls back to TCLOCK_WIDGET_THEME, then config.
    #[arg(long, value_parser = parse_theme_name)]
    pub theme: Option<String>,

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

        self.clock = None;
        self.timer = None;
        self.stopwatch = None;
        self.countdown = None;

        // default mode
        if self.mode.is_none() {
            self.mode = default_config.map(|c| match c.mode.as_str() {
                "timer" => {
                    let timer_config = config.as_ref().map(|c| &c.timer);
                    Mode::Timer {
                        durations: timer_config
                            .map(configured_timer_durations)
                            .unwrap_or_else(default_timer_config_durations),
                        titles: timer_config.map(|c| c.titles.clone()).unwrap_or_default(),
                        repeat: timer_config.map(|c| c.repeat).unwrap_or(false),
                        no_millis: !timer_config.map(|c| c.show_millis).unwrap_or(true),
                        paused: timer_config.map(|c| c.start_paused).unwrap_or(false),
                        auto_quit: timer_config.map(|c| c.auto_quit).unwrap_or(false),
                        execute: timer_config.map(|c| c.execute.clone()).unwrap_or_default(),
                    }
                }
                "stopwatch" => Mode::Stopwatch,
                "countdown" => {
                    let countdown_config = config.as_ref().map(|c| &c.countdown);
                    Mode::Countdown {
                        time: countdown_config
                            .and_then(|c| c.time.as_ref())
                            .and_then(|t| parse_datetime(t).ok())
                            .unwrap_or_else(Local::now),
                        title: countdown_config.map(|c| c.title.clone()).unwrap_or(None),
                        continue_on_zero: countdown_config
                            .map(|c| c.continue_on_zero)
                            .unwrap_or(false),
                        reverse: countdown_config.map(|c| c.reverse).unwrap_or(false),
                        millis: countdown_config.map(|c| c.show_millis).unwrap_or(false),
                    }
                }
                _ => {
                    let clock_config = config.as_ref().map(|c| &c.clock);
                    Mode::Clock {
                        no_date: !clock_config.map(|c| c.show_date).unwrap_or(true),
                        millis: clock_config.map(|c| c.show_millis).unwrap_or(false),
                        no_seconds: !clock_config.map(|c| c.show_seconds).unwrap_or(true),
                        timezone: clock_config.and_then(|c| c.timezone),
                    }
                }
            });
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

        // initialize the clock mode
        match self.mode.as_ref().unwrap_or(&Mode::Clock {
            no_date: false,
            millis: false,
            no_seconds: false,
            timezone: None,
        }) {
            Mode::Clock {
                no_date,
                no_seconds,
                millis,
                timezone,
            } => {
                let clock_config = config.as_ref().map(|c| &c.clock);
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
                    !no_date && clock_config.map(|c| c.show_date).unwrap_or(true),
                    *millis || clock_config.map(|c| c.show_millis).unwrap_or(false),
                    !no_seconds && clock_config.map(|c| c.show_seconds).unwrap_or(true),
                    timezone.or_else(|| clock_config.and_then(|c| c.timezone)),
                    clock_config.map(|c| c.widgets.clone()).unwrap_or_default(),
                    widget_themes,
                ));
            }
            Mode::Timer {
                durations,
                titles,
                repeat,
                no_millis,
                paused,
                auto_quit,
                execute,
            } => {
                let timer_config = config.as_ref().map(|c| &c.timer);
                let format = if *no_millis {
                    DurationFormat::HourMinSec
                } else {
                    DurationFormat::HourMinSecDeci
                };
                self.timer = Some(Timer::new(
                    size,
                    style,
                    durations.to_owned(),
                    titles.to_owned(),
                    *repeat || timer_config.map(|c| c.repeat).unwrap_or(false),
                    format,
                    *paused || timer_config.map(|c| c.start_paused).unwrap_or(false),
                    *auto_quit || timer_config.map(|c| c.auto_quit).unwrap_or(false),
                    execute.to_owned(),
                ));
            }
            Mode::Stopwatch => {
                self.stopwatch = Some(Stopwatch::new(size, style));
            }
            Mode::Countdown {
                time,
                title,
                continue_on_zero,
                reverse,
                millis,
            } => {
                let countdown_config = config.as_ref().map(|c| &c.countdown);
                self.countdown = Some(Countdown {
                    size,
                    style,
                    time: *time,
                    title: title.to_owned(),
                    continue_on_zero: *continue_on_zero
                        || countdown_config
                            .map(|c| c.continue_on_zero)
                            .unwrap_or(false),
                    reverse: *reverse || countdown_config.map(|c| c.reverse).unwrap_or(false),
                    format: if *millis || countdown_config.map(|c| c.show_millis).unwrap_or(false) {
                        DurationFormat::HourMinSecDeci
                    } else {
                        DurationFormat::HourMinSec
                    },
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

    pub fn on_mouse_scroll(&mut self, column: u16, row: u16, delta: i16) {
        if let Some(w) = self.clock.as_mut() {
            w.scroll_widget_at(column, row, delta);
        }
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
    use clap::Parser;

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
    fn app_accepts_theme_option() {
        let app = App::try_parse_from(["tclock", "--theme", "nerv"]).unwrap();

        assert_eq!(app.theme.as_deref(), Some("nerv"));
        assert!(App::try_parse_from(["tclock", "--theme", ""]).is_err());
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
            true,
            false,
            true,
            None,
            Vec::new(),
            vec!["default".to_string(), "nerv".to_string()],
        );
        let mut app = App {
            mode: Some(Mode::Clock {
                timezone: None,
                no_date: false,
                no_seconds: false,
                millis: false,
            }),
            color: None,
            size: None,
            theme: None,
            clock: Some(clock),
            timer: None,
            stopwatch: None,
            countdown: None,
        };

        app.on_key(KeyCode::Char('T'));
        let clock = app.clock.as_ref().expect("clock mode remains active");
        assert_eq!(clock.current_widget_theme_for_test(), "nerv");
        assert_eq!(
            clock.current_theme_for_test().clock_style.fg,
            Some(Color::Indexed(208))
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
    fn theme_precedence_reorders_widget_themes() {
        assert_eq!(
            resolve_widget_themes(None, None, vec!["default".to_string(), "nerv".to_string()]),
            vec!["default", "nerv"]
        );
        assert_eq!(
            resolve_widget_themes(
                None,
                Some("nerv"),
                vec!["default".to_string(), "nerv".to_string()]
            ),
            vec!["nerv", "default"]
        );
        assert_eq!(
            resolve_widget_themes(
                Some("default"),
                Some("nerv"),
                vec!["default".to_string(), "nerv".to_string()]
            ),
            vec!["default", "nerv"]
        );
        assert_eq!(
            resolve_widget_themes(Some("eva"), None, vec!["default".to_string()]),
            vec!["eva", "default"]
        );
    }
}
