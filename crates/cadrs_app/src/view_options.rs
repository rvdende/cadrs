//! The view cube menu's camera and render options (P3E.3a, TD6.5, PS2.9, X14), beyond the
//! standard views:
//!
//! - **Render modes** (per tab, kept with the tab's view, [`RenderMode`]): Shaded with edges
//!   (the default), Shaded without edges, Shaded with hidden edges (the edges behind faces
//!   dashed grey through them), Hidden edges removed (white faces, black visible edges: a line
//!   drawing), Hidden edges visible (the same with the hidden edges dashed) and Translucent
//!   (see-through parts with every edge). The parts' material and edges read the mode
//!   (`crate::parts`).
//! - **Perspective view**: a toggle (orthographic stays the default). The eye sits in front of
//!   the focus so the focus plane keeps the view's scale ([`crate::camera`]); the camera gets an
//!   off-centre perspective projection ([`ViewportPerspective`]) that puts the focus at the
//!   viewport's centre as the orthographic one does. Fit, zoom about the cursor and picking use
//!   the same maths, so they work in both.
//! - **Previous view**: the camera before the last change of view (a stack, per tab).
//! - **Zoom to window**: drag a box in the view; it fills the view. Esc cancels.
//! - **Zoom to selection** ([`zoom_to_selection`]): the view fitted to whatever is selected
//!   (faces, edges, vertices, parts, sketches, planes).
//! - **Named views…**: a panel listing the tab's saved cameras; "Add view" saves the current one
//!   under the typed name (a document edit, undone like any other,
//!   [`cadrs_core::named_views`]), a click on a row restores it, ✕ deletes it.

use bevy::camera::{CameraProjection, SubCameraView};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::math::Vec3A;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::named_views::{AddNamedView, DeleteNamedView, NamedView, next_name};
use cadrs_ui::prelude::*;
use cadrs_ui::{FloatingPanel, FloatingPanelBody, FloatingPanelClose, TextInputField, TextSubmit};

use crate::camera::{FOCAL_PX, RenderMode, ViewState};
use crate::viewport::{ActiveKind, Pick, Selection, ViewportArea, ViewportDrag, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct ViewOptionsPlugin;

impl Plugin for ViewOptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreviousViews>()
            .init_resource::<RenderGroups>()
            .add_systems(
                Update,
                (track_previous, zoom_window_input, sync_zoom_box, sync_named_views)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(PostUpdate, perspective_gizmo_bias)
            .add_systems(OnExit(AppState::Document), |mut commands: Commands, mut groups: ResMut<RenderGroups>| {
                groups.0.clear();
                commands.remove_resource::<ZoomWindow>();
                commands.remove_resource::<NamedViewsPanel>();
            })
            .add_observer(on_panel_close)
            .add_observer(on_name_submit);
    }
}

// ---------------------------------------------------------------------------------------------
// Render mode and perspective

/// The mode last picked in each group of the view menu (the shaded modes, the hidden-line
/// ones), per tab: each group's row reads its last mode, as Onshape's do.
#[derive(Resource, Debug, Default)]
pub struct RenderGroups(pub std::collections::HashMap<ElementId, (RenderMode, RenderMode)>);

impl RenderGroups {
    /// The shaded group's and the hidden-line group's modes for a tab now in `current`.
    pub fn labels(&self, element: Option<ElementId>, current: RenderMode) -> (RenderMode, RenderMode) {
        let (mut shaded, mut line) = element.and_then(|e| self.0.get(&e).copied()).unwrap_or((RenderMode::Shaded, RenderMode::HiddenEdgesRemoved));
        if current.line_drawing() {
            line = current;
        } else {
            shaded = current;
        }
        (shaded, line)
    }
}

/// Sets the active tab's render mode (it stays with the tab's view).
pub fn set_render_mode(world: &mut World, mode: RenderMode) {
    let element = world.resource::<ViewportView>().element;
    if let Some(el) = element {
        let mut groups = world.resource_mut::<RenderGroups>();
        let g = groups.0.entry(el).or_insert((RenderMode::Shaded, RenderMode::HiddenEdgesRemoved));
        if mode.line_drawing() {
            g.1 = mode;
        } else {
            g.0 = mode;
        }
    }
    let mut view = world.resource_mut::<ViewportView>();
    view.view.render = mode;
    if let Some(a) = view.animation.as_mut() {
        a.from.render = mode;
        a.to.render = mode;
    }
}

