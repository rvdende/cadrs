//! The document library shown on the documents page: one [`DocumentEntry`] (name and metadata)
//! per stored document, the sidebar [`Filter`]s, sorting, and the undoable library commands
//! (rename, move to trash, restore).
//!
//! P3E.1 (TD3.1, TD3.3, TD3.7, TD3.8): **labels**, a list of [`LabelEntry`]s (name and colour)
//! in the library and a document's [`DocumentMeta::labels`], created, renamed, assigned and
//! deleted by [`CreateLabel`], [`RenameLabel`], [`SetLabels`] and [`DeleteLabel`]; the sidebar's
//! [`Filter::Label`]; a document's [`DocumentMeta::description`] ([`SetDescription`]); and the
//! list's [`ItemType`] filter (All / Documents / Folders).
//!
//! Library commands follow the same pattern as document commands: a [`LibraryCommand`] runs
//! through a [`LibraryHistory`], which snapshots the library before and after so undo and redo
//! restore a snapshot. The app writes changed entries back to disk after each step (see
//! [`crate::store::Store::sync`]).

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::command::CommandError;
use crate::ids::{DocumentId, FolderId, LabelId};

/// Seconds since the Unix epoch (UTC).
pub type Timestamp = i64;

/// Metadata stored next to a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub created: Timestamp,
    pub modified: Timestamp,
    pub created_by: String,
    pub modified_by: String,
    pub owned_by: String,
    /// When the current user last opened the document.
    #[serde(default)]
    pub last_opened: Option<Timestamp>,
    /// When the document was moved to the trash; `None` if it is not in the trash.
    #[serde(default)]
    pub trashed: Option<Timestamp>,
    /// The folder it is in (P3G.1, ER1.2, ER X8: folders as locations of the Other documents
    /// browser); `None` at the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<FolderId>,
    /// The labels given to it (P3E.1, TD3.8), in the order they were added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelId>,
    /// The document description shown and edited in the details panel (P3E.1, TD3.8).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// P3H.7 (PCB7.3, PCB11.1): set on a PCB component document, the package it models (found
    /// again by it whatever the document's name or folder; `crate::pcb::component_docs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcb_component: Option<crate::pcb::component_docs::ComponentKey>,
    /// The workspace last opened (P3E.4, TD3.7: shown beside the name on the documents page);
    /// `None` for Main.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

impl DocumentMeta {
    /// The last-opened workspace's name ("Main" unless a branch was).
    pub fn workspace_name(&self) -> &str {
        self.workspace.as_deref().unwrap_or(crate::history_log::MAIN_NAME)
    }

    /// Metadata for a document `user` creates at `now`.
    pub fn new(user: &str, now: Timestamp) -> Self {
        Self {
            created: now,
            modified: now,
            created_by: user.to_string(),
            modified_by: user.to_string(),
            owned_by: user.to_string(),
            last_opened: None,
            trashed: None,
            folder: None,
            labels: Vec::new(),
            description: String::new(),
            pcb_component: None,
            workspace: None,
        }
    }
}

/// One document as listed on the documents page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentEntry {
    pub id: DocumentId,
    pub name: String,
    pub meta: DocumentMeta,
}

/// A folder on the documents page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub id: FolderId,
    pub name: String,
    pub created: Timestamp,
    pub owned_by: String,
}

/// A document label (P3E.1): a name and a colour, shared by every document of the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelEntry {
    pub id: LabelId,
    pub name: String,
    /// sRGB.
    pub colour: [u8; 3],
}

/// The colours new labels take in turn (muted, readable behind dark text).
pub const LABEL_COLOURS: [[u8; 3]; 8] = [
    [0x5b, 0x9b, 0xd5],
    [0x70, 0xad, 0x47],
    [0xed, 0x7d, 0x31],
    [0xa5, 0x6c, 0xc1],
    [0xff, 0xc0, 0x00],
    [0x26, 0xa6, 0x9a],
    [0xe0, 0x5b, 0x7a],
    [0x8d, 0x99, 0xa6],
];

