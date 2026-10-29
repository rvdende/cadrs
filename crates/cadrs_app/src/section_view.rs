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
//! - A default plane needn't be picked in the view (an Assembly shows none, and a part with no
//!   flat face offers nothing else, issue #122): the plane button beside **Section planes**
//!   opens Onshape's small chooser, **Top**, **Front** or **Right**, with the flip button and
//!   ✓ / ✕ ([`DefaultPlanePicker`]).
//! - Up to four **Section planes**: the material on the removed side of every plane goes (two
//!   planes take out a wedge). The plane row clicked is the one the Offset, Angle, flip and
//!   gizmo act on. The **Exclude** tab's items are left whole; the **Include** tab's are the
//!   only ones cut.
//! - The gizmo: the arrow moves the plane along its normal, the arc turns it about its in-plane
//!   axis (the angle shown beside it, "25 deg").
//! - The material removed is clipped on the GPU (`part_shading.wesl`), the edges on the CPU, and
//!   picking ignores what was cut away (a pick through a cap stops at it). The caps are hatched.
//! - During movement, the displayed mesh supplies immediate closing faces. After release,
//!   one exact cap job at a time replaces these previews; obsolete drag positions are not queued.
//!   Exact **caps** are the faces a half-space boolean leaves on the plane
//!   ([`cadrs_core::section`], the drawings' section cut), worked out on the kernel thread for
//!   each part shown and cached per plane (both sides of a plane share their caps), drawn in
//!   each part's colour with their outlines.
//! - While the dialog is open the cutting plane is drawn where it cuts (the picked plane moved by
//!   the offset), in the selection orange, with the blue 3D **drag arrow** at its middle
//!   ([`crate::manipulator`], pointing to the removed side): dragging it moves the plane along
//!   its normal, the cut following the pointer 1:1. The reference picked for it (a plane, a
//!   sketch, a face) stays selected, highlighted in the list and the view, until the dialog
//!   closes.
//! - The default planes and plane features stay whole; a sketch on the removed side is not
//!   drawn.
//! - A section is a view, per tab: nothing in the document changes and nothing is undone.

mod preview;

use std::collections::HashMap;
use std::sync::Arc;

use crate::GizmoGroups;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::section::{Cap, SectionItem, SectionPlane};
use cadrs_core::{ElementId, FeatureId, PartId};
use cadrs_sketch::FaceName;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit,
    Select, SelectChange, SelectionList, SelectionListActivate, SelectionListItem,
    SelectionListRemove, TabStrip, TabStripSelect,
};

use crate::part_shading::{PartShading, PartShadingParams};
use crate::parts::{PartCache, PartEdgeGizmos, PickFilter};
use crate::viewport::{
    ActiveKind, Pick, PickFilterOverride, PickRequest, PlaneKind, PlanesVisible, Selection,
    ViewportArea, ViewportView,
};
use crate::{ActiveDocument, AppState};

pub struct SectionViewPlugin;

impl Plugin for SectionViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SectionViews>()
            .gizmo_group::<SectionPlaneGizmos>()
            .gizmo_group::<SectionArcGizmos>()
            .add_systems(Startup, configure_gizmos)
            .init_resource::<SectionClip>()
            .init_resource::<SectionArrow>()
            .init_resource::<ShownBounds>()
            .add_systems(
                Update,
                (
                    take_picks,
                    section_arrow_pointer,
                    sync_dialog,
                    compute_caps,
                    sync_cap_meshes,
                    draw_caps,
                    track_bounds,
                    sync_section_plane,
                    place_section_arrow,
                )
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(PostUpdate, sync_shading)
            .add_systems(
                OnExit(AppState::Document),
                |mut views: ResMut<SectionViews>,
                 mut clip: ResMut<SectionClip>,
                 mut over: ResMut<PickFilterOverride>,
                 mut arrow: ResMut<SectionArrow>| {
                    if views.dialog.is_some() {
                        over.0 = None;
                    }
                    *views = SectionViews::default();
                    *clip = SectionClip::default();
                    *arrow = SectionArrow::default();
                },
            )
            .add_observer(on_tool)
            .add_observer(on_chooser_kind)
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_number)
            .add_observer(on_tab)
            .add_observer(on_list_activate)
            .add_observer(on_list_remove)
            .add_observer(on_plane_row);
    }
}

/// The section plane's outline: depth-tested with a small bias, so the kept part hides it
/// where it is in front (P3E.3a judge: it drew over the part after a flip).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SectionPlaneGizmos;

/// The gizmo's rotation arc: drawn over the parts, as its arrow.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SectionArcGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (arc, _) = store.config_mut::<SectionArcGizmos>();
    arc.line.width = 2.6;
    arc.depth_bias = -1.0;
    let (config, _) = store.config_mut::<SectionPlaneGizmos>();
    config.line.width = 2.4;
    config.depth_bias = -0.002;
}

/// What a section plane was picked from (its row's label).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionRef {
    Plane(PlaneKind),
    Face(PartId, FaceName),
    Feature(FeatureId),
    /// Through an assembly instance's middle (A3.3, X15).
    Instance(PartId),
}

/// The most section planes a section has (the shaders' clip planes).
pub const MAX_PLANES: usize = 4;

/// How far (mm) a section plane sits past where it was put, into its removed side, so the
/// faces lying in it don't fight the caps (the same cut as an offset of 0.001 mm).
pub const PLANE_NUDGE: f32 = 1e-3;

/// One section plane: where it was picked (a point on it, centred on the parts, and its
/// normal), an in-plane axis (how its rectangle is drawn), its flip and offset (mm), and how it
/// was turned: two intrinsic tilts, first about axis 1, then the carried local axis 2.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionCutPlane {
    pub reference: SectionRef,
    pub label: String,
    pub origin: Vec3,
    /// Current arrow origin and rotation pivot, independent of the reference orientation.
    pivot: Vec3,
    pub normal: Vec3,
    pub axis: Vec3,
    pub flip: bool,
    pub offset: f32,
    pub turn: Quat,
    /// Intrinsic orientation angles in degrees, relative to the picked plane.
    tilts: [f32; 2],
}

impl SectionCutPlane {
    fn new(reference: SectionRef, label: String, origin: Vec3, normal: Vec3) -> Self {
        let n = normal.normalize_or_zero();
        let axis = match reference {
            SectionRef::Plane(k) => k.u(),
            _ if n.cross(Vec3::Z).length() > 1e-3 => Vec3::Z.cross(n).normalize(),
            _ => Vec3::X,
        };
        Self {
            reference,
            label,
            origin,
            pivot: origin,
            normal: n,
            axis,
            flip: false,
            offset: 0.0,
            turn: Quat::IDENTITY,
            tilts: [0.0; 2],
        }
    }

    /// Fixed reference axes. The second is perpendicular to the first in the picked plane.
    fn rotation_axis(&self, index: usize) -> Vec3 {
        if index == 0 {
            return self.axis;
        }
        let a = self.normal.cross(self.axis).normalize_or_zero();
        let leading = if a.x.abs() > 1e-5 {
            a.x
        } else if a.y.abs() > 1e-5 {
            a.y
        } else {
            a.z
        };
        if leading < 0.0 { -a } else { a }
    }

    pub fn angle(&self) -> f32 {
        self.tilts[0]
    }

    pub fn set_angle(&mut self, deg: f32) {
        self.set_tilt(0, deg);
    }

    /// Translate from the current pivot along the current normal. Rotating later must
    /// not reinterpret this translation along a different axis or orbit the reference point.
    fn set_offset(&mut self, offset: f32) {
        if !offset.is_finite() {
            return;
        }
        self.pivot += self.turned() * (offset - self.offset);
        self.offset = offset;
    }

    /// Apply tilt 1, then tilt 2 about the axis carried by tilt 1, irrespective of edit order.
    fn set_tilt(&mut self, index: usize, deg: f32) {
        if !deg.is_finite() {
            return;
        }
        self.tilts[index] = deg.clamp(-89.0, 89.0);
        self.turn = Quat::from_axis_angle(self.rotation_axis(0), self.tilts[0].to_radians())
            * Quat::from_axis_angle(self.rotation_axis(1), self.tilts[1].to_radians());
    }

    /// Both handles stay attached to the current plane, independent of the camera.
    fn handle_axis(&self, index: usize) -> Vec3 {
        (self.turn * self.rotation_axis(index)).normalize_or_zero()
    }

    /// Rotate the plane about a local handle at the press. A plane has two orientation
    /// degrees of freedom: discard in-plane roll and recover the two intrinsic angles.
    /// This keeps the fields and drag results identical without accumulating hidden roll.
    fn rotate_local(&mut self, start: Quat, index: usize, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        let (u, v) = (self.rotation_axis(0), self.rotation_axis(1));
        let handed = u.cross(v).dot(self.normal).signum();
        let target = Quat::from_axis_angle(start * self.rotation_axis(index), delta.to_radians())
            * (start * self.normal);
        let first = (-handed * target.dot(v))
            .atan2(target.dot(self.normal))
            .to_degrees();
        let second = (handed * target.dot(u))
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees();
        self.set_tilt(0, first);
        self.set_tilt(1, second);
    }

    /// The plane's normal turned (before the flip).
    fn turned(&self) -> Vec3 {
        (self.turn * self.normal).normalize_or_zero()
    }

    /// The cutting plane through the current pivot, nudged a hair
    /// ([`PLANE_NUDGE`]) into the removed side: a face lying in the plane (the face picked as
    /// it) is kept and drawn whole, the caps only where the plane cuts material.
    pub fn plane(&self) -> (Vec3, Vec3) {
        let n = self.turned();
        let removed = if self.flip { -n } else { n };
        (self.pivot + removed * PLANE_NUDGE, removed)
    }

    /// The plane's in-plane axes as drawn.
    fn axes(&self) -> (Vec3, Vec3) {
        let n = self.turned();
        let u = (self.turn * self.axis).normalize_or_zero();
        (u, n.cross(u).normalize_or_zero())
    }
}

/// The dialog's tabs: the items listed are left whole (Exclude), or are the only ones cut
/// (Include).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionMode {
    #[default]
    Exclude,
    Include,
}

/// The dialog field that takes the picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionField {
    #[default]
    Planes,
    Items,
}

/// A tab's section: its planes (the one the dialog's offset, angle and gizmo act on), and the
/// parts it leaves whole or only cuts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SectionState {
    pub planes: Vec<SectionCutPlane>,
    pub active: usize,
    pub mode: SectionMode,
    pub items: Vec<PartId>,
    pub field: SectionField,
    /// The default plane chooser, while open.
    pub picker: Option<DefaultPlanePicker>,
}

/// The chooser beside **Section planes** (issue #122): a default plane, and which side goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefaultPlanePicker {
    pub kind: PlaneKind,
    pub flip: bool,
}

impl Default for DefaultPlanePicker {
    fn default() -> Self {
        Self {
            kind: PlaneKind::Top,
            flip: false,
        }
    }
}

impl SectionState {
    pub fn with(plane: SectionCutPlane) -> Self {
        Self {
            planes: vec![plane],
            ..default()
        }
    }

    /// The cut: the material on the removed side of every plane goes.
    pub fn cut(&self) -> Option<Cut> {
        Cut::new(&self.planes.iter().map(|p| p.plane()).collect::<Vec<_>>())
    }

    pub fn active_plane(&self) -> Option<&SectionCutPlane> {
        self.planes.get(self.active).or(self.planes.last())
    }

    pub fn active_plane_mut(&mut self) -> Option<&mut SectionCutPlane> {
        let i = self.active.min(self.planes.len().saturating_sub(1));
        self.planes.get_mut(i)
    }

    /// The parts the section leaves whole, of the parts shown.
    pub fn excluded(&self, shown: impl Iterator<Item = PartId>) -> Vec<PartId> {
        match self.mode {
            SectionMode::Exclude => self.items.clone(),
            SectionMode::Include if self.items.is_empty() => Vec::new(),
            SectionMode::Include => shown.filter(|p| !self.items.contains(p)).collect(),
        }
    }
}

/// A section's cut as drawn and picked: up to [`MAX_PLANES`] planes (a point and the removed
/// side's normal). A point is removed when it is on the removed side of every plane, so two
/// planes take out a wedge and one plane a half-space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    len: usize,
    planes: [(Vec3, Vec3); MAX_PLANES],
}

impl Cut {
    pub fn new(planes: &[(Vec3, Vec3)]) -> Option<Cut> {
        let mut c = Cut {
            len: 0,
            planes: [(Vec3::ZERO, Vec3::Z); MAX_PLANES],
        };
        for (o, n) in planes.iter().take(MAX_PLANES) {
            let n = n.normalize_or_zero();
            if n != Vec3::ZERO {
                c.planes[c.len] = (*o, n);
                c.len += 1;
            }
        }
        (c.len > 0).then_some(c)
    }

    pub fn single(origin: Vec3, normal: Vec3) -> Cut {
        Cut::new(&[(origin, normal)]).unwrap_or(Cut {
            len: 0,
            planes: [(Vec3::ZERO, Vec3::Z); MAX_PLANES],
        })
    }

    pub fn planes(&self) -> &[(Vec3, Vec3)] {
        &self.planes[..self.len]
    }

