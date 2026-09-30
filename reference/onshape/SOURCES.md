# Sources for web-sourced Onshape reference material

Fetched 2026-09-24. Unless noted, images come from the Onshape Help Center, which is public. Each
help page carries the footer "Last Updated: September 22, 2026", but individual screenshots may
be older, and some show 2023–2024 UI. The live captures in `screens/` (see `NOTES.md`) take
priority over these whenever they disagree.

These images are for internal reference only. Do not ship them or copy the icons in them; use
Tabler or Lucide icons instead (see CLAUDE.md).

Image base URL: `https://cad.onshape.com/help/Content/Resources/Images/`, shortened to `…/` below.
The two files marked (BMP→PNG) were served as BMP despite their .png names and were converted
locally.

## Topic docs → help pages
| Doc | Help pages (`https://cad.onshape.com/help/Content/…`) |
|---|---|
| `circle_arc.md` | `Sketch/center_point_circle.htm`, `Sketch/3_point_circle.htm`, `Sketch/3_point_arc.htm`, `sketch-tools-arc_tangent.htm`, `Sketch/center_point_arc.htm`, `Sketch/line.htm` |
| `constraints.md` | `Sketch/working_with_constraints.htm`, `Sketch/coincident.htm`, `concentric.htm`, `parallel.htm`, `tangent.htm`, `horizontal.htm`, `vertical.htm`, `perpendicular.htm`, `equal.htm`, `midpoint.htm`, `normal.htm`, `symmetric.htm`, `fix.htm` (all under `Sketch/`) |
| `box_select.md` | `Home/selection.htm`; Tech Tip https://www.onshape.com/en/resource-center/tech-tips/box-drag-selection-sketch (2023-11-07, M. Souders) |
| `inference.md` | `Sketch/automatic_inferencing.htm`, `Sketch/construction.htm`, `Sketch/sketch_basics.htm`; Tech Tip https://www.onshape.com/en/resource-center/tech-tips/2-ways-the-shift-key-can-increase-productivity (2022-05-24) |
| `dimension.md` | `Sketch/dimension.htm`; forum https://forum.onshape.com/discussion/3773/force-radius-or-diameter-dimension-in-a-sketch (2016, reply 2024-10) |
| `sketch_dialog.md` | `Sketch/sketch_basics.htm`, `Sketch/sketch_tools.htm`, `Sketch/working_with_constraints.htm` |
| `shortcuts.md` | `shortcut_keys.htm`, `View/view_navigation_and_the_view_cube.htm` |
| `tab_menu.md` | `Document/document_tabs.htm`, `Assembly/assembly.htm`, `PartStudio/part_studios.htm` |
| `assembly_empty.md` | `Assembly/assembly.htm`, `Assembly/instances_list.htm`, `Assembly/insert_parts_and_assemblies.htm` |

