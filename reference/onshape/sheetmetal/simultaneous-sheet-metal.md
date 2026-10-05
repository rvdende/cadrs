# Onshape "Simultaneous Sheet Metal": requirements for cadrs

Source: Onshape Learning Center, topic **Sheet Metal**
(`learn.onshape.com/catalog?labels=["Topic"]&values=["Sheet Metal"]`), read on 2026-10-01, plus
the Onshape Help Center's 16 sheet metal pages (`cad.onshape.com/help/Content/PartStudio/sheet_metal_*.htm`
and `Drawing/drawing_flat_pattern_view.htm`, "Last Updated: September 24, 2026"), which give the
dialogs' exact fields, options, ranges and defaults that the course only shows on screen.

The topic lists 16 items: the self-paced course **Simultaneous Sheet Metal** and 15 standalone
videos. **All 15 videos are lessons of the course** (same titles and transcripts), so the course
pages are the source here. The course has 6 sections: an introduction, 10 feature lessons, 7
workflow lessons, 2 **Forms** lessons, 4 slide-based exercises and a completion survey. The two
Forms lessons ("Creating a Tag (Form)", "Form Feature") need a paid Learning Center membership
("Purchase Required") and were **not read**; the Help Center's Form page covers the feature instead
(SM20). No quiz, self-check or survey was started, and nothing was tried in Onshape.

The requirements below paraphrase the course and help text; they don't copy it. The raw text, the
lesson videos, frames from them, the exercise slides and the help images are kept locally only
(git-ignored), see [README.md](README.md) for the index.

Each requirement has an ID `SM<lesson>.<n>`. Items already covered by other courses are
cross-referenced, not repeated: [intro-to-part-studios.md](../training/intro-to-part-studios.md)
(`PS*`: Extrude PS4, Fillet/Chamfer, patterns and Mirror, Assign material PS10, Mass properties),
[intro-to-sketching.md](../training/intro-to-sketching.md) (`S*`: Use S20, sketch pattern),
[intro-to-drawings.md](../training/intro-to-drawings.md) (`D*`: templates, Insert view,
dimensions, view context menu), [derived-and-linking.md](../training/derived-and-linking.md)
(`DV*`: Derived, which today treats sheet metal as static, DV3.2),
[intro-to-assemblies.md](../training/intro-to-assemblies.md) (`A*`: in-context Part Studios) and
[essential-tips.md](../training/essential-tips.md) (`T*`: import, export rules).

## Course outline

| # | Section / lesson | Kind | Local material |
|---|---|---|---|
| 1 | Introduction to Simultaneous Sheet Metal | video 1:23 | `01-introduction/` |
| 2 | Sheet Metal Model | video 5:34 | `02-sheet-metal-model/` |
| 3 | Flange | video 2:18 | `03-flange/` |
| 4 | Hem | video 1:36 | `04-hem/` |
| 5 | Tab | video 1:15 | `05-tab/` |
| 6 | Make Joint | video 1:09 | `06-make-joint/` |
| 7 | Corner | one animated slide | `07-corner/slide-1.gif` |
| 8 | Bend Relief | one animated slide | `08-bend-relief/slide-1.gif` |
| 9 | Bend | video 2:10 | `09-bend/` |
| 10 | Finish Sheet Metal Model | video 0:48 | `10-finish-sheet-metal-model/` |
| 11 | Corner Break | video 2:37 | `11-corner-break/` |
| 12 | Non Sheet Metal Features | video 1:42 | `12-non-sheet-metal-features/` |
| 13 | Bend and Joint Table | video 1:44 | `13-bend-and-joint-table/` |
| 14 | Modeling in the Flat View | video 1:58 | `14-modeling-in-the-flat-view/` |
| 15 | Exporting a Flat Pattern | video 1:49 | `15-exporting-a-flat-pattern/` |
| 16 | Drawings | video 2:06 | `16-drawings/` |
| 17 | Importing Legacy Sheet Metal | video 1:23 | `17-importing-legacy-sheet-metal/` |
| 18 | Sheet Metal and Top-Down Design | video 2:28 | `18-sheet-metal-and-top-down-design/` |
| – | Forms: Creating a Tag (Form), Form Feature | paid, not read | (help: `raw/help-sheet_metal_form.txt`) |
| E1 | Exercise: Importing DXF & Bend (12 steps) | slides | `ex1-importing-dxf-bend/` |
| E2 | Exercise: Creating Sheet Metal Parts (14 steps) | slides | `ex2-creating-sheet-metal-parts/` |
| E3 | Exercise: Drawings (12 steps) | slides | `ex3-drawings/` |
| E4 | Exercise: Sheet Metal Rework (10 steps) | slides | `ex4-sheet-metal-rework/` |

