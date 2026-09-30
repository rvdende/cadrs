//! Mate animation (P3B.3, `intro-to-assemblies.md` A6.9, A8.3, A16.5, X7):
//!
//! - **Animate mate DOF** (▶ in the mate dialog, A6.9): plays the mate's first DOF once, from 0
//!   to one end of its range, to the other end and back (the limits when it has them, else
//!   ±25 mm or a whole turn), on the dialog's placements; then the dialog's placements come back.
//! - The **Animate** dialog (the mate menu's Animate…, A16.5): the mate, the **DOF** to drive
//!   when the mate has more than one, **Start** / **End** (the limits when the mate has them),
//!   **Steps**, the **Playback type** (Single: once; Reciprocate: back and forth until stopped;
//!   Loop: start to end, repeated), **Reverse direction**, the read-only **Current value**, and
//!   **Play** / **Stop**. Closing it puts the assembly back.
//!
//! Each frame drives the mate to the next value and solves the whole assembly
//! ([`cadrs_core::assembly::solver`], the mate snapped with its other DOF held), from the last
//! frame's placements; the result is shown as a preview (no undo step). One step per frame, so
//! fewer steps play faster.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::mate::{Dof, MateId, MateType};
use cadrs_core::assembly::solver::{Drive, SolveOptions};
use cadrs_core::assembly::{Assembly, InstanceId};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    FeatureDialogAccept, FeatureDialogCancel, NumberField, NumberFieldCommit, NumberFieldState, Select, SelectChange,
    SelectState, SelectionList,
};

use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct AnimatePlugin;

impl Plugin for AnimatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (step_playback, sync_animate_dialog)
                .chain()
                .after(super::mate_dialog::MateDialogSet)
                .before(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<Playback>();
            commands.remove_resource::<AnimateSession>();
        })
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_select)
        .add_observer(on_number)
        .add_observer(on_button);
    }
}

/// How the Animate dialog plays (A16.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Single,
    Reciprocate,
    Loop,
}

impl PlayMode {
    const ALL: [PlayMode; 3] = [PlayMode::Single, PlayMode::Reciprocate, PlayMode::Loop];

    fn label(self) -> &'static str {
        match self {
            PlayMode::Single => "Single",
            PlayMode::Reciprocate => "Reciprocate",
            PlayMode::Loop => "Loop",
        }
    }
}

/// Where an animation came from: the mate dialog's ▶, or the Animate dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    MateDialog,
    AnimateDialog,
}

/// An animation in progress.
#[derive(Resource)]
pub struct Playback {
    pub mate: MateId,
    pub dof: Dof,
    /// The mate's positions, in order.
    pub values: Vec<f64>,
    pub mode: PlayMode,
    /// The next value's index, and the direction (Reciprocate).
    k: usize,
    forward: bool,
    /// Frames left to hold at an end (Reciprocate turns there).
    dwell: u32,
    pub playing: bool,
    /// The value shown now.
    pub current: f64,
    /// The animated assembly (its placements: the last frame's).
    model: Assembly,
    /// The assembly the preview is measured against (the document's).
    base: Assembly,
    solids: HashMap<InstanceId, Arc<cadrs_core::Solid>>,
    mover: Option<InstanceId>,
    pub source: Source,
}

/// The mate's range for a DOF: its limits, else ±25 mm or a whole turn.
fn range(m: &cadrs_core::assembly::mate::Mate, dof: Dof) -> (f64, f64) {
    m.limit(dof).unwrap_or(if dof.is_angle() { (-std::f64::consts::PI, std::f64::consts::PI) } else { (-25.0, 25.0) })
}

/// Stops any animation and puts the placements it showed back (the mate dialog's, or none).
pub fn stop(world: &mut World) {
    if world.remove_resource::<Playback>().is_none() {
        return;
    }
    let restore = world.get_resource::<super::mate_dialog::MateSession>().map(|s| s.preview.clone()).unwrap_or_default();
    world.resource_mut::<super::AssemblyParts>().preview = restore;
}

fn first_mover(model: &Assembly, mate: MateId) -> Option<InstanceId> {
    let ground = cadrs_core::assembly::solver::grounded(model);
    model.mate(mate)?.instances().into_iter().find(|i| !ground.contains(i))
}

