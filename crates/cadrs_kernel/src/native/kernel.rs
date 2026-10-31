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
        }
    }
    /// Each face's box, grown by the largest of the body's edge and vertex tolerances and
    /// [`tol::POINT`].
    pub fn face_boxes(&self) -> &Vec<Bbox> {
        self.face_boxes.get_or_init(|| {
            let b = &self.body;
            let reach = b
                .edges
                .iter()
                .map(|e| e.tol)
                .chain(b.verts.iter().map(|v| v.tol))
                .fold(tol::POINT, f64::max);
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
    pub fn set_tolerances(&mut self, tol: super::tol::Tolerances) -> Result<()> {
        tol.validate()?;
        self.tol = tol;
        Ok(())
    }

    /// The session's record of a body.
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
    pub fn insert(&mut self, body: Body) -> BodyId {
        let id = self.next;
        self.next += 1;
        self.bodies.insert(id, Arc::new(Entry::new(body)));
        BodyId(id)
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
                    classify::classify_point(&y.body, y.polys(), &p, tol::POINT)
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
    for v in &b.verts {
        add((v.p - o).dot(d));
    }
    for e in &b.edges {
        if e.degenerate {
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
    }
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
    b.edges
        .par_iter()
        .enumerate()
        .map(|(ei, e)| {
            let faces = match ef[ei].as_slice() {
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
                // (Not without the lengths, for tangent chains: those ask of the edges they
                // reach only, [`NativeKernel::tangent_chain`].)
                smooth: !matches!(detail, Detail::Light)
                    && super::blend::smooth_within(b, ei, TANGENT_ANGLE),
            }
        })
        .collect()
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
        super::sweep::path::sweep_with(self, profile, spec)
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
        let r = super::boolean::boolean(self, op, target, tools);
        // With CADRS_DUMP_DIR set, a failed boolean leaves its inputs there.
        if r.is_err() && std::env::var("CADRS_DUMP_DIR").is_ok() {
            dump("boolean-target", self.get(target)?, &format!("{op:?}"));
            for t in tools {
                dump("boolean-tool", self.get(*t)?, &format!("{op:?}"));
            }
            let _ = self.dumped(&r);
        }
        self.colored(r)
    }

    fn fillet(&mut self, body: BodyId, edges: &[EdgeId], radius: f64) -> Result<OpResult> {
        dump(
            "fillet",
            self.get(body)?,
            &format!("edges {edges:?} radius {radius}"),
        );
        let r = super::blend::fillet(self, body, edges, &FilletSpec::radius(radius));
        let r = self.dumped(&r);
        self.colored(r)
    }

    fn fillet_with(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        spec: &FilletSpec,
    ) -> Result<OpResult> {
        dump(
            "fillet",
            self.get(body)?,
            &format!("edges {edges:?} {spec:?}"),
        );
        let r = super::blend::fillet(self, body, edges, spec);
        let r = self.dumped(&r);
        self.colored(r)
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
        let r = super::blend_corner::fillet_smooth(self, body, edges, spec, setback);
        let r = self.dumped(&r);
        self.colored(r)
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
        let r = super::surfacing::delete_faces(self, body, faces);
        self.colored(r)
    }

    fn offset(&mut self, body: BodyId, spec: &OffsetSpec) -> Result<OpResult> {
        let r = super::offset::offset(self, body, spec);
        self.colored(r)
    }

    fn draft(&mut self, body: BodyId, spec: &DraftSpec) -> Result<OpResult> {
        let r = super::local::draft(self, body, spec);
        self.colored(r)
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
        let r = super::offset::round::shell(
            self,
            body,
            &ShellSpec {
                remove: remove.to_vec(),
                thickness,
                outward: false,
                hollow: false,
                sharp: false,
            },
        );
        let r = self.dumped(&r);
        self.colored(r)
    }

    fn shell_with(&mut self, body: BodyId, spec: &ShellSpec) -> Result<OpResult> {
        dump("shell", self.get(body)?, &format!("{spec:?}"));
        let r = super::offset::round::shell(self, body, spec);
        let r = self.dumped(&r);
        self.colored(r)
    }

    fn fillet_partial(
        &mut self,
        body: BodyId,
        edge: EdgeId,
        spec: &FilletSpec,
        from: f64,
        to: f64,
    ) -> Result<OpResult> {
        let r = super::blend::fillet_partial(self, body, edge, spec, from, to);
        self.colored(r)
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
        let r = super::blend::fillet_variable(self, body, laws, allow_overflow);
        let r = self.dumped(&r);
        self.colored(r)
    }

    fn full_round(
        &mut self,
        body: BodyId,
        side1: FaceId,
        center: FaceId,
        side2: FaceId,
    ) -> Result<OpResult> {
        let r = super::blend::full_round(self, body, side1, center, side2);
        self.colored(r)
    }

    fn chamfer(&mut self, body: BodyId, edges: &[EdgeId], spec: ChamferSpec) -> Result<OpResult> {
        dump(
            "chamfer",
            self.get(body)?,
            &format!("edges {edges:?} {spec:?}"),
        );
        let r = super::blend::chamfer(self, body, edges, &ChamferOpts::new(spec));
        let r = self.dumped(&r);
        self.colored(r)
    }

    fn chamfer_with(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        opts: &ChamferOpts,
    ) -> Result<OpResult> {
        dump(
            "chamfer",
            self.get(body)?,
            &format!("edges {edges:?} {opts:?}"),
        );
        let r = super::blend::chamfer(self, body, edges, opts);
        let r = self.dumped(&r);
        self.colored(r)
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
        let b = self.get(body)?.transformed(&Affine {
            m,
            t: motion.translation,
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
        Ok(classify::classify_point(&e.body, e.polys(), &point, tol))
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
        if let Some(why) = built.body.check().into_iter().next() {
            return Err(KernelError::InvalidResult {
                op: "sweep",
                why: format!("the sweep made an invalid body: {why}"),
            });
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

    fn faces(&self, body: BodyId) -> Result<Vec<FaceInfo>> {
        let e = self.entry(body)?;
        use rayon::prelude::*;
        Ok(e.faces
            .get_or_init(|| {
                (0..e.body.faces.len())
                    .into_par_iter()
                    .map(|i| face_info(&e.body, i))
                    .collect()
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
        let m = super::mass::body_mass(self.get(body)?);
        slow("mass_properties", t, self.get(body)?);
        if t.elapsed().as_millis() > 500 {
            dump("slow-mass", self.get(body)?, &format!("{:?}", t.elapsed()));
        }
        Ok(m)
    }

    fn tessellate(&self, body: BodyId, quality: Tessellation) -> Result<TriMesh> {
        let t = web_time::Instant::now();
        let m = self.tessellate_with(body, &tess::TessParams::try_from(quality)?)?;
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

    fn face_from_surface(
        &mut self,
        body: BodyId,
        face: FaceId,
        domain: Option<UvRect>,
    ) -> Result<OpResult> {
        super::face_build::kernel_face_from_surface(self, body, face, domain)
    }

    fn ray_hits(
        &self,
        body: BodyId,
        origin: Point3<f64>,
        dir: nalgebra::Vector3<f64>,
    ) -> Result<Vec<RayHit>> {
        let e = self.entry(body)?;
        let d = dir.normalize();
        Ok(classify::ray_hits(&e.body, e.polys(), &origin, &d)
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

    fn write_body(&self, body: BodyId) -> Result<Vec<u8>> {
        super::exchange::write_body(self, body)
    }

    fn read_body(&mut self, bytes: &[u8]) -> Result<BodyId> {
        super::exchange::read_body(self, bytes)
    }
}

impl NativeKernel {
    /// With `CADRS_DUMP_DIR` set, reports an operation's outcome (solids, or the error).
    fn dumped(&self, r: &Result<OpResult>) -> Result<OpResult> {
        if std::env::var("CADRS_DUMP_DIR").is_ok() {
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
    if e.as_millis() >= 50 && std::env::var("CADRS_PROFILE").is_ok() {
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
