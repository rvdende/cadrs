# Onshape "PCB Studio Fundamentals": requirements for cadrs

Source: Onshape Learning Center, topic **PCB Studios**
(`learn.onshape.com/learn/dashboard?labels=["Topic"]&values=["PCB Studios"]`), read on 2026-09-29.
The topic lists 10 items: the self-paced course **PCB Studio Fundamentals**
(`learn.onshape.com/learn/course/pcb-studio-fundamentals/...`) and 9 standalone videos. **All 9
videos are also lessons in the course** (same titles, same transcripts), so the course pages are the
only source used here and no "View Details" page had to be captured separately. The requirements
below are paraphrased, not the course text. Video poster frames and exercise screenshots are in
`pcb-studio/` (git-ignored, local only).

The course has 7 sections: 8 videos (key takeaways + transcript), 3 slide-based exercises (goal
page, step carousel, then a **Begin Self-Check** quiz page), one knowledge self-check quiz and a
completion survey. **No quiz, self-check or survey was started.** PCB Studio is a paid feature
(the course demos an Onshape Professional account) and needs an admin to configure it, so nothing
was tried in Onshape.

All three exercises start from a **public Onshape document** ("Make a copy to edit"). Exercises 2
and 3 depend on IDF files stored in that document (an "IDF Imports" folder), which we don't have.
cadrs has to author its own sample IDF files (see X4 at the end).

Each requirement has an ID (`PCB<lesson>.<n>`). Items already covered elsewhere are
cross-referenced, not repeated:
[intro-to-assemblies.md](intro-to-assemblies.md) (`A*`: insert A2, triad A3, Fix A3.7, Group A13,
BOM A20, subassemblies X9), [intro-to-part-studios.md](intro-to-part-studios.md) (`PS*`: Extrude
PS4, appearances PS9, materials PS10), [intro-to-sketching.md](intro-to-sketching.md) (`S*`: Use
S20, Area readout X2), [derived-and-linking.md](derived-and-linking.md) (`DV*`: version-pinned
cross-document references DV1, Open linked document DV1.9),
[external-references.md](external-references.md) (`ER*`) and [essential-tips.md](essential-tips.md)
(`T*`: versions T7, import X5).

Image index (`pcb-studio/`):
- `course-cover.png`: course cover (a single-board computer in a translucent case).
- `v1-introduction-poster.png`: an assembly with an enclosure, a board (Solar Tracker) and a
  Keep-in part.
- `v2-settings-select-folder-poster.png`: the settings dialog with the **Select a folder** picker.
- `v3-interface-poster.png`: the PCB Studio tab with two boards.
- `v4-import-idf-poster.png`: an imported board.
- `v5-translate-board-incontext-poster.png`: sketching a board in context.
- `v6-create-assembly-bom-poster.png`: a generated assembly with its BOM.
- `v7-modify-placement-poster.png`: moving components in the generated assembly.
- `v8-component-properties-poster.png`: the component view with its properties pane.
- `ex*-*.png`: exercise steps, named after the step.

## 1. Introduction to PCB Studio

### PCB1 Introduction to PCB Studio (video, 1:12; `v1-introduction-poster.png`)
- PCB1.1 PCB Studio is where mechanical and electrical engineers work on the **same board data in
  one place**. There's no separate dataset to hand over, and no copies to keep in sync.
- PCB1.2 PCB Studio is a **tab type (element) inside a document**, next to Part Studios,
  Assemblies and Drawings.
- PCB1.3 The course assumes the Onshape fundamentals (sketching, parts, assemblies) are already
  known.
- PCB1.4 Changing PCB Studio settings needs a **company administrator** (Professional or
  Enterprise). *Out of scope for cadrs:* there are no roles; settings are local (X6).

### PCB2 Accessing PCB Studio and Settings (video; `v2-settings-select-folder-poster.png`)
- PCB2.1 PCB Studio needs **two storage settings**:
  - **Component library document**: holds the ECAD component **mappings** (footprints). One
    library can serve many PCB Studios. When a mapping changes, the PCB Studios that use it can be
    updated.
  - **Component folder**: holds the **3D model documents**. Creating an assembly (PCB7) makes one
    document per component here.