## 1. The simultaneous model (SM1)

- SM1.1 Sheet metal parts are made in a **Part Studio**, alongside other sheet metal parts and
  ordinary parts (multi-part studio). Several sheet metal models can be active at once, and one
  model can make several parts.
- SM1.2 **Three views, always in sync**: the folded 3D model in the main viewport, the **flat
  pattern**, and the **table** of bends and joints. Editing in any one updates the other two at once
  (every feature rebuild recomputes the flat pattern and the table).
- SM1.3 Once a sheet metal part exists, a right-edge panel button **Sheet metal table and flat
  view** (tool icon at the right edge of the window, `help/icons/smtable-button@1x.png`) opens a
  panel docked on the right. From the top it has:
  - a **Sheet metal context** dropdown that picks which sheet metal model (named after its Sheet
    metal model feature, e.g. "Sheet metal model 1", or the name the user gave it) the panel shows;
  - the **Bends** table and the **Other joints** table, each with a caret to collapse it;
  - the **flat view**: its own 3D viewport (pan, zoom, rotate) with its own view cube, showing the
    flat pattern with dashed **bend centerlines** and bend tangent lines
    (`help/feature-tools/sheetmetalflatpatterntable-02.png`, `01-introduction/poster.jpg`).
  The same button closes it.
- SM1.4 **Cross-highlighting**: hovering or selecting a face, edge, vertex or bend in one view
  highlights the same entity in the other two; selecting a bend or rip in the model selects (and
  scrolls to) its table row; a table row highlights in both viewports.
- SM1.5 **Manufacturability check**: if the flat pattern would overlap itself, the Sheet metal
  model feature fails with "Collision in sheet metal flat pattern" and the flat view shows the
  overlap; choosing other bend edges clears it.
- SM1.6 While a sheet metal model is **active** (not finished), features act on it as sheet metal:
  a cut made with Extrude → Remove (and Hole, Move face, Boolean) through a wall is always
  **perpendicular to the wall**, not at the sketch's angle, and shows in the flat pattern. Extrude
  Add and Intersect aren't allowed on an active model in the folded view (use Tab, or the flat view,
  SM14).

## 2. Sheet metal model feature (SM2)

Toolbar: **Sheet metal model** (`help/icons/startsheetmetal-button@3x.png`) opens the first tool
of a toolbar group whose dropdown holds the other sheet metal tools (`02-sheet-metal-model` frames:
the group sits right of the surfacing tools). Dialog: `help/feature-tools/sheetmetal-dialog-convert-02.png`,
`sheetmetal-extrude-02.png`, `sheetmetal-thicken-02.png`.

- SM2.1 Three tabs (operation types): **Convert** (default), **Extrude**, **Thicken**. Below the
  tab's selections the dialog has three collapsible sections shared by all three: **General**,
  **Material**, **Relief**. The values set here become the defaults for every later feature on this
  model (flanges, hems, bends use "model bend radius", "model K factor", the model's minimal gap and
  reliefs).
- SM2.2 **Convert**: makes one sheet metal wall per face of the selected parts or surfaces (a block
  becomes six walls, six parts in the table until joined):
  - **Parts and surfaces to convert**; **Faces to exclude** (no wall for those faces);
  - **Edges or cylinders to bend**: each picked edge joins the two walls that meet there with a
    **bend**; edges not picked become **rips**; arcs/splines not picked become **tangent joints**.
    The **pick order matters**: it decides which walls end up connected and so the flat pattern's
    shape (re-picking in another order gives a better flat);
  - **Clearance from input** (offset from the input part, default 0) and **Include bends**
    (the clearance includes the bends);
  - **Keep input parts** (off by default: Convert consumes the input part).
  - Works on parts that revolve about one axis (cones) too.
