# Onshape "Hands-On Test Drive": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Onshape Hands-On Test Drive (optional course,
`learn.onshape.com/learn/course/onshape-hands-on-test-drive/...`), read on 2026-09-25. The
requirements below are paraphrased, not the course text. Images are in `test-drive/` (git-ignored,
local only).

The course is a one-hour guided tour: 14 short lessons (mostly 1–2 minute videos with a
transcript, plus one interactive hotspot tour of the Documents page) and a completion survey,
which was not opened. It follows **one worked example** through a copied public document, a
cordless-drill model ("DRILL HOTD"; Initial and Finished documents are linked from lesson 4). It
was read only; the document was not copied and nothing was done in Onshape.

Each requirement has an ID (`TD<lesson>.<n>`). Much of the tour repeats topics that the core
courses already cover in more depth, so those items point to the existing IDs (S, PS, A, D, P, T)
instead of repeating them. What's new here is mainly: **camera and selection basics**, **BOM ↔
part property sync**, **drawing title-block property links**, **branches and merge**, and the
cloud features (sharing, follow mode, comments, release management), most of which are out of
scope for a local app.

## 1. Getting started

### TD1 Introduction (video, 1:48)
- TD1.1 Onshape runs in any WebGL browser with no install and no files. Every action is recorded
  in the document history, so there's **no Save button** and no lost work. cadrs implication:
  **auto-save** every committed command to the document store, with the history from T9 / P1.2.
- TD1.2 Subscription tiers (Standard, Professional with release management, Enterprise with
  analytics and security). Out of scope.

### TD2 Signing in (text page)
- TD2.1 Web sign-in and Enterprise SSO. Out of scope (local app, no accounts).

### TD3 Documents page (video + hotspot tour; image `lesson-documents-page.png`)
The Documents page is the home screen that lists documents. A local-app equivalent is a **project
browser / start screen**. The tour names these areas:
- TD3.1 **Create** button (top left): new Document, Folder, Publication or Label, plus **Import**
  from local disk or cloud storage.
- TD3.2 **Search** field (top): type part of a name, press Enter.
- TD3.3 **Filters** (left column): Recently opened, Created by me, Shared with me, Teams, Labels,
  Public. They narrow the list.
- TD3.4 **Trash** (bottom of the left column): recently deleted documents can be recovered.
- TD3.5 **Last opened by me** (collapsible strip of thumbnails at the top): click the name or
  double-click the thumbnail to open.
- TD3.6 **Folders** section: organize (and share) documents.
- TD3.7 **Documents** list: every other document, most recently modified first, with columns
  thumbnail, **Name** (with the last-opened workspace, e.g. "Main"), labels, **Modified** time,
  **Modified by**, and owner. A **Type** filter dropdown and list/grid view toggles sit above it.
- TD3.8 **Details flyout** (right panel, opened by selecting an item): thumbnail, owner,
  description, labels (searchable checkboxes, **Create new label**), created by; side tabs for
  info, sharing, versions and history, where used.
- TD3.9 Top-right icons: **Action items** (assigned tasks and comments, open/closed), **App
  Store**, **Learning Center**, **Help**, **Account menu** (settings, company, support, sign out).
  Mostly out of scope; Help (with a shortcut list, TD6.4) is in scope.
- TD3.10 Enterprise adds a top toolbar and **Projects**. Out of scope.

### TD4 Copy course documents (video)
- TD4.1 Course documents are public; **Make a copy** creates your own editable copy. The same
  pattern as the D8.1 / A15 exercises. For cadrs: bundled sample documents that open as an
  editable copy.

### TD5 Document interface (video, 0:40)
- TD5.1 A document holds everything for one project; tabs at the bottom are Part Studios,
  Assemblies, Drawings and imported files (PDF, images, video). See T1.1 and PS2.12.
- TD5.2 The **"+"** button adds a tab (see D1.2).
- TD5.3 Tabs can be organized into **tab folders**; inside a folder, a **Home** button returns to
  the top level.