- PCB2.2 The settings dialog (reached from the PCB Studio **gear** icon, PCB3.5) has one field
  per setting, each with a **browse icon** at the right end. The icon opens a **Select a folder**
  / document picker: a My Onshape tree of folders, then **Select** / **Cancel**. The dialog also
  has **Update** (applies the settings) and **Close**. From the poster frame:
  - the folder field's label reads roughly "Create new component documents in this folder", with
    a "Select a folder…" placeholder and a folder icon;
  - a hint says PCB Studio creates documents for the ECAD components in that folder, and that
    every user needs read/write access to it;
  - a build string ("Version 1.2.x") sits at the bottom.
- PCB2.3 Recommended setup: make a **new blank document** for the library and a **new folder** for
  the components, and name them clearly. The library document must contain an **empty PCB Studio
  tab**.
- PCB2.4 To use PCB Studio, a user needs **edit access** to the document with the PCB Studio, to
  the library document and to the component folder. The course recommends Teams (PCB13).
  *Permissions are out of scope.*
- PCB2.5 **Adding a PCB Studio**: the tab bar's **Insert new element** ("+") menu →
  **Create PCB Studio**. The new tab's default name is **"Onshape PCB Studio"** (seen in the
  exercise screenshots).
- PCB2.6 The PCB Studio **tab context menu** offers **Delete**, **Rename**, **Open in new tab**,
  **Properties** and **Move to document** (the same set as the other tab types; see T1.3 and DV4).

## 2. Navigating PCB Studio

### PCB3 PCB Studio Interface (video, 2:51; `v3-interface-poster.png`)
- PCB3.1 One PCB Studio can hold **several boards**. You can import, export and switch between
  them.
- PCB3.2 A **toolbar at the top left** (icon buttons, left to right):
  1. **Import ECAD files**: an upload icon, an arrow pointing up out of a tray (PCB4).
  2. **Export this board to IDF**: a download icon, an arrow pointing down into a tray (PCB9.7).
  3. **Sync a Part Studio or assembly with PCB Studio**: a document icon with an arrow (PCB5).
  4. **Create an assembly from this ECAD data**: an assembly or stack icon (PCB7).
  5. A **Search** field, then a **magnifying glass** button and an **✕** clear button.
  6. A **gear** icon: settings (admin only; PCB2).
  7. A **?** icon: opens the PCB Studio help in a new browser tab.

  Export and create-assembly are greyed out while no board is loaded (`ex1-step10-sync-dialog.png`).
- PCB3.3 **Search**: type a term and click the magnifier. Matching components or boards are
  highlighted in the viewport, with a "result *n* of *m*" counter. **Up/down arrows** step through
  the results, and **✕** clears the search.
- PCB3.4 **Left panel**, with two sections:
  - **Boards** (board icon): the boards imported into this PCB Studio. Clicking one shows it, and
    the active board is shown in bold and blue.
  - **Components**: every component in the configured library, **grouped per board** (one node
    per board).
- PCB3.5 Boards can come from **imported ECAD data, a Part Studio or an Assembly** (PCB4, PCB5).
- PCB3.6 Right-clicking a board → **Delete this board**. The Part Studio and Assembly exported
  from it earlier are **not** deleted.
- PCB3.7 **Viewport**: a 3D view of the board: a green board body, components as coloured boxes,
  and holes. Orientation comes from the **view cube** or the **Camera** pulldown (the same as in
  Part Studios). Empty-state hint text: "use the toolbar to import data from an ECAD file or
  Onshape" (paraphrased; `ex2-step4-import-ecad-dialog.png`).
- PCB3.8 The **right edge** has two panel toggles: **Component properties** (part name, part
  number, 3D representation; PCB11) and **Bill of Materials**.
- PCB3.9 The PCB Studio **BOM** lists the components with their **Designator** (reference
  designator), **Part name** and **Part number**. Selection **cross-highlights** between the BOM
  and the viewport in both directions.
- PCB3.10 **Double-clicking a reference designator** in the BOM edits it in place.

## 3. Importing and translating

### PCB4 Importing an IDF (video, 1:54; `v4-import-idf-poster.png`, `ex2-step4-import-ecad-dialog.png`)
- PCB4.1 **Supported import formats** named in the course:
  - **IDF** (Intermediate Data Format), the main workflow;
  - **IDX**;
  - **Eagle**.

  For the full list, the course points to the Help. No other ECAD vendor is named.
