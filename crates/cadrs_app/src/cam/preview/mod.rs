//! Preview operation: a toolpath (or a whole setup's) played back over its stock, from a bar
//! at the bottom of the view, as Onshape's Back plot, Verify and Simulate:
//!
//! - **Back plot**: the tool moves along the toolpath; what's done is drawn in its colours,
//!   what's left grey.
//! - **Verify**: the stock as the tool leaves it (a height map lowered under the cutter, or the
//!   kerf in a sheet), cut areas shaded by depth, gouges red.
//! - **Simulate**: Verify with the tool's holder drawn, and the collisions found (rapids into
//!   material, cuts deeper than the flutes) listed; it can pause on a collision.
//!
//! The bar: the three modes, the speed, Step back · Run/Pause · Step next · Go to end · Reset ·
//! Close, the progress (drag to scrub), and toggles for the tool, the toolpath, the part and
//! pausing on collisions. Its readout shows the time, the operation, the feed and the tool's
//! position; at the end, what Verify found.

use web_time::Instant;

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::cam::cadrs_cam::heightmap::{EMPTY, HeightMap};
use cadrs_core::cam::cadrs_cam::sim::{self, SimOp};
use cadrs_core::cam::cadrs_cam::strategy::Stock;
use cadrs_core::cam::cadrs_cam::tool::Tool;
use cadrs_core::cam::cadrs_cam::toolpath::{Segment, Timeline};
use cadrs_core::cam::cadrs_cam::{Frame, P3};
use cadrs_core::cam::{CamId, NodeRef, OpKind, program};
use cadrs_ui::i18n::{Translated, tr, tr_with};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, Checkbox, CheckboxChange, Slider, SliderChange};

use super::view::{fill_motion, v3};
use super::{CamUi, Computed, active_cam_owned, toast};
use crate::AppState;
use crate::viewport::ViewportArea;

mod analysis;

use analysis::{Analysis, analyse};

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                poll_analysis,
                advance,
                sync_bar,
                sync_readout,
                draw,
                sync_stock_mesh,
            )
                .chain()
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<PreviewState>();
        })
        .add_observer(on_button)
        .add_observer(on_slider)
        .add_observer(on_check);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    BackPlot,
    Verify,
    Simulate,
}

/// An operation in the preview: where its moves start, and its name and tool.
#[derive(Clone)]
struct OpSpan {
    first: usize,
    name: String,
    tool: Tool,
    /// In a tilted work plane (3+2): its cuts aren't played on the height map shown (the
    /// analysis cuts them on voxels in its own frame).
    tilted: bool,
    /// A tilted operation's own frame in the run's coordinates (it's checked in it).
    frame: Option<Frame>,
    /// The tool's axis (the run's coordinates).
    axis: P3,
}

/// The open preview.
#[derive(Resource)]
pub struct PreviewState {
    pub mode: Mode,
    /// "Rough 1", "Setup 1".
    pub label: String,
    /// The setup's frame: the moves are in its coordinates.
    frame: Frame,
    timeline: Timeline,
    ops: Vec<OpSpan>,
    beam: bool,
    pub t: f64,
    pub playing: bool,
    /// 0 (slow) to 1 (fast).
    pub speed: f32,
    pub show_tool: bool,
    pub show_path: bool,
    pub show_part: bool,
    pub pause_on_collision: bool,
    stock: Stock,
    /// The stock as cut so far (mills), and how many moves are in it.
    map: Option<HeightMap>,
    applied: usize,
    /// The part on the stock's grid, pulled in by a cell (gouges are below it).
    part: Option<HeightMap>,
    top: f32,
    mesh_dirty: bool,
    mesh_at: Option<Instant>,
    /// What the whole run finds: a summary line, and the collisions' times.
    pub summary: String,
    collisions: Vec<(f64, String)>,
    paused_at: Option<f64>,
    /// The stock before the run: as the toolpaths before the previewed ones leave it.
    start: Option<HeightMap>,
    /// A cut sheet's pieces (when the run's check is done), each with whether it's a part.
    sheet: Option<Vec<(cadrs_core::cam::cadrs_cam::region::Region, bool)>>,
    /// How many warnings the run's toolpaths carry (the readout counts them).
    warnings: usize,
    /// The check of the whole run, on a worker thread until it's done.
    analysis: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Analysis>>>,
}

impl PreviewState {
    /// Whether the run's check is still being worked out.
    pub fn checking(&self) -> bool {
        self.analysis.is_some()
    }

    /// Verify and Simulate show their own stock.
    pub fn hides_stock(&self) -> bool {
        self.mode != Mode::BackPlot
    }

    fn total(&self) -> f64 {
        self.timeline.total()
    }

    fn segment_index(&self) -> usize {
        self.timeline.at(self.t).map_or(0, |(_, i)| i)
    }

