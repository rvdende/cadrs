# Hands-On Test Drive: gap analysis (2026-09-27; re-audited 2026-09-29)

This maps [test-drive.md](test-drive.md) against the cadrs code. It was first written on branch
`phase3` (`ff7616b`) and **re-audited on 2026-09-29 on branch `phase3c` (`c41d6e3`)**, after
stages 3A, 3B, 3C, 3D and 3G had passed. ✅ done, with a scenario frame or test that exists · 🟡
partial · ❌ missing · **out of scope**. Only these reasons count as out of scope: cloud and
multi-user collaboration, sharing and permissions, release management and paid tiers, and Onshape
account and learning-site features. The user's decision of 2026-09-29 adds these, with the reason
"niche; out of scope by user decision 2026-09-29": configurations, **image tabs**, full
round/conic/curvature fillets, and a detailed Versions and history panel.

Most of this tour repeats the core courses, so many rows cite their scenarios:
- [intro-to-part-studios-gaps.md](intro-to-part-studios-gaps.md) (P3.x);
- [intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md) (P3B.x);
- [intro-to-drawings-gaps.md](intro-to-drawings-gaps.md) (P3C.x);
- [inspection-and-repair-gaps.md](inspection-and-repair-gaps.md) (P3D.3 history and versions);
- [derived-and-linking-gaps.md](derived-and-linking-gaps.md) (P3G links, Tab manager, Move to document).

**Stage 3E passed (2026-10-02).** Every TD and X row is ✅ (66) or out of scope (9), and so is
every exercise step (1–7 and 9 ✅; 8 and 10 out of scope). The exercise runs through the UI in
`course_td_ex1_drill` with no scenario shortcuts, and `crates/cadrs_core/tests/course_test_drive.rs`
checks the gasket volumes 1676.6483 / 838.3241 / 838.3241 / 1676.6483 mm³ (Main, branch, merged
Main, restored) to 1e−6 against 2·/1·(1200 − 115.125π), the two-way Part number sync and the
drawing (2 views, 3-row BOM, 148/80 dimensions, callouts 1–3, title block).

| Milestone | Judge | Rounds |
|---|---|---|
| P3E.1 Documents page | 8.67 | r1 |
| P3E.2 Tabs | 8.53 | r2 (r1 8.29) |
| P3E.3a Render modes, perspective, named views, Section view | 8.9 | r3 (r1 7.8, r2 8.3) |
| P3E.3b Measure, Analysis, mouse preference | 8.9 | r3 (r1 8.1, r2 8.4) |
| P3E.4 Branches and merge | 8.6 | r1 |
| P3E.5 Walkthrough and drill stand-in | 8.9 | r2 (r1 8.45) |

**Remaining deltas (all minor; for the final regression pass):**
- P3E.5: a connector-style hover glyph on a bolt hole's edge point with two edges selected
  (`course_td_ex1_drill` 16, moderate); the drawing tab is "FUEL AND POWER TRAIN Drawing" (the
  course names it "FUEL AND POWER TRAIN") and no frame shows the rename; the drawing BOM keeps six
  columns and wraps cells instead of dropping optional columns; the assembly BOM panel squeezes the
  viewport (19–24); the Restore row reads "1 change"; a single-entry merge group repeats its entry;
  the upward merge dropdown covers a column header (37, `course_td_branch_merge` 12); two selected
  edges show only "Diameter: 5.500 mm"; no unit test for BOM Switch to or step 5's screw
  placement; `fit_within` returns an overflowing table if the resize fails; K has nothing to hide
  (standard content has no explicit connectors).
- P3E.4: workspaces can't be renamed or deleted ("Copy workspace…" disabled); a merge between
  two branches has no common base and lists every differing tab.
- P3E.3b: the curvature scale follows the largest vertex value (use a percentile), |H| is
  unsigned, band edges are jagged; no zoomed zebra frame of a fillet joint; mouse_prefs 12 repeats
  08b; the Preferences dialog has an empty band above OK.
- P3E.3a: Named views has no thumbnails or rename; hidden-line modes still show planes and
  sketches; a sketch crossing the section plane at an angle is not clipped; extrude/transform
  manipulators still use the flat arrow (the section and draft arrows use `manipulator.rs`).
- Icons: icon-rs has no analysis, zebra or mouse icon; stand-ins are used.
- **Optional, not done** (main session decision 2026-10-01, after a permission refusal of a
  scenario edit): a `course_td_branch_merge` 14c frame with Mass properties on the merged
  assembly's 1 mm gasket (covered by `the_merged_assemblys_gasket_instance_is_the_1_mm_part`),
  and an assembly Curvature frame in `course_td_analysis`.

**Summary (re-audit 2026-09-29).** The part, assembly, BOM and drawing rows landed in other
stages:
- P3.3 extrude from a face, booleans and Parts list;
- P3B.2–P3B.7 mates, Group, subassemblies, standard content and K;
- P3B.6 BOM, properties and CSV;
- P3C.1–P3C.6 drawings, title block and update;
- P3D.3 persisted history, Restore and versions;
- P3G.1 inserting from other documents, and P3G.3 the minimal Tab manager.

What is left is the 3E-only work:
- **Documents page:** labels, the details panel, a click-selects model, opening folders, the Type
  filter and grid view, bundled samples, and Import files… (the Create ▾ items Label… and Import
  files… only show "not supported yet" toasts).
- **Tabs:** tab folders, a searchable and reorderable Tab manager, and tab-bar overflow (the strip
  clips).
- **Viewing and analysis:** render modes and perspective (listed but disabled in the view menu),
  named views, a viewport Section view, Measure and Analysis (the bottom-right icons have no
  handlers), and a mouse-mapping preference.
- **Branches and merge.**
- **The walkthrough on a drill stand-in**, which doesn't exist yet. It also closes a few small
  gaps: Switch to from a BOM row, a BOM snapped to the title block, an assembly Front + Right
  view, and ISO 4762 M5×25 placed on holes.

