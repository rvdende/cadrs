//! Sketch profiles as exact 2D pieces: validation, orientation (outer loops counter-clockwise,
//! holes clockwise) and the merging of regions that share boundary curves (so touching
//! regions extrude as one body with one cap).

use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::NurbsCurve;
use super::tol;
use crate::{Curve2, KernelError, Profile, Result, unsourced_curve};

/// A piece of a loop: the curve (z = 0) between `t0 < t1`, run backwards when `rev`.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub c: Curve,
    pub t0: f64,
    pub t1: f64,
    pub rev: bool,
    /// What the side face it sweeps is named after.
    pub src: Src,
    /// Further origins when pieces of several regions were fused into this one.
    pub more: Vec<Src>,
}

/// The origin of a profile piece: a sketch curve of a region, or an edge of a body (a face used
/// as a profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Src {
    Curve { region: u64, curve: u64 },
    Edge { body: u64, edge: u64 },
}

impl Src {
    pub fn origin(&self) -> crate::Origin {
        match *self {
            Src::Curve { region, curve } => crate::Origin::ProfileCurve { region, curve },
            Src::Edge { body, edge } => crate::Origin::FromEdge {
                body: crate::BodyId(body),
                edge: crate::EdgeId(edge),
            },
        }
    }
    pub fn region(&self) -> Option<u64> {
        match *self {
            Src::Curve { region, .. } => Some(region),
            Src::Edge { .. } => None,
        }
    }
}

impl Piece {
    pub fn ts(&self) -> f64 {
        if self.rev { self.t1 } else { self.t0 }
    }
    pub fn te(&self) -> f64 {
        if self.rev { self.t0 } else { self.t1 }
    }
    pub fn start(&self) -> P2 {
        let p = self.c.point(self.ts());
        P2::new(p.x, p.y)
    }
    pub fn end(&self) -> P2 {
        let p = self.c.point(self.te());
        P2::new(p.x, p.y)
    }
    /// The point at fraction s along the direction of travel.
    pub fn at(&self, s: f64) -> P2 {
        let t = self.ts() + (self.te() - self.ts()) * s;
        let p = self.c.point(t);
        P2::new(p.x, p.y)
    }
    /// Unit tangent in the direction of travel at fraction s.
    pub fn dir(&self, s: f64) -> V2 {
        let t = self.ts() + (self.te() - self.ts()) * s;
        let d = self.c.tangent(t);
        let d = V2::new(d.x, d.y) * if self.rev { -1.0 } else { 1.0 };
        let l = d.norm();
        if l > 0.0 { d / l } else { d }
    }
    pub fn length(&self) -> f64 {
        self.c.length(self.t0, self.t1)
    }
    pub fn reversed(&self) -> Piece {
        Piece {
            rev: !self.rev,
            ..self.clone()
        }
    }
    /// ∮ x dy along the direction of travel (Green: the signed area of a closed loop).
    pub fn x_dy(&self) -> f64 {
        let ts = self.c.pieces(self.t0, self.t1);
        let mut s = 0.0;
        for w in ts.windows(2) {
            s += integrate(w[0], w[1], 2, &GAUSS16, |t| {
                let (p, d, _) = self.c.d2(t);
                p.x * d.y
            });
        }
        if self.rev { -s } else { s }
    }
    /// Points along the piece (both ends included), at most `defl` from it.
    pub fn sample(&self, defl: f64) -> Vec<P2> {
        let ts = self.c.sample_params(self.t0, self.t1, defl, 0.2);
        let mut pts: Vec<P2> = ts
            .iter()
            .map(|&t| {
                let p = self.c.point(t);
                P2::new(p.x, p.y)
            })
            .collect();
        if self.rev {
            pts.reverse();
        }
        pts
    }
    /// The same carrier curve (so two pieces can overlap).
    pub fn same_carrier(&self, o: &Piece, tol: f64) -> bool {
        match (&self.c, &o.c) {
            (Curve::Line { o: a, d: da }, Curve::Line { o: b, d: db }) => {
                let cr = da.x * db.y - da.y * db.x;
                if cr.abs() > tol::ANGULAR {
                    return false;
                }
                let w = b - a;
                (w.x * da.y - w.y * da.x).abs() < tol
            }
            (Curve::Circle { c: a, r: ra, .. }, Curve::Circle { c: b, r: rb, .. }) => {
                (a - b).norm() < tol && (ra - rb).abs() < tol
            }
            (a, b) => a == b,
        }
    }
    /// The parameter on the carrier of a point on it.
    pub fn param_of(&self, p: &P2) -> f64 {
        let q = P3::new(p.x, p.y, 0.0);
        match &self.c {
            Curve::Circle { c, x, y, .. } => {
                let v = q - c;
                let a = v.dot(y).atan2(v.dot(x));
                unwrap_into(a, self.t0, self.t1)
            }
            Curve::Ellipse { .. } => {
                let t = self.c.closest(&q, self.t0 - TAU, self.t1 + TAU);
                unwrap_into(t, self.t0, self.t1)
            }
            _ => self.c.closest(&q, self.t0 - 1e9, self.t1 + 1e9),
        }
    }
    /// Splits at carrier parameters strictly inside, keeping the direction of travel.
    pub fn split_at(&self, mut ts: Vec<f64>) -> Vec<Piece> {
        ts.retain(|t| *t > self.t0 && *t < self.t1);
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < tol::ROUNDOFF_FINE);
        let mut bounds = vec![self.t0];
        bounds.extend(ts);
        bounds.push(self.t1);
        let mut out: Vec<Piece> = bounds
            .windows(2)
            .map(|w| Piece {
                t0: w[0],
                t1: w[1],
                ..self.clone()
            })
            .collect();
        if self.rev {
            out.reverse();
        }
        out
    }
}

