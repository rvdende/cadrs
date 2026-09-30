# Sketch feature dialog, toolbar and status colors (web-sourced)

Sources: help pages "Sketch Basics", "Sketch Tools" and "Working with Constraints" (last updated
2026-09-22). Images are in `sketch_dialog/`. The live `screens/07`–`09`, `08c` and `20` take
priority; they show "Show constraints" checked by default in a fresh account. The help image
shows it unchecked, because the dialog **remembers the last-used checkbox states**.

## Sketch dialog (`sketchdialogblank-01.png`, `constraint-manager-00.png`)
- A floating card at the top left of the viewport, about 230–245 px wide. It is white, with a
  1 px light border and a soft shadow.
- **Header row:**
  - The title "Sketch 1" is bold, about 14 px, **dark red (#b0222a-ish) while invalid**, meaning
    no plane is picked yet.
  - On the right:
    - The **✓ accept** button, a square of about 28 px. It is filled **green #039203** with a
      white check when the dialog can be accepted, and **pale green (#cceacc) with a white check
      while disabled**.
    - The **✕ cancel** button, a red (#c03941) glyph with no fill.
- **Sketch plane field:**
  - Full width with a light border.
  - While waiting for input it is **highlighted pale blue #def1fb**, with placeholder text
    "Sketch plane".
  - Once set, it shows a small caption "Sketch plane" above the value "Front plane" and a ✕ to
    clear it.
  - A clock/"mate connector" icon button sits to the right of the field (implicit mate
    connectors; out of scope).
- **Checkboxes, one per row, about 30 px apart:**
  - ☐ Disable imprinting
  - ☑ Show constraints
  - ☐ Show expressions (always resets to unchecked on reopen)
  - ☑ Show errors
- **Footer:**
  - The "Show sketch diagnostic tools" icon button offers a menu with Profile inspector… and
    Constraint manager….
  - A grey "?" help icon.
- **Accept (✓ or Enter)** commits the sketch feature and restores the part-studio toolbar.
- **Cancel (✕ or Esc with no tool active)** discards every change made while the dialog was
  open. A **toast** "Sketch 1 has been cancelled. **Restore** ✕" appears at the top center
  (`restore.png`): a light-blue pill (#d6ecf8-ish) with dark text and an underlined bold Restore
  link. Restore re-applies the changes. Suggested implementation: keep the sketch-edit undo
  snapshot.
- When Extrude or Revolve is picked from the sketch toolbar, the sketch is accepted and the feature
  dialog opens with **all regions pre-selected**.

## Sketch toolbar (`toolbarSketchbase.png`, `sketch-toolbar-01.png`)
Left to right. ▾ marks a dropdown whose button shows the last-used member.
- Undo, redo | **Use ▾** (Use/project; the newer toolbar also has Intersection) and a sketch
  image-import tool.
- Line.
- Rectangle ▾ (corner, center-point, aligned).
- Circle ▾ (center-point, 3-point, ellipse…).
- Arc ▾ (3-point, tangent, center-point, conic…).
- Polygon ▾ (inscribed, circumscribed).
- Spline ▾.
- Point, Text, Construction (toggle), a Sketch image / pattern-like tool.
- Then: Fillet ▾, Trim ▾ (trim, extend, split), Offset ▾, Mirror, Pattern ▾ (linear, circular),
  DXF/DWG ▾, **Dimension**, and **Constraints ▾** (see `constraints.md`).
- "Search tools… alt/⌥+c" is on the far right.
- **Icon style:** 1.5 px dark-grey (#333) outline icons of about 18–20 px. The dropdown caret is a
  small blue ▾ (#2f6bba-ish) right of the icon.
- **S** opens a floating 2-row mini toolbar at the cursor (`sketch_toolbar_mini-01.png`) with the
  user's favorite sketch tools.

## Status colors and line styles (`line-styles-sketch-act-inact.png`, `line-styles-construction-act-inact.png`, `constraints/definedsketchesandconstraints.png`)
| State | Line | Points | Notes |
|---|---|---|---|
| Active sketch, under-constrained | **Pure blue #0000ff** (anti-aliased thin lines read #4648f4), about 1.5 px | Blue filled dots, about 6 px | |
| Active, fully constrained | **Black #000000** | Black dots | |
| Active, over-constrained or conflict | **Red #be0000** | Red dots | Red dimensions; glyphs white on red |
| Active, hovered | **Orange** (hover) | | See `NOTES.md` live orange |
| Active, selected | **Yellow-orange #f6bc1a to #e6b030** | | |
| Inactive (accepted) sketch | **Mid grey (#8a8f94-ish)**, thin | Small grey dots | Selected: yellow-orange |
| Construction | The same colors as above, **dash-dot** pattern | | |
| Closed region fill (active and inactive) | **#e3e6e9** (a light cool grey) | | |
| Dimension, driving | Black | | |
| Dimension, driven | Light blue-grey (#95a1ab text, #d4d8dc lines) | | |

- The **constraint status of individual points and edges is shown independently**: a rectangle
  with one fixed corner shows black edges touching the origin and blue free edges (live `12`).
- Priority when picking overlapping points: blue or red points before black points.
