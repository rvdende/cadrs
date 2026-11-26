# cadrs

cadrs is an open-source parametric CAD program written in Rust, with its own geometry kernel. It
runs on Linux, Windows and macOS, and the same app runs in a browser at
[cadrs.net](https://cadrs.net/). People can edit a document together, peer to peer, without a
server in between.

Its way of working follows Onshape's. If you come from a file-based CAD program such as
SolidWorks, Inventor, Fusion or IronCAD, a few things work differently:

- **There are no part and assembly files.** A *document* holds Part Studios, Assemblies, Drawings
  and Variable Studios as tabs, and several parts can be designed together in one Part Studio.
  See [Documents and tabs](concepts/documents.md).
- **There is no Save.** Every change is saved as you make it, and kept in the document's history.
  You can go back to any earlier state, and name the ones that matter as *versions*. See
  [History, versions and workspaces](concepts/history.md).
- **Documents live in a library, not in folders on your disk.** You organize them on the
  documents page with folders and labels. A `.cadrs` file is how a document travels to another
  computer: you import it into the library, and export it from there. See
  [The documents page](concepts/documents-page.md) and
  [Importing and exporting](concepts/import-export.md).
- **One document uses another at a version.** An assembly can insert parts from another
  document; it uses a version of that document, and you choose when to update it. See
  [Using one document in another](concepts/linking.md).

The [keyboard shortcuts](reference/keyboard-shortcuts.md) and
[preferences](reference/preferences.md) pages are made from cadrs's code, so they always match
the app they are built with.

cadrs is young and changes quickly. These pages describe how it is organized rather than every
button. If something here doesn't match what you see, please
[open an issue](https://github.com/rvdende/cadrs/issues), or ask on
[Discord](https://discord.gg/AmmqETBTKg).
