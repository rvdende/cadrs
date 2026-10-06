//! Executes a [`Scenario`] one frame at a time.
//!
//! Each step expands into per-frame operations (move the pointer, press, release, key down/up,
//! screenshot). They run in `First`, so picking and input systems in `PreUpdate` see them in the
//! same frame. UI targets are looked up by `Name` when the step starts, and retried for a while if
//! the node does not exist yet.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::camera::NormalizedRenderTarget;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;
use cadrs_ui::{CursorState, FinishAnimations, RenderSurface, ScriptCommand};

use crate::keys::{KeySpec, char_key, parse_chord, parse_single};
use crate::scenario::{Scenario, Step, Target};

/// Maps sketch-plane millimetres to screen pixels for `world(x, y)` targets. The app keeps it
/// up to date (the binary copies its view mapping in every frame); `None` until then.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct WorldToScreen(pub Option<Affine>);

/// Maps the sheet metal flat view's flat millimetres to screen pixels for `flat(x, y)`
/// targets. The app keeps it up to date; `None` while the flat view isn't shown.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct FlatToScreen(pub Option<Affine>);

/// Maps 3D world millimetres to screen pixels for `xyz(x, y, z)` targets (the view is
/// orthographic, so this is affine). The app keeps it up to date.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct SpaceToScreen(pub Option<Affine3>);

/// `screen = origin + x·x_axis + y·y_axis + z·z_axis` (logical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine3 {
    pub origin: Vec2,
    pub x_axis: Vec2,
    pub y_axis: Vec2,
    pub z_axis: Vec2,
}

/// `screen = origin + x·x_axis + y·y_axis` (logical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub origin: Vec2,
    pub x_axis: Vec2,
    pub y_axis: Vec2,
}

impl Affine {
    pub fn apply(&self, p: Vec2) -> Vec2 {
        self.origin + self.x_axis * p.x + self.y_axis * p.y
    }
}

/// The picking pointer the harness drives.
///
/// This is the mouse pointer rather than a `PointerId::Custom` one: `bevy_picking` only derives
/// the `Hovered` component (which widgets use for hover styling and tooltips) from the mouse
/// pointer. Without winit nothing else writes to it, so headless runs are unaffected; in a
/// windowed replay, keep the real mouse outside the window.
pub const HARNESS_POINTER: PointerId = PointerId::Mouse;

/// Frames a step may wait for its UI target before the scenario fails.
const TARGET_TIMEOUT: u32 = 300;
/// Frames to wait for a screenshot readback before failing.
const SHOT_TIMEOUT: u32 = 600;
/// Frames a screenshot waits for the app's pending work.
const WORK_TIMEOUT: u32 = 1200;

#[derive(Debug, Clone)]
enum Op {
    Move(Vec2),
    /// Mouse wheel, in lines (positive = away from the user).
    Scroll(f32),
    Press(PointerButton),
    Release(PointerButton),
    Key(KeySpec, ButtonState),
    FinishAnimations,
    Shot(String),
    Custom(String),
    MeasureStart(String),
    MeasureEnd,
}

/// Frame timing between `MeasureStart` and `MeasureEnd`.
struct Measure {
    label: String,
    last: std::time::Instant,
    frames: Vec<f64>,
}

#[derive(Resource)]
pub struct Runner {
    name: String,
    out_dir: PathBuf,
    steps: VecDeque<Step>,
    frames: VecDeque<Vec<Op>>,
    warmup: u32,
    pointer: Vec2,
    retries: u32,
    shot_index: usize,
    pending: Arc<AtomicUsize>,
    pending_frames: u32,
    done: bool,
    /// Draw the software cursor into screenshots.
    cursor: bool,
    /// The pointer has been moved (before that there is no cursor to draw).
    pointer_known: bool,
    measure: Option<Measure>,
    /// Frames left before a step with a 3D or sketch-plane target resolves it, after the
    /// harness finished the view's animations (`None`: not started for this step).
    settle: Option<u8>,
    /// Frames a screenshot has waited for the app's pending work ([`cadrs_ui::PendingWork`]).
    work_frames: u32,
}

