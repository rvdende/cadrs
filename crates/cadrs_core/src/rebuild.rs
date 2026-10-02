//! Rebuilding a Part Studio's parts from its feature list (P3.1; parts and booleans since P3.3).
//!
//! - **Parts** are carried through the feature list: an extrude with New makes parts (one per
//!   separate solid), Add joins its body to the parts in its merge scope, Remove cuts it from
//!   them and Intersect keeps the overlap; the Boolean feature combines parts; Delete part
//!   removes them. A part keeps its [`PartId`] (and so its name) while later features change it;
//!   a part a boolean splits keeps its id on its largest piece, the other pieces become new
//!   parts, as in Onshape.
//! - **Merge scope** (PS5.4): Merge with all acts on every part; an explicit scope on those
//!   parts; an empty scope on the parts the new body touches (Add) or overlaps (Remove,
//!   Intersect). [`Build::contacts`] tells the Extrude dialog what the new body touches, so it
//!   can pick Add by itself (PS5.2).
//! - Every body goes through the solid-modelling kernel ([`cadrs_kernel`], OpenCascade with the
//!   default `occt` feature; without it, only blind New extrudes, as the prism mesh of
//!   [`crate::solid`]).
//! - **Per-feature cache.** Each feature's result (the parts after it, with their kernel bodies,
//!   display meshes and mass properties, and its error) is cached under a key that chains the
//!   keys of all the features before it with its own parameters. An edit changes the keys from
//!   the edited feature on, so a rebuild recomputes from the first changed feature.
//! - **Off the main thread.** All kernel work runs on one worker thread, which owns the kernel
//!   session and the cache. [`request`] starts a rebuild and returns at once (the app shows the
//!   old parts until the new ones arrive); [`build`] waits for it.
//! - **Errors.** A feature that can't be built (the kernel refused, its regions are gone, …)
//!   gets an error message in [`Build::errors`] and leaves the parts as they were; the feature
//!   list shows it in red. Kernel exceptions come back as errors (see `cadrs_kernel`), and a
//!   panic while rebuilding fails that rebuild instead of the app.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use cadrs_kernel::BodyId;
use serde::{Deserialize, Serialize};
use cadrs_sketch::geom::BezierGeom;
use cadrs_sketch::region::{Piece, Region};
use cadrs_sketch::{CurveId, CurveKind, Sketch, Vec2, Vec3};

use crate::brep::{ChainGeom, ProfileGroup};
use crate::document::{BodyType, ExtrudeFeature, Feature, FeatureKind, RegionRef};
use crate::export::{StepFile, StepRequest};
use crate::ids::{FeatureId, PartId};
use crate::parts::{Part, PartKind};

/// Import and export jobs on the worker (P3F.2).
pub mod exchange;
#[cfg(feature = "occt")]
pub mod persist;
#[cfg(feature = "occt")]
pub mod session;

/// The result of rebuilding a feature list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Build {
    /// Every part, in the order they were made ("Part 1", "Part 2", …).
    pub parts: Vec<Part>,
    /// The features that failed, with why.
    pub errors: Vec<(FeatureId, String)>,
    /// The features that built with a warning (P3.10, PS11.1: Onshape's yellow state), with why.
    pub warnings: Vec<(FeatureId, String)>,
    /// What each extrude's new body touches and overlaps among the parts before it.
    pub contacts: HashMap<FeatureId, Contacts>,
    /// The default name of every part the features made, including the ones a later feature
    /// joined to another or deleted (a Boolean's tools list still names them).
    pub names: Vec<(PartId, String)>,
    /// How long the rebuild took.
    pub elapsed: Duration,
    /// How many features were computed rather than taken from the cache.
    pub computed: usize,
    /// The last part feature's parts before it and its new body, if it is an extrude that
    /// joins parts (Add): the dialog previews the body over the parts as they were.
    pub stage: Option<(FeatureId, Stage)>,
    /// Each revolve's axis: its origin and unit direction (after its flip; the first end turns
    /// counter-clockwise about it), for the dialog's angle arrow.
    pub axes: HashMap<FeatureId, (Vec3, Vec3)>,
    /// Each Plane feature's frame (P3.7), where it built.
    pub planes: HashMap<FeatureId, cadrs_sketch::PlaneFrame>,
    /// Each curve feature's curve (a Helix), where it built.
    pub curves: HashMap<FeatureId, crate::surfacing::HelixGeom>,
    /// Each Mate connector feature's frame (P3.8), where it built.
    pub connectors: HashMap<FeatureId, cadrs_sketch::PlaneFrame>,
    /// Each pattern's instances with where their Skip dots go (P3.8, PS22.5).
    pub dots: HashMap<FeatureId, Vec<crate::pattern::InstanceDot>>,
    /// Each loft's picked directions as arrows (origin, unit direction; P3.11, PS20.4).
    pub arrows: HashMap<FeatureId, Vec<(Vec3, Vec3)>>,
    /// How long each part feature took to regenerate (P3.9, PS2.6), in list order: the time of
    /// the rebuild that computed it (a feature taken from the cache keeps its time).
    pub times: Vec<(FeatureId, Duration)>,
    /// P3.11 (PS11.2): per feature, the features it depended on that its references don't name
    /// (a fillet's tangent chain running over another fillet's faces), for Show dependencies.
    pub uses: HashMap<FeatureId, Vec<FeatureId>>,
    /// P3.11 (P3.9 judge): each sketch's regeneration time, in list order: finding its profile
    /// regions (what the features use), measured when the sketch last changed.
    pub sketch_times: Vec<(FeatureId, Duration)>,
    /// P3H.6: the composite parts (their parts are in `parts` too), in list order.
    pub composites: Vec<crate::transform::Composite>,
    /// P3D.1 (IR5.2): for each feature that failed or warned, the inputs that no longer resolve,
    /// as positions in its main selection list (an extrude's or revolve's regions, then its
    /// whole sketches, then its faces; a fillet's or chamfer's edges and faces). The dialog keeps
    /// them in their field as "Missing Face of Sketch 3" rather than dropping them.
    pub missing: HashMap<FeatureId, Vec<usize>>,
    /// P3G.4 (DV3.5): what each Derived feature brought in.
    pub derived: HashMap<FeatureId, crate::derived::DerivedOutput>,
    /// P3G.4: the derived sketches, placed (each a sketch on a frame of its own, named after the
    /// Derived feature): later features take their regions like any sketch's.
    pub derived_sketches: Vec<Feature>,
    /// P3I.2: each Sheet metal model's definition, flat pattern and parts (the "sheet metal
    /// contexts", SM1.3), in list order; a failed model keeps its context (the flat view shows
    /// why, SM1.5).
    pub sheet_metal: Vec<crate::sheetmetal::SheetMetalContext>,
}

/// P3D.1: how a feature came out of the last rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureStatus {
    Ok,
    /// Built, but something it uses is gone or didn't take (Onshape's yellow state).
    Warning(String),
    /// Failed, with why.
    Error(String),
}

/// The parts before an Add extrude and the body it adds to them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub before: Vec<Part>,
    pub tool: Arc<crate::solid::Solid>,
}

/// The parts an extrude's new body meets (before its boolean).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contacts {
    /// Parts it touches or overlaps (joined by Add).
    pub touches: Vec<PartId>,
    /// Parts it overlaps (with volume: cut by Remove).
    pub overlaps: Vec<PartId>,
}

impl Build {
    /// Why a feature failed, if it did.
    pub fn error(&self, feature: FeatureId) -> Option<&str> {
        self.errors
            .iter()
            .find(|(f, _)| *f == feature)
            .map(|(_, e)| e.as_str())
    }

    /// Why a feature built with a warning (P3.10, PS11.1), if it did.
    pub fn warning(&self, feature: FeatureId) -> Option<&str> {
        self.warnings.iter().find(|(f, _)| *f == feature).map(|(_, e)| e.as_str())
    }

    /// P3D.1: the feature's status: failed, built with a warning, or fine.
    pub fn status(&self, feature: FeatureId) -> FeatureStatus {
        match (self.error(feature), self.warning(feature)) {
            (Some(e), _) => FeatureStatus::Error(e.to_string()),
            (None, Some(w)) => FeatureStatus::Warning(w.to_string()),
            _ => FeatureStatus::Ok,
        }
    }

    /// P3D.1: the positions of the feature's inputs that no longer resolve (see
    /// [`Build::missing`]).
    pub fn missing_inputs(&self, feature: FeatureId) -> &[usize] {
        self.missing.get(&feature).map_or(&[], |v| v.as_slice())
    }

    /// The part with this id.
    pub fn part(&self, id: PartId) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    /// A rebuild in which every part feature failed with `why`.
    fn failed(features: &[Feature], why: &str) -> Self {
        Self {
            errors: features
                .iter()
                .filter(|f| f.is_part_feature())
                .map(|f| (f.id, why.to_string()))
                .collect(),
            ..Self::default()
        }
    }
}

/// A part as the rebuild carries it: the part, its kernel body and its names.
#[derive(Clone, Serialize, Deserialize)]
struct PartState {
    part: Part,
    body: Option<BodyId>,
    names: Arc<cadrs_kernel::BodyNames>,
}

/// The parts after a feature.
#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    parts: Vec<PartState>,
    /// The number the next new part gets ("Part N") and the next surface ("Surface N").
    next_part: u32,
    next_surface: u32,
    /// The swept geometry of every extrude so far (names, frames and silhouettes of faces).
    geoms: Arc<crate::brep::Geoms>,
    /// The frames of the Plane features so far (P3.7).
    planes: HashMap<FeatureId, cadrs_sketch::PlaneFrame>,
    /// The frames of the Mate connector features so far (P3.8).
    connectors: HashMap<FeatureId, cadrs_sketch::PlaneFrame>,
    /// The part owning each Mate connector (P3B.7): its frame is attached to that part's solid
    /// at the end of the rebuild, so it travels with the part into assemblies.
    connector_owners: HashMap<FeatureId, PartId>,
    /// The composite parts so far (P3H.6).
    composites: Vec<crate::transform::Composite>,
    /// The curves of the curve features so far (a Helix).
    curves: HashMap<FeatureId, crate::surfacing::HelixGeom>,
    /// P3G.4: what each Derived feature so far brought in, and its sketches (placed).
    derived: Arc<HashMap<FeatureId, crate::derived::DerivedOutput>>,
    derived_sketches: Arc<Vec<Feature>>,
    /// P3I.2: the sheet metal models so far (their definitions, flats and parts).
    #[serde(default)]
    sheet_metal: Arc<Vec<crate::sheetmetal::SheetMetalContext>>,
}

impl State {
    fn part(&self, id: PartId) -> Option<&PartState> {
        self.parts.iter().find(|p| p.part.id == id)
    }
}

/// What a feature produced.
#[derive(Serialize, Deserialize)]
struct Output {
    state: Arc<State>,
    error: Option<String>,
    /// P3.10 (PS11.1): built, but something the user picked is gone or didn't take (Onshape's
    /// yellow warning state).
    warning: Option<String>,
    contacts: Option<Contacts>,
    /// The kernel bodies this feature made that its state holds.
    owned: Vec<BodyId>,
    /// An Add extrude's parts before it and its new body (for the dialog's preview).
    stage: Option<Stage>,
    /// A revolve's axis (origin, unit direction; after its flip).
    axis: Option<(Vec3, Vec3)>,
    /// A pattern's instances, for Skip instances (P3.8).
    dots: Option<Vec<crate::pattern::InstanceDot>>,
    /// P3.11 (PS11.2): features it depends on that its references don't name (the faces along
    /// a fillet's tangent chain).
    uses: Vec<FeatureId>,
    /// P3.11 (PS20.4): direction arrows to draw while it is edited (origin, unit direction): a
    /// loft's picked start and end directions at its first and last profile.
    arrows: Vec<(Vec3, Vec3)>,
}

struct Entry {
    output: Output,
    /// How long computing it took.
    time: Duration,
    /// The rebuild that last used it.
    last_used: u64,
}

/// Cached outputs are dropped when this many rebuilds went by without using them.
const KEEP_REBUILDS: u64 = 48;
/// And the oldest are dropped beyond this many.
const MAX_ENTRIES: usize = 1024;

/// A kernel session and the per-feature cache. The app uses the one on the worker thread
/// (through [`build`] and [`request`]); tests and benchmarks can own one.
pub struct Rebuilder {
    #[cfg(feature = "occt")]
    kernel: cadrs_kernel::backend::occt::OcctKernel,
    entries: HashMap<u64, Entry>,
    generation: u64,
    /// The feature whose [`Stage`] is worth meshing (the last part feature).
    stage_for: Option<FeatureId>,
    /// The parts after each part feature of the rebuild in progress (P3.8: a feature pattern
    /// without Reapply copies the difference the features made).
    trail: Vec<(FeatureId, Arc<State>)>,
    /// The parts after the last rebuild (with their kernel bodies, for exports).
    last: Arc<State>,
    /// P3.11: each sketch's regeneration time (its profile regions), with the key of the
    /// sketch it was measured for.
    sketch_times: HashMap<FeatureId, (u64, Duration)>,
    /// P3G.4: how deep the Derived features' source rebuilds nest.
    depth: usize,
    /// The generation of the last top-level rebuild: the outputs it used (its Derived sources'
    /// too) have `last_used` at or after it ([`Self::used_keys`]).
    top_generation: u64,
}

impl Default for Rebuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// How far the rebuild thread's current rebuild is: (part features done, part features in all),
/// for the app's "Rebuilding…" indicator. Only rebuilds on that thread count, and only the
/// top-level one (not a Derived feature's source rebuilt inside it).
pub fn progress() -> (usize, usize) {
    (PROGRESS_DONE.load(Ordering::Relaxed), PROGRESS_TOTAL.load(Ordering::Relaxed))
}

static PROGRESS_DONE: AtomicUsize = AtomicUsize::new(0);
static PROGRESS_TOTAL: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Set on the rebuild thread: its rebuilds report [`progress`].
    static REPORTS_PROGRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The geometry fingerprint this build was made with: a hash of the source that decides what a
/// rebuild makes, the locked OpenCASCADE and the compiler (see `build.rs`). Built geometry kept
/// across sessions is keyed by it, so a fix never reuses what older code built.
pub const GEOMETRY_FINGERPRINT: &str = env!("CADRS_GEOMETRY_FINGERPRINT");

/// The cache key [`Rebuilder::rebuild`] gives the last part feature of `features`: the chain of
/// every feature up to it (a snapshot of a rebuild is kept under it, [`session`]).
pub fn final_key(features: &[Feature]) -> u64 {
    chain_keys(features).last().copied().unwrap_or(CHAIN_SEED)
}

/// The cache keys [`Rebuilder::rebuild`] gives the part features of `features`, in order.
pub fn chain_keys(features: &[Feature]) -> Vec<u64> {
    let last = features.iter().rposition(Feature::is_part_feature);
    let var_errors = crate::variables::check(features, &cadrs_sketch::units::Units::default());
    let mut key = CHAIN_SEED;
    let mut keys = Vec::new();
    for i in 0..last.map_or(0, |l| l + 1) {
        let f = &features[i];
        key = chain_key(key, &f.kind);
        if !f.is_part_feature() {
            continue;
        }
        let below = parent_below(features, i).or_else(|| var_errors.iter().find(|(id, _)| *id == f.id).map(|(_, w)| w.clone()));
        if let Some(why) = &below {
            key = chain_key_str(key, why);
        }
        keys.push(key);
    }
    keys
}

/// The chain key before the first feature.
const CHAIN_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

/// Feeds formatted text into a hasher.
struct HashWriter<'a>(&'a mut DefaultHasher);

impl std::fmt::Write for HashWriter<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// The cache key of a feature: its parameters chained onto the key of the features before it.
fn chain_key(before: u64, kind: &FeatureKind) -> u64 {
    let mut h = DefaultHasher::new();
    before.hash(&mut h);
    // Every parameter is in the derived Debug output (floats in full precision).
    let _ = write!(HashWriter(&mut h), "{kind:?}");
    h.finish()
}