    /// The other planes than `i` (a cap on plane `i` is only where they remove too).
    fn without(&self, i: usize) -> Option<Cut> {
        let rest: Vec<(Vec3, Vec3)> = self
            .planes()
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, p)| *p)
            .collect();
        Cut::new(&rest)
    }

    /// `p` was cut away.
    pub fn removes(&self, p: Vec3) -> bool {
        self.len > 0
            && self
                .planes()
                .iter()
                .all(|(o, n)| (p - *o).dot(*n) > ON_PLANE)
    }

    /// The shaders' clip planes: `n.extend(n·o)` each, zero for none.
    pub fn uniforms(&self) -> [Vec4; MAX_PLANES] {
        let mut u = [Vec4::ZERO; MAX_PLANES];
        for (i, (o, n)) in self.planes().iter().enumerate() {
            u[i] = n.extend(n.dot(*o));
        }
        u
    }

    /// Where the line `a + t (b − a)`, for `t` in `lo..hi`, is cut away: one stretch, as the
    /// removed part is convex.
    pub fn removed_span(&self, a: Vec3, b: Vec3, lo: f32, hi: f32) -> Option<(f32, f32)> {
        let (mut t0, mut t1) = (lo, hi);
        for (o, n) in self.planes() {
            let fa = (a - *o).dot(*n) - ON_PLANE;
            let df = (b - a).dot(*n);
            if df.abs() < 1e-12 {
                if fa <= 0.0 {
                    return None;
                }
                continue;
            }
            let t = -fa / df;
            if df > 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
        (t0 < t1).then_some((t0, t1))
    }
}

/// The part of a convex polygon on one side of a plane: the removed side (`removed`) or the
/// kept one.
fn half(pts: &[Vec3], (o, n): (Vec3, Vec3), removed: bool) -> Vec<Vec3> {
    let mut out = Vec::new();
    let side = |p: Vec3| {
        let d = (p - o).dot(n) - ON_PLANE;
        if removed { -d } else { d }
    };
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let (da, db) = (side(a), side(b));
        if da <= 0.0 {
            out.push(a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            out.push(a.lerp(b, da / (da - db)));
        }
    }
    out
}

/// The section of each tab, and the tab whose Section view dialog is open.
#[derive(Resource, Debug, Default)]
pub struct SectionViews {
    pub per: HashMap<ElementId, SectionState>,
    pub dialog: Option<ElementId>,
}

/// The active tab's section as drawn: the cut, the parts it leaves whole and the caps of the
/// parts it cuts.
#[derive(Resource, Default)]
pub struct SectionClip {
    pub plane: Option<Cut>,
    pub excluded: Vec<PartId>,
    pub caps: Arc<Caps>,
    key: Option<Vec<CapKey>>,
    pending: Vec<(CapKey, cadrs_core::rebuild::PendingJob<PlaneCaps>)>,
    cache: Vec<(CapKey, Arc<PlaneCaps>)>,
    preview_cache: Vec<(CapKey, Arc<PlaneCaps>)>,
    displayed: Option<(Vec<CapKey>, Cut, Vec<usize>)>,
}

/// One plane's caps as the kernel works them out, per part.
type PlaneCaps = Vec<(PartId, Cap)>;

/// A cap as drawn: its part, its plane's removed-side normal, its triangles and its outlines
/// (only where the other planes remove too).
#[derive(Debug, Clone)]
pub struct CapPiece {
    pub part: PartId,
    pub normal: Vec3,
    pub triangles: Vec<[Vec3; 3]>,
    pub loops: Vec<Vec<Vec3>>,
}

/// The caps of the parts cut.
pub type Caps = Vec<CapPiece>;

/// What the cap meshes were built for: the caps and the parts' generation.
type CapMeshKey = (usize, u64);

/// What the material uniforms were set from.
type ShadingKey = (
    Option<[[u32; 4]; MAX_PLANES]>,
    bool,
    // The dark scheme (the hidden-line modes' face grey).
    bool,
    bool,
    usize,
    [u32; 4],
    [u32; 4],
);

impl SectionClip {
    /// The caps are being worked out.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The cut a part is drawn with: none for the parts the section leaves whole.
    pub fn clip_for(&self, part: PartId) -> Option<Cut> {
        self.plane.filter(|_| !self.excluded.contains(&part))
    }
}

/// What a plane's caps were worked out for: the tab, the parts' generation, the plane
/// (unoriented: both sides of a plane have the same caps), to 1 µm, and the parts cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CapKey(ElementId, u64, [i64; 4], u64);

const CACHED_PLANES: usize = 8;

fn cap_key(element: ElementId, generation: u64, origin: Vec3, normal: Vec3, parts: u64) -> CapKey {
    let mut n = normal.normalize();
    let first = [n.x, n.y, n.z]
        .into_iter()
        .find(|c| c.abs() > 1e-6)
        .unwrap_or(1.0);
    if first < 0.0 {
        n = -n;
    }
    let d = n.dot(origin);
    let q = |v: f32| (v as f64 * 1e6).round() as i64;
    CapKey(
        element,
        generation,
        [q(n.x), q(n.y), q(n.z), (d as f64 * 1e3).round() as i64],
        parts,
    )
}

/// The active tab has a section.
pub fn active(world: &World) -> bool {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
    else {
        return false;
    };
    world
        .resource::<SectionViews>()
        .per
        .get(&el)
        .is_some_and(|s| !s.planes.is_empty())
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
    let Some(el) = modeling_tab(world) else {
        return;
    };
    let views = world.resource::<SectionViews>();
    if views.dialog == Some(el) {
        return;
    }
    if views.per.get(&el).is_some_and(|s| !s.planes.is_empty()) {
        exit(world);
        return;
    }
    let picked = world
        .resource::<Selection>()
        .0
        .iter()
        .rev()
        .find_map(|p| resolve(world, *p));
    open_with(world, picked);
}

/// Ends the active tab's section.
pub fn exit(world: &mut World) {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
    else {
        return;
    };
    close_dialog(world);
    world.resource_mut::<SectionViews>().per.remove(&el);
}

/// Opens the dialog on the active tab, with `picked` as its plane if given (a new section).
pub fn open_with(world: &mut World, picked: Option<SectionCutPlane>) {
    let Some(el) = modeling_tab(world) else {
        return;
    };
    let mut views = world.resource_mut::<SectionViews>();
    let state = views.per.entry(el).or_default();
    if let Some(p) = picked {
        *state = SectionState::with(p);
    }
    state.field = SectionField::Planes;
    views.dialog = Some(el);
    show_references(world);
    set_pick_filter(world);
}

/// The pick filter for the dialog's active field: planes, planar faces and plane features for
/// the section planes; any face for the items.
fn set_pick_filter(world: &mut World) {
    let Some(el) = world.resource::<SectionViews>().dialog else {
        return;
    };
    let field = world
        .resource::<SectionViews>()
        .per
        .get(&el)
        .map(|s| s.field)
        .unwrap_or_default();
    let kind = *world.resource::<ActiveKind>();
    let planes = world.resource::<PlanesVisible>().0;
    let filter = match (field, kind) {
        (SectionField::Items, _) => PickFilter {
            faces: true,
            ..PickFilter::none()
        },
        (SectionField::Planes, ActiveKind::Assembly) => PickFilter {
            faces: true,
            planar_only: true,
            ..PickFilter::none()
        },
        (SectionField::Planes, _) => PickFilter {
            planes,
            faces: true,
            planar_only: true,
            plane_features: true,
            ..PickFilter::none()
        },
    };
    world.resource_mut::<PickFilterOverride>().0 = Some(filter);
}

/// While the dialog is open its planes' references (planes, sketches, faces) are the selection,
/// highlighted in the list and the view (P3E.3a judge); closing the dialog deselects them.
fn show_references(world: &mut World) {
    let Some(el) = world.resource::<SectionViews>().dialog else {
        return;
    };
    let want: Vec<Pick> = world
        .resource::<SectionViews>()
        .per
        .get(&el)
        .map(|s| {
            s.planes
                .iter()
                .filter_map(|p| reference_pick(p.reference))
                .collect()
        })
        .unwrap_or_default();
    let mut sel = world.resource_mut::<Selection>();
    if sel.0 != want {
        sel.0 = want;
    }
}

/// The view's right-click menu (a plane, a face, empty space): Section view… cut by the plane
/// or planar face right-clicked, or the dialog with no plane yet (a curved face, empty space);
/// with a section on, Exit section view.
pub fn open_for_pick(world: &mut World, pick: Option<Pick>) {
    if active(world) && world.resource::<SectionViews>().dialog.is_none() {
        exit(world);
        return;
    }
    let picked = pick.and_then(|p| resolve(world, p));
    open_with(world, picked);
}

/// A feature's menu (IR5.5): cut by the feature's plane (a plane feature, a sketch's plane).
pub fn open_for_feature(world: &mut World, f: FeatureId) {
    let picked = resolve(world, Pick::Feature(f));
    open_with(world, picked);
}

/// An assembly instance's or triad's menu (A3.3, X15): cut through the instance's middle by
/// the Front plane. The instance is no longer selected (its highlight would stay on the cut).
pub fn open_for_instance(world: &mut World, parts: &[PartId]) {
    world.resource_mut::<Selection>().0.clear();
    let cache = world.resource::<PartCache>();
    let pts: Vec<Vec3> = cache
        .parts
        .iter()
        .filter(|p| {
            parts
                .iter()
                .any(|q| *q == p.id || (q.index == 0 && q.feature == p.id.feature))
        })
        .flat_map(|p| {
            p.solid
                .positions
                .iter()
                .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
        })
        .collect();
    let Some(first) = parts.first().copied() else {
        return;
    };
    let Some((lo, hi)) = pts.iter().fold(None, |acc: Option<(Vec3, Vec3)>, p| {
        Some(acc.map_or((*p, *p), |(lo, hi)| (lo.min(*p), hi.max(*p))))
    }) else {
        return;
    };
    let plane = SectionCutPlane::new(
        SectionRef::Instance(first),
        "Front plane".into(),
        (lo + hi) / 2.0,
        PlaneKind::Front.normal(),
    );
    open_with(world, Some(plane));
}

/// A picked plane, planar face, plane feature or sketch as a section plane, its point moved to
/// the middle of the parts (the gizmo and the turning axis are there).
fn resolve(world: &World, pick: Pick) -> Option<SectionCutPlane> {
    let cache = world.resource::<PartCache>();
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let (reference, label, origin, normal) = match pick {
        Pick::Plane(k) => (
            SectionRef::Plane(k),
            format!("{} plane", k.name()),
            Vec3::ZERO,
            k.normal(),
        ),
        Pick::Face(part, name) => {
            let p = cache.part(part)?;
            let f = p.solid.face(&name)?;
            f.plane?;
            // The mesh's normal (outward), not the frame's.
            let n = p
                .solid
                .indices
                .get(3 * f.first_triangle)
                .and_then(|&i| p.solid.normals.get(i as usize))
                .map(|n| v3(*n))?;
            let o = f.loops.first()?.first().map(|q| v3(*q))?;
            let doc = world.get_resource::<ActiveDocument>()?;
            let label = crate::parts::pick_label(
                doc.active_element().map(|e| e.features()).unwrap_or(&[]),
                cache,
                pick,
            )
            .unwrap_or_else(|| "Face".into());
            (SectionRef::Face(part, name), label, o, n)
        }
        Pick::Feature(f) => {
            let doc = world.get_resource::<ActiveDocument>()?;
            let feature = doc.active_element()?.feature(f)?;
            let frame = match cache.planes.get(&f) {
                Some(frame) => *frame,
                None => feature.sketch()?.plane?.frame(),
            };
            (
                SectionRef::Feature(f),
                feature.name.clone(),
                v3(frame.origin),
                v3(frame.normal()),
            )
        }
        _ => return None,
    };
    let n = normal.normalize_or_zero();
    // The point on the plane nearest the middle of the parts.
    let origin = match world.resource::<ShownBounds>().bounds {
        Some((lo, hi)) => {
            let c = (lo + hi) / 2.0;
            c - n * (c - origin).dot(n)
        }
        None => origin,
    };
    Some(SectionCutPlane::new(reference, label, origin, n))
}

/// The selection a section reference stands for (none for an instance's middle).
fn reference_pick(r: SectionRef) -> Option<Pick> {
    match r {
        SectionRef::Plane(k) => Some(Pick::Plane(k)),
        SectionRef::Face(p, f) => Some(Pick::Face(p, f)),
        SectionRef::Feature(f) => Some(Pick::Feature(f)),
        SectionRef::Instance(_) => None,
    }
}

fn close_dialog(world: &mut World) {
    let Some(el) = world.resource_mut::<SectionViews>().dialog.take() else {
        return;
    };
    world.resource_mut::<PickFilterOverride>().0 = None;
    // The planes picked for it are no longer selected.
    let refs: Vec<Pick> = world
        .resource::<SectionViews>()
        .per
        .get(&el)
        .map(|s| {
            s.planes
                .iter()
                .filter_map(|p| reference_pick(p.reference))
                .collect()
        })
        .unwrap_or_default();
    world
        .resource_mut::<Selection>()
        .0
        .retain(|p| !refs.contains(p));
}