- SM2.3 **Extrude**: sheet metal from sketch curves:
  - **Sketch curves to extrude** (entities or a whole open sketch from the Features list);
    **touching sketch entities become bends automatically**; separate chains become separate
    parts, which merge when a connecting entity is added;
  - **End type**: Blind, Up to next, Up to face, Up to part, Up to vertex; depth by value or by
    dragging the arrow manipulator; opposite direction arrow; **Symmetric** (Blind/Through all);
    **Second end position** (its own end type and depth);
  - an **arc or spline** extrudes as a **rolled wall** (rolled K factor, a tangent joint in the
    table, no bend line on the flat); **Arcs to extrude as bends** makes picked arcs real bends
    (listed in the Bends table with the radius greyed out, since it comes from the sketch; a
    centerline on the flat).
- SM2.4 **Thicken**: **Faces or sketch regions to thicken** (sketch regions, planar surfaces or
  part faces; each picked face is its own wall/part in the table until joined); **Tangent
  propagation** (picks tangent-connected faces); **Edges or cylinders to bend**; Clearance from
  input and Clearance includes bends. Thicken **doesn't consume** the input part. Thickening an arc
  as a bend works as in SM2.3.
- SM2.5 **General**: **Thickness** (e.g. 0.02 in) with an opposite-direction arrow for the side the
  thickness goes; **Bend radius** (inner); **Flip direction up** (which side faces up in the flat
  view and drawings; decides "Up"/"Down" bend directions).
