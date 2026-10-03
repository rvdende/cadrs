# Simultaneous Sheet Metal: gap analysis (2026-10-01)

This maps [simultaneous-sheet-metal.md](simultaneous-sheet-metal.md) (`SM*`, exercises `E1`–`E4`,
`X*`) against cadrs `main` at `6b6fdf0`. ✅ done · 🟡 partial · ❌ missing · **out of scope**
(cloud, sharing, account and learning-site items; iOS/Android notes in the help; plus the
user's 2026-09-29 decision that **conic and curvature fillets** are niche and out of scope, which
covers SM11.2's Conic and Curvature controls).

**Summary:** nothing sheet-metal-specific exists. Derived explicitly treats sheet metal as static
(DV3.2) because "there is no sheet metal in cadrs". What cadrs already has that sheet metal can
build on is listed in the inventory below; the big new pieces are a **sheet metal definition model
with a flat-pattern solver**, the **Sheet metal model** feature and its 13 companion features,
the **table and flat view panel** with its own viewport, **flat-pattern drawing views**, the
**flat DXF/DWG export**, and the sketch **Insert DXF/DWG** import (a placeholder button today) that
exercise E1 starts with.

**Goal (from the user, 2026-10-01):** an Onshape user must be productive in cadrs sheet metal
immediately: the same tools in the same places, the same dialogs, defaults and table, the same
three synchronised views. Every phase below is **judged by a fresh agent from 0 to 10 and must reach
at least 8.5** (the orchestrator's builder → fresh judge loop, `docs/ORCHESTRATOR.md`, rubric
below).

## Code inventory (what sheet metal can reuse)

| Area | Where | State for sheet metal |
|---|---|---|
| Kernel: thicken faces, sketch regions or surfaces | `Kernel::thicken_surfaces` (`ThickenSpec`), feature `surfacing::ThickenFeature` | Makes the walls' solids; the wall layout itself is new. |
| Kernel: booleans, split, offset, sew, face tool, fillet/chamfer (incl. partial, asymmetric) | `cadrs_kernel` (`Kernel` trait) | Bends (cylindrical shells), reliefs (cuts), rips (gaps), corner breaks. |
| Kernel: face/edge/vertex info, adjacency, tangent chains, projection | `faces`, `edges`, `vertices`, `adjacent_faces`, `tangent_chain`, `project` | Convert (faces of a part → walls), Thicken with tangent propagation, flat outline for DXF. |
| Persistent naming | `naming`, `BodyNames` | Bend/rip identity for table rows, Modify joint and drawing bend notes. |
| Feature dialogs: tabs, collapsible sections, selection fields, dims, toggles | `cadrs_ui` (`feature_dialog`, `dialog_fields`, `selection_field`, `collapsible`, `tabs`, `switch`, `checkbox`) | All sheet metal dialogs. |
| Arrow manipulators, Up to entity end types, opposite-direction toggles | `extrude.rs`, `extrude_dialog.rs`, `revolve.rs` | Sheet metal Extrude, Flange depth/partial bounds, Jog. |
| Tables with editable cells, row menus, red invalid cells | `cadrs_ui::table`, assembly `bom_panel.rs`, `variables_ui.rs` | Bends / Other joints tables. |
| Right-edge panel toggles and docked panels | `panel_tab.rs`, `cadrs_ui::dock`, assembly BOM panel, PCB panes | The "Sheet metal table and flat view" panel. |
| A second 3D viewer with its own camera and view cube | `pcb/view.rs`, `view_cube.rs`, `viewport.rs` | The flat view. |
| Patterns and mirrors incl. **Face** pattern/mirror | `pattern.rs` (`PatternType::Face`) | SM12.2, once face patterns understand walls and bends. |
| DXF writer (and DWG via external converter) | `cadrs_core::dxf_export`, `cadrs_drawing::{dxf, dwg}`, `export_dialog.rs` | Flat pattern export (SM15), plus layers for bend lines. |
| DXF reader | `cadrs_drawing::dxf::read_dxf` | Sketch Insert DXF/DWG (E1); the sketch button is `B::Other(...)` today (`sketch.rs:1704`). |
| Drawings: Insert view dialog, view properties, view context menu, dimensions, notes | `crates/cadrs_drawing`, `cadrs_app/src/drawing/*` | Flat pattern views, bend notes, Show/hide bend lines (SM16). |
| Material, mass properties, Parts list menus | `material_dialog.rs`, `mass_props.rs`, `parts_list.rs` | Exercise validation steps (E1, E2). |
| Derived | `derived.rs` | Must carry sheet metal through (SM18.3). |
| Search tools, toolbar groups with dropdowns, feature-list icons | `search_tools.rs`, `document.rs` toolbar | X1, X2. |

## Status by requirement

| IDs | What | Status | Notes |
|---|---|---|---|
| SM1.1 | Multi-part studio, several models, one model → several parts | ✅ | P3I.2: one part per flat-pattern part, alongside ordinary parts. |
| SM1.2–SM1.4 | Three synced views, panel, cross-highlight | ✅ | P3I.3: right-strip toggle, docked panel (context dropdown, tables, flat view with its own camera and cube); rows ↔ model faces ↔ flat pieces select and hover each other, a model pick scrolls to its row. |
| SM1.5 | Collision check | ✅ | P3I.1 logic; P3I.2: the feature fails with "Collision in sheet metal flat pattern" (red, tooltip, dialog line), keeping its context for the flat view. |
| SM1.6 | Active-model behaviour | ✅ | P3I.2: contexts record their parts and `active`. Integration: **one definition and one pipeline** for every feature after the model (see Decisions (integration)): Flange, Hem, Make joint and Modify joint edit the walls at their virtual sharps; Bend, Jog, Tab, cuts, corner breaks, face copies, Corner, Bend relief and Loft (Add) are ordered steps replayed on the built model; forms are kept with the model and applied again at every refold; parts keep their ids. Extrude → Remove whose targets are all active sheet metal cuts the walls perpendicular and shows in the flat; Extrude Add with a merge scope of active sheet metal is refused (use Tab or Flange). Ordinary features (fillets of non-corner edges, booleans, …) still act on the folded part only and are lost at the next refold. |
| SM2.1–SM2.7 | Sheet metal model (Convert / Extrude / Thicken; General / Material / Relief) | ✅ | P3I.2: dialog, rebuild and folded solid; see Decisions (P3I.2). |
| SM2.8 | One model per part; rename names the context | ✅ | Contexts are keyed by the feature; P3I.3's context dropdown lists the features by name. |
| SM3.1–SM3.7 | Flange (alignment, end types, angle control, miter, model radius, partial flange) | ✅ | P3I.4: see Decisions (P3I.4). Per chain finds the chains of edges meeting end to end and bounds only their free ends (fix round). Open: a Per chain bound that goes Up to an entity is measured on the first edge. |
| SM3.8 | Move face on a flange | ❌ | Move face (direct edit) doesn't exist in cadrs; not required by the exercises. |
| SM4.1–SM4.5 | Hem (Straight / Rolled / Tear drop, alignment, corner type) | ✅ | P3I.4: closed-form flat lengths tested; the last values are remembered for the app session (not across sessions). Hems are in the Bends table (P3I.3's table shows them). |
| SM5.1–SM5.4 | Tab (profiles, flanges to merge, subtraction scope/offset) | ✅ | P3I.5: profiles added to parallel walls they touch (auto when *Flange to merge* is empty; bridged coplanar walls merge), clearance pockets (profile grown by the offset) cut from sheet metal walls or ordinary parts in the scope; in the flat. |
| SM6.1–SM6.3 | Make joint | ✅ | P3I.4: rip (edge / butt 1 / butt 2, butt only at 90°) or bend (model or own radius) between two flat walls' edges. |
| SM6.4 | Modify joint | ✅ | P3I.3: made by table edits, edited from the feature list (Joint, Bend/Rip/Tangent, rip style, model radius, model K → calculation + value, red out of range). |
| SM7.1–SM7.3 | Corner | ✅ | P3I.5: a face, edge or vertex picks the nearest corner (`FlatCorner`); its `CornerOverride`. |
| SM8.1–SM8.3 | Bend relief | ✅ | P3I.5: the nearest bend end; its `BendReliefOverride` (all five types, Extend bend relief). |
| SM9.1–SM9.7 | Bend | ✅ | P3I.5: `cadrs_sheetmetal::model_edit::bend_wall`: line projected and extended, face filled in, the smaller side moves (Hold opposite side swaps), six alignments, angle / Align to geometry / Angle from direction, custom radius and K; the flat never changes size (tests). A line across cut-outs makes one bend per stretch. |
| SM10.1–SM10.2 | Finish sheet metal model | ✅ | P3I.5: marks the models of the picked parts finished (warning in the dialog); later features act on the solids and don't touch the flat; rolling back, suppressing or deleting it makes them active again (tested). |
| SM11.1, SM11.3, SM11.4 | Corner break (fillet radius/width, chamfers, table lock) | 🟡 | P3I.5: corner edges or vertices in the folded view; Fillet Radius/Width (Distance), Chamfer Offset/Tangent with all three types; in the wall outline, so in the flat; `SheetMetalContext::corner_broken` for the table's lock (P3I.3 to read). Gaps: picks in the flat view (P3I.3/6), corners made by relief cuts, Asymmetric and Allow edge overflow; Tangent measures like Offset; the round is a polyline (7.5° steps). |
| SM11.2 (Conic, Curvature) | Corner break conic/curvature fillets | **out of scope** | niche; out of scope by user decision 2026-09-29. Radius/Width with Distance control and Asymmetric stay in scope. |
| SM12.1–SM12.3 | Other features on sheet metal (perpendicular cuts, fillets, part/face patterns and mirrors) | 🟡 | P3I.5: Extrude → Remove (perpendicular, in the flat), Fillet/Chamfer of corner edges (as corner breaks), Face pattern and Face mirror of walls with their bends. Part pattern copies the solid as ordinary parts (not added to the model); cuts across bend regions aren't taken out of the bends. |
| SM13.1–SM13.5 | Bend and joint table | ✅ | P3I.3: Bends / Other joints with carets; double-click radius and calculation cells; Move up/down, Convert to rip/bend; rip Type and Style selects; hems reorder only; rows toggle and multi-select. |
| SM14.1–SM14.4 | Modeling in the flat view | 🟡 | P3I.6: flat pattern planes (sketches in the flat's coordinates), the abbreviated Extrude (Add / Remove) editing the definition in the flat (cuts wrap across bends at their exact flat size, tabs grow walls), the SM14.3 error, visible flat sketches in the DXF. New sketch is on the Sheet metal model's and its parts' context menus until the flat view (P3I.3); in 3D the flat plane lies on the anchor wall. Integration: the flat view's right-click menu offers New sketch (`flat_ui::begin_flat_sketch`), Export DXF/DWG of flat pattern and Create drawing of flat pattern, as Onshape's, for the part under the hovered bend (else the model's first); sketches on the flat are drawn in the flat view; the flat extrude is a step of the unified definition (`StepEdit::Flat`), so it survives later features and reorders (scenario `sm_p3i8_flat_view`). |
| SM15.1–SM15.3 | Flat DXF/DWG export | ✅ | P3I.6: Export as DXF/DWG (file name, DXF/DWG, version 2000/2013, the three scopes, Download into a folder, the eight options); layers OUTLINE, CUTOUTS, TEAR_SLITS, BEND_UP, BEND_DOWN, BEND_TANGENT, FLAT_SKETCH; several parts side by side or one file each. Export rules, email and store-as-tab: the existing export dialog has none. |
| SM16.1–SM16.6 | Drawings of flat patterns | ✅ | P3I.7: flat pattern views from Insert view → Flat patterns (`cadrs_drawing::flat_view`, `cadrs_core::flat_drawing`, `cadrs_app` `drawing/flat_views.rs`): outline, holes as circles, tear slits, tangent edges (Hidden/Solid/Phantom), bend lines with up/down pens (View properties), bend notes ("DOWN 90.0° R1.5") that drag off with a leader and reattach when dropped near their line, Show/hide bend lines and notes, dimensions on named flat edges, projected (edge-on) views, Update with the model, DXF layers BEND_UP/BEND_DOWN. Create drawing of flat pattern: `flat_views::open_create_drawing_of_flat` (Parts list menu; the P3I.3 flat view menu should call it). Not yet: form outlines and centermarks, counterbore/countersink outer diameters (no forms or holes in the flat yet), arcs in outlines as arcs (they are line segments). Scenarios `sm_p3i7_e3`, `sm_p3i7_options`. Integration: the flat view menu calls `open_create_drawing_of_flat` (`sm_p3i8_flat_view`, `sm_e3`). |
| SM17.1–SM17.3 | Legacy import via Thicken + tangent propagation + bend cylinders | ✅ | P3I.8: Tangent propagation follows faces that meet smoothly (same plane, or a bend cylinder and the flats beyond it: mesh normals across the shared edge), so one pick takes an imported folded part's whole skin; picked cylinders become bends of their radius, unpicked ones rolled walls with tangent joints (SM17.2); thickness, radius, flip; Delete part of the import. Stand-in: our C-channel exported as plain STEP (`samples::sheetmetal_legacy`); tests: the same volume, flat and bends as the original (`tests/sheetmetal_legacy.rs`); scenario `sm_p3i8_legacy`. |
| SM18.1–SM18.4 | Top-down design (Derived master model, contexts named after features) | ✅ | P3I.8: the Heating Mantle stand-in (`samples::sheetmetal_topdown`, `fixtures/sheetmetal/sm_topdown_standin.cadrs`): Space Envelope derived at the workspace, Enclosure (Convert, two faces excluded, Keep input part) and Cover (Thicken of those faces), the models renamed after their parts; the context dropdown lists every context by its feature's current name; editing the master updates both (tests, `sm_p3i8_topdown`). **Derived carries sheet metal**: a derived sheet metal part brings its context (table, flat, definition): at the source's own place it stays active (later features and table edits refold it in place), placed elsewhere it comes as finished sheet metal with its flat. In-context studios (SM18.4) aren't sheet-metal-specific. |
| SM19.1 | Jog | ✅ | P3I.5: two opposite bends sized for the offset (Blind, Up to entity with offset, Thickness factor; anchors Inside/Nominal/Outside), Preserve material off stretches the sheet so the far end stays (tests). |
| SM19.2 | Sheet metal Loft | ✅ | P3I.9: New/Add (one active model), Profile 1/2 (region, face, edges, point), Connections with draggable handles and Rip, Chordal tolerance, General/Material/Relief; planar facet walls along the tessellation (facet joints, steep non-fanning edges bent), a closed loft ripped at its matched start, mitred folded walls; flat × T matches the folded volume (tests). |
| SM20.1–SM20.3 | Form, Tag (Form), forms library | ✅ | P3I.9: Tag (Form) (add/remove parts, flat sketch, origin connector); Form with Select Part Studio (Current document / Other documents / Libraries), the form's variables (Variable features; `thickness` driven by the model), locations (sketch points, a sketch's points, vertices, mate connectors), target faces, opposite direction; touching joints, rips, corners or edges is an error; outlines and centermarks in `FlatPart.forms`. cadrs's own library: louver, bridge lance, dimple, emboss, extruded hole (`samples::sheetmetal_forms`). Integration: forms are kept with the model and applied again at every refold, placed relative to their wall (they follow it); their outlines and centermarks show in the flat view (`sm_p3i9_form` 09). |
| E1–E4 | Exercises | ✅ | P3I.8: stand-ins built by `samples::sheetmetal_exercises` (fixtures under `fixtures/sheetmetal/`, kept current by `tests/sheetmetal_exercises.rs`) and scenarios doing each end to end: `sm_e1` (Insert DXF, Thicken of 7 regions, 6 Inner Bends, Carbon Steel: **0.356733 kg**), `sm_e2` (Extrude, flanges, partial flange, hem, tab, Flange 2 a 10 lip on the right wall's top edge, Flange 3 10 on its vertical front end edge, Make joint butt 1 between the lip's end and Flange 3's top edge as the slides' steps 9–11, Corner Round – Sized 3.3, Carbon Steel: **0.111319 kg**, the panel shows 14180.808 mm³ as the test), `sm_e3` (flat DXF export from the flat view menu, Create drawing, four views, flat view with bend notes, dimensions: flat **500.833 × 425.833**), `sm_e4` (Finish, the rework with sketch/plane/sweep/mirror/fillets; the flat under Context "Lower Enclosure" unchanged, tested). E2's R35 arc is a trapezoid of lines (Extrude bends only between lines); its tab sketch is on Top, not on the face; Flange 3 stands on the wall's end edge between its bends' tangent lines (the edge the folded part shows), so its foot is open to the base as in the slide. |
| X1 | Toolbar group, Search tools | 🟡 | P3I.2: Sheet metal model button + ▾ with the 12 other tools in Onshape's order (greyed until built), all in Search tools; the table/flat view toggle is P3I.3. P3I.9: Loft, Form; P3I.4: Flange, Hem, Make joint; P3I.5: Finish, Tab, Bend, Jog, Corner, Bend relief and Corner break enabled in both. All twelve tools are built and enabled. |
| X2 | Feature-list icons | 🟡 | P3I.2: Sheet metal model; P3I.4: Flange, Hem, Make joint; P3I.5: its seven features; P3I.9: Loft, Form, Tag; the others come with their features. All sheet metal features have their icons. |
| X4, X5, X6 | Undo, units, errors | ✅ (for P3I.2) | Every edit a command; lengths in the document unit, scales unitless; out-of-range fields red with the range tooltip; errors red with tooltip. |
| X3, X7 | Parts list, stand-ins | ✅ | P3I.8: sheet metal parts rename, take a material, measure and export like any part, through refolds (they keep id, name and material; `tests/sheetmetal_topdown.rs`); the Parts list menu has Create drawing and Create drawing of flat pattern (P3I.7). Stand-ins and scenarios: see E1–E4. Carbon Steel added to the material library. |
| Forms course lessons | "Creating a Tag (Form)", "Form Feature" | not read | Paid Learning Center content; the help page stands in. |
| Quiz, completion survey | | **out of scope** | Learning-site features. |

## Design (proposed)

- **A new crate `cadrs_sheetmetal`** (no bevy, like `cadrs_core`/`cadrs_sketch`): the sheet metal
  **definition** of one Sheet metal model: **walls** (planar, cylindrical "rolled" and conical
  faces of a mid- or side-surface), **joints** on the edges between walls (**bend** with inner
  radius, angle, direction and its K factor / allowance / deduction; **rip** with style and gap;
  **tangent** for rolled walls), **corners** and **bend ends** with their relief types, plus
  **hems**, **tabs** and **forms** as wall attachments. Pure data and maths, unit-testable without
  OCCT.
- **Flat pattern solver** in that crate: pick a fixed wall, walk the joint graph (bends as tree
  edges, rips as cuts), unroll rolled walls with the rolled K factor, and lay each wall flat by
  rotating it about its bend's axis with the bend region's flat width `BA = θ·(R + K·T)` (or the
  table's allowance / deduction). Output: flat outlines with holes, bend centerlines and tangent
  lines with up/down and labels, and a **collision check** (overlapping walls → the SM1.5 error).
  Closed-form tests: an L, a U-channel, a box with rips, a hem, a rolled cylinder (`2π(R + K·T)`).
- **Folded solid** (in `cadrs_core::rebuild` through `cadrs_kernel`): thicken each wall by the
  thickness on the chosen side, add each bend as a cylindrical shell segment of the inner radius,
  trim at rips with the minimal gap, cut corner and bend reliefs, then union per part. Persistent
  names come from the definition (wall and joint ids), so the table, cross-highlighting, Modify
  joint and drawing bend notes stay stable across rebuilds.
- **Active vs finished**: features after a Sheet metal model edit its **definition** (Flange adds a
  wall + bend; Extrude → Remove on an active model becomes a perpendicular cut through the wall in
  the definition, so the flat updates); Finish sheet metal model freezes the folded solid, and later
  features act on it as an ordinary part while the flat stays as it was.
- **Flat view**: a second viewer like the PCB one (own camera, view cube, picking) showing the
  flat solid (or the outlines extruded by the thickness), with the table above it in one docked
  panel. Cross-highlighting through shared persistent names.
- **Kernel growth** only where needed (e.g. a robust "thicken with exact side faces" or
  "cylindrical shell between two planar walls"), each with a conformance test, recorded in
  `docs/KERNEL.md`; OCCT bindings, if missing, go into the `rvdende/opencascade-rs` fork.

## Icons (to add to icon-rs)

None of these exist in icon-rs yet (`~/work/icon-rs/icons`, 215 icons). Add them through the
generator (`tools/icons/*.py`, solid style for features, line style for the panel toggle), never
by hand, then bump icon-rs, push and publish (as for earlier icons). **Never copy Onshape's
artwork**; draw our own from the shapes the features make.

| Name | For | Kind |
|---|---|---|
| `sheet-metal-model` | Sheet metal model (toolbar group head) | solid: a thin folded L/U sheet |
| `sheet-metal-finish` | Finish sheet metal model | solid: folded sheet with a check/stop accent |
| `sheet-metal-flange` | Flange | solid: base sheet with a raised wall, accent on the wall |
| `sheet-metal-hem` | Hem | solid: edge folded back over itself |
| `sheet-metal-tab` | Tab | solid: wall with a tongue added |
| `sheet-metal-bend` | Bend | solid: flat sheet with a bend line, one side lifted |
| `sheet-metal-jog` | Jog | solid: Z/S-offset sheet |
| `sheet-metal-form` | Form | solid: sheet with a louver/dimple |
| `sheet-metal-loft` | Sheet metal loft | solid: transition duct |
| `sheet-metal-make-joint` | Make joint | solid: two walls meeting, accent at the seam |
| `sheet-metal-modify-joint` | Modify joint (feature list) | solid: bend with an edit accent |
| `sheet-metal-corner` | Corner | solid: corner with a round relief |
| `sheet-metal-bend-relief` | Bend relief | solid: bend end with a slot relief |
| `sheet-metal-corner-break` | Corner break | solid: sheet corner rounded |
| `sheet-metal-table` | Sheet metal table and flat view panel toggle | line: table above a flat outline |
| `flat-pattern` | Flat pattern (drawing Insert filter, flat views, context menus) | line or solid: unfolded cross shape with dashed bend lines |

## Proposed milestones

| # | Milestone | Covers | Acceptance |
|---|---|---|---|
| P3I.1 | **Sheet metal definition and flat-pattern solver** (`cadrs_sheetmetal`) | SM1.5 (logic), SM2.6, SM2.7 (relief maths), SM13.1 (data) | Unit tests: closed-form flat lengths (L, U, box with rips, hem, rolled cylinder) for K factor, allowance and deduction; collision detection; relief geometry for all 6 corner and 5 bend relief types. |
| P3I.2 | **Sheet metal model feature + folded solid + toolbar group** | SM1.1, SM1.6, SM2.1–SM2.8, X1, X2, X5, X6, icons | Scenarios: Convert of a block (6 walls, bends picked in two orders → two flats), Extrude of an open sketch with an arc (rolled and as-bend), Thicken of regions; dialogs match `sheetmetal-dialog-convert-02.png` etc.; undo/redo; collision error. |
| P3I.3 | **Table and flat view panel; Modify joint** | SM1.2–SM1.4, SM6.4, SM13.1–SM13.5 | Scenarios match `sheetmetalflatpatterntable-02.png` and the lesson frames: context dropdown, both tables, flat viewport with view cube and centerlines, bend labels, cross-highlighting both ways, radius/K edits (red out-of-range cell), convert bend↔rip, styles, move up/down; each edit an undoable Modify joint feature. |
| P3I.4 | **Flange, Hem, Make joint** | SM3.1–SM3.7, SM4.1–SM4.5, SM6.1–SM6.3 | Scenarios per dialog image (incl. partial flange manipulators); exercise E2 steps 1–6 and 9–11 on its stand-in. |
| P3I.5 | **Tab, Bend, Jog, Corner, Bend relief, Corner break, Finish; sheet-metal-aware cuts, fillets, patterns** | SM5, SM7–SM12, SM19.1 | E2 complete (mass matches the stand-in's expected value); Finish + rework of E4; Bend on a flat. |
| P3I.6 | **Flat-view modelling, flat DXF/DWG export, sketch Insert DXF/DWG** | SM14, SM15, E1 import | E1 complete: DXF imported into a sketch (mm), Thicken of 7 regions, 6 Bends (Inner), Carbon steel mass; exported DXF re-read and compared with the flat. |
| P3I.7 | **Flat pattern drawings** | SM16.1–SM16.6 | E3 complete: Create drawing (ANSI_A MM, Four views), Insert sheet, Insert view → Flat patterns, bend notes dragged and hidden, bend lines show/hide, tangent edge styles, dimensions. |
| P3I.8 | **Legacy import, top-down, exercise stand-ins** | SM17, SM18, X3, X7, E1–E4 | Imported STEP enclosure → Thicken + tangent propagation + bend cylinders; Derived master model updating Enclosure and Cover; all four exercises as stand-ins with scenarios. |
| P3I.9 | **Sheet metal Loft, Form, Tag and a cadrs forms library** (later) | SM19.2, SM20 | Loft between a rectangle and a circle; a louver form placed on sketch points, shown in the flat and DXF. |

## Judging (each milestone)

The orchestrator's loop (`docs/ORCHESTRATOR.md`): a builder implements the milestone, then a
**fresh** judge agent scores it **0–10**; below **8.5** the top deltas go back to a builder and a
new judge scores again (at most 2 fix rounds; ≥ 8.3 passes only when every remaining delta is
minor). The judge must actually Read the images: our headless scenario PNGs side by side with the
course frames (`<lesson>/t*.png`), the exercise slides (`ex*/step-*.png`) and the help dialog
images (`help/feature-tools/*`), and check every covered `SM*` ID against the requirement text.
Rubric (from `docs/PLAN.md`):

| Criterion | Weight |
|---|---|
| Interaction and workflow parity (same clicks lead to the same result and feedback) | 35% |
| Feature completeness for the scenario | 25% |
| Layout and visual fidelity (positions, proportions, colours, typography) | 25% |
| Polish (hover states, cursors, highlights, animation, no glitches) | 15% |

Icon artwork that differs (ours vs Onshape's) is not penalised. For sheet metal the judge also
checks **numbers**: flat pattern sizes against the closed-form values, and the exercises' masses
against the stand-ins' expected values.

## Decisions

- Phase letter **P3I** (next after P3H, PCB Studio).
- Units default to mm; dialog defaults follow Onshape's help (K 0.45, rolled K 0.5, scale ranges),
  with lengths converted to the document unit (the help screenshots' inch values are examples).
- The forms library is cadrs's own (P3I.9); Onshape's library and its configured forms are not
  copied.
- DWG output keeps the existing external-converter approach (no DWG library linked).
- Move face (direct edit), used by the help to adjust flanges and jogs, isn't in cadrs; it isn't
  needed by any exercise and stays a separate future item.

## Risks

| Risk | Mitigation |
|---|---|
| Robust folded solids from many walls and bends in OCCT (thin-wall booleans, tolerance) | Build per wall and per bend, union once; conformance tests on boxes, hems and rolled walls; the definition (not the solid) is the source of truth for the flat. |
| Convert's bend pick order decides the flat; easy to get subtly different from Onshape | Treat pick order as the spanning-tree order explicitly; scenario with two orders. |
| The flat view doubles rendering and picking work | Reuse the PCB viewer setup; render the flat as one mesh per part. |
| Exercise documents are Onshape public docs we don't have | Stand-ins built by `cadrs_core::samples` from the slides' dimensions; E1 needs our own flat DXF. |
| Forms course lessons unread (paid) | Help page covers the feature; Forms are the last milestone. |

### Decisions (P3I.2)

- **Construction** lives in `cadrs_sheetmetal::construct` (no kernel): Convert/Thicken make one wall per
  planar face offset by the clearance on the material side; the **pick order is the spanning-tree
  order** (a picked edge whose walls bends already join stays a rip and is reported in
  `loop_picks`); unpicked shared straight edges are rips (edge joints). Cylinders (fillet rounds)
  picked to bend become a bend of the cylinder's radius between their flat neighbours; unpicked,
  a rolled wall with tangent joints. Other curved faces are left out with a warning.
- **Convert default side**: the material goes outward (the sheet encloses the part); the
  Thickness arrow puts it inside. *Include bends* moves walls out by `r − (r − c)·cos(θ/2)` so the
  bends' inside clears the input edge by the clearance. Thicken's material goes along the face or
  sketch normal.
- **Extrude**: each line a planar wall, touching lines bend with the model radius, collinear lines
  merge, arcs roll (tangent joints) or, picked in *Arcs to extrude as bends* and sitting between
  two lines, become a bend with the arc's inner radius. The default material side is the side the
  chain turns toward (inside a U), left for a straight chain. A closed chain rips where its first
  and last lines meet. Splines and ellipses are refused (error). Up to next/face/part/vertex are
  resolved to a depth along the extrude direction (Up to next: the nearest part point ahead —
  an approximation of Onshape's face-shaped end).
- **Folded solid** (`rebuild/kernel_ops/sheetmetal.rs`) composes existing kernel ops (extrude and
  boolean): planar walls extruded by T, rolled walls and bends as annular sectors extruded along
  their axes, bend relief cuts as wedges of their (along, across) extent (exact for the
  rectangular corner cuts; an approximation for obround/round cuts on a bend region), fused per
  flat part. A fused volume short of the pieces' sum fails the feature ("Sheet metal walls
  intersect") — the 3D intersection check the P3I.1 judge asked for. No new kernel operation, so no
  new conformance case.
- **Numbers checked**: folded volume = Σ walls' flat area × T + Σ bends' flat area × T·(R + T/2)/(R + K·T),
  to 1e-6 relative (`cadrs_core/tests/sheetmetal.rs`), plus closed forms for the block box and the
  extruded hook (rolled and bent give the same sheet).
- **Persistent ids**: walls and joints are keyed by the hashes of the input face, edge and curve
  names (`SheetMetalContext::{wall_keys, joint_keys}`); joint names follow Onshape ("Bend A",
  "Joint B", … one letter sequence).
- **Defaults**: thickness 1 mm, bend radius 1 mm, K 0.45, rolled K 0.5, minimal gap 0.2 mm,
  Simple corner relief, Obround – Scaled bend relief with depth and width scales 2 (as the help
  dialogs show).
- **Toolbar placement**: after Variable, before custom features, as the lesson frames show
  (`02-sheet-metal-model/t0072.0.png`). The icon starts the feature, its own ▾ opens the menu.
- **Dialog**: all four sections open by default (as the help dialogs); their state is kept while
  the dialog is open. While *Faces to exclude* or *Edges or cylinders to bend* is active the view
  shows the parts before the feature, so the consumed part's faces and edges can be picked (as
  the chamfer's Direction overrides do). A new model with nothing picked shows a red
  *Selections* header rather than an error line.
- **Gaps left**: flat view and table (P3I.3); perpendicular cuts on active models (P3I.5); rips
  between planar and rolled walls aren't built (the arc ends of a rolled wall stay unjoined);
  Up to next is approximate; tangent propagation joins flat coplanar faces and cylinders only.

### Decisions (P3I.3)

- **Modify joint rebuilds the model** from a *recipe* kept with its context (the Convert/Thicken
  faces, edges and cylinders, or the Extrude chains; `cadrs_sheetmetal::edit::Recipe`), with the
  joint edits applied to the walls at their virtual sharps before trimming, so a new radius moves
  the tangent lines and a rip leaves the minimal gap. The parts are refolded **in place** (same
  part ids, named with the model's operation) so later features keep their references.
- **Where a new Modify joint goes**: right after the Sheet metal model and its other Modify
  joints, not at the end of the list as Onshape puts it: in cadrs ordinary features after the
  model act on the folded part, so they must come after the joint change. A second edit of the
  same joint edits its Modify joint (one per joint).
- **Move up / Move down** reorder the table only (manufacturing order): kept on the Sheet metal
  model feature (`table_order`), one undoable command, not a Modify joint.
- **Out-of-range table values** (K outside −1.5..1, allowance ≤ 0, deduction < 0) are kept in
  the Modify joint, which fails with the range; the cell shows the typed value red with the
  range as its tooltip, until undone or corrected.
- **Picking a joint** in the model is geometric (`cadrs_sheetmetal::view::joint_at`): a point of
  a bend's region, or of a rip's two side faces, with a tolerance for the tessellation.
- **Flat view**: the flat-pattern parts laid side by side along X, seen from the top (Flip
  direction up shows the other side), drawn as a thin solid with the outline, dashed centrelines,
  tangent lines and bend labels; it reuses the Repair panel's second-camera set-up (own image,
  own cube) rather than the PCB view, which draws through the main camera.
- **Gaps left**: labels float next to bends in the flat view only (not in the folded view);
  Tangent can't be set on a joint that isn't tangent; a bend's own value of a Bend feature (P3I.5)
  and hems' values aren't editable; the flat view doesn't show sketches on the flat (P3I.6).

### Decisions (P3I.9)

- **Loft layout**: the profiles are cut by the chordal tolerance (arcs) and joined by the strip
  of least area between connections (a rectangle's sides each meet one circle point, its corners
  fan): the classic square-to-round. Coplanar neighbours make one planar wall; walls meet at
  **facet joints** (a `JointKind::Tangent` between two planar walls: no bend region, laid edge to
  edge flat, mitred in 3D). An edge where walls meet at 30° or more, sharing no end with another
  such edge, becomes a **bend** of the model radius (a frustum's corners); fanning bends would
  overlap at their common point, so fans stay faceted. A closed loft rips at its first connection
  (the matched start) unless a connection is ripped. Connections are stored as positions along
  each profile (0..1 of its length), which the view's handles drag.
- **Folded loft walls** are mitred slabs made by the kernel's existing `mesh_solid` (no new kernel
  operation, so no conformance case), fused per part; bends reuse the model's shells.
- **Forms**: cadrs has no configurations, so a form's "configuration variables" are its Part
  Studio's Variable features, overridden by the Form feature for its copy (`thickness` follows the
  model). Library forms are generated from their variables (`samples::sheetmetal_forms::studio`).
  The add parts are united before the remove parts are cut. A location is projected onto its
  target face (Z out of the face; the opposite direction places it from the other face, pointing
  the other way). The footprint (the tool parts' hull seen along Z) must keep clear of every
  joint segment and free edge of its wall.
- **Gaps left**: the flat view panel (P3I.3) isn't in this branch, so the flats with form outlines
  are rendered by a test (`target/scenarios/sm_p3i9_flats`); forms applied to the folded part
  aren't re-applied if a later feature re-folds the model from its definition (the SM1.6 hook);
  form previews in the picker; Other documents lists stored documents' form studios (no version
  picker); the form's sketch visibility toggle in the flat view.

### Decisions (P3I.4)

- **Later features edit the definition** (SM1.6): the Sheet metal model keeps the definition it
  was built from (`cadrs_sheetmetal::sharp_edit::SharpDef`: the walls at their virtual sharps,
  their joints and hems, plus the rolled walls and tangent joints added after the build) in its
  context. Flange, Hem and Make joint add walls and joints to it; `Rebuilder::edit_sheet_metal`
  (`rebuild/kernel_ops/sheetmetal_features.rs`) builds the model again, checks it (3D, flat,
  walls intersecting), folds it and puts the parts back under their ids. P3I.5/P3I.9 can use the
  same hook. Faces are named by the feature that added their wall or bend ("Edge of Flange 1").
- **Picks**: an edge or side face of a flat wall's free edge. The side it is on sets the default
  direction: a flange or hem turns towards the face whose edge was picked (a side face: towards
  the material); the arrows flip it.
- **Flange**: Distance from the outer virtual sharp to the tip; the alignment puts the outer
  sharp `T·tan(θ/2)` (Inner), `T·tan(θ/2)/2` (Middle), 0 (Outer) or `(R+T)·tan(θ/2)` (Hold line)
  past the edge. Up to entity measures along the flange to a plane (planar face or plane) or a
  point (vertex, edge midpoint, face centre). Align to geometry: parallel to a line, or lying in a
  plane; Angle from direction: the direction turned about the edge. Automatic miter: flanges of
  one feature meeting at a corner get an edge-joint rip where their planes meet, or, in one plane
  (two walls joined by a bend), a cut along the corner's bisector the minimal gap apart; off: each
  end cut at the miter angle. Partial flange: bounds in from the picked edge's ends (Blind, Up to
  entity [+ offset]); Hold adjacent edges keeps the rest of the edge in place.
- **Hem**: Straight (180°; Flattened = inner radius half the minimal gap), Rolled (the bend
  only; a 0.001 mm leg because every bend needs a wall after it), Tear drop (`β`, `ℓ` solved so
  the leg ends the gap off the wall and Total length from the outermost point). Corners: Simple
  cuts both legs on the corner's bisector, Closed carries them on to it first.
- **Make joint**: both edges carried to where the walls' planes meet, then a rip or bend there.
- **Fix round (P3I.4 judge, 8.4)**: E2's steps 9–11 as the slides (a lip on the right wall's
  top edge, a flange on its vertical end edge, Make joint between their edges, butt 1). A flange
  on a wall's side edge spans the stretch the folded part shows (between the bends' tangent
  lines). Per chain: chains found by shared ends; the first free end met (in pick order) takes the
  first bound, the chain's other free end the second. A partial flange's bounds follow the pick's
  own direction (an Up to vertex bound went to the wrong end when the wall's outline ran the
  other way). Align to geometry takes the parallel on the picked face's side (the arrow the
  other); Angle from direction turns away from the wall, whichever way the edge runs. Automatic
  miter off: the miter plane runs through the corner's outside at the miter angle to the first
  flange; each (square-ended) flange stops where its inside meets it (the cut used to lean away
  from the flange and changed nothing). Hems on
  flanges mitred at a box corner: the later hem stops `2R + T` plus half the gap clear of the
  earlier one (Simple and Closed alike: bend regions end square); a hem's corner cut is where
  its plane crosses the leg's and keeps the leg's whole thickness on its side. Simple hems on one wall's corner: both
  legs are cut on the corner's bisector, half the gap off it (they used to be cut only at the
  other's bend, so they crossed: "walls intersect"); Closed carries them on first.
  The last hem is remembered only when a new
  hem is accepted (not its flip); the hem has a flip arrow in the view. Onshape shows the red "!"
  and a red name while a new feature has nothing picked (`03-flange/t0012.6.png`), as cadrs does.
- **Fix round 2 (judge 8.54)**: a wall's relief cuts are snapped back onto its exact edges (the
  polygon booleans round to 1e-6 mm, which left an oblique edge off its bend's face, so a Flange
  on the sloping edges of a sloped enclosure came out as separate parts); Make joint carries a
  bend that ends at the moved edge on with it (no square corner under the lip in E2); Simple hem
  corners cut both legs on the bisector; a miter-angle end that runs on moves its own corners
  (one straight end); Partial flange is greyed until an edge is picked.
