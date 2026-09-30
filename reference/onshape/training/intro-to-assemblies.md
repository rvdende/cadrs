# Onshape "Introduction to Onshape Assemblies": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Introduction to Onshape Assemblies
(`learn.onshape.com/learn/course/introduction-to-onshape-assemblies/...`), read on 2026-09-25. The
requirements below are paraphrased, not the course text. Exercise step screenshots and a few UI
captures are in `intro-to-assemblies/` (git-ignored, local only).

The course has 24 lessons in four sections, plus a completion survey: one interactive UI tour, one
expandable "mate reference sheet", 18 short videos (key takeaways + transcript), and 4 exercises.
Each exercise ends with a "Check to see if your assembly is correct → **Begin Self-Check**" quiz
page (page 3 of the exercise). The quizzes and the closing survey were **not** started, because
that would record attempts on the user's account. Every self-check asks for the **center of mass
(in)** of an assembly. The course screenshots blank the X/Y/Z values in red, but the other
mass-property values are visible, and they are recorded below so we can check our result.

**All four exercises start from a public Onshape document** ("Make a copy to edit") with
ready-made parts. None of them gives part dimensions, so **none can be rebuilt from scratch**
exactly. What cadrs can reproduce is the workflow, and, with equivalent parts, the structure of
the result (instances, mates, subassemblies, folders).

Each requirement has an ID (`A<lesson>.<n>`) so milestones and judges can refer to it. Items that
are already covered by [intro-to-part-studios.md](intro-to-part-studios.md) (`PS*`, e.g. mass
properties X7, units X8, mate connectors as references X11),
[intro-to-sketching.md](intro-to-sketching.md) (`S*`) and
[intro-to-parametric-cad.md](intro-to-parametric-cad.md) (`P*`, assemblies with motion X4) are
cross-referenced, not repeated.

## 1. Creating an assembly

### A1 Assembly interface (interactive tour; image `lesson-assembly-interface.png`)
The tour highlights these areas of the Assembly tab (shown on a "Leaf Blower" example):
- A1.1 **Insert** button (left end of the toolbar, after Undo/Redo): inserts instances of parts,
  assemblies, sketches and surfaces into the active assembly. See A2.
- A1.2 **Mate connector** tool in the toolbar. A mate connector is a local coordinate system placed
  on or between entities. Mates use them to position and orient instances relative to each other.
  They can also be used to create planes.
- A1.3 **Mate toolbar**: the standard mates (Fastened, Revolute, Slider, Cylindrical, Pin slot,
  Planar, Ball, Parallel) plus **Tangent** and **Width**. Also in the toolbar: **Group**, and
  gear/relation tools.
- A1.4 **Relations** tools (a toolbar group): they constrain the degrees of freedom *between mates*
  (gears, rack and pinion, screw, and so on). They are only named here; the course doesn't teach
  them.
- A1.5 **Filter & search** field at the top of the assembly list ("Filter by name"). It takes
  partial names, colon-prefixed special commands, or a filter picked from the funnel icon (the
  same scheme as the Part Studio feature-list filter, PS3).
- A1.6 **Instances list** (left panel, "Instances (n)"): the top-level assembly row, the
  **Origin**, and every part and subassembly instance. Instance names carry an index, e.g.
  `Muffler Assembly <1>`.
- A1.7 **Mate features list** ("Mate Features (n)"), in the same panel below the instances: every
  mate in the assembly. Between the two lists are **Items (n)** and **Loads (n)** groups (not
  covered by the course).
- A1.8 **Assembly panels** (right-edge icon strip): Bill of Materials, Configurations, Exploded
  views, Named positions, Simulation, Variable table.
- A1.9 The bottom-right icons are the same Measure, Analysis and **Mass properties** tools as in
  the Part Studio (PS2.11).

### A2 Starting an assembly (video)
- A2.1 An assembly defines the **structure** of the inserted part and subassembly instances, and
  their **degrees of freedom and relationships**.
- A2.2 A new document contains one empty Part Studio tab and one empty **Assembly** tab. More
  assembly tabs are added with the tab bar's **"+" (Insert new tab) → Create Assembly**. One
  document can have several assembly tabs.
- A2.3 **Insert** opens the **"Insert parts and assemblies"** dialog (image `ex1-step3.png`):
  - Source tabs: **Current document**, **Other documents**, **Standard content** (see A19).
  - The document name and branch ("Main"), with two icon buttons (insert rigid, and version
    picker).
  - Sub-tabs **Part Studios** and **Assemblies**, a **Search** field that accepts partial words and
    even non-alphanumeric characters, and, on the Part Studios tab, filter icons for the element
    types of a studio (parts, sketches, surfaces, curves).
  - A tree of each Part Studio with a thumbnail per part. A row for the Part Studio itself inserts
    all of its parts.
  - Footer: **Undo to remove instances**, and an **"Inserted: n"** counter. The ✓ / ✗ buttons are
    at the top.
- A2.4 **Insert the Part Studio as rigid** (icon): all its parts come in as one rigid unit and
  can't move relative to each other without mates. A rigid Part Studio instance has an **Edit**
  context-menu item to add or remove parts, sketches and surfaces, or to change its configuration.
- A2.5 If a part or assembly has **configurations**, their options are set in the dialog before
  the item is picked.
- A2.6 **Placement rule**: clicking an item inserts it.
  - With the cursor **inside the dialog**, the instance lands at the **same position and
    orientation relative to the origin** as in its Part Studio.
  - With the cursor moved **outside the dialog**, the instance follows the cursor, and a click in
    the graphics area drops it there.
  - Clicking several times inserts several instances (e.g. 4× O-Ring in one session). Placed
    instances aren't locked in position.

### A3 Positioning assembly instances (video; images `lesson-triad-context-menu.png`, `ex1-step4..8.png`)
- A3.1 Clicking an instance (a face or edge of it) shows the **triad manipulator** at the clicked
  spot, aligned to the picked edge or face.