- PCB4.2 An IDF board is **two files**:
  - **`.emn`** (board file): the **board outline**, **component placement**, **holes**,
    **milling**, and **keep-in / keep-out regions**;
  - **`.emp`** (library file): each component's **outline and height**.
- PCB4.3 **Import ECAD files** dialog: a **Choose Files** file input ("No file chosen"), then
  **Import** / **Cancel**. The user picks the `.emn` and `.emp` together (multi-select) and clicks
  Import.
- PCB4.4 The imported board appears in the viewport and is added under **Boards**. Importing again
  adds another board. Switching and deleting work as in PCB3.4 and PCB3.6.
- PCB4.5 Clicking a component under **Components** switches the viewport to a **component view**:
  that component alone, on a grid, with its part name, part number and 3D representation. Clicking
  the board under Boards or Components goes back (`v8-component-properties-poster.png`).
- PCB4.6 To see a component's properties **without leaving the board view**, expand the Component
  properties pane and click a component in the viewport.
- PCB4.7 The **BOM** pane shows the active board's BOM, **grouped** (with quantities), with
  designator, part name and number. It cross-highlights (PCB3.9).

### PCB5 Translating a Board into PCB Studio (video, 3:13; `v5-translate-board-incontext-poster.png`, `ex1-step10-sync-dialog.png`)
- PCB5.1 A board can start as **MCAD geometry**. Parts in a Part Studio or Assembly are
  recognised **by part name**:
  - a **board** part's name contains **"board"** or **"PCB"** (for example "Mainboard");
  - a **keep-in** part is named **"Keep-in"** or **"Keepin"**;
  - a **keep-out** part is named **"Keep-out"** or **"Keepout"** (PCB9.4).

  The Help has the full list of names. Extra words are allowed (for example "Keep-out Battery").
  Whether matching ignores case isn't stated.
- PCB5.2 **Sync a Part Studio or assembly with PCB Studio** dialog (title in sentence case):
  - **Select part studio or assembly to use**: a dropdown of the other Part Studio and Assembly
    tabs in this document, labelled "<tab name> (Assembly)" and so on;
  - **Top face of board part is parallel to**: a plane dropdown (Top Plane, …), which sets the
    board's orientation;
  - **OK** / **Cancel**.
- PCB5.3 After OK, the board (and any keep-in/keep-out parts) shows in the viewport and under
  Boards (`ex1-step11-synced-board-keepouts.png`, where keep-outs are dark translucent patches on
  the green board).
- PCB5.4 **Parts that don't match the naming** (for example an enclosure) are **not translated**,
  and a message says so.
- PCB5.5 **Syncing an assembly** also picks up its **components**, meaning instances whose source is
  in the component folder, and their **positions**. This is how edits made in an assembly get back
  into PCB Studio (PCB9.5).
- PCB5.6 **Re-syncing** the same source **updates** the existing board (a new outline and new
  keep-outs; PCB6 steps 15–16). It doesn't add a duplicate.
- PCB5.7 **Workflow: design the board around an enclosure.** This uses *managed in-context
  design*:
  - in the assembly, **Create Part Studio in context** (the Display states ▾ menu in the assembly
    toolbar), picking the assembly origin;
  - sketch on the context geometry and **Use** (S20) the enclosure's edges;
  - Extrude, and rename the part so it contains "board" or "PCB";
  - add keep-in/keep-out parts;
  - **Insert and go to Assembly**, then mate or group;
  - create a PCB Studio and sync the assembly;
  - export to ECAD (PCB9.7).

  When the enclosure changes, **Update context** and a re-sync carry the change through with
  little effort. *Managed in-context design isn't covered by any existing cadrs reference; it's a
  prerequisite (X9).*

### PCB6 Exercise: Creating a PCB Board ("Board Exercise"; `ex1-*`)
Goals: create a board from scratch, add mounting holes and keep-out zones, and import it into PCB
Studio. The goal image (`ex1-goal-board.png`) shows a green rounded-rectangle board inside a phone
case. Start: the public document **Exercise: Board** with:
- an **Enclosure** Part Studio (variables **#Width = 70 mm** and **#Length = 140 mm**; parts
  Enclosure and Battery);
- a **Cellphone** assembly (Cell phone, Battery, Enclosure, and an existing Group 1).