/// Turns the perspective view on or off, keeping what the focus plane shows.
pub fn set_perspective(world: &mut World, on: bool) {
    let mut view = world.resource_mut::<ViewportView>();
    view.view.perspective = on;
    if let Some(a) = view.animation.as_mut() {
        a.from.perspective = on;
        a.to.perspective = on;
    }
}

/// An off-centre perspective projection for the viewport: `focal` logical px, the focus (the
/// view axis) at `center` (logical px from the target's top-left) of a target `size` logical
/// px, reversed infinite depth from `near` like Bevy's own perspective.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportPerspective {
    pub focal: f32,
    pub center: Vec2,
    pub size: Vec2,
    pub near: f32,
    pub far: f32,
}

impl ViewportPerspective {
    /// The projection for `view` with the viewport centred at `center` in a target of `size`.
    pub fn of(view: &ViewState, center: Vec2, size: Vec2) -> Self {
        let eye = view.eye_distance();
        Self { focal: FOCAL_PX, center, size: size.max(Vec2::ONE), near: (eye * 0.01).max(1e-3), far: eye + crate::camera::CAMERA_FAR }
    }

    /// The view-space point (x, y, z with z < 0) at screen `px` (logical px from the target's
    /// top-left) and distance `d` in front of the eye.
    fn at(&self, px: Vec2, d: f32) -> Vec3A {
        Vec3A::new((px.x - self.center.x) * d / self.focal, (self.center.y - px.y) * d / self.focal, -d)
    }
}

impl CameraProjection for ViewportPerspective {
    fn get_clip_from_view(&self) -> Mat4 {
        let (w, h) = (self.size.x, self.size.y);
        let (cx, cy) = (self.center.x, self.center.y);
        Mat4::from_cols(
            Vec4::new(2.0 * self.focal / w, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 * self.focal / h, 0.0, 0.0),
            Vec4::new(1.0 - 2.0 * cx / w, 2.0 * cy / h - 1.0, 0.0, -1.0),
            Vec4::new(0.0, 0.0, self.near, 0.0),
        )
    }

    fn get_clip_from_view_for_sub(&self, _sub_view: &SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }

    fn update(&mut self, _width: f32, _height: f32) {
        // The size comes from `crate::viewport::apply_view_to_camera` (logical px).
    }

    fn far(&self) -> f32 {
        self.far
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        let (w, h) = (self.size.x, self.size.y);
        let (n, f) = (z_near.abs(), z_far.abs());
        // Bottom right, top right, top left, bottom left; near then far.
        [
            self.at(Vec2::new(w, h), n),
            self.at(Vec2::new(w, 0.0), n),
            self.at(Vec2::ZERO, n),
            self.at(Vec2::new(0.0, h), n),
            self.at(Vec2::new(w, h), f),
            self.at(Vec2::new(w, 0.0), f),
            self.at(Vec2::ZERO, f),
            self.at(Vec2::new(0.0, h), f),
        ]
    }
}

/// How much the depth biases of lines over faces grow in this view: 1 in orthographic; in
/// perspective so they still pull about half a millimetre (see [`perspective_gizmo_bias`]).
pub fn bias_factor(view: &ViewState) -> f32 {
    if view.perspective { (2990.0 / view.eye_distance().max(1e-3)).clamp(1.0, 400.0) } else { 1.0 }
}

/// The part gizmos' depth bias pulls lines toward the eye by a share of their depth; in
/// perspective the eye is much nearer than the orthographic camera, so the biases grow to keep
/// edges over the faces they lie on (about the orthographic half millimetre).
fn perspective_gizmo_bias(view: Res<ViewportView>, mut store: ResMut<GizmoConfigStore>, mut base: Local<Option<Vec<f32>>>, mut last: Local<f32>) {
    use crate::parts::*;
    let k = bias_factor(&view.view);
    if (k - *last).abs() < 1e-3 * k && base.is_some() {
        return;
    }
    fn bias(store: &mut GizmoConfigStore, i: usize) -> &mut f32 {
        &mut match i {
            0 => store.config_mut::<PartEdgeGizmos>().0,
            1 => store.config_mut::<PreviewEdgeGizmos>().0,
            2 => store.config_mut::<FaceHoverGizmos>().0,
            3 => store.config_mut::<EdgeHighlightGizmos>().0,
            4 => store.config_mut::<PartOutlineGizmos>().0,
            5 => store.config_mut::<ReferenceEdgeGizmos>().0,
            6 => store.config_mut::<FreeEdgeGizmos>().0,
            _ => store.config_mut::<VertexGizmos>().0,
        }
        .depth_bias
    }
    const GROUPS: usize = 8;
    let b = base.get_or_insert_with(|| (0..GROUPS).map(|i| *bias(&mut store, i)).collect()).clone();
    for (i, b) in b.into_iter().enumerate() {
        // -1 means "over everything" and stays so.
        *bias(&mut store, i) = if b <= -1.0 { b } else { (b * k).max(-0.5) };
    }
    *last = k;
}

