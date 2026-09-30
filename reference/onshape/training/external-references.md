# Onshape "Linked Documents" (External References): requirements for cadrs

Source: Onshape Learning Center, Fundamentals → Linked Documents
(`learn.onshape.com/learn/course/fundamentals-external-references/...`), read on 2026-09-29. The
course has two sections (Linked Documents, Moving Elements), six video lessons and two exercises.
The requirements below are paraphrased, not the course text. Exercise step screenshots and one
key frame per video lesson are in `external-references/` (git-ignored, local only).

This course overlaps heavily with our own spec [derived-and-linking.md](derived-and-linking.md)
(`DV*`). Where the course only confirms a DV requirement, the line below points at the DV ID
instead of repeating it; the new material is marked **(new)**.

Each requirement has an ID (`ER<lesson>.<n>`) so milestones and judges can refer to it.

## 1. Linked Documents

### ER1 Inserting linked documents (video, 2:50)
- ER1.1 Every **Insert** dialog (assembly Insert, and the other insert-style dialogs such as
  Derived and drawing views) has source tabs across its top: **Current document | Other
  documents | Standard content**. Other documents is the entry point for cross-document data.
  (DV2.1, DV3.3.1)
- ER1.2 **(new)** The Other documents browser, before a document is picked, shows a filter button,
  a search field (**"Search or paste URL"**-style placeholder) and a list of locations: **My
  Onshape**, **Recently opened**, **Created by me**, **Shared with me**, then the user's teams
  (people icon) and labels (tag icon). Choosing one lists its documents with a thumbnail, name and
  workspace/version subtitle (e.g. "Start"). cadrs stand-in: My documents / Recent / Created by me
  plus local folders and labels from the document library (DV1.12).
- ER1.3 **(new)** **Paste a document URL** into the search field to jump straight to that document
  (the exercise uses this). cadrs stand-in: paste a document path or library ID.
- ER1.4 **(new)** Search also works **inside** a document: filter the Part Studios / Assemblies
  lists by object name or property.
- ER1.5 A linked source must have at least one version (DV1.8). The rationale the course gives:
  referencing a frozen version means edits in the source's workspace never break consumers.
- ER1.6 Once a document is opened in the Other documents tab, the header shows its name with the
  referenced version under it (e.g. "↳ V1"), a back arrow (**<**) to the document list, and two
  small icon buttons to the right: **Create version** (a "branch-plus"-style icon, creates a
  version on the fly: DV1.8) and **Version graph** (a small "branch" icon). Below are **Part
  Studios | Assemblies** sub-tabs with a search box, then the items with thumbnails.
- ER1.7 **Default = newest version.** The dialog assumes the latest version. Clicking the
  **version graph** icon swaps the list for the document's version graph so an older version can
  be picked; the course's example is a drawing that must show an older released revision.
  (DV2.1)
- ER1.8 After picking the version, insertion works exactly as for current-document items (click
  to insert, place, accept; footer shows **Inserted: n** and "Undo to remove instances"). (A2.3)
- ER1.9 A linked part or subassembly shows a **version icon** at the right of its row in the
  Instances list (a small dark "pin on a stem" / version-marker glyph) marking it as coming from
  another document. (DV1.3, DV2.2)
- ER1.10 Deleting the source document, or losing access to it, doesn't break the consumer; it
  keeps working from the stored version. (DV1.7, DV1.11)

### ER2 Updating linked documents (video, 2:14)
- ER2.1 Linked references **never auto-update**; the consumer only changes when the user updates.
  (DV1.4, DV1.11)
- ER2.2 An update needs a **new version** in the source first (toolbar **Create version** →
  name → **Create**). Workspace edits alone never show up downstream.
- ER2.3 **Blue update badge:** once a newer version exists, the version icon next to each linked
  instance gets a **blue circular badge** ("update available"). It's a notice, not an error, and
  updating is optional. (DV1.3) **(new)** The same blue badge also appears on the **tab** (bottom
  tab bar, left of the tab name) of any tab that contains out-of-date links (see
  `ex1-step15.png`, `ex1-step16.png`).
