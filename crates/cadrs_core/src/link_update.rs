//! P3G.2: keeping references up to date (`derived-and-linking-gaps.md` DV1.3, DV1.4, DV1.6,
//! DV1.10, ER2–ER5, ER X1, ER X4; the model is [`crate::external`]).
//!
//! - **Uses.** A consumer uses a reference in two places ([`RefSite`]): an assembly instance
//!   (its [`crate::assembly::Instance::link`]), and a drawing's source (every view of a linked
//!   copy in one Drawing tab, the [`cadrs_drawing::ModelSource`] of that copy). [`uses`] lists the
//!   version references; [`workspace_uses`] the same-document ones that follow the workspace
//!   (for Change to version, ER3.3, D2.10).
//! - **Staleness** ([`staleness`]): a version reference is *out of date* when its source document
//!   has a newer version (the blue badge, ER2.3), and *transitively* out of date when a copy it
//!   holds (a subassembly's own link, further down the chain) is (the arrow variant, ER4.2). The
//!   caller says which version is newest ([`Latest`]), so the query is pure.
//! - **Changes** ([`change_for`], [`UpdateReferences`]): re-pointing uses at another version (or,
//!   within the document, back at the workspace: ER3.6) is one undoable command that adds the
//!   new copies, re-points the instances (keeping their ids, poses and mates: names inside a copy
//!   are the source's, so mates re-resolve) or the drawing's views, and drops the copies nothing
//!   uses any more.
//! - **Pinning** ([`SetPinned`], ER5): a pinned version reference is skipped by Update all.
//! - **Update all** ([`plan_update_all`], [`execute_update_all`], ER4): every unpinned out-of-date
//!   reference, grouped by source document. A source document whose own workspace holds
//!   out-of-date references (A → B → C, updated from C) is updated first and gets an **auto
//!   version** ([`crate::history_log::HistoryLog::create_auto_version`]), which the consumer then
//!   references. Those writes belong to the other documents: undo in the consumer re-points its
//!   references but never removes the versions (ER4.8).
//! - **Where used** ([`where_used`], DV1.6): the library's documents that reference a document,
//!   with the versions they use.

use std::collections::{HashMap, HashSet};

use crate::assembly::{InstanceId, InstanceSource};
use crate::command::{Command, CommandError, History, Scope};
use crate::document::{Document, ElementKind};
use crate::external::{LinkError, LinkSnapshot, LinkedElement, RefAt, Resolver, SourceRef, check_cycle, dependencies};
use crate::history_log::{HistoryLog, Origin, VersionId};
use crate::ids::{DocumentId, ElementId};
use crate::library::Timestamp;
use crate::store::Store;

/// Where a consumer uses a reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefSite {
    /// An instance of an assembly tab.
    Instance { element: ElementId, instance: InstanceId },
    /// Every view of `source` (a linked copy, or a Part Studio or assembly of this document at
    /// the workspace) in the Drawing tab `element`.
    Drawing { element: ElementId, source: ElementId },
    /// P3G.4: a Derived feature of the Part Studio `element`.
    Derived { element: ElementId, feature: crate::ids::FeatureId },
}

impl RefSite {
    /// The tab the use is in.
    pub fn tab(&self) -> ElementId {
        match *self {
            RefSite::Instance { element, .. } | RefSite::Drawing { element, .. } | RefSite::Derived { element, .. } => element,
        }
    }
}

/// One use of a reference (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefUse {
    pub site: RefSite,
    /// What it references; at the workspace for a same-document live use.
    pub reference: SourceRef,
    /// The element the site points at now: the linked copy, or the tab itself at the workspace.
    pub source: ElementId,
}

impl RefUse {
    /// True for a reference to a version (a linked copy).
    pub fn is_version(&self) -> bool {
        matches!(self.reference.at, RefAt::Version(_))
    }
}

