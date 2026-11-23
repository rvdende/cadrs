//! Convex polytopes: the solids that are where half-spaces meet, kept as their faces' polygons.
//!
//! Cutting one by a plane is exact and takes no search: each face's polygon is clipped by the
//! plane, and the points left on the plane, in order round it, are the new face. So where a
//! solid is known to be the meeting of half-spaces (a box's walls moved in for a shell's
//! cavity, a tool that is planes only), it is made here in closed form, where the general
//! boolean of the body with one half-space after another intersects, splits and classifies
//! every face each time.

use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::topo::{Body, Coedge, Face, Shell, Solid};

/// A face of a polytope: its plane (a point of it and its unit normal, out of the solid) and its
/// corners, counter-clockwise about the normal.
#[derive(Debug, Clone)]
struct Flat {
    o: P3,
    n: V3,
    ring: Vec<P3>,
}

/// A convex polytope (see the module docs).
#[derive(Debug, Clone)]
pub(crate) struct Polytope {
    faces: Vec<Flat>,
}

/// The polygon's corners less those within `eps` of the one before (round the ring).
fn distinct(ring: &mut Vec<P3>, eps: f64) {
    ring.dedup_by(|a, b| (*a - *b).norm() <= eps);
    while ring.len() > 1 && (ring[0] - ring[ring.len() - 1]).norm() <= eps {
        ring.pop();
    }
}

/// Twice the polygon's area about `n`.
fn area2(ring: &[P3], n: &V3) -> f64 {
    let o = ring[0];
    (1..ring.len().saturating_sub(1))
        .map(|i| (ring[i] - o).cross(&(ring[i + 1] - o)).dot(n))
        .sum()
}

impl Polytope {
    /// The box `bb`.
    pub(crate) fn of_box(bb: &Bbox) -> Self {
        let (a, b) = (bb.min, bb.max);
        let p = |i: usize| {
            P3::new(
                if i & 1 == 0 { a.x } else { b.x },
                if i & 2 == 0 { a.y } else { b.y },
                if i & 4 == 0 { a.z } else { b.z },
            )
        };
        // (Each face's corners counter-clockwise seen from outside.)
        let sides: [([usize; 4], V3); 6] = [
            ([0, 4, 6, 2], -V3::x()),
            ([1, 3, 7, 5], V3::x()),
            ([0, 1, 5, 4], -V3::y()),
            ([2, 6, 7, 3], V3::y()),
            ([0, 2, 3, 1], -V3::z()),
            ([4, 5, 7, 6], V3::z()),
        ];
        Polytope {
            faces: sides
                .iter()
                .map(|(c, n)| Flat {
                    o: p(c[0]),
                    n: *n,
                    ring: c.iter().map(|&i| p(i)).collect(),
                })
                .collect(),
        }
    }

    /// The body as a polytope, where it is one: a single solid of flat faces with straight edges,
    /// one loop each, every vertex behind every face's plane (within `eps`).
    pub(crate) fn of_body(b: &Body, eps: f64) -> Option<Self> {
        if b.solids.len() != 1 || b.shells.len() != 1 || !b.shells[0].closed || b.faces.len() < 4 {
            return None;
        }
        let mut faces = Vec::with_capacity(b.faces.len());
        for f in &b.faces {
            let Surface::Plane { f: fr } = &f.surf else {
                return None;
            };
            let [lp] = f.loops.as_slice() else {
                return None;
            };
            let mut ring = Vec::with_capacity(lp.len());
            for c in lp {
                let e = &b.edges[c.edge];
                if e.degenerate || !matches!(e.curve, Curve::Line { .. }) {
                    return None;
                }
                ring.push(b.verts[e.v[usize::from(!c.fwd)]].p);
            }
            let n = fr.z.normalize() * if f.sense { 1.0 } else { -1.0 };
            faces.push(Flat {
                o: ring[0],
                n,
                ring,
            });
        }
        let convex = faces
            .iter()
            .all(|f| b.verts.iter().all(|v| (v.p - f.o).dot(&f.n) <= eps));
        convex.then_some(Polytope { faces })
    }

    /// Cuts the polytope by the plane through `o` with normal `n`, keeping what lies behind it
    /// (against the normal); points within `eps` of the plane are on it. False when nothing with
    /// a volume is left (the polytope is then not to be used).
    pub(crate) fn clip(&mut self, o: P3, n: V3, eps: f64) -> bool {
        let n = n.normalize();
        let side = |p: &P3| {
            let d = (p - o).dot(&n);
            if d.abs() <= eps { 0.0 } else { d }
        };
        if self
            .faces
            .iter()
            .all(|f| f.ring.iter().all(|p| side(p) <= 0.0))
        {
            return true;
        }
        // The points left on the plane: the new face's corners.
        let mut on: Vec<P3> = Vec::new();
        let mut kept = Vec::with_capacity(self.faces.len() + 1);
        for f in self.faces.drain(..) {
            let m = f.ring.len();
            let mut ring = Vec::with_capacity(m + 2);
            for i in 0..m {
                let (p, q) = (f.ring[i], f.ring[(i + 1) % m]);
                let (dp, dq) = (side(&p), side(&q));
                if dp <= 0.0 {
                    ring.push(p);
                    if dp == 0.0 {
                        on.push(p);
                    }
                }
                if dp * dq < 0.0 {
                    let x = p + (q - p) * (dp / (dp - dq));
                    ring.push(x);
                    on.push(x);
                }
            }
            distinct(&mut ring, eps);
            if ring.len() >= 3 && area2(&ring, &f.n) > eps * eps {
                kept.push(Flat {
                    o: f.o,
                    n: f.n,
                    ring,
                });
            }
        }
        // In order round the plane's normal about their middle (the cut of a convex solid is
        // convex).
        let mut cap: Vec<P3> = Vec::new();
        for p in on {
            if cap.iter().all(|q| (q - p).norm() > eps) {
                cap.push(p);
            }
        }
        if cap.len() >= 3 {
            let mid = P3::from(cap.iter().map(|p| p.coords).sum::<V3>() / cap.len() as f64);
            let x = any_perpendicular(&n).normalize();
            let y = n.cross(&x);
            let angle = |p: &P3| (p - mid).dot(&y).atan2((p - mid).dot(&x));
            cap.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
            if area2(&cap, &n) > eps * eps {
                kept.push(Flat { o, n, ring: cap });
            }
        }
        self.faces = kept;
        self.faces.len() >= 4
    }

