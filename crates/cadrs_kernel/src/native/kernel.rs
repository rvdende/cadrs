//! `NativeKernel`: the pure-Rust backend of [`crate::Kernel`].

use rustc_hash::FxHashMap as HashMap;
use std::sync::{Arc, OnceLock};

use nalgebra::Point3;

use super::build::Built;
use super::classify;
use super::curve::Curve;
use super::math::*;
use super::profile;
use super::surface::Surface;
use super::tess;
use super::tol;
use super::topo::Body;
use crate::*;

/// A body in the session, with lazily derived data.
pub struct Entry {
    pub body: Arc<Body>,
    polys: OnceLock<Vec<Vec<Vec<P2>>>>,
    /// What [`Kernel::faces`] and [`Kernel::edges`] report (face areas and edge lengths are
    /// integrals: a rebuild asks for them over the same bodies many times).
    faces: OnceLock<Vec<FaceInfo>>,
    edges: OnceLock<Vec<EdgeInfo>>,
    /// [`Kernel::edges_light`]'s (without lengths and smoothness), while `edges` isn't made.
    edges_light: OnceLock<Vec<EdgeInfo>>,
    /// [`Kernel::edges_shown`]'s, with the length below which they are exact.
    edges_shown: OnceLock<(f64, Vec<EdgeInfo>)>,
    /// [`Kernel::bounding_box`]: extremes over its faces (a rebuild asks for every part's box
    /// near each new body).
    aabb: OnceLock<Aabb>,
    /// Each face's box, grown by the body's largest tolerance ([`NativeKernel::surely_apart`]).
    face_boxes: OnceLock<Vec<Bbox>>,
    /// Whether [`Body::check`] finds nothing ([`NativeKernel::sound`]).
    sound: OnceLock<bool>,
    /// [`Body::boxes`]: a boolean asks for its inputs' boxes many times over, and the body a
    /// pass cut is the next pass's target.
    boxes: OnceLock<super::topo::Boxes>,
    /// The body's [`Body::oriented_face_keys`], for the bodies booleans make of it.
    oriented: OnceLock<Arc<rustc_hash::FxHashSet<u128>>>,
    /// Those of the sound body a boolean made this one of ([`NativeKernel::made_of`]).
    made_of: OnceLock<Arc<rustc_hash::FxHashSet<u128>>>,
    /// The faces that are copies of others, as the operation that made the body knows them
    /// (a pattern's: [`super::pattern`]): their mass properties and meshes are the others' moved.
    pub(crate) copies: OnceLock<Arc<super::copies::FaceCopies>>,
    /// [`crate::Kernel::mass_properties`], worked out once (asked for its volume, area,
    /// centroid and inertia one at a time).
    mass: OnceLock<MassProperties>,
    /// Each parametric face's grid for rays ([`classify::ray_hits_kept`]), made at the first
    /// ray that reaches the face.
    ray_grids: OnceLock<Vec<OnceLock<classify::RayGrid>>>,
    /// The body this one is an affine map of ([`crate::Kernel::transform_affine`]), with the
    /// map back and how far it stretches at most: a point is inside this one where its image
    /// back is inside that one, whose surfaces are still analytic (this one's are NURBS).
    affine_of: OnceLock<(Arc<Entry>, Affine, f64)>,
}

impl Entry {
    pub fn new(body: Body) -> Self {
        Self {
            body: Arc::new(body),
            polys: OnceLock::new(),
            faces: OnceLock::new(),
            edges: OnceLock::new(),
            edges_light: OnceLock::new(),
            edges_shown: OnceLock::new(),
            aabb: OnceLock::new(),
            face_boxes: OnceLock::new(),
            sound: OnceLock::new(),
            boxes: OnceLock::new(),
            oriented: OnceLock::new(),
            made_of: OnceLock::new(),
            copies: OnceLock::new(),
            mass: OnceLock::new(),
            ray_grids: OnceLock::new(),
            affine_of: OnceLock::new(),
        }
    }
    /// [`Body::boxes`], made once.
    pub(crate) fn boxes(&self) -> &super::topo::Boxes {
        self.boxes.get_or_init(|| self.body.boxes())
    }

    /// Each face's box, grown by the largest of the body's edge and vertex tolerances and
    /// [`tol::point()`].
    pub fn face_boxes(&self) -> &Vec<Bbox> {
        self.face_boxes.get_or_init(|| {
            let b = &self.body;
            let reach = b
                .edges
                .iter()
                .map(|e| e.tol)
                .chain(b.verts.iter().map(|v| v.tol))
                .fold(tol::point(), f64::max);
            (0..b.faces.len())
                .map(|f| b.face_bbox(f).grow(2.0 * reach))
                .collect()
        })
    }

    pub fn polys(&self) -> &Vec<Vec<Vec<P2>>> {
        self.polys.get_or_init(|| {
            self.body
                .faces
                .iter()
                .map(|f| classify::face_polygons(&self.body, f))
                .collect()
        })
    }
}

/// A kernel session: its bodies and the tolerances its operations run with.
#[derive(Default)]
pub struct NativeKernel {
    /// (Shared with [`Self::fork`]s: a body never changes.)
    bodies: HashMap<u64, Arc<Entry>>,
    next: u64,
    /// The first id this session numbered a body with: a fork's range ([`Self::fork`]); 0 for
    /// a session of its own.
    first: u64,
    tol: super::tol::Tolerances,
    /// Face colours (sRGB and alpha) by body, in face order: an imported file's, or what the
    /// application set. Operations pass them on through their history ([`Self::carry_colors`]).
    colors: HashMap<u64, Vec<Option<[u8; 4]>>>,
    /// The DRAW layer's last offset: its input body, result and history (`savehistory`).
    pub(crate) draw_history: Option<(BodyId, BodyId, crate::History)>,
    /// The last unions [`Kernel::joins`] found one solid, by their inputs (a body's id is never
    /// used again, and a body never changes): a rebuild asks whether a new body joins a part,
    /// then unites them, and the second union of two bodies of many B-spline faces cost as
    /// much as the first (6 s each by pi7zoll-gehäuse's foot). The oldest past
    /// [`JOINED_KEPT`] is released.
    joined: Vec<(BodyId, BodyId, OpResult)>,
    /// The swept tori this session built exactly, by their bodies: the torus and its motion,
    /// which a boolean of two of them is made from again ([`super::sweep_tube::boolean`]: by
    /// what each motion holds, where their surfaces' crossing in space wanders).
    pub(crate) swept_tori: HashMap<u64, Arc<super::sweep_tube::SweptTorus>>,
    /// The pipes this session made (a round section swept along a path from its centre, and
    /// what it comes to with a ball of its radius united at an end), by their bodies: slid, a
    /// pipe is swept as one ([`super::sweep_pipe::slide`]).
    pub(crate) pipes: HashMap<u64, Arc<super::sweep_pipe::Pipe>>,
    /// The bodies this session made by moving another rigidly ([`Kernel::transform`]): the
    /// body first moved, and the motion that takes it to this one ([`super::sweep_cache`]).
    pub(crate) placed: HashMap<u64, (u64, Affine)>,
    /// The sweeps of solids kept, by the body first moved ([`super::sweep_cache`]).
    pub(crate) swept: HashMap<u64, Vec<Arc<super::sweep_cache::Swept>>>,
    /// The unions of several tools kept ([`super::sweep_cache::united`]).
    pub(crate) united: Vec<Arc<super::sweep_cache::United>>,
}

/// How many of [`Kernel::joins`]'s unions a session keeps for the union that may follow.
const JOINED_KEPT: usize = 4;

/// How many body ids each [`NativeKernel::fork`] of a session has to itself.
const FORK_IDS: u64 = 1 << 32;