**Scope decisions made in this re-audit:**
- **Image tabs.** The user's "image tabs" exclusion covers every non-CAD viewer tab: images, PDF
  and video. These requirements are about importing non-CAD files as tabs that only view them
  (TD5.1 "imported files (PDF, images, video)", the "files as tabs" part of TD3.1, and essential
  tips T1.1 "non-CAD files as tabs"), so they are **out of scope** ("niche; out of scope by user
  decision 2026-09-29"). Importing **CAD** files (STEP, STL, and IGES with P3F.2) from the
  documents page stays in scope: it makes a Part Studio, not an image tab.
- **Branches and merge stay in scope.** P3D.3 put "branches" in the detailed Versions panel it
  left out. That exclusion covers the panel's detail (graph, legend, filters, columns, compare).
  It doesn't cover branching as a data operation, which is TD12.4–TD12.7 and exercise step 9.
  P3E.4 builds workspaces and merge without the detailed graph: a workspace switcher, Branch and
  Merge on the existing basic History panel's menus, and a merge dialog. If the user rules
  branches out as well, TD12.4–TD12.7 and X9's branch half go out of scope, and step 9 becomes
  version → edit → Restore.

**Counts (66 TD IDs + 9 X IDs = 75).**

| | ✅ | 🟡 | ❌ | out of scope |
|---|---|---|---|---|
| After P3E.5 (2026-10-02, stage passed, judge 8.9) | 66 | 0 | 0 | 9 |
| P3E.5: TD | 57 | 0 | 0 | 9 |
| P3E.5: X | 9 | 0 | 0 | 0 |
| After P3E.2 (2026-09-30, pre-judge) | 50 | 11 | 5 | 9 |
| P3E.2: TD | 43 | 9 | 5 | 9 |
| P3E.2: X | 7 | 2 | 0 | 0 |
| After P3E.1 (2026-09-29, pre-judge) | 47 | 13 | 6 | 9 |
| P3E.1: TD | 41 | 10 | 6 | 9 |
| P3E.1: X | 6 | 3 | 0 | 0 |
| After the re-audit (2026-09-29) | 40 | 19 | 7 | 9 |
| TD | 35 | 15 | 7 | 9 |
| X | 5 | 4 | 0 | 0 |
| Before the re-audit | 12 | 17 | 37 | 9 |
| Before: TD | 10 | 15 | 32 | 9 |
| Before: X | 2 | 2 | 5 | 0 |

Rows with an out-of-scope part keep their status for the rest: TD3.1, TD3.3, TD3.6, TD3.8, TD3.9
and TD5.1. The 10 exercise-step rows are tracked separately.

**Status changes in P3E.5 (2026-10-01, the stage wrap-up, pre-judge):** TD8.8, TD9.4, TD10.4,
TD10.5 and TD10.6 🟡 → ✅ (P3E.5). With the evidence of P3E.3 and P3E.4, which this file hadn't
recorded: TD3.7, TD6.5, TD6.7, TD12.8, X3 and X9 🟡 → ✅; TD6.6 and TD12.4–TD12.7 ❌ → ✅. Every
TD and X row is now ✅ or out of scope, and so is every exercise step.

**Status changes in P3E.2 (2026-09-30, pre-judge):** TD5.3 ❌ → ✅; TD5.4 and X6 🟡 → ✅. The
cross-stage rows T1.2 and X1 (essential tips, already ✅ from P3F.3/P3G.3) gain the P3E.2 evidence;
the derived-and-linking note on the full Tab manager is done.

**Status changes in P3E.1 (2026-09-29, pre-judge):** TD3.1, TD3.3, TD3.5, TD3.6, TD4.1 and X5
🟡 → ✅; TD3.8 ❌ → ✅. TD3.7 stays 🟡 (its labels column, Type filter and grid view are done; the
real last-opened workspace name is P3E.4). The cross-stage rows ER1.2 and ER8.5
(derived-and-linking) are ✅.

**Status changes in the re-audit:**
- **Now ✅, done in other stages** (27 TD, 4 X):
  - TD1.1 (P3D.3)
  - TD5.1 (P3C.1 drawing tabs; its file tabs are out of scope)
  - TD6.1, TD7.1–TD7.5 (P3.3, P3.9)
  - TD8.2–TD8.7, TD8.9 (P3B)
  - TD9.1–TD9.3, TD9.5 (P3B.6)
  - TD10.1–TD10.3, TD10.7, TD10.8 (P3C)
  - TD12.1–TD12.3 (P3D.3, basic)
  - X1, X4, X7, X8
- **Now 🟡:**
  - TD8.8, TD9.4, TD10.4, TD10.5, TD10.6: the mechanisms exist, and small pieces are missing.
  - TD12.8: versions landed; branches haven't.
  - X6: P3G.3's minimal Tab manager exists.
  - X9: versions landed; branches haven't.
- **Now ❌ (was 🟡):** TD3.8. The details "strip" is a stub with no handler.
- **Now 🟡 (was ✅):**
  - TD3.6: folders can't be opened.
  - TD6.7 and X3: they work in code (`viewport.rs:1278`), but no frame or test shows Space
    clearing the selection.

## 1. Getting started

### TD1–TD2 Introduction, signing in
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD1.1 | No Save button; every action recorded in history | ✅ | Auto-save 1 s after the last change (`document.rs` `auto_save`, `AUTO_SAVE_DELAY`) and on leaving the document; no Save command. **P3D.3**: every committed command is appended to the persisted `history_log` (`reload_roundtrip` 03, `reload_roundtrip_relaunch` 02; `course_insp_history_panel` 01; `history_log.rs::history_survives_save_and_reload`). |
| TD1.2 | Subscription tiers | out of scope | Paid tiers. |
| TD2.1 | Web sign-in, SSO | out of scope | Account feature (local app, no accounts). |

### TD3 Documents page
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD3.1 | Create: Document, Folder, Publication, Label; Import | ✅ | Document… and Folder… work (`landing_create_document` 01–08). **P3E.1**: **Label…** opens "Create label" (`course_td_documents_labels` 01–03; `CreateLabel`, undoable). **Import files…** opens the file picker (STEP/STL; `landing_empty` 06) and the file becomes a new document named after it whose Part Studio holds its Import feature, one undoable library step (`course_td_documents_import` 01–06: 9 920 mm³ bracket; IGES arrives with P3F.2). Out of scope: importing images, PDF or video **as tabs** ("niche; out of scope by user decision 2026-09-29", image tabs); Publication and cloud-storage import (sharing / cloud). |
| TD3.2 | Search by partial name | ✅ | Top-bar search box filters documents and folder names (`landing.rs` `sync_search`); `library.rs::filters_and_search`. |
| TD3.3 | Filters: Recently opened, Created by me, Shared with me, Teams, Labels, Public | ✅ | Owned by me, Recently opened and Created by me work (`library.rs::filters_and_search`; `course_er_ex2_move` 08). **P3E.1**: the **Labels** section lists the labels (a colour dot and the name) as filters (`Filter::Label`), with Rename… and Delete… on right-click and a create-label button (`course_td_documents_labels` 03–04, 10–16; `library.rs::a_label_filter_returns_the_tagged_documents`: 3 of 5). Shared with me, Teams and Public are **out of scope** (sharing); they stay as empty rows. |
| TD3.4 | Trash with recovery | ✅ | Move to trash, Restore, Delete permanently, all undoable library commands (`landing_many_documents` 10–18; `trash_restore_with_undo`, `purge_only_trashed_with_undo`). |
| TD3.5 | Last opened by me strip; click name or double-click thumbnail | ✅ | The strip of up to 6 thumbnail cards (`landing_create_document` 06). **P3E.1**: a click on a card's name (a link) or a double click on its thumbnail opens the document (`course_td_documents_details` 09–10). |
| TD3.6 | Folders | ✅ | Create folder, the "Folders" section, Move to ▸ a folder (`landing_create_document` 07–08, `landing_many_documents` 22; `create_folder_with_undo`). **P3E.1**: a click selects a folder (card or row; its details: owner, created, contents), a double click (or Enter) opens it under a breadcrumb "‹ Owned by me › folder" with Back (`course_td_documents_details` 11–13). Folder sharing is out of scope. |
| TD3.7 | Documents list: thumbnail, Name + workspace, labels, Modified, Modified by, owner; Type filter; list/grid toggle | ✅ | Thumbnail, Name, Modified, Modified by and Owned by columns, sortable (`landing_many_documents` 01, 05–06; `sort_by_each_column`). **P3E.1**: a **Labels** column of colour chips once labels exist (`course_td_documents_labels` 07, 09), the **Type** filter (All / Documents / Folders, `ItemType`) and the **list/grid** toggle (`course_td_documents_details` 14–17). **P3E.4**: the Name column shows the last-opened workspace beside the name ("Alternate Gasket Thickness", `course_td_branch_merge` 07; `DocumentMeta::workspace`). |
| TD3.8 | Details flyout: thumbnail, owner, description, labels, created by; tabs info / versions and history / where used | ✅ | **P3E.1**: a click selects a row (a double click or Enter opens it) and opens the **Details** panel with a rail of Info, Versions and history, Where used. Info: thumbnail, Owner, an editable Document description (saved on Enter or blur, `SetDescription`), Document labels (search field, checkboxes, the chips, "Create new label"), Created by, Created, Modified, Location (`course_td_documents_labels` 05–07, `course_td_documents_details` 01–04, 07). Versions: the history's versions (`documents_page::versions`, `course_td_documents_details` 05; `documents_page.rs::the_versions_list_matches_the_history`). Where used (`link_update::where_used`, 06). The sharing tab is out of scope. Closes **ER8.5** (derived-and-linking). |
| TD3.9 | Top-right icons: Action items, App Store, Learning Center, Help, Account menu | ✅ | Help ▾ → Keyboard shortcuts works (`keyboard_shortcuts` 01). Action items (collaboration), App Store, Learning Center and the Account menu are out of scope (account and learning-site features); cadrs shows the local user's avatar only. |
| TD3.10 | Enterprise toolbar and Projects | out of scope | Paid tier. |

### TD4–TD5 Copying documents, document interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD4.1 | Make a copy of a course document | ✅ | Copy… works and is undoable (`landing_many_documents` 19–21, `course_er_ex2_move` 01–02). **P3E.1**: Explore cadrs lists the bundled **samples** (six course stand-ins, `documents_page::SAMPLES`) with thumbnails and details; **Open a copy** (details button, row menu, double click) adds an editable copy to Owned by me and opens it (`course_td_documents_samples` 01–07; `documents_page.rs::a_sample_copy_has_the_fixtures_volume`: 50·30·25 = 37 500 mm³). **P3E.5**: the drill stand-in ("Drill", Test Drive, `fixtures/drill_standin.cadrs`) is the seventh sample; exercise step 1 opens a copy of it (`course_td_ex1_drill` 01–02). |
| TD5.1 | Tabs: Part Studios, Assemblies, Drawings, imported files | ✅ | Part Studio, Assembly and Drawing tabs from "+" (`tabs_create_assembly` 01, 02, 08; `course_drw_create` 01). **Imported files (PDF, images, video) as tabs: out of scope** ("niche; out of scope by user decision 2026-09-29", image tabs; see the scope decision above). |
| TD5.2 | "+" adds a tab | ✅ | `tabs_create_assembly` 01–02, 08. |
| TD5.3 | Tab folders with a Home button | ✅ | **P3E.2**: "+" → **Create folder** makes "Folder N" right of the active tab, renamed in place; a folder is a tab with a folder icon, and a click **opens** it: the bar shows its tabs after a **Home** button and the folder's path (a breadcrumb; each crumb opens that folder). Tabs are **dragged** into a folder (a blue box), out onto Home or a crumb, and along the bar (a blue line); the tab menu's **Move to folder ▸** (folders, Top level, New folder); the folder's menu Open / Rename… / **Delete folder…**, whose dialog asks about its tabs (Delete folder only / Delete folder and tabs). Each is one undo step and survives a reload (`course_td_tab_folders` 01–18; `course_td_many_tabs` 08–11). Model: `cadrs_core::tab_tree` (`Document::tab_tree`, additive, empty without folders; `tests/tab_folders.rs`: one undo step each, reload, element ids and contents kept, references between tabs (instances, a drawing, a Derived feature and its linked copy) resolve after moves, with the Hexapod's and the block's closed-form volumes). |
| TD5.4 | Tab manager: vertical, searchable list | ✅ | **P3E.2** (`crate::tab_manager`, grown from P3G.3's minimal panel and P3F.3's scrolling list; fix round 1: a **docked full-height left panel** of 300 px pushing the lists right, two-line rows with thumbnails and the type in grey italic, the active row light blue with a dark-blue bar, Sort ▾ and Clear, a large preview at the bottom, folders renamed in their row, as `tab_manager_open-01.png`): the tab bar's leftmost icon opens a vertical list of every tab with its icon (the active one bold), a **search** field and **type filters** (All, Part Studios, Assemblies, Drawings; "2 of 6 tabs"), **folders as expandable rows** ("Plates (3)", tabs indented), **New folder** from the selection, **drag reorder** (a blue line) and drag into a folder (a blue box), click / Ctrl+click / Shift+click multi-select, a row menu (Move to document…, New folder from selection, Move to top level; a folder's Open in tab bar / Rename… / Delete folder…) and **Move to document…** (P3G.3) for the selection, a folder's tabs included (`course_td_tab_manager` 01–15; `course_td_many_tabs` 06–08, 11; `course_tips_move_tab` 06–07). |

