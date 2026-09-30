# Onshape "Introduction to Drawings": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Introduction to Drawings
(`learn.onshape.com/learn/course/introduction-to-2d-drawings/...`), read on 2026-09-25. The
requirements below are paraphrased, not the course text. Exercise step screenshots and a few UI
captures are in `intro-to-drawings/` (git-ignored, local only). The course's own images are small
(about 730×410 px per step, and about 380×300 px for the overview drawings), so small dimension
text is only just readable. The values that matter are written out below.

The course has 16 outline entries in six sections: 7 short videos (key takeaways + transcript),
one interactive "click the area" self-check on the interface, 3 exercises (9–11 step carousels
each, then a "Completed Drawing" page), a final **Self-Check** quiz, and a **Completion Survey**.
The quiz, the interface hotspot self-check and the survey were **not** started, because that would
record attempts on the user's account. The exercises have **no** self-check of their own. Each one
ends with a link to a public "Completed Exercise" document to compare against.

**All three exercises start from a public Onshape document** ("Make a copy"). The documents hold
ready-made parts and assemblies, and the third one also holds a finished three-sheet drawing. The
part dimensions are only partly visible in the drawings, so **none can be rebuilt exactly from
scratch**. cadrs can reproduce the workflow with equivalent parts (see each exercise).

Each requirement has an ID (`D<lesson>.<n>`). Items already covered elsewhere are cross-referenced,
not repeated: [intro-to-parametric-cad.md](intro-to-parametric-cad.md) (`P*`; first- and
third-angle projection in P6, drawings linked to the model in X5),
[intro-to-part-studios.md](intro-to-part-studios.md) (`PS*`; the Hole feature and hole callouts in
PS15, Second end position in PS4.8), [intro-to-assemblies.md](intro-to-assemblies.md) (`A*`; the
assembly BOM panel in A20, standard content in A19), and
[intro-to-sketching.md](intro-to-sketching.md) (`S*`).

## 1. Drawings interface

### D1 Introduction to drawings (video, 3:43; image `lesson-create-drawing-dialog.png`)
- D1.1 A drawing is a 2D sheet made from a 3D part or assembly. It carries what manufacturing and
  assembly need: tolerances, size and fit, machining or welding notes, assembly instructions.
- D1.2 A drawing lives in a **Drawing tab** in the document. Create it with the tab bar's **"+" →
  Create Drawing…**. The same menu has Create Part Studio, Create Assembly, Create folder and
  Import…. Create Drawing is also on an assembly instance's context menu (A X15).
- D1.3 **Create Drawing: <name>** dialog (`lesson-create-drawing-dialog.png`):
  - Two tabs: **Existing templates** and **Custom template**.
  - A left source list: **Onshape** (built-in), My Onshape, Recently opened, Created by me, Shared
    with me, plus company and team libraries. The built-in templates only show when the
    **Onshape** filter is selected.
  - Filter tabs **All / ANSI / ISO**.
  - A table with **Template / Document / Owner** columns. The built-in files are named like
    `ANSI_A_INCH.dwt`, `ANSI_A_MM.dwt`, `ANSI_A_Portrait_INCH.dwt`, `ANSI_A_Portrait_MM.dwt`,
    `ANSI_B_INCH.dwt`, `ANSI_B_MM.dwt`, `ANSI_C_INCH.dwt`, and so on, and they're owned by
    "Onshape".
  - **Options**: **Four views** / **No views** tiles. Four views isn't available in the screenshot
    because nothing is selected; the exercises use **No views**.
  - **OK** / **Cancel**.
- D1.4 Built-in **ANSI** and **ISO** templates. Custom templates are made by editing a built-in one
  in the **Custom template** tab, or by uploading a company template. Templates can be shared
  across a company.
- D1.5 **Custom template** options include **Projection**: **First angle** or **Third angle**. This
  sets where projected views are placed (see P6.2 for the layout rules).
- D1.6 **Sheet sizes**:
  - ANSI: **A** = 8.5×11 in, **B** = 11×17 in, then C, D, E (each roughly doubles the area).
  - ISO: **A4** = 210×297 mm (8.3×11.7 in), **A3** = 297×420 mm (11.7×16.5 in), then A2, A1, A0.
  - Portrait variants exist (`…_Portrait_…`).
- D1.7 After **OK**, the drawing opens and the **Insert view** flow starts at once (D4.1).
- D1.8 A sheet normally has a size, a **border with zone labels** (columns 1, 2…, rows A, B…) and a
  **title block** in the bottom-right corner.
