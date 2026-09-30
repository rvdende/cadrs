# 10 Essential Onshape Tips: gap analysis (2026-09-27, refreshed 2026-09-29)

This maps [essential-tips.md](essential-tips.md) against the cadrs code on branch `phase3`.
It was written at `ff7616b`, **refreshed at `42a136f`** (after stages 3A, 3B, 3C and 3D were
merged), at **`f1e161a`** (P3F.2), after **P3F.3–P3F.4** (on `f12f96e`) and after merging stage
3G (P3G.1–P3G.3). ✅ done · 🟡 partial · ❌ missing · **→ 3G** moved to stage 3G ·
**out of scope** (cloud and multi-user collaboration, sharing and permissions, release management
and paid tiers, Onshape account and learning-site features; and, by user decision 2026-09-29,
"niche; out of scope": configurations, image tabs, full-round/conic/curvature fillets, and a
detailed Versions and history panel with graph, legend, filters and branches).

Stage 3F covers this article and [intro-to-parametric-cad.md](intro-to-parametric-cad.md) with
**one milestone numbering**: this file defines **P3F.1–P3F.3**,
[intro-to-parametric-cad-gaps.md](intro-to-parametric-cad-gaps.md) defines **P3F.4–P3F.6**.
**P3F.1 (cross-document references, version pins, Move to document, Where used across documents)
moved to stage 3G "Derived and linked documents"**, as P3G.1–P3G.3
([derived-and-linking-gaps.md](derived-and-linking-gaps.md)).
Related plans: [intro-to-part-studios-gaps.md](intro-to-part-studios-gaps.md) (P3.x),
[intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md) (P3B.x),
[intro-to-drawings-gaps.md](intro-to-drawings-gaps.md) (P3C.x),
[inspection-and-repair-gaps.md](inspection-and-repair-gaps.md) (P3D.3 history and versions),
[test-drive-gaps.md](test-drive-gaps.md) (P3E.2 tabs).

**Summary (refresh):** the article is about the document and data model. cadrs now has:
- documents with Part Studio, Assembly and Drawing tabs;
- live instances between tabs (P3B.1);
- a persisted document history with named versions and an undoable Restore (P3D.3);
- feature suppression (P3.9);
- **import and export (P3F.2)**: STEP and IGES in, with the assembly choice; STEP, IGES, STL,
  OBJ, and DXF/DWG of a sketch or a face out;
