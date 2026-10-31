//! Lofts through curves (Onshape's loft of wire profiles, with guides): one sheet face.
//!
//! The profiles (open chains of curves, each joined into one B-spline on u ∈ [0, 1]) are run the
//! same way and interpolated across, v ∈ [0, 1] (v_i at profile i): piecewise cubic Hermite,
//! with the end conditions' derivatives at the first and last profile (Match tangent: square
//! to the profile in the faces along it; Normal to profile: square to its plane) and
//! Catmull–Rom derivatives between. Each guide crosses every profile; the profiles are
//! re-parametrized so it does so at the same u (its station), and the guide's departure from
//! the interpolation there is added back, blended across u by cardinal cubics over the stations
//! (the profile ends are stations too: free, or a point where the profiles meet). With a guide
//! along one side and the profiles meeting in a point at the other, that is the Coons patch of
//! the three curves.
//!
//! Every term lies in one tensor-product B-spline space (the profiles' and guides' knots raised
//! to the common degree, the stations as C¹ knots), so the surface is found exactly by
//! interpolation at that space's Greville points: its boundary curves are the profiles and
//! guides themselves.

use std::sync::Arc;

use nalgebra::DMatrix;

use super::build::fix_senses;
use super::curve::Curve;
use super::features::Tagged;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface, basis_ders, find_span};
use super::surface::Surface;
use super::sweep::path::{chain, path_segments};
use super::tol;
use super::topo::{Body, Coedge, Face, Shell};
use crate::*;

fn invalid(why: &str) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// How close (mm) a guide must come to each profile.
const GUIDE_MEETS: f64 = 1e-2;
/// Profile ends this close (mm) are one point (the profiles meet there).
const SAME_POINT: f64 = 1e-5;
/// The fewest spans of the u space (the end derivative fields are fitted in it).
const MIN_SPANS: usize = 12;

/// The loft (see the module docs).
pub fn loft_curves(k: &mut NativeKernel, spec: &CurveLoftSpec) -> Result<OpResult> {
    let (body, names) = build(k, spec)?;
    // Each face named as a loft's side after each sketch curve it was made from (the first
    // name, the others its aliases), or unsourced. Faces along one loft each have a region of
    // their own: curve ids are only their sketch's, so two faces' curves may share one.
    let several = names.len() > 1;
    let tags = names
        .into_iter()
        .enumerate()
        .map(|(f, mut curves)| {
            if curves.is_empty() {
                curves.push(unsourced_curve(0, 0));
            }
            let region = if several {
                crate::naming::stable_hash(format!("{} {f}", spec.source).as_bytes())
            } else {
                spec.source
            };
            curves
                .into_iter()
                .map(|curve| Origin::ProfileCurve { region, curve })
                .collect()
        })
        .collect();
    Ok(Tagged { body, tags }.into_result(k))
}

/// A profile: one B-spline on [0, 1], or a point.
#[derive(Debug, Clone)]
enum Sec {
    Curve(NurbsCurve),
    Point(P3),
}

impl Sec {
    fn at(&self, u: f64) -> P3 {
        match self {
            Sec::Curve(c) => c.point(u),
            Sec::Point(p) => *p,
        }
    }
}

/// What the loft does at a station (a u where the profiles are tied).
#[derive(Debug, Clone)]
enum Station {
    /// A profile end with nothing there.
    Free,
    /// The profiles' ends meet in this point.
    Apex(P3),
    /// Guide `g` crosses the profiles here.
    Guide(usize),
}

