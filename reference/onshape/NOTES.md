# Onshape reference: observed behavior (captured 2026-09-24, live session, Chrome at 1148×1059)

Screens are in `screens/`, numbered in flow order. This is the primary reference for M1–M9, and it
wins over web-sourced material. The walkthrough: create a document, sketch a rectangle on Top,
dimension it to 50 × 30 mm, extrude 25 mm.

## Landing: documents page (`01*`, `02`, `03`)
- **Top bar, about 36 px, light grey:** logo on the left; a centered search box
  ("Search in Owned by me") with a dropdown and a search button; on the right, notifications with
  a badge, apps, a help "?" dropdown, and the user avatar with their name.
- **Left sidebar, about 200 px:**
  - A large blue **Create ▾** button at the top.
  - Filters: Explore Onshape, Owned by me (active: blue text and a blue left bar), Recently
    opened, Created by me, Shared with me, Teams ▸, Labels ▸, Public, Trash.
  - "Subscription: Free" and a "Plans and pricing" button at the bottom.
- **Main area:**
  - A heading with an icon ("Owned by me").
  - Collapsible rows: "Getting started", "Last opened by me", "Folders".
  - A "+ Add" link.
  - A **list view**, not a grid, with columns Name | Modified | Modified by | Owned by. Each row is
    about 38 px: a thumbnail of about 60×34 px (isometric render of the part), the name, a globe
    icon if public, and a "⚲ Main" branch tag. Dates look like "2:19 PM Sep 22".
  - A thin footer with copyright and version.
- **Create menu:** Document…, Folder…, a separator, Import files…, Import from ▸, Label….
- **New document dialog:** centered modal with a title and ✕.
  - "Document name" field, prefilled with "Untitled document" and fully selected.
  - "Document labels" search field.
  - "Document location" folder browser (home, back, current folder).
  - Blue primary button ("Create public document" on the free plan; ours says "Create") and a grey
    Cancel button.
  - The dialog fades in over about 150 ms and dims the page behind it.
- After Create, the new document opens directly. A "Loading studio data…" spinner shows first.

## Document shell (`05*`)
- **Document top bar:**
  - Logo, a ☰ menu, the document name in large bold text, a "Main" branch label in grey.
  - Icons with counters: link, public globe, versions, branches, likes.
  - On the right: "Explore Onshape", notifications, a blue **Share** button, help, user.
- **Left icon rail, about 36 px:** feature list toggle, insert, comments, details, properties,
  history, and so on.
- **Toolbar row, about 32 px:**
  - Feature list toggle, undo, redo.
  - **✎ Sketch** (the only text button).
  - Then icon buttons (extrude, revolve, sweep, loft, thicken, …), several with ▾ dropdowns, and
    thin separators between groups.
  - A "Search tools… alt+c" box on the right.
- **Feature list panel, about 190 px:**
  - A filter input "Filter by name or type".
  - "Features (4)" with buttons for new folder, rollback pause and timer.
  - A "Default geometry" tree: Origin, Top, Front, Right, with plane icons.
  - A horizontal grey rollback bar under the last feature.
  - A lower section "Parts (0)" that is collapsible.
- The panel has a small toggle tab on its right edge, about halfway down.
- **Viewport:**
  - White and light-grey background.
  - Three default planes as **translucent pale-blue squares** with a thin blue outline. Each
    label is in blue italic text, drawn in-plane at the plane's corner.
  - Origin shown as a small black circle with a dot.
  - Default camera is orthographic, a trimetric or isometric-like view with **Z up**.
- **View cube**, top right: a cube with Top/Front/Right faces, curved rotate arrows around it,
  ◀▶▲▼ nudge arrows, an axis triad (X red, Y green, Z blue), and a render-mode cube icon ▾ below.
- **Right edge:** a small vertical strip of panel toggles (appearance, …).
- **Bottom tab bar, about 26 px, grey:**
  - A search icon, a **+** button ("Insert new tab").
  - Tabs with icons: "Part Studio 1" (active: white with a blue underline), "Assembly 1".
  - **New documents start with Part Studio 1 and Assembly 1.**
- **+ menu:** Applications ▸; Create Material Library, Feature Studio, CAM, PCB and Render
  Studios; a separator; **Create Part Studio, Create Assembly**, Create Variable Studio, Create
  Drawing…, Create folder, Import…. For us, only Part Studio and Assembly work; show the others
  disabled or leave them out.

## Planes and selection (`06`)
- Hovering a plane changes its outline to **orange**; the fill stays the same.
- Hover works in both the viewport and the feature list.

