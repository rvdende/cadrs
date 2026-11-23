#!/usr/bin/env bash
# Builds the cadrs Onshape Importer desktop app.
#
#   tools/onshape/build.sh          run the app checks, build the importer, then package the app
#                                   (AppImage + unpacked dir)
#   tools/onshape/build.sh --run    run the app checks, build the importer, then start the app
#                                   from source (no packaging)
#   tools/onshape/build.sh --test   only the app checks (node tests; also the importer's Rust
#                                   tests with CADRS_TEST_RUST=1)
#
# Needs: cargo, node >= 18 and npm. No python3 needed any more: the report summaries
# (summarize.py/roots.py) and the ticket generator are JS now; python3 only adds optional
# FeatureScript query decoding in the Compare tab.
# Output: tools/onshape/desktop/dist/
#
# The Electron/electron-builder downloads and the npm cache go under the business workspace,
# not into $HOME, so they can be prepared (or cleaned) alongside the repo:
#   ELECTRON_BUILDER_CACHE=$REPO/../.electron-builder-cache
#   npm_config_cache=$REPO/../.npm-cache
# Override with the matching environment variables if needed.
#
# Set CADRS_OFFLINE=1 to add --offline to the cargo build, SKIP_TESTS=1 to skip the node tests,
# and ELECTRON_DIST=<unpacked electron dir> to package without downloading Electron again.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
APP="$HERE/desktop"

MODE=package
case "${1:-}" in
  "") ;;
  --test) MODE=test ;;
  --run) MODE=run ;;
  -h|--help)
    sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 0 ;;
  *)
    echo "unknown option: $1 (use --run, --test or no option)" >&2
    exit 2 ;;
esac

command -v node >/dev/null || { echo "missing: node" >&2; exit 1; }

export ELECTRON_BUILDER_CACHE="${ELECTRON_BUILDER_CACHE:-$REPO/../.electron-builder-cache}"
export npm_config_cache="${npm_config_cache:-$REPO/../.npm-cache}"
mkdir -p "$ELECTRON_BUILDER_CACHE" "$npm_config_cache"

# Node 22's test runner takes glob patterns (a bare directory argument is treated as a module
# path, so pass the glob).
run_tests() {
  echo "==> running app tests"
  (cd "$APP" && node --test "test/**/*.test.js")
}

if [[ "$MODE" == "test" ]]; then
  run_tests
  if [[ "${CADRS_TEST_RUST:-0}" == "1" ]]; then
    echo "==> running the importer's Rust tests"
    rust_args=(test --release -p cadrs_onshape)
    if [[ "${CADRS_OFFLINE:-0}" == "1" ]]; then rust_args+=(--offline); fi
    (cd "$REPO" && cargo "${rust_args[@]}")
  fi
  exit 0
fi

for tool in cargo npm; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

if [[ "${SKIP_TESTS:-0}" != "1" ]]; then run_tests; fi

echo "==> building cadrs-onshape (release)"
cargo_args=(build --release -p cadrs_onshape)
if [[ "${CADRS_OFFLINE:-0}" == "1" ]]; then cargo_args+=(--offline); fi
(cd "$REPO" && cargo "${cargo_args[@]}")
mkdir -p "$APP/bin"
cp "$REPO/target/release/cadrs-onshape" "$APP/bin/"

# node_modules is installed for the app already; npm install is only the fallback (the npm cache
# may be read-only in sandboxes).
if [[ ! -d "$APP/node_modules/electron" ]]; then
  echo "==> installing app dependencies"
  (cd "$APP" && npm install --no-audit --no-fund)
fi

if [[ "$MODE" == "run" ]]; then
  echo "==> starting the app"
  cd "$APP" && exec node node_modules/electron/cli.js .
fi

echo "==> packaging (electron-builder: unpacked dir + AppImage)"
builder_args=(--linux dir AppImage --publish never)
# The electron npm package already carries the unpacked runtime: use it instead of downloading
# Electron again unless the caller asks for a different dist.
if [[ -z "${ELECTRON_DIST:-}" && -d "$APP/node_modules/electron/dist" ]]; then
  ELECTRON_DIST="$APP/node_modules/electron/dist"
fi
if [[ -n "${ELECTRON_DIST:-}" ]]; then builder_args+=(--config.electronDist="$ELECTRON_DIST"); fi
(cd "$APP" && node node_modules/electron-builder/out/cli/cli.js "${builder_args[@]}")
echo "done: $APP/dist"