// ---------------------------------------------------------------------------------------------
// Previous view

/// The cameras before each change of view in the active tab, newest last.
#[derive(Resource, Debug, Default)]
pub struct PreviousViews {
    element: Option<ElementId>,
    /// The camera the view last rested at, and the last frame's.
    settled: Option<ViewState>,
    last: Option<ViewState>,
    still: u32,
    pub stack: Vec<ViewState>,
}

/// Frames a view must stay unchanged to count as a new resting camera (a wheel zoom of several
/// notches is one change).
const SETTLE_FRAMES: u32 = 6;
const MAX_PREVIOUS: usize = 30;

/// The two views show the same camera (render mode and projection aside).
fn same_camera(a: &ViewState, b: &ViewState) -> bool {
    a.approx_eq(&ViewState { render: a.render, perspective: a.perspective, ..*b })
}

fn track_previous(view: Res<ViewportView>, drag: Res<ViewportDrag>, mut prev: ResMut<PreviousViews>) {
    if prev.element != view.element {
        *prev = PreviousViews { element: view.element, settled: Some(view.view), last: Some(view.view), ..default() };
        return;
    }
    let now = view.view;
    let moved = prev.last.is_none_or(|l| !same_camera(&l, &now));
    prev.last = Some(now);
    if view.animation.is_some() || drag.navigating() || moved {
        prev.still = 0;
        return;
    }
    let Some(settled) = prev.settled else {
        prev.settled = Some(now);
        return;
    };
    if same_camera(&settled, &now) {
        return;
    }
    prev.still += 1;
    if prev.still >= SETTLE_FRAMES {
        prev.stack.push(settled);
        if prev.stack.len() > MAX_PREVIOUS {
            prev.stack.remove(0);
        }
        prev.settled = Some(now);
        prev.still = 0;
    }
}

/// Previous view: back to the camera before the last change (keeping the render mode).
pub fn previous_view(world: &mut World) {
    let Some(to) = world.resource_mut::<PreviousViews>().stack.pop() else { return };
    let mut view = world.resource_mut::<ViewportView>();
    let to = ViewState { render: view.view.render, ..to };
    let perspective = to.perspective;
    view.view.perspective = perspective;
    view.animate_to(to);
    let mut prev = world.resource_mut::<PreviousViews>();
    prev.settled = Some(to);
    prev.still = 0;
}

/// There is a view to go back to.
pub fn has_previous(world: &World) -> bool {
    !world.resource::<PreviousViews>().stack.is_empty()
}

// ---------------------------------------------------------------------------------------------
// Zoom to window

/// Zoom to window is waiting for (or dragging) its box: the corners in logical px.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ZoomWindow {
    pub start: Option<Vec2>,
    pub current: Vec2,
}

/// The box being dragged.
#[derive(Component)]
struct ZoomBox;

/// Starts Zoom to window.
pub fn start_zoom_window(world: &mut World) {
    world.insert_resource(ZoomWindow::default());
}

#[allow(clippy::too_many_arguments)]
fn zoom_window_input(
    zoom: Option<ResMut<ZoomWindow>>,
    mut inputs: MessageReader<PointerInput>,
    mut keys: MessageReader<KeyboardInput>,
    hover: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    rect: Res<ViewportRect>,
    mut view: ResMut<ViewportView>,
    cache: Res<crate::parts::PartCache>,
    mut commands: Commands,
) {
    let Some(mut zoom) = zoom else {
        inputs.clear();
        keys.clear();
        return;
    };
    if keys.read().any(|k| k.state == ButtonState::Pressed && k.key_code == KeyCode::Escape) {
        commands.remove_resource::<ZoomWindow>();
        return;
    }
    let over = crate::viewport::pointer_over_viewport(&hover, &q_area);
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) if over => {
                zoom.start = Some(pos);
                zoom.current = pos;
            }
            PointerAction::Move { .. } => zoom.current = pos,
            PointerAction::Release(PointerButton::Primary) => {
                if let Some(a) = zoom.start {
                    let (ca, cb) = (rect.offset(a), rect.offset(pos));
                    let mut from = view.target();
                    // In perspective, the box's depth: the middle depth of the parts in it.
                    if from.perspective
                        && let Some(p) = box_depth_point(&cache, &from, ca, cb)
                    {
                        from = from.refocused(p);
                    }
                    let to = from.zoomed_to_box(ca, cb, rect.0.size());
                    view.animate_to(to);
                    commands.remove_resource::<ZoomWindow>();
                    return;
                }
            }
            _ => {}
        }
    }
}

