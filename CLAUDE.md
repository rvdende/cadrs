# cadrs

An Onshape-style CAD app in Rust with Bevy 0.19.1. The plan is in `docs/PLAN.md` and the current
state in `docs/PROGRESS.md`. Background workers follow `docs/ORCHESTRATOR.md`. `docs/` is
git-ignored: those files are the maintainer's local working notes and are not in the repo.
The kernel's design is in `crates/cadrs_kernel/README.md`.

## Layout
- Cargo workspace; crates live in `crates/*`. Shared dependency versions go in the root
  `[workspace.dependencies]`.
- `cadrs_core` and `cadrs_sketch` must not depend on bevy.
- `cadrs_ui` is our Bevy widget library. Put new reusable UI components there, not in
  `cadrs_app`. Use gpui-component (`~/.cargo/registry/src/*/gpui-component-0.6.6/src/`) and GPUI
  (`gpui-pre-0.3.6`) as the reference for the component set, API style and interaction details. Do
  not add GPUI as a dependency.

## Commands
- Build: `cargo build`. Test: `cargo test --workspace`. Lint: `cargo clippy --workspace --all-targets`.
- Headless scenario: `cargo run -- --headless --scenario <name>` writes PNGs to
  `target/scenarios/<name>/`.
- Windowed run from a shell with no display: prefix `WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000`.
- Windows exe: `cargo build --release --target x86_64-pc-windows-gnu`, using the MinGW toolchain,
  which is already installed.
- Full builds are slow (4+ minutes cold). Never run two cargo builds at the same time.

## Conventions
- Put every document or sketch mutation through the command and undo layer.
- Give UI elements a `Name` so scenarios can target them.
- Use icons from our own icon-rs crate (https://rvdende.github.io/icon-rs/; add missing ones there)
  and the Inter font. Never copy Onshape's logo, name or icons.
- Units are mm by default.

## Commits
End commit messages with:
Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
