//! P3D.3: a document's persisted history (IR5.6, X7; shared with T7, T9, TD12).
//!
//! Every committed change of a document is appended to its [`HistoryLog`]: when and by whom,
//! the tab and feature it touched and what was done to it ("Conrod :: Edit : Sketch 2"), and a
//! [`Delta`] that turns the state before it into the state after it. Every
//! [`SNAPSHOT_EVERY`] entries a full copy of the document is kept as well, so any past state
//! is rebuilt from the nearest copy before it and at most that many deltas
//! ([`HistoryLog::state_at`]).
//!
//! - The log is **append-only**: undo, redo and Restore are changes like any other and add
//!   entries of their own. [`RestoreDocument`] is the command Restore runs (a whole-document
//!   step, so it is undone as one).
//! - Entry 0 is **Start**: the document as it was when the log began (created, or opened for
//!   the first time with this version of cadrs), always with a full copy.
//! - It is stored next to the document, in `<store>/<id>/history.ron`
//!   ([`HistoryLog::save`], [`HistoryLog::load`]).
//! - **Versions** (P3D.3, T7, TD12.2): a [`Version`] is a named, immutable pointer to an entry
//!   (a name, an optional description, when and by whom). [`HistoryLog::create_version`] makes
//!   one at the current entry; [`HistoryLog::document_at_version`] is the document it stands
//!   for (Drawings' "Change to version" and views of a version use it). The log is
//!   append-only, so a version's state never changes.
//! - **Last healthy regeneration** (P3D.4, IR3.3): the rebuild notes, per feature, the last
//!   entry at which the feature regenerated without error ([`HistoryLog::note_healthy`]);
//!   "Edit healthy moment" shows the Part Studio at that entry.
//!
//! - **Workspaces** (P3E.4, TD12.4–TD12.7): Main and the **branches** made from a version
//!   ([`HistoryLog::branch`]). Each has its own changes (a [`Track`]); the log's own entries are
//!   the **current** workspace's (the one the document file holds), the others are kept aside
//!   until [`HistoryLog::switch_to`] swaps them in. A branch starts as an exact copy of its
//!   version, so its elements, features and part ids are the same as Main's. Versions belong to
//!   the workspace they were made in ([`Version::workspace`]). A log without branches (every
//!   log before P3E.4) is Main only and is written exactly as before.
//!
//! Commands are applied through snapshots (see [`crate::command`]), not replayed, so a delta
//! is the element contents a change produced rather than the command: replaying a delta can
//! never come out differently from the original change.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind, Feature};
use crate::ids::{DocumentId, ElementId, FeatureId};
use crate::library::Timestamp;
use crate::store::{Store, StoreError};

mod workspaces;
use workspaces::Track;
pub use workspaces::{MAIN_NAME, Workspace, WorkspaceId};

/// A full copy of the document is kept every this many entries.
pub const SNAPSHOT_EVERY: usize = 16;

/// The file the log is stored in, in the document's directory.
pub const HISTORY_FILE: &str = "history.ron";

/// The current `history.ron` schema version: 2 stores a Part Studio's changes as
/// [`StudioPatch`]es (only the features that changed). Version 1 logs (whole elements only)
/// still read, and are rewritten as version 2 when loaded ([`HistoryLog::load_path`]).
pub const HISTORY_VERSION: u32 = 2;

/// What one change did to the document: the new contents of the elements it changed or
/// added, the new list of elements when that changed, and the rest of the document (name,
/// units, libraries, …) when that changed. A Part Studio that was there before is a
/// [`StudioPatch`] rather than a whole copy (its features are most of a document, and a change
/// usually touches one of them).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    /// The document without its elements, if anything but the elements changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Box<Document>>,
    /// The ids of the elements in order, if elements were added, removed or reordered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<ElementId>>,
    /// The elements whose contents or name changed, and the new ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    /// The Part Studios that changed, as patches of their state before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<StudioPatch>,
}

/// The change to a Part Studio: its features that changed or were added, their order when that
/// changed, and the rest of the element (name, parts, appearances, folders, …) when that changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioPatch {
    pub id: ElementId,
    /// The element with no features, if anything but its features changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<Box<Element>>,
    /// The ids of the features in order, if features were added, removed or reordered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<FeatureId>>,
    /// The features whose contents changed, and the new ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
}