/// `a` shifted by whole turns towards [t0, t1].
fn unwrap_into(a: f64, t0: f64, t1: f64) -> f64 {
    let m = (t0 + t1) * 0.5;
    unwrap_near(a, m, TAU)
}

pub type Loop2 = Vec<Piece>;

#[derive(Debug, Clone)]
pub struct Region2 {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
    pub source: u64,
}

/// A planar face to build: an outer loop, holes and the regions it covers.
#[derive(Debug, Clone)]
pub struct MergedFace {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
    pub regions: Vec<u64>,
}

pub fn loop_area(l: &[Piece]) -> f64 {
    l.iter().map(|p| p.x_dy()).sum()
}

pub fn reverse_loop(l: &[Piece]) -> Loop2 {
    l.iter().rev().map(|p| p.reversed()).collect()
}

/// A sampled polygon of the loop.
pub fn loop_polygon(l: &[Piece], defl: f64) -> Vec<P2> {
    let mut out = Vec::new();
    for p in l {
        let s = p.sample(defl);
        out.extend_from_slice(&s[..s.len() - 1]);
    }
    out
}

/// A 2D curve from a sketch curve, with the piece's range and direction.
pub fn piece_of(c: &Curve2, src: Src) -> Result<Piece> {
    let e2 = |v: V2| V3::new(v.x, v.y, 0.0);
    let p3 = |p: P2| P3::new(p.x, p.y, 0.0);
    let arc = |c: Curve, start: f64, sweep: f64| {
        if sweep >= 0.0 {
            Piece {
                c,
                t0: start,
                t1: start + sweep,
                rev: false,
                src,
                more: vec![],
            }
        } else {
            Piece {
                c,
                t0: start + sweep,
                t1: start,
                rev: true,
                src,
                more: vec![],
            }
        }
    };
    let ellipse = |center: P2, major: f64, minor: f64, rotation: f64| {
        let (s, co) = rotation.sin_cos();
        let x = V2::new(co, s) * major.signum();
        let y = V2::new(-s, co) * minor.signum();
        Curve::Ellipse {
            c: p3(center),
            x: e2(x),
            y: e2(y),
            a: major.abs(),
            b: minor.abs(),
        }
    };
    let piece = match *c {
        Curve2::Line { a, b, .. } => {
            let d = b - a;
            let l = d.norm();
            if l < TOL {
                return Err(KernelError::InvalidProfile(
                    "a line shorter than the modelling tolerance".into(),
                ));
            }
            Piece {
                c: Curve::Line {
                    o: p3(a),
                    d: e2(d / l),
                },
                t0: 0.0,
                t1: l,
                rev: false,
                src,
                more: vec![],
            }
        }
        Curve2::Arc {
            center,
            radius,
            start_angle,
            sweep,
            ..
        } => {
            if radius < TOL || (radius * sweep).abs() < TOL {
                return Err(KernelError::InvalidProfile("a degenerate arc".into()));
            }
            arc(
                Curve::Circle {
                    c: p3(center),
                    x: V3::x(),
                    y: V3::y(),
                    r: radius,
                },
                start_angle,
                sweep,
            )
        }
        Curve2::Circle { center, radius, .. } => {
            // A negative radius runs clockwise.
            if radius.abs() < TOL {
                return Err(KernelError::InvalidProfile("a degenerate circle".into()));
            }
            Piece {
                c: Curve::Circle {
                    c: p3(center),
                    x: V3::x(),
                    y: V3::y(),
                    r: radius.abs(),
                },
                t0: 0.0,
                t1: TAU,
                rev: radius < 0.0,
                src,
                more: vec![],
            }
        }
        Curve2::Ellipse {
            center,
            major_radius,
            minor_radius,
            rotation,
            ..
        } => {
            if major_radius.abs() < TOL || minor_radius.abs() < TOL {
                return Err(KernelError::InvalidProfile("a degenerate ellipse".into()));
            }
            Piece {
                c: ellipse(center, major_radius, minor_radius, rotation),
                t0: 0.0,
                t1: TAU,
                rev: false,
                src,
                more: vec![],
            }
        }
        Curve2::EllipseArc {
            center,
            major_radius,
            minor_radius,
            rotation,
            start,
            sweep,
            ..
        } => {
            if major_radius.abs() < TOL
                || minor_radius.abs() < TOL
                || sweep.abs() < tol::ROUNDOFF_FINE
            {
                return Err(KernelError::InvalidProfile(
                    "a degenerate ellipse arc".into(),
                ));
            }
            arc(
                ellipse(center, major_radius, minor_radius, rotation),
                start,
                sweep,
            )
        }
        Curve2::OffsetEllipseArc {
            center,
            major_radius,
            minor_radius,
            rotation,
            start,
            sweep,
            offset,
            ..
        } => {
            let (a, b) = (major_radius.abs(), minor_radius.abs());
            let rmin = a.min(b).powi(2) / a.max(b);
            if offset <= -rmin + tol::ROUNDOFF {
                return Err(KernelError::InvalidParameter(format!(
                    "The offset {:.3} reaches past the ellipse's smallest radius of curvature ({:.3})",
                    -offset, rmin
                )));
            }
            let n = offset_ellipse_nurbs(
                center,
                major_radius,
                minor_radius,
                rotation,
                start,
                sweep,
                offset,
            );
            if sweep >= 0.0 {
                Piece {
                    c: Curve::Nurbs(Arc::new(n)),
                    t0: start,
                    t1: start + sweep,
                    rev: false,
                    src,
                    more: vec![],
                }
            } else {
                Piece {
                    c: Curve::Nurbs(Arc::new(n)),
                    t0: start + sweep,
                    t1: start,
                    rev: true,
                    src,
                    more: vec![],
                }
            }
        }
        Curve2::Bezier { poles, .. } => {
            let n = NurbsCurve::bezier(&poles.map(p3), &[1.0; 4]);
            Piece {
                c: Curve::Nurbs(Arc::new(n)),
                t0: 0.0,
                t1: 1.0,
                rev: false,
                src,
                more: vec![],
            }
        }
        Curve2::Spline {
            degree,
            ref knots,
            ref poles,
            ref weights,
            ..
        } => {
            let n = crate::types::spline_nurbs(degree, knots, poles, weights).ok_or_else(|| {
                KernelError::InvalidProfile(
                    "a spline's knots, poles and weights don't make a B-spline".into(),
                )
            })?;
            let (t0, t1) = n.range();
            // (An exact line or conic stays one, so its faces are planes and quadrics.)
            let c = crate::native::recognize::curve(&n).unwrap_or(Curve::Nurbs(Arc::new(n)));
            let (t0, t1) = match &c {
                Curve::Nurbs(_) => (t0, t1),
                other => {
                    let (a, b) = (
                        curve_point(knots, poles, weights, degree, 0.0),
                        curve_point(knots, poles, weights, degree, 1.0),
                    );
                    match crate::native::xbuild::edge_range(other, &a, &b, (a - b).norm() < 1e-12) {
                        Some(r) => r,
                        None => (t0, t1),
                    }
                }
            };
            Piece {
                c,
                t0,
                t1,
                rev: false,
                src,
                more: vec![],
            }
        }
    };
    Ok(piece)
}

