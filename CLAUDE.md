# cadrs

Onshape-style CAD in Rust + Bevy 0.19.1. Local notes in `docs/` (git-ignored: `PLAN.md`,
`PROGRESS.md`, `ORCHESTRATOR.md` for background workers). Kernel design: `crates/cadrs_kernel/README.md`.

## Commands
- Build in release everywhere so nothing builds twice: `cargo build -r`,
  `cargo clippy --workspace --all-targets`.
- Tests:
  - `cargo test -r`: the fast tests of the crates without Bevy (about a minute).
  - `cargo test -r -- --ignored`: the slow core tests (each over 5 s).
  - `cargo test -r --workspace -F cadrs/app-tests`: also the UI crates and the app's own tests.
  - `cargo test -r -p cadrs -F app-tests --test golden -- --ignored`: the golden screenshot tests
    (slow, GPU; `CADRS_BLESS=1` records new baselines).
- Headless scenario: `cargo run -r -- --headless --scenario <name>` → `target/scenarios/<name>/`.
- Windowed run from a shell with no display: prefix `WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000`.
- Windows exe: `cargo build -r --target x86_64-pc-windows-gnu` (MinGW is installed).
- Cold builds take 4+ minutes. Never run two cargo builds at once.

## Layout
- Crates in `crates/*`; shared versions in root `[workspace.dependencies]`.
- `cadrs_core` and `cadrs_sketch` don't depend on bevy.
- Reusable UI goes in `cadrs_ui`, not `cadrs_app`. Reference for components, API style and
  interaction: gpui-component (`~/.cargo/registry/src/*/gpui-component-0.6.6/src/`) and GPUI
  (`gpui-pre-0.3.6`). Don't add GPUI as a dependency.

## Conventions
- Every document or sketch change goes through the command/undo layer. Give UI elements a `Name`.
- Icons: add missing ones to our icon-rs repo (`~/work/icon-rs`), then use them from there. Inter font.
  Never copy Onshape's logo, name or icons.
- Units are mm.
- End commit messages with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
