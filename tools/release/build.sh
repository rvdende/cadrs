#!/usr/bin/env bash
# Builds the release archives for Linux and Windows into dist/:
#   cadrs-v<version>-x86_64-linux.tar.gz, cadrs-v<version>-x86_64-windows.zip, SHA256SUMS.txt
# (each archive: the stripped executable, the built-in libraries/, README.md and the licenses).
#
# - Linux is built in the `cadrs-linux-build` container (Ubuntu 22.04, see the Dockerfile here),
#   so the binary needs only glibc 2.35. Its build output stays in target/linux-release and it
#   shares ~/.cargo/registry and ~/.cargo/git with the host, so only the first run downloads the
#   crates; later runs rebuild just what changed.
# - Windows is built on the host with the MinGW toolchain (target/x86_64-pc-windows-gnu, cached
#   like any cargo build).
#
# The two builds run one after the other (never two cargo builds at once). Publishing is left to
# you: the script prints the `gh release create` command. Pushing a version tag does all of this
# on GitHub instead (.github/workflows/release.yml).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
# The image carries the nightly pinned in rust-toolchain.toml, so a new pin builds a new image.
toolchain="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
image="cadrs-linux-build:$toolchain"
dist="$root/dist"
echo "cadrs v$version"
# Whole-crate builds, not .cargo/config.toml's incremental ones.
export CARGO_INCREMENTAL=0

if ! docker image inspect "$image" >/dev/null 2>&1; then
    docker build -t "$image" --build-arg "TOOLCHAIN=$toolchain" tools/release
fi

# The container runs as you, with its own CARGO_HOME (the host's has a config the container
# can't use) holding the host's crate downloads. The repo's .cargo/config.toml links with clang and
# mold, which the container doesn't have, so the linker and flags are reset; the toolchain is the
# image's copy of rust-toolchain.toml's nightly.
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
    -e CARGO_INCREMENTAL=0 \
    -e RUSTUP_TOOLCHAIN="$toolchain" \
    "$image" cargo build --release --locked -p cadrs

cargo build --release --locked --target x86_64-pc-windows-gnu -p cadrs

rm -rf "$dist"
tools/release/package.sh linux target/linux-release/release/cadrs
tools/release/package.sh windows target/x86_64-pc-windows-gnu/release/cadrs.exe
linux="cadrs-v$version-x86_64-linux"
windows="cadrs-v$version-x86_64-windows"
(cd "$dist" && sha256sum "$linux.tar.gz" "$windows.zip" > SHA256SUMS.txt && cat SHA256SUMS.txt)

echo
echo "To publish:"
echo "  gh release create v$version dist/$linux.tar.gz dist/$windows.zip dist/SHA256SUMS.txt --title \"cadrs v$version\" --notes-file <notes.md>"
