//! **Edit in context** (P3B.9, `intro-to-assemblies.md` X15) and **managed in-context design**
//! (`managed-in-context-design.md` MC1–MC4): a Part Studio opened from an instance of its part in
//! an assembly shows the rest of the assembly around that part, in the studio's coordinates, as
//! **context geometry** its features can use: sketch on a face of another instance, Use (project)
//! its edges, extrude up to it.
//!
//! - **Contexts** ([`StudioContext`], `Element::contexts`): a Part Studio has any number, each a
//!   **snapshot** of the assembly taken when it was made or last **updated** ([`snapshot`],
//!   [`resnapshot`]): which parts are around (hidden and suppressed instances are not, MC2.2),
//!   where they were relative to the **primary instance** (MC3.2), and their geometry: the
//!   features of their Part Studios as they were ([`StudioContext::studios`]), so editing those
//!   studios, like moving instances, changes nothing here until Update context (MC1.6). Each has
//!   a number ([`StudioContext::id`]) and a name ("Context 1", renamable, MC2.13).
//! - **Ids**: each context part is a part of the studio's view with its own feature id
//!   ([`context_id`] of the context and the occurrence, recognisable by [`is_context`]), so sketch
//!   planes ([`cadrs_sketch::FacePlane`]), links ([`cadrs_sketch::Link`]) and feature references
//!   name it like any part, and the same instance in two contexts gives two parts. Context 0's
//!   ids are the ones the single context of earlier documents had.
//! - **References** are worked out from the features ([`feature_contexts`]): which contexts a
//!   feature uses decides its in-context arrow (MC2.6, MC2.15), never stored.
//! - **Status** ([`status`]): up to date, out of date (the assembly moved, changed or a source
//!   studio was edited: the blue indicator, MC4.4), or without a primary instance (MC3.4).
//! - The regeneration ([`crate::parts::regenerate_with`]) is given every context's solids
//!   ([`solids`]); the app shows the active one's ([`parts_of`]).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::structure::occurrences;
use super::{InstanceId, InstanceSource, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, Feature, PartProps};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::parts::Part;
use crate::rebuild::Build;
pub use crate::transform::ContextSource;

/// The top 32 bits of every context part's feature id.
const MAGIC: u128 = 0xC0A7_E0C7;
const LOW96: u128 = !(0xFFFF_FFFFu128 << 96);

/// A context's number in its Part Studio (the first is 0).
pub type ContextNo = u32;

/// The feature id of the context part standing for the assembly occurrence `occurrence` in the
/// context `context` of a Part Studio.
pub fn context_id(context: ContextNo, occurrence: InstanceId) -> FeatureId {
    let mix = if context == 0 { 0 } else { (cadrs_kernel::naming::stable_hash(format!("context {context}").as_bytes()) as u128) << 32 | context as u128 };
    FeatureId(Uuid::from_u128(((occurrence.0.as_u128() ^ mix) & LOW96) | (MAGIC << 96)))
}

/// Whether a feature id is a context part's.
pub fn is_context(feature: FeatureId) -> bool {
    feature.0.as_u128() >> 96 == MAGIC
}

/// **First reference** (MC2.5): a context opened for editing (Edit in context, New context) is
/// *pending* until a feature references it: it is shown and can be referenced, the regeneration
/// sees it ([`solids`]), but it is not in the document. The app commits it with the command that
/// first references it ([`pending_to_commit`], [`AddContext`]), in the same undo step. Kept per
/// document and Part Studio.
static PENDING: std::sync::RwLock<Vec<((DocumentId, ElementId), StudioContext)>> = std::sync::RwLock::new(Vec::new());

/// Sets (or, `None`, drops) the pending context of the Part Studio `studio` of the document
/// `doc` (see [`pending`]).
pub fn set_pending(doc: DocumentId, studio: ElementId, ctx: Option<StudioContext>) {
    let Ok(mut p) = PENDING.write() else { return };
    p.retain(|(k, _)| *k != (doc, studio));
    if let Some(c) = ctx {
        p.push(((doc, studio), c));
    }
}

/// The pending context of the Part Studio `studio` of the document `doc` (MC2.5).
pub fn pending(doc: DocumentId, studio: ElementId) -> Option<StudioContext> {
    PENDING.read().ok()?.iter().find(|(k, _)| *k == (doc, studio)).map(|(_, c)| c.clone())
}

/// The id of the pending context of a Part Studio, without copying it.
pub fn pending_id(doc: DocumentId, studio: ElementId) -> Option<ContextNo> {
    PENDING.read().ok()?.iter().find(|(k, _)| *k == (doc, studio)).map(|(_, c)| c.id)
}

/// The contexts of the Part Studio `studio` with its pending one (unless the document has it).
pub fn with_pending(doc: &Document, studio: ElementId) -> Vec<StudioContext> {
    let mut out: Vec<StudioContext> = doc.element(studio).map(|e| e.contexts.clone()).unwrap_or_default();
    if pending_id(doc.id, studio).is_some_and(|id| !out.iter().any(|c| c.id == id))
        && let Some(p) = pending(doc.id, studio)
    {
        out.push(p);
    }
    out
}