    /// The polytope as a solid body: a flat face a polygon, their corners and edges shared.
    /// None when its faces don't close (a cut left a sliver thinner than `eps`).
    pub(crate) fn body(&self, eps: f64) -> Option<Body> {
        let mut b = Body::default();
        let mut edges: std::collections::HashMap<(usize, usize), usize> = Default::default();
        for f in &self.faces {
            let surf = Surface::Plane {
                f: Frame::from_z(f.o, f.n),
            };
            let vs: Vec<usize> = f
                .ring
                .iter()
                .map(
                    |p| match b.verts.iter().position(|v| (v.p - p).norm() <= eps) {
                        Some(i) => i,
                        None => b.add_vertex(*p),
                    },
                )
                .collect();
            let m = vs.len();
            let mut lp = Vec::with_capacity(m);
            for i in 0..m {
                let (v, w) = (vs[i], vs[(i + 1) % m]);
                if v == w {
                    return None;
                }
                let (edge, fwd) = match edges.get(&(w, v)) {
                    Some(&e) => (e, false),
                    None => {
                        if edges.contains_key(&(v, w)) {
                            return None;
                        }
                        let (p, q) = (b.verts[v].p, b.verts[w].p);
                        let e = b.add_edge(Curve::Line { o: p, d: q - p }, 0.0, 1.0, v, w);
                        edges.insert((v, w), e);
                        (e, true)
                    }
                };
                let pc = super::pcurve::pcurve(&surf, &b.edges[edge].curve, 0.0, 1.0, None);
                lp.push(Coedge { edge, fwd, pc });
            }
            b.faces.push(Face {
                surf,
                sense: true,
                loops: vec![lp],
            });
        }
        // Closed: every edge used once each way.
        let mut uses = vec![0u8; b.edges.len()];
        for c in b.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
            uses[c.edge] += 1;
        }
        if uses.iter().any(|&u| u != 2) {
            return None;
        }
        b.shells.push(Shell {
            faces: (0..b.faces.len()).collect(),
            closed: true,
        });
        b.solids.push(Solid { shells: vec![0] });
        Some(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> Polytope {
        Polytope::of_box(&Bbox {
            min: P3::new(0.0, 0.0, 0.0),
            max: P3::new(10.0, 8.0, 6.0),
        })
    }

    #[test]
    fn a_box_is_a_sound_solid() {
        let b = unit_box().body(1e-9).unwrap();
        assert_eq!((b.faces.len(), b.edges.len(), b.verts.len()), (6, 12, 8));
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert!((crate::native::mass::body_volume(&b) - 480.0).abs() < 1e-9);
    }

    #[test]
    fn a_corner_cut_off_adds_a_triangle() {
        let mut p = unit_box();
        // the corner at (10, 8, 6), cut by the plane through the points 2 from it along its edges
        let n = V3::new(1.0, 1.0, 1.0);
        assert!(p.clip(P3::new(8.0, 8.0, 6.0), n, 1e-9));
        let b = p.body(1e-9).unwrap();
        assert_eq!((b.faces.len(), b.edges.len(), b.verts.len()), (7, 15, 10));
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert!((crate::native::mass::body_volume(&b) - (480.0 - 8.0 / 6.0)).abs() < 1e-9);
    }

    #[test]
    fn a_cut_along_a_face_changes_nothing_and_one_past_it_leaves_nothing() {
        let mut p = unit_box();
        assert!(p.clip(P3::new(0.0, 0.0, 6.0), V3::z(), 1e-9));
        assert_eq!(p.faces.len(), 6);
        assert!(!p.clip(P3::new(0.0, 0.0, 0.0), V3::z(), 1e-9));
    }

    #[test]
    fn a_box_body_is_read_back_and_cut() {
        let b = unit_box().body(1e-9).unwrap();
        let mut p = Polytope::of_body(&b, 1e-9).unwrap();
        assert!(p.clip(P3::new(5.0, 0.0, 0.0), V3::x(), 1e-9));
        let half = p.body(1e-9).unwrap();
        assert!((crate::native::mass::body_volume(&half) - 240.0).abs() < 1e-9);
    }
}
