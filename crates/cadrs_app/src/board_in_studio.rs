//! PCB Studio boards in a Part Studio: what the app adds to a
//! Derived feature of a board ([`cadrs_core::derived::DerivedBoard`]).
//!
//! - **3D model or layout** (§6): a Derived board shows its 3D model (the board with its skins
//!   and its components' models) or its **layout**: the board without its skins, the components
//!   not drawn, and the layout as the PCB Studio draws it as a picture on the board's top face
//!   (its bottom layers on its bottom face, [`overlay_mesh`]), hidden behind other parts like
//!   any geometry. The choice
//!   is the feature's (its dialog's Show, the Feature list's menu), saved and undone like any
//!   option. [`sync_board_views`] keeps the Parts cache's hidden components and skinless boards
//!   in step; [`sync_overlays`] the layout meshes.
//! - **Grab to edit**: a press on a footprint or track of a board shown as its layout (in a
//!   Part Studio with no dialog open and no sketch edited, the board face the nearest opaque
//!   thing under the pointer) starts editing that layout and hands the press on ([`press_to_edit`]),
//!   so a footprint or track drags straight away.
//! - **Ratsnest while editing**: the edited board's unrouted connections are lines over its top
//!   face drawn from the layout as it is now ([`draw_ratsnest`]), so they follow a footprint
//!   being dragged as in the PCB Studio (the pictures leave them out: a drag redraws only the
//!   moved footprints).
//! - Scenario set-ups ([`script`]): `setup` opens the mains-sensor enclosure
//!   ([`cadrs_core::samples::pcb_enclosure`]); `derive` derives its board onto Board location.
//!
//! Names: `board-layout-<board>` (a layout mesh).

use cadrs_ui::ShortcutKey;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::GizmoGroups;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use cadrs_core::derived::{AddDerived, DerivedBoard, DerivedFeature};
use cadrs_core::external::{RefAt, SourceRef};
use cadrs_core::mate::ConnectorRef;
use cadrs_core::pcb::board3d::BoardView;
use cadrs_core::{ElementId, FeatureId, FeatureKind, PartId};
use cadrs_sketch::PlaneFrame;

use crate::parts::PartCache;
use crate::{ActiveDocument, AppState};

pub struct BoardInStudioPlugin;

impl Plugin for BoardInStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardEditing>()
            .init_resource::<OverlayCache>()
            .gizmo_group::<RatsnestGizmos>()
            .add_systems(Startup, configure_gizmos)
            .add_observer(on_edit_bar)
            .add_observer(on_link_menu)
            .add_systems(
                Update,
                (press_to_edit, navigate3d)
                    .chain()
                    .before(crate::eda::navigate)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                (
                    end_edit_when_gone,
                    sync_board_views,
                    sync_drag_ghosts,
                    sync_overlays,
                    draw_ratsnest,
                    sync_edit_bar,
                    edit_keys,
                )
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut e: ResMut<BoardEditing>| {
                e.target = None
            });
    }
}

/// The derived board copy whose layout is being edited in the Part Studio (§7), if any: its
/// layout is drawn whatever the feature's view, with the Layout tools' preview (a drag in
/// progress) and their selection, layers and highlighted net ([`crate::eda::Preview`],
/// [`crate::eda::SceneInputs`]).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct BoardEditing {
    pub target: Option<BoardTarget>,
}

/// A copy of a derived board in a Part Studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoardTarget {
    pub studio: ElementId,
    pub feature: FeatureId,
    pub copy: usize,
}

/// What the layout overlay shows beyond the layout (the Layout tools' state while editing).
#[derive(Debug, Clone, Default, PartialEq, Hash)]
pub struct OverlayView {
    pub hidden: Vec<cadrs_eda::layer::Layer>,
    pub active: Option<cadrs_eda::layer::Layer>,
    pub selected: Vec<uuid::Uuid>,
    pub net: Option<String>,
    pub dim: bool,
    /// The unrouted connections drawn in the picture (not while editing: [`draw_ratsnest`]).
    pub ratsnest: bool,
}

/// A derived board of the active Part Studio, without its layout: its tab and feature, its view,
/// its layout's hash and its name.
#[derive(Clone, Debug)]
struct BoardRef {
    studio: ElementId,
    feature: FeatureId,
    view: BoardView,
    hash: u64,
    name: String,
    off_board: bool,
}

/// A board instance of the active assembly, by its id taken as a feature's (Edit layout in
/// context): its PCB Studio, its board, and the board as the
/// studio derives it for the assembly ([`cadrs_core::pcb::PcbStudio::part_features`]).
/// (A board inside a subassembly goes by its board part's occurrence id.)
pub fn instance_board(
    doc: &ActiveDocument,
    feature: FeatureId,
) -> Option<(ElementId, cadrs_core::pcb::BoardId, &DerivedFeature)> {
    let model = doc.active_element()?.assembly_model()?;
    let (element, b) = match model.instance(cadrs_core::assembly::InstanceId(feature.0)) {
        Some(i) => match i.source {
            cadrs_core::assembly::InstanceSource::Studio {
                element,
                board: Some(b),
            } => (element, b),
            _ => return None,
        },
        // (Inside a subassembly: as the view's parts were last set up.)
        None => {
            let el = doc.active_element()?.id;
            let t = crate::assembly::BOARD_HOSTS.read().ok()?;
            let (_, pcb, b) = t.as_ref()?.get(&el)?.iter().find(|h| h.0 == feature)?;
            (*pcb, *b)
        }
    };
    let cadrs_core::ElementKind::PcbStudio(p) = &doc.doc.element(element)?.kind else {
        return None;
    };
    let f = p
        .part_features()
        .iter()
        .find(|f| f.id == cadrs_core::pcb::board_feature(b))?;
    match &f.kind {
        FeatureKind::Derived(d) => Some((element, b, d)),
        _ => None,
    }
}

/// The board an occurrence is the board part of (a PCB Studio's board, as an assembly shows it).
pub fn board_of_occurrence(
    doc: &cadrs_core::Document,
    o: &cadrs_core::assembly::structure::Occurrence,
) -> Option<cadrs_core::pcb::BoardId> {
    let s = doc.element(o.element)?.pcb()?;
    let b = s
        .boards
        .iter()
        .find(|b| cadrs_core::pcb::board_feature(b.id) == o.part.feature)?
        .id;
    let board_part = cadrs_core::derived::derived_part(
        o.part.feature,
        PartId::new(FeatureId(uuid::Uuid::nil()), 0),
        0,
    );
    (o.part == board_part).then_some(b)
}

/// The board instances of the active assembly, at any depth: (id as a feature's, name).
fn assembly_boards(doc: &ActiveDocument) -> Vec<(FeatureId, String)> {
    let Some(el) = doc
        .active_element()
        .filter(|e| e.assembly_model().is_some())
    else {
        return vec![];
    };
    // (As the view's parts were last set up: no walk of the assembly every frame.)
    let out: Vec<FeatureId> = crate::assembly::BOARD_HOSTS
        .read()
        .ok()
        .and_then(|t| {
            t.as_ref()?
                .get(&el.id)
                .map(|h| h.iter().map(|x| x.0).collect())
        })
        .unwrap_or_default();
    out.into_iter()
        .filter_map(|f| {
            Some((
                f,
                instance_board(doc, f)?.2.board.as_ref()?.board_name.clone(),
            ))
        })
        .collect()
}

/// The board instances inside the subassembly instance `top` of the active assembly: (id as a
/// feature's, board name).
pub fn boards_inside(
    doc: &ActiveDocument,
    top: cadrs_core::assembly::InstanceId,
) -> Vec<(FeatureId, String)> {
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else {
        return vec![];
    };
    cadrs_core::assembly::structure::occurrences(&doc.doc, model)
        .iter()
        .filter(|o| o.top == top && o.child.is_some() && board_of_occurrence(&doc.doc, o).is_some())
        .filter_map(|o| {
            Some((
                FeatureId(o.id.0),
                instance_board(doc, FeatureId(o.id.0))?
                    .2
                    .board
                    .as_ref()?
                    .board_name
                    .clone(),
            ))
        })
        .collect()
}

/// The derived boards of the active Part Studio, or the board instances of the active assembly
/// (no layout copied: this runs every frame).
fn boards_in(doc: &ActiveDocument) -> Vec<BoardRef> {
    let Some(el) = doc.active_element() else {
        return vec![];
    };
    if el.assembly_model().is_some() {
        return assembly_boards(doc)
            .into_iter()
            .filter_map(|(feature, _)| {
                let b = instance_board(doc, feature)?.2.board.as_ref()?;
                Some(BoardRef {
                    studio: el.id,
                    feature,
                    view: BoardView::ThreeD,
                    hash: b.hash,
                    name: b.board_name.clone(),
                    off_board: false,
                })
            })
            .collect();
    }
    el.features()
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Derived(d) => d.board.as_ref().map(|b| BoardRef {
                studio: el.id,
                feature: f.id,
                view: b.options.view,
                hash: b.hash,
                name: b.board_name.clone(),
                off_board: b.options.off_board,
            }),
            _ => None,
        })
        .collect()
}

/// The layout of the derived board `feature` of the active Part Studio.
fn layout_of(doc: &ActiveDocument, feature: FeatureId) -> Option<&cadrs_eda::board::Board> {
    if let Some((_, _, d)) = instance_board(doc, feature) {
        return d.board.as_ref().map(|b| &b.layout);
    }
    match &doc.active_element()?.feature(feature)?.kind {
        FeatureKind::Derived(d) => d.board.as_ref().map(|b| &b.layout),
        _ => None,
    }
}

/// Keeps the Parts cache's layout-hidden components and skinless boards in step with the
/// derived boards' views (and the board being edited).
pub fn sync_board_views(
    doc: Option<Res<ActiveDocument>>,
    editing: Res<BoardEditing>,
    mut cache: ResMut<PartCache>,
) {
    let Some(doc) = doc else { return };
    let mut hidden = HashSet::new();
    let mut skinless = HashSet::new();
    for b in boards_in(&doc) {
        let Some(out) = cache.derived.get(&b.feature) else {
            continue;
        };
        let edited = editing
            .target
            .filter(|t| t.studio == b.studio && t.feature == b.feature);
        for (k, copy) in out.boards.iter().enumerate() {
            let layout = b.view == BoardView::Layout;
            if layout || edited.is_some_and(|t| t.copy == k) {
                skinless.insert(copy.part);
            }
            if layout {
                hidden.extend(component_parts(out, k));
            }
        }
    }
    if cache.layout_hidden != hidden || cache.skinless != skinless {
        cache.layout_hidden = hidden;
        cache.skinless = skinless;
        cache.generation += 1;
    }
}

/// The component parts of copy `k` of a derived board (they follow its board part in the
/// output, up to the next copy's board part).
pub fn component_parts(out: &cadrs_core::derived::DerivedOutput, k: usize) -> Vec<PartId> {
    let Some(start) = out
        .parts
        .iter()
        .position(|p| out.boards.get(k).is_some_and(|b| b.part == *p))
    else {
        return vec![];
    };
    out.parts[start + 1..]
        .iter()
        .take_while(|p| !out.boards.iter().any(|b| b.part == **p))
        .copied()
        .collect()
}

/// A layout mesh in the view.
#[derive(Component)]
pub struct LayoutOverlay;

/// An overlay's key: (feature, copy, top face, the changes in progress).
type OverlayKey = (FeatureId, usize, bool, bool);

/// An overlay's mesh and picture.
type OverlayMade = Option<(Mesh, Image)>;

/// The layout meshes on screen, by what they show.
#[derive(Resource, Default)]
pub struct OverlayCache {
    /// By (feature, copy, top face, the changes in progress).
    shown: HashMap<OverlayKey, (u64, Entity)>,
    /// Whole layouts being rasterised off the main thread (a commit of a big board would
    /// stall the view): what each will show (its signature, name, face) once ready; the
    /// picture it replaces stays meanwhile.
    pending: HashMap<OverlayKey, (u64, String, bevy::tasks::Task<OverlayMade>)>,
}

