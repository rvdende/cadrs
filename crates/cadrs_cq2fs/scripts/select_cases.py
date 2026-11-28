"""Choose a spectrum of hard, translatable CADEvolve cases.

    python3 -I scripts/select_cases.py <tapes.jsonl> <translatable.json> <cadrs results.jsonl> [--n 100] [--exclude i,j]

Prints the chosen indices, one line per case with why it was chosen. Deterministic for the same
inputs. Groups, filled in order, each case taken once:

- cadrs fails (error, crash, timeout) where the tape translates;
- cadrs swallowed an exception inside the generator's own try (a failure hidden by its except);
- one quota per feature family, most complex first: fillets, chamfers, shells, revolves,
  spheres and tori in booleans, holes (plain, counterbored, countersunk), workplanes on faces,
  intersections, polar patterns and rotated workplanes, arcs and ellipses, mirrors and splits.

Complexity is the number of solid-changing calls, with booleans between curved solids counted
double. Cases a translation can't pin down are left out: first()/last()/item() and val() of
several objects (Parasolid's order isn't OCCT's), CenterOfBoundBox workplanes, chamfers with two
lengths.
"""
import json
import sys
from collections import Counter

BOOL = {"union", "cut", "intersect", "cutBlind", "cutThruAll", "hole", "cboreHole", "cskHole", "shape.fuse", "shape.cut",
        "shape.intersect", "split"}
SOLID = BOOL | {"extrude", "box", "sphere", "cylinder", "revolve", "fillet", "chamfer", "shell", "combine", "mirror",
                "Solid.makeBox", "Solid.makeCylinder", "Solid.makeCone", "Solid.makeSphere"}
CURVED = {"sphere", "revolve", "circle", "cylinder", "Solid.makeCylinder", "Solid.makeCone", "Solid.makeSphere", "ellipse",
          "threePointArc", "hole", "cboreHole", "cskHole"}


def features(rec):
    ops = [e["op"] for e in rec["entries"]]
    c = Counter(ops)
    f = set()
    if c["fillet"]:
        f.add("fillet")
    if c["chamfer"]:
        f.add("chamfer")
    if c["shell"]:
        f.add("shell")
    if c["revolve"]:
        f.add("revolve")
    if (c["sphere"] or c["Solid.makeSphere"] or c["revolve"]) and sum(c[b] for b in BOOL):
        f.add("curved-boolean")
    if c["hole"] or c["cboreHole"] or c["cskHole"]:
        f.add("holes")
    for n, e in enumerate(rec["entries"]):
        if e["op"] == "workplane" and n > 0 and any(p["op"] == "faces" for p in rec["entries"][max(0, n - 2):n]):
            f.add("face-workplane")
        if e["op"] == "transformed" and any(abs(x) > 1e-9 for x in (e["kwargs"].get("rotate") or (e["args"][0] if e["args"] else [0, 0, 0]))):
            f.add("rotated-workplane")
    if c["intersect"] or c["shape.intersect"]:
        f.add("intersect")
    if c["polarArray"] or c["rotate"] >= 3:
        f.add("pattern")
    if c["threePointArc"] or c["ellipse"]:
        f.add("arcs")
    if c["mirror"] or c["split"]:
        f.add("mirror-split")
    complexity = sum(c[o] for o in SOLID) + sum(c[o] for o in BOOL if any(c[x] for x in CURVED))
    return f, complexity, c


def ambiguous(rec):
    for e in rec["entries"]:
        if e["op"] in ("first", "last", "item"):
            return "first/last/item"
        if e["op"] == "workplane" and "CenterOfBoundBox" in json.dumps(e):
            return "CenterOfBoundBox"
        if e["op"] == "chamfer" and (len(e["args"]) > 1 or "length2" in e["kwargs"]):
            return "two-length chamfer"
    return None


def main():
    tapes = {}
    for line in open(sys.argv[1]):
        r = json.loads(line)
        tapes[r["i"]] = r
    ok = set(json.load(open(sys.argv[2])))
    cadrs = {}
    for line in open(sys.argv[3]):
        r = json.loads(line)
        cadrs[r["i"]] = r
    n = int(sys.argv[sys.argv.index("--n") + 1]) if "--n" in sys.argv else 100
    exclude = set(int(x) for x in sys.argv[sys.argv.index("--exclude") + 1].split(",")) if "--exclude" in sys.argv else set()

    pool = []
    for i in sorted(ok):
        if i in exclude or i not in cadrs:
            continue
        rec = tapes[i]
        if ambiguous(rec):
            continue
        f, cx, c = features(rec)
        pool.append((i, f, cx, cadrs[i]))

    chosen = {}

    def take(i, why):
        if i not in chosen and len(chosen) < n:
            chosen[i] = why

    # spread a group over the feature families, most complex first
    def spread(cands, quota, why):
        by_feat = {}
        for i, f, cx, r in cands:
            for x in (f or {"plain"}):
                by_feat.setdefault(x, []).append((cx, i))
        for lst in by_feat.values():
            lst.sort(key=lambda t: (-t[0], t[1]))
        taken = 0
        while taken < quota and any(by_feat.values()):
            for x in sorted(by_feat):
                while by_feat[x] and by_feat[x][0][1] in chosen:
                    by_feat[x].pop(0)
                if by_feat[x] and taken < quota:
                    cx, i = by_feat[x].pop(0)
                    take(i, f"{why} [{x}, complexity {cx}]")
                    taken += 1

    fails = [p for p in pool if p[3]["status"] != "ok"]
    swallowed = [p for p in pool if p[3]["status"] == "ok" and p[3].get("swallowed")]
    spread(fails, 35, "cadrs " + "fails")
    spread(swallowed, 15, "cadrs swallowed an exception")
    rest = [p for p in pool if p[3]["status"] == "ok" and not p[3].get("swallowed")]
    spread(rest, n - len(chosen), "spectrum")
    for i, why in sorted(chosen.items()):
        r = cadrs[i]
        print(json.dumps({"i": i, "name": tapes[i]["name"], "why": why, "cadrs": r["status"], "err": (r.get("err") or "")[:100]}))


if __name__ == "__main__":
    main()
