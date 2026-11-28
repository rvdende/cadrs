#!/usr/bin/env python3
"""Runs generators with the boolean oracle log (dev package) and reports each one's first
failing boolean: its operation and the surface kinds of its operands.

    oracle_batch.py <out dir> <index> [<index> ...]"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
out = sys.argv[1]
for i in sys.argv[2:]:
    d = os.path.join(out, i)
    os.makedirs(d, exist_ok=True)
    env = dict(os.environ, CADRS_CQ_ORACLE=d, CADRS_PKG=os.path.expanduser("~/Work/cadevolve/pkg-dev"))
    try:
        subprocess.run([sys.executable, os.path.join(HERE, "one.py"), i], env=env, capture_output=True, timeout=120)
    except subprocess.TimeoutExpired:
        print(i, "timeout")
        continue
    log = os.path.join(d, "booleans.jsonl")
    if not os.path.exists(log):
        print(i, "no booleans")
        continue
    recs = [json.loads(l) for l in open(log)]
    bad = [(n, r) for n, r in enumerate(recs) if r["status"] != "ok"]
    if not bad:
        print(i, "all booleans ok")
        continue
    n, r = bad[0]
    stem = r["files"][0][:-6]
    print(f"{i} boolean {n}/{len(recs)} {r['op']} {r['kinds']} | {r['err'][:70]} | {stem}")
