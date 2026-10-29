//! Everyone's pointer and camera, live, in a shared document ([`Pointer`]).
//!
//! - Each peer tells the room where its pointer is in its tab, many times a second while it
//!   moves: in a 3D view (Part Studio, Assembly, a PCB or CAM Studio's 3D) the model point
//!   under it, on the part there or else on the ground grid; on a PCB Studio's schematic,
//!   layout, symbol or footprint the design point; on a drawing the sheet point.
//! - Someone in the same tab (and on the same 2D view or sheet) shows in their colour
//!   ([`super::presence::color_of`]), smoothed between updates:
//!   - in 3D, as gizmos drawn over the model: a marker where their pointer is, their camera
//!     (where their eye is in perspective; in orthographic, which has none, brought in to just
//!     outside the parts, looking the way they look) and a faint line from it to the pointer,
//!     with their name beside the marker;
//!   - in 2D, as an arrow pointer with their name.
//!
//! Names: `collab-pointer-<first 8 of their cadrs ID>` (a badge), `-name` its name.

use std::collections::BTreeMap;

use crate::GizmoGroups;
use bevy::ecs::system::SystemState;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::{DocumentId, ElementId, PartId};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::bevy_iroh::prelude::{NetSender, PeerLeft, Received};
use cadrs_net::{Draft, EdaOf, Picked, Ping, Pointer, PointerAt, SketchPlane};
use cadrs_ui::prelude::*;

use super::live::Live;
use super::presence::{People, color_of};
use crate::ActiveDocument;
use crate::camera::ViewState;
use crate::eda::{Eda2d, EdaPointer, EdaUi, Mode, Subject};
use crate::viewport::{
    ActiveKind, OVERLAY_LAYER, ViewportArea, ViewportDrag, ViewportRect, ViewportView,
};

/// How often a moving pointer is sent at most (20 a second).
const SEND_EVERY: f64 = 0.05;
/// How often a resting pointer is sent again, for whoever joined since.
const RESEND_EVERY: f64 = 2.0;
/// Someone not heard from this long is gone.
const FORGET_AFTER: f64 = 15.0;
/// How quickly a shown pointer catches up with the last one heard (per second).
const FOLLOW_RATE: f32 = 18.0;
/// The arrow's size (px) and where its tip is on it (icon-rs's `cursor`: 5, 2.5 of 24).
const ARROW: f32 = 18.0;
const ARROW_TIP: Vec2 = Vec2::new(5.0 / 24.0 * ARROW, 2.5 / 24.0 * ARROW);
/// A pointer that has not moved for this long is put away (it is back when it moves).
const IDLE_AFTER: f64 = 10.0;
/// The 3D pointer's dot and the camera chip (px), and the camera's length on this screen.
const DOT: f32 = 12.0;
const CHIP: f32 = 20.0;
const CAMERA_PX: f32 = 34.0;
/// How long a ping pulses (seconds), and each pulse.
const PING_FOR: f64 = 2.7;
const PING_PULSE: f64 = 0.9;

pub(super) fn plugin(app: &mut App) {
    // Registered here as well: without the network the systems below still need the message.
    app.add_message::<Received<Pointer>>()
        .add_message::<Received<Ping>>()
        .add_message::<PeerLeft>()
        .init_resource::<Pointers>()
        .init_resource::<Pings>()
        .gizmo_group::<CollabGizmos>()
        .gizmo_group::<CollabHiddenGizmos>()
        .gizmo_group::<CollabPickGizmos>()
        .gizmo_group::<CollabGazeGizmos>()
        .add_observer(on_edge_chip_click)
        .add_systems(Startup, configure_gizmos)
        .add_systems(
            Update,
            (
                (tell, hear, hear_pings).run_if(resource_exists::<Iroh>),
                (
                    smooth,
                    sync_badges,
                    sync_edge_chips,
                    sync_overlays,
                    draw_gizmos,
                    sync_pick_tints,
                ),
            )
                .chain(),
        );
}

/// The collaborators' 3D marks (their cameras, the lines to their pointers), where the parts
/// don't hide them.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CollabGizmos;

/// The same marks faint, over the parts: where they are behind something.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CollabHiddenGizmos;

/// What a collaborator has selected, and what they are drawing: wide, over the part's own
/// edges.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CollabPickGizmos;

/// The line from a collaborator's camera to their pointer: dotted and faint, so it never reads
/// as something drawn.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CollabGazeGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<CollabGizmos>();
    config.line.width = 2.0;
    let (config, _) = store.config_mut::<CollabHiddenGizmos>();
    config.line.width = 1.5;
    config.depth_bias = -1.0;
    config.render_layers = bevy::camera::visibility::RenderLayers::layer(OVERLAY_LAYER);
    let (config, _) = store.config_mut::<CollabPickGizmos>();
    config.line.width = 4.0;
    config.depth_bias = -0.02;
    let (config, _) = store.config_mut::<CollabGazeGizmos>();
    config.line.width = 1.5;
    config.line.style = GizmoLineStyle::Dotted;
}

/// Someone else's pointer.
#[derive(Debug, Clone)]
struct Remote {
    tab: ElementId,
    /// The last heard, and the one shown (catching up with it).
    at: Option<PointerAt>,
    shown: Option<PointerAt>,
    heard: f64,
    /// When it last moved (it is put away after [`IDLE_AFTER`]).
    moved: f64,
}

impl Remote {
    fn idle(&self, t: f64) -> bool {
        t - self.moved > IDLE_AFTER
    }
}

/// The others' pointers in the shared document open now, by cadrs ID.
#[derive(Resource, Default)]
pub struct Pointers {
    document: Option<DocumentId>,
    by_id: BTreeMap<String, Remote>,
}

impl Pointers {
    /// When `id`'s pointer last moved (seconds on the real clock).
    pub fn moved_at(&self, id: &str) -> Option<f64> {
        self.by_id.get(id).map(|r| r.moved)
    }
}

fn now(world: &World) -> f64 {
    world.resource::<Time<Real>>().elapsed_secs_f64()
}

// ---------------------------------------------------------------------------------------------
// Telling

/// The app's code for a 2D view's mode, as it travels.
fn mode_code(m: Mode) -> u8 {
    match m {
        Mode::Schematic => 0,
        Mode::Layout => 1,
        Mode::ThreeD => 2,
        Mode::Symbol => 3,
        Mode::Footprint => 4,
    }
}

fn eda_of(s: Subject) -> EdaOf {
    match s {
        Subject::Board(b) => EdaOf::Board(b.0),
        Subject::Component(c) => EdaOf::Component(c.0),
    }
}

/// A 3D tab's view (not a drawing, a render or a table).
fn three_d(kind: ActiveKind) -> bool {
    !kind.is_flat()
}

/// Where this user's pointer is in the active tab, if it is over the tab's view.
fn here(world: &mut World) -> Option<PointerAt> {
    let kind = *world.resource::<ActiveKind>();
    if let Some((_, subject, mode)) = world.resource::<Eda2d>().0
        && mode != Mode::ThreeD
    {
        let p = world.resource::<EdaPointer>();
        return p.over.then_some(PointerAt::Design {
            of: eda_of(subject),
            mode: mode_code(mode),
            at: [p.at.x, p.at.y],
        });
    }
    if kind == ActiveKind::Drawing {
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.get_resource::<crate::drawing::DrawingUi>()?;
        let ((_, sheet), _) = crate::drawing::current_view(doc, ui)?;
        let at = ui.pointer?;
        return over_view(world).then_some(PointerAt::Sheet {
            sheet: sheet.0.as_u128(),
            at: at.to_array(),
        });
    }
    if !three_d(kind) || !over_view(world) {
        return None;
    }
    // Sketching: on the sketch's plane, where the sketch tools put it (snapped).
    if world.contains_resource::<crate::sketch::SketchSession>()
        && let Some(map) = world.resource::<crate::sketch_tools::SketchScreen>().active
        && let Some(c) = world.resource::<crate::sketch_tools::SketchDraw>().cursor
    {
        let f = map.plane.frame();
        return Some(PointerAt::Model(std::array::from_fn(|i| {
            (f.origin[i] + f.u[i] * c.x + f.v[i] * c.y) as f32
        })));
    }
    let offset = world
        .resource::<ViewportRect>()
        .offset(world.resource::<ViewportDrag>().pointer());
    let view = world.resource::<ViewportView>().view;
    let cache = world.resource::<crate::parts::PartCache>();
    Some(PointerAt::Model(
        model_point(cache, &view, offset).to_array(),
    ))
}