/// The context (pending one included) a context part's feature id belongs to, and the part.
pub fn find_part(doc: &Document, studio: ElementId, feature: FeatureId) -> Option<(StudioContext, ContextPart)> {
    if !is_context(feature) {
        return None;
    }
    if let Some(el) = doc.element(studio)
        && let Some(c) = el.context_of(feature)
    {
        return Some((c.clone(), c.part(feature)?.clone()));
    }
    let p = pending(doc.id, studio)?;
    let part = p.part(feature)?.clone();
    Some((p, part))
}

/// A pending context of this document a feature now references, to commit (MC2.5).
pub fn pending_to_commit(doc: &Document) -> Option<(ElementId, StudioContext)> {
    let all: Vec<(ElementId, StudioContext)> = PENDING.read().ok()?.iter().filter(|((d, _), _)| *d == doc.id).map(|((_, s), c)| (*s, c.clone())).collect();
    all.into_iter().find(|(studio, ctx)| {
        let Some(el) = doc.element(*studio) else { return false };
        el.context(ctx.id).is_none() && el.features().iter().any(|f| context_refs(f).iter().any(|r| ctx.part(*r).is_some()))
    })
}

/// One part of the assembly around the edited instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPart {
    /// Its feature id in the studio ([`context_id`]).
    pub id: FeatureId,
    pub element: ElementId,
    pub part: PartId,
    /// Where it is in the studio's coordinates (as of the last update).
    pub pose: Pose,
    /// Its instance name in the assembly ("Base Frame Bar <1>").
    pub name: String,
    /// Its appearance in its Part Studio when the snapshot was taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<crate::appearance::Appearance>,
}

/// One of a Part Studio's assembly contexts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioContext {
    /// Its number in the Part Studio ([`context_id`]); 0 for the first.
    #[serde(default)]
    pub id: ContextNo,
    /// Its name; empty: "Context <id + 1>" ([`StudioContext::label`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The assembly, in this document, or in `document` (MC5).
    pub assembly: ElementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentId>,
    /// The **primary instance** (MC3): an instance of a part of this studio that anchors the
    /// context, or [`InstanceId::ORIGIN`] for a studio made in context and not inserted yet.
    pub instance: InstanceId,
    /// With the Origin as primary: where the studio's origin is in the assembly (the assembly
    /// Origin, or the mate connector picked for Create Part Studio in context, MC2.9).
    #[serde(default, skip_serializing_if = "is_identity")]
    pub origin: Pose,
    pub parts: Vec<ContextPart>,
    /// The features of the Part Studios the parts come from, as they were (MC1.2). Empty in
    /// documents from before frozen contexts: the studios' current features are used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<ContextSource>,
    /// The context geometry is hidden (its eye); it is still there for references.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// P3H.5 (X9): a fingerprint of the context parts' Part Studios' features when the snapshot
    /// was taken ([`sources_fingerprint`]), for contexts without [`StudioContext::studios`].
    /// 0: not recorded.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sources: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

fn is_identity(p: &Pose) -> bool {
    *p == Pose::IDENTITY
}

impl StudioContext {
    /// Its name as shown ("Context 1" until renamed).
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() { format!("Context {}", self.id + 1) } else { self.name.clone() }
    }

    /// The context part with the feature id `feature`.
    pub fn part(&self, feature: FeatureId) -> Option<&ContextPart> {
        self.parts.iter().find(|p| p.id == feature)
    }

    /// The features the context part's source studio had (frozen), or `None` for a context from
    /// before frozen contexts.
    pub fn frozen(&self, element: ElementId) -> Option<&[Feature]> {
        self.studios.iter().find(|s| s.element == element).map(|s| s.features.as_slice())
    }
}

impl Element {
    /// The context numbered `id`.
    pub fn context(&self, id: ContextNo) -> Option<&StudioContext> {
        self.contexts.iter().find(|c| c.id == id)
    }

    pub fn context_mut(&mut self, id: ContextNo) -> Option<&mut StudioContext> {
        self.contexts.iter_mut().find(|c| c.id == id)
    }

    /// The number the next new context gets.
    pub fn next_context_id(&self) -> ContextNo {
        self.contexts.iter().map(|c| c.id + 1).max().unwrap_or(0)
    }

    /// The context a context part's feature id belongs to.
    pub fn context_of(&self, feature: FeatureId) -> Option<&StudioContext> {
        if !is_context(feature) {
            return None;
        }
        self.contexts.iter().find(|c| c.part(feature).is_some())
    }
}