/// The bottom-right Section view tool, and the assembly toolbar's.
fn on_tool(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity)
        .is_ok_and(|n| matches!(n.as_str(), "view-section" | "section-view"))
    {
        commands.queue(toggle);
    }
}

/// ✓: the section stays (with no plane picked, there is none).
pub fn accept(world: &mut World) {
    let Some(el) = world.resource::<SectionViews>().dialog else {
        return;
    };
    close_dialog(world);
    let mut views = world.resource_mut::<SectionViews>();
    if views.per.get(&el).is_none_or(|s| s.planes.is_empty()) {
        views.per.remove(&el);
    }
}

/// The dialog's picks in the view and the feature list (the pick filter is overridden, so they
/// don't reach the selection on their own). In Section planes a plane, planar face, plane
/// feature or sketch is added as a plane (picked again, it is taken away); in the items field a
/// part is added or taken away.
fn take_picks(
    mut picks: MessageReader<PickRequest>,
    views: Res<SectionViews>,
    mut commands: Commands,
) {
    if views.dialog.is_none() {
        picks.clear();
        return;
    }
    let got: Vec<Pick> = picks.read().filter_map(|p| p.0).collect();
    if got.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        let Some(el) = world.resource::<SectionViews>().dialog else {
            return;
        };
        for pick in got {
            let field = world
                .resource::<SectionViews>()
                .per
                .get(&el)
                .map(|s| s.field)
                .unwrap_or_default();
            match field {
                SectionField::Planes => {
                    let Some(plane) = resolve(world, pick) else {
                        continue;
                    };
                    let mut views = world.resource_mut::<SectionViews>();
                    let s = views.per.entry(el).or_default();
                    if let Some(i) = s.planes.iter().position(|p| p.reference == plane.reference) {
                        s.planes.remove(i);
                        s.active = s.planes.len().saturating_sub(1);
                    } else if s.planes.len() < MAX_PLANES {
                        s.planes.push(plane);
                        s.active = s.planes.len() - 1;
                    }
                }
                SectionField::Items => {
                    let Some(part) = pick.part() else { continue };
                    let mut views = world.resource_mut::<SectionViews>();
                    let s = views.per.entry(el).or_default();
                    match s.items.iter().position(|p| *p == part) {
                        Some(i) => {
                            s.items.remove(i);
                        }
                        None => s.items.push(part),
                    }
                }
            }
        }
        show_references(world);
    });
}

fn on_accept(
    ev: On<FeatureDialogAccept>,
    q: Query<(), With<SectionDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(
    ev: On<FeatureDialogCancel>,
    q: Query<(), With<SectionDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(exit);
    }
}

/// Changes the open dialog's section.
fn edit(world: &mut World, f: impl FnOnce(&mut SectionState)) {
    let mut views = world.resource_mut::<SectionViews>();
    let Some(el) = views.dialog else { return };
    if let Some(s) = views.per.get_mut(&el) {
        f(s);
    }
}

/// The active plane's Offset and Angle fields.
fn on_number(ev: On<NumberFieldCommit>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity).map(|n| n.as_str().to_string()) else {
        return;
    };
    if name != "section-offset" && name != "section-angle" && name != "section-angle-2" {
        return;
    }
    let text = ev.text.clone();
    commands.queue(move |world: &mut World| {
        if name == "section-offset" {
            let units = world.resource::<crate::WorkspaceUnits>().0;
            let Some(mm) = units.parse_length(&text) else {
                return;
            };
            edit(world, |s| {
                if let Some(p) = s.active_plane_mut() {
                    p.set_offset(mm as f32);
                }
            });
        } else {
            let Some(deg) = parse_degrees(&text) else {
                return;
            };
            edit(world, |s| {
                if let Some(p) = s.active_plane_mut() {
                    p.set_tilt(if name == "section-angle-2" { 1 } else { 0 }, deg);
                }
            });
        }
    });
}

/// An angle typed in degrees ("25", "25 deg", "25°").
fn parse_degrees(text: &str) -> Option<f32> {
    let t = text
        .trim()
        .trim_end_matches("deg")
        .trim_end_matches('°')
        .trim();
    let v: f32 = t.parse().ok()?;
    v.is_finite().then_some(v.clamp(-89.0, 89.0))
}

/// The tabs, the lists' fields and ✕, and a plane row clicked (the one the offset, angle and
/// gizmo act on).
fn on_tab(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("section-mode") {
        return;
    }
    let mode = if ev.index == 0 {
        SectionMode::Exclude
    } else {
        SectionMode::Include
    };
    commands.queue(move |world: &mut World| edit(world, |s| s.mode = mode));
}

fn on_list_activate(ev: On<SelectionListActivate>, q: Query<&Name>, mut commands: Commands) {
    let field = match q.get(ev.entity).map(|n| n.as_str()) {
        Ok("section-planes") => SectionField::Planes,
        Ok("section-items") => SectionField::Items,
        _ => return,
    };
    commands.queue(move |world: &mut World| {
        edit(world, |s| s.field = field);
        set_pick_filter(world);
    });
}

fn on_list_remove(ev: On<SelectionListRemove>, q: Query<&Name>, mut commands: Commands) {
    let list = match q.get(ev.entity).map(|n| n.as_str()) {
        Ok("section-planes") => SectionField::Planes,
        Ok("section-items") => SectionField::Items,
        _ => return,
    };
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        edit(world, |s| match list {
            SectionField::Planes if i < s.planes.len() => {
                s.planes.remove(i);
                s.active = s.active.min(s.planes.len().saturating_sub(1));
            }
            SectionField::Items if i < s.items.len() => {
                s.items.remove(i);
            }
            _ => {}
        });
        show_references(world);
    });
}

fn on_plane_row(
    ev: On<PointerClick>,
    q_item: Query<&SelectionListItem>,
    q_name: Query<&Name>,
    mut commands: Commands,
) {
    let Ok(item) = q_item.get(ev.entity) else {
        return;
    };
    if q_name.get(item.list).map(|n| n.as_str()) != Ok("section-planes") {
        return;
    }
    let i = item.index;
    commands.queue(move |world: &mut World| {
        edit(world, |s| {
            if i < s.planes.len() {
                s.active = i;
            }
        })
    });
}

fn flip(world: &mut World) {
    edit(world, |s| {
        if let Some(p) = s.active_plane_mut() {
            p.flip = !p.flip;
        }
    });
}

/// Changes the default plane chooser (opening it if need be: a section with no plane yet has
/// no state).
fn edit_picker(world: &mut World, f: impl FnOnce(&mut Option<DefaultPlanePicker>)) {
    let mut views = world.resource_mut::<SectionViews>();
    let Some(el) = views.dialog else { return };
    f(&mut views.per.entry(el).or_default().picker);
}

/// ✓ in the chooser: the default plane becomes a section plane (the active one; picked again,
/// it is only made active and given the chooser's side).
pub fn add_default_plane(world: &mut World) {
    let Some(el) = world.resource::<SectionViews>().dialog else {
        return;
    };
    let Some(picker) = world
        .resource::<SectionViews>()
        .per
        .get(&el)
        .and_then(|s| s.picker)
    else {
        return;
    };
    let Some(mut plane) = resolve(world, Pick::Plane(picker.kind)) else {
        return;
    };
    plane.flip = picker.flip;
    let mut views = world.resource_mut::<SectionViews>();
    let s = views.per.entry(el).or_default();
    s.picker = None;
    s.field = SectionField::Planes;
    match s.planes.iter().position(|p| p.reference == plane.reference) {
        Some(i) => {
            s.planes[i].flip = picker.flip;
            s.active = i;
        }
        None if s.planes.len() < MAX_PLANES => {
            s.planes.push(plane);
            s.active = s.planes.len() - 1;
        }
        None => {}
    }
    show_references(world);
}

/// The chooser's flip, ✓ and ✕.
fn on_chooser_button(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    match q.get(a.entity).map(Name::as_str) {
        Ok("section-default-plane-flip") => commands.queue(|w: &mut World| {
            edit_picker(w, |p| {
                if let Some(p) = p {
                    p.flip = !p.flip;
                }
            })
        }),
        Ok("section-default-plane-ok") => commands.queue(add_default_plane),
        Ok("section-default-plane-cancel") => {
            commands.queue(|w: &mut World| edit_picker(w, |p| *p = None))
        }
        _ => {}
    }
}

/// The chooser's plane.
fn on_chooser_kind(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(Name::as_str) != Ok("section-default-plane-kind") {
        return;
    }
    let Some(kind) = PlaneKind::ALL.get(ev.index).copied() else {
        return;
    };
    commands.queue(move |w: &mut World| {
        edit_picker(w, |p| {
            if let Some(p) = p {
                p.kind = kind;
            }
        })
    });
}

#[derive(Component)]
struct SectionDialog(String);

/// A plane's row: "Section plane 2 (Right plane)", its angle when turned.
fn plane_row(i: usize, p: &SectionCutPlane) -> String {
    let angle = if p.tilts != [0.0; 2] {
        format!(", {} / {} deg", fmt_deg(p.tilts[0]), fmt_deg(p.tilts[1]))
    } else {
        String::new()
    };
    format!("Section plane {} ({}{angle})", i + 1, p.label)
}

fn axis_name(a: Vec3) -> String {
    for (v, label) in [(Vec3::X, "X"), (Vec3::Y, "Y"), (Vec3::Z, "Z")] {
        if a.abs_diff_eq(v, 1e-4) {
            return label.into();
        }
        if a.abs_diff_eq(-v, 1e-4) {
            return format!("−{label}");
        }
    }
    format!("{:.2}, {:.2}, {:.2}", a.x, a.y, a.z)
}

fn fmt_deg(a: f32) -> String {
    let r = (a * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

fn section_dialog(
    t: &Theme,
    s: &SectionState,
    part_names: Vec<String>,
    offset: String,
) -> impl Bundle {
    let tb = t.clone();
    let rows: Vec<String> = s
        .planes
        .iter()
        .enumerate()
        .map(|(i, p)| plane_row(i, p))
        .collect();
    let active = s.active_plane();
    let flipped = active.is_some_and(|p| p.flip);
    let angle = active
        .map(|p| format!("{} deg", fmt_deg(p.angle())))
        .unwrap_or_else(|| "0 deg".into());
    let angle2 = active
        .map(|p| format!("{} deg", fmt_deg(p.tilts[1])))
        .unwrap_or_else(|| "0 deg".into());
    let axis_labels: [String; 2] = std::array::from_fn(|i| {
        active
            .map(|p| axis_name(p.rotation_axis(i)))
            .unwrap_or_default()
    });
    let has_plane = active.is_some();
    let (mode, field, picker) = (s.mode, s.field, s.picker);
    FeatureDialog::new("section-dialog")
        .title("Section view")
        .valid(!s.planes.is_empty())
        .width(260.0)
        .body(move |b| {
            let strip = TabStrip::new("section-mode")
                .compact()
                .tab("Exclude")
                .tab("Include");
            b.spawn(
                strip
                    .selected(if mode == SectionMode::Exclude { 0 } else { 1 })
                    .build(&tb),
            );
            // Section planes, and the button for a default plane (issue #122).
            b.spawn(Node {
                align_items: AlignItems::Start,
                column_gap: Val::Px(2.0),
                margin: UiRect::vertical(Val::Px(2.0)),
                ..default()
            })
            .with_children(|r| {
                r.spawn(
                    SelectionList::new("section-planes")
                        .placeholder("Section planes")
                        .items(rows)
                        .active(field == SectionField::Planes)
                        .build(&tb),
                )
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.flex_grow = 1.0;
                    n.min_width = Val::Px(0.0);
                });
                r.spawn((
                    IconButton::new("section-default-plane", "plane")
                        .icon_size(18.0)
                        .tooltip("Section by a default plane: Top, Front or Right")
                        .build(&tb),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            edit_picker(w, |p| {
                                *p = match p {
                                    Some(_) => None,
                                    None => Some(DefaultPlanePicker::default()),
                                }
                            })
                        })
                    }),
                ))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.width = Val::Px(24.0);
                    n.height = Val::Px(24.0);
                    n.flex_shrink = 0.0;
                });
            });
            // The chooser: Top, Front or Right, the side, ✓ and ✕ (Onshape's).
            if let Some(p) = picker {
                b.spawn((
                    Name::new("section-default-plane-chooser"),
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(2.0),
                        margin: UiRect::vertical(Val::Px(2.0)),
                        ..default()
                    },
                ))
                .with_children(|r| {
                    let mut kinds = Select::new("section-default-plane-kind").width(Val::Px(130.0));
                    for k in PlaneKind::ALL {
                        kinds = kinds.option(k.name(), true);
                    }
                    r.spawn(kinds.selected(p.kind.index()).build(&tb))
                        .entry::<Node>()
                        .and_modify(|mut n| n.flex_grow = 1.0);
                    for (name, icon, tip) in [
                        (
                            "section-default-plane-flip",
                            crate::extrude_dialog::flip_icon(p.flip),
                            "Flip the side removed",
                        ),
                        ("section-default-plane-ok", "check", "Add the section plane"),
                        ("section-default-plane-cancel", "close", "Cancel"),
                    ] {
                        r.spawn((
                            IconButton::new(name, icon)
                                .icon_size(16.0)
                                .tooltip(tip)
                                .build(&tb),
                            observe(on_chooser_button),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| {
                            n.width = Val::Px(22.0);
                            n.height = Val::Px(22.0);
                            n.flex_shrink = 0.0;
                        });
                    }
                });
            }
            let placeholder = if mode == SectionMode::Exclude {
                "Items to exclude"
            } else {
                "Items to include"
            };
            b.spawn(
                SelectionList::new("section-items")
                    .placeholder(placeholder)
                    .items(part_names)
                    .active(field == SectionField::Items)
                    .build(&tb),
            )
            .entry::<Node>()
            .and_modify(|mut n| {
                n.flex_grow = 0.0;
                n.margin = UiRect::vertical(Val::Px(2.0));
            });
            if !has_plane {
                return;
            }
            b.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(2.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(
                    NumberField::new("section-offset", "Offset")
                        .text(offset)
                        .label_width(52.0)
                        .build(&tb),
                )
                .entry::<Node>()
                .and_modify(|mut n| n.flex_grow = 1.0);
                r.spawn((
                    IconButton::new("section-flip", crate::extrude_dialog::flip_icon(flipped))
                        .icon_size(20.0)
                        .tooltip("Flip the side removed")
                        .build(&tb),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(flip)),
                ))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.width = Val::Px(22.0);
                    n.height = Val::Px(22.0);
                    n.flex_shrink = 0.0;
                });
            });
            b.spawn(
                NumberField::new("section-angle", format!("Tilt 1 ({})", axis_labels[0]))
                    .text(angle)
                    .label_width(92.0)
                    .build(&tb),
            );
            b.spawn(
                NumberField::new("section-angle-2", format!("Tilt 2 ({})", axis_labels[1]))
                    .text(angle2)
                    .label_width(92.0)
                    .build(&tb),
            );
            b.spawn((
                Name::new("section-rotation-axis"),
                Text::new("Blue: tilt 1 · Orange: tilt 2\nLocal axes · order: tilt 1, then tilt 2"),
                TextFont {
                    font_size: bevy::text::FontSize::Px(12.0),
                    ..default()
                },
                TextColor(tb.foreground),
                Pickable::IGNORE,
            ));
        })
        .build(t)
}

