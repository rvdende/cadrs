#!/usr/bin/env python3
"""Checks each boolean a cadrs CadQuery run logged (`CADRS_CQ_ORACLE=<dir>`) against
independent booleans of its meshed operands:

- Manifold (robust mesh booleans) for every boolean: our volume must lie within the meshes'
  error, k * (operands' area) * (mesh deflection), of Manifold's;
- CGAL's Nef polyhedra with exact constructions (GMP rationals) where every operand face is
  flat: the meshes are then the exact solids and the answer is exact, so ours must agree to
  rounding (1e-9 relative).

    oracle.py <dir> [--verbose]      (run with the venv that has manifold3d and trimesh)"""
import json, os, subprocess, sys
import numpy as np
import trimesh
import manifold3d as mf

HERE = os.path.dirname(os.path.abspath(__file__))
CGAL = os.path.join(HERE, "cgal", "cgal_bool")


def load(path):
    t = trimesh.load(path, process=True, force="mesh")
    t.merge_vertices(digits_vertex=10)
    mesh = mf.Mesh(vert_properties=np.asarray(t.vertices, dtype=np.float32), tri_verts=np.asarray(t.faces, dtype=np.uint32))
    m = mf.Manifold(mesh)
    return m


def manifold_volume(op, files):
    ms = [load(f) for f in files]
    a, tools = ms[0], ms[1:]
    t = tools[0]
    for x in tools[1:]:
        t = t + x
    r = a + t if op == "Union" else a - t if op == "Subtract" else a ^ t
    return r.volume(), [m.status() for m in ms]


def main():
    d = sys.argv[1]
    verbose = "--verbose" in sys.argv
    recs = [json.loads(l) for l in open(os.path.join(d, "booleans.jsonl"))]
    bad = 0
    for i, r in enumerate(recs):
        try:
            mv, st = manifold_volume(r["op"], r["files"])
        except Exception as e:
            print(f"{i}: {r['op']} manifold failed: {e}")
            continue
        bound = 4.0 * r["area"] * r["deflection"] + 1e-6
        # Sharper where the operands' exact volumes are logged: the meshes' own volume errors
        # (each mesh against its solid), the most the result's can be off by their union, with
        # a margin of three.
        if r.get("in_vols") and all(v is not None for v in r["in_vols"]):
            mesh_err = sum(abs(trimesh.load(f, process=True, force="mesh").volume - v)
                           for f, v in zip(r["files"], r["in_vols"]))
            bound = min(bound, 3.0 * mesh_err + 1e-6 * max(abs(r["vol"] or 0.0), 1.0))
        ex = None
        if (r["planar"] or any(x != mf.Error.NoError for x in st)) and os.path.exists(CGAL):
            p = subprocess.run([CGAL, r["op"]] + r["files"], capture_output=True, text=True, timeout=600)
            if p.returncode == 0:
                ex = float(p.stdout.strip())
                if not r["planar"]:
                    # Curved: CGAL's mesh boolean stands in for Manifold's (its meshes rejected).
                    mv, ex = ex, None
        ours = r["vol"]
        verdict = "ok"
        if r["status"] != "ok":
            verdict = "FAILED (oracle volume %.6f)" % (ex if ex is not None else mv)
        elif ex is not None and abs(ours - ex) > 1e-9 * max(abs(ex), 1.0):
            verdict = f"WRONG: exact {ex:.9f}, ours {ours:.9f}"
        elif abs(ours - mv) > bound:
            verdict = f"WRONG: manifold {mv:.6f} ± {bound:.2e}, ours {ours:.6f}"
        if verdict != "ok":
            bad += 1
        if verdict != "ok" or verbose:
            print(f"{i}: {r['op']:9s} planar={r['planar']!s:5s} {verdict}  {r['err'][:90]}  files {r['files'][0]}")
    print(f"{len(recs)} booleans, {bad} failed or wrong")


main()
