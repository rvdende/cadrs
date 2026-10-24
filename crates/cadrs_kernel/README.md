# cadrs_kernel: solid-modeling layer

## Why a crate of our own
Part Studio features need a B-rep kernel: exact solids with faces, edges and curved surfaces,
plus booleans, fillets, shells and so on. We keep that kernel **behind our own crate** so that:
- the rest of cadrs (`cadrs_core`, `cadrs_app`) never touches a kernel's types;
- the kernel is ours, in pure Rust (no C or C++: it builds in seconds, cross-compiles to Windows
  without a C++ toolchain, and could go to WASM or the GPU), and its operations can be
  rewritten one at a time behind the same API;
- **persistent naming** (stable face and edge IDs across rebuilds) works from the kernel's
  operation history, independent of how an operation is implemented.

## Shape of the crate
```
crates/cadrs_kernel/
  src/lib.rs          # public API: types, Kernel trait, errors
  src/types.rs        # Profile, Curve2, Plane, Frame, MassProperties, TriMesh, ids
  src/naming.rs       # persistent naming: FaceName/EdgeName/VertexName from op history (P3.2)
  src/projection.rs   # drawing-view projection types and source matching (P3C.2)
  src/native/          # the B-rep kernel (see "The native kernel" below)
  src/backend/mod.rs   # `backend::native::NativeKernel` (also `cadrs_kernel::DefaultKernel`)
  tests/conformance.rs # the conformance suite, 84 cases
  tests/occt_*.rs, smlib_*.rs # generated gate suites (see "Tests")
  tests/metamorphic_booleans.rs, stress_booleans.rs, fingerprint.rs # gates that need no reference kernel
  tests/fuzz_readers.rs # property tests: no input panics a reader or a B-spline constructor
  tests/exchange_corpus.rs # a public dataset's STEP and IGES files read and checked (needs the dataset)
  tests/native_debug.rs # reproductions and timings (several replay CADRS_DUMP_DIR dumps)
```
There's no Bevy dependency. `cadrs_core` depends on `cadrs_kernel`, and features rebuild through it
(see "Rebuilding the feature list" below).

## Using the kernel
The rest of cadrs uses the kernel through the `Kernel` trait (`src/lib.rs`) and the types in
`src/types.rs`; `DefaultKernel` is the native kernel. Every public item there is documented
(`#![warn(missing_docs)]`), each operation with its contract, and the main operations have
doctests (`cargo test --profile kernel -p cadrs_kernel --doc`).

```rust
use cadrs_kernel::{BoolOp, DefaultKernel, Extent, Kernel, Tessellation};

let mut k = DefaultKernel::new();                       // a session: it owns the bodies
let block = k.extrude(&rect, Extent::Blind(5.0))?.bodies[0];
let cut = k.boolean(BoolOp::Subtract, block, &[tool])?; // OpResult { bodies, history }
let edge = k.edges(cut.bodies[0])?[0].id;               // ids come from the queries
let round = k.fillet(cut.bodies[0], &[edge], 1.0)?;
let m = k.mass_properties(round.bodies[0])?;            // exact volume, area, inertia
let mesh = k.tessellate(round.bodies[0], Tessellation::default())?;
let step = k.export_step(&round.bodies)?;               // and import_step / import_model
```
- **Session and ids.** A session owns its bodies; a `BodyId` is valid until `release`, and ids
  are never reused. `FaceId`/`EdgeId`/`VertexId` are indices into one body, as `faces`, `edges`
  and `vertices` list them; names that survive rebuilds are `naming`'s (see "Persistent
  naming"), built from each operation's `History`.
- **Operations** never change their inputs. Each returns an `OpResult`: the new bodies and a
  `History` listing every result face as `generated` (with an `Origin`: profile curve, cap,
  input edge or vertex) or `modified` (from an input face), and the input faces `deleted`.
- **Errors** (`KernelError`) say what a caller can do: `InvalidParameter` (a size or selection
  that can't build: tell the user), `InvalidProfile`, `EmptyResult` (an Intersect that finds
  nothing), `OpenResult`/`OperationFailed` (the algorithm gave up: try other options),
  `InvalidInput` (a bad file or body), `Unsupported`, `UnknownBody`. A wrong body is never
  returned instead of an error.
- **Units and tolerances.** mm and radians. Exact geometry is within `tol::POINT` (1e-6 mm);
  read and healed bodies carry their own vertex and edge tolerances. See "Tolerances" below.
- **Validity.** Every result passes `Body::check()` (`native::topo`): the fast structural check
  operations run on their own results (references, loops closing in 3D and in (u, v), p-curves
  on their edges, edge uses, body kinds, positive volume). `Body::check_with(CheckLevel::Full)`
  adds what is too slow for every operation: loops crossing in (u, v) by exact segment tests,
  loop nesting and orientation, shell connectivity and orientation, adaptive edge and vertex
  consistency, faces of a shell crossing each other away from their shared edges
  (`native::check_faces`), and void shells inside their solid; it is meant for tests, and
  every generated suite (DRAW's `checkshape`, the SMLib suites' result checks) runs it.
- **Healing.** `Kernel::heal(body, &[HealOp])` makes an imported or damaged body sound and
  upgrades its geometry, step by step (`native::heal`, `native::heal_split`): fix (loops
  reordered and closed, small edges dropped, missing seams added, face senses and shell
  orientation, p-curves refitted, tolerances fitted), gaps, same-parameter, tolerance update,
  direct faces, conversion to surfaces of revolution and to B-splines, splits by angle, of
  closed faces, at C0 breaks and into Bézier patches, unify and wire tails.
- **Exchange.** `export_step`/`import_step` for plain STEP, `export_model`/`import_model` for
  STEP or IGES with names and assemblies, `mesh_solid` for STL triangles; `write_body` and
  `read_body` are the exact native body format (`native::bodyio`), which keeps every face,
  edge and vertex in order, so names stay valid across a cache or the network.
- **Determinism.** The same inputs are to give the same bytes (bodies, meshes, STEP text)
  whatever the number of rayon threads; `tests/determinism.rs` checks it for every family of
  operation (extrude, revolve, the three booleans, fillet, chamfer, shell, offsets, draft, sweep,
  loft, sew, heal, tessellation, mass properties, STEP export and import), run twice and on 1
  and 4 threads.
- `Profile` is built from `cadrs_sketch` regions using **exact** boundary curves (lines, arcs,
  circles, ellipses, Béziers) on a sketch `Plane`, never the polyline used for display.
- About 45 trait methods default to `Unsupported` so a backend can start small; the native
  kernel implements them all.

### Curated API
`native/mod.rs` declares the kernel's modules `pub(crate)` and re-exports a small stable set:
`NativeKernel`; the B-rep data model (`Body`, `BodyBuilder`, `Vertex`, `Edge`, `Coedge`, `Face`,
`Shell`, `Solid`, `Wire`, `CheckLevel`: a `Body`'s entity lists are read through `Body::verts`,
`edges`, `faces`, … and a body is built with `BodyBuilder`, which checks it; the lists are
`pub` only under `testkit`); its geometry (`Curve`, `Surface`, `NurbsCurve`, `NurbsSurface`)
and vector types (`P3`, `V3`, `Frame`, `Affine`); and the modules `tol` (the tolerance model)
and `bodyio` (the body format). That, `Kernel`, `DefaultKernel` and the types are the public
surface, all documented (`#![warn(missing_docs)]` holds for the whole crate without features).
No other cadrs crate names anything else.

The internals are for the crate's own tests and tools: the `testkit` feature (which the crate's
tests turn on through their dev-dependency) makes every module `pub` again and adds
`cadrs_kernel::testkit`, which re-exports the modules the tests use (`draw`, `oracle`, `smb`,
`brep_occt`, `step`, `iges`, `ssi`, `classify`, `mass`, `tess`, `unify`, `features`, `evolved`,
`surfacing`, ...). The tests still name them as `native::<module>`, which keeps working under
`testkit`; they move to `testkit::` one file at a time (the generators, `tools/occt_to_rust.py`
and the SMLib ones, switch their `use` lines in the same change), after which the `native::`
paths can be sealed. Without `testkit`, what only the DRAW harness reaches (the BRep-text and
SMLib readers, 2D offsets, wire tails, faces from loose edges, area splits) is unused code.
`cargo check -p cadrs_kernel` (no features) and `cargo check -p cadrs_core` are the gate.

## Persistent naming
Every operation returns a `History` (what was generated or modified from which input face, edge
or profile curve). `naming.rs` gives each face and edge a stable name, derived from the feature
that created it and its origin (e.g. "Extrude 1 / side of sketch curve c7" or "Extrude 1 / end
cap"), and resolves names back to backend sub-shapes after a rebuild. Backends that don't report
history get it inferred from geometry. This is what makes "fillet these edges" survive an edit to
an earlier sketch.

**Implemented (P3.2).** `cadrs_kernel::naming`, backend-independent (pure Rust over `History`,
`FaceInfo`, `EdgeInfo`, `VertexInfo`):
- `FaceName { op, origin, split }`. `op` is the operation's id (`OpId`, a uuid: `cadrs_core` uses
  the feature's id). `origin` is `Cap { region, end }`, `Side { region, curve }` (a profile
  region's `source` and a profile curve's `source`: `cadrs_core` uses a key of the region's sketch
  and boundary curves, and the sketch curve id), `FromEdge { edge }` / `FromVertex { vertex }`
  (a stable FNV-1a hash of the input edge's or vertex's name, for fillet and chamfer faces), or
  `Unnamed { index }` for a face the history says nothing about. A face an operation only trims
  keeps its input's name (`History::modified`). A face split into pieces gives each piece the
  name with `split = parent·16 + k + 1`, pieces ordered by centroid; where two input faces merge,
  the smaller name wins.
