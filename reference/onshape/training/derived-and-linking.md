# Derived feature and linked documents: requirements for cadrs

**Source:**
- Onshape Help "Derived" (`cad.onshape.com/help/Content/PartStudio/derived.htm`), read on 2026-09-29.
- The cross-document parts of the course files: `essential-tips.md` T3.*, T7.*; `intro-to-assemblies.md` A2.3.

The text is paraphrased. This isn't a course, but the user asked for it on 2026-09-29, and it is
**in scope** for passing phase 3. Gap status is in [derived-and-linking-gaps.md](derived-and-linking-gaps.md).

IDs use the prefix `DV`.

## DV1 Linked-document references (shared by Derived and assembly Insert)
- DV1.1 **References to another document always pin a version.** Inside the same document, a
  reference can follow the **workspace** (live) or pin a version of it.
- DV1.2 **The stored reference is** `(document, version | workspace, element, entity)`, with the
  entity being a part, sketch, plane, curve or surface, mate connector, or the whole Part Studio
  or assembly. It's resolved read-only from the document store at that version and cached.
- DV1.3 **Linked-document icon.** An instance or feature that references another document shows
  it. The icon turns **blue / "update available"** when the source has a newer version.
- DV1.4 **Update.** The user updates a reference to the latest version, or picks a specific one.
  It's undoable, and nothing changes until the user updates. There is also an "update all"
  action for the tab or document.
- DV1.5 **Circular references are refused**, e.g. A derives from B and B derives from A, across
  tabs or documents, with a clear error.
- DV1.6 **Where used.** A document can list which other documents reference it and at which
  versions. This extends TD3.8 and the assemblies "where used".
- DV1.7 **Missing source** (deleted document, version or entity): the feature or instance is in an
  error state, and Replace and Repair apply (links to the Inspection and Repair course, IR*).
  Specific messages:
  - the source document is in the Trash: "Cannot open a document in the trash. Restore the
    document from Trash.";
  - the version or document is permanently gone: "Resource does not exist";
  - the source isn't accessible: "You cannot modify this feature because you cannot access the
    referenced document". The existing link keeps working read-only, but it can't be updated or
    edited.

*Added from Onshape Help "Linking documents"
(`cad.onshape.com/help/Content/Document/linking_documents.htm`), read on 2026-09-29:*
- DV1.8 **A version is required to link.** A document with no versions can't be linked from
  another document. Picking an unversioned document in the Other documents tab offers an
  **inline "Create version"** dialog. Once that finishes, the new version is selected.
- DV1.9 **Open linked document.** Right-clicking a linked instance (Instances list) or feature
  (Feature list) gives **Open linked document**. It opens the source document at the **linked
  version** (read-only), with the source Part Studio or assembly active and the referenced part
  selected.
- DV1.10 **Reference Manager.** Clicking the blue "update available" linked icon opens a
  **Reference Manager** dialog. It lists the document's external references grouped by source
  document, with the current version and the newest version. You pick a target version per
  reference (newest by default) and update one, several, or all at once. It's undoable.
- DV1.11 **Propagation rules, summarised:**
  - same-document references (assembly to Part Studio, Derived with a workspace reference) update
    instantly;
  - cross-document references only update when the user asks;
  - versions are immutable, so a pinned reference always shows the same geometry.
- DV1.12 **Sharing:** out of scope (cloud permissions). A local stand-in: documents that exist in
  the local library can be linked. A document that's been removed from the library behaves like a
  deleted one (DV1.7).

## DV2 Assembly Insert from other documents
- DV2.1 The Insert dialog's **Other documents** tab lists documents from the library, with search
  and filters. For each document you pick a **version** (the newest by default) and then an
  entity: a part, a whole Part Studio (rigid or not, as in the current-document tab), an assembly
  (inserted as a subassembly), a sketch, or a mate connector-bearing part. (A2.3)
