# AGENTS.md

Neovide: a cross-platform GUI for Neovim (Rust). Single binary crate plus the `neovide-derive` proc-macro workspace member (`Cargo.toml`). Uses winit for the window/event loop and skia for rendering (D3D on Windows, Metal on macOS, OpenGL elsewhere). Neovide **embeds** a real Neovim process at runtime and renders its UI events.

## Commands

- Dev run: `cargo run` — requires `nvim` >= 0.10 on PATH (version check bails at startup, `NEOVIM_REQUIRED_VERSION` in `src/bridge/mod.rs`). Override binary with `--neovim-bin <path>` or `neovim_bin` in the config file.
- Tests: `cargo nextest run` (run via nextest, not `cargo test` — CI uses `cargo nextest run --profile ci`; profile junit config lives in `.config/nextest.toml`; `cargo binstall cargo-nextest` if missing). Matrices on stable + nightly across Windows/macOS/Linux.
- Lint: `cargo clippy --all-targets -- -D warnings` — warnings are denied; keep it clean (nightly clippy is allowed to fail in CI, stable is not).
- Format: `cargo fmt --all --check`. `rustfmt.toml` sets `style_edition = "2024"` + `use_small_heuristics = "Max"` — don't reformat with default rustfmt settings.
- Lua: `stylua` with `stylua.toml` (4-space indent, 120 col).
- Website docs (`website/`, mdbook): markdownlint per `website/.markdownlintrc` (CI lints `website/**/*.md`).

## Architecture

The four-component pipeline is documented in an architecture comment in `src/main.rs::setup()`. Read it before touching cross-component code.

- `src/bridge/` — async connection to the embedded Neovim process (nvim-rs). `NeovimHandler` consumes nvim → GUI events (redraw, `setting_changed`, `option_changed`); `SerialCommand`/`ParallelCommand` + `send_ui` send GUI → nvim commands. Serial commands must stay ordered; parallel ones may not.
- `src/editor/` — CPU-only processing of redraw events on its own thread (ligatures, multi-window, grid state) → emits `DrawCommand`.
- `src/renderer/` — skia drawing of editor output; keeps draw surfaces to avoid full redraws. Platform backends: `d3d.rs` (Windows), `metal.rs` (macOS), `opengl.rs` (fallback).
- `src/window/` — winit event loop: input handling (`keyboard_manager.rs`, `mouse_manager.rs`) and display; forwards `UICommand` back to the bridge.

## Settings & config

- File config: `config.toml` (kebab-case keys) under `NEOVIDE_CONFIG` env override, else platform config dir (`~/.config/neovide` on Linux, `%APPDATA%\neovide` on Windows). Loaded once at startup and **hot-reloaded** (font, box-drawing, window geometry). Docs: `website/docs/config-file.md`.
- Live settings: `neovide_*` global variables / options in Neovim are watched from `lua/init.lua` and forwarded to Rust as `setting_changed` / `option_changed` rpc events. New settings must be **registered** (see `Settings::register` calls in `src/main.rs::setup()`) AND added to the watch lists built in `src/bridge/setup.rs` (passed to init.lua as `global_variable_settings` / `option_settings`). Missing either side silently breaks the setting.
- Env-mutating tests use `scoped_env` + `serial_test::serial` (see `src/cmd_line.rs` tests) — keep that pattern for new tests that touch env vars or global state.

## Gotchas

- `lua/init.lua` and `lua/exit_handler.lua` are compiled into the binary via `include_str!` (`src/bridge/setup.rs`, `src/bridge/ui_commands.rs`) — they are NOT loaded from disk at runtime. Edits require a rebuild, and changes to rpc methods must be mirrored in the Rust bridge handler.
- Build version comes from `git describe` in `build.rs` (`NEOVIDE_BUILD_VERSION`); non-tag commits render as `nightly-<height>+g<commit>` and dirty trees get `-dirty` appended. build.rs reruns on any change to tracked files.
- Platform code is heavily `#[cfg(target_os = ...)]`-gated, and Cargo.toml has per-OS dependency targets (objc2 crates, `windows`, `skia-safe` features vary by OS). Don't add platform APIs to shared code without gating.
- Panics are intercepted and appended to `neovide_backtraces.log` in the data dir (`backtraces_path` config key, `NEOVIDE_BACKTRACES` env override). Running with `--log` / `RUST_LOG` writes `neovide_r*.log` into the working directory (gitignored).
- Test names deliberately use camelCase (`#![cfg_attr(test, allow(non_snake_case))]` in `main.rs`) — don't rename them to snake_case.
- The repo is a work-in-progress PR branch (Cargo.toml version `0.16.2-with-PR3530`). Untracked scratch/backup files with garbage names sit inside `src/` (e.g. `src/bridge/asbidaiuhsduia.rs`, `src/renderer/cursor_renderer/vfxvfxvfx….rs`, `…/cursor_vfx - 副本.rs.txt`). They are **not** declared in any `mod.rs` and not part of the build — don't edit, format, or "fix" them.
- Release packaging (WiX MSI on Windows, `macos-builder/`, `ubuntu-builder/`, `snap/`) is CI-only; Windows release builds need `RUSTFLAGS="-Ctarget-feature=+crt-static"`, ninja, and git long-path support (see `.github/workflows/build.yml`). First local build compiles/downloads skia and is slow.