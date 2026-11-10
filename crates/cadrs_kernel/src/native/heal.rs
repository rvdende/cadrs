//! Shape healing and upgrade ([`crate::Kernel::heal`]): a body made sound and its geometry
//! brought to the kinds a downstream system wants, one [`HealOp`] after another (OCCT's
//! `ShapeFix`, `ShapeCustom` and `ShapeUpgrade`, behind DRAW's `fixshape`, `fixsmall`,
//! `fixwgaps`, `sameparameter`, `updatetolerance`, `directfaces`, `DT_ToBspl`,
//! `DT_ShapeConvertRev`, `DT_SplitAngle`, `DT_ClosedSplit`, `DT_ShapeDivide`,
//! `DT_ShapeConvert` and `unifysamedom`).
//!
//! - **Fix** ([`fix`]): loops closed (vertices of consecutive edges within the precision merged,
//!   edges shorter than it dropped), each face's outer loop first and its sense agreeing with
//!   its loops, each shell's faces oriented alike across their edges and closed shells pointing
//!   out, p-curves refitted where they stray from their edges ([`same_parameter`]), and the
//!   tolerances fitted to the gaps left, up to the largest allowed.
//! - **Small edges and gaps** ([`drop_small_edges`], [`fix_gaps`]): the vertex merging of Fix
//!   on its own.
//! - **Tolerances** ([`same_parameter`], [`update_tolerance`]).
//! - **Direct faces** ([`direct_faces`]): surfaces on mirrored frames re-expressed on direct
//!   ones (the same points; the p-curves mirrored, the face's sense flipped).
//! - **Conversions** ([`to_revolution`], [`to_bspline`]): cylinders, cones, spheres and tori
//!   as surfaces of revolution (the same parametrization), and every curved surface as an exact
//!   B-spline (its p-curves projected again).
//! - **Splits** ([`super::heal_split`]): by angle, closed faces, C0 breaks, Bézier patches.
//! - **Unify**: faces on one surface and edges on one curve merged ([`super::unify`]).
//! - **Tails** ([`super::tails`]).
//!
//! Each step keeps every face's origin, so the result's history says which input face each
//! face continues.

use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, TOL_MIN, co_range};
use crate::{BodyId, FaceId, HealOp, History, InputFace, KernelError, OpResult, Result};

/// A body being healed, with the input faces each of its faces continues.
pub struct Healing {
    /// The body.
    pub body: Body,
    /// For each face of `body`, the input faces it continues.
    pub from: Vec<Vec<usize>>,
}

impl Healing {
    /// The start: every face its own origin.
    pub fn new(body: Body) -> Self {
        let from = (0..body.faces.len()).map(|f| vec![f]).collect();
        Healing { body, from }
    }

    /// Composes a step's face map (each new face's face before the step) into `from`.
    fn mapped(&mut self, step: &[usize]) {
        self.from = step
            .iter()
            .map(|&f| self.from.get(f).cloned().unwrap_or_default())
            .collect();
    }

    /// Runs one operation.
    pub fn apply(&mut self, op: &HealOp) -> Result<()> {
        let b = &mut self.body;
        match *op {
            HealOp::Fix {
                precision,
                max_tolerance,
            } => {
                let step = fix(b, precision, max_tolerance)?;
                self.mapped(&step);
            }
            HealOp::DropSmallEdges { tolerance } => {
                let step = drop_small_edges(b, tolerance);
                self.mapped(&step);
            }
            HealOp::FixGaps { tolerance } => {
                reorder_loops(b, tolerance);
                let step = fix_gaps(b, tolerance);
                fit_edge_ends(b);
                fix_gaps_2d(b);
                b.fit_tolerances_searched(f64::INFINITY);
                self.mapped(&step);
            }
            HealOp::SameParameter { tolerance } => same_parameter(b, tolerance),
            HealOp::UpdateTolerance => update_tolerance(b),
            HealOp::DirectFaces => direct_faces(b),
            HealOp::ToRevolution => to_revolution(b),
            HealOp::ToBSpline => to_bspline(b),
            HealOp::SplitAngle { max_angle } => {
                let step = super::heal_split::split_angle(b, max_angle)?;
                self.mapped(&step);
            }
            HealOp::SplitClosed => {
                let step = super::heal_split::split_closed(b)?;
                self.mapped(&step);
            }
            HealOp::SplitContinuity => {
                let step = super::heal_split::split_continuity(b)?;
                self.mapped(&step);
            }
            HealOp::ToBezier => {
                let step = super::heal_split::split_knots(b)?;
                self.mapped(&step);
            }
            HealOp::Unify => {
                let from: Vec<Vec<(usize, usize)>> = self
                    .from
                    .iter()
                    .map(|s| s.iter().map(|&f| (0, f)).collect())
                    .collect();
                let from = super::unify::unify_faces(b, from, &|_, _| true);
                super::unify::merge_edges(b);
                let solid = !b.solids.is_empty();
                b.shells.clear();
                b.solids.clear();
                super::boolean::assemble_shells(b)?;
                if !solid {
                    b.solids.clear();
                }
                self.from = from
                    .into_iter()
                    .map(|s| s.into_iter().map(|(_, f)| f).collect())
                    .collect();
            }
            HealOp::Tails {
                max_angle,
                max_width,
            } => {
                super::tails::fix_tails(b, max_angle, max_width)?;
            }
        }
        if self.from.len() != self.body.faces.len() {
            return Err(KernelError::InvalidResult {
                op: "heal",
                why: format!(
                    "healing lost track of its faces ({} faces, {} origins)",
                    self.body.faces.len(),
                    self.from.len()
                ),
            });
        }
        Ok(())
    }
}