/// How much of a layout an overlay shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayArea {
    /// The board's area.
    Board,
    /// Everything drawn, the footprints off the board too.
    Everything,
    /// The edit in progress ([`preview_changes`]), over the picture of the stored layout.
    Changes,
}

/// What a tool's preview `preview` has that `stored` hasn't (new or changed footprints, tracks,
/// vias, zones, shapes and texts), as a layout of its own; `None` when nothing.
pub fn preview_changes(
    stored: &cadrs_eda::board::Board,
    preview: &cadrs_eda::board::Board,
) -> Option<cadrs_eda::board::Board> {
    fn changed<T: Clone + PartialEq>(
        old: &[T],
        new: &[T],
        id: impl Fn(&T) -> uuid::Uuid,
    ) -> Vec<T> {
        let by_id: HashMap<uuid::Uuid, &T> = old.iter().map(|x| (id(x), x)).collect();
        new.iter()
            .filter(|x| by_id.get(&id(x)).is_none_or(|o| *o != *x))
            .cloned()
            .collect()
    }
    let out = cadrs_eda::board::Board {
        footprints: changed(&stored.footprints, &preview.footprints, |f| f.id),
        tracks: changed(&stored.tracks, &preview.tracks, |t| t.id),
        vias: changed(&stored.vias, &preview.vias, |v| v.id),
        zones: changed(&stored.zones, &preview.zones, |z| z.id),
        shapes: changed(&stored.shapes, &preview.shapes, |s| s.id),
        texts: changed(&stored.texts, &preview.texts, |t| t.id),
        copper_layers: preview.copper_layers,
        thickness: preview.thickness,
        nets: preview.nets.clone(),
        groups: vec![],
        rules: preview.rules.clone(),
        stackup: preview.stackup.clone(),
        paper: Default::default(),
        title_block: Default::default(),
    };
    let empty = out.footprints.is_empty()
        && out.tracks.is_empty()
        && out.vias.is_empty()
        && out.zones.is_empty()
        && out.shapes.is_empty()
        && out.texts.is_empty();
    (!empty).then_some(out)
}

/// Height of the layout above the board's top face (mm): clear of the face, close enough to
/// read as printed on it.
pub const LIFT: f64 = 0.012;

/// Pixels per mm of a layout overlay (fine enough for 0.8 mm silkscreen text).
pub const OVERLAY_PX_PER_MM: f32 = 32.0;

/// The layout drawn on one face of a board copy whose frame is `frame` (its bottom face;
/// `thickness` mm thick), in Part Studio coordinates: a picture of the layout as the PCB Studio
/// draws it ([`cadrs_eda::skin::raster`]) on one rectangle — on the top face the top layers and
/// those of no side (the outline, drawings, inner copper), on the bottom face the bottom ones,
/// in layout coordinates (so seen from below they read as the bottom does). `area` says how
/// much of it (clear where nothing is drawn). The mesh and its picture.
pub fn overlay_mesh(
    layout: &cadrs_eda::board::Board,
    view: &OverlayView,
    frame: &PlaneFrame,
    thickness: f64,
    side: cadrs_eda::layer::Side,
    area: OverlayArea,
) -> Option<(Mesh, Image)> {
    use cadrs_eda::layer::{Layer, Side};
    let mut v = cadrs_eda::render::BoardView::all(layout);
    let other = match side {
        Side::Top => Side::Bottom,
        Side::Bottom => Side::Top,
    };
    v.visible.retain(|l| {
        !view.hidden.contains(l)
            && l.side() != Some(other)
            && (side == Side::Top || l.side() == Some(side) || *l == Layer::Outline)
    });
    v.dim_inactive = view.dim;
    v.ratsnest = view.ratsnest;
    v.highlight_net = view.net.clone();
    if let Some(a) = view.active {
        v.active = a;
    }
    v.selected = view.selected.clone();
    let list = cadrs_eda::render::board(layout, &cadrs_eda::render::BoardTheme::default(), &v);
    let region = cadrs_eda::outline::board_region(layout);
    let of = |r: &cadrs_eda::poly::Region| {
        r.iter().flatten().fold(None, |acc, p| {
            Some(cadrs_eda::units::Bounds::union(
                acc,
                cadrs_eda::units::Bounds::of(*p),
            ))
        })
    };
    let (bounds, clip) = match area {
        OverlayArea::Board => (of(&region)?, Some(&region)),
        OverlayArea::Everything | OverlayArea::Changes => {
            (list.bounds.or_else(|| of(&region))?, None)
        }
    };
    let skin = cadrs_eda::skin::raster(&list, bounds, OVERLAY_PX_PER_MM, 4096, clip)?;
    // The changes lie over the picture of the rest.
    let lift = if area == OverlayArea::Changes {
        2.0 * LIFT
    } else {
        LIFT
    };
    let n = frame.normal();
    let z = match side {
        Side::Top => thickness + lift,
        Side::Bottom => -lift,
    };
    let at = |x: f64, y: f64| cadrs_core::pcb::frames::to_world(frame, [x, y, z]).map(|c| c as f32);
    let mm = |v: i64| v as f64 / 1e6;
    let (x0, y0, x1, y1) = (
        mm(bounds.min.x),
        mm(bounds.min.y),
        mm(bounds.max.x),
        mm(bounds.max.y),
    );
    let positions = vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    let k = if side == Side::Top { 1.0 } else { -1.0 };
    let normal = [(n[0] * k) as f32, (n[1] * k) as f32, (n[2] * k) as f32];
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal; 4])
    // Row 0 of the picture is the board's highest y.
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    )
    // Its front faces out of its face (the bottom one down).
    .with_inserted_indices(Indices::U32(if side == Side::Top {
        vec![0, 1, 2, 0, 2, 3]
    } else {
        vec![0, 2, 1, 0, 3, 2]
    }));
    let image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: skin.width,
            height: skin.height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        skin.rgba,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    Some((mesh, image))
}

/// Keeps layout meshes (top and bottom face) on every derived board copy shown as its layout,
/// and on the one being edited; the layout is copied only when a mesh is made again.
pub fn sync_overlays(world: &mut World) {
    use cadrs_eda::layer::Side;
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let boards = boards_in(doc);
    let editing = world.resource::<BoardEditing>().clone();
    let preview = world.resource::<crate::eda::Preview>();
    let (has_preview, preview_generation) = (preview.get().is_some(), preview.generation());
    // The footprints the preview moves: left out of the stored layout's picture while it lasts
    // (drawn once when the drag starts), so a moved part shows only where it is going.
    let masked: Vec<uuid::Uuid> = match (editing.target, preview.get()) {
        (Some(t), Some(p)) => layout_of(world.resource::<ActiveDocument>(), t.feature)
            .map(|stored| {
                p.board
                    .footprints
                    .iter()
                    .filter(|f| {
                        stored
                            .footprints
                            .iter()
                            .find(|g| g.id == f.id)
                            .is_none_or(|g| g != *f)
                    })
                    .map(|f| f.id)
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    };
    let inputs = world.resource::<crate::eda::SceneInputs>().clone();
    let edit_view = OverlayView {
        hidden: inputs.hidden_layers.clone(),
        active: inputs.active_layer,
        selected: inputs.board_selected.clone(),
        net: inputs.board_net.clone(),
        dim: inputs.dim_inactive,
        ratsnest: false,
    };
    // Not editing: the layers panel's hidden layers still hold.
    let plain_view = OverlayView {
        hidden: world
            .resource::<crate::eda::layout_tools::LayoutState>()
            .hidden
            .clone(),
        ratsnest: true,
        ..Default::default()
    };
    let cache = world.resource::<PartCache>();
    // (key: feature, copy, top face, the changes; signature, frame, thickness, name, what else it
    // shows, edited, parts off the board). While a tool previews an edit (a drag) the picture of
    // the stored layout stays and a small one of the changes lies over it: the whole layout is
    // drawn again once, when the edit is done.
    type Key = (FeatureId, usize, bool, bool);
    type Want = (Key, u64, PlaneFrame, f64, String, OverlayView, bool, bool);
    let mut want: Vec<Want> = vec![];
    for b in &boards {
        let Some(out) = cache.derived.get(&b.feature) else {
            continue;
        };
        for (k, copy) in out.boards.iter().enumerate() {
            let edited = editing.target
                == Some(BoardTarget {
                    studio: b.studio,
                    feature: b.feature,
                    copy: k,
                });
            if !(b.view == BoardView::Layout || edited) || cache.is_hidden(copy.part) {
                continue;
            }
            let view = if edited {
                edit_view.clone()
            } else {
                plain_view.clone()
            };
            for top in [true, false] {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (b.hash, top).hash(&mut h);
                view.hash(&mut h);
                format!("{:?}{}", copy.frame, copy.thickness).hash(&mut h);
                b.off_board.hash(&mut h);
                if edited {
                    masked.hash(&mut h);
                }
                let sig = h.finish();
                want.push((
                    (b.feature, k, top, false),
                    sig,
                    copy.frame,
                    copy.thickness,
                    b.name.clone(),
                    view.clone(),
                    edited,
                    b.off_board,
                ));
                if edited && has_preview {
                    want.push((
                        (b.feature, k, top, true),
                        sig ^ preview_generation,
                        copy.frame,
                        copy.thickness,
                        b.name.clone(),
                        view.clone(),
                        edited,
                        b.off_board,
                    ));
                }
            }
        }
    }
    let keys: HashSet<Key> = want.iter().map(|w| w.0).collect();
    let stale: Vec<(Key, Entity)> = world
        .resource::<OverlayCache>()
        .shown
        .iter()
        .filter(|(k, _)| !keys.contains(k))
        .map(|(k, (_, e))| (*k, *e))
        .collect();
    for (k, e) in stale {
        if let Ok(ec) = world.get_entity_mut(e) {
            ec.despawn();
        }
        world.resource_mut::<OverlayCache>().shown.remove(&k);
    }
    world
        .resource_mut::<OverlayCache>()
        .pending
        .retain(|k, _| keys.contains(k));
    // The pictures made off the main thread that are ready.
    let ready: Vec<Key> = world
        .resource::<OverlayCache>()
        .pending
        .keys()
        .copied()
        .collect();
    let mut done: Vec<(Key, u64, String, OverlayMade)> = vec![];
    for k in ready {
        let mut cache = world.resource_mut::<OverlayCache>();
        let Some((_, _, task)) = cache.pending.get_mut(&k) else {
            continue;
        };
        if let Some(made) = bevy::tasks::futures::check_ready(task) {
            let (sig, name, _) = cache.pending.remove(&k).expect("just polled");
            done.push((k, sig, name, made));
        }
    }
    for (key, sig, name, made) in done {
        if let Some((_, e)) = world.resource_mut::<OverlayCache>().shown.remove(&key)
            && let Ok(ec) = world.get_entity_mut(e)
        {
            ec.despawn();
        }
        if let Some((mesh, image)) = made {
            spawn_overlay(world, key, sig, &name, mesh, image);
        }
    }
    for (key, sig, frame, thickness, name, view, edited, off_board) in want {
        if world
            .resource::<OverlayCache>()
            .shown
            .get(&key)
            .is_some_and(|(s, _)| *s == sig)
            || world
                .resource::<OverlayCache>()
                .pending
                .get(&key)
                .is_some_and(|(s, _, _)| *s == sig)
        {
            continue;
        }
        let side = if key.2 { Side::Top } else { Side::Bottom };
        let area = if off_board {
            OverlayArea::Everything
        } else {
            OverlayArea::Board
        };
        // A drag's changes (small, following the pointer): now. A whole layout: off the main
        // thread, the old picture shown until it is ready.
        let made = {
            let doc = world.resource::<ActiveDocument>();
            let Some(stored) = layout_of(doc, key.0) else {
                continue;
            };
            if key.3 {
                let preview = world
                    .resource::<crate::eda::Preview>()
                    .get()
                    .filter(|_| edited)
                    .map(|d| &d.board);
                let Some(changes) = preview.and_then(|p| preview_changes(stored, p)) else {
                    continue;
                };
                overlay_mesh(
                    &changes,
                    &view,
                    &frame,
                    thickness,
                    side,
                    OverlayArea::Changes,
                )
            } else {
                let mut layout = stored.clone();
                if edited && !masked.is_empty() {
                    layout.footprints.retain(|f| !masked.contains(&f.id));
                }
                let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                    overlay_mesh(&layout, &view, &frame, thickness, side, area)
                });
                world
                    .resource_mut::<OverlayCache>()
                    .pending
                    .insert(key, (sig, name, task));
                continue;
            }
        };
        if let Some((_, e)) = world.resource_mut::<OverlayCache>().shown.remove(&key)
            && let Ok(ec) = world.get_entity_mut(e)
        {
            ec.despawn();
        }
        let Some((mesh, image)) = made else { continue };
        spawn_overlay(world, key, sig, &name, mesh, image);
    }
}

