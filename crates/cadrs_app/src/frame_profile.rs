//! Where a frame's time goes: the CPU time of each schedule and system, from Bevy's tracing
//! spans (the `bevy/trace` feature makes a span for every system and schedule run;
//! [`profiler_layer`] adds them up), and the GPU time of each render pass, from Bevy's render
//! diagnostics (timestamp queries), and the line vertices each gizmo group sent to the GPU
//! (gizmos are drawn anew every frame). [`FrameProfile`] keeps the last second of frames; the frame
//! panel ([`crate::frame_stats`]) draws it as bars and `get_frame_stats` returns it over MCP.
//!
//! Systems run in parallel on several threads, so their times can add up to more than the
//! frame; the schedules on the main thread add up to about the frame.
//!
//! The CPU times are off unless wanted ([`CpuProfiling`]): the spans cost about a fifth of each
//! frame, and a run decides as it starts (Bevy makes a system's span once, with the system).
//! *Preferences › Time systems* turns them on from the next start; `CADRS_FRAME_PROFILE=1` for one
//! run.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use web_time::Instant;

use bevy::diagnostic::DiagnosticsStore;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::gizmos::{GizmoAsset, GizmoHandles};
use bevy::log::BoxedLayer;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::span::{Attributes, Id};
use bevy::log::tracing::{Metadata, Subscriber};
use bevy::log::tracing_subscriber::Layer;
use bevy::log::tracing_subscriber::layer::Context;
use bevy::log::tracing_subscriber::registry::LookupSpan;
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use serde_json::{Value, json};

/// How far back the profile looks.
const WINDOW_SECS: f64 = 1.0;
/// GPU passes not measured for this long have stopped running.
const GPU_STALE_SECS: f32 = 1.0;

/// The schedules the main loop runs one after another each frame (`Main`'s, then the render
/// world's extract): their bars add up to about the frame.
pub const MAIN_SCHEDULES: &[&str] = &[
    "First",
    "PreUpdate",
    "StateTransition",
    "RunFixedMainLoop",
    "Update",
    "SpawnScene",
    "PostUpdate",
    "Last",
    "ExtractSchedule",
];
/// The render thread's schedules (it renders the frame before while the main loop runs; the
/// extract waits for it). `RenderGraph` runs inside `Render`.
pub const RENDER_SCHEDULES: &[&str] = &["Render"];

pub struct FrameProfilePlugin;

impl Plugin for FrameProfilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameProfile>()
            .init_resource::<CpuProfiling>()
            .add_systems(First, take_frame);
        if !app.is_plugin_added::<RenderDiagnosticsPlugin>() {
            app.add_plugins(RenderDiagnosticsPlugin);
        }
    }
}

/// Whether this run times its systems and schedules. The app inserts it before its plugins:
/// [`profiler_layer`] turns every span off unless it's [`CpuProfiling::On`], and a span made
/// off (a system's, as the system is built) stays off.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CpuProfiling {
    /// Built without the `profiler` feature (`bevy/trace`): there are no spans to time.
    #[default]
    NotBuilt,
    Off,
    On,
}

impl CpuProfiling {
    /// For a build with the `profiler` feature or without: on when `wanted` (the preference, a
    /// scenario's `profile_cpu`), with `CADRS_FRAME_PROFILE=1`, or with `CADRS_SLOW_FRAMES`
    /// (which lists the slowest systems). `CADRS_FRAME_PROFILE=0` keeps it off.
    pub fn at_start(built: bool, wanted: bool) -> Self {
        if !built {
            return Self::NotBuilt;
        }
        let on = match std::env::var("CADRS_FRAME_PROFILE").ok().as_deref() {
            Some("0") => false,
            Some(_) => true,
            None => {
                wanted
                    || std::env::var_os("CADRS_SLOW_FRAMES").is_some()
                    || std::env::var_os("CADRS_TRACE").is_some()
            }
        };
        if on { Self::On } else { Self::Off }
    }

    /// Why there are no CPU times, if there aren't.
    pub fn why_not(self) -> Option<&'static str> {
        match self {
            Self::NotBuilt => Some("not in this build"),
            Self::Off => Some("off. Preferences › Time systems turns them on from the next start"),
            Self::On => None,
        }
    }
}

/// What a span measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Schedule,
    System,
    /// Applying a system's commands (and the observers they trigger).
    Commands,
}

/// What the layer adds up between two frames for one span name: the total time (ns) and the
/// runs. Every span of that name shares it, so a run adds to it without a lock (thousands of
/// spans run a frame, on every thread).
#[derive(Default)]
struct Slot {
    ns: AtomicU64,
    runs: AtomicU32,
}

