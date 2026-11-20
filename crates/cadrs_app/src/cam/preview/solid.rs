//! Verify and Simulate for a run that leans its tool (a rotary or 5-axis pattern, a tilted
//! work plane) or cuts with an undercutting cutter ([`program::run_needs_solid`]): the height
//! map the other runs are cut on is seen down the setup's Z and cannot hold such cuts, so the
//! run is simulated on a solid stock instead ([`program::simulate_solid`], on the analysis
//! worker), which is cut whichever way the tool points.
//!
//! **What is shown.** The stock as the tool leaves it, where the tool is: a worker of its own
//! ([`Live`]) keeps the solid stock and cuts it on to wherever the preview's time is, part
//! way along a move too, and sends back its mesh, coloured by the operation that cut each
//! surface. The stock is cut exactly where it can be ([`cadrs_core::cam::exact`]: a solid
//! of the kernel's, cut by the tool's solid along the machine's motion, drawn from its own
//! surfaces), else on a lattice ([`cadrs_cam::sim::LiveStock`]). While it is cutting, playing waits for it (the
//! playback is slower rather than the stock behind the tool). Before its first mesh the
//! uncut stock is shown (the stock's own shape). At the run's end, once the check is done,
//! the check's own stock is shown, its gouges red.
//!
//! **What is found.** The gouges, the rapids into material, the holder in the stock and the
//! tool in the workholding come from the same simulation, where the height map's are listed.
//!
//! Also here: the tool as a solid ([`tool_mesh`], [`cadrs_cam::cutter::cutter_mesh`]) that
//! the preview moves along the run.

use super::*;
use cadrs_core::cam::CamStudio;
use cadrs_core::cam::cadrs_cam::cutter::{ToolMesh, cutter_mesh};
use cadrs_core::cam::cadrs_cam::dexel::SimMesh;
use cadrs_core::cam::cadrs_cam::toolpath::Toolpath;
use cadrs_core::cam::exact::Tweened;
use cadrs_ui::i18n::{tr, tr_with};

/// How fine the solid stock's lattice is, 0 (fast) to 1 (fine).
const QUALITY: f64 = 0.3;

/// What the worker needs to simulate the run on a solid stock.
#[derive(Clone)]
pub(super) struct SolidInput {
    pub(super) cam: CamStudio,
    pub(super) job: CamId,
    pub(super) setup: CamId,
    /// The operations previewed.
    pub(super) wanted: Vec<CamId>,
    pub(super) parts: Vec<cadrs_core::Part>,
    pub(super) paths: std::collections::HashMap<CamId, Toolpath>,
    /// Only the exact stock: a run the height map simulates, whose stock is shown exactly
    /// where it can be (the worker stops where it can't, or fails too often, and the height
    /// map is shown).
    pub(super) exact_only: bool,
}

/// What the solid simulation gives the preview.
pub(super) struct Solid {
    /// The stock at the run's end, its gouges marked, in the setup's coordinates.
    pub(super) end: SimMesh,
    pub(super) summary: String,
    pub(super) collisions: Vec<(f64, String)>,
}