/// A spline's point at `s` in 0..=1 of its domain, in the plane z = 0.
fn curve_point(
    knots: &[f64],
    poles: &[nalgebra::Point2<f64>],
    weights: &[f64],
    degree: usize,
    s: f64,
) -> P3 {
    let q = Curve2::Spline {
        degree,
        knots: knots.to_vec(),
        poles: poles.to_vec(),
        weights: weights.to_vec(),
        source: None,
    }
    .point_at(s);
    P3::new(q.x, q.y, 0.0)
}

/// The offset ellipse as a C2 cubic B-spline interpolating the exact curve at the ellipse
/// parameter (the knots are the ellipse angles, so the B-spline's t is the ellipse's).
fn offset_ellipse_nurbs(
    center: P2,
    major: f64,
    minor: f64,
    rotation: f64,
    start: f64,
    sweep: f64,
    offset: f64,
) -> NurbsCurve {
    let (lo, hi) = if sweep >= 0.0 {
        (start, start + sweep)
    } else {
        (start + sweep, start)
    };
    let n = ((hi - lo).abs() / TAU * 1024.0).ceil().max(16.0) as usize;
    let ts: Vec<f64> = (0..=n)
        .map(|i| lo + (hi - lo) * i as f64 / n as f64)
        .collect();
    let pts: Vec<P3> = ts
        .iter()
        .map(|&t| {
            let q = crate::offset_ellipse_point(center, major, minor, rotation, offset, t);
            P3::new(q.x, q.y, 0.0)
        })
        .collect();
    // End derivatives by central differences of the exact curve.
    let h = tol::DIFF_STEP;
    let d = |t: f64| {
        let a = crate::offset_ellipse_point(center, major, minor, rotation, offset, t - h);
        let b = crate::offset_ellipse_point(center, major, minor, rotation, offset, t + h);
        V3::new(b.x - a.x, b.y - a.y, 0.0) / (2.0 * h)
    };
    NurbsCurve::interpolate(&pts, &ts, 3, Some(d(lo)), Some(d(hi)))
}

/// Converts and validates the profile's regions: closed loops, outer counter-clockwise, holes
/// clockwise.
pub fn regions(profile: &Profile) -> Result<Vec<Region2>> {
    let mut out = Vec::new();
    let snap = profile_snap(profile);
    for (ri, r) in profile.regions.iter().enumerate() {
        let source = r.source.unwrap_or(ri as u64);
        let lp = |l: &crate::Loop, li: usize| -> Result<Loop2> {
            let mut pieces = Vec::new();
            for (ci, c) in l.curves.iter().enumerate() {
                let cs = c.source().unwrap_or(unsourced_curve(li, ci));
                pieces.push(piece_of(
                    c,
                    Src::Curve {
                        region: source,
                        curve: cs,
                    },
                )?);
            }
            closed_loop(pieces, snap)
        };
        let mut outer = lp(&r.outer, 0)?;
        let a = loop_area(&outer);
        if a.abs() < 1e-10 {
            return Err(KernelError::InvalidProfile("a region with no area".into()));
        }
        if a < 0.0 {
            outer = reverse_loop(&outer);
        }
        let mut holes = Vec::new();
        for (hi, h) in r.holes.iter().enumerate() {
            let mut h = lp(h, hi + 1)?;
            let a = loop_area(&h);
            if a.abs() < 1e-10 {
                return Err(KernelError::InvalidProfile("a hole with no area".into()));
            }
            if a > 0.0 {
                h = reverse_loop(&h);
            }
            holes.push(h);
        }
        let outer = outer.iter().map(along_travel).collect();
        let holes = holes
            .iter()
            .map(|h| h.iter().map(along_travel).collect())
            .collect();
        out.push(Region2 {
            outer,
            holes,
            source,
        });
    }
    Ok(out)
}

/// A line piece on a line that runs the way the loop does (so its edges do: a straight edge
/// used as a direction points the way the loop runs, outer loops counter-clockwise about the
/// plane's normal, as OCCT's profile edges did); other pieces as they are.
fn along_travel(p: &Piece) -> Piece {
    if !p.rev || !matches!(p.c, Curve::Line { .. }) {
        return p.clone();
    }
    let (s, e) = (p.start(), p.end());
    let (c, l) = Curve::line(P3::new(s.x, s.y, 0.0), P3::new(e.x, e.y, 0.0));
    Piece {
        c,
        t0: 0.0,
        t1: l,
        rev: false,
        ..p.clone()
    }
}

/// Joints within this distance count as joined.
pub const JOIN: f64 = 1e-5;

