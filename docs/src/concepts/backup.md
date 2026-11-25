# Backing up and moving to another computer

Everything cadrs keeps is in one folder on your computer, the *documents folder*. A backup is a
copy of that folder, and moving to another computer is copying it there.

## Where it is

| System | Folder |
| --- | --- |
| Windows | `%APPDATA%\cadrs\documents\` |
| macOS | `~/Library/Application Support/cadrs/documents/` |
| Linux | `~/.local/share/cadrs/documents/` |

The documents page's app menu has **Open documents folder**, which opens it.

## What's in it

- **One folder per document**, named by the document's ID. Each holds the document, its whole
  history with its versions, the files imported into it (STEP files, pictures), and its
  thumbnails.
- `folders.ron`: the folders and labels of the documents page.
- `preferences.ron`: your [preferences](../reference/preferences.md).
- `identity.key` and `identities.ron`: who you are to the people you share documents with. Your
  cadrs ID is made from the key, and `identities.ron` holds the user ID your documents record as
  their owner. Keep the key private, as you would a password.

The files are plain text, apart from the pictures and imported files, and are the same on every
system, so a folder from Windows works on Linux or macOS.

## Backing up

1. Close cadrs, so that every change is written and each document's history is tidied up.
2. Copy the whole documents folder somewhere safe: another disk, a backup service, or a zip
   file.

Any backup tool that copies folders works. A synced folder (Dropbox, OneDrive and the like) is
fine for a copy, but don't let cadrs on two computers use the same synced folder at the same
time.

To restore, close cadrs and put the folder back in place.

## Moving to another computer

1. Install cadrs on the new computer and start it once, then close it.
2. Close cadrs on the old computer, and copy its documents folder over the new computer's.
3. Start cadrs on the new computer. Your documents, folders, labels, preferences and cadrs ID are
   all there.

Because the cadrs ID moves with the folder, the people you share documents with see the same you
on the new computer, and the documents you own stay yours. Avoid running cadrs on both computers
with the same ID at the same time.

## Keeping it somewhere else

cadrs can use a documents folder in another place, such as a second disk or a network drive.
There are two ways to tell it where, read each time cadrs starts:

- **The `CADRS_DATA_DIR` environment variable**, set to the folder. It applies however cadrs is
  started. On Windows, set it under Start › *Edit environment variables for your account*, for
  example to `D:\CAD\cadrs` or `\\nas\cad\cadrs`.
- **The `--data-dir` option**, for example in a shortcut: `cadrs --data-dir D:\CAD\cadrs`. It
  applies only when cadrs is started that way, and wins over the variable.

To move your documents there, close cadrs, copy the contents of the old documents folder into the
new one, and start cadrs with the new place set.

A network drive works, with some things to keep in mind:

- cadrs saves each change a moment after you make it, so a slow connection makes saving and
  opening large documents slower.
- If the drive isn't available when cadrs starts, your documents aren't either.
- Don't let two computers use the same folder at the same time.
- The folder holds your identity key, so keep the share private to you.

A safer pattern is often to keep the documents folder on your computer and back it up to the
network drive regularly, for example with your system's backup tool.

## Single documents

To give one document to someone, or to move only a few, use `.cadrs` files instead: the app
menu's **Export** in a document, and **Import** on the documents page. A `.cadrs` file holds the
document as it is now, without its versions or history. See
[Importing and exporting](import-export.md).

## The browser version

In a browser, cadrs keeps its documents in that browser's own storage, not in a folder you can
copy. Clearing the browser's site data deletes them. To keep them, or to move them to the
desktop app, export each document as a `.cadrs` file and import it there.
