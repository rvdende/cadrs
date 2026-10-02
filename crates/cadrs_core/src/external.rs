//! P3G.1: references between documents and to versions (`derived-and-linking-gaps.md` DV1, DV2,
//! DV X1, DV X2; `external-references.md` ER1–ER3, ER X; also T3, T7, TD8.1).
//!
//! - **The reference.** A [`SourceRef`] names a document (`None`: this one), a point in its
//!   history ([`RefAt`]: the live **workspace**, or a named **version**), an element (tab) of
//!   it, and whether it is **pinned** (ER5, used from P3G.2). A reference to another document
//!   always names a version (DV1.1, ER1.5): its workspace never reaches a consumer.
//! - **Frozen copies.** A consumer keeps what a reference points at inside itself: the
//!   referenced element *and every element it depends on* (a subassembly's studios, its own
//!   links), each a [`LinkedElement`] in [`Document::linked`]. So a consumer never depends on
//!   another document's files to open or rebuild (ER1.10, DV1.7: the source trashed or purged,
//!   the geometry stays), and a version link can't change (ER2.1).
//! - **Namespaced ids.** A copy lives under an id of its own ([`linked_id`]: a hash of the source
//!   document, the source element and the copy's contents), so a copy of a document or a
//!   duplicated tab (which keep their uuids) never collides with a local element, and two
//!   references to the same contents share one copy (deduplicated by content). Everything inside
//!   a copy keeps the source's ids and persistent names, so mates re-resolve after an update.
//!   An instance of a linked part is an ordinary instance whose source is the copy's id; its
//!   [`crate::assembly::Instance::link`] holds the reference.
//! - **Rebuilds.** A copy is an ordinary element to the rebuild: its features are built in the
//!   consumer's own kernel session (bodies never move between sessions), and the rebuild cache
//!   is keyed by those features, so a copy's key changes exactly when an update brings other
//!   contents (and the copy then has another id as well).
//! - **The resolver** ([`Resolver`]) reads another document's history (its versions) from the
//!   store, once per file change, to make or update a link, and says whether the source can be
//!   reached ([`LinkState`]). Opening or rebuilding a consumer never needs it.
//! - **Cycles** are refused ([`check_cycle`], DV1.5): a link whose copies reach the consumer's
//!   own element, at any version.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::assembly::{Instance, InstanceSource};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind};
use crate::history_log::{HistoryLog, Version, VersionId};
use crate::ids::{DocumentId, ElementId};
use crate::library::Timestamp;
use crate::store::Store;

/// Where in a document's history a reference points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RefAt {
    /// The live workspace (same-document references only).
    Workspace,
    /// A named, immutable version.
    Version(VersionId),
}

/// A reference to an element of a document at the workspace or a version (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceRef {
    /// The source document; `None`: the document the reference is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentId>,
    pub at: RefAt,
    /// The source element (its id in the source document).
    pub element: ElementId,
    /// Pinned (ER5): update-all skips it. Only version references can be pinned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl SourceRef {
    /// A reference to `element` of `document` at `version`.
    pub fn version(document: Option<DocumentId>, element: ElementId, version: VersionId) -> Self {
        Self { document, at: RefAt::Version(version), element, pinned: false }
    }

    /// The source document, `this` for a same-document reference.
    pub fn document_or(&self, this: DocumentId) -> DocumentId {
        self.document.unwrap_or(this)
    }

    /// The version it points at, if any.
    pub fn version_id(&self) -> Option<VersionId> {
        match self.at {
            RefAt::Version(v) => Some(v),
            RefAt::Workspace => None,
        }
    }

    /// True if it points into another document than `this`.
    pub fn is_external(&self, this: DocumentId) -> bool {
        self.document.is_some_and(|d| d != this)
    }
}

/// A frozen copy of a referenced element, kept in the consumer ([`Document::linked`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedElement {
    /// Where it was copied from (the reference that first needed it).
    pub source: SourceRef,
    /// The source document's and version's names when it was copied (shown while the source
    /// can't be reached).
    #[serde(default)]
    pub document_name: String,
    #[serde(default)]
    pub version_name: String,
    /// The copy's content hash (part of its id).
    pub hash: u64,
    /// The copy, under its namespaced id ([`linked_id`]); inside it the source's ids, except that
    /// an assembly's instances name the copies of their sources.
    pub element: Element,
}

impl LinkedElement {
    pub fn id(&self) -> ElementId {
        self.element.id
    }

    /// "Block source › Block (V1)".
    pub fn describe(&self) -> String {
        if self.version_name.is_empty() {
            format!("{} › {}", self.document_name, self.element.name)
        } else {
            format!("{} › {} ({})", self.document_name, self.element.name, self.version_name)
        }
    }
}