/// The largest gap between consecutive sketch curves that [`closed_loop`] closes, in mm, for a
/// sketch of up to 100 mm. A sketch merges curve ends closer than [`SKETCH_REL`] of its size
/// into one point when it finds its regions, so its loops can arrive with gaps of that order
/// (OCCT, which built main's profiles, joined each curve to the next whatever the gap).
pub const SNAP: f64 = 1e-3;

/// The sketch's own point-merging distance as a fraction of its size (cadrs_sketch's regions).
pub const SKETCH_REL: f64 = 1e-5;

/// A gap is closed only when it is under this fraction of the length of each curve it moves,
/// so snapping never turns or swallows a curve.
const SNAP_SHARE: f64 = 0.25;

/// How far a joint may move when two curves are extended or trimmed to where they cross, as a
/// multiple of the gap (beyond it the crossing is some other point of the carriers).
const MEET_REACH: f64 = 4.0;

/// The gap [`closed_loop`] closes for this profile: [`SNAP`], or [`SKETCH_REL`] of the
/// profile's size when that is larger.
fn profile_snap(profile: &Profile) -> f64 {
    let mut bb = Bbox::empty();
    for r in &profile.regions {
        for c in std::iter::once(&r.outer)
            .chain(r.holes.iter())
            .flat_map(|l| l.curves.iter())
        {
            for p in [c.start(), c.end()] {
                bb.add(&P3::new(p.x, p.y, 0.0));
            }
        }
    }
    let size = if bb.min.x <= bb.max.x {
        bb.diagonal()
    } else {
        0.0
    };
    SNAP.max(SKETCH_REL * size)
}

/// Orders a sketch loop's pieces head to tail ([`chain_loop`]) and closes the gaps up to `snap`
/// between consecutive pieces ([`close_gaps`]): the loop a sketch's region solver found, whose
/// curve ends it merged within its own tolerance.
pub fn closed_loop(pieces: Vec<Piece>, snap: f64) -> Result<Loop2> {
    let mut l = chain_loop_within(pieces, snap)?;
    close_gaps(&mut l, snap)?;
    check_closed(&l)?;
    Ok(l)
}

/// Closes each gap between consecutive pieces wider than [`JOIN`] and at most `snap`: the two
/// carriers extended or trimmed to where they cross near the gap (so lines stay on their lines
/// and arcs on their circles), or, where they don't cross there (tangent curves), one piece
/// moved onto the other's end: a line's end, a circular arc through its own start, middle and
/// the new end, or a B-spline's end pole. Wider gaps are left (the loop is open).
pub fn close_gaps(l: &mut [Piece], snap: f64) -> Result<()> {
    let n = l.len();
    if n < 2 {
        return Ok(());
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let gap = (l[i].end() - l[j].start()).norm();
        if gap <= JOIN || gap > snap {
            continue;
        }
        if let Some((a, b)) = meet(&l[i], &l[j], gap) {
            l[i] = a;
            l[j] = b;
            continue;
        }
        // Move a line before a curve, else the shorter piece.
        let (ea, sb) = (l[i].end(), l[j].start());
        let line_first = |p: &Piece| (!matches!(p.c, Curve::Line { .. }), p.length());
        let mut order = [(i, false, sb), (j, true, ea)];
        order.sort_by(|x, y| {
            let (kx, ky) = (line_first(&l[x.0]), line_first(&l[y.0]));
            kx.0.cmp(&ky.0).then(kx.1.total_cmp(&ky.1))
        });
        for (k, at_start, q) in order {
            if gap > SNAP_SHARE * l[k].length() {
                continue;
            }
            if let Some(p) = moved_end(&l[k], at_start, q) {
                l[k] = p;
                break;
            }
        }
    }
    Ok(())
}

/// Pieces `a` and `b`, which leave a gap between `a`'s end and `b`'s start, extended or trimmed
/// to where their carriers cross near the gap (Newton on `a(s) = b(t)` from the gap's ends).
/// None when they don't cross there: tangent, or the crossing is far or off a B-spline's range.
fn meet(a: &Piece, b: &Piece, gap: f64) -> Option<(Piece, Piece)> {
    const ITERATIONS: usize = 32;
    let (mut s, mut t) = (a.te(), b.ts());
    let at = |c: &Curve, u: f64| {
        let (p, d, _) = c.d2(u);
        (P2::new(p.x, p.y), V2::new(d.x, d.y))
    };
    let mut converged = false;
    for _ in 0..ITERATIONS {
        let ((pa, da), (pb, db)) = (at(&a.c, s), at(&b.c, t));
        let r = pa - pb;
        if r.norm() < tol::CONFUSION {
            converged = true;
            break;
        }
        // [da, −db]·(Δs, Δt) = −r.
        let det = cross2(&db, &da);
        if det.abs() <= tol::ANGULAR * da.norm() * db.norm() || !det.is_finite() {
            return None;
        }
        s += cross2(&r, &db) / det;
        t -= cross2(&da, &r) / det;
    }
    if !converged {
        return None;
    }
    let q = at(&a.c, s).0;
    let reach = MEET_REACH * gap;
    if (q - a.end()).norm() > reach || (q - b.start()).norm() > reach {
        return None;
    }
    let a2 = with_end(a, false, s)?;
    let b2 = with_end(b, true, t)?;
    (gap <= SNAP_SHARE * a2.length().min(b2.length())).then_some((a2, b2))
}

/// The piece with its start (`at_start`) or end in the direction of travel at carrier
/// parameter `u`; None when that empties it, passes a whole turn or leaves a B-spline's range.
fn with_end(p: &Piece, at_start: bool, u: f64) -> Option<Piece> {
    let mut q = p.clone();
    // The start of travel is t1 for a reversed piece.
    if at_start != p.rev {
        q.t0 = u
    } else {
        q.t1 = u
    }
    if q.t1 - q.t0 <= tol::ROUNDOFF || p.c.is_periodic() && q.t1 - q.t0 > TAU + tol::ANGULAR {
        return None;
    }
    if let Some((lo, hi)) = p.c.nurbs_range()
        && (q.t0 < lo || q.t1 > hi)
    {
        return None;
    }
    Some(q)
}

