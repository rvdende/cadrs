# Introduction to Sketching: gap analysis (2026-09-25; re-audited 2026-09-30, Final)

This maps [intro-to-sketching.md](intro-to-sketching.md) against the cadrs code. It was written
at `main` on 2026-09-25 (phase 2, milestones T1–T5 below) and re-audited on branch `phase3` in
the phase 3 Final stage, after phase 3 had changed much of the app (see "Phase 3 re-audit
(Final)" at the end). ✅ done · 🟡 partial · ❌ missing. Each row cites a scenario
(`scenarios/<name>.ron`, frames in `target/scenarios/<name>/`) or a test.

**Summary (Final):** every requirement is ✅. The phase 2 partial rows are built: ellipse
tangency and crossings (S8), fillets where lines meet arcs (S9.1), Normal to a plane (S12.10) and
the Curvature constraint with a Bézier curve (S12.14). The one regression found (the Use
silhouette scenario hovering the view's own silhouette, S20) is fixed. The toolbar has no
enabled no-op buttons: Intersection, sketch Pattern, Insert image and Insert DXF are disabled
(no course lesson teaches them), and Spline ▾ keeps Onshape's fit-point Spline disabled beside the
working Bézier curve.

## Creating a sketch
| ID | Requirement | Status | Notes |
|---|---|---|---|
| S1.1 | Sketch button, Shift+S | ✅ | `sketch_create_on_top`, `course_ex1_basic` 01, `course_ex2_intermediate` 01 (Front). |
| S1.2 | Right-click plane/face → New sketch | ✅ | T2: viewport context menu on planes and planar faces pre-fills the dialog. `course_s1_new_sketch_context_menu` 02–06. |
| S1.3 | Active-sketch cues | ✅ | The sketch dialog (plane field, Disable imprinting, Show constraints / expressions / errors, and since P3D the diagnostics icon and Final in its footer), the sketch toolbar and the plane drawn: `course_ex1_basic` 01. Editing a sketch with features after it rolls the Part Studio back to it with Final beside the diagnostics icon (P3D.1, `course_insp_constraint_manager` 02). |
| S1.4 | P toggles all planes; per-plane toggle | ✅ | T2: eye toggle on plane rows (hover); hidden rows grey out. `course_s1_plane_visibility` 01–07. |
| S1.5 | N / right-click View normal | ✅ | T2: viewport right-click menu has View normal to sketch plane. `course_s1_view_normal_menu` 01–05 (screenshots finish camera animations, so 02 shows the turn done, like 03). |
| S2.1 | ✓ accepts, listed in the feature list | ✅ | `course_ex1_basic` 07–08a, `course_ex2_intermediate` 15. |
| S2.2 | Cancel → Restore for 10–15 s | ✅ | T2: the Restore toast lasts 12 s. `course_s2_restore_toast` 01–05. |
| S2.3 | Right-click Edit/Rename/Delete; double-click edits | ✅ | T2: inline Rename (undoable) from the feature list and viewport menus; the feature menu has P3D's full item list in the course's order (Rename, Edit…, Edit healthy moment…, …). `course_s2_rename_sketch` 01–09. |

## Sketch entities
| ID | Requirement | Status | Notes |
|---|---|---|---|
| S3.1 | Line drag / chain / double-click ends | ✅ | T2: double-click ends the chain; the tool stays active. `course_s3_line_double_click_end` 01–04. |
| S3.2 | Midpoint line | ✅ | T3: Line ▾; the first click is the midpoint. `course_s3_midpoint_line` 01–06. |
| S3.3 | Corner, center, aligned rectangle | ✅ | T3: Aligned rectangle added (⊥ + ∥, length then width boxes). `course_s3_aligned_rectangle` 01–06; center rectangle `course_ex2_intermediate` 13; corner `constrain_rectangle`. |
| S3.4 | Esc / clicking the tool again exits | ✅ | Esc in every scenario; pressing the active tool's key again leaves it (`course_s9_fillet_line_arc` keeps the Arc tool for two arcs and leaves it with one Esc). |
| S4.1 | Center point circle | ✅ | `sketch_circle_arc`, `course_ex1_basic` 02. |
| S4.2 | 3-point circle | ✅ | T3: Circle ▾; diameter box. `course_s4_three_point_circle` 01–05. |
| S4.3 | 3-point arc | ✅ | `sketch_circle_arc`, `course_ex2_intermediate` 03. |
| S4.4 | Tangent arc | ✅ | `course_ex2_intermediate` 05, 10a–10b, 12. |
| S4.5 | Center point arc | ✅ | Arc ▾ → Center point arc (`sketch_circle_arc`). |
| S4.6 | Line → tangent arc by moving back through the endpoint | ✅ | T2: back over the end point and out switches to a tangent arc. `course_s4_line_to_tangent_arc` 01–05 (05: the line after the arc starts from its end; only the line–arc join is tangent). |
| S5.1–2 | Construction, Q, convert selection | ✅ | `course_ex1_basic` 02–03 (the dash-dot centre line), `course_ex2_intermediate` 02. |
| S6 | Slot | ✅ | T3: Offset ▾; line and arc sources, equal widths, width Ø editable, follows the source. `course_s6_slot` 01–06. |
| S7 | Polygon | ✅ | T3: inscribed and circumscribed, 3–50 sides by mouse, "Nx" label editable by double-click. `course_s7_polygon` 01–08. |
| S8 | Ellipse | ✅ | S8.1 (three clicks) since T3: new curve type with solver, regions and π·a·b area, `course_s8_ellipse` 01–05. Final (the phase 2 gaps): a **line tangent to an ellipse** (Tangent, tool-first or selection-first; solver `LineEllipseTangent`, glyph on the line) and **intersection inference** on ellipses (a point snaps where an ellipse crosses a line, circle, arc, axis or ellipse and gets both point-on-curve constraints), `course_s8_ellipse_tangent` 01–04; tests `a_line_tangent_to_an_ellipse`, `inference_finds_where_an_ellipse_crosses_a_line`. |
| S9.1 | Sketch fillet | ✅ | T3: line–line corners, typed or dragged radius, later clicks Equal, resize arrow (`course_s9_fillet` 01–05). Final: corners where a **line meets an arc, or two arcs meet**: every circle of the radius tangent to both carriers, kept where it touches both curves and runs through the corner, the nearest; the line and arc trimmed back, the corner kept as a hollow virtual sharp on the line and the arc's circle, tangent constraints, R dimension or Equal to the first (`course_s9_fillet_line_arc` 01–04; test `fillet_where_a_line_meets_an_arc`: centre (60 − √600, 5) exactly, R8 still tangent to both, the largest fillet found by bisection). |
| S9.2 | Sketch chamfer | ✅ | T3: Fillet ▾; equal default, two typed distances, resize arrow, linked repeats. `course_s9_chamfer` 01–06. |
| S10.1 | Point tool | ✅ | T3: Shift+S in a sketch; on plane, curves or midpoints; dimensionable. `course_s10_point` 01–05. |
| S10.2 | Inferred midpoints | ✅ | `course_s10_point` 02, `constraint_glyphs_more`. |

## Sketch constraints
| ID | Requirement | Status | Notes |
|---|---|---|---|
| S11.1–3 | Inference, hover wake-up, Shift disables | ✅ | `course_ex1_basic` 02, `course_ex2_intermediate` 02; inference against face geometry `course_s21_imprinting`. |
| S11.4 | Dragging existing geometry infers | ✅ | T2: guide and glyph during drag; constraint added on release in the same undo step. `course_s11_drag_inference` 01–06. |
| S11.5 | Selection-first and tool-first | ✅ | `constrain_rectangle`, `course_s12_symmetric` 02–06, `course_s12_normal` 02–05. |
| S11.6 | Hover shows constraints; Shift keeps glyphs visible | ✅ | T2: Shift keeps the hovered entity's glyphs clickable. `course_s11_shift_keeps_glyphs` 01–06. |
| S11.7 | Click glyph + Delete | ✅ | `course_s11_shift_keeps_glyphs` 05, `course_s20_use_edges` 04–05. |
| S11.8 | White vs blue glyphs | ✅ | Origin and axes pale blue; T5: Use/Pierce links light blue (#92d4ee). `course_s20_use_edges` 03, `course_s12_normal_plane` 02. |
| S12.1–9, 13 | Coincident, Concentric, Parallel, Tangent, Horizontal, Vertical, Perpendicular, Equal, Midpoint, Fix | ✅ | Menu, shortcut and solver all work. `constrain_rectangle`, `constraint_glyphs_more`, `course_ex1_basic` 03, `course_ex2_intermediate` 06. |
| S12.12 | Symmetric (Shift+Q) | ✅ | T1: axis then pairs, tool-first and selection-first; the first-picked entity holds still. `course_s12_symmetric` 01–06 (06: the axis is black once Vertical and on the origin, its length free, as Onshape colours it). |
| S12.10 | Normal (Shift+K) | ✅ | T4: line with circle, arc or ellipse; tool-first and selection-first (`course_s12_normal` 01–05). Final: **a curve and a plane**: with a line picked, a plane picked in the feature list or the view (a default plane or a Plane feature) makes the line normal to it; the plane's trace in the sketch is used as a construction line linked to the plane (`Link::Plane`, it follows a Plane feature and breaks like any link) and a Normal constraint holds the line square to it (`SketchOp::NormalToPlane`); a plane parallel to the sketch is refused with why. `course_s12_normal_plane` 01–03. |
| S12.11 | Pierce (Shift+G) | ✅ | T5: a sketch point and a part edge or another sketch's curve through the plane (either order; tool-first and selection-first); follows the source. Curves are not pierced (points only). `course_s12_pierce` 01–05. |
| S12.14 | Curvature (Shift+U) | ✅ | Final: the sketch's **Bézier curve** (Spline ▾ → Bézier curve, the toolbar's Spline button: four clicks, its ends and two control points, drawn with dashed handle lines and hollow handles; a new curve kind in the solver, regions (exact area, crossings with every curve), trim (deleted when nothing crosses it), mirror and symmetric, DXF, and profiles: a kernel `Curve2::Bezier` edge, conformance `bezier_profiles`) and the **Curvature** constraint: two curves meeting end to end, at least one a Bézier curve (the other a Bézier curve, a line or an arc), made G2 there: tangent without a cusp and equal curvature (solver `Smooth` and `Curvature` equations), with its glyph at the join; Tangent works on Bézier joins too (G1). `course_s12_curvature` 01–05 (05: the region extruded, its Bézier sides one smooth face); tests `curvature_joins_two_beziers_g2`, `curvature_of_a_bezier_and_an_arc_is_one_over_r`, `tangent_and_curvature_need_a_shared_end`, `a_bezier_closes_a_region_with_its_exact_area` (120 mm² exactly), `a_bezier_crossing_a_line_splits_regions`. Onshape's fit-point Spline stays disabled (Spline ▾): the course's Curvature needs a spline and the Bézier curve is one. |
| S13.1 | Quick dimensions on nearly all tools | ✅ | T2: lines get a length box, tangent arcs a radius box. `course_s13_quick_dim_line_arc` 01–06. Live values show 5 decimals as Onshape's do; Final: the value editor widens to fit a long value (`course_s13_first_dim_scales` 02). |
| S13.2 | D tool | ✅ | Point/line/angle/radius/diameter/parallel distance. `course_ex1_basic` 04–05b, `course_ex2_intermediate` 07, 14. |
| S13.3 | **The first dimension scales the whole sketch** | ✅ | T1: the Dimension tool's first dimension rescales the sketch as one undo step. `course_s13_first_dim_scales` 01–07; test `first_dimension_scales_the_sketch_as_one_undo_step`. |
| S13.4 | Inside/outside dimensions to arcs and circles | ✅ | T1: point, line and circle to circle/arc; near or far side by click position; concentric rings measure radially. `course_s13_circle_dims` 01–07, `course_ex1_basic` 05a. |
| S13.5 | Automatic angle dimension | ✅ | Two non-parallel lines give an angle: test `angle_quadrant_picks_the_angle` (the angle kind and the quadrant the cursor picks); the Funnel sketch's 105° (`course_ps21_funnel` 02). |
| S13.6 | Double-click edit, Delete | ✅ | `dimension_edit_value`, `course_s7_polygon` 07. |
| S13.7 | Driven vs driving, automatic driven, toggle | ✅ | T2: both directions in the dimension menu. `course_s13_driving_driven_toggle` 01–06. |

## Sketch tools
| ID | Requirement | Status | Notes |
|---|---|---|---|
| S16 | Sketch text | ✅ | T5: box tool, Text dialog (font, bold, italic, flips, live preview), aspect-held box (one dimension defines it), rotates without its Horizontal, right-click Edit text, text regions extrude (raised, or cut with the Extrude's Remove, which has existed since P3.3). Inter faces only. `course_s16_text` 01–15. |
| S17.1 | Trim (click, drag-over) | ✅ | T4: M; hover preview, click to the nearest crossings, drag trail, circle → arc. Crossed ellipses (and Bézier curves) are not trimmed. `course_s17_trim` 01–09, `course_s17_trim_drag` 01–05. |
| S17.2 | Regions without trimming | ✅ | T1 region finder; `course_s17_regions_without_trim` 01–04. |
| S17.3 | Extend | ✅ | T4: X (Trim ▾); line and arc ends to the first boundary, region shades at once. Not to or along ellipses. `course_s17_extend` 01–07. |
| S18 | Split | ✅ | T4: Trim ▾; line, arc, circle; the split point can be dimensioned. `course_s18_split` 01–09. |
| S19.1 | Offset (O) | ✅ | T1: line, arc, circle or dragged chain; arrow flips; typed driving distance. `course_s19_offset` 01–10, `course_ex2_intermediate` 08a–08c. |
| S19.2 | Mirror + Symmetric | ✅ | T1: mirror line then entities; copies linked by Symmetric (Final: Bézier curves too). No shortcut (M is Trim). `course_s19_mirror` 01–05, `course_ex2_intermediate` 04, 11. |
| S20 | Use / project | ✅ | T5: part edges, whole faces, cylinder silhouettes, other sketches' curves; fixed, black, deletable link glyph; follows its source; a lost source puts the sketch in error. Oblique arcs (elliptical arcs) cannot be projected, nor Bézier curves. `course_s20_use_edges` 01–08, `course_s20_use_updates` 01–06, `course_s20_use_silhouette` 01–02. Final: the silhouette scenario had regressed (no hover, nothing used): its pointer sat on the isometric view's own silhouette of the cylinder, where the pick ray only grazes the finer phase 3 tessellation and missed the part; the pointer now rests on the side facing the view, and both silhouettes are used again. |
| S21 | Imprinting | ✅ | T5: edges of part faces in the plane split regions of sketches on faces; the checkbox (undoable, saved) turns it off. `course_s21_imprinting` 01–04, `course_s21_disable_imprinting` 01–04. |

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Workspace units setting | ✅ | T2: ☰ → Workspace units (mm/cm/m/in/ft/yd, decimals); display and typed values follow it; undoable. `course_x1_workspace_units`. |
| X2 | **Region selection → Area readout** | ✅ | T1: click selects regions (curves split at crossings, holes subtracted); "Area: X.XXX mm²" bottom right. `course_x2_region_area`, `course_ex1_basic` 08b–08c (16682.523, 29327.433 mm²), `course_ex2_intermediate` 16 (24906.823 mm²). |
| X3 | Sketching on faces | ✅ | `course_s1_new_sketch_context_menu` 05–06, `course_s20_use_edges`, `course_s21_imprinting`. |

## What each exercise needs (phase 2 plan, done)
- **Exercise 1 (Basic):** X2 area readout, S13.3 first-dimension scaling, and S13.4 circle-to-circle
  dimensions (the 35 ring). Everything else exists.
- **Exercise 2 (Intermediate):** S19.2 Mirror (+ S12.12 Symmetric), S19.1 Offset of an arc, and X2.
  Everything else exists: tangent arcs, 3-point arc, center rectangle, radius dimensions.

## Milestones (phase 2, all done; kept for the record)
Order: unblock the two exercises first, then fill in the course. T1–T5 were built and judged in
phase 2 (see PROGRESS.md); the Final re-audit below finished the rows they left partial.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| T1 | **Exercise enablers** | X2 region select + Area readout; S13.3 first-dimension scaling; S13.4 circle/arc dimensions (point/line/circle to circle, inside/outside); S12.12 Symmetric; S19.2 Mirror; S19.1 Offset | Scenarios `course_ex1_basic` and `course_ex2_intermediate` reproduce every exercise step from the screenshots, end fully defined, and show the analytically computed area. |
| T2 | **Lifecycle and constraint UX** | S1.2, S1.4 per-plane toggle, S1.5/S2.3 viewport right-click menus, S2.2 restore 10–15 s, S2.3 Rename, S3.1 double-click end, S4.6 gesture, S11.4 drag inference, S11.6 Shift keeps glyphs, S13.1 quick dims on lines and tangent arcs, S13.7 driving → driven, X1 units | Per-lesson scenarios; judge ≥ 8.5. |
| T3 | **Entity tools** | S3.2 midpoint line, S3.3 aligned rectangle, S4.2 3-point circle, S10.1 point, S7 polygon, S6 slot, S9 fillet and chamfer, S8 ellipse (new curve type) | One scenario per lesson; judge ≥ 8.5. |
| T4 | **Edit tools** | S17 trim and extend, S18 split, S12.10 Normal | Scenarios; judge ≥ 8.5. |
| T5 | **Projection, text, imprinting** | S20 Use, S21 imprinting, S12.11 Pierce, S16 text | Scenarios; judge ≥ 8.5. S12.14 Curvature waits for splines. |

The same process applied as before: builder and judge agents, headless scenarios, and a judge
against the course screenshots in `intro-to-sketching/` and the Onshape reference notes.
Placeholder buttons for tools not built are disabled (`course_disabled_placeholders`).

## Phase 3 re-audit (Final, 2026-09-30)

**Result: 56 rows, 56 ✅, none out of scope.** (Phase 2 left 52 ✅, 3 🟡 and 1 ❌: S8, S9.1 and
S12.10 partial, S12.14 missing.) Every row now cites a scenario or a test.

**How it was checked.** All 40 sketching scenarios (`course_s*`, `course_ex1_basic`,
`course_ex2_intermediate`) were rendered on the phase 3 code and every frame read against its
scenario's header, the requirement and, for the exercises, each `ex1-step*.png` /
`ex2-step*.png`; the goldens were compared pixel by pixel. What phase 3 changed and was looked for
in particular: the sketch line colours and weights (under-defined dark blue, fully defined black,
the same ~1–1.5 px everywhere; construction dash-dot; accepted sketches thin grey), the toolbar
(new Part Studio buttons from 3F/3G; the sketch toolbar's disabled buttons), the Part Studio
rolled back while a sketch is edited, Final and the diagnostics icon in the sketch dialog footer,
Show errors, and the feature menu. None of these broke a lesson.

**Regressions found and fixed.**
- `course_s20_use_silhouette` showed no silhouette hover and used nothing (01, 02). Cause: the
  scenario's pointer sat exactly on the isometric view's silhouette of the cylinder; with phase
  3's finer tessellation the pick ray grazed past the part. The pointer now rests on the side
  facing the view; the Use code was unchanged and works.
- The dimension value editor clipped a long live value ("40.03378 mm" lost its first digit and
  its unit, `course_s13_first_dim_scales` 02): the field now widens to fit the value it opens with
  (`cadrs_ui::dim_edit`).

**Checked and left as they are (not regressions).** Live values show five decimals while drawing,
as Onshape does (committed values follow the workspace decimals); a line is black once it can't
leave its own line even with free ends (Onshape's rule, `ex1-step4.png`: `course_s17_trim` 03,
`course_s12_symmetric` 06); the Normal and Perpendicular glyphs are drawn in the icon's dark
colour; the harness finishes camera animations before every screenshot, so
`course_s1_view_normal_menu` 02 shows the turn finished; the feature menu lists P3D's items in
the course's order.

**Built in the Final stage** (the partial rows): S8 ellipse tangency and crossings, S9.1 line–arc
and arc–arc fillets, S12.10 Normal to a plane, S12.14 the Bézier curve and Curvature (details in
their rows). New scenarios, in the golden suite: `course_s8_ellipse_tangent`,
`course_s9_fillet_line_arc`, `course_s12_normal_plane`, `course_s12_curvature`; changed:
`course_disabled_placeholders` (02, 05, 06 renamed: the Spline button, Shift+U and Curvature
work now), `course_s20_use_silhouette` (the pointer).

**Exercise self-checks.** `cadrs_core/tests/course_exercises.rs` still checks both exercises
against independently derived areas and passes: Ex1 fully defined, 16682.523 mm² (the plate) and
29327.43 mm² (with the ring); Ex2 fully defined, 24906.823 mm²; and the first dimension scaling as
one undo step. The scenarios read the same values in the Area readout (`course_ex1_basic` 08b,
08c; `course_ex2_intermediate` 16).