- DV2.2 Inserted instances carry the pinned `ExternalRef`, show the linked icon in the instance
  list, and follow DV1.3–DV1.4.
- DV2.3 Mates, patterns and BOM work the same as for same-document instances. BOM rows keep the
  source part's properties from that version.

## DV3 Derived feature (Part Studio)
- DV3.1 **Derived** is a Part Studio feature that inserts **one-way associative** copies of
  entities from another Part Studio, in the same document or another one. Edits flow from the
  source to the derived copy, never back.
- DV3.2 **What can be derived:** parts, sketches, surfaces, curves, planes, mate connectors
  (explicit and implicit), and the whole Part Studio. Active sheet metal is out of scope (no sheet
  metal in cadrs), so it's treated as a static part.
- DV3.3 **Dialog, in order:**
  1. Source Part Studio picker, with tabs **Current document | Other documents**, search, and a
     version picker (workspace or a version for the same document; a version for other
     documents).
  2. Entity selection (all, or specific parts, sketches, planes, curves and mate connectors).
  3. **Locations:** mate connectors in the target Part Studio. Each location creates one copy;
     empty means one copy at the origin.
  4. **Placement:** *Base origin* (default: the source origin goes onto the location) or *Base
     mate connector* (a chosen mate connector of the derived entities goes onto the location).
  5. **Include mate connectors** (checkbox).
  6. **Include properties** (checkbox, on by default; when off, only the name, material and
     appearance are copied).
- DV3.4 **Update behaviour:**
  - Same-document workspace references update live when the source changes.
  - Version references, in the same or another document, show the update indicator (DV1.3) and
    update on demand (DV1.4).
- DV3.5 **Feature list:** one "Derived" entry that expands to show what it brought in (parts, mate
  connectors, sketches, planes) with eye toggles, plus the linked icon for other-document
  sources. Derived parts appear in the Parts list and can be used by later features (sketch on
  their faces, booleans, fillets). Derived sketches can be used for extrudes.
- DV3.6 **Edit / swap source:** double-click or right-click → Edit reopens the dialog. Changing the
  source rebuilds, and downstream features may fail and show the normal repair UI.
- DV3.7 **Rules:**
  - the same Part Studio (and configuration) can't be derived twice in one Part Studio;
  - one source Part Studio per Derived feature;
  - no circular chains (DV1.5);
  - the source's visibility setting is ignored, so derived parts are always shown;
  - many locations work but are slow, and the docs recommend an assembly for that.
- DV3.8 **Mass properties, STEP export and drawings** treat derived parts like native parts, and
  drawings can reference them.

## DV4 Move to document (from essential tips, T1.3)
- DV4.1 **Move a tab to another document.** The target gets a version. References from the source
  document to the moved tab are rewritten into pinned external references (DV1.2), and the
  assembly's world transforms are kept.

## Exercises / checks (ours)
- **ex-dv1:** Document A has a Part Studio with a 50×30×25 block (the Control Arm stand-in works
  too). Document B derives it at the origin and at a mate connector offset (0, 0, 100):
  - two copies;
  - the volume is twice the source's;
  - the centroid Z is offset by 50.
- **ex-dv2:** Edit the source in A. B stays unchanged, the update indicator shows, and Update
  makes the new volume appear.
- **ex-dv3:** In the same document, derive with a workspace reference. A source edit shows up
  immediately.
- **ex-dv4:** An assembly in document C inserts the Part Studio from A at version v1, mates it,
  then updates to v2. The mates survive when the mated faces persist.
- **ex-dv5:** A circular derive (A→B→A) is refused with an error.

## Cross-cutting
- X1 One `ExternalRef` model and one resolver/cache in `cadrs_core`, shared by assemblies, Derived
  and drawings (drawings can already reference parts in other tabs, and cross-document is DV-new).
- X2 Version storage has to be cheap to load read-only; it builds on P3D.3 versions and history.
