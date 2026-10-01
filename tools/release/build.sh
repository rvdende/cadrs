#!/usr/bin/env bash
# Builds the release archives for Linux and Windows into dist/:
#   cadrs-v<version>-x86_64-linux.tar.gz, cadrs-v<version>-x86_64-windows.zip, SHA256SUMS.txt
# (each archive: the stripped executable, README.md and the licenses).
#
# - Linux is built in the `cadrs-linux-build` container (Ubuntu 22.04, see the Dockerfile here),
#   so the binary needs only glibc 2.35. Its build output stays in target/linux-release and it
#   shares ~/.cargo/registry and ~/.cargo/git with the host, so only the first run builds OCCT
#   and downloads the crates; later runs rebuild just what changed.
# - Windows is built on the host with the MinGW toolchain (target/x86_64-pc-windows-gnu, cached
#   like any cargo build).
#
# The two builds run one after the other (never two cargo builds at once). Publishing is left to
# you: the script prints the `gh release create` command.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
image=cadrs-linux-build
dist="$root/dist"
echo "cadrs v$version"

if ! docker image inspect "$image" >/dev/null 2>&1; then
    docker build -t "$image" tools/release
fi

# The container runs as you, with its own CARGO_HOME (the host's has a config the container
# can't use) holding the host's crate downloads. The repo's .cargo/config.toml links with mold and
# passes a nightly flag, and rust-toolchain.toml asks for nightly; the container has none of
# these, so the linker, flags and toolchain are reset to its own (stable).
cargo_home="$root/target/linux-release/cargo-home"
mkdir -p "$cargo_home/registry" "$cargo_home/git" "$HOME/.cargo/registry" "$HOME/.cargo/git"
docker run --rm \
    -u "$(id -u):$(id -g)" \
    -v "$root:/src" -w /src \
    -v "$HOME/.cargo/registry:/src/target/linux-release/cargo-home/registry" \
    -v "$HOME/.cargo/git:/src/target/linux-release/cargo-home/git" \
    -e HOME=/tmp \
    -e CARGO_HOME=/src/target/linux-release/cargo-home \
    -e CARGO_TARGET_DIR=/src/target/linux-release \
    -e CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc \
    -e RUSTFLAGS= \
    -e RUSTUP_TOOLCHAIN=stable \
    "$image" cargo build --release --locked -p cadrs

cargo build --release --locked --target x86_64-pc-windows-gnu -p cadrs

package() { # <dir name> <executable> <exe name in archive> <strip>
    local dir="$dist/$1"
    rm -rf "$dir"
    mkdir -p "$dir"
    cp "$2" "$dir/$3"
    "$4" "$dir/$3"
    cp README.md LICENSE-APACHE LICENSE-MIT "$dir/"
}

rm -rf "$dist"
mkdir -p "$dist"
linux="cadrs-v$version-x86_64-linux"
windows="cadrs-v$version-x86_64-windows"
package "$linux" target/linux-release/release/cadrs cadrs strip
package "$windows" target/x86_64-pc-windows-gnu/release/cadrs.exe cadrs.exe x86_64-w64-mingw32-strip
(cd "$dist" && tar czf "$linux.tar.gz" "$linux" && zip -qr "$windows.zip" "$windows")
(cd "$dist" && sha256sum "$linux.tar.gz" "$windows.zip" > SHA256SUMS.txt && cat SHA256SUMS.txt)

echo
echo "To publish:"
echo "  gh release create v$version dist/$linux.tar.gz dist/$windows.zip dist/SHA256SUMS.txt --title \"cadrs v$version\" --notes-file <notes.md>"