- ER2.4 **(new)** **Right-click an instance → "Update linked document…"** opens the Reference
  manager for that instance.
- ER2.5 **(new)** **Multi-select** (Ctrl- or Shift-click several linked instances) then right-click
  → **Update linked document…** updates them together.
- ER2.6 **(new)** **Right-click a tab** at the bottom → **Update linked document…** updates every
  linked reference in that tab.
- ER2.7 **(new)** To go to a specific (older) version instead of the latest, choose **Selective
  update** in the Reference manager, then the version graph icon. (DV1.10)

### ER3 Referencing versions within a document (video, 2:21)
- ER3.1 An assembly (or drawing, or parent assembly) can reference a tab in its **own** document
  either at the **workspace** (live, updates immediately) or at a **version** (frozen). (DV1.1)
- ER3.2 **(new)** In the Current document tab of Insert, the **Version graph** icon (top right of
  the document header) opens a picker with a legend: **Workspace** (hollow circle), **Version**
  (filled marker), **Release candidate** and **Release** (triangle markers), followed by a vertical
  graph of the branch: **Main** (workspace, at the top), then versions (**V1**, …) down to
  **Start**. Selecting a node sets what the dialog inserts from. The **Create version** icon sits
  next to it for creating one on the fly. (See `lesson-referencing-versions-within-a-document.png`.)
  cadrs: release markers are out of scope (no release management; T7.4), but the legend can show
  just Workspace and Version.
- ER3.3 **(new)** **Right-click a same-document instance → "Change to version…"** opens the
  Reference manager to switch that instance from workspace to a version:
  - **Update to latest** + **Update all** → the newest version;
  - **Selective update** → version graph icon → pick a version → **Update selected**.
- ER3.4 After the switch, the instance shows the version icon, edits to the source tab no longer
  show up, and the blue badge appears when a newer version exists: the same behaviour as a
  cross-document link. (DV1.11)
- ER3.5 **(new)** The same workspace↔version choice applies to parts in assemblies, subassemblies
  in parent assemblies, and parts/assemblies in drawings (drawings detail is in the Drawings
  course, D*).
- ER3.6 **(new, implied)** The reverse (version → workspace, same document only) is also
  available from the Reference manager; the course doesn't show the menu entry. Treat
  "Change to workspace" as a cadrs requirement for symmetry.

### ER4 Update all references (video, 2:11)
- ER4.1 **(new)** **Nested reference chains.** Part in document A, used (at a version) by a
  subassembly in document B, used by a top-level assembly in document C. After A gets a new
  version, the chain is stale at two levels.
- ER4.2 **(new)** **Transitive-update indicator.** The top-level document shows a blue version
  icon with an **arrow** variant, meaning an update exists somewhere down the reference chain,
  not only in the direct source.
- ER4.3 **(new)** **"Update all references to latest versions"** toolbar button, at the **top left
  of the Assembly toolbar** (the circular-arrow icon right of undo/redo; see
  `lesson-update-all-references.png`). It opens the **Reference manager** with every referenced
  document pre-selected; **Update all** then updates the whole chain.
- ER4.4 **(new)** **Reference manager contents:** title **Reference manager** with a close ✕;
  two tabs **Update to latest | Selective update**; a collapsible group header with an info icon
  reading like "Newer versions available for N document(s)"; one row per source document with a
  thumbnail, the document name and **"V1 ⇒ V2"** (current ⇒ target; "V2 ⇒ new version" when a
  version will be auto-created); a blue **Update all** button at the bottom right; a help "?"
  icon. (Extends DV1.10.)
- ER4.5 **(new)** **Auto versions.** Updating through the chain **automatically creates a new
  version** in each intermediate document (B here) so that document's version can point at A's
  new version; C then references B's new version.
- ER4.6 **(new)** After the update, a toast says the instances were updated, with a **show more
  details** link.