- TD5.4 The **Tab manager** (bottom left) shows the tabs as a **vertical list**, easier to
  organize and search than the horizontal bar.

## 2. Part Studios, Assemblies and Drawings

### TD6 Part Studio interface (video)
- TD6.1 A Part Studio can hold **several interrelated parts** that share dimensions and hole
  positions (see PS18).
- TD6.2 Mouse: **wheel scroll = zoom**, **right-drag = rotate**, **middle-drag = pan**.
- TD6.3 Keys: **F** = zoom to fit; **Shift+7** = isometric view.
- TD6.4 The full keyboard-shortcut list is reachable from the **Help** menu.
- TD6.5 The **view cube** re-orients the model; the **camera and render options** menu below it
  has more view and appearance options (see PS2.9, P6.4).
- TD6.6 **Measure** and **analysis** tools at the bottom right (see PS2.11).
- TD6.7 **Persistent selection**: clicking adds to the selection with no Shift or Ctrl needed.
  Clicking empty space or pressing **Space** clears the selection.

### TD7 Creating a part (video; image `lesson-creating-a-part-extrude.png`)
- TD7.1 **Extrude** from a **planar face** directly, with no sketch: select the face, then click
  Extrude. Extrude accepts a sketch region, a planar face or a surface. (PS1 covers sketch
  regions; face input is the new bit.)
- TD7.2 Solid / Surface / Thin and New / Add / Remove / Intersect in one feature (PS X2).
- TD7.3 When the input face belongs to an existing part, the boolean **defaults to Add** with that
  part in **Merge scope** (PS5.2, PS5.4). Choose **New** to make a separate part.
- TD7.4 The screenshot shows the dialog's field order: Solid/Surface/Thin tabs; New/Add/Remove/
  Intersect tabs; the input field (e.g. "Face of Extrude 24"); end type **Blind**; **Depth**;
  **Direction**; **Starting offset**; **Symmetric**; **Draft**; **Second end position**; **Merge
  with all**; **Merge scope**; rollback slider at the bottom. The feature list header shows the
  count, e.g. "Features (187)".
- TD7.5 A new part appears in the **Parts** list and is renamed there.

### TD8 Assemblies (video)
- TD8.1 An assembly defines **hierarchy and motion**. Parts and subassemblies can be inserted
  from the **same or another document** (A2.3, T3).
- TD8.2 A mate defines up to all 6 DOF between two components through **mate connectors** (a
  full coordinate system: origin, axes, XY plane). One mate per relationship (A6.1, A6.2).
- TD8.3 **Group** locks parts together by their origins; it's best for parts from one Part Studio
  (A13).
- TD8.4 A **Revolute** mate with **limits** shows a range of motion (A6.12).
- TD8.5 While placing a mate connector, **hold Shift** to lock the hovered face, then pick the
  hole center (A6.5).
- TD8.6 When inserting an assembly, use the dialog's **Assemblies** tab so you don't insert the
  Part Studio by mistake.
- TD8.7 **Flip primary axis** and **Reorient secondary axis** fix the orientation. In the mate
  dialog only the mated parts move; **Solve** shows the final result (A6.8, A6.11).
- TD8.8 **Standard content**: Insert → Standard content → ISO → Bolts & screws → Socket head
  screws → ISO 4762 hex socket head cap screw, size **M5**, length **25 mm**; click the two holes
  to place (A19.1, A19.6).
- TD8.9 Standard content has explicit mate connectors that stay visible after placement; **K**
  hides them (PS X11).

### TD9 Properties and Bill of Materials (video)
- TD9.1 The **BOM** opens from the right-side panel of an assembly and lists all instances with
  their properties (A20.1).
- TD9.2 In the BOM you can fill in missing properties, add and reorder columns, and **export to
  CSV** (A20.6 covers columns; CSV export is new).
