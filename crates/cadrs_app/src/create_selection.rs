//! **Create selection** (X12, PS21.15; `ex4-step15.png`): right-click empty space → **Select** →
//! **Create selection…** opens a small panel: **Faces | Edges**, the rule (**Tangent
//! connected**), the *Selection* field for the seed edge, "N edges selected" and **Add
//! selection**. While it is open, a click on an edge makes that edge the seed and selects its
//! tangent-connected chain (the kernel's exact tangency, [`crate::advanced::tangent_connected`]).
//! **Add selection** adds the edges to the open feature dialog's active field (a Sweep's path, a
//! Fillet's or Chamfer's entities, a Plane's entities) as one command, or to the selection when
//! no dialog is open, and closes the panel.
//!
//! P3.8 (`ex5-step12.png`): **Faces → Pocket**: a clicked face is the seed and the panel selects
//! the pocket it is in (the faces reached across concave or smooth edges, [`cadrs_core::Solid::
//! pocket_faces`]): "9 faces selected" for the Reflector's filleted pocket. Opened from a
//! pattern's or mirror's Create selection button, Add selection puts them in its faces.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::advanced::PathRef;
use cadrs_core::applied::EdgeOrFace;
use cadrs_core::plane::PlaneEntity;
use cadrs_core::{FeatureKind, PartId};
use cadrs_sketch::EdgeName;
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogCancel, OptionRow, Select, SelectionList, SelectionListState, TabStrip};

use crate::applied::{AppliedField, AppliedSession, current, entity_of, set};
use crate::parts::PartCache;
use crate::viewport::{Pick, PickRequest, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct CreateSelectionPlugin;

impl Plugin for CreateSelectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_cancel)
            .add_observer(on_add)
            .add_observer(on_tab)
            .add_systems(
                Update,
                (create_selection_picks, sync_panel)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
                commands.remove_resource::<CreateSelection>();
            });
    }
}

/// The panel is open: its seed edge and the edges its rule selects, or (Faces) its seed face and
/// the pocket's faces.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CreateSelection {
    /// The Faces tab (Pocket) rather than Edges (Tangent connected).
    pub faces_mode: bool,
    pub seed: Option<(PartId, EdgeName)>,
    pub edges: Vec<(PartId, EdgeName)>,
    pub face_seed: Option<(PartId, cadrs_sketch::FaceName)>,
    pub faces: Vec<(PartId, cadrs_sketch::FaceName)>,
}

#[derive(Component)]
struct CreateSelectionPanel;

#[derive(Component)]
struct SeedField;

#[derive(Component)]
struct CountText;

#[derive(Component)]
struct AddButton;

/// Opens the panel (the viewport menu's Select → Create selection…) on its Edges tab.
pub fn open(world: &mut World) {
    open_with(world, false);
}

/// Opens the panel on its Faces tab (Pocket), as a pattern's or mirror's button does.
pub fn open_faces(world: &mut World) {
    open_with(world, true);
}

fn open_with(world: &mut World, faces_mode: bool) {
    if let Some(c) = world.get_resource::<CreateSelection>() {
        if c.faces_mode == faces_mode {
            return;
        }
        close(world);
    }
    world.insert_resource(CreateSelection { faces_mode, ..CreateSelection::default() });
    let Some(area) = world.query_filtered::<Entity, With<ViewportArea>>().iter(world).next() else { return };
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    let panel = world
        .spawn((
            CreateSelectionPanel,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("create-selection-dialog")
                .title("Create selection")
                .plain_title()
                .no_accept()
                .width(190.0)
                .body_padding(UiRect::ZERO)
                .body(move |b| {
                    let t = &tb;
                    b.spawn(TabStrip::new("create-selection-type").compact().tab("Faces").tab("Edges").selected(if faces_mode { 0 } else { 1 }).build(t));
                    b.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(6.0), Val::Px(4.0)),
                        row_gap: Val::Px(3.0),
                        ..default()
                    })
                    .with_children(|c| {
                        let rule = if faces_mode {
                            Select::new("create-selection-rule")
                                .option("Pocket", true)
                                .option("Boss", false)
                                .option("Tangent connected", false)
                                .option("Feature", false)
                        } else {
                            Select::new("create-selection-rule")
                                .option("Tangent connected", true)
                                .option("Loop", false)
                                .option("Boundary", false)
                        };
                        c.spawn(rule.selected(0).build(t));
                        c.spawn((SeedField, SelectionList::new("create-selection-field").placeholder("Selection").active(true).build(t)));
                        c.spawn(OptionRow::new("create-selection-patterns", "Select patterns").disabled(true).build(t));
                        c.spawn((
                            CountText,
                            Name::new("create-selection-count"),
                            t.text(if faces_mode { "0 faces selected" } else { "0 edges selected" }, t.font_sm, bevy::text::FontWeight::NORMAL, t.muted_foreground),
                        ));
                        c.spawn(Node { justify_content: JustifyContent::Center, margin: UiRect::vertical(Val::Px(3.0)), ..default() })
                            .with_child((AddButton, cadrs_ui::Button::new("create-selection-add").label("Add selection").small().outline().build(t)));
                    });
                })
                .build(&theme),
        ))
        .id();
    if let Some(mut n) = world.get_mut::<Node>(panel) {
        // Beside the feature dialog, as in `ex4-step15.png`.
        n.left = Val::Px(232.0);
        n.top = Val::Px(210.0);
    }
    world.entity_mut(area).add_child(panel);
}

