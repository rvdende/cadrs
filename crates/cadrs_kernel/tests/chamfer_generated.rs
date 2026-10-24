//! Generated chamfer cases checked against exact answers computed independently of the kernel.
//!
//! - **Chamfered prisms**: random polygons (convex and with inside corners, at all sorts of
//!   angles, some with holes) extruded, their top outline chamfered. For a chamfer of distance d
//!   small enough that no corner's cuts run into another's, the material removed at depth below
//!   the top is the band within d of the outline, mitred at the corners (a straight skeleton), so
//!   the removed volume is ∫₀ᵈ (P·t − K·t²) dt = P·d²/2 − K·d³/3, with P the outline's length
//!   and K = Σ cot(θ/2) over its corners (θ the angle on the material's side: a convex corner
//!   takes some away, an inside one adds). The result has the prism's faces and one per chamfer.
//! - **Chamfered convex blocks**: boxes and regular prisms with random sets of edges chamfered,
//!   corners where three chamfers meet included. A convex solid's chamfer is the solid cut by
//!   the plane through the two lines d in from the edge on its faces; with several, by all of
//!   them. The expected solid is clipped from the block here, plane by plane, and compared by
//!   volume and number of faces.
//!
//! - **Chamfered rounded prisms**: regular polygons with their corners rounded R, extruded, their
//!   top outline chamfered d, narrower than R, as wide, a hair either side, and wider. At depth
//!   t below the top the section is the sharp polygon inset by t, A − P·t + K·t², its corners
//!   rounded ρ, which takes C·ρ² off (C = Σ (cot(θ/2) − (π − θ)/2)). Round a corner narrower
//!   than the chamfer the points d in from the outline are the sides' lines, meeting in a point,
//!   and the corner's chamfer a cone from there down to the round: ρ = R − t while d ≤ R, else
//!   R·(1 − t/d). The removed volume is P·d²/2 − K·d³/3 + C·∫₀ᵈ (ρ² − R²) dt.
//!
//! Each run prints every failing case with what it expected, so one run shows the whole picture.

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector2, Vector3};

type P2 = Point2<f64>;
type V3 = Vector3<f64>;

/// A small deterministic generator (the cases are the same every run).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
    fn int(&mut self, a: usize, b: usize) -> usize {
        a + ((self.next() * (b - a + 1) as f64) as usize).min(b - a)
    }
}

fn polygon(points: &[P2]) -> Loop {
    let n = points.len();
    Loop {
        curves: (0..n)
            .map(|i| Curve2::Line {
                a: points[i],
                b: points[(i + 1) % n],
                source: None,
            })
            .collect(),
    }
}

fn signed_area(p: &[P2]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y)
        .sum::<f64>()
        / 2.0
}

/// The interior angle at each vertex of a counter-clockwise polygon.
fn interior_angles(p: &[P2]) -> Vec<f64> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            let (u, v) = (b - a, c - b);
            let turn = (u.x * v.y - u.y * v.x).atan2(u.dot(&v));
            std::f64::consts::PI - turn
        })
        .collect()
}

/// A star-shaped polygon around the origin: its angles take convex and reflex corners alike.
fn star(rng: &mut Rng) -> Vec<P2> {
    let n = rng.int(4, 9);
    let mut angles: Vec<f64> = (0..n)
        .map(|i| (i as f64 + rng.range(0.15, 0.85)) / n as f64 * std::f64::consts::TAU)
        .collect();
    angles.sort_by(|a, b| a.total_cmp(b));
    angles
        .iter()
        .map(|&a| {
            let r = rng.range(14.0, 30.0);
            P2::new(r * a.cos(), r * a.sin())
        })
        .collect()
}

/// A rectangle with rectangular notches cut into its sides (right-angled inside corners).
fn notched(rng: &mut Rng) -> Vec<P2> {
    let (w, h) = (rng.range(40.0, 70.0), rng.range(30.0, 50.0));
    let (x0, y0) = (-w / 2.0, -h / 2.0);
    let mut pts = vec![P2::new(x0, y0)];
    // Bottom side, left to right, with a notch upwards.
    let a = rng.range(0.2, 0.4) * w;
    let b = a + rng.range(0.15, 0.3) * w;
    let depth = rng.range(0.2, 0.4) * h;
    pts.extend([
        P2::new(x0 + a, y0),
        P2::new(x0 + a, y0 + depth),
        P2::new(x0 + b, y0 + depth),
        P2::new(x0 + b, y0),
    ]);
    pts.push(P2::new(x0 + w, y0));
    // Right side with a notch inwards, sometimes.
    if rng.next() < 0.6 {
        let c = rng.range(0.2, 0.4) * h;
        let e = c + rng.range(0.15, 0.3) * h;
        let dx = rng.range(0.1, 0.25) * w;
        pts.extend([
            P2::new(x0 + w, y0 + c),
            P2::new(x0 + w - dx, y0 + c),
            P2::new(x0 + w - dx, y0 + e),
            P2::new(x0 + w, y0 + e),
        ]);
    }
    pts.push(P2::new(x0 + w, y0 + h));
    pts.push(P2::new(x0, y0 + h));
    pts
}