/// The model point under a screen offset: on the nearest part there, else on the ground grid
/// (the XY plane), else (looking along it) on the plane through the focus.
fn model_point(cache: &crate::parts::PartCache, view: &ViewState, offset: Vec2) -> Vec3 {
    let (o, d) = view.ray(offset);
    if let Some((_, _, t)) = crate::parts::pick_opaque_face(cache, view, offset, None) {
        return o + d * t;
    }
    if d.z.abs() > 1e-6 {
        let t = -o.z / d.z;
        if t > 0.0 && t < crate::camera::camera_far(view.scale) {
            return o + d * t;
        }
    }
    view.unproject(offset)
}

/// The pointer is over the tab's view, not a panel, menu or dialog.
fn over_view(world: &mut World) -> bool {
    let mut state = SystemState::<(Res<HoverMap>, Query<Entity, With<ViewportArea>>)>::new(world);
    state
        .get(world)
        .is_ok_and(|(hover, area)| crate::viewport::pointer_over_viewport(&hover, &area))
}

fn tell(world: &mut World, mut last: Local<Option<(Pointer, f64)>>) {
    let Some((document, room)) = world.get_resource::<Live>().map(|l| (l.document, l.room()))
    else {
        *last = None;
        return;
    };
    let Some(tab) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
    else {
        return;
    };
    let p = Pointer {
        document,
        tab,
        at: here(world),
    };
    let t = now(world);
    let due = match &*last {
        None => true,
        Some((prev, at)) => {
            (*prev != p && t - at >= SEND_EVERY) || (p.at.is_some() && t - at >= RESEND_EVERY)
        }
    };
    if !due {
        return;
    }
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(room, &p);
    }
    *last = Some((p, t));
}

/// The pings pulsing now: whose, in which tab, where, and since when.
#[derive(Resource, Default)]
struct Pings(Vec<(String, ElementId, PointerAt, f64)>);

/// "Look here" (Alt+click in a shared document): where this user points, pulsing for
/// everyone in their colour. False when there is no shared session or nowhere to point.
pub fn ping(world: &mut World) -> bool {
    let Some((document, room)) = world.get_resource::<Live>().map(|l| (l.document, l.room()))
    else {
        return false;
    };
    let Some(tab) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
    else {
        return false;
    };
    let Some(at) = here(world) else {
        return false;
    };
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(room, &Ping { document, tab, at });
    }
    let me = world
        .get_resource::<Iroh>()
        .map(|i| i.id().to_string())
        .unwrap_or_default();
    let t = now(world);
    world.resource_mut::<Pings>().0.push((me, tab, at, t));
    true
}

/// The others' pings: shown where they are when this user is in the same tab, else a toast
/// that takes them there.
#[allow(clippy::too_many_arguments)]
fn hear_pings(
    mut rx: MessageReader<Received<Ping>>,
    live: Option<Res<Live>>,
    doc: Option<Res<ActiveDocument>>,
    people: Res<People>,
    mut pings: ResMut<Pings>,
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let t = time.elapsed_secs_f64();
    pings.0.retain(|p| t - p.3 < PING_FOR);
    let Some(live) = live else {
        rx.clear();
        return;
    };
    for m in rx.read() {
        if m.msg.document != live.document {
            continue;
        }
        let id = m.from.to_string();
        pings.0.push((id.clone(), m.msg.tab, m.msg.at, t));
        let here = doc.as_ref().and_then(|d| d.active);
        if here == Some(m.msg.tab) {
            continue;
        }
        let name = people
            .by_id
            .get(&id)
            .map_or_else(|| "Someone".to_string(), |p| p.presence.name.clone());
        let place = doc
            .as_ref()
            .and_then(|d| d.doc.element(m.msg.tab))
            .map_or_else(String::new, |e| format!(" in {}", e.name));
        let toast = cadrs_ui::show_notification(
            &mut commands,
            &theme,
            cadrs_ui::Notification::info(format!("{name} wants you to look here{place}"))
                .name("collab-ping-toast")
                .seconds(8.0),
        );
        let link = cadrs_ui::toast_action(&mut commands, &theme, toast, "collab-ping-go", "Go to");
        commands
            .entity(link)
            .observe(move |_: On<Activate>, mut commands: Commands| {
                let id = id.clone();
                commands.queue(move |w: &mut World| {
                    cadrs_ui::close_toasts(w);
                    super::presence::go_to(w, &id);
                });
            });
    }
}

// ---------------------------------------------------------------------------------------------
// Hearing

fn hear(
    mut rx: MessageReader<Received<Pointer>>,
    mut left: MessageReader<PeerLeft>,
    live: Option<Res<Live>>,
    mut pointers: ResMut<Pointers>,
    time: Res<Time<Real>>,
) {
    let t = time.elapsed_secs_f64();
    let Some(live) = live else {
        if pointers.document.is_some() || !pointers.by_id.is_empty() {
            *pointers = Pointers::default();
        }
        rx.clear();
        left.clear();
        return;
    };
    if pointers.document != Some(live.document) {
        *pointers = Pointers {
            document: Some(live.document),
            by_id: BTreeMap::new(),
        };
    }
    for m in rx.read() {
        if m.msg.document != live.document {
            continue;
        }
        let entry = pointers.by_id.entry(m.from.to_string()).or_insert(Remote {
            tab: m.msg.tab,
            at: None,
            shown: None,
            heard: t,
            moved: t,
        });
        if entry.at != m.msg.at || entry.tab != m.msg.tab {
            entry.moved = t;
        }
        // Another tab, or another view of it: shown where it is at once, not slid there.
        if entry.tab != m.msg.tab || !same_place(entry.shown, m.msg.at) {
            entry.shown = m.msg.at;
        }
        entry.tab = m.msg.tab;
        entry.at = m.msg.at;
        entry.heard = t;
    }
    for l in left.read() {
        if l.room == live.room() {
            pointers.by_id.remove(&l.id.to_string());
        }
    }
    pointers.by_id.retain(|_, r| t - r.heard <= FORGET_AFTER);
}

/// Both on the same view (3D, the same 2D view, the same sheet), so one can slide to the other.
fn same_place(a: Option<PointerAt>, b: Option<PointerAt>) -> bool {
    match (a, b) {
        (Some(PointerAt::Model(_)), Some(PointerAt::Model(_))) => true,
        (
            Some(PointerAt::Design { of, mode, .. }),
            Some(PointerAt::Design { of: o, mode: m, .. }),
        ) => of == o && mode == m,
        (Some(PointerAt::Sheet { sheet, .. }), Some(PointerAt::Sheet { sheet: s, .. })) => {
            sheet == s
        }
        _ => false,
    }
}

