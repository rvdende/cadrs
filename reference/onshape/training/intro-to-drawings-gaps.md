# Introduction to Drawings: gap analysis (2026-09-27; re-audited 2026-09-29)

This maps [intro-to-drawings.md](intro-to-drawings.md) against the cadrs code. First written on
branch `phase3` (`ff7616b`) on 2026-09-27; re-audited on branch `phase3c` on 2026-09-29 (the
stage-3C wrap-up). ✅ done · 🟡 partial · ❌ missing · **out of scope** (only cloud and multi-user
collaboration, sharing and permissions, release management and paid tiers, and Onshape account
and learning-site features). Related: [intro-to-part-studios-gaps.md](intro-to-part-studios-gaps.md)
(P3.1–P3.9), [intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md) (P3B.*: the property model
and BOM in P3B.6, standard content in P3B.5), and [crates/cadrs_kernel/README.md](../../../crates/cadrs_kernel/README.md).

**Summary:** cadrs has **no drawings** at all. What exists that the course touches:
- The tab bar "+" menu lists **Create Drawing…**, drawn **disabled** (`cadrs_app/src/document.rs:843`),
  and so is the tab menu's "Create Drawing of X…" (`:1858`). There is no drawing element type in
  `cadrs_core::ElementKind`.
- Tab context menu: Rename…, Properties…, Duplicate and Delete work; Copy to clipboard, Move to
  document…, Export… and the rest are disabled (`document.rs:1848-1930`).
- The sketch toolbar's "Insert DXF or DWG" is a disabled placeholder (`sketch.rs:1540`).
- Sketch dimensions, the dimension renderer (arrows, extension lines, Ø/R prefixes) and inline
  text editing exist in `cadrs_sketch`/`cadrs_app` (`sketch_dimension.rs`, `cadrs_ui/src/dim_edit.rs`)
  and can be reused for drawing annotations. Sketch text (T5) gives glyph outlines for notes.
- `cadrs_kernel` has tessellation and edge polylines but **no hidden-line removal or projection**
  query. The OCCT fork doesn't bind `HLRBRep`.

The course therefore needs a new element type (sheets, views, annotations, tables), a projection
and hidden-line service in the kernel, the part/assembly **property model** and **BOM** from
P3B.6, **hole data** from the Hole feature (P3.6), and **persistent naming** (P3.2) so annotations
survive model updates. PS15.9 (hole callouts in drawings), "not scheduled" in the Part Studios
plan, is picked up here (P3C.3).

**Cross-stage dependencies.** Version references and the tab menu's Change to version (D2.10,
D13.1, X14) need document versions (P3D.3, now landed) and are built in stage 3G (P3G.1 version
references, P3G.2 Change to version); the tab menu's Move to document (D2.10, X1) was P3F.1 and is
now P3G.3 ([derived-and-linking-gaps.md](derived-and-linking-gaps.md)). Stage 3C is **passed** when P3C.1–P3C.8 are done **and** P3D.3 and P3F.1 have landed; the
orchestrator may pull them forward.