## Images
| File | Source URL | Shows |
|---|---|---|
| `circle_arc/SemicircleSnap.png` | …/SemicircleSnap.png | 3-point arc semicircle snap: yellow snap squares, R label, midpoint glyph |
| `circle_arc/line-to-arc-01.png` | …/sketch-tools/line-to-arc-01.png | Line → tangent-arc switch (cursor glyph), rubber-band arc |
| `constraints/constraints-01.png` | …/sketch-tools/constraints-01.png | The Constraints ▾ menu with all 14 entries and their shortcuts |
| `constraints/constraints-referenced.png` (BMP→PNG) | …/concepts/constraints-referenced.png | A constraint to an external entity (Use) with a light-blue glyph background |
| `constraints/constrainticonsonhover.png` | …/sketch-tools/constrainticonsonhover.png | A hovered line (orange) with its glyph shown |
| `constraints/definedsketchesandconstraints.png` | …/tutorial/definedsketchesandconstraints.png | Blue, black, driven-grey and red (over-constrained) squares |
| `constraints/constraintsviewingallthroughdialog1.png` | …/sketch-tools/constraintsviewingallthroughdialog1.png | Show constraints on: glyph rows, Ø dimensions, the dialog |
| `constraints/midpoint-3-03.png` | …/sketch-tools/midpoint-3-03.png | Glyph boxes over a region, selected points (orange), cursor with selection count "2" |
| `constraints/constraint-manager-01.png` | …/sketch-tools/constraint-manager-01.png | Constraint manager panel |
| `inference/automaticinferencingexample2.png` | …/concepts/automaticinferencingexample2.png | Wake-up then vertical-alignment dotted line |
| `inference/parallelinferencing.png` | …/sketch-tools/parallelinferencing.png | Parallel inference: dotted new line, orange reference line, ⫽ glyph |
| `inference/sketch-coplanar-inference.png` | …/sketch-tools/sketch-coplanar-inference.png | Circle rubber band (light blue, live Ø label) snapping to a vertex of another sketch |
| `box_select/boxselectleft-right-partstudio.png` | …/concepts/boxselectleft-right-partstudio.png | Window selection (blue) and its result |
| `box_select/boxselectright-left-partstudio.png` | …/concepts/boxselectright-left-partstudio.png | Crossing selection (yellow) and its result |
| `dimension/dimension-highlights-01.png` | …/sketch-tools/dimension-highlights-01.png | Two points picked for a dimension, highlighted |
| `dimension/dimension-active-field-01-01.png` | …/sketch-tools/dimension-active-field-01-01.png | Dimension value field with ✓/✕ |
| `dimension/dimension-simple-example-01.png` | …/sketch-tools/dimension-simple-example-01.png | A placed dimension |
| `dimension/dimensioncircumference.png` | …/sketch-tools/dimensioncircumference.png | Diameter dimension on a circle |
| `dimension/dimensionradius.png` | …/sketch-tools/dimensionradius.png | Radius dimension on an arc |
| `dimension/dimension-anglequadrants.png` | …/sketch-tools/dimension-anglequadrants.png | Angle quadrant chosen by cursor placement |
| `dimension/dimensionangle.png` | …/sketch-tools/dimensionangle.png | Angle between lines |
| `dimension/dimensionlineardistance-02.png` | …/sketch-tools/dimensionlineardistance-02.png | Horizontal and vertical distance between points |
| `dimension/dimensionshortestdistance-02.png` | …/sketch-tools/dimensionshortestdistance-02.png | Direct (aligned) distance |
| `dimension/dimension-driven-01.png` | …/sketch-tools/dimension-driven-01.png | Driven dimension and dimension context menu |
| `dimension/sketchdims-line.png` | …/sketch-tools/sketchdims-line.png | Live length while drawing |
| `dimension/sketchdims-linebox.png` | …/sketch-tools/sketchdims-linebox.png | Quick-dimension box after a segment |
| `dimension/sketchdims-linedim.png` | …/sketch-tools/sketchdims-linedim.png | Polyline with 2 of 3 segments dimensioned |
| `tab_menu/tab-bar-wmenu-01.png` | …/feature-tools/tab-bar-wmenu-01.png | Tab bar with the tab context menu open |
| `tab_menu/partstudiormbmenu.png` | …/concepts/partstudiormbmenu.png | Tab context menu (tab manager variant) |
| `tab_menu/insert-new-tab-menu.png` (BMP→PNG; originally `tabmanager.png`) | …/concepts/tabmanager.png | The + Insert-new-tab menu |
| `tab_menu/Delete-tab-dialog.png` | …/concepts/Delete-tab-dialog.png | Delete tabs warning modal |
| `tab_menu/tab_manager_open-01.png` | …/concepts/tab_manager_open-01.png | Tab manager panel |
| `assembly_empty/assembly-context-menu-01.png` | …/assembly/assembly-context-menu-01.png | Assembly tab context menu |
| `assembly_empty/Assemblytoolbar-scrnshot.png` | …/icons/Assemblytoolbar-scrnshot.png | Assembly toolbar with the Insert text button |
| `assembly_empty/assemblyelementui-01.png` | …/assembly/assemblyelementui-01.png | Assembly tab UI: instances list, + menu, view cube |
| `assembly_empty/assemblyfeaturelist2-01.png` | …/assembly/assemblyfeaturelist2-01.png | Instances list |
| `assembly_empty/insert-command.png` | …/assembly/insert-command.png | Insert parts and assemblies dialog |
| `sketch_dialog/sketchdialogblank-01.png` | …/sketch-tools/sketchdialogblank-01.png | Blank sketch dialog (plane field waiting) |
| `sketch_dialog/constraint-manager-00.png` | …/sketch-tools/constraint-manager-00.png | Sketch dialog with plane set, diagnostic menu |
| `sketch_dialog/sketch-toolbar-01.png` | …/sketch-tools/sketch-toolbar-01.png | Current sketch toolbar |
| `sketch_dialog/toolbarSketchbase.png` | …/icons/toolbarSketchbase.png | Sketch toolbar (larger) |
| `sketch_dialog/sketch_toolbar_mini-01.png` | …/sketch-tools/sketch_toolbar_mini-01.png | S-key shortcut toolbar |
| `sketch_dialog/line-styles-sketch-act-inact.png` | …/sketch-tools/line-styles-sketch-act-inact.png | Active, selected, inactive and inactive-selected sketch line styles |
| `sketch_dialog/line-styles-construction-act-inact.png` | …/sketch-tools/line-styles-construction-act-inact.png | Construction line styles (dash-dot) |
| `sketch_dialog/restore.png` | …/sketch-tools/restore.png | "Sketch 1 has been cancelled. Restore" toast |
| `shortcuts/keyboard-shortcut-dialog-03.png` | …/concepts/keyboard-shortcut-dialog-03.png | Keyboard shortcuts dialog (Shift+/) |
| `view/view-cube-menu4-01.png` | …/concepts/view-cube-menu4-01.png | View cube and its dropdown menu |

## Not captured / gaps
- There is no public screenshot of an **empty** Assembly tab; `assembly_empty.md` infers it.
- **Double-click tab rename** is undocumented and unconfirmed.
- The Tech Tip box-select article's inline images load via JavaScript and could not be fetched.
  The help-center images cover the same content.
- Exact inference pick radius and snap priority order are not published; `inference.md` gives
  inferred values.