/// Every stored document, folder and label.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Library {
    pub entries: Vec<DocumentEntry>,
    pub folders: Vec<FolderEntry>,
    /// P3E.1: the labels, in the order they were created.
    pub labels: Vec<LabelEntry>,
}

impl Library {
    pub fn get(&self, id: DocumentId) -> Option<&DocumentEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: DocumentId) -> Option<&mut DocumentEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Adds or replaces an entry.
    pub fn upsert(&mut self, entry: DocumentEntry) {
        match self.get_mut(entry.id) {
            Some(e) => *e = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn label(&self, id: LabelId) -> Option<&LabelEntry> {
        self.labels.iter().find(|l| l.id == id)
    }

    /// The labels, by name (the sidebar's and the details panel's order).
    pub fn labels_by_name(&self) -> Vec<LabelEntry> {
        let mut v = self.labels.clone();
        v.sort_by(|a, b| natural_cmp(&a.name, &b.name).then_with(|| a.id.cmp(&b.id)));
        v
    }

    /// The colour the next new label takes.
    pub fn next_label_colour(&self) -> [u8; 3] {
        LABEL_COLOURS[self.labels.len() % LABEL_COLOURS.len()]
    }

    /// The live (not trashed) documents with `label` (P3E.1, ER1.2: the Other documents
    /// browser's Labels location), by name.
    pub fn with_label(&self, label: LabelId) -> Vec<DocumentEntry> {
        let mut v: Vec<DocumentEntry> = self.entries.iter().filter(|e| e.meta.trashed.is_none() && e.meta.labels.contains(&label)).cloned().collect();
        sort_entries(&mut v, SortKey::Name, SortDir::Ascending);
        v
    }

    /// The live (not trashed) documents in `folder` (P3G.1), by name.
    pub fn in_folder(&self, folder: FolderId) -> Vec<DocumentEntry> {
        let mut v: Vec<DocumentEntry> = self.entries.iter().filter(|e| e.meta.trashed.is_none() && e.meta.folder == Some(folder)).cloned().collect();
        sort_entries(&mut v, SortKey::Name, SortDir::Ascending);
        v
    }

    /// The entries shown for `filter` (for `user`), sorted.
    pub fn view(
        &self,
        filter: Filter,
        user: &str,
        search: &str,
        key: SortKey,
        dir: SortDir,
    ) -> Vec<DocumentEntry> {
        let needle = search.trim().to_lowercase();
        let mut v: Vec<DocumentEntry> = self
            .entries
            .iter()
            .filter(|e| filter.matches(e, user))
            .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        if filter == Filter::RecentlyOpened && key == SortKey::Modified {
            // "Recently opened" orders by when the document was opened.
            v.sort_by(|a, b| {
                let o = a.meta.last_opened.cmp(&b.meta.last_opened);
                let o = if dir == SortDir::Descending {
                    o.reverse()
                } else {
                    o
                };
                o.then_with(|| a.id.cmp(&b.id))
            });
        } else {
            sort_entries(&mut v, key, dir);
        }
        v
    }
}

/// The filters in the documents page sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Filter {
    Explore,
    #[default]
    OwnedByMe,
    RecentlyOpened,
    CreatedByMe,
    SharedWithMe,
    Public,
    Trash,
    /// The documents with this label (P3E.1, TD3.3: the sidebar's Labels section).
    Label(LabelId),
}

impl Filter {
    pub const ALL: [Filter; 7] = [
        Filter::Explore,
        Filter::OwnedByMe,
        Filter::RecentlyOpened,
        Filter::CreatedByMe,
        Filter::SharedWithMe,
        Filter::Public,
        Filter::Trash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::Explore => "Explore cadrs",
            Filter::OwnedByMe => "Owned by me",
            Filter::RecentlyOpened => "Recently opened",
            Filter::CreatedByMe => "Created by me",
            Filter::SharedWithMe => "Shared with me",
            Filter::Public => "Public",
            Filter::Trash => "Trash",
            Filter::Label(_) => "Label",
        }
    }

    /// Whether `entry` is listed under this filter for `user`.
    pub fn matches(self, entry: &DocumentEntry, user: &str) -> bool {
        let m = &entry.meta;
        let live = m.trashed.is_none();
        match self {
            Filter::Explore | Filter::SharedWithMe | Filter::Public => false,
            Filter::OwnedByMe => live && m.owned_by == user,
            Filter::RecentlyOpened => live && m.last_opened.is_some(),
            Filter::CreatedByMe => live && m.created_by == user,
            Filter::Trash => !live,
            Filter::Label(l) => live && m.labels.contains(&l),
        }
    }
}