The steps (18 slides):
1. Copy the document. PCB Studio must already be configured.
2. In the **Cell phone** assembly: **Create Part Studio in context** (under the toolbar's Display
   states ▾ menu). An "Origin of new Part Studio" dialog asks for the origin/mate connector; pick
   the assembly **Origin** and ✓ (`ex1-step2-part-studio-in-context.png`).
3. Sketch on the **top face of the battery**. **Use** the inside face of the case to project the
   board outline.
4. In the same sketch, **Use** the battery's 4 edges and the antenna array's 5 edges to make the
   keep-out outlines.
5. **Extrude** all three regions: Blind, **0.062 mm** (the slide says mm; a typical board is
   0.062 in, but the exported file confirms 0.062 was used as mm), New part, upward. Rename the
   part **Board**. Appearance and material are optional.
6. **Extrude** the battery and antenna regions **1 mm downward** as New parts. Rename them
   **Keep-out Antenna** and **Keep-out Battery**, and rename the context **Board & Keep out**. Tip:
   hide Board and show Sketch 1 to make picking easier.
7. **Insert and go to Assembly**: pick Board and both keep-outs, then ✓.
8. Edit the existing **Group** (A13) and add Board and both keep-outs.
9. **Insert new element → Create PCB Studio**.
10. **Sync a Part Studio or assembly**: Cell phone (Assembly), Top Plane, then OK.
11. The board and both keep-outs appear.
12. In **Enclosure**, set **Width = 85 mm** and **Length = 150 mm**.
13. In the assembly, right-click Board → **Update context** → *Board & Keep out*.
14. The board and keep-outs follow the resized case.
15. Back in PCB Studio, sync again with the same settings.
16. The board and keep-outs update to the new size.
17. **Export this board to IDF** → **IDF 3.0** → Export (`ex1-step17-export-ecad-dialog.png`).
18. Unzip the download and open **`Cell phone.emn`** in a text editor
    (`ex1-step18-exported-emn.png`). Then take the quiz, which **wasn't started**.

**Self-check:** it reads a **value from the exported `.emn`**. The screenshot masks one
board-outline line, so the quiz most likely asks for one coordinate of the outline. The visible
file content (useful as a format fixture):
- header: `BOARD_FILE 3.0 "Onshape PCB Studio v1.2.112.0" <date> 1`, then `"Cell phone" MM`;
- `.BOARD_OUTLINE MCAD` with thickness `0.0620000000000028`, then a single loop `0` of points
  `x y angle`:
  - (32.5, −73, 0), (40.5, −65, 90), (40.5, 65, 0), (32.5, 73, 90), [masked],
  - (−40.5, 65, 90), (−40.5, −65, 0), (−32.5, −73, 90), (32.5, −73, 0).

  That's an **81 × 146 mm** outline centred on the origin, with 8 mm, 90° corner arcs.
- **Empty** `.DRILLED_HOLES` and `.PLACEMENT` sections.
- The keep-outs aren't visible in the shown part of the file.

**Buildable from scratch:** mostly. The enclosure's full geometry isn't given (only
the two variables), but any parametric case with Width/Length variables, a battery and an antenna
block reproduces the workflow. The expected numbers depend on the case wall, so our check should be
our own: an outline size matching our case's inner size, and the thickness written as given.

### PCB7 Creating an Assembly from PCB Studio (video, 2:44; `v6-create-assembly-bom-poster.png`, `ex2-step5-create-assembly-dialog.png`)
- PCB7.1 **Create an assembly from this ECAD data** opens the **"Create assembly from
  '<board>'"** dialog:
  - "Select features to include in the assembly", with checkboxes **Board** (on by default),
    **Components** (on by default) and **Keep-In and Keep-Out Areas** (**off** by default);
  - a note that the build can take several minutes and that you can keep working meanwhile;
  - **OK** / **Cancel**.
- PCB7.2 The conversion creates, **in the same document**:
  - a **Part Studio** with the board and the keep-in/out parts. In exercise 3 its feature list is
    Sketch 1, a feature named "Board [secondary board]", and Extrude 1, and the part is named
    **"Board [<board name>]"** (`ex3-step6-keepout-sketch.png`);
  - an **Assembly** with the board, the keep areas and the components.

  Both tabs are named after the board (e.g. "Vision PCB", "secondary board").
- PCB7.3 For each **new** component, the conversion creates a **component document** in the
  component folder (PCB11.1). The assembly references a **version** of that document (a
  version-pinned external reference; DV1).