/// Simulates the run on a solid stock: what the whole run finds and the stock at its end.
/// `Err`: why it could not be run.
pub(super) fn analyse_solid(
    input: &SolidInput,
    segments: &[Segment],
    spans: &[OpSpan],
) -> Result<Solid, String> {
    let cam = &input.cam;
    let job = cam.job(input.job).ok_or("the job is gone")?;
    let setup = job
        .setups
        .iter()
        .find(|s| s.id == input.setup)
        .ok_or("the setup is gone")?;
    let run = program::setup_run(
        cam,
        job,
        setup,
        &input.parts,
        &input.paths,
        Some(&input.wanted),
    )?;
    let r = program::simulate_solid(
        cam,
        job,
        setup,
        &run,
        &input.parts,
        &input.paths,
        None,
        QUALITY,
    )?;
    let end = r.stock.mesh(1);
    let tl = Timeline::new(segments.to_vec());
    // One of each kind an operation: where it first happens.
    let mut collisions: Vec<(f64, String)> = Vec::new();
    for (hits, what) in [
        (&r.rapid_hits, "cam-collision-rapid-into-stock"),
        (&r.fixture_hits, "cam-collision-tool-into-workholding"),
        (&r.holder_hits, "cam-collision-holder-meets-stock"),
    ] {
        let mut first: Vec<Option<f64>> = vec![None; spans.len()];
        for (k, p) in hits.iter().filter(|(k, _)| *k < spans.len()) {
            let t = super::analysis::time_near(&tl, spans, *k, *p);
            if first[*k].is_none_or(|was| t < was) {
                first[*k] = Some(t);
            }
        }
        for (k, t) in first.iter().enumerate() {
            if let Some(t) = t {
                collisions.push((*t, tr_with(what, &[("operation", &spans[k].name)])));
            }
        }
    }
    collisions.sort_by(|a, b| a.0.total_cmp(&b.0));
    // (Items joined by " · ".)
    let summary = format!(
        "{} · {} · {}{}",
        tr_with("cam-verify-removed", &[("volume", &fmt0(r.removed))]),
        if r.gouge_count == 0 {
            tr("cam-verify-no-gouges")
        } else {
            tr_with(
                "cam-verify-gouges-up-to",
                &[("depth", &fmt2(r.gouge_depth))],
            )
        },
        if collisions.is_empty() {
            tr("cam-verify-no-collisions")
        } else {
            tr_with(
                if collisions.len() == 1 {
                    "cam-verify-collision"
                } else {
                    "cam-verify-collisions"
                },
                &[("count", &collisions.len().to_string())],
            )
        },
        if r.excess > 0.0 {
            format!(
                " · {}",
                tr_with("cam-verify-left-up-to", &[("depth", &fmt2(r.excess))])
            )
        } else {
            String::new()
        }
    );
    Ok(Solid {
        end,
        summary,
        collisions,
    })
}

/// Where a solid run's stock is cut to: the run's move, and how far along it.
pub(super) type Place = (usize, f64);

/// The worker that cuts a solid run's stock to where the preview is (see the module docs).
pub(super) struct Live {
    ask: std::sync::mpsc::Sender<Ask>,
    got: std::sync::Mutex<std::sync::mpsc::Receiver<(Place, Shown)>>,
    notes: std::sync::Mutex<std::sync::mpsc::Receiver<Note>>,
    /// The last place asked for, and the place of the newest mesh.
    asked: Option<Place>,
    pub(super) cut: Option<Place>,
    /// The newest mesh, and a count that changes with it (or with the tween).
    pub(super) mesh: Option<SimMesh>,
    /// The newest mesh's edges, each a line through its points (the exact stock's; none on
    /// the lattice).
    pub(super) edges: Vec<Vec<[f32; 3]>>,
    /// The newest tween of the exact stock along the cut the tool is on: in place of a mesh,
    /// drawn as the tool moves along it with no cut made.
    pub(super) tween: Option<std::sync::Arc<Tweened>>,
    pub(super) version: u64,
    /// When the last place was asked for.
    asked_at: Option<Instant>,
    /// The worker has stopped (it couldn't go on, or an exact-only one gave up).
    pub(super) ended: bool,
    /// The exact stock's cuts, each's stretch of the run (places `n + f`), and the state of
    /// each in the run-ahead cache: what the timeline under the slider shows. Empty on the
    /// lattice.
    pub(super) spans: Vec<(f64, f64)>,
    pub(super) states: Vec<CutState>,
    /// A count that changes with the states.
    pub(super) cache_version: u64,
    /// The cuts' tweens the worker has made and still keeps: a place within one (or between it
    /// and the next) is shown from here at once, without asking the worker.
    tweens: std::collections::HashMap<usize, std::sync::Arc<Tweened>>,
    /// The cut whose tween is shown, and the cut the worker was last told the tool is at.
    tween_cut: Option<usize>,
    told: Option<usize>,
}