## Sketch lifecycle (`07`–`09`, `20`)
- Clicking **Sketch** does the following:
  - The feature dialog **"Sketch 1"** (title in red until valid) appears at the top left over the
    viewport, about 200 px wide.
  - Green ✓ and red ✕ buttons are in the dialog header.
  - The "Sketch plane" field is highlighted blue and waiting for a selection.
  - Checkboxes: Disable imprinting, Show constraints ✓, Show expressions, Show errors ✓.
  - A toast "ⓘ Select a sketch plane ✕" appears at the top center.
  - "Sketch 1" is added to the feature list, selected, in red.
  - The toolbar switches to the **sketch toolbar**, greyed out until a plane is chosen.
- After choosing a plane (clicking it in the viewport), the field shows "Top plane ✕":
  - **The view does NOT rotate automatically.**
  - A larger plane rectangle is drawn to show the sketch plane.
  - The sketch toolbar becomes active.
- **N** switches to the normal-to view with an animation. The view cube shows only "Top" with the
  X/Y triad. The plane is a pale square; the other two planes are seen edge-on as blue lines
  through the origin. **No grid.**
- **Sketch toolbar, left to right:**
  - Feature list, undo, redo, then use/project and a sketch-specific tool.
  - Line ▾, Rectangle ▾, Circle ▾, Arc ▾, Polygon ▾, Spline ▾, Point, Text, Construction.
  - Then fillet ▾, trim ▾, offset ▾, mirror, pattern ▾, DXF ▾, dimension, constraints ▾, and
    Search tools.
- **Accepting (✓):**
  - The sketch shows as a grey filled region with thin grey edges; dimensions and constraint
    glyphs are hidden.
  - The view stays where it is.
  - The plane label is shown ("Top").

## Drawing a corner rectangle (`10`–`14`), shortcut **G**
- Hovering the origin draws an **orange square highlight** on the point: the snap target.
- Click the first corner, then move:
  - A light-blue rubber-band rectangle follows the cursor.
  - **Live dimension values** appear in light blue text next to the width (below) and the height
    (left side), shown to 5 decimals, e.g. 49.12062.
- Click the second corner:
  - Four lines are created. Corners are blue dots; edges touching the axes are **black**
    (constrained); free edges are **dark blue**.
  - The closed region is **filled grey**.
  - **Constraint glyphs** appear in small white boxes near the geometry: perpendicular at corners,
    parallel on a side, "⊥ —" (horizontal) on the top edge, coincident at the origin.
  - A **quick-dimension input box** appears on the width with the value selected. Typing replaces
    it. Enter commits and creates a driving width dimension with arrows, then **automatically
    opens the height box**. Enter there commits the height.
  - Tab from the width box appended the unit ("5 mm") rather than moving to height.
- **Expected end state:** a 50 × 30 rectangle, fully constrained, **all black**, with dimension
  lines showing arrows and centered values ("50", "30").

## Dimension tool (`16*`), shortcut **D**
- Hovering an edge highlights it **orange**.
- Clicking the edge makes the dimension follow the cursor with extension lines and a value in dark
  text.
- Clicking to place opens an **edit popup** with the value selected. Type a value, then Enter.
- The edge colors update right away to show constraint status.

## Line tool and inference (`18*`, `19*`), shortcut **L**
- Hovering a line's midpoint shows an **orange square at the midpoint** plus a midpoint glyph
  (`-•-`) near the cursor.
- Hovering elsewhere on a line highlights the **whole line orange** (point-on-curve target).
- Moving away from geometry shows no inference lines unless a reference point was hovered first.
- Rubber-band line: light blue with a **live length** label, 5 decimals, placed next to the line.
- Snapping tolerance is tight: 6 px off vertical did not snap to vertical.
- Esc cancels the line in progress; a second Esc leaves the tool.

## Errors (`15*`)
When a constraint conflict occurs:
- The affected geometry turns **red**.
- The feature name in the list turns red with a ⓘ.
- A yellow banner appears at the top: "⚠ Some constraints are not applicable … and have not been
  solved. ✕".

## Isometric (`21`)
- **Shift+7** switches to isometric. This is a different angle from the new-document default view
  shown in `05`.

## Extrude (`22`–`24`)
- **Extrude button:** the first icon after Sketch; the dialog is "Extrude 1".
  - Tabs: Solid | Surface | Thin.
  - Sub-tabs: New | Add | Remove | Intersect.
  - A selection field "Faces and sketch regions to extrude".
  - End type "Blind ▾" with a flip-direction icon.
  - "Depth 25 mm" (default 25 mm) with a measure icon.
  - Collapsible options: Direction, Starting offset, Symmetric, Draft, Second end position.
  - A detail slider at the bottom.
- Clicking the sketch region selects it: it turns **orange**, the field shows "Face of Sketch 1",
  and a **live translucent preview** appears with an **arrow manipulator** for dragging the
  depth. "Part 1" shows under Parts right away.
- **After ✓:**
  - The solid is **blue-grey**: top face light blue, sides darker slate, black edges.
  - Sketch 1 is greyed out in the list (consumed), with Extrude 1 listed below it.
  - Parts (1) contains Part 1.
- Hovering a face gives an **orange outline** around it.
