//! Smooth fillet corners (Onshape's Final / OCCT's MakeFilling corner; `Kernel::fillet_smooth`).
//!
//! Where three or more filleted edges meet at a vertex V, the default fillet's corner is cut back
//! by a ball of radius ρ = √(setback² + r²) round V, and the hole is filled by one patch tangent
//! to every face round it. The hole's rim is where the ball meets those faces (planes and the
//! fillets' cylinders, tangent-continuous along their joints), one smooth closed loop. The patch
//! is a B-spline disc: its outer row interpolates the rim (by length), its next row leans along
//! the neighbouring faces' tangent planes (so the patch meets them tangentially at the rim
//! samples, and within a fraction of a degree between them), and its inner rows close on a pole
//! domed towards V.

use std::sync::Arc;

use super::blend::{FaceSrc, Tracked};
use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface};
use super::surface::Surface;
use super::tol;
use super::topo::*;
use crate::*;

pub fn fillet_smooth(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeId],
    spec: &FilletSpec,
    setback: f64,
) -> Result<OpResult> {
    let FilletSize::Radius(r) = spec.size else {
        return Err(KernelError::Unsupported("smooth corners of width fillets"));
    };
    if !(setback > 0.0) {
        return Err(KernelError::InvalidParameter(
            "The setback must be positive".into(),
        ));
    }
    let b0 = std::sync::Arc::clone(k.get(body)?);
    // The picked edges with their tangent chains, and the vertices where three or more meet.
    let mut all: Vec<usize> = Vec::new();
    for &e in edges {
        for c in k.tangent_chain(body, e)? {
            if !all.contains(&(c.0 as usize)) {
                all.push(c.0 as usize);
            }
        }
    }
    let mut corners: Vec<usize> = Vec::new();
    for v in 0..b0.verts.len() {
        let n = all
            .iter()
            .filter(|&&e| b0.edges[e].v.contains(&v) && b0.edges[e].v[0] != b0.edges[e].v[1])
            .count();
        if n >= 3 {
            corners.push(v);
        }
    }
    let plain = super::blend::fillet(k, body, edges, spec)?;
    if corners.is_empty() {
        return Ok(plain);
    }
    // The fillet's faces with their sources.
    let fb = std::sync::Arc::clone(k.get(plain.bodies[0])?);
    let mut src: Vec<Vec<FaceSrc>> = vec![Vec::new(); fb.faces.len()];
    for (f, o) in &plain.history.generated {
        src[f.0 as usize].push(FaceSrc::New(*o));
    }
    for (f, i) in &plain.history.modified {
        src[f.0 as usize].push(FaceSrc::Input(i.face.0 as usize));
    }
    k.release(plain.bodies[0]);
    let mut cur = Tracked {
        body: (*fb).clone(),
        src,
    };
    let rho = (setback * setback + r * r).sqrt();
    for &v in &corners {
        let vp = b0.verts[v].p;
        let mark = FaceSrc::New(Origin::FromVertex {
            body,
            vertex: VertexId(v as u64),
        });
        // Outward: the corner's faces' normals.
        let mut out = V3::zeros();
        for f in &b0.faces {
            if f.loops
                .iter()
                .flatten()
                .any(|c| b0.edges[c.edge].v.contains(&v))
            {
                let (u, w) = f.surf.project(&vp, None);
                out += f.surf.normal(u, w) * if f.sense { 1.0 } else { -1.0 };
            }
        }
        let out =
            out.try_normalize(tol::DEGENERATE)
                .ok_or_else(|| KernelError::InvalidGeometry {
                    op: "smooth corner",
                    why: "the corner has no outside".into(),
                })?;
        // The ball, its poles square to the corner's outward direction (off the hole).
        let ball = super::blend::ball_frame(vp, rho, any_perp(&out), out)?;
        let ball_src = vec![vec![mark.clone()]; ball.faces.len()];
        cur = cur.apply(&ball, &ball_src, BoolOp::Subtract)?;
        cur = fill_corner(cur, &mark, vp, out)?;
    }
    Ok(cur.into_result(k, body))
}

