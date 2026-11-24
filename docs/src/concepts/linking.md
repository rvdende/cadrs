# Using one document in another

Within a document, an assembly or a drawing follows its Part Studios as they change. Between
documents it works differently: one document uses another **at a version**. That way a change in
a shared part library doesn't alter every assembly that uses it until each one chooses to
update.

## Inserting from another document

In an assembly, **Insert** has an **Other documents** browser: search for a document by name, or
look under My documents, Recently opened, Created by me and your folders. Pick a document to see
its tabs at its newest version, or open the version graph to choose an older one. A document
without versions offers **Create version** first, since there is nothing to link to until it has
one.

Drawings can show views of other documents the same way.

## Keeping up to date

When a document you use gets a newer version, a blue badge appears on the linked instances and on
the tabs that use it. The **Reference manager** shows what can be updated:

- **Update to latest** updates every reference to the newest version of each document.
  If a document in between also uses an out-of-date one (A uses B, which uses C), cadrs updates B
  first and makes a version of it, so A can use that.
- **Selective update** lists each reference, so you can choose which to update and to which
  version.

An updated instance keeps its position and its mates. A reference can be **pinned** to its
version, so that updating all leaves it alone.

To see where a document is used, right-click it on the documents page and choose **Used in…**.

## Moving tabs to another document

A tab's **Move to document…** moves it, and the tabs it depends on, into a new or an existing
document. What stays behind then uses the moved tabs at a version of the other document, so the
assemblies and drawings that used them still look the same.

## Copying tabs

A tab's menu also has **Copy**. **Paste** in another document's tab bar (or in another running
cadrs) adds a copy of the tab with what it depends on. A copy isn't linked: after pasting, the
two go their own ways.
