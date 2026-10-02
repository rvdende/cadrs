//! **Managed in-context design** (P3H.5, `pcb-studio.md` X9, PCB5.7, PCB6 steps 2–8): the part
//! of Onshape's in-context workflow that Edit in context ([`super::context`], P3B.9) doesn't
//! cover.
//!
//! - **Create Part Studio in context** ([`CreateStudioInContext`]): from an assembly, a new Part
//!   Studio tab whose context is the whole assembly, placed relative to the chosen origin (the
//!   assembly **Origin**, [`InstanceId::ORIGIN`]): the studio's coordinates are the assembly's.
//!   Its sketches can sit on the context geometry and Use its edges, like Edit in context.
//! - **Insert and go to Assembly** ([`InsertFromStudio`]): the picked parts of such a studio are
//!   inserted into its context's assembly where the studio has them (the context origin's
//!   placement), one undo step.
//! - **Update context** works for both kinds of context: [`resnapshot`] takes the assembly as it
//!   is now (for an origin context: every part not of the studio itself, in assembly
//!   coordinates), and the snapshot records a fingerprint of the context parts' Part Studios
//!   ([`super::context::sources_fingerprint`]), so resizing the enclosure makes the context out
//!   of date just as moving an instance does.

use super::commands::InsertInstance;
use super::context::{ContextNo, StudioContext, snapshot_with};
use super::{Instance, InstanceId, InstanceSource};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element};
use crate::ids::{ElementId, PartId};

/// The context of the Part Studio `studio` created in `assembly` at the assembly's Origin: every
/// part of the assembly (at any depth) that isn't from `studio`, in assembly coordinates. Numbered
/// 0 (see [`snapshot_origin_as`]).
pub fn snapshot_origin(doc: &Document, assembly: ElementId, studio: ElementId) -> Result<StudioContext, CommandError> {
    snapshot_origin_as(doc, assembly, studio, 0, super::Pose::IDENTITY)
}

/// [`snapshot_origin`] as the studio's context `id`, the studio's origin at `origin` in the
/// assembly (the Origin, or a mate connector's frame, MC2.9).
pub fn snapshot_origin_as(doc: &Document, assembly: ElementId, studio: ElementId, id: ContextNo, origin: super::Pose) -> Result<StudioContext, CommandError> {
    snapshot_with(doc, assembly, studio, InstanceId::ORIGIN, origin.inverse(), id)
}

pub use super::context::resnapshot;

/// Where the context's origin is in the assembly: the Origin's identity, or the edited
/// instance's placement.
pub fn origin_pose(doc: &Document, ctx: &StudioContext) -> Option<super::Pose> {
    if ctx.instance == InstanceId::ORIGIN {
        return Some(ctx.origin);
    }
    Some(doc.element(ctx.assembly)?.assembly_model()?.instance(ctx.instance)?.pose)
}

/// The name of the context's origin ("Origin", or the instance's name).
pub fn origin_name(doc: &Document, ctx: &StudioContext) -> Option<String> {
    if ctx.instance == InstanceId::ORIGIN {
        return Some("Origin".into());
    }
    let inst = doc.element(ctx.assembly)?.assembly_model()?.instance(ctx.instance)?;
    Some(inst.name(&super::source_part_name(doc, &inst.source, None)))
}

/// **Create Part Studio in context** (X9, PCB6 step 2): a new Part Studio tab right of the
/// assembly, its context the assembly at its Origin. One undo step (the whole document).
#[derive(Debug, Clone)]
pub struct CreateStudioInContext {
    pub assembly: ElementId,
    /// The new tab's id, chosen by the caller so it can switch to it.
    pub studio: ElementId,
    /// `None`: the next "Part Studio n".
    pub name: Option<String>,
    /// Where the new studio's origin is in the assembly (MC2.9): the Origin
    /// ([`super::Pose::IDENTITY`]) or a mate connector's frame.
    pub origin: super::Pose,
}

impl Command for CreateStudioInContext {
    fn label(&self) -> String {
        "Create Part Studio in context".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.element(self.assembly).and_then(|e| e.assembly_model()).is_none() {
            return Err(CommandError::Invalid("not an Assembly".into()));
        }
        if doc.element(self.studio).is_some() {
            return Err(CommandError::Invalid("element id already in use".into()));
        }
        let name = match &self.name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => doc.next_element_name("Part Studio"),
        };
        let ctx = snapshot_origin_as(doc, self.assembly, self.studio, 0, self.origin)?;
        let mut el = Element::part_studio(name);
        el.id = self.studio;
        el.contexts = vec![ctx];
        let at = doc.element_index(self.assembly).map_or(doc.elements.len(), |i| i + 1);
        doc.elements.insert(at, el);
        crate::commands::refresh_studio(doc, self.studio);
        Ok(())
    }
}

/// **Insert and go to Assembly** (X9, PCB6 step 7): the parts `parts` of the in-context Part
/// Studio `studio` as instances of its context's assembly, where the studio has them (the
/// context origin's placement). One undo step (the assembly).
#[derive(Debug, Clone)]
pub struct InsertFromStudio {
    pub studio: ElementId,
    pub parts: Vec<PartId>,
    /// The new instances' ids (one per part), chosen by the caller.
    pub instances: Vec<InstanceId>,
}

impl InsertFromStudio {
    /// The assembly the parts go into.
    pub fn assembly(&self, doc: &Document) -> Option<ElementId> {
        doc.element(self.studio)?.contexts.first().map(|c| c.assembly)
    }
}

impl Command for InsertFromStudio {
    fn label(&self) -> String {
        if self.parts.len() == 1 { "Insert part".into() } else { format!("Insert {} parts", self.parts.len()) }
    }
    fn scope(&self) -> Scope {
        // The assembly (found from the studio's context).
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let ctx = doc
            .element(self.studio)
            .and_then(|e| e.contexts.first().cloned())
            .ok_or_else(|| CommandError::Invalid("the Part Studio has no assembly context".into()))?;
        if self.parts.is_empty() {
            return Err(CommandError::Invalid("pick the parts to insert".into()));
        }
        if self.parts.len() != self.instances.len() {
            return Err(CommandError::Invalid("one instance id per part".into()));
        }
        let pose = origin_pose(doc, &ctx).ok_or_else(|| CommandError::Invalid("the context's origin is gone".into()))?;
        for (part, id) in self.parts.iter().zip(&self.instances) {
            let insert = InsertInstance { element: ctx.assembly, instance: Instance::new(*id, InstanceSource::Part { element: self.studio, part: *part }, pose) };
            insert.apply(doc)?;
        }
        // MC3.3: the first part of the studio in the assembly becomes the primary instance of
        // the contexts that had the Origin; it is where the Origin was, so nothing moves.
        if let Some(first) = self.instances.first()
            && let Some(el) = doc.element_mut(self.studio)
        {
            for c in el.contexts.iter_mut().filter(|c| c.assembly == ctx.assembly && c.instance == InstanceId::ORIGIN) {
                c.instance = *first;
            }
        }
        Ok(())
    }
}
