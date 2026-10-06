//! **Managed in-context design with linked documents** (`managed-in-context-design.md` MC5;
//! [`cadrs_core::assembly::context`]'s MC5 functions).
//!
//! - **Edit in context** on an instance of a part from another document (a version reference)
//!   opens that document's **workspace** in this window, editable, with its Part Studio active
//!   and the assembly's ghost shown (MC5.1): the snapshot is taken in the assembly's document
//!   and names it ([`StudioContext::document`]). As within a document, a new context is pending
//!   until a feature references it (MC5.3). The assembly's document is kept as it was to go
//!   back to ([`InContextSession`]).
//! - **Go to assembly** goes back without committing: the assembly still uses its version
//!   (MC5.4). **Create version and go to assembly** makes a version of the part's document,
//!   goes back and points the context's primary instance at it (MC5.5).
//! - **Update context from the assembly** (MC5.8.1, the instance menu): the other document's
//!   context takes the snapshot, a version of it is made, and the primary instance is pointed at
//!   it (one undo step here; the version stays). **From the Part Studio** (MC5.8.2, the bar or
//!   the ⋯ menu): the context takes the snapshot of the assembly as its document is now, an
//!   ordinary undo step, no version.
//!
//! Names: `context-go-menu-version`.

use bevy::prelude::*;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::context::{self, ContextNo, StudioContext};
use cadrs_core::link_update::{RefSite, Target, UpdateReferences};
use cadrs_core::{Document, DocumentId, ElementId};

use crate::{ActiveDocument, AppClock, DocumentStore, UserProfile};

/// A linked part's document opened for editing in context from an assembly's document.
#[derive(Resource, Clone)]
pub struct InContextSession {
    /// The assembly's document as it was (its tab, its undo history).
    pub back: ActiveDocument,
    pub assembly: ElementId,
    /// The part's document and its Part Studio.
    pub part_doc: DocumentId,
    pub studio: ElementId,
}

/// The workspace of another document, as the resolver reads it from the store (the session's
/// assembly document as it is in memory).
pub fn document(world: &mut World, id: DocumentId) -> Option<std::sync::Arc<Document>> {
    if let Some(s) = world.get_resource::<InContextSession>()
        && s.back.doc.id == id
    {
        return Some(std::sync::Arc::new(s.back.doc.clone()));
    }
    crate::linked::resolver(world).0.current(id)
}

/// The contexts of the linked Part Studio of `instance` made in `assembly` (for the instance
/// menu), with their status.
pub fn contexts_of(world: &mut World, assembly: ElementId, instance: InstanceId) -> Vec<super::managed_context::InstanceContext> {
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let Some((d, studio)) = context::linked_studio_of(&doc, assembly, instance) else { return Vec::new() };
    let Some(part_doc) = document(world, d) else { return Vec::new() };
    let Some(el) = part_doc.element(studio) else { return Vec::new() };
    el.contexts
        .iter()
        .filter(|c| c.document == Some(doc.id) && c.assembly == assembly)
        .map(|c| {
            let status = context::external_status(&doc, c);
            super::managed_context::InstanceContext {
                id: c.id,
                label: c.label(),
                primary: c.instance == instance,
                stale: status == context::ContextStatus::OutOfDate,
                no_primary: status == context::ContextStatus::NoPrimary,
            }
        })
        .collect()
}

/// **Edit in context** of a linked instance (MC5.1): its document's workspace, editable, with
/// the context.
pub fn edit_linked(world: &mut World, assembly: ElementId, instance: InstanceId, which: super::in_context::Which) {
    let asm_doc = world.resource::<ActiveDocument>().doc.clone();
    let Some((d, studio)) = context::linked_studio_of(&asm_doc, assembly, instance) else { return };
    let store = world.resource::<DocumentStore>().0.clone();
    let file = match store.load(d) {
        Ok(f) => f,
        Err(e) => {
            crate::linked::error_toast(world, format!("The linked document can't be opened: {e}"));
            return;
        }
    };
    let Some(el) = file.document.element(studio) else {
        crate::linked::error_toast(world, "The linked Part Studio is no longer in its document");
        return;
    };
    let (id, pending) = match which {
        super::in_context::Which::Context(id) => {
            if el.context(id).is_none() {
                return;
            }
            (id, None)
        }
        super::in_context::Which::New => {
            let id = el.next_context_id();
            match context::snapshot_linked(&asm_doc, assembly, instance, id) {
                Ok(c) => (id, Some(c)),
                Err(e) => {
                    crate::linked::error_toast(world, e.to_string());
                    return;
                }
            }
        }
    };
    // Save what is open, and keep it to go back to.
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let mut back = world.resource::<ActiveDocument>().clone();
    if let Err(e) = back.save_if_changed(&store, now, &user) {
        warn!("cannot save before editing in context: {e}");
    }
    let mut open = ActiveDocument::stored(file.document, file.meta);
    open.set_active(studio);
    if let Some(c) = pending {
        context::set_pending(d, studio, Some(c));
    }
    world.insert_resource(open);
    world.insert_resource(InContextSession { back, assembly, part_doc: d, studio });
    world.resource_mut::<super::in_context::ActiveContexts>().0.insert(studio, id);
    world.resource_mut::<crate::viewport::Selection>().0.clear();
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
}