/// The loft's sheet, and the sketch curves each of its faces was made from.
fn build(k: &NativeKernel, spec: &CurveLoftSpec) -> Result<(Body, Vec<Vec<u64>>)> {
    let n = spec.sections.len();
    if n < 2 {
        return Err(invalid("Select at least two profiles to loft"));
    }
    for e in [&spec.start, &spec.end] {
        if e.condition != LoftCondition::Default && !(e.magnitude.is_finite() && e.magnitude > 0.0)
        {
            return Err(invalid("A loft end's magnitude must be greater than zero"));
        }
    }
    let mut secs: Vec<Sec> = Vec::with_capacity(n);
    // Where each profile's curves meet (its parameters): its edges end there.
    let mut joints: Vec<Vec<f64>> = vec![Vec::new(); n];
    for (i, s) in spec.sections.iter().enumerate() {
        secs.push(match s {
            CurveSection::Point(p) => {
                if i != 0 && i + 1 != n {
                    return Err(invalid(
                        "A point can only be the first or last loft profile",
                    ));
                }
                Sec::Point(*p)
            }
            CurveSection::Wire(pcs) => {
                let (c, at) = chain_curve(k, pcs)?;
                joints[i] = at;
                Sec::Curve(c)
            }
        });
    }
    if !secs.iter().any(|s| matches!(s, Sec::Curve(_))) {
        return Err(invalid("Select a curve to loft"));
    }
    // The profiles run the same way: each as its predecessor, ends nearest.
    let mut prev: Option<NurbsCurve> = None;
    for (s, at) in secs.iter_mut().zip(joints.iter_mut()) {
        let Sec::Curve(c) = s else { continue };
        if let Some(p) = &prev {
            let (a0, a1) = (p.point(0.0), p.point(1.0));
            let (b0, b1) = (c.point(0.0), c.point(1.0));
            if (b1 - a0).norm() + (b0 - a1).norm() < (b0 - a0).norm() + (b1 - a1).norm() {
                *c = c.reversed();
                *at = at.iter().rev().map(|j| 1.0 - j).collect();
            }
        }
        prev = Some(c.clone());
    }
    // How far the profiles reach from the origin: the scale of the tolerances.
    let size = secs
        .iter()
        .flat_map(|s| (0..=8).map(move |i| s.at(i as f64 / 8.0)))
        .fold(0.0_f64, |r, p| r.max(p.coords.norm()))
        .max(1.0);

    // Guides: where each meets each profile (guide and profile parameters).
    let mut guides: Vec<NurbsCurve> = Vec::new();
    let mut sigma: Vec<Vec<f64>> = Vec::new();
    let mut along: Vec<Vec<f64>> = Vec::new();
    for g in &spec.guides {
        let (mut c, _) = chain_curve(k, g)?;
        let mut s_at = Vec::with_capacity(n);
        let mut u_at = Vec::with_capacity(n);
        for sec in &secs {
            match sec {
                Sec::Curve(p) => {
                    let (s, u, d) = meet(&c, p);
                    if d > GUIDE_MEETS {
                        return Err(invalid("A loft guide must touch every profile"));
                    }
                    s_at.push(s);
                    u_at.push(u);
                }
                Sec::Point(q) => {
                    let s = Curve::Nurbs(Arc::new(c.clone())).closest(q, 0.0, 1.0);
                    if (c.point(s) - q).norm() > GUIDE_MEETS {
                        return Err(invalid("A loft guide must touch every profile"));
                    }
                    s_at.push(s);
                    u_at.push(f64::NAN);
                }
            }
        }
        if s_at[0] > s_at[n - 1] {
            c = c.reversed();
            for s in &mut s_at {
                *s = 1.0 - *s;
            }
        }
        if s_at.windows(2).any(|w| w[1] <= w[0]) {
            return Err(invalid("A loft guide must cross the profiles in order"));
        }
        // Trimmed to the first and last profiles (Onshape's "Trim guides by profiles").
        let (a, b) = (s_at[0], s_at[n - 1]);
        let c = c.restricted(a, b).reparam(0.0, 1.0);
        guides.push(c);
        along.push(s_at.iter().map(|s| (s - a) / (b - a)).collect());
        sigma.push(u_at);
    }

    // The profiles' places across (v): the first guide's, else the mean distances.
    let v: Vec<f64> = match along.first() {
        Some(a) => a.clone(),
        None => {
            let mut d = vec![0.0];
            for i in 1..n {
                let step = (0..=16)
                    .map(|j| {
                        let u = j as f64 / 16.0;
                        (secs[i].at(u) - secs[i - 1].at(u)).norm()
                    })
                    .sum::<f64>()
                    / 17.0;
                d.push(d[i - 1] + step.max(tol::POINT));
            }
            let total = d[n - 1];
            d.iter().map(|x| x / total).collect()
        }
    };
    for (g, a) in guides.iter_mut().zip(&along) {
        *g = remap(g, a, &v)?;
    }

    // Stations: each guide's u (the mean of where it meets the profiles), the profiles
    // re-parametrized to meet it there.
    // (A guide meeting a profile within a hair of its end meets it at its end.)
    for g in sigma.iter_mut() {
        for (i, x) in g.iter_mut().enumerate() {
            let Sec::Curve(c) = &secs[i] else { continue };
            let p = c.point(*x);
            let end = 5.0 * GUIDE_MEETS;
            if *x < 1e-3 || (p - c.point(0.0)).norm() < end {
                *x = 0.0;
            } else if *x > 1.0 - 1e-3 || (p - c.point(1.0)).norm() < end {
                *x = 1.0;
            }
        }
    }
    let mut station_u: Vec<f64> = Vec::new();
    for s in &sigma {
        let at: Vec<f64> = s.iter().copied().filter(|x| x.is_finite()).collect();
        let u = if at.iter().all(|x| *x < 1e-6) {
            0.0
        } else if at.iter().all(|x| *x > 1.0 - 1e-6) {
            1.0
        } else {
            at.iter().sum::<f64>() / at.len() as f64
        };
        station_u.push(u);
    }
    let mut order: Vec<usize> = (0..guides.len()).collect();
    order.sort_by(|a, b| station_u[*a].total_cmp(&station_u[*b]));
    for (i, sec) in secs.iter_mut().enumerate() {
        let Sec::Curve(c) = sec else { continue };
        let mut from = vec![0.0];
        let mut to = vec![0.0];
        for &g in &order {
            let (s, u) = (sigma[g][i], station_u[g]);
            if u > 1e-9 && u < 1.0 - 1e-9 {
                if s <= *from.last().unwrap() + 1e-9 || s >= 1.0 - 1e-9 {
                    return Err(invalid("The loft guides must cross the profiles in order"));
                }
                from.push(s);
                to.push(u);
            }
        }
        from.push(1.0);
        to.push(1.0);
        *c = remap(c, &from, &to)?;
        for j in &mut joints[i] {
            *j = remap_value(*j, &from, &to);
        }
    }
    // Each guide moved (by a function linear between the profiles) to run exactly through the
    // profiles where it meets them: they only came within a hair.
    for (g, c) in guides.iter_mut().enumerate() {
        let miss: Vec<V3> = (0..n)
            .map(|i| secs[i].at(station_u[g]) - c.point(v[i]))
            .map(|m| {
                if m.iter().all(|x| x.is_finite()) {
                    m
                } else {
                    V3::zeros()
                }
            })
            .collect();
        let p = c.degree;
        let knots = c.knots.clone();
        for (j, h) in c.cps.iter_mut().enumerate() {
            let xi = knots[j + 1..=j + p].iter().sum::<f64>() / p as f64;
            let i = v.iter().rposition(|x| *x <= xi).unwrap_or(0).min(n - 2);
            let f = ((xi - v[i]) / (v[i + 1] - v[i])).clamp(0.0, 1.0);
            let m = miss[i] * (1.0 - f) + miss[i + 1] * f;
            *h = super::nurbs::hom(&(super::nurbs::dehom(h) + m), 1.0);
        }
    }
    let mut stations: Vec<(f64, Station)> = Vec::new();
    for end in [0.0, 1.0] {
        if let Some(&g) = order.iter().find(|&&g| (station_u[g] - end).abs() < 1e-9) {
            stations.push((end, Station::Guide(g)));
            continue;
        }
        let ends: Vec<P3> = secs.iter().map(|s| s.at(end)).collect();
        let meet = ends
            .iter()
            .all(|p| (p - ends[0]).norm() < SAME_POINT * size);
        stations.push((
            end,
            if meet {
                Station::Apex(ends[0])
            } else {
                Station::Free
            },
        ));
    }
    for &g in &order {
        let u = station_u[g];
        if u > 1e-9 && u < 1.0 - 1e-9 {
            stations.push((u, Station::Guide(g)));
        }
    }
    stations.sort_by(|a, b| a.0.total_cmp(&b.0));
    let st: Vec<f64> = stations.iter().map(|s| s.0).collect();
    if st.windows(2).any(|w| w[1] - w[0] < 1e-6) {
        return Err(invalid(
            "Two loft guides cross the profiles at the same place",
        ));
    }

    // The u space: the profiles' knots at the common degree, the stations C¹.
    let pu = secs
        .iter()
        .filter_map(|s| match s {
            Sec::Curve(c) => Some(c.degree),
            _ => None,
        })
        .max()
        .unwrap_or(3)
        .max(3);
    let mut ureq: Vec<(f64, usize)> = Vec::new();
    for s in &secs {
        if let Sec::Curve(c) = s {
            ureq.extend(requirements(c, pu));
        }
    }
    for &u in &st[1..st.len() - 1] {
        ureq.push((u, pu - 1));
    }
    let ub = Basis::new(pu, &ureq, MIN_SPANS);
    let psi = Cardinal { at: st.clone() };

    // The guides' derivatives across at the profiles (one-sided at the ends).
    let gd = |g: usize, i: usize| -> V3 {
        let c = &guides[g];
        let h = 1e-7;
        let vi = v[i];
        let d = |t: f64| c.ders(t.clamp(0.0, 1.0), 1)[1];
        if i == 0 {
            d(vi + h)
        } else if i + 1 == n {
            d(vi - h)
        } else {
            (d(vi - h) + d(vi + h)) * 0.5
        }
    };
    let guide_of = |j: usize| match stations[j].1 {
        Station::Guide(g) => Some(g),
        _ => None,
    };

    // The derivative across at each profile, a field along u fitted in the u space.
    let mut tfield: Vec<Vec<V3>> = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = if i == 0 {
            (0, 1)
        } else if i + 1 == n {
            (n - 2, n - 1)
        } else {
            (i - 1, i + 1)
        };
        let base = |u: f64| -> V3 { (secs[b].at(u) - secs[a].at(u)) / (v[b] - v[a]) };
        // Tied to the guides' own derivatives at their stations.
        let tied = |u: f64| -> V3 {
            let mut d = base(u);
            for (j, s) in st.iter().enumerate() {
                if let Some(g) = guide_of(j) {
                    d += (gd(g, i) - base(*s)) * psi.value(j, u);
                }
            }
            d
        };
        let end = if i == 0 {
            Some(&spec.start)
        } else if i + 1 == n {
            Some(&spec.end)
        } else {
            None
        };
        let poles = match end.map(|e| e.condition) {
            None | Some(LoftCondition::Default) => ub.fit(&tied),
            Some(c) => {
                let e = end.unwrap();
                let Sec::Curve(prof) = &secs[i] else {
                    return Err(invalid(
                        "End conditions need profiles at both ends, not points",
                    ));
                };
                let edges: &[PathCurve] = match &spec.sections[i] {
                    CurveSection::Wire(pcs) => pcs,
                    CurveSection::Point(_) => &[],
                };
                let dir = direction(k, c, e, prof, edges)?;
                // Into the loft at the start, onwards at the end: the way the profiles go,
                // point by point (the faces along a profile need not face the same way), else
                // as most of it goes where the profiles are too close to say.
                let vote = {
                    let mut vote = 0.0;
                    for j in 0..=32 {
                        let u = j as f64 / 32.0;
                        vote += dir(u).dot(&base(u));
                    }
                    if vote < 0.0 { -1.0 } else { 1.0 }
                };
                let oriented = |u: f64| -> V3 {
                    let (d, b) = (dir(u), base(u));
                    let along = d.dot(&b);
                    if along.abs() > 0.1 * b.norm() {
                        d * along.signum()
                    } else {
                        d * vote
                    }
                };
                let m = e.magnitude;
                let len = |u: f64| -> f64 {
                    let mut l = m * base(u).norm();
                    for (j, s) in st.iter().enumerate() {
                        if let Some(g) = guide_of(j) {
                            let want = gd(g, i).dot(&oriented(*s));
                            let want = if want > tol::POINT {
                                want
                            } else {
                                m * base(*s).norm()
                            };
                            l += (want - m * base(*s).norm()) * psi.value(j, u);
                        }
                    }
                    l
                };
                ub.fit(&|u: f64| oriented(u) * len(u))
            }
        };
        tfield.push(poles);
    }
    let tval = |i: usize, u: f64| -> V3 { ub.eval_vec(&tfield[i], u) };

    // The interpolation across.
    let across = |u: f64, w: f64| -> P3 {
        let i = match v.iter().rposition(|x| *x <= w) {
            Some(i) => i.min(n - 2),
            None => 0,
        };
        let h = v[i + 1] - v[i];
        let t = ((w - v[i]) / h).clamp(0.0, 1.0);
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        let p = secs[i].at(u).coords * h00
            + tval(i, u) * (h10 * h)
            + secs[i + 1].at(u).coords * h01
            + tval(i + 1, u) * (h11 * h);
        P3::from(p)
    };
    // Each station's departure from it.
    let departure = |j: usize, w: f64| -> V3 {
        let u = st[j];
        match &stations[j].1 {
            Station::Free => V3::zeros(),
            Station::Apex(a) => a - across(u, w),
            Station::Guide(g) => guides[*g].point(w) - across(u, w),
        }
    };
    let surface_at = |u: f64, w: f64| -> P3 {
        let mut p = across(u, w);
        for j in 0..st.len() {
            let f = psi.value(j, u);
            if f != 0.0 {
                p += departure(j, w) * f;
            }
        }
        p
    };

    // The v space: the guides' knots at the common degree, the profiles C¹.
    let pv = guides.iter().map(|g| g.degree).max().unwrap_or(3).max(3);
    let mut vreq: Vec<(f64, usize)> = Vec::new();
    for g in &guides {
        vreq.extend(requirements(g, pv));
    }
    for &w in &v[1..n - 1] {
        vreq.push((w, pv - 1));
    }
    let vb = Basis::new(pv, &vreq, 1);

    let surf = interpolate(&ub, &vb, &surface_at)?;
    // The construction is exact in the space: check it.
    let mut worst: f64 = 0.0;
    for a in 0..7 {
        for b in 0..7 {
            let (u, w) = ((a as f64 + 0.37) / 7.0, (b as f64 + 0.61) / 7.0);
            worst = worst.max((surf.point(u, w) - surface_at(u, w)).norm());
        }
    }
    if !(worst < 1e-6 * size) {
        return Err(KernelError::InvalidResult {
            op: "loft",
            why: format!("the curve loft's surface strays {worst:.2e} mm from its construction"),
        });
    }
    let sides = [
        matches!(secs[0], Sec::Point(_)),
        matches!(stations.last().map(|s| &s.1), Some(Station::Apex(_))),
        matches!(secs[n - 1], Sec::Point(_)),
        matches!(stations[0].1, Station::Apex(_)),
    ];
    // A guide crossing the first or last profile where its curves meet parts the faces there.
    let splits: Vec<f64> = stations
        .iter()
        .filter(|(u, s)| {
            matches!(s, Station::Guide(_))
                && [&joints[0], &joints[n - 1]]
                    .iter()
                    .any(|js| js.iter().any(|j| (j - u).abs() < 1e-6))
        })
        .map(|(u, _)| *u)
        .filter(|u| *u > 1e-9 && *u < 1.0 - 1e-9)
        .collect();
    let body = sheet(surf, sides, [&joints[0], &joints[n - 1]], &splits)?;
    // The sketch curves each face was made from: the profiles' curves over its stretch, and
    // (one face) the guides'.
    let mut bounds = vec![0.0];
    bounds.extend(&splits);
    bounds.push(1.0);
    let mut names: Vec<Vec<u64>> = vec![Vec::new(); bounds.len() - 1];
    let mut name = |f: usize, src: u64| {
        if !names[f].contains(&src) {
            names[f].push(src);
        }
    };
    for (sec, s) in secs.iter().zip(&spec.sections) {
        let (Sec::Curve(c), CurveSection::Wire(pcs)) = (sec, s) else {
            continue;
        };
        let curve = Curve::Nurbs(Arc::new(c.clone()));
        for pc in pcs {
            let PathCurve::Sketch { curve: c2, .. } = pc else {
                continue;
            };
            let Some(src) = c2.source() else { continue };
            let Some(mid) = path_segments(k, std::slice::from_ref(pc))
                .ok()
                .and_then(|v| v.into_iter().next())
                .map(|g| g.c.point(0.5 * (g.t0 + g.t1)))
            else {
                continue;
            };
            let u = curve.closest(&mid, 0.0, 1.0);
            let f = bounds.windows(2).position(|w| u <= w[1]).unwrap_or(0);
            name(f, src);
        }
    }
    if bounds.len() == 2 {
        for pc in spec.guides.iter().flatten() {
            if let PathCurve::Sketch { curve, .. } = pc
                && let Some(src) = curve.source()
            {
                name(0, src);
            }
        }
    }
    Ok((body, names))
}