#[allow(clippy::too_many_arguments)]
fn sync_dialog(
    views: Res<SectionViews>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    arrow: Res<SectionArrow>,
    q_dialog: Query<(Entity, &SectionDialog, &FeatureDialogState)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let open = views
        .dialog
        .filter(|el| doc.as_ref().and_then(|d| d.active) == Some(*el));
    let Some(el) = open else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    if !q_dialog.is_empty() && (arrow.drag.is_some() || arrow.turn.is_some()) {
        return;
    }
    let s = views.per.get(&el).cloned().unwrap_or_default();
    let offset = units.0.with_unit(
        s.active_plane().map_or(0.0, |p| p.offset) as f64,
        cadrs_sketch::units::Quantity::Length,
    );
    let names: Vec<String> = s
        .items
        .iter()
        .map(|p| {
            cache
                .part(*p)
                .map(|q| q.name.clone())
                .unwrap_or_else(|| "Part".into())
        })
        .collect();
    let key = format!(
        "{:?}|{:?}|{:?}|{}|{:?}|{}|{:?}",
        s.planes,
        s.active,
        s.mode,
        offset,
        names,
        s.field == SectionField::Planes,
        s.picker
    );
    if let Some((e, d, _)) = q_dialog.iter().next() {
        if d.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else {
        return;
    };
    let d = commands
        .spawn((
            SectionDialog(key),
            DespawnOnExit(AppState::Document),
            section_dialog(&theme, &s, names, offset),
        ))
        .id();
    commands.entity(area).add_child(d);
}

// ---------------------------------------------------------------------------------------------
// The cut and the caps

/// Works out each plane's caps on the kernel thread (cached per plane and parts cut), then
/// keeps the part of each that the other planes remove too.
fn compute_caps(
    views: Res<SectionViews>,
    arrow: Res<SectionArrow>,
    doc: Option<Res<ActiveDocument>>,
    mut cache: ResMut<PartCache>,
    mut clip: ResMut<SectionClip>,
) {
    // Always drain the one outstanding job, even when the section was closed or changed.
    // Dropping its receiver would not cancel work already on the shared kernel thread.
    let mut done = Vec::new();
    // (Only touched mutably when there is something to change: the parts and the section follow
    // these resources' changes.)
    if !clip.pending.is_empty() {
        clip.pending.retain(|(key, job)| match job.poll() {
            Some(result) => {
                done.push((*key, Arc::new(result.unwrap_or_default())));
                false
            }
            None => true,
        });
    }
    for (key, caps) in done {
        clip.cache.retain(|(k, _)| k.0 != key.0 || k.1 == key.1);
        clip.cache.push((key, caps));
        while clip.cache.len() > CACHED_PLANES {
            clip.cache.remove(0);
        }
    }
    let el = doc.as_ref().and_then(|d| d.active);
    let state = el.and_then(|el| views.per.get(&el));
    let cut = state.and_then(|s| s.cut());
    if clip.plane != cut {
        clip.plane = cut;
    }
    let excluded = match (state, cut) {
        (Some(s), Some(_)) => s.excluded(cache.shown().map(|p| p.id)),
        _ => Vec::new(),
    };
    if clip.excluded != excluded {
        clip.excluded = excluded.clone();
    }
    let Some(cut) = cut else {
        if !clip.caps.is_empty() {
            clip.caps = Arc::default();
        }
        if clip.key.is_some() || clip.displayed.is_some() || !clip.preview_cache.is_empty() {
            clip.key = None;
            clip.displayed = None;
            clip.preview_cache.clear();
        }
        if cache.section.is_some() {
            cache.section = None;
        }
        return;
    };
    let (Some(doc), Some(el)) = (doc, el) else {
        return;
    };
    let cut_parts: Vec<PartId> = cache
        .shown()
        .map(|p| p.id)
        .filter(|p| !excluded.contains(p))
        .collect();
    let parts_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        cut_parts.hash(&mut h);
        h.finish()
    };
    let keys: Vec<CapKey> = cut
        .planes()
        .iter()
        .map(|(o, n)| cap_key(el, cache.generation, *o, *n, parts_hash))
        .collect();
    clip.preview_cache.retain(|(key, _)| keys.contains(key));
    let mut raw = Vec::new();
    let mut missing = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        if let Some((_, caps)) = clip.cache.iter().find(|(k, _)| k == key) {
            raw.push(caps.clone());
            continue;
        }
        missing.push((i, *key));
        let caps = if let Some((_, caps)) = clip.preview_cache.iter().find(|(k, _)| k == key) {
            caps.clone()
        } else {
            let (origin, normal) = cut.planes()[i];
            let caps = Arc::new(
                cache
                    .shown()
                    .filter(|p| p.kind == cadrs_core::PartKind::Solid && cut_parts.contains(&p.id))
                    .filter_map(|p| {
                        let cap = preview::cap(&p.solid, origin, normal);
                        (!cap.triangles.is_empty()).then_some((p.id, cap))
                    })
                    .collect::<PlaneCaps>(),
            );
            clip.preview_cache.push((*key, caps.clone()));
            caps
        };
        raw.push(caps);
    }
    let displayed = (
        keys.clone(),
        cut,
        raw.iter().map(|caps| Arc::as_ptr(caps) as usize).collect(),
    );
    if clip.displayed.as_ref() != Some(&displayed) {
        let caps = combine(&raw, &cut);
        cache.section = Some(crate::parts::SectionPick {
            cut,
            excluded: excluded.clone(),
            caps: caps
                .iter()
                .flat_map(|c| c.triangles.iter().copied())
                .collect(),
        });
        clip.caps = Arc::new(caps);
        clip.displayed = Some(displayed);
    }
    clip.key = missing.is_empty().then_some(keys);
    // Keep only the latest requested position: no queue of intermediate drag samples.
    // An already running job may finish, but its result cannot replace a different cut.
    if missing.is_empty()
        || cache.rebuilding
        || arrow.drag.is_some()
        || arrow.turn.is_some()
        || !clip.pending.is_empty()
    {
        return;
    }
    let items: Vec<SectionItem> = if cache.assembly.is_some() {
        let Some(asm) = doc.active_element().and_then(|e| e.assembly_model()) else {
            return;
        };
        cadrs_core::assembly::interference::items(&doc.doc, asm)
            .into_iter()
            .filter(|it| cut_parts.contains(&it.view_part))
            .map(|it| SectionItem {
                view_part: it.view_part,
                features: it.features,
                part: it.part,
                pose: it.pose,
            })
            .collect()
    } else {
        let Some((_, features, _)) = cache.settled() else {
            return;
        };
        let features = features.to_vec();
        cut_parts
            .iter()
            .map(|p| SectionItem {
                view_part: *p,
                features: features.clone(),
                part: *p,
                pose: cadrs_core::assembly::Pose::IDENTITY,
            })
            .collect()
    };
    let (i, key) = missing[0];
    let (o, n) = cut.planes()[i];
    let plane = SectionPlane {
        origin: o.as_dvec3().to_array(),
        normal: n.as_dvec3().to_array(),
    };
    clip.pending
        .push((key, cadrs_core::section::caps(items, plane)));
}

/// The caps as drawn: each plane's, kept only where the other planes remove too (two planes
/// meet in a wedge, each cap stopping at the other plane).
fn combine(raw: &[Arc<PlaneCaps>], cut: &Cut) -> Caps {
    let v = |p: [f32; 3]| Vec3::from_array(p);
    let mut out = Vec::new();
    for (i, caps) in raw.iter().enumerate() {
        let Some(&(_, normal)) = cut.planes().get(i) else {
            continue;
        };
        let others = cut.without(i);
        for (part, cap) in caps.iter() {
            let (triangles, loops) = match others {
                None => (
                    cap.triangles.iter().map(|t| t.map(v)).collect(),
                    cap.loops
                        .iter()
                        .map(|l| l.iter().map(|p| v(*p)).collect())
                        .collect(),
                ),
                Some(o) => {
                    let mut tris = Vec::new();
                    for t in &cap.triangles {
                        let mut poly: Vec<Vec3> = t.iter().map(|p| v(*p)).collect();
                        for plane in o.planes() {
                            poly = half(&poly, *plane, true);
                        }
                        for k in 1..poly.len().saturating_sub(1) {
                            tris.push([poly[0], poly[k], poly[k + 1]]);
                        }
                    }
                    let loops = cap
                        .loops
                        .iter()
                        .flat_map(|l| {
                            let mut pts: Vec<Vec3> = l.iter().map(|p| v(*p)).collect();
                            if let Some(f) = pts.first().copied() {
                                pts.push(f);
                            }
                            inside_pieces(&pts, &o)
                        })
                        .collect();
                    (tris, loops)
                }
            };
            if !triangles.is_empty() {
                out.push(CapPiece {
                    part: *part,
                    normal,
                    triangles,
                    loops,
                });
            }
        }
    }
    out
}

/// The pieces of a polyline inside a cut's removed part.
fn inside_pieces(pts: &[Vec3], cut: &Cut) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    for w in pts.windows(2) {
        let Some((t0, t1)) = cut.removed_span(w[0], w[1], 0.0, 1.0) else {
            continue;
        };
        let (a, b) = (w[0].lerp(w[1], t0), w[0].lerp(w[1], t1));
        match out.last_mut() {
            Some(l) if l.last().is_some_and(|q| q.distance(a) < 1e-4) => l.push(b),
            _ => out.push(vec![a, b]),
        }
    }
    out
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
    let key = (Arc::as_ptr(&clip.caps) as usize, cache.generation);
    if last.as_ref() == Some(&key) && (clip.plane.is_some() || q.is_empty()) {
        return;
    }
    *last = Some(key);
    for e in &q {
        commands.entity(e).try_despawn();
    }
    if clip.plane.is_none() {
        return;
    }
    let material = mat
        .get_or_insert_with(|| {
            materials.add(PartShading {
                cap: true,
                ..PartShading::new(false, false)
            })
        })
        .clone();
    for (k, cap) in clip.caps.iter().enumerate() {
        let Some(p) = cache.part(cap.part) else {
            continue;
        };
        if !cache.shown().any(|s| s.id == cap.part) {
            continue;
        }
        let base =
            crate::parts::FaceBase::of(cadrs_core::appearance::part_appearance(p, &cache.props));
        let tint = cache.tints.get(&cap.part).copied().unwrap_or(base);
        // A hair toward the eye, so a plane drawn on the cut (the picked Front plane) doesn't
        // fight the cap for depth.
        let n = cap.normal;
        let lift = n * 0.05;
        let positions: Vec<[f32; 3]> = cap
            .triangles
            .iter()
            .flatten()
            .map(|p| (*p + lift).to_array())
            .collect();
        let count = positions.len();
        let color = [
            tint.rgb[0] / 255.0,
            tint.rgb[1] / 255.0,
            tint.rgb[2] / 255.0,
            1.0,
        ];
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); count])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; count])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count])
        .with_inserted_indices(Indices::U32((0..count as u32).collect()));
        commands.spawn((
            Name::new(format!(
                "section-cap-{}-{k}",
                p.name.to_lowercase().replace(' ', "-")
            )),
            CapMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            DespawnOnExit(AppState::Document),
        ));
    }
}

