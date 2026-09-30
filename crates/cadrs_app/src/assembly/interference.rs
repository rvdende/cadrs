//! **Check interference** (P3B.9, `intro-to-assemblies.md` X15;
//! [`cadrs_core::assembly::interference`]): the instance menu's (and the empty-space menu's)
//! **Check interference** intersects the parts on the kernel thread and opens a panel at the top
//! left of the view listing each pair that overlaps with the volume they share ("Block <1> ×
//! Block <2> · 3000.000 mm³"); the parts of those pairs are drawn see-through, and the volume
//! each pair shares solid red inside them. On instances, only the
//! pairs with one of them are checked; on empty space, every pair. A click on a row selects its
//! two instances; the panel checks again when the assembly changes (a drag, a mate); ✕ or Esc
//! closes it.
//!
//! Names: `interference-dialog`, `interference-status`, rows `interference-row-<k>`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::interference::{self, Clash};
use cadrs_core::assembly::{Assembly, InstanceId};
use cadrs_core::rebuild::PendingJob;
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogCancel, Spinner};

use crate::parts::{FaceBase, PartCache};
use crate::viewport::{Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct InterferencePlugin;

impl Plugin for InterferencePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (poll_check, sync_panel, tint_clashes, draw_clash_volumes).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<InterferenceCheck>())
            .add_observer(on_cancel);
    }
}

/// The open Check interference panel.
#[derive(Resource)]
pub struct InterferenceCheck {
    pub element: ElementId,
    /// The top-level instances checked against the rest (all pairs when empty).
    pub among: Vec<InstanceId>,
    /// The assembly the result is for (checked again when it changes).
    model: Option<Assembly>,
    pending: Option<PendingJob<Result<Vec<Clash>, String>>>,
    pub result: Option<Result<Vec<Clash>, String>>,
}

/// Starts checking `among` (every pair when empty) in the assembly `element`.
pub fn check(world: &mut World, element: ElementId, among: Vec<InstanceId>) {
    world.insert_resource(InterferenceCheck { element, among, model: None, pending: None, result: None });
}

fn poll_check(doc: Option<Res<ActiveDocument>>, check: Option<ResMut<InterferenceCheck>>) {
    let (Some(doc), Some(mut c)) = (doc, check) else { return };
    let Some(model) = doc.doc.element(c.element).and_then(|e| e.assembly_model()).cloned() else { return };
    if c.model.as_ref() != Some(&model) && c.pending.is_none() {
        // (Again when the assembly changed.)
        let items = interference::items(&doc.doc, &model);
        c.pending = Some(interference::check(items, c.among.clone()));
        c.model = Some(model);
        return;
    }
    if let Some(p) = &c.pending
        && let Some(r) = p.poll()
    {
        c.result = Some(r.unwrap_or_else(|| Err("The kernel thread stopped".into())));
        c.pending = None;
    }
}

/// The clashing parts in red (in the assembly the panel is for).
fn tint_clashes(doc: Option<Res<ActiveDocument>>, check: Option<Res<InterferenceCheck>>, mut cache: ResMut<PartCache>) {
    let Some(doc) = doc else { return };
    if doc.active_element().and_then(|e| e.assembly_model()).is_none() {
        return;
    }
    let mut tints = HashMap::new();
    if let Some(c) = &check
        && doc.active == Some(c.element)
        && let Some(Ok(clashes)) = &c.result
    {
        // The parts see-through, so the shared volume (drawn red, `draw_clash_volumes`) shows
        // inside them (Final part 3: the Plate × Pin volume was buried in the plate).
        for k in clashes {
            for p in [k.a, k.b] {
                tints.insert(p, FaceBase { rgb: [236.0, 150.0, 140.0], alpha: 0.3 });
            }
        }
    }
    cache.set_tints(tints);
}

/// A shared volume's red mesh.
#[derive(Component)]
struct ClashVolume;

/// Each clash's shared volume, solid red inside its see-through parts (as Onshape shows it).
fn draw_clash_volumes(
    doc: Option<Res<ActiveDocument>>,
    check: Option<Res<InterferenceCheck>>,
    q: Query<Entity, With<ClashVolume>>,
    mut last: Local<Option<Vec<Clash>>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let want: Option<Vec<Clash>> = match (doc.as_deref(), check.as_deref()) {
        (Some(d), Some(c)) if d.active == Some(c.element) => match &c.result {
            Some(Ok(k)) => Some(k.clone()),
            _ => None,
        },
        _ => None,
    };
    if *last == want {
        return;
    }
    for e in &q {
        commands.entity(e).try_despawn();
    }
    if let Some(clashes) = &want {
        let positions: Vec<[f32; 3]> = clashes.iter().flat_map(|k| k.triangles.iter().flat_map(|t| t.iter().copied())).collect();
        if !positions.is_empty() {
            let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.compute_flat_normals();
            commands.spawn((
                Name::new("interference-volume"),
                ClashVolume,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb_u8(0xe0, 0x2a, 0x1f),
                    perceptual_roughness: 0.7,
                    double_sided: true,
                    cull_mode: None,
                    // Its faces lie on the parts' own: in front of them (no z-fighting), and of
                    // the selection's tint (Final part 4: interference 03/05).
                    depth_bias: 2000.0,
                    ..default()
                })),
                Transform::IDENTITY,
                DespawnOnExit(AppState::Document),
            ));
        }
    }
    *last = want;
}

