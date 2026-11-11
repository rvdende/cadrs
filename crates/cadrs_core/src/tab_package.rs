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
//! - **Features** ([`features_document`], [`paste_features`]): some features of a Part Studio
//!   (and the folders that are wholly among them) travel the same way, as a package whose tab
//!   holds only them; pasted, they go in at the rollback bar of another Part Studio with their
//!   folders, appearances and suppression: one undo step ([`PasteFeatures`]). They keep their
//!   ids, so what they reference among themselves stays; where the Part Studio has an id
//!   already (pasted twice, or where they came from) they get new ones, and the references
//!   follow. A reference to a feature that wasn't copied is left dangling, as when that
//!   feature is deleted.
//! - **The clipboard** ([`to_text`], [`from_text`]): the package as text, so it goes through
//!   the system clipboard from one running cadrs to another.

use std::collections::HashSet;

use base64::Engine;

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind, FeatureKind};
use crate::external::{LinkedElement, RefAt};
use crate::ids::{ElementId, FeatureId};
use crate::link_update as lu;
use crate::move_doc;

/// What a package on the clipboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A tab (with the tabs it references).
    Tab,
    /// Features of a Part Studio ([`features_document`]).
    Features,
}

impl Kind {
    /// What its clipboard text starts with.
    fn prefix(self) -> &'static str {
        match self {
            Kind::Tab => "cadrs-tab:1:",
            Kind::Features => "cadrs-features:1:",
        }
    }
}

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
pub fn to_text(kind: Kind, package: &[u8]) -> String {
    // (The id makes each copy another text, so a paste can tell a new copy from the last.)
    format!(
        "{}{}:{}",
        kind.prefix(),
        uuid::Uuid::new_v4().simple(),
        base64::engine::general_purpose::STANDARD.encode(package)
    )
}

/// What kind of package `text` looks like, if any.
pub fn kind_of(text: &str) -> Option<Kind> {
    let text = text.trim_start();
    [Kind::Tab, Kind::Features]
        .into_iter()
        .find(|k| text.starts_with(k.prefix()))
}

/// The package's bytes of clipboard text; `None` if the text is something else.
pub fn from_text(text: &str) -> Option<(Kind, Vec<u8>)> {
    let kind = kind_of(text)?;
    let (_, data) = text.trim().strip_prefix(kind.prefix())?.split_once(':')?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some((kind, bytes))
}

/// References to a version of the document `from` that left the document out name it now: the
/// element is on its way into another document.
fn absolute(el: &mut Element, from: crate::ids::DocumentId) {
    if let Some(m) = el.assembly_model_mut() {
        for s in m.instances.iter_mut().filter_map(|i| i.link.as_mut()) {
            s.document = Some(s.document_or(from));
        }
    }
    for f in el.features_mut().into_iter().flatten() {
        if let FeatureKind::Derived(d) = &mut f.kind
            && let Some(s) = d.source.as_mut()
            && s.at != RefAt::Workspace
        {
            s.document = Some(s.document_or(from));
        }
    }
}

/// A document holding only the features `features` of the Part Studio `tab` of `doc`, with what
/// they need (see the module docs): its first tab is that Part Studio with nothing else in it.
pub fn features_document(
    doc: &Document,
    tab: ElementId,
    features: &[FeatureId],
) -> Option<Document> {
    let mut trimmed = doc.clone();
    let el = trimmed.element_mut(tab)?;
    el.contexts.clear();
    el.open_context = None;
    el.simulation = Default::default();
    el.named_views.clear();
    let ElementKind::PartStudio {
        features: list,
        parts,
        sketch_visibility,
        appearances,
        curve_appearances,
        folders,
        suppressed,
        rollback,
        part_folders,
    } = &mut el.kind
    else {
        return None;
    };
    list.retain(|f| features.contains(&f.id));
    if list.is_empty() {
        return None;
    }
    let kept: HashSet<FeatureId> = list.iter().map(|f| f.id).collect();
    sketch_visibility.retain(|(f, _)| kept.contains(f));
    appearances.retain(|(f, _)| kept.contains(f));
    curve_appearances.retain(|(f, ..)| kept.contains(f));
    suppressed.retain(|f| kept.contains(f));
    // A folder comes along when all its features do.
    folders.retain(|f| !f.features.is_empty() && f.features.iter().all(|x| kept.contains(x)));
    parts.clear();
    part_folders.clear();
    *rollback = None;
    tab_document(&trimmed, tab)
}

/// The features of a features package: its first tab's.
pub fn package_features(package: &Document) -> &[crate::document::Feature] {
    package.elements.first().map_or(&[], |e| e.features())
}

/// Adds a package's features to a Part Studio at its rollback bar, with their folders,
/// appearances and suppression, and the copies their Derived features use: one undo step
/// ([`paste_features`] makes it).
#[derive(Debug, Clone)]
pub struct PasteFeatures {
    pub element: ElementId,
    /// A Part Studio holding only what is added.
    pub fragment: Box<ElementKind>,
    pub linked: Vec<LinkedElement>,
    pub label: String,
}

impl PasteFeatures {
    /// The features it adds.
    pub fn features(&self) -> &[crate::document::Feature] {
        match &*self.fragment {
            ElementKind::PartStudio { features, .. } => features,
            _ => &[],
        }
    }
}

