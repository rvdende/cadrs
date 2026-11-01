#!/usr/bin/env bash
# Builds cadrs for the browser into OUT_DIR (default: target/web):
#   index.html, files.js, cadrs.js (and its snippets/), cadrs_bg.wasm.gz, cadrs.png, and
#   bundle.tar.gz: the built-in libraries and the sample documents, which the page unpacks into
#   its files (/libraries, /fixtures) before the app starts
# Serve that folder over HTTP (`python3 -m http.server -d target/web`); https://cadrs.net is the
# latest release's (.github/workflows/pages.yml). It needs a browser with WebGPU; in one without,
# the page offers the downloads.
#
# Needs the wasm32 target (rust-toolchain.toml installs it) and wasm-bindgen-cli at the version
# of the wasm-bindgen crate in Cargo.lock:
#   cargo install --locked wasm-bindgen-cli --version <version>
# (wasm-opt -O2 isn't run: on this module it takes 13 minutes and 8 GB for 12% less, and the
# gzipped download comes out no smaller.)
# `CADRS_WEB_DEBUG=1` keeps the functions' names, for readable stack traces (about 100 MB more).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
out="${1:-target/web}"
mkdir -p "$out"

want="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"/\1/p}' Cargo.lock)"
have="$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2 || true)"
if [[ "$have" != "$want" ]]; then
    echo "wasm-bindgen-cli $want is needed (found: ${have:-none}):" >&2
    echo "  cargo install --locked wasm-bindgen-cli --version $want" >&2
    exit 1
fi

# Without the default `profiler` feature (bevy/trace): in a page, Bevy's log layer hands each of
# its spans (one a run of every system and schedule, thousands a frame) to the browser's
# performance timeline (tracing-wasm), and frames took about seven times as long. The frame panel
# then shows the GPU passes alone.
cargo build -r --locked --target wasm32-unknown-unknown -p cadrs --no-default-features -F gpu
names=(--remove-name-section)
[[ "${CADRS_WEB_DEBUG:-0}" != 0 ]] && names=()
wasm-bindgen --target web --no-typescript "${names[@]}" --remove-producers-section \
    --out-name cadrs --out-dir "$out" target/wasm32-unknown-unknown/release/cadrs.wasm

# The samples are `cadrs_core::documents_page::SAMPLES`' keys.
samples="$(sed -n 's/^ *key: "\(.*\)",$/fixtures\/\1.cadrs/p' crates/cadrs_core/src/documents_page.rs)"
# shellcheck disable=SC2086
tar --format=ustar --owner=0 --group=0 --mtime=@0 --sort=name -czf "$out/bundle.tar.gz" libraries $samples

# The page unpacks it as it downloads (`index.html`).
gzip -9 -f "$out/cadrs_bg.wasm"

cp tools/web/index.html tools/web/files.js "$out/"
cp assets/icons/cadrs.png "$out/"
du -h "$out"/cadrs_bg.wasm.gz
echo "Built $out. Try it: python3 -m http.server -d $out 8080"