type Slots = HashMap<(Kind, Arc<str>), Arc<Slot>>;

/// The layer's slots, which [`take_frame`] empties each frame.
#[derive(Resource, Clone, Default)]
pub struct ProfilerTally(Arc<Mutex<Slots>>);

/// The [`bevy::log::LogPlugin::custom_layer`] that times system and schedule spans (none
/// without the `bevy/trace` feature: the profile then has only the GPU passes). Unless the
/// app's [`CpuProfiling`] is on, it turns every span off instead, so they cost next to nothing.
pub fn profiler_layer(app: &mut App) -> Option<BoxedLayer> {
    let tally = ProfilerTally::default();
    app.insert_resource(tally.clone());
    let on = app.world().get_resource::<CpuProfiling>() == Some(&CpuProfiling::On);
    let trace = std::env::var_os("CADRS_TRACE").map(|path| {
        let at = std::env::var("CADRS_TRACE_AT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);
        Arc::new(Trace {
            path: path.into(),
            from_ns: (at * 1e9) as u64,
            to_ns: ((at + TRACE_SECS) * 1e9) as u64,
            events: Mutex::default(),
            written: AtomicBool::new(false),
        })
    });
    Some(Box::new(ProfilerLayer {
        slots: tally.0,
        on,
        trace,
    }))
}

struct ProfilerLayer {
    slots: Arc<Mutex<Slots>>,
    /// Off, no span is made at all (the layer's `enabled` turns them away for every layer).
    on: bool,
    trace: Option<Arc<Trace>>,
}

/// How long `CADRS_TRACE` records.
const TRACE_SECS: f64 = 0.25;

/// `CADRS_TRACE=<file.json>`: every span of a quarter second from `CADRS_TRACE_AT` seconds after
/// the start (10 by default), written as a Chrome trace (ui.perfetto.dev, chrome://tracing):
/// which thread ran each system when, so where a frame waits shows.
struct Trace {
    path: std::path::PathBuf,
    from_ns: u64,
    to_ns: u64,
    /// Kind, name, thread (its name), start and length (ns).
    events: Mutex<Vec<(Kind, Arc<str>, Arc<str>, u64, u64)>>,
    written: AtomicBool,
}

impl Trace {
    fn write(&self) {
        let Ok(events) = self.events.lock() else {
            return;
        };
        let mut threads: Vec<Arc<str>> = events.iter().map(|e| e.2.clone()).collect();
        threads.sort();
        threads.dedup();
        let tid = |t: &Arc<str>| threads.binary_search(t).unwrap_or(0);
        let mut out: Vec<Value> = threads
            .iter()
            .enumerate()
            .map(|(i, t)| json!({ "name": "thread_name", "ph": "M", "pid": 1, "tid": i, "args": { "name": t } }))
            .collect();
        out.extend(events.iter().map(|(kind, name, thread, start, len)| {
            json!({
                "name": name,
                "cat": format!("{kind:?}"),
                "ph": "X",
                "pid": 1,
                "tid": tid(thread),
                "ts": *start as f64 / 1e3,
                "dur": *len as f64 / 1e3,
            })
        }));
        let text = json!({ "traceEvents": out }).to_string();
        match std::fs::write(&self.path, text) {
            Ok(()) => info!(
                "trace of {} spans written to {}",
                events.len(),
                self.path.display()
            ),
            Err(e) => warn!("cannot write the trace to {}: {e}", self.path.display()),
        }
    }
}

thread_local! {
    /// This thread's name for `CADRS_TRACE` (its name and id: the pools' threads share names).
    static THREAD: Arc<str> = {
        let t = std::thread::current();
        format!("{} {:?}", t.name().unwrap_or("?"), t.id()).into()
    };
}

/// A timed span's slot, kept on the span, and when it was last entered ([`now_ns`]).
struct Timed {
    slot: Arc<Slot>,
    entered: AtomicU64,
    kind: Kind,
    name: Arc<str>,
}

/// Nanoseconds since the first call.
fn now_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Reads a span's `name` field.
#[derive(Default)]
struct NameField(Option<String>);

impl Visit for NameField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for ProfilerLayer {
    // (Its `register_callsite` follows this: a span's callsite is turned off for good.)
    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        self.on || !metadata.is_span()
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let kind = match attrs.metadata().name() {
            "system" => Kind::System,
            "schedule" => Kind::Schedule,
            "system_commands" => Kind::Commands,
            _ => return,
        };
        let mut name = NameField::default();
        attrs.record(&mut name);
        let (Some(name), Some(span)) = (name.0, ctx.span(id)) else {
            return;
        };
        // Systems' spans are made once, schedules' once a run: the lock is only taken here.
        let Ok(mut slots) = self.slots.lock() else {
            return;
        };
        let name: Arc<str> = name.into();
        let slot = slots.entry((kind, name.clone())).or_default().clone();
        drop(slots);
        span.extensions_mut().insert(Timed {
            slot,
            entered: AtomicU64::new(0),
            kind,
            name,
        });
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(t) = span.extensions().get::<Timed>()
        {
            t.entered.store(now_ns(), Ordering::Relaxed);
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(t) = span.extensions().get::<Timed>()
        {
            let (now, entered) = (now_ns(), t.entered.load(Ordering::Relaxed));
            let ns = now.saturating_sub(entered);
            t.slot.ns.fetch_add(ns, Ordering::Relaxed);
            t.slot.runs.fetch_add(1, Ordering::Relaxed);
            if let Some(trace) = &self.trace {
                if entered >= trace.from_ns && now <= trace.to_ns {
                    if let Ok(mut events) = trace.events.lock() {
                        let thread = THREAD.with(|t| t.clone());
                        events.push((t.kind, t.name.clone(), thread, entered, ns));
                    }
                } else if now > trace.to_ns && !trace.written.swap(true, Ordering::Relaxed) {
                    trace.write();
                }
            }
        }
    }
}

/// One frame's times.
#[derive(Debug, Clone, Default)]
pub struct FrameSample {
    /// When it ended (s since start) and how long it took (ms).
    pub at: f64,
    pub frame_ms: f32,
    /// CPU spans: kind, name, ms, runs.
    pub cpu: Vec<(Kind, Arc<str>, f32, u32)>,
    /// GPU passes measured: path below `render/`, ms.
    pub gpu: Vec<(String, f32)>,
    /// Gizmo groups that drew: type path, line vertices.
    pub gizmos: Vec<(String, u32)>,
}

/// The last [`WINDOW_SECS`] of frames.
#[derive(Resource, Debug, Default)]
pub struct FrameProfile {
    frames: VecDeque<FrameSample>,
}

/// One row of a [`Summary`]: a span's mean time per frame over the window, its most in one
/// frame, and its runs per frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub ms: f32,
    pub max_ms: f32,
    pub runs: f32,
}

