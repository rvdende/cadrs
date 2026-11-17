//! Delete face with healing (defeaturing): faces taken out of a solid and the gap closed by the
//! faces round them, each run on along its own surface until they meet (a round or a bevel
//! deleted: the edge it replaced comes back; a notch or a step deleted: the block is whole).
//!
//! No boolean and no sampling: the faces left are bounded again by exact curves.
//!
//! 1. The kept faces that lose an edge to a deleted face are gathered by surface (two pieces of
//!    one plane either side of a deleted slot become one face).
//! 2. Each run of lost coedges in a loop (from the end of a kept edge to the start of the next)
//!    is replaced by a path along the *candidate curves* of its face: the intersections of the
//!    face's surface with the other faces round the same deleted faces
//!    ([`super::ssi::surface_surface`], closed forms where the surfaces have them), and the
//!    face's own seams run on. The path is the far boundary of the cell of those curves that
//!    lies against the lost run (the gap's part of this surface): traced from the run's start
//!    with the cell on the left, turning leftmost wherever curves cross. Where the cell is
//!    bounded by a kept edge itself, the path runs back along it and the edge is shortened (a
//!    bevel deleted from a pocket's rim: the pocket's mouth narrows).
//! 3. A loop lost whole (a cylinder's end face inside a rounded rim) has no start: a ray on the
//!    surface from the loop, away from the face, finds the cell's far boundary, or none (a
//!    hole's rim: the loop goes and the face closes over).
//! 4. The body is rebuilt from the kept edges and the paths' pieces, shared between the two
//!    faces that traced them ([`super::xbuild`], which fits the p-curves).

use super::curve::Curve;
use super::math::*;
use super::ssi;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use super::xbuild::{XBuilder, XFace};
use crate::{KernelError, Result};

/// Whether `CADRS_DEFEATURE_DEBUG` asks for the tracer's steps on stderr.
fn debug() -> bool {
    crate::native::env_flag!("CADRS_DEFEATURE_DEBUG")
}

fn failed(why: impl Into<String>) -> KernelError {
    KernelError::InvalidResult {
        op: "delete face",
        why: why.into(),
    }
}

/// A curve a face's new boundary may run along.
#[derive(Debug, Clone)]
struct Cand {
    c: Curve,
    t0: f64,
    t1: f64,
    closed: bool,
}

impl Cand {
    /// The parameter of the point nearest `p` and how far it is.
    fn at(&self, p: &P3) -> (f64, f64) {
        let t = self.c.closest(p, self.t0, self.t1);
        (t, (self.c.point(t) - p).norm())
    }
}

/// A piece of a loop: along `c` from `ta` (at `a`) to `tb` (at `b`), either way along the
/// curve; `old` when it is a whole kept coedge (edge, along it).
#[derive(Debug, Clone)]
struct Piece {
    c: Curve,
    ta: f64,
    tb: f64,
    a: P3,
    b: P3,
    old: Option<(usize, bool)>,
    /// Whether it ends at a point where the surface collapses (a cone's apex, a sphere's
    /// pole): the boundary turns back there along the same curve, and nothing cancels.
    pole: bool,
}

impl Piece {
    /// The unit direction of travel at the parameter `t`.
    fn dir(&self, t: f64) -> V3 {
        let d = self.c.tangent(t).normalize();
        if self.tb >= self.ta { d } else { -d }
    }
}

/// A group of kept faces on one surface that lose coedges.
struct Pool {
    faces: Vec<usize>,
    surf: Surface,
    sense: bool,
}

/// The outward normal of a pool's surface at `p`.
fn normal_at(surf: &Surface, sense: bool, p: &P3) -> V3 {
    let (u, v) = surf.project(p, None);
    surf.normal(u, v) * if sense { 1.0 } else { -1.0 }
}

/// The angle from `d` to `e` about `n`, in (−π, π].
fn turn(n: &V3, d: &V3, e: &V3) -> f64 {
    n.dot(&d.cross(e)).atan2(d.dot(e))
}

/// How far round to the right (`side` 1; to the left for −1) `e` lies from `back` about `n`, in
/// (0, 2π]: the way out of a point with the smallest is the leftmost (rightmost) turn for a
/// path that came in from `back`, which keeps the cell on its left (right).
fn right_of(n: &V3, back: &V3, e: &V3, side: f64) -> f64 {
    let a = (-side * turn(n, back, e)).rem_euclid(TAU);
    if a < 1e-9 { TAU } else { a }
}

/// Where the tracer is: on candidate `ci` at `t`, travelling up (`up`) or down its parameter.
#[derive(Clone, Copy)]
struct Cursor {
    ci: usize,
    t: f64,
    up: bool,
}

/// What lies ahead on a curve.
enum Event {
    /// One of the end points, by index.
    End(usize),
    /// A crossing with candidate `ci` at its parameter `t`.
    Cross(usize, f64),
    /// A point where the surface collapses.
    Pole,
}

struct Tracer<'a> {
    surf: &'a Surface,
    sense: bool,
    cands: &'a [Cand],
    /// Points nearer than this are one.
    tol: f64,
    /// How far ahead a path's direction is taken, to tell curves that cross tangentially apart.
    ahead: f64,
    /// The points where the surface collapses.
    poles: Vec<P3>,
}

