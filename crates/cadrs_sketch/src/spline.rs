//! Sketch splines (Onshape's interpolated spline, `skInterpolatedSpline`): a C2 cubic through
//! an ordered list of sketch points, open or closed (periodic).
//!
//! As Onshape does, the points are parametrized **centripetally** (each span's parameter
//! length is the square root of its chord) over `0..=1`. An open spline's ends are *natural*
//! (no curvature) unless a tangent is set there: a derivative in that parameter (mm per unit
//! of the whole spline's parameter), which the Spline tool's end **handles** set. A handle is
//! drawn at `end ± derivative · h / 3`, where `h` is the end span's parameter length: the
//! Bézier control point next to the end, which is where Onshape's `startHandlePosition` is.
//!
//! The curve itself is kept as sketch points (the solver's unknowns: dragging one reshapes the
//! spline) in [`crate::Sketch::splines`], keyed by the curve; [`crate::CurveKind::Spline`]
//! names only its ends. Everything downstream (drawing, regions, the kernel) uses the cubic
//! Bézier spans of [`SplineData::spans`] (exact: the spline *is* those spans).

use serde::{Deserialize, Serialize};

use crate::{PointId, Sketch, Vec2};

/// A spline's definition (see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplineData {
    /// The points it runs through, in order. A closed spline lists each once (it runs from the
    /// last back to the first).
    pub points: Vec<PointId>,
    #[serde(default)]
    pub periodic: bool,
    /// The derivative at the start (open splines), in mm per unit parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_tangent: Option<Vec2>,
    /// The derivative at the end (open splines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_tangent: Option<Vec2>,
}

/// One cubic Bézier span: its four control points.
pub type Bez = [Vec2; 4];

impl SplineData {
    pub fn open(points: Vec<PointId>) -> Self {
        Self { points, periodic: false, start_tangent: None, end_tangent: None }
    }

    /// The spline's spans for these point positions (see [`spans`]).
    pub fn spans(&self, s: &Sketch) -> Vec<Bez> {
        let pts: Vec<Vec2> = self.points.iter().map(|p| s.pos(*p)).collect();
        spans(&pts, self.periodic, self.start_tangent, self.end_tangent)
    }
}

/// The centripetal parameter lengths of the spans (summing to 1).
pub fn knots(pts: &[Vec2], periodic: bool) -> Vec<f64> {
    let n = pts.len();
    let spans = if periodic { n } else { n.saturating_sub(1) };
    let mut h: Vec<f64> = (0..spans).map(|i| pts[i].distance(pts[(i + 1) % n]).sqrt().max(1e-9)).collect();
    let total: f64 = h.iter().sum();
    if total > 0.0 {
        for v in &mut h {
            *v /= total;
        }
    }
    h
}

/// The derivative at each point (in the normalized parameter) of the C2 cubic through `pts`.
pub fn derivatives(pts: &[Vec2], periodic: bool, start: Option<Vec2>, end: Option<Vec2>) -> Vec<Vec2> {
    let n = pts.len();
    if n < 2 {
        return vec![Vec2::ZERO; n];
    }
    let h = knots(pts, periodic);
    // A (dense) linear system per coordinate: row i is the equation for D_i.
    let mut a = vec![vec![0.0; n]; n];
    let mut rhs = vec![Vec2::ZERO; n];
    let interior = |i: usize, im: usize, ip: usize, hm: f64, hi: f64, a: &mut Vec<Vec<f64>>, rhs: &mut Vec<Vec2>| {
        a[i][im] += hi;
        a[i][i] += 2.0 * (hm + hi);
        a[i][ip] += hm;
        rhs[i] = ((pts[ip] - pts[i]) * (hm / hi) + (pts[i] - pts[im]) * (hi / hm)) * 3.0;
    };
    if periodic {
        if n < 3 {
            return vec![Vec2::ZERO; n];
        }
        for i in 0..n {
            let im = (i + n - 1) % n;
            interior(i, im, (i + 1) % n, h[im], h[i], &mut a, &mut rhs);
        }
    } else {
        match start {
            Some(d) => {
                a[0][0] = 1.0;
                rhs[0] = d;
            }
            None => {
                a[0][0] = 2.0;
                a[0][1] = 1.0;
                rhs[0] = (pts[1] - pts[0]) * (3.0 / h[0]);
            }
        }
        for i in 1..n - 1 {
            interior(i, i - 1, i + 1, h[i - 1], h[i], &mut a, &mut rhs);
        }
        match end {
            Some(d) => {
                a[n - 1][n - 1] = 1.0;
                rhs[n - 1] = d;
            }
            None => {
                a[n - 1][n - 2] = 1.0;
                a[n - 1][n - 1] = 2.0;
                rhs[n - 1] = (pts[n - 1] - pts[n - 2]) * (3.0 / h[n - 2]);
            }
        }
    }
    solve(a, rhs).unwrap_or_else(|| {
        // Degenerate (coincident points): chords.
        (0..n).map(|i| pts[(i + 1).min(n - 1)] - pts[i.saturating_sub(1)]).collect()
    })
}

