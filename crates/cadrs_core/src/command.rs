//! The command and undo framework.
//!
//! A [`Command`] declares the [`Scope`] it edits. [`History::execute`] snapshots that scope before
//! and after applying the command, so undo and redo simply restore a snapshot. Sketches are small,
//! so snapshotting an element is cheap and far more robust than hand-written inverse operations.
//!
//! Snapshots of different scopes commute: a document snapshot restores the document's name and
//! its list of elements, but keeps the current contents of elements that still exist (document
//! commands never edit an existing element's contents), and an element-name snapshot restores
//! only the name. So steps of one scope can be merged or dropped (a sketch dialog's steps, see
//! [`History::squash_element_since`]) without disturbing steps of other scopes in between, such
//! as renaming the document while a sketch is open.

use std::fmt;

use crate::document::{Document, Element};
use crate::ids::ElementId;

/// The part of a document a command edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The whole document: its name and the list of elements (adding, removing, reordering).
    Document,
    /// The contents of one existing element.
    Element(ElementId),
    /// Only the name of one existing element (renaming a tab).
    ElementName(ElementId),
    /// The whole document, the contents of every element included: for the few commands that
    /// edit several elements at once, or add an element and fill it (P3B.4: Move to new
    /// subassembly, dragging instances into or out of a subassembly, Dissolve). Undo restores
    /// the whole document as it was, so such steps must not be reordered (they aren't squashed).
    Whole,
}

/// Why a command could not be applied. A failed command leaves the document unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    ElementNotFound(ElementId),
    Invalid(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::ElementNotFound(id) => write!(f, "element {id} not found"),
            CommandError::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// An edit of a [`Document`]. UI, keyboard shortcuts and scripted scenarios all produce commands.
pub trait Command: fmt::Debug + Send + Sync + 'static {
    /// Shown in the undo/redo menu, e.g. "Rename Part Studio 1".
    fn label(&self) -> String;
    /// The part of the document this command edits.
    fn scope(&self) -> Scope;
    /// Applies the command. It may leave the scope half-edited on error; [`History`] restores it.
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError>;
}

#[derive(Debug, Clone, PartialEq)]
enum Snapshot {
    Document(Box<Document>),
    Whole(Box<Document>),
    Element { index: usize, element: Box<Element> },
    ElementName { id: ElementId, name: String },
}

impl Snapshot {
    fn take(doc: &Document, scope: Scope) -> Result<Self, CommandError> {
        Ok(match scope {
            Scope::Document => Snapshot::Document(Box::new(doc.clone())),
            Scope::Whole => Snapshot::Whole(Box::new(doc.clone())),
            Scope::Element(id) => {
                let index = doc
                    .element_index(id)
                    .ok_or(CommandError::ElementNotFound(id))?;
                Snapshot::Element {
                    index,
                    element: Box::new(doc.elements[index].clone()),
                }
            }
            Scope::ElementName(id) => Snapshot::ElementName {
                id,
                name: doc
                    .element(id)
                    .ok_or(CommandError::ElementNotFound(id))?
                    .name
                    .clone(),
            },
        })
    }

    /// The element whose contents this snapshot holds.
    fn element_id(&self) -> Option<ElementId> {
        match self {
            Snapshot::Element { element, .. } => Some(element.id),
            _ => None,
        }
    }