/// How far apart a cap's hatch lines are on screen (px).
const HATCH_PX: f32 = 7.0;

/// The caps' outlines, as part edges, and their hatching: thin diagonal lines across each
/// cap a constant distance apart on screen, as Onshape's.
#[derive(Default)]
struct HatchCache {
    caps: Arc<Caps>,
    view: Option<crate::camera::ViewState>,
    lines: Vec<Vec<(Vec3, Vec3)>>,
}

fn draw_caps(
    clip: Res<SectionClip>,
    arrow: Res<SectionArrow>,
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    mut cached: Local<HatchCache>,
    mut edges: Gizmos<PartEdgeGizmos>,
) {
    if clip.plane.is_none() {
        return;
    }
    let moving = arrow.drag.is_some() || arrow.turn.is_some();
    if !moving && (!Arc::ptr_eq(&cached.caps, &clip.caps) || cached.view != Some(view.view)) {
        cached.lines = clip.caps.iter().map(|cap| hatch(cap, &view.view)).collect();
        cached.caps = clip.caps.clone();
        cached.view = Some(view.view);
    }
    let outline = view.view.render.edges() || view.view.render.line_drawing();
    for (i, cap) in clip.caps.iter().enumerate() {
        if !cache.shown().any(|p| p.id == cap.part) {
            continue;
        }
        if outline {
            for l in &cap.loops {
                edges.linestrip(l.iter().copied(), Color::srgb_u8(0x14, 0x14, 0x14));
            }
        }
        if !moving {
            for &(a, b) in &cached.lines[i] {
                edges.line(a, b, Color::srgba_u8(0x14, 0x14, 0x14, 0xa0));
            }
        }
    }
}

/// A cap's hatch lines: across its triangles at 45° to the plane's axes, `HATCH_PX` apart on
/// screen, lifted off the cap a hair.
pub fn hatch(cap: &CapPiece, view: &crate::camera::ViewState) -> Vec<(Vec3, Vec3)> {
    let n = cap.normal.normalize_or_zero();
    let u = if n.cross(Vec3::Z).length() > 1e-3 {
        Vec3::Z.cross(n).normalize()
    } else {
        Vec3::X
    };
    let v = n.cross(u).normalize_or_zero();
    let across = (u - v).normalize_or_zero();
    let px_per_mm = view
        .project_vector(across)
        .length()
        .max(view.project_vector(u).length())
        .max(1e-6);
    let mut step = HATCH_PX / px_per_mm;
    let lift = n * 0.1;
    // At most a few thousand lines a cap.
    let extent = cap
        .triangles
        .iter()
        .flatten()
        .map(|p| p.dot(across))
        .fold((f32::MAX, f32::MIN), |(a, b), w| (a.min(w), b.max(w)));
    if (extent.1 - extent.0) / step > 4000.0 {
        step = (extent.1 - extent.0) / 4000.0;
    }
    let mut out = Vec::new();
    for t in &cap.triangles {
        let w = t.map(|p| p.dot(across));
        let (lo, hi) = (w[0].min(w[1]).min(w[2]), w[0].max(w[1]).max(w[2]));
        let mut k = (lo / step).ceil();
        while k * step <= hi {
            let c = k * step;
            let mut hits = Vec::with_capacity(2);
            for i in 0..3 {
                let (a, b, wa, wb) = (t[i], t[(i + 1) % 3], w[i], w[(i + 1) % 3]);
                if (wa - c) * (wb - c) <= 0.0 && (wa - wb).abs() > 1e-9 {
                    hits.push(a.lerp(b, (c - wa) / (wb - wa)));
                }
            }
            if hits.len() >= 2 && hits[0].distance(hits[1]) > 1e-6 {
                out.push((hits[0] + lift, hits[1] + lift));
            }
            k += 1.0;
        }
    }
    out
}

/// The parts' material uniforms: the cut, the hidden-line modes' white faces and the
/// Translucent mode's opacity.
fn sync_shading(
    clip: Res<SectionClip>,
    view: Res<ViewportView>,
    kind: Res<ActiveKind>,
    analysis: Res<crate::analysis::ShadingAnalysis>,
    mut materials: ResMut<Assets<PartShading>>,
    mut last: Local<Option<ShadingKey>>,
) {
    let modeling = matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let cut = clip.plane.filter(|_| modeling);
    let mode = view.view.render;
    // P3E.3b: the analysis tools' face colouring.
    let (analysis_v, pull) = analysis.uniforms();
    let bands = analysis.band_colors();
    let planes = cut.map_or([Vec4::ZERO; MAX_PLANES], |c| c.uniforms());
    let key = (
        cut.map(|_| planes.map(|p| p.to_array().map(f32::to_bits))),
        !mode.shaded(),
        cadrs_ui::dark_in_force(),
        mode.translucent(),
        materials.len(),
        analysis_v.to_array().map(f32::to_bits),
        pull.to_array().map(f32::to_bits),
    );
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let ids: Vec<AssetId<PartShading>> = materials.ids().collect();
    for id in ids {
        let Some(m) = materials.get(id) else { continue };
        let params = PartShadingParams {
            clip: if m.cap || m.unclipped {
                [Vec4::ZERO; MAX_PLANES]
            } else {
                planes
            },
            style: Vec4::new(
                if mode.shaded() { 0.0 } else { 1.0 },
                if m.translucent {
                    TRANSLUCENT_ALPHA
                } else {
                    1.0
                },
                // The hidden-line modes' faces: white paper, or the dark viewport's grey.
                if cadrs_ui::dark_in_force() { 0.27 } else { 1.0 },
                0.0,
            ),
            analysis: if m.cap { Vec4::ZERO } else { analysis_v },
            pull,
            bands,
        };
        if m.params != params
            && let Some(mut m) = materials.get_mut(id)
        {
            m.params = params;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The section planes while the dialog is open, the active one's gizmo, and the planes it cuts

/// The section plane drawn while the dialog is open.
#[derive(Component)]
struct SectionPlaneQuad;

/// The box round the parts shown (world coordinates, assembly instances placed), worked out
/// once per rebuild.
#[derive(Resource, Debug, Default)]
pub struct ShownBounds {
    generation: Option<u64>,
    pub bounds: Option<(Vec3, Vec3)>,
}

fn track_bounds(cache: Res<PartCache>, mut b: ResMut<ShownBounds>) {
    if b.generation == Some(cache.generation) {
        return;
    }
    b.generation = Some(cache.generation);
    b.bounds = cache
        .shown()
        .flat_map(|p| {
            p.solid
                .positions
                .iter()
                .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
        })
        .fold(None, |acc: Option<(Vec3, Vec3)>, p| {
            Some(acc.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))))
        });
}

/// The rectangle a section plane is drawn as: its centre (on the cutting plane, round the
/// parts with a margin), its in-plane axes and half its sides.
fn section_square(p: &SectionCutPlane, bounds: &ShownBounds) -> Option<(Vec3, Vec3, Vec3, Vec2)> {
    let (origin, _) = p.plane();
    let (u, v) = p.axes();
    if u == Vec3::ZERO || v == Vec3::ZERO {
        return None;
    }
    let Some((lo, hi)) = bounds.bounds else {
        return Some((origin, u, v, Vec2::splat(crate::viewport::PLANE_HALF)));
    };
    let (mut a, mut b) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for k in 0..8 {
        let c = Vec3::new(
            if k & 1 == 0 { lo.x } else { hi.x },
            if k & 2 == 0 { lo.y } else { hi.y },
            if k & 4 == 0 { lo.z } else { hi.z },
        );
        let q = Vec2::new((c - origin).dot(u), (c - origin).dot(v));
        a = a.min(q);
        b = b.max(q);
    }
    let mid = (a + b) / 2.0;
    // A margin of a tenth of the larger side all round.
    let margin = (b - a).max_element() * 0.1;
    let half = ((b - a) / 2.0 + Vec2::splat(margin)).max(Vec2::splat(5.0));
    Some((origin + u * mid.x + v * mid.y, u, v, half))
}

/// The open dialog's section, in the active tab.
fn dialog_state<'a>(
    views: &'a SectionViews,
    doc: Option<&ActiveDocument>,
) -> Option<&'a SectionState> {
    let el = views
        .dialog
        .filter(|el| doc.and_then(|d| d.active) == Some(*el))?;
    views.per.get(&el)
}

/// The gizmo's centre: on the active plane, at the middle of the parts (the point the plane
/// turns about).
fn gizmo_centre(p: &SectionCutPlane) -> Vec3 {
    p.pivot
}

/// The section planes while the dialog is open: the active one a translucent orange rectangle
/// with its outline (as a selected plane), the others outlined; each where it cuts.
#[allow(clippy::too_many_arguments)]
fn sync_section_plane(
    views: Res<SectionViews>,
    doc: Option<Res<ActiveDocument>>,
    bounds: Res<ShownBounds>,
    cache: Res<PartCache>,
    materials: Option<Res<crate::viewport::PlaneMaterials>>,
    theme: Res<Theme>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut q: Query<(Entity, &mut Transform), With<SectionPlaneQuad>>,
    mut outline: Gizmos<SectionPlaneGizmos>,
    mut commands: Commands,
) {
    // A plane picked as it is (no offset, not turned) is drawn by its own square, selected while
    // the dialog is open: no second rectangle on it.
    let own_square = |p: &SectionCutPlane| {
        p.offset == 0.0
            && p.turn == Quat::IDENTITY
            && (p.pivot - p.origin).dot(p.normal).abs() < 1e-5
            && match p.reference {
                SectionRef::Plane(_) => true,
                SectionRef::Feature(f) => cache.planes.contains_key(&f),
                _ => false,
            }
    };
    let state = dialog_state(&views, doc.as_deref());
    let active = state
        .and_then(|s| s.active_plane())
        .filter(|p| !own_square(p))
        .and_then(|p| section_square(p, &bounds));
    for (i, p) in state
        .map(|s| s.planes.as_slice())
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        if own_square(p) || state.is_some_and(|s| s.active.min(s.planes.len() - 1) == i) {
            continue;
        }
        let Some((c, u, v, h)) = section_square(p, &bounds) else {
            continue;
        };
        let (u, v) = (u * h.x, v * h.y);
        let corners = [c - u - v, c + u - v, c + u + v, c - u + v];
        for k in 0..4 {
            outline.line(
                corners[k],
                corners[(k + 1) % 4],
                Color::srgb_u8(0x96, 0xa9, 0xb8),
            );
        }
    }
    let (Some((c, u, v, h)), Some(m)) = (active, materials) else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let n = u.cross(v);
    let want = Transform {
        translation: c,
        rotation: Quat::from_mat3(&Mat3::from_cols(u, v, n)),
        scale: Vec3::new(h.x, h.y, 1.0),
    };
    match q.iter_mut().next() {
        Some((_, mut t)) => {
            if *t != want {
                *t = want;
            }
        }
        None => {
            let mesh = quad
                .get_or_insert_with(|| meshes.add(Rectangle::new(2.0, 2.0)))
                .clone();
            commands.spawn((
                Name::new("section-plane"),
                SectionPlaneQuad,
                Mesh3d(mesh),
                MeshMaterial3d(m.selected.clone()),
                want,
                DespawnOnExit(AppState::Document),
            ));
        }
    }
    let (u, v) = (u * h.x, v * h.y);
    let corners = [c - u - v, c + u - v, c + u + v, c - u + v];
    for i in 0..4 {
        outline.line(corners[i], corners[(i + 1) % 4], theme.selection_3d);
    }
}

/// The active plane's gizmo: the offset arrow and the rotation arc, where they are on screen,
/// and a drag in progress.
#[derive(Resource, Debug, Default)]
pub struct SectionArrow {
    /// The arrow's base and tip on screen.
    base_tip: Option<(Vec2, Vec2)>,
    /// The rotation arc on screen.
    arc: [Vec<Vec2>; 2],
    hovered: bool,
    arc_hovered: Option<usize>,
    pub drag: Option<SectionArrowDrag>,
    pub turn: Option<SectionTurnDrag>,
}

#[derive(Debug, Clone, Copy)]
pub struct SectionArrowDrag {
    start_offset: f32,
    /// The axis the plane moves along: a point on it (the plane's middle at the press) and the
    /// plane's current normal at the press (before Flip).
    axis: (Vec3, Vec3),
    /// Where the cursor ray met the axis at the press (mm along it).
    start_t: f32,
    /// Screen px per mm along the axis (the snap step).
    px_per_mm: f32,
}

/// A drag about the plane's local axis at the press, through `centre`.
/// Edge-on views use the projected handle tangent instead of a ray/plane intersection.
#[derive(Debug, Clone, Copy)]
pub struct SectionTurnDrag {
    index: usize,
    start_turn: Quat,
    start_screen: Vec2,
    screen_tangent: Option<Vec2>,
    centre: Vec3,
    axis: Vec3,
    /// The pointer's angle (radians) about the axis at the press, measured from `zero`.
    start_pointer: f32,
    zero: Vec3,
}