/// Gaussian elimination with partial pivoting, two right-hand sides at once.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<Vec2>) -> Option<Vec<Vec2>> {
    let n = b.len();
    for col in 0..n {
        let piv = (col..n).max_by(|i, j| a[*i][col].abs().total_cmp(&a[*j][col].abs()))?;
        if a[piv][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for r in col + 1..n {
            let f = a[r][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            let pivot_row = a[col].clone();
            for (x, p) in a[r][col..n].iter_mut().zip(&pivot_row[col..n]) {
                *x -= f * p;
            }
            let bc = b[col];
            b[r] -= bc * f;
        }
    }
    let mut x = vec![Vec2::ZERO; n];
    for r in (0..n).rev() {
        let mut acc = b[r];
        for c in r + 1..n {
            acc -= x[c] * a[r][c];
        }
        x[r] = acc / a[r][r];
    }
    x.iter().all(|v| v.x.is_finite() && v.y.is_finite()).then_some(x)
}

/// The cubic Bézier spans of the spline through `pts` (see the module docs).
pub fn spans(pts: &[Vec2], periodic: bool, start: Option<Vec2>, end: Option<Vec2>) -> Vec<Bez> {
    let n = pts.len();
    if n < 2 || (periodic && n < 3) {
        return Vec::new();
    }
    let h = knots(pts, periodic);
    let d = derivatives(pts, periodic, start, end);
    (0..h.len())
        .map(|i| {
            let j = (i + 1) % n;
            [pts[i], pts[i] + d[i] * (h[i] / 3.0), pts[j] - d[j] * (h[i] / 3.0), pts[j]]
        })
        .collect()
}

/// The handle positions of an open spline's ends: where the tangent handles are drawn, and
/// the derivative a handle at `pos` sets (the inverse).
pub fn handle_positions(pts: &[Vec2], start: Option<Vec2>, end: Option<Vec2>) -> Option<(Vec2, Vec2)> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let h = knots(pts, false);
    let d = derivatives(pts, false, start, end);
    Some((pts[0] + d[0] * (h[0] / 3.0), pts[n - 1] - d[n - 1] * (h[n - 2] / 3.0)))
}

/// The derivative a handle dragged to `pos` sets at the start (`at_start`) or end.
pub fn tangent_for_handle(pts: &[Vec2], at_start: bool, pos: Vec2) -> Option<Vec2> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let h = knots(pts, false);
    Some(if at_start { (pos - pts[0]) * (3.0 / h[0]) } else { (pts[n - 1] - pos) * (3.0 / h[n - 2]) })
}

// ---------------------------------------------------------------------------------------------
// Cubic Bézier geometry

pub fn bez_point(b: &Bez, t: f64) -> Vec2 {
    let u = 1.0 - t;
    b[0] * (u * u * u) + b[1] * (3.0 * u * u * t) + b[2] * (3.0 * u * t * t) + b[3] * (t * t * t)
}

pub fn bez_deriv(b: &Bez, t: f64) -> Vec2 {
    let u = 1.0 - t;
    (b[1] - b[0]) * (3.0 * u * u) + (b[2] - b[1]) * (6.0 * u * t) + (b[3] - b[2]) * (3.0 * t * t)
}

pub fn bez_second(b: &Bez, t: f64) -> Vec2 {
    let u = 1.0 - t;
    (b[2] - b[1] * 2.0 + b[0]) * (6.0 * u) + (b[3] - b[2] * 2.0 + b[1]) * (6.0 * t)
}

/// The unit tangent at `t` (falling back to the chord where the derivative vanishes).
pub fn bez_tangent(b: &Bez, t: f64) -> Vec2 {
    let d = bez_deriv(b, t);
    if d.length() > 1e-12 {
        return d.normalize();
    }
    let e = bez_second(b, t);
    if e.length() > 1e-12 {
        return if t < 0.5 { e.normalize() } else { -e.normalize() };
    }
    (b[3] - b[0]).normalize()
}

