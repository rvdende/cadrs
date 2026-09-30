# Onshape "Introduction to Part Studios": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Introduction to Part Studios
(`learn.onshape.com/learn/course/introduction-to-part-studios/...`), read on 2026-09-25. The
requirements below are paraphrased, not the course text. Exercise drawings, step screenshots and a
few feature-dialog captures are in `intro-to-part-studios/` (git-ignored, local only).

The course has 22 lessons (one interactive UI tour, the rest short videos) and 5 exercises. Each
exercise ends with a "Begin Self-Check" quiz page. The quizzes and the closing completion survey
were **not** started, because that would record attempts on the user's account. Each exercise's
last step names the quantity the quiz asks for. That value is blanked out in the course
screenshot, but other mass-property values are visible, and they are recorded below.

Each requirement has an ID (`PS<lesson>.<n>`) so milestones and judges can refer to it. Sketching
items that are already covered by [intro-to-sketching.md](intro-to-sketching.md) (`S*`) and
concepts from [intro-to-parametric-cad.md](intro-to-parametric-cad.md) (`P*`) are cross-referenced,
not repeated.

## 1. Introduction

### PS1 Start a part (video, 2:26)
- PS1.1 Features take **faces or sketch regions** as input. Picking a **whole sketch** (its row in
  the feature list) uses its outer boundary and treats the enclosed inner loops as holes. The
  sketch doesn't need to be trimmed, and several separate contours are fine.
- PS1.2 You can pick **one closed region, or any combination of regions**, of a sketch instead of
  the whole sketch. No extra option is needed to do this. Closed regions show a **grey fill**.
- PS1.3 One **master sketch** can drive several features, each using different regions of it.
- PS1.4 A feature does **not consume** its sketch. The sketch stays at its place in the feature
  list and is **hidden automatically** the first time a feature uses it.
- PS1.5 To reuse a hidden sketch, click the **eye (show/hide) icon** on its feature-list row.
  Hiding parts makes the regions easier to pick.
- PS1.6 Revolve and Sweep accept whole sketches or regions in the same way.

### PS2 Part Studio interface (interactive tour; image `lesson-part-studio-interface.png`)
The tour highlights these areas of the Part Studio screen:
- PS2.1 **Undo/Redo** buttons at the top left of the toolbar. They work per session. Anything can
  also be undone through the **Versions and history** panel.
- PS2.2 **Part Studio toolbar**: every sketch and feature tool. The tools shown change with the
  context: sketch tools while sketching, feature tools otherwise.
- PS2.3 **Search tools** box at the right end of the toolbar (shortcut shown as `alt/⌥ c`).
  Typing finds a tool by name.
- PS2.4 **Features list** (left panel, e.g. "Features (43)"): every sketch and feature, in
  regeneration order. It has a **Filter by name or type** field at the top (see PS3).
- PS2.5 **New folder** button in the feature-list header, for grouping features.
- PS2.6 **Show regeneration times** button (stopwatch icon) in the feature-list header. It shows
  how long each feature takes, to help diagnose a slow Part Studio.
- PS2.7 **Default geometry** folder: **Origin**, **Top**, **Front**, **Right**.
- PS2.8 **Parts list** below the features, with groups for **Parts (n)**, **Surfaces (n)** and
  **Curves (n)**. Each entity can be selected, renamed, hidden, and so on.
- PS2.9 **View cube** with **camera and render options** (top right). It is used to orient, change
  the view state, and section the view.
- PS2.10 **Side panels** (right edge icon strip): Appearances, Configurations, Custom tables,
  Variables.
- PS2.11 **Measure**, **Analysis** and **Mass properties** tools (bottom-right icons).
- PS2.12 **Document tab bar** along the bottom: a "+" to add tabs, then Part Studio, Assembly,
  Drawing and image tabs.

### PS3 Filtering and organizing the feature list (video)
- PS3.1 Right-click a feature → **Add selection to folder…** makes a new folder that holds the
  feature. The folder can be expanded and collapsed.
- PS3.2 Drag and drop features onto a folder name, or into its contents, to add them. Drag them
  out to remove them. Folder contents keep the history order. Moving a feature into a folder that
  sits higher in the list can break dependencies.
- PS3.3 Right-click a folder → **Unpack folder** removes the folder and keeps its features.
  **Deleting** a folder deletes the features inside it too.
- PS3.4 The **Filter by name or type** field filters the list by feature name *or* feature type.
  For example, "Extrude" lists every extrude, whatever its name.
- PS3.5 A term in **quotes** matches the exact name: `"Extrude 1"` doesn't match `Extrude 10`.
- PS3.6 Hovering over the filter field shows help for its special prefixes: `:part <name>`
  (features that affect the part), `:type <type>`, `:name <name>`, `:errors [name]` (features with
  errors), `:folder <name>`, and `:variable <name>` (variables and the features that use them;
  case-sensitive).

## 2. Basic features

### PS4 Extrude (video)
- PS4.1 Extrude creates a **Solid**, a **Surface** or a **Thin** body. These are the three tabs
  at the top of the dialog.
- PS4.2 Input field **Faces and sketch regions to extrude**. It accepts a sketch, sketch regions,
  sketch entities or planar faces, picked in the feature list or the viewport.
