//! Social cues and Follow mode, as Onshape's (`collaboration.htm`, `follow_mode.htm`).
//!
//! - Everyone in a shared document's room tells the others where they are ([`Presence`]): the
//!   tab, the feature whose dialog or sketch they have open, their 3D view and whom they
//!   follow; a few times a second while it changes, and every few seconds regardless.
//! - Each collaborator has a colour of their own ([`color_of`]). Their **avatars** sit in the
//!   top bar beside this user's own; a click opens a card: where they are, what they are doing
//!   ([`Doing`]), **Follow** and **Go to**. A tab someone is in carries a chip with their
//!   initials in their colour (several for several people), and so does the feature someone is
//!   editing in the feature list.
//! - **Follow mode**: **Follow** follows that person: their tab and their view. The graphics
//!   area is outlined in their colour, with **Stop following** at its top; they see the
//!   follower's avatar outlined. A click, a scroll or a key stops it, as does their leaving.
//! - With voice (the `voice` feature, `super::voice`): **Join voice** beside the avatars, then
//!   mute and leave; an avatar is outlined green while that person speaks.

use std::collections::BTreeMap;

use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::{DocumentId, ElementId, FeatureId};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::bevy_iroh::iroh::EndpointId;
use cadrs_net::bevy_iroh::prelude::{NetSender, PeerLeft, Received};
use cadrs_net::{Doing, Draft, Picked, Presence, SketchPlane, View};
use cadrs_ui::menu::{MenuAction, MenuHeader, MenuItem, open_menu};
use cadrs_ui::prelude::*;
use cadrs_ui::{Avatar, IconButton};

use super::live::Live;
use crate::camera::{RenderMode, ViewState};
use crate::document::{FeatureRow, TabButton};
use crate::viewport::{ViewportArea, ViewportView};
use crate::{ActiveDocument, UserProfile};

/// How often presence goes out at most while it changes, and at least while it doesn't.
const SEND_EVERY: f64 = 0.1;
const HEARTBEAT: f64 = 4.0;
/// Someone not heard from this long is gone.
const FORGET_AFTER: f64 = 15.0;

/// Collaborator colours, as Onshape's social cues: hues far apart (blue, magenta, lime, rose,
/// violet and a warm grey), strong on the dark scheme and the light, and none the app's own
/// (its teal accent, the hover and selection orange), so a person never reads as a control or
/// a selection.
const PALETTE: [&str; 6] = [
    "#3b82f6", "#d946ef", "#84cc16", "#f43f5e", "#8b5cf6", "#a8a29e",
];
/// How often what this user is doing goes out at most while it changes, and at least while it
/// doesn't.
const DOING_EVERY: f64 = 0.25;
const DOING_HEARTBEAT: f64 = 3.0;

pub(super) fn plugin(app: &mut App) {
    app.add_message::<Received<Presence>>()
        .add_message::<Received<Doing>>()
        .add_message::<PeerLeft>()
        .init_resource::<People>()
        .init_resource::<VoiceUi>()
        .add_systems(
            Update,
            (hear, tell, tell_doing, follow, apply_go_to_view)
                .chain()
                .run_if(resource_exists::<Iroh>),
        )
        .add_systems(
            Update,
            (
                sync_people_bar,
                sync_markers,
                sync_follow_outline,
                colour_own_avatar,
            )
                .after(follow),
        )
        .add_observer(on_avatar_click)
        .add_observer(on_person_action)
        .add_observer(on_voice_button)
        .add_observer(stop_on_press);
}

/// A press anywhere (on any picked entity: the graphics area, a panel, a button) stops
/// following, once the double-click that started it is over.
fn stop_on_press(
    _press: On<PointerPress>,
    following: Option<Res<Following>>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    if following.is_some_and(|f| time.elapsed_secs_f64() - f.since > 0.3) {
        commands.remove_resource::<Following>();
    }
}

/// A collaborator's colour (a cadrs ID's).
pub fn color_of(id: &str) -> Color {
    let hash = id
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    Color::Srgba(Srgba::hex(PALETTE[hash as usize % PALETTE.len()]).unwrap_or(Srgba::BLUE))
}

/// Someone else in the shared document open now.
#[derive(Debug, Clone)]
pub struct Person {
    pub id: EndpointId,
    pub presence: Presence,
    /// When they were last heard (seconds on the real clock).
    heard: f64,
    /// When what they told last changed (where they are, their view, what they do).
    pub active: f64,
}

impl Person {
    pub fn key(&self) -> String {
        self.id.to_string()
    }
}

/// The others in the shared document open now, by cadrs ID.
#[derive(Resource, Default)]
pub struct People {
    document: Option<DocumentId>,
    pub by_id: BTreeMap<String, Person>,
    /// What each is doing ([`Doing`]), by cadrs ID.
    pub doing: BTreeMap<String, Activity>,
    /// When this document's room was first heard from: who is there by then was already
    /// there ("is here"), anyone after it joined.
    since: f64,
    /// Who has not been heard from for [`STALE_AFTER`]: shown grey, "reconnecting…", until
    /// they are heard again or forgotten.
    pub stale: std::collections::BTreeSet<String>,
}

/// Someone silent this long (seconds: their presence comes every 4, what they do every 3)
/// shows as reconnecting.
const STALE_AFTER: f64 = 6.0;

impl People {
    /// The colour someone shows in: theirs, or grey while their connection is stale.
    pub fn shown_color(&self, id: &str) -> Color {
        if self.stale.contains(id) {
            color_of(id).mix(&Color::srgb(0.5, 0.5, 0.52), 0.75)
        } else {
            color_of(id)
        }
    }
}

/// What someone is doing: in a few words, and what they have selected in their tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Activity {
    pub what: String,
    pub selected: Vec<Picked>,
    pub draft: Option<Draft>,
    pub plane: Option<SketchPlane>,
}

/// Someone who has done nothing for this long (seconds) shows as idle: their avatar and chips
/// faded, "idle 3 min" on their card.
pub const IDLE_AFTER: f64 = 120.0;

