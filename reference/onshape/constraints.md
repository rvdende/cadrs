# Sketch constraints (web-sourced)

Sources: help pages "Working with Constraints" and the per-constraint pages (Coincident,
Concentric, Parallel, Tangent, Horizontal, Vertical, Perpendicular, Equal, Midpoint, Normal,
Symmetric, Fix), all marked last updated 2026-09-22. Images are in `constraints/`. The live
captures `screens/12a-rect-placed-constraint-glyphs.png` and `15*` also apply.

## Where the tools live
- **Constraints ▾** is the second-to-last group in the sketch toolbar. It comes after Dimension
  and before "Search tools…" (`constraints/constraints-01.png`).
- The button shows the last-used constraint icon (coincident by default) with a ▾.
- The dropdown is a white card with a shadow and rounded corners, about 225 px wide and about 35 px
  per row. Each row has an icon (about 18 px, dark grey), a label, and **right-aligned keycaps**:

| # | Constraint | Key | Icon (description) | Selection it takes |
|---|---|---|---|---|
| 1 | Coincident | I | two short strokes meeting at a point, a slanted "⋌" | 2+ points/curves; point on curve; 2 lines become collinear; a sketch entity and a plane |
| 2 | Concentric | Shift+O | ◎ | circle/arc + circle/arc, or a point + circle/arc |
| 3 | Parallel | B | two slanted parallel strokes "⫽" | 2+ lines |
| 4 | Tangent | T | a line touching a small circle | 2+ curves (line–arc, arc–arc), or a curve and a plane |
| 5 | Horizontal | H | a horizontal bar "—" | 1+ lines, or 2+ points |
| 6 | Vertical | V | a vertical bar "│" | 1+ lines, or 2+ points |
| 7 | Perpendicular | Shift+L | "⊥" with a small square corner | 2 lines |
| 8 | Equal | E | "=" | 2+ lines (length) or 2+ arcs/circles (radius) |
| 9 | Midpoint | Shift+M | "-•-" | a point + a line/arc; or start, end, then a middle point |
| 10 | Normal | Shift+K | a "Y"-like fork | a line + a curve, or a curve + a plane |
| 11 | Pierce | Shift+G | a curve through a point | out of scope (3D) |
| 12 | Symmetric | Shift+Q | "Σ" plus mirrored triangles | **the axis line first**, then 2 entities of the same type |
| 13 | Fix | Shift+J | a hatched ground line "▭////" | 1+ entities |
| 14 | Curvature | Shift+U | nested arcs | splines, out of scope |

For M4 we need rows 1–10, 12 and 13.

## Applying a constraint: both workflows are supported
1. **Select, then tool.**
   - Select entities first. Selection is additive: click toggles; see `box_select.md`.
   - Then click the constraint in the menu or press its key. The constraint is applied to the
     selection right away, and the **selection clears**. The clearing is inferred from the
     "Constraints viewing" images, where nothing stays selected.
   - If the selection does not fit the constraint, nothing is applied. Onshape greys out
     menu entries that do not apply to the current pre-selection; this is widely observed but
     uncertain, so treat it as optional.
2. **Tool, then select** (the "toggle" mode, stated on every constraint page):
   - With nothing selected, pressing the key or clicking the menu entry **toggles the tool on**.
   - Then pick entities. **"Each pair of entities selected are constrained to each other."**
     After the second pick the constraint is created, and the tool stays active for the next pair.
   - Horizontal, Vertical and Fix need only one entity, so each click applies at once.
   - To finish: click the tool again, pick another tool (which turns it off automatically), or
     press Esc.
   - Midpoint and Symmetric need 3 picks in some cases (start, end, middle; or axis + 2 entities).
     The rule is order-sensitive: Symmetric **pre-selects the axis line**.