/// Each shown pointer a step closer to the last one heard.
fn smooth(mut pointers: ResMut<Pointers>, time: Res<Time<Real>>) {
    let k = 1.0 - (-FOLLOW_RATE * time.delta_secs()).exp();
    for r in pointers.by_id.values_mut() {
        r.shown = match (r.shown, r.at) {
            (Some(PointerAt::Model(a)), Some(PointerAt::Model(b))) => Some(PointerAt::Model(
                Vec3::from_array(a).lerp(Vec3::from_array(b), k).to_array(),
            )),
            (
                Some(PointerAt::Design { at: a, .. }),
                Some(PointerAt::Design { of, mode, at: b }),
            ) => {
                let lerp = |a: i64, b: i64| a + ((b - a) as f64 * k as f64).round() as i64;
                Some(PointerAt::Design {
                    of,
                    mode,
                    at: [lerp(a[0], b[0]), lerp(a[1], b[1])],
                })
            }
            (Some(PointerAt::Sheet { at: a, .. }), Some(PointerAt::Sheet { sheet, at: b })) => {
                Some(PointerAt::Sheet {
                    sheet,
                    at: Vec2::from_array(a).lerp(Vec2::from_array(b), k).to_array(),
                })
            }
            (_, at) => at,
        };
    }
}

// ---------------------------------------------------------------------------------------------
// Showing

/// What this user's own tab shows, to place the others' pointers on.
struct Here<'a> {
    tab: Option<ElementId>,
    kind: ActiveKind,
    eda: Option<(Subject, Mode, cadrs_eda::view::View)>,
    /// The sheet shown, its view and where on the screen it is drawn.
    sheet: Option<(u128, crate::drawing::SheetView, Rect)>,
    view: &'a ViewState,
    rect: Rect,
}

impl Here<'_> {
    /// Where a pointer is on this screen (logical px), and whether it is a 3D one, when it is
    /// on the tab's view.
    fn screen(&self, tab: ElementId, at: PointerAt) -> Option<(Vec2, bool)> {
        self.screen_any(tab, at)
            .filter(|(p, _)| self.rect.contains(*p))
    }

    /// A design point of the 2D view `at` (a pointer) is on, on this screen.
    fn design(&self, at: PointerAt, p: [i64; 2]) -> Option<Vec2> {
        let PointerAt::Design { of, mode, .. } = at else {
            return None;
        };
        let (subject, m, view) = self.eda?;
        if eda_of(subject) != of || mode_code(m) != mode {
            return None;
        }
        let s = view.to_screen([p[0] as f64, p[1] as f64]);
        Some(self.rect.min + Vec2::new(s[0] as f32, s[1] as f32))
    }

    /// As [`Self::screen`], also off the view's edges (where an edge arrow points).
    fn screen_any(&self, tab: ElementId, at: PointerAt) -> Option<(Vec2, bool)> {
        if self.tab != Some(tab) {
            return None;
        }
        let (p, model) = match at {
            PointerAt::Model(p) => {
                if !three_d(self.kind) || self.eda.is_some_and(|e| e.1 != Mode::ThreeD) {
                    return None;
                }
                let p = Vec3::from_array(p);
                (self.rect.center() + self.view.project(p), true)
            }
            PointerAt::Design { of, mode, at } => {
                let (subject, m, view) = self.eda?;
                if eda_of(subject) != of || mode_code(m) != mode {
                    return None;
                }
                let s = view.to_screen([at[0] as f64, at[1] as f64]);
                (self.rect.min + Vec2::new(s[0] as f32, s[1] as f32), false)
            }
            PointerAt::Sheet { sheet, at } => {
                let (id, view, area) = self.sheet?;
                if id != sheet {
                    return None;
                }
                (
                    crate::drawing::sheet_to_screen(view, area, Vec2::from_array(at)),
                    false,
                )
            }
        };
        p.is_finite().then_some((p, model))
    }
}

/// A collaborator's pointer badge: whose, and the label it shows (their name, and what they
/// have open).
#[derive(Component)]
struct PointerBadge {
    id: String,
    label: String,
}

/// The badge's arrow (in 2D) and its dot (in 3D, where the point is on the model).
#[derive(Component)]
struct BadgeArrow;
#[derive(Component)]
struct BadgeDot;

/// The chip with a collaborator's initials at their camera: whose.
#[derive(Component)]
struct CameraChip(String, bool);

/// Shows the badge's arrow in 2D and its dot in 3D.
fn shape_display(model: bool, arrow: bool) -> Display {
    if model != arrow {
        Display::Flex
    } else {
        Display::None
    }
}

fn badge(t: &Theme, id: &str, label: &str, model: bool, color: Color) -> impl Bundle {
    let short: String = id.chars().take(8).collect();
    let arrow = |offset: Vec2, tint: Color| {
        cadrs_ui::icon::icon_in(
            "cursor",
            ARROW,
            tint,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(offset.x),
                top: Val::Px(offset.y),
                ..default()
            },
        )
    };
    (
        Name::new(format!("collab-pointer-{short}")),
        PointerBadge {
            id: id.to_string(),
            label: label.to_string(),
        },
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(0.0),
            height: Val::Px(0.0),
            ..default()
        },
        GlobalZIndex(5),
        Pickable::IGNORE,
        children![
            (
                BadgeArrow,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-ARROW_TIP.x),
                    top: Val::Px(-ARROW_TIP.y),
                    width: Val::Px(ARROW),
                    height: Val::Px(ARROW),
                    display: shape_display(model, true),
                    ..default()
                },
                Pickable::IGNORE,
                // A white edge (the arrow nudged a pixel each way) so it shows on any colour.
                children![
                    arrow(Vec2::new(-1.0, 0.0), Color::WHITE),
                    arrow(Vec2::new(1.0, 0.0), Color::WHITE),
                    arrow(Vec2::new(0.0, -1.0), Color::WHITE),
                    arrow(Vec2::new(0.0, 1.0), Color::WHITE),
                    arrow(Vec2::ZERO, color),
                ],
            ),
            (
                BadgeDot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-DOT / 2.0),
                    top: Val::Px(-DOT / 2.0),
                    width: Val::Px(DOT),
                    height: Val::Px(DOT),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::MAX,
                    display: shape_display(model, false),
                    ..default()
                },
                BackgroundColor(color),
                BorderColor::all(Color::WHITE),
                Pickable::IGNORE,
            ),
            (
                Name::new(format!("collab-pointer-{short}-name")),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(ARROW - ARROW_TIP.x - 2.0),
                    top: Val::Px(ARROW - ARROW_TIP.y - 2.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius)),
                    ..default()
                },
                BackgroundColor(color),
                Pickable::IGNORE,
                children![(
                    t.text(
                        label.to_string(),
                        t.font_sm,
                        FontWeight::MEDIUM,
                        cadrs_ui::Avatar::ink_on(color)
                    ),
                    Pickable::IGNORE,
                )],
            ),
        ],
    )
}

fn camera_chip(t: &Theme, id: &str, name: &str, color: Color, stale: bool) -> impl Bundle {
    let short: String = id.chars().take(8).collect();
    (
        Name::new(format!("collab-camera-{short}")),
        CameraChip(id.to_string(), stale),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(CHIP),
            height: Val::Px(CHIP),
            margin: UiRect::new(
                Val::Px(-CHIP / 2.0),
                Val::ZERO,
                Val::Px(-CHIP / 2.0),
                Val::ZERO,
            ),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::MAX,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(Color::WHITE),
        GlobalZIndex(5),
        Pickable::IGNORE,
        children![(
            t.text(
                cadrs_ui::Avatar::initials(name),
                t.font_xs,
                FontWeight::BOLD,
                cadrs_ui::Avatar::ink_on(color)
            ),
            Pickable::IGNORE,
        )],
    )
}

