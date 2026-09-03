# AGENTS.md

## Commands
- CI parity is `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --locked --verbose`, `cargo test --locked --verbose`, then `cargo xtask` followed by `git diff --exit-code assets/gen` (`.github/workflows/rust.yml`). Run all before claiming CI parity.
- Local aliases live in `.cargo/config.toml`: `cargo main -- <tclock args>` runs the app crate, and `cargo xtask` runs the generator helper.
- For package-scoped checks, use the workspace package names: `cargo test --package clock-tui` or `cargo test --package xtask`.
- Rust CI enforces rustfmt, clippy warnings as errors, locked dependency resolution, tests, and generated completion/manpage drift.

## Repo shape
- Root is a Cargo workspace with two members: `clock-tui` (the published app crate) and `xtask` (generation helper).
- `clock-tui` exposes library `clock_tui` from `clock-tui/src/lib.rs` and binary `tclock` from `clock-tui/src/bin/main.rs`.
- CLI modes and clap parsing are centralized in `clock-tui/src/app.rs`; mode widgets are under `clock-tui/src/app/modes/`.
- `clock-tui/src/bin/main.rs` owns terminal raw/alternate-screen setup and the draw/key loop. Keep CLI parsing before alternate-screen setup so `--help` prints normally.

## Generated assets
- `cargo xtask` regenerates shell completions and the `tclock.1` manpage into `assets/gen` using the clap `App` definition. CI fails if `cargo xtask` changes `assets/gen`, so rerun it after changing CLI flags/subcommands/help text.

## Release / packaging
- `.github/workflows/release.yml` builds `tclock` release tarballs for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, creates a GitHub Release on `v*` tags, and publishes `clock-tui-bin` to AUR when `AUR_SSH_KEY` is configured.
- The upstream `clock-tui` AUR source package is maintained by someone else; this fork publishes only `clock-tui-bin`.
- The AUR template lives in `packaging/aur/PKGBUILD-bin`; keep `pkgver` in sync with `clock-tui/Cargo.toml` and leave sha256 values as `SKIP` between releases because the release workflow pins them.
- `clock-tui/Cargo.toml` has cargo-binstall metadata pointing at GitHub Release tarballs; if artifact names change, update that metadata too.

## Runtime gotchas
- Config is loaded from the first existing candidate in this order: absolute `$XDG_CONFIG_HOME/tclock/config.toml`, `~/.config/tclock/config.toml`, then `dirs::config_dir()/tclock/config.toml` as the OS-native fallback. Duplicate paths are removed; missing config is ignored, and invalid TOML prints an error then falls back to defaults.
- Display options (`--title`, `--seconds`/`--no-seconds`, `--millis`/`--no-millis`, `--date`/`--no-date`, `--paused`, plus `--color`/`--size`/`--theme`) are clap `global` args on `App` (`DisplayArgs` in `clock-tui/src/app.rs`), so they parse before or after the mode name. `DisplayOptions::resolve` applies precedence: CLI flag, then the mode's config section, then `[default]`, then the per-mode `ModeDefaults`. Config sections share the `DisplayConfig` struct via `#[serde(flatten)]`; mode-only settings (timezone, durations, countdown time) stay on their sections. Adding a display option means touching `DisplayArgs`, `DisplayConfig`, and `DisplayOptions::resolve` together.
- The `command_line_definition_passes_clap_self_checks` test runs `App::command().debug_assert()`. Keep it: clap only checks duplicate short flags in debug builds, and the global `-c`/`-s`/`-T`/`-S`/`-m`/`-M`/`-D`/`-P` shorts must not collide with any subcommand flag.
- The main key bindings are in `clock-tui/src/bin/main.rs`: `q` exits, space pauses/resumes supported modes, and `c`/`w`/`t` switch to clock/stopwatch/timer. The switch keys build a bare `Mode` (no durations, no timezone); `init_app` fills timer durations and `execute` from `[timer]` when the CLI gave none, and display flags on `App` carry over. There is no countdown switch key in the main loop. Visible clock widgets can contribute character bindings through `[[clock.widgets.popup_actions]]`; these are tried before built-in mode switches (`q` remains reserved). Popup input is modal: `Esc` closes, arrows/PageUp/PageDown/Home/End and the mouse wheel scroll. Other clock-mode keys (`Shift+T` theme cycle, `g` widget-group cycle, `z` clock-only layout toggle, `Home`/`End` widget scroll) are forwarded to `App::on_key`, which dispatches them in `clock-tui/src/app.rs`.
- Clock widgets are config-only under `[[clock.widgets]]`, clock-mode only, implemented in `clock-tui/src/app/modes/clock_widget.rs`; widget commands and generic popup-action commands run from managed worker state, not from `Widget::render()`. Popup actions rerun the widget command plus configured `args` unless they provide a replacement `command`; the clock framework owns key routing/execution/modal UI while the command owns action semantics.
- An optional `group` on a widget makes it part of a set cycled with `g`; ungrouped widgets are always visible. Group membership is filtered in `ClockWidgets::render`, which is what sets `visible`, and `tick()` skips non-visible widgets — so a hidden group costs no subprocesses.

## Tests
- Focused unit tests exist in `clock-tui/src/config.rs`, `clock-tui/src/app.rs` (CLI parsing and display precedence), and `clock-tui/src/app/modes/clock_widget.rs`; use Cargo’s standard filter, e.g. `cargo test clock_widget --package clock-tui`.
- `assets/gen` is gitignored, so the CI `git diff --exit-code assets/gen` step cannot detect drift; still run `cargo xtask` so local completions and the manpage match the CLI.
