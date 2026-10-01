# PCB Studio Fundamentals: gap analysis (2026-09-29)

## Stage 3H summary (stage close, 2026-10-01)

**Passed**: every requirement ID is ✅ or out of scope (with a reason); every exercise has a
scenario and unit-tested self-checks; every milestone judge reached ≥ 8.5 (P3H.6 after one fix
round).

| Milestone | Title | Judge |
|---|---|---|
| P3H.1 | IDF 2.0/3.0 parser and writer + fixtures | 8.9 |
| P3H.2 | Board and component geometry | 8.6 |
| P3H.3 | PCB Studio element and tab | 8.7 |
| P3H.4 | Component properties, BOM, search and component view | 8.65 |
| P3H.5 | Sync, export and the board exercise | 9.0 |
| P3H.6 | Create assembly and exercises 2–3 | 8.79 (r1 8.33) |
| P3H.7 | Component documents and version-pinned references | 8.78 |

**Requirements (84 rows: 67 PCB IDs + quiz + survey + 12 X IDs + 3 exercises):** done 73 ·
out of scope 11 · remaining 0. Out of scope: PCB1.4 (admin roles), PCB2.4 and PCB13.1–PCB13.6
(permissions, sharing, the single-viewer lock), PCB12 and the survey (learning site), X11 (the
out-of-scope list); PCB4.1's IDX/Eagle half (the 3H scope: "IDX and Eagle are out of scope
unless trivial"; PCB4.1 counts as ✅ for IDF).

**Exercises (self-checks are unit tests on independently derived values):**
- **Ex1 Board** (`course_pcb_ex1_board`, stand-in phone case `samples::phone_case`): the exported
  `.emn` outline is exactly the course's 9 points (81 × 146 with R8 corners: (32.5, −73, 0),
  (40.5, −65, 90), … ), test `ex1_exported_outline_matches_the_course`; outline area
  11826 − (4 − π)·64 = 11771.062 mm²; thickness **0.062** in MM — the course's "0.062 mm" quirk is
  followed on purpose (its own `.emn` shows it) and documented in the sample.
- **Ex2 Vision Controller** (`course_pcb_ex2_vision`): U1's top face Area **232.2576 mm²**
  (15.24²; shown "232.258 mm²"), tests `ex2_large_ic_top_face_area`,
  `component_documents_keep_bom_area_sync_and_export`.
- **Ex3 IDF Assembly** (`course_pcb_ex3_idf_assembly`): after the 1 in move and re-sync, X2
  (uBGA48_7.4X7.1) exports at **(4.064182376174947, 8.9)**, 90°, TOP PLACED; the keep-out
  `.PLACE_KEEPOUT` loop has area 12.7·9.525 − (1 − π/4)·6.35² = **112.314 mm²**; tests
  `ex3_moved_component_is_25_4_mm_further`, `ex3_place_keepout_written`.

