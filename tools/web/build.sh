#!/usr/bin/env bash
# Builds cadrs for the browser into OUT_DIR (default: target/web):
#   index.html, files.js, cadrs.png,
#   cadrs.js (and its snippets/) and cadrs_bg.wasm.gz: the app drawn with WebGPU,
#   cadrs_gl.js and cadrs_gl_bg.wasm.gz: the app drawn with WebGL2, and
#   bundle.tar.gz: the built-in libraries and the sample documents, which the page unpacks into
#   its files (/libraries, /fixtures) before the app starts
# Serve that folder over HTTP (`python3 -m http.server -d target/web`); https://cadrs.net is
# main's head, built by .github/workflows/pages.yml on every push to main.
#
# The app is built twice because Bevy picks a page's graphics as it is built, not as it starts:
# the page (`index.html`) loads the WebGPU one where the browser has WebGPU and the WebGL2 one
# where it doesn't (Chrome on Linux, older Safari and Firefox), and offers the downloads where
# there is neither. `CADRS_WEB_ONLY=webgpu|webgl2|phone` builds one of them alone.
#
# Phones get the phone page (`phone.html`, `cadrs_mobile`) instead, unless asked for `?full`.
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
names=(--remove-name-section)
[[ "${CADRS_WEB_DEBUG:-0}" != 0 ]] && names=()
# build <cargo features> <the module's name>: both end as target/…/cadrs.wasm, so each is bound
# and compressed (the page unpacks it as it downloads, `index.html`) before the next is built.
build() {
    cargo build -r --locked --target wasm32-unknown-unknown -p cadrs --no-default-features -F "$1"
    wasm-bindgen --target web --no-typescript "${names[@]}" --remove-producers-section \
        --out-name "$2" --out-dir "$out" target/wasm32-unknown-unknown/release/cadrs.wasm
    gzip -9 -f "$out/$2_bg.wasm"
}
only="${CADRS_WEB_ONLY:-}"
# `gpu` is the kernel's compute shaders, which WebGL2 has none of.
[[ -z "$only" || "$only" == webgl2 ]] && build "" cadrs_gl
[[ -z "$only" || "$only" == webgpu ]] && build gpu,webgpu cadrs

# The samples are `cadrs_core::documents_page::SAMPLES`' keys.
samples="$(sed -n 's/^ *key: "\(.*\)",$/fixtures\/\1.cadrs/p' crates/cadrs_core/src/documents_page.rs)"
# shellcheck disable=SC2086
tar --format=ustar --owner=0 --group=0 --mtime=@0 --sort=name -czf "$out/bundle.tar.gz" libraries $samples

# The phone page (`cadrs_mobile`): no Bevy, a few MB. Also the site's 404 page, so the links of
# contact cards (https://cadrs.net/c/<code>) open it.
if [[ -z "$only" || "$only" == phone ]]; then
    cargo build -r --locked --target wasm32-unknown-unknown -p cadrs_mobile
    wasm-bindgen --target web --no-typescript "${names[@]}" --remove-producers-section \
        --out-name cadrs_mobile --out-dir "$out" target/wasm32-unknown-unknown/release/cadrs_mobile.wasm
    cp tools/web/mobile.html "$out/phone.html"
    cp tools/web/mobile.html "$out/404.html"
fi

cp tools/web/index.html tools/web/files.js "$out/"
cp assets/icons/cadrs.png "$out/"
du -h "$out"/*.wasm.gz "$out"/cadrs_mobile_bg.wasm 2>/dev/null || true
echo "Built $out. Try it: python3 -m http.server -d $out 8080"