## 2. Part Studios, Assemblies and Drawings

### TD6 Part Studio interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD6.1 | Several interrelated parts per studio | ✅ | P3.3: one sketch drives several parts, Merge scope, one feature on several parts (`course_ps2_parts_list` 01, `course_ps4_end_types` 06). |
| TD6.2 | Wheel zoom, right-drag rotate, middle-drag pan | ✅ | `viewport_orbit` 02–06; `camera.rs` tests `zoom_keeps_the_point_under_the_cursor`, `pan_moves_the_scene_with_the_pointer`. |
| TD6.3 | F fit, Shift+7 isometric | ✅ | `viewport_orbit` 07, 12; `keyboard_shortcuts` 07. |
| TD6.4 | Shortcut list from Help | ✅ | Help ▾ → Keyboard shortcuts or Shift+/ (`keyboard_shortcuts` 01–03; `shortcuts.rs::tabs_and_search`). |
| TD6.5 | View cube; camera and render options menu | ✅ | View cube, arrows, corners and view menu (`viewport_orbit` 08–11, `course_p6_cube_corner`). **P3E.3a**: the six render modes, Perspective, Zoom to window, Previous view, Named views (per tab) and Zoom to selection (`course_td_render_modes` 01–25); Section view (`course_td_section` 01–17). |
| TD6.6 | Measure and analysis tools | ✅ | Mass properties (P3.3, `course_x7_mass_options`). **P3E.3b**: Measure (readout and panel, Minimum and Maximum, in Part Studios and assemblies: `course_td_measure` 01–09; `measure_analysis.rs`), Analysis: draft analysis, curvature, curvature combs and zebra stripes (`course_td_analysis` 01–14). |
| TD6.7 | Persistent selection; empty click or Space clears | ✅ | Additive clicks, a click on empty space or Space clears (`viewport.rs`). **P3E.3a**: `course_td_selection` 01–07 (Space clears at 04, an empty click at 06). |