    fn op_at(&self, seg: usize) -> Option<&OpSpan> {
        self.ops.iter().rev().find(|o| o.first <= seg)
    }

    /// Minutes of program per second of playback.
    fn rate(&self) -> f64 {
        // From two minutes for the whole program (slowest) to about two seconds (fastest).
        let secs = 120.0 * 0.02f64.powf(self.speed as f64);
        (self.total() / secs).max(1e-6)
    }

    /// Jumps to a fraction of the program (0 to 1).
    pub fn seek(&mut self, f: f64) {
        let t = self.total() * f.clamp(0.0, 1.0);
        self.jump(t);
    }

    fn jump(&mut self, t: f64) {
        let t = t.clamp(0.0, self.total());
        if t < self.t - 1e-12 && self.map.is_some() {
            // Back: the stock again from the start (as the toolpaths before left it).
            self.map = Some(match &self.start {
                Some(m) => m.clone(),
                None => self.stock.height_map(self.map.as_ref().unwrap().cell),
            });
            self.applied = 0;
        }
        self.t = t;
        self.cut_to_now();
    }

    /// Lowers the stock under every move up to now.
    fn cut_to_now(&mut self) {
        let Some(map) = &mut self.map else { return };
        let upto = self.timeline.at(self.t).map_or(0, |(_, i)| i);
        let done = if self.t >= self.timeline.total() - 1e-12 {
            self.timeline.segments.len()
        } else {
            upto
        };
        while self.applied < done {
            let s = self.timeline.segments[self.applied];
            let op = self
                .ops
                .iter()
                .rev()
                .find(|o| o.first <= self.applied)
                .unwrap_or(&self.ops[0]);
            if !s.is_rapid() && !op.tilted {
                map.cut(&op.tool, s.from, s.to);
            }
            self.applied += 1;
            self.mesh_dirty = true;
        }
    }
}