- A3.2 **Relocate the triad** (without moving the part): drag its **origin** (the centre circle).
  While dragging, mate connector points appear and the triad snaps to them. Holding **Shift** locks
  the current reference entity so the snap doesn't jump. Release to drop it.
- A3.3 Right-click the triad origin → **Move to origin**. This moves the *instance* so that the
  triad origin sits on the **assembly origin**. The instance isn't locked there. (The same menu
  also has the whole instance context menu: Hide, Fix, Section view, Zoom, and so on.)
- A3.4 Triad handles:
  - Drag a **plane indicator** (the small square between two arrows) to move within that plane.
  - Drag an **arrow** to move along that axis. A value field appears for an exact distance.
  - Right-click an arrow → **Align with Z** / **Anti-align with Z**, which rotates the instance so
    that axis points along or against the assembly Z.
  - Drag an **end circle/ring** to rotate about X, Y or Z, with a value field for an exact angle.
    The angle handle's menu has **rotate 90°** and **rotate 180°**.
- A3.5 The assembly's **view cube** (Front/Back/Left/Right/Top/Bottom) defines the orientation that
  drawings use later, so the base part should be oriented deliberately.
- A3.6 Often no manual positioning is needed: accept the insert with the cursor inside the dialog
  (A2.6), and the instances keep their Part Studio placement.
- A3.7 **Fix**: right-click an instance (graphics area or list) → **Fix**. A fixed instance can't
  be dragged, so the assembly doesn't drift while other instances are positioned. Fix only **one**
  base instance and mate everything else to it. **Fix doesn't carry over** when the assembly is
  inserted into a higher-level assembly. A fixed instance shows a fixed icon (hatched lines) in the
  list, and a movable one shows a small triad (DOF) icon.

### A4 Hide & show instances, and Switch to (video)
- A4.1 Each row in the Instances list has an **eye icon** (shown on hover) that hides or shows it.
- A4.2 Select several instances (list or graphics), right-click → **Hide**, **Hide other
  instances** (keeps the selection visible and hides the rest), **Hide all instances**,
  **Isolate…**, **Make transparent…**.
- A4.3 Right-click on **empty graphics space** → **Show all** (instances, mate connectors and mate
  features) or **Show all instances** (hidden instances only). The same menu has Paste, Create
  Drawing, Zoom to fit and Isometric (`ex2-step13.png`).
- A4.4 Select hidden instances in the list, right-click → **Show**.
- A4.5 Shortcuts: hover an instance and press **Y** to hide it; **Shift+Y** shows.
- A4.6 Right-click a part instance → **Switch to <Part Studio>** (Part Studio icon). This activates
  the source Part Studio tab with that part highlighted. For a subassembly instance, **Switch to**
  (assembly icon) opens that subassembly's tab.

### A5 Exercise: Start an Assembly (`intro-to-assemblies/ex1-*`, part image `ex1-drawing.png`)
Goals: add an assembly tab, insert an instance, use the triad to put the instance on the origin,
fix it. Starting document: the public "Exercise: Starting an Assembly" (one Part Studio, "Motor
Mount", with a single part, the **DC Motor Mount**: an L-shaped bracket with a rounded upright
plate, a slot and 6 small holes, two gusset ribs, and a base with 6 holes; features include
Extrude, Rib, Chamfer, Fillet, a **Ø0.375 in THRU** hole and a **Ø0.563 in THRU** hole). Units:
**inch, pound**. **Not buildable from scratch**, because no part dimensions are given.
1. A5.1 Open the public document and **Make a copy to edit** (`ex1-step1.png`).
2. A5.2 **"+" → Create Assembly**. Right-click the new tab → **Rename** → "Mount Assembly"
   (`ex1-step2.png`; the "+" menu lists Create Part Studio, Create Assembly, Create Variable
   Studio, Create Drawing…, Create folder, Import…, and others).