/// Where the ray `(o, d)` passes closest to the line through `p` along unit `n`: the distance
/// along the line from `p` (`None` when they are parallel).
pub fn ray_axis_param(o: Vec3, d: Vec3, p: Vec3, n: Vec3) -> Option<f32> {
    let w = p - o;
    let (a, b, c) = (n.dot(n), n.dot(d), d.dot(d));
    let (dn, dd) = (n.dot(w), d.dot(w));
    let den = a * c - b * b;
    if den.abs() < 1e-9 {
        return None;
    }
    // Line: p + t n; ray: o + s d. Minimise |p + t n − o − s d|².
    Some((b * dd - c * dn) / den)
}

/// The pointer's angle about `axis` through `centre`: where its ray meets the plane square to
/// the axis, measured from `zero` towards `axis × zero`.
fn pointer_angle(o: Vec3, d: Vec3, centre: Vec3, axis: Vec3, zero: Vec3) -> Option<f32> {
    let dn = d.dot(axis);
    if dn.abs() < 1e-6 {
        return None;
    }
    let q = o + d * ((centre - o).dot(axis) / dn) - centre;
    let y = axis.cross(zero);
    Some(q.dot(y).atan2(q.dot(zero)))
}

const ARROW_LEN: f32 = 64.0;
const ARC_FORWARD_PX: f32 = 24.0;

/// The translation arrow stays anchored to the section plane.
fn translation_arrow(
    p: &SectionCutPlane,
    view: &crate::camera::ViewState,
    hot: bool,
) -> Option<crate::manipulator::Arrow3d> {
    let (_, dir) = p.plane();
    let px = view.project_vector(dir).length();
    (px >= 0.05).then(|| crate::manipulator::Arrow3d {
        base: gizmo_centre(p),
        dir,
        length_px: ARROW_LEN,
        hot,
    })
}
/// The rotation arc: its radius (px) and half its sweep (degrees).
const ARC_PX: f32 = 60.0;
const ARC_HALF_DEG: f32 = 40.0;

/// The rotation handle starts along the removed-side normal, perpendicular to its local axis.
fn arc_zero(p: &SectionCutPlane, index: usize) -> Vec3 {
    let (_, n) = p.plane();
    let axis = p.handle_axis(index);
    (n - axis * n.dot(axis)).normalize_or_zero()
}

fn arc_radius(p: &SectionCutPlane, index: usize, view: &crate::camera::ViewState) -> f32 {
    let e = arc_zero(p, index);
    let tangent = p.handle_axis(index).cross(e);
    (ARC_PX + ARC_FORWARD_PX + index as f32 * 20.0)
        / view
            .project_vector(e)
            .length()
            .max(view.project_vector(tangent).length())
            .max(1e-6)
}

/// Arc perpendicular to a local rotation axis, carried with the section plane.
fn arc_points(p: &SectionCutPlane, index: usize, view: &crate::camera::ViewState) -> Vec<Vec3> {
    let e = arc_zero(p, index);
    let c = gizmo_centre(p);
    let r = arc_radius(p, index, view);
    (0..=24)
        .map(|k| {
            let a = (-ARC_HALF_DEG + 2.0 * ARC_HALF_DEG * k as f32 / 24.0).to_radians();
            c + (Quat::from_axis_angle(p.handle_axis(index), a) * e) * r
        })
        .collect()
}

/// Grabs and drags the gizmo: the arrow moves the active plane along its normal (snapped like
/// the extrude's depth, in the document's unit), the arc turns it about its axis (1° steps).
#[allow(clippy::too_many_arguments)]
fn section_arrow_pointer(
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    mut views: ResMut<SectionViews>,
    doc: Option<Res<ActiveDocument>>,
    mut arrow: ResMut<SectionArrow>,
    view: Res<ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    mut grab: ResMut<crate::assembly::ViewportGrab>,
    units: Res<crate::WorkspaceUnits>,
    rect: Res<crate::viewport::ViewportRect>,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton, PointerId};
    let el = views
        .dialog
        .filter(|el| doc.as_ref().and_then(|d| d.active) == Some(*el));
    let Some(el) = el else {
        inputs.clear();
        if arrow.drag.is_some()
            || arrow.turn.is_some()
            || arrow.hovered
            || arrow.arc_hovered.is_some()
        {
            arrow.drag = None;
            arrow.turn = None;
            arrow.hovered = false;
            arrow.arc_hovered = None;
        }
        return;
    };
    let near = |p: Vec2, (a, b): (Vec2, Vec2)| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        p.distance(a + ab * t) <= 8.0
    };
    let near_arc = |p: Vec2, arc: &[Vec2]| arc.windows(2).any(|w| near(p, (w[0], w[1])));
    let arc_hovered = (0..2).find(|&i| near_arc(drag.pointer(), &arrow.arc[i]));
    let hovered =
        arc_hovered.is_none() && arrow.base_tip.is_some_and(|bt| near(drag.pointer(), bt));
    if arrow.hovered != hovered {
        arrow.hovered = hovered;
    }
    if arrow.arc_hovered != arc_hovered {
        arrow.arc_hovered = arc_hovered;
    }
    let plane = views.per.get(&el).and_then(|s| s.active_plane()).cloned();
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                let Some(p) = plane.clone() else { continue };
                let (o, d) = view.view.ray(rect.offset(pos));
                if let Some(bt) = arrow.base_tip
                    && near(pos, bt)
                    && !(0..2).any(|i| near_arc(pos, &arrow.arc[i]))
                {
                    let normal = p.turned();
                    let dir_px = view.view.project_vector(normal);
                    let c = gizmo_centre(&p);
                    if dir_px.length() > 0.05
                        && let Some(t) = ray_axis_param(o, d, c, normal)
                    {
                        arrow.drag = Some(SectionArrowDrag {
                            start_offset: p.offset,
                            axis: (c, normal),
                            start_t: t,
                            px_per_mm: dir_px.length(),
                        });
                        // Not a click on what is under it.
                        grab.0 = true;
                    }
                } else if let Some(index) = (0..2).find(|&i| near_arc(pos, &arrow.arc[i])) {
                    let c = gizmo_centre(&p);
                    let axis = p.handle_axis(index);
                    let zero = arc_zero(&p, index);
                    let tangent = view.view.project_vector(axis.cross(zero))
                        * arc_radius(&p, index, &view.view);
                    let edge_on = view.view.back().dot(axis).abs() < 0.1;
                    let a = pointer_angle(o, d, c, axis, zero);
                    if (!edge_on && a.is_some()) || tangent.length() > 1e-3 {
                        arrow.turn = Some(SectionTurnDrag {
                            index,
                            start_turn: p.turn,
                            start_screen: pos,
                            screen_tangent: edge_on.then_some(tangent),
                            centre: c,
                            axis,
                            start_pointer: a.unwrap_or(0.0),
                            zero,
                        });
                        grab.0 = true;
                    }
                }
            }
            PointerAction::Move { .. } => {
                let (o, dir) = view.view.ray(rect.offset(pos));
                if let Some(d) = arrow.drag {
                    // The point of the normal axis under the cursor (the closest point between
                    // the cursor's ray and the axis), so the plane follows the pointer 1:1.
                    let Some(t) = ray_axis_param(o, dir, d.axis.0, d.axis.1) else {
                        continue;
                    };
                    let along = t - d.start_t;
                    // Snapped to a round step in the document's length unit.
                    let k = units.0.to_mm(1.0).max(1e-9);
                    let step = crate::extrude::snap_step(d.px_per_mm * k as f32);
                    let offset =
                        ((((d.start_offset + along) as f64 / k) / step).round() * step * k) as f32;
                    if let Some(p) = views.per.get_mut(&el).and_then(|s| s.active_plane_mut())
                        && (p.offset - offset).abs() > 1e-6
                    {
                        p.set_offset(offset);
                    }
                }
                if let Some(d) = arrow.turn {
                    let mut delta = if let Some(tangent) = d.screen_tangent {
                        ((pos - d.start_screen).dot(tangent) / tangent.length_squared())
                            .to_degrees()
                    } else {
                        let Some(a) = pointer_angle(o, dir, d.centre, d.axis, d.zero) else {
                            continue;
                        };
                        (a - d.start_pointer).to_degrees()
                    };
                    if d.screen_tangent.is_none() && delta > 180.0 {
                        delta -= 360.0;
                    }
                    if d.screen_tangent.is_none() && delta < -180.0 {
                        delta += 360.0;
                    }
                    // 1° steps.
                    if let Some(p) = views.per.get_mut(&el).and_then(|s| s.active_plane_mut()) {
                        p.rotate_local(d.start_turn, d.index, delta.round());
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) | PointerAction::Cancel => {
                arrow.drag = None;
                arrow.turn = None;
            }
            _ => {}
        }
    }
}

#[derive(Component)]
struct SectionArrowNode;

/// The rotation arc's angle readout ("25 deg"), beside the arc while the plane is turned.
#[derive(Component)]
struct SectionAngleLabel;

/// Places the gizmo at the active plane's middle: the offset arrow along the removed side's
/// normal (it turns round with Flip; the shared 3D drag arrow, [`crate::manipulator`]) and the
/// rotation arc round its axis, orange while hovered or dragged, with the angle beside it.
#[allow(clippy::too_many_arguments)]
fn place_section_arrow(
    views: Res<SectionViews>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    theme: Res<Theme>,
    mut arrow: ResMut<SectionArrow>,
    mut q: Query<(Entity, &mut crate::manipulator::Arrow3d), With<SectionArrowNode>>,
    mut q_label: Query<(Entity, &mut Node, &Children), With<SectionAngleLabel>>,
    mut q_text: Query<&mut Text>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut gizmos: Gizmos<SectionArcGizmos>,
    mut commands: Commands,
) {
    let plane = dialog_state(&views, doc.as_deref())
        .and_then(|s| s.active_plane())
        .cloned();
    let want = plane
        .as_ref()
        .and_then(|p| translation_arrow(p, &view.view, arrow.hovered || arrow.drag.is_some()));
    let placed = want.map(|a| {
        let (f, t) = crate::manipulator::screen_span(&a, &view.view);
        (rect.to_screen(f), rect.to_screen(t))
    });
    if arrow.base_tip != placed {
        arrow.base_tip = placed;
    }
    // Both handles use reference axes, with distinct colours and separated radii.
    let arcs: [Vec<Vec3>; 2] = std::array::from_fn(|i| {
        plane
            .as_ref()
            .map(|p| arc_points(p, i, &view.view))
            .unwrap_or_default()
    });
    let screens: [Vec<Vec2>; 2] = std::array::from_fn(|i| {
        arcs[i]
            .iter()
            .map(|p| rect.to_screen(view.view.project(*p)))
            .collect()
    });
    if arrow.arc != screens {
        arrow.arc = screens.clone();
    }
    for i in 0..2 {
        let hot = arrow.arc_hovered == Some(i) || arrow.turn.is_some_and(|d| d.index == i);
        let color = if hot {
            theme.selection_3d
        } else if i == 0 {
            Color::srgb_u8(0x3d, 0x6f, 0xd8)
        } else {
            Color::srgb_u8(0xd9, 0x7a, 0x16)
        };
        gizmos.linestrip(arcs[i].iter().copied(), color);
        if let (Some(p), Some(mid)) = (plane.as_ref(), arcs[i].get(arcs[i].len() / 2)) {
            let w = view.view.back().normalize_or_zero();
            let px = (ARC_PX + ARC_FORWARD_PX + i as f32 * 20.0) / arc_radius(p, i, &view.view);
            gizmos.circle(
                Isometry3d::new(*mid, Quat::from_rotation_arc(Vec3::Z, w)),
                5.0 / px,
                color,
            );
        }
    }
    let readout = |p: &SectionCutPlane| {
        format!(
            "{}: {} deg · {}: {} deg",
            axis_name(p.rotation_axis(0)),
            fmt_deg(p.tilts[0]),
            axis_name(p.rotation_axis(1)),
            fmt_deg(p.tilts[1])
        )
    };
    let index = arrow
        .turn
        .map(|d| d.index)
        .or(arrow.arc_hovered)
        .unwrap_or(0);
    let label = plane
        .as_ref()
        .filter(|p| p.tilts != [0.0; 2] || arrow.turn.is_some())
        .zip(screens[index].last().copied());
    match (label, q_label.iter_mut().next()) {
        (Some((p, at)), Some((_, mut node, children))) => {
            let local = at - rect.0.min;
            node.left = Val::Px(local.x + 8.0);
            node.top = Val::Px(local.y - 10.0);
            let text = readout(p);
            for c in children.iter() {
                if let Ok(mut t) = q_text.get_mut(c)
                    && t.0 != text
                {
                    t.0 = text.clone();
                }
            }
        }
        (Some((p, at)), None) => {
            if let Some(area) = q_area.iter().next() {
                let local = at - rect.0.min;
                let label = commands
                    .spawn((
                        Name::new("section-angle-label"),
                        SectionAngleLabel,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(local.x + 8.0),
                            top: Val::Px(local.y - 10.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme.background),
                        BorderColor::all(theme.primary),
                        Pickable::IGNORE,
                        DespawnOnExit(AppState::Document),
                        children![(
                            Text::new(readout(p)),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(theme.foreground),
                            Pickable::IGNORE
                        )],
                    ))
                    .id();
                commands.entity(area).add_child(label);
            }
        }
        (None, Some((e, ..))) => commands.entity(e).try_despawn(),
        (None, None) => {}
    }
    let Some(a) = want else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    match q.iter_mut().next() {
        Some((_, mut cur)) => {
            if *cur != a {
                *cur = a;
            }
        }
        None => {
            commands.spawn((
                crate::manipulator::arrow("section-offset-arrow", a),
                SectionArrowNode,
                DespawnOnExit(AppState::Document),
            ));
        }
    }
}