/// The curves of a profile or guide joined end to end into one non-rational B-spline on
/// [0, 1], each over a share of it as long as the curve.
pub(crate) fn chain_curve(k: &NativeKernel, pcs: &[PathCurve]) -> Result<(NurbsCurve, Vec<f64>)> {
    let segs = path_segments(k, pcs)?;
    // Each curve's ends and sketch curve (a sketch spline comes as one curve per span: those
    // aren't joints).
    let ends: Vec<(P3, P3, Option<u64>)> = pcs
        .iter()
        .filter_map(|pc| {
            let src = match pc {
                PathCurve::Sketch { curve, .. } => curve.source(),
                _ => None,
            };
            let s = path_segments(k, std::slice::from_ref(pc))
                .ok()?
                .into_iter()
                .next()?;
            Some((s.start(), s.end(), src))
        })
        .collect();
    let source_of = |s: &super::sweep::path::Seg| -> Option<u64> {
        let (a, z) = (s.start(), s.end());
        ends.iter()
            .find(|(p, q, _)| {
                ((a - p).norm() < 1e-6 && (z - q).norm() < 1e-6)
                    || ((a - q).norm() < 1e-6 && (z - p).norm() < 1e-6)
            })
            .and_then(|x| x.2)
    };
    let (segs, closed) = chain(segs)
        .map_err(|_| invalid("A loft profile's curves must join end to end in one chain"))?;
    let sources: Vec<Option<u64>> = segs.iter().map(source_of).collect();
    if closed {
        return Err(KernelError::Unsupported("lofts through closed curves"));
    }
    let lens: Vec<f64> = segs.iter().map(|s| s.length().max(tol::POINT)).collect();
    let total: f64 = lens.iter().sum();
    let mut pieces: Vec<NurbsCurve> = segs
        .iter()
        .map(|s| {
            let c = s.c.to_nurbs(s.t0, s.t1);
            let c = if s.rev { c.reversed() } else { c };
            polynomial(&c)
        })
        .collect();
    let deg = pieces.iter().map(|c| c.degree).max().unwrap_or(3);
    for c in &mut pieces {
        while c.degree < deg {
            *c = c.elevated();
        }
    }
    let mut at = 0.0;
    let mut out: Option<NurbsCurve> = None;
    let mut joints = Vec::new();
    for (i, (c, l)) in pieces.iter().zip(&lens).enumerate() {
        let next = at + l / total;
        let c = c.reparam(at, next);
        out = Some(match out {
            None => c,
            Some(o) => {
                if sources[i].is_none() || sources[i] != sources[i - 1] {
                    joints.push(at);
                }
                o.joined(&c)
            }
        });
        at = next;
    }
    let out = out.ok_or_else(|| invalid("Select a curve to loft"))?;
    Ok((out.reparam(0.0, 1.0), joints))
}