/// Preview operation for the selection: a toolpath, or every toolpath of a setup (a job: its
/// first setup).
pub fn open_for_selection(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let sel = world.resource::<CamUi>().selected.clone();
    let (job, setup, ops, label) = {
        let mut found = None;
        for id in sel.iter().rev() {
            match cam.find(*id) {
                Some(NodeRef::Operation(j, s, o)) => {
                    let op = &cam.jobs[j].setups[s].operations[o];
                    if matches!(op.kind, OpKind::Toolpath(_)) {
                        found = Some((j, s, Some(vec![op.id]), op.name.clone()));
                        break;
                    }
                }
                Some(NodeRef::Setup(j, s)) => {
                    found = Some((j, s, None, cam.jobs[j].setups[s].name.clone()));
                    break;
                }
                Some(NodeRef::Job(j)) if !cam.jobs[j].setups.is_empty() => {
                    found = Some((j, 0, None, cam.jobs[j].setups[0].name.clone()));
                    break;
                }
                _ => {}
            }
        }
        match found.or_else(|| {
            let j = cam
                .jobs
                .iter()
                .position(|j| Some(j.id) == cam.active().map(|a| a.id))?;
            (!cam.jobs[j].setups.is_empty())
                .then(|| (j, 0, None, cam.jobs[j].setups[0].name.clone()))
        }) {
            Some(f) => f,
            None => {
                toast(world, &tr("cam-select-toolpath-to-preview"));
                return;
            }
        }
    };
    let job = &cam.jobs[job];
    let setup = &job.setups[setup];
    let Some(element) = super::active_cam_id(world) else {
        return;
    };
    let Some(parts) = super::compute::studio_parts(world, element) else {
        toast(world, &tr("cam-parts-still-being-built"));
        return;
    };
    // Every toolpath must be calculated.
    let wanted: Vec<CamId> = setup
        .operations
        .iter()
        .filter(|o| !o.suppressed && matches!(o.kind, OpKind::Toolpath(_)))
        .filter(|o| ops.as_ref().is_none_or(|v| v.contains(&o.id)))
        .map(|o| o.id)
        .collect();
    // The toolpaths before the first one previewed (the setups' before this one's too): the
    // stock they leave is where it starts.
    let first = setup
        .operations
        .iter()
        .position(|o| wanted.contains(&o.id))
        .unwrap_or(0);
    let earlier_setups = job
        .setups
        .iter()
        .take_while(|s| s.id != setup.id)
        .filter(|s| !s.suppressed)
        .flat_map(|s| s.operations.iter());
    let before: Vec<CamId> = earlier_setups
        .chain(&setup.operations[..first])
        .filter(|o| !o.suppressed && matches!(o.kind, OpKind::Toolpath(_)))
        .map(|o| o.id)
        .collect();
    let first_id = setup.operations.get(first).map(|o| o.id);
    let missing: Vec<CamId> = before
        .iter()
        .chain(&wanted)
        .copied()
        .filter(|o| world.resource::<Computed>().current(*o).is_none())
        .collect();
    if !missing.is_empty() {
        world.resource_mut::<Computed>().request(missing);
        toast(world, &tr("cam-calculating-toolpaths-first-preview"));
        return;
    }
    if wanted.is_empty() {
        toast(world, &tr("cam-setup-has-no-toolpaths"));
        return;
    }
    let paths = world.resource::<Computed>().paths();
    let run = match program::setup_run(&cam, job, setup, &parts, &paths, Some(&wanted)) {
        Ok(r) => r,
        Err(e) => {
            toast(world, &e);
            return;
        }
    };
    // What it takes to work out that stock (on the worker thread).
    let carry = (!before.is_empty()).then(|| {
        (
            super::compute::carry_key(world, &cam, job.id, setup.id, first_id),
            cam.clone(),
            job.id,
            setup.id,
            first_id,
            parts.clone(),
            paths.clone(),
        )
    });
    let Some(machine) = job.machine.clone() else {
        return;
    };
    let Some(shape) = program::stock_shape(&cam, job, &parts) else {
        return;
    };
    let stock = program::stock_in(&shape, &run.frame);
    let mut segments: Vec<Segment> = Vec::new();
    let mut spans = Vec::new();
    // As the program moves: from op to op, with the post's entries.
    let start: P3 = [0.0, 0.0, stock.bounds.max[2] + 30.0];
    let motions = program::run_motions(setup, job, &run, start);
    for (o, m) in run.ops.iter().zip(motions) {
        spans.push(OpSpan {
            first: segments.len(),
            name: o.name.to_string(),
            tool: o.tool.clone(),
            tilted: o.tilted,
            frame: o
                .plane
                .as_ref()
                .filter(|_| o.tilted)
                .map(|(wf, _, _)| run.frame.relative(wf)),
            axis: o
                .plane
                .as_ref()
                .map_or([0.0, 0.0, 1.0], |(wf, _, _)| run.frame.dir_to_local(wf.z)),
        });
        segments.extend(m);
    }
    // The model the toolpaths were calculated against (the kernel's: the finest of them),
    // else the display meshes.
    let model = {
        let c = world.resource::<Computed>();
        wanted
            .iter()
            .filter_map(|o| c.current(*o))
            .filter(|d| d.frame == run.frame)
            .map(|d| d.model.clone())
            .max_by_key(|m| m.triangles.len())
    }
    .unwrap_or_else(|| {
        std::sync::Arc::new(cadrs_core::cam::geometry::model_from_parts(
            &program::job_bodies(&cam, job, &parts),
            &run.frame,
        ))
    });
    let beam = machine.kind.is_cutting();
    let cell = sim::sim_cell(&stock, 0.25);
    let (map, top) = if beam {
        (None, 0.0)
    } else {
        (Some(stock.height_map(cell)), stock.bounds.max[2] as f32)
    };
    // What the whole run finds, once, on a worker thread.
    let analysis = {
        let (tx, rx) = std::sync::mpsc::channel();
        let (stock, segments, spans) = (stock.clone(), segments.clone(), spans.clone());
        let fixtures = program::workholding_triangles(job, &parts, &run.frame);
        let frame = run.frame;
        let spawned = cadrs_core::background::spawn("cam-verify", move || {
            let a = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let start = carry.and_then(|(key, cam, job, setup, upto, parts, paths)| {
                    super::compute::carried_stock(key, &frame, cell, || {
                        program::stock_before(
                            &cam,
                            cam.job(job)?,
                            setup,
                            upto,
                            &parts,
                            &paths,
                            &frame,
                            cell,
                        )
                    })
                });
                analyse(
                    &stock, &model, &fixtures, start, &segments, &spans, beam, cell,
                )
            }))
            .unwrap_or_else(|_| Analysis {
                summary: tr("cam-check-failed-unexpectedly"),
                collisions: Vec::new(),
                part: None,
                start: None,
                sheet: None,
            });
            let _ = tx.send(a);
        });
        spawned.ok().map(|_| std::sync::Mutex::new(rx))
    };
    let timeline = Timeline::new(segments);
    super::dialogs::close(world);
    super::export::close(world);
    world.insert_resource(PreviewState {
        mode: Mode::BackPlot,
        label,
        frame: run.frame,
        timeline,
        ops: spans,
        beam,
        t: 0.0,
        playing: true,
        speed: 0.5,
        show_tool: true,
        show_path: true,
        show_part: true,
        pause_on_collision: true,
        stock,
        map,
        applied: 0,
        part: None,
        top,
        mesh_dirty: true,
        mesh_at: None,
        summary: tr("cam-checking-the-run"),
        collisions: Vec::new(),
        paused_at: None,
        start: None,
        sheet: None,
        warnings: run.ops.iter().map(|o| o.toolpath.warnings.len()).sum(),
        analysis,
    });
}

