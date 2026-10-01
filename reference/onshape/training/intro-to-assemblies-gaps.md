# Introduction to Onshape Assemblies: gap analysis (2026-09-27)

This maps [intro-to-assemblies.md](intro-to-assemblies.md) against the cadrs code on branch
`phase3` (`ff7616b`). ✅ done · 🟡 partial · ❌ missing · **out of scope** (ORCHESTRATOR.md phase 3
rule: only cloud and multi-user collaboration, sharing and permissions, release management and paid
tiers, and Onshape account and learning-site features). File references are to the code at the
time of writing. Related: [intro-to-part-studios-gaps.md](intro-to-part-studios-gaps.md)
(milestones **P3.1–P3.9**, in progress; referenced as dependencies, not re-planned), and
[crates/cadrs_kernel/README.md](../../../crates/cadrs_kernel/README.md).

**Summary:** the Assembly tab is a **shell**. What exists:
- `ElementKind::Assembly` is a unit variant with no data (`cadrs_core/src/document.rs:128`). A new
  document has "Part Studio 1" and "Assembly 1". "+" → Create Assembly, tab rename, duplicate and
  delete work (`cadrs_app/src/document.rs:795-857`, `:1848-1930`; scenario `tabs_create_assembly`).
- The assembly toolbar is drawn (`assembly_toolbar`, `document.rs:1271-1336`). **Insert** shows a
  toast ("Inserting parts arrives in a later version"). About 26 other buttons (the 9 mates, Group,
  Mate connector, Replicate, the relations, Explode, BOM, …) are drawn **disabled**.
- The left panel (`instance_list`, `document.rs:1634-1725`) shows "Instances (0)", the root row,
  Origin, "Items (0)", "Mate features (0)" and a hint. The viewport draws only an origin triad.
- The assembly shortcuts (I, M, J, K, H, Ctrl+M, Shift+N, Shift+S) are listed as **off** in
  `shortcuts.rs:58-65`. Ctrl+C/Ctrl+V are off. No copy/paste.
- Nothing else: no instances, transforms, mate connectors, triad manipulator, solver, BOM, part
  properties (beyond names), materials or mass properties. `cadrs_kernel` has `transform` and
  `mass_properties` (volume, area, CoM; **no inertia**), but it isn't wired into `cadrs_core` yet
  (P3.1 does that).

So the whole course is new work on top of the Part Studios plan. It needs from P3.x: the kernel
rebuild (P3.1), persistent naming and 3D edge/face/vertex picking (P3.2), the parts list, part
rename and the mass-properties panel (P3.3), materials, mass units and inertia (P3.5), and mate
connectors in Part Studios (P3.8).

**Cross-stage dependencies.** A few rows can only close with milestones from later stages:
Measure/section/render modes (P3E.3), versions (P3D.3, landed), cross-document insert (P3G.1, was P3F.1), export
(P3F.2), variables panel (P3F.4), simulation (P3F.5) and Create Drawing (P3C.1). Stage 3B is
**passed** when P3B.1–P3B.9 are done **and** those rows are ticked; the orchestrator may pull the
named milestones forward.

**Counts (181 A IDs + 16 X IDs + 2 quiz/survey rows = 199):** ✅ 186 · 🟡 10 · ❌ 0 · out of
scope 3 (A: 173 / 7 / 0 / 1; X: 13 / 3 / 0 / 0; after P3B.9; the quizzes and survey are 2 of the
out of scope). **After P3F.5 and stage 3G (P3G.1–P3G.2):** ✅ 193 · 🟡 3 · ❌ 0 · out of scope 3 (A: 177 / 3 / 0 / 1; X: 16 / 0 / 0 / 0): A1.7, X8 (Loads) and
A1.8, A6.3, X16 (Simulation) are done (see "P3F.5 status" below); P3G.1 moved A2.3 (Other
documents) and P3G.2 X15 (Change to version) to ✅. Every 🟡 row left waits only for another
stage's milestone, named in the row: A1.9 (Measure, Analysis) on P3E.3; A3.5, A4.3 (drawings) on
P3C.1/P3C.2. **Final part 2:** A3.5 and A4.3 checked and ticked: ✅ 195 · 🟡 1 (A1.9, → 3E) · ❌ 0 ·
out of scope 3 (A: 179 / 1 / 0 / 1; X: 16 / 0 / 0 / 0). **After stage 3E (2026-10-02):** A1.9
checked against P3E.3 (Measure and Analysis on placed instances, in assembly coordinates) and
ticked; A3.3 and X15's Section view item is enabled: ✅ 196 · 🟡 0 · ❌ 0 · out of scope 3 (A: 180 /
0 / 0 / 1; X: 16 / 0 / 0 / 0).

## 1. Creating an assembly

