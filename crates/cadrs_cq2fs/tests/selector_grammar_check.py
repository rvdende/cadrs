"""Cross-check cq2fs's selector parser against CadQuery's own pyparsing grammar (selectors.py's
_makeGrammar / _makeExpressionGrammar, copied verbatim below with the selector classes replaced by
plain tuples) on every selector string in the tapes. Needs pyparsing only (no OCCT).

    <python with pyparsing> -I tests/selector_grammar_check.py <tapes.jsonl> <cq2fs binary>
"""
import json, subprocess, sys
from functools import reduce
from pyparsing import (Literal, Word, nums, Optional, Combine, one_of, Group, infix_notation, opAssoc,
                       pyparsing_common)

geom_LUT_EDGE = ["LINE", "CIRCLE", "ELLIPSE", "HYPERBOLA", "PARABOLA", "BEZIER", "BSPLINE", "OFFSET", "OTHER"]
geom_LUT_FACE = ["PLANE", "CYLINDER", "CONE", "SPHERE", "TORUS", "BEZIER", "BSPLINE", "REVOLUTION", "EXTRUSION", "OFFSET", "OTHER"]

def _makeGrammar():
    point = Literal('.')
    plusmin = Literal('+') | Literal('-')
    number = Word(nums)
    integer = Combine(Optional(plusmin) + number)
    floatn = Combine(integer + Optional(point + Optional(number)))
    lbracket = Literal('(')
    rbracket = Literal(')')
    comma = Literal(',')
    vector = Combine(lbracket + floatn('x') + comma + floatn('y') + comma + floatn('z') + rbracket, adjacent=False)
    simple_dir = one_of(['X', 'Y', 'Z', 'XY', 'XZ', 'YZ'])
    direction = simple_dir('simple_dir') | vector('vector_dir')
    cqtype = one_of(set(geom_LUT_EDGE) | set(geom_LUT_FACE), caseless=True)
    cqtype = cqtype.set_parse_action(pyparsing_common.upcase_tokens)
    type_op = Literal('%')
    direction_op = one_of(['>', '<'])
    center_nth_op = one_of(['>>', '<<'])
    ix_number = Group(Optional('-') + Word(nums))
    lsqbracket = Literal('[').suppress()
    rsqbracket = Literal(']').suppress()
    index = lsqbracket + ix_number('index') + rsqbracket
    other_op = one_of(['|', '#', '+', '-'])
    named_view = one_of(['front', 'back', 'left', 'right', 'top', 'bottom'])
    return direction('only_dir') | type_op('type_op') + cqtype('cq_type') | direction_op('dir_op') + direction('dir') + Optional(index) | center_nth_op('center_nth_op') + direction('dir') + Optional(index) | other_op('other_op') + direction('dir') | named_view('named_view')

AXES = {"X": (1, 0, 0), "Y": (0, 1, 0), "Z": (0, 0, 1), "XY": (1, 1, 0), "YZ": (0, 1, 1), "XZ": (1, 0, 1)}
VIEWS = {"front": ((0, 0, 1), True), "back": ((0, 0, 1), False), "left": ((1, 0, 0), False), "right": ((1, 0, 0), True), "top": ((0, 1, 0), True), "bottom": ((0, 1, 0), False)}

def vec(pr):
    if "vector_dir" in pr:
        v = pr.vector_dir
        return [float(v.x), float(v.y), float(v.z)]
    return [float(x) for x in AXES[pr.simple_dir]]

def atom_callback(res):  # _SimpleStringSyntaxSelector._chooseSelector, as cq2fs's compiled form
    pr = res
    if "only_dir" in pr:
        return {"k": "dir", "v": vec(pr)}
    if "type_op" in pr:
        return {"k": "type", "ty": pr.cq_type}
    if "dir_op" in pr:
        mx = pr.dir_op == ">"
        if "index" in pr:
            return {"k": "nth", "key": "center", "v": vec(pr), "n": int("".join(pr.index.asList())), "max": mx, "parallel": True}
        return {"k": "nth", "key": "center", "v": vec(pr), "n": -1, "max": mx, "parallel": False}
    if "center_nth_op" in pr:
        mx = pr.center_nth_op == ">>"
        n = int("".join(pr.index.asList())) if "index" in pr else -1
        return {"k": "nth", "key": "center", "v": vec(pr), "n": n, "max": mx, "parallel": False}
    if "other_op" in pr:
        v = vec(pr)
        k = {"|": "par", "#": "perp", "+": "dir", "-": "dir"}[pr.other_op]
        if pr.other_op == "-":
            v = [-x for x in v]
        return {"k": k, "v": v}
    v, mx = VIEWS[pr.named_view]
    return {"k": "nth", "key": "center", "v": [float(x) for x in v], "n": -1, "max": mx, "parallel": False}

