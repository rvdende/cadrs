//! The main camera and the Part Studio / Assembly viewport.
//!
//! - Orthographic, **Z-up** camera driven by a [`ViewState`] (see [`crate::camera`]): right-drag
//!   orbits, middle-drag and Ctrl+right-drag pan, the wheel zooms toward the cursor. `N` turns
//!   the view normal to the selected plane and `Shift+1`…`Shift+7` pick the standard views,
//!   animated (screenshots skip to the end).
//! - The default planes Top (XY), Front (XZ) and Right (YZ) as translucent pale-blue squares with
//!   thin blue outlines and in-plane blue labels at their corners; the origin as a small ringed
//!   dot. Hovering a plane (in the viewport, or its row in the feature list) turns its outline
//!   orange; clicking selects it.
//! - Each tab keeps its own view. Assemblies show only the origin and an axis triad.
//!
//! Input comes from `bevy_picking`'s `PointerInput` messages (real mouse and the scenario
//! harness alike). The viewport only reacts while the pointer is over the `viewport-area` UI
//! node, so panels and menus on top of it keep their clicks.

use cadrs_ui::ShortcutKey;
use std::collections::HashMap;

use crate::GizmoGroups;
use crate::ground_grid::{GroundGrid, GroundGridMaterial, GroundGridParams, GroundGridPlugin};
use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::{HoverMap, Hovered};
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::{UiGlobalTransform, UiTransform};
use cadrs_core::{ElementId, FeatureId, PartId};
use cadrs_sketch::PlaneRef;
use cadrs_ui::{FinishAnimations, RenderSurface, Selected, Theme};

use crate::camera::{self, StandardView, Turn, ViewState, ease_in_out, ease_out, ray_square};
use crate::{ActiveDocument, AppState};

pub struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Last,
            occluded_camera_when_drawn.after(bevy::gizmos::GizmoMeshSystems),
        );
        // The views shade their own (no Bevy lights), but Bevy's shadow-caster check queues a
        // command every frame that builds a query over every archetype (thousands, with the UI).
        use bevy::ecs::schedule::SystemCondition;
        app.configure_sets(
            PostUpdate,
            bevy::light::SimulationLightSystems::CheckLightVisibility.run_if(
                any_with_component::<DirectionalLight>
                    .or_else(any_with_component::<PointLight>)
                    .or_else(any_with_component::<SpotLight>),
            ),
        );
        app.add_plugins(GroundGridPlugin)
            .add_systems(
                PreUpdate,
                alias_backdrop.after(bevy::picking::PickingSystems::Hover),
            )
            .init_resource::<GridVisible>()
            .init_resource::<PlanesVisible>()
            .init_resource::<ViewportView>()
            .init_resource::<ArrowHold>()
            .init_resource::<ViewportRect>()
            .init_resource::<DialogInset>()
            .init_resource::<PlaneHighlight>()
            .init_resource::<HoverOverride>()
            .init_resource::<ExtraHighlight>()
            .init_resource::<PickFilterOverride>()
            .init_resource::<Selection>()
            .init_resource::<FeatureRowClick>()
            .init_resource::<ViewportDrag>()
            .init_resource::<ActiveKind>()
            .gizmo_group::<HighlightGizmos>()
            .gizmo_group::<HoverGizmos>()
            .gizmo_group::<EdgeOnGizmos>()
            .add_systems(
                PostUpdate,
                flag_pending_work.run_if(in_state(AppState::Document)),
            )
            .add_message::<PickRequest>()
            .add_observer(no_light_clusters)
            .add_systems(
                Startup,
                (spawn_main_camera, configure_gizmos, cpu_light_clusters),
            )
            .add_systems(OnEnter(AppState::Document), spawn_scene)
            .add_systems(OnExit(AppState::Document), reset_viewport)
            .add_systems(
                Update,
                recolor_planes.run_if(resource_changed::<cadrs_ui::ActiveTheme>),
            )
            .add_systems(
                Update,
                (
                    track_active_element,
                    fit_assembly_when_built,
                    track_viewport_rect,
                    track_dialog_inset,
                    viewport_pointer,
                    apply_pick_requests.run_if(in_state(crate::sketch::PartStudioMode::Modeling)),
                    view_shortcuts,
                    arrow_keys_held,
                    animate_view,
                    update_hover,
                    apply_view_to_camera,
                )
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                (
                    sync_scene_visibility,
                    style_planes,
                    draw_plane_edges,
                    sync_grid,
                    fit_grid,
                    sync_plane_rows,
                )
                    .after(apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                PostUpdate,
                (place_plane_labels, place_origin_marker)
                    .before(bevy::ui::UiSystems::Layout)
                    .run_if(in_state(AppState::Document)),
            );
    }
}

/// The camera that renders the 3D viewport.
#[derive(Component)]
pub struct MainCamera;

/// A second camera with the same view that draws after [`MainCamera`]: hover and selection
/// outlines (on [`OVERLAY_LAYER`], so no translucent plane is blended over them) and then the
/// UI on top.
#[derive(Component)]
pub struct OverlayCamera;

/// The render layer of highlight outlines, drawn by the [`OverlayCamera`].
pub const OVERLAY_LAYER: usize = 5;

/// The render layer of what parts hide but no translucent plane is blended over: the sketches
/// shown in the Part Studio (not the one being edited) and the Extrude dialog's regions, as in
/// Onshape. Drawn between the main pass and [`OVERLAY_LAYER`], testing against the parts'
/// depth.
pub const OCCLUDED_LAYER: usize = 6;

/// Which default planes are shown (Top, Front, Right). P shows or hides all of them, and each
/// plane's row in the feature list has its own eye toggle (S1.4). It is a view setting, like
/// Onshape's, not a document edit.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanesVisible(pub [bool; 3]);

impl Default for PlanesVisible {
    fn default() -> Self {
        Self([true; 3])
    }
}

impl PlanesVisible {
    /// True if plane `k` is shown.
    pub fn shows(&self, k: PlaneKind) -> bool {
        self.0[k.index()]
    }

    /// True if any plane is shown.
    pub fn any(&self) -> bool {
        self.0.iter().any(|v| *v)
    }

    pub fn set(&mut self, k: PlaneKind, shown: bool) {
        self.0[k.index()] = shown;
    }

    /// P: hides every plane when any is shown, else shows them all.
    pub fn toggle_all(&mut self) {
        let show = !self.any();
        self.0 = [show; 3];
    }
}

/// Whether the ground grid (on the Top plane) is shown: on by default.
#[derive(Resource)]
pub struct GridVisible(pub bool);

impl Default for GridVisible {
    fn default() -> Self {
        Self(true)
    }
}

/// Marks the UI node the 3D view shows through (between the panels). Its rect is the
/// viewport: the view is centered in it and pointer input over it drives the camera.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ViewportArea;

/// How far (logical px) an open feature dialog covers the viewport from its left edge (0 with
/// none, or while sketching): zoom to fit frames the scene in the rest.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct DialogInset(pub f32);

/// The viewport's rect in logical pixels (y down), from [`ViewportArea`].
#[derive(Resource, Debug, Clone, Copy)]
pub struct ViewportRect(pub Rect);

impl Default for ViewportRect {
    fn default() -> Self {
        Self(Rect::new(226.0, 70.0, 1600.0, 971.0))
    }
}

impl ViewportRect {
    /// Offset of a screen position from the viewport center.
    pub fn offset(&self, screen: Vec2) -> Vec2 {
        screen - self.0.center()
    }

    pub fn to_screen(&self, offset: Vec2) -> Vec2 {
        self.0.center() + offset
    }
}

/// An animated change of view.
#[derive(Debug, Clone, Copy)]
pub struct ViewAnimation {
    pub from: ViewState,
    pub to: ViewState,
    /// The turn from `from`'s orientation to `to`'s, the way the view goes.
    pub turn: Turn,
    pub elapsed: f32,
    pub duration: f32,
    /// It took over from one in progress: it starts at speed ([`ease_out`]) instead of from
    /// rest.
    pub moving: bool,
}

impl ViewAnimation {
    /// How far along it is (0..=1), eased.
    fn progress(&self) -> f32 {
        let t = self.elapsed / self.duration;
        if self.moving {
            ease_out(t)
        } else {
            ease_in_out(t)
        }
    }
}

/// Seconds a view change (N, Shift+7, view cube) takes.
pub const VIEW_ANIMATION_TIME: f32 = 0.35;

/// The current view, one per tab.
#[derive(Resource, Debug, Clone, Default)]
pub struct ViewportView {
    pub view: ViewState,
    pub animation: Option<ViewAnimation>,
    /// Saved views of the other tabs.
    pub per_element: HashMap<ElementId, ViewState>,
    pub element: Option<ElementId>,
    /// An Assembly tab shown for the first time before its parts were built: the view it started
    /// with, fitted to the instances once they are there ([`fit_assembly_when_built`]) unless the
    /// view moved meanwhile.
    pub fit_pending: Option<(ElementId, ViewState)>,
    /// The point a drag orbits about, when not the focus (the PCB Studio's selected
    /// components' centre, [`crate::pcb::view`]).
    pub pivot: Option<Vec3>,
}

impl ViewportView {
    /// Animates to `to`, its orientation turned the shortest way.
    pub fn animate_to(&mut self, to: ViewState) {
        let to = self.keeping_mode(to);
        if self.view.approx_eq(&to) {
            return;
        }
        let turn = Turn::between(self.view.rotation(), to.rotation());
        self.start(to, turn, VIEW_ANIMATION_TIME);
    }

    /// Turns the view by `yaw` and `pitch` degrees about the screen axes (as
    /// [`ViewState::rotate_by`]: the arrow keys, the view cube's arrows), on from the turn in
    /// progress: presses quicker than their turns add up (four of 90° turn it all the way
    /// round), and the view goes on turning without stopping in between (#91).
    pub fn turn_by(&mut self, yaw: f32, pitch: f32) {
        self.turn_by_in(yaw, pitch, VIEW_ANIMATION_TIME);
    }

    /// [`Self::turn_by`], the view getting there in `duration` seconds.
    pub fn turn_by_in(&mut self, yaw: f32, pitch: f32, duration: f32) {
        let from = self.target();
        let mut to = from;
        to.rotate_by(yaw, pitch);
        let step = Turn::between(from.rotation(), to.rotation());
        let turn = self.turn_left().then(step);
        self.start(self.keeping_mode(to), turn, duration);
    }

    /// What's left of the turn in progress ([`Turn::NONE`] when the view isn't moving).
    pub fn turn_left(&self) -> Turn {
        self.animation.map_or(Turn::NONE, |a| Turn {
            angle: a.turn.angle * (1.0 - a.progress()),
            ..a.turn
        })
    }

    /// `to` with the tab's render mode and projection (P3E.3a): a change of view keeps them
    /// (`crate::view_options` sets them).
    fn keeping_mode(&self, to: ViewState) -> ViewState {
        ViewState {
            render: self.view.render,
            perspective: self.view.perspective,
            ..to
        }
    }

    /// Animates from the view now to `to` in `duration` seconds, its orientation turned by
    /// `turn`.
    fn start(&mut self, to: ViewState, turn: Turn, duration: f32) {
        self.animation = Some(ViewAnimation {
            from: self.view,
            to,
            turn,
            elapsed: 0.0,
            duration,
            moving: self.animation.is_some(),
        });
    }

    /// The view the current animation ends at (or the current view).
    pub fn target(&self) -> ViewState {
        self.animation.map(|a| a.to).unwrap_or(self.view)
    }

    /// Moves the animation on by `dt` seconds (to its end with `finish`).
    pub fn advance(&mut self, dt: f32, finish: bool) {
        let Some(mut a) = self.animation else {
            return;
        };
        a.elapsed += dt;
        if finish || a.elapsed >= a.duration {
            self.view = a.to;
            self.animation = None;
        } else {
            self.view = a.from.lerp_turn(&a.to, a.turn, a.progress());
            self.animation = Some(a);
        }
    }
}

/// What the active tab is.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveKind {
    #[default]
    PartStudio,
    Assembly,
    /// A Drawing tab (P3C): the 2D sheet of `crate::drawing` replaces the 3D view.
    Drawing,
    /// A PCB Studio tab (P3H.3): the active board in 3D (`crate::pcb::view`), no planes.
    PcbStudio,
    /// A Render Studio tab (P3F.6): the path-traced view of `crate::render_ui` replaces it.
    Render,
    /// A CAM Studio tab: its components' parts, stock and toolpaths (`crate::cam`), no planes.
    CamStudio,
    /// A Variable Studio tab: its table (`crate::variable_studio_ui`) replaces the 3D view.
    VariableStudio,
}

impl ActiveKind {
    /// True for a tab with no 3D view (a drawing's sheet, a render).
    pub fn is_flat(self) -> bool {
        matches!(
            self,
            ActiveKind::Drawing | ActiveKind::Render | ActiveKind::VariableStudio
        )
    }
}

/// The three default planes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaneKind {
    Top,
    Front,
    Right,
}

impl PlaneKind {
    pub const ALL: [PlaneKind; 3] = [PlaneKind::Top, PlaneKind::Front, PlaneKind::Right];

    pub fn name(self) -> &'static str {
        match self {
            PlaneKind::Top => "Top",
            PlaneKind::Front => "Front",
            PlaneKind::Right => "Right",
        }
    }

    /// The name of its side facing the viewer in `view`, and how much of its label shows
    /// ([`label_alpha`]): seen from behind, the other side's (Front reads Back, Right Left, Top
    /// Bottom).
    pub fn label(self, view: &ViewState) -> (&'static str, f32) {
        let facing = self.normal().dot(view.back());
        let name = if facing >= 0.0 {
            self.name()
        } else {
            match self {
                PlaneKind::Top => "Bottom",
                PlaneKind::Front => "Back",
                PlaneKind::Right => "Left",
            }
        };
        (name, label_alpha(facing.abs()))
    }

    /// Its place in [`PlaneKind::ALL`].
    pub fn index(self) -> usize {
        match self {
            PlaneKind::Top => 0,
            PlaneKind::Front => 1,
            PlaneKind::Right => 2,
        }
    }

    /// In-plane right direction (the label's reading direction).
    pub fn u(self) -> Vec3 {
        match self {
            PlaneKind::Top | PlaneKind::Front => Vec3::X,
            PlaneKind::Right => Vec3::Y,
        }
    }

    /// In-plane up direction.
    pub fn v(self) -> Vec3 {
        match self {
            PlaneKind::Top => Vec3::Y,
            PlaneKind::Front | PlaneKind::Right => Vec3::Z,
        }
    }

    /// The side the plane faces (`u × v`): +Z, -Y, +X.
    pub fn normal(self) -> Vec3 {
        self.u().cross(self.v())
    }

    pub fn rotation(self) -> Quat {
        Quat::from_mat3(&Mat3::from_cols(self.u(), self.v(), self.normal()))
    }

    /// The label's corner: the plane's top-left as seen from its front.
    pub fn label_corner(self) -> Vec3 {
        (self.v() - self.u()) * PLANE_HALF
    }
}

/// Half the side length of a default plane, in mm.
pub const PLANE_HALF: f32 = 75.0;

impl PlaneKind {
    /// The sketch model's name for this plane.
    pub fn plane_ref(self) -> PlaneRef {
        match self {
            PlaneKind::Top => PlaneRef::Top,
            PlaneKind::Front => PlaneRef::Front,
            PlaneKind::Right => PlaneRef::Right,
        }
    }