impl Command for PasteFeatures {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let ElementKind::PartStudio {
            features: new,
            sketch_visibility: new_visibility,
            appearances: new_appearances,
            curve_appearances: new_curves,
            folders: new_folders,
            suppressed: new_suppressed,
            ..
        } = &*self.fragment
        else {
            return Err(CommandError::Invalid("features need a Part Studio".into()));
        };
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let at = el.rollback_index();
        let ElementKind::PartStudio {
            features,
            sketch_visibility,
            appearances,
            curve_appearances,
            folders,
            suppressed,
            rollback,
            ..
        } = &mut el.kind
        else {
            return Err(CommandError::Invalid("features need a Part Studio".into()));
        };
        if new.is_empty() {
            return Err(CommandError::Invalid("no features to paste".into()));
        }
        let taken = |id: FeatureId| {
            features.iter().any(|f| f.id == id) || folders.iter().any(|f| f.id == id)
        };
        if new.iter().any(|f| taken(f.id)) || new_folders.iter().any(|f| taken(f.id)) {
            return Err(CommandError::Invalid("feature id already in use".into()));
        }
        for (k, f) in new.iter().enumerate() {
            features.insert(at + k, f.clone());
        }
        if let Some(r) = rollback {
            *r = at + new.len();
        }
        sketch_visibility.extend(new_visibility.iter().cloned());
        appearances.extend(new_appearances.iter().cloned());
        curve_appearances.extend(new_curves.iter().cloned());
        folders.extend(new_folders.iter().cloned());
        suppressed.extend(new_suppressed.iter().copied());
        for l in &self.linked {
            if doc.linked_element(l.id()).is_none() {
                doc.linked.push(l.clone());
            }
        }
        crate::commands::refresh_studio(doc, self.element);
        Ok(())
    }
}

/// The paste of a features package into the Part Studio `element` of `target` (see the module
/// docs).
pub fn paste_features(
    target: &Document,
    element: ElementId,
    package: &Document,
) -> Result<PasteFeatures, String> {
    let to = target
        .elements
        .iter()
        .find(|e| e.id == element)
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .ok_or("Features are pasted into a Part Studio")?;
    let mut from = package
        .elements
        .first()
        .filter(|e| !e.features().is_empty())
        .cloned()
        .ok_or("No features to paste")?;
    if package.id != target.id {
        absolute(&mut from, package.id);
    }
    // New ids where the Part Studio has theirs: as text, so every reference to them follows.
    let taken: Vec<FeatureId> = from
        .features()
        .iter()
        .map(|f| f.id)
        .chain(from.folders().iter().map(|f| f.id))
        .filter(|id| to.feature(*id).is_some() || to.folders().iter().any(|f| f.id == *id))
        .collect();
    let mut fragment = from.kind;
    if !taken.is_empty() {
        let mut text = ron::to_string(&fragment).map_err(|e| e.to_string())?;
        for id in taken {
            text = text.replace(&id.0.to_string(), &FeatureId::new().0.to_string());
        }
        fragment = ron::from_str(&text).map_err(|e| e.to_string())?;
    }
    let names: Vec<&str> = match &fragment {
        ElementKind::PartStudio { features, .. } => {
            features.iter().map(|f| f.name.as_str()).collect()
        }
        _ => Vec::new(),
    };
    let label = match names.as_slice() {
        [one] => format!("Paste {one}"),
        many => format!("Paste {} features", many.len()),
    };
    Ok(PasteFeatures {
        element,
        linked: package
            .linked
            .iter()
            .filter(|l| target.linked_element(l.id()).is_none())
            .map(|l| {
                let mut l = l.clone();
                l.source.document = Some(l.source.document_or(package.id));
                l
            })
            .collect(),
        fragment: Box::new(fragment),
        label,
    })
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
        if package.id != target.id {
            absolute(&mut el, package.id);
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

        let text = to_text(Kind::Tab, b"zip bytes");
        assert_eq!(kind_of(&text), Some(Kind::Tab));
        assert_eq!(from_text(&text), Some((Kind::Tab, b"zip bytes".to_vec())));
        assert_eq!(
            kind_of(&to_text(Kind::Features, b"zip bytes")),
            Some(Kind::Features)
        );
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

    /// The bracket's first two features (a sketch and the extrude of it) go into another Part
    /// Studio and build there; pasted again they get new ids, the extrude's sketch with them.
    #[test]
    fn features_are_pasted_into_another_part_studio() {
        let mut src = doc_with(&["Bracket"]);
        let tab = src.elements[0].id;
        let features = crate::samples::bracket_pair::bracket();
        let picked: Vec<FeatureId> = features.iter().take(2).map(|f| f.id).collect();
        *src.elements[0].features_mut().unwrap() = features;
        let package = features_document(&src, tab, &picked).unwrap();
        assert_eq!(package_features(&package).len(), 2);

        let mut target = doc_with(&["Other"]);
        let to = target.elements[0].id;
        let mut h = History::default();
        let first = paste_features(&target, to, &package).unwrap();
        h.execute(&mut target, &first).unwrap();
        let ids = |d: &Document| -> Vec<FeatureId> {
            d.elements[0].features().iter().map(|f| f.id).collect()
        };
        assert_eq!(ids(&target), picked);

        let again = paste_features(&target, to, &package).unwrap();
        h.execute(&mut target, &again).unwrap();
        let all = ids(&target);
        assert_eq!(all.len(), 4);
        assert!(!picked.contains(&all[2]) && !picked.contains(&all[3]));
        // The copies differ from the first two only by their ids: the references followed.
        let text = |f: &crate::document::Feature| ron::to_string(&f.kind).unwrap();
        let list = target.elements[0].features();
        let renamed = text(&list[1])
            .replace(&picked[0].0.to_string(), &all[2].0.to_string())
            .replace(&picked[1].0.to_string(), &all[3].0.to_string());
        assert_eq!(renamed, text(&list[3]));
        h.undo(&mut target).unwrap();
        assert_eq!(ids(&target), picked);
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