/// The edited board's ratsnest: thin lines just over its top face.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct RatsnestGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<RatsnestGizmos>();
    c.line.width = 1.0;
    // Over the layout picture it lies on, behind the parts in front of it.
    c.depth_bias = -0.05;
}

/// Draws the edited board's ratsnest from the layout as it is now (the Layout tools' preview
/// while they change it, else the stored layout), worked out again only when that changes.
fn draw_ratsnest(
    doc: Option<Res<ActiveDocument>>,
    editing: Res<BoardEditing>,
    preview: Res<crate::eda::Preview>,
    cache: Res<PartCache>,
    mut gizmos: Gizmos<RatsnestGizmos>,
    mut lines: Local<(u64, Vec<[Vec3; 2]>)>,
) {
    let (Some(doc), Some(t)) = (doc, editing.target) else {
        return;
    };
    let Some(copy) = cache
        .derived
        .get(&t.feature)
        .and_then(|out| out.boards.get(t.copy))
    else {
        return;
    };
    if cache.is_hidden(copy.part) {
        return;
    }
    let Some(b) = boards_in(&doc).into_iter().find(|b| b.feature == t.feature) else {
        return;
    };
    let layout = preview.get().map(|d| &d.board);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (t, b.hash, layout.is_some(), preview.generation()).hash(&mut h);
    format!("{:?}{}", copy.frame, copy.thickness).hash(&mut h);
    let key = h.finish();
    if lines.0 != key {
        let Some(board) = layout.or_else(|| layout_of(&doc, t.feature)) else {
            return;
        };
        let z = copy.thickness + 2.0 * LIFT;
        let at = |p: cadrs_eda::units::Pt| {
            let c = cadrs_core::pcb::frames::to_world(
                &copy.frame,
                [p.x as f64 / 1e6, p.y as f64 / 1e6, z],
            );
            Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32)
        };
        // As the picture: only on the board unless what is off it shows too.
        let region = (!b.off_board).then(|| cadrs_eda::outline::board_region(board));
        let mut out = vec![];
        for a in cadrs_eda::copper::ratsnest(board) {
            match &region {
                None => out.push([at(a.a), at(a.b)]),
                Some(r) => out.extend(inside(r, a.a, a.b).into_iter().map(|[p, q]| [at(p), at(q)])),
            }
        }
        *lines = (key, out);
    }
    let [r, g, bl, a] = cadrs_eda::render::BoardTheme::default().ratsnest;
    let color = Color::srgba_u8(r, g, bl, a);
    for [a, b] in &lines.1 {
        gizmos.line(*a, *b, color);
    }
}

/// The pieces of the line from `a` to `b` inside `region`, to half a millimetre.
fn inside(
    region: &cadrs_eda::poly::Region,
    a: cadrs_eda::units::Pt,
    b: cadrs_eda::units::Pt,
) -> Vec<[cadrs_eda::units::Pt; 2]> {
    let len = ((b.x - a.x) as f64).hypot((b.y - a.y) as f64) / 1e6;
    let n = ((len * 2.0).ceil() as usize).clamp(1, 1000);
    let at = |k: usize| {
        let t = k as f64 / n as f64;
        cadrs_eda::units::Pt::new(
            a.x + ((b.x - a.x) as f64 * t) as i64,
            a.y + ((b.y - a.y) as f64 * t) as i64,
        )
    };
    let mut out = vec![];
    let mut run: Option<(cadrs_eda::units::Pt, cadrs_eda::units::Pt)> = None;
    for k in 0..=n {
        let p = at(k);
        if cadrs_eda::poly::contains(region, p) {
            run = Some((run.map_or(p, |r| r.0), p));
        } else if let Some((p0, p1)) = run.take() {
            out.push([p0, p1]);
        }
    }
    out.extend(run.map(|(p0, p1)| [p0, p1]));
    out.retain(|[p, q]| p != q);
    out
}

/// Puts an overlay picture on screen for `key` (feature, copy, top face, changes).
fn spawn_overlay(
    world: &mut World,
    key: OverlayKey,
    sig: u64,
    name: &str,
    mesh: Mesh,
    image: Image,
) {
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    let image = world.resource_mut::<Assets<Image>>().add(image);
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            // Opaque, its pixels off the board left out, as the board's skin
            // ([`crate::picture`]): not sorted against the board or the parts around.
            alpha_mode: AlphaMode::Mask(0.5),
            // Seen only from outside its face: through the board the other face's would show.
            double_sided: false,
            cull_mode: Some(bevy::render::render_resource::Face::Back),
            // Over the board's own face (it lies [`LIFT`] off it too), not over the components.
            depth_bias: crate::picture::SURFACE_BIAS,
            ..default()
        });
    let label = format!(
        "board-layout-{}{}{}",
        crate::pcb::slug(name),
        if key.2 { "" } else { "-bottom" },
        if key.3 { "-changes" } else { "" }
    );
    let e = world
        .spawn((
            Name::new(label),
            LayoutOverlay,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            DespawnOnExit(AppState::Document),
        ))
        .id();
    world
        .resource_mut::<OverlayCache>()
        .shown
        .insert(key, (sig, e));
}

/// Sets a derived board's view (the Feature list's menu): one undo step.
pub fn set_view(world: &mut World, feature: FeatureId, view: BoardView) {
    let Some(el) = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id)
    else {
        return;
    };
    let Some(mut d) = crate::derived_ui::derived_in_active(world, feature) else {
        return;
    };
    let Some(b) = d.board.as_mut() else { return };
    if b.options.view == view {
        return;
    }
    b.options.view = view;
    let label = match view {
        BoardView::Layout => "Show board layout",
        BoardView::ThreeD => "Show board 3D model",
    };
    if let Err(e) =
        world
            .resource_mut::<ActiveDocument>()
            .execute(&cadrs_core::derived::SetDerived {
                element: el,
                feature,
                derived: d,
                links: vec![],
                label: label.into(),
            })
    {
        crate::linked::error_toast(world, e.to_string());
    }
}

// ---------------------------------------------------------------------------------------------
// Editing a board's layout in the Part Studio (§7)

/// The board copy being edited, if it can be: the derived board's PCB Studio, board and plane
/// in this Part Studio. Only a workspace reference of this document can be edited (a version's
/// copy is read-only, as derived parts are).
pub fn edited_board(
    doc: &ActiveDocument,
    editing: &BoardEditing,
    cache: &PartCache,
) -> Option<(ElementId, cadrs_core::pcb::BoardId, crate::eda::PlaneHost)> {
    let t = editing.target?;
    let el = doc.active_element().filter(|e| e.id == t.studio)?;
    // A board instance of an assembly (in context), or a derived board of a Part Studio.
    let (pcb, board) = match instance_board(doc, t.feature) {
        Some((pcb, board, _)) => (pcb, board),
        None => {
            let FeatureKind::Derived(d) = &el.feature(t.feature)?.kind else {
                return None;
            };
            let (r, b) = (d.source?, d.board.as_ref()?);
            if r.at != RefAt::Workspace || r.document_or(doc.doc.id) != doc.doc.id {
                return None;
            }
            (r.element, b.board)
        }
    };
    doc.doc.element(pcb)?.pcb()?.board(board)?.design()?;
    let copy = cache.derived.get(&t.feature)?.boards.get(t.copy)?;
    Some((
        pcb,
        board,
        crate::eda::PlaneHost {
            target: t,
            frame: copy.frame,
            thickness: copy.thickness,
        },
    ))
}

/// Whether the derived board `feature` of the active Part Studio can have its layout edited
/// here (a workspace reference of this document), and if not why.
pub fn editable(world: &World, feature: FeatureId) -> Result<(), String> {
    let doc = world.resource::<ActiveDocument>();
    // A board instance of an assembly: its PCB Studio's own board.
    if let Some((pcb, board, d)) = instance_board(doc, feature) {
        let name = d
            .board
            .as_ref()
            .map(|b| b.board_name.clone())
            .unwrap_or_default();
        return match doc
            .doc
            .element(pcb)
            .and_then(|e| e.pcb())
            .and_then(|s| s.board(board))
        {
            Some(b) if b.design().is_some() => Ok(()),
            Some(_) => Err(format!(
                "{name} came from IDF: it has no layout to edit here (edit it where it was designed, then import it again)"
            )),
            None => Err(format!("{name} is gone")),
        };
    }
    let d = crate::derived_ui::derived_in_active(world, feature).ok_or("not a Derived feature")?;
    let (r, b) = (
        d.source.ok_or("no source")?,
        d.board.as_ref().ok_or("not a board")?,
    );
    if r.at != RefAt::Workspace || r.document_or(doc.doc.id) != doc.doc.id {
        return Err(format!(
            "{} is referenced at a version: open the linked document to edit its layout",
            b.board_name
        ));
    }
    // An IDF board (from another tool) shows its outline and package boxes, read-only.
    if doc
        .doc
        .element(r.element)
        .and_then(|e| e.pcb())
        .and_then(|s| s.board(b.board))
        .is_some_and(|x| x.design().is_none())
    {
        return Err(format!(
            "{} came from IDF: it has no layout to edit here (edit it where it was designed, then import it again)",
            b.board_name
        ));
    }
    Ok(())
}

/// Starts editing the layout of copy `copy` of the derived board `feature` (the Feature list's
/// Edit layout): the Layout's tools work on it in the view, the layout drawn on its face.
pub fn begin_edit(world: &mut World, feature: FeatureId, copy: usize) {
    if let Err(e) = editable(world, feature) {
        crate::linked::error_toast(world, e);
        return;
    }
    let Some(studio) = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id)
    else {
        return;
    };
    world.resource_mut::<BoardEditing>().target = Some(BoardTarget {
        studio,
        feature,
        copy,
    });
    crate::eda::layout_tools::set_tool(world, crate::eda::layout_tools::Tool::Select);
    world
        .resource_mut::<crate::eda::layout_tools::LayoutState>()
        .selection
        .clear();
    // (What was selected in the view, an assembly's instance say, would be drawn over it.)
    world.resource_mut::<crate::viewport::Selection>().0.clear();
}

/// Stops editing (Done, Esc with nothing going on, another tab).
pub fn end_edit(world: &mut World) {
    if world.resource::<BoardEditing>().target.is_none() {
        return;
    }
    crate::eda::layout_tools::set_tool(world, crate::eda::layout_tools::Tool::Select);
    world
        .resource_mut::<crate::eda::layout_tools::LayoutState>()
        .selection
        .clear();
    // (The PCB Studio's flat view picks either side.)
    world
        .resource_mut::<crate::eda::layout_tools::LayoutState>()
        .facing = None;
    world.resource_mut::<crate::board_fit::FitState>().align = crate::board_fit::AlignStep::Off;
    world.resource_mut::<BoardEditing>().target = None;
}

/// Leaves the edit when its board is gone or another tab is shown.
fn end_edit_when_gone(world: &mut World) {
    let Some(t) = world.resource::<BoardEditing>().target else {
        return;
    };
    let gone = {
        let doc = world.resource::<ActiveDocument>();
        doc.active_element().is_none_or(|e| e.id != t.studio)
            || crate::derived_ui::derived_board_ref(world, t.feature).is_none()
    };
    if gone {
        end_edit(world);
    }
}

/// The bar over the view while a board's layout is edited.
#[derive(Component)]
struct EditBar;