/// A cut's state in the worker's run-ahead cache ([`Live::states`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CutState {
    /// Not made yet.
    Pending,
    /// Being made.
    Computing,
    /// Made: played back at once.
    Ready,
    /// Made, but the kernel couldn't cut it exactly (cut as best it could), or not made.
    Failed,
}

/// What the worker says of its cache: each cut's stretch of the run (once), a cut's state, a
/// tween made, or one let go.
enum Note {
    Cuts(Vec<(f64, f64)>),
    State(usize, CutState),
    Made(usize, std::sync::Arc<Tweened>),
    Dropped(usize),
}

/// What the worker is asked: the stock at a place, or only where the tool is now (a cut shown
/// from the cache), to run ahead from there.
enum Ask {
    Show(Place),
    At(usize),
}

/// The most memory the cuts' tweens kept for playing back may take (bytes): past it the ones
/// farthest behind the tool are let go, and running ahead waits.
const CACHE_BYTES: usize = 1 << 30;

/// About how much memory a tween takes (bytes).
fn tween_bytes(t: &Tweened) -> usize {
    let mesh = |m: &SimMesh| m.positions.len() * (12 + 12 + 16 + 1) + m.indices.len() * 4;
    let lines = |l: &[Vec<[f32; 3]>]| l.iter().map(|x| x.len() * 16).sum::<usize>();
    mesh(&t.before)
        + mesh(&t.after)
        + (t.before_times.len() + t.after_times.len()) * 4
        + lines(&t.before_edges)
        + lines(&t.after_edges)
}

/// The worker's run-ahead cache: each cut's tween as made, within [`CACHE_BYTES`].
struct TweenCache {
    tweens: std::collections::HashMap<usize, (std::sync::Arc<Tweened>, usize)>,
    /// Cuts tried and not made (none to show).
    missing: std::collections::HashSet<usize>,
    bytes: usize,
    cuts: usize,
}

impl TweenCache {
    /// The tween of cut `c`: kept, else made (and told of).
    fn get(
        &mut self,
        e: &mut cadrs_core::cam::exact::ExactStock,
        c: usize,
        note: &dyn Fn(Note),
    ) -> Option<std::sync::Arc<Tweened>> {
        if let Some((t, _)) = self.tweens.get(&c) {
            return Some(t.clone());
        }
        if self.missing.contains(&c) {
            return None;
        }
        note(Note::State(c, CutState::Computing));
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.tweened_cut(c, DEFLECTION)
        }))
        .ok()
        .flatten();
        let Some(t) = made else {
            self.missing.insert(c);
            note(Note::State(c, CutState::Failed));
            return None;
        };
        let t = std::sync::Arc::new(t);
        let bytes = tween_bytes(&t);
        self.bytes += bytes;
        self.tweens.insert(c, (t.clone(), bytes));
        note(Note::Made(c, t.clone()));
        let state = if e.cut_failed(c) {
            CutState::Failed
        } else {
            CutState::Ready
        };
        note(Note::State(c, state));
        Some(t)
    }

    /// Lets the tweens farthest behind `at` go while over [`CACHE_BYTES`]; then the next cut
    /// from `at` on to make, if there is room.
    fn next(&mut self, at: usize, note: &dyn Fn(Note)) -> Option<usize> {
        while self.bytes > CACHE_BYTES {
            let Some(&far) = self.tweens.keys().filter(|&&c| c < at).min() else {
                break;
            };
            if let Some((_, b)) = self.tweens.remove(&far) {
                self.bytes -= b;
            }
            note(Note::Dropped(far));
            note(Note::State(far, CutState::Pending));
        }
        if self.bytes > CACHE_BYTES {
            return None;
        }
        (at..self.cuts).find(|c| !self.tweens.contains_key(c) && !self.missing.contains(c))
    }
}

/// The most places a second the worker is asked to cut to (each a boolean, for the exact
/// stock).
const ASKS_A_SECOND: f64 = 10.0;

