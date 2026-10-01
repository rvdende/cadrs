//! **Component documents** (P3H.7; PCB7.3, PCB11.1–PCB11.3, X7): Create assembly makes one
//! stored document per new ECAD package in the PCB settings' component folder, versions it, and
//! the board assembly inserts each component as a **version-pinned reference** to that version
//! ([`crate::external`]). The geometry is made by `cadrs_pcb::component_docs`; this module holds
//! what is stored and the library-side work, which doesn't need the kernel:
//!
//! - **The key** ([`ComponentKey`]): kept in the component document's metadata
//!   ([`crate::library::DocumentMeta::pcb_component`]), so a later Create (of any board, in any
//!   document) finds the document again by its package and part number, whatever the document
//!   is called now and whichever folder it was moved to (PCB11.3). The documents page lists it
//!   from `entry.ron`, so finding it reads no document.
//! - **The folder** ([`ensure_folder`]): the settings' folder, made again (same id and name)
//!   when it was deleted; with no folder chosen, a top-level folder "PCB Components" (found by
//!   name, else made). Folders are library work and are not undone, like the documents page's
//!   New folder.
//! - **Undo** (decision, as 3G's Move to document): the component documents and their versions
//!   are written when Create runs, before the board document's one undo step. Undoing Create
//!   removes the generated tabs and the frozen copies from the board document; the component
//!   documents and their versions stay (versions are immutable, ER4.8) and the next Create
//!   reuses them.

use serde::{Deserialize, Serialize};

use crate::ids::{DocumentId, ElementId, FolderId, PartId};
use crate::library::{FolderEntry, Library, Timestamp};
use crate::store::{Store, StoreError};

/// The name of the folder used when the PCB settings name none.
pub const DEFAULT_COMPONENT_FOLDER: &str = "PCB Components";

/// What makes a stored document a package's component document (see the module docs). The
/// Part Studio and part are where the component's part was made (a version that lacks them
/// falls back to its first Part Studio's first part).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentKey {
    pub package: String,
    #[serde(default)]
    pub part_number: String,
    pub studio: ElementId,
    pub part: PartId,
}

impl ComponentKey {
    /// True for the key of `package` / `part_number`.
    pub fn is(&self, package: &str, part_number: &str) -> bool {
        self.package == package && self.part_number == part_number
    }
}

/// A package's component document, as a generation used it (kept in
/// [`super::GeneratedAssembly::documents`], so Sync knows which linked documents are components
/// without reading the store).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDocument {
    pub package: String,
    #[serde(default)]
    pub part_number: String,
    pub document: DocumentId,
}

/// The live (not trashed) component document of `package` / `part_number` in `lib`, if any (the
/// newest one if there are several).
pub fn find_component_document(lib: &Library, package: &str, part_number: &str) -> Option<(DocumentId, ComponentKey)> {
    lib.entries
        .iter()
        .filter(|e| e.meta.trashed.is_none())
        .filter_map(|e| e.meta.pcb_component.as_ref().filter(|k| k.is(package, part_number)).map(|k| (e.meta.created, e.id, k.clone())))
        .max_by_key(|(t, _, _)| *t)
        .map(|(_, id, k)| (id, k))
}

/// Every live component document of `lib`.
pub fn component_documents(lib: &Library) -> Vec<(DocumentId, ComponentKey)> {
    lib.entries.iter().filter(|e| e.meta.trashed.is_none()).filter_map(|e| e.meta.pcb_component.clone().map(|k| (e.id, k))).collect()
}