/// Someone's name, what they have open and what they do (`Alex · Sketch 1 · Line`), once
/// their presence says it (a pointer can come first: the start of their cadrs ID until then).
fn label_of(people: &People, doc: Option<&ActiveDocument>, id: &str) -> String {
    let Some(p) = people.by_id.get(id) else {
        return id.chars().take(8).collect();
    };
    if people.stale.contains(id) {
        return format!("{} · reconnecting…", p.presence.name);
    }
    let mut label = p.presence.name.clone();
    if let Some(f) = doc.and_then(|d| super::presence::editing_name(d, &p.presence)) {
        label.push_str(&format!(" · {f}"));
    }
    if let Some(d) = people
        .doing
        .get(id)
        .map(|a| &a.what)
        .filter(|d| !d.is_empty())
    {
        label.push_str(&format!(" · {d}"));
    }
    label
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_badges(
    (pointers, pings, iroh): (Res<Pointers>, Res<Pings>, Option<Res<Iroh>>),
    people: Res<People>,
    (doc, kind, eda, eda_ui, drawing): (
        Option<Res<ActiveDocument>>,
        Res<ActiveKind>,
        Res<Eda2d>,
        Res<EdaUi>,
        Option<Res<crate::drawing::DrawingUi>>,
    ),
    (view, rect, cache, following, time): (
        Res<ViewportView>,
        Res<ViewportRect>,
        Res<crate::parts::PartCache>,
        Option<Res<super::presence::Following>>,
        Res<Time<Real>>,
    ),
    area: Query<Entity, With<ViewportArea>>,
    mut badges: Query<(Entity, &PointerBadge, &mut Node, &Children)>,
    mut shapes: Query<
        (&mut Node, Has<BadgeArrow>),
        (
            Or<(With<BadgeArrow>, With<BadgeDot>)>,
            Without<PointerBadge>,
        ),
    >,
    mut chips: Query<
        (Entity, &CameraChip, &mut Node),
        (
            Without<PointerBadge>,
            Without<BadgeArrow>,
            Without<BadgeDot>,
        ),
    >,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let t = time.elapsed_secs_f64();
    let here = here_of(
        doc.as_deref(),
        *kind,
        &eda,
        &eda_ui,
        drawing.as_deref(),
        &view,
        &rect,
    );
    let Ok(area) = area.single() else {
        return;
    };

    // The pointers (not while their ping shows there: it says "look here" in their place).
    let pinging: Vec<&str> = pings
        .0
        .iter()
        .filter(|(_, tab, at, _)| here.screen(*tab, *at).is_some())
        .map(|(id, ..)| id.as_str())
        .collect();
    let mut want: BTreeMap<&str, (Vec2, bool)> = BTreeMap::new();
    for (id, r) in &pointers.by_id {
        if !r.idle(t)
            && !pinging.contains(&id.as_str())
            && let Some(at) = r.shown
            && let Some(s) = here.screen(r.tab, at)
        {
            want.insert(id.as_str(), s);
        }
    }
    for (e, b, mut node, children) in &mut badges {
        let Some(&(p, model)) = want.get(b.id.as_str()) else {
            commands.entity(e).despawn();
            continue;
        };
        if b.label != label_of(&people, doc.as_deref(), &b.id) {
            // Made again below, with the new label.
            commands.entity(e).despawn();
            continue;
        }
        let local = p - rect.0.min;
        node.left = Val::Px(local.x);
        node.top = Val::Px(local.y);
        for c in children.iter() {
            if let Ok((mut n, arrow)) = shapes.get_mut(c) {
                let display = shape_display(model, arrow);
                if n.display != display {
                    n.display = display;
                }
            }
        }
        want.remove(b.id.as_str());
    }
    for (id, (p, model)) in want {
        let label = label_of(&people, doc.as_deref(), id);
        let local = p - rect.0.min;
        let e = commands
            .spawn(badge(&theme, id, &label, model, people.shown_color(id)))
            .id();
        commands.entity(e).entry::<Node>().and_modify(move |mut n| {
            n.left = Val::Px(local.x);
            n.top = Val::Px(local.y);
        });
        commands.entity(area).add_child(e);
    }

    // The chips at their cameras (in 3D).
    let me = iroh.as_ref().map(|i| i.id().to_string());
    let mut cams: BTreeMap<&str, Vec2> = BTreeMap::new();
    if here.tab.is_some() && three_d(here.kind) && here.eda.is_none_or(|e| e.1 == Mode::ThreeD) {
        for (id, person) in &people.by_id {
            // (Not whoever this user follows, nor whoever follows this user: that camera is
            // this view.)
            if person.presence.tab != here.tab
                || following.as_ref().is_some_and(|f| f.leader == *id)
                || (me.is_some() && person.presence.following == me)
            {
                continue;
            }
            let Some(theirs) = person
                .presence
                .view
                .as_ref()
                .map(super::presence::view_state)
            else {
                continue;
            };
            let p = rect.0.center() + view.view.project(eye_of(&theirs, &cache));
            if rect.0.contains(p) {
                cams.insert(id.as_str(), p);
            }
        }
    }
    for (e, chip, mut node) in &mut chips {
        // (Made again when their connection goes stale or comes back: its colour.)
        match cams
            .remove(chip.0.as_str())
            .filter(|_| chip.1 == people.stale.contains(&chip.0))
        {
            Some(p) => {
                let local = p - rect.0.min;
                node.left = Val::Px(local.x);
                node.top = Val::Px(local.y);
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (id, p) in cams {
        let name = people
            .by_id
            .get(id)
            .map(|p| p.presence.name.clone())
            .unwrap_or_default();
        let local = p - rect.0.min;
        let stale = people.stale.contains(id);
        let e = commands
            .spawn(camera_chip(
                &theme,
                id,
                &name,
                people.shown_color(id),
                stale,
            ))
            .id();
        commands.entity(e).entry::<Node>().and_modify(move |mut n| {
            n.left = Val::Px(local.x);
            n.top = Val::Px(local.y);
        });
        commands.entity(area).add_child(e);
    }
}

/// A piece of a collaborator's draft on a 2D view, their pointer off the view's edge, a ping's
/// ring: made again each frame.
#[derive(Component)]
struct Overlay;

/// A straight piece from `a` to `b` (local px), `width` wide.
fn segment(a: Vec2, b: Vec2, width: f32, color: Color) -> impl Bundle {
    let d = b - a;
    let (len, mid) = (d.length(), (a + b) / 2.0);
    (
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(mid.x - len / 2.0),
            top: Val::Px(mid.y - width / 2.0),
            width: Val::Px(len),
            height: Val::Px(width),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        bevy::ui::UiTransform {
            rotation: Rot2::radians(d.y.atan2(d.x)),
            ..default()
        },
        BackgroundColor(color),
        GlobalZIndex(4),
        Pickable::IGNORE,
    )
}

/// How far in from the view's edges an edge chip sits (px).
const EDGE_INSET: f32 = 60.0;

/// Where a point beyond the view's edge shows: on the view's border inset by [`EDGE_INSET`],
/// toward it; and the direction to it.
fn edge_point(rect: Rect, p: Vec2) -> Option<(Vec2, Vec2)> {
    let inset = rect.inflate(-EDGE_INSET);
    if inset.width() <= 0.0 || inset.height() <= 0.0 {
        return None;
    }
    let c = inset.center();
    let d = p - c;
    let reach = |half: f32, d: f32| {
        if d.abs() > 1e-3 {
            half / d.abs()
        } else {
            f32::MAX
        }
    };
    let k = reach(inset.half_size().x, d.x).min(reach(inset.half_size().y, d.y));
    Some((c + d * k, d.normalize_or_zero()))
}

/// A collaborator's pointer beyond the view's edge: their chip and label there, an arrow toward
/// it; a click takes the view to it. Whose, and what its label says.
#[derive(Component)]
struct EdgeChip {
    id: String,
    label: String,
}

fn edge_chip(t: &Theme, id: &str, name: &str, label: &str) -> impl Bundle {
    let color = color_of(id);
    let short: String = id.chars().take(8).collect();
    (
        Name::new(format!("collab-edge-{short}")),
        EdgeChip {
            id: id.to_string(),
            label: label.to_string(),
        },
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(0.0),
            height: Val::Px(0.0),
            ..default()
        },
        GlobalZIndex(7),
        Pickable::IGNORE,
        children![
            (
                Name::new(format!("collab-edge-{short}-chip")),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-15.0),
                    top: Val::Px(-15.0),
                    padding: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                cadrs_ui::color_scheme::KeepFill,
                Tooltip::new(format!("{label}: click to look there")),
                children![(
                    cadrs_ui::Avatar::new(format!("collab-edge-{short}-face"), name.to_string())
                        .size(26.0)
                        .color(color)
                        .build(t),
                    cadrs_ui::color_scheme::KeepFill,
                    Pickable::IGNORE,
                )],
            ),
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(18.0),
                    top: Val::Px(-11.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius)),
                    ..default()
                },
                BackgroundColor(color),
                cadrs_ui::color_scheme::KeepFill,
                Pickable::IGNORE,
                children![(
                    t.text(
                        label.to_string(),
                        t.font_sm,
                        FontWeight::MEDIUM,
                        cadrs_ui::Avatar::ink_on(color)
                    ),
                    Pickable::IGNORE,
                )],
            ),
        ],
    )
}

