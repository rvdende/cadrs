//! **Section view** (P3E.3a, PS2.9, X14, A3.3, X15, IR5.5): the 3D view of a Part Studio or an
//! Assembly cut by a plane, with the cut faces capped, like Onshape's.
//!
//! - Opened from the view cube menu's **Section view…**, the bottom-right Section view tool, a
//!   feature's context menu (**Section view…**, IR5.5: on a plane or a sketch, cut by its plane)
//!   and an assembly instance's or triad's menu (A3.3, X15: cut through the instance's middle by
//!   the Front plane). The **Section view** dialog takes the cutting plane: a default plane, a
//!   plane feature, a sketch or a planar face, picked in the view; **Offset** moves it along its
//!   normal and the flip button keeps the other side. ✓ keeps the section while you work; ✕
//!   (or **Exit section view**, the menu item and the tool again) ends it.
//! - The material on the side the plane's normal points to is removed: the part meshes are
//!   clipped by the plane on the GPU (`part_shading.wgsl`), the edges on the CPU, and picking
//!   ignores what was cut away (a pick through a cap stops at it).
//! - The **caps** are the faces a half-space boolean leaves on the plane
//!   ([`cadrs_core::section`], the drawings' section cut), worked out on the kernel thread for
//!   each part shown and cached per plane (both sides of a plane share their caps), drawn in
//!   each part's colour with their outlines.
//! - A section is a view, per tab: nothing in the document changes and nothing is undone.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::section::{Cap, SectionItem, SectionPlane};
use cadrs_core::{ElementId, FeatureId, PartId};
use cadrs_sketch::FaceName;
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit, SelectionList};

use crate::part_shading::{PartShading, PartShadingParams};
use crate::parts::{PartCache, PartEdgeGizmos, PickFilter};
use crate::viewport::{ActiveKind, Pick, PickFilterOverride, PickRequest, PlaneKind, PlanesVisible, Selection, ViewportArea, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct SectionViewPlugin;

impl Plugin for SectionViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SectionViews>()
            .init_resource::<SectionClip>()
            .add_systems(
                Update,
                (take_picks, follow_selection, sync_dialog, compute_caps, sync_cap_meshes, draw_caps)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(PostUpdate, sync_shading)
            .add_systems(OnExit(AppState::Document), |mut views: ResMut<SectionViews>, mut clip: ResMut<SectionClip>, mut over: ResMut<PickFilterOverride>| {
                if views.dialog.is_some() {
                    over.0 = None;
                }
                *views = SectionViews::default();
                *clip = SectionClip::default();
            })
            .add_observer(on_tool)
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_offset);
    }
}

/// What a section plane was picked from (its field's label).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionRef {
    Plane(PlaneKind),
    Face(PartId, FaceName),
    Feature(FeatureId),
    /// Through an assembly instance's middle (A3.3, X15).
    Instance(PartId),
}

/// A tab's section: its plane (where it was picked, and its normal), flip and offset (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionState {
    pub reference: Option<SectionRef>,
    pub label: String,
    pub origin: Vec3,
    pub normal: Vec3,
    pub flip: bool,
    pub offset: f32,
}

impl Default for SectionState {
    fn default() -> Self {
        Self { reference: None, label: String::new(), origin: Vec3::ZERO, normal: Vec3::Z, flip: false, offset: 0.0 }
    }
}

impl SectionState {
    /// The cutting plane: a point on it and the normal of the side removed.
    pub fn plane(&self) -> Option<(Vec3, Vec3)> {
        self.reference?;
        let n = self.normal.normalize_or_zero();
        if n == Vec3::ZERO {
            return None;
        }
        Some((self.origin + n * self.offset, if self.flip { -n } else { n }))
    }
}

/// The section of each tab, and the tab whose Section view dialog is open.
#[derive(Resource, Debug, Default)]
pub struct SectionViews {
    pub per: HashMap<ElementId, SectionState>,
    pub dialog: Option<ElementId>,
}

/// The active tab's section as drawn: the clip plane (a point and the removed side's normal)
/// and the caps of the parts on screen.
#[derive(Resource, Default)]
pub struct SectionClip {
    pub plane: Option<(Vec3, Vec3)>,
    pub caps: Arc<Caps>,
    key: Option<CapKey>,
    pending: Option<(CapKey, cadrs_core::rebuild::PendingJob<Caps>)>,
    cache: Vec<(CapKey, Arc<Caps>)>,
}

/// The caps of the parts on screen.
pub type Caps = Vec<(PartId, Cap)>;