/// The version references of `doc`: linked instances of its assembly tabs, and its drawings'
/// sources that are linked copies.
pub fn uses(doc: &Document) -> Vec<RefUse> {
    let mut out = Vec::new();
    for el in &doc.elements {
        if let Some(model) = el.assembly_model() {
            for i in &model.instances {
                if let Some(r) = i.link {
                    out.push(RefUse { site: RefSite::Instance { element: el.id, instance: i.id }, reference: r, source: i.source.element() });
                }
            }
        }
        if let Some(d) = el.drawing_data() {
            for s in &d.sources {
                let id = ElementId(s.element);
                if let Some(l) = doc.linked_element(id) {
                    let mut r = l.source;
                    // A same-document copy's reference names no document.
                    if r.document == Some(doc.id) {
                        r.document = None;
                    }
                    r.pinned = s.pinned;
                    out.push(RefUse { site: RefSite::Drawing { element: el.id, source: id }, reference: r, source: id });
                }
            }
        }
        // P3G.4: Derived features referencing a version.
        for f in el.features() {
            if let crate::document::FeatureKind::Derived(d) = &f.kind
                && let (Some(mut r), Some(copy)) = (d.source, d.copy)
                && matches!(r.at, RefAt::Version(_))
            {
                if r.document == Some(doc.id) {
                    r.document = None;
                }
                out.push(RefUse { site: RefSite::Derived { element: el.id, feature: f.id }, reference: r, source: copy });
            }
        }
    }
    out
}

/// The same-document references of `doc` that follow the workspace (instances of its own Part
/// Studios and assemblies, its drawings' views of its own tabs), in tab `tab` or everywhere.
pub fn workspace_uses(doc: &Document, tab: Option<ElementId>) -> Vec<RefUse> {
    let local = |e: ElementId| doc.elements.iter().any(|x| x.id == e && !matches!(x.kind, ElementKind::Drawing(_)));
    let mut out = Vec::new();
    for el in doc.elements.iter().filter(|e| tab.is_none_or(|t| t == e.id)) {
        if let Some(model) = el.assembly_model() {
            for i in &model.instances {
                let e = i.source.element();
                if i.link.is_none() && local(e) && crate::assembly::standard::standard_of(doc, &i.source).is_none() {
                    let r = SourceRef { document: None, at: RefAt::Workspace, element: e, pinned: false };
                    out.push(RefUse { site: RefSite::Instance { element: el.id, instance: i.id }, reference: r, source: e });
                }
            }
        }
        if let Some(d) = el.drawing_data() {
            for s in &d.sources {
                let e = ElementId(s.element);
                if local(e) {
                    let r = SourceRef { document: None, at: RefAt::Workspace, element: e, pinned: false };
                    out.push(RefUse { site: RefSite::Drawing { element: el.id, source: e }, reference: r, source: e });
                }
            }
        }
        // P3G.4: Derived features following a tab of this document.
        for f in el.features() {
            if let crate::document::FeatureKind::Derived(d) = &f.kind
                && let Some(r) = d.source
                && r.at == RefAt::Workspace
                && r.document_or(doc.id) == doc.id
            {
                let r = SourceRef { document: None, at: RefAt::Workspace, element: r.element, pinned: false };
                out.push(RefUse { site: RefSite::Derived { element: el.id, feature: f.id }, reference: r, source: r.element });
            }
        }
    }
    out
}

/// The use at `site` (a version reference or a workspace one), if there is one.
pub fn use_at(doc: &Document, site: RefSite) -> Option<RefUse> {
    uses(doc).into_iter().chain(workspace_uses(doc, Some(site.tab()))).find(|u| u.site == site)
}

/// Every copy `root` needs (itself first), following the copies' instances.
pub fn closure(doc: &Document, root: ElementId) -> Vec<&LinkedElement> {
    let mut out: Vec<&LinkedElement> = Vec::new();
    let mut stack = vec![root];
    let mut seen: HashSet<ElementId> = HashSet::new();
    while let Some(e) = stack.pop() {
        if !seen.insert(e) {
            continue;
        }
        if let Some(l) = doc.linked_element(e) {
            out.push(l);
            stack.extend(dependencies(&l.element));
        }
    }
    out
}

/// Tells the newest version of a document (`None`: no versions, or it can't be read).
pub type Latest<'a> = dyn FnMut(DocumentId) -> Option<(VersionId, String)> + 'a;

/// Whether a use is out of date (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Staleness {
    /// Its source document's newest version, when that is another than the one referenced.
    pub newer: Option<(VersionId, String)>,
    /// A copy further down the chain is out of date (the transitive indicator, ER4.2).
    pub nested: bool,
}

