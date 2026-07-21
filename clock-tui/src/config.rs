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
}

#[derive(Debug, Deserialize)]
pub struct ClockConfig {
    #[serde(default = "default_true")]
    pub show_date: bool,
    #[serde(default = "default_true")]
    pub show_seconds: bool,
    #[serde(default = "default_false")]
    pub show_millis: bool,
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

#[derive(Debug, Deserialize)]
pub struct TimerConfig {
    #[serde(default = "default_timer_durations")]
    pub durations: Vec<String>,
    #[serde(default)]
    pub titles: Vec<String>,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default = "default_true")]
    pub show_millis: bool,
    #[serde(default)]
    pub start_paused: bool,
    #[serde(default)]
    pub auto_quit: bool,
    #[serde(default)]
    pub execute: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StopwatchConfig {}

#[derive(Debug, Default, Deserialize)]
pub struct CountdownConfig {
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub show_millis: bool,
    #[serde(default)]
    pub continue_on_zero: bool,
    #[serde(default)]
    pub reverse: bool,
}

impl Default for DefaultConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            color: default_color(),
            size: default_size(),
        }
    }
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            show_date: default_true(),
            show_seconds: default_true(),
            show_millis: default_false(),
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
            show_millis: default_true(),
            start_paused: false,
            auto_quit: false,
            execute: Vec::new(),
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

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
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
                eprintln!("解析配置文件失败: {}", e);
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
            "#,
        )
        .unwrap();

        let widget = &config.clock.widgets[0];
        assert_eq!(widget.command, vec!["sh", "-c", "printf ok"]);
        assert_eq!(widget.refresh_secs, 5);
        assert_eq!(widget.timeout_secs, 2);
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
