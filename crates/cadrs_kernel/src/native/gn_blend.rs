//! The blend section of order 2 (E. Hartmann, "Parametric Gⁿ blending of curves and surfaces",
//! The Visual Computer 17, 2001, pp. 1–13): a curvature-continuous fillet section between two
//! lines through an apex, shaped by its thumbweight `w < 1`.
//!
//! Between the legs `A = a − apex` and `B = b − apex` the section is `apex + α(t)·A + β(t)·B`,
//! `t ∈ [0, 1]`, with `α = f·(1 − p₁)`, `β = (1 − f)·p₂`, `f = t̄³/(t̄³ + t³)`,
//! `p₁ = t/(1 − w + wt)`, `p₂ = t(1 − w)/(1 − wt)` and `t̄ = 1 − t`. Since `t̄³ + t³ = 1 − 3t + 3t²`
//! and `1 − p₁ = (1 − w)·t̄/(1 − w + wt)`, the weights share one denominator,
//!
//! `α = t̄⁴(1 − w)(1 − wt)/D`, `β = t⁴(1 − w)(1 − w + wt)/D`,
//! `D = (1 − 3t + 3t²)(1 − w + wt)(1 − wt)`,
//!
//! so the section is exactly a rational Bézier curve of degree 5 ([`section`]). It leaves each leg
//! with contact of order 2: as straight as the line it leaves. Its curvature at the middle falls
//! from unbounded to zero as `w` rises from −∞ to 1 ([`center_w`] finds the `w` for a radius), and
//! the share of the triangle on its legs it takes off falls likewise ([`equal_area_w`] finds the
//! `w` at which it takes off what the circle tangent at the same contacts does).

use super::math::*;
use super::nurbs::{H, NurbsCurve};
use crate::{KernelError, Result};

/// A polynomial in the power basis, lowest coefficient first.
type Poly = Vec<f64>;

fn mul(a: &[f64], b: &[f64]) -> Poly {
    let mut r = vec![0.0; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            r[i + j] += x * y;
        }
    }
    r
}

fn eval(a: &[f64], t: f64) -> f64 {
    a.iter().rev().fold(0.0, |s, c| s * t + c)
}

fn deriv(a: &[f64]) -> Poly {
    a.iter()
        .enumerate()
        .skip(1)
        .map(|(i, c)| i as f64 * c)
        .collect()
}

fn binom(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |r, i| r * (n - i) as f64 / (i + 1) as f64)
}

/// The Bernstein coefficients of degree `n` of a power-basis polynomial of degree at most `n`:
/// `bᵢ = Σ_{j ≤ i} C(i, j)/C(n, j)·aⱼ`.
fn bernstein(a: &[f64], n: usize) -> Vec<f64> {
    (0..=n)
        .map(|i| {
            (0..=i.min(a.len().saturating_sub(1)))
                .map(|j| binom(i, j) / binom(n, j) * a[j])
                .sum()
        })
        .collect()
}

/// The numerators of `α` and `β` and their common denominator `D`, in the power basis.
fn parts(w: f64) -> (Poly, Poly, Poly) {
    let (s, u) = (1.0 - w, w);
    // 1 − w + wt and 1 − wt.
    let (p, q) = ([s, u], [1.0, -u]);
    let tbar4 = mul(
        &mul(&[1.0, -1.0], &[1.0, -1.0]),
        &mul(&[1.0, -1.0], &[1.0, -1.0]),
    );
    let t4 = [0.0, 0.0, 0.0, 0.0, 1.0];
    let na = mul(&tbar4, &q).iter().map(|c| c * s).collect();
    let nb = mul(&t4, &p).iter().map(|c| c * s).collect();
    let d = mul(&mul(&[1.0, -3.0, 3.0], &p), &q);
    (na, nb, d)
}

fn check_w(w: f64) -> Result<()> {
    if w.is_finite() && w < 1.0 {
        Ok(())
    } else {
        Err(KernelError::InvalidParameter(format!(
            "a blend section's thumbweight must be below 1 (got {w})"
        )))
    }
}