/// The largest chamfer the formula holds for: each edge keeps a stretch between its corners'
/// mitres, with room to spare.
fn safe_distance(loops: &[(Vec<P2>, Vec<f64>)]) -> f64 {
    let mut d = f64::INFINITY;
    for (p, ang) in loops {
        let n = p.len();
        for i in 0..n {
            let len = (p[(i + 1) % n] - p[i]).norm();
            let cot = |t: f64| (1.0 / (t / 2.0).tan()).abs();
            d = d.min(len / (cot(ang[i]) + cot(ang[(i + 1) % n]) + 1.0));
        }
    }
    d * 0.3
}

struct Case {
    name: String,
    failures: Vec<String>,
}

impl Case {
    fn check(&mut self, what: &str, ok: bool, detail: String) {
        if !ok {
            self.failures
                .push(format!("{}: {what}: {detail}", self.name));
        }
    }
}

#[test]
fn chamfered_prisms() {
    let mut rng = Rng(0x5eed_c4a3);
    let mut failures = Vec::new();
    let cases = 60;
    for i in 0..cases {
        let kind = i % 3;
        let mut outer = if kind == 1 {
            notched(&mut rng)
        } else {
            star(&mut rng)
        };
        if signed_area(&outer) < 0.0 {
            outer.reverse();
        }
        let mut loops: Vec<(Vec<P2>, Vec<f64>)> = vec![(outer.clone(), interior_angles(&outer))];
        let mut holes = Vec::new();
        if kind == 2 {
            // A small regular hole in the middle (its corners are inside corners of the part).
            let m = rng.int(3, 6);
            let r = rng.range(3.0, 6.0);
            let rot = rng.range(0.0, 1.0);
            let mut hole: Vec<P2> = (0..m)
                .map(|j| {
                    let a = rot + j as f64 / m as f64 * std::f64::consts::TAU;
                    P2::new(r * a.cos(), r * a.sin())
                })
                .collect();
            if signed_area(&hole) < 0.0 {
                hole.reverse();
            }
            // On the material's side the angles are the hole's outside ones.
            let ang = interior_angles(&hole)
                .into_iter()
                .map(|t| std::f64::consts::TAU - t)
                .collect();
            loops.push((hole.clone(), ang));
            holes.push(hole);
        }
        let d = safe_distance(&loops).min(2.0);
        let h = rng.range(8.0, 20.0);
        let perimeter: f64 = loops
            .iter()
            .map(|(p, _)| {
                (0..p.len())
                    .map(|j| (p[(j + 1) % p.len()] - p[j]).norm())
                    .sum::<f64>()
            })
            .sum();
        let k_sum: f64 = loops
            .iter()
            .flat_map(|(_, a)| a.iter())
            .map(|t| 1.0 / (t / 2.0).tan())
            .sum();
        let expected = perimeter * d * d / 2.0 - k_sum * d * d * d / 3.0;

        let mut case = Case {
            name: format!(
                "prism {i} ({} corners, {} holes, d {d:.3})",
                outer.len(),
                holes.len()
            ),
            failures: Vec::new(),
        };
        let mut k = NativeKernel::new();
        let profile = Profile::new(
            Plane::top(),
            vec![Region {
                outer: polygon(&outer),
                holes: holes.iter().map(|h| polygon(h)).collect(),
                source: None,
            }],
        );
        let b = match k.extrude(&profile, Extent::Blind(h)) {
            Ok(r) => r.bodies[0],
            Err(e) => {
                failures.push(format!("{}: extrude failed: {e}", case.name));
                continue;
            }
        };
        let edges: Vec<EdgeId> = k
            .edges(b)
            .unwrap()
            .iter()
            .filter(|e| (e.start.z - h).abs() < 1e-9 && (e.end.z - h).abs() < 1e-9)
            .map(|e| e.id)
            .collect();
        let n0 = k.faces(b).unwrap().len();
        let v0 = k.mass_properties(b).unwrap().volume;
        match k.chamfer_with(b, &edges, &ChamferOpts::new(ChamferSpec::EqualDistance(d))) {
            Err(e) => case.check("chamfer", false, e.to_string()),
            Ok(r) => {
                let out = r.bodies[0];
                let issues = k.get(out).unwrap().check();
                case.check("valid", issues.is_empty(), format!("{issues:?}"));
                let removed = v0 - k.mass_properties(out).unwrap().volume;
                case.check(
                    "volume",
                    (removed - expected).abs() < 1e-6 * (1.0 + expected),
                    format!("removed {removed:.9}, expected {expected:.9}"),
                );
                let nf = k.faces(out).unwrap().len();
                case.check(
                    "faces",
                    nf == n0 + edges.len(),
                    format!("{nf} faces, expected {}", n0 + edges.len()),
                );
            }
        }
        failures.extend(case.failures);
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------
// Convex blocks

/// A convex polyhedron as outward-oriented face polygons.
#[derive(Clone)]
struct Poly {
    faces: Vec<Vec<Point3<f64>>>,
}

impl Poly {
    /// Keeps the part where n·x ≤ c, closing the cut with a face.
    fn clip(&self, n: V3, c: f64) -> Poly {
        let side = |p: &Point3<f64>| n.dot(&p.coords) - c;
        let mut faces = Vec::new();
        let mut cut: Vec<Point3<f64>> = Vec::new();
        for f in &self.faces {
            let mut out = Vec::new();
            for i in 0..f.len() {
                let (a, b) = (f[i], f[(i + 1) % f.len()]);
                let (sa, sb) = (side(&a), side(&b));
                let eps = 1e-9;
                if sa <= eps {
                    out.push(a);
                }
                if sa.abs() <= eps {
                    // On the plane: a corner of the cut too.
                    cut.push(a);
                }
                if (sa < -eps && sb > eps) || (sa > eps && sb < -eps) {
                    let q = a + (b - a) * (sa / (sa - sb));
                    out.push(q);
                    cut.push(q);
                }
            }
            if out.len() >= 3 {
                faces.push(out);
            }
        }
        if cut.len() >= 3 {
            // Around the cut's centre, counter-clockwise about n (outward).
            let c0 = cut.iter().fold(V3::zeros(), |s, p| s + p.coords) / cut.len() as f64;
            let x = (cut[0].coords - c0).normalize();
            let y = n.normalize().cross(&x);
            cut.sort_by(|p, q| {
                let ang = |p: &Point3<f64>| (p.coords - c0).dot(&y).atan2((p.coords - c0).dot(&x));
                ang(p).total_cmp(&ang(q))
            });
            cut.dedup_by(|p, q| (*p - *q).norm() < 1e-9);
            if cut.len() >= 3 {
                faces.push(cut);
            }
        }
        Poly { faces }
    }

    fn volume(&self) -> f64 {
        self.faces
            .iter()
            .map(|f| {
                (1..f.len() - 1)
                    .map(|i| f[0].coords.dot(&f[i].coords.cross(&f[i + 1].coords)) / 6.0)
                    .sum::<f64>()
            })
            .sum()
    }

    /// Faces with some area.
    fn face_count(&self) -> usize {
        self.faces
            .iter()
            .filter(|f| {
                (1..f.len() - 1)
                    .map(|i| (f[i] - f[0]).cross(&(f[i + 1] - f[0])).norm() / 2.0)
                    .sum::<f64>()
                    > 1e-9
            })
            .count()
    }
}

/// A regular n-gon round (10, 10), of inradius `ri`, its corners rounded `r`: the outline, and
/// the sharp polygon's area, perimeter and K = Σ cot(θ/2) (θ its interior angle).
fn rounded_polygon(n: usize, ri: f64, r: f64) -> (Loop, f64, f64, f64) {
    use std::f64::consts::PI;
    let c0 = P2::new(10.0, 10.0);
    let turn = 2.0 * PI / n as f64;
    let theta = PI - turn;
    let rc = ri / (PI / n as f64).cos();
    let corner = |i: usize| {
        let a = turn * i as f64;
        c0 + Vector2::new(a.cos(), a.sin()) * rc
    };
    // Each side's line, cut back where the rounds start, and the round at its far corner.
    let cut = r / (theta / 2.0).tan();
    let mut curves = Vec::new();
    for i in 0..n {
        let (a, b) = (corner(i), corner(i + 1));
        let dir = (b - a).normalize();
        curves.push(Curve2::Line {
            a: a + dir * cut,
            b: b - dir * cut,
            source: None,
        });
        let centre = b + (c0 - b).normalize() * (r / (theta / 2.0).sin());
        let from = b - dir * cut - centre;
        curves.push(Curve2::Arc {
            center: centre,
            radius: r,
            start_angle: from.y.atan2(from.x),
            sweep: turn,
            source: None,
        });
    }
    let side = 2.0 * ri * (PI / n as f64).tan();
    let k = n as f64 / (theta / 2.0).tan();
    (
        Loop { curves },
        n as f64 * side * ri / 2.0,
        n as f64 * side,
        k,
    )
}

#[test]
fn chamfered_rounded_prisms() {
    use std::f64::consts::PI;
    let h = 10.0;
    let mut failures = Vec::new();
    let mut cases = 0;
    for (n, ri, r) in [(3, 6.0, 1.0), (4, 6.0, 1.5), (5, 6.0, 0.7), (6, 6.0, 1.0)] {
        let (outline, _, p, kk) = rounded_polygon(n, ri, r);
        let theta = PI - 2.0 * PI / n as f64;
        let c = n as f64 * (1.0 / (theta / 2.0).tan() - (PI - theta) / 2.0);
        for d in [0.5 * r, r - 1e-6, r, r + 1e-6, 1.5 * r, 3.0 * r] {
            cases += 1;
            let rounds = if d <= r {
                d * d * d / 3.0 - r * d * d
            } else {
                -2.0 * r * r * d / 3.0
            };
            let want = p * d * d / 2.0 - kk * d * d * d / 3.0 + c * rounds;
            let mut k = NativeKernel::new();
            let profile = Profile::new(
                Plane::top(),
                vec![Region {
                    outer: outline.clone(),
                    holes: vec![],
                    source: None,
                }],
            );
            let body = k.extrude(&profile, Extent::Blind(h)).unwrap().bodies[0];
            let v0 = k.mass_properties(body).unwrap().volume;
            let top: Vec<EdgeId> = k
                .edges(body)
                .unwrap()
                .iter()
                .filter(|e| (e.mid.z - h).abs() < 1e-6)
                .map(|e| e.id)
                .collect();
            // By the local chamfer alone (the blend tool's, matching by chance, would hide a
            // broken corner): the prism's faces (its sides, rounds, top and bottom) and one per
            // chamfer, the corners' cones (fanning or not).
            match cadrs_kernel::native::blend::chamfer_local(&mut k, body, &top, d) {
                Ok(out) => {
                    let o = out.bodies[0];
                    let body = k.get(o).unwrap();
                    let issues = body.check();
                    let faces = body.faces.len();
                    let removed = v0 - k.mass_properties(o).unwrap().volume;
                    if !issues.is_empty()
                        || faces != 2 + 4 * n
                        || (removed - want).abs() > 1e-6 * (1.0 + want)
                    {
                        failures.push(format!(
                            "{n}-gon r {r} d {d}: removed {removed:.9}, want {want:.9}, {faces} faces, {issues:?}"
                        ));
                    }
                }
                Err(e) => failures.push(format!("{n}-gon r {r} d {d}: {e}")),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn chamfered_convex_blocks() {
    let mut rng = Rng(0xb10c_0042);
    let mut failures = Vec::new();
    let cases = 40;
    for i in 0..cases {
        // A box or a regular prism.
        let sides = if i % 2 == 0 { 4 } else { rng.int(3, 8) };
        let r = rng.range(15.0, 25.0);
        let rot = rng.range(0.0, 1.0);
        let base: Vec<P2> = if sides == 4 && i % 4 == 0 {
            let (w, l) = (rng.range(20.0, 40.0), rng.range(20.0, 40.0));
            vec![
                P2::new(0.0, 0.0),
                P2::new(w, 0.0),
                P2::new(w, l),
                P2::new(0.0, l),
            ]
        } else {
            (0..sides)
                .map(|j| {
                    let a = rot + j as f64 / sides as f64 * std::f64::consts::TAU;
                    P2::new(r * a.cos(), r * a.sin())
                })
                .collect()
        };
        let h = rng.range(15.0, 30.0);
        let mut k = NativeKernel::new();
        let profile = Profile::new(
            Plane::top(),
            vec![Region {
                outer: polygon(&base),
                holes: vec![],
                source: None,
            }],
        );
        let b = k.extrude(&profile, Extent::Blind(h)).unwrap().bodies[0];
        let infos = k.edges(b).unwrap();
        let faces = k.faces(b).unwrap();
        // A random set of edges, at least one; often all of them (three chamfers at each corner).
        let all = rng.next() < 0.35;
        let picked: Vec<&EdgeInfo> = infos.iter().filter(|_| all || rng.next() < 0.5).collect();
        let picked: Vec<&EdgeInfo> = if picked.is_empty() {
            vec![&infos[0]]
        } else {
            picked
        };
        // Short enough that no chamfer swallows an edge between two others.
        let min_len = infos.iter().map(|e| e.length).fold(f64::INFINITY, f64::min);
        let d = (min_len * 0.15).min(rng.range(0.8, 3.0));

        // The expected solid: the block cut by each chamfer's plane.
        let normal_of = |f: Option<FaceId>| -> V3 {
            let f = faces.iter().find(|x| Some(x.id) == f).unwrap();
            let pl = f.plane.unwrap();
            let n = pl.normal.into_inner();
            // Outward: away from the block's middle.
            let mid = V3::new(0.0, 0.0, h / 2.0)
                + if sides == 4 && i % 4 == 0 {
                    V3::new(base[2].x / 2.0, base[2].y / 2.0, 0.0)
                } else {
                    V3::zeros()
                };
            if n.dot(&(pl.origin.coords - mid)) < 0.0 {
                -n
            } else {
                n
            }
        };
        let mut expect = Poly {
            faces: block_faces(&base, h),
        };
        for e in &picked {
            let (n1, n2) = (normal_of(e.faces[0]), normal_of(e.faces[1]));
            let t = (e.end - e.start).normalize();
            let mut w1 = n1.cross(&t).normalize();
            if w1.dot(&n2) > 0.0 {
                w1 = -w1;
            }
            let mut w2 = n2.cross(&t).normalize();
            if w2.dot(&n1) > 0.0 {
                w2 = -w2;
            }
            let q1 = e.start.coords + w1 * d;
            let mut m = t.cross(&(w2 - w1)).normalize();
            if m.dot(&(n1 + n2)) < 0.0 {
                m = -m;
            }
            expect = expect.clip(m, m.dot(&q1));
        }
        let ids: Vec<EdgeId> = picked.iter().map(|e| e.id).collect();
        let mut case = Case {
            name: format!(
                "block {i} ({sides} sides, {} of {} edges, d {d:.3})",
                ids.len(),
                infos.len()
            ),
            failures: Vec::new(),
        };
        match k.chamfer_with(b, &ids, &ChamferOpts::new(ChamferSpec::EqualDistance(d))) {
            Err(e) => case.check("chamfer", false, e.to_string()),
            Ok(r) => {
                let out = r.bodies[0];
                let issues = k.get(out).unwrap().check();
                case.check("valid", issues.is_empty(), format!("{issues:?}"));
                let v = k.mass_properties(out).unwrap().volume;
                let want = expect.volume();
                case.check(
                    "volume",
                    (v - want).abs() < 1e-6 * want,
                    format!("{v:.9}, expected {want:.9}"),
                );
                let nf = k.faces(out).unwrap().len();
                case.check(
                    "faces",
                    nf == expect.face_count(),
                    format!("{nf} faces, expected {}", expect.face_count()),
                );
            }
        }
        failures.extend(case.failures);
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The faces of a prism over a counter-clockwise base, outward.
fn block_faces(base: &[P2], h: f64) -> Vec<Vec<Point3<f64>>> {
    let n = base.len();
    let at = |p: P2, z: f64| Point3::new(p.x, p.y, z);
    let mut faces = vec![
        base.iter().rev().map(|&p| at(p, 0.0)).collect::<Vec<_>>(),
        base.iter().map(|&p| at(p, h)).collect(),
    ];
    for i in 0..n {
        let (a, b) = (base[i], base[(i + 1) % n]);
        faces.push(vec![at(a, 0.0), at(b, 0.0), at(b, h), at(a, h)]);
    }
    faces
}

#[test]
fn oracle_one_edge() {
    // A 10 × 20 × 30 box with its edge along x at y = 20, z = 30 cut by the plane through the
    // lines 2 in on its faces: 2²/2 × 10 = 20 less.
    let base = [
        P2::new(0.0, 0.0),
        P2::new(10.0, 0.0),
        P2::new(10.0, 20.0),
        P2::new(0.0, 20.0),
    ];
    let p = Poly {
        faces: block_faces(&base, 30.0),
    };
    assert!((p.volume() - 6000.0).abs() < 1e-9, "{}", p.volume());
    let m = V3::new(0.0, 1.0, 1.0).normalize();
    let q = V3::new(0.0, 18.0, 30.0);
    let c = p.clip(m, m.dot(&q));
    assert!((c.volume() - 5980.0).abs() < 1e-9, "{}", c.volume());
    assert_eq!(c.face_count(), 7);
}

#[test]
#[ignore]
fn debug_box_single_edges() {
    let base = [
        P2::new(0.0, 0.0),
        P2::new(30.0, 0.0),
        P2::new(30.0, 20.0),
        P2::new(0.0, 20.0),
    ];
    let mut k = NativeKernel::new();
    let profile = Profile::new(
        Plane::top(),
        vec![Region {
            outer: polygon(&base),
            holes: vec![],
            source: None,
        }],
    );
    let b = k.extrude(&profile, Extent::Blind(25.0)).unwrap().bodies[0];
    let v0 = k.mass_properties(b).unwrap().volume;
    for e in k.edges(b).unwrap() {
        let r = k
            .chamfer_with(
                b,
                &[e.id],
                &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
            )
            .unwrap();
        let removed = v0 - k.mass_properties(r.bodies[0]).unwrap().volume;
        eprintln!(
            "edge {:?} {:?}->{:?} len {} removed {removed} expected {}",
            e.id,
            e.start,
            e.end,
            e.length,
            2.0 * e.length
        );
    }
}

#[test]
#[ignore]
fn debug_box_pairs() {
    // Two edges meeting at a corner (z top edge along x, and vertical edge at x = 30, y = 0).
    let base = [
        P2::new(0.0, 0.0),
        P2::new(30.0, 0.0),
        P2::new(30.0, 20.0),
        P2::new(0.0, 20.0),
    ];
    let mut k = NativeKernel::new();
    let profile = Profile::new(
        Plane::top(),
        vec![Region {
            outer: polygon(&base),
            holes: vec![],
            source: None,
        }],
    );
    let b = k.extrude(&profile, Extent::Blind(25.0)).unwrap().bodies[0];
    let v0 = k.mass_properties(b).unwrap().volume;
    for set in [
        vec![5, 7],
        vec![5, 1],
        vec![5, 7, 1],
        vec![4, 5],
        vec![0, 1, 2, 3],
        vec![5, 7, 9, 11],
    ] {
        let ids: Vec<EdgeId> = set.iter().map(|&i| EdgeId(i)).collect();
        match k.chamfer_with(b, &ids, &ChamferOpts::new(ChamferSpec::EqualDistance(2.0))) {
            Ok(r) => eprintln!(
                "{set:?}: removed {} faces {}",
                v0 - k.mass_properties(r.bodies[0]).unwrap().volume,
                k.faces(r.bodies[0]).unwrap().len()
            ),
            Err(e) => eprintln!("{set:?}: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// More generated cases: fillets on the top outline, chamfers and fillets on the vertical edges
// (convex corners cut, inside corners filled).

/// An outline's loops, each with the material's angle at its corners.
type AngledLoops = Vec<(Vec<P2>, Vec<f64>)>;

/// A random outline (star, notched, or one with a hole) as (outer, holes, loops with the
/// material's angles).
fn outline(rng: &mut Rng, i: usize) -> (Vec<P2>, Vec<Vec<P2>>, AngledLoops) {
    let kind = i % 3;
    let mut outer = if kind == 1 { notched(rng) } else { star(rng) };
    if signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    let mut loops = vec![(outer.clone(), interior_angles(&outer))];
    let mut holes = Vec::new();
    if kind == 2 {
        let m = rng.int(3, 6);
        let r = rng.range(3.0, 6.0);
        let rot = rng.range(0.0, 1.0);
        let mut hole: Vec<P2> = (0..m)
            .map(|j| {
                let a = rot + j as f64 / m as f64 * std::f64::consts::TAU;
                P2::new(r * a.cos(), r * a.sin())
            })
            .collect();
        if signed_area(&hole) < 0.0 {
            hole.reverse();
        }
        let ang = interior_angles(&hole)
            .into_iter()
            .map(|t| std::f64::consts::TAU - t)
            .collect();
        loops.push((hole.clone(), ang));
        holes.push(hole);
    }
    (outer, holes, loops)
}

fn prism(k: &mut NativeKernel, outer: &[P2], holes: &[Vec<P2>], h: f64) -> Result<BodyId> {
    let profile = Profile::new(
        Plane::top(),
        vec![Region {
            outer: polygon(outer),
            holes: holes.iter().map(|h| polygon(h)).collect(),
            source: None,
        }],
    );
    Ok(k.extrude(&profile, Extent::Blind(h))?.bodies[0])
}

#[test]
fn filleted_prisms() {
    let mut rng = Rng(0xf111_e7ed);
    let mut failures = Vec::new();
    let cases = 45;
    for i in 0..cases {
        let (outer, holes, loops) = outline(&mut rng, i);
        let r = safe_distance(&loops).min(2.0);
        let h = rng.range(8.0, 20.0);
        let perimeter: f64 = loops
            .iter()
            .map(|(p, _)| {
                (0..p.len())
                    .map(|j| (p[(j + 1) % p.len()] - p[j]).norm())
                    .sum::<f64>()
            })
            .sum();
        let k_sum: f64 = loops
            .iter()
            .flat_map(|(_, a)| a.iter())
            .map(|t| 1.0 / (t / 2.0).tan())
            .sum();
        // ∫₀ʳ g(t) (P − 2Kt) dt, g the round's depth at t from the edge (Gauss–Legendre).
        let g = |t: f64| r - (r * r - (r - t) * (r - t)).max(0.0).sqrt();
        let n = 400;
        let expected: f64 = (0..n)
            .map(|j| {
                let t = (j as f64 + 0.5) / n as f64 * r;
                g(t) * (perimeter - 2.0 * k_sum * t) * r / n as f64
            })
            .sum();
        let mut case = Case {
            name: format!(
                "filleted prism {i} ({} corners, {} holes, r {r:.3})",
                outer.len(),
                holes.len()
            ),
            failures: Vec::new(),
        };
        let mut k = NativeKernel::new();
        let Ok(b) = prism(&mut k, &outer, &holes, h) else {
            failures.push(format!("{}: extrude failed", case.name));
            continue;
        };
        let edges: Vec<EdgeId> = k
            .edges(b)
            .unwrap()
            .iter()
            .filter(|e| (e.start.z - h).abs() < 1e-9 && (e.end.z - h).abs() < 1e-9)
            .map(|e| e.id)
            .collect();
        let n0 = k.faces(b).unwrap().len();
        let v0 = k.mass_properties(b).unwrap().volume;
        match k.fillet(b, &edges, r) {
            Err(e) => case.check("fillet", false, e.to_string()),
            Ok(res) => {
                let out = res.bodies[0];
                if std::env::var("CADRS_GEN_CASE")
                    .ok()
                    .and_then(|x| x.parse::<usize>().ok())
                    == Some(i)
                {
                    eprintln!(
                        "outline {outer:?}\nangles {:?}",
                        interior_angles(&outer)
                            .iter()
                            .map(|a| a.to_degrees())
                            .collect::<Vec<_>>()
                    );
                    for f in k.faces(out).unwrap() {
                        eprintln!(
                            "  {:?} area {:.4} centre {:?}",
                            f.kind,
                            f.area,
                            [f.center.x, f.center.y, f.center.z]
                        );
                    }
                }
                let issues = k.get(out).unwrap().check();
                case.check("valid", issues.is_empty(), format!("{issues:?}"));
                let removed = v0 - k.mass_properties(out).unwrap().volume;
                // The midpoint rule is good to ~1e-6 of the removed volume here.
                case.check(
                    "volume",
                    (removed - expected).abs() < 2e-4 * expected,
                    format!("removed {removed:.6}, expected {expected:.6}"),
                );
                let nf = k.faces(out).unwrap().len();
                case.check(
                    "faces",
                    nf == n0 + edges.len(),
                    format!("{nf} faces, expected {}", n0 + edges.len()),
                );
            }
        }
        failures.extend(case.failures);
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn vertical_edges_chamfered_and_filleted() {
    let mut rng = Rng(0x7e47_1ca1);
    let mut failures = Vec::new();
    let cases = 60;
    for i in 0..cases {
        let (outer, holes, loops) = outline(&mut rng, i);
        let size = safe_distance(&loops).min(2.0);
        let h = rng.range(8.0, 20.0);
        let fillet = i % 2 == 1;
        // The area each vertical corner loses (negative: an inside corner gains).
        let corner = |theta: f64| -> f64 {
            if fillet {
                let r = size;
                if theta < std::f64::consts::PI {
                    r * r / (theta / 2.0).tan() - (std::f64::consts::PI - theta) * r * r / 2.0
                } else {
                    let phi = std::f64::consts::TAU - theta;
                    -(r * r / (phi / 2.0).tan() - (std::f64::consts::PI - phi) * r * r / 2.0)
                }
            } else {
                size * size / 2.0 * theta.sin()
            }
        };
        let expected: f64 = loops
            .iter()
            .flat_map(|(_, a)| a.iter())
            .map(|&t| corner(t) * h)
            .sum();
        let mut case = Case {
            name: format!(
                "vertical {} {i} ({} corners, {} holes, size {size:.3})",
                if fillet { "fillets" } else { "chamfers" },
                outer.len(),
                holes.len()
            ),
            failures: Vec::new(),
        };
        let mut k = NativeKernel::new();
        let Ok(b) = prism(&mut k, &outer, &holes, h) else {
            failures.push(format!("{}: extrude failed", case.name));
            continue;
        };
        let edges: Vec<EdgeId> = k
            .edges(b)
            .unwrap()
            .iter()
            .filter(|e| (e.start.z - e.end.z).abs() > 1e-9)
            .map(|e| e.id)
            .collect();
        let n0 = k.faces(b).unwrap().len();
        let v0 = k.mass_properties(b).unwrap().volume;
        let r = if fillet {
            k.fillet(b, &edges, size)
        } else {
            k.chamfer_with(
                b,
                &edges,
                &ChamferOpts::new(ChamferSpec::EqualDistance(size)),
            )
        };
        match r {
            Err(e) => case.check("blend", false, e.to_string()),
            Ok(res) => {
                let out = res.bodies[0];
                let issues = k.get(out).unwrap().check();
                case.check("valid", issues.is_empty(), format!("{issues:?}"));
                let removed = v0 - k.mass_properties(out).unwrap().volume;
                case.check(
                    "volume",
                    (removed - expected).abs() < 1e-6 * (1.0 + expected.abs()),
                    format!("removed {removed:.9}, expected {expected:.9}"),
                );
                let nf = k.faces(out).unwrap().len();
                case.check(
                    "faces",
                    nf == n0 + edges.len(),
                    format!("{nf} faces, expected {}", n0 + edges.len()),
                );
            }
        }
        failures.extend(case.failures);
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Every edge of random prisms chamfered at once: top and bottom outlines cut, vertical edges
/// cut or filled, corners of three. No exact volume here; the result must be valid and have
/// the prism's faces, one per edge and Onshape's corner triangles (no slivers, spikes or end
/// caps).
#[test]
fn prisms_chamfered_all_round() {
    let mut rng = Rng(0xa11_70d);
    let mut failures = Vec::new();
    let cases = 45;
    for i in 0..cases {
        let (outer, holes, loops) = outline(&mut rng, i);
        let d = safe_distance(&loops).min(1.5);
        let h = rng.range(10.0, 20.0);
        let mut case = Case {
            name: format!(
                "all-round {i} ({} corners, {} holes, d {d:.3})",
                outer.len(),
                holes.len()
            ),
            failures: Vec::new(),
        };
        let mut k = NativeKernel::new();
        let Ok(b) = prism(&mut k, &outer, &holes, h) else {
            failures.push(format!("{}: extrude failed", case.name));
            continue;
        };
        let infos = k.edges(b).unwrap();
        let edges: Vec<EdgeId> = infos.iter().map(|e| e.id).collect();
        // Onshape closes each end of a filled inside corner, where the two chamfers either side
        // meet it, with a triangular corner face: two per inside corner.
        let inside: usize = loops
            .iter()
            .flat_map(|(_, a)| a.iter())
            .filter(|&&t| t > std::f64::consts::PI)
            .count();
        let n0 = k.faces(b).unwrap().len() + 2 * inside;
        match k.chamfer_with(b, &edges, &ChamferOpts::new(ChamferSpec::EqualDistance(d))) {
            Err(e) => case.check("chamfer", false, e.to_string()),
            Ok(res) => {
                let out = res.bodies[0];
                let issues = k.get(out).unwrap().check();
                case.check("valid", issues.is_empty(), format!("{issues:?}"));
                let nf = k.faces(out).unwrap().len();
                case.check(
                    "faces",
                    nf == n0 + edges.len(),
                    format!("{nf} faces, expected {}", n0 + edges.len()),
                );
                if std::env::var("CADRS_GEN_ALL")
                    .ok()
                    .and_then(|x| x.parse::<usize>().ok())
                    == Some(i)
                {
                    eprintln!(
                        "outline {outer:?}\nangles {:?}",
                        interior_angles(&outer)
                            .iter()
                            .map(|a| a.to_degrees())
                            .collect::<Vec<_>>()
                    );
                    for f in k.faces(out).unwrap() {
                        let n = f.plane.map(|p| [p.normal.x, p.normal.y, p.normal.z]);
                        eprintln!(
                            "  {:?} area {:.4} normal {n:?} centre {:?}",
                            f.kind,
                            f.area,
                            [f.center.x, f.center.y, f.center.z]
                        );
                    }
                }
            }
        }
        failures.extend(case.failures);
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Convex prisms (boxes and regular prisms) with every edge filleted at one radius: the
/// result is the prism shrunk by r grown again by a ball of r, so Steiner's formula gives its
/// volume exactly: V' + A'r + (Σ L(π − θ)/2) r² + (4/3)πr³ over the shrunk prism (θ each edge's
/// dihedral angle). Faces: the prism's, one per edge, one sphere patch per corner.
#[test]
fn convex_prisms_filleted_all_round() {
    let mut rng = Rng(0x0ba1_10f5);
    let mut failures = Vec::new();
    let cases = 30;
    for i in 0..cases {
        let sides = if i % 3 == 0 { 4 } else { rng.int(3, 8) };
        let rad = rng.range(15.0, 25.0);
        let rot = rng.range(0.0, 1.0);
        let base: Vec<P2> = if i % 3 == 0 {
            let (w, l) = (rng.range(20.0, 40.0), rng.range(20.0, 40.0));
            vec![
                P2::new(0.0, 0.0),
                P2::new(w, 0.0),
                P2::new(w, l),
                P2::new(0.0, l),
            ]
        } else {
            (0..sides)
                .map(|j| {
                    let a = rot + j as f64 / sides as f64 * std::f64::consts::TAU;
                    P2::new(rad * a.cos(), rad * a.sin())
                })
                .collect()
        };
        let h = rng.range(15.0, 30.0);
        let n = base.len();
        // The base shrunk by r (mitred, convex): each corner moves r/sin(θ/2) along its bisector.
        let ang = interior_angles(&base);
        let shortest = (0..n)
            .map(|j| (base[(j + 1) % n] - base[j]).norm())
            .fold(f64::INFINITY, f64::min);
        let r = rng.range(0.05, 0.25) * shortest.min(h) * 0.5;
        let inner: Vec<P2> = (0..n)
            .map(|j| {
                let (a, b, c) = (base[(j + n - 1) % n], base[j], base[(j + 1) % n]);
                let bis = ((a - b).normalize() + (c - b).normalize()).normalize();
                b + bis * (r / (ang[j] / 2.0).sin())
            })
            .collect();
        let hi = h - 2.0 * r;
        let area = signed_area(&inner);
        let per: f64 = (0..n).map(|j| (inner[(j + 1) % n] - inner[j]).norm()).sum();
        let v_in = area * hi;
        let a_in = 2.0 * area + per * hi;
        // Edges: the base and top outlines (dihedral 90°), the vertical edges (the corner's θ).
        let m_in = per * 2.0 * (std::f64::consts::PI / 2.0) / 2.0
            + (0..n)
                .map(|j| hi * (std::f64::consts::PI - ang[j]) / 2.0)
                .sum::<f64>();
        let expected =
            v_in + a_in * r + m_in * r * r + 4.0 / 3.0 * std::f64::consts::PI * r * r * r;
        let mut case = Case {
            name: format!("rounded prism {i} ({n} sides, r {r:.3})"),
            failures: Vec::new(),
        };
        let mut k = NativeKernel::new();
        let Ok(b) = prism(&mut k, &base, &[], h) else {
            failures.push(format!("{}: extrude failed", case.name));
            continue;
        };
        let edges: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
        let n0 = k.faces(b).unwrap().len();
        let nv = k.vertices(b).unwrap().len();
        match k.fillet(b, &edges, r) {
            Err(e) => case.check("fillet", false, e.to_string()),
            Ok(res) => {
                let out = res.bodies[0];
                let issues = k.get(out).unwrap().check();
                case.check("valid", issues.is_empty(), format!("{issues:?}"));
                let v = k.mass_properties(out).unwrap().volume;
                case.check(
                    "volume",
                    (v - expected).abs() < 1e-6 * expected,
                    format!("{v:.9}, expected {expected:.9}"),
                );
                let nf = k.faces(out).unwrap().len();
                case.check(
                    "faces",
                    nf == n0 + edges.len() + nv,
                    format!("{nf} faces, expected {}", n0 + edges.len() + nv),
                );
            }
        }
        failures.extend(case.failures);
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore]
fn debug_octagon_round() {
    for (rot, r) in [(0.0, 1.0), (0.2, 1.0), (0.0, 0.5), (0.39, 1.5)] {
        {
            let sides = 8;
            let base: Vec<P2> = (0..sides)
                .map(|j| {
                    let a = rot + j as f64 / sides as f64 * std::f64::consts::TAU;
                    P2::new(20.0 * a.cos(), 20.0 * a.sin())
                })
                .collect();
            let mut k = NativeKernel::new();
            let b = prism(&mut k, &base, &[], 20.0).unwrap();
            let edges: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
            let res = k.fillet(b, &edges, r);
            eprintln!(
                "sides {sides} rot {rot} r {r}: {:?}",
                res.map(|x| k.faces(x.bodies[0]).unwrap().len())
            );
        }
    }
}