/// Takes the run's check when the worker has finished it.
fn poll_analysis(state: Option<ResMut<PreviewState>>) {
    let Some(mut s) = state else { return };
    let Some(rx) = s.analysis.as_mut() else {
        return;
    };
    let got = match rx.get_mut().map(|r| r.try_recv()) {
        Ok(Ok(a)) => a,
        Ok(Err(std::sync::mpsc::TryRecvError::Empty)) => return,
        _ => Analysis {
            summary: tr("cam-check-stopped-unexpectedly"),
            collisions: Vec::new(),
            part: None,
            start: None,
            sheet: None,
        },
    };
    s.analysis = None;
    s.summary = match s.warnings {
        0 => got.summary,
        1 => format!("{} · ⚠ {}", got.summary, tr("cam-one-toolpath-warning")),
        n => format!(
            "{} · ⚠ {}",
            got.summary,
            tr_with("cam-toolpath-warnings", &[("count", &n.to_string())])
        ),
    };
    s.collisions = got.collisions;
    s.part = got.part;
    s.sheet = got.sheet;
    if let Some(start) = got.start {
        // Played again from the in-process stock.
        s.map = Some(start.clone());
        s.start = Some(start);
        s.applied = 0;
        s.cut_to_now();
    }
    s.mesh_dirty = true;
}