/// How far past a cutting plane (mm) a polygon or line still counts as on it, and is kept.
const ON_PLANE: f32 = 1e-3;

/// The kept part of a convex polygon, as convex pieces: the part each plane keeps that the
/// planes before it remove (they don't overlap).
pub fn clip_polygon(pts: &[Vec3], cut: Cut) -> Vec<Vec<Vec3>> {
    let mut out = Vec::new();
    let planes = cut.planes();
    for i in 0..planes.len() {
        let mut poly = half(pts, planes[i], false);
        for p in &planes[..i] {
            poly = half(&poly, *p, true);
        }
        if poly.len() >= 3 {
            out.push(poly);
        }
    }
    out
}

/// Draws a line, cut by the section if there is one.
pub fn clipped_line<T: GizmoConfigGroup>(
    g: &mut Gizmos<T>,
    clip: Option<Cut>,
    a: Vec3,
    b: Vec3,
    color: Color,
) {
    match clip {
        None => g.line(a, b, color),
        Some(cut) => {
            for piece in clip_polyline([a, b], cut) {
                g.line(piece[0], piece[piece.len() - 1], color);
            }
        }
    }
}

/// A sketch plane parallel to a one-plane section, on its removed side.
pub fn plane_removed(frame: &cadrs_sketch::PlaneFrame, cut: Cut) -> bool {
    let [(origin, normal)] = cut.planes() else {
        return false;
    };
    let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let n = v(frame.u).cross(v(frame.v)).normalize_or_zero();
    n.dot(*normal).abs() > 0.9999 && (v(frame.origin) - *origin).dot(*normal) > 1e-3
}

/// The Translucent render mode's opacity.
pub const TRANSLUCENT_ALPHA: f32 = 0.35;