impl People {
    /// How long `id` has been idle, in whole minutes, once they are ([`IDLE_AFTER`]); their
    /// pointer's last move (`moved`) counts as activity.
    pub fn idle_minutes(&self, id: &str, now: f64, moved: Option<f64>) -> Option<u32> {
        let p = self.by_id.get(id)?;
        let since = moved.map_or(p.active, |m| m.max(p.active));
        let idle = now - since;
        (idle > IDLE_AFTER).then(|| (idle / 60.0) as u32)
    }
}

/// How long after opening a shared document the people heard were already there.
const ALREADY_THERE: f64 = 3.0;

/// Voice in the top bar (`super::voice` fills it when voice is built in): whether it is,
/// whether this user joined and is muted, who is speaking (cadrs IDs), and what the buttons
/// asked for.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct VoiceUi {
    pub available: bool,
    pub joined: bool,
    pub muted: bool,
    pub speaking: Vec<String>,
    /// Join (true) or leave (false), asked by a button.
    pub want_join: Option<bool>,
    /// Mute or unmute, asked by a button.
    pub toggle_mute: bool,
}

fn on_voice_button(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut ui: ResMut<VoiceUi>) {
    match q.get(a.entity).map(Name::as_str) {
        Ok("collab-voice-join") => ui.want_join = Some(true),
        Ok("collab-voice-leave") => ui.want_join = Some(false),
        Ok("collab-voice-mute") => ui.toggle_mute = true,
        _ => {}
    }
}

/// Following someone: their cadrs ID, and since when (the click that started it doesn't stop
/// it).
#[derive(Resource, Debug, Clone)]
pub struct Following {
    pub leader: String,
    since: f64,
}

fn now(world: &World) -> f64 {
    world.resource::<Time<Real>>().elapsed_secs_f64()
}

#[allow(clippy::too_many_arguments)]
fn hear(
    mut rx: MessageReader<Received<Presence>>,
    mut doing: MessageReader<Received<Doing>>,
    mut left: MessageReader<PeerLeft>,
    live: Option<Res<Live>>,
    doc: Option<Res<ActiveDocument>>,
    iroh: Option<Res<Iroh>>,
    mut people: ResMut<People>,
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let t = time.elapsed_secs_f64();
    let me = iroh.map(|i| i.id().to_string());
    let Some(live) = live else {
        if people.document.is_some() || !people.by_id.is_empty() {
            *people = People::default();
        }
        rx.clear();
        doing.clear();
        left.clear();
        return;
    };
    if people.document != Some(live.document) {
        *people = People {
            document: Some(live.document),
            since: t,
            ..default()
        };
    }
    for m in doing.read() {
        if m.msg.document != live.document {
            continue;
        }
        let key = m.from.to_string();
        let a = Activity {
            what: m.msg.what.clone(),
            selected: m.msg.selected.clone(),
            draft: m.msg.draft.clone(),
            plane: m.msg.plane,
        };
        // Heard from (what they do comes every few seconds too).
        if let Some(p) = people.by_id.get_mut(&key) {
            p.heard = t;
        }
        if people.doing.get(&key) != Some(&a) {
            if let Some(p) = people.by_id.get_mut(&key) {
                p.active = t;
            }
            people.doing.insert(key, a);
        }
    }
    let mut toasts = Vec::new();
    // Who joined (not who was already here): their toast offers Follow and Go to.
    let mut joined: Vec<(String, String, String)> = Vec::new();
    for m in rx.read() {
        if m.msg.document != live.document {
            continue;
        }
        if !people.by_id.contains_key(&m.from.to_string()) {
            let how = if t - people.since < ALREADY_THERE {
                "is here"
            } else {
                "joined"
            };
            // Where they are, as the avatar says it.
            let at = m
                .msg
                .tab
                .and_then(|t| doc.as_ref()?.doc.element(t).map(|e| e.name.clone()));
            let text = match at {
                Some(at) => format!("{} {how}, in {at}", m.msg.name),
                None => format!("{} {how}", m.msg.name),
            };
            joined.push((m.from.to_string(), m.msg.name.clone(), text));
        }
        let before = people.by_id.get(&m.from.to_string());
        // Someone starting (or stopping) to follow this user says so.
        let follows = |p: Option<&Presence>| {
            me.is_some() && p.and_then(|p| p.following.as_ref()) == me.as_ref()
        };
        let (was, is) = (follows(before.map(|b| &b.presence)), follows(Some(&m.msg)));
        if is && !was {
            toasts.push((m.from.to_string(), m.msg.name.clone(), "is following you"));
        } else if was && !is {
            toasts.push((
                m.from.to_string(),
                m.msg.name.clone(),
                "stopped following you",
            ));
        }
        let active = match before {
            Some(b) if b.presence == m.msg => b.active,
            _ => t,
        };
        people.by_id.insert(
            m.from.to_string(),
            Person {
                id: m.from,
                presence: m.msg.clone(),
                heard: t,
                active,
            },
        );
    }
    for l in left.read() {
        if l.room == live.room()
            && let Some(p) = people.by_id.remove(&l.id.to_string())
        {
            people.doing.remove(&l.id.to_string());
            toasts.push((p.key(), p.presence.name, "left"));
        }
    }
    let quiet: std::collections::BTreeSet<String> = people
        .by_id
        .iter()
        .filter(|(_, p)| t - p.heard > STALE_AFTER)
        .map(|(k, _)| k.clone())
        .collect();
    if people.stale != quiet {
        people.stale = quiet;
    }
    let stale: Vec<String> = people
        .by_id
        .iter()
        .filter(|(_, p)| t - p.heard > FORGET_AFTER)
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale {
        people.doing.remove(&k);
        if let Some(p) = people.by_id.remove(&k) {
            toasts.push((k, p.presence.name, "left"));
        }
    }
    for (id, name, what) in toasts {
        cadrs_ui::show_notification(
            &mut commands,
            &theme,
            cadrs_ui::Notification::info(format!("{name} {what}"))
                .name("collab-person-toast")
                .avatar(name, color_of(&id)),
        );
    }
    for (id, name, text) in joined {
        let toast = cadrs_ui::show_notification(
            &mut commands,
            &theme,
            cadrs_ui::Notification::info(text)
                .name("collab-joined-toast")
                .avatar(name, color_of(&id))
                .seconds(8.0),
        );
        let short: String = id.chars().take(8).collect();
        for (action, label) in [("goto", "Go to"), ("follow", "Follow")] {
            let link = cadrs_ui::toast_action(
                &mut commands,
                &theme,
                toast,
                format!("collab-joined-{action}-{short}"),
                label,
            );
            let id = id.clone();
            commands.entity(link).observe(
                move |_: On<bevy::ui_widgets::Activate>, mut commands: Commands| {
                    let id = id.clone();
                    commands.queue(move |w: &mut World| {
                        cadrs_ui::close_toasts(w);
                        if action == "follow" {
                            toggle_follow(w, id);
                        } else {
                            go_to(w, &id);
                        }
                    });
                },
            );
        }
    }
}

