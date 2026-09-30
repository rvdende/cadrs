//! Loft sections (P3.7, PS20), independent of the backend.
//!
//! - [`section_loop`] joins a profile's regions into the one closed contour a loft section must
//!   be (PS20.5): a curve two selected regions share runs both ways round their boundaries and
//!   drops out, so the Funnel's rim band and the disc inside it become the disc's outer ellipse.
//!   Anything but one contour is an error ("shown in red").
//! - [`Curve2::reversed`] runs a curve the other way.
//! - [`sample_sections`] samples the sections for a loft that interpolates them (the one that
//!   takes end conditions): every section is made to run the same way round the loft's axis and
//!   is started where it best lines up with the previous one; each curve (or, for sections made
//!   of one smooth closed curve, the whole contour) gets the same number of samples, so sample
//!   `j` of every section lies on corresponding curves.

use nalgebra::{Point2, Point3, Vector3};

use crate::{Curve2, KernelError, Plane, Profile, Result};

/// How close two curve ends must be to join (mm).
const JOIN: f64 = 1e-6;

impl Curve2 {
    /// The same curve run the other way (a whole circle or ellipse becomes a clockwise arc of a
    /// whole turn).
    pub fn reversed(&self) -> Curve2 {
        use std::f64::consts::TAU;
        match self.clone() {
            Curve2::Line { a, b, source } => Curve2::Line { a: b, b: a, source },
            Curve2::Arc { center, radius, start_angle, sweep, source } => Curve2::Arc {
                center,
                radius,
                start_angle: start_angle + sweep,
                sweep: -sweep,
                source,
            },
            Curve2::Circle { center, radius, source } => Curve2::Arc {
                center,
                radius: radius.abs(),
                start_angle: 0.0,
                sweep: -TAU,
                source,
            },
            Curve2::Ellipse { center, major_radius, minor_radius, rotation, source } => Curve2::EllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start: 0.0,
                sweep: -TAU,
                source,
            },
            Curve2::EllipseArc { center, major_radius, minor_radius, rotation, start, sweep, source } => {
                Curve2::EllipseArc {
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    start: start + sweep,
                    sweep: -sweep,
                    source,
                }
            }
            Curve2::OffsetEllipseArc { center, major_radius, minor_radius, rotation, start, sweep, offset, source } => {
                Curve2::OffsetEllipseArc {
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    start: start + sweep,
                    sweep: -sweep,
                    offset,
                    source,
                }
            }
            Curve2::Bezier { poles: [a, b, c, d], source } => Curve2::Bezier { poles: [d, c, b, a], source },
        }
    }

    /// The unit tangent at `s` in 0..=1, in the direction the curve runs (numerically).
    pub fn tangent_at(&self, s: f64) -> nalgebra::Vector2<f64> {
        let h = 1e-6;
        let (a, b) = ((s - h).max(0.0), (s + h).min(1.0));
        let d = self.point_at(b) - self.point_at(a);
        let l = d.norm();
        if l < 1e-300 { d } else { d / l }
    }
}