/// How far behind the tool (seconds) the stock may be before playing waits for it.
const MOST_BEHIND: f64 = 0.15;

impl Live {
    /// Starts the worker: it builds the stock ([`program::solid_start`]), then cuts it to each
    /// place asked for (the newest, when several are waiting). `None`: no worker.
    pub(super) fn start(input: SolidInput) -> Option<Live> {
        let (ask, asks) = std::sync::mpsc::channel::<Ask>();
        let (send, got) = std::sync::mpsc::channel();
        let (tell, notes) = std::sync::mpsc::channel::<Note>();
        let spawned = cadrs_core::background::spawn("cam-live-stock", move || {
            let Some(mut live) = Engine::of(&input) else {
                return;
            };
            let note = |n: Note| {
                let _ = tell.send(n);
            };
            let mut cache = TweenCache {
                tweens: Default::default(),
                missing: Default::default(),
                bytes: 0,
                cuts: 0,
            };
            if let Engine::Exact(e) = &live {
                let spans = e.cut_spans();
                cache.cuts = spans.len();
                note(Note::Cuts(spans));
            }
            // The cut the tool is at: running ahead goes on from there.
            let mut at = 0usize;
            loop {
                // A place asked for (the newest); waited for only when there is nothing to
                // run ahead to (the cache full, or every cut made).
                // (Not in a page, whose one thread this runs on.)
                let ahead = match &live {
                    Engine::Exact(_)
                        if std::env::var_os("CADRS_NO_TWEEN").is_none()
                            && !cfg!(target_arch = "wasm32") =>
                    {
                        cache.next(at, &note)
                    }
                    _ => None,
                };
                let first = match ahead {
                    None => match asks.recv() {
                        Ok(a) => Some(a),
                        Err(_) => break,
                    },
                    Some(_) => match asks.try_recv() {
                        Ok(a) => Some(a),
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    },
                };
                let Some(first) = first else {
                    // Nothing asked: the next cut ahead made, for playing back at once.
                    if let (Some(c), Engine::Exact(e)) = (ahead, &mut live) {
                        cache.get(e, c, &note);
                    }
                    continue;
                };
                // (Only the newest place matters.)
                let mut to = None;
                for a in std::iter::once(first).chain(asks.try_iter()) {
                    match a {
                        Ask::Show(p) => to = Some(p),
                        Ask::At(c) => at = c,
                    }
                }
                let Some(to) = to else { continue };
                if let Engine::Exact(e) = &live {
                    at = e
                        .cut_at(to.0, to.1)
                        .map(|(c, _)| c)
                        .or_else(|| e.cut_before(to.0, to.1))
                        .unwrap_or(0);
                }
                let Some(mut mesh) = live.show(to, &mut cache, &note) else {
                    break;
                };
                if live.gives_up() && input.exact_only {
                    warn!("Simulate's exact stock fails too often here: the height map from now");
                    break;
                }
                if live.gives_up()
                    && let Some(run) = program::setup_run(
                        &input.cam,
                        input
                            .cam
                            .job(input.job)
                            .expect("the job the engine was made for"),
                        input
                            .cam
                            .job(input.job)
                            .and_then(|j| j.setups.iter().find(|s| s.id == input.setup))
                            .expect("the setup the engine was made for"),
                        &input.parts,
                        &input.paths,
                        Some(&input.wanted),
                    )
                    .ok()
                    && let Some(lattice) = Engine::lattice(&input, &run)
                {
                    warn!("Simulate's exact stock fails too often here: on the lattice from now");
                    live = lattice;
                    note(Note::Cuts(Vec::new()));
                    let Some(m) = live.cut_to(to) else { break };
                    mesh = Shown::Mesh(m.0, m.1);
                }
                if send.send((to, mesh)).is_err() {
                    break;
                }
            }
        });
        spawned.ok().map(|_| Live {
            ask,
            got: std::sync::Mutex::new(got),
            notes: std::sync::Mutex::new(notes),
            asked: None,
            cut: None,
            mesh: None,
            edges: Vec::new(),
            tween: None,
            version: 0,
            asked_at: None,
            ended: false,
            spans: Vec::new(),
            states: Vec::new(),
            cache_version: 0,
            tweens: Default::default(),
            tween_cut: None,
            told: None,
        })
    }