fn close(world: &mut World) {
    world.remove_resource::<CreateSelection>();
    let panels: Vec<Entity> = world.query_filtered::<Entity, With<CreateSelectionPanel>>().iter(world).collect();
    for e in panels {
        world.entity_mut(e).despawn();
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<CreateSelectionPanel>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}

/// Faces | Edges: the panel again on that tab.
fn on_tab(ev: On<cadrs_ui::TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "create-selection-type") {
        let faces = ev.index == 0;
        commands.queue(move |world: &mut World| open_with(world, faces));
    }
}

/// While the panel is open, a clicked edge is its seed.
fn create_selection_picks(mut picks: MessageReader<PickRequest>, create: Option<Res<CreateSelection>>, mut commands: Commands) {
    if create.is_none() {
        return;
    }
    for p in picks.read() {
        if let Some(Pick::Face(part, face)) = p.0 {
            commands.queue(move |world: &mut World| {
                let faces = world.resource::<PartCache>().part(part).map_or(Vec::new(), |p| {
                    let seed = p.solid.faces.iter().position(|f| f.name == face);
                    seed.map_or(Vec::new(), |i| p.solid.pocket_faces(i).into_iter().map(|j| p.solid.faces[j].name).collect())
                });
                if let Some(mut c) = world.get_resource_mut::<CreateSelection>()
                    && c.faces_mode
                {
                    c.face_seed = Some((part, face));
                    c.faces = faces.into_iter().map(|f| (part, f)).collect();
                }
            });
            continue;
        }
        let Some(Pick::Edge(part, edge)) = p.0 else { continue };
        commands.queue(move |world: &mut World| {
            let edges = crate::advanced::tangent_connected(world.resource::<PartCache>(), part, edge);
            if let Some(mut c) = world.get_resource_mut::<CreateSelection>() {
                c.seed = Some((part, edge));
                c.edges = edges.into_iter().map(|e| (part, e)).collect();
            }
        });
    }
}

/// The seed's label, the count, and the chain shown selected.
fn sync_panel(
    create: Option<Res<CreateSelection>>,
    doc: Option<Res<ActiveDocument>>,
    mut q_field: Query<&mut SelectionListState, With<SeedField>>,
    mut q_count: Query<&mut Text, With<CountText>>,
    mut selection: ResMut<Selection>,
    session: Option<Res<AppliedSession>>,
) {
    let Some(c) = create else { return };
    if !c.is_changed() {
        return;
    }
    let features = doc.as_ref().and_then(|d| d.active_element()).map(|e| e.features().to_vec()).unwrap_or_default();
    let name = |op: cadrs_sketch::OpId| features.iter().find(|f| f.id.0 == op).map_or("part", |f| f.name.as_str()).to_string();
    let items: Vec<String> = if c.faces_mode {
        c.face_seed.iter().map(|(_, f)| format!("Face of {}", name(f.op))).collect()
    } else {
        c.seed.iter().map(|(_, e)| format!("Edge of {}", name(crate::parts::edge_maker(&features, e)))).collect()
    };
    for mut l in &mut q_field {
        if l.items != items {
            l.items = items.clone();
        }
    }
    let (n, what) = if c.faces_mode { (c.faces.len(), "face") } else { (c.edges.len(), "edge") };
    let text = format!("{n} {what}{} selected", if n == 1 { "" } else { "s" });
    for mut t in &mut q_count {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
    // Without a feature dialog, the chain shows as the selection.
    if session.is_none() {
        selection.0 = if c.faces_mode {
            c.faces.iter().map(|(p, f)| Pick::Face(*p, *f)).collect()
        } else {
            c.edges.iter().map(|(p, e)| Pick::Edge(*p, *e)).collect()
        };
    }
}

fn on_add(a: On<Activate>, q: Query<(), With<AddButton>>, mut commands: Commands) {
    if !q.contains(a.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let Some(c) = world.get_resource::<CreateSelection>().cloned() else { return };
        if c.faces_mode {
            add_faces(world, &c.faces);
        } else {
            add_edges(world, &c.edges);
        }
        close(world);
    });
}