- ER4.7 **(new)** Auto-created versions get a distinct **auto-version icon** in the intermediate
  document's **History** and **Version graph**.
- ER4.8 **(new)** Auto-version creation **can't be undone**. Undo can re-point references to an
  older version but the created version stays. cadrs: warn about this in the Reference manager
  before an Update all that will create versions.

### ER5 Pinning references (video, 2:53)
- ER5.1 **(new)** **Right-click an instance that references a version → "Pin reference".** The
  version icon gains a **thumbtack** (see `lesson-pinning-references.png`).
- ER5.2 **(new)** A pinned reference with a newer version available changes its icon but **not to
  blue**: no "update available" alert.
- ER5.3 **(new)** Update-all actions (ER4.3, ER2.6) **skip** pinned references; the Reference
  manager lists everything else.
- ER5.4 **(new)** To update one pinned reference, click its version icon to open the Reference
  manager, switch to **Selective update**, and update.
- ER5.5 **(new)** To update several pinned references (or the whole assembly), click the update
  icon at the top of the graphics area, switch to **Selective update**, tick the **checkbox** next
  to each instance to update, then update. So Selective update has per-row checkboxes.
- ER5.6 **(new)** **Drawings can pin** too. The drawing must reference a version. In the
  **Sheets** pane, expand the sheet, right-click the reference → **Pin reference**. To update:
  right-click the **drawing tab** → **Update linked document…** → **Selective update**.
- ER5.7 **(new)** **Unpin:** right-click the thumbtack → **Unpin reference**. It becomes an ordinary
  link again, and the badge goes blue if a newer version exists.
- ER5.8 **(new)** Pinning applies only to version references (workspace references can't be
  pinned; they always follow the live tab).

### ER6 Exercise: Using Linked Documents (Linked Document Piston Exercise)
Goals: insert a subassembly from another document, mate it, copy/paste instances, then change the
source and update the link.

**Starting documents:** two public documents, "Exercise: Linked Document- Piston" (tabs
**Pneumatic Piston** Part Studio with 8 parts: Cylinder, Rod, Pin, UJoint, Eye, Ball Joint Bearing,
Ball Joint Sleeve, Ball Joint Pin; and **Piston Assembly** with 8 mates: Fastened ×5, Revolute,
Ball, Cylindrical) and "Exercise: Linked Documents- Project" (tabs **Hexapod** assembly,
**Topplate**, **Baseplate**). **Not buildable from scratch** without recreating both: the piston
has ~76 features and exact sketch dimensions not given in the course. cadrs needs a sample stand-in
(two library documents: a piston with a Ø15.725 cylinder and a 28 rod-length sketch dimension,
and a hexapod with two plates and six holes on each).

Steps (images `ex1-step<k>.png`):
1. Copy the Piston document (Document menu → **Copy workspace…**).
2. **Create version** (toolbar icon beside the document name) → dialog "Create version from
   **Main**" with **Name** (defaults to V1), **Description** (max 10000 characters), a note that
   multi-branch merges only apply to Part Studio/Assembly tabs, buttons **Create**, **Create
   version and edit properties**, **Cancel**. Name it **V1**.
3. Copy the Project (Hexapod) document; it has only the two plates.
4. Hexapod assembly → **Insert** → **Other documents** → paste the Piston document URL in the
   search → pick the document → **Assemblies** sub-tab → **Piston Assembly**. Insert one instance.
5. The instance row **Piston Assembly <1>** shows the version icon.
6. **Revolute** mate between a mate connector of the Baseplate hole and one on the UJoint (Top
   plate hidden for clarity).