/// The feature whose dialog or sketch is open.
fn editing_feature(world: &World) -> Option<(ElementId, FeatureId)> {
    if let Some(s) = world.get_resource::<crate::sketch::SketchSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::extrude::ExtrudeSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::applied::AppliedSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::boolean::BooleanSession>() {
        return Some((s.element, s.feature));
    }
    None
}

fn view_of(v: &ViewState) -> View {
    View {
        azimuth: v.azimuth,
        elevation: v.elevation,
        roll: v.roll,
        focus: v.focus.to_array(),
        scale: v.scale,
        perspective: v.perspective,
        render: RenderMode::ALL
            .iter()
            .position(|r| *r == v.render)
            .unwrap_or(0) as u8,
    }
}

pub(super) fn view_state(v: &View) -> ViewState {
    ViewState {
        azimuth: v.azimuth,
        elevation: v.elevation,
        roll: v.roll,
        focus: Vec3::from_array(v.focus),
        scale: v.scale,
        perspective: v.perspective,
        render: RenderMode::ALL
            .get(v.render as usize)
            .copied()
            .unwrap_or_default(),
    }
}

/// This user's presence, to the room: when it changes (at most every [`SEND_EVERY`]) and
/// every [`HEARTBEAT`].
fn tell(world: &mut World, mut last: Local<Option<(Presence, f64)>>) {
    let Some((document, room)) = world.get_resource::<Live>().map(|l| (l.document, l.room()))
    else {
        *last = None;
        return;
    };
    let Some(d) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let tab = d.active;
    let view = world
        .get_resource::<ViewportView>()
        .filter(|v| v.element == tab)
        .map(|v| view_of(&v.view));
    let p = Presence {
        document,
        name: world.resource::<UserProfile>().display_name.clone(),
        tab,
        editing: editing_feature(world),
        view,
        following: world.get_resource::<Following>().map(|f| f.leader.clone()),
    };
    let t = now(world);
    let due = match &*last {
        None => true,
        Some((prev, at)) => (*prev != p && t - at >= SEND_EVERY) || t - at >= HEARTBEAT,
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

/// What this user is doing, in a few words, for the others' cues: the sketch tool, the
/// schematic's or layout's tool or move, the drawing's view tool, else how much is selected;
/// empty while nothing in particular.
fn doing_now(world: &World) -> String {
    use crate::eda::Mode;
    use crate::eda::layout_tools::{DrawKind, LayoutState, Tool as LayTool};
    use crate::eda::schematic_tools::{SchState, Tool as SchTool};
    let selected = |n: usize| match n {
        0 => String::new(),
        n => format!("{n} selected"),
    };
    if world.contains_resource::<crate::sketch::SketchSession>()
        && let Some(t) = world.get_resource::<crate::sketch::ActiveSketchTool>()
        && t.tool != crate::sketch::SketchTool::Select
    {
        return t.tool.label().to_string();
    }
    let mode = world
        .get_resource::<crate::eda::Eda2d>()
        .and_then(|e| e.0)
        .map(|(_, _, m)| m);
    match mode {
        Some(Mode::Schematic) => {
            let Some(s) = world.get_resource::<SchState>() else {
                return String::new();
            };
            if s.moving.is_some() {
                return "Moving".into();
            }
            return match &s.tool {
                SchTool::Select => selected(s.selection.len()),
                SchTool::Place(sym) => {
                    let name = sym.id.rsplit(':').next().unwrap_or(&sym.id);
                    format!("Placing {name}")
                }
                SchTool::Wire(_) => "Wiring".into(),
                SchTool::Label(_) | SchTool::GlobalLabel(_) => "Placing a label".into(),
                SchTool::NoConnect => "Marking no-connects".into(),
                SchTool::Paste(_) => "Pasting".into(),
                SchTool::Text(_) => "Placing text".into(),
            };
        }
        Some(Mode::Layout) => {
            let Some(s) = world.get_resource::<LayoutState>() else {
                return String::new();
            };
            return match &s.tool {
                LayTool::Select => selected(s.selection.len()),
                LayTool::Outline(_) => "Drawing the outline".into(),
                LayTool::Route { net, .. } if net.is_empty() => "Routing".into(),
                LayTool::Route { net, .. } => format!("Routing {net}"),
                LayTool::Zone { net, .. } => format!("Drawing a {net} zone"),
                LayTool::Draw { kind, .. } => match kind {
                    DrawKind::Line => "Drawing a line".into(),
                    DrawKind::Rect => "Drawing a rectangle".into(),
                    DrawKind::Circle => "Drawing a circle".into(),
                },
                LayTool::Polygon(_) => "Drawing a polygon".into(),
                LayTool::Text(_) => "Placing text".into(),
                LayTool::Graphic(_) => "Placing artwork".into(),
                LayTool::Keepout { .. } => "Drawing a keep-out".into(),
                LayTool::Measure { .. } => "Measuring".into(),
            };
        }
        _ => {}
    }
    if let Some(ui) = world.get_resource::<crate::drawing::DrawingUi>()
        && *world.resource::<crate::viewport::ActiveKind>() == crate::viewport::ActiveKind::Drawing
    {
        use crate::drawing::view_tools::ViewTool;
        let what = match &ui.tool {
            ViewTool::None => None,
            ViewTool::Insert => Some("Inserting a view"),
            ViewTool::Projected { .. } => Some("Projecting a view"),
            ViewTool::Auxiliary { .. } => Some("Adding an auxiliary view"),
            ViewTool::Section { .. } => Some("Adding a section view"),
            _ => Some("Editing views"),
        };
        if let Some(w) = what {
            return w.into();
        }
    }
    world
        .get_resource::<crate::viewport::Selection>()
        .map_or_else(String::new, |s| selection_words(world, &s.0))
}

/// What is selected, in a few words: one thing by name ("Face of Extrude 1", "Part 1"), else
/// how many of each ("2 faces, 1 edge").
fn selection_words(world: &World, picks: &[crate::viewport::Pick]) -> String {
    use crate::viewport::Pick;
    if let [one] = picks {
        let doc = world.get_resource::<ActiveDocument>();
        let features = doc
            .and_then(|d| d.active.and_then(|t| d.doc.element(t)))
            .map(|e| e.features())
            .unwrap_or_default();
        let named = match one {
            Pick::Feature(f) => features.iter().find(|x| x.id == *f).map(|x| x.name.clone()),
            p => {
                crate::parts::pick_label(features, world.resource::<crate::parts::PartCache>(), *p)
            }
        };
        if let Some(n) = named {
            return n;
        }
    }
    let mut counts: Vec<(&str, &str, usize)> = Vec::new();
    for p in picks {
        let (one, many) = match p {
            Pick::Face(..) => ("face", "faces"),
            Pick::Edge(..) => ("edge", "edges"),
            Pick::Vertex(..) | Pick::SketchPoint(..) => ("vertex", "vertices"),
            Pick::Part(..) => ("part", "parts"),
            Pick::Feature(..) => ("feature", "features"),
            Pick::Plane(..) => ("plane", "planes"),
            _ => ("item", "items"),
        };
        match counts.iter_mut().find(|c| c.0 == one) {
            Some(c) => c.2 += 1,
            None => counts.push((one, many, 1)),
        }
    }
    counts
        .iter()
        .map(|(one, many, n)| {
            if *n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The plane of the sketch this user has open, as it travels.
fn plane_now(world: &World) -> Option<SketchPlane> {
    let s = world.get_resource::<crate::sketch::SketchSession>()?;
    let map = world
        .get_resource::<crate::sketch_tools::SketchScreen>()?
        .active?;
    let f = map.plane.frame();
    let v = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
    Some(SketchPlane {
        origin: v(f.origin),
        u: v(f.u),
        v: v(f.v),
        half: s.plane_extent.to_array(),
    })
}

/// The shape this user is drawing, not made yet: a sketch tool's points so far (model mm), or
/// the schematic's wire or the layout's route, zone or outline so far (design nm).
fn draft_now(world: &World) -> Option<Draft> {
    use crate::eda::Mode;
    use crate::sketch_tools::{DrawState as S, SVec2, SketchDraw, SketchScreen};
    if world.contains_resource::<crate::sketch::SketchSession>() {
        let f = world.get_resource::<SketchScreen>()?.active?.plane.frame();
        let to = |p: SVec2| -> [f32; 3] {
            std::array::from_fn(|i| (f.origin[i] + f.u[i] * p.x + f.v[i] * p.y) as f32)
        };
        let chain = |pts: &[Option<SVec2>]| {
            Some(Draft::Chain(pts.iter().flatten().map(|p| to(*p)).collect()))
        };
        let d = world.get_resource::<SketchDraw>()?;
        return match d.state {
            S::Idle | S::BoxSelect { .. } => None,
            S::Line { start, .. } | S::TangentArc { start, .. } => chain(&[Some(start)]),
            S::MidLine { mid } => chain(&[Some(mid)]),
            S::Rect { first, centered } => Some(Draft::Rect {
                first: to(first),
                centered,
            }),
            S::TextBox { first } => Some(Draft::Rect {
                first: to(first),
                centered: false,
            }),
            S::Circle { center }
            | S::Ellipse { center, .. }
            | S::Polygon { center, .. }
            | S::CenterArc { center, .. } => Some(Draft::Circle(to(center))),
            S::Arc { start, end } => chain(&[Some(start), end]),
            S::AlignedRect { p0, p1 } => chain(&[Some(p0), p1]),
            S::Circle3 { p1, p2 } => chain(&[Some(p1), p2]),
            S::Bezier { pts, n } => chain(
                &pts[..n.min(3) as usize]
                    .iter()
                    .map(|p| Some(*p))
                    .collect::<Vec<_>>(),
            ),
            S::Spline { .. } => Some(Draft::Chain(
                d.pending_points().into_iter().map(to).collect(),
            )),
        };
    }
    let mode = world
        .get_resource::<crate::eda::Eda2d>()
        .and_then(|e| e.0)
        .map(|(_, _, m)| m);
    let design = |pts: &[cadrs_eda::units::Pt]| {
        (!pts.is_empty()).then(|| Draft::Design(pts.iter().map(|p| [p.x, p.y]).collect()))
    };
    match mode {
        Some(Mode::Schematic) => {
            use crate::eda::schematic_tools::{SchState, Tool};
            match &world.get_resource::<SchState>()?.tool {
                Tool::Wire(pts) => design(pts),
                _ => None,
            }
        }
        Some(Mode::Layout) => {
            use crate::eda::layout_tools::{LayoutState, Tool};
            match &world.get_resource::<LayoutState>()?.tool {
                Tool::Route { runs, .. } => {
                    let pts: Vec<_> = runs.iter().flat_map(|(_, p)| p.iter().copied()).collect();
                    design(&pts)
                }
                Tool::Zone { pts, .. } | Tool::Polygon(pts) | Tool::Keepout { pts, .. } => {
                    design(pts)
                }
                Tool::Outline(Some(p)) | Tool::Draw { start: Some(p), .. } => design(&[*p]),
                Tool::Measure { a: Some(a), b } => {
                    design(&[Some(*a), *b].into_iter().flatten().collect::<Vec<_>>())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The most picks a selection travels with (a box selection can hold thousands).
const MAX_PICKED: usize = 64;

/// What this user has selected in their tab, as it travels: the 3D selection (features, parts,
/// faces, edges, vertices), or the schematic's or layout's items.
fn selected_now(world: &World) -> Vec<Picked> {
    use crate::eda::Mode;
    use crate::viewport::Pick;
    let mode = world
        .get_resource::<crate::eda::Eda2d>()
        .and_then(|e| e.0)
        .map(|(_, _, m)| m);
    let mut out: Vec<Picked> = match mode {
        Some(Mode::Schematic) => world
            .get_resource::<crate::eda::schematic_tools::SchState>()
            .map(|s| {
                use cadrs_eda::sch_edit::SchItem as I;
                s.selection
                    .iter()
                    .map(|i| match i {
                        I::Symbol(u)
                        | I::Field(u, _)
                        | I::Wire(u)
                        | I::Bus(u)
                        | I::Junction(u)
                        | I::NoConnect(u)
                        | I::Label(u)
                        | I::Note(u)
                        | I::Drawing(u) => Picked::Item(u.as_u128()),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Some(Mode::Layout) => world
            .get_resource::<crate::eda::layout_tools::LayoutState>()
            .map(|s| {
                s.selection
                    .iter()
                    .map(|i| Picked::Item(i.id().as_u128()))
                    .collect()
            })
            .unwrap_or_default(),
        _ => world
            .get_resource::<crate::viewport::Selection>()
            .map(|s| {
                s.0.iter()
                    .filter_map(|p| match *p {
                        Pick::Feature(f) => Some(Picked::Feature(f)),
                        Pick::Part(p) => Some(Picked::Part(p)),
                        Pick::Face(p, n) => Some(Picked::Face(p, n)),
                        Pick::Edge(p, n) => Some(Picked::Edge(p, n)),
                        Pick::Vertex(p, n) => Some(Picked::Vertex(p, n)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    out.truncate(MAX_PICKED);
    out
}

/// What this user is doing ([`doing_now`], [`selected_now`]), to the room: when it changes (at
/// most every [`DOING_EVERY`]) and every [`DOING_HEARTBEAT`].
fn tell_doing(world: &mut World, mut last: Local<Option<(Activity, f64)>>) {
    let Some((document, room)) = world.get_resource::<Live>().map(|l| (l.document, l.room()))
    else {
        *last = None;
        return;
    };
    let what = Activity {
        what: doing_now(world),
        selected: selected_now(world),
        draft: draft_now(world),
        plane: plane_now(world),
    };
    let t = now(world);
    let due = match &*last {
        None => true,
        Some((prev, at)) => (*prev != what && t - at >= DOING_EVERY) || t - at >= DOING_HEARTBEAT,
    };
    if !due {
        return;
    }
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(
            room,
            &Doing {
                document,
                what: what.what.clone(),
                selected: what.selected.clone(),
                draft: what.draft.clone(),
                plane: what.plane,
            },
        );
    }
    *last = Some((what, t));
}

// ---------------------------------------------------------------------------------------------
// Follow mode

/// Starts following `id`, or stops if already following them.
pub fn toggle_follow(world: &mut World, id: String) {
    let same = world
        .get_resource::<Following>()
        .is_some_and(|f| f.leader == id);
    if same {
        world.remove_resource::<Following>();
        return;
    }
    let since = now(world);
    world.insert_resource(Following { leader: id, since });
}

/// Takes this user to where `id` is, once (not following): their tab and their view.
pub fn go_to(world: &mut World, id: &str) {
    let Some(p) = world
        .resource::<People>()
        .by_id
        .get(id)
        .map(|p| p.presence.clone())
    else {
        return;
    };
    if let Some(tab) = p.tab {
        let mut d = world.resource_mut::<ActiveDocument>();
        if d.active != Some(tab) && d.doc.element(tab).is_some() {
            d.set_active(tab);
        }
    }
    if let Some(v) = &p.view {
        let to = view_state(v);
        let mut vv = world.resource_mut::<ViewportView>();
        if vv.element == p.tab {
            vv.view = to;
            vv.animation = None;
        } else {
            // The tab's view is made when it opens: start it at theirs.
            world.insert_resource(GoToView(p.tab, to));
        }
    }
}

/// A view [`go_to`] gives the tab it opened, once that tab's view is there.
#[derive(Resource)]
struct GoToView(Option<ElementId>, ViewState);

fn apply_go_to_view(world: &mut World) {
    let Some(GoToView(tab, to)) = world.remove_resource::<GoToView>() else {
        return;
    };
    let mut vv = world.resource_mut::<ViewportView>();
    if vv.element == tab {
        vv.view = to;
        vv.animation = None;
    } else if world.resource::<ActiveDocument>().active == tab {
        // Not there yet: next frame.
        world.insert_resource(GoToView(tab, to));
    }
}

/// Follows whoever's cadrs ID starts with `prefix` (the `follow` script command).
pub fn follow_prefix(world: &mut World, prefix: &str) {
    let id = world
        .resource::<People>()
        .by_id
        .keys()
        .find(|k| k.starts_with(prefix))
        .cloned();
    match id {
        Some(id) => toggle_follow(world, id),
        None => warn!("follow: nobody here whose cadrs ID starts with {prefix}"),
    }
}

/// Keeps the leader's tab and view; any click, scroll or key of the follower's own stops it.
fn follow(world: &mut World) {
    let Some(f) = world.get_resource::<Following>().cloned() else {
        return;
    };
    let t = now(world);
    let leader = world
        .resource::<People>()
        .by_id
        .get(&f.leader)
        .map(|p| p.presence.clone());
    let Some(leader) = leader else {
        world.remove_resource::<Following>();
        crate::linked::toast(world, "Stopped following: they left");
        return;
    };
    if t - f.since > 0.3 {
        let pressed = world
            .resource::<ButtonInput<MouseButton>>()
            .get_just_pressed()
            .next()
            .is_some();
        let scrolled = !world.resource::<Messages<MouseWheel>>().is_empty();
        let typed = !world.resource::<Messages<KeyboardInput>>().is_empty();
        if pressed || scrolled || typed {
            world.remove_resource::<Following>();
            return;
        }
    }
    if let Some(tab) = leader.tab {
        let mut d = world.resource_mut::<ActiveDocument>();
        if d.active != Some(tab)
            && let Some(name) = d.doc.element(tab).map(|e| e.name.clone())
        {
            d.set_active(tab);
            // Not the switch that follows the start.
            if t - f.since > 0.5 {
                crate::linked::toast(world, format!("Following {} to {name}", leader.name));
            }
        }
    }
    if let Some(v) = &leader.view {
        let mut vv = world.resource_mut::<ViewportView>();
        if vv.element == leader.tab {
            let to = view_state(v);
            if vv.view != to {
                vv.view = to;
                vv.animation = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The cues

/// The avatars' place in the top bar (`crate::document`).
#[derive(Component)]
pub struct PeopleBar;

/// An avatar: whose.
#[derive(Component)]
struct PersonAvatar(String);

/// What the avatars show, to rebuild them only when it changes: each person (id, name, where
/// they are, (follows me, I follow them, speaking), idle minutes), the status, and voice (built
/// in, joined, muted).
#[derive(PartialEq, Clone, Default)]
struct BarState(Vec<BarPerson>, Option<String>, (bool, bool, bool));

/// One avatar: id, name, tooltip, (follows me, I follow them, speaking, stale), idle minutes.
type BarPerson = (
    String,
    String,
    String,
    (bool, bool, bool, bool),
    Option<u32>,
);

/// "In Part Studio 1 · Line", with "idle 3 min" after it once they are idle.
pub(crate) fn with_idle(text: String, idle: Option<u32>) -> String {
    let Some(m) = idle else {
        return text;
    };
    let idle = if m < 1 {
        "idle".to_string()
    } else {
        format!("idle {m} min")
    };
    if text.is_empty() {
        idle
    } else {
        format!("{text} · {idle}")
    }
}

/// Where someone is and what they do, for their card and tooltip: "In Part Studio 1, editing
/// Extrude 1 · Line".
pub(crate) fn whereabouts(doc: &ActiveDocument, p: &Presence, doing: Option<&str>) -> String {
    let mut text = String::new();
    if let Some(e) = p.tab.and_then(|t| doc.doc.element(t)) {
        text.push_str(&format!("In {}", e.name));
    }
    if let Some(f) = editing_name(doc, p) {
        text.push_str(&format!(", editing {f}"));
    }
    if let Some(d) = doing.filter(|d| !d.is_empty()) {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str(d);
    }
    text
}

/// The name of the feature someone has open (its dialog or sketch).
pub(super) fn editing_name(doc: &ActiveDocument, p: &Presence) -> Option<String> {
    let (e, f) = p.editing?;
    let f = doc.doc.element(e)?.features().iter().find(|x| x.id == f)?;
    Some(f.name.clone())
}

/// This user's own avatar in the top bar, while online: in the colour the others see them in
/// (their cadrs ID's), not the one picked from their name.
fn colour_own_avatar(
    iroh: Option<Res<Iroh>>,
    mut avatars: Query<(&Name, &mut BackgroundColor)>,
    added: Query<(), Added<Name>>,
    mut last: Local<Option<String>>,
    profile: Res<crate::UserProfile>,
) {
    let me = iroh.map(|i| i.id().to_string());
    if *last == me && added.is_empty() {
        return;
    }
    let want = me.as_deref().map_or_else(
        || cadrs_ui::Avatar::color_for(&profile.display_name),
        color_of,
    );
    for (name, mut bg) in &mut avatars {
        if name.as_str() == "identity-avatar" && bg.0 != want {
            bg.0 = want;
        }
    }
    *last = me;
}

#[allow(clippy::too_many_arguments)]
fn sync_people_bar(
    people: Res<People>,
    pointers: Res<super::pointers::Pointers>,
    time: Res<Time<Real>>,
    following: Option<Res<Following>>,
    voice: Res<VoiceUi>,
    doc: Option<Res<ActiveDocument>>,
    live: Option<Res<Live>>,
    iroh: Option<Res<Iroh>>,
    q_bar: Query<Entity, With<PeopleBar>>,
    theme: Res<Theme>,
    mut last: Local<BarState>,
    mut commands: Commands,
) {
    let Ok(bar) = q_bar.single() else {
        *last = BarState::default();
        return;
    };
    let Some(doc) = doc else { return };
    let me = iroh.map(|i| i.id().to_string()).unwrap_or_default();
    let leader = following.as_ref().map(|f| f.leader.clone());
    let rows: Vec<_> = people
        .by_id
        .values()
        .map(|p| {
            let key = p.key();
            let follows_me = p.presence.following.as_deref() == Some(me.as_str());
            let i_follow = leader.as_deref() == Some(key.as_str());
            let speaking = voice.speaking.contains(&key);
            let doing = people.doing.get(&key).map(|a| a.what.as_str());
            let idle = people.idle_minutes(&key, time.elapsed_secs_f64(), pointers.moved_at(&key));
            let mut tip = p.presence.name.clone();
            let place = with_idle(whereabouts(&doc, &p.presence, doing), idle);
            if !place.is_empty() {
                tip.push_str(&format!(" · {place}"));
            }
            if follows_me {
                tip.push_str(" · following you");
            }
            let stale = people.stale.contains(&key);
            if stale {
                tip = format!("{} · reconnecting…", p.presence.name);
            }
            (
                key,
                p.presence.name.clone(),
                tip,
                (follows_me, i_follow, speaking, stale),
                idle,
            )
        })
        .collect();
    let status = live.as_ref().and_then(|l| l.status());
    let voice_state = (voice.available && live.is_some(), voice.joined, voice.muted);
    let state = BarState(rows, status, voice_state);
    if *last == state {
        return;
    }
    *last = state.clone();
    commands.entity(bar).despawn_children();
    let t = &*theme;
    commands.entity(bar).with_children(|b| {
        if let Some(status) = &state.1 {
            b.spawn((
                Name::new("collab-status"),
                t.text(status, t.font_sm, FontWeight::MEDIUM, t.muted_foreground),
                Node {
                    margin: UiRect::right(Val::Px(6.0)),
                    ..default()
                },
            ));
        }
        // Voice: join; once joined, mute and leave.
        let (available, joined, muted) = state.2;
        if available && !joined {
            b.spawn(
                IconButton::new("collab-voice-join", "volume-mute")
                    .small()
                    .tooltip("Join voice: talk with everyone in this document")
                    .build(t),
            );
        } else if available {
            b.spawn(
                IconButton::new(
                    "collab-voice-mute",
                    if muted { "volume-mute" } else { "volume" },
                )
                .small()
                .tooltip(if muted { "Unmute" } else { "Mute" })
                .build(t),
            );
            b.spawn(
                IconButton::new("collab-voice-leave", "close")
                    .small()
                    .tooltip("Leave voice")
                    .build(t),
            )
            .entry::<Node>()
            .and_modify(|mut n| n.margin = UiRect::right(Val::Px(4.0)));
        }
        for (key, name, tip, (follows_me, i_follow, speaking, stale), idle) in &state.0 {
            // Faded while they are idle, grey while their connection is stale.
            let color = if *stale {
                people.shown_color(key)
            } else if idle.is_some() {
                color_of(key).mix(&t.control, 0.6)
            } else {
                color_of(key)
            };
            let outlined = *follows_me || *i_follow;
            let tip = if *i_follow {
                format!("{tip}. Following: click for more")
            } else {
                format!("{tip}. Click to follow or go to them")
            };
            b.spawn((
                Name::new(format!("collab-avatar-{}", &key[..8.min(key.len())])),
                PersonAvatar(key.clone()),
                bevy::picking::hover::Hovered::default(),
                Node {
                    padding: UiRect::all(Val::Px(1.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius + 2.0)),
                    ..default()
                },
                BorderColor::all(if *speaking {
                    t.success
                } else if outlined {
                    color
                } else {
                    Color::NONE
                }),
                Tooltip::new(tip),
            ))
            .with_children(|a| {
                a.spawn((
                    Avatar::new(format!("collab-avatar-face-{key}"), name.clone())
                        .size(24.0)
                        .color(color)
                        .build(t),
                    cadrs_ui::color_scheme::KeepFill,
                    Pickable::IGNORE,
                ));
                // Following this user: an eye on their avatar.
                if *follows_me {
                    a.spawn((
                        Name::new(format!(
                            "collab-avatar-follows-you-{}",
                            &key[..8.min(key.len())]
                        )),
                        Node {
                            position_type: PositionType::Absolute,
                            right: Val::Px(-6.0),
                            bottom: Val::Px(-6.0),
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            border: UiRect::all(Val::Px(1.5)),
                            border_radius: BorderRadius::MAX,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(color_of(key)),
                        BorderColor::all(Color::WHITE),
                        cadrs_ui::color_scheme::KeepFill,
                        Pickable::IGNORE,
                        children![(
                            cadrs_ui::icon("visible", 10.0, Avatar::ink_on(color_of(key))),
                            Pickable::IGNORE
                        )],
                    ));
                }
            });
        }
    });
}

/// A click on an avatar opens its card: who, where they are and what they are doing, then
/// **Follow** (or **Stop following**) and **Go to**.
fn on_avatar_click(ev: On<PointerClick>, q: Query<&PersonAvatar>, mut commands: Commands) {
    let Ok(a) = q.get(ev.entity) else {
        return;
    };
    let (anchor, id) = (ev.entity, a.0.clone());
    commands.queue(move |world: &mut World| {
        let Some(p) = world.resource::<People>().by_id.get(&id).cloned() else {
            return;
        };
        let doing = world
            .resource::<People>()
            .doing
            .get(&id)
            .map(|a| a.what.clone());
        let idle = world.resource::<People>().idle_minutes(
            &id,
            now(world),
            world.resource::<super::pointers::Pointers>().moved_at(&id),
        );
        let place = if world.resource::<People>().stale.contains(&id) {
            "Reconnecting…".to_string()
        } else {
            world
                .get_resource::<ActiveDocument>()
                .map(|d| with_idle(whereabouts(d, &p.presence, doing.as_deref()), idle))
                .unwrap_or_default()
        };
        let following = world
            .get_resource::<Following>()
            .is_some_and(|f| f.leader == id);
        let name = p.presence.name.clone();
        let mut header = MenuHeader::new(name.clone())
            .avatar_colored(world.resource::<People>().shown_color(&id));
        if !place.is_empty() {
            header = header.subtitle(place);
        }
        let follow = if following {
            MenuItem::new("collab-person-follow", "Stop following").icon("close")
        } else {
            MenuItem::new("collab-person-follow", format!("Follow {name}"))
                .icon("visible")
                .tooltip("See their tab and their view as they move")
        };
        // Beside the avatar, not under it: the view cube sits below the top bar's right end.
        let menu = cadrs_ui::Menu::new("collab-person-menu")
            .side(bevy::ui_widgets::popover::PopoverSide::Left)
            .min_width(240.0)
            .header(header)
            .separator()
            .item(follow)
            .item(
                MenuItem::new("collab-person-goto", format!("Go to {name}'s view"))
                    .icon("location")
                    .tooltip("Open their tab and view, once"),
            );
        let theme = world.resource::<Theme>().clone();
        open_menu(&mut world.commands(), anchor, menu.build(&theme));
        world.flush();
    });
}

fn on_person_action(ev: On<MenuAction>, q: Query<&PersonAvatar>, mut commands: Commands) {
    let Ok(a) = q.get(ev.entity) else {
        return;
    };
    let id = a.0.clone();
    match ev.item.as_str() {
        "collab-person-follow" => commands.queue(move |w: &mut World| toggle_follow(w, id)),
        "collab-person-goto" => commands.queue(move |w: &mut World| go_to(w, &id)),
        _ => {}
    }
}

/// Whom a chip shows: cadrs ID, name, and whether their connection is stale (grey).
type Who = (String, String, bool);

/// The chips on tabs and feature rows: whose they show.
#[derive(Component, PartialEq)]
struct Marker(Vec<Who>);

/// A chip with each person's initials in their colour: after a tab's name (in its row), or at
/// a feature row's right end.
fn marker(people: Vec<Who>, on_tab: bool, t: &Theme) -> impl Bundle {
    const CHIP: f32 = 16.0;
    let node = if on_tab {
        Node {
            margin: UiRect::left(Val::Px(2.0)),
            column_gap: Val::Px(2.0),
            flex_shrink: 0.0,
            ..default()
        }
    } else {
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(6.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            ..default()
        }
    };
    let t = t.clone();
    (
        Name::new("collab-marker"),
        Marker(people.clone()),
        node,
        Pickable::IGNORE,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            for (id, name, stale) in people {
                let color = if stale {
                    color_of(&id).mix(&Color::srgb(0.5, 0.5, 0.52), 0.75)
                } else {
                    color_of(&id)
                };
                p.spawn((
                    Avatar::new(format!("collab-marker-{}", &id[..8.min(id.len())]), name)
                        .size(CHIP)
                        .color(color)
                        .build(&t),
                    cadrs_ui::color_scheme::KeepFill,
                    Pickable::IGNORE,
                ));
            }
        })),
    )
}

/// Puts each collaborator's chip on the tab they are in and the feature they edit.
fn sync_markers(
    people: Res<People>,
    tabs: Query<(Entity, &TabButton)>,
    rows: Query<(Entity, &FeatureRow)>,
    markers: Query<(Entity, &Marker, &ChildOf)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let mut want: BTreeMap<Entity, (Vec<Who>, bool)> = BTreeMap::new();
    for p in people.by_id.values() {
        let who = (
            p.key(),
            p.presence.name.clone(),
            people.stale.contains(&p.key()),
        );
        if let Some(tab) = p.presence.tab
            && let Some((e, _)) = tabs.iter().find(|(_, b)| b.0 == tab)
        {
            want.entry(e)
                .or_insert((Vec::new(), true))
                .0
                .push(who.clone());
        }
        // The feature they edit, and the features they have selected.
        let selected = people.doing.get(&p.key()).into_iter().flat_map(|a| {
            a.selected.iter().filter_map(|s| match s {
                Picked::Feature(f) => Some(*f),
                _ => None,
            })
        });
        for f in p
            .presence
            .editing
            .map(|(_, f)| f)
            .into_iter()
            .chain(selected)
        {
            if let Some((e, _)) = rows.iter().find(|(_, r)| r.0 == f) {
                let chips = &mut want.entry(e).or_insert((Vec::new(), false)).0;
                if !chips.contains(&who) {
                    chips.push(who.clone());
                }
            }
        }
    }
    let mut have: BTreeMap<Entity, Entity> = BTreeMap::new();
    for (m, Marker(ids), parent) in &markers {
        match want.get(&parent.parent()) {
            Some((w, _)) if w == ids => {
                have.insert(parent.parent(), m);
            }
            _ => commands.entity(m).despawn(),
        }
    }
    for (e, (ids, on_tab)) in want {
        if !have.contains_key(&e) {
            commands.entity(e).with_child(marker(ids, on_tab, &theme));
        }
    }
}

/// The follower's graphics area, outlined in the leader's colour.
#[derive(Component)]
struct FollowOutline(String);

fn sync_follow_outline(
    following: Option<Res<Following>>,
    people: Res<People>,
    area: Query<Entity, With<ViewportArea>>,
    outlines: Query<(Entity, &FollowOutline)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let want = following.as_ref().and_then(|f| {
        let name = people.by_id.get(&f.leader)?.presence.name.clone();
        Some((f.leader.clone(), name))
    });
    let current = outlines.iter().next();
    if current.map(|(_, o)| o.0.clone()) == want.as_ref().map(|w| w.0.clone()) {
        return;
    }
    if let Some((e, _)) = current {
        commands.entity(e).despawn();
    }
    let (Some((id, name)), Ok(area)) = (want, area.single()) else {
        return;
    };
    let color = color_of(&id);
    let t = &*theme;
    commands.entity(area).with_child((
        Name::new("follow-outline"),
        FollowOutline(id),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            border: UiRect::all(Val::Px(3.0)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BorderColor::all(color),
        Pickable::IGNORE,
        children![(
            Name::new("follow-banner"),
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(3.0), Val::Px(3.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius + 2.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                height: Val::Px(30.0),
                ..default()
            },
            BackgroundColor(color),
            cadrs_ui::color_scheme::KeepFill,
            Pickable::IGNORE,
            children![
                (
                    Avatar::new("follow-banner-avatar", name.clone())
                        .size(22.0)
                        .color(color.mix(&Color::BLACK, 0.25))
                        .build(t),
                    cadrs_ui::color_scheme::KeepFill,
                    Pickable::IGNORE,
                ),
                (
                    t.text(
                        format!("Following {name}"),
                        t.font_sm,
                        FontWeight::SEMIBOLD,
                        Avatar::ink_on(color)
                    ),
                    Pickable::IGNORE,
                ),
                // Any click stops following (`stop_on_press`), so this one does too; Esc, as
                // any key, as well.
                (
                    Name::new("follow-stop"),
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(t.radius)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        height: Val::Px(22.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.28)),
                    cadrs_ui::color_scheme::KeepFill,
                    Tooltip::new("Stop following (Esc, or click anywhere)"),
                    children![
                        (
                            t.text("Stop", t.font_sm, FontWeight::MEDIUM, Color::WHITE),
                            Pickable::IGNORE,
                        ),
                        (
                            Node {
                                padding: UiRect::axes(Val::Px(4.0), Val::Px(0.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            Pickable::IGNORE,
                            children![(
                                t.text("Esc", t.font_xs, FontWeight::MEDIUM, Color::WHITE),
                                Pickable::IGNORE,
                            )],
                        ),
                    ],
                ),
            ],
        )],
    ));
}
