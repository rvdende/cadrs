# Introduction to Parametric Feature-Based CAD: gap analysis (2026-09-27, refreshed 2026-09-29)

This maps [intro-to-parametric-cad.md](intro-to-parametric-cad.md) against the cadrs code on branch
`phase3`: written at `ff7616b`, **refreshed at `42a136f`** (stages 3A, 3B, 3C and 3D merged),
at `f1e161a` (P3F.2) and after P3F.3–P3F.4. ✅ done · 🟡 partial · ❌ missing · **→ 3G** moved to stage 3G · **out of scope**
(cloud and multi-user collaboration, sharing and permissions, release management and paid tiers,
and Onshape account and learning-site features; and, by user decision 2026-09-29, "niche; out of
scope": configurations, image tabs, full-round/conic/curvature fillets, a detailed Versions and
history panel). Stage 3F covers this course and
[essential-tips.md](essential-tips.md) with **one milestone numbering**:
[essential-tips-gaps.md](essential-tips-gaps.md) defines **P3F.1–P3F.3** (P3F.2 is import/export,
used here too); this file defines **P3F.4–P3F.6**. Related plans:
[intro-to-part-studios-gaps.md](intro-to-part-studios-gaps.md) (P3.x),
[intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md) (P3B.x),
[intro-to-drawings-gaps.md](intro-to-drawings-gaps.md) (P3C.x),
[inspection-and-repair-gaps.md](inspection-and-repair-gaps.md) (P3D.3).

**Summary (refresh):** the course is conceptual. cadrs is now a full parametric feature modeller.
- **Modelling.** Every feature regenerates through the OCCT kernel with persistent names (P3.1,
  P3.2). There are Revolve (P3.4), Hole (P3.6), and linear, circular and curve patterns (P3.8).
- **Assemblies** have mates (P3B). **Drawings** are linked to the model, with first/third angle
  and isometric views, and update on request (P3C).
- **History.** The document has a persisted history with versions (P3D.3).
- **Import and export (P3F.2).** STEP, IGES, STL and OBJ; DXF and DWG of a sketch or a face.
- **Views.** View cube corners give trimetric views (P3.9).

- **Variables (P3F.4).** The Variable feature, the Variable table, `#name` in every value field,
  and the expression engine (+ − × ÷, units, sqrt, sin, cos, tan, min, max).

- **Simulation (P3F.5).** Linear static FEA: the Simulation panel, Loads, quadratic tetrahedra,
  a sparse solve, von Mises and displacement maps with a legend and probe; bonded assemblies.

- **Rendering and images (P3F.6).** Render Studio tabs (a CPU path tracer with procedural
  environments, PBR materials from appearances and materials, a ground shadow, a PNG at a chosen
  size, the same bytes for the same seed); Export image… (PNG/JPEG) from 3D tabs and exploded
  views.

Left: nothing. Search across documents' parts was delivered by stage 3G (P3G.1, P1.3).

**Cross-stage dependencies.** Most rows close through P3.x, P3B, P3C and P3D.3, which run earlier.
P3F.2, P3F.4, P3F.5 and P3F.6 add the rest. The assemblies stage waits for P3F.4 (Variable table)
and P3F.5 (Simulation, Loads); see [intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md).

**Counts (30 P IDs + 9 X IDs + 1 quiz row = 40).**
- **At the analysis (`ff7616b`):** ✅ 3 · 🟡 18 · ❌ 17 · out of scope 2 (P: 2 / 13 / 14 / 1; X:
  1 / 5 / 3 / 0; the quiz is the other out-of-scope row).
- **Refreshed after P3F.2:** ✅ 29 · 🟡 6 · ❌ 3 · out of scope 2 (P: 21 / 5 / 3 / 1; X: 8 / 1 /
  0 / 0).
- **Refreshed after P3F.4:** ✅ 34 · 🟡 2 · ❌ 2 · out of scope 2.
  - P: 25 / 2 / 2 / 1 (P5.1, P5.2, P5.3, P6.3 done).
  - X: 9 / 0 / 0 / 0 (X2 done).
  - The quiz is out of scope; P1.3's cross-document search is in 3G.
- **Refreshed after P3F.5:** ✅ 35 · 🟡 2 · ❌ 1 · out of scope 2.
  - P: 26 / 2 / 1 / 1 (P3.5 done; P3.6 rendering is P3F.6).
  - X: 9 / 0 / 0 / 0.
  - The assemblies course's simulation rows (A1.7, A1.8, A6.3, X8, X16) are done too, in
    [intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md).
- **After merging stage 3G:** ✅ 36 · 🟡 1 · ❌ 1 · out of scope 2 (P: 27 / 1 / 1 / 1; X: 9 / 0 / 0 / 0; P3G.1 moved P1.3 to ✅).
- **After P3F.6:** ✅ 38 · 🟡 0 · ❌ 0 · → 3G 0 · out of scope 2 (P1.4 multi-user and cloud; the
  quiz).
  - P: 29 / 0 / 0 / 1 (P3.6 and P3.7 done).
  - X: 9 / 0 / 0 / 0.

## 1. Introduction

