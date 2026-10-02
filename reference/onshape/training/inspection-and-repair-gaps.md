# Inspection and Repair Tools: gap analysis (2026-09-27, refreshed 2026-09-29)

This maps [inspection-and-repair.md](inspection-and-repair.md) against the cadrs code on branch
`phase3`. It was written at `ff7616b`; on 2026-09-29 every row was re-checked at `6efe92d` (after
Part Studios P3.1–P3.11: red failed rows and the header error icon, persistent naming,
rollback, Suppress, Show dependencies, the `:errors` filter, folders, Final, warnings, Draft)
and then updated for P3D.1–P3D.2 and P3D.3–P3D.4 (see the status sections at the end). ✅ done ·
🟡 partial · ❌ missing · **out of scope** (only cloud and multi-user collaboration, sharing and
permissions, release management and paid tiers, Onshape account and learning-site features, and
what the user has ruled out: the detailed Versions and history panel, "niche; out of scope by
user decision 2026-09-29", and configurations). Related: [intro-to-part-studios-gaps.md](intro-to-part-studios-gaps.md) (P3.1–P3.11),
[essential-tips-gaps.md](essential-tips-gaps.md) and [test-drive-gaps.md](test-drive-gaps.md)
(document history and versions, shared with P3D.3).

**Summary (after P3D.4):** the course is covered. P3D.3 added the persisted document history
(`cadrs_core::history_log`) and a basic History panel; P3D.4 added Repair (a second, view-only
viewport at a history entry or a feature's last healthy regeneration), Replace reference with
Propagate, Offset of a face region and the Conrod stand-in with its exercise. The detailed
Versions and history panel (graph, legend, filters, Name/Modified columns, versions, branches)
is out of scope by user decision 2026-09-29; the feature menu's Section view was enabled in P3E.3.
- Solver and diagnostics: `SolveReport::conflicting` names the constraints and dimensions left
  unsolved; `cadrs_sketch::diagnostics` adds loose ends (grouped), constraint rows with type,
  numbered name, entities, mode (internal / external via Use and Pierce links / in-context) and
  status (solved / driven / error).
- The sketch footer's diagnostics icon opens Profile inspector… / Constraint manager…, two
  floating panels (`cadrs_app::sketch_diagnostics`, widgets `cadrs_ui::{FloatingPanel, Switch,
  ActionRow}`).
- Feature errors: every part feature reports Ok / Warning / Error (`rebuild::FeatureStatus`),
  with the inputs it lost (`Build::missing`); dialogs keep a lost input as "Missing Face of
  Sketch 3" / "Missing Edge of Extrude 5", red; the header "!" selects the first failing row;
  the "Sketch could not be solved." and "Offset could not be created at this distance." toasts.
- The feature menu has the course's items in its order; Edit healthy moment, Section view,
  Dynamic suppression and Add comment are shown disabled.
- History (P3D.3): every committed change is an entry ("Conrod :: Edit : Sketch 2") with its
  delta and a full copy every 16 entries, stored beside the document; the History panel (left
  rail) lists Main, the changes grouped by author ("2 changes"), Start; right-click Restore
  (undoable) and View in repair.
- Repair (P3D.4): `cadrs_app::repair` (own camera, render layer, view cube, Synchronize view,
  Zoom to fit / Isometric, the right strip's icon, the missing reference's old geometry in
  yellow), `cadrs_app::replace_reference`, `SketchOp::OffsetLoop`, `samples::conrod`.

**Cross-stage dependencies.** Draft (in the course's Conrod) now exists (P3.10), so the P3D.4
stand-in may use it. Everything else is in P3D.3–P3D.4 or P3.x.

**Counts (50 IR IDs + 8 X IDs + 2 quiz/survey rows = 60):**
- As written at `ff7616b`: ✅ 1 · 🟡 9 · ❌ 48 · out of scope 2.
- Refreshed at `6efe92d`, before P3D.1: ✅ 3 · 🟡 15 · ❌ 40 · out of scope 2 (IR: 2 / 12 / 36;
  X: 1 / 3 / 4).
- After P3D.1–P3D.2 (fix round 1): ✅ 29 · 🟡 5 · ❌ 24 · out of scope 2 (IR: 25 / 4 / 21; X: 4 / 1 / 3).
- After P3D.3–P3D.4: ✅ 58 · 🟡 0 · ❌ 0 · out of scope 2 (IR: 50 / 0 / 0; X: 8 / 0 / 0). IR5.6 and
  X7 are ✅ for the basic history panel, their detailed parts out of scope (user decision
  2026-09-29); IR5.5's and X6's remaining disabled entries are owned elsewhere (Section view →
  P3E.3, Suppress by variable → P3F.4) or out of scope (configurations, Add comment).
- After stage 3E (2026-10-02): unchanged, ✅ 58 · 🟡 0 · ❌ 0 · out of scope 2. IR5.5's Section
  view item is enabled (P3E.3, `course_td_section` 08, 15); Suppress by variable is still drawn
  disabled (variables exist since P3F.4, the suppression rule isn't built).
- IR5.5 follow-up (2026-10-02): unchanged, ✅ 58 · 🟡 0 · ❌ 0 · out of scope 2. Suppress by
  variable is built and enabled (`course_insp_suppress_by_variable`,
  `cadrs_core/tests/suppress_by_variable.rs`); the feature menu's only disabled entries left are
  Suppress by configuration and Add comment (both out of scope).

## 1. Sketch troubleshooting

### IR1 Profile inspector
| ID | Requirement | Status | Notes |
|---|---|---|---|
| IR1.1 | Find loose ends of non-construction geometry | ✅ | P3D.2: `cadrs_sketch::diagnostics::loose_ends`: ends of regular lines and arcs that no other regular curve shares, no coincident record or point-on-curve holds, and that don't lie on another curve or an imprinted edge; clustered within 5 % of the sketch's size (0.01–5 mm). Unit tests: 1 lone end + a 0.508 mm pair, none in a closed rectangle, construction ignored. (Before: ❌.) |
| IR1.2 | Diagnostics icon in the sketch footer → Profile inspector… / Constraint manager… | ✅ | P3D.2: the icon opens a menu with both (`sketch-diag-menu`); Final sits beside it (P3D.1). `course_insp_profile_inspector` 01, `course_insp_constraint_manager` 03. (Before: 🟡, icon only.) |
| IR1.3 | Floating panel, "Loose ends" list (grouped), red circle per loose end | ✅ | P3D.2: `profile-inspector` under the sketch dialog, × close, rows "Loose end" / "Loose ends (2)", filled red dots in the view. `course_insp_profile_inspector` 02. (Before: ❌.) |
| IR1.4 | Click a row zooms to it; Previous/Next; ? | ✅ | P3D.2: a row click or Previous / Next (wrapping) selects the row and zooms (a 5 mm box around the group, right of the panels); the "?" has a tooltip. Frames 03–05. (Before: ❌.) |
| IR1.5 | Live list | ✅ | P3D.2: rebuilt whenever the sketch changes; Coincident on the gap drops its row. Frame 06; unit test `closing_the_gap_with_coincident_removes_its_row`. (Before: ❌.) |

### IR2 Constraint manager
| ID | Requirement | Status | Notes |
|---|---|---|---|
| IR2.1 | Floating panel listing every constraint and dimension | ✅ | P3D.2: `constraint-manager`: every stored constraint (the hidden bookkeeping ones and composites' quiet ones left out), every dimension (the stand-in's Diameter 1), and curve ends sharing a point as Coincident rows; the list takes the height left under the panel's filters and scrolls. `course_insp_constraint_manager` 04. (Before: ❌.) |
| IR2.2 | Open from the diagnostics menu; × close | ✅ | P3D.2. Frames 03, 13. (Before: 🟡.) |
| IR2.3 | Filters: auto-select toggle, Type icon grid, Mode (internal/external/in-context), Status (driven/solved/errors) | ✅ | P3D.2: collapsible Filters; the switch (off: only the selected entities' constraints, frame 10); a 22-icon Type grid; Mode and Status toggles (none on = all). In-context is a filter that nothing matches yet (cadrs has no in-context references). Several icons are the closest icon-rs ones (see the status section). (Before: ❌.) |
| IR2.4 | Sort by constraint / by entity | ✅ | P3D.2: tabs; by entity lists each entity with its constraints under it. Frame 07. (Before: ❌.) |
| IR2.5 | Constraint rows with entity children; external sources in brackets | ✅ | P3D.2: "Equal 1 [Extrude 1]" for constraints on Used/Pierced geometry (the link's feature), entities ("Circle 1", "Line 2") under each row. Frame 04. (Before: ❌.) |
| IR2.6 | Error rows red; hover highlights geometry | ✅ | P3D.2: the whole conflicting set is red and bold (`solve::conflict_set`: what the solver left unsolved plus every constraint or dimension whose removal alone lets the sketch solve; its geometry is drawn red too, and, since Final part 2, its constraints' glyphs are white on red chips as in `ex1-step4.png`, `course_insp_constraint_manager` 02); hovering a row draws its entities with a 4 px orange band over the sketch's lines and makes it the sketch's hover, so its glyph turns orange. Frames 05, 06; unit test: the Errors filter equals the conflicting set (Equal 1 and both diameters). (Before: 🟡.) |
| IR2.7 | Per-row delete; Delete all (filtered); undoable | ✅ | P3D.2: each trash and Delete all is one `EditSketch` step (`SketchOp::Delete`); shared-end Coincident rows can't be deleted from the list (their trash is greyed). The row × drops a row from the list until the panel reopens. Frames 08, 11, 12; unit test `delete_all_is_one_undo_step`. (Before: ❌.) |

## 2. Repair and replace references

### IR3 Repair
| ID | Requirement | Status | Notes |
|---|---|---|---|
| IR3.1 | Show an earlier healthy state next to the current one | ✅ | P3D.4: `cadrs_app::repair`: the Part Studio rebuilt at a history entry (`HistoryLog::state_at`, rebuilt on the kernel thread) in a panel beside the viewport. `course_insp_ex1_conrod` 13, `course_insp_edit_healthy_moment` 02. (Before: ❌.) |
| IR3.2 | Versions and history → Repair icon → pick an entry; or "View in repair" | ✅ | P3D.4: the History panel's wrench (`history-repair`) shows the blue banner "Select a history entry to repair this Part Studio."; the entry clicked opens Repair; right-click an entry → View in repair. Picking again re-targets the open panel. `course_insp_ex1_conrod` 12–13. (Before: ❌.) |
| IR3.3 | "Edit healthy moment of <feature>…" (feature menu and broken selection menu) | ✅ | P3D.4: enabled when the feature has a last healthy regeneration; opens Repair on it and the feature's dialog. The rebuild notes every healthy feature at the current entry when it settles (`DocLog`, `HistoryLog::note_healthy`, persisted). Also on a red item's right-click menu in a feature dialog. `course_insp_edit_healthy_moment` 01–03; unit test (`last_healthy` of Extrude 5 = the Sketch 2 fix). (Before: ❌.) |
| IR3.4 | Repair panel: second view-only viewport with header, open-in-new, ?, own view cube | ✅ | P3D.4: its own camera on render layer 7 drawing into an image the panel shows (at the panel's size), part edges on that layer, "Repair" tab, "Viewing Main :: <entry>", open-in-new (disabled: one window), "?", its own view cube (the cube's scene seen from the Repair view; its faces clickable). The main viewport is split; both halves are fitted when it opens. (Before: ❌.) |
| IR3.5 | Synchronize view (on by default) | ✅ | P3D.4: a checkbox; on, a change of either view is copied to the other; off, they move apart (right-drag orbits, middle pans, the wheel zooms the Repair view). `course_insp_edit_healthy_moment` 04–05. (Before: ❌.) |
| IR3.6 | Hover a missing reference → highlight where it was in the Repair panel | ✅ | P3D.4: `cadrs_core::repair::outline`: a region's outline in the sketch as it was, a face's loops, an edge with its tangent chain (on the parts before the feature). Also for the Replace dialog's "Selection to replace". `course_insp_ex1_conrod` 14, 17; `course_insp_edit_healthy_moment` 03. (Before: ❌.) |
| IR3.7 | Repair panel menu: Zoom to fit, Isometric | ✅ | P3D.4: right-click in the panel (a right-drag is an orbit, not a click). `course_insp_edit_healthy_moment` 06–08. (Before: ❌.) |
| IR3.8 | Repair icon in the right strip while open | ✅ | P3D.4: `panel-repair` (pressed; it closes Repair). `course_insp_ex1_conrod` 13, `course_insp_edit_healthy_moment` 10. (Before: ❌.) |

### IR4 Replace reference
| ID | Requirement | Status | Notes |
|---|---|---|---|
| IR4.1 | Swap a reference and propagate downstream | ✅ | P3D.4: `cadrs_core::repair::ReplaceReference` (one undo step, inside the feature's dialog). Unit test `propagate_replaces_every_later_use`; Conrod unit test. (Before: ❌.) |
| IR4.2 | Only while Repair is open: Replace icon on field hover | ✅ | P3D.4: `cadrs_ui::SelectionListReplaceable`: the rows of the extrude's, revolve's, fillet's and chamfer's lists show a "Replace reference" icon on hover while Repair is open. `course_insp_ex1_conrod` 14. (Before: ❌.) |
| IR4.3 | Replace reference dialog: Selection to replace (red) / with (focused); one at a time | ✅ | P3D.4: `replace-reference-dialog` beside the feature's; the next pick (region, face, edge, sketch) replaces the item at once and the feature previews with it. `course_insp_ex1_conrod` 15, 19. (Before: ❌.) |
| IR4.4 | Propagate changes (on by default) | ✅ | P3D.4: every later use of the same sketch region, sketch, or persistent face or edge name. (Before: ❌.) |
| IR4.5 | ✓ on the feature accepts both dialogs | ✅ | P3D.4: the replacement is a step of the feature's session, so its ✓ keeps it and its ✕ drops it; the Replace dialog's own ✕ takes it back. `course_insp_ex1_conrod` 16, 20. (Before: ❌.) |
| IR4.6 | Fix top to bottom (guidance) | ✅ | P3D.1: errors are listed in feature order and the header "!" selects the first failing row (opening its folder). `course_insp_error_states` 06. (Before: 🟡, the "!" showed but did nothing.) |
| IR4.7 | Tangent chain from one edge; check a replacement with non-tangent edges | ✅ | Tangent propagation (P3.6); P3D.4: `repair::chain_check`: with tangent propagation, a replacement standing for fewer edges than the missing edge's chain had (counted in the Repair state) gets a note ("… 1 of the 4 edges …: pick the face"). `course_insp_ex1_conrod` 18; Conrod unit test. (Before: 🟡.) |

### IR5 Error display
| ID | Requirement | Status | Notes |
|---|---|---|---|
| IR5.1 | Red feature name + warning badge; red "!" on the Features header | ✅ | P3.1: red name and icon, an ⓘ with the reason after the name, the header's red "!" (P3.10: amber warnings). P3D.1: the "!" selects the first failing row. `course_insp_error_states` 04–06. (Before: ✅ at `6efe92d`; the stale list said 🟡.) |
| IR5.2 | "Missing Face of Sketch 3" / "Missing Edge of Extrude 5" kept in the field, red tint | ✅ | P3D.1: the rebuild records each failed or warned feature's unresolved inputs (`Build::missing`; regions, whole sketches and faces of extrudes and revolves, edges and faces of fillets and chamfers); their items read "Missing …", red, in a red-tinted field, never dropped. `course_insp_error_states` 07, 08; unit test `error_propagation_marks_only_dependants`. (Before: ❌.) |
| IR5.3 | Yellow toasts "Sketch could not be solved.", "Offset could not be created at this distance."; red dialog title | ✅ | P3D.1: opening a sketch that can't be solved shows "Sketch could not be solved." beside the dialog (red title, P3.1); an Offset distance that can't be made (the solve fails, a radius goes to zero, or the offset would jump to the other side) is refused with the Offset toast. `course_insp_error_states` 10, 11; unit test `an_impossible_offset_distance_is_refused`. (Before: 🟡.) |
| IR5.4 | Features below the one being edited greyed and italic | ✅ | P3.9 rolls back part-feature edits; P3D.1 rolls the Part Studio back to an edited sketch too (the features after it not built, their rows and closed folders grey and italic), with Final in the sketch dialog's footer. `course_insp_error_states` 02, `course_insp_constraint_manager` 02. (Before: 🟡, grey only.) |
| IR5.5 | Feature menu: Rename, Edit, Edit healthy moment, Copy sketch, Show dimensions, Add to folder, Show, Show all sketches, Section view, Suppress, Dynamic suppression, Add comment, Zoom to selection, Show dependencies, Roll to here, Edit sketch appearance, Delete | ✅ | P3D.1: every item, in the course's order. P3D.4: Edit healthy moment works. **P3E.3**: Section view… is enabled and sections the Part Studio on the feature's plane (`course_td_section` 15, 08: Sketch 2's plane, capped). **IR5.5 follow-up (2026-10-02)**: Dynamic suppression ▸ Suppress by variable… is enabled: a dialog picks a variable defined above the feature (+ "Suppress when true (not 0)"), one undo step (`SetSuppressByVariable`); the feature is suppressed while it evaluates to 0, its row tagged "#withHole" and greyed/struck through like a suppressed one; Remove suppression variable clears it; a variable not defined above fails the feature ("Suppression: #x is not defined") (`course_insp_suppress_by_variable` 02 menu, 03 dialog, 04 tag, 05 #withHole = 0: greyed, hole gone, 07 = 1: back, 08 removed, 09 undo; `cadrs_core/tests/suppress_by_variable.rs`: volume 21 989.381 ↔ 24 000 mm³ with the variable, undo/redo, unknown and used-before-defined errors, save/reload, older files). Out of scope: Suppress by configuration (configurations, user decision 2026-09-29) and Add comment (collaboration). `course_insp_feature_menu`, `course_insp_edit_healthy_moment` 01. (Before: 🟡.) |
| IR5.6 | Versions and history panel (header icons, search, filters, Name/Modified columns, graph with Main and "n changes", Start, legend, Repair banner) | ✅ basic | P3D.3: the basic History panel (`cadrs_app::history_panel`, widget `cadrs_ui::TimelineRow`): the rail with Main (open circle), "n changes" groups (by author, collapsible), each entry "tab :: action : feature" with who and when, Start; the Repair icon and × in the header; the Repair banner (P3D.4); "Right-click a row for actions" (Restore, View in repair); basic **versions** (Create version with a name and optional description, squares on the rail, Open read-only, Restore). **Out of scope** ("niche; out of scope by user decision 2026-09-29"): the detailed panel's search, filters, Name/Modified columns, legend, branches and compare. `course_insp_history_panel`. (Before: ❌.) |

## 3. Exercise

### IR6 Exercise: Conrod (stand-in: `fixtures/conrod_standin.cadrs`)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| IR6.1 | Copy the document; Sketch 2 broken, later features red | ✅ | P3D.4: `fixtures/conrod_standin.cadrs` (`samples::conrod`, `fixture conrod_standin`) ships broken with its history: Sketch 2 (unsolvable and open), Extrude 4 and Circular pattern 1 red (a feature pattern of a failed feature fails too). `course_insp_ex1_conrod` 01. (Before: 🟡, the `inspection` stand-in.) |
| IR6.2 | Edit Sketch 2; "Sketch could not be solved." | ✅ | P3D.1, on the inspection stand-in. `course_insp_constraint_manager` 02. (Before: 🟡.) |
| IR6.3 | Open the Constraint manager | ✅ | P3D.2. `course_insp_constraint_manager` 03–04. |
| IR6.4 | Status → Errors lists the red constraints | ✅ | P3D.2 (fix round 1): "Equal 1 [Extrude 1]" and "Diameter 1", the stand-in's conflicting set (the Ø18 circle made equal to the projected Ø30 edge). Frame 05. |
| IR6.5 | Delete Equal 1; close | ✅ | P3D.2. Frames 08, 13. |
| IR6.6 | Open the Profile inspector | ✅ | P3D.2. `course_insp_profile_inspector` 01–02. |
| IR6.7 | "Loose end" and "Loose ends (2)"; click zooms | ✅ | P3D.2. Frames 02–03. |
| IR6.8 | Coincident (i) closes the loop; errors clear | ✅ | Coincident with I, end to end; the row goes, the tab's region comes back, Extrude 2 builds, no errors remain. Frames 06–07; unit test `inspection_stand_in_repairs_as_the_course_does`. |
| IR6.9 | Edit Sketch 3, delete all its entities | ✅ | P3D.1: the downstream features go red and keep their lost inputs as "Missing …" (not dropped). `course_insp_error_states` 02–08. (Before: 🟡.) |
| IR6.10 | Offset the web face region 0.1 in inward; Area 1.339 in² readout | ✅ | P3D.4: in a sketch on a face, the Offset tool's click inside the face offsets its outer loop, taken from the kernel's edges as exact curves (`repair::face_loop`), used as construction and offset (`SketchOp::OffsetLoop`, `cadrs_sketch::face_offset`); the region reads "Area: 2.279 in²" on the stand-in (the course's model: 1.339). `course_insp_ex1_conrod` 09–10; unit tests. (Before: ❌.) |
| IR6.11 | Versions and history → Repair → expand "2 changes" | ✅ | P3D.3–P3D.4. `course_insp_ex1_conrod` 12. (Before: ❌.) |
| IR6.12 | Pick "Conrod :: Edit : Sketch 2"; Repair panel shows it | ✅ | P3D.4. `course_insp_ex1_conrod` 13. (Before: ❌.) |
| IR6.13 | Edit Extrude 5; hover "Missing Face of Sketch 3"; Replace reference | ✅ | P3D.4. `course_insp_ex1_conrod` 14. (Before: ❌.) |
| IR6.14 | Show Sketch 3; pick the new region; ✓; hide | ✅ | P3D.4: Sketch 3 shows while Extrude 5 is edited (its eye works as well, P3.3); the region picked, ✓. `course_insp_ex1_conrod` 15–16. (Before: ❌.) |
| IR6.15 | Edit Fillet 4; hover "Missing Edge of Extrude 5"; Replace | ✅ | P3D.4 (the stand-in's Fillet 1). `course_insp_ex1_conrod` 17. (Before: ❌.) |
| IR6.16 | Replace with the pocket's bottom face; no errors remain | ✅ | P3D.4. `course_insp_ex1_conrod` 18–20. (Before: ❌.) |
| IR6.17 | Mass properties in lb | ✅ | P3D.4: 0.530 lb, 1.868 in³ on the repaired stand-in (= V × 0.2836 lb/in³, and equal to the healthy model's volume to 1e−9). `course_insp_ex1_conrod` 21; unit test. (Before: 🟡.) |

## Self-check and survey
| ID | Requirement | Status | Notes |
|---|---|---|---|
| Quiz | "Begin Self-Check" (mass in lb) | out of scope | Learning-site feature; the mass is checked by a unit test on the stand-in. |
| Survey | Completion survey | out of scope | Learning-site feature. |

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Sketch diagnostics menu with Profile inspector and Constraint manager | ✅ | P3D.2. (Before: ❌.) |
| X2 | Per-constraint error status; loose-end detection | ✅ | P3D.2: `cadrs_sketch::diagnostics`. (Before: 🟡.) |
| X3 | Feature error states, "Missing …" placeholders, warning toasts | ✅ | P3.1 (red rows, header "!"), P3.10 (warnings), P3D.1 (`FeatureStatus`, `Build::missing`, Missing items, the toasts, the "!" jump). (Before: 🟡 at `6efe92d`.) |
| X4 | History-backed Repair view (past state or last healthy regeneration), synced cameras, old-geometry highlight | ✅ | P3D.3, P3D.4 (see IR3). (Before: ❌.) |
| X5 | Replace reference with Propagate | ✅ | P3D.4 (see IR4). (Before: ❌.) |
| X6 | Feature menu: Edit healthy moment, Roll to here, Dynamic suppression, Suppress, Show dependencies | ✅ | Roll to here, Suppress, Show dependencies (P3.9); Edit healthy moment (P3D.4); Dynamic suppression is shown; Suppress by variable works (IR5.5 follow-up 2026-10-02, `course_insp_suppress_by_variable`), by configuration is out of scope. (Before: 🟡.) |
| X7 | Versions and history panel and legend | ✅ basic | P3D.3 (see IR5.6): history and basic versions; the legend and the detailed panel are out of scope by user decision 2026-09-29. (Before: ❌.) |
| X8 | Inch and lb for the exercise | ✅ | Inch (X1) and Pound (P3.5, `MassUnit`). (Before: ❌ in the stale list.) |

## What each exercise needs
**Conrod → `course_insp_ex1_conrod`.** The course starts from a deliberately broken public document
and gives only a few dimensions (its model uses Draft, which P3.10 has since added). So the scenario
runs on a stand-in, `fixtures/conrod_standin.cadrs` (inch, Steel 0.2836 lb/in³), built from our
own features and shipped **already broken**, with its history (P3D.3) containing the healthy
states:
- **Extrude 1 (web):** a trapezoid on Top, full width 1.0 at y = 1 and 0.6 at y = 5 (area
  3.200 in²), 0.3 thick. **Extrude 2 / 3:** big-end ring Ø1.5/Ø0.9 at (0, 0) and small-end ring
  Ø1.0/Ø0.6 at (0, 5.8), 0.5 thick, Add. **Fillet 1** on the ring edges (optional).
- **Sketch 2** on the small ring's face: a 30° notch outline, shipped with a conflicting **Equal 1**
  between two concentric circles of different dimensions, and a gap of 0.02 in between two line
  endpoints (the "Loose ends (2)" pair). **Extrude 4** (Remove) and **Circular pattern 1** use it,
  so they fail.
- **Sketch 3** on the web face: a hand-drawn slot, used by **Extrude 5** (Remove, Blind 0.07) and
  **Fillet 4** (R0.03 on the pocket floor edges, tangent propagation, overflow).
- Steps IR6.2–IR6.17 run as in the course: Constraint manager → Errors → delete Equal 1; Profile
  inspector → Coincident closes the gap; Sketch 3 cleared and redrawn with **Offset of the web
  face region, 0.1 in inward**; Repair on "Conrod :: Edit : Sketch 2"; Replace reference for
  Extrude 5 (new region) and Fillet 4 (pocket bottom face).
- Independent checks (unit test `crates/cadrs_core/tests/course_inspection.rs`):
  - Offset region area = (2·0.39487508 + 2·0.20487508)/2 × 3.8 = **2.279051 in²** (the inward
    offset of the trapezoid; the slanted sides move 0.1·√(1 + 0.05²) horizontally). The Area
    readout must show **2.279 in²** (the course's own model shows 1.339 in²).
  - Extrude 5 removes 2.279051 × 0.07 = **0.159534 in³** (volume before minus after, to 1e−6).
  - Fillet 4 adds material in the concave floor edges: ΔV within 3 % of (1 − π/4)·0.03²·P with
    pocket perimeter P = 8.808994 in, i.e. **1.7014e−3 in³** (corner patches make up the rest).
  - The repaired part's volume equals the volume of the same model built directly without the
    break (a second, healthy fixture) to 1e−9 relative, and mass = V × 0.2836 lb/in³.
  - The course's own values (V 1.149 in³, A 16.269 in², CoM (0, 0, 1.555) in) are for reference
    only. `crates/cadrs_kernel/README.md` lists them as a conformance case; that case can't be built without the
    real geometry and should be replaced by the stand-in's values in P3D.4.

## Proposed milestones (stage 3D)
Order: error states first (everything else shows them), then the sketch diagnostics, then the
persisted history, then Repair with the exercise. Each milestone ships `course_insp_*` scenarios
and a fresh-judge round (≥ 8.5) against `inspection-and-repair/`.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3D.1 | **Feature error states and the feature menu** | IR4.6, IR5.1–IR5.3, IR5.5 (listed items), IR6.1, IR6.2, IR6.9, X3, X6 (part) | Every feature reports `Ok / Warning / Error(reason)` from rebuild; rows go red with a badge and a tooltip, the Features header shows "!" and clicking it selects the first failing row. A lost input stays in its field as "Missing Face of Sketch 3" (red tint), never dropped. Toasts "Sketch could not be solved." and "Offset could not be created at this distance." Scenarios `course_insp_error_states` (delete a sketch's entities → Extrude red with "Missing …"; undo clears it) and `course_insp_feature_menu` (Copy sketch, Show dimensions, Show all sketches, Zoom to selection, Dynamic suppression). Unit test: error propagation marks only dependants. |
| P3D.2 | **Profile inspector and Constraint manager** | IR1.*, IR2.*, IR6.3–IR6.8, X1, X2 | Unit tests: `loose_ends` finds 3 ends (1 + a pair) in the Sketch 2 fixture and none in a closed rectangle; construction geometry is ignored. Constraint manager filters by type/mode/status; Errors lists exactly `SolveReport::conflicting` (+ conflicting dimensions); Delete all removes the filtered set in one undo step. Scenarios `course_insp_profile_inspector` (rows, zoom, Previous/Next, live removal) and `course_insp_constraint_manager` (Errors filter, sort by entity, hover highlight, trash). |
| P3D.3 | **Persisted document history and versions** (built as basic history: versions and the detailed panel out of scope by user decision 2026-09-29) | IR5.6, X7; shared with T7, T9 (essential tips), TD12.1–TD12.3 (test drive), P1.2 (parametric CAD), PS2.1 | Every committed command is appended to a per-document history in the store, with element, feature, action and time ("Conrod :: Edit : Sketch 2"). The Versions and history panel (left rail History) shows the graph (Main workspace open circle, collapsible "n changes", Start), search, filters, Name/Modified columns, the legend, and Create version (name + description). Right-click → **Restore** (a new history entry, undoable) and "Open read-only". Unit tests: restoring entry k reproduces the exact document of step k (hash) for a 200-step fixture; history survives a relaunch; a version is immutable. Scenario `course_insp_history_panel`. |
| P3D.4 | **Repair, Replace reference, face-region offset; Conrod** | IR3.*, IR4.*, IR5.5 (Edit healthy moment), IR6.10–IR6.17, X4, X5, X6 (rest) | `course_insp_ex1_conrod` runs IR6.2–IR6.17 on `conrod_standin`: the Repair panel shows the chosen history state with synced cameras, hovering a missing reference highlights its old geometry there, Replace reference (propagate on) fixes Extrude 5 and Fillet 4, and no errors remain. The unit test asserts the offset area **2.279051 in²** (readout "2.279 in²"), the pocket volume **0.159534 in³**, the fillet ΔV within 3 % of 1.7014e−3 in³, and repaired V == healthy-fixture V. `course_insp_edit_healthy_moment` opens Repair from a broken feature. Sketch Offset accepts a face region. KERNEL.md's Conrod conformance case is replaced by the stand-in. |

**P3D.1 details:** a rebuild status per feature in `cadrs_core` (reason enum: missing input, kernel
failure, empty result, solve failure), carried to the feature list and dialogs; unresolved
persistent names (P3.2) kept as "Missing <kind> of <feature>" entries; the header "!" and badges;
the two warning toasts; the extra feature-menu items (Copy sketch, Show dimensions, Show, Show all
sketches, Zoom to selection, Dynamic suppression, Edit sketch appearance with P3.5).

**P3D.2 details:** `cadrs_sketch::diagnostics` (loose ends, grouping, per-constraint status:
solved, driven, error; mode: internal, external, in-context); two floating panels in `cadrs_ui`
(Profile inspector, Constraint manager) opened from the footer icon; hover cross-highlight; delete
and Delete all through commands.

**P3D.3 details:** an append-only history log in the document store (commands are already
serialisable for undo); snapshots every N entries so any state rebuilds quickly; versions as named
pointers; Restore as a new entry; the Versions and history panel. Branches and merge build on
this in P3E.4.

**P3D.4 details:** a second, view-only viewport (own camera, view cube, render layer) that
rebuilds the Part Studio at a history entry or at a feature's last healthy regeneration; Synchronize
view; the Repair icon in the history panel and the right strip; Replace reference (one at a time,
propagate to all downstream uses of the same persistent name); Edit healthy moment; Offset of a
face region in sketches (the face's outer loop from the kernel as exact curves); the Conrod
stand-in fixture (broken and healthy variants).

Not scheduled: nothing. The quiz and the survey are out of scope (learning-site); "Add comment" in
the feature menu is out of scope (collaboration).

## Risks
- **History size and speed.** Rebuilding any past state for Repair needs snapshots; a naive replay
  of hundreds of commands per hover is too slow. Keep periodic snapshots and cache the Repair
  state's kernel bodies.
- **Replace reference depends on naming quality.** If P3.2's naming resolves a missing reference to
  the wrong entity instead of "missing", the whole repair flow is bypassed. The P3D.1 rule "never
  guess, show Missing" must be enforced in P3.2's resolver.
- **Healthy-moment tracking.** Each feature needs a pointer to its last successful rebuild in
  history, recorded at rebuild time, not recomputed.
- **Second viewport cost.** A second full render of the studio doubles GPU work; render the Repair
  panel at reduced resolution when idle.
- **The Conrod conformance case in KERNEL.md** asserts the course's volume and area, which a
  stand-in can't reproduce. Replace it with the stand-in's closed-form values.

## P3D.1–P3D.2 status (2026-09-29)

Built together in one round on `phase3` (from `6efe92d`).

**P3D.1: feature error states and the feature menu**
- `cadrs_core::rebuild`: `FeatureStatus { Ok, Warning, Error }` from `Build::status(id)`, and
  `Build::missing` / `missing_inputs(id)`: for each failed or warned feature, the positions of
  its unresolved inputs (extrude and revolve regions, whole sketches and faces; fillet and chamfer
  edges and faces), worked out after the fact from the parts before the feature, only for
  features that failed or warned. Additive: nothing else in the rebuild changed.
- Dialogs (extrude, revolve, fillet, chamfer) keep a lost input as "Missing Face of Sketch 1" /
  "Missing Edge of Extrude 1", red, in a red-tinted field (`SelectionListState`: `error` with
  `red` marks tints the field and reds only the marked items).
- The header "!" selects the first failing feature in list order and opens its folder.
- "Sketch could not be solved." (opening a sketch that already can't be solved; a conflict made
  while drawing keeps the longer banner) and "Offset could not be created at this distance." (the
  typed distance can't be solved, collapses a radius, or would put the offset on the other side).
- Rows below the rollback bar are italic as well as grey; editing a sketch before the end moves
  the list's bar under it.
- The feature menu in the course's order (see IR5.5), with Copy sketch + Ctrl+V paste
  (`SketchOp::Paste`, `cadrs_sketch::edit::paste`), Show / Hide dimensions, Show / Hide, Show all
  sketches (one undo step), Zoom to selection (the feature's own faces, or the sketch).
- The inspection stand-in (`cadrs_core::samples::inspection`, set-up command `inspection`): a
  ring and a tab whose Sketch 2 is unsolvable (Equal 1 on the ring's projected edges) and open
  (a spur, a 0.508 mm gap).
- Scenarios: `course_insp_error_states`, `course_insp_feature_menu`.
- Tests: `crates/cadrs_core/tests/course_inspection.rs` (`error_propagation_marks_only_dependants`,
  `delete_all_is_one_undo_step`, `inspection_stand_in_repairs_as_the_course_does`),
  `sketch_edit_tools::tests::an_impossible_offset_distance_is_refused`,
  `edit::paste_tests::paste_copies_geometry_constraints_and_dimensions`.

**P3D.2: Profile inspector and Constraint manager**
- `cadrs_sketch::diagnostics`: `loose_ends` (grouped), `EntityNames`, `items` (type, numbered
  name, entities, mode, status, source), `Filter`, `deletion`. Five unit tests.
- `cadrs_ui`: `FloatingPanel` (title, ×, draggable header), `Switch`, `ActionRow` (a row with
  trailing icon actions and an error style), `panel_caption`.
- `cadrs_app::sketch_diagnostics`: the footer menu, both panels, the red loose-end dots, the
  hover highlight, zoom, Previous / Next, per-row delete and Delete all through `EditSketch`.
- Scenarios: `course_insp_profile_inspector`, `course_insp_constraint_manager`.

**Decisions**
- Coincidence of curve ends is structural in cadrs (a shared point), so the Constraint manager
  lists shared ends as Coincident rows that can't be deleted from the list (their trash is
  greyed); separating them would need an op that splits a point.
- The Errors status is the whole conflicting set (fix round 1): `solve::conflict_set` takes
  what the solver left unsolved and adds every constraint or dimension of the same components
  whose removal alone lets the sketch solve (at most 64 candidates, each a solve); its geometry
  is drawn red. The solver's own pick stays first. Plus broken Use / Pierce links.
- A row's × drops it from the list until the panel is reopened (the course doesn't say what it
  does).
- "Automatically select constraints" on lists everything and a row click selects its
  constraint; off lists only the constraints of the selected entities.
- Loose ends are grouped within 5 % of the sketch's size, clamped to 0.01–5 mm.
- Dynamic suppression is suppression driven by a variable or a configuration; cadrs has neither,
  so the submenu's entries are disabled.

**Not done, and why**
- Edit healthy moment (P3D.4), Section view (P3E.3), Dynamic suppression (needs variables or
  configurations), Add comment (out of scope): shown disabled.
- The Conrod stand-in and its history (P3D.3, P3D.4).

**Icons**: icon-rs has no angular, radial or diametral dimension icon (the Type grid uses
three-point-arc, center-arc and center-circle), no internal / external / in-context mode icons
(sketch, use, link), no driven-status icon (ruler) and no loose-end / profile-inspector icon.

**Fix round 1** (judge 8.4): the whole conflicting set red (and bold) in the Errors filter and
the view; a 4 px hover band over the red plus the hovered constraint's glyph; editing a sketch
rolls the Part Studio back to it, with Final in the sketch footer; rolled-back folder rows grey
and italic; the ⓘ right after the name on every row; the Constraint manager's list takes the
panel's remaining height; a selected Profile inspector row semibold; the stand-in's Sketch 2
has a Diameter dimension (a dimension row, and a conflict of two); scenario captions corrected.

- P3D.1–P3D.2 judge round 2 (**8.6**, 2026-09-29): passed. Its minor deltas were fixed in P3D.3–P3D.4 (see there).

## P3D.3–P3D.4 status (2026-09-29)

Built together in one round on `phase3` (from `eac8b6f`).

**P3D.3: basic persisted document history and versions** (the detailed Versions and history
panel is "niche; out of scope by user decision 2026-09-29")
- `cadrs_core::history_log`: `HistoryLog`, append-only. Each entry: time, user, the tab, the
  action and the feature ("Conrod :: Edit : Sketch 2", worked out from the states either side;
  undo, redo and Restore are entries of their own, "Conrod :: Undo : Insert Sketch 3",
  "Restore : …"), a `Delta` (the changed elements, the element order, the rest of the document)
  and the state's hash; a full copy every 16 entries (`SNAPSHOT_EVERY`), so `state_at(k)` is a
  copy and at most 15 deltas. `RestoreDocument` (one whole-document undo step). Stored as
  `<store>/<id>/history.ron`, written after each entry; a document opened without one starts
  one (Start = the document as opened), and one out of step gets a catch-up entry.
- Healthy moments: `HistoryLog::note_healthy` / `last_healthy`, persisted with the log.
- Versions (a scope addition for Drawings' "Change to version"): `Version` (id, name, optional
  description, entry, time, user; read-only accessors), `HistoryLog::create_version`,
  `versions`, `version`, `document_at_version(id)`. In the panel: the header's Create version
  icon and the document menu's "Create version…" (a popup with the name, "V<n>" by default, and
  the description; `cadrs_ui::NamePopup::description`); versions as squares on the rail,
  breaking the "n changes" groups; right-click Open read-only (the view-only panel, "Viewing
  V1", tab "Read-only") and Restore. Test `a_version_is_immutable_and_persists`.
- `cadrs_app::history_panel`: `DocLog` loads the log when a document opens and appends when the
  document changed and no dialog or sketch is open (a dialog's steps become one entry on ✓, as
  they become one undo step); `note_healthy` marks the features the settled rebuild of the
  current state built without error (and the sketches that solve). The panel (left rail
  History): Main, the changes grouped by author ("n changes", collapsible, by oldest entry),
  Start; right-click Restore / View in repair; the Repair icon and its banner.
- `cadrs_ui::TimelineRow`: a row on a vertical rail (workspace circle, change dot, start dot,
  optional chevron and subtitle), a `ContextMenuTarget` button.
- Tests: `crates/cadrs_core/tests/history_log.rs` (`every_entry_rebuilds_exactly`: ~200 steps of
  sketch edits, tabs added and renamed, document renames, undo; every entry's state equals the
  real one by hash and by value; `restoring_an_entry_reproduces_it_and_is_undoable`;
  `history_survives_save_and_reload`).
- Scenario: `course_insp_history_panel` (06–11: versions).

**P3D.4: Repair, Replace reference, face-region offset; Conrod**
- `cadrs_app::repair`: the Repair panel (see IR3); `open_at`, `edit_healthy_moment`; the red
  missing item's right-click menu in a dialog has "Edit healthy moment of <feature>…" too.
- `cadrs_core::repair`: `Reference`, `references` / `set_references`, `replace` and
  `ReplaceReference` (propagate), `outline` (where a reference was), `tangent_chain`,
  `entity_edges`, `chain_check` (IR4.7), `face_loop` (a sketch's face's outer loop, exact).
- `cadrs_app::replace_reference`: the Replace reference dialog (see IR4); the extrude's,
  revolve's and fillet's own pick handlers stand aside while it is open.
- `cadrs_ui::SelectionList`: `SelectionListItem` (hoverable rows), `SelectionListReplaceable`
  and `SelectionListReplace` (the hover icon).
- Face-region offset: `cadrs_sketch::face_offset` (`use_loop`, `offset_loop`, `outer_loop`,
  `loop_contains`) and `SketchOp::OffsetLoop`; the Offset tool takes a click inside the sketch's
  face.
- The Conrod stand-in (`cadrs_core::samples::conrod`): inch, Steel; Sketch 1 (web trapezoid,
  rings Ø1.5/Ø0.9 and Ø1.0/Ø0.6, necks), Extrude 1–3, Sketch 2 (30° notch, spur, construction
  circles Ø0.6/Ø1.0, imprinting off), Extrude 4 (the notch through the ring), Circular pattern 1
  (×3 about the small end), Sketch 3 (slot R0.15), Extrude 5 (0.07 deep), Fillet 1 (R0.03, the
  course's Fillet 4). Shipped broken (the gap, Equal 1) with 13 healthy history entries plus
  the break: `fixtures/conrod_standin.cadrs`, `fixtures/conrod_standin.history.ron`; the
  healthy model `fixtures/conrod_standin_healthy.cadrs`. Checked current by
  `conrod_fixture_is_current`.
- Numbers (`conrod_stand_in_repairs_as_the_course_does`): offset region **2.279051 in²**
  ("2.279 in²"), pocket **0.159534 in³**, fillet ΔV 1.6910e−3 in³ (−0.6 % from 1.7014e−3),
  repaired V = healthy V = **1.867789 in³** (to 1e−9), mass **0.529704 lb** (V × 0.2836 lb/in³).
- Scenarios: `course_insp_ex1_conrod` (IR6.1–IR6.17), `course_insp_edit_healthy_moment`.
- `crates/cadrs_kernel/README.md`: the Conrod conformance case now lists the stand-in's values.

**Decisions**
- Deltas, not commands, are logged: commands are applied through snapshots (not serialisable),
  and replaying a stored result can't come out differently.
- Entries are grouped by author (consecutive changes by the same user), since versions (which
  bound Onshape's "n changes") are out of scope.
- A region reference whose boundary curves are all gone no longer falls back to "the region
  under its seed point" (`RegionRef::resolve`): a sketch redrawn from scratch has lost the
  region, as Onshape's "Missing Face of Sketch 3" (never guess).
- A feature pattern or mirror of a failed feature fails too (`ex1-step1.png`).
- The Repair view is drawn to an image the UI shows (not a second window viewport), so menus
  and dialogs draw over it; its cube shares the main cube's scene (a label faces either view).
- The replacement is applied as soon as it is picked (the feature previews it); the Replace
  dialog's ✕ takes it back, the feature's ✓ / ✕ accept or drop both.
- Stand-in: the web's top face stays a trapezoid (the necks share its short sides), so its
  offset region is the gap list's closed form; Sketch 2's apex is fixed so closing the gap
  restores the notch exactly; Sketch 2's imprinting is off (with it, the open notch would still
  bound a region with the face's edges).

**Judge follow-up (P3D.3–P3D.4 judged 8.6, committed 4afb139)**
- The Replace reference icon overlays the item's right end only while hovered (no reserved
  slot), so "Missing Face of Sketch 3" shows whole; its tooltip is in `course_insp_ex1_conrod`
  14b.
- `conrod_stand_in_repairs_as_the_course_does` checks the rod before the pocket against its
  closed form, 2.025631 in³ (web, rings with their necks, three notches; derived in the test),
  and the finished rod against 1.86779 in³ and 0.52970 lb.
- The panel is titled "Versions and history"; version rows give their author and time.
- `course_insp_ex1_conrod` closes Repair before the Mass properties frame (21) and shows and
  hides Sketch 3 with its eye (16b, IR6.14).

**Carried deltas from P3D.1–P3D.2, fixed**
- The feature list's toggle tab moves below a feature dialog that would cover it
  (`cadrs_app::panel_tab`; `course_insp_error_states` 08).
- A selected Profile inspector row (any selected `ActionRow`) is bold.
- The Constraint manager's sort row reads "Sort by  Constraint | Entity", inside the panel.
- Rolled-back and suppressed rows fade full-colour icons (a fillet's) as the grey glyphs.
- Loose-end bubbles are 13 px across (15 px for the selected group).
- `solve::conflict_set_cached`: the conflicting set is remembered per sketch revision
  (constraints, dimensions, curve topology), so dragging doesn't redo its trial solves.

**Not done, and why**
- The detailed Versions and history panel (branches, compare, search, filters, columns,
  legend): out of scope by user decision 2026-09-29. Drawings' use of versions follows when
  main is merged.
- Section view (P3E.3); Suppress by variable (P3F.4); Suppress by configuration and Add comment
  (out of scope).
- The Repair panel renders at full resolution while open (no reduced idle resolution).

**Icons**: icon-rs has no repair (wrench and hammer) icon: the wrench `tool` is used; no replace
reference icon: `restore` is used.

- P3D.3–P3D.4 judge round 1 (**8.6**, 2026-09-29): passed, and the course passes. The minor deltas carried to Final:
  - the feature field shows the replacement before ✓ (ex1-step16 keeps it red until then);
  - version rows have no author or time;
  - the offset placement has no direction arrow, and it leaves a construction copy of the face loop;
  - the Repair cube has no triad, and its "?" is faint;
  - the notch highlight in the Repair view is tiny, and the model runs under the header after Top;
  - the Sketch 3 eye step (IR6.14) is not in the scenario.
