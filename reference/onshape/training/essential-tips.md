# Onshape "10 Essential Onshape Tips": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → 10 Essential Onshape Tips (an article,
`learn.onshape.com/learn/article/10-essential-onshape-tips`), read on 2026-09-25. The
requirements below are paraphrased, not the article text. The article is a short list of ten
best-practice guidelines (about 5 minutes of reading). It has **no exercises, no shortcuts and no
screenshots**; its only image is a decorative hero render of an engine assembly, which was not
saved. There's no image folder for this item.

Each requirement has an ID (`T<n>.<k>`, where `n` is the tip number) so milestones and judges can
refer to it. Most tips describe Onshape's document and data model rather than a UI flow, so most
IDs are product-level requirements. The cloud-only parts (sharing, multi-user history, paid
release management) are noted as such; they're mostly out of scope for a local desktop app, but
the data model behind them (documents, tabs, versions, history) is not.

## Tips

### T1 Documents are project containers
- T1.1 A **document** is the container for everything in one project: Part Studios, Assemblies,
  Drawings, and **non-CAD files** (photos, videos, PDFs) stored as tabs. See P1.1 and PS2.12 (the
  tab bar with Part Studio, Assembly, Drawing and image tabs).
- T1.2 Guidance: keep a document to about **40 tabs or fewer**. cadrs should cope with that many
  tabs in the tab bar (scrolling or overflow), but needs no hard limit.
- T1.3 A tab can be **moved to another document** as a design matures (the tab menu's **Move to
  document…**, already listed in D2.10).

### T2 Same-workspace references update automatically
- T2.1 Part Studios and Assemblies that reference each other **within one document workspace**
  update automatically when the source changes: no manual refresh step. (An assembly instance
  follows its Part Studio's edits live; see A2.)

### T3 Cross-document references point at a version
- T3.1 When a product spans several documents, a reference from one document into another points
  at a **version** of the source document, not its live workspace.
- T3.2 The referencing side **chooses when to update** to a newer version (an explicit "update
  reference" action). Until then, it keeps showing the pinned version. This is the article's
  answer to the broken-assembly problem of file-based CAD.
- T3.3 cadrs implication: an inserted instance or derived part from another document has to store
  `(document, version, element, part)` and offer an update-to-latest-version action. See the
  **Other documents** source tab in A2.3 and D X11 (referencing a version instead of the
  workspace).

### T4 Part Studios define parts, not assemblies
- T4.1 A Part Studio is for **shape**, not for positioning parts as an assembly. Motion and
  instancing belong in an Assembly (see PS18.4).
- T4.2 Guidance: a typical Part Studio holds **1–10 unique parts** and **fewer than 250 features**.
  cadrs should stay responsive at that size (a regeneration budget target for 250 features),
  and could warn gently when a studio grows well past it.

### T5 One mate per part pair
- T5.1 A single mate usually defines the whole relationship between two parts (the mate type
  carries all the DOF). Already captured as A6.2; nothing new.

### T6 Share instead of export
- T6.1 Sharing a document with collaborators (with permissions) is preferred over exporting
  files, to protect IP. Cloud-only; out of scope for a local app. Export itself is covered in
  P X7 and D X13.

### T7 Versions
- T7.1 A **version** is a **named, immutable** snapshot of the whole document.
- T7.2 **All tabs are versioned together**; there's no per-tab version.
- T7.3 References should target versions wherever possible (T3).
- T7.4 Versions are not a release process. Formal **release management** (approvals, revisions,
  part numbers) is a paid Onshape tier and out of scope.
- T7.5 cadrs implication: a **Create version** command (name + optional description) on the
  document, a read-only view of any version, and version pins for references. See P1.2 and P X8
  (automatic history), which this extends with named versions.

### T8 Import
- T8.1 Choose the import option to fit how the imported data will be used (for instance, keep an
  imported assembly as an assembly, or flatten it into one Part Studio).
- T8.2 Preferred neutral formats, best first: **Parasolid**, then **STEP**, then **IGES**. For
  cadrs, **STEP** is the realistic first target (Parasolid is proprietary), with IGES as a
  lower-priority option.

### T9 Restore from document history
- T9.1 The document's **history** can restore **any earlier state** of the document.
- T9.2 A restore applies across **all tabs** and all changes (by all users) at once; it's a
  document-level rollback, not a per-feature undo. This is different from the session
  Undo/Redo in PS2.1.
- T9.3 cadrs implication: a persistent, document-level change history (every command recorded,
  not just the in-memory undo stack), with a **Restore** action that itself goes on the history
  as a new entry, so it can be undone. See P1.2 and PS2.1 (**Versions and history** panel).

### T10 Avoid heavy cosmetic features
- T10.1 Guidance: avoid or **suppress** purely cosmetic, geometry-heavy features, such as modelled
  threads, knurling, and complex embossed text or symbols.
- T10.2 cadrs implication: features need a **Suppress / Unsuppress** toggle (right-click on a
  feature), and a suppressed feature is skipped during regeneration but kept in the list. Hole
  threads should be cosmetic (metadata, not geometry) by default (see PS15).

## Exercises
None. The article has no hands-on steps, quiz or self-check.

## Cross-cutting requirements found in the article
- X1 **Document data model**: one document holds many tabs of mixed types (Part Studio, Assembly,
  Drawing, imported files such as images, video, PDF), and tabs can move between documents (T1).
- X2 **Live in-document references** that regenerate automatically (T2).
- X3 **Named immutable versions** of the whole document, and **version-pinned cross-document
  references** with an explicit update action (T3, T7).
- X4 **Document history with restore**, persisted with the document and covering every tab (T9).
- X5 **Import** of STEP (and later IGES), with a choice of how to bring in assemblies (T8).
- X6 **Feature suppression** (T10).
- X7 **Scale targets**: about 40 tabs per document, 250 features and 10 parts per Part Studio
  should stay responsive (T1.2, T4.2).
- Out of scope (cloud or paid tier): sharing and permissions (T6), multi-user history (T9.2's
  "all users"), release management (T7.4).