impl StudioPatch {
    /// The change from Part Studio `before` to Part Studio `after` (same id).
    fn between(before: &Element, after: &Element) -> Self {
        let shell = (shell_of(before) != shell_of(after)).then(|| Box::new(shell_of(after)));
        let ids = |e: &Element| e.features().iter().map(|f| f.id).collect::<Vec<_>>();
        let order = (ids(before) != ids(after)).then(|| ids(after));
        let features = after
            .features()
            .iter()
            .filter(|f| before.features().iter().find(|b| b.id == f.id) != Some(*f))
            .cloned()
            .collect();
        Self { id: after.id, shell, order, features }
    }

    /// Applies the change to `element` (the Part Studio it was made from).
    fn apply(&self, element: &mut Element) {
        let mut old = element.features_mut().map(std::mem::take).unwrap_or_default();
        if let Some(shell) = &self.shell {
            *element = (**shell).clone();
        }
        let order = self.order.clone().unwrap_or_else(|| old.iter().map(|f| f.id).collect());
        let mut features = Vec::with_capacity(order.len());
        for id in order {
            if let Some(f) = self.features.iter().find(|f| f.id == id) {
                features.push(f.clone());
            } else if let Some(i) = old.iter().position(|f| f.id == id) {
                features.push(old.swap_remove(i));
            }
        }
        if let Some(fs) = element.features_mut() {
            *fs = features;
        }
    }
}

/// A Part Studio without its features.
fn shell_of(e: &Element) -> Element {
    let mut shell = e.clone();
    if let Some(fs) = shell.features_mut() {
        fs.clear();
    }
    shell
}

fn is_studio(e: &Element) -> bool {
    matches!(e.kind, ElementKind::PartStudio { .. })
}

impl Delta {
    /// The change from `before` to `after`.
    pub fn between(before: &Document, after: &Document) -> Self {
        let head = (head_of(before) != head_of(after)).then(|| Box::new(head_of(after)));
        let ids = |d: &Document| d.elements.iter().map(|e| e.id).collect::<Vec<_>>();
        let order = (ids(before) != ids(after)).then(|| ids(after));
        let mut elements = Vec::new();
        let mut studios = Vec::new();
        for e in &after.elements {
            match before.elements.iter().find(|b| b.id == e.id) {
                Some(b) if b == e => {}
                Some(b) if is_studio(b) && is_studio(e) => studios.push(StudioPatch::between(b, e)),
                _ => elements.push(e.clone()),
            }
        }
        Self { head, order, elements, studios }
    }

    /// True if it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.head.is_none() && self.order.is_none() && self.elements.is_empty() && self.studios.is_empty()
    }

    /// Applies the change to `doc` (the state it was made from).
    pub fn apply(&self, doc: &mut Document) {
        if let Some(h) = &self.head {
            let elements = std::mem::take(&mut doc.elements);
            *doc = (**h).clone();
            doc.elements = elements;
        }
        let order: Vec<ElementId> = self
            .order
            .clone()
            .unwrap_or_else(|| doc.elements.iter().map(|e| e.id).collect());
        let mut old: Vec<Element> = std::mem::take(&mut doc.elements);
        for id in order {
            if let Some(e) = self.elements.iter().find(|e| e.id == id) {
                doc.elements.push(e.clone());
            } else if let Some(i) = old.iter().position(|e| e.id == id) {
                let mut e = old.swap_remove(i);
                if let Some(p) = self.studios.iter().find(|p| p.id == id) {
                    p.apply(&mut e);
                }
                doc.elements.push(e);
            }
        }
    }
}

/// The document without its elements.
fn head_of(doc: &Document) -> Document {
    Document {
        elements: Vec::new(),
        ..doc.clone()
    }
}

/// A stable hash of a document's contents (its RON text), to check a rebuilt state.
pub fn document_hash(doc: &Document) -> u64 {
    let text = ron::to_string(doc).unwrap_or_default();
    cadrs_kernel::naming::stable_hash(text.as_bytes())
}

