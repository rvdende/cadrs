#!/usr/bin/env python3
"""Summarizes an import report (`cadrs-onshape import` output): totals, and the most common
reasons features were skipped or only partly imported, to decide what to implement next.

    python3 tools/onshape/summarize.py [import-report.txt] [--top N]
"""
import collections
import os
import re
import sys

ROOT = os.path.expanduser(os.environ.get("CADRS_ONSHAPE_ROOT", "~/work/cadrs_onshape"))


def main():
    argv = sys.argv[1:]
    top = 30
    if "--top" in argv:
        i = argv.index("--top")
        top = int(argv[i + 1])
        del argv[i : i + 2]
    args = [a for a in argv if not a.startswith("--")]
    path = args[0] if args else os.path.join(ROOT, "import-report.txt")
    text = open(path).read()

    docs = len(re.findall(r"^# ", text, re.M))
    studios = re.findall(r"\[partstudio\] .*?: (\d+) full, (\d+) partial, (\d+) skipped(?:; parts (\d+)/(\d+) match)?", text)
    full = sum(int(s[0]) for s in studios)
    partial = sum(int(s[1]) for s in studios)
    skipped = sum(int(s[2]) for s in studios)
    matched = sum(int(s[3] or 0) for s in studios)
    parts = sum(int(s[4] or 0) for s in studios)
    clean = sum(1 for s in studios if s[2] == "0" and s[4] and s[3] == s[4])
    failed = len(re.findall(r"import failed or kept hanging", text))
    hung = len(re.findall(r"rebuild hung", text))
    print(f"documents {docs} ({failed} failed), part studios {len(studios)} ({clean} fully imported with every part matching)")
    total = full + partial + skipped
    if total:
        print(f"features {total}: {full} full ({100 * full / total:.0f} %), {partial} partial, {skipped} skipped; {hung} left out because the rebuild hung or crashed")
    if parts:
        print(f"parts matching Onshape's volume: {matched}/{parts} ({100 * matched / parts:.0f} %)")

    by_kind = collections.Counter()
    reasons = collections.Counter()
    current = None
    for line in text.splitlines():
        m = re.match(r"\s+(SKIPPED|partial)\s+.*\((\w+)\)$", line)
        if m:
            current = (m.group(1).lower(), m.group(2))
            if current[0] == "skipped":
                by_kind[current[1]] += 1
            continue
        m = re.match(r"\s+- (.*)", line)
        if m and current:
            r = re.sub(r"\d+(\.\d+)?", "N", m.group(1))
            r = re.sub(r'"[^"]*"', '"…"', r)
            reasons[(current[0], current[1], r)] += 1
        elif not line.startswith("                 "):
            current = None if not m else current
    print("\nskipped features by type:")
    for k, n in by_kind.most_common(top):
        print(f"  {n:5}  {k}")
    print("\nreasons:")
    for (k, ft, r), n in reasons.most_common(top):
        print(f"  {n:5}  {k:8} {ft:16} {r}")


if __name__ == "__main__":
    main()