**Stand-ins and decisions:** authored IDF fixtures in `fixtures/idf/` (cell phone, secondary
board, Vision PCB + a THOU copy, an IDF 2.0 sample) replace the course's public documents; a
stand-in phone case for Ex1 (Width/Length as driving sketch dimensions); Vision PCB has 29
components + the board (the course's unpublished board has another count; PCB8); local
workspace-level settings replace the admin-only cloud settings (X6); component documents and
their versions are store work, not undone with Create (as 3G's Move to document); component
documents are found by a metadata key (package + part number), not name or folder; no Onshape
name anywhere (tab "PCB Studio 1", IDF header "cadrs PCB Studio v0.1"); IDF 2.0 export drops keep
areas as the course says, although the 2.0 format has them (crate README). Stage close:
Create checks a reused document's part at its newest version (else its first part, else a new
document); the store's library lock serialises folder and component-document creation with
library edits; Sync matches untied component instances in the sync plane's board frame,
nearest first over all of them; a custom-part mapping is what Create inserts; a drag manipulator
for custom parts.

**Known remaining minor deltas (for the phase 3 final pass):**
- F in a Part Studio frames the default planes too, so a small board's studio (ex3 05) is framed
  loosely (changing fit for every Part Studio would move ~130 scenarios' frames);
- IDF: cite IDF 2.0 section numbers for VIA_KEEPOUT/min height, a bottom-side golden from a real
  exporter, an unclosed-loop warning, quoted keyword refdes, unquoted header source with spaces;
- Sync: mate connectors and plane features as the "top face parallel to" plane; default sync
  source; dialog title case; re-fit after re-sync;
- the sketch-fill skip rule for a region straddling a face edge; faceted selection shading in a
  linked V1 view; the Board BOM row has no Part number unless the IDF `.NOTES` gives one;
- no scenario frames for PCB11.3's moves/reuse (tests only) or PCB9.3's take-out/add-back loop;
- the BOM pane's Part number column ends long numbers in an ellipsis;
- the custom-part manipulator has no value box at the pointer (the pane's fields show the values)
  and no plane squares.


This maps [pcb-studio.md](pcb-studio.md) against the cadrs code on branch `phase3h` at `eab7bee`
(`main` after stages 3A–3D and 3B/3C merges; stage 3G is **not** on this branch yet). ✅ done ·
🟡 partial · ❌ missing · **out of scope** (only cloud and multi-user collaboration, sharing and
permissions, paid tiers and admin roles, Onshape account and learning-site features, the
single-viewer lock and cloud "Download" of tabs; plus IDX and Eagle import, deferred by X11).
Related: [intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md) (insert, triad, Fix, Group,
BOM, Edit in context), [inspection-and-repair-gaps.md](inspection-and-repair-gaps.md) (versions,
P3D.3), `derived-and-linking-gaps.md` (stage 3G, not written yet: ExternalRef, Open linked
document, Move to document).

**Summary (at `eab7bee`):** nothing PCB-specific exists. The **+** menu already lists "Create PCB
Studio" (disabled, icon `pcb-studio`), and the assembly stage left almost everything the
generated PCB assembly needs: Insert, triad with typed values, Fix, Group, the BOM with Part
number/Description and Subassembly BOM behavior, and **Edit in context** with Update context.
Missing, beyond PCB Studio itself: **Create Part Studio in context** / **Insert and go to
Assembly**, the **Transform** feature (no copy in place), **Composite part**, variables (P3F.4),
a face **Area** readout outside sketches (Measure is P3E.3), box select in the assembly view,
multi-file pick, a zip writer, and cross-document references (stage 3G).

**Cross-stage dependencies.**
- Stage 3G (ExternalRef, version-pinned inserts from other documents, Open linked document, Move
  to document): P3H.6 and the Custom part half of PCB11.6. Merge `main` after 3G lands.
- Stage 3F P3F.4 (variables): Ex1's #Width/#Length. Until it lands the stand-in case is driven by
  sketch dimensions (see Decisions).
- Stage 3E P3E.3 (Measure): the bottom-right face Area of Ex2/Ex3. P3H.6 adds a minimal planar-face
  Area readout if P3E.3 isn't on `main` by then.

**Counts (67 PCB IDs + 2 quiz/survey rows + 12 X IDs + 3 exercises = 84), at the stage close
(2026-10-01):** ✅ 73 · 🟡 0 · ❌ 0 · out of scope 11 (PCB: 59 / 0 / 0 / 8; quiz and survey:
0 / 0 / 0 / 2; X: 11 / 0 / 0 / 1; exercises: 3 / 0 / 0 / 0). The stage-close audit flipped
PCB2.1–PCB2.3, PCB3.3, PCB3.8–PCB3.10, PCB4.5–PCB4.7, PCB11.4–PCB11.8, X1, X6 and X10 to ✅ (each
row names its frames and tests; PCB11.4, PCB11.6 and X10 needed code: Create uses custom-part
mappings, and a drag manipulator); PCB4.1 is ✅ for IDF with IDX/Eagle out of scope. After P3H.7
it was ✅ 54 · 🟡 18 · ❌ 1; after P3H.6 ✅ 46 · 🟡 23 · ❌ 4; at `eab7bee` ✅ 8 · 🟡 10 · ❌ 55 ·
out of scope 11. PCB1.3, PCB9.1 and X12 are informational and count as ✅.

## Code inventory (what PCB Studio can reuse)

| Area | Where | State for PCB |
|---|---|---|
| Element kinds | `cadrs_core::document::ElementKind` (PartStudio, Assembly, Drawing); `commands::{AddElement, NewElementKind}`; app `document.rs` `TabKind` | Needs `ElementKind::PcbStudio`, `NewElementKind::PcbStudio`, `TabKind::PcbStudio` (store schema bump). |
| **+** Insert new element menu | `cadrs_app/src/document.rs` ~l.868 | "Create PCB Studio" item exists, **disabled**, icon `pcb-studio` (icon-rs has it). |
| Tab context menu | `cadrs_app/src/document.rs` ~l.1940 | Delete, Rename…, Properties…, Duplicate, Copy work generically; "Open in new window" and "Move to document…" disabled (3G). |
| Assemblies | `cadrs_core::assembly` (`commands::InsertInstance`, `solver`, `bom`, `structure`), `cadrs_app/src/assembly/*` (`insert.rs`, `triad.rs`, `group_dialog.rs`, `bom_panel.rs`) | Insert, triad (typed "1 in"), Fix, Group (+ edit via `SetMateFeature`), BOM, Subassembly BOM behavior all ✅ (A3, A13, A20). No box select in the view. |
| In-context | `cadrs_core::assembly::context` (`StudioContext`, `SetStudioContext`), `cadrs_app/src/assembly/in_context.rs` | **Edit in context** of an existing instance's studio + **Update context** + Use of context edges ✅ (P3B.9). No *Create Part Studio in context*, no origin dialog, no *Insert and go to Assembly*. |
| Transform / Composite | `document::FeatureKind` | No Transform feature at all; no Composite part (`InstanceSource::is_composite` means subassembly/rigid studio, unrelated). |
| Sketch Use, Extrude, rename part | `cadrs_sketch::ops::SketchOp::Use`, `FeatureKind::Extrude` (New part), `commands::RenamePart` | ✅ generic. |
| Area readout | `cadrs_app/src/region_select.rs` ("Area: … mm²", sketch regions only); `cadrs_kernel::FaceInfo::area` | Face area exists in the kernel, not shown for a selected face. |
| Kernel | `cadrs_kernel::{Profile, Region{outer, holes}, Curve2::Arc, extrude, boolean, transform, faces, edges, project}`; `cadrs_core::repair::face_loop`, `cadrs_sketch::face_offset::outer_loop` | Enough for board/keep/component bodies and for reading a board face's outer loop and holes as lines and arcs. |
| View cube / camera | `cadrs_app/src/view_cube.rs`, `camera.rs`; Repair's second viewport `cadrs_app/src/repair.rs` | Reusable; component view can follow Repair's render-to-image viewport. |
| File dialogs | `cadrs_ui::file_picker::{open_file_picker, open_folder_picker}`; STEP `export_dialog.rs` (Downloads folder, " (2)" names) | Single-file pick only (Import needs .emn + .emp); folder picker fits the settings dialog. |
| Zip | workspace has `flate2` only | No zip writer. |
| Documents, folders, versions | `cadrs_core::store::Store::{create, copy_document}`, `library::FolderEntry`, `history_log::{create_version, document_at_version}` | Can create component documents in a folder and version them; landing "Move to…" is disabled; no cross-document references (3G). |
| Variables | `feature_list.rs` says "cadrs has no variables yet" | Missing (P3F.4). |
| IDF / ECAD | — | Nothing. |
| UI widgets | `cadrs_ui::{Dialog, Checkbox, Tree, Table, Collapsible, FloatingPanel, SelectionList, TextInput, Menu, Toolbar, Spinner, Toast, InlineEdit}` | Enough for every PCB dialog and pane; a radio group may be new (check `dialog_fields.rs`). |
| Icons (icon-rs 0.3.6) | `upload`, `file-import`, `file-export`, `search`, `settings`, `help`, `chip`, `bill-of-materials`, `assembly`, `folder`, `link`, `pcb-studio` | Missing: `download`, a "sync document" and a "board" icon; add to icon-rs in P3H.3. |

## 1. Introduction

### PCB1 Introduction
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB1.1 | Mechanical and electrical engineers share one board dataset in one place | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB1.2 | PCB Studio is a tab type next to Part Studio, Assembly, Drawing | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB1.3 | Course assumes the fundamentals | ✅ | Informational; sketching, part studio and assembly courses are passed. |
| PCB1.4 | Settings need a company administrator | out of scope | Admin roles and paid tiers; cadrs uses local settings (X6). |

### PCB2 Accessing PCB Studio and settings
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB2.1 | Two storage settings: component library document, component folder | ✅ | P3H.4: workspace-level (`<store>/pcb-workspace.ron`), copied into every PCB Studio (`cadrs_core::pcb::library::LibrarySync`); scenario `course_pcb_settings` 01–06 (06: a second PCB Studio sees the same library and folder). The library holds the mappings for every PCB Studio (tests `workspace_settings_are_shared_by_every_pcb_studio`, `mappings_live_in_the_library_document`; `course_pcb_custom_part` 08); the folder receives one document per component (P3H.7: `create_assembly_makes_one_document_per_new_package`, `course_pcb_component_documents` 03). **Stage close audit: met.** |
| PCB2.2 | Settings dialog: two fields with browse icons, Select a folder picker, Update / Close, build string | ✅ | P3H.3/P3H.4: "Component library document" and "Create new component documents in this folder" fields, each with a browse icon at its right end, the hint text, the build string "cadrs PCB Studio v0.1", Update / Close (`course_pcb_settings` 01); Select a library document (02) and Select a folder (My documents tree, New folder, Select / Cancel; 04), as `v2-settings-select-folder-poster.png`. **Stage close audit: met.** |
| PCB2.3 | Recommended setup: new blank library document (with an empty PCB Studio tab) and a folder | ✅ | P3H.4: Select a library document → **New library document** makes a stored "PCB Component Library" with one empty PCB Studio tab and chooses it (`course_pcb_settings` 03, test `mappings_live_in_the_library_document` checks the one tab); Select a folder → **New folder** (04). **Stage close audit: met.** |
| PCB2.4 | Users need edit access to all three | out of scope | Permissions. |
| PCB2.5 | **+** → Create PCB Studio; default name | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB2.6 | Tab menu: Delete, Rename, Open in new tab, Properties, Move to document | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |

## 2. Navigating PCB Studio

### PCB3 Interface
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB3.1 | Several boards per PCB Studio; import, export, switch | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB3.2 | Toolbar: Import, Export, Sync, Create assembly, Search + magnifier + ✕, gear, ? | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB3.3 | Search highlights matches, "result n of m", up/down, ✕ | ✅ | P3H.4: `cadrs_core::pcb::search` (designators, packages, part numbers and board names), toolbar; P3H.5: Up/Down keys in the field, the matches in their own blue tint (the current one orange), ✕ also clears the selection it made. `course_pcb_search` 01 ("R": 1 of 13), 02 (down twice: 3 of 13, framed), 03/03b (up, Down key), 04 (✕: field, counter and tints cleared); test `search_matches_and_steps`. **Stage close audit: met.** |
| PCB3.4 | Left panel: Boards (active bold blue) and Components per board | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB3.5 | Boards from ECAD, a Part Studio or an Assembly | ✅ | P3H.3: import; P3H.5: Sync (`cadrs_pcb::sync`, `cadrs_app::pcb::transfer`); scenarios `course_pcb_sync_partstudio`, `course_pcb_ex1_board`. |
| PCB3.6 | Right-click board → Delete this board; generated tabs stay | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB3.7 | 3D viewport (green board, coloured boxes, holes), view cube, Camera, empty-state hint | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB3.8 | Right edge toggles: Component properties, Bill of Materials | ✅ | P3H.4: two toggle icons stacked on the viewport's right edge open the docked Component and Bill of materials panes (`cadrs_app::pcb::panes`); `course_pcb_component_properties` 01, `course_pcb_bom` 01. **Stage close audit: met.** |
| PCB3.9 | BOM: Designator, Part name, Part number; cross-highlight both ways | ✅ | P3H.4: `cadrs_core::pcb::bom`, BOM pane with Qty, Designator, Part name, Part number; a row under the pointer lights its components (`course_pcb_bom` 02), a component clicked in the view selects its row (03, 03b). Long part numbers end in an ellipsis in the narrow pane (minor). **Stage close audit: met.** |
| PCB3.10 | Double-click a refdes to edit it | ✅ | P3H.4: `SetRefdes` (undoable, duplicates refused); a group opens into per-component rows first. `course_pcb_bom` 04 (U1 → U10), 05 (group opened), 06 (duplicate refused), 07 (undone); test `refdes_edit_is_undoable_and_rejects_duplicates`. **Stage close audit: met.** |

## 3. Importing and translating

### PCB4 Importing an IDF
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB4.1 | Import formats: IDF, IDX, Eagle | ✅ (IDF) · **out of scope** (IDX, Eagle) | IDF: P3H.1 + P3H.3 (`cadrs_idf`, Import ECAD files; `course_pcb_import_idf`). **IDX and Eagle: out of scope** — the stage 3H scope says "IDX and Eagle are out of scope unless trivial" (ORCHESTRATOR.md, Stage 3H), and neither is (IDX is the ProSTEP EDMD XML schema with incremental change messages; Eagle `.brd` is a full ECAD XML with its own library model). Counted as ✅ for its IDF half. |
| PCB4.2 | IDF = `.emn` (outline, placement, holes, milling, keep areas) + `.emp` (outline, height) | ✅ | P3H.1: `cadrs_idf` parses/writes both files; fixtures in `fixtures/idf/`. |
| PCB4.3 | Import ECAD files dialog: Choose Files (multi-select), Import / Cancel | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB4.4 | Imported board shows and is added under Boards; again adds another | ✅ | P3H.3 (judge 8.7): `cadrs_app::pcb`, `cadrs_core::pcb`; scenarios `course_pcb_{studio_create,import_idf,delete_board}`. |
| PCB4.5 | Click a component under Components → component view; board goes back | ✅ | P3H.4: `course_pcb_component_view` 01 (packages listed per board with counts), 02 (QFP100_600MIL alone on the grid, Part name/number/Representation in the pane), 03 (another package), 04 (the board clicked: board view as it was). **Stage close audit: met.** |
| PCB4.6 | Component properties pane + click in viewport, staying in board view | ✅ | P3H.4: `course_pcb_component_properties` 01 (pane open, hint), 02 (U1 clicked in the board view: its properties, the board stays). **Stage close audit: met.** |
| PCB4.7 | BOM grouped with quantities; cross-highlights | ✅ | P3H.4: one row per part number with Qty and a designator range ("R1-R10"); `course_pcb_bom` 01–03; test `bom_groups_by_part_number`. **Stage close audit: met.** |

### PCB5 Translating a board into PCB Studio
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB5.1 | Recognise parts by name: board/PCB, Keep-in/Keepin, Keep-out/Keepout | ✅ | P3H.2 logic (`cadrs_pcb::names`); P3H.5 Sync uses it on part names (every top face of the board part as one region, so a board extruded from touching regions has no seams). |
| PCB5.2 | Sync dialog: source dropdown "<tab> (Assembly)", "Top face of board part is parallel to" plane, OK / Cancel | ✅ | P3H.5: `pcb-sync-dialog` (Part Studio and Assembly tabs, Top/Front/Right Plane; a re-sync opens on the board's source and plane); runs on the kernel thread. Mate connectors and plane features as the plane: not offered (`SyncPlane::Custom` exists in `cadrs_pcb`). |
| PCB5.3 | After OK the board and keep areas show and are listed | ✅ | P3H.5: the board named after the tab under Boards, shown; keep-outs under the board also drawn as dark translucent patches on its top face (display only). `course_pcb_ex1_board` 15. |
| PCB5.4 | Non-matching parts not translated; a message says so | ✅ | P3H.5: the toast "Added Cell phone. Not translated: Enclosure, Battery, Antenna" (each name once); test `untranslated_parts_are_reported`. |
| PCB5.5 | Syncing an assembly picks up component instances and positions | ✅ | P3H.5: every part at any depth in assembly coordinates; part-name fallback ("`<refdes> <package>`") for boards built into a Part Studio. P3H.6: the instances Create assembly made are tied to their placements by designator. **P3H.7**: the instances are references to component documents (the component-folder rule): an untied instance of a component document (inserted by hand, or its tie lost) is recognised by its document's package (`GeneratedAssembly::documents`) at the nearest free placement, whatever the part or document is called; at depth and after an update to a new version too. Tests `sync_recognises_components_through_their_link` (renamed part in V2, delete + re-insert, inside a Product assembly), `resync_keeps_board_and_keep_ids_and_reads_moved_components` (+25.4 mm → Y 8.9), `component_documents_keep_bom_area_sync_and_export`. **Stage close**: an untied instance's position is measured in the sync plane's board frame (as the sync writes placements), and instances are assigned to placements nearest first over all of them: test `untied_instances_match_in_the_board_frame_of_the_sync_plane` (two SOT23s on a Front-plane sync, inserted in the misleading order). |
| PCB5.6 | Re-sync updates the board in place, no duplicate | ✅ | P3H.5: `SyncBoard` (one undo step) updates the board synced from the same tab; keep areas keep their ids by part name, components by designator; the board keeps its name. |
| PCB5.7 | Workflow: Create Part Studio in context, Use enclosure edges, Extrude, rename, Insert and go to Assembly, Update context | ✅ | P3H.5: Display states ▾ → Create Part Studio in context (Origin of new Part Studio: the assembly Origin; mate connectors not offered), the context bar's Insert and go to Assembly, the instance menu's Update context ▸ <studio>; the context records a fingerprint of its parts' studios so a resize makes it out of date (`cadrs_core::assembly::managed_context`). `course_pcb_ex1_board` 03–18. |

### PCB7 Creating an assembly from PCB Studio
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB7.1 | Create assembly dialog: Board ✓, Components ✓, Keep-In and Keep-Out ☐, slow-build note, OK / Cancel | ✅ | P3H.6: `pcb-create-dialog` ("Create assembly from '<board>'", Board ✓, Components ✓, Keep-In and Keep-Out Areas ☐, the note, OK / Cancel); builds on the kernel thread with a progress card and a toast (`cadrs_app::pcb::create_assembly`). `course_pcb_ex2_vision` 04, 06. Fix round 1: tighter checkbox rows; P3H.7: the rows were still 27 px apart (19 px rows + the dialog body's 8 px row gap), now one group with no gap: **19 px pitch** as the course's (`course_pcb_ex2_vision` 04); the progress card photographed in `course_pcb_ex2_vision` 05 (held up by the scenario-only `pcb-create-hold <frames>`). |
| PCB7.2 | Generates a Part Studio (part "Board [<name>]") and an Assembly, both named after the board | ✅ | P3H.6: `cadrs_pcb::create_assembly::generate` → `CreatePcbAssembly` (one undo step): Part Studio "<board>" (Sketch 1, "Board [<board>]"), Assembly "<board>". Test `create_assembly_makes_a_studio_and_an_assembly_named_after_the_board`. |
| PCB7.3 | One component document per new package in the component folder; version-pinned references | ✅ | **P3H.7**: Create assembly writes one stored document per new package into the PCB settings' component folder (made again when deleted; "PCB Components" when none is chosen), each versioned V1, and inserts the components as version-pinned references to that version (3G `SourceRef`, frozen copies in the board document, `cadrs_pcb::component_docs`, `generate_linked`). A package that has a document already (any earlier Create, any board or document) is reused at its newest version, found by the key in its metadata (`DocumentMeta::pcb_component`: package + part number), so a renamed or moved document still matches. The board document's Create is one undo step; the component documents and versions stay (decision, as Move to document). Tests `create_assembly_makes_one_document_per_new_package`, `second_create_reuses_component_documents`; `course_pcb_component_documents` 01–03. The in-document components Part Studio (P3H.6) is kept only as the fallback with no document store (`generate`). **Stage close**: a reused document's newest version is built and its part checked (else its first part, else a new document is made for the package; test `reuse_checks_the_part_at_the_newest_version`); finding and making documents and the folder hold the store's library lock and re-read the library, and `Store::sync` merges folders with what is on disk, so concurrent Creates and library edits can't duplicate or lose them (tests `creates_from_a_stale_library_make_one_document_per_package`, `concurrent_ensure_folder_makes_one_folder`, `a_stale_library_edit_keeps_the_component_folder`). |
| PCB7.4 | Components unmated; Fix the board, Group the components | ✅ | P3H.6: no mates; box select in the assembly view (`assembly/box_select.rs`, window/crossing) fills Group; Fix (A3.7). `course_pcb_ex2_vision` 10–12. Fix round 1: while a box is dragged, the instances it will select are pre-highlighted (hover tint, `HoverParts.2`; ex2 10, ex3 10); selected components under the board no longer show through it (silhouette depth bias −5e-5; ex2 11, ex3 10b). |
| PCB7.5 | Instance names from package names with `<n>` | ✅ | P3H.6: "<package> <n>" (test `create_assembly_makes_a_studio_…`; `course_pcb_ex2_vision` 08). |
| PCB7.6 | BOM rows per ECAD part number with Description | ✅ | P3H.6: Part number + Description from `.emp` PROP (`description`); test `create_assembly_bom_has_one_row_per_part_number_with_description`; `course_pcb_ex2_vision` 09. Fix round 1: every package of the Vision PCB fixtures (mm and thou) has a DESCRIPTION PROP, so every BOM row has one (test checks all). P3H.7: the same with linked instances (the copies carry the properties), and the board row has a Description ("Board, <name>", or a Part number/Description from an IDF `.NOTES` record): `component_documents_keep_bom_area_sync_and_export`, `course_pcb_ex2_vision` 09. |
| PCB7.7 | Subassembly BOM behavior → Show assembly only | ✅ | Generic assembly property (A20.5); applies to the generated assembly unchanged. |
| PCB7.8 | The PCB assembly is ordinary: insert as subassembly, in-context design | ✅ | Subassembly insert (A17.2) and Edit in context (P3B.9) are generic. |
| PCB7.9 | One part for the PCB: Part Studio in context, Transform copy in place, Composite part (Closed) | ✅ | P3H.6: main's Transform (Copy in place) takes assembly-context parts (`TransformFeature::context`/`sources`, `context_copies`; copies keep their colours) and **Composite part** (`CompositeFeature`, Closed = one part in the Parts list and one instance; `composite_ui.rs`). `course_pcb_one_part`; tests in `crates/cadrs_core/tests/composite.rs`. Fix round 1: Update context re-snapshots the Transform's context copies (matched by context id, same undo step; test `context_copies_follow_update_context`); the Large studio banner counts only the studio's own parts (Parts list and banner agree, 06: 30); dialog field "Parts and composite parts"; Parts list, Instances and BOM rows show the `composite-part` icon; the composite goes into a separate parent assembly (Assembly 1), selected, with a one-row BOM (`course_pcb_one_part` 09–10). |
| PCB7.10 | Right-click a component → Open linked document | ✅ | **P3H.7**: the instance menu's Open linked document opens the component document at the referenced version, read-only (3G `linked_session`), its part selected; the banner's new **Edit Main** opens its workspace for editing (saved as usual) with "Editing Main of <document>" and **Back to <board document>** (the board document as it was, its undo history kept). `course_pcb_component_documents` 04–07. |

## 4. Working with PCB data

### PCB9 Modifying placement and exporting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB9.1 | Why edit placement (interference, off-limits areas) | ✅ | Informational; Check interference exists (P3B.9). |
| PCB9.2 | Fix the board, Group the components | ✅ | A3.7, A13. |
| PCB9.3 | Move: take out of Group, triad drag or typed distance, add back | ✅ | Group Edit (row ✕), triad typed "1 in" (A3.4). |
| PCB9.4 | Keep areas: sketch on board, extrude New part, name Keep-in/Keep-out, insert and place | ✅ | Generic modelling and Insert (✓ in dialog keeps studio position, A2.3); recognition is PCB5.1. Fix round 1: a part named as a keep-out or keep-in defaults to grey (`appearance::KEEP_GREY`, via `cadrs_core::pcb::names`, which `cadrs_pcb::names` re-exports). |
| PCB9.5 | Edit the board; create versions; update references in the assembly | ✅ | Editing ✅, versions ✅ (P3D.3). **P3H.7**: a component document edited and versioned shows the update badge on its instances and tab in the board assembly; Update (Reference manager or Update linked document) re-points them, keeping ids, poses and designator ties. Test `edit_version_and_update_change_the_instances` (63.048 → 105.08 mm³); `course_pcb_component_documents` 08–12. |
| PCB9.6 | Round trip: sync the edited assembly; outline and positions update | ✅ | P3H.5 sync in place; P3H.6 Ex3 end to end: tests `ex3_moved_component_is_25_4_mm_further`, `ex3_place_keepout_written`; `course_pcb_ex3_idf_assembly` 12–14. Fix round 1: the PCB view re-fits when a re-sync changes the board's extent; ex3 13 is fitted (F after Top). |
| PCB9.7 | Export ECAD files dialog: IDF 2.0 / 3.0 radios, Export / Cancel; 2.0 has no keep areas | ✅ | P3H.5: `pcb-export-dialog` (3.0 chosen; a note that 2.0 files have no keep areas — the course's claim about Onshape's exporter, not the format, see the crate README). `course_pcb_export_idf`; test `export_idf2_has_no_keepouts`. |
| PCB9.8 | Zip of `<board>.emn` + `<board>.emp` (outline, placements, holes, keep areas; outlines, heights) | ✅ | P3H.5: `<board>.zip` (" (2)" when taken) in the Downloads folder (`$CADRS_EXPORT_DIR`; a scenario's `exports` folder), the header dated by the app clock; the toast's Open .emn shows the board file. |
| PCB9.9 | ECAD flow back is file-based only | ✅ | P3H.5: the export is the flow back; no live link. |

## 5. Library components

### PCB11 Electronic component and properties
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB11.1 | One document per component (one Part Studio, one part; empty assembly) | ✅ | **P3H.7**: each component document has one Part Studio (named after the package) with one part (the ECAD box, Part number, Description, PCB colour) and an empty "Assembly 1", a V1, and a thumbnail of its part for the documents page. Test `create_assembly_makes_one_document_per_new_package`; `course_pcb_component_documents` 03. |
| PCB11.2 | Edit a component's studio, create a version, update the assemblies | ✅ | **P3H.7**: Open linked document → Edit Main → edit (e.g. the extrude depth) → Create version → Back → update badge → Update. Test `edit_version_and_update_change_the_instances` (7.4·7.1·1.2 = 63.048 → 7.4·7.1·2 = 105.08 mm³, undo back); `course_pcb_component_documents` 05–12. |
| PCB11.3 | Move component documents to other folders; references stay tracked | ✅ | **P3H.7**: references name documents by id, so moving (and renaming) component documents keeps Where used, updates and reuse (by the metadata key). Test `moved_component_documents_stay_tracked`. |
| PCB11.4 | Library document keeps footprint → representation mappings, shared, auto-maintained | ✅ | P3H.4: `cadrs_core::pcb::library` (the library document's PCB Studio tab, else `<store>/pcb-component-library.ron`, made when first written); every mapping change is an undoable `SetRepresentation`, written to the library and pulled into the document's other PCB Studios at once and into other documents' when they open (`LibrarySync`): every component of the package on every board shows the new representation (`course_pcb_custom_part` 03–08, 08 a second PCB Studio). **Stage close**: a mapping to a custom part is also what the next **Create assembly** inserts for that package (a version-pinned reference to the custom part's document, placed by the mapping's transform; Sync reads it back through its designator tie), and changing the mapping back makes the next Create use the package's own document again: test `a_custom_part_mapping_is_used_by_create`. |
| PCB11.5 | Component pane: Part name, Part number, Representation None / From ECAD data (link icon) / Custom part | ✅ | P3H.4: `SetRepresentation`; `course_pcb_component_properties` 02 (Part name, Part number read-only, the three radios, the open-link icon by From ECAD data), 03–06 (None hides U1, From ECAD back, undo both ways). **Stage close audit: met.** |
| PCB11.6 | Custom part: Select custom part (document, version), Translate/Rotate, Center, ✓ | ✅ | P3H.4: Select custom part (stored documents, their versions, their parts; `course_pcb_custom_part` 01–03), typed Translate/Rotate (04, 05), Center (05), Accept (06). **Stage close**: the **manipulator** (`cadrs_app::pcb::manipulator`): while a custom part is edited, a triad on it (board view with the component selected, or the component view); dragging an arrow translates along the package axes, dragging a ring turns the part about its middle in 5° steps, the fields follow, Accept/Cancel as for typed values (`course_pcb_custom_part` 07b X arrow, 07c Z ring a quarter turn, 07d accepted). |
| PCB11.7 | Two ways in: pane on a viewport selection (accept before moving), or the component view | ✅ | P3H.4: (a) a component clicked in the board view, Custom part chosen in the pane (the Select custom part dialog's OK accepts the replacement; only then the Translate/Rotate fields and the manipulator appear), moved, Accept (`course_pcb_custom_part` 01–06); (b) Components → the package → the component view with the same pane (07–07d). Closing the pane and clicking empty space leave it (`course_pcb_component_properties`). **Stage close audit: met.** |
| PCB11.8 | Component view: grid floor, iso view cube, dark-red top, bright-red sides | ✅ | P3H.4: `course_pcb_component_view` 02 (isometric, grid floor, blue Z axis, dark-red top, bright-red sides, as `v8-component-properties-poster.png`). **Stage close audit: met.** |

### PCB12, completion survey
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB12 | Self-check: understanding the library (quiz) | out of scope | Learning-site feature; the library behaviour is covered by P3H.4/P3H.6 tests. |
| Survey | Completion survey | out of scope | Learning-site feature. |

## 6. Data management

### PCB13 Sharing
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB13.1 | Edit needs Edit on document, library and folder | out of scope | Permissions. |
| PCB13.2 | View-only users can't see the tab without library/folder view | out of scope | Permissions. |
| PCB13.3 | Edit document + view library: import/export only | out of scope | Permissions. |
| PCB13.4 | Only one person can view a PCB Studio at a time | out of scope | Single-viewer lock (multi-user). |
| PCB13.5 | Share library and folder with Teams | out of scope | Sharing. |
| PCB13.6 | View-only sharing for suppliers | out of scope | Sharing. |

## Exercises
| ID | Requirement | Status | Notes |
|---|---|---|---|
| PCB6 | Board Exercise: board in context of a phone case, keep-outs, sync, resize, re-sync, export IDF 3.0 | ✅ | P3H.5: stand-in `samples::phone_case` (`fixtures/phone_case_standin.cadrs`), scenario `course_pcb_ex1_board` (steps 2–18 in the UI; the resize is `phone-case-size 85 150`), tests `ex1_outline_before_resize`, `ex1_exported_outline_matches_the_course` (the course's 9 points exactly, thickness 0.062). |
| PCB8 | Vision Controller: import IDF, create assembly, Group, Fix, area of a large IC's top face | ✅ | P3H.6: `course_pcb_ex2_vision` (01–13: import, Create assembly, BOM, box-select Group, Fix, U1 top face "Area: 232.258 mm²"); tests `ex2_large_ic_top_face_area`, `create_assembly_*`. Instances (30) vs the course's (29): the authored fixture has 29 components + the board; the course board is a different, unpublished design, and dropping a placement would shift every Vision count, test and golden of P3H.2–P3H.6 for no behavioural gain, so it stays (fix round 1 decision). |
| PCB10 | IDF Assembly: import, create assembly, keep-out, move a part 1 in, sync, export | ✅ | P3H.6: `course_pcb_ex3_idf_assembly` (keep-out sketch/extrude/insert, triad 1 in, Group, Fix, re-sync, Export IDF 3.0, .emn viewer); tests `ex3_moved_component_is_25_4_mm_further`, `ex3_place_keepout_written`. Fix round 1: step 6 is drawn with the sketch tools on the board's top face (Rectangle from the board corner, Dimension tool 0.5 in / 0.375 in corner to corner, sketch Fillet 0.25 in; 03a–03d), no script command; the exported Y prints "8.9" (placement write-back drops float noise below 1e-12 mm; test asserts the token); no hover outline during the triad drag (08). |

## Cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| X1 | PCB Studio element: toolbar, tree, viewer, Component/BOM panes, search, component view, many boards, undoable mutations | ✅ | P3H.3: tab type, toolbar, Boards/Components, viewer with view cube, many boards; P3H.4: panes, search, component view; every mutation (import, sync, delete board, settings, representation, refdes) is an undoable command (`cadrs_core::pcb` tests `add_pcb_studio_is_undoable`, `delete_board_is_undoable_and_moves_the_active_board`, `settings_are_undoable`, `refdes_edit_is_undoable_and_rejects_duplicates`; `course_pcb_component_properties` 05–06, `course_pcb_bom` 07). **Stage close audit: met.** |
| X2 | Board data model (`Board`, `Placement`, `Package`, `(x, y, angle)` loops) bevy-free | ✅ | P3H.1: `cadrs_idf::model` (serde, file units, exact nm conversion, `place_loops`). |
| X3 | IDF 2.0/3.0 parser and writer, MM/THOU, zipped output | ✅ | P3H.1: all 13 .emn sections + ELECTRICAL/MECHANICAL/PROP, 2.0 and 3.0, MM/THOU(/TNM), own zip writer on flate2; spec notes in the crate README. |
| X4 | Authored fixtures: (a) cell phone, (b) secondary board, (c) vision controller; round-trip tests | ✅ | P3H.1: `fixtures/idf/` cell phone, secondary board, Vision PCB (+THOU copy), 2.0 sample; round-trip + proptests. |
| X5 | IDF → B-rep (board, cut-outs, holes, keep bodies, components with rotation/flip, colours) and MCAD → board | ✅ | P3H.2: `cadrs_pcb::{geometry,mcad,placement}` (per-package body cache, exact placement inverse, projection onto Top/Front/Right/custom planes, round trips on all fixtures). |
| X6 | Local PCB settings: library location, component folder, Update / Close | ✅ | P3H.4: workspace-level settings and library (see PCB2.1–PCB2.3, PCB11.4); `course_pcb_settings`. **Stage close audit: met.** |
| X7 | Create assembly: studio + assembly, component documents, version-pinned refs, no mates, options, progress, non-blocking | ✅ | P3H.6: studio + assembly, no mates, options, progress card, non-blocking. **P3H.7**: component documents (written and versioned on the kernel thread too) and version-pinned references (see PCB7.3). |
| X8 | Re-sync updates in place, then export | ✅ | P3H.5 (see PCB5.6, PCB9.7). |
| X9 | Prerequisites: managed in-context design, Transform copy in place, Composite part (Closed) | ✅ | P3H.5 managed in-context ✅; P3H.6 Transform copy in place of context parts (main's Transform) and Composite part (Closed). Fix round 1: Update context refreshes the copies. |
| X10 | Custom representation editor: part from another document/version, translate/rotate, Center, stored in the mapping | ✅ | P3H.4 (see PCB11.6) + the stage-close manipulator (translate/rotate by drag); stored in the library mapping (`representation_mapping_round_trips`), used by Create assembly (`a_custom_part_mapping_is_used_by_create`). |
| X11 | Out of scope list: admin, sharing, lock, Download; IDX/Eagle later | out of scope | As stated; IDX/Eagle revisit after P3H.6. |
| X12 | Rough size | ✅ | Informational; reflected in the milestone split below. |

## What each exercise needs

**PCB6 Board Exercise (`course_pcb_ex1_*`, P3H.5).** Stand-in document `samples::phone_case`
("Exercise: Board (stand-in)"): Enclosure studio (a shell case, outer Width × Length with R10
corners, 2 mm wall, so the inner cavity is (W−4) × (L−4) with R8 corners; a Battery block; an
Antenna block of 5 edges), and a Cellphone assembly (Cell phone, Battery, Enclosure, Group 1).
Width/Length start at 70/140 (P3F.4 variables if on `main`, else two driving sketch dimensions).
- Steps 2–7: **Create Part Studio in context** (origin dialog: assembly Origin) → sketch on the
  battery's top face, Use the case's inner face and the battery/antenna edges → Extrude 0.062 mm
  New part "Board" upward, Extrude battery/antenna regions 1 mm down as "Keep-out Battery" /
  "Keep-out Antenna", rename the studio "Board & Keep out" → **Insert and go to Assembly**.
- Step 8: edit Group 1 and add three instances (exists). Steps 9–11: Create PCB Studio, Sync
  (Cell phone (Assembly), Top Plane). Steps 12–16: set 85/150, Update context, re-sync.
  Steps 17–18: Export IDF 3.0 → `Cell phone.zip` → `Cell phone.emn`.
- Self-check (unit test `ex1_exported_outline_matches_the_course`): after the resize the inner
  cavity is 81 × 146 R8, so the loop is exactly the course's (32.5, −73, 0), (40.5, −65, 90),
  (40.5, 65, 0), (32.5, 73, 90), (−32.5, 73, 0), (−40.5, 65, 90), (−40.5, −65, 0),
  (−32.5, −73, 90), (32.5, −73, 0) (to 1e−9); thickness 0.062; empty `.DRILLED_HOLES` and
  `.PLACEMENT`; two `.PLACE_KEEPOUT` records. Before the resize: 66 × 136 R8. Outline area
  11826 − (4 − π)·64 = **11771.062 mm²**.

**PCB8 Vision Controller (`course_pcb_ex2_*`, P3H.6).** Fixture `fixtures/idf/vision controller/Vision PCB.emn/.emp`
(X4 c; also a THOU copy in `vision controller thou/`): 4 × 3 in, 4 Ø3.175 NPTH MTG corner holes, two header strips, 29 placements
including two large square ICs (U1 `QFP100_600MIL` 15.24 × 15.24 mm, U2 10.16 mm square).
- Import both files, Create assembly (defaults), Group all components (**box select**, missing),
  Fix the board, Top view, select U1's top face, read Area.
- Self-check (`ex2_large_ic_top_face_area`): 15.24 × 15.24 = **232.2576 mm²** (0.36 in²); needs the face
  Area readout (P3E.3 or the P3H.6 minimal one). Also 29 instances + board, and the BOM grouping.

**PCB10 IDF Assembly (`course_pcb_ex3_*`, P3H.6).** Fixture `fixtures/idf/secondary board/secondary board.emn/.emp`
(X4 b), in inches-equivalent MM like the course: outline (−5.20972, −22.74338) →
(45.59028, 15.35662) (2 × 1.5 in), thickness 0.84, ~20 placements (`BUTTON_EVQPUA02 5209001 X0`
at 24.47, −9.48, 270; `CRYSTAL_CX_4V 4510219 X1`; `1210_SR73K2E`, `TSSOP_20`, `1206C`, `SOT23`)
and `uBGA48_7.4X7.1` at (4.064182376174947, −16.5), rotation 90, TOP, over the bottom-left corner.
- Sketch 0.5 × 0.375 in with R0.25 at the bottom-left corner on the board's top face, Extrude
  0.125 in New part "Keep-out", Insert with ✓ in the dialog, move `uBGA48_7.4X7.1` 1 in along Y
  with the triad, Group, Fix, re-sync, Export IDF 3.0.
- Self-check (`ex3_moved_component_is_25_4_mm_further`): exported X = 4.064182376174947, **Y =
  −16.5 + 25.4 = 8.9**, rotation 90, TOP PLACED (1e−9), and a `.PLACE_KEEPOUT` loop of the corner
  profile (12.7 × 9.525 mm, R6.35) exists. Keep-out area 12.7·9.525 − (1 − π/4)·6.35² =
  **112.314 mm²** (checks the projection).

## Proposed milestones

| Milestone | Title | Covers | Done when |
|---|---|---|---|
| P3H.1 | **IDF 2.0/3.0 parser and writer + fixtures** | X2, X3, X4, PCB4.2, PCB9.8 (file content), PCB9.7 (2.0 vs 3.0 keep areas) | Unit and property tests pass; the three fixtures round-trip. |
| P3H.2 | **Board and component geometry** | X5, PCB5.1, PCB5.4 (logic) | Geometry tests pass on the fixtures and a hand-built Part Studio. |
| P3H.3 | **PCB Studio element and tab** | PCB1.1, PCB1.2, PCB2.1–PCB2.3, PCB2.5, PCB2.6, PCB3.1–PCB3.7, PCB4.1 (IDF), PCB4.3, PCB4.4, X1, X6 | Scenarios match the interface and dialog images. |
| P3H.4 | **Component properties, BOM, search and component view** | PCB3.8–PCB3.10, PCB4.5–PCB4.7, PCB11.4–PCB11.8, X10 | Scenarios match `v8`; custom-part mapping round-trips. |
| P3H.5 | **Sync, export and the board exercise** | PCB5.*, PCB6, PCB9.6–PCB9.9, X8, X9 (in-context) | `course_pcb_ex1_*` passes; `.emn` test matches the course's points. |
| P3H.6 | **Create assembly and exercises 2–3** | PCB7.*, PCB8, PCB9.1–PCB9.5, PCB10, PCB11.1–PCB11.3, X7, X9 (Transform, Composite) | Both exercises pass with their unit tests; needs 3G on `main`. **Done: judge 8.79** (r1 8.33; fix round 1 16f19c7). |
| P3H.7 | **Component documents and version-pinned references** | PCB5.5, PCB7.3, PCB7.10, PCB9.5, PCB11.1–PCB11.3, X7 | One document per package in the component folder, versioned, referenced by version; Open linked document, edit, version, update; moves keep tracking. Tests in `crates/cadrs_pcb/tests/component_documents.rs`; `course_pcb_component_documents`. **Done: judge 8.78** (first round). |

### P3H.1 IDF 2.0/3.0 parser and writer + fixtures
- New bevy-free crate `crates/cadrs_idf` (depends on serde only; `cadrs_core` may depend on it).
  Model (X2): `Board { name, units, thickness, outline: Vec<Loop>, other_outlines, route_outlines,
  place_outlines, keepouts: Vec<KeepArea{kind: Route|Place|Via, side, height, loops}>,
  place_regions, holes: Vec<Hole{d, x, y, plating, assoc, kind, owner}>, notes, placements }`,
  `Placement`, `Package { name, part_number, units, height, outline, electrical props }`, `Loop =
  Vec<(x, y, angle)>` with a 360° pair as a circle; `Length` kept in file units with `to_mm()`.
- `.emn`: HEADER, BOARD_OUTLINE, PANEL_OUTLINE, OTHER_OUTLINE, ROUTE_OUTLINE, PLACE_OUTLINE,
  ROUTE_KEEPOUT, PLACE_KEEPOUT, VIA_KEEPOUT, PLACE_REGION, DRILLED_HOLES, NOTES, PLACEMENT;
  `.emp`: ELECTRICAL, MECHANICAL; quoted strings, comments (`#`), owner fields (MCAD/ECAD/
  UNOWNED), MM and THOU. IDF 2.0 subset: read tolerant; write omits keep areas and 3.0-only
  sections (course claim; verify against the 2.0 spec and record).
- Writer: header `BOARD_FILE 3.0 "cadrs PCB Studio v0.1" <YYYY/MM/DD.HH:MM:SS> 1`, board name and
  units line, full float precision (`{}` shortest round-trip), sections in spec order.
- Zip: `<board>.emn` + `<board>.emp` (see Decisions).
- Fixtures `fixtures/idf/`: `cell_phone.emn/.emp` (a), `secondary_board.emn/.emp` (b),
  `vision_pcb.emn/.emp` (c), plus one THOU file and one IDF 2.0 file.
- Unit tests: `parses_each_fixture`, `round_trip_is_identity` (parse → write → parse),
  proptest `any_board_round_trips`, `thou_converts_to_mm`, `idf2_writer_drops_keep_areas`,
  `header_names_cadrs_not_onshape`, `zip_holds_emn_and_emp`, `circle_pair_is_a_full_circle`.
- Scenarios: none. Images: `ex1-step18-exported-emn.png`, `ex3-step14-exported-emn-placement.png`
  (file layout only).

### P3H.2 Board and component geometry
- `cadrs_pcb::geometry` (bevy-free crate; built in P3H.2): IDF loop → `cadrs_kernel::Region` (lines, arcs from
  the included angle, circles), board = first loop minus later loops and drilled holes, extruded
  by thickness; keep bodies extruded by their height (thin 0.1 mm marker when none), translucent
  dark; component = `.emp` outline × height, placed at (x, y) + rotation, flipped for BOTTOM
  (mirror through the board, on the far side); colours green board, per-package palette.
- MCAD → board: name matching (case-insensitive, word match: `board`, `pcb`, `keep-in`/`keepin`,
  `keep-out`/`keepout`); the board face whose normal is parallel to the chosen plane (top), its
  loops through `repair::face_loop` / kernel edges into `(x, y, angle)` with arcs, thickness from
  the body extent along the normal; keep regions from their faces with height; component
  instances (source in the component folder) → placements; the list of untranslated parts.
- Unit tests: `board_volume_is_area_times_thickness_minus_holes`, `rotated_component_footprint`,
  `bottom_side_component_is_below_the_board`, `name_matching_cases` (Mainboard, PCB_1,
  "Keep-out Battery", Enclosure → none), `rounded_rect_projects_to_lines_and_90_degree_arcs`
  (81 × 146 R8 → the course's 9 points), `keepout_corner_profile_area` (112.314 mm²).
- Scenarios: none (rendered from P3H.3). Images: `v4-import-idf-poster.png`,
  `ex1-step11-synced-board-keepouts.png`, `ex1-goal-board.png`.

### P3H.3 PCB Studio element and tab
- Layout from `v3-interface-poster.png` / `ex1-step10-sync-dialog.png`: a thin icon toolbar at the
  top left (upload, download │ sync-document, create-assembly │ a "Search" field, magnifier, ✕,
  gear, ?), Export and Create assembly greyed with no board; a left panel (~20 % width) with
  **Boards** (board icon, rows indented, the active one bold blue with a blue left bar) and
  **Components** (one node per board); a panel-collapse tab on the panel's right edge; the
  viewport with the view cube top right; two small toggle icons stacked on the viewport's right
  edge (Component, BOM); tab icon `pcb-studio` in the tab bar. Empty state: centred grey hint "Use
  the toolbar to import data from an ECAD file or from a Part Studio or Assembly".
- `ElementKind::PcbStudio(Box<PcbStudio>)` (boards, active board, per-board source link),
  `NewElementKind::PcbStudio`, `TabKind::PcbStudio`; store schema bump; default name "PCB Studio
  1". Commands: `ImportBoard`, `DeleteBoard`, `SetActiveBoard` (view state, not undoable).
- Import ECAD files dialog (`ex2-step4-import-ecad-dialog.png`): Choose Files (multi-select picker,
  "No file chosen"), Import / Cancel; errors as toasts.
- Local settings dialog (X6, `v2-settings-select-folder-poster.png`): "Component library
  document" and "Create new component documents in this folder" fields with browse icons (a
  library-document picker and `open_folder_picker` over the library folders), hint text, "cadrs
  PCB Studio v0.1" at the bottom, Update / Close; saved in the user config dir.
- ? opens `docs/` help page (local) instead of a web page.
- Icons to add to icon-rs: `download`, `sync-document`, `board` (and use `file-import`/`upload`).
- Scenarios: `course_pcb_create_tab` (+ menu, default name, tab menu), `course_pcb_settings`,
  `course_pcb_import` (dialog, Vision PCB fixture imported), `course_pcb_boards` (second board,
  switch, bold active, Delete this board, undo). Unit tests: `add_pcb_studio_is_undoable`,
  `delete_board_keeps_generated_tabs`, `pcb_studio_survives_save_and_load`.
- Images: `v3-interface-poster.png`, `v2-settings-select-folder-poster.png`,
  `ex2-step4-import-ecad-dialog.png`, `v4-import-idf-poster.png`, `v1-introduction-poster.png`.

### P3H.4 Component properties, BOM, search and component view
- Layout from `v8-component-properties-poster.png`: component view = the part alone on an
  isometric grid floor with a blue Z axis line, view cube top right; a right pane headed
  "Component" (icon) with read-only grey **Part name** and **Part number** fields and a
  **Representation** radio group: None, From ECAD data (selected, small open-link icon), Custom
  part. Generic box dark red on top, bright red sides.
- BOM pane: Designator, Part name, Part number, grouped with Qty; hover/click cross-highlight
  both ways; double-click Designator to edit (`SetRefdes`, undoable).
- Search: the toolbar field, magnifier and ✕; matches refdes, package, part number and board
  names; highlight in the view; "result n of m" with up/down.
- Library mapping (X6, PCB11.4): package → `Representation::{None, FromEcad, Custom{part ref,
  version, transform}}`, stored in the library document's PCB Studio tab; changing it updates
  every PCB Studio using the library. Custom part: Select custom part (this document's parts; other
  documents' versions after 3G), Translate X/Y/Z and Rotate with the triad or typed values,
  Center, ✓.
- Scenarios: `course_pcb_component_view`, `course_pcb_bom` (grouped rows, cross-highlight, refdes
  edit, undo), `course_pcb_search` (n of m, stepping, clear), `course_pcb_custom_part`. Unit
  tests: `bom_groups_by_part_number`, `refdes_edit_is_undoable`, `search_matches`,
  `representation_mapping_round_trips`, `center_puts_the_part_at_the_origin`.
- Images: `v8-component-properties-poster.png`, `v3-interface-poster.png`.

### P3H.5 Sync, export and the board exercise
**Done (P3H.5):** all of it; scenarios `course_pcb_ex1_board` (steps 2–18), `course_pcb_sync_partstudio`, `course_pcb_export_idf` (instead of the three names below), tests in `cadrs_pcb/tests/sync.rs` and `cadrs_core/tests/phone_case.rs`.

- Sync dialog (`ex1-step10-sync-dialog.png`): "Sync a part studio or assembly with PCB studio",
  ✕; "Select part studio or assembly to use:" dropdown of the other Part Studio and Assembly tabs
  as "<tab> (Assembly)" / "<tab> (Part Studio)"; "Top face of board part is parallel to:" dropdown
  (Top/Front/Right Plane); blue OK, grey Cancel. Untranslated-parts toast; re-sync updates the
  board keyed by source (one undo step); assembly sync reads placements.
- Export ECAD files dialog (`ex1-step17-export-ecad-dialog.png`): "Write to IDF Version:" radios
  IDF 2.0 / IDF 3.0 (3.0 selected), blue Export, grey Cancel; writes `<board>.zip` to Downloads
  with a toast.
- Prerequisites (X9): **Create Part Studio in context** in the assembly's Display states ▾ menu
  with the "Origin of new Part Studio" dialog (assembly Origin or a mate connector) creating a
  studio whose `StudioContext` is the whole assembly; **Insert and go to Assembly** (a studio
  header action: pick parts, ✓ inserts them at their studio positions and switches tab). Update
  context from the assembly instance menu (exists for Edit in context; extend the menu with
  "Update context ▸ <studio>").
- Stand-in `cadrs_core::samples::phone_case` (see "What each exercise needs").
- Scenarios: `course_pcb_ex1_board` (steps 2–11), `course_pcb_ex1_resync_export` (12–18),
  `course_pcb_in_context_create` (origin dialog, Insert and go to Assembly). Unit tests:
  `ex1_exported_outline_matches_the_course`, `ex1_outline_before_resize`, `resync_updates_in_place`,
  `untranslated_parts_are_reported`, `export_idf2_has_no_keepouts`.
- Images: `ex1-goal-board.png`, `ex1-step2-part-studio-in-context.png`,
  `v5-translate-board-incontext-poster.png`, `ex1-step10-sync-dialog.png`,
  `ex1-step11-synced-board-keepouts.png`, `ex1-step17-export-ecad-dialog.png`,
  `ex1-step18-exported-emn.png`.

### P3H.6 Create assembly and exercises 2–3 (after 3G is merged)
- Create assembly dialog (`ex2-step5-create-assembly-dialog.png`): "Create assembly from
  '<board>'", "Select features to include in the assembly:", Board ✓, Components ✓, Keep-In and
  Keep-Out Areas ☐, the slow-build note, OK / Cancel; runs on the kernel thread with a spinner and
  a toast when done (non-blocking).
- Generates a Part Studio "<board>" (Sketch 1, "Board [<board>]" feature, part "Board [<board>]",
  keep parts) and an Assembly "<board>"; one component document per new package in the component
  folder (one studio with one part, part number/description from `.emp`, an empty assembly),
  versioned, inserted as version-pinned ExternalRefs; instances named from packages with `<n>`;
  no mates. Open linked document on a component (3G).
- **Transform** feature (Translate/Rotate … with **Copy part** = copy in place) and **Composite
  part** (Closed) for PCB7.9; **box select** in the assembly view (for Group); a bottom-right face
  **Area** readout if P3E.3 isn't on `main`.
- Scenarios: `course_pcb_create_assembly`, `course_pcb_component_docs` (Open linked document, edit,
  version, update), `course_pcb_one_part` (Transform copy in place + Composite), `course_pcb_ex2_vision`,
  `course_pcb_ex3_idf_assembly`. Unit tests: `ex2_large_ic_top_face_area` (232.2576 mm²),
  `ex3_moved_component_is_25_4_mm_further` (Y 8.9), `ex3_place_keepout_written`,
  `create_assembly_makes_one_document_per_new_package`, `second_create_reuses_component_documents`,
  `transform_copy_in_place_keeps_the_original`, `composite_closed_volume_is_the_union`.
- Images: `ex2-step5-create-assembly-dialog.png`, `v6-create-assembly-bom-poster.png`,
  `ex2-step7-area-check.png`, `v7-modify-placement-poster.png`, `ex3-step6-keepout-sketch.png`,
  `ex3-step9-triad-move.png`, `ex3-step14-exported-emn-placement.png`, `course-cover.png`.

### P3H.7 Component documents (after 3G on `main`)
- Create assembly (`cadrs_app::pcb::create_assembly` → `cadrs_pcb::create_assembly::generate_linked`
  with `cadrs_pcb::component_docs::ComponentDocuments`): one stored document per new package in
  the PCB settings' component folder ("PCB Components" when none is chosen; a deleted chosen
  folder is made again with its id and name), named after the package: one Part Studio (named
  after the package) with one part (the ECAD box, Part number, Description, PCB colour), an
  empty "Assembly 1", a V1 and a thumbnail; reused by package + part number
  (`DocumentMeta::pcb_component`) at its newest version. The instances are version-pinned
  references (3G `SourceRef`, frozen copies in `Document::linked`, added by
  `CreatePcbAssembly::links`); `GeneratedAssembly::documents` records which documents are
  components (Sync).
- Open linked document (3G) + the banner's **Edit Main** (the source's workspace, editable) +
  **Back to <document>**; edit → Create version → update badge → Update (3G).
- Scenario `course_pcb_component_documents` (01–12); `course_pcb_ex2_vision` loses 07b (no
  components Part Studio any more).

## Decisions
- **Component documents and undo (P3H.7)**, as 3G's Move to document: the component
  documents, their versions and the folder are library/store work, written when Create runs (on
  the kernel thread, before the board document's command). Undoing Create removes the generated
  tabs and the frozen copies from the board document (one step); the component documents and
  their versions stay (versions are immutable, ER4.8) and the next Create reuses them.
- **Which document is a package's (P3H.7)**: a key in the document's metadata (package and part
  number, plus the Part Studio and part made), not its name or folder, so a renamed or moved
  document still matches; a trashed one doesn't (a new one is made). Same package name with
  another part number is another component (the fixtures' two `SOT23`s).
- **The in-document components Part Studio (P3H.6)** stays only as the fallback with no
  document store (`generate`, pure-core tests); the app always uses component documents.
- **No Onshape name or logo** anywhere: the tab default is "PCB Studio 1", the IDF header source
  id is `"cadrs PCB Studio v0.1"`, the settings build string likewise.
- **Local settings** replace the admin-only cloud settings (X6): per user, in the config dir.
- **0.062 mm thickness quirk:** Ex1 extrudes 0.062 **mm** as the course says (its `.emn` confirms
  it); the stand-in follows it and the test asserts `0.062` in MM. Documented in the sample.
- **IDX and Eagle deferred** (X11); IDF is the workflow.
- **Public exercise documents replaced** by authored fixtures in `fixtures/idf/` (cell phone,
  secondary board, vision controller) and a stand-in phone case whose resized cavity reproduces
  the course's exported outline exactly.
- Course quirk kept out: `ex2-step5` titles the dialog "secondary board" while Vision PCB is
  active; ours uses the active board's name.
- Zip: a small stored/deflate writer on `flate2` (CRC-32 + local/central headers) or the `zip`
  crate with default features off; pick whichever keeps the Windows build simple.
- Until P3F.4, Ex1's Width/Length are the two driving dimensions of the Enclosure's "Case
  outline (Width, Length)" sketch (a sketch dimension can't be named, so the sketch's name says
  which is which; the horizontal one is Width); switch to variables when P3F.4 is on `main`.
- **Sync (P3H.5):** the target of a sync is the board last synced from the same tab (the shown
  one first); otherwise a new board named after the tab. Keep areas keep their ids by the name
  of the part they came from, components by designator; the board keeps its name. **Component
  instances** are recognised by part name (`<refdes> <package>` matching a placement of a board
  of this PCB Studio) until component documents exist (P3H.6); their package frame is recorded
  on the synced board at the first sync. Only the Top/Front/Right planes are offered.
- **Export (P3H.5):** `<board>.zip` goes where the other exports go (`$CADRS_EXPORT_DIR`, else
  the Downloads folder, else home; scenarios: their `exports` folder), " (2)" when taken; the
  header's date is the app clock's.
- **Managed in-context (P3H.5):** a context created in an assembly has the assembly Origin as its
  origin (`InstanceId::ORIGIN`, studio coordinates = assembly coordinates) and holds every part
  of the assembly not from the studio itself; every context also records a fingerprint of its
  parts' studios' features, so editing them (not only moving instances) shows "Assembly
  changed" and makes Update context take effect.
- **Workspace-level settings (P3H.4):** the settings live in `<store>/pcb-workspace.ron`; each
  PCB Studio keeps a copy, changed only by `SetPcbSettings` (so Update and its undo are
  ordinary undo steps) and kept in step by `LibrarySync` (a changed copy is written out; a
  studio seen for the first time reads it, with no undo step). The library is a **stored cadrs
  document** whose first PCB Studio tab's `library` holds the mappings (with no library chosen:
  `<store>/pcb-component-library.ron`); the component folder is a documents-page folder.
- **Which board is shown** is view state (not undone, not saved): an import or delete still
  shows its board through the element's `active`.
- **Grouped designators (PCB3.10):** a BOM row of several components opens (double-click its
  designators) into one row per component; each component's designator is edited in place.
  Designators must be unique on the board (case-insensitive) and have no spaces.
- **Custom part transform:** the part is rotated about X, then Y, then Z (through its origin),
  then translated, into the package frame; the Rotate fields turn it about its middle (the
  translation follows), and Center puts its box's middle on the footprint origin with its bottom
  on the board. The picker offers every stored document (and this one) with its versions.

## Risks
- **3G dependency:** P3H.6 (and custom parts from other documents) can't finish without
  ExternalRef, Open linked document and cross-document version updates.
- **Missing prerequisites grow P3H.5/P3H.6:** Create Part Studio in context, Insert and go to
  Assembly, Transform, Composite part, box select and a face Area readout are all new.
- **Viewer:** the PCB viewport and component view are a new scene type; reusing the Part Studio
  mesh path (or Repair's render-to-image view) matters for the ~30-part boards' frame time.
- **IDF 2.0 vs 3.0:** the spec may allow some keep-outs in 2.0; we follow the course and record it.
- **Board-frame mapping:** the "top face parallel to" plane and the export's X/Y must match the
  course's numbers (Ex1 points, Ex3 +25.4 mm); tests pin both.
- **Store schema** and `document.rs` are shared files: keep the new kind additive.
