//! Keeps the PCB Studios' copies of the workspace settings and the component library in step
//! with the shared stores (X6, PCB11.4), through [`cadrs_core::pcb::library::LibrarySync`]:
//! after every document change (a command, undo or redo) a changed copy is written to
//! `pcb-workspace.ron` and the library document, and a PCB Studio seen for the first time (the
//! document opened, a tab added) reads them. Reading is not an edit: it adds no undo step.

use bevy::prelude::*;
use cadrs_core::pcb::library::LibrarySync;
use cadrs_ui::{Notification, Theme, show_notification};

use crate::{ActiveDocument, DocumentStore};

/// The open document's sync state.
#[derive(Resource, Default)]
pub struct PcbLibrarySync(pub LibrarySync);

/// Runs the sync when the document changed.
pub fn sync_library(world: &mut World) {
    let changed = world.get_resource_ref::<ActiveDocument>().is_some_and(|d| d.is_changed());
    if !changed {
        return;
    }
    let Some(store) = world.get_resource::<DocumentStore>().map(|s| s.0.clone()) else { return };
    let has_studio = world.resource::<ActiveDocument>().doc.elements.iter().any(|e| e.pcb().is_some());
    if !has_studio {
        return;
    }
    let mut sync = std::mem::take(&mut world.resource_mut::<PcbLibrarySync>().0);
    let out = {
        let mut doc = world.resource_mut::<ActiveDocument>();
        let d = doc.bypass_change_detection();
        let out = sync.sync(&mut d.doc, &store);
        if out.changed_document() {
            doc.set_changed();
        }
        out
    };
    world.resource_mut::<PcbLibrarySync>().0 = sync;
    if !out.errors.is_empty() {
        warn!("PCB library: {:?}", out.errors);
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_notification(&mut commands, &theme, Notification::warning(out.errors.join("; ")).name("pcb-library-toast").max_width(560.0));
        world.flush();
    }
}