fn chain_key_str(before: u64, s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    before.hash(&mut h);
    s.hash(&mut h);
    h.finish()
}

impl Rebuilder {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "occt")]
            kernel: cadrs_kernel::backend::occt::OcctKernel::new(),
            entries: HashMap::new(),
            generation: 0,
            stage_for: None,
            trail: Vec::new(),
            last: Arc::default(),
            sketch_times: HashMap::new(),
            depth: 0,
            top_generation: 0,
        }
    }

    /// P3G.4: rebuilds another feature list (a Derived feature's source) inside the rebuild in
    /// progress, in the same kernel session and cache (bodies can't move between sessions), and
    /// gives its build and its final parts with their kernel bodies. The rebuild in progress is
    /// left as it was.
    fn sub_build(&mut self, features: &[Feature]) -> Result<(Build, Arc<State>), String> {
        if self.depth >= 8 {
            return Err("Derived features nest too deeply".into());
        }
        let trail = std::mem::take(&mut self.trail);
        let stage_for = self.stage_for;
        let last = self.last.clone();
        let times = self.sketch_times.clone();
        self.depth += 1;
        let build = self.rebuild(features);
        self.depth -= 1;
        let state = std::mem::replace(&mut self.last, last);
        self.trail = trail;
        self.stage_for = stage_for;
        self.sketch_times = times;
        Ok((build, state))
    }

    /// How many feature outputs are cached.
    pub fn cached(&self) -> usize {
        self.entries.len()
    }

    /// Rebuilds the parts of `features`, reusing every cached feature output up to the first
    /// changed feature.
    pub fn rebuild(&mut self, features: &[Feature]) -> Build {
        let start = Instant::now();
        self.generation += 1;
        let generation = self.generation;
        if self.depth == 0 {
            self.top_generation = generation;
        }
        let mut out = Build::default();
        // Nothing after the last part feature affects a part, so it isn't even hashed (a sketch
        // being drawn at the end of the list costs nothing here).
        let last = features.iter().rposition(Feature::is_part_feature);
        let mut key = CHAIN_SEED;
        let mut state = Arc::new(State::default());
        self.trail.clear();
        // P3F.4: expressions naming a variable defined below them, or nowhere, fail their
        // feature (the variables themselves and sketches are reported after the loop).
        // (Which unit a bare number is in changes values, not whether they evaluate.)
        let var_errors = crate::variables::check(features, &cadrs_sketch::units::Units::default());
        let report = self.depth == 0 && REPORTS_PROGRESS.with(|r| r.get());
        if report {
            let total = features[..last.map_or(0, |l| l + 1)].iter().filter(|f| f.is_part_feature()).count();
            PROGRESS_DONE.store(0, Ordering::Relaxed);
            PROGRESS_TOTAL.store(total, Ordering::Relaxed);
        }
        for i in 0..last.map_or(0, |l| l + 1) {
            let f = &features[i];
            key = chain_key(key, &f.kind);
            if !f.is_part_feature() {
                continue;
            }
            let before = state.clone();
            // A parent below it in the list (PS11.3): it fails, and leaves the parts alone. The
            // chain key covers only what is above, so this outcome gets a key of its own (a
            // result cached while the parent was missing altogether must not stand for it).
            let below = parent_below(features, i).or_else(|| var_errors.iter().find(|(id, _)| *id == f.id).map(|(_, w)| w.clone()));
            if let Some(why) = &below {
                key = chain_key_str(key, why);
            }
            if !self.entries.contains_key(&key) {
                out.computed += 1;
                // Only the last feature's stage is kept (the one a dialog edits).
                self.stage_for = (Some(i) == last).then_some(f.id);
                let t0 = Instant::now();
                let output = match below {
                    Some(why) => Output {
                        state: state.clone(),
                        error: Some(why),
                        warning: None,
                        contacts: None,
                        owned: Vec::new(),
                        stage: None,
                        axis: None,
                        arrows: Vec::new(),
                        dots: None,
                        uses: Vec::new(),
                    },
                    None => self.compute(&features[..i], f, &state),
                };
                self.entries.insert(
                    key,
                    Entry {
                        output,
                        time: t0.elapsed(),
                        last_used: generation,
                    },
                );
            }
            if report {
                PROGRESS_DONE.fetch_add(1, Ordering::Relaxed);
            }
            let entry = self.entries.get_mut(&key).expect("just inserted");
            entry.last_used = generation;
            state = entry.output.state.clone();
            out.times.push((f.id, entry.time));
            self.trail.push((f.id, state.clone()));
            for p in &state.parts {
                if !out.names.iter().any(|(id, _)| *id == p.part.id) {
                    out.names.push((p.part.id, p.part.name.clone()));
                }
            }
            if let Some(e) = &entry.output.error {
                out.errors.push((f.id, e.clone()));
            } else if pattern_seeds(f).iter().any(|s| out.errors.iter().any(|(e, _)| e == s)) {
                // P3D.4 (IR6.1, `ex1-step1.png`): a feature pattern or mirror of a feature
                // that failed fails too.
                out.errors.push((f.id, "A feature to pattern failed".into()));
            }
            if let Some(w) = &entry.output.warning {
                out.warnings.push((f.id, w.clone()));
            }
            // P3D.1: which of a failed or warned feature's inputs are gone.
            if entry.output.error.is_some() || entry.output.warning.is_some() {
                let with: Vec<Feature>;
                let above: &[Feature] = if before.derived_sketches.is_empty() {
                    &features[..i]
                } else {
                    with = [&features[..i], &before.derived_sketches[..]].concat();
                    &with
                };
                let lost = missing_inputs(above, f, &before);
                if !lost.is_empty() {
                    out.missing.insert(f.id, lost);
                }
            }
            if let Some(c) = &entry.output.contacts {
                out.contacts.insert(f.id, c.clone());
            }
            if let Some(a) = entry.output.axis {
                out.axes.insert(f.id, a);
            }
            if let Some(d) = &entry.output.dots {
                out.dots.insert(f.id, d.clone());
            }
            if !entry.output.arrows.is_empty() {
                out.arrows.insert(f.id, entry.output.arrows.clone());
            }
            if !entry.output.uses.is_empty() {
                out.uses.insert(f.id, entry.output.uses.clone());
            }
            out.stage = entry.output.stage.clone().map(|s| (f.id, s));
        }
        // Like the part features, only the sketches up to the last part feature regenerate (a
        // sketch being drawn at the end of the list costs nothing).
        for f in &features[..last.map_or(0, |l| l + 1)] {
            let Some(sk) = f.sketch() else { continue };
            let k = chain_key(0, &f.kind);
            let t = match self.sketch_times.get(&f.id) {
                Some((key, t)) if *key == k => *t,
                _ => {
                    let t0 = Instant::now();
                    std::hint::black_box(cadrs_sketch::region::regions(&sk.geometry));
                    let t = t0.elapsed();
                    self.sketch_times.insert(f.id, (k, t));
                    t
                }
            };
            out.sketch_times.push((f.id, t));
        }
        self.sketch_times.retain(|id, _| features.iter().any(|f| f.id == *id));
        out.composites = state.composites.iter().filter(|c| state.part(c.part).is_some()).cloned().collect();
        for (id, why) in &var_errors {
            if !out.errors.iter().any(|(e, _)| e == id) && features.iter().any(|f| f.id == *id && !f.is_part_feature()) {
                out.errors.push((*id, why.clone()));
            }
        }
        // P3H.6 (PCB7.9): a closed composite part is one part: its members aren't listed, shown,
        // picked or inserted on their own (they stay in the rebuild state for later features).
        let absorbed: HashSet<PartId> = out.composites.iter().filter(|c| c.closed).flat_map(|c| c.members.iter().copied()).collect();
        out.parts = state.parts.iter().filter(|p| !absorbed.contains(&p.part.id)).map(|p| p.part.clone()).collect();
        out.planes = state.planes.clone();
        out.curves = state.curves.clone();
        out.connectors = state.connectors.clone();
        // P3B.7 (A22.2): each part carries the explicit connectors it owns, in list order.
        for f in features {
            let (Some(owner), Some(frame)) = (state.connector_owners.get(&f.id), state.connectors.get(&f.id)) else { continue };
            if let Some(p) = out.parts.iter_mut().find(|p| p.id == *owner) {
                std::sync::Arc::make_mut(&mut p.solid).connectors.push(crate::solid::SolidConnector { feature: f.id, frame: *frame });
            }
        }
        // P3G.4: the mate connectors Derived features brought in go with their derived parts.
        for f in features {
            let Some(d) = state.derived.get(&f.id) else { continue };
            for (c, _) in &d.connectors {
                let (Some(owner), Some(frame)) = (state.connector_owners.get(c), state.connectors.get(c)) else { continue };
                if let Some(p) = out.parts.iter_mut().find(|p| p.id == *owner) {
                    std::sync::Arc::make_mut(&mut p.solid).connectors.push(crate::solid::SolidConnector { feature: *c, frame: *frame });
                }
            }
        }
        out.derived = (*state.derived).clone();
        out.derived_sketches = (*state.derived_sketches).clone();
        out.sheet_metal = (*state.sheet_metal).clone();
        debug_assert!(self.depth != 0 || key == final_key(features), "final_key must follow the rebuild's keys");
        self.trail.clear();
        self.last = state;
        self.evict();
        out.elapsed = start.elapsed();
        out
    }

    /// The cache keys of the outputs the last top-level rebuild used.
    pub fn used_keys(&self) -> Vec<u64> {
        self.entries.iter().filter(|(_, e)| e.last_used >= self.top_generation).map(|(k, _)| *k).collect()
    }

    /// What a snapshot of rebuilding `features` holds: the outputs the last rebuild used, and
    /// its Derived features' sources' outputs, which it doesn't touch while the Derived output
    /// itself is cached (an edit of the Derived feature then needs them).
    pub fn snapshot_keys(&self, features: &[Feature]) -> Vec<u64> {
        fn sources(features: &[Feature], out: &mut Vec<u64>, depth: usize) {
            for f in features {
                if let FeatureKind::Derived(d) = &f.kind
                    && depth < 8
                {
                    out.extend(chain_keys(&d.studio));
                    sources(&d.studio, out, depth + 1);
                }
            }
        }
        let mut keys = self.used_keys();
        sources(features, &mut keys, 0);
        keys
    }

    /// Drops outputs no recent rebuild used, and releases the kernel bodies no remaining output
    /// holds.
    fn evict(&mut self) {
        let generation = self.generation;
        let mut stale: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, e)| e.last_used + KEEP_REBUILDS < generation)
            .map(|(k, _)| *k)
            .collect();
        if self.entries.len() - stale.len() > MAX_ENTRIES {
            let mut all: Vec<(u64, u64)> =
                self.entries.iter().map(|(k, e)| (*k, e.last_used)).collect();
            all.sort_by_key(|(_, used)| *used);
            stale = all[..self.entries.len() - MAX_ENTRIES].iter().map(|(k, _)| *k).collect();
        }
        if stale.is_empty() {
            return;
        }
        let mut released: Vec<BodyId> = Vec::new();
        for k in stale {
            if let Some(e) = self.entries.remove(&k) {
                released.extend(e.output.owned);
            }
        }
        let held: HashSet<BodyId> = self
            .entries
            .values()
            .flat_map(|e| e.output.state.parts.iter().filter_map(|p| p.body))
            .collect();
        for b in released {
            if !held.contains(&b) {
                self.release(b);
            }
        }
    }

    #[cfg(feature = "occt")]
    fn release(&mut self, body: BodyId) {
        use cadrs_kernel::Kernel;
        self.kernel.release(body);
    }

    #[cfg(not(feature = "occt"))]
    fn release(&mut self, _body: BodyId) {}

    /// Rebuilds `features` and writes the requested parts (as they are after the last feature)
    /// to STEP: one file per part, or one file with them all (see [`crate::export`]).
    #[cfg(feature = "occt")]
    pub fn export_step(&mut self, features: &[Feature], req: &StepRequest) -> Result<Vec<StepFile>, String> {
        use cadrs_kernel::Kernel;
        if req.parts.is_empty() {
            return Err("No parts to export".into());
        }
        let build = self.rebuild(features);
        let state = self.last.clone();
        let mut bodies = Vec::new();
        for (id, name) in &req.parts {
            let body = state
                .part(*id)
                .and_then(|p| p.body)
                .ok_or_else(|| match build.part(*id) {
                    Some(_) => format!("{name} has no solid model to export"),
                    None => format!("{name} no longer exists"),
                })?;
            bodies.push((body, name.as_str()));
        }
        // Y up: turn -90° about X, so +Z (the Top plane's normal) becomes +Y.
        let mut turned = Vec::new();
        if req.y_up {
            let turn = cadrs_kernel::Transform::rotation(nalgebra::Vector3::x() * -std::f64::consts::FRAC_PI_2);
            for (body, _) in &mut bodies {
                let r = self.kernel.transform(*body, &turn).map_err(|e| e.to_string());
                let new = r.and_then(|r| r.bodies.first().copied().ok_or_else(|| "turning a part failed".into()));
                match new {
                    Ok(b) => {
                        turned.push(b);
                        *body = b;
                    }
                    Err(e) => {
                        turned.iter().for_each(|b| self.kernel.release(*b));
                        return Err(e);
                    }
                }
            }
        }
        let groups: Vec<&[(BodyId, &str)]> = if req.individual {
            bodies.chunks(1).collect()
        } else {
            vec![&bodies[..]]
        };
        let files = groups
            .into_iter()
            .map(|g| {
                let ids: Vec<BodyId> = g.iter().map(|(b, _)| *b).collect();
                let names: Vec<&str> = g.iter().map(|(_, n)| *n).collect();
                let bytes = self.kernel.export_step(&ids).map_err(|e| e.to_string())?;
                let text = String::from_utf8_lossy(&bytes);
                Ok(StepFile {
                    part: (g.len() == 1).then(|| names[0].to_string()),
                    bytes: crate::export::name_products(&text, &names).into_bytes(),
                })
            })
            .collect();
        for b in turned {
            self.kernel.release(b);
        }
        files
    }

    #[cfg(not(feature = "occt"))]
    pub fn export_step(&mut self, _features: &[Feature], _req: &StepRequest) -> Result<Vec<StepFile>, String> {
        Err("STEP export needs the solid-modelling kernel".into())
    }

    /// Rebuilds `features` and projects the requested parts for a drawing view (P3C.2): the
    /// kernel's hidden-line removal, the persistent names of each edge's source, and the shaded
    /// triangles when asked for.
    pub fn project_view(
        &mut self,
        features: &[Feature],
        req: &crate::views::ViewRequest,
    ) -> Result<crate::views::ViewGeometry, String> {
        let build = self.rebuild(features);
        // A flat pattern view (P3I.7): the part's flat, from its sheet metal model.
        if req.flat {
            return crate::flat_drawing::flat_geometry(&build.sheet_metal, req);
        }
        let state = self.last.clone();
        let parts = crate::views::view_parts(&build.parts, req.part);
        if parts.is_empty() {
            return Err(match req.part {
                Some(_) => "The part no longer exists".into(),
                None => "The Part Studio has no parts".into(),
            });
        }
        let (edges, holes) = crate::views::model_data(&parts, features);
        let mut out = crate::views::ViewGeometry {
            parts: parts.iter().map(|p| p.id).collect(),
            bounds: crate::views::mesh_bounds(&parts, &req.frame),
            edges,
            holes,
            threads: crate::views::threads(&parts, features),
            chamfers: crate::views::chamfers(features),
            ..Default::default()
        };
        if req.shaded {
            out.shaded = crate::views::shade(&parts, &req.frame, &req.props, &req.appearances);
        }
        let (projection, hatch) = self.project_parts(&state, &parts, req)?;
        out.projection = projection;
        out.hatch = hatch;
        Ok(out)
    }

    /// Projects an assembly for a drawing view (P3C.5, see [`crate::drawing_assembly`]): each
    /// studio rebuilt, a moved copy of each shown occurrence's body made with the kernel, all
    /// projected together (so parts hide each other), and the shaded triangles in each studio's
    /// appearances. The geometry's parts are the occurrences (their ids as the feature), in the
    /// order of the projected edges' body indices.
    pub fn project_assembly(
        &mut self,
        st: &crate::drawing_assembly::AssemblyState,
        req: &crate::views::ViewRequest,
    ) -> Result<crate::views::ViewGeometry, String> {
        let mut out = crate::views::ViewGeometry::default();
        let mut moved: Vec<Part> = Vec::new();
        let mut tris: Vec<crate::views::ShadedTriangle> = Vec::new();
        // Each triangle's body (its index in `bodies`), for `hide_occluded_pieces`.
        let mut owners: Vec<Option<usize>> = Vec::new();
        #[cfg(feature = "occt")]
        let mut bodies: Vec<(BodyId, Arc<cadrs_kernel::BodyNames>)> = Vec::new();
        for (el, studio) in &st.studios {
            let occs: Vec<&crate::drawing_assembly::OccState> =
                st.occurrences.iter().filter(|o| o.element == *el && !o.hidden).collect();
            if occs.is_empty() {
                continue;
            }
            let build = self.rebuild(&studio.features);
            #[cfg(feature = "occt")]
            let kstate = self.last.clone();
            for o in occs {
                let Some(p) = build.part(o.part) else { continue };
                let mut q = p.clone();
                q.solid = Arc::new(crate::assembly::transform_solid(&p.solid, &o.pose));
                // The triangles shade a shaded view, and give every assembly view the depths
                // that check its edges' visibility (`hide_occluded_pieces`).
                let shaded = crate::views::shade(&[&q], &req.frame, &studio.props, &studio.appearances);
                // Owned by the body pushed below, if it is.
                #[cfg(feature = "occt")]
                let first = tris.len();
                owners.extend(std::iter::repeat_n(None, shaded.len()));
                tris.extend(shaded);
                #[cfg(feature = "occt")]
                {
                    use cadrs_kernel::Kernel;
                    let Some(s) = kstate.part(o.part) else { continue };
                    let Some(body) = s.body else { continue };
                    let r = o.pose.rotation_matrix();
                    let rot = nalgebra::UnitQuaternion::from_rotation_matrix(&nalgebra::Rotation3::from_matrix_unchecked(r));
                    let t = o.pose.translation;
                    let iso = cadrs_kernel::Transform::from_parts(nalgebra::Translation3::new(t[0], t[1], t[2]), rot);
                    match self.kernel.transform(body, &iso).map_err(|e| e.to_string()).and_then(|r| r.bodies.first().copied().ok_or_else(|| "moving a part failed".to_string())) {
                        Ok(b) => {
                            owners[first..].fill(Some(bodies.len()));
                            bodies.push((b, s.names.clone()));
                        }
                        Err(e) => {
                            for (b, _) in &bodies {
                                self.kernel.release(*b);
                            }
                            return Err(e);
                        }
                    }
                }
                out.parts.push(o.view_part());
                moved.push(q);
            }
        }
        if moved.is_empty() {
            return Err("The assembly has no parts to show".into());
        }
        let refs: Vec<&Part> = moved.iter().collect();
        out.bounds = crate::views::mesh_bounds(&refs, &req.frame);
        // Farthest first, over every studio's triangles (with their owners).
        let mean = |t: &crate::views::ShadedTriangle| (t.depths[0] + t.depths[1] + t.depths[2]) / 3.0;
        let mut paired: Vec<(crate::views::ShadedTriangle, Option<usize>)> = tris.into_iter().zip(owners).collect();
        paired.sort_by(|a, b| mean(&b.0).total_cmp(&mean(&a.0)));
        let (tris, owners): (Vec<_>, Vec<_>) = paired.into_iter().unzip();
        #[cfg(feature = "occt")]
        {
            use cadrs_kernel::Kernel;
            let ids: Vec<BodyId> = bodies.iter().map(|(b, _)| *b).collect();
            let projected = self.kernel.project(&ids, &req.frame, &req.options).map_err(|e| e.to_string());
            let mut projected = projected;
            if let Ok(p) = &mut projected {
                hide_occluded_pieces(&self.kernel, &ids, &req.frame, &tris, Some(&owners), p);
                if req.intersections {
                    let extra = part_intersections(&mut self.kernel, &ids, &req.frame, &req.options, &tris);
                    p.edges.extend(extra);
                }
            }
            for b in &ids {
                self.kernel.release(*b);
            }
            let mut proj = projected?;
            for e in &mut proj.edges {
                if let Some(src) = &mut e.source
                    && let Some((_, n)) = bodies.get(src.body)
                {
                    src.edge_name = src.edge.and_then(|id| n.edge(id));
                    src.face_name = src.face.and_then(|id| n.face(id));
                }
            }
            out.projection = proj;
        }
        if req.shaded {
            out.shaded = tris;
        }
        Ok(out)
    }

    #[cfg(feature = "occt")]
    fn project_parts(
        &mut self,
        state: &State,
        parts: &[&Part],
        req: &crate::views::ViewRequest,
    ) -> Result<ProjectedParts, String> {
        use cadrs_kernel::Kernel;
        let mut bodies = Vec::new();
        let mut names = Vec::new();
        for p in parts {
            let s = state.part(p.id).ok_or("a part has no solid model")?;
            bodies.push(s.body.ok_or_else(|| format!("{} has no solid model", p.name))?);
            names.push(s.names.clone());
        }
        // Section and broken-out views: project the bodies with the cut material removed
        // (temporary bodies, released after), and hatch the faces the cut leaves (P3C.8).
        let mut hatch = Vec::new();
        let mut temps = Vec::new();
        if let Some(cut) = &req.cut {
            let named: Vec<(cadrs_kernel::BodyId, &cadrs_kernel::naming::BodyNames)> =
                bodies.iter().copied().zip(names.iter().map(|n| &**n)).collect();
            let c = crate::section::cut_bodies(&mut self.kernel, &named, &req.frame, cut)?;
            bodies = c.bodies;
            names = c.names.into_iter().map(Arc::new).collect();
            hatch = c.hatch;
            temps = c.temps;
        }
        let projected = self.kernel.project(&bodies, &req.frame, &req.options).map_err(|e| e.to_string());
        for b in temps {
            self.kernel.release(b);
        }
        let mut proj = projected?;
        for e in &mut proj.edges {
            if let Some(src) = &mut e.source
                && let Some(n) = names.get(src.body)
            {
                src.edge_name = src.edge.and_then(|id| n.edge(id));
                src.face_name = src.face.and_then(|id| n.face(id));
            }
        }
        Ok((proj, hatch))
    }

    #[cfg(not(feature = "occt"))]
    fn project_parts(
        &mut self,
        _state: &State,
        _parts: &[&Part],
        _req: &crate::views::ViewRequest,
    ) -> Result<ProjectedParts, String> {
        Err("Drawing views need the solid-modelling kernel".into())
    }

    /// Builds one feature from the parts before it.
    fn compute(&mut self, before: &[Feature], f: &Feature, state: &Arc<State>) -> Output {
        // P3G.4: derived sketches are there for the features after them, like any sketch.
        let with: Vec<Feature>;
        let before: &[Feature] = if state.derived_sketches.is_empty() {
            before
        } else {
            with = [before, &state.derived_sketches[..]].concat();
            &with
        };
        let fail = |why: String| Output {
            state: state.clone(),
            error: Some(why),
            warning: None,
            contacts: None,
            owned: Vec::new(),
            stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
        };
        match &f.kind {
            FeatureKind::Sketch(_) | FeatureKind::Variable(_) => fail(String::new()),
            FeatureKind::DeletePart(d) => {
                let mut next = (**state).clone();
                let missing = d.parts.iter().filter(|p| state.part(**p).is_none()).count();
                next.parts.retain(|p| !d.parts.contains(&p.part.id));
                Output {
                    state: Arc::new(next),
                    error: None,
                    // PS11.1: the other parts are deleted; a warning, as Onshape's yellow.
                    warning: (missing > 0).then(|| {
                        if missing == 1 {
                            "1 deleted part no longer exists".to_string()
                        } else {
                            format!("{missing} deleted parts no longer exist")
                        }
                    }),
                    contacts: None,
                    owned: Vec::new(),
                    stage: None,
                    axis: None,
                    arrows: Vec::new(),
                    dots: None,
                    uses: Vec::new(),
                }
            }
            #[cfg(feature = "occt")]
            FeatureKind::Extrude(e) => match self.extrude(before, f.id, e, state) {
                Ok(o) => o,
                Err(why) => fail(why),
            },
            #[cfg(feature = "occt")]
            FeatureKind::Boolean(b) => match self.boolean(f.id, b, state) {
                Ok(o) => o,
                Err(why) => fail(why),
            },
            #[cfg(feature = "occt")]
            FeatureKind::Revolve(r) => match self.revolve(before, f.id, r, state) {
                Ok(o) => o,
                Err(why) => fail(why),
            },
            #[cfg(feature = "occt")]
            FeatureKind::Fillet(x) => self.fillet(f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Chamfer(x) => self.chamfer(f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Shell(x) => self.shell(f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Hole(x) => self.hole(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Plane(x) => self.plane(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Sweep(x) => self.sweep(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Loft(x) => self.loft(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Split(x) => self.split_parts(before, f.id, x, state).unwrap_or_else(fail),
            FeatureKind::MateConnector(x) => self.mate_connector(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Pattern(x) => self.pattern(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Mirror(x) => self.mirror(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Draft(x) => self.draft(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Derived(x) => self.derived(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Derived(_) => fail("This feature needs the solid-modelling kernel".into()),
            FeatureKind::Transform(x) => self.transform(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Composite(x) => self.composite(f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Import(x) => self.import(f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Thicken(x) => self.thicken(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Helix(x) => self.helix(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::Fill(x) => self.fill(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(feature = "occt")]
            FeatureKind::SheetMetalModel(x) => self.sheet_metal_model(before, f.id, x, state).unwrap_or_else(fail),
            #[cfg(not(feature = "occt"))]
            FeatureKind::SheetMetalModel(_) => fail("Sheet metal needs the solid-modelling kernel".into()),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Thicken(_) | FeatureKind::Helix(_) | FeatureKind::Fill(_) => {
                fail("This feature needs the solid-modelling kernel".into())
            }
            #[cfg(not(feature = "occt"))]
            FeatureKind::Composite(_) => fail("This feature needs the solid-modelling kernel".into()),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Draft(_) | FeatureKind::Transform(_) => fail("This feature needs the solid-modelling kernel".into()),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Import(_) | FeatureKind::Derived(_) => fail("Imports need the solid-modelling kernel".into()),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Pattern(_) | FeatureKind::Mirror(_) => fail("This feature needs the solid-modelling kernel".into()),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Plane(_) | FeatureKind::Sweep(_) | FeatureKind::Loft(_) | FeatureKind::Split(_) => {
                fail("This feature needs the solid-modelling kernel".into())
            }
            #[cfg(not(feature = "occt"))]
            FeatureKind::Fillet(_) | FeatureKind::Chamfer(_) | FeatureKind::Shell(_) | FeatureKind::Hole(_) => {
                fail("This feature needs the solid-modelling kernel".into())
            }
            #[cfg(not(feature = "occt"))]
            FeatureKind::Revolve(_) => fail("Revolves need the solid-modelling kernel".into()),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Extrude(e) => prism_fallback(before, f.id, e, state).unwrap_or_else(fail),
            #[cfg(not(feature = "occt"))]
            FeatureKind::Boolean(_) => fail("Booleans need the solid-modelling kernel".into()),
        }
    }
}

impl Rebuilder {
    /// A Mate connector feature (P3.8, X11): its frame, from its origin entity, placed.
    fn mate_connector(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        x: &crate::mate::MateConnectorFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        let parts: Vec<Part> = state.parts.iter().map(|p| p.part.clone()).collect();
        #[allow(unused_mut)]
        let mut base = x.base_frame(before, &parts)?;
        // P3.11: the Alignment entity's direction is the primary axis.
        #[cfg(feature = "occt")]
        if let Some(d) = &x.alignment {
            let z = self.direction(before, state, d)?;
            base = crate::mate::aligned(base, [z.x, z.y, z.z]);
        }
        let frame = x.place(base);
        let mut next = (**state).clone();
        next.connectors.insert(id, frame);
        if let Some(owner) = x.owner_part(&parts) {
            next.connector_owners.insert(id, owner);
        }
        Ok(Output {
            state: Arc::new(next),
            error: None,
            warning: None,
            contacts: None,
            owned: Vec::new(),
            stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
        })
    }
}

/// The frame of a mate connector a feature refers to, among the parts and connectors so far.
fn connector_frame(before: &[Feature], state: &State, c: &crate::mate::ConnectorRef) -> Result<cadrs_sketch::PlaneFrame, String> {
    let parts: Vec<Part> = state.parts.iter().map(|p| p.part.clone()).collect();
    crate::mate::frame(c, before, &parts, &state.connectors)
}

/// Why feature `i` can't be built where it is: a feature it refers to comes after it (dragged
/// above its parent, PS11.3).
fn parent_below(features: &[Feature], i: usize) -> Option<String> {
    let f = &features[i];
    f.parents().into_iter().find_map(|p| {
        let j = features.iter().position(|g| g.id == p)?;
        (j > i).then(|| format!("{} is below this feature in the list; move it back above", features[j].name))
    })
}

/// Without a kernel: a blind New extrude as the prism mesh.
#[cfg(not(feature = "occt"))]
fn prism_fallback(before: &[Feature], id: FeatureId, e: &ExtrudeFeature, state: &Arc<State>) -> Result<Output, String> {
    if let Some(p) = e.problem() {
        return Err(p.into());
    }
    if e.op != crate::document::BooleanOp::New || e.end != crate::document::EndType::Blind {
        return Err("This extrude needs the solid-modelling kernel".into());
    }
    let (groups, _) = extrude_groups(before, e);
    let mut out: Option<crate::solid::Solid> = None;
    for g in &groups {
        let s = crate::solid::extrude(id.0, &g.frame, &g.regions, e.depth, e.flip);
        match &mut out {
            Some(o) => o.append(s),
            None => out = Some(s),
        }
    }
    let solid = out.filter(|s| !s.indices.is_empty()).ok_or("nothing to extrude")?;
    let mut next = (**state).clone();
    next.next_part += 1;
    next.parts.push(PartState {
        part: Part {
            id: PartId::new(id, 0),
            feature: id,
            name: format!("Part {}", next.next_part),
            kind: PartKind::Solid,
            palette: next.next_part + next.next_surface - 1,
            solid: Arc::new(solid),
            mass: None,
            features: vec![id],
            source: None,
            derived: None,
        },
        body: None,
        names: Arc::default(),
    });
    Ok(Output {
        state: Arc::new(next),
        error: None,
        warning: None,
        contacts: None,
        owned: Vec::new(),
        stage: None,
        axis: None,
        arrows: Vec::new(),
        dots: None,
        uses: Vec::new(),
    })
}

/// The features a feature pattern or mirror copies (none for other features).
fn pattern_seeds(f: &Feature) -> Vec<FeatureId> {
    use crate::pattern::PatternType;
    match &f.kind {
        FeatureKind::Pattern(x) if x.pattern_type == PatternType::Feature => x.features.clone(),
        FeatureKind::Mirror(x) if x.mirror_type == PatternType::Feature => x.features.clone(),
        _ => Vec::new(),
    }
}

/// P3D.1 (IR5.2): the positions of `f`'s inputs that no longer resolve against the features
/// before it and the parts they made (see [`Build::missing`]). Never guesses: a region is
/// missing when [`RegionRef::resolve`] finds nothing, an edge or face when no part has it.
fn missing_inputs(before: &[Feature], f: &Feature, state: &State) -> Vec<usize> {
    let region_gone = |r: &RegionRef| {
        before
            .iter()
            .find(|x| x.id == r.sketch)
            .and_then(|x| x.sketch())
            .is_none_or(|sk| sk.plane.is_none() || r.resolve(&sk.geometry).is_none())
    };
    let sketch_gone = |s: &FeatureId| {
        before
            .iter()
            .find(|x| x.id == *s)
            .and_then(|x| x.sketch())
            .is_none_or(|sk| sk.plane.is_none() || sk.geometry.curves.is_empty())
    };
    let mut out = Vec::new();
    let mut flags: Vec<bool> = Vec::new();
    match &f.kind {
        FeatureKind::Extrude(e) => {
            flags.extend(e.regions.iter().map(region_gone));
            flags.extend(e.sketches.iter().map(sketch_gone));
            flags.extend(e.faces.iter().map(|x| !face_resolves(state, x)));
        }
        FeatureKind::Revolve(r) => {
            flags.extend(r.regions.iter().map(region_gone));
            flags.extend(r.sketches.iter().map(sketch_gone));
            flags.extend(r.faces.iter().map(|x| !face_resolves(state, x)));
        }
        FeatureKind::Fillet(x) => flags.extend(x.entities.iter().map(|e| !entity_resolves(state, e))),
        FeatureKind::Chamfer(x) => flags.extend(x.entities.iter().map(|e| !entity_resolves(state, e))),
        _ => {}
    }
    for (i, gone) in flags.into_iter().enumerate() {
        if gone {
            out.push(i);
        }
    }
    out
}

#[cfg(feature = "occt")]
fn face_resolves(state: &State, f: &crate::document::FaceRef) -> bool {
    kernel_ops::face_resolves(state, f)
}

#[cfg(not(feature = "occt"))]
fn face_resolves(state: &State, f: &crate::document::FaceRef) -> bool {
    state.parts.iter().any(|p| p.part.solid.faces.iter().any(|x| x.name == f.face))
}

#[cfg(feature = "occt")]
fn entity_resolves(state: &State, e: &crate::applied::EdgeOrFace) -> bool {
    kernel_ops::entity_resolves(state, e)
}

#[cfg(not(feature = "occt"))]
fn entity_resolves(state: &State, e: &crate::applied::EdgeOrFace) -> bool {
    match e {
        crate::applied::EdgeOrFace::Edge(r) => {
            state.parts.iter().any(|p| p.part.solid.edges.iter().any(|x| x.name == r.edge))
        }
        crate::applied::EdgeOrFace::Face(f) => face_resolves(state, f),
    }
}

// ---------------------------------------------------------------------------------------------
// Extrude and Boolean through the kernel

#[cfg(feature = "occt")]
mod kernel_ops {
    use super::*;
    use cadrs_kernel::naming::{self, BodyNames};
    use cadrs_kernel::{BoolOp, ExtrudeEnd, ExtrudeSpec, FaceInput, Kernel, OpResult, RevolveEnd, RevolveSpec};
    use nalgebra::{Point3, Unit, Vector3};

    use crate::document::{
        AxisRef, BooleanFeature, BooleanKind, BooleanOp, DirectionRef, EndType, Offset, RevolveFeature, RevolveType, UpTo,
    };

    /// A body a boolean made, split into its solids: each piece with its names and the input
    /// bodies it has faces of.
    struct Piece {
        body: BodyId,
        names: BodyNames,
        from: HashSet<BodyId>,
        volume: f64,
    }

    /// A new or changed part: its id and its piece.
    type Placed = (PartId, Piece);

    /// How a new body combines with the parts (an extrude's or a revolve's tabs).
    struct Merge<'a> {
        op: BooleanOp,
        merge_all: bool,
        scope: &'a [PartId],
        surface: bool,
    }

    mod advanced;
    mod applied;
    mod derived;
    mod draft;
    mod import;
    mod linked;
    mod pattern;
    mod sheetmetal;
    mod surfacing;
    mod transform;
    pub(super) use advanced::plane_of;

    /// P3D.1: whether a face reference still finds its face (see `missing_inputs`).
    pub(super) fn face_resolves(state: &State, f: &crate::document::FaceRef) -> bool {
        face_ids(state, f).is_some()
    }

    /// P3D.1: whether a fillet's or chamfer's edge or face still resolves.
    pub(super) fn entity_resolves(state: &State, e: &crate::applied::EdgeOrFace) -> bool {
        match e {
            crate::applied::EdgeOrFace::Edge(r) => applied::edge_ids(state, r).is_some(),
            crate::applied::EdgeOrFace::Face(f) => face_ids(state, f).is_some(),
        }
    }

    /// Degrees to radians.
    fn rad(deg: f64) -> f64 {
        deg.to_radians()
    }

    impl Rebuilder {
        pub(super) fn extrude(
            &mut self,
            before: &[Feature],
            id: FeatureId,
            e: &ExtrudeFeature,
            state: &Arc<State>,
        ) -> Result<Output, String> {
            if let Some(p) = e.problem() {
                return Err(p.into());
            }
            let (groups, missing) = extrude_groups(before, e);
            let faces = self.face_inputs(&e.faces, state)?;
            if groups.is_empty() && faces.is_empty() {
                return Err(if e.faces.is_empty() {
                    "The selected sketch regions no longer exist".into()
                } else {
                    "The selected faces no longer exist".into()
                });
            }
            let missing_error = (missing > 0).then(|| {
                if missing == 1 {
                    "1 selected sketch region no longer exists".to_string()
                } else {
                    format!("{missing} selected sketch regions no longer exist")
                }
            });
            let op = id.0;
            let scene: Vec<BodyId> = state
                .parts
                .iter()
                .filter(|p| p.part.kind == PartKind::Solid)
                .filter_map(|p| p.body)
                .collect();
            let body_kind = match e.body {
                BodyType::Solid => cadrs_kernel::BodyKind::Solid,
                BodyType::Surface => cadrs_kernel::BodyKind::Surface,
                BodyType::Thin => {
                    let (left, right) = e.thin.sides();
                    cadrs_kernel::BodyKind::Thin { left, right }
                }
            };
            let explicit_dir = e
                .direction
                .map(|d| self.direction(before, state, &d))
                .transpose()?;
            let first = self.end_of(state, e.end, e.depth, &e.up_to, &e.offset)?;
            let second = match (&e.second, e.symmetric) {
                (Some(s), false) => Some(self.end_of(state, s.end, s.depth, &s.up_to, &s.offset)?),
                _ => None,
            };
            let start_offset = e.start_offset.as_ref().map_or(0.0, Offset::signed);
            // One sweep per sketch (and one per picked face), fused.
            let mut sweeps: Vec<(cadrs_kernel::Profile, Vec<FaceInput>)> = groups
                .iter()
                .map(|g| (crate::brep::profile(g), Vec::new()))
                .collect();
            for (plane, input) in faces {
                sweeps.push((cadrs_kernel::Profile::new(plane, vec![]), vec![input]));
            }
            let mut made: Vec<(BodyId, BodyNames)> = Vec::new();
            let mut dir0 = None;
            for (profile, inputs) in &sweeps {
                let n = profile.plane.normal;
                let dir = explicit_dir.unwrap_or(n);
                let dir = if e.flip { -dir } else { dir };
                dir0.get_or_insert(dir);
                let spec = ExtrudeSpec {
                    body: body_kind,
                    direction: dir,
                    start_offset,
                    end: first,
                    symmetric: e.symmetric,
                    second,
                    scene: scene.clone(),
                    faces: inputs.clone(),
                };
                let names_of = |k: &mut dyn Kernel, r: &OpResult| -> cadrs_kernel::Result<(BodyId, BodyNames)> {
                    let body = r.bodies[0];
                    let inputs: Vec<(BodyId, &BodyNames)> = state
                        .parts
                        .iter()
                        .filter_map(|p| Some((p.body?, &*p.names)))
                        .collect();
                    Ok((body, naming::name_body(k, body, op, &r.history, &inputs)?))
                };
                // P3.10 (PS4.9): Draft leans the sides of a solid extrude.
                let made_body = match (&e.draft, e.body) {
                    (Some(d), BodyType::Solid) => self.drafted_extrude(profile, &spec, d.signed()),
                    _ => self.kernel.extrude_with(profile, &spec),
                };
                match made_body.and_then(|r| names_of(&mut self.kernel, &r)) {
                    Ok(b) => made.push(b),
                    Err(err) => {
                        for (b, _) in made {
                            self.kernel.release(b);
                        }
                        return Err(format!("Extrude failed: {err}"));
                    }
                }
            }
            // The swept geometry of this extrude, for its faces' names, frames and silhouettes.
            let n0 = dir0.map_or([0.0, 0.0, 1.0], |d| [d.x, d.y, d.z]);
            let mut geoms = (*state.geoms).clone();
            geoms.insert(op, Arc::new(crate::brep::OpGeom::new(&groups, n0, e.depth)));
            let geoms = Arc::new(geoms);
            // One tool body.
            let tool = match made.len() {
                1 => made.pop().expect("one"),
                _ => {
                    let (first_b, _) = made[0];
                    let rest: Vec<BodyId> = made[1..].iter().map(|(b, _)| *b).collect();
                    let merged = self.kernel.boolean(BoolOp::Union, first_b, &rest).and_then(|r| {
                        let body = r.bodies[0];
                        let inputs: Vec<(BodyId, &BodyNames)> = made.iter().map(|(b, n)| (*b, n)).collect();
                        Ok((body, naming::name_body(&self.kernel, body, op, &r.history, &inputs)?))
                    });
                    for (b, _) in &made {
                        self.kernel.release(*b);
                    }
                    merged.map_err(|err| format!("Extrude failed: {err}"))?
                }
            };
            let merge = Merge {
                op: e.op,
                merge_all: e.merge_all,
                scope: &e.merge_scope,
                surface: e.body == BodyType::Surface,
            };
            self.combine(id, &merge, tool, state, geoms).map(|mut o| {
                o.warning = o.warning.or(missing_error);
                o
            })
        }

        /// A new body's boolean with the parts (New, Add, Remove, Intersect), with what it
        /// touches (the automatic Add) and, for an Add being edited, the dialog's preview.
        fn combine(
            &mut self,
            id: FeatureId,
            e: &Merge,
            tool: (BodyId, BodyNames),
            state: &Arc<State>,
            geoms: Arc<crate::brep::Geoms>,
        ) -> Result<Output, String> {
            let op = id.0;
            let surface = e.surface;
            let contacts = if surface {
                Contacts::default()
            } else {
                self.contacts(tool.0, state)
            };
            // The dialog's preview of an Add: the new body over the parts as they were.
            let joins = e.merge_all || !e.scope.is_empty() || !contacts.touches.is_empty();
            let stage = (self.stage_for == Some(id) && e.op == BooleanOp::Add && !surface && joins)
                .then(|| crate::brep::solid_of(&self.kernel, tool.0, &tool.1, &geoms, Some(op)).ok())
                .flatten()
                .map(|s| Stage {
                    before: state.parts.iter().map(|p| p.part.clone()).collect(),
                    tool: Arc::new(s),
                });
            let result = self.apply_extrude_op(id, e, tool, state, &contacts, geoms);
            result.map(|mut o| {
                o.stage = stage;
                o.contacts = Some(contacts);
                o
            })
        }

        /// Applies a new body's boolean to the parts (`tool` is released).
        fn apply_extrude_op(
            &mut self,
            id: FeatureId,
            e: &Merge,
            tool: (BodyId, BodyNames),
            state: &Arc<State>,
            contacts: &Contacts,
            geoms: Arc<crate::brep::Geoms>,
        ) -> Result<Output, String> {
            let op = id.0;
            let surface = e.surface;
            let solids: Vec<PartId> = state
                .parts
                .iter()
                .filter(|p| p.part.kind == PartKind::Solid)
                .map(|p| p.part.id)
                .collect();
            let scope: Vec<PartId> = if e.merge_all {
                solids.clone()
            } else if !e.scope.is_empty() {
                e.scope.iter().copied().filter(|p| solids.contains(p)).collect()
            } else {
                match e.op {
                    BooleanOp::Add => contacts.touches.clone(),
                    _ => contacts.overlaps.clone(),
                }
            };
            let mut next = State {
                geoms: geoms.clone(),
                ..(**state).clone()
            };
            let result = (|| -> Result<Vec<Placed>, String> {
                if surface && e.op != BooleanOp::New {
                    return Err("Surfaces can only be New".into());
                }
                match e.op {
                    BooleanOp::New => self.new_parts(id, &tool, &mut next),
                    BooleanOp::Add if scope.is_empty() => self.new_parts(id, &tool, &mut next),
                    BooleanOp::Add => self.add(id, &tool, &scope, &mut next),
                    BooleanOp::Remove | BooleanOp::Intersect => {
                        if scope.is_empty() {
                            return Err(if e.op == BooleanOp::Remove {
                                "Nothing to remove: the extrude doesn't reach a part".into()
                            } else {
                                "Nothing to intersect: the extrude doesn't reach a part".into()
                            });
                        }
                        let kind = if e.op == BooleanOp::Remove { BoolOp::Subtract } else { BoolOp::Intersect };
                        let mut placed = Vec::new();
                        for part in &scope {
                            placed.extend(self.cut(id, *part, kind, &[&tool], &mut next)?);
                        }
                        Ok(placed)
                    }
                }
            })();
            self.kernel.release(tool.0);
            let placed = result?;
            self.finish(id, placed, next, op, geoms, if surface { PartKind::Surface } else { PartKind::Solid })
        }

        /// Turns placed pieces into parts (meshes, mass properties) in `next`.
        fn finish(
            &mut self,
            id: FeatureId,
            placed: Vec<Placed>,
            mut next: State,
            op: cadrs_kernel::OpId,
            geoms: Arc<crate::brep::Geoms>,
            kind: PartKind,
        ) -> Result<Output, String> {
            let mut owned = Vec::new();
            let mut built = Vec::new();
            for (pid, piece) in placed {
                let solid = crate::brep::solid_of(&self.kernel, piece.body, &piece.names, &geoms, Some(op));
                let mass = self.kernel.mass_properties(piece.body).map_err(|e| e.to_string());
                match (solid, mass) {
                    (Ok(s), Ok(m)) => {
                        owned.push(piece.body);
                        built.push((pid, piece, s, m));
                    }
                    (Err(err), _) | (_, Err(err)) => {
                        self.kernel.release(piece.body);
                        for b in owned {
                            self.kernel.release(b);
                        }
                        for (_, p, _, _) in built {
                            self.kernel.release(p.body);
                        }
                        return Err(format!("Failed: {err}"));
                    }
                }
            }
            for (pid, piece, solid, mass) in built {
                let names = Arc::new(piece.names);
                match next.parts.iter_mut().find(|p| p.part.id == pid) {
                    Some(p) => {
                        p.body = Some(piece.body);
                        p.names = names;
                        p.part.solid = Arc::new(solid);
                        p.part.mass = Some(mass);
                        if !p.part.features.contains(&id) {
                            p.part.features.push(id);
                        }
                    }
                    None => {
                        let name = match kind {
                            PartKind::Solid => {
                                next.next_part += 1;
                                format!("Part {}", next.next_part)
                            }
                            PartKind::Surface => {
                                next.next_surface += 1;
                                format!("Surface {}", next.next_surface)
                            }
                        };
                        next.parts.push(PartState {
                            part: Part {
                                id: pid,
                                feature: pid.feature,
                                name,
                                kind,
                                palette: next.next_part + next.next_surface - 1,
                                solid: Arc::new(solid),
                                mass: Some(mass),
                                features: vec![id],
                                source: None,
                                derived: None,
                            },
                            body: Some(piece.body),
                            names,
                        });
                    }
                }
            }
            Ok(Output {
                state: Arc::new(next),
                error: None,
                warning: None,
                contacts: None,
                owned,
                stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
            })
        }

        /// A free id for a new part of feature `id`.
        fn new_id(id: FeatureId, next: &State, taken: &[PartId]) -> PartId {
            let mut i = 0;
            loop {
                let p = PartId::new(id, i);
                if next.part(p).is_none() && !taken.contains(&p) {
                    return p;
                }
                i += 1;
            }
        }

        /// New: one part per solid of the tool (a surface stays one body).
        fn new_parts(&mut self, id: FeatureId, tool: &(BodyId, BodyNames), next: &mut State) -> Result<Vec<Placed>, String> {
            let pieces = self.split(tool.0, id.0, &[(tool.0, &tool.1)])?;
            let mut placed: Vec<Placed> = Vec::new();
            for p in pieces {
                let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                placed.push((Self::new_id(id, next, &taken), p));
            }
            Ok(placed)
        }

        /// Add: the tool joined to the parts of `scope`. A piece keeps the id of the first part
        /// in the list it has faces of; the parts it absorbed are gone.
        fn add(
            &mut self,
            id: FeatureId,
            tool: &(BodyId, BodyNames),
            scope: &[PartId],
            next: &mut State,
        ) -> Result<Vec<Placed>, String> {
            let parts: Vec<PartState> = next
                .parts
                .iter()
                .filter(|p| scope.contains(&p.part.id))
                .cloned()
                .collect();
            let Some(first) = parts.first().and_then(|p| p.body) else {
                return self.new_parts(id, tool, next);
            };
            let mut others: Vec<BodyId> = parts[1..].iter().filter_map(|p| p.body).collect();
            others.push(tool.0);
            let mut inputs: Vec<(BodyId, &BodyNames)> = parts
                .iter()
                .filter_map(|p| Some((p.body?, &*p.names)))
                .collect();
            inputs.push((tool.0, &tool.1));
            let r = self
                .kernel
                .boolean(BoolOp::Union, first, &others)
                .map_err(|e| format!("Add failed: {e}"))?;
            let pieces = self.split_result(r, id.0, &inputs)?;
            let mut placed: Vec<Placed> = Vec::new();
            let mut used: Vec<PartId> = Vec::new();
            for piece in pieces {
                let owner = parts
                    .iter()
                    .find(|p| p.body.is_some_and(|b| piece.from.contains(&b)) && !used.contains(&p.part.id))
                    .map(|p| p.part.id);
                let pid = match owner {
                    Some(o) => o,
                    None => {
                        let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                        Self::new_id(id, next, &taken)
                    }
                };
                used.push(pid);
                placed.push((pid, piece));
            }
            // The parts it absorbed.
            next.parts.retain(|p| !scope.contains(&p.part.id) || used.contains(&p.part.id));
            Ok(placed)
        }

        /// Remove or Intersect `tools` with the part `part`: its largest piece keeps its id,
        /// the others are new parts; a part with nothing left is gone.
        fn cut(
            &mut self,
            id: FeatureId,
            part: PartId,
            kind: BoolOp,
            tools: &[&(BodyId, BodyNames)],
            next: &mut State,
        ) -> Result<Vec<Placed>, String> {
            let Some(p) = next.part(part).cloned() else {
                return Ok(Vec::new());
            };
            let Some(body) = p.body else {
                return Ok(Vec::new());
            };
            let tool_bodies: Vec<BodyId> = tools.iter().map(|t| t.0).collect();
            let mut inputs: Vec<(BodyId, &BodyNames)> = vec![(body, &*p.names)];
            inputs.extend(tools.iter().map(|t| (t.0, &t.1)));
            let what = if kind == BoolOp::Subtract { "Remove" } else { "Intersect" };
            let r = match self.kernel.boolean(kind, body, &tool_bodies) {
                Ok(r) => r,
                // Nothing left of the part (an empty result).
                Err(cadrs_kernel::KernelError::OperationFailed(m)) if m.contains("empty") => {
                    next.parts.retain(|q| q.part.id != part);
                    return Ok(Vec::new());
                }
                Err(e) => return Err(format!("{what} failed: {e}")),
            };
            let mut pieces = self.split_result(r, id.0, &inputs)?;
            if pieces.is_empty() {
                next.parts.retain(|q| q.part.id != part);
                return Ok(Vec::new());
            }
            pieces.sort_by(|a, b| b.volume.total_cmp(&a.volume));
            let mut placed: Vec<Placed> = Vec::new();
            for (i, piece) in pieces.into_iter().enumerate() {
                let pid = if i == 0 {
                    part
                } else {
                    let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                    Self::new_id(id, next, &taken)
                };
                placed.push((pid, piece));
            }
            Ok(placed)
        }

        /// Names a boolean's result and splits it into its solids.
        fn split_result(
            &mut self,
            r: OpResult,
            op: cadrs_kernel::OpId,
            inputs: &[(BodyId, &BodyNames)],
        ) -> Result<Vec<Piece>, String> {
            let body = r.bodies[0];
            let names = naming::name_body(&self.kernel, body, op, &r.history, inputs).map_err(|e| e.to_string());
            let names = match names {
                Ok(n) => n,
                Err(e) => {
                    self.kernel.release(body);
                    return Err(e);
                }
            };
            // Which input body each face of the result continues.
            let mut from_face: HashMap<u64, BodyId> = HashMap::new();
            for (f, input) in &r.history.modified {
                from_face.entry(f.0).or_insert(input.body);
            }
            let pieces = self.split_named(body, op, &names, &from_face);
            self.kernel.release(body);
            pieces
        }

        /// Splits a named body into its solids (each named from it), with the input bodies
        /// (`from_face`: by face of `body`) each piece has faces of.
        fn split_named(
            &mut self,
            body: BodyId,
            op: cadrs_kernel::OpId,
            names: &BodyNames,
            from_face: &HashMap<u64, BodyId>,
        ) -> Result<Vec<Piece>, String> {
            let parts = self.kernel.split_solids(body).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for r in parts {
                let b = r.bodies[0];
                let named = naming::name_body(&self.kernel, b, op, &r.history, &[(body, names)]);
                let volume = self.kernel.mass_properties(b).map(|m| m.volume).unwrap_or(0.0);
                match named {
                    Ok(n) => {
                        let from = r
                            .history
                            .modified
                            .iter()
                            .filter_map(|(_, input)| from_face.get(&input.face.0).copied())
                            .collect();
                        out.push(Piece {
                            body: b,
                            names: n,
                            from,
                            volume,
                        });
                    }
                    Err(e) => {
                        self.kernel.release(b);
                        for p in out {
                            self.kernel.release(p.body);
                        }
                        return Err(e.to_string());
                    }
                }
            }
            Ok(out)
        }

        /// Splits a new body into its solids.
        fn split(
            &mut self,
            body: BodyId,
            op: cadrs_kernel::OpId,
            inputs: &[(BodyId, &BodyNames)],
        ) -> Result<Vec<Piece>, String> {
            let names = &inputs[0].1;
            self.split_named(body, op, names, &HashMap::new())
        }

        /// Which parts the new body touches (Add) and overlaps (Remove, Intersect).
        fn contacts(&mut self, tool: BodyId, state: &State) -> Contacts {
            let mut out = Contacts::default();
            let Ok(tb) = self.kernel.bounding_box(tool) else {
                return out;
            };
            let tol = 1e-6 * (1.0 + tb.diagonal());
            for p in &state.parts {
                let (Some(b), PartKind::Solid) = (p.body, p.part.kind) else {
                    continue;
                };
                let Ok(pb) = self.kernel.bounding_box(b) else { continue };
                let apart = (0..3).any(|i| tb.min[i] > pb.max[i] + tol || pb.min[i] > tb.max[i] + tol);
                if apart {
                    continue;
                }
                // Touching or overlapping: their union is one solid.
                if self.kernel.joins(b, tool).unwrap_or(false) {
                    out.touches.push(p.part.id);
                }
                if let Ok(r) = self.kernel.boolean(BoolOp::Intersect, b, &[tool]) {
                    let v = self.kernel.mass_properties(r.bodies[0]).map(|m| m.volume).unwrap_or(0.0);
                    self.kernel.release(r.bodies[0]);
                    if v > 1e-9 {
                        out.overlaps.push(p.part.id);
                    }
                }
            }
            out
        }

        /// The kernel end for an end type.
        fn end_of(
            &self,
            state: &State,
            end: EndType,
            depth: f64,
            up_to: &Option<UpTo>,
            offset: &Option<Offset>,
        ) -> Result<ExtrudeEnd, String> {
            let offset = offset.as_ref().map_or(0.0, Offset::signed);
            let lost = |what: &str| format!("The {what} to extrude up to no longer exists");
            Ok(match end {
                EndType::Blind => ExtrudeEnd::Blind(depth),
                EndType::ThroughAll => ExtrudeEnd::ThroughAll,
                EndType::UpToNext => ExtrudeEnd::UpToNext { offset },
                EndType::UpToPart => {
                    let Some(UpTo::Part(pid)) = up_to else {
                        return Err("Select a part to extrude up to".into());
                    };
                    let body = state.part(*pid).and_then(|p| p.body).ok_or_else(|| lost("part"))?;
                    ExtrudeEnd::UpToPart { body, offset }
                }
                EndType::UpToFace => {
                    let Some(UpTo::Face(f)) = up_to else {
                        return Err("Select a face to extrude up to".into());
                    };
                    let (part, faces) = face_ids(state, f).ok_or_else(|| lost("face"))?;
                    let body = part.body.ok_or_else(|| lost("face"))?;
                    // The largest piece of the face.
                    let infos = self.kernel.faces(body).map_err(|e| e.to_string())?;
                    let face = faces
                        .iter()
                        .copied()
                        .max_by(|a, b| {
                            let area = |f: &cadrs_kernel::FaceId| infos.iter().find(|i| i.id == *f).map_or(0.0, |i| i.area);
                            area(a).total_cmp(&area(b))
                        })
                        .ok_or_else(|| lost("face"))?;
                    ExtrudeEnd::UpToFace { body, face, offset }
                }
                EndType::UpToVertex => {
                    let Some(UpTo::Vertex(v)) = up_to else {
                        return Err("Select a vertex to extrude up to".into());
                    };
                    let solid = state
                        .part(v.part)
                        .map(|p| p.part.solid.clone())
                        .ok_or_else(|| lost("vertex"))?;
                    let point = solid
                        .vertex(&v.vertex)
                        .map(|x| x.point)
                        .or_else(|| {
                            // Renamed: the vertex still at the stored point.
                            solid
                                .vertices
                                .iter()
                                .find(|x| crate::solid::dist3(x.point, v.point) < 1e-6)
                                .map(|x| x.point)
                        })
                        .ok_or_else(|| lost("vertex"))?;
                    ExtrudeEnd::UpToVertex {
                        point: Point3::from(point),
                        offset,
                    }
                }
            })
        }

        /// The picked faces as kernel inputs, each with its plane (the face's own frame).
        fn face_inputs(&self, faces: &[crate::document::FaceRef], state: &State) -> Result<Vec<(cadrs_kernel::Plane, FaceInput)>, String> {
            let mut out = Vec::new();
            for f in faces {
                let Some((part, ids)) = face_ids(state, f) else {
                    continue;
                };
                let (Some(body), Some(frame)) = (part.body, face_frame(part, f)) else {
                    continue;
                };
                let n = cadrs_sketch::Vec3::from(frame.normal());
                let plane = cadrs_kernel::Plane {
                    origin: Point3::from(frame.origin),
                    x_dir: Unit::new_normalize(Vector3::from(frame.u)),
                    normal: Unit::new_normalize(Vector3::from(n)),
                };
                let source = naming::stable_hash(format!("{:?}", f.face).as_bytes());
                for face in ids {
                    out.push((plane, FaceInput { body, face, source }));
                }
            }
            Ok(out)
        }

        /// The unit direction a Direction reference gives.
        pub(super) fn direction(&self, before: &[Feature], state: &State, d: &DirectionRef) -> Result<Unit<Vector3<f64>>, String> {
            let lost = || "The extrude direction no longer exists".to_string();
            let v: [f64; 3] = match d {
                DirectionRef::Edge(r) => {
                    let part = state.part(r.part).ok_or_else(lost)?;
                    let edge = part.part.solid.edge(&r.edge).ok_or_else(lost)?;
                    let (a, b) = (edge.points[0], *edge.points.last().ok_or_else(lost)?);
                    let straight = edge.points.iter().all(|p| {
                        let ab = crate::solid::sub3(b, a);
                        let ap = crate::solid::sub3(*p, a);
                        let c = crate::solid::cross3(ab, ap);
                        crate::solid::dot3(c, c).sqrt() < 1e-6 * crate::solid::dot3(ab, ab).max(1e-12)
                    });
                    if !straight {
                        return Err("The extrude direction must be a straight edge".into());
                    }
                    crate::solid::sub3(b, a)
                }
                DirectionRef::SketchLine { sketch, curve } => {
                    let sk = before.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
                    let frame = sk.plane.ok_or_else(lost)?.frame();
                    let CurveKind::Line { a, b } = sk.geometry.curves.get(*curve).ok_or_else(lost)?.kind else {
                        return Err("The extrude direction must be a line".into());
                    };
                    crate::solid::sub3(frame.to_world(sk.geometry.pos(b)), frame.to_world(sk.geometry.pos(a)))
                }
                DirectionRef::FaceNormal(f) => {
                    let part = state.part(f.part).ok_or_else(lost)?;
                    face_frame(part, f).ok_or_else(lost)?.normal()
                }
                DirectionRef::PlaneNormal(p) => plane_of(state, p).ok_or_else(lost)?.normal(),
                DirectionRef::Connector(c) => connector_frame(before, state, c)?.normal(),
            };
            let v = Vector3::from(v);
            if v.norm() < 1e-12 {
                return Err(lost());
            }
            Ok(Unit::new_normalize(v))
        }

        /// A revolve (P3.4, PS7): the regions (or, for surface and thin revolves of whole
        /// sketches, the curves) turned about the axis, then combined with the parts like an
        /// extrude.
        pub(super) fn revolve(
            &mut self,
            before: &[Feature],
            id: FeatureId,
            r: &RevolveFeature,
            state: &Arc<State>,
        ) -> Result<Output, String> {
            if let Some(p) = r.problem() {
                return Err(p.into());
            }
            let (groups, missing) = sweep_groups(before, &r.regions, &r.sketches, r.body);
            let faces = self.face_inputs(&r.faces, state)?;
            if groups.is_empty() && faces.is_empty() {
                return Err(if r.faces.is_empty() {
                    "The selected sketch regions no longer exist".into()
                } else {
                    "The selected faces no longer exist".into()
                });
            }
            let missing_error = (missing > 0).then(|| {
                if missing == 1 {
                    "1 selected sketch region no longer exists".to_string()
                } else {
                    format!("{missing} selected sketch regions no longer exist")
                }
            });
            let op = id.0;
            let axis = self.axis(before, state, &r.axis.ok_or("Select a revolve axis")?)?;
            let axis = if r.flip {
                cadrs_kernel::Axis { dir: -axis.dir, ..axis }
            } else {
                axis
            };
            let scene: Vec<BodyId> = state
                .parts
                .iter()
                .filter(|p| p.part.kind == PartKind::Solid)
                .filter_map(|p| p.body)
                .collect();
            let body_kind = match r.body {
                BodyType::Solid => cadrs_kernel::BodyKind::Solid,
                BodyType::Surface => cadrs_kernel::BodyKind::Surface,
                BodyType::Thin => {
                    let (left, right) = r.thin.sides();
                    cadrs_kernel::BodyKind::Thin { left, right }
                }
            };
            let to_end = |this: &Self, end: EndType, angle: f64, up_to: &Option<UpTo>, offset: &Option<Offset>| {
                let offset = offset.as_ref().map_or(0.0, |o| rad(o.signed()));
                Ok::<_, String>(match this.end_of(state, end, 1.0, up_to, &None)? {
                    ExtrudeEnd::UpToNext { .. } => RevolveEnd::UpToNext { offset },
                    ExtrudeEnd::UpToFace { body, face, .. } => RevolveEnd::UpToFace { body, face, offset },
                    ExtrudeEnd::UpToPart { body, .. } => RevolveEnd::UpToPart { body, offset },
                    ExtrudeEnd::UpToVertex { point, .. } => RevolveEnd::UpToVertex { point, offset },
                    ExtrudeEnd::Blind(_) | ExtrudeEnd::ThroughAll => RevolveEnd::Angle(rad(angle)),
                })
            };
            let end = to_end(self, r.kind.end_type(), r.angle, &r.up_to, &r.offset)?;
            let second = match (&r.second, r.kind.one_sided()) {
                (Some(s), true) => Some(to_end(self, s.end, s.depth, &s.up_to, &s.offset)?),
                _ => None,
            };
            let spec = RevolveSpec {
                body: body_kind,
                axis,
                full: r.kind == RevolveType::Full,
                end,
                symmetric: r.kind == RevolveType::Symmetric,
                second,
                scene,
                faces: Vec::new(),
            };
            // One revolve per sketch (and one per picked face), fused.
            let mut sweeps: Vec<(cadrs_kernel::Profile, Vec<FaceInput>)> =
                groups.iter().map(|g| (crate::brep::profile(g), Vec::new())).collect();
            for (plane, input) in faces {
                sweeps.push((cadrs_kernel::Profile::new(plane, vec![]), vec![input]));
            }
            let mut made: Vec<(BodyId, BodyNames)> = Vec::new();
            for (profile, inputs) in &sweeps {
                let spec = RevolveSpec { faces: inputs.clone(), ..spec.clone() };
                let named = self.kernel.revolve_with(profile, &spec).and_then(|res| {
                    let body = res.bodies[0];
                    let inputs: Vec<(BodyId, &BodyNames)> =
                        state.parts.iter().filter_map(|p| Some((p.body?, &*p.names))).collect();
                    Ok((body, naming::name_body(&self.kernel, body, op, &res.history, &inputs)?))
                });
                match named {
                    Ok(b) => made.push(b),
                    Err(err) => {
                        for (b, _) in made {
                            self.kernel.release(b);
                        }
                        return Err(format!("Revolve failed: {err}"));
                    }
                }
            }
            // The swept geometry: where the revolve starts and how far it turns (a whole turn
            // for the ends that stop at a target).
            let angle_of = |e: &RevolveEnd| match e {
                RevolveEnd::Angle(a) => *a,
                _ => std::f64::consts::TAU,
            };
            let (start, sweep) = if spec.full {
                (0.0, std::f64::consts::TAU)
            } else if spec.symmetric {
                (-angle_of(&spec.end) / 2.0, angle_of(&spec.end))
            } else {
                let b = spec.second.as_ref().map_or(0.0, angle_of);
                (-b, (angle_of(&spec.end) + b).min(std::f64::consts::TAU))
            };
            let mut geoms = (*state.geoms).clone();
            let (o, d) = (axis.origin, axis.dir);
            let o_ = o;
            geoms.insert(
                op,
                Arc::new(crate::brep::OpGeom::revolve(&groups, [o.x, o.y, o.z], [d.x, d.y, d.z], start, sweep)),
            );
            let geoms = Arc::new(geoms);
            let tool = match made.len() {
                1 => made.pop().expect("one"),
                _ => {
                    let (first_b, _) = made[0];
                    let rest: Vec<BodyId> = made[1..].iter().map(|(b, _)| *b).collect();
                    let merged = self.kernel.boolean(BoolOp::Union, first_b, &rest).and_then(|res| {
                        let body = res.bodies[0];
                        let inputs: Vec<(BodyId, &BodyNames)> = made.iter().map(|(b, n)| (*b, n)).collect();
                        Ok((body, naming::name_body(&self.kernel, body, op, &res.history, &inputs)?))
                    });
                    for (b, _) in &made {
                        self.kernel.release(*b);
                    }
                    merged.map_err(|err| format!("Revolve failed: {err}"))?
                }
            };
            let merge = Merge {
                op: r.op,
                merge_all: r.merge_all,
                scope: &r.merge_scope,
                surface: r.body == BodyType::Surface,
            };
            self.combine(id, &merge, tool, state, geoms).map(|mut o| {
                o.warning = o.warning.or(missing_error);
                o.axis = Some(([o_.x, o_.y, o_.z], [d.x, d.y, d.z]));
                o
            })
        }

        /// The axis a revolve axis reference gives (exact, from the sketch or the kernel).
        fn axis(&self, before: &[Feature], state: &State, a: &AxisRef) -> Result<cadrs_kernel::Axis, String> {
            let lost = || "The revolve axis no longer exists".to_string();
            let make = |o: [f64; 3], d: [f64; 3]| -> Result<cadrs_kernel::Axis, String> {
                let d = Vector3::from(d);
                if d.norm() < 1e-12 {
                    return Err(lost());
                }
                Ok(cadrs_kernel::Axis {
                    origin: Point3::from(o),
                    dir: Unit::new_normalize(d),
                })
            };
            match a {
                AxisRef::SketchCurve { sketch, curve } => {
                    let sk = before.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
                    let frame = sk.plane.ok_or_else(lost)?.frame();
                    let g = &sk.geometry;
                    match g.curves.get(*curve).ok_or_else(lost)?.kind {
                        CurveKind::Line { a, b } => {
                            let (pa, pb) = (frame.to_world(g.pos(a)), frame.to_world(g.pos(b)));
                            make(pa, crate::solid::sub3(pb, pa))
                        }
                        CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. } => {
                            make(frame.to_world(g.pos(center)), frame.normal())
                        }
                        CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => Err("An ellipse can't be a revolve axis".into()),
                        CurveKind::Spline { .. } => Err("A spline can't be a revolve axis".into()),
                        CurveKind::Bezier { .. } => Err("A Bézier curve can't be a revolve axis".into()),
                    }
                }
                AxisRef::Edge(r) => {
                    // The part with the edge (a boolean may have joined its part to another).
                    let part = state
                        .part(r.part)
                        .filter(|p| p.part.solid.edge(&r.edge).is_some())
                        .or_else(|| state.parts.iter().find(|p| p.part.solid.edge(&r.edge).is_some()))
                        .ok_or_else(lost)?;
                    // The exact edge from the kernel.
                    if let Some(body) = part.body {
                        let ids = part.names.edges_named(&r.edge);
                        let infos = self.kernel.edges(body).map_err(|e| e.to_string())?;
                        if let Some(info) = ids.first().and_then(|id| infos.iter().find(|i| i.id == *id)) {
                            if let Some(c) = info.circle {
                                let (o, n) = (c.center, c.normal.into_inner());
                                return make([o.x, o.y, o.z], [n.x, n.y, n.z]);
                            }
                            if info.curve == cadrs_kernel::CurveKind::Line {
                                let (s, e) = (info.start, info.end);
                                return make([s.x, s.y, s.z], [e.x - s.x, e.y - s.y, e.z - s.z]);
                            }
                            return Err("The revolve axis must be a straight or circular edge".into());
                        }
                    }
                    // Else from its polyline.
                    let edge = part.part.solid.edge(&r.edge).ok_or_else(lost)?;
                    match crate::links::edge_curve(edge).ok_or_else(lost)? {
                        crate::links::Curve3::Line(p, q) => make(p, crate::solid::sub3(q, p)),
                        crate::links::Curve3::Circle { center, normal, .. }
                        | crate::links::Curve3::Arc { center, normal, .. } => make(center, normal),
                        crate::links::Curve3::Ellipse { .. } => Err(lost()),
                    }
                }
                AxisRef::Connector(c) => {
                    let f = connector_frame(before, state, c)?;
                    make(f.origin, f.normal())
                }
                AxisRef::Face(f) => {
                    let (part, ids) = face_ids(state, f).ok_or_else(lost)?;
                    let body = part.body.ok_or_else(lost)?;
                    let infos = self.kernel.faces(body).map_err(|e| e.to_string())?;
                    let axis = ids
                        .iter()
                        .find_map(|id| infos.iter().find(|i| i.id == *id)?.axis)
                        .ok_or("The revolve axis must be a cylindrical or conical face")?;
                    Ok(axis)
                }
            }
        }

        /// The Boolean feature (PS5.5).
        pub(super) fn boolean(&mut self, id: FeatureId, b: &BooleanFeature, state: &Arc<State>) -> Result<Output, String> {
            if let Some(p) = b.problem() {
                return Err(p.into());
            }
            let lost = |p: &PartId| state.part(*p).is_none();
            if b.tools.iter().chain(&b.targets).any(lost) {
                return Err("A selected part no longer exists".into());
            }
            let op = id.0;
            let mut next = (**state).clone();
            let tools: Vec<PartState> = b.tools.iter().filter_map(|p| state.part(*p).cloned()).collect();
            let mut nothing_to_union = false;
            let placed = match b.op {
                BooleanKind::Union | BooleanKind::Intersect => {
                    let kind = if b.op == BooleanKind::Union { BoolOp::Union } else { BoolOp::Intersect };
                    let first = &tools[0];
                    let rest: Vec<BodyId> = tools[1..].iter().filter_map(|t| t.body).collect();
                    let inputs: Vec<(BodyId, &BodyNames)> =
                        tools.iter().filter_map(|t| Some((t.body?, &*t.names))).collect();
                    let r = self
                        .kernel
                        .boolean(kind, first.body.ok_or("A part has no body")?, &rest)
                        .map_err(|e| format!("{} failed: {e}", b.op.label()))?;
                    let mut pieces = self.split_result(r, op, &inputs)?;
                    if pieces.is_empty() {
                        return Err(format!("{}: nothing is left", b.op.label()));
                    }
                    pieces.sort_by(|x, y| y.volume.total_cmp(&x.volume));
                    // Each piece takes the identity of the first tool it is made of (a union of
                    // parts that don't touch leaves them as they were, as Onshape does); the
                    // tools that went into another part's piece are gone.
                    let mut placed: Vec<Placed> = Vec::new();
                    let mut merged = false;
                    for piece in pieces {
                        let owners: Vec<PartId> = tools.iter().filter(|t| t.body.is_some_and(|b| piece.from.contains(&b))).map(|t| t.part.id).collect();
                        merged |= owners.len() > 1;
                        let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                        let pid = owners.iter().copied().find(|p| !taken.contains(p)).unwrap_or_else(|| Self::new_id(id, &next, &taken));
                        placed.push((pid, piece));
                    }
                    if !b.keep_tools {
                        let kept: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                        next.parts.retain(|p| !b.tools.contains(&p.part.id) || kept.contains(&p.part.id));
                    }
                    if b.op == BooleanKind::Union && !merged && tools.len() > 1 {
                        nothing_to_union = true;
                    }
                    placed
                }
                BooleanKind::Subtract => {
                    let mut tool_refs: Vec<(BodyId, BodyNames)> = tools
                        .iter()
                        .filter_map(|t| Some((t.body?, (*t.names).clone())))
                        .collect();
                    // P3.10 (PS5.5): the offset tools cut instead (their faces keep the tools'
                    // names).
                    let mut offset_bodies: Vec<BodyId> = Vec::new();
                    if let Some(o) = &b.offset {
                        let mut grown = Vec::new();
                        for (t, (body, names)) in tools.iter().zip(&tool_refs) {
                            let faces: Vec<(cadrs_kernel::FaceId, f64)> = if o.all {
                                Vec::new()
                            } else {
                                o.faces
                                    .iter()
                                    .filter_map(|f| face_ids(state, f))
                                    .filter(|(p, _)| p.part.id == t.part.id)
                                    .flat_map(|(_, ids)| ids.into_iter().map(|i| (i, o.signed())))
                                    .collect()
                            };
                            if !o.all && faces.is_empty() {
                                continue;
                            }
                            let spec = cadrs_kernel::OffsetSpec { distance: if o.all { o.signed() } else { 0.0 }, faces, sharp: true };
                            let made = self.kernel.offset(*body, &spec).and_then(|r| {
                                let nb = r.bodies[0];
                                Ok((nb, naming::name_body(&self.kernel, nb, op, &r.history, &[(*body, names)])?))
                            });
                            match made {
                                Ok(x) => {
                                    offset_bodies.push(x.0);
                                    grown.push(x);
                                }
                                Err(e) => {
                                    for b in offset_bodies {
                                        self.kernel.release(b);
                                    }
                                    return Err(format!("Subtract failed: the offset: {e}"));
                                }
                            }
                        }
                        if !o.all {
                            // Tools with no picked face cut as they are.
                            for (t, r) in tools.iter().zip(tool_refs.iter()) {
                                let picked = o.faces.iter().filter_map(|f| face_ids(state, f)).any(|(p, _)| p.part.id == t.part.id);
                                if !picked {
                                    grown.push(r.clone());
                                }
                            }
                        }
                        tool_refs = grown;
                    }
                    let refs: Vec<&(BodyId, BodyNames)> = tool_refs.iter().collect();
                    let mut placed = Vec::new();
                    for target in &b.targets {
                        match self.cut(id, *target, BoolOp::Subtract, &refs, &mut next) {
                            Ok(p) => placed.extend(p),
                            Err(e) => {
                                for b in &offset_bodies {
                                    self.kernel.release(*b);
                                }
                                return Err(e);
                            }
                        }
                    }
                    for b in &offset_bodies {
                        self.kernel.release(*b);
                    }
                    if !b.keep_tools {
                        next.parts.retain(|p| !b.tools.contains(&p.part.id));
                    }
                    placed
                }
            };
            let geoms = state.geoms.clone();
            self.finish(id, placed, next, op, geoms, PartKind::Solid).map(|mut o| {
                if nothing_to_union {
                    o.warning = Some("The parts don't touch: nothing was joined".into());
                }
                o
            })
        }
    }

    /// The part a face reference is on and the face's kernel ids (by its current name).
    fn face_ids<'a>(state: &'a State, f: &crate::document::FaceRef) -> Option<(&'a PartState, Vec<cadrs_kernel::FaceId>)> {
        let candidates: Vec<&PartState> = state
            .part(f.part)
            .into_iter()
            .chain(state.parts.iter().filter(|p| p.part.id != f.part))
            .collect();
        // By name on any part first (a face can move to another part: the funnel's loft faces
        // when the loft goes from Add to New, PS21.11), then where the face was.
        for by_name in [true, false] {
            for part in &candidates {
                let solid = &part.part.solid;
                let Ok((i, how)) = solid.resolve_face(&f.face, None, Some(f.seed)) else {
                    continue;
                };
                if by_name && how == cadrs_kernel::naming::Match::Geometric {
                    continue;
                }
                let name = solid.faces[i].name;
                let ids = part.names.faces_named(&name);
                if !ids.is_empty() {
                    return Some((part, ids));
                }
            }
        }
        None
    }

    /// A planar face's frame (u × v is its outward normal).
    fn face_frame(part: &PartState, f: &crate::document::FaceRef) -> Option<cadrs_sketch::PlaneFrame> {
        let solid = &part.part.solid;
        let (i, _) = solid.resolve_face(&f.face, None, Some(f.seed)).ok()?;
        solid.faces[i].plane
    }
}

// ---------------------------------------------------------------------------------------------
// Profiles

/// The regions (and, for surface and thin extrudes of whole sketches, the curve chains) an
/// extrude refers to, grouped by sketch with the sketch's plane, and how many of its regions
/// could not be found (their sketch is gone, has no plane, or the region is gone).
pub fn extrude_groups(features: &[Feature], e: &ExtrudeFeature) -> (Vec<ProfileGroup>, usize) {
    sweep_groups(features, &e.regions, &e.sketches, e.body)
}

/// False when a loft profile's regions (or its whole sketch) make more than one closed contour
/// (PS20.5: the Loft dialog shows it red). Faces and points are one section each.
pub fn loft_profile_is_one_contour(features: &[Feature], p: &crate::advanced::LoftProfile) -> bool {
    use crate::advanced::LoftProfile;
    let groups = match p {
        LoftProfile::Regions { regions, .. } => sweep_groups(features, regions, &[], BodyType::Solid).0,
        LoftProfile::Sketch(s) => sweep_groups(features, &[], &[*s], BodyType::Solid).0,
        _ => return true,
    };
    groups.first().is_none_or(|g| cadrs_kernel::loft::section_loop(&crate::brep::profile(g)).is_ok())
}

/// [`extrude_groups`] for any feature that takes sketch regions and whole sketches (an extrude
/// or a revolve).
pub fn sweep_groups(
    features: &[Feature],
    regions: &[RegionRef],
    sketches: &[FeatureId],
    body: BodyType,
) -> (Vec<ProfileGroup>, usize) {
    struct E<'a> {
        regions: &'a [RegionRef],
        sketches: &'a [FeatureId],
        body: BodyType,
    }
    let e = E { regions, sketches, body };
    let mut out: Vec<(FeatureId, ProfileGroup)> = Vec::new();
    let mut missing = 0;
    let group = |out: &mut Vec<(FeatureId, ProfileGroup)>, sketch: FeatureId, frame| -> usize {
        match out.iter().position(|(id, _)| *id == sketch) {
            Some(i) => i,
            None => {
                out.push((sketch, ProfileGroup::new(frame, Vec::new())));
                out.len() - 1
            }
        }
    };
    for r in e.regions {
        let Some(sk) = features.iter().find(|f| f.id == r.sketch).and_then(|f| f.sketch()) else {
            missing += 1;
            continue;
        };
        let (Some(plane), Some(region)) = (sk.plane, r.resolve(&sk.geometry)) else {
            missing += 1;
            continue;
        };
        let i = group(&mut out, r.sketch, plane.frame());
        out[i].1.regions.push((r.key(), region));
    }
    for &s in e.sketches {
        let Some(sk) = features.iter().find(|f| f.id == s).and_then(|f| f.sketch()) else {
            missing += 1;
            continue;
        };
        let Some(plane) = sk.plane else {
            missing += 1;
            continue;
        };
        let i = group(&mut out, s, plane.frame());
        if e.body == BodyType::Solid {
            for r in whole_sketch_regions(&sk.geometry) {
                let key = RegionRef::new(s, &r).key();
                if !out[i].1.regions.iter().any(|(k, _)| *k == key) {
                    out[i].1.regions.push((key, r));
                }
            }
        } else {
            out[i].1.chains.extend(sketch_chains(s, &sk.geometry));
        }
        if out[i].1.regions.is_empty() && out[i].1.chains.is_empty() {
            out.remove(i);
            missing += 1;
        }
    }
    (out.into_iter().map(|(_, g)| g).collect(), missing)
}

/// The regions a whole sketch extrudes (PS1.1): its outer boundaries less the loops inside them.
/// A region is kept when an even number of the other regions' outer boundaries enclose it
/// (even-odd nesting: a disc inside a plate is its hole, a boss inside that hole is kept again).
pub fn whole_sketch_regions(g: &Sketch) -> Vec<Region> {
    // The sketch's own curves: a face's imprinted edges (S21) don't split a whole sketch (P3.4:
    // the Reducer Coupling's second flange, drawn on the revolve's end face, is one region with
    // its bore and holes left out, not split along the face's rim).
    let own;
    let g = if g.imprint.is_empty() {
        g
    } else {
        let mut c = g.clone();
        c.imprint.clear();
        own = c;
        &own
    };
    let all = cadrs_sketch::region::regions(g);
    let inside = |poly: &[Vec2], p: Vec2| -> bool {
        let mut c = false;
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                c = !c;
            }
        }
        c
    };
    let seeds: Vec<Vec2> = all.iter().map(crate::document::interior_point).collect();
    all.iter()
        .enumerate()
        .filter(|(i, _)| {
            let depth = all
                .iter()
                .enumerate()
                .filter(|(j, other)| j != i && inside(&other.outer, seeds[*i]))
                .count();
            depth % 2 == 0
        })
        .map(|(_, r)| r.clone())
        .collect()
}

/// A sketch's (non-construction) curves joined end to end into chains: closed ones (whole
/// circles and ellipses, and lines and arcs that come back to their start) and open ones.
pub fn sketch_chains(sketch: FeatureId, g: &Sketch) -> Vec<ChainGeom> {
    use cadrs_sketch::geom::ArcGeom;
    use slotmap::Key;
    let tau = std::f64::consts::TAU;
    let key = |c: CurveId| {
        let mut bytes = sketch.0.as_bytes().to_vec();
        bytes.extend_from_slice(&c.data().as_ffi().to_le_bytes());
        cadrs_kernel::naming::stable_hash(&bytes)
    };
    let mut out = Vec::new();
    // Ends of the lines and arcs, to chain them.
    let mut open: Vec<(CurveId, cadrs_sketch::PointId, cadrs_sketch::PointId)> = Vec::new();
    for (id, c) in &g.curves {
        if c.construction {
            continue;
        }
        match c.kind {
            CurveKind::Circle { center, radius } => out.push(ChainGeom {
                source: key(id),
                pieces: vec![(
                    Piece::Arc(ArcGeom {
                        center: g.pos(center),
                        radius,
                        start_angle: 0.0,
                        sweep: tau,
                    }),
                    id,
                )],
                closed: true,
            }),
            CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => {
                if let Some(e) = g.ellipse_geom(id) {
                    out.push(ChainGeom {
                        source: key(id),
                        pieces: vec![(Piece::Ellipse { g: e, t0: 0.0, sweep: tau }, id)],
                        closed: true,
                    });
                }
            }
            CurveKind::Line { a, b } | CurveKind::Bezier { a, b, .. } => open.push((id, a, b)),
            CurveKind::Arc { start, end, .. } => open.push((id, start, end)),
            CurveKind::Spline { start, end } if start == end => {
                let pieces: Vec<(Piece, CurveId)> =
                    g.spline_spans(id).unwrap_or_default().into_iter().map(|b| (Piece::Bezier(BezierGeom::new(b)), id)).collect();
                if !pieces.is_empty() {
                    out.push(ChainGeom { source: key(id), pieces, closed: true });
                }
            }
            CurveKind::Spline { start, end } => open.push((id, start, end)),
        }
    }
    let piece = |id: CurveId, forward: bool| -> Option<Vec<Piece>> {
        let p = match g.curves.get(id)?.kind {
            CurveKind::Line { a, b } => vec![Piece::Line(g.pos(a), g.pos(b))],
            CurveKind::Arc { .. } => vec![Piece::Arc(g.arc_geom(id)?)],
            CurveKind::Spline { .. } => g.spline_spans(id)?.into_iter().map(|b| Piece::Bezier(BezierGeom::new(b))).collect(),
            CurveKind::Bezier { .. } => vec![Piece::Bezier(g.bezier_geom(id)?)],
            _ => return None,
        };
        Some(if forward { p } else { p.iter().rev().map(Piece::reversed).collect() })
    };
    while let Some((first, a, b)) = open.pop() {
        let mut ids = vec![(first, true)];
        let (start, mut end) = (a, b);
        // Extend forward from the end, then see if it closed.
        while let Some(i) = open.iter().position(|(_, x, y)| *x == end || *y == end) {
            let (id, x, y) = open.swap_remove(i);
            if x == end {
                ids.push((id, true));
                end = y;
            } else {
                ids.push((id, false));
                end = x;
            }
            if end == start {
                break;
            }
        }
        let closed = end == start && ids.len() > 1;
        let mut start = start;
        if !closed {
            // Extend backward from the start too.
            while let Some(i) = open.iter().position(|(_, x, y)| *x == start || *y == start) {
                let (id, x, y) = open.swap_remove(i);
                if y == start {
                    ids.insert(0, (id, true));
                    start = x;
                } else {
                    ids.insert(0, (id, false));
                    start = y;
                }
            }
        }
        let pieces: Vec<(Piece, CurveId)> = ids
            .iter()
            .flat_map(|(id, fwd)| piece(*id, *fwd).unwrap_or_default().into_iter().map(|p| (p, *id)))
            .collect();
        if !pieces.is_empty() {
            out.push(ChainGeom {
                source: key(ids[0].0),
                pieces,
                closed,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The worker thread

enum Job {
    Rebuild {
        features: Vec<Feature>,
        reply: mpsc::Sender<Arc<Build>>,
        cancelled: Arc<AtomicBool>,
        /// The app's rebuild of a Part Studio: restored from and saved to the session
        /// snapshots ([`session`]); not a rebuild of part of a list (a lost-face check).
        persist: bool,
    },
    Export {
        features: Vec<Feature>,
        request: StepRequest,
        reply: mpsc::Sender<Result<Vec<StepFile>, String>>,
    },
    /// P3B.9: work on the kernel session (interference, assembly export), see [`run_on_worker`].
    Run(Box<dyn FnOnce(&mut Rebuilder) + Send>),
    Project {
        features: Vec<Feature>,
        request: Box<crate::views::ViewRequest>,
        reply: mpsc::Sender<Result<Arc<crate::views::ViewGeometry>, String>>,
        cancelled: Arc<AtomicBool>,
    },
    /// A drawing view of an assembly (P3C.5).
    ProjectAssembly {
        state: Box<crate::drawing_assembly::AssemblyState>,
        request: Box<crate::views::ViewRequest>,
        reply: mpsc::Sender<Result<Arc<crate::views::ViewGeometry>, String>>,
        cancelled: Arc<AtomicBool>,
    },
}

fn worker() -> &'static Mutex<mpsc::Sender<Job>> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("cadrs-rebuild".into())
            .spawn(move || run(rx))
            .expect("failed to start the rebuild thread");
        Mutex::new(tx)
    })
}

fn run(rx: mpsc::Receiver<Job>) {
    REPORTS_PROGRESS.with(|r| r.set(true));
    let mut rebuilder = Rebuilder::new();
    // The snapshot of the last app rebuild, written once the thread has been idle a while.
    #[cfg(feature = "occt")]
    let mut pending_save: Option<session::PendingSave> = None;
    loop {
        #[cfg(feature = "occt")]
        let job = match rx.recv_timeout(session::SAVE_AFTER) {
            Ok(job) => job,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(save) = pending_save.take() {
                    let _ = catch_unwind(AssertUnwindSafe(|| rebuilder.save_snapshot(save)));
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        #[cfg(not(feature = "occt"))]
        let Ok(job) = rx.recv() else { break };
        match job {
            Job::Rebuild { features, reply, cancelled, persist } => {
                if cancelled.load(Ordering::Relaxed) {
                    continue;
                }
                #[cfg(feature = "occt")]
                if persist {
                    let _ = catch_unwind(AssertUnwindSafe(|| rebuilder.restore_snapshot(&features)));
                }
                #[cfg(not(feature = "occt"))]
                let _ = persist;
                let result = catch_unwind(AssertUnwindSafe(|| rebuilder.rebuild(&features)));
                let build = match result {
                    Ok(b) => b,
                    Err(_) => {
                        // The session may be inconsistent after a panic: start a fresh one.
                        rebuilder = Rebuilder::new();
                        Build::failed(&features, "Internal error while rebuilding")
                    }
                };
                #[cfg(feature = "occt")]
                if persist && build.computed > 0 {
                    pending_save = rebuilder.plan_save(&features);
                }
                let _ = reply.send(Arc::new(build));
            }
            Job::Export { features, request, reply } => {
                let result = catch_unwind(AssertUnwindSafe(|| rebuilder.export_step(&features, &request)));
                let files = result.unwrap_or_else(|_| {
                    rebuilder = Rebuilder::new();
                    Err("Internal error while exporting".into())
                });
                let _ = reply.send(files);
            }
            Job::Run(f) => {
                if catch_unwind(AssertUnwindSafe(|| f(&mut rebuilder))).is_err() {
                    rebuilder = Rebuilder::new();
                }
            }
            Job::Project { features, request, reply, cancelled } => {
                // Nobody waits for it any more (the view was deleted or changed again).
                if cancelled.load(Ordering::Relaxed) {
                    continue;
                }
                let result = catch_unwind(AssertUnwindSafe(|| rebuilder.project_view(&features, &request)));
                let view = result.unwrap_or_else(|_| {
                    rebuilder = Rebuilder::new();
                    Err("Internal error while projecting the view".into())
                });
                let _ = reply.send(view.map(Arc::new));
            }
            Job::ProjectAssembly { state, request, reply, cancelled } => {
                if cancelled.load(Ordering::Relaxed) {
                    continue;
                }
                let result = catch_unwind(AssertUnwindSafe(|| rebuilder.project_assembly(&state, &request)));
                let view = result.unwrap_or_else(|_| {
                    rebuilder = Rebuilder::new();
                    Err("Internal error while projecting the assembly".into())
                });
                let _ = reply.send(view.map(Arc::new));
            }
        }
    }
}

/// Work queued on the kernel thread (P3B.9, [`run_on_worker`]).
pub struct PendingJob<T> {
    rx: Mutex<mpsc::Receiver<T>>,
}

impl<T> PendingJob<T> {
    /// The result, once it is done (`Some(None)`: the job failed or the worker stopped).
    pub fn poll(&self) -> Option<Option<T>> {
        match self.rx.lock().ok()?.try_recv() {
            Ok(r) => Some(Some(r)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
        }
    }

    /// Waits for the result (`None`: the job failed or the worker stopped).
    pub fn wait(self) -> Option<T> {
        self.rx.into_inner().ok()?.recv().ok()
    }
}

/// Runs `f` on the kernel thread, with the rebuild session (its cached parts and kernel bodies),
/// after the jobs queued before it (P3B.9: Check interference, assembly export).
pub fn run_on_worker<T: Send + 'static>(f: impl FnOnce(&mut Rebuilder) -> T + Send + 'static) -> PendingJob<T> {
    let (reply, rx) = mpsc::channel();
    let job = Job::Run(Box::new(move |r: &mut Rebuilder| {
        let _ = reply.send(f(r));
    }));
    let _ = worker().lock().map(|tx| tx.send(job));
    PendingJob { rx: Mutex::new(rx) }
}

/// P3B.9: kernel bodies of placed parts, for work across Part Studios (an assembly's
/// interference and export). The bodies it returns are the caller's to release.
#[cfg(feature = "occt")]
impl Rebuilder {
    /// A copy of the part `part` of the Part Studio `features` (rebuilt, mostly from the cache)
    /// moved by the rotation `rotation` (rows) and `translation` (mm).
    pub fn placed_body(&mut self, features: &[Feature], part: PartId, rotation: [[f64; 3]; 3], translation: [f64; 3]) -> Result<BodyId, String> {
        use cadrs_kernel::Kernel;
        self.rebuild(features);
        let body = self.last.part(part).and_then(|p| p.body).ok_or_else(|| "the part has no solid model".to_string())?;
        let r = nalgebra::Matrix3::from_row_slice(&rotation.concat());
        let q = nalgebra::UnitQuaternion::from_matrix(&r);
        let iso = nalgebra::Isometry3::from_parts(nalgebra::Translation3::new(translation[0], translation[1], translation[2]), q);
        let out = self.kernel.transform(body, &iso).map_err(|e| e.to_string())?;
        out.bodies.first().copied().ok_or_else(|| "moving the part failed".to_string())
    }

    /// The kernel, for queries on bodies from [`Rebuilder::placed_body`] (P3H.5: PCB Studio's
    /// Sync reads the board part's faces and edges).
    pub fn kernel(&self) -> &dyn cadrs_kernel::Kernel {
        &self.kernel
    }

    /// The volume (mm³) two bodies share: their boolean Intersect's (0 when they don't meet).
    pub fn common_volume(&mut self, a: BodyId, b: BodyId) -> Result<f64, String> {
        use cadrs_kernel::Kernel;
        // Bodies that don't meet (or only touch) give no solid: an error, which is no volume.
        let Ok(out) = self.kernel.boolean(cadrs_kernel::BoolOp::Intersect, a, &[b]) else {
            return Ok(0.0);
        };
        let mut v = 0.0;
        for body in &out.bodies {
            v += self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(0.0);
            self.kernel.release(*body);
        }
        Ok(v)
    }

    /// [`Self::common_volume`] and the shared volume's triangles (to draw it).
    pub fn common_volume_mesh(&mut self, a: BodyId, b: BodyId) -> Result<(f64, Vec<Triangle32>), String> {
        use cadrs_kernel::Kernel;
        let Ok(out) = self.kernel.boolean(cadrs_kernel::BoolOp::Intersect, a, &[b]) else {
            return Ok((0.0, Vec::new()));
        };
        let mut v = 0.0;
        let mut tris = Vec::new();
        for body in &out.bodies {
            v += self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(0.0);
            if let Ok(m) = self.kernel.tessellate(*body, cadrs_kernel::Tessellation { deflection: 0.05, angle: std::f64::consts::PI / 36.0 }) {
                let p = |i: u32| {
                    let q = m.positions[i as usize];
                    [q.x as f32, q.y as f32, q.z as f32]
                };
                tris.extend(m.indices.iter().map(|t| [p(t[0]), p(t[1]), p(t[2])]));
            }
            self.kernel.release(*body);
        }
        Ok((v, tris))
    }

    /// Bounding box (min, max) of a body.
    pub fn body_box(&self, body: BodyId) -> Option<([f64; 3], [f64; 3])> {
        use cadrs_kernel::Kernel;
        let b = self.kernel.bounding_box(body).ok()?;
        Some(([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]))
    }

    /// STEP text of `bodies` with their products named `names`.
    pub fn step_of(&mut self, bodies: &[BodyId], names: &[&str]) -> Result<Vec<u8>, String> {
        use cadrs_kernel::Kernel;
        let bytes = self.kernel.export_step(bodies).map_err(|e| e.to_string())?;
        Ok(crate::export::name_products(&String::from_utf8_lossy(&bytes), names).into_bytes())
    }

    /// Releases a body `placed_body` made.
    pub fn release_body(&mut self, body: BodyId) {
        self.release(body);
    }
}

/// A rebuild in progress on the worker thread. Dropping it cancels the rebuild if it hasn't
/// started yet.
pub struct Pending {
    rx: Mutex<mpsc::Receiver<Arc<Build>>>,
    cancelled: Arc<AtomicBool>,
    done: Option<Arc<Build>>,
}

impl Pending {
    /// The result, if the rebuild finished.
    pub fn poll(&mut self) -> Option<Arc<Build>> {
        self.wait(Some(Duration::ZERO))
    }

    /// Waits up to `timeout` (forever with `None`) for the result.
    pub fn wait(&mut self, timeout: Option<Duration>) -> Option<Arc<Build>> {
        if self.done.is_none() {
            let rx = self.rx.lock().ok()?;
            let got = match timeout {
                None => rx.recv().ok(),
                Some(d) if d.is_zero() => rx.try_recv().ok(),
                Some(d) => rx.recv_timeout(d).ok(),
            };
            drop(rx);
            self.done = got;
        }
        self.done.clone()
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Starts rebuilding `features` on the worker thread.
pub fn request(features: Vec<Feature>) -> Pending {
    request_with(features, false)
}

/// Starts the app's rebuild of a Part Studio's `features` on the worker thread: restored from
/// a session snapshot when the session doesn't have their outputs, and snapshotted afterwards
/// ([`session`]).
pub fn request_persisted(features: Vec<Feature>) -> Pending {
    request_with(features, true)
}

fn request_with(features: Vec<Feature>, persist: bool) -> Pending {
    let (reply, rx) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let job = Job::Rebuild {
        features,
        reply,
        cancelled: cancelled.clone(),
        persist,
    };
    let sent = worker().lock().map(|tx| tx.send(job).is_ok()).unwrap_or(false);
    let mut pending = Pending {
        rx: Mutex::new(rx),
        cancelled,
        done: None,
    };
    if !sent {
        pending.done = Some(Arc::new(Build::default()));
    }
    pending
}

/// Rebuilds `features` and waits for the result (fast when the features before the first
/// change are cached).
pub fn build(features: &[Feature]) -> Arc<Build> {
    if !features.iter().any(Feature::is_part_feature) {
        return Arc::new(Build::default());
    }
    request(features.to_vec())
        .wait(None)
        .unwrap_or_else(|| Arc::new(Build::failed(features, "The rebuild thread stopped")))
}

/// A STEP export in progress on the worker thread.
pub struct PendingExport {
    rx: Mutex<mpsc::Receiver<Result<Vec<StepFile>, String>>>,
}

impl PendingExport {
    /// The files (or why the export failed), once it is done.
    pub fn poll(&self) -> Option<Result<Vec<StepFile>, String>> {
        match self.rx.lock().ok()?.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The rebuild thread stopped".into())),
        }
    }

    /// Waits for the files.
    pub fn wait(self) -> Result<Vec<StepFile>, String> {
        let rx = self.rx.into_inner().map_err(|_| "The rebuild thread stopped".to_string())?;
        rx.recv().unwrap_or_else(|_| Err("The rebuild thread stopped".into()))
    }
}

/// Starts exporting parts of `features` to STEP on the worker thread (after the rebuilds
/// requested before it).
pub fn export_step(features: Vec<Feature>, request: StepRequest) -> PendingExport {
    let (reply, rx) = mpsc::channel();
    let job = Job::Export { features, request, reply };
    // If the worker is gone, the reply sender is dropped and `poll` reports it.
    let _ = worker().lock().map(|tx| tx.send(job));
    PendingExport { rx: Mutex::new(rx) }
}

/// Starts projecting a drawing view on the worker thread (see [`crate::views::request`]); it
/// is skipped if `cancelled` is set before it starts.
pub(crate) fn project_view(
    features: Vec<Feature>,
    request: crate::views::ViewRequest,
    cancelled: Arc<AtomicBool>,
) -> mpsc::Receiver<Result<Arc<crate::views::ViewGeometry>, String>> {
    let (reply, rx) = mpsc::channel();
    let job = Job::Project {
        features,
        request: Box::new(request),
        reply,
        cancelled,
    };
    let _ = worker().lock().map(|tx| tx.send(job));
    rx
}

/// Starts projecting an assembly view on the worker thread (P3C.5).
pub(crate) fn project_assembly_view(
    state: crate::drawing_assembly::AssemblyState,
    request: crate::views::ViewRequest,
    cancelled: Arc<AtomicBool>,
) -> mpsc::Receiver<Result<Arc<crate::views::ViewGeometry>, String>> {
    let (reply, rx) = mpsc::channel();
    let job = Job::ProjectAssembly { state: Box::new(state), request: Box::new(request), reply, cancelled };
    let _ = worker().lock().map(|tx| tx.send(job));
    rx
}

/// A triangle's corners (f32, assembly coordinates).
pub type Triangle32 = [[f32; 3]; 3];

/// OCCT's HLR of several separate solids (an assembly view) classifies each piece of an edge
/// as a whole and splits edges only where they cross in 2D, so where parts overlap (the Ex3
/// stand-in after the course's edits: the grown Handle Plate runs into the enclosure) pieces of
/// edges that another part hides stay visible, some in short alternating pieces: stray dashes in
/// a shaded view (P3C wrap-up). Each visible piece of an edge is checked against the parts'
/// triangles (`tris`, a depth buffer): points every half millimetre along it are lifted back
/// onto the 3D edge (the point of its polyline that projects nearest) and a point is hidden when
/// a triangle covers it more than 0.05 mm nearer the eye. Runs of hidden points become hidden
/// pieces; nothing hidden is made visible.
///
/// With `owners` (each triangle's index in `ids`), an edge is checked against the *other* parts'
/// triangles only: HLR already hides a part's edges behind its own faces, and its coarse display
/// mesh lying a hair nearer than the true surface cut visible outlines into stubs (Final
/// regression judge: `course_drw_ex3_update` 29–32, the cut-out ring and the screw outlines).
#[cfg(feature = "occt")]
fn hide_occluded_pieces(
    k: &dyn cadrs_kernel::Kernel,
    ids: &[BodyId],
    frame: &cadrs_kernel::ViewFrame,
    tris: &[crate::views::ShadedTriangle],
    owners: Option<&[Option<usize>]>,
    proj: &mut cadrs_kernel::Projection,
) {
    use cadrs_kernel::{ProjEdge, ProjVisibility};
    use nalgebra::{Point2, Point3};
    if ids.is_empty() || tris.is_empty() {
        return;
    }
    // A grid of the triangles' 2D boxes.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for t in tris {
        for p in &t.points {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
    }
    let cell = 2.0f64;
    let (nx, ny) = ((((hi[0] - lo[0]) / cell).ceil() as usize).clamp(1, 2048), (((hi[1] - lo[1]) / cell).ceil() as usize).clamp(1, 2048));
    let ix = |x: f64| (((x - lo[0]) / cell).floor().max(0.0) as usize).min(nx - 1);
    let iy = |y: f64| (((y - lo[1]) / cell).floor().max(0.0) as usize).min(ny - 1);
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
    for (i, t) in tris.iter().enumerate() {
        let (x0, x1) = (t.points.iter().map(|p| p[0]).fold(f64::MAX, f64::min), t.points.iter().map(|p| p[0]).fold(f64::MIN, f64::max));
        let (y0, y1) = (t.points.iter().map(|p| p[1]).fold(f64::MAX, f64::min), t.points.iter().map(|p| p[1]).fold(f64::MIN, f64::max));
        for gy in iy(y0)..=iy(y1) {
            for gx in ix(x0)..=ix(x1) {
                grid[gy * nx + gx].push(i as u32);
            }
        }
    }
    let covered_nearer = |q: Point2<f64>, depth: f64, own: usize| -> bool {
        if q.x < lo[0] || q.x > hi[0] || q.y < lo[1] || q.y > hi[1] {
            return false;
        }
        grid[iy(q.y) * nx + ix(q.x)].iter().any(|&i| {
            if owners.is_some_and(|o| o.get(i as usize).copied().flatten() == Some(own)) {
                return false;
            }
            let t = &tris[i as usize];
            let [a, b, c] = t.points;
            let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if det.abs() < 1e-14 {
                return false;
            }
            let l1 = ((b[1] - c[1]) * (q.x - c[0]) + (c[0] - b[0]) * (q.y - c[1])) / det;
            let l2 = ((c[1] - a[1]) * (q.x - c[0]) + (a[0] - c[0]) * (q.y - c[1])) / det;
            let l3 = 1.0 - l1 - l2;
            let e = -1e-9;
            l1 >= e && l2 >= e && l3 >= e && l1 * t.depths[0] + l2 * t.depths[1] + l3 * t.depths[2] < depth - 0.05
        })
    };
    let mut polys: HashMap<usize, HashMap<cadrs_kernel::EdgeId, Vec<Point3<f64>>>> = HashMap::new();
    let mut out: Vec<ProjEdge> = Vec::with_capacity(proj.edges.len());
    for e in std::mem::take(&mut proj.edges) {
        let src = e.source.as_ref().and_then(|s| s.edge.map(|id| (s.body, id)));
        let (true, Some((body, eid))) = (e.visibility == ProjVisibility::Visible, src) else {
            out.push(e);
            continue;
        };
        let Some(&bid) = ids.get(body) else {
            out.push(e);
            continue;
        };
        let edges = polys.entry(body).or_insert_with(|| {
            k.tessellate(bid, cadrs_kernel::Tessellation { deflection: 0.02, angle: std::f64::consts::PI / 72.0 })
                .map(|m| m.edges.into_iter().collect())
                .unwrap_or_default()
        });
        let Some(poly) = edges.get(&eid).filter(|p| p.len() >= 2) else {
            out.push(e);
            continue;
        };
        let lift = |q: Point2<f64>| -> Point3<f64> {
            let mut best = (f64::MAX, poly[0]);
            for w in poly.windows(2) {
                let (a, b) = (frame.to_2d(&w[0]), frame.to_2d(&w[1]));
                let d = b - a;
                let l2 = d.norm_squared();
                let t = if l2 > 1e-18 { ((q - a).dot(&d) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let dist = (a + d * t - q).norm();
                if dist < best.0 {
                    best = (dist, w[0] + (w[1] - w[0]) * t);
                }
            }
            best.1
        };
        let len = e.length();
        let n = ((len / 0.5).ceil() as usize).clamp(2, 400);
        // Hidden at each sample (the samples sit mid-way in n equal steps).
        let hidden: Vec<bool> = (0..n)
            .map(|i| {
                let t = (i as f64 + 0.5) / n as f64;
                let q = e.point_at(t);
                covered_nearer(q, frame.depth(&lift(q)), body)
            })
            .collect();
        if !hidden.iter().any(|h| *h) {
            out.push(e);
            continue;
        }
        // Runs of one visibility (a lone sample doesn't make a run).
        let mut runs: Vec<(usize, usize, bool)> = Vec::new();
        for (i, h) in hidden.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.2 == *h => r.1 = i + 1,
                _ => runs.push((i, i + 1, *h)),
            }
        }
        let mut merged: Vec<(usize, usize, bool)> = Vec::new();
        for r in runs {
            match merged.last_mut() {
                Some(m) if r.1 - r.0 < 2 || m.2 == r.2 => m.1 = r.1,
                _ => merged.push(r),
            }
        }
        for (a, b, h) in merged {
            let (t0, t1) = (a as f64 / n as f64, b as f64 / n as f64);
            let mut piece = e.clone();
            piece.points = sub_polyline(&e.points, t0 * len, t1 * len);
            if h {
                piece.visibility = ProjVisibility::Hidden;
            }
            if piece.points.len() >= 2 {
                out.push(piece);
            }
        }
    }
    proj.edges = out;
}

/// **Show part intersections** (X6): the curves where parts run into each other. Each pair of
/// parts whose boxes overlap is intersected (the kernel's Intersect of copies; the parts stay),
/// and the edges of each common volume are projected on their own and kept where no part's
/// triangles hide them: the curves on the parts' surfaces where one part enters another (edges
/// of the common inside a part are hidden by that part's faces). They are drawn as visible
/// edges with no source (nothing attaches to them).
#[cfg(feature = "occt")]
fn part_intersections(
    k: &mut cadrs_kernel::backend::occt::OcctKernel,
    ids: &[BodyId],
    frame: &cadrs_kernel::ViewFrame,
    opts: &cadrs_kernel::ProjectOptions,
    tris: &[crate::views::ShadedTriangle],
) -> Vec<cadrs_kernel::ProjEdge> {
    use cadrs_kernel::{BoolOp, Kernel, ProjVisibility};
    let boxes: Vec<Option<cadrs_kernel::Aabb>> = ids.iter().map(|b| k.bounding_box(*b).ok()).collect();
    let tol = 1e-3;
    let overlap = |a: &cadrs_kernel::Aabb, b: &cadrs_kernel::Aabb| (0..3).all(|i| a.min[i] < b.max[i] - tol && b.min[i] < a.max[i] - tol);
    let mut out = Vec::new();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let (Some(a), Some(b)) = (boxes[i], boxes[j]) else { continue };
            if !overlap(&a, &b) {
                continue;
            }
            let Ok(r) = k.boolean(BoolOp::Intersect, ids[i], &[ids[j]]) else { continue };
            for c in r.bodies {
                if k.solid_count(c).unwrap_or(0) > 0
                    && let Ok(mut p) = k.project(&[c], frame, opts)
                {
                    p.edges.retain(|e| e.visibility == ProjVisibility::Visible && e.source.as_ref().is_some_and(|s| s.edge.is_some()));
                    hide_occluded_pieces(k, &[c], frame, tris, None, &mut p);
                    out.extend(p.edges.into_iter().filter(|e| e.visibility == ProjVisibility::Visible).map(|mut e| {
                        e.source = None;
                        e
                    }));
                }
                k.release(c);
            }
        }
    }
    out
}

/// The part of polyline `pts` from arc length `s0` to `s1`.
#[cfg(feature = "occt")]
fn sub_polyline(pts: &[nalgebra::Point2<f64>], s0: f64, s1: f64) -> Vec<nalgebra::Point2<f64>> {
    let mut out = Vec::new();
    let mut s = 0.0;
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).norm();
        let (a, b) = (s, s + l);
        if b >= s0 && a <= s1 && l > 0.0 {
            let p = |x: f64| w[0] + (w[1] - w[0]) * ((x - a) / l).clamp(0.0, 1.0);
            if out.is_empty() {
                out.push(p(s0.max(a)));
            }
            out.push(p(s1.min(b)));
        }
        s = b;
    }
    out
}

/// A view's projection and its hatch loops (P3C.8).
type ProjectedParts = (cadrs_kernel::Projection, Vec<Vec<[f64; 2]>>);