/// What the cap meshes were built for: the caps, the plane's normal (bits) and the parts.
type CapMeshKey = (usize, Option<[u32; 3]>, u64);

/// What the material uniforms were set from.
type ShadingKey = (Option<(Vec3, Vec3)>, bool, bool, usize);

impl SectionClip {
    /// The caps are being worked out.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
}

/// What a set of caps was worked out for: the tab, the parts' generation and the plane
/// (unoriented: both sides of a plane have the same caps), to 1 µm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CapKey(ElementId, u64, [i64; 4]);

const CACHED_PLANES: usize = 8;

fn cap_key(element: ElementId, generation: u64, origin: Vec3, normal: Vec3) -> CapKey {
    let mut n = normal.normalize();
    let first = [n.x, n.y, n.z].into_iter().find(|c| c.abs() > 1e-6).unwrap_or(1.0);
    if first < 0.0 {
        n = -n;
    }
    let d = n.dot(origin);
    let q = |v: f32| (v as f64 * 1e6).round() as i64;
    CapKey(element, generation, [q(n.x), q(n.y), q(n.z), (d as f64 * 1e3).round() as i64])
}

/// The active tab has a section.
pub fn active(world: &World) -> bool {
    let Some(el) = world.get_resource::<ActiveDocument>().and_then(|d| d.active) else { return false };
    world.resource::<SectionViews>().per.get(&el).is_some_and(|s| s.reference.is_some())
}

fn modeling_tab(world: &World) -> Option<ElementId> {
    let kind = *world.resource::<ActiveKind>();
    if !matches!(kind, ActiveKind::PartStudio | ActiveKind::Assembly) {
        return None;
    }
    world.get_resource::<ActiveDocument>()?.active
}

/// The view menu's Section view… / Exit section view, and the bottom-right tool: opens the
/// dialog (with a plane, face or sketch selected, cut by it), or ends the section.
pub fn toggle(world: &mut World) {
    let Some(el) = modeling_tab(world) else { return };
    let views = world.resource::<SectionViews>();
    if views.dialog == Some(el) {
        return;
    }
    if views.per.get(&el).is_some_and(|s| s.reference.is_some()) {
        exit(world);
        return;
    }
    let picked = world.resource::<Selection>().0.iter().rev().find_map(|p| resolve(world, *p));
    open_with(world, picked);
}

/// Ends the active tab's section.
pub fn exit(world: &mut World) {
    let Some(el) = world.get_resource::<ActiveDocument>().and_then(|d| d.active) else { return };
    close_dialog(world);
    world.resource_mut::<SectionViews>().per.remove(&el);
}

/// Opens the dialog on the active tab, with `picked` as its plane if given.
pub fn open_with(world: &mut World, picked: Option<SectionState>) {
    let Some(el) = modeling_tab(world) else { return };
    let kind = *world.resource::<ActiveKind>();
    let mut views = world.resource_mut::<SectionViews>();
    let state = views.per.entry(el).or_default();
    if let Some(p) = picked {
        *state = SectionState { flip: false, offset: 0.0, ..p };
    }
    views.dialog = Some(el);
    let planes = world.resource::<PlanesVisible>().0;
    let filter = if kind == ActiveKind::Assembly {
        PickFilter { faces: true, planar_only: true, ..PickFilter::none() }
    } else {
        PickFilter { planes, faces: true, planar_only: true, plane_features: true, ..PickFilter::none() }
    };
    world.resource_mut::<PickFilterOverride>().0 = Some(filter);
}

/// A feature's menu (IR5.5): cut by the feature's plane (a plane feature, a sketch's plane).
pub fn open_for_feature(world: &mut World, f: FeatureId) {
    let picked = resolve(world, Pick::Feature(f));
    open_with(world, picked);
}

/// An assembly instance's or triad's menu (A3.3, X15): cut through the instance's middle by
/// the Front plane.
pub fn open_for_instance(world: &mut World, parts: &[PartId]) {
    let cache = world.resource::<PartCache>();
    let pts: Vec<Vec3> = cache
        .parts
        .iter()
        .filter(|p| parts.iter().any(|q| *q == p.id || (q.index == 0 && q.feature == p.id.feature)))
        .flat_map(|p| p.solid.positions.iter().map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)))
        .collect();
    let picked = (!pts.is_empty()).then(|| {
        let (lo, hi) = pts.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        let name = parts.first().and_then(|p| cache.part_name(*p)).unwrap_or("instance").to_string();
        SectionState {
            reference: parts.first().map(|p| SectionRef::Instance(*p)),
            label: format!("Front plane through {name}"),
            origin: (lo + hi) / 2.0,
            normal: PlaneKind::Front.normal(),
            ..default()
        }
    });
    open_with(world, picked);
}

