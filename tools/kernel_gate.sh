#!/bin/sh
# The kernel's gate: every cadrs_kernel test binary run on a clean tree, each case classified
# (pass, known failure, STALE known failure, PANIC, REGRESSION, HANG, CRASH; slow over 60 s), with
# a result file the scoreboard (tools/kernel_scoreboard.py) reads. Exits 1 when anything is STALE,
# a PANIC (a known failure that panicked), a REGRESSION, a HANG or a CRASH, or when
# `cargo check -p cadrs_kernel` (no features: the library without the tests' `testkit`),
# `cargo check -p cadrs_core` or tools/kernel_clippy.sh fails. See tools/kernel_gate.py for the
# classification.
#
# usage: tools/kernel_gate.sh [--rev REV | --tree DIR] [--commit SHA] [--out DIR] [kernel_gate.py options]
#
#   --rev REV   test `git archive REV` (default HEAD), exported to <out>/tree
#   --tree DIR  test DIR as it is (e.g. the working tree): nothing exported or regenerated
#   --commit SHA  the commit DIR is, recorded in results.json (for --tree; default
#               $CADRS_GATE_COMMIT, else DIR's git HEAD, with "+dirty" for uncommitted changes)
#   --out DIR   results.json, summary.txt and stderr.log (default $CARGO_TARGET_DIR/kernel_gate,
#               else target/kernel_gate)
#   other options go to tools/kernel_gate.py: --only smlib_offset,smlib_fillets, --skip ...,
#   --hang 900 (seconds before a test is a hang), --binary-timeout 14400, --slow 60
#
# Environment: CADRS_OCCT_DATA (the unpacked opencascade-dataset-7.8.0, default
# ../occt-dataset/opencascade-dataset-7.8.0: STEP files some tests read), CADRS_SMLIB_ORACLE (default target/smlib_oracle/smlib_oracle when built), RUST_TEST_THREADS (3),
# CARGO_TARGET_DIR, CARGO_BUILD_JOBS.
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
rev=HEAD
tree=
out=
commit=${CADRS_GATE_COMMIT:-}
while [ $# -gt 0 ]; do
    case "$1" in
        --rev) rev=$2; shift 2 ;;
        --tree) tree=$(cd "$2" && pwd); shift 2 ;;
        --out) out=$2; shift 2 ;;
        --commit) commit=$2; shift 2 ;;
        *) break ;;
    esac
done
target=${CARGO_TARGET_DIR:-$repo/target}
case "$target" in /*) ;; *) target=$repo/$target ;; esac
export CARGO_TARGET_DIR=$target
out=${out:-$target/kernel_gate}
mkdir -p "$out"
out=$(cd "$out" && pwd)
export CADRS_OCCT_DATA=${CADRS_OCCT_DATA:-$repo/../occt-dataset/opencascade-dataset-7.8.0}
export RUST_TEST_THREADS=${RUST_TEST_THREADS:-3}
# The SMLib oracle (tools/smlib_oracle/README.md), when it's built: smlib_oracle's tests use it.
if [ -z "${CADRS_SMLIB_ORACLE:-}" ] && [ -x "$repo/target/smlib_oracle/smlib_oracle" ]; then
    export CADRS_SMLIB_ORACLE="$repo/target/smlib_oracle/smlib_oracle"
fi

if [ -z "$tree" ]; then
    commit=$(git -C "$repo" rev-parse "$rev")
    tree=$out/tree
    rm -rf "$tree"
    mkdir -p "$tree"
    git -C "$repo" archive "$commit" | tar -x -C "$tree"
    echo "kernel_gate: $commit exported to $tree" >&2
elif [ -z "$commit" ] && [ "$(git -C "$tree" rev-parse --show-toplevel 2>/dev/null)" = "$tree" ]; then
    # The tree's own checkout (not a repository it happens to sit in).
    commit=$(git -C "$tree" rev-parse HEAD)
    [ -z "$(git -C "$tree" status --porcelain --untracked-files=no)" ] || commit="$commit+dirty"
fi
exec python3 -I "$repo/tools/kernel_gate.py" --tree "$tree" --out "$out" --commit "$commit" "$@"