/// A fingerprint (FNV-1a of their serialized features) of the Part Studios the context parts
/// come from, in element id order (P3H.5).
pub fn sources_fingerprint(doc: &Document, parts: &[ContextPart]) -> u64 {
    let mut els: Vec<ElementId> = parts.iter().map(|p| p.element).collect();
    els.sort();
    els.dedup();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for e in els {
        let Some(el) = doc.element(e) else { continue };
        let text = ron::to_string(el.features()).unwrap_or_default();
        for b in e.to_string().bytes().chain(text.bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h.max(1)
}

/// The Part Studio an instance's part comes from (for Edit in context), if it is a part or
/// rigid Part Studio instance.
pub fn studio_of(doc: &Document, assembly: ElementId, instance: InstanceId) -> Option<ElementId> {
    let i = doc.element(assembly)?.assembly_model()?.instance(instance)?;
    match i.source {
        InstanceSource::Part { element, .. } | InstanceSource::Studio { element } => Some(element),
        InstanceSource::Assembly { .. } => None,
    }
}

/// The snapshot of `assembly` for the Part Studio `studio` as context `id`, placed by
/// `to_studio` (assembly coordinates to the studio's): every part at any depth that isn't from
/// `studio` and isn't hidden, with its Part Studio's features as they are now.
pub fn snapshot_with(doc: &Document, assembly: ElementId, studio: ElementId, instance: InstanceId, to_studio: Pose, id: ContextNo) -> Result<StudioContext, CommandError> {
    let asm = doc.element(assembly).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(assembly))?;
    let mut studios: Vec<ContextSource> = Vec::new();
    let mut parts = Vec::new();
    for o in occurrences(doc, asm).into_iter().filter(|o| o.element != studio && !o.hidden) {
        let Some(src) = doc.element(o.element) else { continue };
        if !studios.iter().any(|s| s.element == o.element) {
            studios.push(ContextSource { element: o.element, features: src.active_features() });
        }
        parts.push(ContextPart {
            id: context_id(id, o.id),
            element: o.element,
            part: o.part,
            pose: o.pose.then(&to_studio),
            name: format!("{} <{}>", super::source_part_name(doc, &InstanceSource::Part { element: o.element, part: o.part }, None), o.index),
            appearance: src.part_prop(o.part).and_then(|p| p.appearance),
        });
    }
    let sources = sources_fingerprint(doc, &parts);
    Ok(StudioContext { id, name: String::new(), assembly, document: None, instance, origin: if instance == InstanceId::ORIGIN { to_studio.inverse() } else { Pose::IDENTITY }, parts, studios, hidden: false, sources })
}

/// The context of the Part Studio of `instance` in `assembly` as the assembly is now, numbered
/// 0 (see [`snapshot_as`]).
pub fn snapshot(doc: &Document, assembly: ElementId, instance: InstanceId) -> Result<StudioContext, CommandError> {
    snapshot_as(doc, assembly, instance, 0)
}

/// The context `id` of the Part Studio of `instance` (its primary instance) in `assembly` as the
/// assembly is now: every other part placed relative to the instance.
pub fn snapshot_as(doc: &Document, assembly: ElementId, instance: InstanceId, id: ContextNo) -> Result<StudioContext, CommandError> {
    let asm = doc.element(assembly).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(assembly))?;
    let inst = asm.instance(instance).ok_or(CommandError::Invalid(NO_PRIMARY.into()))?;
    let studio = match inst.source {
        InstanceSource::Part { element, .. } | InstanceSource::Studio { element } => element,
        InstanceSource::Assembly { .. } => return Err(CommandError::Invalid("a subassembly is edited in its own tab".into())),
    };
    snapshot_with(doc, assembly, studio, instance, inst.pose.inverse(), id)
}

/// Why a context can't be updated without its primary instance (MC3.4).
pub const NO_PRIMARY: &str = "The context has no primary instance: set one in the assembly (Set as primary instance)";

/// The context `ctx` of the Part Studio `studio` as the assembly is now (Update context): the
/// same number, name and eye. Fails without a primary instance (MC3.4).
pub fn resnapshot(doc: &Document, studio: ElementId, ctx: &StudioContext) -> Result<StudioContext, CommandError> {
    let mut now = if ctx.instance == InstanceId::ORIGIN {
        snapshot_with(doc, ctx.assembly, studio, InstanceId::ORIGIN, ctx.origin.inverse(), ctx.id)?
    } else {
        let asm = doc.element(ctx.assembly).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(ctx.assembly))?;
        let inst = asm.instance(ctx.instance).filter(|i| !i.suppressed).ok_or(CommandError::Invalid(NO_PRIMARY.into()))?;
        snapshot_with(doc, ctx.assembly, studio, ctx.instance, inst.pose.inverse(), ctx.id)?
    };
    now.name = ctx.name.clone();
    now.hidden = ctx.hidden;
    now.document = ctx.document;
    Ok(now)
}

/// Where a context stands against its assembly (MC3.4, MC4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextStatus {
    UpToDate,
    /// The assembly moved or changed, or a source studio was edited, since the snapshot: an
    /// update is available.
    OutOfDate,
    /// The primary instance is gone: no update until another is set.
    NoPrimary,
    /// The assembly is gone, or in another document (checked there).
    Unknown,
}