impl Staleness {
    pub fn any(&self) -> bool {
        self.newer.is_some() || self.nested
    }
}

/// Whether `u` of `doc` is out of date, directly or further down (ER2.3, ER4.2). A workspace use
/// never is.
pub fn staleness(doc: &Document, u: &RefUse, latest: &mut Latest<'_>) -> Staleness {
    let RefAt::Version(v) = u.reference.at else { return Staleness::default() };
    let newer = latest(u.reference.document_or(doc.id)).filter(|(l, _)| *l != v);
    let mut nested = false;
    for l in closure(doc, u.source).into_iter().skip(1) {
        if let RefAt::Version(nv) = l.source.at
            && latest(l.source.document_or(doc.id)).is_some_and(|(x, _)| x != nv)
        {
            nested = true;
            break;
        }
    }
    Staleness { newer, nested }
}

/// A use re-pointed: at `to` (with the copies `snapshot` it needs), or at the workspace
/// (`snapshot` `None`, same document only).
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub site: RefSite,
    pub to: SourceRef,
    pub snapshot: Option<LinkSnapshot>,
}

/// Where a use should point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Its source document's newest version.
    Latest,
    Version(VersionId),
    /// The workspace (a same-document reference only, ER3.6).
    Workspace,
}

/// The change that points `u` at `target` (`None` when it points there already). `log` is
/// `doc`'s own history (same-document versions).
pub fn change_for(res: &mut Resolver, doc: &Document, log: Option<&HistoryLog>, u: &RefUse, target: Target) -> Result<Option<Change>, LinkError> {
    let source_doc = u.reference.document_or(doc.id);
    let this = source_doc == doc.id;
    let v = match target {
        Target::Workspace => {
            if !this {
                return Err(LinkError::NotLinkable("Another document's workspace".into()));
            }
            if u.reference.at == RefAt::Workspace {
                return Ok(None);
            }
            let to = SourceRef { document: None, at: RefAt::Workspace, element: u.reference.element, pinned: false };
            return Ok(Some(Change { site: u.site, to, snapshot: None }));
        }
        Target::Version(v) => v,
        Target::Latest => {
            let latest = if this { log.and_then(|l| l.versions().last().map(|v| v.id())) } else { res.latest(source_doc).map(|v| v.id()) };
            latest.ok_or(LinkError::NoVersion)?
        }
    };
    if u.reference.at == RefAt::Version(v) {
        return Ok(None);
    }
    let to = SourceRef { document: if this { None } else { Some(source_doc) }, at: RefAt::Version(v), element: u.reference.element, pinned: u.reference.pinned };
    let snapshot = res.resolve(to, doc, log)?;
    Ok(Some(Change { site: u.site, to, snapshot: Some(snapshot) }))
}

fn with_element(s: InstanceSource, e: ElementId) -> InstanceSource {
    match s {
        InstanceSource::Part { part, .. } => InstanceSource::Part { element: e, part },
        InstanceSource::Assembly { .. } => InstanceSource::Assembly { element: e },
        InstanceSource::Studio { .. } => InstanceSource::Studio { element: e },
    }
}

/// Re-points uses (Update to latest, Selective update, Change to version / workspace, Update
/// all): one undo step over the whole document (DV1.4, ER2.4–ER2.7, ER3.3–ER3.6).
#[derive(Debug, Clone)]
pub struct UpdateReferences {
    pub changes: Vec<Change>,
    pub label: String,
}