/// One change in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub time: Timestamp,
    pub user: String,
    /// The tab it changed (the first, if several) and its name then.
    #[serde(default)]
    pub element: Option<ElementId>,
    #[serde(default)]
    pub element_name: String,
    /// "Insert", "Edit", "Delete", "Rename", "Undo", "Redo", "Restore", … ("Start" for entry 0).
    pub action: String,
    /// The feature it changed, if one, and its name then.
    #[serde(default)]
    pub feature: Option<FeatureId>,
    #[serde(default)]
    pub feature_name: String,
    /// What the history panel shows: "Conrod :: Edit : Sketch 2".
    pub label: String,
    /// [`document_hash`] of the state after it.
    pub hash: u64,
    pub delta: Delta,
}

/// A version's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VersionId(pub uuid::Uuid);

impl VersionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for VersionId {
    fn default() -> Self {
        Self::new()
    }
}

/// A named, immutable pointer to a history entry (P3D.3). Its fields are read through its
/// methods: nothing changes a version once made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Version {
    id: VersionId,
    name: String,
    #[serde(default)]
    description: String,
    entry: usize,
    time: Timestamp,
    user: String,
    /// Made by Update all references in an intermediate document (P3G.2, ER4.5, ER4.7), not by
    /// the user.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    auto: bool,
    /// The workspace it was made in (P3E.4; Main for every version before).
    #[serde(default, skip_serializing_if = "WorkspaceId::is_main")]
    workspace: WorkspaceId,
}

impl Version {
    /// The workspace it was made in (P3E.4): its entry is one of that workspace's.
    pub fn workspace(&self) -> WorkspaceId {
        self.workspace
    }
    /// Made automatically by Update all references (ER4.7).
    pub fn auto(&self) -> bool {
        self.auto
    }
    pub fn id(&self) -> VersionId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    /// The history entry it points at.
    pub fn entry(&self) -> usize {
        self.entry
    }
    pub fn time(&self) -> Timestamp {
        self.time
    }
    pub fn user(&self) -> &str {
        &self.user
    }
}

/// A feature's last healthy regeneration: the entry after which it last built without error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthyMark {
    pub element: ElementId,
    pub feature: FeatureId,
    pub entry: usize,
}

/// A document's history (see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryLog {
    pub version: u32,
    pub document: DocumentId,
    pub entries: Vec<LogEntry>,
    /// Full copies: (entry index, the state after it). Entry 0 always has one.
    snapshots: Vec<(usize, Document)>,
    #[serde(default)]
    pub healthy: Vec<HealthyMark>,
    /// The versions, oldest first.
    #[serde(default)]
    versions: Vec<Version>,
    /// P3E.4: the workspace whose changes `entries` are (and whose state the document file
    /// holds); Main unless a branch was last opened.
    #[serde(default, skip_serializing_if = "WorkspaceId::is_main")]
    current: WorkspaceId,
    /// P3E.4: the branches, oldest first (Main is implied).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    workspaces: Vec<Workspace>,
    /// P3E.4: the changes of every workspace but the current one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parked: Vec<Track>,
    /// The current state (the last entry's), kept so appending doesn't rebuild it.
    #[serde(skip)]
    head: Option<Document>,
    /// Read from an older schema and rewritten in memory: save it to keep the new form.
    #[serde(skip)]
    upgraded: bool,
}

/// How a change was made, for its action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A command (its undo label, "Insert Sketch 1", "Edit Extrude 5", …).
    Command(String),
    Undo(String),
    Redo(String),
    /// Restore to an earlier entry (its label).
    Restore(String),
    /// P3E.4: a merge from another workspace (its name).
    Merge(String),
}

impl HistoryLog {
    /// A new log whose Start entry is `doc`.
    pub fn start(doc: &Document, time: Timestamp, user: &str) -> Self {
        let entry = LogEntry {
            time,
            user: user.to_string(),
            element: None,
            element_name: String::new(),
            action: "Start".into(),
            feature: None,
            feature_name: String::new(),
            label: "Start".into(),
            hash: document_hash(doc),
            delta: Delta::default(),
        };
        Self {
            version: HISTORY_VERSION,
            document: doc.id,
            entries: vec![entry],
            snapshots: vec![(0, doc.clone())],
            healthy: Vec::new(),
            versions: Vec::new(),
            current: WorkspaceId::MAIN,
            workspaces: Vec::new(),
            parked: Vec::new(),
            head: Some(doc.clone()),
            upgraded: false,
        }
    }

    /// The number of entries (Start included).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Never true: a log always has its Start entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The index of the last entry (the current state's).
    pub fn head_index(&self) -> usize {
        self.entries.len().saturating_sub(1)
    }