/// What resolving a reference gives: the id of the referenced element's copy and every copy it
/// needs (itself first).
#[derive(Debug, Clone, PartialEq)]
pub struct LinkSnapshot {
    pub root: ElementId,
    pub links: Vec<LinkedElement>,
}

impl LinkSnapshot {
    /// The referenced element's copy.
    pub fn root_link(&self) -> Option<&LinkedElement> {
        self.links.iter().find(|l| l.id() == self.root)
    }

    /// The referenced element (the copy).
    pub fn element(&self) -> Option<&Element> {
        self.root_link().map(|l| &l.element)
    }
}

/// Whether a linked document can be reached (DV1.7, ER1.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Ok,
    /// In the trash.
    Trashed,
    /// Deleted permanently (or never in this library).
    Gone,
    /// There, but can't be read (the local stand-in for "no access").
    Inaccessible,
}

impl LinkState {
    /// The message shown for a reference in this state (Onshape's wording, DV1.7).
    pub fn message(self) -> Option<&'static str> {
        match self {
            LinkState::Ok => None,
            LinkState::Trashed => Some("Cannot open a document in the trash. Restore the document from Trash."),
            LinkState::Gone => Some("Resource does not exist"),
            LinkState::Inaccessible => Some("You cannot modify this feature because you cannot access the referenced document"),
        }
    }
}

