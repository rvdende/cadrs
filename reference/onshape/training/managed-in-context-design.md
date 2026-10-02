# Onshape "Managed In-Context Design": requirements for cadrs

Source: Onshape Learning Center, Managed In-Context Design
(`learn.onshape.com/learn/course/managed-in-context-design/...`), read on 2026-10-02. The course has
three sections: an introduction, Understanding Managed In-Context Design (three video lessons and
two exercises), and Using Managed In-Context Design with Linked Documents (one video lesson and
one exercise). The requirements below are paraphrased, not the course text.

It extends what [intro-to-assemblies.md](intro-to-assemblies.md) X15 (Edit in context) and
[pcb-studio.md](pcb-studio.md) X9 (Create Part Studio in context) already asked for. Where the
course only confirms one of those, the line points at it; the new material is marked **(new)**.

Each requirement has an ID (`MC<lesson>.<n>`) so milestones and judges can refer to it.

Seen in the scraped Onshape data (ekgpulse, the bracket Part Studio, 2026-10-02): a feature that
references a context carries one **"In context entity"** subfeature (`importForeign`, with a
`foreignId` naming the frozen geometry and `isInContext: true`) per context entity it uses, and
its queries name that subfeature (`qCreatedBy(id + "<subfeature>", EntityType.FACE)`). So
Onshape freezes the context's **geometry**, not only its positions.

## MC1 What managed in-context design is (video, 2:13)
- MC1.1 Parts are created or edited **in the context of an assembly**: geometric or spatial
  relations between parts of different Part Studios, or parts whose position in the assembly
  (its motion) defines a feature. (X15)
- MC1.2 **(new)** Creating or editing in context takes a **snapshot** of the assembly: the
  position, orientation, **geometry** and **hide/show state** of everything in it at that moment.
  The Part Studio shows the snapshot as a ghosted image to design around.
- MC1.3 The ghosted snapshot is referenced as if its parts were in the Part Studio: sketch on its
  faces, Use (project) its edges, and use it for **end types** of features such as Extrude (up to
  a face of the context). (X15 had sketch-on-face and Use; **(new)**: end types.)
- MC1.4 Part modelling still happens in the Part Studio; the assembly comes to the studio.
- MC1.5 The assembly can be in the same document as the Part Studio, or in another one (linked
  documents, MC4).
- MC1.6 **(new)** **Stability**: an in-context design updates predictably, only when the user
  says so. Moving parts in the assembly, or editing other parts, never changes or breaks an
  in-context feature by itself.
- MC1.7 **(new)** **Several contexts** can be referenced from one Part Studio, e.g. one per
  position of a moving assembly, each with the features that need that position.

## MC2 Creating in-context references (video, 5:50)
- MC2.1 Two ways to make a context: **Edit in context** on an existing part (instance menu), or
  **Create Part Studio in context** on the assembly toolbar for new parts. (X15, X9)
- MC2.2 Edit in context opens the part's Part Studio with the ghosted snapshot (hide/show state,
  position and orientation as in the assembly when the context is made). Hidden instances are
  **not** in the context. (X15; hidden instances **(new)**)
- MC2.3 **(new)** A **transparency slider** at the top centre of the graphics area sets how
  see-through the ghost is.
- MC2.4 The ghost's faces, edges and vertices can be picked and referenced like the studio's own.
  (X15)
- MC2.5 **(new)** A context is only **created when the first reference** to it is made (a
  dimension to its edge, a Use of its edge, a sketch on its face, an extrude up to it). Opening a
  studio in context and leaving without referencing anything leaves no context behind.
- MC2.6 **(new)** A feature defined with in-context references shows an **in-context arrow** next
  to it in the Feature list.
- MC2.7 Leaving: **Go to assembly** (a menu at the top of the graphics area) or the assembly's
  tab. The edits show in the assembly. (X15 "Back to assembly")
- MC2.8 **(new)** In the assembly's Instance list, a part with in-context references shows an
  arrow too (MC3).
- MC2.9 Create Part Studio in context asks for the **origin** of the new Part Studio relative to
  the assembly: the assembly **Origin** or a **mate connector** (made beforehand when position and
  orientation matter). It sets the new studio's origin and orientation. (X9; mate connector
  **(new)**)
- MC2.10 The new Part Studio tab is created in the document and shows the ghost. As with editing,
  the context is only created on the first reference (MC2.5).
- MC2.11 New parts made in context must be **inserted** into the assembly and mated or grouped:
  **Insert and go to assembly** from the menu at the top of the graphics area (pick parts, accept:
  the assembly opens with them inserted), or the assembly's ordinary Insert. (X9)
- MC2.12 **(new)** A **context drop-down** at the top of the Feature list selects which context
  is shown and active, or **editing outside of the context** (no ghost).
- MC2.13 **(new)** Its **overflow menu** next to the context: **Rename**, **Update** and
  **Delete** the context. Descriptive names matter with several contexts.
- MC2.14 **(new)** **New context** for a part that already has one: the instance menu's **Edit in
  context ▸ New context** (after moving the assembly to the wanted position) opens the studio with
  a fresh snapshot.
- MC2.15 **(new)** Each in-context feature shows its arrow; the arrows of the features that
  reference the **active** context are **yellow**, the others not, so it is clear which features
  belong to which context.
