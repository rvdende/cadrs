# Circle and arc tools (web-sourced)

Sources: help pages Center Point Circle, 3 Point Circle, 3 Point Arc, Tangent Arc, Center Point
Arc, and Line ("Switching between Line and Tangent arc"), all marked last updated 2026-09-22.
Images: `circle_arc/SemicircleSnap.png`, `circle_arc/line-to-arc-01.png`,
`inference/sketch-coplanar-inference.png` (a circle rubber band with a live Ø label).

## Toolbar placement
In the sketch toolbar (`sketch_dialog/toolbarSketchbase.png`), after Line and Rectangle ▾:
- **Circle ▾** contains Center point circle (default) and 3 point circle.
- **Arc ▾** contains 3 point arc (default), Tangent arc and Center point arc.

A dropdown group shows the icon of the **last tool used** from that group. This follows the help
text on the Sketch Tools page.

## Shared behaviors (all five tools)
- **Two input styles:** click–move–click, or press–drag–release. The drag gesture replaces the
  first two clicks: press sets point 1 and release sets point 2.
- **Quick dimension after creation:**
  - Right after the entity is created, typing a number and pressing Enter dimensions it with no
    Dimension tool needed. The value box works like the rectangle one in `NOTES.md`.
  - **Circles get a diameter** dimension.
  - **Arcs get a radius** dimension.
  - "=#var" enters a variable.
- **Live feedback:**
  - The rubber-band entity is light blue (sampled **#449ccd**), thinner and lighter than committed
    geometry (committed under-constrained geometry is pure blue, about #0000ff).
  - The live size label is light-blue text:
    - Circles show "Ø1.08" inside the circle, drawn along a radius (see
      `inference/sketch-coplanar-inference.png`).
    - Arcs show "R1.172" near the chord.
  - Snap targets under the cursor get a **yellow-orange square** (about #ffcc34, a 10–12 px square
    outline with a translucent fill) on the point.
  - An inference glyph (for example coincident `⋌`) appears in a small grey box beside the cursor.
- The tool stays active after creating an entity, ready for the next one. Esc leaves it (as with
  Line).
- Center points of circles and arcs are drawn as a **dot** in the entity's status color (blue when
  free). Endpoints are dots too.

## Center-point circle, shortcut **C**
1. Click to set the center. It can snap to the origin or an existing point, which adds a
   coincident constraint.
2. Move: a rubber-band circle follows, with its radius set by the cursor distance and a live "Ø…"
   label.
3. Click to set a point on the circumference. The circle is created, and the quick-dimension box
   opens for the **diameter**.
- If the second click lands on an entity, it adds coincident, or tangent when hovering a line or
  circle tangentially (uncertain: the help text does not say which inferences apply on the
  circumference).

## 3-point circle (no default shortcut)
1. Click point 1 on the circumference.
2. Click point 2 on the circumference. While moving between clicks 1 and 2, the preview is a
   **chord line**; this is the most likely behavior, not documented.
3. Move: the circle through all three points follows the cursor.
4. Click point 3. The circle is created and the quick-dimension box opens for the diameter.
- Stored as an ordinary circle (center + radius). The three picked points are not kept as
  constraints unless they snapped to existing geometry.

## 3-point arc, shortcut **A**
1. Click the **start point**.
2. Click the **end point**. Between the two clicks a straight preview line shows the chord.
3. Move: an arc through start, end and the cursor follows (the cursor sets the radius and the
   bulge side).
4. Click to finish, then enter the radius in the quick-dimension box.
- **Semicircle snap** (`SemicircleSnap.png`):
  - When the arc's center falls on the start–end chord, the center snaps onto the chord line.
  - The two endpoints and the center point all show yellow snap squares, and a midpoint glyph
    `-•-` appears near the cursor.
  - Clicking gives an exact semicircle: a midpoint constraint puts the center at the chord's
    midpoint.

## Tangent arc (no default key; reached through **Shift+A** from Line)
- **Standalone tool:**
  1. Click the **end of an existing line or arc** to start. The start is coincident with that
     endpoint, and the arc is tangent to that entity.
  2. Move: an arc tangent at the start follows the cursor.
  3. Click to set the end.
  Drag-release also works.
- **From the Line tool (mid-polyline):**
  1. After finishing a segment, either press **Shift+A** or **move the cursor back near the
     segment's end point**.
  2. The cursor icon switches from the line glyph to the tangent-arc glyph (`line-to-arc-01.png`,
     boxed in red).
  3. Click to set the arc end. The tool **switches back to Line automatically**, continuing from
     the arc end.
  4. Press Shift+A again to chain another tangent arc.
- The arc's direction follows the way the cursor leaves the endpoint: it starts tangent to the
  previous segment, and swinging to the other side reverses it.
- The live label during a tangent arc is the length or radius in light-blue text (for example
  "0.08" next to the following line segment in the screenshot).
- **Constraints created:** coincident at the start and tangent to the previous entity.

## Center-point arc (no default key)
1. Click the **center** (can snap to an existing point).
2. Click the **start point**. This fixes the radius, and a dashed or rubber-band radius line shows
   from the center.
3. Move: the arc sweeps from start toward the cursor at a constant radius. The sweep direction
   follows the first move, clockwise or counter-clockwise (standard CAD behavior; uncertain for
   Onshape).
4. Click the **end point**. The end is projected onto the circle, so only its angle matters.
5. The quick-dimension box then opens for the radius.

## Arcs and circles in the solver and regions
- A full circle alone is a closed region, filled grey like the rectangle in `NOTES.md` once the
  sketch is accepted.
- Arcs combine with lines into regions.
- Dimensioning a full circle gives Ø (diameter). Dimensioning an arc gives R (radius). A circle
  cannot be switched to radius in the sketch tool; the forum workaround is a construction circle.
  See `dimension.md`.
