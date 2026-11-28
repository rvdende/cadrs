"""One CadEvolve worker: reads generator tasks (JSON lines) on stdin, runs each with its defaults
and writes the result's measures (JSON lines) on stdout. Used by run.py with either real CadQuery
or cadrs's (`PYTHONPATH` picks which `cadquery` is imported)."""
import json, sys, time, traceback, random, resource, io, contextlib

def measure(cq, res):
    if isinstance(res, cq.Workplane):
        objs = [o for o in res.vals() if isinstance(o, cq.Shape)]
    elif isinstance(res, cq.Shape):
        objs = [res]
    elif isinstance(res, (list, tuple)):
        objs = [o.val() if isinstance(o, cq.Workplane) else o for o in res]
        objs = [o for o in objs if isinstance(o, cq.Shape)]
    else:
        return {"status": "no-shape", "err": type(res).__name__}
    if not objs:
        return {"status": "no-shape", "err": "empty"}
    shape = objs[0] if len(objs) == 1 else cq.Compound.makeCompound(objs)
    bb = shape.BoundingBox()
    return {
        "status": "ok",
        "vol": shape.Volume(),
        "area": shape.Area(),
        "bbox": [bb.xmin, bb.ymin, bb.zmin, bb.xmax, bb.ymax, bb.zmax],
        "solids": len(shape.Solids()),
        "faces": len(shape.Faces()),
        "valid": bool(shape.isValid()),
    }

def main():
    mem = int(sys.argv[1]) if len(sys.argv) > 1 else 4 << 30
    resource.setrlimit(resource.RLIMIT_AS, (mem, mem))
    # Results go out on a private copy of stdout: C++ code (OCCT) prints on fd 1 itself.
    import os
    proto = os.fdopen(os.dup(1), "w", buffering=1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 1)
    sys.stdout = open(os.devnull, "w")
    # Another kernel's CadQuery (`pykentron.shim` for kentron's) imported first.
    pre = os.environ.get("CADEVOLVE_PREIMPORT")
    if pre:
        __import__(pre)
    import cadquery as cq
    try:
        import numpy as np
    except ImportError:
        np = None
    # Workplane calls that raised but whose generator went on (its own try/except): a fillet
    # one kernel can't build and skips is no measure of the other's that it can.
    swallowed = []
    depth = [0]
    def watch(name, f):
        def call(*a, **k):
            depth[0] += 1
            try:
                return f(*a, **k)
            except Exception:
                # (Every call, in order: a program may round twice, one kernel failing only the
                # second.)
                if depth[0] == 1:
                    swallowed.append(name)
                raise
            finally:
                depth[0] -= 1
        return call
    for name in ["fillet", "chamfer", "shell", "union", "cut", "intersect", "loft", "sweep",
                 "revolve", "extrude", "twistExtrude", "cutBlind", "cutThruAll", "hole",
                 "cboreHole", "cskHole", "split", "combine", "mirror", "offset2D", "text"]:
        f = getattr(cq.Workplane, name, None)
        if f is not None:
            try:
                setattr(cq.Workplane, name, watch(name, f))
            except (TypeError, AttributeError):
                pass
    for line in sys.stdin:
        task = json.loads(line)
        out = {"i": task["i"], "name": task["name"]}
        swallowed.clear()
        t = time.perf_counter()
        random.seed(0)
        if np is not None:
            np.random.seed(0)
        try:
            import math
            ns = {"__name__": "cadevolve_gen", "cq": cq, "cadquery": cq, "math": math}
            if True:
                exec(compile(task["code"], task["name"], "exec"), ns)
                fn = ns.get(task["name"])
                if not callable(fn):
                    fns = [v for k, v in ns.items() if callable(v) and getattr(v, "__module__", None) == "cadevolve_gen"
                           and hasattr(v, "__code__")]
                    fn = fns[-1] if fns else None
                if fn is None:
                    out.update(status="no-function")
                else:
                    out.update(measure(cq, fn()))
        except BaseException as e:  # noqa: BLE001 - a generator may raise anything
            if isinstance(e, KeyboardInterrupt):
                raise
            tb = traceback.extract_tb(e.__traceback__)
            where = next((f"{f.name}:{f.lineno}" for f in reversed(tb) if f.filename == task["name"]), "")
            out.update(status="error", err=f"{type(e).__name__}: {str(e)[:300]}", where=where)
        if out.get("status") == "ok":
            out["swallowed"] = list(swallowed)
        out["secs"] = round(time.perf_counter() - t, 4)
        proto.write(json.dumps(out) + "\n")
        proto.flush()

if __name__ == "__main__":
    main()