    /// The default plane, if `p` is one (not a face).
    pub fn from_plane_ref(p: PlaneRef) -> Option<Self> {
        match p {
            PlaneRef::Top => Some(PlaneKind::Top),
            PlaneRef::Front => Some(PlaneKind::Front),
            PlaneRef::Right => Some(PlaneKind::Right),
            PlaneRef::Face(_) | PlaneRef::Feature(_) => None,
        }
    }
}

/// The pick that stands for a plane: a default plane, or a Plane feature (P3.7: picked in the
/// view or the feature list as the feature).
pub fn plane_pick(p: PlaneRef) -> Option<Pick> {
    match p {
        PlaneRef::Feature(f) => Some(Pick::Feature(FeatureId(f.feature))),
        p => PlaneKind::from_plane_ref(p).map(Pick::Plane),
    }
}

/// How a field names a plane: "Top plane", a Plane feature's name ("Lower Plane"), or a sheet
/// metal flat pattern's "Face of Sheet metal model 1" (P3I.6, lesson t0052).
pub fn plane_label(features: &[cadrs_core::Feature], p: PlaneRef) -> String {
    if let PlaneRef::Feature(f) = p
        && let Some((model, _)) = cadrs_core::sheetmetal_flat::flat_target(features, f.feature)
    {
        let name = features
            .iter()
            .find(|x| x.id == model)
            .map_or("Sheet metal model", |x| x.name.as_str());
        return format!("Face of {name}");
    }
    match p {
        PlaneRef::Feature(f) => features
            .iter()
            .find(|x| x.id.0 == f.feature)
            .map_or("Plane".into(), |x| x.name.clone()),
        p => p.display_name(),
    }
}

/// Something that can be selected in the viewport, the feature list or the parts list: one
/// selection model for planes, features, sketch regions, and a part's faces, edges and
/// vertices (P3.2, by persistent name) or the whole part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pick {
    Plane(PlaneKind),
    Origin,
    /// A feature in the feature list (a sketch).
    Feature(FeatureId),
    /// A face of a part.
    Face(PartId, cadrs_sketch::FaceName),
    /// An edge of a part.
    Edge(PartId, cadrs_sketch::EdgeName),
    /// A vertex of a part.
    Vertex(PartId, cadrs_sketch::VertexName),
    /// A whole part, from the Parts list.
    Part(PartId),
    /// A closed region of a sketch (its index among the sketch's regions).
    Region(FeatureId, u32),
    /// A curve of a sketch shown in the Part Studio (a revolve axis, P3.4).
    SketchCurve(FeatureId, cadrs_sketch::CurveId),
    /// A point of a sketch shown in the Part Studio (where a hole goes, P3.6).
    SketchPoint(FeatureId, cadrs_sketch::PointId),
    /// The top-level assembly (the Instances list's root row, P3B.1): measured as a whole.
    Assembly,
    /// An instance of the pattern being edited, by grid index: its Skip dot (P3.8, PS22.5).
    Instance(FeatureId, u32, u32),
}

impl Pick {
    /// The part it is on (faces, edges, vertices and parts).
    pub fn part(&self) -> Option<PartId> {
        match *self {
            Pick::Face(f, _) | Pick::Edge(f, _) | Pick::Vertex(f, _) | Pick::Part(f) => Some(f),
            _ => None,
        }
    }
}

/// The feature that controls how `pick` looks: the one that made a face (an edge's or a
/// vertex's latest), the sketch of a sketch curve, point or region, the feature that made a part.
pub fn feature_of(features: &[cadrs_core::Feature], pick: Pick) -> Option<FeatureId> {
    let op = match pick {
        Pick::Feature(f)
        | Pick::Region(f, _)
        | Pick::SketchCurve(f, _)
        | Pick::SketchPoint(f, _) => return Some(f),
        Pick::Part(p) => return Some(p.feature),
        Pick::Face(_, face) => face.op,
        Pick::Edge(_, edge) => crate::parts::edge_maker(features, &edge),
        Pick::Vertex(_, v) => {
            let at = |op: cadrs_sketch::OpId| features.iter().position(|f| f.id.0 == op);
            v.faces.iter().map(|f| f.op).max_by_key(|op| at(*op))?
        }
        _ => return None,
    };
    let id = FeatureId(op);
    features
        .iter()
        .any(|f| f.id == id)
        .then_some(id)
        .or_else(|| pick.part().map(|p| p.feature))
}

/// A double-click in the Part Studio: opens the feature behind what was clicked (its sketch,
/// its extrude, …) and shows it in the feature list, unless a dialog or sketch is open
/// already.
fn edit_picked(world: &mut World, pick: Pick) {
    if crate::history_panel::editing(world) {
        return;
    }
    let feature = world
        .get_resource::<crate::ActiveDocument>()
        .and_then(|d| feature_of(d.active_element()?.features(), pick));
    if let Some(id) = feature {
        world.resource_mut::<Selection>().0.clear();
        crate::document::reveal_feature(world, id);
        crate::document::edit_feature(world, id);
    }
}

/// A click that picks something (or nothing: `None` clears) in the viewport or the feature
/// list. In modeling mode it toggles the [`Selection`]; while a sketch dialog is open the
/// sketch uses it (to choose its plane).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickRequest(pub Option<Pick>);

/// Hover highlights: from the viewport and from the feature list.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PlaneHighlight {
    pub viewport: Option<Pick>,
    pub list: Option<Pick>,
}

impl PlaneHighlight {
    pub fn is_hovered(&self, p: Pick) -> bool {
        self.viewport == Some(p) || self.list == Some(p)
    }
}

/// An entity to show as hovered in the view instead of the one under the pointer (the mate
/// dialog's Shift lock, A6.5).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct HoverOverride(pub Option<Pick>);

/// Entities a panel or dialog lights up besides the pointer's and the selection's: `hovered`
/// draws in the hover orange (a Sheet metal table row's joint, all its faces), `selected` in the
/// selection amber without being selected (the Modify joint dialog's joint). Each owner sets
/// its own list and clears it when done.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ExtraHighlight {
    pub hovered: Vec<Pick>,
    pub selected: Vec<Pick>,
}

/// A panel's own picking (P3F.5: the simulation's load dialog picks faces): while set, clicks
/// and hover in the view (Part Studio or Assembly) use this filter and the picks go to that
/// panel, not to the selection.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct PickFilterOverride(pub Option<crate::parts::PickFilter>);

/// The selection (additive, like Onshape: clicking toggles, clicking empty space clears).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Selection(pub Vec<Pick>);

/// How a click on a feature-list row selects, set with its [`PickRequest`] (the view's own
/// clicks keep toggling).
#[derive(Debug, Clone, PartialEq)]
pub enum RowSelect {
    /// A plain click: the row alone (clicking the only selected row clears it).
    Only,
    /// Ctrl: the row added or taken away.
    Toggle,
    /// Shift: every row from the anchor to this one, in the list's order.
    Range(Vec<Pick>),
}

/// The feature list's pending row click, and the row a Shift+click ranges from (the last one
/// clicked without Shift).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct FeatureRowClick {
    pub pending: Option<(Pick, RowSelect)>,
    pub anchor: Option<Pick>,
}

impl Selection {
    pub fn contains(&self, p: Pick) -> bool {
        self.0.contains(&p)
    }

    pub fn toggle(&mut self, p: Pick) {
        if let Some(i) = self.0.iter().position(|x| *x == p) {
            self.0.remove(i);
        } else {
            self.0.push(p);
        }
    }

    /// The first selected plane.
    pub fn plane(&self) -> Option<PlaneKind> {
        self.0.iter().find_map(|p| match p {
            Pick::Plane(k) => Some(*k),
            _ => None,
        })
    }

    /// The first selected part face.
    pub fn face(&self) -> Option<(PartId, cadrs_sketch::FaceName)> {
        self.0.iter().find_map(|p| match p {
            Pick::Face(f, t) => Some((*f, *t)),
            _ => None,
        })
    }

    /// The first selected feature.
    pub fn feature(&self) -> Option<FeatureId> {
        self.0.iter().find_map(|p| match p {
            Pick::Feature(f) => Some(*f),
            _ => None,
        })
    }
}

/// Marks a feature-list row that stands for a plane or the origin.
#[derive(Component, Debug, Clone, Copy)]
pub struct PickRow(pub Pick);

/// Plane label UI nodes.
#[derive(Component)]
struct PlaneLabel(PlaneKind);

/// The inner (rotated) node of an affine label.
#[derive(Component)]
pub(crate) struct AffineInner;

#[derive(Component)]
struct OriginMarker;

#[derive(Component)]
struct OriginDot;

/// Plane materials: normal and selected.
#[derive(Resource)]
pub(crate) struct PlaneMaterials {
    pub(crate) normal: Handle<StandardMaterial>,
    pub(crate) selected: Handle<StandardMaterial>,
}

/// Wider lines for selected outlines, drawn on top of everything by the [`OverlayCamera`].
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct HighlightGizmos;

/// Hover outlines (as thin as the plane edges), drawn on top like [`HighlightGizmos`].
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct HoverGizmos;

/// Planes seen edge-on (the sketch axes in a normal-to view): 1.5 px lines.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EdgeOnGizmos;

/// Pointer drag state for the viewport.
#[derive(Resource, Debug, Default)]
pub struct ViewportDrag {
    pointer: Vec2,
    buttons: [bool; 3],
    /// Where the primary button went down over the viewport, if it did.
    primary_down: Option<Vec2>,
    /// A right or middle drag that started over the viewport.
    navigating: bool,
    /// Where that drag started (a drag zoom keeps it in place).
    nav_start: Vec2,
    moved: f32,
    /// Where the secondary button went down over the viewport (a right-click that does not
    /// move opens a context menu).
    secondary_down: Option<Vec2>,
    /// The last click in the view: when (s), where, and what it picked (a second click on the
    /// same thing soon after is a double-click).
    last_click: Option<(f32, Vec2, Pick)>,
}

impl ViewportDrag {
    /// The last known mouse position (logical px).
    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }

    /// Where the primary button went down over the viewport, while it is down.
    pub fn primary_down(&self) -> Option<Vec2> {
        self.primary_down
    }

    /// A right or middle drag (orbit, pan) is in progress.
    pub fn navigating(&self) -> bool {
        self.navigating
    }

    /// A right-drag orbit is in progress.
    pub fn orbiting(&self, ctrl: bool) -> bool {
        self.navigating && self.buttons[1] && !ctrl && !self.buttons[2]
    }

    /// A middle-drag (or Ctrl+right-drag) pan is in progress.
    pub fn panning(&self, ctrl: bool) -> bool {
        self.navigating && (self.buttons[2] || (self.buttons[1] && ctrl))
    }
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 1.5;
    let (config, _) = store.config_mut::<HighlightGizmos>();
    config.line.width = 2.4;
    config.render_layers = RenderLayers::layer(OVERLAY_LAYER);
    let (config, _) = store.config_mut::<EdgeOnGizmos>();
    config.line.width = 1.5;
    let (config, _) = store.config_mut::<HoverGizmos>();
    config.line.width = 1.5;
    config.render_layers = RenderLayers::layer(OVERLAY_LAYER);
}

/// The viewport cameras' orthographic projection at `scale` mm per logical pixel.
pub fn ortho_projection(scale: f32) -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::WindowSize,
        scale,
        near: 0.0,
        far: camera::camera_far(scale),
        ..OrthographicProjection::default_3d()
    })
}

/// No light clustering: the app lights with directional lights only, so there is nothing to
/// cluster, but Bevy's GPU clustering allocates and uploads every 3D camera's cluster buffers
/// each frame, at capacities that only grow (half a second a frame after a long session). On the
/// CPU (GPU clustering fails with no clusters), with none.
fn cpu_light_clusters(settings: Option<ResMut<bevy::light::cluster::GlobalClusterSettings>>) {
    if let Some(mut settings) = settings {
        settings.gpu_clustering = None;
    }
}

/// The occluded camera renders only while something is on its layer (a sketch shown, the Extrude
/// dialog's regions): a camera costs the render thread its passes each frame with nothing to
/// draw too (house_test went from 187 to 218 fps without it). Off in a drawing and a board's flat
/// views, whose cameras take its order.
#[allow(clippy::type_complexity)]
fn occluded_camera_when_drawn(
    kind: Res<ActiveKind>,
    eda2d: Res<crate::eda::Eda2d>,
    store: Res<GizmoConfigStore>,
    handles: Res<bevy::gizmos::GizmoHandles>,
    drawn: Query<
        (&RenderLayers, &InheritedVisibility),
        Or<(With<Mesh3d>, With<bevy::gizmos::retained::Gizmo>)>,
    >,
    mut q_cam: Query<(&mut Camera, &Name), With<OverlayCamera>>,
) {
    let layer = RenderLayers::layer(OCCLUDED_LAYER);
    // Immediate gizmos drawn this frame (a group's handle is set when it drew).
    let gizmos = || {
        store.iter().any(|(id, config, _)| {
            config.enabled
                && config.render_layers.intersects(&layer)
                && handles.handles().get(id).is_some_and(Option::is_some)
        })
    };
    let meshes = || {
        drawn
            .iter()
            .any(|(l, vis)| vis.get() && l.intersects(&layer))
    };
    let flat = *kind == ActiveKind::Drawing || eda2d.flat();
    let want = !flat && (gizmos() || meshes());
    for (mut cam, name) in &mut q_cam {
        if name.as_str() == "occluded-camera" && cam.is_active != want {
            cam.is_active = want;
        }
    }
}

fn no_light_clusters(add: On<Add<Camera3d>>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(bevy::light::cluster::ClusterConfig::None);
}