/// Runs `ops` on a copy of `b`, in order.
pub fn heal(b: &Body, ops: &[HealOp]) -> Result<Healing> {
    let mut h = Healing::new(b.clone());
    for op in ops {
        h.apply(op)?;
    }
    Ok(h)
}

/// [`crate::Kernel::heal`]: `ops` run on `body`, the result checked ([`Body::check`]) and its
/// history recorded.
pub fn heal_op(k: &mut super::NativeKernel, body: BodyId, ops: &[HealOp]) -> Result<OpResult> {
    let h = heal(k.get(body)?, ops)?;
    if let Some(e) = h.body.check().first() {
        return Err(KernelError::InvalidResult {
            op: "heal",
            why: format!("healing left an invalid body: {e}"),
        });
    }
    let mut history = History::default();
    let mut used = vec![false; k.get(body)?.faces.len()];
    for (fi, src) in h.from.iter().enumerate() {
        for &f in src {
            history.modified.push((
                FaceId(fi as u64),
                InputFace {
                    body,
                    face: FaceId(f as u64),
                },
            ));
            if let Some(u) = used.get_mut(f) {
                *u = true;
            }
        }
    }
    history.deleted = (0..used.len())
        .filter(|&f| !used[f])
        .map(|f| InputFace {
            body,
            face: FaceId(f as u64),
        })
        .collect();
    let id = k.insert(h.body);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// A union–find over vertices; the smallest index of a set stands for it.
struct Merge(Vec<usize>);

impl Merge {
    fn root(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) -> bool {
        let (a, b) = (self.root(a), self.root(b));
        if a == b {
            return false;
        }
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        self.0[hi] = lo;
        true
    }
}

/// Merges each set of `m` into its smallest vertex (its tolerance grown to cover the others),
/// drops the coedges of edges that `drop` marks, then loops left empty and faces left with no
/// loop. Returns each remaining face's index before.
fn apply_merge(b: &mut Body, m: &mut Merge, drop: &[bool]) -> Vec<usize> {
    let n = b.verts.len();
    for i in 0..n {
        let r = m.root(i);
        if r != i {
            let d = (b.verts[i].p - b.verts[r].p).norm() + b.verts[i].tol;
            if d > b.verts[r].tol {
                b.verts[r].tol = d;
            }
        }
    }
    for e in &mut b.edges {
        e.v = [m.root(e.v[0]), m.root(e.v[1])];
    }
    for w in &mut b.wires {
        w.edges
            .retain(|&(e, _)| !drop.get(e).copied().unwrap_or(false));
    }
    b.wires.retain(|w| !w.edges.is_empty());
    for p in &mut b.points {
        *p = m.root(*p);
    }
    let mut keep = Vec::new();
    let mut faces = Vec::new();
    for (fi, mut f) in std::mem::take(&mut b.faces).into_iter().enumerate() {
        for l in &mut f.loops {
            l.retain(|c| !drop.get(c.edge).copied().unwrap_or(false));
        }
        f.loops.retain(|l| !l.is_empty());
        if !f.loops.is_empty() {
            keep.push(fi);
            faces.push(f);
        }
    }
    b.faces = faces;
    let mut newidx = vec![None; keep.iter().copied().max().map_or(0, |x| x + 1)];
    for (k, &fi) in keep.iter().enumerate() {
        newidx[fi] = Some(k);
    }
    for s in &mut b.shells {
        s.faces = s
            .faces
            .iter()
            .filter_map(|&f| newidx.get(f).copied().flatten())
            .collect();
    }
    let gone: Vec<usize> = (0..b.shells.len())
        .filter(|&s| b.shells[s].faces.is_empty())
        .collect();
    if !gone.is_empty() {
        let mut smap = vec![None; b.shells.len()];
        let mut shells = Vec::new();
        for (si, s) in std::mem::take(&mut b.shells).into_iter().enumerate() {
            if !s.faces.is_empty() {
                smap[si] = Some(shells.len());
                shells.push(s);
            }
        }
        b.shells = shells;
        for so in &mut b.solids {
            so.shells = so.shells.iter().filter_map(|&s| smap[s]).collect();
        }
        b.solids.retain(|s| !s.shells.is_empty());
    }
    keep
}

/// Merges the vertices of consecutive coedges of a loop that don't meet but lie within
/// `tolerance` of each other (beyond their tolerances: the gap closed is recorded in the merged
/// vertex's tolerance). Returns each face's face before.
pub fn fix_gaps(b: &mut Body, tolerance: f64) -> Vec<usize> {
    let mut m = Merge((0..b.verts.len()).collect());
    let mut any = false;
    for f in &b.faces {
        for l in &f.loops {
            for k in 0..l.len() {
                let (x, y) = (b.co_end(&l[k]), b.co_start(&l[(k + 1) % l.len()]));
                if x != y
                    && (b.verts[x].p - b.verts[y].p).norm()
                        <= tolerance.max(b.verts[x].tol + b.verts[y].tol)
                {
                    any |= m.join(x, y);
                }
            }
        }
    }
    if !any {
        return (0..b.faces.len()).collect();
    }
    let drop = vec![false; b.edges.len()];
    let keep = apply_merge(b, &mut m, &drop);
    tidy(b);
    keep
}

/// The joints of a loop whose coedges don't meet at one vertex.
fn breaks(b: &Body, l: &[Coedge]) -> usize {
    (0..l.len())
        .filter(|&k| b.co_end(&l[k]) != b.co_start(&l[(k + 1) % l.len()]))
        .count()
}

/// Puts each loop's coedges in chain order (OCCT's `ShapeFix_Wire::FixReorder`): from the
/// first, the coedge starting at the vertex the chain has reached, else one ending there (run
/// the other way), else the one starting nearest it (within `tolerance`, a gap merged later).
/// Coedges left over when a chain closes make loops of their own. A loop is changed only when
/// that leaves fewer joints that don't meet.
pub fn reorder_loops(b: &mut Body, tolerance: f64) {
    for fi in 0..b.faces.len() {
        reorder_face_loops(b, fi, tolerance);
    }
}

/// [`reorder_loops`] on face `fi` alone.
pub fn reorder_face_loops(b: &mut Body, fi: usize, tolerance: f64) {
    let loops = b.faces[fi].loops.clone();
    let before: usize = loops.iter().map(|l| breaks(b, l)).sum();
    if before == 0 {
        return;
    }
    let mut rest: Vec<Coedge> = loops.into_iter().flatten().collect();
    let mut out: Vec<Vec<Coedge>> = Vec::new();
    while !rest.is_empty() {
        let mut cur = vec![rest.remove(0)];
        loop {
            let end = b.co_end(&cur[cur.len() - 1]);
            if end == b.co_start(&cur[0]) {
                break;
            }
            if let Some(i) = rest.iter().position(|c| b.co_start(c) == end) {
                cur.push(rest.remove(i));
                continue;
            }
            if let Some(i) = rest.iter().position(|c| b.co_end(c) == end) {
                let mut c = rest.remove(i);
                c.fwd = !c.fwd;
                cur.push(c);
                continue;
            }
            let p = b.verts[end].p;
            let near = rest
                .iter()
                .enumerate()
                .map(|(i, c)| (i, (b.verts[b.co_start(c)].p - p).norm()))
                .filter(|&(_, d)| d <= tolerance)
                .min_by(|x, y| x.1.total_cmp(&y.1));
            match near {
                Some((i, _)) => cur.push(rest.remove(i)),
                None => break,
            }
        }
        out.push(cur);
    }
    let after: usize = out.iter().map(|l| breaks(b, l)).sum();
    if after < before {
        b.faces[fi].loops = out;
    }
}

/// Moves the ends of edges whose curve misses its vertices onto them where the curve allows
/// it exactly: a line made the line between its vertices, a B-spline's end pole moved to the
/// vertex (its other poles stay). The p-curves of a moved edge are projected again.
pub fn fit_edge_ends(b: &mut Body) {
    let mut moved = Vec::new();
    for ei in 0..b.edges.len() {
        let e = &b.edges[ei];
        if e.degenerate {
            continue;
        }
        let (p0, p1) = (b.verts[e.v[0]].p, b.verts[e.v[1]].p);
        let off = |k: usize, p: &P3| (e.point(e.t[k]) - p).norm();
        let (d0, d1) = (off(0, &p0), off(1, &p1));
        let allowed = e.tol.max(super::tol::POINT);
        if d0 <= allowed && d1 <= allowed {
            continue;
        }
        let curve = match &e.curve {
            Curve::Line { .. } if (e.t[1] - e.t[0]).abs() > 1e-300 && (p1 - p0).norm() > 0.0 => {
                let d = (p1 - p0) / (e.t[1] - e.t[0]);
                Some(Curve::Line {
                    o: p0 - d * e.t[0],
                    d,
                })
            }
            Curve::Nurbs(n) => {
                let (lo, hi) = n.range();
                let mut n = (**n).clone();
                let mut ok = true;
                for (k, p, at, idx) in [(0, p0, lo, 0), (1, p1, hi, n.cps.len() - 1)] {
                    if off(k, &p) <= allowed {
                        continue;
                    }
                    if (e.t[k] - at).abs() > tol::REL_FINE * (hi - lo) {
                        ok = false;
                        continue;
                    }
                    let w = n.cps[idx].w;
                    n.cps[idx] = super::nurbs::hom(&p, w);
                }
                ok.then(|| Curve::Nurbs(std::sync::Arc::new(n)))
            }
            _ => None,
        };
        if let Some(c) = curve {
            b.edges[ei].curve = c;
            moved.push(ei);
        }
    }
    if moved.is_empty() {
        return;
    }
    for f in &mut b.faces {
        for c in f.loops.iter_mut().flatten() {
            if !moved.contains(&c.edge) {
                continue;
            }
            let e = &b.edges[c.edge];
            let near = c.pc.point(e.mid_t());
            c.pc = super::pcurve::pcurve(
                &f.surf,
                &e.curve,
                e.t[0],
                e.t[1],
                Some(P2::new(near.x, near.y)),
            );
        }
    }
}

/// Closes gaps in (u, v) between consecutive coedges that meet at a vertex (OCCT's
/// `FixGaps2d`): a p-curve whose end isn't on the vertex is projected from its edge again, and
/// on a periodic surface each p-curve is shifted by whole periods to start where the one before
/// ends.
pub fn fix_gaps_2d(b: &mut Body) {
    for fi in 0..b.faces.len() {
        for li in 0..b.faces[fi].loops.len() {
            let n = b.faces[fi].loops[li].len();
            for k in 0..n {
                let f = &b.faces[fi];
                let c = &f.loops[li][k];
                let e = &b.edges[c.edge];
                if e.degenerate {
                    continue;
                }
                let tol = |v: usize| b.verts[v].tol.max(e.tol).max(super::tol::LOOP_GAP);
                let miss = (0..2).any(|j| {
                    let uv = c.pc.point(e.t[j]);
                    (f.surf.eval(uv.x, uv.y) - b.verts[e.v[j]].p).norm() > tol(e.v[j])
                });
                if !miss {
                    continue;
                }
                let near = c.pc.point(e.mid_t());
                let pc = super::pcurve::pcurve(
                    &f.surf,
                    &e.curve,
                    e.t[0],
                    e.t[1],
                    Some(P2::new(near.x, near.y)),
                );
                let better = Coedge { pc, ..c.clone() };
                if pc_gap(b, &f.surf, &better) < pc_gap(b, &f.surf, c)
                    || pc_gap(b, &f.surf, &better) <= e.tol.max(super::tol::POINT)
                {
                    b.faces[fi].loops[li][k] = better;
                }
            }
            // Whole periods: each p-curve starting where the one before ends.
            let surf = b.faces[fi].surf.clone();
            let periods = super::pcurve::closed_periods(&surf);
            if periods == [None, None] {
                continue;
            }
            for k in 1..n {
                let (prev, cur) = (&b.faces[fi].loops[li][k - 1], &b.faces[fi].loops[li][k]);
                let (pe, ce) = (&b.edges[prev.edge], &b.edges[cur.edge]);
                let a = prev.pc.point(co_range(pe, prev.fwd).1);
                let z = cur.pc.point(co_range(ce, cur.fwd).0);
                let step = |d: f64, per: Option<f64>| per.map_or(0.0, |p| (d / p).round() * p);
                let (du, dv) = (step(a.x - z.x, periods[0]), step(a.y - z.y, periods[1]));
                if du != 0.0 || dv != 0.0 {
                    let pc = cur
                        .pc
                        .transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                    b.faces[fi].loops[li][k].pc = pc;
                }
            }
        }
    }
}

/// Closes the (u, v) gaps at degenerate edges (a cone's apex, a sphere's pole): there the
/// surface's whole iso-line is one point, so the edges meeting it may arrive at any u, and the
/// degenerate edge's p-curve, a line along it, is set to run from where the coedge before ends
/// to where the one after starts. Only a p-curve whose ends move by less than half a radian of
/// the surface's parameter (a run across the apex kept, not turned round it the other way).
/// Returns how many it set.
pub fn bridge_degenerate(b: &mut Body) -> usize {
    let mut set = 0;
    for fi in 0..b.faces.len() {
        for li in 0..b.faces[fi].loops.len() {
            let n = b.faces[fi].loops[li].len();
            if n < 3 {
                continue;
            }
            for k in 0..n {
                let l = &b.faces[fi].loops[li];
                let c = &l[k];
                let e = &b.edges[c.edge];
                if !e.degenerate {
                    continue;
                }
                let (prev, next) = (&l[(k + n - 1) % n], &l[(k + 1) % n]);
                let (pe, ne) = (&b.edges[prev.edge], &b.edges[next.edge]);
                if pe.degenerate || ne.degenerate {
                    continue;
                }
                let a = prev.pc.point(co_range(pe, prev.fwd).1);
                let z = next.pc.point(co_range(ne, next.fwd).0);
                let (t0, t1) = co_range(e, c.fwd);
                if (t1 - t0).abs() <= tol::ROUNDOFF {
                    continue;
                }
                let (s, w) = (c.pc.point(t0), c.pc.point(t1));
                let moved = (a - s).norm().max((z - w).norm());
                if moved <= tol::ROUNDOFF_FINE || moved >= 0.5 {
                    continue;
                }
                let d = (z - a) / (t1 - t0);
                b.faces[fi].loops[li][k].pc = Curve::Line { o: a - d * t0, d };
                set += 1;
            }
        }
    }
    set
}

/// Adds the seam a face on a surface closed in u is missing (OCCT's
/// `ShapeFix_Face::FixMissingSeam`): a face bounded by two loops that each go once round the
/// surface (each closes in (u, v) only a period apart, as two rims of a cylinder do) becomes
/// one loop: the first rim, a seam edge up the iso-line u = const through the rims' first
/// vertices, the second rim back, and the seam down. Only when both rims start at the same u.
pub fn fix_missing_seams(b: &mut Body) {
    for fi in 0..b.faces.len() {
        let f = &b.faces[fi];
        let Some(per) = super::pcurve::closed_periods(&f.surf)[0] else {
            continue;
        };
        if f.loops.len() != 2 {
            continue;
        }
        // Each loop's u travel: ±per when it goes round.
        let travel = |l: &[Coedge]| -> f64 {
            l.iter()
                .map(|c| {
                    let e = &b.edges[c.edge];
                    let (t0, t1) = co_range(e, c.fwd);
                    c.pc.point(t1).x - c.pc.point(t0).x
                })
                .sum()
        };
        let (ta, tb) = (travel(&f.loops[0]), travel(&f.loops[1]));
        if !((ta.abs() - per).abs() < tol::REL_POINT * per
            && (tb.abs() - per).abs() < tol::REL_POINT * per
            && ta * tb < 0.0)
        {
            continue;
        }
        let start = |l: &[Coedge]| {
            let e = &b.edges[l[0].edge];
            (b.co_start(&l[0]), l[0].pc.point(co_range(e, l[0].fwd).0))
        };
        let ((va, pa), (vb, pb)) = (start(&f.loops[0]), start(&f.loops[1]));
        let k = ((pa.x - pb.x) / per).round();
        if (pa.x - pb.x - k * per).abs() > tol::REL * per || va == vb {
            continue;
        }
        let u = pa.x;
        let Some(curve) = super::heal_split::iso_curve(&f.surf, super::heal_split::Dir::U, u)
        else {
            continue;
        };
        // Loop a runs in u by ta; the seam goes from a's start (after a's run, at u + ta) to
        // b's start on that side, and back down at u.
        let (lo, hi) = (pa.y.min(pb.y), pa.y.max(pb.y));
        if hi - lo < tol::ROUNDOFF_FINE {
            continue;
        }
        let (v0, v1) = if pa.y < pb.y { (va, vb) } else { (vb, va) };
        b.edges.push(super::topo::Edge {
            curve,
            t: [lo, hi],
            v: [v0, v1],
            degenerate: false,
            tol: TOL_MIN,
        });
        let se = b.edges.len() - 1;
        let line = |uu: f64| Curve::Line {
            o: P3::new(uu, 0.0, 0.0),
            d: V3::y(),
        };
        let up = pa.y < pb.y;
        // After loop a (ending at its start vertex, at u + ta): seam a → b; loop b shifted to
        // start there; then seam b → a at u.
        let shift = |c: &Coedge, du: f64| Coedge {
            pc: c
                .pc
                .transformed(&Affine::translation(V3::new(du, 0.0, 0.0))),
            ..c.clone()
        };
        let la = f.loops[0].clone();
        let lb: Vec<Coedge> = f.loops[1].iter().map(|c| shift(c, u + ta - pb.x)).collect();
        let mut l = la;
        l.push(Coedge {
            edge: se,
            fwd: up,
            pc: line(u + ta),
        });
        l.extend(lb);
        l.push(Coedge {
            edge: se,
            fwd: !up,
            pc: line(u),
        });
        b.faces[fi].loops = vec![l];
    }
}

/// Drops the edges shorter than `tolerance` that aren't closed curves (a seam's or pole's
/// edge stays), merging their ends; loops and faces left empty go. Returns each face's face
/// before.
pub fn drop_small_edges(b: &mut Body, tolerance: f64) -> Vec<usize> {
    let mut m = Merge((0..b.verts.len()).collect());
    let mut drop = vec![false; b.edges.len()];
    // An edge used twice by one face is a seam: it stays whatever its length.
    let mut seam = vec![false; b.edges.len()];
    for f in &b.faces {
        let mut seen = std::collections::HashSet::new();
        for c in f.loops.iter().flatten() {
            if !seen.insert(c.edge) {
                seam[c.edge] = true;
            }
        }
    }
    for (ei, e) in b.edges.iter().enumerate() {
        if e.degenerate || seam[ei] || e.closed() {
            continue;
        }
        let len = e.curve.length(e.t[0].min(e.t[1]), e.t[0].max(e.t[1]));
        if len <= tolerance {
            drop[ei] = true;
            m.join(e.v[0], e.v[1]);
        }
    }
    if !drop.iter().any(|&d| d) {
        return (0..b.faces.len()).collect();
    }
    let keep = apply_merge(b, &mut m, &drop);
    tidy(b);
    keep
}

/// Drops edges and vertices nothing uses any more (a body with wires or points keeps its
/// indices as they are).
fn tidy(b: &mut Body) {
    if b.wires.is_empty() && b.points.is_empty() {
        super::unify::compact(b);
    }
}

/// The largest distance between a coedge's p-curve on its face and its edge, from 33 samples.
fn pc_gap(b: &Body, surf: &Surface, c: &Coedge) -> f64 {
    let e = &b.edges[c.edge];
    (0..=32)
        .map(|i| {
            let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 32.0;
            let uv = c.pc.point(t);
            let d = (surf.eval(uv.x, uv.y) - e.point(t)).norm();
            if d.is_nan() { f64::INFINITY } else { d }
        })
        .fold(0.0, f64::max)
}

/// Refits each p-curve that strays from its edge by more than the edge's tolerance (and
/// `tolerance`) by projecting the edge onto the face again, when that strays less; then fits
/// the tolerances to the gaps left (`fit_tolerances_searched`).
pub fn same_parameter(b: &mut Body, tolerance: f64) {
    refit_pcurves(b, tolerance);
    b.fit_tolerances_searched(f64::INFINITY);
    cover_loop_gaps(b);
}

/// The refitting of [`same_parameter`] alone: each p-curve that strays from its edge by more
/// than the edge's tolerance (and `tolerance`) projected again where that strays less. The
/// tolerances are left as they are.
pub fn refit_pcurves(b: &mut Body, tolerance: f64) {
    for fi in 0..b.faces.len() {
        for li in 0..b.faces[fi].loops.len() {
            for k in 0..b.faces[fi].loops[li].len() {
                let f = &b.faces[fi];
                let c = &f.loops[li][k];
                let e = &b.edges[c.edge];
                if e.degenerate {
                    continue;
                }
                let gap = pc_gap(b, &f.surf, c);
                if gap <= e.tol.max(tolerance) {
                    continue;
                }
                let near = c.pc.point(e.mid_t());
                let pc = super::pcurve::pcurve(
                    &f.surf,
                    &e.curve,
                    e.t[0],
                    e.t[1],
                    Some(P2::new(near.x, near.y)),
                );
                let better = Coedge { pc, ..c.clone() };
                if pc_gap(b, &f.surf, &better) < gap {
                    b.faces[fi].loops[li][k] = better;
                }
            }
        }
    }
}

/// Raises each vertex's tolerance to cover the gaps its loops leave in (u, v) where one coedge
/// ends and the next starts there (measured as the check measures them: scaled by the
/// surface's metric).
fn cover_loop_gaps(b: &mut Body) {
    let mut need = vec![0.0f64; b.verts.len()];
    for f in &b.faces {
        let (mu, mv) = f.surf.metric();
        for l in &f.loops {
            for k in 0..l.len() {
                let (x, y) = (&l[k], &l[(k + 1) % l.len()]);
                let (ex, ey) = (&b.edges[x.edge], &b.edges[y.edge]);
                let (p, q) = (
                    x.pc.point(co_range(ex, x.fwd).1),
                    y.pc.point(co_range(ey, y.fwd).0),
                );
                let d = ((p.x - q.x).abs() * mu).max((p.y - q.y).abs() * mv);
                let v = b.co_end(x);
                if d.is_finite() && d > need[v] {
                    need[v] = d;
                }
            }
        }
    }
    for (v, d) in b.verts.iter_mut().zip(need) {
        if d * 1.01 > v.tol && d > super::tol::LOOP_GAP {
            v.tol = d * 1.01;
        }
    }
}

/// Sets every tolerance to what the body's gaps need: lowered to the least as well as raised.
pub fn update_tolerance(b: &mut Body) {
    for v in &mut b.verts {
        v.tol = TOL_MIN;
    }
    for e in &mut b.edges {
        e.tol = TOL_MIN;
    }
    b.fit_tolerances_searched(f64::INFINITY);
    cover_loop_gaps(b);
    // A vertex covers its edges' tolerances where they meet it (as OCCT's vertex tolerance
    // covers its edges').
    for ei in 0..b.edges.len() {
        let (t, v) = (b.edges[ei].tol, b.edges[ei].v);
        for k in v {
            if b.verts[k].tol < t {
                b.verts[k].tol = t;
            }
        }
    }
}

/// Whether a surface lies on a mirrored frame (OCCT's indirect elementary surface).
pub fn indirect(s: &Surface) -> bool {
    match s {
        Surface::Plane { f }
        | Surface::Cylinder { f, .. }
        | Surface::Cone { f, .. }
        | Surface::Sphere { f, .. }
        | Surface::Torus { f, .. } => !f.right_handed(),
        _ => false,
    }
}

/// Re-expresses each surface on a mirrored frame on a direct one: the frame's y reversed, which
/// mirrors the parameters (v of a plane, the angle u of the others: u → 2π − u), so the
/// p-curves are mirrored with it and the face's sense flips (its outward side stays).
pub fn direct_faces(b: &mut Body) {
    for f in &mut b.faces {
        if !indirect(&f.surf) {
            continue;
        }
        let plane = matches!(f.surf, Surface::Plane { .. });
        let flip = |fr: &mut Frame| fr.y = -fr.y;
        match &mut f.surf {
            Surface::Plane { f }
            | Surface::Cylinder { f, .. }
            | Surface::Cone { f, .. }
            | Surface::Sphere { f, .. }
            | Surface::Torus { f, .. } => flip(f),
            _ => continue,
        }
        let m = if plane {
            Affine {
                m: nalgebra::Matrix3::from_diagonal(&V3::new(1.0, -1.0, 1.0)),
                t: V3::zeros(),
            }
        } else {
            Affine {
                m: nalgebra::Matrix3::from_diagonal(&V3::new(-1.0, 1.0, 1.0)),
                t: V3::new(TAU, 0.0, 0.0),
            }
        };
        for c in f.loops.iter_mut().flatten() {
            c.pc = c.pc.transformed(&m);
        }
        f.sense = !f.sense;
    }
}

/// Cylinders, cones, spheres and tori as surfaces of revolution of their meridian (OCCT's
/// `ShapeCustom::ConvertToRevolution`): the same points at the same parameters, so the
/// p-curves stay.
pub fn to_revolution(b: &mut Body) {
    for f in &mut b.faces {
        if let Some(s) = as_revolution(&f.surf) {
            f.surf = s;
        }
    }
}

/// The surface of revolution a cylinder, cone, sphere or torus is (parametrized alike).
pub fn as_revolution(s: &Surface) -> Option<Surface> {
    let (f, m) = match s {
        Surface::Cylinder { f, r } => (
            f,
            Curve::Line {
                o: f.o + f.x * *r,
                d: f.z,
            },
        ),
        Surface::Cone { f, r, a } => (
            f,
            Curve::Line {
                o: f.o + f.x * *r,
                d: f.x * a.sin() + f.z * a.cos(),
            },
        ),
        Surface::Sphere { f, r } => (
            f,
            Curve::Circle {
                c: f.o,
                x: f.x,
                y: f.z,
                r: *r,
            },
        ),
        Surface::Torus { f, rmaj, rmin } => (
            f,
            Curve::Circle {
                c: f.o + f.x * *rmaj,
                x: f.x,
                y: f.z,
                r: *rmin,
            },
        ),
        _ => return None,
    };
    Some(Surface::Revolution { f: *f, m })
}

/// Every surface but planes (and B-splines already) as an exact B-spline over its face's
/// (u, v) box (OCCT's `ShapeCustom::ConvertToBSpline` after `ConvertToRevolution`): the
/// p-curves are projected onto the new surface; a face whose edges don't lie on it within
/// their tolerances keeps its surface.
pub fn to_bspline(b: &mut Body) {
    for fi in 0..b.faces.len() {
        if matches!(b.faces[fi].surf, Surface::Plane { .. } | Surface::Nurbs(_)) {
            continue;
        }
        let Some(((u0, u1), (v0, v1))) = super::heal_split::face_uv_box(b, fi) else {
            continue;
        };
        if !(u1 > u0 && v1 > v0) {
            continue;
        }
        let ns = Surface::Nurbs(std::sync::Arc::new(
            b.faces[fi].surf.to_nurbs((u0, u1), (v0, v1)),
        ));
        if let Some(loops) = reproject(b, fi, &ns) {
            b.faces[fi].surf = ns;
            b.faces[fi].loops = loops;
        }
    }
    b.fit_tolerances_searched(f64::INFINITY);
}

/// Face `fi`'s loops with p-curves on surface `s` (projected; a pole's p-curve the line between
/// its neighbours' ends), or `None` when an edge doesn't lie on `s` within its tolerance and a
/// part in 10⁶ of the face's size.
fn reproject(b: &Body, fi: usize, s: &Surface) -> Option<Vec<Vec<Coedge>>> {
    let f = &b.faces[fi];
    let size = b.face_bbox(fi).diagonal().max(tol::ROUNDOFF);
    let mut loops = Vec::new();
    for l in &f.loops {
        let mut out: Vec<Coedge> = Vec::new();
        for c in l {
            let e = &b.edges[c.edge];
            if e.degenerate {
                out.push(c.clone());
                continue;
            }
            let near = c.pc.point(e.mid_t());
            let pc =
                super::pcurve::pcurve(s, &e.curve, e.t[0], e.t[1], Some(P2::new(near.x, near.y)));
            let nc = Coedge { pc, ..c.clone() };
            if !(pc_gap(b, s, &nc) <= e.tol.max(tol::REL_POINT * size)) {
                return None;
            }
            out.push(nc);
        }
        // Poles: from the end of the coedge before to the start of the one after.
        let n = out.len();
        for k in 0..n {
            let e = &b.edges[out[k].edge];
            if !e.degenerate {
                continue;
            }
            let (prev, next) = (&out[(k + n - 1) % n], &out[(k + 1) % n]);
            let (pe, ne) = (&b.edges[prev.edge], &b.edges[next.edge]);
            let a = prev.pc.point(co_range(pe, prev.fwd).1);
            let z = next.pc.point(co_range(ne, next.fwd).0);
            let (ts, te) = co_range(e, out[k].fwd);
            if (te - ts).abs() < 1e-300 {
                continue;
            }
            let d = (z - a) / (te - ts);
            out[k].pc = Curve::Line { o: a - d * ts, d };
        }
        loops.push(out);
    }
    Some(loops)
}

/// Fix (OCCT's `ShapeFix_Shape` at `precision`, tolerances up to `max_tolerance`): see the
/// module docs. Returns each face's face before.
pub fn fix(b: &mut Body, precision: f64, max_tolerance: f64) -> Result<Vec<usize>> {
    let precision = precision.max(TOL_MIN);
    let max_tolerance = max_tolerance.max(precision);
    // Wires: small edges dropped, gaps closed.
    let keep1 = drop_small_edges(b, precision);
    reorder_loops(b, max_tolerance);
    let keep2 = fix_gaps(b, max_tolerance);
    fit_edge_ends(b);
    fix_gaps_2d(b);
    fix_missing_seams(b);
    let step: Vec<usize> = keep2.iter().map(|&k| keep1[k]).collect();
    // Faces: the outer loop first (the largest in (u, v)), and the sense its loops give.
    for f in &mut b.faces {
        if f.loops.len() < 2 && !f.loops.is_empty() {
            continue;
        }
        let areas: Vec<f64> = f.loops.iter().map(|l| loop_area(&b.edges, l)).collect();
        if let Some(outer) =
            (0..areas.len()).max_by(|&x, &y| areas[x].abs().total_cmp(&areas[y].abs()))
            && outer != 0
        {
            f.loops.swap(0, outer);
        }
    }
    for fi in 0..b.faces.len() {
        let l = &b.faces[fi].loops;
        if l.is_empty() {
            continue;
        }
        let outer = loop_area(&b.edges, &l[0]);
        let holes_agree = l[1..].iter().all(|h| loop_area(&b.edges, h) * outer <= 0.0);
        // Loops that all run against the face's sense: the sense is what's wrong.
        if outer.abs() > 0.0 && holes_agree && (outer > 0.0) != b.faces[fi].sense {
            b.faces[fi].sense = !b.faces[fi].sense;
        }
    }
    orient_shells(b);
    same_parameter(b, precision);
    b.fit_tolerances_searched(max_tolerance);
    Ok(step)
}

/// The signed area in (u, v) of a loop (dense samples of its p-curves).
fn loop_area(edges: &[super::topo::Edge], l: &[Coedge]) -> f64 {
    let mut pts: Vec<P2> = Vec::new();
    for c in l {
        let e = &edges[c.edge];
        let (a, z) = co_range(e, c.fwd);
        let n = if matches!(c.pc, Curve::Line { .. }) {
            1
        } else {
            32
        };
        for i in 0..n {
            let q = c.pc.point(a + (z - a) * i as f64 / n as f64);
            pts.push(P2::new(q.x, q.y));
        }
    }
    polygon_area(&pts)
}

/// Orients each shell's faces alike: from its first face, a neighbour using a shared edge the
/// same way is reversed; a closed shell of a solid enclosing a negative volume is turned
/// inside out (a void shell, after the first, the other way). Closed flags follow the edge
/// uses.
fn orient_shells(b: &mut Body) {
    for si in 0..b.shells.len() {
        let faces = b.shells[si].faces.clone();
        if faces.is_empty() {
            continue;
        }
        let mut by_edge: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        for &f in &faces {
            for c in b.faces[f].loops.iter().flatten() {
                by_edge.entry(c.edge).or_default().push(f);
            }
        }
        let uses = |b: &Body, f: usize, e: usize| -> Vec<bool> {
            b.faces[f]
                .loops
                .iter()
                .flatten()
                .filter(|c| c.edge == e)
                .map(|c| c.fwd)
                .collect()
        };
        let mut done: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for &start in &faces {
            if !done.insert(start) {
                continue;
            }
            let mut stack = vec![start];
            while let Some(f) = stack.pop() {
                let edges: Vec<usize> = b.faces[f].loops.iter().flatten().map(|c| c.edge).collect();
                for e in edges {
                    if b.edges[e].degenerate {
                        continue;
                    }
                    let Some(nbrs) = by_edge.get(&e) else {
                        continue;
                    };
                    if nbrs.len() != 2 || nbrs[0] == nbrs[1] {
                        continue;
                    }
                    let g = if nbrs[0] == f { nbrs[1] } else { nbrs[0] };
                    if done.contains(&g) {
                        continue;
                    }
                    let (uf, ug) = (uses(b, f, e), uses(b, g, e));
                    if uf.len() == 1 && ug.len() == 1 && uf[0] == ug[0] {
                        super::topo::Body::reverse_face(&mut b.faces[g]);
                    }
                    done.insert(g);
                    stack.push(g);
                }
            }
        }
        // Closed: each edge used once each way.
        let mut count: std::collections::HashMap<usize, (u32, u32)> =
            std::collections::HashMap::new();
        for &f in &faces {
            for c in b.faces[f].loops.iter().flatten() {
                let u = count.entry(c.edge).or_default();
                if c.fwd {
                    u.0 += 1;
                } else {
                    u.1 += 1;
                }
            }
        }
        b.shells[si].closed = count
            .iter()
            .all(|(&e, &u)| u == (1, 1) || b.edges[e].degenerate);
    }
    for so in 0..b.solids.len() {
        let shells = b.solids[so].shells.clone();
        for (k, &si) in shells.iter().enumerate() {
            if !b.shells[si].closed {
                continue;
            }
            let faces = b.shells[si].faces.clone();
            let sub = super::kernel::subset(b, &faces, true);
            let v = super::mass::body_volume(&sub);
            // The outer shell encloses a positive volume, a void a negative one.
            if (k == 0 && v < 0.0) || (k > 0 && v > 0.0) {
                for f in faces {
                    super::topo::Body::reverse_face(&mut b.faces[f]);
                }
            }
        }
    }
}

/// The number of faces on a mirrored frame (OCCT's statshape "Indirect Surface").
pub fn count_indirect(b: &Body) -> usize {
    b.faces.iter().filter(|f| indirect(&f.surf)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::NativeKernel;
    use crate::native::topo::CheckLevel;

    fn body_of(k: &NativeKernel, s: crate::native::draw::Shape) -> Body {
        match s {
            crate::native::draw::Shape::Body(id) => (**k.get(id).unwrap()).clone(),
            _ => unreachable!(),
        }
    }

    fn valid(b: &Body) {
        let e = b.check_with(CheckLevel::Full);
        assert!(e.is_empty(), "{e:?}");
    }

    fn volume(b: &Body) -> f64 {
        crate::native::mass::body_volume(b)
    }

    /// A cylinder (a whole turn) split by angle, split closed, made revolution and B-spline:
    /// valid every time, the same volume, the faces as expected.
    #[test]
    fn upgrades_of_a_cylinder() {
        let mut k = NativeKernel::new();
        let s = k.pcylinder(2.0, 5.0).unwrap();
        let b = body_of(&k, s);
        let v0 = volume(&b);
        valid(&b);
        let ncurved = |b: &Body| {
            b.faces
                .iter()
                .filter(|f| !matches!(f.surf, Surface::Plane { .. }))
                .count()
        };
        for (op, curved) in [
            (
                HealOp::SplitAngle {
                    max_angle: 95f64.to_radians(),
                },
                4,
            ),
            (HealOp::SplitClosed, 2),
            (HealOp::ToRevolution, 1),
            (HealOp::ToBSpline, 1),
        ] {
            let h = heal(&b, &[op]).unwrap();
            valid(&h.body);
            assert_eq!(ncurved(&h.body), curved, "{op:?}");
            assert!(
                (volume(&h.body) - v0).abs() < 1e-6 * v0,
                "{op:?}: {} vs {v0}",
                volume(&h.body)
            );
            assert_eq!(h.from.len(), h.body.faces.len());
        }
    }

    /// A sphere split by angle and closed: the poles' edges split with it.
    #[test]
    fn upgrades_of_a_sphere() {
        let mut k = NativeKernel::new();
        let s = k.psphere(3.0).unwrap();
        let b = body_of(&k, s);
        let v0 = volume(&b);
        for op in [
            HealOp::SplitAngle { max_angle: 1.0 },
            HealOp::SplitClosed,
            HealOp::ToBSpline,
        ] {
            let h = heal(&b, &[op]).unwrap();
            valid(&h.body);
            assert!(
                (volume(&h.body) - v0).abs() < 1e-6 * v0,
                "{op:?}: {} vs {v0}",
                volume(&h.body)
            );
        }
    }

    /// A box mirrored (its frames left-handed) made direct: no indirect faces left, the body
    /// unchanged in volume and valid.
    #[test]
    fn direct_faces_of_a_mirrored_box() {
        let mut k = NativeKernel::new();
        let s = k.pcylinder(1.0, 2.0).unwrap();
        let m = Affine {
            m: nalgebra::Matrix3::from_diagonal(&V3::new(1.0, -1.0, 1.0)),
            t: V3::zeros(),
        };
        let b = body_of(&k, s).transformed(&m);
        assert!(count_indirect(&b) > 0);
        valid(&b);
        let h = heal(&b, &[HealOp::DirectFaces]).unwrap();
        assert_eq!(count_indirect(&h.body), 0);
        valid(&h.body);
        assert!((volume(&h.body) - volume(&b)).abs() < 1e-9);
    }

    /// Fix and the tolerance steps leave a sound body as it is (in volume and validity).
    #[test]
    fn fix_keeps_a_sound_body() {
        let mut k = NativeKernel::new();
        let s = k.pcylinder(2.0, 5.0).unwrap();
        let b = body_of(&k, s);
        let h = heal(
            &b,
            &[
                HealOp::Fix {
                    precision: 1e-3,
                    max_tolerance: 5e-3,
                },
                HealOp::SameParameter { tolerance: 1e-3 },
                HealOp::UpdateTolerance,
            ],
        )
        .unwrap();
        valid(&h.body);
        assert_eq!(h.body.faces.len(), b.faces.len());
        assert!((volume(&h.body) - volume(&b)).abs() < 1e-9);
    }
}