/// [`FrameProfile`] averaged over its window, each list slowest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub frames: usize,
    pub frame_ms: f32,
    /// [`MAIN_SCHEDULES`] that ran, in the loop's order.
    pub main: Vec<Row>,
    /// [`RENDER_SCHEDULES`] that ran.
    pub render: Vec<Row>,
    pub schedules: Vec<Row>,
    pub systems: Vec<Row>,
    pub gpu: Vec<Row>,
    /// Gizmo groups by line vertices a frame (in `ms`: a count, not a time).
    pub gizmos: Vec<Row>,
}

impl FrameProfile {
    /// The newest frame.
    pub fn last(&self) -> Option<&FrameSample> {
        self.frames.back()
    }

    pub fn push(&mut self, sample: FrameSample) {
        let now = sample.at;
        self.frames.push_back(sample);
        // At least the last frame stays, however long it took.
        while self.frames.len() > 1
            && self
                .frames
                .front()
                .is_some_and(|f| now - f.at > WINDOW_SECS)
        {
            self.frames.pop_front();
        }
    }

    pub fn summary(&self) -> Summary {
        let n = self.frames.len();
        if n == 0 {
            return Summary::default();
        }
        let rows = |items: &mut dyn Iterator<Item = (String, f32, u32)>| {
            let mut by: HashMap<String, (f32, f32, u32)> = HashMap::new();
            for (name, ms, runs) in items {
                let e = by.entry(name).or_default();
                e.0 += ms;
                e.1 = e.1.max(ms);
                e.2 += runs;
            }
            let mut rows: Vec<Row> = by
                .into_iter()
                .map(|(name, (sum, max_ms, runs))| Row {
                    name,
                    ms: sum / n as f32,
                    max_ms,
                    runs: runs as f32 / n as f32,
                })
                .collect();
            rows.sort_by(|a, b| b.ms.total_cmp(&a.ms).then_with(|| a.name.cmp(&b.name)));
            rows
        };
        let cpu = |kind: Kind| {
            let mut it = self.frames.iter().flat_map(move |f| {
                f.cpu
                    .iter()
                    .filter(move |c| c.0 == kind)
                    .map(|c| (c.1.to_string(), c.2, c.3))
            });
            rows(&mut it)
        };
        let schedules = cpu(Kind::Schedule);
        let pick = |names: &[&str]| {
            names
                .iter()
                .filter_map(|s| schedules.iter().find(|r| r.name == *s).cloned())
                .collect()
        };
        let (main, render) = (pick(MAIN_SCHEDULES), pick(RENDER_SCHEDULES));
        let mut gpu = self
            .frames
            .iter()
            .flat_map(|f| f.gpu.iter().map(|(p, ms)| (p.clone(), *ms, 1)));
        let mut gizmos = self
            .frames
            .iter()
            .flat_map(|f| f.gizmos.iter().map(|(g, n)| (g.clone(), *n as f32, 1)));
        Summary {
            frames: n,
            frame_ms: self.frames.iter().map(|f| f.frame_ms).sum::<f32>() / n as f32,
            main,
            render,
            systems: cpu(Kind::System),
            schedules,
            gpu: rows(&mut gpu),
            gizmos: rows(&mut gizmos),
        }
    }