/// The piece with its start (`at_start`) or end moved to `to`: a line between its other end
/// and `to`, a circular arc through its other end, its middle and `to`, or a clamped B-spline
/// with its end pole moved. None for other curves, or an arc that would turn the other way.
fn moved_end(p: &Piece, at_start: bool, to: P2) -> Option<Piece> {
    let (s, e) = if at_start {
        (to, p.end())
    } else {
        (p.start(), to)
    };
    let p3 = |q: P2| P3::new(q.x, q.y, 0.0);
    match &p.c {
        Curve::Line { .. } => {
            let (c, l) = Curve::line(p3(s), p3(e));
            (l > tol::ROUNDOFF).then(|| Piece {
                c,
                t0: 0.0,
                t1: l,
                rev: false,
                ..p.clone()
            })
        }
        Curve::Circle { .. } => {
            let m = p.at(0.5);
            // The circle through s, m and e: its centre is equidistant from the three.
            let (u, v) = (m - s, e - s);
            let den = 2.0 * cross2(&u, &v);
            if den.abs() <= tol::TINY {
                return None;
            }
            let (uu, vv) = (u.norm_squared(), v.norm_squared());
            let c = s + V2::new(v.y * uu - u.y * vv, u.x * vv - v.x * uu) / den;
            let r = (s - c).norm();
            let ang = |q: P2| (q.y - c.y).atan2(q.x - c.x);
            // Counter-clockwise when the turn from s through m to e is to the left.
            let ccw = den > 0.0;
            let (a0, a1) = if ccw {
                (ang(s), ang(e))
            } else {
                (ang(e), ang(s))
            };
            let sweep = (a1 - a0).rem_euclid(TAU);
            let sweep = if sweep <= tol::ANGULAR { TAU } else { sweep };
            if (sweep - (p.t1 - p.t0)).abs() > SNAP_SHARE {
                return None;
            }
            let c = Curve::Circle {
                c: p3(c),
                x: V3::x(),
                y: V3::y(),
                r,
            };
            Some(Piece {
                c,
                t0: a0,
                t1: a0 + sweep,
                rev: !ccw,
                ..p.clone()
            })
        }
        Curve::Nurbs(n) => {
            let (lo, hi) = n.range();
            let first = at_start != p.rev;
            let k = if first { 0 } else { n.cps.len() - 1 };
            let clamped = if first {
                p.t0 == lo && n.knots[0] == lo
            } else {
                p.t1 == hi && n.knots[n.knots.len() - 1] == hi
            };
            if !clamped {
                return None;
            }
            let mut m = (**n).clone();
            let w = m.cps[k].w;
            m.cps[k] = super::nurbs::H::new(to.x * w, to.y * w, 0.0, w);
            Some(Piece {
                c: Curve::Nurbs(Arc::new(m)),
                ..p.clone()
            })
        }
        Curve::Ellipse { .. } => None,
    }
}

/// The widest gap [`regions`] closes between consecutive curves of the profile's loops (0 when
/// they all join within [`JOIN`]).
pub fn closed_gap(profile: &Profile) -> f64 {
    let snap = profile_snap(profile);
    let mut widest: f64 = 0.0;
    for (ri, r) in profile.regions.iter().enumerate() {
        let source = r.source.unwrap_or(ri as u64);
        for (li, l) in std::iter::once(&r.outer).chain(r.holes.iter()).enumerate() {
            let pieces: Result<Vec<Piece>> = l
                .curves
                .iter()
                .enumerate()
                .map(|(ci, c)| {
                    piece_of(
                        c,
                        Src::Curve {
                            region: source,
                            curve: c.source().unwrap_or(unsourced_curve(li, ci)),
                        },
                    )
                })
                .collect();
            let Ok(chain) = pieces.and_then(|p| chain_loop_within(p, snap)) else {
                continue;
            };
            for (k, a) in chain.iter().enumerate() {
                let gap = (a.end() - chain[(k + 1) % chain.len()].start()).norm();
                if gap <= snap {
                    widest = widest.max(gap);
                }
            }
        }
    }
    widest
}

/// Raises the tolerance of a body's edges and vertices to `gap` (see [`closed_gap`]).
pub fn carry_gap(b: &mut super::topo::Body, gap: f64) {
    for e in &mut b.edges {
        e.tol = e.tol.max(gap);
    }
    for v in &mut b.verts {
        v.tol = v.tol.max(gap);
    }
}

pub fn check_closed(l: &[Piece]) -> Result<()> {
    if l.is_empty() {
        return Err(KernelError::InvalidProfile("an empty loop".into()));
    }
    // Reorder so each piece starts where the previous ends (sketch loops may list curves in
    // any direction): flip pieces as needed, in order.
    for k in 0..l.len() {
        let (a, b) = (&l[k], &l[(k + 1) % l.len()]);
        if (a.end() - b.start()).norm() > JOIN {
            return Err(KernelError::InvalidProfile(format!(
                "the loop is not closed: a gap of {:.3e} mm",
                (a.end() - b.start()).norm()
            )));
        }
    }
    Ok(())
}

/// Puts a loop's pieces in order and direction so each starts where the previous ends (sketch
/// loops list their curves in their own directions).
pub fn chain_loop(pieces: Vec<Piece>) -> Result<Loop2> {
    chain_loop_within(pieces, JOIN)
}

