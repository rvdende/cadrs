#!/usr/bin/env python3
"""Removes the `Wait(n)` steps scenarios no longer need: every step now waits for the app to
settle (see `crates/cadrs_harness/src/scenario.rs`).

A Wait is kept where it times something the app's pending work doesn't cover:
- after the pointer moved or a button went down or up (Hover, MoveTo, MoveWorld, Press,
  PressWith, Release, ReleaseWith, Scroll): a tooltip's delay, a hover state, a frame caught
  mid-drag;
- before a DoubleClick (its two clicks must not join the click before), and inside a
  MeasureStart … MeasureEnd timing.

Usage: tools/strip_waits.py scenarios/*.ron   (prints how many Waits each file lost)
"""
import re
import sys

KEEP_AFTER = {"Hover", "MoveTo", "MoveWorld", "Press", "PressWith", "Release", "ReleaseWith", "Scroll", "MeasureStart"}
KEEP_BEFORE = {"DoubleClick", "MeasureEnd"}
STEP = re.compile(r"^\s+([A-Z][A-Za-z]*)\b")
WAIT = re.compile(r"^\s+Wait\(\d+\),?\s*$")


def strip(text: str) -> tuple[str, int]:
    lines = text.split("\n")
    kinds = [m.group(1) if (m := STEP.match(l)) else None for l in lines]
    out, removed, measuring = [], 0, False
    for i, line in enumerate(lines):
        kind = kinds[i]
        if kind == "MeasureStart":
            measuring = True
        elif kind == "MeasureEnd":
            measuring = False
        if WAIT.match(line):
            prev = next((k for k in reversed(kinds[:i]) if k), None)
            nxt = next((k for k in kinds[i + 1 :] if k), None)
            if not measuring and prev not in KEEP_AFTER and nxt not in KEEP_BEFORE:
                removed += 1
                continue
        out.append(line)
    return "\n".join(out), removed


def main() -> None:
    total = 0
    for path in sys.argv[1:]:
        text = open(path).read()
        new, n = strip(text)
        if n:
            open(path, "w").write(new)
            total += n
    print(f"removed {total} Waits from {len(sys.argv) - 1} files")


if __name__ == "__main__":
    main()
