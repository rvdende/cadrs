# Keyboard shortcuts and view navigation (web-sourced)

Sources: help page "Keyboard Shortcuts" (`shortcut_keys.htm`) and "View Navigation and the View
Cube", both marked last updated 2026-09-22. Image: `shortcuts/keyboard-shortcut-dialog-03.png`
shows the in-app shortcut dialog, opened with **Shift+/**. The live captures in `NOTES.md` confirm
G, D, L, N and Shift+7.

Keys are case-insensitive letters; "Shift+x" means hold Shift. The same key can do different
things in different contexts (sketch, Part Studio, Assembly). The sketch binding wins while a
sketch is being edited.

## Shortcut dialog (`shortcuts/keyboard-shortcut-dialog-03.png`)
- A modal about 994×658 px with the title "Keyboard shortcuts" and a blue "Customize keyboard
  shortcuts" link.
- A "Search shortcuts" box at the top right, plus pop-out ↗ and close ✕ buttons.
- Tabs: General | Part Studio | Assembly | 3D view | Sketch | Feature Studio | Drawing. The active
  tab is blue text with a blue underline.
- Each row is right-aligned **keycaps** (small bordered boxes such as `shift` `enter`), then a
  left-aligned description. A 🔒 marks Onshape-controlled bindings that cannot be rebound.
- A legend footer: Customized (blue-bordered keycap), Onshape default, Disabled (struck through),
  Onshape controlled, Has conflict (red !), and "* applies in drawing sketches".

## General
| Key | Action |
|---|---|
| Enter | Accept (commit) the active command or dialog |
| Shift+Enter | Accept, then reopen the same command |
| Esc | Cancel or exit the current operation or tool |
| Space | Clear selection |
| Delete / Backspace | Delete the selection |
| Ctrl+Z / Ctrl+Y (Shift+⌘+Z on Mac) | Undo / redo |
| Ctrl+C / Ctrl+V | Copy / paste a feature |
| Q | Toggle the Construction tool |
| S | Shortcut toolbar (a floating mini toolbar at the cursor) |
| Alt+C | Focus the "Search tools…" box |
| Alt+T | Tab manager |
| Ctrl+Space | Cycle recent tabs |
| Shift+/ | Keyboard shortcuts dialog |
| ` (backtick) | "Select other": list the selectable entities under the cursor |
| Shift+C | Curve and surface analysis (out of scope) |
| Shift+D | Dihedral analysis (out of scope) |
| [ | Measure dialog |

## Part Studio
| Key | Action |
|---|---|
| Shift+S | New Sketch feature |
| Shift+E | Extrude |
| Shift+W | Revolve |
| Shift+F | Fillet |
| Shift+H | Show or hide all sketches |
| Rollback bar selected, then ↑/↓ | Move the rollback bar |

## 3D view (Part Studio, Assembly and Sketch)
| Key | Action |
|---|---|
| Shift+1 … Shift+6 | Front, Back, Left, Right, Top, Bottom |
| Shift+7 | Isometric |
| N | Normal to the selected plane or face; in a sketch, normal to the sketch plane |
| F | Zoom to fit |
| W | Zoom to a window or bounding box |
| Z / Shift+Z | Zoom out / zoom in |
| ← → ↑ ↓ | Rotate 15° |
| Shift+arrows | Rotate 90° |
| Ctrl+arrows | Rotate 5° |
| Ctrl+Shift+arrows | Pan |
| P | Show or hide planes |
| Shift+P | Hide construction geometry |
| Y / Shift+Y | Hide the selected instances / show hidden instances |
| Shift+I | Isolate |
| Shift+T | Transparent |
| Shift+X | Section view |
| Shift+V | Named views |
| Shift+R | High-quality render |
| Alt+click | Select through a transparent entity |

## Sketch
| Key | Tool | Key | Tool |
|---|---|---|---|
| L | Line | Shift+A | Switch between Line and Tangent arc (while drawing a line) |
| G | Corner rectangle | R | Center-point rectangle |
| C | Center-point circle | A | 3-point arc |
| Shift+S | Sketch point | Q | Construction toggle |
| D | Dimension | U | Use (project/convert) |
| O | Offset | M | Trim |
| X | Extend | Shift+F | Sketch fillet |
| I | Coincident | Shift+O | Concentric |
| B | Parallel | T | Tangent |
| H | Horizontal | V | Vertical |
| Shift+L | Perpendicular | E | Equal |
| Shift+M | Midpoint | Shift+K | Normal |
| Shift+G | Pierce | Shift+Q | Symmetric |
| Shift+J | Fix | Shift+U | Curvature |
| **Shift (held)** | **Suppress automatic inferences (no snapping or auto-constraints)** | | |

The following tools have no default key: 3-point circle, tangent arc (reached through Shift+A from
Line), center-point arc, polygon, spline, text, mirror and pattern.

Notes:
- The sketch shortcut list has **no N entry**. N comes from the 3D view set.
- When the dimension or quick-dimension field has focus, typing goes into the field. Per
  sketch_basics, typing "#" with the field *not* focused triggers a view key. Our app must route
  keys to the text field first.

## Assembly (for later milestones)
| Key | Action |
|---|---|
| I | Insert parts and assemblies |
| M | Fastened mate |
| J | Show or hide mates |
| H | Toggle show-mates mode |
| K | Show or hide mate connectors |
| Ctrl+M | Mate connector |
| Shift+S | Snap mode |
| Shift (held) | Lock mate inference |
| Shift+N | Rename the selected instance |
| A | Flip primary axis (in the mate connector dialog) |

## Mouse and view navigation (desktop defaults, Windows and Linux)
| Action | Binding |
|---|---|
| Rotate | **Right-button drag** |
| Rotate without roll | Alt+right-drag: horizontal motion turns about the model's up axis, vertical motion pitches |
| Snap to nearest "floor down" view | Alt, which animates to the nearest view without roll (uncertain exactly when it triggers; low priority) |
| Pan | **Middle-button drag** or **Ctrl+right-drag** |
| Zoom | **Wheel**: up zooms in, down zooms out. Onshape zooms about the cursor (widely observed, not stated in the help page) |
| Select | Left click. Selection is additive and toggles; see `box_select.md` |
| Context menu | Right click without dragging. A right press that moves is a rotate, so open the menu on release only if the mouse did not move |
| View cube face or corner click | Animate to that standard view (corner = trimetric or isometric) |
| View cube arrow click | Rotate 15°; Shift+click 90°; Ctrl+click 5° |

- Mouse bindings can be changed in the account preferences, where other CAD presets are offered.
  We only need the defaults above.
- **View cube dropdown** (`view/view-cube-menu4-01.png`, the cube icon ▾ under the view cube):
  - Isometric, Dimetric, Trimetric.
  - Graphics preferences…
  - Named views…, Previous view.
  - Zoom to fit, Zoom to window.
  - Perspective view, Orient normal to sketch on edit.
  - Shaded with edges ▸, Hidden edges removed ▸, Tangent edges visible ▸.
  - View in high quality, Highlight boundary edges, Section view….
  - Separators appear between groups, and checked items show a ✓ on the left. "Orient normal to
    sketch on edit" is an option and is **off** by default, which matches the live capture: no
    automatic rotation.