- MC2.16 **(new)** **Named positions** with contexts: switch the assembly to a named position,
  then make a context there (no manual moving).

## MC3 Primary and secondary instances (video, 2:10)
- MC3.1 **(new)** The Instance list arrow: **solid** on the context's **primary instance**,
  **dashed** on **secondary** instances (other instances of parts from the same Part Studio).
  Features that reference a context also show the arrow in the Feature list (MC2.6).
- MC3.2 **(new)** The primary instance **anchors** the ghost: the context comes into the Part
  Studio in the same position and orientation relative to the primary instance as in the
  assembly. Secondary instances' positions are not used for that.
- MC3.3 **(new)** The primary instance is the instance Edit in context was used on, or (Create
  Part Studio in context) the first part of the new studio once it is in the assembly. It can be
  changed to another instance of a part of the same Part Studio: **Set as primary instance**
  (instance menu); the solid arrow moves there.
- MC3.4 **(new)** A context **can't be updated without a primary instance**: when the primary
  instance is deleted from the assembly, Update context is refused until another instance is set
  as primary.

## MC4 Editing and updating in-context references (video)
- MC4.1 **(new)** Instance menu **Edit in context ▸ <context>** on a part with arrows opens its
  Part Studio with that context active and the ghost shown; its sketches and features are edited
  as usual.
- MC4.2 **(new)** The same by hand: open the Part Studio and pick the context in the drop-down
  (MC2.12).
- MC4.3 Moving parts in the assembly never breaks or updates the in-context references by
  itself (MC1.6).
- MC4.4 **(new)** When the assembly changes in a way that would change a context (an instance
  moved, a part edited, added, deleted, hidden or shown), a **blue indicator** next to the
  in-context arrow says an update is available. Nothing updates automatically. (X15 has the
  "Assembly changed" bar.)
- MC4.5 Update a context to take: parts added to or deleted from the assembly, changed position,
  orientation or hide/show state, changed geometry. (X15)
- MC4.6 **(new)** Update from the assembly: arrange it, then the instance menu **Update context ▸
  <context>**: the Part Studio's context takes the current snapshot and the features that
  reference it follow. (Recommended, since the assembly is in view.)
- MC4.7 **(new)** Update from the Part Studio: the context drop-down's overflow menu **Update
  context** (the assembly must be in the wanted position).
- MC4.8 **(new)** Contexts belong to their Part Studio: updating one updates that studio only.
  Two studios that need the same assembly position each have their own context.

## MC5 In-context design with linked documents (video, 5:17)
- MC5.1 **(new)** With the Part Studio in another document than the assembly, a **version** is
  always referenced (as for any linked document). Edit in context on such an instance opens the
  part's document with its Part Studio active and the ghost of the assembly shown.
- MC5.2 The ghost is the snapshot of the assembly in the assembly's document (hide/show state,
  position, orientation). Hidden instances are hidden in the context. The ghost is referenced as
  in MC2.4.
- MC5.3 As MC2.5: the context is created on the first reference; the features show arrows.
- MC5.4 **(new)** **Go to assembly** switches back without committing: the assembly still uses
  its version, so the edits don't show there yet.
- MC5.5 **(new)** To bring the edits over, create a version in the part's document: the Create
  version button, or **Create version and go to assembly** in the menu at the top of the graphics
  area, which also updates the assembly's **primary instance** to the new version.
- MC5.6 **(new)** **Create Part Studio in context** always creates the new studio in the
  assembly's document (never a linked document). After its parts are inserted and mated, the tab
  can be moved to another document and becomes linked. (DV4 Move to document)
- MC5.7 The context drop-down and its overflow menu (Rename, Update, Delete) work as in MC2.12,
  MC2.13; **New context** as in MC2.14, then Create version and go to assembly.
- MC5.8 Updating the ghost is needed for the same reasons as MC4.5. Two ways:
  - MC5.8.1 **(new)** From the assembly (instance menu Update context ▸ <context>): the context in
    the other document's Part Studio takes the current snapshot and its features follow, a **new
    version** is created in that document automatically, and the assembly's primary instance is
    updated to it.
  - MC5.8.2 **(new)** From the Part Studio (its workspace, drop-down overflow **Update context**):
    **no** version is created and the assembly keeps the old one, so several contexts can be
    updated before one version (by hand, or Create version and go to assembly). The assembly then
    shows that a new version is available; update it with the Reference manager. (ER2)

## MC exercises
- MCX1 **Suspension** (within a document): edit an existing part in context; create a new part in
  context; update in-context edits after a design change.
- MCX2 **Slide** (primary and secondary instances): a design change that deletes the primary
  instance; set a secondary instance as the new primary.
- MCX3 **Gripper** (linked documents): edit a linked part in context; create a new part in
  context and move it to its own document; update in-context edits after a design change with
  linked documents.

The exercises' documents aren't public; each runs on a stand-in fixture built from our own
features (as the other courses do).

## MC cross-cutting (observed in Onshape, 2026-10-02, ekgpulse bracket)
- MCC1 The bar at the top of the graphics area while a context is active reads **"Context 3 of
  EKG Pulse"**, with **Hide/show instances ▾**, **Done**, **Show internal geometry** and **Select
  transparent geometry** checkboxes, and the transparency slider under them.
- MCC2 The Feature list's header shows **Assembly contexts** with the drop-down ("Context 3 ·
  EKG P…") and an overflow button ("…").