#[derive(Component)]
struct InterferencePanel(String);

#[derive(Component, Clone, Copy)]
struct ClashRow(cadrs_core::PartId, cadrs_core::PartId);

#[allow(clippy::too_many_arguments)]
fn sync_panel(
    doc: Option<Res<ActiveDocument>>,
    check: Option<Res<InterferenceCheck>>,
    cache: Res<PartCache>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    keys: Res<ButtonInput<KeyCode>>,
    q: Query<(Entity, &InterferencePanel)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let shown = check.as_ref().is_some_and(|c| doc.as_ref().is_some_and(|d| d.active == Some(c.element)));
    if check.is_some() && shown && keys.just_pressed(KeyCode::Escape) {
        commands.remove_resource::<InterferenceCheck>();
        return;
    }
    let Some(c) = check.filter(|_| shown) else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let name = |p: cadrs_core::PartId| cache.part_name(p).unwrap_or("part").to_string();
    // (status line, rows)
    let (status, rows): (String, Vec<(cadrs_core::PartId, cadrs_core::PartId, String, String)>) = match &c.result {
        _ if c.pending.is_some() && c.result.is_none() => ("Checking…".into(), Vec::new()),
        None => ("Checking…".into(), Vec::new()),
        Some(Err(e)) => (format!("Check interference failed: {e}"), Vec::new()),
        Some(Ok(v)) if v.is_empty() => ("No interference found".into(), Vec::new()),
        Some(Ok(v)) => (
            if v.len() == 1 { "1 interference".into() } else { format!("{} interferences", v.len()) },
            v.iter().map(|k| (k.a, k.b, format!("{} × {}", name(k.a), name(k.b)), units.0.volume(k.volume))).collect(),
        ),
    };
    let scope = if c.among.is_empty() {
        "All instances".to_string()
    } else {
        let names: Vec<String> = c.among.iter().map(|i| cache.part_name(i.part_id()).map(str::to_string).unwrap_or_else(|| "instance".into())).collect();
        names.join(", ")
    };
    let busy = c.pending.is_some();
    let key = format!("{status}{rows:?}{scope}{busy}");
    if let Some((e, p)) = q.iter().next() {
        if p.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else { return };
    let t = theme.clone();
    let red = Color::srgb_u8(0xd2, 0x3c, 0x32);
    let clash = !rows.is_empty();
    let d = commands
        .spawn((
            InterferencePanel(key),
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("interference-dialog")
                .title("Check interference")
                .valid(false)
                .plain_title()
                .width(330.0)
                .body(move |b| {
                    b.spawn((t.text(format!("Instances: {scope}"), 11.5, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
                    b.spawn((Name::new("interference-status"), Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), margin: UiRect::bottom(Val::Px(4.0)), ..default() }))
                        .with_children(|r| {
                            if busy {
                                r.spawn(Spinner::new("interference-spinner").size(14.0).thickness(2.0).build(&t));
                            } else {
                                r.spawn(icon(if clash { "warning-filled" } else { "check" }, 14.0, if clash { red } else { Color::srgb_u8(0x2e, 0x9e, 0x44) }));
                            }
                            r.spawn(t.text(status.clone(), 12.0, FontWeight::SEMIBOLD, t.foreground));
                        });
                    for (k, (a, bb, label, vol)) in rows.iter().enumerate() {
                        b.spawn((
                            Name::new(format!("interference-row-{}", k + 1)),
                            ClashRow(*a, *bb),
                            Button,
                            Node {
                                height: Val::Px(24.0),
                                padding: UiRect::horizontal(Val::Px(6.0)),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            cadrs_ui::Visuals {
                                background: cadrs_ui::StateColors::new(Color::NONE, t.list_hover, t.list_active, Color::NONE),
                                border: cadrs_ui::StateColors::all(Color::NONE),
                                foreground: cadrs_ui::StateColors::all(t.foreground),
                                focus_ring: t.focus_ring,
                            },
                            Tooltip::new("Select these instances"),
                        ))
                        .observe(|click: On<Pointer<Click>>, q: Query<&ClashRow>, mut sel: ResMut<Selection>| {
                            if let Ok(r) = q.get(click.entity) {
                                sel.0 = vec![Pick::Part(r.0), Pick::Part(r.1)];
                            }
                        })
                        .with_children(|r| {
                            r.spawn((Node { width: Val::Px(10.0), height: Val::Px(10.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(red), Pickable::IGNORE));
                            r.spawn((t.text(label.clone(), 12.0, FontWeight::NORMAL, t.foreground), Node { flex_grow: 1.0, overflow: Overflow::clip(), ..default() }, Pickable::IGNORE));
                            r.spawn((t.text(vol.clone(), 12.0, FontWeight::MEDIUM, t.foreground), Pickable::IGNORE));
                        });
                    }
                })
                .build(&theme),
        ))
        .id();
    commands.entity(area).add_child(d);
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<InterferencePanel>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.remove_resource::<InterferenceCheck>();
    }
}