- PCB7.4 The generated components are **not mated** and move freely. Best practice: **Fix** the
  board (A3.7) and **Group** (A13) the components.
- PCB7.5 Instance names come from the ECAD package or footprint names ("0603-N…", "BTN_KM…",
  "uBGA48_7.4X7.1", "SOD123F/DSS1…"), with the usual `<n>` suffix.
- PCB7.6 The assembly **BOM** (A20) fills with one row per ECAD part number, with quantities and
  metadata: Item, Quantity, Part number, Description (for example "Resistor 113K OHM"). It
  cross-highlights with the Instances list and the viewport.
- PCB7.7 To show the PCB as **one BOM line** in a parent assembly: assembly Properties →
  **Subassembly BOM behavior** → **Show assembly only** (A20.5).
- PCB7.8 The PCB assembly is an ordinary assembly. It can be inserted as a subassembly (A2, X9), or
  used for in-context design, for example an enclosure built around it.
- PCB7.9 **One part for the whole PCB**: create a Part Studio in context, **Transform** the
  context snapshot with **copy in place**, then **Composite part** with **Closed** checked, and
  insert it. *Composite part is a new feature for cadrs (X9).*
- PCB7.10 Right-clicking a component → **Open linked document** (DV1.9). The component document
  opens in a new tab, where you can edit its geometry, appearance, material and properties.

### PCB8 Exercise: Creating a PCB Assembly from an IDF ("Vision Controller Exercise"; `ex2-*`)
Goals: import ECAD data and create an Onshape assembly. Start: the public document **Exercise:
Vision Controller**, which has an **IDF Imports** folder with the IDF tabs.
1. Copy the document. PCB Studio must be configured.
2. Open the **IDF Imports** folder, click **Download** on each tab to get `Vision PCB.emn` and
   `Vision PCB.emp`, then go back with the **All tabs** icon.
3. **Insert new element → Create PCB Studio**.
4. **Import ECAD files** → Choose Files → pick both files → Open → **Import**.
5. **Create an assembly from this ECAD data**, keep the defaults (Board and Components), then OK.
6. In the **Vision PCB** assembly: **Group**, box-select all the components, ✓, then **Fix** the
   board.
7. Switch to the **Top** view and select the marked component (A), a large central IC. Read the
   **Area** in the bottom-right corner (`ex2-step7-area-check.png`, value masked). Then take the
   quiz, which **wasn't started**.

**Self-check:** it measures the **area of the selected component's top face** (the status bar
readout; see S X2). The result has 29 instances, a rectangular board with 4 corner holes, two
long header strips, and two large square ICs.

**Buildable from scratch:** no. `Vision PCB.emn/.emp` aren't available. cadrs needs its own
equivalent IDF pair (X4); with that, the workflow and the area check can be reproduced.

## 4. Working with PCB data

### PCB9 Modifying Component Placement and Exporting (video, 2:56; `v7-modify-placement-poster.png`)
- PCB9.1 Why edit placement: interference with the enclosure, or areas that are off-limits for
  components.
- PCB9.2 Best practice again: **Fix** the board and **Group** the components (PCB7.4).
- PCB9.3 **Moving a component**:
  - take it out of the Group;
  - move it with the **triad manipulator** (A3), dragging an axis or typing a distance;
  - add it back to the Group.
- PCB9.4 **Keep areas**: a keep-in is where components *should* go, and a keep-out is where they
  must not. To create one:
  - sketch on the board in the generated Part Studio;
  - extrude it as a **New part**;
  - name it **Keep-in/Keepin** or **Keep-out/Keepout** (PCB5.1);
  - insert it into the assembly and place it.
- PCB9.5 The board itself can be edited with sketches and extrudes (adding or removing material).
  Create versions as needed and update the references in the assembly.
- PCB9.6 **Round trip back to PCB Studio**: sync the edited **assembly** (PCB5.2; the Top plane in
  the example). The board outline **and the component positions** update.
- PCB9.7 **Export this board to IDF** opens the **"Export ECAD files"** dialog:
  - "Write to IDF Version", with radios **IDF 2.0** and **IDF 3.0** (3.0 is shown selected in
    the screenshot; whether that's the default isn't stated);
  - **Export** / **Cancel**.

  **IDF 2.0 can't carry keep-in/keep-out areas; IDF 3.0 can.**