fn spawn_main_camera(mut commands: Commands, surface: Res<RenderSurface>) {
    let view = ViewState::default();
    let projection = || ortho_projection(view.scale);
    let transform =
        Transform::from_translation(view.camera_position()).with_rotation(view.rotation());
    commands.spawn((
        Name::new("main-camera"),
        MainCamera,
        Camera3d::default(),
        surface.target.clone(),
        // The `ClearColor` resource: the scheme's viewport background (`cadrs_ui::color_scheme`).
        Camera {
            clear_color: ClearColorConfig::Default,
            ..default()
        },
        projection(),
        // Keep colors exact and screenshots deterministic.
        Tonemapping::None,
        DebandDither::Disabled,
        transform,
    ));
    commands.spawn((
        Name::new("occluded-camera"),
        OverlayCamera,
        // The parts' depth from the main pass: what it draws is hidden behind parts.
        Camera3d {
            depth_load_op: bevy::camera::Camera3dDepthLoadOp::Load,
            ..default()
        },
        surface.target.clone(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(OCCLUDED_LAYER),
        projection(),
        Tonemapping::None,
        DebandDither::Disabled,
        transform,
    ));
    commands.spawn((
        Name::new("overlay-camera"),
        OverlayCamera,
        Camera3d::default(),
        surface.target.clone(),
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(OVERLAY_LAYER),
        projection(),
        Tonemapping::None,
        DebandDither::Disabled,
        IsDefaultUiCamera,
        transform,
    ));
}

/// Onshape's default view for a new Part Studio as a camera transform (Z up, from the
/// front-right and above).
pub fn isometric_transform() -> Transform {
    let v = ViewState::default();
    Transform::from_translation(v.camera_position()).with_rotation(v.rotation())
}

/// The plane fill: a pale cyan at low opacity; in dark, the schematic's warm grid colour,
/// fainter, so the three planes don't tint the whole view.
pub fn plane_fill() -> Color {
    cadrs_ui::by_scheme(
        Color::LinearRgba(LinearRgba::new(0.05, 0.42, 0.47, 0.09)),
        Color::LinearRgba(LinearRgba::new(0.254, 0.216, 0.133, 0.07)),
    )
}

/// Gives the planes the scheme's fill.
fn recolor_planes(
    planes: Option<Res<PlaneMaterials>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Some(planes) = planes
        && let Some(mut m) = materials.get_mut(&planes.normal)
    {
        m.base_color = plane_fill();
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut grid_materials: ResMut<Assets<GroundGridMaterial>>,
    theme: Res<Theme>,
    mut view: ResMut<ViewportView>,
    mut selection: ResMut<Selection>,
    mut highlight: ResMut<PlaneHighlight>,
) {
    *view = ViewportView::default();
    selection.0.clear();
    *highlight = PlaneHighlight::default();
    let fill = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        // Behind whatever lies on a plane (a face at 0, a picture dropped on Top), which
        // otherwise fought it for depth: twice a picture's lift the other way (about 40 µm).
        depth_bias: -2.0 * crate::picture::SURFACE_BIAS,
        ..default()
    };
    let normal = materials.add(fill(plane_fill()));
    let selected = materials.add(fill(theme.selection_3d.with_alpha(0.16)));
    commands.insert_resource(PlaneMaterials {
        normal: normal.clone(),
        selected,
    });
    let quad = meshes.add(Rectangle::new(PLANE_HALF * 2.0, PLANE_HALF * 2.0));
    for kind in PlaneKind::ALL {
        let lower = kind.name().to_lowercase();
        commands.spawn((
            Name::new(format!("plane-{lower}")),
            kind,
            Mesh3d(quad.clone()),
            MeshMaterial3d(normal.clone()),
            Transform::from_rotation(kind.rotation()),
            DespawnOnExit(AppState::Document),
        ));
    }
    commands.spawn((
        Name::new("ground-grid"),
        GroundGrid,
        Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
        MeshMaterial3d(grid_materials.add(GroundGridMaterial {
            params: GroundGridParams::default(),
        })),
        Transform::IDENTITY,
        Visibility::Hidden,
        bevy::light::NotShadowCaster,
        Pickable::IGNORE,
        DespawnOnExit(AppState::Document),
    ));
}

/// Spawns the plane labels and the origin marker inside the viewport area (so the panels clip
/// them). Called by the document shell when it builds the viewport area.
pub fn spawn_viewport_overlay(p: &mut ChildSpawnerCommands, theme: &Theme) {
    for kind in PlaneKind::ALL {
        let lower = kind.name().to_lowercase();
        // Outer node: rotation + (non-uniform) scale; inner text: second rotation. Together
        // they map the text onto the plane (see `affine_parts`).
        p.spawn((
            Name::new(format!("plane-label-{lower}")),
            PlaneLabel(kind),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_child((
            AffineInner,
            theme.text(kind.name(), 17.0, FontWeight::SEMIBOLD, theme.plane_label),
            Pickable::IGNORE,
        ));
    }
    p.spawn((
        Name::new("origin"),
        OriginMarker,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(12.0),
            height: Val::Px(12.0),
            // A 2 px ring with a white fill around a 4 px dot, Onshape's bullseye: geometry
            // meeting at the origin (a rectangle corner, its dot) stays under it (`screens/13`,
            // `17a`).
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::MAX,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor::all(theme.foreground),
        BackgroundColor(Color::WHITE),
        Pickable::IGNORE,
    ))
    .with_child((
        OriginDot,
        Node {
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(theme.foreground),
        Pickable::IGNORE,
    ));
}

fn reset_viewport(mut drag: ResMut<ViewportDrag>, mut highlight: ResMut<PlaneHighlight>) {
    *drag = ViewportDrag::default();
    *highlight = PlaneHighlight::default();
}

/// Keeps [`ActiveKind`] in sync with the active tab and swaps the per-tab view.
fn track_active_element(
    doc: Option<Res<ActiveDocument>>,
    mut kind: ResMut<ActiveKind>,
    mut view: ResMut<ViewportView>,
    mut selection: ResMut<Selection>,
    rect: Res<ViewportRect>,
    cache: Res<crate::parts::PartCache>,
) {
    let Some(doc) = doc else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let k = match el.kind {
        cadrs_core::ElementKind::PartStudio { .. } => ActiveKind::PartStudio,
        cadrs_core::ElementKind::Assembly => ActiveKind::Assembly,
        cadrs_core::ElementKind::Drawing(_) => ActiveKind::Drawing,
        cadrs_core::ElementKind::PcbStudio(_) => ActiveKind::PcbStudio,
        cadrs_core::ElementKind::Render(_) => ActiveKind::Render,
        cadrs_core::ElementKind::CamStudio(_) => ActiveKind::CamStudio,
        cadrs_core::ElementKind::VariableStudio(_) => ActiveKind::VariableStudio,
    };
    kind.set_if_neq(k);
    if view.element != Some(el.id) {
        let v = &mut *view;
        if let Some(old) = v.element {
            let current = v.target();
            v.per_element.insert(old, current);
        }
        v.animation = None;
        v.fit_pending = None;
        v.view = match v.per_element.get(&el.id).copied() {
            Some(saved) => saved,
            // A Part Studio or Assembly tab opened for the first time: its parts (an assembly's
            // instances, with the assembly margin, `ex3-step5.png`) framed once they are built:
            // the rebuild goes on in the background (a big one takes seconds).
            None if k == ActiveKind::PartStudio
                || k == ActiveKind::CamStudio
                || (k == ActiveKind::Assembly
                    && el.assembly_model().is_some_and(|a| !a.instances.is_empty())) =>
            {
                let start = ViewState::default();
                match built_points(&cache, el.id, k) {
                    Some(pts) if pts.is_empty() => start,
                    Some(pts) => start.fitted(&pts, rect.0.size(), fit_fill(k)),
                    None => {
                        v.fit_pending = Some((el.id, start));
                        start
                    }
                }
            }
            None => ViewState::default(),
        };
        v.element = Some(el.id);
        if !selection.0.is_empty() {
            selection.0.clear();
        }
    }
}

/// The points of the shown parts of tab `element` (an Assembly's instances) as built in the
/// part cache; `None` while they are being built. Empty for a Part Studio without parts.
fn built_points(
    cache: &crate::parts::PartCache,
    element: ElementId,
    kind: ActiveKind,
) -> Option<Vec<Vec3>> {
    if kind == ActiveKind::Assembly {
        if cache.assembly != Some(element) || cache.rebuilding {
            return None;
        }
    } else if cache.rebuilding || cache.settled().map(|(id, ..)| id) != Some(element) {
        return None;
    }
    let pts: Vec<Vec3> = cache
        .shown()
        .flat_map(|p| {
            p.solid
                .positions
                .iter()
                .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
        })
        .collect();
    Some(if pts.is_empty() && kind == ActiveKind::Assembly {
        vec![Vec3::ZERO]
    } else {
        pts
    })
}

/// Fits a Part Studio's or Assembly's first view to its parts once they are built
/// ([`ViewportView::fit_pending`]).
fn fit_assembly_when_built(
    mut view: ResMut<ViewportView>,
    cache: Res<crate::parts::PartCache>,
    rect: Res<ViewportRect>,
    kind: Res<ActiveKind>,
) {
    let Some((el, start)) = view.fit_pending else {
        return;
    };
    if view.element != Some(el) {
        view.fit_pending = None;
        return;
    }
    let Some(pts) = built_points(&cache, el, *kind) else {
        return;
    };
    view.fit_pending = None;
    if !pts.is_empty() && view.animation.is_none() && view.view.approx_eq(&start) {
        view.view = start.fitted(&pts, rect.0.size(), fit_fill(*kind));
    }
}

fn track_viewport_rect(
    q: Query<(&ComputedNode, &UiGlobalTransform), With<ViewportArea>>,
    mut rect: ResMut<ViewportRect>,
) {
    let Some((node, t)) = q.iter().next() else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let size = node.size() * scale;
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let center = t.translation * scale;
    let r = Rect::from_center_size(center, size);
    if r != rect.0 {
        rect.0 = r;
    }
}

fn track_dialog_inset(
    q: Query<(&ComputedNode, &UiGlobalTransform), With<cadrs_ui::FeatureDialogState>>,
    rect: Res<ViewportRect>,
    session: Option<Res<crate::sketch::SketchSession>>,
    mut inset: ResMut<DialogInset>,
) {
    let mut new = 0.0;
    if session.is_none() {
        for (node, t) in &q {
            let scale = node.inverse_scale_factor();
            let size = node.size() * scale;
            if size.x <= 0.0 {
                continue;
            }
            let r = Rect::from_center_size(t.translation * scale, size);
            // A dialog in the viewport's left half.
            if r.min.x < rect.0.center().x {
                new = f32::max(new, r.max.x - rect.0.min.x);
            }
        }
    }
    if inset.0 != new {
        inset.0 = new;
    }
}

/// The gaps between the floating panels are the 3D view's too: the pointer over the backdrop
/// behind the panels counts as over the viewport area, for everything that asks the hover map.
fn alias_backdrop(
    mut hover: ResMut<HoverMap>,
    q_back: Query<Entity, With<crate::document::ViewportBackdrop>>,
    q_area: Query<Entity, With<ViewportArea>>,
) {
    let (Some(back), Some(area)) = (q_back.iter().next(), q_area.iter().next()) else {
        return;
    };
    for hits in hover.values_mut() {
        if let Some(hit) = hits.get(&back).cloned() {
            hits.insert(area, hit);
        }
    }
}

/// True while the mouse pointer is over the viewport area (not a panel, menu or dialog).
pub(crate) fn pointer_over_viewport(
    hover: &HoverMap,
    q_area: &Query<Entity, With<ViewportArea>>,
) -> bool {
    let Some(hits) = hover.get(&PointerId::Mouse) else {
        return false;
    };
    q_area.iter().any(|e| hits.contains_key(&e))
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn viewport_pointer(
    mut inputs: MessageReader<PointerInput>,
    mut drag: ResMut<ViewportDrag>,
    mut view: ResMut<ViewportView>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    keys: Res<ButtonInput<KeyCode>>,
    kind: Res<ActiveKind>,
    mut picks: MessageWriter<PickRequest>,
    planes: Res<PlanesVisible>,
    (parts, sketch, extrude, applied, create, pick_override, eda2d): (
        Res<crate::parts::PartCache>,
        Option<Res<crate::sketch::SketchSession>>,
        Option<Res<crate::extrude::ExtrudeSession>>,
        Option<Res<crate::applied::AppliedSession>>,
        Option<Res<crate::create_selection::CreateSelection>>,
        Res<PickFilterOverride>,
        Res<crate::eda::Eda2d>,
    ),
    (mut focus, q_number, mut grab, zoom_window, time): (
        ResMut<bevy::input_focus::InputFocus>,
        Query<(), With<cadrs_ui::NumberFieldEdit>>,
        ResMut<crate::assembly::ViewportGrab>,
        Option<Res<crate::view_options::ZoomWindow>>,
        Res<Time>,
    ),
    prefs: Res<crate::preferences_ui::LocalPreferences>,
    mut commands: Commands,
) {
    if kind.is_flat() || eda2d.flat() {
        // The sheet has its own navigation (`crate::drawing`); a render has none.
        inputs.clear();
        return;
    }
    let over = pointer_over_viewport(&hover, &q_area);
    let filter = pick_filter(
        planes.0,
        sketch.as_deref(),
        extrude.as_deref(),
        applied.as_deref(),
        create.as_deref(),
    );
    let forced = pick_override.0;
    let filter = forced.or(filter);
    let modeling = sketch.is_none() && extrude.is_none();
    let mods = crate::preferences_ui::modifiers(&keys);
    let mouse = prefs.mouse();
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => {
                let delta = pos - drag.pointer;
                drag.pointer = pos;
                drag.moved += delta.length();
                if drag.navigating {
                    view.animation = None;
                    // P3E.3: what the drag does is the mouse preference's (Onshape's: right
                    // rotates, Alt+right without roll, middle or Ctrl+right pans, Shift+middle
                    // zooms).
                    use cadrs_core::preferences::ViewAction;
                    let action = [(2, PointerButton::Middle), (1, PointerButton::Secondary)]
                        .into_iter()
                        .filter(|(i, _)| drag.buttons[*i])
                        .find_map(|(_, b)| {
                            mouse.action(crate::preferences_ui::mouse_button(b), mods)
                        });
                    match action {
                        Some(ViewAction::Rotate) => match view.pivot {
                            Some(p) => view.view.orbit_about(p, |v| v.orbit(delta)),
                            None => view.view.orbit(delta),
                        },
                        Some(ViewAction::RotateTurntable) => match view.pivot {
                            Some(p) => view.view.orbit_about(p, |v| v.orbit_turntable(delta)),
                            None => view.view.orbit_turntable(delta),
                        },
                        Some(ViewAction::Pan) => view.view.pan(delta),
                        Some(ViewAction::Zoom) => {
                            let at = rect.offset(drag.nav_start);
                            view.view.zoom_at(DRAG_ZOOM_PER_PX.powf(-delta.y), at);
                        }
                        None => {}
                    }
                }
            }
            PointerAction::Press(b) => {
                drag.pointer = pos;
                let i = button_index(b);
                drag.buttons[i] = true;
                match b {
                    PointerButton::Primary => {
                        // Zoom to window takes the drag (P3E.3a).
                        drag.primary_down = (over && zoom_window.is_none()).then_some(pos);
                        drag.moved = 0.0;
                        // A click in the view leaves a dialog's number field (committing what
                        // was typed), as in Onshape (P3.10 judge: a caret stayed in Tip angle).
                        if over && focus.get().is_some_and(|f| q_number.contains(f)) {
                            focus.clear();
                        }
                    }
                    PointerButton::Secondary => {
                        drag.secondary_down = over.then_some(pos);
                        if over && mouse.navigates_with(crate::preferences_ui::mouse_button(b)) {
                            drag.navigating = true;
                            drag.nav_start = pos;
                        }
                    }
                    _ => {
                        if over && mouse.navigates_with(crate::preferences_ui::mouse_button(b)) {
                            drag.navigating = true;
                            drag.nav_start = pos;
                        }
                    }
                }
            }
            PointerAction::Release(b) => {
                drag.pointer = pos;
                drag.buttons[button_index(b)] = false;
                if b == PointerButton::Primary {
                    // A press a manipulator took (the assembly triad, an insert placement) is
                    // not a click that selects.
                    let grabbed = std::mem::take(&mut grab.0);
                    let down = drag.primary_down.take();
                    // A board's layout edited in the view takes the clicks
                    // (`crate::board_in_studio`).
                    if let Some(down) = down
                        && over
                        && !grabbed
                        && down.distance(pos) < 4.0
                        && eda2d.plane().is_none()
                    {
                        // A click in the viewport: toggle what is under the cursor, or clear.
                        let picked = match (*kind, filter) {
                            (ActiveKind::Assembly, Some(f)) if forced.is_some() => {
                                crate::parts::pick_scene(&parts, &view.view, rect.offset(pos), f)
                            }
                            (ActiveKind::PartStudio, Some(f)) => {
                                crate::parts::pick_scene(&parts, &view.view, rect.offset(pos), f)
                            }
                            // P3B.1: faces, edges and vertices of the instances.
                            (ActiveKind::Assembly, Some(_)) => crate::parts::pick_scene(
                                &parts,
                                &view.view,
                                rect.offset(pos),
                                crate::assembly::pick_filter(),
                            ),
                            _ => None,
                        };
                        // Double-clicking a face, edge, vertex, sketch or part edits the
                        // feature that made it, as in Onshape. Its second click doesn't select
                        // (the dialog would take what is selected as its own).
                        let now = time.elapsed_secs();
                        let again = drag.last_click.take().is_some_and(|(t, at, p)| {
                            Some(p) == picked
                                && now - t <= cadrs_ui::inline_edit::DOUBLE_CLICK_TIME
                                && at.distance(pos) < 6.0
                        });
                        match picked {
                            Some(p)
                                if again
                                    && modeling
                                    && forced.is_none()
                                    && *kind == ActiveKind::PartStudio =>
                            {
                                commands.queue(move |world: &mut World| edit_picked(world, p));
                            }
                            // Alt+click in a shared document: "look here" for everyone
                            // (`collab::pointers::ping`), not a pick; elsewhere a pick as ever.
                            _ if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) => {
                                commands.queue(move |world: &mut World| {
                                    if !crate::collab::pointers::ping(world) {
                                        world.write_message(PickRequest(picked));
                                    }
                                });
                            }
                            _ => {
                                picks.write(PickRequest(picked));
                                if !again {
                                    drag.last_click = picked.map(|p| (now, pos, p));
                                }
                            }
                        }
                    } else if let Some(down) = down
                        && applied.is_some()
                        && *kind == ActiveKind::PartStudio
                    {
                        // P3.11 (P3.8 judge): a box dragged over a pattern's Skip dots toggles
                        // every dot in it.
                        let (a, b) = (rect.offset(down), rect.offset(pos));
                        commands.queue(move |world: &mut World| {
                            crate::pattern::skip_dots_in_box(world, a, b)
                        });
                    }
                } else {
                    // S1.2, S2.3: a right-click (no drag) in the Part Studio opens the menu of
                    // what is under the pointer. Inside a sketch the sketch tools open theirs.
                    if b == PointerButton::Secondary
                        && *kind == ActiveKind::Assembly
                        && let Some(down) = drag.secondary_down.take()
                        && over
                        && down.distance(pos) < 4.0
                    {
                        // P3B.1: the instance menu, or the empty-space menu.
                        let picked = crate::parts::pick_scene(
                            &parts,
                            &view.view,
                            rect.offset(pos),
                            crate::assembly::pick_filter(),
                        );
                        commands.queue(move |world: &mut World| {
                            crate::assembly::menu::open_viewport_menu(world, pos, picked)
                        });
                    }
                    if b == PointerButton::Secondary
                        && let Some(down) = drag.secondary_down.take()
                        && over
                        && down.distance(pos) < 4.0
                        && modeling
                        && *kind == ActiveKind::PartStudio
                    {
                        let picked = crate::parts::pick_scene(
                            &parts,
                            &view.view,
                            rect.offset(pos),
                            crate::parts::PickFilter::modeling(planes.0),
                        );
                        commands.queue(move |world: &mut World| {
                            crate::viewport_menu::open_modeling_menu(world, pos, picked)
                        });
                    }
                    if !drag.buttons[1] && !drag.buttons[2] {
                        drag.navigating = false;
                    }
                }
            }
            PointerAction::Scroll { unit, y, .. } => {
                if over {
                    let lines = match unit {
                        MouseScrollUnit::Line => y,
                        MouseScrollUnit::Pixel => y / 40.0,
                    };
                    view.animation = None;
                    let cursor = rect.offset(pos);
                    // P3E.3a: in perspective, zoom about the part under the cursor.
                    if view.view.perspective
                        && let Some((_, _, t)) = crate::parts::pick_face(&parts, &view.view, cursor)
                    {
                        let (o, d) = view.view.ray(cursor);
                        view.view = view.view.refocused(o + d * t);
                    }
                    view.view.wheel(lines, cursor);
                }
            }
            PointerAction::Cancel => {
                *drag = ViewportDrag::default();
            }
        }
    }
}

/// What a click in the viewport can pick: in modeling, the origin, faces, sketch regions and
/// planes; while a
/// sketch waits for its plane, planar faces and planes; while the Extrude dialog is open, sketch
/// regions. `None` while a sketch is being drawn (the sketch tools own the pointer).
pub fn pick_filter(
    planes: [bool; 3],
    sketch: Option<&crate::sketch::SketchSession>,
    extrude: Option<&crate::extrude::ExtrudeSession>,
    applied: Option<&crate::applied::AppliedSession>,
    create: Option<&crate::create_selection::CreateSelection>,
) -> Option<crate::parts::PickFilter> {
    use crate::parts::PickFilter;
    // Create selection (X12): part edges only (P3.8: faces on its Faces tab).
    if let Some(c) = create
        && sketch.is_none()
    {
        return Some(if c.faces_mode {
            PickFilter {
                faces: true,
                ..PickFilter::none()
            }
        } else {
            PickFilter {
                edges: true,
                ..PickFilter::none()
            }
        });
    }
    if let Some(a) = applied {
        return Some(a.pick_filter());
    }
    if let Some(x) = extrude {
        use crate::extrude::ExtrudeField;
        let none = PickFilter {
            origin: false,
            planes: [false; 3],
            faces: false,
            planar_only: false,
            edges: false,
            regions: false,
            sketch_curves: false,
            sketch_points: false,
            plane_features: false,
            connectors: false,
            dots: None,
            // A click goes through the extrude's own preview.
            skip_op: Some(x.feature.0),
            whole_parts: false,
        };
        return Some(match x.field {
            // Regions (a click through the translucent preview reaches the region under it) and
            // planar faces.
            ExtrudeField::Input => PickFilter {
                faces: true,
                planar_only: true,
                regions: true,
                ..none
            },
            // Faces, edges and vertices (a part through any of them).
            ExtrudeField::UpTo | ExtrudeField::SecondUpTo => PickFilter {
                faces: true,
                edges: true,
                ..none
            },
            // Whole parts only: no edges or vertices to hover.
            ExtrudeField::MergeScope => PickFilter {
                faces: true,
                whole_parts: true,
                ..none
            },
            // P3.7 (PS12.3): a plane's normal is a direction too; P3.8: a mate connector's Z.
            ExtrudeField::Direction => PickFilter {
                faces: true,
                planar_only: true,
                edges: true,
                planes,
                plane_features: true,
                connectors: true,
                ..none
            },
            // A revolve axis: a sketch line (or circle), a part edge, a curved face, a mate
            // connector's Z (P3.8).
            ExtrudeField::Axis => PickFilter {
                faces: true,
                edges: true,
                sketch_curves: true,
                connectors: true,
                ..none
            },
            // P3.10 (PS7.2): a mate connector, explicit or implicit.
            ExtrudeField::AxisConnector => PickFilter {
                origin: true,
                faces: true,
                edges: true,
                connectors: true,
                ..none
            },
        });
    }
    match sketch {
        Some(s) if s.waiting_for_plane => Some(PickFilter {
            planar_only: true,
            edges: false,
            regions: false,
            ..PickFilter::modeling(planes)
        }),
        Some(_) => None,
        None => Some(PickFilter::modeling(planes)),
    }
}

/// Modeling mode: a pick toggles the selection; picking nothing clears it. (While the Extrude
/// dialog is open, picks go to it instead.)
#[allow(clippy::too_many_arguments)]
fn apply_pick_requests(
    mut picks: MessageReader<PickRequest>,
    mut selection: ResMut<Selection>,
    extrude: Option<Res<crate::extrude::ExtrudeSession>>,
    boolean: Option<Res<crate::boolean::BooleanSession>>,
    mass: Option<Res<crate::mass_props::MassPanel>>,
    applied: Option<Res<crate::applied::AppliedSession>>,
    create: Option<Res<crate::create_selection::CreateSelection>>,
    insert: Option<Res<crate::assembly::insert::InsertSession>>,
    mate: Option<Res<crate::assembly::mate_dialog::MateSession>>,
    (connector, composite): (
        Option<Res<crate::assembly::connector_tool::ConnectorSession>>,
        Option<Res<crate::composite_ui::CompositeSession>>,
    ),
    pick_override: Res<PickFilterOverride>,
    derived: Option<Res<crate::derived_ui::DerivedSession>>,
    mut row_click: ResMut<FeatureRowClick>,
) {
    // (The mate dialog takes the picks as connectors, P3B.2; the assembly's Mate connector
    // dialog its entities, P3B.7.)
    // The Insert dialog's Standard content tab takes holes from the selection (P3B.5).
    let insert_takes = insert.is_some_and(|s| !s.standard);
    if extrude.is_some()
        || boolean.is_some()
        || mass.is_some()
        || applied.is_some()
        || create.is_some()
        || insert_takes
        || mate.is_some()
        || connector.is_some()
        || pick_override.0.is_some()
        || derived.is_some()
        || composite.is_some()
    {
        picks.clear();
        row_click.pending = None;
        return;
    }
    for p in picks.read() {
        let row = row_click
            .pending
            .take_if(|(r, _)| Some(*r) == p.0)
            .map(|(_, how)| how);
        match (p.0, row) {
            (Some(p), Some(RowSelect::Only)) => {
                selection.0 = if selection.0 == [p] {
                    Vec::new()
                } else {
                    vec![p]
                };
                row_click.anchor = Some(p);
            }
            (Some(p), Some(RowSelect::Range(rows))) => {
                selection.0 = if rows.is_empty() { vec![p] } else { rows }
            }
            (Some(p), Some(RowSelect::Toggle)) => {
                selection.toggle(p);
                row_click.anchor = Some(p);
            }
            (Some(p), None) => selection.toggle(p),
            (None, _) => selection.0.clear(),
        }
    }
}

fn button_index(b: PointerButton) -> usize {
    match b {
        PointerButton::Primary => 0,
        PointerButton::Secondary => 1,
        PointerButton::Middle => 2,
    }
}

/// The nearest default plane under a screen offset, or the origin if the cursor is on it.
pub fn pick(view: &ViewState, offset: Vec2) -> Option<Pick> {
    pick_in(view, offset, true)
}

/// [`pick`], optionally ignoring the planes (hidden with P).
pub fn pick_in(view: &ViewState, offset: Vec2, planes: bool) -> Option<Pick> {
    if view.project(Vec3::ZERO).distance(offset) <= 6.0 {
        return Some(Pick::Origin);
    }
    if !planes {
        return None;
    }
    let (o, d) = view.ray(offset);
    PlaneKind::ALL
        .into_iter()
        .filter_map(|k| ray_square(o, d, Vec3::ZERO, k.u(), k.v(), PLANE_HALF).map(|t| (t, k)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, k)| Pick::Plane(k))
}

/// Keyboard view commands (`reference/onshape/shortcuts.md`, 3D view): Shift+1…7 standard
/// views, N normal to (the sketch plane, a selected plane or a selected sketch's plane), F zoom
/// to fit, Z / Shift+Z zoom out / in, Ctrl+Shift+arrows pan, P shows or hides the default
/// planes, Space clears the selection. (The arrows held turn the view: `arrow_keys_held`.) And
/// Blender's numpad ([`numpad_key`]).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn view_shortcuts(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut view: ResMut<ViewportView>,
    mut selection: ResMut<Selection>,
    kind: Res<ActiveKind>,
    rect: Res<ViewportRect>,
    session: Option<Res<crate::sketch::SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut planes: ResMut<PlanesVisible>,
    inset: Res<DialogInset>,
    mut commands: Commands,
    (cache, asm_parts, pcb_scene, eda2d, layout): (
        Res<crate::parts::PartCache>,
        Res<crate::assembly::AssemblyParts>,
        Option<Res<crate::pcb::view::PcbScene>>,
        Res<crate::eda::Eda2d>,
        Res<crate::eda::layout_tools::LayoutState>,
    ),
) {
    if kind.is_flat() || eda2d.flat() {
        // No 3D views on a sheet or a render; F and Ctrl+S are handled by `crate::drawing`.
        keys_in.clear();
        return;
    }
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    for k in keys_in.read() {
        if k.state != bevy::input::ButtonState::Pressed || typing || !q_dialogs.is_empty() || alt {
            continue;
        }
        // Arrows: with Ctrl+Shift, pan; otherwise they turn the view while held
        // (`arrow_keys_held`).
        let arrow = match k.shortcut_code() {
            KeyCode::ArrowLeft => Some(Vec2::new(-1.0, 0.0)),
            KeyCode::ArrowRight => Some(Vec2::new(1.0, 0.0)),
            KeyCode::ArrowUp => Some(Vec2::new(0.0, -1.0)),
            KeyCode::ArrowDown => Some(Vec2::new(0.0, 1.0)),
            _ => None,
        };
        if let Some(dir) = arrow {
            if ctrl && shift {
                // The scene moves with the arrow, a tenth of the viewport per press.
                let mut to = view.target();
                to.pan(dir * rect.0.size().min_element() * 0.1);
                view.animate_to(to);
            }
            continue;
        }
        match numpad_key(k.shortcut_code(), ctrl) {
            Some(NumpadKey::View(s)) => {
                let to = view.target().oriented(s);
                view.animate_to(to);
                continue;
            }
            Some(NumpadKey::Opposite) => {
                view.turn_by(180.0, 0.0);
                continue;
            }
            Some(NumpadKey::Perspective) => {
                let on = !view.view.perspective;
                commands.queue(move |world: &mut World| {
                    crate::view_options::set_perspective(world, on)
                });
                continue;
            }
            None => {}
        }
        if ctrl {
            continue;
        }
        let standard = if shift {
            match k.shortcut_code() {
                KeyCode::Digit1 => Some(StandardView::Front),
                KeyCode::Digit2 => Some(StandardView::Back),
                KeyCode::Digit3 => Some(StandardView::Left),
                KeyCode::Digit4 => Some(StandardView::Right),
                KeyCode::Digit5 => Some(StandardView::Top),
                KeyCode::Digit6 => Some(StandardView::Bottom),
                KeyCode::Digit7 => Some(StandardView::Isometric),
                _ => None,
            }
        } else {
            None
        };
        if let Some(s) = standard {
            if s == StandardView::Isometric {
                commands.queue(isometric_and_fit);
            } else {
                let to = view.target().oriented(s);
                view.animate_to(to);
            }
            continue;
        }
        match k.shortcut_code() {
            // A board's layout edited in the view: normal to the board, from the top (N) or the
            // bottom (Shift+N).
            KeyCode::KeyN
                if *kind == ActiveKind::PartStudio
                    && let Some(h) = eda2d.plane() =>
            {
                let n = h.frame.normal();
                let n = Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32);
                let to = view.target().normal_to(if shift { -n } else { n });
                view.animate_to(to);
            }
            KeyCode::KeyN if !shift && *kind == ActiveKind::PartStudio => {
                // In a sketch: normal to the sketch plane. Otherwise normal to the selected
                // plane, or to the plane of the selected sketch.
                let sketch_plane = |f: FeatureId| {
                    let p = doc
                        .as_ref()?
                        .active_element()?
                        .feature(f)?
                        .sketch()?
                        .plane?;
                    Some(plane_normal(p))
                };
                let normal = match &session {
                    Some(s) => sketch_plane(s.feature),
                    None => selection
                        .plane()
                        .map(|k| k.normal())
                        .or_else(|| {
                            let el = doc.as_ref()?.active_element()?;
                            selection
                                .face()
                                .and_then(|(f, tag)| {
                                    crate::parts::face_plane_of(el, f.feature, tag)
                                })
                                .map(plane_normal)
                        })
                        .or_else(|| selection.feature().and_then(sketch_plane)),
                };
                if let Some(n) = normal {
                    let to = view.target().normal_to(n);
                    view.animate_to(to);
                }
            }
            // A board's layout edited in the view: F flips the selected footprint
            // (`crate::eda::layout_tools`).
            KeyCode::KeyF
                if !shift
                    && eda2d.plane().is_some()
                    && layout
                        .selection
                        .iter()
                        .any(|i| matches!(i, cadrs_eda::board_edit::BoardItem::Footprint(_))) => {}
            KeyCode::KeyF if !shift => {
                let mut pts = scene_points(doc.as_deref(), *kind, planes.0, Some(&cache));
                // In an assembly with a mate dialog's or a drag's preview, the instances as drawn
                // (Final part 3: `course_asm_variables` 02 was fitted to the pin where it was
                // before the pick); else the shown occurrences, as before (Final part 4: the
                // drawn parts include hidden ones, so the fit shrank).
                if *kind == ActiveKind::Assembly && !asm_parts.preview.is_empty() {
                    let drawn: Vec<Vec3> = cache
                        .shown()
                        .flat_map(|p| {
                            p.solid
                                .positions
                                .iter()
                                .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
                        })
                        .collect();
                    if !drawn.is_empty() {
                        pts = drawn;
                    }
                }
                // PCB Studio: the board shown (as the zoom-to-fit button does; P3H.6 fix round 1,
                // F framed the origin).
                if *kind == ActiveKind::PcbStudio
                    && let Some(scene) = pcb_scene.as_deref()
                {
                    pts = scene.fit_points();
                }
                let to = view
                    .target()
                    .fitted_beside(&pts, rect.0.size(), fit_fill(*kind), inset.0);
                view.animate_to(to);
            }
            KeyCode::KeyZ => {
                let factor = if shift {
                    ZOOM_KEY_FACTOR
                } else {
                    1.0 / ZOOM_KEY_FACTOR
                };
                let to = view.target().zoomed(factor);
                view.animate_to(to);
            }
            KeyCode::KeyP if !shift && *kind == ActiveKind::PartStudio => {
                planes.toggle_all();
            }
            KeyCode::Space if !shift => selection.0.clear(),
            _ => {}
        }
    }
}