/// Keeps the edit bar on screen while editing: "Layout of <board> (<studio>)", the fit tools,
/// Move by…, Open in PCB Studio, Done.
fn sync_edit_bar(
    world: &mut World,
    q: &mut QueryState<Entity, With<EditBar>>,
    qa: &mut QueryState<Entity, With<crate::viewport::ViewportArea>>,
    mut was_editing: Local<bool>,
) {
    let want = world
        .resource::<crate::eda::Eda2d>()
        .plane()
        .map(|h| h.target);
    let title = want.and_then(|t| {
        let d = crate::derived_ui::derived_ref(world, t.feature)?;
        let b = d.board.as_ref()?;
        // (An assembly's board instance: its PCB Studio's tab name.)
        let doc = world.resource::<ActiveDocument>();
        let studio = instance_board(doc, t.feature)
            .and_then(|(pcb, _, _)| doc.doc.element(pcb))
            .map_or_else(|| d.source_name.clone(), |e| e.name.clone());
        Some(format!("Layout of {} ({studio})", b.board_name))
    });
    let existing = q.iter(world).next();
    match (title, existing) {
        (Some(title), None) => {
            let Some(area) = qa.iter(world).next() else {
                return;
            };
            let t = world.resource::<cadrs_ui::Theme>().clone();
            let fit = world.resource::<crate::board_fit::FitState>().context_alpha;
            let t2 = t.clone();
            world.commands().entity(area).with_children(|vp| {
                vp.spawn((
                    EditBar,
                    cadrs_ui::FloatBar::new("board-edit-bar")
                        .top(8.0)
                        .build(&t, move |b| {
                            let t = t2;
                            b.spawn(cadrs_ui::icon("pcb-studio", 14.0, t.muted_foreground));
                            b.spawn((
                                Name::new("board-edit-title"),
                                t.text(
                                    title,
                                    t.font_base,
                                    bevy::text::FontWeight::BOLD,
                                    t.foreground,
                                ),
                            ));
                            for (name, label, tip) in crate::board_fit::BUTTONS {
                                b.spawn(
                                    cadrs_ui::Button::new(name)
                                        .label(label)
                                        .small()
                                        .outline()
                                        .tooltip(tip)
                                        .build(&t),
                                );
                                if name == "board-fit-context" {
                                    b.spawn(
                                        cadrs_ui::dialog_fields::Slider::new(
                                            crate::board_fit::CONTEXT_OPACITY,
                                        )
                                        .value((fit - 0.1) / 0.9)
                                        .width(64.0)
                                        .tooltip("How see-through the parts around are")
                                        .build(&t),
                                    );
                                }
                            }
                            b.spawn(
                                cadrs_ui::Button::new("board-edit-moveby")
                                    .label("Move by…")
                                    .small()
                                    .outline()
                                    .tooltip(
                                        "Move the selected footprints by typed distances (Shift+M)",
                                    )
                                    .build(&t),
                            );
                            b.spawn(
                                cadrs_ui::Button::new("board-edit-open")
                                    .label("Open in PCB Studio")
                                    .small()
                                    .outline()
                                    .tooltip("Show the board in its PCB Studio")
                                    .build(&t),
                            );
                            b.spawn(
                                cadrs_ui::Button::new("board-edit-done")
                                    .label("Done")
                                    .small()
                                    .primary()
                                    .tooltip("Stop editing the layout (Esc)")
                                    .build(&t),
                            );
                        }),
                ));
            });
            world.flush();
        }
        (None, Some(e)) => {
            world.entity_mut(e).despawn();
        }
        _ => {}
    }
    // The fit buttons and the status line are there only while a board's layout is edited (and
    // the frame after, to take the status line away): looking for them went through every named
    // entity every frame.
    let editing = want.is_some();
    if editing || *was_editing {
        sync_fit_buttons(world);
    }
    *was_editing = editing;
}

/// The fit buttons show their state (on: selected); Align's prompt shows under the bar.
fn sync_fit_buttons(world: &mut World) {
    let state = world.resource::<crate::board_fit::FitState>().clone();
    let mut q = world.query::<(Entity, &Name, Has<cadrs_ui::style::Selected>)>();
    let changes: Vec<(Entity, bool)> = q
        .iter(world)
        .filter(|(_, n, _)| crate::board_fit::BUTTONS.iter().any(|b| b.0 == n.as_str()))
        .filter_map(|(e, n, sel)| {
            let on = crate::board_fit::is_on(&state, n.as_str());
            (on != sel).then_some((e, on))
        })
        .collect();
    for (e, on) in changes {
        if on {
            world.entity_mut(e).insert(cadrs_ui::style::Selected);
        } else {
            world.entity_mut(e).remove::<cadrs_ui::style::Selected>();
        }
    }
    let host = world.resource::<crate::eda::Eda2d>().plane().copied();
    // Align's prompt; else, in an assembly, the edited board's components running into the
    // other instances (a Part Studio's Derived feature says so in its warning).
    let prompt: Option<String> = host.and_then(|h| {
        crate::board_fit::align_prompt(&state)
            .map(String::from)
            .or_else(|| {
                let doc = world.resource::<ActiveDocument>();
                instance_board(doc, h.target.feature)?;
                let cache = world.resource::<PartCache>();
                let name = |p: PartId| {
                    cache
                        .part(p)
                        .map(|x| cadrs_core::parts::display_name(x, &cache.props).to_string())
                        .unwrap_or_default()
                };
                // One line: each part run into, with the components (by reference) in its way.
                let mut by: Vec<(PartId, Vec<String>)> = vec![];
                for c in &cache.derived.get(&h.target.feature)?.collisions {
                    let r = name(c.component)
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    match by.iter_mut().find(|(p, _)| *p == c.other) {
                        Some((_, v)) => v.push(r),
                        None => by.push((c.other, vec![r])),
                    }
                }
                let lines: Vec<String> = by
                    .iter()
                    .map(|(p, v)| format!("{}: {}", name(*p), v.join(", ")))
                    .collect();
                (!lines.is_empty()).then(|| format!("⚠ Components run into {}", lines.join(" · ")))
            })
    });
    let mut q = world.query::<(Entity, &Name)>();
    let existing = q
        .iter(world)
        .find(|(_, n)| n.as_str() == "board-fit-status")
        .map(|(e, _)| e);
    match (prompt, existing) {
        (Some(text), None) => {
            let mut qa = world.query_filtered::<Entity, With<crate::viewport::ViewportArea>>();
            let Some(area) = qa.iter(world).next() else {
                return;
            };
            let t = world.resource::<cadrs_ui::Theme>().clone();
            let t2 = t.clone();
            world.commands().entity(area).with_children(|vp| {
                vp.spawn(
                    cadrs_ui::FloatBar::new("board-fit-status")
                        .top(82.0)
                        .prompt()
                        .build(&t, move |b| {
                            b.spawn(t2.text(
                                text,
                                t2.font_sm,
                                bevy::text::FontWeight::NORMAL,
                                t2.foreground,
                            ));
                        }),
                );
            });
            world.flush();
        }
        (Some(text), Some(e)) => {
            let child = world.get::<Children>(e).and_then(|c| c.first().copied());
            if let Some(c) = child
                && let Some(mut tx) = world.get_mut::<Text>(c)
                && tx.0 != text
            {
                tx.0 = text;
            }
        }
        (None, Some(e)) => {
            // The prompt's row with it.
            let row = world.get::<ChildOf>(e).map(|c| c.parent()).unwrap_or(e);
            world.entity_mut(row).despawn();
        }
        _ => {}
    }
}

fn on_edit_bar(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    match n.as_str() {
        "board-edit-done" => commands.queue(end_edit),
        "board-edit-open" => commands.queue(open_in_pcb_studio),
        "board-edit-moveby" => commands.queue(|w: &mut World| {
            let ids: Vec<uuid::Uuid> = w
                .resource::<crate::eda::layout_tools::LayoutState>()
                .selection
                .iter()
                .filter_map(|i| {
                    if let cadrs_eda::board_edit::BoardItem::Footprint(id) = i {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();
            crate::eda::lay_dialogs::open_move_by(w, ids);
        }),
        _ => {}
    }
}

/// Shows the edited board in its PCB Studio (and stops editing here).
fn open_in_pcb_studio(world: &mut World) {
    let eda = world.resource::<crate::eda::Eda2d>();
    let Some((pcb, board, _)) = eda.board().filter(|_| eda.plane().is_some()) else {
        return;
    };
    end_edit(world);
    world.resource_mut::<ActiveDocument>().set_active(pcb);
    crate::pcb::show_board(world, pcb, board);
    crate::eda::set_mode(world, crate::eda::Mode::Layout);
}

/// Esc with the Select tool, nothing moving and nothing selected (as of the frame before)
/// stops editing.
fn edit_keys(world: &mut World, mut idle: Local<bool>) {
    let keys: Vec<bevy::input::keyboard::KeyboardInput> = world
        .resource_mut::<Messages<bevy::input::keyboard::KeyboardInput>>()
        .iter_current_update_messages()
        .cloned()
        .collect();
    if world.resource::<crate::eda::Eda2d>().plane().is_none() {
        *idle = false;
        return;
    }
    let esc = keys.iter().any(|k| {
        k.state == bevy::input::ButtonState::Pressed && k.shortcut_code() == KeyCode::Escape
    });
    if esc && *idle && crate::eda::ui::keys_for(world, crate::eda::Mode::Layout) {
        end_edit(world);
    }
    let s = world.resource::<crate::eda::layout_tools::LayoutState>();
    *idle = matches!(s.tool, crate::eda::layout_tools::Tool::Select)
        && s.moving.is_none()
        && s.selection.is_empty();
}

/// Where a view ray meets the board face facing the camera: the layout point (nm). `None` when
/// the ray runs along the board.
pub fn ray_to_layout(
    host: &crate::eda::PlaneHost,
    origin: Vec3,
    dir: Vec3,
) -> Option<cadrs_eda::units::Pt> {
    let v = |a: Vec3| [a.x as f64, a.y as f64, a.z as f64];
    cadrs_core::pcb::frames::ray_to_board(&host.frame, host.thickness, v(origin), v(dir))
        .map(cadrs_core::pcb::frames::board_to_layout)
}

/// The component under a view ray, of the edited board copy: its footprint's place on the
/// layout, if the ray meets its model before the board face (a click on a tall part's top
/// picks it).
fn component_under(
    world: &World,
    host: &crate::eda::PlaneHost,
    origin: Vec3,
    dir: Vec3,
) -> Option<(uuid::Uuid, cadrs_eda::units::Pt)> {
    let cache = world.resource::<PartCache>();
    let out = cache.derived.get(&host.target.feature)?;
    let parts = component_parts(out, host.target.copy);
    let layout = &crate::derived_ui::derived_board_ref(world, host.target.feature)?.layout;
    // How far along the ray the board face it sees is (the parts must be nearer).
    let plane_t = ray_to_layout(host, origin, dir).map_or(f32::MAX, |at| {
        let [x, y] = cadrs_core::pcb::frames::layout_to_board(at);
        let z = if dir.dot(Vec3::from_array(host.frame.normal().map(|c| c as f32))) < 0.0 {
            host.thickness
        } else {
            0.0
        };
        let p = cadrs_core::pcb::frames::to_world(&host.frame, [x, y, z]);
        (Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) - origin).dot(dir) / dir.length_squared()
    });
    let mut best: Option<(f32, uuid::Uuid)> = None;
    for pid in parts {
        let Some(part) = cache.part(pid) else {
            continue;
        };
        let Some(fp) = out
            .footprints
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, f)| *f)
        else {
            continue;
        };
        let s = &part.solid;
        let p = |i: u32| {
            let q = s.positions[i as usize];
            Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)
        };
        for t in s.indices.as_chunks::<3>().0 {
            if let Some(d) = ray_triangle(origin, dir, p(t[0]), p(t[1]), p(t[2]))
                && d < plane_t
                && best.is_none_or(|(b, _)| d < b)
            {
                best = Some((d, fp));
            }
        }
    }
    let (_, fp) = best?;
    layout
        .footprints
        .iter()
        .find(|f| f.id == fp)
        .map(|f| (f.id, f.placement.at))
}