/// Where the piecewise linear map taking `from[i]` to `to[i]` takes `x`.
fn remap_value(x: f64, from: &[f64], to: &[f64]) -> f64 {
    for i in 1..from.len() {
        if x <= from[i] || i + 1 == from.len() {
            let f = (x - from[i - 1]) / (from[i] - from[i - 1]);
            return to[i - 1] + (to[i] - to[i - 1]) * f;
        }
    }
    x
}

/// The curve as a non-rational B-spline: itself, or (rational) interpolated through dense
/// points on it.
fn polynomial(c: &NurbsCurve) -> NurbsCurve {
    if !c.is_rational() {
        return c.clone();
    }
    let spans = c.breaks().len().saturating_sub(1).max(1);
    let m = 16 * spans;
    let (a, b) = c.range();
    let params: Vec<f64> = (0..=m).map(|i| i as f64 / m as f64).collect();
    let pts: Vec<P3> = params.iter().map(|t| c.point(a + (b - a) * t)).collect();
    let d = c.ders(a, 1)[1] * (b - a);
    let e = c.ders(b, 1)[1] * (b - a);
    NurbsCurve::interpolate(&pts, &params, 3, Some(d), Some(e))
}

/// Where curves `a` and `b` come closest: (parameter on `a`, on `b`, the distance).
fn meet(a: &NurbsCurve, b: &NurbsCurve) -> (f64, f64, f64) {
    let ca = Curve::Nurbs(Arc::new(a.clone()));
    let cb = Curve::Nurbs(Arc::new(b.clone()));
    let mut best = (0.0, 0.0, f64::INFINITY);
    for i in 0..=64 {
        let s = i as f64 / 64.0;
        let p = a.point(s);
        let t = cb.closest(&p, 0.0, 1.0);
        let d = (b.point(t) - p).norm();
        if d < best.2 {
            best = (s, t, d);
        }
    }
    let (mut s, mut t) = (best.0, best.1);
    for _ in 0..20 {
        s = ca.closest(&b.point(t), 0.0, 1.0);
        t = cb.closest(&a.point(s), 0.0, 1.0);
    }
    (s, t, (a.point(s) - b.point(t)).norm())
}