    /// The current state (the last entry's).
    pub fn head(&mut self) -> &Document {
        if self.head.is_none() {
            let last = self.head_index();
            self.head = Some(self.state_at(last).unwrap_or_else(|| Document::empty("")));
        }
        self.head.as_ref().expect("set above")
    }

    /// Appends the change from the current state to `doc`, if there is one. Returns the new
    /// entry's index.
    pub fn record(&mut self, doc: &Document, origin: Origin, time: Timestamp, user: &str) -> Option<usize> {
        let before = self.head().clone();
        let delta = Delta::between(&before, doc);
        if delta.is_empty() {
            return None;
        }
        let what = describe(&before, doc);
        let (action, label) = match &origin {
            Origin::Command(l) => (what.action.clone(), what.label(&what.action, l)),
            // What was undone or redone, by its command's name ("Conrod :: Undo : Insert
            // Sketch 3", "Conrod :: Undo : Restore to …").
            Origin::Undo(l) | Origin::Redo(l) => {
                let action = if matches!(origin, Origin::Undo(_)) { "Undo" } else { "Redo" };
                let label = match (what.element.is_some(), l.is_empty()) {
                    (true, false) => format!("{} :: {action} : {l}", what.element_name),
                    (false, false) => format!("{action} : {l}"),
                    _ => what.label(action, l),
                };
                (action.to_string(), label)
            }
            Origin::Restore(l) => ("Restore".to_string(), format!("Restore : {l}")),
            Origin::Merge(l) => ("Merge".to_string(), format!("Merge from {l}")),
        };
        self.entries.push(LogEntry {
            time,
            user: user.to_string(),
            element: what.element,
            element_name: what.element_name.clone(),
            action,
            feature: what.feature,
            feature_name: what.feature_name.clone(),
            label,
            hash: document_hash(doc),
            delta,
        });
        let i = self.head_index();
        if i.is_multiple_of(SNAPSHOT_EVERY) {
            self.snapshots.push((i, doc.clone()));
        }
        self.head = Some(doc.clone());
        Some(i)
    }

    /// The document as it was after entry `k`: the nearest full copy at or before it with the
    /// deltas after that applied.
    pub fn state_at(&self, k: usize) -> Option<Document> {
        if k >= self.entries.len() {
            return None;
        }
        if k == self.head_index()
            && let Some(h) = &self.head
        {
            return Some(h.clone());
        }
        track_state_at(&self.entries, &self.snapshots, k)
    }

    /// The indices of the entries with a full copy.
    pub fn snapshot_indices(&self) -> Vec<usize> {
        self.snapshots.iter().map(|(i, _)| *i).collect()
    }

    /// Makes a version of the current state: `name` (trimmed; "V<n>" when empty) and an
    /// optional `description`. Returns its id.
    pub fn create_version(&mut self, name: &str, description: &str, time: Timestamp, user: &str) -> VersionId {
        self.create_version_with_id(VersionId::new(), name, description, time, user)
    }

    /// [`Self::create_version`] with a given id (P3G.5: the stand-in fixtures' versions have
    /// fixed ids, so they regenerate exactly).
    pub fn create_version_with_id(&mut self, id: VersionId, name: &str, description: &str, time: Timestamp, user: &str) -> VersionId {
        let name = match name.trim() {
            "" => self.next_version_name(),
            n => n.to_string(),
        };
        self.versions.push(Version {
            id,
            name,
            description: description.trim().to_string(),
            entry: self.head_index(),
            time,
            user: user.to_string(),
            auto: false,
            workspace: self.current,
        });
        id
    }

    /// Makes an **auto version** (P3G.2, ER4.5): as [`Self::create_version`], marked as made by
    /// Update all references. Like every version it can't be removed (ER4.8).
    pub fn create_auto_version(&mut self, description: &str, time: Timestamp, user: &str) -> VersionId {
        let id = self.create_version("", description, time, user);
        if let Some(v) = self.versions.last_mut() {
            v.auto = true;
        }
        id
    }

    /// The default name of the next version: "V1", "V2", …
    pub fn next_version_name(&self) -> String {
        format!("V{}", self.versions.len() + 1)
    }

    /// Every version, oldest first.
    pub fn versions(&self) -> &[Version] {
        &self.versions
    }