- PCB9.8 The export writes an **`.emn` and an `.emp`**, **zipped together** and downloaded. The
  zip unpacks to a folder. The `.emn` has:
  - the board outline;
  - component positions;
  - holes;
  - keep-in/keep-out areas.

  The `.emp` has component outlines and heights. The file base name is the board name ("Cell
  phone.emn", "secondary board.emn").
- PCB9.9 The electrical engineer imports these files into their ECAD tool and updates the
  electrical design. *The flow back to ECAD is file-based only;* the course names no live
  ECAD link.

### PCB10 Exercise: Importing and Modifying an IDF ("IDF Assembly Exercise"; `ex3-*`)
Goals: build an assembly from imported ECAD data, change the component placement, and write the
changes back to IDF. Start: the public document **Exercise: IDF Assembly** (with an IDF Imports
folder).
1. Copy the document.
2. Download both IDF tabs (the `secondary board` .emn/.emp).
3. Create a PCB Studio.
4. Import both files.
5. **Create an assembly**, keeping the defaults.
6. In the **secondary board** Part Studio, sketch on the **top face of the board** a
   **0.5 × 0.375 in** corner profile with an **R0.25** fillet at the board's bottom-left corner
   (`ex3-step6-keepout-sketch.png`).
7. **Extrude**: Blind, **0.125 in**, New part. Rename it **Keep-out**.
8. In the **secondary board** assembly: **Insert** the Keep-out part and click ✓ inside the
   dialog, so it keeps its Part Studio position (A2.3).
9. In the Top view, select the **interfering component** (it sits over the new keep-out). Drag the
   triad's **Y axis 1 in** (`ex3-step9-triad-move.png`, which also shows an Area readout of
   0.084 in² for the selection).
10. **Group** all the components (box select), ✓, then **Fix** the board.
11. In PCB Studio, **sync** the assembly with the same parameters as before.
12. PCB Studio shows the moved component and the keep-out.
13. **Export this board to IDF** → **IDF 3.0**, which supports keep-outs → Export.
14. Unzip, open `secondary board.emn`, and search for **`uBGA48_7.4X7.1`**, the moved
    component (`ex3-step14-exported-emn-placement.png`). Then take the quiz, which **wasn't
    started**.

**Self-check:** it reads the **new placement coordinate** of `uBGA48_7.4X7.1` in the exported
`.emn` (the Y value is masked; X = 4.064182376174947, rotation 90, TOP, PLACED). Visible file
content (a useful fixture):
- the header, as in PCB6;
- a `.BOARD_OUTLINE MCAD` with thickness **0.84**, outline (−5.20972, −22.74338) →
  (45.59028, 15.35662), which is **50.8 × 38.1 mm (2 × 1.5 in)**;
- empty `.DRILLED_HOLES`;
- `.PLACEMENT` records as two lines each, `package part_number refdes` then
  `x y mount_offset rotation side status`, for example `BUTTON_EVQPUA02 5209001 X0` /
  `24.47 −9.48 0 270 TOP PLACED` and `CRYSTAL_CX_4V 4510219 X1` / `32.37 3.10 ~0 270 TOP PLACED`.

  Other packages: `1210_SR73K2E`, `TSSOP_20`, `1206C`. Reference designators are `X<n>`.

  Values are written at full float precision, and near-zero values come out as noise
  (`-5.55e-15`).

**Buildable from scratch:** no, because the source IDF isn't available. We can author a 2 × 1.5 in
board file with similar packages and reproduce every step. The check becomes: the exported Y of the
moved part equals the original Y + 25.4 mm (in the board frame, allowing for the Top-plane
mapping), and a `.PLACE_KEEPOUT` (or equivalent) record exists for the new keep-out.

## 5. Library components

### PCB11 Electronic Component and Properties (video, 3:34; `v8-component-properties-poster.png`)
- PCB11.1 **Component folder**: one **document per component**, created automatically when an
  assembly is created. Each document has **one Part Studio with a single part** (the generic
  shape made from the ECAD footprint outline and height) and an **empty assembly**.
- PCB11.2 You can edit a component's Part Studio (cut-outs, appearance, metadata; PS9, PS10). Then
  **create a version** so the change reaches other boards, and **update the assemblies** that use
  it (ER4, DV1.10).
- PCB11.3 Component documents can be **moved to other folders** (for example, grouped per PCB).
  References and versions stay tracked.