/// The signed area a closed run of curves encloses (positive counter-clockwise), from dense
/// samples.
pub fn signed_area(curves: &[Curve2]) -> f64 {
    let mut pts: Vec<Point2<f64>> = Vec::new();
    for c in curves {
        for k in 0..64 {
            pts.push(c.point_at(k as f64 / 64.0));
        }
    }
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

/// The curves of a loop, run counter-clockwise (`ccw`) or clockwise.
pub fn oriented_loop(curves: &[Curve2], ccw: bool) -> Vec<Curve2> {
    if (signed_area(curves) > 0.0) == ccw {
        curves.to_vec()
    } else {
        curves.iter().rev().map(Curve2::reversed).collect()
    }
}

/// The one closed contour of a profile's regions, counter-clockwise about its plane's normal:
/// every curve the regions share (run once each way) drops out, and what is left must close up
/// into exactly one loop.
pub fn section_loop(profile: &Profile) -> Result<Vec<Curve2>> {
    let mut pieces: Vec<Curve2> = Vec::new();
    for r in &profile.regions {
        pieces.extend(oriented_loop(&r.outer.curves, true));
        for h in &r.holes {
            pieces.extend(oriented_loop(&h.curves, false));
        }
    }
    if pieces.is_empty() {
        return Err(KernelError::InvalidProfile("a loft profile needs a closed region".into()));
    }
    // Drop pairs of the same curve run both ways.
    let same_reversed = |a: &Curve2, b: &Curve2| -> bool {
        a.source() == b.source()
            && (a.start() - b.end()).norm() < JOIN
            && (a.end() - b.start()).norm() < JOIN
            && (a.point_at(0.5) - b.point_at(0.5)).norm() < JOIN
            && a.tangent_at(0.5).dot(&b.tangent_at(0.5)) < -0.9
    };
    let mut keep = vec![true; pieces.len()];
    for i in 0..pieces.len() {
        if !keep[i] {
            continue;
        }
        if let Some(j) = (i + 1..pieces.len()).find(|&j| keep[j] && same_reversed(&pieces[i], &pieces[j])) {
            keep[i] = false;
            keep[j] = false;
        }
    }
    let mut left: Vec<Curve2> = pieces.into_iter().zip(keep).filter_map(|(c, k)| k.then_some(c)).collect();
    // Chain what is left into loops.
    let mut loops: Vec<Vec<Curve2>> = Vec::new();
    while let Some(first) = left.pop() {
        let mut lp = vec![first];
        loop {
            let end = lp.last().expect("one").end();
            if (end - lp[0].start()).norm() < JOIN {
                break;
            }
            match left.iter().position(|c| (c.start() - end).norm() < JOIN) {
                Some(i) => lp.push(left.swap_remove(i)),
                None => {
                    return Err(KernelError::InvalidProfile(
                        "a loft profile must be one closed contour".into(),
                    ));
                }
            }
        }
        loops.push(lp);
    }
    match loops.len() {
        1 => Ok(oriented_loop(&loops[0], true)),
        0 => Err(KernelError::InvalidProfile("a loft profile needs a closed region".into())),
        n => Err(KernelError::InvalidProfile(format!(
            "a loft profile must be one closed contour; the selection has {n}"
        ))),
    }
}

/// A section as the interpolating loft sees it: its plane and its closed loop of curves, or
/// (P3.10, a face of a body as a section) its loop of edges as dense 3D polylines in order,
/// each with its source (the edge's index); `polylines` wins when it isn't empty. A non-planar
/// face's polylines may leave `plane`, which then only orients the loop.
#[derive(Debug, Clone)]
pub struct SectionCurves {
    pub plane: Plane,
    pub curves: Vec<Curve2>,
    pub polylines: Vec<(Vec<Point3<f64>>, Option<u64>)>,
}

impl SectionCurves {
    pub fn new(plane: Plane, curves: Vec<Curve2>) -> Self {
        Self { plane, curves, polylines: Vec::new() }
    }

    /// A section made of 3D polylines (a face's boundary edges, in loop order).
    pub fn sampled(plane: Plane, polylines: Vec<(Vec<Point3<f64>>, Option<u64>)>) -> Self {
        Self { plane, curves: Vec::new(), polylines }
    }
}

/// One piece of a section's loop: an exact curve on the plane, or a dense polyline.
#[derive(Debug, Clone)]
enum Piece {
    Exact(Curve2),
    Dense { pts: Vec<Point3<f64>>, acc: Vec<f64>, closed: bool, source: Option<u64> },
}

impl Piece {
    fn dense(pts: Vec<Point3<f64>>, source: Option<u64>) -> Self {
        let mut acc = vec![0.0];
        for w in pts.windows(2) {
            acc.push(acc[acc.len() - 1] + (w[1] - w[0]).norm());
        }
        let total = acc[acc.len() - 1];
        let closed = pts.len() > 2 && (pts[0] - pts[pts.len() - 1]).norm() < 1e-6 * total.max(1.0);
        Piece::Dense { pts, acc, closed, source }
    }

    fn is_closed(&self) -> bool {
        match self {
            Piece::Exact(c) => c.is_closed(),
            Piece::Dense { closed, .. } => *closed,
        }
    }

    fn source(&self) -> Option<u64> {
        match self {
            Piece::Exact(c) => c.source(),
            Piece::Dense { source, .. } => *source,
        }
    }

    fn reversed(&self) -> Piece {
        match self {
            Piece::Exact(c) => Piece::Exact(c.reversed()),
            Piece::Dense { pts, source, .. } => Piece::dense(pts.iter().rev().copied().collect(), *source),
        }
    }
}

/// A section's pieces on its plane.
#[derive(Debug, Clone)]
struct Sec {
    plane: Plane,
    curves: Vec<Piece>,
}

impl Sec {
    fn point(&self, curve: usize, s: f64) -> Point3<f64> {
        match &self.curves[curve] {
            Piece::Exact(c) => self.plane.to_model(c.point_at(s)),
            Piece::Dense { pts, acc, .. } => {
                let total = acc[acc.len() - 1].max(1e-12);
                let t = total * s.clamp(0.0, 1.0);
                let k = acc.partition_point(|a| *a < t).clamp(1, pts.len() - 1);
                let f = ((t - acc[k - 1]) / (acc[k] - acc[k - 1]).max(1e-12)).clamp(0.0, 1.0);
                pts[k - 1] + (pts[k] - pts[k - 1]) * f
            }
        }
    }

    /// The section's pieces run counter-clockwise (`ccw`) about its plane's normal, or not.
    fn oriented(s: &SectionCurves, ccw: bool) -> Sec {
        if s.polylines.is_empty() {
            return Sec { plane: s.plane, curves: oriented_loop(&s.curves, ccw).into_iter().map(Piece::Exact).collect() };
        }
        let (x, y) = (s.plane.x_dir.into_inner(), s.plane.y_dir().into_inner());
        let pts: Vec<Point2<f64>> = s
            .polylines
            .iter()
            .flat_map(|(p, _)| p.iter())
            .map(|p| {
                let d = p - s.plane.origin;
                Point2::new(d.dot(&x), d.dot(&y))
            })
            .collect();
        let n = pts.len();
        let area: f64 = (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum::<f64>() / 2.0;
        let pieces: Vec<Piece> = s.polylines.iter().map(|(p, src)| Piece::dense(p.clone(), *src)).collect();
        let curves = if (area > 0.0) == ccw { pieces } else { pieces.iter().rev().map(Piece::reversed).collect() };
        Sec { plane: s.plane, curves }
    }
}

/// Sampled sections: `patches[p][i]` holds the samples of patch `p` along section `i`.
#[derive(Debug, Clone)]
pub struct Samples {
    pub patches: Vec<Vec<Vec<Point3<f64>>>>,
    /// One smooth closed curve per section: a single periodic patch whose samples don't repeat
    /// the first point.
    pub periodic: bool,
    /// The centre of each section (the mean of its samples).
    pub centers: Vec<Point3<f64>>,
    /// Each section's plane normal, turned to point along the loft (from the first section
    /// towards the last).
    pub normals: Vec<Vector3<f64>>,
    /// The source of the first section's curve each patch runs along (its curve id).
    pub sources: Vec<Option<u64>>,
}

/// Samples per curve (or per whole contour, for smooth closed sections).
pub const SAMPLES: usize = 96;

/// Samples the sections for the interpolating loft (see the module docs). Sections of one smooth
/// closed curve each give one periodic patch; otherwise every section must have the same number
/// of curves, and each curve is a patch.
pub fn sample_sections(sections: &[SectionCurves]) -> Result<Samples> {
    if sections.len() < 2 {
        return Err(KernelError::InvalidParameter("a loft needs at least two profiles".into()));
    }
    let centroid = |s: &Sec| -> Point3<f64> {
        let mut acc = Vector3::zeros();
        let mut n = 0.0;
        for (i, _) in s.curves.iter().enumerate() {
            for k in 0..16 {
                acc += s.point(i, k as f64 / 16.0).coords;
                n += 1.0;
            }
        }
        Point3::from(acc / n)
    };
    let raw: Vec<Sec> = sections.iter().map(|s| Sec::oriented(s, true)).collect();
    let axis = centroid(&raw[raw.len() - 1]) - centroid(&raw[0]);
    if axis.norm() < 1e-9 {
        return Err(KernelError::InvalidParameter("the loft's profiles lie on top of each other".into()));
    }
    // Every loop runs counter-clockwise about the loft's axis.
    let mut secs: Vec<Sec> = sections
        .iter()
        .map(|s| {
            let ccw = s.plane.normal.dot(&axis) >= 0.0;
            Sec::oriented(s, ccw)
        })
        .collect();
    let smooth_closed = |s: &Sec| s.curves.len() == 1 && s.curves[0].is_closed();
    let periodic = secs.iter().all(smooth_closed);
    let count = secs[0].curves.len();
    let mut patches: Vec<Vec<Vec<Point3<f64>>>> = Vec::new();
    let mut sources = Vec::new();
    if !periodic && secs.iter().any(|s| s.curves.len() != count) {
        // PS20.3: profiles with different numbers of edges (a square and a circle). The profile
        // with the most edges gives the corners; every other one is cut where its direction
        // from its centre (about the loft's axis) is nearest each corner's, and each piece is
        // resampled by length (a square's sides meet a circle's quarters).
        let n_axis = axis.normalize();
        let e1 = {
            let t = if n_axis.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
            (t - n_axis * t.dot(&n_axis)).normalize()
        };
        let e2 = n_axis.cross(&e1);
        let polar = |c: &Point3<f64>, p: &Point3<f64>| {
            let d = p - c;
            d.dot(&e2).atan2(d.dot(&e1))
        };
        let reference = (0..secs.len()).max_by_key(|&i| secs[i].curves.len()).unwrap_or(0);
        let n = secs[reference].curves.len();
        let rc = centroid(&secs[reference]);
        let corners: Vec<f64> = (0..n).map(|k| polar(&rc, &secs[reference].point(k, 0.0))).collect();
        let resample = |pts: &[Point3<f64>]| -> Vec<Point3<f64>> {
            let mut acc = vec![0.0];
            for w in pts.windows(2) {
                acc.push(acc[acc.len() - 1] + (w[1] - w[0]).norm());
            }
            let total = acc[acc.len() - 1].max(1e-12);
            (0..SAMPLES)
                .map(|j| {
                    let t = total * j as f64 / (SAMPLES - 1) as f64;
                    let k = acc.partition_point(|a| *a < t).clamp(1, pts.len() - 1);
                    let f = ((t - acc[k - 1]) / (acc[k] - acc[k - 1]).max(1e-12)).clamp(0.0, 1.0);
                    pts[k - 1] + (pts[k] - pts[k - 1]) * f
                })
                .collect()
        };
        let mut per_section: Vec<Vec<Vec<Point3<f64>>>> = Vec::new();
        for (i, sec) in secs.iter().enumerate() {
            if sec.curves.len() == n && i == reference {
                per_section.push((0..n).map(|k| (0..SAMPLES).map(|j| sec.point(k, j as f64 / (SAMPLES - 1) as f64)).collect()).collect());
                continue;
            }
            // The whole loop, densely.
            let dense: Vec<Point3<f64>> = (0..sec.curves.len())
                .flat_map(|k| (0..SAMPLES * 4).map(move |j| (k, j as f64 / (SAMPLES * 4) as f64)))
                .map(|(k, t)| sec.point(k, t))
                .collect();
            let c = centroid(sec);
            let angle_gap = |a: f64, b: f64| {
                let d = (a - b).rem_euclid(std::f64::consts::TAU);
                d.min(std::f64::consts::TAU - d)
            };
            let cuts: Vec<usize> = corners
                .iter()
                .map(|&a| (0..dense.len()).min_by(|&x, &y| angle_gap(polar(&c, &dense[x]), a).total_cmp(&angle_gap(polar(&c, &dense[y]), a))).unwrap_or(0))
                .collect();
            let m = dense.len();
            let pieces = (0..n)
                .map(|k| {
                    let (from, to) = (cuts[k], cuts[(k + 1) % n]);
                    let len = (to + m - from) % m;
                    let len = if len == 0 { m } else { len };
                    let run: Vec<Point3<f64>> = (0..=len).map(|j| dense[(from + j) % m]).collect();
                    resample(&run)
                })
                .collect();
            per_section.push(pieces);
        }
        for k in 0..n {
            patches.push(per_section.iter().map(|sec| sec[k].clone()).collect());
            sources.push(secs[reference].curves[k].source());
        }
    } else if periodic {
        // Sample each closed curve, then start each section where its direction from the centre
        // best matches the previous section's samples.
        let mut rings: Vec<Vec<Point3<f64>>> = secs
            .iter()
            .map(|s| (0..SAMPLES).map(|j| s.point(0, j as f64 / SAMPLES as f64)).collect())
            .collect();
        for i in 1..rings.len() {
            let dirs = |r: &[Point3<f64>]| -> Vec<Vector3<f64>> {
                let c = r.iter().fold(Vector3::zeros(), |a, p| a + p.coords) / r.len() as f64;
                r.iter().map(|p| (p.coords - c).normalize()).collect()
            };
            let (prev, cur) = (dirs(&rings[i - 1]), dirs(&rings[i]));
            let best = (0..SAMPLES)
                .min_by(|&a, &b| {
                    let cost = |r: usize| (0..SAMPLES).map(|j| (cur[(j + r) % SAMPLES] - prev[j]).norm_squared()).sum::<f64>();
                    cost(a).total_cmp(&cost(b))
                })
                .unwrap_or(0);
            rings[i].rotate_left(best);
        }
        patches.push(rings);
        sources.push(secs[0].curves[0].source());
    } else {
        // Start each section at the curve whose start best matches the previous one's.
        for i in 1..secs.len() {
            let c0 = centroid(&secs[i - 1]);
            let c1 = centroid(&secs[i]);
            let d0 = (secs[i - 1].point(0, 0.0) - c0).normalize();
            let best = (0..count)
                .min_by(|&a, &b| {
                    let cost = |r: usize| ((secs[i].point(r, 0.0) - c1).normalize() - d0).norm_squared();
                    cost(a).total_cmp(&cost(b))
                })
                .unwrap_or(0);
            secs[i].curves.rotate_left(best);
        }
        for p in 0..count {
            let patch: Vec<Vec<Point3<f64>>> = secs
                .iter()
                .map(|s| (0..SAMPLES).map(|j| s.point(p, j as f64 / (SAMPLES - 1) as f64)).collect())
                .collect();
            patches.push(patch);
            sources.push(secs[0].curves[p].source());
        }
    }
    let centers: Vec<Point3<f64>> = secs.iter().map(centroid).collect();
    let normals = secs
        .iter()
        .map(|s| {
            let n = s.plane.normal.into_inner();
            if n.dot(&axis) >= 0.0 { n } else { -n }
        })
        .collect();
    Ok(Samples { patches, periodic, centers, normals, sources })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Loop, Region};
    use nalgebra::Point2;

    fn line(a: (f64, f64), b: (f64, f64), s: u64) -> Curve2 {
        Curve2::Line { a: Point2::new(a.0, a.1), b: Point2::new(b.0, b.1), source: Some(s) }
    }

    /// Two squares sharing a side join into the 2 × 1 rectangle around them (the shared side
    /// runs once each way and drops out); a disc and a ring round it join into the ring's outer
    /// circle.
    #[test]
    fn regions_join_into_one_contour() {
        let a = Region {
            outer: Loop { curves: vec![line((0., 0.), (1., 0.), 1), line((1., 0.), (1., 1.), 2), line((1., 1.), (0., 1.), 3), line((0., 1.), (0., 0.), 4)] },
            holes: vec![],
            source: Some(1),
        };
        let b = Region {
            outer: Loop { curves: vec![line((1., 0.), (2., 0.), 5), line((2., 0.), (2., 1.), 6), line((2., 1.), (1., 1.), 7), line((1., 1.), (1., 0.), 2)] },
            holes: vec![],
            source: Some(2),
        };
        let p = Profile::new(Plane::top(), vec![a, b]);
        let lp = section_loop(&p).unwrap();
        assert_eq!(lp.len(), 6);
        assert!((signed_area(&lp) - 2.0).abs() < 1e-9);
        let circle = |r: f64, s: u64| Curve2::Circle { center: Point2::origin(), radius: r, source: Some(s) };
        let disc = Region { outer: Loop { curves: vec![circle(1.0, 10)] }, holes: vec![], source: Some(1) };
        let ring = Region { outer: Loop { curves: vec![circle(2.0, 11)] }, holes: vec![Loop { curves: vec![circle(1.0, 10)] }], source: Some(2) };
        let lp = section_loop(&Profile::new(Plane::top(), vec![disc, ring])).unwrap();
        assert_eq!(lp.len(), 1);
        assert_eq!(lp[0].source(), Some(11));
        assert!((signed_area(&lp) - std::f64::consts::PI * 4.0).abs() < 0.05, "a 64-gon");
        // Two separate squares are two contours: an error.
        let far = Region {
            outer: Loop { curves: vec![line((5., 0.), (6., 0.), 8), line((6., 0.), (6., 1.), 9), line((6., 1.), (5., 1.), 10), line((5., 1.), (5., 0.), 11)] },
            holes: vec![],
            source: Some(3),
        };
        let a2 = Region {
            outer: Loop { curves: vec![line((0., 0.), (1., 0.), 1), line((1., 0.), (1., 1.), 2), line((1., 1.), (0., 1.), 3), line((0., 1.), (0., 0.), 4)] },
            holes: vec![],
            source: Some(1),
        };
        assert!(section_loop(&Profile::new(Plane::top(), vec![a2, far])).is_err());
    }

    /// The offset ellipse's points lie `offset` from the ellipse along its normal: at t = 0 the
    /// point (a + d, 0), at t = π/2 the point (0, b + d).
    #[test]
    fn offset_ellipse_points() {
        let c = Curve2::OffsetEllipseArc {
            center: Point2::origin(),
            major_radius: 3.0,
            minor_radius: 2.0,
            rotation: 0.0,
            start: 0.0,
            sweep: std::f64::consts::TAU,
            offset: -0.125,
            source: None,
        };
        assert!((c.point_at(0.0) - Point2::new(2.875, 0.0)).norm() < 1e-12);
        assert!((c.point_at(0.25) - Point2::new(0.0, 1.875)).norm() < 1e-12);
        let r = c.reversed();
        assert!((r.point_at(0.25) - Point2::new(0.0, -1.875)).norm() < 1e-12);
    }
}
