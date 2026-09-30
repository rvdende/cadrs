//! **Edit** a rigid Part Studio instance (P3B.8, `intro-to-assemblies.md` A2.4): the instance
//! menu's Edit… opens this dialog with a checkbox for every part of the studio (its current
//! rebuild, so parts made since are listed too): ticked parts are in the instance. ✓ sets them
//! ([`cadrs_core::assembly::commands::SetStudioParts`], one undo step); at least one part stays.
//!
//! Unticked parts are hidden in the view while the dialog is open ([`super::AssemblyParts`]).
//!
//! Names: `studio-edit-dialog`, the rows `studio-edit-part-<slug>` (their checkboxes
//! `…-checkbox`).

use bevy::prelude::*;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::commands::SetStudioParts;
use cadrs_core::{ElementId, PartId};
use cadrs_ui::dialog_fields::OptionRow;
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState};

use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct StudioEditPlugin;

impl Plugin for StudioEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_dialog.after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<StudioEditSession>())
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_check);
    }
}

/// The open dialog.
#[derive(Resource, Debug, Clone)]
pub struct StudioEditSession {
    pub element: ElementId,
    pub instance: InstanceId,
    pub title: String,
    /// Every part of the studio: its id, name and whether it is in the instance.
    pub parts: Vec<(PartId, String, bool)>,
}

#[derive(Component)]
struct StudioEditDialog;

/// A part's row.
#[derive(Component, Debug, Clone, Copy)]
struct PartRow(usize);

/// Opens the dialog on a rigid Part Studio instance.
pub fn open(world: &mut World, instance: InstanceId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(inst) = doc.active_element().and_then(|e| e.assembly_model()?.instance(instance).cloned()) else { return };
    let cadrs_core::assembly::InstanceSource::Studio { element: studio } = inst.source else { return };
    let d = doc.doc.clone();
    let Some(el) = d.element(studio) else { return };
    let name = el.name.clone();
    let props = el.part_props().to_vec();
    let Some(build) = world.resource_mut::<super::AssemblyParts>().build(&d, studio) else { return };
    let parts = build.parts.iter().map(|p| (p.id, cadrs_core::parts::display_name(p, &props).to_string(), inst.parts.contains(&p.id))).collect();
    world.insert_resource(StudioEditSession { element, instance, title: format!("Edit {name} <{}>", inst.index), parts });
}

fn on_check(
    ev: On<CheckboxChange>,
    q: Query<&ChildOf>,
    q_row: Query<&PartRow>,
    session: Option<ResMut<StudioEditSession>>,
    mut parts: ResMut<super::AssemblyParts>,
) {
    let Some(mut s) = session else { return };
    // The checkbox is the row's child.
    let Some(row) = q.get(ev.entity).ok().and_then(|c| q_row.get(c.parent()).ok()) else { return };
    if let Some(p) = s.parts.get_mut(row.0) {
        p.2 = ev.checked;
    }
    // Unticked parts disappear from the view at once (P3B.8 judge).
    parts.studio_preview = Some((s.instance, s.parts.iter().filter(|p| p.2).map(|p| p.0).collect()));
}

fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<StudioEditSession>().cloned() else { return };
    let parts: Vec<PartId> = s.parts.iter().filter(|p| p.2).map(|p| p.0).collect();
    if parts.is_empty() {
        return;
    }
    let same = world
        .resource::<ActiveDocument>()
        .doc
        .element(s.element)
        .and_then(|e| e.assembly_model()?.instance(s.instance))
        .is_some_and(|i| i.parts == parts);
    if same || super::run(world, &SetStudioParts { element: s.element, instance: s.instance, parts }) {
        world.remove_resource::<StudioEditSession>();
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<StudioEditDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<StudioEditDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<StudioEditSession>();
        });
    }
}

fn sync_dialog(
    session: Option<Res<StudioEditSession>>,
    theme: Res<Theme>,
    mut q_dialog: Query<(Entity, &mut FeatureDialogState), With<StudioEditDialog>>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for (e, _) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        commands.queue(|world: &mut World| {
            let mut p = world.resource_mut::<super::AssemblyParts>();
            if p.studio_preview.is_some() {
                p.studio_preview = None;
            }
        });
        return;
    };
    let valid = s.parts.iter().any(|p| p.2);
    if let Some((_, mut st)) = q_dialog.iter_mut().next() {
        if st.valid != valid {
            st.valid = valid;
        }
        return;
    }
    let Some(area) = q_area.iter().next() else { return };
    let t = theme.clone();
    let parts = s.parts.clone();
    let d = commands
        .spawn((
            StudioEditDialog,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("studio-edit-dialog")
                .title(s.title.clone())
                .valid(valid)
                .width(230.0)
                .body(move |b| {
                    b.spawn((
                        Name::new("studio-edit-hint"),
                        t.text("Parts in this rigid instance", t.font_sm, bevy::text::FontWeight::MEDIUM, t.muted_foreground),
                        Node { margin: UiRect::new(Val::Px(4.0), Val::ZERO, Val::Px(2.0), Val::Px(4.0)), ..default() },
                    ));
                    for (k, (_, name, on)) in parts.iter().enumerate() {
                        let slug = super::insert::slug(name);
                        b.spawn((OptionRow::new(format!("studio-edit-part-{slug}"), name.clone()).checked(*on).build(&t), PartRow(k)));
                    }
                })
                .build(&theme),
        ))
        .id();
    commands.entity(area).add_child(d);
}
