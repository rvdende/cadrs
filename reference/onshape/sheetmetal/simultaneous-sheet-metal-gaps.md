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
| SM1.1–SM1.6 | Simultaneous model: multi-part, three synced views, panel, cross-highlight, collision check, active-model behaviour | ❌ | Core of P3I.1–P3I.3. |
| SM2.1–SM2.8 | Sheet metal model (Convert / Extrude / Thicken; General / Material / Relief) | ❌ | P3I.2. Thicken/Extrude/booleans exist as building blocks. |
| SM3.1–SM3.8 | Flange (alignment, end types, angle control, miter, model radius, partial flange) | ❌ | P3I.4. Move face (SM3.8) doesn't exist in cadrs (direct edit); noted, not required by the exercises. |
| SM4.1–SM4.5 | Hem (Straight / Rolled / Tear drop, alignment, corner type) | ❌ | P3I.4. |
| SM5.1–SM5.4 | Tab (profiles, flanges to merge, subtraction scope/offset) | ❌ | P3I.5. |
| SM6.1–SM6.4 | Make joint, Modify joint | ❌ | P3I.4 (Make joint), P3I.3 (Modify joint from table edits). |
| SM7.1–SM7.3 | Corner | ❌ | P3I.5. |
| SM8.1–SM8.3 | Bend relief | ❌ | P3I.5. |
| SM9.1–SM9.7 | Bend | ❌ | P3I.5. |
| SM10.1–SM10.2 | Finish sheet metal model | ❌ | P3I.5. |
| SM11.1, SM11.3, SM11.4 | Corner break (fillet radius/width, chamfers, table lock) | ❌ | P3I.5. Fillet and chamfer kernel ops exist. |
| SM11.2 (Conic, Curvature) | Corner break conic/curvature fillets | **out of scope** | niche; out of scope by user decision 2026-09-29. Radius/Width with Distance control and Asymmetric stay in scope. |
| SM12.1–SM12.3 | Other features on sheet metal (perpendicular cuts, fillets, part/face patterns and mirrors) | ❌ | P3I.5. Patterns exist; making them sheet-metal-aware is new. |
| SM13.1–SM13.5 | Bend and joint table | ❌ | P3I.3. Table widget exists. |
| SM14.1–SM14.4 | Modeling in the flat view | ❌ | P3I.6. |
| SM15.1–SM15.3 | Flat DXF/DWG export | 🟡 | DXF/DWG writing exists for sketches and faces (P3F.2); the flat dialog, scopes and bend layers are P3I.6. Email and "store as tab" follow the existing export dialog's options. |
| SM16.1–SM16.6 | Drawings of flat patterns | ❌ | P3I.7. Drawings, Insert view and view menus exist. |
| SM17.1–SM17.3 | Legacy import via Thicken + tangent propagation + bend cylinders | ❌ | P3I.8. |
| SM18.1–SM18.4 | Top-down design (Derived master model, contexts named after features) | 🟡 | Derived and in-context studios exist; sheet metal through Derived is P3I.8. |
| SM19.1 | Jog | ❌ | P3I.5. |
| SM19.2 | Sheet metal Loft | ❌ | P3I.9 (later). |
| SM20.1–SM20.3 | Form, Tag (Form), forms library | ❌ | P3I.9 (later); cadrs ships its own small forms library (louver, lance, dimple, emboss), never Onshape's. |
| E1–E4 | Exercises | ❌ | Stand-ins and scenarios in P3I.8 (E2 earlier, as each phase's acceptance). |
| X1–X7 | Toolbar group, feature list, parts list, undo, units, errors, stand-ins | ❌ | Spread over the phases; X4 (undo) is mandatory in each. |
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