/// The curve re-parametrized piecewise linearly, `from[i]` going to `to[i]` (both rising from
/// 0 to 1): split there and each piece stretched.
fn remap(c: &NurbsCurve, from: &[f64], to: &[f64]) -> Result<NurbsCurve> {
    if from
        .iter()
        .zip(to)
        .all(|(a, b)| (a - b).abs() < tol::ROUNDOFF)
    {
        return Ok(c.clone());
    }
    let mut rest = c.clone();
    let mut out: Option<NurbsCurve> = None;
    for i in 1..from.len() {
        let piece = if i + 1 < from.len() {
            let (l, r) = rest.split(from[i])?;
            rest = r;
            l
        } else {
            rest.clone()
        };
        let piece = piece.reparam(to[i - 1], to[i]);
        out = Some(match out {
            None => piece,
            Some(o) => o.joined(&piece),
        });
    }
    out.ok_or_else(|| invalid("a loft curve has no pieces"))
}

/// The knot multiplicities a space of degree `p` needs to hold the curve exactly.
fn requirements(c: &NurbsCurve, p: usize) -> Vec<(f64, usize)> {
    let q = c.degree;
    let inner = &c.knots[q + 1..c.knots.len() - q - 1];
    let mut out: Vec<(f64, usize)> = Vec::new();
    for &k in inner {
        match out.last_mut() {
            Some((x, m)) if (k - *x).abs() < 1e-12 => *m += 1,
            _ => out.push((k, 1)),
        }
    }
    out.into_iter()
        .map(|(k, m)| (k, (p - q + m).min(p)))
        .collect()
}

/// A clamped B-spline basis on [0, 1].
struct Basis {
    p: usize,
    knots: Vec<f64>,
}

impl Basis {
    /// Degree `p` with the interior knots `req` (value, multiplicity; equal values merged at
    /// the larger), the widest spans halved until there are at least `spans`.
    fn new(p: usize, req: &[(f64, usize)], spans: usize) -> Self {
        let mut req: Vec<(f64, usize)> = req
            .iter()
            .copied()
            .filter(|(k, _)| *k > 1e-9 && *k < 1.0 - 1e-9)
            .collect();
        req.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, usize)> = Vec::new();
        for (k, m) in req {
            match merged.last_mut() {
                Some((x, mm)) if (k - *x).abs() < 1e-9 => *mm = (*mm).max(m),
                _ => merged.push((k, m.min(p))),
            }
        }
        while merged.len() + 1 < spans {
            let mut bounds = vec![0.0];
            bounds.extend(merged.iter().map(|x| x.0));
            bounds.push(1.0);
            let (i, _) = bounds
                .windows(2)
                .enumerate()
                .max_by(|a, b| (a.1[1] - a.1[0]).total_cmp(&(b.1[1] - b.1[0])))
                .unwrap();
            merged.insert(i, ((bounds[i] + bounds[i + 1]) * 0.5, 1));
        }
        let mut knots = vec![0.0; p + 1];
        for (k, m) in &merged {
            knots.extend(std::iter::repeat_n(*k, *m));
        }
        knots.extend(std::iter::repeat_n(1.0, p + 1));
        Self { p, knots }
    }

    fn len(&self) -> usize {
        self.knots.len() - self.p - 1
    }

    /// The Greville abscissae.
    fn greville(&self) -> Vec<f64> {
        (0..self.len())
            .map(|i| self.knots[i + 1..=i + self.p].iter().sum::<f64>() / self.p as f64)
            .collect()
    }

    /// The non-zero basis functions at `t`: the first's index and the values.
    fn values(&self, t: f64) -> (usize, Vec<f64>) {
        let n = self.len();
        let span = find_span(n - 1, self.p, t, &self.knots);
        let b = basis_ders(span, t, self.p, 0, &self.knots);
        (span - self.p, b[0].clone())
    }

    /// The matrix of the basis functions (columns) at `ts` (rows).
    fn matrix(&self, ts: &[f64]) -> DMatrix<f64> {
        let mut m = DMatrix::zeros(ts.len(), self.len());
        for (r, &t) in ts.iter().enumerate() {
            let (first, vals) = self.values(t);
            for (k, x) in vals.iter().enumerate() {
                m[(r, first + k)] = *x;
            }
        }
        m
    }

    fn eval_vec(&self, poles: &[V3], t: f64) -> V3 {
        let (first, vals) = self.values(t);
        vals.iter()
            .enumerate()
            .fold(V3::zeros(), |a, (k, x)| a + poles[first + k] * *x)
    }

    /// Least-squares poles of a vector field (exact for one the space holds).
    fn fit(&self, f: &dyn Fn(f64) -> V3) -> Vec<V3> {
        let n = self.len();
        let m = (8 * n).max(64);
        let ts: Vec<f64> = (0..=m).map(|i| i as f64 / m as f64).collect();
        let a = self.matrix(&ts);
        let mut rhs = DMatrix::zeros(ts.len(), 3);
        for (r, &t) in ts.iter().enumerate() {
            let d = f(t);
            for c in 0..3 {
                rhs[(r, c)] = d[c];
            }
        }
        let sol = a
            .svd(true, true)
            .solve(&rhs, tol::ROUNDOFF_FINE)
            .unwrap_or_else(|_| DMatrix::zeros(n, 3));
        (0..n)
            .map(|i| V3::new(sol[(i, 0)], sol[(i, 1)], sol[(i, 2)]))
            .collect()
    }
}