pub fn add_runner(app: &mut App, scenario: Scenario, name: String, out_dir: PathBuf) {
    let _ = std::fs::remove_dir_all(&out_dir);
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        panic!("cannot create {}: {e}", out_dir.display());
    }
    app.init_resource::<WorldToScreen>();
    app.init_resource::<SpaceToScreen>();
    app.init_resource::<FlatToScreen>();
    app.insert_resource(Runner {
        name,
        out_dir,
        steps: scenario.steps.into(),
        frames: VecDeque::new(),
        warmup: scenario.warmup,
        pointer: Vec2::ZERO,
        retries: 0,
        shot_index: 0,
        pending: Arc::new(AtomicUsize::new(0)),
        pending_frames: 0,
        done: false,
        cursor: scenario.cursor,
        pointer_known: false,
        measure: None,
        settle: None,
        work_frames: 0,
    })
    .add_systems(First, drive);
}

fn fail(world: &mut World, msg: String) {
    error!("scenario failed: {msg}");
    eprintln!("scenario failed: {msg}");
    world.resource_mut::<Runner>().done = true;
    world.write_message(AppExit::error());
}

fn drive(world: &mut World) {
    let mut runner = world.resource_mut::<Runner>();
    if runner.done {
        return;
    }
    if let Some(m) = runner.measure.as_mut() {
        let now = std::time::Instant::now();
        m.frames.push(now.duration_since(m.last).as_secs_f64() * 1000.0);
        m.last = now;
    }
    if runner.pending.load(Ordering::SeqCst) > 0 {
        runner.pending_frames += 1;
        if runner.pending_frames > SHOT_TIMEOUT {
            fail(world, "screenshot readback timed out".into());
        }
        return;
    }
    runner.pending_frames = 0;
    if runner.warmup > 0 {
        runner.warmup -= 1;
        return;
    }
    if let Some(ops) = runner.frames.pop_front() {
        for op in ops {
            execute(world, op);
        }
        return;
    }
    let Some(step) = runner.steps.pop_front() else {
        let (name, dir, n) = (
            runner.name.clone(),
            runner.out_dir.clone(),
            runner.shot_index,
        );
        runner.done = true;
        info!(
            "scenario {name} finished: {n} screenshot(s) in {}",
            dir.display()
        );
        // Everything the app saved is on disk (saves are synchronous): a relaunch scenario
        // (`data_from`) may now take its documents.
        if let Err(e) = std::fs::write(dir.join(crate::COMPLETE_MARKER), b"") {
            warn!("cannot mark {} complete: {e}", dir.display());
        }
        world.write_message(AppExit::Success);
        return;
    };
    // A screenshot waits for the app's background work (drawing views being projected), a few
    // frames more for what it produced to be drawn, and at most `WORK_TIMEOUT` frames.
    if matches!(step, Step::Screenshot(_)) {
        let busy = world.get_resource::<cadrs_ui::PendingWork>().is_some_and(|w| w.0);
        let mut runner = world.resource_mut::<Runner>();
        if busy && runner.work_frames < WORK_TIMEOUT {
            runner.work_frames += 1;
            runner.steps.push_front(step);
            return;
        }
        if runner.work_frames > 0 && runner.work_frames < WORK_TIMEOUT {
            // Drawn two frames later.
            runner.work_frames = WORK_TIMEOUT + 2;
        }
        if runner.work_frames > WORK_TIMEOUT {
            runner.work_frames -= 1;
            runner.steps.push_front(step);
            return;
        }
        if busy {
            warn!("screenshot taken while work was still pending");
        }
        runner.work_frames = 0;
    }
    // A `world()` or `xyz()` target depends on the view. Camera animations (zoom to fit, view
    // cube turns) run on wall-clock time, so on a loaded machine one may still be running after
    // the scenario's frame waits: finish them first, and resolve the target once the view's
    // mappings have caught up, so a click lands where it would on an idle machine.
    if uses_view(&step) {
        let mut runner = world.resource_mut::<Runner>();
        match runner.settle {
            None => {
                runner.settle = Some(2);
                runner.steps.push_front(step);
                world.write_message(FinishAnimations);
                return;
            }
            Some(n) if n > 0 => {
                runner.settle = Some(n - 1);
                runner.steps.push_front(step);
                return;
            }
            Some(_) => {}
        }
    }
    match expand(world, &step) {
        Ok(frames) => {
            let mut runner = world.resource_mut::<Runner>();
            runner.retries = 0;
            runner.settle = None;
            runner.frames.extend(frames);
        }
        Err(missing) => {
            let mut runner = world.resource_mut::<Runner>();
            runner.retries += 1;
            if runner.retries > TARGET_TIMEOUT {
                if let Step::ExpectText(name, _) = &step {
                    let got = node_text(world, name);
                    if !got.is_empty() {
                        return fail(world, format!("{step:?}: the node reads {got:?}"));
                    }
                }
                let names = ui_names(world);
                fail(
                    world,
                    format!("{step:?}: no visible UI node named {missing:?}. Named nodes: {names}"),
                );
            } else {
                runner.steps.push_front(step);
            }
        }
    }
}

