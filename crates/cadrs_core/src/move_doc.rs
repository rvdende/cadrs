//! P3G.3: **Move to document** (`derived-and-linking-gaps.md` DV4.1, ER7, ER8; essential tips
//! T1.3; drawings D2.10, X1). Moving tabs out of a document into a new or an existing one:
//!
//! - **Which tabs** ([`referenced_tabs`]): the tabs picked, and the tabs of the same document
//!   they reference at the workspace (an assembly's Part Studios and subassemblies, a drawing's
//!   sources), recursively: "N referenced tabs will be moved" (ER7.3). Each referenced tab can
//!   be left behind; a moved tab that referenced it then references it **back** at a version
//!   of this document, made automatically (an auto version, as Update all makes: ER4.5).
//! - **The target** ([`MoveTarget`]): a new document (named after the first tab by default,
//!   created by the user, so the documents page's Created by me lists it, ER8.5) or an existing
//!   one. Either way the target gets a **version** after the tabs arrive (ER7.2, ER7.5): an
//!   auto version "V1" in a new document, a version the user named in an existing one.
//! - **The source** ([`MoveTabs`]): one undoable command that re-points every use of a moved
//!   tab in the tabs left (an assembly's instances, a drawing's views) at that version of the
//!   target, removes the moved tabs and notes where each went ([`MovedElement`], for ER7.6).
//!   Instances keep their ids, poses, indices and mates; a linked copy holds the moved tab's
//!   contents unchanged, so every instance's **world pose is unchanged** (DV4.1). Uses of a
//!   moved tab at a version of this document (ER3) keep pointing at that version: the Reference
//!   manager offers **Update to the new document** for them ([`moved_record`],
//!   [`change_to_new_document`], ER7.6).
//! - **Order and undo** (the gap list's multi-document risk): everything that can fail is
//!   checked before anything is written; then the source's auto version (only when a tab is left
//!   behind), then the target and its version are written, and only then the source command
//!   runs, through the caller's undo history. Undo puts the tabs back in the source; the target
//!   document and the versions stay (versions are immutable, ER4.8), and the caller says so.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::assembly::InstanceSource;
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind};
use crate::external::{self, LinkError, LinkSnapshot, LinkedElement, RefAt, Resolver, SourceRef};
use crate::history_log::{HistoryLog, Origin, VersionId};
use crate::ids::{DocumentId, ElementId};
use crate::library::{DocumentMeta, Timestamp};
use crate::link_update::{self as lu, Change, RefSite, RefUse, UpdateReferences};
use crate::store::Store;

/// Where a moved tab went (kept in the source document, [`Document::moved`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedElement {
    /// Its id in this document.
    pub element: ElementId,
    /// The document it was moved to, and its id there.
    pub document: DocumentId,
    pub to: ElementId,
    /// The target document's name when it moved (shown in the Reference manager).
    #[serde(default)]
    pub document_name: String,
}

/// Why a move can't be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveError(pub String);

impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MoveError {}

impl From<LinkError> for MoveError {
    fn from(e: LinkError) -> Self {
        MoveError(e.to_string())
    }
}

impl From<CommandError> for MoveError {
    fn from(e: CommandError) -> Self {
        MoveError(e.to_string())
    }
}

/// The tabs of `doc` that `el` references at the workspace (not linked copies or standard
/// content): an assembly's instances' sources, a drawing's sources and views.
pub fn tab_dependencies(doc: &Document, el: &Element) -> Vec<ElementId> {
    let is_tab = |e: ElementId| e != el.id && doc.elements.iter().any(|x| x.id == e);
    let mut out: Vec<ElementId> = Vec::new();
    let mut add = |e: ElementId| {
        if is_tab(e) && !out.contains(&e) {
            out.push(e);
        }
    };
    if let Some(m) = el.assembly_model() {
        for i in m.instances.iter().filter(|i| i.link.is_none()) {
            add(i.source.element());
        }
    }
    // P3F.6: a Render Studio renders its source tab.
    if let crate::document::ElementKind::Render(r) = &el.kind
        && let Some(src) = r.source
    {
        add(src);
    }
    if let Some(d) = el.drawing_data() {
        for s in &d.sources {
            add(ElementId(s.element));
        }
        for sheet in &d.sheets {
            if let Some(r) = &sheet.reference {
                add(ElementId(r.element));
            }
            for v in &sheet.views {
                add(ElementId(v.reference.element));
            }
        }
    }
    out
}

