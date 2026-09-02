use std::path::{Path, PathBuf};

use chrono_tz::Tz;
use serde::{Deserialize, Deserializer};

pub(crate) const DEFAULT_WIDGET_REFRESH_SECS: u64 = 15 * 60;
pub(crate) const DEFAULT_WIDGET_TIMEOUT_SECS: u64 = 30;
pub(crate) const DEFAULT_WIDGET_THEMES: [&str; 3] = ["default", "evangelion", "nerv"];

fn deserialize_timezone<'de, D>(deserializer: D) -> Result<Option<Tz>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

fn deserialize_widget_command<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CommandValue {
        Program(String),
        Args(Vec<String>),
    }

    match CommandValue::deserialize(deserializer)? {
        CommandValue::Program(program) => Ok(vec![program]),
        CommandValue::Args(args) => Ok(args),
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default: DefaultConfig,
    #[serde(default)]
    pub clock: ClockConfig,
    #[serde(default)]
    pub timer: TimerConfig,
    #[serde(default)]
    pub stopwatch: StopwatchConfig,
    #[serde(default)]
    pub countdown: CountdownConfig,
}

#[derive(Debug, Deserialize)]
pub struct DefaultConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_size")]
    pub size: u16,
    /// Display defaults shared by every mode. A mode section can override any key.
    #[serde(flatten)]
    pub display: DisplayConfig,
}

/// Display options every mode understands. They can appear under `[default]`
/// and under any mode section; the mode section wins, then `[default]`, then
/// the built-in default for that mode. Command-line flags override both.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct DisplayConfig {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub show_date: Option<bool>,
    #[serde(default)]
    pub show_seconds: Option<bool>,
    #[serde(default)]
    pub show_millis: Option<bool>,
    #[serde(default)]
    pub start_paused: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ClockConfig {
    #[serde(flatten)]
    pub display: DisplayConfig,
    #[serde(default, deserialize_with = "deserialize_timezone")]
    pub timezone: Option<Tz>,
    #[serde(default)]
    pub widgets: Vec<ClockWidgetConfig>,
    #[serde(default = "default_widget_themes")]
    pub widget_themes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetPosition {
    /// Placed in the horizontal widget row below the clock (the default).
    #[default]
    Auto,
    /// Placed in a full-width band at the bottom, beneath the widget row,
    /// sized to fit the widget's output.
    Bottom,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClockWidgetConfig {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_widget_command")]
    pub command: Vec<String>,
    #[serde(default)]
    pub popup_actions: Vec<ClockWidgetPopupActionConfig>,
    #[serde(default = "default_widget_refresh_secs")]
    pub refresh_secs: u64,
    #[serde(default = "default_widget_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub position: WidgetPosition,
    /// Optional group name. Widgets sharing a group are shown together, and
    /// only one group is on screen at a time (cycled with `g`). Widgets with no
    /// group are always shown. Group order follows first appearance in config,
    /// so the first grouped widget's group is the one shown at startup.
    #[serde(default)]
    pub group: Option<String>,
}

/// A global key binding contributed by a clock widget.
///
/// Triggering the key while the widget is visible runs `command`, or reruns
/// the widget's command when omitted, appends `args`, and shows the result in a
/// modal popup. This keeps popup behavior generic: widget commands decide what
/// their action means, while the clock framework only handles execution and UI.
#[derive(Debug, Clone, Deserialize)]
pub struct ClockWidgetPopupActionConfig {
    pub key: char,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_widget_command")]
    pub command: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional action-specific timeout. Omit it to inherit the widget timeout.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct TimerConfig {
    #[serde(default = "default_timer_durations")]
    pub durations: Vec<String>,
    /// One title per duration. Takes precedence over `title`.
    #[serde(default)]
    pub titles: Vec<String>,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default)]
    pub auto_quit: bool,
    #[serde(default)]
    pub execute: Vec<String>,
    #[serde(flatten)]
    pub display: DisplayConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct StopwatchConfig {
    #[serde(flatten)]
    pub display: DisplayConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct CountdownConfig {
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub continue_on_zero: bool,
    #[serde(default)]
    pub reverse: bool,
    #[serde(flatten)]
    pub display: DisplayConfig,
}

impl Default for DefaultConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            color: default_color(),
            size: default_size(),
            display: DisplayConfig::default(),
        }
    }
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            display: DisplayConfig::default(),
            timezone: None,
            widgets: Vec::new(),
            widget_themes: default_widget_themes(),
        }
    }
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            durations: default_timer_durations(),
            titles: Vec::new(),
            repeat: false,
            auto_quit: false,
            execute: Vec::new(),
            display: DisplayConfig::default(),
        }
    }
}

