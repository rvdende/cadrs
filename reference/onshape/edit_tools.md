# Edit tools: Trim, Extend, Split, and the Normal constraint (web-sourced)

Sources are listed at the end, fetched on 2026-09-25. Images are in `edit_tools/` (git-ignored;
a copy is in the main checkout's `reference/onshape/edit_tools/`). They come from
`https://cad.onshape.com/help/Content/Resources/Images/…`.

## Toolbar placement
- `entity_tools/sketch-toolbar-01.png` shows a **Trim ▾** (scissors with a caret) between
  Fillet ▾ and Offset ▾. Onshape groups **Extend** with Trim. The Split help page does not say
  where Split lives; cadrs puts it in Trim ▾ too (Trim, Extend, Split).
- Shortcuts (`shortcut_keys.htm`, `shortcuts.md`): **M** Trim, **X** Extend, **Shift+K** Normal.
  Split has none.

## Trim (`trim.htm`; `trim-points-01/02/03.png`)
- "Trim a curve to the first intersecting point or bounding geometry." With no intersection,
  the whole curve is deleted. Standalone sketch points are deleted too.
- Hover (`trim-points-03.png`): the piece a click removes is drawn in the **hover tan** (a wide
  light-orange band), with dots at its ends; the rest of the curve keeps its colour.
- Click-drag (`trim-points-02.png`): a **thin grey trail** follows the cursor; every piece the
  trail touches is trimmed as the trail reaches it (the right frame shows them gone while the
  trail is still being drawn).
- `trim-points-01.png`: hovering a lone point shows a tan disc; clicking deletes it.

## Extend (`extend.htm`; `extend-line-01/02.png`, `extend-arc-01/02-01/03.png`)
- "Extend a line or arc to the first intersecting point or bounding geometry. If no
  intersection exists, the geometry terminates at the release point."
- The picked part of the curve is highlighted (yellow-green), and the extension shows as a
  **light-blue** line (or arc) from the end to the cursor; the result stops at the first curve
  in the way (`extend-line-02.png`), or at the cursor when nothing is in the way
  (`extend-line-01.png`). An arc extends round its circle (`extend-arc-*.png`), and the closed
  region shades.
- The course (S17.3) describes it as click, move, click; the help as a press-drag-release. cadrs
  takes both. cadrs's preview stops at the boundary rather than running on to the cursor, so
  the preview is the result.

## Split (`sketch_split.htm`; `bezier-split-01/02.png`)
- "Click the sketch curve to split; click one or more locations along the curve." Open curves
  need one point, **closed curves two or more**.
- Hovering highlights the curve (tan) with the split point under the cursor
  (`bezier-split-01.png`, middle). The pieces are coincident at the split point; moving one
  moves the other (`bezier-split-02.png`).

## Normal constraint (`normal.htm`; `normaliconLG.png`)
- "Make a line and curve, or a curve and a plane, normal to each other." Tool-first: each pair
  picked is constrained, the tool stays on until toggled off.
- The icon is a line meeting an arc square to it. cadrs uses its own glyph with the same idea.
- For a circle or arc, normal means the line's direction passes through the center. cadrs also
  does a line and an ellipse (the line square to the ellipse where its nearer end meets it).

## Sources (fetched 2026-09-25)
- https://cad.onshape.com/help/Content/Sketch/trim.htm
- https://cad.onshape.com/help/Content/Sketch/extend.htm
- https://cad.onshape.com/help/Content/Sketch/sketch_split.htm
- https://cad.onshape.com/help/Content/Sketch/normal.htm
- https://cad.onshape.com/help/Content/Sketch/sketch_tools.htm (toolbar image)