    fn restore(&self, doc: &mut Document) {
        match self {
            Snapshot::Document(d) => {
                // The name and the list of elements come from the snapshot; elements that exist
                // now keep their current contents (they may have been edited since).
                let mut restored = (**d).clone();
                for e in &mut restored.elements {
                    if let Some(current) = doc.element(e.id) {
                        *e = current.clone();
                    }
                }
                *doc = restored;
            }
            Snapshot::Whole(d) => *doc = (**d).clone(),
            Snapshot::ElementName { id, name } => {
                if let Some(e) = doc.element_mut(*id) {
                    e.name = name.clone();
                }
            }
            Snapshot::Element { index, element } => {
                if let Some(i) = doc.element_index(element.id) {
                    // The name is not part of the contents (see [`Scope::ElementName`]).
                    let name = std::mem::take(&mut doc.elements[i].name);
                    doc.elements[i] = (**element).clone();
                    doc.elements[i].name = name;
                } else {
                    // Should not happen: element-scoped commands cannot add or remove elements.
                    let i = (*index).min(doc.elements.len());
                    doc.elements.insert(i, (**element).clone());
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    before: Snapshot,
    after: Snapshot,
}

/// Undo/redo stacks for one document.
#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(200)
    }
}

impl History {
    /// A history that keeps at most `limit` undo steps.
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Applies `cmd` to `doc` and records it. On error the document is left unchanged and nothing
    /// is recorded.
    pub fn execute(&mut self, doc: &mut Document, cmd: &dyn Command) -> Result<(), CommandError> {
        let scope = cmd.scope();
        let before = Snapshot::take(doc, scope)?;
        if let Err(e) = cmd.apply(doc) {
            before.restore(doc);
            return Err(e);
        }
        // P3G.4: Derived features that follow a tab of this document live.
        crate::derived::refresh(doc);
        let after = Snapshot::take(doc, scope)?;
        if after == before {
            // A no-op (e.g. renaming to the same name) is not worth an undo step.
            return Ok(());
        }
        self.undo.push(Entry {
            label: cmd.label(),
            before,
            after,
        });
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(())
    }

    /// Undoes the last command. Returns its label, or `None` if there is nothing to undo.
    pub fn undo(&mut self, doc: &mut Document) -> Option<String> {
        let entry = self.undo.pop()?;
        entry.before.restore(doc);
        crate::derived::refresh(doc);
        let label = entry.label.clone();
        self.redo.push(entry);
        Some(label)
    }

    /// Redoes the last undone command. Returns its label, or `None` if there is nothing to redo.
    pub fn redo(&mut self, doc: &mut Document) -> Option<String> {
        let entry = self.redo.pop()?;
        entry.after.restore(doc);
        crate::derived::refresh(doc);
        let label = entry.label.clone();
        self.undo.push(entry);
        Some(label)
    }

    /// Number of undo steps.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo steps.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Merges every undo step above the first `mark` steps into one step called `label`, as
    /// a feature dialog does on accept: everything done while it was open undoes at once. The
    /// redo stack is cleared (it holds states from inside the session). Returns false, and
    /// changes nothing, if the steps do not all edit the same scope; with no steps above
    /// `mark` it does nothing and returns true.
    pub fn squash_since(&mut self, mark: usize, label: impl Into<String>) -> bool {
        if self.undo.len() <= mark {
            self.redo.clear();
            return true;
        }
        let steps = &self.undo[mark..];
        let same_scope = |a: &Snapshot, b: &Snapshot| match (a, b) {
            (Snapshot::Document(_), Snapshot::Document(_)) => true,
            (Snapshot::Whole(_), Snapshot::Whole(_)) => true,
            (Snapshot::Element { element: x, .. }, Snapshot::Element { element: y, .. }) => {
                x.id == y.id
            }
            (Snapshot::ElementName { id: x, .. }, Snapshot::ElementName { id: y, .. }) => x == y,
            _ => false,
        };
        if !steps.iter().all(|e| same_scope(&e.before, &steps[0].before)) {
            return false;
        }
        let before = steps[0].before.clone();
        let after = steps[steps.len() - 1].after.clone();
        self.undo.truncate(mark);
        self.redo.clear();
        if before != after {
            self.undo.push(Entry {
                label: label.into(),
                before,
                after,
            });
        }
        true
    }

    /// Merges the undo steps above the first `mark` that edit the contents of `element` into
    /// one step called `label`, placed after the other steps (which are kept as they are). A
    /// sketch dialog's accept does this, so renaming the document or a tab while the dialog is
    /// open stays its own step. Clears the redo stack. Returns the indices of the merged steps
    /// (before merging) and whether the merged step was pushed (it is not when it changes
    /// nothing).
    pub fn squash_element_since(
        &mut self,
        mark: usize,
        element: ElementId,
        label: impl Into<String>,
    ) -> (Vec<usize>, bool) {
        self.redo.clear();
        let idx: Vec<usize> = (mark..self.undo.len())
            .filter(|i| self.undo[*i].before.element_id() == Some(element))
            .collect();
        let (Some(&first), Some(&last)) = (idx.first(), idx.last()) else {
            return (idx, false);
        };
        let before = self.undo[first].before.clone();
        let after = self.undo[last].after.clone();
        for i in idx.iter().rev() {
            self.undo.remove(*i);
        }
        let pushed = before != after;
        if pushed {
            self.undo.push(Entry {
                label: label.into(),
                before,
                after,
            });
        }
        (idx, pushed)
    }

    /// Reverts and forgets the undo steps above the first `mark` that edit the contents of
    /// `element` (a sketch dialog cancelled with nothing worth keeping), keeping every other
    /// step. Clears the redo stack. Returns the indices of the removed steps.
    pub fn discard_element_since(
        &mut self,
        doc: &mut Document,
        mark: usize,
        element: ElementId,
    ) -> Vec<usize> {
        self.redo.clear();
        let idx: Vec<usize> = (mark..self.undo.len())
            .filter(|i| self.undo[*i].before.element_id() == Some(element))
            .collect();
        if let Some(&first) = idx.first() {
            self.undo[first].before.restore(doc);
            crate::derived::refresh(doc);
        }
        for i in idx.iter().rev() {
            self.undo.remove(*i);
        }
        idx
    }

    /// Drops the undo steps above the first `mark` steps without touching the document (the
    /// caller has already restored the state they started from) and clears the redo stack.
    pub fn discard_since(&mut self, mark: usize) {
        self.undo.truncate(mark);
        self.redo.clear();
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use crate::commands::{AddSketch, RenameDocument, RenameElement};
    use crate::ids::FeatureId;

    #[test]
    fn element_steps_merge_and_drop_around_a_rename() {
        let mut doc = Document::new("Doc");
        let mut h = History::default();
        let ps = doc.elements[0].id;
        let f = FeatureId::new();
        h.execute(&mut doc, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(&mut doc, &RenameDocument { name: "Bracket v2".into() }).unwrap();
        h.execute(&mut doc, &RenameElement { id: ps, name: "Plate".into() }).unwrap();
        // Cancelling the sketch drops only its own step.
        let removed = h.discard_element_since(&mut doc, 0, ps);
        assert_eq!(removed, vec![0]);
        assert_eq!(doc.name, "Bracket v2");
        assert_eq!(doc.elements[0].name, "Plate");
        assert!(doc.elements[0].features().is_empty());
        assert_eq!(h.undo_len(), 2);
        // Undoing the renames does not bring the sketch back.
        h.undo(&mut doc);
        h.undo(&mut doc);
        assert_eq!(doc.name, "Doc");
        assert_eq!(doc.elements[0].name, "Part Studio 1");
        assert!(doc.elements[0].features().is_empty());
    }

    #[test]
    fn squashing_keeps_other_steps() {
        let mut doc = Document::new("Doc");
        let mut h = History::default();
        let ps = doc.elements[0].id;
        let f = FeatureId::new();
        h.execute(&mut doc, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(&mut doc, &RenameDocument { name: "B".into() }).unwrap();
        let (idx, pushed) = h.squash_element_since(0, ps, "Insert Sketch 1");
        assert_eq!((idx, pushed), (vec![0], true));
        assert_eq!(h.undo_label(), Some("Insert Sketch 1"));
        h.undo(&mut doc);
        assert_eq!(doc.name, "B");
        assert!(doc.elements[0].features().is_empty());
        h.undo(&mut doc);
        assert_eq!(doc.name, "Doc");
        h.redo(&mut doc);
        h.redo(&mut doc);
        assert_eq!(doc.name, "B");
        assert_eq!(doc.elements[0].features().len(), 1);
    }
}