impl Command for UpdateReferences {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.changes.is_empty() {
            return Err(CommandError::Invalid("Nothing to update".into()));
        }
        let mut old: Vec<ElementId> = Vec::new();
        for c in &self.changes {
            if let Some(s) = &c.snapshot {
                if let RefSite::Instance { element, .. } | RefSite::Derived { element, .. } = c.site {
                    check_cycle(doc, element, &s.links, s.root_link())?;
                }
                for l in &s.links {
                    if doc.linked_element(l.id()).is_none() {
                        doc.linked.push(l.clone());
                    }
                }
            }
            let new = c.snapshot.as_ref().map(|s| s.root).unwrap_or(c.to.element);
            let version = matches!(c.to.at, RefAt::Version(_));
            match c.site {
                RefSite::Instance { element, instance } => {
                    let inst = doc
                        .element_mut(element)
                        .and_then(|e| e.assembly_model_mut())
                        .and_then(|a| a.instance_mut(instance))
                        .ok_or(CommandError::ElementNotFound(element))?;
                    old.push(inst.source.element());
                    inst.source = with_element(inst.source, new);
                    inst.link = version.then_some(c.to);
                }
                RefSite::Drawing { element, source } => {
                    let mut src = crate::drawing_source::live_source(doc, new).ok_or_else(|| CommandError::Invalid("The referenced tab can't be shown".into()))?;
                    src.pinned = version && c.to.pinned;
                    let el = doc.element_mut(element).ok_or(CommandError::ElementNotFound(element))?;
                    let ElementKind::Drawing(d) = &mut el.kind else { return Err(CommandError::ElementNotFound(element)) };
                    for sheet in &mut d.sheets {
                        if let Some(r) = sheet.reference.as_mut()
                            && r.element == source.0
                        {
                            r.element = new.0;
                        }
                        for v in &mut sheet.views {
                            if v.reference.element == source.0 {
                                v.reference.element = new.0;
                                v.source_hash = src.hash_of(v.reference.part);
                            }
                        }
                    }
                    d.sources.retain(|s| s.element != source.0 && s.element != new.0);
                    d.sources.push(src);
                    old.push(source);
                }
                RefSite::Derived { element, feature } => {
                    // The source's features and settings at the new point; the host's own
                    // selection, locations and options stay.
                    let from = match &c.snapshot {
                        Some(s) => s.root_link().map(|l| (l.element.clone(), l.document_name.clone(), l.version_name.clone())),
                        None => doc.elements.iter().find(|e| e.id == c.to.element).map(|e| (e.clone(), String::new(), String::new())),
                    };
                    let (el, dn, vn) = from.ok_or(CommandError::ElementNotFound(c.to.element))?;
                    let d = derived_site(doc, element, feature)?;
                    if let Some(o) = d.copy {
                        old.push(o);
                    }
                    d.fill_from(&el, &dn, &vn);
                    d.source = Some(c.to);
                    d.copy = c.snapshot.as_ref().map(|s| s.root);
                }
            }
        }
        prune(doc, &old);
        // P3G.5 (ex-dv4, ER6.17): the assemblies whose instances were re-pointed re-solve their
        // mates on the new geometry (a lost mate is left out and shows as an error).
        let mut solved: Vec<ElementId> = Vec::new();
        for c in &self.changes {
            if let RefSite::Instance { element, .. } = c.site
                && !solved.contains(&element)
            {
                solved.push(element);
                crate::assembly::resolve_after_update(doc, element);
            }
        }
        Ok(())
    }
}

/// The Derived feature at a site.
fn derived_site(doc: &mut Document, element: ElementId, feature: crate::ids::FeatureId) -> Result<&mut crate::derived::DerivedFeature, CommandError> {
    let el = doc.element_mut(element).ok_or(CommandError::ElementNotFound(element))?;
    let ElementKind::PartStudio { features, .. } = &mut el.kind else { return Err(CommandError::ElementNotFound(element)) };
    match features.iter_mut().find(|f| f.id == feature).map(|f| &mut f.kind) {
        Some(crate::document::FeatureKind::Derived(d)) => Ok(d),
        _ => Err(CommandError::Invalid("Derived feature not found".into())),
    }
}

/// Drops the copies among `candidates` (and what they need) that nothing uses any more.
pub(crate) fn prune(doc: &mut Document, candidates: &[ElementId]) {
    let mut used: HashSet<ElementId> = HashSet::new();
    for el in &doc.elements {
        if let Some(m) = el.assembly_model() {
            used.extend(m.instances.iter().map(|i| i.source.element()));
        }
        if let Some(d) = el.drawing_data() {
            used.extend(d.sources.iter().map(|s| ElementId(s.element)));
            for s in &d.sheets {
                used.extend(s.reference.iter().map(|r| ElementId(r.element)));
                used.extend(s.views.iter().map(|v| ElementId(v.reference.element)));
            }
        }
        // P3G.4: Derived features' copies.
        for f in el.features() {
            if let crate::document::FeatureKind::Derived(d) = &f.kind
                && let Some(c) = d.copy
            {
                used.insert(c);
            }
        }
    }
    let mut keep: HashSet<ElementId> = HashSet::new();
    for e in used {
        keep.extend(closure(doc, e).iter().map(|l| l.id()));
    }
    let mut drop: HashSet<ElementId> = HashSet::new();
    for c in candidates {
        drop.extend(closure(doc, *c).iter().map(|l| l.id()));
    }
    doc.linked.retain(|l| !drop.contains(&l.id()) || keep.contains(&l.id()));
}