/// Keeps an [`EdgeChip`] at the edge toward each pointer beyond it (not idle ones).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_edge_chips(
    pointers: Res<Pointers>,
    people: Res<People>,
    (doc, kind, eda, eda_ui, drawing): (
        Option<Res<ActiveDocument>>,
        Res<ActiveKind>,
        Res<Eda2d>,
        Res<EdaUi>,
        Option<Res<crate::drawing::DrawingUi>>,
    ),
    (view, rect, time): (Res<ViewportView>, Res<ViewportRect>, Res<Time<Real>>),
    area: Query<Entity, With<ViewportArea>>,
    mut chips: Query<(Entity, &EdgeChip, &mut Node)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let here = here_of(
        doc.as_deref(),
        *kind,
        &eda,
        &eda_ui,
        drawing.as_deref(),
        &view,
        &rect,
    );
    let t = time.elapsed_secs_f64();
    let mut want: BTreeMap<&str, (Vec2, Vec2)> = BTreeMap::new();
    for (id, r) in &pointers.by_id {
        if let Some((p, _)) = r
            .shown
            .filter(|_| !r.idle(t))
            .and_then(|at| here.screen_any(r.tab, at))
            && !rect.0.contains(p)
            && let Some(e) = edge_point(rect.0, p)
        {
            want.insert(id.as_str(), e);
        }
    }
    let local = |at: Vec2| at - rect.0.min;
    for (e, chip, mut node) in &mut chips {
        let label = label_of(&people, doc.as_deref(), &chip.id);
        match want.remove(chip.id.as_str()) {
            Some((at, _)) if label == chip.label => {
                let at = local(at);
                node.left = Val::Px(at.x);
                node.top = Val::Px(at.y);
            }
            // Gone, or its label changed (made again below).
            _ => commands.entity(e).despawn(),
        }
    }
    let Ok(area) = area.single() else {
        return;
    };
    for (id, (at, _)) in want {
        let name = people
            .by_id
            .get(id)
            .map_or_else(String::new, |p| p.presence.name.clone());
        let label = label_of(&people, doc.as_deref(), id);
        let e = commands.spawn(edge_chip(&theme, id, &name, &label)).id();
        let at = local(at);
        commands.entity(e).entry::<Node>().and_modify(move |mut n| {
            n.left = Val::Px(at.x);
            n.top = Val::Px(at.y);
        });
        commands.entity(area).add_child(e);
    }
}

/// A click on an edge chip takes this user's view to that pointer.
fn on_edge_chip_click(
    ev: On<bevy::picking::events::PointerClick>,
    parents: Query<&ChildOf>,
    chips: Query<&EdgeChip>,
    mut commands: Commands,
) {
    let Some(id) = std::iter::once(ev.entity)
        .chain(parents.iter_ancestors(ev.entity))
        .find_map(|e| chips.get(e).ok())
        .map(|c| c.id.clone())
    else {
        return;
    };
    commands.queue(move |world: &mut World| look_at(world, &id));
}

/// This user's view, moved to centre `id`'s pointer (in 3D, or on the same 2D view).
fn look_at(world: &mut World, id: &str) {
    let Some(at) = world
        .resource::<Pointers>()
        .by_id
        .get(id)
        .and_then(|r| r.at.or(r.shown))
    else {
        return;
    };
    match at {
        PointerAt::Model(p) => {
            let mut vv = world.resource_mut::<ViewportView>();
            let mut to = vv.target();
            to.focus = Vec3::from_array(p);
            vv.animate_to(to);
        }
        PointerAt::Design { at, .. } => {
            let Some(key) = world.resource::<Eda2d>().0 else {
                return;
            };
            let size = world.resource::<ViewportRect>().0.size();
            let mut ui = world.resource_mut::<EdaUi>();
            if let Some((view, fitted)) = ui.views.get_mut(&key) {
                let s = view.to_screen([at[0] as f64, at[1] as f64]);
                view.pan([size.x as f64 / 2.0 - s[0], size.y as f64 / 2.0 - s[1]]);
                *fitted = false;
            }
        }
        PointerAt::Sheet { .. } => {}
    }
}

/// A small pill of text beside a mark (local px): "Alex's view", "Look here".
fn mark_label(t: &Theme, text: String, at: Vec2, color: Color) -> impl Bundle {
    (
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            ..default()
        },
        BackgroundColor(color),
        GlobalZIndex(6),
        Pickable::IGNORE,
        children![(
            t.text(
                text,
                t.font_sm,
                FontWeight::MEDIUM,
                cadrs_ui::Avatar::ink_on(color)
            ),
            Pickable::IGNORE,
        )],
    )
}

/// A ring (local px), `width` wide.
fn ring(at: Vec2, radius: f32, width: f32, color: Color) -> impl Bundle {
    (
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x - radius),
            top: Val::Px(at.y - radius),
            width: Val::Px(radius * 2.0),
            height: Val::Px(radius * 2.0),
            border: UiRect::all(Val::Px(width)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BorderColor::all(color),
        GlobalZIndex(6),
        Pickable::IGNORE,
    )
}