- **Gaps left**: Move face (SM3.8); hems and flanges only on flat walls' edges (not on rolled
  walls or hem legs); the E2 stand-in replaces the R35 arc by lines (the sheet metal Extrude bends
  only between lines); Per chain Up to bounds measure on the first edge; the 12 px labels are in
  the sheet metal feature dialogs only (other dialogs' label columns are sized for 11 px).

### Decisions (P3I.5)

- **One feature kind** (`FeatureKind::SheetMetalTool`, `cadrs_core::sheetmetal_tools`) for Finish,
  Tab, Bend, Jog, Corner, Bend relief and Corner break; their dialogs in
  `cadrs_app::sheetmetal_tools_ui` (own row component, observers and sync), hooked into the
  applied-feature session.
- **Edit and refold**: every sheet metal feature after the model edits a copy of the active
  model's definition (`cadrs_sheetmetal::edit`) and `rebuild/kernel_ops/sheetmetal/tools.rs`
  refolds it (validate, flatten, fold, parts matched by their walls so ids and names stay).
  Picks are matched to the definition by position (the refold renames faces). New walls and
  joints get ids hashed from the feature id, so later features can rely on them.
- **Bend**: the bend region (as wide as the allowance at the bend's own radius and K) is taken
  out of the flat wall where the alignment puts it, so the flat keeps its size; the moving side
  is the smaller one; the bend turns towards the picked face (the face under a sketch line on the
  plate's plane is its bottom, so it bends down; the opposite angle arrow flips it). Folded
  alignments solve for the band position that puts the chosen face of the bent wall on the line.
- **Jog**: Up to entity measures from the picked face to the entity along the jog direction,
  then applies the anchor; a jog whose middle wall would vanish fails ("too small").
- **Boolean exactness**: polygon booleans round to 1 nm; their results are snapped back to the
  inputs' vertices and crossings, else a wall and its bend could be fused as two solids.
- **Gaps**: Extrude Add with an automatic merge scope isn't refused; ordinary edits of a sheet metal part
  are lost at the next refold; cuts don't cut bend regions; Part pattern instances aren't sheet
  metal; Tangent chamfer measures like Offset; corner breaks in the flat view wait for P3I.3/6;
  bridging tabs need the walls coplanar with the same material side.


### Decisions (integration: one sheet metal editing model)

- **One definition** per model: `cadrs_sheetmetal::definition::Definition`, kept in the
  context (`SheetMetalContext::def`). Its **base** is P3I.4's `SharpDef` (the walls at their
  virtual sharps, their joints and hems), or a fixed model for a Sheet metal Loft made on its
  own. Its **steps** are P3I.5's model edits as data (`StepEdit`: Bend, Jog, Tab, Cut, corner
  breaks, face copies, Corner and Bend relief overrides) plus a loft's added walls, in feature
  order, each with its feature's name for errors. `build()` builds the base, replays the steps
  and applies the table order (Move up / Move down, `table_order`).
- **Who edits what**: Flange, Hem and Make joint add walls and joints to the base; Modify joint
  and the table change a joint of the base in place (`edit_joint`, P3I.3's `joint_edit::apply`),
  or patch the radius / K factor of the Bend or Jog step that made the joint (a rip of such a
  bend is refused); every other feature appends a step. A base edit made after steps (a Flange
  after a Bend) goes under them and the steps replay on the changed walls: the result is the same
  in either feature order (tested). Base edits pick their edges on the built model and
  `Definition::pull_back` carries them back to the base wall (undoing the rigid moves of steps);
  walls a step made (a Bend's moving side, a Jog, copies, loft walls) can't take a Flange, Hem or
  Make joint (an error says so).
- **One pipeline** (`rebuild/kernel_ops/sheetmetal/refold.rs`): `Rebuilder::edit_sheet_metal`
  (the feature changes the context) then `Rebuilder::refold`: build and validate, flatten (with
  the forms' outlines), fold (walls extruded, loft walls as mitred slabs, faces named by the
  feature that made each wall or bend: `owners`), the walls-intersect check, the parts a loft
  joined united (`merges`), the **forms applied again** (`SheetMetalContext::forms`: each copy
  placed relative to its wall, so it moves with it), then the parts back under their ids (a
  folded part takes the id of the old part it shares walls with). The Sheet metal model, a
  Loft (New) and every later feature go through it. A flat collision or walls running into
  each other fail the feature but keep the context, so the flat view shows the problem.
- **Modules renamed**: P3I.3's `cadrs_sheetmetal::edit` is `joint_edit` (its `Recipe` is gone:
  the definition replaces it), P3I.5's is `model_edit`.
- **Modify joint placement**: after the last feature that changed the model (the context's
  `editors`), so a Flange's bend is modified after the Flange; ordinary features after that
  still come after it.
- **Gaps left**: ordinary (non-sheet-metal-aware) features after the model act on the folded
  part and are lost at the next refold; a loft's joints can't be modified; a hem's radius is set
  in its Hem feature (not the table).

### Decisions (P3I.6)

- **Flat pattern planes**: each flat-pattern part registers a Plane-feature frame (id: the
  model's own for its first part, derived for the others) on its anchor wall (the wall its flat is
  laid out from), so a sketch on it is in the flat's coordinates and follows the flat. Until the
  flat view (P3I.3) the sketch shows in 3D over that wall.
- **Flat extrude** is its own feature kind (`FlatExtrude`, listed as "Extrude", applied-dialog
  `flat-extrude`): Remove keeps the regions in the model as flat cuts in the anchor wall's
  coordinates, applied by the flat solver like relief cuts on every piece (flat outline, bend lines
  and each piece's removed material, `(s, u)` on bend regions); Add grows the wall whose edge the
  new material runs along. The parts are refolded in place (same ids and names). A cut on a bend
  region that isn't a rectangle in `(s, u)` is taken out as 24 slices (exact for rectangles, like
  the lesson's slot). A new one starts on Remove when every region lies on material.
- **SM14.3**: an ordinary Extrude of a flat-pattern sketch fails with Onshape's message.
- **Flat DXF**: our DXF writer with seven new layers; round holes (polygon circles) are written as
  CIRCLE; bend up/down by layer and colour (integration: the CENTER linetype for both, as P3I.7's flat pattern views; one layer set in `cadrs_drawing::export::Layer` for both).
- **Insert DXF or DWG**: the dialog lists the DXF/DWG files of the import folder (cadrs has no file
  tabs) with dark thumbnails, Units (the document's unit to start with) and Use file origin
  position; one undo step "Insert DXF/DWG". The reader keeps the file's numbers; Units scales them.
- **E1 fixture**: our own flat export of `cadrs_sheetmetal::samples::e1_tray` (7 walls, 6 bends, 9
  cut-outs): imported, its bend lines split the sheet into the exercise's 7 regions.
- **Gaps left**: the flat view's menu (P3I.3 calls `flat_ui::begin_flat_sketch` and
  `flat_export_dialog::open`); features after the model that changed the folded part are rebuilt
  away by a later flat extrude (a warning says so); rolled walls ignore flat cuts in the folded
  solid; counterbore/countersink and form options add nothing yet (forms are P3I.9).

### Decisions (P3I.8)

- **Legacy import (SM17)**: Thicken's Tangent propagation adds a neighbour across an edge when
  the two faces meet smoothly there: two planes in one plane facing the same way, or any faces
  whose mesh normals at the edge's middle agree (a bend cylinder and its flats); a bend's end
  face (normal along the axis) doesn't carry on. The stand-in is our own C-channel's folded solid
  exported as STEP, so the test can demand the original's volume, flat and bends back.
- **Top-down (SM18)**: deriving the master works as it did; Derived now also brings the sheet
  metal contexts of the parts it derives (named after the source model): unchanged definition at
  the source's own place (active), a moved model as finished sheet metal elsewhere. Contexts carry
  their name; the table's dropdown lists every context (models, lofts, derived ones).
- **Exercise stand-ins (X7)**: built by `samples::sheetmetal_exercises` through the same commands
  as clicks, with fixed ids; fixtures live in `fixtures/sheetmetal/` (thin sheet doesn't mesh
  within the FEA course-parts check's 1 %, so they stay out of `fixtures/`). Each exercise's measured
  value is frozen in `tests/sheetmetal_exercises.rs` and the scenarios reproduce it in the UI.
  E1's Bends go in order: the short end flanges' lines first (a long line's band would cross their
  joints), then outermost first (a lip's line lies on its wall only while that wall is flat).
- **Flat view menu**: New sketch, Export DXF/DWG of flat pattern, Create drawing of flat pattern,
  then Zoom to fit; the part is the one with the hovered bend, else the model's first.
- **Gaps left**: E2's arc stand-in; the flat view's part pick is by hovered bend, not by the
  clicked piece; a shown flat sketch's region fill draws a shading artefact over the folded part
  (`sm_e1` 04+); flat sketches of loft or derived contexts aren't drawn (only Sheet metal model
  flats have planes); in-context sheet metal (SM18.4) isn't sheet-metal-specific.