def _makeExpressionGrammar(atom):
    and_op = Literal('and'); or_op = Literal('or'); delta_op = one_of(['exc', 'except']); not_op = Literal('not')
    atom.set_parse_action(atom_callback)
    def and_callback(res):
        items = res.asList()[0][::2]; return reduce(lambda a, b: {"k": "and", "a": a, "b": b}, items)
    def or_callback(res):
        items = res.asList()[0][::2]; return reduce(lambda a, b: {"k": "or", "a": a, "b": b}, items)
    def exc_callback(res):
        items = res.asList()[0][::2]; return reduce(lambda a, b: {"k": "exc", "a": a, "b": b}, items)
    def not_callback(res):
        return {"k": "not", "a": res.asList()[0][1]}
    return infix_notation(atom, [(and_op, 2, opAssoc.LEFT, and_callback), (or_op, 2, opAssoc.LEFT, or_callback),
                                 (delta_op, 2, opAssoc.LEFT, exc_callback), (not_op, 1, opAssoc.RIGHT, not_callback)])

GRAMMAR = _makeExpressionGrammar(_makeGrammar())

def cq_parse(s):
    try:
        return GRAMMAR.parse_string(s, parse_all=True).asList()[0]
    except Exception as e:
        return {"error": type(e).__name__}

def norm(x):
    if isinstance(x, dict):
        return {k: norm(v) for k, v in x.items()}
    if isinstance(x, list):
        return [norm(v) for v in x]
    if isinstance(x, float):
        return 0.0 if x == 0 else x
    return x

strings = set()
for line in open(sys.argv[1]):
    r = json.loads(line)
    for e in r["entries"]:
        if e["op"] in ("faces", "edges", "vertices", "solids"):
            s = e["kwargs"].get("selector", e["args"][0] if e["args"] else None)
            if isinstance(s, str):
                strings.add(s)
strings = sorted(strings)
out = subprocess.run([sys.argv[2], "selectors"], input="\n".join(json.dumps(s) for s in strings), capture_output=True, text=True).stdout.splitlines()
assert len(out) == len(strings), (len(out), len(strings))
bad = 0
for s, mine in zip(strings, out):
    mine = json.loads(mine)
    ref = cq_parse(s)
    if "error" in ref or "error" in mine:
        same = ("error" in ref) == ("error" in mine)
    else:
        same = norm(ref) == norm(mine)
    if not same:
        bad += 1
        print("MISMATCH", repr(s), "\n  cadquery:", ref, "\n  cq2fs:   ", mine)
print(f"{len(strings)} selector strings, {bad} mismatches")

# precedence corner cases, whatever the tapes contain
EXTRA = ["|X and not |Y or |Z", "not |X and |Y", "|X or not |Y and |Z", "not |X or |Y", "not not |X", "|X and not not |Y",
         "|X exc not |Y", "not |X exc |Y", "|X or |Y exc |Z and |X", "(not |X) and |Y", "|X and not |Y and |Z",
         "not (|X) or |Y and |Z", ">Z[-2]", "<<Y[1]", ">(1,0,0)", "(0,0,1)", "%cylinder or %CONE", "front", ">XY", "-Z", "#Y except >Z"]
out = subprocess.run([sys.argv[2], "selectors"], input="\n".join(json.dumps(s) for s in EXTRA), capture_output=True, text=True).stdout.splitlines()
bad = 0
for s, mine in zip(EXTRA, out):
    mine = json.loads(mine); ref = cq_parse(s)
    same = (("error" in ref) == ("error" in mine)) if ("error" in ref or "error" in mine) else norm(ref) == norm(mine)
    if not same:
        bad += 1; print("MISMATCH", repr(s), "\n  cadquery:", ref, "\n  cq2fs:   ", mine)
print(f"{len(EXTRA)} extra selector strings, {bad} mismatches")
