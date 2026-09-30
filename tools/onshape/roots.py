#!/usr/bin/env python3
"""The first feature each Part Studio could not import, grouped by reason: later failures
often follow from it, so these are the ones to fix first.

    python3 tools/onshape/roots.py [import-report.txt]
"""
import collections
import os
import re
import sys

ROOT = os.path.expanduser(os.environ.get("CADRS_ONSHAPE_ROOT", "~/work/cadrs_onshape"))
text = open(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "import-report.txt")).read()
roots = collections.defaultdict(list)
for block in text.split("\n# "):
    lines = block.splitlines()
    doc = lines[0].lstrip("# ")
    studio = None
    for i, line in enumerate(lines):
        m = re.match(r"\s+\[partstudio\] (.*?):", line)
        if m:
            studio = m.group(1)
            continue
        m = re.match(r"\s+SKIPPED\s+(.*) \((\w+)\)$", line)
        if m and studio:
            reason = lines[i + 1].strip().lstrip("- ") if i + 1 < len(lines) else ""
            roots[(m.group(2), re.sub(r"\d+", "N", reason))].append(f"{doc} / {studio} / {m.group(1)}")
            studio = None
for (kind, reason), where in sorted(roots.items(), key=lambda kv: -len(kv[1])):
    print(f"{len(where):3}  {kind:14} {reason}")
    for w in where[: int(os.environ.get("ROOTS_SHOW", "3"))]:
        print(f"       {w}")