7. Right-click the instance → **Copy**; right-click in the graphics area → **Paste Piston Assembly
   <1>** (or Ctrl+C / Ctrl+V). Repeat until there are 6 instances. The single-instance context
   menu shown: Properties…, Hide, Hide other instances, Hide all instances, Isolate…, Make
   transparent…, Suppress, Use best available tessellation, Check interference…, Show mates,
   Hide mates, Replace instances…, Copy, Move to new subassembly, **Update linked document…**,
   **Open linked document**, **Pin reference**, Export…, Revision history…, Add comment, Zoom to
   selection, Create Drawing of Piston Assembly…, Create new subassembly, Delete. The graphics-area
   menu: Show all, Show all instances, Paste …, Create Drawing of Hexapod…, Zoom to fit, Isometric.
8. Revolute-mate the other five cylinders to the other baseplate holes, using matching connector
   points.
9. **Fastened** mate between a Ball Joint Pin connector and a Topplate hole. Tip: drag with the
   triad first so the solver picks the intended solution.
10. Fasten the other five. The piston's internal mates still solve inside the top-level assembly
    (all levels are solved together).
11. Back in the Piston document, open the **Pneumatic Piston** Part Studio.
12. Edit **Main Sketch** (on the Right plane) and change the cylinder diameter **Ø15.725 → Ø20
    mm**. Accept.
13. Edit **Rod Sketch** (on the Front plane) and change the rod length dimension **28 → 75 mm**.
    Accept.
14. Create version **V2**.
15. In the Hexapod document, all six piston rows show the **blue badge** (and the Hexapod tab
    too), but the geometry is unchanged.
16. Select all six → right-click → **Update linked document…** → Reference manager, **Update to
    latest** tab, row "Linked Document- Piston V1 ⇒ V2" → **Update all** → close. (The multi-select
    menu is a shorter list: Hide…, Isolate…, Suppress, Check interference…, Show/Hide mates,
    Replace instances…, Copy, Move to new subassembly, Update linked document…, Pin reference, Zoom
    to selection, Create new subassembly, Clear selection, Delete.)
17. Geometry updates, badges go back to the plain black icon. To go back to V1: Update linked
    document… → **Selective update** → version graph → V1.
18. Select all instances → **Mass properties** (bottom-right icon of the graphics area).

**Self-check:** the total mass of the Hexapod assembly (answer hidden in the screenshot). Visible
values in `ex1-step18.png` (with all 8 instances selected): volume **1.180e+6 mm³**, surface area
**293022.474 mm²**, center of mass **(1.804, −9.526, 86.597) mm**, inertia (kg·mm²) Lxx
158709.178, Lyy 157168.304, Lzz 117188.105, Lxy 27.958, Lxz −3561.736, Lyz 12405.818. The
dialog lists "Instances to measure", "Mate connector for reference frame", "Show calculation
variance", Mass with an **Override** checkbox, volume, surface area, center of mass (with
Override), and the inertia matrix (with Override).

## 2. Moving Elements

### ER7 Move to Document (video, 2:50)
- ER7.1 **Right-click a tab → "Move to document…"** opens the **Move to document** dialog. The tab
  menu shown: Open in new browser tab, Rename…, Properties…, Create Drawing of …, **Move to
  document…**, Export…, Delete. (DV4.1, T1.3)
- ER7.2 **(new)** Dialog tabs **New document | Other documents**.
  - **New document:** a **Document name** field (defaults to the moved tab's name, e.g.
    "Pneumatic Piston").
  - **Other documents:** browse/search as in ER1.2; after picking a target, the dialog requires a
    **version** of the target to be created as part of the move.
- ER7.3 **(new)** **Referenced tabs move too.** A summary line "**N referenced tabs will be
  moved**" with a **Show details / Hide details** toggle lists them (e.g. Piston Assembly +
  Pneumatic Piston). Moving an assembly that references a Part Studio's workspace takes the Part
  Studio with it. The key takeaway says the user gets to choose whether to move referenced tabs;
  the dialog as shown just lists them, so cadrs should offer a per-tab choice in the details list,
  and any referenced tab left behind becomes a cross-document link the other way.
- ER7.4 Buttons **Move** (primary) and **Cancel**; a status line such as "Moving 2 tabs to
  Pneumatic Piston".