/// The pointer input since a press started a board's edit, replayed to [`navigate3d`] once the
/// Layout's tools work on the board (a frame or two later).
#[derive(Default)]
pub(crate) struct Replay {
    inputs: Vec<bevy::picking::pointer::PointerInput>,
    frames: u32,
}

/// A press on a footprint or track of a board shown as its layout starts editing it (see the
/// module docs), the press and what follows it replayed to the Layout's tools.
pub(crate) fn press_to_edit(
    world: &mut World,
    mut cursor: Local<bevy::ecs::message::MessageCursor<bevy::picking::pointer::PointerInput>>,
    mut replay: Local<Replay>,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
    let events: Vec<PointerInput> = cursor
        .read(world.resource::<Messages<PointerInput>>())
        .cloned()
        .collect();
    if !replay.inputs.is_empty() {
        if world.resource::<crate::eda::Eda2d>().plane().is_some() {
            for e in std::mem::take(&mut replay.inputs) {
                world.write_message(e);
            }
            // (A release before now picked the board face in the view: the layout is edited.)
            world.resource_mut::<crate::viewport::Selection>().0.clear();
            // (Read here next frame: the edit is on by then, so they are let be.)
        } else {
            replay.inputs.extend(events);
            replay.frames += 1;
            if replay.frames > 30 {
                *replay = Replay::default();
            }
        }
        return;
    }
    if world.resource::<BoardEditing>().target.is_some()
        || world.resource::<crate::eda::Eda2d>().plane().is_some()
    {
        return;
    }
    // (The press first: the rest builds queries, and most frames have none.)
    let Some(k) = events.iter().position(|e| {
        e.pointer_id == PointerId::Mouse
            && matches!(e.action, PointerAction::Press(PointerButton::Primary))
    }) else {
        return;
    };
    let modeling = world
        .get_resource::<State<crate::sketch::PartStudioMode>>()
        .is_some_and(|m| *m.get() == crate::sketch::PartStudioMode::Modeling);
    let dialogs = world
        .query_filtered::<(), With<cadrs_ui::FeatureDialogState>>()
        .iter(world)
        .next()
        .is_some();
    if !modeling || dialogs {
        return;
    }
    let over = {
        let areas: Vec<Entity> = world
            .query_filtered::<Entity, With<crate::viewport::ViewportArea>>()
            .iter(world)
            .collect();
        world
            .resource::<bevy::picking::hover::HoverMap>()
            .get(&PointerId::Mouse)
            .is_some_and(|hits| areas.iter().any(|e| hits.contains_key(e)))
    };
    if !over {
        return;
    }
    let Some((feature, copy)) = layout_item_under(world, events[k].location.position) else {
        return;
    };
    begin_edit(world, feature, copy);
    if world.resource::<BoardEditing>().target.is_some() {
        *replay = Replay {
            inputs: events[k..].to_vec(),
            frames: 0,
        };
    }
}

/// The derived board shown as its layout (feature, copy) whose footprint or track is under the
/// screen point, its board face the nearest face there of a part that isn't see-through.
fn layout_item_under(world: &World, pos: Vec2) -> Option<(FeatureId, usize)> {
    let doc = world.resource::<ActiveDocument>();
    let el = doc.active_element()?;
    if el.assembly_model().is_some() {
        return None;
    }
    let view = &world.resource::<crate::viewport::ViewportView>().view;
    let offset = world
        .resource::<crate::viewport::ViewportRect>()
        .offset(pos);
    let (origin, dir) = view.ray(offset);
    let cache = world.resource::<PartCache>();
    // (See-through parts let the press through, as they let the layout be seen.)
    let nearest = crate::parts::pick_opaque_face(cache, view, offset, None).map(|(_, _, t)| t);
    for b in boards_in(doc)
        .into_iter()
        .filter(|b| b.view == BoardView::Layout)
    {
        let Some(layout) = layout_of(doc, b.feature) else {
            continue;
        };
        let Some(out) = cache.derived.get(&b.feature) else {
            continue;
        };
        for (copy, c) in out.boards.iter().enumerate() {
            let host = crate::eda::PlaneHost {
                target: BoardTarget {
                    studio: el.id,
                    feature: b.feature,
                    copy,
                },
                frame: c.frame,
                thickness: c.thickness,
            };
            let Some(at) = ray_to_layout(&host, origin, dir) else {
                continue;
            };
            // The face seen: how far along the ray, and which side's footprints come first.
            let n = Vec3::from_array(host.frame.normal().map(|v| v as f32));
            let up = dir.dot(n) < 0.0;
            let [x, y] = cadrs_core::pcb::frames::layout_to_board(at);
            let p = cadrs_core::pcb::frames::to_world(
                &host.frame,
                [x, y, if up { host.thickness } else { 0.0 }],
            );
            let t = (Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) - origin).dot(dir)
                / dir.length_squared();
            if nearest.is_some_and(|f| f < t - 0.05) {
                continue;
            }
            // Pick tolerance: 5 px at the board.
            let px_per_mm = (view.magnification(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
                / view.scale.max(1e-6)) as f64;
            let tol = (5.0 / px_per_mm.max(1e-6) * 1e6) as cadrs_eda::units::Nm;
            let side = if up {
                cadrs_eda::layer::Side::Top
            } else {
                cadrs_eda::layer::Side::Bottom
            };
            let hit = cadrs_eda::board_edit::hit_from(layout, at, tol, Some(side));
            if matches!(
                hit,
                Some(
                    cadrs_eda::board_edit::BoardItem::Footprint(_)
                        | cadrs_eda::board_edit::BoardItem::Track(_)
                        | cadrs_eda::board_edit::BoardItem::Via(_)
                )
            ) {
                return Some((b.feature, copy));
            }
        }
    }
    None
}

/// Möller–Trumbore: the distance along the ray to the triangle, if it meets it.
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// The Layout's pointer on a board in the 3D view: viewport presses, releases and moves become
/// [`crate::eda::EdaClick`]s and [`crate::eda::EdaPointer`] at the layout point under the
/// pointer (on the board face facing the camera, or a component's place when its model is
/// under the pointer), and the pick tolerance follows the zoom at the board.
#[allow(clippy::too_many_arguments)]
pub(crate) fn navigate3d(
    eda: Res<crate::eda::Eda2d>,
    rect: Res<crate::viewport::ViewportRect>,
    view: Res<crate::viewport::ViewportView>,
    hover: Res<bevy::picking::hover::HoverMap>,
    q_area: Query<Entity, With<crate::viewport::ViewportArea>>,
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut pointer: ResMut<crate::eda::EdaPointer>,
    mut ui: ResMut<crate::eda::EdaUi>,
    mut last_press: Local<Option<(f64, Vec2)>>,
    fit: Res<crate::board_fit::FitState>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton};
    let fit_align_active = fit.align != crate::board_fit::AlignStep::Off;
    let events: Vec<_> = inputs.read().cloned().collect();
    let (Some(host), Some(key)) = (eda.plane().copied(), eda.0) else {
        return;
    };
    // Pixels per mm at the board (for the tools' pick tolerance).
    let o = Vec3::new(
        host.frame.origin[0] as f32,
        host.frame.origin[1] as f32,
        host.frame.origin[2] as f32,
    );
    let scale = (view.view.magnification(o) / view.view.scale.max(1e-6)) as f64;
    let size = [rect.0.width() as f64, rect.0.height() as f64];
    ui.views.insert(
        key,
        (
            cadrs_eda::view::View {
                center: [0.0, 0.0],
                scale,
                size,
            },
            false,
        ),
    );
    let over = crate::viewport::pointer_over_viewport(&hover, &q_area);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for input in events {
        let pos = input.location.position;
        let (origin, dir) = view.view.ray(rect.offset(pos));
        // Off the board's plane (seen edge on, or beyond the horizon): a release still ends a
        // drag, where the pointer last met the plane; anything else waits.
        let at = match ray_to_layout(&host, origin, dir) {
            Some(at) => at,
            None if matches!(input.action, PointerAction::Release(PointerButton::Primary)) => {
                pointer.at
            }
            None => continue,
        };
        pointer.screen = pos;
        pointer.at = at;
        pointer.over = over;
        match input.action {
            PointerAction::Press(PointerButton::Primary) if over => {
                // Align waits for a pick (`crate::board_fit`): the press is its.
                let offset = rect.offset(pos);
                if fit_align_active {
                    commands.queue(move |world: &mut World| {
                        crate::board_fit::align_press(world, offset);
                    });
                    continue;
                }
                let now = time.elapsed_secs_f64();
                let double =
                    last_press.is_some_and(|(t, p)| now - t < 0.4 && p.distance(pos) < 4.0);
                *last_press = Some((now, pos));
                commands.queue(move |world: &mut World| {
                    // The side of the board seen: its footprints are picked first.
                    let n = host.frame.normal();
                    let up = dir.dot(Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32)) < 0.0;
                    world
                        .resource_mut::<crate::eda::layout_tools::LayoutState>()
                        .facing = Some(if up {
                        cadrs_eda::layer::Side::Top
                    } else {
                        cadrs_eda::layer::Side::Bottom
                    });
                    // A component's model under the pointer: the press picks its footprint (to
                    // select or move it, from where the pointer meets the board, so it keeps its
                    // place under the pointer); a drawing tool's points stay where they are.
                    let selecting = matches!(
                        world
                            .resource::<crate::eda::layout_tools::LayoutState>()
                            .tool,
                        crate::eda::layout_tools::Tool::Select
                    );
                    let under = if selecting {
                        component_under(world, &host, origin, dir)
                    } else {
                        None
                    };
                    world
                        .resource_mut::<crate::eda::layout_tools::LayoutState>()
                        .pick = under.map(|(id, _)| id);
                    world.write_message(crate::eda::EdaClick::Press { at, shift, ctrl });
                    if double {
                        // (Its properties: at its origin, where the layout finds it.)
                        world.write_message(crate::eda::EdaClick::Double {
                            at: under.map_or(at, |(_, o)| o),
                            shift,
                        });
                    }
                });
                if double {
                    *last_press = None;
                }
            }
            PointerAction::Release(PointerButton::Primary) if !fit_align_active => {
                commands.queue(move |world: &mut World| {
                    world.write_message(crate::eda::EdaClick::Release { at });
                });
            }
            _ => {}
        }
    }
}

/// A component moving with a drag (§7.2): its part drawn where the Layout's preview has its
/// footprint, the part itself hidden until the rebuild after the drop has it there.
#[derive(Component)]
struct DragGhost {
    part: PartId,
}

/// What the ghosts were made for: per part, its solid (to tell the rebuilt one) and footprint.
#[derive(Default)]
struct Ghosts {
    parts: HashMap<PartId, (usize, uuid::Uuid)>,
    /// The derived layout's hash while dragging: a drop that changed nothing (refused) leaves
    /// it as it was, and the ghosts go at once rather than waiting for a rebuild.
    dragged_from: Option<u64>,
}

/// The motion of a footprint frame onto the Part Studio for a board copy (`frame`, `thickness`).
fn motion_transform(m: &cadrs_kernel::Motion) -> Transform {
    let l = m.linear;
    let mat = Mat4::from_cols(
        Vec4::new(l[(0, 0)] as f32, l[(1, 0)] as f32, l[(2, 0)] as f32, 0.0),
        Vec4::new(l[(0, 1)] as f32, l[(1, 1)] as f32, l[(2, 1)] as f32, 0.0),
        Vec4::new(l[(0, 2)] as f32, l[(1, 2)] as f32, l[(2, 2)] as f32, 0.0),
        Vec4::new(
            m.translation.x as f32,
            m.translation.y as f32,
            m.translation.z as f32,
            1.0,
        ),
    );
    Transform::from_matrix(mat)
}

