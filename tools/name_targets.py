#!/usr/bin/env python3
"""Replaces the pixel positions in scenarios (`MoveTo(x, y)`, `ClickAt(x, y)`, `at(x, y)`) with
the named targets a recording run found for them (`crates/cadrs_harness/src/record.rs`):
`MoveTo` becomes `Hover(..)`, `ClickAt` `Click(..)`, `at(..)` the target itself.

Record first, then rewrite (each run's `<name>/targets.tsv` lists its positions in run order):
    CADRS_RECORD_TARGETS=1 cargo test -r -p cadrs -F app-tests --test scenarios -- --ignored <test>
    CADRS_RECORD_TARGETS=1 cargo run -r -- --headless --no-gpu --scenario <name>
    tools/name_targets.py scenarios target/tmp/scenarios target/scenarios

A scenario whose run stopped early keeps the positions it didn't reach. Rewrite each scenario
once from its original text: the recording lists the positions of that text.
"""
import os
import re
import sys

PIX = re.compile(r"\b(MoveTo|ClickAt|at)\(\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\)")


def code_mask(text):
    """True for characters outside comments and strings."""
    mask = [True] * len(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                mask[k] = False
            i = j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                mask[k] = False
            i = j
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            for k in range(i, min(j + 1, n)):
                mask[k] = False
            i = j + 1
        else:
            i += 1
    return mask


def occurrences(text):
    mask = code_mask(text)
    return [m for m in PIX.finditer(text) if mask[m.start()]]


def main():
    sdir, roots = sys.argv[1], sys.argv[2:]
    stats = {"done": 0, "partial": 0, "none": 0, "kept": 0, "mismatch": 0, "unrun": 0}
    for f in sorted(os.listdir(sdir)):
        if not f.endswith(".ron"):
            continue
        name = f[:-4]
        path = os.path.join(sdir, f)
        text = open(path).read()
        occ = occurrences(text)
        if not occ:
            stats["none"] += 1
            continue
        rec = None
        for r in roots:
            p = os.path.join(r, name, "targets.tsv")
            if os.path.exists(p):
                rec = [l.rstrip("\n").split("\t") for l in open(p) if l.strip()]
                break
        if rec is None:
            print(f"{name}: not recorded ({len(occ)} pixel targets)")
            stats["unrun"] += 1
            continue
        out, last, kept = [], 0, 0
        for i, m in enumerate(occ):
            if i >= len(rec):
                break
            x, y, target = float(rec[i][0]), float(rec[i][1]), rec[i][2]
            if (float(m.group(2)), float(m.group(3))) != (x, y):
                print(f"{name}: occurrence {i} is {m.group(0)}, recorded {x},{y}: stopping")
                stats["mismatch"] += 1
                break
            if target.startswith("at("):
                kept += 1
                print(f"{name}: kept {m.group(0)} ({rec[i][3]})")
                continue
            kind = m.group(1)
            new = {"MoveTo": f"Hover({target})", "ClickAt": f"Click({target})", "at": target}[kind]
            out.append(text[last : m.start()])
            out.append(new)
            last = m.end()
        else:
            i = len(occ)
        out.append(text[last:])
        new_text = "".join(out)
        if len(rec) != len(occ) or i < len(occ):
            print(f"{name}: {len(occ)} pixel targets, {len(rec)} recorded")
            stats["partial"] += 1
        else:
            stats["done"] += 1
        stats["kept"] += kept
        if new_text != text:
            open(path, "w").write(new_text)
    print(stats)


main()
