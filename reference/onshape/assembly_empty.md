# Assembly tab, empty state (web-sourced)

Sources: help pages "Assemblies", "Instances List", and "Insert Parts and Assemblies" (last
updated 2026-09-22). Images are in `assembly_empty/`. **No public screenshot of a truly empty
assembly was found**. The layout below combines populated screenshots with the help text: "When a
new Document is created, an empty Part Studio and Assembly tab exists within it … Add instances to
the assembly with Insert." Anything marked (inferred) should be checked against a live capture
later.

## Layout (from `assemblyelementui-01.png`, 906×457 crop)
The shell is the same as the Part Studio (top bar, left icon rail, bottom tab bar, view cube top
right, right-edge panel strip). The differences:
- **Toolbar** (`Assemblytoolbar-scrnshot.png`, one row of about 32 px), left to right:
  - Undo, redo, then a dimmed round icon.
  - **"⎘ Insert"**, the only text button (like "Sketch" in the Part Studio), shortcut **I**.
  - Then: Named positions/Replicate, then mates (Fastened, Revolute, Slider, Planar, Cylindrical,
    Pin slot, Ball, Parallel, Tangent), Mate connector, Group, Relations, Linear pattern, Circular
    pattern, Explode, Replicate, and more.
  - "Search tools… alt c" on the right.
  - For cadrs, show Insert plus the mate icons disabled (M-later).
- **Instances list** (the left panel, replacing the feature list; `assemblyfeaturelist2-01.png`):
  - Top row: a filter funnel icon, a "Filter by name" input, and a list-view toggle button (blue
    when active).
  - "**Instances (N)**" bold header with a "new folder" icon on the right.
  - The **assembly root** ("Assembly 1") with an assembly icon, and under it **Origin** (⊙ grey).
  - Instance rows: "Part 1 <1>" with a part icon. The instance number is in angle brackets.
  - "**Items (0)**", a collapsible section (newer UI).
  - "**Mate features (N)**", a collapsible section.
- **Empty state (inferred):**
  - The list shows "Instances (0)" → "Assembly 1" → "Origin", then "Items (0)" and "Mate features
    (0)".
  - The viewport shows only the assembly **origin triad** and **no default planes**. Assemblies
    have no Top/Front/Right planes, only the origin.
  - The background matches the Part Studio.
  - There is no big hint banner or placeholder illustration.
  - The only cue is the Insert button. Whether a first-use tooltip appears is unknown. For
    cadrs, a small centered grey hint such as "Insert parts to start (I)" is acceptable, since it
    is ours and not copied.
- Right-clicking the Assembly root offers Expand all / Collapse all and Create new subassembly.

## Insert dialog (`insert-command.png`, about 302×587 px, docked top left over the list)
- Title "Insert parts and assemblies", with a ✓ (pale green until something is inserted) and ✕.
- Tabs: **Current document** | Other documents | Standard content. The active tab has blue text
  and a blue underline.
- The document name, branch "Main", and two icon buttons.
- Sub-tabs: **Part Studios** | Assemblies.
- A "Search Part Studios" input.
- Filter icons: parts, surfaces, sketches, and on the right "insert Part Studio as rigid".
- A list of Part Studios: an expand chevron, a thumbnail of about 60 px, and the name. Expanding
  lists the individual parts.
- Footer: "↶ Undo to remove instances", "Inserted: 0", and "?".
- **Behavior:**
  - Clicking a Part Studio inserts **all** its parts. Clicking a part inserts that part.
  - With the cursor inside the dialog, the instance is placed with its Part Studio origin at the
    assembly origin.
  - If the cursor is moved into the viewport first, the next click places it there.
  - Repeated inserts of the same part are placed at a slight offset.
  - The dialog stays open for more inserts, and ✓ closes it.
  - Onshape remembers which top tab was used last.
- The first inserted instance is **not fixed** automatically. Fix is in the instance's context
  menu.
- Deleting an instance deletes its mates and mate connectors.

## Assembly tab context menu
See `tab_menu.md`. The assembly variant (`assembly-context-menu-01.png`) adds Release…, Release
all configurations…, and Revision history….