### TD7 Creating a part
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD7.1 | Extrude a planar face directly | ✅ | P3.3 (PS4.2): `course_ps4_end_types` 07–08; conformance `extrude_a_face`. |
| TD7.2 | Solid/Surface/Thin × New/Add/Remove/Intersect | ✅ | P3.3–P3.4: `course_ps4_surface_thin` 01–06, `course_ps5_boolean` 01–04; `remove_intersect_add_two_boxes`, `surface_and_thin`. |
| TD7.3 | Face of an existing part defaults to Add with that part in Merge scope | ✅ | P3.3 (PS5.2, PS5.4): Add is picked by itself on contact, with the touched parts in Merge scope, until a tab is clicked (`course_ps4_end_types` 07, `course_ps6_control_arm` 07; New when it no longer touches, `course_ps4_end_types` 09; test `merge_scope`). The exercise's face → Add → **New**: `course_td_ex1_drill` 03–04 (P3E.5). |
| TD7.4 | Dialog field order; "Features (n)" | ✅ | All fields in the course's order: Blind, Depth, Direction, Starting offset, Symmetric, Draft, Second end position, Merge with all, Merge scope (`course_ps4_end_types` 01, 03, 05, 09, 10; `course_ps4_draft`; `course_ps6_control_arm` 04). Rollback slider: `course_ps13_rollback_final`. Features (n) header: `course_ps3_filter` 01. |
| TD7.5 | New part in the Parts list, renamed there | ✅ | `course_ps2_parts_list` 05, `course_ps6_control_arm` 09–10. |

### TD8 Assemblies
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD8.1 | Hierarchy and motion; insert from same or other document | ✅ | Same document P3B.1, P3B.4; other documents P3G.1 (`course_er_insert_linked` 03–10). |
| TD8.2 | Mates through mate connectors; one mate per relationship | ✅ | P3B.2 (A6.1, A6.2): `course_asm_mate_connectors` 01–04; `assembly_solver.rs::each_type_leaves_its_dof`. |
| TD8.3 | Group | ✅ | P3B.3 (A13): `course_asm_ex2_pneumatic` 07–08; `a_group_keeps_relative_transforms`. |
| TD8.4 | Revolute with limits | ✅ | `course_asm_ex4_connectors` 09, 11 (the drag stops at the limit). |
| TD8.5 | Shift locks the hovered face | ✅ | A6.5: `course_asm_mate_connectors` 03. |
| TD8.6 | Insert an assembly via the Assemblies tab | ✅ | P3B.4 (A17.2): `course_asm_subassemblies` 16–17. |
| TD8.7 | Flip / Reorient; Solve | ✅ | A6.8, A6.11: `course_asm_ex2_pneumatic` 12; `course_asm_mate_dialog_options` 07–08. |
| TD8.8 | Standard content ISO 4762 M5 × 25 on two holes | ✅ | P3B.5: the library has ISO 4762 (M5, length 25; `standard_content.ron`) and batch placement on selected holes (`course_asm_std_batch` 01, 04). **P3E.5**: two ISO 4762 M5 × 25 on the manifold's two bolt hole edges, each with its Fastened mate, in a subassembly's occurrence (`course_td_ex1_drill` 16–18; `course_test_drive.rs` step 7: "Socket head cap screw M5 x 25" × 2 in the BOM). |
| TD8.9 | Standard content connectors; K hides them | ✅ | Standard content carries its connectors (A19); K hides connectors (`course_asm_ex4_connectors` 10). |

