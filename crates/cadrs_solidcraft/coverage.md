# Coverage: SolidCraft recipes → cadrs

What the translator in this crate handles, measured against the reference cases copied into
`crates/cadrs_core/tests/reference_parts/solidcraft/` (90 folders with a `recipe.json` and an
`expected.json`, unchanged from SolidCraft's `tests/oracle/` at commit
`fad397d325f573fe4bb0f64df70671407cb8acae`; MIT, Copyright (c) 2026 SolidCraft contributors).

**Result today: 90 / 90 fixtures translate, 89 / 90 pass the oracle.** One case fails, for a
geometry reason, not a translation gap:

| Case | Why it fails |
|---|---|
| `75-sm-bracket-holes` | SolidCraft's two flanges meet in a mitred corner, both bends ending at the base's corner; cadrs makes no corner between flanges of two features, so the first flange runs the full 80 mm. Documented in `crates/cadrs_core/tests/reference_parts/README.md`. |

(The task/issue text mentions 84 cases; the repository actually holds **90**. The counts here
are the repository's.)

## Feature operations

"Cases" is how many of the 90 fixtures use the operation at all; "pass" is how many of those
cases pass today (the single known failure uses `sketch`, `extrude` and `sm_flange`).

| Operation | Status | Cases | Pass | Notes |
|---|---|---:|---:|---|
| `sketch` | supported | 90 | 89 | lines, circles, three-point and centre-start-sweep arcs, points; constraints and dimensions (see below). |
| `extrude` | supported | 73 | 72 | extents `distance`, `through_all`, `symmetric`, `two_sides`; thin walls `one_side`/`two_sides`; operations new/add/join/cut/intersect; up-to-face. |
| `fillet` | supported | 23 | 23 | `constant_radius`, `chord_length`, `variable_radius`, `spline_radius`, `taper` (fit); `tangent_chain`; rounds and faces. |
| `hole` | supported | 11 | 11 | `simple`, `counterbore`, `countersink`; positional and end conditions the fixtures use; holes on planar and cylindrical faces. |
| `sm_flange` | supported | 10 | 9 | base and edge flanges, outer-face height, bend inside; flange types the fixtures use. |
| `construction_plane` | supported | 9 | 9 | methods `offset` and `at_angle` only. |
| `revolve` | supported | 8 | 8 | world Z axis; one-sided/symmetric/partial. |
| `chamfer` | supported | 7 | 7 | `equal_distance` only. |
| `shell` | supported | 6 | 6 | inward face-opening shells. |
| `combine` | supported | 5 | 5 | `join`, `cut`, `intersect`, `remove`, `new_body`. |
| `circular_pattern` | supported | 5 | 5 | world Z axis. |
| `rectangular_pattern` | supported | 3 | 3 | world-axis directions. |
| `sweep` | supported | 3 | 3 | sketch path (all or some of its curves); a helix `fitted_spline` path. |
| `loft` | supported | 3 | 3 | profiles/sections; no rails. |
| `mirror` | supported | 2 | 2 | features and bodies. |
| `draft` | supported | 2 | 2 | neutral plane the fixtures use. |
| `sheet_metal_rule_edit` | supported | 2 | 2 | the "Steel (mm)" rule's settings. |
| `split_body` | supported | 1 | 1 | the tool types the fixture uses. |
| `path_pattern` | supported | 1 | 1 | sketch-curve path. |
| `thread` | partial | 1 | 1 | cosmetic only (`"modeled": false`); a modelled thread is unsupported. |
| `sm_hem` | supported | 1 | 1 | the hem type the fixture uses. |
| `sm_unfold` | supported | 1 | 1 | full unfold; flat patterns laid out from another wall are unsupported. |
| `sm_refold` | supported | 1 | 1 | — |
| `boundary_surface` | supported | 1 | 1 | boundaries from sketch curves. |
| `thicken` | supported | 1 | 1 | — |
| `remove` | supported | 1 | 1 | — |

Every operation the 90 fixtures use is translated; none of them is reported as unsupported by
`check`.

## Sketch entities

| Entity | Status | Cases | Pass | Notes |
|---|---|---:|---:|---|
| `line` | supported | 80 | 79 | |
| `circle` | supported | 40 | 39 | |
| `arc_three_point` | supported | 11 | 11 | |
| `arc_center_start_sweep` | supported | 4 | 4 | |
| `point` | supported | 2 | 2 | |
| `fitted_spline` | partial | 1 | 1 | only a one-entity sketch whose fit points lie on a helix, which becomes a cadrs Helix feature (`49-helical-sweep`); any other spline is unsupported. |

Constraints: horizontal/vertical (line or two points), coincident/point-on-curve, midpoint,
symmetric, fix, tangent, equal, parallel, perpendicular, concentric. `collinear` is unsupported.
Dimensions: radius, diameter, offset, distance (aligned/horizontal/vertical, point-to-line,
origin-to-point), angle to a line.

## Not translated

An operation (or an option) outside the above is reported by `check` as `unsupported` — it is
not silently skipped, and a check with any unsupported case exits nonzero. Examples: a modelled
`thread`, `collinear` constraints, a fillet of several edges with a variable radius, a loft with
rails, a plane at an angle about a non-world axis, a sheet metal rule other than "Steel (mm)",
an `up_to` extent the fixtures do not use.

## Running the checks

```sh
cargo test -r -p cadrs_solidcraft --offline                     # the 90 cases, plus CLI smoke tests
cargo run -r -p cadrs_solidcraft --offline -- check <recipe.json>… [--json]
cargo test -r -p cadrs_core --test reference_parts --offline    # the same cases, one test each
```

`check` prints a per-case table (result, volume error, area error, face-count error, notes), the
operations used, and the unsupported-op coverage; it exits nonzero when a case fails or is
unsupported. The crate's test asserts the 89/90 result and a coverage floor of 89 passing cases,
and that no fixture is unsupported.
