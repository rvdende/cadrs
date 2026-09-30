# Dimension tool, shortcut D (web-sourced)

Sources: help page "Dimension" (`Sketch/dimension.htm`, last updated 2026-09-22) and the forum
thread "Force radius or diameter dimension in a sketch" (2016, still an open question in 2024).
Images are in `dimension/`. The live `screens/16*` (see `NOTES.md`) take priority.

## General flow
1. Press D or click the tool. Hovered entities highlight orange.
2. Click one entity, or two.
3. Move the mouse. The dimension (extension lines, arrowed dimension line and value) follows the
   cursor, and **its type depends on the selection and where the cursor is**.
4. Click to place. The value field opens at once, pre-filled with the current measured value and
   **fully selected** (`dimension-active-field-01-01.png`).
   - The field is a white popup with a subtle border and shadow, about 130×28 px, and a drag
     handle "▫▫▫" on top.
   - The value is right-aligned with its unit, for example "1.87 in" (mm in our case).
   - It has a green ✓ button (about 20 px, green #039203 with a white check) and a red ✕ button.
5. Type a value or an expression and press Enter. Esc cancels the edit, and the dimension keeps
   its measured value (uncertain).

Selected entities while dimensioning are shown with **yellow/orange dots**
(`dimension-highlights-01.png`: two corners marked with orange discs about 8 px wide).

After placing, the dimension is black and driving. Its geometry turns black once it is fully
constrained.

## Types by selection and cursor placement
| Selection | Result | Placement rule |
|---|---|---|
| 1 line | Length (aligned) | Dragging perpendicular to the line gives an aligned length. For a slanted line, dragging straight up/down or left/right gives a **horizontal or vertical** projected length instead (as with two points) |
| 2 points (endpoints) | **Linear H or V distance**, or **direct (aligned) distance** | Drag **straight up/down → horizontal distance**; drag **left/right → vertical distance**; drag **at an angle** (off the axes, into the diagonal region) → aligned shortest distance (`dimensionlineardistance-02.png`, `dimensionshortestdistance-02.png`) |
| 2 parallel lines | Perpendicular distance | Implies parallel without adding a visible parallel glyph |
| Point + line | Perpendicular distance | |
| Full circle | **Diameter "Ø1.5"** | An arrowed leader from outside the circle to its edge (`dimensioncircumference.png`). Drag in or out to position it |
| Arc | **Radius "R0.9"** | A line from the center to the arc with an arrow (`dimensionradius.png`) |
| 2 non-parallel lines | **Angle** | The value is shown with a ° sign and an arc with arrows. **The quadrant the cursor is in picks which angle**: acute, supplementary, or reflex such as 47.7°, 132.3° or 312.3° (`dimension-anglequadrants.png`) |
| Arc endpoint, arc endpoint, arc | Arc length | |
| Point/line + construction line | Distance. Moving the cursor **across** the construction line toggles to a **centerline** (doubled, diameter-style) dimension | |
| Sketch point + plane | Distance to the plane | |

- There is no radius↔diameter toggle on a full circle: circle → Ø and arc → R. To get a radius on
  a circle, the workaround is to dimension a construction circle, or type "2*r".
- **Negative values** are allowed for distance, angle and line length, and flip the geometry.
  They are not allowed for radius, diameter or arc length.
- **Solve rule for a new value:** both dimensioned entities move by **half** the change each,
  unless they are constrained. Radius and diameter changes resize about the center.
- **Over-defining:** a dimension that would over-constrain the sketch is created as **driven**
  instead of failing.
  - Driven dimensions are light blue-grey (#95a1ab to #d4d8dc) and not editable.
  - Right-click → "Change to driving dimension" switches it
    (`dimension-driven-01.png`, which also shows the dimension context menu: Change to driving
    dimension, Escape dimension, Confirm Sketch 1, Copy sketch, Show all, Select ▸, Select other…,
    Add comment, Zoom to fit, View normal to sketch plane).
- **Editing:** double-click a dimension value to reopen the field.
- **Deleting:** select the dimension, then press Delete or use the context menu Delete.
- **Dragging** a placed dimension's text repositions it (standard; implied by the images).

## Appearance (measured from help images)
- Dimension lines and text are black. The text is about 13–14 px Inter-like sans, centred on the
  dimension line with a gap in the line.
- Arrows are filled, narrow, about 8 px long.
- Extension lines are thin black and start a small gap (about 2–3 px) from the geometry.
- A diameter label is prefixed "Ø", a radius label "R", and an angle is suffixed "°".
- Values show trailing precision as entered. Live values while drawing use 5 decimals (live notes).
