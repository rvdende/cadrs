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
use super::context::{ContextPart, StudioContext, context_id, snapshot, sources_fingerprint};
use super::structure::occurrences;
use super::{Instance, InstanceId, InstanceSource};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element};
use crate::ids::{ElementId, PartId};

/// The context of the Part Studio `studio` created in `assembly` at the assembly's Origin: every
/// part of the assembly (at any depth) that isn't from `studio`, in assembly coordinates.
pub fn snapshot_origin(doc: &Document, assembly: ElementId, studio: ElementId) -> Result<StudioContext, CommandError> {
    let asm = doc.element(assembly).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(assembly))?;
    let parts: Vec<ContextPart> = occurrences(doc, asm)
        .into_iter()
        .filter(|o| o.element != studio)
        .map(|o| ContextPart {
            id: context_id(o.id),
            element: o.element,
            part: o.part,
            pose: o.pose,
            name: format!("{} <{}>", super::source_part_name(doc, &InstanceSource::Part { element: o.element, part: o.part }, None), o.index),
        })
        .collect();
    let sources = sources_fingerprint(doc, &parts);
    Ok(StudioContext { assembly, instance: InstanceId::ORIGIN, parts, hidden: false, sources })
}

/// The context of `studio` (whose current context is `ctx`) as the assembly is now: Update
/// context. Keeps the eye's state.
pub fn resnapshot(doc: &Document, studio: ElementId, ctx: &StudioContext) -> Result<StudioContext, CommandError> {
    let mut now = if ctx.instance == InstanceId::ORIGIN { snapshot_origin(doc, ctx.assembly, studio)? } else { snapshot(doc, ctx.assembly, ctx.instance)? };
    now.hidden = ctx.hidden;
    Ok(now)
}

/// Where the context's origin is in the assembly: the Origin's identity, or the edited
/// instance's placement.
pub fn origin_pose(doc: &Document, ctx: &StudioContext) -> Option<super::Pose> {
    if ctx.instance == InstanceId::ORIGIN {
        return Some(super::Pose::IDENTITY);
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
        let ctx = snapshot_origin(doc, self.assembly, self.studio)?;
        let mut el = Element::part_studio(name);
        el.id = self.studio;
        el.context = Some(ctx);
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
        doc.element(self.studio)?.context.as_ref().map(|c| c.assembly)
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
            .and_then(|e| e.context.clone())
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
        Ok(())
    }
}