- SM2.6 **Material**: **Bend calculation**: **K Factor** (default; neutral axis as a fraction of
  thickness), **Bend allowance** (neutral arc length between the bend's tangent lines), **Bend
  deduction** (sum of flange lengths to the apex minus the flat length). With K Factor: **Default
  bend K Factor** (default **0.45**) and **Rolled K Factor** (default **0.5**). The chosen
  calculation names the last column of the Bends table (SM13.3).
- SM2.7 **Relief**:
  - **Minimal gap** (smallest gap at a rip; e.g. 0.1 in in the help screenshot);
  - **Corner relief type**: Square – Sized, Rectangle – Scaled, Round – Sized, Round – Scaled,
    Closed, **Simple** (default in the screenshot); **Corner relief scale** 1.00–2.00 (scaled
    types), **Corner relief width/diameter** (sized types) (images
    `help/feature-tools/sheetmetal-corner-*.png`, `sheetmetalcorner-*.png`, flat and 3D for each);
  - **Bend relief type**: Rectangle – Scaled, **Obround – Scaled** (default in the screenshot),
    Tear; **Bend relief depth scale** 1.00–5.00 (a value of 1 makes an obround relief just touch
    the bend; above 1 adds `(scale − 1) × bend radius` of depth), **Bend relief width scale**
    0.0625–2.00 (width = `thickness × scale`). Onshape remembers these two scales as defaults
    across documents.
- SM2.8 Best practice taught: **one Sheet metal model feature per part**, each with its own flat
  pattern and table (the context dropdown, SM1.3), and **rename the feature** to name the
  context (e.g. "Enclosure"). Lesson 17: when the original is no longer needed after converting
  an import, add a **Delete part** feature.

## 3. Flange (SM3)

Dialog `help/feature-tools/sheetmetalflange-dialog-01.png` (and `-03.png` for partial flanges).

- SM3.1 **Edges or side faces to flange**: one or more; the same parameters apply to each, and
  each gets a wall joined by a bend.
- SM3.2 **Flange alignment**: **Inner** (default; inner face of the new wall on the edge),
  **Outer**, **Middle**, **Hold line** (the bend starts at the edge)
  (`help/feature-tools/flange-alignment-01.png`).
- SM3.3 **End type**: **Blind** with **Distance** (measured from the virtual sharp, where the outer
  faces of base and flange meet, to the flange tip; default 0.025 m in the screenshot), **Up to
  entity**, **Up to entity with offset**.
- SM3.4 **Angle control**: **Bend angle** (default 90 deg) with an opposite-direction toggle;
  **Align to geometry** (parallel to an edge, face, plane or mate connector); **Angle from
  direction** (an angle 1–359° from a reference).
- SM3.5 **Automatic miter** (default on): flanges from the same feature that meet are trimmed or
  extended into a miter; off → a **Miter angle** field.
- SM3.6 **Use model bend radius** (default on); off → a **Bend radius** field for this feature.
- SM3.7 **Partial flange** (off by default): **Overall parameters** **Per edge**/**Per chain** with
  a **Flip sides** toggle; **Hold adjacent edges** (default on); **End conditions**: a bound type
  (Blind with Distance, Up to entity, Up to entity with offset) and an optional **Second bound**
  with the same options; on-screen arrow manipulators at both ends (`-03.png`).
- SM3.8 A flange's wall can be moved later with **Move face** (direct edit).

## 4. Hem (SM4)

Dialog `help/feature-tools/shmetal-hem-dialog.png`.

- SM4.1 **Edges or side faces to hem**, a **flip** arrow for the side.
- SM4.2 **Hem type**:
  - **Straight** (default; folded back 180°): **Flattened** (default on: hem lies flat with the
    model's minimal gap) or an **Inner radius**; **Total length** (from the outermost edge to the
    hem's end; 12.5 mm in the screenshot);
  - **Rolled**: **Inner radius** and **Angle** (>180°, e.g. 200° or 270°; how far it curls back);
  - **Tear drop**: **Inner radius**, **Minimal gap** (use the model's) or a **Gap**, **Total
    length**.
- SM4.3 **Hem alignment**: **Outer** (default; the hem's outside on the original edge) or **In
  place** (the hem bend starts above the edge).
- SM4.4 **Corner type** where hems meet: **Simple** (default; straight cut in the flat) or
  **Closed** (closes the corner, a more complex cut in the flat).
- SM4.5 Hems are listed in the Bends table (radius and angle) and can be reordered there, not
  edited. The dialog remembers the last values for the next hem.

## 5. Tab (SM5)

Dialog `help/feature-tools/sheetmetaltab-dialog.png`; picks only in the folded view.

- SM5.1 **Tab profile**: one or more closed sketch profiles parallel to a wall.
- SM5.2 **Flange to merge**: filled in automatically with the wall under the profile; can be
  changed; any number of walls parallel to the sketch, even on other parts (a tab can bridge two
  walls of one model).
- SM5.3 **Subtraction offset** (default 0) and **Subtraction scope**: walls (or ordinary parts) to
  cut a clearance pocket around the tab out of: the typical fix for a tab interfering with a
  mating part (the help's example uses 0.05 in).
- SM5.4 Tabs show in the flat pattern. Walls must be parallel and cross the profile, else no tab
  appears on them.

## 6. Make joint and Modify joint (SM6)

Dialogs `help/feature-tools/sheetmetalmakejoint-dialog.png`, `modify-joint-02.png`,
`sheetmetaljoint-dialog.png`.

- SM6.1 **Make joint** joins two existing walls that meet (or extends them to meet): **Edges or
  side faces to join** takes exactly two (two edges, two faces, or one of each).
- SM6.2 Joint type **Rip** (default) with a style: **Edge joint** (both walls extended to meet at
  their inner edges, minimal gap between), **Butt joint – Direction 1** (one wall's outer face
  runs past the other; only for **90°** joints), **Butt joint – Direction 2** (the other way round).
  Non-90° rips must be Edge joints.
- SM6.3 Joint type **Bend**: a bend between the walls; **Use model bend radius** or a custom
  radius.
- SM6.4 **Modify joint** isn't on the toolbar: editing a bend or rip in the table (SM13) creates a
  **Modify joint** feature (own icon, `help/icons/joint-button@1x.png.png`) that can be edited like
  any feature: **Joint** (the entity), type **Bend / Rip / Tangent** (tangent: for circular
  faces), rip style, bend radius, and **Use model K Factor** off → **Bend calculation** + a custom
  K Factor (−1.50 to 1.00), Bend allowance or Bend deduction.

## 7. Corner (SM7)

Dialog `help/feature-tools/sheetmetalcorner-dialog.png`; course slide `07-corner/slide-1.gif`.

- SM7.1 Overrides one corner's relief (the cut where two bends meet) independently of the model
  setting: pick a **face, edge or vertex** of the corner.
- SM7.2 **Corner relief type**: the six of SM2.7. Sized types take a size (square side / round
  **diameter**, e.g. Round – Sized 3.3 mm in exercise E2), scaled types a **Corner relief scale**
  1.00–2.00 (1.5 in the screenshot); Closed and Simple take nothing.
- SM7.3 One Corner feature per corner (add more for more corners).

## 8. Bend relief (SM8)

Dialog `help/feature-tools/sheetmetalbendrelief-dialog.png`; slide `08-bend-relief/slide-1.gif`.

- SM8.1 Overrides one bend relief (the cut where a bend ends at a free edge): pick a face, edge or
  vertex of the bend end.
- SM8.2 **Bend relief type**: **Square – Sized**, **Rectangle – Scaled**, **Obround – Scaled**,
  **Obround – Sized**, **Tear**. Scaled: depth scale 1.00–5.00 and width scale 0.0625–2.00. Sized:
  a **Bend relief depth** (0.25 in in the screenshot).
- SM8.3 **Extend bend relief**: flips the relief cut and runs it to the end of the sheet (for a
  flange whose relief collides in the flat) (`bendrelief-extendbefore/after.png`).

## 9. Bend (SM9)

Dialog `help/feature-tools/bend-dialog.png`.

- SM9.1 Folds a flat sheet along a line: the way to turn an **imported DXF/DWG** flat into a 3D
  part (exercise E1).
- SM9.2 **Bend line**: a line or edge; it needn't lie on the face (it's projected), needn't cross
  it fully (it's extended to the face's edges), needn't be in any sketch, may span several cuts in
  the face, and may be at any angle to the face.
- SM9.3 **Sheet metal face to bend**: exactly one face per Bend feature (filled in automatically);
  **Hold opposite side** toggle picks which side moves.
- SM9.4 **Bend alignment** (six): flat-pattern based **Bend line** (default; line at the bend's
  middle), **Hold line** (bend starts at the line), **Hold other line** (bend ends at the line);
  folded based **Inner**, **Outer**, **Middle** (the bent wall's inner/outer/mid face on the line)
  (`smb-bends1-02.png`, `smb-bends2-02.png`).
- SM9.5 Angle control: **Bend angle** 1–359° (default 90) with an **opposite angle** toggle;
  **Align to geometry** (parallel to an edge, face, plane or mate connector); **Angle from
  direction**.
- SM9.6 **Use model bend radius** and **Use model K Factor** (both default on); off → custom
  values. A custom K factor changes the bent length in the folded model only: **a Bend never
  changes the flat pattern's size**.
- SM9.7 A bend may not collide with an earlier Bend or Corner (error); bends on flange and hem
  faces are allowed. Bend allowance/deduction are edited in the model or the table, not here.

## 10. Finish sheet metal model (SM10)

- SM10.1 **Finish sheet metal model** (`help/icons/endsheetmetal-button@1x.png`): pick one or more
  sheet metal parts; they become ordinary solids for every later feature (angled holes, weld
  fills, sweeps, fillets of any kind), which **don't show in the flat pattern** (exercise E4: a
  warning in the dialog says so).
- SM10.2 Only needed for such post-fabrication features. Rolling back above it (or suppressing or
  deleting it) makes the part active sheet metal again. The flat pattern and table of a finished
  part stay available (the context dropdown still lists it, E4 step 10).

## 11. Corner break (SM11)

- SM11.1 Fillet or chamfer the **corners** (corner edges through the thickness, or their vertices)
  of a sheet metal part, picked in the folded **or the flat** view, including corners made by
  relief cuts. Tabs **Fillet** / **Chamfer**.
- SM11.2 Fillet: Measurement **Radius** or **Width**; Control **Distance**, **Conic** (Rho) or
  **Curvature** (Magnitude); **Asymmetric** with a second radius and flip; **Allow edge overflow**
  (Distance and Conic).
- SM11.3 Chamfer: Measurement **Offset** or **Tangent**; type **Equal distance**, **Two
  distances**, **Distance and angle**, with an opposite-direction toggle.
- SM11.4 After a Corner break, joints' type and style **can't be edited** in the table any more
  (the table greys them out, `help/feature-tools/sm-cornerbreak-05.png`): add corner breaks last.

## 12. Other features on sheet metal (SM12)

- SM12.1 On an active model in the folded view: **Extrude → Remove** cuts (perpendicular to the
  wall, SM1.6), **Tab** to add; **Fillet** and **Chamfer** on corners (picked at the very corner).
  A fillet keeps the original sharp edge as the construction edge for later features (a flange on
  a filleted edge uses the sharp edge; Move face can then pull it back).
- SM12.2 **Patterns and mirrors** work as for any part: **Part pattern** (e.g. a linear pattern of
  the whole part along the thickness), **Face pattern** / **Face mirror** of walls and tabs (flanges
  and their bends come along), feature patterns allowed but slower. The flat updates too.
- SM12.3 Sketch patterns of cut-outs, centered with equal constraints (sketching, S*).

## 13. Bend and joint table (SM13)

- SM13.1 **Bends** table columns: **#**, **Name** (Bend A, Bend B, …; the same labels float next
  to each bend in the viewport and the flat view), **Radius** (units), **Angle (deg)**, **Bend
  direction** (Up/Down, from Flip direction up), and the calculation column **K Factor** /
  **Bend allowance** / **Bend deduction** (per SM2.6).
- SM13.2 **Other joints** table: **Name**, **Type** (Rip, Bend, Tangent), **Style** (Edge joint,
  Butt joint direction 1/2) for rips and tangent joints.
- SM13.3 Editing: double-click a **Radius** cell → new radius (adds a Modify joint feature, or
  edits the Bend feature if the bend came from one); double-click the calculation cell → per-bend
  value; a value out of range turns the cell **red** with a tooltip giving the valid range.
- SM13.4 Row context menu: **Move up**, **Move down** (bend order, for manufacturing), **Convert
  to rip** (the row moves to Other joints and a Modify joint feature appears); a rip's **Type**
  dropdown converts it back to a bend; its **Style** dropdown changes the rip style.
- SM13.5 Rows: click selects, click again deselects, multi-select; cross-highlighting (SM1.4).
  Hems are listed (radius, angle) and can be reordered but not edited (SM4.5). Rolled walls appear
  as tangent joints, not bends (SM2.3).

## 14. Modeling in the flat view (SM14)

- SM14.1 Right-click the flat pattern → **New sketch**: a sketch on the flat pattern's face
  (manufacturing marks, cut-outs that must have an exact flat size, tabs, hole patterns).
- SM14.2 Extruding such a sketch opens an **abbreviated Extrude** dialog (Add / Remove only,
  `help/feature-tools/extrude2_abbrev_dialogbox.png`): material is added to or removed from the
  flat pattern and the folded part follows, wrapping cuts **across bends** with the exact flat
  size (the lesson's 0.5 × 6.0 in slot) and adding tabs in the flat.
- SM14.3 A 3D Extrude can't be reused as a flat-pattern extrude (a regeneration warning).
- SM14.4 Visible flat-pattern sketches can be exported with the DXF (SM15.3).

## 15. Exporting a flat pattern (SM15)

Dialog `help/feature-tools/sheetmetal-export-01-03.png`.

- SM15.1 Flat view right-click → **Export DXF/DWG of flat pattern** opens **Export as DXF/DWG**:
  **File name** (default "<document> - Flat pattern of <part>", or from **export rules** with a
  "View export rules" link), **Format** DXF/DWG, **Version** (e.g. 2000), **Scope**.
- SM15.2 Scope: **Single flat pattern part only**, **All flat pattern parts in the current model**
  (every part of that Sheet metal model), **All flat pattern parts in the Part Studio**.
- SM15.3 Options: download / store as a tab / email (as other exports, T*); **Export splines as
  polylines**; **Set z-height to zero and normals to positive** (default on); **Include bend
  centerlines** (default on); **Include bend tangent lines**; **Include counterbore and countersink
  lines**; **Include form feature outlines**; **Include form feature centermarks**; **Include
  visible sketches** (default on).

## 16. Drawings (SM16)

- SM16.1 Flat view right-click → **Create drawing of flat pattern**: template and view type
  dialog, then a new drawing tab with Insert view armed to place the flat pattern; projected views
  of it may follow.
- SM16.2 In any drawing: Insert view → **Insert** → filter **Flat patterns** → pick one → place
  (a flat pattern view can only come from this Insert dialog). Folded views are ordinary part
  views (D*). Exercise E3 also uses Parts list → **Create drawing of <part>** with the **ANSI_A
  MM** template and **Four views**, then **Insert sheet**.
- SM16.3 Flat pattern views show **bend lines** (up and down bends with their own line weight and
  colour in the view properties), **bend notes** (direction, angle, radius, e.g. "UP 90° R 0.2"),
  form outlines and centermarks, and counterbore/countersink outer diameters.
- SM16.4 **Bend notes**: drag a note's node to move it; drop it near a bend line to reattach it;
  right-click a note or the view → **Hide bend notes**.
- SM16.5 Flat view context menu → **Show/hide** → **Hide bend lines** / **Show bend lines**;
  **Tangent edges** → Hidden / Solid / Phantom.
- SM16.6 Flat patterns are dimensioned like any view.

## 17. Importing legacy sheet metal (SM17)

- SM17.1 An imported folded part (STEP etc.) becomes active sheet metal with **Sheet metal model →
  Thicken** (the key takeaway says Convert; the video uses Thicken): pick a face with **Tangent
  propagation** so the whole skin is picked; the flat preview appears at once.
- SM17.2 Without bends picked, the flat only shows tangent edges where bends are; pick the bend
  cylinders in **Edges or cylinders to bend** to get real bends with centerlines.
- SM17.3 Enter thickness and bend radius (measure them on the import, Measure tool), flip the
  thickness side if needed. Formed features (louvres, lances) should be removed first (direct
  edit). Errors are diagnosed from the flat pattern. Delete the original with Delete part.

## 18. Sheet metal and top-down design (SM18)

- SM18.1 Sheet metal is modelled around a **space allocation** ("master model") part, derived into
  the sheet metal Part Studio (Derived, DV*): Convert the derived part (excluding two faces,
  picking bend edges, Keep input part) → "Enclosure"; Thicken the other two faces → "Cover".
- SM18.2 Each Sheet metal model feature renamed after its part; the context dropdown lists the
  names (SM1.3).
- SM18.3 Editing the master part in its own studio updates both sheet metal parts through Derived
  (workspace reference); a version reference stops that. **Derived must carry active sheet metal
  across** (today DV3.2 turns it static), or at least derive the master part into the studio where
  the sheet metal is made, which is what the lesson does.
- SM18.4 Alternatively make the sheet metal in an in-context Part Studio of an assembly (A*), which
  doesn't update automatically.

## 19. Jog and Loft (help only, not in the course) (SM19)

- SM19.1 **Jog** (`help/feature-tools/sm-jog-01.png`): an S/Z offset with two opposite bends at a
  **Bend line** on one **Sheet metal face to bend**; Hold opposite side; Bend alignment (as
  SM9.4); angle control as SM9.5 (default 90°); Use model bend radius / K factor; **Bounding
  type** Blind (**Jog offset**, e.g. 0.5 cm) / Up to entity (with an Offset distance) / Thickness
  (a factor of the model thickness); **Jog offset anchor** Inside (default) / Nominal / Outside;
  **Preserve material** (default on: the flat doesn't grow).
- SM19.2 **Sheet metal Loft**: New or Add (Merge scope: one active model); **Profile 1**, **Profile
  2** (region, face, edge or point); **Connections** with draggable manipulators; **Rip** at the
  connection; **Chordal tolerance**; General / Material / Relief as SM2.5–SM2.7 when New.

## 20. Forms (help only; course lessons paid) (SM20)

- SM20.1 **Form** places a formed feature (louver, lance, dimple, emboss) from the current
  document, other documents or a **library** (Onshape ships a sheet metal forms library) at
  **Locations** (sketch points, vertices or mate connectors) on **Target faces**, with an opposite
  direction toggle and the form's configuration variables (e.g. Length, Width, Height, Angle).
- SM20.2 A form is authored in its own Part Studio with a **Tag (Form)** feature: an **Add** part
  (material added), a **Subtract** part (material removed) and an origin mate connector; an
  optional construction-only sketch shows on the flat pattern.
- SM20.3 Forms can't touch side walls, rolled walls, rips, joints or corners; corner breaks don't
  apply to form edges; flat exports and flat drawing views can include form outlines and
  centermarks.

## Exercises

- **E1 Importing DXF & Bend** (public doc "Exercise: Import DXF"): sketch on Top → **Insert
  DXF/DWG** (millimetres, `Flat Pattern.DXF`) → Sheet metal model **Thicken** of the 7 closed
  regions → show the sketch → six **Bend** features along the DXF's bend lines (alignment
  **Inner**, Hold opposite side as needed; Shift+Enter accepts and starts the next) → Assign
  material **Carbon steel** → Mass properties (kg) checked by a quiz. Target: a U-shaped enclosure
  tray with a fan grille and slots (`ex1-importing-dxf-bend/cover.jpg`).
- **E2 Creating Sheet Metal Parts** (new document, mm and kg): Front-plane sketch of lines and an
  arc → Sheet metal model **Extrude** → **Flange** on two back edges, then **Partial flange** →
  **Hem** on two back edges → a sketched rectangle and **Tab** merged onto a flange → two more
  **Flanges** at the front-right corner → **Make joint** (Rip, **Butt direction 1**) → **Corner**
  (Round – Sized, 3.3 mm) at the rear-left corner → Carbon steel → mass check.
- **E3 Drawings** (public doc "Exercise: Drawings", part "Sheet Metal Box"): export the flat
  pattern as DXF/DWG from the flat view → Parts list → Create drawing (ANSI_A MM, Four views) →
  dimension the folded views → Insert sheet → Insert view → Flat patterns filter → place → drag bend
  notes → dimension the flat → Hide bend notes.
- **E4 Sheet Metal Rework** (public doc "Exercise: Sheet metal rework", part "Lower Enclosure"):
  **Finish sheet metal model** (warning) → sketch with Use of an obround edge → Plane (Plane point)
  → 2 × 8 mm rectangle → **Sweep** (Solid, Add) → **Mirror** (Feature mirror, Reapply features)
  → two Fillets → the flat pattern (Context: Lower Enclosure) shows none of it.

The exercise documents are public Onshape documents we don't have; cadrs needs its own stand-ins
(as for the other courses: `fixtures/*_standin.cadrs` built by `cadrs_core::samples`), including a
flat-pattern DXF for E1.

## Cross-cutting (X)

- X1 **Toolbar group**: Sheet metal model, then a dropdown with Finish sheet metal model, Flange,
  Hem, Tab, Bend, Jog, Form, Loft, Make joint, Corner, Bend relief, Corner break; the table/flat
  view toggle on the right edge; all of them findable in **Search tools**.
- X2 **Feature list**: each sheet metal feature with its own icon; Modify joint features appear
  from table edits; Finish sheet metal model listed like any feature; renaming a Sheet metal model
  renames its context.
- X3 **Parts list**: sheet metal parts are parts (rename, material, appearance, mass properties,
  export); a sheet metal part's right-click menu offers **Create drawing**.
- X4 **Every edit goes through the command/undo layer** (CLAUDE.md): table edits become features
  (Modify joint), so undo/redo works on them like on any feature.
- X5 **Units**: thickness, radii, gaps and distances in the document's length unit (mm default);
  angles in degrees; scales unitless with the ranges above, validated with a red field and range
  tooltip.
- X6 **Errors**: "Collision in sheet metal flat pattern" and other failures shown like other
  feature errors (red feature, tooltip) with the flat view showing the problem.
- X7 **Stand-ins and scenarios**: one stand-in document per exercise (E1–E4) plus a headless
  scenario per lesson, with screenshots compared side by side with the course frames.