/// The tabs `selected` reference at the workspace, directly or further down, that aren't
/// selected themselves, in tab order ("N referenced tabs will be moved", ER7.3).
pub fn referenced_tabs(doc: &Document, selected: &[ElementId]) -> Vec<ElementId> {
    let mut seen: HashSet<ElementId> = selected.iter().copied().collect();
    let mut stack: Vec<ElementId> = selected.to_vec();
    let mut found: HashSet<ElementId> = HashSet::new();
    while let Some(e) = stack.pop() {
        let Some(el) = doc.elements.iter().find(|x| x.id == e) else { continue };
        for d in tab_dependencies(doc, el) {
            if seen.insert(d) {
                found.insert(d);
                stack.push(d);
            }
        }
    }
    doc.elements.iter().map(|e| e.id).filter(|e| found.contains(e)).collect()
}

/// Where the tabs go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveTarget {
    /// A new document called `name` (the first tab's name when empty).
    New { name: String },
    /// An existing document of the library, which gets a version called `version_name` ("V<n>"
    /// when empty).
    Existing { document: DocumentId, version_name: String },
}

/// Re-points the uses of moved tabs, removes them and notes where they went: the source's one
/// undo step of a move (see the module docs).
#[derive(Debug, Clone)]
pub struct MoveTabs {
    pub changes: Vec<Change>,
    pub remove: Vec<ElementId>,
    pub records: Vec<MovedElement>,
    pub label: String,
}

impl Command for MoveTabs {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.remove.is_empty() || doc.elements.iter().all(|e| self.remove.contains(&e.id)) {
            return Err(CommandError::Invalid("A document keeps at least one tab".into()));
        }
        if !self.changes.is_empty() {
            UpdateReferences { changes: self.changes.clone(), label: self.label.clone() }.apply(doc)?;
        }
        // The copies only the moved tabs used go with them.
        let mut candidates: Vec<ElementId> = Vec::new();
        for el in doc.elements.iter().filter(|e| self.remove.contains(&e.id)) {
            candidates.extend(linked_sources(doc, el));
        }
        doc.elements.retain(|e| !self.remove.contains(&e.id));
        for r in &self.records {
            doc.moved.retain(|m| m.element != r.element);
            doc.moved.push(r.clone());
        }
        lu::prune(doc, &candidates);
        Ok(())
    }
}

/// The linked copies `el` uses directly.
fn linked_sources(doc: &Document, el: &Element) -> Vec<ElementId> {
    let mut out = Vec::new();
    if let Some(m) = el.assembly_model() {
        out.extend(m.instances.iter().map(|i| i.source.element()).filter(|e| doc.linked_element(*e).is_some()));
    }
    if let Some(d) = el.drawing_data() {
        out.extend(d.sources.iter().map(|s| ElementId(s.element)).filter(|e| doc.linked_element(*e).is_some()));
    }
    out
}

/// What a move did.
#[derive(Debug, Clone)]
pub struct MoveOutcome {
    /// The source's command (run it through the undo history).
    pub command: MoveTabs,
    pub target: DocumentId,
    pub target_name: String,
    /// The target was made by the move.
    pub created: bool,
    /// The target's version the source now references.
    pub version: VersionId,
    pub version_name: String,
    /// The source's auto version, when a tab was left behind and is referenced back.
    pub source_version: Option<(VersionId, String)>,
    /// Each moved tab's id here and in the target.
    pub ids: Vec<(ElementId, ElementId)>,
    /// Uses in the target re-pointed back at the source's version.
    pub links_back: usize,
}

