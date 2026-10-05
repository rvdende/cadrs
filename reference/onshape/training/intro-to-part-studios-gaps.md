# Introduction to Part Studios: gap analysis (2026-09-26)

This maps [intro-to-part-studios.md](intro-to-part-studios.md) against the cadrs code at `main`
(`69ae705`). ✅ done · 🟡 partial · ❌ missing. File references are to the code at the time of
writing. Related: [intro-to-sketching-gaps.md](intro-to-sketching-gaps.md) (the `S*` items),
[intro-to-parametric-cad.md](intro-to-parametric-cad.md) (the `P*` items), and
[crates/cadrs_kernel/README.md](../../../crates/cadrs_kernel/README.md) (the solid-modelling layer).

**Summary:** cadrs has the Part Studio *shell* and the sketch-to-part path, but not a solid
modeller yet. What works today:
- Region-based input: click to toggle regions, holes found by the region finder
  (`crates/cadrs_app/src/region_select.rs`, `extrude.rs`).
- An Extrude dialog that does **Solid / New / Blind** only, with a flip button and a drag arrow
  (`crates/cadrs_app/src/extrude.rs`).
- A prism mesh per extrude (`crates/cadrs_core/src/solid.rs`). It keeps face and edge tags
  (`FaceTag`/`EdgeTag`) and supports face hover, sketch on face, Use and imprinting. **Since P3.1
  (2026-09-27) extrudes are OCCT bodies** rebuilt through `cadrs_kernel` (see the P3.1 status
  below); the prism mesh is only the fallback without the `occt` feature.
- A "Parts (n)" list (`crates/cadrs_app/src/document.rs`, `parts.rs`).
- In-order regeneration of sketches on faces and of links (`crates/cadrs_core/src/parts.rs`).
- Workspace length units, including inch.

Every extrude makes its **own part**. There are no booleans (Add/Remove/Intersect), end types
other than Blind, or mass properties, and no other features. The course's Revolve, Boolean, Plane,
Fillet, Chamfer, Hole, Shell, Sweep, Loft, pattern and Mirror features are all missing.

Several pieces of UI are placeholders: most toolbar buttons are disabled, and the Folder, Pause
and Regeneration-time buttons in the feature-list header do nothing. The side-panel icons and the
Measure and Mass-properties icons do nothing either. The filter field accepts text but doesn't
filter. The rollback bar is drawn but can't be dragged. The Extrude footer slider is a "Preview
detail" slider, not the rollback preview.

The plan therefore starts by moving the feature list onto `cadrs_kernel` with the OCCT backend
(P3.1–P3.2). After that, each exercise gives a milestone its acceptance test.

**Counts (182 PS rows + 14 X rows = 196), after stage 3E (2026-10-02):** ✅ 195 (PS 181, X 14) · out of scope 1 (PS9.8, configurations) · owned by another stage 0 · 🟡/❌ 0. The five rows owned by other stages were checked against what they built and ticked: PS2.9, PS2.11 and X14 (P3E.3: render modes, section view, Measure, Analysis), PS2.12 (P3C.1 drawing tabs) and PS15.9 (P3C.3 hole callouts). **After P3.11:** every row ✅ or placed: ✅ 190 (PS 177, X 13; some with an out-of-scope part, below) · owned by another stage 5 (PS2.9, PS2.11 → P3E.3 for their section/render and Measure/Analysis parts; PS2.12 → P3C.1 for drawing tabs; PS15.9 → P3C.3; X14 → P3E.3) · out of scope 1 (PS9.8, configurations). **After P3.6:** ✅ 84 · 🟡 43 · ❌ 69 (PS alone: 80 / 36 / 66; X: 4 / 7 / 3). After P3.4: ✅ 46 · 🟡 37 · ❌ 113. After P3.3: ✅ 34 · 🟡 38 · ❌ 124. At the analysis: ✅ 11 · 🟡 45 · ❌ 140.

## 1. Introduction

