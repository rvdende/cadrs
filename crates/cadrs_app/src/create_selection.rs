//! **Create selection** (X12, PS21.15; `ex4-step15.png`): right-click empty space → **Select** →
//! **Create selection…** opens a small panel: **Faces | Edges**, the rule (**Tangent
//! connected**), the *Selection* field for the seed edge, "N edges selected" and **Add
//! selection**. While it is open, a click on an edge makes that edge the seed and selects its
//! tangent-connected chain (the kernel's exact tangency, [`crate::advanced::tangent_connected`]).
//! **Add selection** adds the edges to the open feature dialog's active field (a Sweep's path, a
//! Fillet's or Chamfer's entities, a Plane's entities) as one command, or to the selection when
//! no dialog is open, and closes the panel.
//!
//! P3.8 (`ex5-step12.png`): **Faces**: a clicked face is the seed and the rule selects the faces
//! that go with it, as Onshape's rules ([`FaceRule`]): **Protrusion** (across convex or smooth
//! edges), **Pocket** (concave or smooth: "9 faces selected" for the Reflector's filleted
//! pocket), **Hole** (the round hole's walls, floors and point), **Fillets** (every fillet of
//! the seed's radius), **Tangent connected** and **Bounded faces** (the faces between the seed
//! and the faces or edges picked after it). **Select patterns** adds the rule's faces from every
//! face like the seed (a pattern's copies). Opened from a pattern's, mirror's or direct
//! edit's Create selection button, Add selection puts them in its faces (#22: an imported
//! part's pocket or boss deleted or moved in one go).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::advanced::PathRef;
use cadrs_core::applied::EdgeOrFace;
use cadrs_core::plane::PlaneEntity;
use cadrs_core::{FeatureKind, PartId};
use cadrs_sketch::EdgeName;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    FeatureDialogCancel, OptionRow, Select, SelectionList, SelectionListState, TabStrip,
};

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
            .add_observer(on_rule)
            .add_observer(on_patterns)
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

/// How the Faces tab grows a seed face into a selection (Onshape's order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceRule {
    Protrusion,
    #[default]
    Pocket,
    Hole,
    Fillets,
    TangentConnected,
    Bounded,
}

impl FaceRule {
    pub const ALL: [FaceRule; 6] = [
        FaceRule::Protrusion,
        FaceRule::Pocket,
        FaceRule::Hole,
        FaceRule::Fillets,
        FaceRule::TangentConnected,
        FaceRule::Bounded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FaceRule::Protrusion => "Protrusion",
            FaceRule::Pocket => "Pocket",
            FaceRule::Hole => "Hole",
            FaceRule::Fillets => "Fillets",
            FaceRule::TangentConnected => "Tangent connected",
            FaceRule::Bounded => "Bounded faces",
        }
    }

    /// The faces (indices) the rule selects from the seed face `i` (Bounded faces: with the
    /// boundary's faces and edges).
    pub fn faces(
        self,
        solid: &cadrs_core::Solid,
        i: usize,
        bound_faces: &[usize],
        bound_edges: &[EdgeName],
    ) -> Vec<usize> {
        match self {
            FaceRule::Bounded => solid.bounded_faces(i, bound_faces, bound_edges),
            FaceRule::Protrusion => solid.protrusion_faces(i),
            FaceRule::Pocket => solid.pocket_faces(i),
            FaceRule::Hole => solid.hole_faces(i),
            FaceRule::Fillets => solid.fillet_faces(i),
            FaceRule::TangentConnected => solid.tangent_faces(i),
        }
    }
}

/// The panel is open: its seed edge and the edges its rule selects, or (Faces) its seed face and
/// the faces its rule selects.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct CreateSelection {
    /// The Faces tab rather than Edges (Tangent connected).
    pub faces_mode: bool,
    /// The Faces tab's rule.
    pub rule: FaceRule,
    /// Bounded faces' boundary: faces and edges picked after the seed.
    pub bound_faces: Vec<(PartId, cadrs_sketch::FaceName)>,
    pub bound_edges: Vec<(PartId, EdgeName)>,
    /// Select patterns: the rule's faces from the faces like the seed too.
    pub patterns: bool,
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
struct BoundField;

