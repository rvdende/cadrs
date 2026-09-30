//! **Edit in context** (P3B.9, `intro-to-assemblies.md` X15): a Part Studio opened from an
//! instance of its part in an assembly shows the rest of the assembly around that part, in the
//! studio's coordinates, as **context geometry** its sketches can use: sketch on a face of
//! another instance, Use (project) its edges.
//!
//! The context ([`StudioContext`], on the Part Studio's element) is a **snapshot**, as Onshape's
//! is: which parts of the assembly are around the edited instance and where they were relative
//! to it when the context was made or last **updated** ([`snapshot`], [`SetStudioContext`]). The
//! parts themselves are drawn and referenced from their Part Studios' current rebuild. Moving
//! instances in the assembly changes nothing in the studio until **Update context**, which takes
//! a new snapshot and regenerates the studio: a sketch on a context face, and edges used from the
//! context, move to where the geometry is now.
//!
//! Each context part is a part of the studio's view with its own feature id ([`context_id`],
//! recognisable by [`is_context`]), so sketch planes ([`cadrs_sketch::FacePlane`]) and links
//! ([`cadrs_sketch::Link`]) name it like any part: the regeneration
//! ([`crate::parts::regenerate_with`]) is given the context's solids ([`solids`]).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::structure::occurrences;
use super::{InstanceId, InstanceSource, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, PartProps};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::parts::Part;
use crate::rebuild::Build;

/// The top 32 bits of every context part's feature id.
const MAGIC: u128 = 0xC0A7_E0C7;

/// The feature id of the context part standing for the assembly occurrence `occurrence`.
pub fn context_id(occurrence: InstanceId) -> FeatureId {
    FeatureId(Uuid::from_u128((occurrence.0.as_u128() & !(0xFFFF_FFFFu128 << 96)) | (MAGIC << 96)))
}

/// Whether a feature id is a context part's.
pub fn is_context(feature: FeatureId) -> bool {
    feature.0.as_u128() >> 96 == MAGIC
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
}

/// A Part Studio's assembly context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioContext {
    /// The assembly and the instance (of a part of this studio) it was opened from.
    pub assembly: ElementId,
    pub instance: InstanceId,
    pub parts: Vec<ContextPart>,
    /// The context geometry is hidden (its eye); it is still there for references.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// P3H.5 (X9): a fingerprint of the context parts' Part Studios' features when the snapshot
    /// was taken ([`sources_fingerprint`]), so an edit of their geometry (not only a move)
    /// makes the context out of date ("Assembly changed", Update context). 0: not recorded.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sources: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
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

/// The context of the Part Studio of `instance` in `assembly` as the assembly is now: every
/// other part at any depth that isn't from this studio, placed relative to the instance.
pub fn snapshot(doc: &Document, assembly: ElementId, instance: InstanceId) -> Result<StudioContext, CommandError> {
    let asm = doc.element(assembly).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(assembly))?;
    let inst = asm.instance(instance).ok_or_else(|| CommandError::Invalid(format!("instance {instance} not found")))?;
    let studio = match inst.source {
        InstanceSource::Part { element, .. } | InstanceSource::Studio { element } => element,
        InstanceSource::Assembly { .. } => return Err(CommandError::Invalid("a subassembly is edited in its own tab".into())),
    };
    let to_studio = inst.pose.inverse();
    let parts = occurrences(doc, asm)
        .into_iter()
        .filter(|o| o.element != studio)
        .map(|o| ContextPart {
            id: context_id(o.id),
            element: o.element,
            part: o.part,
            pose: o.pose.then(&to_studio),
            name: format!("{} <{}>", super::source_part_name(doc, &InstanceSource::Part { element: o.element, part: o.part }, None), o.index),
        })
        .collect::<Vec<_>>();
    let sources = sources_fingerprint(doc, &parts);
    Ok(StudioContext { assembly, instance, parts, hidden: false, sources })
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

/// The context parts of the Part Studio `studio` as parts of its view (studio coordinates),
/// with their settings (appearance): each is its source part moved to where the context has it.
/// `build_of` gives each source Part Studio's rebuild.
pub fn parts(doc: &Document, studio: ElementId, mut build_of: impl FnMut(ElementId) -> Option<Arc<Build>>) -> (Vec<Part>, Vec<PartProps>) {
    let Some(ctx) = doc.element(studio).and_then(|e| e.context.as_ref()) else {
        return (Vec::new(), Vec::new());
    };
    let mut parts = Vec::new();
    let mut props = Vec::new();
    for c in &ctx.parts {
        let Some(build) = build_of(c.element) else { continue };
        let Some(src) = build.part(c.part) else { continue };
        let id = PartId::new(c.id, 0);
        let mut p = PartProps::new(id);
        p.name = Some(c.name.clone());
        if let Some(sp) = doc.element(c.element).and_then(|e| e.part_prop(c.part)) {
            p.appearance = sp.appearance;
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

/// The context solids of the Part Studio `studio`, by feature id (for regenerating its
/// sketches, [`crate::parts::regenerate_with`]); rebuilt from the other studios.
pub fn solids(doc: &Document, studio: ElementId) -> Vec<(FeatureId, Arc<crate::solid::Solid>)> {
    if doc.element(studio).and_then(|e| e.context.as_ref()).is_none() {
        return Vec::new();
    }
    let mut builds: std::collections::HashMap<ElementId, Arc<Build>> = std::collections::HashMap::new();
    let (parts, _) = parts(doc, studio, |e| {
        if let Some(b) = builds.get(&e) {
            return Some(b.clone());
        }
        let b = crate::rebuild::build(&doc.element(e)?.active_features());
        builds.insert(e, b.clone());
        Some(b)
    });
    parts.into_iter().map(|p| (p.feature, p.solid)).collect()
}

/// Sets (Edit in context, Update context) or clears a Part Studio's assembly context, and
/// regenerates its sketches against it: one undo step.
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
        let el = doc.element_mut(self.studio).ok_or(CommandError::ElementNotFound(self.studio))?;
        if el.features_mut().is_none() {
            return Err(CommandError::Invalid("not a Part Studio".into()));
        }
        el.context = self.context.clone();
        crate::commands::refresh_studio(doc, self.studio);
        Ok(())
    }
}

/// Shows or hides the context geometry (its eye): one undo step.
#[derive(Debug, Clone)]
pub struct SetContextHidden {
    pub studio: ElementId,
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
        let el = doc.element_mut(self.studio).ok_or(CommandError::ElementNotFound(self.studio))?;
        let c = el.context.as_mut().ok_or_else(|| CommandError::Invalid("no context".into()))?;
        c.hidden = self.hidden;
        Ok(())
    }
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
