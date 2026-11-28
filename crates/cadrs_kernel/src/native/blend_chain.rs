//! Blend tools along tangent chains of edges (a slope cut through a plate's rounded corners:
//! lines and ellipse arcs, the faces' angle changing along the chain).
//!
//! At each station along the chain the rolling ball (or the chamfer) touches the faces where
//! its contacts really are: a contact that falls off the edge's own face is found on the
//! tangent-continuous neighbour it moved onto (round a corner from a wall onto the corner's
//! cylinder). The tool is lofted through line–arc–line sections, its corner pushed out of the
//! body, and split where a contact crosses from one face to another (bisected), so each piece's
//! contact rails lie on one face each: the boolean then takes a rail where the blend face is
//! tangent to a face as the contact (`boolean::tangent_along_shared_edge`) instead of contouring
//! two tangent surfaces. The pieces are glued at their common sections.

use super::blend::{FaceSrc, SectionKind, Tool, along_face, edge_sides, out_dist};
use super::features::Tagged;
use super::math::*;
use super::profile::{Piece, Src};
use super::tol;
use super::topo::*;
use crate::*;

/// A station's section: its pieces in its plane and the faces its contacts lie on.
struct Station {
    s: f64,
    pieces: Vec<Piece>,
    plane: Plane,
    faces: [usize; 2],
}

/// The chain's geometry for sections.
struct Chain<'a> {
    b: &'a Body,
    chain: &'a [(usize, bool)],
    starts: Vec<f64>,
    lens: Vec<f64>,
    kind: &'a dyn Fn(f64) -> SectionKind,
    /// Each edge's own faces (the section's size follows their angle at the edge).
    sides: Vec<[(usize, bool); 2]>,
    convex: bool,
    polys: std::cell::RefCell<std::collections::HashMap<usize, std::sync::Arc<Vec<Vec<P2>>>>>,
}