/// Keeps a ghost on each component whose footprint the Layout's preview moves, turns or flips
/// (live 3D while dragging); the ghost stays after the drop until the rebuilt part is there.
fn sync_drag_ghosts(
    world: &mut World,
    mut ghosts: Local<Ghosts>,
    q: &mut QueryState<(Entity, &'static DragGhost, &'static mut Transform)>,
) {
    let host = world.resource::<crate::eda::Eda2d>().plane().copied();
    let preview = world.resource::<crate::eda::Preview>().get();
    // Each moved footprint's part and motion (preview placement ∘ stored placement⁻¹).
    let mut moved: Vec<(PartId, uuid::Uuid, Transform)> = vec![];
    if let (Some(h), Some(preview)) = (host, preview.map(|d| &d.board.footprints))
        && let Some(layout) =
            crate::derived_ui::derived_board_ref(world, h.target.feature).map(|b| &b.layout)
    {
        let cache = world.resource::<PartCache>();
        if let Some(out) = cache.derived.get(&h.target.feature) {
            let parts = component_parts(out, h.target.copy);
            for (pid, fp) in out.footprints.iter().filter(|(p, _)| parts.contains(p)) {
                let (Some(old), Some(new)) = (
                    layout.footprints.iter().find(|f| f.id == *fp),
                    preview.iter().find(|f| f.id == *fp),
                ) else {
                    continue;
                };
                if old.placement == new.placement {
                    continue;
                }
                let m =
                    cadrs_core::pcb::frames::footprint_world(&h.frame, h.thickness, &old.placement)
                        .inverse()
                        .then(&cadrs_core::pcb::frames::footprint_world(
                            &h.frame,
                            h.thickness,
                            &new.placement,
                        ));
                moved.push((*pid, *fp, motion_transform(&m)));
            }
        }
    }
    // Keep a ghost whose part isn't rebuilt yet (the drop is being rebuilt), unless the drop
    // didn't change the layout.
    let hash_now = host
        .and_then(|h| crate::derived_ui::derived_board_ref(world, h.target.feature))
        .map(|b| b.hash);
    if !moved.is_empty() {
        ghosts.dragged_from = hash_now;
    }
    let changed = ghosts.dragged_from.is_some() && hash_now != ghosts.dragged_from;
    let cache = world.resource::<PartCache>();
    let pending: Vec<PartId> = ghosts
        .parts
        .iter()
        .filter(|(pid, (ptr, _))| {
            changed
                && !moved.iter().any(|m| m.0 == **pid)
                && cache
                    .part(**pid)
                    .is_some_and(|p| std::sync::Arc::as_ptr(&p.solid) as usize == *ptr)
                && host.is_some()
        })
        .map(|(p, _)| *p)
        .collect();
    // The parts hidden for their ghosts.
    let hidden: HashSet<PartId> = moved
        .iter()
        .map(|m| m.0)
        .chain(pending.iter().copied())
        .collect();
    let mut have: HashSet<PartId> = HashSet::new();
    let mut gone = vec![];
    for (e, g, mut tr) in q.iter_mut(world) {
        if let Some((_, _, t)) = moved.iter().find(|m| m.0 == g.part) {
            *tr = *t;
            have.insert(g.part);
        } else if pending.contains(&g.part) {
            have.insert(g.part);
        } else {
            gone.push((e, g.part));
        }
    }
    for (e, p) in gone {
        world.entity_mut(e).despawn();
        ghosts.parts.remove(&p);
    }
    for (pid, fp, t) in &moved {
        if have.contains(pid) {
            continue;
        }
        let Some(part) = world.resource::<PartCache>().part(*pid).cloned() else {
            continue;
        };
        ghosts
            .parts
            .insert(*pid, (std::sync::Arc::as_ptr(&part.solid) as usize, *fp));
        // Drawn as the part is (its faces' colours, the parts' shading).
        let bases = world.resource::<PartCache>().bases(&part);
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(crate::parts::part_base_mesh(&part, false, &bases));
        let material = world
            .resource_mut::<Assets<crate::part_shading::PartShading>>()
            .add(crate::part_shading::PartShading::new(false, false));
        world.spawn((
            Name::new(format!("board-drag-{}", crate::pcb::slug(&part.name))),
            DragGhost { part: *pid },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            *t,
            DespawnOnExit(AppState::Document),
        ));
    }
    let mut cache = world.resource_mut::<PartCache>();
    if cache.drag_hidden != hidden {
        cache.drag_hidden = hidden;
        cache.generation += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// Board outlines from sketches (§8)

/// The regions of the sketch `sketch` a board is made from: the ones selected in the view, else
/// the largest (with its holes).
fn sketch_regions(world: &World, sketch: FeatureId) -> Vec<cadrs_core::document::RegionRef> {
    let doc = world.resource::<ActiveDocument>();
    let Some(g) = doc
        .active_element()
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
    else {
        return vec![];
    };
    let all = cadrs_sketch::region::regions_shared(&g);
    let picked: Vec<usize> = world
        .resource::<crate::viewport::Selection>()
        .0
        .iter()
        .filter_map(|p| match p {
            crate::viewport::Pick::Region(f, i) if *f == sketch => Some(*i as usize),
            _ => None,
        })
        .collect();
    let chosen: Vec<&cadrs_sketch::region::Region> = if picked.is_empty() {
        all.iter()
            .max_by(|a, b| a.area().total_cmp(&b.area()))
            .into_iter()
            .collect()
    } else {
        picked.iter().filter_map(|i| all.get(*i)).collect()
    };
    chosen
        .into_iter()
        .map(|r| cadrs_core::document::RegionRef::new(sketch, r))
        .collect()
}

/// The largest region of a sketch (with its holes), as a board outline's.
pub fn largest_region(
    g: &cadrs_sketch::Sketch,
    sketch: FeatureId,
) -> Vec<cadrs_core::document::RegionRef> {
    let all = cadrs_sketch::region::regions_shared(g);
    all.iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .map(|r| cadrs_core::document::RegionRef::new(sketch, r))
        .into_iter()
        .collect()
}

/// The sketch row's Create PCB board…: the dialog for a board with the sketch's outline (the
/// regions picked in the view, else the largest), derived onto the sketch (one undo step,
/// [`crate::eda::lay_dialogs::open_create_board`]).
pub fn create_from_sketch(world: &mut World, sketch: FeatureId) {
    let regions = sketch_regions(world, sketch);
    if regions.is_empty() {
        crate::linked::error_toast(
            world,
            "The sketch has no closed region to make a board from",
        );
        return;
    }
    let Some(element) = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id)
    else {
        return;
    };
    crate::eda::lay_dialogs::open_create_board(world, element, sketch, regions);
}

/// The derived board `feature`'s source: its PCB Studio and board.
fn board_of(world: &World, feature: FeatureId) -> Option<(ElementId, cadrs_core::pcb::BoardId)> {
    let d = crate::derived_ui::derived_ref(world, feature)?;
    Some((d.source?.element, d.board.as_ref()?.board))
}

/// Break outline link: the outline stays as it is, editable in the layout (one undo step).
pub fn unlink_outline(world: &mut World, feature: FeatureId) {
    let Some((pcb, board)) = board_of(world, feature) else {
        return;
    };
    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(
        &cadrs_core::pcb::outline_link::SetOutlineLink {
            pcb,
            board,
            link: None,
            keep_parts: false,
        },
    ) {
        crate::linked::error_toast(world, e.to_string());
    }
}

/// Link outline to a sketch: a menu of the sketches before the feature with a closed region.
pub fn link_outline_menu(world: &mut World, feature: FeatureId) {
    let doc = world.resource::<ActiveDocument>();
    let Some(el) = doc.active_element() else {
        return;
    };
    let before: Vec<(FeatureId, String)> = el
        .features()
        .iter()
        .take_while(|f| f.id != feature)
        .filter(|f| {
            f.sketch()
                .is_some_and(|s| !cadrs_sketch::region::regions_shared(&s.geometry).is_empty())
        })
        .map(|f| (f.id, f.name.clone()))
        .collect();
    if before.is_empty() {
        crate::linked::error_toast(
            world,
            "No sketch with a closed region comes before the board",
        );
        return;
    }
    let pos = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .next()
        .and_then(|w| w.cursor_position())
        .unwrap_or(Vec2::new(400.0, 300.0));
    let theme = world.resource::<cadrs_ui::Theme>().clone();
    let mut menu = cadrs_ui::Menu::new("board-link-menu")
        .min_width(180.0)
        .item_height(22.0);
    for (id, name) in &before {
        menu = menu.item(
            cadrs_ui::MenuItem::new(format!("board-link-{}", id.0), name.clone()).icon("sketch"),
        );
    }
    let pending = LinkMenu {
        feature,
        sketches: before.iter().map(|(id, _)| *id).collect(),
    };
    world.insert_resource(pending);
    let mut commands = world.commands();
    cadrs_ui::menu::open_context_menu(&mut commands, pos, menu.build(&theme));
    world.flush();
}

/// The sketches Link outline to a sketch offered, for its menu's pick.
#[derive(Resource, Debug, Clone)]
struct LinkMenu {
    feature: FeatureId,
    sketches: Vec<FeatureId>,
}

fn on_link_menu(ev: On<cadrs_ui::MenuAction>, mut commands: Commands) {
    let Some(rest) = ev.item.strip_prefix("board-link-") else {
        return;
    };
    let rest = rest.to_string();
    commands.queue(move |world: &mut World| {
        let Some(m) = world.get_resource::<LinkMenu>().cloned() else {
            return;
        };
        let Some(sketch) = m.sketches.iter().copied().find(|s| s.0.to_string() == rest) else {
            return;
        };
        let Some((pcb, board)) = board_of(world, m.feature) else {
            return;
        };
        let Some(element) = world
            .resource::<ActiveDocument>()
            .active_element()
            .map(|e| e.id)
        else {
            return;
        };
        let regions = sketch_regions(world, sketch);
        let cmd = cadrs_core::pcb::outline_link::SetOutlineLink {
            pcb,
            board,
            link: Some((element, sketch, regions)),
            keep_parts: true,
        };
        if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&cmd) {
            crate::linked::error_toast(world, e.to_string());
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Scenario set-ups

/// The Derived feature `derive` makes.
pub const DERIVED: FeatureId = FeatureId::from_u128(0x7e00_0000_0000_0000_0000_0000_0000_0e01);
/// The plane and the sketch on it `outline-sketch` makes.
pub const OUTLINE_PLANE: FeatureId =
    FeatureId::from_u128(0x7e00_0000_0000_0000_0000_0000_0000_0e03);
pub const OUTLINE_SKETCH: FeatureId =
    FeatureId::from_u128(0x7e00_0000_0000_0000_0000_0000_0000_0e02);

/// `setup`: the mains-sensor enclosure, stored and open on its Part Studio. `derive`: its
/// board derived onto Board location. `layout` / `3d`: that board's view. `transparent a, b`:
/// the parts named so see-through. `outline-sketch`: "Board outline", a sketch on Top to make a
/// board from; `outline-narrow`: its right side moved in.
pub fn script(world: &mut World, arg: &str) {
    use cadrs_core::samples::pcb_enclosure as pe;
    match arg.trim() {
        "setup" => {
            let Ok(doc) = pe::document() else { return };
            let store = world.resource::<crate::DocumentStore>().0.clone();
            let now = world.resource::<crate::AppClock>().now();
            let user = world.resource::<crate::UserProfile>().id.clone();
            let meta = cadrs_core::DocumentMeta::new(&user, now - 600);
            if let Err(e) = store.create(&doc, &meta) {
                warn!("board setup: {e}");
            }
            let mut d = ActiveDocument::stored(doc, meta);
            d.set_active(pe::STUDIO);
            world.insert_resource(d);
        }
        "derive" => {
            let doc = world.resource::<ActiveDocument>().doc.clone();
            let Some(board) = doc
                .element(pe::PCB)
                .and_then(|e| e.pcb())
                .and_then(|s| s.boards.first())
                .map(|b| b.id)
            else {
                return;
            };
            let d = DerivedFeature {
                locations: vec![ConnectorRef::Feature(pe::BOARD_LOCATION)],
                board: Some(Box::new(DerivedBoard::new(board, "", Default::default()))),
                ..Default::default()
            };
            let r = SourceRef {
                document: None,
                at: RefAt::Workspace,
                element: pe::PCB,
                pinned: false,
            };
            let got = {
                let mut res = crate::linked::resolver(world);
                cadrs_core::derived::resolve(&mut res.0, &doc, None, d, r)
            };
            match got {
                Ok(g) => {
                    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&AddDerived {
                        element: pe::STUDIO,
                        feature: DERIVED,
                        derived: g.derived,
                        links: g.links,
                    }) {
                        warn!("board derive: {e}");
                    }
                }
                Err(e) => warn!("board derive: {e}"),
            }
        }
        // `transparent Tube, End plate`: those parts see-through (Make transparent).
        a if a.starts_with("transparent ") => {
            let names: Vec<String> = a["transparent ".len()..]
                .split(',')
                .map(|n| n.trim().to_string())
                .collect();
            let mut cache = world.resource_mut::<PartCache>();
            let ids: Vec<PartId> = cache
                .parts
                .iter()
                .filter(|p| {
                    names
                        .iter()
                        .any(|n| *n == cadrs_core::parts::display_name(p, &cache.props))
                })
                .map(|p| p.id)
                .collect();
            cache.transparent.extend(ids);
            cache.generation += 1;
        }
        // A sketch on Top inside the tube: a 38 × 38 square from (−19, −19) with a ⌀3.2 mounting
        // hole at (−15, 15).
        "outline-sketch" => {
            use cadrs_sketch::{Link, SketchOp, Vec2};
            let mut doc = world.resource_mut::<ActiveDocument>();
            // On "Board plane", 4 mm up from Top inside the tube.
            let plane = cadrs_core::plane::PlaneFeature {
                kind: cadrs_core::plane::PlaneType::Offset,
                entities: vec![cadrs_core::plane::PlaneEntity::Plane(
                    cadrs_sketch::PlaneRef::Top,
                )],
                offset: 4.0,
                offset_expr: "4 mm".into(),
                ..Default::default()
            };
            let _ = doc.execute(&cadrs_core::commands::AddFeature {
                element: pe::STUDIO,
                feature: OUTLINE_PLANE,
                base_name: "Board plane".into(),
                kind: FeatureKind::Plane(plane),
            });
            let features = doc
                .active_element()
                .map(|e| e.features().to_vec())
                .unwrap_or_default();
            let Some(on) = cadrs_core::parts::plane_feature_ref(&features, OUTLINE_PLANE) else {
                return;
            };
            let _ = doc.execute(&cadrs_core::commands::AddSketch {
                element: pe::STUDIO,
                feature: OUTLINE_SKETCH,
                plane: Some(on),
            });
            let sketch = |doc: &ActiveDocument| {
                doc.doc
                    .element(pe::STUDIO)
                    .and_then(|e| e.feature(OUTLINE_SKETCH))
                    .and_then(|f| f.sketch())
                    .cloned()
            };
            let Some(frame) = sketch(&doc).and_then(|s| s.plane).map(|p| p.frame()) else {
                return;
            };
            // Use: the tube's four inner edges (on Top, seen on the board plane).
            let build = cadrs_core::rebuild::build(&features);
            let Some(tube) = build
                .parts
                .iter()
                .find(|p| p.feature == pe::TUBE)
                .map(|p| p.solid.clone())
            else {
                return;
            };
            let ctx = cadrs_core::links::LinkContext {
                solids: vec![(pe::TUBE, &*tube)],
                features: &features,
            };
            let items: Vec<_> = tube
                .edges
                .iter()
                .filter(|e| {
                    e.points.iter().all(|p| {
                        p[2].abs() < 1e-6 && (p[0].abs().max(p[1].abs()) - 20.0).abs() < 1e-6
                    })
                })
                .filter_map(|e| {
                    let link = Link::Edge {
                        feature: pe::TUBE.0,
                        edge: e.name,
                    };
                    Some((ctx.shape(link, &frame)?, link))
                })
                .collect();
            let _ = doc.execute(&cadrs_core::commands::EditSketch {
                element: pe::STUDIO,
                feature: OUTLINE_SKETCH,
                op: SketchOp::Use { items },
            });
            // Offset 1 mm in from them (the side toward the middle), and a ⌀3.2 mounting hole.
            let Some(g) = sketch(&doc).map(|s| s.geometry) else {
                return;
            };
            let Some(first) = g.curves.keys().next() else {
                return;
            };
            let (chain, _) = cadrs_sketch::edit::chain_of(&g, first);
            let inward = |left: bool| {
                cadrs_sketch::edit::offset_geometry(&g, &chain, 1.0, left)
                    .ok()
                    .and_then(|o| o.first().copied())
                    .is_some_and(|o| matches!(o, cadrs_sketch::edit::OffsetShape::Line(a, b) if a.midpoint(b).length() < 19.5))
            };
            let left = inward(true);
            for op in [
                SketchOp::Offset {
                    chain,
                    distance: 1.0,
                    left,
                    label: (2.0, 0.0),
                },
                SketchOp::AddCircle {
                    center: Vec2::new(-15.0, 15.0),
                    radius: 1.6,
                    construction: false,
                },
            ] {
                let _ = doc.execute(&cadrs_core::commands::EditSketch {
                    element: pe::STUDIO,
                    feature: OUTLINE_SKETCH,
                    op,
                });
            }
            let _ = doc.execute(&cadrs_core::commands::RenameFeature {
                element: pe::STUDIO,
                feature: OUTLINE_SKETCH,
                name: "Board outline".into(),
            });
        }
        // The tube's inside narrowed: its right wall in to x = 10 (the board, 1 mm in from it,
        // follows).
        "outline-narrow" => {
            use cadrs_sketch::{SketchOp, Vec2};
            let mut doc = world.resource_mut::<ActiveDocument>();
            let Some(g) = doc
                .doc
                .element(pe::STUDIO)
                .and_then(|e| e.feature(pe::TUBE_SKETCH))
                .and_then(|f| f.sketch())
                .map(|s| s.geometry.clone())
            else {
                return;
            };
            let corner = |x: f64, y: f64| g.point_at(Vec2::new(x, y), 1e-6);
            let (Some(a), Some(b)) = (corner(20.0, -20.0), corner(20.0, 20.0)) else {
                return;
            };
            let _ = doc.execute(&cadrs_core::commands::EditSketch {
                element: pe::STUDIO,
                feature: pe::TUBE_SKETCH,
                op: SketchOp::MovePoints {
                    moves: vec![(a, Vec2::new(10.0, -20.0)), (b, Vec2::new(10.0, 20.0))],
                },
            });
        }
        // Where a footprint of the derived board is, as text a scenario can check
        // (`ExpectText("board-check", "J4 at 15.50, 27.50 top 0°")`); `count`: its tracks,
        // vias and zones; `size`: the first derived board part's size in its Part Studio.
        a if a.starts_with("check ") || a == "count" || a == "size" => {
            let r = a.strip_prefix("check ").unwrap_or("").trim().to_string();
            let board = crate::derived_ui::derived_board_ref(world, DERIVED).or_else(|| {
                let doc = world.resource::<ActiveDocument>();
                // (An assembly: its first board instance.)
                if let Some(b) = boards_in(doc).first() {
                    return instance_board(doc, b.feature)?.2.board.as_deref();
                }
                doc.active_element()?
                    .features()
                    .iter()
                    .find_map(|f| match &f.kind {
                        FeatureKind::Derived(d) => d.board.as_deref(),
                        _ => None,
                    })
            });
            let text = if a == "count" {
                board.map_or_else(String::new, |b| {
                    let l = &b.layout;
                    let area: f64 = l
                        .zones
                        .iter()
                        .flat_map(|z| z.filled.iter())
                        .flat_map(|(_, ps)| ps.iter())
                        .map(|p| {
                            let a = |r: &Vec<cadrs_eda::units::Pt>| {
                                cadrs_eda::poly::ring_area(r).abs() / 1e12
                            };
                            a(&p.outer) - p.holes.iter().map(a).sum::<f64>()
                        })
                        .sum();
                    format!(
                        "tracks {}, vias {}, zones {} (filled {}, {area:.0} mm²)",
                        l.tracks.len(),
                        l.vias.len(),
                        l.zones.len(),
                        l.zones.iter().filter(|z| !z.filled.is_empty()).count()
                    )
                })
            } else if a == "size" {
                let cache = world.resource::<PartCache>();
                cache
                    .derived
                    .values()
                    .flat_map(|o| o.boards.first())
                    .next()
                    .and_then(|c| cache.part(c.part).map(|p| (c.frame, p.solid.clone())))
                    .and_then(|(frame, solid)| cadrs_core::pcb::fit::bounds_in(&frame, &solid))
                    .map_or_else(String::new, |(lo, hi)| {
                        format!(
                            "board {:.2} × {:.2} × {:.2}",
                            hi[0] - lo[0],
                            hi[1] - lo[1],
                            hi[2] - lo[2]
                        )
                    })
            } else {
                board
                    .and_then(|b| b.layout.footprints.iter().find(|f| f.reference() == r))
                    .map_or_else(
                        || format!("{r}: no such footprint"),
                        |f| {
                            let [x, y] = cadrs_core::pcb::frames::layout_to_board(f.placement.at);
                            let side = if f.placement.side == cadrs_eda::layer::Side::Top {
                                "top"
                            } else {
                                "bottom"
                            };
                            format!("{r} at {x:.2}, {y:.2} {side} {}°", f.placement.angle)
                        },
                    )
            };
            let mut q = world.query::<(Entity, &Name)>();
            let existing = q
                .iter(world)
                .find(|(_, n)| n.as_str() == "board-check")
                .map(|(e, _)| e);
            match existing {
                Some(e) => {
                    if let Some(mut t) = world.get_mut::<Text>(e) {
                        t.0 = text;
                    }
                }
                None => {
                    world.spawn((
                        Name::new("board-check"),
                        Text::new(text),
                        Node {
                            display: Display::None,
                            ..default()
                        },
                        DespawnOnExit(AppState::Document),
                    ));
                }
            }
        }
        "edit" => begin_edit(world, DERIVED, 0),
        "done" => end_edit(world),
        "layout" => set_view(world, DERIVED, BoardView::Layout),
        "3d" => set_view(world, DERIVED, BoardView::ThreeD),
        other => warn!("board: unknown set-up {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::samples::pcb_enclosure as pe;

    #[test]
    fn the_overlay_lies_on_the_top_face_in_the_board_frame() {
        let lib = cadrs_eda::library::LibraryTable::builtin();
        let layout = cadrs_eda::ac_board::board(&lib);
        // A board frame standing up: x along world y, y along world z, the normal along +x.
        let frame = PlaneFrame {
            origin: [10.0, 0.0, 0.0],
            u: [0.0, 1.0, 0.0],
            v: [0.0, 0.0, 1.0],
        };
        let (m, image) = overlay_mesh(
            &layout,
            &OverlayView::default(),
            &frame,
            1.6,
            cadrs_eda::layer::Side::Top,
            OverlayArea::Board,
        )
        .unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        assert!(
            p.iter()
                .all(|q| (q[0] - (10.0 + 1.6 + LIFT as f32)).abs() < 1e-4),
            "on the top face"
        );
        // The board's box: 31 × 39 mm, from its corner at the layout's origin.
        let (ymin, ymax) = p
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), q| (a.min(q[1]), b.max(q[1])));
        assert!(
            ymin.abs() < 1e-4 && (ymax - cadrs_eda::ac_board::WIDTH as f32).abs() < 1e-4,
            "{ymin} {ymax}"
        );
        assert_eq!(
            image.width(),
            (cadrs_eda::ac_board::WIDTH as f32 * OVERLAY_PX_PER_MM).ceil() as u32
        );
        // The bottom face's picture lies under the board.
        let (m, _) = overlay_mesh(
            &layout,
            &OverlayView::default(),
            &frame,
            1.6,
            cadrs_eda::layer::Side::Bottom,
            OverlayArea::Board,
        )
        .unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        assert!(p.iter().all(|q| (q[0] - (10.0 - LIFT as f32)).abs() < 1e-4));
    }

    /// The mains sensor with its board derived onto Board location, open on the Enclosure, and
    /// the Parts cache as the view has it; the edit host of that board copy.
    fn derived_world() -> (World, crate::eda::PlaneHost) {
        let mut doc = pe::document().unwrap();
        let board = doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id;
        let d = DerivedFeature {
            locations: vec![ConnectorRef::Feature(pe::BOARD_LOCATION)],
            board: Some(Box::new(DerivedBoard::new(board, "", Default::default()))),
            ..Default::default()
        };
        let mut res = cadrs_core::external::Resolver::new(cadrs_core::Store::new(
            std::env::temp_dir().join("cadrs-board-in-studio-tests"),
        ));
        let r = SourceRef {
            document: None,
            at: RefAt::Workspace,
            element: pe::PCB,
            pinned: false,
        };
        let got = cadrs_core::derived::resolve(&mut res, &doc, None, d, r).unwrap();
        let mut h = cadrs_core::History::default();
        h.execute(
            &mut doc,
            &AddDerived {
                element: pe::STUDIO,
                feature: DERIVED,
                derived: got.derived,
                links: got.links,
            },
        )
        .unwrap();
        let build = cadrs_core::rebuild::build(&doc.element(pe::STUDIO).unwrap().part_features());
        let mut w = World::new();
        let mut ad = ActiveDocument::new(doc);
        ad.set_active(pe::STUDIO);
        w.insert_resource(ad);
        let mut cache = PartCache::default();
        cache.parts = build.parts.clone();
        cache.derived = build.derived.clone();
        let copy = cache.derived[&DERIVED].boards[0].clone();
        w.insert_resource(cache);
        (
            w,
            crate::eda::PlaneHost {
                target: BoardTarget {
                    studio: pe::STUDIO,
                    feature: DERIVED,
                    copy: 0,
                },
                frame: copy.frame,
                thickness: copy.thickness,
            },
        )
    }

    #[test]
    fn a_press_picks_the_component_whose_model_it_meets_first() {
        let (w, host) = derived_world();
        let layout = crate::derived_ui::derived_board_ref(&w, DERIVED)
            .unwrap()
            .layout
            .clone();
        let c1 = layout
            .footprints
            .iter()
            .find(|f| f.reference() == "C1")
            .unwrap();
        let [x, y] = cadrs_core::pcb::frames::layout_to_board(c1.placement.at);
        let over = cadrs_core::pcb::frames::to_world(&host.frame, [x, y, 0.0]);
        let at = Vec3::new(over[0] as f32, over[1] as f32, 0.0);
        // From above, straight down onto C1 (on the top face): C1, at its origin.
        let down = component_under(&w, &host, at + Vec3::Z * 100.0, -Vec3::Z);
        assert_eq!(down, Some((c1.id, c1.placement.at)));
        // From below at the same place: the board's bottom face comes before C1; only a
        // component on the bottom could be picked.
        let up = component_under(&w, &host, at - Vec3::Z * 100.0, Vec3::Z);
        let side = |id: uuid::Uuid| {
            layout
                .footprints
                .iter()
                .find(|f| f.id == id)
                .map(|f| f.placement.side)
        };
        assert!(
            up.is_none_or(|(id, _)| side(id) == Some(cadrs_eda::layer::Side::Bottom)),
            "{up:?}"
        );
        // Beside the board: nothing.
        assert_eq!(
            component_under(&w, &host, at + Vec3::new(200.0, 0.0, 100.0), -Vec3::Z),
            None
        );
    }

    #[test]
    fn an_assembly_hosts_its_board_instance_where_the_instance_is() {
        use cadrs_core::assembly::{Instance, InstanceId, Pose};
        let mut doc = pe::document().unwrap();
        let board = doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id;
        let mut asm = cadrs_core::Element::assembly("Assembly 1");
        let asm_id = ElementId::new();
        asm.id = asm_id;
        let parts = doc
            .element(pe::PCB)
            .unwrap()
            .pcb()
            .unwrap()
            .board_parts(board);
        let i = InstanceId::new();
        let pose = Pose {
            translation: [100.0, 0.0, 50.0],
            ..Pose::IDENTITY
        };
        asm.assembly_model_mut()
            .unwrap()
            .instances
            .push(Instance::board(i, pe::PCB, board, parts, pose));
        doc.elements.push(asm);
        let mut ad = ActiveDocument::new(doc);
        ad.set_active(asm_id);
        let el = ad.doc.element(asm_id).unwrap().clone();
        let mut cache = PartCache::default();
        let mut parts = crate::assembly::AssemblyParts::default();
        crate::assembly::update_assembly_parts(&ad, &el, &mut cache, &mut parts, None);
        // The instance as a derived board: its frame where the instance put the layout's origin,
        // its components the instance's parts, each with its footprint.
        let f = FeatureId(i.0);
        let out = cache.derived.get(&f).expect("the board instance's output");
        assert_eq!(out.boards[0].frame.origin, [100.0, 0.0, 50.0]);
        assert!(
            !out.footprints.is_empty()
                && out
                    .footprints
                    .iter()
                    .all(|(p, _)| cache.parts.iter().any(|x| x.id == *p))
        );
        // Edited in context: the host on that frame, the layout the PCB Studio's own.
        let editing = BoardEditing {
            target: Some(BoardTarget {
                studio: asm_id,
                feature: f,
                copy: 0,
            }),
        };
        let (pcb, b, host) = edited_board(&ad, &editing, &cache).expect("editable in context");
        assert_eq!((pcb, b), (pe::PCB, board));
        assert_eq!(host.frame.origin, [100.0, 0.0, 50.0]);
        assert_eq!(
            layout_of(&ad, f).map(|l| l.footprints.len()),
            ad.doc
                .element(pe::PCB)
                .unwrap()
                .pcb()
                .unwrap()
                .board(board)
                .unwrap()
                .design()
                .map(|d| d.board.footprints.len())
        );
    }

    #[test]
    fn a_board_inside_a_subassembly_is_hosted_where_it_ends_up() {
        use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
        let mut doc = pe::document().unwrap();
        let board = doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id;
        // "Sub": the board 10 along x. The top assembly: Sub 100 along y.
        let mut sub = cadrs_core::Element::assembly("Sub");
        let sub_id = ElementId::new();
        sub.id = sub_id;
        let parts = doc
            .element(pe::PCB)
            .unwrap()
            .pcb()
            .unwrap()
            .board_parts(board);
        sub.assembly_model_mut()
            .unwrap()
            .instances
            .push(Instance::board(
                InstanceId::new(),
                pe::PCB,
                board,
                parts,
                Pose {
                    translation: [10.0, 0.0, 0.0],
                    ..Pose::IDENTITY
                },
            ));
        let mut top = cadrs_core::Element::assembly("Top");
        let top_id = ElementId::new();
        top.id = top_id;
        let si = InstanceId::new();
        top.assembly_model_mut()
            .unwrap()
            .instances
            .push(Instance::new(
                si,
                InstanceSource::Assembly { element: sub_id },
                Pose {
                    translation: [0.0, 100.0, 0.0],
                    ..Pose::IDENTITY
                },
            ));
        doc.elements.push(sub);
        doc.elements.push(top);
        let mut ad = ActiveDocument::new(doc);
        ad.set_active(top_id);
        let el = ad.doc.element(top_id).unwrap().clone();
        let mut cache = PartCache::default();
        let mut parts = crate::assembly::AssemblyParts::default();
        crate::assembly::update_assembly_parts(&ad, &el, &mut cache, &mut parts, None);
        // The subassembly's menu lists the board; its output sits where both moves put it.
        let inner = boards_inside(&ad, si);
        assert_eq!(inner.len(), 1, "{inner:?}");
        let f = inner[0].0;
        assert_eq!(
            cache.derived.get(&f).map(|o| o.boards[0].frame.origin),
            Some([10.0, 100.0, 0.0])
        );
        let editing = BoardEditing {
            target: Some(BoardTarget {
                studio: top_id,
                feature: f,
                copy: 0,
            }),
        };
        let (pcb, b, host) = edited_board(&ad, &editing, &cache).expect("editable in context");
        assert_eq!(
            (pcb, b, host.frame.origin),
            (pe::PCB, board, [10.0, 100.0, 0.0])
        );
    }

    #[test]
    fn two_boards_two_levels_down_are_hosted_apart() {
        use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
        let mut doc = pe::document().unwrap();
        let board = doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id;
        let at = |x: f64, y: f64| Pose {
            translation: [x, y, 0.0],
            ..Pose::IDENTITY
        };
        // "Inner": the board twice, at x 0 and x 50; "Sub": Inner at x 10; "Top": Sub at y 100.
        let parts = doc
            .element(pe::PCB)
            .unwrap()
            .pcb()
            .unwrap()
            .board_parts(board);
        let (inner_id, sub_id, top_id) = (ElementId::new(), ElementId::new(), ElementId::new());
        let mut inner = cadrs_core::Element::assembly("Inner");
        inner.id = inner_id;
        for x in [0.0, 50.0] {
            inner
                .assembly_model_mut()
                .unwrap()
                .instances
                .push(Instance::board(
                    InstanceId::new(),
                    pe::PCB,
                    board,
                    parts.clone(),
                    at(x, 0.0),
                ));
        }
        let mut sub = cadrs_core::Element::assembly("Sub");
        sub.id = sub_id;
        sub.assembly_model_mut()
            .unwrap()
            .instances
            .push(Instance::new(
                InstanceId::new(),
                InstanceSource::Assembly { element: inner_id },
                at(10.0, 0.0),
            ));
        let mut top = cadrs_core::Element::assembly("Top");
        top.id = top_id;
        let si = InstanceId::new();
        top.assembly_model_mut()
            .unwrap()
            .instances
            .push(Instance::new(
                si,
                InstanceSource::Assembly { element: sub_id },
                at(0.0, 100.0),
            ));
        doc.elements.extend([inner, sub, top]);
        let mut ad = ActiveDocument::new(doc);
        ad.set_active(top_id);
        let el = ad.doc.element(top_id).unwrap().clone();
        let mut cache = PartCache::default();
        let mut parts = crate::assembly::AssemblyParts::default();
        crate::assembly::update_assembly_parts(&ad, &el, &mut cache, &mut parts, None);
        let inner = boards_inside(&ad, si);
        assert_eq!(inner.len(), 2, "{inner:?}");
        let outs: Vec<&cadrs_core::derived::DerivedOutput> =
            inner.iter().map(|(f, _)| &cache.derived[f]).collect();
        let mut origins: Vec<[f64; 3]> = outs.iter().map(|o| o.boards[0].frame.origin).collect();
        origins.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(origins, vec![[10.0, 100.0, 0.0], [60.0, 100.0, 0.0]]);
        // Each its own parts.
        assert!(outs[0].parts.iter().all(|p| !outs[1].parts.contains(p)));
        assert_eq!(outs[0].parts.len(), outs[1].parts.len());
        assert_eq!(assembly_boards(&ad).len(), 2);
    }

    #[test]
    fn a_view_ray_meets_the_face_it_sees() {
        let frame = PlaneFrame {
            origin: [0.0, 0.0, 12.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
        };
        let host = crate::eda::PlaneHost {
            target: BoardTarget {
                studio: ElementId::new(),
                feature: FeatureId::new(),
                copy: 0,
            },
            frame,
            thickness: 1.6,
        };
        // From above, slanting (as a perspective ray does): it meets the top face (z 13.6).
        let o = Vec3::new(10.0, -20.0, 113.6);
        let d = (Vec3::new(4.0, 6.0, 13.6) - o).normalize();
        let p = ray_to_layout(&host, o, d).unwrap();
        assert!(
            (p.x as f64 / 1e6 - 4.0).abs() < 1e-3 && (p.y as f64 / 1e6 - 6.0).abs() < 1e-3,
            "{p:?}"
        );
        // From below: the bottom face (z 12).
        let o = Vec3::new(0.0, 0.0, -50.0);
        let d = (Vec3::new(-3.0, 2.0, 12.0) - o).normalize();
        let p = ray_to_layout(&host, o, d).unwrap();
        assert!(
            (p.x as f64 / 1e6 + 3.0).abs() < 1e-3 && (p.y as f64 / 1e6 - 2.0).abs() < 1e-3,
            "{p:?}"
        );
        // Along the board: nowhere.
        assert!(ray_to_layout(&host, Vec3::new(0.0, 0.0, 13.0), Vec3::X).is_none());
    }
}