impl MoveOutcome {
    /// "Moved 2 tabs to Pneumatic Piston".
    pub fn summary(&self) -> String {
        let n = self.ids.len();
        format!("Moved {n} tab{} to {}", if n == 1 { "" } else { "s" }, self.target_name)
    }
}

/// Points `el`'s references to moved tabs at their ids in the target (`map`: the tabs that got
/// a new id there).
fn remap(el: &mut Element, map: &HashMap<ElementId, ElementId>) {
    if map.is_empty() {
        return;
    }
    let m = |e: ElementId| map.get(&e).copied().unwrap_or(e);
    if let Some(model) = el.assembly_model_mut() {
        for i in model.instances.iter_mut().filter(|i| i.link.is_none()) {
            i.source = match i.source {
                InstanceSource::Part { element, part } => InstanceSource::Part { element: m(element), part },
                InstanceSource::Assembly { element } => InstanceSource::Assembly { element: m(element) },
                InstanceSource::Studio { element } => InstanceSource::Studio { element: m(element) },
            };
        }
    }
    if let ElementKind::Drawing(d) = &mut el.kind {
        let mu = |u: uuid::Uuid| m(ElementId(u)).0;
        for s in &mut d.sources {
            s.element = mu(s.element);
        }
        for sheet in &mut d.sheets {
            if let Some(r) = sheet.reference.as_mut() {
                r.element = mu(r.element);
            }
            for v in &mut sheet.views {
                v.reference.element = mu(v.reference.element);
            }
        }
    }
}

/// True if the store holds document `id`.
fn stored(store: &Store, id: DocumentId) -> bool {
    store.document_path(id).is_file()
}

