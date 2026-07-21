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
- The main key bindings are in `clock-tui/src/bin/main.rs`: `q` exits, space pauses/resumes supported modes, and `c`/`w`/`t` switch to clock/stopwatch/timer. There is no countdown switch key in the main loop. Clock-mode-only keys (`Shift+T` theme cycle, `g` widget-group cycle, `Home`/`End` scroll) are forwarded to `App::on_key`, which dispatches them in `clock-tui/src/app.rs`.
- Clock widgets are config-only under `[[clock.widgets]]`, clock-mode only, implemented in `clock-tui/src/app/modes/clock_widget.rs`; widget commands run from `App::tick()` state, not from `Widget::render()`.
- An optional `group` on a widget makes it part of a set cycled with `g`; ungrouped widgets are always visible. Group membership is filtered in `ClockWidgets::render`, which is what sets `visible`, and `tick()` skips non-visible widgets — so a hidden group costs no subprocesses.

## Tests
- Focused unit tests exist in `clock-tui/src/config.rs` and `clock-tui/src/app/modes/clock_widget.rs`; use Cargo’s standard filter, e.g. `cargo test clock_widget --package clock-tui`.