- `EdgeName { faces: [FaceName; 2] (sorted), index }`: an edge is named by the faces on either
  side. Edges between the same two faces that join end to end (a sketch curve cut into pieces)
  share one name; separate chains are indexed by position (smallest midpoint, rounded to 1 µm).
  Edges inside one named face (a seam, a joint between two pieces of one curve) get no name.
- `VertexName { faces: [FaceName; 3], index }`: the three smallest names of the faces around it;
  points where fewer than three faces meet (a circle's seam) are not vertices.
- `name_body(kernel, body, op, history, inputs)` names a whole body (`BodyNames`, by kernel id).
- `resolve(name, candidates, distance, tol)`: the exact name, else a unique entity with the same
  base name (`split`/`index` dropped), else the one of several the hint puts nearest, else, as a
  geometric fallback, the entity within `tol` of where the reference was. Otherwise `Lost`: an
  error, never a guess. `cadrs_core` stores a seed point with a sketch-on-face reference (the
  face must contain it for the fallback) and uses the projected curve or the pierced point as
  the hint for Use and Pierce links; a reference found other than by its exact name is repaired
  to the current name.
- Regions are keyed by their sketch and boundary curves, not their place in the extrude's list,
  so adding, removing or reordering the other regions leaves the names alone (tested: removing
  the hub ring from the Control Arm's Extrude 1 makes the sketch on its top face *lost*, it does
  not jump to the coplanar web top).

Tests: `crates/cadrs_kernel/src/naming.rs` (unit), the conformance cases below, and
`crates/cadrs_core/tests/naming.rs` (the P3.2 acceptance: on the Control Arm, after a sketch
dimension change (eye hole Ø20 → Ø24), an extrude depth change (40 → 55, 25 → 10) and a reorder
of the two extrudes, every sketch on a face, Use and Pierce link and stored edge name resolves by
its exact name to the geometrically matching entity; a redrawn curve is found again by the
geometric fallback; a removed region is a lost reference; a version 3 document loads).


## The native kernel
`cadrs_kernel::native` is a B-rep kernel written for cadrs in Rust: exact geometry, boundary
representation, booleans, features, tessellation, mass properties, hidden-line removal and
STEP/IGES exchange.

**Geometry.** Curves (`curve.rs`): lines, circles, ellipses and rational B-splines (`nurbs.rs`,
homogeneous control points; banded B-spline interpolation, so large fits are O(n)). Surfaces
(`surface.rs`): plane, cylinder, cone, sphere, torus, surfaces of revolution and extrusion of any
curve, and rational B-spline surfaces. Lengths are mm; tolerances are in "Tolerances" below.
B-splines from outside the kernel (files, scripts) go through the checked constructors
`NurbsCurve::new`, `NurbsSurface::new` and `NurbsCurve::try_bezier` (knot count, monotone finite
knots, degree, finite poles, positive weights), so a malformed one is an error, not an index
panic in an evaluator.

**Topology** (`topo.rs`). A `Body` has vertices, edges (a curve on [t0, t1]; a closed edge, a whole
circle, has one vertex; degenerate edges sit at surface poles), faces (a surface, a sense and
loops of coedges, each with a p-curve parametrised by the edge's t), shells and solids. Loops run
counter-clockwise about the face's outward normal; the sense is derived from the uv loop
orientation (`build::fix_senses`). Periodic faces have seams (an edge used twice by one face).

**Operations, by module.**
| Module | What it does |
|---|---|
| `profile`, `sweep`, `build`, `features` | Sketch regions to profile pieces (merged across shared curves), and one sweep builder for prisms and revolutions (`SweepKind`: the side surface per piece and its p-curves); extrudes and revolves with every end type, thin walls and surfaces. |
| `sweep::path`, `sweep::frames`, `sweep::corner`, `sweep::body` | Sweeps along paths: lines and arcs exactly (prism and revolve pieces glued at their caps); other curves with rotation-minimizing frames (double reflection), one skinned B-spline surface per profile curve over a tangent run of path curves, whose rails are exactly the surfaces' iso-curves; mitred and round corner joins; points, wires and sheets swept one kind higher. |
| `evolved`, `cells` | Evolved solids (`Kernel::evolved`: a profile swept square to a planar spine of lines and arcs, arc joins at its outer corners; DRAW's `evolved` with wire profiles, open ones closed by lids, cut into strips that reach the spine) and the cells of an arrangement of solids (`Kernel::cells`). |
| `loft_build` | Lofts: exact skinning of compatible section curves (two sections give ruled surfaces), or sampled interpolation with end conditions. `loft_pieces` skins sections that already correspond. |
| `ssi` | Surface intersections: analytic cases (planes, quadrics, tori, coaxial surfaces of revolution, plane × extrusion, plane × cone as exact conics), and the numeric engine (`ssi_numeric`): the uv boxes cut into cells at knots and quarter turns, cell pairs subdivided while their boxes overlap until their normal cones hold no pair of parallel normals (Sederberg–Meyers' loop test), seeds where branches cross the cells' borders, each branch marched along n_a × n_b and corrected onto both surfaces, then fitted (projections hinted by the neighbouring point). Curve–curve and curve–surface hits. |
| `boolean`, `boolean_kinds`, `split`, `classify`, `unify` | Booleans: a pool of the inputs' vertices and edges, split at vertex-on-edge, edge–edge, edge–face and face–face intersections; coincident faces imprinted and coincident pieces made one edge; faces split in uv; pieces classified by exact ray parity; the kept faces assembled into shells; coplanar and same-surface faces merged (`unify`). Sheets, wires and points (`boolean_kinds`): sheets split by `imprint` and their pieces kept by a point inside each, solid − sheet a split into solids, wires through `edge_bop`; the result kind per operation is tabled in the module. |
| `fillet`, `fillet_geom` | Constant-radius fillets by local trimming (SMLib's fillet executive): the rolling ball's contact curves on both faces, the strip imprinted into its faces in place of the edge (no boolean), three-edge corners a sphere patch, other corners (some of a vertex's edges filleted, or more than three) set back with one patch filling the hole; end faces that meet past the vertex grow over the fillet's end, and end faces that only touch at the vertex (a corner whose next round an outward shell took away) are capped on that round's surface, one end edge swept along the other; faces the strip hangs over grow to its cut (seams run on to it); two strips rounding the same way either side of a face too narrow for both weld across it, each radius that of the ball touching all three faces where its full one would reach past the other's wall, their rails one edge where they meet (a full round where they meet all along); width fillets where the faces' angle varies take a law of radii, two-distance chamfers on a face curving across a straight edge measure round it; what it doesn't cover falls back to `blend`. |
| `blend`, `blend_chain`, `blend_corner` | Fillets and chamfers as tool bodies (an exact prism or revolution of the rolling-ball or chamfer section where the edge allows, a lofted chain tool otherwise) subtracted or added; corner tools where three blended edges meet; variable, partial, asymmetric, conic and curvature sections; overlap and fit checks. |
| `local`, `offset_round`, `slab` | Offsets (sharp: the faces' new surfaces re-intersected; rounded: the body ∪ each face's slab ∪ a tube round each convex edge (none on smooth or concave edges) ∪ a ball at each vertex, a concave cylinder's slab its wedge out to the axis when the distance passes its radius; rounded insets: the body less each face's inward slab, a tube round each concave edge and a ball at each of its vertices), drafts, face replacement (`Kernel::replace_faces`, SMLib's local operation, the new surface's domain picking the crossings), shells (inward and outward, hollow, corners rounded or sharp; the cavity built from its definition: the body less each kept face's slab, tubes and balls). Slabs keep the body's edge and vertex tolerances. |
| `surfacing` | Thicken (a sewn sheet with flat open ends shelled as the solid it bounds), fill (planar, Coons), sew, simplify, delete faces. |
| `defeature` | Delete face where the deleted faces aren't whole inner loops (a teardrop, a fillet, a chamfer, a notch running into a pocket): the empty space (a cut) or material (a protrusion) in a box round each group of deleted faces, split into cells by the extended surfaces of the faces next to them (planes and cylinders for now); the cells on the deleted faces are added or taken away and faces on one surface merged. Right when every face of the result continues one of the body's; else a larger box. |
| `move_sweep` | Move face where the faces round the moved ones can't just be extended to meet them (a face moved off a coplanar neighbour, a wall's end moved with the fillets round it): each moved face, and the fillets it carries along with the corners between them, swept to where it goes (`offset::slab::face_prism`), the sweeps added to the body where they move out or cut from it where they move in, faces on one surface merged; the sweeps' sides fill the gaps (a step). Right when no moved face is left where it was. |
| `mass` | Exact mass properties: the divergence theorem on each face, integrated by Green's theorem over the uv domain (nested Gauss–Legendre), along v for ruled faces, and by a tensor rule on knot spans for untrimmed B-spline patches. |
| `tess` | Tessellation: every edge sampled once (chord deflection and half the angle step), each face triangulated by a constrained Delaunay triangulation (spade) in metric-scaled uv, in parallel (rayon). |
| `hlr` | Hidden-line removal for drawing views: exact silhouettes and edges, crossings refined by Newton, visibility by rays against the faces. |
| `silhouette`, `distance`, `face_build`, `uvdomain` | Queries (`Kernel::silhouettes`, `min_distance`, `max_distance`, `face_from_surface`): silhouette curves of faces for parallel and perspective views (exact generators and circles on quadrics, elsewhere n·w = 0 traced by marching squares through saddle crossings and refined onto the zero, clipped to the face; `hlr` draws its outlines from them); minimum and maximum distances between points, edges, faces and bodies (each entity cut into faces, edges and vertices; branch and bound on cell boxes, then damped Newton with all second derivatives); a sheet face of a bare surface over its natural or a given uv domain (iso-curve edges, poles as degenerate edges, seams where the domain closes); a face's trimmed uv domain for these. |
| `step`, `iges`, `xbuild`, `exchange`, `bodyio`, `meshsolid` | STEP (AP214) and IGES read and write with assemblies; other systems' files rebuilt into valid bodies; an exact binary body format (`write_body`/`read_body`, the rebuild cache's); triangle meshes to solids. |

**Tolerances** (`native::tol`). One model, three levels ten times apart (mm):
- `CONFUSION` 1e-7: exact geometry; points closer are one point, and no entity's own tolerance is
  smaller (`topo::TOL_MIN`).
- `POINT` 1e-6: what an operation merges. A boolean pools vertices closer than this
  (`boolean::VTOL`); `Body::check` allows an edge end or a p-curve this far off when the entity's
  own tolerance is smaller.
- `LOOP_GAP` 1e-5: the largest gap between consecutive p-curves of a loop that `check` accepts at
  a vertex of smaller tolerance.
- `SIZE` 1e8: the size box. Beyond it a double no longer resolves `CONFUSION`; the readers refuse
  geometry outside it.

Below these, the code's round-off margins are named by meaning (all 1e-9, so results are what
they were as literals): `tol::REL` (relative: scaled by a size, a span or `1 + |t|`, or a
parameter fraction), `tol::ANGULAR` (unit vectors parallel or square, angles in radians),
`tol::ROUNDOFF` (absolute lengths and parameters of exact geometry), and `tol::TINY` (1e-300, the
guard of a division or normalisation against zero). A thousandth of them (1e-12) for values
computed exactly: `tol::REL_FINE`, `ANGULAR_FINE`, `ROUNDOFF_FINE`, and `tol::DEGENERATE` (a
computed length, norm or denominator taken as zero: no direction, nothing to divide by). At the
merge level (1e-6): `tol::POINT` (absolute), `REL_POINT` (a fraction of a size or span),
`ANGULAR_POINT` (directions or angles taken as the same) and `DIFF_STEP` (the step of a finite
difference or of a nudge off a point). Bare literals of these values are left only in tests and
where a constant names them already.

Above these, vertices and edges carry their own tolerance (a read or healed body's gaps,
`Body::fit_tolerances`), and operations take the larger of their own and the entities'.
The values an operation runs with are a `tol::Tolerances` carried by the session
(`NativeKernel::tolerances`, `set_tolerances`; the default is the model above) and passed
explicitly: `boolean::boolean_bodies_with(a, b, op, BoolOptions { tol, .. })`. A boolean whose
first run leaves an open shell runs again at `Tolerances::retries` (1e-5, then 1e-4). A run
passes its merge distance down to its helpers and the intersections it calls as an argument
(no thread-local or global state); `BoolOptions` also says whether faces on one surface are
merged afterwards (`merge_coplanar`, off for the DRAW harness). A result that fails `Body::check` is an error,
retried at the larger tolerances. A boolean also checks that no two faces of its result cross
(across the inputs, and loops crossing in (u, v) on new periodic faces): a result whose only
fault is crossing faces is returned flagged, not failed, and `native::take_invalid_results()`
counts such results on the thread; with `CADRS_BOOL_STRICT=1` it is a `BooleanFailed` error
instead (the tests run strict).

**Errors.** Every operation returns `Result<_, KernelError>`, with a message in words for the
user. The variant says what a caller can do: `EmptyResult` (an Intersect of bodies that don't
meet: cadrs reads it as "no interference" or "nothing left of the part"), `OpenResult` (faces
that don't close into a solid; a boolean retries at larger tolerances on it), `InvalidInput`
(a file or body that doesn't read, malformed B-splines, non-finite numbers, geometry outside the
size box), `InvalidParameter` and `InvalidProfile` (the caller's values), `Unsupported`,
`UnknownBody`. When an algorithm gives up (`KernelError::is_failure`), the variant says where:
`TooLarge { op }` (a radius, distance or thickness the geometry can't hold: a smaller one may
work), `InvalidGeometry { op }` (tangent faces at a blended edge, a degenerate section, faces
that no longer meet), `BooleanFailed { stage }` (`"face closure"`, `"validity"`, `"crossing"`, `"sheet
union"`), `BlendFailed { stage }` (a local fillet or chamfer that gave up: `"rolling ball"`,
`"corner"`, `"end"`, `"surface"`, `"trimming"`), `BudgetExhausted { stage }` (a boolean's or a
blend's deterministic work budget ran out), `InvalidResult { op }` (a built body failed its check and was not returned), and
`OperationFailed` for the rest. The enum is `#[non_exhaustive]`. `KernelError::message` gives
the words without the kind's prefix; `Display` is "operation failed: ..." for every failure.

**Environment variables.** None is needed in normal use. Every one the kernel reads is listed
here; `tools/kernel_gate.sh` and CI run with none of the switches set.

Debug output (on stderr; results don't change):
- `CADRS_PROFILE=1`: each boolean's phases, slow mass properties and tessellations, sweeps.
  `CADRS_TESS_PROFILE=1`: a face tessellation's phases.
- `CADRS_BOOL_DEBUG=1`, `CADRS_SPLIT_DEBUG=1`, `CADRS_SHELL_DEBUG=1`, `CADRS_VERTEX_DEBUG=1`,
  `CADRS_PCURVE_DEBUG=1`, `CADRS_OFFSET_DEBUG=1`, `CADRS_SLAB_DEBUG=1`: the boolean's
  intersections, face splits and output faces; face splitting in uv; a shell's cavity steps;
  vertex merging; p-curve fits that don't converge; the offset's piece whose boolean fails
  (with `CADRS_DUMP_DIR`, both inputs are written); a face slab's build warnings.
- `CADRS_MOVE_DEBUG=1`: why Move face's kept path and its swept fallback both refused a move.
- `CADRS_SNAP_DEBUG=1`, `CADRS_TANGENT_DEBUG=1`, `CADRS_EBOP_DEBUG=1`: a boolean's snapping of
  B-spline faces onto planes and cylinders, its tangent B-spline contacts, an edge boolean's
  splits.
- `CADRS_SSI_DEBUG=1` (with `CADRS_SSI_SEEDS=1` and `CADRS_SSI_DUMP=1` for the seeds and the
  traced points): surface-surface intersection.
- `CADRS_FILLET_DEBUG=1`, `CADRS_BLEND_DEBUG=1`, `CADRS_DRAFT_DEBUG=1`, `CADRS_SWEEP_DEBUG=1`:
  the local fillet's progress, with times; the blend tool's chains, stations and corners (and why
  a local fillet fell back to it); a draft's edges and unsupported faces; a sweep's path and
  helix fits.
- `CADRS_BOOL_WORK=1` (`boolean.rs`): each boolean's work count against its budget.
- `CADRS_EXCHANGE_DEBUG=1`: the warnings of a STEP, IGES or BRep import. `CADRS_ORDER_DEBUG=1`:
  the DRAW harness's edge order (`explode e`, test code).
- `CADRS_DUMP_DIR=<dir>`: shells, fillets and chamfers write their input body (exact bytes) and
  parameters there, with the outcome; slow mass properties too. `tests/native_debug.rs` replays
  them (`CADRS_REPRO=<file.body> cargo test -r -p cadrs_kernel --test native_debug debug_repro_shell`).

Debug switches that **change results** (to isolate a stage when a case fails; never set them
otherwise):
- `CADRS_DEBUG_NO_RETRY=1` (`boolean.rs`): no retries of a boolean at larger tolerances, to
  see the first run's failure.
- `CADRS_DEBUG_NO_SNAP=1` (`boolean.rs`): no snapping of B-spline faces onto the other body's
  planes and cylinders before a boolean.
- `CADRS_DEBUG_NO_PROPAGATE=1` (`boolean.rs`): each split face is classified on its own, without
  the in/out propagation through its connected group.
- `CADRS_NO_LOCAL_FILLET=1` (`blend.rs`): circular fillets (by radius or width) and chamfers
  skip the local (boolean-free) blend and go straight to the blend tool.
  `CADRS_LOCAL_FILLET_ONLY=1`: the opposite, the blend tool is never used (a local blend's
  failure is the result), for measuring what local trimming covers.
- `CADRS_NO_RECOG_S=1`, `CADRS_NO_RECOG_C=1` (`xbuild.rs`): imported B-spline surfaces and
  curves are kept as B-splines instead of being recognised as planes, cylinders, cones, spheres,
  tori and extrusions, lines and circles.
- `CADRS_BOOL_STRICT=1` (`boolean.rs`): a boolean whose result has crossing faces fails
  instead of returning the result flagged (`take_invalid_results`). The metamorphic and
  stress tests don't set it; their helper (`tests/meta_common`) treats a flagged result as an
  error instead.
- `CADRS_OCCT_REBUILD=1` (`brep_occt.rs`): a text BRep file's faces are always rebuilt from
  their edges instead of being read directly.

Test harness (tests only): `CADRS_OCCT_DATA` (the dataset of the generated suites,
default `../occt-dataset/opencascade-dataset-7.8.0` beside the checkout), `CADRS_SMLIB_ORACLE`
(the SMLib oracle binary), `CADRS_USD_BREP` (a usd-brep checkout, for the ignored
`smb::every_ssi_part_reads`, which skips without it),
`CADRS_EXCHANGE_FILES` and `CADRS_EXPORT_DIR` (the ignored corpus tests in `exchange.rs`),
`PROPTEST_CASES`, `FUZZ_TRACE`, `FUZZ_REPRO` (`tests/fuzz_readers.rs`) and the `CADRS_REPRO*`
inputs of `tests/native_debug.rs`'s replays.

**Status.** All 84 conformance cases pass (`cargo test -r -p cadrs_kernel --test conformance`,
about 3 s). The measured state of every suite is the gate's: `tools/kernel_gate.sh` (below). At
the last full gate (2026-10-23) about 6550 cases passed and about 486 were known failures
(each listed with its reason). No occt_blend case falls back to the tool-and-boolean blend (`blend`) any more.
`python3 -I tools/kernel_scoreboard.py` shows its last results per test binary (cases run,
passing, known failures, stale ones, regressions, hangs, and why the rest don't run yet); with no
results, or with `--static`, it falls back to reading the suites' source, which runs nothing.

**Tests.**
- Unit tests in the modules (`cargo test -r -p cadrs_kernel --lib`), the conformance suite, and
  the suites generated from SMLib's tests (`tests/smlib_*.rs`) and from OpenCASCADE's public test
  scripts (`tests/occt_*.rs`, `tools/occt_to_rust.py`), which serve as a reference test corpus. A generated case listed in
  `tools/occt_known_failures.txt` or `tools/smlib_known_failures*` asserts that it still fails, and that it fails by returning
  its error: a panic (an index out of bounds in the test code, an unwrap) fails the test, and
  the gate calls it a PANIC. When it passes, remove the line and regenerate the file. Every line
  of `tools/occt_known_failures.txt` gives its reason, inline (`group case  # why`) or in the
  comment just above its group of lines (a blank line ends a group); the generator refuses a
  line without one, copies the reason above the generated case, and warns about lines that
  aren't a converted case. A reason holding `[panic: text]` lets the case panic with that text.
  The DRAW harness reports a script's sub-shape past the end of an `explode` as a "topology
  mismatch" failure (`draw::sub`): the shape exploded has other topology than the script expects.
- Gates that need no reference kernel: `tests/metamorphic_booleans.rs` (on random boxes,
  cylinders and spheres, some rotated: vol(A ∪ B) + vol(A ∩ B) = vol(A) + vol(B), tool-order
  invariance, A − A = ∅, A ∪ A = A, a STEP round trip of the union, and random extrude sequences
  of join, cut and intersect steps checked after each step against the exact volume of the model
  so far), `tests/stress_booleans.rs` (a cube united with copies rotated by tiny angles across
  the tolerance, 12 iterated unions checked for volume and tolerance drift, an iterated
  drill-bit cut chain) and `tests/fingerprint.rs` (a hash of each result's body for a sample of
  booleans and offsets on 1 and 4 rayon threads, which must agree). `CADRS_META_SEED`,
  `CADRS_META_CASES`, `CADRS_STRESS_*` and `CADRS_FINGERPRINT_*` set seeds, sizes and output.
- The generated suites run on a DRAW-style harness (`native::draw`) and the SMLib oracle's body
  writer (`native::oracle`): test code, built only for tests and with the `testkit` feature
  (the crate's own tests turn it on through a dev-dependency on itself), not kernel API.
- `tests/fuzz_readers.rs`: property tests (proptest) that feed the STEP, IGES, BRep-text, SMLib
  and body readers edited files (numbers swapped for extreme ones, bytes flipped, cut short) and
  random bytes, and the B-spline constructors random knots, degrees, weights and points; nothing
  may panic. `PROPTEST_CASES=<n>` runs more cases, `FUZZ_TRACE=1` prints each, and the ignored
  `replay` test reads the file `FUZZ_REPRO` names with every reader (the one ignored test).
- Clippy is a gate: `tools/kernel_clippy.sh` (all targets, warnings denied; CI runs it). The lint
  levels are in the crate's `Cargo.toml` (`unsafe_code` denied).
- The full `cargo test -r -p cadrs_kernel` takes about 12 minutes; run the suites a change
  touches and the conformance suite.
- The gate, `tools/kernel_gate.sh`, makes "HEAD is green" a measured fact. It exports a clean
  tree (`git archive HEAD`, or `--rev`; `--tree DIR` tests a tree as it is) and regenerates the
  gitignored generated suites there (`OCCT_TESTS`: the test scripts' `tests/` directory; `CADRS_OCCT_DATA`). It
  checks that the library builds without features (`cargo check -p cadrs_kernel`: every test
  turns `testkit` on), that `cargo check -p cadrs_core`, the kernel's consumer, passes, and that
  `tools/kernel_clippy.sh` is clean. Then
  it builds with `--profile kernel` and runs every test binary, `RUST_TEST_THREADS` (3) at a
  time, with every kind of ignored test that can run: the known failures, `slow`, and the ones
  ignored with a reason (not the generated suites' cases waiting for a command, whose bodies are
  comments, nor debugging tools and tests that need a file, a checkout or the SMLib oracle they
  aren't given). Each case is a pass, a known failure still failing, STALE (a listed known
  failure that passes: take it off the list), STALE-IGNORE (an ignored test that passes:
  un-ignore it), a PANIC (a listed known failure that panicked instead of returning its
  error), a REGRESSION (an unlisted failure), a HANG (over `--hang`, 900 s; the binary
  goes on without it) or a CRASH; slow over 60 s is flagged. With a baseline (`--baseline`, the
  durations of an earlier run; `--write-baseline` writes one when the gate passes), a case more
  than 3× and 5 s slower than its baseline, scaled by the run's load (the median ratio of the
  durations), is a SLOW-REGRESSION. It writes `results.json` and `summary.txt` (a table per
  suite, the lists, the slowest 20 cases) to `$CARGO_TARGET_DIR/kernel_gate/` and exits 1 when
  any list or check fails. `results.json` records the commit measured: the exported revision,
  or for `--tree` the tree's git HEAD (`+dirty` with uncommitted changes), `--commit SHA` or
  `$CADRS_GATE_COMMIT`; a baseline records the commit of the run it came from
  (`kernel_gate.py --make-baseline RESULTS FILE [--commit SHA]`). `--only
  occt_blend,smlib_fillets` builds and runs some binaries; `--sample occt_boolean=4` runs every
  4th case of a binary. CI runs the checks, clippy and a fast subset of the gate on every push
  and pull request (the merge requirement: the unit tests, conformance, smlib_booleans,
  validity, determinism and every 4th runnable case of the occt_boolean suite, generated
  without the dataset), and the whole gate nightly on main and kernel-10 against the branch's
  last passing run (delete its `kernel-gate-baseline-<branch>-*` Actions cache to accept a
  slowdown).

**GPU** (spike, `crates/cadrs_gpu_kernels`, `crates/cadrs_gpu`; see `bench/gpu_spike.md` in the
working notes): kernels written in Rust compile to SPIR-V with rust-gpu (f64 included) and run
on Vulkan, 8–20× faster than rayon on the CPU for surface sampling, tessellation points, point
classification by ray parity and quadrature. Topology, booleans and intersection stay on the
CPU. Build the shaders with `cargo gpu build --shader-crate crates/cadrs_gpu_kernels
--output-dir crates/cadrs_gpu/shaders` (cargo-gpu: `cargo +nightly-2026-07-03 install
cargo-gpu --version 0.11.0 --locked`).

## Performance

Speed is benchmarked against OpenCASCADE 7.8: `tools/kernel_perf.py` times both on the same
operations and inputs and writes `PERF.md` (per case, per category, the 20 worst ratios). Both
sides read one DRAW-like case file (`examples/kernel_bench.rs` runs it on the native kernel). The
cases are the generated suites' boolean, blend, offset, pipe and draft cases that pass here,
SMLib's fillet, boolean and offset cases, meshes and mass properties of the largest SMLib
models, and STEP files of the public dataset; plus the conformance suite against the earlier
times recorded for it. Time is CPU time on one thread (the machine is shared). Geometric mean of
native / reference per category (2026-10-21; fillets 2026-10-23, after the fillet passes):

| Faster | About even | Slower |
|---|---|---|
| booleans 0.48 (occt_boolean), 0.17 (SMLib); 2D offsets and offset shapes 0.17; STEP export 0.16; mass properties 0.52 against adaptive integration to 1e-9; conformance 0.36 | meshing 0.97 | fillets about 1.1 (occt_blend) and 1.2 (SMLib), CPU times of each case run by both in turn; sweeps 2.4; drafts 1.9; STEP import 1.9; mass properties 11 against fixed Gauss rules (1e-3 off on the blade hubs) |

All 660 cases: 0.55; `PERF.md` lists the slowest. B-spline evaluation (`native::nurbs`) is
allocation-free up to degree 7, and mass properties contract a B-spline face along v once per
inner integral (`NurbsSurface::iso_v`).

## Conformance suite
The expected values come from the Onshape course exercises (see `reference/onshape/training/`):
- Control Arm: volume 368 749.705 mm³.
- Reducer Coupling: volume 53.932 in³ and surface area 248.367 in² (P3.4: built through the feature
  list in `cadrs_core/tests/revolve.rs`, both to 1e−3, and the single-revolve variant PS8.9).
- Conrod stand-in (Inspection course, P3D.4; the course's own model and its 1.149 in³ /
  16.269 in² can't be rebuilt from the few dimensions it gives): the offset web region
  2.279051 in², the pocket 0.159534 in³, the floor fillet's ΔV within 3 % of 1.7014e−3 in³, the
  repaired rod's volume equal to the directly built healthy model's to 1e−9 (1.867789 in³, mass
  0.529704 lb in Steel), all in `cadrs_core/tests/course_inspection.rs`
  (`conrod_stand_in_repairs_as_the_course_does`).
- Further primitive-level cases for each operation.

The kernel must pass the suite; a case it can't do yet fails visibly (see "Status" above).

### SMLib suites
NVIDIA's SMLib (usd-brep, Apache-2.0) has a test program, `tests/prog_test/src/*.cpp`, of
numbered cases on models in `TestFiles/`. `tools/smlib_to_rust.py <usd-brep> [suite...]` turns
them into `tests/smlib_<suite>.rs`:
- `tools/smlib/<suite>.py`, one per suite, finds the cases in its source file. It turns each
  into a call of the suite's runner, the Rust in its `PRELUDE`, and lists the blocks it leaves
  out with the reason. Suites are separate files so they can be added side by side.
- The driver splits the source into blocks (`if (lCount == N)`, `case N:`) for the suite
  modules. It copies the models into `tests/data/smlib/<suite>/` with a licence README. Cases
  listed in `tools/smlib_known_failures/<suite>.txt` (`name`, or `name slow`) must still
  fail, so one that starts to pass has to come off the list. Slow cases are `#[ignore]`d.
- With `CADRS_SMLIB_ORACLE` set to `tools/smlib_oracle`'s binary, the driver runs SMLib once per
  case. It bakes SMLib's volume, area, face and edge counts and solidity into the test. Volume
  and area must agree to a relative 1e-4. A result that SMLib itself doesn't call a manifold
  solid is not used (most of these enclose no volume). Without the oracle, a case checks
  SMLib's asserted counts and the invariants: `check()` is clean, the shells are closed, the
  Euler characteristic is possible, and the volume is positive. Face and edge counts are
  compared only when the input model is all planes.
- `tests/smlib_common/mod.rs` is shared by every suite. It reads models (`file.smb`, or
  `file.smp#k`), checks results, and finds SMLib's edges and faces in our bodies by their
  middle points and centroids. A pick that is ambiguous, or that finds nothing, panics.
  `examples/smlib_points.rs` gives the driver the same points to hand to the oracle.

`native::smb` reads every SMLib ASCII format the suites use:
- B-reps (`.smb`), with the legacy B-spline layout and the versioned (38, 42) layout of
  `Object Type` records: lines, circles, planes, cones and cylinders, surfaces of revolution
  and of extrusion.
- Parts (`.smp`: curves, surfaces, B-reps) and bare curves and surfaces (`.smc`, `.sms`).

Free curves and surfaces keep SMLib's parametrisation. Faces get the exact analytic surface.

**P3B.9 (assemblies: Check interference, assembly export).** No new `Kernel` operation: an
assembly's interference is the existing `transform` of each part's body to its placement,
then `boolean(Intersect, a, [b])` and `mass_properties(..).volume`. The case
`intersect_moved_copies` checks that chain: a 10 × 20 × 30 box and a copy turned 90° about Z and
moved by (15, 5, 10) share 10 × 10 × 20 = 2000 mm³; copies that don't meet make Intersect
**fail** ("the result is empty"), which the caller reads as no interference. `cadrs_core`
drives this on the rebuild worker (`rebuild::run_on_worker`, a job with the rebuild session, so
each Part Studio's bodies come from its cache): `Rebuilder::placed_body` (a part's body moved by
a placement, released by the caller), `common_volume` and `step_of` (several placed bodies in
one STEP file with their product names), used by `assembly::interference::{check, export_step}`.


**P3F.2 (import and export).** Two `Kernel` operations, with their types in
`cadrs_kernel::exchange`:
- `import_model(format, bytes) -> ImportedModel`: reads a STEP or IGES file.
  - Each distinct part comes once as a body, with its product name.
  - Every occurrence comes with its part, its placement (a `Motion` in the file's coordinates, mm)
    and its instance name.
  - A file without assemblies has one occurrence per part, at the identity.
- `export_model(format, name, parts, instances) -> bytes`: writes bodies with product names. With
  `instances`, it writes one assembly `name` holding the parts at those placements (STEP
  `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s). IGES writes solids as MSBO solids, so they read back as
  solids.

The native reader and writer are `native::step` and `native::iges`, sharing `native::xbuild`,
which rebuilds the files' faces into valid bodies (p-curves fitted where a file has none; faces
sewn by their edges). `tests/exchange_corpus.rs` reads every STEP and IGES file of the public dataset the generated suites use
(389 files: samples and bug-report files, many broken on purpose) and checks each part (valid
topology within the file's own tolerance, each solid's volume positive, equal to its
tessellation's within 1 % and unchanged by a move, no face dropped): 330 read clean. The other
59 are listed in the test with the first problem each hits (the commonest: p-curves off their
edges by more than a ten-thousandth of the model, loops that don't close, files with no geometry
of their own), and the test fails when a listed file reads clean or an unlisted one doesn't. The
gate runs it (it needs the dataset, so CI's per-push subset doesn't). FreeCAD reads our STEP and
IGES exports as valid solids with the same volume and area to about 1e-14.

**Mesh files** are written by the kernel: `exchange::write_stl_binary`, `write_stl_ascii` and
`write_obj` write the `TriMesh` of `tessellate` at the asked chord and angle tolerance.
`mesh_volume` gives the divergence-theorem volume Σ a·(b×c)/6, and there are readers for tests.

Conformance case `exchange_round_trips`:
- A 100 × 60 × 25 box through STEP keeps V = 150 000 and A = 20 000 (1e−6).
- A box used twice (once turned 90° about Z and moved) plus a Ø20 × 10 pin, written as an
  assembly, reads back as 2 parts named "Box" and "Pin" and 3 occurrences at the same placements
  and names (1e−9).
- IGES keeps the box's area and volume (1e−3).
- The box tessellates to 12 triangles whose mesh volume through binary STL, ASCII STL and OBJ is
  150 000 (1e−6).

`cadrs_core` uses these for the Import feature (`rebuild::kernel_ops::import`) and for
`rebuild::exchange`, the import plan and export jobs (see `essential-tips-gaps.md`, P3F.2).


## Drawing-view projection (P3C.2)
`Kernel::project(bodies, frame, opts) -> Projection` is the kernel side of drawing views
(`cadrs_drawing::view`, `cadrs_core::views`). It is a query: it makes no bodies.
- **Types** (`src/projection.rs`): `ViewFrame { origin, dir, x }` (the direction of sight,
  eye → model, and the sheet-right direction; 2D y is `x × dir`, so a front view looking along
  +Y with x = +X has y = +Z); `ProjectOptions { tolerance, hidden }`; `Projection { edges }` of
  `ProjEdge { visibility: Visible | Hidden, class: Sharp | Smooth | Outline, curve: Line | Arc |
  Polyline, points, source }`. `ProjSource { body, edge, face, edge_name, face_name }` says where
  an edge came from: the index of the body in the call, the body's `EdgeId` (sharp and smooth
  edges) or `FaceId` (a curved face's outline). The kernel fills the ids; `cadrs_core` adds the
  persistent names from the body's `BodyNames`, so P3C.3 annotations attach to model topology.
- **Native** (`native::hlr`): the bodies' edges and the exact silhouettes of their curved faces
  (cylinders and cones in closed form, spheres, others traced), split where they cross on the
  sheet (Newton on the exact curves) and at collinear overlaps; each piece is visible when a ray
  from it towards the eye meets no face (exact surfaces for planes and quadrics, the mesh
  otherwise; a ray leaving through an edge between two faces is blocked). Hidden edges under
  visible lines are dropped; hidden silhouettes are kept. Seams are not drawn.

**Section and broken-out views (P3C.8).** No new kernel operation: the cut is
made from existing ones in `cadrs_core::section` (`ViewRequest::cut`). The tool is an
`extrude` of the cut region (the whole view plus a margin for a section, the broken-out boundary
otherwise) on a plane in front of the parts, facing along the direction of sight, blind to the
cutting depth; each part is `boolean(Subtract, part, [tool])` (the part's own body is untouched:
the boolean works on a copy), named with `naming::name_body` from the boolean's history and the
part's names (op `SECTION_OP`), so edges the cut leaves alone keep their persistent names. The
temporary bodies are released after `project`. The hatch region is the planar faces of the result
whose normal is the direction of sight at the cutting depth, from `tessellate`: their triangles'
boundary, chained into loops (outer counter-clockwise, holes clockwise). Conformance case
`section_cut`: a tube Ø40/Ø20 × 30 minus the half-space box x > 0 leaves two cut faces of 600 mm²
in all, and the input body keeps its volume.

## Rebuilding the feature list (`cadrs_core`, P3.1; names since P3.2; parts and booleans since P3.3; revolves since P3.4)
**P3G.1 (linked elements; no kernel change, no new conformance case).** A reference to another
document or to a version (`cadrs_core::external`) keeps a frozen copy of the referenced element's
*features* in the consumer (`Document::linked`), never bodies: kernel bodies can't move between
kernel sessions and cadrs has no BRep read/write, so a copy is rebuilt from its features in the
consumer's own session by the ordinary `rebuild::build`, whose per-feature cache key is the
features themselves (a copy with other contents is other features and another key; the copy's
namespaced element id changes with its contents too). Names inside a copy are the source's
(persistent naming depends only on feature ids, sketch curves and regions), so mates keep
resolving across an update; instances of different copies never collide because each instance's
parts are named by the instance. Derived (P3G.4) will need a sub-build inside a host rebuild
(reentrancy, `FaceOrigin` namespacing) and its own conformance case.

**P3.10.** `FeatureKind::Draft` (`cadrs_core::draft`, `rebuild/kernel_ops/draft.rs`) and the
extrude's `draft: Option<ExtrudeDraft>` (serde default):
- The Draft feature's neutral plane is a `MirrorPlane` (a plane, a flat face or a mate
  connector); its normal (flipped by the Opposite direction) is the pull; the faces are drafted
  per part; faces that no longer exist are left out with a **warning**.
- An extrude with Draft builds each end on its own (Symmetric: two halves; a second end: the
  other way), drafts its side faces (the faces the profile's curves made) about the start plane
  with the pull along that end, joins the ends, and carries the extrude's names through (the far
  face of a second end is its start cap), so sketches on its faces keep working.
- **Warnings** (PS11.1, Onshape's yellow): `Output.warning` and `Build::warnings` (additive):
  an extrude, revolve or sweep whose regions are partly gone, a Delete part whose parts are
  partly gone, a hole whose points partly miss the merge scope, a draft whose faces are partly
  gone. The feature builds; the app shows the row amber with the reason.
- Hole (`HoleFeature.start_plane`, `.up_to`, `HoleSpec` P3.10 fields): *Start from selected
  plane* starts at the plane's crossing with the axis (inside a part the hole is buried: the
  tool starts at the plane, not 0.5 mm above it); *Up to entity* ends the full diameter at a
  plane or flat face (a curved face: its first crossing by ray), less the *Offset*; the default
  Merge scope is the part the first hole drills (`Build::contacts` of the hole, which the dialog
  puts in the field once).
- Fillet: `asymmetric`/`second`/`flip_asymmetric` → `FilletProfile::Asymmetric`; `variable`
  with `vertices` (a radius at a vertex, else the size) and `edge_points` (location 0–1, radius)
  → `fillet_variable` per part, the laws over each picked edge's tangent chain.
- Boolean: `BooleanFeature.offset: Option<BooleanOffset>` (Subtract): each tool offset (all its
  faces, or the picked faces) before it cuts, named from the tool's names; the tools' order
  gives a Union its identity (unchanged; the dialog now reorders them).
- Loft: `LoftCondition::MatchTangent`/`MatchCurvature` are built (face profiles), and a
  `LoftProfile::Face` may be any face.
- Mass properties: `parts::mass_report_with(parts, props, MassOptions { override_mass,
  reference })`; `SolidFace.area` (the kernel's exact area, for the Face tab).

**P3.8.** `FeatureKind::MateConnector`, `Pattern` and `Mirror` (`cadrs_core::mate`,
`cadrs_core::pattern`, `rebuild/kernel_ops/pattern.rs`):
- A mate connector builds no body: its frame (an origin entity's frame, its Z checked against the
  display mesh's face normal so a pocket floor's Z points out of the material, then flip,
  reorient, move and rotate) goes into `State.connectors`. Connectors serve as pattern axes
  (`AxisRef::Connector`, its Z), directions (`DirectionRef::Connector`), mirror planes (its XY
  plane) and hole places (`HoleFeature.connectors`, drilled along −Z). Implicit connectors
  (`ConnectorRef::Implicit(ConnectorOrigin)`) are computed from their entity when used.
- Patterns: the instance transforms (linear with a second direction and Centered, circular with
  Equal spacing, curve along a chain of sketch curves and edges with Tangent to curve), less the
  skipped `[i, j]` indices. **Part**: copies of the parts (New: new parts with `source` set, so
  they show the seed's appearance and material, PS9.6; else combined). **Face**: `face_tool` of
  the faces, moved, and cut or added by `classify`. **Feature**: without Reapply, the features'
  effect (the diff between the state before them and after, from `Rebuilder.trail`) moved and
  combined; with Reapply, the features computed again as synthetic features with moved sketch
  planes (`PlaneRef::Feature` frames under the same sketch ids) and their face and edge
  references remapped, then renamed per instance. `Output.dots` carries each instance's centre
  for the Skip instances dots. Mirror is the same with one reflected instance.
- **Split** (P3.8, as Onshape's dialog): `SplitType::Part | Face`; the Face type calls
  `split_faces` per part; *Keep both sides* off keeps only the pieces whose centroid is on the
  front of a flat tool (`per_part_keeping`), *Trim to face boundaries* splits with the face
  itself instead of its plane, and a surface split with is used up unless *Keep tools*.

**P3.7.** `FeatureKind::Plane`, `Sweep`, `Loft` and `Split` (`cadrs_core::plane`,
`cadrs_core::advanced`, `rebuild/kernel_ops/advanced.rs`):
- A Plane builds no body: its frame (`plane::frame`, from the entities' geometry: planes, faces,
  edges as lines, circles or curves, vertices, sketch points and curves, the origin) goes into
  `State.planes`, and sketches on it, extrude directions (`DirectionRef::PlaneNormal`) and split
  tools refer to it as `PlaneRef::Feature` (its frame is refreshed with the faces' planes,
  `parts::regenerate`).
- A sweep's path is sketch curves (a whole sketch: its non-construction curves) and part edges,
  resolved by name; a loft's sections are regions of one sketch joined into one contour, a whole
  sketch, a planar face, a sketch point or a vertex. Both make a new part or combine with the
  merge scope as an extrude does. A split replaces each part by one part per piece (the first
  keeps the part's identity).

**P3.6.** `FeatureKind::Fillet`, `Chamfer`, `Shell` and `Hole` (`cadrs_core::applied`,
`rebuild/kernel_ops/applied.rs`):
- Fillet and chamfer entities are edges (`EdgeRef`: part, persistent name, a point on it) or faces
  (all their edges but seams), resolved by name, else the edge nearest the stored point; the
  kernel op runs per part and the result keeps the part's id (its largest piece). Chamfer tangent
  propagation lists each edge's `tangent_chain`. With **Tangent propagation off** (fillet or
  chamfer), only the picked edges are blended (`FilletSpec::propagate` false). A **Full round** (`FilletType::FullRound`,
  one side face, the center face, the other side face, on one part) calls `Kernel::full_round`;
  Conic and Curvature pass `FilletProfile` to `fillet_with`.
- Shell faces (`FaceRef`) or, Hollow, parts.
- **Hole** (`cadrs_core::hole`): at each picked sketch point (or every non-construction vertex of a
  whole sketch: standalone points, line and arc ends, circle and arc centres), drilling against the
  sketch plane's normal (flip: along it). *Start from part*: the first crossing of the axis with
  the parts in scope ahead of the sketch plane (`ray_hits`); *from sketch plane*: the plane. *Through
  all*: past the far side of the scope's boxes, no drill point; *Blind*: the full diameter's depth
  with the drill point (118° by default: `r / tan(59°)` deep) beyond; *Up to next*: until the axis
  leaves the part it entered. The half section (counterbore: `(0, −0.5) → (r_c, −0.5) → (r_c, h_c)
  → (r, h_c) → (r, D) → tip`; countersink: a cone from the countersink Ø at the start) is revolved
  a full turn about the axis (its region named per point, so the hole's faces keep their names when
  points are added) and subtracted from the parts in the merge scope the tools reach (default:
  every solid part). A point whose axis meets no part in the scope gets no hole (PS15.3: a point
  over a part left out of the merge scope); none reaching one is an error. The crossings are
  found from 1 mm behind the plane, so a part face lying on the sketch plane still counts. On the
  Inch tab the depths are in inches (to 0.001 in), on the Metric tab in mm. The feature's name is
  its callout (`HoleSpec::callout`) until renamed.
- **Parents** (`Feature::parents`): the sketches, faces, edges and parts a feature refers to. A
  parent below the feature in the list (dragged above it, PS11.3) makes it fail with "Sketch 4 is
  below this feature in the list; move it back above", leaving the parts as they were. The
  rebuild cache's chain key covers only the features above, so this outcome gets a key of its own
  (judge round 1: an output cached while the sketch was missing altogether, "The selected sketch
  points no longer exist", was reused for the sketch dragged below).
- **Folders** (`ElementKind::PartStudio::folders`, `FeatureFolder { id, name, features, open }`) hold
  runs of features; `MoveFeatures` moves a feature or a whole folder and keeps folders contiguous.
  They don't take part in the rebuild.

**P3.4.** `FeatureKind::Revolve(RevolveFeature)`: regions or whole sketches (`sweep_groups`,
shared with the extrude), the axis (`AxisRef`: a sketch line or circle, a straight or circular
part edge, a curved face; resolved exactly from the sketch or the kernel's `EdgeInfo.circle` /
`FaceInfo.axis`), the type (Full, Blind, Symmetric, the "Up to" family with an offset angle), a
second end, Solid/Surface/Thin, and the same New/Add/Remove/Intersect and merge scope as the
extrude (`Merge`, one `combine` for both). `Build::axes` gives each revolve's axis to the dialog's
angle arrow.
- `brep::OpGeom::revolve` keeps the revolution (axis, start, sweep): a revolve's caps get the
  sketch's frame turned to where they are; its planar sides use the kernel's plane (the Reducer
  Coupling's end face: centred on the axis). Curved faces from lines (cones, cylinders) get
  **meridian rulings** (the profile line turned every 5°, only where the face reaches); doubly
  curved ones (an arc's torus or sphere) a **`SurfaceGrid`** (rows of points and outward normals)
  whose silhouette is found cell by cell, as it may run round the face.
- `SolidEdge.circle`: the kernel's exact circle of a circular edge; Use (`links::edge_curve`)
  projects it instead of fitting the mesh polyline.
- A whole sketch's regions ignore a face's imprinted edges (the Reducer Coupling's second flange
  on the revolve's end face is one region less its bore and holes).
- A sketch on a face is only lost if the face is gone where the sketch is in the list (a later
  feature may cover the face: the second flange covers the revolve's end face).

**P3.3.** The rebuild carries the **parts** through the feature list: each feature's cached
result is the list of parts after it (part id, kernel body, names, display mesh, mass
properties), so an edit recomputes from the first changed feature and reuses the rest.
- Extrude **New** makes one part per separate solid of its body (`split_solids`); **Add** fuses
  its body with the parts in its merge scope (a piece keeps the id of the first of them it has
  faces of; the parts it absorbed are gone); **Remove** and **Intersect** cut or intersect each
  part in scope (the largest piece keeps the part's id, the others are new parts; a part with
  nothing left is gone). The merge scope is Merge with all, an explicit list, or by default the
  parts the new body touches (Add: their fusion is one solid) or overlaps (Remove, Intersect:
  their common part has volume); `Build::contacts` gives them to the dialog for the automatic
  Add.
- The **Boolean** feature (Union, Subtract, Intersect, Keep tools) and **Delete part** work on
  the same part list. A failed feature leaves the parts as they were.
- Parts are named "Part N" ("Surface N" for surfaces) in the order they are made; renames and
  hidden parts are the Part Studio's `PartProps`, outside the feature list (so they don't
  invalidate the cache).
- `brep::solid_of` builds any body's display mesh with the swept geometry of every extrude its
  faces come from (`Geoms`): cap and side frames from the sketch (origin from the actual face,
  so they work for any end type), the kernel's plane otherwise (oblique direction, trimmed
  caps, thin walls), and silhouette rulings from the face's own mesh points, grouped along the
  sweep, so a face a boolean trimmed gets rulings only as far as it reaches.
- Sketches on faces and Use/Pierce links look for their face or edge among the parts the
  referenced feature made, then among all parts (a boolean may have joined the part to
  another).


`cadrs_core` rebuilds through the kernel with its default `brep` feature (without it, extrudes
fall back to the old prism mesh of `solid::extrude`).
- `cadrs_core::rebuild` rebuilds a Part Studio's features. Each feature's output (its kernel
  `BodyId`, display mesh, exact mass properties and error) is cached under a key that chains the
  keys of the features before it with its own parameters, so an edit recomputes from the first
  changed feature. Outputs no rebuild used in the last 48 rebuilds are evicted and their bodies
  released.
- All kernel work runs on one worker thread that owns the kernel (`DefaultKernel`) and the cache.
  `rebuild::request` returns at once; the app waits at most 30 ms per frame (`RebuildBudget`) and
  otherwise keeps showing the old parts, so a slow rebuild never freezes the UI (scripted runs
  wait, for reproducible screenshots). `rebuild::build` waits, for code that needs parts now
  (sketches on faces, links, thumbnails). A panic while rebuilding fails that rebuild and resets
  the session.
- `cadrs_core::brep` builds the `Profile` from the sketch regions' exact boundary pieces (each
  region's `source` is its `RegionRef::key`, each curve's the sketch curve id) and turns the
  kernel's tessellation into the `Solid` the app draws and picks, with faces, edges and vertices
  named by `naming` from the kernel's history (P3.2). A face the history leaves unnamed is named
  from geometry, the P3.1 way: a planar face parallel to the sketch plane at depth 0 or `depth` is
  the start or end cap of the region that contains it; a face along the extrude direction is the
  side of the boundary piece its points lie on. Planar faces get the same `PlaneFrame`s as the
  prism, so sketches on faces keep their coordinates. Unit tests check that every face is named
  from history and that the names, frames and edges match the prism mesh's for boxes, holes,
  arcs, touching regions and a half disc.
- Documents store names: `PlaneRef::Face` (`FacePlane.face: FaceName`, plus a seed point) and the
  Use/Pierce `Link::Edge { edge: EdgeName }` and `Link::Silhouette { face: FaceName }`. Schema
  version 4; version 3 files (`FaceTag`/`EdgeTag`) convert losslessly on load
  (`cadrs_sketch::legacy`, `store::migrate::from_v3`; fixture
  `crates/cadrs_core/tests/fixtures/v3_control_arm.ron`).
- Rebuild errors (kernel failures, regions that no longer exist) are in `Build::errors`; the
  feature list shows the feature in red with the message as its tooltip.
- Display tessellation: 0.05 mm and 5°.

Measured (debug build, `cadrs_core/tests/kernel_rebuild.rs`): a 20-feature studio (10 sketches,
10 extrudes of slots with holes) rebuilds from scratch in about 40 ms, and in about 4 ms after an
edit to the last extrude. With persistent naming (P3.2: the history, and naming every face, edge
and vertex) it is about 54 ms and 5 ms. The Control Arm built through the feature list with both extrudes as New
gives two parts totalling 368 749.7048 mm³.

**Windows.** `cargo build --release --target x86_64-pc-windows-gnu` cross-compiles the app with
MinGW; the kernel needs no C++ toolchain or runtime DLLs.

Conformance history (the cases as they were added): all 77 cases pass (Final: `bezier_profiles` and `fillet_smooth_corner`
added to 75). Before: all 71 cases pass (the phase3c merge of main's P3.10/P3.11, 69 cases, with
3C's `project_view` and `section_cut`). P3C.8 added `section_cut` (see "Drawing-view projection").
P3C.2 added `project_view` (a 20 mm cube with a
Ø10 hole along Z: from the front exactly two hidden edges, the hole's sides at x = 5 and 15, 20
long, sourced to the cylinder face; the visible outline 80 long, its horizontal edges sourced to
the edges nearest the eye; from above the hole a visible circle of radius 5 at (10, 10) sourced
to the top circle, the bottom one dropped behind it; no hidden edges with hidden off; a filleted
edge's boundaries are smooth edges; two bodies hide each other). Before P3C.2: 67 cases (69 with P3.11's `fillet_partial` and
`loft_direction_conditions`). P3.10 added `draft_cube_sides` (a 100³
cube's four sides drafted 5° about z = 0: the frustum h(a² + ab + b²)/3 with b = 100 − 200 tan 5°;
−5° leans out; about z = 50; a Ø20 × 20 cylinder to the cone πh(R² + Rr + r²)/3; the top face,
parallel to the neutral plane, refused), `offset_solids` (the 10 × 20 × 30 box offset 1 sharp:
12 × 22 × 32; rounded: Steiner's abc + 2(ab + bc + ca)d + π(a + b + c)d² + 4πd³/3; only the top
face by 5: 10 × 20 × 35), `fillet_variable_radius` (a law 3 → 3 → 3 is the constant R3 exactly;
2 → 4 bounded by R2's and R4's and within 1 % of (1 − π/4)·280), `fillet_asymmetric` ((1 −
π/4)·2·4 per mm both ways round), `loft_match_tangent` (Loft 2 from Loft 1's cap: its side's
normal at the joint equals the cone's, n_z = 0.25/√1.0625; None gives −n_z, Normal to profile
0; Match curvature G1 too; a sketch profile refused) and `loft_from_a_curved_face` (a half
cylinder's curved face lofted to a rectangle: 12 000 − 1000π, the curved face its cap). Before
P3.10: all 61 cases pass. P3.8 added `mirror_and_motions` (the box reflected in
x = 0: 6000 at x −10..0 with outward faces; joined to the original, 12 000; a half cylinder and its
mirror image unioned both ways round, π·500; a scaling matrix refused), `face_tools` (a pocket's 5
faces give its 1000 mm³ tool, classified outside and cut again mirrored; a boss's tool; a drilled
hole's cylinder and cone; a single face refused), `classify_points` and `split_faces_by_plane` (the
box's top face split by x = 5: two 100 mm² faces continuing it, 7 faces, V 6000; a plane that misses
is an error). Before P3.8: all 57 cases pass. P3.7 added `offset_ellipse_profiles` (a
region bounded by an ellipse's offset: the prism's volume to 5e-5 relative, the
rim's edge length L − 2πd to 1e-7),
`sweep_along_paths` (a Ø10 circle along an L of 100 and 80 with an R50 bend: π·25·(100 + 80 +
π/2·50) exactly; the profile partway along sweeping both ways; Thin; Keep profile orientation
along a straight path; a slot's top edges as the path of a bead, joined to the slot by a
union), `loft_sections_and_conditions` (equal squares: the prism 2000; two circles: the frustum
3500π/3; a circle to a point: the cone 500π; Normal to profile on equal squares is still the
prism; a bulge with larger magnitudes; three sections; two squares joined into one contour;
two separate contours refused; a square to a circle with Normal to profile, centred and between
the inscribed cylinder's and the prism's volumes; a Thin loft between equal circles, the tube) and `split_by_plane_and_face` (a box split by a plane into pieces
whose volumes add up, by a slanted face). Before P3.7: all 53 cases pass. Judge round 1 added `fillet_sections` (on the
box's 90° edge, r 3: a conic at the circle's rho sin45°/(1 + sin45°) equals the circular fillet,
(1 − π/4)·9 per mm; rho 0.5, a parabola, removes 1/3 of the contact triangle, 1.5 per mm; the
curvature section removes its quintic's area, integrated in the test; the concave corner of an L
gains the parabola's 4/6 per mm at r 2; a cylinder's rim is refused) and `full_round` (the
10 × 20 × 30 box's top rounded between its x sides: 6000 − (2 − π/2)·25·20, one R5 cylinder, no
flat top; non-parallel sides refused); `chamfer_options` now checks Tangent = Offset on a
cylinder's rim, and on a D-shaped prism's straight edge (flat side to curved side) Tangent equals
the two-distance chamfer (chord on the curved side, 2 on the flat) and differs from Offset.
P3.6 added `fillet_cube_all_edges` (a 100³ cube
with R10 on its 12 edges: 1e6 − (4 − π)·100·12·100/4 + 8[3(1 − π/4) − (1 − π/6)]·10³ =
975 587.01, 26 faces), `fillet_width` (width 3 on a 90° edge: r = 3/√2; on a 45° slope r =
3/(2 sin 22.5°), removing r²(tan(φ/2) − φ/2) per mm; a tangent chain whose angle varies from 76° to
90° removes between those rates), `fillet_overflow` (an R5 fillet next to a Ø6 hole runs into it:
refused with overflow off; away from the hole it isn't), `chamfer_options` (2 × 45° removes 2·2/2
per mm; two distances flipped trade faces; an override flips one edge; Tangent equals Offset on
planes), `shell_options` (100 × 60 × 40, bottom open: 100·60·40 −
92·52·36 = 67 776; outward with rounded edges and corners; hollow 240 000 − 92·52·32; 35 mm walls
refused), `shell_around_a_counterbore` (the region shell matches the closed form) and `edge_face_radius`. Before P3.6: all 44 cases pass. P3.5 added `inertia_tensor` (above). Before P3.5:
all 43 cases pass. P3.4 added `revolve_a_face` (a body's face turned a
quarter: π·75·5/4, sides from the body's edges), `revolve_torus` (2π²Rr², 4π²Rr, one
toroidal face with its axis), `revolve_types` (Full, 90°, flipped, Symmetric, 90° + 45°; more than a
turn, an axis across the profile and an axis normal to the plane refused), `revolve_up_to` (Up to
vertex 60°, Up to face through the axis 90° and with a 30° offset, Up to part and Up to next
conforming, errors when nothing is met), `revolve_surface_and_thin` (a cylinder sheet 2π·5·20, a
quarter sheet's free edges, a thin tube r 4..5) and `face_axes_and_edge_circles`. Before P3.4:
all 36 cases pass. P3.3 added `extrude_up_to_face_parallel`
(3000 and, with a 5 mm offset, 2500 mm³), `extrude_up_to_face_oblique` (a wedge's slanted
underside z = 30 + 0.2x over a 10 × 10 square: 3000), `extrude_up_to_part_conforms` (under a
cylinder r10 at z = 30: 10·(300 − (5√75 + 100 asin ½)) = 2043.39; Up to next finds the same;
a profile wider than the cylinder is an error), `extrude_up_to_next_from_a_face` (a hole from
a plate's top face down to its bottom, π·25·20; nothing ahead is an error; the first of two
plates stops it), `extrude_through_all` (symmetric from mid-plane, and from above),
`extrude_options` (starting offset 5 + up to vertex z 40 with offset 10: 2500; second end: 1400;
along (0, 1, 1)/√2: 1000; a direction in the plane is an error), `surface_extrude` (area 300, no
volume, free edges, a seam), `thin_extrude` (1440, 2280, an open line 600, the left side),
`split_solids_rays_and_boxes`, `extrude_a_face` and `union_merges_coplanar_faces` (a block
on a block is one box with 6 faces, its sides continuing the target's; two regions extruded
together keep their four caps). Earlier: all 26 cases pass. P3.2 added `extrude_history`,
`boolean_history_and_split_names`, `names_survive_edits`, `face_adjacency`, `tangent_chains`
(a slot's top outline is one chain of four edges, 2·30 + 2π·10 long) and `edges_and_vertices`,
and `failures_are_errors` checks the depth refusal. (P3.1 added `rectangle_100x60x25`,
`ellipse_profiles`, `failures_are_errors`, `chamfer_two_distances_and_angle` and
`tessellation_normals_and_edges`). The Control Arm (PS6), built as in the course from
a Profile with exact arcs, lines and circles (Extrude 1: hub ring, right web, right eye ring,
40 mm; Extrude 2: left web, left eye ring, 25 mm, Add), gives volume 368 749.7048 mm³ and surface
area 50 179.7110 mm² (expected 368 749.705 and 50 179.71).

**P3G.4 (Derived feature).** `FeatureKind::Derived` (`cadrs_core::derived`,
`rebuild/kernel_ops/derived.rs`). No new kernel operation:
- The source's features travel inside the feature (`DerivedFeature::studio`, with its part
  settings), so every rebuild of the host (the app's, an assembly's, a drawing's, an export's)
  builds them, and the cache key (the feature's `Debug`) changes exactly when the source does.
  Workspace references are brought up to date after every command, undo and redo
  (`derived::refresh`, called by `History`).
- The source is rebuilt **inside** the host's rebuild, in the same kernel session and cache
  (`Rebuilder::sub_build`: it saves and restores `trail`, `last`, `stage_for` and the sketch
  times, and nests at most 8 deep), because bodies can't move between kernel sessions.
- Each location's copy is `transform_motion` by the rigid motion that takes the base frame (the
  origin, or a source mate connector's frame) onto the location's frame
  (`motion_between(base, to) = L·Bᵀ, t = oₗ − L·Bᵀ·o_b`).
- **Naming** reuses `FaceOrigin::Instance { of, face, instance }` under the Derived feature's op,
  instance = copy index + 1 (`derived::derived_face`), so derived faces never collide with the
  host's own (a derived duplicate of a studio included) and a fillet on a derived edge survives
  source edits that keep the face. Parts are `PartId { feature: the Derived feature, index:
  stable_hash(source part, copy) }` (`derived::derived_part`); derived sketches, planes and
  connectors get `derived::derived_entity` ids. Derived sketches go into `State.derived_sketches`
  (placed on `PlaneRef::Feature` frames of the Derived feature) and are appended to `before`
  for later features, so an extrude takes their regions like any sketch's.
- Conformance: `derived_frame_motion` (a box's top-face centre onto a frame at (0, 0, 100) turned
  a quarter: V 6000, centroid (0, 0, 85), every face `modified` from its original).

## The kernel in its own process (`cadrs_core::rebuild::process`)
A kernel bug can take the process down (a stack overflow, say). The app therefore runs the kernel in a **process of its own**, its
own executable started with `--kernel-process <port> <token>`; a crash there costs a rebuild,
not the app. `CADRS_KERNEL=thread` keeps the kernel on a thread of the app (for debugging), and
tests and tools (`rebuild::build` without `process::use_process`) always do.
- **What runs there**: the worker loop of the kernel thread as it was (`rebuild::run`: the
  `Rebuilder`, its cache, the session snapshots). Every `rebuild::Job` is one request and one
  reply. Closures can't be sent, so kernel work other than rebuilds, exports and drawing views
  is a named `rebuild::task::Task` (section caps, interference, import plans, model exports, the
  stage preview, Full rebuild, PCB Sync, ...); tasks of other crates are registered at start
  (`cadrs_app::register_kernel_tasks`), in the app and the kernel process alike.
- **The connection**: a loopback TCP socket the app listens on (not stdout, which libraries may
  write to), checked with a random token. Messages are MessagePack with named fields
  (`rmp-serde`; the documents' optional fields are skipped when empty, which bincode can't
  read), each after its length. A rebuild sends each part's mesh once: one sent before goes as
  its id, and the app hands out the `Arc` it already holds (so an edit costs what it did on the
  thread, and the app's pointer checks still see unchanged parts as unchanged).
- **Crashes**: the kernel process reports the cache key of the feature it is computing; if the
  connection closes during a rebuild, that key is *poisoned*, a new kernel process is started
  (it restores the session snapshot as the app's next start would) and the rebuild is asked
  again. The feature fails with "The geometry kernel crashed while building this feature"; the
  rest of the list builds. Changing the feature or anything above it gives it a new key.
- **Shared state**: the app sends the files of its blob cache the kernel process lacks, and the
  kernel process sends the ones it makes (a board's skin); the documents and snapshot folders
  are sent when set (`process::set_documents`, `set_snapshots`). A Sheet metal model's
  `picking` (never saved, so not serialized) travels beside the feature list.
- Tests: `cadrs_core/tests/kernel_process.rs` (its own executable is the kernel process; a
  feature made to crash fails alone, exports, tasks and blobs go through), and
  `process::tests` (every fixture's feature lists and a rebuild's result survive the trip).
  `CADRS_KERNEL_CRASH_ON=<feature name>` makes a kernel process crash on that feature.