/// Moves `tabs` of `doc` (the tabs picked plus the referenced tabs chosen to go with them) to
/// `target` (see the module docs). `source_log` is `doc`'s history (started if `None` and a
/// version of it is needed); it is saved when `doc` is in the store. Writes the target (and any
/// versions) at once; returns the command for the source.
pub fn move_tabs(store: &Store, doc: &Document, source_log: &mut Option<HistoryLog>, tabs: &[ElementId], target: &MoveTarget, now: Timestamp, user: &str) -> Result<MoveOutcome, MoveError> {
    // Everything that can fail before anything is written.
    let moved: Vec<ElementId> = doc.elements.iter().map(|e| e.id).filter(|e| tabs.contains(e)).collect();
    if moved.is_empty() {
        return Err(MoveError("No tabs to move".into()));
    }
    if moved.len() == doc.elements.len() {
        return Err(MoveError("A document keeps at least one tab: leave one here".into()));
    }
    let (mut t, meta, old_target) = match target {
        MoveTarget::New { name } => {
            let first = doc.elements.iter().find(|e| e.id == moved[0]).map(|e| e.name.clone()).unwrap_or_default();
            let name = if name.trim().is_empty() { first } else { name.trim().to_string() };
            let mut t = Document::empty(name);
            t.units = doc.units;
            t.properties = doc.properties.clone();
            t.material_libraries = doc.material_libraries.clone();
            (t, DocumentMeta::new(user, now), None)
        }
        MoveTarget::Existing { document, .. } => {
            if *document == doc.id {
                return Err(MoveError("The tabs are in that document already".into()));
            }
            let file = store.load(*document).map_err(|_| MoveError(external::link_state(store, *document).message().unwrap_or("The document can't be read").into()))?;
            if file.meta.trashed.is_some() {
                return Err(MoveError(external::LinkState::Trashed.message().unwrap_or_default().into()));
            }
            let old = file.document.clone();
            (file.document, file.meta, Some(old))
        }
    };
    let left_behind: Vec<ElementId> = {
        let mut out: Vec<ElementId> = Vec::new();
        for e in &moved {
            let el = doc.elements.iter().find(|x| x.id == *e).expect("moved tabs are tabs");
            for d in tab_dependencies(doc, el) {
                if !moved.contains(&d) && !out.contains(&d) {
                    out.push(d);
                }
            }
        }
        out
    };
    // The moved tabs, under new ids where the target has theirs already.
    let taken = |e: ElementId, t: &Document| t.element(e).is_some() || t.linked_element(e).is_some();
    let mut map: HashMap<ElementId, ElementId> = HashMap::new();
    for e in &moved {
        if taken(*e, &t) {
            map.insert(*e, ElementId::new());
        }
    }
    let ids: Vec<(ElementId, ElementId)> = moved.iter().map(|e| (*e, map.get(e).copied().unwrap_or(*e))).collect();
    let new_id = |e: ElementId| map.get(&e).copied().unwrap_or(e);
    // The source's version the target references back (only when a tab stays behind).
    let source_version = if left_behind.is_empty() {
        None
    } else {
        let log = source_log.get_or_insert_with(|| HistoryLog::start(doc, now, user));
        if log.head() != doc {
            log.record(doc, Origin::Command("Move to document".into()), now, user);
        }
        let names: Vec<String> = moved.iter().filter_map(|e| doc.element(*e).map(|x| x.name.clone())).collect();
        let v = log.create_auto_version(&format!("Created by Move to document: {} moved to {}", names.join(", "), t.name), now, user);
        let name = log.version(v).map(|x| x.name().to_string()).unwrap_or_default();
        if stored(store, doc.id) {
            log.save(store).map_err(|e| MoveError(format!("The history of {} ({e})", doc.name)))?;
        }
        Some((v, name))
    };
    // The target's contents: the tabs, the copies and standard content they use.
    for e in &moved {
        let mut el = doc.elements.iter().find(|x| x.id == *e).cloned().expect("moved tabs are tabs");
        el.id = new_id(*e);
        remap(&mut el, &map);
        t.elements.push(el);
    }
    for e in &moved {
        let el = doc.elements.iter().find(|x| x.id == *e).expect("moved tabs are tabs");
        for src in linked_sources(doc, el) {
            for l in lu::closure(doc, src) {
                if t.linked_element(l.id()).is_none() {
                    let mut l: LinkedElement = l.clone();
                    l.source.document = Some(l.source.document_or(doc.id));
                    t.linked.push(l);
                }
            }
        }
        if let Some(m) = el.assembly_model() {
            for i in &m.instances {
                if let Some(sp) = crate::assembly::standard::standard_of(doc, &i.source)
                    && t.standard_part(sp.element.id).is_none()
                {
                    t.standard_content.push(sp.clone());
                }
            }
        }
    }
    // Uses of a tab left behind: a link back at the source's version.
    let mut links_back = 0;
    if let Some((sv, sv_name)) = &source_version {
        let mut snaps: HashMap<ElementId, LinkSnapshot> = HashMap::new();
        let mut changes = Vec::new();
        for (_, to) in &ids {
            let el = t.elements.iter().find(|x| x.id == *to).expect("just added");
            let mut sites: Vec<(RefSite, ElementId)> = Vec::new();
            if let Some(m) = el.assembly_model() {
                for i in m.instances.iter().filter(|i| i.link.is_none() && left_behind.contains(&i.source.element())) {
                    sites.push((RefSite::Instance { element: el.id, instance: i.id }, i.source.element()));
                }
            }
            if let Some(d) = el.drawing_data() {
                for s in d.sources.iter().filter(|s| left_behind.contains(&ElementId(s.element))) {
                    sites.push((RefSite::Drawing { element: el.id, source: ElementId(s.element) }, ElementId(s.element)));
                }
            }
            for (site, src) in sites {
                let r = SourceRef::version(Some(doc.id), src, *sv);
                if let std::collections::hash_map::Entry::Vacant(v) = snaps.entry(src) {
                    v.insert(external::snapshot(doc, r, sv_name)?);
                }
                changes.push(Change { site, to: r, snapshot: snaps.get(&src).cloned() });
            }
        }
        links_back = changes.len();
        if !changes.is_empty() {
            UpdateReferences { changes, label: "Move to document".into() }.apply(&mut t)?;
        }
    }
    // Write the target and its version.
    let tabs_moved: Vec<String> = ids.iter().filter_map(|(_, e)| t.elements.iter().find(|x| x.id == *e).map(|x| x.name.clone())).collect();
    let label = format!("Move {} to {}", if ids.len() == 1 { tabs_moved.first().cloned().unwrap_or_default() } else { format!("{} tabs", ids.len()) }, t.name);
    let created = old_target.is_none();
    let mut log = match &old_target {
        None => {
            store.create(&t, &meta).map_err(|e| MoveError(format!("Cannot create {} ({e})", t.name)))?;
            HistoryLog::start(&t, now, user)
        }
        Some(old) => {
            let mut m = meta.clone();
            m.modified = now;
            m.modified_by = user.to_string();
            let log = HistoryLog::load(store, t.id).ok().flatten();
            store.save(&t, &m).map_err(|e| MoveError(format!("Cannot save {} ({e})", t.name)))?;
            let mut log = log.unwrap_or_else(|| HistoryLog::start(old, meta.created, user));
            log.record(&t, Origin::Command(format!("Moved in from {}: {}", doc.name, tabs_moved.join(", "))), now, user);
            log
        }
    };
    let description = format!("Moved from {}: {}", doc.name, tabs_moved.join(", "));
    let version = match target {
        MoveTarget::New { .. } => log.create_auto_version(&format!("Created by Move to document. {description}"), now, user),
        MoveTarget::Existing { version_name, .. } => log.create_version(version_name, &description, now, user),
    };
    let version_name = log.version(version).map(|v| v.name().to_string()).unwrap_or_default();
    log.save(store).map_err(|e| MoveError(format!("The history of {} ({e})", t.name)))?;
    // The source: its uses of the moved tabs point at that version of the target.
    let mut snaps: HashMap<ElementId, LinkSnapshot> = HashMap::new();
    let mut changes = Vec::new();
    for u in lu::workspace_uses(doc, None) {
        if moved.contains(&u.site.tab()) || !moved.contains(&u.source) {
            continue;
        }
        let to = SourceRef::version(Some(t.id), new_id(u.source), version);
        if let std::collections::hash_map::Entry::Vacant(v) = snaps.entry(u.source) {
            v.insert(external::snapshot(&t, to, &version_name)?);
        }
        changes.push(Change { site: u.site, to, snapshot: snaps.get(&u.source).cloned() });
    }
    let records = ids.iter().map(|(from, to)| MovedElement { element: *from, document: t.id, to: *to, document_name: t.name.clone() }).collect();
    Ok(MoveOutcome {
        command: MoveTabs { changes, remove: moved, records, label },
        target: t.id,
        target_name: t.name.clone(),
        created,
        version,
        version_name,
        source_version,
        ids,
        links_back,
    })
}