/// What zoom to fit frames in the active tab: the planes shown, the parts and the sketches (an
/// Assembly's instances as drawn, a PCB Studio's board).
fn fit_points(world: &World) -> Vec<Vec3> {
    let kind = *world.resource::<ActiveKind>();
    let planes = world.resource::<PlanesVisible>().0;
    let mut pts = scene_points(
        world.get_resource::<ActiveDocument>(),
        kind,
        planes,
        world.get_resource::<crate::parts::PartCache>(),
    );
    if kind == ActiveKind::Assembly {
        // The instances as drawn: a mate dialog's or a drag's placements included.
        let cache = world.resource::<crate::parts::PartCache>();
        let drawn: Vec<Vec3> = cache
            .shown()
            .flat_map(|p| {
                p.solid
                    .positions
                    .iter()
                    .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
            })
            .collect();
        if !drawn.is_empty() {
            pts = drawn;
        }
    }
    if kind == ActiveKind::PcbStudio {
        // P3H.3: the board shown.
        pts = world.resource::<crate::pcb::view::PcbScene>().fit_points();
    }
    pts
}

/// Zoom to fit (F, and the context menus): frames the planes and the sketches.
pub fn zoom_to_fit(world: &mut World) {
    orient_and_fit(world, |v| v);
}

/// Turns the view (`orient` from the current one) and zooms to fit, as the view menu's
/// isometric, dimetric and trimetric views and Shift+7 do. With nothing built in the tab, the
/// isometric view frames the default planes ([`fitted_isometric_for`]).
pub fn orient_and_fit(world: &mut World, orient: impl FnOnce(ViewState) -> ViewState) {
    let kind = *world.resource::<ActiveKind>();
    let pts = fit_points(world);
    let size = world.resource::<ViewportRect>().0.size();
    let inset = world.resource::<DialogInset>().0;
    let mut view = world.resource_mut::<ViewportView>();
    let to = orient(view.target()).fitted_beside(&pts, size, fit_fill(kind), inset);
    view.animate_to(to);
}

