#!/usr/bin/env python3
"""Compares a cadrs run with the reference (OCCT) run: per generator, whether the result
matches (volume, area and box within tolerance), mismatches, or which side failed.

    compare.py ref.jsonl ours.jsonl [--show CATEGORY] [--errors] [--rel 1e-3]"""
import argparse, collections, json, re


def load(p):
    out = {}
    for l in open(p):
        try:
            r = json.loads(l)
        except json.JSONDecodeError:
            continue
        out[r["i"]] = r
    return out


def close(a, b, rel, abs_=1e-6):
    return abs(a - b) <= max(abs_, rel * max(abs(a), abs(b)))


def verdict(ref, ours, rel):
    rok, ook = ref.get("status") == "ok", ours.get("status") == "ok"
    if not rok and not ook:
        return "both-fail"
    if not rok:
        return "ref-fails"
    # The reference's result itself broken: an invalid or empty shape.
    ref_bad = (not ref.get("valid", True)) or (ref.get("vol", 0) <= 1e-9 and ref.get("solids", 0) > 0)
    if not ook:
        return "ref-invalid" if ref_bad else "we-fail"
    ok_vol = close(ref["vol"], ours["vol"], rel)
    ok_area = close(ref["area"], ours["area"], rel)
    # (OCCT's "optimal" box comes from its mesh: inside the true one by up to its deflection.)
    diag = sum((ref["bbox"][j + 3] - ref["bbox"][j]) ** 2 for j in range(3)) ** 0.5
    ok_box = all(abs(a - b) <= 2e-3 * diag + 1e-6 for a, b in zip(ref["bbox"], ours["bbox"]))
    if ok_vol and ok_area and ok_box:
        return "match" if ours.get("valid", True) else "match-invalid"
    if ref_bad:
        return "ref-invalid"
    return "mismatch"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ref")
    ap.add_argument("ours")
    ap.add_argument("--show", default="")
    ap.add_argument("--errors", action="store_true")
    ap.add_argument("--rel", type=float, default=1e-3)
    ap.add_argument("--limit", type=int, default=40)
    a = ap.parse_args()
    ref, ours = load(a.ref), load(a.ours)
    keys = sorted(set(ref) & set(ours))
    cats = collections.defaultdict(list)
    for k in keys:
        cats[verdict(ref[k], ours[k], a.rel)].append(k)
    n = len(keys)
    print(f"{n} generators in both runs")
    for c in ["match", "match-invalid", "mismatch", "we-fail", "ref-fails", "ref-invalid", "both-fail"]:
        print(f"  {c:14s} {len(cats[c]):5d}  {100 * len(cats[c]) / max(n, 1):5.1f}%")
    if a.errors:
        c = collections.Counter()
        for k in cats["we-fail"]:
            e = ours[k].get("err") or ours[k]["status"]
            c[re.sub(r"-?\d+(\.\d+)?(e-?\d+)?", "N", e)[:120]] += 1
        print("\nour failures where the reference works:")
        for e, v in c.most_common(a.limit):
            print(f"{v:5d} {e}")
    if a.show:
        for k in cats[a.show][: a.limit]:
            r, o = ref[k], ours[k]
            print(f"{k:5d} {r['name'][:50]:50s} ref vol {r.get('vol', '-')!s:>22} ours {o.get('vol', o.get('err', o['status']))!s:.80}")


if __name__ == "__main__":
    main()
