"""Record CADEvolve generators as tapes (see cadquery/__init__.py).

    python -I trace.py <generators.json> <out.jsonl> [index ...]     # all generators without indices

One JSON line per generator: {"i", "name", "status", "reason", "entries", "result"}.
status: "ok" (translatable tape), "dynamic" (reads geometry back), "unsupported" (API not ported),
"error" (the generator itself raised, e.g. a ValueError from its own checks).
"""
import ast
import json
import math
import os
import random
import signal
import sys
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
for m in [m for m in sys.modules if m == "cadquery" or m.startswith("cadquery.")]:
    del sys.modules[m]
import cadquery as cq  # noqa: E402  (the recorder)

assert cq.__version__.endswith("recorder"), cq.__file__


class _Timeout(Exception):
    pass


def _alarm(*_):
    raise _Timeout()


def try_ranges(code):
    """Line ranges of `try:` bodies whose handlers catch broadly (a failure there is swallowed)."""
    out = []
    try:
        tree = ast.parse(code)
    except SyntaxError:
        return out
    for node in ast.walk(tree):
        if isinstance(node, ast.Try) and node.body:
            out.append((node.body[0].lineno, max(getattr(n, "end_lineno", n.lineno) for n in node.body)))
    return out


def trace_one(i, g):
    name = g["name"]
    fname = f"<gen {i} {name}>"
    tape = cq._reset(fname, try_ranges(g["code"]))
    random.seed(0)
    ns = {"__name__": "gen", "cq": cq, "cadquery": cq, "math": math}
    rec = {"i": i, "name": name}
    signal.signal(signal.SIGALRM, _alarm)
    signal.alarm(10)
    try:
        exec(compile(g["code"], fname, "exec"), ns)
        fn = ns[name]
        res = fn()
        if isinstance(res, cq.Workplane):
            rec["result"] = {"wp": res._id}
        elif isinstance(res, cq.Shape):
            rec["result"] = {"shape": res._id}
        elif isinstance(res, (list, tuple)) and res and all(isinstance(r, (cq.Workplane, cq.Shape)) for r in res):
            rec["result"] = {"list": [cq._enc(r) for r in res]}
        else:
            raise cq.Unsupported(f"returns {type(res).__name__}")
        rec["status"] = "ok"
    except cq.Dynamic as e:
        rec["status"], rec["reason"] = "dynamic", str(e)
    except cq.Unsupported as e:
        rec["status"], rec["reason"] = "unsupported", str(e)
    except ImportError as e:
        rec["status"], rec["reason"] = "unsupported", f"import: {e}"
    except _Timeout:
        rec["status"], rec["reason"] = "error", "timeout in Python"
    except RecursionError:
        rec["status"], rec["reason"] = "error", "recursion"
    except Exception as e:
        tb = traceback.extract_tb(sys.exc_info()[2])
        where = next((f"{f.lineno}" for f in reversed(tb) if f.filename == fname), "?")
        rec["status"], rec["reason"] = "error", f"{type(e).__name__}: {e} (line {where})"
    finally:
        signal.alarm(0)
    rec["entries"] = tape.entries
    rec["guarded"] = any(e.get("guarded") for e in tape.entries)
    return rec


def main():
    gens = json.load(open(sys.argv[1]))
    idx = [int(a) for a in sys.argv[3:]] or range(len(gens))
    with open(sys.argv[2], "w") as out:
        for i in idx:
            rec = trace_one(i, gens[i])
            out.write(json.dumps(rec) + "\n")


if __name__ == "__main__":
    main()