### P1 History of CAD
| ID | Requirement | Status | Notes |
|---|---|---|---|
| P1.1 | One store per document holding its studios, assemblies and drawings | ✅ | `cadrs_core::store` saves a document as one unit with all its elements. |
| P1.2 | Automatic versions / history to go back to | ✅ | P3D.3: every change is recorded in the persisted document history (`history_log.rs`, `history.ron`), with Restore and named versions. |
| P1.3 | Searchable, reusable metadata on parts and documents | ✅ | Documents have names, owners and dates, searchable on the documents page. Part properties (part number, description…) are P3B.6. P3G.1: the Insert dialog's search finds parts by name, part number or description, in this document and in other documents at a version, and documents by name or pasted id (`course_er_insert_linked` 05). |
| P1.4 | Real-time multi-user editing; browser access without install | out of scope | Multi-user collaboration and cloud. |

### P2 2D vs 3D CAD
| ID | Requirement | Status | Notes |
|---|---|---|---|
| P2.1 | Parametric 3D modeller; drawing views come from the model | ✅ | Drawings (P3C.2) project their views from the model through the kernel (HLR). |
| P2.2 | Each sketch and feature stores editable parameters; model regenerates | ✅ | Every feature kind stores its parameters and regenerates through the kernel (P3.1–P3.10). |
| P2.3 | A parameter change can produce a big change without remodelling | ✅ | Full kernel rebuild of the feature list (P3.1) with stable references (P3.2, `cadrs_core/tests/naming.rs`). |
| P2.4 | Drawing views stay linked and update when the model changes | ✅ | P3C.6 (D13.2): views follow the model on Update (a gold Update icon while out of date), as Onshape does. |