impl Chain<'_> {
    fn edge_at(&self, s: f64) -> usize {
        let k = self.starts.iter().rposition(|&st| st <= s).unwrap_or(0);
        k.min(self.chain.len() - 1)
    }

    fn polys(&self, fi: usize) -> std::sync::Arc<Vec<Vec<P2>>> {
        self.polys
            .borrow_mut()
            .entry(fi)
            .or_insert_with(|| {
                std::sync::Arc::new(super::classify::face_polygons(self.b, &self.b.faces[fi]))
            })
            .clone()
    }

    /// Whether `q` lies on face `fi`.
    fn on(&self, fi: usize, q: &P3) -> bool {
        let f = &self.b.faces[fi];
        let (u, v) = f.surf.project(q, None);
        if (f.surf.eval(u, v) - q).norm() > tol::point() {
            return false;
        }
        let ps = self.polys(fi);
        super::classify::periodic_candidates(&f.surf, u, v, &ps)
            .into_iter()
            .any(|uv| {
                super::classify::in_face(&ps, &f.surf, uv, 1e-7) != super::classify::InFace::Outside
            })
    }

    /// The point `dist` from `p` across face `own` in the plane square to `t` (from direction
    /// `w`), continuing on face `next`'s surface past `own`'s boundary.
    fn walk(&self, own: usize, next: usize, p: &P3, t: &V3, w: &V3, dist: f64) -> P3 {
        if own == next {
            return along_face(&self.b.faces[own], p, t, w, dist);
        }
        let steps = 64;
        let h = dist / steps as f64;
        let (mut q, mut dir) = (*p, *w);
        let mut left = dist;
        let mut cur = own;
        let onto = |fi: usize, x: P3| -> P3 {
            let f = &self.b.faces[fi];
            let mut x = x;
            for _ in 0..4 {
                let (u, v) = f.surf.project(&x, None);
                x = f.surf.eval(u, v);
                x -= t * (x - p).dot(t);
            }
            x
        };
        while left > tol::ROUNDOFF_FINE {
            let step = h.min(left);
            let x = onto(cur, q + dir * step);
            let (x, used) = if cur == own && !self.on(own, &x) {
                // Where the step leaves the face (bisection); on from there on the next face.
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..40 {
                    let m = (lo + hi) / 2.0;
                    if self.on(own, &onto(own, q + dir * (step * m))) {
                        lo = m
                    } else {
                        hi = m
                    }
                }
                cur = next;
                (onto(own, q + dir * (step * lo)), step * lo)
            } else {
                (x, step)
            };
            let d = x - q;
            if d.norm() > 1e-15 {
                dir = d.normalize();
            }
            q = x;
            left -= used;
        }
        q
    }

    /// The line–arc–line (or line–line–line) section at chain position `s` with the contacts
    /// on `faces` (each with the side flag of the edge's own face it continues), and whether
    /// each contact lies on its face.
    fn section(&self, s: f64, faces: [(usize, bool); 2]) -> Result<(Vec<Piece>, Plane, [bool; 2])> {
        let b = self.b;
        let k = self.edge_at(s);
        let fr = (s - self.starts[k]) / self.lens[k].max(tol::TINY);
        let (e, fwd) = self.chain[k];
        let ed = &b.edges[e];
        let fr = if fwd { fr } else { 1.0 - fr };
        let t = ed.t[0] + (ed.t[1] - ed.t[0]) * fr;
        let p = ed.curve.point(t);
        let tan = ed.curve.tangent(t).normalize() * if fwd { 1.0 } else { -1.0 };
        let (f1, f2) = (&b.faces[faces[0].0], &b.faces[faces[1].0]);
        // The faces' outward normals at the corner and the directions into them square to the
        // edge (the surfaces continue past the faces' boundaries for the extensions).
        let dirs = |f: &Face, co_fwd: bool| -> (V3, V3) {
            let (u, v) = f.surf.project(&p, None);
            let nrm = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
            let d = tan * if co_fwd == fwd { 1.0 } else { -1.0 };
            (nrm, nrm.cross(&d))
        };
        let (_, w1) = dirs(f1, faces[0].1);
        let (_, w2) = dirs(f2, faces[1].1);
        // The angle between the edge's own faces at the edge (continuous along the chain, where
        // the contacts' faces change as the contacts cross onto tangent neighbours).
        let own = self.sides[k];
        let (n1, _) = dirs(&b.faces[own[0].0], own[0].1);
        let (n2, _) = dirs(&b.faces[own[1].0], own[1].1);
        let phi = n1.dot(&n2).clamp(-1.0, 1.0).acos();
        let theta = w1.dot(&w2).clamp(-1.0, 1.0).acos();
        enum Mid {
            Line,
            Arc(P3),
        }
        let (c1, c2, mid) = match (self.kind)(phi) {
            SectionKind::Circle { r } => {
                let sg = if self.convex { -1.0 } else { 1.0 };
                let bis = (w1 + w2).normalize();
                let mut x = p + bis * (r / (theta / 2.0).sin());
                for _ in 0..60 {
                    let (d1, g1, _) = out_dist(f1, &x);
                    let (d2, g2, _) = out_dist(f2, &x);
                    let m = nalgebra::Matrix3::from_rows(&[
                        g1.transpose(),
                        g2.transpose(),
                        tan.transpose(),
                    ]);
                    let rhs = V3::new(sg * r - d1, sg * r - d2, -(x - p).dot(&tan));
                    let Some(inv) = m.try_inverse() else { break };
                    let dx = inv * rhs;
                    x += dx;
                    if dx.norm() < 1e-14 {
                        break;
                    }
                }
                if !x.coords.iter().all(|c| c.is_finite()) {
                    return Err(KernelError::InvalidGeometry {
                        op: "blend",
                        why: "the blend's section degenerates along the edge".into(),
                    });
                }
                let (_, _, q1) = out_dist(f1, &x);
                let (_, _, q2) = out_dist(f2, &x);
                (q1, q2, Mid::Arc(x))
            }
            SectionKind::Chamfer { d1, d2 } => {
                // Along the edge's own face, onto the contact's face where the walk leaves it
                // (so neighbouring sections agree where the contact changes face).
                let (_, o1) = dirs(&b.faces[own[0].0], own[0].1);
                let (_, o2) = dirs(&b.faces[own[1].0], own[1].1);
                (
                    self.walk(own[0].0, faces[0].0, &p, &tan, &o1, d1),
                    self.walk(own[1].0, faces[1].0, &p, &tan, &o2, d2),
                    Mid::Line,
                )
            }
            _ => return Err(KernelError::InvalidParameter(
                "Conic, curvature and asymmetric fillets work on straight edges between flat faces"
                    .into(),
            )),
        };
        // Faces tangent at the edge (or a failed solve) give no section, not a NaN one.
        let finite = |q: &P3| q.coords.iter().all(|x| x.is_finite());
        if !finite(&c1)
            || !finite(&c2)
            || (c1 - p).norm() < tol::DEGENERATE
            || (c2 - p).norm() < tol::DEGENERATE
        {
            return Err(KernelError::InvalidGeometry {
                op: "blend",
                why: "the blend's section degenerates along the edge".into(),
            });
        }
        let inside = [self.on(faces[0].0, &c1), self.on(faces[1].0, &c2)];
        // The section's plane: through the contacts and the corner.
        let nrm = (c1 - p).cross(&(c2 - p));
        if nrm.norm() < tol::DEGENERATE {
            return Err(KernelError::InvalidGeometry {
                op: "blend",
                why: "the blend's section degenerates along the edge".into(),
            });
        }
        let nrm = nrm.normalize();
        let xd = (c1 - p).normalize();
        let plane = Plane {
            origin: p,
            x_dir: nalgebra::Unit::new_normalize(xd),
            normal: nalgebra::Unit::new_normalize(nrm),
        };
        let yd = plane.y_dir().into_inner();
        let to2 = |q: &P3| nalgebra::Point2::new((q - p).dot(&xd), (q - p).dot(&yd));
        let (q1, q2) = (to2(&c1), to2(&c2));
        let bis = (q1.coords.normalize() + q2.coords.normalize()).normalize();
        let size = q1.coords.norm().max(q2.coords.norm());
        // The corner pushed out of the body, so the tool's straight sides cross the faces.
        let pc = nalgebra::Point2::origin() - bis * (size * 0.5 + 1e-3);
        let (midc, q1, q2) = match mid {
            Mid::Line => (
                Curve2::Line {
                    a: q1,
                    b: q2,
                    source: Some(1),
                },
                q1,
                q2,
            ),
            Mid::Arc(center) => {
                let cc = to2(&center);
                let r = ((q1 - cc).norm() + (q2 - cc).norm()) * 0.5;
                let a1 = (q1.y - cc.y).atan2(q1.x - cc.x);
                let a2 = (q2.y - cc.y).atan2(q2.x - cc.x);
                let mut sw = a2 - a1;
                while sw > PI {
                    sw -= TAU;
                }
                while sw < -PI {
                    sw += TAU;
                }
                let arc = Curve2::Arc {
                    center: cc,
                    radius: r,
                    start_angle: a1,
                    sweep: sw,
                    source: Some(1),
                };
                let (s0, s1) = (arc.start(), arc.end());
                (arc, s0, s1)
            }
        };
        let curves = [
            Curve2::Line {
                a: pc,
                b: q1,
                source: Some(0),
            },
            midc,
            Curve2::Line {
                a: q2,
                b: pc,
                source: Some(2),
            },
        ];
        let pieces = curves
            .iter()
            .enumerate()
            .map(|(i, c)| {
                super::profile::piece_of(
                    c,
                    Src::Curve {
                        region: 0,
                        curve: i as u64,
                    },
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((pieces, plane, inside))
    }
}

/// One blend tool along a chain of edges (each run forwards or backwards). On each edge the
/// first face is the one whose loop runs along the chain (swapped when `swap`); `range` and
/// `extend` apply to the chain's ends.
pub fn chain_tool(
    b: &Body,
    input: BodyId,
    chain: &[(usize, bool)],
    kind: &dyn Fn(f64) -> SectionKind,
    swap: bool,
    range: (f64, f64),
    extend: (bool, bool),
) -> Result<Tool> {
    let mut sides: Vec<[(usize, bool); 2]> = Vec::new();
    for &(e, fwd) in chain {
        let [a, c] = edge_sides(b, e)?;
        let (mut s1, mut s2) = if a.1 == fwd { (a, c) } else { (c, a) };
        if swap {
            std::mem::swap(&mut s1, &mut s2);
        }
        sides.push([s1, s2]);
    }
    let lens: Vec<f64> = chain
        .iter()
        .map(|&(e, _)| b.edges[e].curve.length(b.edges[e].t[0], b.edges[e].t[1]))
        .collect();
    let len: f64 = lens.iter().sum();
    let mut acc = 0.0;
    let starts: Vec<f64> = lens
        .iter()
        .map(|l| {
            let s0 = acc;
            acc += l;
            s0
        })
        .collect();
    let e0 = chain[0].0;
    let convex = {
        let ed = &b.edges[e0];
        let tm = ed.mid_t();
        let f = |fi: usize, fwd: bool| -> Result<(V3, V3)> {
            let face = &b.faces[fi];
            let c = face
                .loops
                .iter()
                .flatten()
                .find(|c| c.edge == e0 && c.fwd == fwd)
                .ok_or_else(|| KernelError::InvalidGeometry {
                    op: "blend",
                    why: "the blended edge isn't on the face beside it".into(),
                })?;
            let uv = c.pc.point(tm);
            let n = face.surf.normal(uv.x, uv.y) * if face.sense { 1.0 } else { -1.0 };
            let d = ed.curve.tangent(tm).normalize() * if fwd { 1.0 } else { -1.0 };
            Ok((n, n.cross(&d)))
        };
        let (n1, _) = f(sides[0][0].0, sides[0][0].1)?;
        let (_, w2) = f(sides[0][1].0, sides[0][1].1)?;
        w2.dot(&n1) < 0.0
    };
    let ch = Chain {
        b,
        chain,
        starts: starts.clone(),
        lens: lens.clone(),
        kind,
        sides: sides.clone(),
        convex,
        polys: Default::default(),
    };
    // Tangent-continuous neighbours of a face (across edges where their normals agree).
    let ef = b.edge_faces();
    let neighbours = |fi: usize| -> Vec<usize> {
        let mut out = Vec::new();
        for c in b.faces[fi].loops.iter().flatten() {
            for &g in &ef[c.edge] {
                if g != fi && !out.contains(&g) {
                    let e = &b.edges[c.edge];
                    let m = e.point(e.mid_t());
                    let (fa, fb) = (&b.faces[fi], &b.faces[g]);
                    let (ua, va) = fa.surf.project(&m, None);
                    let (ub, vb) = fb.surf.project(&m, None);
                    let na = fa.surf.normal(ua, va) * if fa.sense { 1.0 } else { -1.0 };
                    let nb = fb.surf.normal(ub, vb) * if fb.sense { 1.0 } else { -1.0 };
                    if na.dot(&nb) > 1.0 - tol::ANGULAR {
                        out.push(g);
                    }
                }
            }
        }
        out
    };
    let size_guess0 = match kind(PI / 2.0) {
        SectionKind::Circle { r }
        | SectionKind::Conic { r, .. }
        | SectionKind::Curvature { r, .. }
        | SectionKind::Thumbweight { r, .. } => r,
        SectionKind::Chamfer { d1, d2 } => d1.max(d2),
        SectionKind::Asymmetric { r1, r2 } => r1.max(r2),
    };
    let ext_len = size_guess0 * 2.0 + 0.5;
    let (ra, rb) = (range.0 * len, range.1 * len);
    let total_lo = ra - if extend.0 { ext_len } else { 0.0 };
    let total_hi = rb + if extend.1 { ext_len } else { 0.0 };
    // A station: the contacts on the faces they lie on (the edge's own, or the neighbour the
    // contact moved onto; outside the chain's ends, the end edge's own faces).
    let within = |s: f64| (-tol::ROUNDOFF..=len + tol::ROUNDOFF).contains(&s);
    let station = |s: f64, prefer: Option<[usize; 2]>| -> Result<Station> {
        let k = ch.edge_at(s);
        let own = sides[k];
        let mut faces = [
            (prefer.map_or(own[0].0, |p| p[0]), own[0].1),
            (prefer.map_or(own[1].0, |p| p[1]), own[1].1),
        ];
        for _ in 0..4 {
            let (pieces, plane, inside) = ch.section(s, faces)?;
            if !within(s) || (inside[0] && inside[1]) {
                return Ok(Station {
                    s,
                    pieces,
                    plane,
                    faces: [faces[0].0, faces[1].0],
                });
            }
            let mut moved = false;
            for side in 0..2 {
                if inside[side] {
                    continue;
                }
                // The edge's own face first, then the tangent neighbours.
                let mut cands = vec![own[side].0];
                cands.extend(neighbours(faces[side].0));
                cands.extend(neighbours(own[side].0));
                for g in cands {
                    if g == faces[side].0 {
                        continue;
                    }
                    let mut f2 = faces;
                    f2[side].0 = g;
                    if let Ok((_, _, ins)) = ch.section(s, f2)
                        && ins[side]
                    {
                        faces = f2;
                        moved = true;
                        break;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        let (pieces, plane, _) = ch.section(s, faces)?;
        Ok(Station {
            s,
            pieces,
            plane,
            faces: [faces[0].0, faces[1].0],
        })
    };
    // Stations marched with an adaptive step (as OCCT's blend walking): a step is taken when
    // the edge's direction and each contact rail turn little enough over it that the lofted
    // rails stay on their faces within the boolean's tolerance. A cubic through sections a
    // chord c apart that turn by θ errs by about c·θ³/384: kept under 1e-7. Steps grow by 1.5
    // where the turn is well inside that, up to 1.5.
    const RAIL_TOL: f64 = 1e-7;
    let (max_step, min_step): (f64, f64) = (1.5, 1e-3);
    // The chain's vertices and its two ends (where the extensions start) are stations.
    let mut breaks: Vec<f64> = starts
        .iter()
        .skip(1)
        .copied()
        .chain([0.0, len])
        .filter(|&sj| sj > total_lo + tol::point() && sj < total_hi - tol::point())
        .collect();
    breaks.push(total_hi);
    breaks.sort_by(f64::total_cmp);
    let contacts = |st: &Station| -> [P3; 3] {
        let w = |q: P3| {
            st.plane.origin
                + st.plane.x_dir.into_inner() * q.x
                + st.plane.y_dir().into_inner() * q.y
        };
        let end = |x: &Piece| x.c.point(if x.rev { x.t0 } else { x.t1 });
        let start = |x: &Piece| x.c.point(if x.rev { x.t1 } else { x.t0 });
        [
            w(end(&st.pieces[0])),
            w(start(&st.pieces[2])),
            st.plane.origin,
        ]
    };
    // The worst c·θ³ over the rails (and the edge's own points) between three stations.
    let bend = |a: &[P3; 3], b: &[P3; 3], c: &[P3; 3]| -> f64 {
        (0..3)
            .map(|i| {
                let (d0, d1) = (b[i] - a[i], c[i] - b[i]);
                let (l0, l1) = (d0.norm(), d1.norm());
                if l0 < tol::DEGENERATE || l1 < tol::DEGENERATE {
                    return 0.0;
                }
                let th = (d0.dot(&d1) / (l0 * l1)).clamp(-1.0, 1.0).acos();
                l1 * th.powi(3) / 384.0
            })
            .fold(0.0, f64::max)
    };
    let mut stations: Vec<Station> = vec![station(total_lo, None)?];
    let mut step = max_step;
    let mut bi = 0;
    // A bound on the march (a path that never settles fails instead of hanging): in stations
    // kept, and in sections solved (tried steps included). Counted, not timed, so the same
    // input always gets the same answer.
    const MAX_STATIONS: usize = 5000;
    const MAX_SECTIONS: usize = 4_000;
    let mut solved = 0usize;
    while stations.last().unwrap().s < total_hi - tol::ROUNDOFF {
        if solved > MAX_SECTIONS {
            return Err(KernelError::BudgetExhausted {
                stage: "sections",
                why: format!("the blend's path needs more than {MAX_SECTIONS} sections solved"),
            });
        }
        if stations.len() > MAX_STATIONS {
            return Err(KernelError::BudgetExhausted {
                stage: "sections",
                why: format!(
                    "the blend's path needs more than {MAX_STATIONS} sections (at {:.4} of [{total_lo:.3}, {total_hi:.3}], step {step:.2e})",
                    stations.last().unwrap().s
                ),
            });
        }
        let s0 = stations.last().unwrap().s;
        while breaks[bi] <= s0 + tol::ROUNDOFF {
            bi += 1;
        }
        let prev = within(s0)
            .then(|| stations.last().map(|x| x.faces))
            .flatten();
        // Even steps up to the next forced station (a short step next to long ones makes the
        // loft through the sections overshoot).
        let rem = breaks[bi] - s0;
        let mut h = rem / (rem / step).ceil().max(1.0);
        loop {
            let s1 = s0 + h;
            solved += 1;
            let st = station(s1, prev.filter(|_| within(s1)))?;
            // Against the station before the last (the turn of the rails at the last one), or
            // before there is one, the half step.
            let err = if stations.len() >= 2 {
                let n = stations.len();
                bend(
                    &contacts(&stations[n - 2]),
                    &contacts(&stations[n - 1]),
                    &contacts(&st),
                )
            } else {
                let mid = station(s0 + 0.5 * h, prev)?;
                bend(&contacts(&stations[0]), &contacts(&mid), &contacts(&st))
            };
            if err <= RAIL_TOL || h <= min_step {
                if err < RAIL_TOL / 4.0 {
                    step = (h * 1.5).min(max_step);
                } else {
                    step = h;
                }
                stations.push(st);
                break;
            }
            h *= 0.5;
        }
    }
    if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
        let ss: Vec<String> = stations.iter().map(|st| format!("{:.3}", st.s)).collect();
        eprintln!(
            "chain stations ({} on [{total_lo:.3}, {total_hi:.3}], chain length {len:.3}, {solved} sections solved): {}",
            ss.len(),
            ss.join(" ")
        );
    }
    // Where the contacts change faces: a station there in both pieces (bisected).
    let mut groups: Vec<Vec<Station>> = vec![Vec::new()];
    for st in stations {
        let last = groups.last().unwrap().last().map(|x: &Station| x.faces);
        match last {
            Some(f) if f != st.faces && within(st.s) => {
                let a = groups.last().unwrap().last().unwrap().s;
                let (mut lo, mut hi) = (a, st.s);
                let flags = |m: f64| {
                    let own = sides[ch.edge_at(m)];
                    (own[0].1, own[1].1)
                };
                for _ in 0..50 {
                    let m = 0.5 * (lo + hi);
                    let (g0, g1) = flags(m);
                    match ch.section(m, [(f[0], g0), (f[1], g1)]) {
                        Ok((_, _, ins)) if ins[0] && ins[1] => lo = m,
                        _ => hi = m,
                    }
                }
                let (g0, g1) = flags(lo);
                let (pa, pla, _) = ch.section(lo, [(f[0], g0), (f[1], g1)])?;
                let (pb, plb, _) = ch.section(lo, [(st.faces[0], g0), (st.faces[1], g1)])?;
                if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
                    let ends = |ps: &[Piece], pl: &Plane| {
                        ps.iter()
                            .map(|x| {
                                let q = x.c.point(if x.rev { x.t1 } else { x.t0 });
                                pl.origin
                                    + pl.x_dir.into_inner() * q.x
                                    + pl.y_dir().into_inner() * q.y
                            })
                            .collect::<Vec<_>>()
                    };
                    let d: Vec<f64> = ends(&pa, &pla)
                        .iter()
                        .zip(ends(&pb, &plb))
                        .map(|(a, b)| (a - b).norm())
                        .collect();
                    eprintln!(
                        "chain tool: face change at s {lo:.6} (from {a:.4} to {:.4}, edge starts {:?}) {f:?} -> {:?}, section ends apart {d:?}, pa {:?}",
                        st.s,
                        ch.starts,
                        st.faces,
                        ends(&pa, &pla)
                    );
                }
                groups.last_mut().unwrap().push(Station {
                    s: lo,
                    pieces: pa.clone(),
                    plane: pla,
                    faces: f,
                });
                groups.push(vec![Station {
                    s: lo,
                    pieces: pa.clone(),
                    plane: pla,
                    faces: st.faces,
                }]);
                groups.last_mut().unwrap().push(st);
            }
            _ => groups.last_mut().unwrap().push(st),
        }
    }
    // Loft each piece (its sections as they are) and glue them.
    let mut tk = super::kernel::NativeKernel::new();
    let mut acc: Option<Tagged> = None;
    // Each piece's faces are named after the chain edge it runs along (a chamfer round a cut
    // fillet keeps a face per edge, with edges between them, as other systems show).
    let mut group_edges: Vec<usize> = Vec::new();
    for g in groups.iter() {
        let mid = g
            .first()
            .zip(g.last())
            .map_or(0.0, |(a, b)| 0.5 * (a.s + b.s));
        group_edges.push(chain[ch.edge_at(mid)].0);
        let region = group_edges.len() as u64;
        let mut secs: Vec<(Vec<Piece>, Plane)> = Vec::new();
        let mut last_s = f64::NEG_INFINITY;
        for st in g {
            if st.s - last_s < tol::point() {
                continue;
            }
            last_s = st.s;
            secs.push((st.pieces.clone(), st.plane));
        }
        if secs.len() < 2 {
            continue;
        }
        let built = super::loft_build::loft_pieces(&tk, &secs, region)?;
        let t = Tagged::from_built(built);
        acc = Some(match acc {
            None => t,
            Some(a) => match super::sweep::path::glue(&a, &t) {
                Some(gl) => gl,
                None => super::features::tagged_union(&a, &t)?,
            },
        });
    }
    let t = acc.ok_or_else(|| KernelError::InvalidGeometry {
        op: "blend",
        why: "the blend's tool is empty".into(),
    })?;
    if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
        eprintln!(
            "chain tool: {} pieces {:?}, volume {}, check {:?}",
            groups.len(),
            groups
                .iter()
                .map(|g| (g.len(), g.first().map(|s| s.faces)))
                .collect::<Vec<_>>(),
            super::mass::body_mass(&t.body).volume,
            t.body.check()
        );
    }
    let lr = t.into_result(&mut tk);
    let body = (**tk.get(lr.bodies[0])?).clone();
    let mut src = vec![Vec::new(); body.faces.len()];
    for (f, o) in &lr.history.generated {
        if let Origin::ProfileCurve { region, curve } = o
            && *curve == unsourced_curve(0, 1)
        {
            let e = group_edges
                .get((*region as usize).wrapping_sub(1))
                .copied()
                .unwrap_or(e0);
            src[f.0 as usize].push(FaceSrc::New(Origin::FromEdge {
                body: input,
                edge: EdgeId(e as u64),
            }));
        }
    }
    let z = (P3::origin(), V3::zeros(), V3::zeros());
    Ok(Tool {
        body,
        src,
        convex,
        ends: [z, z],
    })
}
