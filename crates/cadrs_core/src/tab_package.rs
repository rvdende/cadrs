//! A tab on its way to another document: copied to the clipboard, or exported as a `.cadrs`
//! file and imported.
//!
//! - **The package** ([`tab_document`]): a document holding the tab with its features, the tabs
//!   of its document it references at the workspace (a Derived feature's Part Studio, an
//!   assembly's Part Studios and subassemblies, a drawing's sources), the linked copies and
//!   standard content those use and the document's material libraries. It is an ordinary
//!   document, so it travels as a `.cadrs` package ([`crate::native::pack`]) and a whole
//!   document's `.cadrs` file pastes the same way.
//! - **Pasting** ([`paste`]): one undoable command ([`PasteTabs`]). A tab nothing else in the
//!   package references (the tab copied) always arrives, under a new id and the next free
//!   "(n)" name where the document has its id or name already. A tab the others reference is
//!   added only if the document doesn't hold it yet (pasted before, or the document the copy
//!   came from), so the references into it stay as they are.
//! - **The clipboard** ([`to_text`], [`from_text`]): the package as text, so it goes through
//!   the system clipboard from one running cadrs to another.

use std::collections::HashSet;

use base64::Engine;

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, FeatureKind};
use crate::external::{LinkedElement, RefAt};
use crate::ids::ElementId;
use crate::link_update as lu;
use crate::move_doc;

/// What a tab on the clipboard starts with.
const PREFIX: &str = "cadrs-tab:1:";

/// The tabs of `doc` that `el` references at the workspace.
pub fn references(doc: &Document, el: &Element) -> Vec<ElementId> {
    let mut out = move_doc::tab_dependencies(doc, el);
    for u in lu::workspace_uses(doc, Some(el.id)) {
        if u.source != el.id && !out.contains(&u.source) {
            out.push(u.source);
        }
    }
    out
}

/// The tabs of `doc` nothing else in it references: the ones a package is about.
pub fn roots(doc: &Document) -> Vec<&Element> {
    let used: HashSet<ElementId> = doc
        .elements
        .iter()
        .flat_map(|e| references(doc, e))
        .collect();
    doc.elements
        .iter()
        .filter(|e| !used.contains(&e.id))
        .collect()
}

/// A document holding only the tab `tab` of `doc` and what it needs (see the module docs). It
/// keeps `doc`'s id and name: where the tab came from.
pub fn tab_document(doc: &Document, tab: ElementId) -> Option<Document> {
    doc.elements.iter().find(|e| e.id == tab)?;
    let mut keep: HashSet<ElementId> = HashSet::from([tab]);
    let mut stack = vec![tab];
    while let Some(e) = stack.pop() {
        let Some(el) = doc.elements.iter().find(|x| x.id == e) else {
            continue;
        };
        for d in references(doc, el) {
            if keep.insert(d) {
                stack.push(d);
            }
        }
    }
    let mut out = doc.clone();
    out.elements.retain(|e| keep.contains(&e.id));
    // The tab copied first: it is the one named and opened.
    out.elements.sort_by_key(|e| e.id != tab);
    out.tab_tree = Default::default();
    out.moved.clear();
    let linked: Vec<ElementId> = out.linked.iter().map(|l| l.id()).collect();
    lu::prune(&mut out, &linked);
    let standard: HashSet<ElementId> = out
        .elements
        .iter()
        .chain(out.linked.iter().map(|l| &l.element))
        .filter_map(|e| e.assembly_model())
        .flat_map(|m| m.instances.iter().map(|i| i.source.element()))
        .collect();
    out.standard_content
        .retain(|s| standard.contains(&s.element.id));
    Some(out)
}

/// A package's bytes as clipboard text.
pub fn to_text(package: &[u8]) -> String {
    // (The id makes each copy another text, so a paste can tell a new copy from the last.)
    format!(
        "{PREFIX}{}:{}",
        uuid::Uuid::new_v4().simple(),
        base64::engine::general_purpose::STANDARD.encode(package)
    )
}

/// True if `text` looks like a tab on the clipboard.
pub fn is_text(text: &str) -> bool {
    text.trim_start().starts_with(PREFIX)
}

/// The package's bytes of clipboard text; `None` if the text is something else.
pub fn from_text(text: &str) -> Option<Vec<u8>> {
    let (_, data) = text.trim().strip_prefix(PREFIX)?.split_once(':')?;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// Adds a package's tabs, and the copies, standard content and material libraries they use: one
/// undo step ([`paste`] makes it).
#[derive(Debug, Clone)]
pub struct PasteTabs {
    /// The new tabs, in order, put right of `after` (at the end without one).
    pub elements: Vec<Element>,
    pub linked: Vec<LinkedElement>,
    pub standard: Vec<crate::assembly::standard::StandardPart>,
    pub material_libraries: Vec<crate::material::MaterialLibrary>,
    pub after: Option<ElementId>,
    pub label: String,
}

impl Command for PasteTabs {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.elements.is_empty() {
            return Err(CommandError::Invalid("no tabs to paste".into()));
        }
        if self.elements.iter().any(|e| doc.element(e.id).is_some()) {
            return Err(CommandError::Invalid("element id already in use".into()));
        }
        let at = self
            .after
            .and_then(|a| doc.element_index(a))
            .map_or(doc.elements.len(), |i| i + 1);
        for (k, el) in self.elements.iter().enumerate() {
            doc.elements.insert(at + k, el.clone());
        }
        for l in &self.linked {
            if doc.linked_element(l.id()).is_none() {
                doc.linked.push(l.clone());
            }
        }
        for s in &self.standard {
            if doc.standard_part(s.element.id).is_none() {
                doc.standard_content.push(s.clone());
            }
        }
        for m in &self.material_libraries {
            if doc.material_libraries.iter().all(|x| x.name != m.name) {
                doc.material_libraries.push(m.clone());
            }
        }
        Ok(())
    }
}

