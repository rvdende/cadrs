//! Evolved solids (OCCT's `BRepOffsetAPI_MakeEvolved` in volume mode, DRAW's `evolved -v`): a
//! profile region swept along a planar spine, held square to it, with its x measured outwards
//! from the spine in the spine's plane and its y up from it.
//!
//! Each spine edge sweeps the profile exactly: a line a prism of it, a circular arc a revolution
//! about the arc's axis. At a corner where the profile's side of the spine turns away (a left
//! turn of a counter-clockwise spine for a profile outside it, x ≥ 0; a right turn for one
//! inside, x ≤ 0) the gap is filled by the profile turned about the corner's vertical axis, so
//! offsets of the spine round off as OCCT's do (arc joins); where it turns in, the neighbouring
//! sweeps overlap. The pieces are united. A closed spine is taken counter-clockwise about its
//! plane's normal, which is "up".
//!
//! [`evolved_wire`] takes the profile as DRAW's `evolved` does: a wire in space (open or
//! closed), measured from the spine point nearest it (`BRepFill_PipeShell`'s placement in the
//! volume mode; the plain mode's global frame is the spine plane's own axes, which agree with
//! it on OCCT's cases), its part along the spine dropped. An open profile is
//! closed by flat lids across the spine's offsets at its ends (`BRepFill_AdvancedEvolved::GetLids`),
//! so the solid holds the points whose signed distance inwards from the spine, at their height,
//! lies in the profile's region. The union of sweeps above is that set only where the region
//! reaches the spine at every height (a sweep's slab may cover a point nearer another edge), so
//! the region is cut into horizontal strips and each strip's spans are built from sweeps of
//! spans that do: a span `[a, b]` away from the spine is the sweep of `[a, 0]` less the sweep
//! of `[b, 0]`, a span reaching past every inward offset is the spine's prism less the sweep of
//! the rest, and sweeps inwards are kept inside the spine's prism (outwards, outside it).

use super::curve::Curve;
use super::features::Tagged;
use super::kernel::NativeKernel;
use super::math::*;
use super::tol;
use super::topo::Body;
use crate::*;

/// A spine edge in the direction of travel.
struct SpineEdge {
    c: Curve,
    t0: f64,
    t1: f64,
}

impl SpineEdge {
    fn point(&self, s: f64) -> P3 {
        self.c.point(self.t0 + (self.t1 - self.t0) * s)
    }
    fn dir(&self, s: f64) -> V3 {
        (self.c.tangent(self.t0 + (self.t1 - self.t0) * s) * (self.t1 - self.t0).signum())
            .normalize()
    }
}

/// A planar spine of lines and circular arcs, ready to sweep along.
struct Spine {
    edges: Vec<SpineEdge>,
    /// The plane's normal: the spine runs counter-clockwise about it when closed.
    up: V3,
    closed: bool,
    /// The spine's extent (the largest distance from its first point).
    size: f64,
}