/// The mate dialog's ▶ (A6.9): the dialog's mate, its first DOF, once through its range.
pub fn preview_dialog_mate(world: &mut World) {
    let Some(s) = world.get_resource::<super::mate_dialog::MateSession>().cloned() else { return };
    let Some(m) = s.mate() else { return };
    let Some(&dof) = m.mate_type.dof().first() else { return };
    let Some((doc_model, solids)) = super::mate_dialog::model_and_solids(world) else { return };
    let model = s.model(&doc_model);
    let (lo, hi) = range(&m, dof);
    let n = 24;
    let mut values = Vec::new();
    for (a, b) in [(0.0, hi), (hi, lo), (lo, 0.0)] {
        for k in 0..n {
            values.push(a + (b - a) * k as f64 / n as f64);
        }
    }
    values.push(0.0);
    let mover = s.mover(&doc_model);
    world.insert_resource(Playback {
        mate: s.id,
        dof,
        values,
        mode: PlayMode::Single,
        k: 0,
        forward: true,
        dwell: 0,
        playing: true,
        current: 0.0,
        model,
        base: doc_model,
        solids,
        mover,
        source: Source::MateDialog,
    });
}

/// Frames Reciprocate holds at each end.
const END_DWELL: u32 = 12;

/// Plays one step per frame.
fn step_playback(world: &mut World) {
    let Some(mut pb) = world.remove_resource::<Playback>() else { return };
    if !pb.playing {
        world.insert_resource(pb);
        return;
    }
    let Some(&value) = pb.values.get(pb.k) else {
        world.insert_resource(pb);
        return;
    };
    let opts = SolveOptions {
        movers: pb.mover.into_iter().collect(),
        snap: Some(pb.mate),
        drives: vec![Drive { mate: pb.mate, dof: pb.dof, value }],
        snap_only: false,
        hold_free: true,
    };
    let sol = cadrs_core::assembly::solve(&pb.model, &pb.solids, &opts);
    for (id, p) in &sol.poses {
        if let Some(i) = pb.model.instance_mut(*id) {
            i.pose = *p;
        }
    }
    pb.current = value;
    let preview: HashMap<InstanceId, cadrs_core::assembly::Pose> = sol.changed(&pb.base).into_iter().collect();
    world.resource_mut::<super::AssemblyParts>().preview = preview;
    // The next step.
    let last = pb.values.len() - 1;
    match pb.mode {
        PlayMode::Single => {
            if pb.k >= last {
                pb.playing = false;
            } else {
                pb.k += 1;
            }
        }
        PlayMode::Loop => pb.k = if pb.k >= last { 0 } else { pb.k + 1 },
        PlayMode::Reciprocate => {
            // At an end it holds a moment before turning back.
            let at_end = (pb.forward && pb.k >= last) || (!pb.forward && pb.k == 0);
            if at_end && pb.dwell == 0 {
                pb.dwell = END_DWELL;
            }
            if pb.dwell > 0 {
                pb.dwell -= 1;
                if pb.dwell > 0 {
                    world.insert_resource(pb);
                    return;
                }
                pb.forward = !pb.forward;
            }
            if last > 0 {
                pb.k = if pb.forward { pb.k + 1 } else { pb.k - 1 };
            }
        }
    }
    let done = !pb.playing && pb.source == Source::MateDialog;
    world.insert_resource(pb);
    if done {
        stop(world);
    }
}

// ---------------------------------------------------------------------------------------------
// The Animate dialog

/// The open Animate dialog (A16.5).
#[derive(Resource, Debug, Clone)]
pub struct AnimateSession {
    pub element: ElementId,
    pub mate: MateId,
    pub name: String,
    pub dofs: Vec<Dof>,
    pub dof: Dof,
    pub start: f64,
    pub end: f64,
    pub steps: u32,
    pub mode: PlayMode,
    pub reverse: bool,
    /// The mate's position along each of its DOF now (the Current value until ▶).
    pub now: Vec<(Dof, f64)>,
}

/// The mate's position along each of its DOF at the document's placements.
fn mate_now(world: &mut World, mate: MateId) -> Vec<(Dof, f64)> {
    let Some((model, solids)) = super::mate_dialog::model_and_solids(world) else { return Vec::new() };
    let Some(m) = model.mate(mate).and_then(|f| f.mate().cloned()) else { return Vec::new() };
    let world_frame = |c: &cadrs_core::assembly::connector::MateConnector| {
        let pose = model.instance(c.instance).map(|i| i.pose).unwrap_or_default();
        c.local_frame(solids.get(&c.instance).map(|x| &**x)).moved(&pose)
    };
    let (a, g) = (world_frame(&m.connectors[0]), m.target(&world_frame(&m.connectors[1])));
    m.mate_type.dof().iter().map(|d| (*d, cadrs_core::assembly::mate::dof_value(&a, &g, *d))).collect()
}