- D1.9 **Title block**: a table holding the scale, projection symbol, drawn by and date, approved
  by, company logo, tolerance block ("unless otherwise specified…"), "DO NOT SCALE DRAWING", title,
  size, drawing number, revision and "sheet n of m". Most fields are filled **parametrically**: the
  sheet scale, the projection, and the part or assembly properties (name, description, drawn and
  approved by and date). An empty property shows dashes. In the exercises the title field reads
  `<Part name>` / `Made by Onshape` (for example "Universal Joint Flange / Made by Onshape").

### D2 Drawings interface (video, 3:25; images `lesson-drawing-interface.png`, `lesson-drawing-properties-sheets-flyout.png`)
- D2.1 The middle of the tab is the **sheet**, on a grey background.
- D2.2 **Mouse**: left click selects, the wheel zooms, and right-drag or middle-drag pans. There's
  **no rotate**. The mouse mapping follows the account preferences.
- D2.3 **F** fits the sheet to the window.
- D2.4 **Drawing toolbar**, left to right:
  - Undo and Redo.
  - **Update from this workspace**, which turns **gold** when the drawing is out of date (D13).
  - View creation tools: Insert view, Projected, Auxiliary, and others in dropdowns.
  - Dimension tools (a dropdown).
  - Manufacturing annotations: **Weld symbol**, **Surface finish** and others in that group.
  - **Note** (A), **Callout**, **Table**, **Insert BOM**, and others.
  - **Centerline**, **Centermark**, **Virtual sharp**.
  - Sketch **Line** and **Spline**.
  - **Insert DXF/DWG** and **Insert image**.
- D2.5 **Drawing properties** panel: the **wrench icon** on the right edge of the sheet area opens
  it. It sets defaults for the **whole drawing** (every sheet), and the settings are saved into
  templates. It has icon tabs for **Units and precision**, **Dimensions**, **Annotations**,
  **Views**, **Construction geometry**, **Formats** and **Tables**.
  - Units and precision → **Primary**: Units (Inches), Decimal separator (Period), Precision
    (`0.123`), Tolerance precision (`0.123`), Angular precision (`0.1`).
  - **Dual**: Show dual dimensions, Show dual unit, Dimension location (Top), Units (Millimeters),
    Precision (`0.12`), Tolerance precision (`0.12`).
  - **Leading and trailing zeros**: Length leading zeros (off), Length trailing zeros (on in the
    screenshot), and so on.
  - Footer: **Update properties from a template…**, and **Lock drawing properties**.
- D2.6 A single dimension or annotation can override the drawing default through its own palette,
  for example dual units on one dimension (D6.6).
- D2.7 **Sheets flyout**: an icon on the left edge of the sheet area, or **Ctrl+S**. It shows
  "Sheets(n)" as a tree: each **sheet** (e.g. "Assembly", "Part") → the **referenced part or
  assembly** → its **views** (Front, Top, Right, Isometric). Projected views are nested under
  their parent view. There's an **Insert sheet** icon at the top right, and **double-clicking** a
  sheet makes it active.
- D2.8 **Sheet properties**: right-click empty sheet space → **Sheet properties…**, or right-click
  the sheet name in the flyout → **Properties**. It changes only that sheet: **scale**, **size**,
  **border and zones**, and the **referenced object**, which is the part or assembly that
  parametric notes and the title block read from (D9.4).
- D2.9 **View properties**: right-click a view → **View properties…**. It shows the referenced
  object (document, workspace or version, type, and a link that opens it), the **Scale**, and the
  **Sheet** the view is on.
- D2.10 A document's bottom tab bar holds the Drawing tabs next to Part Studio and Assembly tabs,
  with a drawing icon. The **tab context menu** (`ex1-step6.png`) has Delete, Open in new browser
  tab, **Rename…**, Properties…, Duplicate, Copy to clipboard, Change to version…, Select as
  document thumbnail, Move to document…, and **Export…**.

### D3 Self-check: drawing interface (interactive hotspot quiz; not started)
- "Click on each area in the Drawing interface described below", 6 items. Item 1 is **Sheets
  flyout**. The other five were not revealed without answering. Judging by D2 they're likely the
  toolbar, the Update icon, Drawing properties, the sheet, and the title block or document tabs.
  The screenshot the quiz uses is saved as `lesson-drawing-properties-sheets-flyout.png`; it shows
  the Universal Joint Flange drawing with the Sheets flyout and the Drawing properties panel open.

## 2. Creating views