/// Cardinal cubics over stations: 1 at its own, 0 at the others, Catmull–Rom slopes (two
/// stations: the straight line).
struct Cardinal {
    at: Vec<f64>,
}

impl Cardinal {
    fn value(&self, j: usize, u: f64) -> f64 {
        let s = &self.at;
        let m = s.len();
        let i = match s.iter().rposition(|x| *x <= u) {
            Some(i) => i.min(m - 2),
            None => 0,
        };
        let y = |k: usize| if k == j { 1.0 } else { 0.0 };
        let slope = |k: usize| -> f64 {
            if k == 0 {
                (y(1) - y(0)) / (s[1] - s[0])
            } else if k + 1 == m {
                (y(m - 1) - y(m - 2)) / (s[m - 1] - s[m - 2])
            } else {
                (y(k + 1) - y(k - 1)) / (s[k + 1] - s[k - 1])
            }
        };
        let h = s[i + 1] - s[i];
        let t = ((u - s[i]) / h).clamp(0.0, 1.0);
        let (t2, t3) = (t * t, t * t * t);
        y(i) * (2.0 * t3 - 3.0 * t2 + 1.0)
            + slope(i) * h * (t3 - 2.0 * t2 + t)
            + y(i + 1) * (-2.0 * t3 + 3.0 * t2)
            + slope(i + 1) * h * (t3 - t2)
    }
}

/// The unit direction an end condition gives along a profile (its sign still to choose).
fn direction<'a>(
    k: &'a NativeKernel,
    c: LoftCondition,
    e: &'a CurveLoftEnd,
    prof: &'a NurbsCurve,
    curves: &[PathCurve],
) -> Result<Box<dyn Fn(f64) -> V3 + 'a>> {
    Ok(match c {
        LoftCondition::NormalToProfile => {
            let pts: Vec<P3> = (0..=32).map(|i| prof.point(i as f64 / 32.0)).collect();
            let n = plane_normal(&pts)
                .ok_or_else(|| invalid("Normal to profile needs a profile in a plane"))?;
            Box::new(move |_| n)
        }
        LoftCondition::NormalDirection(d) => {
            let d = V3::new(d[0], d[1], d[2]);
            let d = d
                .try_normalize(tol::DEGENERATE)
                .ok_or_else(|| invalid("A loft direction needs a non-zero vector"))?;
            Box::new(move |_| d)
        }
        LoftCondition::MatchTangent | LoftCondition::MatchCurvature => {
            // The faces picked, else those along a profile of part edges.
            let mut picked = e.faces.clone();
            if picked.is_empty() {
                for pc in curves {
                    if let PathCurve::Edge { body, edge } = pc {
                        let b = k.get(*body)?;
                        let ef = b.edge_faces();
                        for &f in ef.get(edge.0 as usize).into_iter().flatten() {
                            if !picked.contains(&(*body, FaceId(f as u64))) {
                                picked.push((*body, FaceId(f as u64)));
                            }
                        }
                    }
                }
            }
            if picked.is_empty() {
                return Err(invalid("Match tangent needs the faces along the profile"));
            }
            let mut faces: Vec<(Arc<Body>, usize)> = Vec::new();
            for (b, f) in &picked {
                let body = k.get(*b)?.clone();
                let fi = f.0 as usize;
                if fi >= body.faces.len() {
                    return Err(invalid("A face to match no longer exists"));
                }
                faces.push((body, fi));
            }
            Box::new(move |u: f64| {
                let p = prof.point(u);
                let t = prof.ders(u, 1)[1];
                let mut best: Option<(f64, V3)> = None;
                for (b, fi) in &faces {
                    let f = &b.faces[*fi];
                    let (fu, fv) = f.surf.project(&p, None);
                    let d = (f.surf.eval(fu, fv) - p).norm();
                    if best.is_none_or(|(bd, _)| d < bd) {
                        let nrm = f.surf.normal(fu, fv) * if f.sense { 1.0 } else { -1.0 };
                        best = Some((d, nrm));
                    }
                }
                let nrm = best.map(|b| b.1).unwrap_or_else(V3::zeros);
                t.cross(&nrm)
                    .try_normalize(tol::DEGENERATE)
                    .unwrap_or_else(V3::zeros)
            })
        }
        _ => {
            return Err(KernelError::Unsupported(
                "this end condition on a loft through curves",
            ));
        }
    })
}

/// The normal of the plane the points lie in, if they do.
fn plane_normal(pts: &[P3]) -> Option<V3> {
    let c = P3::from(pts.iter().fold(V3::zeros(), |a, p| a + p.coords) / pts.len() as f64);
    let mut best = V3::zeros();
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            let n = (pts[i] - c).cross(&(pts[j] - c));
            if n.norm() > best.norm() {
                best = n;
            }
        }
    }
    let n = best.try_normalize(tol::DEGENERATE)?;
    let size = pts.iter().map(|p| (p - c).norm()).fold(0.0, f64::max);
    pts.iter()
        .all(|p| (p - c).dot(&n).abs() < 1e-6 * (1.0 + size))
        .then_some(n)
}