    /// What the worker made for `at`.
    fn take(&mut self, at: Place, shown: Shown) {
        self.cut = Some(at);
        match shown {
            Shown::Mesh(m, edges) => {
                self.tween_cut = None;
                (self.mesh, self.tween, self.edges) = (Some(m), None, edges);
                self.version += 1;
            }
            Shown::Tween(t) => {
                if !self
                    .tween
                    .as_ref()
                    .is_some_and(|have| std::sync::Arc::ptr_eq(have, &t))
                {
                    self.tween_cut = Some(t.cut);
                    (self.mesh, self.tween, self.edges) = (None, Some(t), Vec::new());
                    self.version += 1;
                }
            }
        }
    }

    /// Takes what the worker has said of its cache.
    fn read_notes(&mut self) {
        let Ok(rx) = self.notes.get_mut() else { return };
        for n in rx.try_iter() {
            match n {
                Note::Cuts(spans) => {
                    self.states = vec![CutState::Pending; spans.len()];
                    self.spans = spans;
                    self.tweens.clear();
                }
                Note::State(c, st) => {
                    if let Some(s) = self.states.get_mut(c) {
                        *s = st;
                    }
                }
                Note::Made(c, t) => {
                    self.tweens.insert(c, t);
                }
                Note::Dropped(c) => {
                    self.tweens.remove(&c);
                }
            }
            self.cache_version += 1;
        }
    }

    /// The cut whose tween shows the stock at `to`: the one the tool is on, else the last it
    /// finished (shown whole); `None` before the first.
    fn cut_of(&self, to: Place) -> Option<usize> {
        let place = to.0 as f64 + to.1.clamp(0.0, 1.0);
        let c = self.spans.partition_point(|s| s.1 <= place);
        match self.spans.get(c) {
            Some(s) if s.0 <= place => Some(c),
            _ => c.checked_sub(1),
        }
    }

    /// Whether it is still cutting to the last place asked for.
    pub(super) fn busy(&self) -> bool {
        self.asked.is_some() && self.asked != self.cut
    }

    /// Whether the stock is behind the tool by more than [`MOST_BEHIND`]: playing then waits.
    pub(super) fn lagging(&self) -> bool {
        self.busy()
            && self
                .asked_at
                .is_some_and(|t| t.elapsed().as_secs_f64() > MOST_BEHIND)
    }

