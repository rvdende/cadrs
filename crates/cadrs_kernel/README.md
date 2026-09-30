# cadrs_kernel: solid-modeling layer

## Why a crate of our own
Part Studio features need a B-rep kernel: exact solids with faces, edges and curved surfaces,
plus booleans, fillets, shells and so on. We keep that kernel **behind our own crate** so that:
- the rest of cadrs (`cadrs_core`, `cadrs_app`) never touches a kernel's types;
- backends can be swapped or combined, e.g. OpenCascade for robustness and a pure-Rust kernel for
  WASM, or our own code for a single operation;
- we can add operations a backend lacks (for example mass properties on monstertruck) in one place;
- **persistent naming** (stable face and edge IDs across rebuilds) is ours, not the backend's.

## Shape of the crate
```
crates/cadrs_kernel/
  src/lib.rs          # public API: types, Kernel trait, errors
  src/types.rs        # Profile, Curve2, Plane, Frame, MassProperties, TriMesh, ids
  src/naming.rs       # persistent naming: FaceName/EdgeName/VertexName from op history (P3.2)
  src/projection.rs   # drawing-view projection types and source matching (P3C.2)
  src/backend/occt.rs         # feature "occt": fork of opencascade-rs (implemented)
  src/backend/monstertruck.rs # feature "monstertruck" (not planned for now)
  tests/conformance.rs        # the same suite for every enabled backend (implemented)
```
There's no Bevy dependency. `cadrs_core` depends on `cadrs_kernel`, and features rebuild through it
(see "Rebuilding the feature list" below).

## API sketch
```rust
pub struct BodyId;   // a solid/body owned by the kernel session
pub struct FaceId;   // an index into one body; FaceName is the persistent name (see naming)
pub struct EdgeId;

pub trait Kernel {
    // creation
    fn extrude(&mut self, profile: &Profile, extent: Extent) -> Result<OpResult>;
    fn revolve(&mut self, profile: &Profile, axis: Axis, angle: Angle) -> Result<OpResult>;
    fn sweep(&mut self, profile: &Profile, path: &Path3) -> Result<OpResult>;
    fn loft(&mut self, sections: &[Profile], opts: LoftOpts) -> Result<OpResult>;
    // combination and editing
    fn boolean(&mut self, op: BoolOp, target: BodyId, tools: &[BodyId]) -> Result<OpResult>;
    fn fillet(&mut self, body: BodyId, edges: &[EdgeId], radius: f64) -> Result<OpResult>;
    fn chamfer(&mut self, body: BodyId, edges: &[EdgeId], spec: ChamferSpec) -> Result<OpResult>;
    fn shell(&mut self, body: BodyId, remove: &[FaceId], thickness: f64) -> Result<OpResult>;
    fn transform(&mut self, body: BodyId, t: Isometry3) -> Result<OpResult>; // patterns, mirror
    // queries
    fn faces(&self, body: BodyId) -> Vec<FaceInfo>;   // id, surface kind, plane if planar
    fn edges(&self, body: BodyId) -> Vec<EdgeInfo>;
    fn mass_properties(&self, body: BodyId) -> MassProperties; // exact where possible
    fn tessellate(&self, body: BodyId, tol: Tolerance) -> TriMesh; // with face and edge ids
    fn export_step(&self, bodies: &[BodyId]) -> Result<Vec<u8>>;
}

pub struct OpResult {
    pub bodies: Vec<BodyId>,
    pub history: History, // generated/modified/deleted sub-shapes → feeds naming
}
```
- `Profile` is built from `cadrs_sketch` regions using **exact** boundary curves (lines, arcs,
  circles, ellipses, later splines) on a sketch `Plane`, never the polyline used for display.
- Hole, pattern, mirror and draft are built on top of these operations in `cadrs_core`, unless a
  backend provides a better native version.

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

## Backends
| Backend | Status | Notes |
|---|---|---|
| `occt` | **chosen primary, implemented** (`backend::occt::OcctKernel`) | Fork of `bschwind/opencascade-rs`, local at `~/work/opencascade-rs`, branch `main` (the `cadrs` branch was merged into it on 2026-09-30). It builds OCCT 7.8.1 from source (about 6 min cold). Our fork adds `Shape::mass_properties()`. Spike (2026-09-26): the Control Arm volume is 368 749.705 mm³, exact to 0.0002, and its area 50 179.711 mm² matches the course self-check. STEP export works. See "OCCT backend" below. |
| `monstertruck` | rejected as the primary | Pure Rust (Apache-2.0), 0.4.1. Spike (2026-09-26): every boolean in the Control Arm failed (`EmptyOutputShell`) on coplanar caps, on tangent contact, and where a plane cuts a cylinder wall. Its fillet silently did nothing on curved edges, and one fillet left an open shell. There are no exact mass properties, and no chamfer, shell or draft. Tessellation and STEP export are good, so it may be useful later for a viewer or WASM. |
| `cgalrs` | helper | `~/work/cgalrs`: pure-Rust CGAL port for mesh and computational-geometry utilities (triangulation, spatial queries), not a B-rep kernel. |

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

Each enabled backend must pass the suite. Failures are recorded per backend, so a gap in one
backend is visible instead of silent.

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

**P3F.2 (import and export).** Two new `Kernel` operations, with their types in
`cadrs_kernel::exchange`:
- `import_model(format, bytes) -> ImportedModel`: reads a STEP or IGES file.
  - Each distinct part comes once as a body, with its product name.
  - Every occurrence comes with its part, its placement (a `Motion` in the file's coordinates, mm)
    and its instance name.
  - A file without assemblies has one occurrence per part, at the identity.
  - Invalid shapes are healed with OCCT's `ShapeFix_Shape` (kept only if the healed shape is
    valid).
