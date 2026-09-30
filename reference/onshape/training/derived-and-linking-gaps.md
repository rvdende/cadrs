# Derived feature and linked documents: gap analysis (2026-09-29)

This maps [derived-and-linking.md](derived-and-linking.md) (`DV*`, from the Onshape Help pages
"Derived" and "Linking documents") and [external-references.md](external-references.md) (`ER*`,
the Learning Center course "Linked Documents") against the cadrs code on branch `phase3c` at
`26ea0b1` (main `b48ea20` merged in: 3B closed, 3D passed, P3D.3 versions landed). ✅ done ·
🟡 partial · ❌ missing · **out of scope** (only cloud and multi-user collaboration, sharing and
permissions, release management and paid tiers, Onshape account and learning-site features, and,
by user decision 2026-09-29, configurations, image tabs, full round/conic/curvature fillets and a
detailed Versions panel). Milestones are **P3G.1–P3G.5** (stage 3G, worker C). **P3F.1 has moved
here**: its rows in the other gap files now name a P3G milestone. Related:
[essential-tips-gaps.md](essential-tips-gaps.md) (T1.3, T3.*, T7.*),
[intro-to-assemblies-gaps.md](intro-to-assemblies-gaps.md) (A2.3, X15),
[intro-to-drawings-gaps.md](intro-to-drawings-gaps.md) (D2.10, D13.1, X1, X14),
[inspection-and-repair-gaps.md](inspection-and-repair-gaps.md) (P3D.3 history and versions),
[test-drive-gaps.md](test-drive-gaps.md) (TD3.8, TD8.1),
[intro-to-parametric-cad-gaps.md](intro-to-parametric-cad-gaps.md) (P1.3) and
[crates/cadrs_kernel/README.md](../../../crates/cadrs_kernel/README.md).

**Summary:** cadrs has everything *inside* one document and nothing *between* documents or at a
version. What exists:
- **Versions (P3D.3).** `cadrs_core::history_log`: an append-only `HistoryLog` per document in
  `<store>/<id>/history.ron`, with a full snapshot every 16 entries (`SNAPSHOT_EVERY`, l.41).
  A `Version` (l.164) is an immutable named pointer to an entry: `create_version` l.357 (empty
  name → "V<n>"), `versions` / `version` / `document_at_version(id)` l.380–389 (a copy plus at
  most 15 deltas). Create version from the History panel header and the document menu
  (`history_panel.rs:535`, a `NamePopup` with name and description); versions are squares on the
  rail (`cadrs_ui::TimelineRow`, `timeline.rs:35`); right-click **Open read-only** opens the
  view-only Repair panel, **Part Studios only** (`repair.rs:288`, `open_entry` l.302 skips
  assemblies and drawings). No caller outside tests uses `document_at_version`. The top bar's
  "Main" label and versions/branches counters are static ("0", `cadrs_app/src/document.rs:456-470`).
  No branches or version graph (the detailed panel is out of scope; the small version-graph
  *picker* the course needs is not the detailed panel and is in scope).
- **Documents and library.** `cadrs_core::store` (`<root>/<uuid>/document.ron`, schema v4,
  `store.rs:40`), `library.rs` (`DocumentMeta` with created/modified/owner/last_opened/trashed,
  `Filter::{OwnedByMe, RecentlyOpened, CreatedByMe, Trash, …}` l.130, folders that **hold
  nothing**: no folder field on a document, no labels model). Documents page (`landing.rs`) with
  Copy… (`open_copy_dialog` l.2015 → `Store::copy_document` l.278, which **does not copy
  `history.ron`**), Rename, Trash/Restore/Purge (undoable `LibraryHistory` commands).
  `DocumentId`, `ElementId`, `FeatureId`, `PartId` are uuid newtypes (`ids.rs`).
- **Insert dialogs.** Assembly Insert (`assembly/insert.rs`): Current document ✅, Standard
  content ✅ (P3B.5), **Other documents disabled** (`.disabled(1)`, l.178-186), version button
  `insert-version` disabled (l.214), Part Studios / Assemblies sub-tabs with search. Drawing
  Insert view browser (`drawing/view_tools.rs:586`): Current document ✅, **Other documents
  disabled** (l.631-637). **No Part Studio insert or Derived dialog.**
- **References inside one document.** `InstanceSource::{Part{element, part}, Assembly{element},
  Studio{element}}` (`assembly/mod.rs:197`): always the live workspace, no document or version.
  Drawings keep a frozen snapshot per source: `cadrs_drawing::ModelSource { element, snapshot
  (RON StudioState), parts: Vec<PartHash>, assembly }` (`cadrs_drawing/src/lib.rs:104`),
  written by `cadrs_core::drawing_source` / `drawing_assembly::source_of`, updated on demand
  (`drawing/update.rs`). That snapshot-plus-update model is exactly what a version-pinned link
  needs, but it isn't tied to versions. **No `ExternalRef` type anywhere.**