### TD9 Properties and BOM
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD9.1 | BOM panel | ✅ | P3B.6 (A20.1): `course_asm_bom` 01. |
| TD9.2 | Fill properties, add/reorder columns, export CSV | ✅ | `course_asm_bom` 09–11, 17; `course_asm_bom_template` 08 (Export to CSV); `assembly_bom.rs::csv_export_matches_the_golden_file`. |
| TD9.3 | Two-way BOM ↔ part properties | ✅ | A20.10: `course_asm_bom` 17–19; `assembly_bom.rs::bom_cells_and_properties_are_the_same_data`. |
| TD9.4 | Generate next part number; Switch to | ✅ | The BOM row menu's Generate next part number and Generate missing part numbers (`course_asm_bom_template` 09–10). **P3E.5**: the row menu's **Switch to <tab>** opens the row's Part Studio with its part selected, or its subassembly's tab (`course_td_ex1_drill` 20–22: the gasket's row inside the expanded CARBURETOR; disabled for standard content and items). |
| TD9.5 | Parts list → Properties dialog | ✅ | Parts list right-click → Properties… (`parts_list.rs:247`) with Part number, Description and Generate (`course_asm_bom_template` 11–12 "td9.5"). |

### TD10 Drawings
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD10.1 | Drawings linked to the model | ✅ | P3C.6 (D X11): `course_drw_ex3_update` 12d–15. |
| TD10.2 | Create drawing, ANSI_B_MM; custom templates | ✅ | P3C.1: "+" → Create Drawing… with built-in ANSI/ISO templates in every size and unit (ANSI_B_MM among them, `cadrs_drawing/src/template.rs:135-150`) and custom templates (`course_drw_create` 01–05, 10–11). The exercise picks ANSI_B_MM (P3E.5). |
| TD10.3 | Rename the tab | ✅ | `course_drw_ex2_assembly` 08, `tabs_create_assembly` 03–04. |
| TD10.4 | Insert view Front 1:2 + Right projected | ✅ | Front 1:2 + projected views of a part (`course_drw_ex1_ujoint` 04–05). **P3E.5**: an assembly's Front 1:2 and its projected Right view (`course_td_ex1_drill` 26–27; `course_test_drive.rs::step7_the_drawing`). |
| TD10.5 | Structured – Top level BOM anchored bottom-right at the title block | ✅ | Insert BOM with type Structured – Top level and a fixed corner (`course_drw_ex2_assembly` 10–11). **P3E.5**: a table's fixed corner snaps to the title block too (`snap_table_corner`, `snap_to_title_block`): a right-hand corner to the block's left edge (its corners, or level along it), a bottom corner onto its top (`course_td_ex1_drill` 28–28b; `assembly.rs::a_table_corner_snaps_to_the_title_block`). |
| TD10.6 | Height and depth dimensions; item-number callouts | ✅ | Dimensions on part views (`course_drw_ex1_ujoint` 16) and item callouts (`course_drw_ex2_assembly` 14–16). **P3E.5**: on an assembly's views, the height (148.00, Front) and depth (80.00, Right) and the Item No. callouts 1–3 (`course_td_ex1_drill` 29–31). |
| TD10.7 | Title-block fields linked to the model's Name and Part number | ✅ | The title block's Title and Number resolve from the sheet reference's Name and Part number through the property model (`cadrs_drawing/src/title_block.rs:95-110`, test `fields_resolve_from_properties`); parametric notes bound to properties (P3C.4, D X3, `course_drw_notes`). cadrs links the fields by default, so the course's "switch the field, then delete the unused annotations" isn't needed. The assembly's Name and Part number in the title block: `course_td_ex1_drill` 27 ("FUEL AND POWER TRAIN", PRT-000005; `course_test_drive.rs::step7_the_drawing`). |
| TD10.8 | Update from this workspace | ✅ | P3C.6 (D13.2): `course_drw_ex3_update` 13–15, 29. |

## 3. Collaboration and data management

### TD11 Sharing and collaboration
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD11.1 | Share with users, teams, links | out of scope | Sharing and permissions. |
| TD11.2 | Social cues, follow mode | out of scope | Multi-user collaboration. |
| TD11.3 | Comments, @mentions, action items, markups | out of scope | Collaboration. |

### TD12 Data management
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD12.1 | Versions and history panel; Restore from any point | ✅ basic | P3D.3: the basic History panel, whose Restore is an undoable entry (`course_insp_history_panel` 01–05; `history_log.rs::restoring_an_entry_reproduces_it_and_is_undoable`). The detailed panel (graph, legend, filters, columns) is **out of scope** ("niche; out of scope by user decision 2026-09-29"). |
| TD12.2 | Workspace shown as an open circle | ✅ | "Main" with an open circle (`TimelineMarker::Workspace`; `course_insp_history_panel` 01). |
| TD12.3 | Create version with a name | ✅ | P3D.3 / P3G.1: Create version (name, description) (`course_insp_history_panel` 06–07; `course_er_versions_in_document` 03–04; `a_version_is_immutable_and_persists`). |
| TD12.4 | Branch from a version → new workspace | ✅ | **P3E.4**: "Branch to create workspace…" on a version's menu; the branch is a copy of the version and opens (`course_td_branch_merge` 02–04; `workspaces.rs::a_branch_starts_as_its_version`). The exercise's branch: `course_td_ex1_drill` 32–33. |
| TD12.5 | Edit in the branch; Main unchanged | ✅ | **P3E.4**: `course_td_branch_merge` 05–09; `workspaces.rs::an_edit_in_a_branch_leaves_main_unchanged`. The exercise: the gasket 1 mm in the branch, 2 mm in Main (`course_td_ex1_drill` 34–36; `course_test_drive.rs::step9_gasket_volumes_in_each_workspace`). |
| TD12.6 | Merge into current workspace; per-tab replace or keep | ✅ | **P3E.4**: the merge dialog lists the changed tabs, each Replace (default) or Keep (`course_td_branch_merge` 11–14; `workspaces.rs::merge_replaces_exactly_the_chosen_tabs`); `course_td_ex1_drill` 37–38. |
| TD12.7 | Merge is a history entry; undo via Restore | ✅ | **P3E.4**: one "Merge from …" entry, undone by Restore (`course_td_branch_merge` 14–15; `workspaces.rs::a_merge_is_one_entry_and_restore_undoes_it`); `course_td_ex1_drill` 39–40. |
| TD12.8 | Priority: versions and restore first, then branches and merge | ✅ | Versions and Restore (P3D.3), then branches and merge (P3E.4). |

### TD13–TD14 Release management, what's next
| ID | Requirement | Status | Notes |
|---|---|---|---|
| TD13.1 | Revisions and approvals | out of scope | Release management (paid tier). |
| TD13.2 | Release candidates, part numbers for release, approvals, released drawing | out of scope | Release management. The drawing **revision table** is a plain table (P3C.8); Generate missing part numbers is P3B.6 (✅). |
| TD14.1 | Links to further learning | out of scope | Learning-site feature. |

## Exercise
P3E.5 runs the walkthrough on the drill stand-in (`course_td_ex1_drill`, every step through the
UI) and closed the small TD8.8, TD9.4, TD10.4–TD10.6 pieces.

| ID | Requirement | Status | Notes |
|---|---|---|---|
| Step 1 | Copy the Initial document | ✅ | The `drill_standin` sample opened as a copy (`course_td_ex1_drill` 01–02). |
| Step 2 | Extrude the manifold face New 2 mm → CARBURETOR_GASKET | ✅ | `course_td_ex1_drill` 03–05: 1676.648 mm³; `course_test_drive.rs::step2_the_gasket_is_the_mounting_face_2_mm_thick` (1676.6483). |
| Step 3 | Insert the gasket; Fastened with Shift-locked hole centres | ✅ | `course_td_ex1_drill` 06–09 (Flip puts it on the manifold rather than in it). |
| Step 4 | Insert the CARBURETOR assembly; Fastened, Flip/Reorient, Solve | ✅ | `course_td_ex1_drill` 10–15 (Flip; Reorient isn't needed: the hole pattern is symmetric). |
| Step 5 | Two ISO 4762 M5×25 screws; K | ✅ | `course_td_ex1_drill` 16–18. |
| Step 6 | BOM part number ↔ Properties | ✅ | `course_td_ex1_drill` 19–24; `course_test_drive.rs::step6_part_number_syncs_both_ways`. |
| Step 7 | Drawing ANSI_B_MM, Front + Right, structured BOM, dimensions, callouts, linked title fields | ✅ | `course_td_ex1_drill` 25–31; `course_test_drive.rs::step7_the_drawing`. |
| Step 8 | Share, follow mode, comment with markup | out of scope | Collaboration. |
| Step 9 | Version → branch → 1 mm gasket → merge | ✅ | `course_td_ex1_drill` 32–40; `course_test_drive.rs::step9_gasket_volumes_in_each_workspace` (1676.6483 / 838.3241 / 838.3241 / 1676.6483). |
| Step 10 | Revision table, Release | out of scope | Release management (the table itself: P3C.8). |

(The exercise rows aren't requirement IDs of their own; they aren't in the counts above.)

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | Auto-save, no Save button | ✅ | See TD1.1 (`reload_roundtrip` 03, `history_survives_save_and_reload`). |
| X2 | Camera controls, F, Shift+7, view cube, shortcut list | ✅ | `viewport_orbit` 02–12, `keyboard_shortcuts` 01, 07. |
| X3 | Persistent additive selection; Space clears | ✅ | See TD6.7 (`course_td_selection`). |
| X4 | Extrude from a planar face | ✅ | `course_ps4_end_types` 07–08. |
| X5 | Start screen: create, import, search, recent, folders, trash, details | ✅ | Create, search, recent, folders and trash; **P3E.1**: labels, the details panel, opening folders, samples and Import files… (`course_td_documents_labels`, `_details`, `_samples`, `_import`). |
| X6 | Tab folders and vertical tab manager | ✅ | See TD5.3 and TD5.4 (P3E.2: `course_td_tab_folders`, `course_td_tab_manager`, `course_td_many_tabs`; `tests/tab_folders.rs`). |
| X7 | Single-source part properties; BOM CSV export | ✅ | `course_asm_bom` 17–19, `course_asm_bom_template` 08; `bom_cells_and_properties_are_the_same_data`, `csv_export_matches_the_golden_file`. |
| X8 | Title-block fields linked to model properties | ✅ | See TD10.7 (`fields_resolve_from_properties`). |
| X9 | Versions, branches and merge | ✅ | Versions (P3D.3, P3G.1); branches and merge (P3E.4, `course_td_branch_merge`); the exercise's step 9 (`course_td_ex1_drill` 32–40). |

## Cross-stage rows assigned to stage 3E
These rows live in other gap lists and close with a P3E milestone. The milestone table names
them in its Covers column.

| Gap list | Row | Status there | Milestone | What closes it |
|---|---|---|---|---|
| derived-and-linking | ER1.2 Other documents browser: Labels location | ✅ (P3E.1) | P3E.1 | The labels model; the browser lists each label as a location of its documents (`course_td_documents_labels` 17–18) |
| derived-and-linking | ER8.5 Documents page → details panel for the new document | ✅ (P3E.1) | P3E.1 | Click-selects model and details panel (owner, description, labels, created/modified) (`course_td_documents_details` 01–07) |
| derived-and-linking | (note) full Tab manager | ER7.7 ✅ on the minimal one | P3E.2 (done) | Search, filters, folders, reordering (`course_td_tab_manager`) |
| essential-tips | T1.1 non-CAD files as tabs | 🟡 | out of scope | Image/PDF/video tabs are "image tabs" (niche; out of scope by user decision 2026-09-29); the rest is ✅ (drawing tabs P3C.1). Owner of that file should re-mark it. |
| essential-tips | T1.2 cope with ~40 tabs | ✅ (P3F.3, P3E.2) | P3E.2 | Overflow ▾ listing the tabs out of sight, folders, the full Tab manager (`course_td_many_tabs`: 60 tabs) |
| essential-tips | X1 Tab manager search, filters, reordering | ✅ (P3G.3, P3E.2) | P3E.2 | As above (`course_td_tab_manager`) |
| part-studios | PS2.9, X14 section view and render modes | → P3E.3 | P3E.3 | Render modes, section view |
| part-studios | PS2.11 Measure, Analysis | → P3E.3 | P3E.3 | Measure, Analysis |
| assemblies | A1.9 Measure, Analysis | 🟡 | P3E.3 | The same tools in assemblies (instances in assembly coordinates) |
| assemblies | A3.3, X15 Section view item | ✅ (disabled item) | P3E.3 | The triad and instance menus' Section view enabled |
| drawings | D2.2 mouse-mapping preference | ✅ (preference by P3E.3) | P3E.3 | A local mouse-mapping preference; drawings read it |
| inspection | IR5.5 feature menu → Section view | ✅ (disabled item) | P3E.3 | The feature menu's Section view enabled |

## What each exercise needs
The tour is one walkthrough on a large public document (DRILL HOTD, 187 features), with **no
numeric self-check**. It runs on a stand-in, `fixtures/drill_standin.cadrs` (mm), built by
`samples::drill` (P3E.5; `course_test_drive.rs::drill_fixture_is_current`). It ships as a bundled
sample ("Drill", Test Drive). Its contents, every part Aluminum 6061 with a part number:
- Part Studio **CARBURETOR**: **MANIFOLD** (Extrude 1, z 0..20), a 40 × 30 mm plate with a Ø20
  bore and two Ø5.5 bolt holes at x = ±15.5, all through. Its top face is the mounting face:
  40·30 − π(10² + 2·2.75²) = 1200 − 115.125π = **838.3241 mm²**. **CARBURETOR_BODY**
  (Extrude 2, a 22 × 30 block z −36..0 under it, clear of the bolt holes).
- Assembly **CARBURETOR**: MANIFOLD <1> fixed, CARBURETOR_BODY <1> fastened under it.
- Part Studio **DRILL BODY**: **DRILL_BODY**, a 160 × 80 × 90 block (z −90..0) with a Ø20 port
  and two Ø4.2 tapping holes on the manifold's pattern.
- Assembly **FUEL AND POWER TRAIN**: DRILL_BODY <1> fixed (the base part).

The scenario **`course_td_ex1_drill`** (43 frames) runs steps 1–7 and 9 through the UI:
1. Samples → Drill → Open a copy (01–02).
2. The mounting face → Extrude (Add is picked by itself) → **New**, 2 mm → renamed
   **CARBURETOR_GASKET**: 1676.648 mm³ (03–05).
3. The gasket inserted in CARBURETOR; Fastened on Shift-locked hole centres, Flip (06–09).
4. CARBURETOR inserted in FUEL AND POWER TRAIN (Assemblies tab); Fastened between the gasket's
   bore centre and the drill body's port centre (both faces Shift-locked), Flip, Solve (10–15).
5. Two ISO 4762 M5 × 25 on the manifold's bolt hole edges; K (16–18).
6. BOM: Generate next part number on the gasket's row (PRT-000006); **Switch to** from its row;
   its Properties show the number, a Description typed there shows in the BOM (19–24).
7. Drawing ANSI_B_MM of FUEL AND POWER TRAIN: Front 1:2 + Right; a Structured – Top level BOM
   snapped at the title block's left edge (fitted to the frame); height and depth; Item No.
   callouts; the title block shows the assembly's Name and Part number (25–31).
9. Version "FUEL AND POWER TRAIN COMPLETE" → branch "Alternate Gasket Thickness" → the gasket
   1 mm (838.324 mm³) → Main still 1676.648 → merge into Main replacing the CARBURETOR tab →
   838.324 → Restore the entry before the merge (the version marks it) → 1676.648 (32–40).

The unit test `crates/cadrs_core/tests/course_test_drive.rs` checks:
- the gasket volumes 1676.6483 (Main), 838.3241 (the branch; Main still 1676.6483), 838.3241
  (Main after the merge) and 1676.6483 (after the Restore), to 1e−6 relative, against the closed
  form 2·(1200 − 115.125π) and 1·(1200 − 115.125π) computed from the dimensions;
- that the BOM's Part number equals the part property after edits in both directions;
- the drawing: 2 views, a 3-row Structured – Top level BOM (DRILL_BODY, CARBURETOR, Socket head
  cap screw M5 x 25 × 2) snapped to the title block and within the frame, the 148.00 height and
  80.00 depth measured on the views, Item No. callouts 1–3, and the title block's Title and
  Number ("FUEL AND POWER TRAIN", PRT-000005).

## Stand-in substitutions
- **The document.** DRILL HOTD (187 features, many tabs) is replaced by the four-tab stand-in
  above; the gasket's face is the manifold's mounting face, sized so the course's volumes come
  out in closed form. The base part of FUEL AND POWER TRAIN is a plain DRILL_BODY block.
- **No scenario shortcuts.** Every step of `course_td_ex1_drill` is clicked through the UI,
  including the mates (implicit connectors on Shift-locked faces), Flip and Solve, the standard
  content and the drawing; no `Custom(...)` set-up command runs after the sample is copied.
- **Step 3 inserts the gasket beside the carburetor** (a click in the view) rather than at its
  studio position, so the Fastened mate visibly brings it onto the manifold.
- **Step 4's Reorient** isn't clicked: the stand-in's hole pattern is symmetric, so Flip alone
  lines the holes up.
- **Step 5's K** has nothing to hide: the stand-in has no explicit mate connectors and cadrs
  doesn't draw the fasteners' connectors after insertion, so the frame is taken after K.
- **Step 7's BOM** lists all six default columns, wider than the space left of the title block:
  placed there, its columns are fitted to the frame and the cells wrap (`fit_within`).
- **The gasket's material** (Nylon 6/6) is assigned in step 2 so its mass properties have a mass
  and centre of mass; the course leaves the part without one.
- **The drawing tab** is renamed "FUEL AND POWER TRAIN Drawing" (cadrs names a new drawing
  "Drawing 1", as Onshape does).
- **Step 9's Restore** is picked on the version's row ("FUEL AND POWER TRAIN COMPLETE"), which
  marks the entry before the merge (Main has no change between the version and the merge).

## Proposed milestones (stage 3E, revised 2026-09-29)
Every milestone ships `course_td_*` scenarios and a fresh-judge round against `test-drive/` and the
core courses' screenshots it reuses. The pass mark is ≥ 8.5, or ≥ 8.3 with only minor deltas;
at most 2 fix rounds. The earlier P3E.1–P3E.5 are kept in number. Their scope shrank: the file
tabs are out of scope, the done rows are dropped, and P3E.5 took the small drawing and BOM gaps.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3E.1 | **Documents page: labels, details, samples, import** | TD3.1 (Label, Import files…), TD3.3 (Labels), TD3.5, TD3.6 (open folder), TD3.7 (labels column, Type filter, grid), TD3.8, TD4.1, X5; **ER1.2**, **ER8.5** (derived-and-linking) | **Labels**: `DocumentMeta::labels` plus a library label list (name, colour). Create label… (Create ▾ and the details panel's "Create new label"), assign with searchable checkboxes in the details panel and the row menu, the sidebar Labels section filters by label, a labels column, and the Other documents browser's Labels location (ER1.2). **Row model**: click selects, double-click opens, Enter opens. **Details panel**: thumbnail, owner, editable description, labels, created by, created and modified, a versions list and where used (ER8.5). **Last opened**: click the name or double-click the thumbnail to open. **Folders**: open a folder (breadcrumb, Back). **Type filter** (All / Documents / Folders) and a **list/grid** toggle. **Samples**: an Explore/Samples list of the bundled stand-in fixtures, where "Open a copy" makes an editable document. **Import files…**: a STEP/STL file becomes a new document with a Part Studio holding its Import feature. Every action is a library command (undoable). Scenarios `course_td_documents_labels`, `course_td_documents_details`, `course_td_samples`, `course_td_import`. Unit tests: label create/assign/unassign/delete undo, redo and survive reload; filtering by a label returns exactly the tagged set (3 of 5 seeded); a sample's copy has the fixture's volume (e.g. the bracket sample's closed-form volume). |
| P3E.2 | **Tabs: folders, full Tab manager, overflow** | TD5.3, TD5.4, X6; **T1.2**, tips X1 (Tab manager search, filters, reorder); the full Tab manager noted in derived-and-linking | **Tab folders**: "+" → Create folder, drag tabs in and out, a folder tab opens its contents with a **Home** button back to the top, rename and delete (the delete asks about its tabs); stored in `Document` as an element-order tree (additive, old files load). **Tab manager**: search field, type filters (Part Studios, Assemblies, Drawings), drag reorder, folders as expandable rows; keeps P3G.3's multi-select and Move to document. **Overflow**: the tab strip scrolls (wheel, arrows) and an overflow ▾ lists hidden tabs; the active tab scrolls into view. Scenarios `course_td_tab_folders`, `course_td_tab_manager`, `course_td_many_tabs` (60 tabs). Unit tests: folder create/move/delete and reorder are single undo steps, survive save and reload, and keep element ids; a v5 document without folders loads unchanged. |
| P3E.3 | **Viewing and analysis tools** | TD6.5, TD6.6, TD6.7, X3; **PS2.9, PS2.11, X14** (part studios), **A1.9, A3.3, X15** section item (assemblies), **D2.2** (drawings), **IR5.5** Section view (inspection) | **Render modes**: Shaded, Shaded without edges, Shaded with hidden edges, Hidden edges removed, Hidden edges visible, Translucent. **Perspective** toggle. **Zoom to window**, and zoom to selection for any selection. **Named views**: save and restore the camera, stored per tab. **Section view** in Part Studios and assemblies: a plane or face, flip, offset, capped section faces; enabled in the view menu, the feature menu and the instance and triad menus. **Measure**: distance, minimum distance, angle, length, area, radius and diameter between picked entities, with a readout card. **Analysis**: draft analysis (angle bands against a pull direction), curvature display and zebra stripes. **Mouse-mapping preference** (Onshape / SolidWorks-like presets), read by viewports and drawings. Frames of Space and empty click clearing the selection. Scenarios `course_td_render_modes`, `course_td_section`, `course_td_measure`, `course_td_analysis`, `course_td_selection`. Unit tests: measure on a 100 × 60 × 25 box reads 100 / 60 / 25, face area 6000, adjacent faces 90°; the minimum distance between two boxes 15 apart reads 15; a Ø40 cylinder's radius reads 20; a section through a Ø40 cylinder caps with area 400π = 1256.6371; a 5°-drafted face falls in the 3–6° band. |
| P3E.4 | **Branches and merge** | TD3.7 (workspace name), TD12.4–TD12.8, X9 | On P3D.3's history log: **workspaces** as named heads (Main plus branches); "Branch to create workspace…" on a **version**'s menu only; a workspace switcher in the document header and the History panel; each workspace has its own entries and undo. "Merge into current workspace…" on another workspace, with a dialog listing the **changed tabs**, each set to replace (default) or keep. The merge is one history entry, undone by Restore. Documents-page rows show the last-opened workspace. The detailed graph stays out of scope. Scenario `course_td_branch_merge`. Unit tests: an edit in a branch leaves Main's state hash unchanged; a merge with "replace" copies exactly the chosen tabs (element-hash equality) and keeps the others; references from kept tabs to replaced tabs re-resolve (an assembly instance of a replaced studio keeps its part); Restore to the entry before the merge returns Main to its pre-merge hash; workspaces survive reload. |
| P3E.5 | **Test drive walkthrough and stand-in** (last) | TD8.8, TD9.4 (Switch to from a BOM row), TD10.4, TD10.5 (BOM snaps to the title block), TD10.6, TD4.1 (drill sample), exercise steps 1–7 and 9; the stage wrap-up | `fixtures/drill_standin.cadrs` (`samples::drill`; `drill_fixture_is_current`) as a bundled sample. The BOM row menu gets **Switch to**; a drawing BOM table's fixed corner **snaps to the title block's left edge**. `course_td_ex1_drill` runs steps 1–7 and 9 as listed above. `crates/cadrs_core/tests/course_test_drive.rs` checks the gasket volumes **1676.6483 / 838.3241 / 1676.6483 mm³** (Main, branch and merged Main, then restored) to 1e−6 relative against 2·(1200 − 115.125π) and 1·(1200 − 115.125π), in the right workspaces, plus the two-way Part number sync. The drawing has 2 views, a 3-row Structured – Top level BOM (CARBURETOR, the base part, the M5×25 screws ×2), dimensions and callouts, and the title block shows the model's Name and Part number. Every TD and X row is ✅ or out of scope. |

**Order:** P3E.1 → P3E.2 → P3E.3 → P3E.4 → P3E.5. P3E.4 must land before P3E.5 (step 9);
P3E.1–P3E.3 are independent of each other.

**P3E.1 built (2026-09-29, pre-judge).** Scenarios `course_td_documents_labels` (18 frames),
`course_td_documents_details` (18), `course_td_documents_samples` (7) and
`course_td_documents_import` (6); the milestone table's `course_td_samples` / `course_td_import`
are named `course_td_documents_samples` / `course_td_documents_import`. Labels are kept in
`folders.ron` next to the folders (additive `labels` field); the Details panel is built from
`cadrs_ui::doc_details` pieces rather than `FloatingPanel`, because it docks on the page's right
edge like Onshape's; the row menu's Properties… became Details….

**P3E.2 built (2026-09-30, pre-judge).** Scenarios `course_td_tab_folders` (18 frames),
`course_td_tab_manager` (15) and `course_td_many_tabs` (11). The tab tree is
`cadrs_core::tab_tree` (`TabTree { root, folders }` in `Document::tab_tree`, serde default, not
written when empty, so no schema bump): the stored order is read in a **normal form** (unknown ids
skipped; a tab it doesn't mention goes after its neighbour in the element list, in that tab's
folder, so a new or duplicated tab inside a folder stays there), and each command writes the tree
back and reorders `Document::elements` to the tree's order. Commands `CreateTabFolder`,
`RenameTabFolder`, `DeleteTabFolder { delete_tabs }` and `MoveTabItems`, all document-scope (one
undo step). The tab bar shows one level (`tab_folders::TabFolderView`), opens the folder of a tab
selected elsewhere, and keeps P3F.3's whole-tab scrolling; its new **▾** lists the tabs out of
sight. The Tab manager moved from `move_document.rs` to `tab_manager.rs`. Decisions: a folder opens
on a primary click (a right-click only opens its menu); drag onto Home moves to the top level, onto
a crumb into that folder; reordering in the manager is off while it is filtered or searched; a
selected folder's tabs are what Move to document moves.

**P3E.5 fix round 1 (2026-10-01).** A placed drawing BOM keeps within the frame
(`cadrs_drawing::assembly::fit_within`); the branch dialog names the version in its body; a merge
is its own rail row ("Merge from …"); the measure line only draws with the readout or the panel.
Optional, not done (the main session's decision): `course_td_branch_merge` 14c (the merged
assembly's gasket instance's mass properties; `workspaces.rs::the_merged_assemblys_gasket_instance_is_the_1_mm_part`
checks the value) and an assembly Curvature frame in `course_td_analysis`.

**P3E.5 built (2026-10-01, pre-judge).** `samples::drill` and `fixtures/drill_standin.cadrs` (the
seventh bundled sample), the BOM row menu's Switch to (`assembly::menu::switch_to_owner`), the
title-block snap of a drawing BOM's fixed corner (`cadrs_drawing::assembly::snap_table_corner`),
the scenario `course_td_ex1_drill` (43 frames) and `crates/cadrs_core/tests/course_test_drive.rs`
(5 tests). See "What each exercise needs" and "Stand-in substitutions".

**P3E.1 details:**
- Labels in `cadrs_core::library` (a `Label { id, name, colour }` list in the library index, and
  `DocumentMeta::labels`, serde default).
- `DocumentMeta::description`.
- Library commands `CreateLabel`, `SetLabels`, `DeleteLabel`, `SetDescription`.
- `Filter::Label(id)` and `Filter::Type`.
- The details panel as a `cadrs_ui` side panel (reuse `FloatingPanel` / `ActionRow`).
- The row selection model in `cadrs_ui::Table` (Select on click, Activate on double-click). This
  changes `landing_many_documents` 03–04; re-bless them.
- The samples index: the fixture list with its thumbnails, copied into the store on "Open a copy".

**P3E.2 details:** tab folders as a tree over the element order (`Document::tab_tree`, additive);
a folder's "tab" is a pseudo-element with no geometry. The Tab manager extends
`move_document.rs`'s panel, or moves it to its own module `tab_manager.rs`. Overflow is a scroll
container in the tab strip.

**P3E.3 details:**
- A render-mode resource, with per-mode materials and edge visibility.
- Perspective: a projection swap on the camera. The fit and zoom maths need a perspective path.
- Section view: a clip plane in the render, with caps from a half-space boolean on the kernel
  thread (cached per plane). This reuses the drawings' `section.rs` cut, not the drawing
  projection.
- Measure: a panel over kernel queries. The minimum distance needs `BRepExtrema_DistShapeShape`
  in the fork (a new kernel op with a conformance case).
- Draft analysis: face normal against the pull direction, as a per-vertex colour.
- Zebra: a stripes shader over the view direction.
- Curvature: combs on edges.

**P3E.4 details:**
- `HistoryLog` gets `workspaces: Vec<Workspace { id, name, from_version, head }>`, entries tagged
  by workspace, and `state_at` per workspace. It stays schema-compatible, since existing logs are
  Main only.
- Merge: a per-tab element replace (with its linked copies) as one `MergeWorkspace` entry.
- Persistent part ids across branches (the same ids, because the branch starts from a copy of
  the version) keep the assembly instances valid.

**P3E.5 details:** the stand-in built in `samples::drill`, like `samples::piston` in P3G.5. It uses
a scenario shortcut only where the mate code can't be reached by clicking; document any such
shortcut in "Stand-in substitutions".

**Out of scope:**
- TD1.2, TD2.1, TD3.10, TD11.*, TD13.*, TD14.1 and exercise steps 8 and 10: collaboration,
  accounts, paid tiers, release management, the learning site.
- The sharing and cloud parts of TD3.1, TD3.3, TD3.6 and TD3.8.
- The image, PDF and video tabs of TD3.1 and TD5.1: image tabs, "niche; out of scope by user
  decision 2026-09-29".
- The detailed Versions and history panel in TD12.1: niche, by the same user decision.

## Risks
- **Branches vs the user's "detailed Versions panel" exclusion.** P3D.3 listed branches among the
  panel's out-of-scope details. This plan keeps branching and merging in scope, without the
  graph, because the course's data-management lesson and exercise step 9 need them. If the user
  disagrees, drop P3E.4 and mark TD12.4–TD12.7 out of scope. Step 9 then becomes version → edit →
  Restore, with the same volumes.
- **Merge semantics.** Tab-level replace is simple. References between tabs must re-resolve: an
  assembly in Main that references a studio replaced from the branch, and linked copies
  (P3G.1 `Document::linked`) that the replaced tab brings. Part ids must stay the same across
  branches.
- **P3E.3 is the largest milestone.** It has 6 render modes, perspective, a section with caps,
  Measure with a new kernel op, and three analyses. If the judge rounds run long, split it into
  P3E.3a (render modes, perspective, named views, section) and P3E.3b (Measure, Analysis, mouse
  preference).
- **Minimum distance** needs a new OCCT binding (`BRepExtrema_DistShapeShape`) in the fork, which
  the other workers share: pull first, keep it additive, and add a conformance case.
- **The row selection model change** (click selects instead of opening) touches every
  documents-page scenario that opens by a single click (`landing_*`, `course_er_*` "move-doc
  open" paths). Re-bless those goldens in one batch.
- **Perspective** breaks assumptions in the fit, zoom-to-cursor and picking code that was written
  for orthographic. Keep orthographic the default, and test picking in both.
- **The drill stand-in** needs an assembly of an assembly with standard content and a drawing. It
  is a large fixture, so keep it small (4 parts) and regenerate it from `samples::drill`.
- **Shared files.** P3E.2 (tab tree) and P3E.4 (workspaces) touch `document.rs` and
  `history_log.rs`, which worker A and worker D (3H) also edit. Keep the changes additive and
  merge `main` before each.