- ER7.5 After the move, every use in the original document is **rewritten to a linked
  reference** to a **version** of the target document (automatically created), shown with the
  version icon. No manual re-referencing. (DV4.1)
- ER7.6 **(new)** **Moving a tab that others reference at a version** (same-document version
  reference, ER3): the consumer keeps pointing at the version in the **original** document. The
  Reference manager (click the version icon) then offers **Update to the new document**; after
  that, new versions in the new document produce the blue badge.
- ER7.7 **(new)** **Multi-tab move:** the **Tab manager** (bottom-left icon of the tab bar)
  allows Ctrl/Shift multi-select of tabs and moving them all at once.

### ER8 Exercise: Moving Elements to Another Document
Goal: split the piston out of an all-in-one Hexapod document.

**Starting document:** public "Exercise: Move to Document" with tabs **Hexapod** (assembly, 6
piston instances, 13 mates), **Pneumatic Piston** (Part Studio), **Piston Assembly**, **Topplate**,
**Baseplate**. **Not buildable from scratch** (same piston as ER6); a cadrs sample can reuse the
ER6 stand-in merged into one document.

Steps (images `ex2-step<k>.png`):
1. Copy the document (**Copy workspace…**).
2. Right-click the **Piston Assembly** tab → **Move to document…**.
3. **New document**, name **Pneumatic Piston**, note "2 referenced tabs will be moved" → **Move**.
4. The Hexapod's six piston rows now show the black version icon. The Piston Assembly and
   Pneumatic Piston tabs are gone; only Hexapod, Topplate, Baseplate remain.
5. Documents page → **Created by me**: the new **Pneumatic Piston** document is listed (with a
   details panel: owner, description, labels, sharing, created/modified).
6. Open it: it has the **Pneumatic Piston** and **Piston Assembly** tabs.
7. In Piston Assembly select all instances → Mass properties.

**Self-check:** the piston assembly mass (hidden). Visible values in `ex2-step7.png` (the 8
instances selected): volume **31360.866 mm³**, surface area **15846.167 mm²**, center of mass
**(0.002, −3.153, 74.777) mm**, inertia (kg·mm²) Lxx 6.579e+2, Lyy 6.569e+2, Lzz 8.800e+0, Lxy
2.245e−2, Lxz −5.507e−2, Lyz 3.036e+1. (This is the original, un-edited piston, not the ER6 V2
geometry.)

## Cross-cutting requirements found in the course
- X1 **One Reference manager** dialog (ER4.4) serves every entry point: the version icon click,
  instance and tab right-click **Update linked document…**, **Change to version…**, the Update-all
  toolbar button and the moved-tab **Update to the new document** case. It has **Update to latest
  | Selective update**, per-row checkboxes and a version-graph picker. (DV1.10)
- X2 **Version icon states** in instance lists (and tabs, Feature list, drawing Sheets pane):
  plain black (up to date), blue badge (update available), blue with arrow (update down the
  chain), thumbtack (pinned; never blue). (DV1.3)
- X3 **Version graph picker** reused in Insert (both tabs), the Reference manager and Move to
  document, with **Create version** on the fly. (DV1.8)
- X4 **Reference flags:** `ExternalRef` needs `pinned: bool`, and the resolver needs a
  transitive "stale anywhere downstream" query plus an update that auto-creates versions in
  intermediate documents (ER4.5); these extend DV1.2/DV X1.
- X5 **Copy / paste of assembly instances** (Ctrl+C / Ctrl+V and right-click **Copy** /
  graphics-area **Paste <name>**) keeps the external reference of the copied instance.
- X6 **Mass properties over a multi-selection of instances**, including linked subassemblies,
  is the self-check for both exercises.
- X7 **Copy workspace…** (Document menu) is how the exercises start. cadrs: "Duplicate document"
  in the local library.
- X8 Out of scope: teams, sharing, labels filters and URL paste are cloud features. Keep local
  equivalents (DV1.12): library folders, a "created by me"/recent filter and pasting a library
  path or ID.
