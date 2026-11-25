# Documents and tabs

In a file-based CAD program each part, assembly and drawing is a file of its own, and an assembly
refers to its part files by name. In cadrs, as in Onshape, a **document** is the unit you open,
share and keep history for, and everything in a project can live in one document.

## Tabs

A document is a set of **tabs**, shown along the bottom of the window. **+** in the tab bar
creates a new one:

- **Part Studio:** sketches and features that make parts. One Part Studio can hold several parts,
  so parts that depend on each other (a lid that fits its box) are designed together, from the
  same sketches.
- **Assembly:** instances of parts, from this document's Part Studios or from other documents,
  positioned with mates. An assembly can contain other assemblies.
- **Drawing:** sheets with views of a Part Studio or an Assembly, dimensions, notes and tables.
- **Variable Studio:** variables that other tabs can use in their dimensions.

Double-click a tab to rename it. Right-click it to duplicate, delete, copy it, or move it to
another document. **+ › Create folder** groups tabs in folders, and tabs can be dragged into
them.

## A Part Studio's features

A Part Studio is built from its **feature list**, on the left: each sketch, extrude, fillet and so
on, in order. Editing a feature rebuilds everything after it. The **rollback bar** at the end of
the list can be dragged up to see the Part Studio as it was at an earlier feature, and to insert
new features there.

## No Save

cadrs saves every change a moment after you make it. There is no Save command and nothing to
lose when the app closes. Undo and redo (<kbd>Ctrl</kbd>+<kbd>Z</kbd> and
<kbd>Ctrl</kbd>+<kbd>Y</kbd>) step through your recent changes, and the document's
[history](history.md) keeps every change, so you can go back further than undo reaches.

## How many documents?

There's no fixed rule. Some ways to divide work:

- **One document per product**, with a Part Studio per group of related parts, an assembly per
  subassembly, and the drawings. References inside a document follow the latest changes
  automatically.
- **A document of shared parts** (fasteners, a motor, a standard bracket), used by other
  documents [at a version](linking.md), so a change there doesn't alter them until they update.

A tab's menu has **Move to document…**, so a design that outgrows its document can be split
later, with its references kept.
