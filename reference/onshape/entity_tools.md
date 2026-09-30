# Entity tools: midpoint line, aligned rectangle, 3-point circle, polygon, slot, ellipse, fillet, chamfer, point (web-sourced)

Sources are listed at the end. The help pages are marked "Last Updated: September 24, 2026" and
were fetched on 2026-09-25. Images are in `entity_tools/`. The help images come from
`https://cad.onshape.com/help/Content/Resources/Images/sketch-tools/…`, and one BMP (served under a
.png name) was converted to PNG. The polygon images come from a Purdue TRAILS PDF (`polygon-*.png`)
and an Onshape Tech Tip (`polygon-techtip-hero.png`). Colors follow `circle_arc.md` and
`sketch_dialog.md`:
- The rubber band is light blue, about #449ccd.
- Committed under-constrained geometry is blue, #0000ff.
- Dimensions are dark grey (#333) with filled black arrowheads.
- A selected entity is orange (about #f8b745 to #de9425), or tan (#d5ae61) for a selected line.

## Toolbar placement (`sketch-toolbar-01.png`, live `screens/08a`/`08b`)
The order is Line ▾, Rectangle ▾, Circle ▾, Arc ▾, Polygon ▾, Spline ▾, Point, Text, Use ▾, an
unlabelled |⇄| tool, Fillet ▾, Trim ▾, Offset ▾, Mirror, Pattern ▾, DXF ▾, Dimension, Constraints ▾. As before, a dropdown
button shows the last-used member.
- **Line ▾**: Line (L) and **Midpoint line**. The Midpoint line help says "Click the dropdown next
  to the Line tool".
- **Rectangle ▾**: Corner (G), Center point (R) and **Aligned rectangle**.
- **Circle ▾**: Center point (C), **3 point circle** and, most likely, **Ellipse**. The help pages
  don't name the dropdown for Ellipse; this repo's `sketch_dialog.md` places it in Circle ▾.
- **Polygon ▾**: **Inscribed polygon** and **Circumscribed polygon**. The Purdue PDF confirms this:
  "Select the Polygon icon drop down arrow".
- **Point** is a standalone button: a small hollow circle glyph.
- **Fillet ▾**: Sketch fillet (Shift+F) and most likely **Sketch chamfer**. The toolbar image shows a
  ▾ on Fillet, but no help text names its members.
- **Slot**: no dedicated button is visible in either toolbar capture. It is probably inside a
  dropdown, most likely Offset ▾ (unverified). For our app it could go in Offset ▾ or stand alone.
- **Shortcuts** (`shortcut_keys.htm`): **Point = Shift+S** and **Sketch fillet = Shift+F**.
  Midpoint line, aligned rectangle, 3-point circle, polygons, slot, ellipse and chamfer have no
  default key.

## Midpoint line (`midpoint-line-01.png`)
- Click to place the **midpoint**, then click to place **one end point**. The line grows
  symmetrically, so the other end mirrors the cursor.
- The preview is a light-blue line with a **blue dot at the midpoint (the first click)** and a dot
  at the far end.
- The live length label ("2.45") is light blue and sits **below the line near the midpoint**.
- A horizontal inference shows as an **olive/yellow dashed overlay** (#d2bf56) on the half from the
  midpoint to the cursor. It is the same dotted inference style as other tools.
- Constraints are not documented. The natural choice is a midpoint constraint between the center
  point and the line (plus horizontal or vertical if inferred). Onshape probably keeps the
  midpoint point.

## Aligned rectangle (`aligned-rectangle-01/02/05.png`)
1. Click to start the first side.
2. Click to set its end. While moving, the preview is a single light-blue line with a live length
   label ("13") beside it.
3. Move perpendicular to the first side and click at the distance you want. The rotated rectangle
   is created.
- While moving, the preview has **two live labels**: the first side's length ("13") beside it and
  the width ("14") along the second side.
- **Quick dimension:** type the length and press Enter, then type the width and press Enter. The
  value box sits next to the side being set.
- The result has **aligned dimensions parallel to the sides**, with extension lines and arrows
  (`aligned-rectangle-05.png`: 1.9 along the top, 1.6 along the left).
- Implied constraints are 4 coincident and **3 perpendicular plus parallel** (no H/V). This is
  inferred, not documented.
- **Alt held** while sketching makes it a square (equal sides).

## 3-point circle (no images)
- Click point 1, then point 2, then point 3 on the circumference. Alternatively, press at point 1,
  drag and release at point 2, then click point 3.
- Afterwards, the quick-dimension box sets the **diameter**. See also `circle_arc.md`.

## Polygon: inscribed and circumscribed (`polygon-inscribed-sides.png`, `polygon-inscribed-circumscribed.png`, `polygon-techtip-hero.png`)
- Onshape's naming is backwards from the textbook sense:
  - **Inscribed** = "polygon on the outside of the drawn circle". The circle touches the
    **flats**, so use it "if the distance across the flats is important".
  - **Circumscribed** = polygon inside the circle. The **vertices** lie on the circle, so use it
    for the distance across the vertices.
- **Steps:**
  1. Click the center.
  2. Drag or move to set the circle size. The circle is drawn with the **construction flag**.
  3. Click to lock the circle. **A value field appears for the number of sides.**
  4. Type the number of sides, or **drag away from the polygon for more sides and toward it for
     fewer**. You can also type the count before the second click.
  5. Click again or press Enter to finish. Sides run from **3 to 50**.
  6. Then the quick-dimension box sets the **circle diameter**.
- **Result:**
  - A **dash-dot construction circle** with a center point.
  - N equal sides, with a blue dot at each vertex.
  - For inscribed: a **hollow (white-filled) point** where the circle touches one side, at that
    side's midpoint. This is the tangent/midpoint reference.
- **Side-count label:** "**3x**", "**4x**", "6x"… in dark-grey text **outside the polygon**. Its
  **leader line with a filled arrowhead** points to one side:
  - For inscribed, it points at that hollow tangent point.
  - For circumscribed, it points at the middle of a side.
  - The label looks like a dimension, and training S7.4 says it can be edited by double-clicking.
- Constraints: all sides equal, and the vertices (circumscribed) or side midpoints/tangency
  (inscribed) tied to the construction circle. The exact set is not documented.

## Slot (`slot-*.png`, `preselect-dim-slot.png`)
- Slot is **not a draw-from-scratch tool**. It wraps a slot around **existing sketch curves**
  (lines, arcs, splines, chains, closed profiles).
  - You can select the curves first and then click Slot, or click Slot and then pick the curves.
    The selected line shows tan (#d5ae61).
  - The source curve **stays as-is** (solid blue in the images; construction is typical).
  - Its endpoints are the **centers of the end-cap arcs**.
- **Width** is shown as a **diameter dimension "Ø0.5"**:
  - The label sits **above the slot**, with a **vertical leader and arrow pointing down onto an
    end-cap arc**.
  - For a closed profile, it shows as a linear "0.3" offset dimension to the side.
  - Double-click the label to edit it, before or after placement.
- **Multiple curves:**
  - When several curves are selected before clicking Slot, all their slots share **one
    dimension** (equal).
  - Clicking more curves while the tool is active gives them the same size, and editing the
    dimension changes all of them.
  - A pre-selected chain produces **one continuous slot outline** in that shape.
- Each slot adds 2 offset side lines with endpoint dots and 2 end arcs, so a 4-line pre-selection
  merges into a scalloped outline (`preselect-dim-slot.png`).

## Ellipse (`ellipse-01/02/04.png`)
1. Click the **center**.
2. Move and click to set the **primary axis radius**, which also sets the direction. The preview
   is **already a full ellipse**, not a line.
3. Move and click to set the **secondary axis radius**, perpendicular to the first.
- Either axis becomes major or minor depending on size.
- **Live labels** are light blue. After click 2, "0.4" shows between the center and the cursor.
  During step 3, "0.2" (secondary) and "0.4" both show inside the ellipse.
- **Quick dimension:** the primary axis diameter, Enter, then the secondary axis diameter, Enter.
- **Result** (`ellipse-04.png`):
  - The ellipse has a **center dot** and **no visible construction axis lines or axis-end
    points**.
  - The two dimensions are drawn as **full-axis lines through the center**, with arrowheads at the
    ellipse (labels "0.5" on the major and "0.25" on the minor).
  - The labels sit inside the ellipse, near the center.
  - The help says "diameter", but the values shown (0.5 and 0.25 versus live 0.4 and 0.2) are
    ambiguous.

## Sketch fillet, shortcut **Shift+F** (`sketchfillet*.png`, `sketch-fillet-*.png`)
- **Select** a vertex, or two curves: lines, arcs or splines.
- **Radius input:**
  - The radius box appears near the corner, showing units ("0.25 in").
  - When two lines are picked, clicking the first does nothing visible. **Press-and-drag the
    second** to estimate the size live.
- **Result:**
  - A tangent arc with **filled dots** at both tangent points and at the **arc center**.
  - The original corner is kept as a **"virtual sharp": a small hollow circle**. It keeps the
    **coincident constraints** to both lines and the radius dimension.
- **Dimension:** a radius label "**R0.25**" / "R1.5", with a leader and arrowhead from the label to
  the arc.
- **Repeats:**
  - Further clicks while the tool is active reuse the **first radius**. Only **one R label** is
    shown for the set, which is effectively equal (`sketchfilletvertexexample.png`: 4 corners,
    one "R0.25"; `sketchfilletsamesize.png`: one "R0.075").
  - Editing the first value changes all of them.
- If the radius is too large, the fillet arc is drawn **red**.
- **Manipulator:** an **orange hollow arrow** at the corner, pointing outward diagonally. Dragging
  it resizes the fillet (light-blue preview arc).
- **Active value:** clicking the "R0.436" label turns it into an edit field with a **tan/beige
  background** (#cab395).
- Constraints: tangent on both sides plus the radius dimension, as noted above. Tangent glyphs are
  not shown in the images.

## Sketch chamfer (`chamfer-*.png`)
1. Select a vertex or two edges. The selected edges turn **orange**.
2. A preview chamfer line (light blue) appears, with **two distance dimensions** that default to
   **equal distances (45°)**.
3. Click the first value, type, press Enter. Click the second value, type, press Enter.
- **Dimensions:**
  - Each dimension is measured **along its edge from the original corner (virtual sharp)** to the
    chamfer end.
  - They are drawn as linear dims with extension lines ("4" / "4"; "0.25" horizontal and "0.25"
    vertical).
- **Result:** the original corner stays as a point, with **dash-dot construction extension lines**
  from the chamfer ends to the corner (`chamfer-creation-01.png`).
- **Repeats:** after selecting several vertices or edges, editing the first dimension updates all
  of them (4 corners with one pair of dims in `chamfer-multi-select-step3-01.png`).
- **Manipulator:** a **grey hollow arrow** on an edge; drag it to resize.
- If the chamfer is too large it is drawn **red**. If it cannot be created, a yellow warning bar
  appears.

## Point, shortcut **Shift+S**
- Click to place. It takes most constraints (coincident, midpoint, dimensions…).
- It is drawn as the standard sketch point, a **filled dot** of about 6 px in the status color.
  The toolbar glyph is a small hollow circle. The help has no screenshot.

## Sources (fetched 2026-09-25)
- https://cad.onshape.com/help/Content/Sketch/midpoint_line.htm
- https://cad.onshape.com/help/Content/Sketch/aligned_rectangle.htm
- https://cad.onshape.com/help/Content/Sketch/3_point_circle.htm
- https://cad.onshape.com/help/Content/Sketch/inscribed_polygon.htm
- https://cad.onshape.com/help/Content/Sketch/circumscribed_polygon.htm
- https://cad.onshape.com/help/Content/Sketch/slot.htm
- https://cad.onshape.com/help/Content/Sketch/ellipse.htm
- https://cad.onshape.com/help/Content/Sketch/sketch_fillet.htm
- https://cad.onshape.com/help/Content/Sketch/sketch_chamfer.htm
- https://cad.onshape.com/help/Content/Sketch/point.htm
- https://cad.onshape.com/help/Content/Sketch/sketch_tools.htm (toolbar image, dropdown rule)
- https://cad.onshape.com/help/Content/shortcut_keys.htm
- Tech Tip "Learn About Sketching Polygons in Onshape" (2021-11-23, S. Meyers):
  https://www.onshape.com/en/resource-center/tech-tips/tech-tip-learn-about-sketching-polygons-in-onshape
- Tech Tip "Using the Slot Tool": https://www.onshape.com/en/resource-center/tech-tips/tech-tip-how-to-sketch-slots
- Purdue TRAILS "Onshape Introduction Polygons" (2024-11, screenshots of Nx labels):
  https://www.purdue.edu/trails/wp-content/uploads/2024/11/4-Onshape-Introduction-Polygons.pdf