**Counts (98 D IDs + 14 X IDs + 3 check/survey rows = 115), after P3G.3:** ✅ 111 ·
🟡 0 · ❌ 0 · out of scope 4 (D: 97 / 0 / 0 / 1 (D3); X: 14 / 0 / 0 / 0; P3G.3 (2026-09-29) moved
D2.10 and X1 to ✅ (Move to document); P3G.1 (2026-09-29) moved D13.1 and X14 to ✅; quiz, survey and
"compare with the public document": 3 out of scope). P3C.8 moved D4.12 and D6.4 to ✅; P3C.5
moved D1.2, D11.*, D12.*, D14.7, D14.8, X3 and X10 to ✅; the wrap-up moved D14.1 (Document
Copy… exists, as D8.1) and X6 (Show part intersections) to ✅. The 2 🟡 rows (D2.10, X1) are blocked on
another stage's feature (below); every ✅ row cites a scenario frame or a test that exists
(checked by script against the scenarios' Screenshot names, 2026-09-29). **After stage 3E
(2026-10-02):** unchanged, ✅ 111 · 🟡 0 · ❌ 0 · out of scope 4; D2.2's mouse-mapping half, left
to P3E.3, is checked against the preference it built (`course_td_mouse_prefs` 08b–12).

**Out of scope** (only these count): cloud and multi-user collaboration, sharing and
permissions, release management and paid tiers, Onshape account and learning-site features, and,
by user decision 2026-09-29, configurations, image tabs, full round/conic/curvature fillets and a
detailed Versions panel.

## Cross-stage items pending
The course passes on this branch except for these, which need a feature another stage builds:

| Row | What | Blocked on |
|---|---|---|
| D2.10 | Tab menu: Change to version | done (P3G.2, `course_er_change_to_version` 11–13) |
| D2.10, X1 | Tab menu: Move to document | done (P3G.3, `course_tips_move_tab` 01–05) |
| ~~D13.1~~ | Views that reference a version | done in P3G.1 (2026-09-29) |
| ~~X14~~ | Version references (the rest of X14 is done) | done in P3G.1 (2026-09-29) |

## 1. Drawings interface

### D1 Introduction to drawings
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D1.1 | A drawing is a 2D sheet made from a part or assembly | ✅ | P3C.1. `course_drw_ex1_ujoint` 05 (a part), `course_drw_ex2_assembly` 07 (an assembly). |
| D1.2 | Drawing tab; "+" → Create Drawing…; also from an instance menu | ✅ | "+" and the tab menu (P3C.1; an Assembly tab's too, P3C.5); P3C.5: the assembly instance menu's "Create Drawing of <part>…" (the instance's part, by its Name; a standard content part is not called "Standard content") and "Create Drawing of <assembly>…", the assembly's empty-space menu, and the Parts list's "Create Drawing of <part>…" open the Create Drawing dialog on that reference; with No views Insert view opens on it (an assembly on the Assemblies tab, picked). Four views works for an assembly too (its placed occurrences' bounds). `course_drw_create` 01 and `course_drw_ex2_assembly` 04 ("+"), `course_drw_sheets` 01 (tab menu), `course_drw_ex2_assembly` 03c–03d (instance menu: "Create Drawing of Pan head machine screw 1/4-28 x 0.75…", the part, not its Standard content studio, and the dialog it opens). |
| D1.3 | Create Drawing dialog: Existing/Custom template tabs, source list, All/ANSI/ISO, table, Four views / No views, OK/Cancel | ✅ | P3C.1. Source list is local ("Built-in", "This document", "My templates", "Recently used"); "Shared with me" and company/team libraries are **out of scope** (sharing). `course_drw_create` 02–04, 10. |
| D1.4 | Built-in ANSI/ISO templates; custom templates by editing or uploading | ✅ | P3C.1. Company-wide template sharing is out of scope; local custom templates are in. `course_drw_create` 04–05 (built-in), 10–11 (custom). |
| D1.5 | Custom template Projection: first or third angle | ✅ | P3C.1 (setting), P3C.2 (placement). `course_drw_create` 10–11; `course_drw_four_views` 02 (first angle placed); `drawing_views.rs` L-block tests. |
| D1.6 | Sheet sizes ANSI A–E, ISO A4–A0, portrait variants | ✅ | P3C.1. `course_drw_create` 03–04, `course_drw_sheets` 07–08; `cadrs_drawing/tests/template_sizes.rs`. |
| D1.7 | After OK, Insert view starts at once | ✅ | P3C.2: OK (No views) opens Insert view at once; Four views places the views instead. `course_drw_ex1_ujoint` 03–04, `course_drw_views` 02. |
| D1.8 | Border with zone labels; title block bottom right | ✅ | P3C.1. `course_drw_create` 05–06. |
| D1.9 | Parametric title block (scale, projection, drawn/approved, dates, title, size, number, revision, sheet n of m; dashes when empty) | ✅ | P3C.1 (fields), properties from P3B.6. `course_drw_create` 06, `course_drw_insert_dxf_image` 20. |

### D2 Drawings interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D2.1 | Sheet on a grey background | ✅ | P3C.1. `course_drw_create` 05. |
| D2.2 | Left click selects, wheel zooms, right/middle drag pans, no rotate; mouse mapping from preferences | ✅ | P3C.1 (`course_drw_create` 06–07). **P3E.3**: the mouse mapping is a local account preference (Preferences… in the account menu; Onshape, SolidWorks and other presets), and the sheet follows it: with SolidWorks a middle drag pans and a right drag does nothing; back on Onshape a right drag pans again; never a rotate (`course_td_mouse_prefs` 08b–12; `preferences.rs`: `sheets_pan_with_the_rotate_and_pan_gestures`, `preferences_round_trip_through_the_store_root`). |
| D2.3 | F fits the sheet | ✅ | P3C.1. `course_drw_create` 08. |
| D2.4 | Drawing toolbar (update, views, dimensions, annotations, note, callout, table, BOM, centerline, centermark, virtual sharp, line, spline, DXF/DWG, image) | ✅ | P3C.1 (toolbar with every group; items enabled as their milestones land: P3C.2–P3C.8). P3C.7: sheet sketch Line and Spline, Insert DXF or DWG and Insert image work (`course_drw_insert_dxf_image`). |
| D2.5 | Drawing properties panel (Units and precision, dual units, zeros, …; update from template; lock) | ✅ | P3C.1. `course_drw_sheets` 14–16, 22–24. |
| D2.6 | Per-annotation override of drawing defaults | ✅ | P3C.3: the dimension palette sets one dimension's prefix/suffix, tolerance, precision and dual units over the drawing properties (`course_drw_dimension_palette` 04: dual on one dimension only). |
| D2.7 | Sheets flyout (Ctrl+S): sheets → reference → views → projected children; Insert sheet; double-click activates | ✅ | P3C.1 (sheets); P3C.2: sheet → reference → views → projected children, a view row selects it. `course_drw_sheets` 02, 10–13; `course_drw_views` 17. |
| D2.8 | Sheet properties: scale, size, border and zones, referenced object | ✅ | P3C.1. `course_drw_sheets` 05–08. |
| D2.9 | View properties: reference (with link), scale, sheet | ✅ | P3C.2: View properties (reference with a link to its tab, scale, sheet). `course_drw_views` 08–09. |
| D2.10 | Drawing tab with its icon; tab menu (Delete, Rename, Properties, Duplicate, Copy to clipboard, Change to version, Move to document, Export, …) | ✅ | Icon, Copy to clipboard, Open in new window done (P3C.1; `course_drw_sheets` 19–21); Export… done (P3C.7, `course_drw_export` 02: PDF, DXF, DWG, PNG, JPEG). Change to version… (P3G.2: a drawing tab's menu, its views of the workspace pointed at a version through the Reference manager, `course_er_change_to_version` 11–13, `course_drw_export` 02; `link_update.rs::a_drawing_reference_changes_version_pins_and_updates`); Move to document… (**P3G.3**: the drawing moves with the Part Studios it shows, or links back to one left here at an auto version, `course_tips_move_tab` 01–05). |

### D3 Self-check: drawing interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D3 | Hotspot quiz on the drawing interface | out of scope | Learning-site feature. The areas it asks about are D2 rows. |

## 2. Creating views

### D4 View creation
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D4.1 | Insert view dialog (reference, orientation, display state, scale, 4th option) + part/assembly browser | ✅ | P3C.2: Insert view card (reference, orientation, scale, "None" stub) and the Select a part or assembly browser (Current/Other documents, document and branch, Part Studios/Assemblies, search, type filters, studios with their parts). Display state appears with assemblies (P3B.8); Other documents and assembly rows are disabled until then. `course_drw_views` 02; `course_drw_ex2_assembly` 06 (Assemblies tab). |
| D4.2 | Reference a part, not a Part Studio; title block shows the part name | ✅ | P3C.2: a warning when a whole Part Studio is picked; the first view sets the sheet reference, so the title block shows the part name. `course_drw_views` 02–03; the part name in the title block: `course_drw_ex1_ujoint` 05. |
| D4.3 | Pick, set orientation and scale, click to place the base view | ✅ | P3C.2: the picked view follows the cursor (blue) and a click places it. `course_drw_views` 03. |
| D4.4 | Tool switches to Projected view; ortho or isometric by direction; Esc ends | ✅ | P3C.2: Projected view by the cursor direction (ortho or diagonal isometric), click another view to change parent, Esc or the tool button ends it. `course_drw_views` 04–07. |
| D4.5 | Projected views aligned to the parent, placed by first/third angle | ✅ | P3C.2: folds by the template projection; unit tests on the L-block (first and third angle). `course_drw_views` 07, `course_drw_four_views` 02; `drawing_views.rs::l_block_first_angle_puts_left_on_the_right_and_top_below`, `l_block_third_angle_puts_top_above_and_right_on_the_right`. |
| D4.6 | Auxiliary view from an edge | ✅ | P3C.2: pick a straight edge, then place the view folded about it. `course_drw_views` 18–20. |
| D4.7 | Sheet scale from the first view; inherited; changed in Insert view or View properties | ✅ | P3C.2: sheet scale from the first view (and follows it), projected views inherit the parent's, Insert view and View properties set it. `course_drw_views` 03, 09. |
| D4.8 | View context menu (hidden lines, tangent edges, shaded, threads, part intersections, display state, projected view, properties, order, BOM, switch to, move to sheet, align, clear, zoom, delete, suppress alignment, show sketches) | ✅ | P3C.2 (every item; Insert BOM (P3C.5), Display state (assemblies), Bring to front / Send to back shown disabled). P3C.8: **Show threads** works (tapped holes' major diameter as a thin 3/4 circle end on, thin lines along the thread from the side, dashed where hidden, solid where a section opens it; `course_drw_more_views` 02–04); the menu also offers Remove crop / breaks / broken-out section. |
| D4.9 | Shaded view with part appearances | ✅ | P3C.2: display-mesh triangles painted back to front in the part and face appearances, lit from the viewer, visible edges over them. `course_drw_views` 10. |
| D4.10 | Show / hide hidden lines (dashed) | ✅ | P3C.2: hidden edges dashed (unit test: a 20 mm cube with a Ø10 hole shows 2 dashed lines from the side). `course_drw_views` 12; `drawing_views.rs::cube_with_a_hole_shows_two_dashed_lines_from_the_side`. |
| D4.11 | Tangent edges: Hidden, Solid, Phantom | ✅ | P3C.2: Hidden, Solid, Phantom (long, short, short). `course_drw_views` 11–12. |
| D4.12 | Section, detail, broken-out, break, crop views (not taught) | ✅ | P3C.8: **Section view** (toolbar): a cutting line on a view (snapped H/V), the section folded out aligned on the side it is placed, the parts cut with the kernel (extrude + boolean subtract, no new kernel op) and the cut faces hatched ANSI31 (45°, 3.175 mm), the chain cutting line with arrows and letters on the parent, "SECTION A-A", hidden lines off. **Detail view** (the view group's ▾): a circle and a scale (2× the parent's), the parent clipped to the circle, "DETAIL B" / "SCALE 1:1", the circle and letter on the parent. **Crop view** (rectangle or closed spline), **Broken-out section** (closed spline + depth dialog, hatched inside), **Break view** (two break lines, the band removed, zig-zag break lines; dimensions across read the true length). All exported to PDF/DXF/images. `course_drw_section_detail` (Ex1), `course_drw_more_views` (bar); tests in `cadrs_core/tests/drawing_sections.rs` (tube hatch 600 mm², detail 2:1 reads Ø6, break reads 200, undo). |

### D5 Geometric annotations
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D5.1 | Annotation tools on the toolbar | ✅ | P3C.1/P3C.3. `course_drw_ex1_ujoint` 09–10, `course_drw_more_annotations` 02. |
| D5.2 | Centerline: point-to-point, line-to-line, circle (3 or 2 points); snaps; extend by dragging | ✅ | P3C.3: Centerline ▾ with the four modes; snaps to line ends and midpoints, arc ends and centres, circle centres; a selected centerline's end grips extend it (`course_drw_ex1_ujoint` 10–12). |
| D5.3 | Centermark on circles/arcs | ✅ | P3C.3: a cross at the centre, its arms out past the rim on small circles (`course_drw_ex1_ujoint` 09). |
| D5.4 | Virtual sharp (mark or edge extensions, set in Drawing properties) | ✅ | P3C.3: two lines' crossing, drawn as a mark or as edge extensions by Drawing properties → Virtual sharp (`course_drw_dimension_palette` 09). |

### D6 Dimensions
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D6.1 | Drawing dimensions are driven (reference) | ✅ | P3C.3: dimensions read the referenced edges' exact model geometry (named B-rep edges projected into the view), the 2D projection only for silhouettes; unit test `ex1_dimensions_and_callouts_match_the_model` (1e−6 in). `course_drw_ex1_ujoint` 16; `hand_brake.rs::ex3_values_after_update`. |
| D6.2 | Smart dimension D: linear, angle, diameter, radius from picks | ✅ | P3C.3: D; circle → diameter, arc → radius, line → length, parallel lines → distance, other lines → angle, points → horizontal/vertical/aligned by where the text goes; the sketch dimensions' look (filled arrowheads, extension lines with gap and overshoot, text in a break, Ø/R). `course_drw_ex1_ujoint` 15–16. |
| D6.3 | Orange snap points on hover | ✅ | P3C.3: the hovered edge's snap points as orange squares, the one under the cursor larger. `course_drw_ex1_ujoint` 15b. |
| D6.4 | Radial (Shift+R), Diameter (Shift+D), point-to-point, line-to-line, angular; baseline, ordinate, chamfer, arc length | ✅ | P3C.3: Dimension ▾ with radial, diameter, point-to-point, line-to-line and angular (Shift+R, Shift+D). P3C.8: **baseline** (a base and targets; further picks add stacked dimensions), **ordinate** (a zero point, then points; values lined up at one level, leaders jogged when values would touch), **chamfer** ("1.00 x 45°" from the Chamfer feature, else the edge's legs) and **arc length** (⌒, r·θ) (`course_drw_more_annotations` 02–07; `cadrs_core/tests/drawing_annotations_more.rs`). |
| D6.5 | Re-attach by dragging grips; blue attachment highlight; value updates | ✅ | P3C.3: a selected dimension shows its attachments in blue and its grips; dragging an attachment grip onto other geometry re-attaches it and the value follows live (`course_drw_dimension_palette` 06–08). P3C.6: dangling (red) annotations re-attach the same way (`course_drw_ex3_update` 21–23: the red 75.00 onto the large hole's centre reads 46.00, black). |
| D6.6 | Dimension palette: prefix/suffix, symbols, tolerance, precision, dual units | ✅ | P3C.3: the flyout button beside a selected dimension opens the palette (prefix, suffix, Ø ± ° ⌴ ⌵ ↧, tolerance none/symmetric/deviation/limits with upper and lower values, precision, dual units). `course_drw_dimension_palette` 02–04. |
| D6.7 | Hole callout from Hole-feature data; Edit → Prefix (`4x`); ⌴ ↧ Ø | ✅ | P3C.3: the callout text comes from the Hole feature's spec (the picked edge's faces name the Hole feature, PS15.9); right-click → Edit… opens the Hole callout card with Prefix and ✓/✗. ⌴ ⌵ ↧ are drawn as vector strokes (Inter lacks them). `course_drw_ex1_ujoint` 12b–14. |
| D6.8 | Black annotations, orange when selected or placing | ✅ | P3C.3. `course_drw_dimension_palette` 01, `course_drw_ex2_assembly` 14b. |

### D7 Adjusting views
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D7.1 | Projected views stay aligned; Suppress alignment with parent | ✅ | P3C.2: aligned views slide along the fold line; moving a parent carries its children; Suppress/Restore alignment with parent. `course_drw_views` 13–14. |
| D7.2 | Drag views to move | ✅ | P3C.2: drag a view; one undo step. `course_drw_views` 13. |
| D7.3 | Move to sheet… (annotations go, children don't); View properties → Sheet | ✅ | P3C.2: Move to sheet… and View properties → Sheet; children stay; the view's annotations go with it (they belong to the view, P3C.3). `course_drw_views` 21–23. |
| D7.4 | Show/hide sketches… (searchable checklist) | ✅ | P3C.2: searchable checklist of the studio's sketches, drawn over the view. `course_drw_views` 15–16. Final part 2: the sketch is drawn blue over the view's edges (it lay under the outline it traces, so 16 showed nothing). |
| D7.5 | Align view vertical / horizontal to an edge | ✅ | P3C.2: pick an edge after the menu item; the view turns by the smallest angle. `course_drw_views` 24–25. |

### D8 Exercise: Universal Joint Drawing (stand-in: `fixtures/ujoint_flange_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D8.1 | Make a copy | ✅ | P3C.3: the stand-in `fixtures/ujoint_flange_standin.cadrs` (`samples::ujoint`, script `ujoint-flange`); Document Copy… exists (`landing.rs`: the documents page's context menu → Copy…, `landing_many_documents` 19, 21; `store.rs::copies_are_undoable_library_steps`). |
| D8.2 | "+" → Create Drawing… | ✅ | P3C.1. `course_drw_ex1_ujoint` 02. |
| D8.3 | ANSI_A_INCH.dwt, No views, OK | ✅ | P3C.1. `course_drw_ex1_ujoint` 03–04. |
| D8.4 | Insert the part, Front, 1:2, place; Projected view stays on | ✅ | P3C.2, on the Ex1 stand-in in P3C.3 (`course_drw_ex1_ujoint` 04–05). |
| D8.5 | Top, Right, Isometric; iso 1:4 shaded; hidden lines + phantom tangent edges; title block shows the part | ✅ | P3C.3 on the stand-in: title block "Universal Joint Flange (stand-in)" / "Made by cadrs" (the part's name and description) (`course_drw_ex1_ujoint` 06). |
| D8.6 | Rename the drawing tab | ✅ | P3C.3 (`course_drw_ex1_ujoint` 07–08). |
| D8.7 | Centermarks, centerlines (edge to edge, extended), circle centerline | ✅ | P3C.3 (`course_drw_ex1_ujoint` 09–12). |
| D8.8 | Hole callouts with 4x / 8x prefixes | ✅ | P3C.3: `4x Ø.266 THRU ⌴Ø.438 ↧.250` and `8x Ø.266 THRU` (`course_drw_ex1_ujoint` 13–14; unit test). |
| D8.9 | The 10 dimensions, 3 decimals in inches, angles 1 decimal | ✅ | P3C.3: Ø4.750, 3.282, 2.061, 6.000, 2.600, 2.500, 43.0°, 120.0°, Ø1.750, Ø1.250 (the course's 10), each equal to the model's parameter to 1e−6 in (`course_drw_ex1_ujoint` 15–17; unit test). |

## 3. Notes and tables

### D9 Notes
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D9.1 | Note (N): free, or with a leader to an edge/point | ✅ | P3C.4: N or the toolbar; a click on sheet places a free note, a click on an edge or snap point first picks the leader's end (persistent name + point on the edge), then a click places the text, typed in place (`course_drw_notes` 01–05). |
| D9.2 | Add leader (several per note) | ✅ | P3C.4: right-click → Add leader; leaders follow the model when the view changes (`course_drw_notes` 06, 07, 12). |
| D9.3 | Rich text toolbar (bold, italic, underline, strike, align, lists, wrap ruler, symbols) | ✅ | P3C.4: note toolbar (B I U S, alignment, bulleted/numbered lists, text height, symbol menu), ruler with double-arrow wrap handles; Inter Medium/ExtraBold/Italic on the sheet, ⌴ ⌵ ↧ as vector strokes (`course_drw_notes` 02, 03). |
| D9.4 | Parametric notes: sheet reference properties and drawing properties; formats; dashes when undefined | ✅ | P3C.4: Insert sheet reference / drawing property cards (text case, date format, preview); fields resolve at draw time through `rich::ReferenceProperties` (P3B.6 plugs in part number etc.); undefined shows dashes (`course_drw_notes` 08–12). |
| D9.5 | Rotate, resize, double-click to edit | ✅ | P3C.4: rotation handle (5° steps), side handles set the wrap width, corner scales the text; double-click edits; orange when selected (`course_drw_notes` 13–17). |

### D10 Tables
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D10.1 | Table dialog: rows, columns, title row, header row, fixed corner | ✅ | P3C.4 (`course_drw_tables` 01, 02). |
| D10.2 | Typing fills cells; Tab / Shift+Tab; double-click edits in the note editor | ✅ | P3C.4 (`course_drw_tables` 03, 04, 16). |
| D10.3 | Cell toolbar: insert/remove rows and columns, merge, unmerge, format; Shift+click | ✅ | P3C.4 (`course_drw_tables` 05–09); merge/unmerge undo test in cadrs_core. |
| D10.4 | Grips: midpoint resizes from the fixed (black) corner, corner moves; Table properties | ✅ | P3C.4 (`course_drw_tables` 10–15). |

### D11 BOMs and callouts
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D11.1 | Insert BOM mirrors the assembly BOM set-up | ✅ | P3C.5: the rows are the assembly's own BOM (`assembly::bom::compute_with` with its saved columns, excluded rows and top-level row; `cadrs_core::drawing_assembly::bom_data`); the Item column reads "Item No." as the course's tables. Unit test: drawing rows == assembly rows for the three types and both orders (`cadrs_core/tests/drawing_assembly.rs`). `course_drw_ex2_assembly` 02–03, 10–11. |
| D11.2 | Insert BOM dialog: assembly, Flattened / Structured top / multi level, order, fixed corner; snaps to the border | ✅ | P3C.5: toolbar Insert BOM (and the view menu's "Insert BOM for <assembly>…"): the Insert BOM card (the assembly, BOM type, Order, the four fixed-corner buttons); the table follows the cursor in orange and snaps to the frame's matching corner within 8 mm; a click places it (one undo step). Bottom to top puts the header at the bottom, item 1 above it (`course_drw_ex2_assembly` 10, 11). |
| D11.3 | BOM Table properties; resize and format like a table | ✅ | P3C.5: right-click → BOM Table properties… (type, order, fixed corner; ✓ applies, the rows the workspace's for a new type); a BOM table is a `Table` with its `BomData`, so the midpoint/corner grips, cell toolbar and Table properties work; column lines drag where they meet the top edge (new `TableGrip::Column`, any table) (`course_drw_ex2_assembly` 12–13). P3C wrap-up: BOM Table properties shown (`course_drw_ex2_assembly` 11b–11e: Order → Bottom to top previewed live on the table, applied, then back to Top to bottom, 11e); cells wrap between words only and no column gets narrower than its longest word (`table.rs::cells_wrap_between_words_and_columns_keep_their_longest_word`); tables and BOMs have an opaque sheet-coloured background, so view lines don't show through them while placing or placed (`course_drw_ex2_assembly` 10–11; `views.rs::view_lines_are_cut_out_of_tables`). The BOM updates with Update (P3C.6 model: its assembly's dependency hash). |
| D11.4 | Callout dialog: border shape and size, text height, 5 text fields with component or BOM properties | ✅ | P3C.5: the Callout card at the top left (`ex2-step10.png`): the component property menu (Part: Name, Part number, Description, Material, Revision, Vendor) and the BOM table property menu (Table: Item No., Table: Qty.) insert `{Part: …}` / `{Table: …}` tokens into the field last typed in, shown as chips ("Part: Name", "x Table: Qty.") while the field isn't edited, the field back at its start after the menu (P3C wrap-up, `course_drw_ex2_assembly` 14, 14b; `input.rs::tokens_become_chip_runs`); upper, left, centre, right and lower fields; text height (drawing units); border Circle / Underline / Box / Triangle / None; size Tight Fit or 1–4 characters. Pick a part's edge in an assembly view (the leader's end, on the nearest visible edge), the callout follows the cursor, a click places it; again until ✓. Fields resolve from the drawing's copy of the assembly (occurrence properties) and the sheet's BOM tables; without a BOM table, Table fields read empty (`course_drw_ex2_assembly` 14–15). |
| D11.5 | Edit a callout | ✅ | P3C.5: right-click → Edit… opens the card on the callout, changed live (orange), ✓ applies one undo step. `course_drw_ex2_assembly` 16b–16d (the lower field given Part: Part number, shown as a chip; previewed; ✓; undone). |
| D11.6 | Inference lines while dragging callouts | ✅ | P3C.5: while placing or dragging a callout its anchor snaps (2.5 mm) to the vertical and horizontal through the other callouts' anchors, with dashed blue guides (`course_drw_ex2_assembly` 14c, 16). |

### D12 Exercise: Universal Joint Assembly Drawing (stand-in: `fixtures/ujoint_assembly_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D12.1 | Make a copy | ✅ | As D8.1: `Custom("fixture ujoint_assembly_standin")` copies the stand-in into the scenario's store (`course_drw_ex2_assembly` 01). |
| D12.2 | Assembly BOM panel: Add column Name, Move left | ✅ | P3B.6's panel: Add column → Name, then Move left three times (`course_drw_ex2_assembly` 02–03b). |
| D12.3 | "+" → Create Drawing… | ✅ | P3C.1. `course_drw_ex2_assembly` 04. |
| D12.4 | ANSI_A_INCH, OK | ✅ | P3C.1. `course_drw_ex2_assembly` 05. |
| D12.5 | Insert the assembly, Isometric, 1:2 | ✅ | P3C.5: Insert view → Assemblies → Universal Joint Assembly, Isometric, 1:2; every occurrence projected with its placement by one hidden-line removal (`cadrs_core::drawing_assembly`), Projected view ended with Esc (`course_drw_ex2_assembly` 06, 07). |
| D12.6 | Rename the tab | ✅ | As D8.6 (`course_drw_ex2_assembly` 08). |
| D12.7 | Shaded view; tangent edges Phantom | ✅ | P3C.5: the assembly shaded in each part's appearance (`course_drw_ex2_assembly` 09). |
| D12.8 | Insert BOM Flattened, top to bottom, top-right corner, snapped | ✅ | P3C.5 (`course_drw_ex2_assembly` 10, 11): the course's five rows (unit test: exactly `Item No. / Name / Quantity / Part number / Description` with 2, 1, 4, 4, 16). P3C wrap-up: the columns are sized to their content on insert (one line each up to 30 text heights: the table spans the frame as in `ex2-step8.png`; the Ex3 Name column is single-line as in `ex3-drawing.png`; `assembly.rs::bom_columns_fit_their_content`). |
| D12.9 | Resize with the left midpoint grip; column widths | ✅ | P3C.5 (`course_drw_ex2_assembly` 12–13): the left midpoint grip, then a column line (P3C wrap-up: the part numbers stay whole, "Quantity" is not split; the view sits left of the opaque table as in `ex2-drawing.png`, 17). |
| D12.10 | Callouts: Underline, `Part: Name`, right field `x` + `Table: Qty.` on 5 parts | ✅ | P3C.5 (`course_drw_ex2_assembly` 14–15; the wrap-up puts the Axle's leader on the axle and orders the anchors down the view like the labels, so no leaders cross, 17): "Universal Joint Flange x 2", "Universal Joint Centre Block x 1", "Universal Joint Axle x 4", "Pan head machine screw 1/4-28 x 0.75 x 16", "Graphite Phosphor Bronze Bushes x 4" (unit-tested). |
| D12.11 | Line the callouts up with inference lines | ✅ | P3C.5 (`course_drw_ex2_assembly` 14c, 16, 17). |

## 4. Updating a drawing

### D13 Updating a drawing
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D13.1 | Views reference the workspace by default (or a version) | ✅ | P3C.6: views reference the workspace; the drawing keeps the studio's state as of its last update (`cadrs_drawing::ModelSource`, a snapshot of the features) and each view the dependency hash of its part (`course_drw_ex3_update` 12d–15; `hand_brake.rs::the_dependency_tracker_marks_exactly_the_affected_views`). **P3G.1**: the Insert view browser's version graph (Current document) and Other documents tab place views of a **version**: the view references the version's frozen copy (`cadrs_core::external`, stored with the view in one undo step), so workspace edits never change it or make it out of date, while a workspace view still goes out of date and updates (`course_drw_version_reference` 03–09, 10–13 another document's V1; `external_refs.rs::a_drawing_view_of_a_version_is_unchanged_by_a_workspace_edit`: V1 view 25 high before and after, the workspace view 40 after Update). |
| D13.2 | No automatic update; gold Update icon; click or Ctrl+Q updates all | ✅ | P3C.6: editing the studio changes nothing on the sheet; the Update button turns gold while a view's part hash differs from the workspace's (tooltip: how many views are out of date); a click or Ctrl+Q regenerates every out-of-date view and re-measures its annotations in one undoable step "Update from this workspace" (`course_drw_ex3_update` 13–15). A change in another studio or another part leaves the views alone (unit tests). BOM tables: with P3C.5. |
| D13.3 | Dangling annotations drawn red; drag a grip to re-attach or delete | ✅ | P3C.6: an annotation whose persistent edge/face name no longer resolves is drawn red (dimensions, centerlines, centermarks, hole callouts, note leaders); only its dead references are frozen, exactly where they were (never re-attached to a guess), so the red dot (a dangling centermark is a filled dot), the red 75.00's dead end, the 2x Ø8.25 leader and the Top view's red centerline meet at one point as in ex3-step9; live references follow the model and a dangling dimension shows its last value (75.00); texts follow their geometry on Update (fix rounds 1–2). Selected, a dangling annotation is orange (D6.8) with its dead attachments red, its live ones blue. Selecting it shows its grips; dragging an attachment grip onto new geometry re-attaches it (the value updates, black again); Delete removes it; both undoable (`course_drw_ex3_update` 15–25). |

### D14 Exercise: Hand Brake Update (stand-in: `fixtures/hand_brake_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| D14.1 | Make a copy | ✅ | As D8.1: Document Copy… (`landing_many_documents` 19, 21; `store.rs::copies_are_undoable_library_steps`) copies a document; the stand-in (`samples::hand_brake`, `fixtures/hand_brake_standin.cadrs`) opens in the scenario's document (`Custom("hand-brake")`, `course_drw_ex3_update` 01). P3C wrap-up: the stand-in no longer gains the new document's default "Assembly 1" tab (the course's tab bar in `ex3-step8.png` has none; `course_drw_ex3_update` 29). |
| D14.2 | Open the drawing; Sheets flyout; double-click each of 3 sheets | ✅ | P3C.1; P3C.6: the Hand Brake Drawing's Sheets(3) flyout with parts and views under each sheet (`course_drw_ex3_update` 02–04). P3C.5: the Assembly sheet has the Hydraulic Brake Unit isometric and shaded, its 10-row BOM and balloons 1–10. |
| D14.3 | Handle studio → Edit Main Sketch | ✅ | `course_drw_ex3_update` 05. |
| D14.4 | Change 5 dimensions; delete the small circle; accept | ✅ | P3C.6: the fully defined Main Sketch (original values 225, 100, bar 20, offset 15, Ø8.25); 250, 125, 25, 25, Ø15 typed in; the small circle deleted (`course_drw_ex3_update` 06–09). |
| D14.5 | Extrude 2 Second end position 175 mm | ✅ | `course_drw_ex3_update` 10 (the stand-in's grip was 160, so 185 = 175 + 10 after). |
| D14.6 | Hole 1 → ISO Clearance counterbore M6 (6.6, ⌴11.25, ↧6) | ✅ | `course_drw_ex3_update` 11 (from M5: 5.5, ⌴9.75, ↧5). |
| D14.7 | Edit standard content instances → M6 | ✅ | P3C.5: the stand-in's Hydraulic Brake Unit (`samples::hand_brake::assembly`); the three ISO 4762 cap screws selected → Edit standard content instance… → Size M6 → Update → ✓ (P3B.5's dialog; `course_drw_ex3_update` 12b, 12c). |
| D14.8 | Drawing shows gold Update; click updates views and BOM | ✅ | P3C.6/P3C.5: the views (the Assembly sheet too, 12d) unchanged and the icon gold (13); its tooltip "7 views, 1 BOM table out of date" (14); Ctrl+Q updates every view and the BOM table in one step (15, 29, 30: the M6 heads, row 8 the M6 configuration). Unit test: only the assembly view and its BOM go out of date after D14.7, and after Update row 8's owner is the M6 configuration (its Description "… M6 x 16 …"). `course_drw_ex3_update` 12d–15, 29–30; `hand_brake.rs::d14_7_m6_cap_screws_update_the_assembly_sheet`. |
| D14.9 | Handle sheet updated; delete dangling centerline and `2x Ø8.25` | ✅ | P3C.6: 250.00, 25.00, R30.00, Ø25.40, 78.00, R16.00, 3x Ø5.50, 8.00; the centerline, `2x Ø8.25` (a diameter with the prefix "2x"), 75.00 and the centermark red; the first two deleted (`course_drw_ex3_update` 15–20). |
| D14.10 | Re-attach red 75.00 to the large hole → 46.00; delete dangling centermark; tidy | ✅ | P3C.6: `course_drw_ex3_update` 21–26. |
| D14.11 | Grip sheet: Ø25.00, 185.00, 35.00, 62.50, 8.00, 8.50, `3x Ø6.60 THRU ⌴Ø11.25 ↧6.00` | ✅ | P3C.6: `course_drw_ex3_update` 27, 28; each value unit-tested against an independent value (`cadrs_core/tests/hand_brake.rs`). |

## Self-checks and survey
| ID | Requirement | Status | Notes |
|---|---|---|---|
| Quiz | "Self-Check: Introduction to Drawings" | out of scope | Learning-site feature. |
| Survey | Completion survey | out of scope | Learning-site feature. |
| Compare | Compare with public "Completed Exercise" documents | out of scope | Needs Onshape's public documents (account feature). Replaced by structural checks in each exercise scenario. |

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Drawing tab element (+ Create Drawing, instance menu; Rename, Duplicate, Properties, Export, Move to document) | ✅ | Drawing element, tab menu, Duplicate, Copy/Paste done (P3C.1; `course_drw_sheets` 19–21, `course_drw_ex1_ujoint` 07–08); Export done (P3C.7, `course_drw_export` 02); the instance and Parts list menus' Create Drawing (P3C.5; `course_drw_ex2_assembly` 03c–03d); Move to document (**P3G.3**, `course_tips_move_tab` 01–05). |
| X2 | Templates (ANSI/ISO sizes, orientations, units; custom with projection; picker; Four views / No views) | ✅ | P3C.1; Four views placed by P3C.2. `course_drw_create` 02–05, 10–11; `course_drw_four_views` 01–02. |
| X3 | Parametric title block and notes bound to properties | ✅ | Title block (P3C.1) and notes (P3C.4); P3C.5: both read the property model (`cadrs_core::properties`, through `rich::ReferenceProperties` / `drawing_export::reference_props`): Name, Part number, Description, Revision, Material, Vendor of a part, an assembly's own; undefined ones dashes. The P3C.3 `description` field on parts is retired: files that have it read it into the Description property (unit test). Project stays dashes (no such property). |
| X4 | 2D sheet viewport (pan/zoom, F), multiple sheets, Sheets flyout | ✅ | P3C.1. `course_drw_create` 06–08; `course_drw_sheets` 02–13. |
| X5 | Drawing / sheet / view properties | ✅ | Drawing and sheet properties (P3C.1), View properties (P3C.2). `course_drw_sheets` 06, 14; `course_drw_views` 09. |
| X6 | View generation: projection at scale, HLR, tangent-edge modes, shaded, threads, part intersections; regenerate on change | ✅ | P3C.2: HLR (`Kernel::project`, OCCT HLRBRep) at a scale, hidden, tangent modes, shaded (`course_drw_views` 10–12); P3C.6: views regenerate on Update from this workspace (not by themselves; `course_drw_ex3_update` 13–15); threads (P3C.8, `course_drw_more_views` 02–04b); section and broken-out cuts (P3C.8, `course_drw_section_detail` 04–06, `course_drw_more_views` 08–10). P3C.5: views of assemblies (every occurrence moved by the kernel and projected together; shaded in the parts' appearances). P3C wrap-up: assembly views check each visible edge piece against the parts' triangles (a depth buffer), so edges inside or behind another part stay hidden where OCCT's HLR of separate solids left short visible pieces (`course_drw_ex3_update` 30; `hand_brake.rs::assembly_view_hides_edges_inside_overlapping_parts`); **Show part intersections** draws the curves where parts run into each other: each pair of overlapping parts is intersected (the kernel's Intersect of copies) and the common volume's edges that no part hides are drawn (`course_drw_ex3_update` 31–32; same test). Display state stays a disabled stub (configurations are out of scope, user decision 2026-09-29). |
| X7 | View placement: base, projected (first/third angle), auxiliary, suppress alignment, move to sheet, align to edge, show sketches | ✅ | P3C.2: base, projected (first/third angle), auxiliary, suppress alignment, move to sheet, align to edge, show sketches. `course_drw_views` 04–07, 13–14, 18–25; `course_drw_four_views` 02. |
| X8 | Annotations tied to model topology: centerline, centermark, virtual sharp, driven dimensions, palette, hole callouts; dangling red; re-attach | ✅ | P3C.3: every annotation references a view and persistent edge/face names (`cadrs_drawing::annotation`); P3C.6: dangling ones (a name that no longer resolves) are frozen where they were and drawn red, never re-bound; re-attached by dragging a grip (`cadrs_drawing::update`). |
| X9 | Rich-text notes with leaders; tables with merge, navigation, grips | ✅ | P3C.4. `course_drw_notes` 01–17, `course_drw_tables` 01–17. |
| X10 | Drawing BOM table + callouts bound to BOM rows | ✅ | P3C.5: BOM tables (`cadrs_drawing::assembly::BomData` on a `Table`) and callouts (`AnnotationKind::Callout`) whose `Table:` fields read the row of a BOM table on the sheet that lists their occurrence; both update with Update; callouts whose instance is gone dangle red with their last text (unit test). |
| X11 | Update model: gold out-of-date icon, explicit Ctrl+Q update, dependency tracking | ✅ | P3C.6: per-part dependency hashes of the part's edges (names, exact geometry), faces, hole specs and appearance (`cadrs_core::drawing_source`); snapshots per referenced studio; gold icon; Ctrl+Q; one undoable update. P3C.5: assemblies too: the drawing keeps each assembly's occurrences and studios' states; its hash covers the occurrences (placements, sources, hidden), their parts' hashes, their properties and the BOM settings; BOM tables go out of date with it and update in the same step; a change to an assembly the drawing doesn't show marks nothing (unit tests). |
| X12 | Shortcuts per tab type (F, Ctrl+S, D, Shift+R, Shift+D, N, Ctrl+Q, Esc, Tab) | ✅ | F, Ctrl+S, Esc, D, Shift+R, Shift+D (P3C.1, P3C.3), N and Tab/Shift+Tab (P3C.4), Ctrl+Q (P3C.6) live. `course_drw_create` 08 (F), `course_drw_sheets` 18, `course_drw_tables` 03–04 (Tab), `course_drw_ex3_update` 15 (Ctrl+Q). |
| X13 | Import/export: Insert DXF/DWG, Insert image, Export… (PDF, DWG, DXF, DWT, images) | ✅ | P3C.7. Export… from the tab menu: PDF (vector, a page per sheet at its paper size, Inter embedded with a ToUnicode map so text is text: pdftotext reads "Ø4.750"), DXF (our writer, R2013 or R2000, mm, layers, HIDDEN/PHANTOM linetypes on layer and entity; ezdxf reads it with no audit errors and draws hidden lines dashed), DWG through an external converter on PATH (LibreDWG or the ODA File Converter; disabled with a tooltip without one), DWT on the same terms (P3C wrap-up: the sheet written as a DWG-format template named `.dwt` through the converter, disabled with the same tooltip and note without one; `drawing_export.rs::dwt_export_uses_a_converter_or_is_skipped`, skipped like DWG without a converter, and `dwt_is_a_converter_format`; `course_drw_export` 03–04), PNG/JPEG at 150/300/600 dpi; all sheets or the current one; colour or black and white; the folder typed or chosen with Browse…. Insert DXF or DWG (our DXF reader; a block that moves and deletes as one) and Insert image (PNG/JPEG, corner grips keep the aspect ratio) through an in-app file picker. Local custom templates (D1.4: edited from a built-in template and kept under My templates, `course_drw_create` 10–11) are cadrs's own template format. `course_drw_export` 01–12, `course_drw_insert_dxf_image` 11–19. |
| X14 | Shown, not taught: section, detail, broken-out, break, crop views; GD&T; weld; surface finish; ordinate, baseline, chamfer dims; sheet sketch lines/splines; revision tables; version references | ✅ | Sheet sketch lines and splines done (P3C.7, `course_drw_insert_dxf_image` 01–10). P3C.8: the views (D4.12), the dimensions (D6.4), **GD&T** feature control frames (the 14 ASME characteristics as vector symbols, tolerance with Ø and Ⓜ/Ⓛ/Ⓢ, datums 1–3, a leader) and **datum feature** symbols (boxed letter on a filled triangle), **surface finish** (basic, removal required, prohibited, value) and **weld** symbols (arrow, reference line, fillet / V / square on the arrow or other side, size, all around), each placed from a settings card; a **Revision table** preset in the Table dialog (REV, DESCRIPTION, DATE, APPROVED; typed, the release workflow is out of scope) (`course_drw_more_annotations` 01–15, `course_drw_more_views` 01–13, `course_drw_section_detail` 01–12). Version references: built in **P3G.1** (as D13.1: `course_drw_version_reference` 04–09). |

## What each exercise needs
None of the exercises has a numeric self-check; each compares against a public "Completed Exercise"
document. For cadrs, each scenario checks the drawing's **structure**: views and their types,
scales, the dimension texts, the callout texts, the BOM rows, and that no annotation is dangling.
Each needs a stand-in fixture, because the starting documents are Onshape public documents.

- **Ex1 Universal Joint Drawing → `course_drw_ex1_ujoint`**, fixture
  `ujoint_flange_standin.cadrs` (inch). A yoke built from the drawing's own dimensions, so the
  driven dimensions must read exactly the course values:
  - flange disc **Ø4.750** × 0.500 with a square pocket; **4× counterbored holes Ø.266 THRU
    ⌴Ø.438 ↧.250** made with the Hole feature on a **2.061 × 3.282** rectangle (read from
    `ex1-step9.png`: the 3.282 and 2.061 dimensions span hole centres, not a pocket);
  - two lugs **2.500** wide, **2.600** apart, overall height **6.000**, each with a **Ø1.750** boss,
    a **Ø1.250** cross hole and **4× Ø.266 THRU** holes around it (8x in total), top corners
    chamfered at **43.0°** and a base flare at **120.0°**.
  - Checks: 4 views (Front 1:2, Top, Right, Isometric 1:4 shaded); hidden lines on and tangent
    edges Phantom on the 3 orthographic views; title block reads "Universal Joint Flange (stand-in)
    / Made by cadrs"; dimension texts `Ø4.750, 3.282, 2.061, 6.000, 2.600, 2.500, 43.0°, 120.0°,
    Ø1.750, Ø1.250`; callouts `4x Ø.266 THRU ⌴Ø.438 ↧.250` and `8x Ø.266 THRU`.
  - A unit test extracts each dimension from the projected view geometry and compares it with the
    model value (the independent value is the sketch/feature parameter), to 1e−6 in.
  - Needs: templates, sheet, views (HLR, tangent modes, shaded, scale), centermarks, centerlines,
    circle centerline, hole callouts with prefix, smart/diameter dimensions, tab rename.
- **Ex2 Universal Joint Assembly Drawing → `course_drw_ex2_assembly`**, fixture
  `ujoint_assembly_standin.cadrs`: 2× the Ex1 flange, 1 centre block, 4 bushes, 4 axles and 16
  pan head machine screws 1/4-28 × 0.75 (standard content, P3B.5), with the Part numbers and
  Descriptions of the course table set as properties.
  - Checks: the BOM table equals the course's 5 rows exactly (Item No., Name, Quantity, Part
    number, Description; quantities 2, 1, 4, 4, 16); callout texts "Universal Joint Flange x 2",
    "Universal Joint Centre Block x 1", "Graphite Phosphor Bronze Bushes x 4", "Universal Joint
    Axle x 4", "Pan head machine screw 1/4-28 x 0.75 x 16"; view isometric 1:2, shaded, phantom.
  - A unit test checks that the drawing BOM rows equal the assembly BOM (P3B.6) for Flattened,
    Structured top level and multi level.
  - *Done (P3C.5):* `samples::ujoint_assembly` (inch): the studio "Universal Joint" (the Ex1
    flange, renamed "Universal Joint Flange"), the studio "Universal Joint Components" (a 2.4 in
    centre block; a flanged bush: Ø2.75 × 0.25 flange with four Ø.266 holes on the lug holes'
    Ø2.125 bolt circle at 45° and a Ø1.25 sleeve with a Ø.75 bore; a Ø.75 axle standing 0.25 out
    of the bush) and the assembly "Universal Joint Assembly": the second flange turned over and a
    quarter turn so the cross holes meet at z 3.75, the block, the bushes and axles on the four
    boss faces, and the 16 screws inserted on the bushes' hole edges (their Fastened mates, A19.5),
    the first flange fixed. The BOM starts without Name (Item, Quantity, Part number,
    Description) so D12.2's Add column + Move left gives the course's table; the assembly's
    Description is "Made by cadrs" (the title block's second line). **Decision:** P3B.5 had no
    pan head 1/4-28; ASME B18.6.3 Table 17 gives it the 1/4-20 head, so that row was added to
    `data/standard_content.ron` (the part is then named exactly "Pan head machine screw 1/4-28 x
    0.75" and described "… Stainless Steel"), and its Part number set to STD-03923.
- **Ex3 Hand Brake Update → `course_drw_ex3_update`**, fixture `hand_brake_standin.cadrs` (mm):
  - Handle Part Studio with the **Main Sketch of `ex3-step4.png`** in its *original* values (225,
    100, bar height and left offset as before, Ø8.25 hole present), Extrude 1 (8 mm), Extrude 2
    grip bar (Blind 10 + second end), Hole 1 (ISO clearance counterbore, smaller than M6), plus
    Handle Grip (Ø25 bar with 3 holes).
  - Hydraulic Brake Unit assembly with 10 BOM rows as in the course (stand-in bodies for Master
    Cylinder, Spacer ×2, Enclosure Lower/Upper; ISO 4162, 4035, 7089, 4762 fasteners from P3B.5).
  - A finished 3-sheet drawing (Assembly, Handle, Grip) with dimensions, centerline, centermark
    and a `2x Ø8.25` callout.
  - Steps D14.3–D14.11. Checks after Update: Handle sheet dimensions **250.00, 25.00, Ø25.40,
    78.00, R16.00, 3x Ø5.50, 8.00**; the re-attached dimension reads **46.00**; Grip sheet **Ø25.00,
    185.00, 35.00, 62.50, 8.00, 8.50** and callout **`3x Ø6.60 THRU ⌴Ø11.25 ↧6.00`**; no red
    annotations remain; the BOM shows the M6 cap screws.
  - Independent values: 78.00 = 25 + 3 + 50 and 46.00 from the sketch (horizontal distance
    between the Ø25.4 and Ø15 centres); 185.00 = 175 + 10 (both extrude ends); 6.60 / 11.25 / 6.00
    from the ISO 273 medium clearance and counterbore table for M6. A unit test asserts each.
  - **Ambiguous:** the Handle sheet's **R30.00** (step 10) has no source in the visible sketch
    (maybe a fillet on Extrude 1). The stand-in adds an R30 corner fillet so the value appears;
    record it as an assumption.
  - *Done (P3C.6):* `samples::hand_brake` builds the Handle studio through the command layer:
    the fully defined Main Sketch on Front (the bar's top-left corner on the origin; our original
    values 225, 100, a 20 bar, a 15 offset and Ø8.25; the arm's edges at 33° to the vertical
    through the large hole and 36° to the bar's underside, tangent to the R16 arc; the small
    circle equal to the Ø8.25 hole, 29 left of the large one), Extrude 1 (8), Fillet 1 (the R30
    assumption, on the corner where the bar's top meets the arm), Extrude 2 (the Ø25 grip, Blind
    10 and a second end of 160: 185 = 175 + 10 after D14.5; made of two half circles, because
    OCCT's hidden-line removal drops pieces of an outline that runs along a cylinder's seam),
    Extrude 3 (the 8 wide slot the plate sits in, removed from the grip only), Plane 1 (Front
    offset 20) with Sketch 4's three points, and Hole 1 (ISO clearance counterbore M5, Start from
    part, merge scope Handle Grip; the course's dialog says Start from sketch plane, which on our
    plane would start in the air). The drawing is ANSI A in mm (2 decimals): Handle 1:2 and Grip
    1:1, Front + Top + shaded Isometric each. `2x Ø8.25` is a diameter dimension with the prefix
    "2x" on the small circle (it is what dangles when the circle goes); 78.00 runs from the top
    edge to the end hole's centre. The fixture carries the finished drawing with its studio
    snapshots, so it opens up to date. *P3C.5:* the placeholder Assembly sheet is replaced:
    `samples::hand_brake::assembly` builds the studios "Master Cylinder" (a Ø32 body, a push rod
    and a reservoir), "Enclosures" (Enclosure Lower: a U channel round the pivot; Enclosure
    Upper: the plate the cylinder goes through) and "Hardware" (Spacer; Hex flange bolt small
    ISO 4162 as a plain M8 flange bolt: P3B.5 has no ISO 4162) and the assembly "Hydraulic Brake
    Unit" with the course's 14 instances in its order and 10 BOM rows (Item, Name, Quantity):
    standard content ISO 4035 M8 thin nut, 2 × ISO 7089 size 8 washers and 3 × ISO 4762 M5 × 16
    cap screws; their Name properties are the course's ("Hex socket head cap screw ISO 4762":
    Onshape names standard content by component and standard, the size is in the Description).
    **Decision:** the M6 configuration is in the document with the same name (inserted as M6,
    named, edited to M5), so D14.7 keeps row 8's name as the course's `ex3-completed.png` does;
    the M6 shows in the view (bigger heads) and in the row's owner/Description. The Assembly
    sheet: the assembly isometric (seen from the front, left and above so the handle runs up to
    the cylinder as in `ex3-drawing.png`), shaded, 1:2, the BOM at the frame's top-left corner
    (2.5 mm text), and circle balloons 1–10 (Item No.) next to their parts (P3C wrap-up: each
    balloon takes the nearest spot round its leader's end clear of the parts' visible edges and
    shading, the other balloons and leaders, the BOM and the title block, as `ex3-completed.png`;
    it was a ring with crossing leaders). *P3C wrap-up:* the Hydraulic Brake Unit has the
    course's 12 mates: both enclosures fixed and 12 Fastened mates (each at the child's origin, so
    nothing moves; `samples::hand_brake::assembly::MATES`, tested in
    `hand_brake.rs::d14_7_m6_cap_screws_update_the_assembly_sheet`). The course's edits grow the
    Handle Plate into the enclosure (the stand-in has no revolute to swing it clear); the stray
    dashed arcs this left in the shaded close-up were OCCT hidden-line pieces of edges inside the
    enclosure, now hidden (see X6). Also decided: the small circle is **equal** to the end
    hole (an Equal constraint, no dimension of its own), so D14.4's Ø8.25 → Ø15 resizes it too
    before it is deleted; the course's picture shows no dimension on it. The Hole dialog shows a
    Standard row, a "Start from sketch plane" checkbox (off for the stand-in: its sketch plane
    is 3.5 mm outside the grip) and "Through" as `ex3-step6.png` does (fix round 1); its row
    order still follows the Part Studios course's dialog (reordering it like `ex3-step6.png` was
    skipped: it is stage 3A's dialog, reworked on the phase3 branch, so a merge conflict is
    likely). Fix round 2: the 36° is measured between the lower edge and a horizontal
    construction line through the end hole (the same angle, its label below the arm), the 3 from
    the underside's step. Drawing layout (all drawings): a vertical dimension with its text
    along its span keeps the text beside the line (as `ex1-drawing.png`'s 6.000 and
    `ex3-step9.png`'s 25.00), arrows go outside when the text doesn't fit between them, and a
    count prefix ("3x") stacks over a dimension's value; hole callouts stay on one line as in
    `ex1-drawing.png`.

**P3C.1 done 2026-09-28 (judge 8.6). P3C.2 done 2026-09-28 (judge 8.6). P3C.3 done 2026-09-28 (judge 8.5). P3C.4 done 2026-09-28 (judge 8.5). P3C.6 done 2026-09-28 (judge 8.7). P3C.7 done 2026-09-29 (judge 8.6). P3C.8 done 2026-09-29 (judge 8.5). P3C.5 done 2026-09-29 (judge 8.5).** **P3C.5 built 2026-09-29**: assembly views, BOM tables, callouts, Ex2 (`course_drw_ex2_assembly`) and Ex3's Assembly sheet with D14.7/D14.8 (`course_drw_ex3_update` 12b–12d, 29, 30).

## Proposed milestones (stage 3C)
Order: element and sheet first, then view generation (the kernel work), then annotations with
Ex1, then notes, BOM and Ex2, then updates with Ex3, then export and the untaught tools. Every
milestone ships scenarios (`course_drw_*`) and a fresh-judge round (≥ 8.5) against
`intro-to-drawings/`.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3C.1 | **Drawing element, templates, sheets, properties** | D1.1–D1.6, D1.8, D1.9, D2.1–D2.5, D2.7, D2.8, D2.10 (icon, copy), D5.1 (toolbar), D14.2, X1, X2, X3 (title block), X4, X5 (drawing, sheet), X12 | `course_drw_create`: "+" → Create Drawing… → template picker (All/ANSI/ISO) → ANSI_A_INCH → an 11×8.5 in sheet with zones 1–2 / A–B and a title block whose fields show the scale, projection symbol and dashes for empty properties. `course_drw_sheets`: Ctrl+S flyout, Insert sheet, double-click activates, Sheet properties change size to ISO A3. Unit test: every template's sheet size equals the ANSI Y14.1 / ISO 5457 value; title-block fields resolve from a part's properties. Drawing properties panel with Units and precision, Dual, zeros; F fits. |
| P3C.2 | **View generation and placement** | D1.7, D2.9, D4.1–D4.11, D7.*, D8.4, D8.5, D12.5, D12.7, X6, X7 | Kernel: `project(bodies, view_dir, hidden: bool) -> Vec<Edge2>` with visible / hidden / tangent classification (OCCT `HLRBRep` bound in the fork), cached per view. Unit tests: the P6.2 L-block (4×4 with a 2×2 notch) in **first angle** puts Left right of Front and Top below; in **third angle** Top above, Right right; a 20 mm cube with a Ø10 through hole shows 2 dashed lines in the side view; view scale 1:2 halves every projected length. `course_drw_views`: Insert view Front 1:2, projected Top/Right/Iso, iso 1:4 shaded, hidden lines, tangent Phantom, auxiliary from an edge, suppress alignment, move to sheet, align vertical, show sketches. |
| P3C.3 | **Annotations, dimensions, hole callouts; Ex1** | D2.6, D5.2–D5.4, D6.1–D6.8, D8.* (with P3C.1–2), X8 (placement) | `course_drw_ex1_ujoint` on `ujoint_flange_standin` ends with the 4 views, centermarks, centerlines, circle centerline, both hole callouts with prefixes, and the 10 dimension texts listed above, 3 decimals / 1 decimal for angles; the unit test compares each dimension with the model value. `course_drw_dimension_palette` adds a ±0.005 tolerance and dual mm units to one dimension. Hole callout text is generated from the Hole feature's stored spec (PS15.9). |
| P3C.4 | **Notes and tables** | D9.*, D10.*, X3 (notes), X9 | `course_drw_notes`: free note, note with 2 leaders, bold/italic/list, symbol insert, a parametric note showing the part's Name and the sheet scale, which update when the property or scale changes; rotate and resize grips. `course_drw_tables`: 3×4 table with title and header rows, Tab / Shift+Tab, merge and unmerge, midpoint and corner grips, Table properties changes the fixed corner. Unit tests: property-field resolution (undefined → dashes); merge/unmerge is undoable. |
| P3C.5 | **Drawing BOM and callouts; Ex2** | D11.*, D12.* (with P3B.6), X10 | `course_drw_ex2_assembly` on `ujoint_assembly_standin`: Insert BOM Flattened, top to bottom, top-right corner (snapped to the border), left-grip resize; the table equals the course's 5 rows; 5 callouts read "… x 2", "… x 1", "… x 4", "… x 4", "… x 16" and line up on inference lines. Unit test: drawing BOM rows == assembly BOM rows for the three BOM types and both orders. |
| P3C.6 | **Update from the workspace, dangling annotations; Ex3** | D6.5 (re-attach), D13.1 (workspace), D13.2, D13.3, D14.*, X8 (dangling), X11 | `course_drw_ex3_update` on `hand_brake_standin`: after the studio and assembly edits the Update icon is gold and nothing on the sheet has changed; Ctrl+Q updates views and BOM; the dangling centerline, centermark and `2x Ø8.25` show red and are deleted; the red dimension re-attached to the large hole reads 46.00; the Handle and Grip sheets read the values listed above; no red remains. Unit test for the dependency tracker: a change in an unreferenced studio doesn't mark the drawing out of date. |
| P3C.7 | **Drawing export and import** (built) | X13, D2.4 (DXF/DWG, image, line, spline), D2.10 (Export) | Export… → PDF (vector, text as text), DXF, DWG, PNG/JPEG. Unit tests: the Ex1 drawing's PDF contains the string "Ø4.750"; the DXF round-trips (export → Insert DXF on a new sheet) with the same line/arc count; DWG opens in the reference reader used by the test (see Risks). `course_drw_insert_dxf_image` inserts a DXF logo and a PNG into the title block; sheet sketch Line and Spline draw on the sheet. Built: `cadrs_drawing::{export, pdf, dxf, dwg, raster, sheet_sketch}`, `cadrs_core::drawing_export`, `cadrs_core/tests/drawing_export.rs` (PDF text via pdftotext and our own reader, one page per sheet at size in points, DXF round trip to 1e-6 on a new sheet, DWG skipped without a converter, raster sizes, line/spline undo, the logo fixtures); `course_drw_export` (12 frames), `course_drw_insert_dxf_image` (20 frames). |
| P3C.8 | **Untaught view types and annotations, version references** (built, except version references: P3G.1) | D4.8 (threads), D4.12, D6.4 (baseline, ordinate, chamfer, arc length), D13.1 (version), X14 | Section view of a Ø40/Ø20 × 30 tube through its axis shows hatched area = 2 × (40 − 20)/2 × 30 = 600 mm² (unit test on the hatch region); detail view 2:1 of a hole reads the same dimension as its parent; broken-out, break and crop views render on the Ex1 part (scenario). GD&T feature control frame and datum symbols, weld and surface-finish symbols, ordinate/baseline/chamfer/arc-length dimensions (scenario `course_drw_more_annotations`). A revision table as an editable table. A view that references a named version (P3D.3) doesn't change when the workspace does. |

**P3C.1 details:** `ElementKind::Drawing` with sheets (size, orientation, scale, border, zones,
title block, projection, referenced object), each sheet a 2D scene; templates are bundled data
files (ANSI A–E and ISO A4–A0, landscape and portrait, INCH and MM) plus local custom templates;
the Create Drawing dialog; the Drawing properties panel (Units and precision, Dimensions,
Annotations, Views, Construction geometry, Formats, Tables; update from template; lock); a 2D camera
(pan, wheel zoom, F); the Sheets flyout; Sheet properties; the drawing toolbar with every group
present and items enabled by later milestones.

**P3C.2 details:** a projection service in `cadrs_kernel` (visible, hidden, tangent and
silhouette edges, exact where possible), views as sheet objects (base, projected, isometric,
auxiliary) with alignment constraints following the template's projection, scale inheritance,
shaded views from the tessellation with appearances, the view context menu, View properties, Move
to sheet, Align vertical/horizontal, Show/hide sketches, the Insert view dialog and browser.

**P3C.3 details:** annotations reference model topology through persistent names (P3.2) and a view
id; centerline modes, centermark, virtual sharp; driven dimensions (smart D, radial Shift+R,
diameter Shift+D, point-to-point, line-to-line, angular) with snap points, grips and the palette;
hole callouts from the hole spec, with an Edit… prefix dialog.

*Done (P3C.3):* `cadrs_drawing::annotation` (model, measurement, text, layout) and
`cadrs_app::drawing::{annotations, dim_palette}` (tools, selection, grips, palette, callout
dialog). Values are measured from the referenced edges' exact B-rep geometry (`cadrs_core::views`
carries each view's named edges and the studio's Hole specs), falling back to the 2D projection
for silhouettes. Symbols ⌴ ⌵ ↧ are vector strokes (no symbol font bundled). The ANSI A/B title
block is 1.75 in tall (was 2 in), close to the course's, so Ex1's four views fit at 1:2 as in
`ex1-drawing.png`. The stand-in's choices (Ø1.750 read as a boss, not the lug holes' bolt circle,
which is Ø2.125; the 43.0° angle to the lug's top edge; the 120.0° angle between flare and flange
outside the part; the pocket, the flats, the fillet and the rim chamfer) are recorded in
`samples::ujoint`.

**P3C.4–P3C.5 details:** rich-text notes on the Inter faces with leaders and property fields;
generic tables; BOM tables derived from P3B.6 with BOM type, order and fixed corner; callouts
(balloons) with border shapes and 5 bound fields; inference lines.

**P3C.6 details:** per-view dependency hashes on the referenced studio/assembly state; the gold
Update icon and Ctrl+Q; dangling detection when a persistent name no longer resolves, drawn red,
kept with its last position, re-attachable by dragging a grip.

**P3C.7 details:** PDF (vector) writer, DXF writer and reader (R2013 ASCII), DWG through a
converter (see Risks), raster export; Insert DXF/DWG and Insert image on sheets; sheet sketch
lines and splines. Decisions (P3C.7): the PDF embeds Inter whole (Medium, ExtraBold for bold,
Italic; TrueType as CIDFontType2 with Identity-H and a ToUnicode map), no subsetting; ⌴ ↧ ⌵ stay
vector strokes. DXF is written in millimetres at paper size (the sheet's own coordinates), one
file per sheet, texts as TEXT (cap height), arrowheads as SOLID; shaded views and images are left
out of DXF/DWG. Splines are C2 cubics through their points, stored as their fit points and written
as clamped cubic B-splines (the Bézier segments joined, interior knots triple) with the fit
points. An imported DXF keeps its entities (units from `$INSUNITS`) as one block whose origin is
the bottom-left of its extent. Images are stored in the document as base64 PNG/JPEG.

**P3C.8 built (2026-09-29):** everything above except version references (now P3G.1; P3D.3 versions have landed). Scenarios
`course_drw_section_detail` (12 frames), `course_drw_more_views` (13), `course_drw_more_annotations`
(15); the bar sample `cadrs_core::samples::drawing_bar` (`Custom("bar-drawing")`).

**P3C.8 details:** section views (cutting line, hatch), detail views (circle and scale), broken-out,
break and crop; GD&T frames and datums; weld and surface-finish symbols; baseline, ordinate, chamfer
and arc-length dimensions; revision tables; thread display (P3F.3); version-referenced views.

Not scheduled: nothing. Quizzes, the survey and comparisons with Onshape's public documents are out
of scope (learning-site/account); template sharing libraries are out of scope (sharing); the
release workflow behind revision tables is out of scope (release management).

## Risks
- **Hidden-line removal.** The OCCT fork doesn't bind `HLRBRep_Algo`/`HLRBRep_HLRToShape`. Adding
  it is the critical path for every view (a ~6-minute OCCT rebuild per binding cycle); batch it at
  the start of P3C.2. Fallback: mesh-based visibility from the tessellation (ray-cast edge samples),
  which is slower and less exact for tangent edges.
- **Topology-stable annotations.** Dimensions and callouts must follow persistent names (P3.2). If
  naming is wrong, annotations silently attach to other geometry instead of going red. Mitigation:
  the P3C.6 dangling rule (unresolved → red, never a guess), plus edit-then-update tests on Ex3.
- **DWG.** There's no permissively licensed DWG writer in Rust. Options: LibreDWG (GPL-3, would have
  to run as a separate converter process, not linked), or the ODA File Converter (proprietary,
  installed by the user). **Decided in P3C.7:** DWG export writes our DXF and converts it with
  whichever converter is on PATH (`dxf2dwg`/`dwg2dxf` first, else `ODAFileConverter`), as a
  separate process, never linked; Insert DWG converts to DXF the same way. Without one the DWG
  format is disabled with a tooltip saying what to install, and the DWG test is skipped with a
  message. Neither converter is installed here, so DWG output is untested on this machine.
- **Text and symbols.** Drawings need GD&T and hole symbols (⌴ ↧ Ø ⌀ ±); Inter lacks some of them.
  Bundle an open symbol font (for example from the OSIFONT project, GPL with font exception) or
  draw the symbols as vector glyphs. Record the choice.
- **Standards data.** Template layouts and hole tables must come from public sources (ANSI Y14.1,
  ISO 5457, ISO 273), not copied from Onshape's templates; our title block is our own layout.
- **Performance.** HLR on assemblies (the Ex2 and Ex3 models) can take seconds; run it off the main
  thread and cache per view, showing a placeholder while it runs.
- **Stand-ins can't match Onshape exactly.** The judges compare workflow and layout with the
  screenshots; the dimension values match because the stand-ins are built from them.