/// The list's **Type** filter (P3E.1, TD3.7): which kinds of item the documents list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ItemType {
    #[default]
    All,
    Documents,
    Folders,
}

impl ItemType {
    pub const ALL: [ItemType; 3] = [ItemType::All, ItemType::Documents, ItemType::Folders];

    pub fn label(self) -> &'static str {
        match self {
            ItemType::All => "All",
            ItemType::Documents => "Documents",
            ItemType::Folders => "Folders",
        }
    }

    pub fn shows_documents(self) -> bool {
        self != ItemType::Folders
    }

    pub fn shows_folders(self) -> bool {
        self != ItemType::Documents
    }
}

/// A sortable column of the document list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortKey {
    Name,
    #[default]
    Modified,
    ModifiedBy,
    OwnedBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortDir {
    Ascending,
    #[default]
    Descending,
}

impl SortDir {
    pub fn flipped(self) -> Self {
        match self {
            SortDir::Ascending => SortDir::Descending,
            SortDir::Descending => SortDir::Ascending,
        }
    }
}

/// Case-insensitive comparison that orders embedded numbers numerically ("Part 2" < "Part 10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Sorts entries by `key` in direction `dir`. Ties fall back to the most recently modified
/// first, then the id, so the order is always deterministic.
pub fn sort_entries(entries: &mut [DocumentEntry], key: SortKey, dir: SortDir) {
    entries.sort_by(|a, b| {
        let primary = match key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Modified => a.meta.modified.cmp(&b.meta.modified),
            SortKey::ModifiedBy => natural_cmp(&a.meta.modified_by, &b.meta.modified_by),
            SortKey::OwnedBy => natural_cmp(&a.meta.owned_by, &b.meta.owned_by),
        };
        let primary = if dir == SortDir::Descending {
            primary.reverse()
        } else {
            primary
        };
        primary
            .then_with(|| b.meta.modified.cmp(&a.meta.modified))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// An edit of the [`Library`].
pub trait LibraryCommand: fmt::Debug + Send + Sync + 'static {
    fn label(&self) -> String;
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError>;
}

fn entry_mut(lib: &mut Library, id: DocumentId) -> Result<&mut DocumentEntry, CommandError> {
    lib.get_mut(id)
        .ok_or_else(|| CommandError::Invalid(format!("document {id} not found")))
}

/// Renames a document from the documents page.
#[derive(Debug, Clone)]
pub struct RenameEntry {
    pub id: DocumentId,
    pub name: String,
    pub user: String,
    pub now: Timestamp,
}

impl LibraryCommand for RenameEntry {
    fn label(&self) -> String {
        format!("Rename document to {}", self.name.trim())
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("name must not be empty".into()));
        }
        let e = entry_mut(lib, self.id)?;
        if e.name == name {
            return Ok(());
        }
        e.name = name.to_string();
        e.meta.modified = self.now;
        e.meta.modified_by = self.user.clone();
        Ok(())
    }
}

/// Moves a document to the trash.
#[derive(Debug, Clone)]
pub struct TrashEntry {
    pub id: DocumentId,
    pub now: Timestamp,
}

impl LibraryCommand for TrashEntry {
    fn label(&self) -> String {
        "Move document to trash".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let e = entry_mut(lib, self.id)?;
        if e.meta.trashed.is_some() {
            return Err(CommandError::Invalid("document is already in the trash".into()));
        }
        e.meta.trashed = Some(self.now);
        Ok(())
    }
}

/// Restores a document from the trash.
#[derive(Debug, Clone)]
pub struct RestoreEntry {
    pub id: DocumentId,
}