/// Adds edges to the open feature dialog's active field (those it doesn't have yet), or to the
/// selection.
pub fn add_edges(world: &mut World, edges: &[(PartId, EdgeName)]) {
    let refs: Vec<_> = {
        let cache = world.resource::<PartCache>();
        edges
            .iter()
            .filter_map(|(p, e)| match entity_of(cache, Pick::Edge(*p, *e))? {
                EdgeOrFace::Edge(r) => Some(r),
                _ => None,
            })
            .collect()
    };
    let Some(s) = world.get_resource::<AppliedSession>().cloned() else {
        let mut sel = world.resource_mut::<Selection>();
        for (p, e) in edges {
            if !sel.contains(Pick::Edge(*p, *e)) {
                sel.0.push(Pick::Edge(*p, *e));
            }
        }
        return;
    };
    let Some(f) = current(world) else { return };
    let mut kind = f.kind.clone();
    match (&mut kind, s.field) {
        (FeatureKind::Sweep(x), AppliedField::Path) => {
            for r in refs {
                if !x.path.iter().any(|p| matches!(p, PathRef::Edge(g) if g.edge == r.edge)) {
                    x.path.push(PathRef::Edge(r));
                }
            }
        }
        (FeatureKind::Fillet(x), AppliedField::Entities) => {
            for r in refs {
                if !x.entities.iter().any(|e| matches!(e, EdgeOrFace::Edge(g) if g.edge == r.edge)) {
                    x.entities.push(EdgeOrFace::Edge(r));
                }
            }
        }
        (FeatureKind::Chamfer(x), AppliedField::Entities) => {
            for r in refs {
                if !x.entities.iter().any(|e| matches!(e, EdgeOrFace::Edge(g) if g.edge == r.edge)) {
                    x.entities.push(EdgeOrFace::Edge(r));
                }
            }
        }
        (FeatureKind::Plane(x), AppliedField::PlaneEntities) => {
            for r in refs {
                if !x.entities.iter().any(|e| matches!(e, PlaneEntity::Edge(g) if g.edge == r.edge)) {
                    x.entities.push(PlaneEntity::Edge(r));
                }
            }
        }
        _ => return,
    }
    if kind != f.kind {
        set(world, kind, "Add selection");
    }
}

/// Adds faces to the open pattern's or mirror's faces (those it doesn't have yet), or to the
/// selection.
pub fn add_faces(world: &mut World, faces: &[(PartId, cadrs_sketch::FaceName)]) {
    let refs: Vec<cadrs_core::FaceRef> = {
        let cache = world.resource::<PartCache>();
        faces
            .iter()
            .filter_map(|(p, f)| match entity_of(cache, Pick::Face(*p, *f))? {
                EdgeOrFace::Face(r) => Some(r),
                _ => None,
            })
            .collect()
    };
    if world.get_resource::<AppliedSession>().is_none() {
        let mut sel = world.resource_mut::<Selection>();
        for (p, f) in faces {
            if !sel.contains(Pick::Face(*p, *f)) {
                sel.0.push(Pick::Face(*p, *f));
            }
        }
        return;
    }
    let Some(f) = current(world) else { return };
    let mut kind = f.kind.clone();
    let list = match &mut kind {
        FeatureKind::Pattern(x) if x.pattern_type == cadrs_core::pattern::PatternType::Face => &mut x.faces,
        FeatureKind::Mirror(x) if x.mirror_type == cadrs_core::pattern::PatternType::Face => &mut x.faces,
        _ => return,
    };
    for r in refs {
        if !list.iter().any(|g| g.face == r.face) {
            list.push(r);
        }
    }
    if kind != f.kind {
        set(world, kind, "Add selection");
    }
}