/// The part of the span from `t0` to `t1` (either order), itself a cubic Bézier.
pub fn bez_sub(b: &Bez, t0: f64, t1: f64) -> Bez {
    // Blossoming: the control points of [t0, t1] are f(t0,t0,t0), f(t0,t0,t1), f(t0,t1,t1), f(t1,t1,t1).
    let blossom = |a: f64, c: f64, e: f64| {
        let l = |p: Vec2, q: Vec2, t: f64| p + (q - p) * t;
        let p1 = [l(b[0], b[1], a), l(b[1], b[2], a), l(b[2], b[3], a)];
        let p2 = [l(p1[0], p1[1], c), l(p1[1], p1[2], c)];
        l(p2[0], p2[1], e)
    };
    [blossom(t0, t0, t0), blossom(t0, t0, t1), blossom(t0, t1, t1), blossom(t1, t1, t1)]
}

pub fn bez_reversed(b: &Bez) -> Bez {
    [b[3], b[2], b[1], b[0]]
}

/// `½∫(x dy − y dx)` along the span (exact: 4-point Gauss–Legendre on a degree-5 polynomial).
pub fn bez_area_term(b: &Bez) -> f64 {
    const X: [f64; 4] = [-0.861_136_311_594_052_6, -0.339_981_043_584_856_3, 0.339_981_043_584_856_3, 0.861_136_311_594_052_6];
    const W: [f64; 4] = [0.347_854_845_137_453_9, 0.652_145_154_862_546_1, 0.652_145_154_862_546_1, 0.347_854_845_137_453_9];
    let mut s = 0.0;
    for k in 0..4 {
        let t = 0.5 * (X[k] + 1.0);
        s += W[k] * 0.5 * bez_point(b, t).cross(bez_deriv(b, t));
    }
    s / 2.0
}

/// The span's length (Gauss–Legendre on 8 sub-intervals).
pub fn bez_length(b: &Bez) -> f64 {
    const X: [f64; 4] = [-0.861_136_311_594_052_6, -0.339_981_043_584_856_3, 0.339_981_043_584_856_3, 0.861_136_311_594_052_6];
    const W: [f64; 4] = [0.347_854_845_137_453_9, 0.652_145_154_862_546_1, 0.652_145_154_862_546_1, 0.347_854_845_137_453_9];
    let m = 8;
    let mut s = 0.0;
    for i in 0..m {
        let (a, c) = (i as f64 / m as f64, (i + 1) as f64 / m as f64);
        for k in 0..4 {
            let t = a + (c - a) * 0.5 * (X[k] + 1.0);
            s += W[k] * 0.5 * (c - a) * bez_deriv(b, t).length();
        }
    }
    s
}

/// Points along the span, both ends included: at least `min` segments, more where it bends.
pub fn bez_tessellate(b: &Bez, min: usize) -> Vec<Vec2> {
    // Segments from the control polygon's turning (a good bound on the curve's).
    let turn: f64 = (0..2)
        .map(|i| {
            let (u, v) = (b[i + 1] - b[i], b[i + 2] - b[i + 1]);
            if u.length() < 1e-12 || v.length() < 1e-12 { 0.0 } else { u.cross(v).atan2(u.dot(v)).abs() }
        })
        .sum();
    let n = ((turn / (std::f64::consts::PI / 72.0)).ceil() as usize).max(min.max(1)).min(min.max(256));
    (0..=n).map(|i| bez_point(b, i as f64 / n as f64)).collect()
}

/// The parameter of the span's point nearest `p`, and the distance.
pub fn bez_nearest(b: &Bez, p: Vec2) -> (f64, f64) {
    const N: usize = 32;
    let mut best = (0.0, f64::INFINITY);
    for i in 0..=N {
        let t = i as f64 / N as f64;
        let d = bez_point(b, t).distance(p);
        if d < best.1 {
            best = (t, d);
        }
    }
    // Newton on (B(t) − p)·B'(t) = 0.
    let mut t = best.0;
    for _ in 0..8 {
        let q = bez_point(b, t) - p;
        let d1 = bez_deriv(b, t);
        let d2 = bez_second(b, t);
        let f = q.dot(d1);
        let df = d1.dot(d1) + q.dot(d2);
        if df.abs() < 1e-300 {
            break;
        }
        t = (t - f / df).clamp(0.0, 1.0);
    }
    let d = bez_point(b, t).distance(p);
    if d < best.1 { (t, d) } else { best }
}

/// The span's bounding box (of its control points, a superset of the curve's).
pub fn bez_bounds(b: &Bez) -> (Vec2, Vec2) {
    (b[0].min(b[1]).min(b[2]).min(b[3]), b[0].max(b[1]).max(b[2]).max(b[3]))
}