#[derive(Component)]
struct AddButton;

/// Opens the panel (the viewport menu's Select → Create selection…) on its Edges tab.
pub fn open(world: &mut World) {
    open_with(world, false);
}

/// Opens the panel on its Faces tab with `face` as the seed (the face's right-click menu):
/// the selection shows the rule's faces, ready for Add selection.
pub fn open_from_face(world: &mut World, part: PartId, face: cadrs_sketch::FaceName) {
    open_with(world, true);
    update(world, |c| c.face_seed = Some((part, face)));
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
    world.insert_resource(CreateSelection {
        faces_mode,
        ..CreateSelection::default()
    });
    let Some(area) = world
        .query_filtered::<Entity, With<ViewportArea>>()
        .iter(world)
        .next()
    else {
        return;
    };
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
                    b.spawn(
                        TabStrip::new("create-selection-type")
                            .compact()
                            .tab("Faces")
                            .tab("Edges")
                            .selected(if faces_mode { 0 } else { 1 })
                            .build(t),
                    );
                    b.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::new(
                            Val::Px(4.0),
                            Val::Px(4.0),
                            Val::Px(6.0),
                            Val::Px(4.0),
                        ),
                        row_gap: Val::Px(3.0),
                        ..default()
                    })
                    .with_children(|c| {
                        let rule = if faces_mode {
                            FaceRule::ALL
                                .iter()
                                .fold(Select::new("create-selection-rule"), |s, r| {
                                    s.option(r.label(), true)
                                })
                                .selected(
                                    FaceRule::ALL
                                        .iter()
                                        .position(|r| *r == FaceRule::default())
                                        .unwrap_or(0),
                                )
                        } else {
                            Select::new("create-selection-rule")
                                .option("Tangent connected", true)
                                .option("Loop", false)
                                .option("Boundary", false)
                                .selected(0)
                        };
                        c.spawn(rule.build(t));
                        c.spawn((
                            SeedField,
                            SelectionList::new("create-selection-field")
                                .placeholder("Selection")
                                .active(true)
                                .build(t),
                        ));
                        if faces_mode {
                            c.spawn((
                                BoundField,
                                SelectionList::new("create-selection-boundary")
                                    .placeholder("Boundary (Bounded faces)")
                                    .build(t),
                            ));
                        }
                        c.spawn(
                            OptionRow::new("create-selection-patterns", "Select patterns")
                                .disabled(!faces_mode)
                                .build(t),
                        );
                        c.spawn((
                            CountText,
                            Name::new("create-selection-count"),
                            t.text(
                                if faces_mode {
                                    "0 faces selected"
                                } else {
                                    "0 edges selected"
                                },
                                t.font_sm,
                                bevy::text::FontWeight::NORMAL,
                                t.muted_foreground,
                            ),
                        ));
                        c.spawn(Node {
                            justify_content: JustifyContent::Center,
                            margin: UiRect::vertical(Val::Px(3.0)),
                            ..default()
                        })
                        .with_child((
                            AddButton,
                            cadrs_ui::Button::new("create-selection-add")
                                .label("Add selection")
                                .small()
                                .outline()
                                .build(t),
                        ));
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
    let panels: Vec<Entity> = world
        .query_filtered::<Entity, With<CreateSelectionPanel>>()
        .iter(world)
        .collect();
    for e in panels {
        world.entity_mut(e).despawn();
    }
}

fn on_cancel(
    ev: On<FeatureDialogCancel>,
    q: Query<(), With<CreateSelectionPanel>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}

/// Faces | Edges: the panel again on that tab.
fn on_tab(ev: On<cadrs_ui::TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity)
        .is_ok_and(|n| n.as_str() == "create-selection-type")
    {
        let faces = ev.index == 0;
        commands.queue(move |world: &mut World| open_with(world, faces));
    }
}