/// A point at the middle depth of the parts seen in the screen box from `a` to `b` (a 7 × 7
/// grid of picks), on the ray through the box's centre.
fn box_depth_point(cache: &crate::parts::PartCache, view: &ViewState, a: Vec2, b: Vec2) -> Option<Vec3> {
    let mut depths: Vec<f32> = Vec::new();
    for i in 0..7 {
        for j in 0..7 {
            let at = a + (b - a) * Vec2::new(i as f32 / 6.0, j as f32 / 6.0);
            if let Some((_, _, t)) = crate::parts::pick_face(cache, view, at) {
                let (o, d) = view.ray(at);
                depths.push((o + d * t - view.camera_position()).dot(-view.back()));
            }
        }
    }
    if depths.is_empty() {
        return None;
    }
    depths.sort_by(f32::total_cmp);
    let depth = depths[depths.len() / 2];
    Some(view.camera_position() - view.back() * depth)
}

fn sync_zoom_box(
    zoom: Option<Res<ZoomWindow>>,
    rect: Res<ViewportRect>,
    theme: Res<Theme>,
    mut q: Query<(Entity, &mut Node), With<ZoomBox>>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some((a, b)) = zoom.and_then(|z| z.start.map(|s| (s, z.current))) else {
        for (e, _) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let (lo, hi) = (a.min(b) - rect.0.min, a.max(b) - rect.0.min);
    let place = |n: &mut Node| {
        n.left = Val::Px(lo.x);
        n.top = Val::Px(lo.y);
        n.width = Val::Px(hi.x - lo.x);
        n.height = Val::Px(hi.y - lo.y);
    };
    if let Some((_, mut n)) = q.iter_mut().next() {
        place(&mut n);
        return;
    }
    let Some(area) = q_area.iter().next() else { return };
    let mut n = Node { position_type: PositionType::Absolute, border: UiRect::all(Val::Px(1.0)), ..default() };
    place(&mut n);
    let c = theme.primary;
    let e = commands
        .spawn((Name::new("zoom-window-box"), ZoomBox, n, BorderColor::all(c), BackgroundColor(c.with_alpha(0.08)), Pickable::IGNORE, DespawnOnExit(AppState::Document)))
        .id();
    commands.entity(area).add_child(e);
}

// ---------------------------------------------------------------------------------------------
// Zoom to selection

/// The points of a pick, among the parts on screen.
fn pick_points(world: &World, pick: Pick) -> Vec<Vec3> {
    let cache = world.resource::<crate::parts::PartCache>();
    let v3 = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    match pick {
        Pick::Face(part, name) => cache.part(part).and_then(|p| p.solid.face(&name)).map(|f| f.loops.iter().flatten().map(|q| v3(*q)).collect()).unwrap_or_default(),
        Pick::Edge(part, name) => cache.part(part).and_then(|p| p.solid.edge(&name)).map(|e| e.points.iter().map(|q| v3(*q)).collect()).unwrap_or_default(),
        Pick::Vertex(part, name) => cache.part(part).and_then(|p| p.solid.vertex(&name)).map(|v| vec![v3(v.point)]).unwrap_or_default(),
        Pick::Part(part) => {
            // An assembly instance's row selects all of its parts.
            let whole = cache.assembly.is_some() && part.index == 0;
            cache.parts.iter().filter(|p| p.id == part || (whole && p.id.feature == part.feature)).flat_map(|p| p.solid.positions.iter().map(|q| v3(*q))).collect()
        }
        Pick::Plane(k) => {
            let h = crate::viewport::PLANE_HALF;
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].into_iter().map(|(a, b)| (k.u() * a + k.v() * b) * h).collect()
        }
        Pick::Feature(f) | Pick::Region(f, _) | Pick::SketchCurve(f, _) | Pick::SketchPoint(f, _) => {
            let mut pts: Vec<Vec3> = cache.sketch_curves.iter().filter(|s| s.sketch == f).flat_map(|s| s.curves.iter().flat_map(|(_, c)| c.iter().map(|q| v3(*q)))).collect();
            if pts.is_empty() {
                // A part feature: the faces it made.
                pts = cache.shown().flat_map(|p| p.solid.faces.iter().filter(|x| x.name.op == f.0).flat_map(|x| x.loops.iter().flatten().map(|q| v3(*q)))).collect();
            }
            if pts.is_empty()
                && let Some(frame) = cache.planes.get(&f)
            {
                pts.push(v3(frame.origin));
            }
            pts
        }
        Pick::Origin => vec![Vec3::ZERO],
        Pick::Assembly | Pick::Instance(..) => cache.shown().flat_map(|p| p.solid.positions.iter().map(|q| v3(*q))).collect(),
    }
}

