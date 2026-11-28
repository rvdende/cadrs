"""Run a generator on cadrs's CadQuery (crates/cadrs_py) and log, after every Workplane call, its
solids and their volume: the same calls in the same order as the tape's Workplane calls, so
`cq2fs steps` can line them up with Parasolid's step log and find where the two first differ.

    python -I cadrs_steps.py <generators.json> <i>   # JSON lines (cadrs's cadquery from crates/cadrs_py/python)
"""
import json
import math
import random
import sys
import traceback

import os

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../cadrs_py/python"))
import cadquery as real  # noqa: E402  (cadrs's)

assert not real.__version__.endswith("recorder"), "run with cadrs's cadquery on PYTHONPATH"


def measure(wp):
    try:
        objs = [o for o in wp.vals() if isinstance(o, real.Shape)]
        solids = [s for o in objs for s in o.Solids()]
        return {"solids": len(solids), "vol": sum(s.Volume() for s in solids)}
    except Exception as e:  # noqa: BLE001
        return {"err": f"{type(e).__name__}: {e}"}


LOG = []


class Workplane:
    """Wraps a real Workplane: every call forwarded, the result wrapped and logged."""

    def __init__(self, *args, _real=None, **kwargs):
        self._w = _real if _real is not None else real.Workplane(*[_unwrap(a) for a in args], **{k: _unwrap(v) for k, v in kwargs.items()})
        if _real is None:
            LOG.append({"op": "Workplane", **measure(self._w)})

    def __getattr__(self, name):
        attr = getattr(self._w, name)
        if not callable(attr):
            return attr

        def call(*args, **kwargs):
            try:
                r = attr(*[_unwrap(a) for a in args], **{k: _unwrap(v) for k, v in kwargs.items()})
            except Exception as e:
                LOG.append({"op": name, "raised": f"{type(e).__name__}: {e}"[:300]})
                raise
            if isinstance(r, real.Workplane):
                LOG.append({"op": name, **measure(r)})
                return Workplane(_real=r)
            return r
        return call

    # operators CadQuery defines, which the recorder logs as union/cut/intersect
    def __add__(self, o):
        return self.union(o)

    def __sub__(self, o):
        return self.cut(o)

    def __mul__(self, o):
        return self.intersect(o)

    def __or__(self, o):
        return self.union(o)

    def __and__(self, o):
        return self.intersect(o)

    def __iter__(self):
        return iter(self._w)

    def __len__(self):
        return len(self._w)


def _unwrap(v):
    if isinstance(v, Workplane):
        return v._w
    if isinstance(v, (list, tuple)):
        return type(v)(_unwrap(x) for x in v)
    return v


class _Proxy:
    def __getattr__(self, name):
        if name == "Workplane":
            return Workplane
        return getattr(real, name)


def main():
    gens = json.load(open(sys.argv[1]))
    i = int(sys.argv[2])
    g = gens[i]
    cq = _Proxy()
    sys.modules["cadquery"] = cq  # `import cadquery as cq` inside the generator
    random.seed(0)
    ns = {"__name__": "gen", "cq": cq, "cadquery": cq, "math": math}
    status = "ok"
    try:
        exec(compile(g["code"], g["name"], "exec"), ns)
        ns[g["name"]]()
    except Exception:
        status = "error: " + traceback.format_exc().strip().split("\n")[-1][:300]
    for n, s in enumerate(LOG):
        print(json.dumps({"n": n + 1, **s}))
    print(json.dumps({"status": status}))


if __name__ == "__main__":
    main()