impl Spine {
    fn new(spine: &[(Curve, f64, f64)]) -> Result<Spine> {
        if spine.is_empty() {
            return Err(KernelError::InvalidParameter(
                "an evolved solid needs a spine".into(),
            ));
        }
        let edges: Vec<SpineEdge> = spine
            .iter()
            .map(|(c, t0, t1)| SpineEdge {
                c: c.clone(),
                t0: *t0,
                t1: *t1,
            })
            .collect();
        for e in &edges {
            if !matches!(e.c, Curve::Line { .. } | Curve::Circle { .. }) {
                return Err(KernelError::Unsupported(
                    "evolved solids along spines of curves other than lines and arcs",
                ));
            }
        }
        let n = edges.len();
        let first = edges[0].point(0.0);
        let closed =
            (edges[n - 1].point(1.0) - first).norm() < tol::CONFUSION * (1.0 + first.coords.norm());
        // The spine's plane normal ("up"): counter-clockwise for a closed spine.
        let pts: Vec<P3> = edges
            .iter()
            .flat_map(|e| (0..16).map(move |i| e.point(i as f64 / 16.0)))
            .collect();
        let mut up = V3::zeros();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            up += (a - pts[0]).cross(&(b - pts[0]));
        }
        let size = pts
            .iter()
            .map(|p| (p - pts[0]).norm())
            .fold(0.0, f64::max)
            .max(tol::ROUNDOFF);
        if up.norm() < tol::REL * size * size {
            // An open straight spine: any normal square to it.
            up = super::axes::ax2_x(&edges[0].dir(0.0));
        }
        let up = up.normalize();
        if pts
            .iter()
            .any(|p| (p - pts[0]).dot(&up).abs() > tol::point() * size)
        {
            return Err(KernelError::InvalidParameter(
                "an evolved solid's spine must be planar".into(),
            ));
        }
        Ok(Spine {
            edges,
            up,
            closed,
            size,
        })
    }

    /// The profile placed at a point of the spine with travel direction t: x outwards (t × up).
    fn place(&self, p: P3, t: V3, regions: Vec<Region>) -> Profile {
        let x = t.cross(&self.up).normalize();
        Profile::new(
            Plane {
                origin: p,
                x_dir: nalgebra::Unit::new_normalize(x),
                normal: nalgebra::Unit::new_normalize(x.cross(&self.up)),
            },
            regions,
        )
    }

    /// Where an arc edge's axis lies in the profile's x at the edge's start: `-r` when the arc
    /// bends inwards (its centre on the inner side), `r` when outwards; `None` for a line.
    fn axis_x(&self, e: &SpineEdge) -> Option<f64> {
        let Curve::Circle { c, .. } = &e.c else {
            return None;
        };
        Some((c - e.point(0.0)).dot(&e.dir(0.0).cross(&self.up)))
    }

    /// The spine's point nearest `q`: its distance, the point and the travel direction there.
    fn nearest(&self, q: &P3) -> (f64, P3, V3) {
        let mut best = (f64::INFINITY, P3::origin(), V3::zeros());
        for e in &self.edges {
            let t = e.c.closest(q, e.t0.min(e.t1), e.t0.max(e.t1));
            let p = e.c.point(t);
            let d = (p - q).norm();
            if d < best.0 {
                best = (d, p, e.dir((t - e.t0) / (e.t1 - e.t0)));
            }
        }
        best
    }

    /// The spine as a polygon in its plane (arcs as fine chords), and the map into that plane.
    fn polygon(&self) -> (impl Fn(&P3) -> P2 + use<>, Vec<P2>) {
        const ARC: usize = 64;
        let (o, u) = (self.edges[0].point(0.0), super::axes::ax2_x(&self.up));
        let v = self.up.cross(&u);
        let to2 = move |p: &P3| P2::new((p - o).dot(&u), (p - o).dot(&v));
        let poly = self
            .edges
            .iter()
            .flat_map(|e| {
                let n = if matches!(e.c, Curve::Line { .. }) {
                    1
                } else {
                    ARC
                };
                (0..n).map(move |i| e.point(i as f64 / n as f64))
            })
            .map(|p| to2(&p))
            .collect();
        (to2, poly)
    }

    /// A bound on how far inside the closed spine's region a point can be from the spine (the
    /// largest distance on a grid over the region, plus the grid's spacing): an inward offset
    /// at least this far holds the whole region.
    fn inradius_bound(&self) -> f64 {
        const GRID: usize = 64;
        let (_, poly) = self.polygon();
        let (lo, hi) = poly.iter().fold(
            (
                P2::new(f64::INFINITY, f64::INFINITY),
                P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            |(a, b), p| {
                (
                    P2::new(a.x.min(p.x), a.y.min(p.y)),
                    P2::new(b.x.max(p.x), b.y.max(p.y)),
                )
            },
        );
        let step = (hi.x - lo.x).max(hi.y - lo.y) / GRID as f64;
        let mut best: f64 = 0.0;
        for i in 0..=GRID {
            for j in 0..=GRID {
                let q = P2::new(lo.x + step * i as f64, lo.y + step * j as f64);
                if point_in_polygon(&q, &poly) {
                    let d = (0..poly.len())
                        .map(|k| seg_dist2(&q, &poly[k], &poly[(k + 1) % poly.len()]))
                        .fold(f64::INFINITY, f64::min);
                    best = best.max(d);
                }
            }
        }
        best + step
    }

    /// Whether the sweeps of a profile reaching `depth` from the spine on its side (`outside`)
    /// stay on that side of it: sampled along each edge and round each corner where the side
    /// turns away, every point of the sweep's slab lies off the spine's region (outside) or in
    /// it, more than `eps` from its boundary. Their union then needs no trimming to the side.
    fn keeps_side(&self, outside: bool, depth: f64, eps: f64) -> bool {
        const ALONG: usize = 32;
        const DEEP: usize = 16;
        let (to2, poly) = self.polygon();
        let side = if outside { 1.0 } else { -1.0 };
        // (On the spine counts as on its side: a slab's end along the next edge at a square
        // corner.)
        let ok = |q: P3| point_in_polygon(&to2(&q), &poly) != outside || self.nearest(&q).0 <= eps;
        let n = self.edges.len();
        for (i, e) in self.edges.iter().enumerate() {
            for a in 0..=ALONG {
                let s = a as f64 / ALONG as f64;
                let (p, x) = (e.point(s), e.dir(s).cross(&self.up).normalize() * side);
                if !(1..=DEEP).all(|j| ok(p + x * (depth * j as f64 / DEEP as f64))) {
                    return false;
                }
            }
            if i + 1 == n && !self.closed {
                continue;
            }
            let (ta, tb) = (e.dir(1.0), self.edges[(i + 1) % n].dir(0.0));
            let turn = ta.cross(&tb).dot(&self.up).atan2(ta.dot(&tb));
            if turn.abs() < tol::ANGULAR || (turn > 0.0) != outside {
                continue;
            }
            let (vtx, x0) = (e.point(1.0), ta.cross(&self.up).normalize() * side);
            for a in 0..=ALONG {
                let r = nalgebra::Rotation3::from_axis_angle(
                    &nalgebra::Unit::new_normalize(self.up),
                    turn * a as f64 / ALONG as f64,
                );
                if !(1..=DEEP).all(|j| ok(vtx + r * x0 * (depth * j as f64 / DEEP as f64))) {
                    return false;
                }
            }
        }
        true
    }
}

/// The evolved solid of `profile` (closed regions in the profile's (x, y): outwards, up) along
/// `spine` (lines and circular arcs, chained in order, in one plane). The profile must lie on
/// one side of x = 0. Returns the solid's body.
pub fn evolved(
    k: &mut NativeKernel,
    spine: &[(Curve, f64, f64)],
    profile: &[Region],
) -> Result<BodyId> {
    if spine.is_empty() || profile.is_empty() {
        return Err(KernelError::InvalidParameter(
            "an evolved solid needs a spine and a profile".into(),
        ));
    }
    let sp = Spine::new(spine)?;
    // Which side of the spine the profile lies on.
    let mut xs = (f64::INFINITY, f64::NEG_INFINITY);
    for r in profile {
        for c in r
            .outer
            .curves
            .iter()
            .chain(r.holes.iter().flat_map(|h| h.curves.iter()))
        {
            for i in 0..=8 {
                let p = c.point_at(i as f64 / 8.0);
                xs = (xs.0.min(p.x), xs.1.max(p.x));
            }
        }
    }
    let outside = if xs.0 >= -tol::ROUNDOFF {
        true
    } else if xs.1 <= tol::ROUNDOFF {
        false
    } else {
        return Err(KernelError::Unsupported(
            "evolved solids of profiles on both sides of the spine",
        ));
    };
    sweep_union(
        k,
        &sp,
        &|_| Ok(profile.to_vec()),
        &|o| {
            Ok(if o == outside {
                profile.to_vec()
            } else {
                Vec::new()
            })
        },
        false,
    )?
    .ok_or_else(|| {
        KernelError::InvalidParameter("an evolved solid needs a spine and a profile".into())
    })
}