/// ER7.6: where the tab `u` references went, if it was moved out of its document (and isn't
/// there any more). `load` reads another document.
pub fn moved_record(doc: &Document, u: &RefUse, load: &mut dyn FnMut(DocumentId) -> Option<std::sync::Arc<Document>>) -> Option<MovedElement> {
    let RefAt::Version(_) = u.reference.at else { return None };
    let d = u.reference.document_or(doc.id);
    let find = |src: &Document| {
        if src.elements.iter().any(|e| e.id == u.reference.element) {
            return None;
        }
        src.moved.iter().find(|m| m.element == u.reference.element).cloned()
    };
    if d == doc.id { find(doc) } else { find(&*load(d)?) }
}

/// ER7.6: "Update to the new document": `u` pointed at the newest version of the document its
/// tab moved to (`m`).
pub fn change_to_new_document(res: &mut Resolver, doc: &Document, log: Option<&HistoryLog>, u: &RefUse, m: &MovedElement) -> Result<Change, LinkError> {
    let this = m.document == doc.id;
    let v = if this { log.and_then(|l| l.versions().last().map(|v| v.id())) } else { res.latest(m.document).map(|v| v.id()) }.ok_or(LinkError::NoVersion)?;
    let to = SourceRef { document: (!this).then_some(m.document), at: RefAt::Version(v), element: m.to, pinned: u.reference.pinned };
    let snapshot = res.resolve(to, doc, log)?;
    Ok(Change { site: u.site, to, snapshot: Some(snapshot) })
}
