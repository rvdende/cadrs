# Managed In-Context Design: gap analysis (2026-10-02; MIC.1–MIC.4 built the same day)

This maps [managed-in-context-design.md](managed-in-context-design.md) (`MC*`) against cadrs on
`main` at `2dc5ac1`. ✅ done · 🟡 partial · ❌ missing · **out of scope** (as in the other gap
files: cloud collaboration, permissions, release management, configurations). Milestones are
**MIC.1–MIC.5** below.

**What exists** (P3B.9 Edit in context, P3H.5 managed in-context for the PCB course):
- `cadrs_core::assembly::context`: one `StudioContext` per Part Studio (`Element::context`): the
  assembly, the instance it was opened from, the parts around it (`ContextPart`: source element,
  part, pose in studio coordinates, name), an eye (`hidden`) and a fingerprint of the source
  studios' features (`sources`). Context parts are views of the source studios' **current**
  rebuild (`context::parts`, `context::solids`): only the poses are frozen. Their feature ids are
  `context_id(occurrence)` (magic top bits, `is_context`). Sketch planes on context faces and Use
  links are baked into the sketch by `parts::regenerate_with` (run by `refresh_studio`), so the
  studio's own rebuild never needs the context.
- `snapshot` takes every occurrence that isn't from the studio, **hidden ones included**.
- `managed_context`: Create Part Studio in context (assembly **Origin** only), Insert and go to
  Assembly, `resnapshot` (Update context), `origin_pose`/`origin_name`.
- App (`assembly/in_context.rs`, `assembly/managed_context.rs`): the context bar ("In context of
  Assembly 1 · Cover <1>", Assembly changed, Update context, eye, Insert and go to Assembly, Back
  to assembly, ✕ remove), ghost tinted at a fixed alpha 0.45, the instance menu's Edit in context
  (disabled on linked instances) and Update context ▸ <studio>, the Display states ▾ menu's Create
  Part Studio in context with the Origin dialog.
- Scenario `course_asm_edit_in_context`, tests `assembly_p3b9.rs`, `phone_case.rs`.