## Constraint glyphs (icons drawn in the viewport)
- **Visibility:**
  - With the sketch dialog's **Show constraints** checked, all glyphs are drawn. The live capture
    `NOTES.md` shows it checked by default.
  - With it unchecked, only the glyphs of the **hovered** entity are shown.
  - Holding **Shift** while moving the mouse keeps the currently shown glyphs visible, so you can
    reach them.
- **Placement:**
  - Glyphs sit near the geometry: at the midpoint side of lines for H/V/parallel/equal, at the
    corner for perpendicular and coincident.
  - Several constraints on one entity group into a **row** of glyphs, for example "— ⋌" or
    "⊘ ⋌ ⋌ ⫽ ⫽".
  - The user can **drag** a glyph or group to move it.
- **Appearance** (`midpoint-3-03.png`, `constraintsviewingallthroughdialog1.png`):
  - Each glyph is a dark-grey (#5f6060 to #333) line icon about 14–16 px, in a pale box about
    20 px tall.
  - The box is **#eef0f1**, a translucent white, when it sits over a filled region (#e3e6e9). Over
    white space it is effectively white.
  - No border, square corners, about 1–2 px padding. Adjacent glyphs of the same group touch.
  - **Well-defined** constraints are black on grey. **Conflicting** constraints are **white on a
    red background**; see `definedsketchesandconstraints.png` and live `15*`.
  - **External reference** constraints (Use/project) have a **light-blue background, #92d4ee**,
    which turns darker blue on hover (`constraints-referenced.png`).
- **Hover:**
  - Hovering a glyph highlights the entities it applies to (orange/yellow on hover, as with any
    hover).
  - Hovering an entity shows its glyphs, and the hovered entity is drawn orange
    (`constrainticonsonhover.png`: the bottom line is orange and its glyph "⋌" is shown).
  - **Related entities are highlighted yellow** when a constraint is selected.
- **Deleting:** click a single glyph to select it, then press **Delete/Backspace**, or right-click
  it and choose Delete. Selecting an entity and deleting it removes the entity together with its
  constraints.
- **Midpoint glyph:** a short dash with a dot, "-•-", at the line's midpoint.
- **Coincident at a shared endpoint:** a "⋌" glyph near the point. The coincident of two
  endpoints created by drawing is usually implied, with no glyph; this is consistent with the
  rectangle corners in live `12a`.

## Auto-created constraints (from drawing or inference)
- While drawing, the inference glyph appears **next to the cursor** (for example "│" for vertical
  and "⫽" for parallel). Clicking at that moment creates the constraint. See `inference.md`.
- The rectangle tool creates 2 horizontal, 2 vertical, and 4 implicit corner coincidences, plus
  perpendicular or parallel. Live `12a` shows perpendicular, parallel and horizontal glyphs.
- Line polylines are coincident at their joints. A tangent arc is coincident and tangent at its
  start.

## Status colors (entities)
- **Blue = under-constrained.** Sampled lines are about **#0000ff**; anti-aliased thin lines read
  #4a4cf5. Points are blue dots.
- **Black = fully constrained** (#000000). Points are black dots.
- **Red = over-constrained or conflicting** (core pixels are **#be0000**; anti-aliased pixels read about #ca3e3e). The red dimension text
  uses the same color. Conflicting dimensions are drawn red too.
- **Driven (reference) dimensions** are **light blue-grey** (darkest pixels are about #95a1ab). They are created
  automatically when a new dimension would over-define the sketch.
- When selecting, non-fully-constrained points (blue or red) are picked **in preference to**
  overlapping black points.
- The feature list entry and the dialog title turn red on errors (live `15`).

## Constraint manager (optional, later)
- Opened from the sketch dialog's "Show sketch diagnostic tools" icon (bottom right of the dialog)
  → Constraint manager (`constraint-manager-01.png`).
- It offers filters by Type (an icon grid), Mode (internal, external, in context) and Status
  (driven, solved, errors), sorting by constraint or by entity, an "Entities and constraints"
  list with a delete 🗑 per row, and "Delete all".
- This is not needed for M4.