3. A5.3 **Insert** → Current document → Part Studios → pick **Motor Mount** → click in the
   graphics area to place it (it's off the origin) → ✓ (`ex1-step3.png`).
4. A5.4 Click the edge of the **centre back hole** on the underside of the base. The triad
   appears there (`ex1-step4.png`).
5. A5.5 Drag the triad origin onto the **centre point of that hole** (the readout shows
   "Diameter: 0.563 in") (`ex1-step5.png`).
6. A5.6 Right-click the triad origin → **Move to origin** (`ex1-step6.png`).
7. A5.7 Right-click the triad's **Z arrow** → **Anti-align with Z** (`ex1-step7.png`).
8. A5.8 Check the orientation: base plate horizontal, upright plate hanging below it (the part is
   upside-down relative to its Part Studio, with the hole's axis anti-aligned to Z). If it's wrong,
   use the other align option (`ex1-step8.png`).
9. A5.9 Right-click the instance → **Fix** (`ex1-step9.png`, full instance context menu).
10. A5.10 Select the top-level **Mount Assembly** row → **Mass properties** (`ex1-step10.png`).
- **Self-check**: the assembly's **center of mass (in)** (X, Y, Z; blanked in the screenshot). The
  visible values are **Mass 10.318 lb**, **Volume 36.383 in³**, **Surface area 136.696 in²**, and
  **inertia (in²·lb): Lxx 51.687, Lyy 47.333, Lzz 45.598, Lxz = Lzx −13.398, Lxy = Lyz = 0**. The CoM
  checks the *placement*: the hole centre on the origin and the Z anti-alignment. So cadrs must
  report the CoM in **assembly** coordinates, after instance transforms.

## 2. Mating assembly instances

### A6 Mating in Onshape (video; image `lesson-mate-dialog-offset.png`)
- A6.1 Mates don't join geometry directly. Each mate joins **two mate connectors**, which are
  local coordinate systems (X, Y, Z) on or between entities. Connectors are **explicit** (made
  with the Mate connector tool, outside a mate) or **implicit** (made while picking inside a mate
  dialog; they're owned by the mate).
- A6.2 **One mate feature defines all the DOF between two instances.** Each mate type is named
  after the motion it allows. **Fastened removes all 6.**
- A6.3 **Mate dialog** layout (`ex2-step7.png`, `ex2-step12.png`): a title ("Revolute 1"), ✓ / ✗,
  a **mate-type dropdown** (the type can be changed at any time without re-picking), a **Mate
  connectors** field with two entries ("Mate connector of <part>", each with an edit icon and ×)
  and a **Reorder items** (⇅) button, and checkboxes **Offset**, **Limits** and **Simulation
  connection**. The bottom row has **Flip primary axis** (arrow), **Reorient secondary axis**
  (circular arrows), **Animate** (▶), **Solve**, and help (?).
- A6.4 **Implicit mate connector points** appear while hovering faces and edges with a mate dialog
  open: at the **centroid** of a face or sketch profile, at **midpoints**, **vertices**, the
  **centres of circular edges and holes**, the centres of cut (negative) space, and the **virtual
  sharp** of conic faces. The connector snaps to the nearest point.
- A6.5 Holding **Shift** locks the current face or edge, so a point in a tight spot or in negative
  space can be reached.
- A6.6 Circular edges and faces are the fastest picks: they have only a centroid point, and no
  midpoints or vertices.
- A6.7 **Solving on pick**: after the second connector is picked, the instances move so that the
  two connectors **coincide**, with their primary **Z axes aligned** (facing each other).
- A6.8 **Flip primary axis** reverses Z. **Reorient secondary axis** rotates the X/Y pair in
  **90° steps**. Both act on the **first** connector in the list, or on the second if the first
  one's instance can't move (e.g. it's fixed). To act on the other connector, click **Reorder
  items**, drag the handles on the right, and click **Done**.
- A6.9 **Animate mate DOF** (▶) previews the allowed motion. Fastened shows none; switching the
  type to Slider animates along Z.
- A6.10 **Offset** ✓ gives **X, Y, Z** distance fields, a **"Rotate about X/Y/Z"** dropdown and a
  **Rotation angle** (deg). **Limits** ✓ gives min/max fields for each of the mate's DOF (e.g.
  **Z min / Z max** distance for a slider, or angles for a revolute).
- A6.11 The whole assembly is **not re-solved continuously** while a mate is being edited. **Solve**
  in the dialog re-solves all mates and updates positions. Accepting also solves.
- A6.12 **Mate Features list**: mates listed in creation order. A mate with limits shows a **limit
  icon**. Hovering a mate shows a tooltip with its offset and limit values. Double-click (or
  right-click → **Edit**) edits it.
- A6.13 Mate context menu (`lesson-mate-context-menu.png`): **Rename**, **Edit…**, **Apply limit
  position ▸** (jump to a limit), **Reset** (back to the mate's zero position), **Show**, **Show
  all mates**, **Isolate…**, **Make transparent…**, **Edit named position driver mate…**,
  **Suppress**, **Animate…**, **Add comment**, **Add selection to folder…**, **Expand ▸**,
  **Collapse ▸**, **Delete**.

### A7 Mate reference sheet (expandable cards)
The course's summary table. All axes are in the mate's coordinate system, where **Z is the
primary axis**:

| ID | Mate | DOF | Allowed motion |
|---|---|---|---|
| A7.1 | Fastened | 0 | none |
| A7.2 | Revolute | 1 | rotate about Z |
| A7.3 | Slider | 1 | translate along Z |
| A7.4 | Cylindrical | 2 | translate along Z + rotate about Z |
| A7.5 | Pin slot | 2 | rotate about Z + translate along **X** |
| A7.6 | Planar | 3 | translate along X and Y + rotate about Z |
| A7.7 | Ball | 3 | rotate about X, Y and Z |
| A7.8 | Parallel | 4 | translate along X, Y and Z + rotate about Z |
| A7.9 | Tangent | (entity-based) | keeps two entities tangent (A11) |
| A7.10 | Width | 2 (per the course) | tabs centred between two width connectors (A12) |

### A8 Fastened, Revolute and Slider mates (video)
- A8.1 **Fastened**: e.g. a rod in a hole, or welded parts. Pick the hole-centre connector, then
  the rod's end-face centroid. Then **Offset** ✓ with a **Z** value leaves room at the end of the
  rod for fasteners. A negative value offsets the other way.
- A8.2 Hovering the mate in the list shows its offset without opening it.
- A8.3 **Revolute** (1 rotational DOF): e.g. a mount that swivels under a rear cap. Pick the
  centroid of the mount's top face and the centroid of the cap's bottom face. **Animate** to
  check.
- A8.4 **Slider** (1 translational DOF): e.g. a piston in a barrel. Pick the bottom cap's top face
  centroid, then the piston's bottom face centroid. **Limits** ✓ stops the piston before it hits
  the top cap. If the limit acts in the wrong direction, negate it.

### A9 Cylindrical and Pin slot mates (video)
- A9.1 **Cylindrical**: e.g. a recoil-starter grip pulled out of its cover. Pick the grip's bottom
  face, then the cover's cord-inlet face. It's free both ways along Z without limits, so add a
  **Limit** that keeps it from going through the cover.
- A9.2 **Pin slot**: **the first pick must be the slot, the second the pin**, and the field order
  reflects that. If they were picked the wrong way round, use **Reorder items** → drag → **Done**.
  Pick the slot by moving the cursor to the **centre of the slot**. Flip the primary axis if
  needed.
- A9.3 Pin slot **Limits**: **X min = −½ slot length**, **X max = +½ slot length**. To stop
  rotation, set the **Z (angle) min and max to 0**. Animate to verify.

### A10 Planar, Ball and Parallel mates (video)
- A10.1 **Planar** (3 DOF): pick a point on each instance where they should touch. They move into
  contact but stay free within the plane. It's often combined with other mates, e.g. **Planar +
  Tangent** keeps a pin in a curved slot.
- A10.2 **Ball** (3 rotational DOF): a connector at the centre of the socket and one at the centre
  of the ball.
- A10.3 **Parallel** (4 DOF): e.g. a magnet on a robot arm kept parallel to a base. Pick
  connectors on the two faces that must stay parallel. They're placed touching, but that position
  isn't held.

### A11 Tangent mate (video)
- A11.1 The **only mate that doesn't use mate connectors**: it takes **two entity picks** (faces,
  edges, vertices, surfaces, sketch entities).
- A11.2 **Tangent propagation** checkbox (**on by default**): the tangency extends to adjacent
  tangent faces. When it's off, only the picked entities stay tangent.
- A11.3 It has a Flip (primary axis) toggle for the alignment side.

### A12 Width mate (video)
- A12.1 Two fields: **Tab** mate connectors (**up to two**) and **Width** mate connectors
  (**exactly two**). The Width pair defines a centre plane, and the Tabs are kept **symmetric**
  about it.
- A12.2 Picks may come from different instances, but **no instance may appear in both fields**.
- A12.3 If both tabs belong to one instance, that instance stays centred between the width
  connectors, free to slide along and rotate about the centre plane. If the tabs belong to two
  instances, they stay mirror-symmetric about the centre plane as they move.

### A13 Grouping instances (video)
- A13.1 **Group** (toolbar) removes all DOF *between* the selected instances. They move together
  as one rigid unit.
- A13.2 The dialog has a single **Instances** list, then ✓. The group appears in the Mate Features
  list as "Group 1" (`ex2-step5.png`).
- A13.3 Grouped instances keep their positions **relative to their origins**. Geometry changes are
  ignored. Best for purchased parts, or for parts **from the same Part Studio** (which share an
  origin). With parts from different studios, edits can make instances overlap.
- A13.4 A group still needs **Fix** or a mate to the rest of the assembly. There's no limit on the
  number of groups.

### A14 Hide & show mates (video)
- A14.1 Mates are **hidden in the graphics area by default**. **J** toggles showing all of them.
- A14.2 Hovering a mate in the list highlights its instances and its graphics icon. Each row has
  an eye toggle.
- A14.3 Select mates → right-click → **Show / Hide**. Right-click an **instance** → **Show mates /
  Hide mates** acts on every mate of that instance.
- A14.4 **Show mates mode** (toggle, or **H**): hovering an instance (list or graphics) shows its
  mates. Clicking pins them. In subassemblies it shows mates at any level.
- A14.5 Clicking a mate's graphics icon cross-highlights it in the list. Its right-click menu has
  **Edit**.

### A15 Exercise: Pneumatic Cylinder (`ex2-*`, overview `ex2-drawing.png`)
Goals: create an assembly, insert instances, use Fastened, Cylindrical and Slider mates, Group,
limits and offsets. Starting document: the public "Exercise: Pneumatic Cylinder", with one Part
Studio "**Cylinder parts**" (53 features, a "Cylinder Housing" folder) containing 9 parts:
**Barrel** (clear tube), **Top Cap**, **Structural Rod**, **Piston & Rod**, **Retaining Plate**,
**O-Ring 0.125**, **O-Ring 0.185**, **Rear Cap**, **Rear Cap mount**. Units: **inch, pound**.
**Not buildable from scratch.** (The goals mention Cylindrical, but no step uses it; the steps use
Group, Revolute, Fastened and Slider.)
1. A15.1 Make a copy (`ex2-step1.png`).
2. A15.2 "+" → **Create Assembly**, rename it "**Cylinder assembly**" (`ex2-step2.png`).
3. A15.3 **Insert** **Barrel, Rear Cap, Retaining Plate, Top Cap** in one session, accepting with
   the cursor in the dialog so they keep their Part Studio positions (`ex2-step3.png`).
4. A15.4 Right-click the **Barrel** → **Fix** (`ex2-step4.png`).
5. A15.5 **Group** all four instances → ✓, giving "Group 1". They're made in one studio, so they
   share an origin (`ex2-step5.png`).
6. A15.6 **Insert** the **Rear Cap mount**, placed by clicking in the graphics area (`ex2-step6.png`).
7. A15.7 **Revolute** mate: the circular edge of the recess on the Rear Cap's underside, then the
   circular top edge of the mount. The mount then swivels under the cap (`ex2-step7.png`).
8. A15.8 **Insert** 4× **O-Ring 0.125**, clicking four times (`ex2-step8.png`).
9. A15.9 Hide the Barrel (eye icon). **Fastened**: the O-Ring's top edge, then the top edge of a
   Rear Cap groove. Flip the primary axis if needed (`ex2-step9.png`).
10. A15.10 Repeat for the other three O-Rings: two grooves on the Rear Cap and two on the Top Cap,
    giving Fastened 1–4 (`ex2-step10.png`).
11. A15.11 **Insert** the **Structural Rod** (`ex2-step11.png`).
12. A15.12 **Fastened**: the rod's top edge, then a hole edge on the Top Cap flange. **Offset** ✓,
    **Z = 0.5 in**, leaving room for a nut. The dialog shows X 0 in, Y 0 in, Z 0.5 in, Rotate about
    X, 0 deg (`ex2-step12.png`).
13. A15.13 Right-click the rod in the list → **Copy Structural Rod <1>**. Right-click empty
    graphics → **Paste Structural Rod <1>**, three times, for 4 rods (`ex2-step13.png`).
14. A15.14 Repeat A15.12 for the other three rods at the other flange holes, giving Fastened 5–8
    (`ex2-step14.png`).
15. A15.15 **Insert** the **Piston & Rod** (`ex2-step15.png`).
16. A15.16 Hide the Barrel. **Slider**: an edge on the Rear Cap (inner top), then the bottom edge
    of the piston. **Limits** ✓, **Z min = −4.5 in**, Z max = 0 in. If it runs the wrong way, use
    0 / +4.5. The limits keep the piston between the caps (`ex2-step16.png`).
17. A15.17 **Insert** 3× **O-Ring 0.185** (`ex2-step17.png`).
18. A15.18 **Fastened**: each O-Ring's top edge to a piston groove's top edge, giving Fastened
    9–11 (`ex2-step18.png`).
19. A15.19 Right-click **Slider 1** → **Reset** (back to its zero position) (`ex2-step19.png`).
20. A15.20 Select the top-level **Cylinder assembly** → **Mass properties** (`ex2-step20.png`).
- End state: **17 instances** (Barrel, Rear Cap, Retaining Plate, Top Cap, Rear Cap mount,
  4× O-Ring 0.125, 4× Structural Rod, Piston & Rod, 3× O-Ring 0.185) and **14 mate features**
  (Group 1, Revolute 1, Fastened 1–11, Slider 1).
- **Self-check**: the assembly **center of mass (in)**, blanked. The visible values are **Mass
  30.609 lb**, **Volume 122.673 in³**, **Surface area 757.21 in²**, and **inertia (in²·lb): Lxx
  776.299, Lyy 776.569, Lzz 77.188, Lxy = Lyx 0.003, the others 0**. The CoM depends on the piston's
  position, which is why the slider is reset first.

## 3. Working with an assembly

### A16 Assembly motion (video, 2:54)
- A16.1 Instances that still have free DOF show a **triad icon** next to them in the list. Its
  tooltip says the instance has degrees of freedom.
- A16.2 A **subassembly** instance is flexible or rigid. Toggle this with the **lock icon** in the
  list, or right-click → **Lock/follow position to** (lock to the current position, or follow a
  **Named position**). A subassembly whose parts are all fully constrained shows a **Rigid** icon
  instead of the plain assembly icon.
- A16.3 The top-level assembly row's icon shows how the base is held: **fixed**, or **fastened to
  the origin** with a mate. Neither carries into a parent assembly. Use it for one foundation
  component only.
- A16.4 **Drag** a movable instance to see the motion that its mates allow (e.g. a piston slides
  and stops at its slider limit). The **triad** gives precise, value-driven drags.
- A16.5 **Animate**: right-click a mate with DOF → **Animate**. If the mate has more than one DOF,
  pick which one. Fields:
  - **Start / End**: they default to the full limit range if the mate has limits.
  - **Steps**: fewer steps play faster, more play slower.
  - **Playback type**: **Single** (once), **Reciprocate** (back and forth until stopped), **Loop**
    (start to end, repeated).
  - A **reverse direction** arrow, a read-only **Current value**, and **Play** / **Stop**.

### A17 Assembly structure (video)
- A17.1 Break complex designs into **subassemblies** wherever possible.
- A17.2 Method 1: "+" → **Create Assembly**, build it, then in the parent **Insert → Assemblies**
  tab. By default, a subassembly's mates are **respected** in the parent (it moves as it would in
  its own tab).
- A17.3 Method 2 (on the fly): right-click instance(s) → **Move to new subassembly** (creates a
  new tab "Assembly 1" containing them *and their mates*), or → **Create empty subassembly**.
- A17.4 **Drag and drop** in the Instances list: drop instances onto a subassembly row to move them
  in (a badge shows "n items"), or drag them out of an expanded subassembly back to the top level.
  Mates move with them.
- A17.5 Right-click a subassembly → **Dissolve subassembly**: its instances and mates return to the
  parent and the row disappears. The now **empty Assembly tab stays** in the document, to reuse or
  delete.

### A18 Assembly folders (video; image `lesson-assembly-folders.png`)
- A18.1 Folders organize the **Instances** list and the **Mate Features** list.
- A18.2 Create one with the **New folder** icon in the list header: name it and accept. It goes to
  the bottom of the list. Drag instances into it, and drag the folder to reorder it.
- A18.3 Or select first, then click New folder or right-click → **Add selection to folder…**: a
  "Folder name" popup with ✓ / ✗. The folder is placed at the first selected item.
- A18.4 The folder's eye icon hides or shows everything in it. Its context menu has **Rename**,
  Hide/Show, **Suppress/Unsuppress** (applies to the contents), **Delete** (removes the folder *and
  its contents*) and **Unpack folder** (removes only the folder).
- A18.5 Mate folders work the same way: select mates → right-click → **Add selection to folder**.

### A19 Standard content (video; image `lesson-standard-content.png`)
- A19.1 **Insert → Standard content** tab: a fastener library. Cascading dropdowns: **Standard**
  (e.g. ANSI inch), **Category** (Bolts & screws, Nuts, …), **Class** (Hex bolts, Hex nuts, …),
  **Component** (Hex cap screw, Hex nut, …). Then component options such as **Size**, **Length**,
  **Thread length**, **Material**, and for nuts **Bearing face** and **Finish**.
- A19.2 **Auto-size** button next to Size: pick a circular edge or cylindrical face and the best
  size is filled in. For bolts and screws it rounds **down** (no interference). For a nut picked on
  a bolt shaft it rounds **up** (so it fits).
- A19.3 **Part number** and **Description** fields (Description is auto-filled, e.g. "Hex cap
  screw 1/4-28 x 0.75 S…"). Setting them needs permissions, and applies company-wide per unique
  component.
- A19.4 A **preview** drawing of the component at the bottom of the dialog.
- A19.5 **Single placement**: click **Insert**, hover a hole, click a connector point. **A** flips
  the fastener's direction while placing. A **Fastened** mate is created automatically.
- A19.6 **Batch placement**: *select first* (hole faces, hole edges, or faces containing holes; any
  mix), then click **Insert**. One fastener goes on each hole, each with its own mate.
- A19.7 **Stacking**: **Insert closest to selection** and **Insert furthest from selection** buttons
  (next to Insert) add later parts (washers, nuts) to the existing stack mated to that geometry.
- A19.8 Standard content instances get their own **Standard content icon** in the list.
- A19.9 **Bulk edit**: right-click a fastener → **Select instances with the same configuration** or
  **… with same part and same configuration**, then right-click → **Edit standard content
  instance**. Size and Length can be changed, but not Standard, Category, Class or Component.
  **Update**, then ✓.

### A20 Simultaneous Bill of Materials (video)
- A20.1 Every assembly has an automatic, **live BOM table** (right-panel icon) that updates as the
  assembly and properties change.
- A20.2 Hovering a row highlights the component in the viewport. Clicking an item number
  cross-highlights it in the list and the viewport.
- A20.3 Row order follows the **Instances list order**, so dragging instances reorders the BOM.
  **Double-clicking a column header** sorts by it.
- A20.4 Two views: **Structured** (top-level items; double-click a subassembly's item number to
  expand or collapse it) and **Flattened** (every instance as if it were top-level).
- A20.5 Per subassembly: right-click → **Properties** → **Subassembly BOM behavior**:
  - **Show assembly and components** (the default, indented),
  - **Show assembly only** (one line, e.g. for purchased assemblies),
  - **Show components only** (as if the parts were inserted at the top level).
- A20.6 Columns: **Add column** (any property). The header right-click menu has **Remove column**,
  **Move left** and **Move right**.
- A20.7 **Apply template**, and overflow ⋯ → **Save as template**. Overflow ⋯ also has **Copy
  table** (clipboard) and **Export to CSV**.
- A20.8 Right-click a row → **Suppress from this BOM**. Overflow → **Show excluded/suppressed**:
  suppressed rows show "–" as their item number. Right-click → **Unsuppress in this BOM**, and
  overflow → **Hide excluded/suppressed**.
- A20.9 If the company has aggregated properties: overflow → **Show top-level assembly row**, with
  totals at the bottom.
- A20.10 **Cells are editable in place** (e.g. Part number, Description). Double-clicking a
  Material cell opens a material picker. Edits write back to the part or subassembly properties,
  and property edits show up in the BOM. It's **two-way**.
- A20.11 With sequential part numbering enabled: right-click the Part number header → **Generate
  missing part numbers**.

### A21 Exercise: Assembly Structure (`ex3-*`, target image `ex3-drawing.png`)
Goals: standard content, subassemblies (create, fill, move out, dissolve), an assembly folder.
Starting document: the public "Exercise: Assembly structure". It's the **finished Pneumatic
Cylinder** from A15: 17 instances, 14 mates, the Barrel fixed. Units: **inch, pound** (the first
step says to check the workspace units). **Not buildable from scratch**, but it follows on from
the A15 result.
1. A21.1 Make a copy, and check that the workspace units are **Inch / Pound** (`ex3-step1.png`).
2. A21.2 **Insert → Standard content**: ANSI inch / Bolts & screws / Hex bolts / **Hex cap screw**,
   Size **1/4-28**, Length **0.75**, Thread length **0.75**, Material **Stainless Steel**. With
   the Piston & Rod hidden, click the **6 hole edges** on the Retaining Plate (top of the Top Cap)
   → **Insert** → ✓ (`ex3-step2.png`).
3. A21.3 Select the 6 screws (tip: right-click one → *Select instances with same part and same
   configuration*) → right-click → **Add selection to folder…** → "**Hardware**"
   (`ex3-step3.png`).
4. A21.4 Select **Retaining Plate, Top Cap, O-Ring 0.125 <3>, <4>** (the two on the Top Cap) →
   right-click → **Move to new subassembly** (`ex3-step4.png`).
5. A21.5 Open the new "Assembly 1" tab → rename it "**Top Cap subassembly**" → right-click the Top
   Cap → **Fix**. It contains 4 instances and 3 mates: Fastened 1, Fastened 2 (the O-Rings) and **Fastened 20**.
   Fastened 20 probably holds the Retaining Plate to the Top Cap, taking over from the old Group
   (`ex3-step5.png`).
6. A21.6 In **Cylinder assembly**, right-click any instance → **Create empty subassembly**
   (`ex3-step6.png`).
7. A21.7 Select **Rear Cap, Rear Cap mount, O-Ring 0.125 <1>, <2>** and **drag** them onto the
   empty "Assembly 1" row (badge "4 items") (`ex3-step7.png`).
8. A21.8 Open "Assembly 1" → rename it "**Rear Cap subassembly**" → right-click the Rear Cap →
   **Fix**. It gets 3 mates: two Fastened and Revolute 1 (`ex3-step8.png`).
9. A21.9 In the top level, select **Piston & Rod** and the 3× **O-Ring 0.185** → **Move to new
   subassembly** (`ex3-step9.png`).
10. A21.10 Rename it "**Piston subassembly**" and **Fix** the Piston & Rod. It has Fastened 9, 10,
    11 (`ex3-step10.png`).
11. A21.11 In the top level, expand Rear Cap subassembly and **drag the Rear Cap mount out** to the
    top level (`ex3-step11.png`).
12. A21.12 Right-click **Piston subassembly** → **Dissolve subassembly**. Its instances return to
    the top level, and the tab remains (`ex3-step12.png`).
13. A21.13 Open the empty **Piston subassembly** tab (0 instances) → right-click the tab →
    **Delete**. The tab menu has Delete, Open in new browser tab, Rename, Properties, Duplicate,
    Copy to clipboard, Create Drawing…, Select as document thumbnail, Move to document…, Export…,
    Create task… (`ex3-step13.png`).
14. A21.14 **Insert → Standard content**: ANSI inch / Nuts / Hex nuts / **Hex nut**, Size
    **3/8-16**, Bearing face **Chamfered**, Material **Stainless Steel**, Finish **Plain**. Click
    the **4 rod-hole edges on top of the Top Cap** and the **4 on the underside of the Rear Cap** →
    Insert → ✓, giving 8 nuts (`ex3-step14.png`).
15. A21.15 **Drag** the 8 Hex nuts into the **Hardware** folder, which then holds 14 items
    (`ex3-step15.png`).
16. A21.16 Select the **Top Cap subassembly** (in its own tab, top-level row) → **Mass
    properties** (`ex3-step16.png`).
- End state (`ex3-drawing.png`): **Instances (26)**: Cylinder assembly, Origin, Rear Cap mount
  <1>, Barrel <1> (fixed), **Rear Cap subassembly <1>**, **Top Cap subassembly <1>**, Structural
  Rod <1–4>, Piston & Rod <1>, O-Ring 0.185 <1–3>, and **Hardware (14)** (6 screws + 8 nuts).
- **Self-check**: the **Top Cap subassembly's center of mass (in)**, blanked. The visible values
  are **Mass 10.305 lb**, **Volume 37.298 in³**, **Surface area 142.341 in²**, and **inertia
  (in²·lb): Lxx = Lyy 22.49, Lzz 30.857, the products 0** (it's axisymmetric, so the CoM lies on
  the Z axis).

## 4. Explicit mate connectors

### A22 Explicit mate connectors (video)
- A22.1 Usually the implicit points plus Flip and Reorient are enough. When no suitable point
  exists (e.g. the bottom centre of a helical spring), create an **explicit mate connector**.
- A22.2 **Where to create it**: in the **Part Studio**, it belongs to a part and **travels with the
  part into every assembly** it's inserted into. In an **assembly**, it stays in that assembly
  only.
- A22.3 Workflow: right-click the part → **Switch to** its Part Studio → **Mate connector** tool
  (toolbar, in the curve/plane dropdown, shortcut **Ctrl+M**) → click a connector point (e.g. the
  bottom centre of the helix's construction cylinder) → ✓.
- A22.4 **Between entities** option: pick two entities (e.g. the two end faces of a cylinder) and
  the connector goes midway between them.
- A22.5 **Owner part** field: in a multi-part studio, check that the connector is owned by the
  right part (clear it and pick the spring, not the construction cylinder).
- A22.6 After placing it, the connector can be adjusted: **flip the primary axis**, **reorient the
  secondary axis**, **Realign** to model references, or **Move**.
- A22.7 Back in the assembly, the connector shows on the instance and can be picked in a mate.
- A22.8 **Any sketch entity** (e.g. a circle's centre point) can be the reference, so a sketch can
  place connectors where there's no solid geometry.

### A23 Editing implicit mate connectors (video)
(The lesson is listed as "Editing Explicit Mate Connectors", but its content is about implicit
ones.)
- A23.1 Implicit connectors belong to their mate. **Expand the mate** in the Mate Features list to
  see its two connectors. Hovering one highlights its geometry.
- A23.2 Right-click the connector → **Edit** opens the mate-connector dialog.
- A23.3 Example: a Pin slot mate whose slot connector's X axis isn't along the slot. Check
  **Realign**, click into **Secondary axis** and pick the slot's edge. The X axis rotates to be
  parallel to it → ✓.
- A23.4 Other adjustments in the same dialog: **Move** (translate or rotate offsets), **Flip
  primary**, **Reorient secondary**.

### A24 Exercise: Creating Explicit Mate Connectors (`ex4-*`, target image `ex4-drawing.png`)
Goals: Revolute mates with limits; explicit mate connectors. Starting document: the public
"Creating Mate Connectors - Start", a **folding step stool**. Tabs: **Step Stool Assembly**, Part
Studios **Large Frame Bar** and **Base Frame Bar** (plus a "Large Step" tab and a "Part Studios"
folder). The assembly is already mostly built: 13 instances (Large Frame Bar (fixed), Front Foot
<1>/<2>, Small step, Link 1, Link 2, Back Foot, Base Frame Bar, Cross Bar, **Replicate 1**, Large
Step…) and 14 mates (Fastened 1–5, Revolute 1–4 and 6, Cylindrical 1–4). Units: **inch, pound**.
**Not buildable from scratch.**
1. A24.1 Make a copy, and check the units are **Inch / Pound** (`ex4-step1.png`).
2. A24.2 Open **Step Stool Assembly**. The one missing mate is the hinge between the two frames,
   with limits for the folding motion (`ex4-step2.png`).
3. A24.3 Switch to the **Base Frame Bar** Part Studio. It has 3 parts: Base Frame Bar, Cross Bar,
   Back Foot (`ex4-step3.png`).
4. A24.4 Start a **Mate connector** feature (toolbar dropdown, or **Ctrl+M**) (`ex4-step4.png`).
5. A24.5 Type **Between entities**. **Origin entity** = the edge of the hinge hole in the top lug
   of one leg. **Between entity** = the inside face of the lug on the opposite leg. **Owner
   entity** = Base Frame Bar. The result is "Mate connector 2" at mid-span on the hinge axis. The
   dialog has Realign, Move and Owner entity checkboxes, plus flip/reorient icons
   (`ex4-step5.png`).
6. A24.6 Switch to the **Large Frame Bar** Part Studio. Its **Hole Positions** sketch is visible
   (`ex4-step6.png`).
7. A24.7 Start another **Mate connector** (`ex4-step7.png`).
8. A24.8 Type **On entity**. Origin entity = the edge or profile of the **centre hole circle in the
   Hole Positions sketch**. Owner = Large Frame Bar. This gives "Mate connector 3". The note: a
   Between-entities connector on the cylindrical faces would land in the wrong spot, so reference a
   sketch or add an offset (`ex4-step8.png`).
9. A24.9 Back in **Step Stool Assembly**, both new connectors show on their instances
   (`ex4-step9.png`).
10. A24.10 **Revolute** between the two connectors (primary axes already aligned). **Limits** ✓:
    **Z min angle −42.5°**, **Z max angle 0°**, so the base frame swings towards the other leg (or
    0 / +42.5, depending on pick order). **K** hides the mate connectors afterwards. This gives
    "Revolute 5" (`ex4-step10.png`).
11. A24.11 Drag to check that the stool opens and closes, then right-click **Revolute 5** →
    **Reset** (`ex4-step11.png`).
12. A24.12 **Mass properties** of the whole assembly. Materials are already assigned
    (`ex4-step12.png`).
- **Self-check**: the assembly **center of mass (in)**, blanked. The visible values (note: Mass
  shows in **grams**, and inertia in g·in², despite the pound setting) are **Mass 7380.439 g**,
  **Volume 269.589 in³**, **Surface area 2691.084 in²**, and **inertia (g·in²): Lxx 881 544.305,
  Lyy 881 123.645, Lzz 587 138.18, Lxy −9.085, Lxz −16.318, Lyz −63 975.64**.

## Knowledge checks and survey
- All four exercises have a page-3 **Begin Self-Check** quiz that asks for a center of mass
  (inches). None were started. The visible mass properties are recorded with each exercise.
- There are no stand-alone knowledge-check quizzes in this course.
- A **Completion Survey** ("Completion Certificate" section) is required for the certificate. It
  wasn't started.

## Cross-cutting requirements found in the course
- X1 **Assembly tab/document model**: several Assembly tabs per document, created from "+"
  (Create Assembly), with tab rename, delete, duplicate and other context-menu items (A2.2,
  A21.13). An assembly instance references a Part Studio part or another assembly tab, and stays
  **live**: it updates when the source changes.
- X2 **Instance model**: each inserted item is an instance with a transform, a display name
  `<Name> <n>`, and states for hidden, suppressed, fixed, transparent and isolated. Multiple
  instances of one part are allowed (insert repeatedly, or copy/paste) (A2, A15.13).
- X3 **Insert dialog** with Part Studios/Assemblies tabs, search, per-type filters, insert-rigid,
  the placement rule (inside dialog = original position; outside = click to place), multi-insert
  with a counter, and "Undo to remove instances" (A2.3–A2.6).
- X4 **Triad manipulator** in the assembly: relocatable origin with snapping, planar/axial/
  rotational drag with typed values, **Move to origin**, **Align/Anti-align with Z**, and rotate
  90°/180° (A3).
- X5 **Mate connectors** as full frames: implicit points (centroid, midpoint, vertex, circle
  centre, virtual sharp) with Shift-lock, flip primary, reorient secondary in 90° steps, and
  reorder; explicit connectors in Part Studios (On entity / Between entities, Owner part, Realign,
  Move) that travel with the part; editing implicit connectors under a mate (A6, A22, A23). See
  also PS X11.
- X6 **Mate solver** for the 10 mate types in A7, with **offsets** (XYZ + rotation), **limits**
  (per DOF, min/max), **Reset**, **Apply limit position**, drag-to-move that respects mates and
  limits, and a Solve button. **Fix** and **Group** (A3.7, A13) as rigid constraints. DOF
  indicators on instances (A16.1).
- X7 **Mate animation**: the in-dialog ▶ preview, and a full **Animate** dialog with Start/End,
  Steps, Single/Reciprocate/Loop, reverse, and a current-value readout (A6.9, A16.5).
- X8 **Assembly tree**: Instances list and Mate Features list, with Items/Loads groups, a filter
  field, eye toggles, hover cross-highlighting, folders in both lists, drag-and-drop reorder and
  re-parenting (A1, A14, A17, A18).
- X9 **Subassemblies**: insert an assembly tab as an instance; Move to new subassembly, Create
  empty subassembly, drag in and out, Dissolve; flexible vs rigid (lock icon, Named positions)
  (A16.2, A17).
- X10 **Visibility shortcuts**: **Y** hide / **Shift+Y** show hovered instances, **J** all mates,
  **H** show-mates mode, **K** mate connectors; plus Show all / Show all instances / Hide other
  instances (A4, A14, A24.10). Note that **H** means Horizontal inside a sketch (S12.5) and
  show-mates mode inside an assembly, so shortcuts are context-dependent.
- X11 **Switch to** from an instance to its source Part Studio (with the part highlighted) or
  subassembly tab (A4.6).
- X12 **Standard content library**: a standards table of fasteners with size/length/material
  options, auto-size from a picked hole or shaft, single or batch placement with auto Fastened
  mates, **A** to flip, stacking (closest/furthest), and bulk edit (A19). This is a large data
  dependency.
- X13 **Bill of Materials** panel: structured/flattened, per-subassembly BOM behaviour, editable
  cells that write back to properties, column management, templates, CSV export, suppress rows
  (A20).
- X14 **Assembly mass properties**: the Mass properties panel (PS X7) must work on a **whole
  assembly or subassembly**, in assembly coordinates, after mate solving. Every self-check is an
  assembly center of mass. The Mass and inertia units follow the workspace units (lb/in or g/in in
  the exercises).
- X15 **Instance context menu** (`lesson-instance-context-menu.png`): Properties, Hide / Hide
  other / Hide all, Isolate, Make transparent, Suppress, Fix, Show/Hide mates, Check
  interference, Add mate connector to instance origin, Replace instances, Edit in context, Switch
  to, Copy, Move to new subassembly, Change to version, Export, Where used, Create Drawing, Add
  selection to folder, Create empty subassembly, Delete. Many of these (Check interference, Replace,
  Edit in context, Change to version, Where used) are shown but not taught.
- X16 Seen but not taught: **Relations** (gear, rack, screw…), **Simulation connection**
  checkbox, **Named positions**, **Exploded views**, **Replicate** (an instance pattern, e.g.
  "Replicate 1" in A24), **Items** and **Loads** lists, and Configurations in assemblies.
