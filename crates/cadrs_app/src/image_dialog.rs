//! The **Image** feature's dialog ([`cadrs_core::picture`], issue #10): where a dropped picture
//! lies on its plane and how big it is. Double-clicking the Image in the feature list, or the
//! picture in the view, opens it.
//!
//! - **Plane**: the plane or flat face it lies on (a drop puts it on the default plane facing
//!   the view); click another plane, Plane feature or flat face to move it there.
//! - **X**, **Y**: its centre on the plane; **Width**, **Height**, with **Keep aspect ratio**
//!   (on: changing one changes the other); **Rotation** about its centre (degrees).
//! - **In the view** (as the Extrude's arrow): drag the picture to move it, a corner to scale it
//!   (about its centre; with Keep aspect ratio, both ways together), the round handle above it
//!   to turn it (whole degrees, 15° steps caught). The picture follows live; release is one
//!   change.
//! - Each change is a command, shown at once; ✓ keeps them as one step ("Edit Image 1"), ✕
//!   puts the picture back.
//!
//! Names: `image-dialog`, `image-plane-field`, `image-x`, `image-y`, `image-width`, `image-height`, `image-rotation`
//! (their text fields `…-field`), `image-keep-aspect`.

use crate::GizmoGroups;
use bevy::prelude::*;
use cadrs_core::commands::{ReplaceFeature, SetFeature};
use cadrs_core::picture::ImageFeature;
use cadrs_core::{ElementId, Feature, FeatureId, FeatureKind};
use cadrs_sketch::PlaneRef;
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField,
    NumberFieldCommit, NumberFieldState, OptionRow, SelectionField,
};

use crate::parts::PartCache;
use crate::viewport::{Pick, PickRequest, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ImageDialogPlugin;

impl Plugin for ImageDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ImageGizmo>()
            .gizmo_group::<ImageGizmos>()
            .add_systems(Startup, configure_gizmos)
            .add_systems(
                Update,
                (image_picks, gizmo_pointer)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                (sync_image_dialog, draw_gizmo)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |world: &mut World| {
                if world.contains_resource::<ImageSession>() {
                    finish(world);
                }
            })
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_number)
            .add_observer(on_keep_aspect);
    }
}

/// The Image whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct ImageSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub mark: usize,
    pub before: Feature,
    /// Width and height change together.
    pub keep_aspect: bool,
}

#[derive(Component)]
struct ImageDialog;

/// What the dialog shows (it is rebuilt when it changes).
#[derive(Component, Debug, Clone, PartialEq)]
struct BuiltFor(String, [String; 5], bool);

/// Which value a number field sets.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    X,
    Y,
    Width,
    Height,
    Rotation,
}

impl Field {
    const ALL: [Field; 5] = [
        Field::X,
        Field::Y,
        Field::Width,
        Field::Height,
        Field::Rotation,
    ];

    fn name(self) -> &'static str {
        match self {
            Field::X => "image-x",
            Field::Y => "image-y",
            Field::Width => "image-width",
            Field::Height => "image-height",
            Field::Rotation => "image-rotation",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Field::X => "X",
            Field::Y => "Y",
            Field::Width => "Width",
            Field::Height => "Height",
            Field::Rotation => "Rotation",
        }
    }

    fn quantity(self) -> Quantity {
        if self == Field::Rotation {
            Quantity::Angle
        } else {
            Quantity::Length
        }
    }

    fn get(self, x: &ImageFeature) -> f64 {
        match self {
            Field::X => x.center[0],
            Field::Y => x.center[1],
            Field::Width => x.width,
            Field::Height => x.height,
            Field::Rotation => x.rotation,
        }
    }
}

/// Opens an Image for editing.
pub fn edit_image(world: &mut World, feature: FeatureId) {
    if world
        .get_resource::<ImageSession>()
        .is_some_and(|s| s.feature == feature)
    {
        return;
    }
    if world.contains_resource::<ImageSession>() {
        finish(world);
    }
    crate::import_dialog::close_other_sessions(world);
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el
        .feature(feature)
        .filter(|f| matches!(f.kind, FeatureKind::Image(_)))
        .cloned()
    else {
        return;
    };
    let mark = doc.history.undo_len();
    world.insert_resource(ImageSession {
        element,
        feature,
        mark,
        before,
        keep_aspect: true,
    });
}

