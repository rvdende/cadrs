# Box (window and crossing) selection (web-sourced)

Sources:
- Help page "Selection" (`Home/selection.htm`, last updated 2026-09-22).
- Tech Tip "Using Box or Drag Selection in Onshape Sketches" by M. Souders, 2023-11-07.

Images: `box_select/boxselectleft-right-partstudio.png` and
`box_select/boxselectright-left-partstudio.png`. They are Part Studio examples; the help says the
same behavior applies in Sketch, Part Studio and Assembly.

## Rules
- The box starts with a **left-button press on empty space** followed by a drag. A press on an
  entity selects it or starts dragging it instead.
- **Left → right = window ("enclosing") selection.**
  - Selects only entities **entirely inside** the box.
  - The box has a **solid blue outline** and a translucent blue fill.
- **Right → left = crossing selection.**
  - Selects entities inside the box **and any the box touches or crosses**.
  - The box has a **dashed (dotted) yellow outline** and a translucent yellow fill. The help says
    "yellow"; the tech tip calls it "orange".
- The direction is decided by the drag's horizontal sign relative to the start point, and it can
  flip live while dragging. Only horizontal direction matters.
- **Additive:** a box selection **adds** to the current selection; it does not replace it.
  Onshape selection is always additive and toggling: a click on a selected entity deselects it.
- **Ctrl + drag** makes a **deselect** box, removing the entities it picks from the selection.
- Clear the selection by clicking empty space (without dragging), pressing **Space**, or choosing
  "Clear selection" in the context menu.
- The **origin point can be box-selected** too. The tech tip notes you often have to click it off
  again.
- The cursor shows a count badge of selected entities, capped at "5+".

## Measured colors (from the help images; they are downscaled, so treat edges as approximate)
| Element | Window (L→R) | Crossing (R→L) |
|---|---|---|
| Fill (composited on white) | **#d1dfee** | **#fff3cf** |
| Outline (anti-aliased, 1 px) | about **#6f9ac9** (true color likely a mid blue, about #5a8ccb) | about **#ffe28c** (true color likely #ffd24d to #ffcc33), **dashed** |
| Fill opacity estimate | a blue of about #5a8ccb at 25–30 % | a yellow of about #ffcc33 at 25 % |

- Selected entities render **yellow-orange**: sampled edges are **#f5c128 / #f2c131**. Entities
  highlighted inside a sketch look similar (the hover and selected line in
  `sketch_dialog/line-styles-sketch-act-inact.png` is about #e6b030 to #f6bc1a).
- Suggested tokens for cadrs:
  - `selection.window.fill = rgba(90,140,203,0.28)` and `selection.window.stroke = #5a8ccb`,
    solid, 1 px.
  - `selection.crossing.fill = rgba(255,204,51,0.25)` and `selection.crossing.stroke = #f0b400`,
    dashed 4/3, 1 px.

## Sketch specifics (from the tech tip)
- Example: a plate outline plus 4 holes. The L→R window catches only the fully enclosed holes. The
  R→L crossing catches all 4 holes without the plate outline, because the box crosses the holes
  but not the plate's edges.
- Selected sketch entities can then receive a constraint (for example Equal) in the
  select-then-tool workflow; see `constraints.md`.