/// [`chain_loop`], joining a piece to the next within [`JOIN`], or else to the nearest piece end
/// within `snap` (the gap [`close_gaps`] then closes).
fn chain_loop_within(mut pieces: Vec<Piece>, snap: f64) -> Result<Loop2> {
    if pieces.len() <= 1 {
        return Ok(pieces);
    }
    let mut out = vec![pieces.remove(0)];
    // The first piece's direction: keep it unless its end meets nothing but its start does.
    while !pieces.is_empty() {
        let Some(end) = out.last().map(Piece::end) else {
            break;
        };
        let mut found = None;
        for (i, p) in pieces.iter().enumerate() {
            if (p.start() - end).norm() < JOIN {
                found = Some((i, false));
                break;
            }
            if (p.end() - end).norm() < JOIN {
                found = Some((i, true));
                break;
            }
        }
        if found.is_none() {
            found = nearest_end(&pieces, &end, snap);
        }
        match found {
            Some((i, flip)) => {
                let p = pieces.remove(i);
                out.push(if flip { p.reversed() } else { p });
            }
            None => {
                if out.len() == 1 {
                    // Try the first piece the other way round.
                    out[0] = out[0].reversed();
                    let end = out[0].end();
                    if nearest_end(&pieces, &end, snap.max(JOIN)).is_none() {
                        return Err(KernelError::InvalidProfile("the loop is not closed".into()));
                    }
                    continue;
                }
                return Err(KernelError::InvalidProfile("the loop is not closed".into()));
            }
        }
    }
    Ok(out)
}

/// The piece with an end nearest `p` within `within`, and whether it is its end (so the piece
/// runs reversed to start there).
fn nearest_end(pieces: &[Piece], p: &P2, within: f64) -> Option<(usize, bool)> {
    pieces
        .iter()
        .enumerate()
        .flat_map(|(i, q)| {
            [
                ((q.start() - p).norm(), (i, false)),
                ((q.end() - p).norm(), (i, true)),
            ]
        })
        .filter(|(d, _)| *d < within)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, f)| f)
}

/// Merges regions that share boundary pieces into faces (one cap per connected group).
pub fn merge_regions(regions: &[Region2]) -> Result<Vec<MergedFace>> {
    let separate = || -> Vec<MergedFace> {
        regions
            .iter()
            .map(|r| MergedFace {
                outer: r.outer.clone(),
                holes: r.holes.clone(),
                regions: vec![r.source],
            })
            .collect()
    };
    if regions.len() < 2 {
        return Ok(separate());
    }
    // Regions whose interiors overlap aren't a partition: leave them separate (the caller unites
    // their solids).
    if regions_overlap(regions) {
        return Ok(separate());
    }
    // All loops with their owner.
    let mut loops: Vec<Loop2> = Vec::new();
    for r in regions {
        loops.push(r.outer.clone());
        loops.extend(r.holes.iter().cloned());
    }
    let size = {
        let mut bb = Bbox::empty();
        for l in &loops {
            for p in l {
                let q = p.start();
                bb.add(&P3::new(q.x, q.y, 0.0));
            }
        }
        bb.diagonal().max(1.0)
    };
    let tol = JOIN.max(size * 1e-10);
    // Split every piece at the vertices of other loops lying on it.
    let verts: Vec<(usize, P2)> = loops
        .iter()
        .enumerate()
        .flat_map(|(li, l)| l.iter().map(move |p| (li, p.start())))
        .collect();
    let mut pieces: Vec<(usize, Piece)> = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        for p in l {
            let mut ts = Vec::new();
            // (Only a vertex near the piece's box can lie on it: no search along the piece for
            // each of a spline profile's hundreds of vertices.)
            let near = piece_box(p).map(|b| b.grow(tol));
            for (lj, v) in &verts {
                if *lj == li || near.is_some_and(|b| !in_box(&b, v)) {
                    continue;
                }
                let t = p.param_of(v);
                if t > p.t0 + tol::ROUNDOFF && t < p.t1 - tol::ROUNDOFF {
                    let q = p.c.point(t);
                    if (P2::new(q.x, q.y) - v).norm() < tol {
                        ts.push(t);
                    }
                }
            }
            for s in p.split_at(ts) {
                pieces.push((li, s));
            }
        }
    }
    // Cancel coincident pieces run in opposite directions.
    let mut dead = vec![false; pieces.len()];
    let mut any = false;
    for i in 0..pieces.len() {
        if dead[i] {
            continue;
        }
        for j in i + 1..pieces.len() {
            if dead[j] || pieces[i].0 == pieces[j].0 {
                continue;
            }
            let (a, b) = (&pieces[i].1, &pieces[j].1);
            if (a.start() - b.end()).norm() < tol
                && (a.end() - b.start()).norm() < tol
                && (a.at(0.5) - b.at(0.5)).norm() < tol
                && a.same_carrier(b, tol)
            {
                dead[i] = true;
                dead[j] = true;
                any = true;
                break;
            }
        }
    }
    if !any {
        return Ok(separate());
    }
    let live: Vec<Piece> = pieces
        .into_iter()
        .zip(dead)
        .filter(|(_, d)| !d)
        .map(|((_, p), _)| p)
        .collect();
    // Trace loops, turning as far left as possible at junctions.
    let mut used = vec![false; live.len()];
    let mut traced: Vec<Loop2> = Vec::new();
    for s in 0..live.len() {
        if used[s] {
            continue;
        }
        let mut lp = vec![live[s].clone()];
        used[s] = true;
        let start = live[s].start();
        while let Some(cur) = lp.last() {
            let end = cur.end();
            if (end - start).norm() < tol {
                break;
            }
            let din = cur.dir(1.0);
            let mut best: Option<(f64, usize)> = None;
            for (k, q) in live.iter().enumerate() {
                if used[k] || (q.start() - end).norm() >= tol {
                    continue;
                }
                let dout = q.dir(0.0);
                let ang = cross2(&din, &dout).atan2(din.dot(&dout));
                if best.is_none_or(|(b, _)| ang > b) {
                    best = Some((ang, k));
                }
            }
            match best {
                Some((_, k)) => {
                    used[k] = true;
                    lp.push(live[k].clone());
                }
                None => {
                    return Err(KernelError::InvalidProfile(
                        "the regions' boundaries don't close after merging".into(),
                    ));
                }
            }
            if lp.len() > live.len() {
                return Err(KernelError::InvalidProfile(
                    "the regions' boundaries don't close after merging".into(),
                ));
            }
        }
        traced.push(share_curve_names(fuse_pieces(lp, tol, false)));
    }
    // Outer loops and holes.
    let mut outers: Vec<(Loop2, Vec<P2>, f64)> = Vec::new();
    let mut holes: Vec<Loop2> = Vec::new();
    for l in traced {
        let a = loop_area(&l);
        if a > 0.0 {
            let poly = loop_polygon(&l, size * 1e-4);
            outers.push((l, poly, a));
        } else {
            holes.push(l);
        }
    }
    let mut faces: Vec<MergedFace> = outers
        .iter()
        .map(|(l, _, _)| MergedFace {
            outer: l.clone(),
            holes: vec![],
            regions: vec![],
        })
        .collect();
    for h in holes {
        let q = h[0].at(0.5);
        let best = outers
            .iter()
            .enumerate()
            .filter(|(_, (_, poly, _))| point_in_polygon(&q, poly))
            .min_by(|a, b| a.1.2.total_cmp(&b.1.2))
            .map(|(i, _)| i);
        match best {
            Some(i) => faces[i].holes.push(h),
            None => {
                return Err(KernelError::InvalidProfile(
                    "a hole outside every region".into(),
                ));
            }
        }
    }
    // Which face each region went into: a point just inside its outer loop.
    for r in regions {
        let p = &r.outer[0];
        let d = p.dir(0.5);
        let len = p.length();
        let q = p.at(0.5) + V2::new(-d.y, d.x) * (len * 1e-3).min(size * 1e-5).max(1e-7);
        let mut found = None;
        for (i, (_, poly, _)) in outers.iter().enumerate() {
            if point_in_polygon(&q, poly) {
                let in_hole = faces[i]
                    .holes
                    .iter()
                    .any(|h| point_in_polygon(&q, &loop_polygon(h, size * 1e-4)));
                if !in_hole {
                    found = Some(i);
                    break;
                }
            }
        }
        let i = found.unwrap_or(0);
        faces[i].regions.push(r.source);
    }
    Ok(faces)
}