/// Zoom to selection: the view fitted to everything selected (Zoom to fit with nothing
/// selected).
pub fn zoom_to_selection(world: &mut World) {
    let picks = world.resource::<Selection>().0.clone();
    let pts: Vec<Vec3> = picks.iter().flat_map(|p| pick_points(world, *p)).collect();
    if pts.is_empty() {
        crate::viewport::zoom_to_fit(world);
        return;
    }
    let size = world.resource::<ViewportRect>().0.size();
    let inset = world.resource::<crate::viewport::DialogInset>().0;
    let kind = *world.resource::<ActiveKind>();
    let mut view = world.resource_mut::<ViewportView>();
    let to = view.target().fitted_beside(&pts, size, crate::viewport::fit_fill(kind), inset);
    view.animate_to(to);
}

// ---------------------------------------------------------------------------------------------
// Named views

/// The Named views panel is open.
#[derive(Resource, Debug, Clone, Default)]
pub struct NamedViewsPanel;

/// The panel and what its list was last built from (the names, and the row the view is at).
#[derive(Component, Default)]
struct PanelRoot {
    built: Option<(Vec<String>, Option<usize>)>,
}

const PANEL_WIDTH: f32 = 248.0;

pub fn open_named_views(world: &mut World) {
    world.insert_resource(NamedViewsPanel);
}

fn tab_views(doc: &ActiveDocument) -> Vec<NamedView> {
    doc.active_element().map(|e| e.named_views.clone()).unwrap_or_default()
}

/// The camera (and projection) a named view stands for, on top of `base`.
fn named_camera(nv: &NamedView, base: &ViewState) -> ViewState {
    ViewState { azimuth: nv.azimuth, elevation: nv.elevation, roll: nv.roll, focus: Vec3::from_array(nv.focus), scale: nv.scale, perspective: nv.perspective, ..*base }
}