    /// Takes the newest mesh it has made, and asks for `to` when it is free and isn't there.
    pub(super) fn follow(&mut self, to: Option<Place>) {
        let mut got: Vec<(Place, Shown)> = Vec::new();
        if let Ok(rx) = self.got.get_mut() {
            loop {
                match rx.try_recv() {
                    Ok(g) => got.push(g),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.ended = true;
                        break;
                    }
                }
            }
        }
        for (at, shown) in got {
            self.take(at, shown);
        }
        self.read_notes();
        if self.ended {
            self.asked = None;
            return;
        }
        // A place within a cut made (or between it and the next): shown from the cache, the
        // worker only told where the tool is (to run ahead from there).
        if let Some(to) = to
            && let Some(c) = self.cut_of(to)
            && let Some(t) = self.tweens.get(&c).cloned()
        {
            if self.told != Some(c) && self.ask.send(Ask::At(c)).is_ok() {
                self.told = Some(c);
            }
            self.take(to, Shown::Tween(t));
            self.asked = Some(to);
            return;
        }
        let due = self
            .asked_at
            .is_none_or(|t| t.elapsed().as_secs_f64() >= 1.0 / ASKS_A_SECOND);
        if let Some(to) = to
            && !self.busy()
            && due
            && self.cut != Some(to)
            && self.ask.send(Ask::Show(to)).is_ok()
        {
            self.asked = Some(to);
            self.asked_at = Some(Instant::now());
        }
    }

    /// Waits (up to `limit`) until the worker has made every cut it will run ahead to (none
    /// pending or being made, or it has stopped): for scripts.
    pub(super) fn settle_cache(&mut self, to: Option<Place>, limit: std::time::Duration) {
        let until = Instant::now() + limit;
        loop {
            self.follow(to);
            let busy = self
                .states
                .iter()
                .any(|s| matches!(s, CutState::Pending | CutState::Computing));
            if self.ended || !busy || Instant::now() >= until || cfg!(target_arch = "wasm32") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Waits (up to `limit`) until the stock is cut to `to`: for scripts, which take a
    /// picture right after a seek.
    pub(super) fn settle(&mut self, to: Place, limit: std::time::Duration) {
        let until = Instant::now() + limit;
        while self.cut != Some(to) && !self.ended && Instant::now() < until {
            // (A script waits: asked at once, however soon after the last.)
            self.asked_at = None;
            self.follow(Some(to));
            if self.cut == Some(to) {
                break;
            }
            let Ok(rx) = self.got.get_mut() else { return };
            if let Ok((at, shown)) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
                self.take(at, shown);
            }
        }
    }
}

/// How finely the exact stock is drawn (mm from its surfaces).
const DEFLECTION: f64 = 0.01;

/// What cuts the stock as the run plays: exactly, the stock a solid of the kernel's
/// ([`cadrs_core::cam::exact`]), where the stock has an exact solid (a box or a bar, the
/// job's first setup); else on the lattice the check runs on.
enum Engine {
    Exact(Box<cadrs_core::cam::exact::ExactStock>),
    Lattice(Box<cadrs_core::cam::cadrs_cam::sim::LiveStock>),
}

/// What the worker sends for a place: the stock as a mesh, or the tween along the cut the tool
/// is on (the place along it the viewer's own).
pub(super) enum Shown {
    /// The stock and its edges.
    Mesh(SimMesh, Vec<Vec<[f32; 3]>>),
    Tween(std::sync::Arc<Tweened>),
}

impl Engine {
    /// The stock at `to`: the exact one tweened along the cut the tool is on, or the last it
    /// finished (shown whole), from the cache (`cache`, made there if need be); else as a mesh.
    fn show(&mut self, to: Place, cache: &mut TweenCache, note: &dyn Fn(Note)) -> Option<Shown> {
        if let Engine::Exact(e) = self
            && std::env::var_os("CADRS_NO_TWEEN").is_none()
            && let Some(c) = e
                .cut_at(to.0, to.1)
                .map(|(c, _)| c)
                .or_else(|| e.cut_before(to.0, to.1))
            && let Some(t) = cache.get(e, c, note)
        {
            return Some(Shown::Tween(t));
        }
        self.cut_to(to).map(|(m, e)| Shown::Mesh(m, e))
    }