| ID | Requirement | Status | Notes / milestone |
|---|---|---|---|
| MC1.1 | Parts edited in context | ✅ | X15 |
| MC1.2 | Snapshot of position, orientation, **geometry**, hide/show | ✅ | MIC.1: `StudioContext::studios` freezes the source studios' features (and each part's appearance); hidden and suppressed instances are left out. `a_context_freezes_the_geometry_until_updated`, `hidden_instances_are_not_in_the_context`. |
| MC1.3 | Sketch on faces, Use edges, **end types** | ✅ | Extrude and Revolve up to a face, part or vertex of a context: the feature carries the part frozen (`ContextTarget`, kept current by `refresh_studio`), the rebuild places it in a reference state (`rebuild/kernel_ops/context.rs`) and releases it. `an_extrude_goes_up_to_a_context_face_and_follows_an_update` (90π → 135π mm³). |
| MC1.4 | Modelling stays in the Part Studio | ✅ | |
| MC1.5 | Same document or linked documents | ✅ | MIC.4. |
| MC1.6 | Stability: nothing changes until updated | ✅ | Frozen geometry; `status` reports Out of date. |
| MC1.7 | Several contexts in one Part Studio | ✅ | `Element::contexts` (old documents' single `context` reads as context 0, same part ids); per-context ids `context_id(context, occurrence)`; a sketch on a context face imprints that context only. `several_contexts_have_their_own_ids_names_and_features`, `a_sketch_on_a_context_face_imprints_only_that_context`; `course_mic_contexts` 09–11. |
| MC2.1 | Edit in context, Create Part Studio in context | ✅ | |
| MC2.2 | Ghost with hide/show state | ✅ | |
| MC2.3 | Transparency slider | ✅ | The bar's second row (`context-opacity`), with Select transparent geometry (`PartCache::unpickable`). `course_mic_contexts` 12. |
| MC2.4 | Pick ghost faces, edges, vertices | ✅ | |
| MC2.5 | Context created on first reference | ✅ | A pending context (`context::set_pending`, per document and studio) is seen by the regeneration and committed by `ActiveDocument::execute` in the undo step of the first command that references it; leaving or Done drops it. `a_pending_context_is_committed_by_its_first_reference`; `course_mic_contexts` 01–03. |
| MC2.6 | In-context arrow on features | ✅ | `context::feature_contexts` (plane, Use links, up-to ends, Transform copies; not imprints). Stand-in arrow until icon-rs releases `in-context`. |
| MC2.7 | Go to assembly | ✅ | The bar's Go to assembly ▾. |
| MC2.8 | Arrow in the Instance list | ✅ | MIC.3. |
| MC2.9 | Origin or **mate connector** | ✅ | The Origin dialog takes a click: an explicit connector, the Origin, or the implicit connector of a face, edge or vertex; `StudioContext::origin`. `a_studio_made_at_a_mate_connector_takes_its_frame`. |
| MC2.10 | New Part Studio tab with the ghost | 🟡 | The studio's context is created with the tab (not on its first reference): Insert and go to assembly places the parts by it. |
| MC2.11 | Insert and go to assembly | ✅ | The first inserted part becomes the primary instance. |
| MC2.12 | Context drop-down, edit outside of context | ✅ | The Feature list's Assembly contexts row. `course_mic_contexts` 10, 14. |
| MC2.13 | Rename, Update, Delete | ✅ | ⋯ menu; `RenameContext`, `UpdateContext`, `RemoveContext`. `course_mic_contexts` 04, 13, 15. |
| MC2.14 | Edit in context ▸ New context | ✅ | `course_mic_contexts` 08–09. |
| MC2.15 | Yellow arrows for the active context | ✅ | Grey for the others. `course_mic_contexts` 09, 11. |
| MC2.16 | Contexts at named positions | ✅ | A context takes the assembly as it is (named positions, P3B.8). |
| MC3.1 | Solid / dashed arrows | ✅ | Solid on the primary, faint (dashed icon when released) on secondaries. `course_mic_ex2_slide` 03. |
| MC3.2 | Primary instance anchors the ghost | ✅ | |
| MC3.3 | Set as primary instance | ✅ | `SetPrimaryInstance` (all the studio's contexts in the assembly), instance menu. `course_mic_ex2_slide` 06–07. |
| MC3.4 | No update without a primary instance | ✅ | `ContextStatus::NoPrimary`: the bar says so, Update context is disabled. `the_primary_instance_anchors_and_is_needed_to_update`; `course_mic_ex2_slide` 04–05. |
| MC4.1 | Edit in context ▸ <context> | ✅ | |
| MC4.2 | Pick the context in the drop-down | ✅ | |
| MC4.3 | Moves don't update or break | ✅ | |
| MC4.4 | Blue indicator | ✅ | A dot on feature and instance arrows, in the drop-down and the Update context submenu. |
| MC4.5 | Update takes adds, deletes, moves, hide/show, geometry | ✅ | |
| MC4.6 | Update from the assembly ▸ <context> | ✅ | |
| MC4.7 | Update from the drop-down's overflow | ✅ | Also the bar's Update context. |
| MC4.8 | Contexts belong to their studio | ✅ | |
| MC5.1 | Edit in context of a linked part opens its document | ✅ | `assembly/linked_context.rs`: the source document's workspace, editable, with the snapshot taken in the assembly's document (`StudioContext::document`). `course_mic_ex3_gripper` 02. |
| MC5.2 | Ghost from the assembly's document | ✅ | |
| MC5.3 | First reference; arrows | ✅ | |
| MC5.4 | Go to assembly without committing | ✅ | |
| MC5.5 | Create version and go to assembly | ✅ | The primary instance is pointed at the new version (`UpdateReferences`). `course_mic_ex3_gripper` 04–05. |
| MC5.6 | Create Part Studio in context in the assembly's document | ✅ | Moving it out: Move to document (DV4). |
| MC5.7 | Drop-down, overflow, New context | ✅ | |
| MC5.8.1 | Update from the assembly: version, primary instance updated | ✅ | `context::update_linked_context` (writes the other document's workspace, logs it, auto version "Created by Update context in …"). `a_linked_parts_context_is_updated_from_the_assembly_with_a_new_version`; `course_mic_ex3_gripper` 06–08. |
| MC5.8.2 | Update from the Part Studio: no version | ✅ | `resnapshot_external` against the assembly's document from the store. |
| MCX1 | Suspension | 🟡 | `course_mic_contexts` covers its steps (edit in context, new part in context is `course_pcb_*`, update after a change) on the Edit in context stand-in. |
| MCX2 | Slide | ✅ | `course_mic_ex2_slide` on `fixtures/mic_slide_standin.cadrs`. |
| MCX3 | Gripper | ✅ | `course_mic_ex3_gripper` on `fixtures/mic_gripper_*`. Making the new part's tab a linked document is Move to document (DV4), not repeated here. |
| MCC1 | Bar: "Context N of <assembly>", Done, checkboxes, slider | 🟡 | All but Hide/show instances ▾ and Show internal geometry. |
| MCC2 | Feature list header with the drop-down | ✅ | |

**Icons.** `in-context` and `in-context-secondary` are added to icon-rs (`tools/icons/ui_a.py`) but not
released: until cadrs builds against a release with them, the arrows use icon-rs's `arrow-up`
turned left (`cadrs_ui::icon::has_icon` picks the real ones when they're there).

## Design decisions
- **A context freezes features, not bodies** (as linked copies do, P3G.1): each context holds,
  per source Part Studio, that studio's active features as they were at the snapshot, and each
  context part its pose. Context solids are rebuilt from those frozen features (the rebuild cache
  keys them), so editing a source studio changes nothing until Update context, and a context
  needs nothing outside the Part Studio to show (MC5's other-document assemblies included).
- **Ids are per context**: a context part's feature id is `context_id(context, occurrence)`, so
  the same instance in two contexts gives two parts, and references name the context they use.
- **Which features reference which context** is derived from the features (sketch plane, links,
  extrude up-to and every other face, edge or part reference whose feature id is a context id),
  never stored: arrows and "who uses this context" are queries.
- **Up to a context face** (MC1.3): the rebuild gets the context's parts as **reference bodies**
  (in the state, not output parts) from a feature-list prefix the rebuild builds when a Part
  Studio has contexts (its key: the frozen features and poses), so Extrude/Revolve up to a face,
  part or vertex of the context resolves like the studio's own.
- **First reference** (MC2.5): Edit in context adds the context as a step of its own (so sketches
  regenerate against it), and leaving the studio with nothing referencing a context made in that
  visit drops it again (the step is discarded, or removed and merged when other edits followed).
- **Active context and transparency** are view state (the app's, per Part Studio), not undoable.

## Milestones
| ID | Contents |
|---|---|
| MIC.1 | Core: `Element::contexts` (several, named, id, primary instance) with serde from the old `context`; frozen source features; hidden and suppressed instances left out; per-context ids; references query; staleness (out of date, primary missing); commands Add / Rename / Delete / Update / Set primary; Insert from studio sets the primary; rebuild reference bodies for up-to ends. Unit tests. |
| MIC.2 | Part Studio: Feature list context drop-down (contexts, Edit outside of context) with ⋯ Rename / Update / Delete; arrows (yellow for the active context, blue dot when out of date); the bar "Context N of <assembly>" with transparency slider, Select transparent geometry, Go to assembly ▾ (Go to assembly, Insert and go to assembly), Done; context created on first reference. |
| MIC.3 | Assembly: Instance list arrows (solid primary, dashed secondary, blue dot); Edit in context ▸ contexts / New context; Update context ▸ contexts; Set as primary instance; Create Part Studio in context from a mate connector. |
| MIC.4 | Linked documents: Edit in context on a linked instance opens the source document with the snapshot; Go to assembly without committing; Create version and go to assembly; Update context from the assembly writes the other document, versions it and updates the primary instance; Update from the Part Studio reads the assembly's document and makes no version. |
| MIC.5 | Exercises on stand-ins (Suspension, Slide, Gripper) as scenarios; goldens; the Onshape importer maps "In context entity" features onto a context rebuilt from the assembly. |