impl Tracer<'_> {
    /// The events ahead of parameter `t` on `c` (travelling `up` or down), each with its
    /// parameter on `c`, nearest first. `skip`: the candidate `c` is.
    fn events(
        &self,
        c: &Cand,
        skip: Option<usize>,
        t: f64,
        up: bool,
        ends: &[P3],
    ) -> Vec<(f64, f64, Event)> {
        let speed = c.c.tangent(t).norm().max(tol::TINY);
        let period = c.t1 - c.t0;
        // How far ahead (along the parameter) a parameter lies; behind is a turn ahead on a
        // closed curve, nothing on an open one.
        let ahead = |x: f64| -> Option<f64> {
            let mut d = if up { x - t } else { t - x };
            if c.closed {
                d = d.rem_euclid(period);
                if d * speed < self.tol * 4.0 {
                    d += period;
                }
            }
            (d * speed > self.tol * 4.0).then_some(d)
        };
        let mut out = Vec::new();
        for (k, e) in ends.iter().enumerate() {
            let (x, dist) = c.at(e);
            if dist < self.tol * 4.0
                && let Some(d) = ahead(x)
            {
                out.push((d, x, Event::End(k)));
            }
        }
        for q in &self.poles {
            let (x, dist) = c.at(q);
            if dist < self.tol * 4.0
                && let Some(d) = ahead(x)
            {
                out.push((d, x, Event::Pole));
            }
        }
        for (j, o) in self.cands.iter().enumerate() {
            if Some(j) == skip {
                continue;
            }
            for (x, y) in ssi::curve_curve(&c.c, c.t0, c.t1, &o.c, o.t0, o.t1, self.tol) {
                if let Some(d) = ahead(x) {
                    out.push((d, x, Event::Cross(j, y)));
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// The direction a path leaves `x` in, a little ahead along candidate `ci` from `t`.
    fn leaving(&self, x: &P3, ci: usize, t: f64, up: bool) -> Option<V3> {
        let c = &self.cands[ci];
        let speed = c.c.tangent(t).norm().max(tol::TINY);
        let mut s = t + self.ahead / speed * if up { 1.0 } else { -1.0 };
        if !c.closed {
            if s > c.t1 + tol::ROUNDOFF || s < c.t0 - tol::ROUNDOFF {
                return None;
            }
            s = s.clamp(c.t0, c.t1);
        }
        (c.c.point(s) - x).try_normalize(tol::DEGENERATE)
    }

    /// The path from `start`, which the boundary reaches from the direction `back`, round the
    /// cell on its left (`side` 1; on its right for −1) to one of `ends`: its pieces and the
    /// end's index.
    fn trace(&self, start: P3, back: V3, ends: &[P3], side: f64) -> Result<(Vec<Piece>, usize)> {
        let open = || failed("the faces round the deleted ones don't close the gap");
        let n = normal_at(self.surf, self.sense, &start);
        // The leftmost way out of the start, along any candidate through it.
        let mut best: Option<(f64, Cursor)> = None;
        for (ci, c) in self.cands.iter().enumerate() {
            let (t, dist) = c.at(&start);
            if dist > self.tol * 4.0 {
                continue;
            }
            for up in [true, false] {
                let Some(e) = self.leaving(&start, ci, t, up) else {
                    continue;
                };
                let a = right_of(&n, &back, &e, side);
                if best.is_none_or(|(b, _)| a < b) {
                    best = Some((a, Cursor { ci, t, up }));
                }
            }
        }
        if debug() {
            eprintln!(
                "trace from {start:?} back {back:?} ends {ends:?}: {} cands, start {:?}",
                self.cands.len(),
                best.map(|b| (b.0, b.1.ci, b.1.t, b.1.up))
            );
            for (i, c) in self.cands.iter().enumerate() {
                eprintln!(
                    "  cand {i}: {:?} [{}, {}] closed {} at-start {:?}",
                    c.c,
                    c.t0,
                    c.t1,
                    c.closed,
                    c.at(&start)
                );
            }
        }
        let mut cur = best.ok_or_else(open)?.1;
        let mut from = (cur.t, start);
        let mut pieces = Vec::new();
        for _ in 0..256 {
            let c = &self.cands[cur.ci];
            let ev = self.events(c, Some(cur.ci), cur.t, cur.up, ends);
            if debug() {
                eprintln!(
                    "  on {} t {} up {}: events {:?}",
                    cur.ci,
                    cur.t,
                    cur.up,
                    ev.iter()
                        .map(|e| (
                            e.0,
                            e.1,
                            match e.2 {
                                Event::End(k) => format!("end {k}"),
                                Event::Cross(j, y) => format!("cross {j} at {y}"),
                                Event::Pole => "pole".into(),
                            }
                        ))
                        .collect::<Vec<_>>()
                );
            }
            let Some(first) = ev.first() else {
                return Err(open());
            };
            let x = c.c.point(first.1);
            let piece = |tb: f64, b: P3| Piece {
                c: c.c.clone(),
                ta: from.0,
                tb: if c.closed && (tb - from.0).abs() * c.c.tangent(tb).norm() < self.tol {
                    // A whole turn of a closed curve.
                    from.0 + (c.t1 - c.t0) * if cur.up { 1.0 } else { -1.0 }
                } else if c.closed && cur.up && tb < from.0 {
                    tb + (c.t1 - c.t0)
                } else if c.closed && !cur.up && tb > from.0 {
                    tb - (c.t1 - c.t0)
                } else {
                    tb
                },
                a: from.1,
                b,
                old: None,
                pole: false,
            };
            // Everything at the first event's point.
            let here: Vec<&(f64, f64, Event)> = ev
                .iter()
                .take_while(|e| (c.c.point(e.1) - x).norm() < self.tol * 4.0)
                .collect();
            if let Some(k) = here.iter().find_map(|e| match e.2 {
                Event::End(k) => Some(k),
                _ => None,
            }) {
                pieces.push(piece(first.1, ends[k]));
                return Ok((pieces, k));
            }
            // At a pole the boundary turns back along the curve it came by (the other use
            // of a seam; the pole's own edge is put between them when the body is built).
            if here.iter().any(|e| matches!(e.2, Event::Pole)) {
                pieces.push(Piece {
                    pole: true,
                    ..piece(first.1, x)
                });
                cur.t = first.1;
                cur.up = !cur.up;
                from = (cur.t, x);
                continue;
            }
            // Straight on, or the leftmost of the curves crossing here: the first way out
            // round to the right from where the path came in.
            let nx = normal_at(self.surf, self.sense, &x);
            let d = c.c.tangent(first.1).normalize() * if cur.up { 1.0 } else { -1.0 };
            let Some(back) = self.leaving(&x, cur.ci, first.1, !cur.up) else {
                return Err(open());
            };
            let on = self
                .leaving(&x, cur.ci, first.1, cur.up)
                .map_or(f64::INFINITY, |e| right_of(&nx, &back, &e, side));
            let mut best: Option<(f64, Cursor)> = None;
            for e in &here {
                let Event::Cross(j, y) = e.2 else { continue };
                for up in [true, false] {
                    let Some(l) = self.leaving(&x, j, y, up) else {
                        continue;
                    };
                    // Never back along a curve that touches this one here (a wall's line
                    // and the circle of the cylinder rounding its end): a boundary has no
                    // cusp; it passes from one to the other.
                    let dj = self.cands[j].c.tangent(y).normalize() * if up { 1.0 } else { -1.0 };
                    if dj.dot(&d) < -1.0 + tol::ANGULAR {
                        continue;
                    }
                    // On along a curve that touches this one: the two faces behind them
                    // join smoothly here (in an edge that ends at this point), and the
                    // boundary passes from the one's curve to the other's.
                    let a = if dj.dot(&d) > 1.0 - tol::ANGULAR {
                        0.0
                    } else {
                        right_of(&nx, &back, &l, side)
                    };
                    if best.is_none_or(|(b, _)| a < b) {
                        best = Some((a, Cursor { ci: j, t: y, up }));
                    }
                }
            }
            match best {
                Some((a, next)) if a < on => {
                    pieces.push(piece(first.1, x));
                    cur = next;
                    from = (next.t, x);
                }
                _ => {
                    if on == f64::INFINITY {
                        return Err(open());
                    }
                    // (Past the crossing; a closed curve's parameter stays in its turn.)
                    cur.t = first.1;
                }
            }
        }
        Err(open())
    }
}

/// Removes the doubling back from a loop of pieces: where a piece runs back along the one
/// before it (a kept edge that the gap's cell lay against), the two cancel as far as they
/// overlap. `None` when nothing of the loop is left.
fn cancel(mut l: Vec<Piece>, tol: f64) -> Result<Option<Vec<Piece>>> {
    let mut guard = 0;
    'again: loop {
        guard += 1;
        if guard > 4096 {
            return Err(failed("a face's new boundary doesn't settle"));
        }
        let n = l.len();
        if n == 0 {
            return Ok(None);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            if i == j {
                break;
            }
            let (p, q) = (&l[i], &l[j]);
            if (p.b - q.a).norm() > tol * 8.0 {
                return Err(failed("a face's new boundary has a gap"));
            }
            if p.pole || p.dir(p.tb).dot(&q.dir(q.ta)) > -1.0 + 1e-9 {
                continue;
            }
            // `q` runs back along `p`'s curve? Where its end falls on `p`.
            let (lo, hi) = (p.ta.min(p.tb), p.ta.max(p.tb));
            let span = hi - lo;
            let wide = (lo - span - 1.0, hi + span + 1.0);
            let t = match &p.c {
                Curve::Line { .. } => p.c.closest(&q.b, wide.0, wide.1),
                _ if p.c.is_periodic() => {
                    let t = p.c.closest(&q.b, lo, lo + TAU);
                    // Back from `p`'s end, within a turn.
                    if p.tb >= p.ta {
                        unwrap_near(t, p.tb - PI, TAU)
                    } else {
                        unwrap_near(t, p.tb + PI, TAU)
                    }
                }
                _ => p.c.closest(&q.b, lo, hi),
            };
            if (p.c.point(t) - q.b).norm() > tol * 4.0 {
                continue;
            }
            // (The middle of `q` on `p`'s curve too: two arcs can share their ends alone.)
            let qm = q.c.point(0.5 * (q.ta + q.tb));
            let tm = match &p.c {
                Curve::Line { .. } => p.c.closest(&qm, wide.0, wide.1),
                _ if p.c.is_periodic() => p.c.closest(&qm, lo, lo + TAU),
                _ => p.c.closest(&qm, lo, hi),
            };
            if (p.c.point(tm) - qm).norm() > tol * 4.0 {
                continue;
            }
            let along = if p.tb >= p.ta { t - p.ta } else { p.ta - t };
            let speed = p.c.tangent(p.ta).norm().max(tol::TINY);
            if along * speed > tol * 4.0 {
                // `q` ends inside `p`: `p` is shortened to there.
                let b = q.b;
                l[i] = Piece {
                    tb: t,
                    b,
                    old: None,
                    ..l[i].clone()
                };
                l.remove(j);
            } else if along * speed > -tol * 4.0 {
                // They cancel whole.
                let (hi, lo) = (i.max(j), i.min(j));
                l.remove(hi);
                l.remove(lo);
            } else {
                // `q` runs on past `p`'s start: what is left of it starts there.
                let (tq, _) = (q.c.closest(&p.a, q.ta.min(q.tb), q.ta.max(q.tb)), ());
                let a = p.a;
                l[j] = Piece {
                    ta: tq,
                    a,
                    old: None,
                    ..l[j].clone()
                };
                l.remove(i);
            }
            continue 'again;
        }
        return Ok(Some(l));
    }
}

/// `b` without the faces `gone`, the faces round them run on along their surfaces to close
/// the gap: the new body and, for each of its faces, the faces of `b` it comes from.
pub fn delete_faces(b: &Body, gone: &[usize]) -> Result<(Body, Vec<Vec<usize>>)> {
    let nf = b.faces.len();
    let mut is_gone = vec![false; nf];
    for &f in gone {
        is_gone[f] = true;
    }
    let ef = b.edge_faces();
    let lost = |e: usize| ef[e].iter().any(|&f| is_gone[f]);
    let size = b.bbox().diagonal().max(tol::POINT);
    let ptol = tol::POINT;

    // The deleted faces in groups joined through vertices, and the vertices of each.
    let mut group = vec![usize::MAX; nf];
    let mut vert_group = vec![usize::MAX; b.verts.len()];
    let mut ngroups = 0;
    {
        let mut vf: Vec<Vec<usize>> = vec![Vec::new(); b.verts.len()];
        for &f in gone {
            for c in b.faces[f].loops.iter().flatten() {
                for v in b.edges[c.edge].v {
                    vf[v].push(f);
                }
            }
        }
        for &s in gone {
            if group[s] != usize::MAX {
                continue;
            }
            let mut stack = vec![s];
            group[s] = ngroups;
            while let Some(f) = stack.pop() {
                for c in b.faces[f].loops.iter().flatten() {
                    for v in b.edges[c.edge].v {
                        vert_group[v] = ngroups;
                        for &g in &vf[v] {
                            if group[g] == usize::MAX {
                                group[g] = ngroups;
                                stack.push(g);
                            }
                        }
                    }
                }
            }
            ngroups += 1;
        }
    }

    // The kept faces touching a group (by a vertex at least), and those losing coedges, pooled
    // by surface.
    let mut touching: Vec<Vec<usize>> = vec![Vec::new(); ngroups];
    let mut losing = Vec::new();
    for fi in 0..nf {
        if is_gone[fi] {
            continue;
        }
        let mut loses = false;
        for c in b.faces[fi].loops.iter().flatten() {
            loses |= lost(c.edge);
            for v in b.edges[c.edge].v {
                let g = vert_group[v];
                if g != usize::MAX && !touching[g].contains(&fi) {
                    touching[g].push(fi);
                }
            }
        }
        if loses {
            losing.push(fi);
        }
    }
    let same = |x: usize, y: usize| {
        let (fx, fy) = (&b.faces[x], &b.faces[y]);
        fx.surf
            .same_domain(&fy.surf, tol::CONFUSION.max(tol::REL * size))
            .is_some_and(|s| s == (fx.sense == fy.sense))
    };
    let mut pools: Vec<Pool> = Vec::new();
    let mut pool_of = vec![usize::MAX; nf];
    for &fi in &losing {
        let at = pools.iter().position(|p| same(p.faces[0], fi));
        let pi = at.unwrap_or_else(|| {
            pools.push(Pool {
                faces: Vec::new(),
                surf: b.faces[fi].surf.clone(),
                sense: b.faces[fi].sense,
            });
            pools.len() - 1
        });
        pools[pi].faces.push(fi);
        pool_of[fi] = pi;
    }

    // The candidate curves of each pool: its surface against every other surface touching the
    // groups it touches, once per pair of surfaces; and its own kept edges that both lie in it
    // (seams), run on.
    let bb = b.bbox().grow(size);
    let reach = 4.0 * size;
    let mut cands: Vec<Vec<Cand>> = vec![Vec::new(); pools.len()];
    {
        // The distinct surfaces touching each group.
        let mut others: Vec<Vec<usize>> = vec![Vec::new(); pools.len()];
        for (pi, p) in pools.iter().enumerate() {
            let mut gs: Vec<usize> = Vec::new();
            for &f in &p.faces {
                for c in b.faces[f].loops.iter().flatten() {
                    for v in b.edges[c.edge].v {
                        let g = vert_group[v];
                        if g != usize::MAX && !gs.contains(&g) {
                            gs.push(g);
                        }
                    }
                }
            }
            for g in gs {
                for &f in &touching[g] {
                    if same(p.faces[0], f) || others[pi].iter().any(|&o| same(o, f)) {
                        continue;
                    }
                    // (Opposite senses of one surface are one surface here too.)
                    let tc = tol::CONFUSION.max(tol::REL * size);
                    if b.faces[f].surf.same_domain(&p.surf, tc).is_some()
                        || others[pi]
                            .iter()
                            .any(|&o| b.faces[o].surf.same_domain(&b.faces[f].surf, tc).is_some())
                    {
                        continue;
                    }
                    others[pi].push(f);
                }
            }
        }
        for (pi, p) in pools.iter().enumerate() {
            for &o in &others[pi] {
                // The pair's curves, the same whichever of the two asks: ordered by pool where
                // both are pools.
                let so = &b.faces[o].surf;
                let swap = pool_of[o] != usize::MAX && pool_of[o] < pi;
                let (sa, sb) = if swap { (so, &p.surf) } else { (&p.surf, so) };
                for ic in ssi::surface_surface(sa, sa.param_box(), sb, sb.param_box(), &bb) {
                    if ic.t1 - ic.t0 <= 0.0 {
                        continue;
                    }
                    let (t0, t1) = match &ic.c {
                        Curve::Line { o, d } => {
                            // Over the whole of the search box, whatever part was given.
                            let m = (bb.center() - o).dot(d) / d.norm_squared();
                            let h = reach / d.norm();
                            (m - h, m + h)
                        }
                        _ => (ic.t0, ic.t1),
                    };
                    cands[pi].push(Cand {
                        c: ic.c,
                        t0,
                        t1,
                        closed: ic.closed,
                    });
                }
            }
            for &f in &p.faces {
                for c in b.faces[f].loops.iter().flatten() {
                    let e = &b.edges[c.edge];
                    if e.degenerate
                        || lost(c.edge)
                        || !ef[c.edge].iter().all(|&g| pool_of.get(g) == Some(&pi))
                        || ef[c.edge].len() < 2
                    {
                        continue;
                    }
                    // Only next to a lost coedge does a seam bound a gap.
                    if !e.v.iter().any(|&v| vert_group[v] != usize::MAX) {
                        continue;
                    }
                    // A seam is an iso-curve of its surface: run on over the surface's own
                    // range (a sphere's meridian from pole to pole, not round its back).
                    let mid = e.point(e.mid_t());
                    let (u, v) = p.surf.project(&mid, None);
                    let ((u0, u1), (v0, v1)) = p.surf.param_box();
                    let on = |c: &Curve, lo: f64, hi: f64| {
                        [0.0, 0.3, 0.7, 1.0].iter().all(|s| {
                            let q = e.point(e.t[0] + (e.t[1] - e.t[0]) * s);
                            (c.point(c.closest(&q, lo, hi)) - q).norm() < ptol
                        })
                    };
                    let iso = [
                        (p.surf.iso_u(u), (v0, v1), p.surf.v_periodic()),
                        (p.surf.iso_v(v), (u0, u1), p.surf.u_periodic()),
                    ]
                    .into_iter()
                    .find_map(|(c, (lo, hi), periodic)| {
                        let c = c?;
                        let (lo, hi) = match &c {
                            Curve::Line { d, .. } if hi - lo > 1e3 => {
                                let m = c.closest(&mid, -1e12, 1e12);
                                let h = reach / d.norm();
                                (m - h, m + h)
                            }
                            _ => (lo, hi),
                        };
                        on(&c, lo, hi).then_some(Cand {
                            c,
                            t0: lo,
                            t1: hi,
                            closed: periodic,
                        })
                    });
                    let cand = iso.unwrap_or_else(|| match &e.curve {
                        Curve::Line { d, .. } => {
                            let h = reach / d.norm();
                            Cand {
                                c: e.curve.clone(),
                                t0: e.t[0] - h,
                                t1: e.t[1] + h,
                                closed: false,
                            }
                        }
                        _ => Cand {
                            c: e.curve.clone(),
                            t0: e.t[0],
                            t1: e.t[1],
                            closed: false,
                        },
                    });
                    let dup = cands[pi].iter().any(|k| {
                        [0.25, 0.5, 0.75].iter().all(|s| {
                            let q = e.point(e.t[0] + (e.t[1] - e.t[0]) * s);
                            k.at(&q).1 < ptol
                        })
                    });
                    if !dup {
                        cands[pi].push(cand);
                    }
                }
            }
        }
    }

    // Each pool's loops again: kept runs joined by traced paths.
    struct NewLoop {
        pieces: Vec<Piece>,
        /// A face of the pool it belongs to.
        owner: usize,
        /// The faces whose kept coedges it runs along.
        from: Vec<usize>,
    }
    let old_piece = |c: &super::topo::Coedge| -> Piece {
        let e = &b.edges[c.edge];
        let (ta, tb) = if c.fwd {
            (e.t[0], e.t[1])
        } else {
            (e.t[1], e.t[0])
        };
        let (va, vb) = if c.fwd {
            (e.v[0], e.v[1])
        } else {
            (e.v[1], e.v[0])
        };
        Piece {
            c: e.curve.clone(),
            ta,
            tb,
            a: b.verts[va].p,
            b: b.verts[vb].p,
            old: Some((c.edge, c.fwd)),
            pole: false,
        }
    };
    let mut pool_loops: Vec<Vec<NewLoop>> = Vec::new();
    for (pi, p) in pools.iter().enumerate() {
        let tracer = Tracer {
            surf: &p.surf,
            sense: p.sense,
            cands: &cands[pi],
            tol: ptol,
            ahead: 1e-4 * size,
            poles: super::local::singular_points(&p.surf),
        };
        // The kept runs (a face, its pieces) and the lost runs (start, heading back into the
        // start, end), over all the pool's loops.
        let mut kept: Vec<(usize, Vec<Piece>)> = Vec::new();
        let mut runs: Vec<(P3, V3, P3)> = Vec::new();
        let mut loops: Vec<NewLoop> = Vec::new();
        for &f in &p.faces {
            for l in &b.faces[f].loops {
                let l: Vec<&super::topo::Coedge> =
                    l.iter().filter(|c| !b.edges[c.edge].degenerate).collect();
                let n = l.len();
                let is_lost: Vec<bool> = l.iter().map(|c| lost(c.edge)).collect();
                if !is_lost.contains(&true) {
                    loops.push(NewLoop {
                        pieces: l.iter().map(|c| old_piece(c)).collect(),
                        owner: f,
                        from: vec![f],
                    });
                    continue;
                }
                if is_lost.iter().all(|&x| x) {
                    // Lost whole: the far side of the cell beyond it (outside the face, else
                    // inside it), found by a ray.
                    let first = old_piece(l[0]);
                    let tm = 0.5 * (first.ta + first.tb);
                    let m = first.c.point(tm);
                    let d = first.dir(tm);
                    let nm = normal_at(&p.surf, p.sense, &m);
                    let right = d.cross(&nm);
                    let (u, v) = p.surf.project(&m, None);
                    let probes: Vec<Curve> = match &p.surf {
                        Surface::Plane { .. } => vec![Curve::Line { o: m, d: right }],
                        s => [s.iso_u(u), s.iso_v(v)].into_iter().flatten().collect(),
                    };
                    // The iso-curve heading most nearly across the loop.
                    let mut ray: Option<(f64, Cand, f64)> = None;
                    for c in probes {
                        let (t0, t1, closed) = match &c {
                            Curve::Line { d, .. } => {
                                let h = reach / d.norm();
                                (-h, h, false)
                            }
                            c if c.is_periodic() => (0.0, TAU, true),
                            c => {
                                let (a, z) = c.nurbs_range().unwrap_or((0.0, 1.0));
                                (a, z, false)
                            }
                        };
                        let cand = Cand { c, t0, t1, closed };
                        let (t, dist) = cand.at(&m);
                        if dist > ptol * 4.0 {
                            continue;
                        }
                        let along = cand.c.tangent(t).normalize().dot(&right);
                        if ray.as_ref().is_none_or(|r| along.abs() > r.0.abs()) {
                            ray = Some((along, cand, t));
                        }
                    }
                    let Some((along, ray, t)) = ray else {
                        return Err(failed("no way across a face from a deleted loop"));
                    };
                    if along.abs() < 1e-3 {
                        return Err(failed("no way across a face from a deleted loop"));
                    }
                    let mut found = None;
                    for side in [1.0, -1.0] {
                        let up = (along > 0.0) == (side > 0.0);
                        let ev = tracer.events(&ray, None, t, up, &[]);
                        let hit = ev.iter().find_map(|e| match e.2 {
                            Event::Cross(_, _) => Some(e.1),
                            _ => None,
                        });
                        let Some(hit) = hit else { continue };
                        let x = ray.c.point(hit);
                        let back = ray.c.tangent(hit).normalize() * if up { -1.0 } else { 1.0 };
                        if let Ok((pieces, _)) = tracer.trace(x, back, &[x], side) {
                            found = Some(pieces);
                            break;
                        }
                    }
                    // (Nothing either side: the loop goes and the face closes over.)
                    if let Some(pieces) = found {
                        loops.push(NewLoop {
                            pieces,
                            owner: f,
                            from: vec![f],
                        });
                    }
                    continue;
                }
                // Runs, from a kept coedge that follows a lost one.
                let s = (0..n)
                    .find(|&i| !is_lost[i] && is_lost[(i + n - 1) % n])
                    .expect("a loop with both kept and lost coedges");
                let mut i = 0;
                while i < n {
                    let k = (s + i) % n;
                    if !is_lost[k] {
                        let mut run = Vec::new();
                        while i < n && !is_lost[(s + i) % n] {
                            run.push(old_piece(l[(s + i) % n]));
                            i += 1;
                        }
                        kept.push((f, run));
                    } else {
                        let first = old_piece(l[k]);
                        let mut last = first.clone();
                        while i < n && is_lost[(s + i) % n] {
                            last = old_piece(l[(s + i) % n]);
                            i += 1;
                        }
                        // Where the lost run leaves its start for, a little along it.
                        let speed = first.c.tangent(first.ta).norm().max(tol::TINY);
                        let step = (tracer.ahead / speed).min((first.tb - first.ta).abs() * 0.5);
                        let near = first
                            .c
                            .point(first.ta + step * if first.tb >= first.ta { 1.0 } else { -1.0 });
                        let back = (near - first.a)
                            .try_normalize(tol::DEGENERATE)
                            .unwrap_or_else(|| first.dir(first.ta));
                        runs.push((first.a, back, last.b));
                    }
                }
            }
        }
        // Every lost run's path, to any run's end.
        let ends: Vec<P3> = runs.iter().map(|r| r.2).collect();
        let mut paths: Vec<(P3, Vec<Piece>, P3)> = Vec::new();
        for (start, back, _) in &runs {
            // The cell outside the face (the face grows over the gap), else the one inside it
            // (the gap takes a part of the face), where the candidates bound none outside.
            let (pieces, k) = match tracer.trace(*start, *back, &ends, 1.0) {
                Ok(grown) => grown,
                Err(e) => tracer.trace(*start, *back, &ends, -1.0).map_err(|_| e)?,
            };
            paths.push((*start, pieces, ends[k]));
        }
        // Kept runs and paths linked end to start into loops.
        let mut used_kept = vec![false; kept.len()];
        for s in 0..kept.len() {
            if used_kept[s] {
                continue;
            }
            let mut pieces: Vec<Piece> = Vec::new();
            let mut from = Vec::new();
            let mut k = s;
            loop {
                used_kept[k] = true;
                if !from.contains(&kept[k].0) {
                    from.push(kept[k].0);
                }
                pieces.extend(kept[k].1.iter().cloned());
                let end = kept[k].1.last().expect("a kept run has a coedge").b;
                let path = paths
                    .iter()
                    .find(|p| (p.0 - end).norm() < ptol * 4.0)
                    .ok_or_else(|| failed("a kept edge leads to no new boundary"))?;
                pieces.extend(path.1.iter().cloned());
                let next = (0..kept.len())
                    .find(|&j| (kept[j].1[0].a - path.2).norm() < ptol * 4.0)
                    .ok_or_else(|| failed("a new boundary leads to no kept edge"))?;
                if next == s {
                    break;
                }
                if used_kept[next] {
                    return Err(failed("the new boundaries of a face cross"));
                }
                k = next;
            }
            loops.push(NewLoop {
                pieces,
                owner: from[0],
                from,
            });
        }
        // Doubling back cancelled; loops left with nothing go.
        let mut done = Vec::new();
        for l in loops {
            if let Some(pieces) = cancel(l.pieces, ptol)? {
                done.push(NewLoop { pieces, ..l });
            }
        }
        pool_loops.push(done);
    }

    // New pieces split at every end of another that lies inside them (a rim traced whole on
    // one face and from a seam on the next), so the two faces of an edge name the same pieces.
    {
        let mut ends: Vec<P3> = Vec::new();
        for l in pool_loops.iter().flatten() {
            for p in &l.pieces {
                for q in [p.a, p.b] {
                    if !ends.iter().any(|e| (e - q).norm() < ptol * 8.0) {
                        ends.push(q);
                    }
                }
            }
        }
        for l in pool_loops.iter_mut().flatten() {
            let mut out = Vec::with_capacity(l.pieces.len());
            for p in l.pieces.drain(..) {
                if p.old.is_some() {
                    out.push(p);
                    continue;
                }
                let (lo, hi) = (p.ta.min(p.tb), p.ta.max(p.tb));
                let speed = p.c.tangent(p.ta).norm().max(tol::TINY);
                let mut cuts: Vec<(f64, P3)> = Vec::new();
                for e in &ends {
                    let t = p.c.closest(e, lo, hi);
                    if (p.c.point(t) - e).norm() < ptol * 4.0
                        && (t - lo) * speed > ptol * 8.0
                        && (hi - t) * speed > ptol * 8.0
                        && !cuts.iter().any(|c| (c.0 - t).abs() * speed < ptol * 8.0)
                    {
                        cuts.push((t, *e));
                    }
                }
                if p.tb >= p.ta {
                    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
                } else {
                    cuts.sort_by(|a, b| b.0.total_cmp(&a.0));
                }
                let mut from = (p.ta, p.a);
                for (t, e) in cuts {
                    out.push(Piece {
                        ta: from.0,
                        a: from.1,
                        tb: t,
                        b: e,
                        ..p.clone()
                    });
                    from = (t, e);
                }
                out.push(Piece {
                    ta: from.0,
                    a: from.1,
                    ..p
                });
            }
            l.pieces = out;
        }
    }

    // The new body: vertices and edges pooled by position, kept faces as they were.
    let mut xb = XBuilder::new();
    xb.trust_orientation = true;
    let mut vpos: Vec<P3> = Vec::new();
    let mut vertex = |xb: &mut XBuilder, p: &P3| -> usize {
        match vpos.iter().position(|q| (q - p).norm() < ptol * 8.0) {
            Some(i) => i,
            None => {
                vpos.push(*p);
                xb.vertex(*p)
            }
        }
    };
    let mut old_vert = vec![usize::MAX; b.verts.len()];
    let mut old_edge = vec![usize::MAX; b.edges.len()];
    // (start vertex, end vertex, middle, edge)
    let mut new_edges: Vec<(usize, usize, P3, usize)> = Vec::new();
    let mut coedge = |xb: &mut XBuilder, p: &Piece| -> Result<(usize, bool)> {
        if let Some((e, fwd)) = p.old {
            if old_edge[e] == usize::MAX {
                let ed = &b.edges[e];
                let mut v = [0; 2];
                for k in 0..2 {
                    if old_vert[ed.v[k]] == usize::MAX {
                        old_vert[ed.v[k]] = vertex(xb, &b.verts[ed.v[k]].p);
                    }
                    v[k] = old_vert[ed.v[k]];
                }
                old_edge[e] = xb
                    .body
                    .add_edge(ed.curve.clone(), ed.t[0], ed.t[1], v[0], v[1]);
                let m = ed.point(ed.mid_t());
                new_edges.push((v[0], v[1], m, old_edge[e]));
            }
            return Ok((old_edge[e], fwd));
        }
        let (va, vb) = (vertex(xb, &p.a), vertex(xb, &p.b));
        let m = p.c.point(0.5 * (p.ta + p.tb));
        for &(a, z, q, e) in &new_edges {
            if (q - m).norm() < ptol * 8.0 {
                if (a, z) == (va, vb)
                    && (va != vb || {
                        let ed = &xb.body.edges[e];
                        ed.curve
                            .tangent(ed.mid_t())
                            .dot(&p.dir(0.5 * (p.ta + p.tb)))
                            > 0.0
                    })
                {
                    return Ok((e, true));
                }
                if (a, z) == (vb, va) {
                    return Ok((e, false));
                }
            }
        }
        // Along the curve's own direction: its parameter rising from the first vertex.
        let (c, t0, t1) = if p.tb >= p.ta {
            (p.c.clone(), p.ta, p.tb)
        } else {
            let (c, t0, t1) = p.c.reversed(p.tb, p.ta);
            (c, t0, t1)
        };
        let e = xb.body.add_edge(c, t0, t1, va, vb);
        new_edges.push((va, vb, m, e));
        Ok((e, true))
    };
    let mut sources: Vec<Vec<usize>> = Vec::new();
    for fi in 0..nf {
        if is_gone[fi] || pool_of[fi] != usize::MAX {
            continue;
        }
        let f = &b.faces[fi];
        let mut loops = Vec::new();
        for l in &f.loops {
            let mut nl = Vec::new();
            for c in l.iter().filter(|c| !b.edges[c.edge].degenerate) {
                nl.push(coedge(&mut xb, &old_piece(c))?);
            }
            loops.push(nl);
        }
        xb.face(XFace {
            surf: f.surf.clone(),
            sense: f.sense,
            loops,
        });
        sources.push(vec![fi]);
    }
    for (pi, p) in pools.iter().enumerate() {
        // The pool's faces joined where a loop runs along more than one.
        let mut root: Vec<usize> = (0..p.faces.len()).collect();
        let idx = |f: usize| p.faces.iter().position(|&g| g == f).expect("a pool's face");
        use super::math::set_of as find;
        for l in &pool_loops[pi] {
            let a = find(&mut root, idx(l.from[0]));
            for &f in &l.from[1..] {
                let z = find(&mut root, idx(f));
                root[z] = a;
            }
        }
        for r in 0..p.faces.len() {
            if find(&mut root, r) != r {
                continue;
            }
            let mut loops = Vec::new();
            for l in &pool_loops[pi] {
                if find(&mut root, idx(l.owner)) != r {
                    continue;
                }
                let mut nl = Vec::new();
                for piece in &l.pieces {
                    nl.push(coedge(&mut xb, piece)?);
                }
                loops.push(nl);
            }
            // Nothing left of its boundary: the whole of a surface that closes on itself (a
            // ball whose flat went), which the builder bounds by its seam.
            if loops.is_empty() && !matches!(p.surf, Surface::Sphere { .. } | Surface::Torus { .. })
            {
                continue;
            }
            xb.face(XFace {
                surf: p.surf.clone(),
                sense: p.sense,
                loops,
            });
            let members: Vec<usize> = (0..p.faces.len())
                .filter(|&i| find(&mut root, i) == r)
                .map(|i| p.faces[i])
                .collect();
            sources.push(members);
        }
    }
    let solid = !b.solids.is_empty();
    let all: Vec<usize> = (0..xb.faces.len()).collect();
    let shell = xb.shell(all, solid);
    if solid {
        xb.solid(vec![shell]);
    }
    let (mut out, _warnings, map) = xb.finish_mapped();
    let mut from = vec![Vec::new(); out.faces.len()];
    for (i, m) in map.iter().enumerate() {
        match m {
            Some(f) if *f < from.len() => from[*f] = sources[i].clone(),
            _ => return Err(failed("a face round the deleted ones has no boundary left")),
        }
    }
    out.shells.clear();
    out.solids.clear();
    super::boolean::assemble_shells(&mut out)?;
    if solid {
        if debug() {
            eprintln!(
                "healed: {} faces, {} edges, {} solids, shells closed {:?}, warnings {_warnings:?}, check {:?}",
                out.faces.len(),
                out.edges.len(),
                out.solids.len(),
                out.shells.iter().map(|s| s.closed).collect::<Vec<_>>(),
                out.check()
            );
            for (i, f) in out.faces.iter().enumerate() {
                eprintln!(
                    "  face {i}: {:?}",
                    f.loops
                        .iter()
                        .map(|l| l
                            .iter()
                            .map(|c| (
                                c.edge,
                                c.fwd,
                                out.edges[c.edge].degenerate,
                                out.edges[c.edge].t
                            ))
                            .collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                );
            }
        }
        if out.solids.len() != 1 || out.shells.iter().any(|s| !s.closed) {
            return Err(failed(
                "the faces round the deleted ones don't close the gap",
            ));
        }
    } else {
        // A sheet stays a sheet.
        out.solids.clear();
        for s in &mut out.shells {
            s.closed = false;
        }
    }
    // Edges run on are one edge again (a seam and its piece up to the new rim).
    super::unify::merge_edges(&mut out);
    let problems = out.check();
    if !problems.is_empty() {
        return Err(failed(format!(
            "the healed body is not valid: {}",
            problems[0]
        )));
    }
    Ok((out, from))
}
