# Getting cadrs

There are three ways to use cadrs: open it in a browser, install a release, or build it from the
source code.

## In a browser

Open [cadrs.net](https://cadrs.net/). The whole app runs in the page, and your documents stay in
that browser's storage. You need a browser with WebGPU or WebGL2: a recent Chrome, Edge, Firefox
or Safari. The browser version has no MCP server for AI assistants and no SpaceMouse support.

## Installing a release

Each [release on GitHub](https://github.com/rvdende/cadrs/releases) has a file for each system.
The newest release's are also at these addresses:

| System | File | Needs |
| --- | --- | --- |
| Windows | [cadrs-x86_64-windows.zip](https://github.com/rvdende/cadrs/releases/latest/download/cadrs-x86_64-windows.zip) | Windows 10 or 11, x86_64 |
| macOS | [cadrs-universal-macos.dmg](https://github.com/rvdende/cadrs/releases/latest/download/cadrs-universal-macos.dmg) | macOS 11 or newer, Apple silicon or Intel |
| Linux | [cadrs-x86_64-linux.tar.gz](https://github.com/rvdende/cadrs/releases/latest/download/cadrs-x86_64-linux.tar.gz) | x86_64 with glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 or newer), a Vulkan driver |

- **Windows and Linux:** unpack the archive and run `cadrs.exe` (Windows) or `cadrs` (Linux),
  keeping the `libraries` folder next to it. Nothing else needs installing. The Windows build
  isn't code-signed yet, so SmartScreen may warn the first time: choose **More info › Run
  anyway**.
- **macOS:** open the `.dmg` and drag cadrs to Applications. It isn't signed with an Apple
  Developer ID yet: the first time, right-click it and choose **Open**, then **Open** again. On
  macOS 15 and later, open it once, then choose **Open Anyway** in System Settings › Privacy &
  Security.

cadrs keeps your documents in its data folder: `%APPDATA%\cadrs\` on Windows,
`~/Library/Application Support/cadrs/` on macOS and `~/.local/share/cadrs/` on Linux. You don't
need to open it yourself; the documents page has **Open documents folder** in its menu if you
want to.

## Building from the source code

You need Git and [rustup](https://rustup.rs/). The repository names the Rust version it builds
with (in `rust-toolchain.toml`), and rustup installs it on the first build. On Linux you also
need clang and mold, and on Debian or Ubuntu these packages:
`cmake g++ pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.

```sh
git clone https://github.com/rvdende/cadrs
cd cadrs
cargo run -r
```

The first build takes a long while, since it compiles the kernel and the Bevy engine. Later
builds are much quicker. `cargo install-local` installs the build for your user, with a launcher
entry.

## Where to start

When cadrs opens you see the documents page. **New document** makes a document with an empty
Part Studio; **New document ▾ › From a sample…** opens a copy of one of the bundled samples, which
is a good way to look around. Press **Shift+/** in a document to see the
[keyboard shortcuts](reference/keyboard-shortcuts.md), and choose your mouse buttons under
**Preferences › Navigation** (there are presets for Onshape, SolidWorks, Inventor and Creo).