- PCB11.4 The **component library document** is created and maintained **automatically** by PCB
  Studio. It stores the **mappings**, meaning footprint → 3D representation, and shares them across
  PCB Studios. When a mapping changes, the components that use it update.
- PCB11.5 **Component pane**, headed "Component":
  - **Part name** (for example "AMPHENOL_1140084168");
  - **Part number** (the demo shows "Comment");
  - **Representation**, a radio group:
    - **None**: the component isn't shown in PCB Studio;
    - **From ECAD data** (the default): a generic box built from the ECAD outline and height;
      there's a small open-link icon next to it;
    - **Custom part**.
- PCB11.6 **Custom part**:
  - click **Select custom part**, browse to the component's document, pick a **version**, then ✓;
  - if the model doesn't line up with the footprint, use **Translate** (X/Y/Z, by manipulator
    drag or typed values; **Center** puts it at 0, 0, 0) and **Rotate** (manipulator or value);
  - click ✓, then **Components** to go back.
- PCB11.7 There are **two ways in** from the PCB Studio:
  - (a) select a component in the viewport and change it in the expanded Components pane. You
    must accept the replacement before translating or rotating. Collapse the pane and deselect
    when done.
  - (b) click the board under **Components**, then a component, to open the component view
    (PCB4.5).
- PCB11.8 The component view shows the part on a **grid floor** with an iso view cube. The generic
  box is dark red on top with bright red sides.

### PCB12 Self-Check: Understanding the Library (quiz)
- A knowledge quiz about the library and components. **Not started.**

## 6. Data management

### PCB13 Sharing PCB Studio Data (video, 1:49)
*All of this is cloud permission behaviour and out of scope for a local cadrs. It's recorded so a
future collaboration layer knows the rules.*
- PCB13.1 **Edit** in PCB Studio needs Edit on all three: the document, the library document and
  the component folder.
- PCB13.2 A **View-only** user of the document **can't see the PCB Studio tab** unless they also
  have View on the library and the folder.
- PCB13.3 With **Edit on the document** but **View on the library/folder**, a user can **import
  and export**, but not **sync**, **create assemblies**, or set a custom representation from the
  component folder.
- PCB13.4 **Only one person can view a PCB Studio at a time**, however they are shared in. This is
  a single-user lock, unlike the rest of Onshape.
- PCB13.5 Best practice: share the library document and component folder with **Teams**, so
  permissions follow team membership.
- PCB13.6 View-only sharing suits suppliers and the shop floor.
- The poster frame shows the folder **Share** dialog. It wasn't saved, because it shows other
  people's e-mail addresses.

## 7. Completion
- A **Completion Survey** unlocks the certificate. **Not started.**

## Knowledge checks and survey
- There are three exercise self-checks (page 3 of each exercise: **Begin Self-Check**):
  - PCB6: a value from the exported `.emn`;
  - PCB8: the area of a component;
  - PCB10: the moved component's coordinate in the exported `.emn`.
- One knowledge self-check (PCB12) and the Completion Survey. **None were started.**

## Cross-cutting requirements found in the course
- X1 **PCB Studio element**: a new tab type ("Onshape PCB Studio") with its own toolbar (PCB3.2),
  a Boards/Components tree, a 3D viewer (reusing the view cube and camera), right-side
  Component and BOM panes, search with result stepping, and a component-view mode. It holds **many
  boards**. All mutations (import, sync, delete board, representation change, refdes edit) go
  through the command/undo layer.
- X2 **Board data model** (in `cadrs_core`, no bevy):
  - `Board { name, units, thickness, outline: Vec<Loop>, holes, keepouts/keepins (placement,
    route, via; with side and height), components: Vec<Placement> }`;
  - `Placement { package, part_number, refdes, x, y, mount_offset, rotation_deg, side: Top|Bottom,
    status: Placed|Unplaced|Mcad|Ecad }`;
  - `Package { name, part_number, units, height, outline }`.

  An outline loop is a list of `(x, y, sweep_angle)` points: an angle of 0 means a straight
  segment, and anything else is an arc with that included angle (IDF convention). A 360° pair is a
  circle. The first loop is the outline, and later loops are cut-outs.
