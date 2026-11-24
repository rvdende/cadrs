# Importing and exporting

Documents live in cadrs's library, so files are for going in and out of it: to and from another
computer, another person, or another program.

## `.cadrs` files

A `.cadrs` file is a cadrs document packed into one file, with everything needed to edit it:
its tabs, their features and the files they use. The app menu's **Export** in a document writes
the whole document as a `.cadrs` file. It holds the document as it is now, without its versions
or history.

Importing a `.cadrs` file on the documents page makes a new, editable document in your library.
Imported into an open document (the tab bar's **+ › Import…**), its tabs are added to that
document. A single tab can be exported as a `.cadrs` file too, from a part's **Export…** dialog.

## Files from other CAD programs

| Format | Comes in as |
| --- | --- |
| STEP, IGES, BREP | parts, with names; a STEP assembly can keep its structure as an Assembly tab |
| STL | parts made from the mesh, one per connected piece |
| SVG, EPS, PDF, DXF, DWG | sketch curves, dropped on a Part Studio or an open sketch |
| PNG, JPEG, GIF | a picture in a Part Studio, to trace |

Where it goes depends on where you bring it in:

- **On the documents page** (**Import**, or drop the file on the window): a new document.
- **In a document** (**+ › Import…** in the tab bar, or drop it on an Assembly or Drawing
  tab): new tabs.
- **In a Part Studio** (drop it on the window): an **Import** feature in the feature list, which
  can be edited, and built on, like any other.

For a STEP assembly, the import asks whether to put every part where the file places it in one
Part Studio, or to keep the assembly structure: each distinct part once, and an Assembly tab with
an instance at each place. Files from programs where Y points up can be turned so that Z is up.

## Exporting parts

Right-click a part, or several, and choose **Export…**:

| Format | For |
| --- | --- |
| STEP (AP214) | other CAD programs: exact solids with part names |
| IGES | older CAD programs: exact solids |
| STL, OBJ | meshes, for 3D printing and other tools |
| glTF (GLB) | meshes with colours, for Blender, games and the web |
| DXF, DWG | flat shapes at full size, in millimetres, for cutting machines |
| BREP | OpenCASCADE programs (FreeCAD, CadQuery): exact geometry only |
| cadrs | the tab with its features, to import into another document |

DWG needs LibreDWG or the ODA File Converter installed. Drawings export from their own tab.