/// On 2D views, what the collaborators draw (a wire, a route so far, to their pointer); on
/// every view, an arrow at the edge toward a pointer beyond it, and the pings' rings.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_overlays(
    pointers: Res<Pointers>,
    people: Res<People>,
    pings: Res<Pings>,
    (doc, kind, eda, eda_ui, drawing): (
        Option<Res<ActiveDocument>>,
        Res<ActiveKind>,
        Res<Eda2d>,
        Res<EdaUi>,
        Option<Res<crate::drawing::DrawingUi>>,
    ),
    (view, rect, time, cache, iroh): (
        Res<ViewportView>,
        Res<ViewportRect>,
        Res<Time<Real>>,
        Res<crate::parts::PartCache>,
        Option<Res<Iroh>>,
    ),
    area: Query<Entity, With<ViewportArea>>,
    old: Query<Entity, With<Overlay>>,
    views: Option<Res<crate::drawing::views::ViewCache>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let Ok(area) = area.single() else {
        return;
    };
    let here = here_of(
        doc.as_deref(),
        *kind,
        &eda,
        &eda_ui,
        drawing.as_deref(),
        &view,
        &rect,
    );
    let t = time.elapsed_secs_f64();
    let local = |p: Vec2| p - rect.0.min;
    let mut spawned = Vec::new();
    // What they draw on this 2D view.
    for (id, a) in &people.doing {
        let (Some(Draft::Design(pts)), Some(r)) = (&a.draft, pointers.by_id.get(id)) else {
            continue;
        };
        let Some(at) = r.shown.or(r.at) else {
            continue;
        };
        let screen: Vec<Vec2> = pts.iter().filter_map(|p| here.design(at, *p)).collect();
        if screen.len() != pts.len() {
            continue;
        }
        let color = color_of(id);
        for w in screen.windows(2) {
            spawned.push(
                commands
                    .spawn(segment(local(w[0]), local(w[1]), 4.0, color))
                    .id(),
            );
        }
        // A dot at each point placed.
        for p in &screen {
            spawned.push(commands.spawn(ring(local(*p), 4.5, 4.5, color)).id());
        }
        // The piece not placed yet, to their pointer: dashed.
        if let (Some(last), Some((to, _))) = (screen.last().copied(), here.screen(r.tab, at)) {
            let (len, dir) = ((to - last).length(), (to - last).normalize_or_zero());
            let mut d = 0.0;
            while d < len {
                let (a, b) = (last + dir * d, last + dir * (d + 8.0).min(len));
                spawned.push(
                    commands
                        .spawn(segment(local(a), local(b), 4.0, color.with_alpha(0.8)))
                        .id(),
                );
                d += 14.0;
            }
        }
    }
    let me = iroh.map(|i| i.id().to_string());
    // The arrows of the edge chips (`sync_edge_chips`): a chevron on the chip's side toward
    // the pointer, aimed at it.
    for (id, r) in &pointers.by_id {
        if let Some((p, _)) = r
            .shown
            .filter(|_| !r.idle(t))
            .and_then(|at| here.screen_any(r.tab, at))
            && !rect.0.contains(p)
            && let Some((at, dir)) = edge_point(rect.0, p)
        {
            let color = color_of(id);
            let tip = at + dir * 30.0;
            let side = Vec2::new(-dir.y, dir.x) * 10.0;
            let back = tip - dir * 11.0;
            for wing in [back + side, back - side] {
                spawned.push(
                    commands
                        .spawn(segment(local(wing), local(tip), 4.5, color))
                        .id(),
                );
            }
        }
    }
    // On a drawing: the view someone points at, tinted faintly in their colour, with its name.
    if let (Some((sheet_id, sv, area_rect)), Some(doc), Some(ui), Some(views)) = (
        here.sheet,
        doc.as_deref(),
        drawing.as_deref(),
        views.as_deref(),
    ) && let Some((did, d)) = crate::drawing::active_drawing(doc)
    {
        let index = ui.sheet_index(did, d);
        for (id, r) in &pointers.by_id {
            let Some(PointerAt::Sheet { sheet, at }) = r.shown.filter(|_| !r.idle(t)) else {
                continue;
            };
            if sheet != sheet_id || Some(r.tab) != here.tab {
                continue;
            }
            let Some(v) =
                crate::drawing::views::view_at(d, index, views, Vec2::from_array(at), 2.0)
                    .and_then(|vid| d.sheets.get(index)?.views.iter().find(|v| v.id == vid))
            else {
                continue;
            };
            let (lo, hi) = crate::drawing::views::sheet_bounds(v, views.geometry(v).as_deref());
            let corner = |c: [f64; 2]| {
                crate::drawing::sheet_to_screen(sv, area_rect, Vec2::new(c[0] as f32, c[1] as f32))
            };
            let (a, b) = (corner(lo), corner(hi));
            let (min, max) = (a.min(b) - Vec2::splat(1.0), a.max(b) + Vec2::splat(1.0));
            let color = color_of(id);
            let at = local(min);
            spawned.push(
                commands
                    .spawn((
                        Overlay,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(at.x),
                            top: Val::Px(at.y),
                            width: Val::Px(max.x - min.x),
                            height: Val::Px(max.y - min.y),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(color.with_alpha(0.12)),
                        BorderColor::all(color.with_alpha(0.85)),
                        GlobalZIndex(3),
                        Pickable::IGNORE,
                    ))
                    .id(),
            );
            // The view's name (their own name is at their pointer).
            let label = v.name.clone();
            spawned.push(
                commands
                    .spawn(mark_label(
                        &theme,
                        label,
                        at + Vec2::new(0.0, -22.0),
                        color.with_alpha(0.9),
                    ))
                    .id(),
            );
        }
    }
    // In 3D, someone whose mouse is off their view: a ring where they look (its centre).
    if here.tab.is_some() && three_d(here.kind) && here.eda.is_none_or(|e| e.1 == Mode::ThreeD) {
        for (id, person) in &people.by_id {
            let pointing = pointers
                .by_id
                .get(id)
                .is_some_and(|r| r.at.is_some() && !r.idle(t) && Some(r.tab) == here.tab);
            // (Whoever follows this user looks at this very view.)
            let follows_me = me.is_some() && person.presence.following == me;
            if person.presence.tab != here.tab || pointing || follows_me {
                continue;
            }
            let Some(v) = person.presence.view.as_ref() else {
                continue;
            };
            let p = rect.0.center() + view.view.project(Vec3::from_array(v.focus));
            if !rect.0.contains(p) {
                continue;
            }
            let color = color_of(id);
            let at = local(p);
            // On their camera's chip (looking straight at it): the chip says it, with the label.
            let eye = rect.0.center()
                + view
                    .view
                    .project(eye_of(&super::presence::view_state(v), &cache));
            if eye.distance(p) > 20.0 {
                spawned.push(
                    commands
                        .spawn(ring(at, 16.0, 2.5, color.with_alpha(0.85)))
                        .id(),
                );
                spawned.push(commands.spawn(ring(at, 3.0, 3.0, color)).id());
            }
            let label = format!("{}'s view", person.presence.name);
            spawned.push(
                commands
                    .spawn(mark_label(
                        &theme,
                        label,
                        at + Vec2::new(18.0, 10.0),
                        color.with_alpha(0.85),
                    ))
                    .id(),
            );
        }
    }
    // Pings: rings growing out of the point, one after another, and "Look here"; beyond the
    // view's edge, at its edge chip.
    for (id, tab, at, start) in &pings.0 {
        let Some(p) = here.screen_any(*tab, *at).and_then(|(p, _)| {
            if rect.0.contains(p) {
                Some(p)
            } else {
                edge_point(rect.0, p).map(|(e, _)| e)
            }
        }) else {
            continue;
        };
        let color = color_of(id);
        let age = t - start;
        let left = (1.0 - (age / PING_FOR) as f32).clamp(0.0, 1.0);
        for k in [0.0, 0.33, 0.66] {
            let phase = (age / PING_PULSE + k).fract() as f32;
            let radius = 8.0 + 46.0 * phase;
            let fade = (1.0 - phase).powf(0.7) * left;
            spawned.push(
                commands
                    .spawn(ring(local(p), radius, 4.0, color.with_alpha(fade)))
                    .id(),
            );
        }
        let name = people
            .by_id
            .get(id)
            .map(|p| format!("{}: look here", p.presence.name))
            .unwrap_or_else(|| "Look here".into());
        spawned.push(
            commands
                .spawn(mark_label(
                    &theme,
                    name,
                    local(p) + Vec2::new(14.0, -30.0),
                    color.with_alpha(left.max(0.3)),
                ))
                .id(),
        );
        let at = local(p);
        spawned.push(
            commands
                .spawn((
                    Overlay,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(at.x - 5.0),
                        top: Val::Px(at.y - 5.0),
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(color),
                    GlobalZIndex(6),
                    Pickable::IGNORE,
                ))
                .id(),
        );
    }
    commands.entity(area).add_children(&spawned);
}

