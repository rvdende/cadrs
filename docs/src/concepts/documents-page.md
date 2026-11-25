# The documents page

The documents page is cadrs's library: every document you have, wherever it came from. It's what
you see when cadrs starts, and the app menu's first item in a document brings you back to it.

## Finding documents

The list shows each document's name, when it was last changed and by whom, and who owns it.
Click a column to sort by it, use the search box, or switch between the list and a grid of
thumbnails. Double-click a document, or select it and press <kbd>Enter</kbd>, to open it.

Right-click a document for **Rename…**, **Labels**, **Properties…** (its description and
labels), **History…**, **Used in…** (the documents that use it) and **Move to trash**.

## Folders and labels

There are two ways to organize documents, and they work together:

- **Folders** hold documents and other folders, like folders on a disk. **Folder** in the command
  strip creates one. Drag documents onto a folder, or onto a folder in the path above the list, to
  move them. <kbd>Ctrl</kbd>+click and <kbd>Shift</kbd>+click select several at once.
- **Labels** are coloured tags. A document can have any number of them, so one document can be
  both "Customer A" and "3D printed". **Label** in the command strip creates and assigns them,
  and the app menu's **Labels** shows the documents with a label.

A document is in one folder, but can have many labels.

## The trash

**Move to trash** (or <kbd>Delete</kbd>) moves documents and folders to the trash, which the app
menu opens. From there they can be restored, or deleted for good. Renaming, moving and trashing
can all be undone with <kbd>Ctrl</kbd>+<kbd>Z</kbd>.

## Other ways in

- **New document ▾** also has **From a sample…** (a copy of a bundled sample) and **Join shared
  document…** (a document someone shared with you).
- **Import** brings a `.cadrs`, STEP, IGES or STL file in as a new document. Dropping the file on
  the window does the same. See [Importing and exporting](import-export.md).
- The **Public documents** panel lists documents that people using cadrs have published. You
  can open them, star them, and fork a copy to edit. See
  [Public documents](../under-the-hood/public-documents.md).
