#!/usr/bin/env python3
"""Judges a cadrs run by consensus with two other kernels (OCCT, kentron): which results are
confirmed, which are ours alone to fix, and which are disputed.

    compare3.py occt.jsonl kentron.jsonl cadrs.jsonl [--show CATEGORY] [--limit N]

Categories:
  agree         ours matches a valid result of another kernel
  only-valid    the others fail or give invalid shapes; ours is valid (we are the only answer)
  bug           the two others agree (valid) and ours differs or fails: ours to fix first
  we-fail       ours fails; one other kernel gives a valid result
  disputed      ours differs from each other valid result, and they differ from each other
  all-fail      no kernel gives a valid result
"""
import argparse, collections, json, re
from compare import load, close


def same(a, b, rel=1e-3, area=None):
    """The same solid by volume and box (and by area too with `--area`: OCCT keeps faces
    inside a solid, two caps where a loft meets itself, which add area and no volume)."""
    if a.get("status") != "ok" or b.get("status") != "ok":
        return False
    use_area = AREA if area is None else area
    diag = sum((a["bbox"][j + 3] - a["bbox"][j]) ** 2 for j in range(3)) ** 0.5
    return (close(a["vol"], b["vol"], rel) and (not use_area or close(a["area"], b["area"], rel))
            and all(abs(x - y) <= 2e-3 * diag + 1e-6 for x, y in zip(a["bbox"], b["bbox"])))


AREA = "--area" in __import__("sys").argv


def valid(r):
    return r.get("status") == "ok" and r.get("valid", True) and not (r.get("vol", 0) <= 1e-9 and r.get("solids", 0) > 0)


def judge(o, k, c):
    others = [r for r in (o, k) if r is not None and valid(r)]
    if c.get("status") != "ok":
        if not others:
            return "all-fail"
        if len(others) == 2 and same(others[0], others[1]):
            return "bug"
        return "we-fail"
    if any(same(c, r) for r in others):
        return "agree"
    if not others:
        return "only-valid" if valid(c) else "all-fail"
    if len(others) == 2 and same(others[0], others[1]):
        return "bug"
    return "disputed"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("occt"); ap.add_argument("kentron"); ap.add_argument("cadrs")
    ap.add_argument("--show", default=""); ap.add_argument("--limit", type=int, default=50)
    ap.add_argument("--errors", action="store_true")
    a = ap.parse_args()
    o, k, c = load(a.occt), load(a.kentron), load(a.cadrs)
    keys = sorted(set(c) & set(o) & set(k))
    cats = collections.defaultdict(list)
    for i in keys:
        cats[judge(o[i], k[i], c[i])].append(i)
    n = len(keys)
    print(f"{n} generators run by all three")
    for cat in ["agree", "only-valid", "bug", "we-fail", "disputed", "all-fail"]:
        print(f"  {cat:11s} {len(cats[cat]):5d}  {100 * len(cats[cat]) / max(n, 1):5.1f}%")
    if a.errors:
        cnt = collections.Counter()
        for i in cats["bug"] + cats["we-fail"]:
            e = c[i].get("err") or ("mismatch" if c[i]["status"] == "ok" else c[i]["status"])
            cnt[re.sub(r"-?\d+(\.\d+)?(e-?\d+)?", "N", e)[:110]] += 1
        for e, v in cnt.most_common(a.limit):
            print(f"{v:5d} {e}")
    if a.show:
        for i in cats[a.show][: a.limit]:
            print(f"{i:5d} {c[i]['name'][:46]:46s} occt {o[i].get('vol', o[i]['status'])!s:.12} kentron {k[i].get('vol', k[i]['status'])!s:.12} ours {c[i].get('vol', c[i].get('err', c[i]['status']))!s:.70}")


if __name__ == "__main__":
    main()
