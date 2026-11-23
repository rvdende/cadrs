#!/usr/bin/env python3
"""Decodes the opaque parts of an Onshape FeatureScript: the `qCompressed(...)` queries (what
each feature selects: entity type, the operation that made it, the sketch entities it derives
from) and the sketches' initial guesses (the solved circle centres and radii).

    fs_decode.py part.fs      # JSON on stdout
"""
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import query  # noqa: E402


def ids(v, out):
    """Every sketch-entity / operation id reached by a decoded query."""
    if isinstance(v, dict):
        for k, x in v.items():
            if k in ("derivedFrom", "originals", "disambiguationData", "entities"):
                ids(x, out)
            elif k == "operationId":
                out["operation"] = x
            elif k == "queryType":
                out["type"] = x
            elif k == "entityType":
                out["entity"] = x
            elif k == "historyType":
                out["history"] = x
            elif k == "disambiguationType":
                out["disambiguation"] = x
            elif k == "sketchEntity":
                out.setdefault("sketch_entities", []).append(x)
            else:
                ids(x, out)
    elif isinstance(v, list):
        for x in v:
            ids(x, out)
    elif isinstance(v, str) and re.fullmatch(r"[A-Za-z0-9]{12}", v):
        out.setdefault("sketch_entities", [])
        if v not in out["sketch_entities"]:
            out["sketch_entities"].append(v)


def describe(d):
    t = d.get("type", "?")
    e = d.get("entity", "")
    s = f"{e} query ({t})".strip()
    if d.get("history"):
        s += f", history {d['history']}"
    if d.get("operation"):
        s += f", from operation {d['operation']}"
    if d.get("disambiguation"):
        s += f", disambiguated by {d['disambiguation']}"
    if d.get("sketch_entities"):
        s += ", sketch entities " + ", ".join(d["sketch_entities"])
    return s


def main():
    src = open(sys.argv[1], encoding="utf-8").read()
    queries = {}
    for m in re.finditer(r'(\w+)=qCompressed\((1\.0,"[^"]+")', src):
        try:
            d = {}
            ids(query.plain(query.decode("qCompressed(" + m.group(2) + ")")), d)
            queries[m.group(1)] = {"summary": describe(d), **d}
        except Exception as e:  # a form the decoder does not know
            queries[m.group(1)] = {"summary": f"undecoded ({type(e).__name__})"}
    for m in re.finditer(r'(\w+)\s*=\s*qBodyType\(qCreatedBy\(id \+ "(\w+)", EntityType\.(\w+)\), BodyType\.(\w+)\)', src):
        queries[m.group(1)] = {"summary": f"{m.group(4)} {m.group(3).lower()} created by feature {m.group(2)}"}
    guesses = {}
    for m in re.finditer(r'const initialGuess(\w+) = (\{.*\});', src):
        try:
            guesses[m.group(1)] = json.loads(m.group(2))
        except ValueError:
            pass
    print(json.dumps({"queries": queries, "initialGuess": guesses}))


main()