/// The union of the sweeps along each spine edge, and of the knuckles turned about the corners.
/// `regions(axis_x)` gives the profile for an edge (`axis_x` from [`Spine::axis_x`], so an
/// arc's profile can stop at its axis; `None` for lines), `knuckle(outside)` the part of it
/// turned about a corner where the spine turns away from that side. Pieces meeting at a shared
/// cap are glued there when `glue`, else (and where they overlap) united. `None` when every
/// profile is empty.
fn sweep_union(
    k: &mut NativeKernel,
    sp: &Spine,
    regions: &dyn Fn(Option<f64>) -> Result<Vec<Region>>,
    knuckle: &dyn Fn(bool) -> Result<Vec<Region>>,
    glue: bool,
) -> Result<Option<BodyId>> {
    let n = sp.edges.len();
    // The corners: the turn from each edge into the next (none past an open spine's end).
    let turns: Vec<Option<f64>> = (0..n)
        .map(|i| {
            if i + 1 == n && !sp.closed {
                return None;
            }
            let (ta, tb) = (sp.edges[i].dir(1.0), sp.edges[(i + 1) % n].dir(0.0));
            Some(ta.cross(&tb).dot(&sp.up).atan2(ta.dot(&tb)))
        })
        .collect();
    // The knuckle's profile at each corner (the part of the profile on the side the spine
    // turns away from: outside at a left turn).
    let mut knuckles: Vec<Vec<Region>> = Vec::with_capacity(n);
    for t in &turns {
        knuckles.push(match t {
            Some(t) if t.abs() >= tol::ANGULAR => knuckle(*t > 0.0)?,
            _ => Vec::new(),
        });
    }
    // Where part of the profile turns in, the sweeps on either side overlap: starting after
    // such a corner, every other joint is a shared cap.
    let whole = regions(None)?;
    let first = if sp.closed {
        (0..n)
            .position(|i| {
                turns[i].is_some_and(|t| t.abs() >= tol::ANGULAR)
                    && knuckles[i].len() != whole.len()
            })
            .map_or(0, |i| (i + 1) % n)
    } else {
        0
    };
    let mut pieces: Vec<Tagged> = Vec::new();
    for i in (0..n).map(|j| (first + j) % n) {
        let e = &sp.edges[i];
        let p0 = e.point(0.0);
        let rs = regions(sp.axis_x(e))?;
        if !rs.is_empty() {
            let prof = sp.place(p0, e.dir(0.0), rs);
            let r = match &e.c {
                Curve::Line { .. } => super::features::prism_along(k, &prof, e.point(1.0) - p0)?,
                Curve::Circle { c, x, y, .. } => {
                    // About the arc's axis, the way it runs.
                    let ax = x.cross(y) * (e.t1 - e.t0).signum();
                    let sweep = (e.t1 - e.t0).abs();
                    crate::Kernel::revolve(
                        k,
                        &prof,
                        Axis {
                            origin: *c,
                            dir: nalgebra::Unit::new_normalize(ax),
                        },
                        sweep,
                    )?
                }
                _ => {
                    return Err(KernelError::Unsupported(
                        "evolved solids along spines of curves other than lines and arcs",
                    ));
                }
            };
            pieces.push(tagged(k, r)?);
        }
        // The corner at the edge's end: the knuckle turned about it.
        let Some(turn) = turns[i] else { continue };
        let rs = knuckles[i].clone();
        if rs.is_empty() {
            continue;
        }
        let v = e.point(1.0);
        let prof = sp.place(v, e.dir(1.0), rs);
        let axis = Axis {
            origin: v,
            dir: nalgebra::Unit::new_normalize(sp.up * turn.signum()),
        };
        let r = crate::Kernel::revolve(k, &prof, axis, turn.abs())?;
        pieces.push(tagged(k, r)?);
    }
    // Pieces meeting at a shared cap are glued there; overlapping ones united.
    let mut it = pieces.into_iter();
    let Some(mut acc) = it.next() else {
        return Ok(None);
    };
    let mut prev_end = caps(&acc, true);
    for p in it {
        let (start, end) = (caps(&p, false), caps(&p, true));
        // Glued only where the caps meet one to one (else part of the profile turns in there).
        let meet = start.len() == prev_end.len()
            && start
                .iter()
                .all(|a| prev_end.iter().any(|b| same_cap(a, b)));
        acc = match (glue && meet)
            .then(|| super::sweep::path::glue(&acc, &p))
            .flatten()
            .filter(|g| additive(&g.body, &acc.body, &p.body))
        {
            Some(g) => g,
            None => super::features::tagged_union(&acc, &p)?,
        };
        prev_end = end;
    }
    if glue
        && sp.closed
        && let Some(g) = super::sweep::path::glue_self(&acc)
    {
        acc = g;
    }
    Ok(Some(k.insert(acc.body)))
}

/// A piece's start or end caps: their areas and centroids.
fn caps(t: &Tagged, end: bool) -> Vec<(f64, P3)> {
    (0..t.body.faces.len())
        .filter(|&f| {
            t.tags[f].iter().any(|o| {
                if end {
                    matches!(o, Origin::EndCap { .. })
                } else {
                    matches!(o, Origin::StartCap { .. })
                }
            })
        })
        .map(|f| super::mass::face_area(&t.body, f))
        .collect()
}