/// Why a reference can't be made or resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// The source document can't be reached.
    State(LinkState),
    /// A reference to another document needs a version (DV1.8, ER1.5).
    NoVersion,
    /// The version or element isn't there.
    NotFound(String),
    /// Drawings can't be referenced.
    NotLinkable(String),
    /// The link would reach its own consumer (DV1.5): the path, "A › X → B › Y → A".
    Circular(String),
    /// P3G.4: refused by a rule, with why (a Part Studio derived twice, DV3.7).
    Refused(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::State(s) => write!(f, "{}", s.message().unwrap_or("The document can't be reached")),
            LinkError::NoVersion => write!(f, "A linked document needs a version. Create a version of it first."),
            LinkError::NotFound(what) => write!(f, "{what} not found"),
            LinkError::NotLinkable(what) => write!(f, "{what} can't be referenced"),
            LinkError::Circular(path) => write!(f, "Circular reference: {path}"),
            LinkError::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for LinkError {}

impl From<LinkError> for CommandError {
    fn from(e: LinkError) -> Self {
        CommandError::Invalid(e.to_string())
    }
}

/// The namespaced id of a copy of `element` of `document` with content hash `hash`.
pub fn linked_id(document: DocumentId, element: ElementId, hash: u64) -> ElementId {
    let mut b = Vec::with_capacity(48);
    b.extend_from_slice(b"cadrs-linked:");
    b.extend_from_slice(document.0.as_bytes());
    b.extend_from_slice(element.0.as_bytes());
    b.extend_from_slice(&hash.to_le_bytes());
    let hi = cadrs_kernel::naming::stable_hash(&b);
    b.reverse();
    let lo = cadrs_kernel::naming::stable_hash(&b);
    ElementId(uuid::Uuid::from_u128(((hi as u128) << 64) | lo as u128))
}

/// Copies `element` of `source` (the document as it is at the referenced point, see
/// [`Resolver::resolve`]) and everything it depends on, for the reference `r`. `version_name` is
/// shown with the copies ("V1"; empty at the workspace).
pub fn snapshot(source: &Document, r: SourceRef, version_name: &str) -> Result<LinkSnapshot, LinkError> {
    let mut links: Vec<LinkedElement> = Vec::new();
    let root = snap(source, r, version_name, r.element, &mut links, 0)?;
    // The referenced element first.
    if let Some(i) = links.iter().position(|l| l.id() == root) {
        let l = links.remove(i);
        links.insert(0, l);
    }
    Ok(LinkSnapshot { root, links })
}

fn snap(source: &Document, r: SourceRef, version_name: &str, id: ElementId, out: &mut Vec<LinkedElement>, depth: usize) -> Result<ElementId, LinkError> {
    if depth > 32 {
        return Err(LinkError::Circular(format!("{} nests references too deeply", source.name)));
    }
    // The source's own link: copied as it is (its id is namespaced already), with a relative
    // document made absolute; and what it needs.
    if let Some(l) = source.linked_element(id) {
        if out.iter().all(|x| x.id() != id) {
            let mut l = l.clone();
            l.source.document = Some(l.source.document_or(source.id));
            out.push(l.clone());
            for dep in dependencies(&l.element) {
                snap(source, r, version_name, dep, out, depth + 1)?;
            }
        }
        return Ok(id);
    }
    let el = source.element(id).ok_or_else(|| LinkError::NotFound(format!("Element {id} of {}", source.name)))?;
    let mut copy = el.clone();
    match &el.kind {
        ElementKind::Drawing(_) => return Err(LinkError::NotLinkable(format!("The drawing {}", el.name))),
        ElementKind::PcbStudio(_) => return Err(LinkError::NotLinkable(format!("The PCB Studio {}", el.name))),
        // P3F.6: a Render Studio renders its source here; it isn't linked elsewhere.
        ElementKind::Render(_) => return Err(LinkError::NotLinkable(format!("The Render Studio {}", el.name))),
        ElementKind::PartStudio { .. } => {
            copy.contexts.clear();
            // P3G.4: the copies its Derived features need come too, and their same-document
            // references now name the source document.
            for dep in dependencies(el) {
                snap(source, r, version_name, dep, out, depth + 1)?;
            }
            if let Some(features) = copy.features_mut() {
                for f in features {
                    if let crate::document::FeatureKind::Derived(d) = &mut f.kind
                        && let Some(s) = d.source.as_mut()
                    {
                        s.document = Some(s.document_or(source.id));
                    }
                }
            }
        }
        ElementKind::Assembly => {
            let mut map: HashMap<ElementId, ElementId> = HashMap::new();
            for dep in dependencies(el) {
                let to = snap(source, r, version_name, dep, out, depth + 1)?;
                map.insert(dep, to);
            }
            for inst in &mut copy.assembly.instances {
                let m = |e: ElementId| map.get(&e).copied().unwrap_or(e);
                inst.source = match inst.source {
                    InstanceSource::Part { element, part } => InstanceSource::Part { element: m(element), part },
                    InstanceSource::Assembly { element } => InstanceSource::Assembly { element: m(element) },
                    InstanceSource::Studio { element } => InstanceSource::Studio { element: m(element) },
                };
            }
        }
    }
    let mut bytes = source.id.0.as_bytes().to_vec();
    bytes.extend_from_slice(ron::to_string(&copy).unwrap_or_default().as_bytes());
    let hash = cadrs_kernel::naming::stable_hash(&bytes);
    let new_id = linked_id(source.id, id, hash);
    copy.id = new_id;
    if out.iter().all(|x| x.id() != new_id) {
        out.push(LinkedElement {
            source: SourceRef { element: id, pinned: false, ..r },
            document_name: source.name.clone(),
            version_name: version_name.to_string(),
            hash,
            element: copy,
        });
    }
    Ok(new_id)
}

/// The elements an element's instances come from.
pub(crate) fn dependencies(el: &Element) -> Vec<ElementId> {
    let mut out: Vec<ElementId> = Vec::new();
    for i in &el.assembly.instances {
        let e = i.source.element();
        if !out.contains(&e) {
            out.push(e);
        }
    }
    // P3G.4: a Part Studio's Derived features need their copies.
    for f in el.features() {
        if let crate::document::FeatureKind::Derived(d) = &f.kind
            && let Some(c) = d.copy
            && !out.contains(&c)
        {
            out.push(c);
        }
    }
    out
}

/// DV1.5: refuses copies that reach `target` of `doc` (its own element at any version), with the
/// path "Consumer › Assembly → Source › Element → Consumer".
pub fn check_cycle(doc: &Document, target: ElementId, links: &[LinkedElement], root: Option<&LinkedElement>) -> Result<(), LinkError> {
    for l in links {
        if l.source.document_or(doc.id) == doc.id && l.source.element == target {
            let here = doc.elements.iter().find(|e| e.id == target).map(|e| e.name.clone()).unwrap_or_default();
            let via = root.unwrap_or(l);
            return Err(LinkError::Circular(format!("{} › {here} → {} › {} → {}", doc.name, via.document_name, via.element.name, doc.name)));
        }
    }
    Ok(())
}

/// The copies a document holds that `links` doesn't have yet (by id).
fn add_links(doc: &mut Document, links: &[LinkedElement]) {
    for l in links {
        if doc.linked_element(l.id()).is_none() {
            doc.linked.push(l.clone());
        }
    }
}

/// Adds the copies a reference needs (a drawing view of a version, D13.1), before the view that
/// uses them; one step of the document. Adding a copy that is there already changes nothing.
#[derive(Debug, Clone)]
pub struct AddLinks {
    pub links: Vec<LinkedElement>,
}

impl Command for AddLinks {
    fn label(&self) -> String {
        match self.links.first() {
            Some(l) => format!("Reference {}", l.describe()),
            None => "Reference".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        add_links(doc, &self.links);
        Ok(())
    }
}

/// Another command together with the copies it needs (a drawing view of a version placed for the
/// first time, D13.1): one undo step over the whole document.
#[derive(Debug, Clone)]
pub struct WithLinks<C: Command> {
    pub links: Vec<LinkedElement>,
    pub command: C,
}

impl<C: Command> Command for WithLinks<C> {
    fn label(&self) -> String {
        self.command.label()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        add_links(doc, &self.links);
        self.command.apply(doc)
    }
}

/// Inserts instances of a linked element (Insert → Other documents, or the Current document at a
/// version: ER1.8, ER3.1): the copies it needs, then the instances (numbered as usual), each
/// carrying the reference. One undo step; refused when circular (DV1.5).
#[derive(Debug, Clone)]
pub struct InsertLinked {
    /// The assembly inserted into.
    pub element: ElementId,
    pub snapshot: LinkSnapshot,
    /// The instances; their sources name the snapshot's copies.
    pub instances: Vec<Instance>,
    pub reference: SourceRef,
}

impl Command for InsertLinked {
    fn label(&self) -> String {
        "Insert linked instance".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check_cycle(doc, self.element, &self.snapshot.links, self.snapshot.root_link())?;
        add_links(doc, &self.snapshot.links);
        for inst in &self.instances {
            let mut inst = inst.clone();
            inst.link = Some(self.reference);
            crate::assembly::commands::InsertInstance { element: self.element, instance: inst }.apply(doc)?;
        }
        Ok(())
    }
}

/// Whether `document` can be reached in `store` (DV1.7).
pub fn link_state(store: &Store, document: DocumentId) -> LinkState {
    let path = store.document_path(document);
    if !path.is_file() {
        return LinkState::Gone;
    }
    match store.load(document) {
        Ok(f) if f.meta.trashed.is_some() => LinkState::Trashed,
        Ok(_) => LinkState::Ok,
        Err(_) => LinkState::Inaccessible,
    }
}

/// A file's modification time and length, to tell when it changed.
type Stamp = Option<(std::time::SystemTime, u64)>;

fn stamp(path: &PathBuf) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// Reads other documents' histories for links, caching each until its file changes (DV1.2,
/// DV X2).
#[derive(Debug, Clone)]
pub struct Resolver {
    store: Store,
    logs: HashMap<DocumentId, (Stamp, Arc<HistoryLog>)>,
    /// Other documents as they are on disk now (their state, and their workspace when it could
    /// be read), read again only when their `document.ron` changed: the app asks every few
    /// seconds, and parsing a big document each time stalled frames.
    current: HashMap<DocumentId, (Stamp, LinkState, Option<Arc<Document>>)>,
    versions: HashMap<(DocumentId, VersionId), Arc<Document>>,
    /// How many times a `history.ron` was read (tests: resolving twice reads once).
    pub reads: usize,
}

impl Resolver {
    pub fn new(store: Store) -> Self {
        Self { store, logs: HashMap::new(), current: HashMap::new(), versions: HashMap::new(), reads: 0 }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Whether `document` can be reached ([`link_state`], read again only when its file changed).
    pub fn state(&mut self, document: DocumentId) -> LinkState {
        self.read_current(document).0
    }

    /// `document`'s workspace as it is on disk (in the trash too), read again only when its file
    /// changed; `None` when it is gone or can't be read.
    pub fn current(&mut self, document: DocumentId) -> Option<Arc<Document>> {
        self.read_current(document).1
    }

    fn read_current(&mut self, document: DocumentId) -> (LinkState, Option<Arc<Document>>) {
        let path = self.store.document_path(document);
        let now = stamp(&path);
        if now.is_none() || !path.is_file() {
            self.current.remove(&document);
            return (LinkState::Gone, None);
        }
        if let Some((s, state, doc)) = self.current.get(&document)
            && *s == now
        {
            return (*state, doc.clone());
        }
        let (state, doc) = match self.store.load(document) {
            Ok(f) if f.meta.trashed.is_some() => (LinkState::Trashed, Some(Arc::new(f.document))),
            Ok(f) => (LinkState::Ok, Some(Arc::new(f.document))),
            Err(_) => (LinkState::Inaccessible, None),
        };
        self.current.insert(document, (now, state, doc.clone()));
        (state, doc)
    }

    /// The history of `document`, read again only when its file changed.
    pub fn log(&mut self, document: DocumentId) -> Option<Arc<HistoryLog>> {
        let path = HistoryLog::path(&self.store, document);
        let now = stamp(&path);
        now?;
        if let Some((s, log)) = self.logs.get(&document)
            && *s == now
        {
            return Some(log.clone());
        }
        self.reads += 1;
        let log = Arc::new(HistoryLog::load_path(&path).ok()?);
        self.versions.retain(|(d, _), _| *d != document);
        self.logs.insert(document, (now, log.clone()));
        Some(log)
    }

    /// The versions of `document`, oldest first.
    pub fn versions(&mut self, document: DocumentId) -> Vec<Version> {
        self.log(document).map(|l| l.versions().to_vec()).unwrap_or_default()
    }

    /// Forgets what was read of `document` (P3G.2: after writing it, as Update all's auto
    /// versions do), so the next read sees the new file whatever its time stamp.
    pub fn forget(&mut self, document: DocumentId) {
        self.logs.remove(&document);
        self.current.remove(&document);
        self.versions.retain(|(d, _), _| *d != document);
    }

    /// Its newest version.
    pub fn latest(&mut self, document: DocumentId) -> Option<Version> {
        self.versions(document).last().cloned()
    }

    /// `document` as it was at `version` (read-only, cached).
    pub fn document_at(&mut self, document: DocumentId, version: VersionId) -> Result<Arc<Document>, LinkError> {
        let log = self.log(document).ok_or(LinkError::NoVersion)?;
        if let Some(d) = self.versions.get(&(document, version)) {
            return Ok(d.clone());
        }
        let doc = Arc::new(log.document_at_version(version).ok_or_else(|| LinkError::NotFound("The version".into()))?);
        self.versions.insert((document, version), doc.clone());
        Ok(doc)
    }

    /// Resolves `r` (from `this`, whose own history is `this_log`) into the copies it needs.
    pub fn resolve(&mut self, r: SourceRef, this: &Document, this_log: Option<&HistoryLog>) -> Result<LinkSnapshot, LinkError> {
        let doc_id = r.document_or(this.id);
        let version = r.version_id();
        if doc_id == this.id {
            let v = version.ok_or(LinkError::NoVersion)?;
            let log = this_log.ok_or(LinkError::NoVersion)?;
            let name = log.version(v).map(|v| v.name().to_string()).unwrap_or_default();
            let at = log.document_at_version(v).ok_or_else(|| LinkError::NotFound("The version".into()))?;
            return snapshot(&at, r, &name);
        }
        match self.state(doc_id) {
            LinkState::Ok => {}
            s => return Err(LinkError::State(s)),
        }
        let v = version.ok_or(LinkError::NoVersion)?;
        let name = self.log(doc_id).and_then(|l| l.version(v).map(|v| v.name().to_string())).unwrap_or_default();
        let at = self.document_at(doc_id, v)?;
        snapshot(&at, r, &name)
    }

    /// Makes a version of another document as it is on disk (Create version in the Other
    /// documents browser, DV1.8): its log is started or caught up first. Not undoable: versions
    /// are immutable, and the version belongs to the other document.
    pub fn create_version(&mut self, document: DocumentId, name: &str, description: &str, now: Timestamp, user: &str) -> Result<VersionId, LinkError> {
        let file = self.store.load(document).map_err(|_| LinkError::State(self.state(document)))?;
        let mut log = HistoryLog::load(&self.store, document)
            .ok()
            .flatten()
            .unwrap_or_else(|| HistoryLog::start(&file.document, file.meta.created, user));
        if log.head() != &file.document {
            log.catch_up(&file.document, now, user);
        }
        let id = log.create_version(name, description, now, user);
        log.save(&self.store).map_err(|e| LinkError::NotFound(format!("The history ({e})")))?;
        self.logs.remove(&document);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_ids_depend_on_the_document_the_element_and_the_contents() {
        let (d, e) = (DocumentId::from_u128(1), ElementId::from_u128(2));
        let a = linked_id(d, e, 7);
        assert_eq!(a, linked_id(d, e, 7));
        assert_ne!(a, linked_id(d, e, 8));
        assert_ne!(a, linked_id(DocumentId::from_u128(3), e, 7));
        assert_ne!(a, linked_id(d, ElementId::from_u128(4), 7));
        assert_ne!(a, e);
    }

    #[test]
    fn messages_are_onshapes() {
        assert_eq!(LinkState::Trashed.message(), Some("Cannot open a document in the trash. Restore the document from Trash."));
        assert_eq!(LinkState::Gone.message(), Some("Resource does not exist"));
        assert_eq!(LinkState::Ok.message(), None);
    }
}