/// The next range of body ids a fork takes: every fork its own, a fork of a fork too (a fork's
/// range past its parent's lane would be another fork's), and past every id handed out before.
static FORK_RANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl NativeKernel {
    /// An empty session at the default tolerances.
    pub fn new() -> Self {
        Self::default()
    }

    /// The tolerances the session's operations run with ([`super::tol`]).
    pub fn tolerances(&self) -> super::tol::Tolerances {
        self.tol
    }

    /// Sets the tolerances the session's operations run with from now on.
    /// The merge distance ([`super::tol::point`]) is one for the process: setting it here sets
    /// it for every session.
    pub fn set_tolerances(&mut self, tol: super::tol::Tolerances) -> Result<()> {
        tol.validate()?;
        self.tol = tol;
        super::tol::set_point(tol.point);
        Ok(())
    }

    /// The session's record of a body.
    /// The volume of `id`'s solids where its mass properties are worked out already (a caller
    /// measuring a body before an operation on it), else None: what an operation that checks
    /// its result against its input's volume needn't integrate again.
    pub(crate) fn known_volume(&self, id: BodyId) -> Option<f64> {
        self.bodies.get(&id.0)?.mass.get().map(|m| m.volume)
    }

    /// [`Self::entry`] shared (kept by another body that refers to it).
    pub(crate) fn entry_arc(&self, id: BodyId) -> Result<Arc<Entry>> {
        self.bodies
            .get(&id.0)
            .cloned()
            .ok_or(KernelError::UnknownBody(id))
    }

    pub(crate) fn entry(&self, id: BodyId) -> Result<&Entry> {
        self.bodies
            .get(&id.0)
            .map(Arc::as_ref)
            .ok_or(KernelError::UnknownBody(id))
    }

    /// The B-rep of a body of the session (`UnknownBody` if it has none of that id).
    pub fn get(&self, id: BodyId) -> Result<&Arc<Body>> {
        Ok(&self.entry(id)?.body)
    }

    /// Adds a body to the session as it is (run [`Body::check`] first on one built by hand).
    pub fn insert(&mut self, mut body: Body) -> BodyId {
        // (Every body's loops on periodic surfaces run on in (u, v): see the function.)
        body.chain_periodic_pcurves();
        let id = self.next;
        self.next += 1;
        self.bodies.insert(id, Arc::new(Entry::new(body)));
        BodyId(id)
    }

    /// [`Self::insert`] for a body its maker has just checked ([`Body::check`] found nothing,
    /// or it was cut from one such in a way that leaves it so): [`Self::sound`] needn't again.
    pub(crate) fn insert_sound(&mut self, body: Body) -> BodyId {
        let id = self.insert(body);
        self.mark_sound(id);
        id
    }

    /// Notes that a body of the session is sound ([`Self::insert_sound`], of one in it already).
    pub(crate) fn mark_sound(&self, id: BodyId) {
        if let Ok(e) = self.entry(id) {
            let _ = e.sound.set(true);
        }
    }

    /// Whether [`Body::check`] finds nothing wrong with a body of the session (false for no
    /// such body). Checked once: a body never changes, and a sweep's steps each ask of the
    /// body the step before handed on.
    pub(crate) fn sound(&self, id: BodyId) -> bool {
        self.entry(id).is_ok_and(|e| {
            *e.sound.get_or_init(|| match e.made_of.get() {
                Some(from) => e.body.check_cut_from(from).is_empty(),
                None => e.body.check().is_empty(),
            })
        })
    }

    /// [`seam_through_a_vertex`] of a body of the session, its boxes kept with it.
    fn seam_through_a_vertex(&self, id: BodyId) -> bool {
        self.entry(id).is_ok_and(|e| {
            let _kept = super::topo::keep_boxes_made(&e.body, e.boxes());
            seam_through_a_vertex(&e.body)
        })
    }

    /// Notes that a boolean made body `id` of `target` (cut from it, added to it, or what it
    /// shares with another): where `target` is known sound, `id`'s check takes the faces it
    /// kept of it as pointing the right way ([`Body::check_cut_from`]).
    pub(crate) fn made_of(&self, id: BodyId, target: BodyId) {
        let (Ok(e), Ok(t)) = (self.entry(id), self.entry(target)) else {
            return;
        };
        if t.sound.get() == Some(&true) {
            let keys = t
                .oriented
                .get_or_init(|| Arc::new(t.body.oriented_face_keys().into_iter().collect()));
            let _ = e.made_of.set(Arc::clone(keys));
        }
    }

    /// Whether two solid bodies are sure to be apart, no boolean needed: no face of one comes
    /// near a face of the other (their [`Entry::face_boxes`]), and no solid of either holds a
    /// point of the other (as one inside the other would, no faces near). A rebuild asks
    /// whether a new body touches every part whose box meets its own, and in a house a wall's box
    /// meets most of its windows'.
    pub fn surely_apart(&self, a: BodyId, b: BodyId) -> Result<bool> {
        let (ea, eb) = (self.entry(a)?, self.entry(b)?);
        // (Solids alone: a sheet has no inside.)
        let solid = |x: &Body| {
            !x.solids.is_empty()
                && (0..x.solids.len())
                    .map(|s| x.solid_faces(s).len())
                    .sum::<usize>()
                    == x.faces.len()
        };
        if !solid(&ea.body) || !solid(&eb.body) {
            return Ok(false);
        }
        let (fa, fb) = (ea.face_boxes(), eb.face_boxes());
        if fa.iter().any(|x| fb.iter().any(|y| x.overlaps(y))) {
            return Ok(false);
        }
        // A point of each solid of one, outside the other.
        let outside = |x: &Entry, y: &Entry| {
            (0..x.body.solids.len()).all(|s| {
                let point = x.body.solid_faces(s).iter().find_map(|&f| {
                    let c = x.body.faces[f].loops.first()?.first()?;
                    let e = &x.body.edges[c.edge];
                    Some(x.body.verts[e.v[0]].p)
                });
                point.is_some_and(|p| {
                    classify::classify_point(&y.body, y.polys(), &p, tol::point())
                        == PointClass::Outside
                })
            })
        };
        Ok(outside(ea, eb) && outside(eb, ea))
    }

    /// [`Kernel::joins`] from one boolean run (no retries when it leaves the union broken, and
    /// the union not kept for an Add): a question only, as a New's dialog asks it.
    pub fn joins_once(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        if self.surely_apart(a, b)? || super::boolean::union_falls_apart(self.get(a)?, self.get(b)?)
        {
            return Ok(false);
        }
        let r = super::boolean::boolean_once(self, BoolOp::Union, a, &[b])?;
        let joined = self.solid_count(r.bodies[0]).unwrap_or(2) == 1;
        for x in r.bodies {
            self.release(x);
        }
        Ok(joined)
    }

    /// [`Kernel::overlaps`] from one boolean run, like [`Self::joins_once`].
    pub fn overlaps_once(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        if self.surely_apart(a, b)? {
            return Ok(false);
        }
        let r = super::boolean::boolean_once(self, BoolOp::Intersect, a, &[b])?;
        let v = self
            .get(r.bodies[0])
            .map(|x| super::mass::body_volume(x))
            .unwrap_or(0.0);
        for x in r.bodies {
            self.release(x);
        }
        Ok(v > tol::ROUNDOFF)
    }

    /// A session to run an operation in on another thread, its bodies, colours and
    /// tolerances this one's; [`Self::absorb`] takes back what it made. Its new bodies are
    /// numbered from a range of ids of its own ([`FORK_RANGES`]).
    pub fn fork(&self) -> NativeKernel {
        let first = FORK_RANGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) * FORK_IDS;
        NativeKernel {
            bodies: self.bodies.clone(),
            next: first,
            first,
            tol: self.tol,
            colors: self.colors.clone(),
            draw_history: None,
            // (Not the unions kept for a union to come: a boolean takes its result out of them,
            // and two sessions mustn't hand out the same body.)
            joined: Vec::new(),
            swept_tori: self.swept_tori.clone(),
            pipes: self.pipes.clone(),
            placed: self.placed.clone(),
            swept: self.swept.clone(),
            united: self.united.clone(),
        }
    }

    /// Takes the bodies (and their colours) a [`Self::fork`] of this session made, and the
    /// unions it kept for [`Kernel::joins`] (after this session's, as though made here in turn:
    /// the oldest past [`JOINED_KEPT`] released).
    pub fn absorb(&mut self, fork: NativeKernel) {
        for (id, e) in fork.bodies {
            // (Not those it had from here: one released here since stays released.)
            if id < fork.first || self.bodies.contains_key(&id) {
                continue;
            }
            if let Some(c) = fork.colors.get(&id) {
                self.colors.insert(id, c.clone());
            }
            if let Some(t) = fork.swept_tori.get(&id) {
                self.swept_tori.insert(id, t.clone());
            }
            if let Some(t) = fork.pipes.get(&id) {
                self.pipes.insert(id, t.clone());
            }
            self.bodies.insert(id, e);
            self.next = self.next.max(id + 1);
        }
        for j in fork.joined {
            self.joined.push(j);
            if self.joined.len() > JOINED_KEPT {
                let (_, _, old) = self.joined.remove(0);
                for x in old.bodies {
                    self.release(x);
                }
            }
        }
    }

    /// Adds a built body with its faces' origins as the history of a new result.
    pub(crate) fn insert_result(&mut self, b: Built) -> OpResult {
        let mut history = History::default();
        for (f, o) in &b.origins {
            history.generated.push((FaceId(*f as u64), *o));
        }
        let id = self.insert(b.body);
        OpResult {
            bodies: vec![id],
            history,
        }
    }

    /// A body's face colours, in face order (empty when it has none).
    pub fn colors(&self, id: BodyId) -> &[Option<[u8; 4]>] {
        self.colors.get(&id.0).map_or(&[], Vec::as_slice)
    }

    /// Sets a body's face colours, in face order (cut or padded to its faces; none at all
    /// clears them).
    pub fn set_colors(&mut self, id: BodyId, mut colors: Vec<Option<[u8; 4]>>) {
        let Ok(n) = self.get(id).map(|b| b.faces.len()) else {
            return;
        };
        colors.resize(n, None);
        if colors.iter().all(Option::is_none) {
            self.colors.remove(&id.0);
        } else {
            self.colors.insert(id.0, colors);
        }
    }

    /// An operation on one body, run at a working size where the body is small: a body no
    /// larger than a boolean's small model ([`super::boolean::SMALL_MODEL`]) is scaled up by
    /// a power of two (which changes no mantissa), `op` runs on that body with its lengths
    /// scaled as much (`op`'s last argument), and the result is scaled back, its history
    /// naming the body given. The kernel's merge distances and many of its guards are lengths
    /// in mm: on a part a few microns across they are the part's own size, as in a boolean.
    /// `op` on the body turned into the frame of its first analytic surface (where a model
    /// moved by a rigid motion was likely drawn, its faces along the axes), the results turned
    /// back: an operation that commutes with rigid motions, whose searches bound geometry by
    /// axis-aligned boxes (loose round a tilted face). As given where that frame is already
    /// along the axes.
    fn in_own_frame(
        &mut self,
        body: BodyId,
        op: impl FnOnce(&mut Self, BodyId) -> Result<OpResult>,
    ) -> Result<OpResult> {
        let b = Arc::clone(self.get(body)?);
        let Some(f) = b.faces.iter().find_map(|f| f.surf.frame().copied()) else {
            return op(self, body);
        };
        let aligned = |v: &V3| v.amax() > 1.0 - 1e-12;
        if aligned(&f.x) && aligned(&f.y) && aligned(&f.z) {
            return op(self, body);
        }
        let turn = Affine {
            m: nalgebra::Matrix3::from_rows(&[f.x.transpose(), f.y.transpose(), f.z.transpose()]),
            t: V3::zeros(),
        };
        let back = Affine {
            m: turn.m.transpose(),
            t: V3::zeros(),
        };
        let turned = self.insert(b.transformed(&turn));
        if let Some(c) = self.colors.get(&body.0).cloned() {
            self.colors.insert(turned.0, c);
        }
        let r = op(self, turned);
        self.bodies.remove(&turned.0);
        self.colors.remove(&turned.0);
        let mut r = r?;
        for id in &mut r.bodies {
            let moved = self.get(*id)?.transformed(&back);
            let colors = self.colors.remove(&id.0);
            self.bodies.remove(&id.0);
            *id = self.insert(moved);
            if let Some(c) = colors {
                self.colors.insert(id.0, c);
            }
        }
        let given = |x: &mut BodyId| {
            if *x == turned {
                *x = body;
            }
        };
        for (_, o) in &mut r.history.generated {
            match o {
                Origin::FromEdge { body, .. } | Origin::FromVertex { body, .. } => given(body),
                _ => {}
            }
        }
        for (_, from) in &mut r.history.modified {
            given(&mut from.body);
        }
        for from in &mut r.history.deleted {
            given(&mut from.body);
        }
        Ok(r)
    }

    /// `op` on copies of `bodies` mapped by `m` (a rotation scaled by a power of two, and a
    /// shift), its results mapped back, its history naming the bodies given.
    fn in_frame(
        &mut self,
        bodies: &[BodyId],
        m: Affine,
        op: impl FnOnce(&mut Self, &[BodyId]) -> Result<OpResult>,
    ) -> Result<OpResult> {
        let back = {
            let inv =
                m.m.try_inverse()
                    .unwrap_or_else(nalgebra::Matrix3::identity);
            Affine {
                m: inv,
                t: -(inv * m.t),
            }
        };
        let mut copies = Vec::with_capacity(bodies.len());
        for &b in bodies {
            let moved = self.get(b)?.transformed(&m);
            let id = self.insert(moved);
            if let Some(c) = self.colors.get(&b.0).cloned() {
                self.colors.insert(id.0, c);
            }
            copies.push(id);
        }
        let r = op(self, &copies);
        for id in &copies {
            self.bodies.remove(&id.0);
            self.colors.remove(&id.0);
        }
        let mut r = r?;
        for id in &mut r.bodies {
            let mapped = self.get(*id)?.transformed(&back);
            let colors = self.colors.remove(&id.0);
            self.bodies.remove(&id.0);
            *id = self.insert(mapped);
            if let Some(c) = colors {
                self.colors.insert(id.0, c);
            }
        }
        let given = |x: &mut BodyId| {
            if let Some(i) = copies.iter().position(|c| c == x) {
                *x = bodies[i];
            }
        };
        for (_, o) in &mut r.history.generated {
            match o {
                Origin::FromEdge { body, .. } | Origin::FromVertex { body, .. } => given(body),
                _ => {}
            }
        }
        for (_, from) in &mut r.history.modified {
            given(&mut from.body);
        }
        for from in &mut r.history.deleted {
            given(&mut from.body);
        }
        Ok(r)
    }

    fn at_working_size(
        &mut self,
        body: BodyId,
        op: impl FnOnce(&mut Self, BodyId, f64) -> Result<OpResult>,
    ) -> Result<OpResult> {
        self.at_size(body, false, op)
    }

    /// [`Self::at_working_size`], and with `large` a body larger than [`WORKED_LARGE`] scaled
    /// down too (an offset's: a fillet's tuned on parts of a hundred millimetres and more are
    /// left as they are).
    fn at_size(
        &mut self,
        body: BodyId,
        large: bool,
        op: impl FnOnce(&mut Self, BodyId, f64) -> Result<OpResult>,
    ) -> Result<OpResult> {
        let b = Arc::clone(self.get(body)?);
        let size = b.bbox().diagonal();
        if !(size > 0.0
            && size.is_finite()
            && (size < super::boolean::SMALL_MODEL || (large && size > WORKED_LARGE)))
        {
            return op(self, body, 1.0);
        }
        let k = (super::boolean::WORKING_SIZE / size).log2().floor().exp2();
        let scaled = |by: f64| Affine {
            m: nalgebra::Matrix3::identity() * by,
            t: V3::zeros(),
        };
        let big = self.insert(b.transformed(&scaled(k)));
        if let Some(c) = self.colors.get(&body.0).cloned() {
            self.colors.insert(big.0, c);
        }
        let r = op(self, big, k);
        self.bodies.remove(&big.0);
        self.colors.remove(&big.0);
        let mut r = r?;
        for id in &mut r.bodies {
            let small = self.get(*id)?.transformed(&scaled(1.0 / k));
            let colors = self.colors.remove(&id.0);
            self.bodies.remove(&id.0);
            *id = self.insert(small);
            if let Some(c) = colors {
                self.colors.insert(id.0, c);
            }
        }
        // (The history's input was the scaled body: the one given.)
        let given = |x: &mut BodyId| {
            if *x == big {
                *x = body;
            }
        };
        for (_, o) in &mut r.history.generated {
            match o {
                Origin::FromEdge { body, .. } | Origin::FromVertex { body, .. } => given(body),
                _ => {}
            }
        }
        for (_, from) in &mut r.history.modified {
            given(&mut from.body);
        }
        for from in &mut r.history.deleted {
            given(&mut from.body);
        }
        Ok(r)
    }

    /// Gives an operation's result the colours of the input faces its faces continue (the
    /// history's `modified` faces); new faces have none. Only for a result of one body (the
    /// history's face ids are that body's).
    pub(crate) fn carry_colors(&mut self, r: &OpResult) {
        if self.colors.is_empty() || r.bodies.len() != 1 {
            return;
        }
        let colors: Vec<Option<[u8; 4]>> = {
            let Ok(n) = self.get(r.bodies[0]).map(|b| b.faces.len()) else {
                return;
            };
            let mut out = vec![None; n];
            for (f, from) in &r.history.modified {
                let c = self
                    .colors
                    .get(&from.body.0)
                    .and_then(|v| v.get(from.face.0 as usize))
                    .copied()
                    .flatten();
                if let (Some(c), Some(slot)) = (c, out.get_mut(f.0 as usize)) {
                    *slot = Some(c);
                }
            }
            out
        };
        self.set_colors(r.bodies[0], colors);
    }

    /// [`Self::carry_colors`] for an operation's outcome.
    fn colored(&mut self, r: Result<OpResult>) -> Result<OpResult> {
        if let Ok(o) = &r {
            self.carry_colors(o);
        }
        r
    }

    /// Extent of a body along a unit direction from a point: (min, max) of (p − o)·d.
    pub fn extent_along(&self, body: BodyId, o: &P3, d: &V3) -> Result<(f64, f64)> {
        let b = self.get(body)?;
        Ok(extent_along(b, o, d))
    }
}

/// Exact extent of the body's edges along `d` (and of its curved faces' interior extremes).
pub fn extent_along(b: &Body, o: &P3, d: &V3) -> (f64, f64) {
    let lo = std::cell::Cell::new(f64::INFINITY);
    let hi = std::cell::Cell::new(f64::NEG_INFINITY);
    let add = |x: f64| {
        lo.set(lo.get().min(x));
        hi.set(hi.get().max(x));
    };
    // (Only what the faces use: an operation can leave an edge it replaced in the body's
    // arrays, and a chamfered rim's old circle widened the box to it.)
    let mut used = vec![false; b.edges.len()];
    for f in &b.faces {
        for c in f.loops.iter().flatten() {
            if let Some(u) = used.get_mut(c.edge) {
                *u = true;
            }
        }
    }
    let faced = !b.faces.is_empty();
    for (ei, e) in b.edges.iter().enumerate() {
        if faced && !used[ei] {
            continue;
        }
        for &v in &e.v {
            if let Some(v) = b.verts.get(v) {
                add((v.p - o).dot(d));
            }
        }
    }
    if !faced || b.edges.is_empty() {
        for v in &b.verts {
            add((v.p - o).dot(d));
        }
    }
    for (ei, e) in b.edges.iter().enumerate() {
        if e.degenerate || (faced && !used[ei]) {
            continue;
        }
        match &e.curve {
            Curve::Line { .. } => {}
            Curve::Circle { c, x, y, r } => {
                // Stationary points of r(cos t x·d + sin t y·d).
                let (a, bb) = (x.dot(d) * r, y.dot(d) * r);
                let t = bb.atan2(a);
                for tt in [t, t + PI] {
                    let tt = super::curve::clamp_periodic(tt, e.t[0], e.t[1]);
                    add((e.point(tt) - o).dot(d));
                }
                let _ = c;
            }
            Curve::Ellipse { x, y, a, b: bb, .. } => {
                let (p, q) = (x.dot(d) * a, y.dot(d) * bb);
                let t = q.atan2(p);
                for tt in [t, t + PI] {
                    let tt = super::curve::clamp_periodic(tt, e.t[0], e.t[1]);
                    add((e.point(tt) - o).dot(d));
                }
            }
            Curve::Nurbs(_) => {
                let n = 64;
                let mut best = (f64::INFINITY, f64::NEG_INFINITY, e.t[0], e.t[0]);
                for i in 0..=n {
                    let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / n as f64;
                    let x = (e.point(t) - o).dot(d);
                    if x < best.0 {
                        best.0 = x;
                        best.2 = t;
                    }
                    if x > best.1 {
                        best.1 = x;
                        best.3 = t;
                    }
                }
                for t0 in [best.2, best.3] {
                    // Newton on C'(t)·d = 0.
                    let mut t = t0;
                    for _ in 0..20 {
                        let (_, d1, d2) = e.curve.d2(t);
                        let f = d1.dot(d);
                        let df = d2.dot(d);
                        if df.abs() < tol::TINY {
                            break;
                        }
                        t = (t - f / df).clamp(e.t[0], e.t[1]);
                    }
                    add((e.point(t) - o).dot(d));
                }
            }
        }
    }
    // Interior extremes of curved faces: where the normal is ±d.
    for (fi, f) in b.faces.iter().enumerate() {
        // (An extrusion's height along d is linear along its rulings: its extremes lie on its
        // edges, and the search finds none.)
        if matches!(f.surf, Surface::Plane { .. } | Surface::Extrusion { .. }) {
            continue;
        }
        // A B-spline whose control points keep within the extent so far adds nothing (it lies
        // in their hull): no search over it (seconds in all over a part's boxes).
        if let Surface::Nurbs(n) = &f.surf
            && n.cps.iter().flatten().all(|h| {
                let x = (super::nurbs::dehom(h) - o).dot(d);
                h.w > 0.0 && x >= lo.get() && x <= hi.get()
            })
        {
            continue;
        }
        for cand in face_extremes(b, fi, d) {
            add((cand - o).dot(d));
        }
    }
    (lo.get(), hi.get())
}

/// Points of a face's interior where its normal is parallel to `d` (candidates for extremes).
fn face_extremes(b: &Body, fi: usize, d: &V3) -> Vec<P3> {
    let f = &b.faces[fi];
    let polys = classify::face_polygons(b, f);
    let mut out = Vec::new();
    let mut ub = (f64::INFINITY, f64::NEG_INFINITY);
    let mut vb = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &polys {
        for q in p {
            ub = (ub.0.min(q.x), ub.1.max(q.x));
            vb = (vb.0.min(q.y), vb.1.max(q.y));
        }
    }
    if !ub.0.is_finite() {
        return out;
    }
    let n = 8;
    for i in 0..=n {
        for j in 0..=n {
            let (mut u, mut v) = (
                ub.0 + (ub.1 - ub.0) * i as f64 / n as f64,
                vb.0 + (vb.1 - vb.0) * j as f64 / n as f64,
            );
            // Newton on (S_u·d, S_v·d) = 0.
            let mut ok = false;
            for _ in 0..30 {
                let s = f.surf.d2(u, v);
                let (gu, gv) = (s[1].dot(d), s[2].dot(d));
                let (a, bb, c) = (s[3].dot(d), s[4].dot(d), s[5].dot(d));
                let Some((du, dv)) = solve2(a, bb, bb, c, -gu, -gv) else {
                    break;
                };
                u += du;
                v += dv;
                if du.abs() + dv.abs() < 1e-13 {
                    ok = true;
                    break;
                }
            }
            if ok
                && u >= ub.0 - tol::ROUNDOFF
                && u <= ub.1 + tol::ROUNDOFF
                && v >= vb.0 - tol::ROUNDOFF
                && v <= vb.1 + tol::ROUNDOFF
                && classify::in_face(&polys, &f.surf, P2::new(u, v), tol::ROUNDOFF)
                    != classify::InFace::Outside
            {
                out.push(f.surf.eval(u, v));
            }
        }
    }
    out
}