/// Pins or unpins references to versions (ER5.1, ER5.6, ER5.7): Update all skips pinned ones.
/// A reference to the workspace can't be pinned (ER5.8).
#[derive(Debug, Clone)]
pub struct SetPinned {
    pub sites: Vec<RefSite>,
    pub pinned: bool,
}

/// Why a workspace reference can't be pinned.
pub const PIN_WORKSPACE: &str = "Only a reference to a version can be pinned: a reference to the workspace always follows the tab";

impl Command for SetPinned {
    fn label(&self) -> String {
        if self.pinned { "Pin reference".into() } else { "Unpin reference".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        for site in &self.sites {
            match *site {
                RefSite::Instance { element, instance } => {
                    let inst = doc
                        .element_mut(element)
                        .and_then(|e| e.assembly_model_mut())
                        .and_then(|a| a.instance_mut(instance))
                        .ok_or(CommandError::ElementNotFound(element))?;
                    match inst.link.as_mut() {
                        Some(r) if matches!(r.at, RefAt::Version(_)) => r.pinned = self.pinned,
                        _ => return Err(CommandError::Invalid(PIN_WORKSPACE.into())),
                    }
                }
                RefSite::Derived { element, feature } => {
                    let d = derived_site(doc, element, feature)?;
                    match d.source.as_mut() {
                        Some(r) if matches!(r.at, RefAt::Version(_)) => r.pinned = self.pinned,
                        _ => return Err(CommandError::Invalid(PIN_WORKSPACE.into())),
                    }
                }
                RefSite::Drawing { element, source } => {
                    if doc.linked_element(source).is_none() {
                        return Err(CommandError::Invalid(PIN_WORKSPACE.into()));
                    }
                    let el = doc.element_mut(element).ok_or(CommandError::ElementNotFound(element))?;
                    let ElementKind::Drawing(d) = &mut el.kind else { return Err(CommandError::ElementNotFound(element)) };
                    let s = d.sources.iter_mut().find(|s| s.element == source.0).ok_or(CommandError::ElementNotFound(source))?;
                    s.pinned = self.pinned;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Update all (ER4)

/// What a row of Update all updates its references to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowTarget {
    /// An existing version ("V1 ⇒ V2").
    Version(VersionId, String),
    /// A version made on the way ("V1 ⇒ new version", ER4.4, ER4.5).
    NewVersion,
}

/// One source document's references at one version, and where they go.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRow {
    pub document: DocumentId,
    /// The source document's name.
    pub name: String,
    /// The version referenced now ("V1").
    pub from: String,
    pub to: RowTarget,
    pub sites: Vec<RefSite>,
}

impl PlanRow {
    /// "V1 ⇒ V2", "V1 ⇒ new version".
    pub fn arrow(&self) -> String {
        match &self.to {
            RowTarget::Version(_, n) => format!("{} ⇒ {n}", self.from),
            RowTarget::NewVersion => format!("{} ⇒ new version", self.from),
        }
    }
}

/// The newest version of `d` as the resolver reads it (its own log for `this`).
fn latest_of(res: &mut Resolver, this: &Document, log: Option<&HistoryLog>, d: DocumentId) -> Option<(VersionId, String)> {
    if d == this.id {
        return log.and_then(|l| l.versions().last()).map(|v| (v.id(), v.name().to_string()));
    }
    res.latest(d).map(|v| (v.id(), v.name().to_string()))
}

/// Whether document `d`'s workspace (in the store) holds unpinned out-of-date references, here
/// or further down: updating a reference to `d` then needs a new version of it (ER4.5).
pub fn needs_auto_version(res: &mut Resolver, d: DocumentId) -> bool {
    needs_auto(res, d, &mut HashSet::new())
}

fn needs_auto(res: &mut Resolver, d: DocumentId, seen: &mut HashSet<DocumentId>) -> bool {
    if !seen.insert(d) {
        return false;
    }
    let Ok(file) = res.store().load(d) else { return false };
    let doc = file.document;
    let log = HistoryLog::load(res.store(), d).ok().flatten();
    for u in uses(&doc).into_iter().filter(|u| !u.reference.pinned) {
        let st = staleness(&doc, &u, &mut |x| latest_of(res, &doc, log.as_ref(), x));
        if st.any() {
            return true;
        }
        let src = u.reference.document_or(doc.id);
        if src != doc.id && needs_auto(res, src, seen) {
            return true;
        }
    }
    false
}

/// Update all's rows for `doc` (ER4.3, ER4.4): its unpinned version references (only those at
/// `only`'s sites, when given) that are out of date, directly or further down, grouped by source
/// document and version. A source whose workspace needs updating itself gets a new version.
pub fn plan_update_all(res: &mut Resolver, doc: &Document, log: Option<&HistoryLog>, only: Option<&[RefSite]>) -> Vec<PlanRow> {
    let mut rows: Vec<PlanRow> = Vec::new();
    let mut auto: HashMap<DocumentId, bool> = HashMap::new();
    for u in uses(doc) {
        if u.reference.pinned || only.is_some_and(|o| !o.contains(&u.site)) {
            continue;
        }
        let RefAt::Version(v) = u.reference.at else { continue };
        let d = u.reference.document_or(doc.id);
        let latest = latest_of(res, doc, log, d);
        let copy = doc.linked_element(u.source);
        let from = copy.map(|l| l.version_name.clone()).unwrap_or_default();
        let name = if d == doc.id { doc.name.clone() } else { copy.map(|l| l.document_name.clone()).unwrap_or_default() };
        let needs = d != doc.id && *auto.entry(d).or_insert_with(|| needs_auto_version(res, d));
        let to = match latest {
            _ if needs => RowTarget::NewVersion,
            Some((l, n)) if l != v => RowTarget::Version(l, n),
            // Out of date only further down, with the source's workspace already up to date
            // (updated but not versioned): a new version of it carries the update.
            _ if d != doc.id && staleness(doc, &u, &mut |x| latest_of(res, doc, log, x)).nested => RowTarget::NewVersion,
            _ => continue,
        };
        match rows.iter_mut().find(|r| r.document == d && r.from == from && r.to == to) {
            Some(r) => r.sites.push(u.site),
            None => rows.push(PlanRow { document: d, name, from, to, sites: vec![u.site] }),
        }
    }
    rows
}

/// An auto version Update all made in another document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoVersion {
    pub document: DocumentId,
    pub document_name: String,
    pub version: VersionId,
    pub name: String,
}

/// Carries out `rows` for `doc`: first the auto versions (each intermediate document's
/// workspace updated the same way, saved, and versioned, ER4.5), then the command that
/// re-points `doc`'s references, to run through the undo history. The auto versions are written
/// at once and stay whatever happens to the command (ER4.8).
pub fn execute_update_all(res: &mut Resolver, doc: &Document, log: Option<&HistoryLog>, rows: &[PlanRow], now: Timestamp, user: &str) -> Result<(UpdateReferences, Vec<AutoVersion>), LinkError> {
    let mut made: HashMap<DocumentId, VersionId> = HashMap::new();
    let mut autos = Vec::new();
    let cmd = execute_rows(res, doc, log, rows, now, user, &mut made, &mut autos, 0)?;
    Ok((cmd, autos))
}

#[allow(clippy::too_many_arguments)]
fn execute_rows(
    res: &mut Resolver,
    doc: &Document,
    log: Option<&HistoryLog>,
    rows: &[PlanRow],
    now: Timestamp,
    user: &str,
    made: &mut HashMap<DocumentId, VersionId>,
    autos: &mut Vec<AutoVersion>,
    depth: usize,
) -> Result<UpdateReferences, LinkError> {
    if depth > 32 {
        return Err(LinkError::Circular(format!("{} nests references too deeply", doc.name)));
    }
    let mut changes = Vec::new();
    for row in rows {
        let v = match &row.to {
            RowTarget::Version(v, _) => *v,
            RowTarget::NewVersion => auto_version(res, row.document, &doc.name, now, user, made, autos, depth + 1)?,
        };
        for site in &row.sites {
            let Some(u) = use_at(doc, *site) else { continue };
            if let Some(c) = change_for(res, doc, log, &u, Target::Version(v))? {
                changes.push(c);
            }
        }
    }
    Ok(UpdateReferences { changes, label: "Update all references".into() })
}

/// Updates document `d`'s workspace references (recursively) and makes its auto version.
#[allow(clippy::too_many_arguments)]
fn auto_version(
    res: &mut Resolver,
    d: DocumentId,
    consumer: &str,
    now: Timestamp,
    user: &str,
    made: &mut HashMap<DocumentId, VersionId>,
    autos: &mut Vec<AutoVersion>,
    depth: usize,
) -> Result<VersionId, LinkError> {
    if let Some(v) = made.get(&d) {
        return Ok(*v);
    }
    let store = res.store().clone();
    let mut file = store.load(d).map_err(|_| LinkError::State(res.state(d)))?;
    let log = HistoryLog::load(&store, d).ok().flatten();
    let rows = plan_update_all(res, &file.document, log.as_ref(), None);
    let cmd = execute_rows(res, &file.document, log.as_ref(), &rows, now, user, made, autos, depth)?;
    if !cmd.changes.is_empty() {
        let mut h = History::default();
        h.execute(&mut file.document, &cmd).map_err(|e| LinkError::NotFound(e.to_string()))?;
        file.meta.modified = now;
        store.save(&file.document, &file.meta).map_err(|e| LinkError::NotFound(format!("{} ({e})", file.document.name)))?;
    }
    let mut log = log.unwrap_or_else(|| HistoryLog::start(&file.document, file.meta.created, user));
    if log.head() != &file.document {
        log.record(&file.document, Origin::Command("Update all references".into()), now, user);
    }
    let v = log.create_auto_version(&format!("Created by Update all references in {consumer}"), now, user);
    let name = log.version(v).map(|x| x.name().to_string()).unwrap_or_default();
    log.save(&store).map_err(|e| LinkError::NotFound(format!("The history ({e})")))?;
    res.forget(d);
    made.insert(d, v);
    autos.push(AutoVersion { document: d, document_name: file.document.name.clone(), version: v, name });
    Ok(v)
}

// ---------------------------------------------------------------------------------------------
// Where used (DV1.6)

/// A document of the library that references another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub document: DocumentId,
    pub document_name: String,
    /// The consumer's tab.
    pub tab: String,
    /// The referenced tab (its name at the version) and version.
    pub element: String,
    pub version: String,
    pub count: usize,
}

/// The uses in `doc` of references to `document` (DV1.6), grouped by tab, referenced tab and
/// version.
pub fn usages_in(doc: &Document, document: DocumentId) -> Vec<Usage> {
    let mut out: Vec<Usage> = Vec::new();
    for u in uses(doc) {
        if u.reference.document_or(doc.id) != document {
            continue;
        }
        let copy = doc.linked_element(u.source);
        let tab = doc.elements.iter().find(|x| x.id == u.site.tab()).map(|x| x.name.clone()).unwrap_or_default();
        let element = copy.map(|l| l.element.name.clone()).unwrap_or_default();
        let version = copy.map(|l| l.version_name.clone()).unwrap_or_default();
        match out.iter_mut().find(|x| x.tab == tab && x.element == element && x.version == version) {
            Some(x) => x.count += 1,
            None => out.push(Usage { document: doc.id, document_name: doc.name.clone(), tab, element, version, count: 1 }),
        }
    }
    out
}

/// Every workspace in `store` (trash excluded) that references `document`, with its tab, the
/// referenced tab and version, and how many uses (DV1.6, TD3.8).
pub fn where_used(store: &Store, document: DocumentId) -> Vec<Usage> {
    let (lib, _) = store.list();
    let mut out: Vec<Usage> = Vec::new();
    for e in lib.entries.iter().filter(|e| e.meta.trashed.is_none() && e.id != document) {
        let Ok(file) = store.load(e.id) else { continue };
        out.extend(usages_in(&file.document, document));
    }
    out.sort_by(|a, b| (&a.document_name, &a.tab, &a.version).cmp(&(&b.document_name, &b.tab, &b.version)));
    out
}
