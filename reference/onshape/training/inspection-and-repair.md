# Onshape "Inspection and Repair Tools": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Inspection and Repair Tools
(`learn.onshape.com/learn/course/inspection-and-repair-tools/...`), read on 2026-09-25. The
requirements below are paraphrased, not the course text. The exercise's 17 step screenshots and
an overview render are in `inspection-and-repair/` (git-ignored, local only).

The course is short (about 15 minutes): four video lessons (under a minute each, with key
takeaways and a transcript) and one slide exercise with 17 steps, followed by a self-check, which
was **not started**, and a completion survey, which was not opened. The video frames couldn't be
saved (they came out blank), so the images are all from the exercise slides, which show the same
panels.

Each requirement has an ID (`IR<lesson>.<n>`). Earlier notes already cover some of this: the
sketch dialog's diagnostics menu and the Constraint manager's filters are in
[../sketch_dialog.md](../sketch_dialog.md) and [../constraints.md](../constraints.md) ("optional,
later"), the `:errors` feature-list filter is PS3, dependencies and reordering are PS11, and
rollback and **Final** are PS13.

## 1. Sketch troubleshooting

### IR1 Profile inspector (video, 0:34; exercise images `ex1-step6.png`, `ex1-step7.png`, `ex1-step8.png`)
- IR1.1 A sketch diagnostic that finds **loose ends**: endpoints that don't join anything, so a
  profile doesn't close into a region.
- IR1.2 Open it from the sketch dialog footer: the **Show sketch diagnostic tools** icon (a small
  magnifier-over-sketch icon next to **Final**) → menu **Profile inspector…** /
  **Constraint manager…** (`ex1-step3.png`, `ex1-step6.png`).
- IR1.3 It opens as a small floating panel titled **Profile inspector** with a × close. Its list,
  captioned "Loose ends", has one row per loose end; coincident pairs are grouped (e.g. "Loose
  end", "Loose ends (2)"). Every loose end is marked with a **red circle** in the viewport.
- IR1.4 Clicking a row **zooms the view to that location**. **Previous** and **Next** buttons (blue)
  step through the list, and a "?" help icon sits at the bottom.
- IR1.5 The list is **live**: once a loose end is fixed (e.g. by a Coincident that closes the
  loop), its row disappears.

### IR2 Constraint manager (video; images `ex1-step4.png`, `ex1-step5.png`)
- IR2.1 A floating panel that lists **every constraint and dimension** in the active sketch.
- IR2.2 Open it from the same diagnostic tools menu (IR1.2). Panel title **Constraint manager**,
  × close.
- IR2.3 **Filters** section (collapsible, with a dropdown chevron):
  - **Automatically select constraints** toggle (on by default). When **off**, the list shows only
    the constraints of the **currently selected entities**, which isolates one area of the sketch.
  - **Type**: an icon grid with one toggle per constraint type (coincident, concentric, parallel,
    tangent, horizontal, vertical, perpendicular, equal, midpoint, normal, pierce, symmetric,
    dimension kinds, curvature, pattern and others; about 22 icons), matching the S12 table.
  - **Mode**: three icons, **internal**, **external** and **in-context** (from
    [../constraints.md](../constraints.md)).
  - **Status**: three icons, **driven**, **solved** and **errors**. The exercise uses the red
    **Errors** icon.
- IR2.4 **Sort by constraint** / **Sort by entity** tabs.
- IR2.5 The **Entities and constraints** list: each constraint is a header row (type icon + name,
  e.g. "Parallel 1", "Equal 1", "Symmetric 1") with the entities it uses indented below ("Line 2",
  "Circle 1"). Constraints on external geometry show the source in brackets, e.g.
  "Coincident 18 [Extru…]".
- IR2.6 Rows in **error are red**. Hovering a constraint or entity row **highlights** its geometry
  in the viewport.
- IR2.7 Each constraint row has a **trash icon** (delete that constraint) and a **×** icon. **Delete
  all** (a red button at the bottom) deletes every constraint in the filtered list. Both go
  through undo.

## 2. Repair and replace references

### IR3 Repair (video; images `ex1-step11.png`, `ex1-step12.png`, `ex1-step13.png`)
- IR3.1 **Repair** shows an earlier, healthy state of the Part Studio next to the current one, so
  you can see where a missing reference used to point.
- IR3.2 Entry point 1: **Versions and history** panel → the **Repair** icon (a wrench and hammer)
  in the panel header → pick a history entry from **before** the reference broke. Or right-click
  a history entry → **View in repair**.
- IR3.3 Entry point 2: right-click a broken feature → **Edit healthy moment of <feature>…** (in the
  feature context menu under Edit…, `ex1-step2.png`). The same command is on the right-click menu
  of a broken selection inside a feature dialog. The Repair panel then shows the feature's **last
  successful regeneration**.
- IR3.4 The **Repair panel** is a **second, view-only viewport** on the right side of the Part
  Studio. Its header says **Repair** and "Viewing Main :: <tab> :: <change>" (e.g. "Viewing Main
  :: Conrod :: Edit : Sketch 2"), with an open-in-new icon and a "?" icon. It has its own view
  cube. Clicking the Repair icon again picks a different entry.
- IR3.5 **Synchronize view** (on by default): the main viewport and the Repair panel rotate, pan
  and zoom together. Turn it off to move them independently.
- IR3.6 While editing the broken feature in the main viewport, **hovering the missing reference**
  in a selection field highlights **where it used to be** in the Repair panel. Fix it by selecting
  new references and deleting the missing ones from the field.
- IR3.7 Right-click in the Repair panel: **Zoom to fit** and **Isometric**.
- IR3.8 A related icon (wrench and hammer) also appears in the right-edge viewport icon strip while
  Repair is open (`ex1-step13.png`).

### IR4 Replace reference (video; images `ex1-step13.png`–`ex1-step16.png`)
- IR4.1 **Replace reference** swaps a missing (or any) reference in a feature for a new one, and
  can **propagate** the swap to all downstream features that used the same reference.
- IR4.2 It's available **only while the Repair panel is open**: hovering a selection field that
  holds a reference shows a small **Replace reference** icon at the right end of that field
  (tooltip "Replace reference").
- IR4.3 The icon opens a second dialog, **Replace reference**, next to the feature dialog, with its
  own ✓/✗ and two fields: **Selection to replace** (pre-filled with the missing reference, in red)
  and **Selection to replace with** (focused, pale blue). Select the new face, edge or region in
  the viewport or the feature list. It replaces **one reference at a time**.
- IR4.4 **Propagate changes** checkbox (on by default): push the replacement to every dependent
  feature.
- IR4.5 Accepting Replace reference returns to the feature. Accepting the **feature** (✓) accepts
  both. The course notes that a single ✓ on the feature accepts both dialogs.
- IR4.6 Best practice: with several broken features, fix them **top to bottom** in the feature
  list, because later errors are often caused by earlier ones.
- IR4.7 A tangent chain picks up the whole loop from one edge (tangent propagation). A replacement
  face whose edges aren't tangent-connected gives a different edge set, so check the result
  (step 16 pro-tip).

### Error display seen in the exercise (not taught explicitly, but visible in every screenshot)
- IR5.1 A failing feature's name is drawn **red** in the feature list, with a small **warning
  badge** on its icon. The **Features (n)** header gets a red **!** icon when anything fails
  (`ex1-step1.png`).
- IR5.2 A feature whose input disappeared shows the field entry in red, e.g. **Missing Face of
  Sketch 3** or **Missing Edge of Extrude 5**, and the field gets a red tint (`ex1-step13.png`,
  `ex1-step15.png`).
- IR5.3 An unsolvable sketch shows a yellow warning toast at the top of the viewport, "Sketch
  could not be solved." with a ×, and its dialog title turns red (`ex1-step3.png`). Offset shows a
  similar toast when the distance is impossible, "Offset could not be created at this distance."
  (`ex1-step10.png`).
- IR5.4 Features below the one being edited are shown greyed and italic (rolled back, PS13.1).
- IR5.5 **Feature context menu** as captured in `ex1-step2.png`: Rename, **Edit…**, **Edit healthy
  moment of <name>…**, Copy sketch, Show dimensions, Add selection to folder…, Show, Show all
  sketches, Section view…, Suppress, Dynamic suppression ▸, Add comment, Zoom to selection, Show
  dependencies…, **Roll to here**, Edit sketch appearance…, Delete.
- IR5.6 **Versions and history panel** as captured in `ex1-step11.png`/`ex1-step12.png`: opened
  from the top of the left icon strip. Header: title, create-version icon, branch icon,
  compare icon, **Repair** icon, ×. A **Search history** box with a dropdown. Two filter toggles.
  Columns **Name** / **Modified** (user and time). A vertical graph: **Main** (workspace, open
  circle) with a collapsible "**n changes**" row that expands into entries named like
  "Conrod :: Edit : Sketch 2"; **Start** (document creation). A legend at the bottom: Workspace,
  Version, Change, Release candidate, Release, Contains obsolete, Automatic version. Footer hint:
  right-click a row for actions. In Repair mode, a blue banner says to pick a history entry to
  repair the Part Studio.

## 3. Exercise

### IR6 Exercise: Conrod (`inspection-and-repair/ex1-*`, overview `ex1-overview.png`)
Goals: fix an unsolvable sketch with the Constraint manager and the Profile inspector, then fix
missing references with Repair and Replace reference. The model is a **connecting rod** (two ring
bosses joined by a tapered I-beam web with a long slot pocket), in **inches**. It starts from the
**public document "Exercise: Conrod"** (Make a copy). Its Part Studio "Conrod" has **Features
(19)**: Sketch 1, Extrude 1–3, Draft 1, Fillet 1–2, Sketch 2, Extrude 4, Circular pattern 1,
Fillet 3, Sketch 3, Extrude 5, Fillet 4, Mirror 1; and one part, **Conrod**.

Not buildable from scratch as written: the exercise starts from a **deliberately broken**
document, and no full dimensions are given. The visible dimensions are the Sketch 2 notch angle
(**30°**), the Sketch 3 slot (**R0.15**, **0.1** end clearances), Extrude 5 depth **0.07 in**
(Remove), and Fillet 4 radius **0.03 in**. A cadrs scenario can reproduce the *workflow* on any
similar part (see below).

1. IR6.1 Open "Exercise: Conrod", **Make a copy**. Sketch 2 has an error, and the features after
   it fail. The course text names Extrude 5 and Fillet 4. In the step 1 screenshot, **Extrude 4**
   and **Circular pattern 1** are the red ones (`ex1-step1.png`, `ex1-step2.png`).
2. IR6.2 Right-click **Sketch 2** → **Edit…**. It's on "Face of Extrude 3", a ring on the small end
   with a 30° notch. The "Sketch could not be solved." warning shows (`ex1-step3.png`).
3. IR6.3 Sketch dialog → diagnostic tools → **Constraint manager…**.
4. IR6.4 Status filter → **Errors** (`ex1-step4.png`). The list shows Coincident 18 and 21 (to
   Extrude geometry), Concentric 1 (Circle 1, Circle 2) and **Equal 1** (Circle 1), all red
   (`ex1-step5.png`).
5. IR6.5 Delete **Equal 1** with its trash icon; close the Constraint manager.
6. IR6.6 Diagnostic tools → **Profile inspector…** (`ex1-step6.png`).
7. IR6.7 Two rows: "Loose end" and **"Loose ends (2)"**. Click the second to zoom to it
   (`ex1-step7.png`).
8. IR6.8 **Coincident** (shortcut **i**): click endpoint 1, then endpoint 2. The loop closes into a
   region. Accept the sketch. Every feature error clears (`ex1-step8.png`).
9. IR6.9 Right-click **Sketch 3** (on "Face of Extrude 1": the slot outline on the web) → Edit…
   → select all its entities and **delete** them (`ex1-step9.png`).
10. IR6.10 **Offset**: click the web's face region, drag the arrow **inward**, click, type
    **0.1 in**, then accept. The bottom-right **Area** readout shows 1.339 in² while the region is
    highlighted (`ex1-step10.png`). Extrude 5 and Fillet 4 now fail, because the region and edges
    they used are gone.
11. IR6.11 **Versions and history** → **Repair** icon → expand "**2 changes**" (`ex1-step11.png`).
12. IR6.12 Pick the entry **"Conrod :: Edit : Sketch 2"** (before the Sketch 3 edit); close the
    history panel. The Repair panel shows that state (`ex1-step12.png`).
13. IR6.13 Right-click **Extrude 5** → Edit… (Solid, **Remove**, Blind, 0.07 in, Merge scope
    Conrod). Hover the red "Missing Face of Sketch 3" entry; the old slot region lights up in the
    Repair panel. Click the **Replace reference** icon (`ex1-step13.png`).
14. IR6.14 Show Sketch 3 (eye icon on its row). With **Selection to replace with** focused, click
    the new Sketch 3 region. ✓ the feature (accepts both). Hide Sketch 3 (`ex1-step14.png`).
15. IR6.15 Right-click **Fillet 4** → Edit… (Edge, Radius 0.03 in, Tangent propagation ✓, Allow
    edge overflow ✓). Hover "Missing Edge of Extrude 5", then Replace reference
    (`ex1-step15.png`).
16. IR6.16 **Selection to replace with**: the **bottom face of the pocket** ("Face of Extrude 5").
    ✓. No errors remain (`ex1-step16.png`).
17. IR6.17 Select **Conrod** in the Parts list → **Mass properties** → read the mass (**lb**). The
    self-check quiz asks for it (`ex1-step17.png`).

Self-check: the **mass in lb**, which the screenshot blanks out in red. The same screenshot shows
**Volume 1.149 in³**, **Surface area 16.269 in²** and **Center of mass ≈ (0, 0, 1.555) in**. The
inertia values are Lxx 0.859, Lyy 0.892, Lzz 0.049 lb·in². The volume and area are the usable
checks for cadrs, because the material (and so the density) isn't given.

cadrs scenario idea (buildable): any part with (a) a sketch that has a conflicting redundant
constraint and a gap, and (b) a pocket and fillet built on a sketch region that is then deleted
and redrawn. Expected results: the Constraint manager lists the conflict in red; the Profile
inspector finds the gap; after the redraw, Extrude and Fillet show "Missing …"; Repair with
Replace reference restores them; the final volume matches the unbroken model.

## Self-check and survey
- The exercise's **Begin Self-Check** page (page 3) was visited but not started. It asks for the
  mass from IR6.17.
- The **Completion Survey** ("Completion Certificate" section) was not opened.

## Cross-cutting requirements found in the course
- X1 **Sketch diagnostics menu** in the sketch dialog footer with two floating panels: the
  **Profile inspector** (loose ends, zoom to one, Previous/Next, a live list) and the **Constraint
  manager** (filters by Type/Mode/Status, an auto-select toggle, sort by constraint or entity,
  red error rows, hover highlight, per-row delete, Delete all) (IR1, IR2).
- X2 **Solver error reporting**: the solver has to name *which* constraints conflict (a
  per-constraint error status), not just "failed". It also has to detect **loose endpoints** of
  non-construction geometry (IR2.6, IR1.1).
- X3 **Feature error states**: red feature names with a warning badge, a red "!" on the list
  header, "Missing <entity> of <feature>" placeholders kept in selection fields (not silently
  dropped), and warning toasts ("Sketch could not be solved.", "Offset could not be created at
  this distance.") (IR5). This needs **persistent references** that stay in the field when they
  can't be resolved.
- X4 **History-backed repair view**: a second, view-only viewport that renders the Part Studio at
  a past history entry, or at a feature's **last healthy regeneration**, with synchronized
  cameras, and cross-highlighting of a missing reference's old geometry (IR3). This needs the
  document history of T9 / TD12 to be able to **regenerate any past state**.
- X5 **Replace reference** with **Propagate changes** (rewrite every downstream use of a
  reference) (IR4). This is the repair counterpart of persistent naming.
- X6 **Feature context menu** entries: **Edit healthy moment**, **Roll to here**, **Dynamic
  suppression**, **Suppress**, Show dependencies (IR5.5; see PS11, T10).
- X7 **Versions and history panel** layout, and the legend for workspace, version and change
  (IR5.6), shared with T7, T9 and TD12.
- X8 **Inch units** and **lb** mass for this exercise (PS X8).
