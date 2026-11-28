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


def _broad(h):
    """A handler that catches whatever a CadQuery call raises."""
    if h.type is None:
        return True
    names = h.type.elts if isinstance(h.type, ast.Tuple) else [h.type]
    return any(isinstance(n, ast.Name) and n.id in ("Exception", "BaseException") for n in names)


class _Guards(ast.NodeTransformer):
    """Mark every `try:` body on the tape (cadquery._guard_begin / _guard_end, and
    _guard_caught at the top of each handler), so a failure Parasolid meets inside one is handled
    as CadQuery's caller would: a body that is one assignment `x = ...` with handlers that only
    `pass` ("skippable") leaves `x` as it was; any other is reported, not guessed. Bare
    `except:` becomes `except Exception:`, so the recorder's own signals (BaseExceptions) pass."""

    def __init__(self):
        self.k = 0

    def visit_Try(self, node):
        self.generic_visit(node)
        self.k += 1
        k = self.k
        for h in node.handlers:
            if h.type is None:
                h.type = ast.Name("Exception", ast.Load())
        one = node.body[0] if len(node.body) == 1 else None
        target = one.targets[0].id if isinstance(one, ast.Assign) and len(one.targets) == 1 and isinstance(one.targets[0], ast.Name) else None
        skippable = (target is not None and not node.orelse and not node.finalbody
                     and all(_broad(h) and all(isinstance(s, ast.Pass) for s in h.body) for h in node.handlers))
        val = f"(lambda: {target})" if skippable else "None"
        # The body, wrapped so its exit is always on the tape, and how: "end" (it ran to its
        # end, or left by return / break / continue) or "caught" (an exception left it).
        wrap = ast.parse(f"""
cq._guard_begin({k}, {skippable}, {val})
_cq_g{k} = [0]
try:
    pass
except BaseException:
    _cq_g{k}[0] = 1
    cq._guard_exit({k}, {val}, "caught")
    raise
finally:
    if _cq_g{k}[0] == 0:
        cq._guard_exit({k}, {val}, "end")
""").body
        wrap[2].body = node.body
        for n in wrap:
            ast.copy_location(n, node.body[0])
        node.body = wrap
        return node


def guarded_code(code, fname):
    tree = _Guards().visit(ast.parse(code))
    ast.fix_missing_locations(tree)
    return compile(tree, fname, "exec")


def trace_one(i, g, fail_at=()):
    name = g["name"]
    fname = f"<gen {i} {name}>"
    tape = cq._reset(fname, try_ranges(g["code"]), fail_at)
    random.seed(0)
    ns = {"__name__": "gen", "cq": cq, "cadquery": cq, "math": math}
    rec = {"i": i, "name": name}
    signal.signal(signal.SIGALRM, _alarm)
    signal.alarm(10)
    try:
        exec(guarded_code(g["code"], fname), ns)
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
    if fail_at:
        rec["fail_at"] = list(fail_at)
    rec["guarded"] = any(e.get("guarded") for e in tape.entries)
    return rec


def main():
    gens = json.load(open(sys.argv[1]))
    # --fail <i>=<step>,<step>: replay generator i with those steps raising (see CQFailure)
    fails = {}
    args = sys.argv[3:]
    while "--fail" in args:
        k = args.index("--fail")
        i, steps = args[k + 1].split("=")
        fails[int(i)] = [int(x) for x in steps.split(",") if x]
        del args[k:k + 2]
    idx = [int(a) for a in args] or range(len(gens))
    with open(sys.argv[2], "w") as out:
        for i in idx:
            rec = trace_one(i, gens[i], fails.get(i, ()))
            out.write(json.dumps(rec) + "\n")


if __name__ == "__main__":
    main()