/// Whether two caps coincide (as the sweep's gluing takes them).
fn same_cap(a: &(f64, P3), b: &(f64, P3)) -> bool {
    (a.1 - b.1).norm() < tol::point() && (a.0 - b.0).abs() < tol::point() * (1.0 + a.0)
}

/// A swept piece with its faces' origins (its caps among them), out of the kernel.
fn tagged(k: &mut NativeKernel, r: OpResult) -> Result<Tagged> {
    let id = r.bodies[0];
    let body = Body::clone(k.get(id)?);
    let mut tags = vec![Vec::new(); body.faces.len()];
    for (f, o) in r.history.generated {
        if let Some(t) = tags.get_mut(f.0 as usize) {
            t.push(o);
        }
    }
    crate::Kernel::release(k, id);
    Ok(Tagged { body, tags })
}

/// The union of bodies (`None` for none): glued where they share whole faces, else a boolean.
fn union_all(k: &mut NativeKernel, pieces: Vec<BodyId>) -> Result<Option<BodyId>> {
    let mut it = pieces.into_iter();
    let Some(mut acc) = it.next() else {
        return Ok(None);
    };
    for p in it {
        if let Some(g) = glue_shared(k.get(acc)?, k.get(p)?) {
            acc = k.insert(g);
            continue;
        }
        acc = crate::Kernel::boolean(k, BoolOp::Union, acc, &[p])?.bodies[0];
    }
    Ok(Some(acc))
}

/// Two solids that touch along whole faces (each face of one with a face of the other with the
/// same area and centroid, facing it), glued there into one; `None` when they share none or
/// don't glue into one solid.
fn glue_shared(a: &Body, b: &Body) -> Option<Body> {
    let areas = |x: &Body| -> Vec<(f64, P3)> {
        (0..x.faces.len())
            .map(|f| super::mass::face_area(x, f))
            .collect()
    };
    let (fa, fb) = (areas(a), areas(b));
    let size = a
        .verts
        .iter()
        .chain(&b.verts)
        .map(|v| v.p.coords.norm())
        .fold(1.0, f64::max);
    let mut ta = Tagged {
        body: a.clone(),
        tags: vec![Vec::new(); a.faces.len()],
    };
    let mut tb = Tagged {
        body: b.clone(),
        tags: vec![Vec::new(); b.faces.len()],
    };
    let mut shared = 0;
    for (i, (aa, ca)) in fa.iter().enumerate() {
        if let Some(j) = fb.iter().position(|(ab, cb)| {
            (aa - ab).abs() <= tol::REL * size * size && (ca - cb).norm() <= tol::REL * size
        }) {
            ta.tags[i].push(Origin::EndCap { region: shared });
            tb.tags[j].push(Origin::StartCap { region: shared });
            shared += 1;
        }
    }
    if shared == 0 {
        return None;
    }
    super::sweep::path::glue(&ta, &tb)
        .map(|t| t.body)
        .filter(|g| additive(g, a, b))
}

/// Whether `g`, `a` and `b` glued, holds their volumes together (they only touched: a glue
/// across an overlap would count it twice).
fn additive(g: &Body, a: &Body, b: &Body) -> bool {
    let (va, vb) = (super::mass::body_volume(a), super::mass::body_volume(b));
    (super::mass::body_volume(g) - va - vb).abs() <= tol::point() * (va.abs() + vb.abs())
}

