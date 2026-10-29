# cadrs

Onshape-style CAD in Rust + Bevy 0.20.1. Local notes in `docs/` (git-ignored: `PLAN.md`,
`PROGRESS.md`, `ORCHESTRATOR.md` for background workers). Kernel design: `crates/cadrs_kernel/README.md`.

## Commands
- Build in release everywhere so nothing builds twice: `cargo build -r`,
  `cargo clippy --workspace --all-targets`.
- Format before committing: `cargo fmt --all` (and `cargo fmt` in `site/`); CI's "Format" job
  checks it.
- Tests:
  - `cargo test -r`: the fast tests of the crates without Bevy (about a minute).
  - `cargo test -r -- --ignored`: the slow core tests (each over 5 s).
  - `cargo test -r --workspace -F cadrs/app-tests`: also the UI crates and the app's own tests.
  - `cargo test -r -p cadrs -F app-tests --test golden -- --ignored`: the golden screenshot tests
    (slow, GPU; `CADRS_BLESS=1` records new baselines).
- Headless scenario: `cargo run -r -- --headless --scenario <name>` → `target/scenarios/<name>/`.
- The app runs the kernel in a process of its own (`cadrs_core::rebuild::process`, so a
  kernel crash costs a rebuild, not the app); `CADRS_KERNEL=thread` keeps it in-process for
  debugging. Kernel work besides rebuilds is a named `rebuild::task::Task`, not a closure.
- Windowed run from a shell with no display: prefix `WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000`.
- Windows exe: `cargo build -r --target x86_64-pc-windows-gnu` (MinGW is installed).
- Browser build: `tools/web/build.sh` → `target/web/` (WebAssembly, drawn with WebGPU; serve the
  folder over HTTP). Needs wasm-bindgen-cli at Cargo.lock's version. Releases attach it as
  `cadrs-v<version>-web.tar.gz` and the site serves the latest at `/cadrs/app/`. A page has one
  thread and no `std::fs` or `std` clock, so code it runs (everything but `cadrs_mcp`, the
  updater and the kernel's debug dumps) reads files through `cadrs_fs` (also `cadrs_fs::exists`,
  `is_file`, `is_dir`, not `Path`'s), tells the time with `web_time`, and starts threads with
  `cadrs_core::background::spawn` (inline in a page); kernel jobs run inline there. CI's
  "Check (browser)" job compiles it.
- Release: bump `version` in the root `Cargo.toml`, write `tools/release/notes/v<version>.md`,
  then push the tag `v<version>`; `.github/workflows/release.yml` builds the Linux and Windows
  archives and the macOS disk image (universal, ad-hoc signed; only on GitHub's macOS runner),
  publishes the GitHub release and redeploys the site. `tools/release/build.sh` builds the Linux
  and Windows archives locally. Actions › Release › Run workflow with "Only build" builds them all
  as the run's artifacts without releasing.
- Autorouter GPU shader: after changing `crates/cadrs_router_kernels`, rebuild its SPIR-V with
  `cargo gpu build --shader-crate crates/cadrs_router_kernels --output-dir crates/cadrs_router/shaders`
  (rust-gpu; commit the `.spv`). Without one the router runs on the CPU. Same for the kernel's
  GPU spike (`crates/cadrs_gpu_kernels` → `crates/cadrs_gpu/shaders`). Install cargo-gpu with
  `cargo +nightly-2026-07-03 install cargo-gpu --version 0.11.0 --locked`.
- Kernel debugging: `CADRS_PROFILE=1`, `CADRS_BOOL_DEBUG=1` and `CADRS_DUMP_DIR=<dir>` (inputs of
  failing shells/fillets, replayed by `crates/cadrs_kernel/tests/native_debug.rs`); see the
  kernel README. The kernel is pure Rust (`cadrs_kernel::native`); there is no OCCT any more.
- App debugging: `CADRS_SLOW_FRAMES=<ms>` logs every frame slower than that with its slowest
  systems (`cadrs_app::debug_tools`; add further env-switched aids there). `CADRS_NO_VSYNC=1`
  shows a frame's real cost; `CADRS_UI_CHANGES=1` names the UI nodes that change every frame
  (each one costs a full UI layout), and with Bevy's `track_location` feature the code that
  changed them. System times (the
  F12 panel, `get_frame_stats`) are off unless asked for, as they cost about a fifth of a frame:
  start with `CADRS_FRAME_PROFILE=1`, or the *Time systems* preference. `CADRS_TRACE=<file.json>`
  (with `CADRS_TRACE_AT=<s>`) writes a quarter second of every system's span, by thread, as a
  Chrome trace; `CADRS_SINGLE_THREADED=Update,…` runs schedules on the main thread for `perf`.
- Cold builds take 4+ minutes. Never run two cargo builds at once.
- Project site: `site/` is a Proa app of its own (stable toolchain, built with `RUSTFLAGS=`),
  exported to GitHub Pages by `.github/workflows/pages.yml`; see `site/README.md`.

## Layout
- Crates in `crates/*`; shared versions in root `[workspace.dependencies]`.
- `vendor/`: patched copies of dependencies (`[patch.crates-io]`), each with a `CADRS.md` saying
  what changed and why: gpu-allocator stages wgpu's uploads in system memory.
- `cadrs_core` and `cadrs_sketch` don't depend on bevy.
- Library parts are data, never code: `libraries/<Library>/<name>.symbol.ron|.footprint.ron`,
  read at runtime. `cargo run -r -p cadrs_libgen -- libraries` rewrites the built-in ones from
  their generators. The library browser's JLCPCB parts tab (`cadrs_easyeda`) downloads parts
  into the user's library folder (`~/.local/share/cadrs/libraries`).
- Reusable UI goes in `cadrs_ui`, not `cadrs_app`. Reference for components, API style and
  interaction: gpui-component (`~/.cargo/registry/src/*/gpui-component-0.6.6/src/`) and GPUI
  (`gpui-pre-0.3.6`). Don't add GPUI as a dependency.

## Conventions
- Every document or sketch change goes through the command/undo layer. Give UI elements a `Name`.
- Icons: add missing ones to our icon-rs repo (`~/work/icon-rs`), then use them from there. Inter font.
  Never copy Onshape's logo, name or icons.
- Units are mm.
- End commit messages with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