### P3 Utilizing CAD data
| ID | Requirement | Status | Notes |
|---|---|---|---|
| P3.1 | Manufacturing drawings from the model | ✅ | P3C.1–P3C.8. |
| P3.2 | DXF and DWG for cutting machines | ✅ | P3F.2: **Export as DXF/DWG…** on a sketch or a planar face (the drawings' DXF writer; DWG through an external converter, the licensing decision in [essential-tips-gaps.md](essential-tips-gaps.md)); drawings' DXF/DWG (P3C.7). `course_pcad_export` 07–11; `dxf_export` and `tests/exchange.rs` DXF tests. |
| P3.3 | STL and OBJ for 3D printing | ✅ | P3F.2: STL (binary/text) and OBJ from the kernel's tessellation with chord and angle tolerance and units; a 100 × 60 × 25 box is 12 triangles enclosing 150 000 mm³ (`tests/exchange.rs`). `course_pcad_export` 03–06. |
| P3.4 | A CAM-grade B-rep export (STEP) | ✅ | P3F.2: STEP (AP214, product names, optionally as an assembly) and IGES from the tab, part or instance Export…; Control Arm round trip keeps V = 368 749.705 mm³ (1e−3). |
| P3.5 | Simulation / FEA on the model | ✅ | P3F.5: linear static FEA (`cadrs_fea`: Delaunay tetrahedral meshing of the kernel's surface, 10-node tetrahedra, sparse Cholesky off the main thread) from the right strip's **Simulation** panel: materials from the library, the Loads list (Fixed, Force, Pressure), Mesh, Solve with progress, von Mises or displacement on the (exaggerated) deformed shape with a legend and a probe. The course's cantilever: tip 0.20002 mm vs 0.200 (EB), mid-span σ 30.03 MPa vs 30.0. `course_pcad_simulation`; see "P3F.5 status". |
| P3.6 | Photorealistic rendering | ✅ | P3F.6: **Create Render Studio** (the tab "+" menu) makes a Render tab of a Part Studio or an Assembly: a path-traced live preview; environment (Studio, Soft light, Outdoor, Sunset; procedural), its rotation, background (environment, white, transparent), ground shadow; camera from a named view or the source tab's current view, perspective or orthographic; PBR materials from the parts' appearances and materials; **Render…** to a PNG at a chosen size and quality, off the main thread with progress and Cancel. `course_pcad_render` writes the Control Arm at 1920 × 1080; `cadrs_core/tests/render.rs`, `cadrs_render/tests/render.rs`. See "P3F.6 status". |
| P3.7 | Model data reused in instructions and manuals (exploded views, images) | ✅ | Exploded views (P3B.8) and drawings with PNG/JPEG export (P3C.7); P3F.6: **Export image…** (PNG or JPEG, the viewport's size, HD, Full HD, 4K or a custom size, transparent background) from a Part Studio's or an Assembly's tab menu and from the Exploded views panel (the exploded view shown). `course_pcad_export_image`. |

## 2. Parametric feature-based CAD

### P4 Introduction to 3D CAD
| ID | Requirement | Status | Notes |
|---|---|---|---|
| P4.1 | Sketch profiles + Extrude and Revolve | ✅ | Extrude (P3.3), Revolve (P3.4). |
| P4.2 | Features run top to bottom; later ones depend on earlier ones | ✅ | Kernel rebuild in list order (P3.1); Show dependencies and reorder (P3.9). |
| P4.3 | Each feature defines one aspect (shape, hole size and position) | ✅ | Hole feature (P3.6) and the other features. |
| P4.4 | Parts go into assemblies with mates | ✅ | P3B.1, P3B.2. |
| P4.5 | Parts and assemblies go onto 2D drawings | ✅ | P3C.2, P3C.5 (assembly drawings). |

### P5 Design intent
| ID | Requirement | Status | Notes |
|---|---|---|---|
| P5.1 | Master sketch drives a Revolve; bore = piston diameter + clearance | ✅ | P3F.4: `samples::design_intent`: the master sketch's bore dimensioned diametrally as `#piston_d + #clearance`, revolved (69 869.021 → 85 199.993 mm³ when `#piston_d` goes 40 → 50 mm). `course_pcad_design_intent`. |
| P5.2 | Variables used in dimensions and feature parameters | ✅ | P3F.4: the **Variable** feature (`#name = expression`, Length/Angle/Number/Any, a description) in the feature list and in assemblies' Mate Features; the **Variable table** (right strip); `#name` in every value field (sketch dimensions, depths, angles, pattern counts and distances, hole sizes, fillet/chamfer/shell sizes, plane offsets, connector offsets, mate offsets). `course_pcad_variables`, `course_asm_variables`. |
| P5.3 | Expressions (variable + clearance) in dimensions | ✅ | P3F.4: the expression engine (`cadrs_sketch::units`): + − × ÷, parentheses, units (mm, cm, m, in, ft, yd, deg, rad), `#name`, sqrt, sin, cos, tan, min, max, abs, pi; kept as typed next to its value and re-evaluated when a variable changes. |
| P5.4 | Use links a diameter to another part's geometry | ✅ | Use / project (S20, T5) follows its source across parts in a studio. |
| P5.5 | Linear pattern repeats a feature and follows edits | ✅ | P3.8 (feature and part patterns). |
| P5.6 | Parent/child relations survive a rebuild after changing the one input | ✅ | Persistent naming (P3.2): sketch-on-face, Use and Pierce links resolve by name after dimension and depth edits (`cadrs_core/tests/naming.rs`). P3F.4: end to end with variables: `#piston_d` 40 → 50 mm rebuilds the body, grooves, piston and the clamp's bore (Use of the piston's edge) with no feature errors (`tests/variables.rs`). |

### P6 View projections
| ID | Requirement | Status | Notes |
|---|---|---|---|
| P6.1 | Six orthographic views (Front, Top, Right, Left, Back, Bottom) on drawings | ✅ | 3D view cube and named views; drawings' projected views (P3C.2). |
| P6.2 | First-angle and third-angle layouts | ✅ | P3C.1 (template projection), P3C.2 (placement; the L-block test in `drawing_views.rs`). |
| P6.3 | Isometric, dimetric, trimetric axonometric views | ✅ | The 3D view menu has all three (`view_cube.rs`) and drawings have isometric views (P3C.2). P3F.4: `axonometric_axis_scales`: isometric √(2/3) = 0.81650 on all three axes; dimetric (45°, 20.705°) X = Y = 0.75, Z 0.935; trimetric (30°, 30°) 0.9014, 0.6614, 0.8660. |
| P6.4 | View cube corners give trimetric views | ✅ | P3.9: a corner patch (lit on hover) turns the view to the trimetric view from that corner, the default view's 30°/30° mirrored into its octant (`view_cube.rs` `corner_view`; test `corners_turn_to_trimetric_views`; `course_p6_cube_corner` 01–04). |

## Knowledge check
| ID | Requirement | Status | Notes |
|---|---|---|---|
| Quiz | Course quiz | out of scope | Learning-site feature. Its topics (projections, views) are P6 rows. |

## Exercises
None (the course is lecture-only). Each milestone ships `course_pcad_*` scenarios, and the numeric
checks below replace a self-check.

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Parametric regeneration of the ordered feature list | ✅ | P3.1 (kernel rebuild, per-feature cache, off the UI thread). |
| X2 | Variables and expressions in dimensions and feature fields | ✅ | P3F.4 (P5.2, P5.3). |
| X3 | Revolve and Linear pattern | ✅ | P3.4, P3.8. |
| X4 | Assemblies with mates / motion | ✅ | P3B.1–P3B.3. |
| X5 | Drawings linked to the model, first/third angle, isometric | ✅ | P3C.1, P3C.2, P3C.6. |
| X6 | View cube faces → orthographic, corners → trimetric; named standard views | ✅ | Faces, named views, corners (P3.9). |
| X7 | Exports: DXF/DWG, STL/OBJ, STEP | ✅ | P3F.2 (3D tabs, parts, instances, sketches, faces) and P3C.7 (drawings). |
| X8 | Automatic version history | ✅ | P3D.3. |
| X9 | Cross-part references through Use | ✅ | S20 / T5. |

## What each lesson needs (no exercises)
- **P5 design intent → `course_pcad_design_intent`** (built from scratch, mm). A hydraulic cylinder
  body driven by variables `#piston_d = 40 mm` and `#clearance = 0.5 mm`:
  - master sketch on Front, revolved: bore ID = `#piston_d + #clearance`, wall 5 mm (OD = ID + 10),
    length 100 mm;
  - one O-ring groove (2 mm deep, 2 mm wide) in the bore wall, linear-patterned ×3 (P3.8);
  - a separate piston part Ø`#piston_d`, and a clamp whose bore uses Use on the piston's edge.
  - Closed form: V_body = π/4·(OD² − ID²)·100 − 3·π/4·((ID + 4)² − ID²)·2 = **69 869.021 mm³** at
    `#piston_d = 40`; after changing it to 50 mm in the Variable table, **85 199.993 mm³**, with no
    feature errors and the clamp bore following (Ø50). Unit test to 1e−6 relative.
- **P6 projections → unit tests in P3C.2 and P3F.4.** The L-block of
  `intro-to-parametric-cad/view-projections-first-angle.png` (4×4 with a 2×2 notch) in first angle:
  Left view to the right of Front, Top below (P3C.2). Axonometric axis scales (P3F.4): isometric
  (view dir (1, 1, 1)/√3) gives all three projected unit axes length √(2/3) = 0.81650; the dimetric
  view gives exactly two equal; the trimetric gives three different lengths.
- **P3.5 simulation → `course_pcad_simulation`.** A steel cantilever 100 × 10 × 10 mm (E = 200 GPa,
  ν = 0.3), one end fixed, 100 N down at the tip: Euler–Bernoulli tip deflection PL³/(3EI) =
  **0.200 mm** (I = 833.33 mm⁴), bending stress at mid-span M·c/I = **30.0 MPa**.
- **P3.6 rendering → `course_pcad_render`.** A rendered PNG of the Control Arm (P3.3) with
  appearances and an environment; no numeric check.

## Proposed milestones (stage 3F, part 2)
Every milestone ships scenarios and a fresh-judge round (≥ 8.5) against the requirement text and
`intro-to-parametric-cad/`.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3F.2 | **Import and export** ✅ (built in [essential-tips-gaps.md](essential-tips-gaps.md)'s numbering; judge 8.5) | P3.2–P3.4, X7 | See "P3F.2 status" below and the "P3F.1–P3F.2 status" section of [essential-tips-gaps.md](essential-tips-gaps.md). |
| P3F.4 | **Variables, expressions, design intent** ✅ (judge 8.5) | P5.1 (expression), P5.2, P5.3, P5.6 (demo), P6.3 (test), X2; also A1.8 "Variable table" (assemblies), PS3.6 `:variable` filter (with P3.9) | A **Variable** feature (#name = expression with units) in the feature list and a **Variable table** panel (right strip) listing variables with values; `#name` accepted in every numeric field (sketch dimensions, extrude/revolve depths, pattern counts, hole sizes, mate offsets); dependency ordering (a variable must come before its uses; forward use is an error). The expression engine supports + − × ÷, parentheses, units, and sqrt, sin, cos, tan, min, max. Unit tests: `#piston_d + #clearance` evaluates with mixed mm/in units; a variable used before its definition errors; the `course_pcad_design_intent` model gives 69 869.021 → 85 199.993 mm³ after changing `#piston_d` (1e−6 relative); the axonometric axis-scale test above. `:variable` filter lists the features that use a variable. |
| P3F.5 | **Simulation (linear static FEA)** ✅ (judge r2 8.65, r1 8.4) | P3.5; also A1.7 "Loads", A1.8 "Simulation", A6.3 "Simulation connection" and X16 simulation items (assemblies) | A Simulation panel in Part Studios and Assemblies: material from P3.5, **fixed** faces, **force** and **pressure** loads (the Loads list), quadratic tetrahedral mesh (via a Rust mesher or OCCT's `BRepMesh` + a tet generator), a sparse linear solve, displacement and von Mises results as a colour map with a legend and probe. Mates with "Simulation connection" bond the two parts (Fastened = bonded). Unit tests: the cantilever tip deflection is within 3 % of 0.200 mm and mid-span σ within 5 % of 30.0 MPa; two 50 mm halves joined by a Fastened mate with Simulation connection give the same tip deflection within 5 %. Solve under 10 s for 20k elements, off the main thread. |
| P3F.6 | **Rendering and images** ✅ (judge 8.45, all deltas minor) | P3.6, P3.7 (images) | "Render Studio" in the "+" menu creates a Render tab that references a studio or assembly, with an environment (bundled CC0 HDRIs), camera from a named view, PBR materials from appearances and materials, ground shadow, and **Render** to a PNG at a chosen resolution (path-traced or high-sample raster, accumulated off the main thread). "Export image…" from any 3D tab and from an exploded view (P3B.8) writes PNG/JPEG at a chosen size. Scenario `course_pcad_render` writes a 1920×1080 PNG of the Control Arm; a test checks the size and that the image isn't blank (luminance variance > threshold); deterministic output for a fixed seed. |

## P3F.2 status (this course's rows)
**P3F.2 is built** (the full status is in [essential-tips-gaps.md](essential-tips-gaps.md),
"P3F.1–P3F.2 status"). For this course:
- **P3.2 (DXF, DWG):**
  - **Export as DXF/DWG…** on a sketch (feature list and view) and a planar face (view).
  - The drawings' DXF writer; R2013 or R2000.
  - DWG through an external converter.
- **P3.3 (STL, OBJ):**
  - STL (binary or text) and OBJ from the tessellation.
  - Resolution Coarse, Medium, Fine or Custom (chord and angle tolerance); units mm … ft.
- **P3.4 (STEP):**
  - STEP AP214 and IGES 5.3 of a studio's parts, an assembly's instances (optionally as an
    assembly of instances), a part or an instance.
  - Y axis up; individual files.
- **Scenario `course_pcad_export`** (Control Arm):
  - 01–02: the tab menu and the dialog.
  - 03–05: STL and OBJ options.
  - 06: an STL written.
  - 07–09: a sketch's DXF.
  - 10–11: a face's DXF.
- **Unit tests:** `cadrs_core/tests/exchange.rs`, `dxf_export`.

**P3F.4 details (the plan):** a `Variable` feature kind in `cadrs_core`, the expression evaluator extended
from `cadrs_sketch::units` with identifiers and functions (no Bevy), variable references stored
as expressions next to their evaluated values, the Variable table panel, and expression editing in
sketch dimension boxes (`cadrs_ui/src/dim_edit.rs`) and feature fields.

**P3F.5 details:** a `cadrs_fea` crate (no Bevy) with meshing, element assembly and a sparse
Cholesky/CG solver; a Simulation panel and Loads list in the app; results as vertex colours on the
tessellation.

**P3F.6 details:** a render element kind; a high-quality renderer path (Bevy PBR with shadows,
SSAO and many-sample accumulation, or a small CPU path tracer over the tessellation); image export
dialog shared with exploded views.

Not scheduled: nothing. Out of scope: P1.4 (multi-user, cloud) and the quiz (learning site).

## P3F.3–P3F.4 status
**P3F.4: done** (judge 8.5). P3F.3 is in [essential-tips-gaps.md](essential-tips-gaps.md).
- **Expressions** (`cadrs_sketch::units`, no Bevy): values carry length and angle exponents;
  `#name` reads a [`Variables`] lookup; functions sqrt, sin, cos, tan (degrees for a plain
  number), min, max, abs; `pi`; `×`, `÷`, `−`; a comma separates a function's arguments and is a
  decimal comma elsewhere. Errors: an undefined variable, an unknown function, mixed units
  (`#length + #angle`), sqrt of a negative. Tests `variables_with_mixed_units` (`#piston_d +
  #clearance` = 40.5; with 0.02 in, 40.508; an inch workspace's bare numbers), `functions`.
- **Model** (`cadrs_core::variables`): `FeatureKind::Variable` (name, type, expression, value,
  description), listed as "#name"; `slots` visits every numeric field's (expression, value);
  sketches keep dimension expressions in `Sketch::expressions`; a pattern's count in
  `count_expr`. **Propagation**: `variables::refresh` runs in `commands::refresh_studio` after
  every Part Studio edit, so a variable's new value reaches every use in one undo step (sketch
  dimensions re-solved). **Order**: `variables::check` in the rebuild fails a feature naming a
  variable defined below it ("Depth: #wall is used before it is defined; move its Variable above
  this feature") or nowhere (P3D.1's red row and reason). **Filter**: `:variable <name>` lists
  the Variable and the features using it (`feature_list::variable_facts`).
- **Assemblies** (A1.8): `MateKind::Variable` in the Mate Features list (the assembly toolbar's
  Variable), offsets typed as expressions (`MateFeature::exprs`, `assembly::vars`), re-evaluated
  and re-solved when a variable changes.
- **App**: the Variable dialog (`variables_ui`: Type tabs, Name, Value with what it evaluates
  to, Description; `variable-*`), the toolbar's Variable (ƒx), the Variable table in the right
  strip (each row's value editable: `variable-row-<name>`; errors in red under it), `#name` in
  the dialogs' value fields (`ActiveVariables`), sketch dimension boxes keeping the expression
  (shown on the dimension with the sketch's **Show expressions**).
- **Numbers** (`cadrs_core/tests/variables.rs`, `samples::design_intent`): the course's model,
  re-derived: ID = #piston_d + #clearance, OD = ID + 10, 100 long, three grooves 2 deep and 2
  wide: V = π/4·(OD² − ID²)·100 − 3·π/4·((ID + 4)² − ID²)·2 = **69 869.021 mm³** at 40 mm
  (π/4·910·100 − 3·π/4·340·2) and **85 199.993 mm³** at 50 mm (π/4·1110·100 − 3·π/4·420·2), both
  to 1e−6 relative, as the gap list gave them; the piston and the clamp (its bore by Use)
  follow; one undo restores 69 869.021. Also: mixed-unit variables, a variable used before its
  definition (moved below the master sketch: the sketch fails; the piston's depth `#stroke`
  undefined, then defined below, then above), types and uses, a plain value dropping an
  expression.
- **Axonometric scales**: `view_cube::tests::axonometric_axis_scales` (P6.3 above).
- **Scenarios**: `course_pcad_design_intent` 01–06 (the model, the Variable table, Mass
  properties 69869.021 mm³, the master sketch with Show expressions "Ø#piston_d + #clearance",
  #piston_d = 50 mm in the table, 85199.993 mm³); `course_pcad_variables` 01–09 (the dialog,
  #width, #depth = #width / 2, the table, "#width" typed into a dimension and "#depth" into the
  extrude depth, #width = 70 in the table propagating, a forward use in red with why, the
  `:variable width` filter); `course_asm_variables` 01–04 (a Variable in an assembly driving a
  mate's Offset Z).

**Not done in P3F.4:** mate limits and relation ratios don't take expressions (only offsets);
Suppress by variable stays disabled (see [inspection-and-repair-gaps.md](inspection-and-repair-gaps.md));
variables across documents (Variable Studios) are stage 3G.

## P3F.5 status
**P3F.5: done** (judge r2 8.65). Also closes the assemblies course's A1.7, A1.8, A6.3, X8 and
X16 simulation rows (see [intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md), "P3F.5
status").
- **Mesher** (`cadrs_fea::mesh`, our own, no Bevy; the risk below decided): the kernel's
  tessellation of a part is welded into one closed surface and **sampled** about the element
  size h apart (points along its edges and a lattice inside its larger triangles, each point
  knowing its CAD faces); an interior cubic lattice (h, at least 0.4 h from the surface) is
  added; the points are **Delaunay-tetrahedralized** (`cadrs_fea::delaunay`: Bowyer–Watson with
  ghost tetrahedra, so the mesh is exactly the hull of the points; exact `orient3d`/`insphere`
  from the `robust` crate, MIT/Apache-2.0, so degenerate grids and coplanar faces never make flat
  elements; a seeded biased-random insertion order, so meshes repeat) and **carved** by ray
  parity over the surface; slivers of one curved face's chords are peeled off; pieces hanging by
  less than a face are dropped. Boundary triangles are tagged with their CAD face (loads find
  them by it). A convex part is meshed exactly to its faces; a concave one to within the sampling.
  Every part of every course stand-in (89 parts in the fixtures: gear cover, hand brake, pneumatic
  cylinder, U-joint, conrod, flange, step stool, reflector, motor mount, …) meshes, the worst
  within 0.73 % of its kernel's exact volume (the mass properties; `course_parts_mesh_or_explain`
  asserts all 89 meshed and every one within 1 %, P3F.5 judge). A part that can't be
  meshed (an open surface, too thin for the element size) is reported in the panel.
- **Elements and solve** (`cadrs_fea::element`, `solve`): **10-node tetrahedra** (quadratic,
  exact 4-point integration), isotropic linear elasticity in mm, N, MPa; loads: Fixed (all nodes
  of the faces), a force spread uniformly over faces' area (consistent nodal loads: a 6-node
  triangle's corners get nothing, its mid-edge nodes a third each), a force along the faces'
  normals, a pressure; bonds (penalty ties, 10⁴·E·h, both ways: each body's surface nodes to the
  other's 6-node faces where they touch and face each other). The stiffness is assembled over the free
  degrees of freedom and factored by **faer**'s supernodal sparse Cholesky (MIT), in a geometric
  nested-dissection order (or AMD when that fills much less) on up to four threads, on a
  background thread with stage progress and Cancel; nodal stresses are averaged, von Mises from them. A model nothing holds,
  or a part free to move, is an error naming it.
- **Model** (`cadrs_core::simulation`): `Element::simulation` (saved with the tab, empty for old
  documents) holds the Loads list (`SimLoad`: Fixed; Force with N, direction normal / X / Y / Z
  and Flip; Pressure in Pa; faces by part and persistent face name) and the mesh density
  (Coarse, Medium, Fine), edited through `AddLoad`, `SetLoad`, `DeleteLoad`, `SetMeshDensity` (so
  they undo). `setup` builds the solver's model from the parts' current solids and **materials**
  (the library's Young's modulus and Poisson's ratio; every entry has them; **Steel - A36**, ASM:
  E 200 GPa, ν 0.26, is added for the course beam); the parts in play are those loaded and those
  bonded to them. Units: "100 N", "2.5 kN", "10 lbf"; "0.5 MPa", "250 kPa", "5 psi".
- **App** (`cadrs_app::simulation_ui`; the colour legend is `cadrs_ui::ColorLegend`): the right
  strip's Simulation button (icon-rs has no simulation icon: Thicken's deformed sheet stands in),
  the panel (Materials, Loads (n) with + Fixed / + Force / + Pressure and rows to edit or delete,
  Mesh, Solve and Cancel with a progress bar, errors in words, Results: Show von Mises or
  Displacement, Deformation undeformed / actual / exaggerated ×N, max displacement, max von Mises,
  min safety factor = tensile yield ÷ von Mises); the load dialog (Faces picked in the view and
  highlighted, Direction and Flip, the value with units); load glyphs in the view (ground
  hatching, force arrows, pressure arrows); results as vertex colours on the refined surface of
  the tetrahedral mesh, lit like the parts, with the parts hidden and their edges kept as the
  undeformed outline; the legend; the **probe** (hover: von Mises and displacement at the point,
  interpolated on the result surface). Results go out of date when the model, materials, loads
  or connections change.
- **Numbers** (derivations in `cadrs_fea/tests/acceptance.rs` and `cadrs_core/tests/simulation.rs`):
  the beam of "P3.5 simulation" above (100 × 10 × 10, E = 200 GPa, fixed at x = 0, 100 N down at
  x = 100; I = 833.33 mm⁴, PL³/(3EI) = 0.200 mm, M·c/I = 30.0 MPa):
  - `cadrs_fea` on the box: tip 0.20002 mm (+0.01 %), mid-span top-fibre σxx 30.03 MPa
    (+0.11 %), nodal von Mises there 30.01 MPa;
  - through the kernel (the beam modelled with a sketch and an extrude, the loads added by
    commands): tip 0.20037 mm (+0.19 %), σxx 30.01 MPa (+0.03 %);
  - two 50 mm halves bonded (in `cadrs_fea`: +0.05 % against the solid beam, the top fibre's
    von Mises at the seam 30.6 and 30.2 MPa on its two sides against 30.0; as an assembly with a
    Fastened mate with Simulation connection: +0.02 %); unbonded, "free to move"; apart, "don't
    touch";
  - a 1 MPa pressure and a 100 N normal force on a 10 mm cube's top: σzz −1 MPa;
  - **speed**: 24 058 elements (110 670 degrees of freedom) solve in 2.5 s off the main thread
    (mesh 0.1 s, assembly 0.3 s, factorization 2.1 s on four threads; the test takes the best of
    three runs against 10 s). The factorization uses at most four threads: on this 12-core
    machine, loaded by other work, one thread took 3.2 s, four 2.3 s and all twelve 9.4 s.
- **Scenarios**: `course_pcad_simulation` 01–08 (the beam; the panel; Fixed on x = 0; Force 100 N
  along −Z; the Loads list with glyphs; the von Mises map (Coarse mesh) with legend, max
  displacement 0.2006 mm; the probe at mid-span 30.2 MPa; Displacement with the probe near the
  tip); `course_asm_simulation` 01–08 (see the assemblies gap list).

**Not done in P3F.5:** loads on edges or vertices, remote loads, bearing and moment loads,
gravity, thermal loads; contact (sliding or separating) and mates as anything but bonded
(Onshape's revolute and slider connections keep their freedom; here every connected mate is
bonded); modal, buckling and fatigue analyses; mesh refinement near stress raisers and mesh
controls per face (one element size per solve); curved-boundary mid-edge nodes sit on the chord,
not the true surface; the solve isn't incremental (every Solve re-meshes).

**Also in P3F.5 (the P3F.3–P3F.4 judge's deltas):** an instance's or a part's Export… names the
file after it ("Shaft", an instance's number dropped) and says "Exporting: Shaft <1> (1 part)"
(`course_tips_export_assembly` 02–04; the tab's Export… still takes the tab's name); the large
studio notice says "251 features (excluding default geometry)", which agrees with "Features
(255)" (`course_tips_scale` 03); the assembly tab menu's Export… has the file-export icon (the
Part Studio's tab menu is text-only, as Onshape's); the Variable table shows each name in the
normal text colour with its description under it (`course_pcad_variables` 04,
`course_pcad_design_intent` 02, `course_asm_variables`).

## P3F.6 status
**P3F.6: done** (judge 8.45, all deltas minor). P3.6 and P3.7 are ✅.
- **Renderer** (`crates/cadrs_render`, no Bevy; MIT/Apache dependencies only: `rayon`, `image`):
  a CPU path tracer over the parts' tessellation.
  - A binned-SAH BVH; a metallic/roughness surface (Lambert under GGX with Smith masking and
    Schlick's Fresnel, the glTF/Bevy model), see-through appearances passing light through.
  - Lights: **procedural environments**, so no HDRI is bundled and no licence is involved:
    Studio (a grey dome, key, fill, rim and overhead soft boxes), Soft light (overcast), Outdoor
    (sky and sun), Sunset. Round area lights at infinity are sampled directly with multiple
    importance sampling against the surface's lobes; Russian roulette after three bounces.
  - **Ground shadow**: an infinite plane under the model that only catches shadows (the ratio of
    occluded to open light, smoothed over the ground), over the environment's backdrop, white,
    or a transparent background (a translucent black shadow).
  - **Accumulation**: one sample a pixel per pass on a thread pool; the random numbers of each
    sample come from (seed, pixel, sample), so **a fixed seed gives identical bytes whatever the
    thread count** (tested with 1 and 4 threads). An edge-avoiding à-trous denoiser guided by
    the first hit's normal, albedo and position; Khronos PBR Neutral tone mapping, which keeps
    appearance colours.
- **Model** (`cadrs_core::render`): `ElementKind::Render(RenderStudio)`: the source tab, the
  environment and its rotation, background, ground shadow, the view (a named view or "Current
  view", the angles captured from the source tab), perspective, size, samples, seed, denoise,
  exposure. Every change is a `SetRenderStudio` command (undoable); a Render tab refuses a
  drawing or itself as its source, and is refused as a link source. **PBR materials**: the
  face's appearance (face, feature, part) gives the colour and opacity; a metal material
  (aluminium, steel, stainless, iron, cast iron, brass, bronze, copper, titanium) makes it
  metallic with that metal's roughness, and shows the metal's own colour when no colour was
  chosen; plastics and unassigned parts are dielectrics (glossier for acrylic and polycarbonate).
- **App** (`cadrs_app::render_ui`): the "+" menu's Create Render Studio (the active or first Part
  Studio or Assembly); the Render tab's toolbar (Undo, Redo, **Render…**, Use current view) and
  left panel (`render-panel`: Model, Environment, Rotation, Background, Ground shadow, View,
  Perspective, Size, Quality, Exposure, Seed, Denoise, and the Materials as the renderer reads
  them); the view shows a live preview (16 samples at the viewport's size, denoised when done)
  and after a render its result, with a caption; the **Render dialog** (File name, Size,
  Quality, Folder); the render runs off the main thread with progress and Cancel in the panel
  and the view, then writes the PNG and a toast says where.
- **Export image…** (`cadrs_app::export_image`): from a Part Studio's or Assembly's tab menu and
  from the Exploded views panel. An offscreen camera with the viewport's orientation and centre,
  zoomed so everything the viewport shows fits, renders the view at the chosen size (up to
  8192 px); PNG (optionally transparent) or JPEG. The view's hover highlight isn't captured.
- **Tests**: `cadrs_render/tests/render.rs` (identical bytes for a seed across thread counts; not
  blank in all four environments; transparency; cancel; framing), `cadrs_core/tests/render.rs`
  (the Control Arm from a Render Studio tab: a 1920 × 1080 PNG written and read back, luminance
  variance > 0.005; the same seed, the same image; settings undo and round-trip through the
  file), `export_image` unit tests (the requested size and format written; the export scale
  shows what the viewport shows), and the goldens `golden_course_pcad_render` (the file is
  1920 × 1080 and not blank) and `golden_course_pcad_export_image` (each file has the size asked
  for; the transparent one is clear around the model).
- **Scenarios**: `course_pcad_render` 01–08 (the Control Arm, Part 1 in Aluminum - 6061 and
  Part 2 blue ABS; the "+" menu; the Render tab and its preview; Outdoor with the Current view;
  Soft light on white, Top, orthographic; the Render dialog; the render paused at 6 of 16
  samples; the result and its toast); `course_pcad_export_image` 01–09 (the tab menu; the
  dialog; 1280 × 720 PNG; a custom 800 × 600 JPEG; a transparent PNG; the Cylinder assembly's
  Exploded view 1 exported from the Exploded views panel at 1920 × 1080).

**Also in P3F.6 (the P3F.5 judge's deltas):**
- In an assembly's load dialog a picked face doesn't select its instance: no triad, no list
  row, no outline; the faces are outlined (`course_asm_simulation` 03–04).
- Cancelling the mate dialog clears the selection and the mate row (asm 05–08).
- `course_parts_mesh_or_explain` asserts 89 of 89 parts meshed and each within 1 % of the
  kernel's exact volume (worst 0.73 %).
- The load glyphs are hidden over the results.
- Show has **Safety factor** (tensile yield ÷ von Mises; red at the smallest factor, blue from
  five times it; the probe reads it).
- New frames in `course_pcad_simulation`: the Pressure dialog (06), a solve in progress (07, a
  scripted `sim-hold`), the safety factor (11), Deformation Actual (12) and Undeformed (13).
- The Loads header's buttons read "+ Fixed", "+ Force", "+ Pressure".
- Tab `Name`s are unique, so scenarios click tabs by name.

**Also in P3F.6 (the P3F.3–P3F.4 judge's deltas):** the sketches below a sketch being edited
(rolled back) aren't drawn (`course_pcad_design_intent` 05, `course_insp_error_states` 02–03); a
see-through Body shows the bore, grooves, piston and clamp following #piston_d
(`course_pcad_design_intent` 04, 06); `course_asm_variables` 02 has the mate dialog open with
"#depth" typed; the tab-strip and scale deltas are in [essential-tips-gaps.md](essential-tips-gaps.md).

**Not done in P3F.6:** image-based (HDRI) environments, textures and decals, depth of field,
refraction (a see-through appearance lets light straight through), render region, per-part
material overrides inside the Render tab (the materials come from the source), animation
renders; Export image… doesn't capture the view's overlays (dimensions, mate
glyphs).

## Risks
- **FEA is a large, specialist feature.** *Decided in P3F.5:* our own Delaunay mesher with exact
  predicates (no TetGen or Gmsh), faer's sparse Cholesky; see "P3F.5 status". Tet meshing of arbitrary B-reps is the hard part; OCCT's
  `BRepMesh` gives surface meshes only. Options: TetGen (AGPL, as a separate process only),
  Gmsh (GPL, separate process), or a Rust Delaunay tet mesher (e.g. built on `cgalrs` utilities).
  Decide at the start of P3F.5; the acceptance test only needs simple solids, but the panel must
  work on course parts.
- **Rendering quality vs effort.** A real path tracer is a project of its own; the plan accepts a
  high-sample Bevy PBR render with accumulation as "photorealistic enough", judged by eye.
  *Decided in P3F.6:* a small CPU path tracer (`cadrs_render`, about 1 000 lines) with a denoiser:
  a Full HD render of the Control Arm at 16 samples takes about 3 s on six threads, and it is the
  same on every machine (no GPU dependence, so the test can check the bytes).
- **Variables change evaluation order.** Variables are features with positions in the list; moving
  one after a use must error (P3.9 reorder + P3D.1 error states).
- **Dimetric/trimetric conventions.** Onshape's exact dimetric and trimetric angles aren't given in
  the course; the unit test checks the defining property (two equal / three different axis
  scales), not specific angles.

**Stage 3F passed (2026-09-30):** the essential-tips list is 28 ✅ and 2 out of scope; this list is 38 ✅ and 2 out of scope. The former 3G rows cite delivered 3G evidence. Minor deltas carried to Final:
- aluminium reads as grey paint under Studio (no reflectable highlights);
- the Render dialog's Quality overwrites the studio setting;
- Transparent background stays enabled for JPEG;
- the stale header on the simulation frame 11 (the minimum safety factor is 4.07);
- pin the mesh-test part count (== 89);
- pressure arrows bunch together;
- mixed legend precision;
- a Render tab inserted mid-strip;
- asm_variables 02 framing, and the tips_scale 04 edited-hole marker;
- the ps14 fillet width arrow's stand-off.