/// The isometric view, fitted: to what the tab shows, or (nothing built yet, so the planes
/// alone would leave it far zoomed in or out) to the default planes as before.
pub fn isometric_and_fit(world: &mut World) {
    let built = !world.resource::<crate::parts::PartCache>().parts.is_empty();
    if built {
        orient_and_fit(world, |v| v.oriented(StandardView::Isometric));
    } else {
        let size = world.resource::<ViewportRect>().0.size();
        let mut view = world.resource_mut::<ViewportView>();
        let to = fitted_isometric_for(view.view.perspective, size);
        view.animate_to(to);
    }
}

/// View normal to the sketch plane (the dimension menu; N while sketching).
pub fn normal_to_sketch(world: &mut World) {
    let plane = crate::sketch_tools::session_plane(
        world.get_resource::<crate::sketch::SketchSession>(),
        world.get_resource::<ActiveDocument>(),
    );
    if let Some(p) = plane {
        let mut view = world.resource_mut::<ViewportView>();
        let to = view.target().normal_to(plane_normal(p));
        view.animate_to(to);
    }
}

/// A sketch plane's normal (the side it faces).
pub fn plane_normal(p: PlaneRef) -> Vec3 {
    let n = p.frame().normal();
    Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32)
}

/// What a numpad key does, as in Blender ([`numpad_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumpadKey {
    /// A standard view.
    View(StandardView),
    /// The view turned round to its other side, about the screen's up axis.
    Opposite,
    /// Perspective or orthographic, whichever the view isn't.
    Perspective,
}

/// What a numpad key does, as in Blender (the keys themselves, Num Lock on or off): 7 Top, 1
/// Front, 3 Right, with Ctrl the opposite (Bottom, Back, Left); 9 the other side; 5 perspective
/// or orthographic. (8, 2, 4 and 6 turn the view as the arrows: [`held_direction`].)
pub fn numpad_key(key: KeyCode, ctrl: bool) -> Option<NumpadKey> {
    let (view, opposite) = match key {
        KeyCode::Numpad7 => (StandardView::Top, StandardView::Bottom),
        KeyCode::Numpad1 => (StandardView::Front, StandardView::Back),
        KeyCode::Numpad3 => (StandardView::Right, StandardView::Left),
        KeyCode::Numpad9 if !ctrl => return Some(NumpadKey::Opposite),
        KeyCode::Numpad5 if !ctrl => return Some(NumpadKey::Perspective),
        _ => return None,
    };
    Some(NumpadKey::View(if ctrl { opposite } else { view }))
}

/// Which way the keys held turn the view (x right, y down, each -1, 0 or 1): the arrows, and
/// Blender's numpad 8, 2, 4 and 6 (Num Lock on or off).
pub fn held_direction(keys: &ButtonInput<KeyCode>) -> Vec2 {
    let key = |k: [KeyCode; 2]| f32::from(u8::from(keys.any_pressed(k)));
    Vec2::new(
        key([KeyCode::ArrowRight, KeyCode::Numpad6]) - key([KeyCode::ArrowLeft, KeyCode::Numpad4]),
        key([KeyCode::ArrowDown, KeyCode::Numpad2]) - key([KeyCode::ArrowUp, KeyCode::Numpad8]),
    )
}

/// The arrow keys held down, turning the view ([`hold_arrows`]).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct ArrowHold {
    /// Degrees turned since the first of them was pressed.
    turned: f32,
    /// While any is held: the way they turn the view (x right, y down, unit) and the step a
    /// press turns at least (degrees).
    held: Option<(Vec2, f32)>,
}

/// Degrees a second held arrows turn the view: 90 (Shift 180, Ctrl 30).
pub fn arrow_rate(shift: bool, ctrl: bool) -> f32 {
    if shift {
        180.0
    } else if ctrl {
        30.0
    } else {
        90.0
    }
}

/// Seconds the view takes to catch up with held arrows' turn: short, so it stops soon after
/// they're let go.
const HELD_TURN_TIME: f32 = 0.15;

/// A frame of the arrow keys (`held`: the ones held, x right, y down, each -1, 0 or 1; `dt`
/// seconds). From the first pressed to the last let go, the view turns their way at
/// [`arrow_rate`], diagonally with two, a press turning at least its step ([`arrow_step`]: a tap
/// turns 15°). (#91: by the system's key repeats, it waited out their delay, then turned in
/// steps.)
pub fn hold_arrows(
    view: &mut ViewportView,
    hold: &mut ArrowHold,
    held: Vec2,
    shift: bool,
    ctrl: bool,
    dt: f32,
) {
    if held != Vec2::ZERO {
        let dir = held.normalize();
        let d = dir * arrow_rate(shift, ctrl) * dt;
        view.turn_by_in(d.x, d.y, HELD_TURN_TIME);
        hold.turned += d.length();
        hold.held = Some((dir, arrow_step(shift, ctrl)));
    } else if let Some((dir, step)) = hold.held.take() {
        let short = step - hold.turned;
        if short > 0.0 {
            view.turn_by(dir.x * short, dir.y * short);
        }
        hold.turned = 0.0;
    }
}

/// The arrow keys (and the numpad's, [`held_direction`]) held down turn the view
/// ([`hold_arrows`]); with Ctrl+Shift the arrows pan instead (`view_shortcuts`).
#[allow(clippy::too_many_arguments)]
fn arrow_keys_held(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    kind: Res<ActiveKind>,
    eda2d: Res<crate::eda::Eda2d>,
    time: Res<Time>,
    mut view: ResMut<ViewportView>,
    mut hold: ResMut<ArrowHold>,
) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let off = kind.is_flat()
        || eda2d.flat()
        || focus.get().is_some_and(|e| q_fields.contains(e))
        || !q_dialogs.is_empty()
        || alt
        || (ctrl && shift);
    let held = if off {
        Vec2::ZERO
    } else {
        held_direction(&keys)
    };
    // (Nothing held nor let go: the view isn't touched, for its change detection.)
    if held == Vec2::ZERO && hold.held.is_none() {
        return;
    }
    hold_arrows(&mut view, &mut hold, held, shift, ctrl, time.delta_secs());
}

/// Degrees an arrow key rotates the view: 15°, Shift 90°, Ctrl 5° (as the view cube arrows).
pub fn arrow_step(shift: bool, ctrl: bool) -> f32 {
    if shift {
        90.0
    } else if ctrl {
        5.0
    } else {
        15.0
    }
}

/// How much of the viewport zoom to fit fills.
pub const FIT_FILL: f32 = 0.8;
/// ... in an Assembly tab, which Onshape frames with a wider margin (`ex3-step5.png`,
/// `ex3-step16.png`; P3B.4 judge).
pub const ASM_FIT_FILL: f32 = 0.6;

/// The zoom to fit fill for a tab of `kind`.
pub fn fit_fill(kind: ActiveKind) -> f32 {
    if kind == ActiveKind::Assembly {
        ASM_FIT_FILL
    } else {
        FIT_FILL
    }
}
/// Zoom per Z / Shift+Z press.
pub const ZOOM_KEY_FACTOR: f32 = 1.25;

/// A drag zoom (Shift+middle in Onshape): the zoom factor per pixel dragged up.
pub const DRAG_ZOOM_PER_PX: f32 = 1.006;

/// The points zoom to fit frames: the default planes (the ones shown) and every sketch of the
/// active Part Studio (the origin for an Assembly).
///
/// The parts come from `cache` when it holds the active tab's (rebuilt on the rebuild thread);
/// otherwise they are rebuilt here, which waits for the rebuild thread.
pub fn scene_points(
    doc: Option<&ActiveDocument>,
    kind: ActiveKind,
    planes: [bool; 3],
    cache: Option<&crate::parts::PartCache>,
) -> Vec<Vec3> {
    let mut pts = Vec::new();
    if kind == ActiveKind::PcbStudio {
        // The board is framed by `crate::pcb::view` (zoom_to_fit uses its box).
        return vec![Vec3::ZERO];
    }
    let active = doc.and_then(|d| d.active);
    let to_vec3 = |p: &cadrs_core::Part| -> Vec<Vec3> {
        p.solid
            .positions
            .iter()
            .map(|q| Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32))
            .collect()
    };
    if kind == ActiveKind::Assembly {
        // The shown instances, so they fill the view; the origin only when there are none.
        match cache.filter(|c| c.assembly.is_some() && c.assembly == active && !c.rebuilding) {
            Some(c) => pts.extend(c.shown().flat_map(to_vec3)),
            None => {
                if let Some(d) = doc {
                    pts.extend(crate::assembly::shown_points(d));
                }
            }
        }
        if pts.is_empty() {
            pts.push(Vec3::ZERO);
        }
        return pts;
    }
    // A CAM Studio shows no planes: its parts alone are framed.
    let planes = if kind == ActiveKind::CamStudio {
        [false; 3]
    } else {
        planes
    };
    {
        for k in PlaneKind::ALL.into_iter().filter(|k| planes[k.index()]) {
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                pts.push((k.u() * a + k.v() * b) * PLANE_HALF);
            }
        }
    }
    let Some(el) = doc.and_then(|d| d.active_element()) else {
        return pts;
    };
    match cache.filter(|c| !c.rebuilding && c.settled().map(|(id, ..)| id) == Some(el.id)) {
        Some(c) => pts.extend(c.parts.iter().flat_map(to_vec3)),
        None => {
            for part in cadrs_core::parts::parts(el.features()) {
                pts.extend(to_vec3(&part));
            }
        }
    }
    // Only the sketches shown (P3.8: a consumed sketch's far-off arc centre, such as the
    // Reflector's revolve profile 400 mm up, doesn't count).
    let hidden = crate::parts::hidden_sketches(el, el.features(), None);
    for f in el.features() {
        if hidden.contains(&f.id) {
            continue;
        }
        let Some(sk) = f.sketch() else { continue };
        let Some(plane) = sk.plane else { continue };
        let frame = plane.frame();
        let w = |p: cadrs_sketch::Vec2| {
            let q = frame.to_world(p);
            Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)
        };
        for (_, p) in &sk.geometry.points {
            pts.push(w(p.pos));
        }
        for id in sk.geometry.curves.keys() {
            pts.extend(
                cadrs_sketch::hit::curve_polyline(&sk.geometry, id)
                    .into_iter()
                    .map(w),
            );
        }
    }
    if pts.is_empty() {
        pts.push(Vec3::ZERO);
    }
    pts
}