/// The folder new component documents go in (see the module docs): `chosen` when it exists, made
/// again under its id when it was deleted, else [`DEFAULT_COMPONENT_FOLDER`]. Writes the
/// library's folders when one is made. Holds the store's library lock from reading the folders
/// to writing them, so two callers can't both make the folder (it runs on the kernel thread, next
/// to the main thread's library edits).
pub fn ensure_folder(store: &Store, chosen: Option<&super::FolderRef>, user: &str, now: Timestamp) -> Result<FolderEntry, StoreError> {
    let lock = store.lock_library();
    let (before, _) = store.list();
    let found = match chosen {
        Some(f) => before.folders.iter().find(|x| x.id == f.id).cloned(),
        None => before.folders.iter().find(|x| x.name == DEFAULT_COMPONENT_FOLDER).cloned(),
    };
    if let Some(f) = found {
        return Ok(f);
    }
    let entry = match chosen {
        Some(f) => FolderEntry { id: f.id, name: f.name.clone(), created: now, owned_by: user.to_string() },
        None => FolderEntry { id: FolderId::new(), name: DEFAULT_COMPONENT_FOLDER.into(), created: now, owned_by: user.to_string() },
    };
    let mut after = before.clone();
    after.folders.push(entry.clone());
    store.sync_locked(&lock, &before, &after)?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{DocumentEntry, DocumentMeta};

    fn entry(name: &str, created: Timestamp, key: Option<ComponentKey>, trashed: bool) -> DocumentEntry {
        let mut meta = DocumentMeta::new("u", created);
        meta.pcb_component = key;
        meta.trashed = trashed.then_some(created + 1);
        DocumentEntry { id: DocumentId::new(), name: name.into(), meta }
    }

    fn key(p: &str) -> ComponentKey {
        ComponentKey { package: p.into(), part_number: "PN".into(), studio: ElementId::from_u128(1), part: PartId::new(crate::ids::FeatureId::from_u128(2), 0) }
    }

    #[test]
    fn finds_by_key_not_name_and_skips_the_trash() {
        let mut lib = Library::default();
        lib.entries.push(entry("renamed", 10, Some(key("SOT23")), false));
        lib.entries.push(entry("SOT23", 20, None, false));
        lib.entries.push(entry("old", 30, Some(key("SOT23")), true));
        let (id, k) = find_component_document(&lib, "SOT23", "PN").unwrap();
        assert_eq!(id, lib.entries[0].id);
        assert_eq!(k.package, "SOT23");
        assert!(find_component_document(&lib, "SOT23", "other").is_none());
        assert_eq!(component_documents(&lib).len(), 1);
    }

    fn temp_store(tag: &str) -> Store {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        Store::new(std::env::temp_dir().join(format!("cadrs-compdocs-{tag}-{}-{n}", std::process::id())))
    }

    #[test]
    fn concurrent_ensure_folder_makes_one_folder() {
        // Eight threads at once (as Creates on the kernel thread next to the main thread): one
        // "PCB Components" folder, the same for all.
        let store = temp_store("ensure");
        let ids: Vec<FolderId> = std::thread::scope(|s| {
            let jobs: Vec<_> = (0..8).map(|_| s.spawn(|| ensure_folder(&store, None, "u", 1).unwrap().id)).collect();
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        assert!(ids.iter().all(|i| *i == ids[0]));
        let lib = store.list().0;
        assert_eq!(lib.folders.iter().filter(|f| f.name == DEFAULT_COMPONENT_FOLDER).count(), 1);
        let _ = std::fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_stale_library_edit_keeps_the_component_folder() {
        // The documents page read the library before Create made "PCB Components"; its New
        // folder (from that stale copy) must not drop it.
        let store = temp_store("stale");
        let (before, _) = store.list();
        let made = ensure_folder(&store, None, "u", 1).unwrap();
        let mut after = before.clone();
        after.folders.push(FolderEntry { id: FolderId::new(), name: "Mine".into(), created: 2, owned_by: "u".into() });
        store.sync(&before, &after).unwrap();
        let names: Vec<String> = store.list().0.folders.into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Mine", DEFAULT_COMPONENT_FOLDER]);
        // Deleting a folder (undo of New folder) from the same stale copy keeps it too.
        store.sync(&after, &before).unwrap();
        let lib = store.list().0;
        assert_eq!(lib.folders.iter().map(|f| f.id).collect::<Vec<_>>(), [made.id]);
        let _ = std::fs::remove_dir_all(store.root());
    }
}
