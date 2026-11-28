//! A face's trimmed domain in (u, v): its loops as uv polygons (`classify::face_polygons`) and
//! their box, with the point and box tests the face queries (silhouettes, hidden lines,
//! distances) clip and prune by.

use super::classify::{InFace, in_face};
use super::math::*;
use super::surface::Surface;
use super::tol;

/// A face's trimmed domain: its uv polygons and their box.
pub struct FaceDomain<'a> {
    pub surf: &'a Surface,
    pub polys: &'a [Vec<P2>],
    pub ub: (f64, f64),
    pub vb: (f64, f64),
    /// Loops that enclose no area (slits only: a seam and a cut, as some SMLib files bound a
    /// whole sphere) bound the whole of their box, as SMLib reads them.
    pub whole: bool,
}

impl<'a> FaceDomain<'a> {
    pub fn new(surf: &'a Surface, polys: &'a [Vec<P2>]) -> Self {
        let mut ub = (f64::INFINITY, f64::NEG_INFINITY);
        let mut vb = (f64::INFINITY, f64::NEG_INFINITY);
        for q in polys.iter().flatten() {
            ub = (ub.0.min(q.x), ub.1.max(q.x));
            vb = (vb.0.min(q.y), vb.1.max(q.y));
        }
        let area: f64 = polys.iter().map(|p| polygon_area(p)).sum();
        let boxed = (ub.1 - ub.0) * (vb.1 - vb.0);
        let whole = boxed > 0.0 && area.abs() < tol::REL_FINE * boxed;
        Self {
            surf,
            polys,
            ub,
            vb,
            whole,
        }
    }

    /// No area: no loops, or loops that are all one line.
    pub fn is_empty(&self) -> bool {
        !(self.ub.0 < self.ub.1 && self.vb.0 <= self.vb.1)
    }

    /// Whether (u, v), shifted by whole periods, lies in the face (on its boundary too unless
    /// `strict`). The boundary is a band of about 1e-3 mm (`in_face` at 1e-6, squared and
    /// scaled by the surface's metric).
    pub fn contains(&self, u: f64, v: f64, strict: bool) -> bool {
        if self.is_empty() {
            return false;
        }
        if self.whole {
            let inb = |x: f64, (lo, hi): (f64, f64), periodic: bool| {
                let x = if periodic {
                    lo + (x - lo).rem_euclid(TAU)
                } else {
                    x
                };
                x >= lo - tol::ROUNDOFF
                    && x <= hi + tol::ROUNDOFF
                    && !(strict && (x - lo < tol::ROUNDOFF || hi - x < tol::ROUNDOFF))
            };
            return inb(u, self.ub, self.surf.u_periodic())
                && inb(v, self.vb, self.surf.v_periodic());
        }
        let shifts = |x: f64, (lo, hi): (f64, f64), periodic: bool| -> Vec<f64> {
            let ks: &[f64] = if periodic {
                &[0.0, -1.0, 1.0, -2.0, 2.0]
            } else {
                &[0.0]
            };
            ks.iter()
                .map(|k| x + TAU * k)
                .filter(|y| *y >= lo - tol::ROUNDOFF && *y <= hi + tol::ROUNDOFF)
                .collect()
        };
        for u in shifts(u, self.ub, self.surf.u_periodic()) {
            for v in shifts(v, self.vb, self.surf.v_periodic()) {
                match in_face(self.polys, self.surf, P2::new(u, v), tol::point()) {
                    InFace::Inside => return true,
                    InFace::Boundary if !strict => return true,
                    _ => {}
                }
            }
        }
        false
    }

    /// Whether the uv rectangle [u0, u1] × [v0, v1] (inside the domain's box) meets the face:
    /// a loop vertex inside it, a loop segment crossing it, or the face around its centre.
    pub fn meets_box(&self, u0: f64, u1: f64, v0: f64, v1: f64) -> bool {
        if self.whole {
            return true;
        }
        let inb = |q: &P2| q.x >= u0 && q.x <= u1 && q.y >= v0 && q.y <= v1;
        for p in self.polys {
            for (i, a) in p.iter().enumerate() {
                if inb(a) {
                    return true;
                }
                let b = &p[(i + 1) % p.len()];
                if segment_meets_box(a, b, u0, u1, v0, v1) {
                    return true;
                }
            }
        }
        self.contains(0.5 * (u0 + u1), 0.5 * (v0 + v1), false)
    }
}

/// Whether the segment ab meets the box (Liang–Barsky clipping).
fn segment_meets_box(a: &P2, b: &P2, u0: f64, u1: f64, v0: f64, v1: f64) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - u0),
        (d.x, u1 - a.x),
        (-d.y, a.y - v0),
        (d.y, v1 - a.y),
    ] {
        if p.abs() < tol::TINY {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}