fn here_of<'a>(
    doc: Option<&ActiveDocument>,
    kind: ActiveKind,
    eda: &Eda2d,
    eda_ui: &EdaUi,
    drawing: Option<&crate::drawing::DrawingUi>,
    view: &'a ViewportView,
    rect: &ViewportRect,
) -> Here<'a> {
    let tab = doc.and_then(|d| d.active);
    let eda_view = eda.0.and_then(|(el, subject, mode)| {
        let v = eda_ui.views.get(&(el, subject, mode))?.0;
        Some((subject, mode, v))
    });
    let sheet = (kind == ActiveKind::Drawing)
        .then(|| doc.zip(drawing))
        .flatten()
        .and_then(|(doc, ui)| {
            let ((_, id), v) = crate::drawing::current_view(doc, ui)?;
            Some((id.0.as_u128(), v, crate::drawing::sheet_area(rect, ui)))
        });
    Here {
        tab,
        kind,
        eda: eda_view,
        sheet,
        view: &view.view,
        rect: rect.0,
    }
}

/// In 3D: each collaborator's camera and the line from it to their pointer, strong where the
/// parts don't hide them and faint where they do.
#[allow(clippy::too_many_arguments)]
fn draw_gizmos(
    pointers: Res<Pointers>,
    people: Res<People>,
    (doc, kind, eda): (Option<Res<ActiveDocument>>, Res<ActiveKind>, Res<Eda2d>),
    view: Res<ViewportView>,
    cache: Res<crate::parts::PartCache>,
    following: Option<Res<super::presence::Following>>,
    time: Res<Time<Real>>,
    mut shown: Gizmos<CollabGizmos>,
    mut hidden: Gizmos<CollabHiddenGizmos>,
    mut picked: Gizmos<CollabPickGizmos>,
    (mut gaze, iroh): (Gizmos<CollabGazeGizmos>, Option<Res<Iroh>>),
) {
    let me = iroh.map(|i| i.id().to_string());
    let tab = doc.as_deref().and_then(|d| d.active);
    if tab.is_none() || !three_d(*kind) || eda.0.is_some_and(|e| e.2 != Mode::ThreeD) {
        return;
    }
    let t = time.elapsed_secs_f64();
    let mine = &view.view;
    let px = |p: Vec3| mine.scale / mine.magnification(p);
    for (id, person) in &people.by_id {
        if person.presence.tab != tab {
            continue;
        }
        let color = people.shown_color(id);
        // What they have selected, their sketch's plane and what they are drawing on it.
        if let Some(a) = people.doing.get(id) {
            draw_picks(&mut picked, &cache, &a.selected, color, &px);
            if let Some(plane) = a.plane {
                draw_plane(&mut shown, plane, color.with_alpha(0.5));
                draw_plane(&mut hidden, plane, color.with_alpha(0.2));
            }
            if let Some(d) = &a.draft {
                let at = pointers
                    .by_id
                    .get(id)
                    .filter(|r| Some(r.tab) == tab)
                    .and_then(|r| match r.shown {
                        Some(PointerAt::Model(p)) => Some(Vec3::from_array(p)),
                        _ => None,
                    });
                draw_draft(&mut picked, d, a.plane, at, color, mine, &px);
                // Where a part hides it (a sketch on a face it is drawn under), faint over it.
                draw_draft(
                    &mut hidden,
                    d,
                    a.plane,
                    at,
                    color.with_alpha(0.45),
                    mine,
                    &px,
                );
            }
        }
        // Their camera, while they look at this tab in 3D (not whoever this user follows, nor
        // whoever follows this user: either way that camera is this view).
        if following.as_ref().is_some_and(|f| f.leader == *id)
            || (me.is_some() && person.presence.following == me)
        {
            continue;
        }
        let drawing = people.doing.get(id).is_some_and(|a| a.draft.is_some());
        let Some(theirs) = person
            .presence
            .view
            .as_ref()
            .map(super::presence::view_state)
        else {
            continue;
        };
        let faint = color.with_alpha(0.3);
        let eye = eye_of(&theirs, &cache);
        let pointer = pointers
            .by_id
            .get(id)
            .filter(|r| Some(r.tab) == tab && !r.idle(t))
            .and_then(|r| match r.shown {
                Some(PointerAt::Model(p)) => Some(Vec3::from_array(p)),
                _ => None,
            });
        let size = CAMERA_PX * px(eye);
        draw_camera(&mut shown, &theirs, eye, size, color);
        draw_camera(&mut hidden, &theirs, eye, size, faint);
        // Where they look from to where they point (not while they draw: their drawing goes
        // there), or to their view's centre while their mouse is off their view (its ring is a
        // badge, `sync_overlays`).
        match pointer {
            Some(p) if !drawing => gaze.line(eye, p, color.with_alpha(0.4)),
            Some(_) => {}
            None => gaze.line(eye, theirs.focus, color.with_alpha(0.3)),
        }
    }
}

/// A collaborator's selection, outlined in their colour: a face's boundary, an edge, a vertex,
/// a whole part's edges. (Features show on their rows.)
fn draw_picks(
    g: &mut Gizmos<CollabPickGizmos>,
    cache: &crate::parts::PartCache,
    picks: &[Picked],
    color: Color,
    px: &impl Fn(Vec3) -> f32,
) {
    let part = |id: PartId| cache.shown().find(|p| p.id == id);
    for pick in picks {
        match *pick {
            Picked::Face(id, name) => {
                let Some(f) = part(id).and_then(|p| p.solid.faces.iter().find(|f| f.name == name))
                else {
                    continue;
                };
                for l in &f.loops {
                    g.linestrip(l.iter().chain(l.first()).map(|p| m3(*p)), color);
                }
            }
            Picked::Edge(id, name) => {
                if let Some(e) =
                    part(id).and_then(|p| p.solid.edges.iter().find(|e| e.name == name))
                {
                    g.linestrip(e.points.iter().map(|p| m3(*p)), color);
                }
            }
            Picked::Vertex(id, name) => {
                if let Some(v) =
                    part(id).and_then(|p| p.solid.vertices.iter().find(|v| v.name == name))
                {
                    let at = m3(v.point);
                    g.sphere(Isometry3d::from_translation(at), 5.0 * px(at), color);
                }
            }
            Picked::Part(id) => {
                if let Some(p) = part(id) {
                    for e in &p.solid.edges {
                        g.linestrip(e.points.iter().map(|p| m3(*p)), color);
                    }
                }
            }
            Picked::Feature(_) | Picked::Item(_) => {}
        }
    }
}

/// A collaborator's selected faces, tinted in their colour (over the part's own faces).
#[derive(Component)]
struct PickTint;

/// Each collaborator's selected faces, for this tab, as last tinted (and the parts' generation).
type TintKey = (u64, Vec<(String, Vec<Picked>)>);