    fn of(input: &SolidInput) -> Option<Engine> {
        let cam = &input.cam;
        let job = cam.job(input.job)?;
        let setup = job.setups.iter().find(|s| s.id == input.setup)?;
        let run = program::setup_run(
            cam,
            job,
            setup,
            &input.parts,
            &input.paths,
            Some(&input.wanted),
        )
        .ok()?;
        let first_setup = job
            .setups
            .iter()
            .find(|s| !s.suppressed)
            .is_none_or(|s| s.id == setup.id);
        // (Exactly unless `CADRS_EXACT_SIM=0`. On the lattice where it can't be made, or once
        // it fails too often: [`Engine::gives_up`]; a run on the height map then shows that.)
        let exact = std::env::var_os("CADRS_EXACT_SIM").is_none_or(|v| v != "0");
        if exact
            && first_setup
            && let (Some(machine), Some(shape)) = (
                job.machine.as_ref(),
                program::stock_shape(cam, job, &input.parts),
            )
        {
            let stock = program::stock_in(&shape, &run.frame);
            let above = [0.0, 0.0, stock.bounds.max[2] + 30.0];
            let motions = program::run_motions(setup, job, &run, above);
            let track = program::axis_track(machine, &run, &motions);
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cadrs_core::cam::exact::ExactStock::new(
                    machine,
                    &run,
                    &shape,
                    &motions,
                    &track.rows,
                )
            }));
            match made {
                Ok(Ok(e)) => return Some(Engine::Exact(Box::new(e))),
                Ok(Err(why)) => info!("Simulate's stock not exact: {why}"),
                Err(_) => warn!("Simulate's exact stock could not be made"),
            }
        }
        if input.exact_only {
            return None;
        }
        Engine::lattice(input, &run)
    }

    /// The stock on the lattice (tri-dexels): every setup, and the fallback of the exact one.
    fn lattice(input: &SolidInput, run: &program::SetupRun) -> Option<Engine> {
        let cam = &input.cam;
        let job = cam.job(input.job)?;
        let setup = job.setups.iter().find(|s| s.id == input.setup)?;
        let start = program::solid_start(
            cam,
            job,
            setup,
            run,
            &input.parts,
            &input.paths,
            None,
            QUALITY,
        )
        .ok()?;
        Some(Engine::Lattice(Box::new(
            cadrs_core::cam::cadrs_cam::sim::LiveStock::new(start.stock, start.ops),
        )))
    }

    /// The lattice in place of an exact stock that fails too often: more than one cut in a
    /// hundred the kernel couldn't make (a 5-axis finishing pass, rotary roughing round a
    /// bar), past the first fifty. Its stock would show what those cuts leave; the lattice's,
    /// coarser, shows them all.
    fn gives_up(&self) -> bool {
        match self {
            Engine::Exact(e) => e.cut_count_done() >= 50 && e.failed_share() > 0.01,
            Engine::Lattice(_) => false,
        }
    }

    /// The stock cut to `to`, as a mesh, and its edges. `None`: the engine failed.
    fn cut_to(&mut self, to: Place) -> Option<(SimMesh, Vec<Vec<[f32; 3]>>)> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match self {
            Engine::Exact(e) => {
                e.cut_to(to.0, to.1);
                e.sim_mesh_and_edges(DEFLECTION)
            }
            Engine::Lattice(l) => {
                l.cut_to(to.0, to.1);
                Some((l.stock.mesh(1), Vec::new()))
            }
        }))
        .ok()
        .flatten()
    }
}

/// A mesh the viewport's part material lights ([`crate::part_shading::PartShading`]): its
/// vertex colours are the surfaces' own, and nothing of it is selected.
fn lit_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    let uv = vec![[0.0f32; 2]; positions.len()];
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

/// The solid stock (in the setup's coordinates, `frame`) as a mesh in the model's.
pub(super) fn stock_mesh(m: &SimMesh, frame: &Frame) -> Mesh {
    let positions = m
        .positions
        .iter()
        .map(|p| v3(frame.to_world([p[0] as f64, p[1] as f64, p[2] as f64])).to_array())
        .collect();
    let normals = m
        .normals
        .iter()
        .map(|n| v3(frame.dir_to_world([n[0] as f64, n[1] as f64, n[2] as f64])).to_array())
        .collect();
    lit_mesh(positions, normals, m.colors.clone(), m.indices.clone())
}

/// How far past the cut's ends (0 to 1) a tween's never and always are drawn.
const TIME_PAST: f32 = 0.002;

