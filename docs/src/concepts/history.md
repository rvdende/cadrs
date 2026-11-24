# History, versions and workspaces

File-based CAD keeps the last saved state of each file, and older states only if you copy the
file. cadrs keeps every change of a document in its **history**, so nothing is lost by changing
it, and names the states that matter as **versions**.

## The history

**History** on the left rail opens the History panel. It lists the document's changes, newest
first, grouped by who made them, down to **Start**, when the history began. Right-click an entry
to:

- **Restore** the document to that state. Restoring is a change like any other: it is added to
  the history, and can be undone.
- **View in repair**, to look at the Part Studio as it was, without changing anything.

When a document is closed, cadrs tidies its history and keeps the versions and the latest state.

## Versions

A **version** is a named state of the document that never changes afterwards: "Sent for
quote", "Rev B", "As printed". The **Create version** icon in the top bar (or the app menu's
**Create version…**) names the current state, with an optional description.

Versions show as squares in the History panel. Right-click one to open it read-only or to
restore it. Versions are also what other documents use when they insert parts from this one; see
[Using one document in another](linking.md).

There is no need to make a version for every change, since the history has them all. Make one
when a state matters: something was sent, made, or released, or another document should use it.

## Workspaces and branches

The document you edit is its **Main** workspace. To try an idea without disturbing Main, make a
**branch**: right-click a version in the History panel and choose **Branch to create
workspace…**. The branch starts as an exact copy of that version and has its own history and its
own undo.

While a branch is open, its name shows next to the document's name; click it to switch
workspaces. The app menu and the History panel list the workspaces too. To bring a branch's work back, switch to the workspace that should receive it
(usually Main), right-click the branch in the History panel and choose **Merge into current
workspace…**. The merge lists the tabs the branch changed; for each, choose **Replace** (take the
branch's) or **Keep** (keep the current one). A merge is one step in the history, and can be
undone.

When a document is shared, Main is what everyone edits together. A branch is your own until you
merge it.