fn default_mode() -> String {
    "clock".to_string()
}

fn default_color() -> String {
    "green".to_string()
}

fn default_size() -> u16 {
    1
}

fn default_timer_durations() -> Vec<String> {
    vec!["25m".to_string(), "5m".to_string()]
}

fn default_widget_refresh_secs() -> u64 {
    DEFAULT_WIDGET_REFRESH_SECS
}

fn default_widget_timeout_secs() -> u64 {
    DEFAULT_WIDGET_TIMEOUT_SECS
}

fn default_widget_themes() -> Vec<String> {
    DEFAULT_WIDGET_THEMES
        .iter()
        .map(|theme| (*theme).to_string())
        .collect()
}

impl Config {
    fn config_paths_from_dirs(
        xdg_config_home: Option<PathBuf>,
        home_dir: Option<PathBuf>,
        native_config_dir: Option<PathBuf>,
    ) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(xdg) = xdg_config_home.filter(|path| path.is_absolute()) {
            dirs.push(xdg);
        }
        if let Some(home) = home_dir {
            dirs.push(home.join(".config"));
        }
        if let Some(native) = native_config_dir {
            dirs.push(native);
        }

        let mut paths = Vec::new();
        for dir in dirs {
            let path = dir.join("tclock").join("config.toml");
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
    }

    /// Ordered list of locations to look for the config file, highest priority
    /// first. An absolute `$XDG_CONFIG_HOME` and `~/.config` are honored on
    /// every platform (so the same `~/.config/tclock/config.toml` works on
    /// macOS and Linux), with the OS-native directory
    /// (`~/Library/Application Support` on macOS) kept as a fallback for
    /// existing setups. Duplicates are removed so the same file is never read
    /// twice.
    pub fn config_paths() -> Vec<PathBuf> {
        Self::config_paths_from_dirs(
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            dirs::home_dir(),
            dirs::config_dir(),
        )
    }

    /// The config path tclock reads: the first candidate that exists, or the
    /// highest-priority candidate as a default when none exist yet.
    pub fn config_path() -> Option<PathBuf> {
        let paths = Self::config_paths();
        paths
            .iter()
            .find(|path| path.exists())
            .cloned()
            .or_else(|| paths.into_iter().next())
    }

    pub fn load() -> Option<Self> {
        Self::config_paths()
            .into_iter()
            .find(|path| path.exists())
            .and_then(Self::load_from_path)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return None;
        };