/// The plane a pick stands for, if it is planar.
fn resolve(world: &World, pick: Pick) -> Option<SectionState> {
    let cache = world.resource::<PartCache>();
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let (reference, label, origin, normal) = match pick {
        Pick::Plane(k) => (SectionRef::Plane(k), format!("{} plane", k.name()), Vec3::ZERO, k.normal()),
        Pick::Face(part, name) => {
            let p = cache.part(part)?;
            let f = p.solid.face(&name)?;
            f.plane?;
            // The mesh's normal (outward), not the frame's.
            let n = p.solid.indices.get(3 * f.first_triangle).and_then(|&i| p.solid.normals.get(i as usize)).map(|n| v3(*n))?;
            let o = f.loops.first()?.first().map(|q| v3(*q))?;
            let doc = world.get_resource::<ActiveDocument>()?;
            let label = crate::parts::pick_label(doc.active_element().map(|e| e.features()).unwrap_or(&[]), cache, pick).unwrap_or_else(|| "Face".into());
            (SectionRef::Face(part, name), label, o, n)
        }
        Pick::Feature(f) => {
            let doc = world.get_resource::<ActiveDocument>()?;
            let feature = doc.active_element()?.feature(f)?;
            let frame = match cache.planes.get(&f) {
                Some(frame) => *frame,
                None => feature.sketch()?.plane?.frame(),
            };
            (SectionRef::Feature(f), feature.name.clone(), v3(frame.origin), v3(frame.normal()))
        }
        _ => return None,
    };
    Some(SectionState { reference: Some(reference), label, origin, normal: normal.normalize_or_zero(), ..default() })
}

fn close_dialog(world: &mut World) {
    let Some(el) = world.resource_mut::<SectionViews>().dialog.take() else { return };
    world.resource_mut::<PickFilterOverride>().0 = None;
    // The plane picked for it is no longer selected.
    let reference = world.resource::<SectionViews>().per.get(&el).and_then(|s| s.reference);
    let pick = match reference {
        Some(SectionRef::Plane(k)) => Some(Pick::Plane(k)),
        Some(SectionRef::Face(p, f)) => Some(Pick::Face(p, f)),
        Some(SectionRef::Feature(f)) => Some(Pick::Feature(f)),
        _ => None,
    };
    if let Some(pick) = pick {
        let mut selection = world.resource_mut::<Selection>();
        if selection.0.contains(&pick) {
            selection.0.retain(|p| *p != pick);
        }
    }
}

/// The bottom-right Section view tool, and the assembly toolbar's.
fn on_tool(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).is_ok_and(|n| matches!(n.as_str(), "view-section" | "section-view")) {
        commands.queue(toggle);
    }
}

/// ✓: the section stays (with no plane picked, there is none).
pub fn accept(world: &mut World) {
    let Some(el) = world.resource::<SectionViews>().dialog else { return };
    close_dialog(world);
    let mut views = world.resource_mut::<SectionViews>();
    if views.per.get(&el).is_none_or(|s| s.reference.is_none()) {
        views.per.remove(&el);
    }
}

/// The dialog's picks in the view (the pick filter is overridden, so they don't reach the
/// selection on their own): a plane, a planar face or a plane feature becomes the selection.
fn take_picks(mut picks: MessageReader<PickRequest>, views: Res<SectionViews>, mut selection: ResMut<Selection>) {
    if views.dialog.is_none() {
        picks.clear();
        return;
    }
    if let Some(pick) = picks.read().filter_map(|p| p.0).last() {
        selection.0 = vec![pick];
    }
}

/// While the dialog is open, the selection (from the view, or a plane or sketch clicked in the
/// feature list) sets the plane.
fn follow_selection(selection: Res<Selection>, views: Res<SectionViews>, mut commands: Commands) {
    if views.dialog.is_none() || !selection.is_changed() {
        return;
    }
    let picks = selection.0.clone();
    commands.queue(move |world: &mut World| {
        let Some(el) = world.resource::<SectionViews>().dialog else { return };
        let Some(state) = picks.iter().rev().find_map(|p| resolve(world, *p)) else { return };
        let mut views = world.resource_mut::<SectionViews>();
        let s = views.per.entry(el).or_default();
        if s.reference != state.reference {
            *s = SectionState { flip: s.flip, offset: s.offset, ..state };
        }
    });
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<SectionDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<SectionDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(exit);
    }
}