- X3 **IDF parser and writer** (a new crate, or a module in `cadrs_core`):
  - read **IDF 2.0 and 3.0**: `.emn` sections HEADER, BOARD_OUTLINE, (PANEL_OUTLINE),
    OTHER_OUTLINE, ROUTE_OUTLINE, PLACE_OUTLINE, ROUTE_KEEPOUT, PLACE_KEEPOUT, VIA_KEEPOUT,
    PLACE_REGION (keep-in), DRILLED_HOLES, NOTES, PLACEMENT; `.emp` sections ELECTRICAL and
    MECHANICAL;
  - write **IDF 2.0** (no keep areas; PCB9.7) and **3.0**, in the header form seen in PCB6/PCB10;
  - MM and THOU units;
  - the output is zipped as `<board>.emn` + `<board>.emp`.

  The section list comes from the public IDF 3.0 spec, not the course. Check the version-2
  keep-out claim against the spec.
- X4 **Sample IDF fixtures we author ourselves** (under `assets/` or test data):
  - (a) a "cell phone" board, 81 × 146 mm with R8 corners, no parts (matching PCB6's output);
  - (b) a 2 × 1.5 in "secondary board" with about 20 placements using package names like
    BUTTON_EVQPUA02, CRYSTAL_CX_4V, uBGA48_7.4X7.1, TSSOP_20, 1206C, SOT23, plus a `.emp` with
    box outlines and heights;
  - (c) a "vision controller" board with 4 corner holes, headers and about 29 parts.

  Round-trip tests: parse → write → parse must give the same data.
- X5 **Geometry mapping** (IDF → B-rep):
  - board = the outline loops extruded by the thickness (cut-outs and drilled holes subtracted);
  - keep-out/keep-in = the region extruded by its height (or a thin marker if there's none), shown
    dark and translucent;
  - component = its `.emp` outline extruded by its height, placed at (x, y) with rotation, and
    flipped for Bottom;
  - colours by class: green board, coloured bodies for components.

  The reverse (MCAD → IDF, PCB5):
  - find parts by name (board/PCB, keep-in/keepin, keep-out/keepout);
  - project them onto the chosen "top face parallel to" plane to get the outline loops (lines and
    arcs), thickness and keep regions;
  - read component instance transforms back as placements.
- X6 **Settings and storage (local)**: replace the admin-only cloud settings with a per-workspace
  **PCB settings** dialog. It has a **component library** location (a document or file holding
  the mappings: package → representation None / FromEcad / Custom(part ref, version, transform))
  and a **component folder** (a directory for the generated component documents), with Update
  and Close.
- X7 **Create assembly** (PCB7):
  - generate a Part Studio (board + keep parts as features) and an Assembly;
  - generate one component document per new package in the component folder, and version it;
  - insert them as **version-pinned external references** (DV1, ER);
  - build the instances with no mates;
  - options Board / Components / Keep areas;
  - it may be slow, so it needs a progress indicator and must stay non-blocking.

  It reuses the assembly (A*), BOM (A20), Fix/Group (A3.7, A13) and triad (A3) requirements.
- X8 **Sync back**: re-syncing from a Part Studio/Assembly **updates** the existing board in place
  (outline, keep areas, placements). Then export IDF. This is how placement changes flow back to
  ECAD: as files only, with no live link.
- X9 **Prerequisites not yet in any cadrs reference**:
  - **managed in-context design** (Create Part Studio in context, context snapshot, Update
    context, Insert and go to Assembly);
  - **Transform → copy in place**;
  - **Composite part** (Closed).

  PCB6 depends on in-context design. Without it, PCB6 can be done by modelling the board in the
  enclosure's Part Studio.
- X10 **Custom representation**: a component-view editor that swaps in a part from another
  document or version, with a translate/rotate manipulator and a Center button (PCB11.6), stored
  in the library mapping (X6).
- X11 **Out of scope**: admin roles, sharing and Teams (PCB13), the single-viewer lock (PCB13.4),
  and cloud "Download" of document tabs. **IDX** (ProSTEP EDMD XML) and **Eagle** (`.brd` XML)
  import are named but can come later, after IDF.
- X12 **Rough size**: this is a large, mostly new subsystem:
  - the IDF parser/writer and fixtures (medium);
  - the board, keep and component B-rep generation (medium);
  - the PCB Studio element UI (large);
  - MCAD → board recognition and projection (medium);
  - create-assembly generation with external component documents (large, and it depends on
    DV/ER);
  - export and re-sync (small once the rest exists).