- PS4.3 **End type** dropdown:
  - **Blind**: extrude to the **Depth** value.
  - **Up to next**, **Up to face**, **Up to part**, **Up to vertex** (the "Up to" family): stop at
    the chosen geometry. Each one has an **Offset distance** checkbox and value, with an
    opposite-direction arrow to flip the offset.
  - **Through all**: goes through the furthest extent of every part or surface along the path.
- PS4.4 An **opposite direction** arrow button next to the end type flips the extrude direction.
  In the viewport there's a draggable arrow manipulator.
- PS4.5 **Starting offset** checkbox (collapsible): start the extrude at a distance from the sketch
  plane, with its own flip arrow.
- PS4.6 **Direction** checkbox: extrude along a picked sketch line, linear edge, plane normal or
  mate connector instead of the sketch normal (which is the default).
- PS4.7 **Symmetric** checkbox: extrude to both sides of the sketch plane. The depth is then the
  **total** depth.
- PS4.8 **Second end position** checkbox: a separate end type and depth for the other direction,
  with the same options as the first.
- PS4.9 **Draft** checkbox: a draft angle on the side walls, with a flip arrow. It's only for
  solids, not surfaces.
- PS4.10 Surface extrude allows **open profiles**.
- PS4.11 **Thin** extrude gives an open or closed profile a wall thickness: **Thickness 1** and
  **Thickness 2** on either side, a **Flip wall** arrow, and a **Mid plane** checkbox for a
  symmetric wall.
- PS4.12 Default dialog layout, seen in the exercise screenshots: the Solid/Surface/Thin tabs;
  then the New/Add/Remove/Intersect tabs; the selection box; End type (default **Blind**) with the
  flip arrow; **Depth**; the checkboxes for Direction, Starting offset, Symmetric, Draft and Second
  end position; **Merge with all** or a Merge scope (for anything but New); and at the bottom the
  **rollback preview slider**. The ✓ and ✗ buttons sit in the header.

### PS5 Boolean options (video)
- PS5.1 Extrude (and Revolve, Sweep, Loft, Thicken, the patterns and Mirror) has four boolean
  tabs: **New**, **Add**, **Remove**, **Intersect**.
- PS5.2 The first solid feature has to be **New**. After that, the default tab is picked
  automatically: **Add** when the new body intersects an existing part (as in the exercises).
  **New** can still be chosen to make a separate part.
- PS5.3 **Remove** cuts material (an extruded cut). **Add** joins material and can merge several
  parts into one where the geometry bridges them. **Intersect** keeps only the overlap.
- PS5.4 Add, Remove and Intersect have a **Merge scope** field that limits which parts are
  affected, and a **Merge with all** checkbox.
- PS5.5 There's also a standalone **Boolean** feature with **Union**, **Subtract** and
  **Intersect** tabs and a **Tools** list. Subtract also has **Targets** and an optional offset.
  **Keep tools** keeps the tool bodies, which are consumed by default. In a Union, the resulting
  part takes its **properties from the first tool** selected. The Tools list can be reordered.

### PS6 Exercise: Control Arm (`intro-to-part-studios/ex1-*`, drawing `ex1-drawing.png`)
A flat link, symmetric in the sketch: a hub with a keyed bore in the middle, two end eyes, and
tangent webs between them. Units: **mm**. The part ends up **asymmetric**: the right half is
40 mm thick and the left half 25 mm.
Sketch (on **Top**; dimensions are in `ex1-drawing.png`):
- Vertical construction centerline from the origin.
- Hub: **Ø70** outer circle and **Ø35** inner circle, both centered on the origin. A
  **center-point rectangle 45 × 10** on the origin makes the keyway. The bore is the union of the
  Ø35 circle and the 45×10 rectangle.
- End eyes: **Ø35** outer and **Ø20** inner circles on each side, **Symmetric** about the
  centerline. The left eye center is **Horizontal** to the origin, and the right eye follows
  through the symmetry.
- Four lines, each **Tangent** to the Ø70 hub circle and an Ø35 eye circle (external tangents).
- Overall length **250**, from the outer edge of one eye to the outer edge of the other. That
  puts the eye centers at x = ±107.5.
Steps:
1. PS6.1 New document. Set **Workspace units → Length = Millimeter** (document ☰ menu →
   *Workspace units…*). Rename the document and the Part Studio tab to "Control Arm"; the Assembly
   tab can be deleted.
2. PS6.2 Draw the sketch above. Most constraints come from inference. The only manual ones are
   Symmetric, Tangent and Horizontal. Accept it when it's fully defined.