### D4 View creation (video)
- D4.1 **Insert view** has two parts. One is the **Insert view** dialog at the top left. Its rows
  are: the referenced object, the **view orientation** (Front, Top, Right, Left, Back, Bottom,
  Isometric…, i.e. orthographic or a named view), the display state (e.g. "Show all", for
  assemblies), the **scale** (e.g. `1:2`), and a fourth option that shows "None" (the course
  doesn't explain it; probably an exploded view or named position). The other is the
  **Select a part or assembly** browser (`ex1-step4.png`, `ex2-step5.png`):
  - **Current document** and **Other documents** tabs, the document name and branch, and icons.
  - **Part Studios** and **Assemblies** sub-tabs, a **Search parts or sketches** field, and type
    filters (parts, sketches, surfaces).
  - A tree of Part Studios, which expand to their parts, or of assemblies (top level and
    subassemblies), with thumbnails.
- D4.2 **Reference a part, not a Part Studio.** Inserting a whole Part Studio works but isn't
  recommended, because its properties differ from a part's. That breaks parametric notes and the
  title block (D1.9, D9.4). In a single-part studio, expand the studio and pick the part. A check:
  the title block shows the part name when a part was picked.
- D4.3 Pick the item, set the orientation and scale, then **click on the sheet** to place the
  first (base) view.
- D4.4 After the first view, the tool **switches to Projected view** by itself. Move the cursor
  away from the parent view and click to drop an orthographic projection (Top, Bottom, Right,
  Left) or an **isometric** view (diagonal direction). To make several views, click the parent
  view again and then place the next one. **Esc** or clicking the tool ends it.
- D4.5 Projected orthographic views are **aligned to their parent** (see D7.1). Where they go
  follows the template's projection. The exercises use **third angle** (ANSI): top view above the
  front view, right view to the right, isometric in the top-right corner.
- D4.6 **Auxiliary view**: pick an edge in a view. The new view is folded 90° out from that edge,
  showing the face's true size and shape.
- D4.7 **Scale**: the **sheet scale** is taken from the first view inserted on the sheet. New views
  inherit it, and projected views inherit their parent's. You can change it in the Insert view
  dropdown, or later in View properties → **Scale** (a dropdown of ratios such as 1:1, 1:2, 1:4,
  2:1).
- D4.8 The **view context menu** (`lesson-view-context-menu.png`) has:
  - Show hidden lines, Tangent edges ▸, **Show shaded view**, Show threads, Show part
    intersections, Display state ▸.
  - Create projected view, View properties…, Bring to front, Send to back.
  - Insert BOM for <assembly>…, Switch to <referenced tab>, **Move to sheet…**, **Align view
    vertical**, **Align view horizontal**.
  - Clear selection, Zoom to fit, Delete.
  - Also Suppress alignment with parent and Show/hide sketches… (D7).
- D4.9 **Shaded view**: right-click → Show shaded view draws the part appearances (colors) instead
  of lines only. The exercise uses it on isometric views.
- D4.10 **Hidden lines**: right-click → **Show hidden lines** (dashed) or **Hide hidden lines**.
- D4.11 **Tangent edges** ▸ **Hidden** (not drawn), **Solid** (continuous lines) or **Phantom**
  (broken lines).
- D4.12 Other view types (section, detail, broken-out, break, crop) aren't taught here. The course
  points to the Fundamentals drawing course for them. Their icons are in the view-tools group.

### D5 Detailing views with geometric annotations (video)
- D5.1 The annotation tools sit toward the right of the toolbar.
- D5.2 **Centerline** shows symmetry and is often used on cylindrical faces. There are several
  modes:
  - **Point to point**.
  - **Line to line** (edge to edge), including hole silhouette edges. This puts the centerline
    halfway between them.
  - **Circle centerline**: either **3 points** on the circle (e.g. a bolt circle) or **2 points**
    (center, then a point on the circle).
  - It snaps to line midpoints and vertices, and to arc and circle quadrants and centers.
  - After placing, the ends can be dragged to extend the line (used in the exercise on
    counterbore holes).
- D5.3 **Centermark**: pick circle or arc edges. A cross is drawn at each center.
- D5.4 **Virtual sharp**: pick two straight edges that would meet at a corner hidden by a fillet or
  round. This marks the theoretical sharp point for dimensioning. It's drawn either as a
  **centermark-style mark** or as **edge extensions**, set in Drawing properties.

### D6 Detailing views with dimensions (video, 1:56)
- D6.1 Dimensions on a drawing are **driven** (reference, read from the model); they don't edit the
  model. The course also mentions "driving" dimensions, but in practice drawing dimensions only
  report model values.
- D6.2 **D** (or the toolbar icon) is the **smart dimension** tool. It makes linear, angle,
  diameter and radius dimensions from points, edges, arcs and circles, and the type is chosen from
  what you pick. Flow: pick the entities, then click to place the text.
- D6.3 **Orange snap points** show on geometry you hover with the dimension tool.
- D6.4 The dimension dropdown also holds specific tools that work the same way (pick two entities,
  then place):
  - **Radial** (**Shift+R**): pick a circle or arc to dimension its radius.
  - **Diameter** (**Shift+D**): pick a circle or arc to dimension its diameter.
  - Others are only named, not detailed: point-to-point, line-to-line, angular, baseline, ordinate,
    chamfer, arc length.
- D6.5 **Re-attaching** a dimension: select it and drag the grips at the ends of its extension or
  leader lines to other geometry. The current attachment is highlighted **blue**. The value
  updates to the new reference.
- D6.6 **Dimension palette**: select a dimension and hover the flyout icon that appears next to it.
  From there you can add prefix and suffix text, symbols, **tolerances**, **precision** and dual
  units for that one dimension.
- D6.7 **Hole callout** tool: pick a hole made with the **Hole feature** (PS15.9) to get a callout
  such as `Ø.266 THRU` or `4x Ø.266 THRU ⌴Ø.438 ↧.250`. The count prefix isn't automatic: right-
  click the callout → **Edit…** opens a small **Hole callout** dialog with a **Prefix** field (e.g.
  `4x`), with ✓ / ✗. Symbols: ⌴ counterbore, ↧ depth, Ø diameter.
- D6.8 Dimensions and annotations are drawn in black. When one is selected or being placed it's
  highlighted orange.

### D7 Adjusting views (video)
- D7.1 Orthographic projected views are **aligned** with their parent, so moving one moves the
  others to keep the alignment. Right-click → **Suppress alignment with parent** frees a view.
- D7.2 Views are dragged to move them on the sheet.
- D7.3 **Move to sheet…** (right-click): pick an existing sheet from a dropdown and accept. You can
  also use View properties → **Sheet**. The view's own annotations (labels, dimensions) go with it,
  but its **child views don't**.
- D7.4 **Show/hide sketches…** (right-click): a dialog with a searchable, checkable list of the
  referenced part's sketches. The checked sketches are drawn over the view. Unchecking them hides
  them again.
- D7.5 **Align view vertical** / **Align view horizontal** (right-click): then click an edge in
  the view. The view rotates so that edge is vertical or horizontal on the sheet.

### D8 Exercise: Universal Joint Drawing (`intro-to-drawings/ex1-*`, target `ex1-drawing.png`)
Goals: create a part drawing, insert views, add dimensions. The starting document is **public**
("Exercise: Universal Joint Drawing"). It holds one Part Studio, "Universal Joint", with 16
features (Main Body, Extrude 10–12, Sketch 8–9, Chamfer 10, …) and a single part, **Universal Joint
Flange**. That's a yoke: a round flange with 4 counterbored holes, and two lugs with a cross hole
and a 4-hole pattern. The course doesn't give the full part geometry, so it **can't be rebuilt
exactly**. An equivalent yoke can be modelled from the drawing's dimensions (inch):
- Flange **Ø4.750**, square pocket **3.282** × 2.061 (half-width) in the top view, **4x Ø.266
  THRU ⌴Ø.438 ↧.250**.
- Height **6.000**, slot width **2.600** in the front view.
- Lug width **2.500**, **Ø1.750** boss, **Ø1.250** cross hole, **8x Ø.266 THRU**, and angles
  **43.0°** and **120.0°** in the right view.
Steps:
1. D8.1 Open the public document and **Make a copy**.
2. D8.2 **"+" → Create Drawing…**.
3. D8.3 Pick **ANSI_A_INCH.dwt** (Onshape filter, ANSI tab), keep **No views**, click **OK**.
4. D8.4 In Insert view, expand the Part Studio and pick the **part** Universal Joint Flange.
   Orientation **Front**, scale **1:2**. Click to place it. The tool switches to Projected view;
   leave it on.
5. D8.5 Project **Top** (above), **Right** (to the right) and **Isometric** (top-right) from the
   front view, then turn off the tool. On the isometric view: View properties… → Scale **1:4** →
   ✓, then **Show shaded view**. On the three orthographic views: **Show hidden lines**, and
   **Tangent edges → Phantom**. The title block should now say "Universal Joint Flange / Made by
   Onshape"; if not, a Part Studio was picked instead of the part.
6. D8.6 Right-click the drawing tab → **Rename…** → "Universal Joint Flange Drawing", then Enter.
7. D8.7 Add **centermarks** (top view center, top-view hole centers, right-view hole centers),
   **centerlines** (edge to edge on the counterbored holes in the front view, extended after
   placing), and a **circle centerline** through the bolt circle in the right view
   (`ex1-step7.png`).
8. D8.8 **Hole callout** on a top-view hole and a right-view hole. Edit each one to add the prefix
   **4x** or **8x**.
9. D8.9 Dimensions as in `ex1-step9.png`: Ø4.750 with the **Diameter** tool, then 3.282, 2.061,
   6.000, 2.600, 2.500, 43.0°, 120.0°, Ø1.750 and Ø1.250. They show with 3 decimals in inches
   (template precision `0.123`), and angles with 1 decimal.
- **Check**: no quiz. Compare with the public "Completed Exercise: Universal Joint Drawing".

## 3. Notes and tables

### D9 Notes (video, 2:10)
- D9.1 **Note** (**N**, or the toolbar icon, which shows an "A"): click empty sheet space for a note
  **without a leader**, or click an edge or point in a view for a note **with a leader** attached
  to it. Type the text and accept.
- D9.2 More leaders: right-click the note → **Add leader**, then pick in a view. A note can have
  several leaders.
- D9.3 **Note toolbar** (rich text): bold, italic, underline, strikethrough, alignment, bulleted and
  numbered lists, a **ruler** with double-arrow handles for the wrap width, and a **symbol**
  dropdown (Ø, °, ±, ⌴, ↧ and others).
- D9.4 **Parametric notes** use two insert buttons:
  - **Insert sheet reference property** reads a property of the sheet's referenced part or assembly,
    such as Name, Description or Part number.
  - **Insert drawing property** reads a property of the drawing itself, such as sheet scale or
    drawing name.
  You can choose the text format, and for dates the date format. The note updates when the
  property changes. An undefined property shows **dashes** until it's set on the part or assembly.
- D9.5 Once placed, a note can be **rotated** with its rotation handle, **resized** with its other
  handles, and edited by **double-clicking** it.

### D10 Tables (video, 1:33)
- D10.1 **Table** tool: a dialog asks for **rows**, **columns**, whether there's a **Title row** and
  a **Header row**, and the **fixed corner** (the corner that stays put while the table grows or
  shrinks). Click the sheet to place it, then accept.
- D10.2 After placing, typing goes straight into the first cell. **Tab** moves to the next cell and
  **Shift+Tab** to the previous one. **Double-click** a cell to edit it in the note editor (text,
  symbols, properties).
- D10.3 A **single click** on a cell opens the **Cell toolbar**: insert or remove rows and columns,
  **Merge cells** and **Unmerge cell**, and text formatting. **Shift+click** selects several
  cells.
- D10.4 Hovering the table shows grips. Dragging a **midpoint grip** resizes it from the fixed
  corner, which is drawn **black**. Dragging a **corner grip** moves it. Right-click → **Table
  properties…** changes the fixed corner.

### D11 BOMs and callouts (video)
- D11.1 **Insert BOM** picks an assembly (it defaults to the one in the view). The drawing table is
  the **assembly's BOM (A20) as it's set up there**: the same columns and order. So set the BOM up
  in the assembly first.
- D11.2 The **Insert BOM** dialog (`ex2-step8.png`) has:
  - The assembly.
  - **BOM type**: **Flattened** (parts only), **Structured – Top level** (top-level items, with
    subassemblies as rows), or **Structured – Multi level** (subassemblies and their parts,
    indented).
  - **Order**: **Top to bottom** (header at the top, counting down) or **Bottom to top** (header at
    the bottom, counting up; the usual style above a title block).
  - **Select fixed corner**: four icon buttons.
  Click the sheet to place the table; it snaps to the border corner.
- D11.3 Right-click → **BOM Table properties…** changes these options later. The table can also be
  resized, and its cells formatted, like a normal table (D10).
- D11.4 **Callout** (balloon) tool dialog (`ex2-step10.png`):
  - **Border shape** (circle, underline, and others) and **size** (e.g. "Tight Fit").
  - **Text height** (e.g. 0.1200).
  - **Five text fields**: center, top, bottom, left and right. The center text goes inside the
    balloon, and the others are drawn around it. Each field takes typed text and properties, either
    of the **component** (e.g. `Part: Name`) or of the **BOM table** (e.g. `Table: Qty.`, `Table:
    Item No.`).
  Then click edges or vertices in the view to place callouts one by one, and accept with ✓.
  Callouts work without a BOM table on the sheet; only the component properties are available then.
- D11.5 Right-click a callout → **Edit…** changes its format or properties.
- D11.6 Callouts snap to **inference lines** while being dragged, which lines them up with each
  other.

### D12 Exercise: Universal Joint Assembly Drawing (`ex2-*`, target `ex2-drawing.png`)
Goals: an assembly drawing, a BOM table, callouts. The starting document is **public** ("Exercise:
Universal Joint Assembly Drawing"). It has Part Studio tabs "Universal Joint" and "Universal Joint
Components", the **Universal Joint Assembly** (23 instances), and a "Universal Joint Axle
Subassembly". Only the result is shown, so it **needs the starting document**. The equivalent in
cadrs would be any assembly with 5 distinct parts. The target BOM (flattened, 5 rows):

| Item No. | Name | Quantity | Part number | Description |
|---|---|---|---|---|
| 1 | Universal Joint Flange | 2 | MSB-0004 | Universal joint flange |
| 2 | Universal Joint Centre Block | 1 | MSB-0001xC | Universal Joint Center Block |
| 3 | Graphite Phosphor Bronze Bushes | 4 | MSB-0002 | Bushes for main bearing |
| 4 | Universal Joint Axle | 4 | MSB-0003 | Central Axle |
| 5 | Pan head machine screw 1/4-28 x 0.75 | 16 | STD-03923 | Pan head machine screw 1/4-28 x 0.75 Stainless Steel |

Steps:
1. D12.1 Open the public document and **Make a copy**.
2. D12.2 In the **Universal Joint Assembly** tab, open the **BOM table** panel (right edge,
   `ex2-step2.png`). **Add column → Name**. The Add column list shows Appearance, Name, Revision,
   State, Vendor, Project, Product line, Material, Title 1–3, Not revision managed, Unit of
   measure, Category and Tessellation quality. Then right-click the new header → **Move left**
   until it's next to Item No. (A20.6).
3. D12.3 **"+" → Create Drawing…**.
4. D12.4 **ANSI_A_INCH.dwt**, then **OK**.
5. D12.5 Insert view → **Assemblies** tab → **Universal Joint Assembly**, **Isometric**, **1:2**.
   Place it, then turn off Projected view.
6. D12.6 Rename the tab to "Universal Joint Assembly Drawing".
7. D12.7 Right-click the view → **Show shaded view**, then **Tangent edges → Phantom**.
8. D12.8 **Insert BOM**: **Flattened**, **Top to bottom**, fixed corner **top right**. Click the
   top-right corner of the border.
9. D12.9 Drag the table's **left midpoint grip** so it fits on the sheet, then adjust the column
   widths.
10. D12.10 **Callout** tool: border **Underline**, center field **Part: Name**, right field
    `x` + **Table: Qty.**. Click each of the 5 parts, then ✓. The callouts read "Universal Joint
    Flange x 2", "… Axle x 4", "Pan head machine screw 1/4-28 x 0.75 x 16", and so on.
11. D12.11 Drag the callouts into line using the inference lines.
- **Check**: no quiz. Compare with the public "Completed Exercise: Universal Joint Assembly
  Drawing".

## 4. Updating a drawing

### D13 Updating a drawing (video, 1:03)
- D13.1 New views reference the **workspace** (the live state) by default. They can reference a
  **version** instead; that's covered in another course.
- D13.2 Drawings **don't update automatically**. When the referenced part or assembly changes, the
  **Update from this workspace** icon turns **gold**. Clicking it, or **Ctrl+Q**, updates every
  view, annotation and BOM that references the workspace. The idea is that the detailer and the
  engineer can work in parallel.
- D13.3 After an update, annotations whose geometry is gone **dangle** and are drawn **red**. You
  can drag a red dimension's grip onto new geometry to re-attach it (the value updates), or delete
  it.

### D14 Exercise: Hand Brake Update (`ex3-*`, overview `ex3-drawing.png`, result `ex3-completed.png`)
(The course spells it "Hand Break".) Goals: change a part and an assembly, update the drawing, move
between sheets, fix dangling annotations. The starting document is **public** ("Exercise: Hand
Brake Update"). It has a **Hydraulic Brake Unit** assembly (14 instances, 12 mates, standard
content fasteners), a "Master Cylinder" folder, an "Enclosures" studio, a **Handle** Part Studio
(16 features: Main Sketch, Extrude 1, Fillet 1, Sketch 2, Extrude 2, Sketch 3, Extrude 3, …,
Hole 1), and a finished **Hand Break Drawing** tab. That tab has **3 sheets** (Sheets(3)):
- **Assembly**: the Hydraulic Brake Unit, isometric and shaded, with a 10-row BOM and ballooned
  callouts 1–10.
- **Handle**: the Handle Plate, Front + Top + Isometric, dimensioned in mm (2 decimals).
- **Grip**: the Handle Grip, Front + Top + Isometric.
It **needs the starting document**. Target BOM (flattened, Item No. / Name / Quantity):
1 Master Cylinder 1; 2 Handle Grip 1; 3 Handle Plate 1; 4 Spacer 2; 5 Hex flange bolt small ISO
4162 1; 6 Hex thin nut grade A & B ISO 4035 1; 7 Plain washer normal grade A ISO 7089 2; 8 Hex
socket head cap screw ISO 4762 3; 9 Enclosure Lower 1; 10 Enclosure Upper 1.
Steps:
1. D14.1 Open the public document and **Make a copy**.
2. D14.2 Open the **Hand Break Drawing** tab. Open the Sheets flyout and **double-click** each of
   the 3 sheets to look at it.
3. D14.3 **Handle** Part Studio → right-click **Main Sketch** → **Edit…**.
4. D14.4 Change these dimensions (`ex3-step4.png`): 225 → **250**, 100 → **125**, bar height → **25**,
   left offset → **25**, Ø8.25 → **Ø15**. **Delete** the small circle next to the Ø25.4 hole.
   Accept the sketch. Unchanged values: Ø5.5, Ø25.4, 29, 33°, 50, 46, R16, 36°.
5. D14.5 Edit **Extrude 2** (the grip bar): **Second end position** depth → **175 mm** (the first
   direction is Blind 10 mm).
6. D14.6 Edit **Hole 1**: Counterbore, Through, Standard ISO, Hole type Clearance, Size **M6** (it
   was smaller). That gives 6.6 mm, ⌴11.25 mm, depth 6 mm. The dialog also shows Start from sketch
   plane, 3× "Vertex of Sketch 4", and Merge scope "Handle Grip".
7. D14.7 In the **Hydraulic Brake Unit** assembly, select the 3 **Hex socket head cap screw ISO
   4762** instances. Right-click → **Edit standard content instance…** → Size **M6** → **Update**
   → ✓ (A19.9).
8. D14.8 Go back to the drawing. The views haven't changed yet, and the Update icon is **gold**.
   Click it: the assembly view and BOM update.
9. D14.9 **Handle** sheet: the views and dimensions have updated (250.00, 25.00, R30.00, Ø25.40,
   78.00, R16.00, 3x Ø5.50, 8.00). Delete the **dangling (red) centerline** and the dangling
   dimension `2x Ø8.25` that pointed at the deleted hole.
10. D14.10 Drag the end grip of the red **75.00** dimension onto the center of the large hole. It
    re-attaches and reads **46.00**. Delete the dangling **centermark**, then tidy the views and
    dimensions so everything fits on the sheet.
11. D14.11 **Grip** sheet: check the updated views (Ø25.00, **185.00** length, 35.00, 62.50, 8.00,
    8.50, callout **3x Ø6.60 THRU ⌴Ø11.25 ↧6.00**) and tidy them.
- **Check**: no quiz. Compare with the public "Completed Exercise: Hand Brake Update".

## Self-checks and survey
- **Self-Check: Drawing Interface** (D3): a 6-item hotspot quiz. Not started.
- **Self-Check: Introduction to Drawings**: a knowledge quiz ("Begin Self-Check"). Not started, so
  its questions aren't captured. Visiting the intro page marked it as viewed in the course outline,
  but no attempt was made.
- **Completion Survey** (Completion Certificate section): not opened.
- The exercises have **no numeric self-check**. Each one compares against a public "Completed
  Exercise" document. For cadrs, a scenario can check the structure instead: the number of views
  and their types, the view scales, the BOM rows, the callout texts, the dimension values, and the
  absence of dangling annotations.

## Cross-cutting requirements found in the course
- X1 **Drawing tab** as a document element next to Part Studios and Assemblies, created with
  "+" → Create Drawing… (or from an instance context menu). It supports Rename, Duplicate,
  Properties, Export… and Move to document (D1.2, D2.10).
- X2 **Templates**: built-in ANSI and ISO templates (A–E and A4–A0, landscape or portrait, INCH or
  MM variants), custom templates with first- or third-angle projection, a template picker with
  All/ANSI/ISO filters and source libraries, and a "Four views / No views" option (D1.3–D1.6). A
  template sets the sheet size, border and zones, title block, projection, units and precision.
- X3 **Parametric title block and notes**: fields bound to drawing properties (scale, projection,
  sheet n of m, drawing name) and to the referenced part's or assembly's properties (name,
  description, part number, drawn and approved by and date), with dashes for empty values
  (D1.9, D9.4). This depends on a **properties model** for parts and assemblies (see A20.10 for
  editing properties from the BOM).
- X4 **2D sheet viewport**: pan and zoom only, **F** to fit, and multiple sheets per drawing with a
  **Sheets flyout** (**Ctrl+S**) tree of sheet → reference → views → projected children, plus
  Insert sheet and double-click to activate (D2.2–D2.7).
- X5 **Three-level properties**: Drawing (units and precision, dual units, zeros, dimension,
  annotation, view, construction, format and table styles; lock; update from template), Sheet
  (scale, size, border and zones, referenced object) and View (reference, scale, sheet)
  (D2.5–D2.9).
- X6 **View generation from the B-rep**: orthographic and isometric projection at a scale, with
  **hidden-line removal** (hidden lines shown dashed or not at all), **tangent-edge modes** (hidden,
  solid, phantom), a **shaded** mode using part appearances, thread display, and part
  intersections. Views must regenerate when the model changes (D4, D13).
- X7 **View placement**: a base view, then **projected views** from it with alignment constraints
  that follow first- or third-angle projection, an **auxiliary** view from an edge,
  suppress-alignment, move to sheet (children stay), align a view vertical or horizontal to an
  edge, and show/hide model sketches (D4.4–D4.6, D7).
- X8 **Drawing annotations tied to view geometry**: centerline (point-to-point, edge-to-edge,
  circle through 2 or 3 points), centermark, virtual sharp, driven dimensions (smart **D**, radial
  **Shift+R**, diameter **Shift+D**, and others) with orange snap points, a per-dimension palette
  (tolerance, precision, text, dual units), and **hole callouts** built from Hole-feature data
  with an editable prefix. Every annotation keeps a **reference to model topology** so it survives
  updates or shows up as **dangling (red)**, and it can be re-attached by dragging a grip
  (D5, D6, D13.3). This needs **persistent topology naming** between the model and the drawing.
- X9 **Rich-text notes** with leaders (more than one allowed), symbols and property fields; and
  **tables** with title and header rows, a fixed corner, cell navigation (Tab / Shift+Tab), merge
  and unmerge, and grips for resizing and moving (D9, D10).
- X10 **Drawing BOM table** that mirrors the assembly BOM's columns (Flattened, Structured top
  level, Structured multi level; top-to-bottom or bottom-to-top; fixed corner), with **callouts**
  (balloons) that have 5 text fields bound to component or BOM-row properties, several border
  shapes, and inference-line alignment while dragging (D11). The Item No. in a callout must match
  the BOM row.
- X11 **Update model**: views reference the workspace (or a version). A gold "out of date"
  indicator appears when the model changes, and an explicit **Update** (**Ctrl+Q**) refreshes the
  drawing; nothing updates silently (D13). For cadrs, the drawing's dependency on its source
  models has to be tracked and diffed.
- X12 **Shortcut summary**: **F** fit, **Ctrl+S** sheets flyout, **D** dimension, **Shift+R**
  radial, **Shift+D** diameter, **N** note, **Ctrl+Q** update, **Esc** ends a tool, **Tab** /
  **Shift+Tab** move between table cells. Note that **N** means "view normal" in a Part Studio
  (S1.5) but Note in a drawing, and **D** is Dimension in both sketches and drawings. Shortcuts
  depend on the tab type (see A X10).
- X13 **Import and export** (only named in the course): insert **DXF/DWG** into a drawing, insert
  images, and **Export…** from the tab menu. The course doesn't list export formats. Onshape's
  drawing export offers PDF, DWG, DXF, DWT and image formats, which matches P X7 (DXF/DWG).
- X14 Shown but not taught: **section, detail, broken-out, break and crop views**; **GD&T**
  (feature control frames, datums); **weld symbols**; **surface finish** symbols; ordinate,
  baseline and chamfer dimensions; sketch lines and splines on the sheet; drawing revision tables;
  and referencing a version instead of the workspace. They're toolbar entries only, to be taken
  from a later drawing course.