/// Points along whole spans (each span's points, the joints once).
pub fn tessellate(spans: &[Bez], min_per_span: usize) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for b in spans {
        let pts = bez_tessellate(b, min_per_span);
        let skip = usize::from(!out.is_empty());
        out.extend(pts.into_iter().skip(skip));
    }
    out
}

/// The nearest point on the spans to `p`: (span, parameter, distance).
pub fn nearest(spans: &[Bez], p: Vec2) -> Option<(usize, f64, f64)> {
    spans
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (t, d) = bez_nearest(b, p);
            (i, t, d)
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// The total length of the spans.
pub fn length(spans: &[Bez]) -> f64 {
    spans.iter().map(bez_length).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2, eps: f64) -> bool {
        a.distance(b) < eps
    }

    #[test]
    fn open_spline_runs_through_its_points_with_c2_joints() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 5.0), Vec2::new(20.0, -3.0), Vec2::new(35.0, 2.0)];
        let sp = spans(&pts, false, None, None);
        assert_eq!(sp.len(), 3);
        let h = knots(&pts, false);
        for (i, b) in sp.iter().enumerate() {
            assert!(close(b[0], pts[i], 1e-12) && close(b[3], pts[i + 1], 1e-12));
        }
        for i in 0..2 {
            // Derivatives in the global parameter match at the joints (C1), and second ones (C2).
            let d0 = bez_deriv(&sp[i], 1.0) / h[i];
            let d1 = bez_deriv(&sp[i + 1], 0.0) / h[i + 1];
            assert!(close(d0, d1, 1e-9), "{d0:?} {d1:?}");
            let s0 = bez_second(&sp[i], 1.0) / (h[i] * h[i]);
            let s1 = bez_second(&sp[i + 1], 0.0) / (h[i + 1] * h[i + 1]);
            assert!(close(s0, s1, 1e-6), "{s0:?} {s1:?}");
        }
        // Natural ends.
        assert!(bez_second(&sp[0], 0.0).length() < 1e-9);
        assert!(bez_second(&sp[2], 1.0).length() < 1e-9);
    }

    #[test]
    fn two_points_make_a_line() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        let sp = spans(&pts, false, None, None);
        assert!((length(&sp) - 10.0).abs() < 1e-9);
        assert!(bez_point(&sp[0], 0.5).y.abs() < 1e-12);
    }

    #[test]
    fn periodic_spline_through_a_square_is_near_a_circle() {
        // Eight points on a circle of radius 10: a closed spline close to the circle.
        let pts: Vec<Vec2> = (0..8).map(|k| Vec2::from_angle(k as f64 * std::f64::consts::TAU / 8.0) * 10.0).collect();
        let sp = spans(&pts, true, None, None);
        assert_eq!(sp.len(), 8);
        assert!(close(sp[7][3], pts[0], 1e-12));
        let area: f64 = sp.iter().map(bez_area_term).sum();
        let circle = std::f64::consts::PI * 100.0;
        assert!((area - circle).abs() / circle < 2e-3, "{area}");
        assert!((length(&sp) - std::f64::consts::TAU * 10.0).abs() < 0.05);
        for b in &sp {
            assert!((bez_point(b, 0.5).length() - 10.0).abs() < 0.03);
        }
    }

    #[test]
    fn onshape_handles_match() {
        // The Onshape segment of innerdoor_honda92's Sketch 2 (metres): its handles are the
        // Bézier points next to the ends.
        let pts = [
            Vec2::new(0.009505211375653744, 0.0896020233631134),
            Vec2::new(-0.0029987390153110027, 0.03581143915653229),
            Vec2::new(0.009741135872900486, -0.022225765511393547),
        ];
        let sd = Vec2::new(-0.03786702121308697, -0.10850606775282601);
        let ed = Vec2::new(0.03787686553832734, -0.11510923356507317);
        let (hs, he) = handle_positions(&pts, Some(sd), Some(ed)).unwrap();
        assert!(close(hs, Vec2::new(0.003309527329263405, 0.07184859857879489), 1e-9), "{hs:?}");
        assert!(close(he, Vec2::new(0.0033128087710101997, -0.0026898351938381302), 1e-9), "{he:?}");
        let back = tangent_for_handle(&pts, true, hs).unwrap();
        assert!(close(back, sd, 1e-12));
    }

    #[test]
    fn sub_spans_and_nearest() {
        let b = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 2.0), Vec2::new(4.0, 0.0)];
        let s = bez_sub(&b, 0.25, 0.75);
        assert!(close(s[0], bez_point(&b, 0.25), 1e-12));
        assert!(close(s[3], bez_point(&b, 0.75), 1e-12));
        assert!(close(bez_point(&s, 0.5), bez_point(&b, 0.5), 1e-12));
        let (t, d) = bez_nearest(&b, bez_point(&b, 0.3) + Vec2::new(0.0, 0.01) * 0.0);
        assert!((t - 0.3).abs() < 1e-6 && d < 1e-9);
        // The area term of a closed loop: the span and the chord back.
        let a = bez_area_term(&b) + b[3].cross(b[0]) / 2.0;
        // Area under the curve y(t) against x: the parabola-like hump (exact by the formula
        // 3/10·... checked against fine sampling).
        let pts = bez_tessellate(&b, 2000);
        let mut poly = 0.0;
        for w in pts.windows(2) {
            poly += w[0].cross(w[1]) / 2.0;
        }
        poly += b[3].cross(b[0]) / 2.0;
        assert!((a - poly).abs() < 1e-5, "{a} {poly}");
    }

    #[test]
    fn spline_regions() {
        use crate::SketchOp;
        // A closed spline on its own is a region, with the spline's exact area.
        let mut s = Sketch::new();
        let pts: Vec<Vec2> = (0..8).map(|k| Vec2::from_angle(k as f64 * std::f64::consts::TAU / 8.0) * 10.0).collect();
        SketchOp::AddSpline { points: pts.clone(), periodic: true, start_tangent: None, end_tangent: None, construction: false }
            .apply(&mut s)
            .unwrap();
        let exact: f64 = spans(&pts, true, None, None).iter().map(bez_area_term).sum();
        let r = crate::region::regions(&s);
        assert_eq!(r.len(), 1);
        assert!((r[0].area() - exact).abs() < 1e-9, "{} {exact}", r[0].area());
        assert_eq!(s.points.len(), 8);

        // An open spline closed by a line; a line across it splits the region in two.
        let mut s = Sketch::new();
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 8.0), Vec2::new(20.0, 0.0)];
        SketchOp::AddSpline { points: pts.to_vec(), periodic: false, start_tangent: None, end_tangent: None, construction: false }
            .apply(&mut s)
            .unwrap();
        s.add_line(Vec2::new(20.0, 0.0), Vec2::new(0.0, 0.0));
        let whole = crate::region::regions(&s);
        assert_eq!(whole.len(), 1);
        let a = whole[0].area();
        let exact: f64 = spans(&pts, false, None, None).iter().map(bez_area_term).sum::<f64>();
        assert!((a - exact.abs()).abs() < 1e-9, "{a} {exact}");
        s.add_line(Vec2::new(5.0, -1.0), Vec2::new(5.0, 20.0));
        let split = crate::region::regions(&s);
        assert_eq!(split.len(), 2);
        let sum: f64 = split.iter().map(|r| r.area()).sum();
        assert!((sum - a).abs() < 1e-6, "{sum} {a}");

        // Removing the spline removes its points (the ends the line shares stay).
        let sp = s.curves.iter().find(|(_, c)| matches!(c.kind, crate::CurveKind::Spline { .. })).unwrap().0;
        s.remove_curve(sp);
        assert!(s.splines.is_empty());
        assert!(s.point_at(Vec2::new(10.0, 8.0), 1e-9).is_none());
        assert!(s.point_at(Vec2::new(0.0, 0.0), 1e-9).is_some());
    }

    #[test]
    fn spline_survives_serde_and_dragging() {
        use crate::SketchOp;
        let mut s = Sketch::new();
        SketchOp::AddSpline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 8.0), Vec2::new(20.0, 0.0)],
            periodic: false,
            start_tangent: Some(Vec2::new(0.0, 30.0)),
            end_tangent: None,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let text = ron::to_string(&s).unwrap();
        let back: Sketch = ron::from_str(&text).unwrap();
        assert_eq!(back, s);
        let mid = s.point_at(Vec2::new(10.0, 8.0), 1e-9).unwrap();
        SketchOp::MovePoints { moves: vec![(mid, Vec2::new(10.0, 12.0))] }.apply(&mut s).unwrap();
        let id = s.splines.keys().next().unwrap();
        let sp = s.spline_spans(id).unwrap();
        assert!(sp[0][3].distance(Vec2::new(10.0, 12.0)) < 1e-9);
        let d = bez_deriv(&sp[0], 0.0) / knots(&s.splines[id].points.iter().map(|p| s.pos(*p)).collect::<Vec<_>>(), false)[0];
        assert!(d.distance(Vec2::new(0.0, 30.0)) < 1e-9);
    }
}