/// `a` less `b`, `None` when nothing is left.
fn subtract(k: &mut NativeKernel, a: BodyId, b: BodyId) -> Result<Option<BodyId>> {
    match crate::Kernel::boolean(k, BoolOp::Subtract, a, &[b]) {
        Ok(r) => Ok(Some(r.bodies[0])),
        Err(KernelError::EmptyResult(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `a` and `b` in common, `None` when they don't meet.
fn intersect(k: &mut NativeKernel, a: BodyId, b: BodyId) -> Result<Option<BodyId>> {
    match crate::Kernel::boolean(k, BoolOp::Intersect, a, &[b]) {
        Ok(r) => Ok(Some(r.bodies[0])),
        Err(KernelError::EmptyResult(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

// --- Wire profiles (DRAW's `evolved`) ---------------------------------------------------------

/// An evolved solid of a wire profile: the solid, and the planes (a point and the normal) of
/// the lids that close an open profile.
#[derive(Debug, Clone)]
pub struct EvolvedWire {
    /// The solid.
    pub body: BodyId,
    /// The lids' planes (empty for a closed profile).
    pub lids: Vec<(P3, V3)>,
}

/// A piece of a profile's boundary in the profile's (x, y).
#[derive(Debug, Clone, Copy)]
enum Piece {
    Line {
        a: P2,
        b: P2,
    },
    /// From angle `a0` to `a1` (either way round).
    Arc {
        c: P2,
        r: f64,
        a0: f64,
        a1: f64,
    },
}

/// Bisection steps (far past f64 precision on [0, 1]).
const BISECT: usize = 64;

impl Piece {
    fn point(&self, f: f64) -> P2 {
        match *self {
            Piece::Line { a, b } => a + (b - a) * f,
            Piece::Arc { c, r, a0, a1 } => {
                let t = a0 + (a1 - a0) * f;
                c + V2::new(t.cos(), t.sin()) * r
            }
        }
    }
    fn part(&self, f0: f64, f1: f64) -> Piece {
        match *self {
            Piece::Line { .. } => Piece::Line {
                a: self.point(f0),
                b: self.point(f1),
            },
            Piece::Arc { c, r, a0, a1 } => Piece::Arc {
                c,
                r,
                a0: a0 + (a1 - a0) * f0,
                a1: a0 + (a1 - a0) * f1,
            },
        }
    }
    /// Where the piece is cut so each part is monotone in x and y and on one side of x = 0.
    fn cuts(&self) -> Vec<f64> {
        let mut fs = Vec::new();
        match *self {
            Piece::Line { a, b } => {
                if (a.x < 0.0 && b.x > 0.0) || (a.x > 0.0 && b.x < 0.0) {
                    fs.push(a.x / (a.x - b.x));
                }
            }
            Piece::Arc { c, r, a0, a1 } => {
                let (lo, hi) = (a0.min(a1), a0.max(a1));
                let mut angles: Vec<f64> = (0..4)
                    .map(|q| q as f64 * std::f64::consts::FRAC_PI_2)
                    .collect();
                if (c.x / r).abs() < 1.0 {
                    let t = (-c.x / r).acos();
                    angles.extend([t, -t]);
                }
                for t in angles {
                    let mut t = wrap(t, lo, std::f64::consts::TAU);
                    while t < hi {
                        if t > lo {
                            fs.push((t - a0) / (a1 - a0));
                        }
                        t += std::f64::consts::TAU;
                    }
                }
            }
        }
        fs.retain(|f| *f > tol::REL && *f < 1.0 - tol::REL);
        fs.sort_by(f64::total_cmp);
        fs
    }
    /// The parameter where a monotone `g(point)` reaches `v` (between the ends).
    fn solve(&self, v: f64, g: impl Fn(P2) -> f64) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        let up = g(self.point(1.0)) >= g(self.point(0.0));
        for _ in 0..BISECT {
            let m = 0.5 * (lo + hi);
            if (g(self.point(m)) < v) == up {
                lo = m;
            } else {
                hi = m;
            }
        }
        0.5 * (lo + hi)
    }
    /// The part of a piece monotone in y between heights `y0` and `y1`, running up.
    fn between(&self, y0: f64, y1: f64) -> Piece {
        let (f0, f1) = (self.solve(y0, |p| p.y), self.solve(y1, |p| p.y));
        self.part(f0, f1)
    }
    fn curve(&self) -> Curve2 {
        match *self {
            Piece::Line { a, b } => Curve2::Line { a, b, source: None },
            Piece::Arc { c, r, a0, a1 } => Curve2::Arc {
                center: c,
                radius: r,
                start_angle: a0,
                sweep: a1 - a0,
                source: None,
            },
        }
    }
}

/// A side of a strip's span: a vertical line, or a piece of the profile running up.
#[derive(Debug, Clone, Copy)]
enum Bound {
    Const(f64),
    Piece(Piece),
}

impl Bound {
    fn x(&self, y: f64) -> f64 {
        match self {
            Bound::Const(x) => *x,
            Bound::Piece(p) => p.point(p.solve(y, |q| q.y)).x,
        }
    }
    fn between(&self, y0: f64, y1: f64) -> Bound {
        match self {
            Bound::Const(x) => Bound::Const(*x),
            Bound::Piece(p) => Bound::Piece(p.between(y0, y1)),
        }
    }
    /// The heights between `y0` and `y1` where the side crosses x = `c`.
    fn crossing(&self, c: f64, y0: f64, y1: f64, eps: f64) -> Option<f64> {
        let Bound::Piece(p) = self else { return None };
        let (a, b) = (p.point(0.0).x - c, p.point(1.0).x - c);
        if (a < -eps && b > eps) || (a > eps && b < -eps) {
            let y = p.point(p.solve(c, |q| q.x)).y;
            (y > y0 + eps && y < y1 - eps).then_some(y)
        } else {
            None
        }
    }
    fn curve(&self, y0: f64, y1: f64) -> Curve2 {
        match self {
            Bound::Const(x) => Curve2::Line {
                a: P2::new(*x, y0),
                b: P2::new(*x, y1),
                source: None,
            },
            Bound::Piece(p) => p.curve(),
        }
    }
}

/// A span of a horizontal strip of the profile's region: from the side `l` to the side `r`
/// between heights `y0` and `y1`.
#[derive(Debug, Clone, Copy)]
struct Trap {
    y0: f64,
    y1: f64,
    l: Bound,
    r: Bound,
}

impl Trap {
    fn mid(&self) -> f64 {
        0.5 * (self.y0 + self.y1)
    }
    /// The part between heights `y0` and `y1`.
    fn part(&self, y0: f64, y1: f64) -> Trap {
        Trap {
            y0,
            y1,
            l: self.l.between(y0, y1),
            r: self.r.between(y0, y1),
        }
    }
}

/// The strips' spans of the region inside a closed loop of pieces (even–odd), the loop cut at
/// x = 0 so each span lies on one side of the spine. `eps` is the length tolerance; a piece
/// along x = `deep` becomes the vertical side `Const(deep)`.
fn strips(pieces: &[Piece], deep: f64, eps: f64) -> Result<Vec<Trap>> {
    let mut parts: Vec<Piece> = Vec::new();
    for p in pieces {
        let mut f0 = 0.0;
        for f in p.cuts().into_iter().chain([1.0]) {
            parts.push(p.part(f0, f));
            f0 = f;
        }
    }
    let mut ys: Vec<f64> = parts
        .iter()
        .flat_map(|p| [p.point(0.0).y, p.point(1.0).y])
        .collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() <= eps);
    let mut traps = Vec::new();
    for w in ys.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        let ym = 0.5 * (y0 + y1);
        let mut sides: Vec<(f64, Bound)> = Vec::new();
        for p in &parts {
            let (a, b) = (p.point(0.0), p.point(1.0));
            if a.y.min(b.y) > y0 + eps || a.y.max(b.y) < y1 - eps || (a.y - b.y).abs() <= eps {
                continue;
            }
            let side = if (a.x - deep).abs() <= eps && (b.x - deep).abs() <= eps {
                Bound::Const(deep)
            } else if a.x.abs() <= eps && b.x.abs() <= eps {
                Bound::Const(0.0)
            } else {
                Bound::Piece(p.between(y0, y1))
            };
            sides.push((side.x(ym), side));
        }
        if sides.len() % 2 == 1 {
            return Err(KernelError::InvalidGeometry {
                op: "evolved",
                why: "the profile's region doesn't close".into(),
            });
        }
        sides.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in sides.chunks(2) {
            let (l, r) = ((pair[0].0, pair[0].1), (pair[1].0, pair[1].1));
            if l.0 < -eps && r.0 > eps {
                // Across the spine: a span each side.
                traps.push(Trap {
                    y0,
                    y1,
                    l: l.1,
                    r: Bound::Const(0.0),
                });
                traps.push(Trap {
                    y0,
                    y1,
                    l: Bound::Const(0.0),
                    r: r.1,
                });
            } else if r.0 - l.0 > eps {
                traps.push(Trap {
                    y0,
                    y1,
                    l: l.1,
                    r: r.1,
                });
            }
        }
    }
    Ok(traps)
}

/// The spans with the part on the far side of x = `c` dropped (`keep_right`: the part at x ≥
/// `c` kept, else at x ≤ `c`).
fn clip(traps: &[Trap], c: f64, keep_right: bool, eps: f64) -> Vec<Trap> {
    let mut out = Vec::new();
    for t in traps {
        let mut cuts: Vec<f64> = [
            t.l.crossing(c, t.y0, t.y1, eps),
            t.r.crossing(c, t.y0, t.y1, eps),
        ]
        .into_iter()
        .flatten()
        .collect();
        cuts.sort_by(f64::total_cmp);
        let mut y0 = t.y0;
        for y1 in cuts.into_iter().chain([t.y1]) {
            if y1 - y0 <= eps {
                continue;
            }
            let mut p = t.part(y0, y1);
            y0 = y1;
            let (xl, xr) = (p.l.x(p.mid()), p.r.x(p.mid()));
            if keep_right {
                if xr <= c + eps {
                    continue;
                }
                if xl < c {
                    p.l = Bound::Const(c);
                }
            } else {
                if xl >= c - eps {
                    continue;
                }
                if xr > c {
                    p.r = Bound::Const(c);
                }
            }
            out.push(p);
        }
    }
    out
}

/// The inner spans (at x ≤ 0) with the part of them past x = `c` (where the spine's region
/// holds no point) reaching to `deep`.
fn deepen(traps: &[Trap], c: f64, deep: f64, eps: f64) -> Vec<Trap> {
    let mut out = Vec::new();
    for t in traps {
        if t.r.x(t.mid()) > eps || matches!(t.l, Bound::Const(_)) {
            out.push(*t);
            continue;
        }
        let mut y0 = t.y0;
        for y1 in t.l.crossing(c, t.y0, t.y1, eps).into_iter().chain([t.y1]) {
            let mut p = t.part(y0, y1);
            y0 = y1;
            if p.l.x(p.mid()) < c {
                p.l = Bound::Const(deep);
            }
            out.push(p);
        }
    }
    out
}

/// The spans as regions: spans stacked on each other with the same shared side merged into one.
fn regions(traps: &[Trap], eps: f64) -> Vec<Region> {
    struct Column {
        bottom: (f64, f64, f64),
        top: (f64, f64, f64),
        rights: Vec<(Bound, f64, f64)>,
        lefts: Vec<(Bound, f64, f64)>,
    }
    let mut sorted: Vec<&Trap> = traps.iter().collect();
    sorted.sort_by(|a, b| {
        a.y0.total_cmp(&b.y0)
            .then(a.l.x(a.mid()).total_cmp(&b.l.x(b.mid())))
    });
    let mut cols: Vec<Column> = Vec::new();
    for t in sorted {
        let (xl0, xr0, xl1, xr1) = (t.l.x(t.y0), t.r.x(t.y0), t.l.x(t.y1), t.r.x(t.y1));
        let found = cols.iter_mut().find(|c| {
            (c.top.2 - t.y0).abs() <= eps
                && (c.top.0 - xl0).abs() <= eps
                && (c.top.1 - xr0).abs() <= eps
        });
        let col = match found {
            Some(c) => c,
            None => {
                cols.push(Column {
                    bottom: (xl0, xr0, t.y0),
                    top: (xl0, xr0, t.y0),
                    rights: vec![],
                    lefts: vec![],
                });
                let last = cols.len() - 1;
                &mut cols[last]
            }
        };
        col.rights.push((t.r, t.y0, t.y1));
        col.lefts.push((t.l, t.y0, t.y1));
        col.top = (xl1, xr1, t.y1);
    }
    let line = |a: P2, b: P2| Curve2::Line { a, b, source: None };
    let reversed = |c: Curve2| match c {
        Curve2::Line { a, b, .. } => line(b, a),
        Curve2::Arc {
            center,
            radius,
            start_angle,
            sweep,
            ..
        } => Curve2::Arc {
            center,
            radius,
            start_angle: start_angle + sweep,
            sweep: -sweep,
            source: None,
        },
        other => other,
    };
    cols.into_iter()
        .map(|c| {
            let mut curves = Vec::new();
            let (xl, xr, y) = c.bottom;
            if xr - xl > eps {
                curves.push(line(P2::new(xl, y), P2::new(xr, y)));
            }
            curves.extend(c.rights.iter().map(|(b, y0, y1)| b.curve(*y0, *y1)));
            let (xl, xr, y) = c.top;
            if xr - xl > eps {
                curves.push(line(P2::new(xr, y), P2::new(xl, y)));
            }
            curves.extend(
                c.lefts
                    .iter()
                    .rev()
                    .map(|(b, y0, y1)| reversed(b.curve(*y0, *y1))),
            );
            Region {
                outer: Loop { curves },
                holes: vec![],
                source: None,
            }
        })
        .collect()
}

/// The union of the sweeps of `traps` (each on one side of the spine) along the spine; arcs
/// sweep only the part of the spans short of their axis, corners turn the part on the side
/// the spine turns away from.
fn sweep_traps(
    k: &mut NativeKernel,
    sp: &Spine,
    traps: &[Trap],
    eps: f64,
    glue: bool,
) -> Result<Option<BodyId>> {
    if traps.is_empty() {
        return Ok(None);
    }
    let make = |axis: Option<f64>| -> Result<Vec<Region>> {
        Ok(match axis {
            Some(c) if c < 0.0 => regions(&clip(traps, c, true, eps), eps),
            Some(c) => regions(&clip(traps, c, false, eps), eps),
            None => regions(traps, eps),
        })
    };
    let side = |outside: bool| -> Result<Vec<Region>> {
        let ts: Vec<Trap> = traps
            .iter()
            .filter(|t| (0.5 * (t.l.x(t.mid()) + t.r.x(t.mid())) > 0.0) == outside)
            .copied()
            .collect();
        Ok(regions(&ts, eps))
    };
    sweep_union(k, sp, &make, &side, glue)
}

/// Height ranges sorted and merged where they meet.
fn merge_ranges(mut ranges: Vec<(f64, f64)>, eps: f64) -> Vec<(f64, f64)> {
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (a, b) in ranges {
        match merged.last_mut() {
            Some(last) if a <= last.1 + eps => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// The spine's region swept up from height `y0` to `y1`.
fn spine_prism(
    k: &mut NativeKernel,
    sp: &Spine,
    face: &Profile,
    y0: f64,
    y1: f64,
) -> Result<BodyId> {
    let mut f = face.clone();
    f.plane.origin += sp.up * y0;
    Ok(super::features::prism_along(k, &f, sp.up * (y1 - y0))?.bodies[0])
}

/// DRAW's `evolved` with a wire profile (see the module's notes): the profile (lines and
/// circular arcs in one plane square to the spine, open or closed) measured from the spine point
/// nearest it, swept
/// along the closed `spine` (lines and circular arcs) whose region is `spine_face`. Returns the
/// solid and its lids' planes.
pub fn evolved_wire(
    k: &mut NativeKernel,
    spine: &[(Curve, f64, f64)],
    spine_face: &Profile,
    profile: &[(Curve, f64, f64)],
) -> Result<EvolvedWire> {
    // The sweeps' pieces glued at their shared caps; where a boolean of the glued sweeps fails,
    // again with the pieces united instead (their faces then split otherwise).
    evolved_wire_with(k, spine, spine_face, profile, true)
        .or_else(|_| evolved_wire_with(k, spine, spine_face, profile, false))
}

fn evolved_wire_with(
    k: &mut NativeKernel,
    spine: &[(Curve, f64, f64)],
    spine_face: &Profile,
    profile: &[(Curve, f64, f64)],
    glue: bool,
) -> Result<EvolvedWire> {
    if profile.is_empty() {
        return Err(KernelError::InvalidParameter(
            "an evolved solid needs a profile".into(),
        ));
    }
    let sp = Spine::new(spine)?;
    if !sp.closed {
        return Err(KernelError::Unsupported(
            "evolved solids of wire profiles along open spines",
        ));
    }
    // The profile's frame: an origin on the spine, x outwards and up.
    let samples: Vec<P3> = profile
        .iter()
        .flat_map(|(c, t0, t1)| (0..=16).map(move |i| c.point(t0 + (t1 - t0) * i as f64 / 16.0)))
        .collect();
    let (_, origin, t) = samples
        .iter()
        .map(|q| sp.nearest(q))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .ok_or_else(|| KernelError::InvalidParameter("an evolved solid needs a profile".into()))?;
    let xo = t.cross(&sp.up).normalize();
    let normal = xo.cross(&sp.up);
    let to2 = |p: &P3| P2::new((p - origin).dot(&xo), (p - origin).dot(&sp.up));
    let mut pieces: Vec<Piece> = Vec::new();
    for (c, t0, t1) in profile {
        match c {
            Curve::Line { .. } => pieces.push(Piece::Line {
                a: to2(&c.point(*t0)),
                b: to2(&c.point(*t1)),
            }),
            Curve::Circle { c: centre, x, y, r } => {
                let axis = x.cross(y);
                if axis.cross(&normal).norm() > tol::point() {
                    return Err(KernelError::Unsupported(
                        "evolved profiles with arcs out of the profile's plane",
                    ));
                }
                let cc = to2(centre);
                let a = to2(&c.point(*t0)) - cc;
                let a0 = a.y.atan2(a.x);
                pieces.push(Piece::Arc {
                    c: cc,
                    r: *r,
                    a0,
                    a1: a0 + (t1 - t0) * axis.dot(&normal).signum(),
                });
            }
            _ => {
                return Err(KernelError::Unsupported(
                    "evolved profiles of curves other than lines and arcs",
                ));
            }
        }
    }
    let size = sp.size.max(
        samples
            .iter()
            .map(|p| (p - origin).norm())
            .fold(0.0, f64::max),
    );
    let eps = tol::point() * (1.0 + size);
    let (p0, p1) = (pieces[0].point(0.0), pieces[pieces.len() - 1].point(1.0));
    // A point past every inward offset of the spine.
    let deep = -2.0 * size - 1.0;
    let mut lids = Vec::new();
    if (p1 - p0).norm() > eps {
        // An open profile, closed by lids inwards at its ends.
        let line = |a: P2, b: P2| Piece::Line { a, b };
        pieces.push(line(p1, P2::new(deep, p1.y)));
        if (p1.y - p0.y).abs() > eps {
            pieces.push(line(P2::new(deep, p1.y), P2::new(deep, p0.y)));
        }
        pieces.push(line(P2::new(deep, p0.y), p0));
        lids = vec![
            (origin + sp.up * p0.y, sp.up),
            (origin + sp.up * p1.y, sp.up),
        ];
    }
    let traps = strips(&pieces, deep, eps)?;
    // Inward past every offset of the spine's region is as deep as can be.
    let r_in = sp.inradius_bound();
    let traps = deepen(&traps, -r_in, deep, eps);
    // The spans by how they're built.
    let is = |b: &Bound, x: f64| matches!(b, Bound::Const(v) if (v - x).abs() <= eps);
    let (mut touch_in, mut touch_out, mut deep_ranges, mut deep_cut) =
        (vec![], vec![], vec![], vec![]);
    let mut floating: Vec<(Trap, Trap, bool)> = vec![];
    // The heights where the solid holds the spine's whole region.
    let mut full: Vec<(f64, f64)> = vec![];
    for t in &traps {
        let zero = Bound::Const(0.0);
        if t.r.x(t.mid()) <= eps {
            match (is(&t.l, deep), is(&t.r, 0.0)) {
                (true, true) => {
                    deep_ranges.push((t.y0, t.y1));
                    full.push((t.y0, t.y1));
                }
                (true, false) => {
                    deep_ranges.push((t.y0, t.y1));
                    deep_cut.push(Trap {
                        l: t.r,
                        r: zero,
                        ..*t
                    });
                }
                (false, true) => touch_in.push(*t),
                (false, false) => floating.push((
                    Trap { r: zero, ..*t },
                    Trap {
                        l: t.r,
                        r: zero,
                        ..*t
                    },
                    false,
                )),
            }
        } else if is(&t.l, 0.0) {
            touch_out.push(*t);
        } else {
            floating.push((
                Trap { l: zero, ..*t },
                Trap {
                    l: zero,
                    r: t.l,
                    ..*t
                },
                true,
            ));
        }
    }
    let range = |ts: &[Trap]| {
        ts.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| {
                (a.min(t.y0), b.max(t.y1))
            })
    };
    let full = merge_ranges(full, eps);
    // Whether the solid holds the spine's whole region at every height of the spans (whatever of
    // a sweep outwards reaches back into it is then part of the solid anyway).
    let held = |ts: &[Trap]| {
        ts.iter().all(|t| {
            full.iter()
                .any(|(a, b)| *a <= t.y0 + eps && t.y1 <= *b + eps)
        })
    };
    let mut parts: Vec<BodyId> = Vec::new();
    // Spans reaching past every inward offset: the spine's prism less the sweep of the rest.
    let mut prisms = Vec::new();
    for (a, b) in merge_ranges(deep_ranges, eps) {
        prisms.push(spine_prism(k, &sp, spine_face, a, b)?);
    }
    if let Some(mut d) = union_all(k, prisms)? {
        let mut keep = true;
        // (A cut deeper than every offset of the region takes it all: the sweep stops there.)
        if let Some(cut) = sweep_traps(k, &sp, &clip(&deep_cut, -r_in, true, eps), eps, glue)? {
            match subtract(k, d, cut)? {
                Some(r) => d = r,
                None => keep = false,
            }
        }
        if keep {
            parts.push(d);
        }
    }
    // Spans reaching the spine: their sweeps, kept to the spine's side (in one sweep when
    // neither side's sweep crosses the spine).
    let depth = |ts: &[Trap]| {
        ts.iter()
            .flat_map(|t| {
                [
                    t.l.x(t.y0),
                    t.l.x(t.y1),
                    t.l.x(t.mid()),
                    t.r.x(t.y0),
                    t.r.x(t.y1),
                    t.r.x(t.mid()),
                ]
            })
            .fold(0.0, |m: f64, x| m.max(x.abs()))
    };
    let keeps = |ts: &[Trap], outside: bool| {
        ts.is_empty() || (outside && held(ts)) || sp.keeps_side(outside, depth(ts), eps)
    };
    let (keep_in, keep_out) = (keeps(&touch_in, false), keeps(&touch_out, true));
    let mut done = false;
    if keep_in && keep_out && !touch_in.is_empty() && !touch_out.is_empty() {
        // Both sides in one sweep; failing that (a side's knuckle meets the other side's
        // overlap in a boolean there), each side on its own.
        let all: Vec<Trap> = touch_in.iter().chain(&touch_out).copied().collect();
        if let Ok(Some(s)) = sweep_traps(k, &sp, &all, eps, glue) {
            parts.push(s);
            done = true;
        }
    }
    if !done {
        for (ts, outside, keep) in [(&touch_in, false, keep_in), (&touch_out, true, keep_out)] {
            let Some(s) = sweep_traps(k, &sp, ts, eps, glue)? else {
                continue;
            };
            if keep {
                parts.push(s);
                continue;
            }
            let (a, b) = range(ts);
            let prism = spine_prism(k, &sp, spine_face, a, b)?;
            if let Some(r) = if outside {
                subtract(k, s, prism)?
            } else {
                intersect(k, s, prism)?
            } {
                parts.push(r);
            }
        }
    }
    // Spans apart from the spine: the sweep to their far side less the sweep to their near side.
    for (far, near, outside) in floating {
        let (Some(f), Some(n)) = (
            sweep_traps(k, &sp, &[far], eps, glue)?,
            sweep_traps(k, &sp, &[near], eps, glue)?,
        ) else {
            continue;
        };
        let Some(s) = subtract(k, f, n)? else {
            continue;
        };
        if keeps(&[far], outside) {
            parts.push(s);
            continue;
        }
        let prism = spine_prism(k, &sp, spine_face, far.y0, far.y1)?;
        if let Some(r) = if outside {
            subtract(k, s, prism)?
        } else {
            intersect(k, s, prism)?
        } {
            parts.push(r);
        }
    }
    let body = union_all(k, parts)?
        .ok_or_else(|| KernelError::EmptyResult("the evolved profile sweeps nothing".into()))?;
    Ok(EvolvedWire { body, lids })
}