fn any_perp(v: &V3) -> V3 {
    let a = if v.x.abs() < 0.9 { V3::x() } else { V3::y() };
    v.cross(&a).normalize()
}

/// Replaces the faces marked `mark` (the ball's, round the corner `vp`) by one tangent patch.
fn fill_corner(mut t: Tracked, mark: &FaceSrc, vp: P3, out: V3) -> Result<Tracked> {
    let hole: Vec<usize> = (0..t.body.faces.len())
        .filter(|&f| t.src[f].contains(mark))
        .collect();
    if hole.is_empty() {
        return Err(KernelError::InvalidGeometry {
            op: "smooth corner",
            why: "the corner's ball cut nothing".into(),
        });
    }
    let b = &t.body;
    // The rim: edges used once by the hole's faces, in their direction there.
    let mut uses: std::collections::HashMap<usize, Vec<bool>> = Default::default();
    for &f in &hole {
        for c in b.faces[f].loops.iter().flatten() {
            if !b.edges[c.edge].degenerate {
                uses.entry(c.edge).or_default().push(c.fwd);
            }
        }
    }
    let mut rim: Vec<(usize, bool)> = uses
        .iter()
        .filter(|(_, u)| u.len() == 1)
        .map(|(e, u)| (*e, u[0]))
        .collect();
    // The same seam every run.
    rim.sort();
    // Chain into one loop.
    let start_of = |&(e, fwd): &(usize, bool)| {
        if fwd {
            b.edges[e].v[0]
        } else {
            b.edges[e].v[1]
        }
    };
    let end_of = |&(e, fwd): &(usize, bool)| {
        if fwd {
            b.edges[e].v[1]
        } else {
            b.edges[e].v[0]
        }
    };
    let mut chain = vec![rim[0]];
    let mut used = vec![false; rim.len()];
    used[0] = true;
    while end_of(chain.last().unwrap()) != start_of(&chain[0]) {
        let e = end_of(chain.last().unwrap());
        let Some(i) = (0..rim.len()).find(|&i| !used[i] && start_of(&rim[i]) == e) else {
            return Err(KernelError::InvalidGeometry {
                op: "smooth corner",
                why: "the corner's rim is not one loop".into(),
            });
        };
        used[i] = true;
        chain.push(rim[i]);
    }
    if used.iter().any(|u| !u) {
        return Err(KernelError::InvalidGeometry {
            op: "smooth corner",
            why: "the corner's rim is more than one loop".into(),
        });
    }
    // The faces beside each rim edge (not of the hole).
    let ef = b.edge_faces();
    let beside: Vec<usize> = chain
        .iter()
        .map(|&(e, _)| {
            ef[e]
                .iter()
                .copied()
                .find(|f| !hole.contains(f))
                .unwrap_or(0)
        })
        .collect();
    // Dense samples along the rim (by length): point, neighbour face, tangent along the rim.
    struct Smp {
        p: P3,
        n: V3,
        tan: V3,
        s: f64,
        edge: usize,
        t: f64,
    }
    let mut smp: Vec<Smp> = Vec::new();
    let mut s_acc = 0.0;
    for (ci, &(e, fwd)) in chain.iter().enumerate() {
        let ed = &b.edges[e];
        let m = 24;
        let f = &b.faces[beside[ci]];
        for i in 0..m {
            let fr = i as f64 / m as f64;
            let tt = if fwd {
                ed.t[0] + (ed.t[1] - ed.t[0]) * fr
            } else {
                ed.t[1] + (ed.t[0] - ed.t[1]) * fr
            };
            let p = ed.point(tt);
            if let Some(last) = smp.last() {
                s_acc += (p - last.p).norm();
            }
            let (u, w) = f.surf.project(&p, None);
            let n = f.surf.normal(u, w) * if f.sense { 1.0 } else { -1.0 };
            let tan = ed.curve.tangent(tt).normalize() * if fwd { 1.0 } else { -1.0 };
            smp.push(Smp {
                p,
                n,
                tan,
                s: s_acc,
                edge: e,
                t: tt,
            });
        }
    }
    let total = s_acc + (smp[0].p - smp.last().unwrap().p).norm();
    // The centre: the rim's centroid, domed towards the corner.
    let g = P3::from(smp.iter().fold(V3::zeros(), |a, x| a + x.p.coords) / smp.len() as f64);
    let to_v = vp - g;
    let c = g + to_v * 0.45;
    let axis = to_v.try_normalize(tol::DEGENERATE).unwrap_or(out);
    // Rows by length v ∈ [0, 1): the rim and the row leaning along the neighbours.
    let n = smp.len();
    let params: Vec<f64> = (0..=n)
        .map(|i| if i < n { smp[i].s / total } else { 1.0 })
        .collect();
    let mut rim_pts: Vec<P3> = smp.iter().map(|x| x.p).collect();
    rim_pts.push(smp[0].p);
    let mut lean_pts: Vec<P3> = Vec::with_capacity(n + 1);
    for x in &smp {
        // Into the hole along the neighbour's tangent plane, square to the rim.
        let mut m = x.n.cross(&x.tan);
        if m.dot(&(c - x.p)) < 0.0 {
            m = -m;
        }
        let reach = (x.p - c).norm();
        lean_pts.push(x.p + m.normalize() * (0.33 * reach));
    }
    lean_pts.push(lean_pts[0]);
    // Closed rows: the end tangents agree (the rim's direction at the seam point).
    let d_rim = smp[0].tan * total;
    let row3 = NurbsCurve::interpolate(&rim_pts, &params, 3, Some(d_rim), Some(d_rim));
    let d_lean = (lean_pts[1] - lean_pts[n - 1]) / (params[1] + (1.0 - params[n - 1]));
    let row2 = NurbsCurve::interpolate(&lean_pts, &params, 3, Some(d_lean), Some(d_lean));
    if row3.cps.len() != row2.cps.len() || row3.knots != row2.knots {
        return Err(KernelError::InvalidGeometry {
            op: "smooth corner",
            why: "the corner patch's rows don't match".into(),
        });
    }
    // The pole's rows: the centre, and the next row in the plane through it square to the axis.
    let perp = |p: &P3| {
        let w = p - c;
        c + (w - axis * w.dot(&axis)) * 0.4
    };
    let nv = row3.cps.len();
    let cps: Vec<Vec<H>> = (0..4)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    let p3 = super::nurbs::dehom(&row3.cps[j]);
                    let p2 = super::nurbs::dehom(&row2.cps[j]);
                    let p = match i {
                        0 => c,
                        1 => perp(&p2),
                        2 => p2,
                        _ => p3,
                    };
                    H::new(p.x, p.y, p.z, 1.0)
                })
                .collect()
        })
        .collect();
    let patch = NurbsSurface {
        screw: None,
        du: 3,
        dv: 3,
        ku: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        kv: row3.knots.clone(),
        cps,
    };
    let surf = Surface::Nurbs(Arc::new(patch));
    // The face: the rim (u = 1, v by length), the seam (v = 1 then v = 0) and the pole (u = 0).
    let mut body = t.body.clone();
    let pole = body.add_vertex(c);
    let seam_curve = Curve::Nurbs(Arc::new(NurbsCurve {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        cps: (0..4).map(|i| surf_row_point(&surf, i, 0)).collect(),
    }));
    let v0 = start_of(&chain[0]);
    let seam = body.add_edge(seam_curve, 0.0, 1.0, pole, v0);
    let pole_e = body.add_degenerate(c, 0.0, 1.0, pole);
    let line2 = super::sweep::line2;
    let mut lp: Vec<Coedge> = Vec::new();
    // The rim's p-curves: (1, v(t)), v from the samples' lengths.
    let mut k = 0;
    for &(e, fwd) in &chain {
        let ed = body.edges[e].clone();
        let first = k;
        while k < smp.len() && smp[k].edge == e {
            k += 1;
        }
        // (t, v) of this edge's samples, and its far end.
        let mut ts: Vec<f64> = smp[first..k].iter().map(|x| x.t).collect();
        let mut vs: Vec<f64> = smp[first..k].iter().map(|x| x.s / total).collect();
        let (t_end, v_end) = if fwd {
            (ed.t[1], if k < smp.len() { smp[k].s / total } else { 1.0 })
        } else {
            (ed.t[0], if k < smp.len() { smp[k].s / total } else { 1.0 })
        };
        ts.push(t_end);
        vs.push(v_end);
        // In increasing t.
        if !fwd {
            ts.reverse();
            vs.reverse();
        }
        let pts: Vec<P3> = vs.iter().map(|&v| P3::new(1.0, v, 0.0)).collect();
        let pc = NurbsCurve::interpolate(&pts, &ts, 3.min(pts.len() - 1), None, None);
        lp.push(Coedge {
            edge: e,
            fwd,
            pc: Curve::Nurbs(Arc::new(pc)),
        });
    }
    // Back down the seam at v = 1, round the pole, up the seam at v = 0.
    lp.push(Coedge {
        edge: seam,
        fwd: false,
        pc: line2(P2::new(0.0, 1.0), V2::new(1.0, 0.0)),
    });
    lp.push(Coedge {
        edge: pole_e,
        fwd: true,
        pc: line2(P2::new(0.0, 1.0), V2::new(0.0, -1.0)),
    });
    lp.push(Coedge {
        edge: seam,
        fwd: true,
        pc: line2(P2::new(0.0, 0.0), V2::new(1.0, 0.0)),
    });
    let face = Face {
        surf,
        sense: true,
        loops: vec![lp],
    };
    // Replace the hole's faces.
    let keep: Vec<usize> = (0..body.faces.len())
        .filter(|f| !hole.contains(f))
        .collect();
    let mut faces: Vec<Face> = keep.iter().map(|&f| body.faces[f].clone()).collect();
    let mut src: Vec<Vec<FaceSrc>> = keep.iter().map(|&f| t.src[f].clone()).collect();
    faces.push(face);
    src.push(vec![mark.clone()]);
    body.faces = faces;
    let nf = body.faces.len();
    super::build::fix_senses(&mut body, nf - 1..nf);
    if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
        let f = &body.faces[nf - 1];
        let mut worst: f64 = 0.0;
        for x in &smp {
            let (u, w) = f.surf.project(&x.p, None);
            let n = f.surf.normal(u, w) * if f.sense { 1.0 } else { -1.0 };
            let a = n.dot(&x.n).clamp(-1.0, 1.0).acos().to_degrees();
            if a > worst {
                worst = a;
                eprintln!(
                    "corner: rim sample s {:.3} at ({u:.4}, {w:.4}), angle {a:.3}°, off {:.2e}",
                    x.s / total,
                    (f.surf.eval(u, w) - x.p).norm()
                );
            }
        }
        eprintln!("corner: worst rim angle {worst:.3}°, sense {}", f.sense);
    }
    super::unify::compact(&mut body);
    body.shells.clear();
    body.solids.clear();
    super::boolean::assemble_shells(&mut body)?;
    if body.solids.len() != 1 {
        return Err(KernelError::InvalidResult {
            op: "smooth corner",
            why: "the smooth corner left an open body".into(),
        });
    }
    t.body = body;
    t.src = src;
    Ok(t)
}

/// Row `i` (u) of the patch's control net at column `j`, as a point.
fn surf_row_point(s: &Surface, i: usize, j: usize) -> H {
    let Surface::Nurbs(n) = s else { unreachable!() };
    n.cps[i][j]
}
