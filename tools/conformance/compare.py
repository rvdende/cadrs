#!/usr/bin/env python3
"""Compare brep-conformance runs: outcomes per test, and each operation's time against a
reference kernel's run (springrep's), both in the calibration workload's units so machines and
loads compare.

    compare.py RUN.json [--ref REF.json] [--base BASE.json] [--failing] [--ops]

--base lists the cases whose outcome changed since BASE (a fix's gains and losses);
--failing lists every case not passing, by test, with its first failed check;
--ops lists each operation's median and total against REF's, slowest ratio first.
"""

import json
import sys
from collections import Counter, defaultdict

OUTCOMES = ["pass", "unsupported", "refused", "timeout", "fail", "crash"]


def load(path):
    with open(path) as f:
        return json.load(f)


def totals(run):
    return Counter(c["outcome"] for c in run["cases"])


def line(name, t):
    n = sum(t.values())
    return f"{name:<14} {n:>5} " + " ".join(f"{o} {t.get(o, 0):>4}" for o in OUTCOMES)


def first_failure(c):
    for ch in c.get("checks", []):
        if not ch.get("ok", True):
            return f"{ch.get('clause')}: {ch.get('detail', '')}"[:160]
    for o in c.get("ops", []):
        if o.get("error") or o.get("err"):
            return str(o.get("error") or o.get("err"))[:160]
    return c.get("reason") or c.get("error") or ""


def ops(run):
    unit = run["calibration_ms"]
    by = defaultdict(list)
    for c in run["cases"]:
        for o in c.get("ops", []):
            if o.get("op") and o.get("ms") is not None:
                by[o["op"]].append(o["ms"] / unit)
    return by


def median(xs):
    xs = sorted(xs)
    return xs[len(xs) // 2] if xs else float("nan")


def main():
    args = sys.argv[1:]
    flag = lambda f: args[args.index(f) + 1] if f in args else None
    run = load(args[0])
    print(line(run["kernel"]["name"] if isinstance(run["kernel"], dict) else "run", totals(run)))
    ref = load(flag("--ref")) if flag("--ref") else None
    if ref:
        print(line("reference", totals(ref)))
    if flag("--base"):
        base = {c["id"]: c["outcome"] for c in load(flag("--base"))["cases"]}
        moved = Counter()
        for c in run["cases"]:
            was = base.get(c["id"])
            if was and was != c["outcome"]:
                moved[(was, c["outcome"])] += 1
                print(f"  {was:>11} -> {c['outcome']:<11} {c['test']}  {c['id']}")
        print("moved:", dict(moved))
    if "--failing" in args:
        by = defaultdict(list)
        for c in run["cases"]:
            if c["outcome"] != "pass":
                by[c["test"]].append(c)
        for test, cs in sorted(by.items(), key=lambda kv: -len(kv[1])):
            kinds = Counter(c["outcome"] for c in cs)
            print(f"\n{test}  {dict(kinds)}")
            for c in cs[:3]:
                print(f"    {c['id']} [{c['outcome']}] {first_failure(c)}")
    if "--ops" in args and ref:
        mine, theirs = ops(run), ops(ref)
        rows = []
        for op, t in mine.items():
            r = theirs.get(op)
            if not r:
                continue
            rows.append((sum(t) / max(sum(r), 1e-9), op, len(t), median(t), median(r), sum(t), sum(r)))
        rows.sort(reverse=True)
        print(f"\n{'operation':<24} {'calls':>6} {'median':>8} {'ref':>8} {'total':>9} {'ref':>9} {'ratio':>7}")
        for ratio, op, n, m, mr, s, sr in rows:
            print(f"{op:<24} {n:>6} {m:>8.2f} {mr:>8.2f} {s:>9.1f} {sr:>9.1f} {ratio:>7.2f}")
        tm = sum(sum(t) for op, t in mine.items() if op in theirs)
        tr = sum(sum(t) for op, t in theirs.items() if op in mine)
        print(f"\nshared operations' total: {tm:.0f} vs {tr:.0f} units ({tm / tr:.2f}x)")


if __name__ == "__main__":
    main()