/// See [`ContextStatus`].
pub fn status(doc: &Document, studio: ElementId, ctx: &StudioContext) -> ContextStatus {
    if ctx.document.is_some() || doc.element(ctx.assembly).and_then(|e| e.assembly_model()).is_none() {
        return ContextStatus::Unknown;
    }
    match resnapshot(doc, studio, ctx) {
        Ok(now) => {
            let moved = now.parts.iter().map(|p| (p.id, p.element, p.part, p.pose)).ne(ctx.parts.iter().map(|p| (p.id, p.element, p.part, p.pose)));
            let edited = if ctx.studios.is_empty() { ctx.sources != 0 && now.sources != ctx.sources } else { now.studios != ctx.studios };
            if moved || edited { ContextStatus::OutOfDate } else { ContextStatus::UpToDate }
        }
        Err(CommandError::Invalid(m)) if m == NO_PRIMARY => ContextStatus::NoPrimary,
        Err(_) => ContextStatus::Unknown,
    }
}

/// The features a context part's source studio is built from: the frozen ones, or (an old
/// context) the studio's current ones.
pub fn source_features(doc: &Document, ctx: &StudioContext, element: ElementId) -> Option<Vec<Feature>> {
    match ctx.frozen(element) {
        Some(f) => Some(f.to_vec()),
        None if ctx.studios.is_empty() => doc.element(element).map(|e| e.active_features()),
        None => None,
    }
}

/// The parts of the context `ctx` as parts of the studio's view (studio coordinates), with their
/// settings (appearance): each is its source part, from the snapshot's features, moved to where
/// the context has it. `build_of` rebuilds a source studio's features.
pub fn parts_of(doc: &Document, ctx: &StudioContext, mut build_of: impl FnMut(ElementId, &[Feature]) -> Option<Arc<Build>>) -> (Vec<Part>, Vec<PartProps>) {
    let mut builds: HashMap<ElementId, Option<Arc<Build>>> = HashMap::new();
    let mut parts = Vec::new();
    let mut props = Vec::new();
    for c in &ctx.parts {
        let build = builds.entry(c.element).or_insert_with(|| source_features(doc, ctx, c.element).and_then(|f| build_of(c.element, &f)));
        let Some(build) = build.as_ref() else { continue };
        let Some(src) = build.part(c.part) else { continue };
        let id = PartId::new(c.id, 0);
        let mut p = PartProps::new(id);
        p.name = Some(c.name.clone());
        p.appearance = c.appearance;
        if ctx.studios.is_empty()
            && let Some(sp) = doc.element(c.element).and_then(|e| e.part_prop(c.part))
        {
            p.appearance = sp.appearance;
        }
        if let Some(sp) = doc.element(c.element).and_then(|e| e.part_prop(c.part)) {
            p.material = sp.material.clone();
        }
        parts.push(Part {
            id,
            feature: c.id,
            name: c.name.clone(),
            kind: src.kind,
            palette: src.palette,
            solid: Arc::new(super::transform_solid(&src.solid, &c.pose)),
            mass: None,
            features: vec![c.id],
            source: None,
            derived: None,
        });
        props.push(p);
    }
    (parts, props)
}

/// The parts of every context of the Part Studio `studio` (see [`parts_of`]).
pub fn parts(doc: &Document, studio: ElementId, mut build_of: impl FnMut(ElementId, &[Feature]) -> Option<Arc<Build>>) -> (Vec<Part>, Vec<PartProps>) {
    let mut out = (Vec::new(), Vec::new());
    for ctx in &with_pending(doc, studio) {
        let (p, q) = parts_of(doc, ctx, &mut build_of);
        out.0.extend(p);
        out.1.extend(q);
    }
    out
}

/// The context solids of every context of the Part Studio `studio`, by feature id (for
/// regenerating its sketches, [`crate::parts::regenerate_with`]).
pub fn solids(doc: &Document, studio: ElementId) -> Vec<(FeatureId, Arc<crate::solid::Solid>)> {
    if doc.element(studio).is_none_or(|e| e.contexts.is_empty()) && pending_id(doc.id, studio).is_none() {
        return Vec::new();
    }
    let (parts, _) = parts(doc, studio, |_, f| Some(crate::rebuild::build(f)));
    parts.into_iter().map(|p| (p.feature, p.solid)).collect()
}

/// [`solids`] with the number of the context each is in (a sketch on a context face imprints
/// only its own context's geometry).
pub fn solids_by_context(doc: &Document, studio: ElementId) -> Vec<(FeatureId, ContextNo, Arc<crate::solid::Solid>)> {
    if doc.element(studio).is_none_or(|e| e.contexts.is_empty()) && pending_id(doc.id, studio).is_none() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for ctx in &with_pending(doc, studio) {
        out.extend(context_solids(doc, ctx).iter().map(|(f, s)| (*f, ctx.id, s.clone())));
    }
    out
}