impl LibraryCommand for RestoreEntry {
    fn label(&self) -> String {
        "Restore document".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let e = entry_mut(lib, self.id)?;
        if e.meta.trashed.is_none() {
            return Err(CommandError::Invalid("document is not in the trash".into()));
        }
        e.meta.trashed = None;
        Ok(())
    }
}

/// Creates a folder.
#[derive(Debug, Clone)]
pub struct CreateFolder {
    pub id: FolderId,
    pub name: String,
    pub user: String,
    pub now: Timestamp,
}

impl LibraryCommand for CreateFolder {
    fn label(&self) -> String {
        format!("Create folder {}", self.name.trim())
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("name must not be empty".into()));
        }
        if lib.folders.iter().any(|f| f.id == self.id) {
            return Err(CommandError::Invalid("folder id already in use".into()));
        }
        lib.folders.push(FolderEntry {
            id: self.id,
            name: name.to_string(),
            created: self.now,
            owned_by: self.user.clone(),
        });
        Ok(())
    }
}

/// Moves a document into a folder, or back to the top level with `None` (P3G.1, ER X8: the
/// documents page's Move to…).
#[derive(Debug, Clone)]
pub struct MoveToFolder {
    pub id: DocumentId,
    pub folder: Option<FolderId>,
}

impl LibraryCommand for MoveToFolder {
    fn label(&self) -> String {
        "Move document to folder".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        if let Some(f) = self.folder
            && lib.folders.iter().all(|x| x.id != f)
        {
            return Err(CommandError::Invalid("folder not found".into()));
        }
        entry_mut(lib, self.id)?.meta.folder = self.folder;
        Ok(())
    }
}

/// Adds a document that already exists on disk to the library (Copy…: the store writes the
/// copy first, see [`crate::store::Store::copy_document`]). Undo removes the entry, and the
/// store sets its files aside like a permanent delete, so redo can bring them back.
#[derive(Debug, Clone)]
pub struct AddEntry {
    pub entry: DocumentEntry,
}

impl LibraryCommand for AddEntry {
    fn label(&self) -> String {
        format!("Copy document {}", self.entry.name)
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        if lib.get(self.entry.id).is_some() {
            return Err(CommandError::Invalid("document id already in use".into()));
        }
        lib.entries.push(self.entry.clone());
        Ok(())
    }
}

/// Deletes a trashed document permanently. The store keeps the files aside until the app
/// exits, so undo can bring it back.
#[derive(Debug, Clone)]
pub struct PurgeEntry {
    pub id: DocumentId,
}

impl LibraryCommand for PurgeEntry {
    fn label(&self) -> String {
        "Delete document permanently".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let i = lib
            .entries
            .iter()
            .position(|e| e.id == self.id)
            .ok_or_else(|| CommandError::Invalid(format!("document {} not found", self.id)))?;
        if lib.entries[i].meta.trashed.is_none() {
            return Err(CommandError::Invalid(
                "only documents in the trash can be deleted permanently".into(),
            ));
        }
        lib.entries.remove(i);
        Ok(())
    }
}

/// Creates a label (P3E.1: Create ▸ Label…, the details panel's "Create new label"), and gives
/// it to `assign` in the same step.
#[derive(Debug, Clone)]
pub struct CreateLabel {
    pub id: LabelId,
    pub name: String,
    pub colour: [u8; 3],
    pub assign: Vec<DocumentId>,
}

fn check_label_name(lib: &Library, name: &str, except: Option<LabelId>) -> Result<(), CommandError> {
    if name.is_empty() {
        return Err(CommandError::Invalid("name must not be empty".into()));
    }
    if lib.labels.iter().any(|l| Some(l.id) != except && l.name.to_lowercase() == name.to_lowercase()) {
        return Err(CommandError::Invalid(format!("a label named {name} already exists")));
    }
    Ok(())
}

