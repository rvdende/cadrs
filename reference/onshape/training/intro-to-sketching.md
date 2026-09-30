# Onshape "Introduction to Sketching": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Introduction to Sketching
(`learn.onshape.com/learn/course/fundamentals-sketching/...`), read on 2026-09-25. The
requirements below are paraphrased, not the course text. Exercise drawings and step screenshots
are in `intro-to-sketching/` (git-ignored, local only). Gap status is in
[intro-to-sketching-gaps.md](intro-to-sketching-gaps.md).

Each requirement has an ID (`S<lesson>.<n>`) so milestones and judges can refer to it.

## 1. Creating a sketch

### S1 Starting a sketch
- S1.1 Start a sketch from the toolbar **Sketch** button, or with **Shift+S**, then pick a plane
  or planar face.
- S1.2 Alternatively, right-click a plane or face and choose **New sketch**. The picked plane
  pre-fills the dialog.
- S1.3 While a sketch is active, you can see: the sketch feature dialog, the toolbar switched to
  sketch tools, and the sketch plane drawn in the viewport.
- S1.4 **P** hides or shows all planes. Each plane row in the feature list has its own show/hide
  toggle.
- S1.5 **N**, or right-click → **View normal to sketch plane**, turns the view normal to the
  sketch plane.

### S2 Completing a sketch
- S2.1 The green ✓ accepts the sketch; it's then listed in the feature list in creation order.
- S2.2 The red ✗ cancels. After a cancel, a **Restore** action appears at the top of the viewport
  for about 10–15 seconds to undo it.
- S2.3 Right-clicking a sketch (in the feature list or the viewport) offers **Edit**, **Rename**
  and **Delete**. Double-clicking the feature row edits the sketch.

## 2. Sketch entities

### S3 Lines and rectangles
- S3.1 **Line**: press, drag and release makes a single segment. Click-click makes chained
  segments, each one starting at the previous end. Double-clicking the last point, or leaving the
  tool, ends the chain.
- S3.2 **Midpoint line**: the first click is the midpoint, and the line grows symmetrically in
  both directions.
- S3.3 The rectangle dropdown has three types: **Corner**, **Center point**, **Aligned**.
  - Corner: press, drag and release between opposite corners. The sides are horizontal and
    vertical.
  - Center point: the first point is the center, and dragging sets a corner. The sides are
    horizontal and vertical.
  - Aligned: two clicks set the first side's angle and length, and a third click sets the
    perpendicular width.
- S3.4 Esc, or clicking the active tool again, leaves the tool.

### S4 Circles and arcs
- S4.1 **Center point circle**: click the center, then click to set the size, with a live
  preview.
- S4.2 **3-point circle**: three clicks on the circumference.
- S4.3 **3-point arc**: click the start, click the end, then a third click sets the bulge
  (radius).
- S4.4 **Tangent arc**: start from the endpoint of existing geometry. The arc stays tangent and
  gets a Tangent constraint.
- S4.5 **Center point arc**: click the center, then the start point (which sets the radius), then
  the end point.
- S4.6 **Line → tangent arc transition**: in the Line tool, hover an endpoint and move away to
  draw a tangent arc without switching tools.

### S5 Construction geometry
- S5.1 Construction geometry helps define the sketch but is ignored by features: no regions, and
  not used by extrude.
- S5.2 **Q** (or the toolbar toggle) makes new geometry construction while it's on, and converts
  selected geometry to construction.

### S6 Slot
- S6.1 The Slot tool turns a line, an arc or an edge (usually construction) into a slot. The
  entity's length is the distance between the slot's arc centers.
- S6.2 Clicking several entities in one use makes their slots equal-width. The width is edited by
  double-clicking its value.
- S6.3 Changing the source entity updates the slot. The equal relationship can be deleted like
  any constraint.

### S7 Polygons
- S7.1 **Inscribed** and **circumscribed** polygons, built on a construction circle.
- S7.2 Click the center, click to set the circle size, then move the mouse up or right for more
  sides and down or left for fewer. A final click commits.
- S7.3 Right after placement, a quick-dimension box sets the circle diameter.
- S7.4 3–50 sides, all equal. The size is set by the circle or by one side. The side count can be
  edited later by double-clicking its value.