    pub fn version(&self, id: VersionId) -> Option<&Version> {
        self.versions.iter().find(|v| v.id == id)
    }

    /// The document as it was at version `id`.
    pub fn document_at_version(&self, id: VersionId) -> Option<Document> {
        let v = self.version(id)?;
        self.workspace_state_at(v.workspace, v.entry)
    }

    /// Notes that `features` of `element` regenerated without error in the current state.
    pub fn note_healthy(&mut self, element: ElementId, features: impl IntoIterator<Item = FeatureId>) {
        let entry = self.head_index();
        for feature in features {
            match self.healthy.iter_mut().find(|m| m.element == element && m.feature == feature) {
                Some(m) => m.entry = entry,
                None => self.healthy.push(HealthyMark { element, feature, entry }),
            }
        }
    }

    /// The entry of `feature`'s last healthy regeneration, if it ever had one.
    pub fn last_healthy(&self, element: ElementId, feature: FeatureId) -> Option<usize> {
        self.healthy
            .iter()
            .find(|m| m.element == element && m.feature == feature)
            .map(|m| m.entry)
            .filter(|e| *e < self.entries.len())
    }

    /// `<store>/<id>/history.ron`.
    pub fn path(store: &Store, id: DocumentId) -> PathBuf {
        store.doc_dir(id).join(HISTORY_FILE)
    }

    /// Writes the log next to its document.
    pub fn save(&self, store: &Store) -> Result<(), StoreError> {
        self.save_path(&Self::path(store, self.document))
    }