impl LibraryCommand for CreateLabel {
    fn label(&self) -> String {
        format!("Create label {}", self.name.trim())
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let name = self.name.trim();
        check_label_name(lib, name, None)?;
        if lib.label(self.id).is_some() {
            return Err(CommandError::Invalid("label id already in use".into()));
        }
        lib.labels.push(LabelEntry { id: self.id, name: name.to_string(), colour: self.colour });
        for d in &self.assign {
            let e = entry_mut(lib, *d)?;
            if !e.meta.labels.contains(&self.id) {
                e.meta.labels.push(self.id);
            }
        }
        Ok(())
    }
}

/// Renames a label (the sidebar label's Rename…).
#[derive(Debug, Clone)]
pub struct RenameLabel {
    pub id: LabelId,
    pub name: String,
}

impl LibraryCommand for RenameLabel {
    fn label(&self) -> String {
        format!("Rename label to {}", self.name.trim())
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let name = self.name.trim();
        check_label_name(lib, name, Some(self.id))?;
        let l = lib.labels.iter_mut().find(|l| l.id == self.id).ok_or_else(|| CommandError::Invalid("label not found".into()))?;
        l.name = name.to_string();
        Ok(())
    }
}

/// Deletes a label and takes it off every document (one step).
#[derive(Debug, Clone)]
pub struct DeleteLabel {
    pub id: LabelId,
}

impl LibraryCommand for DeleteLabel {
    fn label(&self) -> String {
        "Delete label".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        let i = lib.labels.iter().position(|l| l.id == self.id).ok_or_else(|| CommandError::Invalid("label not found".into()))?;
        lib.labels.remove(i);
        for e in &mut lib.entries {
            e.meta.labels.retain(|l| *l != self.id);
        }
        Ok(())
    }
}

/// Sets a document's labels (the details panel's checkboxes and the row menu's Labels ▸).
#[derive(Debug, Clone)]
pub struct SetLabels {
    pub id: DocumentId,
    pub labels: Vec<LabelId>,
}

impl LibraryCommand for SetLabels {
    fn label(&self) -> String {
        "Change document labels".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        if let Some(l) = self.labels.iter().find(|l| lib.label(**l).is_none()) {
            return Err(CommandError::Invalid(format!("label {l} not found")));
        }
        let mut labels: Vec<LabelId> = Vec::new();
        for l in &self.labels {
            if !labels.contains(l) {
                labels.push(*l);
            }
        }
        entry_mut(lib, self.id)?.meta.labels = labels;
        Ok(())
    }
}

/// Sets a document's description (the details panel).
#[derive(Debug, Clone)]
pub struct SetDescription {
    pub id: DocumentId,
    pub description: String,
}