/// The section of thumbweight `w` from `a` to `b` between the lines from `apex` to each: the
/// rational quintic Bézier `apex·D + A·N_α + B·N_β` over `D` in the Bernstein basis (its weights
/// are positive for every `w < 1`), split at its middle. (`D`'s factor `1 − 3t + 3t²` has its
/// roots at `½ ± i/(2√3)`, close over the middle: on halves, quadratures of the faces it sweeps
/// converge some fifty times faster per point.)
pub fn section(a: P3, apex: P3, b: P3, w: f64) -> Result<NurbsCurve> {
    let mut c = bezier(a, apex, b, w)?;
    for _ in 0..5 {
        c = c.insert_knot(0.5);
    }
    c.validate()?;
    Ok(c)
}

/// [`section`] as one Bézier piece.
fn bezier(a: P3, apex: P3, b: P3, w: f64) -> Result<NurbsCurve> {
    check_w(w)?;
    let (na, nb, d) = parts(w);
    let (na, nb, d) = (bernstein(&na, 5), bernstein(&nb, 5), bernstein(&d, 5));
    let (la, lb) = (a - apex, b - apex);
    let cps: Vec<H> = (0..6)
        .map(|i| {
            let p = apex.coords * d[i] + la * na[i] + lb * nb[i];
            H::new(p.x, p.y, p.z, d[i])
        })
        .collect();
    let mut knots = vec![0.0; 6];
    knots.extend([1.0; 6]);
    NurbsCurve::new(5, knots, cps)
}

/// The legs' weights `[α, β]` of the section of thumbweight `w` at `t`.
pub fn legs_at(w: f64, t: f64) -> [f64; 2] {
    weights(w, t).0
}

/// The weights `(α, β)` at `t` and their first derivatives.
fn weights(w: f64, t: f64) -> ([f64; 2], [f64; 2]) {
    let (na, nb, d) = parts(w);
    let (dv, dd) = (eval(&d, t), eval(&deriv(&d), t));
    let f = |n: &Poly| {
        let v = eval(n, t);
        (v / dv, (eval(&deriv(n), t) * dv - v * dd) / (dv * dv))
    };
    let ((a, da), (b, db)) = (f(&na), f(&nb));
    ([a, b], [da, db])
}

/// The legs of a section with legs `d` long meeting at the angle `alpha`, in the plane: apex at
/// the origin.
fn legs(alpha: f64, d: f64) -> (P3, P3, P3) {
    (
        P3::new(d, 0.0, 0.0),
        P3::origin(),
        P3::new(d * alpha.cos(), d * alpha.sin(), 0.0),
    )
}

/// The radius of curvature at the middle (`t = ½`, by symmetry) of the section of thumbweight `w`
/// whose legs are `d` long and meet at the angle `alpha`: exact, from the rational curve's
/// derivatives.
pub fn center_radius(w: f64, alpha: f64, d: f64) -> Result<f64> {
    let (a, apex, b) = legs(alpha, d);
    let c = bezier(a, apex, b, w)?;
    let (_, d1, d2) = c.ders2(0.5);
    Ok(d1.norm().powi(3) / d1.cross(&d2).norm())
}