/// A context's solids, cached by its snapshot (they're moved copies of its parts, made again on
/// every command in the studio otherwise: a context of hundreds of parts took most of an
/// import's time). A snapshot is identified by its fingerprint of the frozen features and its
/// parts' ids and poses; an old context (no frozen features) follows the studios and isn't
/// cached.
fn context_solids(doc: &Document, ctx: &StudioContext) -> Arc<Vec<(FeatureId, Arc<crate::solid::Solid>)>> {
    type Cache = HashMap<u64, Arc<Vec<(FeatureId, Arc<crate::solid::Solid>)>>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let make = || {
        let (parts, _) = parts_of(doc, ctx, |_, f| Some(crate::rebuild::build(f)));
        Arc::new(parts.into_iter().map(|p| (p.feature, p.solid)).collect::<Vec<_>>())
    };
    if ctx.studios.is_empty() || ctx.sources == 0 {
        return make();
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&(ctx.sources, ctx.studios.len(), ctx.parts.len()), &mut h);
    for p in &ctx.parts {
        std::hash::Hash::hash(&(p.id, p.element, p.part), &mut h);
        for v in p.pose.rotation.iter().flatten().chain(&p.pose.translation) {
            std::hash::Hash::hash(&v.to_bits(), &mut h);
        }
    }
    let key = std::hash::Hasher::finish(&h);
    if let Some(hit) = CACHE.lock().ok().and_then(|c| c.as_ref()?.get(&key).cloned()) {
        return hit;
    }
    let solids = make();
    if let Ok(mut c) = CACHE.lock() {
        let c = c.get_or_insert_with(HashMap::new);
        if c.len() >= 32 {
            c.clear();
        }
        c.insert(key, solids.clone());
    }
    solids
}

/// The feature ids of context parts a feature references: its sketch plane, its links and
/// imprints, and every face, edge, vertex or part it names.
pub fn context_refs(f: &Feature) -> Vec<FeatureId> {
    let mut out: Vec<FeatureId> = f.parents().into_iter().filter(|p| is_context(*p)).collect();
    let mut add = |id: FeatureId| {
        if is_context(id) && !out.contains(&id) {
            out.push(id);
        }
    };
    if let Some(sk) = f.sketch() {
        if let Some(cadrs_sketch::PlaneRef::Face(fp)) = sk.plane {
            add(FeatureId(fp.feature));
        }
        for (_, _, l) in sk.geometry.links() {
            add(FeatureId(l.feature()));
        }
        // Imprints are the plane's own (automatic), not references.
    }
    if let crate::document::FeatureKind::Transform(x) = &f.kind {
        for c in &x.context {
            add(c.id);
        }
    }
    if let crate::document::FeatureKind::Extrude(e) = &f.kind {
        match &e.up_to {
            Some(crate::document::UpTo::Face(r)) => add(r.part.feature),
            Some(crate::document::UpTo::Vertex(v)) => add(v.part.feature),
            _ => {}
        }
    }
    if let crate::document::FeatureKind::Revolve(r) = &f.kind {
        match &r.up_to {
            Some(crate::document::UpTo::Face(x)) => add(x.part.feature),
            Some(crate::document::UpTo::Part(p)) => add(p.feature),
            Some(crate::document::UpTo::Vertex(v)) => add(v.part.feature),
            None => {}
        }
    }
    out
}

/// The contexts of the Part Studio `el` a feature uses (MC2.6), in the studio's order.
pub fn feature_contexts(el: &Element, f: &Feature) -> Vec<ContextNo> {
    let refs = context_refs(f);
    el.contexts.iter().filter(|c| refs.iter().any(|r| c.part(*r).is_some())).map(|c| c.id).collect()
}

/// Whether any feature of the Part Studio `el` uses the context `id`.
pub fn is_referenced(el: &Element, id: ContextNo) -> bool {
    el.features().iter().any(|f| feature_contexts(el, f).contains(&id))
}

/// Adds a context to a Part Studio, numbered as given, and regenerates its sketches: one undo
/// step (Edit in context, Create Part Studio in context, New context).
#[derive(Debug, Clone)]
pub struct AddContext {
    pub studio: ElementId,
    pub context: StudioContext,
}

impl Command for AddContext {
    fn label(&self) -> String {
        "Edit in context".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.studio)?;
        if el.context(self.context.id).is_some() {
            return Err(CommandError::Invalid(format!("context {} already exists", self.context.id)));
        }
        el.contexts.push(self.context.clone());
        crate::commands::refresh_studio(doc, self.studio);
        Ok(())
    }
}

/// Replaces a context by a new snapshot of it (Update context, MC4.6, MC4.7) and regenerates the
/// studio: the features that reference it follow. One undo step.
#[derive(Debug, Clone)]
pub struct UpdateContext {
    pub studio: ElementId,
    pub context: StudioContext,
}

impl Command for UpdateContext {
    fn label(&self) -> String {
        "Update context".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.studio)?;
        let c = el.context_mut(self.context.id).ok_or_else(|| CommandError::Invalid("no such context".into()))?;
        *c = self.context.clone();
        crate::commands::refresh_studio(doc, self.studio);
        Ok(())
    }
}

/// Deletes a context (MC2.13): the features that referenced it keep what they last had (a
/// sketch stays where it was; an extrude up to it fails). One undo step.
#[derive(Debug, Clone)]
pub struct RemoveContext {
    pub studio: ElementId,
    pub id: ContextNo,
}

impl Command for RemoveContext {
    fn label(&self) -> String {
        "Delete context".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.studio)?;
        let n = el.contexts.len();
        el.contexts.retain(|c| c.id != self.id);
        if el.contexts.len() == n {
            return Err(CommandError::Invalid("no such context".into()));
        }
        Ok(())
    }
}

