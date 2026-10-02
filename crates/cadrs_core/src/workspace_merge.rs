//! P3E.4 (TD12.6, TD12.7): merging one workspace into another, tab by tab.
//!
//! - [`changed_tabs`]: the tabs the source workspace changed (since the version the branch
//!   started from, when there is one) and that differ from the destination: changed, added or
//!   deleted in the source. The merge dialog lists them.
//! - [`merge`]: the destination with each chosen tab **replaced** by the source's (an added tab
//!   is added, a deleted one removed); every other tab is **kept**. Branches start as exact
//!   copies of a version, so element, feature and part ids are the same in both workspaces and
//!   references from kept tabs to replaced ones (an assembly instance of a replaced Part
//!   Studio) resolve as before. The linked copies and standard content a replaced tab uses
//!   come with it.
//! - [`MergeWorkspace`]: the command that applies a merge, as one history entry and one undo
//!   step (Restore to the entry before it undoes it, TD12.7).

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element};
use crate::ids::ElementId;

/// How a tab differs in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabChange {
    Changed,
    Added,
    Deleted,
}

/// A tab the merge can replace.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedTab {
    pub element: ElementId,
    /// Its name in the source (in the destination when the source deleted it).
    pub name: String,
    pub change: TabChange,
    /// True for an assembly (for its icon).
    pub assembly: bool,
}

/// The tabs `source` changed (since `base`, when known) that differ from `dest`, in the
/// source's order, then the ones it deleted.
pub fn changed_tabs(base: Option<&Document>, source: &Document, dest: &Document) -> Vec<ChangedTab> {
    let mut out = Vec::new();
    let changed_in_source = |id: ElementId, now: Option<&Element>| match base {
        Some(b) => b.elements.iter().find(|e| e.id == id) != now,
        None => true,
    };
    for e in &source.elements {
        let d = dest.elements.iter().find(|x| x.id == e.id);
        if d == Some(e) || !changed_in_source(e.id, Some(e)) {
            continue;
        }
        out.push(ChangedTab { element: e.id, name: e.name.clone(), change: if d.is_some() { TabChange::Changed } else { TabChange::Added }, assembly: e.assembly_model().is_some() });
    }
    for d in &dest.elements {
        if source.elements.iter().any(|e| e.id == d.id) || !changed_in_source(d.id, None) {
            continue;
        }
        out.push(ChangedTab { element: d.id, name: d.name.clone(), change: TabChange::Deleted, assembly: d.assembly_model().is_some() });
    }
    out
}

/// `dest` with the tabs `replace` taken from `source` (see the module docs).
pub fn merge(dest: &Document, source: &Document, replace: &[ElementId]) -> Document {
    let mut out = dest.clone();
    for &id in replace {
        match source.elements.iter().position(|e| e.id == id) {
            Some(si) => {
                let e = source.elements[si].clone();
                if let Some(slot) = out.elements.iter_mut().find(|x| x.id == id) {
                    *slot = e;
                } else {
                    // After the nearest tab before it in the source that is here.
                    let at = source.elements[..si].iter().rev().find_map(|p| out.element_index(p.id)).map_or(0, |i| i + 1);
                    out.elements.insert(at, e);
                }
            }
            None => out.elements.retain(|e| e.id != id),
        }
    }
    if !replace.is_empty() {
        for l in &source.linked {
            if !out.linked.iter().any(|x| x.id() == l.id()) {
                out.linked.push(l.clone());
            }
        }
        for s in &source.standard_content {
            if !out.standard_content.iter().any(|x| x.element.id == s.element.id) {
                out.standard_content.push(s.clone());
            }
        }
    }
    out
}

/// **Merge** (TD12.6): puts the merged document in place, one undo step and one history entry
/// ("Merge from <source>").
#[derive(Debug, Clone)]
pub struct MergeWorkspace {
    pub state: Box<Document>,
    /// The source workspace's name.
    pub source: String,
}

/// The start of a merge's undo label (the history panel tells merges by it).
pub const MERGE_LABEL: &str = "Merge from ";

impl Command for MergeWorkspace {
    fn label(&self) -> String {
        format!("{MERGE_LABEL}{}", self.source)
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