### S8 Ellipse
- S8.1 Three clicks: the center, the major axis end, then the minor axis extent.

### S9 Sketch fillet and chamfer
- S9.1 **Fillet**: click a corner vertex and type a radius. Further clicks reuse the same radius.
  Press-and-drag on a vertex sizes the fillet live.
- S9.2 **Chamfer**: select connected entities or vertices. By default it's two equal distances
  (45°), with a drag arrow for live sizing. Then type the first distance, press Enter, type the
  second, press Enter.

### S10 Sketch points
- S10.1 The Point tool places points on the plane or on entities. Points can be dimensioned and
  constrained.
- S10.2 Inferred points, such as midpoints of lines and arcs, can be snapped to without creating
  them first (midpoint inference).

## 3. Sketch constraints

### S11 Using constraints
- S11.1 Inference adds constraints automatically while you sketch, including against geometry of
  the face the sketch is on.
- S11.2 Hovering entities "wakes up" their inference points. The type of the suggested constraint
  shows under the cursor. Vertices and midpoints get a yellow/orange highlight; horizontal and
  vertical alignment get an orange dashed line.
- S11.3 Holding **Shift** turns inference off temporarily.
- S11.4 Dragging existing geometry also infers. For example, dragging a circle center until it's
  vertically aligned with the origin adds a Vertical constraint.
- S11.5 Manual constraints work in two orders. **Selection first**: select entities, then click
  the constraint to apply it once. **Tool first**: click the constraint, then pick entities; the
  tool stays active until you leave it.
- S11.6 Hovering an entity shows its constraints, and hovering a glyph highlights its entities.
  Holding **Shift** while moving keeps glyphs visible so they can be clicked. The **Show
  constraints** checkbox shows all of them.
- S11.7 Click a glyph and press **Delete** to remove the constraint.
- S11.8 Glyph color: **white** means a constraint within this sketch; **blue** means it involves
  external geometry (origin, other sketches, part edges).

### S12 Available constraints (toolbar + shortcut)
| ID | Constraint | Key | Picks |
|---|---|---|---|
| S12.1 | Coincident | i | two entities (point/curve/line/plane) |
| S12.2 | Concentric | Shift+O | two or more arcs/circles |
| S12.3 | Parallel | B | two lines, or a line and an edge |
| S12.4 | Tangent | T | two entities (e.g. a line and a circle) |
| S12.5 | Horizontal | H | a line, or two points |
| S12.6 | Vertical | V | a line, or two points |
| S12.7 | Perpendicular | Shift+L | two lines, or a line and an edge |
| S12.8 | Equal | E | two lines, circles or arcs |
| S12.9 | Midpoint | Shift+M | a point and a line/arc/edge |
| S12.10 | Normal | Shift+K | a line and a curve, or a curve and a plane |
| S12.11 | Pierce | Shift+G | a point/curve and an edge crossing the sketch plane |
| S12.12 | Symmetric | Shift+Q | an axis line, then two entities |
| S12.13 | Fix | Shift+J | one entity; it becomes immovable |
| S12.14 | Curvature | Shift+U | two splines/conics (G2 continuity) |

All of them follow the S11.5 tool-first flow and exit with Esc or by clicking the tool again.

### S13 Dimensions
- S13.1 **While sketching**: after placing geometry, type a number and press Enter. This works for
  nearly all tools. Two-value tools (rectangles) take the horizontal value first; Enter then moves
  to the vertical value.
- S13.2 **Afterwards**: the **D** tool. Click the entities, click to place, type a value, press
  Enter.
- S13.3 **The first dimension scales the whole sketch** to match it.
- S13.4 Arcs and circles: clicking near the outside dimensions to the outside; clicking near the
  inside dimensions to the inside.
- S13.5 Two non-parallel lines give an **angle** dimension automatically.
- S13.6 Double-click a dimension to edit it; select it and press Delete to remove it.
- S13.7 **Driven** (reference) dimensions show grey; driving ones show black. A new dimension that
  would over-define the sketch is made driven automatically. The right-click menu toggles
  driving/driven.