- TD9.3 **Two-way property link**: editing a cell in the BOM writes the component's own property
  (e.g. Part number), and editing the part's **Properties** dialog updates the BOM. There's one
  source of truth, the part's metadata (P1.3).
- TD9.4 BOM row right-click: **Generate next part number** (when automatic numbering is on) and a
  **Switch to** the source Part Studio (A4.6).
- TD9.5 Parts list right-click → **Properties** opens the part's property dialog (Part number,
  Description, …).

### TD10 Drawings (video)
- TD10.1 Drawings stay parametrically linked to the model (D X11).
- TD10.2 **"+" → Create drawing**, template **ANSI_B_MM** (D1.2); custom templates can be made and
  shared.
- TD10.3 Rename the tab by right-clicking it (D2.10).
- TD10.4 Insert view: **Assemblies** tab → pick the assembly → **Front** at **1:2**, click to
  place, move right and click again to place a **Right** projected view (D4).
- TD10.5 Insert a **BOM table** of type **Structured – Top level**, anchored at its bottom-right
  corner to the left edge of the title block (D11).
- TD10.6 Add height and depth dimensions, and item-number callouts (D6, D11).
- TD10.7 **Title-block property links**: a title-block annotation can refer to a **drawing
  property** or to a **sheet reference (model) property**. The exercise switches the title field
  to the model's **Name** and the drawing number to the model's **Part number**, and deletes the
  unused annotations. cadrs needs a note field type that resolves to a property of the referenced
  model.
- TD10.8 After a model change, **Update from this workspace** refreshes the views (D13).

## 3. Collaboration and data management

### TD11 Sharing and collaboration (video)
- TD11.1 **Share** (top right of a document): share with users or teams, or with a **link** that
  needs no account. Folders can be shared the same way. Out of scope.
- TD11.2 **Social cues**: avatars of users currently in the document, which tab and feature
  they're in; double-click one to enter **Follow mode** (see their view, cursor, selections and
  dialogs). Out of scope (no multi-user).
- TD11.3 **Comments** on a document, with **@mentions** and **assign as action item**. A comment
  can carry a **markup** (arrows and text drawn over a view capture, attached as an image).
  Out of scope for now; a local "notes" panel could be considered later.

### TD12 Data management (video)
- TD12.1 The **Versions and history** panel lists every change with action, user, date and
  time. **Restore** works from any point (T9, PS2.1).
- TD12.2 The editable state is the **workspace**, shown with an **open circle** in the history
  graph.
- TD12.3 **Create version** with a name (e.g. "FUEL AND POWER TRAIN COMPLETE") marks a milestone
  (T7).
- TD12.4 **Branches**: right-click a named version → **Branch to create workspace**, with a name
  ("Alternate Gasket Thickness"). A branch is a separate, independent workspace for variants or
  experiments. A branch can only start from a **version**.
- TD12.5 Worked example: in the branch, edit the gasket's Extrude depth from 2 mm to 1 mm; the
  **Main** workspace keeps 2 mm.
- TD12.6 **Merge**: activate the destination workspace, right-click the source workspace →
  **Merge into current workspace**. A dialog lists the **changed tabs**; for each tab choose
  **replace with the source branch** (default) or **keep the destination**. Then **Merge**.
- TD12.7 The merge is itself a history entry; to undo it, right-click the entry before it →
  **Restore**.
- TD12.8 cadrs priority: named versions and restore (T7, T9) come first; branches and merge are a
  later, optional layer on the same history model (tab-level replace is simpler than a
  feature-level merge).

### TD13 Release management (video)
- TD13.1 Formal revisions and approvals of parts, assemblies, drawings and files; paid tiers
  only. Out of scope, like T7.4.