/// Opens the Animate dialog on a mate with DOF (the mate menu's Animate…).
pub fn open_animate_dialog(world: &mut World, mate: MateId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(f) = doc.active_element().and_then(|e| e.assembly_model()?.mate(mate).cloned()) else { return };
    let Some(m) = f.mate() else { return };
    let dofs = m.mate_type.dof().to_vec();
    let Some(&dof) = dofs.first() else { return };
    super::mate_dialog::cancel(world);
    world.remove_resource::<super::group_dialog::GroupSession>();
    stop(world);
    let (start, end) = if m.limit(dof).is_some() { range(m, dof) } else if dof.is_angle() { (0.0, std::f64::consts::TAU) } else { (0.0, 25.4) };
    let now = mate_now(world, mate);
    world.insert_resource(AnimateSession {
        element,
        mate,
        name: f.name.clone(),
        dofs,
        dof,
        start,
        end,
        steps: 25,
        mode: PlayMode::Single,
        reverse: false,
        now,
    });
}

fn close(world: &mut World) {
    world.remove_resource::<AnimateSession>();
    stop(world);
}

/// Play: from the document's placements, Start → End (reversed: End → Start) in Steps.
fn play(world: &mut World) {
    let Some(s) = world.get_resource::<AnimateSession>().cloned() else { return };
    let Some((model, solids)) = super::mate_dialog::model_and_solids(world) else { return };
    // Continue from where the last run left the assembly.
    let mut from = model.clone();
    if let Some(pb) = world.get_resource::<Playback>() {
        from = pb.model.clone();
    }
    let n = s.steps.max(1) as usize;
    let (a, b) = if s.reverse { (s.end, s.start) } else { (s.start, s.end) };
    let values: Vec<f64> = (0..=n).map(|k| a + (b - a) * k as f64 / n as f64).collect();
    let mover = first_mover(&model, s.mate);
    world.insert_resource(Playback {
        mate: s.mate,
        dof: s.dof,
        values,
        mode: s.mode,
        k: 0,
        forward: true,
        dwell: 0,
        playing: true,
        current: a,
        model: from,
        base: model,
        solids,
        mover,
        source: Source::AnimateDialog,
    });
}

#[derive(Component)]
struct AnimateDialog;

/// What an Animate dialog widget edits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Dof,
    Start,
    End,
    Steps,
    Mode,
}

/// What the dialog was built for.
#[derive(Component, Debug, Clone, PartialEq)]
struct AnimateLayout {
    mate: MateId,
    dofs: usize,
}

fn dof_label(d: Dof) -> &'static str {
    match d {
        Dof::X => "X translation",
        Dof::Y => "Y translation",
        Dof::Z => "Z translation",
        Dof::Angle => "Z rotation",
    }
}

fn value_text(units: &cadrs_sketch::units::Units, dof: Dof, v: f64) -> String {
    if dof.is_angle() { units.with_unit(v.to_degrees(), Quantity::Angle) } else { units.with_unit(v, Quantity::Length) }
}

