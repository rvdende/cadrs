# Document tabs: context menu, rename, insert, delete (web-sourced)

Sources: help pages "Document Tabs", "Assemblies" (context menu section) and "Part Studios" (all
last updated 2026-09-22). Images are in `tab_menu/` and `assembly_empty/assembly-context-menu-01.png`.
The live `screens/05e-bottom-tabs.png` covers the bar itself.

## Tab bar (bottom, about 26 px; see the live notes for its look)
- Left to right:
  - The **Tab manager** icon (magnifier over list).
  - **+** (Insert new tab).
  - Then the tabs. Each tab is a type icon, the name, and a fixed width of about 170 px, with a
    thin grey divider between tabs (`tab-bar-wmenu-01.png`).
- The active tab is white with a **blue underline**. Inactive tabs are light grey (#e8e8e8-ish).
- **Hovering** a tab shows a thumbnail preview of its contents after a short delay.
- Tabs can be **dragged to reorder**. The order is shared with every user of the workspace. **A
  new tab is inserted directly right of the active tab and becomes active.**
- Opening a document selects the previously active tab, or the first tab the first time.

## Tab context menu (right-click a tab)
From `tab-bar-wmenu-01.png` (Part Studio, 2024-ish UI) and `partstudiormbmenu.png` (from the tab
manager list). Group separators are shown as rules:

```
Delete                          (⊗ grey circle-x icon in newer UI)
─────────
Open in new browser tab
Rename…            (older/other image: "Rename")
Properties…
Show code          (Part Studio only)
─────────
Duplicate
Copy to clipboard
Create Drawing of <tab name>…
─────────
Select as document thumbnail
─────────
Move to document…
Export…
Release…           (Release all configurations…, Revision history… for assemblies)
Create task…
```

- The tab manager variant adds **Create… ▸** at the top and **Add selection to folder**.
- **Menu style:**
  - A white card with a shadow and about 4 px radius.
  - Rows are about 20–22 px, with a 13 px Inter-like dark-grey label and about 14 px left padding.
    There are no keyboard hints.
  - Thin light-grey separators.
  - The menu opens **above** the tab bar, anchored at the cursor.
- **Behaviors:**
  - **Delete** removes the tab even if it is active. **The last remaining tab cannot be deleted**:
    Delete is disabled or refused.
  - If other tabs reference the tab being deleted, a **"Delete tabs" modal** warns
    (`Delete-tab-dialog.png`):
    - A pale yellow alert (#fdf3dc-ish) with ⚠: "Deleting this tab will result in failed or
      missing features in 2 tabs".
    - A list of the affected tabs with their icons.
    - A blue **Delete** button (#1f5fad-ish) and a grey **Cancel** button.
    - Otherwise the tab is deleted **without confirmation**. For us, this should go through
      undo/redo.
  - **Rename…** turns the tab label into an inline text field with the name pre-selected. Enter
    commits and Esc cancels.
    - Uncertain: in current Onshape the rename may happen inline in the tab, or in a small
      dialog. The help only says "Rename the tab".
    - **Double-click to rename is not documented** in any help page or forum post found, so it
      could not be confirmed. The live app should be checked.
    - Recommendation for cadrs: support both double-click and the menu, both opening the same
      inline editor.
  - **Duplicate** inserts a copy named "<name> (1)"-style right of the original. The copy is not
    associative. The exact naming pattern is uncertain.
  - **Properties…** opens a modal with Name, Description and other metadata fields.
  - For cadrs: implement Rename, Duplicate, Delete and Properties (the last can be a
    name+description modal). Show the other items disabled or leave them out, following the
    `NOTES.md` guidance for the + menu.

## Insert new tab (+) menu (`insert-new-tab-menu.png`; opens upward from +)
- Applications ▸
- Create Material Library, Create Feature Studio, Create Render Studio, Create PCB Studio (beta
  pill), Create CAM Studio (beta pill)
- ─────────
- **Create Part Studio**, **Create Assembly**, Create Variable Studio, Create Drawing…, Create
  folder, Import…

Each row has an outline icon of about 18 px. New tabs are named "Part Studio N" or "Assembly N",
using the next free number.

## Tab manager (`tab_manager_open-01.png`)
- A left panel about 300 px wide that pushes the feature list to the right.
- Header "Tabs" with toggle icons: search (active: light-blue bg), sort ▾, filter, list view,
  detail view, and ✕.
- A "Search tabs" field, type-filter icons (Part Studio, Assembly) and a "Clear" button.
- Rows show a thumbnail of about 48 px, the name in bold and the type in grey italic
  ("Part Studio"). The active row has a light-blue fill (#b3dcf2-ish) and a dark-blue left bar.
- A large thumbnail preview of the selected tab sits at the bottom.
- This is not needed for M1–M9. Ctrl+Space cycles recent tabs.
