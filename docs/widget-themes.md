# System-health widget themes

`tclock-system-health` supports named ANSI color themes for the bundled bottom widget.

## Use a theme

List available themes:

```bash
tclock-system-health --list-themes
```

Use the default theme explicitly:

```bash
tclock-system-health --theme default
```

Use the screenshot-inspired NERV terminal theme:

```bash
tclock-system-health --theme nerv
```

Use the older purple/lavender Evangelion-inspired palette:

```bash
tclock-system-health --theme evangelion
```

When used as a `tclock` clock widget, `widget_themes` controls the clock-mode theme cycle. Press `Shift+T` in clock mode to cycle the configured themes; lowercase `t` still switches to Timer mode. For built-in app palettes (`default`, `evangelion`, and `nerv`), the app themes the clock digits, date/header text, and widget base/chrome styles itself, and also sets `TCLOCK_WIDGET_THEME` for every widget subprocess. Other names are still passed to widget commands, but the app UI falls back to default styling unless that palette is added to `tclock` too. Theme names are a contract between your config and the widget commands: a command must understand the name it receives if it wants to match its internal ANSI palette.

Use `tclock --theme nerv` or `TCLOCK_WIDGET_THEME=nerv tclock` to choose the initial app/widget theme without editing config. Explicit `--theme` wins over the environment variable.

```toml
[clock]
widget_themes = ["default", "evangelion", "nerv"]
```

An empty or single-item list makes `Shift+T` a no-op. For coherent app + `tclock-system-health` theming, keep built-in names such as `default`, `evangelion`, and `nerv` unless you add the palette to both `tclock` and the script below.

You can also set the system-health-specific environment variable, which is convenient in wrapper scripts and takes precedence over `TCLOCK_WIDGET_THEME`:

```bash
#!/usr/bin/env bash
exec tclock-system-health --theme nerv --snapshots "$@"
```

or:

```bash
TCLOCK_SYSTEM_HEALTH_THEME=nerv tclock-system-health
```

Precedence is: explicit `--theme`, then `TCLOCK_SYSTEM_HEALTH_THEME`, then generic `TCLOCK_WIDGET_THEME`, then `default`.

Then point a bottom widget at the wrapper:

```toml
[[clock.widgets]]
title = ""
command = "my-system-health"
refresh_secs = 300
position = "bottom"

[[clock.widgets.popup_actions]]
key = "d"
label = "details"
args = ["--details"]
```

The popup action is theme-aware too: press `d` for a flagged-problems summary followed by failed/retained-unit, timer-job, system (zombies/load/memory), snapshot, scheduled-job, storage-capacity, and Btrfs allocation/I/O details, then `Esc` to close it. Diagnostics are read-only, and the largest-directory scan is capped at three seconds per filesystem over the configured warning threshold.

## Built-in themes

- `default`: the original compact health palette: green OK, yellow warning, red error, cyan labels.
- `evangelion`: the original purple/lavender Evangelion-inspired palette with orange labels and EVA green accents.
- `nerv`: screenshot-inspired NERV colors: red monitor clock, hot amber chrome/warnings, EVA mint OK/active markers, alarm red failures.

## Add a new theme

The bundled widget's ANSI themes live in `examples/widgets/tclock-system-health` and are intentionally small Bash functions. To make a new name also change `tclock`'s own clock/date/widget chrome colors, add a matching app palette in `ClockTheme::named`.

Add a function named `theme_<name>()` that sets these semantic variables:

- `G`: OK/success values.
- `Y`: warning values.
- `R`: error/critical values.
- `D`: dim separators and secondary text.
- `B`: title emphasis.
- `N`: reset sequence.
- `LBL`: section labels.
- `OK`, `WA`, `ER`: status glyphs built from the colors above.

Example skeleton:

```bash
theme_example() {
  G=$(sgr '38;5;118')
  Y=$(sgr '38;5;208')
  R=$(sgr '38;5;196')
  D=$'\033[2m'
  B=$'\033[1m'
  N=$'\033[0m'
  LBL=$(sgr '1;38;5;39')
  OK="${G}✔${N}"
  WA="${Y}▲${N}"
  ER="${R}✖${N}"
}
```

Then:

1. Add the name to `list_themes()`.
2. Add a branch in `apply_theme()`.
3. Run:

```bash
bash -n examples/widgets/tclock-system-health
examples/widgets/tclock-system-health --list-themes
examples/widgets/tclock-system-health --theme <name> --no-btrfs --single-column
```

Keep themes semantic rather than hard-coding colors in report logic. That keeps future themes easy to review and avoids scattering style decisions through the health checks.
