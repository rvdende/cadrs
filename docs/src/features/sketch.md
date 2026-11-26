# Sketches

A sketch is a flat drawing on a plane: lines, arcs, circles and other curves, held in shape by
constraints and dimensions. The closed regions of a sketch are the profiles that features such
as Extrude and Revolve turn into solids. A sketch is a feature like any other: it sits in the
Part Studio's feature list, and editing it later rebuilds everything after it.

## Starting a sketch

Click **Sketch** in the Part Studio toolbar, press <kbd>Shift</kbd>+<kbd>S</kbd>, or right-click
in the view and choose **New sketch**. A new "Sketch 1" appears in the feature list and the sketch
dialog opens with its **Sketch plane** field waiting.

Pick the plane to draw on, in the view or in the feature list:

- one of the default planes (**Top**, **Front**, **Right**);
- a Plane feature;
- a flat face of a part.

If a plane was already selected when you started, it becomes the sketch plane straight away.
Until the sketch has a plane, its row is red and the sketch toolbar is greyed out.

The view doesn't turn by itself unless you ask it to: press <kbd>N</kbd> to look straight at the
sketch plane, or turn on **View normal to the sketch plane** in
[Preferences](../reference/preferences.md) › Navigation to have it turn whenever a sketch opens.

## The sketch dialog

Besides the plane, the dialog has four options:

- **Disable imprinting:** on a part face, the face's edges normally become part of the sketch, so
  what you draw splits the face into regions. Tick this to keep the face's edges out.
- **Show constraints:** shows the constraint glyphs beside the geometry.
- **Show expressions:** shows a dimension's expression (such as `#width / 2`) instead of its
  value.
- **Show errors:** highlights constraints that are in conflict.

The display options are remembered from one sketch to the next, except **Show expressions**,
which starts off each time.