/// The pieces of a polyline the section keeps, cut where it enters and leaves the removed part.
pub fn clip_polyline(pts: impl IntoIterator<Item = Vec3>, cut: Cut) -> Vec<Vec<Vec3>> {
    let pts: Vec<Vec3> = pts.into_iter().collect();
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    let flush = |cur: &mut Vec<Vec3>, out: &mut Vec<Vec<Vec3>>| {
        if cur.len() >= 2 {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        match cut.removed_span(a, b, 0.0, 1.0) {
            None => {
                if cur.is_empty() {
                    cur.push(a);
                }
                cur.push(b);
            }
            Some((t0, t1)) => {
                if t0 > 0.0 {
                    if cur.is_empty() {
                        cur.push(a);
                    }
                    cur.push(a.lerp(b, t0));
                }
                flush(&mut cur, &mut out);
                if t1 < 1.0 {
                    cur.push(a.lerp(b, t1));
                    cur.push(b);
                }
            }
        }
    }
    if pts.len() == 1 && !cut.removes(pts[0]) {
        cur.push(pts[0]);
    }
    flush(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #122: a default plane from the chooser, with nothing picked in the view (an
    /// Assembly shows no planes); chosen again, it is only made active with the new side.
    #[test]
    fn the_chooser_adds_a_default_plane_without_a_pick() {
        let el = ElementId::new();
        let mut world = World::new();
        world.init_resource::<PartCache>();
        world.init_resource::<ShownBounds>();
        world.init_resource::<Selection>();
        world.insert_resource(SectionViews {
            dialog: Some(el),
            ..default()
        });
        let choose = |world: &mut World, kind, flip| {
            edit_picker(world, |p| *p = Some(DefaultPlanePicker { kind, flip }));
            add_default_plane(world);
        };
        choose(&mut world, PlaneKind::Front, true);
        choose(&mut world, PlaneKind::Right, false);
        let s = world.resource::<SectionViews>().per[&el].clone();
        assert_eq!(s.planes.len(), 2);
        assert_eq!(s.planes[0].reference, SectionRef::Plane(PlaneKind::Front));
        assert!(s.planes[0].flip);
        assert_eq!(s.active, 1);
        assert!(s.picker.is_none());
        // The Front plane's references show selected, as a pick's would.
        assert_eq!(world.resource::<Selection>().0.len(), 2);
        choose(&mut world, PlaneKind::Front, false);
        let s = world.resource::<SectionViews>().per[&el].clone();
        assert_eq!((s.planes.len(), s.active), (2, 0));
        assert!(!s.planes[0].flip);
    }

    #[test]
    fn dragging_caps_follow_current_cut_and_preserve_holes_without_kernel_jobs() {
        let mut doc = ActiveDocument::new(cadrs_core::Document::new("Section preview"));
        let el = doc.doc.elements[0].id;
        doc.active = Some(el);
        let feature = FeatureId::new();
        let part = PartId::new(feature, 0);
        let mut cache = PartCache::default();
        cache.parts.push(cadrs_core::Part {
            id: part,
            feature,
            name: "Tube".into(),
            kind: cadrs_core::PartKind::Solid,
            palette: 0,
            solid: Arc::new(preview::tests::tube()),
            mass: None,
            features: vec![feature],
            source: None,
            derived: None,
        });
        let plane = SectionCutPlane::new(
            SectionRef::Plane(PlaneKind::Top),
            "Top".into(),
            Vec3::ZERO,
            Vec3::Z,
        );
        let mut views = SectionViews::default();
        views.dialog = Some(el);
        views.per.insert(el, SectionState::with(plane));
        let mut arrow = SectionArrow::default();
        arrow.drag = Some(SectionArrowDrag {
            start_offset: 0.0,
            axis: (Vec3::ZERO, Vec3::Z),
            start_t: 0.0,
            px_per_mm: 1.0,
        });
        let mut app = App::new();
        app.insert_resource(doc)
            .insert_resource(cache)
            .insert_resource(views)
            .insert_resource(arrow)
            .init_resource::<SectionClip>()
            .add_systems(Update, compute_caps);
        let mut obsolete = None;
        for step in 0..30 {
            app.world_mut()
                .resource_mut::<SectionViews>()
                .per
                .get_mut(&el)
                .unwrap()
                .planes[0]
                .set_offset(step as f32 * 0.02);
            app.update();
            let clip = app.world().resource::<SectionClip>();
            assert!(
                clip.pending.is_empty(),
                "dragging must not enqueue intermediate positions"
            );
            assert_eq!(clip.caps.len(), 1);
            let area: f32 = clip.caps[0]
                .triangles
                .iter()
                .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5)
                .sum();
            assert!((area - 12.0).abs() < 1e-4, "preview filled the hole");
            let (origin, normal) = clip.plane.unwrap().planes()[0];
            assert!(
                clip.caps[0]
                    .triangles
                    .iter()
                    .flatten()
                    .all(|p| (*p - origin).dot(normal).abs() < 1e-5)
            );
            if step == 0 {
                obsolete = Some(clip.preview_cache[0].0);
            }
        }
        let before = app.world().resource::<SectionClip>().caps.clone();
        app.world_mut()
            .resource_mut::<SectionClip>()
            .cache
            .push((obsolete.unwrap(), Arc::new(Vec::new())));
        app.update();
        assert!(
            Arc::ptr_eq(&before, &app.world().resource::<SectionClip>().caps),
            "obsolete results cannot replace the latest preview"
        );
        // Repeated frames reuse caps; exact results for the current plane replace the preview.
        let (key, exact) = app.world().resource::<SectionClip>().preview_cache[0].clone();
        app.world_mut()
            .resource_mut::<SectionClip>()
            .cache
            .push((key, exact));
        app.update();
        assert!(app.world().resource::<SectionClip>().key.is_some());
        app.world_mut()
            .resource_mut::<SectionViews>()
            .per
            .get_mut(&el)
            .unwrap()
            .items
            .push(part);
        app.update();
        assert!(
            app.world().resource::<SectionClip>().caps.is_empty(),
            "excluded parts must remain whole"
        );
        {
            let mut views = app.world_mut().resource_mut::<SectionViews>();
            let state = views.per.get_mut(&el).unwrap();
            state.items.clear();
            state.planes.push(SectionCutPlane::new(
                SectionRef::Plane(PlaneKind::Right),
                "Right".into(),
                Vec3::ZERO,
                Vec3::X,
            ));
        }
        app.update();
        let clip = app.world().resource::<SectionClip>();
        let top = clip
            .caps
            .iter()
            .find(|c| c.normal.dot(Vec3::Z) > 0.99)
            .unwrap();
        let area: f32 = top
            .triangles
            .iter()
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5)
            .sum();
        assert!(
            (area - 6.0).abs() < 0.01,
            "the other plane must clip the preview too"
        );
    }

    #[test]
    fn polylines_are_clipped_at_the_plane() {
        let plane = Cut::single(Vec3::new(0.0, 0.0, 5.0), Vec3::Z);
        // Up through the plane and back down: two pieces, cut at z = 5.
        let pts = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 10.0),
            Vec3::new(2.0, 0.0, 0.0),
        ];
        let pieces = clip_polyline(pts, plane);
        assert_eq!(pieces.len(), 2);
        // (within the on-plane tolerance, `ON_PLANE`)
        assert!((pieces[0][1] - Vec3::new(0.5, 0.0, 5.0)).length() < 2e-3);
        assert!((pieces[1][0] - Vec3::new(1.5, 0.0, 5.0)).length() < 2e-3);
        // All kept, all removed.
        assert_eq!(
            clip_polyline(pts, Cut::single(Vec3::new(0.0, 0.0, 20.0), Vec3::Z)),
            vec![pts.to_vec()]
        );
        assert!(clip_polyline(pts, Cut::single(Vec3::new(0.0, 0.0, -1.0), Vec3::Z)).is_empty());
    }

    #[test]
    fn two_planes_take_out_a_wedge() {
        // x > 0 and y > 0 removed: the quarter in the first quadrant.
        let cut = Cut::new(&[(Vec3::ZERO, Vec3::X), (Vec3::ZERO, Vec3::Y)]).unwrap();
        assert!(cut.removes(Vec3::new(1.0, 1.0, 0.0)));
        assert!(!cut.removes(Vec3::new(1.0, -1.0, 0.0)));
        assert!(!cut.removes(Vec3::new(-1.0, 1.0, 0.0)));
        // A line along y = 1 from x = −2 to 2: kept up to x = 0.
        let pieces = clip_polyline([Vec3::new(-2.0, 1.0, 0.0), Vec3::new(2.0, 1.0, 0.0)], cut);
        assert_eq!(pieces.len(), 1);
        assert!((pieces[0][1].x - 0.0).abs() < 2e-3, "{pieces:?}");
        // A 4 × 4 square round the origin keeps three quarters: area 12.
        let sq = [
            Vec3::new(-2.0, -2.0, 0.0),
            Vec3::new(2.0, -2.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(-2.0, 2.0, 0.0),
        ];
        let area: f32 = clip_polygon(&sq, cut)
            .iter()
            .map(|p| {
                (1..p.len() - 1)
                    .map(|k| (p[k] - p[0]).cross(p[k + 1] - p[0]).length() / 2.0)
                    .sum::<f32>()
            })
            .sum();
        assert!((area - 12.0).abs() < 0.05, "{area}");
    }

    #[test]
    fn rotation_keeps_the_translated_arrow_origin_fixed() {
        for flip in [false, true] {
            let mut p = SectionCutPlane::new(
                SectionRef::Plane(PlaneKind::Top),
                "Top".into(),
                Vec3::new(20.0, -10.0, 5.0),
                Vec3::Z,
            );
            p.flip = flip;
            p.set_offset(15.0);
            let pivot = gizmo_centre(&p);
            p.set_tilt(0, 35.0);
            p.set_tilt(1, -25.0);
            assert_eq!(
                gizmo_centre(&p),
                pivot,
                "typed angles must not orbit the starting point"
            );
            for index in 0..2 {
                let start = p.turn;
                for delta in [-10.0, 10.0, 20.0] {
                    p.rotate_local(start, index, delta);
                    assert_eq!(
                        gizmo_centre(&p),
                        pivot,
                        "arc drags must rotate about the arrow origin"
                    );
                    let (point, normal) = p.plane();
                    assert!((point - pivot - normal * PLANE_NUDGE).length() < 1e-5);
                }
            }
            let normal = p.turned();
            p.set_offset(20.0);
            let moved = gizmo_centre(&p);
            assert!((moved - pivot - normal * 5.0).length() < 1e-5);
            p.set_tilt(0, 0.0);
            p.set_tilt(1, 0.0);
            assert_eq!(
                gizmo_centre(&p),
                moved,
                "resetting angles must also keep the current pivot"
            );
        }
    }

    #[test]
    fn tilted_section_arrow_moves_only_on_its_visible_axis_including_flip() {
        use crate::camera::{StandardView, ViewState};
        use bevy::picking::pointer::{
            Location, PointerAction, PointerButton, PointerId, PointerInput,
        };
        for camera in [StandardView::Top, StandardView::Isometric] {
            for flip in [false, true] {
                let mut doc = ActiveDocument::new(cadrs_core::Document::new("Tilted arrow"));
                let el = doc.doc.elements[0].id;
                doc.active = Some(el);
                let mut p = SectionCutPlane::new(
                    SectionRef::Plane(PlaneKind::Top),
                    "Top".into(),
                    Vec3::ZERO,
                    Vec3::Z,
                );
                p.set_tilt(0, 30.0);
                p.set_tilt(1, 20.0);
                p.set_offset(5.0);
                p.flip = flip;
                let base = gizmo_centre(&p);
                let visible_axis = p.plane().1;
                let view = ViewState {
                    scale: 0.1,
                    ..ViewState::standard(camera)
                };
                let rendered = translation_arrow(&p, &view, false).unwrap();
                assert_eq!(
                    rendered.base, base,
                    "the arrow must stay anchored to the section plane"
                );
                let (first, tip) = crate::manipulator::screen_span(&rendered, &view);
                let press = (first + tip) * 0.5;
                let moved = press + view.project_vector(visible_axis * 10.0);
                let mut views = SectionViews::default();
                views.dialog = Some(el);
                views.per.insert(el, SectionState::with(p));
                let mut arrow = SectionArrow::default();
                arrow.base_tip = Some((first, tip));
                let mut app = App::new();
                app.add_message::<PointerInput>()
                    .insert_resource(doc)
                    .insert_resource(views)
                    .insert_resource(arrow)
                    .insert_resource(ViewportView { view, ..default() })
                    .insert_resource(crate::viewport::ViewportRect(Rect::new(
                        0.0, 0.0, 1000.0, 1000.0,
                    )))
                    .init_resource::<crate::viewport::ViewportDrag>()
                    .init_resource::<crate::assembly::ViewportGrab>()
                    .init_resource::<crate::WorkspaceUnits>()
                    .add_systems(Update, section_arrow_pointer);
                let window = app.world_mut().spawn_empty().id();
                let target =
                    bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                        .normalize(None)
                        .unwrap();
                let event = |position, action| {
                    PointerInput::new(
                        PointerId::Mouse,
                        Location {
                            target: target.clone(),
                            position,
                        },
                        action,
                    )
                };
                app.world_mut()
                    .write_message(event(press, PointerAction::Press(PointerButton::Primary)));
                app.update();
                assert!(app.world().resource::<SectionArrow>().drag.is_some());
                app.world_mut().write_message(event(
                    moved,
                    PointerAction::Move {
                        delta: moved - press,
                    },
                ));
                app.update();
                let views = app.world().resource::<SectionViews>();
                let plane = views.per[&el].active_plane().unwrap();
                let delta = gizmo_centre(plane) - base;
                assert!(
                    delta.cross(visible_axis).length() < 1e-4,
                    "arrow slid off its visible axis: {delta}"
                );
                assert!(
                    (delta.dot(visible_axis) - 10.0).abs() < 0.01,
                    "pointer movement must be one-to-one: {delta}"
                );
                assert!(
                    delta.z.abs() > 1.0,
                    "tilted arrow must not slide at constant Z"
                );
                app.world_mut()
                    .write_message(event(moved, PointerAction::Release(PointerButton::Primary)));
                app.update();
                assert!(app.world().resource::<SectionArrow>().drag.is_none());
                // Even if projected hit areas intersect, the blue arc takes the press.
                app.world_mut().resource_mut::<SectionArrow>().arc[0] =
                    vec![press - Vec2::X * 2.0, press + Vec2::X * 2.0];
                app.world_mut()
                    .write_message(event(press, PointerAction::Press(PointerButton::Primary)));
                app.update();
                let state = app.world().resource::<SectionArrow>();
                assert!(
                    state.drag.is_none(),
                    "the arrow must not steal rotation presses"
                );
                assert_eq!(state.turn.unwrap().index, 0);
            }
        }
    }

    #[test]
    fn the_arrow_drag_follows_the_pointer_one_to_one() {
        // An isometric view of a plane at z = 12 with its normal up: the cursor over the axis
        // point at z = 50 reads 38 mm along it, whatever the view's tilt.
        let view = crate::camera::ViewState::default();
        let c = Vec3::new(10.0, -5.0, 12.0);
        let target = c + Vec3::Z * 38.0;
        let (o, d) = view.ray(view.project(target));
        let t0 = ray_axis_param(
            view.ray(view.project(c)).0,
            view.ray(view.project(c)).1,
            c,
            Vec3::Z,
        )
        .unwrap();
        let t = ray_axis_param(o, d, c, Vec3::Z).unwrap();
        assert!((t - t0 - 38.0).abs() < 1e-2, "{}", t - t0);
        // Looking straight down the axis: no answer.
        assert!(ray_axis_param(Vec3::ZERO, Vec3::Z, Vec3::X, Vec3::Z).is_none());
    }

    #[test]
    fn rotation_arc_keeps_reference_axis_when_camera_changes() {
        use crate::camera::{StandardView, ViewState};
        for kind in [PlaneKind::Top, PlaneKind::Front, PlaneKind::Right] {
            let mut p = SectionCutPlane::new(
                SectionRef::Plane(kind),
                kind.name().into(),
                Vec3::ZERO,
                kind.normal(),
            );
            let fixed_axis = p.rotation_axis(0);
            p.set_angle(25.0);
            for camera in [
                StandardView::Top,
                StandardView::Front,
                StandardView::Right,
                StandardView::Isometric,
            ] {
                let view = ViewState::standard(camera);
                let c = gizmo_centre(&p);
                let points = arc_points(&p, 0, &view);
                assert_eq!(points.len(), 25);
                for point in points {
                    assert!(
                        (point - c).dot(fixed_axis).abs() < 1e-3,
                        "arc must stay perpendicular to its fixed axis"
                    );
                    assert!(view.project(point).is_finite());
                }
                assert_eq!(p.rotation_axis(0), fixed_axis);
                assert!((p.angle() - 25.0).abs() < 1e-3);
            }
            // A handle turn and a typed angle use the same reference axis and zero.
            let zero = arc_zero(&p, 0);
            let target = Quat::from_axis_angle(fixed_axis, 15f32.to_radians()) * zero;
            let angle = pointer_angle(
                target + fixed_axis * 100.0,
                -fixed_axis,
                Vec3::ZERO,
                fixed_axis,
                zero,
            )
            .unwrap();
            p.set_angle(p.angle() + angle.to_degrees());
            assert!((p.angle() - 40.0).abs() < 1e-3);
            assert_eq!(p.rotation_axis(0), fixed_axis);
        }
    }

    #[test]
    fn two_tilts_are_independent_and_have_a_consistent_rotation_order() {
        let mut planes = vec![
            SectionCutPlane::new(
                SectionRef::Plane(PlaneKind::Front),
                "Y plane".into(),
                Vec3::ZERO,
                Vec3::Y,
            ),
            SectionCutPlane::new(
                SectionRef::Plane(PlaneKind::Top),
                "Top".into(),
                Vec3::ZERO,
                Vec3::Z,
            ),
            SectionCutPlane::new(
                SectionRef::Feature(FeatureId::new()),
                "Inclined face".into(),
                Vec3::ZERO,
                Vec3::new(1.0, 2.0, 3.0),
            ),
        ];
        for p in &mut planes {
            let axes = [p.rotation_axis(0), p.rotation_axis(1)];
            assert!(axes[0].dot(axes[1]).abs() < 1e-6);
            for a in axes {
                assert!(a.dot(p.normal).abs() < 1e-6);
            }
            let mut other_order = p.clone();
            p.set_tilt(0, 30.0);
            p.set_tilt(1, -20.0);
            other_order.set_tilt(1, -20.0);
            other_order.set_tilt(0, 30.0);
            assert!(p.turn.abs_diff_eq(other_order.turn, 1e-6));
            assert_eq!(p.tilts, [30.0, -20.0]);
            let expected = Quat::from_axis_angle(axes[0], 30f32.to_radians())
                * Quat::from_axis_angle(axes[1], -20f32.to_radians());
            assert!(p.turned().abs_diff_eq(expected * p.normal, 1e-6));
            for i in 0..2 {
                let axis = p.handle_axis(i);
                assert!(
                    axis.dot(p.turned()).abs() < 1e-6,
                    "handles must stay in the section plane"
                );
                let before = p.turned();
                let mut changed = p.clone();
                changed.rotate_local(p.turn, i, 5.0);
                assert!(
                    changed.turned().abs_diff_eq(
                        Quat::from_axis_angle(axis, 5f32.to_radians()) * before,
                        1e-6
                    ),
                    "drag must follow the visible local axis"
                );
                let mut typed = p.clone();
                typed.set_tilt(0, changed.tilts[0]);
                typed.set_tilt(1, changed.tilts[1]);
                assert!(
                    typed.turn.abs_diff_eq(changed.turn, 1e-6),
                    "fields must reproduce a drag without hidden roll"
                );
                changed.rotate_local(p.turn, i, 5.0);
                assert!(
                    typed.turn.abs_diff_eq(changed.turn, 1e-6),
                    "pointer moves are relative to the press, not cumulative"
                );
                let view =
                    crate::camera::ViewState::standard(crate::camera::StandardView::Isometric);
                for point in arc_points(p, i, &view) {
                    assert!((point - gizmo_centre(p)).dot(axis).abs() < 1e-3);
                }
            }
            p.set_tilt(0, 0.0);
            p.set_tilt(1, 0.0);
            assert!(p.turn.abs_diff_eq(Quat::IDENTITY, 1e-6));
        }
    }

    #[test]
    fn local_drags_round_trip_angles_for_both_handed_frames_and_keep_limits() {
        for (kind, normal) in [(PlaneKind::Top, Vec3::Z), (PlaneKind::Front, Vec3::Y)] {
            let mut p =
                SectionCutPlane::new(SectionRef::Plane(kind), "Plane".into(), Vec3::ZERO, normal);
            for angles in [[0.0, 0.0], [-45.0, 40.0], [60.0, -30.0]] {
                p.set_tilt(0, angles[0]);
                p.set_tilt(1, angles[1]);
                for index in 0..2 {
                    let start = p.turn;
                    let mut changed = p.clone();
                    changed.rotate_local(start, index, 0.0);
                    assert!(changed.turn.abs_diff_eq(start, 1e-6));
                    for delta in [-5.0, 5.0] {
                        changed.rotate_local(start, index, delta);
                        let target =
                            Quat::from_axis_angle(p.handle_axis(index), delta.to_radians())
                                * p.turned();
                        assert!(changed.turned().abs_diff_eq(target, 1e-6));
                        assert!(changed.handle_axis(0).dot(changed.handle_axis(1)).abs() < 1e-6);
                    }
                    changed.rotate_local(start, index, 180.0);
                    assert!(
                        changed
                            .tilts
                            .iter()
                            .all(|a| a.is_finite() && a.abs() <= 89.0)
                    );
                    let finite = changed.clone();
                    changed.rotate_local(start, index, f32::NAN);
                    assert_eq!(changed, finite);
                }
            }
        }
    }

    #[test]
    fn a_turned_plane_tilts_about_its_axis() {
        // The Top plane turned 30° about its u axis (x): the removed side's normal leans to −y.
        let mut p = SectionCutPlane::new(
            SectionRef::Plane(PlaneKind::Top),
            "Top plane".into(),
            Vec3::ZERO,
            Vec3::Z,
        );
        p.set_offset(5.0);
        p.set_angle(30.0);
        let (o, n) = p.plane();
        assert!((o - (Vec3::Z * 5.0 + n * PLANE_NUDGE)).length() < 2e-3);
        assert!(
            (n - Vec3::new(0.0, -0.5, 3f32.sqrt() / 2.0)).length() < 1e-5,
            "{n}"
        );
        assert!((p.angle() - 30.0).abs() < 1e-3);
        assert_eq!(parse_degrees("25 deg"), Some(25.0));
        assert_eq!(parse_degrees("-10°"), Some(-10.0));
    }

    #[test]
    fn a_flipped_plane_shares_its_caps() {
        let el = ElementId::new();
        let o = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(
            cap_key(el, 4, o, Vec3::Z, 7),
            cap_key(el, 4, o, -Vec3::Z, 7)
        );
        assert_ne!(
            cap_key(el, 4, o, Vec3::Z, 7),
            cap_key(el, 4, o + Vec3::Z, Vec3::Z, 7)
        );
        let mut p = SectionCutPlane::new(
            SectionRef::Plane(PlaneKind::Top),
            "Top plane".into(),
            Vec3::ZERO,
            Vec3::Z,
        );
        p.set_offset(5.0);
        p.flip = true;
        let (o, n) = p.plane();
        assert_eq!(n, -Vec3::Z);
        // Nudged a hair into the removed side (−z).
        assert!(
            (o - Vec3::new(0.0, 0.0, 5.0 - PLANE_NUDGE)).length() < 1e-6,
            "{o}"
        );
        assert_eq!(SectionState::default().cut(), None);
    }
}