/// Renames a context (MC2.13). One undo step.
#[derive(Debug, Clone)]
pub struct RenameContext {
    pub studio: ElementId,
    pub id: ContextNo,
    pub name: String,
}

impl Command for RenameContext {
    fn label(&self) -> String {
        "Rename context".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.studio)?;
        let c = el.context_mut(self.id).ok_or_else(|| CommandError::Invalid("no such context".into()))?;
        c.name = self.name.trim().to_string();
        Ok(())
    }
}

/// **Set as primary instance** (MC3.3): another instance of a part of the contexts' studio
/// anchors the contexts `contexts` from their next update. One undo step.
#[derive(Debug, Clone)]
pub struct SetPrimaryInstance {
    pub studio: ElementId,
    pub contexts: Vec<ContextNo>,
    pub instance: InstanceId,
}

impl Command for SetPrimaryInstance {
    fn label(&self) -> String {
        "Set as primary instance".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        for id in &self.contexts {
            let assembly = doc.element(self.studio).and_then(|e| e.context(*id)).map(|c| c.assembly).ok_or_else(|| CommandError::Invalid("no such context".into()))?;
            if studio_of(doc, assembly, self.instance) != Some(self.studio) {
                return Err(CommandError::Invalid("the primary instance must be a part of this Part Studio".into()));
            }
            let el = studio_mut(doc, self.studio)?;
            el.context_mut(*id).expect("checked").instance = self.instance;
        }
        Ok(())
    }
}

/// Sets (Edit in context, Update context) a Part Studio's context `context.id`, adding it when
/// it has none by that number, or clears every context (`None`), and regenerates its sketches:
/// one undo step.
#[derive(Debug, Clone)]
pub struct SetStudioContext {
    pub studio: ElementId,
    pub context: Option<StudioContext>,
}

impl Command for SetStudioContext {
    fn label(&self) -> String {
        if self.context.is_some() { "Update context".into() } else { "Remove context".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.studio)?;
        match &self.context {
            Some(c) => match el.context_mut(c.id) {
                Some(old) => *old = c.clone(),
                None => el.contexts.push(c.clone()),
            },
            None => el.contexts.clear(),
        }
        resnapshot_context_copies(doc, self.studio);
        crate::commands::refresh_studio(doc, self.studio);
        Ok(())
    }
}

/// P3H.6: after the Part Studio `studio`'s context changed (Update context), every Transform in
/// it that copies context parts takes a new snapshot of them (the source studios' features and
/// where the context has the parts now), matched by context id; a part the context no longer
/// has is dropped from the Transform. Part of the same undo step.
fn resnapshot_context_copies(doc: &mut Document, studio: ElementId) {
    let Some(features) = doc.element(studio).map(|e| e.features()) else { return };
    let updated: Vec<(usize, crate::transform::TransformFeature)> = features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| match &f.kind {
            crate::document::FeatureKind::Transform(x) if !x.context.is_empty() => {
                let mut y = x.clone();
                y.set_picked(doc, studio, &x.picked());
                (y != *x).then_some((i, y))
            }
            _ => None,
        })
        .collect();
    let Some(features) = doc.element_mut(studio).and_then(|e| e.features_mut()) else { return };
    for (i, y) in updated {
        if let Some(f) = features.get_mut(i) {
            f.kind = crate::document::FeatureKind::Transform(y);
        }
    }
}

/// Shows or hides a context's geometry (its eye): one undo step.
#[derive(Debug, Clone)]
pub struct SetContextHidden {
    pub studio: ElementId,
    pub id: ContextNo,
    pub hidden: bool,
}

impl Command for SetContextHidden {
    fn label(&self) -> String {
        if self.hidden { "Hide context".into() } else { "Show context".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.studio)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.studio)?;
        let c = el.context_mut(self.id).ok_or_else(|| CommandError::Invalid("no context".into()))?;
        c.hidden = self.hidden;
        Ok(())
    }
}

fn studio_mut(doc: &mut Document, studio: ElementId) -> Result<&mut Element, CommandError> {
    let el = doc.element_mut(studio).ok_or(CommandError::ElementNotFound(studio))?;
    if el.features_mut().is_none() {
        return Err(CommandError::Invalid("not a Part Studio".into()));
    }
    Ok(el)
}

/// The Part Studios with a context in the assembly `assembly`, and which of their contexts.
pub fn contexts_in(doc: &Document, assembly: ElementId) -> Vec<(ElementId, Vec<ContextNo>)> {
    doc.elements
        .iter()
        .filter_map(|e| {
            let ids: Vec<ContextNo> = e.contexts.iter().filter(|c| c.assembly == assembly && c.document.is_none()).map(|c| c.id).collect();
            (!ids.is_empty()).then_some((e.id, ids))
        })
        .collect()
}

