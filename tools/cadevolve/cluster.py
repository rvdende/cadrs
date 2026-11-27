#!/usr/bin/env python3
"""Groups a cadrs run's failures (where the others succeed) by error and the CadQuery call on
the failing line, most frequent first.

    cluster.py occt.jsonl kentron.jsonl cadrs.jsonl [--examples 3]"""
import collections, json, re, sys
from compare import load
from compare3 import judge

DATA = "/home/rouan/Work/cadevolve/CADEvolve-G/parametric_generators.json"


def call_of(line):
    calls = re.findall(r"\.(\w+)\(", line)
    return calls[-1] if calls else "?"


def main():
    o, k, c = (load(p) for p in sys.argv[1:4])
    ex = int(sys.argv[sys.argv.index("--examples") + 1]) if "--examples" in sys.argv else 3
    gens = json.load(open(DATA))
    groups = collections.defaultdict(list)
    for i in sorted(set(o) & set(k) & set(c)):
        j = judge(o[i], k[i], c[i])
        if j not in (("bug",) if "--strong" in sys.argv else ("bug", "we-fail", "disputed")):
            continue
        r = c[i]
        if r["status"] == "ok":
            key = ("mismatch", "")
        else:
            err = re.sub(r"-?\d+(\.\d+)?(e-?\d+)?", "N", r.get("err") or r["status"])
            err = err.replace("RuntimeError: StdFail_NotDone: operation failed: ", "")[:70]
            w = r.get("where", "")
            ln = int(w.split(":")[-1]) if ":" in w else 0
            line = gens[i]["code"].split("\n")[ln - 1] if ln else ""
            key = (err, call_of(line))
        groups[key].append(i)
    for (err, call), v in sorted(groups.items(), key=lambda x: -len(x[1])):
        print(f"{len(v):5d}  {call:14s} {err}   e.g. {v[:ex]}")


main()