fn fmt0(v: f64) -> String {
    let s = format!("{:.0}", v);
    // Thin spaces between thousands.
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 && c != '-' {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}
fn fmt1(v: f64) -> String {
    format!("{v:.1}")
}
fn fmt2(v: f64) -> String {
    format!("{v:.2}")
}

/// Plays the preview on.
fn advance(time: Res<Time>, state: Option<ResMut<PreviewState>>) {
    let Some(mut s) = state else { return };
    if !s.playing {
        return;
    }
    let to = s.t + time.delta_secs_f64() * s.rate();
    // Pause on the next collision.
    if s.pause_on_collision
        && s.mode == Mode::Simulate
        && let Some((t, _)) = s
            .collisions
            .iter()
            .find(|(t, _)| *t > s.t + 1e-9 && *t <= to)
            .cloned()
        && s.paused_at != Some(t)
    {
        s.jump(t);
        s.playing = false;
        s.paused_at = Some(t);
        return;
    }
    s.jump(to);
    if s.t >= s.total() - 1e-12 {
        s.playing = false;
    }
}

// ---------------------------------------------------------------------------------------------
// The bar

#[derive(Component)]
struct PreviewBar;

#[derive(Component)]
struct Readout;

#[derive(Component, PartialEq, Clone)]
struct BarKey(Mode, bool, bool, bool, bool, bool, bool);

fn sync_bar(
    state: Option<Res<PreviewState>>,
    q: Query<(Entity, &BarKey), With<PreviewBar>>,
    q_area: Query<Entity, With<ViewportArea>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Some(s) = state else {
        for (e, _) in &q {
            commands.entity(e).despawn();
        }
        return;
    };
    let key = BarKey(
        s.mode,
        s.playing,
        s.show_tool,
        s.show_path,
        s.show_part,
        s.pause_on_collision,
        s.beam,
    );
    if let Ok((e, k)) = q.single() {
        if *k == key {
            return;
        }
        commands.entity(e).despawn();
    }
    let Some(area) = q_area.iter().next() else {
        return;
    };
    let t = theme.clone();
    let speed = s.speed;
    let progress = if s.total() > 0.0 {
        (s.t / s.total()) as f32
    } else {
        0.0
    };
    let bar = commands
        .spawn((
            Name::new("cam-preview-bar"),
            PreviewBar,
            key.clone(),
            DespawnOnExit(AppState::Document),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                bottom: Val::Px(14.0),
                margin: UiRect::left(Val::Px(-330.0)),
                width: Val::Px(660.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(t.popover),
            BorderColor::all(t.border),
        ))
        .with_children(|b| {
            b.spawn((
                Name::new("cam-pv-controls"),
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    ..default()
                },
            ))
            .with_children(|r| {
                for (name, label, m) in [
                    ("cam-pv-backplot", "cam-back-plot", Mode::BackPlot),
                    ("cam-pv-verify", "cam-verify", Mode::Verify),
                    ("cam-pv-simulate", "cam-simulate", Mode::Simulate),
                ] {
                    let mut bt = Button::new(name).label(tr(label)).small();
                    bt = if s.mode == m {
                        bt.primary()
                    } else {
                        bt.outline()
                    };
                    r.spawn(bt.build(&t));
                }
                r.spawn((
                    Name::new("cam-pv-gap"),
                    Node {
                        width: Val::Px(12.0),
                        ..default()
                    },
                ));
                r.spawn((
                    t.text(
                        tr("cam-preview-speed"),
                        11.0,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Translated("cam-preview-speed"),
                ));
                r.spawn(
                    Slider::new("cam-pv-speed")
                        .value(speed)
                        .width(110.0)
                        .tooltip(tr("cam-playback-speed"))
                        .build(&t),
                );
                r.spawn((
                    Name::new("cam-pv-spacer"),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                // (`tip` is a catalog key.)
                for (name, icon, tip) in [
                    ("cam-pv-back", "chevron-left", "cam-step-back"),
                    (
                        "cam-pv-run",
                        if s.playing { "pause" } else { "play" },
                        if s.playing { "cam-pause" } else { "cam-run" },
                    ),
                    ("cam-pv-next", "chevron-right", "cam-step-next"),
                    ("cam-pv-end", "skip-forward", "cam-go-to-end"),
                    ("cam-pv-reset", "skip-back", "cam-reset"),
                    ("cam-pv-close", "close", "cam-close-the-preview"),
                ] {
                    r.spawn(IconButton::new(name, icon).tooltip(tr(tip)).build(&t));
                }
            });
            b.spawn(
                Slider::new("cam-pv-progress")
                    .value(progress)
                    .width(638.0)
                    .tooltip(tr("cam-drag-through-program"))
                    .build(&t),
            );
            b.spawn((
                Name::new("cam-pv-options"),
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
            ))
            .with_children(|r| {
                r.spawn(
                    Checkbox::new("cam-pv-tool")
                        .label(tr("cam-tool"))
                        .checked(s.show_tool)
                        .build(&t),
                );
                r.spawn(
                    Checkbox::new("cam-pv-path")
                        .label(tr("cam-toolpath"))
                        .checked(s.show_path)
                        .build(&t),
                );
                r.spawn(
                    Checkbox::new("cam-pv-part")
                        .label(tr("common-part"))
                        .checked(s.show_part)
                        .build(&t),
                );
                if s.mode == Mode::Simulate && !s.beam {
                    r.spawn(
                        Checkbox::new("cam-pv-pause")
                            .label(tr("cam-pause-on-collision"))
                            .checked(s.pause_on_collision)
                            .build(&t),
                    );
                }
            });
            b.spawn((
                Readout,
                Name::new("cam-pv-readout"),
                t.text("", 11.5, FontWeight::NORMAL, t.foreground),
            ));
        })
        .id();
    commands.entity(area).add_child(bar);
}

/// The readout and the progress slider follow the time.
fn sync_readout(
    state: Option<Res<PreviewState>>,
    mut q: Query<&mut Text, With<Readout>>,
    mut q_slider: Query<(&Name, &mut cadrs_ui::SliderState)>,
) {
    let Some(s) = state else { return };
    let i = s.segment_index();
    let seg = s.timeline.segments.get(i);
    let pos = s.timeline.at(s.t).map(|(p, _)| p).unwrap_or([0.0; 3]);
    let op = s.op_at(i).map(|o| o.name.as_str()).unwrap_or("");
    let feed = match seg.and_then(|g| g.feed) {
        Some(f) => tr_with("cam-readout-feed", &[("feed", &fmt0(f))]),
        None => tr("cam-readout-rapid"),
    };
    let mut text = format!(
        "{} · {} / {} · {} · {} · X{:.3} Y{:.3} Z{:.3}",
        s.label,
        super::compute::fmt_minutes(s.t),
        super::compute::fmt_minutes(s.total()),
        op,
        feed,
        pos[0],
        pos[1],
        pos[2]
    );
    if s.mode != Mode::BackPlot && s.t >= s.total() - 1e-12 {
        text = format!("{} · {}", s.label, s.summary);
    }
    if s.mode == Mode::Simulate
        && let Some(p) = s.paused_at
        && (p - s.t).abs() < 1e-9
        && let Some((_, why)) = s.collisions.iter().find(|(t, _)| (*t - p).abs() < 1e-9)
    {
        text = tr_with("cam-readout-paused", &[("reason", why)]);
    }
    for mut t in &mut q {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
    let progress = if s.total() > 0.0 {
        (s.t / s.total()) as f32
    } else {
        0.0
    };
    for (n, mut st) in &mut q_slider {
        if n.as_str() == "cam-pv-progress" && (st.value - progress).abs() > 1e-4 {
            st.value = progress;
        }
    }
}

fn on_button(ev: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(ev.entity) else { return };
    let name = n.as_str().to_string();
    if !name.starts_with("cam-pv-") {
        return;
    }
    commands.queue(move |w: &mut World| {
        if name == "cam-pv-close" {
            w.remove_resource::<PreviewState>();
            return;
        }
        let Some(mut s) = w.get_resource_mut::<PreviewState>() else {
            return;
        };
        match name.as_str() {
            // The toolpath on for the back plot, off over the cut stock (the toggle brings it
            // back).
            "cam-pv-backplot" => {
                s.mode = Mode::BackPlot;
                s.show_path = true;
            }
            "cam-pv-verify" => {
                s.mode = Mode::Verify;
                s.show_path = false;
                s.mesh_dirty = true;
            }
            "cam-pv-simulate" => {
                s.mode = Mode::Simulate;
                s.show_path = false;
                s.mesh_dirty = true;
            }
            "cam-pv-run" => {
                if s.t >= s.total() - 1e-12 {
                    s.jump(0.0);
                }
                s.playing = !s.playing;
            }
            "cam-pv-back" => {
                s.playing = false;
                let i = s.segment_index();
                let t = if i == 0 {
                    0.0
                } else {
                    s.timeline.end_of(i - 1)
                };
                let t = if (t - s.t).abs() < 1e-12 && i >= 2 {
                    s.timeline.end_of(i - 2)
                } else {
                    t
                };
                s.jump(t);
            }
            "cam-pv-next" => {
                s.playing = false;
                let i = s.segment_index();
                let t = s.timeline.end_of(i);
                let t = if (t - s.t).abs() < 1e-12 {
                    s.timeline.end_of(i + 1)
                } else {
                    t
                };
                s.jump(t);
            }
            "cam-pv-end" => {
                s.playing = false;
                let t = s.total();
                s.jump(t);
            }
            "cam-pv-reset" => {
                s.playing = false;
                s.paused_at = None;
                s.jump(0.0);
            }
            _ => {}
        }
    });
}

fn on_slider(ev: On<SliderChange>, q: Query<&Name>, state: Option<ResMut<PreviewState>>) {
    let Some(mut s) = state else { return };
    let Ok(n) = q.get(ev.entity) else { return };
    match n.as_str() {
        "cam-pv-speed" => s.speed = ev.value.clamp(0.0, 1.0),
        "cam-pv-progress" => {
            let t = s.total() * ev.value.clamp(0.0, 1.0) as f64;
            s.playing = false;
            s.jump(t);
        }
        _ => {}
    }
}

fn on_check(ev: On<CheckboxChange>, q: Query<&Name>, state: Option<ResMut<PreviewState>>) {
    let Some(mut s) = state else { return };
    let Ok(n) = q.get(ev.entity) else { return };
    match n.as_str() {
        "cam-pv-tool" => s.show_tool = ev.checked,
        "cam-pv-path" => s.show_path = ev.checked,
        "cam-pv-part" => s.show_part = ev.checked,
        "cam-pv-pause" => s.pause_on_collision = ev.checked,
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The toolpath (done in colour, left grey) and the tool.
fn draw(
    state: Option<Res<PreviewState>>,
    mut g: Gizmos,
    mut assets: ResMut<Assets<bevy::gizmos::GizmoAsset>>,
    mut slot: Local<Option<(Entity, Handle<bevy::gizmos::GizmoAsset>)>>,
    mut last: Local<Option<(usize, bool, bool)>>,
    mut commands: Commands,
) {
    let Some(s) = state else {
        if let Some((e, _)) = slot.take() {
            commands.entity(e).try_despawn();
        }
        *last = None;
        return;
    };
    let i = s.segment_index();
    // The path: rebuilt when the segment changes.
    let key = (i, s.show_path, s.mode == Mode::BackPlot);
    if *last != Some(key) {
        *last = Some(key);
        let mut asset = bevy::gizmos::GizmoAsset::default();
        if s.show_path {
            let segs = &s.timeline.segments;
            let done = &segs[..(i + 1).min(segs.len())];
            fill_motion(&mut asset, done, &s.frame, s.beam, None);
            if s.mode == Mode::BackPlot {
                let rest = &segs[(i + 1).min(segs.len())..];
                fill_motion(
                    &mut asset,
                    rest,
                    &s.frame,
                    s.beam,
                    Some(Color::srgba(0.6, 0.62, 0.66, 0.45)),
                );
            }
        }
        match &*slot {
            Some((_, h)) => {
                let _ = assets.insert(h.id(), asset);
            }
            None => {
                let h = assets.add(asset);
                let e = commands
                    .spawn((
                        Name::new("cam-preview-path"),
                        bevy::gizmos::retained::Gizmo {
                            handle: h.clone(),
                            line_config: bevy::gizmos::config::GizmoLineConfig {
                                width: 1.8,
                                joints: bevy::gizmos::config::GizmoLineJoint::Round(4),
                                ..default()
                            },
                            depth_bias: -0.0005,
                        },
                        Transform::default(),
                        DespawnOnExit(AppState::Document),
                    ))
                    .id();
                *slot = Some((e, h));
            }
        }
    }
    // The tool where it is now.
    if !s.show_tool {
        return;
    }
    let Some((p, _)) = s.timeline.at(s.t) else {
        return;
    };
    let Some(op) = s.op_at(i) else { return };
    let tool = &op.tool;
    let f = &s.frame;
    let axis = v3(f.dir_to_world(op.axis));
    let tip = v3(f.to_world(p));
    let iso = Isometry3d::new(tip, Quat::from_rotation_arc(Vec3::Z, axis));
    let r = tool.radius() as f32;
    let color = Color::srgb_u8(0x52, 0x5a, 0x66);
    if tool.is_beam() {
        // A beam: a cone of light down to the kerf.
        g.circle(iso, r.max(0.3), Color::srgb_u8(0xff, 0x8a, 0x00));
        g.line(tip, tip + axis * 20.0, Color::srgb_u8(0xff, 0xb0, 0x40));
        return;
    }
    let flute = tool.cutting_length as f32;
    // The cutter's outline: its tip shape, the flutes and the shank to its overall length.
    let n = 8;
    let mut prev: Option<(f32, f32)> = None;
    for k in 0..=n {
        let rr = r * k as f32 / n as f32;
        let h = tool.height_at(rr as f64).unwrap_or(0.0) as f32;
        g.circle(
            Isometry3d::new(tip + axis * h, iso.rotation),
            rr.max(0.05),
            color,
        );
        if let Some((pr, ph)) = prev {
            for a in [0.0f32, 90.0, 180.0, 270.0] {
                let d = iso.rotation * (Quat::from_rotation_z(a.to_radians()) * Vec3::X);
                g.line(tip + axis * ph + d * pr, tip + axis * h + d * rr, color);
            }
        }
        prev = Some((rr, h));
    }
    let top = tip + axis * flute;
    g.circle(Isometry3d::new(top, iso.rotation), r, color);
    for a in [0.0f32, 90.0, 180.0, 270.0] {
        let d = iso.rotation * (Quat::from_rotation_z(a.to_radians()) * Vec3::X);
        g.line(
            tip + axis * prev.map_or(0.0, |x| x.1) + d * r,
            top + d * r,
            color,
        );
    }
    let shank = tip + axis * (tool.overall_length as f32).max(flute);
    g.line(top, shank, color);
    if s.mode == Mode::Simulate
        && let Some(h) = &tool.holder
    {
        // The holder from the end of the stick-out.
        let hc = Color::srgb_u8(0x9a, 0x6b, 0x2e);
        let mut last: Option<[f64; 2]> = None;
        for q in &h.points {
            let c = shank + axis * q[1] as f32;
            g.circle(Isometry3d::new(c, iso.rotation), q[0] as f32, hc);
            if let Some(lp) = last {
                for a in [0.0f32, 90.0, 180.0, 270.0] {
                    let d = iso.rotation * (Quat::from_rotation_z(a.to_radians()) * Vec3::X);
                    g.line(
                        shank + axis * lp[1] as f32 + d * lp[0] as f32,
                        c + d * q[0] as f32,
                        hc,
                    );
                }
            }
            last = Some(*q);
        }
    }
}

#[derive(Component)]
struct VerifyMesh;

/// A cut sheet as Verify shows it: its pieces at the top of the sheet, the parts cut free in
/// the parts' blue, the scrap and skeleton grey; the kerf is the gaps between them.
fn sheet_mesh(
    pieces: &[(cadrs_core::cam::cadrs_cam::region::Region, bool)],
    z: f64,
    frame: &Frame,
) -> Mesh {
    use cadrs_core::cam::cadrs_cam::region;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    for (rings, part) in pieces {
        let c = if *part {
            [0.47, 0.62, 0.78, 1.0]
        } else {
            [0.80, 0.82, 0.86, 1.0]
        };
        for t in region::triangulate(rings) {
            for q in t {
                pos.push(v3(frame.to_world([q[0], q[1], z])).to_array());
                col.push(c);
            }
        }
    }
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
}

/// Verify's stock: the height map as a surface (cut areas shaded by depth, gouges red), built
/// again at most ten times a second while it changes.
#[allow(clippy::too_many_arguments)]
fn sync_stock_mesh(
    state: Option<ResMut<PreviewState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<Entity, With<VerifyMesh>>,
    mut mat: Local<Option<Handle<StandardMaterial>>>,
    mut commands: Commands,
) {
    let Some(mut s) = state else {
        for e in &q {
            commands.entity(e).despawn();
        }
        return;
    };
    let at_end = s.t >= s.total() - 1e-9;
    let sheet = s.beam && s.sheet.is_some() && at_end;
    if s.mode == Mode::BackPlot || (s.map.is_none() && !sheet) {
        for e in &q {
            commands.entity(e).despawn();
        }
        s.mesh_dirty = true;
        return;
    }
    if !s.mesh_dirty && !q.is_empty() {
        return;
    }
    if s.mesh_at.is_some_and(|t| t.elapsed().as_secs_f32() < 0.1) && !q.is_empty() {
        return;
    }
    s.mesh_dirty = false;
    s.mesh_at = Some(Instant::now());
    let mesh = match &s.map {
        Some(map) => height_mesh(map, s.part.as_ref(), s.top, &s.frame),
        // (A hair over the sheet's top, which the part's top face shares.)
        None => sheet_mesh(
            s.sheet.as_deref().unwrap_or_default(),
            s.stock.bounds.max[2] + 0.05,
            &s.frame,
        ),
    };
    let m = mat
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                // The shading is in the vertex colours (the view has no lights for it).
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .clone();
    let handle = meshes.add(mesh);
    match q.iter().next() {
        Some(e) => {
            commands.entity(e).insert(Mesh3d(handle));
        }
        None => {
            commands.spawn((
                Name::new("cam-verify-stock"),
                VerifyMesh,
                Mesh3d(handle),
                MeshMaterial3d(m),
                Transform::default(),
                DespawnOnExit(AppState::Document),
            ));
        }
    }
}

/// A height map as a surface with walls down to its base, coloured by what happened there.
fn height_mesh(map: &HeightMap, part: Option<&HeightMap>, top: f32, frame: &Frame) -> Mesh {
    let (nx, ny) = (map.nx, map.ny);
    // At most about 600 × 600 vertices: a block of cells to a vertex for big maps, at the
    // block's highest (so a wall stays where it is instead of aliasing into spikes).
    let step = ((nx.max(ny) as f32 / 600.0).ceil() as usize).max(1);
    let block = |i: usize, j: usize| -> f32 {
        let mut v = EMPTY;
        for jj in j..(j + step).min(ny) {
            for ii in i..(i + step).min(nx) {
                let c = map.at(ii, jj);
                if c != EMPTY && (v == EMPTY || c > v) {
                    v = c;
                }
            }
        }
        v
    };
    let xs: Vec<usize> = (0..nx).step_by(step).collect();
    let ys: Vec<usize> = (0..ny).step_by(step).collect();
    let base = map
        .z
        .iter()
        .copied()
        .filter(|v| *v != EMPTY)
        .fold(f32::INFINITY, f32::min)
        .min(top)
        - 0.0;
    let stock_c = [0.78f32, 0.80, 0.84, 1.0];
    let mut pos = Vec::with_capacity(xs.len() * ys.len());
    let mut col = Vec::with_capacity(xs.len() * ys.len());
    for &j in &ys {
        for &i in &xs {
            let v = block(i, j);
            let z = if v == EMPTY { base } else { v };
            let q = map.point(i, j);
            pos.push(v3(frame.to_world([q[0], q[1], z as f64])).to_array());
            let gouge = part.is_some_and(|p| {
                let pv = p.at(i, j);
                pv != EMPTY && v != EMPTY && v < pv - (map.cell as f32 * 0.5).max(0.01)
            });
            let c = if gouge {
                [0.86, 0.15, 0.12, 1.0]
            } else if v != EMPTY && v < top - 1e-3 {
                // Cut: darker blue-grey the deeper it goes.
                let d = ((top - v) / (top - base).max(1e-3)).clamp(0.0, 1.0);
                [0.62 - 0.25 * d, 0.70 - 0.22 * d, 0.80 - 0.12 * d, 1.0]
            } else {
                stock_c
            };
            col.push(c);
        }
    }
    let w = xs.len();
    let mut idx = Vec::new();
    for j in 0..ys.len().saturating_sub(1) {
        for i in 0..w.saturating_sub(1) {
            let a = (j * w + i) as u32;
            let (b, c, d) = (a + 1, a + w as u32, a + w as u32 + 1);
            let empty = |k: u32| {
                let (ii, jj) = (xs[k as usize % w], ys[k as usize / w]);
                block(ii, jj) == EMPTY
            };
            if empty(a) || empty(b) || empty(c) || empty(d) {
                continue;
            }
            idx.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
    .with_inserted_indices(bevy::mesh::Indices::U32(idx));
    // Flat shaded: a wall the grid steps round (a hole's) reads as facets, not as the streaks
    // smooth normals draw down a staircase.
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    // Lit from above right, as the parts are: the colour times a diffuse term.
    let light = Vec3::new(0.35, -0.45, 0.82).normalize();
    let normals: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) => {
            n.iter().map(|v| Vec3::from_array(*v)).collect()
        }
        _ => Vec::new(),
    };
    if let Some(bevy::mesh::VertexAttributeValues::Float32x4(cols)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
    {
        for (c, n) in cols.iter_mut().zip(&normals) {
            let k = 0.62 + 0.38 * n.dot(light).max(0.0);
            for v in c.iter_mut().take(3) {
                *v *= k;
            }
        }
    }
    mesh
}
