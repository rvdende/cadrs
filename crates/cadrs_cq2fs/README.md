# cadrs_cq2fs: Parasolid answers for CADEvolve cases

Reference answers for cadrs's kernel from a kernel we trust more than OCCT: Parasolid, run through
Onshape. CADEvolve's CadQuery generators are recorded in Python, translated to FeatureScript,
and evaluated by Onshape (`evalFeatureScript`) in a logged-in browser tab; cadrs runs the same
generators through its own CadQuery (`crates/cadrs_py`), and the two are compared.

## How a generator becomes FeatureScript

1. **Record** (`recorder/`): the generator runs against a stand-in `cadquery` package that computes
   no geometry. It resolves the Python (loops, arithmetic, `Vector`, `Plane`, `Location`, ported
   exactly from CadQuery 2.8's `geom.py`) and writes every CadQuery call to a *tape*. A generator
   that reads geometry back (`len(wp.vals())`, `BoundingBox()`, callbacks) is *dynamic* and left
   out. Names CadQuery 2.8 doesn't have raise `AttributeError`, as CadQuery does, so a
   generator's `except` paths are the ones CadQuery takes.
2. **Translate** (`src/`): each call is bound to CadQuery's own signature (`sigs.rs`), selector
   strings are compiled with a port of CadQuery's grammar (`selector.rs`), and the tape becomes
   calls into `fs/cqrt.fs`.
3. **Run** (`fs/cqrt.fs`): CadQuery's `Workplane` semantics written in FeatureScript, one
   function per `cq.py` method: the object stack, the context's pending wires and edges, planes
   and `toWorldCoords`, `eachpoint` placements, `workplane()` on faces, the selectors, booleans.
   Every geometric decision (which face is `>Z`, where a face's workplane goes) is made on
   Parasolid's geometry, not cadrs's. Bodies are never changed in place: an operation copies its
   inputs, so every shape value and selection stays valid, as CadQuery's values are immutable.

What isn't ported (lofts, sweeps, splines, text, tapered extrusions, chamfers with two lengths,
partial spheres) is refused at translation, never approximated.

## Checks on the translation

- `tests/selector_grammar_check.py`: the selector parser against CadQuery's own pyparsing grammar
  (copied verbatim), on every selector string the tapes use plus precedence corner cases.
- `cases/smoke.json`: 30 basic cases; 29 agree with cadrs to ~1e-15 in volume, area and bounding
  box (two independent ports of CadQuery agreeing). Primitives match their closed forms.

## The loop

```sh
cargo build -r -p cadrs_cq2fs -p cadrs_py      # cq2fs, and cadrs's CadQuery native module
# crates/cadrs_py/python/cadquery/_cadrs.abi3.so links to target/release/lib_cadrs.so
cq2fs scan                                     # record every generator: target/cq2fs/tapes.jsonl
cq2fs translate target/cq2fs/tapes.jsonl       # what translates, and why the rest doesn't
cq2fs add cases/<set>.json <i> ...             # add cases: generator.py, tape.json
cq2fs serve cases/<set>.json                   # job server; then in a cad.onshape.com tab:
#   eval(await (await fetch('http://127.0.0.1:8766/client.js', {targetAddressSpace: 'loopback'})).text())
cq2fs cadrs cases/<set>.json                   # cadrs's answers: cases/<dir>/cadrs.jsonl
cq2fs compare cases/<set>.json --cadrs cases/cadevolve/cadrs.jsonl
```

Each case keeps everything in `cases/<dir>/<i>_<name>/`: `generator.py` (CADEvolve's source,
Apache-2.0), `tape.json`, `case.fs` (exactly what Parasolid ran) and `parasolid.json` (its
volume, area, bounding box, topology counts, a volume after each step, warnings). A case is run
in Onshape once and its answer committed.

The tab uses the person's own Onshape session against a document they own (default
`test_shell`); `evalFeatureScript` evaluates in a scratch context and leaves the document
unchanged. Onshape rate-limits session requests (about 1,280 a day), so cases go in batches.