/// Whether the step has a target placed by the current view (`world()` or `xyz()`).
fn uses_view(step: &Step) -> bool {
    let view = |t: &Target| matches!(t, Target::World(..) | Target::Xyz(..) | Target::Flat(..));
    match step {
        Step::Click(t)
        | Step::RightClick(t)
        | Step::DoubleClick(t)
        | Step::Hover(t)
        | Step::Press(t)
        | Step::PressWith(_, t)
        | Step::Scroll(t, _) => view(t),
        Step::Drag(a, b) | Step::RightDrag(a, b) | Step::MiddleDrag(a, b) => view(a) || view(b),
        _ => false,
    }
}

/// Expands a step into per-frame operations. `Err(name)` means a UI target was not found yet.
fn expand(world: &mut World, step: &Step) -> Result<Vec<Vec<Op>>, String> {
    use PointerButton::{Middle, Primary, Secondary};
    let drag = |a: Vec2, b: Vec2, button: PointerButton| {
        let mut f = vec![vec![Op::Move(a)], vec![Op::Press(button)]];
        const N: usize = 8;
        for i in 1..=N {
            f.push(vec![Op::Move(a.lerp(b, i as f32 / N as f32))]);
        }
        f.push(vec![Op::Release(button)]);
        f.push(vec![]);
        f
    };
    let click = |pos: Vec2, b: PointerButton| {
        vec![
            vec![Op::Move(pos)],
            vec![Op::Press(b)],
            vec![Op::Release(b)],
            vec![],
        ]
    };
    Ok(match step {
        Step::Click(t) => click(resolve(world, t)?, Primary),
        Step::ClickAt(x, y) => click(Vec2::new(*x, *y), Primary),
        Step::ClickWorld(x, y) => click(resolve(world, &Target::World(*x, *y))?, Primary),
        Step::MoveWorld(x, y) => vec![
            vec![Op::Move(resolve(world, &Target::World(*x, *y))?)],
            vec![],
        ],
        Step::DragWorld(x0, y0, x1, y1) => drag(
            resolve(world, &Target::World(*x0, *y0))?,
            resolve(world, &Target::World(*x1, *y1))?,
            Primary,
        ),
        Step::RightClick(t) => click(resolve(world, t)?, Secondary),
        Step::DoubleClick(t) => {
            let pos = resolve(world, t)?;
            vec![
                vec![Op::Move(pos)],
                vec![Op::Press(Primary)],
                vec![Op::Release(Primary)],
                vec![Op::Press(Primary)],
                vec![Op::Release(Primary)],
                vec![],
            ]
        }
        Step::Hover(t) => vec![vec![Op::Move(resolve(world, t)?)], vec![]],
        Step::MoveTo(x, y) => vec![vec![Op::Move(Vec2::new(*x, *y))], vec![]],
        Step::Press(t) => vec![
            vec![Op::Move(resolve(world, t)?)],
            vec![Op::Press(Primary)],
            vec![],
        ],
        Step::Release => vec![vec![Op::Release(Primary)], vec![]],
        Step::PressWith(b, t) => {
            let button = match parse_button(b) {
                Ok(b) => b,
                Err(e) => {
                    fail(world, e);
                    return Ok(vec![]);
                }
            };
            vec![vec![Op::Move(resolve(world, t)?)], vec![Op::Press(button)], vec![]]
        }
        Step::ReleaseWith(b) => {
            let button = match parse_button(b) {
                Ok(b) => b,
                Err(e) => {
                    fail(world, e);
                    return Ok(vec![]);
                }
            };
            vec![vec![Op::Release(button)], vec![]]
        }
        Step::Drag(a, b) => drag(resolve(world, a)?, resolve(world, b)?, Primary),
        Step::RightDrag(a, b) => drag(resolve(world, a)?, resolve(world, b)?, Secondary),
        Step::MiddleDrag(a, b) => drag(resolve(world, a)?, resolve(world, b)?, Middle),
        Step::Scroll(t, lines) => vec![
            vec![Op::Move(resolve(world, t)?)],
            vec![Op::Scroll(*lines)],
            vec![],
        ],
        Step::KeyDown(k) | Step::KeyUp(k) => {
            let key = match parse_single(k) {
                Ok(k) => k,
                Err(e) => {
                    fail(world, e);
                    return Ok(vec![]);
                }
            };
            let state = if matches!(step, Step::KeyDown(_)) {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            vec![vec![Op::Key(key, state)], vec![]]
        }
        Step::Type(text) => text
            .chars()
            .map(|c| {
                let k = char_key(c);
                vec![
                    Op::Key(k.clone(), ButtonState::Pressed),
                    Op::Key(k, ButtonState::Released),
                ]
            })
            .chain([vec![]])
            .collect(),
        Step::Key(chord) => {
            let chord = match parse_chord(chord) {
                Ok(c) => c,
                Err(e) => {
                    fail(world, e);
                    return Ok(vec![]);
                }
            };
            let mut f = Vec::new();
            if !chord.modifiers.is_empty() {
                f.push(
                    chord
                        .modifiers
                        .iter()
                        .map(|m| Op::Key(m.clone(), ButtonState::Pressed))
                        .collect(),
                );
            }
            f.push(vec![Op::Key(chord.key.clone(), ButtonState::Pressed)]);
            f.push(vec![Op::Key(chord.key, ButtonState::Released)]);
            if !chord.modifiers.is_empty() {
                f.push(
                    chord
                        .modifiers
                        .iter()
                        .map(|m| Op::Key(m.clone(), ButtonState::Released))
                        .collect(),
                );
            }
            f.push(vec![]);
            f
        }
        Step::Wait(n) => vec![vec![]; *n as usize],
        Step::WaitFor(name) => {
            resolve(world, &Target::Ui(name.clone()))?;
            vec![vec![]]
        }
        Step::ExpectText(name, want) => {
            let got = node_text(world, name);
            if !got.iter().any(|t| t.contains(want.as_str())) {
                return Err(name.clone());
            }
            vec![vec![]]
        }
        Step::AssertEnabled(name) => {
            resolve(world, &Target::Ui(name.clone()))?;
            let mut q = world.query::<(&Name, bevy::ecs::query::Has<bevy::ui::InteractionDisabled>)>();
            if q.iter(world).any(|(n, disabled)| n.as_str() == name && disabled) {
                return Err(format!("{name} is disabled"));
            }
            vec![vec![]]
        }
        Step::Custom(cmd) => vec![vec![Op::Custom(cmd.clone())], vec![]],
        Step::MeasureStart(label) => vec![vec![Op::MeasureStart(label.clone())]],
        Step::MeasureEnd => vec![vec![Op::MeasureEnd], vec![]],
        Step::Screenshot(label) => vec![
            vec![Op::FinishAnimations],
            vec![],
            vec![],
            vec![Op::Shot(label.clone())],
        ],
    })
}

fn parse_button(name: &str) -> Result<PointerButton, String> {
    match name.to_ascii_lowercase().as_str() {
        "left" | "primary" => Ok(PointerButton::Primary),
        "right" | "secondary" => Ok(PointerButton::Secondary),
        "middle" => Ok(PointerButton::Middle),
        other => Err(format!("unknown mouse button {other:?}")),
    }
}

/// The logical position of a target: the center of a visible UI node, or a fixed point.
fn resolve(world: &mut World, target: &Target) -> Result<Vec2, String> {
    match target {
        Target::At(x, y) => Ok(Vec2::new(*x, *y)),
        Target::World(x, y) => {
            let map = world.get_resource::<WorldToScreen>().copied().unwrap_or_default();
            // Whole pixels, like a real mouse.
            map.0
                .map(|m| m.apply(Vec2::new(*x, *y)).round())
                .ok_or_else(|| "world(..): no sketch plane mapping".to_string())
        }
        Target::Flat(x, y) => {
            let map = world.get_resource::<FlatToScreen>().copied().unwrap_or_default();
            map.0.map(|m| m.apply(Vec2::new(*x, *y)).round()).ok_or_else(|| "flat(..): the flat view isn't shown".to_string())
        }
        Target::Xyz(x, y, z) => {
            let map = world.get_resource::<SpaceToScreen>().copied().unwrap_or_default();
            map.0
                .map(|m| (m.origin + m.x_axis * *x + m.y_axis * *y + m.z_axis * *z).round())
                .ok_or_else(|| "xyz(..): no view mapping".to_string())
        }
        Target::UiAt(name, fx, fy) => {
            let mut q = world.query::<(&Name, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>();
            q.iter(world)
                .find(|(n, node, _, vis)| n.as_str() == name && vis.get() && node.size().x > 0.0 && node.size().y > 0.0)
                .map(|(_, node, t, _)| {
                    let scale = node.inverse_scale_factor();
                    let size = node.size() * scale;
                    (t.translation * scale + Vec2::new(fx - 0.5, fy - 0.5) * size).round()
                })
                .ok_or_else(|| name.clone())
        }
        Target::Ui(name) => {
            let mut q = world.query::<(
                Entity,
                &Name,
                &ComputedNode,
                &UiGlobalTransform,
                &InheritedVisibility,
            )>();
            let found: Vec<(Entity, Vec2)> = q
                .iter(world)
                .filter(|(_, n, node, _, vis)| {
                    n.as_str() == name && vis.get() && node.size().x > 0.0 && node.size().y > 0.0
                })
                .map(|(e, _, node, t, _)| (e, t.translation * node.inverse_scale_factor()))
                .collect();
            if found.len() > 1 {
                warn!(
                    "{} UI nodes are named {name:?}; using the first",
                    found.len()
                );
            }
            let (e, pos) = found.first().copied().ok_or_else(|| name.clone())?;
            // P3E.3a: a node scrolled out of its list is scrolled into view first (as a user
            // would), and looked up again once the layout has moved it.
            if scroll_into_view(world, e, pos) {
                return Err(name.clone());
            }
            Ok(pos)
        }
    }
}

/// If the point `pos` (logical px) of the node `e` lies outside a scrolling ancestor's visible
/// box, scrolls that ancestor so the point is inside it and returns true.
fn scroll_into_view(world: &mut World, e: Entity, pos: Vec2) -> bool {
    let mut cur = e;
    loop {
        // A node placed absolutely (a popup, a dialog over a list) isn't scrolled by the lists
        // it sits in.
        if world.get::<Node>(cur).is_some_and(|n| n.position_type == PositionType::Absolute) {
            return false;
        }
        let Some(parent) = world.get::<ChildOf>(cur).map(|c| c.parent()) else { return false };
        cur = parent;
        let Some(node) = world.get::<Node>(cur) else { continue };
        let (sx, sy) = (node.overflow.x == OverflowAxis::Scroll, node.overflow.y == OverflowAxis::Scroll);
        if !sx && !sy {
            continue;
        }
        let (Some(c), Some(t)) = (world.get::<ComputedNode>(cur), world.get::<UiGlobalTransform>(cur)) else { continue };
        let k = c.inverse_scale_factor();
        let (center, half) = (t.translation * k, c.size() * k / 2.0);
        let margin = 6.0;
        let mut delta = Vec2::ZERO;
        for (i, on) in [(0usize, sx), (1, sy)] {
            if !on {
                continue;
            }
            let (lo, hi) = (center[i] - half[i] + margin, center[i] + half[i] - margin);
            if pos[i] < lo {
                delta[i] = pos[i] - lo - margin;
            } else if pos[i] > hi {
                delta[i] = pos[i] - hi + margin;
            }
        }
        if delta == Vec2::ZERO {
            continue;
        }
        // How far it can scroll (logical px): a list already at its end is left alone.
        let max = ((c.content_size - c.size) * k).max(Vec2::ZERO);
        if let Some(mut s) = world.get_mut::<ScrollPosition>(cur) {
            let to = (s.0 + delta).clamp(Vec2::ZERO, max);
            if (to - s.0).length() < 0.5 {
                return false;
            }
            s.0 = to;
            return true;
        }
        return false;
    }
}

/// The text of each visible UI node with this name: its own and its descendants' `Text`,
/// `TextSpan` and text field values, joined with spaces.
fn node_text(world: &mut World, name: &str) -> Vec<String> {
    let mut q = world.query::<(Entity, &Name, &InheritedVisibility)>();
    let roots: Vec<Entity> = q
        .iter(world)
        .filter(|(_, n, vis)| n.as_str() == name && vis.get())
        .map(|(e, ..)| e)
        .collect();
    roots
        .into_iter()
        .map(|root| {
            let mut parts = Vec::new();
            let mut stack = vec![root];
            while let Some(e) = stack.pop() {
                let Ok(r) = world.get_entity(e) else { continue };
                if let Some(t) = r.get::<Text>() {
                    parts.push(t.0.clone());
                }
                if let Some(t) = r.get::<TextSpan>() {
                    parts.push(t.0.clone());
                }
                if let Some(t) = r.get::<bevy::text::EditableText>() {
                    parts.push(t.value().to_string());
                }
                if let Some(c) = r.get::<Children>() {
                    stack.extend(c.iter().rev());
                }
            }
            parts.join(" ")
        })
        .collect()
}

fn ui_names(world: &mut World) -> String {
    let mut q = world.query_filtered::<&Name, With<ComputedNode>>();
    let mut names: Vec<String> = q.iter(world).map(|n| n.as_str().to_string()).collect();
    names.sort();
    names.dedup();
    names.join(", ")
}

fn pointer_target(world: &mut World) -> Option<NormalizedRenderTarget> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next();
    world.resource::<RenderSurface>().target.normalize(window)
}

fn execute(world: &mut World, op: Op) {
    match op {
        Op::Move(pos) => {
            let Some(target) = pointer_target(world) else {
                return fail(world, "render surface has no target".into());
            };
            let mut runner = world.resource_mut::<Runner>();
            let delta = pos - runner.pointer;
            runner.pointer = pos;
            runner.pointer_known = true;
            world.write_message(PointerInput::new(
                HARNESS_POINTER,
                Location {
                    target,
                    position: pos,
                },
                PointerAction::Move { delta },
            ));
        }
        Op::Scroll(lines) => {
            let Some(target) = pointer_target(world) else {
                return fail(world, "render surface has no target".into());
            };
            let position = world.resource::<Runner>().pointer;
            world.write_message(PointerInput::new(
                HARNESS_POINTER,
                Location { target, position },
                PointerAction::Scroll {
                    unit: bevy::input::mouse::MouseScrollUnit::Line,
                    x: 0.0,
                    y: lines,
                    phase: bevy::input::touch::TouchPhase::Moved,
                },
            ));
        }
        Op::Press(b) | Op::Release(b) => {
            let Some(target) = pointer_target(world) else {
                return fail(world, "render surface has no target".into());
            };
            let position = world.resource::<Runner>().pointer;
            let action = if matches!(op, Op::Press(_)) {
                PointerAction::Press(b)
            } else {
                PointerAction::Release(b)
            };
            world.write_message(PointerInput::new(
                HARNESS_POINTER,
                Location { target, position },
                action,
            ));
        }
        Op::Key(k, state) => {
            let window = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .iter(world)
                .next()
                .unwrap_or(Entity::PLACEHOLDER);
            let text = if state == ButtonState::Pressed {
                k.text.map(Into::into)
            } else {
                None
            };
            world.write_message(KeyboardInput {
                key_code: k.code,
                logical_key: k.logical,
                state,
                text,
                repeat: false,
                window,
            });
        }
        Op::FinishAnimations => {
            world.write_message(FinishAnimations);
        }
        Op::Custom(cmd) => {
            world.write_message(ScriptCommand(cmd));
        }
        Op::MeasureStart(label) => {
            world.resource_mut::<Runner>().measure = Some(Measure {
                label,
                last: std::time::Instant::now(),
                frames: Vec::new(),
            });
        }
        Op::MeasureEnd => {
            let (m, dir) = {
                let mut runner = world.resource_mut::<Runner>();
                (runner.measure.take(), runner.out_dir.clone())
            };
            if let Some(m) = m {
                let report = perf_report(&m.frames);
                let path = dir.join(format!("perf-{}.txt", m.label));
                info!("perf {}: {report}", m.label);
                println!("perf {}: {report}", m.label);
                if let Err(e) = std::fs::write(&path, format!("{report}\n")) {
                    error!("cannot write {}: {e}", path.display());
                }
            }
        }
        Op::Shot(label) => {
            let cursor = {
                let runner = world.resource::<Runner>();
                (runner.cursor && runner.pointer_known).then_some(runner.pointer)
            }
            .map(|p| {
                let kind = world
                    .get_resource::<CursorState>()
                    .map(|c| c.kind)
                    .unwrap_or_default();
                (kind, p)
            });
            let (path, pending) = {
                let mut runner = world.resource_mut::<Runner>();
                runner.shot_index += 1;
                let file = if label.starts_with(|c: char| c.is_ascii_digit()) {
                    format!("{label}.png")
                } else {
                    format!("{:02}-{label}.png", runner.shot_index)
                };
                (runner.out_dir.join(file), runner.pending.clone())
            };
            pending.fetch_add(1, Ordering::SeqCst);
            let target = world.resource::<RenderSurface>().target.clone();
            world
                .spawn(Screenshot(target))
                .observe(move |shot: On<ScreenshotCaptured>| {
                    match shot.image.clone().try_into_dynamic() {
                        Ok(img) => match {
                            let mut rgb = img.to_rgb8();
                            if let Some((kind, p)) = cursor {
                                crate::cursor::draw_cursor(&mut rgb, kind, (p.x, p.y));
                            }
                            rgb
                        }
                        .save(&path)
                        {
                            Ok(()) => info!("saved {}", path.display()),
                            Err(e) => error!("cannot save {}: {e}", path.display()),
                        },
                        Err(e) => error!("cannot convert screenshot: {e:?}"),
                    }
                    pending.fetch_sub(1, Ordering::SeqCst);
                });
        }
    }
}

/// "frames 120, mean 4.21 ms, median 4.10 ms, p95 5.02 ms, max 6.80 ms (237 fps)".
pub fn perf_report(frames: &[f64]) -> String {
    if frames.is_empty() {
        return "no frames".into();
    }
    let mut v = frames.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let pick = |q: f64| v[((n - 1) as f64 * q).round() as usize];
    format!(
        "frames {n}, mean {mean:.2} ms, median {:.2} ms, p95 {:.2} ms, max {:.2} ms ({:.0} fps)",
        pick(0.5),
        pick(0.95),
        v[n - 1],
        1000.0 / mean
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_report_statistics() {
        let r = perf_report(&[10.0, 20.0, 30.0, 40.0]);
        assert!(r.starts_with("frames 4, mean 25.00 ms"), "{r}");
        assert!(r.contains("max 40.00 ms"), "{r}");
        assert_eq!(perf_report(&[]), "no frames");
    }
}