### PS1 Start a part
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS1.1 | Faces or regions as input; whole sketch = outer boundary minus holes | ✅ | P3.3: a sketch clicked in the feature list while the Extrude dialog waits for input is taken whole (P3.4: the field reads "Face of Sketch 1", or "Faces of Sketch 1" for several regions, as in ex2-step3/ex2-step5): its regions nested an even number of times (`rebuild::whole_sketch_regions`: a plate less its hole, a boss in the hole kept again); surface and thin extrudes take its curves, chained end to end. Planar part faces are input too (PS4.2). Scenario `course_ps1_whole_sketch`; test `whole_sketch_input`. |
| PS1.2 | Any combination of regions; grey fill | ✅ | Click toggles a region in the field; the fill and orange outline are drawn in `extrude.rs` (`RegionFill`, `RegionGizmos`). |
| PS1.3 | One master sketch drives several features | ✅ | P3.3: the eye shows a used sketch again, so another feature can take other regions of it (`course_ps1_whole_sketch` 04, `course_ps6_control_arm` 07). |
| PS1.4 | Sketch not consumed; auto-hidden on first use | ✅ | `consumed_sketches` / `PartCache::hidden_sketches` (`cadrs_app/src/parts.rs`); the row is greyed. P3.1: unchanged on the kernel rebuild (`course_ps_kernel_extrude` 03–05: both sketches greyed and hidden). |
| PS1.5 | Eye icon shows a hidden sketch again | ✅ | P3.3: sketch rows have Onshape's eye (shown on hover, "Show Sketch 1"/"Hide Sketch 1"), an undoable `SetSketchVisibility` kept outside the feature list; a sketch shown this way is hidden again when a feature that uses it is accepted (as after Extrude 2 in `ex1-step5`). `course_ps6_control_arm` 06–08. |
| PS1.6 | Revolve and Sweep take regions the same way | ✅ | P3.4: Revolve takes regions and whole sketches (from the feature list) exactly as Extrude does (`sweep_groups`); surface and thin revolves take a whole sketch's curves, open chains included. Sweep in P3.7. `course_ps7_revolve_types`, `course_ps8_reducer_coupling` 06. P3.10: Sweep too: a whole sketch picked in the feature list sweeps as its regions do, and a lost region is left out with a warning, as the extrude's (test `sweep_takes_a_whole_sketch_like_its_regions`: π·25·50 either way). Fix round 1: `course_p310_profiles` 01 (Sketch 1 picked whole in the feature list as the Sweep's profile). |

### PS2 Part Studio interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS2.1 | Undo/Redo buttons; Versions and history panel | ✅ | Buttons and per-session history work (`cadrs_app/src/lib.rs` `ActiveDocument`); every P3.9 list action (suppress, rollback bar, folders) is one undo step (`course_ps13_rollback_final` 12, `course_ps3_folders` 13). The detailed Versions and history panel: **out of scope** (niche; out of scope by user decision 2026-09-29). |
| PS2.2 | Context-dependent toolbar | ✅ | The Part Studio toolbar and the sketch toolbar swap with the context (`document.rs` `part_studio_toolbar`, `sketch.rs` `sketch_toolbar`). P3.11 audit (the old text, "only Sketch and Extrude enabled", was stale): every feature tool this course uses is enabled (Sketch, Extrude, Revolve, Sweep, Loft, Fillet, Chamfer, Draft, Shell, Hole, the Linear/Circular/Curve pattern menu, Mirror, Boolean, Split, Plane, Mate connector) and in a sketch every sketch tool of the Introduction to Sketching course (Use, Line…, Dimension, the constraints). The greyed buttons are tools no phase-3 course teaches (Thicken, Rib, Thread, Transform, Add custom features; the sketch Intersection, sketch pattern, Insert image, Insert DXF; since the Final re-audit the sketch Spline button draws Bézier curves, its fit-point Spline disabled in its ▾); they stay on the bar as in `screens/05a`/`05b`/`08a`. Search tools follows the swap (`course_ps2_search_tools` 07–08). |
| PS2.3 | Search tools (Alt+C) | ✅ | P3.9: clicking the box or Alt+C opens a field over it (`cadrs_ui::CommandPalette`) listing the toolbar's tools whose names match, with their shortcuts, the pattern menu's three patterns included and missing tools greyed; ↑/↓, Enter or a click launches the tool as its button does, Esc closes (`search_tools.rs`; unit test `search_finds_tools_by_name`). In a sketch it lists the sketch tools. `course_ps2_search_tools` 01–08. |
| PS2.4 | Features list with count and filter | ✅ | "Features (n)", in regeneration order; the filter works (PS3.4–3.6). `course_ps3_filter`. |
| PS2.5 | New folder button | ✅ | P3.6: puts the selected features in a new folder; P3.9 adds the menu's Add selection to folder… (PS3.1). |
| PS2.6 | Show regeneration times | ✅ | P3.9: the stopwatch toggles (pressed look) a time on every part feature's row, from the rebuild's per-feature timing (`Build::times`: a feature taken from the cache keeps the time it took when computed; core test `regeneration_times_are_measured_per_feature`). The Gear Cover stand-in shows tens of ms per feature (`course_ps2_regeneration_times` 02; real timings, so they change from run to run). Sketch rows show no time (their solve isn't part of the rebuild). |
| PS2.7 | Default geometry: Origin, Top, Front, Right | ✅ | |
| PS2.8 | Parts list: Parts / Surfaces / Curves groups, rename, hide… | ✅ | P3.3: "Parts (n)" and "Surfaces (n)" groups; right-click: Rename (in place), Hide/Show (greyed row with a crossed eye), Isolate (with "Exit isolate"), Delete (a "Delete part N" feature, as Onshape). P3.5: Assign material…, Edit appearance… and Make transparent… (a view state; the item reads Make opaque while it is on) work, as in `ex1-step5`; a right-click no longer selects the part (it stays its colour in the view) and its row gets a light context band (`ex2-step8`). Properties and the others stay disabled. `course_ps2_parts_list`, `course_ps9_appearance` 02, 19. P3.11 audit, **Curves**: Onshape lists a group only when it has entities (`ex5-step2.png`: Parts (1), Surfaces (1), no Curves), and curve bodies come from the curve features (Helix, Composite curve, Projected curve, Bridging curve…), none of which this course or cadrs has; sketches are not curve bodies. No feature can make one, so the group never shows (as in every course screenshot). |
| PS2.9 | View cube with camera and render options, section | ✅ | View cube, arrows and view menu (Isometric, Dimetric, Trimetric) in `view_cube.rs`. P3.9: a **corner** (the square patch where three faces meet, lit on hover) gives the trimetric view from that corner (P6.4, `course_p6_cube_corner`, test `corners_turn_to_trimetric_views`). **P3E.3** (stage 3E): the view cube menu's six render modes, Perspective view, Zoom to window, Previous view, Named views… (per tab) and Section view… (`course_td_render_modes` 01–25: the menu at 02; `course_td_section` 01–07: Front plane, capped, flipped, offset 25 mm with its arrow; `render_modes_are_kept_per_tab`, `the_previous_view_stack`, `named_views_are_undone_and_persist`, `a_section_through_a_40_mm_cylinder_caps_with_its_circle`). (Before: ✅ / → P3E.3.) |
| PS2.10 | Side panels (Appearances, Configurations, Custom tables, Variables) | ✅ | Icons in the right rail (`document.rs` ~l.685). P3.5: **Appearances** opens its panel (PS9.7); **Variables** opens the Variable table (P3F.4: each Variable with its value, editable, its description and errors). P3.6: the panels dock full height against the rail and the open panel's rail icon shows pressed; one panel at a time. P3.11: **Custom tables** opens its panel (`appearance.rs` `sync_custom_tables_panel`): Onshape lists there the tables FeatureScript table features define, and cadrs has no FeatureScript, so it says so, with Add custom table… disabled (FeatureScript tables: **out of scope**, niche; out of scope by user decision 2026-09-29). **Configurations**: greyed, **out of scope** (niche; out of scope by user decision 2026-09-29). `course_ps9_appearance` 20, 23, 23a, 24. |
| PS2.11 | Measure, Analysis, Mass properties tools | ✅ | P3.3: the Mass properties tool opens the Mass and section properties panel (X7, ✅). **P3E.3**: the bottom-right Measure (readout and panel: area, length, distance with Minimum / Maximum, angle, a vertex's X Y Z; `course_td_measure` 01–07; `measure_analysis.rs`: `a_100_by_60_by_25_box`, `two_boxes_15_apart`, `a_40_diameter_cylinder`) and Analysis (draft analysis with its pull direction and bands, curvature map, curvature combs, zebra stripes; `course_td_analysis` 01–11; `a_5_degree_drafted_face_falls_in_the_3_to_6_band`, `analysis.rs` tests). (Before: ✅ / → P3E.3.) |
| PS2.12 | Document tab bar with + and element tabs | ✅ | Part Studio and Assembly tabs, +, rename, duplicate, delete. Drawing tabs: built in **P3C.1** ("+" → Create Drawing…, D1.2 ✅ in `intro-to-drawings-gaps.md`; `course_drw_create`). (Before: ✅ / → P3C.1.) Image tabs: **out of scope** (niche; out of scope by user decision 2026-09-29). |

### PS3 Filtering and organising the feature list
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS3.1 | Add selection to folder; expand and collapse | ✅ | P3.6: folders, the header's New folder, a folder row "Base Features (6)" with a chevron. P3.9: the feature menu's **Add selection to folder…** puts the selection (or the row) in a new open folder with its name in edit (`course_ps3_folders` 01–02); rename in place (double-click or the folder menu's Rename, `SetFolder`, 03); the chevron closes and opens it (04–05); a folder's features are indented under it. A right-click on another feature makes it the selection, on a selected one keeps the selection. |
| PS3.2 | Drag features into and out of folders | ✅ | P3.6: dragging a row shows a blue drop line; dropped between two features of an open folder it joins it, elsewhere it leaves (`MoveFeatures`). P3.9: dropped **onto a folder's name** it joins the folder at its end, open or closed. `course_ps3_folders` 06–07 (Extrude 11 dragged out: "Bosses (10)") and 08–09 (dragged onto "Bosses": back, "Bosses (11)"). Folder contents keep the history order (`normalize_folders`). |
| PS3.3 | Unpack folder; deleting a folder deletes its contents | ✅ | P3.6: Unpack folder. P3.9: the folder menu (Rename, Open / close, Unpack folder, **Delete folder and its features**, `DeleteFolder`, one undo step; core test `deleting_a_folder_deletes_its_features`). `course_ps3_folders` 10–13. |
| PS3.4 | Filter by name or type | ✅ | P3.9 (`cadrs_core::feature_list::Filter`): plain text matches the name or the type, ignoring case; "Extrude" lists every extrude; the folders holding matches open, Default geometry hides; Esc clears. `course_ps3_filter` 03–04, 08 (on the bracket stand-in, `samples::bracket`, whose 11 extrudes give "Extrude 10"/"Extrude 11"); unit tests `plain_text_matches_name_or_type`, `filter_on_the_bracket`. |
| PS3.5 | Quoted exact-name match | ✅ | P3.9: `"Extrude 1"` matches Extrude 1 only, unquoted it also matches Extrude 10 and 11 (`course_ps3_filter` 04–05; test `quotes_match_the_exact_name`). |
| PS3.6 | `:part`, `:type`, `:name`, `:errors`, `:folder`, `:variable` prefixes and hover help | ✅ | P3.9: all six prefixes (`:part` by the parts a feature made or changed, `:errors [name]`, `:folder`); hovering the field shows a help card listing them (`cadrs_ui` `Tooltip::help`; `course_ps3_filter` 02, 06 `:type Fillet` matching a fillet renamed "Rim round", 07 `:folder bosses`; test `prefixes`). P3F.4: `:variable <name>` lists the Variable and the features whose expressions use it (case-sensitive, with or without `#`; `course_pcad_variables` 09). |

## 2. Basic features

### PS4 Extrude
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS4.1 | Solid / Surface / Thin tabs | ✅ | P3.3: Solid, Surface and Thin all work (`BodyKind`); surfaces make "Surface N" parts. `course_ps4_surface_thin`. |
| PS4.2 | Input: sketch, regions, entities, planar faces | ✅ | P3.3: regions, whole sketches (from the list) and planar part faces (swept with their own history: sides named after the body's edges). Picks go through the extrude's own preview. Single sketch entities (not closed) only as part of a whole sketch. |
| PS4.3 | End types: Blind, Up to next/face/part/vertex (+ offset), Through all | ✅ | P3.3: Blind, Up to next, Up to face, Up to part, Up to vertex (each with Offset distance and its flip), Through all (`ExtrudeEnd`, see KERNEL.md "The full extrude"): parallel faces and vertices give a depth, oblique planar faces trim, Up to next/part/curved face conform to the target. Surfaces: depth ends only. `course_ps4_end_types`; conformance cases for each. |
| PS4.4 | Opposite-direction arrow; drag manipulator | ✅ | Flip button and arrow drag with snapping (`extrude.rs`); scenario `extrude_preview_drag`. P3.1: kept on the kernel extrude (each drag frame is a cached kernel rebuild); `extrude_preview_drag` renders the same as before. P3.4 (P3.3 judge): the arrow also shows on an Add preview (the added body's far cap), follows the Direction, shows for every end type (dragging an Up to end makes it Blind) and on surfaces (their far edges). |
| PS4.5 | Starting offset | ✅ | P3.3: the Starting offset option with its value and flip. `course_ps4_end_types` 09. |
| PS4.6 | Direction (along an edge, line or normal) | ✅ | P3.3: the Direction option: a straight part edge or a planar face's normal (an oblique prism; conformance `extrude_options`). `course_ps4_end_types` 10 (along a wedge's sloping edge: an oblique prism). P3.11 fix round 1 (stale text): the Direction field also takes a sketch line, a plane's normal and a mate connector's Z (`DirectionRef::{SketchLine, PlaneNormal, Connector}`, P3.8), as the loft's direction fields do (`course_ps20_loft_direction` 01–02 pick a sketch line). |
| PS4.7 | Symmetric (total depth) | ✅ | P3.3: Symmetric (total depth; also with Through all). `course_ps4_end_types` 05. |
| PS4.8 | Second end position | ✅ | P3.3: Second end position with its own end type, depth or target and offset. `course_ps4_end_types` 03. |
| PS4.9 | Draft | ✅ | Shown disabled. Needs draft in the kernel (not in the trait). P3.10: the **Draft** feature (toolbar Draft): Neutral plane \| Parting line (Parting line not built: choosing it says why), Neutral plane (a plane, a flat face or a mate connector; its normal is the pull), Faces to draft, Draft angle with the opposite-direction flip, Tangent propagation, Reference entity propagation (disabled); kernel `draft` (`BRepOffsetAPI_DraftAngle`, fork `f7d8eba`). And the extrude's **Draft** option (solids only), the angle inline on its row with its flip; each end drafted away from the sketch plane. `course_ps4_draft` 01–05 (02: Mass properties 835 228.361 mm³; 05, fix round 1: the drafted boss built Up to face with the 5 mm offset); tests `draft_feature_on_a_cube` (the frustum h(a² + ab + b²)/3 with b = 100 − 200 tan 5°, the bottom face as neutral plane, the flip, a lost face as a warning), `extrude_with_draft` (the frustum, flipped, symmetric, the Ø40 cone), conformance `draft_cube_sides`. |
| PS4.10 | Surface extrude of open profiles | ✅ | P3.3: surface extrudes of region loops and of a whole sketch's curves, open chains included (sheets). A surface is drawn two-sided (each side shaded by the way it faces, so an open tube's inside reads as its inside) with its free edges wider. `course_ps4_surface_thin` 03–06 (05: an open chain). P3.4: the preview of an open sheet is translucent on both sides (it was edges only from its back). |
| PS4.11 | Thin extrude (Thickness 1/2, Flip wall, Mid plane) | ✅ | P3.3: Thickness 1, Flip wall, Mid plane, Thickness 2, as exact 2D bands with mitred corners (`cadrs_kernel::thin`), so every end type works. Not along ellipses. `course_ps4_surface_thin` 01–02; tests `thin_extrude`, `surface_and_thin`. |
| PS4.12 | Default dialog layout (tabs, field, end type, depth, checkboxes, merge, slider, ✓✗) | ✅ | P3.3: the layout of `ex1-step3`/`ex1-step4` with every option live; the rows follow the choices (Up to fields, offsets, the second end, the Thin rows, Merge with all and a Merge scope field that lists the parts found by themselves). The footer slider is the before/after slider since P3.9 (PS13.2). P3.11 audit against `ex1-step3`/`ex1-step4`: Solid/Surface/Thin, New/Add/Remove/Intersect, the regions field, the end type with its flip, Depth with its measure button, Direction and Starting offset (with chevrons), Symmetric, Draft (P3.10), Second end position, Merge with all (Add) and the slider with ✓/✗ are all there, in that order (`course_ps6_control_arm` 04, 07); nothing is missing. The only difference is the faint ">" before Depth, which newer Onshape builds show for the depth's tolerance. |

### PS5 Boolean options
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS5.1 | New / Add / Remove / Intersect tabs | ✅ | P3.3: New, Add, Remove and Intersect all work (for solids and thin walls). |
| PS5.2 | First solid must be New; Add is picked automatically on overlap | ✅ | P3.3: a new extrude is New until its body touches a part, then Add by itself (`Build::contacts`), until a tab is clicked. The Add preview shows the parts as they were, opaque, with the added body translucent (`Build::stage`, `ex1-step4`); an accepted Add merges faces that meet on one surface (no seam: `course_ps4_end_types` 08). `course_ps6_control_arm` 07, `course_ps5_boolean` 01. |
| PS5.3 | Remove cuts, Add merges parts, Intersect keeps the overlap | ✅ | P3.3: Remove cuts (a part cut in two gives a new part for the smaller piece), Add joins several parts where the body bridges them, Intersect keeps the overlap. Tests `remove_intersect_add_two_boxes` (190 000 / 50 000 / 340 000 from the closed form), `a_remove_that_splits_a_part`, `merge_scope`. |
| PS5.4 | Merge scope and Merge with all | ✅ | P3.3: Merge with all, and a Merge scope field (empty: the parts touched or overlapped, listed; picking parts sets it). |
| PS5.5 | Boolean feature: Union/Subtract/Intersect, Tools, Targets, offset, Keep tools, reorderable tools | ✅ | P3.3: the Boolean feature (toolbar): Union, Subtract (Tools and Targets), Intersect, Keep tools; the result takes the first tool's identity. P3.4: the tools and targets show selected (orange) in the view; Subtract's Offset option is shown but disabled (the kernel has no offset yet). No reordering of tools by drag. `course_ps5_boolean` 05, 05b, 06; test `boolean_feature`. P3.10: Subtract's **Offset** (Offset all, or Faces to offset picked on the tools; the Distance with its flip; kernel `offset`, sharp edges) and the **Tools reordered** by drag (↑↓, the first tool gives a Union its identity). Fix round 1: while it subtracts, the tools are ghosted (faint, in their own colour) over the orange targets and the Faces to offset are blue, so the offset pocket reads (01, 03, 04 from the top: the 1 mm ring all round, on +X only, and the block's edge under the post when flipped; 02 the blue face close up); Offset's rows nest under it. `course_ps5_boolean_options` 01–06; tests `boolean_offset_and_tool_order` (22·22·21 removed with a 1 mm offset, one face 20·20·21, inward 18·18·19; the union is the first tool), conformance `offset_solids`. |

### PS6 Exercise: Control Arm
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS6.1 | New doc, mm units, rename doc and tab, delete Assembly tab | ✅ | Units dialog (`units_dialog.rs`), inline renames, tab delete. |
| PS6.2 | The sketch: tangent webs, symmetric eyes, fully defined | ✅ | P3.3: drawn through the tools in `course_ps6_control_arm` 03: circles (only the left eye dimensioned; Symmetric about the centreline sizes the right eye and hole, as in ex1-step2), centre rectangle, construction centreline, four webs, Tangent, Horizontal and the 250 overall dimension; fully defined (black). Planes hidden (P). |
| PS6.3 | Extrude 1: New, Blind 40, 3 regions | ✅ | P3.3: done through the dialog in `course_ps6_control_arm` 04 (three "Face of Sketch 1" rows, New, Blind 40). |
| PS6.4 | Show Sketch 1, Extrude 2 **Add** Blind 25, 2 regions | ✅ | P3.3: the eye shows Sketch 1; Extrude 2 picks Add by itself (its body touches Part 1), Blind 25: one part. `course_ps6_control_arm` 06–08. |
| PS6.5 | Parts list → Rename "Control Arm" | ✅ | P3.3: Parts list → right-click → Rename → "Control Arm". `course_ps6_control_arm` 09–10. |
| PS6.6 | Mass and section properties | ✅ | P3.3: Mass and section properties: "Control Arm" in red (no material), Volume 368749.705 mm³, Surface area 50179.711 mm² (the self-check, 50 179.71), X/Y/Z centre of mass rows left blank with no material, as in ex1-step6. `course_ps6_control_arm` 11; test `control_arm_self_check` asserts both to 0.01. |

### PS7 Revolve
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS7.1 | Input: sketch, regions, entities, face, curve | ✅ | P3.4: the Revolve feature (toolbar, Shift+W): sketch regions, whole sketches ("Face of Sketch 1", or "Faces of …" for several regions; "Sketch curves to revolve" for surfaces) and planar part faces (solid revolves; `RevolveSpec.faces`, their sides named after the body's edges). Conformance `revolve_a_face`; test `revolve_about_part_geometry_and_of_a_face`. |
| PS7.2 | Revolve axis: line, edge, cylindrical face, circular edge, arc, mate connector | ✅ | P3.4: the "Revolve axis" field takes a sketch line (a construction centreline) or circle/arc (its axis), a straight part edge, a circular edge (its axis, the kernel's exact circle) or a cylindrical/conical face (the kernel's axis, fork `face_axes`); sketch curves of shown sketches are pickable while the field is active. The mate connector button is shown disabled (mate connectors are P3.8). `course_ps7_revolve_types` 02 (a centreline), 15 (a cylindrical face), 16 (a circular edge), 17 (a straight part edge); test `revolve_about_part_geometry_and_of_a_face` (1750π about each). P3.10: the mate connector button works: on, the next pick is a mate connector (an explicit one, or the implicit one of a face, edge, vertex or the origin), its Z the axis; an explicit connector can be picked straight into the field. Fix round 1: while the button is on, the connector under the pointer shows its whole glyph (disc and triad, enlarged); once picked, the connector stays drawn and the axis runs along its Z through the view. `course_ps7_revolve_connector` 01–03; test `revolve_about_a_mate_connector` (9000π about the origin's connector). |
| PS7.3 | Full / Blind / Symmetric / Up to …, second end | ✅ | P3.4: Full, Blind (with its flip and a drag arrow on the end face), Symmetric, Up to next / face / part / vertex with an Offset angle and its flip, Second end position (its own type, angle or target and offset) (`RevolveSpec`, KERNEL.md "The full revolve"). Tests: conformance `revolve_types`, `revolve_up_to`; `cadrs_core/tests/revolve.rs`. `course_ps7_revolve_types` 03–07, 10 (Full, Blind with the arrow hovered and dragged, Second end, Symmetric) and 17 (Up to face with an Offset angle). |
| PS7.4 | Thin revolve | ✅ | P3.4: Surface and Thin tabs (Thickness 1, Flip wall, Mid plane, Thickness 2), exact bands turned as a solid; surfaces of loops and open chains. Conformance `revolve_surface_and_thin`; test `revolve_remove_surface_thin_up_to`; `course_ps7_revolve_types` 08 (Thin), 09 (Surface). |
| PS7.5 | Boolean tabs and Merge with all | ✅ | P3.4: New/Add/Remove/Intersect with Merge with all and Merge scope, and the automatic Add, shared with the extrude (`combine`). `course_ps8_reducer_coupling` 06 (Add picked by itself); a Remove groove in `revolve_remove_surface_thin_up_to`. |

### PS8 Exercise: Reducer Coupling
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS8.1 | New doc, inch units, rename | ✅ | P3.4: `course_ps8_reducer_coupling` 01–02 (Workspace units Inch, renames, Assembly tab deleted). |
| PS8.2 | Flange sketch on Right (bolt circle, 4 equal holes) | ✅ | P3.4: drawn through the tools, fully defined (Ø6, Ø2, Ø4.75 construction, 4 × Ø0.625 Equal, Horizontal/Vertical with the origin). `course_ps8_reducer_coupling` 03 (ex2-step2). |
| PS8.3 | Extrude New 0.62 in, whole sketch excludes holes | ✅ | P3.4: the whole sketch picked in the list; the field reads "Face of Sketch 1" as in ex2-step3 ("Faces of …" when the sketch has several regions); Blind 0.62 in, +X. 04 (ex2-step3). |
| PS8.4 | Sketch on Front: Use bore edge, parallelogram, **diametral** dimensions to a centerline | ✅ | P3.4: Use of the bore's edge on the flange face (a vertical line), the construction centreline, the profile with Parallel sides, and the **diametral** dimensions Ø2.625 and Ø3.75 (`DimensionKind::Diametral`: point, centreline, label placed across it; drawn from the point to its mirror image, "Ø"), 4.625 axial; fully defined. 05 (ex2-step4). |
| PS8.5 | Revolve Add, Full, about the centreline | ✅ | P3.4: Revolve 1, Add picked by itself, Full, "Edge of Sketch 2" (the centreline), Merge with all. 06 (ex2-step5). |
| PS8.6 | Sketch on the revolve's end face; Use its circular edge | ✅ | P3.4: the sketch on the revolve's end face ("Face of Revolve 1", a planar side of the revolve, centred on the axis); Use of its inner edge gives the kernel's exact Ø3.125 circle (test `reducer_coupling`); Ø7.5, Ø6 construction, 4 × Ø0.75. Also: Use of a cone's silhouettes (its generators). 07 (ex2-step6). |
| PS8.7 | Extrude Add 0.75 in | ✅ | P3.4: the whole of Sketch 3, Add, Merge with all, the drag arrow on the added flange. 08 (ex2-step7). |
| PS8.8 | Rename part | ✅ | P3.4: 09–10 (ex2-step8). |
| PS8.9 | Optional: one Revolve + hole extrudes (Remove) | ✅ | P3.4: test `reducer_coupling` (second half): one revolve of the whole outline, two hole extrudes (Remove, the second with a starting offset of 5.245 in): the same volume and area to 1e−3. No scenario. |
| PS8.10 | Mass properties | ✅ | P3.4: **Volume 53.932 in³**, **Surface area 248.367 in²** in the panel (11, ex2-step10; inertia heading in in² lb); test `reducer_coupling` asserts both to 1e−3 against the analytic values (V = 14.8214 + 13.0542 + 26.0562 = 53.9318; A = 248.3665 summed face by face in the test comment). The course's numbers agree with the drawing. |

## 3. Appearance and material

### PS9 Appearances
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS9.1 | 8-colour cycling palette, stable on delete | ✅ | P3.5: `cadrs_core::appearance::PALETTE` (the first is the light blue parts always had); a part keeps the colour of the number it was made with (`Part::palette`, parts and surfaces share the count), so a Delete part leaves the others alone (test `material::palette_colours_are_stable`). `course_ps9_appearance` 01, 03, 14. |
| PS9.2 | Edit appearance dialog: swatches, mixer, custom colours, hex/RGB | ✅ | P3.5: `appearance.rs`: 24 preset swatches (the palette first), Custom colors (**+** saves the current colour in the document; right-click → Update color / Delete, 11–13), the Mixer (`cadrs_ui::ColorMixer`: saturation/value square and hue strip, UI gradients), Hex and R G B fields (Enter lets go of the focus); the view shows the colour live, ✓ applies it as one undo step, Default takes it off. 04, 05, 10. |
| PS9.3 | Transparency slider | ✅ | P3.5: Opacity slider (0–100 %); a translucent face or part is drawn blended, its hidden edges showing through. 06–07. |
| PS9.4 | Face and feature appearances override the part | ✅ | P3.5: right-click a face → Add appearance to face…; right-click a part feature → Add appearance to feature… (the faces it made); a face's own wins over its feature's, which wins over the part's. 08–09, 13. |
| PS9.5 | Sketch and curve appearance | ✅ | P3.5: Edit sketch appearance… (feature row or the sketch in the view) colours an accepted sketch's curves (16); right-click one curve in the view → Edit curve appearance… colours only it, over its sketch's (`SetCurveAppearance`; 17–18, undone in 22). |
| PS9.6 | Pattern instances inherit appearance | ✅ | P3.8: a pattern's or mirror's New copies are parts with `source` = the seed part: they take its palette colour, and show its appearance and material unless they get their own (`parts::part_material`, `appearance::part_appearance`); a copied face (`FaceOrigin::Instance`) shows its source face's appearance. Test `linear_part_pattern_of_a_cube`; `course_ps22_skip_instances` 01, 04. |
| PS9.7 | Appearance side panel | ✅ | P3.5: the palette icon on the right opens the Appearances panel: parts (and their faces with their own), features with an appearance, sketches, each with its swatch; single curves under their sketch; double-click or right-click → Edit appearance… opens the dialog. 20–21. |
| PS9.8 | Per-configuration appearance | — | **Out of scope**: configurations (niche; out of scope by user decision 2026-09-29). |

### PS10 Materials
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS10.1 | Assign material (Library / Custom tabs) | ✅ | P3.5: `material_dialog.rs`, from the Parts list's or a face's menu, for the selected parts; one undo step (`SetPartMaterial`); Remove material. `course_ps10_material` 02–07, 12. |
| PS10.2 | Library dropdown, searchable materials, property list | ✅ | P3.5: `cadrs_core::material::LIBRARY`, 18 materials with published values and their sources in the code (ASM/MatWeb data sheets, EN 1993-1-1 for Steel; Polypropylene as the course's `ex4-step17` lists it); a closed material dropdown (as `ex4-step17`) whose popup has a search box that filters as you type and the list; the 8 properties in the workspace units (kg/m³ and MPa; lb/in³ and Psi in inches, matching `ex4-step17` exactly, test `polypropylene_reads_like_the_course`). 03–04, 11. The library is called "Standard materials". |
| PS10.3 | Custom material; custom libraries | ✅ | P3.5: the Custom tab (name, density and the other properties, stored with the part). 06–07. P3.6: the **+** beside the library makes "Library N" (`Document.material_libraries`, saved, undoable `SetMaterialLibraries`); with it chosen the Custom tab offers **Add to Library N**, and the Library tab lists its materials (the bordered library select). `course_ps10_material` 13–15. |
| PS10.4 | Mass, centre of mass, inertia from the material | ✅ | P3.5: `MassProperties.inertia` (fork `GProp_GProps::MatrixOfInertia`), `parts::mass_report` (mass ΣρᵢVᵢ, mass-weighted centre, inertia about it). Tests: the 100 mm steel cube 7.85 kg, Ixx 13 083.333 kg·mm²; the Control Arm in Polypropylene V × ρ = 0.331 875 kg. 05, 07, 10. |

## 4. Part design concepts

### PS11 Dependencies
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS11.1 | Children fail when a parent changes | ✅ | `regenerate` runs in order. A sketch whose face is gone shows red with "Missing face" (`sketch_face_lost`), and broken Use links mark the sketch. P3.1: an extrude fails when its regions or sketch are gone, or when the kernel refuses it: its row turns red with the reason as tooltip, and the header shows the error icon (`cadrs_core::rebuild::Build::errors`, `course_ps_kernel_extrude` 04–05). Missing: warnings (Onshape's yellow state), and dependents of other feature types (none exist yet). P3.10: Onshape's yellow **warnings**: a feature that builds with something missing (an extrude, revolve or sweep with a lost region, a Delete part with a lost part, a hole whose points partly miss the merge scope, a draft with a lost face) shows its row amber with a warning glyph and the reason on hover (`Build::warnings`). `course_ps15_hole_options` 07; tests `warnings_for_what_didnt_take`, `draft_feature_on_a_cube`. |
| PS11.2 | Show dependencies | ✅ | P3.9: the feature menu's **Show dependencies** highlights the feature, its parents above (amber) and its children below (green), with a legend and ✕ under the header; Esc ends it (`cadrs_core::feature_list::{parents, children}`: `Feature::parents` plus a sketch's face or Plane feature). P3.11: with what the rebuild knows (`feature_list::{parents_with, children_with}`, used by the app): a feature that changed a part it doesn't name (an Add or Remove whose merge scope it found by itself, a hole's default scope) has the part's feature as a parent (`Part::features`), so the ten bosses are Extrude 1's children; an edge reference depends on both its faces' features, and a fillet or chamfer on the features of every face along its tangent chain (`Build::uses`), so Fillet 2 (picked on the plate's rim, its chain running round Fillet 1's corners) has Fillet 1 as a parent. A closed folder holding parents or children is tinted too, each dependency row has a stripe in its legend swatch's colour, and the swatches are the saturated hues (P3.9 judge). `course_ps11_dependencies` 02 (Extrude 1: Sketch 1; the Bosses folder, Fillet 1, Fillet 2), 03 (Sketch 2: the ten bosses), 04 (Fillet 2: Extrude 1, Fillet 1); tests `implicit_parents_of_the_bracket`, `dependencies_of_the_gear_cover` (Extrude 2's cut: Extrude 1), `sketch_on_a_face_depends_on_its_feature`. |
| PS11.3 | Drag to reorder; a child above its parent fails | ✅ | P3.6: feature rows drag with a drop line (`feature_folders.rs`, `MoveFeatures`, one undo step). A feature whose parent (`Feature::parents`: its sketches, the features that made its faces, edges and parts) is below it fails: "Sketch 4 is below this feature in the list; move it back above", red, the parts as before it (`course_ps17_gear_cover` 07 (Shell 1 to the bottom), 07b (Sketch 4 below the hole); test `a_feature_dragged_above_its_parent_fails`). |
| PS11.4 | Order changes the result (fillet vs hole, shell) | ✅ | P3.6: a face fillet after a hole rounds the hole's rim too, before it doesn't: the difference is the rim fillet's closed form 2π·5.2234·(1 − π/4) (`applied.rs::order_changes_the_result`); the Gear Cover's shell before and after its holes (`course_ps17_gear_cover` 06–07, `gear_cover.rs`). |

### PS12 Reference planes
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS12.1 | Top, Front, Right at the origin | ✅ | |
| PS12.2 | Plane feature: Offset, Plane point, Line angle, Point normal, Three point, Mid plane, Curve point, Tangent | ✅ | P3.7: the **Plane** feature with all the types, each in the dialog's type list: Offset (distance with its flip and a **draggable arrow** on the plane), Plane point, Line angle (angle, flip, Flip alignment), Point normal, Three point, Mid plane (two planes at an angle: **Flip alignment** takes the other bisector), Curve point, **Tangent** (to a cylindrical or conical face at an angle round its axis, or through a point: outside a cylinder the two tangents, Flip alignment the other) and Fit; Flip normal. Frames checked against hand-built ones (`cadrs_core::plane` `eight_plane_types`, `tangent_planes`). Entities: default planes, other planes, faces, edges, vertices, sketch points and curves, the origin. Shown as a translucent square with its name (hidden where a part is in front of it), hover and selection outlines. Scenario `course_ps12_planes` 01–15 shows every type but Fit in the UI. |
| PS12.3 | Planes as sketch planes, directions, mirror planes | ✅ | P3.7: Plane features are sketch planes (picked in the list or the view) and extrude **directions** (their normal, `DirectionRef::PlaneNormal`), split tools and a sweep's locked direction (`course_ps12_planes` 07–08). P3.8: **mirror planes** (`MirrorPlane::Plane`, default planes and Plane features alike; also planar faces and mate connectors). |

### PS13 Previewing feature generation
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS13.1 | Editing an earlier feature rolls the studio back to it | ✅ | P3.7 left the later features out of the view; P3.9 also moves the rollback bar under the edited feature and greys the rows below (`course_ps13_rollback_final` 02). The **rollback bar** is draggable (a grey ghost shows where it goes; `SetRollback`, saved in the Part Studio, one undo step; 06–07), and the menu has Roll history bar to here / Roll to end (08–09). Below the bar features are greyed and not built (`Element::active_features`), a new feature goes in at the bar, and a rolled-back feature can't be edited. Core test `rollback_bar_leaves_out_the_features_below_it`. Editing a *sketch* doesn't roll back (its later features stay in view, as before P3.9). |
| PS13.2 | Before/after slider in every dialog | ✅ | P3.9: the footer slider of every feature dialog (Extrude, Revolve, Boolean, and the applied, P3.7 and P3.8 dialogs) replaces "Preview detail": its left half shows the Part Studio *before* the feature (the feature and everything after it left out, the bar above it), its right half *after* (`PartOverride::before`; `course_ps13_rollback_final` 04–05). A rebuilt dialog keeps the slider's side. |
| PS13.3 | Final button | ✅ | P3.7 (PS21.11): Final shows the features after the edited one (`course_ps21_funnel` 11); P3.9 adds it to Boolean and shows it only on a feature that isn't the last one built (hidden on the last feature, as the course says). `course_ps13_rollback_final` 03: editing Extrude 1 of the Control Arm, Final shows Extrude 2's downstream web and eye. |

## 5. Applied features

### PS14 Fillet and chamfer
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS14.1 | Edge / Full round tabs | ✅ | P3.6: the Fillet dialog (`applied_dialog.rs`) with **Edge** and **Full round** tabs. Full round (judge round 1): *Side face 1*, *Center face*, *Side face 2* (a pick fills a field and moves on), `Kernel::full_round`: the centre face becomes a half-cylinder tangent to the sides, r = half their distance (kernel `full_round`: 6000 − (2 − π/2)·25·20; core `full_round_and_conic_sections`; `course_ps14_fillet` 09). Built for a flat rectangular centre face between two parallel flat side faces at least r deep; other faces are refused with that reason (OCCT has no full-round fillet). Other full rounds: **out of scope** (niche; out of scope by user decision 2026-09-29). |
| PS14.2 | Edges or faces; tangent propagation | ✅ | P3.6: *Entities to fillet* takes edges ("Edge of Extrude 2": the feature that made the edge) and faces (all their edges); tangent propagation (on by default) lists and draws the tangent chains in amber. Off: only the picked edge is listed and drawn, and a pick whose chain has unpicked tangent edges fails with the reason ("…would stop part-way along a smooth edge chain (4 tangent edges not picked), which OpenCascade cannot do; turn on Tangent propagation or pick the whole chain"), since OCCT's fillets always run on along tangent edges (judge round 1: it used to be ignored). Test `tangent_propagation_off_is_refused_on_a_chain`; `course_ps14_fillet` 01–02. |
| PS14.3 | Radius / Width | ✅ | P3.6: *Measurement* Radius or **Width**: the chord stays constant, a radius per sample along each edge `w / (2 sin(φ/2))` from the faces' angle (fork `edge_normals`, `try_fillet_variable_h`); kernel `fillet_width` (90°, 45°, a chain from 76° to 90°). `course_ps14_fillet` 03, `course_ps17_gear_cover` 10–11. |
| PS14.4 | Distance / Conic (Rho) / Curvature | ✅ | P3.6 (judge round 1): *Control* Distance, **Conic** with *Rho* and **Curvature** with *Magnitude*. OCCT's fillet only rolls a circle (`ChFi3d_FilletShape` picks the surface approximation, `Law_Function` only varies the radius), so the conic (a rational Bezier, exact circle at the circle's rho) and the G2 section (a quintic Bezier with zero end curvature) are built by the kernel as swept sections, fork `Edge::try_bezier`: on straight edges between flat faces, convex or concave (kernel `fillet_sections`: rho 0.5 removes 1/3 of the contact triangle; `course_ps14_fillet` 07–08). Curved edges or faces are refused with the reason; corners where section fillets meet aren't blended. The rest (conic and curvature on curved edges, blended corners): **out of scope** (niche; out of scope by user decision 2026-09-29). |
| PS14.5 | Drag arrow for the radius | ✅ | P3.6: an arrow on the first picked edge, out of its corner; dragging it sets the radius or width live (snapped), one undo step on release. `course_ps14_fillet` 04. |
| PS14.6 | Asymmetric, partial, variable, overflow, smooth corners | ✅ | Later. P3.10: **Asymmetric** (Second radius with its flip; the conic with the circle's weight, an ellipse at 90°), **Variable fillet** (Vertices with a radius each, Points on edge with a location and radius each, Smooth transition; kernel `fillet_variable`), **Allow edge overflow** (P3.6). P3.11 fix round 1: **Partial fillet** built: nested under its checkbox, Boundary type (Parameter \| Length), First bound with its opposite-direction flip (measured from the edge's other end) and Second bound, on one edge; the round runs only between the bounds and stops at a flat end face square to the edge at each (kernel `fillet_partial`: the whole edge's fillet's removed or added material cut down to the slab between the bound planes, see crates/cadrs_kernel/README.md "Partial fillet"). `course_ps14_fillet_options` 06 (R10, 0.25–0.75), 06b (Length 0–12 mm, flipped: from the top end); tests `partial_fillet` (core: (1 − π/4)·9·15 for 0.2–0.7 of the 30 mm edge, 21 mm by Length, flipped the same volume with the centroid 3 mm along, errors off the edge and for two edges), conformance `fillet_partial` ((1 − π/4)·4·15 exactly, the end faces' areas (1 − π/4)·4 at 7.5 and 22.5, 0–1 equals the whole fillet, half a rim's round, a concave edge). **Smooth fillet corners** (Final): where three or more filleted edges meet, the corner is set back 1.5 × R along the fillets and blended with one N-sided patch tangent (G1) to every face round it: the filleted solid cut back by a ball about the vertex, the cut face replaced by a `BRepOffsetAPI_MakeFilling` patch and sewn back (fork `Shape::try_fill_face`, commit `f65c3e1`; kernel `fillet_smooth`, crates/cadrs_kernel/README.md "Smooth fillet corners"); circular radius fillets only (else an error saying so); it turns Asymmetric, Partial and Variable off; the row's tooltip says what it does (06c). `course_ps14_fillet_options` 08 (the default sphere corner, R6 on three edges of a block corner), 09 (Smooth fillet corners on), 10 (accepted); tests: conformance `fillet_smooth_corner` (30 mm cube corner, R3, setback 4.5: a valid solid; V_F − \|F ∩ B\| ≤ V ≤ V_F − \|F ∩ B\| + πρ³/6 (outside the ball nothing changes, and what the patch encloses lies in the ball's octant of the sharp cube), in fact 0.77 mm³ less than the default; one patch generated from the vertex; its six neighbours' normals agree with it to under 1° at 32 points on each shared edge, worst 0.37°), core `smooth_fillet_corners` (the 10 × 20 × 30 block's corner: a little less volume than the default corner, less than πρ³/6 less; 10 faces; refused for a Width fillet). G2 isn't built: MakeFilling refuses G2 against these supports (KERNEL.md). The variable fillet's **Magnitude** belongs to Onshape's Curvature cross section: **out of scope** (niche; out of scope by user decision 2026-09-29), with that reason on its disabled rows (05). P3.10 fix round 1: laid out as the course's (`lesson-fillet-and-chamfer.png`): nested under the checkbox, a Vertices group with an expandable entry per vertex ("[2 mm] Vertex of Extrude 1", ✕; Radius and Magnitude under it) and a Points on edges group with CLEAR, an entry per point (Edge, Location, Radius, Magnitude) and **Add point on edge**; Magnitude is shown disabled with why; `cadrs_ui::entry_list` (EntryGroup, Entry). `course_ps14_fillet_options` 01–07 (03–05 the entries, 05 Magnitude's why); tests `variable_and_asymmetric_fillets` (R3 exactly; 2 → 8 linear, bounded and within 1 % of (1 − π/4)·280 for 2 → 4; asymmetric (1 − π/4)·2·4·30), conformance `fillet_variable_radius`, `fillet_asymmetric`. Full round, conic and curvature beyond P3.6: out of scope (niche; out of scope by user decision 2026-09-29). Magnitude: out of scope as part of the Curvature cross section (niche; out of scope by user decision 2026-09-29). |
| PS14.7 | Chamfer: Offset / Tangent measurement | ✅ | P3.6: *Measurement* Offset or Tangent. Tangent measures along the faces' tangent planes: the same as Offset on a face straight across the edge (a plane, a cylinder's rim), the chord `2R sin(atan(d/R)/2)` on a face that curves across it (a sphere, a cylinder along a straight edge), on that side only (judge round 1: the chord was applied to any cylinder). Kernel `chamfer_options` (rim: Tangent = Offset; a D-shaped prism's straight edge: the two-distance chamfer); `course_ps14_chamfer` 06–07. |
| PS14.8 | Equal / Two distances / Distance and angle; flips and direction overrides | ✅ | P3.6: the three types; the opposite-direction flip (all edges) and *Direction overrides* (per edge, picked on the edges as they were before the chamfer). 2 mm × 45° removes 2·2/2 per mm (`chamfer_two_by_45_removes_two_per_mm`). `course_ps14_chamfer` 01–05, `course_ps17_gear_cover` 08. |
| PS14.9 | Chamfer tangent propagation | ✅ | P3.6: on by default; each edge's tangent chain is chamfered (`Kernel::tangent_chain`). |

### PS15 Hole
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS15.1 | Holes at sketch points, vertices, circle centres; whole sketch | ✅ | P3.6: sketch points of visible sketches are picked in the view ("Vertex of Sketch 4", drawn as amber dots); a whole sketch picked in the list takes every non-construction vertex (standalone points, line and arc ends, circle and arc centres: `hole::hole_vertices`). Test `hole_whole_sketch_merge_scope_and_parent_below` (three points, two blocks: 3·π·5²·30); `course_ps15_hole` 01, 08 (Sketch 4 picked in the list), `course_ps17_gear_cover` 05. |
| PS15.2 | Mate connectors as locations | ✅ | P3.8: the Hole dialog's mate connector button next to "Sketch points to place holes" takes Mate connector features and implicit connectors (a face's centroid, a circular edge's centre, a vertex, a sketch point); the hole is drilled along the connector's −Z. Test `hole_at_a_mate_connector` (a 46 × 12 blind hole's closed form); `course_x11_mate_connectors` 04–05, `course_ps27_reflector` 06. |
| PS15.3 | Merge scope | ✅ | P3.6: *Merge scope* field (parts; empty: every solid part a hole reaches). A point over a part left out of the scope gets no hole (not an error; none reaching a part is one). Test `hole_whole_sketch_merge_scope_and_parent_below` (scope = the first block: 2·π·5²·30, also from part); `course_ps15_hole` 09. |
| PS15.4 | Dialog: Inch/Metric, Simple/Counterbore/Countersink, … | ✅ | P3.6: Inch/Metric and Simple/Counterbore/Countersink tabs, points, merge scope, Hole type, Size, Fastener fit or Pitch, the diameter, Start plane, Termination with its flip (its icon shows the state), depth, tip angle (118°), counterbore Ø and depth, countersink Ø and angle, tapped depth; on the Inch tab the depths are in inches (`course_ps15_hole` 06: 0.394 in). The title is the callout, ending in "…" when too long (so are feature rows, keeping the ⓘ in view). Final since P3.7 (shown on a hole before the end, P3.9). P3.11 fix round 1 (stale text): the tapped hole's **Thread class** checkbox is there since P3.10 (with its class in the callout, PS15.5). `course_ps15_hole`; `course_ps15_hole_options` 01. |
| PS15.5 | Drilled / Clearance / Tapped / PEM with standards tables | ✅ | P3.6: `cadrs_core::hole` tables with their sources: ISO 273 clearance (M1.6–M48: M5 Close 5.3, M45 Close 46), tap drills d − P (M10×1.5 8.5, fine pitches), ISO 4762 counterbores (dk + 1.25 × k: M5 9.75 × 5, the course's), 90° countersinks 2.24·d; ANSI #0–#12 and 1/4–1 clearance (ASME B18.2.8), UNC/UNF tap drills, counterbores and 82° countersinks; metric and number/letter/fractional drill sizes. PEM not scheduled. Tests `hole::tests`. P3.10: **PEM®** (metric; PennEngineering's published mounting holes: CLS/S nuts M2.5–M8, M4 Ø5.41; FH studs, the nominal Ø; +0.08/−0 mm) with its Fastener row; the tapped hole's **Tap type** (Straight tap; Tapered listed disabled), **Pitch** as "1.50 mm (Coarse)", **Fastener fit** None, **Thread class** (6H/5H/7H, 2B/1B/3B) in the callout and the **Tap clearance** row in threads ((20 − 10.02)/1.5 = 6.653, PS27.8); the name uses the hole depth ("M10x1.50 ↧ 20 mm", P3.8 judge); the Merge scope defaults to the part the first hole drills. `course_ps15_hole_options` 01, 02, 06; `course_ps15_hole` 08–09; tests `hole::tests` (`course_sizes`, `tolerances_and_pem`), `tapped_and_clearance_tables`. |
| PS15.6 | Start from part / sketch plane / selected plane | ✅ | P3.6: Start from part (the first crossing ahead of the sketch plane, `ray_hits`) and from sketch plane (test `hole_start_plane_and_up_to_next`: the counterbore in the air above the part). Start from selected plane not done. P3.10: **Start from selected plane** with its *Hole start plane* (a plane, flat face or mate connector); started inside a part the hole is buried (nothing cut above the plane). `course_ps15_hole_options` 04 (the counterbore ends 2 mm into the plate; fix round 1: the long "Start from selected plane" cut with "…" before the caret); test `hole_start_from_selected_plane_and_up_to_entity` (π·25·5 + the point, the top face whole; from the sketch plane it opens it). |
| PS15.7 | Through all / Blind / Up to next / Up to entity; 118° tip | ✅ | P3.6: Through all, Blind (full diameter to the depth, the 118° drill point beyond) and Up to next, with the flip; the counterbore's closed form π·5²·4 + π·2.5²·16 + π·2.5²·h/3 (`counterbore_hole_closed_form`). Up to entity not done. P3.10: **Up to entity** (a plane, flat face or connector; a curved face by its first crossing), with an **Offset** for Up to next and Up to entity. Fix round 1: Tip angle is a dropdown ("118 deg ▾": 118, 90, 120, 135, 140, `ex5-step8.png`) and the Offset has its flip (past the target instead of short of it). `course_ps15_hole_options` 05; test `hole_start_from_selected_plane_and_up_to_entity` (to B's top: π·25·10 + the point; offset 2: 8 deep; to Top: π·25·30; no target: an error). |
| PS15.8 | Tolerances for the callout | ✅ |  P3.10: collapsible **Diameter tolerance** and **Depth tolerance** (Symmetrical, Deviation, Limits, Min, Max, Basic; upper and lower deviations; precision) written into the callout and the name ("Ø 5 mm +0.10/−0.05 ↧ 10 mm MAX"). `course_ps15_hole_options` 03 (fix round 1: the deviations in the standard's unit, "0.1 mm", and the section's rows nested under its checkbox); test `tolerances_and_pem`. P3.11: the **counterbore Ø and depth** and **countersink Ø and angle** tolerances (`HoleSpec::style_tol`, `StyleTolerance`), each a section under its size's row, in the callout ("⌴Ø 9.75 mm +0.20/−0.00 ↧ 12 mm ±0.10", "⌵Ø 11.2 mm X 90° ±1.0"; an angle's limits "91.0°/89.0°"); the sections' rows indented to their checkbox's label, Thread class with its ">" (P3.10 judge). `course_ps15_hole_options` 04a, 04b; test `tolerances_and_pem`. |
| PS15.9 | Callouts in drawings | ✅ | **P3C.3**: the drawings' hole callouts read the Hole feature's stored spec (P3.6) through the picked edge's faces, which name the Hole feature: `4x Ø.266 THRU ⌴Ø.438 ↧.250` (D6.7 and D8.8 ✅ in `intro-to-drawings-gaps.md`; `course_drw_ex1_ujoint` 12b–14). (Before: → P3C.3.) |
| PS15.10 | Live callout text in the title and feature row | ✅ | P3.6: `HoleSpec::callout`: "Ø 5.3 mm THRU \| ⌴Ø 9.75 mm ↧ 5 mm", "M10x1.50 ↧ 20 mm", in inches for the Inch tab; the dialog title (cut off at the ✓) and the feature row (until renamed). ⌴ ⌵ ↧ are drawn from cadrs's own small symbols font (`tools/make_symbols_font.py`; Inter has no such glyphs). `course_ps15_hole`, `course_ps17_gear_cover` 05. |

### PS16 Shell
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS16.1 | Faces to remove, thickness | ✅ | P3.6: the Shell dialog (`ex3-step3`): Faces to remove, Shell thickness. 100 × 60 × 40 bottom open 4 mm: 100·60·40 − 92·52·36 (`shell_open_box_and_failure`, kernel `shell_options`). `course_ps16_shell_fail` 01, `course_ps17_gear_cover` 03. |
| PS16.2 | Inward by default; flip to outward | ✅ | P3.6: the opposite-direction arrow grows the walls outside (OCCT's arc join rounds the outer edges). `course_ps16_shell_fail` 02. |
| PS16.3 | Hollow (closed) | ✅ | P3.6: Hollow takes parts: a closed box with a void (240 000 − 92·52·32). `course_ps16_shell_fail` 03. |
| PS16.4 | Fails on self-intersection; order matters | ✅ | P3.6: walls that would cross are refused with the reason (red title, the reason in the dialog; after an upstream edit the row is red with its ⓘ): `course_ps16_shell_fail` 04, 06–07. Order: the Gear Cover's Shell dragged below its holes wraps them (07). |

### PS17 Exercise: Jackhammer Gear Cover
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS17.1 | Copy the doc; mm and **kg** units | ✅ | P3.6: the stand-in document (`fixtures/gear_cover_standin.cadrs`, script `gear-cover`) in mm and kg. `course_ps17_gear_cover` 01–02. |
| PS17.2 | Image tab, Base Features folder | ✅ | P3.6: the six base features in a closed "Base Features (6)" folder. The image tab (`ex3-step1`'s "Jackhammer.png"): **out of scope** (niche; out of scope by user decision 2026-09-29). |
| PS17.3 | Shell, bottom face, 4 mm | ✅ | P3.6: `course_ps17_gear_cover` 03. |
| PS17.4 | 6 points, symmetric, concentric with part edges | ✅ | P3.6 (judge round 1: all in the sketcher): Sketch 4 on the bosses' top, Use of the two bosses' edges, a vertical construction centreline from the origin, six points placed roughly; the "A" pair **Concentric** with the used edges (Shift+O), the other pairs **Symmetric** about the centreline (Shift+Q), dimensions **4** (A to the middle pair) and **16** (the back pair from the centreline) as the course's, and 60 and 24 vertical (the course's 95 and 93 don't fit the 200 mm stand-in). A sketch holding Used geometry no longer rescales on its first dimension. 04. |
| PS17.5 | Hole: M5 Close counterbore Ø9.75×5, through all | ✅ | P3.6: 05–06; the callout is the feature's name. |
| PS17.6 | Drag Shell 1 to the bottom | ✅ | P3.6: 07 (seen from below: the counterbores whole, the shell wrapping them); the volume matches the closed form (`gear_cover.rs`). |
| PS17.7 | Chamfer Distance and angle 2 mm × 45° | ✅ | P3.6: on the two bosses' top edges (the stand-in's), 08. |
| PS17.8 | Fillet 1 mm, overflow on | ✅ | P3.6: three bottom rim edges, their tangent chain the whole rim, 09. |
| PS17.9 | Fillet Width 3 mm on a loft edge | ✅ | P3.6: on the stand-in's slope edge ("Edge of Extrude 2", its angle to the walls 79°–90°): Radius 3 (10), then Width 3 (11), zoomed on the front corner as ex3-step9's inset (11b). |
| PS17.10 | Mass properties with Aluminum 380 | ✅ | P3.6: 0.521 kg (V 188 789.391 mm³ × 2760 kg/m³), the CoM glyph in the view, X = 0 by symmetry (12). Before the fillets the volume equals the closed form 189 140.949 mm³ to 1e−3 mm³. The Width fillet's removal (226.2 mm³) is bounded analytically: its chain's straight edges exactly (r²(tan(φ/2) − φ/2) per mm at 90° and 78.69°), the R20 corners between those rates less Pappus's shortening, the two ends at most A·r each: 207.5 to 236.2 mm³. OCCT's volume matches the display mesh's (0.05 mm deflection) to 1.5e−4 relative (it is 9.3e−5: the mesh chords cut into the curved faces). The mass is recorded as a golden 0.521 059 kg (`gear_cover.rs`). |

## 6. Multi-part Part Studios

### PS18 Multi-part applications
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS18.1 | One master sketch drives independent parts | ✅ | P3.3: one sketch drives several parts (one per separate solid of an extrude, and further extrudes of its regions); each part has its own name and visibility. |
| PS18.2 | Reference other parts: sketch on faces, Use edges, Up to face | ✅ | P3.3: Up to face (and part, vertex) of another part, besides sketch on face and Use. |
| PS18.3 | One feature acting on several parts | ✅ | P3.3: an extrude's Remove/Intersect (and Add with a merge scope) and the Boolean feature act on several parts at once. Fillet etc. come in P3.6. P3.10: a Fillet (and Chamfer, Shell, Draft: all run per part) on several parts at once. `course_ps14_fillet_options` 06; test `one_fillet_on_two_parts`. |
| PS18.4 | Guidance: identical copies belong in assemblies, one BOM line per part | ✅ | Guidance only; nothing to build. |
| PS18.5 | Split, Shell one piece, Boolean Union back together | ✅ | P3.7: the **Split** feature (parts or surfaces to split; a plane, a Plane feature, a face or a sketch to split with; `BRepAlgoAPI_Splitter`), one part per piece. Scenario `course_ps18_split`: split at mid-height, the upper piece shelled, the two unioned. Bridging two modelled pieces needs nothing new. P3.8: the dialog as Onshape's (help page "Split"): **Part** and **Face** tabs, *Keep tools*, *Trim to face boundaries* (a face tool), *Keep both sides* with its opposite-direction arrow; the Face type splits only the picked faces (`Kernel::split_faces`). `course_ps18_split_options` 01–04; test `split_types_and_options`. |

## 7. Advanced features

### PS19 Sweep
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS19.1 | Profile + path; Solid/Surface/Thin; booleans | ✅ | P3.7: **Sweep** (`Kernel::sweep_with`, `SweepSpec` with a 3D path of sketch curves and model edges), Solid / Surface / Thin, New / Add / Remove / Intersect. Ø10 along an L with an R50 bend: V = π·25·(L₁ + L₂ + π/2·50) exactly (conformance `sweep_along_paths`). Scenario `course_ps19_sweep`. |
| PS19.2 | Path from entities, sketch, curves, model edges | ✅ | Sketch curves, a whole sketch (picked in the list) and part edges, chained end to end in any order and direction. |
| PS19.3 | Closed profile gives a solid; open gives a surface or thin body; Thin closed gives a hollow sweep | ✅ | A region or planar face gives a solid; open chains a surface or a thin wall; a closed profile with Thin a hollow tube (`course_ps19_sweep` 02–03: the default 5 mm wall on a Ø10 fails and says why, 1 mm works). |
| PS19.4 | Profile on a curve-point plane; Pierce | ✅ | `course_ps19_sweep_planes` 01–03: a Curve point plane at the path's end, the profile's centre Pierced to the path, swept. |
| PS19.5 | Profile mid-path sweeps both ways | ✅ | Two spines from where the profile's plane crosses the path (conformance `sweep_along_paths`; `course_ps19_sweep_planes` 04–05: a profile 30 along the path runs over the whole path). On a closed path it runs once round. |
| PS19.6 | Profile control, merge scope, Final | ✅ | None, Keep profile orientation, Lock profile direction (with its Direction field); Merge with all and Merge scope; **Final** in every applied feature dialog's footer (see PS21.11). |

### PS20 Loft
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS20.1 | Ordered profiles (sketches, faces, point) | ✅ | P3.7: **Loft** (`Kernel::loft_with`): regions of a sketch (joined into one contour: "Faces of Sketch 2"), a whole sketch, planar faces, a sketch point or vertex first or last; Solid, Surface, Thin. Equal squares give the prism, two circles the frustum, a circle and a point the cone (conformance `loft_sections_and_conditions`). Scenario `course_ps20_loft`. P3.10: **non-planar faces** as profiles (the face itself is the loft's cap, sewn on). Tests `loft_from_a_curved_face` (core: 12 000 − 1000π through the feature list; conformance: the same, joined to the half cylinder into the box); `course_p310_profiles` 02 (P3.11: reframed, the rod clear of the dialog, Plane 1 sized to the parts; fix round 1: the half cylinder moved in from x = 100 to 40, so the parts and the plane sized to them, as `ex4-step5.png`, are compact). A whole surface (sheet body) as a profile: not built; its profile would be the sheet's free-edge loop, and cadrs makes no bounded planar sheets (its surfaces are extruded, revolved, swept or lofted walls, open at the ends), so no course step can pick one; a surface's face is a face profile like a part's. |
| PS20.2 | Reorder items with drag handles | ✅ | The Profiles list's ↑↓ **Reorder items** button turns on drag handles and a **Done** button (`cadrs_ui::SelectionList::reorderable`); a drop is one command (`course_ps20_loft` 03–04). |
| PS20.3 | Matching vertex counts; Split a circle | ✅ | Profiles with different edge counts loft with or without end conditions: with conditions, each profile is cut where its direction from its centre meets the corners of the profile with the most edges (a square's sides meet a circle's quarters; conformance `loft_sections_and_conditions`, `course_ps20_loft` 05). Sketch Split (S18) can still be used to choose the matching. |
| PS20.4 | Start/End conditions and magnitudes | ✅ | **Normal to profile** and **Tangent to profile** with Start/End magnitude (the section columns interpolated with end derivatives, see KERNEL.md; the funnel's loft matches Onshape's volume to 0.04 %). P3.10: **Match tangent** and **Match curvature** (face profiles): the loft continues the neighbouring faces (G1; G2 with their normal curvature, two profiles). `course_ps20_loft_match` 01–05; tests `loft_match_tangent` (conformance), `loft_match_tangent_from_a_face` (core). P3.11: **Normal direction** and **Tangent direction**, each with a Start/End direction field under its condition that takes a picked vector (an edge, a sketch line, a face's or plane's normal, a mate connector's Z): Normal to profile / Tangent to profile with the picked direction in place of the profile's normal (kernel `LoftCondition::{NormalDirection, TangentDirection}`, the same per-sample end derivatives; the direction is turned to point along the loft). Tests: conformance `loft_direction_conditions` (along the profile's normal they equal Normal/Tangent to profile; oblique on equal squares the volume stays 2000 by Cavalieri while the body leans), core `loft_normal_and_tangent_direction`. `course_ps20_loft_direction` 01–03 (fix round 1: the direction a sketch line leaning 14°, so the loft leans cleanly, with no fin at the square's corner; Tangent direction's End magnitude 0.2, so it rolls into the circle; the picked directions drawn as arrows on the first and last profile while the dialog is open, `Build::arrows`). |
| PS20.5 | Multi-contour profile shown in red | ✅ | A profile of two separate contours is red in the Profiles list and the dialog says why (`course_ps20_loft` 08). |
| PS20.6 | Thin loft | ✅ | The loft's side sheet thickened (Thickness 1/2, Mid plane): conformance (a thin loft between equal circles is the tube) and `course_ps20_loft` 09. |

### PS21 Exercise: Funnel
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS21.1 | Inch and **pound** units | ✅ | Inch and pound workspace units (P3.5); `course_ps21_funnel` 01. |
| PS21.2 | Ellipse 6×4, offset 0.125 inward, symmetric tab, 105° | ✅ | P3.7: **offset of an ellipse** is an exact offset-ellipse curve (`CurveKind::EllipseOffset`, a C2 B-spline to 1e-8 in the kernel), its distance a driving dimension drawn between the two curves. `course_ps21_funnel` 02: the construction centreline, the slanted lines **Symmetric** about it, the end line **1.5**, **4.5** from the origin, **105°**: fully defined. |
| PS21.3 | Extrude whole sketch New 0.125 | ✅ | The whole sketch picked in the list: the band and the handle, the inner disc left out. |
| PS21.4 | Sketch on the bottom face; offset the ellipse 0.05 | ✅ | On the rim's bottom face (picked from the Bottom view, as `ex4-step4.png`): Use of the rim's inner edge projects the exact offset ellipse (links test `use_of_an_offset_ellipse_edge_projects_the_whole_offset_ellipse`), offset 0.05 outwards. |
| PS21.5 | Plane Offset 3 in → "Lower Plane" | ✅ | Offset 3 in from Top, flipped down; renamed in the list. |
| PS21.6 | Plane Mid plane → "Middle Plane" | ✅ | Mid plane of Top and the Lower Plane (z = −1.5 in). |
| PS21.7 | Ø2.5 circle on Middle Plane, 0.5 from origin | ✅ | **Direction resolved** (`cadrs_core/tests/funnel.rs` builds both): the circles lie **away from the handle (−X)**, as drawn: CoM X 0.556 in (the screenshot's 0.556); on +X it would be 0.778. The scenario dimensions the 0.5 and makes the centre Horizontal to the origin. |
| PS21.8 | Ø0.5 circle on Lower Plane, 0.75 from origin | ✅ | At x = −0.75 (same side), dimensioned. |
| PS21.9 | Loft Add; Normal to profile; magnitudes 0.5 / 1 | ✅ | Solid / **Add**, profiles "Faces of Sketch 2", Sketch 3, Sketch 4; Normal to profile, 0.5 and 1 (`course_ps21_funnel` 07). |
| PS21.10 | Shell 0.05 fails on the handle | ✅ | With the loft Added to the rim, Shell 1 fails: its title and the faces to remove are red, the part's edges red (`ex4-step10.png`); ✓ keeps it, red in the list (`course_ps21_funnel` 08). |
| PS21.11 | Edit Loft → New; Final shows the fix | ✅ | P3.7 brings **Final** forward from P3.9: editing a feature before the end shows the Part Studio rolled back to it (09); New (10); Final shows the features after it, without the edited feature's preview: the Shell now works (11, as `ex4-step11.png`). The rollback bar: P3.9 (PS13.1). |
| PS21.12 | Boolean Union; the first tool gives the properties | ✅ | The loft's part picked first keeps its identity. |
| PS21.13 | Fillet R0.5 on the tab corners | ✅ | The four vertical corner edges. |
| PS21.14 | D-shaped bead sketch on Front, Pierce to the edge | ✅ | The R0.125 3-point arc on the handle's top outer edge (its radius dimensioned) and three lines. |
| PS21.15 | Sweep along 8 rim edges (Create selection → Tangent connected) | ✅ | Right-click → Select → **Create selection…** → Tangent connected → Add selection (X12): **8 edges**, as Onshape (a sliver edge OCCT's fillet leaves where two top faces meet is merged away, `brep::SLIVER`). |
| PS21.16 | Extrude a face Add 1 in −Z (spout) | ✅ | The loft's bottom annulus, picked from the Bottom view, Add 1 in. |
| PS21.17 | Rename; Assign Polypropylene | ✅ | |
| PS21.18 | Mass properties | ✅ | `course_ps21_funnel` 18 reads **0.098 lb**, **2.973 in³**, **84.106 in²**, CoM (0.556, −8.141e−6, −0.548) (values that would read 0.000 in scientific notation, as Onshape), Lxx 0.205, Lyy 0.518, Lxz −0.059 in² lb (course: 0.098 lb, 2.974, 84.098, (0.556, −7.5e−5, −0.547), 0.205, 0.518, −0.059). Polypropylene is 0.033 lb/in³ (913.437 kg/m³) as Onshape's library (`ex4-step17.png`), which `ex4-step18.png`'s mass and inertia confirm. `funnel.rs` asserts the 3-decimal mass and inertia. |

## 8. Patterning

### PS22 Introduction to patterns
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS22.1 | Linear, Circular, Mirror, Curve | ✅ | P3.8: the toolbar's pattern menu (Linear, Circular, Curve pattern) and Mirror. Scenarios `course_ps22_skip_instances`, `course_x11_mate_connectors` 06, `course_ps26_mirror`, `course_ps27_reflector` 05, 09, 12. |
| PS22.2 | Part / Feature / Face pattern types | ✅ | P3.8: **Part** (copies; New, Add, Remove, Intersect), **Feature** (the features' effect moved, or reapplied), **Face** (the faces' pocket or boss volume, `Kernel::face_tool`, cut or added by `Kernel::classify`; OCCT has no face replacement, but a pocket's or boss's faces bound a volume, which is enough for the course's face patterns and mirrors). Tests in `cadrs_core/tests/pattern.rs`. |
| PS22.3 | Reapply features | ✅ | P3.8: the features computed again per instance with their sketch planes moved and references remapped, so an Up to face extrude meets the curved bottom at each place (`course_ps27_reflector` 05; test `circular_feature_pattern_of_a_hole`, with and without Reapply: 6 × the hole's volume). |
| PS22.4 | Create selection helper | ✅ | P3.8: the Face pattern and Face mirror entity fields have the Create selection button; Faces → **Pocket** (the connected faces of a pocket: `Solid::pocket_faces`, concave boundary) → Add selection (`course_ps27_reflector` 09, 12a). |
| PS22.5 | Skip instances (grey dots, index list, CLEAR) | ✅ | P3.8: Skip instances shows a dot on every instance (grey; skipped ones light blue); clicking one toggles it; "Instances to skip" lists them as "(2, 0)" with CLEAR (`course_ps22_skip_instances` 01–05; `course_ps27_reflector` 09 skips (2, 0) and (3, 0)). |
| PS22.6 | Merge scope / Merge with all | ✅ | P3.8: Part patterns and mirrors with Add/Remove/Intersect have Merge with all and Merge scope, as the extrude (test `mirror_of_a_half_control_arm`: Add, one part). |

### PS23 Linear pattern
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS23.1 | Direction, distance, count, flip | ✅ | P3.8: Direction from an edge, a sketch line, a face's normal, a plane's normal or a mate connector; distance, instance count and its flip (`course_ps22_skip_instances`, `course_ps27_reflector` 09). |
| PS23.2 | Centered | ✅ | P3.8: Centered spreads the instances about the seed (test `linear_part_pattern_of_a_cube`). |
| PS23.3 | Second direction (grid) | ✅ | P3.8: a second direction with its own distance, count, flip and Centered; instances indexed (i, j) (`course_ps22_skip_instances` 03; test: 10 parts). |

### PS24 Circular pattern
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS24.1 | Axis: circular edge, cylindrical face, mate connector, circle | ✅ | P3.8: the axis field takes circular edges, cylindrical faces, sketch circles and lines, straight edges and mate connectors (their Z, the field's connector button): `course_x11_mate_connectors` 06, `course_ps27_reflector` 05. |
| PS24.2 | Angle, count, Equal spacing semantics, Centered | ✅ | P3.8: Equal spacing spreads the count over the angle (360: count steps, else count − 1), else the angle is the step; Centered; flip (test `circular_feature_pattern_of_a_hole`). |

### PS25 Curve pattern
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS25.1 | Path chain | ✅ | P3.8: the path is sketch curves and part edges chained end to end (lines and arcs exactly; test `curve_pattern_along_an_arc`). |
| PS25.2 | Equal spacing / distance | ✅ | P3.8: Equal spacing spreads the count over the path's length, else the distance is the step along it. |
| PS25.3 | Tangent to curve orientation | ✅ | P3.8: Tangent to curve turns each instance with the path's tangent (test: the arc's instances turned by the arc's angle). |

### PS26 Mirror
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS26.1 | Mirror across a face, plane or mate connector | ✅ | P3.8: `Kernel::transform_motion` with a reflection (`Motion::reflection`; conformance `mirror_and_motions`); the mirror plane is a default plane, a Plane feature, a planar face or a mate connector's XY plane. |
| PS26.2 | Part mirror with Add | ✅ | P3.8: the half Control Arm mirrored across Front with Add: one part, **368 749.705 mm³** (test `mirror_of_a_half_control_arm`; `course_ps26_mirror` 01–03). The unioned part keeps seam faces along the mirror plane (the union is made without merging coplanar faces when OCCT's merge would lose faces, KERNEL.md). |
| PS26.3 | Part / Feature / Face mirror; Create selection | ✅ | P3.8: all three types (test `face_and_feature_mirror_of_a_pocket`; `course_ps26_mirror` 04 feature mirror of a hole; `course_ps27_reflector` 12 face mirror of the pocket's 9 faces from Create selection). |

### PS27 Exercise: Rocket Guidance Reflector
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PS27.1 | Copy the doc; mm and kg | ✅ | P3.8: the stand-in document (see below) opened with the `reflector` script command; Workspace units Millimeter and Kilogram (`course_ps27_reflector` 01–02). |
| PS27.2 | Mate connector as a coordinate system | ✅ | P3.8: the stand-in's "Pattern Axis" Mate connector feature on the top face centre; the connectors' triads (X red, Y green, Z blue). |
| PS27.3 | Extrude Remove, Up to face + 10 mm offset | ✅ | P3.8: the two triangles, Up to face the curved bottom (Face of Revolve 1) with Offset distance 10 (`course_ps27_reflector` 03; the volume drop by Gauss–Legendre quadrature in `tests/reflector.rs`). |
| PS27.4 | Fillet 6 mm on 12 edges | ✅ | P3.8: 12 edges (6 vertical, 6 round the floors), 6 mm (04). The fillets' volume isn't closed-form: bounded analytically and checked against the display mesh (6e−4 overall). |
| PS27.5 | Circular feature pattern ×4 about a mate connector, Reapply | ✅ | P3.8: Extrude 2 and Fillet 1, ×4 about Pattern Axis, Reapply (05): the volume drop ×3 more, CoM X, Y = 0 to 1e−6. |
| PS27.6 | Hole Simple, at a mate connector, M45 Close Ø46, Blind 12, 118° | ✅ | P3.8: at the Pattern Axis connector (06); closed form in the test. |
| PS27.7 | Sketch point Vertical, 26 below the origin; K toggles connectors | ✅ | P3.8: the point Vertical to the origin, dimensioned **65 from the origin** (= 26 from the stand-in's front edge; the course dimensions it to the edge) (07); **K** shows and hides mate connectors (`course_x11_mate_connectors` 02–03). |
| PS27.8 | Tapped hole M10×1.5, tap drill 8.5, Blind 20, tapped 10.02 | ✅ | P3.8: (08); the tap drill's blind volume in the test. |
| PS27.9 | Linear face pattern ×6 @ 26 with skipped (2,0), (3,0) | ✅ | P3.8: Face pattern of the tapped hole's faces (Create selection → Pocket), along the straight side edge, 26 mm, 6, (2, 0) and (3, 0) skipped on their dots (09): 3 more holes in the test. |
| PS27.10 | Show sketch; Extrude Remove the rectangle, Blind 11 | ✅ | P3.8: the Feature Sketch shown with its eye; 13 × 32 × 11 (10). |
| PS27.11 | Fillet 6 mm on the pocket | ✅ | P3.8: the pocket's 4 vertical edges (11), 4(1 − π/4)·36·11 in the test. |
| PS27.12 | Face mirror of 9 pocket faces across Right | ✅ | P3.8: Create selection → Faces → Pocket on the floor finds the 9 faces (12a), mirrored across Right (12). |
| PS27.13 | Mass properties with Aluminum 1060 | ✅ | P3.8: Aluminum - 1060: **2.665 kg**, V **985 314.338 mm³** as the panel shows it (13; P3.11 fix round 1: the text cited 985 314.335, the core test's value, 3e−9 of the volume away from the panel's), CoM (−2.4e−10, 7.06e−8, …) (P3.11: near-zero values show as 0.000); the test asserts the mass (`GOLDEN_MASS_KG` 2.665 275 kg in `tests/reflector.rs`, within 5e−4). Onshape's 2.04 kg is for its own document, which the stand-in doesn't reproduce. |

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Region-based input, whole sketch, toggle, auto-hide, eye | ✅ | P3.3: regions, whole sketches, toggle, auto-hide and the eye. |
| X2 | Solid/Surface/Thin × New/Add/Remove/Intersect, merge scope, automatic tab | ✅ | P3.3: Solid/Surface/Thin × New/Add/Remove/Intersect (surfaces New only), merge scope and Merge with all, the automatic tab. |
| X3 | End types and the extrude options | ✅ | P3.3: every end type with offsets, Symmetric, Second end position, Starting offset, Direction. No Draft (not scheduled). P3.4: the revolve's (Full, Blind, Symmetric, Up to …, second end). P3.10: **Draft** (the angle inline with its flip, solids only), Symmetric only for Blind and Through all, and an Up to end's Offset distance inline on its checkbox row (P3.8 judge). `course_ps4_draft` 03–05 (05 built: Up to the drafted cube's top, offset 5); test `extrude_with_draft`. |
| X4 | Dialog chrome: ✓✗, rollback slider, Final, roll back on edit | ✅ | ✓✗ and Enter/Esc (`cadrs_ui/src/feature_dialog.rs`); P3.9: the before/after slider (PS13.2), Final (PS13.3), roll back on edit (PS13.1). |
| X5 | Folders, filter, show dependencies, reorder, regeneration times | ✅ | P3.6: folders and drag to reorder (PS11.3); P3.9: add selection, rename, drop onto a folder, delete with contents (PS3.1–3.3), the filter (PS3.4–3.6), Show dependencies (PS11.2), regeneration times (PS2.6), suppress / unsuppress (`SetSuppressed`, greyed and struck through, not built; `course_ps13_rollback_final` 10–12, core test `suppressed_features_are_not_built`). |
| X6 | Parts list with groups and a context menu | ✅ | P3.3: Parts and Surfaces groups; Rename, Hide/Show, Isolate, Delete. P3.5: Assign material, Edit appearance and the palette swatches. P3.11: no Curves group because no feature makes curve bodies (see PS2.8). |
| X7 | Mass and section properties panel | ✅ | P3.3: Part tab: Parts to measure (the selection), Volume, Surface area in the workspace units, several parts together. P3.4: in inches. P3.5: each measured part's name is red without a material and black with one; **Mass**, **Center of mass** X Y Z and the **inertia tensor** Lxx…Lzz (about the centre of mass, axes parallel to the Part Studio's, Onshape's convention without a reference mate connector) once every measured part has a material, in the workspace mass and length units. `course_ps10_material` 01, 05, 10, 12. Face tab, Override and the mate connector later. P3.10: the **Face** tab (Faces to measure, exact Area and Centroid), **Override** mass (the typed mass; the centre stays, the inertia scales; without materials a uniform density) and the **Mate connector for reference frame** (an explicit or implicit connector: the centre of mass in its frame, the inertia along its axes). The panel's settings, not saved with the document. Fix round 1: a typed value survives leaving its field (the lost Override, see the status below); each tab shows only what it measures (the Face tab its faces, the Part tab's parts put back when it returns); the reference connector's glyph (disc and triad) drawn in the view. `course_x7_mass_options` 01–04 (02 checks the typed 10 kg and 16 666.667); tests `mass_properties_override_and_reference`, `face_areas_are_exact`. The centre-of-mass and inertia Overrides stay unavailable. |
| X8 | Workspace units: length and **mass** | ✅ | Length and decimals (`units_dialog.rs`, `cadrs_sketch::units`). P3.5: Mass units (Kilogram, Gram, Pound, Ounce; `MassUnit`, saved with the document, older files read as kg): mass, density and inertia follow it. `course_ps10_material` 09–10; `course_ps8_reducer_coupling` 01 now sets Pound as the course does. |
| X9 | Materials and appearances | ✅ | P3.5: PS9 and PS10 above. |
| X10 | New features beyond Extrude | ✅ | Revolve, Boolean (P3.3–P3.4); P3.6: Fillet, Chamfer, Shell, Hole. P3.10: Draft; fillet Asymmetric and Variable; the rest of the list (Full round, conic and curvature beyond P3.6: out of scope, niche; out of scope by user decision 2026-09-29). |
| X11 | Mate connectors (explicit, implicit, K) | ✅ | P3.8: the Mate connector feature (origin entity, flip primary axis, reorient, X/Y/Z move, rotation), implicit connectors shown as dots on hover while a connector field is active, **K** (listed on in `shortcuts.rs`). `course_x11_mate_connectors` 01–06. |
| X12 | Create selection; Skip instances | ✅ | P3.7: Create selection → Edges → Tangent connected. P3.8: Faces → **Pocket**; Skip instances. Other Create selection rules stay disabled. |
| X13 | Sketch tools reused: Use, Pierce, diametral dimensions, ellipse offset, sketch on faces | ✅ | Use, Pierce, ellipse and sketch on faces work, on kernel edges and faces by persistent name (P3.2; version 3 documents convert). P3.4: **diametral dimensions** to a construction centreline (point or parallel line; the label across the line toggles it), Use takes circular edges from the kernel (exact circles), sketches on a revolve's faces; cone silhouettes by meridian rulings, torus silhouettes by a grid. P3.7: **ellipse offset** (exact), and Use of an offset-ellipse edge. |
| X14 | Sections and render modes | ✅ | **P3E.3**: render modes from the view cube menu (Shaded, Shaded without edges, Shaded with hidden edges, Hidden edges removed, Hidden edges visible, Translucent; `course_td_render_modes` 01–08) and the section view (a plane or a face, flip, offset, capped; `course_td_section` 01–07, 16–17; `a_section_through_a_40_mm_cylinder_caps_with_its_circle`). From the "Navigating a Document" course (Hands-On Test Drive TD6.5). (Before: → P3E.3.) |

Parametric-CAD items this course relies on: **P2.2/P2.3** (full rebuild from the list) 🟡, since
it runs today but only for prisms. **P5.2/P5.3** (variables and expressions) 🟡: arithmetic
expressions with units work in the depth field (`depth_expr`), but there are no variables. **P5.5**
(linear pattern) ❌. **P6.4** (view cube corners → trimetric) ✅ (P3.9, `course_p6_cube_corner`).

## What each exercise needs

- **Control Arm (PS6), buildable from scratch.** Steps 1–3 work today. It still needs:
  1. **Kernel-backed extrude**: exact arcs, so the area comes out exact.
  2. The **sketch-row eye** (PS1.5).
  3. **Add** with automatic tab selection and union into Part 1 (PS5.2, PS5.3).
  4. A **Parts-list context menu → Rename** (PS6.5).
  5. The **Mass and section properties** panel with Volume and Surface area (X7).

  Self-check: **surface area 50 179.71 mm²**, with **volume 368 749.705 mm³**. The OCCT spike
  already reproduces both: 368 749.705 and 50 179.711 (`crates/cadrs_kernel/README.md`).
- **Reducer Coupling (PS8), buildable from scratch, in inches.** It needs:
  1. **Whole-sketch input** (PS1.1). Picking regions one by one is a fallback.
  2. **Diametral dimensions** to a construction centreline (X13).
  3. A **Revolve** feature: Full, axis from a sketch line or a cylindrical face or circular edge
     (PS7.1–7.3, 7.5).
  4. Sketching on and **Use** of edges of a revolved (toroidal/conical) body. This means Use has
     to take its edges from the kernel instead of `Solid` rulings.
  5. **Add** (PS5).
  6. Part **Rename** and the **mass properties** panel, in in³ and in².

  Self-check: **surface area 248.367 in²** (predicted), with **volume 53.932 in³** (analytic
  53.9318 = flange 1 14.8214 + transition 13.0542 by Pappus + flange 2 26.0562). The optional
  PS8.9 also needs **Remove**.
- **Jackhammer Gear Cover (PS17), needs a starting document.** We can't copy Onshape's public
  document, so we'd ship a seeded cadrs stand-in, `fixtures/gear_cover_standin.cadrs`, built from
  our own features:
  - a rounded-rectangle cover, extruded and then lofted or drafted at the top so it has a sloped
    "Edge of Loft 2" for PS17.9;
  - two bosses whose circular edges act as the "A" references;
  - Aluminum 380 assigned;
  - its features grouped in a "Base Features" folder.

  The workflow scenario `course_ps17_gear_cover` then runs these steps:
  1. Shell the bottom face at 4 mm.
  2. Sketch the 6 points: Symmetric, and Concentric to the boss edges via Use.
  3. Hole: M5 Close counterbore Ø9.75×5, through all.
  4. Drag Shell 1 to the bottom.
  5. Chamfer 2 mm × 45°.
  6. Fillet R1 with overflow.
  7. Fillet Width 3 mm.
  8. Mass properties in kg.

  P3.6: built as planned, with the slope as an extruded cut instead of a loft or draft (neither
  exists yet) and the bosses on the top; see "P3.6 status" below for the substitution and its
  closed-form checks.

  Capability gaps: Shell, Hole (with a clearance table and counterbore), drag reorder, Chamfer
  (distance and angle), Fillet (radius and width), tangent propagation, edge picking, folders,
  materials, mass units, and an image tab (optional). Onshape's own values can't be reached with
  a stand-in (V 160 818.108 mm³, A 82 534.703 mm², CoM (−2.102e−4, −59.006, 23.106) mm, mass
  ≈ 0.444 kg at ρ 2.76 g/cm³). The check is analytic for the stand-in instead:
  - before the fillets, the shelled box with holes has a closed-form volume;
  - the CoM X must be 0 by symmetry;
  - after the fillets, check against OCCT's value, recorded once as a golden with a tolerance
    and cross-checked against a finely tessellated mesh volume.
- **Funnel (PS21), buildable from scratch, in inches and pounds.** It needs:
  1. **Ellipse offset** in sketches (PS21.2, PS21.4).
  2. Whole-sketch input.
  3. The **Plane** feature: Offset and Mid plane (PS12.2).
  4. **Loft** with Normal-to-profile end conditions and magnitudes (PS20.1, PS20.4).
  5. **Shell** with a clear failure state (PS16.4), and **Edit → New** with **Final** (PS13.3).
  6. **Boolean Union**, where the first tool supplies the properties (PS5.5).
  7. Fillet.
  8. **Sweep** along model edges, using **Create selection → Tangent connected** (PS19, X12).
  9. **Face input** for an extrude (PS4.2).
  10. **Polypropylene** and **mass in lb** (X8, PS10).

  Self-check: **mass ≈ 0.098 lb**, from V 2.974 in³ × 0.033 lb/in³. The screenshot also shows
  A 84.098 in², CoM (0.556, −7.545e−5, −0.547) in, and Lxx 0.205, Lyy 0.518, Lxz −0.059 in²·lb.
  The loft differs from Onshape's (Parasolid), so compare the volume within ~1 %.

  **Flagged direction question (PS21.7/21.8):** the drawing (`ex4-step7.png`) puts the Ø2.5 and
  Ø0.5 circles on the side *away from* the handle, which is −X since the handle is on +X. But the
  screenshot CoM is X = **+0.556**, which fits either reading, because the heavy handle and bead
  are on +X. Build both variants in a unit test and keep the one whose CoM X and volume are
  closer to (0.556, 2.974). Record the answer in the requirements file.
  **Answer (P3.7):** away from the handle (−X), as drawn. Built both ways
  (`cadrs_core/tests/funnel.rs`): −X gives CoM X 0.5558 and V 2.9752 in³; +X gives CoM X 0.778.
- **Rocket Guidance Reflector (PS27), needs a starting document.** We'd ship a stand-in,
  `fixtures/reflector_standin.cadrs`:
  - a rounded-square plate, with its curved bottom from a Revolve (Intersect or Remove of a large
    arc profile);
  - a **Pattern Axis** mate connector at the top-face centre;
  - a "Feature Sketch" with two triangles and a rectangle;
  - Aluminum 1060 assigned.

  The scenario `course_ps27_reflector` then runs these steps:
  1. Extrude Remove up to the curved face, with a 10 mm offset.
  2. Fillet 6 mm.
  3. Circular **feature** pattern ×4 about the mate connector, with Reapply.
  4. Hole Simple M45 Close Ø46, Blind 12, placed at the connector.
  5. Tapped hole M10×1.5, Blind 20 / 10.02.
  6. Linear **face** pattern ×6 @ 26, skipping (2,0) and (3,0).
  7. Pocket Remove, 11 mm.
  8. Fillet.
  9. **Face mirror** across Right.
  10. Mass properties in kg.

  Capability gaps: mate connectors (explicit, implicit, K), Up to face with offset, circular and
  linear patterns (feature and face types, skip instances), Hole (simple and tapped tables),
  Mirror (face type, with reflection support in the kernel), Create selection → Pocket, and
  materials. Onshape's values for reference: V 754 429.926 mm³, A 123 490.317 mm², CoM
  (−1.465e−5, −3.110e−5, 30.533) mm, mass ≈ 2.04 kg at ρ 2.705. For the stand-in, check that the
  CoM X and Y are 0 (a 4-fold pattern plus a mirror means symmetry). Check the volume drop of each
  step against a closed form: the pocket is 11 mm deep and its fillets are analytic, and the holes
  are cylinder plus cone.

## Proposed milestones (phase 3)
Order: move to the kernel first, so every later feature is a thin layer over `cadrs_kernel`.
Then each exercise gives a milestone its acceptance test. Every milestone keeps
`cargo test --workspace` and clippy clean. Each milestone ships **conformance tests** in
`crates/cadrs_kernel/tests/conformance.rs`, **headless scenarios** (`course_ps*`) and a judge
round against the course screenshots in `intro-to-part-studios/` (gate ≥ 8.5, as in phase 2).

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3.1 | **Kernel rebuild**. Details below the table. | P2.2/P2.3 groundwork, PS1.4, PS11.1 (part), PS4.4 (kept) | Every existing solid scenario renders the same or is re-blessed with a stated reason: `extrude_rectangle`, `extrude_preview_drag`, `sketch_on_face`, `course_s20_use_*`, `course_s21_*`, `reload_roundtrip*`, `perf_500`. Conformance: a rectangle 100×60×25 gives V = 150 000 and A = 20 000 (2(6000 + 2500 + 1500)). **Control Arm with both extrudes as New** gives a total volume of 368 749.705 mm³ ± 1e−3. Rebuild of a 20-feature studio takes < 100 ms. A Windows exe with OCCT builds and launches. |
| P3.2 | **Persistent naming and 3D picking**. Details below the table. | PS14.2 (picking part), PS18.2, X13 (Use on kernel edges), prerequisite for all edge/face features | Unit tests: change a Control Arm sketch dimension, change an extrude depth, then reorder two independent extrudes. In each case every sketch-on-face, Use link and stored `EdgeId` still resolves to the geometrically matching entity. Scenarios: `course_ps_edge_hover` (edge, face and vertex hover and selection on the Control Arm) and `course_ps_face_names` (the selection field reads "Face of Extrude 1"). An old v3 document still loads. |
| P3.3 | **Extrude complete, Boolean, Parts, Mass properties**. Details below the table. | PS1.1–1.5, PS4.1–4.8, PS4.10–4.12, PS5.1–5.5, PS6.*, PS2.8, PS18.1–18.3, X1–X3, X6, X7 (volume/area/CoM) | `course_ps6_control_arm` reproduces the course steps and screenshots `ex1-step1..6`. The mass-properties panel reads **Volume 368 749.705 mm³** and **Surface area 50 179.71 mm²**, and a unit test asserts both to 0.01. Other scenarios: `course_ps4_end_types` (Up to face, Through all, Symmetric), `course_ps5_boolean` (Remove and Intersect of two boxes, V from a closed form), `course_ps1_whole_sketch`. |
| P3.4 | **Revolve and the Reducer Coupling**. Details below the table. | PS7.*, PS8.*, PS1.6 (revolve), X13 | `course_ps8_reducer_coupling` in inches reads **Volume 53.932 in³** and **Surface area 248.367 in²**, asserted in a test to 1e−3. The optional single-revolve variant (PS8.9) gives the same volume. `course_ps7_revolve_types`: Full, Blind 90°, Symmetric; a torus from a circle gives V = 2π²Rr². |
| P3.5 | **Appearance, material, mass units**. Details below the table. | PS9.1–9.7, PS10.*, X8, X9, PS2.10 (Appearances) | `course_ps9_appearance` (palette cycling, hex entry, transparency, face override) and `course_ps10_material`. A test gives a 100 mm steel cube of 7.85 kg with Ixx = m(b²+c²)/12 = 13 083.3 kg·mm². The Control Arm in Polypropylene shows mass = V × ρ. The parts list shows the name in red without a material. |
| P3.6 | **Fillet, chamfer, hole, shell, reorder**. Details below the table. | PS14.1–14.5, PS14.7–14.9, PS15.1, PS15.3–15.7, PS15.10, PS16.*, PS11.3, PS11.4, PS17.* | A cube 100³ with R10 on 12 edges has V = 1e6 − (4−π)·100·12·100/4 plus the corner correction, computed in the test. Chamfer 2×45° on a box edge removes 2·2/2·L. Shell 4 mm of a 100×60×40 box, bottom open, has V = 100·60·40 − 92·52·36. A counterbore hole matches a closed form (cylinders plus the 118° cone). `course_ps17_gear_cover` on the stand-in runs every step, including the Shell reorder, and ends with the analytic/golden mass in kg. `course_ps16_shell_fail` shows the red error state. |
| P3.7 | **Reference planes, sweep, loft, split**. Details below the table. | PS12.2–12.3, PS19.*, PS20.*, PS21.*, PS18.5, X12 (Tangent connected), X13 (ellipse offset) | Plane-type tests: each of the 8 types against hand-built frames. Sweep of a Ø10 circle along an L path with a bend radius of 50 gives V = π·25·(L₁ + L₂ + π/2·50). A loft between equal squares equals the prism volume; between two circles with default conditions, the frustum volume. `course_ps21_funnel` in inches and pounds, with the PS21.7 direction resolved, reads **mass ≈ 0.098 lb** and volume within 1 % of 2.974 in³, area within 2 % of 84.098 in². |
| P3.8 | **Patterns, mirror, mate connectors**. Details below the table. | PS22.*, PS23.*, PS24.*, PS25.*, PS26.*, PS27.*, PS15.2, PS9.6, X11, X12 (Pocket, Skip) | A linear part pattern of a 10 mm cube ×5 @ 20 has V ×5. A circular feature pattern of a hole ×6 removes 6× the hole volume. A mirror of a half Control Arm with Add equals the full volume. `course_ps27_reflector` runs every step on the stand-in, with CoM X,Y = 0 ± 1e−6 and the closed-form volume steps. `course_ps22_skip_instances` shows the dots and the (2,0) index list. |
| P3.9 | **Feature-list management and rollback**. Details below the table. | PS2.3–2.6, PS3.*, PS11.2, PS13.*, X4, X5, P6.4 | Scenarios: `course_ps3_folders`, `course_ps3_filter` ("Extrude" vs `"Extrude 1"` vs `:type Fillet`), `course_ps11_dependencies`, `course_ps13_rollback_final` (edit Extrude 1 of the Control Arm and check that Final shows downstream parts), `course_ps2_search_tools`, `course_p6_cube_corner`. The regeneration-time column shows non-zero times for the gear cover. |
| P3.10 | **Part Studios remainder: features**. Details below the table. | PS4.9, X3 (Draft), PS5.5, PS7.2, PS14.6, PS15.5 (PEM), PS15.6–15.8, PS20.1, PS20.4, PS1.6, PS11.1 (warnings), PS18.3, X7, X10 | Draft: a 100³ cube's four sides drafted 5° about the bottom face has the closed-form frustum volume; an extrude with Draft 5° likewise. Hole Start from selected plane and Up to entity closed forms. A variable-radius fillet test. Loft Match tangent between two lofts. Scenarios per item (`course_ps4_draft`, `course_ps15_hole_options`, `course_ps14_fillet_options`, `course_ps5_boolean_options`, `course_ps20_loft_match`). |
| P3.11 | **Part Studios remainder: carried deltas and audit**. Details below the table. | PS2.2, PS2.8, PS2.10, X6 (Curves group), PS11.2, the minor judge deltas carried from P3.1–P3.9, and every remaining 🟡/❌ row not in P3.10 or out of scope | Every PS/X row is ✅ or marked out of scope with a reason (except rows owned by 3E: sections, render modes, Measure and Analysis, X14, PS2.11 → P3E.3). The carried deltas are fixed or listed with a reason. |

**P3.1 Kernel rebuild** details:
- `cadrs_core` rebuilds the feature list through `cadrs_kernel::Kernel` with the OCCT backend.
- `Profile` is built from exact sketch curves, ellipses included.
- Each feature's output (`BodyId`s) is cached, so an edit rebuilds from the first changed feature.
- The display uses the kernel's tessellation (`TriMesh`) with per-face and per-edge ids, replacing
  `solid::extrude`. Face hover and sketch on face work on it.
- Extrude has the same scope as today (Solid/New/Blind).
- Rebuild errors show on the feature row.
- The Windows OCCT cross-build is verified.

**P3.1 status (2026-09-27): delivered.**
- `cadrs_core::rebuild` rebuilds the feature list through `cadrs_kernel` (OCCT) on a worker
  thread, with a per-feature cache keyed by the chain of feature parameters (an edit recomputes
  from the first changed feature). The app waits at most 30 ms per frame, then keeps the old
  parts on screen until the rebuild lands (`RebuildBudget`).
- `cadrs_core::brep` builds the `Profile` from the regions' exact pieces (lines, arcs, circles,
  ellipses and ellipse arcs; the fork gained a real ellipse binding) and converts the kernel
  tessellation to the app's `Solid`, tagging faces and edges with the prism's `FaceTag`/`EdgeTag`
  geometrically (unit tests compare tags, frames and edges with the prism). Face hover, sketch on
  face, Use (edges, faces, silhouettes) and imprinting work unchanged.
- OCCT exceptions are caught (fork `opencascade::safe`, commit `04a0dd2`).
- Rebuild errors: red feature row, reason as tooltip, header error icon.
- Tests: conformance 100×60×25 box (V 150 000, A 20 000; the acceptance row first said 20 200,
  an arithmetic slip, now corrected), Control Arm through the feature list with
  both extrudes New = 368 749.7048 mm³ (± 1e−3), 20-feature rebuild ≈ 40 ms from scratch in a
  debug build (asserted < 100 ms release, < 200 ms debug).
- Scenarios: every listed solid scenario renders within the golden tolerance of its
  pre-P3.1 render, except the face-hover frames, re-blessed because the hover outline is now
  2 px and depth-tested (judge round 1: the hidden side of a curved face showed through). New
  `course_ps_kernel_extrude` (kernel part with a round end, curved-face hover, boss on a face,
  failed Extrude 2 in red with an error tooltip, undo rebuilds it).
- Not done: persistent naming (P3.2); sketches on faces and links still rebuild the parts before
  them synchronously on the main thread (fast when cached).

**P3.2 Persistent naming and 3D picking** details:
- `naming.rs`: a `FaceId`/`EdgeId` from the op `History` ("Extrude 1 / side of c7",
  "… / end cap"), resolved after every rebuild.
- `PlaneRef::Face` and the Use/Pierce links move from `FaceTag`/`EdgeTag` to kernel names.
- Schema v4 with a migration from v3.
- Picking: **edges** and **vertices**, with hover and selection, and a selection model shared by
  faces, edges, vertices and parts.
- Geometry queries: tangent-connected edge chains and face adjacency.

**P3.2 status (2026-09-27): delivered.**
- `cadrs_kernel::naming`: `FaceName` (feature id + origin from the op `History`: the side of
  sketch curve c of a region, a region's start or end cap, a face from an input edge or vertex,
  and a split number), `EdgeName` (the two faces, + an index), `VertexName` (three faces, + an
  index), resolved after every rebuild: exact name, else the renamed piece nearest the old place,
  else a geometric fallback at the old place, else a *lost reference* error. The history comes
  from OCCT's `Generated`/`Modified`/`IsDeleted`/`FirstShape`/`LastShape`, bound in the fork
  (commit `c249012`); regions are keyed by sketch and boundary curves, not by list position.
- `PlaneRef::Face` and the Use/Pierce links store names; schema v4; v3 documents convert
  losslessly (checked-in fixture `crates/cadrs_core/tests/fixtures/v3_control_arm.ron`, written
  by the P3.1 code).
- Picking: vertices (8 px) over edges (6 px, visible ones only) over faces; hover in pale
  orange, selection in the course screenshots' amber (`#e8a838`, `ex3-step3.png`,
  `ex3-step9.png`, `lesson-fillet-and-chamfer.png`), with selected faces and parts tinted amber
  and outlined 3 px; one `Pick`/`Selection` model for planes, features, regions, faces, edges,
  vertices and parts (Parts-list rows select their part). A selection readout at the top left
  of the viewport lists the selection in selection fields ("Edge of Extrude 1", "Vertex of
  Extrude 1"; a stand-in until the Fillet dialog of P3.6). Cylinder seams are neither drawn nor
  picked, and silhouettes are no longer interpolated across the gap between two pieces of a
  face (a stray line inside the eye hole).
- Selection fields name a face after the feature that made it: "Face of Extrude 1" (and "Edge
  of …", "Vertex of …").
- Kernel queries: `adjacent_faces`, `tangent_chain`, `vertices`, exact edge geometry; 6 new
  conformance cases (26 in all).
- P3.1 carry-overs: the header keeps "Features (8)" with the error icon; failed rows end in a red
  ⓘ that shows the reason too; `course_ps_kernel_extrude` 07/08 show the kernel itself refusing a
  depth below its modelling tolerance, with its reason; part edges are drawn 1.6 px with round
  joints, so arcs and circles are as dark as straight edges.
- Tests: `crates/cadrs_core/tests/naming.rs` (Control Arm: dimension change Ø20 → Ø24 and
  undo, depth changes and undo, reorder of the two extrudes; every sketch on a face, Use and
  Pierce link, stored edge name and stored vertex name resolves by its exact name to its own
  counterpart (same end, same side, same x: a top/bottom or ±y swap fails); geometric fallback
  for a redrawn curve; lost references; the v3 fixture). Scenarios: `course_ps_edge_hover`,
  `course_ps_face_names`.
- Not done: the Fillet/Chamfer dialogs (P3.6) and "Create selection" (P3.7) that will use the
  edge picking and tangent chains; the names of faces made by fillets are hashes of the input
  edge's name (untested until P3.6); no reorder UI yet (a `MoveFeature` command exists).

**P3.3 Extrude complete** details:
- **Add / Remove / Intersect**, with the automatic tab, Merge with all and Merge scope.
- End types **Up to next / face / part / vertex** with offset, and **Through all**.
- Symmetric, Second end position, Starting offset and Direction.
- Surface and Thin tabs.
- Whole-sketch input from the list, and planar faces as input.
- The **eye** on sketch rows.
- The **Boolean** feature: Union, Subtract, Intersect, Keep tools, with the properties taken from
  the first tool.
- A Parts-list context menu: Rename, Hide/Show, Isolate, Delete.
- The **Mass and section properties** panel: Parts to measure, Volume, Surface area, Center of
  mass, and the inertia tensor once P3.5 adds density. It follows the workspace units.

**P3.3 status (2026-09-27): delivered.**
- Kernel: `Kernel::extrude_with` (`ExtrudeSpec`: Solid/Surface/Thin, every end type with
  offsets, Symmetric, second end, starting offset, direction, faces as input), `split_solids`,
  `solid_count`, `bounding_box`, `ray_hits`; thin walls as exact 2D bands
  (`cadrs_kernel::thin`); free edges of surfaces named. Fork commit `b45eb6d` (sub-shapes,
  compounds, thickening, rays, boxes, wire sweeps). 36 conformance cases (10 new).
- Rebuild: parts are carried through the feature list with their ids (New/Add/Remove/Intersect,
  merge scope, the Boolean feature, Delete part; a split part keeps its id on its largest
  piece); `Build::contacts` for the automatic Add; renames and hidden parts outside the
  feature list. Display meshes of bodies whose faces come from several extrudes, with
  silhouettes trimmed to the face.
- App: the complete Extrude dialog (`extrude_dialog.rs`: one active selection field at a time,
  multi-row selection lists from `cadrs_ui::SelectionList`), the Boolean dialog, the Parts list
  menu (`parts_list.rs`), the Mass and section properties panel (`mass_props.rs`), the sketch
  eye.
- P3.2 carry-overs: the selection readout is off (a `selection-readout` scenario command turns
  it on for the naming scenarios); failed sketch rows end in the ⓘ with their reason; the ⓘ is
  the filled red glyph; a depth the kernel refuses keeps the dialog open with the Depth field
  and title red (Enter doesn't accept it); hovered edges are 3 px in a more saturated orange;
  `references_survive_edits_when_extrude_2_adds` (Extrude 2 as Add: names cross a boolean).
- Tests: `cadrs_core/tests/part_studio.rs` (Control Arm self-check V = 368 749.705, A =
  50 179.71 to 0.01; two boxes Remove/Intersect/Add from the closed form; split parts; whole
  sketch; end types; merge scope; Boolean feature; surface and thin; face input; failed booleans;
  combined mass; the Control Arm sketch fully defined). Scenarios `course_ps6_control_arm`,
  `course_ps4_end_types`, `course_ps5_boolean`, `course_ps1_whole_sketch`,
  `course_ps2_parts_list`, `course_ps4_surface_thin`.
- Not done: Draft (not scheduled), surface booleans and Up to ends for surfaces, the Curves
  group, Boolean offset and tool reordering, the Face tab and inertia of Mass properties (P3.5),
  thin walls along ellipses, picking sketch lines or mate connectors for Direction.

**P3.4 Revolve** details:
- The Revolve feature (Shift+W): Solid, Surface and Thin; axis from a line, edge, cylindrical face
  or circular edge.
- Full, Blind, Symmetric, Up to …, and Second end.
- Booleans.
- **Diametral dimensions** to a centreline in sketches.
- Use and sketch on the faces and edges of revolved bodies.

**P3.4 status (2026-09-27): delivered.**
- Kernel: `Kernel::revolve_with` (`RevolveSpec`: Solid/Surface/Thin; Full, angle, Symmetric, Up
  to next/face/part/vertex with offset angles, second end), `FaceInfo.axis`,
  `EdgeInfo.circle`. Fork commit `df2e7c4` (shape and wire revolves with history, face axes,
  edge circles). 42 conformance cases (5 new, among them the torus 2π²Rr²).
- Core: `FeatureKind::Revolve` (`AxisRef`, `RevolveType`), `AddRevolve`/`SetRevolve`, the
  revolve in the rebuild (shared regions and booleans with the extrude), `Build::axes`;
  revolve geometry for faces' frames, meridian rulings and `SurfaceGrid`s (torus silhouettes);
  `SolidEdge.circle` (Use projects exact circles); a whole sketch ignores imprinted edges; a
  sketch on a face that a later feature covers is not lost.
- Sketch: `DimensionKind::Diametral` (proposed when a point or parallel line is dimensioned to a
  construction line with the label across it; solved as twice the distance; "Ø").
- App: the Revolve dialog (`revolve_dialog.rs`) and session (`revolve.rs`, sharing the extrude's
  session), the angle arrow, sketch-curve picking for the axis, Shift+W, the feature row's icon,
  mass panel in inches.
- P3.3 carry-overs: the extrude arrow in Add previews (the added body's cap), along the Direction,
  for every end type (a drag makes it Blind) and on surfaces (their far edges); an open sheet's
  preview is two-sided; a selected part is the saturated orange of `ex1-step6`/`ex2-step10`
  (shaded), no tan wash; the Boolean dialog highlights its tools and targets and shows Subtract's
  Offset (disabled); Isolate no longer greys the other parts' rows; accepted-sketch dots pulled in
  front of their fill again; `course_s21_imprinting` 04 claimed (one part, from the automatic
  Add).
- Tests: `cadrs_core/tests/revolve.rs` (types and torus, Remove/Surface/Thin/Up to part, the
  Reducer Coupling V 53.932 in³ / A 248.367 in² to 1e−3 and its single-revolve variant, Use of
  the revolve's circle, a cone's silhouettes), `dimension::tests::diametral_to_a_centreline`,
  `revolve::tests::dragging_the_angle_snaps_and_flips`. Scenarios `course_ps8_reducer_coupling`,
  `course_ps7_revolve_types`.
- Judge round 1 (8.3): disabled checkboxes greyed (label and box) in `cadrs_ui`; ps7 frames for
  the drag arrow (hovered, dragging), Second end, Thin, Surface, the axis from a cylindrical face,
  a circular edge and a straight edge, Up to face; planar faces as revolve input; a selected
  part's edges dark amber; face sketch planes grow to hold their geometry; a whole sketch reads
  "Face of Sketch N"; the extrude preview's far-cap edges no longer cover the region outline in
  a normal view.
- Not done: mate connectors as an axis, the Boolean Subtract
  offset (needs a kernel offset), Use of a torus's silhouette (only straight silhouettes), Up to
  ends for surface revolves.

**P3.5 Appearance, material, mass units** details:
- Mass units in Workspace units (kg, g, lb, oz).
- The 8-colour palette, and the Edit appearance dialog (swatches, mixer, hex/RGB, transparency).
- Face and feature appearances, and sketch appearance.
- The Appearances panel.
- **Assign material**: a bundled library (Onshape-like names and published densities) plus
  custom materials.
- Mass, CoM and **inertia** in the panel. This needs `MassProperties` to gain an inertia tensor
  and an OCCT `GProp` binding.

**P3.5 status (2026-09-28): delivered.**
- Kernel: `MassProperties.inertia` (unit-density tensor about the centroid) and
  `inertia_about`; fork commit `7278a45` (`GProp_GProps::MatrixOfInertia`); conformance case
  `inertia_tensor` (44 cases).
- Core: `appearance` (`Appearance`, the palette, swatches, face > feature > part), `material`
  (`Material`, the library with sources, search), `PartProps.{appearance, faces, material}`, the
  Part Studio's feature/sketch `appearances`, `Document.custom_colors`, `Part::palette`,
  `parts::mass_report`; commands `SetPartAppearance`, `SetFaceAppearance`,
  `SetFeatureAppearance`, `SetPartMaterial`, `SetCustomColors` (all undoable, saved; covered by
  `undo_and_persistence`); `cadrs_sketch::units::MassUnit`.
- UI (`cadrs_ui`): `ColorSwatch`, `ColorMixer`, a swatch row in `Menu`, per-item red in
  `SelectionList`, press-to-set on `Slider`, `InlineEditHide`, `LastPointerButton`.
- App: `appearance.rs` (Edit appearance dialog with live preview, Appearances panel),
  `material_dialog.rs`, the mass panel's mass/CoM/inertia, the units dialog's Mass units, the
  parts shaded per face in their appearances (thumbnails too), sketches in theirs.
- P3.4 carry-overs: Add/New previews in the part's colour, slightly translucent (0.82), the Add
  preview in the colour of the part it joins, existing parts opaque; a dialog's edge references
  (the revolve axis, an extrude direction edge) get a dark core so they read on a selected face's
  border, and an extrude's Direction sketch line stays highlighted; drag arrows on the second end
  of an extrude and a revolve (`extrude-arrow-2`, `revolve-arrow-2`); a Surface's region reads
  "Curves of Sketch N" (revolve and extrude); right-clicking a part row doesn't select it; the
  rename field keeps its 150 px (the row's eye toggle steps aside while editing); PS8.9 has its
  scenario `course_ps8_single_revolve`.
- Tests: `cadrs_core/tests/material.rs` (steel cube, Control Arm in Polypropylene, two
  materials, palette), `appearance::tests`, `material::tests`,
  `material_dialog::tests::polypropylene_reads_like_the_course`,
  `commands::tests::workspace_units_are_undoable_and_saved`. Scenarios `course_ps9_appearance`,
  `course_ps10_material`, `course_ps8_single_revolve`.
- Judge round 1 (8.4): the part menu's swatch row removed (not in Onshape's menu); a right-clicked
  part row gets a light context band; the material dropdown closed with its search inside (as
  `ex4-step17`), "Psi"; the Center of mass and inertia Overrides filled grey while there is no
  mass (`ex1-step6`); custom colour Update/Delete and the panel's double-click in frames; Make
  transparent… (a view state); curve appearance; the Variables panel (placeholder) and greyed
  Custom tables/Configurations; the Hex field lets go of the focus on Enter; a selected warm
  (sand, tan) part turns a deeper orange, and a selected warm face's outline gets a dark core;
  `course_ps9_appearance` 16's caption fixed (now 22: one undo undoes the last appearance; the
  script's sketch is one Add sketch and one Edit sketch step, so a second undo removed its
  curves, as expected).
- Not done: custom material libraries, per-configuration appearance, the panel's Face tab and
  mate connector, Mass Override.

**P3.6 Fillet, chamfer, hole, shell, reorder** details:
- Fillet, Edge tab: Radius or Width, Distance control, tangent propagation, overflow, drag arrow.
- Chamfer: Equal distance, Two distances, Distance and angle; Offset or Tangent measurement.
- Hole: Simple, Counterbore and Countersink; Drilled, Clearance and Tapped with ISO and ANSI
  tables; Start from part or sketch plane; Through all, Blind, Up to next; live callout.
- Shell: inward or outward, Hollow, and a failure state.
- **Drag to reorder** in the feature list, with failure below a parent. Folders are enough to
  hold the stand-in's "Base Features".
- The Jackhammer stand-in fixture.

**P3.6 status (2026-09-28): delivered.**
- **The Jackhammer stand-in** (`cadrs_core::samples::gear_cover`, `fixtures/gear_cover_standin.cadrs`,
  regenerated by `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test gear_cover`; script
  command `gear-cover`). *Substitution*: Onshape's public "Jackhammer" can't be copied, so the
  Gear Cover is ours: a rounded rectangle 110 × 200, R20 corners, 40 high (Extrude 1; its bottom is
  the face the Shell removes); a slope z = 24 + 0.2(y + 200) cut over the front (Extrude 2, Remove:
  it stands in for "Edge of Loft 2", PS17.9: the slope meets the walls at 79° to 90°, so a Width
  fillet's radius varies); two Ø30 × 6 bosses on the top at (±30, −40) whose circular top edges are
  the "A" references (Extrude 3, Add); the six features in a closed "Base Features" folder;
  Aluminum - 380 (2760 kg/m³); mm and kg. The hole points go on the bosses (concentric with their
  edges) and at (±34, −100) and (±16, −16) on the flat top; the chamfer is on the bosses' top edges;
  the R1 fillet on the bottom rim. With the course's Onshape document out of reach, its numbers
  (160 818.108 mm³, 0.444 kg) can't be compared; the stand-in is checked against closed forms
  instead: the base body, the shell after the holes (Pappus for the rounded concave corners of the
  cavity round each hole and boss) and the chamfer, 189 140.949 mm³ before the fillets (to 1e−3);
  after them the Width fillet's removal lies within its analytic bounds (207.5–236.2 mm³: 226.2)
  and OCCT's volume matches the display mesh's to 1.5e−4 (9.3e−5); mass 0.521 kg, CoM X = 0.
- Kernel: `fillet_with` (Radius, Width, overflow), `chamfer_with` (Offset/Tangent, flips,
  overrides), `shell_with` (outward, hollow, walls that cross refused, the region shell when OCCT's
  thick solid can't drop a vanishing face), `FaceInfo.radius`; fork `8fcc94f`; 7 new conformance
  cases (51).
- Core: `applied` (Fillet, Chamfer, Shell, Hole features), `hole` (tables, sections, callouts),
  `Feature::parents` and the parent-below failure, folders, `MoveFeatures`, `SetFeature`,
  `CreateFolder`/`UnpackFolder`/`SetFolder`, `SetMaterialLibraries`; tests `applied.rs` (12-edge
  cube, width, chamfer, shell, counterbore, start plane, up to next, tapped, order, folders) and
  `gear_cover.rs`.
- App: `applied.rs` / `applied_dialog.rs` (the four dialogs, picks, references drawn on the parts
  before the feature, the fillet arrow, the error line), sketch point picks, `feature_folders.rs`
  (folder rows, drag to reorder), toolbar Fillet/Chamfer/Shell/Hole and Shift+F, the callout
  symbols (`cadrs_ui::symbols`).
- P3.5 carry-overs: the CoM glyph (a quartered circle) while Mass properties shows a mass; custom
  material libraries; side panels docked full height with the rail icon pressed; right-clicking one
  sketch curve highlights only it; the bordered library select (`Select::bordered`); the custom
  colour's menu opens down from the pointer, clear of the swatch rows and the "Custom" label;
  docked side panels sit beside the graphics area, which shrinks (the view cube stays in view);
  tooltips kept on screen; frames for the greyed
  rail tooltip, Make opaque and a warm face's selection outline (`course_ps9_appearance` 03c, 24–26).
- Judge round 1 (8.1): chamfer Tangent only where a face curves across the edge; Tangent
  propagation off refused on a partly picked chain; Full round, Conic and Curvature (above, with
  their limits); the gear cover's Sketch 4 through the sketcher with Concentric, Symmetric and
  dimensions; zoom to fit in `course_ps17_gear_cover` 03 and 07, a zoomed fillet frame (11b), the
  drop line mid-drag (07a) and the parent-below reason on the ⓘ (07b, which also fixed a rebuild
  cache key); ellipsis for long titles and rows; the Width fillet bounded analytically; the
  chamfer flip icon and the override preview; the fillet arrow standing off by its value; inch
  depths; the whole-sketch and merge-scope hole tests and frames; docked panels beside the view.
- Not done: Full round other than a flat face between parallel flat sides; Conic and Curvature on
  curved edges or faces, and blended corners where they meet; Start from selected plane and Up to
  entity holes; the Thread class checkbox and hole tolerances; chamfers with different distances
  per edge under Tangent measurement; folder rename and Add selection to folder (P3.9).

**P3.7 Reference planes, sweep, loft, split** details:
- The **Plane** feature with all 8 types, and named planes as sketch planes, directions and mirror
  planes.
- **Ellipse offset**, as a spline or an offset-curve entity.
- **Sweep** along 3D paths of sketch curves and model edges, with Thin.
- **Loft** with ordered profiles, the reorder handles, start/end conditions and magnitudes, point
  sections, and Thin.
- **Split** part.
- **Create selection** → Edges: Tangent connected.
- The Final button (needed for PS21.11). It can be delivered early from P3.9.

**P3.8 Patterns, mirror, mate connectors** details:
- Mate connectors, explicit and implicit, with K to toggle them.
- **Linear**, **Circular** and **Curve** patterns, with Part, Feature (Reapply) and Face types,
  Skip-instance dots, Centered and the second direction.
- **Mirror**: Part, Feature and Face. This needs reflection support in the kernel.
- Create selection → Faces: Pocket.
- Holes placed at mate connectors.
- Pattern instances inherit appearance.
- The Reflector stand-in fixture.

**P3.8 status (2026-09-28): delivered.**
- **The Reflector stand-in** (`cadrs_core::samples::reflector`, `fixtures/reflector_standin.cadrs`,
  regenerated by `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test reflector`; script command
  `reflector`). *Substitution*: Onshape's public Rocket Guidance Reflector can't be copied, so the
  plate is ours: a 182 × 182 rounded square (R36 corners), 45 high (Extrude 1); its curved bottom
  from a Revolve Remove of an R400 arc on Front (Sketch 2); a **Pattern Axis** Mate connector at the
  top face's centre; a **Feature Sketch** with two triangles and a rectangle; renamed "Reflector",
  orange, Aluminum - 1060, in a closed "Reflector Surface Features" folder; mm and kg. Differences
  from the course: Sketch 3's point is dimensioned 65 from the origin (26 from the stand-in's
  front edge) because the stand-in has no edge 26 above it to dimension from; the fillet edges
  are picked from turned views; the step-2 fillets aren't closed-form (bounded and checked against
  the display mesh). Every other step's volume drop is closed-form (quadrature for the curved
  bottom), CoM X, Y = 0 within 1e−6, **2.665 kg** (Onshape's 2.04 kg is for its own geometry).
- Kernel: `transform_motion` (reflections), `face_tool`, `classify`, `split_faces`,
  `FaceOrigin::Instance`; fork `c8cfc22`…`86000a6`, `506f409`; conformance 61 (4 new cases).
- Core: `mate` (connectors, implicit origins), `pattern` (Linear, Circular, Curve; Part, Feature
  with Reapply, Face; Skip instances; Mirror), holes at connectors, `Part.source` (PS9.6), Split
  types and options; tests `pattern.rs` (8) and `reflector.rs`.
- App: the pattern menu, Mirror and Mate connector buttons and dialogs, connector triads and K,
  hover dots for implicit connectors, instance dots, Create selection → Pocket; scenarios
  `course_ps22_skip_instances`, `course_ps26_mirror`, `course_ps27_reflector`,
  `course_x11_mate_connectors`, `course_ps18_split_options`.
- P3.7 carry-overs: a failing shell tints only the shelled body red (the faces made by the ops of
  its faces to remove) and keeps the faces to remove in the selection colour (funnel 08); the
  funnel's bead sketch fully defined (tangent, vertical, pierce) with the edited sketch's fill
  drawn translucent over parts (14); one sketch-plane outline (no second, larger box); "Features
  (N)" spaced from the icons when the error icon shows; plane labels move to another corner of
  their square when the first is off the view, behind a part or a region fill, or on another
  label, and hide only when no corner is clear (ps12 14 shows Plane 3's; funnel 05/07/09 keep them
  off the fills); the loft profile rows start with a ">" chevron (drawn; the rows don't expand);
  the Split dialog's Part/Face types and options (above).
- Not done: a mirrored part joined with Add keeps seam faces on the mirror plane (see KERNEL.md);
  the loft rows' chevrons don't open per-profile options; Create selection rules other than
  Tangent connected and Pocket; the Split Face type's projection options (Use sketch plane
  direction, Normal to target) for curve tools.

- Judge round 1 (**8.6**, 2026-09-28): passed. Minor deltas carried to P3.10: the reflector's
  top face grey in 01–02; the tapped hole's name should use the 20 mm hole depth, and its dialog
  lacks Tap type, Pitch "1.50 mm (Coarse)", Fastener fit, Thread class and the tapped-clearance
  row; Merge scope should default to the drilled part, and Extrude 2 should have Merge with all on;
  the mirrored pocket out of view in 12; picked fillet edges not highlighted in the preview (04,
  11); Offset distance inline on its checkbox row, and no Symmetric for Up to face; a Curve
  pattern dialog frame (PS25 has test evidence only); one pattern-preview style; the mate
  connector's Alignment and Owner part fields and implicit-connector hover dots; the centre of
  mass without a material; box-selecting skip dots. PS27.13's frame reads V 985 314.338 mm³
  (the test build 985 314.335; both within tolerance).

**P3.9 Feature-list management and rollback** details:
- A draggable **rollback bar**; editing a feature rolls back to it.
- The before/after **preview slider** in every dialog, and the **Final** button.
- **Show dependencies**, parents and children, with row highlighting.
- **Folders**: add selection, drag, unpack, delete.
- The **filter**: name or type, quoted names, the `:` prefixes and hover help.
- **Show regeneration times**.
- Suppress and unsuppress.
- **Search tools** (Alt+C).
- View cube **corners** give trimetric views (P6.4).

**P3.9 status (2026-09-28): delivered.**
- Core (`cadrs_core`): the Part Studio keeps `suppressed` features and the `rollback` bar
  (additive, serde-defaulted fields); `Element::active_features` is what is built (above the bar,
  not suppressed), used by the view, thumbnails, STEP export and the dialogs' checks. Commands
  `SetSuppressed`, `SetRollback`, `DeleteFolder` (`commands/list.rs`); new features are inserted
  at the bar; deleting a feature keeps the bar under the same features. `feature_list`: the filter
  language (`Filter::parse`/`matches`, `FILTER_HELP`), `type_label`, `parents`/`children`.
  `rebuild::Build::times` (per-feature regeneration times, kept with the cache). The bracket
  stand-in `samples::bracket` (script command `bracket`: a plate, ten bosses Extrude 2–11 in a
  closed "Bosses" folder, Fillet 1 R5, Fillet 2 R1). Tests: `feature_list` unit tests (4) and
  `tests/feature_list.rs` (6: rollback, suppression, delete folder, dependencies, times, the
  filter on the bracket).
- UI: `cadrs_ui::CommandPalette` (Search tools), `Tooltip::help` (the filter's help card),
  `TreeItem::strikethrough`/`trailing`; `cadrs_app::feature_list` (filter, rollback bar drag and
  ghost, Show dependencies with legend, regeneration times, the dialogs' before/after slider and
  Final visibility), `search_tools` (Alt+C), folder rename/delete/drop onto a name
  (`feature_folders`), feature menu (Show dependencies, Suppress/Unsuppress, Add selection to
  folder…, Roll history bar to here, Roll to end), view cube corners (`view_cube`).
- Scenarios: `course_ps3_folders` (13 frames), `course_ps3_filter` (8), `course_ps11_dependencies`
  (5), `course_ps13_rollback_final` (13), `course_ps2_search_tools` (8), `course_p6_cube_corner`
  (4), `course_ps2_regeneration_times` (3); goldens for each.
- *Substitution*: the course's filter/folder lessons use a 43-feature Part Studio that isn't
  public (video only, no screenshots), so the bracket stand-in stands in for it; PS3/PS11/PS13 are
  video lessons, so the frames follow the requirement text and `lesson-part-studio-interface.png`.
- Decisions: Show dependencies highlights direct parents and children (amber, green; blue stays
  the selection); rolled-back and suppressed features can't be edited (Edit… disabled);
  suppressed rows are greyed and struck through; the rollback bar is document state, so moving it
  is an undo step; the slider's left half is "before", right half "after" (knob at 70 % by
  default, as `ex1-step4.png`); sketch rows show no regeneration time; editing a sketch does not
  roll the studio back (unchanged, to keep the sketch workflows' frames); Esc in the filter clears
  it; right-clicking an unselected feature makes it the selection.
- Out of scope (niche; out of scope by user decision 2026-09-29), marked in the rows:
  configurations (PS9.8), image tabs (PS2.12), full-round/conic/curvature fillets beyond what
  P3.6 built (PS14.1, PS14.4), the detailed Versions and history panel (PS2.1).
- Not done: the features pane doesn't scroll (a very long list is clipped; P3F.3 made it scroll); `:variable` matches
  nothing until variables exist (P5.2; P3F.4 made it work); an Add with an automatic merge scope has no part-feature
  parent in Show dependencies; Search tools launches a dropdown tool button's menu rather than
  its first tool.
- Judge round 1 (**8.5**, 2026-09-28): passed; all deltas minor, carried to P3.10: implicit
  merge-scope and face/edge parents in Show dependencies (PS11.2 stays 🟡); Final drawn
  translucent blue with see-through edges (ps13 03) instead of opaque parts with the preview on
  top; a whole-row highlight when dropping onto a folder; Search tools to index dropdown items as
  tools; regeneration times for sketch rows and fixed times in golden runs; a clear (✕) button in
  the filter field and the match count on filtered folders; stronger Parents/Children legend
  swatches; real part and error facts in `filter_on_the_bracket`.

**P3.10 Part Studios remainder: features** details:
- **Draft** (PS4.9) as a feature (Neutral plane, faces to draft, angle, flip, tangent propagation;
  `BRepOffsetAPI_DraftAngle` in the fork) and as the extrude's Draft option (X3).
- **Hole**: Start from selected plane (PS15.6), Up to entity (PS15.7), tolerances in the callout
  (PS15.8), a small PEM table (PS15.5), the Thread class checkbox; the tapped hole dialog's Tap
  type, Pitch "1.50 mm (Coarse)", Fastener fit and tapped-clearance rows, and the name using the
  hole depth (P3.8 deltas 2–3); Merge scope defaulting to the drilled part (P3.8 delta 4).
- **Fillet** (PS14.6): variable radius (OCCT law), overflow and smooth corner options where OCCT
  offers them; asymmetric and partial as far as the kernel allows (else record why).
- **Boolean** offset and reorderable tools (PS5.5); **Revolve** axis from a mate connector
  (PS7.2); **Loft** Match tangent / Match curvature and non-planar profiles (PS20.1, PS20.4);
  **Sweep** regions like Extrude (PS1.6); Onshape's yellow **warnings** (PS11.1); fillet and the
  other features acting on several parts (PS18.3); X7's remaining items; X10.
- Picked fillet edges highlighted in the fillet preview, and no stray orange specks (P3.8 delta 6);
  Offset distance inline on its checkbox row and no Symmetric for Up to face (P3.8 delta 7).

**P3.10 status (2026-09-29): done, judge round 2 8.5 (r1 8.0); awaiting hands-on check.**
- Fix round 1 (the judge's deltas):
  - Typed values lost (x7 02's 1 kg, the reflector's tapped depth 10 mm, ps15_hole 08). Root
    cause: `cadrs_ui`'s blur commit (`commit_on_blur`, PostUpdate) decided "was it edited" by
    comparing the field's text with its `NumberFieldState`, and it wasn't ordered against
    `sync_number_fields`. (a) When the sync ran first, it put the state's old text back into the
    field it had just left, so the typed value was never committed (the order is an ambiguity
    the scheduler resolves per build: the merge of main changed it). (b) A field only passed
    through (Tab into the hole's Tap clearance, then a click on the title) had its state updated
    by the dialog in `Update` of the frame the click cleared the focus, so the stale 6.667 in
    the field no longer equalled the state and was committed, and Tap clearance set the tapped
    depth back to 10. Now a field commits on blur only if its text differs from what it showed
    when it gained focus, the blur commit runs before the sync, and Esc reverts and leaves the
    field (tests `leaving_a_field_commits_what_was_typed`, `leaving_an_untouched_field_commits_
    nothing`). ps15_hole's third extrude: its region at (120, 25, 0) was outside the plate's fit
    (the click landed on the render-mode buttons), so the scenario zooms to fit first. The
    harness has `ExpectText(name, text)` (the reflector 08, x7 02, fillet 04, the profiles
    scenario check theirs).
  - Boolean: tools ghosted, Faces to offset blue, top views (PS5.5); fillet Variable entries
    (PS14.6); the Mass properties tabs and reference glyph (X7); hole Tip angle dropdown, Offset
    flip, tolerance units, nested rows, select values cut with "…" (PS15.6–15.8); a click in the
    view or Esc leaves a dialog's number field; picked fillet edges drawn over everything, whole
    (`ex5-step4.png`); the revolve's connector glyph on hover and its axis (PS7.2); no Area
    readout under an applied dialog's profiles; the select label column 80 px ("Measurement"
    ran into its value); ellipses at word boundaries; evidence frames `course_p310_profiles`
    (PS1.6, PS20.1) and `course_ps4_draft` 05 built.
- Kernel (additive, crates/cadrs_kernel/README.md "Trait changes in P3.10"): `draft` (`BRepOffsetAPI_DraftAngle`),
  `offset` (`BRepOffset_MakeOffset`, per-face distances), `fillet_variable` (a law per edge),
  `FilletProfile::Asymmetric`, `LoftCondition::MatchTangent`/`MatchCurvature`, non-planar face
  sections (sewn caps). Fork commit `f7d8eba` on `cadrs` (draft, offset, face derivatives,
  sewing, per-sample loft derivatives and quintic columns), pinned in Cargo.lock. Conformance:
  67 cases (6 new: `draft_cube_sides`, `offset_solids`, `fillet_variable_radius`,
  `fillet_asymmetric`, `loft_match_tangent`, `loft_from_a_curved_face`).
- Core: `FeatureKind::Draft` (`cadrs_core::draft`), `ExtrudeFeature.draft`, the hole's Start
  from selected plane / Up to entity / Offset / tolerances / Thread class / Tap type / tap
  clearance / PEM® table (`cadrs_core::hole`), the default Merge scope (the part the first hole
  drills), the fillet's Asymmetric and Variable options, the Boolean's Subtract offset, loft Match
  conditions and non-planar faces, warnings (`Build::warnings`: PS11.1), `mass_report_with`
  (Override, reference frame), `SolidFace.area`. Tests: `tests/part_studio_options.rs` (13, every
  expected value derived in its comment), `hole::tests::tolerances_and_pem`, the updated
  `course_sizes` and `tapped_and_clearance_tables`.
- App: the Draft dialog (`cadrs_app::draft_ui`), the extrude's Draft row and inline Offset
  distance (no Symmetric for the "Up to" ends), the Hole, Fillet and Boolean rows above, the
  revolve axis's mate connector button, amber warning rows with a glyph and tooltip, the Mass
  properties Face tab, Override and reference connector, picked fillet edges drawn whole over the
  preview (a stronger depth bias: no specks), the reflector's Extrude 2 with Merge with all.
- Scenarios (goldens added): `course_ps4_draft` (5), `course_ps15_hole_options` (7),
  `course_ps14_fillet_options` (6), `course_ps5_boolean_options` (5), `course_ps20_loft_match`
  (5), `course_ps7_revolve_connector` (3), `course_x7_mass_options` (4); `course_ps15_hole`
  08–09 and `course_ps27_reflector` 03 updated for the default Merge scope and Merge with all.
- Decisions: a positive draft angle leans the faces in towards the material along the pull (a
  boss narrows as it rises; the extrude's Draft likewise, its flip leans out); Symmetric and
  second-end extrudes draft each end away from the sketch plane; the Draft's default angle is
  3° (Onshape's); the hole's default Merge scope is the part the *first* hole drills, filled once
  when the rebuild knows it (so a later point off that part is a warning, not a cut); a hole
  started from a selected plane inside a part is buried; the tapped hole's Fastener fit shows
  "None" (Close/Normal/Loose listed disabled); the tap clearance is in threads and editing it
  sets the tapped depth; tolerance sections are checkbox rows (on: Symmetrical ±0.10, 2
  decimals), the callout writing "5 mm ±0.10", "5 mm +0.10/−0.05", "5.10/4.90 mm", "5 mm MIN",
  "5 mm MAX", "[5 mm]"; the variable fillet's vertices and points are picked on the parts before the
  fillet (the view rolls back while those fields take picks, as the chamfer's overrides do); a
  point on an edge starts halfway with the fillet's radius; Smooth transition off samples a
  linear law; the Mass properties Override and reference frame are panel settings, not saved
  (nothing in the document changes; the inertia with a reference is about the centre of mass
  along the connector's axes); warnings use a dark amber name and the icon-rs `warning-filled`
  glyph (the Draft row uses icon-rs `draft`; no icon was missing).
- Not done, and why: the Draft feature's Parting line type and Reference entity propagation
  (`DraftAngle` drafts about a plane only); Partial fillet and Smooth fillet corners (OCCT can't:
  see KERNEL.md "What OCCT can't do here"); asymmetric fillets on curved edges or faces (the
  conic-section construction needs straight edges between flat faces); Match curvature with
  more than two profiles; loft Normal direction / Tangent direction (a picked vector) and sheet
  bodies as loft profiles; counterbore and countersink tolerances; the Mass properties centre of
  mass and inertia Overrides.

- Judge round 2 (**8.5**, 2026-09-29): passed; every round 1 delta was fixed and all remaining deltas are minor. Carried to P3.11: stray edge stubs at the corners of the Boolean preview (05, 06); the tolerance child rows not indented, and Thread class with no ">" chevron; scientific notation for near-zero mass values (reflector 13); the Faces to draft not tinted in the Draft preview; oversized datum planes in the loft frames, with the swept rod clipped behind the dialog (p310_profiles 02); the hole's Offset distance not inline on its checkbox; no radius labels in the view for the variable fillet; an off-palette face-selection tint (x7 04).

**P3.11 Part Studios remainder: carried deltas and audit** details:
- Audit every 🟡/❌ PS and X row: fix stale rows, then build what is missing, or mark it out of
  scope with a reason from the allowed list.
- The Parts list's Curves group (PS2.8, X6); the context toolbar (PS2.2); Custom tables (PS2.10).
- Show dependencies' implicit merge-scope and face/edge parents (PS11.2).
- The carried minor judge deltas, as listed in docs/PROGRESS.md's phase 3 log ("Carried to P3.10"
  entries) and in the P3.6–P3.9 judge notes above, including: Final drawn opaque with the preview
  on top; the folder drop-target highlight; Search tools indexing dropdown items; sketch
  regeneration times and fixed times in golden runs; the filter's ✕ and match counts; legend
  swatches; the reflector's grey top face; a Curve pattern dialog frame; one pattern-preview
  style; the mate connector's Alignment and Owner part fields and hover dots; the centre of mass
  without a material; the mirrored pocket in view; box-selecting skip dots.

**P3.11 status (2026-09-29): done, judge round 2 8.6 (r1 8.5); awaiting hands-on check. The course passes, apart from Smooth fillet corners, deferred to Final.**

**Final (2026-09-30):** Smooth fillet corners is built (PS14.6 ✅, see its row; fork `f65c3e1`), so the course passes with nothing deferred.
- Audit of the 17 🟡/❌ PS and X rows (P3.11 details above), each now ✅ or placed:
  - Built: **PS11.2** Show dependencies with what the rebuild knows (`feature_list::{parents_with,
    children_with}`: a feature that changed a part it doesn't name has the part's feature as a
    parent, via `Part::features`; an edge depends on both its faces' features; a fillet or
    chamfer on the features along its tangent chain, `Build::uses`); **PS15.8** the counterbore
    and countersink tolerances (`StyleTolerance`, `HoleSpec::style_tol`); **PS20.4** Normal
    direction and Tangent direction with a picked vector (kernel `LoftCondition::{NormalDirection,
    TangentDirection}`, core `LoftFeature::{start,end}_direction`, the dialog's Start/End
    direction fields); **PS2.10** the Custom tables panel.
  - Verified and restated (stale text): **PS2.2** (every tool the course uses is enabled; the
    greyed ones are taught by no phase-3 course), **PS4.12** (the extrude dialog has every row of
    `ex1-step3`/`ex1-step4`), **PS2.8/X6** (no feature makes curve bodies, so Onshape's Curves
    group, shown only when non-empty, can't appear).
  - Out of scope (niche; out of scope by user decision 2026-09-29): image tabs (PS2.12, PS17.2),
    configurations (PS2.10's Configurations, PS9.8), FeatureScript custom tables (PS2.10), the
    full-round/conic/curvature fillets beyond P3.6 (PS14.1, PS14.4).
  - Owned by another stage: PS2.9 (section, render modes) and PS2.11 (Measure, Analysis) → P3E.3,
    X14 → P3E.3 (stage 3E, Hands-On Test Drive); PS2.12's drawing tabs → P3C.1 and PS15.9 → P3C.3
    (stage 3C, Introduction to Drawings).
  - Not built: a whole sheet body as a loft profile (PS20.1: cadrs makes no bounded planar sheets
    for a course step to pick; a surface's face is a face profile).
- Per-row result, the 17 audited rows: ✅ 12 (PS2.2, PS2.8, PS2.10, PS4.12, PS11.2, PS14.1,
  PS14.4, PS15.8, PS17.2, PS20.1, PS20.4, X6; four of them with an out-of-scope part: PS2.10,
  PS14.1, PS14.4, PS17.2), ✅ for this course's part with the rest
  owned by another stage 3 (PS2.9, PS2.11 → P3E.3; PS2.12 → P3C.1), owned by another stage 2
  (PS15.9 → P3C.3, X14 → P3E.3). Whole table: ✅ 190 · owned by another stage 5 (3 of them ✅ for
  this course's part) · out of scope 1 (PS9.8) · 🟡/❌ 0.
- Carried judge deltas, fixed (scenario frames to look at):
  - P3.10 r2: no stray edge stubs at the Boolean preview's corners (an outlined part's edges in
    their own gizmo group at the plain edges' depth bias, no black edges under them, and an
    opaque solid's edges between two back-facing flat faces culled; `course_ps5_boolean_options`
    05, 06); the tolerance rows indented to their checkbox's label and Thread class with its ">"
    (`course_ps15_hole_options` 01, 03, 04a); near-zero mass values below 1e−9 of the part's size
    (or of the largest moment) show as 0.000, Onshape's own 1e−5-scale values stay scientific
    (`fixed_or_scientific_of`; `course_ps27_reflector` 13; units test); the Draft's faces to draft
    tinted on the drafted body (the kernel now keeps the drafted faces' names; `course_ps4_draft`
    01); Plane features sized to the parts' outline on them (as `ex4-step5.png`) and
    `course_p310_profiles` 02 reframed with the rod in view (`course_ps20_loft` 01–09); the hole's
    Offset distance inline on its checkbox (the extrude's `inline_option`;
    `course_ps15_hole_options` 05); the variable fillet's radii labelled in the view
    (`course_ps14_fillet_options` 03, 04); the face-selection tint at 0.6 of the selection amber
    #E8A838 (`course_x7_mass_options` 04; every face selection).
  - P3.9: Final draws the finished parts opaque with the edited feature's faces in the preview's
    translucent blue (`course_ps13_rollback_final` 03); dropping onto a folder outlines the whole
    folder row (`course_ps3_folders` 08); Search tools lists every variant of a sketch tool's ▾ as
    a tool and launches it directly, and the key that closes the search no longer reaches the
    sketch (`course_ps2_search_tools` 07, 08); sketches get regeneration times (their regions,
    `Build::sketch_times`) and a scripted run shows fixed times (`course_ps2_regeneration_times`
    02; test `regeneration_times_are_measured_per_feature`); the filter field's ✕
    (`TextInput::cleanable`) and "Bosses (10 of 11)" on filtered folders (`course_ps3_filter`
    03–08); saturated legend swatches with matching row stripes (`course_ps11_dependencies` 02);
    `filter_on_the_bracket` takes its part and error facts from the build (and fails a fillet to
    check `:errors`).
  - P3.8: the reflector's top face orange (a shown sketch on a face fills only what it draws,
    translucent; `course_ps27_reflector` 01, 02); both pockets in view in 12; a Curve pattern
    frame (new `course_ps25_curve_pattern` 01–04); one pattern-preview style (a feature or face
    pattern's and a face mirror's copies tinted as the preview, as a part pattern's are
    translucent parts; reflector 05, 09, 12); the Mate connector's **Alignment** (the primary axis
    along a picked direction) and **Owner part** fields (`course_x11_mate_connectors` 01, 01a) and
    a dot at every implicit connector of the hovered face (04a); box-selecting Skip dots
    (`course_ps22_skip_instances` 06, 07).
  - P3.6/P3.7: the fillet overflow frames framed on the plate (`course_ps14_fillet` 05, 06); a
    whole sketch's hole points marked (`course_ps15_hole` 08); the Hollow's void seen through the
    part made transparent (`course_ps16_shell_fail` 03a); a ghost of the dragged row
    (`course_ps3_folders` 06, 08).
  - Verified already fixed: the centre of mass stays blank without a material, as Onshape's
    (`ex1-step6.png`; `course_x7_mass_options` 01); "Features (N)" clear of the header icons with
    the error icon (P3.7; `course_x11_mate_connectors` 04a); the Mixer label uncovered
    (`course_ps9_appearance` 10); ellipses at word boundaries (P3.10).
  - Listed, not built: the loft profile rows' ">" chevrons open no per-profile options (Onshape's
    per-profile connections belong to its surfacing course, not this one); the Split Face type's
    projection options (Normal to target needs a curve-onto-face projection,
    `BRepOffsetAPI_NormalProjection`, which the fork doesn't bind; Use sketch plane direction is
    what the face split does).
- Kernel (crates/cadrs_kernel/README.md "Trait changes in P3.11"): the loft direction conditions; the draft's
  lost face names recovered. Conformance: 68 cases (`loft_direction_conditions` new).
- Tests added or extended: `implicit_parents_of_the_bracket`, `filter_on_the_bracket`,
  `dependencies_of_the_gear_cover`, `regeneration_times_are_measured_per_feature`
  (`feature_list.rs`); `loft_normal_and_tangent_direction`, `draft_feature_on_a_cube`
  (`part_studio_options.rs`); `tolerances_and_pem` (hole); `alignment_turns_the_primary_axis`
  (mate); the units' noise floor.
- No icon was missing from icon-rs.
- Goldens: the full suite (104 scenarios, 2 new: `course_ps20_loft_direction`,
  `course_ps25_curve_pattern`) was run; re-blessed for the changes above: ps13_rollback_final
  (Final), ps11_dependencies, ps20_loft_match (plane size), ps15_hole_options (new frames,
  indent, inline Offset), ps4_draft (tint), ps5_boolean / ps5_boolean_options (outlines, no
  stubs), ps6_control_arm / ps7_revolve_types / ps7_revolve_connector / ps8_reducer_coupling /
  x7_mass_options (the selected part's outline at the plain edges' depth; x7 04 the tint),
  sketch_on_face 06 (the face's outline region not filled), and, within tolerance but changed on
  purpose, ps3_folders, ps3_filter, ps2_search_tools, ps2_regeneration_times,
  ps14_fillet_options.
- **Fix round 1** (2026-09-29; the judge's 8.5 held, but PS14.6 failed the course pass rule):
  - **Partial fillet built** (PS14.6): kernel `fillet_partial` (crates/cadrs_kernel/README.md "Partial
    fillet": the whole edge's fillet's removed or added material cut to the slab between the
    planes square to the edge at the bounds, then cut from or added to the body; flat end faces
    at the bounds, as Onshape's); core `FilletFeature::{partial, partial_bound, partial_first,
    partial_second, flip_partial}` (`PartialBound::{Parameter, Length}`, `partial_range`,
    `set_partial_bound`, which keeps the bounds' places when the type changes); the dialog's rows
    (Boundary type, First bound with its flip, Second bound). `course_ps14_fillet_options` 06,
    06b; tests `partial_fillet` (core), `fillet_partial` (conformance).
  - **Smooth fillet corners: not built**, investigated: OCCT's three-fillet corner is the sphere or
    a `GeomFill_ConstrainedFilling` patch between the fillets' ends, and `BRepFilletAPI_MakeFillet`
    has no setback (`SetParams`, `SetContinuity`, `SetFilletShape` don't move the fillets' ends);
    a smoother corner needs the fillets cut back and an N-sided G2 filling
    (`BRepOffsetAPI_MakeFilling`, not bound in the fork). Its row stays disabled with a shorter
    reason (06c). The variable fillet's **Magnitude** is out of scope (Onshape's Curvature cross
    section; niche, user decision 2026-09-29), said on its disabled rows (05).
  - The Funnel (`course_ps21_funnel`), re-rendered and checked against `ex4-step*`: fixed the
    spout's depth arrow (17), which pointed up while the extrude went down (a face profile's
    extrude took +Z for its arrow; now the face's outward normal); the header's mass values
    brought to the panel's (18: 2.975 in³, 84.127 in², CoM Z −0.547; the course: 2.974, 84.098,
    −0.547) and 08's text to the P3.8 failure display. Left as they are: the plane labels drawn
    over the translucent loft preview (07, 09) and Sketch 2's offset curve showing through the
    rim's thin wall (05–07), both cosmetic.
  - Goldens for the five exercises (`course_ps6_control_arm` and `course_ps8_reducer_coupling`
    already had them; `course_ps17_gear_cover`, `course_ps21_funnel`, `course_ps27_reflector` new)
    and every other course scenario without one (`course_ps10_material`, `ps12_planes`,
    `ps14_chamfer`, `ps14_fillet`, `ps15_hole`, `ps16_shell_fail`, `ps18_split`,
    `ps18_split_options`, `ps19_sweep`, `ps19_sweep_planes`, `ps20_loft`, `ps22_skip_instances`,
    `ps26_mirror`, `ps8_single_revolve`, `ps9_appearance`, `course_p310_profiles`,
    `course_x11_mate_connectors`): 20 new, each rendered twice to the same pixels.
  - Deltas: `course_ps20_loft_direction` 01–03, the direction a Sketch 3 line leaning 14° (no fin
    at the square's corner; Tangent direction with End magnitude 0.2, no lift above the circle),
    the picked directions drawn as arrows on the first and last profile (`Build::arrows`), and
    after ✓ only Plane 1, a shown plane feature, stays drawn; `course_p310_profiles` 02, the half
    cylinder moved in to x = 40 so Plane 1, sized to the parts, is compact;
    `course_ps15_hole_options` 04b, started from the part so the countersinks' cones are in the
    plate; `course_ps13_rollback_final` 03, Final shown pressed (the Outline button's selected
    style: light blue, blue border and label) and the view zoomed to fit beside the dialog;
    `course_x7_mass_options` 04, a selected face shaded in the selection orange as a selected
    part is (`FaceSelection`; the amber wash over the part's colour read as pale tan), in every
    face selection; `course_ps14_fillet` 05 reframed, the Gear Cover out of view instead of
    peeking out under the dialog; the PS4.6, PS15.4 and PS27.13 row texts corrected (sketch lines
    and connectors are pickable directions; the Thread class checkbox exists; the panel's
    V 985 314.338 mm³ with the test's 985 314.335).
  - Re-blessed goldens (all intended): `course_ps14_fillet_options` (06–06c new frames),
    `course_ps20_loft_direction` (new directions, arrows), `course_ps15_hole_options` (04b),
    `course_ps13_rollback_final` (Final pressed, zoomed from 03 on), `course_x7_mass_options` and
    every scenario with a selected face (the face shading: `course_ps20_loft_match`,
    `course_ps4_draft`, `course_ps4_end_types`, `course_ps_edge_hover`, `course_ps7_revolve_types`,
    `course_ps_face_names`, `course_s1_new_sketch_context_menu`).

- Judge round 2 (**8.6**, 2026-09-29): passed. All round 1 fixes were verified, and the face-selection shading change caused no regressions. Course pass rule: (b) exercises and (c) self-checks pass; (a) passes with PS14.6 Smooth fillet corners deferred to the Final stage. Minor deltas carried to Final:
  - PS20.1: add a test lofting from a surface extrude's face; the row's "no bounded planar sheets" reason is wrong.
  - Selected faces need an outline in the selection colour; it is weak on orange parts (reflector 12/12a vs ex5-step12) and missing in x7 04 and ps_face_names 01 (ex3-step3).
  - The partial fillet's bound markers in the view (fillet_options 06/06b).
  - The picked sketch line should be highlighted while the loft direction field is active (loft_direction 01/02).

Not scheduled in phase 3:
- the full-round, conic, curvature and variable fillets (PS14.4, PS14.6);
- Draft (PS4.9), which needs a kernel draft op;
- the PEM hole type and tolerances (PS15.5 PEM, PS15.8);
- hole callouts in drawings (PS15.9);
- configurations (PS9.8);
- the Versions and history panel (PS2.1);
- sections and render modes (X14);
- image tabs (PS2.12).

## Risks and open questions
- **Kernel operations missing from the fork.**
  - `opencascade-rs` binds only a small part of OCCT. Shell (`BRepOffsetAPI_MakeThickSolid`),
    sweep with a 3D spine (`BRepOffsetAPI_MakePipeShell`) and loft with tangency
    (`BRepOffsetAPI_ThruSections` plus constraints) need cxx bindings added to our fork. So do
    variable, conic and chord-width fillets (`ChFi3d`), draft (`BRepOffsetAPI_DraftAngle`),
    inertia (`GProp_GProps::MatrixOfInertia`), and ray or distance queries (for Up to next and
    Start from part).
  - Each one costs a ~6-minute OCCT rebuild cycle. Batch the fork additions at the start of the
    milestone that needs them.
- **The trait API is short of what the course needs.**
  - `Extent` has no Up to …, Through all, offsets or draft.
  - `sweep` takes a 2D path.
  - `loft` has no options.
  - `transform` is an `Isometry3`, which can't mirror.
  - There's no surface-body or open-profile input, and no face input for extrude.
  - `MassProperties` has no inertia tensor.
  - There's no ray-cast or measure query.

  Extend the trait in P3.1–P3.3 before the features depend on it.
- **Persistent naming** is the hardest part to get right: the topological naming problem. OCCT
  reports `Generated`/`Modified` history per operation, but face splits (from booleans, fillets
  and patterns) and merged faces need tie-breaking rules. If naming is wrong, sketches on faces,
  Use links and fillet edge lists silently jump to other geometry after an upstream edit. Mitigation:
  - geometric fallback matching, the way `RegionRef` already does with its seed point;
  - an explicit "lost reference" error rather than guessing;
  - a regression suite of edit-then-resolve cases.
- **Migrating the existing documents.** Saved documents reference `FaceTag`/`EdgeTag` from the
  prism mesh (`PlaneRef::Face`, links, imprints). Schema v4 needs a lossless mapping, or else
  those references get marked broken.
- **Windows build.** The OCCT cross-build with MinGW (`x86_64-pc-windows-gnu`) is unverified
  (KERNEL.md). It involves static linking of C++ with the MinGW libstdc++, exe size, and a cold
  build time added to every release. Verify it in P3.1 before building on it. The fallback is an
  MSVC build on a Windows runner.
- **Rebuild performance.** Onshape rebuilds everything on every edit. OCCT booleans and fillets on
  curved bodies can take tens to hundreds of ms each. We need several things:
  - per-feature caching of `BodyId`s, rebuilding only from the first changed feature;
  - rebuilding off the main thread, so Bevy never stalls;
  - a cheap preview path for arrow drags. The prism mesh could stay for sketch-plane-only
    previews.
  - Regeneration times (PS2.6) become a real diagnostic.
- **Determinism for golden PNGs.** Tessellation depends on the tolerance and can differ between
  platforms and OCCT versions. Fix the tessellation parameters and compare scenario images with a
  pixel tolerance.
- **Matching Onshape (Parasolid).** Loft and sweep surfaces, and fillet corner blends, won't
  match exactly. The Funnel and Jackhammer self-checks need tolerances, and analytic checks where
  possible.
- **Standards data.** Hole tables (ISO 273 clearance and tap drills, ANSI) and material densities
  have to come from public sources, not from Onshape's library. Record the source per value.
- **Stand-in fixtures** for Jackhammer and Rocket can't be compared to Onshape's numbers. Their
  acceptance comes from our own analytic checks, so a judge can only score the workflow and the
  UI, not the numbers.
- **Open questions.**
  - The PS21.7 funnel direction (see above).
  - Do Surface and Thin bodies (PS4.10–4.11, PS19.3) belong in phase 3, or should they wait for a
    surface-body type in the kernel? This plan includes them in P3.3/P3.4. They can be cut
    without blocking any exercise, because no exercise uses them.
  - Is face pattern and face mirror (PS27.9, PS27.12) feasible in OCCT? If not, the fallback is
    a feature pattern of the equivalent features, which gives the same Reflector result.