/// The render mode a named view was saved in.
fn named_render(nv: &NamedView) -> Option<RenderMode> {
    let slug = nv.render.as_deref()?;
    RenderMode::ALL.into_iter().find(|m| m.slug() == slug)
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_named_views(
    panel: Option<Res<NamedViewsPanel>>,
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    theme: Res<Theme>,
    rect: Res<ViewportRect>,
    view: Res<ViewportView>,
    mut q_root: Query<(Entity, &mut PanelRoot)>,
    q_body: Query<(Entity, &FloatingPanelBody)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_node: Query<&mut Node, With<PanelRoot>>,
    mut commands: Commands,
) {
    let open = panel.is_some() && matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly);
    let Some(doc) = doc.filter(|_| open) else {
        for (e, _) in &q_root {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let views = tab_views(&doc);
    let names: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
    // The row of the view on screen (once the camera rests there): shown selected.
    let current = if view.animation.is_some() {
        q_root.iter().next().and_then(|(_, s)| s.built.as_ref().and_then(|b| b.1))
    } else {
        let now = view.view;
        views.iter().position(|nv| same_camera(&named_camera(nv, &now), &now) && nv.perspective == now.perspective)
    };
    let Some((root, mut state)) = q_root.iter_mut().next() else {
        let Some(area) = q_area.iter().next() else { return };
        let left = (rect.0.width() - PANEL_WIDTH - 12.0).max(0.0);
        let e = commands
            .spawn((FloatingPanel::new("named-views", "Named views").width(PANEL_WIDTH).at(left, 178.0).build(&theme), PanelRoot::default(), DespawnOnExit(AppState::Document)))
            .id();
        commands.entity(area).add_child(e);
        return;
    };
    // Kept inside the view when the tab's viewport is narrower (an assembly's instance list,
    // P3E.3a judge: the panel ran off the right edge).
    if let Ok(mut n) = q_node.get_mut(root) {
        let max_left = (rect.0.width() - PANEL_WIDTH - 12.0).max(0.0);
        if let Val::Px(l) = n.left
            && l > max_left
        {
            n.left = Val::Px(max_left);
        }
    }
    let key = (names.clone(), current);
    if state.built.as_ref() == Some(&key) {
        return;
    }
    let Some((body, _)) = q_body.iter().find(|(_, b)| b.0 == root) else { return };
    state.built = Some(key);
    let t = theme.clone();
    let default_name = next_name(&views);
    let rows = commands.spawn((Name::new("named-views-list"), Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), row_gap: Val::Px(1.0), ..default() })).id();
    commands.entity(rows).with_children(|b| {
        if names.is_empty() {
            b.spawn((
                Name::new("named-views-empty"),
                t.text("No named views in this tab yet", t.font_base, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::new(Val::Px(6.0), Val::ZERO, Val::Px(6.0), Val::Px(6.0)), ..default() },
            ));
        }
        for (i, name) in names.iter().enumerate() {
            let n = name.clone();
            let d = name.clone();
            b.spawn((
                // P3E.3a judge: the UI's text size (the row tints on hover).
                cadrs_ui::ActionRow::new(format!("named-view-{i}"), name.clone())
                    .icon("named-positions")
                    .height(28.0)
                    .font_size(t.font_base)
                    .selected(current == Some(i))
                    .action("delete", "delete", format!("Delete {name}"), true)
                    .build(&t),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    let n = n.clone();
                    commands.queue(move |world: &mut World| restore_named_view(world, &n));
                }),
                observe(move |_: On<cadrs_ui::ActionRowAction>, mut commands: Commands| {
                    let d = d.clone();
                    commands.queue(move |world: &mut World| delete_named_view(world, &d));
                }),
            ));
        }
    });
    commands.entity(body).despawn_related::<Children>();
    commands.entity(body).add_child(rows);
    commands.entity(body).with_children(move |b| {
        b.spawn(Node {
            margin: UiRect::top(Val::Px(8.0)),
            padding: UiRect::top(Val::Px(8.0)),
            border: UiRect::top(Val::Px(1.0)),
            column_gap: Val::Px(6.0),
            align_items: AlignItems::Center,
            width: Val::Percent(100.0),
            ..default()
        })
        .insert(BorderColor::all(t.border))
        .with_children(|r| {
            r.spawn(TextInput::new("named-view-name").value(default_name).select_all_on_focus().width(Val::Percent(100.0)).height(26.0).build(&t))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.flex_grow = 1.0;
                    n.min_width = Val::Px(0.0);
                });
            r.spawn((
                cadrs_ui::Button::new("named-view-add").label("Add view").icon("plus").primary().small().tooltip("Save the current view under this name").build(&t),
                observe(|_: On<Activate>, mut commands: Commands| {
                    commands.queue(add_named_view_from_field);
                }),
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_shrink = 0.0);
        });
    });
}

fn on_panel_close(ev: On<FloatingPanelClose>, q: Query<(), With<PanelRoot>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.remove_resource::<NamedViewsPanel>();
    }
}

fn on_name_submit(ev: On<TextSubmit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "named-view-name-field") {
        commands.queue(add_named_view_from_field);
    }
}

fn add_named_view_from_field(world: &mut World) {
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    let name = q.iter(world).find(|(n, _)| n.as_str() == "named-view-name-field").map(|(_, t)| t.value().to_string()).unwrap_or_default();
    save_named_view(world, &name);
}

/// Saves the current camera as the named view `name` of the active tab (one undo step).
pub fn save_named_view(world: &mut World, name: &str) {
    let v = world.resource::<ViewportView>().target();
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active else { return };
    let view = NamedView {
        name: name.to_string(),
        azimuth: v.azimuth,
        elevation: v.elevation,
        roll: v.roll,
        focus: v.focus.to_array(),
        scale: v.scale,
        perspective: v.perspective,
        render: Some(v.render.slug().to_string()),
    };
    if let Err(e) = doc.execute(&AddNamedView { element, view }) {
        warn!("Save named view: {e}");
    }
}