fn animate_dialog(t: &Theme, s: &AnimateSession, units: &cadrs_sketch::units::Units) -> impl Bundle {
    let tb = t.clone();
    let tf = t.clone();
    let s2 = s.clone();
    let units = *units;
    (
        AnimateDialog,
        AnimateLayout { mate: s.mate, dofs: s.dofs.len() },
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("animate-dialog")
            .title("Animate")
            .valid(true)
            .width(224.0)
            .body(move |b| {
                let t = &tb;
                let s = &s2;
                b.spawn(SelectionList::new("animate-mate").placeholder("Mate").items(vec![s.name.clone()]).build(t))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(3.0), Val::Px(4.0));
                    });
                if s.dofs.len() > 1 {
                    let mut sel = Select::new("animate-dof");
                    for d in &s.dofs {
                        sel = sel.option(dof_label(*d), true);
                    }
                    let i = s.dofs.iter().position(|d| *d == s.dof).unwrap_or(0);
                    b.spawn(Node { height: Val::Px(28.0), align_items: AlignItems::Center, ..default() })
                        .with_child((Field::Dof, sel.selected(i).build(t)));
                }
                for (field, name, label, text) in [
                    (Field::Start, "animate-start", "Start", value_text(&units, s.dof, s.start)),
                    (Field::End, "animate-end", "End", value_text(&units, s.dof, s.end)),
                    (Field::Steps, "animate-steps", "Steps", s.steps.to_string()),
                ] {
                    b.spawn((field, NumberField::new(name, label).text(text).label_width(64.0).build(t)));
                }
                let mut mode = Select::new("animate-playback");
                for m in PlayMode::ALL {
                    mode = mode.option(m.label(), true);
                }
                let i = PlayMode::ALL.iter().position(|m| *m == s.mode).unwrap_or(0);
                b.spawn(Node { height: Val::Px(28.0), align_items: AlignItems::Center, ..default() })
                    .with_child((Field::Mode, mode.selected(i).build(t)));
                b.spawn(Node { height: Val::Px(26.0), align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                    r.spawn(
                        IconButton::new("animate-reverse", "flip-horizontal")
                            .icon_size(16.0)
                            .selected(s.reverse)
                            .tooltip("Reverse direction")
                            .build(t),
                    );
                    r.spawn((Name::new("animate-current-label"), t.text("Current value", 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground)));
                    r.spawn(Node { flex_grow: 1.0, ..default() });
                    r.spawn((
                        Name::new("animate-current"),
                        CurrentValue,
                        t.text(value_text(&units, s.dof, s.now_of(s.dof)), 11.5, bevy::text::FontWeight::MEDIUM, t.foreground),
                    ));
                });
                // Which way it is going while it plays (→ toward End, ← toward Start).
                b.spawn(Node { height: Val::Px(18.0), justify_content: JustifyContent::FlexEnd, ..default() }).with_child((
                    Name::new("animate-direction"),
                    DirectionText,
                    t.text("", 10.5, bevy::text::FontWeight::NORMAL, t.muted_foreground),
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(Node { flex_grow: 1.0, align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                    for (name, icon_name, size, tip) in [("animate-play", "play", 21.0, "Play"), ("animate-stop", "stop", 21.0, "Stop")] {
                        r.spawn(IconButton::new(name, icon_name).icon_size(size).tooltip(tip).build(t)).entry::<Node>().and_modify(|mut n| {
                            n.width = Val::Px(26.0);
                            n.height = Val::Px(26.0);
                            n.overflow = Overflow::visible();
                        });
                    }
                    r.spawn(Node { flex_grow: 1.0, ..default() });
                    r.spawn((Name::new("animate-help"), icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
                });
            })
            .build(t),
    )
}

/// The read-only Current value.
#[derive(Component)]
struct CurrentValue;

/// The direction line under it.
#[derive(Component)]
struct DirectionText;

impl AnimateSession {
    fn now_of(&self, d: Dof) -> f64 {
        self.now.iter().find(|(x, _)| *x == d).map(|(_, v)| *v).unwrap_or(0.0)
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_animate_dialog(
    session: Option<Res<AnimateSession>>,
    playback: Option<Res<Playback>>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    q_dialog: Query<(Entity, &AnimateLayout), With<AnimateDialog>>,
    mut q_num: Query<(&Field, &mut NumberFieldState)>,
    mut q_sel: Query<(&Field, &mut SelectState)>,
    mut q_current: Query<(&mut Text, Has<DirectionText>), Or<(With<CurrentValue>, With<DirectionText>)>>,
    q_buttons: Query<(Entity, &Name, Has<cadrs_ui::style::Selected>)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for (e, _) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let layout = AnimateLayout { mate: s.mate, dofs: s.dofs.len() };
    match q_dialog.iter().next() {
        Some((_, l)) if *l == layout => {}
        other => {
            if let Some((e, _)) = other {
                commands.entity(e).try_despawn();
            }
            let Some(area) = q_area.iter().next() else { return };
            let d = commands.spawn(animate_dialog(&theme, &s, &units.0)).id();
            commands.entity(area).add_child(d);
            return;
        }
    }
    for (f, mut st) in &mut q_num {
        let want = match f {
            Field::Start => value_text(&units.0, s.dof, s.start),
            Field::End => value_text(&units.0, s.dof, s.end),
            Field::Steps => s.steps.to_string(),
            _ => continue,
        };
        if !st.error && st.text != want {
            st.text = want;
        }
    }
    for (f, mut st) in &mut q_sel {
        let want = match f {
            Field::Dof => s.dofs.iter().position(|d| *d == s.dof).unwrap_or(0),
            Field::Mode => PlayMode::ALL.iter().position(|m| *m == s.mode).unwrap_or(0),
            _ => continue,
        };
        if st.selected != want {
            st.selected = want;
        }
    }
    let pb = playback.as_ref().filter(|p| p.source == Source::AnimateDialog);
    let current = pb.map(|p| p.current).unwrap_or(s.now_of(s.dof));
    let text = value_text(&units.0, s.dof, current);
    let direction = match pb {
        Some(p) if p.playing && p.dwell > 0 => "turning at the end".to_string(),
        Some(p) if p.playing => {
            // Along the values' order (Start → End unless reversed), or back.
            let up = p.values.last().copied().unwrap_or(0.0) >= p.values.first().copied().unwrap_or(0.0);
            let to_end = p.forward == up;
            if to_end { "→ toward End".to_string() } else { "← toward Start".to_string() }
        }
        Some(_) => "stopped".to_string(),
        None => String::new(),
    };
    for (mut t, is_dir) in &mut q_current {
        let want = if is_dir { &direction } else { &text };
        if t.0 != *want {
            t.0 = want.clone();
        }
    }
    for (e, n, sel) in &q_buttons {
        if n.as_str() == "animate-reverse" && sel != s.reverse {
            if s.reverse {
                commands.entity(e).insert(cadrs_ui::style::Selected);
            } else {
                commands.entity(e).remove::<cadrs_ui::style::Selected>();
            }
        }
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<AnimateDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<AnimateDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Field>, mut commands: Commands) {
    let Ok(f) = q.get(ev.entity).copied() else { return };
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        let doc_limits = world.get_resource::<ActiveDocument>().and_then(|d| {
            let s = world.get_resource::<AnimateSession>()?;
            d.active_element()?.assembly_model()?.mate(s.mate)?.mate().cloned()
        });
        let Some(mut s) = world.get_resource_mut::<AnimateSession>() else { return };
        match f {
            Field::Dof => {
                if let Some(d) = s.dofs.get(i).copied() {
                    s.dof = d;
                    let (a, b) = match &doc_limits {
                        Some(m) if m.limit(d).is_some() => range(m, d),
                        _ if d.is_angle() => (0.0, std::f64::consts::TAU),
                        _ => (0.0, 25.4),
                    };
                    s.start = a;
                    s.end = b;
                }
            }
            Field::Mode => {
                if let Some(m) = PlayMode::ALL.get(i) {
                    s.mode = *m;
                }
            }
            _ => {}
        }
    });
}

fn on_number(ev: On<NumberFieldCommit>, q: Query<&Field>, mut q_state: Query<&mut NumberFieldState>, units: Res<crate::WorkspaceUnits>, session: Option<ResMut<AnimateSession>>) {
    let (Ok(f), Some(mut s)) = (q.get(ev.entity).copied(), session) else { return };
    let text = ev.text.trim();
    let ok = match f {
        Field::Steps => text.parse::<f64>().ok().filter(|v| *v >= 1.0).map(|v| s.steps = v.round() as u32).is_some(),
        Field::Start | Field::End => {
            let q = if s.dof.is_angle() { Quantity::Angle } else { Quantity::Length };
            match units.0.eval(text, q) {
                Ok(v) if v.is_finite() => {
                    let v = if s.dof.is_angle() { v.to_radians() } else { v };
                    if f == Field::Start {
                        s.start = v;
                    } else {
                        s.end = v;
                    }
                    true
                }
                _ => false,
            }
        }
        _ => true,
    };
    if let Ok(mut st) = q_state.get_mut(ev.entity) {
        st.error = !ok;
        if !ok {
            st.text = text.to_string();
        }
    }
}

fn on_button(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity) else { return };
    match name.as_str() {
        "animate-play" => commands.queue(play),
        "animate-stop" => commands.queue(|world: &mut World| {
            if let Some(mut pb) = world.get_resource_mut::<Playback>() {
                pb.playing = false;
            }
        }),
        "animate-reverse" => commands.queue(|world: &mut World| {
            if let Some(mut s) = world.get_resource_mut::<AnimateSession>() {
                s.reverse = !s.reverse;
            }
        }),
        _ => {}
    }
}

/// Whether a mate type can be animated (it has a DOF to drive).
pub fn animatable(t: MateType) -> bool {
    !t.dof().is_empty()
}
