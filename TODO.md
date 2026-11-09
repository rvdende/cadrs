# TODO

The focus is **correctness for a release** of cadrs on the native kernel (`cadrs_kernel`, pure
Rust, no OpenCASCADE). New features wait until the release blockers are done.

## 1. Release blockers: correctness

### Real documents rebuild at least as well as on main (the old OpenCASCADE-based release)

All 149 stored documents (173 Part Studios) are rebuilt headlessly with kernel-10 and with main
(the old OpenCASCADE-based release) and compared per feature. Neither crashes or hangs.

| | Feature errors | New vs main |
|---|---|---|
| kernel-10 at `bdf82f3` (first run) | 140 | 63 in 28 Part Studios |
| kernel-10 at `dc8d23f` | ~84 | 4 |
| main (old release) | 106 | — |

Done when no Part Studio has an error that main doesn't also have, and none rebuilds much slower.
Rebuild a single document with `cadrs --document "<name>" --check`.

Fixed (dropped from the list): open sketch loops (d4 desk device v2/v3, d4_desk_device_test, emax,
squish's profile), the open-shell and 3-face booleans, plant_pot's extrude direction, the stroller,
pvc and rianatest blends, Dr Browns cap's Fillet 1 (rounds round a window across the dome's seam)
and the cable organizer's Fillet 2 (full rounds meeting at a corner), references (mate connectors, pibox thicken, t20, trimmer's unnamed
faces), mirror_test's mesh import, squish's Extrude 6 (the fuzzy boolean), the kids cart and
trimmer, Dr Browns cap's Boolean 3 (a merged face's seam now placed clear of its holes) and
nightlight's Boolean 7 (a circle clipping a thin cylinder between two samples).

Still new against main:
- [ ] Flagged boolean result: shape_type_a_a1 (a near-tangent tube on a slab cylinder; the SSI
  gives 8 curves without vertex ends). volumemaker_f5 and nightlight's Boolean 7 are fixed.

Differences without errors, still to check:
- [ ] Part counts that differ from main (garbage_cabinet 7 vs 3, TubeAdapter 3 vs 1 from chamfer
  slivers, racingrigrumble 10 vs 8, and others in compare runs).
- [ ] Small volume differences (pibox mount 1802.4 vs 1800.1 mm³, stroller chamfer 1429 vs 762 mm³).
- [ ] Rebuild time: 896 s locally against main's 423 s on gray, before the speed passes of
  2026-10-22/23 (their commit messages: the user documents 358 s to 295 s). Measure again
  against main. pico2, nightlight_sphere, ekgpulse, squish and trimmer were 2–5× slower.
- [ ] Add a document check to the release process. A small headless runner rebuilds every
  document and compares against a saved result. Private documents stay out of the repo; only
  minimal dumped bodies go in as regression fixtures.

### Kernel correctness (independent review 4, 7.45/10 at `b589424`)