/// A tween's side of the stock ([`stock_mesh`]), each vertex's time along the cut in its UV y
/// (the part shader's tween: `crate::part_shading::PartShadingParams::tween`).
pub(super) fn timed_stock_mesh(m: &SimMesh, times: &[f32], frame: &Frame) -> Mesh {
    let mut mesh = stock_mesh(m, frame);
    // (Never and always as times just past the cut's ends (0 to 1): a triangle's time is
    // extrapolated a little past its edges where a pixel's samples straddle it, and from an
    // infinity, or a far-off time, that lands anywhere: −∞ or −0.1 there punched dots along the
    // cut's outline ahead. A step past the ends keeps it near.)
    // Nearer still: never a step past the latest of the times beside it (the vertices it shares
    // a triangle with), always a step before the earliest; else just past the ends.
    let mut shown: Vec<f32> = times
        .iter()
        .map(|t| if t.is_finite() { *t } else { f32::NAN })
        .collect();
    for t in m.indices.chunks(3) {
        let finite: Vec<f32> = t
            .iter()
            .map(|&i| times[i as usize])
            .filter(|x| x.is_finite())
            .collect();
        let (Some(lo), Some(hi)) = (
            finite.iter().copied().reduce(f32::min),
            finite.iter().copied().reduce(f32::max),
        ) else {
            continue;
        };
        for &i in t {
            let (v, s) = (times[i as usize], &mut shown[i as usize]);
            if v == f32::INFINITY {
                *s = s.max(hi + TIME_PAST);
            } else if v == f32::NEG_INFINITY {
                *s = if s.is_nan() {
                    lo - TIME_PAST
                } else {
                    s.min(lo - TIME_PAST)
                };
            }
        }
    }
    let uv: Vec<[f32; 2]> = times
        .iter()
        .zip(&shown)
        .map(|(t, s)| {
            let far = if *t > 0.0 {
                1.0 + TIME_PAST
            } else {
                -TIME_PAST
            };
            [
                0.0,
                if s.is_nan() {
                    far
                } else {
                    s.clamp(-TIME_PAST, 1.0 + TIME_PAST)
                },
            ]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh
}

/// The uncut stock as a solid, from its surface's triangles in the model's coordinates: flat
/// faces in the stock's grey.
pub(super) fn blank_mesh(triangles: &[[P3; 3]]) -> Mesh {
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut normals = Vec::with_capacity(triangles.len() * 3);
    for t in triangles {
        let [a, b, c] = t.map(v3);
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        for p in [a, b, c] {
            positions.push(p.to_array());
            normals.push(n);
        }
    }
    let colors = vec![[0.78, 0.80, 0.84, 1.0]; positions.len()];
    let indices = (0..positions.len() as u32).collect();
    lit_mesh(positions, normals, colors, indices)
}

/// A milling tool as a solid, its tip at the origin and its axis along +Z: flutes, shank,
/// holder and arbor each in its colour.
pub(super) fn tool_mesh(tool: &Tool) -> Mesh {
    let ToolMesh {
        positions,
        normals,
        indices,
        colors,
    } = cutter_mesh(tool, 48);
    lit_mesh(positions, normals, colors, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uncut_stock_is_a_flat_faced_solid() {
        let tris = cadrs_core::cam::cadrs_cam::model::box_triangles(
            &cadrs_core::cam::cadrs_cam::geom::Aabb::new([0.0; 3], [10.0, 20.0, 30.0]),
        );
        let m = blank_mesh(&tris);
        assert_eq!(m.count_vertices(), 36);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) =
            m.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        // Every normal is along an axis, pointing out of the box.
        assert!(
            n.iter()
                .all(|v| v.iter().filter(|x| x.abs() > 0.999).count() == 1)
        );
    }

    #[test]
    fn a_tool_is_a_closed_lit_mesh() {
        let tool = Tool::new(cadrs_core::cam::cadrs_cam::tool::ToolKind::EndMill, 6.0);
        let m = tool_mesh(&tool);
        let n = m.count_vertices();
        assert!(n > 100);
        for a in [
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_COLOR,
            Mesh::ATTRIBUTE_UV_0,
        ] {
            assert_eq!(m.attribute(a).map(|v| v.len()), Some(n));
        }
        assert_eq!(m.indices().map(|i| i.len() % 3), Some(0));
    }
}
