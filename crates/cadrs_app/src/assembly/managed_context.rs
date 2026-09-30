//! **Managed in-context design** in the app (P3H.5, `pcb-studio.md` X9, PCB5.7, PCB6 steps
//! 2–8, 13; [`cadrs_core::assembly::managed_context`]):
//!
//! - The assembly toolbar's **Display states ▾** menu has **Create Part Studio in context**
//!   (`ex1-step2-part-studio-in-context.png`): it opens the **Origin of new Part Studio** dialog
//!   (a ✓ / ✕ panel with the "Select origin/mate connector" field, the assembly **Origin**
//!   picked); ✓ makes a new Part Studio tab whose context is the whole assembly (one undo step)
//!   and switches to it. Its sketches sit on the context geometry and Use its edges as with Edit
//!   in context.
//! - The context bar of an in-context studio has **Insert and go to Assembly**
//!   (`v5-translate-board-incontext-poster.png`): the **Insert and go to Assembly** panel lists
//!   the parts picked (in the view or the Parts list); ✓ inserts them into the assembly where the
//!   studio has them (one undo step) and switches to the assembly.
//! - The instance menu's **Update context ▸ <Part Studio>** (PCB6 step 13) updates the context
//!   of the instance's Part Studio from the assembly it was made in.
//!
//! Names: `display-states` (its menu `display-states-menu-states`,
//! `display-states-menu-in-context`), `origin-dialog` (`origin-dialog-field`,
//! `origin-dialog-accept`), `context-insert`, `insert-go-dialog` (`insert-go-parts`,
//! `insert-go-dialog-accept`), `asm-update-context` / `asm-update-context-studio`.

use bevy::prelude::*;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::context::is_context;
use cadrs_core::assembly::managed_context::{CreateStudioInContext, InsertFromStudio};
use cadrs_core::{ElementId, PartId};
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, SelectionList, SelectionListRemove, SelectionListState};

use crate::parts::PartCache;
use crate::viewport::{Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ManagedContextPlugin;

impl Plugin for ManagedContextPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync_origin_dialog, follow_parts, sync_insert_dialog).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
                commands.remove_resource::<OriginSession>();
                commands.remove_resource::<InsertSession>();
            })
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_remove);
    }
}

/// The Display states ▾ menu (the assembly toolbar).
pub fn display_states_menu() -> Menu {
    Menu::new("display-states-menu")
        .min_width(210.0)
        .item(MenuItem::new("display-states-menu-states", "Display states").icon("display-states").disabled(true).tooltip("Named display states aren't in cadrs yet"))
        .item(MenuItem::new("display-states-menu-in-context", "Create Part Studio in context").icon("part-studio"))
}

/// A Display states menu item was chosen.
pub fn on_display_states_item(world: &mut World, item: &str) {
    if item == "display-states-menu-in-context" {
        open_origin_dialog(world);
    }
}

// ---------------------------------------------------------------------------------------------
// Origin of new Part Studio

/// The open Origin of new Part Studio dialog: the assembly it creates the studio in.
#[derive(Resource, Debug, Clone)]
pub struct OriginSession {
    pub assembly: ElementId,
}

#[derive(Component)]
struct OriginDialog;

/// Opens the Origin of new Part Studio dialog in the active assembly.
pub fn open_origin_dialog(world: &mut World) {
    let Some(assembly) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else { return };
    world.insert_resource(OriginSession { assembly });
    world.resource_mut::<Selection>().0 = vec![Pick::Origin];
}

fn origin_dialog(t: &Theme) -> impl Bundle {
    let tb = t.clone();
    (
        OriginDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("origin-dialog")
            .title("Origin of new Part Studio")
            .valid(true)
            .width(230.0)
            .body(move |b| {
                b.spawn(SelectionList::new("origin-dialog-field").placeholder("Select origin/mate connector").items(vec!["Origin".to_string()]).active(true).build(&tb))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(2.0), Val::Px(2.0));
                    });
            })
            .build(t),
    )
}

fn sync_origin_dialog(session: Option<Res<OriginSession>>, theme: Res<Theme>, q: Query<Entity, With<OriginDialog>>, q_area: Query<Entity, With<ViewportArea>>, mut commands: Commands) {
    match (session, q.iter().next()) {
        (None, Some(e)) => commands.entity(e).try_despawn(),
        (Some(_), None) => {
            let Some(area) = q_area.iter().next() else { return };
            let d = commands.spawn(origin_dialog(&theme)).id();
            commands.entity(area).add_child(d);
        }
        _ => {}
    }
}

/// ✓: the new Part Studio in context of the assembly (one undo step), switched to.
pub fn accept_origin(world: &mut World) {
    let Some(s) = world.remove_resource::<OriginSession>() else { return };
    let studio = ElementId::new();
    if super::run(world, &CreateStudioInContext { assembly: s.assembly, studio, name: None }) {
        world.resource_mut::<ActiveDocument>().set_active(studio);
        world.resource_mut::<Selection>().0.clear();
    }
}