3. PS6.3 **Extrude 1**, Solid/**New**, **Blind 40 mm**, towards **+Z** (use the view-cube triad to
   check). Regions: the **hub ring**, the **right web** and the **right eye ring** (3 "Face of
   Sketch 1" entries).
4. PS6.4 **Show** Sketch 1 again (the eye icon), then **Extrude 2**, Solid/**Add** (picked
   automatically), **Blind 25 mm**, +Z. Regions: the **left web** and the **left eye ring** (2
   entries).
5. PS6.5 Parts list → right-click the part → **Rename** → "Control Arm".
6. PS6.6 Select the part and open **Mass and section properties** (bottom-right icon).
- **Self-check**: the **surface area**, to 2 decimals (mm²). The screenshot shows **Volume =
  368 749.705 mm³**. Recomputing it analytically from the region areas gives 368 749.705, which
  confirms the region split above. The predicted **surface area is 50 179.71 mm²**: bottom
  10 704.256, top faces the same, outer and hole walls, plus a 15 mm step wall along the Ø70 arc
  where the hub meets the left web. The dialog shows the part name in red because no material is
  assigned; Volume and Surface area still show.

### PS7 Revolve (video)
- PS7.1 The input can be a sketch, sketch regions, sketch entities, a planar face or a curve.
  Solid needs closed regions. Surface and Thin also accept open profiles.
- PS7.2 The **Revolve axis** field takes a linear sketch entity, a linear edge or curve, a
  **cylindrical face**, a **circular edge**, a circular sketch entity or arc (which use their
  axis), or a **mate connector**. A button next to the field creates a mate connector on the spot.
- PS7.3 Revolve type: **Full** (360°, the default), **Blind** (an angle, with a flip arrow),
  **Symmetric** (the angle split evenly across the profile), **Up to next / face / part /
  vertex**, each with an **Offset** option and a flip. There's also a **Second end position**.
- PS7.4 **Thin** revolve: Thickness 1 and 2, **Flip wall**, **Mid plane**.
- PS7.5 It has the same boolean tabs and Merge with all as Extrude (PS5).

### PS8 Exercise: Reducer Coupling (`ex2-*`, drawings `ex2-step2.png`, `ex2-step4.png`, `ex2-step6.png`)
A pipe reducer: two bolted flanges joined by a conical transition. Units: **inch**. The part axis
is **X**.
1. PS8.1 New document. **Length unit = Inch**. Rename it "Reducer Coupling".
2. PS8.2 Sketch on **Right**: **Ø6** outer and **Ø2** bore, both on the origin. A **Ø4.75**
   construction bolt circle carries **4× Ø0.625** holes (all **Equal**) at 0°/90°/180°/270°,
   placed with Vertical and Horizontal constraints to the origin. Tips: **N** views normal, **P**
   toggles planes.
3. PS8.3 **Extrude**, Solid/New, **Blind 0.62 in** towards **+X**. Selecting the whole sketch
   automatically excludes the bore and bolt-hole regions. Clicking a region in the viewport
   toggles it in or out of the selection.
4. PS8.4 Sketch on **Front**. **Use** (project) the bore edge on the flange face; it shows as a
   vertical line. Optionally make it construction. Add a horizontal construction centerline from
   the origin. Draw a closed four-sided profile: a slanted line up and to the right from the top
   of the projected line, two vertical ends, and a second slanted line **Parallel** to the first.
   Dimensions: **Ø2.625** at the flange end and **Ø3.75** at the far end. These are *diametral*:
   pick the endpoint, then the centerline, then place the dimension on the far side of the
   centerline. The axial length is **4.625**. Hold **Shift** while drawing to suppress inference.
   Tip: a **Pierce** constraint to the edge could replace the projection.
   - The resulting parallelogram runs from x = 0.62 to 5.245. The outer line goes from r = 1.3125
     to 1.875 and the inner one from r = 1.0 to 1.5625, so the wall is 0.3125 thick measured
     radially.
5. PS8.5 **Revolve**, Solid/**Add**, the new sketch, **Revolve axis** = the centerline (or a
   cylindrical face or circular edge of the flange), **Full**, Merge with all ✓.
6. PS8.6 Sketch on the revolve's end face. **Use** its inner circular edge (Ø3.125), then add the
   **Ø7.5** outer circle, a **Ø6** construction bolt circle and **4× Ø0.75** equal holes at the
   quadrants. Tip: hide the part to sketch more easily.
7. PS8.7 **Extrude**, Solid/**Add**, **Blind 0.75 in**, +X.
8. PS8.8 Rename the part "Reducer Coupling".
9. PS8.9 (Optional) Model it again in a new Part Studio tab, with a single Revolve for both
   flanges and the transition, plus one hole extrude per flange.
10. PS8.10 Mass properties.
- **Self-check**: the **surface area**, to 3 decimals (in²). The screenshot shows **Volume =
  53.932 in³**. The analytic value is 53.9318: flange 1 14.8214 + transition 13.0542 (Pappus) +
  flange 2 26.0562, which matches. The predicted **surface area is 248.367 in²**.

## 3. Appearance and material

### PS9 Applying appearances (video)
- PS9.1 New parts and surfaces get colors from a **palette of 8** that cycles. Deleting a part
  doesn't recolor the others.
- PS9.2 Right-click a part or surface → **Edit appearance** opens a dialog with preset swatches, a
  **Mixer** for custom colors, **+** to save a custom color, and right-click on a custom color to
  **Delete** or **Update color**. You can also enter a **Hex** code or **RGB** values.
- PS9.3 A **transparency slider**. Appearances carry through to assemblies.
- PS9.4 **Face** appearances (right-click a face → *Add appearance to face*) and **feature**
  appearances (right-click a feature → *Add appearance to feature*) override the part color.
- PS9.5 Right-click a sketch or curve → **Edit sketch appearance** or **Edit curve appearance**.
- PS9.6 Pattern instances inherit the seed part's appearance.
- PS9.7 An **Appearance panel** (right side panel) lists the appearances of parts, surfaces,
  sketches and curves. Double-click, or right-click → *Edit appearance*, to change one.
- PS9.8 Appearances can be configured per configuration. That's out of scope until configurations
  exist.

### PS10 Applying materials (video; exercise screenshot `ex4-step17.png`)
- PS10.1 Select one or more parts, right-click → **Assign material**. The dialog has **Library**
  and **Custom** tabs.
- PS10.2 The Library tab has a library dropdown (**Onshape Material Library** plus any custom
  libraries), a **+** button, and a material dropdown you can search. The chosen material's
  properties are listed: Name, Density, Poisson's ratio, Young's modulus, Tensile yield strength,
  Ultimate tensile strength, Compressive yield strength, Ultimate compressive strength.
- PS10.3 The Custom tab takes your own property values, stored only in that part's metadata.
  Custom libraries hold custom materials you use often.
- PS10.4 **Mass and section properties** uses the material to show **Mass**, **Center of mass**
  and **Mass moments of inertia**. These values can be used in drawings and the BOM.
- Materials used in the exercises: **Polypropylene** (0.033 lb/in³), **Aluminum - 380**,
  **Aluminum - 1060**.

## 4. Part design concepts

### PS11 Dependencies (video)
- PS11.1 Features depend on features above them. Editing or deleting a parent can make its
  children fail with errors or warnings.
- PS11.2 Right-click a feature → **Show dependencies** lists its **parents above** and its
  **children below**.
- PS11.3 **Drag and drop** features in the list to reorder them. A child dragged above its parent
  fails.
- PS11.4 The order changes the result. For example, a face fillet placed *after* a Hole feature
  also rounds the hole edges; moving the fillet above the hole avoids that. See also PS17.6 and
  the Shell ordering in PS16.4.

### PS12 Reference planes (video)
- PS12.1 Every Part Studio has **Top**, **Front** and **Right**, which intersect at the origin.
- PS12.2 The **Plane** feature has a **Plane type** dropdown and an **Entities** selection:
  - **Offset**: parallel to a plane, a planar face or a mate connector's XY plane, at an **Offset
    distance**. It has a draggable arrow, an opposite-direction flip and a **Flip normal**
    checkbox.
  - **Plane point**: parallel to a plane, through a point or vertex.
  - **Line angle**: through a line or edge, at an **angle** to a plane, face, point or axis, with
    a flip.
  - **Point normal**: normal to a line or axis, through a point.
  - **Three point**: through three points.
  - **Mid plane**: halfway between two planes or faces. If they aren't parallel, it bisects the
    angle between them. **Flip alignment** chooses the other bisector.
  - **Curve point**: through a point, normal to the curve's tangent there (used for sweep
    profiles).
  - **Tangent**: tangent to a cylindrical face, through a point.
- PS12.3 Planes are used as sketch planes, as directions, and as mirror planes.

### PS13 Previewing feature generation (video)
- PS13.1 Editing a feature higher in the list **rolls the Part Studio back** to that feature.
- PS13.2 The **slider at the bottom of every feature dialog** switches the preview between the
  model *before* and *after* the feature.
- PS13.3 Every feature dialog except the last feature's has a **Final** button. It previews the
  whole Part Studio after full regeneration with the edit applied, so the effect on downstream
  features shows before you accept.

## 5. Applied features

### PS14 Fillet and chamfer (video; image `lesson-fillet-and-chamfer.png`)
- PS14.1 The Fillet dialog has an **Edge** tab and a **Full round** tab.
- PS14.2 Edge fillet: **Entities to fillet** accepts edges or faces; a face fillets all of its
  edges. **Tangent propagation** is on by default and continues through tangent-connected edges.
- PS14.3 **Measurement**: **Radius** or **Width** (the chord between the two tangent lines).
- PS14.4 **Control**: **Distance** (a circular profile), **Conic** (radius plus **Rho**),
  **Curvature** (radius plus **Magnitude**, G2).
- PS14.5 Set the radius by typing it or by dragging an arrow manipulator.
- PS14.6 Further options seen in the dialog: **Asymmetric**, **Partial fillet**, **Variable
  fillet** (radius and magnitude per vertex, plus **Points on edge** with a Location, Radius and
  Magnitude, and an *Add point on edge* button), **Allow edge overflow** (on by default),
  **Smooth fillet corners**, **Smooth transition**.
- PS14.7 Chamfer: **Entities to chamfer** (edges or faces). **Measurement** is **Offset** (the
  distance from the edge along the faces) or **Tangent** (the distance from where the adjacent
  faces' tangents meet).
- PS14.8 **Chamfer type**: **Equal distance**, **Two distances**, **Distance and angle**. For the
  last two there's an opposite-direction flip for all edges and a **Direction overrides** field
  for flipping individual edges.
- PS14.9 **Tangent propagation** is on by default for chamfers too.

### PS15 Hole feature (video, 6:31)
- PS15.1 A hole is placed at each selected **sketch point**: a standalone point, a vertex or a
  circle center. Selecting a whole sketch in the list uses every non-construction vertex in it.
- PS15.2 **Mate connectors**, implicit or explicit, can also locate holes. The **Select mate
  connectors** button next to the field lets you pick implicit ones, and clicking the connector
  icon in the field edits it.
- PS15.3 A **Merge scope** field sets which parts get cut.
- PS15.4 Dialog layout: **Inch / Metric** tabs, then **Simple / Counterbore / Countersink** tabs,
  then *Sketch points to place holes*, *Merge scope*, **Hole type**, **Size**, **Fastener fit**
  (for Clearance) or **Tap type / Pitch** (for Tapped), a **Thread class** checkbox, a diameter
  row, **Start plane**, **Termination** with a flip arrow, a depth row, a **tip angle** (118° by
  default), and the tapped depth or clearance rows. There's also a Final button.
- PS15.5 **Hole type**: **Drilled** (pick a drill size, diameter editable), **Clearance** (Size,
  Fastener fit Close/Normal/Loose), **Tapped** (Tap type, Size, Pitch, tapped depth), and
  **PEM®**.
- PS15.6 **Start plane**: **Start from part** (the default; the start moves to where the part
  begins, but only in the hole direction), **Start from sketch plane**, **Start from selected
  plane** (with a *Hole start plane* pick). The video shows counterbore depths that differ
  depending on this choice.
- PS15.7 **Termination**: **Through all** (through every part in the merge scope), **Blind**
  (depth), **Up to next** (optional offset from the tip, with a flip), **Up to entity** (the full
  diameter reaches the face or plane; a non-flat tip goes past it; *Offset from tip* adjusts).
- PS15.8 Collapsible **Diameter and Depth tolerance** controls feed the **hole callout**. There
  are more for counterbores and countersinks.
- PS15.9 **Hole callouts** in drawings only work on Hole-feature holes, not on extruded cuts.
  Standard-content **auto-size** also relies on Hole features.
- PS15.10 The dialog title shows a live callout summary, e.g. `Ø 5.3 mm THRU | ⌴Ø 9.75 …` or
  `M10x1.50 ↧ 20 mm`, and the feature-list row uses the same text.

### PS16 Shell (video)
- PS16.1 Shell makes constant-thickness walls without a sketch. Pick the **Faces to remove**
  (openings) and set the **Shell thickness**.
- PS16.2 The thickness goes **inward by default**. The opposite-direction arrow puts it outward.
- PS16.3 **Hollow** checkbox: a closed hollow body. You pick the part instead of faces.
- PS16.4 Shell **fails when the walls would self-intersect**, so reduce the thickness. Its
  position in the list matters: reorder it so that features such as tabs or holes aren't
  shelled.

### PS17 Exercise: Jackhammer Gear Cover (`ex3-*`, sketch `ex3-step4.png`)
It starts from Onshape's public "Jackhammer" document (**Make a copy to edit**), which has a
**Gear Cover** part built by 30 features in a "Base Features" folder. **cadrs can't reproduce the
starting geometry**, so only the workflow and the mass-property readout can be matched. Units:
**mm, kg**.
1. PS17.1 Copy the document. Check **Length = Millimeter** and **Mass = Kilogram**.
2. PS17.2 Look over the document: an image tab and the Part Studio with the Base Features folder.
3. PS17.3 **Shell**: remove the **bottom face**, **4 mm**.
4. PS17.4 Sketch on the marked top face: a vertical construction centerline through the origin
   and **6 points** symmetric about it. The two bottom points are **Concentric** with existing
   circular edges ("A"). Dimensions: **16**, **93**, **95**, and **4** (horizontal offsets).
5. PS17.5 **Hole**: Metric, **Counterbore**, the 6 points, Merge scope = the Gear Cover, **Hole
   type Clearance**, **Size M5**, **Fastener fit Close** → Ø **5.3 mm**, **Start from part**,
   **Through all**, counterbore **Ø 9.75 × 5 mm**. The thin shelled wall makes most of the holes
   look like plain holes.
6. PS17.6 **Reorder**: drag **Shell 1** to the bottom of the list. Now the counterbores are
   complete, and the shell wall wraps around them.
7. PS17.7 **Chamfer**: 2 marked edges (tangent propagation picks up the connected ones),
   **Measurement Offset**, **Distance and angle**, **2 mm**, **45°**.
8. PS17.8 **Fillet**, Edge tab: 3 bottom edges (plus their tangent chains), **Radius**,
   **Distance**, **1 mm**, Allow edge overflow ✓.
9. PS17.9 **Fillet** on the top edge (Edge of Loft 2): at Radius 3 mm it gets very thin where the
   angle between the faces changes. Switch **Measurement to Width = 3 mm** for an even look.
10. PS17.10 Mass properties. The material **Aluminum - 380** is already applied.
- **Self-check**: the **mass in kg**, to 3 decimals. The screenshot shows **Volume = 160 818.108
  mm³**, **Surface area = 82 534.703 mm²**, **Center of mass = (−2.102e−4, −59.006, 23.106) mm**,
  and **Lxx = 2040.215, Lyy = 616.821, Lxy = −0.014, Lxz = −0.002, Lyz = −28.253 kg·mm²**. So the
  mass ≈ 160 818 mm³ × ρ(Al 380). At ρ ≈ 2.76 g/cm³ that's ≈ 0.444 kg; the exact figure depends
  on Onshape's library density.

## 6. Multi-part Part Studios

### PS18 Multi-part Part Studio applications (video)
- PS18.1 Top-down design inside one Part Studio: a single master sketch or feature drives several
  **independent parts**. Each part has its own properties and is inserted into assemblies on its
  own. See P5.
- PS18.2 Parts can reference other parts: sketch on their faces, **Use** their edges, and extrude
  **Up to face** of another part.
- PS18.3 One feature (e.g. a single Fillet) can act on **several parts** at once.
- PS18.4 Guidance: don't put unrelated parts in one studio. A part can't be moved out of its
  studio later. Multiple identical instances belong in an assembly, not in the studio. Each part
  in the studio is its own BOM line.
- PS18.5 Split a part temporarily: a plane plus a **Split** feature, then **Shell** only one
  piece, then a **Boolean Union** to join them again. Also: model two pieces and **bridge** them.
  (Split part is a new feature.)

## 7. Advanced features

### PS19 Sweep (video)
- PS19.1 Sweep needs a **profile** (*Faces and sketch regions to sweep*) and a **Sweep path**. It
  can make a Solid, a Surface or a Thin body, and has the boolean tabs.
- PS19.2 The path can be sketch entities, a whole sketch, a curve, or **model edges**; several
  connected entities are fine.
- PS19.3 A closed profile or a face gives a solid. An open profile gives a surface or a thin body
  (**Thickness 1/2**, **Mid plane**). A closed profile with **Thin** gives a hollow sweep.
- PS19.4 Best practice: put the profile on a plane normal to the path's end (e.g. a **Curve
  point** plane) and tie it to the path with **Pierce** or **Coincident**.
- PS19.5 If the profile sits partway along the path, the sweep goes **both ways** along the whole
  path.
- PS19.6 The dialog shows a profile-control dropdown (default **None**), **Merge with all**, a
  **Merge scope** and **Final**.

### PS20 Loft (video)
- PS20.1 Loft blends between **two or more Profiles**, picked **in order**. Profiles can be
  sketches, planar or non-planar faces, surfaces, or a single point. It makes a Solid, a Surface
  or a Thin body.
- PS20.2 A **Reorder items** button (the ↑↓ icon in the Profiles header) turns on drag handles;
  click **Done** to finish.
- PS20.3 Profiles should have the **same number of vertices**, otherwise the loft twists. The fix
  is to **Split** a circle into 4 arcs to match a rectangle.
- PS20.4 **End conditions**: a **Start profile condition** and an **End profile condition**, each
  **Normal to profile**, **Tangent to profile**, **Match tangent** or **Match curvature** (the
  last two need adjacent faces), **Normal direction** or **Tangent direction** (with a picked
  vector), plus a **Start/End magnitude**.
- PS20.5 A profile made of more than one contour is invalid and shown **in red**.
- PS20.6 The Thin loft has thicknesses on each side and **Mid plane**.

### PS21 Exercise: Funnel (`ex4-*`, drawings `ex4-step2.png`, `ex4-step7.png`, `ex4-step8.png`, `ex4-step14.png`)
An elliptical-rimmed funnel with a flat tab handle, a lofted cone, a spout, and a swept bead
around the rim. Units: **inch, pound**.
1. PS21.1 New document. **Length = Inch**, **Mass = Pound**. Rename it "Funnel".
2. PS21.2 Sketch on **Top**: an **ellipse 6 × 4** (major axis along X) centered on the origin, an
   inner ellipse **offset 0.125** inward (the rim), and a horizontal construction centerline.
   Handle: three lines ending on the outer ellipse, forming a tapered tab on +X. Its top and
   bottom lines are **Symmetric** about the centerline. The end line is **1.5** long, **4.5** from
   the origin, and **105°** from the slanted lines.
3. PS21.3 **Extrude** (whole sketch, so the inner hole is excluded automatically), Solid/New,
   **Blind 0.125 in**, towards **+Z**.
4. PS21.4 Sketch on the **bottom face** of the rim: **offset** the inner rim ellipse **0.05 in**
   outward, into the rim. (Profile 1 = 2 regions: the inner ellipse region and the 0.05 band.)
5. PS21.5 **Plane**: **Offset** from Top, **3 in**, towards **−Z**. Rename it "Lower Plane".
6. PS21.6 **Plane**: **Mid plane** between Top and Lower Plane → "Middle Plane" (at −1.5 in).
7. PS21.7 Sketch on the Middle Plane: a **Ø2.5** circle with its center **Horizontal** to the
   origin, **0.5** from it, on the side away from the handle (as drawn in `ex4-step7.png`; confirm the sign against the CoM X = +0.556 readout).
8. PS21.8 Sketch on the Lower Plane: a **Ø0.5** circle, center Horizontal to the origin, **0.75**
   from it on the same side.
9. PS21.9 **Loft**, Solid/**Add**, profiles in order (Sketch 2 regions → Middle → Lower). **Start
   and End condition = Normal to profile**, **Start magnitude 0.5**, **End magnitude 1**.
10. PS21.10 **Shell**: remove the loft's top and bottom faces, **0.05 in**. It **fails**, because
    it also tries to shell the thin handle.
11. PS21.11 Edit the Loft: switch it to **New** (a separate part). **Final** shows that the shell
    error is gone. Accept.
12. PS21.12 **Boolean → Union** of the two parts. The first tool picked (Part 2) supplies the
    resulting part's properties. (Tip: a **Thin** loft would avoid the shell altogether.)
13. PS21.13 **Fillet** Edge, **Radius 0.5 in**, Distance, on the 4 vertical corner edges of the
    handle tab.
14. PS21.14 Sketch on **Front**: a **half-circle arc R0.125** centered on the handle's top outer
    edge (use **Pierce** or Coincident), closed by two vertical lines and a bottom line on the
    handle's bottom face. That makes a D-shaped bead profile, 0.25 wide.
15. PS21.15 **Sweep**, Solid/**Add**, profile = that sketch, **Sweep path = the 8 outer edges**
    of the rim and handle. Tip: right-click empty space → **Select → Create selection** →
    **Edges** tab → **Tangent connected**, click one edge, then **Add selection** fills the path.
16. PS21.16 **Extrude**, Solid/Add, the loft's bottom annular face, **Blind 1 in**, towards **−Z**
    (the spout).
17. PS21.17 Rename the part "Funnel". **Assign material → Polypropylene**.
18. PS21.18 Mass properties.
- **Self-check**: the **mass in lb**, to 3 decimals. The screenshot shows **Volume = 2.974 in³**,
  **Surface area = 84.098 in²**, **Center of mass = (0.556, −7.545e−5, −0.547) in**, and **Lxx =
  0.205, Lyy = 0.518, Lxy = 4.422e−5, Lxz = −0.059, Lyz = 4.476e−6 in²·lb**. Mass ≈ 2.974 ×
  0.033 ≈ **0.098 lb**. The loft shape depends on Onshape's loft math, so expect small
  differences in cadrs; compare the volume within a tolerance.

## 8. Patterning

### PS22 Introduction to patterns (video)
- PS22.1 There are four pattern features: **Linear**, **Circular**, **Mirror**, **Curve**.
- PS22.2 A **Pattern type** dropdown: **Part pattern** (has the New/Add/Remove/Intersect tabs;
  New makes separate parts, which isn't recommended for identical copies), **Feature pattern**
  (features from the list), **Face pattern** (faces; the fastest, and preferred when the result
  is the same).
- PS22.3 Feature pattern has a **Reapply features** option, which regenerates each instance with
  its own end conditions. It's slower, so use it only when needed.
- PS22.4 The face selection has a **Create selection** helper (e.g. *Pocket*, *Tangent
  connected*): pick one face, then **Add selection**.
- PS22.5 Every pattern except Mirror has **Skip instances**. Each instance shows a grey selection
  dot; clicking a dot, or box-selecting several, skips those instances, which turn light blue.
  The skipped instances are listed by grid index, e.g. `(2, 0)`, with a **CLEAR** link.
- PS22.6 The **Merge scope** / **Merge with all** decides which parts the pattern affects.

### PS23 Linear pattern (video)
- PS23.1 **Direction** can be a plane or planar face, a linear edge, a linear sketch entity, or a
  curve. Then a **Distance** (the spacing) and an **Instance count** (including the seed), with a
  flip arrow.
- PS23.2 **Centered**: spread the instances symmetrically about the seed.
- PS23.3 **Second direction**: its own Direction, Distance and Count, which makes a grid.

### PS24 Circular pattern (video)
- PS24.1 **Axis of pattern**: a circular edge, a cylindrical face, a **mate connector** (its
  primary Z axis), or a sketched circle. There's a **Select mate connector** button for implicit
  connectors, and the connector can be edited in place.
- PS24.2 **Angle** and **Instance count** (including the seed). With **Equal spacing** on, the
  instances fill the angle evenly: 3 over 60° gives a 30° step. With it off, the angle is the
  step between instances: 3 × 60° spans 120°. There's also a flip arrow and **Centered**.

### PS25 Curve pattern (video)
- PS25.1 **Path to pattern along**: a chain of sketch entities (spline, arc, circle), 3D curves or
  edges, connected end to end. It works best when the path starts on the seed.
- PS25.2 **Instance count** (including the seed), then **Equal spacing** along the whole path, or
  a fixed **Distance**.
- PS25.3 **Orientation**: **Tangent to curve** keeps each instance oriented to the path the same
  way the seed is.

### PS26 Mirror (video)
- PS26.1 Mirror copies parts, surfaces, faces or features across a **Mirror plane**: a planar
  face, a default plane, a Plane feature, or a mate connector (its secondary-axes plane). There's
  a Select mate connector button.
- PS26.2 The usual workflow is to model half and then **Part mirror** with **Add** to make one
  symmetric part. It's faster to model and to regenerate.
- PS26.3 There's a **Mirror type** dropdown (Part, Feature, **Face mirror**) and Create selection
  for faces.

### PS27 Exercise: Rocket Guidance Reflector (`ex5-*`)
It starts from Onshape's public "Rocket Guidance System" document (**Make a copy**): a rounded
square **Reflector** plate with a curved bottom made by a Revolve, a **Pattern Axis** mate
connector at the center of the top face, and a **Feature Sketch** with two triangles and a
rectangle. **cadrs can't reproduce the starting geometry.** Units: **mm, kg**.
1. PS27.1 Copy the document; check mm and kg.
2. PS27.2 Look it over. Tip: a mate connector is a full coordinate system you can reference.
3. PS27.3 **Extrude** Solid/**Remove**, the 2 triangle regions, **Up to face** = the curved
   bottom face (Face of Revolve 1), **Offset distance 10 mm**, Merge with all ✓.
4. PS27.4 **Fillet 6 mm** on the 12 edges of the cutouts.
5. PS27.5 **Circular pattern**, **Feature pattern** of Extrude 2 + Fillet 2, **Axis = the Pattern
   Axis mate connector** (its Z axis), **360°**, **4** instances, Equal spacing ✓, **Reapply
   features ✓**. It's needed because each copy has to keep its own 10 mm offset from the curved
   bottom.
6. PS27.6 **Hole**, Metric/**Simple**, placed at the **mate connector** (not a sketch point),
   Merge scope = Reflector, **Clearance M45 Close → Ø46 mm**, Start from part, **Blind 12 mm**,
   tip **118°**.
7. PS27.7 Sketch on the top face: one point **Vertical** below the origin, **26** from it.
   Shortcut **K** toggles mate-connector visibility.
8. PS27.8 **Hole**, Metric/Simple, **Tapped**, **Straight tap**, **M10 × 1.50 (Coarse)**, Fastener
   fit None; tap drill **8.5 mm**, Start from part, **Blind 20 mm**, **118°**, tapped depth
   **10.02 mm**, and a last row showing **6.653** (the tapped-clearance field; its label isn't
   visible).
9. PS27.9 **Linear pattern**, **Face pattern** of the 2 hole faces, Direction = a straight side
   edge, **26 mm**, **6** instances, **Skip instances** (2,0) and (3,0), the two that would fall
   inside the central clearance hole.
10. PS27.10 Show the Feature Sketch. **Extrude** Remove, the rectangle region only, **Blind 11
    mm**.
11. PS27.11 **Fillet 6 mm** on the 4 edges of the rectangular pocket.
12. PS27.12 **Mirror**, **Face mirror** of the 9 pocket faces (via **Create selection → Faces →
    Pocket**), Mirror plane = **Right**.
13. PS27.13 Mass properties. The material **Aluminum - 1060** is already applied.
- **Self-check**: the **mass in kg**, to 3 decimals. The screenshot shows **Volume = 754 429.926
  mm³**, **Surface area = 123 490.317 mm²**, **Center of mass = (−1.465e−5, −3.110e−5, 30.533)
  mm**, and **Lxx = 6072.444, Lyy = 5998.863, Lxy = −0.301, Lxz = 2.914e−4, Lyz = −0.001 kg·mm²**.
  At ρ(Al 1060) ≈ 2.705 g/cm³, mass ≈ 2.04 kg.

## Knowledge checks and survey
- Each exercise has a "Check to see if your model is correct → **Begin Self-Check**" page (page 3
  of each exercise). None were started. The quantity each one asks for is listed with its
  exercise above.
- A **Completion Survey** closes the course and is required for the certificate. It wasn't
  started.

## Cross-cutting requirements found in the course
- X1 **Region-based feature input** (PS1): features accept a whole sketch (holes found
  automatically) or any set of closed regions. Clicking a region toggles it. A sketch is
  auto-hidden after its first use, not consumed, and can be shown again with the eye icon.
- X2 **Solid / Surface / Thin** tabs and **New / Add / Remove / Intersect** tabs, with **Merge
  scope** and **Merge with all**, shared by Extrude, Revolve, Sweep, Loft, the patterns and Mirror
  (PS4, PS5). The boolean tab is picked automatically.
- X3 **End types**: Blind, Up to next / face / part / vertex (with offset), Through all, plus
  Symmetric, Second end position, Starting offset, Direction and Draft (PS4, PS7).
- X4 **Feature dialog chrome**: ✓ and ✗, a **rollback preview slider**, and a **Final** button on
  any feature that isn't last (PS13). Editing a feature rolls the studio back to it.
- X5 **Feature-list management**: folders (add, drag, unpack, delete), the filter with its `:`
  prefixes and quoted names, **Show dependencies**, **drag to reorder** with failure on bad
  order, and **Show regeneration times** (PS2, PS3, PS11).
- X6 **Parts list** with Parts, Surfaces and Curves groups, and right-click **Rename, Assign
  material, Edit appearance, Hide/Isolate, Delete**, etc. (PS2.8, `ex1-step5.png`).
- X7 **Mass and section properties** panel (Part and Face tabs): Parts to measure, a reference
  mate connector, Mass (override), **Volume**, **Surface area**, **Center of mass**, and the
  **inertia tensor**. This is the self-check readout of every exercise. The part name shows in
  red when there's no material.
- X8 **Workspace units** for Length and Mass (mm/inch, kg/lb). Two exercises are in inches and
  pounds.
- X9 **Materials** (a library with density) and **appearances** (palette, RGB/hex, transparency,
  per face or feature) (PS9, PS10).
- X10 **New features** beyond extrude: Revolve, **Plane** (8 types), **Fillet** (edge, full round,
  width, conic, variable), **Chamfer**, **Hole** (standards table: clearance, tapped, drilled;
  counterbore, countersink), **Shell**, **Boolean**, **Split**, **Sweep**, **Loft**, the **Linear,
  Circular and Curve patterns**, and **Mirror**.
- X11 **Mate connectors** as references (hole location, pattern axis, mirror plane), including
  implicit ones, and **K** to toggle their visibility.
- X12 **Create selection** helper (Faces: Pocket; Edges: Tangent connected) and **Skip
  instances** dots on patterns.
- X13 Sketch tools used again here: **Use** on part edges, **Pierce**, diametral dimensions to a
  centerline, **offset** of an ellipse, **ellipse**, sketching on part faces. See S13, S19, S20.
- X14 Sections and render modes from the view cube menu (PS2.9) come from the separate
  "Navigating a Document" course. They're noted here but not detailed.
