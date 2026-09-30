//! The Group dialog (P3B.2, `intro-to-assemblies.md` A13; `ex2-step5.png`): a single
//! **Instances** field that follows the selection (click instances in the view or the list; the
//! selection preselected when it opened counts), each row with its ✕. ✓ adds "Group 1" to the
//! Mate Features list (one undo step): its instances then move as one rigid body, keeping their
//! placements relative to each other (A13.3); the group still needs Fix or a mate (A13.4).

use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::commands::{AddMateFeature, SetMateFeature};
use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind, next_name};
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, SelectionList, SelectionListRemove, SelectionListState};

use crate::parts::PartCache;
use crate::viewport::{Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct GroupDialogPlugin;

impl Plugin for GroupDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (follow_selection, sync_group_dialog).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<GroupSession>())
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_remove);
    }
}

/// The open Group dialog.
#[derive(Resource, Debug, Clone)]
pub struct GroupSession {
    pub element: ElementId,
    pub id: MateId,
    pub name: String,
    pub editing: bool,
    pub instances: Vec<InstanceId>,
}

#[derive(Component)]
struct GroupDialog;

/// Opens the Group dialog (the toolbar's Group) with the selected instances.
pub fn open_group_dialog(world: &mut World) {
    world.remove_resource::<super::relation_dialog::RelationSession>();
    world.remove_resource::<super::mate_dialog::MateSession>();
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let name = next_name(&model.mates, "Group");
    let instances = super::selected_instances(world.resource::<Selection>());
    world.insert_resource(GroupSession { element, id: MateId::new(), name, editing: false, instances });
}

/// Opens it on an existing group (Edit…).
pub fn edit_group(world: &mut World, id: MateId) {
    world.remove_resource::<super::mate_dialog::MateSession>();
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(f) = doc.active_element().and_then(|e| e.assembly_model()?.mate(id).cloned()) else { return };
    let MateKind::Group { instances } = f.kind else { return };
    world.resource_mut::<Selection>().0 = instances.iter().map(|i| Pick::Part(i.part_id())).collect();
    world.insert_resource(GroupSession { element, id, name: f.name, editing: true, instances });
}

/// The field shows the selected instances.
fn follow_selection(selection: Res<Selection>, session: Option<ResMut<GroupSession>>) {
    let Some(mut s) = session else { return };
    if !selection.is_changed() {
        return;
    }
    let now = super::selected_instances(&selection);
    if s.instances != now {
        s.instances = now;
    }
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("group-instances") {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        let Some(id) = world.get_resource::<GroupSession>().and_then(|s| s.instances.get(i).copied()) else { return };
        world.resource_mut::<Selection>().0.retain(|p| super::instance_of(p) != Some(id));
    });
}

pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<GroupSession>().cloned() else { return };
    if s.instances.len() < 2 {
        return;
    }
    let feature = MateFeature::new(s.id, s.name.clone(), MateKind::Group { instances: s.instances.clone() });
    let element = s.element;
    let ok = if s.editing {
        super::run(world, &SetMateFeature { element, feature, poses: Vec::new() })
    } else {
        super::run(world, &AddMateFeature { element, feature, poses: Vec::new() })
    };
    if ok {
        world.remove_resource::<GroupSession>();
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<GroupDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<GroupDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<GroupSession>();
        });
    }
}

fn group_dialog(t: &Theme, name: &str, items: Vec<String>) -> impl Bundle {
    let tb = t.clone();
    let tf = t.clone();
    let valid = items.len() >= 2;
    (
        GroupDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("group-dialog")
            .title(name.to_string())
            .valid(valid)
            .width(214.0)
            .body(move |b| {
                b.spawn(SelectionList::new("group-instances").placeholder("Instances").items(items).active(true).build(&tb))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(2.0), Val::Px(2.0));
                    });
            })
            .footer(move |f| {
                f.spawn((Name::new("group-help"), icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
                let _ = &tf;
            })
            .build(t),
    )
}

#[allow(clippy::type_complexity)]
fn sync_group_dialog(
    session: Option<Res<GroupSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    mut q_dialog: Query<(Entity, &mut FeatureDialogState), With<GroupDialog>>,
    mut q_list: Query<(&Name, &mut SelectionListState)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for (e, _) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let items: Vec<String> = s.instances.iter().map(|i| cache.part_name(i.part_id()).unwrap_or("instance").to_string()).collect();
    let Some((_, mut st)) = q_dialog.iter_mut().next() else {
        let Some(area) = q_area.iter().next() else { return };
        let d = commands.spawn(group_dialog(&theme, &s.name, items)).id();
        commands.entity(area).add_child(d);
        return;
    };
    let valid = items.len() >= 2;
    if st.valid != valid {
        st.valid = valid;
    }
    for (n, mut l) in &mut q_list {
        if n.as_str() == "group-instances" && l.items != items {
            l.items = items.clone();
        }
    }
}
