"""Check Onshape's STEP of a case set against what Onshape said of the same cases.

    python -I scripts/step_check.py <set.json>        (cadrs's cadquery, crates/cadrs_py/python)

For every batch `cq2fs step` exported (<dir>/step/<set>-<k>.{step,json}), three answers for
each case, which should be one:
- eval:  evalFeatureScript's volume (parasolid.json), the case built in a scratch context;
- part:  Onshape's mass properties of the parts the generator feature built and named
         c<i>.<n> in the Part Studio (the batch's json);
- step:  the STEP file read back (by cadrs's STEP reader), its solids given to cases by the
         slot each lies in (case k of a batch moved k * slot_mm along X).
eval = step (within 1e-7) says the file holds what evalFeatureScript built, read by another
kernel's reader; part (the REST mass properties, computed less accurately: 8.6e-7 off on
CADEvolve 24 where eval and step agree to 2e-8) is held to 1e-6. Writes
<dir>/step/<set>-check.json.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../cadrs_py/python"))
import cadquery as cq  # noqa: E402  (cadrs's)

FLAG = 1e-7
PART = 1e-6


def main():
    set_path = sys.argv[1]
    s = json.load(open(set_path))
    base = os.path.dirname(os.path.abspath(set_path))
    d = os.path.join(base, s["dir"])
    name = os.path.splitext(os.path.basename(set_path))[0]
    names = {c["i"]: c["name"] for c in s["cases"]}
    rows = []
    k = 0
    while os.path.exists(os.path.join(d, "step", f"{name}-{k}.json")):
        meta = json.load(open(os.path.join(d, "step", f"{name}-{k}.json")))
        k_file = os.path.join(d, "step", f"{name}-{k}.step")
        if "error" in meta or not os.path.exists(k_file):
            print(f"batch {k}: {meta.get('error', 'no STEP')}")
            k += 1
            continue
        slot = meta["slot_mm"]
        part_vol = {}
        ids = {p["partId"]: p["name"] for p in meta["parts"]}
        for pid, m in meta["mass"].items():
            nm = ids.get(pid, "")
            if not nm.startswith("c"):
                continue
            i = int(nm[1:].split(".")[0])
            part_vol[i] = part_vol.get(i, 0.0) + m["volume"][0] * 1e9  # m^3 -> mm^3
        shape = cq.importers.importStep(k_file)
        step_vol = {}
        sols = [x for o in shape.vals() for x in o.Solids()]
        for so in sols:
            bb = so.BoundingBox()
            cx = (bb.xmin + bb.xmax) / 2
            slot_k = round(cx / slot)
            if 0 <= slot_k < len(meta["cases"]):
                i = meta["cases"][slot_k]
                step_vol[i] = step_vol.get(i, 0.0) + so.Volume()
        for i in meta["cases"]:
            p = json.load(open(os.path.join(d, f"{i}_{names[i]}", "parasolid.json")))
            ev = p.get("vol")
            pv = part_vol.get(i)
            sv = step_vol.get(i)
            rel = lambda a, b: abs(a - b) / max(abs(a), abs(b), 1e-12) if a is not None and b is not None else None
            r = {"i": i, "name": names[i], "batch": k, "eval": ev, "part": pv, "step": sv,
                 "eval_vs_step": rel(ev, sv), "eval_vs_part": rel(ev, pv)}
            r["ok"] = (r["eval_vs_step"] is not None and r["eval_vs_step"] <= FLAG
                       and r["eval_vs_part"] is not None and r["eval_vs_part"] <= PART)
            rows.append(r)
            mark = "" if r["ok"] else "  <-"
            f = lambda x: "-" if x is None else f"{x:.3e}"
            print(f"{i:6} {names[i][:40]:40} eval/step {f(r['eval_vs_step'])}  eval/part {f(r['eval_vs_part'])}{mark}")
        k += 1
    ok = sum(r["ok"] for r in rows)
    print(f"{ok} of {len(rows)} cases: the STEP holds what evalFeatureScript built (within {FLAG}), the Part Studio's parts too (within {PART})")
    json.dump({"rows": rows, "ok": ok, "of": len(rows)}, open(os.path.join(d, "step", f"{name}-check.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