/// **Where used** (X15): the Assembly tabs that use the Part Studio `element` (only its part
/// `part`, when given), each with how many of its parts at any depth come from it and whether any
/// is a direct instance (else through a subassembly).
pub fn where_used(doc: &Document, element: ElementId, part: Option<PartId>) -> Vec<(ElementId, usize, bool)> {
    let mut out = Vec::new();
    for e in &doc.elements {
        let Some(asm) = e.assembly_model() else { continue };
        let occ: Vec<_> = occurrences(doc, asm).into_iter().filter(|o| o.element == element && part.is_none_or(|p| o.part == p)).collect();
        if occ.is_empty() {
            continue;
        }
        let direct = occ.iter().any(|o| o.child.is_none());
        out.push((e.id, occ.len(), direct));
    }
    out
}

/// **Where used** of an Assembly tab: the Assembly tabs that hold it as a subassembly (directly).
pub fn assembly_used_in(doc: &Document, element: ElementId) -> Vec<(ElementId, usize, bool)> {
    doc.elements
        .iter()
        .filter_map(|e| {
            let n = e.assembly_model()?.instances.iter().filter(|i| i.source == InstanceSource::Assembly { element }).count();
            (n > 0).then_some((e.id, n, true))
        })
        .collect()
}

/// A sketch plane on the planar face `face` of the solid `s`, a part made by `feature` (a
/// context part's, [`context_id`]): the face's frame and a point on it now.
pub fn face_plane_on(s: &crate::solid::Solid, feature: FeatureId, face: cadrs_sketch::FaceName) -> Option<cadrs_sketch::PlaneRef> {
    let i = s.faces.iter().position(|f| f.name == face)?;
    let frame = s.faces[i].plane?;
    Some(cadrs_sketch::PlaneRef::Face(cadrs_sketch::FacePlane {
        feature: feature.0,
        face,
        origin: frame.origin,
        u: frame.u,
        v: frame.v,
        seed: s.face_point(i),
    }))
}

/// Reads `Element::contexts`: a list, or the single `context: Some(...)` / `None` of documents
/// from before several contexts.
pub(crate) fn deserialize_contexts<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<StudioContext>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<StudioContext>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of contexts, or an optional context")
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            Ok(vec![StudioContext::deserialize(d)?])
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(c) = seq.next_element()? {
                out.push(c);
            }
            Ok(out)
        }
    }
    d.deserialize_any(V)
}

/// A context part an Extrude or Revolve end goes **up to** (MC1.3: up to a face, part or vertex
/// of the context), frozen in the feature as its context has it, so the rebuild can build the
/// part without the context (and the rebuild cache sees an Update context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextTarget {
    /// The context part's feature id ([`context_id`]): the end's reference names it.
    pub part: FeatureId,
    /// Its Part Studio's features as the context has them, and the part there.
    pub source: ContextSource,
    pub source_part: PartId,
    /// Where the context has it (studio coordinates).
    pub pose: Pose,
}