impl LibraryCommand for SetDescription {
    fn label(&self) -> String {
        "Change document description".into()
    }
    fn apply(&self, lib: &mut Library) -> Result<(), CommandError> {
        entry_mut(lib, self.id)?.meta.description = self.description.trim().to_string();
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct LibEntry {
    label: String,
    before: Library,
    after: Library,
}

/// Undo/redo for library commands.
#[derive(Debug, Clone, Default)]
pub struct LibraryHistory {
    undo: Vec<LibEntry>,
    redo: Vec<LibEntry>,
}

impl LibraryHistory {
    /// Applies `cmd`. On error the library is unchanged and nothing is recorded.
    pub fn execute(
        &mut self,
        lib: &mut Library,
        cmd: &dyn LibraryCommand,
    ) -> Result<(), CommandError> {
        let before = lib.clone();
        if let Err(e) = cmd.apply(lib) {
            *lib = before;
            return Err(e);
        }
        if *lib == before {
            return Ok(());
        }
        self.undo.push(LibEntry {
            label: cmd.label(),
            before,
            after: lib.clone(),
        });
        self.redo.clear();
        Ok(())
    }

    /// Undoes the last command, returning its label.
    pub fn undo(&mut self, lib: &mut Library) -> Option<String> {
        let e = self.undo.pop()?;
        *lib = e.before.clone();
        let label = e.label.clone();
        self.redo.push(e);
        Some(label)
    }

    /// Redoes the last undone command, returning its label.
    pub fn redo(&mut self, lib: &mut Library) -> Option<String> {
        let e = self.redo.pop()?;
        *lib = e.after.clone();
        let label = e.label.clone();
        self.undo.push(e);
        Some(label)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Forgets all steps (used when the library is reloaded from disk).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: u128, name: &str, modified: Timestamp) -> DocumentEntry {
        let mut meta = DocumentMeta::new("me", modified);
        meta.modified_by = if n.is_multiple_of(2) { "alice" } else { "bob" }.into();
        DocumentEntry {
            id: DocumentId::from_u128(n),
            name: name.into(),
            meta,
        }
    }

    fn lib() -> Library {
        Library {
            folders: vec![],
            labels: vec![],
            entries: vec![
                entry(1, "bracket", 300),
                entry(2, "Axle 10", 100),
                entry(3, "Axle 2", 200),
                entry(4, "cover", 400),
            ],
        }
    }

    fn names(v: &[DocumentEntry]) -> Vec<&str> {
        v.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("Axle 2", "Axle 10"), Ordering::Less);
        assert_eq!(natural_cmp("axle", "Axle"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "ab"), Ordering::Less);
        assert_eq!(natural_cmp("B", "a"), Ordering::Greater);
    }

    #[test]
    fn sort_by_each_column() {
        let mut v = lib().entries;
        sort_entries(&mut v, SortKey::Name, SortDir::Ascending);
        assert_eq!(names(&v), ["Axle 2", "Axle 10", "bracket", "cover"]);
        sort_entries(&mut v, SortKey::Name, SortDir::Descending);
        assert_eq!(names(&v), ["cover", "bracket", "Axle 10", "Axle 2"]);
        sort_entries(&mut v, SortKey::Modified, SortDir::Descending);
        assert_eq!(names(&v), ["cover", "bracket", "Axle 2", "Axle 10"]);
        sort_entries(&mut v, SortKey::Modified, SortDir::Ascending);
        assert_eq!(names(&v), ["Axle 10", "Axle 2", "bracket", "cover"]);
        // Ties on "modified by" fall back to newest first.
        sort_entries(&mut v, SortKey::ModifiedBy, SortDir::Ascending);
        assert_eq!(names(&v), ["cover", "Axle 10", "bracket", "Axle 2"]);
    }

    #[test]
    fn filters_and_search() {
        let mut l = lib();
        l.entries[0].meta.trashed = Some(500);
        l.entries[1].meta.last_opened = Some(600);
        let owned = l.view(
            Filter::OwnedByMe,
            "me",
            "",
            SortKey::Name,
            SortDir::Ascending,
        );
        assert_eq!(names(&owned), ["Axle 2", "Axle 10", "cover"]);
        let trash = l.view(Filter::Trash, "me", "", SortKey::Name, SortDir::Ascending);
        assert_eq!(names(&trash), ["bracket"]);
        let recent = l.view(
            Filter::RecentlyOpened,
            "me",
            "",
            SortKey::Modified,
            SortDir::Descending,
        );
        assert_eq!(names(&recent), ["Axle 10"]);
        let found = l.view(
            Filter::OwnedByMe,
            "me",
            "axle",
            SortKey::Name,
            SortDir::Ascending,
        );
        assert_eq!(names(&found), ["Axle 2", "Axle 10"]);
        assert!(
            l.view(
                Filter::OwnedByMe,
                "someone else",
                "",
                SortKey::Name,
                SortDir::Ascending
            )
            .is_empty()
        );
    }

    #[test]
    fn rename_with_undo_redo() {
        let mut l = lib();
        let mut h = LibraryHistory::default();
        let id = DocumentId::from_u128(1);
        h.execute(
            &mut l,
            &RenameEntry {
                id,
                name: "  Bracket v2 ".into(),
                user: "me".into(),
                now: 999,
            },
        )
        .unwrap();
        assert_eq!(l.get(id).unwrap().name, "Bracket v2");
        assert_eq!(l.get(id).unwrap().meta.modified, 999);
        assert_eq!(h.undo(&mut l).as_deref(), Some("Rename document to Bracket v2"));
        assert_eq!(l.get(id).unwrap().name, "bracket");
        assert_eq!(l.get(id).unwrap().meta.modified, 300);
        h.redo(&mut l);
        assert_eq!(l.get(id).unwrap().name, "Bracket v2");
        assert!(!h.can_redo());
    }

    #[test]
    fn rename_rejects_empty_and_ignores_noop() {
        let mut l = lib();
        let before = l.clone();
        let mut h = LibraryHistory::default();
        let id = DocumentId::from_u128(1);
        let bad = RenameEntry {
            id,
            name: "   ".into(),
            user: "me".into(),
            now: 1,
        };
        assert!(h.execute(&mut l, &bad).is_err());
        let same = RenameEntry {
            name: "bracket".into(),
            ..bad
        };
        h.execute(&mut l, &same).unwrap();
        assert_eq!(l, before);
        assert!(!h.can_undo());
    }

    #[test]
    fn trash_keeps_modified_time() {
        let mut l = lib();
        let mut h = LibraryHistory::default();
        let id = DocumentId::from_u128(3);
        h.execute(&mut l, &TrashEntry { id, now: 9_999 }).unwrap();
        assert_eq!(l.get(id).unwrap().meta.modified, 200);
        h.execute(&mut l, &RestoreEntry { id }).unwrap();
        assert_eq!(l.get(id).unwrap().meta.modified, 200);
    }

    #[test]
    fn create_folder_with_undo() {
        let mut l = lib();
        let mut h = LibraryHistory::default();
        let id = FolderId::from_u128(1);
        let cmd = CreateFolder {
            id,
            name: " Fixtures ".into(),
            user: "me".into(),
            now: 5,
        };
        h.execute(&mut l, &cmd).unwrap();
        assert_eq!(l.folders[0].name, "Fixtures");
        assert!(h.execute(&mut l, &cmd).is_err());
        h.undo(&mut l);
        assert!(l.folders.is_empty());
        h.redo(&mut l);
        assert_eq!(l.folders.len(), 1);
    }

    #[test]
    fn purge_only_trashed_with_undo() {
        let mut l = lib();
        let mut h = LibraryHistory::default();
        let id = DocumentId::from_u128(2);
        assert!(h.execute(&mut l, &PurgeEntry { id }).is_err());
        h.execute(&mut l, &TrashEntry { id, now: 1 }).unwrap();
        h.execute(&mut l, &PurgeEntry { id }).unwrap();
        assert!(l.get(id).is_none());
        h.undo(&mut l);
        assert!(l.get(id).is_some());
    }

    #[test]
    fn trash_restore_with_undo() {
        let mut l = lib();
        let mut h = LibraryHistory::default();
        let id = DocumentId::from_u128(3);
        h.execute(&mut l, &TrashEntry { id, now: 700 }).unwrap();
        assert_eq!(l.get(id).unwrap().meta.trashed, Some(700));
        assert!(h.execute(&mut l, &TrashEntry { id, now: 701 }).is_err());
        h.undo(&mut l);
        assert_eq!(l.get(id).unwrap().meta.trashed, None);
        h.redo(&mut l);
        h.execute(&mut l, &RestoreEntry { id }).unwrap();
        assert_eq!(l.get(id).unwrap().meta.trashed, None);
        h.undo(&mut l);
        assert_eq!(l.get(id).unwrap().meta.trashed, Some(700));
        assert!(
            h.execute(
                &mut l,
                &RestoreEntry {
                    id: DocumentId::from_u128(99)
                }
            )
            .is_err()
        );
    }

    fn five() -> Library {
        let mut l = lib();
        l.entries.push(entry(5, "hinge", 500));
        l
    }

    #[test]
    fn labels_create_assign_delete_with_undo() {
        let mut l = five();
        let mut h = LibraryHistory::default();
        let fixtures = LabelId::from_u128(1);
        let (a, b) = (DocumentId::from_u128(1), DocumentId::from_u128(2));
        h.execute(&mut l, &CreateLabel { id: fixtures, name: " Fixtures ".into(), colour: LABEL_COLOURS[0], assign: vec![a] }).unwrap();
        assert_eq!(l.label(fixtures).unwrap().name, "Fixtures");
        assert_eq!(l.get(a).unwrap().meta.labels, [fixtures]);
        // Names are unique (case-insensitive) and not empty.
        assert!(h.execute(&mut l, &CreateLabel { id: LabelId::from_u128(2), name: "fixtures".into(), colour: [0; 3], assign: vec![] }).is_err());
        assert!(h.execute(&mut l, &CreateLabel { id: LabelId::from_u128(2), name: "  ".into(), colour: [0; 3], assign: vec![] }).is_err());
        h.execute(&mut l, &SetLabels { id: b, labels: vec![fixtures, fixtures] }).unwrap();
        assert_eq!(l.get(b).unwrap().meta.labels, [fixtures]);
        h.execute(&mut l, &RenameLabel { id: fixtures, name: "Jigs".into() }).unwrap();
        assert_eq!(l.label(fixtures).unwrap().name, "Jigs");
        h.execute(&mut l, &DeleteLabel { id: fixtures }).unwrap();
        assert!(l.labels.is_empty());
        assert!(l.entries.iter().all(|e| e.meta.labels.is_empty()));
        // Undo brings the label back on both documents, then its old name, then unassigns.
        h.undo(&mut l);
        assert_eq!(l.get(a).unwrap().meta.labels, [fixtures]);
        assert_eq!(l.get(b).unwrap().meta.labels, [fixtures]);
        h.undo(&mut l);
        assert_eq!(l.label(fixtures).unwrap().name, "Fixtures");
        h.undo(&mut l);
        assert!(l.get(b).unwrap().meta.labels.is_empty());
        h.undo(&mut l);
        assert!(l.labels.is_empty() && l.get(a).unwrap().meta.labels.is_empty());
        h.redo(&mut l);
        assert_eq!(l.get(a).unwrap().meta.labels, [fixtures]);
        // An unknown label can't be assigned.
        assert!(h.execute(&mut l, &SetLabels { id: b, labels: vec![LabelId::from_u128(9)] }).is_err());
    }

    #[test]
    fn a_label_filter_returns_the_tagged_documents() {
        let mut l = five();
        let mut h = LibraryHistory::default();
        let hw = LabelId::from_u128(7);
        let tagged = [1u128, 3, 5].map(DocumentId::from_u128);
        h.execute(&mut l, &CreateLabel { id: hw, name: "Hardware".into(), colour: [1, 2, 3], assign: tagged.to_vec() }).unwrap();
        assert_eq!(l.view(Filter::OwnedByMe, "me", "", SortKey::Name, SortDir::Ascending).len(), 5);
        let v = l.view(Filter::Label(hw), "me", "", SortKey::Name, SortDir::Ascending);
        assert_eq!(names(&v), ["Axle 2", "bracket", "hinge"]);
        assert_eq!(v.len(), 3);
        assert!(v.iter().all(|e| tagged.contains(&e.id)));
        assert_eq!(names(&l.with_label(hw)), ["Axle 2", "bracket", "hinge"]);
        // Trashed documents leave the label's list.
        h.execute(&mut l, &TrashEntry { id: tagged[0], now: 1 }).unwrap();
        assert_eq!(l.view(Filter::Label(hw), "me", "", SortKey::Name, SortDir::Ascending).len(), 2);
    }

    #[test]
    fn description_with_undo() {
        let mut l = lib();
        let mut h = LibraryHistory::default();
        let id = DocumentId::from_u128(4);
        h.execute(&mut l, &SetDescription { id, description: " Extended hubcap ".into() }).unwrap();
        assert_eq!(l.get(id).unwrap().meta.description, "Extended hubcap");
        h.undo(&mut l);
        assert_eq!(l.get(id).unwrap().meta.description, "");
    }

    #[test]
    fn item_type_filter() {
        assert!(ItemType::All.shows_documents() && ItemType::All.shows_folders());
        assert!(ItemType::Documents.shows_documents() && !ItemType::Documents.shows_folders());
        assert!(!ItemType::Folders.shows_documents() && ItemType::Folders.shows_folders());
    }
}