fn curve_kind(e: &super::topo::Edge) -> CurveKind {
    if e.degenerate {
        return CurveKind::Degenerate;
    }
    match e.curve {
        Curve::Line { .. } => CurveKind::Line,
        Curve::Circle { .. } => CurveKind::Circle,
        Curve::Ellipse { .. } => CurveKind::Ellipse,
        Curve::Nurbs(_) => CurveKind::Other,
    }
}

pub fn face_info(b: &Body, fi: usize) -> FaceInfo {
    let f = &b.faces[fi];
    let (area, center) = super::mass::face_area(b, fi);
    let plane = match &f.surf {
        Surface::Plane { f: fr } => {
            let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
            Some(Plane {
                origin: fr.o,
                x_dir: nalgebra::Unit::new_normalize(fr.x),
                normal: nalgebra::Unit::new_normalize(n),
            })
        }
        _ => None,
    };
    let axis = f.surf.axis().map(|(o, d)| Axis {
        origin: o,
        dir: nalgebra::Unit::new_normalize(d),
    });
    let radius = match &f.surf {
        Surface::Cylinder { r, .. } | Surface::Sphere { r, .. } | Surface::Cone { r, .. } => {
            Some(*r)
        }
        Surface::Torus { rmaj, .. } => Some(*rmaj),
        _ => None,
    };
    FaceInfo {
        id: FaceId(fi as u64),
        kind: f.surf.kind(),
        plane,
        area,
        center,
        axis,
        radius,
        fillet_radius: fillet_radius(b, fi),
    }
}

/// The radius of a face rounded like a constant-radius fillet ([`FaceInfo::fillet_radius`]): a
/// torus's tube, or a face whose sections one way across its extent in (u, v) are all arcs (each
/// in a plane, turning at least 2°) of one radius, to a hundred-thousandth of it.
pub fn fillet_radius(b: &Body, fi: usize) -> Option<f64> {
    let f = &b.faces[fi];
    match &f.surf {
        Surface::Torus { rmin, .. } => return Some(*rmin),
        Surface::Plane { .. }
        | Surface::Cylinder { .. }
        | Surface::Sphere { .. }
        | Surface::Cone { .. } => {
            return None;
        }
        _ => {}
    }
    let ((u0, u1), (v0, v1)) = super::heal_split::face_uv_box(b, fi)?;
    if !(u1 > u0 && v1 > v0) {
        return None;
    }
    // The circle through each section's first, middle and last points, and how far its other
    // points lie off it.
    let section = |across: bool, s: f64| -> Option<f64> {
        let at = |t: f64| {
            if across {
                f.surf.eval(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t)
            } else {
                f.surf.eval(u0 + (u1 - u0) * t, v0 + (v1 - v0) * s)
            }
        };
        let pts: Vec<P3> = (0..=8).map(|i| at(i as f64 / 8.0)).collect();
        let (a, m, c) = (pts[0], pts[4], pts[8]);
        let (ab, ac) = (m - a, c - a);
        let n = ab.cross(&ac);
        if n.norm() <= 1e-12 * ab.norm() * ac.norm() {
            return None;
        }
        let centre = a
            + (ac.cross(&n) * ab.norm_squared() + n.cross(&ab) * ac.norm_squared())
                / (2.0 * n.norm_squared());
        let r = (a - centre).norm();
        let normal = n.normalize();
        let off = pts.iter().map(|p| {
            ((p - centre).norm() - r)
                .abs()
                .max((p - a).dot(&normal).abs())
        });
        let turn = ((a - centre).normalize().dot(&(c - centre).normalize()))
            .clamp(-1.0, 1.0)
            .acos();
        (off.fold(0.0, f64::max) <= 1e-5 * r && turn >= 2f64.to_radians()).then_some(r)
    };
    for across in [true, false] {
        let rs: Option<Vec<f64>> = (0..=6).map(|i| section(across, i as f64 / 6.0)).collect();
        if let Some(rs) = rs {
            let r = rs.iter().sum::<f64>() / rs.len() as f64;
            if rs.iter().all(|x| (x - r).abs() <= 1e-5 * r) {
                return Some(r);
            }
        }
    }
    None
}

pub fn edge_infos(b: &Body) -> Vec<EdgeInfo> {
    edge_infos_with(b, Detail::Full)
}

/// What [`edge_infos_with`] works out besides where each edge lies and between which faces:
/// each edge's length (an adaptive integral along a B-spline: most of the work) and whether it
/// is smooth (projections along it), for the queries that read them.
#[derive(Debug, Clone, Copy)]
enum Detail {
    /// Exact lengths and smoothness ([`Kernel::edges`]).
    Full,
    /// Neither: lengths 0, none smooth (naming, tangent chains).
    Light,
    /// Smoothness; the lengths below this exact, the others their chord (no more than the
    /// length, and no less than this): a display's test for slivers.
    Shown(f64),
}

/// [`edge_infos`] with what `detail` asks for.
fn edge_infos_with(b: &Body, detail: Detail) -> Vec<EdgeInfo> {
    use rayon::prelude::*;
    let ef = b.edge_faces();
    // (Side by side: each edge's smoothness projects onto its faces, a part's display a
    // tenth of a second of a rebuild's own thread.)
    let one = |(ei, e): (usize, &super::topo::Edge)| {
        let faces = match &ef[ei] {
            [] => [None, None],
            [a] => [Some(FaceId(*a as u64)), None],
            [a, b, ..] => [Some(FaceId(*a as u64)), Some(FaceId(*b as u64))],
        };
        let (t0, t1) = (e.t[0], e.t[1]);
        let tan = |t: f64| {
            let d = e.curve.tangent(t);
            let l = d.norm();
            if l > 0.0 { d / l } else { d }
        };
        let circle = match &e.curve {
            Curve::Circle { c, x, y, r } if !e.degenerate => Some(Circle3 {
                center: *c,
                normal: nalgebra::Unit::new_normalize(x.cross(y)),
                radius: *r,
            }),
            _ => None,
        };
        let ellipse = match &e.curve {
            Curve::Ellipse { c, x, y, a, b } if !e.degenerate => Some(if a >= b {
                Ellipse3 {
                    center: *c,
                    major: x * *a,
                    minor: y * *b,
                }
            } else {
                Ellipse3 {
                    center: *c,
                    major: y * *b,
                    minor: x * *a,
                }
            }),
            _ => None,
        };
        EdgeInfo {
            id: EdgeId(ei as u64),
            faces,
            length: match detail {
                _ if e.degenerate => 0.0,
                Detail::Light => 0.0,
                Detail::Full => e.curve.length(t0, t1),
                Detail::Shown(short) => {
                    let chord = (b.verts[e.v[1]].p - b.verts[e.v[0]].p).norm();
                    if chord < short {
                        e.curve.length(t0, t1)
                    } else {
                        chord
                    }
                }
            },
            curve: curve_kind(e),
            start: b.verts[e.v[0]].p,
            end: b.verts[e.v[1]].p,
            mid: if e.degenerate {
                b.verts[e.v[0]].p
            } else {
                e.point(e.mid_t())
            },
            start_tangent: tan(t0),
            end_tangent: tan(t1),
            circle,
            ellipse,
            // (Not without the lengths, for tangent chains: those ask of the edges they
            // reach only, [`NativeKernel::tangent_chain`].)
            smooth: !matches!(detail, Detail::Light)
                && super::blend::smooth_within(b, ei, TANGENT_ANGLE),
        }
    };
    if super::par::threads_for(b.edges.len(), false) {
        b.edges.par_iter().enumerate().map(one).collect()
    } else {
        b.edges.iter().enumerate().map(one).collect()
    }
}

/// Copies the listed faces (with the edges and vertices they use) into a new body; returns it
/// and the new index of each listed face.
pub fn subset(b: &Body, faces: &[usize], closed: bool) -> Body {
    let mut out = Body::default();
    let mut vmap: HashMap<usize, usize> = HashMap::default();
    let mut emap: HashMap<usize, usize> = HashMap::default();
    for &fi in faces {
        let mut f = b.faces[fi].clone();
        for l in &mut f.loops {
            for c in l.iter_mut() {
                let ne = *emap.entry(c.edge).or_insert_with(|| {
                    let mut e = b.edges[c.edge].clone();
                    for v in e.v.iter_mut() {
                        let ov = *v;
                        *v = *vmap.entry(ov).or_insert_with(|| {
                            out.verts.push(b.verts[ov].clone());
                            out.verts.len() - 1
                        });
                    }
                    out.edges.push(e);
                    out.edges.len() - 1
                });
                c.edge = ne;
            }
        }
        out.faces.push(f);
    }
    out.shells.push(super::topo::Shell {
        faces: (0..faces.len()).collect(),
        closed,
    });
    if closed {
        out.solids.push(super::topo::Solid { shells: vec![0] });
    }
    out
}

/// The index among [`Kernel::vertices`] of each vertex of the body (those edges end at, and the
/// acorn points), None for the others.
pub fn vertex_ids(b: &Body) -> Vec<Option<usize>> {
    let mut used = vec![false; b.verts.len()];
    for e in &b.edges {
        used[e.v[0]] = true;
        used[e.v[1]] = true;
    }
    for &v in &b.points {
        used[v] = true;
    }
    let mut k = 0;
    used.iter()
        .map(|&u| {
            u.then(|| {
                k += 1;
                k - 1
            })
        })
        .collect()
}

/// A body's wires and points alone (their vertices and edges copied), if it has any.
pub fn loose_part(b: &Body) -> Option<Body> {
    if b.wires.is_empty() && b.points.is_empty() {
        return None;
    }
    let mut out = Body::default();
    let mut vmap: HashMap<usize, usize> = HashMap::default();
    let mut vert = |out: &mut Body, v: usize| {
        *vmap.entry(v).or_insert_with(|| {
            out.verts.push(b.verts[v].clone());
            out.verts.len() - 1
        })
    };
    for w in &b.wires {
        let mut nw = super::topo::Wire::default();
        for &(e, fwd) in &w.edges {
            let mut ed = b.edges[e].clone();
            ed.v = [vert(&mut out, ed.v[0]), vert(&mut out, ed.v[1])];
            out.edges.push(ed);
            nw.edges.push((out.edges.len() - 1, fwd));
        }
        out.wires.push(nw);
    }
    for &v in &b.points {
        let nv = vert(&mut out, v);
        out.points.push(nv);
    }
    Some(out)
}

/// The wire body of [`Kernel::create_wire`]: the curves chained end to end, the vertices at
/// their joints (with the tolerance of the joints' gaps).
fn wire_body(k: &NativeKernel, curves: &[PathCurve]) -> Result<Body> {
    wire_of_segments(super::sweep::path::path_segments(k, curves)?)
}

/// A wire body of 3D curves, each over [t0, t1] (run backwards when t0 > t1), joined end to end.
pub fn wire_of_curves(curves: &[(Curve, f64, f64)]) -> Result<Body> {
    wire_of_segments(
        curves
            .iter()
            .map(|(c, t0, t1)| super::sweep::path::Seg {
                c: c.clone(),
                t0: t0.min(*t1),
                t1: t0.max(*t1),
                rev: t0 > t1,
            })
            .collect(),
    )
}

fn wire_of_segments(segs: Vec<super::sweep::path::Seg>) -> Result<Body> {
    if segs.is_empty() {
        return Err(KernelError::InvalidParameter("A wire needs a curve".into()));
    }
    let (segs, closed) = super::sweep::path::chain(segs).map_err(|_| {
        KernelError::InvalidParameter("The wire's curves must join end to end in one chain".into())
    })?;
    let mut b = Body::default();
    let first = b.add_vertex(segs[0].start());
    let mut at = first;
    let mut w = super::topo::Wire::default();
    for (i, s) in segs.iter().enumerate() {
        let next = if closed && i + 1 == segs.len() {
            first
        } else {
            b.add_vertex(s.end())
        };
        let e = if s.rev {
            b.add_edge(s.c.clone(), s.t0, s.t1, next, at)
        } else {
            b.add_edge(s.c.clone(), s.t0, s.t1, at, next)
        };
        w.edges.push((e, !s.rev));
        at = next;
    }
    b.wires.push(w);
    b.fit_tolerances(1e-4);
    if let Some(why) = b
        .check()
        .into_iter()
        .next()
        .or_else(|| b.size_issue(super::tol::SIZE))
    {
        return Err(KernelError::InvalidParameter(format!(
            "the wire isn't valid: {why}"
        )));
    }
    Ok(b)
}