/// The context parts an Extrude's or Revolve's ends name.
fn up_to_context_parts(ends: &[&Option<crate::document::UpTo>]) -> Vec<FeatureId> {
    let mut out = Vec::new();
    for u in ends.iter().filter_map(|u| u.as_ref()) {
        let f = match u {
            crate::document::UpTo::Face(r) => r.part.feature,
            crate::document::UpTo::Part(p) => p.feature,
            crate::document::UpTo::Vertex(v) => v.part.feature,
        };
        if is_context(f) && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// The [`ContextTarget`] of the context part `part` of the Part Studio `el`, as its context has
/// it now.
fn target(doc: &Document, el: &Element, part: FeatureId) -> Option<ContextTarget> {
    let (ctx, cp) = find_part(doc, el.id, part)?;
    let features = source_features(doc, &ctx, cp.element)?;
    Some(ContextTarget { part, source: ContextSource { element: cp.element, features }, source_part: cp.part, pose: cp.pose })
}

/// Brings the [`ContextTarget`]s of the Part Studio `studio`'s features in line with its
/// contexts: an end up to a context part gets the part as its context has it; one whose
/// context is gone keeps what it had (MC1.6: nothing changes until the user says so).
pub fn refresh_targets(doc: &mut Document, studio: ElementId) {
    let Some(el) = doc.element(studio) else { return };
    let mut updates: Vec<(FeatureId, Vec<ContextTarget>)> = Vec::new();
    for f in el.features() {
        let (wanted, have) = match &f.kind {
            crate::document::FeatureKind::Extrude(e) => (up_to_context_parts(&[&e.up_to, &e.second.as_ref().and_then(|s| s.up_to)]), &e.context),
            crate::document::FeatureKind::Revolve(r) => (up_to_context_parts(&[&r.up_to, &r.second.as_ref().and_then(|s| s.up_to)]), &r.context),
            _ => continue,
        };
        let now: Vec<ContextTarget> = wanted
            .iter()
            .filter_map(|p| target(doc, el, *p).or_else(|| have.iter().find(|t| t.part == *p).cloned()))
            .collect();
        if &now != have {
            updates.push((f.id, now));
        }
    }
    let Some(features) = doc.element_mut(studio).and_then(|e| e.features_mut()) else { return };
    for (id, now) in updates {
        match features.iter_mut().find(|f| f.id == id).map(|f| &mut f.kind) {
            Some(crate::document::FeatureKind::Extrude(e)) => e.context = now,
            Some(crate::document::FeatureKind::Revolve(r)) => r.context = now,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// MC5: the assembly in another document

/// The Part Studio in another document a linked instance of `assembly` comes from: (its
/// document, its element there).
pub fn linked_studio_of(doc: &Document, assembly: ElementId, instance: InstanceId) -> Option<(DocumentId, ElementId)> {
    let inst = doc.element(assembly)?.assembly_model()?.instance(instance)?;
    let r = inst.link?;
    if !r.is_external(doc.id) || inst.source.is_assembly() {
        return None;
    }
    Some((r.document?, r.element))
}

/// A context `id` of the linked Part Studio of `instance` (MC5.1): the snapshot taken in the
/// assembly's document, naming it.
pub fn snapshot_linked(asm_doc: &Document, assembly: ElementId, instance: InstanceId, id: ContextNo) -> Result<StudioContext, CommandError> {
    let mut c = snapshot_as(asm_doc, assembly, instance, id)?;
    c.document = Some(asm_doc.id);
    Ok(c)
}

/// A context whose assembly is in another document, as that document (`asm_doc`) has the
/// assembly now (Update context, MC5.8): anchored on the primary instance, the linked studio's
/// own parts left out.
pub fn resnapshot_external(asm_doc: &Document, ctx: &StudioContext) -> Result<StudioContext, CommandError> {
    let asm = asm_doc.element(ctx.assembly).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(ctx.assembly))?;
    let inst = asm.instance(ctx.instance).filter(|i| !i.suppressed).ok_or(CommandError::Invalid(NO_PRIMARY.into()))?;
    let studio = studio_of(asm_doc, ctx.assembly, ctx.instance).ok_or(CommandError::Invalid(NO_PRIMARY.into()))?;
    let mut now = snapshot_with(asm_doc, ctx.assembly, studio, ctx.instance, inst.pose.inverse(), ctx.id)?;
    now.name = ctx.name.clone();
    now.hidden = ctx.hidden;
    now.document = ctx.document;
    Ok(now)
}

/// [`status`] of a context whose assembly is in `asm_doc`.
pub fn external_status(asm_doc: &Document, ctx: &StudioContext) -> ContextStatus {
    match resnapshot_external(asm_doc, ctx) {
        Ok(now) => {
            let moved = now.parts.iter().map(|p| (p.id, p.element, p.part, p.pose)).ne(ctx.parts.iter().map(|p| (p.id, p.element, p.part, p.pose)));
            if moved || now.studios != ctx.studios { ContextStatus::OutOfDate } else { ContextStatus::UpToDate }
        }
        Err(CommandError::Invalid(m)) if m == NO_PRIMARY => ContextStatus::NoPrimary,
        Err(_) => ContextStatus::Unknown,
    }
}

/// **Update context from the assembly** of a linked Part Studio (MC5.8.1): the context `id` of
/// the studio `studio` in the document `part_doc` takes the snapshot of the assembly as
/// `asm_doc` has it, in that document's workspace (saved, logged), and a new version of it is
/// made ("Created by Update context in <assembly document>"). The caller then points the
/// assembly's primary instance at that version. Those writes belong to the other document:
/// undo in the assembly re-points the instance but keeps the version.
pub fn update_linked_context(
    res: &mut crate::external::Resolver,
    asm_doc: &Document,
    part_doc: DocumentId,
    studio: ElementId,
    id: ContextNo,
    now: crate::library::Timestamp,
    user: &str,
) -> Result<crate::history_log::VersionId, crate::external::LinkError> {
    use crate::external::LinkError;
    use crate::history_log::{HistoryLog, Origin};
    let store = res.store().clone();
    let mut file = store.load(part_doc).map_err(|_| LinkError::State(res.state(part_doc)))?;
    let ctx = file.document.element(studio).and_then(|e| e.context(id)).cloned().ok_or_else(|| LinkError::NotFound("The context is gone from the linked Part Studio".into()))?;
    let fresh = resnapshot_external(asm_doc, &ctx).map_err(|e| LinkError::NotFound(e.to_string()))?;
    let mut h = crate::command::History::default();
    if fresh != ctx {
        h.execute(&mut file.document, &UpdateContext { studio, context: fresh }).map_err(|e| LinkError::NotFound(e.to_string()))?;
        file.meta.modified = now;
        store.save(&file.document, &file.meta).map_err(|e| LinkError::NotFound(format!("{} ({e})", file.document.name)))?;
    }
    let mut log = HistoryLog::load(&store, part_doc).ok().flatten().unwrap_or_else(|| HistoryLog::start(&file.document, file.meta.created, user));
    if log.head() != &file.document {
        log.record(&file.document, Origin::Command("Update context".into()), now, user);
    }
    let v = log.create_auto_version(&format!("Created by Update context in {}", asm_doc.name), now, user);
    log.save(&store).map_err(|e| LinkError::NotFound(format!("The history ({e})")))?;
    res.forget(part_doc);
    Ok(v)
}