// ---------------------------------------------------------------------------------------------
// Insert and go to Assembly

/// The open Insert and go to Assembly panel: the in-context studio and the parts picked.
#[derive(Resource, Debug, Clone)]
pub struct InsertSession {
    pub studio: ElementId,
    pub parts: Vec<PartId>,
}

#[derive(Component)]
struct InsertDialog;

/// The studio's own parts in a selection (a face or edge picks its part), in order, each once.
fn picked_parts(selection: &Selection) -> Vec<PartId> {
    let mut out: Vec<PartId> = Vec::new();
    for p in &selection.0 {
        let part = match p {
            Pick::Part(id) | Pick::Face(id, _) | Pick::Edge(id, _) | Pick::Vertex(id, _) => *id,
            _ => continue,
        };
        if !is_context(part.feature) && !out.contains(&part) {
            out.push(part);
        }
    }
    out
}

/// Opens Insert and go to Assembly in the in-context studio `studio`, with the parts picked.
pub fn open_insert_dialog(world: &mut World, studio: ElementId) {
    let parts = picked_parts(world.resource::<Selection>());
    world.insert_resource(InsertSession { studio, parts });
}

fn follow_parts(selection: Res<Selection>, session: Option<ResMut<InsertSession>>) {
    let Some(mut s) = session else { return };
    if !selection.is_changed() {
        return;
    }
    let now = picked_parts(&selection);
    if s.parts != now {
        s.parts = now;
    }
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("insert-go-parts") {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        let Some(part) = world.get_resource::<InsertSession>().and_then(|s| s.parts.get(i).copied()) else { return };
        world.resource_mut::<Selection>().0.retain(|p| !matches!(p, Pick::Part(q) | Pick::Face(q, _) | Pick::Edge(q, _) | Pick::Vertex(q, _) if *q == part));
    });
}

fn insert_dialog(t: &Theme, items: Vec<String>) -> impl Bundle {
    let tb = t.clone();
    let valid = !items.is_empty();
    (
        InsertDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("insert-go-dialog")
            .title("Insert and go to Assembly")
            .valid(valid)
            .width(230.0)
            .body(move |b| {
                b.spawn(SelectionList::new("insert-go-parts").placeholder("Parts to insert").items(items).active(true).build(&tb))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(2.0), Val::Px(2.0));
                    });
            })
            .build(t),
    )
}

#[allow(clippy::type_complexity)]
fn sync_insert_dialog(
    session: Option<Res<InsertSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    mut q_dialog: Query<(Entity, &mut FeatureDialogState), With<InsertDialog>>,
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
    let items: Vec<String> = s.parts.iter().map(|p| cache.part_name(*p).unwrap_or("Part").to_string()).collect();
    let Some((_, mut st)) = q_dialog.iter_mut().next() else {
        let Some(area) = q_area.iter().next() else { return };
        let d = commands.spawn(insert_dialog(&theme, items)).id();
        commands.entity(area).add_child(d);
        return;
    };
    let valid = !items.is_empty();
    if st.valid != valid {
        st.valid = valid;
    }
    for (n, mut l) in &mut q_list {
        if n.as_str() == "insert-go-parts" && l.items != items {
            l.items = items.clone();
        }
    }
}

/// ✓: the parts into the assembly (one undo step), and the assembly shown.
pub fn accept_insert(world: &mut World) {
    let Some(s) = world.get_resource::<InsertSession>().cloned() else { return };
    if s.parts.is_empty() {
        return;
    }
    let cmd = InsertFromStudio { studio: s.studio, instances: s.parts.iter().map(|_| InstanceId::new()).collect(), parts: s.parts.clone() };
    let Some(assembly) = cmd.assembly(&world.resource::<ActiveDocument>().doc) else { return };
    if super::run(world, &cmd) {
        world.remove_resource::<InsertSession>();
        world.resource_mut::<ActiveDocument>().set_active(assembly);
        world.resource_mut::<Selection>().0.clear();
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q_origin: Query<(), With<OriginDialog>>, q_insert: Query<(), With<InsertDialog>>, mut commands: Commands) {
    if q_origin.contains(ev.entity) {
        commands.queue(accept_origin);
    } else if q_insert.contains(ev.entity) {
        commands.queue(accept_insert);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q_origin: Query<(), With<OriginDialog>>, q_insert: Query<(), With<InsertDialog>>, mut commands: Commands) {
    if q_origin.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<OriginSession>();
            world.resource_mut::<Selection>().0.clear();
        });
    } else if q_insert.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<InsertSession>();
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Update context from the assembly

/// The Part Studio of `instance` whose context was made in `assembly` (for the instance menu's
/// Update context ▸), and its name.
pub fn context_studio_of(doc: &ActiveDocument, assembly: ElementId, instance: InstanceId) -> Option<(ElementId, String)> {
    let studio = cadrs_core::assembly::context::studio_of(&doc.doc, assembly, instance)?;
    let el = doc.doc.element(studio)?;
    (el.context.as_ref()?.assembly == assembly).then(|| (studio, el.name.clone()))
}