At the bottom of the dialog, the diagnostics button opens the
[Profile inspector and Constraint manager](#checking-a-sketch). When you edit a sketch that isn't
the last feature, **Final** shows the Part Studio's final result while you work.

## Drawing

The sketch toolbar's tools are grouped under buttons; a button with ▾ holds related tools.

| Tool | How it draws |
| --- | --- |
| **Line** (<kbd>L</kbd>) | Click, click, click: each segment starts where the last ended. Drag to draw a single segment. <kbd>Esc</kbd> ends the chain. While chaining, <kbd>Shift</kbd>+<kbd>A</kbd> makes the next segment a tangent arc. |
| **Midpoint line** | The first click is the line's middle, the second its end. |
| **Corner rectangle** (<kbd>G</kbd>) | Two opposite corners. Hold <kbd>Alt</kbd> for a square. |
| **Center point rectangle** (<kbd>R</kbd>) | The centre, then a corner. |
| **Aligned rectangle** | The first side, then the width. |
| **Center point circle** (<kbd>C</kbd>) | The centre, then a point on the circle. |
| **3 point circle** | Three points on the circle. |
| **Ellipse** | Under the Circle button. |
| **3 point arc** (<kbd>A</kbd>) | Start, end, then a point on the arc. |
| **Tangent arc** | Click the end of a line or arc, then the arc's end. |
| **Center point arc** | The centre, then the arc's ends. |
| **Inscribed polygon**, **Circumscribed polygon** | The centre, then the size. Move up or right for more sides, down or left for fewer. |
| **Bézier curve** | Four clicks: the start, two control points, the end. |
| **Point** (<kbd>Shift</kbd>+<kbd>S</kbd>) | A single point. |
| **Text** | Draw a box: its lower edge is the baseline and its height the height of the capitals. Type the text in the dialog that opens. Right-click a text and choose **Edit text** to change it. |

The rectangle, circle and similar tools open a small value box as soon as the shape is placed,
so you can type its size right away; what you type becomes a dimension.

**Insert DXF, DWG, SVG, EPS or PDF** brings a 2D drawing into the sketch. Pick a file from the
**Current folder** or **Other folders** tabs, or **Import…**. **Units** applies to DXF and DWG
files; **Use file origin position** keeps the drawing where it is relative to its own origin
(untick it to centre it on the sketch origin). Lines, arcs, circles, polylines and splines come
in joined at their ends, so they close regions.

**Spline**, **Intersection**, **Insert image** and **Linear pattern** are on the toolbar but not
available yet.

### Inference

As you draw, the pointer snaps to points, midpoints, intersections, curves, and horizontal and
vertical guides lined up with points you passed over. Each snap adds the matching constraint
(coincident, horizontal, and so on) along with the new geometry; the small grey glyph beside the
pointer shows what will be added. Hold <kbd>Shift</kbd> to draw without inference.

## Editing geometry

With no tool active, click geometry to add it to the selection; clicking empty space clears it.
Drag from empty space to box-select: left to right selects what is fully inside the box, right to
left also what it touches. <kbd>Ctrl</kbd>+<kbd>A</kbd> selects everything, and
<kbd>Delete</kbd> removes the selection.

Drag a point or a line to move it, or a circle or arc to change its radius; constraints and
dimensions hold while you drag.

| Tool | What it does |
| --- | --- |
| **Sketch fillet** (<kbd>Shift</kbd>+<kbd>F</kbd>) | Click a corner where two lines meet (or the two lines) and type the radius, or drag from the corner to size it. Further corners in the same use get fillets equal to the first. |
| **Sketch chamfer** | The same picks; two equal distances by default. |
| **Trim** (<kbd>M</kbd>) | Click a curve to remove the piece between its nearest crossings, or the whole curve if nothing crosses it. Drag a trail across curves to trim every piece it crosses. |
| **Extend** (<kbd>X</kbd>) | Click a line or arc near the end to grow, then click where it should end; it stops at the first curve in the way. |
| **Split** | Click a curve to cut it in two there. A circle takes two clicks. |
| **Offset** (<kbd>O</kbd>) | Click curves (or drag along one to take its chain), then click empty space or press <kbd>Enter</kbd>, choose the side with the pointer and type the distance. In a sketch on a face, clicking inside the face offsets its outline. |
| **Slot** | Click a line or arc (usually construction) to make it the centre line of a slot, then type the width. Further picks make slots of the same width. |
| **Mirror** | Pick the mirror line first, then each curve to mirror (or drag a box around several). Each copy is held to its original by a Symmetric constraint. <kbd>Enter</kbd> or <kbd>Esc</kbd> ends the tool. |

<kbd>Esc</kbd> leaves the active tool; pressed again, it clears the selection. It never leaves
the sketch itself.

## Construction geometry

Construction geometry is drawn dashed. It helps lay out a sketch (a centre line, a reference
circle) but makes no regions, so features don't use it.

**Construction** (<kbd>Q</kbd>) works two ways: with curves selected, it turns them into
construction geometry, or back if they all are already; with nothing selected, it switches
construction mode on or off for the geometry you draw next.

## Constraints

A constraint fixes a relation between pieces of geometry. Choose one from the **Constraints**
menu (the toolbar button with ▾) or with its key. With geometry selected, the constraint applies
to it at once. With nothing selected, the tool stays active: pick the geometry, and each set that
fits gets the constraint, until <kbd>Esc</kbd> or another tool.

| Constraint | Key | Takes |
| --- | --- | --- |
| **Coincident** | <kbd>I</kbd> | Two points (they become one), a point and a curve (the point lies on it), or two lines (they become collinear). |
| **Concentric** | <kbd>Shift</kbd>+<kbd>O</kbd> | Two circles or arcs, or a point and a circle (the point goes to the centre). |
| **Parallel** | <kbd>B</kbd> | Two or more lines. |
| **Tangent** | <kbd>T</kbd> | A line and a circle or arc, two circles or arcs, a line and an ellipse, or a Bézier curve and the curve it meets. |
| **Horizontal** | <kbd>H</kbd> | A line, or two points. |
| **Vertical** | <kbd>V</kbd> | A line, or two points. |
| **Perpendicular** | <kbd>Shift</kbd>+<kbd>L</kbd> | Two lines. |
| **Equal** | <kbd>E</kbd> | Two or more lines (equal lengths) or circles and arcs (equal radii). |
| **Midpoint** | <kbd>Shift</kbd>+<kbd>M</kbd> | A point and a line. |
| **Normal** | <kbd>Shift</kbd>+<kbd>K</kbd> | A line and a circle, arc or ellipse. Or a line, then a default plane or Plane feature: the line is held normal to that plane. |
| **Pierce** | <kbd>Shift</kbd>+<kbd>G</kbd> | A point and a part edge (or another sketch's curve) that crosses the sketch plane: the point stays where it crosses. |
| **Symmetric** | <kbd>Shift</kbd>+<kbd>Q</kbd> | The axis line first, then two points or two curves of the same kind. |
| **Fix** | <kbd>Shift</kbd>+<kbd>J</kbd> | Anything: it stays where it is. |
| **Curvature** | | Two curves meeting end to end, at least one a Bézier curve: they join smoothly, with the same curvature. |

Constraints show as small glyphs beside the geometry. A point that is coincident with something
is drawn as a square instead of a round dot. Click a glyph to select it, then press
<kbd>Delete</kbd>, or right-click it for its menu. Hovering a glyph highlights the geometry it
holds.

Constraints can also refer to geometry outside the sketch: with a constraint tool, you can pick
a part's corner as if it were a sketch point.

## Dimensions

**Dimension** (<kbd>D</kbd>) picks one or two pieces of geometry, and the dimension follows the
pointer; click to place it. What it measures depends on the picks:

- a line: its length;
- two points, or a slanted line: the horizontal distance if you move the label up or down, the
  vertical distance if you move it left or right, the straight distance in between;
- a point and a line: the perpendicular distance;
- two lines: the angle between them, the quadrant you place the label in choosing which angle;
- a circle: its diameter; an arc: its radius;
- a circle or arc with another piece of geometry: the distance to its near or far side, by which
  side of the circle you clicked;
- a point and a construction line, placed across the line: a diameter (twice the distance), as a
  revolve about that line will have.

Once placed, the value box opens: type the value and press <kbd>Enter</kbd>, and the geometry
moves to match. <kbd>Esc</kbd> keeps the measured value. Double-click a dimension's value to edit
it later, drag the value to move its label, or select it and press <kbd>Delete</kbd>.

Values are in mm unless you say otherwise. They can carry units (`25`, `2.5 cm`, `1 in`,
`30 deg`) and arithmetic (`2*10+5`, `(50)/2`), the functions `sqrt`, `sin`, `cos`, `tan`, `min`,
`max` and `abs`, and `pi`. Type `#` and a variable's name to use a variable, as in
`#width / 2`; the variable must be defined above the sketch in the feature list, or come from
one of the document's Variable Studios. A value that can't be used turns red with a message, and isn't applied.

The first length you type into a sketch scales the whole sketch to match, so its shape is kept.

A dimension is **driving** (it sets the geometry) or **driven** (it only measures). Right-click a
dimension and choose **Change to driven dimension** or **Change to driving dimension**.

## Fully and under-defined geometry

Geometry is coloured by how well its constraints and dimensions pin it down:

- **blue:** under-defined: it can still move;
- **black:** fully defined;
- **red:** part of a conflict.

When constraints conflict, the sketch shows "Some constraints are not applicable to the current
geometry and have not been solved." and its row in the feature list turns red. Undo the last
change, or delete one of the red constraints or dimensions.

Closed regions are shaded grey: those are the profiles other features can use.

## Using edges

**Use** (<kbd>U</kbd>) brings outside geometry into the sketch. Hover a part edge, another
sketch's curve, or a face to preview its projection onto the sketch plane, and click to take it.
A flat face gives all its edges; a curved face gives its outline as seen along the sketch
normal. Used curves are fixed in place and drawn black, with a link glyph you can delete to
remove the link.

Used curves follow their source: when the part or sketch they came from changes, they update.

## Checking a sketch

The diagnostics button at the bottom of the sketch dialog opens two panels:

- **Profile inspector** lists the **Loose ends**: ends of regular geometry that join nothing,
  the usual reason a region doesn't close. Each is marked with a red dot; click a row, or
  **Previous** and **Next**, to zoom to it.
- **Constraint manager** lists the sketch's constraints and dimensions. Under **Filters** you
  can narrow the list by type, by **Mode** (**Internal**, **External**, **In context**) and by
  **Status** (**Driven**, **Solved**, **Errors**), and **Sort by** **Constraint** or **Entity**.
  With **Automatically select constraints** off, it lists only what the selected geometry uses.
  Hover a row to highlight its geometry; each row's trash deletes it, and **Delete all** deletes
  every row the filters show.

## Finishing a sketch

Click ✓ or press <kbd>Enter</kbd> to accept: everything you did in the sketch becomes one undo
step. ✕ cancels: a new sketch is removed, and an edited one goes back to how it was. A toast
offers **Restore** in case you didn't mean it. <kbd>Esc</kbd> never leaves a sketch, so an extra
press can't throw your work away.

To edit a sketch again, double-click it in the feature list, or right-click it and choose
**Edit**.

## Shortcuts

The sketch tools and constraints have single-key shortcuts, listed in the tables above and on
the [Keyboard shortcuts](../reference/keyboard-shortcuts.md#sketch) page.
