//! P3E.4 (TD12.4–TD12.7): the history's workspaces, Main and the branches (see the parent
//! module's docs). Kept apart from the log itself so the shared `history_log.rs` changes little.

use serde::{Deserialize, Serialize};

use super::{Delta, HealthyMark, HistoryLog, LogEntry, Version, VersionId, document_hash, track_state_at};
use crate::document::Document;
use crate::library::Timestamp;

/// A workspace's id (P3E.4). Main's is [`WorkspaceId::MAIN`] (the nil id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct WorkspaceId(pub uuid::Uuid);

impl WorkspaceId {
    pub const MAIN: Self = Self(uuid::Uuid::nil());

    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    pub fn is_main(&self) -> bool {
        *self == Self::MAIN
    }
}

/// The name of the Main workspace.
pub const MAIN_NAME: &str = "Main";

/// A branch (P3E.4, TD12.4): a workspace made from a version, with a name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    id: WorkspaceId,
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    /// The version it was made from.
    from_version: VersionId,
    time: Timestamp,
    user: String,
}

impl Workspace {
    pub fn id(&self) -> WorkspaceId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn from_version(&self) -> VersionId {
        self.from_version
    }
    pub fn time(&self) -> Timestamp {
        self.time
    }
    pub fn user(&self) -> &str {
        &self.user
    }
}

/// A workspace's changes while another workspace is current (see [`HistoryLog::switch_to`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Track {
    pub(super) workspace: WorkspaceId,
    pub(super) entries: Vec<LogEntry>,
    pub(super) snapshots: Vec<(usize, Document)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) healthy: Vec<HealthyMark>,
}

/// P3E.4: workspaces.
impl HistoryLog {
    /// The workspace whose changes the log's entries are (the document file's state).
    pub fn current_workspace(&self) -> WorkspaceId {
        self.current
    }

    /// The branches, oldest first (Main is not one).
    pub fn branches(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Every workspace: Main, then the branches oldest first.
    pub fn workspace_ids(&self) -> Vec<WorkspaceId> {
        std::iter::once(WorkspaceId::MAIN).chain(self.workspaces.iter().map(|w| w.id)).collect()
    }

    /// A branch by id (`None` for Main).
    pub fn branch_info(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// "Main" or the branch's name.
    pub fn workspace_name(&self, id: WorkspaceId) -> String {
        if id.is_main() {
            return MAIN_NAME.to_string();
        }
        self.branch_info(id).map(|w| w.name.clone()).unwrap_or_default()
    }

    /// The current workspace's name.
    pub fn current_name(&self) -> String {
        self.workspace_name(self.current)
    }

    /// A workspace's changes (the current one's are [`Self::entries`]).
    pub fn workspace_entries(&self, id: WorkspaceId) -> Option<&[LogEntry]> {
        if id == self.current {
            return Some(&self.entries);
        }
        self.parked.iter().find(|t| t.workspace == id).map(|t| t.entries.as_slice())
    }

    /// A workspace's state after its entry `k`.
    pub fn workspace_state_at(&self, id: WorkspaceId, k: usize) -> Option<Document> {
        if id == self.current {
            return self.state_at(k);
        }
        let t = self.parked.iter().find(|t| t.workspace == id)?;
        track_state_at(&t.entries, &t.snapshots, k)
    }

    /// A workspace's current state.
    pub fn workspace_head(&self, id: WorkspaceId) -> Option<Document> {
        let n = self.workspace_entries(id)?.len();
        self.workspace_state_at(id, n.checked_sub(1)?)
    }

    /// The versions made in workspace `id`, by their index in [`Self::versions`].
    pub fn versions_in(&self, id: WorkspaceId) -> impl Iterator<Item = (usize, &Version)> {
        self.versions.iter().enumerate().filter(move |(_, v)| v.workspace == id)
    }

    /// The default name of the next branch: "Branch 1", "Branch 2", …
    pub fn next_branch_name(&self) -> String {
        format!("Branch {}", self.workspaces.len() + 1)
    }

    /// **Branch to create workspace** (TD12.4): a new workspace `name` (trimmed; the next
    /// "Branch n" when empty) whose start is version `from`, exactly. It isn't switched to.
    pub fn branch(&mut self, from: VersionId, name: &str, description: &str, time: Timestamp, user: &str) -> Option<WorkspaceId> {
        self.branch_with_id(WorkspaceId::new(), from, name, description, time, user)
    }

    /// [`Self::branch`] with a given id.
    pub fn branch_with_id(&mut self, id: WorkspaceId, from: VersionId, name: &str, description: &str, time: Timestamp, user: &str) -> Option<WorkspaceId> {
        let version = self.version(from)?.name.clone();
        let doc = self.document_at_version(from)?;
        let name = match name.trim() {
            "" => self.next_branch_name(),
            n => n.to_string(),
        };
        let start = LogEntry {
            time,
            user: user.to_string(),
            element: None,
            element_name: String::new(),
            action: "Start".into(),
            feature: None,
            feature_name: String::new(),
            label: format!("Branch from {version}"),
            hash: document_hash(&doc),
            delta: Delta::default(),
        };
        self.workspaces.push(Workspace { id, name, description: description.trim().to_string(), from_version: from, time, user: user.to_string() });
        self.parked.push(Track { workspace: id, entries: vec![start], snapshots: vec![(0, doc)], healthy: Vec::new() });
        self.sort_parked();
        Some(id)
    }

    /// Makes workspace `id` the current one: its changes become the log's entries (and the
    /// current one's are kept aside). Returns its state, which the document should now hold.
    pub fn switch_to(&mut self, id: WorkspaceId) -> Option<Document> {
        if id != self.current {
            let i = self.parked.iter().position(|t| t.workspace == id)?;
            let next = self.parked.swap_remove(i);
            let last = self.head_index();
            let head = self.head.take().or_else(|| self.state_at(last));
            let old = Track {
                workspace: self.current,
                entries: std::mem::replace(&mut self.entries, next.entries),
                snapshots: std::mem::replace(&mut self.snapshots, next.snapshots),
                healthy: std::mem::replace(&mut self.healthy, next.healthy),
            };
            // A parked track rebuilds its head from its full copies; one is kept at its last
            // entry so that stays quick.
            let mut old = old;
            if let Some(h) = head {
                let n = old.entries.len() - 1;
                if !old.snapshots.iter().any(|(i, _)| *i == n) {
                    old.snapshots.push((n, h));
                }
            }
            self.parked.push(old);
            self.sort_parked();
            self.current = id;
            self.head = None;
        }
        Some(self.head().clone())
    }

    /// The state both workspaces started from, when one is a branch: the version the source
    /// (else the destination) was made from. For a merge's changed tabs.
    pub fn merge_base(&self, source: WorkspaceId, dest: WorkspaceId) -> Option<Document> {
        let from = self.branch_info(source).or_else(|| self.branch_info(dest))?.from_version;
        self.document_at_version(from)
    }

    fn sort_parked(&mut self) {
        let order = self.workspace_ids();
        self.parked.sort_by_key(|t| order.iter().position(|w| *w == t.workspace).unwrap_or(usize::MAX));
    }
}