/// **Go to assembly** from a linked part's document (MC5.4), or (`version`) **Create version and
/// go to assembly** (MC5.5): its primary instance then uses the new version.
pub fn go_back(world: &mut World, version: bool) {
    let Some(session) = world.remove_resource::<InContextSession>() else { return };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let (part_doc, studio) = (session.part_doc, session.studio);
    // The context the part's document shows (its primary instance gets the version).
    let ctx: Option<StudioContext> = {
        let doc = &world.resource::<ActiveDocument>().doc;
        world.resource::<super::in_context::ActiveContexts>().0.get(&studio).and_then(|id| doc.element(studio)?.context(*id).cloned())
    };
    if let Err(e) = world.resource_mut::<ActiveDocument>().save_if_changed(&store, now, &user) {
        warn!("cannot save the linked document: {e}");
    }
    let made = version.then(|| make_version(world)).flatten();
    context::set_pending(part_doc, studio, None);
    world.resource_mut::<super::in_context::ActiveContexts>().0.remove(&studio);
    world.insert_resource(session.back);
    world.resource_mut::<ActiveDocument>().set_active(session.assembly);
    crate::linked::resolver(world).0.forget(part_doc);
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
    if let (Some(v), Some(ctx)) = (made, ctx) {
        point_at(world, session.assembly, ctx.instance, v, "Create version and go to assembly");
    }
}

/// A version of the open document (the next "V<n>"), stored.
fn make_version(world: &mut World) -> Option<cadrs_core::history_log::VersionId> {
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let store = world.resource::<DocumentStore>().0.clone();
    let mut l = world.resource_mut::<crate::history_panel::DocLog>();
    let log = l.log.as_mut()?;
    let v = log.create_version("", "", now, &user);
    let _ = log.save(&store);
    l.generation += 1;
    Some(v)
}

/// Points the instance `instance` of `assembly` (in the open document) at version `v` of its
/// source: one undo step.
fn point_at(world: &mut World, assembly: ElementId, instance: InstanceId, v: cadrs_core::history_log::VersionId, label: &str) {
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let Some(u) = cadrs_core::link_update::use_at(&doc, RefSite::Instance { element: assembly, instance }) else { return };
    let change = cadrs_core::link_update::change_for(&mut crate::linked::resolver(world).0, &doc, None, &u, Target::Version(v));
    match change {
        Ok(Some(c)) => {
            super::run(world, &UpdateReferences { changes: vec![c], label: label.into() });
        }
        Ok(None) => {}
        Err(e) => crate::linked::error_toast(world, e.to_string()),
    }
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
}

/// **Update context from the assembly** of a linked part (MC5.8.1): the other document's context
/// takes the snapshot, gets a version, and the primary instance uses it.
pub fn update_from_assembly(world: &mut World, assembly: ElementId, instance: InstanceId, id: ContextNo) {
    let asm_doc = world.resource::<ActiveDocument>().doc.clone();
    let Some((d, studio)) = context::linked_studio_of(&asm_doc, assembly, instance) else { return };
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let primary = document(world, d).and_then(|p| p.element(studio)?.context(id).map(|c| c.instance)).unwrap_or(instance);
    let made = context::update_linked_context(&mut crate::linked::resolver(world).0, &asm_doc, d, studio, id, now, &user);
    match made {
        Ok(v) => point_at(world, assembly, primary, v, "Update context"),
        Err(e) => crate::linked::error_toast(world, e.to_string()),
    }
}

/// The snapshot a context whose assembly is in another document takes now (MC5.8.2: from the
/// Part Studio, no version).
pub fn resnapshot(world: &mut World, ctx: &StudioContext) -> Result<StudioContext, String> {
    let d = ctx.document.ok_or("not a linked context")?;
    let asm_doc = document(world, d).ok_or("The assembly's document can't be read")?;
    context::resnapshot_external(&asm_doc, ctx).map_err(|e| e.to_string())
}

/// Opens the assembly's document of a context made from another document (Go to assembly
/// without a session: the part's document was opened by itself).
pub fn open_assembly(world: &mut World, ctx: &StudioContext) {
    let Some(d) = ctx.document else { return };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let file = match store.load(d) {
        Ok(f) => f,
        Err(e) => {
            crate::linked::error_toast(world, format!("The assembly's document can't be opened: {e}"));
            return;
        }
    };
    if let Err(e) = world.resource_mut::<ActiveDocument>().save_if_changed(&store, now, &user) {
        warn!("cannot save: {e}");
    }
    let mut open = ActiveDocument::stored(file.document, file.meta);
    open.set_active(ctx.assembly);
    world.insert_resource(open);
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
}