/// A merged loop with the pieces of one sketch curve that meet across where two regions joined
/// (a spline split at a junction of their boundaries) named alike: each takes the others'
/// origins too, so the faces they sweep take one name, as a curve's pieces within one region
/// do (a spline's spans), and show as one face.
fn share_curve_names(mut lp: Loop2) -> Loop2 {
    let n = lp.len();
    if n < 2 {
        return lp;
    }
    let curve = |p: &Piece| match p.src {
        Src::Curve { curve, .. } => Some(curve),
        Src::Edge { .. } => None,
    };
    // Runs of adjacent pieces of one curve, round the loop.
    let mut class: Vec<usize> = (0..n).collect();
    for i in 0..n {
        let j = (i + 1) % n;
        if curve(&lp[i]).is_some() && curve(&lp[i]) == curve(&lp[j]) {
            let (a, b) = (class[i], class[j]);
            for c in &mut class {
                if *c == b {
                    *c = a;
                }
            }
        }
    }
    let srcs: Vec<Vec<Src>> = (0..n)
        .map(|i| {
            let mut out: Vec<Src> = Vec::new();
            for k in (0..n).filter(|&k| class[k] == class[i]) {
                for s in std::iter::once(lp[k].src).chain(lp[k].more.iter().copied()) {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
            out
        })
        .collect();
    for (p, s) in lp.iter_mut().zip(srcs) {
        for o in s {
            if o != p.src && !p.more.contains(&o) {
                p.more.push(o);
            }
        }
    }
    lp
}

/// The faces with each loop's pieces that continue each other on one line fused into one (a
/// feature's profile: the app's OCCT kernel merged the faces they swept, on one surface,
/// after every operation; a DRAW-level prism keeps them apart, as OCCT's does).
pub fn fuse_sides(faces: Vec<MergedFace>) -> Vec<MergedFace> {
    faces
        .into_iter()
        .map(|f| MergedFace {
            outer: fuse_pieces(f.outer, JOIN, true),
            holes: f
                .holes
                .into_iter()
                .map(|h| fuse_pieces(h, JOIN, true))
                .collect(),
            regions: f.regions,
        })
        .collect()
}

/// Fuses consecutive pieces (of one curve, of one region's curves or of touching regions') that
/// continue each other on one line, or on one circle unless `lines_only`: OCCT merged the faces they sweep into one after
/// every operation (and Parasolid merges the side faces of touching regions). The fused piece
/// keeps the first piece's origin and lists the others in `more`.
fn fuse_pieces(mut lp: Loop2, tol: f64, lines_only: bool) -> Loop2 {
    let mut changed = true;
    while changed && lp.len() > 1 {
        changed = false;
        let n = lp.len();
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (&lp[i], &lp[j]);
            // A feature's lines of one region (`lines_only`), else pieces of touching regions.
            let apart = if lines_only {
                a.src == b.src || !matches!(a.c, Curve::Line { .. })
            } else {
                a.src.region() == b.src.region()
            };
            if apart || !a.same_carrier(b, tol) || a.rev != b.rev {
                continue;
            }
            if a.dir(1.0).dot(&b.dir(0.0)) < 1.0 - tol::ANGULAR {
                continue;
            }
            let mut more = a.more.clone();
            for o in std::iter::once(b.src).chain(b.more.iter().copied()) {
                if o != a.src && !more.contains(&o) {
                    more.push(o);
                }
            }
            let fused = match &a.c {
                Curve::Line { .. } => {
                    let (s, e) = (a.start(), b.end());
                    let d = e - s;
                    let l = d.norm();
                    Piece {
                        c: Curve::Line {
                            o: P3::new(s.x, s.y, 0.0),
                            d: V3::new(d.x / l, d.y / l, 0.0),
                        },
                        t0: 0.0,
                        t1: l,
                        rev: false,
                        src: a.src,
                        more,
                    }
                }
                Curve::Circle { .. } => {
                    let span = b.t1 - b.t0;
                    let (t0, t1) = if a.rev {
                        (a.t0 - span, a.t1)
                    } else {
                        (a.t0, a.t1 + span)
                    };
                    if t1 - t0 > TAU + tol::ANGULAR {
                        continue;
                    }
                    Piece {
                        c: a.c.clone(),
                        t0,
                        t1,
                        rev: a.rev,
                        src: a.src,
                        more,
                    }
                }
                _ => continue,
            };
            lp[i] = fused;
            lp.remove(j);
            if j < i {
                lp.rotate_left(0);
            }
            changed = true;
            break;
        }
    }
    lp
}

/// Orders an open chain's pieces head to tail (flipping pieces as needed).
pub fn chain_loop_open(mut pieces: Vec<Piece>) -> Result<Loop2> {
    if pieces.len() <= 1 {
        return Ok(pieces);
    }
    // Start from a piece end that touches no other piece.
    let touches = |p: &P2, pieces: &[Piece], skip: usize| {
        pieces.iter().enumerate().any(|(i, q)| {
            i != skip && ((q.start() - p).norm() < JOIN || (q.end() - p).norm() < JOIN)
        })
    };
    let mut first = 0;
    let mut flip = false;
    for (i, p) in pieces.iter().enumerate() {
        if !touches(&p.start(), &pieces, i) {
            first = i;
            break;
        }
        if !touches(&p.end(), &pieces, i) {
            first = i;
            flip = true;
            break;
        }
    }
    let p = pieces.remove(first);
    let mut out = vec![if flip { p.reversed() } else { p }];
    while !pieces.is_empty() {
        let Some(end) = out.last().map(Piece::end) else {
            break;
        };
        let Some(i) = pieces
            .iter()
            .position(|q| (q.start() - end).norm() < JOIN || (q.end() - end).norm() < JOIN)
        else {
            return Err(KernelError::InvalidProfile(
                "the chain's curves are not connected".into(),
            ));
        };
        let q = pieces.remove(i);
        out.push(if (q.start() - end).norm() < JOIN {
            q
        } else {
            q.reversed()
        });
    }
    Ok(out)
}

/// Crossings of the ray from q towards +x with a piece (exact for lines and circular arcs).
fn ray_crossings(p: &Piece, q: &P2) -> i32 {
    match &p.c {
        Curve::Line { .. } => {
            let (a, b) = (p.start(), p.end());
            if (a.y > q.y) != (b.y > q.y) {
                let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > q.x {
                    return 1;
                }
            }
            0
        }
        Curve::Circle { c, x, y, r } => {
            let dy = q.y - c.y;
            if dy.abs() >= *r {
                return 0;
            }
            let dx = (r * r - dy * dy).sqrt();
            let mut n = 0;
            for px in [c.x - dx, c.x + dx] {
                if px <= q.x {
                    continue;
                }
                let v = V3::new(px - c.x, q.y - c.y, 0.0);
                let ang = v.dot(y).atan2(v.dot(x));
                let a = unwrap_near(ang, (p.t0 + p.t1) * 0.5, TAU);
                // Half-open on the parameter so joints count once.
                let (lo, hi) = (p.t0, p.t1);
                if a > lo && a <= hi || (a - lo).abs() < 1e-14 && false {
                    n += 1;
                }
            }
            n
        }
        _ => {
            let pts = p.sample(1e-5);
            let mut n = 0;
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y > q.y) != (b.y > q.y) {
                    let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if x > q.x {
                        n += 1;
                    }
                }
            }
            n
        }
    }
}