/// The thumbweight at which the section with legs `d` long meeting at `alpha` has radius of
/// curvature `center` at its middle. The middle radius falls from unbounded to zero as `w` rises
/// to 1: bisected (to rounding) on a bracket stepped down from −1 until it holds the radius.
pub fn center_w(alpha: f64, d: f64, center: f64) -> Result<f64> {
    if !(center > 0.0 && center.is_finite()) {
        return Err(KernelError::InvalidParameter(
            "The blend's centre radius must be positive".into(),
        ));
    }
    let (mut lo, mut hi) = (-1.0f64, 1.0 - 1e-9);
    if center_radius(hi, alpha, d)? > center {
        return Err(KernelError::InvalidParameter(format!(
            "no blend section between these faces curves as tightly as a centre radius of {center}"
        )));
    }
    let mut steps = 0;
    while center_radius(lo, alpha, d)? < center {
        hi = lo;
        lo = 2.0 * lo - 1.0;
        steps += 1;
        if steps > 60 {
            return Err(KernelError::InvalidParameter(format!(
                "no blend section between these faces is as flat as a centre radius of {center}"
            )));
        }
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if center_radius(mid, alpha, d)? > center {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo <= 1e-15 * (1.0 - lo) {
            break;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// The share of the triangle on its legs that the section of thumbweight `w` takes off,
/// `∫₀¹ αβ' − α'β dt` (the same for every angle and leg length): Gauss–Legendre, five points on
/// each of 64 panels, exact to rounding for this smooth rational integrand.
pub fn area_share(w: f64) -> f64 {
    const X: [f64; 5] = [
        -0.906_179_845_938_664,
        -0.538_469_310_105_683,
        0.0,
        0.538_469_310_105_683,
        0.906_179_845_938_664,
    ];
    const WT: [f64; 5] = [
        0.236_926_885_056_189,
        0.478_628_670_499_366,
        0.568_888_888_888_889,
        0.478_628_670_499_366,
        0.236_926_885_056_189,
    ];
    let panels = 64;
    let h = 1.0 / panels as f64;
    let mut sum = 0.0;
    for k in 0..panels {
        let mid = (k as f64 + 0.5) * h;
        for (x, g) in X.iter().zip(WT) {
            let (v, dv) = weights(w, mid + 0.5 * h * x);
            sum += g * 0.5 * h * (v[0] * dv[1] - dv[0] * v[1]);
        }
    }
    sum
}

/// The thumbweight at which the section in a wedge of interior angle `alpha` takes off what the
/// circle tangent at the same contacts does. The circle's share of the triangle on the legs is
/// `(cot(α/2) − (π − α)/2)/(cot(α/2)·cos²(α/2))`; the section's share falls as `w` rises, so the
/// root is bisected on a bracket stepped down from −1.
pub fn equal_area_w(alpha: f64) -> Result<f64> {
    if !(alpha > 0.0 && alpha < PI) {
        return Err(KernelError::InvalidParameter(
            "a blend section needs faces meeting at an angle".into(),
        ));
    }
    let c = 1.0 / (alpha / 2.0).tan();
    let target = (c - (PI - alpha) / 2.0) / (c * (alpha / 2.0).cos().powi(2));
    let (mut lo, mut hi) = (-1.0f64, 1.0 - 1e-9);
    let mut steps = 0;
    while area_share(lo) < target {
        hi = lo;
        lo = 2.0 * lo - 1.0;
        steps += 1;
        if steps > 60 {
            return Err(KernelError::InvalidParameter(
                "no blend section takes off what the round does here".into(),
            ));
        }
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if area_share(mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo <= 1e-15 * (1.0 - lo) {
            break;
        }
    }
    Ok(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quintic_is_the_section() {
        for w in [-4.0, -1.0, 0.0, 0.4, 0.9] {
            let (a, apex, b) = legs(1.2, 3.0);
            let c = section(a, apex, b, w).unwrap();
            assert!(
                bezier(a, apex, b, w).unwrap().cps.iter().all(|h| h.w > 0.0),
                "w = {w}"
            );
            for k in 0..=20 {
                let t = k as f64 / 20.0;
                let (v, _) = weights(w, t);
                let want = apex + (a - apex) * v[0] + (b - apex) * v[1];
                // And straight from Hartmann's form.
                let f = (1.0 - t).powi(3) / ((1.0 - t).powi(3) + t.powi(3));
                let (p1, p2) = (t / (1.0 - w + w * t), t * (1.0 - w) / (1.0 - w * t));
                let hart = apex + (a - apex) * (f * (1.0 - p1)) + (b - apex) * ((1.0 - f) * p2);
                assert!((c.point(t) - want).norm() < 1e-13);
                assert!((hart - want).norm() < 1e-13);
            }
            // Contact of order 2: no curvature where it meets each leg.
            for t in [0.0, 1.0] {
                let (_, d1, d2) = c.ders2(t);
                assert!(d1.cross(&d2).norm() < 1e-9 * d1.norm().powi(3));
            }
        }
    }

    #[test]
    fn center_and_area_thumbweights_invert() {
        let (alpha, d) = (PI / 2.0, 1.0);
        let w = center_w(alpha, d, 1.5).unwrap();
        assert!((center_radius(w, alpha, d).unwrap() - 1.5).abs() < 1e-12);
        // The equal-area section of a square corner takes off (1 − π/4)·r², and curves at its
        // middle with a radius of about 1.389 r.
        let w = equal_area_w(alpha).unwrap();
        assert!((area_share(w) * 0.5 - (1.0 - PI / 4.0)).abs() < 1e-13);
        let c = center_radius(w, alpha, d).unwrap();
        assert!((c - 1.389).abs() < 1e-3, "{c}");
    }
}
