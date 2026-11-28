"""Check Onshape's STEP of a case set against what Onshape said of the same cases.

    python -I scripts/step_check.py <set.json>        (cadrs's cadquery, crates/cadrs_py/python)

For every batch `cq2fs step` exported (<dir>/step/<set>-<k>.{step,json}), three answers for
each case, which should be one:
- eval:  evalFeatureScript's volume (parasolid.json), the case built in a scratch context;
- part:  Onshape's mass properties of the parts the generator feature built and named
         c<i>.<n> in the Part Studio (the batch's json);
- step:  the STEP file imported back into Onshape and measured by Parasolid
         (`cq2fs step --verify`: <set>-<k>.reimport.json), parts by their names;
- cadrs: the STEP file read by cadrs's STEP reader, its solids given to cases by the slot each
         lies in (case k of a batch moved k * slot_mm along X): a check of that reader.
eval = step (within 1e-7) says the file holds what evalFeatureScript built, read by Parasolid
itself, not by the kernel under test; step = cadrs (within 1e-7) says cadrs's reader reads it;
part (the REST mass properties) is reported, not held to anything: it is computed less
accurately (up to 4e-4 off on hard100's lattices where eval and step agree to 1e-8). Writes
<dir>/step/<set>-check.json.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "../../cadrs_py/python"))
import cadquery as cq  # noqa: E402  (cadrs's)

FLAG = 1e-7


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
        reimport = {}
        rp = os.path.join(d, "step", f"{name}-{k}.reimport.json")
        if os.path.exists(rp):
            for part in json.load(open(rp)).get("parts") or []:
                nm = part.get("name") or ""
                if nm.startswith("c") and "." in nm:
                    i = int(nm[1:].split(".")[0])
                    reimport[i] = reimport.get(i, 0.0) + part["vol"]
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
            sv = reimport.get(i)
            cv = step_vol.get(i)
            rel = lambda a, b: abs(a - b) / max(abs(a), abs(b), 1e-12) if a is not None and b is not None else None
            r = {"i": i, "name": names[i], "batch": k, "eval": ev, "part": pv, "step": sv, "cadrs_read": cv,
                 "eval_vs_step": rel(ev, sv), "eval_vs_part": rel(ev, pv), "step_vs_cadrs_read": rel(sv, cv)}
            r["cadrs_reads_it"] = r["step_vs_cadrs_read"] is not None and r["step_vs_cadrs_read"] <= FLAG
            r["ok"] = r["eval_vs_step"] is not None and r["eval_vs_step"] <= FLAG
            rows.append(r)
            mark = ("" if r["ok"] else "  <- step") + ("" if r["cadrs_reads_it"] else "  <- cadrs read")
            f = lambda x: "-" if x is None else f"{x:.3e}"
            print(f"{i:6} {names[i][:40]:40} eval/step {f(r['eval_vs_step'])}  eval/part {f(r['eval_vs_part'])}  step/cadrs-read {f(r['step_vs_cadrs_read'])}{mark}")
        k += 1
    ok = sum(r["ok"] for r in rows)
    reads = sum(r["cadrs_reads_it"] for r in rows)
    print(f"cadrs's STEP reader reads {reads} of {len(rows)} as Parasolid does (within {FLAG})")
    print(f"{ok} of {len(rows)} cases: the STEP holds what evalFeatureScript built (within {FLAG})")
    json.dump({"rows": rows, "ok": ok, "of": len(rows)}, open(os.path.join(d, "step", f"{name}-check.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
