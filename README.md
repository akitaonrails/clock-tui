# clock-tui (`tclock`)

![Clock mode with NERV theme and command widgets](./assets/screenshot-clock-widgets.png)

Clock mode supports runtime themes; this screenshot shows the built-in `nerv` theme with command widgets. Press `Shift+T` to cycle `default` → `evangelion` → `nerv`.

`tclock` is a terminal clock app with clock, timer, stopwatch, and countdown modes.

This repository is a maintained fork of the original [`race604/clock-tui`](https://github.com/race604/clock-tui). The original project appears to have been paused for a long time, so this fork keeps the core idea and modernizes it with updated Rust dependencies, GitHub release binaries, AUR packaging, and clock-mode command widgets.

## Install

### Arch Linux / AUR

The recommended install method on Arch Linux is the prebuilt AUR package from this fork:

```shell
yay -S clock-tui-bin
```

### GitHub Releases

Prebuilt Linux binaries are published for `x86_64` and `aarch64`:

<https://github.com/akitaonrails/clock-tui/releases>

Download the tarball for your architecture and put `tclock` somewhere in your `PATH`.

### Build from source

To install this fork directly from GitHub:

```shell
cargo install --git https://github.com/akitaonrails/clock-tui --package clock-tui
```

To build a local checkout:

```shell
cargo build --release
```

The binary will be at `target/release/tclock`.

## Basic usage

```shell
tclock
```

Press `q` to exit. In the main loop, press `c`, `w`, or `t` to switch to clock, stopwatch, or timer mode. Press `Space` to pause/resume modes that support pausing.

Use `--help` for all options:

```shell
tclock --help
tclock clock --help
tclock timer --help
```

## Modes

### Clock

```shell
tclock clock

# The clock is also the default mode:
tclock
```

![clock](./assets/demo-clock-mode.gif)

Clock options include timezone, seconds, milliseconds, date visibility, color, and size:

```shell
tclock clock --timezone America/New_York
tclock clock --no-seconds
tclock --color '#e63946'
tclock --size 2
tclock --theme nerv
```

### Timer

```shell
# Start a 5-minute timer
tclock timer --duration 5m
```

Durations can use suffixes such as `s`, `m`, `h`, and `d`. Timer mode can run several durations sequentially and can execute a command when time is up:

```shell
tclock timer --duration 25m 5m --title Focus Break
tclock timer --duration 25m --execute terminal-notifier -title tclock -message "Time is up!"
```

![timer](./assets/demo-timer-mode.gif)

### Stopwatch

```shell
tclock stopwatch
```

![stopwatch](./assets/demo-stopwatch-mode.gif)

### Countdown

```shell
tclock countdown --time 2026-01-01 --title 'New Year 2026'
```

`--time` accepts values such as `2026-01-01`, `20:00`, `2026-12-25 20:00:00`, or `2026-12-25T20:00:00-04:00`.

![countdown](./assets/demo-countdown-mode.gif)

## Configuration

`tclock` reads config from the XDG config path:

```text
$XDG_CONFIG_HOME/tclock/config.toml
```

That is usually:

```text
~/.config/tclock/config.toml
```

Missing config is ignored. Invalid TOML prints an error and falls back to defaults.

Example:

```toml
[default]
mode = "clock"
color = "green"
size = 1

[clock]
show_date = true
show_seconds = true
show_millis = false
timezone = "America/Sao_Paulo"

[timer]
durations = ["25m", "5m"]
titles = ["Focus", "Break"]
repeat = false
show_millis = true
start_paused = false
auto_quit = false
```

## Clock widgets

Clock mode can display command widgets below the clock. A widget runs a command, captures its output, renders ANSI colors/styles, and refreshes independently.

Widgets are useful for small status panels: GitHub pending work, calendars, system stats, reminders, CI state, or any command that prints useful text and exits.

The clock automatically sizes itself into the top area when widgets are configured, and the bottom area shows up to 2 widgets on square-ish terminals, 4 on wide terminals, and 6 on ultra-wide terminals.

Widgets with `position = "bottom"` are placed in a full-width band beneath the widget row instead, stacked in config order and each sized to exactly fit its output. The widget row keeps a minimum height when both are present, and a bottom widget that cannot get at least 3 rows is hidden rather than squeezed. Bottom widgets don't count against the per-row widget limits, so a status strip can coexist with a full row of columns.

When a widget has more output than fits on screen, scroll it with the mouse wheel over that widget. `Home` and `End` jump the active widget to the top or bottom. In clock mode, press `Shift+T` to cycle the configured clock theme; lowercase `t` still switches to Timer mode.

Each widget supports:

- `title`: optional display title; omit it to fall back to the command name, or set it to an empty string (`title = ""`) to suppress the title line entirely so the command output owns the whole widget (useful for self-rendered headers)
- `command`: executable string, or array form with arguments
- `refresh_secs`: refresh interval, default `900`
- `timeout_secs`: command timeout, default `30`
- `position`: `"auto"` (default, widget row) or `"bottom"` (full-width band below the row, sized to content)

`widget_themes` controls the clock-mode theme cycle. For built-in app palettes (`default`, `evangelion`, and `nerv`), the app themes the clock digits, date/header text, and widget base/chrome styles itself, and also injects the current theme name into every widget subprocess as `TCLOCK_WIDGET_THEME`. `tclock --theme nerv` or `TCLOCK_WIDGET_THEME=nerv tclock` chooses the initial theme and keeps the configured cycle order after it. Other names are still passed to widget commands, but the app UI falls back to default styling unless that palette is added to `tclock` too. Theme names are a contract between your config and the widget commands: a command must understand the name it receives if it wants to match its internal ANSI palette.

```toml
[clock]
widget_themes = ["default", "evangelion", "nerv"]
```

An empty or single-item list makes `Shift+T` harmless. For coherent app + bundled-widget theming, keep built-in names such as `default`, `evangelion`, and `nerv` unless you also add the palette to both `tclock` and `tclock-system-health`.

### Bundled example: system-health widget

The repo ships a ready-to-use widget at [`examples/widgets/tclock-system-health`](./examples/widgets/tclock-system-health). The AUR package installs it as `/usr/bin/tclock-system-health`; release tarballs include it beside `tclock`, so manual installs can extract it to `~/.local/bin` or copy it to `/usr/local/bin`. It renders a two-column health dashboard with a one-line verdict header: backup/cleanup timer freshness, timeshift snapshots, live system/jobs/storage state, and a full-width btrfs row (scrub age per filesystem, fstrim age, allocation pressure, device I/O error counters) — all without root.

Run it with no arguments and it auto-detects common setups (backup-looking user timers with staleness derived from each timer's own period, timeshift via grub-btrfs, btrfs rows only when btrfs is mounted, removable media excluded). Host-specific tuning is plain flags — see `tclock-system-health --help`. The intended pattern is a tiny wrapper script on your PATH holding your host's flags, referenced from the widget config:

The widget supports named color themes, including `default`, the original purple/lavender `evangelion` theme, and the screenshot-inspired red/amber/green `nerv` theme. It honors `TCLOCK_WIDGET_THEME`, so it works with `Shift+T` without a wrapper; an explicit `--theme` or `TCLOCK_SYSTEM_HEALTH_THEME` still wins. See [System-health widget themes](./docs/widget-themes.md) for usage and contributor notes. Packaged installs also include this guide as `/usr/share/doc/clock-tui/widget-themes.md`.

```toml
[[clock.widgets]]
title = ""                    # the script renders its own title+verdict line
command = "my-system-health"  # your wrapper around tclock-system-health
refresh_secs = 300
position = "bottom"
```

Example wrapper using the NERV theme:

```bash
#!/usr/bin/env bash
exec tclock-system-health --theme nerv "$@"
```

### Screenshot example

The screenshot at the top uses the current local config from this fork, with three widget commands:

- [`ghpending`](https://github.com/akitaonrails/ghpending) for GitHub pending tasks
- [`google-calendar-tui`](https://github.com/akitaonrails/google-calendar-tui) for Google Calendar agenda output
- `tclock-system-health` as a bottom status strip

```toml
[clock]
show_date = true

[[clock.widgets]]
title = "GitHub pending"
command = "ghpending"
refresh_secs = 900

[[clock.widgets]]
title = "Google Calendar"
command = "google-calendar-tui"
refresh_secs = 3600

[[clock.widgets]]
title = ""
command = "tclock-system-health"
refresh_secs = 300
position = "bottom"
```

Array commands are supported when you need arguments or a shell wrapper:

```toml
[[clock.widgets]]
title = "GPU"
command = ["nvidia-smi"]
refresh_secs = 60

[[clock.widgets]]
title = "Shell command"
command = ["sh", "-c", "printf 'hello from a widget'"]
```

Widget commands should be finite stdout-producing commands that exit. Long-running alternate-screen TUIs are not a good fit unless they also provide a command or flag that prints a snapshot and exits.

Widget output is intended for compact status text and is capped in memory; very large command outputs are truncated.

## Credits

Original project and core app by [Race604](https://github.com/race604). This fork keeps the original MIT license and continues the project with maintenance, packaging, and widget features.

## License

MIT License. See [LICENSE](./LICENSE).