#[allow(clippy::too_many_arguments)]
fn sync_pick_tints(
    people: Res<People>,
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    cache: Res<crate::parts::PartCache>,
    q: Query<Entity, With<PickTint>>,
    mut last: Local<Option<TintKey>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let tab = doc.as_deref().and_then(|d| d.active);
    let want: Vec<(String, Vec<Picked>)> = if three_d(*kind) {
        people
            .by_id
            .iter()
            .filter(|(_, p)| p.presence.tab.is_some() && p.presence.tab == tab)
            .filter_map(|(id, _)| {
                let faces: Vec<Picked> = people
                    .doing
                    .get(id)?
                    .selected
                    .iter()
                    .filter(|p| matches!(p, Picked::Face(..)))
                    .copied()
                    .collect();
                (!faces.is_empty()).then(|| (id.clone(), faces))
            })
            .collect()
    } else {
        Vec::new()
    };
    let key = (cache.generation, want);
    if last.as_ref() == Some(&key) {
        return;
    }
    for e in &q {
        commands.entity(e).try_despawn();
    }
    for (id, picks) in &key.1 {
        let mut positions: Vec<[f32; 3]> = Vec::new();
        for pick in picks {
            let Picked::Face(pid, name) = *pick else {
                continue;
            };
            let Some((s, face)) = cache.shown().find(|p| p.id == pid).and_then(|p| {
                let f = p.solid.faces.iter().find(|f| f.name == name)?;
                Some((&p.solid, f))
            }) else {
                continue;
            };
            let idx = &s.indices
                [3 * face.first_triangle..3 * (face.first_triangle + face.triangle_count)];
            positions.extend(idx.iter().map(|i| m3(s.positions[*i as usize]).to_array()));
        }
        if positions.is_empty() {
            continue;
        }
        let indices: Vec<u32> = (0..positions.len() as u32).collect();
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices));
        let material = materials.add(StandardMaterial {
            base_color: color_of(id).with_alpha(0.22),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            double_sided: true,
            depth_bias: 1000.0,
            ..default()
        });
        commands.spawn((
            Name::new("collab-pick-tint"),
            PickTint,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            DespawnOnExit(crate::AppState::Document),
        ));
    }
    *last = Some(key);
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::from_array(p)
}

/// A model point of a part's mesh (f64) as drawn.
fn m3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// The plane of a collaborator's sketch: its drawn rectangle.
fn draw_plane<G: GizmoConfigGroup>(g: &mut Gizmos<G>, plane: SketchPlane, color: Color) {
    let (o, u, v) = (v3(plane.origin), v3(plane.u), v3(plane.v));
    let (hx, hy) = (plane.half[0], plane.half[1]);
    let c = [
        o + u * hx + v * hy,
        o - u * hx + v * hy,
        o - u * hx - v * hy,
        o + u * hx - v * hy,
    ];
    g.linestrip([c[0], c[1], c[2], c[3], c[0]], color);
}

/// What a collaborator is drawing: the points placed so far (each with a dot), the last piece
/// to their pointer.
fn draw_draft<G: GizmoConfigGroup>(
    g: &mut Gizmos<G>,
    draft: &Draft,
    plane: Option<SketchPlane>,
    pointer: Option<Vec3>,
    color: Color,
    view: &ViewState,
    px: &impl Fn(Vec3) -> f32,
) {
    let facing = Quat::from_rotation_arc(Vec3::Z, view.back());
    let mut dot = |p: Vec3| {
        let r = 4.0 * px(p);
        g.circle(Isometry3d::new(p, facing), r, color);
        g.circle(Isometry3d::new(p, facing), r * 0.45, color);
    };
    match draft {
        Draft::Chain(pts) => pts.iter().for_each(|p| dot(v3(*p))),
        Draft::Rect { first, .. } | Draft::Circle(first) => dot(v3(*first)),
        Draft::Design(_) => {}
    }
    match draft {
        Draft::Chain(pts) => {
            let all: Vec<Vec3> = pts.iter().map(|p| v3(*p)).chain(pointer).collect();
            if all.len() >= 2 {
                g.linestrip(all, color);
            }
        }
        Draft::Rect { first, centered } => {
            let (Some(p), Some(plane)) = (pointer, plane) else {
                return;
            };
            let (u, v, f) = (v3(plane.u), v3(plane.v), v3(*first));
            let d = p - f;
            let (a, b) = (u * d.dot(u), v * d.dot(v));
            let c = if *centered {
                [f + a + b, f - a + b, f - a - b, f + a - b]
            } else {
                [f, f + a, f + a + b, f + b]
            };
            g.linestrip([c[0], c[1], c[2], c[3], c[0]], color);
        }
        Draft::Circle(center) => {
            let Some(p) = pointer else {
                return;
            };
            let c = v3(*center);
            let n = plane
                .map(|pl| v3(pl.u).cross(v3(pl.v)))
                .filter(|n| n.length_squared() > 1e-9)
                .map_or(view.back(), Vec3::normalize);
            let d = p - c;
            let r = (d - n * d.dot(n)).length();
            if r > 1e-6 {
                g.circle(
                    Isometry3d::new(c, Quat::from_rotation_arc(Vec3::Z, n)),
                    r,
                    color,
                );
            }
        }
        // On a 2D view (`sync_overlays`).
        Draft::Design(_) => {}
    }
}

/// Where to draw someone's camera: their eye in perspective; in orthographic (no eye), on their
/// line of sight through the focus, just outside the shown parts, so it says where they look
/// from.
fn eye_of(v: &ViewState, cache: &crate::parts::PartCache) -> Vec3 {
    if v.perspective {
        return v.camera_position();
    }
    let reach = cache
        .shown()
        .filter_map(|p| p.solid.pick_index().bounds)
        .flat_map(|(lo, hi)| {
            let (lo, hi) = (
                Vec3::new(lo[0] as f32, lo[1] as f32, lo[2] as f32),
                Vec3::new(hi[0] as f32, hi[1] as f32, hi[2] as f32),
            );
            [lo, hi]
        })
        .map(|c| c.distance(v.focus))
        .fold(0.0_f32, f32::max);
    // About what they see across, without parts.
    let reach = if reach > 0.0 {
        reach * 1.3
    } else {
        v.scale * 300.0
    };
    v.focus + v.back() * reach
}

/// A camera: a short cone of sight from the eye (where its chip is) the way they look, with a
/// tick on its rim for their up.
fn draw_camera<G: GizmoConfigGroup>(
    g: &mut Gizmos<G>,
    v: &ViewState,
    eye: Vec3,
    size: f32,
    color: Color,
) {
    let (f, r, u) = (-v.back(), v.right(), v.up());
    let base = eye + f * size;
    let radius = size * 0.45;
    g.circle(
        Isometry3d::new(base, Quat::from_rotation_arc(Vec3::Z, f)),
        radius,
        color,
    );
    let spoke = color.with_alpha(color.alpha() * 0.6);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        g.line(eye, base + (r * a.cos() + u * a.sin()) * radius, spoke);
    }
    g.line(base + u * radius, base + u * radius * 1.45, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_slides_only_within_one_view() {
        let m = |x| Some(PointerAt::Model([x, 0.0, 0.0]));
        let d = |b, mode| {
            Some(PointerAt::Design {
                of: EdaOf::Board(b),
                mode,
                at: [0, 0],
            })
        };
        assert!(same_place(m(0.0), m(5.0)));
        assert!(same_place(d(1, 0), d(1, 0)));
        assert!(!same_place(d(1, 0), d(1, 1)));
        assert!(!same_place(d(1, 0), d(2, 0)));
        assert!(!same_place(m(0.0), d(1, 0)));
        assert!(!same_place(None, m(0.0)));
    }

    #[test]
    fn every_mode_has_its_own_code() {
        let modes = [
            Mode::Schematic,
            Mode::Layout,
            Mode::ThreeD,
            Mode::Symbol,
            Mode::Footprint,
        ];
        let codes: std::collections::HashSet<u8> = modes.iter().map(|m| mode_code(*m)).collect();
        assert_eq!(codes.len(), modes.len());
    }
}