fn animate_view(
    time: Res<Time>,
    mut finish: MessageReader<FinishAnimations>,
    mut view: ResMut<ViewportView>,
) {
    let finish_all = finish.read().count() > 0;
    if view.animation.is_some() {
        view.advance(time.delta_secs(), finish_all);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hover(
    drag: Res<ViewportDrag>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    kind: Res<ActiveKind>,
    q_rows: Query<(&PickRow, &Hovered)>,
    mut highlight: ResMut<PlaneHighlight>,
    session: Option<Res<crate::sketch::SketchSession>>,
    planes: Res<PlanesVisible>,
    hover_override: Res<HoverOverride>,
    (parts, extrude, applied, create, pick_override, triad): (
        Res<crate::parts::PartCache>,
        Option<Res<crate::extrude::ExtrudeSession>>,
        Option<Res<crate::applied::AppliedSession>>,
        Option<Res<crate::create_selection::CreateSelection>>,
        Res<PickFilterOverride>,
        Option<Res<crate::assembly::triad::Triad>>,
    ),
    (zoom_window, section_arrow, eda2d, fit, asm_parts): (
        Option<Res<crate::view_options::ZoomWindow>>,
        Res<crate::section_view::SectionArrow>,
        Res<crate::eda::Eda2d>,
        Res<crate::board_fit::FitState>,
        Res<crate::assembly::AssemblyParts>,
    ),
    mut last: Local<Option<(Vec2, crate::parts::PickFilter, Option<Pick>)>>,
    box_select: Option<Res<crate::assembly::box_select::AssemblyBox>>,
) {
    // A box drag only updates its rectangle. Avoid picking and rebuilding hover tints for
    // every part the pointer crosses; normal hover resumes once the drag ends.
    if box_select.is_some_and(|b| b.active()) {
        *last = None;
        if highlight.viewport.is_some() || highlight.list.is_some() {
            *highlight = PlaneHighlight::default();
        }
        return;
    }
    // While a sketch has its plane, planes no longer react to the pointer (the sketch tools
    // own it); while it waits for one, planes highlight as pick candidates.
    let filter = pick_override.0.or(pick_filter(
        planes.0,
        session.as_deref(),
        extrude.as_deref(),
        applied.as_deref(),
        create.as_deref(),
    ));
    let picking = filter.is_some();
    // Zoom to window's box and the section plane's arrow own the pointer: nothing under it
    // highlights (P3E.3a).
    // A board's layout edited in the view owns the pointer too, unless Align waits for a pick.
    let layout_edit = eda2d.plane().is_some() && fit.align == crate::board_fit::AlignStep::Off;
    let over = pointer_over_viewport(&hover, &q_area)
        && zoom_window.is_none()
        && section_arrow.drag.is_none()
        && !layout_edit;
    let query = match filter {
        Some(f) if over && !drag.navigating && *kind == ActiveKind::PartStudio => Some(f),
        // No pre-highlight while a triad drag or its value box is active (P3H.6 judge), nor
        // while the parts are drawn moved from their solids (a drag, an animation: picking
        // would find them where they were).
        Some(f)
            if over
                && !drag.navigating
                && *kind == ActiveKind::Assembly
                && !triad.as_deref().is_some_and(|t| t.busy())
                && !asm_parts.live =>
        {
            Some(if pick_override.0.is_some() {
                f
            } else {
                crate::assembly::pick_filter()
            })
        }
        _ => None,
    };
    if query.is_none() {
        // The parts or the view may change unseen meanwhile.
        *last = None;
    }
    // Picking walks every part's edges and triangles: pick again only when the pointer, the
    // filter, the view or the parts changed, not every frame the pointer rests over the view.
    let viewport = query.and_then(|f| {
        let offset = rect.offset(drag.pointer);
        if let Some((o, lf, picked)) = *last
            && o == offset
            && lf == f
            && !parts.is_changed()
            && !view.is_changed()
        {
            return picked;
        }
        let picked = crate::parts::pick_scene(&parts, &view.view, offset, f);
        *last = Some((offset, f, picked));
        picked
    });
    let viewport = hover_override.0.or(viewport);
    let list = q_rows
        .iter()
        .find(|(r, h)| h.get() && (picking || matches!(r.0, Pick::Feature(_) | Pick::Part(_))))
        .map(|(r, _)| r.0);
    let new = PlaneHighlight { viewport, list };
    if highlight.viewport != new.viewport || highlight.list != new.list {
        *highlight = new;
    }
}

#[allow(clippy::type_complexity)]
pub fn apply_view_to_camera(
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    mut q: Query<
        (&Camera, &mut Transform, &mut Projection),
        Or<(With<MainCamera>, With<OverlayCamera>)>,
    >,
) {
    for (camera, mut t, mut projection) in &mut q {
        let v = view.view;
        let new_t = Transform::from_translation(v.camera_position()).with_rotation(v.rotation());
        if *t != new_t {
            *t = new_t;
        }
        let Some(size) = camera.logical_target_size() else {
            continue;
        };
        // P3E.3a: the perspective view's off-centre projection, the focus at the viewport's
        // centre (`crate::view_options::ViewportPerspective`).
        if v.perspective {
            let p = crate::view_options::ViewportPerspective::of(&v, rect.0.center(), size);
            let same = matches!(&*projection, Projection::Custom(c) if c.get::<crate::view_options::ViewportPerspective>() == Some(&p));
            if !same {
                *projection = Projection::custom(p);
            }
            continue;
        }
        if !matches!(&*projection, Projection::Orthographic(_)) {
            *projection = ortho_projection(v.scale);
        }
        if let Projection::Orthographic(o) = &mut *projection {
            let c = rect.0.center();
            let origin = Vec2::new(c.x / size.x, 1.0 - c.y / size.y);
            if o.scale != v.scale || o.viewport_origin != origin {
                o.scale = v.scale;
                o.far = camera::camera_far(v.scale);
                o.viewport_origin = origin;
            }
        }
    }
}

fn sync_scene_visibility(
    kind: Res<ActiveKind>,
    planes: Res<PlanesVisible>,
    mut q_planes: Query<(&PlaneKind, &mut Visibility)>,
) {
    for (k, mut v) in &mut q_planes {
        let vis = if *kind == ActiveKind::PartStudio && planes.shows(*k) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        v.set_if_neq(vis);
    }
}

fn style_planes(
    selection: Res<Selection>,
    materials: Option<Res<PlaneMaterials>>,
    mut q: Query<(&PlaneKind, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(m) = materials else {
        return;
    };
    for (k, mut mat) in &mut q {
        let want = if selection.contains(Pick::Plane(*k)) {
            &m.selected
        } else {
            &m.normal
        };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_plane_edges(
    mut gizmos: Gizmos,
    mut hl_gizmos: Gizmos<HighlightGizmos>,
    mut hover_gizmos: Gizmos<HoverGizmos>,
    mut edge_on_gizmos: Gizmos<EdgeOnGizmos>,
    q: Query<(&PlaneKind, &GlobalTransform, &InheritedVisibility)>,
    // The scheme's (gizmos aren't mapped like the UI).
    theme: Res<cadrs_ui::color_scheme::ActiveTheme>,
    highlight: Res<PlaneHighlight>,
    selection: Res<Selection>,
    view: Res<ViewportView>,
    section: Res<crate::section_view::SectionClip>,
    mut section_gizmos: Gizmos<crate::section_view::SectionPlaneGizmos>,
) {
    // A section view doesn't cut the planes (they stay whole, as Onshape's); a selected one is
    // depth-tested while a section is on, so the kept part hides it.
    let clip: Option<crate::section_view::Cut> = None;
    let sectioned = section.plane.is_some();
    use crate::section_view::clipped_line as cut;
    for (kind, t, vis) in &q {
        if !vis.get() {
            continue;
        }
        // Seen edge-on (as in a normal-to view), a plane is a darker 2 px line through the
        // origin, like Onshape's (measured about #96a9b8 in `screens/09`).
        let edge_on = kind.normal().dot(view.view.back()).abs() < 0.02;
        let h = PLANE_HALF;
        let corners = [
            Vec3::new(-h, -h, 0.0),
            Vec3::new(h, -h, 0.0),
            Vec3::new(h, h, 0.0),
            Vec3::new(-h, h, 0.0),
        ]
        .map(|c| t.transform_point(c));
        let p = Pick::Plane(*kind);
        let (selected, hovered) = (selection.contains(p), highlight.is_hovered(p));
        if edge_on && !hovered && !selected {
            // Draw the edge-on plane as one line through the origin (not its near outline
            // edge), so parts in front of it hide it and their edges stay black.
            let back = view.view.back();
            let x = t.affine().transform_vector3(Vec3::X);
            let y = t.affine().transform_vector3(Vec3::Y);
            let along = if x.dot(back).abs() > y.dot(back).abs() {
                Vec3::Y
            } else {
                Vec3::X
            };
            cut(
                &mut edge_on_gizmos,
                clip,
                t.transform_point(-along * h),
                t.transform_point(along * h),
                Color::srgb_u8(0x79, 0xa1, 0xcc),
            );
            continue;
        }
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            if hovered {
                // Hover: a thin orange outline (1.5 px, like the plane edges).
                cut(&mut hover_gizmos, clip, a, b, theme.highlight);
            } else if selected && sectioned {
                // In a section view (the picked plane, P3E.3b) depth-tested, so the kept part
                // hides it where it is in front.
                cut(&mut section_gizmos, clip, a, b, theme.selection_3d);
            } else if selected {
                cut(&mut hl_gizmos, clip, a, b, theme.selection_3d);
            } else if edge_on {
                // The sketch axes in a normal-to view: `#79a1cc` (`screens/10`).
                cut(
                    &mut edge_on_gizmos,
                    clip,
                    a,
                    b,
                    Color::srgb_u8(0x79, 0xa1, 0xcc),
                );
            } else {
                cut(&mut gizmos, clip, a, b, theme.plane_edge_line());
            }
        }
    }
}

trait PlaneEdge {
    fn plane_edge_line(&self) -> Color;
}

impl PlaneEdge for Theme {
    /// The outline color: a cyan grey in light, the schematic's warm grid colour in dark.
    fn plane_edge_line(&self) -> Color {
        self.plane_edge
    }
}

/// The ground grid's step follows the zoom (1, 2 or 5 times a power of ten mm, lines at least
/// 18 px apart, [`crate::ground_grid::step_for`]) and it fades out past the view; its quad
/// follows the focus. Its colours follow the scheme: in dark, the schematic's faint warm lines
/// and crosses, its salmon and green for the axes.
fn fit_grid(
    view: Res<ViewportView>,
    q: Query<(&MeshMaterial3d<GroundGridMaterial>, &mut Transform), With<GroundGrid>>,
    mut materials: ResMut<Assets<GroundGridMaterial>>,
    mut last: Local<Option<(f32, f32, bool, Vec3)>>,
) {
    let v = &view.view;
    let step = crate::ground_grid::step_for(v.scale);
    // Fades linearly from the eye: about 70 % at the focus, gone a few view sizes beyond it.
    let fade = if v.perspective {
        v.eye_distance() * 4.0
    } else {
        crate::camera::camera_distance(v.scale) / 0.3
    };
    let dark = cadrs_ui::dark_in_force();
    let key = (step, fade, dark, v.focus);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    let rgba = |r: u8, g: u8, b: u8, a: f32| Color::srgba_u8(r, g, b, (a * 255.0) as u8);
    let (line, cross, x, y) = if dark {
        // The schematic's (`cadrs_eda::render`, Horizon's theme).
        (
            rgba(0x5a, 0x52, 0x3e, 0.16),
            rgba(0x8a, 0x80, 0x66, 0.5),
            rgba(0xe6, 0x7b, 0x54, 0.8),
            rgba(0x73, 0xbc, 0x74, 0.8),
        )
    } else {
        (
            rgba(0x8f, 0xb3, 0xb8, 0.28),
            rgba(0x4f, 0x80, 0x87, 0.6),
            rgba(0xe8, 0x85, 0x7f, 1.0),
            rgba(0x7f, 0xc2, 0x7a, 1.0),
        )
    };
    let lin = |c: Color| c.to_linear().to_vec4();
    let params = GroundGridParams {
        line: lin(line),
        cross: lin(cross),
        x_axis: lin(x),
        y_axis: lin(y),
        grid: Vec4::new(
            1.0 / step,
            1.0 / fade,
            crate::ground_grid::CROSS_EVERY,
            crate::ground_grid::CROSS_ARM,
        ),
        // Seen edge-on, far away, the lines would crowd into a hatch: they fade first.
        fade: Vec4::new(2.0, 0.0, 0.0, 0.0),
    };
    for (m, mut t) in q {
        if let Some(mut m) = materials.get_mut(&m.0)
            && m.params != params
        {
            m.params = params;
        }
        // A quad as wide as the grid is seen, around the focus.
        let want =
            Transform::from_xyz(v.focus.x, v.focus.y, 0.0).with_scale(Vec3::new(fade, fade, 1.0));
        if *t != want {
            *t = want;
        }
    }
}

fn sync_grid(grid: Res<GridVisible>, mut q: Query<&mut Visibility, With<GroundGrid>>) {
    for mut v in &mut q {
        v.set_if_neq(if grid.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Feature-list rows show the selection.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_plane_rows(
    selection: Res<Selection>,
    kind: Res<ActiveKind>,
    planes: Res<PlanesVisible>,
    theme: Res<Theme>,
    mut q: Query<(
        Entity,
        &PickRow,
        Has<Selected>,
        Option<&mut cadrs_ui::Visuals>,
    )>,
    mut q_toggle: Query<&mut cadrs_ui::TreeRowToggle>,
    children: Query<&Children>,
    mut q_img: Query<&mut ImageNode>,
    (q_eye, q_parent): (Query<(), With<cadrs_ui::TreeRowToggle>>, Query<&ChildOf>),
    mut commands: Commands,
) {
    // Hidden planes (P) grey out their rows, text and icon (`intro-to-sketching/ex1-step1.png`).
    let hidden_fg = Color::srgb_u8(0xb4, 0xb4, 0xb4);
    for (e, row, has, visuals) in &mut q {
        // In an assembly, an instance's row is highlighted while a face, edge or vertex of it
        // is selected (P3B.1, `intro-to-assemblies/ex1-step5.png`).
        let want = selection.contains(row.0)
            || (*kind == ActiveKind::Assembly
                && matches!(row.0, Pick::Part(_))
                && selection.0.iter().any(|p| p.part() == row.0.part()));
        if want && !has {
            commands.entity(e).try_insert(Selected);
        } else if !want && has {
            commands.entity(e).try_remove::<Selected>();
        }
        let Pick::Plane(k) = row.0 else { continue };
        let shown = planes.shows(k);
        for mut t in &mut q_toggle {
            if t.row == e && t.on != shown {
                t.on = shown;
            }
        }
        let fg = if shown { theme.foreground } else { hidden_fg };
        if let Some(mut v) = visuals
            && v.foreground.normal != fg
        {
            v.foreground = cadrs_ui::StateColors::new(fg, fg, fg, theme.disabled_foreground);
            let icon_fg = if shown { theme.feature_icon } else { hidden_fg };
            for d in children.iter_descendants(e) {
                // The eye toggle keeps its own colour.
                if q_eye.contains(d) || q_parent.get(d).is_ok_and(|p| q_eye.contains(p.parent())) {
                    continue;
                }
                if let Ok(mut img) = q_img.get_mut(d) {
                    img.color = icon_fg;
                }
            }
        }
    }
}

/// Splits a 2×2 linear map (columns `a`, `b`, screen y down) into rotation · scale · rotation,
/// which is what two nested `UiTransform`s can express. Returns (outer angle, scale, inner
/// angle).
pub fn affine_parts(a: Vec2, b: Vec2) -> (f32, Vec2, f32) {
    // M = [[a.x, b.x], [a.y, b.y]]
    let (m00, m01, m10, m11) = (a.x, b.x, a.y, b.y);
    let e = (m00 + m11) / 2.0;
    let f = (m00 - m11) / 2.0;
    let g = (m10 + m01) / 2.0;
    let h = (m10 - m01) / 2.0;
    let q = (e * e + h * h).sqrt();
    let r = (f * f + g * g).sqrt();
    let a1 = g.atan2(f);
    let a2 = h.atan2(e);
    let theta = (a2 - a1) / 2.0;
    let phi = (a2 + a1) / 2.0;
    (phi, Vec2::new(q + r, q - r), theta)
}

/// Places an affine label: the text's top-left corner at `corner` (screen px, relative to the
/// label's parent) plus `pad` text pixels, with text x along `a` and text y along `b`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_affine(
    node: &mut Node,
    transform: &mut UiTransform,
    inner: &mut UiTransform,
    size: Vec2,
    corner: Vec2,
    a: Vec2,
    b: Vec2,
    pad: Vec2,
) {
    let local_center = pad + size / 2.0;
    let center = corner + a * local_center.x + b * local_center.y;
    let (outer, scale, inner_angle) = affine_parts(a, b);
    let left = Val::Px(center.x - size.x / 2.0);
    let top = Val::Px(center.y - size.y / 2.0);
    if node.left != left || node.top != top {
        node.left = left;
        node.top = top;
    }
    let t = UiTransform {
        rotation: Rot2::radians(outer),
        scale,
        ..default()
    };
    if *transform != t {
        *transform = t;
    }
    let ti = UiTransform {
        rotation: Rot2::radians(inner_angle),
        ..default()
    };
    if *inner != ti {
        *inner = ti;
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_plane_labels(
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    kind: Res<ActiveKind>,
    theme: Res<Theme>,
    mut q: Query<
        (
            &PlaneLabel,
            &Children,
            &mut Node,
            &mut UiTransform,
            &mut Visibility,
        ),
        Without<AffineInner>,
    >,
    mut q_inner: Query<
        (&ComputedNode, &mut UiTransform, &mut TextColor, &mut Text),
        With<AffineInner>,
    >,
    sketch: Res<crate::sketch_tools::SketchScreen>,
    planes: Res<PlanesVisible>,
    parts: Option<Res<crate::parts::PartCache>>,
    meshes: Res<Assets<Mesh>>,
    fills: Query<(&Mesh3d, &InheritedVisibility), With<crate::plane_display::LabelOccluder>>,
    mut part_bounds: Local<Option<Vec<([f64; 3], [f64; 3])>>>,
) {
    let v = view.view;
    let sketch_plane = sketch.active.map(|m| m.plane);
    if parts.as_ref().is_none_or(|p| p.is_changed()) {
        *part_bounds = None;
    }
    // The parts' screen bounds (viewport-relative): a label over a part would draw over its
    // edges, so it hides. From the corners of each part's 3D box (a little larger than its
    // outline; projecting every vertex of every part each frame cost a large assembly most of
    // its frame). Worked out once a label is shown (an assembly shows none, and building every
    // part's boxes cost a large one half a second on each switch to its tab).
    let mut part_boxes: Option<Vec<Rect>> = None;
    // (Each part's 3D box kept until the parts change: reading every part's each frame cost
    // a Part Studio of hundreds of parts a cache miss or two apiece.)
    let mut boxes = || -> Vec<Rect> {
        part_bounds
            .get_or_insert_with(|| {
                parts
                    .iter()
                    .flat_map(|c| c.parts.iter())
                    .filter_map(|part| part.solid.pick_index().bounds)
                    .collect()
            })
            .iter()
            .filter_map(|&(lo, hi)| {
                let mut pts = (0..8).map(|k| {
                    let c = [
                        if k & 1 == 0 { lo[0] } else { hi[0] },
                        if k & 2 == 0 { lo[1] } else { hi[1] },
                        if k & 4 == 0 { lo[2] } else { hi[2] },
                    ];
                    rect.to_screen(v.project(Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32)))
                        - rect.0.min
                });
                let first = pts.next()?;
                Some(pts.fold(Rect::from_corners(first, first), |r, p| r.union_point(p)))
            })
            .collect()
    };
    for (label, children, mut node, mut transform, mut vis) in &mut q {
        let Some(&child) = children.first() else {
            continue;
        };
        let Ok((inner_node, mut inner_t, mut color, mut text)) = q_inner.get_mut(child) else {
            continue;
        };
        let k = label.0;
        // Text axes on screen, normalized so labels keep a constant size when zooming, and
        // turned so the text is never mirrored (a plane seen from behind) or upside down.
        let (u, w) = readable_axes(&v, k.u(), k.v());
        let a = v.project_vector(u) * v.scale;
        let b = v.project_vector(-w) * v.scale;
        // Hide the label once the plane is more than about 70° from facing the viewer (the
        // squashed text smears), fading in over a few degrees. Seen from behind, it names that
        // side (the text turned to read: `readable_axes`).
        let (name, alpha) = k.label(&v);
        if text.0 != name {
            text.0 = name.to_string();
        }
        // The plane being sketched on keeps its label (`screens/08`) except when viewed
        // straight on, where the sketch covers it (`screens/09`).
        let normal_view = k.normal().dot(v.back()).abs() > 0.999;
        let show = *kind == ActiveKind::PartStudio
            && planes.shows(k)
            && alpha > 0.0
            && !(sketch_plane == Some(k.plane_ref()) && normal_view);
        vis.set_if_neq(if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if !show {
            continue;
        }
        let size = inner_node.size() * inner_node.inverse_scale_factor();
        // Screen position relative to the viewport area (the labels' parent).
        let corner = rect.to_screen(v.project((w - u) * PLANE_HALF)) - rect.0.min;
        let pad = Vec2::new(3.0, 1.0);
        let label_box = [
            Vec2::ZERO,
            Vec2::new(size.x, 0.0),
            Vec2::new(0.0, size.y),
            size,
        ]
        .map(|q| corner + a * (pad.x + q.x) + b * (pad.y + q.y));
        // Clipped to the viewport: a rotated label's glyphs escape the area's clip and drew
        // over the toolbar (P3I.2 judge, `sheetmetal_extrude`), so a label not wholly inside
        // hides, as the Plane features' and the sketch plane's do.
        let inside = Rect::from_corners(Vec2::ZERO, rect.0.size());
        if label_box.iter().any(|p| !inside.contains(*p)) {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let label_box = label_box[1..]
            .iter()
            .fold(Rect::from_corners(label_box[0], label_box[0]), |r, p| {
                r.union_point(*p)
            });
        // A sketch's region fill in front of the label hides it too (`course_ps21_funnel` 05).
        let world =
            |q: Vec2| (w - u) * PLANE_HALF + (u * (pad.x + q.x) - w * (pad.y + q.y)) * v.scale;
        let behind_fill = [
            Vec2::ZERO,
            Vec2::new(size.x, 0.0),
            Vec2::new(0.0, size.y),
            size,
            size / 2.0,
        ]
        .iter()
        .any(|q| {
            let p = world(*q);
            let (ro, rd) = v.ray(v.project(p));
            crate::plane_display::fill_in_front(&meshes, &fills, ro, rd, (p - ro).dot(rd))
        });
        // A label running out of the view hides: the UI's clip doesn't follow its turned
        // text, so it drew over the toolbar (P3E.3a judge, `course_td_render_modes` 20).
        let outside = !Rect::from_corners(Vec2::ZERO, rect.0.size()).contains(label_box.min)
            || !Rect::from_corners(Vec2::ZERO, rect.0.size()).contains(label_box.max);
        if behind_fill
            || outside
            || part_boxes
                .get_or_insert_with(&mut boxes)
                .iter()
                .any(|r| !r.intersect(label_box).is_empty())
        {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        place_affine(
            &mut node,
            &mut transform,
            &mut inner_t,
            size,
            corner,
            a,
            b,
            pad,
        );
        // Labels stay blue; hover and selection only change the outline.
        let c = theme.plane_label.with_alpha(alpha);
        if color.0 != c {
            color.0 = c;
        }
    }
}

/// Label opacity for a plane whose front side makes `facing` = cos(angle) with the direction
/// to the viewer: hidden beyond about 70° (cos 0.342) and from behind (negative), fully shown
/// from about 66° in.
pub fn label_alpha(facing: f32) -> f32 {
    ((facing - 0.342) / 0.06).clamp(0.0, 1.0)
}

/// The in-plane text axes (reading direction, up) for a label on a plane with axes `u`, `v`:
/// as given when the plane faces the viewer, mirrored when it is seen from behind (so the
/// text is not reversed), and turned half round when it would read upside down.
pub fn readable_axes(view: &ViewState, u: Vec3, v: Vec3) -> (Vec3, Vec3) {
    let (mut u, mut v) = (u, v);
    if u.cross(v).dot(view.back()) < 0.0 {
        u = -u;
    }
    let a = view.project_vector(u);
    let up = view.project_vector(v);
    // Screen y points down: the text reads left to right and its top points up (roughly).
    if a.x < -0.2 * a.length() || (a.x.abs() <= 0.2 * a.length() && up.y > 0.0) {
        u = -u;
        v = -v;
    }
    (u, v)
}

/// The Shift+7 view: isometric, centered on the origin and zoomed so the default planes fill
/// about 72% of the viewport, as Onshape's isometric view does.
pub fn fitted_isometric(viewport: Vec2) -> ViewState {
    let v = ViewState::standard(StandardView::Isometric);
    // The projection is the tab's; a perspective view is fitted on the planes' corners.
    fitted_isometric_from(v, viewport)
}

/// [`fitted_isometric`] for a view in perspective or not.
pub fn fitted_isometric_for(perspective: bool, viewport: Vec2) -> ViewState {
    let v = ViewState {
        perspective,
        ..ViewState::standard(StandardView::Isometric)
    };
    if !perspective {
        return fitted_isometric_from(v, viewport);
    }
    let pts: Vec<Vec3> = PlaneKind::ALL
        .into_iter()
        .flat_map(|k| {
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(a, b)| (k.u() * a + k.v() * b) * PLANE_HALF)
        })
        .collect();
    v.fitted(&pts, viewport, 0.72)
}

fn fitted_isometric_from(mut v: ViewState, viewport: Vec2) -> ViewState {
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for k in PlaneKind::ALL {
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = v.project((k.u() * a + k.v() * b) * PLANE_HALF);
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let extent = (hi - lo) / (viewport * 0.72);
    v.scale *= extent.x.max(extent.y);
    v
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
fn place_origin_marker(
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    highlight: Res<PlaneHighlight>,
    selection: Res<Selection>,
    theme: Res<Theme>,
    mut q: Query<
        (&mut Node, &mut BorderColor, &mut BackgroundColor),
        (With<OriginMarker>, Without<OriginDot>),
    >,
    mut q_dot: Query<&mut BackgroundColor, (With<OriginDot>, Without<OriginMarker>)>,
    (draw, sketch_hover, sketch_selection): (
        Res<crate::sketch_tools::SketchDraw>,
        Res<crate::sketch_tools::SketchHover>,
        Res<crate::sketch_tools::SketchSelection>,
    ),
) {
    use cadrs_sketch::SketchEntity;
    let p = rect.to_screen(view.view.project(Vec3::ZERO)) - rect.0.min;
    // A sketch tool snapping to the origin highlights it like a hovered point (`screens/10`).
    let snapped = draw.over_viewport && draw.snapped_origin();
    let c = if snapped {
        // Blends into the snap highlight's disc.
        Color::srgb_u8(0xfe, 0xc6, 0x85)
    } else if sketch_selection.contains(SketchEntity::Origin) {
        Color::srgb_u8(0xf6, 0xbc, 0x1a)
    } else if highlight.is_hovered(Pick::Origin) || sketch_hover.0 == Some(SketchEntity::Origin) {
        theme.highlight
    } else if selection.contains(Pick::Origin) {
        theme.selection_3d
    } else {
        theme.foreground
    };
    // Snapped, the snap disc shows through the ring (`screens/10`).
    let fill = if snapped { Color::NONE } else { Color::WHITE };
    for (mut node, mut border, mut bg) in &mut q {
        bg.set_if_neq(BackgroundColor(fill));
        let (l, t) = (Val::Px(p.x - 6.0), Val::Px(p.y - 6.0));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        border.set_if_neq(BorderColor::all(c));
    }
    for mut bg in &mut q_dot {
        bg.set_if_neq(BackgroundColor(c));
    }
}

/// Scripted steps wait while a rebuild runs, the view is animating or a section's caps are
/// being worked out ([`cadrs_ui::PendingWork`]).
fn flag_pending_work(
    cache: Res<crate::parts::PartCache>,
    view: Res<ViewportView>,
    section: Res<crate::section_view::SectionClip>,
    mut pending: ResMut<cadrs_ui::PendingWork>,
    mut why: ResMut<cadrs_ui::PendingWhy>,
) {
    for (busy, what) in [
        (cache.rebuilding, "rebuild"),
        (view.animation.is_some(), "view animation"),
        (section.busy(), "section caps"),
    ] {
        if busy {
            pending.0 = true;
            why.add(what);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The turn (world axis, radians) taking rotation `a` to `b`, the short way.
    fn turn_between(a: Quat, b: Quat) -> (Vec3, f32) {
        let q = b * a.inverse();
        let q = if q.w < 0.0 { -q } else { q };
        q.normalize().to_axis_angle()
    }

    /// Runs the view's animation to its end, a frame at a time, checking each frame's turn from
    /// the last with `check` (axis, radians).
    fn run(v: &mut ViewportView, mut check: impl FnMut(Vec3, f32)) {
        let mut prev = v.view.rotation();
        for _ in 0..600 {
            if v.animation.is_none() {
                return;
            }
            v.advance(1.0 / 60.0, false);
            let q = v.view.rotation();
            let (axis, angle) = turn_between(prev, q);
            check(axis, angle);
            prev = q;
        }
        panic!("the view never stopped");
    }

    /// #91: a turn by the arrows (a tap, the view cube's) is animated about the screen axis it
    /// turns about, whatever the orientation (over the top too, where the azimuth and roll swing
    /// round).
    #[test]
    fn an_arrow_turn_is_animated_about_the_screen_axis() {
        for (azimuth, elevation, roll) in [
            (30.0, 30.0, 0.0),
            (10.0, 85.0, 0.0),
            (-120.0, -80.0, 40.0),
            (150.0, 60.0, -30.0),
        ] {
            for (yaw, pitch) in [
                (15.0f32, 0.0f32),
                (-15.0, 0.0),
                (0.0, 15.0),
                (0.0, -15.0),
                (90.0, 0.0),
                (0.0, 90.0),
            ] {
                let from = ViewState {
                    azimuth,
                    elevation,
                    roll,
                    ..ViewState::default()
                };
                // Positive yaw turns the model right: the view about screen up, the other way.
                let axis = if yaw != 0.0 {
                    -from.up() * yaw.signum()
                } else {
                    -from.right() * pitch.signum()
                };
                let mut v = ViewportView {
                    view: from,
                    ..default()
                };
                v.turn_by(yaw, pitch);
                let mut total = 0.0;
                run(&mut v, |a, angle| {
                    if angle > 1e-4 {
                        assert!(
                            a.dot(axis) > 0.999,
                            "from ({azimuth}, {elevation}, {roll}) by ({yaw}, {pitch}): \
                             turned about {a} by {angle}, not {axis}"
                        );
                    }
                    total += angle;
                });
                let want = (yaw + pitch).abs().to_radians();
                assert!(
                    (total - want).abs() < 1e-3,
                    "from ({azimuth}, {elevation}, {roll}) by ({yaw}, {pitch}): {total}"
                );
            }
        }
    }

    /// Frames (60 a second) of the arrow keys `held` (x right, y down) for `frames` frames, then
    /// let go and the view run to its end; each frame's turn checked with `check` (axis,
    /// radians). The degrees turned while held and after.
    fn hold(
        v: &mut ViewportView,
        held: Vec2,
        (shift, ctrl): (bool, bool),
        frames: usize,
        mut check: impl FnMut(usize, Vec3, f32),
    ) -> (f32, f32) {
        let dt = 1.0 / 60.0;
        let mut keys = ArrowHold::default();
        let mut prev = v.view.rotation();
        let mut during = 0.0;
        for frame in 0..frames {
            hold_arrows(v, &mut keys, held, shift, ctrl, dt);
            v.advance(dt, false);
            let q = v.view.rotation();
            let (a, angle) = turn_between(prev, q);
            check(frame, a, angle);
            during += angle;
            prev = q;
        }
        hold_arrows(v, &mut keys, Vec2::ZERO, shift, ctrl, dt);
        let mut after = 0.0;
        run(v, |a, angle| {
            check(frames, a, angle);
            after += angle;
        });
        (during.to_degrees(), after.to_degrees())
    }

    /// #91: an arrow held turns the view from the moment it's pressed, one way about the
    /// screen's up axis at a steady 90° a second (not waiting out the system's key repeat, then
    /// stepping), and stops within a few degrees of being let go.
    #[test]
    fn a_held_arrow_turns_the_view_steadily() {
        let from = ViewState::default();
        let axis = from.up();
        let mut v = ViewportView {
            view: from,
            ..default()
        };
        let (during, after) = hold(
            &mut v,
            Vec2::new(-1.0, 0.0),
            (false, false),
            180,
            |f, a, angle| {
                assert!(angle > 1e-5 || f >= 180, "frame {f}: still");
                if angle > 1e-5 {
                    assert!(a.dot(axis) > 0.999, "frame {f}: about {a} by {angle}");
                }
                // Past the first few frames, at its rate.
                if (30..180).contains(&f) {
                    let rate = angle.to_degrees() * 60.0;
                    assert!((rate - 90.0).abs() < 1.0, "frame {f}: {rate}°/s");
                }
            },
        );
        assert!(after < 8.0, "{after}");
        assert!((during + after - 270.0).abs() < 0.1, "{during} + {after}");
    }

    /// #91: a tap turns the view one step (15°, Shift 90°, Ctrl 5°), however short.
    #[test]
    fn a_tap_turns_the_view_one_step() {
        for (keys, step) in [
            ((false, false), 15.0),
            ((true, false), 90.0),
            ((false, true), 5.0),
        ] {
            for frames in [1, 3] {
                let from = ViewState::default();
                let axis = -from.up();
                let mut v = ViewportView {
                    view: from,
                    ..default()
                };
                let (during, after) =
                    hold(&mut v, Vec2::new(1.0, 0.0), keys, frames, |f, a, angle| {
                        assert!(
                            angle < 1e-5 || a.dot(axis) > 0.999,
                            "{keys:?} frame {f}: about {a}"
                        );
                    });
                assert!(
                    (during + after - step).abs() < 0.05,
                    "{keys:?} for {frames} frames: {during} + {after}"
                );
            }
        }
    }

    /// #91: two arrows held turn the view diagonally: → and ↑ about the axis halfway between the
    /// screen's down and right axes (as the view turns, its axes with it: the same world axis).
    #[test]
    fn two_arrows_held_turn_the_view_diagonally() {
        let from = ViewState::default();
        let axis = (from.right() - from.up()).normalize();
        let mut v = ViewportView {
            view: from,
            ..default()
        };
        let (during, after) = hold(
            &mut v,
            Vec2::new(1.0, -1.0),
            (false, false),
            60,
            |f, a, angle| {
                assert!(
                    angle < 1e-5 || a.dot(axis) > 0.999,
                    "frame {f}: about {a}, not {axis}"
                );
            },
        );
        // A second: 90°, as fast as one arrow.
        assert!((during + after - 90.0).abs() < 0.1, "{during} + {after}");
    }

    /// A default plane seen from behind is labelled with that side's name: Front reads Back,
    /// Right Left, Top Bottom; edge on, no label.
    #[test]
    fn default_planes_are_labelled_by_the_side_seen() {
        use StandardView as V;
        for (plane, front, back) in [
            (PlaneKind::Front, V::Front, V::Back),
            (PlaneKind::Right, V::Right, V::Left),
            (PlaneKind::Top, V::Top, V::Bottom),
        ] {
            assert_eq!(
                plane.label(&ViewState::standard(front)),
                (plane.name(), 1.0)
            );
            let behind = plane.label(&ViewState::standard(back));
            assert_eq!(behind, (back_name(back), 1.0), "{plane:?}");
        }
        // Isometric: the three front sides; turned round, the three others.
        let iso = ViewState::standard(V::Isometric);
        let names = |v: &ViewState| PlaneKind::ALL.map(|k| k.label(v).0);
        assert_eq!(names(&iso), ["Top", "Front", "Right"]);
        let mut other = ViewportView {
            view: iso,
            ..default()
        };
        other.turn_by(180.0, 0.0);
        other.advance(1.0, true);
        assert_eq!(names(&other.view), ["Bottom", "Back", "Left"]);
        // Edge on (the Front view, the Top plane): hidden.
        assert_eq!(PlaneKind::Top.label(&ViewState::standard(V::Front)).1, 0.0);
    }

    fn back_name(v: StandardView) -> &'static str {
        match v {
            StandardView::Back => "Back",
            StandardView::Left => "Left",
            StandardView::Bottom => "Bottom",
            _ => unreachable!(),
        }
    }

    /// Blender's numpad: 7 Top, 1 Front, 3 Right, with Ctrl Bottom, Back and Left; 9 the other
    /// side, 5 perspective; 8, 2, 4 and 6 turn the view as the arrows (two at once diagonally).
    #[test]
    fn numpad_keys_as_blender() {
        use StandardView as V;
        for (key, view, opposite) in [
            (KeyCode::Numpad7, V::Top, V::Bottom),
            (KeyCode::Numpad1, V::Front, V::Back),
            (KeyCode::Numpad3, V::Right, V::Left),
        ] {
            assert_eq!(numpad_key(key, false), Some(NumpadKey::View(view)));
            assert_eq!(numpad_key(key, true), Some(NumpadKey::View(opposite)));
        }
        assert_eq!(
            numpad_key(KeyCode::Numpad9, false),
            Some(NumpadKey::Opposite)
        );
        assert_eq!(
            numpad_key(KeyCode::Numpad5, false),
            Some(NumpadKey::Perspective)
        );
        for key in [KeyCode::Numpad9, KeyCode::Numpad5] {
            assert_eq!(numpad_key(key, true), None);
        }
        for key in [KeyCode::Numpad8, KeyCode::Numpad0, KeyCode::Digit7] {
            assert_eq!(numpad_key(key, false), None);
        }
        let held = |ks: &[KeyCode]| {
            let mut keys = ButtonInput::<KeyCode>::default();
            for &k in ks {
                keys.press(k);
            }
            held_direction(&keys)
        };
        assert_eq!(held(&[KeyCode::Numpad4]), held(&[KeyCode::ArrowLeft]));
        assert_eq!(held(&[KeyCode::Numpad6]), Vec2::new(1.0, 0.0));
        assert_eq!(held(&[KeyCode::Numpad8]), Vec2::new(0.0, -1.0));
        assert_eq!(held(&[KeyCode::Numpad2]), Vec2::new(0.0, 1.0));
        assert_eq!(
            held(&[KeyCode::Numpad8, KeyCode::Numpad6]),
            Vec2::new(1.0, -1.0)
        );
        // Both ways at once: still.
        assert_eq!(held(&[KeyCode::Numpad4, KeyCode::ArrowRight]), Vec2::ZERO);
    }

    /// Numpad 9 turns the view round to its other side about the screen's up axis: what faced
    /// the viewer faces away, up stays up.
    #[test]
    fn numpad_9_turns_the_view_to_its_other_side() {
        for s in [
            StandardView::Front,
            StandardView::Top,
            StandardView::Isometric,
        ] {
            let from = ViewState::standard(s);
            let axis = from.up();
            let mut v = ViewportView {
                view: from,
                ..default()
            };
            v.turn_by(180.0, 0.0);
            run(&mut v, |a, angle| {
                assert!(
                    angle < 1e-4 || a.dot(axis).abs() > 0.999,
                    "{s:?}: about {a}"
                );
            });
            assert!(
                (v.view.back() + from.back()).length() < 1e-3,
                "{s:?}: {:?}",
                v.view
            );
            assert!(
                (v.view.up() - from.up()).length() < 1e-3,
                "{s:?}: {:?}",
                v.view
            );
        }
    }

    /// #91: taps quicker than their turns add up: four of Shift+→ (90° each) turn the view a
    /// whole revolution, the same way all along, back where it started.
    #[test]
    fn quick_taps_add_up_to_a_whole_turn() {
        let from = ViewState::default();
        let axis = -from.up();
        let mut v = ViewportView {
            view: from,
            ..default()
        };
        let dt = 1.0 / 60.0;
        let mut keys = ArrowHold::default();
        let mut total = 0.0;
        let mut prev = from.rotation();
        for frame in 0..8 {
            let held = if frame % 2 == 0 { Vec2::X } else { Vec2::ZERO };
            hold_arrows(&mut v, &mut keys, held, true, false, dt);
            v.advance(dt, false);
            let q = v.view.rotation();
            let (a, angle) = turn_between(prev, q);
            assert!(
                angle < 1e-4 || a.dot(axis) > 0.999,
                "frame {frame}: about {a}"
            );
            total += angle;
            prev = q;
        }
        run(&mut v, |a, angle| {
            assert!(angle < 1e-4 || a.dot(axis) > 0.999, "about {a} by {angle}");
            total += angle;
        });
        assert!((total - std::f32::consts::TAU).abs() < 1e-3, "{total}");
        assert!(v.view.approx_eq(&from), "{:?}", v.view);
    }

    /// P3E.3a (TD6.5): the render mode and the projection are per tab: they stay with a tab's
    /// view when another tab is shown and come back with it; a tab opened for the first time
    /// is Shaded and orthographic; a change of view (an animation) keeps them.
    #[test]
    fn render_mode_and_perspective_are_per_tab() {
        use crate::camera::RenderMode;
        let mut doc = cadrs_core::Document::empty("Tabs");
        doc.elements.push(cadrs_core::Element::part_studio("A"));
        doc.elements.push(cadrs_core::Element::part_studio("B"));
        let (a, b) = (doc.elements[0].id, doc.elements[1].id);
        let mut app = App::new();
        app.insert_resource(ActiveDocument::new(doc))
            .init_resource::<ActiveKind>()
            .init_resource::<ViewportView>()
            .init_resource::<Selection>()
            .init_resource::<ViewportRect>()
            .init_resource::<PlanesVisible>()
            .init_resource::<crate::parts::PartCache>()
            .add_systems(Update, track_active_element);
        let show = |app: &mut App, el| {
            app.world_mut().resource_mut::<ActiveDocument>().active = Some(el);
            app.update();
        };
        show(&mut app, a);
        {
            let mut v = app.world_mut().resource_mut::<ViewportView>();
            v.view.render = RenderMode::HiddenEdgesVisible;
            v.view.perspective = true;
            // A view change keeps them.
            let to = ViewState::standard(StandardView::Top);
            v.animate_to(to);
            assert_eq!(v.target().render, RenderMode::HiddenEdgesVisible);
            assert!(v.target().perspective);
        }
        show(&mut app, b);
        let v = app.world().resource::<ViewportView>().view;
        assert_eq!(v.render, RenderMode::Shaded);
        assert!(!v.perspective);
        app.world_mut().resource_mut::<ViewportView>().view.render = RenderMode::Translucent;
        show(&mut app, a);
        let v = app.world().resource::<ViewportView>().view;
        assert_eq!(v.render, RenderMode::HiddenEdgesVisible);
        assert!(v.perspective);
        show(&mut app, b);
        assert_eq!(
            app.world().resource::<ViewportView>().view.render,
            RenderMode::Translucent
        );
    }

    #[test]
    fn affine_parts_rebuild_the_map() {
        let rot =
            |a: f32| Mat2::from_cols(Vec2::new(a.cos(), a.sin()), Vec2::new(-a.sin(), a.cos()));
        for (a, b) in [
            (Vec2::new(0.9, 0.26), Vec2::new(0.0, 0.86)),
            (Vec2::new(0.5, -0.43), Vec2::new(0.0, 0.86)),
            (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)),
            (Vec2::new(0.7, 0.2), Vec2::new(-0.5, 0.4)),
        ] {
            let (outer, s, inner) = affine_parts(a, b);
            let m = rot(outer) * Mat2::from_diagonal(s) * rot(inner);
            assert!((m.col(0) - a).length() < 1e-4, "{m:?}");
            assert!((m.col(1) - b).length() < 1e-4, "{m:?}");
        }
    }

    #[test]
    fn picks_the_nearest_plane() {
        let v = ViewState::default();
        assert!(matches!(
            pick(&v, Vec2::new(-60.0, -120.0)),
            Some(Pick::Plane(_))
        ));
        // Straight above the origin only the vertical planes reach; the Front plane's corner
        // region at the far upper left belongs to the Front plane alone.
        assert_eq!(
            pick(&v, Vec2::new(-240.0, -290.0)),
            Some(Pick::Plane(PlaneKind::Front))
        );
        assert_eq!(pick(&v, Vec2::ZERO), Some(Pick::Origin));
        assert_eq!(pick(&v, Vec2::new(2000.0, 0.0)), None);
        // Far left, only the Top plane reaches.
        assert_eq!(
            pick(&v, Vec2::new(-360.0, 40.0)),
            Some(Pick::Plane(PlaneKind::Top))
        );
    }

    #[test]
    fn labels_fade_when_edge_on() {
        assert_eq!(label_alpha(1.0), 1.0);
        assert_eq!(label_alpha(0.1), 0.0);
        assert_eq!(label_alpha(70.1f32.to_radians().cos()), 0.0); // 70° from facing the viewer
        assert!(label_alpha(0.37) > 0.0 && label_alpha(0.37) < 1.0);
        assert_eq!(label_alpha(0.43), 1.0); // the Right plane in the default view
        // Seen from behind: never shown.
        assert_eq!(label_alpha(-0.9), 0.0);
        // In the Top view the Front and Right planes are edge-on: no labels.
        let top = ViewState::standard(StandardView::Top);
        assert_eq!(
            label_alpha(PlaneKind::Front.normal().dot(top.back()).abs()),
            0.0
        );
    }

    #[test]
    fn labels_are_never_mirrored_or_upside_down() {
        for (az, el, roll) in [
            (30.0, 30.0, 0.0),
            (210.0, 30.0, 0.0),
            (-120.0, -40.0, 0.0),
            (30.0, 30.0, 170.0),
            (100.0, 10.0, 90.0),
        ] {
            let view = ViewState {
                azimuth: az,
                elevation: el,
                roll,
                ..ViewState::default()
            };
            for k in PlaneKind::ALL {
                let (u, v) = readable_axes(&view, k.u(), k.v());
                let a = view.project_vector(u);
                let b = view.project_vector(-v);
                // Screen y down: reading direction then down is a right-handed pair.
                if a.perp_dot(b).abs() > 1e-3 {
                    assert!(a.perp_dot(b) > 0.0, "{k:?} mirrored at {az} {el} {roll}");
                }
                assert!(
                    a.x >= -0.2 * a.length() - 1e-4,
                    "{k:?} upside down at {az} {el} {roll}"
                );
            }
        }
    }

    #[test]
    fn isometric_fits_the_planes() {
        let vp = Vec2::new(1374.0, 901.0);
        let v = fitted_isometric(vp);
        let mut hi = Vec2::ZERO;
        for k in PlaneKind::ALL {
            for (a, b) in [(-1.0, -1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, 1.0)] {
                hi = hi.max(v.project((k.u() * a + k.v() * b) * PLANE_HALF).abs());
            }
        }
        assert!((hi.y * 2.0 - vp.y * 0.72).abs() < 1.0 || (hi.x * 2.0 - vp.x * 0.72).abs() < 1.0);
        assert_eq!(v.focus, Vec3::ZERO);
    }

    #[test]
    fn plane_frames() {
        assert_eq!(PlaneKind::Top.normal(), Vec3::Z);
        assert_eq!(PlaneKind::Front.normal(), -Vec3::Y);
        assert_eq!(PlaneKind::Right.normal(), Vec3::X);
        assert_eq!(PlaneKind::Front.label_corner(), Vec3::new(-75.0, 0.0, 75.0));
        assert_eq!(PlaneKind::Right.label_corner(), Vec3::new(0.0, -75.0, 75.0));
        assert_eq!(PlaneKind::Top.label_corner(), Vec3::new(-75.0, 75.0, 0.0));
    }
}