    /// Writes the log to `path`.
    pub fn save_path(&self, path: &Path) -> Result<(), StoreError> {
        let text = ron::to_string(self).map_err(|e| StoreError::Parse(e.to_string()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("ron.tmp");
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Reads the log of document `id`, if it has one.
    pub fn load(store: &Store, id: DocumentId) -> Result<Option<Self>, StoreError> {
        let path = Self::path(store, id);
        if !path.is_file() {
            return Ok(None);
        }
        Self::load_path(&path).map(Some)
    }

    /// Reads a log file.
    pub fn load_path(path: &Path) -> Result<Self, StoreError> {
        let text = std::fs::read_to_string(path)?;
        let mut log: Self = ron::from_str(&text).map_err(|e| StoreError::Parse(e.to_string()))?;
        if log.version > HISTORY_VERSION {
            return Err(StoreError::UnsupportedVersion(log.version));
        }
        if log.entries.is_empty() || !log.snapshots.iter().any(|(i, _)| *i == 0) {
            return Err(StoreError::Parse("a history needs its Start entry".into()));
        }
        if log.version < HISTORY_VERSION {
            log.upgrade();
        }
        Ok(log)
    }

    /// True if it was read from an older schema and rewritten ([`Self::upgrade`]): saving it
    /// keeps the new form.
    pub fn upgraded(&self) -> bool {
        self.upgraded
    }

    /// Rewrites a version 1 log's deltas in the current form (Part Studios as
    /// [`StudioPatch`]es). Every new delta must give exactly the state the old one gave, and
    /// the states must match the full copies (a past state is rebuilt from the nearest one);
    /// otherwise the log is left as it was (still readable, only larger).
    fn upgrade(&mut self) {
        let Some(mut before) = self.state_at(0) else { return };
        let mut deltas = Vec::with_capacity(self.entries.len());
        for k in 1..self.entries.len() {
            let mut after = before.clone();
            self.entries[k].delta.apply(&mut after);
            let delta = Delta::between(&before, &after);
            let mut check = before.clone();
            delta.apply(&mut check);
            let snapshot = self.snapshots.iter().find(|(i, _)| *i == k).map(|(_, d)| d);
            if check != after || snapshot.is_some_and(|d| *d != after) {
                return;
            }
            deltas.push(delta);
            before = after;
        }
        for (e, d) in self.entries[1..].iter_mut().zip(deltas) {
            e.delta = d;
        }
        self.version = HISTORY_VERSION;
        self.head = Some(before);
        self.upgraded = true;
    }

    /// Makes the log end at `doc`: opened with a log whose last state isn't the document (it
    /// was changed without one), the difference becomes an entry of its own.
    pub fn catch_up(&mut self, doc: &Document, time: Timestamp, user: &str) {
        self.record(doc, Origin::Command(String::new()), time, user);
    }
}

/// The document as it was after entry `k` of a workspace's changes: the nearest full copy at or
/// before it with the deltas after that applied.
fn track_state_at(entries: &[LogEntry], snapshots: &[(usize, Document)], k: usize) -> Option<Document> {
    if k >= entries.len() {
        return None;
    }
    let (at, snap) = snapshots.iter().filter(|(i, _)| *i <= k).max_by_key(|(i, _)| *i)?;
    let mut doc = snap.clone();
    for e in &entries[at + 1..=k] {
        e.delta.apply(&mut doc);
    }
    Some(doc)
}

/// The tab, feature and action a change is about, worked out from the states either side.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Described {
    pub element: Option<ElementId>,
    pub element_name: String,
    pub feature: Option<FeatureId>,
    pub feature_name: String,
    pub action: String,
}

impl Described {
    /// "Conrod :: Edit : Sketch 2"; with only a tab "Conrod :: Rename", with neither the
    /// command's own label.
    fn label(&self, action: &str, command: &str) -> String {
        match (self.element.is_some(), self.feature.is_some()) {
            (true, true) => format!("{} :: {action} : {}", self.element_name, self.feature_name),
            // A tab's own settings: the command says what ("Conrod :: Rename part to Conrod").
            (true, false) if action == "Edit" && !command.is_empty() => format!("{} :: {command}", self.element_name),
            (true, false) => format!("{} :: {action}", self.element_name),
            _ if !command.is_empty() && action != "Undo" && action != "Redo" => command.to_string(),
            _ => action.to_string(),
        }
    }
}

/// What changed between two states: a tab added, deleted or renamed, or in the first tab
/// whose contents changed, a feature inserted, deleted or edited (in that order of preference;
/// the first in list order), else the tab's own settings.
pub fn describe(before: &Document, after: &Document) -> Described {
    for e in &after.elements {
        if before.elements.iter().all(|b| b.id != e.id) {
            return Described {
                element: Some(e.id),
                element_name: e.name.clone(),
                action: "Insert".into(),
                ..Described::default()
            };
        }
    }
    for b in &before.elements {
        if after.elements.iter().all(|e| e.id != b.id) {
            return Described {
                element: Some(b.id),
                element_name: b.name.clone(),
                action: "Delete".into(),
                ..Described::default()
            };
        }
    }
    for e in &after.elements {
        let Some(b) = before.elements.iter().find(|b| b.id == e.id) else {
            continue;
        };
        if b == e {
            continue;
        }
        let mut d = Described {
            element: Some(e.id),
            element_name: e.name.clone(),
            ..Described::default()
        };
        if b.name != e.name && (Element { name: e.name.clone(), ..b.clone() }) == *e {
            d.action = "Rename".into();
            return d;
        }
        let (fb, fa) = (b.features(), e.features());
        let feature = |f: &crate::document::Feature, action: &str, d: &mut Described| {
            d.feature = Some(f.id);
            d.feature_name = f.name.clone();
            d.action = action.into();
        };
        if let Some(f) = fa.iter().find(|f| fb.iter().all(|x| x.id != f.id)) {
            feature(f, "Insert", &mut d);
        } else if let Some(f) = fb.iter().find(|f| fa.iter().all(|x| x.id != f.id)) {
            feature(f, "Delete", &mut d);
        } else if let Some(f) = fa.iter().find(|f| fb.iter().find(|x| x.id == f.id) != Some(*f)) {
            let renamed = fb.iter().find(|x| x.id == f.id).is_some_and(|x| {
                crate::document::Feature { name: f.name.clone(), ..x.clone() } == *f
            });
            feature(f, if renamed { "Rename" } else { "Edit" }, &mut d);
        } else {
            d.action = "Edit".into();
        }
        return d;
    }
    Described {
        action: "Edit".into(),
        ..Described::default()
    }
}

/// Restore (IR5.6): puts the whole document back as it was at an earlier entry. One undo step.
#[derive(Debug, Clone)]
pub struct RestoreDocument {
    pub state: Box<Document>,
    /// The entry restored to ("Conrod :: Edit : Sketch 2").
    pub entry: String,
}

impl Command for RestoreDocument {
    fn label(&self) -> String {
        format!("Restore to {}", self.entry)
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let id = doc.id;
        *doc = (*self.state).clone();
        doc.id = id;
        Ok(())
    }
}