/// A paste into a document: its command, and the tab to open afterwards.
#[derive(Debug, Clone)]
pub struct Paste {
    pub command: PasteTabs,
    pub open: ElementId,
}

/// The paste of `package` into `target`, right of `after` (see the module docs); `None` if the
/// package has no tabs.
pub fn paste(target: &Document, package: &Document, after: Option<ElementId>) -> Option<Paste> {
    let roots: HashSet<ElementId> = roots(package).iter().map(|e| e.id).collect();
    let mut names: HashSet<String> = target.elements.iter().map(|e| e.name.clone()).collect();
    let mut elements = Vec::new();
    let mut open = None;
    for el in &package.elements {
        let taken = target.element(el.id).is_some();
        let root = roots.contains(&el.id);
        if taken && !root {
            continue;
        }
        let mut el = el.clone();
        if taken {
            el.id = ElementId::new();
        }
        let base = el.name.clone();
        let mut n = 1;
        while names.contains(&el.name) {
            el.name = format!("{base} ({n})");
            n += 1;
        }
        names.insert(el.name.clone());
        // References to a version of the document it came from name that document now.
        if package.id != target.id {
            if let Some(m) = el.assembly_model_mut() {
                for s in m.instances.iter_mut().filter_map(|i| i.link.as_mut()) {
                    s.document = Some(s.document_or(package.id));
                }
            }
            for f in el.features_mut().into_iter().flatten() {
                if let FeatureKind::Derived(d) = &mut f.kind
                    && let Some(s) = d.source.as_mut()
                    && s.at != RefAt::Workspace
                {
                    s.document = Some(s.document_or(package.id));
                }
            }
        }
        if root && open.is_none() {
            open = Some(el.id);
        }
        elements.push(el);
    }
    let open = open.or(elements.first().map(|e| e.id))?;
    let linked = package
        .linked
        .iter()
        .filter(|l| target.linked_element(l.id()).is_none())
        .map(|l| {
            let mut l = l.clone();
            l.source.document = Some(l.source.document_or(package.id));
            l
        })
        .collect();
    let label = match elements.as_slice() {
        [one] => format!("Paste {}", one.name),
        many => format!("Paste {} tabs", many.len()),
    };
    Some(Paste {
        command: PasteTabs {
            elements,
            linked,
            standard: package
                .standard_content
                .iter()
                .filter(|s| target.standard_part(s.element.id).is_none())
                .cloned()
                .collect(),
            material_libraries: package.material_libraries.clone(),
            after,
            label,
        },
        open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::History;
    use crate::commands::{AddElement, NewElementKind};

    fn doc_with(names: &[&str]) -> Document {
        let mut doc = Document::empty("Source");
        let mut h = History::default();
        for n in names {
            h.execute(
                &mut doc,
                &AddElement {
                    id: ElementId::new(),
                    kind: NewElementKind::PartStudio,
                    name: Some(n.to_string()),
                    after: None,
                },
            )
            .unwrap();
        }
        doc
    }

    #[test]
    fn a_tab_goes_through_text_into_another_document() {
        let src = doc_with(&["Bracket", "Other"]);
        let tab = src.elements[0].id;
        let package = tab_document(&src, tab).unwrap();
        assert_eq!(package.elements.len(), 1);
        assert_eq!(package.name, "Source");

        let text = to_text(b"zip bytes");
        assert!(is_text(&text));
        assert_eq!(from_text(&text).as_deref(), Some(&b"zip bytes"[..]));
        assert_eq!(from_text("Bracket"), None);

        let mut target = doc_with(&["Bracket"]);
        let after = target.elements[0].id;
        let p = paste(&target, &package, Some(after)).unwrap();
        let mut h = History::default();
        h.execute(&mut target, &p.command).unwrap();
        assert_eq!(target.elements.len(), 2);
        assert_eq!(target.elements[1].id, p.open);
        // Its own id (the target didn't have it), and the next free name.
        assert_eq!(p.open, tab);
        assert_eq!(target.elements[1].name, "Bracket (1)");
        h.undo(&mut target).unwrap();
        assert_eq!(target.elements.len(), 1);
    }

    #[test]
    fn pasted_where_it_came_from_it_is_a_copy() {
        let mut src = doc_with(&["Bracket"]);
        let tab = src.elements[0].id;
        let package = tab_document(&src, tab).unwrap();
        let p = paste(&src, &package, Some(tab)).unwrap();
        assert_ne!(p.open, tab);
        History::default().execute(&mut src, &p.command).unwrap();
        assert_eq!(src.elements.len(), 2);
        assert_eq!(src.elements[1].name, "Bracket (1)");
    }
}