- `export_model(format, name, parts, instances) -> bytes`: writes bodies with product names. With
  `instances`, it writes one assembly `name` holding the parts at those placements (STEP
  `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s). IGES writes solids as MSBO solids
  (`write.iges.brep.mode = 1`), so they read back as solids.

The OCCT backend (`backend/occt_exchange.rs`) goes through the fork's XDE bindings: `opencascade::xde`
and `opencascade_sys::cadrs_xde`, fork commits `277175f` and `a475208`.
- **Reading:** `STEPCAFControl_Reader` / `IGESCAFControl_Reader` into an XCAF document in mm. The
  free shapes are walked: assemblies' components are followed with their locations composed, and
  each non-assembly label is a part, listed once.
- **Writing:** an XCAF document of parts (`AddShape`, `TDataStd_Name`) and one assembly
  (`NewShape`, `AddComponent`), written by `STEPCAFControl_Writer` / `IGESCAFControl_Writer`.
- Linking adds `TKVCAF`, `TKV3d`, `TKService` and `TKCDF` (and `gdi32`, `advapi32`, `ole32` and `windowscodecs` on Windows; the MinGW cross-link of the conformance tests was checked).
- OCCT reads and writes files and keeps global state, so calls go through a temporary file under a
  mutex.

**Mesh files are ours**, not a backend's: `exchange::write_stl_binary`, `write_stl_ascii` and
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

## OCCT backend
Build and test it with the `occt` feature. `cadrs_kernel` has no default features, so the rest of
the workspace does not compile OCCT:
```
cargo build -p cadrs_kernel --features occt   # ~6.5 min cold (OCCT 7.8.1 from source), then seconds
cargo test  -p cadrs_kernel --features occt   # conformance suite, 58 cases
```
The dependency is `opencascade` from `github.com/rvdende/opencascade-rs`, branch `main`
(declared in the root `[workspace.dependencies]`).

Status (2026-09-27, P3.1; P3.3 and P3.4 rows marked):
| Operation | Status |
|---|---|
| extrude | Blind (±), Symmetric (total depth), TwoSided. One planar face per region with exact lines, arcs, circles, ellipses and ellipse arcs; holes as inner wires. Each loop is built through shared joint points, so OCCT sees a connected wire. Loops are reoriented (outer CCW, holes CW), so sketch loop direction doesn't matter. Several regions are fused into one body (the caps stay split along the regions' shared curves). |
| revolve | Any angle up to ±2π. |
| revolve_with (P3.4) | The full revolve (`RevolveSpec`): Solid, Surface, Thin; Full, an angle, Symmetric, Up to next / face / part / vertex with an offset angle, a second end. See "The full revolve" below. |
| extrude_with (P3.3) | The full extrude (`ExtrudeSpec`): Solid, Surface, Thin; Blind, Up to next / face / part / vertex with an offset, Through all; Symmetric; a second end; a starting offset; any direction out of the plane; planar faces of bodies as input. See "The full extrude" below. |
| boolean | Union, Subtract, Intersect; several tools are applied in turn. |
| split_solids (P3.3) | One body per solid (`TopExp` solids of the result; each face `modified` from the same face of the input). |
| compound (P3H.6) | Copies of several bodies gathered into one `TopoDS_Compound`, not merged (Composite part): faces in input order, each `modified` from its input face; volume and area are the sums. Conformance case `compound_gathers_bodies` (a 10 × 20 × 30 box and a 10 × 10 × 5 plate on it: 6500, 12 faces, 2 solids). |
| solid_count, bounding_box, ray_hits (P3.3) | Solids in a body; a tight `Bnd_Box` (`BRepBndLib::AddOptimal`, exact geometry); every crossing of a line with the faces (`BRepIntCurveSurface_Inter`), sorted by distance. |
| transform | Rotation then translation (`Isometry3`). |
| fillet | Constant radius. |
| fillet_with (P3.6) | Radius or constant **Width** (a radius per sample along each edge from the faces' angle), the edges' tangent chains, **Allow edge overflow** off refusing a fillet that runs onto another face; **Conic** (Rho) and **Curvature** (G2, Magnitude) sections on straight edges between flat faces. See "Applied features" below. |
| full_round (P3.6) | Onshape's Full round: a flat rectangular middle face replaced by a half-cylinder tangent to two parallel flat side faces (r = half their distance); anything else is an `InvalidParameter` error saying what it needs. |
| chamfer | `EqualDistance`, `TwoDistances` and `DistanceAngle` (the first distance on the edge's first face in explorer order). |
| chamfer_with (P3.6) | The same types, **Offset** or **Tangent** measurement, the opposite direction for all edges and per-edge **Direction overrides**. |
| shell | Inward (`BRepOffsetAPI_MakeThickSolid`). |
| shell_with (P3.6) | Faces removed or **Hollow**, inward or **outward**; walls that would cross are an error (`InvalidParameter`), never a wrong body; an inward shell OCCT can't build is built from its definition. |
| faces | Area exact. `SurfaceKind` from `BRepAdaptor_Surface::GetType` (Plane, Cylinder, Cone, Sphere, Torus, Other). P3.4: `axis` of a cylinder, cone, sphere, torus or surface of revolution (fork `Shape::face_axes`). |
| edges | Adjacent faces; length exact for lines and circular arcs, polyline length otherwise. P3.3: an edge with one face is a seam when the face runs along it twice (`faces: [f, f]`), else a free edge of a surface (`faces: [f, None]`). P3.4: `circle` (center, plane normal, radius) of a circular edge or arc (fork `Shape::edge_circles`). |
| mass_properties | Exact (`BRepGProp`). A surface body (no solids) has volume 0 and its area centroid. P3.5: the inertia tensor at unit density about the centroid (`GProp_GProps::MatrixOfInertia`, fork `7278a45`); zero for a surface body. |
| tessellate | `Tessellation { deflection, angle }` (BRepMesh, parallel over faces). Positions, outward normals, counter-clockwise indices, per-triangle `FaceId`, and edge polylines made of the mesh's own points (so drawn edges sit on the triangles). |
| release | Drops a body (the rebuild cache evicts old feature outputs). |
| export_step | Writes through a temp file. OCCT prints a line to stdout per write. |
| sweep_with (P3.7) | A profile (regions, open chains or planar faces) along a 3D path of sketch curves and body edges (`PathCurve`), chained end to end; Solid, Surface, Thin; profile control None (corrected Frenet), Keep profile orientation (fixed), Lock profile direction (binormal). See "Sweep, loft, split" below. |
| loft_with (P3.7) | Ordered sections (profiles, planar faces, a point first or last); start and end conditions None, Normal to profile, Tangent to profile, with magnitudes; Solid, Surface, Thin. |
| split (P3.7) | A body split by a plane, a face (a planar face by its whole plane) or another body (`BRepAlgoAPI_Splitter`), one body holding the pieces. |
| transform_motion (P3.8) | A copy moved by a `Motion` (x ↦ M·x + t, M orthonormal: a rotation or a **reflection**); `BRepBuilderAPI_Transform` with copy, so a mirrored body is a valid solid with outward faces. A matrix that scales or shears is refused. |
| face_tool (P3.8) | The solid the picked faces bound with flat caps across their free-edge loops (sewing, `ShapeAnalysis_FreeBounds::ConnectEdgesToWires`, planar `MakeFace`, `MakeSolid`, `OrientClosedSolid`): a pocket's or a boss's volume, which a Face pattern or Face mirror copies and cuts or adds. Caps are `StartCap { region: source }`; a tool under 1e-9 mm³ is an error. |
| classify (P3.8) | Where a point is relative to a solid (`BRepClass3d_SolidClassifier`): Inside, Outside, OnBoundary. A face tool whose centroid is outside its body is a pocket (cut), inside a boss (added). |
| split_faces (P3.8) | The picked faces cut where a tool (plane, face, sheet) crosses them (`BRepAlgoAPI_Section` for the curves, `BRepFeat_SplitShape`): still one body, each split face `modified` into its pieces. The Split feature's Face type. |
| project (P3C.2) | Hidden-line removal for drawing views: bodies seen along a `ViewFrame`, as 2D `ProjEdge`s (visible or hidden; sharp, smooth or outline; line, arc or polyline), each with its source body edge or face. Fork `opencascade::hlr` (`HLRBRep_Algo`, exact), fork commit `9c4bc31`. See "Drawing-view projection" below. |

| draft (P3.10) | Faces turned about the neutral plane for a pull direction (`BRepOffsetAPI_DraftAngle`, fork `Shape::try_draft_h`); planes, cylinders and cones. |
| offset (P3.10) | A solid's faces moved outward by a distance, or each by its own (`BRepOffset_MakeOffset`, skin mode, intersection joins for sharp edges; fork `Shape::try_offset_h`). |
| fillet_variable (P3.10) | A `(t, r)` law per edge (the P3.6 `Shape::try_fillet_variable_h`). |

**Exceptions.** Every call the backend makes that can fail goes through the fork's
`opencascade::safe` API (`opencascade-sys/include/cadrs_safe.hxx`). Those functions return cxx
`Result`s, and the header defines a `rust::behavior::trycatch` that also catches OCCT's
`Standard_Failure` (which is not a `std::exception`, so cxx's default would call
`std::terminate`). A failed wire, face, prism, boolean, fillet, chamfer, shell or mesh is now an
`OperationFailed` error with OCCT's message; `failures_are_errors` in the conformance suite
checks it.

**Trait changes in Final** (phase 3 Final; all additive, defaults `Unsupported`).
- `Curve2::Bezier { poles: [Point2; 4], source }`: a sketch's cubic Bézier curve (S12.14) in a
  profile, built as an exact `Geom_BezierCurve` edge (`Edge::try_bezier`, no fork change); its
  loop area by three-point Gauss–Legendre (the integrand is a quintic, so it's exact). Thin
  walls along one are refused. Conformance `bezier_profiles` (the region under the Bézier
  (0,0) (0,10) (20,10) (20,0) closed by its chord is exactly 120 mm², so 600 mm³ at 5 mm, either
  way round; as a hole in a square, 8000 − 600).
- `fillet_smooth(body, edges, &FilletSpec, setback)`: Onshape's **Smooth fillet corners**
  (PS14.6). See "Smooth fillet corners" below. Conformance `fillet_smooth_corner`.
- `face_normals_at(body, face, points)`: a face's exact outward normals at the points nearest
  those given (fork `Shape::face_derivatives`), for checking that faces meet tangentially.

**Smooth fillet corners** (Final, `backend/occt_smooth.rs`; fork commit `f65c3e1`, pushed to the
`cadrs` branch). ChFi3d closes a corner where three fillets meet with its own patch and has no
setback, so the smooth corner is built on the filleted solid afterwards: at every vertex of the
input where three or more filleted edges (with their tangent chains) meet, a ball of radius
`ρ = √(setback² + r²)` about the vertex is cut away (its seam and poles turned away from the
material), which leaves one spherical face where the corner was, bounded by the section of
every face round it (on a cube corner: three flat faces and three fillets, six edges). That
face is replaced by an N-sided filling through the same edges, G1 to each neighbouring face
(fork `Shape::try_fill_face`, `BRepOffsetAPI_MakeFilling` with each edge's other face as its
support), and the faces are sewn back into an outward solid; each output face is traced back
through the sewing to the face it continues, the patch generated `FromVertex`. The app sets the
setback to 1.5 × the radius (the fillets' contact lines end there). Measured on the conformance
cube's corner (R3, setback 4.5): the patch meets its six neighbours within 0.37° (the worst at
the hole's corners), the filling's own G1 error 0.02°. Findings: MakeFilling refuses G2 against
these supports ("the continuity is not G0 G1 or G2"), so the corner is G1; its default
approximation (degree ≤ 8 in ≤ 9 segments) leaves 0.95° at the hole's corners, so the backend
asks for up to degree 10 in 30 segments; degree 4 or 5 surfaces meet their constraints but bulge
far out of the corner (a 27 543 mm³ cube), so a patch that leaves the ball is refused
("gave a patch that bulges out of the corner"), as are a non-circular or width fillet and a
corner OCCT can't cut or fill (each an `OperationFailed` error naming the corner). Where no three
filleted edges meet it is the plain fillet. None of the course parts has such a corner (probed:
every fillet of the fixtures with the option on builds the same volume).

**Trait changes in P3.10** (all additive; defaults `Unsupported`).
- `draft(body, &DraftSpec { faces, angle, pull, neutral, tangent_propagation })`: a positive
  angle leans the faces in towards the material as they run along `pull` from the neutral
  plane (through `neutral`, normal `pull`). OCCT's `BRepOffsetAPI_DraftAngle` has the same sign
  (pinned by `draft_cube_sides`). Its `Flag = false` (no tangent propagation) can crash OCCT on
  a face with a seam or a tangent neighbour, so the backend always propagates, and without
  Tangent propagation refuses a pick with an unpicked tangent neighbour (as the fillet does).
  Every result is checked with `BRepCheck_Analyzer`.
- `offset(body, &OffsetSpec { distance, faces: Vec<(FaceId, f64)>, sharp })`: the Boolean's
  Subtract offset. The result must be one valid solid whose volume moved the right way (OCCT's
  offset can fail quietly with an inside-out solid); a zero offset is a copy.
- `fillet_variable(body, &[FilletLaw { edge, radii: Vec<(t, r)> }], allow_overflow)`: every edge
  tangent to a listed edge must be listed too; laws with more than one point are extended to
  `t = 0` and `1`. OCCT interpolates the law smoothly (`Law_Interpol`); a linear law is sampled
  finely by `cadrs_core` (8 points per span) when Smooth transition is off.
- `FilletProfile::Asymmetric { second, flip }`: built with the conic sections of P3.6 (straight
  edges between flat faces): contact lines at the circular fillet's for the size on the edge's
  first face and for `second` on the other (`flip` swaps them), the rational quadratic with the
  circle's middle weight `sin(θ/2)`: an affine image of the circular fillet, which at 90° is the
  quarter ellipse with those semi-axes (`fillet_asymmetric`: (1 − π/4)·2·4 per mm).
- `LoftCondition::MatchTangent`, `MatchCurvature` (face sections only): at every sample of the
  section the direction in the neighbouring face's tangent plane square to the boundary
  (`t × n` of the section's tangent and the neighbour's outward normal, leaving the first face
  away from its body and arriving into the last), `magnitude × L` long; Match curvature also
  sets the second derivative to `κ(d) L² n`, `κ(d)` the neighbour's normal curvature in that
  direction from its first and second fundamental forms (fork `Shape::face_derivatives`,
  projected with `ShapeAnalysis_Surface::ValueOfUV`, which also finds points on a face's
  boundary where `GeomAPI_ProjectPointOnSurf` finds none). With curvature the columns are
  quintic Hermite Béziers, so Match curvature takes exactly two profiles.
- `LoftSection::Face` may be non-planar (PS20.1): without end conditions `ThruSections` runs
  through its outer wire as a shell and the face itself is sewn on as the cap (fork
  `Shape::try_sew_solid`); with conditions the face's boundary is sampled densely (4096 chords
  per curved edge, so the sampled loop is within a few µm of the edge) and every face end —
  planar too — is capped by the face itself, sewn within 1e-4 mm (a flat cap made from the
  interpolated section would lie a hair off the face, and an Add onto the face's part would
  leave slivers that don't mesh).
- `cadrs_kernel::loft::SectionCurves` gained `polylines` (3D pieces in loop order, for face
  sections) and the constructors `new` and `sampled`.

**Trait changes in P3.11** (additive; no fork change).
- Fix round 1: `fillet_partial(body, edge, &FilletSpec, from, to)` (default `Unsupported`), the
  partial fillet below.
- `LoftCondition::NormalDirection([f64; 3])` and `TangentDirection([f64; 3])` (PS20.4): Normal to
  profile and Tangent to profile with a picked vector in place of the section's normal (the
  vector is normalized and turned to point along the loft: the same way as the section's normal,
  or, square to it, from the section towards its neighbour); a zero vector is an error.
  `LoftCondition` is no longer `Eq` (it holds floats). Conformance `loft_direction_conditions`.
- `draft`: `BRepOffsetAPI_DraftAngle`'s history reports the drafted faces as deleted, so their
  persistent names were lost (a later feature on a drafted face, or the dialog's tint of the
  faces to draft, found nothing). The backend now carries each lost input face to the new face
  nearest its centroid that the history doesn't account for, of the same surface kind and with a
  normal within the draft angle of its own (core test `draft_feature_on_a_cube`).

**P3.10 fork commit** (branch `cadrs`, pushed to `origin`): `f7d8eba`: `Shape::try_draft_h`
(`BRepOffsetAPI_DraftAngle`), `Shape::try_offset_h` (`BRepOffset_MakeOffset` with per-face
offsets), `Shape::face_derivatives` (point, outward normal, D1U, D1V, D2U, D2V, D2UV) with
`FaceDerivatives::normal_curvature`, `Shape::try_sew_solid`, and `LoftDerivative::Samples` in
`try_loft_solid` (a vector per sample, optionally a second derivative: quintic columns).

**Partial fillet** (P3.11 fix round 1, PS14.6; `Kernel::fillet_partial(body, edge, &FilletSpec,
from, to)`, `backend/occt_partial.rs`). `BRepFilletAPI_MakeFillet` always runs a contour to the
ends of its edges (and on along tangent edges), and splitting the edge first doesn't stop it: the
pieces are tangent, so the contour runs on across the split (tried in P3.10). So the partial
fillet is built from the whole edge's fillet: what that fillet removes (`body − filleted`, a
convex edge) or adds (`filleted − body`, a concave one) is cut down to the part between the
planes square to the edge at the two bounds (each plane a large half-space block, intersected),
and only that part is cut from or added to the body (`apply_tools`). The fillet face is the whole
fillet's face between the bounds and each bound gets a flat end face square to the edge, as
Onshape's partial fillet ends; the section and so the volume are exactly the whole fillet's
there: (1 − π/4)·R²·ℓ on a 90° straight edge (conformance `fillet_partial`: 0.25–0.75 of the
box's 30 mm edge, R2; the whole range equals the whole fillet; a circular rim's half removes half
the round's volume; a concave edge adds the same section). A curved edge is taken in pieces that
each turn less than 60°, so each piece lies in the wedge between its planes. At a bound of 0 or 1
whose vertex has no tangent neighbour, the plane moves out past the vertex (4 × the size) so the
fillet ends there as the whole fillet does. History: the fillet face is generated `FromEdge`, the
end faces `StartCap`/`EndCap { region: 0 }`. Any profile of `fillet_with` works (asymmetric,
conic, width).

**What OCCT can't do here** (P3.10, PS14.6):
- *Smooth fillet corners*: built in the Final stage (see "Smooth fillet corners" above). The
  P3.11 investigation stands for OCCT's own fillet: ChFi3d builds the corner as the exact sphere
  or a `GeomFill_ConstrainedFilling` patch between the fillets' ends, and `SetParams`,
  `SetContinuity` and `SetFilletShape` don't move where the fillets end; hence the cut-and-fill
  corner builder.
- The variable fillet's *Magnitude* belongs to Onshape's Curvature cross section: out of scope
  (niche; out of scope by user decision 2026-09-29), with the full round/conic/curvature options
  beyond P3.6.
- Asymmetric and variable fillets only with the Distance control; asymmetric only on straight
  edges between flat faces (as Conic and Curvature). A variable fillet's radius law is exact at
  its points; between them OCCT's evolving surface removes up to ~1 % more than circular
  sections of the interpolated radius would (`fillet_variable_radius`: 60.619 against 60.089
  mm³ for 2 → 4 mm over 30 mm).
- The Draft feature's *Parting line* type (a draft about a parting curve with a split): not
  built; `DraftAngle` drafts about a plane only.

**Trait changes in P3.8.**
- `transform_motion(body, &Motion)`: `Motion { linear: Matrix3, translation: Vector3 }` with
  `identity`, `translation`, `rotation(&Axis, angle)`, `reflection(point, normal)`,
  `from_isometry`, `then`, `inverse`, `point`, `vector`, `is_reflection`, `to_isometry`. The
  old `transform(Isometry3)` stays (it can't mirror).
- `face_tool(body, &[FaceId], source) -> OpResult`, `classify(body, Point3, tol) -> PointClass
  { Inside, Outside, OnBoundary }` and `split_faces(body, &[FaceId], &SplitTool) -> OpResult`.
  All default to `Unsupported`.
- Naming: `FaceOrigin::Instance { of: OpId, face: u64, instance: u32 }` (`face_bytes` tag 5,
  `naming::face_hash`): the face of a pattern or mirror copy, named after the op and face it
  copies and the instance's index, so faces of different instances never collide.
- **Unions and UnifySameDomain.** `try_union_clean_h` measures the fused volume before merging
  coplanar faces; if the merged shape's volume differs (UnifySameDomain edits the fused shape in
  place and can drop a mirrored half cylinder's faces: π·500 came out as 523.6), the union is
  made again without merging. The price: a mirrored part joined with Add keeps the seam faces
  along the mirror plane.

**P3.8 fork commits** (branch `cadrs`, pushed to `origin` only): `c8cfc22` (`try_transform_h`,
`try_face_tool_h`, `classify`), `36b5a3b` and `05806b0` (the union's volume check and the
unmerged fallback), `86000a6` (the final union path, after dropping the `ShapeCustom::DirectFaces`
reflections of `f308296`, which didn't help and made later booleans hang, and the input swap of
`86f8715`), `506f409` (`try_split_faces_h`).

**Trait changes in P3.7.**
- `sweep(profile, &[Curve2])` and `loft(&[Profile])` (defaults, never implemented) are replaced
  by `sweep_with(&Profile, &SweepSpec { body, path: Vec<PathCurve>, control: SweepControl, faces })`
  and `loft_with(&LoftSpec { body, sections: Vec<LoftSection>, start: LoftEnd, end: LoftEnd,
  source })` (`LoftSection::{Profile, Face(FaceInput), Point}`, `LoftEnd { condition:
  LoftCondition::{Default, NormalToProfile, TangentToProfile}, magnitude }`), and
  `split(body, &SplitTool::{Plane, Face { body, face }, Body}, source)` is added. All three
  default to `Unsupported`.
- `Curve2::OffsetEllipseArc { center, major_radius, minor_radius, rotation, start, sweep, offset,
  source }`: the exact offset of an ellipse (PS21.2, X13). It is not an ellipse; the backend
  approximates it by a C2 B-spline to 1e-8 (`GeomConvert_ApproxCurve` of a `Geom_OffsetCurve`),
  and refuses an inward offset past the smallest radius of curvature (b²/a). `Curve2::is_closed`.
- New module `cadrs_kernel::loft` (backend-free): section loops (a region's pieces joined into
  one closed contour, else `InvalidProfile("one closed contour")`), orientation and start
  alignment, sampling.

**Sweep, loft, split (P3.7, `backend/occt_sweep.rs`).**
- The path's edges are chained end to end (joins within 1e-5 mm; edges shorter than 1e-4 mm, a
  fillet's sliver where two faces of a rim meet, are left out; `cadrs_core::brep` also leaves
  such slivers out of a solid's edges and joins the tangent chains across them, so Create
  selection → Tangent connected finds the funnel's 8 edges). A profile whose plane crosses
  the path partway sweeps both ways (two spines, the backward one's caps swapped); on a closed
  path the spine restarts at the crossing. Open paths use `BRepOffsetAPI_MakePipe`, closed ones
  and Lock profile direction `BRepOffsetAPI_MakePipeShell` (`MakePipe` on a closed path leaves two
  coincident caps inside the solid). Thin sweeps the thin profile (`thin::thin_profile`).
- A loft without end conditions is `BRepOffsetAPI_ThruSections` (ruled off, faces tagged from its
  history). With conditions, cadrs builds the surface itself (`cadrs_loft_solid` in the fork):
  each section is sampled at the same parameters, every column is interpolated through the
  sections (`GeomAPI_Interpolate`, parameters by the centroids' chord length, normalized) with the
  end derivative = magnitude × total chord length × the section plane's normal (Normal) or the
  radial direction in it (Tangent), then caps are added, sewn and made a solid. With this
  convention the course's funnel (magnitudes 0.5 and 1) is within 0.04 % of Onshape's volume.
  Profiles with different numbers of edges (PS20.3, judge round 1): the profile with the most
  edges gives the corners; every other one is cut where its direction from its centre (about the
  loft's axis) is nearest each corner's, and each piece is resampled by arc length (a square's
  sides meet a circle's quarters).
- A whole offset-ellipse loop is split into two half edges before a face is made from it: a
  closed non-periodic B-spline seam made the union of the funnel's rim and its shelled loft
  produce a bogus face (fuzzy, glue, ShapeFix and periodic B-splines didn't help).

**P3.7 fork additions** (commit `08628ab` on `cadrs`): `Edge::try_offset_ellipse`,
`Edge::try_split_at`, `Edge::try_reversed`, `Edge::edge_samples`, `Shape::try_vertex`,
`Wire::try_pipe_h` (`MakePipe`, modes corrected Frenet, fixed, Frenet, discrete),
`Wire::try_pipe_shell_h` (`MakePipeShell`, also binormal; `MakeSolid`), `try_thru_sections_h`,
`try_loft_solid` (the interpolated loft above) and `try_split_h` (`BRepAlgoAPI_Splitter`), each
with its history.

**Trait changes in P3.6.**
- `fillet_with(body, edges, &FilletSpec { size: FilletSize::Radius(r) | Width(w), allow_overflow })`
  (default: a radius with overflow maps onto `fillet`).
- `chamfer_with(body, edges, &ChamferOpts { spec: ChamferSpec, measurement: ChamferMeasure::Offset
  | Tangent, flip, flipped: Vec<EdgeId> })` (default: Offset without flips maps onto `chamfer`).
- `shell_with(body, &ShellSpec { remove, thickness, outward, hollow })` (default: an inward shell
  maps onto `shell`).
- `FaceInfo.radius: Option<f64>` (serde default): a cylinder's or sphere's radius (a cone's
  reference radius, a torus's major one), from the fork's `face_axes`.
- (Judge round 1) `FilletSpec.profile: FilletProfile::{Circular, Conic { rho }, Curvature {
  magnitude }}` (serde default Circular), and `full_round(body, side1, center, side2)` (default
  `Unsupported`).

**Applied features (P3.6, `backend/occt.rs`).**
- **Fillet.** Every edge's tangent chain is filleted with it (OCCT's fillet contours always
  continue along G1 edges, so the chain is listed explicitly and each edge gets its own radii).
  *Width* `w`: at 9 samples along an edge the fork's `Shape::edge_normals` gives the two faces'
  outward normals, `φ` apart; the fillet's arc then spans `φ` and its chord is `2r sin(φ/2)`, so
  `r = w / (2 sin(φ/2))`; an edge whose angle doesn't change gets one radius, others a law through
  the samples (`BRepFilletAPI_MakeFillet::SetRadius(UandR, IC, IinC)`, fork
  `Shape::try_fillet_variable_h`). *Allow edge overflow* off: the result is refused when a fillet
  face touches a face that continues an input face other than the ones meeting at its edge or at
  its edge's end vertices (checked on OCCT's history and the result's edge–face map). OCCT has no
  Parasolid-style overflow: where Onshape would spill a fillet onto a neighbour, OCCT either
  trims it against the neighbour (the overflow the check catches, e.g. a fillet running into a
  hole next to its edge) or fails.
- **Fillet sections** (Conic, Curvature). `BRepFilletAPI_MakeFillet` only rolls a ball: its
  `ChFi3d_FilletShape` (Rational, QuasiAngular, Polynomial) chooses how the circular section's
  surface is approximated, and its `Law_Function`s vary only the radius, so OCCT has no conic
  (rho) or curvature-continuous section. They are built here for straight edges between flat
  faces: the contact points are the circular fillet's (`d = r / tan(θ/2)` from the corner, θ the
  angle between the faces inside the section; Width: `d = w / (2 sin(θ/2))`); the section between
  the corner and the curve (a rational quadratic Bezier with poles contact–corner–contact and middle
  weight `rho/(1 − rho)`, which is exactly the circle at the circle's rho; or a quintic Bezier
  with its first three and last three poles on the faces' lines, so its curvature is zero where it
  meets them) is swept along the edge and cut from a convex edge or added to a concave one (by
  ray parity, as the shell's concave test). The rest of the tool runs out past the faces so no
  tool face lies on a body face. The tool's faces are named from the edge (`Origin::FromEdge`).
  Other edges are refused with that reason. Where two section fillets meet at a corner the
  corner isn't blended.
- **Full round.** The two corner slivers between the middle face's edges and the round (each a
  quarter-disc's complement, swept along the middle face) are cut, so the round is two quarter
  cylinders that continue the middle face (its name); the volume removed is checked against
  `(2 − π/2) r² L`, which catches side faces too shallow for the radius.
- **Chamfer.** The first distance (and the angle) go on the edge's first face in explorer order,
  on the other face when flipped (all edges) or overridden (that edge). *Tangent* measurement:
  the tangent-plane distance `d` differs from the distance along the face only where the face
  curves across the edge: a sphere, or a cylinder whose axis runs along a straight edge; there,
  for radius R, it is the chord `2R sin(atan(d/R)/2)`, on that side only (judge round 1: it was
  applied to any cylinder, so a cylinder's rim got a smaller bevel; a rim's side face is straight
  across the rim, so Tangent = Offset there). OCCT takes one chamfer kind per call, so edges
  needing different distances (Tangent on mixed faces) are refused for now.
- **Shell.** Walls facing each other across the body (kept, planar, axis-aligned, anti-parallel,
  not adjacent, their extents overlapping) closer than twice the thickness are refused first
  ("The shell's walls would intersect themselves; reduce the thickness"), since OCCT may return
  a wrong body there (even the input unchanged). *Outward*: OCCT's offset rounds the outer edges
  and corners (arc join), so outward walls are exactly `t` from the body everywhere. *Hollow*: the
  offset solid with no face removed (OCCT's `MakeThickSolidByJoin` with an empty list returns the
  offset body itself) subtracted from the body (outward: the body from it). Every result is checked
  (`BRepCheck_Analyzer`, fork `Shape::is_valid`; one solid; the volume moved the right way).
  **`shell_by_regions`**: OCCT's thick solid can't drop an offset face that vanishes (a
  counterbore's floor, 2.2 mm wide, under a 4 mm wall: the Gear Cover's Shell after its holes,
  PS17.6), so an inward shell it fails on or gets wrong is built from its definition: the walls
  are the body's points within `t` of its kept faces, i.e. the union of each kept face swept
  inward by `t` (`Shape::try_thicken_h` of the face), a tube round each concave edge (a cylinder
  along a line, a torus section round a circle or arc; concave: the point `m + δ(n₁ − n₂)` next to
  the edge's middle is inside the body, by ray parity) and a ball at each vertex of a concave edge
  (convex edges and vertices need none: their nearest points lie outside). The cavity is the body
  less those (subtracted one at a time: a compound of overlapping solids is not a valid boolean
  argument) and the shell the body less the cavity. The kernel's `shell_around_a_counterbore` and
  `cadrs_core/tests/gear_cover.rs` check it against closed forms (Pappus for the rounded concave
  corners) to 1e-6 mm³.
- **Hole** is not a kernel op: `cadrs_core` revolves each hole's half section about its axis and
  subtracts the tools (see "Rebuilding the feature list" below).

**P3.6 fork additions** (commit `8fcc94f` on `cadrs`): `Shape::try_fillet_variable_h` (a radius per
edge, constant or a `(t, r)` law), `Shape::edge_normals` (points and the two faces' outward normals
along an edge, from the faces' p-curves and `BRepLProp_SLProps`) and `Shape::is_valid`
(`BRepCheck_Analyzer`). Commit `53c1041`: `Edge::try_bezier(poles, weights)` (a rational
`Geom_BezierCurve` edge, for the conic and curvature sections).

**Trait changes in P3.5.**
- `MassProperties.inertia: Matrix3<f64>` (serde default): the volume's inertia tensor at unit
  density (mm⁵) about `center_of_mass`, axes parallel to the model's, `I = ∫(|r|²E − r rᵀ) dV`
  (the diagonal holds the moments, the products carry the minus sign, as OCCT's
  `MatrixOfInertia`). `MassProperties::inertia_about(point)` moves it to another point (parallel
  axes). A density (kg/mm³) times it gives kg·mm².
- **Convention** (recorded for the Mass properties panel): the tensor is about the **centre of
  mass**, aligned with the Part Studio's axes, which is what Onshape's Mass and section properties
  shows when no mate connector is set as the reference frame. Several parts measured together are
  combined about their common mass-weighted centre (`cadrs_core::parts::mass_report`).
- Conformance `inertia_tensor`: the 10 × 20 × 30 box about its centre (V(b²+c²)/12 = 650 000,
  500 000, 250 000; no products) and about the origin (2 600 000, Ixy −300 000); an L whose
  product is +100 000/3 (the sign convention); a cylinder r 5 h 10 (Izz = V r²/2, Ixx =
  V(3r² + h²)/12). `cadrs_core/tests/material.rs`: a 100 mm steel cube (7850 kg/m³) is 7.85 kg with
  Ixx = Iyy = Izz = 13 083.333 kg·mm²; the Control Arm in Polypropylene (900 kg/m³) is
  V × ρ = 0.331 875 kg; two cubes of steel and aluminium combine about their mass centre.

**Trait changes in P3.4.**
- `revolve_with(&mut self, profile, &RevolveSpec) -> OpResult`: `RevolveSpec { body: BodyKind,
  axis: Axis, full, end: RevolveEnd, symmetric, second: Option<RevolveEnd>, scene }`,
  `RevolveEnd = Angle(rad) | UpToNext { offset } | UpToFace { body, face, offset } |
  UpToPart { body, offset } | UpToVertex { point, offset }` (offsets are angles: positive stops
  short of the target, negative goes past it), `faces: Vec<FaceInput>` (planar faces of bodies,
  solid revolves only). The first end turns counter-clockwise about
  `axis.dir`, the second the other way. The default implementation maps a solid Full or single
  angle onto `revolve` and refuses the rest.
- `FaceInfo.axis: Option<Axis>` and `EdgeInfo.circle: Option<Circle3 { center, normal, radius }>`
  (serde default): a revolve axis can be a cylindrical face or a circular edge, and Use projects
  the exact circle.

**The full revolve (P3.4, `backend/occt_revolve.rs`).**
- The profile must not cross an axis lying in its plane, and the axis must not be normal to the
  plane (`InvalidParameter`). Angles are measured about the axis from the half plane holding the
  profile.
- Ends that are an angle (an angle, Up to vertex: the vertex's angle less the offset; Up to face
  of a plane through the axis: its angle) make one revolution: the profile's plane is turned back
  to where the second end starts and swept through both angles together (at most a turn).
- Up to part, Up to next and Up to any other face **conform**, as the extrude's do: the profile is
  turned almost a whole turn (2π·(1 − 10⁻⁴)), split by the targets (turned back by the offset
  angle) into `sweep − targets` and `sweep ∩ targets`, and the solids that hold the start cap are
  kept; a kept piece that still has the far cap means part of the profile met nothing (an error).
  Up to next takes the scene's bodies the almost-full sweep overlaps. Faces a trim makes are the
  end cap of the region they end (the point turned back onto the profile's plane).
- Surface: each region loop and open chain is a wire revolved into a sheet (fork
  `Wire::try_revolve_h`); angle ends only. Thin: the bands of `thin_profile`, revolved as a solid.
- Faces are tagged through OCCT's history exactly as the extrude's: the side of each profile
  curve, the start cap (the profile, or the second end's far face) and the end cap; a whole turn
  has no caps.

**Trait changes in P3.3.**
- `extrude_with(&mut self, profile, &ExtrudeSpec) -> OpResult`: `ExtrudeSpec { body: BodyKind
  (Solid | Surface | Thin { left, right }), direction: Unit<Vector3>, start_offset,
  end: ExtrudeEnd, symmetric, second: Option<ExtrudeEnd>, scene: Vec<BodyId>, faces:
  Vec<FaceInput> }`, `ExtrudeEnd = Blind(d) | UpToNext { offset } | UpToFace { body, face,
  offset } | UpToPart { body, offset } | UpToVertex { point, offset } | ThroughAll`
  (`offset > 0` stops short of the target, `< 0` goes past it), `FaceInput { body, face,
  source }`. The default implementation maps what `extrude` can do (a blind or symmetric solid
  along the normal) and refuses the rest.
- `Profile.chains: Vec<Chain>` (open chains of curves, `Chain { curves, source }`, serde
  default) for surface and thin extrudes; `Profile::new(plane, regions)`.
- `compound(bodies) -> OpResult` (P3H.6; default `Unsupported`): the bodies gathered unmerged.
- `split_solids(body) -> Vec<OpResult>`, `solid_count(body)` (default 1), `bounding_box(body) ->
  Aabb`, `ray_hits(body, origin, dir) -> Vec<RayHit { face, t, point }>` (defaults:
  `Unsupported`).
- `cadrs_kernel::thin`: `thin_profile(profile, left, right)` turns a profile into the regions of
  a thin wall (backend-independent; see below). `THIN_LEFT`/`THIN_RIGHT` mark the curve ids of a
  wall's offset sides.
- `naming::name_edges` names a free edge (one face) as `EdgeName { faces: [f, f] }`, so a
  surface's boundary is drawn and can be referenced.

**The full extrude (P3.3, `backend/occt_extrude.rs`).**
- Ends that are a depth (Blind; Up to vertex: the vertex's height less the offset; Through all:
  the far side of the scene bodies' boxes; Up to face of a planar face square to the direction:
  its height less the offset) make one prism from the second end to the first. Symmetric
  splits a Blind depth (or goes Through all both ways).
- Other ends are trimmed: an oblique planar face cuts the long prism with the half space beyond
  its plane (a slab built from a large square). Up to part, Up to next and Up to a curved face
  **conform**: the long prism is split by the target bodies (moved by the offset along the
  direction) into `prism − targets` and `prism ∩ targets`, and the solids touching the start
  plane are kept, so every point of the profile stops at the first target face it meets,
  entering or leaving it (a cut from a part's face "up to next" stops at the part's other
  side). A kept piece that still reaches the far end means part of the profile met nothing:
  an error, not a guess. Up to next first finds the bodies ahead with rays from sample points
  of the profile (and errors when there are none). Faces a trim makes are the end cap (the
  start cap for a second end) of the region under them.
- Surface: each region loop and open chain is a wire swept into a sheet (`Wire::try_extrude_h`);
  the sheets are one compound body. Up to next/part/face ends are not supported for surfaces
  yet.
- Thin: `thin_profile` builds exact 2D bands along every curve (a line's rectangle, an arc's
  annular sector, a circle's annulus) and fills the outer side of each corner up to where the
  offset lines meet (a mitred corner); the bands are regions, so the thin wall takes every end
  type, and their offset sides and the fills' outer sides are named after the curve (with
  `THIN_LEFT`/`THIN_RIGHT`), so no seams show. `MakeThickSolidBySimple` (bound as
  `Shape::try_thicken_h`) was tried first: it doesn't join offset faces at corners (a 20 × 20
  square came out at −920 mm³ instead of 1440). Ellipses can't be offset this way (an error).
- A planar face as input is swept with its own history: its sides are `FromEdge` of the body's
  edges, its caps named after the caller's `source`.

**Trait changes in P3.2.**
- `History` is `generated: Vec<(FaceId, Origin)>`, `modified: Vec<(FaceId, InputFace)>`,
  `deleted: Vec<InputFace>` (`InputFace { body, face }`). `Origin` is `ProfileCurve { region,
  curve }`, `StartCap { region }`, `EndCap { region }`, `FromEdge { body, edge }`,
  `FromVertex { body, vertex }` (was `ProfileCurve(u64)`, `StartCap`, `EndCap`, `FromEdge`,
  `FromFace`).
- `Region.source: Option<u64>`: the caller's id for a profile region (its index if `None`).
- `FaceInfo.center` (area centroid). `EdgeInfo` gains `curve: CurveKind`, `start`, `end`, `mid`,
  `start_tangent`, `end_tangent`, and `length` is exact for every curve (was polyline length for
  ellipses and B-splines).
- `vertices(body) -> Vec<VertexInfo { id, point, edges }>` (default: the distinct ends of
  `edges()`; OCCT overrides it with its own vertex list).
- `adjacent_faces(body, face)` and `tangent_chain(body, edge)` (default methods over `edges()`
  and `vertices()`). A tangent chain is `edge` and every edge reached through vertices where the
  tangents are parallel within `TANGENT_ANGLE` (0.1°), Onshape's tangent propagation.
- `KernelError::InvalidParameter(String)`: a parameter the kernel can't build with, worded for
  the user. The OCCT extrude refuses depths below `MIN_DEPTH` (1e-6 mm, ten times
  `Precision::Confusion`) with it; the app shows the reason on the failed feature row.

**Trait changes in P3.1.**
- `tessellate(&self, body, quality: Tessellation)` replaces the `tolerance: f64` argument (an
  angular limit is needed for smooth curved faces).
- `release(&mut self, body)` (default: no-op).
- `Curve2::EllipseArc { center, major_radius, minor_radius, rotation, start, sweep, source }`,
  and `Curve2::start`/`end`/`point_at`/`source`. The minor radius may be larger than the major
  one or negative (the sketch's conventions); the backend restates it for OCCT.

**Ids.** `FaceId(i)`, `EdgeId(i)` and `VertexId(i)` are indices in `TopExp::MapShapes` order (the
explorer's order, each sub-shape once). They are only valid for the body they were read from;
documents store names (see "Persistent naming").

**History (P3.2).** The fork's `*_h` operations (commit `c249012` on `cadrs`: prism, revolve,
boolean, fillet, chamfer, thick solid) return OCCT's own history as indices: for every face of the
inputs the result faces it became (`Modified`, itself if kept, none if `IsDeleted`), for every
input edge and vertex the faces generated from it (`Generated`), and a sweep's
`FirstShape`/`LastShape`. The backend turns it into our `History`:
- extrude and revolve: each profile edge's generated face is the side of the profile curve the
  edge lies on (matched by the edge's midpoint against the exact curves), the first and last
  shapes are the start and end caps; several regions are fused with the tags carried through
  the fusion's history, so the result has only `generated` entries;
- boolean: every result face `modified` from the target's or a tool's face it continues (all
  pieces of a split face), the rest `deleted`. A Union also merges a face of the target with a
  face of the tool it meets on the same surface (`ShapeUpgrade_UnifySameDomain` keeping every
  edge between two faces of one input: fork `cadrs_fuse_clean_h`, P3.3 judge round 3), so no
  seam is left where the bodies met; the merged face continues the target's face (the first
  input wins), so references to it keep resolving. Faces of one body are never merged (the
  caps of regions extruded together stay apart);
- transform: every face modified from the same face (a copy keeps the topology);
- fillet, chamfer, shell: kept faces modified, faces from edges and vertices generated.

**Topology queries (P3.2).** Also from the fork: exact edge geometry (ends, midpoint, unit end
tangents, exact length via `GCPnts_AbscissaPoint`, curve type), the faces around each edge
(`MapShapesAndUniqueAncestors`), and each vertex's point and edges.

**P3.5 fork addition** (commit `7278a45` on `cadrs`): `MassProperties.inertia` in
`Shape::mass_properties` (`GProp_GProps::MatrixOfInertia` of the volume properties, through a
`GProp_GProps_MatrixOfInertia` helper that fills a slice).

**P3.4 fork additions** (commit `df2e7c4` on `cadrs`): `Shape::try_revolve_h` and
`Wire::try_revolve_h` (revolves of any shape with history: surfaces of wires),
`Shape::face_axes` (`BRepAdaptor_Surface` cylinder, cone, sphere, torus, surface of revolution
axes) and `Shape::edge_circles` (`BRepAdaptor_Curve::Circle`).

**P3.3 fork additions** (commit `b45eb6d` on `cadrs`): `Shape::sub_count`/`sub_shapes`
(solids, shells, faces, wires, edges of a shape, sharing its sub-shapes), `Shape::try_compound`,
`Shape::try_thicken_h` (MakeThickSolidBySimple, with history; not used, see Thin), `Shape::ray_hits`,
`Shape::bbox`, `Wire::try_extrude_h`, `Wire::edges_geometry`, `Wire::to_shape`.

**Known gaps in the fork**, to add on the fork's `main` branch when needed:
- `Face::normal_at` projects the point onto the surface, which throws when the projection is
  degenerate (for example a cylinder's centroid on its axis). The backend only calls it for
  planar faces, with a point on the face.
- The older, non-`safe` API of the fork still aborts on OCCT exceptions; the backend doesn't use
  it for anything that can fail (`translated`/`rotated`, explorers, `mass_properties`,
  STEP export).

## Drawing-view projection (P3C.2)
`Kernel::project(bodies, frame, opts) -> Projection` is the kernel side of drawing views
(`cadrs_drawing::view`, `cadrs_core::views`). It is a query: it makes no bodies.
- **Types** (`src/projection.rs`, no backend types): `ViewFrame { origin, dir, x }` (the
  direction of sight, eye → model, and the sheet-right direction; 2D y is `x × dir`, so a front
  view looking along +Y with x = +X has y = +Z); `ProjectOptions { tolerance, hidden }`;
  `Projection { edges }` of `ProjEdge { visibility: Visible | Hidden, class: Sharp | Smooth |
  Outline, curve: Line | Arc | Polyline, points, source }`. `ProjSource { body, edge, face,
  edge_name, face_name }` says where an edge came from: the index of the body in the call, the
  body's `EdgeId` (sharp and smooth edges) or `FaceId` (a cylinder's outline). The kernel fills
  the ids; `cadrs_core` adds the persistent names from the body's `BodyNames`, so P3C.3
  annotations attach to model topology.
- **OCCT** (fork `opencascade::hlr::hlr_project`, commit `9c4bc31`): exact `HLRBRep_Algo` on the
  body (several bodies as one compound, so they hide each other). Hidden edges lying on visible
  ones (a box's back edges behind its front ones) are dropped
  (`remove_hidden_behind_visible`). HLR reports no topology, so `attach_sources` finds it by
  geometry: a projected piece belongs to the 3D edge (its display-mesh polyline, 0.05 mm) whose
  projection runs through its 25/50/75 % points; where several do (edges behind each other), a
  visible piece takes the one nearest the eye and a hidden piece the farthest. A straight
  outline belongs to the cylinder whose projected axis is parallel to it, the radius away.
  **Seams**: OCCT reports a cylinder's seam as a sharp edge (and drops the outline it lies on
  when the seam is on the silhouette); a piece that comes from a seam becomes that cylinder's
  outline if it is one, and is dropped otherwise (seams aren't drawing lines).
- **Caveats** (from the fork): visible edges may be split at silhouette points (merge collinear
  pieces before dimensioning); circles seen edge-on come back as lines; some fillet boundaries
  are classed sharp; exact HLR takes about 6.5 ms on a filleted box (debug) and grows with the
  model, so `cadrs_core` runs it on the rebuild worker thread and the app caches each view.

**Section and broken-out views (P3C.8).** No new kernel operation and no fork change: the cut is
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
*features* in the consumer (`Document::linked`), never bodies: OCCT bodies can't move between
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
  chamfer), a pick whose tangent chain has edges not picked fails ("Without tangent propagation
  the fillet would stop part-way along a smooth edge chain (N tangent edges not picked), which
  OpenCascade cannot do; …"): OCCT's fillet and chamfer contours always run on along G1 edges, so
  the only honest results are the whole chain or an error. A **Full round** (`FilletType::FullRound`,
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


`cadrs_core` depends on `cadrs_kernel` with the `occt` feature (its default feature `occt`;
without it, extrudes fall back to the old prism mesh of `solid::extrude`).
- `cadrs_core::rebuild` rebuilds a Part Studio's features. Each feature's output (its kernel
  `BodyId`, display mesh, exact mass properties and error) is cached under a key that chains the
  keys of the features before it with its own parameters, so an edit recomputes from the first
  changed feature. Outputs no rebuild used in the last 48 rebuilds are evicted and their bodies
  released.
- All kernel work runs on one worker thread that owns the `OcctKernel` and the cache.
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
- Display tessellation: 0.05 mm and 5°. BRepMesh dominates an extrude's rebuild time (about
  1.5 ms of 2.5 ms for a plate with a hole in a debug build).

Measured (debug build, `cadrs_core/tests/kernel_rebuild.rs`): a 20-feature studio (10 sketches,
10 extrudes of slots with holes) rebuilds from scratch in about 40 ms, and in about 4 ms after an
edit to the last extrude. With persistent naming (P3.2: the history, and naming every face, edge
and vertex) it is about 54 ms and 5 ms. The Control Arm built through the feature list with both extrudes as New
gives two parts totalling 368 749.7048 mm³.

**Windows.** `cargo build -p cadrs_kernel --features occt --release --target x86_64-pc-windows-gnu`
works with the installed MinGW (GCC 13, win32 threads); the cold build takes about 7 min. Two
fixes were needed, both in this repo rather than in the fork:
- `crates/cadrs_kernel/build.rs` links `advapi32`, which OCCT's OSD layer uses (security
  descriptors, registry, `GetUserNameW`) and `opencascade-sys` doesn't link.
- `.cargo/config.toml` sets `CXXSTDLIB_x86_64_pc_windows_gnu = "static:-bundle=stdc++"`, so the
  exe doesn't need `libstdc++-6.dll`. The test exe then imports only system DLLs (KERNEL32,
  ADVAPI32, USER32, msvcrt, ...).
The Windows test exe links, but it hasn't been run yet: there's no Wine here, so it needs a
Windows machine.

P3.1 (2026-09-27): the whole app with OCCT, `cargo build --release --target x86_64-pc-windows-gnu`,
builds and links (10.5 min cold in a fresh target dir; 257 MB exe, copied to
`dist/cadrs-P3.1.exe`). It imports only system DLLs (KERNEL32, USER32, ADVAPI32, msvcrt,
combase, …). It hasn't been launched: there's still no Windows machine or Wine here.

Conformance results (OCCT): all 77 cases pass (Final: `bezier_profiles` and `fillet_smooth_corner`
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
region bounded by an ellipse's offset: the prism's volume to 5e-5 relative, OCCT integrating the
B-spline bounded faces at a fixed order; the rim's edge length L − 2πd to 1e-7),
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
refused), `shell_around_a_counterbore` (OCCT's thick solid fails; the region shell matches the
closed form) and `edge_face_radius`. Before P3.6: all 44 cases pass. P3.5 added `inertia_tensor` (above). Before P3.5:
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
`rebuild/kernel_ops/derived.rs`). No new kernel operation and no fork change:
- The source's features travel inside the feature (`DerivedFeature::studio`, with its part
  settings), so every rebuild of the host (the app's, an assembly's, a drawing's, an export's)
  builds them, and the cache key (the feature's `Debug`) changes exactly when the source does.
  Workspace references are brought up to date after every command, undo and redo
  (`derived::refresh`, called by `History`).
- The source is rebuilt **inside** the host's rebuild, in the same kernel session and cache
  (`Rebuilder::sub_build`: it saves and restores `trail`, `last`, `stage_for` and the sketch
  times, and nests at most 8 deep), because bodies can't move between OCCT sessions.
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