    /// What `get_frame_stats` returns about the frame's parts: the `top` slowest systems.
    pub fn describe(&self, top: usize, cpu: CpuProfiling) -> Value {
        let s = self.summary();
        let r = |x: f32| (x as f64 * 100.0).round() / 100.0;
        let rows = |rows: &[Row], top: usize| -> Vec<Value> {
            rows.iter()
                .take(top)
                .map(|w| json!({ "name": w.name, "ms": r(w.ms), "max_ms": r(w.max_ms), "runs": r(w.runs) }))
                .collect()
        };
        json!({
            "window_frames": s.frames,
            "cpu": {
                "timed": cpu == CpuProfiling::On,
                "why_not": cpu.why_not().map(|w| match cpu {
                    CpuProfiling::Off => format!("{w} (or start cadrs with CADRS_FRAME_PROFILE=1)"),
                    _ => w.to_string(),
                }),
                "main_loop": rows(&s.main, usize::MAX),
                "render_thread": rows(&s.render, usize::MAX),
                "schedules": rows(&s.schedules, top),
                "systems": rows(&s.systems, top),
            },
            "gpu": {
                "measured": !s.gpu.is_empty(),
                "passes": rows(&s.gpu, top),
            },
            "gizmo_vertices": {
                "total": s.gizmos.iter().map(|g| g.ms as f64).sum::<f64>().round(),
                "groups": s.gizmos.iter().take(top).map(|g| json!({ "group": g.name, "vertices": g.ms.round() })).collect::<Vec<_>>(),
            },
        })
    }
}

/// A system's name for the panel: no generics, the last two parts of its path.
pub fn short_name(name: &str) -> String {
    let mut plain = String::with_capacity(name.len());
    let mut depth = 0usize;
    for ch in name.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(ch),
            _ => {}
        }
    }
    let parts: Vec<&str> = plain.split("::").collect();
    parts[parts.len().saturating_sub(2)..].join("::")
}