- TD13.2 Items the course shows, for reference: a drawing **Revision table**; tab right-click →
  **Release** to create a **Release candidate** that pulls in the drawing's references; every
  item needs a **unique part number** (**Generate missing part numbers** icon in the dialog);
  release name, notes, **Release** or **Submit** to an approver; history marks with a hollow
  triangle (pending) and a solid triangle (released); the released drawing gets the approver and
  date filled in. A **revision table** on drawings is the only piece worth keeping in mind (it's
  already listed as shown-but-not-taught in D X14).

## 4. What's next

### TD14 Where to go from here (text page)
- TD14.1 Links to the CAD Basics and Onshape Fundamentals pathways, Bootcamp, and advanced topics
  (Top-down design, Configurations, Sheet metal, Release management for admins). No
  requirements.

## Exercises
The whole course is one guided walkthrough on the public "DRILL HOTD" document. It is **not
buildable from scratch**: it starts from a large existing model (187 features in the Carburetor
Part Studio) and only adds a gasket, a mate, two screws, a BOM edit, a drawing and a branch.
There's no numeric self-check such as mass or area. The steps, in order:
1. Copy the Initial document (TD4).
2. CARBURETOR Part Studio: select the MANIFOLD's mounting face → Extrude → **New**, depth
   **2 mm** → rename the part **CARBURETOR_GASKET** (TD7).
3. CARBURETOR Assembly: insert the gasket; **Fastened** mate gasket ↔ MANIFOLD using hole-center
   connectors (hold Shift to lock faces) (TD8.5).
4. FUEL AND POWER TRAIN Assembly: insert the CARBURETOR **assembly** (Assemblies tab), Fastened
   mate with Flip / Reorient as needed, Solve (TD8.6, TD8.7).
5. Insert two **ISO 4762 M5×25** socket head cap screws into the manifold's back holes; press K
   (TD8.8, TD8.9).
6. BOM: fill the gasket's Part number (Generate next part number or type it); switch to the Part
   Studio, open Properties, check it; fill Description there and check it in the BOM (TD9).
7. Drawing: ANSI_B_MM, tab "FUEL AND POWER TRAIN", Front 1:2 + Right, Structured top-level BOM,
   height and depth dimensions, callouts, title fields linked to model Name and Part number
   (TD10).
8. Share / follow mode / comment with a markup "1 mm thick" (TD11; cloud only).
9. Version "FUEL AND POWER TRAIN COMPLETE" → branch "Alternate Gasket Thickness" → gasket depth
   1 mm → merge into Main, replacing tabs (TD12).
10. Revision table "Initial Release" → Release the drawing (TD13; paid tier).
The **Finished document** is the comparison target. For cadrs, steps 2–7 and 9 could be a
scenario on any small multi-part model: extrude-from-face as a new part, a fastened mate, BOM ↔
properties sync, a drawing with property-linked title fields, and version → branch → merge.

## Cross-cutting requirements found in the course
- X1 **Auto-save**: every committed command is persisted, with no Save button (TD1.1).
- X2 **Camera controls**: wheel zoom, right-drag rotate, middle-drag pan, **F** fit, **Shift+7**
  isometric, view cube, and a shortcut list in Help (TD6).
- X3 **Persistent (additive) selection**: plain clicks add to the selection; empty-space click or
  **Space** clears (TD6.7).
- X4 **Extrude from a planar face**, not just from sketch regions (TD7.1).
- X5 **Start screen / document browser**: create, import, search, recent documents, folders,
  trash with recovery, a details panel (TD3).
- X6 **Tab folders and a vertical tab manager** (TD5.3, TD5.4).
- X7 **Single-source part properties**: Part number and Description edited in the BOM or in the
  part's Properties dialog are the same data; BOM **CSV export** (TD9).
- X8 **Title-block fields linked to model properties** (Name, Part number) in drawings (TD10.7).
- X9 **Versions, branches and merge** on the document history (TD12), building on T7 and T9.
- Out of scope: sign-in and SSO, subscriptions, sharing and link sharing, social cues and follow
  mode, comments and action items, App Store, release management.