/// Faces: another rule, applied to the seed face.
fn on_rule(ev: On<cadrs_ui::SelectChange>, q: Query<&Name>, mut commands: Commands) {
    if !q
        .get(ev.entity)
        .is_ok_and(|n| n.as_str() == "create-selection-rule")
    {
        return;
    }
    let Some(rule) = FaceRule::ALL.get(ev.index).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| {
        update(world, |c| c.rule = rule);
    });
}

/// Select patterns.
fn on_patterns(ev: On<cadrs_ui::CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if !q
        .get(ev.entity)
        .is_ok_and(|n| n.as_str() == "create-selection-patterns-checkbox")
    {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| update(world, |c| c.patterns = on));
}

/// Changes the Faces tab's inputs and selects again.
fn update(world: &mut World, change: impl FnOnce(&mut CreateSelection)) {
    let Some(mut c) = world.get_resource::<CreateSelection>().cloned() else {
        return;
    };
    if !c.faces_mode {
        return;
    }
    change(&mut c);
    c.faces = select_faces(world, &c);
    world.insert_resource(c);
}

/// The faces the Faces tab selects: the rule's from the seed (with the boundary), and with
/// Select patterns from each face like the seed.
fn select_faces(world: &World, c: &CreateSelection) -> Vec<(PartId, cadrs_sketch::FaceName)> {
    let Some((part, face)) = c.face_seed else {
        return Vec::new();
    };
    let Some(p) = world.resource::<PartCache>().part(part) else {
        return Vec::new();
    };
    let s = &p.solid;
    let index = |n: cadrs_sketch::FaceName| s.faces.iter().position(|f| f.name == n);
    let Some(seed) = index(face) else {
        return Vec::new();
    };
    let bound_faces: Vec<usize> = c
        .bound_faces
        .iter()
        .filter(|(q, _)| *q == part)
        .filter_map(|(_, f)| index(*f))
        .collect();
    let bound_edges: Vec<EdgeName> = c
        .bound_edges
        .iter()
        .filter(|(q, _)| *q == part)
        .map(|(_, e)| *e)
        .collect();
    let mut seeds = vec![seed];
    if c.patterns {
        seeds.extend(s.pattern_copies(seed));
    }
    let mut out: Vec<(PartId, cadrs_sketch::FaceName)> = Vec::new();
    for i in seeds {
        for j in c.rule.faces(s, i, &bound_faces, &bound_edges) {
            let f = (part, s.faces[j].name);
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// While the panel is open, a clicked edge is its seed.
fn create_selection_picks(
    mut picks: MessageReader<PickRequest>,
    create: Option<Res<CreateSelection>>,
    mut commands: Commands,
) {
    if create.is_none() {
        return;
    }
    for p in picks.read() {
        if let Some(Pick::Face(part, face)) = p.0 {
            commands.queue(move |world: &mut World| {
                update(world, |c| {
                    let f = (part, face);
                    // Bounded faces: after the seed, faces are the boundary (the seed again
                    // starts over).
                    if c.rule == FaceRule::Bounded && c.face_seed.is_some() {
                        if c.face_seed == Some(f) {
                            c.face_seed = None;
                            c.bound_faces.clear();
                            c.bound_edges.clear();
                        } else if let Some(i) = c.bound_faces.iter().position(|g| *g == f) {
                            c.bound_faces.remove(i);
                        } else {
                            c.bound_faces.push(f);
                        }
                    } else {
                        c.face_seed = Some(f);
                    }
                });
            });
            continue;
        }
        let Some(Pick::Edge(part, edge)) = p.0 else {
            continue;
        };
        // Bounded faces: an edge of the boundary.
        if create
            .as_ref()
            .is_some_and(|c| c.faces_mode && c.rule == FaceRule::Bounded)
        {
            commands.queue(move |world: &mut World| {
                update(world, |c| {
                    let e = (part, edge);
                    match c.bound_edges.iter().position(|g| *g == e) {
                        Some(i) => {
                            c.bound_edges.remove(i);
                        }
                        None => c.bound_edges.push(e),
                    }
                });
            });
            continue;
        }
        commands.queue(move |world: &mut World| {
            let edges =
                crate::advanced::tangent_connected(world.resource::<PartCache>(), part, edge);
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
    mut q_field: Query<&mut SelectionListState, (With<SeedField>, Without<BoundField>)>,
    mut q_bounds: Query<&mut SelectionListState, (With<BoundField>, Without<SeedField>)>,
    mut q_count: Query<&mut Text, With<CountText>>,
    mut selection: ResMut<Selection>,
    session: Option<Res<AppliedSession>>,
) {
    let Some(c) = create else { return };
    if !c.is_changed() {
        return;
    }
    let features = doc
        .as_ref()
        .and_then(|d| d.active_element())
        .map(|e| e.features().to_vec())
        .unwrap_or_default();
    let name = |op: cadrs_sketch::OpId| {
        features
            .iter()
            .find(|f| f.id.0 == op)
            .map_or("part", |f| f.name.as_str())
            .to_string()
    };
    let items: Vec<String> = if c.faces_mode {
        c.face_seed
            .iter()
            .map(|(_, f)| format!("Face of {}", name(f.op)))
            .collect()
    } else {
        c.seed
            .iter()
            .map(|(_, e)| format!("Edge of {}", name(crate::parts::edge_maker(&features, e))))
            .collect()
    };
    for mut l in &mut q_field {
        if l.items != items {
            l.items = items.clone();
        }
    }
    let bounds: Vec<String> = c
        .bound_faces
        .iter()
        .map(|(_, f)| format!("Face of {}", name(f.op)))
        .chain(
            c.bound_edges
                .iter()
                .map(|(_, e)| format!("Edge of {}", name(crate::parts::edge_maker(&features, e)))),
        )
        .collect();
    for mut l in &mut q_bounds {
        if l.items != bounds {
            l.items = bounds.clone();
        }
    }
    let (n, what) = if c.faces_mode {
        (c.faces.len(), "face")
    } else {
        (c.edges.len(), "edge")
    };
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
        let Some(c) = world.get_resource::<CreateSelection>().cloned() else {
            return;
        };
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
                if !x
                    .path
                    .iter()
                    .any(|p| matches!(p, PathRef::Edge(g) if g.edge == r.edge))
                {
                    x.path.push(PathRef::Edge(r));
                }
            }
        }
        (FeatureKind::Fillet(x), AppliedField::Entities) => {
            for r in refs {
                if !x
                    .entities
                    .iter()
                    .any(|e| matches!(e, EdgeOrFace::Edge(g) if g.edge == r.edge))
                {
                    x.entities.push(EdgeOrFace::Edge(r));
                }
            }
        }
        (FeatureKind::Chamfer(x), AppliedField::Entities) => {
            for r in refs {
                if !x
                    .entities
                    .iter()
                    .any(|e| matches!(e, EdgeOrFace::Edge(g) if g.edge == r.edge))
                {
                    x.entities.push(EdgeOrFace::Edge(r));
                }
            }
        }
        (FeatureKind::Plane(x), AppliedField::PlaneEntities) => {
            for r in refs {
                if !x
                    .entities
                    .iter()
                    .any(|e| matches!(e, PlaneEntity::Edge(g) if g.edge == r.edge))
                {
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

/// Adds faces to the open pattern's, mirror's or direct edit's faces (those it doesn't have
/// yet), or to the selection.
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
        FeatureKind::Pattern(x) if x.pattern_type == cadrs_core::pattern::PatternType::Face => {
            &mut x.faces
        }
        FeatureKind::Mirror(x) if x.mirror_type == cadrs_core::pattern::PatternType::Face => {
            &mut x.faces
        }
        FeatureKind::DirectEdit(x) => &mut x.faces,
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