- **Instances list and menus** (`assembly/list.rs`, `menu.rs`): Copy/Paste (one instance only,
  though the label says "Copy N items", `menu.rs:545`), Replace instances (this document's parts),
  **Where used…** (this document; "Other documents: … (P3F.1)" note, `where_used.rs:93`),
  **Change to version…** disabled with a stale P3D.3 tooltip (`menu.rs:162, 235`). Mass
  properties over a multi-selection of instances, subassemblies expanded (`mass_props.rs:180`).
- **Tabs.** `AddElement`, `DuplicateElement`, `RenameElement`, `DeleteElement`, `InsertElement`
  commands; tab menu **Move to document…** disabled (`cadrs_app/src/document.rs:1975`); the
  `tab-manager` button has no handler (l.766); no tab multi-select.
- **Persistent naming (P3.2).** Names come only from feature uuids, sketch curve ids and region
  keys (`cadrs_kernel::naming`), so a studio rebuilt at a version, or in a copied document, gets
  **identical** names and `PartId`s. Good for mates surviving an update; a hazard for Derived
  (a derived copy of a duplicate collides with the host's names) unless the copies are
  namespaced like pattern instances (`FaceOrigin::Instance`, `kernel_ops/pattern.rs:59`).
- **Kernel.** `transform` / `transform_motion` copy a body with a `modified` history
  (`occt.rs:979, 999`), exact `mass_properties`, `export_step`. No public plain copy, no BRep
  read/write, one kernel session per `Rebuilder`. `Rebuilder::placed_body` (`rebuild.rs:2530`)
  already builds *another* feature list and transforms a part, but calls `rebuild()`, which resets
  `trail`/`last`, so it isn't reentrant inside `compute`. The cache key (`chain_key`, l.273)
  covers only the host feature list.
- **Feature list.** 17 `FeatureKind`s; rows don't expand (only folders do); sketches have eye
  toggles; error/warning glyphs (`cadrs_app/src/document.rs:2713-2750`). Mate connectors in
  studios (`mate.rs`, `SolidConnector`), implicit connectors keyed by persistent names
  (`assembly/connector.rs`). A mate whose entity is lost **silently** keeps its stored frame
  (at the plan; P3G.5 makes it an error, `assembly::lost_mates`).

So the stage is: one reference model and resolver (with frozen snapshots and version loading),
the Other documents browser and version picker shared by every Insert-style dialog, the update
UI (badges, Reference manager, pins, update all with auto versions), Open linked document and
Move to document, and the Derived feature.

**Cross-stage dependencies.** Needs P3D.3 (versions, landed), P3B (instances, mates, Insert,
landed), P3C.2/P3C.6 (drawing Insert view and snapshots, landed), P3D.4 (Replace reference and
Repair, landed). Two rows touch worker A's later stage 3E: the **labels** location in the Other
documents browser (ER1.2) and the document details panel (ER8 step 5) come from P3E.1; the full
**Tab manager** (ER7.7) is P3E.2. 3G runs before 3E, so P3G.1 adds folder membership (needed for
folder locations anyway) and shows labels once P3E.1 adds them, and P3G.3 builds a minimal Tab
manager (list, multi-select, Move to document) that P3E.2 extends. 3G closes these rows elsewhere:
A2.3 and X15 (assemblies), D2.10, D13.1, X1, X14 (drawings), T1.3, T3.*, T7.3, T7.5 pins, X1, X3
(essential tips), TD3.8 where used and TD8.1 (test drive), P1.3 search (parametric CAD).

**Counts (24 DV IDs + 5 DV exercises + 2 DV X; 46 ER IDs + 27 ER exercise rows + 8 ER X = 112),
after P3E.1 (2026-09-29, pre-judge):** ✅ 112 · 🟡 0 · ❌ 0 · out of scope 0 (DV: 24 / 0 / 0; DV
exercises: 5 / 0 / 0; DV X: 2 / 0 / 0; ER1–ER5, ER7: 46 / 0 / 0; ER6 and ER8 steps: 27 / 0 / 0;
ER X: 8 / 0 / 0): P3E.1 closed the two carried rows, ER1.2 and ER8.5. After the 3G cleanup it
was ✅ 110 · 🟡 2 (both carried to P3E.1). After P3G.4 it was
✅ 103 · 🟡 6 · ❌ 3; after P3G.3 it was ✅ 92 · 🟡 6 · ❌ 14; after P3G.2 ✅ 76 · 🟡 9 · ❌ 27; after P3G.1 ✅ 41 · 🟡 15 · ❌ 56; at the plan (26ea0b1)
✅ 8 · 🟡 25 · ❌ 79. No whole row is out of
scope; the cloud parts inside rows (sharing in DV1.12, teams and "Shared with me" in ER1.2, release
markers in ER3.2, Revision history and Add comment in ER6 step 7, URL paste and teams in ER X8) are
out of scope and noted in the row. ER6/ER8 steps are numbered `ER6.<step>` / `ER8.<step>` here
(the requirements file numbers them only as steps).

**P3G.1 done (2026-09-29, judge 8.6).** `cadrs_core::external` (the reference, frozen
content-addressed copies under namespaced ids, the resolver and its cache, the cycle check, link
states; schema 5 with a v4 migration; `DocumentMeta::folder`), `cadrs_ui::{VersionGraph,
DocumentRow, BrowserSearch, OpenedHeader}`, the Other documents tab and version graph in the
assembly Insert dialog and the drawing Insert view browser, the Create version dialog and top-bar
button with a live counter, linked icons on instance rows, copy/paste of several (linked)
instances, Move to ▸ a folder on the documents page. Scenarios `course_er_insert_linked` (20
frames), `course_er_versions_in_document` (14), `course_drw_version_reference` (13); tests
`cadrs_core/tests/external_refs.rs` (11). No kernel change: a copy's features are rebuilt in the
consumer's own session.

**P3G.2 done (2026-09-29, judge 8.5).** `cadrs_core::link_update` (uses of references in
assemblies and drawings, the direct and transitive stale query, `UpdateReferences`, `SetPinned`,
Update all with auto versions in intermediate documents, library-wide where used),
`Version::auto`, `ModelSource::pinned`; the app's badges (instance rows, tabs, Sheets pane), the
Reference manager (`cadrs_app::reference_manager`), the instance, tab and Sheets pane menu items,
the toolbar's Update all references, the error toast. Scenarios `course_er_update_linked` (14),
`course_er_reference_manager` (12), `course_er_update_all` (10), `course_er_pinning` (16),
`course_er_change_to_version` (13); tests `cadrs_core/tests/link_update.rs` (7).

**P3G.3 done (2026-09-29, judge 8.5).** `cadrs_core::move_doc` (referenced tabs, the move as
one checked-first multi-document transaction: the source's auto version when a tab stays
behind, the target and its version written, then `MoveTabs` for the source's undo history;
`Document::moved` and "Update to the new document"), `cadrs_core::samples::piston` (the piston /
hexapod stand-in, reusable by P3G.5); the app's read-only linked session
(`cadrs_app::linked_session`), the Move to document dialog and the minimal Tab manager
(`cadrs_app::move_document`), the Reference manager's moved rows, nested rows and open buttons.
Scenarios `course_er_open_linked` (13), `course_er_move_to_document` (19),
`course_tips_move_tab` (12); tests `cadrs_core/tests/move_document.rs` (5) and
`cadrs_app::tests::a_read_only_document_refuses_edits`.

**P3G.4 done (2026-09-29, judge 8.7).** `cadrs_core::derived` (`DerivedFeature`: the source
reference and its copy, the source's features and part settings at the reference, the
selection, locations, placement, the two options; `resolve`, `check` (self, twice, cycles),
`AddDerived` / `SetDerived`, `refresh` for live workspace references, derived part / face / entity
ids), `rebuild/kernel_ops/derived.rs` (the source rebuilt inside the host's rebuild, copies moved
frame to frame and renamed as pattern instances; derived sketches, planes and connectors),
`link_update::RefSite::Derived` (update, pin, where used, the Reference manager), the conformance
case `derived_frame_motion`; the app's Derived dialog (`cadrs_app::derived_ui`, the icon rail's
Insert), the Feature list's Derived row (chevron, children with eyes, linked icon, menu).
Scenarios `course_dv_derived_dialog` (19), `course_dv_derived_options` (18 after fix round 1); tests
`cadrs_core/tests/derived.rs` (11). Carried P3G.3 deltas fixed: toasts wrap in a capped width
(clear of the view cube), the move toast's wording, the Reference manager's group label, linked
instance rows' room, equal Move dialog tabs, Back restores the selection, the open button's
tooltip beside it, "1 version" rows in the Move dialog, the Sheets pane reference's tooltip, the
tab menu's Update linked document… icon.

**P3G.5 built (2026-09-29, pre-judge).** The exercises on stand-in fixtures (see "Stand-in
substitutions" below): `linked_block_standin` (+ `.history.ron`, V1), `linked_block_host_standin`,
`linked_base_standin`, `linked_piston_standin`, `linked_hexapod_standin` and
`move_to_document_standin`, each with a `*_fixture_is_current` test in
`cadrs_core/tests/course_linked.rs` (15 tests: the fixtures, ex-dv1–ex-dv5, the piston, the
Hexapod at V1, V2 and back, the mated pistons' poses, no false lost mates in any shipped
assembly). A mate whose entity is lost after an update is an **error** now
(`assembly::lost_mates`, `MateConnector::resolves`): its row in Mate Features is red with
"Missing reference: …" in its tooltip, and it no longer holds its instances at the stored frame;
an update re-solves the updated assemblies' mates (`assembly::resolve_after_update`, called by
`UpdateReferences`), so the Topplate rises with longer pistons. Scenarios
`course_dv_ex1_two_copies` (7), `course_dv_ex2_update` (9), `course_dv_ex3_workspace` (8),
`course_dv_ex4_assembly_update` (10), `course_dv_ex5_circular` (5), `course_er_ex1_hexapod` (22),
`course_er_ex2_move` (10); `course_dv_derived_options` gains 09b. Carried P3G.4 minors: a Base
origin + Include mate connectors frame (`course_dv_ex1_two_copies` 06), the Extrude field's
"Face of Sketch 2 (Derived 1)" (`course_dv_derived_options` 09b), a new Derived feature stays
neutral while its dialog waits for a source (`course_dv_ex1_two_copies` 02), the derived-sketch
part has a material (`course_dv_derived_options` 09); the toolbar's Update-all slot is not
reserved (see PROGRESS.md, Phase 3 decisions).

## DV1 Linked-document references
| ID | Requirement | Status | Notes |
|---|---|---|---|
| DV1.1 | Other-document references pin a version; same-document ones follow the workspace or pin a version | ✅ | Same-document instances follow the workspace live (P3B.1) or, picked in the Insert dialog's version graph, pin a version (ER3.1); another document is always referenced at a version (**P3G.1**: `SourceRef { document, at: Workspace | Version, element, pinned }` in `cadrs_core::external`; `course_er_versions_in_document` 06–12, `course_er_insert_linked` 06–14; `external_refs.rs::a_same_document_version_reference_is_frozen_and_a_workspace_one_follows`). |
| DV1.2 | Stored reference `(document, version \| workspace, element, entity)`, resolved read-only and cached | ✅ | **P3G.1**: `SourceRef` on the instance (`Instance::link`), the entity in its source (part, whole studio, assembly); `Resolver` loads `document_at_version` read-only through the store, cached by `(document, version)` until `history.ron` changes; the consumer keeps the frozen, content-addressed copies (`Document::linked`, schema 5). **P3G.4**: a Derived feature holds the same reference (`DerivedFeature::source`, its copy in `Document::linked`) and brings parts, sketches, planes and mate connectors of it (DV3.2). Tests: `external_refs.rs` (all), e.g. `external_refs.rs::resolving_a_version_twice_reads_the_history_once`; `course_er_insert_linked` 10. |
| DV1.3 | Linked icon; blue "update available" when the source has a newer version | ✅ | **P3G.1**: the plain linked icon on instance rows, red while the source can't be reached (`course_er_insert_linked` 10, 19: the whole icon red since P3G.2). **P3G.2**: the blue badge when the source has a newer version, on the row and on the tab (`course_er_update_linked` 02–03; `link_update.rs::update_to_latest_brings_the_new_version_and_undo_restores_it` checks the stale query), the transitive and pinned states (ER X2), and the Sheets pane's reference (`course_er_pinning` 11). **P3G.4**: Derived feature rows get the same icon and states (`course_dv_derived_dialog` 10–11, 15). |
| DV1.4 | Update to latest or a chosen version; undoable; "update all" | ✅ | **P3G.2**: `cadrs_core::link_update::UpdateReferences` (one undo step; new copies added, instances re-pointed with their ids, poses and mates, unused copies dropped), from the Reference manager's Update to latest / Selective update and the toolbar's Update all (`course_er_update_linked` 05–09: 3 · 37 500 → 3 · 60 000 = 180 000 mm³, undo 112 500; `link_update.rs::update_to_latest_brings_the_new_version_and_undo_restores_it`, `selective_update_goes_back_to_an_older_version`: V3 82 500 → V1 37 500 → V2 60 000, undone back). |
| DV1.5 | Circular references refused with a clear error | ✅ | **P3G.1**: inserts are checked (`external::check_cycle`, refused when the copies reach the consumer's own element at any version, with the path "A › X → B › Y → A"; also a version of an assembly into itself; `external_refs.rs::a_circular_insert_is_refused`; the Insert dialog shows it as a toast, `course_er_insert_linked` 18c). **P3G.4**: Derived chains too (`derived::check`: through this document's workspace references, and through a version's copies across documents; a Part Studio deriving itself is refused as well): `derived.rs::derive_cycles_are_refused` ("Circular reference: Block source › Block → Block source › Part Studio 2 → Block source"; across documents "Block source › Block → Block derived › Part Studio 1 → Block source", nothing added), the red toast in `course_dv_derived_options` 15. |
| DV1.6 | Where used across documents, with versions | ✅ | Where used… lists this document's assemblies (P3B.9). **P3G.2**: `link_update::where_used` / `usages_in` (every library document referencing a document, its tab, the referenced tab and version, the number of uses); the Where used dialog lists them for a tab or a linked instance's source (`course_er_update_linked` 14: Block consumer › Assembly 1 (V3): 3 uses, Block user 2 › Assembly 1 (V1): 1 use; `link_update.rs::where_used_lists_the_documents_that_reference_a_document`). The documents page's details panel shows it with P3E.1 (TD3.8). |
| DV1.7 | Missing source: error state, Replace/Repair, the three messages | ✅ | **P3G.1**: `LinkState` Trashed ("Cannot open a document in the trash. Restore the document from Trash."), Gone ("Resource does not exist"), Inaccessible (local stand-in: the store can't read it; "You cannot modify this feature because you cannot access the referenced document"); the instance's link icon turns red with the message and the geometry keeps coming from the stored copy (`course_er_insert_linked` 19–20; `external_refs.rs::a_trashed_or_purged_source_keeps_the_geometry_and_says_why`); Replace instances… applies to linked instances (P3D.4/P3B.9). Update and Open are not offered for them (P3G.2 disables update for these states). |
| DV1.8 | A version is required to link; inline Create version in Other documents | ✅ | Create version (P3D.3). **P3G.1**: a document without versions shows "No versions" in the browser and, opened, the message and an inline **Create version** (the "Create version from Main" dialog), after which the new version is opened and selected (`course_er_insert_linked` 16–18); the Current document header's Create version selects the new version too (`course_er_versions_in_document` 13–14). |
| DV1.9 | Open linked document (instance or feature menu) at the linked version, read-only, source tab active, part selected | ✅ | **P3G.3** (`cadrs_app::linked_session`): the instance menu's **Open linked document** (also the tab menu, a linked icon's row menu, the Sheets pane reference's menu and the Reference manager's rows) opens the source document **at the referenced version** in this window, the source tab active and the referenced part selected; the session is read-only (`ActiveDocument::read_only`: every command, undo and redo refused with a toast, the toolbar and **+** turned off, features can't be opened, nothing saved), the top bar reads "V1" instead of "Main", a blue banner "Viewing V1 of Block source (read-only)" with **Back to Block consumer**, which returns to the consumer exactly as it was (tab, undo history). Same-document version references open this document at the version (`course_er_open_linked` 01–13; `lib.rs::a_read_only_document_refuses_edits`). **P3G.4**: the Feature list's Derived row menu has Open linked document (`course_dv_derived_dialog` 13–14). |
| DV1.10 | Reference Manager: grouped by source document, current and newest version, per-reference target, update one/several/all, undoable | ✅ | **P3G.2**: `reference_manager.rs`: Update to latest (one row per source document and version, "V1 ⇒ V2" / "V1 ⇒ new version", Update all) and Selective update (one row per reference with a checkbox and its target, picked in the version graph; Update selected); every update one undo step (`course_er_reference_manager` 02–12, `course_er_update_linked` 05–09). |
| DV1.11 | Propagation: same-document instant, cross-document on request, versions immutable | ✅ | Same-document instant ✅ (P3B.1); versions immutable ✅; cross-document links never change until updated (P3G.1, `course_er_insert_linked` 12); **P3G.2**: the update on request (`course_er_update_linked` 06). |
| DV1.12 | Sharing out of scope; local library documents can be linked; removed ones behave as deleted | ✅ | Sharing itself is out of scope (cloud permissions). **P3G.1**: any document in the local library can be linked (the Other documents browser lists the store); a trashed or purged one behaves as DV1.7 (`course_er_insert_linked` 03–05, 19–20). |

## DV2 Assembly Insert from other documents
| ID | Requirement | Status | Notes |
|---|---|---|---|
| DV2.1 | Other documents tab: library list, search, filters, version (newest default), then part / Part Studio (rigid or not) / assembly / sketch / connector-bearing part | ✅ | **P3G.1**: the Other documents tab: search (names, or a pasted document id), locations, a document's newest version by default and the version graph for an older one, then Part Studios (with Insert as rigid) and Assemblies with thumbnails, as the Current document (`course_er_insert_linked` 02–08). Sketches aren't insertable in cadrs's assemblies at all (the filter stays disabled); parts carry their mate connectors. |
| DV2.2 | Inserted instances carry the pinned reference, show the linked icon, follow DV1.3–DV1.4 | ✅ | **P3G.1** the reference and icon (`course_er_insert_linked` 10, 15); **P3G.2** the badge and the update (`course_er_update_linked` 02–06). |
| DV2.3 | Mates, patterns, BOM work as for same-document; BOM keeps the source's properties at that version | ✅ | **P3G.1**: linked instances are ordinary instances of their copies: mates, patterns and the BOM work, and the BOM reads the part's properties at the version (`external_refs.rs::linked_instances_mate_and_keep_their_properties_in_the_bom`: a Fastened mate puts the V1 block on the workspace one at z 25; 2 × BLK-001 at V1 beside 1 × BLK-002 in the workspace). |

## DV3 Derived feature (Part Studio)
| ID | Requirement | Status | Notes |
|---|---|---|---|
| DV3.1 | One-way associative copies from another Part Studio (same or other document) | ✅ | **P3G.4** (`cadrs_core::derived`, `FeatureKind::Derived`): the source's features and part settings travel in the feature at the reference; the source is rebuilt inside the host's rebuild and its parts copied (`rebuild/kernel_ops/derived.rs`); edits flow only from the source (a version: on Update; this document's workspace: at once). `derived.rs::ex_dv1_two_copies_at_the_origin_and_at_a_connector`, `course_dv_derived_dialog` 08–10. Documents saved by main's pre-merge importer Derived feature load into this one (3G cleanup: `import_derived.rs::main_pre_merge_derived_format_loads_and_rebuilds`; the importer's own flow, `scenarios/onshape_derived.ron`, now drives this dialog). |
| DV3.2 | Parts, sketches, surfaces, curves, planes, mate connectors (explicit, implicit), whole studio; sheet metal as static | ✅ | **P3G.4**: *Part Studio* (everything) or ticked parts (solids and surfaces alike), sketches (with their curves: cadrs has no separate curve features), Plane features and explicit mate connectors; a source mate connector (explicit, or implicit on a source entity) as the base. Derived planes are drawn, labelled, picked and hidden like the host's own (`course_dv_derived_options` 08, 10–12); derived connectors render like the host's and travel with their part (05, 08; `base_mate_connector_placement…`: the connector at (0, 0, 100) on the derived part); derived sketches are sketches later features take (`derived.rs::a_derived_sketch_extrudes`: 20·10·5 = 1 000; 08–09; the Extrude field names them "Face of Sketch 2 (Derived 1)"). Not derived: a source's *implicit* connectors as entities of their own (only as the base), and its sheet metal / composite parts (cadrs has neither). |
| DV3.3 | Dialog: source picker (Current \| Other documents, search, version), entities, Locations, Placement (Base origin \| Base mate connector), Include mate connectors, Include properties | ✅ | **P3G.4** (`cadrs_app::derived_ui`, the icon rail's Insert in a Part Studio): Current document (this document's other Part Studios at Main, or at a version from its graph) \| Other documents (P3G.1's browser: search, locations, newest version, version graph, Create version), the source card with Change, Derive (Part Studio or each entity), Locations (mate connectors or the origin picked in the view or the list), Placement (Base origin \| Base mate connector, the source's connectors), Include mate connectors, Include properties (`course_dv_derived_dialog` 02–09, `course_dv_derived_options` 02–05, 13–14; `derived.rs::include_properties_off_copies_only_the_name_material_and_appearance`). |
| DV3.4 | Workspace references live; version references show the indicator and update on demand | ✅ | **P3G.4**: this document's workspace references follow after every command, undo and redo (`derived::refresh`; `derived.rs::ex_dv3_a_workspace_derive_follows_the_source`: 37 500 → 45 000, undo 37 500; `course_dv_derived_options` 16–18: Part Studio 1's hole suppressed, Plate's derived block follows); version references take P3G.2's badge and update (`RefSite::Derived`; `derived.rs::update_to_a_new_version_brings_the_new_volume_and_undo_restores_it`: 37 500 → 60 000, undo 37 500; `course_dv_derived_dialog` 15–17). |
| DV3.5 | One expandable feature-list entry with children and eye toggles, linked icon; derived parts usable downstream; derived sketches extrudable | ✅ | **P3G.4**: the Derived row's chevron opens its children (parts and sketches with eyes, planes, mate connectors), its linked icon and states (`course_dv_derived_dialog` 10–12, `course_dv_derived_options` 10–12: a derived plane's eye hovered and clicked); derived parts are ordinary parts (a Ø10 hole cut through one: 37 500 − 25π·25 = 35 536.505, `derived.rs::a_hole_cut_into_a_derived_part`, `course_dv_derived_options` 07; a fillet on a derived edge of a duplicated studio survives a source edit, `a_fillet_on_a_derived_edge_of_a_duplicated_studio_survives_a_source_edit`); a derived sketch extrudes to 1 000 (`a_derived_sketch_extrudes`, `course_dv_derived_options` 09). |
| DV3.6 | Edit / swap source; downstream failures use the normal repair UI | ✅ | **P3G.4**: double-click or Edit… reopens the dialog; Change picks another source (its own selection starts from Part Studio); every change is one `SetDerived` step, ✓ one undo step, ✕ takes them back (`course_dv_derived_options` 02–05, 08, 14; `derived.rs::editing_a_derived_feature_swaps_what_it_brings`); a later feature that loses its derived input fails as any feature does (P3D.4's repair UI). |
| DV3.7 | Rules: no double derive of one studio, one source per feature, no cycles, source visibility ignored | ✅ | **P3G.4** (`derived::check`): a Part Studio derived twice in one Part Studio is refused ("Block is derived already in Part Studio 1 by Derived 1: a Part Studio can be derived only once in a Part Studio", `derived.rs::deriving_one_studio_twice_or_itself_is_refused`, `course_dv_derived_dialog` 18), one source per feature (the model has one), cycles refused (DV1.5), the source's hidden parts are derived and shown (visibility is the host's own). Many locations work (one copy each). |
| DV3.8 | Mass properties, STEP export and drawings treat derived parts as native | ✅ | **P3G.4**: derived parts are ordinary `Part`s of the host's build with their material and appearance (the host's settings follow the source's), so mass properties (`course_dv_derived_options` 01, 06, 07; ex-dv1's inertia in `derived.rs`), STEP export and drawing views (they build the host's features) take them unchanged. |

## DV4 Move to document
| ID | Requirement | Status | Notes |
|---|---|---|---|
| DV4.1 | Move a tab; target gets a version; source references rewritten to pinned external refs; world transforms kept | ✅ | **P3G.3** (`cadrs_core::move_doc`): the target gets a version (an auto V1 in a new document, a named one in an existing document); the source's uses of the moved tabs are re-pointed at that version (one undo step), instances keep ids, poses and indices, so every world pose is unchanged (`move_document.rs::moving_the_piston_assembly_takes_two_tabs_and_keeps_every_world_pose`: 26 occurrences to 1e−9, V 589 534.041, centroid z 50.348; `course_er_move_to_document` 06–08). The references are ordinary version references (pinnable; not pinned by the move, as Onshape). |

## DV exercises
| ID | Exercise | Status | Notes |
|---|---|---|---|
| ex-dv1 | Derive A's 50×30×25 block at the origin and at (0,0,100): 2 copies, 2× volume, centroid Z +50 | ✅ | **P3G.4**: the feature and its values (`derived.rs::ex_dv1_two_copies_at_the_origin_and_at_a_connector`). **P3G.5** on the fixtures: `course_dv_ex1_two_copies` 01–07 (Other documents › Linked parts › Block source V1, the Origin and Mate connector 1 picked as Locations, 04 two copies, 05 mass 75 000 mm³, 14 000 mm², centroid (25, 15, 62.5)); `course_linked.rs::ex_dv1_two_copies` (2·50·30·25, 2·2(50·30 + 50·25 + 30·25), z̄ (12.5 + 112.5)/2, Ixx 2(V(30² + 25²)/12 + V·50²) = 197 031 250). |
| ex-dv2 | Edit A: B unchanged, indicator, Update brings the new volume | ✅ | **P3G.5**: `course_dv_ex2_update` 01–09 (A's Extrude 1 25 → 40 in A's own document, V2 from the top bar, B's badge with 75 000 unchanged, the Reference manager "V1 ⇒ V2", 08 Update all 120 000 mm³, 18 800 mm², centroid (25, 15, 70), 09 undo 75 000); `course_linked.rs::ex_dv2_update_brings_the_new_version` (2·50·30·40, 2·2(1500 + 2000 + 1200), z̄ (20 + 120)/2, the stale query). |
| ex-dv3 | Same-document workspace derive updates immediately | ✅ | **P3G.5**: `course_dv_ex3_workspace` 01–08 (a new Part Studio 1 derives "Block" from Current document, 04 37 500 with no link icon, 05 Sketch 1's length dimension 50 → 60, 07 Part Studio 1's derived part reads 45 000 mm³ at once, 08 Ctrl+Z, then Part Studio 1: its derived part back at 37 500); `course_linked.rs::ex_dv3_a_workspace_derive_follows_a_length_edit` (60·30·25, no version use). |
| ex-dv4 | Assembly in C inserts A at v1, mates, updates to v2; mates survive | ✅ | **P3G.5**: `course_dv_ex4_assembly_update` 01–10 (Insert → Other documents → Block source V1 into C's Assembly 1, 04 Fastened 1's dialog, 05 137 500 mm³, z̄ 9.773, 06–07 the badge and the manager, 08 V2: 160 000 mm³, z̄ 14.375, the mate healthy, 09 V3 with every face renamed: Fastened 1 red, "Missing reference: …", 10 undo: healthy); `course_linked.rs::ex_dv4_mates_survive_an_update_and_a_lost_face_is_an_error` ((100 000·5 + 50·30·h·(10 + h/2))/(100 000 + 1500h) for h 25 and 40; `lost_mates` empty at V2, `[Fastened 1]` at V3, empty after undo). The mate re-solves on update (`assembly::resolve_after_update`). |
| ex-dv5 | A→B→A derive refused | ✅ | **P3G.5**: `course_dv_ex5_circular` 01–05 (B derives A's Block at V1 and is versioned from the top bar; in A, Derived → Other documents › Block derived V1 › Part Studio 1: 04 the red toast "Circular reference: Block source › Block → Block derived › Part Studio 1 → Block source", 05 nothing added); `course_linked.rs::ex_dv5_a_circular_derive_is_refused` (the message, the document unchanged). |

## DV cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| DV X1 | One `ExternalRef` model and resolver/cache in `cadrs_core`, shared by assemblies, Derived and drawings | ✅ | **P3G.1**: `cadrs_core::external` (`SourceRef`, `LinkedElement`, `LinkSnapshot`, `Resolver`, `check_cycle`, `LinkState`), used by assembly instances and drawing views; Derived (P3G.4) uses the same (`derived.rs`, `course_linked.rs::ex_dv1_two_copies`). |
| DV X2 | Versions cheap to load read-only | ✅ | **P3G.1**: the resolver reads a `history.ron` once per change (`external_refs.rs::resolving_a_version_twice_reads_the_history_once`: two resolves, one read; a version of a 100-edit history in well under the 2 s bound). |

## ER1 Inserting linked documents
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER1.1 | Every Insert-style dialog has Current \| Other documents \| Standard content | ✅ | **P3G.1**: the assembly Insert dialog (Current | Other documents | Standard content, `course_er_insert_linked` 02) and the drawing Insert view (Current | Other documents, `course_drw_version_reference` 03, 10) both have Other documents; **P3G.4**: the Derived dialog has Current document | Other documents (Standard content has no Part Studios to derive) (`course_dv_derived_dialog` 02–05). |
| ER1.2 | Other documents browser: filter, search, locations (My, Recent, Created by me, Shared, teams, labels), rows with thumbnail, name, version subtitle | ✅ | **P3G.1**: the filter button, "Search or paste document id", My documents, Recently opened, Created by me and the library's folders (a document's folder: `DocumentMeta::folder`, the documents page's Move to ▸), then rows with thumbnail, name and newest version (`course_er_insert_linked` 03–04). **P3E.1**: each label is a location (tag icon) after the folders, listing its documents (`Location::Label`, `Library::with_label`; `course_td_documents_labels` 17–18). Shared with me and teams are out of scope (sharing). |
| ER1.3 | Paste a document URL into search | ✅ | **P3G.1**: a document id pasted into the search lists that document (`course_er_insert_linked` 05). |
| ER1.4 | Search inside a document by object name or property | ✅ | **P3G.1**: the Part Studios lists filter by part name, part number or description, in this document and in other documents at a version (**P3G.5** frame: `course_er_ex1_hexapod` 04b, "Rod" in the Piston document at V1). |
| ER1.5 | A linked source needs a version | ✅ | **P3G.1** (as DV1.8: `course_er_insert_linked` 16–18). |
| ER1.6 | Opened-document header: name, "↳ V1", back arrow, Create version and Version graph icons; Part Studios \| Assemblies sub-tabs with search | ✅ | **P3G.1**: name, "↳ V1", the back arrow, Create version and Version graph, then Part Studios | Assemblies with a search (`course_er_insert_linked` 06). |
| ER1.7 | Newest version by default; version graph to pick an older one | ✅ | **P3G.1**: the newest version by default; the `cadrs_ui::VersionGraph` picker for another (`course_er_insert_linked` 07, 13; `course_drw_version_reference` 04–05). |
| ER1.8 | Insertion then works as for the current document (Inserted: n, Undo to remove) | ✅ | **P3G.1**: click to pick up, the part follows the pointer, a click places it, "Inserted: n", Undo to remove (`course_er_insert_linked` 08–09). |
| ER1.9 | Version icon at the right of linked instance rows | ✅ | **P3G.1**: the link icon (stand-in `link`, see `docs/icon-migration.md`) in a right-aligned column at the end of a linked instance's row, its tooltip naming the source and version (`course_er_insert_linked` 10); instances of a part are numbered in one sequence whatever their source (`course_er_versions_in_document` 09). |
| ER1.10 | Deleting the source or losing access doesn't break the consumer | ✅ | **P3G.1**: geometry from the consumer's stored copy (`course_er_insert_linked` 19–20). |

## ER2 Updating linked documents
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER2.1 | Links never auto-update | ✅ | **P3G.1**: version pins; the source's edits and new versions change nothing (`course_er_insert_linked` 12). |
| ER2.2 | Updating needs a new version (toolbar Create version) | ✅ | **P3G.1**: the top bar's Create version button (after the counters, so nothing in the bar moves) and a live versions counter (`course_er_versions_in_document` 02–04). |
| ER2.3 | Blue badge on each stale instance row and on the tab | ✅ | **P3G.2**: a blue disc with the version glyph on each out-of-date row and left of the tab's name (`course_er_update_linked` 02, `ex1-step15.png`); its tooltip "V2 is available. Click to update." (03). |
| ER2.4 | Instance menu "Update linked document…" → Reference manager | ✅ | **P3G.2** (`course_er_update_linked` 04–05, `ex1-step16.png`). |
| ER2.5 | Multi-select then Update linked document… | ✅ | **P3G.2**: the manager takes every selected linked instance (`course_er_update_linked` 04–06: three rows, "Updated 3 references"). |
| ER2.6 | Tab menu "Update linked document…" updates every link in the tab | ✅ | **P3G.2**: shown on tabs holding references (`course_er_update_linked` 11–13; a drawing tab: `course_er_pinning` 14). |
| ER2.7 | Selective update → version graph → older version | ✅ | **P3G.2**: each Selective row's version graph button opens the graph; a version picked is its target (`course_er_reference_manager` 06–10: V2 ⇒ V1, 37 500 mm³ in 11). |

## ER3 Referencing versions within a document
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER3.1 | Same-document references at the workspace or at a version | ✅ | **P3G.1** (`course_er_versions_in_document` 06–12). |
| ER3.2 | Current document tab: version graph picker with legend (Workspace, Version), Main … V1 … Start; Create version beside it | ✅ | **P3G.1**: the Current document header's Version graph button: the Workspace / Version legend, Main, the versions, Start; Create version beside it (`course_er_versions_in_document` 05–07, 13–14). Release candidate and Release markers are out of scope (release management). |
| ER3.3 | Instance menu "Change to version…" → Reference manager (Update to latest / Selective) | ✅ | **P3G.2**: enabled for every instance but standard content; a workspace instance's row reads "Workspace ⇒ V1" (`course_er_change_to_version` 02–04). |
| ER3.4 | After the switch: version icon, frozen, blue badge on newer versions | ✅ | **P3G.2** (`course_er_change_to_version` 04–06: the icon, 37 500 mm³ after the studio went to 40, the badge after V2; `link_update.rs::change_to_version_and_back_to_workspace_round_trips`). |
| ER3.5 | Same choice for parts, subassemblies and drawing views | ✅ | **P3G.1**: parts and subassemblies (the Assemblies tab at a version) in assemblies, and parts in drawing views (`course_drw_version_reference` 04–09; D13.1). |
| ER3.6 | Change to workspace (the reverse, same document) | ✅ | **P3G.2**: Main in a same-document row's version graph (`course_er_change_to_version` 07–10: back to the workspace, 60 000 mm³); the document round-trips byte for byte (`change_to_version_and_back_to_workspace_round_trips`). |

## ER4 Update all references
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER4.1 | Nested chains A → B → C | ✅ | **P3G.1** resolves a chain through the copies; **P3G.2** updates it (`course_er_update_all`; `link_update.rs::update_all_through_a_chain_makes_an_auto_version`). |
| ER4.2 | Transitive indicator (arrow variant) | ✅ | **P3G.2**: `link_update::staleness` (`nested`: a copy further down has a newer version); a blue ring with an arrow on the row, and the tab badge (`course_er_update_all` 02; the chain test flags C when only A changed). |
| ER4.3 | "Update all references to latest versions" toolbar button (right of undo/redo) → Reference manager, all pre-selected | ✅ | **P3G.2**: right of undo and redo in the Part Studio, assembly and drawing toolbars, shown when the document references versions (stand-in icon `arrow-up`; `course_er_reference_manager` 01–02, `course_er_update_all` 03). |
| ER4.4 | Reference manager contents: title, ✕, two tabs, "Newer versions available for N document(s)" group, rows with thumbnail, name, "V1 ⇒ V2" / "⇒ new version", Update all, "?" | ✅ | **P3G.2** (`course_er_update_linked` 05, `course_er_update_all` 04, `course_er_reference_manager` 02–03 (the group collapsed), 09 (the "?")). |
| ER4.5 | Auto versions in intermediate documents | ✅ | **P3G.2**: `link_update::execute_update_all` updates each intermediate document's workspace, saves it and makes its auto version, then points the consumer at it (`course_er_update_all` 05–07: C reads 60 000; the chain test). |
| ER4.6 | Toast with "show more details" | ✅ | **P3G.2**: "Updated n references" with **show more details**, which opens the list of changes and auto versions (`course_er_update_linked` 06–07, `course_er_update_all` 05–06). |
| ER4.7 | Auto-version icon in History and version graph | ✅ | **P3G.2**: `Version::auto` (serde default false), an open square on the rail and in the graph, "Auto version" in its subtitle and the graph's legend (`course_er_update_all` 09–10). |
| ER4.8 | Auto versions can't be undone; warn first | ✅ | **P3G.2**: the manager warns before an Update all that makes versions; undo re-points the consumer, the versions stay (`course_er_update_all` 04, 08–09; the chain test). |

## ER5 Pinning references
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER5.1 | Instance menu "Pin reference"; thumbtack icon | ✅ | **P3G.2**: `link_update::SetPinned`; the thumbtack (stand-in `location`) (`course_er_pinning` 02–03). |
| ER5.2 | Pinned + newer version: icon changes, never blue | ✅ | **P3G.2**: a grey ring round the pin (`course_er_pinning` 04). |
| ER5.3 | Update-all skips pinned | ✅ | **P3G.2** (`course_er_pinning` 05–06; `link_update.rs::update_all_skips_a_pinned_reference`: 60 000 + 37 500). |
| ER5.4 | Update one pinned reference via its icon → Selective update | ✅ | **P3G.2**: a pinned icon opens the manager on Selective update (`course_er_pinning` 07–08). |
| ER5.5 | Selective update with per-row checkboxes for several pinned references | ✅ | **P3G.2** (`course_er_reference_manager` 12, `course_er_pinning` 15). |
| ER5.6 | Drawings pin too (Sheets pane → Pin reference; drawing tab → Update linked document…) | ✅ | **P3G.2**: `ModelSource::pinned`; the Sheets pane's reference row shows the version, the icon and a menu (Update linked document…, Change to version…, Pin / Unpin reference); the drawing tab's Update linked document… (`course_er_pinning` 11–16; `link_update.rs::a_drawing_reference_changes_version_pins_and_updates`). |
| ER5.7 | Unpin reference | ✅ | **P3G.2** (`course_er_pinning` 09–10). |
| ER5.8 | Only version references can be pinned | ✅ | **P3G.2**: refused for a workspace reference with a message; the item is disabled there (`link_update.rs::update_all_skips_a_pinned_reference`). |

## ER6 Exercise: Using Linked Documents (stand-in: `fixtures/linked_piston_standin.cadrs`, `fixtures/linked_hexapod_standin.cadrs`)
| ID | Step | Status | Notes |
|---|---|---|---|
| ER6.1 | Copy the Piston document | ✅ | Documents page Copy… (`landing.rs:2015`; D14.1); **P3G.5** on the stand-in: `course_er_ex1_hexapod` 01–02. |
| ER6.2 | Create version dialog (Name V1, Description, Create / Create and edit properties / Cancel) | ✅ | **P3G.1**: "Create version from Main" with Name (V1), Description (max 10 000 characters), **Create**, **Create version and edit properties** (the version made and the History panel opened on it), **Cancel** (`course_er_versions_in_document` 03–04; on the copied Piston, `course_er_ex1_hexapod` 03). The multi-branch note is dropped (no branches until P3E.4). |
| ER6.3 | Copy the Hexapod document | ✅ | As ER6.1; **P3G.5**: the Project copied and opened on the Hexapod, the two plates only (`course_er_ex1_hexapod` 04). |
| ER6.4 | Insert → Other documents → paste id → document → Assemblies → Piston Assembly | ✅ | **P3G.1**: Insert → Other documents → paste id → document → Assemblies / Part Studios (`course_er_insert_linked` 05–06); **P3G.5**: the Hexapod run, the Piston document searched by name (its copy's id is new) → V1 → Assemblies → Piston Assembly (`course_er_ex1_hexapod` 05–06). |
| ER6.5 | "Piston Assembly <1>" shows the version icon | ✅ | **P3G.1** (`course_er_insert_linked` 10); **P3G.5** (`course_er_ex1_hexapod` 07). |
| ER6.6 | Revolute between a Baseplate hole connector and a UJoint connector | ✅ | **P3G.1**: linked instances mate as local ones (DV2.3 test); **P3G.5**: Revolute 1, the UJoint's bottom-face centre on the first hole's top-edge centre (`samples::piston::revolute_piston`; its dialog "Mate connector of UJoint" / "Mate connector of Baseplate": `course_er_ex1_hexapod` 08; the pick is the harness's shortcut, see "Stand-in substitutions"); `course_linked.rs::the_mated_hexapod_places_the_pistons_where_the_placed_one_does`. |
| ER6.7 | Copy/paste to 6 instances; the full instance menu | ✅ | **P3G.1**: Copy of a multi-selection copies every instance and Paste keeps each link (`course_er_insert_linked` 15). **P3G.2**: Update linked document…, Pin / Unpin reference and Change to version… (`course_er_insert_linked` 15b, `course_er_pinning` 02, 09). **P3G.3**: Open linked document (`course_er_open_linked` 01). **P3G.5**: the Hexapod's menu and Copy, then Ctrl+V five times: six instances (`course_er_ex1_hexapod` 09–10). Revision history and Add comment stay out of scope (release management, collaboration). |
| ER6.8 | Revolute the other five | ✅ | As ER6.6; **P3G.5**: Revolute 2–6 (`course_er_ex1_hexapod` 11: Mate Features (12); `course_linked.rs::hexapod_stand_in_updates_as_the_course_does`). |
| ER6.9 | Fastened Ball Joint Pin ↔ Topplate hole, triad drag first | ✅ | As ER6.6 (Fastened: DV2.3 test); **P3G.5**: Fastened 1–6, each Eye's top-face centre on a Topplate hole's bottom-edge centre (`samples::piston::fasten_topplate`; `course_er_ex1_hexapod` 11). The stand-in has no Ball Joint Pin: the Eye stands in for it. |
| ER6.10 | Linked subassembly's own mates solve inside the top level | ✅ | **P3G.1**: a linked subassembly is an ordinary subassembly of its copy (its own mates solve in the top level as P3B.4; `external_refs.rs::a_chain_of_links_resolves_through_the_copies` places B's block through C's copy). **P3G.5**: the six linked Piston Assembly subassemblies solve with the twelve top-level mates (`course_er_ex1_hexapod` 11, 19). |
| ER6.11 | Open the Pneumatic Piston Part Studio in the Piston document | ✅ | **P3G.5** (`course_er_ex1_hexapod` 13). |
| ER6.12 | Edit Main Sketch Ø15.725 → Ø20 | ✅ | **P3G.5**: the Main Sketch's Ø15.725 dimension double-clicked and set to 20 (`course_er_ex1_hexapod` 13; `samples::piston::set_diameter` in the test). The stand-in's Main Sketch is on Top, the course's on Right. |
| ER6.13 | Edit Rod Sketch 28 → 75 | ✅ | **P3G.5** on the stand-in: the rod's length is Extrude 3's depth, 28 → 75 (`course_er_ex1_hexapod` 14; `samples::piston::set_rod_length`); the Eye, sketched on the rod's top face, follows. |
| ER6.14 | Create version V2 | ✅ | P3D.3 (`course_insp_history_panel` 06–11); **P3G.5** (`course_er_ex1_hexapod` 15). |
| ER6.15 | Six blue badges and the tab badge; geometry unchanged | ✅ | **P3G.2** on the block stand-in: three badges and the tab's (`course_er_update_linked` 02); **P3G.5**: the six Piston Assembly rows and the Hexapod tab (`course_er_ex1_hexapod` 16). |
| ER6.16 | Select all → Update linked document… → Update to latest → "V1 ⇒ V2" → Update all | ✅ | **P3G.2** (`course_er_update_linked` 04–06); **P3G.5** (`course_er_ex1_hexapod` 17–18). |
| ER6.17 | Geometry updates, badges clear; Selective update back to V1 | ✅ | **P3G.2** (`course_er_update_linked` 06, 08; `course_er_reference_manager` 06–11); **P3G.5**: the Hexapod's pistons lengthen and the Topplate rises with them (the mates re-solved on update), then Selective update of the six rows to V1 (`course_er_ex1_hexapod` 19–21). |
| ER6.18 | Mass properties of the selected instances | ✅ | **P3G.1**: mass properties of linked instances (`course_er_insert_linked` 11, 14, 20); **P3G.5** (`course_er_ex1_hexapod` 12, 19, 21). |
| ER6.check | Self-check: total mass of the Hexapod | ✅ | **P3G.5** on the stand-in: V1 589 534.041 mm³, centroid (0, 0, 50.348), 1.592 kg Al (`course_er_ex1_hexapod` 12, 21); V2 640 689.203 mm³, (0, 0, 65.964), 1.730 kg (19); `course_linked.rs::hexapod_stand_in_updates_as_the_course_does` against the closed form (plates π(R² − 6·5²)·10, pistons 10³ + 60π(D/2)² + 9πL + 8³ raised 20, the Topplate at 88 + L). The course's own values (1.180e+6 mm³) are the public document's, which can't be copied. |

## ER7 Move to Document
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER7.1 | Tab menu "Move to document…" | ✅ | **P3G.3**: every tab's menu (Part Studio, assembly, drawing) (`course_er_move_to_document` 02, `course_tips_move_tab` 01). |
| ER7.2 | Dialog New document (name defaults to the tab name) \| Other documents (a target version is created) | ✅ | **P3G.3** (`cadrs_app::move_document`): "Move to document", New document with **Document name** (the first tab's name by default), Other documents with the linked documents' browser (search, locations, folders); a picked document shows its versions and **Version name** ("V2"): the version made with the moved tabs (`course_er_move_to_document` 03, `course_tips_move_tab` 08–10; `move_document.rs::moving_to_an_existing_document_creates_a_version_there`). |
| ER7.3 | "N referenced tabs will be moved", Show/Hide details, per-tab choice; tabs left behind become links the other way | ✅ | **P3G.3**: `move_doc::referenced_tabs` (an assembly's studios and subassemblies, a drawing's sources, recursively); "2 referenced tabs will be moved", Show / Hide details with each tab (the referenced ones with a checkbox); a tab left here is referenced from the new location at an **auto version** of this document (`course_er_move_to_document` 03–04, `course_tips_move_tab` 02–05; `move_document.rs::a_referenced_tab_left_behind_becomes_a_link_back`: 4 instances back at the source's V1, 13 956.271 mm³). |
| ER7.4 | Move / Cancel, "Moving 2 tabs to …" | ✅ | **P3G.3**: the footer's "Moving 2 tabs to Pneumatic Piston", **Move** and **Cancel**; then a toast "Moved 2 tabs to Pneumatic Piston…" with **Open Pneumatic Piston** (`course_er_move_to_document` 05–06). |
| ER7.5 | Uses in the source are rewritten to links at an auto-created version | ✅ | **P3G.3**: `MoveTabs` re-points every use (instances, drawing views) at the target's version, one undo step; undo puts the tabs back, the other document and the versions stay (the toast says so) (`course_er_move_to_document` 06–10; `moving_the_piston_assembly…`: undo restores the document exactly). |
| ER7.6 | A same-document version reference to the moved tab keeps the old version; Reference manager offers "Update to the new document" | ✅ | **P3G.3**: the source notes where each tab went (`Document::moved`, additive); such a reference keeps resolving to the old version, shows the blue badge ("Its tab moved to …"), and the Reference manager's row offers **Update to the new document** (`move_doc::moved_record`, `change_to_new_document`) for all its uses (`course_er_move_to_document` 15–19; `move_document.rs::a_same_document_version_reference_to_a_moved_tab_offers_the_new_document`). After it, the new document's versions drive the badge. |
| ER7.7 | Tab manager multi-select move | ✅ | **P3G.3**: a minimal Tab manager (the tab bar's leftmost icon): every tab with its icon, click opens, Ctrl+click / Shift+click select, **Move N tabs to document…** (button or right-click), and the tab menu's Move to document… takes the Tab manager's selection (`course_tips_move_tab` 06–11). P3E.2 extends it (search, filters, reordering). |

## ER8 Exercise: Moving Elements to Another Document (stand-in: `fixtures/move_to_document_standin.cadrs`)
| ID | Step | Status | Notes |
|---|---|---|---|
| ER8.1 | Copy the document | ✅ | Copy… (D14.1); **P3G.5** on the stand-in (`course_er_ex2_move` 01–03). |
| ER8.2 | Piston Assembly tab → Move to document… | ✅ | **P3G.3** on the stand-in `cadrs_core::samples::piston::move_to_document` (`course_er_move_to_document` 02); **P3G.5** on the copied fixture (`course_er_ex2_move` 04). |
| ER8.3 | New document "Pneumatic Piston", "2 referenced tabs will be moved", Move | ✅ | **P3G.3** (`course_er_move_to_document` 03–05); **P3G.5** (`course_er_ex2_move` 05). |
| ER8.4 | Hexapod rows show the version icon; the two tabs are gone | ✅ | **P3G.3** (`course_er_move_to_document` 06–07); **P3G.5** (`course_er_ex2_move` 06–07). |
| ER8.5 | Documents page → Created by me lists the new document (details panel) | ✅ | **P3G.3**: the new document is the user's, so Created by me lists it, with its thumbnail (`course_er_move_to_document` 11, `course_er_ex2_move` 08, both sorted by name; the test checks `Filter::CreatedByMe`). **P3E.1**: a click selects it and the Details panel shows its owner, description, labels, created by, created and modified, versions and where used (`course_td_documents_details` 01–07). |
| ER8.6 | The new document has Pneumatic Piston and Piston Assembly | ✅ | **P3G.3** (`course_er_move_to_document` 12; the test: tabs ["Pneumatic Piston", "Piston Assembly"]); **P3G.5** (`course_er_ex2_move` 09). |
| ER8.7 | Mass properties of the Piston Assembly instances | ✅ | **P3G.3** on the stand-in: 13 956.271 mm³, centroid (0, 0, 32.263), Al 37.682 g (`course_er_move_to_document` 13; the test checks the closed form); **P3G.5** (`course_er_ex2_move` 10). |
| ER8.check | Self-check: piston assembly mass | ✅ | **P3G.5** on the stand-in: 13 956.271 mm³, centroid (0, 0, 32.263), 37.682 g Al (`course_er_ex2_move` 10); `course_linked.rs::piston_assembly_matches_its_closed_form` (10³ + 60π·7.8625² + 9π·28 + 8³; z̄ (1000·(−5) + 11 652.588·30 + 791.681·74 + 512·92)/V; ×0.0027 g/mm³) and `move_document.rs::moving_the_piston_assembly_takes_two_tabs_and_keeps_every_world_pose`. The course's own values (31 360.866 mm³) are the public document's. |

## ER cross-cutting
| ID | Requirement | Status | Notes |
|---|---|---|---|
| ER X1 | One Reference manager for every entry point | ✅ | **P3G.2**: one `reference_manager` for the icon, the instance and multi-select menus, the tab menu, the Sheets pane and the toolbar (`course_er_reference_manager` 01–02, 05, `course_er_update_linked` 04–05, 11–13, `course_er_pinning` 07–08). P3G.3 adds "Update to the new document" rows to it. |
| ER X2 | Version icon states: plain, blue, blue with arrow, thumbtack (instances, tabs, Feature list, Sheets pane) | ✅ | **P3G.1** plain and red; **P3G.2** blue, blue ring with arrow, thumbtack and pinned-with-update on instance rows, tabs (the update states) and the Sheets pane (`course_er_update_linked` 02, `course_er_update_all` 02, `course_er_pinning` 03–04, 11–13). **P3G.4**: Derived rows in the Feature list show them too (`course_dv_derived_dialog` 10–11, 15). |
| ER X3 | One version-graph picker with Create version, in Insert, Reference manager and Move to document | ✅ | **P3G.1** in both Insert tabs and the drawing Insert view; **P3G.2** in the Reference manager (`course_er_reference_manager` 07); **P3G.3**: Move to document always creates the target's version (the course's dialog: "a version of the target is created"), so it offers the Version name, not a graph (`course_tips_move_tab` 10). |
| ER X4 | `pinned` flag; transitive stale query; update with auto versions | ✅ | **P3G.1** the flag; **P3G.2** `link_update::{staleness, plan_update_all, execute_update_all}` (the tests in `cadrs_core/tests/link_update.rs`, e.g. `link_update.rs::update_all_through_a_chain_makes_an_auto_version`). |
| ER X5 | Copy/paste keeps the instance's reference | ✅ | **P3G.1** (`course_er_insert_linked` 15). |
| ER X6 | Mass properties over a multi-selection including linked subassemblies | ✅ | **P3G.1** (`course_er_insert_linked` 20: the whole assembly, 195 000 mm³; `external_refs.rs`: 37 500 + 60 000). |
| ER X7 | Copy workspace = local Copy… | ✅ | `landing.rs:2015` (D14.1); `course_er_ex1_hexapod` 01–02, `course_er_ex2_move` 01–02. |
| ER X8 | Local equivalents of cloud browsing: folders, created by me / recent, paste id | ✅ | **P3G.1**: folders, Created by me, Recently opened and a pasted id (`course_er_insert_linked` 03–05). Teams, sharing and web URLs are out of scope. |

## Cross-cutting design (decisions for 3G)
- **One reference type** in a new bevy-free module `cadrs_core::external`:
  `SourceRef { document: Option<DocumentId> /* None = this document */, at: RefAt::{Workspace,
  Version(VersionId)}, element: ElementId, pinned: bool }`. Cross-document refs are always
  `Version`. `InstanceSource` and drawing `ObjectRef`/`ModelSource` gain an optional `link:
  Option<SourceRef>` (additive, `#[serde(default)]`, schema v5 with a trivial migration);
  `FeatureKind::Derived` holds one.
- **Frozen snapshot in the consumer.** Every version link stores the referenced element *and the
  elements it depends on* (a subassembly's studios, recursively, with their own links) as a
  content-addressed snapshot in the consumer document (`Document.linked: BTreeMap<SnapshotKey,
  LinkedElement>`, deduplicated by hash, the same idea as `ModelSource.snapshot`). This makes
  ER1.10 and DV1.7 work (source trashed or purged, geometry stays), keeps opening a document
  independent of other documents' history files, and gives the rebuild cache a stable key.
  The resolver only reads the source's `history.ron` to **create or update** a link and to check
  staleness (newest version id, cached per session).
- **Identity of linked geometry.** Linked elements are built in the consumer's rebuild under a
  namespaced element id (`SnapshotKey`), so a copy of the source document or a duplicate tab can't
  collide with local `ElementId`s or `PartId`s. Names inside stay the source's names (so mates
  re-resolve after an update, ex-dv4).
- **Cycles.** A link is refused when its source's snapshot graph reaches the consumer's own
  `(document, element)` at *any* version, which is stricter than a version-level graph but matches
  DV1.5's "A derives from B and B derives from A" and is easy to explain.
- **Updates are commands.** Update, Change to version/workspace, Pin/Unpin are undoable commands
  (`Scope::Whole` when several tabs change). Auto-created versions (ER4.5) and versions created by
  Move to document are written to the other document's history log and **not** undone (ER4.8);
  the Reference manager warns before it creates any.
- **Copy… keeps no versions** (as Onshape's Copy workspace); the exercises create V1 after copying,
  as the course does.

## What each exercise needs
Both courses' starting documents are Onshape public documents, so every exercise uses a stand-in
fixture built by a `cadrs_core::samples` module (fixed document and element uuids so links between
fixtures resolve, regenerated with `CADRS_REGENERATE_FIXTURES=1` and checked by a
`*_fixture_is_current` test). Tests load the fixtures into a temporary library store. Mass
properties are at **unit density** (the kernel's convention, mm³ and mm⁵) unless a material is
named; "Al" is Aluminium 6061, 2.70 g/cm³.

- **ex-dv1 → `course_dv_ex1_two_copies`**, fixture `linked_block_standin.cadrs` (document A,
  "Block source"): Part Studio "Block", a 50×30×25 box with its corner at the origin
  (x 0..50, y 0..30, z 0..25), versions V1. Document B ("Block derived") has a Part Studio with a
  mate connector at (0, 0, 100) and a Derived feature from A@V1 with Locations = {origin,
  connector}.
  - Per block: V = 37 500 mm³, area = 2(1500 + 1250 + 750) = 7 000 mm², centroid (25, 15, 12.5).
  - Two copies: V = **75 000**, area **14 000**, centroid **(25, 15, 62.5)** (source 12.5 + 50).
  - Inertia about the combined centroid, unit density: each block about its own centroid has
    Ixx = V(30² + 25²)/12 = 4 765 625, Iyy = V(50² + 25²)/12 = 9 765 625,
    Izz = V(50² + 30²)/12 = 10 625 000; the copies sit ±50 in z, so
    **Ixx = 2·4 765 625 + 2·37 500·50² = 197 031 250**, **Iyy = 207 031 250**,
    **Izz = 21 250 000** mm⁵, products 0.
- **ex-dv2 → `course_dv_ex2_update`** (continues ex-dv1): in A, change the extrude depth 25 → 40
  and create V2. B still reads 75 000 and shows the badge; after Update: per block 60 000, area
  2(1500 + 2000 + 1200) = 9 400; two copies **120 000 mm³**, **18 800 mm²**, centroid
  **(25, 15, 70)** (copies at z 20 and 120). Undo returns to 75 000.
- **ex-dv3 → `course_dv_ex3_workspace`**: in document A, a new Part Studio ("Part Studio 1")
  derives "Block" at the **workspace**; changing the block's length 50 → 60 gives **45 000 mm³**
  in Part Studio 1 on the next rebuild, with no badge and no Update; undo gives 37 500 there.
- **ex-dv4 → `course_dv_ex4_assembly_update`**, fixture `linked_base_standin.cadrs` (document C):
  Part Studio "Base", a 100×100×10 plate (x, y 0..100, z 0..10); an assembly with Base fixed and
  "Block" from A@V1 inserted and **Fastened** bottom-face centroid (25, 15, 0) to the plate's
  top-face centroid (50, 50, 10), so the block spans x 25..75, y 35..65, z 10..35.
  - V1: V = 137 500, centroid **(50, 50, 9.7727)** (= (100 000·5 + 37 500·22.5)/137 500).
  - Update to V2 (height 40): V = **160 000**, centroid **(50, 50, 14.375)**
    (= (500 000 + 60 000·30)/160 000). The mate still resolves to the same bottom face by its
    exact name (`Match::Exact`), with no mate error.
- **ex-dv5 → `course_dv_ex5_circular`**: B's Part Studio 1 derives A's Block at V1 (ex-dv1) and B
  is versioned; A's Block then tries to derive B's Part Studio 1 at that version: refused with
  "Circular reference: Block source › Block → Block derived › Part Studio 1 → Block source" and
  nothing is added. (The plan's first wording had A's Part Studio 2 as the consumer; the cycle
  check is per element, so the exercise derives into the Block itself, which B's copy holds.)
- **ER Ex1 (Hexapod) → `course_er_ex1_hexapod`**, fixtures `linked_piston_standin.cadrs` and
  `linked_hexapod_standin.cadrs`. The substitution is documented in the sample module.
  - **Piston document** ("Linked Document- Piston (stand-in)"), Part Studio "Pneumatic Piston",
    axis Z, all parts centred on it: **UJoint** 10×10×10 cube (z −10..0); **Cylinder** from
    **Main Sketch** (Top plane, circle Ø15.725) extruded 60 (z 0..60); **Rod** from **Rod
    Sketch** (a Ø6 circle) extruded from z 60 for L = **28** by **Extrude 3** (its length is
    that extrude's depth; the course revolves a 3 × L rectangle on Front about Z, which gives
    the same solid); **Eye** an
    8×8 square sketched on the Rod's top face, extruded 8. Assembly "Piston Assembly": the four
    parts placed on the axis, UJoint fixed (the stand-in places them instead of the course's
    Revolute/Ball/Cylindrical mates, so the mass properties are deterministic).
    V_piston = 1 000 + 60π(D/2)² + 9πL + 512.
  - **Hexapod document**: "Baseplate" Ø200 × 10 disc (z 0..10) and "Topplate" Ø160 × 10 disc, each
    with six Ø10 holes on a Ø140 circle at 0°, 60°, …; volumes 98 500π = 309 446.876 and
    62 500π = 196 349.541. "Hexapod" assembly: Baseplate fixed; six Piston Assembly instances
    (V1), each **Revolute** UJoint bottom-face centre ↔ baseplate hole top-edge centre (the
    UJoint spans z 10..20), each **Fastened** Eye top-face centre ↔ Topplate hole bottom-edge
    centre, so the Topplate sits at z 88 + L .. 98 + L.
  - **V1** (D 15.725, L 28): V_piston = **13 956.271**; the 8 instances:
    **V = 589 534.041 mm³**, centroid **(0, 0, 50.348)** (every part is centred on its piston axis
    and the pistons are 6-fold symmetric, so x = y = 0 for any revolute angle); Al mass
    **1 591.742 g**.
  - **V2** (Ø20, L 75, after Update all): V_piston = **22 482.131**; **V = 640 689.203 mm³**,
    centroid **(0, 0, 65.964)**, Al mass **1 729.861 g**. Selective update back to V1 restores the
    V1 numbers exactly.
  - Unit test `hexapod_stand_in_updates_as_the_course_does` checks both states to 1e−3 against
    the closed form (independently computed in the test from the formulas above).
- **ER Ex2 (Move to document) → `course_er_ex2_move`**, fixture
  `move_to_document_standin.cadrs`: one document with tabs Hexapod, Pneumatic Piston, Piston
  Assembly, Topplate, Baseplate (the ER Ex1 geometry at V1, all same-document workspace refs).
  - Move Piston Assembly → New document "Pneumatic Piston": the dialog says "2 referenced tabs will
    be moved"; afterwards the source has Hexapod, Topplate, Baseplate; the six instances link to
    the new document's auto-created V1; the new document is in Created by me.
  - Checks: every instance's world pose is identical before and after (1e−9); the Hexapod still
    reads **V = 589 534.041**, centroid **(0, 0, 50.348)**; in the new document the Piston
    Assembly's four instances read **V = 13 956.271 mm³**, centroid **(0, 0, 32.263)**
    (= (1 000·(−5) + 11 652.588·30 + 791.681·74 + 512·92)/13 956.271), Al mass **37.682 g**.

## Stand-in substitutions (P3G.5)
The courses' starting documents are Onshape public documents, so every exercise runs on a
stand-in built by `cadrs_core::samples` through the command layer (fixed ids), shipped in
`fixtures/` and checked by a `*_fixture_is_current` test (`cadrs_core/tests/course_linked.rs`;
regenerate with `CADRS_REGENERATE_FIXTURES=1`). Scenarios load them with `linked-fixture store` /
`linked-fixture open` (`cadrs_app::linked_exercises`).
- **Block source** (`linked_block_standin.cadrs`, with `.history.ron` holding V1): the
  50 × 30 × 25 block (Aluminum 6061) with Sketch 1 fully defined (the 50 and 30 dimensions, for
  ex-dv3's length edit) and "Mate connector 1" on its top face. The derived-and-linking page asks
  for "a Part Studio with a 50×30×25 block"; the Control Arm isn't used.
- **Block derived** (`linked_block_host_standin.cadrs`): "Part Studio 1" with "Mate connector 1"
  at (0, 0, 100). ex-dv2 and ex-dv5 start from ex-dv1's Derived 1, made by `linked-ex dv1`.
- **Block assembly** (`linked_base_standin.cadrs`): "Base", 100 × 100 × 10, and "Assembly 1" with
  Base <1> fixed. ex-dv4's Fastened 1 is made by `linked-ex dv4-mate` (the block's bottom face is
  hidden from every standard view, so the harness can't pick it) and shown in its dialog; the
  lost-face version V3 (`linked-ex dv4-redraw`) redraws Sketch 1, which renames every face (a
  stand-in for a remodelled source).
- **Linked Document- Piston** (`linked_piston_standin.cadrs`): the course's 8-part piston with 76
  features is replaced by four aluminium parts on one axis: UJoint (10 mm cube), Cylinder (Main
  Sketch Ø15.725 on **Top**, the course's on Right, extruded 60), Rod (Ø6, **its length is
  Extrude 3's depth**, the course's a Rod Sketch dimension on Front revolved), Eye (an 8 mm
  square **sketched on the rod's top face**, so it follows the rod; it stands in for the Ball
  Joint Pin). Piston Assembly places the four parts (UJoint fixed) instead of the course's 8
  mates, so its mass properties are fixed.
- **Linked Documents- Project** (`linked_hexapod_standin.cadrs`): Baseplate Ø200 and Topplate
  Ø160 × 10 with six Ø10 holes on a Ø140 circle; the Hexapod holds Baseplate <1> (fixed) and
  Topplate <1>. After the UI's first insert and copy/paste, Revolute 1 (`linked-ex
  hexapod-revolute`) and Revolute 2–6 with Fastened 1–6 (`linked-ex hexapod-mates`) are made by
  the scenario's shortcut rather than by picking 24 connectors in the view; each is a real mate
  on implicit connectors (the UJoint's bottom-face centre on a hole's top-edge centre; an Eye's
  top-face centre on a Topplate hole's bottom-edge centre), solved like the dialog's ✓. 12 mates
  instead of the course's 13.
- **Exercise: Move to Document** (`move_to_document_standin.cadrs`): the same parts in one
  document (Hexapod, Pneumatic Piston, Piston Assembly, Topplate, Baseplate), the pistons placed
  (no mates).
- Opening another document in a scenario uses the documents page's Open by name
  (`move-doc open <name>`); a copy's id is new, so the Insert dialog finds it by name, not by a
  pasted id.

## Cross-stage items pending
Rows that waited on another stage (3G runs before 3E). **Both closed by P3E.1 (2026-09-29,
pre-judge)**: ER1.2 (the browser lists each label as a location, `course_td_documents_labels`
17–18) and ER8.5 (the click-selects model and the Details panel, `course_td_documents_details`
01–07). The notes as they were:
- **ER1.2** (was 🟡 carried to P3E.1): the **Labels** location of the Other documents browser. cadrs
  has no labels model yet (a label list per user, assigning labels from the documents page and
  its menu, the sidebar's Labels section, then the browser's location); P3E.1 adds labels to the
  documents page and the browser shows them then. Shared with me and teams are out of scope
  (sharing). Checked at the 3G cleanup: not small (a model, its commands and three UI places).
- **ER8.5** (was 🟡 carried to P3E.1): the documents page's **details panel** for the new document
  (owner, description, labels, created/modified). Created by me lists it now. Checked at the 3G
  cleanup: not small either, because a single click on a documents-page row opens the document
  (a row can only be marked by right-click), so the panel needs P3E.1's selection model
  (click selects, double-click opens) as well as description and labels in the metadata.
- Not rows here, noted for P3E.2: the full Tab manager (search, filters, reordering) extends
  P3G.3's minimal one (ER7.7 is ✅ on the minimal one).

## Proposed milestones (stage 3G)
Order: the shared reference model and the Other documents browser first (everything else hangs
off them), then updating and pinning, then the whole-document operations, then Derived, then the
exercises. Every milestone ships scenarios and a fresh-judge round (≥ 8.5, or ≥ 8.3 with only
minor deltas) against `external-references/` and the requirement text.

| # | Milestone | Covers | Done when |
|---|---|---|---|
| P3G.1 | **External references, version-pinned links, Other documents in Insert** (built 2026-09-29, pre-judge) | DV1.1, DV1.2, DV1.3 (plain icon), DV1.5 (insert), DV1.7, DV1.8, DV1.11, DV1.12, DV2.1–DV2.3, DV X1, DV X2, ER1.*, ER2.1, ER2.2, ER3.1, ER3.2, ER3.5, ER4.1 (resolve), ER6.2, ER6.4–ER6.10, ER6.18, ER X2 (plain), ER X3, ER X4 (flag), ER X5, ER X6, ER X8; also A2.3, D13.1, X14 (drawings, version references), T3.1, T3.3 (store), T7.3, TD8.1, P1.3 | `cadrs_core::external` (`SourceRef`, snapshots, resolver, cycle check, link states) with schema v5; the `cadrs_ui` **DocumentBrowser** (locations, search, paste id, rows with thumbnail and version subtitle) and **VersionGraph** picker (Workspace / Version legend, Main … Start, inline Create version) used by assembly Insert (Other documents and the Current document version icon) and the drawing Insert view; version icon on linked rows; top-bar Create version icon and live counter; folder membership. Unit tests: an instance of A@V1 keeps V = 37 500 after A's workspace edit and a new V2 (geometry hash unchanged); a same-document version reference stays 37 500 while a workspace one reads 60 000; trashing A shows "Cannot open a document in the trash. Restore the document from Trash." and purging it "Resource does not exist", both still 37 500; a circular insert is refused; a drawing view of V1 is unchanged by a workspace edit; resolving one version twice reads `history.ron` once; a v4 document loads. Scenarios `course_er_insert_linked`, `course_er_versions_in_document`, `course_drw_version_reference`. |
| P3G.2 | **Update badges, Reference manager, Pin/Unpin, Update all** (built 2026-09-29, pre-judge) | DV1.3 (blue), DV1.4, DV1.6, DV1.10, DV1.11, DV2.2, ER2.3–ER2.7, ER3.3, ER3.4, ER3.6, ER4.*, ER5.*, ER6.7 (menu items), ER6.15–ER6.17, ER X1, ER X2, ER X4; also D2.10 (Change to version), X15 (assemblies, Change to version…), T3.2, T3.3 (update), T7.5 (pins), TD3.8 (where used) | Icon states plain / blue / blue-arrow / thumbtack on instance rows, Derived-ready feature rows, the Sheets pane and tabs; the Reference manager (Update to latest \| Selective update, grouped rows "V1 ⇒ V2" / "⇒ new version", per-row checkboxes, version graph, Update all, "?"), reached from the icon, instance and multi-select menus (Update linked document…, Change to version…, Pin/Unpin reference), the tab menu and the Update-all toolbar button; auto versions (`Version.auto`, marked in History); toast; library-wide Where used. Unit tests: Update to latest turns 37 500 into 60 000 and undo restores it; Selective update to V1 from V3; Update all skips a pinned reference; a chain A → B → C updated from C creates B's auto version, C reads the new volume, undo re-points C but B's version stays; the transitive query flags C when only A changed; Change to version and back to workspace round-trips the document hash. Scenarios `course_er_update_linked`, `course_er_reference_manager`, `course_er_update_all`, `course_er_pinning` (with a drawing), `course_er_change_to_version`. |
| P3G.3 | **Open linked document, Move to document** (built 2026-09-29, pre-judge) | DV1.9, DV4.1, ER6.7 (Open linked document), ER7.*, ER8.2–ER8.6; also D2.10 and X1 (drawings, Move to document), A21.13, T1.3, X1 (essential tips, move) | A read-only document session at a version for every tab kind ("Viewing V1"), opened from the instance and feature menus with the source tab active and the part selected; the Move to document dialog (New document \| Other documents, referenced tabs with Show details and per-tab choice, "Moving n tabs to …"), one multi-document transaction (target written first, then the source rewritten as one undo step); "Update to the new document" in the Reference manager (ER7.6); a minimal Tab manager with multi-select. Unit tests: moving the Piston Assembly to a new document takes 2 tabs, keeps every world pose (1e−9) and the Hexapod's volume and centroid; a referenced tab left behind becomes a link from the new document back; moving to an existing document creates a version there; a same-document version reference to a moved tab still resolves and offers the new document; the opened linked document refuses edits. Scenarios `course_er_open_linked`, `course_er_move_to_document`, `course_tips_move_tab`. |
| P3G.4 | **Derived feature** (built 2026-09-29, pre-judge) | DV1.5 (derive), DV3.1–DV3.8, ER1.1 (Derived dialog) | `FeatureKind::Derived { source: SourceRef, entities, locations, placement: BaseOrigin \| BaseConnector, include_connectors, include_properties }`; the source studio built reentrantly in the host rebuild (cache key includes the source snapshot's chain key); copies namespaced like pattern instances (a `FaceOrigin` variant, recorded in KERNEL.md with a conformance case); expandable Feature-list row with children, eye toggles, linked icon and badge; the dialog with Current \| Other documents, version picker, entity list, Locations, Placement, the two checkboxes. Unit tests: ex-dv1's 75 000 / 14 000 / centroid z 62.5 / Ixx 197 031 250; Base mate connector placement (source connector at the top-face centre (25, 15, 25) onto a location at (0, 0, 100) gives the copy's centroid (0, 0, 87.5)); a Ø10 through-hole cut into a derived block gives 37 500 − 25π·25 = 35 536.505; a derived 20×10 sketch rectangle extruded 5 gives 1 000; Include properties off copies only name, material, appearance; deriving one studio twice and a derive cycle are refused; a fillet on a derived edge of a duplicated studio survives a source edit. Scenarios `course_dv_derived_dialog`, `course_dv_derived_options`. |
| P3G.5 | **Exercises and stage wrap-up** (built 2026-09-29, pre-judge) | ex-dv1–ex-dv5, ER6 (whole flow, ER6.check), ER8 (whole flow, ER8.7, ER8.check) | The four stand-in fixtures with `*_fixture_is_current` tests; scenarios `course_dv_ex1_two_copies`, `course_dv_ex2_update`, `course_dv_ex3_workspace`, `course_dv_ex4_assembly_update`, `course_dv_ex5_circular`, `course_er_ex1_hexapod`, `course_er_ex2_move`; unit tests with the values in "What each exercise needs" (hexapod V1 589 534.041 / z 50.348 and V2 640 689.203 / z 65.964; piston 13 956.271 / z 32.263; the dv values); the gap list re-audited (every row ✅ or out of scope, each ✅ citing a scenario frame or test) and the other gap files' P3G rows ticked. |

## Risks
- **Rebuild reentrancy.** `Rebuilder::placed_body` resets `trail`/`last`, and `rebuild()` sees
  only `&[Feature]`. Derived needs a sub-build that saves and restores that state, or a separate
  child `Rebuilder` sharing the kernel session; bodies can't move between OCCT sessions (no BRep
  read/write). Decide early in P3G.4; if a second session is needed, add `write_brep`/`read_brep`
  to the fork (a conformance case and a KERNEL.md entry).
- **Cache staleness.** `chain_key` hashes only the host feature list. Linked instances and Derived
  must mix in the snapshot key, or an update rebuilds nothing.
- **Name collisions.** Copies and duplicates keep feature uuids, so linked and derived geometry
  must be namespaced; getting this wrong makes mates and fillets jump to the wrong copy.
- **Mates silently keep their frame** when their entity is lost (`assembly/connector.rs:410`).
  ex-dv4 needs "mates survive" to mean *resolved*, so P3G.2 must surface a lost mate as an error.
  **Done in P3G.5**: `assembly::lost_mates` (the row red with "Missing reference: …"), the lost
  mate left out of the solve, and an update re-solves the mates (`course_dv_ex4_assembly_update`
  08–10, `course_linked.rs::ex_dv4_mates_survive_an_update_and_a_lost_face_is_an_error`).
- **Snapshot size.** A snapshot per linked element (recursive for subassemblies) can bloat
  documents; dedupe by content hash and store feature lists, not geometry.
- **Multi-document transactions.** Move to document and auto versions touch two documents and
  the library. Write the target first, make the source rewrite one undo step, never leave a link
  to an unversioned workspace, and test a failure in the middle.
- **Undo that can't undo.** Auto versions and move-created versions stay after Undo (ER4.8); the
  UI must say so, and history tests must cover it.
- **Read-only sessions.** The only read-only view is Part Studio-only inside the Repair panel;
  Open linked document needs every tool to respect a read-only document.
- **Overlap with worker A.** Landing (labels, details), the tab bar (Tab manager) and
  `cadrs_core/src/document.rs` are shared; keep 3G's changes additive and in new modules
  (`cadrs_core::external`, `cadrs_app::linked`, `cadrs_ui::{DocumentBrowser, VersionGraph}`).
- **Icons.** The version-link states, thumbtack and auto-version marker are probably missing from
  icon-rs; add them the icon-rs way at the start of P3G.1–P3G.2.