/// Point strictly inside a loop (even–odd on the exact pieces).
/// A box a piece lies in (`None`: none known for sure, a B-spline with weights not all
/// positive).
fn piece_box(p: &Piece) -> Option<Bbox> {
    if let Curve::Nurbs(n) = &p.c
        && n.cps.iter().any(|h| h.w <= 0.0)
    {
        return None;
    }
    Some(super::boolean::curve_bbox(&p.c, p.t0, p.t1))
}

/// Whether `q` lies in the box `b` seen in the profile's plane (x and y).
fn in_box(b: &Bbox, q: &P2) -> bool {
    q.x >= b.min.x && q.x <= b.max.x && q.y >= b.min.y && q.y <= b.max.y
}

pub fn in_loop(l: &[Piece], q: &P2) -> bool {
    l.iter().map(|p| ray_crossings(p, q)).sum::<i32>() % 2 == 1
}

/// Whether the interiors of any two regions overlap (points just inside one region's boundary
/// lie inside another).
pub fn regions_overlap(regions: &[Region2]) -> bool {
    // (A point outside a region's outer loop's box is outside it: no ray crossed with each of
    // the loop's pieces.)
    let boxes: Vec<Option<Bbox>> = regions
        .iter()
        .map(|r| {
            r.outer.iter().try_fold(Bbox::empty(), |mut bb, p| {
                bb.union(&piece_box(p)?);
                Some(bb)
            })
        })
        .collect();
    let inside = |q: &P2, j: usize| {
        boxes[j].is_none_or(|b| in_box(&b, q))
            && in_loop(&regions[j].outer, q)
            && !regions[j].holes.iter().any(|h| in_loop(h, q))
    };
    for (i, r) in regions.iter().enumerate() {
        for l in std::iter::once(&r.outer).chain(r.holes.iter()) {
            for p in l {
                for s in [0.25, 0.5, 0.75] {
                    let d = p.dir(s);
                    let q = p.at(s) + V2::new(-d.y, d.x) * 1e-5;
                    for j in 0..regions.len() {
                        if j != i && inside(&q, j) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}