- [ ] Fillets and blends:
  - SMLib fillets: 43/100 pass (the sheet inputs' results compared as sheets, on their area);
    group A, the tangent-chain cases whose face pair changes at a vertex (10 cases: 116, 226,
    231, 234, 237, 401, 410, 411, 412, 421), each stops on another limit past the junction (a
    fillet larger than the round it runs over, a rail running off a face, a crease of 1° taken as
    tangent); groups E, G and C are still open;
  - fillets take about 1.2× OCCT's time on smlib_fillets.
- [ ] Booleans: a boolean now checks that its result's faces don't cross and flags a result that does
  (`take_invalid_results`; `CADRS_BOOL_STRICT=1` makes it an error). Three booleans still
  return a flagged result: volumemaker_f5, shape_type_a_a1 and nightlight's Boolean 7.
- [ ] Offsets: smlib_offset has 4 known failures (offset_04, 05, 07, 08).
- [ ] Remaining known failures: `tools/smlib_known_failures/` (the SMLib ssi suites and a few
  others).
- [ ] The BRep-text reader: 5 dataset files the reference reads; CFI_5_b12fgg.rle edge count.
- [ ] STEP/IGES: 52 of the 389 dataset files don't read clean (`tests/exchange_corpus.rs`).
- [ ] Fillet speed: 1.17× the reference on smlib_fillet (CPU times, each
  case run by both in turn: the shared machine's load moves either side alone by tens of
  percent); the goal is at or below it. The result's check is about 40% of a local fillet's time.

### Release mechanics

- [ ] Branch protection: the required check is now "Kernel (checks, clippy, fast gate)".
- [ ] Full gate green on the release commit (`tools/kernel_gate.sh`), and an independent review of it.
- [ ] Merge `kernel-10` into main once the document check matches or beats main.

## 2. After the release

- [ ] An optional G2 corner mode for fillets: where three edges of different radii meet, ease each
  strip's radius to one vertex sphere's (Dahl §3.5.2) instead of the patched C0 corner (18° kink
  on a box with radii 1/2/3). Built and parked (`h3_corners.patch` in agent A's scratchpad): it
  makes each eased strip a B-spline, 2–5× slower on complex_e3/d2, with areas a little further
  from the reference's, so it stays out of the default path.

- [ ] Native B-rep threads: external and internal, multi-start, taper, partial length, run-outs.
  Built from exact helix curves and ruled surfaces. The Pali Ijo kernel's design
  (`packages/pali_ijo/doc/brep_threads.md` in github.com/braydennilsen/PaliCAD, MIT; the rest of
  PaliCAD is PolyForm Internal Use, so don't use it) is a good reference.
- [ ] Ideas from the Pali Ijo kernel (`packages/pali_ijo` only, MIT; credit "adapted from Pali Ijo,
  MIT, Copyright (c) 2026 Brayden Nilsen" wherever an algorithm is followed closely):
  - Mesh repair for imported meshes: vertex welding, removing degenerate faces, stitching
    junctions, sealing open boundaries, splitting connected components (`lib/src/mesh/mesh_build.dart`,
    `mesh_regions.dart`, `lib/src/brep/heal.dart`). This also helps the mirror_test import above.
  - Mesh tools for STL/3MF bodies: mesh booleans, convex hull and convexity check, Minkowski
    sums and differences (round offsets of meshes), plane cuts.
  - Exchange: 3MF and OBJ read and write, next to STEP, IGES and STL.
  - Chamfer by distance and angle: the second setback is `d1·sin(angle)/sin(beta + angle)`, where
    beta is the section angle between the faces; reject values that don't fit the section.
    Check it against ours.
  - Tessellation: keep patch boundaries watertight when the faces on either side need different
    sample counts (shared boundary samples), and use derivative bounds with aligned knots to
    avoid over-subdividing cubic spans.
  - Measurement: sphericity, next to volume, area, centre of mass and bounds.
- [ ] Feature gaps from review 4:
  - draft on freeform faces;
  - an N-argument cell decomposition;
  - booleans of mixed body kinds.

## Open from the 2026-10-23 session

Unfinished patches are kept outside the repository in `../cadrs-wip-2026-10-23/`.

- Booleans (WIP `fbwip/`, ungated): fb24 (retry empty or crossing results of near-coincident
  operands) and fb25 (periodic fit of closed traced curves) cut the soak classes a lot
  (pcurve_off 21->1, open_shell 23->2), but fb25 makes faces_type_i_f2, e9, offset_03 and
  bfuseblend_b7 10-50x slower. Fix that and regate; fb26-fb29 stack on them.
- Soak classes still failing: loop_crosses_itself 26, crossing_faces 17, tilt_first_order 26,
  tilt_tolerance_growth 24, step_round_trip 3 (`tests/metamorphic_booleans.rs`,
  `stress_booleans.rs`).
- Fallback-only document fillets: 11 of 16 build locally. fa47 (garbage_cabinet Chamfer 1,
  TubeAdapter Chamfer 2) passes locally but is ungated, and its volumes are below the
  fallback's (traced to the fallback's own errors). Still open: ESP32 Fillet 1, alu_extrusion
  Fillet 1 (`fa/alu_free_end_wip.diff`), piboxv7 Fillet 3, gridfinity Chamfer 2, breadboard
  Chamfer 1. Inputs are in `doc_fixtures`.
- Equal-distance chamfers on a curved face put their contacts at 1.07 and 1.2 instead of 1;
  settle the convention against an OCCT chamfer.
- fb20's curve-surface refinement costs about 4% on fillets (in the result check); keep the
  check exact.
- Three older tests embed user-document bodies (`tests/data/docs/cross_hole_fillet.body`,
  `pipe_snap_chamfer.body`, `wheel_window_chamfer.body`); move them to `$CADRS_DOC_FIXTURES`.