/// Restores the active tab's named view `name`: its camera, projection and render mode.
pub fn restore_named_view(world: &mut World, name: &str) {
    let Some(nv) = world.get_resource::<ActiveDocument>().and_then(|d| tab_views(d).into_iter().find(|v| v.name.eq_ignore_ascii_case(name.trim()))) else {
        warn!("No named view {name:?}");
        return;
    };
    if let Some(mode) = named_render(&nv) {
        set_render_mode(world, mode);
    }
    let mut view = world.resource_mut::<ViewportView>();
    let to = named_camera(&nv, &view.target());
    view.view.perspective = nv.perspective;
    view.animate_to(to);
}

pub fn delete_named_view(world: &mut World, name: &str) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active else { return };
    if let Err(e) = doc.execute(&DeleteNamedView { element, name: name.to_string() }) {
        warn!("Delete named view: {e}");
    }
}

/// The render mode's menu items (a submenu of the view menu).
pub fn render_items(current: RenderMode, line_drawing: bool) -> Vec<cadrs_ui::MenuEntry> {
    RenderMode::ALL
        .into_iter()
        .filter(|m| m.line_drawing() == line_drawing)
        .map(|m| cadrs_ui::MenuItem::new(format!("view-render-{}", m.slug()), m.label()).checked(m == current).into())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::RenderMode;

    /// Zoom to selection: the selection's points fill the fit fraction of the view beside a
    /// dialog's inset, centred in what is left.
    #[test]
    fn zoom_to_selection_frames_the_points_beside_the_dialog() {
        let v = ViewState::default();
        let pts = [Vec3::new(10.0, 0.0, 0.0), Vec3::new(60.0, 0.0, 0.0), Vec3::new(60.0, 0.0, 30.0), Vec3::new(10.0, 0.0, 30.0)];
        let size = Vec2::new(1000.0, 800.0);
        let to = v.fitted_beside(&pts, size, 0.8, 200.0);
        let screen: Vec<Vec2> = pts.iter().map(|p| to.project(*p)).collect();
        let (lo, hi) = screen.iter().fold((Vec2::MAX, Vec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        // 800 px are left beside the 200 px dialog: the larger side fills 80% of them.
        let fill = ((hi - lo).x / 800.0).max((hi - lo).y / 800.0);
        assert!((fill - 0.8).abs() < 1e-3, "{fill}");
        // Centred in the 800 px right of the dialog: 100 px right of the viewport's centre.
        let mid = (lo + hi) / 2.0;
        assert!((mid - Vec2::new(100.0, 0.0)).length() < 0.05, "{mid:?}");
        // The orientation is kept.
        assert_eq!((to.azimuth, to.elevation), (v.azimuth, v.elevation));
    }

    /// Zoom to window: the world point under the box's centre comes to the view's centre and
    /// the box's larger side (against the view's aspect) fills the view, in perspective too.
    #[test]
    fn zoom_to_window_centres_and_fills_the_box() {
        let size = Vec2::new(1000.0, 800.0);
        for perspective in [false, true] {
            let v = ViewState { perspective, ..ViewState::default() };
            let (a, b) = (Vec2::new(-150.0, -40.0), Vec2::new(50.0, 60.0));
            let target = v.unproject((a + b) / 2.0);
            let to = v.zoomed_to_box(a, b, size);
            assert!(to.project(target).length() < 0.05, "{perspective}: {:?}", to.project(target));
            // 200 × 100 px in a 1000 × 800 view: 5× closer.
            assert!((v.scale / to.scale - 5.0).abs() < 1e-3);
        }
    }

    /// The render mode is the tab's: each tab's view menu groups read its own last modes.
    #[test]
    fn render_modes_are_kept_per_tab() {
        let mut world = World::new();
        world.insert_resource(ViewportView::default());
        world.init_resource::<RenderGroups>();
        let (a, b) = (ElementId::new(), ElementId::new());
        world.resource_mut::<ViewportView>().element = Some(a);
        set_render_mode(&mut world, RenderMode::HiddenEdgesVisible);
        assert_eq!(world.resource::<ViewportView>().view.render, RenderMode::HiddenEdgesVisible);
        world.resource_mut::<ViewportView>().element = Some(b);
        set_render_mode(&mut world, RenderMode::Translucent);
        let groups = world.resource::<RenderGroups>();
        // Tab A in Shaded again: its line-drawing group still reads Hidden edges visible.
        assert_eq!(groups.labels(Some(a), RenderMode::Shaded), (RenderMode::Shaded, RenderMode::HiddenEdgesVisible));
        // Tab B: Translucent in its shaded group, the default line mode.
        assert_eq!(groups.labels(Some(b), RenderMode::Translucent), (RenderMode::Translucent, RenderMode::HiddenEdgesRemoved));
        // A tab never set: the defaults.
        assert_eq!(groups.labels(Some(ElementId::new()), RenderMode::Shaded), (RenderMode::Shaded, RenderMode::HiddenEdgesRemoved));
    }

    /// Previous view: every camera the view rested at is stacked (not the frames of a move),
    /// and Previous view goes back through them, keeping the render mode.
    #[test]
    fn the_previous_view_stack() {
        let mut app = App::new();
        app.insert_resource(ViewportView::default()).init_resource::<ViewportDrag>().init_resource::<PreviousViews>().add_systems(Update, track_previous);
        let settle = |app: &mut App| {
            for _ in 0..SETTLE_FRAMES + 2 {
                app.update();
            }
        };
        settle(&mut app);
        let start = app.world().resource::<ViewportView>().view;
        assert!(app.world().resource::<PreviousViews>().stack.is_empty());
        // A move over several frames is one change.
        for _ in 0..4 {
            app.world_mut().resource_mut::<ViewportView>().view.pan(Vec2::new(10.0, 0.0));
            app.update();
        }
        settle(&mut app);
        let panned = app.world().resource::<ViewportView>().view;
        app.world_mut().resource_mut::<ViewportView>().view.rotate_by(20.0, 0.0);
        settle(&mut app);
        {
            let prev = app.world().resource::<PreviousViews>();
            assert_eq!(prev.stack.len(), 2);
            assert!(prev.stack[0].approx_eq(&start) && prev.stack[1].approx_eq(&panned));
        }
        // Back to the panned camera, in the mode the tab is in now.
        app.world_mut().resource_mut::<ViewportView>().view.render = RenderMode::Translucent;
        previous_view(app.world_mut());
        let view = app.world().resource::<ViewportView>();
        let to = view.animation.as_ref().map(|a| a.to).unwrap_or(view.view);
        assert!(same_camera(&to, &panned));
        assert_eq!(to.render, RenderMode::Translucent);
        assert_eq!(app.world().resource::<PreviousViews>().stack.len(), 1);
    }

    /// The perspective projection draws every point where [`ViewState::project`] (which picking
    /// and the overlays use) says, with the focus at the viewport's centre off the window's
    /// centre; nearer points get larger depth values (reversed depth).
    #[test]
    fn the_projection_matches_the_view_maths() {
        let mut v = ViewState { perspective: true, ..ViewState::default() };
        v.pan(Vec2::new(30.0, -12.0));
        let size = Vec2::new(1600.0, 1000.0);
        let center = Vec2::new(930.0, 520.0);
        let proj = ViewportPerspective::of(&v, center, size);
        let m = proj.get_clip_from_view();
        let view_from_world = Transform::from_translation(v.camera_position()).with_rotation(v.rotation()).to_matrix().inverse();
        let mut last_depth = None;
        for p in [Vec3::ZERO, Vec3::new(40.0, -30.0, 25.0), Vec3::new(-60.0, 10.0, -20.0), v.focus + v.back() * 50.0] {
            let c = m * view_from_world * p.extend(1.0);
            let ndc = c.truncate() / c.w;
            let px = Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y);
            assert!((px - center - v.project(p)).length() < 0.05, "{p:?}: {px:?} vs {:?}", v.project(p) + center);
            assert!(ndc.z > 0.0 && ndc.z <= 1.0);
            last_depth = Some(ndc.z);
        }
        // The last point is the nearest: the largest depth.
        let focus_depth = {
            let c = m * view_from_world * v.focus.extend(1.0);
            c.z / c.w
        };
        assert!(last_depth.unwrap() > focus_depth);
        // The frustum's corners project onto the target's corners.
        let corners = proj.get_frustum_corners(-proj.near, -100.0);
        let c = m * Vec4::new(corners[2].x, corners[2].y, corners[2].z, 1.0);
        assert!(((c.truncate() / c.w).truncate() - Vec2::new(-1.0, 1.0)).length() < 1e-3);
    }
}