fn params(world: &World) -> Option<ImageFeature> {
    let s = world.get_resource::<ImageSession>()?;
    match &world
        .get_resource::<ActiveDocument>()?
        .doc
        .element(s.element)?
        .feature(s.feature)?
        .kind
    {
        FeatureKind::Image(x) => Some(x.clone()),
        _ => None,
    }
}

fn set(world: &mut World, x: ImageFeature, label: &str) {
    let Some(s) = world.get_resource::<ImageSession>().cloned() else {
        return;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    if let Err(e) = doc.execute(&SetFeature {
        element: s.element,
        feature: s.feature,
        kind: FeatureKind::Image(x),
        label: label.into(),
    }) {
        warn!("cannot change the image: {e}");
    }
}

/// ✓: keeps the changes as one step.
pub fn accept(world: &mut World) {
    let Some(s) = world.remove_resource::<ImageSession>() else {
        return;
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, format!("Edit {}", s.before.name));
    }
}

/// ✕: puts the picture back as it was.
pub fn cancel(world: &mut World) {
    let Some(s) = world.remove_resource::<ImageSession>() else {
        return;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let name = s.before.name.clone();
    doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
    let now = doc
        .doc
        .element(s.element)
        .and_then(|e| e.feature(s.feature))
        .cloned();
    if now.is_some_and(|f| f != s.before) {
        let _ = doc.execute(&ReplaceFeature {
            element: s.element,
            feature: s.before,
            label: format!("Cancel {name}"),
        });
    }
}

/// Keeps what changed (the dialog closes when something else opens).
pub fn finish(world: &mut World) {
    accept(world);
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ImageDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ImageDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

fn on_number(
    ev: On<NumberFieldCommit>,
    q: Query<&Field>,
    mut q_state: Query<&mut NumberFieldState>,
    units: Res<crate::WorkspaceUnits>,
    mut commands: Commands,
) {
    let Ok(field) = q.get(ev.entity).copied() else {
        return;
    };
    let v = units
        .0
        .eval(ev.text.trim(), field.quantity())
        .ok()
        .filter(|v| v.is_finite() && (!matches!(field, Field::Width | Field::Height) || *v > 0.0));
    let Some(v) = v else {
        if let Ok(mut s) = q_state.get_mut(ev.entity) {
            s.text = ev.text.trim().to_string();
            s.error = true;
        }
        return;
    };
    commands.queue(move |world: &mut World| {
        let Some(mut x) = params(world) else { return };
        let keep = world
            .get_resource::<ImageSession>()
            .is_some_and(|s| s.keep_aspect);
        match field {
            Field::X => x.center[0] = v,
            Field::Y => x.center[1] = v,
            Field::Width => {
                if keep {
                    x.height *= v / x.width;
                }
                x.width = v;
            }
            Field::Height => {
                if keep {
                    x.width *= v / x.height;
                }
                x.height = v;
            }
            Field::Rotation => x.rotation = v,
        }
        set(world, x, field.label());
    });
}

fn on_keep_aspect(
    ev: On<CheckboxChange>,
    q: Query<&Name>,
    mut session: Option<ResMut<ImageSession>>,
) {
    if q.get(ev.entity)
        .is_ok_and(|n| n.as_str() == "image-keep-aspect-checkbox")
        && let Some(s) = session.as_mut()
    {
        s.keep_aspect = ev.checked;
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_image_dialog(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<ImageSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    units: Res<crate::WorkspaceUnits>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_dialog: Query<(Entity, &BuiltFor, &mut FeatureDialogState), With<ImageDialog>>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(f) = doc
        .doc
        .element(s.element)
        .and_then(|e| e.feature(s.feature))
    else {
        return;
    };
    let FeatureKind::Image(x) = &f.kind else {
        return;
    };
    let error = cache.errors.get(&f.id).cloned();
    let valid = f.is_valid() && error.is_none();
    let texts = Field::ALL.map(|k| units.0.with_unit(k.get(x), k.quantity()));
    let plane = doc
        .doc
        .element(s.element)
        .map(|el| crate::sketch::plane_label(x.plane, el.features(), &cache))
        .unwrap_or_default();
    let layout = BuiltFor(plane.clone(), texts.clone(), s.keep_aspect);
    let built = q_dialog.iter().next().map(|(e, k, _)| (e, k.clone()));
    if built.as_ref().is_none_or(|(_, k)| *k != layout) {
        if let Some((e, _)) = built {
            commands.entity(e).try_despawn();
        }
        let Some(area) = q_area.iter().next() else {
            return;
        };
        let t = theme.clone();
        let keep = s.keep_aspect;
        let dialog = commands
            .spawn((
                ImageDialog,
                layout,
                DespawnOnExit(AppState::Document),
                FeatureDialog::new("image-dialog")
                    .title(f.name.clone())
                    .valid(valid)
                    .body(move |b| {
                        b.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|b| {
                            b.spawn(
                                SelectionField::new("image-plane-field")
                                    .placeholder("Plane")
                                    .value(Some(plane))
                                    .build(&t),
                            );
                            for (k, text) in Field::ALL.into_iter().zip(texts) {
                                b.spawn((
                                    k,
                                    NumberField::new(k.name(), k.label())
                                        .text(text)
                                        .label_width(64.0)
                                        .build(&t),
                                ));
                                if k == Field::Height {
                                    b.spawn(
                                        OptionRow::new("image-keep-aspect", "Keep aspect ratio")
                                            .checked(keep)
                                            .build(&t),
                                    );
                                }
                            }
                        });
                    })
                    .build(&theme),
            ))
            .id();
        commands.entity(area).add_child(dialog);
        return;
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState {
            title: f.name.clone(),
            valid,
            error: error.is_some(),
        };
        if *st != want {
            *st = want;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Plane

/// While the dialog is open, a plane, Plane feature or flat face clicked becomes the picture's
/// plane (not its own face).
fn image_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<ImageSession>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        picks.clear();
        return;
    };
    let own = s.feature;
    for p in picks.read() {
        let Some(pick) = p.0 else { continue };
        commands.queue(move |world: &mut World| {
            let plane = world
                .get_resource::<ActiveDocument>()
                .and_then(|d| d.active_element())
                .and_then(|el| match pick {
                    Pick::Plane(k) => Some(k.plane_ref()),
                    Pick::Feature(f) => cadrs_core::parts::plane_feature_ref(el.features(), f),
                    Pick::Face(f, tag) if f.feature != own => {
                        crate::parts::face_plane_of(el, f.feature, tag)
                    }
                    _ => None,
                });
            if let (Some(plane), Some(mut x)) = (plane, params(world))
                && x.plane != plane
            {
                x.plane = plane;
                set(world, x, "Image plane");
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Gizmo

/// What the gizmo has under the pointer or in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grip {
    Move,
    /// A corner, counter-clockwise from the lower left ([`ImageFeature::corners`]).
    Corner(usize),
    Rotate,
}

/// The picture's manipulators: what is hovered, and the drag in progress.
#[derive(Resource, Debug, Default)]
pub struct ImageGizmo {
    hovered: Option<Grip>,
    drag: Option<GizmoDrag>,
}

#[derive(Debug, Clone)]
struct GizmoDrag {
    grip: Grip,
    /// Where the press was on the plane, and the picture then.
    start: [f64; 2],
    before: ImageFeature,
    /// The picture as dragged.
    now: ImageFeature,
}

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ImageGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<ImageGizmos>();
    c.line.width = 2.0;
    // Over the picture and whatever is in front of it.
    c.depth_bias = -1.0;
}

/// Corner handles' half size, the turn handle's distance above the picture and its radius, and
/// how near the pointer must be (logical px).
const CORNER_PX: f32 = 5.0;
const TURN_GAP_PX: f32 = 32.0;
const TURN_PX: f32 = 7.0;
const GRAB_PX: f32 = 9.0;

/// The picture's handles on screen: its corners and its turn handle.
struct OnScreen {
    corners: [Vec2; 4],
    turn: Vec2,
}

fn to_world(plane: PlaneRef, [u, v]: [f64; 2]) -> Vec3 {
    let p = plane.frame().to_world(cadrs_sketch::Vec2::new(u, v));
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// The turn handle's place on the plane: above the middle of the picture's top edge.
fn turn_at(x: &ImageFeature, mm_per_px: f64) -> [f64; 2] {
    let (s, c) = x.rotation.to_radians().sin_cos();
    let up = x.height / 2.0 + TURN_GAP_PX as f64 * mm_per_px;
    [x.center[0] - up * s, x.center[1] + up * c]
}

fn on_screen(
    x: &ImageFeature,
    view: &crate::camera::ViewState,
    rect: &crate::viewport::ViewportRect,
) -> OnScreen {
    let screen = |p: [f64; 2]| rect.to_screen(view.project(to_world(x.plane, p)));
    OnScreen {
        corners: x.corners().map(screen),
        turn: screen(turn_at(x, view.scale as f64)),
    }
}

/// Inside a convex quad (either winding).
fn inside(q: &[Vec2; 4], p: Vec2) -> bool {
    let side = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
    let s: Vec<f32> = (0..4).map(|i| side(q[i], q[(i + 1) % 4])).collect();
    s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
}

fn grip_at(o: &OnScreen, p: Vec2) -> Option<Grip> {
    if o.turn.distance(p) <= GRAB_PX {
        return Some(Grip::Rotate);
    }
    if let Some(i) = (0..4).find(|i| o.corners[*i].distance(p) <= GRAB_PX) {
        return Some(Grip::Corner(i));
    }
    inside(&o.corners, p).then_some(Grip::Move)
}

/// The picture with a grip dragged to `at` (on its plane), snapped by `step` (mm).
fn dragged(d: &GizmoDrag, at: [f64; 2], step: f64, keep_aspect: bool) -> ImageFeature {
    let mut x = d.before.clone();
    let snap = |v: f64| ((v / step).round() * step * 1e6).round() / 1e6 + 0.0;
    match d.grip {
        Grip::Move => {
            x.center = [
                snap(d.before.center[0] + at[0] - d.start[0]),
                snap(d.before.center[1] + at[1] - d.start[1]),
            ];
        }
        Grip::Corner(_) => {
            // The pointer in the picture's own axes, from its centre.
            let (s, c) = (-x.rotation.to_radians()).sin_cos();
            let (dx, dy) = (at[0] - x.center[0], at[1] - x.center[1]);
            let (lx, ly) = ((dx * c - dy * s).abs(), (dx * s + dy * c).abs());
            let (hw, hh) = (d.before.width / 2.0, d.before.height / 2.0);
            if keep_aspect {
                // Along the corner's diagonal.
                let k = ((lx * hw + ly * hh) / (hw * hw + hh * hh)).max(1e-3);
                let w = snap((2.0 * hw * k).max(step));
                x.height = d.before.height * w / d.before.width;
                x.width = w;
            } else {
                x.width = snap((2.0 * lx).max(step));
                x.height = snap((2.0 * ly).max(step));
            }
        }
        Grip::Rotate => {
            let a = (at[1] - x.center[1])
                .atan2(at[0] - x.center[0])
                .to_degrees()
                - 90.0;
            let mut a = a.round();
            let near = (a / 15.0).round() * 15.0;
            if (a - near).abs() <= 3.0 {
                a = near;
            }
            // In [-180, 180).
            x.rotation = (a + 180.0).rem_euclid(360.0) - 180.0 + 0.0;
        }
    }
    x
}

/// Hovers, grabs, drags and releases the picture's manipulators.
#[allow(clippy::too_many_arguments)]
fn gizmo_pointer(
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    session: Option<Res<ImageSession>>,
    mut gizmo: ResMut<ImageGizmo>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<crate::viewport::ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    drag: Res<crate::viewport::ViewportDrag>,
    mut over: ResMut<crate::parts::PartOverride>,
    mut grab: ResMut<crate::assembly::ViewportGrab>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton};
    let current = session.as_ref().and_then(|s| {
        match &doc
            .as_ref()?
            .doc
            .element(s.element)?
            .feature(s.feature)?
            .kind
        {
            FeatureKind::Image(x) => Some(x.clone()),
            _ => None,
        }
    });
    let (Some(s), Some(current)) = (session, current) else {
        inputs.clear();
        gizmo.hovered = None;
        gizmo.drag = None;
        return;
    };
    let shown = gizmo
        .drag
        .as_ref()
        .map_or(current.clone(), |d| d.now.clone());
    let o = on_screen(&shown, &view.view, &rect);
    let hovered = gizmo.drag.as_ref().map(|d| d.grip).or_else(|| {
        rect.0
            .contains(drag.pointer())
            .then(|| grip_at(&o, drag.pointer()))
            .flatten()
    });
    if gizmo.hovered != hovered {
        gizmo.hovered = hovered;
    }
    let on_plane = |p: Vec2| crate::picture::on_plane(shown.plane, view.view.ray(rect.offset(p)));
    for input in inputs.read() {
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) if rect.0.contains(pos) => {
                if let (Some(grip), Some(start)) = (grip_at(&o, pos), on_plane(pos)) {
                    gizmo.drag = Some(GizmoDrag {
                        grip,
                        start,
                        before: current.clone(),
                        now: current.clone(),
                    });
                    // Not a click that picks.
                    grab.0 = true;
                }
            }
            PointerAction::Move { .. } => {
                let Some(at) = on_plane(pos) else { continue };
                if let Some(d) = gizmo.drag.as_mut() {
                    let step = crate::extrude::snap_step(1.0 / view.view.scale.max(1e-9));
                    let x = dragged(d, at, step, s.keep_aspect);
                    if x != d.now {
                        d.now = x.clone();
                        over.applied = Some((s.feature, FeatureKind::Image(x)));
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) if gizmo.drag.is_some() => {
                commands.queue(|world: &mut World| {
                    world.resource_mut::<crate::parts::PartOverride>().applied = None;
                    let Some(d) = world.resource_mut::<ImageGizmo>().drag.take() else {
                        return;
                    };
                    if d.now != d.before {
                        let label = match d.grip {
                            Grip::Move => "Move image",
                            Grip::Corner(_) => "Scale image",
                            Grip::Rotate => "Rotate image",
                        };
                        set(world, d.now, label);
                    }
                });
            }
            PointerAction::Cancel => {
                gizmo.drag = None;
                over.applied = None;
            }
            _ => {}
        }
    }
}

/// Draws the manipulators over the picture: its outline, a square at each corner, and the turn
/// handle on its stalk; the one under the pointer (or dragged) orange.
fn draw_gizmo(
    session: Option<Res<ImageSession>>,
    gizmo: Res<ImageGizmo>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<crate::viewport::ViewportView>,
    mut gizmos: Gizmos<ImageGizmos>,
) {
    let Some(s) = session else { return };
    let x = match &gizmo.drag {
        Some(d) => d.now.clone(),
        None => match doc.as_ref().and_then(|d| {
            d.doc
                .element(s.element)?
                .feature(s.feature)
                .map(|f| f.kind.clone())
        }) {
            Some(FeatureKind::Image(x)) => x,
            _ => return,
        },
    };
    let rgb = |c: [u8; 3]| Color::srgb_u8(c[0], c[1], c[2]);
    let color = |g: Grip| {
        if gizmo.hovered == Some(g) {
            rgb(crate::manipulator::ARROW_HOT)
        } else {
            rgb(crate::manipulator::ARROW_BLUE)
        }
    };
    let mm_px = view.view.scale as f64;
    let w = |p: [f64; 2]| to_world(x.plane, p);
    let c = x.corners();
    let outline = color(Grip::Move);
    for i in 0..4 {
        gizmos.line(w(c[i]), w(c[(i + 1) % 4]), outline);
    }
    // Corner squares, square to the picture, a fixed size on screen.
    let (sn, cs) = x.rotation.to_radians().sin_cos();
    let h = CORNER_PX as f64 * mm_px;
    let local =
        |p: [f64; 2], dx: f64, dy: f64| [p[0] + dx * cs - dy * sn, p[1] + dx * sn + dy * cs];
    for (i, p) in c.iter().enumerate() {
        let sq = [
            local(*p, -h, -h),
            local(*p, h, -h),
            local(*p, h, h),
            local(*p, -h, h),
        ];
        let col = color(Grip::Corner(i));
        for k in 0..4 {
            gizmos.line(w(sq[k]), w(sq[(k + 1) % 4]), col);
        }
    }
    // The turn handle: a stalk from the top edge's middle to a ring.
    let top = [(c[2][0] + c[3][0]) / 2.0, (c[2][1] + c[3][1]) / 2.0];
    let at = turn_at(&x, mm_px);
    let col = color(Grip::Rotate);
    let r = TURN_PX as f64 * mm_px;
    let len = (at[0] - top[0]).hypot(at[1] - top[1]).max(1e-9);
    let stalk_end = [
        at[0] - (at[0] - top[0]) / len * r,
        at[1] - (at[1] - top[1]) / len * r,
    ];
    gizmos.line(w(top), w(stalk_end), col);
    const N: usize = 24;
    for k in 0..N {
        let t = |k: usize| std::f64::consts::TAU * k as f64 / N as f64;
        let p = |k: usize| [at[0] + r * t(k).cos(), at[1] + r * t(k).sin()];
        gizmos.line(w(p(k)), w(p(k + 1)), col);
    }
}