fn on_offset(ev: On<NumberFieldCommit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("section-offset") {
        return;
    }
    let text = ev.text.clone();
    commands.queue(move |world: &mut World| {
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let Some(mm) = units.parse_length(&text) else { return };
        let mut views = world.resource_mut::<SectionViews>();
        let Some(el) = views.dialog else { return };
        if let Some(s) = views.per.get_mut(&el) {
            s.offset = mm as f32;
        }
    });
}

fn flip(world: &mut World) {
    let mut views = world.resource_mut::<SectionViews>();
    let Some(el) = views.dialog else { return };
    let s = views.per.entry(el).or_default();
    s.flip = !s.flip;
}

#[derive(Component)]
struct SectionDialog(String);

fn section_dialog(t: &Theme, s: &SectionState, offset: String) -> impl Bundle {
    let tb = t.clone();
    let items = if s.reference.is_some() { vec![s.label.clone()] } else { Vec::new() };
    let flipped = s.flip;
    FeatureDialog::new("section-dialog")
        .title("Section view")
        .valid(s.reference.is_some())
        .width(240.0)
        .body(move |b| {
            b.spawn(SelectionList::new("section-plane").placeholder("Section plane").items(items).active(true).build(&tb))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.flex_grow = 0.0;
                    n.margin = UiRect::vertical(Val::Px(2.0));
                });
            b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
                r.spawn(NumberField::new("section-offset", "Offset").text(offset).label_width(52.0).build(&tb))
                    .entry::<Node>()
                    .and_modify(|mut n| n.flex_grow = 1.0);
                r.spawn((
                    IconButton::new("section-flip", crate::extrude_dialog::flip_icon(flipped)).icon_size(20.0).tooltip("Flip the side removed").build(&tb),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(flip)),
                ))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.width = Val::Px(22.0);
                    n.height = Val::Px(22.0);
                    n.flex_shrink = 0.0;
                });
            });
        })
        .build(t)
}