        let content = std::fs::read_to_string(path).ok()?;
        match toml::from_str(&content) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("failed to parse config file {}: {}", path.display(), e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_widget_defaults_and_string_command_parse() {
        let config: Config = toml::from_str(
            r#"
            [clock]
            [[clock.widgets]]
            title = "Pending"
            command = "ghpending"
            "#,
        )
        .unwrap();

        let widget = &config.clock.widgets[0];
        assert_eq!(widget.title.as_deref(), Some("Pending"));
        assert_eq!(widget.command, vec!["ghpending"]);
        assert!(widget.popup_actions.is_empty());
        assert_eq!(widget.refresh_secs, 15 * 60);
        assert_eq!(widget.timeout_secs, 30);
        assert_eq!(widget.position, WidgetPosition::Auto);
    }

    #[test]
    fn clock_widget_bottom_position_parse() {
        let config: Config = toml::from_str(
            r#"
            [clock]
            [[clock.widgets]]
            command = "system-health"
            position = "bottom"
            "#,
        )
        .unwrap();

        assert_eq!(config.clock.widgets[0].position, WidgetPosition::Bottom);
    }

    #[test]
    fn clock_widget_themes_default_and_parse() {
        let default_config: Config = toml::from_str("[clock]").unwrap();
        assert_eq!(
            default_config.clock.widget_themes,
            vec!["default", "evangelion", "nerv"]
        );

        let custom_config: Config = toml::from_str(
            r#"
            [clock]
            widget_themes = ["light", "dark"]
            "#,
        )
        .unwrap();
        assert_eq!(custom_config.clock.widget_themes, vec!["light", "dark"]);
    }

    #[test]
    fn clock_widget_arg_command_parse() {
        let config: Config = toml::from_str(
            r#"
            [clock]
            [[clock.widgets]]
            command = ["sh", "-c", "printf ok"]
            refresh_secs = 5
            timeout_secs = 2

            [[clock.widgets.popup_actions]]
            key = "d"
            label = "details"
            args = ["--details"]
            timeout_secs = 4
            "#,
        )
        .unwrap();

        let widget = &config.clock.widgets[0];
        assert_eq!(widget.command, vec!["sh", "-c", "printf ok"]);
        assert_eq!(widget.refresh_secs, 5);
        assert_eq!(widget.timeout_secs, 2);
        assert_eq!(widget.popup_actions.len(), 1);
        let action = &widget.popup_actions[0];
        assert_eq!(action.key, 'd');
        assert_eq!(action.label.as_deref(), Some("details"));
        assert_eq!(action.args, vec!["--details"]);
        assert_eq!(action.timeout_secs, Some(4));
    }

    #[test]
    fn display_keys_parse_under_default_and_every_mode_section() {
        let config: Config = toml::from_str(
            r#"
            [default]
            title = "Shared"
            show_seconds = false

            [clock]
            show_date = false
            show_millis = true

            [timer]
            durations = ["10m"]
            titles = ["Work"]
            start_paused = true

            [stopwatch]
            title = "Lap"
            show_millis = false

            [countdown]
            time = "20:00"
            title = "Dinner"
            "#,
        )
        .unwrap();

        assert_eq!(config.default.display.title.as_deref(), Some("Shared"));
        assert_eq!(config.default.display.show_seconds, Some(false));
        assert_eq!(config.default.display.show_millis, None);

        assert_eq!(config.clock.display.show_date, Some(false));
        assert_eq!(config.clock.display.show_millis, Some(true));
        assert_eq!(config.clock.display.show_seconds, None);
        assert_eq!(config.clock.widget_themes, default_widget_themes());

        assert_eq!(config.timer.durations, vec!["10m"]);
        assert_eq!(config.timer.titles, vec!["Work"]);
        assert_eq!(config.timer.display.start_paused, Some(true));

        assert_eq!(config.stopwatch.display.title.as_deref(), Some("Lap"));
        assert_eq!(config.stopwatch.display.show_millis, Some(false));

        assert_eq!(config.countdown.time.as_deref(), Some("20:00"));
        assert_eq!(config.countdown.display.title.as_deref(), Some("Dinner"));
    }

    #[test]
    fn missing_sections_leave_display_options_unset() {
        let config: Config = toml::from_str("").unwrap();

        assert_eq!(config.default.display, DisplayConfig::default());
        assert_eq!(config.stopwatch.display, DisplayConfig::default());
        assert_eq!(config.timer.display, DisplayConfig::default());
        assert!(!config.timer.repeat);
        assert_eq!(config.timer.durations, default_timer_durations());
    }

    #[test]
    fn config_paths_prefer_absolute_xdg_and_remove_duplicates() {
        let root = std::env::current_dir().unwrap();
        let xdg = root.join("xdg");
        let home = root.join("home");
        let paths = Config::config_paths_from_dirs(
            Some(xdg.clone()),
            Some(home.clone()),
            Some(xdg.clone()),
        );

        assert_eq!(
            paths,
            vec![
                xdg.join("tclock").join("config.toml"),
                home.join(".config").join("tclock").join("config.toml"),
            ]
        );
    }

    #[test]
    fn config_paths_ignore_relative_xdg_config_home() {
        let root = std::env::current_dir().unwrap();
        let home = root.join("home");
        let native = root.join("native");
        let paths = Config::config_paths_from_dirs(
            Some(PathBuf::from("relative")),
            Some(home.clone()),
            Some(native.clone()),
        );

        assert_eq!(
            paths,
            vec![
                home.join(".config").join("tclock").join("config.toml"),
                native.join("tclock").join("config.toml"),
            ]
        );
    }
}