/// The surface of the space `ub` × `vb` through `f` at the Greville points.
fn interpolate(ub: &Basis, vb: &Basis, f: &dyn Fn(f64, f64) -> P3) -> Result<NurbsSurface> {
    let (gu, gv) = (ub.greville(), vb.greville());
    let (nu, nv) = (gu.len(), gv.len());
    let au = ub.matrix(&gu).lu();
    let av = vb.matrix(&gv).lu();
    let fail = || KernelError::InvalidResult {
        op: "loft",
        why: "the curve loft's interpolation is singular".into(),
    };
    // Along u for each v, then along v for each u.
    let mut rows: Vec<DMatrix<f64>> = Vec::with_capacity(nv);
    for &w in &gv {
        let mut rhs = DMatrix::zeros(nu, 3);
        for (i, &u) in gu.iter().enumerate() {
            let p = f(u, w);
            for c in 0..3 {
                rhs[(i, c)] = p[c];
            }
        }
        rows.push(au.solve(&rhs).ok_or_else(fail)?);
    }
    let mut cps: Vec<Vec<H>> = Vec::with_capacity(nu);
    for i in 0..nu {
        let mut rhs = DMatrix::zeros(nv, 3);
        for (j, r) in rows.iter().enumerate() {
            for c in 0..3 {
                rhs[(j, c)] = r[(i, c)];
            }
        }
        let col = av.solve(&rhs).ok_or_else(fail)?;
        cps.push(
            (0..nv)
                .map(|j| H::new(col[(j, 0)], col[(j, 1)], col[(j, 2)], 1.0))
                .collect(),
        );
    }
    NurbsSurface::new(ub.p, vb.p, ub.knots.clone(), vb.knots.clone(), cps)
}

/// The sheet on the surface (u, v ∈ [0, 1]): one face, or one per stretch between the `splits`
/// (u where a guide crosses the profiles at their curves' joints), sharing the surface.
/// `degenerate`: the sides v = 0, u = 1, v = 1, u = 0 that are points; `joints`: the u where the
/// first and last profiles' edges end (where their curves meet).
pub(crate) fn sheet(
    surf: NurbsSurface,
    degenerate: [bool; 4],
    joints: [&[f64]; 2],
    splits: &[f64],
) -> Result<Body> {
    let surface = Surface::Nurbs(Arc::new(surf.clone()));
    let mut b = Body::default();
    let vertex = |b: &mut Body, p: P3| -> usize {
        match b.verts.iter().position(|v| (v.p - p).norm() < SAME_POINT) {
            Some(i) => i,
            None => b.add_vertex(p),
        }
    };
    let line2 = |o: P2, d: V2| Curve::Line {
        o: P3::new(o.x, o.y, 0.0),
        d: V3::new(d.x, d.y, 0.0),
    };
    let iso = |along_u: bool, at_end: bool| -> Curve {
        Curve::Nurbs(Arc::new(super::loft_build::iso(&surf, along_u, at_end)))
    };
    let inner = |u: f64| u > 1e-9 && u < 1.0 - 1e-9;
    let mut bounds = vec![0.0];
    bounds.extend(splits.iter().copied().filter(|u| inner(*u)));
    bounds.push(1.0);
    // The profile sides' edges: (u0, u1, edge), cut at the joints and the splits.
    let profile = |b: &mut Body, top: bool| -> Vec<(f64, f64, usize)> {
        let w = if top { 1.0 } else { 0.0 };
        let c = iso(true, top);
        let mut at: Vec<f64> = bounds.clone();
        at.extend(
            joints[usize::from(top)]
                .iter()
                .copied()
                .filter(|u| inner(*u)),
        );
        at.sort_by(f64::total_cmp);
        at.dedup_by(|a, z| (*a - *z).abs() < 1e-9);
        let vs: Vec<usize> = at.iter().map(|&u| vertex(b, surf.point(u, w))).collect();
        (0..at.len() - 1)
            .map(|k| {
                (
                    at[k],
                    at[k + 1],
                    b.add_edge(c.clone(), at[k], at[k + 1], vs[k], vs[k + 1]),
                )
            })
            .collect()
    };
    let point_side = |side: usize| degenerate[side];
    let bottom = if point_side(0) {
        Vec::new()
    } else {
        profile(&mut b, false)
    };
    let top = if point_side(2) {
        Vec::new()
    } else {
        profile(&mut b, true)
    };
    // The sides across (u fixed): the loft's ends, and the edges between its faces.
    let mut across: Vec<usize> = Vec::with_capacity(bounds.len());
    for (k, &u) in bounds.iter().enumerate() {
        let (v0, v1) = (
            vertex(&mut b, surf.point(u, 0.0)),
            vertex(&mut b, surf.point(u, 1.0)),
        );
        let end = if k == 0 {
            Some(3)
        } else if k + 1 == bounds.len() {
            Some(1)
        } else {
            None
        };
        let e = match end {
            Some(side) if point_side(side) => b.add_degenerate(b.verts[v0].p, 0.0, 1.0, v0),
            Some(side) => b.add_edge(iso(false, side == 1), 0.0, 1.0, v0, v1),
            None => {
                let c = super::heal_split::iso_curve(&surface, super::heal_split::Dir::U, u)
                    .ok_or_else(|| invalid("a curve loft's face can't be cut"))?;
                b.add_edge(c, 0.0, 1.0, v0, v1)
            }
        };
        across.push(e);
    }
    let degenerate_edge = |b: &mut Body, u: f64, w: f64| -> usize {
        let v = vertex(b, surf.point(u, w));
        b.add_degenerate(b.verts[v].p, u, u, v)
    };
    let mut faces = Vec::new();
    for k in 0..bounds.len() - 1 {
        let (ua, ub) = (bounds[k], bounds[k + 1]);
        let within = |(a, z, _): &&(f64, f64, usize)| *a >= ua - 1e-9 && *z <= ub + 1e-9;
        let mut lp = Vec::new();
        if bottom.is_empty() {
            let e = degenerate_edge(&mut b, ua, 0.0);
            b.edges[e].t = [ua, ub];
            lp.push(Coedge {
                edge: e,
                fwd: true,
                pc: line2(P2::new(0.0, 0.0), V2::new(1.0, 0.0)),
            });
        }
        for &(_, _, e) in bottom.iter().filter(within) {
            lp.push(Coedge {
                edge: e,
                fwd: true,
                pc: line2(P2::new(0.0, 0.0), V2::new(1.0, 0.0)),
            });
        }
        lp.push(Coedge {
            edge: across[k + 1],
            fwd: true,
            pc: line2(P2::new(ub, 0.0), V2::new(0.0, 1.0)),
        });
        if top.is_empty() {
            let e = degenerate_edge(&mut b, ua, 1.0);
            b.edges[e].t = [ua, ub];
            lp.push(Coedge {
                edge: e,
                fwd: false,
                pc: line2(P2::new(0.0, 1.0), V2::new(1.0, 0.0)),
            });
        }
        for &(_, _, e) in top.iter().filter(within).collect::<Vec<_>>().iter().rev() {
            lp.push(Coedge {
                edge: *e,
                fwd: false,
                pc: line2(P2::new(0.0, 1.0), V2::new(1.0, 0.0)),
            });
        }
        lp.push(Coedge {
            edge: across[k],
            fwd: false,
            pc: line2(P2::new(ua, 0.0), V2::new(0.0, 1.0)),
        });
        b.faces.push(Face {
            surf: surface.clone(),
            sense: true,
            loops: vec![lp],
        });
        faces.push(b.faces.len() - 1);
    }
    fix_senses(&mut b, faces.clone());
    b.shells.push(Shell {
        faces,
        closed: false,
    });
    Ok(b)
}