fn sync_dialog(
    views: Res<SectionViews>,
    doc: Option<Res<ActiveDocument>>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    q_dialog: Query<(Entity, &SectionDialog, &FeatureDialogState)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let open = views.dialog.filter(|el| doc.as_ref().and_then(|d| d.active) == Some(*el));
    let Some(el) = open else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let s = views.per.get(&el).cloned().unwrap_or_default();
    let offset = units.0.with_unit(s.offset as f64, cadrs_sketch::units::Quantity::Length);
    let key = format!("{:?}|{}|{}|{}", s.reference.is_some(), s.label, s.flip, offset);
    if let Some((e, d, _)) = q_dialog.iter().next() {
        if d.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else { return };
    let d = commands.spawn((SectionDialog(key), DespawnOnExit(AppState::Document), section_dialog(&theme, &s, offset))).id();
    commands.entity(area).add_child(d);
}

// ---------------------------------------------------------------------------------------------
// The clip plane and the caps

fn compute_caps(views: Res<SectionViews>, doc: Option<Res<ActiveDocument>>, mut cache: ResMut<PartCache>, mut clip: ResMut<SectionClip>) {
    let el = doc.as_ref().and_then(|d| d.active);
    let plane = el.and_then(|el| views.per.get(&el)).and_then(|s| s.plane());
    if clip.plane != plane {
        clip.plane = plane;
    }
    let pick = plane.map(|(o, n)| crate::parts::SectionPick { origin: o, normal: n, caps: Vec::new() });
    let Some((origin, normal)) = plane else {
        if !clip.caps.is_empty() {
            clip.caps = Arc::default();
        }
        clip.key = None;
        clip.pending = None;
        if cache.section.is_some() {
            cache.section = None;
        }
        return;
    };
    let (Some(doc), Some(el)) = (doc, el) else { return };
    let key = cap_key(el, cache.generation, origin, normal);
    let caps_for_pick = |caps: &[(PartId, Cap)]| {
        let v = |p: [f32; 3]| Vec3::from_array(p);
        caps.iter().flat_map(|(_, c)| c.triangles.iter().map(|t| t.map(v))).collect::<Vec<_>>()
    };
    if clip.key == Some(key) {
        // The plane moved along itself (a flip): the same caps, the other side.
        if cache.section.as_ref().is_none_or(|s| s.normal != normal || s.origin != origin) {
            cache.section = pick.map(|p| crate::parts::SectionPick { caps: caps_for_pick(&clip.caps), ..p });
        }
        return;
    }
    if let Some((_, hit)) = clip.cache.iter().find(|(k, _)| *k == key) {
        let hit = hit.clone();
        cache.section = pick.map(|p| crate::parts::SectionPick { caps: caps_for_pick(&hit), ..p });
        clip.caps = hit;
        clip.key = Some(key);
        clip.pending = None;
        return;
    }
    // The clip plane applies at once; the caps follow when the kernel has them.
    if cache.section.as_ref().is_none_or(|s| s.normal != normal || s.origin != origin) {
        cache.section = pick.clone();
    }
    if let Some((k, job)) = &clip.pending
        && *k == key
    {
        if let Some(done) = job.poll() {
            let caps = Arc::new(done.unwrap_or_default());
            clip.cache.retain(|(k, _)| k.0 != el || k.1 == key.1);
            clip.cache.push((key, caps.clone()));
            if clip.cache.len() > CACHED_PLANES {
                clip.cache.remove(0);
            }
            cache.section = pick.map(|p| crate::parts::SectionPick { caps: caps_for_pick(&caps), ..p });
            clip.caps = caps;
            clip.key = Some(key);
            clip.pending = None;
        }
        return;
    }
    if cache.rebuilding {
        return;
    }
    let shown: Vec<PartId> = cache.shown().map(|p| p.id).collect();
    let items: Vec<SectionItem> = if cache.assembly.is_some() {
        let Some(asm) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
        cadrs_core::assembly::interference::items(&doc.doc, asm)
            .into_iter()
            .filter(|it| shown.contains(&it.view_part))
            .map(|it| SectionItem { view_part: it.view_part, features: it.features, part: it.part, pose: it.pose })
            .collect()
    } else {
        let Some((_, features, _)) = cache.settled() else { return };
        let features = features.to_vec();
        shown.iter().map(|p| SectionItem { view_part: *p, features: features.clone(), part: *p, pose: cadrs_core::assembly::Pose::IDENTITY }).collect()
    };
    let plane = SectionPlane { origin: origin.as_dvec3().to_array(), normal: normal.as_dvec3().to_array() };
    clip.pending = Some((key, cadrs_core::section::caps(items, plane)));
}

/// A cap's mesh.
#[derive(Component)]
struct CapMesh;

#[allow(clippy::too_many_arguments)]
fn sync_cap_meshes(
    clip: Res<SectionClip>,
    cache: Res<PartCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PartShading>>,
    mut mat: Local<Option<Handle<PartShading>>>,
    q: Query<Entity, With<CapMesh>>,
    mut last: Local<Option<CapMeshKey>>,
    mut commands: Commands,
) {
    let normal = clip.plane.map(|(_, n)| n);
    let key = (Arc::as_ptr(&clip.caps) as usize, normal.map(|n| n.to_array().map(f32::to_bits)), cache.generation);
    if last.as_ref() == Some(&key) && (normal.is_some() || q.is_empty()) {
        return;
    }
    *last = Some(key);
    for e in &q {
        commands.entity(e).try_despawn();
    }
    let Some(n) = normal else { return };
    let material = mat.get_or_insert_with(|| materials.add(PartShading { cap: true, ..PartShading::new(false, false) })).clone();
    for (part, cap) in clip.caps.iter() {
        let Some(p) = cache.part(*part) else { continue };
        if !cache.shown().any(|s| s.id == *part) {
            continue;
        }
        let base = crate::parts::FaceBase::of(cadrs_core::appearance::part_appearance(p, &cache.props));
        let tint = cache.tints.get(part).copied().unwrap_or(base);
        // A hair toward the eye, so a plane drawn on the cut (the picked Front plane) doesn't
        // fight the cap for depth.
        let lift = n * 0.05;
        let positions: Vec<[f32; 3]> = cap.triangles.iter().flatten().map(|p| (Vec3::from_array(*p) + lift).to_array()).collect();
        let count = positions.len();
        let color = [tint.rgb[0] / 255.0, tint.rgb[1] / 255.0, tint.rgb[2] / 255.0, 1.0];
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count])
            .with_inserted_indices(Indices::U32((0..count as u32).collect()));
        commands.spawn((
            Name::new(format!("section-cap-{}", p.name.to_lowercase().replace(' ', "-"))),
            CapMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            DespawnOnExit(AppState::Document),
        ));
    }
}