impl Kernel for NativeKernel {
    fn name(&self) -> &'static str {
        "native"
    }

    fn extrude(&mut self, profile: &Profile, extent: Extent) -> Result<OpResult> {
        let n = profile.plane.normal.into_inner();
        let (s0, s1) = match extent {
            Extent::Blind(d) => (0.0, d),
            Extent::Symmetric(d) => (-d.abs() / 2.0, d.abs() / 2.0),
            Extent::TwoSided { forward, backward } => (-backward, forward),
        };
        if !(s0.is_finite() && s1.is_finite()) {
            return Err(KernelError::InvalidParameter(format!(
                "an extrusion's distances must be finite: {extent:?}"
            )));
        }
        let (dir, s0, s1) = if s1 >= s0 {
            (n, s0, s1)
        } else {
            (-n, -s0, -s1)
        };
        let regions = profile::regions(profile)?;
        if regions.is_empty() {
            return Err(KernelError::InvalidProfile("no regions".into()));
        }
        let faces = profile::merge_regions(&regions)?;
        let kind = super::sweep::Prism {
            plane: profile.plane,
            dir,
        };
        let b = super::sweep::sweep(&profile.plane, &faces, &kind, s0, s1, false)?;
        Ok(self.insert_result(b))
    }

    fn extrude_with(&mut self, profile: &Profile, spec: &ExtrudeSpec) -> Result<OpResult> {
        let r = super::features::extrude_with(self, profile, spec)?;
        // A gap the profile closed is how far its curves and the faces they sweep may be from
        // where the sketch drew them: its edges and vertices carry it (OCCT's did), so a
        // boolean finds the faces of the sketch's other features there coincident.
        let gap = profile::closed_gap(profile);
        if gap > profile::JOIN {
            for id in &r.bodies {
                // (Just made: its entry is this session's alone.)
                if let Some(e) = self.bodies.get_mut(&id.0).and_then(Arc::get_mut) {
                    profile::carry_gap(Arc::make_mut(&mut e.body), gap);
                }
            }
        }
        Ok(r)
    }

    fn revolve(&mut self, profile: &Profile, axis: Axis, angle: f64) -> Result<OpResult> {
        let spec = RevolveSpec {
            full: angle.abs() >= TAU - tol::ANGULAR_FINE,
            ..RevolveSpec::solid(axis, Some(angle))
        };
        self.revolve_with(profile, &spec)
    }

    fn revolve_with(&mut self, profile: &Profile, spec: &RevolveSpec) -> Result<OpResult> {
        super::features::revolve_with(self, profile, spec)
    }

    fn loft_with(&mut self, spec: &LoftSpec) -> Result<OpResult> {
        super::loft_build::loft_with(self, spec)
    }

    fn loft_guided(&mut self, spec: &LoftSpec, guides: &[Vec<PathCurve>]) -> Result<OpResult> {
        let r = super::loft_build::loft_guided(self, spec, guides);
        self.colored(r)
    }

    fn loft_curves(&mut self, spec: &CurveLoftSpec) -> Result<OpResult> {
        super::curve_loft::loft_curves(self, spec)
    }

    fn project_curve(&self, spec: &ProjectCurveSpec) -> Result<Vec<Bspline3>> {
        super::curve_split::project_curve(self, spec)
    }

    fn face_blend(&mut self, spec: &FaceBlendSpec) -> Result<OpResult> {
        super::face_blend::face_blend(self, spec)
    }

    fn sweep_curves(&mut self, spec: &CurveSweepSpec) -> Result<OpResult> {
        super::curve_loft::sweep_curves(self, spec)
    }

    fn evolved(&mut self, spine: &[PathCurve], profile: &[Region]) -> Result<OpResult> {
        let segs = super::sweep::path::path_segments(self, spine)?;
        let (segs, _) = super::sweep::path::chain(segs).map_err(|_| {
            KernelError::InvalidParameter(
                "The spine's curves must join end to end in one chain".into(),
            )
        })?;
        let spine: Vec<(Curve, f64, f64)> = segs
            .into_iter()
            .map(|s| {
                if s.rev {
                    (s.c, s.t1, s.t0)
                } else {
                    (s.c, s.t0, s.t1)
                }
            })
            .collect();
        let id = super::evolved::evolved(self, &spine, profile)?;
        Ok(OpResult {
            bodies: vec![id],
            history: History::default(),
        })
    }

    fn sweep_with(&mut self, profile: &Profile, spec: &SweepSpec) -> Result<OpResult> {
        let r = super::sweep::path::sweep_with(self, profile, spec)?;
        // A round section swept along a path from its centre, square to it: a pipe.
        if let ([body], [region]) = (r.bodies.as_slice(), profile.regions.as_slice())
            && let [Curve2::Circle { center, radius, .. }] = region.outer.curves.as_slice()
            && region.holes.is_empty()
            && matches!(spec.body, BodyKind::Solid)
            && matches!(spec.control, SweepControl::None)
            && spec.faces.is_empty()
            && let Ok(segs) = super::sweep::path::path_segments(self, &spec.path)
            && let Ok((segs, false)) = super::sweep::path::chain(segs)
            && let Some(first) = segs.first()
        {
            let pl = &profile.plane;
            let y = pl.normal.cross(&pl.x_dir);
            let c = pl.origin + pl.x_dir.into_inner() * center.x + y * center.y;
            let rad = radius.abs();
            if (c - first.start()).norm() <= 1e-9 * (1.0 + rad) && {
                let (_, d1, _) = first.c.d2(if first.rev { first.t1 } else { first.t0 });
                // (Square to within two thousandths of a radian, which leaves the
                // pipe round to two millionths of its radius: a helix drawn in conic
                // pieces sets out that far off the helix's own tangent.)
                d1.normalize().cross(&pl.normal).norm() <= 2e-3
            } {
                let segs = segs
                    .iter()
                    .map(|g| {
                        if g.rev {
                            (g.c.clone(), g.t1, g.t0)
                        } else {
                            (g.c.clone(), g.t0, g.t1)
                        }
                    })
                    .collect();
                self.pipes.insert(
                    body.0,
                    Arc::new(super::sweep_pipe::Pipe {
                        segs,
                        r: rad,
                        balls: [false; 2],
                    }),
                );
            }
        }
        Ok(r)
    }

    fn sweep_solid(&mut self, body: BodyId, motion: &SolidMotion) -> Result<OpResult> {
        super::sweep_solid::clear_declined();
        if let Some((m, moved)) = canonical_motion(self, &[body], motion) {
            return self.in_frame(&[body], m, |k, ids| {
                super::sweep_cache::sweep_solid(k, ids[0], &moved)
            });
        }
        super::sweep_cache::sweep_solid(self, body, motion)
    }

    fn cut_swept(
        &mut self,
        target: BodyId,
        tool: BodyId,
        motion: &SolidMotion,
    ) -> Result<OpResult> {
        super::sweep_solid::clear_declined();
        if let Some((m, moved)) = canonical_motion(self, &[target, tool], motion) {
            return self.in_frame(&[target, tool], m, |k, ids| {
                super::sweep_solid::cut_swept(k, ids[0], ids[1], &moved)
            });
        }
        super::sweep_solid::cut_swept(self, target, tool, motion)
    }

    fn cut_swept_exact(
        &mut self,
        target: BodyId,
        tool: BodyId,
        motion: &SolidMotion,
    ) -> Result<OpResult> {
        super::sweep_solid::clear_declined();
        super::sweep_solid::exact_only(|| super::sweep_solid::cut_swept(self, target, tool, motion))
    }

    fn cut_swept_indexed(
        &mut self,
        target: BodyId,
        tool: BodyId,
        motion: &SolidMotion,
        copies: usize,
    ) -> Result<OpResult> {
        super::sweep_solid::cut_swept_indexed(self, target, tool, motion, copies)
    }

    fn subtract_turned(
        &mut self,
        target: BodyId,
        tool: BodyId,
        axis: Axis,
        count: usize,
    ) -> Result<OpResult> {
        super::sweep_generate::subtract_turned(self, target, tool, &axis, count)
    }

    fn primitive(&mut self, p: &Primitive) -> Result<OpResult> {
        use nalgebra::{Point2, Unit};
        let bad = |what: &str| {
            Err(KernelError::InvalidParameter(format!(
                "The {what} must be positive"
            )))
        };
        let line = |a: (f64, f64), b: (f64, f64)| Curve2::Line {
            a: Point2::new(a.0, a.1),
            b: Point2::new(b.0, b.1),
            source: None,
        };
        let region = |curves: Vec<Curve2>| Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        };
        // A plane through `o` containing the axis `z`, its sketch x square to it and y along it.
        let through = |o: Point3<f64>, z: V3| {
            let z = z.normalize();
            let x = any_perpendicular(&z);
            Plane {
                origin: o,
                x_dir: Unit::new_normalize(x),
                normal: Unit::new_normalize(x.cross(&z)),
            }
        };
        let turned = |k: &mut Self, plane: Plane, curves: Vec<Curve2>, o: Point3<f64>, z: V3| {
            let axis = Axis {
                origin: o,
                dir: Unit::new_normalize(z),
            };
            k.revolve_with(
                &Profile::new(plane, vec![region(curves)]),
                &RevolveSpec::solid(axis, None),
            )
        };
        match *p {
            Primitive::Box { min, max } => {
                let d = max - min;
                if !(d.x > 0.0 && d.y > 0.0 && d.z > 0.0) {
                    return bad("box's size");
                }
                let plane = Plane {
                    origin: Point3::new(0.0, 0.0, min.z),
                    x_dir: Unit::new_normalize(V3::x()),
                    normal: Unit::new_normalize(V3::z()),
                };
                let (x0, y0, x1, y1) = (min.x, min.y, max.x, max.y);
                let r = region(vec![
                    line((x0, y0), (x1, y0)),
                    line((x1, y0), (x1, y1)),
                    line((x1, y1), (x0, y1)),
                    line((x0, y1), (x0, y0)),
                ]);
                self.extrude_with(
                    &Profile::new(plane, vec![r]),
                    &ExtrudeSpec::blind(plane.normal, d.z),
                )
            }
            Primitive::Cylinder {
                base,
                axis,
                radius,
                height,
            } => {
                if !(radius > 0.0 && height > 0.0) {
                    return bad("cylinder's radius and height");
                }
                self.cylinder_seamed(base, axis, radius, height, any_perpendicular(&axis))
            }
            Primitive::Cone {
                base,
                axis,
                base_radius: r0,
                top_radius: r1,
                height: h,
            } => {
                if !(h > 0.0 && r0 >= 0.0 && r1 >= 0.0 && r0 + r1 > 0.0) {
                    return bad("cone's height and a radius");
                }
                let mut pts = vec![(0.0, 0.0)];
                if r0 > 0.0 {
                    pts.push((r0, 0.0));
                }
                if r1 > 0.0 {
                    pts.push((r1, h));
                }
                pts.push((0.0, h));
                let curves = (0..pts.len())
                    .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
                    .collect();
                let plane = through(base, axis.into_inner());
                turned(self, plane, curves, base, axis.into_inner())
            }
            Primitive::Sphere {
                center,
                radius,
                axis,
                seam,
            } => {
                if !(radius > 0.0) {
                    return bad("sphere's radius");
                }
                let z = axis.into_inner();
                let x = (seam.into_inner() - z * seam.dot(&z))
                    .try_normalize(tol::ANGULAR)
                    .unwrap_or_else(|| any_perpendicular(&z));
                let b = super::blend::ball_frame(center, radius, z, x)?;
                let n = b.faces.len();
                let id = self.insert(b);
                Ok(OpResult {
                    bodies: vec![id],
                    history: History {
                        generated: (0..n)
                            .map(|i| {
                                (
                                    FaceId(i as u64),
                                    Origin::ProfileCurve {
                                        region: 0,
                                        curve: i as u64,
                                    },
                                )
                            })
                            .collect(),
                        ..Default::default()
                    },
                })
            }
            Primitive::Torus {
                center,
                axis,
                major,
                minor,
                spindle,
            } => {
                if !(minor > 0.0 && major >= 0.0) {
                    return Err(KernelError::InvalidParameter(
                        "The torus needs a tube and a ring that is no less than none".into(),
                    ));
                }
                // A ring torus's tube turned whole; a horn torus's (the tube as wide as the ring)
                // touches the axis at the centre. A tube wider than its ring crosses the axis and
                // describes no torus: refused (a spindle is a revolve of the tube's outer part).
                if minor > major * (1.0 + tol::REL) {
                    if spindle == Spindle::Refuse {
                        return Err(KernelError::InvalidParameter(
                            "The torus's tube is wider than its ring: it crosses the axis".into(),
                        ));
                    }
                    // The tube circle's outer arc, from where it crosses the axis below round
                    // through (major + minor, 0) to where it crosses above, closed along the axis.
                    let h = (minor * minor - major * major).sqrt();
                    let half = std::f64::consts::PI - h.atan2(major);
                    let plane = through(center, axis.into_inner());
                    let outline = vec![
                        Curve2::arc(Point2::new(major, 0.0), minor, -half, 2.0 * half),
                        line((0.0, h), (0.0, -h)),
                    ];
                    return turned(self, plane, outline, center, axis.into_inner());
                }
                let plane = through(center, axis.into_inner());
                let tube = vec![Curve2::Circle {
                    center: Point2::new(major, 0.0),
                    radius: minor,
                    source: None,
                }];
                turned(self, plane, tube, center, axis.into_inner())
            }
        }
    }

    fn screw(&mut self, profile: &Profile, spec: &ScrewSpec) -> Result<OpResult> {
        super::constructors::screw(self, profile, spec)
    }

    fn slab(&mut self, spec: &SlabSpec) -> Result<OpResult> {
        let b = super::constructors::slab(spec)?;
        Ok(OpResult {
            bodies: vec![self.insert_sound(b)],
            history: History::default(),
        })
    }

    fn sweep_with_law(
        &mut self,
        profile: &Profile,
        spec: &SweepSpec,
        law: &SweepLaw,
    ) -> Result<OpResult> {
        super::sweep::path::sweep_with_law(self, profile, spec, law)
    }

    fn sweep_chain(&mut self, pieces: &[(Profile, SweepSpec, SweepLaw)]) -> Result<OpResult> {
        super::sweep::chain::sweep_chain(self, pieces)
    }

    fn cells(&mut self, bodies: &[BodyId], select: &[(usize, bool)]) -> Result<Vec<BodyId>> {
        for &b in bodies {
            self.get(b)?;
        }
        if let Some((i, _)) = select.iter().find(|(i, _)| *i >= bodies.len()) {
            return Err(KernelError::InvalidParameter(format!(
                "The cell selection names body {i} of {}",
                bodies.len()
            )));
        }
        super::cells::select_cells(self, bodies, select)
    }

    fn boolean(&mut self, op: BoolOp, target: BodyId, tools: &[BodyId]) -> Result<OpResult> {
        // The union [`Kernel::joins`] just made of the same two bodies.
        if let (BoolOp::Union, [tool]) = (op, tools)
            && let Some(i) = self
                .joined
                .iter()
                .position(|(a, b, _)| *a == target && b == tool)
        {
            return Ok(self.joined.remove(i).2);
        }
        // Two swept tori: by what each motion holds.
        if let [tool] = tools
            && let (Some(a), Some(b)) = (
                self.swept_tori.get(&target.0).cloned(),
                self.swept_tori.get(&tool.0).cloned(),
            )
            && let Some(Ok(r)) = super::sweep_tube::boolean(self, op, &a, &b)
        {
            return self.colored(Ok(r));
        }
        // The inputs' boxes, made once for all the operation asks of them.
        let held: Vec<(Arc<Body>, super::topo::Boxes)> = std::iter::once(&target)
            .chain(tools)
            .filter_map(|id| {
                let e = self.entry(*id).ok()?;
                Some((Arc::clone(&e.body), e.boxes().clone()))
            })
            .collect();
        let _kept: Vec<super::topo::KeptBoxes> = held
            .iter()
            .map(|(b, made)| super::topo::keep_boxes_made(b, made))
            .collect();
        // With the tool's faces that lie a hair off the target's moved onto them, the history
        // naming the tool's own faces.
        let on_snapped = |k: &mut Self, snapped: Body| -> Result<OpResult> {
            let tool = tools[0];
            let id = k.insert(snapped);
            let r = super::boolean::boolean(k, op, target, &[id]);
            k.release(id);
            r.map(|mut res| {
                for (_, from) in res.history.modified.iter_mut() {
                    if from.body == id {
                        from.body = tool;
                    }
                }
                for from in res.history.deleted.iter_mut() {
                    if from.body == id {
                        from.body = tool;
                    }
                }
                res
            })
        };
        let snap = !crate::native::env_flag!("CADRS_NO_SNAP") && tools.len() == 1;
        // Faces within a couple of point tolerances all over first (coaxial cylinders whose radii
        // differ by 2 µm: a sliver of tube the boolean places in some places and not others).
        let first = snap
            .then(|| super::snap::snapped(self.get(target).ok()?, self.get(tools[0]).ok()?, true))
            .flatten();
        let mut r = match first {
            Some(snapped) => on_snapped(self, snapped),
            None => super::boolean::boolean(self, op, target, tools),
        };
        // Refused with a tool whose faces graze the target's a hair off them (a copy turned by a
        // microradian): again with those faces moved onto the target's surfaces.
        if r.is_err()
            && snap
            && let Some(snapped) =
                super::snap::snapped(self.get(target)?, self.get(tools[0])?, false)
        {
            match on_snapped(self, snapped) {
                Ok(res) => r = Ok(res),
                // (Nothing left is an answer too: a box less its copy turned a hair.)
                Err(e @ KernelError::EmptyResult(_)) => r = Err(e),
                Err(_) => {}
            }
        }
        // A result with a vertex inside a face's own seam (a cylinder's seam line running
        // through the corner where the boolean's cut meets it again: its loop folds back over
        // the seam, which no mesh follows): again with an operand cylinder's seam turned half a
        // turn, kept when that one is clear of it.
        if let (Ok(res), [tool]) = (&r, tools)
            && !crate::native::env_flag!("CADRS_NO_SEAM_TURN")
            && res.bodies.iter().any(|id| self.seam_through_a_vertex(*id))
        {
            for which in [*tool, target] {
                let Some(turned) = self.get(which).ok().and_then(|b| cylinder_turned(b)) else {
                    continue;
                };
                let Ok(made) =
                    self.cylinder_seamed(turned.0, turned.1, turned.2, turned.3, turned.4)
                else {
                    continue;
                };
                let [id] = made.bodies.as_slice() else {
                    continue;
                };
                let id = *id;
                let again = if which == *tool {
                    super::boolean::boolean(self, op, target, &[id])
                } else {
                    super::boolean::boolean(self, op, id, tools)
                };
                self.release(id);
                let Ok(mut res2) = again else { continue };
                if res2.bodies.iter().any(|b| self.seam_through_a_vertex(*b)) {
                    for b in &res2.bodies {
                        self.release(*b);
                    }
                    continue;
                }
                for (_, from) in res2.history.modified.iter_mut() {
                    if from.body == id {
                        from.body = which;
                    }
                }
                for from in res2.history.deleted.iter_mut() {
                    if from.body == id {
                        from.body = which;
                    }
                }
                if let Ok(old) = &r {
                    for b in old.bodies.clone() {
                        self.release(b);
                    }
                }
                r = Ok(res2);
                break;
            }
        }
        // A pipe united with a ball of its radius about an end of its spine: a pipe closed
        // there.
        if let (BoolOp::Union, [tool], Ok(res)) = (op, tools, &r)
            && let [made] = res.bodies.as_slice()
        {
            for (pipe, ball) in [(target, *tool), (*tool, target)] {
                let Some(pipe) = self.pipes.get(&pipe.0).cloned() else {
                    continue;
                };
                let Ok(b) = self.get(ball) else {
                    continue;
                };
                let [f] = b.faces.as_slice() else {
                    continue;
                };
                let super::surface::Surface::Sphere { f: fr, r: rs } = &f.surf else {
                    continue;
                };
                let (centre, rs) = (fr.o, *rs);
                if (rs - pipe.r).abs() > 1e-9 * pipe.r {
                    continue;
                }
                let (first, last) = (&pipe.segs[0], &pipe.segs[pipe.segs.len() - 1]);
                let ends = [first.0.point(first.1), last.0.point(last.2)];
                for (e, end) in ends.iter().enumerate() {
                    if (centre - end).norm() <= 1e-9 * (1.0 + pipe.r) {
                        let mut closed = (*pipe).clone();
                        closed.balls[e] = true;
                        self.pipes.insert(made.0, Arc::new(closed));
                    }
                }
            }
        }
        // With CADRS_DUMP_DIR set, a failed boolean leaves its inputs there.
        if r.is_err() && crate::native::env_flag!("CADRS_DUMP_DIR") {
            dump("boolean-target", self.get(target)?, &format!("{op:?}"));
            for t in tools {
                dump("boolean-tool", self.get(*t)?, &format!("{op:?}"));
            }
            let _ = self.dumped(&r);
        }
        self.colored(r)
    }

    fn fillet(&mut self, body: BodyId, edges: &[EdgeId], radius: f64) -> Result<OpResult> {
        no_edges(edges, "fillet")?;
        dump(
            "fillet",
            self.get(body)?,
            &format!("edges {edges:?} radius {radius}"),
        );
        self.at_working_size(body, |k, body, by| {
            let r = super::blend::fillet(k, body, edges, &FilletSpec::radius(radius * by));
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn fillet_with(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        spec: &FilletSpec,
    ) -> Result<OpResult> {
        no_edges(edges, "fillet")?;
        dump(
            "fillet",
            self.get(body)?,
            &format!("edges {edges:?} {spec:?}"),
        );
        self.at_working_size(body, |k, body, by| {
            let r = super::blend::fillet(k, body, edges, &fillet_scaled(spec, by));
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn fillet_smooth(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        spec: &FilletSpec,
        setback: f64,
    ) -> Result<OpResult> {
        dump(
            "fillet",
            self.get(body)?,
            &format!("edges {edges:?} {spec:?} smooth {setback}"),
        );
        self.at_working_size(body, |k, body, by| {
            let spec = fillet_scaled(spec, by);
            let r = super::blend_corner::fillet_smooth(k, body, edges, &spec, setback * by);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn fillet_setback(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        spec: &FilletSpec,
        setback: f64,
    ) -> Result<OpResult> {
        no_edges(edges, "fillet")?;
        self.at_working_size(body, |k, body, by| {
            let spec = fillet_scaled(spec, by);
            let r = super::blend_setback::fillet_setback(k, body, edges, &spec, setback * by);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn fillet_faces(
        &mut self,
        body: BodyId,
        face_a: FaceId,
        face_b: FaceId,
        radius: f64,
    ) -> Result<OpResult> {
        self.at_working_size(body, |k, body, by| {
            let r = super::face_fillet::fillet_faces(k, body, face_a, face_b, radius * by);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn thicken_surfaces(&mut self, spec: &ThickenSpec) -> Result<OpResult> {
        super::surfacing::thicken(self, spec)
    }

    fn fill(&mut self, spec: &FillSpec) -> Result<OpResult> {
        super::surfacing::fill(self, spec)
    }

    fn sew_sheets(&mut self, bodies: &[BodyId], tol: f64) -> Result<OpResult> {
        super::surfacing::sew_sheets(self, bodies, tol)
    }

    fn sew_step(&mut self, bytes: &[u8], tol: f64) -> Result<OpResult> {
        if !(tol > 0.0 && tol.is_finite()) {
            return Err(KernelError::InvalidParameter(format!(
                "The sewing tolerance must be positive (it is {tol})"
            )));
        }
        let bodies = self.import_step(bytes)?;
        if bodies.is_empty() {
            return Err(KernelError::InvalidInput(
                "The STEP file holds no faces to sew".into(),
            ));
        }
        let r = self.sew_solid(&bodies, tol);
        for b in bodies {
            self.release(b);
        }
        r
    }

    fn sew_solid(&mut self, bodies: &[BodyId], tol: f64) -> Result<OpResult> {
        let r = super::surfacing::sew(self, bodies, tol);
        self.colored(r)
    }

    fn simplify(&mut self, body: BodyId) -> Result<OpResult> {
        let r = super::surfacing::simplify(self, body);
        self.colored(r)
    }

    fn heal(&mut self, body: BodyId, ops: &[HealOp]) -> Result<OpResult> {
        let r = super::heal::heal_op(self, body, ops);
        self.colored(r)
    }

    fn delete_faces(&mut self, body: BodyId, faces: &[FaceId]) -> Result<OpResult> {
        self.at_working_size(body, |k, body, _| {
            let r = super::surfacing::delete_faces(k, body, faces);
            k.colored(r)
        })
    }

    fn offset(&mut self, body: BodyId, spec: &OffsetSpec) -> Result<OpResult> {
        self.in_own_frame(body, |k, body| {
            k.at_size(body, true, |k, body, by| {
                let spec = OffsetSpec {
                    distance: spec.distance * by,
                    faces: spec.faces.iter().map(|&(f, d)| (f, d * by)).collect(),
                    sharp: spec.sharp,
                };
                let r = super::offset::offset(k, body, &spec);
                k.colored(r)
            })
        })
    }

    fn offset_meshed(&mut self, body: BodyId, distance: f64) -> Result<OpResult> {
        super::sweep_solid::offset_meshed(self, body, distance)
    }

    fn draft(&mut self, body: BodyId, spec: &DraftSpec) -> Result<OpResult> {
        self.at_working_size(body, |k, body, by| {
            let spec = DraftSpec {
                neutral: Point3::from(spec.neutral.coords * by),
                ..spec.clone()
            };
            let r = super::local::draft(k, body, &spec);
            k.colored(r)
        })
    }

    fn replace_faces(&mut self, body: BodyId, faces: &[FaceReplacement]) -> Result<OpResult> {
        let r = super::local::replace_faces(self, body, faces);
        self.colored(r)
    }

    fn move_faces(&mut self, body: BodyId, faces: &[FaceId], motion: &Motion) -> Result<OpResult> {
        let r = super::local::move_faces(self, body, faces, motion);
        self.colored(r)
    }

    fn shell(&mut self, body: BodyId, remove: &[FaceId], thickness: f64) -> Result<OpResult> {
        dump(
            "shell",
            self.get(body)?,
            &format!("remove {remove:?} thickness {thickness}"),
        );
        self.at_working_size(body, |k, body, by| {
            let r = super::offset::round::shell(
                k,
                body,
                &ShellSpec {
                    remove: remove.to_vec(),
                    thickness: thickness * by,
                    outward: false,
                    hollow: false,
                    sharp: false,
                    nothing_hollowed: Default::default(),
                },
            );
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn shell_with(&mut self, body: BodyId, spec: &ShellSpec) -> Result<OpResult> {
        dump("shell", self.get(body)?, &format!("{spec:?}"));
        self.at_working_size(body, |k, body, by| {
            let spec = ShellSpec {
                thickness: spec.thickness * by,
                ..spec.clone()
            };
            let r = super::offset::round::shell(k, body, &spec);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn fillet_partial(
        &mut self,
        body: BodyId,
        edge: EdgeId,
        spec: &FilletSpec,
        from: f64,
        to: f64,
    ) -> Result<OpResult> {
        self.at_working_size(body, |k, body, by| {
            let r = super::blend::fillet_partial(k, body, edge, &fillet_scaled(spec, by), from, to);
            k.colored(r)
        })
    }

    fn fillet_variable(
        &mut self,
        body: BodyId,
        laws: &[FilletLaw],
        allow_overflow: bool,
    ) -> Result<OpResult> {
        dump(
            "fillet_variable",
            self.get(body)?,
            &format!("{laws:?} overflow {allow_overflow}"),
        );
        self.at_working_size(body, |k, body, by| {
            let laws: Vec<FilletLaw> = laws
                .iter()
                .map(|l| FilletLaw {
                    edge: l.edge,
                    radii: l.radii.iter().map(|&(s, r)| (s, r * by)).collect(),
                })
                .collect();
            let r = super::blend::fillet_variable(k, body, &laws, allow_overflow);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn chamfer_variable(&mut self, body: BodyId, laws: &[ChamferLaw]) -> Result<OpResult> {
        dump("chamfer_variable", self.get(body)?, &format!("{laws:?}"));
        self.at_working_size(body, |k, body, by| {
            let scaled = |law: &[(f64, f64)]| law.iter().map(|&(t, d)| (t, d * by)).collect();
            let laws: Vec<ChamferLaw> = laws
                .iter()
                .map(|l| ChamferLaw {
                    edge: l.edge,
                    face: l.face,
                    first: scaled(&l.first),
                    second: scaled(&l.second),
                })
                .collect();
            let r = super::blend::chamfer_variable(k, body, &laws);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn full_round(
        &mut self,
        body: BodyId,
        side1: FaceId,
        center: FaceId,
        side2: FaceId,
    ) -> Result<OpResult> {
        self.at_working_size(body, |k, body, _| {
            let r = super::blend::full_round(k, body, side1, center, side2);
            k.colored(r)
        })
    }

    fn chamfer(&mut self, body: BodyId, edges: &[EdgeId], spec: ChamferSpec) -> Result<OpResult> {
        no_edges(edges, "chamfer")?;
        dump(
            "chamfer",
            self.get(body)?,
            &format!("edges {edges:?} {spec:?}"),
        );
        self.at_working_size(body, |k, body, by| {
            let opts = ChamferOpts::new(chamfer_scaled(spec, by));
            let r = super::blend::chamfer(k, body, edges, &opts);
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn chamfer_with(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        opts: &ChamferOpts,
    ) -> Result<OpResult> {
        no_edges(edges, "chamfer")?;
        dump(
            "chamfer",
            self.get(body)?,
            &format!("edges {edges:?} {opts:?}"),
        );
        self.at_working_size(body, |k, body, by| {
            let opts = &ChamferOpts {
                spec: chamfer_scaled(opts.spec, by),
                ..opts.clone()
            };
            // Without overflow, each chamfer must fit the faces at its edge.
            if !opts.overflow {
                let b = k.get(body)?.clone();
                let mut sized = Vec::new();
                for e in super::blend::real_edges(&b, edges)? {
                    let e = e.0 as usize;
                    let [s1, s2] = super::blend::edge_sides(&b, e)?;
                    let (d1, d2) = match opts.spec {
                        ChamferSpec::EqualDistance(d) => (d, d),
                        ChamferSpec::TwoDistances(a, c) => (a, c),
                        ChamferSpec::DistanceAngle { distance, angle } => {
                            (distance, distance * angle.tan())
                        }
                    };
                    let second_first = opts.first_on.contains(&FaceId(s2.0 as u64))
                        && !opts.first_on.contains(&FaceId(s1.0 as u64));
                    sized.push(if second_first {
                        (e, d2, d1)
                    } else {
                        (e, d1, d2)
                    });
                }
                super::blend::chamfers_fit(&b, &sized)?;
            }
            // A convex polyhedron's chamfers by distances: its planes' polytope, built at once.
            if opts.measurement == ChamferMeasure::Offset {
                let b = k.get(body)?.clone();
                let real = super::blend::real_edges(&b, edges)?;
                let flat = opts.corners == ChamferCorners::Flat;
                if let Some(sized) = super::blend::chamfer_distances(&b, &real, opts)
                    && let Some(r) = super::convex_chamfer::chamfer(k, body, &sized, flat)
                {
                    let r = k.dumped(&r);
                    return k.colored(r);
                }
            }
            let r = super::blend::chamfer(k, body, edges, opts);
            let r = match r {
                Ok(r) if opts.corners == ChamferCorners::Flat => {
                    super::blend::flat_corners(k, body, edges, opts, r)
                }
                r => r,
            };
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn chamfer_distances(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        distances: &[f64],
        overflow: bool,
    ) -> Result<OpResult> {
        no_edges(edges, "chamfer")?;
        if edges.len() != distances.len() {
            return Err(KernelError::InvalidParameter(format!(
                "{} edges but {} distances",
                edges.len(),
                distances.len()
            )));
        }
        if let Some(d) = distances.iter().find(|d| !(**d > 0.0 && d.is_finite())) {
            return Err(KernelError::InvalidParameter(format!(
                "A chamfer's distance must be positive (one is {d})"
            )));
        }
        // Each edge's tangent chain at its distance: one distance an edge.
        let mut each: Vec<(EdgeId, f64)> = Vec::new();
        for (&e, &d) in edges.iter().zip(distances) {
            for c in self.tangent_chain(body, e)? {
                match each.iter().find(|x| x.0 == c) {
                    Some(&(_, d0)) if d0 != d => {
                        return Err(KernelError::InvalidParameter(format!(
                            "{c:?} is given two chamfer distances, {d0} and {d}"
                        )));
                    }
                    Some(_) => {}
                    None => each.push((c, d)),
                }
            }
        }
        // One distance for all: the chamfer of one distance.
        if each.iter().all(|x| x.1 == each[0].1) {
            let ids: Vec<EdgeId> = each.iter().map(|x| x.0).collect();
            let opts = ChamferOpts {
                overflow,
                ..ChamferOpts::new(ChamferSpec::EqualDistance(each[0].1))
            };
            return self.chamfer_with(body, &ids, &opts);
        }
        dump(
            "chamfer",
            self.get(body)?,
            &format!("edges and distances {each:?}"),
        );
        self.at_working_size(body, |k, body, by| {
            let b = k.get(body)?.clone();
            let ids: Vec<EdgeId> = each.iter().map(|x| x.0).collect();
            let real = super::blend::real_edges(&b, &ids)?;
            let each: Vec<(usize, f64)> = each
                .iter()
                .filter(|x| real.contains(&x.0))
                .map(|&(e, d)| (e.0 as usize, d * by))
                .collect();
            // Without overflow, each chamfer must fit the faces at its edge.
            if !overflow {
                let sized: Vec<(usize, f64, f64)> = each.iter().map(|&(e, d)| (e, d, d)).collect();
                super::blend::chamfers_fit(&b, &sized)?;
            }
            let r = match super::blend::chamfer_each(k, body, &each) {
                Ok(r) => Ok(r),
                // Where the strips can't be trimmed against each other at a corner: each
                // distance's chamfer on its own, those solids intersected (after springrep's
                // blend::chamfer_distances, MIT), the corner where their planes meet.
                Err(first) => chamfer_groups(k, body, &each, overflow)
                    .map_err(|e| e.after("chamfering by trimming", &first)),
            };
            let r = k.dumped(&r);
            k.colored(r)
        })
    }

    fn split(&mut self, body: BodyId, tool: &SplitTool, source: u64) -> Result<OpResult> {
        let r = super::ops::split(self, body, tool, source);
        self.colored(r)
    }

    fn split_faces(
        &mut self,
        body: BodyId,
        faces: &[FaceId],
        tool: &SplitTool,
    ) -> Result<OpResult> {
        let r = super::ops::split_faces(self, body, faces, tool);
        self.colored(r)
    }

    fn face_tool(&mut self, body: BodyId, faces: &[FaceId], source: u64) -> Result<OpResult> {
        super::ops::face_tool(self, body, faces, source)
    }

    fn boundary_normals(
        &self,
        body: BodyId,
        points: &[Point3<f64>],
        within: f64,
    ) -> Result<Vec<Option<nalgebra::Vector3<f64>>>> {
        super::ops::boundary_normals(self, body, points, within)
    }

    fn face_normals_at(
        &self,
        body: BodyId,
        face: FaceId,
        points: &[Point3<f64>],
    ) -> Result<Vec<nalgebra::Vector3<f64>>> {
        super::ops::face_normals_at(self, body, face, points)
    }

    fn transform(&mut self, body: BodyId, t: &Transform) -> Result<OpResult> {
        self.transform_motion(body, &Motion::from_isometry(t))
    }

    fn transform_motion(&mut self, body: BodyId, motion: &Motion) -> Result<OpResult> {
        let m = motion.linear;
        if (m.transpose() * m - nalgebra::Matrix3::identity()).norm() > tol::ROUNDOFF {
            return Err(KernelError::InvalidParameter(
                "The motion must be a rotation or a reflection (no scaling or shear)".into(),
            ));
        }
        let moved = Affine {
            m,
            t: motion.translation,
        };
        let b = self.get(body)?.transformed(&moved);
        let nf = b.faces.len();
        let id = self.insert(b);
        self.place(id, body, &moved);
        let history = History {
            modified: (0..nf)
                .map(|i| {
                    (
                        FaceId(i as u64),
                        InputFace {
                            body,
                            face: FaceId(i as u64),
                        },
                    )
                })
                .collect(),
            ..Default::default()
        };
        self.colored(Ok(OpResult {
            bodies: vec![id],
            history,
        }))
    }

    fn scale(&mut self, body: BodyId, center: Point3<f64>, factor: f64) -> Result<OpResult> {
        if factor <= 0.0 {
            return Err(KernelError::InvalidParameter(
                "The scale must be positive".into(),
            ));
        }
        let m = nalgebra::Matrix3::identity() * factor;
        let b = self.get(body)?.transformed(&Affine {
            m,
            t: center.coords - m * center.coords,
        });
        let nf = b.faces.len();
        let id = self.insert(b);
        let history = History {
            modified: (0..nf)
                .map(|i| {
                    (
                        FaceId(i as u64),
                        InputFace {
                            body,
                            face: FaceId(i as u64),
                        },
                    )
                })
                .collect(),
            ..Default::default()
        };
        self.colored(Ok(OpResult {
            bodies: vec![id],
            history,
        }))
    }

    fn transform_affine(
        &mut self,
        body: BodyId,
        m: nalgebra::Matrix3<f64>,
        t: nalgebra::Vector3<f64>,
    ) -> Result<OpResult> {
        let map = Affine { m, t };
        let b = super::affine::affine_body(self.get(body)?, &map)?;
        let nf = b.faces.len();
        let from = self.entry_arc(body)?;
        let id = self.insert(b);
        // (A similarity keeps every surface's kind: nothing to gain by going back.)
        if !super::affine::similarity(&map)
            && let Some(inv) = m.try_inverse()
        {
            let back = Affine {
                m: inv,
                t: -(inv * t),
            };
            // (A map of a map goes back to the first body, the two maps back composed.)
            let (from, back) = match from.affine_of.get() {
                Some((first, b0, _)) => (
                    first.clone(),
                    Affine {
                        m: b0.m * back.m,
                        t: b0.m * back.t + b0.t,
                    },
                ),
                None => (from, back),
            };
            let stretch = back.m.svd(false, false).singular_values.max();
            if let Some(e) = self.bodies.get(&id.0) {
                let _ = e.affine_of.set((from, back, stretch));
            }
        }
        let history = History {
            modified: (0..nf)
                .map(|i| {
                    (
                        FaceId(i as u64),
                        InputFace {
                            body,
                            face: FaceId(i as u64),
                        },
                    )
                })
                .collect(),
            ..Default::default()
        };
        self.colored(Ok(OpResult {
            bodies: vec![id],
            history,
        }))
    }

    fn compound(&mut self, bodies: &[BodyId]) -> Result<OpResult> {
        if bodies.is_empty() {
            return Err(KernelError::InvalidParameter(
                "A composite needs at least one body".into(),
            ));
        }
        let mut out = Body::default();
        let mut history = History::default();
        for &b in bodies {
            let src = self.get(b)?.clone();
            let (_, _, fo) = out.append(&src);
            for i in 0..src.faces.len() {
                history.modified.push((
                    FaceId((fo + i) as u64),
                    InputFace {
                        body: b,
                        face: FaceId(i as u64),
                    },
                ));
            }
        }
        let id = self.insert(out);
        self.colored(Ok(OpResult {
            bodies: vec![id],
            history,
        }))
    }

    fn split_solids(&mut self, body: BodyId) -> Result<Vec<OpResult>> {
        let b = self.get(body)?.clone();
        let mut out = Vec::new();
        let groups: Vec<(Vec<usize>, bool)> = if b.solids.is_empty() {
            b.shells.iter().map(|s| (s.faces.clone(), false)).collect()
        } else {
            (0..b.solids.len())
                .map(|s| (b.solid_faces(s), true))
                .collect()
        };
        for (faces, closed) in groups {
            let mut nb = subset(&b, &faces, closed);
            // Keep the void shells of the solid.
            if closed && b.solids.len() == 1 && b.solids[0].shells.len() > 1 {
                nb.shells.clear();
                nb.solids.clear();
                let mut k = 0;
                let mut sh = Vec::new();
                for &s in &b.solids[0].shells {
                    let n = b.shells[s].faces.len();
                    nb.shells.push(super::topo::Shell {
                        faces: (k..k + n).collect(),
                        closed: true,
                    });
                    sh.push(nb.shells.len() - 1);
                    k += n;
                }
                nb.solids.push(super::topo::Solid { shells: sh });
            }
            let history = History {
                modified: faces
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        (
                            FaceId(i as u64),
                            InputFace {
                                body,
                                face: FaceId(*f as u64),
                            },
                        )
                    })
                    .collect(),
                ..Default::default()
            };
            let id = self.insert(nb);
            // (Faces that were copies of others in it still are, among the piece's faces.)
            if let Some(c) = self.entry(body)?.copies.get().cloned() {
                let mut new_of = vec![None; c.len()];
                for (i, &f) in faces.iter().enumerate() {
                    new_of[f] = Some(i);
                }
                let carried: super::copies::FaceCopies = faces
                    .iter()
                    .map(|&f| {
                        let (g, r) = c.get(f).copied().flatten()?;
                        Some((new_of[g]?, r))
                    })
                    .collect();
                let _ = self.entry(id)?.copies.set(Arc::new(carried));
            }
            let r = OpResult {
                bodies: vec![id],
                history,
            };
            self.carry_colors(&r);
            out.push(r);
        }
        // The wires and points, as one more body.
        if let Some(loose) = loose_part(&b) {
            let id = self.insert(loose);
            out.push(OpResult {
                bodies: vec![id],
                history: History::default(),
            });
        }
        Ok(out)
    }

    fn classify(&self, body: BodyId, point: Point3<f64>, tol: f64) -> Result<PointClass> {
        let e = self.entry(body)?;
        if let Some((from, back, stretch)) = e.affine_of.get() {
            return Ok(classify::classify_point(
                &from.body,
                from.polys(),
                &back.point(&point),
                tol * stretch,
            ));
        }
        Ok(classify::classify_point(&e.body, e.polys(), &point, tol))
    }

    fn classify_many(
        &self,
        body: BodyId,
        points: &[Point3<f64>],
        tol: f64,
    ) -> Result<Vec<PointClass>> {
        use rayon::prelude::*;
        let e = self.entry(body)?;
        // An affine map of another body: the points mapped back, classified against it (its
        // boundary's tolerance the map's most stretched back).
        if let Some((from, back, stretch)) = e.affine_of.get() {
            let back: Vec<Point3<f64>> = points.iter().map(|p| back.point(p)).collect();
            let c = classify::Classifier::new(&from.body, from.polys());
            let tol = tol * stretch;
            return Ok(if super::par::threads_for(back.len(), false) {
                back.par_iter().map(|p| c.classify(p, tol)).collect()
            } else {
                back.iter().map(|p| c.classify(p, tol)).collect()
            });
        }
        let c = classify::Classifier::new(&e.body, e.polys());
        Ok(if super::par::threads_for(points.len(), false) {
            points.par_iter().map(|p| c.classify(p, tol)).collect()
        } else {
            points.iter().map(|p| c.classify(p, tol)).collect()
        })
    }

    fn release(&mut self, body: BodyId) {
        self.bodies.remove(&body.0);
        self.colors.remove(&body.0);
    }

    fn body_class(&self, body: BodyId) -> Result<BodyClass> {
        Ok(self.get(body)?.class())
    }

    fn create_wire(&mut self, curves: &[PathCurve]) -> Result<OpResult> {
        let b = wire_body(self, curves)?;
        Ok(self.insert_result(Built {
            body: b,
            origins: Vec::new(),
        }))
    }

    fn create_points(&mut self, points: &[Point3<f64>]) -> Result<OpResult> {
        if points.is_empty() {
            return Err(KernelError::InvalidParameter(
                "A point body needs a point".into(),
            ));
        }
        let mut b = Body::default();
        for p in points {
            if !p.iter().all(|x| x.is_finite()) {
                return Err(KernelError::InvalidInput("a point isn't finite".into()));
            }
            b.add_point(*p);
        }
        if let Some(why) = b.size_issue(super::tol::SIZE) {
            return Err(KernelError::InvalidParameter(why));
        }
        Ok(self.insert_result(Built {
            body: b,
            origins: Vec::new(),
        }))
    }

    fn sweep_body(&mut self, body: BodyId, sweep: &BodySweep) -> Result<OpResult> {
        let b = self.get(body)?.clone();
        let built = match *sweep {
            BodySweep::Translate(v) => {
                let len = v.norm();
                if !len.is_finite() || len < super::build::MIN_DEPTH {
                    return Err(KernelError::InvalidParameter(format!(
                        "The sweep's length {len:e} is below the modelling tolerance"
                    )));
                }
                super::sweep::sweep_body(
                    &b,
                    &super::sweep::BodyMove::Straight(v / len),
                    len,
                    false,
                    body.0,
                )?
            }
            BodySweep::Rotate { axis, angle } => {
                if !angle.is_finite() || angle.abs() < tol::ANGULAR {
                    return Err(KernelError::InvalidParameter(
                        "The sweep's angle must be above zero".into(),
                    ));
                }
                if angle.abs() > TAU + tol::ANGULAR {
                    return Err(KernelError::InvalidParameter(
                        "The sweep turns more than a whole turn".into(),
                    ));
                }
                // Counter-clockwise about the axis: a negative angle turns about the reversed axis.
                let z = axis.dir.into_inner() * angle.signum();
                let x = if z.x.abs() < 0.9 { V3::x() } else { V3::y() };
                let x = (x - z * x.dot(&z)).normalize();
                let turn = super::sweep::Turn {
                    plane: Plane::top(),
                    f: Frame {
                        o: axis.origin,
                        x,
                        y: z.cross(&x),
                        z,
                    },
                    up: true,
                };
                let whole = angle.abs() >= TAU - tol::ANGULAR_FINE;
                super::sweep::sweep_body(
                    &b,
                    &super::sweep::BodyMove::Turn(&turn),
                    if whole { TAU } else { angle.abs() },
                    whole,
                    body.0,
                )?
            }
        };
        let mut built = built;
        if let Some(why) = built.body.check().into_iter().next() {
            // P-curves a hair off their edges (a turned spline's, fitted to its points), fitted
            // again from the edges: the faces and edges stay as they are. An edge still off its
            // face (a fitted curve swept) has its tolerance grown to cover the gap, up to
            // LOOP_GAP, as OCCT grows its edges'.
            let mut b = built.body.clone();
            super::heal::refit_pcurves(&mut b, super::tol::point());
            if !b.check().is_empty() {
                b.fit_tolerances_searched(super::tol::LOOP_GAP);
            }
            if !b.check().is_empty() {
                // With CADRS_DUMP_DIR set, the body the sweep made is left there.
                dump("sweep-invalid", &built.body, &why);
                return Err(KernelError::InvalidResult {
                    op: "sweep",
                    why: format!("the sweep made an invalid body: {why}"),
                });
            }
            built.body = b;
        }
        Ok(self.insert_result(built))
    }

    fn cylindrical_box(&mut self, spec: &CylindricalBox) -> Result<OpResult> {
        let CylindricalBox {
            frame,
            length,
            inner_radius: ri,
            outer_radius: ro,
            start_angle: a0,
            end_angle: a1,
        } = *spec;
        let bad = |why: &str| Err(KernelError::InvalidParameter(why.into()));
        if !(length > 0.0) || !length.is_finite() {
            return bad("A cylindrical box needs a length above zero");
        }
        if !(ri > 0.0) || !ro.is_finite() {
            return bad("A cylindrical box needs an inside radius above zero");
        }
        if !(ro > ri) {
            return bad("A cylindrical box's outside radius must be above its inside radius");
        }
        if !(0.0..TAU).contains(&a0) {
            return bad("A cylindrical box's start angle must lie in [0°, 360°)");
        }
        if !(a1 > a0) || a1 - a0 > TAU + tol::ANGULAR_FINE {
            return bad(
                "A cylindrical box's end angle must come after its start angle, at most a turn past it",
            );
        }
        let z = frame.normal.into_inner();
        let (x, y) = (frame.x_dir.into_inner(), frame.y_dir().into_inner());
        let radial = x * a0.cos() + y * a0.sin();
        // The section through the axis at the start angle: x radial, y along the axis.
        let plane = Plane {
            origin: frame.origin,
            x_dir: nalgebra::Unit::new_normalize(radial),
            normal: nalgebra::Unit::new_normalize(radial.cross(&z)),
        };
        let p = |u: f64, v: f64| nalgebra::Point2::new(u, v);
        let pts = [p(ri, 0.0), p(ro, 0.0), p(ro, length), p(ri, length)];
        let curves = (0..4)
            .map(|i| Curve2::Line {
                a: pts[i],
                b: pts[(i + 1) % 4],
                source: None,
            })
            .collect();
        let profile = Profile::new(
            plane,
            vec![Region {
                outer: Loop { curves },
                holes: vec![],
                source: Some(0),
            }],
        );
        self.revolve(
            &profile,
            Axis {
                origin: frame.origin,
                dir: frame.normal,
            },
            (a1 - a0).min(TAU),
        )
    }

    fn face_kinds(&self, body: BodyId) -> Result<Vec<SurfaceKind>> {
        Ok(self
            .get(body)?
            .faces
            .iter()
            .map(|f| f.surf.kind())
            .collect())
    }

    fn faces(&self, body: BodyId) -> Result<Vec<FaceInfo>> {
        let e = self.entry(body)?;
        use rayon::prelude::*;
        Ok(e.faces
            .get_or_init(|| {
                let n = e.body.faces.len();
                // (Faces that are copies of one another integrated once a set: their areas
                // then come from the face cache.)
                if let Some(c) = e.copies.get() {
                    super::mass::prime_copies(&e.body, c);
                }
                if super::par::threads_for(n, false) {
                    (0..n)
                        .into_par_iter()
                        .map(|i| face_info(&e.body, i))
                        .collect()
                } else {
                    (0..n).map(|i| face_info(&e.body, i)).collect()
                }
            })
            .clone())
    }

    fn edges(&self, body: BodyId) -> Result<Vec<EdgeInfo>> {
        let e = self.entry(body)?;
        Ok(e.edges.get_or_init(|| edge_infos(&e.body)).clone())
    }

    fn edges_shown(&self, body: BodyId, short: f64) -> Result<Vec<EdgeInfo>> {
        let e = self.entry(body)?;
        if let Some(full) = e.edges.get() {
            return Ok(full.clone());
        }
        let (at, infos) = e
            .edges_shown
            .get_or_init(|| (short, edge_infos_with(&e.body, Detail::Shown(short))));
        if *at == short {
            return Ok(infos.clone());
        }
        Ok(edge_infos_with(&e.body, Detail::Shown(short)))
    }

    fn face_count(&self, body: BodyId) -> Result<usize> {
        Ok(self.get(body)?.faces.len())
    }

    fn edges_light(&self, body: BodyId) -> Result<Vec<EdgeInfo>> {
        let e = self.entry(body)?;
        if let Some(full) = e.edges.get() {
            return Ok(full.clone());
        }
        Ok(e.edges_light
            .get_or_init(|| edge_infos_with(&e.body, Detail::Light))
            .clone())
    }

    fn joins(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        // [`Kernel::joins`], its union kept when it is one solid (the union the rebuild asks for
        // next).
        if let Some((_, _, r)) = self.joined.iter().find(|(x, y, _)| *x == a && *y == b) {
            return Ok(self.solid_count(r.bodies[0]).unwrap_or(2) == 1);
        }
        // Sure to be several solids by their boxes (a window head of six blocks and the frame
        // under one of them), or apart: no boolean.
        if super::boolean::union_falls_apart(self.get(a)?, self.get(b)?)
            || self.surely_apart(a, b)?
        {
            return Ok(false);
        }
        let r = self.boolean(BoolOp::Union, a, &[b])?;
        let joined = self.solid_count(r.bodies[0]).unwrap_or(2) == 1;
        if joined {
            self.joined.push((a, b, r));
            if self.joined.len() > JOINED_KEPT {
                let (_, _, old) = self.joined.remove(0);
                for x in old.bodies {
                    self.release(x);
                }
            }
        } else {
            for x in r.bodies {
                self.release(x);
            }
        }
        Ok(joined)
    }

    fn overlaps(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        // The union [`Kernel::joins`] kept of them: what they share is what it lacks of their
        // volumes (no second boolean over the same faces).
        if let Some((_, _, r)) = self.joined.iter().find(|(x, y, _)| *x == a && *y == b) {
            let v = |id: BodyId| self.get(id).map_or(0.0, |x| super::mass::body_volume(x));
            let (va, vb, vu) = (v(a), v(b), v(r.bodies[0]));
            return Ok(va + vb - vu > tol::REL_POINT * (va + vb));
        }
        if self.surely_apart(a, b)? {
            return Ok(false);
        }
        // [`Kernel::overlaps`] with the intersection's volume alone (the same number as its
        // mass properties', for a fraction of the work).
        let r = self.boolean(BoolOp::Intersect, a, &[b])?;
        let v = self
            .get(r.bodies[0])
            .map(|x| super::mass::body_volume(x))
            .unwrap_or(0.0);
        self.release(r.bodies[0]);
        Ok(v > tol::ROUNDOFF)
    }

    fn tangent_chain(&self, body: BodyId, edge: EdgeId) -> Result<Vec<EdgeId>> {
        // (The edges without their lengths: the chain doesn't read them, and rebuilds ask for
        // chains over bodies of many B-spline edges again and again.)
        // Along tangent edges of the same kind: a blend filling a concave corner can't run on
        // round a convex one (an edge continuing in line where one face turns the other way:
        // Onshape's tangent propagation stops there too).
        // Not onto a smooth edge (nothing to blend there), asked of the edges reached only.
        let b = self.get(body)?;
        let convex = |e: EdgeId| super::blend::edge_convex(b, e.0 as usize);
        crate::tangent_chain_of(
            &edge_infos_with(b, Detail::Light),
            &self.vertices(body)?,
            edge,
            &|a, c| {
                (match (convex(a), convex(c)) {
                    (Some(x), Some(y)) => x == y,
                    _ => true,
                }) && !super::blend::smooth_within(b, c.0 as usize, TANGENT_ANGLE)
            },
        )
    }

    fn vertices(&self, body: BodyId) -> Result<Vec<VertexInfo>> {
        let b = self.get(body)?;
        let mut edges: Vec<Vec<EdgeId>> = vec![Vec::new(); b.verts.len()];
        for (ei, e) in b.edges.iter().enumerate() {
            for v in e.v {
                if !edges[v].contains(&EdgeId(ei as u64)) {
                    edges[v].push(EdgeId(ei as u64));
                }
            }
        }
        Ok(b.verts
            .iter()
            .enumerate()
            .filter(|(i, _)| !edges[*i].is_empty() || b.points.contains(i))
            .enumerate()
            .map(|(k, (i, v))| VertexInfo {
                id: VertexId(k as u64),
                point: v.p,
                edges: edges[i].clone(),
            })
            .collect())
    }

    fn mass_properties(&self, body: BodyId) -> Result<MassProperties> {
        let t = web_time::Instant::now();
        let e = self.entry(body)?;
        if let Some(m) = e.mass.get() {
            return Ok(*m);
        }
        let m = super::mass::body_mass_with(&e.body, e.copies.get().map(|c| c.as_slice()));
        let _ = e.mass.set(m);
        slow("mass_properties", t, self.get(body)?);
        if t.elapsed().as_millis() > 500 {
            dump("slow-mass", self.get(body)?, &format!("{:?}", t.elapsed()));
        }
        Ok(m)
    }

    fn tessellate(&self, body: BodyId, quality: Tessellation) -> Result<TriMesh> {
        self.tessellate_quality(body, quality, TessQuality::Accurate)
    }

    fn tessellate_quality(
        &self,
        body: BodyId,
        quality: Tessellation,
        mode: TessQuality,
    ) -> Result<TriMesh> {
        let t = web_time::Instant::now();
        let p = tess::TessParams::try_from(quality)?.with_quality(mode);
        let m = self.tessellate_with(body, &p)?;
        slow("tessellate", t, self.get(body)?);
        Ok(m)
    }

    fn solid_count(&self, body: BodyId) -> Result<usize> {
        Ok(self.get(body)?.solids.len())
    }

    fn size_hint(&self, body: BodyId) -> Result<f64> {
        // (The box of its vertices, edges and faces' control points: no search for extremes.)
        Ok(self.get(body)?.bbox().diagonal())
    }

    fn bounding_box(&self, body: BodyId) -> Result<Aabb> {
        let e = self.entry(body)?;
        Ok(*e.aabb.get_or_init(|| {
            let o = P3::origin();
            // (A big body's three side by side: a rebuild asks each new body's box on its own
            // thread. A box's few edges aren't worth the threads.)
            let along = |d: V3| extent_along(&e.body, &o, &d);
            let ((x0, x1), ((y0, y1), (z0, z1))) = if e.body.edges.len() > 64 {
                super::par::start();
                rayon::join(
                    || along(V3::x()),
                    || rayon::join(|| along(V3::y()), || along(V3::z())),
                )
            } else {
                (along(V3::x()), (along(V3::y()), along(V3::z())))
            };
            Aabb {
                min: P3::new(x0, y0, z0),
                max: P3::new(x1, y1, z1),
            }
        }))
    }

    fn project(
        &self,
        bodies: &[BodyId],
        frame: &ViewFrame,
        opts: &ProjectOptions,
    ) -> Result<Projection> {
        let entries = bodies
            .iter()
            .map(|&id| self.entry(id))
            .collect::<Result<Vec<_>>>()?;
        let inputs: Vec<super::hlr::HlrBody> = entries
            .iter()
            .map(|e| super::hlr::HlrBody {
                body: &e.body,
                polys: e.polys(),
            })
            .collect();
        Ok(super::hlr::project(&inputs, frame, opts))
    }

    fn silhouettes(
        &self,
        body: BodyId,
        view: &SilhouetteView,
        tolerance: f64,
    ) -> Result<Vec<SilhouetteCurve>> {
        super::silhouette::kernel_silhouettes(self, body, view, tolerance)
    }

    fn min_distance(&self, a: &DistanceTarget, b: &DistanceTarget) -> Result<DistanceResult> {
        super::distance::kernel_distance(self, a, b, super::distance::Sense::Min)
    }

    fn max_distance(&self, a: &DistanceTarget, b: &DistanceTarget) -> Result<DistanceResult> {
        super::distance::kernel_distance(self, a, b, super::distance::Sense::Max)
    }

    fn curvatures_at(&self, body: BodyId, points: &[Point3<f64>]) -> Result<Vec<Option<[f64; 2]>>> {
        super::measure::curvatures_at(self, body, points)
    }

    fn plane_section(
        &self,
        body: BodyId,
        origin: Point3<f64>,
        normal: nalgebra::Vector3<f64>,
        tolerance: f64,
    ) -> Result<Vec<Vec<Point3<f64>>>> {
        super::measure::plane_section(self, body, origin, normal, tolerance)
    }

    fn section_area(
        &self,
        body: BodyId,
        origin: Point3<f64>,
        normal: nalgebra::Vector3<f64>,
    ) -> Result<f64> {
        super::measure::section_area(self, body, origin, normal)
    }

    fn betti_numbers(&self, body: BodyId) -> Result<[u64; 3]> {
        super::measure::betti_numbers(self, body)
    }

    fn interference(&mut self, a: BodyId, b: BodyId) -> Result<Interference> {
        super::measure::interference(self, a, b)
    }

    fn face_from_surface(
        &mut self,
        body: BodyId,
        face: FaceId,
        domain: Option<UvRect>,
    ) -> Result<OpResult> {
        super::face_build::kernel_face_from_surface(self, body, face, domain)
    }

    fn bspline_face(
        &mut self,
        surface: &BsplineSurface,
        domain: Option<UvRect>,
    ) -> Result<OpResult> {
        let n = super::nurbs::NurbsSurface::from_bspline(surface)?;
        let s = super::surface::Surface::Nurbs(std::sync::Arc::new(n));
        let id = self
            .face_from_bare_surface(&s, domain.map(|d| ((d[0][0], d[0][1]), (d[1][0], d[1][1]))))?;
        Ok(OpResult {
            bodies: vec![id],
            history: History::default(),
        })
    }

    fn ray_hits(
        &self,
        body: BodyId,
        origin: Point3<f64>,
        dir: nalgebra::Vector3<f64>,
    ) -> Result<Vec<RayHit>> {
        let e = self.entry(body)?;
        let d = dir.normalize();
        let all: Vec<usize> = (0..e.body.faces.len()).collect();
        let grids = e
            .ray_grids
            .get_or_init(|| (0..e.body.faces.len()).map(|_| OnceLock::new()).collect());
        let mut hits = classify::ray_hits_kept(
            &e.body,
            e.polys(),
            &all,
            Some(e.face_boxes()),
            Some(grids),
            &origin,
            &d,
        );
        // A line through an edge (or a vertex) meets each face there: one crossing, not one per
        // face (springrep's tests/rays.rs, a ray through an edge crosses there once).
        hits.dedup_by(|a, b| (a.0 - b.0).abs() <= super::tol::point());
        Ok(hits
            .into_iter()
            .map(|(t, f, p)| RayHit {
                face: FaceId(f as u64),
                t,
                point: p,
            })
            .collect())
    }

    // Exchange (native/exchange.rs).
    fn export_step(&self, bodies: &[BodyId]) -> Result<Vec<u8>> {
        super::exchange::export_step(self, bodies)
    }

    fn export_brep(&self, bodies: &[BodyId]) -> Result<Vec<u8>> {
        super::exchange::export_brep(self, bodies)
    }

    fn import_step(&mut self, bytes: &[u8]) -> Result<Vec<BodyId>> {
        super::exchange::import_step(self, bytes)
    }

    fn import_model(
        &mut self,
        format: exchange::ExchangeFormat,
        bytes: &[u8],
    ) -> Result<exchange::ImportedModel> {
        super::exchange::import_model(self, format, bytes)
    }

    fn export_model(
        &self,
        format: exchange::ExchangeFormat,
        name: &str,
        parts: &[(BodyId, String)],
        instances: &[exchange::ExportInstance],
    ) -> Result<Vec<u8>> {
        super::exchange::export_model(self, format, name, parts, instances)
    }

    fn face_colors(&self, body: BodyId) -> Vec<Option<[u8; 4]>> {
        self.colors(body).to_vec()
    }

    fn set_face_colors(&mut self, body: BodyId, colors: Vec<Option<[u8; 4]>>) {
        self.set_colors(body, colors);
    }

    fn mesh_solid(&mut self, triangles: &[[Point3<f64>; 3]], tol: f64) -> Result<BodyId> {
        super::exchange::mesh_solid(self, triangles, tol)
    }

    fn mesh_sheet(&mut self, triangles: &[[Point3<f64>; 3]], tol: f64) -> Result<BodyId> {
        super::exchange::mesh_sheet(self, triangles, tol)
    }

    fn write_body(&self, body: BodyId) -> Result<Vec<u8>> {
        super::exchange::write_body(self, body)
    }

    fn read_body(&mut self, bytes: &[u8]) -> Result<BodyId> {
        super::exchange::read_body(self, bytes)
    }
}

impl NativeKernel {
    /// A cylinder from `base` along `axis` by `height`, its seam along `seam` (square to the
    /// axis) from it.
    pub(crate) fn cylinder_seamed(
        &mut self,
        base: P3,
        axis: nalgebra::Unit<V3>,
        radius: f64,
        height: f64,
        seam: V3,
    ) -> Result<OpResult> {
        let plane = Plane {
            origin: base,
            x_dir: nalgebra::Unit::new_normalize(seam),
            normal: axis,
        };
        let r = Region {
            outer: Loop {
                curves: vec![Curve2::Circle {
                    center: nalgebra::Point2::origin(),
                    radius,
                    source: None,
                }],
            },
            holes: vec![],
            source: None,
        };
        self.extrude_with(
            &Profile::new(plane, vec![r]),
            &ExtrudeSpec::blind(axis, height),
        )
    }

    /// With `CADRS_DUMP_DIR` set, reports an operation's outcome (solids, or the error).
    fn dumped(&self, r: &Result<OpResult>) -> Result<OpResult> {
        if crate::native::env_flag!("CADRS_DUMP_DIR") {
            match r {
                Ok(o) => {
                    let solids: Vec<usize> = o
                        .bodies
                        .iter()
                        .filter_map(|b| self.get(*b).ok().map(|x| x.solids.len()))
                        .collect();
                    eprintln!(
                        "dump: -> solids {solids:?} in {:?}",
                        DUMP_T.with(|t| t.get().elapsed())
                    );
                }
                Err(e) => eprintln!(
                    "dump: -> error {e} in {:?}",
                    DUMP_T.with(|t| t.get().elapsed())
                ),
            }
        }
        r.clone()
    }
}

/// With `CADRS_PROFILE` set, reports calls slower than 50 ms.
fn slow(what: &str, t: web_time::Instant, b: &Body) {
    let e = t.elapsed();
    if e.as_millis() >= 50 && crate::native::env_flag!("CADRS_PROFILE") {
        let nurbs = b
            .faces
            .iter()
            .filter(|f| matches!(f.surf, super::surface::Surface::Nurbs(_)))
            .count();
        eprintln!(
            "[profile] {what}: {e:?} ({} faces, {nurbs} NURBS)",
            b.faces.len()
        );
    }
}

thread_local! {
    static DUMP_T: std::cell::Cell<web_time::Instant> = std::cell::Cell::new(web_time::Instant::now());
}

/// With `CADRS_DUMP_DIR` set, writes an operation's input body (exact bytes) and its parameters
/// there, numbered, to reproduce failures in kernel tests.
pub(crate) fn dump(op: &str, b: &Body, params: &str) {
    let Ok(dir) = std::env::var("CADRS_DUMP_DIR") else {
        return;
    };
    DUMP_T.with(|t| t.set(web_time::Instant::now()));
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::path::Path::new(&dir).join(format!("{op}-{n:03}"));
    let _ = std::fs::write(base.with_extension("body"), super::bodyio::write(b));
    let _ = std::fs::write(base.with_extension("txt"), params);
    eprintln!("dump: {op}-{n:03} {params}");
}

/// A fillet's spec with its size scaled ([`NativeKernel::at_working_size`]).
fn fillet_scaled(spec: &FilletSpec, by: f64) -> FilletSpec {
    FilletSpec {
        size: match spec.size {
            FilletSize::Radius(r) => FilletSize::Radius(r * by),
            FilletSize::Width(w) => FilletSize::Width(w * by),
            FilletSize::Setback(d) => FilletSize::Setback(d * by),
        },
        profile: match spec.profile {
            FilletProfile::Thumbweight { center } => FilletProfile::Thumbweight {
                center: center.map(|c| c * by),
            },
            // (The second distance is a length too.)
            FilletProfile::Asymmetric { second, flip } => FilletProfile::Asymmetric {
                second: second * by,
                flip,
            },
            p => p,
        },
        ..*spec
    }
}

/// A chamfer's spec with its distances scaled ([`NativeKernel::at_working_size`]).
fn chamfer_scaled(spec: ChamferSpec, by: f64) -> ChamferSpec {
    match spec {
        ChamferSpec::EqualDistance(d) => ChamferSpec::EqualDistance(d * by),
        ChamferSpec::TwoDistances(a, c) => ChamferSpec::TwoDistances(a * by, c * by),
        ChamferSpec::DistanceAngle { distance, angle } => ChamferSpec::DistanceAngle {
            distance: distance * by,
            angle,
        },
    }
}

/// Each distance's edges chamfered on their own, the largest first, and the solids intersected:
/// [`Kernel::chamfer_distances`] where the chamfers' strips can't be trimmed at a corner.
fn chamfer_groups(
    k: &mut NativeKernel,
    body: BodyId,
    each: &[(usize, f64)],
    overflow: bool,
) -> Result<OpResult> {
    let mut sizes: Vec<f64> = each.iter().map(|e| e.1).collect();
    sizes.sort_by(|a, b| b.total_cmp(a));
    sizes.dedup();
    let mut made: Vec<BodyId> = Vec::new();
    let mut out: Result<OpResult> =
        Err(KernelError::InvalidParameter("no edges to chamfer".into()));
    for d in sizes {
        let these: Vec<EdgeId> = each
            .iter()
            .filter(|e| e.1 == d)
            .map(|e| EdgeId(e.0 as u64))
            .collect();
        let opts = ChamferOpts {
            overflow,
            ..ChamferOpts::new(ChamferSpec::EqualDistance(d))
        };
        let cut = k.chamfer_with(body, &these, &opts).and_then(|r| r.first());
        let cut = match cut {
            Ok(c) => c,
            Err(e) => {
                out = Err(e);
                break;
            }
        };
        made.push(cut);
        out = match made.len() {
            1 => Ok(OpResult {
                bodies: vec![cut],
                history: History::default(),
            }),
            _ => {
                let prev = made[made.len() - 2];
                let r = k.boolean(BoolOp::Intersect, prev, &[cut]);
                if let Ok(r) = &r
                    && let [b] = r.bodies[..]
                {
                    made.push(b);
                }
                r
            }
        };
        if out.is_err() {
            break;
        }
    }
    let keep: Vec<BodyId> = out.as_ref().map(|r| r.bodies.clone()).unwrap_or_default();
    for b in made {
        if !keep.contains(&b) {
            k.release(b);
        }
    }
    out
}

/// A blend of no edges refused: nothing to blend (springrep's tests/blends.rs).
fn no_edges(edges: &[EdgeId], what: &str) -> Result<()> {
    if edges.is_empty() {
        return Err(KernelError::InvalidParameter(format!("no edges to {what}")));
    }
    Ok(())
}

/// Whether a face of `b` on a periodic surface has a vertex of its own lying inside its seam
/// (a line it uses twice), not at the seam's ends.
fn seam_through_a_vertex(b: &Body) -> bool {
    let near = 1e-7 * b.bbox().diagonal().max(1.0) + super::tol::LOOP_GAP;
    b.faces.iter().any(|f| {
        if !f.surf.u_periodic() {
            return false;
        }
        f.loops.iter().any(|l| {
            l.iter().enumerate().any(|(k, c)| {
                let e = &b.edges[c.edge];
                if e.degenerate
                    || !matches!(e.curve, super::curve::Curve::Line { .. })
                    || !l[k + 1..].iter().any(|d| d.edge == c.edge)
                {
                    return false;
                }
                let (p0, p1) = (e.point(e.t[0]), e.point(e.t[1]));
                let d = p1 - p0;
                let l2 = d.norm_squared();
                if l2 <= 0.0 {
                    return false;
                }
                f.loops.iter().flatten().any(|q| {
                    let v = b.co_start(q);
                    if e.v.contains(&v) {
                        return false;
                    }
                    let p = b.verts[v].p;
                    let s = (p - p0).dot(&d) / l2;
                    let len = l2.sqrt();
                    s * len > near && (1.0 - s) * len > near && (p0 + d * s - p).norm() < near
                })
            })
        })
    })
}

/// A body that is a plain cylinder (its side between two whole circles, two flat caps): its
/// base, axis, radius and height, and its seam's direction turned half a turn.
fn cylinder_turned(b: &Body) -> Option<(P3, nalgebra::Unit<V3>, f64, f64, V3)> {
    use super::curve::Curve;
    if b.faces.len() != 3 || b.solids.len() != 1 {
        return None;
    }
    let si = b
        .faces
        .iter()
        .position(|f| matches!(f.surf, Surface::Cylinder { .. }))?;
    let side = &b.faces[si];
    let Surface::Cylinder { f: fr, r } = &side.surf else {
        return None;
    };
    if !(0..3)
        .filter(|&i| i != si)
        .all(|i| matches!(b.faces[i].surf, Surface::Plane { .. }))
    {
        return None;
    }
    let [l] = side.loops.as_slice() else {
        return None;
    };
    let mut centres: Vec<P3> = Vec::new();
    let mut seam: Option<P3> = None;
    for c in l {
        let e = &b.edges[c.edge];
        match &e.curve {
            Curve::Circle { c: o, .. } if e.v[0] == e.v[1] => centres.push(*o),
            Curve::Line { .. } => seam = Some(b.verts[e.v[0]].p),
            _ => return None,
        }
    }
    let ([c0, c1], Some(on_seam)) = (centres.as_slice(), seam) else {
        return None;
    };
    let mut axis = c1 - c0;
    let height = axis.norm();
    if height <= 0.0 {
        return None;
    }
    let base = if axis.dot(&fr.z) >= 0.0 {
        *c0
    } else {
        axis = -axis;
        *c1
    };
    let axis = nalgebra::Unit::new_normalize(axis);
    // The seam's direction from the axis, turned half a turn.
    let out = on_seam - base;
    let out = out - axis.into_inner() * out.dot(&axis);
    if out.norm() <= 0.0 {
        return None;
    }
    Some((base, axis, *r, height, -out.normalize()))
}

/// Bodies larger than this (mm, their box's diagonal) are offset scaled down by a power of two,
/// as small ones are scaled up ([`NativeKernel::at_size`]): an offset's tolerances are
/// absolute, and were tuned on parts of tens of millimetres.
const WORKED_LARGE: f64 = 128.0;

/// Bodies larger than this (mm, their boxes' longest diagonal) are swept about an axis scaled
/// down by a power of two to under it ([`canonical_motion`]).
const SWEPT_SIZE: f64 = 4.0;

/// For a motion about one axis (a turn, a screw, a climb, or rigid turns about one axis with
/// slides along it) whose axis isn't the z axis or whose bodies are larger than [`SWEPT_SIZE`]:
/// the map that puts the axis on the z axis through the origin and scales them by a power of
/// two to under that size, and the motion so mapped. The exact sweeps of a solid are made in
/// the axis's own frame, and are sound where their parts are of an ordinary size: a ball
/// screwed about a tilted axis, or ten times larger, was meshed where the same ball screwed
/// about z at its own size was exact. None where the motion is already so placed.
fn canonical_motion(
    k: &NativeKernel,
    bodies: &[BodyId],
    motion: &SolidMotion,
) -> Option<(Affine, SolidMotion)> {
    let axis = match motion {
        SolidMotion::Turn { axis }
        | SolidMotion::Screw { axis, .. }
        | SolidMotion::Climb { axis, .. } => *axis,
        SolidMotion::Rigid { steps } => {
            let mut axis: Option<Axis> = None;
            for st in steps {
                if let RigidStep::Turn { axis: a, .. } = st {
                    match axis {
                        None => axis = Some(*a),
                        Some(b) => {
                            let w = b.dir.into_inner();
                            let off = a.origin - b.origin;
                            if a.dir.cross(&b.dir).norm() > 1e-12
                                || (off - w * off.dot(&w)).norm() > 1e-9 * (1.0 + off.norm())
                            {
                                return None;
                            }
                        }
                    }
                }
            }
            let axis = axis?;
            let w = axis.dir.into_inner();
            for st in steps {
                if let RigidStep::Slide { from, to } = st {
                    for v in [from, to] {
                        if (v - w * v.dot(&w)).norm() > 1e-12 * (1.0 + v.norm()) {
                            return None;
                        }
                    }
                }
            }
            axis
        }
        _ => return None,
    };
    let size = bodies
        .iter()
        .filter_map(|b| k.get(*b).ok().map(|x| x.bbox().diagonal()))
        .fold(0.0, f64::max);
    if !(size > 0.0 && size.is_finite()) {
        return None;
    }
    let z = V3::z();
    let w = axis.dir.into_inner();
    // (Turned onto z only where the motion slides along its axis, a screw's: a turn alone is
    // exact about any axis, and a body turned onto z by a rotation in floating point loses the
    // symmetries a turn about its own diagonal finds, a cube's spun to three faces.)
    let slides = match motion {
        SolidMotion::Screw { .. } | SolidMotion::Climb { .. } => true,
        SolidMotion::Rigid { steps } => steps.iter().any(|s| matches!(s, RigidStep::Slide { .. })),
        _ => false,
    };
    let tilted = slides && (w - z).norm() > 1e-15;
    let scale = if size > SWEPT_SIZE {
        (SWEPT_SIZE / size).log2().floor().exp2()
    } else {
        1.0
    };
    if !tilted && scale == 1.0 {
        return None;
    }
    let q = if tilted {
        nalgebra::Rotation3::rotation_between(&w, &z)
            .unwrap_or_else(|| {
                nalgebra::Rotation3::from_axis_angle(&V3::x_axis(), std::f64::consts::PI)
            })
            .into_inner()
    } else {
        nalgebra::Matrix3::identity()
    };
    // The axis's point nearest the origin goes to the origin (turned onto z), or the origin
    // stays where it is (scaled about it).
    let o = if tilted {
        axis.origin - w * axis.origin.coords.dot(&w)
    } else {
        P3::origin()
    };
    let lin = q * scale;
    let m = Affine {
        m: lin,
        t: -(lin * o.coords),
    };
    let along = |d: f64| d * scale;
    // (A distance along the axis from its origin, mapped: the origin moved to `o`.)
    let shift = if tilted {
        axis.origin.coords.dot(&w)
    } else {
        0.0
    };
    let zaxis = if tilted {
        Axis {
            origin: P3::origin(),
            dir: nalgebra::Unit::new_unchecked(z),
        }
    } else {
        Axis {
            origin: P3::from(axis.origin.coords * scale),
            dir: axis.dir,
        }
    };
    let moved = match motion {
        SolidMotion::Turn { .. } => SolidMotion::Turn { axis: zaxis },
        SolidMotion::Screw { rise, from, to, .. } => SolidMotion::Screw {
            axis: zaxis,
            rise: along(*rise),
            from: along(from + shift),
            to: along(to + shift),
        },
        SolidMotion::Climb { from, to, rise, .. } => {
            // (Its rise is how far the body has moved along the axis: no position on it.)
            let rise: Vec<f64> = rise.iter().map(|c| along(*c)).collect();
            SolidMotion::Climb {
                axis: zaxis,
                from: *from,
                to: *to,
                rise,
            }
        }
        SolidMotion::Rigid { steps } => SolidMotion::Rigid {
            steps: steps
                .iter()
                .map(|st| match st {
                    RigidStep::Turn { from, to, .. } => RigidStep::Turn {
                        axis: zaxis,
                        from: *from,
                        to: *to,
                    },
                    RigidStep::Slide { from, to } => RigidStep::Slide {
                        from: lin * from,
                        to: lin * to,
                    },
                })
                .collect(),
        },
        _ => return None,
    };
    Some((m, moved))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The session's shortcuts answer as the trait's defaults do: the tangent chain over edges
    /// without their lengths, faces and edges kept per body, and `overlaps` by the volume alone.
    #[test]
    fn shortcuts_match_the_defaults() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/smlib/fillets");
        for name in [
            "blade_hub.smb",
            "cyl_on_box_w_fillet.smb",
            "Heart.smb",
            "box_diff_3cyls.smb",
        ] {
            let b = super::super::smb::read_brep(&std::fs::read(format!("{dir}/{name}")).unwrap())
                .unwrap()
                .body;
            let mut k = NativeKernel::new();
            let id = k.insert(b);
            let (edges, vertices) = (k.edges(id).unwrap(), k.vertices(id).unwrap());
            assert_eq!(edges, edge_infos(k.get(id).unwrap()), "{name}");
            assert_eq!(k.edges(id).unwrap(), edges, "{name}");
            assert_eq!(k.faces(id).unwrap(), k.faces(id).unwrap(), "{name}");
            for e in edges.iter().step_by(3) {
                assert_eq!(
                    k.tangent_chain(id, e.id).unwrap(),
                    crate::tangent_chain_of(&edges, &vertices, e.id, &|a, c| {
                        let b = k.get(id).unwrap();
                        let convex = |e: EdgeId| super::super::blend::edge_convex(b, e.0 as usize);
                        match (convex(a), convex(c)) {
                            (Some(x), Some(y)) => x == y,
                            _ => true,
                        }
                    })
                    .unwrap(),
                    "{name} {:?}",
                    e.id
                );
            }
        }
        // `overlaps` of a model with a box across it.
        for name in ["cyl_on_box_w_fillet.smb", "box_diff_3cyls.smb"] {
            let b = super::super::smb::read_brep(&std::fs::read(format!("{dir}/{name}")).unwrap())
                .unwrap()
                .body;
            let bb = b.bbox();
            let mut k = NativeKernel::new();
            let id = k.insert(b);
            let c = bb.center();
            let d = bb.max - bb.min;
            let Ok(super::super::draw::Shape::Body(tool)) = k.box_at(
                c.x - 0.1 * d.x,
                c.y - 0.2 * d.y,
                c.z - 0.3 * d.z,
                d.x,
                d.y,
                d.z,
            ) else {
                panic!("no box")
            };
            let full = k
                .boolean(BoolOp::Intersect, id, &[tool])
                .and_then(|r| k.mass_properties(r.bodies[0]))
                .map(|m| m.volume > tol::ROUNDOFF);
            assert_eq!(k.overlaps(id, tool).ok(), full.ok(), "{name}");
        }
    }
}
