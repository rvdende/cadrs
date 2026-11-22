# Reference parts

Parts with expected values worked out independently of cadrs, rebuilt through cadrs' feature
list by `tests/reference_parts.rs` and compared (issue #101). The results table is
`docs/reference-parts.md`.

## `solidcraft/`

The reference cases of [SolidCraft](https://github.com/tangency-labs/solidcraft)
(`tests/oracle/`, at commit `fad397d325f573fe4bb0f64df70671407cb8acae`), copied unchanged:
each case's `recipe.json` (the part, in SolidCraft's recipe format) and `expected.json` (what
it must measure: closed-form values, or values from an independent FreeCAD build, as its
`source` says), with one exception below. Their `reference.stl` files (and `24-user-parameters`'
`sweep.json`) are not copied yet.
Copyright (c) 2026 SolidCraft contributors, used under the MIT License
(`solidcraft/LICENSE-MIT`); SolidCraft is also available under the Apache License 2.0.

Where SolidCraft changed a case's expected values for a convention of its own rather than a
geometric error, cadrs keeps the earlier values:

- `17-loft` keeps its FreeCAD values (from commit `0870d647`). SolidCraft's analytic values
  match each side of the square to one rational quarter of the circle's B-spline (35080.73 mm³),
  which depends on how the circle is stored; FreeCAD, like cadrs, matches by angle
  (35125.50 mm³, which the closed form gives to 9 digits).

`75-sm-bracket-holes` takes SolidCraft's new values (its two flanges now meet in a mitred
corner, both bends ending at the base's corner) and fails: cadrs makes no corner between flanges
of two features, so the first flange still runs the full 80 mm.

The recipe format and its conventions are described in SolidCraft's `tests/oracle/README.md`.
The translation into cadrs features follows SolidCraft's own reader
(`apps/solidcraft-cli/src/recipe.rs`) for what the format means, and its comparison rules
(`apps/solidcraft-cli/src/oracle.rs`): volume and area within a relative 1e-3 unless a case
says otherwise, centre of mass and bounding box within 1e-3 of the bounding box's diagonal,
face counts where given, and a STEP export read back with the same volume and area. cadrs
also checks the mesh it draws: each face's triangles cover its exact area within 1e-2.

## `issues/`

Parts from cadrs' issues and imports: the user's `document.ron`, `expected.json` and, where
there is one, `target.step` (the part as another CAD system builds it), from which the values
`expected.json` doesn't give are measured. A case measures the document's solids, or with
`"part": "<name>"` that one part (a surface too: its area, faces and bounding box; it has no
volume), against the target's surfaces. `"compare"` limits what is compared and `"tolerance"`
loosens it, for a case that only comes close (`race-car-surfaces`, an import whose splines are
approximated).

## Running

Each case is a test of its own, named after its folder (`case_49_helical_sweep` for
`49-helical-sweep/`), so one case or a few run by name:

```sh
cargo test -r -p cadrs_core --test reference_parts -- helical
```

A case copied in needs its line in the test file's `cases!` list; `every_case_has_a_test`
fails until it has one.

## Updating the passing list

`passing.txt` lists the cases cadrs passes; the test fails if one of them stops passing. After
a change that makes more cases pass, record them with:

```sh
CADRS_BLESS=1 cargo test -r -p cadrs_core --test reference_parts
```

which also rewrites `docs/reference-parts.md`.