/// The caps' outlines, as part edges.
fn draw_caps(clip: Res<SectionClip>, cache: Res<PartCache>, view: Res<ViewportView>, mut edges: Gizmos<PartEdgeGizmos>) {
    if clip.plane.is_none() || !(view.view.render.edges() || view.view.render.line_drawing()) {
        return;
    }
    for (part, cap) in clip.caps.iter() {
        if !cache.shown().any(|p| p.id == *part) {
            continue;
        }
        for l in &cap.loops {
            let mut pts: Vec<Vec3> = l.iter().map(|p| Vec3::from_array(*p)).collect();
            if let Some(f) = pts.first().copied() {
                pts.push(f);
            }
            edges.linestrip(pts, Color::srgb_u8(0x14, 0x14, 0x14));
        }
    }
}

/// The parts' material uniforms: the clip plane, the hidden-line modes' white faces and the
/// Translucent mode's opacity.
fn sync_shading(clip: Res<SectionClip>, view: Res<ViewportView>, kind: Res<ActiveKind>, mut materials: ResMut<Assets<PartShading>>, mut last: Local<Option<ShadingKey>>) {
    let modeling = matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let plane = clip.plane.filter(|_| modeling);
    let mode = view.view.render;
    let key = (plane, !mode.shaded(), mode.translucent(), materials.len());
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let clip_v = plane.map_or(Vec4::ZERO, |(o, n)| n.extend(n.dot(o)));
    let ids: Vec<AssetId<PartShading>> = materials.ids().collect();
    for id in ids {
        let Some(m) = materials.get(id) else { continue };
        let params = PartShadingParams {
            clip: if m.cap { Vec4::ZERO } else { clip_v },
            style: Vec4::new(if mode.shaded() { 0.0 } else { 1.0 }, if m.translucent { TRANSLUCENT_ALPHA } else { 1.0 }, 0.0, 0.0),
        };
        if m.params != params
            && let Some(mut m) = materials.get_mut(id)
        {
            m.params = params;
        }
    }
}

/// The Translucent render mode's opacity.
pub const TRANSLUCENT_ALPHA: f32 = 0.35;

/// The pieces of a polyline on the kept side of a section plane (`origin`, removed side's
/// `normal`), cut where it crosses the plane.
pub fn clip_polyline(pts: impl IntoIterator<Item = Vec3>, (origin, normal): (Vec3, Vec3)) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    let mut prev: Option<(Vec3, f32)> = None;
    for p in pts {
        let d = (p - origin).dot(normal);
        if let Some((q, dq)) = prev
            && (dq <= 0.0) != (d <= 0.0)
        {
            cur.push(q.lerp(p, dq / (dq - d)));
            if dq <= 0.0 {
                out.push(std::mem::take(&mut cur));
            }
        }
        if d <= 0.0 {
            cur.push(p);
        }
        prev = Some((p, d));
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.retain(|l| l.len() >= 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polylines_are_clipped_at_the_plane() {
        let plane = (Vec3::new(0.0, 0.0, 5.0), Vec3::Z);
        // Up through the plane and back down: two pieces, cut at z = 5.
        let pts = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 10.0), Vec3::new(2.0, 0.0, 0.0)];
        let pieces = clip_polyline(pts, plane);
        assert_eq!(pieces.len(), 2);
        assert!((pieces[0][1] - Vec3::new(0.5, 0.0, 5.0)).length() < 1e-5);
        assert!((pieces[1][0] - Vec3::new(1.5, 0.0, 5.0)).length() < 1e-5);
        // All kept, all removed.
        assert_eq!(clip_polyline(pts, (Vec3::new(0.0, 0.0, 20.0), Vec3::Z)), vec![pts.to_vec()]);
        assert!(clip_polyline(pts, (Vec3::new(0.0, 0.0, -1.0), Vec3::Z)).is_empty());
    }

    #[test]
    fn a_flipped_plane_shares_its_caps() {
        let el = ElementId::new();
        let o = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(cap_key(el, 4, o, Vec3::Z), cap_key(el, 4, o, -Vec3::Z));
        assert_ne!(cap_key(el, 4, o, Vec3::Z), cap_key(el, 4, o + Vec3::Z, Vec3::Z));
        let s = SectionState { reference: Some(SectionRef::Plane(PlaneKind::Top)), normal: Vec3::Z, offset: 5.0, flip: true, ..default() };
        assert_eq!(s.plane(), Some((Vec3::new(0.0, 0.0, 5.0), -Vec3::Z)));
        assert_eq!(SectionState::default().plane(), None);
    }
}
