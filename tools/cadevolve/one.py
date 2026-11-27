#!/usr/bin/env python3
"""Runs one generator (by index or name) on cadrs's cadquery or OCCT's, with its traceback.

    one.py <index|name> [--ref] [--code] [--step out.step]"""
import json, os, subprocess, sys

DATA = os.path.expanduser("~/Work/cadevolve/CADEvolve-G/parametric_generators.json")
CADRS_PY = os.path.expanduser("~/Work/cadevolve/.venv-cadrs/bin/python")
REF_PY = os.path.expanduser("~/Work/cadevolve/.venv/bin/python")
PATH = os.environ.get("CADRS_PKG") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../crates/cadrs_py/python")

RUN = r'''
import json, sys, time, traceback, random, math
import cadquery as cq
g = json.load(open(sys.argv[1]))[int(sys.argv[2])]
random.seed(0)
ns = {"__name__": "gen", "cq": cq, "cadquery": cq, "math": math}
exec(compile(g["code"], g["name"], "exec"), ns)
t = time.perf_counter()
r = ns[g["name"]]()
dt = time.perf_counter() - t
objs = [o for o in r.vals() if isinstance(o, cq.Shape)] if isinstance(r, cq.Workplane) else [r]
s = objs[0] if len(objs) == 1 else cq.Compound.makeCompound(objs)
bb = s.BoundingBox()
print(f"{g['name']}: {dt:.3f} s  vol {s.Volume():.6f}  area {s.Area():.6f}  solids {len(s.Solids())}  faces {len(s.Faces())}  valid {s.isValid()}")
print(f"  box ({bb.xmin:.4f}, {bb.ymin:.4f}, {bb.zmin:.4f}) .. ({bb.xmax:.4f}, {bb.ymax:.4f}, {bb.zmax:.4f})")
if len(sys.argv) > 3:
    s.exportStep(sys.argv[3])
'''


def main():
    args = sys.argv[1:]
    gens = json.load(open(DATA))
    key = args[0]
    i = int(key) if key.isdigit() else next(j for j, g in enumerate(gens) if g["name"] == key)
    if "--code" in args:
        print(gens[i]["code"])
        return
    ref = "--ref" in args
    env = dict(os.environ)
    if not ref:
        env["PYTHONPATH"] = PATH
    extra = []
    if "--step" in args:
        extra = [args[args.index("--step") + 1]]
    subprocess.run([REF_PY if ref else CADRS_PY, "-c", RUN, DATA, str(i)] + extra, env=env)


main()
