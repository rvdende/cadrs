# Reporting a problem

If cadrs closes unexpectedly or does something wrong, a report on
[GitHub](https://github.com/rvdende/cadrs/issues) helps us fix it, most of all with the files
that show what happened.

## After a crash

The next time cadrs starts, it says **cadrs closed unexpectedly** and shows the error, if there
was one. From there:

- **Report on GitHub** opens a new issue with the version, your system and the error filled in,
  and the folder with the files. Describe what you were doing, then drag the files into the
  issue.
- **Show files** opens that folder: `report.txt` (the version, your system and graphics card,
  the document and tab that were open, the error and the end of the log) and the session's log.
- **Copy report** copies `report.txt`, to paste anywhere.
- **Add document** puts a copy of the document that was open in the folder (a `.cadrs` file),
  so we can open it and do what you did.

Nothing is sent anywhere unless you choose to. The files may hold the names of your documents
and folders: look them over before you attach them.

## Any other problem

The app menu's **Report a problem…** opens the same choices for the session that is running:
its log so far, and the document that is open.

## Where the files are

In the [documents folder](../concepts/backup.md):

- `logs/`: the log of each of the last ten times cadrs ran, what it would print in a terminal.
- `crashes/`: a folder for each crash, and each **Report a problem…**, with the files above.