/// Stations along a curve sweep's path.
const SWEEP_STATIONS: usize = 128;

/// A sweep of curves along a path (see [`Kernel::sweep_curves`]): the profile's poles carried
/// along the path in the frame the control gives (the path's tangent, and the locked faces'
/// normal or the turn-minimizing normal), at stations along it, and interpolated across them.
pub fn sweep_curves(k: &mut NativeKernel, spec: &CurveSweepSpec) -> Result<OpResult> {
    let (profile, _) = chain_curve(k, &spec.profile)?;
    let (mut path, _) = chain_curve(k, &spec.path)?;
    // The path runs from the profile's end of it.
    let near = |q: &P3| {
        let c = Curve::Nurbs(Arc::new(profile.clone()));
        let t = c.closest(q, 0.0, 1.0);
        (c.point(t) - q).norm()
    };
    if near(&path.point(1.0)) < near(&path.point(0.0)) {
        path = path.reversed();
    }
    let lock: Vec<(BodyId, FaceId)> = match &spec.frame {
        SweepFrame::LockFaces(faces) if faces.is_empty() => {
            let mut out = Vec::new();
            for pc in &spec.path {
                if let PathCurve::Edge { body, edge } = pc {
                    let b = k.get(*body)?;
                    for &f in b.edge_faces().get(edge.0 as usize).into_iter().flatten() {
                        out.push((*body, FaceId(f as u64)));
                    }
                }
            }
            if out.is_empty() {
                return Err(invalid("Select faces to lock the profile to"));
            }
            out
        }
        SweepFrame::LockFaces(faces) => faces.clone(),
        _ => Vec::new(),
    };
    let mut lock_faces: Vec<(Arc<Body>, usize)> = Vec::new();
    for (b, f) in &lock {
        let body = k.get(*b)?.clone();
        if f.0 as usize >= body.faces.len() {
            return Err(invalid("A face to lock to no longer exists"));
        }
        lock_faces.push((body, f.0 as usize));
    }
    let normal_at = |p: &P3| -> Option<V3> {
        let mut best: Option<(f64, V3)> = None;
        for (b, fi) in &lock_faces {
            let f = &b.faces[*fi];
            let (u, v) = f.surf.project(p, None);
            let d = (f.surf.eval(u, v) - p).norm();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }));
            }
        }
        best.map(|b| b.1)
    };
    // The frames along the path: (point, x along it, y, z).
    let n = SWEEP_STATIONS;
    let mut frames: Vec<(P3, V3, V3, V3)> = Vec::with_capacity(n + 1);
    for j in 0..=n {
        let s = j as f64 / n as f64;
        let d = path.ders(s, 1);
        let (p, t) = (P3::from(d[0]), d[1].normalize());
        let z = match &spec.frame {
            SweepFrame::LockFaces(_) => {
                let nrm = normal_at(&p).ok_or_else(|| invalid("No face to lock the profile to"))?;
                (nrm - t * nrm.dot(&t))
                    .try_normalize(tol::DEGENERATE)
                    .ok_or_else(|| invalid("The path runs square to the locked faces"))?
            }
            SweepFrame::KeepOrientation => V3::z(),
            SweepFrame::Follow => match frames.last() {
                // Turned as little as it can (the last frame's normal, square to the tangent).
                Some(&(_, _, _, z0)) => (z0 - t * z0.dot(&t))
                    .try_normalize(tol::DEGENERATE)
                    .unwrap_or(z0),
                None => {
                    let a = if t.x.abs() < 0.9 { V3::x() } else { V3::y() };
                    t.cross(&a).normalize()
                }
            },
        };
        let (x, y, z) = match &spec.frame {
            SweepFrame::KeepOrientation => (V3::x(), V3::y(), V3::z()),
            _ => (t, z.cross(&t), z),
        };
        frames.push((p, x, y, z));
    }
    // The profile carried along: its poles in the first frame, placed in each.
    let (p0, x0, y0, z0) = frames[0];
    let local: Vec<V3> = profile
        .cps
        .iter()
        .map(|h| {
            let w = super::nurbs::dehom(h) - p0;
            V3::new(w.dot(&x0), w.dot(&y0), w.dot(&z0))
        })
        .collect();
    let mut sv: Vec<f64> = vec![0.0];
    for w in frames.windows(2) {
        sv.push(sv.last().unwrap() + (w[1].0 - w[0].0).norm().max(tol::DEGENERATE));
    }
    let total = *sv.last().unwrap();
    for s in &mut sv {
        *s /= total;
    }
    let mut cols: Vec<Vec<super::nurbs::H>> = Vec::with_capacity(local.len());
    let mut kv = Vec::new();
    for l in &local {
        let pts: Vec<P3> = frames
            .iter()
            .map(|(p, x, y, z)| p + x * l.x + y * l.y + z * l.z)
            .collect();
        let c = NurbsCurve::interpolate(&pts, &sv, 3, None, None);
        kv = c.knots.clone();
        cols.push(c.cps);
    }
    let surf = NurbsSurface::new(profile.degree, 3, profile.knots.clone(), kv, cols)?;
    let body = sheet(surf, [false; 4], [&[], &[]], &[])?;
    let curve = spec
        .profile
        .iter()
        .find_map(|pc| match pc {
            PathCurve::Sketch { curve, .. } => curve.source(),
            _ => None,
        })
        .unwrap_or(unsourced_curve(0, 0));
    let t = Tagged {
        body,
        tags: vec![vec![Origin::ProfileCurve {
            region: spec.source,
            curve,
        }]],
    };
    Ok(t.into_result(k))
}