### S14 Exercise: Basic Sketching (`intro-to-sketching/ex1-*`)
A plate with a Ø-pair of bosses: a large circle with a concentric inner circle on the left; a
body to the right with a centered rectangular notch; two Ø25 holes on the right tabs.
1. New document (mm). Sketch on **Top**, press N, hide planes with P.
2. Rough outline with circle, line and construction line, using inference.
3. Constrain it horizontally symmetric, with all lines horizontal or vertical. The construction
   line's end gets a **Midpoint** constraint on the notch's back line, and three lines are
   **Equal**.
4. The first dimension (**200**) rescales the whole sketch.
5. Remaining dimensions: 35 (ring width, radial), 50, 70. It's now fully defined (all black).
6. Two equal circles on the tabs, positioned with construction lines using Midpoint; Ø25 and 35.
7. Accept (✓), noting the cancel/restore safety net.
8. View normal, select the region → the **Area** readout at the bottom right (the course quiz
   checks it).

### S15 Exercise: Intermediate Sketching (`intro-to-sketching/ex2-*`)
A keyhole-shaped plate on **Front**: an R75 head, R125 waist arcs, a 125-wide base, and an inner
keyhole slot (R40 head made by a 40 offset, R15 bottom arc, 100 tall) plus an 80×20 rectangular
cut-out 25 from the base.
1. New document (mm), sketch on **Front**.
2. A vertical construction center line from the origin; a horizontal base half-line and a
   vertical side line (Q, H, V).
3. A 3-point arc up from the side line (its top end not vertically above the bottom).
4. **Mirror** the lines and arc about the construction line.
5. A **Tangent arc** between the two arc ends (the head).
6. Make the head arc's center Coincident with the top of the construction line.
7. Dimensions: R75, R125, 200, 50, 125.
8. **Offset** the head arc 40 inward (shortcut **O**).
9. A vertical line; 10. a tangent arc joining it to the offset arc; 11. **Mirror** both
   (coincident + tangent result); 12. a tangent arc joining the two bottom ends.
13. A **Center-point rectangle** on the center line.
14. Dimensions: R40, R15, 100, 20, 25, 80. It's now fully defined.
15. Accept. 16. Select the region → the **Area** readout.

## 4. Sketch tools

### S16 Sketch text
- S16.1 Text tool: draw a corner rectangle for the text box. A dialog takes the text, the font,
  bold/italic, and horizontal/vertical mirroring, with a live preview.
- S16.2 The text fills the box. Dimension only the width *or* the height (both over-defines).
  Position it with dimensions and constraints. Deleting its Horizontal constraint lets it rotate.
- S16.3 Right-click → **Edit text**. Text regions can be extruded (raised or cut).

### S17 Trim and extend
- S17.1 **Trim**: click an entity to cut it back to the nearest intersections. If there is no
  intersection, the entity is deleted. Press-and-drag across entities trims everything touched.
  Ctrl+Z undoes.
- S17.2 Trimming isn't needed to form regions; overlapping geometry already makes regions.
- S17.3 **Extend**: click an entity, move, and click to extend it to a boundary. Closed regions
  shade immediately.

### S18 Split
- S18.1 **Split** divides an entity at the clicked point into separate entities. The split points
  can be dimensioned and constrained.

### S19 Offset and mirror
- S19.1 **Offset** (O): pick an entity or a loop. Dragging a piece picks the whole chain. Click
  the arrow to flip the side. Click to place, then type the distance.
- S19.2 **Mirror**: pick the mirror line, then the entities. Adds **Symmetric** constraints
  automatically.

### S20 Use (project)
- S20.1 **Use** projects edges, faces, silhouettes or other sketch entities into the active sketch
  with a link to the source; the projection updates when the source changes.
- S20.2 The link is a constraint that can be deleted. A source that disappears puts the sketch in
  an error state.

### S21 Sketch imprinting
- S21.1 When sketching on a part face, the face's edges imprint on the sketch: the overlaps become
  selectable regions without projecting anything. It's on by default.
- S21.2 The **Disable imprinting** checkbox in the sketch dialog turns it off (for speed, and
  easier region picking).

## Cross-cutting requirements found in the course
- X1 A **Workspace units** setting (the exercises require mm).
- X2 **Region selection → Area readout** in the bottom-right status area (the exercises' self-check).
- X3 Sketching on **part faces** (S11.1, S20, S21).