### A1 Assembly interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A1.1 | Insert button (parts, assemblies, sketches, surfaces) | ✅ | Insert opens the Insert dialog (`cadrs_app/src/assembly/insert.rs`); I in assemblies. P3B.1. |
| A1.2 | Mate connector tool in the toolbar | ✅ | P3B.7: the toolbar's Mate connector button and Ctrl+M open the assembly's Mate connector dialog (`assembly::connector_tool`, the Part Studio's rows); its connectors stay in the assembly (`course_asm_connector_tool_origin` 02–03). |
| A1.3 | Mate toolbar: 8 standard mates + Tangent, Width, Group, relations | ✅ | All ten mates (M for Fastened) and Group (P3B.2, P3B.3); P3B.9: the relation tools (Gear, Rack and pinion, Screw, Linear, and the Relations button's menu of them) open the relation dialog (A1.4; `course_asm_relations` 02–03). |
| A1.4 | Relations tools (gear, rack and pinion, screw, linear) | ✅ | P3B.9: relations between mates ([`assembly::relation`](../../../crates/cadrs_core/src/assembly/relation.rs), `MateKind::Relation`): **Gear** (ratio a : b, Reverse), **Rack and pinion** (distance per revolution), **Screw** (pitch, one Cylindrical mate), **Linear** (ratio); a dialog (type, the Mates field filled by clicking mate rows, the values, Reverse direction), a row of the Mate Features list whose ▸ lists its mates, undo; the solver holds `k₁v₁ + k₂v₂ ≡ 0` modulo whole turns, so drag, Animate, Reset and Apply limit move the related mates together (`course_asm_relations` 03–14; `assembly_p3b9.rs`: a 2 : 1 gear turns the driven revolute −45° for 90°, a rack moves 2πr per turn, a screw one pitch per turn, a linear relation scales the travel, a drag turns both gears). |
| A1.5 | Filter & search field over the assembly lists | ✅ | P3B.4: the Instances field filters both lists with P3.9's filter language (`feature_list::Filter`: text, "exact", `:type`, `:folder`, …); matching rows keep their folders, a folder whose name matches shows all it holds (`course_asm_folders` 16–17). |
| A1.6 | Instances list: root, Origin, instances `Name <n>` | ✅ | Root row (selects the whole assembly), Origin, one row per instance `<part> <n>` with fixed/DOF icon and hover eye (`assembly/list.rs`). The filter field is P3B.4. |
| A1.7 | Mate Features list; Items and Loads groups | ✅ | Mate Features list (`assembly/mates_list.rs`; P3B.9: relations are rows too) and the **Items (n)** group (P3B.8, `course_asm_items`). P3F.5: **Loads (n)** lists the simulation's loads (`load-row-<k>`: icon, name, value; double-click edits it in the Simulation panel; `simulation_ui.rs`, `course_asm_simulation` 05). |
| A1.8 | Right panels: BOM, Configurations, Exploded views, Named positions, Simulation, Variable table | ✅ | BOM (P3B.6), Exploded views and Named positions (P3B.8). Configurations out of scope (niche; out of scope by user decision 2026-09-29). **Variable table ✅ (P3F.4)**: the assembly's Variables (the toolbar's Variable, rows in the Mate Features) listed with their values, editable, and mate offsets typed as `#name` follow them (`assembly::vars`, `course_asm_variables`). **Simulation ✅ (P3F.5)**: the right strip's Simulation panel (materials, Loads, Mesh, Solve with progress, results with legend and probe; `course_asm_simulation`). |
| A1.9 | Bottom-right Measure, Analysis, Mass properties | ✅ | Mass properties measure instances and the root in assembly coordinates (P3B.1, `course_asm_ex1_start` 13); P3B.9: its own mass unit (the course's step stool reads grams with the pound setting, A24.12). **P3E.3** (stage 3E): Measure and Analysis work on instances as placed, in assembly coordinates: Measure between two Block instances' tops reads 5 mm, and a vertex reads X 90, Y 40, Z 25 in the assembly's frame (`course_td_measure` 08–09); the draft analysis takes its pull direction from a face of Block <2>, flips it, and zebra stripes run across the instances (`course_td_analysis` 12–14). Tests: `placed_instances_measure_in_assembly_coordinates` (two instances of the 100 × 60 × 25 box, one turned 90° about Z and moved: 40 mm apart, the turned edge 100 mm at (200, 0, 0)–(200, 100, 0), the top still 6000 mm²) and `a_rotated_instance_carries_its_pull_direction_and_normals` (`measure_analysis.rs`). (Before: 🟡, → 3E.) |

### A2 Starting an assembly
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A2.1 | Assembly = structure of instances + DOF and relationships | ✅ | `cadrs_core::assembly`: instances (source (studio, part), pose, `<n>`, hidden, fixed) on the Assembly element, all through commands with undo. P3B.1. |
| A2.2 | New doc has Part Studio + Assembly; "+" → Create Assembly; several assembly tabs | ✅ | `Document::new`, `create-assembly`; scenario `tabs_create_assembly`. |
| A2.3 | Insert dialog: Current/Other documents/Standard content, Part Studios/Assemblies tabs, search, type filters, thumbnails tree, "Inserted: n", Undo to remove | ✅ | Current document → Part Studios (P3B.1), Assemblies (P3B.4), Standard content (P3B.5). **Other documents: P3G.1** (2026-09-29): locations, search or pasted id, a document at its newest version or one picked in the version graph, the same Part Studios / Assemblies lists, inserted as version links (`course_er_insert_linked` 02–14; `course_er_versions_in_document` 05–08 for this document at a version). |
| A2.4 | Insert a Part Studio as rigid; Edit its contents | ✅ | P3B.8: Insert's "Insert the Part Studio as rigid" toggle makes a studio's row one rigid instance (`InstanceSource::Studio`, its parts on the instance); it moves and mates as one, lists its parts under its ▸, stays live, and its Edit… ticks parts in or out, unticked ones hidden while the dialog is open (`SetStudioParts`; `course_asm_rigid_insert` 01–08; `a_rigid_part_studio_instance_moves_as_one`, `…_is_edited_and_stays_live`). |
| A2.5 | Pick configuration options in the dialog | out of scope | Configurations: niche; out of scope by user decision 2026-09-29 (ORCHESTRATOR.md speed-up rule 4). |
| A2.6 | Placement: cursor inside dialog = studio position; outside = follows cursor; multi-click inserts several | ✅ | Pointer in the dialog: studio position; in the view: follows the pointer, each click drops one; picking another row or ✓ inserts a waiting item at its studio position (`course_asm_insert_placement` 01–03). |

### A3 Positioning assembly instances
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A3.1 | Click an instance → triad at the clicked spot, aligned to the entity | ✅ | Click a face/edge/vertex of an instance: triad at the clicked spot, Z along the face normal / circle axis (into the part) / edge (`assembly/triad.rs`; `course_asm_ex1_start` 05, `course_asm_triad` 01). |
| A3.2 | Drag the triad origin to relocate; snaps to connector points; Shift locks | ✅ | Snaps to the implicit mate connector points (centroids, circle/hole centres, hole axis middles, midpoints, vertices, virtual sharps) with dots on the entity under the pointer; Shift locks it (`course_asm_triad` 02, `course_asm_mate_connectors` 05). |
| A3.3 | Triad origin menu → Move to origin (+ instance menu items) | ✅ | Triad origin menu: Move to origin + the instance menu (`course_asm_ex1_start` 07–08). **P3E.3**: the triad menu's Section view… is enabled and sections the assembly through the instance (`course_td_section` 13–14). |
| A3.4 | Plane/arrow/ring drags with value fields; Align / Anti-align with Z; rotate 90°/180° | ✅ | Arrow, plane-square and ring drags with a live value box; after the drag the box takes a typed value ("1 in", "90 deg"; Enter applies it in place of the drag, one undo step; Esc closes) (`course_asm_triad` 04–05b); Align / Anti-align with Z; Rotate 90°/180°. |
| A3.5 | Assembly view cube defines drawing orientation | ✅ | The view cube is in every tab (`view_cube.rs`), and its named views are the drawings' (`cadrs_drawing::NamedView`): test `camera::tests::view_cube_views_are_the_drawing_views`. An assembly's drawing views use its own axes (`course_drw_ex2_assembly` 06–07, `course_drw_ex3_update` 29). Final part 2. |
| A3.6 | Accept inside the dialog to keep studio placement | ✅ | ✓ with a waiting item keeps its studio placement. |
| A3.7 | Fix / Unfix; fixed icon vs DOF icon; Fix not carried to a parent | ✅ | Fix / Unfix, fixed vs DOF icon (P3B.1). P3B.4: a Fix inside a subassembly is not carried into the parent (`structure::solver_model`; `assembly_structure.rs` `a_fix_in_the_child_is_ignored_in_the_parent`); Dissolve gives the instances the subassembly's own Fix. |

### A4 Hide & show instances, Switch to
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A4.1 | Eye icon per instance row | ✅ | Eye on hover, crossed eye on hidden rows (`course_asm_hide_show` 01–02). |
| A4.2 | Hide, Hide other, Hide all, Isolate, Make transparent | ✅ | Hide, Hide other instances, Hide all instances, Isolate… (Exit isolate), Make transparent… on the selection (`course_asm_hide_show` 03–08). |
| A4.3 | Empty-space menu: Show all, Show all instances, Paste, Create Drawing, Zoom to fit, Isometric | ✅ | Show all, Show all instances, Paste <instance> (P3B.2), Zoom to fit, Isometric work; P3B.9: Check interference of every pair. Create Drawing of Assembly 1… is enabled since P3C.5 (`course_asm_hide_show` 05) and opens the Create drawing dialog (the same path as the instance's, `course_asm_where_used` 08, `course_drw_ex2_assembly` 03c–03d). Final part 2. |
| A4.4 | Select hidden rows → Show | ✅ | Hidden rows are selectable; their menu has Show (`course_asm_hide_show` 12–13). |
| A4.5 | Y hides the hovered instance, Shift+Y shows | ✅ | Y hides the instance under the pointer, Shift+Y shows all (`course_asm_hide_show` 09–11). |
| A4.6 | Switch to the source Part Studio (part highlighted) or subassembly tab | ✅ | Part Studio (P3B.1, `course_asm_hide_show` 14–15); P3B.4: "Switch to <subassembly>" (assembly icon) opens its tab (`course_asm_subassemblies` 03–04). |

### A5 Exercise: Start an Assembly (stand-in: `fixtures/motor_mount_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A5.1 | Make a copy of the starting document | ✅ | Fixture `fixtures/motor_mount_standin.cadrs` (built from features by `cadrs_core::samples::motor_mount`, checked by `the_fixture_is_current`); scenario command `fixture <name>` opens it as a stored copy. |
| A5.2 | "+" → Create Assembly, rename "Mount Assembly" | ✅ | Works today (`tabs_create_assembly`). |
| A5.3 | Insert Motor Mount, click to place off the origin | ✅ | `course_asm_ex1_start` 03–04. |
| A5.4 | Click the back hole's edge; triad appears there | ✅ | `course_asm_ex1_start` 05 (view from below, hole edge clicked). |
| A5.5 | Drag triad origin to the hole centre ("Diameter: 0.563 in" readout) | ✅ | `course_asm_ex1_start` 06: snapped to the hole centre, "Diameter: 0.563 in" readout. |
| A5.6 | Move to origin | ✅ | `course_asm_ex1_start` 07–08. |
| A5.7 | Anti-align with Z | ✅ | `course_asm_ex1_start` 09 (half turn about the triad X = model X). |
| A5.8 | Check orientation; use the other option if wrong | ✅ | `course_asm_ex1_start` 10: base on top, upright below. |
| A5.9 | Fix the instance | ✅ | `course_asm_ex1_start` 11–12. |
| A5.10 | Mass properties of the top-level assembly (CoM in assembly coordinates) | ✅ | `course_asm_ex1_start` 13: 0.866 lb, 8.876 in³, CoM (0.000, −0.380, −1.137) in; unit test `ex1_start_an_assembly_mass_properties` (closed form incl. inertia, 1e−4). |

## 2. Mating assembly instances

### A6 Mating in Onshape
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A6.1 | Mates join two mate connectors (explicit or implicit) | ✅ | A mate joins two `MateConnector` frames (origin, Z, X) on instances: implicit (P3B.2), a Part Studio's explicit connector carried by the part (`ConnectorAnchor::Explicit`), the assembly's own (`Local`) or the Origin (P3B.7). |
| A6.2 | One mate defines all DOF between two instances; Fastened removes 6 | ✅ | Each type constrains exactly 6 − DOF (`assembly_solver.rs` `each_type_leaves_its_dof`). |
| A6.3 | Mate dialog: type dropdown, connectors field + Reorder, Offset/Limits/Simulation connection, Flip, Reorient, Animate, Solve, ? | ✅ | Title, ✓/✕, type dropdown with all ten types, Mate connectors field with ⇅ Reorder, Offset, Limits, Flip, Reorient, Animate ▶, Solve, ? (P3B.2, P3B.3). P3F.5: **Simulation connection** is a real option (`Mate::simulation`, undoable with the mate): checked, the mate bonds its two parts in a simulation where they touch (Fastened is bonded; every connected mate is treated as bonded); unchecked, a part held only by it is "free to move" (`course_asm_simulation` 02, 08; `cadrs_core/tests/simulation.rs`). |
| A6.4 | Implicit points: centroid, midpoints, vertices, circle/hole centres, negative space, virtual sharp | ✅ | `connector::implicit_points` on kernel faces and edges (plus sphere centres, P3B.3); dots on the hovered entity, the nearest as a connector glyph; the points of the entity just left stay pickable while the pointer is near them, so a bore's axis middle can be reached (`course_asm_mate_connectors` 01, 04). Planar-face Z is the outward normal (checked against the mesh). |
| A6.5 | Shift locks the hovered face/edge | ✅ | The hover highlight stays on the locked entity (`course_asm_mate_connectors` 03). |
| A6.6 | Circular edges give one centroid point | ✅ | `course_asm_mate_connectors` 02; `course_pneumatic.rs` `implicit_points_on_the_parts`. |
| A6.7 | Solve on pick: connectors coincide, Z axes aligned | ✅ | The second pick places the first movable instance so the frames coincide (`course_asm_ex2_pneumatic` 10, 12). |
| A6.8 | Flip primary, Reorient secondary (90°), act on first movable; Reorder items | ✅ | Flip / Reorient act on the first connector, or the second when the first can't move; ⇅ Reorder with drag handles and Done (`course_asm_ex2_pneumatic` 12). |
| A6.9 | Animate ▶ previews the DOF | ✅ | Plays the mate's first DOF 0 → one end → the other → 0 on the dialog's placements (`assembly/animate.rs`; `course_asm_mates_pinslot` 08). |
| A6.10 | Offset (XYZ + rotate about + angle); Limits per DOF | ✅ | Offset X/Y/Z, Rotate about, angle for every connector mate; Limits per DOF: Z (Slider, Cylindrical), X (Pin slot, Planar), Y (Planar), Z angle (Revolute, Cylindrical, Pin slot, Planar) (`course_asm_mates_pinslot` 07, `course_asm_mate_dialog_options` 03, 07: Offset Z −15 mm on a Revolute). |
| A6.11 | No continuous solve while editing; Solve button; accept solves | ✅ | Picks and dialog edits place only this mate's instance; Solve and ✓ solve all mates (`cadrs_core::assembly::solver`; `course_asm_mate_dialog_options` 07–09: an offset on a Revolute (Z held) drops the pin and leaves the magnet fastened to it floating; Solve brings the magnet down; the offset holds after ✓). Final part 3: `course_asm_mate_dialog_options` 07 (the pin at the offset, the magnet left floating) and 08 (Solve re-seats it) now differ, asserted in `golden_course_asm_mate_dialog_options`; the magnet had been fastened to the socket by a mis-pick. Test `revolute_offset_holds_and_solve_reseats_a_fastened_part`. |
| A6.12 | Mate list in creation order, limit icon, hover tooltip, double-click edits | ✅ | Creation order, type icons, limit icon, double-click edits; the hover tooltip gives the type, offset and limits (`course_asm_mates_pinslot` 12). |
| A6.13 | Mate context menu: Rename, Edit, Apply limit position, Reset, Show, Isolate, Suppress, Animate, folders, Expand, Delete, … | ✅ | Rename, Edit…, Apply limit position ▸, Reset, Show/Hide, Show all mates, Isolate…, Suppress/Unsuppress, Animate…, Expand/Collapse (a Replicate's and a relation's too), Add selection to folder…, Delete (P3B.3, P3B.4); P3B.9: **Make transparent…** (the mate's instances, Make opaque again) and **Edit named position driver mate…** (the Named positions panel with that mate's column marked). "Add comment" is collaboration: out of scope. |

### A7 Mate reference sheet
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A7.1 | Fastened, 0 DOF | ✅ | Rank 6 (`each_type_leaves_its_dof`). |
| A7.2 | Revolute, rotate Z | ✅ | Rank 5; turning about Z stays, the rest is undone (`each_type_allows_its_motion_and_no_other`). |
| A7.3 | Slider, translate Z | ✅ | Rank 5, as A7.2. |
| A7.4 | Cylindrical, translate + rotate Z | ✅ | Rank 4, as A7.2. |
| A7.5 | Pin slot, rotate Z + translate X | ✅ | Rank 4; X slide and Z turn stay, the rest is undone (`assembly_solver.rs` `each_type_leaves_its_dof`, `each_type_allows_its_motion_and_no_other`). |
| A7.6 | Planar, translate X, Y + rotate Z | ✅ | Rank 3, as A7.5. |
| A7.7 | Ball, rotate X, Y, Z | ✅ | Rank 3; turns about X, Y and Z stay (as A7.5). |
| A7.8 | Parallel, translate X, Y, Z + rotate Z | ✅ | Rank 2, as A7.5. |
| A7.9 | Tangent (entity-based) | ✅ | Plane, cylinder, sphere, straight edge and vertex pairs (`assembly_mates.rs` `tangent_keeps_a_cylinder_on_a_plane`: Ø20 stays at 10, 4 DOF; `tangent_pairs_hold`). |
| A7.10 | Width (tabs centred between width connectors) | ✅ | Tabs' middle on the centre plane, tab Z parallel to its normal: one instance keeps 3 DOF (slide in and turn about the plane, as A12.3 describes; the course's table says 2) (`width_centres_one_instances_tabs`). |

### A8–A14 Mate types, grouping, showing mates
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A8.1 | Fastened with Z offset (negative flips) | ✅ | `offset_z_moves_the_rod_by_the_offset`; `course_asm_ex2_pneumatic` 15 (0.5 in), 13 (−0.25 in). |
| A8.2 | Hover shows the offset | ✅ | The row tooltip (a card beside the row) shows the offset and the limits (`course_asm_mate_dialog_options` 08, `course_asm_mates_pinslot` 12). |
| A8.3 | Revolute on face centroids; Animate | ✅ | Revolute works (`course_asm_ex2_pneumatic` 10); ▶ and the Animate dialog (A6.9, A16.5). |
| A8.4 | Slider with Limits; negate if reversed | ✅ | `a_limit_clamps_a_drag`; `course_asm_ex2_pneumatic` 20. |
| A9.1 | Cylindrical with a limit | ✅ | `cylindrical_z_and_angle_limits_clamp`; `course_asm_mate_dialog_options` 03, 06 (the drag stops at Z max). |
| A9.2 | Pin slot: slot first, pin second; Reorder | ✅ | The slot's connector first (X along the slot); picked the wrong way round, Reorder → drag → Done (`course_asm_mates_pinslot` 02–05). |
| A9.3 | Pin slot limits ±½ slot length, Z angle 0/0 | ✅ | Limits X ±20 mm (the 40 mm slot) and Z angle 0/0: a drag stops at the end, a ring drag can't turn it (`pin_slot_limits_half_the_slot_and_lock_rotation`; `course_asm_mates_pinslot` 07, 10, 11). |
| A10.1 | Planar (+ Tangent combination) | ✅ | Planar: face to face, free in the plane (`course_asm_mates_planar_ball_parallel` 02–05). Planar + Tangent keep the Cam Pin in the Cam Plate's curved slot: it follows the arc and stops at the slot's round end (`course_asm_mates_tangent_width` 05–09; `planar_and_tangent_keep_a_pin_in_a_curved_slot`). |
| A10.2 | Ball | ✅ | The socket's rim centre and the ball's centre (sphere centre point) (`course_asm_mates_planar_ball_parallel` 06–07). |
| A10.3 | Parallel (placed touching, not held) | ✅ | Placed touching by the pick's snap, then free (4 DOF): a drag lifts it (`course_asm_mates_planar_ball_parallel` 08–10). |
| A11.1 | Tangent takes two entity picks, no connectors | ✅ | An "Entities" field takes two face/edge/vertex picks ("Face of Roller") (`course_asm_mates_tangent_width` 02). |
| A11.2 | Tangent propagation checkbox, on by default | ✅ | On by default. With it on, the solve uses, of the faces tangent-continuous with each pick (`connector::tangent_faces`), the one the contact lies on at the current placements (`assembly::propagate_tangents`), so the contact follows the chain: the roller over the ramp's fillet and up its slope, the pin round the slot's end (`course_asm_mates_tangent_width` 02–04, 08–09; `tangent_propagation_rolls_the_roller_over_the_ramp`). |
| A11.3 | Tangent flip | ✅ | The first solve takes the side the entities are on; Flip puts the other side (`tangent_keeps_a_cylinder_on_a_plane`; `course_asm_mates_tangent_width` 09a–09b: the ball on, then under, the table). |
| A12.1 | Width: up to two Tab + exactly two Width connectors | ✅ | Tab mate connectors (up to two) and Width mate connectors (exactly two); the active field takes the picks, the tabs hand over at two (`course_asm_mates_tangent_width` 05, 07). |
| A12.2 | No instance in both fields | ✅ | A pick of an instance already in the other field is refused and the field turns red (`course_asm_mates_tangent_width` 06); the command checks it too. |
| A12.3 | One-instance tabs stay centred; two-instance tabs stay mirror-symmetric | ✅ | `width_centres_one_instances_tabs`, `width_keeps_two_instances_mirror_symmetric`; `course_asm_mates_tangent_width` 12 (one instance centred), 14–16 (two Jaws mirror-symmetric about the clevis's centre plane; a drag of one moves the other the opposite way). Final part 3: a width pair picked on two parallel edges at different points no longer tilts the centre plane (`width_on_edge_picks_centres_the_jaws_square`; `course_asm_mates_tangent_width` 14–16). |
| A13.1 | Group removes DOF between the selected instances | ✅ | A group is one rigid body in the solver (`a_group_keeps_relative_transforms`). |
| A13.2 | Group dialog (Instances list) → "Group 1" in the mate list | ✅ | `assembly/group_dialog.rs`; `course_asm_ex2_pneumatic` 07–08. |
| A13.3 | Grouped instances keep positions relative to their origins | ✅ | `a_group_keeps_relative_transforms`. |
| A13.4 | A group still needs Fix or a mate | ✅ | An unfixed, unmated group keeps 6 DOF (same test). |
| A14.1 | Mates hidden by default; J toggles all | ✅ | Hidden by default (grey rows); J shows all or hides all (`course_asm_show_mates` 01–03). |
| A14.2 | Hover a mate highlights its instances; eye per row | ✅ | Row hover highlights the mate's instances and shows its glyph; eye per row (`course_asm_show_mates` 04–05). |
| A14.3 | Show/Hide mates on mates or on an instance | ✅ | Mate menu Show/Hide, Show all mates; instance menu Show mates / Hide mates (`course_asm_show_mates` 06–07). |
| A14.4 | Show mates mode (H): hover shows, click pins, any level | ✅ | H toggles the mode (a chip says so): hovering an instance (view or list) shows its mates, a click pins them (`course_asm_show_mates` 08–09). Levels come with subassemblies (P3B.4). |
| A14.5 | Click a mate icon cross-highlights the row; its menu has Edit | ✅ | Each shown mate has a type badge (`mate-glyph-<n>`): click selects its row, right-click opens the mate menu with Edit… (`course_asm_show_mates` 10–11). |

### A15 Exercise: Pneumatic Cylinder (stand-in: `fixtures/pneumatic_cylinder_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A15.1 | Make a copy | ✅ | Fixture `fixtures/pneumatic_cylinder_standin.cadrs` (`samples::pneumatic`, checked by `course_pneumatic.rs` `the_fixture_is_current`); `course_asm_ex2_pneumatic` 01. |
| A15.2 | Create Assembly, rename "Cylinder assembly" | ✅ | Works today. |
| A15.3 | Insert 4 parts in one session, keeping studio positions | ✅ | `course_asm_ex2_pneumatic` 03–04. |
| A15.4 | Fix the Barrel | ✅ | `course_asm_ex2_pneumatic` 05–06. |
| A15.5 | Group the 4 → "Group 1" | ✅ | `course_asm_ex2_pneumatic` 07–08. |
| A15.6 | Insert Rear Cap mount by clicking | ✅ | `course_asm_ex2_pneumatic` 09. |
| A15.7 | Revolute: recess edge ↔ mount top edge | ✅ | `course_asm_ex2_pneumatic` 10 (the recess edge picked from below). |
| A15.8 | Insert 4× O-Ring 0.125 | ✅ | `course_asm_ex2_pneumatic` 11. |
| A15.9–A15.10 | Hide Barrel; Fastened 1–4 O-rings (flip if needed) | ✅ | `course_asm_ex2_pneumatic` 12–13. The stand-in has no grooves: the O-rings go on the spigots' edges (flipped, offsets 0 / −0.25 / 0 / 0.25 in). |
| A15.11 | Insert Structural Rod | ✅ | `course_asm_ex2_pneumatic` 14. |
| A15.12 | Fastened with Offset Z 0.5 in | ✅ | `course_asm_ex2_pneumatic` 15. |
| A15.13 | Copy/Paste the rod 3× | ✅ | Instance clipboard: Copy <instance> / Paste <instance> menu items, Ctrl+C / Ctrl+V (`course_asm_ex2_pneumatic` 16–17). |
| A15.14 | Fastened 5–8 | ✅ | `course_asm_ex2_pneumatic` 18. |
| A15.15 | Insert Piston & Rod | ✅ | `course_asm_ex2_pneumatic` 19. |
| A15.16 | Slider with limits Z −4.5…0 (stand-in: −3.25…0) | ✅ | `course_asm_ex2_pneumatic` 20. |
| A15.17 | Insert 3× O-Ring 0.185 | ✅ | `course_asm_ex2_pneumatic` 21. |
| A15.18 | Fastened 9–11 | ✅ | `course_asm_ex2_pneumatic` 22 (17 instances, 14 mates). |
| A15.19 | Slider 1 → Reset | ✅ | `course_asm_ex2_pneumatic` 25; Apply limit position 23–24 (CoM z 3.990 in). |
| A15.20 | Mass properties of the assembly | ✅ | `course_asm_ex2_pneumatic` 26: 2.853 lb, 20.624 in³, CoM (0.000, 0.000, 3.167) in; `course_pneumatic.rs` `ex2_pneumatic_cylinder_mass_properties` (1e−4, both slider positions). |

## 3. Working with an assembly

### A16 Assembly motion
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A16.1 | Triad (DOF) icon + tooltip on under-constrained instances | ✅ | DOF from the null space of the mate Jacobian (`solver::dof_counts`): a triad icon with "This instance has n degrees of freedom", the fixed icon, or none when held (`course_asm_ex2_pneumatic` 22). |
| A16.2 | Subassembly flexible/rigid (lock icon; Lock/follow Named position); Rigid icon | ✅ | P3B.4: rigid by default, the row's lock and Make flexible / Make rigid (`course_asm_subassemblies` 09, 12). P3B.8: the menu's **Lock / follow position to ▸** Current position or a Named position of its tab; a following row shows the named-position icon (`SetFollowPosition`; `course_asm_named_positions` 09–10; `a_subassembly_follows_a_named_position_of_its_tab`). |
| A16.3 | Root icon shows fixed vs fastened to origin; not carried to a parent | ✅ | P3B.4: the root row's icon when an instance is fixed (`course_asm_subassemblies` 02); P3B.7: also when an instance has a Fastened mate to the Origin (`InstanceId::ORIGIN`, tooltip "Fastened to the origin by a mate", `course_asm_connector_tool_origin` 01). Neither carries to a parent (origin mates are dropped from a flexible subassembly's solve). |
| A16.4 | Drag respects mates and limits; triad for exact drags | ✅ | Drag in the view and triad drags go through `solver::drag` (null-space step toward the pointer, then projected onto the mates and limits): a revolute only turns, a fully constrained instance stays, a slider stops at its limit (`assembly_mates.rs` drag tests; `course_asm_mates_pinslot` 10–11, `course_asm_mates_planar_ball_parallel` 05, 10). |
| A16.5 | Animate dialog: DOF pick, Start/End, Steps, Single/Reciprocate/Loop, reverse, current value, Play/Stop | ✅ | DOF pick, Start/End (the limits by default), Steps, Single/Reciprocate/Loop (Reciprocate holds a moment at each end), reverse, Current value (the mate's position until ▶) with the direction while playing, Play/Stop (a pause icon: icon-rs has no stop icon); closing puts the assembly back (`course_asm_animate` 02–10). |

### A17 Assembly structure
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A17.1 | Break designs into subassemblies | ✅ | P3B.4: subassembly instances ([`structure`](../../../crates/cadrs_core/src/assembly/structure.rs)): parts at any depth as occurrences; drawn, picked, measured and solved (`course_asm_ex3_structure`). |
| A17.2 | Insert → Assemblies tab; subassembly mates respected in the parent | ✅ | Insert's Assemblies tab (`course_asm_subassemblies` 16–17); cycles refused (`cycles_are_refused`); a flexible subassembly's mates act in the parent (A16.2). |
| A17.3 | Move to new subassembly (with mates); Create empty subassembly | ✅ | One undo step each: a new tab right of the parent's, every world placement kept, the mates between the moved instances go along, a group's moved members get a Fastened, mates to the rest reach into it (`move_to_new_subassembly_keeps_every_placement_and_takes_the_mates`; `course_asm_subassemblies` 01–06). |
| A17.4 | Drag instances in/out of a subassembly row ("n items" badge); mates move | ✅ | Drop rows onto a subassembly row (badge "4 items"), drag an opened subassembly's instance out; mates move and re-target (`ex3_structure_steps`; `course_asm_subassemblies` 07–11). |
| A17.5 | Dissolve subassembly; the empty tab stays | ✅ | `dissolve_restores_the_flat_structure`; `course_asm_subassemblies` 13–15. |

### A18 Assembly folders
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A18.1 | Folders in both lists | ✅ | P3B.4: the P3.9 folder model (`FeatureFolder`, runs kept contiguous) over instances and mates ([`folders`](../../../crates/cadrs_core/src/assembly/folders.rs)); `course_asm_folders`. |
| A18.2 | New folder icon; drag in; reorder the folder | ✅ | New folder → Folder name popup (empty folder last), drag rows onto a folder's row, drag the folder (`course_asm_folders` 01–07). |
| A18.3 | Add selection to folder… popup; placed at the first selected item | ✅ | The Folder name popup (✓ / ✗) beside the first selected row; the selection gathers at its first item (`folders_in_both_lists`; `course_asm_folders` 03–05). |
| A18.4 | Folder eye, Rename, Hide/Show, Suppress, Delete (with contents), Unpack | ✅ | Eye, menu (Rename, Hide/Show, Suppress/Unsuppress, Unpack folder, Delete with its instances and their mates), double-click renames; one undo step each (`course_asm_folders` 08–13). |
| A18.5 | Mate folders | ✅ | Mate menu → Add selection to folder…, drag mates in (`course_asm_folders` 14–15). |

### A19 Standard content
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A19.1 | Standard content tab: Standard / Category / Class / Component + options | ✅ | P3B.5: cascading dropdowns over the bundled library `cadrs_core/data/standard_content.ron` (ANSI inch: hex cap screws B18.2.1, hex nuts B18.2.2 Chamfered / Washer faced, pan head machine screws B18.6.3, plain washers B18.22.1; ISO 4762, 4032, 4035, 7089), then Size, Length, Thread length, Bearing face, Material, Finish (`course_asm_std_batch` 02–03, `course_asm_ex3_structure` 02, 21). Parts generated as parametric Part Studios (threads drawn as plain cylinders). |
| A19.2 | Auto-size from a hole or shaft (bolts round down, nuts round up) | ✅ | P3B.5: the Size row's Auto-size (selected or next clicked edge); `standard::auto_size` keeps the current size when it has that diameter, else the first (coarse) one; Ø0.266 hole → 1/4, Ø0.375 shaft → 3/8 (`standard_content.rs` `auto_size_rounds_bolts_down_and_nuts_up`). Frames: a wrong size, then Auto-size from the selected Ø0.266 holes → 1/4-20; a nut's Auto-size, armed, then a click on a rod's Ø0.375 end edge → 3/8-16 (`course_asm_std_batch` 02a–02b, 06a–06b). |
| A19.3 | Part number and Description (auto-filled) | ✅ | P3B.5: filled from the configuration ("HCS-1/4-28-0.75-SS", "Hex cap screw 1/4-28 x 0.75 Stainless Steel"), editable, stored per document with the configuration (`Document::standard_content`); P3B.6 reads them. The company-wide part is **out of scope** (permissions). |
| A19.4 | Preview drawing of the component | ✅ | P3B.5/P3B.6: an end and a side view of the generated part as line drawings (hidden lines removed, silhouettes of curved faces; `thumb::line_views`), as the course draws it (`course_asm_std_batch` 03, 12: the ISO 4762 hex socket). |
| A19.5 | Single placement on a hole connector; A flips; auto Fastened mate | ✅ | P3B.5: Insert with nothing selected arms placement; the fastener shows on the hovered hole edge, A flips it, a click inserts it with a Fastened mate (`course_asm_std_batch` 09–11; `nuts_go_out_of_the_part_on_either_side`). |
| A19.6 | Batch placement on preselected holes / faces | ✅ | P3B.5: one per selected hole edge, hole face or face with holes, each Fastened, one undo step (`course_asm_std_batch` 01, 04; `six_screws_on_the_retaining_plate`, `a_face_gives_its_holes`). |
| A19.7 | Insert closest / furthest from selection (stacking) | ✅ | P3B.5: closest puts it against the face and moves the stack out; furthest on top (`course_asm_std_batch` 07–08; `stacking_closest_and_furthest`). |
| A19.8 | Standard content icon in the list | ✅ | P3B.5: icon-rs `standard-content` (`course_asm_std_batch` 05). |
| A19.9 | Select same configuration / same part; Edit standard content instance (Size, Length); Update | ✅ | P3B.5: both Select items; Edit standard content instance with Standard…Component fixed, Size/Length, Update, ✓; bulk edit keeps ids and mates (`course_asm_std_bulk_edit` 01–05; `bulk_edit_three_sizes`). Final part 3: `course_asm_std_bulk_edit` 02–04 edit three screws at once; the dialog title counts them ("Edit standard content (3)"). |

### A20 Bill of Materials
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A20.1 | Live BOM panel | ✅ | P3B.6: a live table computed from the instances and the parts' properties ([`assembly::bom`](../../../crates/cadrs_core/src/assembly/bom.rs), [`properties`](../../../crates/cadrs_core/src/properties.rs)); every edit recomputes it (`course_asm_bom` 01). |
| A20.2 | Hover highlights; item number cross-highlights | ✅ | Hovering a row highlights its parts at any depth; clicking a row (its item number) selects its instances in the list and the view, and the row shows selected while they are (`course_asm_bom` 02–03). |
| A20.3 | Order follows the instances list; double-click header sorts | ✅ | Rows in the Instances list order; double-click a header sorts (again: the other way), item numbers stay with their items; overflow → Reset sort (`sorting_by_a_column`; `course_asm_bom` 12). |
| A20.4 | Structured (expand) vs Flattened | ✅ | Structured: 9 top-level rows on the Ex3 model, 14 with both subassemblies expanded (double-click the item number); Flattened: 11 rows, 31 parts (`structured_and_flattened_counts_on_ex3`; `course_asm_bom` 04–05). |
| A20.5 | Subassembly BOM behaviour (assembly and components / assembly only / components only) | ✅ | The subassembly's Properties → Subassembly BOM behavior: assembly and components / assembly only / components only (merged into the parent's rows) (`subassembly_bom_behaviour`; `course_asm_bom` 06–08). |
| A20.6 | Add column; Remove, Move left/right | ✅ | Add column (any property, custom ones too); header menu Remove column, Move left, Move right; one undo step each (`course_asm_bom` 09–11, `course_asm_bom_template` 01). |
| A20.7 | Apply / Save as template; Copy table; Export to CSV | ✅ | Apply template; overflow → Save as template… (saved in the document: templates are local), Copy table (tab-separated, system clipboard), Export to CSV (golden `tests/fixtures/ex3_bom.csv`) (`template_round_trip`, `csv_export_matches_the_golden_file`; `course_asm_bom_template` 02–08). |
| A20.8 | Suppress from BOM; show/hide excluded ("–" item number); unsuppress | ✅ | Row menu → Suppress from this BOM / Unsuppress in this BOM (a subassembly takes its components); overflow → Show / Hide excluded/suppressed: "–" item, no number, not counted (`suppress_from_bom`; `course_asm_bom` 13–15). |
| A20.9 | Show top-level assembly row with aggregated totals | ✅ | Overflow → Show top-level assembly row: the assembly itself with the total quantity (26 on Ex3) and mass, and the totals under the table; per-document property definitions stand in for company aggregated properties (`top_level_row_totals`; `course_asm_bom` 16). |
| A20.10 | Editable cells write back to properties (two-way); material picker cell | ✅ | Double-click a text cell to edit it in place; the Material cell opens the material picker (library and document materials, Assign material…); both write the part's own property, and the Properties dialog shows the same data (`bom_cells_and_properties_are_the_same_data`; `course_asm_bom` 17–21). |
| A20.11 | Generate missing part numbers (sequential numbering) | ✅ | Part number header menu (and overflow) → Generate missing part numbers: the document's sequential scheme (PRT-000001…), in BOM order, skipping numbers in use, keeping existing ones; a row's Generate next part number; the Properties dialog's Generate button (`generate_missing_part_numbers`; `course_asm_bom_template` 09–10). |

### A21 Exercise: Assembly Structure (stand-in: continues `pneumatic_cylinder_standin`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A21.1 | Copy; check Inch / Pound units | ✅ | The fixture is the copy; ☰ → Workspace units shows Inch / Pound (`course_asm_ex3_structure` 00). |
| A21.2 | Standard content Hex cap screw 1/4-28 × 0.75, SS, on 6 hole edges | ✅ | P3B.5: `course_asm_ex3_structure` 02–03 (Fastened 12–17). |
| A21.3 | Select same part and configuration → Add to folder "Hardware" | ✅ | P3B.5: `course_asm_ex3_structure` 04–06 (the Folder name popup at the menu point). |
| A21.4 | Move 4 instances to new subassembly | ✅ | `course_asm_ex3_structure` 07; `ex3_structure_steps`. |
| A21.5 | Rename "Top Cap subassembly", Fix Top Cap; 3 mates incl. the Fastened that replaces the Group | ✅ | Fastened 3, 4 and "Fastened 18" (the group's moved members); the screws' mates stay at the top across the level (`course_asm_ex3_structure` 08). |
| A21.6 | Create empty subassembly | ✅ | `course_asm_ex3_structure` 09. |
| A21.7 | Drag 4 instances onto it ("4 items") | ✅ | `course_asm_ex3_structure` 10–11 (the drop target lit). |
| A21.8 | Rename "Rear Cap subassembly", Fix Rear Cap | ✅ | Fastened 1, 2, Revolute 1, listed as `ex3-step8.png` lists them (`course_asm_ex3_structure` 12). |
| A21.9 | Move Piston & Rod + 3 O-rings to new subassembly | ✅ | `course_asm_ex3_structure` 13. |
| A21.10 | Rename "Piston subassembly", Fix | ✅ | Fastened 9–11 (`course_asm_ex3_structure` 14). |
| A21.11 | Drag Rear Cap mount out to the top level | ✅ | Revolute 1 comes out, holding it to the Rear Cap inside; the open subassembly shows its Items and Mate Features (`course_asm_ex3_structure` 15–16). |
| A21.12 | Dissolve Piston subassembly | ✅ | The piston slides again (its Fix inside isn't carried), the tab stays (`course_asm_ex3_structure` 17–18). |
| A21.13 | Delete the empty tab (tab menu items) | ✅ | Tab menu → Delete (`course_asm_ex3_structure` 19–20); Move to document… works (**P3G.3**, `course_er_move_to_document` 02); Copy to clipboard, Export (P3F.2), Create Drawing (P3C.1), Create task… as their rows say. |
| A21.14 | Standard content Hex nut 3/8-16 Chamfered SS Plain on 8 edges | ✅ | P3B.5/P3B.6: with the rods shown, all eight rod-hole edges picked in one view from above (4 on top of the Top Cap, 4 on the Rear Cap's flange, as `ex3-drawing.png` shows the nuts): a hole edge its rod fills stays pickable, and edges show through the clear barrel (`course_asm_ex3_structure` 20b–22). |
| A21.15 | Drag the 8 nuts into Hardware (14 items) | ✅ | P3B.5: "8 items" over the lit Hardware row; Hardware (14); the end state matches `ex3-drawing` (`course_asm_ex3_structure` 23–24, 26). |
| A21.16 | Mass properties of Top Cap subassembly | ✅ | **0.48362 lb, 5.05843 in³, CoM (0, 0, 6.03294) in** (its 4 instances; the screws stay in Cylinder assembly): `top_cap_subassembly_self_check` and, with the screws, `screws_stay_at_the_top_when_the_plate_moves_into_a_subassembly` (1e−4); the panel reads 0.484 lb, 5.058 in³, (0, 0, 6.033) in (`course_asm_ex3_structure` 25). |

## 4. Explicit mate connectors

### A22–A23 Explicit and implicit connectors
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A22.1 | Explicit connector where no implicit point exists | ✅ | P3B.7: the Mate connector feature places connectors where no implicit point exists (midway between lugs, `course_asm_ex4_connectors` 05; a sketch circle's centre, 07). |
| A22.2 | Part Studio connectors travel with the part; assembly connectors stay local | ✅ | Part Studio connectors are attached to their owner part's solid at rebuild (`Solid::connectors`) and so travel into every instance (`course_asm_ex4_connectors` 08; test `a_part_studio_connector_travels_with_its_part_and_follows_a_studio_edit`); assembly connectors live in `Assembly::connectors` (`course_asm_connector_tool_origin` 03, 06: no studio feature); P3B.8: each is a row of the Mate Features list with Rename, Edit… and Delete (07–09), and an instance's Part Studio connectors are listed under its ▸ (`course_asm_ex4_connectors` 10). |
| A22.3 | Switch to → Mate connector tool (Ctrl+M) → pick → ✓ | ✅ | Switch to → Ctrl+M (both tabs; the toolbar button too) → pick → ✓ (`course_asm_ex4_connectors` 02–05). |
| A22.4 | Between entities (midway) | ✅ | Origin type Between entities: the origin entity's point and its projection on the Between entity (a flat face's plane, an axis, a line), halfway (`course_asm_ex4_connectors` 04–05; test `between_entities_gives_the_midpoint_and_a_sketch_circle_its_centre`). |
| A22.5 | Owner part field | ✅ | Owner entity ✓ with its part field, filled from the first pick; sketch origins need one (`course_asm_ex4_connectors` 05, 07). |
| A22.6 | Flip, Reorient, Realign, Move | ✅ | Flip primary / Reorient secondary buttons, Realign (primary and secondary axes to edges, faces or sketch lines), Move (X, Y, Z, rotation) (`course_x11_mate_connectors` 01; test `realign_turns_the_secondary_axis_onto_an_edge`). |
| A22.7 | Connector shown on the instance, pickable in mates | ✅ | Instances draw their connectors (K toggles); the mate dialog takes the connector nearest the pointer before implicit points (`course_asm_ex4_connectors` 08–09, `course_asm_connector_tool_origin` 04). |
| A22.8 | Sketch entities as references | ✅ | Sketch points and sketch curves (a circle's or arc's centre, a line's midpoint) as origin, between or realign entities ("Edge of Hole Positions", `course_asm_ex4_connectors` 07). |
| A23.1 | Expand a mate to see its connectors; hover highlights | ✅ | A mate's ▸ (or Expand ▸ Mate connectors) lists its connectors; hovering one draws it in the hover colour (`course_asm_edit_implicit_connector` 03–04). |
| A23.2 | Right-click connector → Edit | ✅ | Right-click a connector row → Edit… opens the Mate connector dialog on it; the edits are stored on the mate's connector (`ConnectorEdit`) (`course_asm_edit_implicit_connector` 05–06). |
| A23.3 | Realign the secondary axis to an edge | ✅ | Realign → Secondary axis = the slot's edge: X along the slot; the part doesn't move, the slide direction does (the Pin slot now slides along the slot connector's X) (`course_asm_edit_implicit_connector` 07, 12; test `a_realigned_slot_connector_sets_the_pin_slot_direction`). |
| A23.4 | Move, Flip primary, Reorient secondary in the same dialog | ✅ | Move, Flip primary, Reorient secondary in the same dialog, previewed on the mate (`course_asm_edit_implicit_connector` 08–11; test `editing_a_mates_connector_realigns_moves_flips_and_reorients`). |

### A24 Exercise: Explicit Mate Connectors (stand-in: `fixtures/step_stool_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| A24.1 | Copy; Inch / Pound | ✅ | The fixture is the copy; P3B.9: its workspace units are **Inch / Pound** (`course_asm_ex4_connectors` 00), and the Mass properties panel's own mass unit is set to g, as the course's panel reads (12, 14). |
| A24.2 | Open the assembly (one hinge mate missing) | ✅ | `fixtures/step_stool_standin.cadrs`: Large Frame Bar (fixed), Base Frame Bar and Cross Bar (Fastened 1), no hinge (`course_asm_ex4_connectors` 01). |
| A24.3 | Switch to Base Frame Bar studio (3 parts) | ✅ | Switch to → the Base Frame Bar studio: Base Frame Bar, Cross Bar, Back Foot (`course_asm_ex4_connectors` 02). |
| A24.4 | Start Mate connector (Ctrl+M) | ✅ | Ctrl+M (`course_asm_ex4_connectors` 03). |
| A24.5 | Between entities: hole edge + opposite lug face, owner Base Frame Bar | ✅ | Between entities: the lug hole's inner rim, the other lug's inner face, owner Base Frame Bar: (0, −0.5, 23) in, Z +X (`course_asm_ex4_connectors` 04–05). |
| A24.6 | Switch to Large Frame Bar; Hole Positions sketch visible | ✅ | The Large Frame Bar studio with its Hole Positions sketch shown (`course_asm_ex4_connectors` 06). |
| A24.7 | Start another connector | ✅ | Ctrl+M again (`course_asm_ex4_connectors` 07). |
| A24.8 | On entity: sketch circle centre, owner Large Frame Bar | ✅ | On entity: "Edge of Hole Positions" (the circle's centre), owner Large Frame Bar (`course_asm_ex4_connectors` 07). |
| A24.9 | Both connectors show in the assembly | ✅ | Both connectors on their instances (they coincide on the hinge axis) (`course_asm_ex4_connectors` 08). |
| A24.10 | Revolute with limits −42.5°…0°; K hides connectors | ✅ | Revolute 1 between the two explicit connectors (the stand-in has no Revolute 1–4, so it is Revolute 1), limits −42.5°…0°; K hides the connectors (`course_asm_ex4_connectors` 09–10). |
| A24.11 | Drag to check; Reset | ✅ | Dragging stops at −42.5°; Apply limit position, Reset (`course_asm_ex4_connectors` 11–13). |
| A24.12 | Mass properties (g, g·in²) | ✅ | Mass properties in g and in² g: 18 131.792 g, CoM (0, −0.033, 11.642) in; at −42.5° (0, −2.422, 12.755) (`course_asm_ex4_connectors` 12, 14; test `ex4_step_stool_mass_properties_at_both_limits`). |

## Knowledge checks and survey
| ID | Requirement | Status | Notes |
|---|---|---|---|
| Quiz | Four "Begin Self-Check" quizzes (CoM entry) | out of scope | Learning-site feature. The underlying CoM values are checked by unit tests instead. |
| Survey | Completion survey | out of scope | Learning-site feature. |

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Assembly tabs; instances reference studio parts or assembly tabs and stay live | ✅ | Studio parts (P3B.1) and Assembly tabs (P3B.4, subassembly instances drawn from the tab's current state). |
| X2 | Instance model: transform, `<Name> <n>`, hidden/suppressed/fixed/transparent/isolated; multiples | ✅ | Transform, `<Name> <n>`, hidden, fixed; transparent/isolated as view state; multiples; copy/paste (P3B.2); Suppress / Unsuppress (P3B.4: greyed, struck through, not drawn or solved; `suppressed_instances_are_not_drawn_or_solved`). |
| X3 | Insert dialog (tabs, search, filters, rigid, placement rule, counter, undo) | ✅ | Current document, Part Studios and Assemblies tabs, search, filters, placement rule, counter, undo (P3B.1, P3B.4); Insert as rigid (P3B.8, `course_asm_rigid_insert` 01). |
| X4 | Triad manipulator | ✅ | Placed on the clicked entity, relocate with the connector snaps and Shift, Move to origin, Align/Anti-align, rotate 90/180, arrow/plane/ring drags with typed values; drawn over the parts (P3B.1, P3B.2). |
| X5 | Mate connectors as full frames (implicit, explicit, editing) | ✅ | Implicit connectors re-resolved by persistent names (P3B.2); P3B.7: explicit Part Studio connectors carried by their part and following studio edits, the assembly's own connectors, and editing a mate's connector (`ConnectorEdit`: between, realign, move). |
| X6 | Mate solver for 10 types, offsets, limits, Reset, Apply limit, drag, Solve, Fix, Group, DOF icons | ✅ | Solver (`cadrs_core::assembly::solver`): all ten types, offsets, per-DOF limits, Reset, Apply limit position, drag, Solve, Fix, Group, suppression, DOF icons (P3B.2, P3B.3; `assembly_solver.rs`, `assembly_mates.rs`). |
| X7 | Mate animation (▶ preview, Animate dialog) | ✅ | In-dialog ▶ and the Animate dialog (`assembly/animate.rs`; `course_asm_mates_pinslot` 08, `course_asm_animate`). |
| X8 | Assembly tree: lists, Items/Loads, filter, eyes, cross-highlight, folders, drag and re-parent | ✅ | Lists, eyes, cross-highlight (P3B.1–P3B.3); filter, folders, drag to reorder / into folders / into and out of subassemblies (P3B.4); Items (P3B.8); relations in the Mate Features list (P3B.9); P3F.5: **Loads (n)** with the loads' rows (A1.7). |
| X9 | Subassemblies (insert, move to new, create empty, drag, dissolve, flexible/rigid) | ✅ | All of it (P3B.4: `assembly_structure.rs`, `course_asm_subassemblies`); following a Named position (P3B.8, A16.2). |
| X10 | Visibility shortcuts Y, Shift+Y, J, H, K; Show all / Hide other; context-dependent keys | ✅ | Y and Shift+Y (P3B.1); J and H in assemblies (P3B.3); K in both tabs and Ctrl+M (P3B.7, `course_asm_connector_tool_origin` 05). |
| X11 | Switch to source studio or subassembly tab | ✅ | Part Studio (P3B.1); subassembly tab (P3B.4, `course_asm_subassemblies` 03–04). |
| X12 | Standard content library | ✅ | P3B.5: `cadrs_core::assembly::standard` + `data/standard_content.ron` (sources per row; data and geometry tests `every_bundled_size_matches_its_standard`, `every_bundled_size_generates_its_dimensions`). |
| X13 | BOM panel | ✅ | P3B.6: the BOM panel (A20) on the part property model (`properties`), usable by the drawings' BOM tables (`bom::compute_with`). |
| X14 | Assembly mass properties in assembly coordinates, after solving; units follow workspace | ✅ | Per instance transform in assembly coordinates, parallel-axis inertia, workspace units, measured on the solved placements (`course_asm_ex2_pneumatic` 24, 26). |
| X15 | Instance context menu (Properties … Delete) | ✅ | All of Onshape's instance menu works: Properties, Hide / Hide other / Hide all, Isolate, Make transparent, Suppress, Fix, Show/Hide mates, Switch to, Copy, Move to new subassembly, Add selection to folder, Create empty subassembly, Delete (P3B.1–P3B.8); P3B.9: **Check interference** (kernel Intersect of the placed parts, the pairs in red with their shared volume, `course_asm_interference`; `two_overlapping_boxes_report_their_common_volume`: 3000 mm³ and 90π), **Add mate connector to instance origin…** (`course_asm_where_used` 05–06), **Replace instances…** (mates kept on matching faces and edges, by name or geometry, `course_asm_replace`; `replace_keeps_the_mates_on_matching_faces`), **Edit in context** (the studio with the assembly as translucent context geometry: sketch on a face of another instance, Use its edges; Update context moves the references, `course_asm_edit_in_context`; `edit_in_context_holes_follow_the_base`), **Where used…** (`course_asm_where_used` 02–04), **Export…** (STEP of the instances where the assembly has them, `an_assembly_exports_its_parts_where_they_are`). **Change to version…** (P3G.2: the Reference manager, a workspace instance to a version and back, `course_er_change_to_version` 02–10) and **Create Drawing of …** (P3C.1, enabled in `course_er_insert_linked` 15b); a linked instance adds Update linked document…, Pin / Unpin reference (P3G.2) and Open linked document (P3G.3). Revision history (release management), Create task and Add comment (collaboration) are out of scope; Use best available tessellation, Select other and View normal to are drawn disabled with a tooltip. **P3E.3**: Section view… is enabled: Front plane through the instance, capped, ✓ keeps it (`course_td_section` 10–12; `a_section_through_a_40_mm_cylinder_caps_with_its_circle`). Final part 3: Check interference draws each shared volume solid red inside its see-through parts (`course_asm_interference` 03–06). |
| X16 | Seen but not taught: Relations, Simulation connection, Named positions, Exploded views, Replicate, Items/Loads, Configurations | ✅ | Named positions, Exploded views, Replicate, Items (P3B.8); P3B.9: **Relations** (A1.4). Configurations out of scope (niche; out of scope by user decision 2026-09-29). P3F.5: the **Simulation connection** checkbox (A6.3) and **Loads** (A1.7). |

## What each exercise needs
None of the four exercises can be rebuilt from the course (no part dimensions), and the course's
self-check values (**centre of mass**) are **blanked** in every screenshot. So each exercise runs on
a **stand-in fixture** built from our own features, with hand-computed checks. The Onshape values
(mass, volume, area, inertia) are recorded in the requirements file for reference only. All
stand-in values below were computed independently (closed forms: boxes, cylinders, rings; densities
Aluminium 6061 2.70, Polycarbonate 1.20, Nitrile 1.00, Steel 7.85 g/cm³; 1 g/cm³ =
0.0361273 lb/in³).

- **Ex1 Start an Assembly → `course_asm_ex1_start`**, fixture `motor_mount_standin.cadrs`
  (inch, lb). Part Studio "Motor Mount", one part "DC Motor Mount (stand-in)", Aluminium 6061:
  - base box x∈[−1.5, 1.5], y∈[0, 3], z∈[0, 0.5]; upright box x∈[−1.5, 1.5], y∈[2.5, 3],
    z∈[0.5, 3.5]; one Ø0.563 through hole in the base at (0, 1.75).
  - Studio values: V = 8.87553 in³, A = 45.38646 in², CoM (0, 2.13026, 1.13727) in,
    mass 0.86575 lb.
  - After the steps (hole's underside centre → origin, then Anti-align Z = 180° about X): **CoM
    (0.00000, −0.38026, −1.13727) in**, same mass. If the triad's X runs along Y, the
    anti-align is a 180° turn about Y instead and CoM Y is +0.38026. The test asserts the value for
    the transform the scenario actually applied, and |Y| = 0.38026 in both cases.
  - Needs: instances, Insert dialog, triad (relocate, Move to origin, Anti-align), Fix, assembly
    mass properties with instance transforms, lb units (P3.5).
- **Ex2 Pneumatic Cylinder → `course_asm_ex2_pneumatic`**, fixture
  `pneumatic_cylinder_standin.cadrs` (inch, lb). One Part Studio "Cylinder parts", every part at
  its assembled position (axis Z):
  | Part | Shape | Material |
  |---|---|---|
  | Rear Cap | box 2.5×2.5, z 0–0.75; spigot Ø1.5 z 0.75–1.25; recess Ø1.0 z 0–0.1; 4× Ø0.375 thru at (±0.95, ±0.95) | Al |
  | Barrel | tube Ø2.0/Ø1.75, z 0.75–5.75 | PC |
  | Top Cap | box 2.5×2.5, z 5.75–6.5; spigot Ø1.5 z 5.25–5.75; Ø0.5 thru; 4× Ø0.375 and 6× Ø0.266 (r 0.55) thru the box | Al |
  | Retaining Plate | disc Ø1.5, z 6.5–6.625; Ø0.5 and 6× Ø0.266 (r 0.55) holes | Al |
  | Rear Cap mount | cylinder Ø1.0, z −0.4–0.1 (fills the recess) | Al |
  | O-Ring 0.125 ×4 | ring Ø1.5/Ø1.75 × 0.125 at z 0.75, 1.0, 5.625, 5.375 (Fastened, offsets 0 / 0.25) | NBR |
  | Piston & Rod | Ø1.5 z 1.25–2.0 + rod Ø0.5 z 2.0–8.0 | Steel |
  | O-Ring 0.185 ×3 | ring Ø1.5/Ø1.75 × 0.185 stacked from z 1.25 | NBR |
  | Structural Rod ×4 | Ø0.375, z −0.5–7.0 (Fastened, Offset Z 0.5) | Steel |
  - End state: 17 instances, 14 mates (Group 1, Revolute 1, Fastened 1–11, Slider 1), slider
    limits 0…3.25 (the stand-in's travel; the course's is 4.5).
  - After Reset: **mass 2.85319 lb, V 20.62443 in³, CoM (0, 0, 3.16698) in**. At the slider's
    upper limit (Apply limit position): CoM Z 3.99028 in.
  - Needs: multi-insert, Group, Revolute, Fastened with offset, Slider with limits, Reset, copy/paste
    instances, hide.
- **Ex3 Assembly Structure → `course_asm_ex3_structure`**, continues the Ex2 end state.
  - Standard content: 6× Hex cap screw 1/4-28 × 0.75 on the Retaining Plate's Ø0.266 holes, and
    8× Hex nut 3/8-16 on the rod holes (4 on top of the Top Cap, 4 on the Rear Cap flange, z 0.75 in, as in ex3-drawing and ex3-step14). Geometry
    from the public ASME B18.2.1/B18.2.2 dimensions (recorded with sources in the data file).
  - End state: Instances list = Cylinder assembly, Origin, Rear Cap mount, Barrel (fixed), Rear Cap
    subassembly, Top Cap subassembly, 4 rods, Piston & Rod, 3 O-Ring 0.185, Hardware (14).
  - Self-check, Top Cap subassembly (Top Cap, Retaining Plate, O-Ring 0.125 <3>, <4>; positions
    kept by Move to new subassembly): **mass 0.48362 lb, V 5.05843 in³, CoM (0, 0, 6.03294) in**.
  - Needs: standard content (auto-size: a Ø0.266 hole gives 1/4, a Ø0.375 shaft gives 3/8),
    batch placement, select-same, folders, subassemblies (move to new, create empty, drag in/out,
    dissolve), tab delete.
- **Ex4 Explicit Mate Connectors → `course_asm_ex4_connectors`**, fixture
  `step_stool_standin.cadrs` (inch and pound, as the course sets them; the Mass properties panel set to g, as the course's panel reads). Part Studios "Large Frame
  Bar" (one part + a "Hole Positions" sketch on Right with a Ø0.5 circle at (y −0.5, z 23)) and
  "Base Frame Bar" (Base Frame Bar, Cross Bar, a third part "Back Foot" optional), all Aluminium:
  - Large Frame Bar: box x∈[−6, 6], y∈[0, 1], z∈[0, 24] (fixed).
  - Base Frame Bar: plate x∈[−5, 5], y∈[−1.5, −1], z∈[0, 22] + lugs x∈[−5, −4.5] and [4.5, 5],
    y∈[−1.5, 0], z∈[22, 24] with Ø0.5 hinge holes on the axis (y −0.5, z 23). (P3B.7: the first
    spec had the lugs at y −1…0, touching the plate only along an edge, which the kernel keeps as
    separate bodies, so separate parts; the lugs now stand on the plate's top face.)
  - Cross Bar: box x∈[−4.5, 4.5], y∈[−2.5, −1.5], z∈[4, 5], Fastened to the Base Frame Bar.
  - Connectors: Between entities (lug hole edge + opposite lug inner face) → (0, −0.5, 23);
    On entity (sketch circle) → the same point. Revolute 5 with limits −42.5°…0°.
  - After Reset (0°): **mass 18 131.792 g (39.974 lb), V 409.8037 in³, CoM (0, −0.03331,
    11.64212) in**. At −42.5° (the opening direction): CoM (0, −2.42189, 12.75461) in. (First
    spec, edge-touching lugs: 18 087.547 g, CoM (0, −0.03034, 11.61434).)
  - Needs: explicit connectors in Part Studios (Between entities, On entity with a sketch
    reference, Owner), connectors travelling into the assembly, Revolute with angle limits, drag,
    Reset, K, gram units.

Each self-check gets a unit test in `crates/cadrs_core/tests/course_assemblies.rs` that builds the
stand-in from features, solves the assembly and compares with the closed-form values above to
1e−4 (relative), and the scenario's mass-properties panel must show them rounded to 3 decimals.

## Proposed milestones (stage 3B)
Order: instances and the Ex1 flow first, then the solver with Ex2, then breadth. Every milestone
keeps `cargo test --workspace` and clippy clean, gives UI elements a `Name`, routes every change
through the command and undo layer, and ends with a fresh-judge round (≥ 8.5) against
`intro-to-assemblies/`.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3B.1 | **Instances, Insert, triad, Fix, assembly mass properties** | A1.1, A1.6, A1.9 (mass), A2.1, A2.3 (current document, Part Studios), A2.6, A3.1–A3.6, A3.7 (Fix), A4.1–A4.6 (Part Studio switch), A5.*, X1, X2, X3, X4, X10 (Y, Shift+Y), X11, X14, X15 (core items) | `course_asm_ex1_start` reproduces A5.2–A5.10 on `motor_mount_standin` and the panel reads **CoM (0.000, −0.380, −1.137) in, mass 0.866 lb, V 8.876 in³**; a unit test asserts the closed form to 1e−4. `course_asm_insert_placement` shows inside-dialog vs click placement and "Inserted: 3". `course_asm_hide_show` covers eye, Hide other, Isolate, Y/Shift+Y. Editing the studio's extrude depth updates the instance live. Undo removes inserts one by one. |
| P3B.2 | **Implicit connectors, solver, first mates, Group, copy/paste** | A1.2 (implicit), A1.3 (Fastened/Revolute/Slider/Cylindrical/Group), A1.7 (mates), A3.2 (full point set), A6.1–A6.8, A6.10 (these types), A6.11, A7.1–A7.4, A8.1, A8.3 (mate), A8.4, A9.1, A13.*, A15.*, A16.1, X5 (implicit), X6 (part) | `course_asm_ex2_pneumatic` ends with 17 instances and 14 mates; after Slider Reset the panel reads **mass 2.853 lb, V 20.624 in³, CoM (0, 0, 3.167) in**, and Apply limit gives CoM Z 3.990; unit tests assert both to 1e−4. Solver unit tests: each of the 4 types leaves exactly its A7 DOF (rank of the constraint Jacobian), Offset Z 0.5 moves the rod by 0.5, a limit clamps a drag. Solve on a 50-instance chain < 50 ms. |
| P3B.3 | **All mates, motion, mate list UI** | A6.9, A6.10 (remaining types), A6.12, A6.13, A7.5–A7.10, A8.2, A8.3 (Animate), A9.2, A9.3, A10.*, A11.*, A12.*, A14.*, A16.4, A16.5, X6, X7, X10 (J, H) | Unit test per mate type (all 10): perturb, solve, check the constraint and the DOF count of A7. Width test: two tabs stay mirror-symmetric; Tangent test: a Ø20 cylinder on a plane stays at distance 10. Scenarios `course_asm_mates_pinslot` (limits ±½ slot, rotation locked), `course_asm_mates_tangent_width`, `course_asm_animate` (Reciprocate frames), `course_asm_show_mates` (J, H, pinned). |
| P3B.4 | **Subassemblies, folders, list filter** | A1.5, A2.3 (Assemblies tab), A3.7 (not carried), A4.6 (subassembly), A16.2 (flexible/rigid), A16.3, A17.*, A18.*, A21.3–A21.13, X8, X9 (part), X11 | Unit tests: Move to new subassembly keeps every world transform and moves the mates; Dissolve restores the flat structure; a Fix in the child is ignored in the parent; folder Delete removes contents, Unpack doesn't; all undoable. `course_asm_subassemblies` (move, create empty, drag in with "4 items", drag out, dissolve, empty tab kept) and `course_asm_folders`. |
| P3B.5 | **Standard content** | A19.*, A21.* (with P3B.4), X12 | Data test: every bundled size's head/nut dimensions match the cited standard table; auto-size picks 1/4 for a Ø0.266 hole and 3/8 for a Ø0.375 shaft. `course_asm_ex3_structure` reaches the A21 end state; Top Cap subassembly reads **mass 0.484 lb, V 5.058 in³, CoM (0, 0, 6.033) in** (unit test to 1e−4). `course_asm_std_batch` places 6 screws from a preselection with 6 Fastened mates; `course_asm_std_bulk_edit` changes 3 sizes at once. |
| P3B.6 | **Part properties and the BOM panel** | A1.8 (BOM), A20.*, X13 | Property model (Name, Part number, Description, Material, Revision, Vendor, Unit of measure, Category…) on parts and assemblies, with a Properties dialog. Unit tests: editing a BOM cell writes the part property and vice versa; flattened vs structured counts on the Ex3 model; "Generate missing part numbers" fills unique sequential numbers; CSV export matches a golden file. Scenarios `course_asm_bom` (columns add/move/remove, sort, suppress row, show excluded, subassembly behaviour) and `course_asm_bom_template`. |
| P3B.7 | **Explicit mate connectors and the step stool** | A1.2 (tool), A22.*, A23.*, A24.*, X5, X10 (K) | `course_asm_ex4_connectors` on `step_stool_standin`: Between-entities and On-entity connectors in the studios appear on the instances; Revolute 5 limited −42.5°…0°; after Reset the panel reads **mass 18 131.792 g, CoM (0, −0.033, 11.642) in** (lugs on the plate's top face, see the Ex4 spec); at −42.5° CoM (0, −2.422, 12.755); unit test to 1e−4. `course_asm_edit_implicit_connector` realigns a pin-slot connector's X to an edge. |
| P3B.8 | **Rigid studio insert, named positions, exploded views, replicate, Items** | A1.7 (Items), A1.8 (Exploded views, Named positions), A2.4, A16.2 (follow position), X9 (rest), X16 (part); A1.8 (Configurations) and A2.5 out of scope (configurations: niche; out of scope by user decision 2026-09-29, ORCHESTRATOR.md speed-up rule 4) | ~~Configurations: a list input "Size" (S/M/L) driving an extrude depth; inserting config M gives V = closed form; changing the instance's configuration updates it~~ (out of scope, as above). Rigid Part Studio insert moves as one and its Edit adds or removes parts. Named position "Open" restores the Ex4 stool at −42.5° (CoM check). Exploded view scenario with step list and slider. Replicate places a screw on 6 matching holes (6 instances, 6 mates). |
| P3B.9 | **Relations and the rest of the instance menu** | A1.3 (relations), A1.4, X15 (Check interference, Replace instances, Edit in context, Where used, Add mate connector to instance origin), X16 (Relations) | Unit tests: a gear relation with ratio 2:1 turns the driven revolute −45° when the driver turns 90°; rack and pinion moves 2πr per turn; screw moves one pitch per turn. Check interference of two overlapping boxes reports the closed-form overlap volume. Replace instances keeps the mates when the new part has matching faces. `course_asm_edit_in_context` sketches on a face of another instance and the reference follows. Change to version / Export / Create Drawing items are enabled when P3D.3 / P3F.2 / P3C.1 land. **Done (P3B.9):** all of it; Change to version, Create Drawing wait for P3G.2 (was P3D.3) / P3C.1 (disabled with a tooltip); Export is enabled (STEP of the instances). |

**P3B.1 details:**
- `ElementKind::Assembly` gains an instance tree (id, source ref `(element, part)` or
  `(assembly element)`, transform, name `<Name> <n>`, flags), all mutated through commands.
- Instances render from the source studio's cached tessellation (P3.1), transformed; the source
  stays live, so a studio edit shows at once.
- Insert dialog (current document → Part Studios): tree with thumbnails, search, filters,
  "Inserted: n", Undo to remove; the placement rule.
- Triad: relocatable origin with snaps (centroid, circle centre, vertex, midpoint), Shift lock,
  arrow/plane/ring drags with value boxes, Move to origin, Align/Anti-align with Z, 90°/180°.
- Fix / Unfix with list icons; instance context menu core (Hide, Hide other, Hide all, Isolate,
  Make transparent, Fix, Switch to, Delete); empty-space menu (Show all, Show all instances, Zoom
  to fit, Isometric); Y / Shift+Y.
- The P3.3 mass-properties panel accepts instances and the root; aggregates per instance with the
  instance transform, in assembly coordinates; inertia via the parallel-axis theorem once P3.5
  lands.
- The `motor_mount_standin` fixture and a harness step to load a fixture into a fresh document.

**P3B.2 details:**
- Mate connector frames from P3.8; the implicit point finder over kernel faces and edges
  (centroid, midpoints, vertices, circle centres, negative-space centres, virtual sharps).
- Solver in `cadrs_core` (no Bevy): rigid bodies per instance or group, mates as residuals on
  frame pairs (Newton / Levenberg–Marquardt with the fixed instance as ground), limits as clamped
  inequalities, offsets as frame transforms. Solve only on pick, Solve and accept (A6.11).
- Mate dialog chrome: type dropdown (changeable), connector field with Reorder, Offset, Limits,
  Flip, Reorient, Solve; mates Fastened, Revolute, Slider, Cylindrical; Group; Reset.
- Instance clipboard: Copy, Paste (Ctrl+C/V in assemblies).
- DOF icon per instance.

**P3B.3 details:** the other six mate types; drag-to-move under the solver; in-dialog ▶ and the
Animate dialog; mate list tooltips, limit icon, context menu (Apply limit position, Reset, Show,
Isolate, Suppress, Animate, Rename, Delete); J, show-mates mode H with pinning, cross-highlight.

**P3B.4 details:** assembly-tab instances (the Insert dialog's Assemblies tab), flexible or rigid
subassembly instances, Move to new / Create empty subassembly, drag in and out with mates, Dissolve,
folder model in both lists, the filter field (shares P3.9's filter engine), Fix scoping per level.

**P3B.5 details:** a bundled, data-driven fastener library (ANSI inch hex cap screws, hex nuts,
pan head machine screws, plain washers; ISO 4762, 4162, 4035, 7089 metric subsets) generated as
parametric parts with configuration-like options; Auto-size; single and batch placement with
auto Fastened mates; A to flip; stacking; select-same; bulk edit; its own list icon.

**P3B.6 details:** the property model (used later by drawings' title blocks and BOM tables,
P3C.4–P3C.5, and the test drive, P3E.5); the BOM panel with structured/flattened views, columns,
templates, CSV, suppression, two-way edits, material picker, part-number generation.

**P3B.7 details:** Mate connector feature in Part Studios (On entity, Between entities, Owner,
Realign, Move, flip, reorient, Ctrl+M) that travels with the part; assembly-level explicit
connectors; editing implicit connectors under an expanded mate; K.

**P3B.8 details:** ~~a Configurations panel (list, checkbox and quantity inputs driving feature
parameters and suppression, per Part Studio and Assembly) used by the Insert dialog~~ (out of
scope: configurations are niche, out of scope by user decision 2026-09-29, ORCHESTRATOR.md
speed-up rule 4); rigid Part Studio instances with Edit; Named positions and Lock/follow;
Exploded views (steps, slider, trails); Replicate (instance pattern on matching geometry); the
Items list.

**P3B.9 details:** gear, rack and pinion, screw and linear relations between mates; Check
interference (kernel boolean Intersect volume); Replace instances; Edit in context (the studio opens
with the assembly shown as context geometry that can be used as references); Where used; Add mate
connector to instance origin.

Not scheduled: nothing. The quizzes and the survey are out of scope (learning-site features); the
company-wide part of A19.3 is out of scope (permissions); "Add comment" in the mate menu is out of
scope (collaboration).

## P3F.5 status (this course's simulation rows)
**Built** (judge pending); the milestone is P3F.5 of
[intro-to-parametric-cad-gaps.md](intro-to-parametric-cad-gaps.md), where its full status is.
- **A1.7, X8 (Loads)**: the Instances list's **Loads (n)** (`simulation_ui::LoadsHeader`,
  `LoadRows`): a row per load with its icon (Fixed, Force, Pressure), name and value
  ("100 N along −Z"); double-click opens it in the Simulation panel.
- **A1.8 (Simulation)**: the right strip's **Simulation** panel works in assemblies as in Part
  Studios: the instances' materials, the Loads list, Mesh, Solve, results (von Mises or
  displacement on the deformed shape, legend, probe).
- **A6.3, X16 (Simulation connection)**: the mate dialog's checkbox is stored on the mate
  (`Mate::simulation`, saved, undoable); `simulation::assembly_bonds` turns every checked,
  unsuppressed mate between part instances into a bond (penalty ties, both ways, of each part's
  surface nodes to the other's faces where they touch). A mate whose parts don't touch is an error naming it; a
  mate on a subassembly instance is noted and left out.
- **Numbers** (`cadrs_core/tests/simulation.rs`, `simulation_halves` fixture): the course beam as
  two 50 mm halves joined by Fastened 1 with Simulation connection deflects 0.20040 mm at the
  tip against the solid beam's 0.20037 mm (+0.02 %; the gap list asks for 5 %); unchecked, "Half
  2 <1> is free to move".
- **Scenario** `course_asm_simulation` 01–08: the assembly, Fastened 1 with Simulation connection
  checked, Fixed on Half 1, Force on Half 2, the Loads rows in both lists, the von Mises map over
  both halves (continuous across the joint), the probe at the joint (30.3 MPa), and the error
  once the connection is unchecked.

## Risks
- **Solver robustness.** Mixed mates with limits (inequalities) and closed loops (the stool, gears)
  make convergence and "which instance moves" hard. Use a least-squares solver with a
  minimum-motion regulariser, solve per rigid cluster, and add a regression suite of loop cases.
- **Persistent naming for mate connectors.** Implicit connectors reference faces and edges of the
  source parts; a studio edit must not silently move them. Depends on P3.2; add edit-then-resolve
  tests for connectors.
- **Rendering many instances.** Share one mesh per source part and instance it; the Ex3 model has
  26 rows, but the test-drive model (P3E.5) and T4 targets need hundreds.
- **Standard-content data.** Fastener dimensions must come from public standards tables, with the
  source recorded per row; the library is a large data task. Keep the first set to the sizes the
  courses use, with the generator ready for more.
- **Stand-ins can't match Onshape's numbers.** The judges can only compare workflow and UI with
  the screenshots; the numbers are our closed forms.
- **Cross-stage dependencies** (see the summary): stage 3B stays open until P3C.1, P3D.3, P3E.3,
  P3G.1–P3G.2 (was P3F.1), P3F.2, P3F.4 and P3F.5 land, unless the orchestrator pulls those
  forward. (All of them have landed.)
- ~~**Configurations** are also a Part Studio concept (PS9.8, PS2.10); P3B.8 must build them for
  both, not only for assemblies.~~ Out of scope (user decision 2026-09-29, speed-up rule 4).