- **the scale budgets and cosmetic threads (P3F.3)**: 250 features and 10 parts in a studio and
  40 tabs in a document measured, the size notice, tab overflow (a scrolling strip and the Tab
  manager's list), a scrolling feature list, and tapped holes' threads drawn, not modelled.

Nothing of this article is left in 3F. Cross-document references and Move to document were
built in stage 3G (P3G.1–P3G.3, the former P3F.1).

**Counts (23 T IDs + 7 X IDs = 30).**
- **At the analysis (`ff7616b`):** ✅ 0 · 🟡 3 · ❌ 25 · out of scope 2 (T: 0 / 2 / 19 / 2; X:
  0 / 1 / 6 / 0).
- **Refreshed after P3F.2 (`f1e161a`):** ✅ 16 · 🟡 5 · ❌ 2 · → 3G 5 · out of scope 2.
  - T: 12 / 3 / 1 / 5 / 2.
  - X: 4 / 2 / 1 / 0 / 0.
- **Refreshed after P3F.3:** ✅ 20 · 🟡 3 · ❌ 0 · → 3G 5 · out of scope 2.
  - T: 15 / 1 / 0 / 5 / 2 (T1.2, T4.2, T10.2 done).
  - X: 5 / 2 / 0 / 0 / 0 (X7 done).
  - T7.5, X1 and X3 are 🟡 because their cross-document parts are in 3G.
- **After merging stage 3G (P3G.1–P3G.3):** ✅ 27 · 🟡 1 · ❌ 0 · out of scope 2.
  - T: 21 / 0 / 0 / 2 (T1.3, T3.1–T3.3, T7.3 and T7.5 done in 3G).
  - X: 6 / 1 / 0 / 0 (X3 done in 3G; X1 waits for the Tab manager's search and reordering, P3E.2).
- **After P3F.6 (checked against 3G's delivered work):** ✅ 28 · 🟡 0 · ❌ 0 · → 3G 0 · out of
  scope 2 (T6.1, T7.4).
  - T: 21 / 0 / 0 / 0 / 2. Every former "→ 3G" row is delivered: T1.3 (P3G.3 Move to
    document), T3.1–T3.3 (P3G.1–P3G.2 version references, update, Reference manager), T7.3 and
    T7.5 (P3G.1 version pins, Change to version), P1.3's cross-document part search (P3G.1's
    Insert dialog "Other documents"; see [intro-to-parametric-cad-gaps.md](intro-to-parametric-cad-gaps.md)).
  - X: 7 / 0 / 0 / 0: X1 is ✅ (tabs of every kind, moved between documents by P3G.3); the Tab
    manager's search and reordering, which the row doesn't ask for, are P3E.2.

## Tips
| ID | Requirement | Status | Notes |
|---|---|---|---|
| T1.1 | A document holds Part Studios, Assemblies, Drawings and non-CAD files as tabs | ✅ | Part Studio, Assembly (P3B) and Drawing (P3C) tabs. Image tabs: niche; out of scope by user decision 2026-09-29. PDF and video tabs are treated the same way (an orchestrator extension of that decision, flagged for the user in PROGRESS.md). |
| T1.2 | Cope with ~40 tabs (scroll or overflow) | ✅ | P3F.3: the tab strip scrolls sideways (the wheel over it; `scale_ui`), the selected tab scrolls into view, and the **Tab manager** lists every tab to switch to one out of sight (P3G.3's panel, which scrolls). P3F.6 (P3F.3–P3F.4 judge): the strip scrolls a whole tab per wheel notch or chevron click, with a chevron and a fade at each end that has more tabs, so no clipped stub tab shows bare. A 40-tab document reads in 0.6 s (release) and switches tabs in 2 ms (`tests/scale.rs`); `course_tips_scale` 06–09. **P3E.2** adds tab folders (the strip shows one level), a **▾** listing the tabs out of sight, and the full Tab manager (search, type filters, folders, drag reorder); a 60-tab document in `course_td_many_tabs` 01–11. |
| T1.3 | Move a tab to another document | ✅ | **P3G.3**: every tab's menu → **Move to document…** (a new or an existing document; the tabs it references go with it or are linked back), and several tabs from the Tab manager (`course_tips_move_tab`, `course_er_move_to_document`; `cadrs_core/tests/move_document.rs`). |
| T2.1 | Same-workspace references update automatically | ✅ | P3B.1: assembly instances are drawn and measured from their studio's current rebuild (`assembly::instance_parts`); drawings wait for Update on purpose (P3C.6, D13.2). |
| T3.1 | Cross-document references point at a version | ✅ | P3G.1: another document is always inserted at a version (its newest, or one picked in the version graph), kept as a frozen copy (`course_er_insert_linked` 06–14; `external_refs.rs::an_instance_of_a_version_keeps_its_volume_after_the_source_changes`). |
| T3.2 | The referencing side chooses when to update | ✅ | P3G.2: a newer version shows a badge; nothing changes until Update to latest, Selective update or Update all in the Reference manager (`course_er_update_linked` 02–06; `link_update.rs::update_to_latest_brings_the_new_version_and_undo_restores_it`). |
| T3.3 | Store `(document, version, element, part)`; update-to-latest action | ✅ | The stored reference (P3G.1: `cadrs_core::external::SourceRef` on the instance, the part in its source; `course_er_insert_linked` 10); Update to latest (P3G.2, `course_er_update_linked` 05–06). |
| T4.1 | Part Studios for shape; motion and instancing in assemblies | ✅ | P3B.1–P3B.3: instances, mates, motion. |
| T4.2 | Responsive at 1–10 parts and < 250 features; gentle warning beyond | ✅ | P3F.3: a generated studio of 250 features and 10 parts (`samples::scale`) rebuilds after an edit of the last feature in 20 ms CPU (release) and of the first plate in 1.95 s CPU (< 3 s), frames stay < 50 ms during the background rebuild (43.9 ms max, dev build), and past 250 features or 10 parts a gentle notice sits under the feature list's header (`features-scale-notice`, with Onshape's advice on hover). The feature list scrolls. `tests/scale.rs`, `course_tips_scale`. |
| T5.1 | One mate per part pair | ✅ | A6.2 (P3B.2): each mate type carries all its DOF (Fastened, Revolute, Slider, Cylindrical, Pin slot, Planar, Ball, Parallel, Tangent, Width). The tip is guidance; nothing blocks a second mate, as in Onshape. |
| T6.1 | Share instead of export | out of scope | Sharing and permissions. Export itself: P3F.2 (✅), P3C.7. |
| T7.1 | Version = named, immutable snapshot | ✅ | P3D.3: `history_log::Version` points at an entry of the append-only log. |
| T7.2 | All tabs versioned together | ✅ | P3D.3: a version is of the whole document (`document_at_version`). |
| T7.3 | References target versions | ✅ | P3G.1: assembly instances and drawing views reference a named version of another document or of this one (`course_er_versions_in_document` 06–12, `course_drw_version_reference` 04–09). |
| T7.4 | Versions aren't a release process | out of scope | Release management. |
| T7.5 | Create version (name, description), read-only view, version pins | ✅ | Create version and the read-only view (P3D.3; the Create version dialog P3G.1, `course_er_versions_in_document` 03–04); version pins (P3G.2: Pin reference, `course_er_pinning` 02–08). |
| T8.1 | Import option by use: keep an assembly, or flatten into one Part Studio | ✅ | P3F.2: the Import dialog's **Part Studio (flatten)** / **Keep assembly structure** (`import_dialog.rs`, `cadrs_core::import`). `course_tips_import_step` 03, 06; `tests/exchange.rs` (2-part assembly → 3 instances at the file's placements; flattened → parts where the instances were). |
| T8.2 | Neutral formats: Parasolid, STEP, IGES; STEP first, IGES later | ✅ | P3F.2: STEP and IGES import and export through OCCT XDE (fork `277175f`, `a475208`). Parasolid is proprietary, not offered (the dialog says so). |
| T9.1 | Restore any earlier state | ✅ | P3D.3: Restore on any history entry. |
| T9.2 | Restore applies across all tabs at once (document-level) | ✅ | P3D.3 `RestoreDocument` (a whole-document step). The "changes by all users" part is out of scope (multi-user). |
| T9.3 | Persistent history; Restore is itself an undoable entry | ✅ | P3D.3: `<store>/<id>/history.ron`, append-only; Restore is a command. |
| T10.1 | Suppress cosmetic, heavy features | ✅ | P3.9: Suppress/Unsuppress (`commands/list.rs` `SetSuppressed`, feature menu). |
| T10.2 | Suppressed features skipped but kept; threads cosmetic by default | ✅ | P3F.3: a suppressed feature is left out of the rebuild (nothing is recomputed, the cube's cached result stands) and kept greyed in the list; a Ø10 × 20 hole suppressed gives the 50 mm cube's 125 000 mm³ back and unsuppressed 125 000 − π·25·20 (`tests/scale.rs`). Tapped holes are cosmetic: the solid is the plain tap drill (no helical faces), the thread is drawn on it (a ¾ circle of the major diameter at the entry and a helix at the pitch on the wall, `threads_ui`) and named in the callout ("M10x1.50 ↧ 20 mm") and in drawings (P3C.8). `course_tips_suppress_threads`. |

## Exercises
None (the article has no exercises, quiz or self-check). Each milestone ships `course_tips_*`
scenarios instead.

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | One document, many mixed tabs; tabs move between documents | ✅ | Mixed Part Studio / Assembly tabs; Drawing tabs (P3C.1); Render Studio tabs (P3F.6); tabs move between documents (**P3G.3**: Move to document…, one tab or several from the Tab manager; `course_tips_move_tab`, `course_er_move_to_document`, `cadrs_core/tests/move_document.rs`). **P3E.2**: the Tab manager's search, type filters, folders and drag reordering ([test-drive-gaps.md](test-drive-gaps.md) TD5.4; `course_td_tab_manager`), and tab folders (TD5.3; `course_td_tab_folders`). |
| X2 | Live in-document references | ✅ | P3B.1. |
| X3 | Named immutable versions; version-pinned cross-document references with update | ✅ | Versions (P3D.3), version-pinned references (P3G.1) and their update (P3G.2: the Reference manager and Update all, `course_er_update_linked`, `course_er_update_all`). |
| X4 | Persisted document history with restore | ✅ | P3D.3. |
| X5 | Import STEP (later IGES) with an assembly choice | ✅ | P3F.2 (below). |
| X6 | Feature suppression | ✅ | P3.9; P3F.3 adds the rebuild-skip checks (`tests/scale.rs`). |
| X7 | Scale targets: ~40 tabs, 250 features, 10 parts per studio | ✅ | P3F.3 (T1.2, T4.2 above). |

## Proposed milestones (stage 3F, part 1)
Every milestone ships scenarios and a fresh-judge round (≥ 8.5). The article has no screenshots, so
the judges score against the requirement text and the related Onshape reference screens in
`reference/onshape/`.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3F.1 | **Moved to stage 3G** (2026-09-29): now P3G.1 (references, Other documents), P3G.2 (updates, pins, where used) and P3G.3 (Move to document) in [derived-and-linking-gaps.md](derived-and-linking-gaps.md). Original plan: **Cross-document references, version pins, Move to document** | T1.3, T3.*, T7.3, T7.5 (pins), X1 (move), X3 (pins); also A2.3 "Other documents" (assemblies), D2.10 "Move to document", TD3.8 "where used", P1.3 (search) | A reference type `ExternalRef { document, version, element, part }` for instances and derived parts. Insert dialogs gain **Other documents** (search across the library, pick a version, newest by default). A referenced document edit plus a new version shows "Update available" on the instance; **Update** (to latest version) re-pins it; until then nothing changes. **Move to document…** moves a tab, creates a version in the target, and rewrites references in the source into pinned external refs. "Where used" lists the referencing documents. Unit tests: pinned geometry hash is unchanged after the source edit; update changes it to the new version's; move keeps the assembly's world transforms. Scenarios `course_tips_cross_document`, `course_tips_move_tab`. |
| P3F.2 | **Import and export** ✅ (judge 8.5; its deltas fixed in P3F.3) | T8.*, X5; also P3.2–P3.4, X7 (parametric CAD), TD3.1 (import), A X15 "Export" | See "P3F.1–P3F.2 status" below. |
| P3F.3 | **Suppression checks, cosmetic threads, scale targets** ✅ (judge 8.5) | T1.2 (budget), T4.2, T10.*, X6, X7 | Suppressing a feature skips it in rebuild and restores the downstream volume (unit test: suppressing a Ø10 × 20 hole in a 50 mm cube gives V = 125 000 exactly; unsuppress gives 125 000 − π·25·20). Tapped holes (P3.6) are **cosmetic by default**: no helical geometry, a thread display on the face and the thread in the callout (feeds P3C.8's thread display). Performance scenario `course_tips_scale`: a studio with 250 features and 10 parts rebuilds from an edit of the last feature in < 100 ms and from the first sketch in < 3 s (release build), without freezing the UI (frame time < 50 ms during the background rebuild); a document with 40 tabs opens in < 2 s and switches tabs in < 100 ms. A gentle notice appears in the feature list header past 250 features or 10 parts. |

## P3F.1–P3F.2 status
**P3F.1: not built here; moved to stage 3G** (P3G.1–P3G.3), where it was built: T1.3, T3.1–T3.3,
T7.3, T7.5's pins, X1's tab moves and X3's pins are done there (Other documents in Insert, the
tab menu's **Move to document…**, the Reference manager and Where used across documents).

**P3F.2: built.**
- **Kernel.**
  - `Kernel::import_model` and `Kernel::export_model`, with the types in
    `cadrs_kernel::exchange`, through the fork's new XDE bindings (`opencascade::xde`, fork commit
    `277175f`; `a475208` adds the Windows link libraries).
  - Import gives each distinct part once as a body, with its product name. It also gives every
    occurrence: its part, its placement and its instance name.
  - Imported shapes that aren't valid are healed with OCCT's `ShapeFix_Shape`.
  - Export writes parts with product names. With instances, it writes one assembly of them. IGES
    writes solids as MSBO solids, so a round trip keeps the volume.
  - Mesh files are written in Rust from the tessellation: `write_stl_binary`, `write_stl_ascii`
    and `write_obj`. `mesh_volume` computes the divergence-theorem volume.
  - Conformance case: `exchange_round_trips`.
- **Core.**
  - `cadrs_core::import`:
    - `ImportFeature`: a new feature kind holding the file's text, rebuilt through the kernel in
      `rebuild::kernel_ops::import`.
    - `ImportPlan`.
    - `import_elements`: a Part Studio, plus an Assembly with instances at the file's placements
      when the structure is kept.
    - `ImportFile`: one undo step.
    - `imported_document`.
  - `rebuild::exchange`:
    - `plan_import` and `export_files` on the kernel thread.
    - STEP and IGES, optionally as an assembly of instances.
    - STL (binary or text) and OBJ, with chord and angle tolerance and units.
    - Y axis up for all of these.
    - Individual files.
  - `dxf_export`: a sketch or a planar face as a flat page, written by the **drawings' DXF
    writer** (`cadrs_drawing::dxf`), so cadrs has one DXF writer. DWG goes through the drawings'
    external converter.
- **App.**
  - Import:
    - **Create ▸ Import files…** on the documents page makes a new document.
    - **+ ▸ Import…** in a document adds new tabs.
    - Both use the in-app file picker, then the Import dialog (`import_dialog.rs`).
  - Export (`export_dialog.rs`, the drawings' dialog layout):
    - Opened from the Parts list's **Export…**, an instance's **Export…** and the tab menu's
      **Export…** (a studio's parts, or an assembly's instances with "Keep assembly structure").
    - **Export as DXF/DWG…** on a sketch (feature list and view) and on a planar face (view).
    - Formats: STEP, IGES, STL, OBJ, DXF and DWG.
- **DWG licensing decision.** There is no permissively licensed DWG library in Rust. ODA's
  libraries are proprietary, and LibreDWG is GPL-3. So cadrs never links one: it writes DXF and
  runs a converter the user has installed on `PATH` (LibreDWG's `dxf2dwg` or the ODA File
  Converter) as a separate process. This is the same rule as drawings (P3C.7,
  `cadrs_drawing::dwg`). Without a converter, the DWG button is disabled and the dialog says what
  to install.
- **Tests.**
  - `cadrs_core/tests/exchange.rs`:
    - STEP export → import of the Control Arm: V = 368 749.705 mm³ ± 1e−3.
    - A 100 × 60 × 25 box as binary STL, text STL and OBJ: 12 triangles each, and a mesh volume
      of 150 000 mm³ (also 150 000 / 25.4³ in inches).
    - A two-part STEP assembly (a box twice, one turned 90° and moved, and a pin) imports as 2
      parts and 3 instances at the written placements (1e−9). Flattened, the turned box's
      centroid is at (170, 60, 17.5).
    - IGES keeps the box's area and volume (1e−3).
    - Planar faces as DXF: a box top is 4 lines, a cylinder top is 1 circle.
    - The bracket-pair fixture.
  - `dxf_export` unit test: a sketch rectangle with a circle is 4 LINEs and 1 CIRCLE, read back
    with the drawings' DXF reader.
  - `import` unit tests: names, counts, and one undo step.
  - Kernel `exchange` unit test and the conformance case.
- **Scenarios.**
  - `course_tips_import_step` (01–08).
  - `course_pcad_export` (01–11).

**Not done in P3F.2:**
- **XCAF is bound**, so the assembly structure comes with STEP.
  - Nested subassemblies come in as one level of instances, with their placements composed.
  - Colours and layers are not read.
- The import dialog has no preview.
- **STL/OBJ import:** not required.
- **Parasolid:** proprietary, not offered.

**API for stage 3G** (the P3F.1 brief asked for a documented external-ref and version-resolution
API; 3G owns it now). What already exists for 3G to build on:
- `HistoryLog::load(store, document)`;
- `HistoryLog::versions()`;
- `HistoryLog::document_at_version(version) -> Option<Document>`.

The pattern `Document::standard_content` uses is a fitting model for pinned references: a copy
of the source element, held by the document, that `Document::element` resolves under its own id.

## P3F.3–P3F.4 status
**P3F.3: done** (judge 8.5, with P3F.4). P3F.4 (variables) is in
[intro-to-parametric-cad-gaps.md](intro-to-parametric-cad-gaps.md), "P3F.3–P3F.4 status".
- **Suppression** (T10, X6): `cadrs_core/tests/scale.rs`:
  - a Ø10 × 20 flat hole (a circle cut 20 down) in a 50 mm cube: 125 000 − π·25·20 =
    123 429.204 mm³; suppressed, the cube's 125 000 (nothing recomputed: the cube's cached result
    stands, `computed == 0`); unsuppressed, the hole again;
  - the same with a Hole feature (Ø10 Blind 20 with its 118° point).
- **Cosmetic threads** (T10.2): tapped holes are the plain tap drill: an M10×1.5 hole 20 deep with
  15 of thread is 8 faces (the cube's 6, the drill's wall and point) and the drill's volume;
  its thread (major Ø10, 15 long, from the entry) comes from `views::threads`, which P3C.8's
  drawing thread display already uses, and the 3D view draws it (`threads_ui`: the ¾ circle of
  the major diameter at the entry and a helix at the pitch on the wall). The callout (the
  feature's name, "M10x1.50 ↧ 20 mm") names the thread. `course_tips_suppress_threads` 01–05.
- **Scale** (T1.2, T4.2, X7), a generated fixture (`samples::scale`): 10 plates, each a Variable
  (`#t_N`, its thickness), a sketch and an extrude, and 11 blind holes each a sketch and an
  extrude: 250 features (120 of them part features), 10 parts; and a 40-tab document with it as
  the first tab. Timings on this thread's CPU time (other workers share the machine), release
  build (`cargo test --release -p cadrs_core --test scale`):
  - from scratch: 2.95 s CPU (3.7 s wall);
  - the last feature edited: **20 ms CPU** (58 ms wall; budget 100 ms), 1 feature computed;
  - the first plate's sketch edited: **1.95 s CPU** (3.2 s wall under load; budget 3 s), the 120
    part features below it computed;
  - the 40-tab document read: **0.6 s** (budget 2 s); a tab switch (a small studio's rebuild):
    **2.3 ms** (budget 100 ms).
  - In the app (`course_tips_scale`, dev build, `perf-*.txt`): opened with the interactive
    rebuild budget, the tabs and the 250-row list show at once and the rebuild runs in the
    background (frames max 70 ms, median 19 ms); **#t_1 changed in the Variable table**, the
    120-feature background rebuild (3.6 s) never freezes a frame (**max 43.9 ms**, median 17 ms);
    the last hole's depth edited: 59 ms; a tab chosen from the Tab manager: 2.1 ms rebuild, frames
    max 21 ms. The golden test checks these (8× in a dev build).
- **The notice**: past 250 features or 10 parts, `features-scale-notice` under the feature list's
  header ("Large studio: 251 features, 10 parts. Consider splitting it.", Onshape's advice on
  hover). The feature list now scrolls (a 250-row list in its pane, with a slim scrollbar).
- **Tabs**: the strip scrolls sideways with the wheel; the selected tab is kept in view; the Tab
  manager button (left of the tabs) opens a menu of every tab. Frames 06–09.

**P3F.2 judge deltas, fixed:**
1. `course_tips_export_assembly` (01–10): Export… from the Parts list (IGES), from an instance,
   and from the assembly tab with Keep assembly structure (STEP), then + ▸ Import… of that STEP
   (an assembly of 2 parts with 3 instances, back as an assembly) and of the IGES with "File is Y
   axis up".
2. Keep assembly structure places each part in its Part Studio where its first occurrence is
   (`ImportFeature::at_first_occurrence`), the instances relative to it (`tests/exchange.rs`: the
   shaft/pin at its lift, not at the origin; `course_tips_import_step` 05).
3. DXF extents: `dxf_export::fit` returns the geometry's true min and max, and the DXF writer's
   `$EXTMIN`/`$EXTMAX` and view come from `cadrs_drawing::dxf::extents` (a page of strokes only
   spans its geometry; a drawing sheet its paper). Test: a sketch from (−55, −25) to (60, 35).
4. `course_pcad_export` empties its folder first (`Custom("clear-dir …")`, only under
   `target/`): "Part Studio 1.stl", not "(2)".
5. DXF tests assert the rectangle's corners, the circle's centre and radius, `$INSUNITS` 4 (mm);
   the odd `1 - 1` is gone; new tests: Y axis up (the box's Y extent is 25) and individual files
   (2 for the Control Arm, named for their parts, STEP and STL).
6. The Custom chord and angle fields are checked as typed (why under each field, Export disabled
   until both fit; `tolerance_error`); the face and sketch menus have an icon column with
   `file-export` on Export as DXF/DWG…; the Import dialog has **File is Y axis up**
   (`import::file_is_y_up`, parts and instances turned +90° about X); this header's P3F.2
   refresh is `f1e161a`.

## P3F.6 status (this article's rows)
P3F.6 (rendering and images) is defined in
[intro-to-parametric-cad-gaps.md](intro-to-parametric-cad-gaps.md). For this article it adds the
Render Studio tab kind (T1.1, X1), and, from the P3F.3–P3F.4 judge:
- the tab strip scrolls a whole tab per wheel notch (or per 60 px of a touchpad) and per chevron
  click, with a chevron and a fade at each end that has more tabs (`course_tips_scale` 06–09);
- `course_tips_scale` 04 shows the edited hole: the last plate seen from below, where its pockets
  are (the fixture drills them up from the underside);
- `course_tips_export_assembly` exports the Bracket as IGES with Y axis up, so importing it back
  as Y-up lands it upright (10);
- tab `Name`s are unique (a later tab whose name reads the same gets its kind after it:
  `tab-bracket-pair-assembly`), so `course_tips_import_step` and `course_tips_export_assembly`
  click tabs by name, not by place.

## Risks
- **Version storage.** Cross-document pins need versions to be cheap to store and load; P3D.3's
  snapshot design must allow loading a version read-only without rebuilding the whole history
  (stage 3G).
- **Moving tabs rewrites references** (stage 3G).
- **STEP/IGES fidelity.** Imported bodies have no feature history and may be invalid. P3F.2 heals
  invalid shapes with `ShapeFix_Shape`, and a failure shows on the Import feature's row (red,
  with the reason). XCAF is bound in the fork now (P3F.2).
- **DWG**: through an external converter only (see the decision above).
- **Performance targets** depend on P3.1's per-feature caching and background rebuild. P3F.3
  meets them on CPU time; a hole in a 250-feature studio costs 20–90 ms (the boolean, its names
  and the part's tessellation and mass for every intermediate state), so the first-sketch edit is
  within 2× of its budget on wall time when the machine is shared.
