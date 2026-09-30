# Sketch inference, snapping and construction (web-sourced)

Sources:
- Help pages "Automatic Inferencing", "Working with Constraints", "Sketch Basics", "Line",
  "Construction", "Keyboard Shortcuts" (all last updated 2026-09-22).
- Tech Tip "2 Ways the Shift Key Can Increase Productivity" (2022-05-24).

Images are in `inference/`, plus `circle_arc/SemicircleSnap.png` and
`constraints/midpoint-3-03.png`. The live captures `screens/10`, `18*` and `19*` (see
`NOTES.md`) take priority.

## Answer to the plan's question: does Shift disable inference?
**Yes.** The shortcut list has "**Shift: Suppress automatic inferences**" in the Sketch section,
and the Automatic Inferencing page says: "To suppress automatic inferences, press the Shift key
when dragging with the mouse."

The tech tip adds: "hold the Shift key to temporarily suppress inferencing". Releasing Shift turns
inference back on. This applies both while placing points with a sketch tool and while dragging
existing geometry.

With Shift held, the cursor position is used raw:
- No snap to points or curves.
- No horizontal or vertical snap.
- No auto-constraints are created.

Secondary Shift meaning: with Show constraints off, Shift also "keeps constraint glyphs visible"
while moving the mouse. There is no conflict: that only matters when no tool is placing geometry.

## Inference kinds
Onshape wakes up these inferences while a sketch tool is active:

| Inference | Trigger | Visual | Constraint created on click |
|---|---|---|---|
| Coincident (point) | Cursor within a few px of an existing point: an endpoint, center, the origin, or a vertex of another sketch or part edge coplanar with the sketch | **Yellow-orange square** around the point (about 10–12 px, outline about #ffcc34 with a pale fill) and a "⋌" glyph near the cursor | Coincident |
| Point on curve | Cursor on a line, arc or circle body | Whole **curve highlighted orange** (live `18a`) | Coincident (point on curve) |
| Midpoint | Cursor near a line's midpoint (arc midpoints too) | Square at the midpoint plus a "-•-" glyph | Midpoint |
| Horizontal / vertical of the segment being drawn | Line direction within a small angle of the H/V axis (live: 6 px off did not snap) | Line snaps; "—" or "│" glyph at the cursor | Horizontal / Vertical |
| Horizontal / vertical **alignment** with a woken point | After **hovering an existing point or the origin** ("wake up"), move so the cursor aligns with it | A **dotted yellow line** (about #ffd969) from the woken point to the cursor, plus an H or V glyph (`inference/automaticinferencingexample2.png`) | Horizontal / Vertical between the two points |
| Parallel | The line being drawn is nearly parallel to a **woken** line | The new line turns **dotted**, the reference line is highlighted **orange** (#c0975c as seen through translucency), and a "⫽" glyph appears (`inference/parallelinferencing.png`) | Parallel |
| Perpendicular | Nearly perpendicular to a woken line, or to the previous segment in a polyline | "⊥" glyph | Perpendicular |
| Tangent | Starting a line or arc from an arc's end, or an arc ending tangent to a curve | Tangent glyph | Tangent |
| Semicircle / arc center on chord | 3-point arc whose center hits the chord | All 3 points squared plus a "-•-" glyph | Midpoint |
| Coplanar external | Hovering edges or vertices of other sketches or parts on the same plane | Those entities "wake up" (highlight) and become snap targets | Coincident, or a Use-type reference |

"Wake up" rule: many inferences, including alignment and parallel to non-adjacent entities, need
the cursor to **pass over** the reference entity first. The help lists horizontal, vertical,
midpoint, parallel and coincident as the common wake-up inferences. The live notes confirm that
without a woken point there are no alignment lines. A reasonable implementation:
- Keep a small most-recently-hovered list of about 3 points and lines.
- Offer alignment and parallel inferences only against that list, the origin, and the start of
  the current segment.

## Snapping priorities (inferred; the help text gives no order)
1. An existing point, taking the closest within the pick radius.
2. A midpoint.
3. An intersection or point-on-curve.
4. Direction snaps (H/V for the segment itself), then alignment to woken points, then
   parallel/perpendicular to woken lines.
5. Otherwise the raw cursor position.

When a point snap and an H/V alignment combine, both constraints are created: for example, the
endpoint lands on the dotted vertical line through a woken point *and* on a curve.

## Cursor-side indicators
"When sketching, constraint indicators appear next to the mouse cursor as the curves snap to
inferences." (Working with Constraints)
- A glyph of about 16 px sits about 8–12 px right of and slightly above or below the cursor
  crosshair, and shows **one glyph per active inference**.
- The cursor is a thin crosshair "+" in sketch tools, with a small tool glyph in the Line and
  Tangent-arc switch case.

## Live values while drawing
Rubber-band geometry is light blue (#449ccd), with values in light-blue text:
- line length at the line's midpoint;
- rectangle width and height;
- circle Ø;
- arc R.

After creation an editable box appears (`dimension/sketchdims-linebox.png`: a grey-bordered white
box of about 36×22 px with the value). Type to set it, or keep drawing to leave the geometry
undimensioned. **Alt+arrow** switches between multiple boxes (for example the rectangle's width
and height).

## Construction mode (Q)
- **Q** toggles the Construction tool, and the toolbar button shows pressed.
- **With a selection:** toggles the selected entities between regular and construction.
- **With no selection:** arms construction mode, so the next drawn entities are construction.
  Press Q again to disarm.
- **Look** (`sketch_dialog/line-styles-construction-act-inact.png`):
  - Construction lines are **dash-dot** ("— · —"): long dash about 12 px, gap about 4 px, dot
    about 2 px, gap about 4 px.
  - They use the same status color as regular geometry (blue when free, black when constrained).
  - Selected ones are orange-yellow (#f6bc1a). In an inactive (accepted) sketch they are grey.
- Construction geometry does not bound regions and is not used by features.
- Dimensioning to a construction line allows **centerline (diameter-about-axis) dimensions**; see
  `dimension.md`.

## Line tool details from the help (complements `NOTES.md`)
- Click–click chains segments, and each new segment starts at the previous end.
- Click-drag-release creates a single segment.
- The quick length box appears after each segment.
- For Shift+A and tangent arcs, see `circle_arc.md`.