/// Ends the frame's tally: the spans timed since the last frame and the GPU passes measured.
#[allow(clippy::too_many_arguments)]
pub(crate) fn take_frame(
    time: Res<Time<Real>>,
    tally: Option<Res<ProfilerTally>>,
    store: Option<Res<DiagnosticsStore>>,
    gizmo_handles: Option<Res<GizmoHandles>>,
    gizmo_configs: Option<Res<GizmoConfigStore>>,
    gizmo_assets: Option<Res<Assets<GizmoAsset>>>,
    mut profile: ResMut<FrameProfile>,
) {
    let cpu = tally
        .and_then(|t| {
            let slots = t.0.lock().ok()?;
            Some(
                slots
                    .iter()
                    .filter_map(|((kind, name), slot)| {
                        let runs = slot.runs.swap(0, Ordering::Relaxed);
                        let ns = slot.ns.swap(0, Ordering::Relaxed);
                        (runs > 0).then(|| (*kind, name.clone(), ns as f32 / 1e6, runs))
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let now = Instant::now();
    let gpu = store
        .map(|s| {
            s.iter()
                .filter_map(|d| {
                    let path = d
                        .path()
                        .as_str()
                        .strip_prefix("render/")?
                        .strip_suffix("/elapsed_gpu")?;
                    let m = d.measurement()?;
                    (now.saturating_duration_since(m.time).as_secs_f32() < GPU_STALE_SECS)
                        .then(|| (path.to_string(), m.value as f32))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut gizmos = Vec::new();
    if let (Some(handles), Some(configs), Some(assets)) =
        (gizmo_handles, gizmo_configs, gizmo_assets)
    {
        for (id, handle) in handles.handles() {
            let Some(asset) = handle.as_ref().and_then(|h| assets.get(h)) else {
                continue;
            };
            let b = asset.buffer();
            let n = (b.list_positions.len() + b.strip_positions.len()) as u32;
            if n > 0 {
                // (Named only when drawing: naming all 70 groups each frame cost a string each.)
                let name = configs
                    .iter()
                    .find(|(c, ..)| *c == id)
                    .map_or("?".into(), |(.., group)| {
                        group.reflect_type_path().to_string()
                    });
                gizmos.push((name, n));
            }
        }
    }
    profile.push(FrameSample {
        at: time.elapsed_secs_f64(),
        frame_ms: time.delta_secs() * 1000.0,
        cpu,
        gpu,
        gizmos,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_drop_generics_and_crates() {
        assert_eq!(
            short_name("cadrs_app::plane_display::place_plane_feature_labels"),
            "plane_display::place_plane_feature_labels"
        );
        assert_eq!(
            short_name("bevy_ecs::event::update<bevy_window::WindowResized>"),
            "event::update"
        );
        assert_eq!(short_name("tick"), "tick");
    }

    #[test]
    fn the_layer_adds_up_the_runs_of_each_name() {
        use bevy::log::tracing_subscriber::layer::SubscriberExt;
        let tally = ProfilerTally::default();
        let subscriber = bevy::log::tracing_subscriber::registry().with(ProfilerLayer {
            slots: tally.0.clone(),
            on: true,
            trace: None,
        });
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            // A system's span is made once and entered every run.
            let a = bevy::log::info_span!("system", name = "a");
            for _ in 0..3 {
                drop(a.enter());
            }
            // Another span of the same name adds to the same slot.
            drop(bevy::log::info_span!("system", name = "a").entered());
            drop(bevy::log::info_span!("schedule", name = "Update").entered());
            drop(bevy::log::info_span!("render pass", name = "a").entered());
        });
        let runs = |kind, name: &str| {
            let slots = tally.0.lock().unwrap();
            slots
                .get(&(kind, Arc::from(name)))
                .map(|s| s.runs.swap(0, Ordering::Relaxed))
        };
        assert_eq!(runs(Kind::System, "a"), Some(4));
        assert_eq!(runs(Kind::Schedule, "Update"), Some(1));
        assert_eq!(
            tally.0.lock().unwrap().len(),
            2,
            "untimed spans get no slot"
        );
        // Read as `take_frame` reads it, a slot starts again from nothing.
        assert_eq!(runs(Kind::System, "a"), Some(0));
    }

    #[test]
    fn off_the_layer_turns_spans_away_but_not_events() {
        use bevy::log::tracing_subscriber::layer::SubscriberExt;
        let tally = ProfilerTally::default();
        let subscriber = bevy::log::tracing_subscriber::registry().with(ProfilerLayer {
            slots: tally.0.clone(),
            on: false,
            trace: None,
        });
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            let span = bevy::log::info_span!("system", name = "a");
            assert!(span.is_disabled());
            drop(span.entered());
            assert!(bevy::log::tracing::event_enabled!(
                bevy::log::tracing::Level::INFO
            ));
        });
        assert!(tally.0.lock().unwrap().is_empty());
    }

    #[test]
    fn the_summary_averages_over_the_window() {
        let mut p = FrameProfile::default();
        let sys: Arc<str> = "a::b::slow".into();
        for i in 0..4 {
            p.push(FrameSample {
                at: i as f64 * 0.1,
                frame_ms: 100.0,
                cpu: vec![
                    (Kind::System, sys.clone(), 10.0 * (i + 1) as f32, 1),
                    (Kind::Schedule, "Update".into(), 50.0, 1),
                ],
                gpu: vec![("main_pass".into(), 2.0)],
                gizmos: vec![("Lines".into(), 100)],
            });
        }
        let s = p.summary();
        assert_eq!(s.frames, 4);
        assert_eq!(
            s.systems[0],
            Row {
                name: "a::b::slow".into(),
                ms: 25.0,
                max_ms: 40.0,
                runs: 1.0
            }
        );
        assert_eq!(s.main.len(), 1);
        assert_eq!(s.gpu[0].ms, 2.0);
        assert_eq!(s.gizmos[0].ms, 100.0);
        // A frame long after drops the others, but the last frame always stays.
        p.push(FrameSample {
            at: 5.0,
            frame_ms: 3000.0,
            ..default()
        });
        assert_eq!(p.summary().frames, 1);
        let d = p.describe(5, CpuProfiling::Off);
        assert_eq!(d["window_frames"], 1);
        assert_eq!(d["cpu"]["timed"], false);
        assert!(
            d["cpu"]["why_not"]
                .as_str()
                .is_some_and(|w| w.contains("CADRS_FRAME_PROFILE=1"))
        );
    }
}
