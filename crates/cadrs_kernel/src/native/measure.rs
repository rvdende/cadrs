//! Shape interrogation: principal curvatures of the boundary, plane sections and the areas they
//! cut, the Betti numbers of a solid set, and how two solids interfere.

use super::boolean::{BoolOut, boolean_bodies};
use super::distance::{Piece, Sense, Where, extremum, point_pieces};
use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use super::uvdomain::FaceDomain;
use crate::*;

/// The principal curvatures of `s` at (u, v) against its natural normal `Su × Sv` (negative
/// where the surface bends away from that normal, as a ball does from its outward one), the
/// larger first; `None` where the surface has no tangent plane (a cone's apex).
pub fn principal_curvatures(s: &Surface, u: f64, v: f64) -> Option<[f64; 2]> {
    let d = s.d2(u, v);
    let (su, sv) = (d[1], d[2]);
    let (e, f, g) = (su.dot(&su), su.dot(&sv), sv.dot(&sv));
    let det = e * g - f * f;
    if det <= tol::DEGENERATE * (e + g) * (e + g) {
        // A pole of the parametrization. A ball bends alike every way; other surfaces are taken
        // a little inside the pole, where they have a tangent plane.
        return match s {
            Surface::Sphere { r, .. } => Some([-1.0 / r, -1.0 / r]),
            Surface::Cone { .. } => None,
            _ => {
                let h = tol::DIFF_STEP;
                [(0.0, h), (0.0, -h), (h, 0.0), (-h, 0.0)]
                    .into_iter()
                    .find_map(|(du, dv)| {
                        let d = s.d2(u + du, v + dv);
                        let (su, sv) = (d[1], d[2]);
                        let (e, f, g) = (su.dot(&su), su.dot(&sv), sv.dot(&sv));
                        (e * g - f * f > tol::DEGENERATE * (e + g) * (e + g))
                            .then(|| principal_curvatures(s, u + du, v + dv))
                            .flatten()
                    })
            }
        };
    }
    let n = su.cross(&sv) / det.sqrt();
    let (l, m, nn) = (d[3].dot(&n), d[4].dot(&n), d[5].dot(&n));
    // The roots of det(II − κ I) = 0: κ = H ± √(H² − K).
    let h = (e * nn + g * l - 2.0 * f * m) / (2.0 * det);
    let k = (l * nn - m * m) / det;
    let r = (h * h - k).max(0.0).sqrt();
    Some([h + r, h - r])
}

/// The outward normal and principal curvatures (positive where the boundary bends away from its
/// outward normal) of face `fi` of `b` at the point of its surface nearest `q`, from (u, v) when
/// known.
fn face_bending(b: &Body, fi: usize, q: &P3, uv: Option<(f64, f64)>) -> (V3, Option<[f64; 2]>) {
    let f = &b.faces[fi];
    let (u, v) = uv.unwrap_or_else(|| f.surf.project(q, None));
    let sign = if f.sense { 1.0 } else { -1.0 };
    let n = f.surf.normal(u, v) * sign;
    // Against the outward normal −κ, so bending away from it is positive.
    let k = principal_curvatures(&f.surf, u, v).map(|[a, c]| {
        let (a, c) = (-sign * a, -sign * c);
        if a >= c { [a, c] } else { [c, a] }
    });
    (n, k)
}

/// The faces of `b` that use edge `ei`.
fn edge_faces(b: &Body, ei: usize) -> Vec<usize> {
    (0..b.faces.len())
        .filter(|&f| b.faces[f].loops.iter().flatten().any(|c| c.edge == ei))
        .collect()
}

/// The faces of `b` that have vertex `vi` on a loop.
fn vertex_faces(b: &Body, vi: usize) -> Vec<usize> {
    (0..b.faces.len())
        .filter(|&f| {
            b.faces[f]
                .loops
                .iter()
                .flatten()
                .any(|c| b.edges[c.edge].v.contains(&vi))
        })
        .collect()
}

/// The pieces of `faces` of `b` for a distance search (`polys`: the body's faces' uv
/// polygons): the faces, the edges of their loops and those edges' vertices; not the free edges
/// and points a body may also hold (a ball keeps its axis as a free edge).
fn faces_pieces<'a>(b: &'a Body, polys: &'a [Vec<Vec<P2>>], faces: &[usize]) -> Vec<Piece<'a>> {
    let mut edges: Vec<usize> = faces
        .iter()
        .flat_map(|&f| b.faces[f].loops.iter().flatten().map(|c| c.edge))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut verts: Vec<usize> = edges.iter().flat_map(|&e| b.edges[e].v).collect();
    verts.sort_unstable();
    verts.dedup();
    let mut out: Vec<Piece> = faces
        .iter()
        .map(|&fi| Piece::Face {
            dom: FaceDomain::new(&b.faces[fi].surf, &polys[fi]),
            face: fi,
        })
        .collect();
    out.extend(
        edges
            .into_iter()
            .filter(|&e| !b.edges[e].degenerate)
            .map(|e| Piece::Curve {
                c: &b.edges[e].curve,
                t: (b.edges[e].t[0], b.edges[e].t[1]),
                edge: e,
                ends: b.edges[e].v,
            }),
    );
    out.extend(verts.into_iter().map(|v| Piece::Point {
        p: b.verts[v].p,
        at: Where::Vertex(v),
    }));
    out
}

/// How far apart the outward normals of faces meeting at a point may turn for the boundary to
/// be smooth there (radians): a tangent edge's faces are tangent to rounding.
const SMOOTH: f64 = 1e-6;

/// [`Kernel::curvatures_at`]: for each point, the principal curvatures of the boundary at the
/// point of it nearest, or `None` where that point is on a crease.
pub fn curvatures_at(
    k: &NativeKernel,
    body: BodyId,
    points: &[P3],
) -> Result<Vec<Option<[f64; 2]>>> {
    let en = k.entry(body)?;
    let b = &*en.body;
    if b.faces.is_empty() {
        return Err(KernelError::InvalidInput("a body without faces".into()));
    }
    let all: Vec<usize> = (0..b.faces.len()).collect();
    let pieces = faces_pieces(b, en.polys(), &all);
    points
        .iter()
        .map(|p| {
            if !p.iter().all(|x| x.is_finite()) {
                return Err(KernelError::InvalidParameter(
                    "a point that isn't finite".into(),
                ));
            }
            let x = extremum(&point_pieces(*p), &pieces, Sense::Min).ok_or_else(|| {
                KernelError::EmptyResult("nothing to measure from: no faces".into())
            })?;
            let q = x.b;
            let at: Vec<(usize, Option<(f64, f64)>)> = match x.wb {
                Where::Face(f, u, v) => vec![(f, Some((u, v)))],
                Where::Edge(e, _) => edge_faces(b, e).into_iter().map(|f| (f, None)).collect(),
                Where::Vertex(v) => vertex_faces(b, v).into_iter().map(|f| (f, None)).collect(),
                Where::Point => vec![],
            };
            let bends: Vec<_> = at
                .iter()
                .map(|&(f, uv)| face_bending(b, f, &q, uv))
                .collect();
            let Some((n0, k0)) = bends.first() else {
                return Ok(None);
            };
            // A crease: faces meeting there at an angle.
            if bends.iter().any(|(n, _)| n.dot(n0) < SMOOTH.cos()) {
                return Ok(None);
            }
            Ok(*k0)
        })
        .collect()
}

/// Whether the plane through `o` square to the unit `n` crosses `b` strictly: some of it lies on
/// each side.
fn plane_crosses(b: &Body, o: &P3, n: &V3) -> bool {
    let (lo, hi) = super::kernel::extent_along(b, o, n);
    lo < -tol::CONFUSION && hi > tol::CONFUSION
}

fn unit_normal(n: &V3) -> Result<V3> {
    let l = n.norm();
    if !(l.is_finite() && l > tol::TINY) || !n.iter().all(|x| x.is_finite()) {
        return Err(KernelError::InvalidParameter(
            "the plane's normal has no direction".into(),
        ));
    }
    Ok(n / l)
}

/// The part of the solids of `b` on the normal's side of the plane through `o` square to the
/// unit `n`, and its faces from the half space: the cut. `None` where the plane misses them.
/// After springrep's `measure::section` (MIT, https://github.com/springcraft-ai/springrep): one
/// boolean gives both the cut's outline, as its faces' loops in order, and its area.
fn caps(b: &Body, o: P3, n: V3) -> Result<Option<(Body, Vec<usize>)>> {
    if b.solids.is_empty() || !plane_crosses(b, &o, &n) {
        return Ok(None);
    }
    let half = super::ops::half_space_box(o, n, &b.bbox())?;
    let r: BoolOut = match boolean_bodies(b, &half, BoolOp::Intersect) {
        Ok(r) => r,
        Err(KernelError::EmptyResult(_)) => return Ok(None),
        Err(e) => return Err(e),
    };
    let caps = r
        .from
        .iter()
        .enumerate()
        .filter(|(_, srcs)| srcs.iter().any(|&(owner, _)| owner == 1))
        .map(|(f, _)| f)
        .collect();
    Ok(Some((r.body, caps)))
}

/// The section of the boundary of `b` by the plane through `o` square to the unit `n` as loops
/// of edges (each an edge of the section body and whether the loop runs along it), chained by
/// their shared vertices: what a cut is made of where the boolean that makes its faces fails.
fn section_loops(b: &Body, o: P3, n: V3) -> Result<(Body, Vec<Vec<(usize, bool)>>)> {
    let bb = b.bbox();
    let sheet = super::ops::plane_sheet(
        o,
        n,
        bb.diagonal() * 2.0 + (bb.center() - o).norm() * 2.0 + 10.0,
    );
    let sec = match super::boolean::section(b, &sheet) {
        Ok(s) => s,
        Err(KernelError::EmptyResult(_)) => return Ok((Body::default(), Vec::new())),
        Err(e) => return Err(e),
    };
    // Each stretch once (an edge found twice, from both sides of a face pair, counts once).
    let mut edges: Vec<usize> = Vec::new();
    for e in (0..sec.edges.len()).filter(|&e| !sec.edges[e].degenerate) {
        let ed = &sec.edges[e];
        let (mid, len) = (ed.point(ed.mid_t()), ed.curve.length(ed.t[0], ed.t[1]));
        let twin = edges.iter().any(|&x| {
            let o = &sec.edges[x];
            (o.point(o.mid_t()) - mid).norm() < tol::POINT
                && (o.curve.length(o.t[0], o.t[1]) - len).abs() < tol::POINT
        });
        if !twin {
            edges.push(e);
        }
    }
    // Chained end to start by where their ends are (two edges may end at one point with
    // vertices of their own), the nearest end within the loop gap next.
    let gap = tol::LOOP_GAP * bb.diagonal().max(1.0);
    let end = |e: usize, k: usize| sec.verts[sec.edges[e].v[k]].p;
    let mut used = vec![false; sec.edges.len()];
    let mut loops = Vec::new();
    for &e0 in &edges {
        if used[e0] {
            continue;
        }
        used[e0] = true;
        let start = end(e0, 0);
        let mut lp = vec![(e0, true)];
        let mut at = end(e0, 1);
        while (at - start).norm() > gap {
            let next = edges
                .iter()
                .filter(|&&e| !used[e])
                .flat_map(|&e| {
                    [
                        (e, true, (end(e, 0) - at).norm()),
                        (e, false, (end(e, 1) - at).norm()),
                    ]
                })
                .filter(|x| x.2 <= gap)
                .min_by(|x, y| x.2.total_cmp(&y.2));
            let Some((e, fwd, _)) = next else {
                return Err(KernelError::OpenResult(
                    "the plane's section doesn't close".into(),
                ));
            };
            used[e] = true;
            at = end(e, usize::from(fwd));
            lp.push((e, fwd));
        }
        loops.push(lp);
    }
    Ok((sec, loops))
}

/// The polyline of edge `e` of `b` within `chord` of its curve, its ends at its vertices, run
/// backwards unless `fwd`, its last point left out (the next run's first).
fn edge_run(b: &Body, e: usize, fwd: bool, chord: f64) -> Vec<P3> {
    let ed = &b.edges[e];
    let ts = ed
        .curve
        .sample_params(ed.t[0], ed.t[1], chord, std::f64::consts::FRAC_PI_4);
    let mut run: Vec<P3> = ts.iter().map(|&t| ed.point(t)).collect();
    if let Some(a) = run.first_mut() {
        *a = b.verts[ed.v[0]].p;
    }
    if let Some(z) = run.last_mut() {
        *z = b.verts[ed.v[1]].p;
    }
    if !fwd {
        run.reverse();
    }
    run.pop();
    run
}

/// The closed polylines of loops of edges of `b` (each closed on its first point).
fn loop_polylines(b: &Body, loops: &[Vec<(usize, bool)>], chord: f64) -> Vec<Vec<P3>> {
    loops
        .iter()
        .filter_map(|lp| {
            let mut pts: Vec<P3> = lp
                .iter()
                .flat_map(|&(e, fwd)| edge_run(b, e, fwd, chord))
                .collect();
            let first = *pts.first()?;
            pts.push(first);
            Some(pts)
        })
        .collect()
}

/// The area the loops of edges of `b` on the plane through `o` square to `n` enclose: each
/// loop's by Green's theorem along its exact curves (Gauss–Legendre on each smooth piece), a
/// loop inside an odd number of others a hole.
fn loops_area(b: &Body, loops: &[Vec<(usize, bool)>], o: P3, n: V3) -> f64 {
    let f = Frame::from_z(o, n);
    let flat = |p: P3| {
        let d = p - f.o;
        (d.dot(&f.x), d.dot(&f.y))
    };
    let areas: Vec<f64> = loops
        .iter()
        .map(|lp| {
            let mut a = 0.0;
            for &(e, fwd) in lp {
                let ed = &b.edges[e];
                let ps = ed.curve.pieces(ed.t[0], ed.t[1]);
                let mut s = 0.0;
                for w in ps.windows(2) {
                    s += integrate(w[0], w[1], 1, &GAUSS16, |t| {
                        let (p, d, _) = ed.curve.d2(t);
                        let (x, y) = flat(p);
                        let (dx, dy) = (d.dot(&f.x), d.dot(&f.y));
                        0.5 * (x * dy - y * dx)
                    });
                }
                a += if fwd { s } else { -s };
            }
            a.abs()
        })
        .collect();
    let polys: Vec<Vec<(f64, f64)>> = loop_polylines(b, loops, 1e-3 * b.bbox().diagonal().max(1.0))
        .into_iter()
        .map(|pl| pl.into_iter().map(flat).collect())
        .collect();
    let inside = |q: (f64, f64), pl: &[(f64, f64)]| {
        let mut odd = false;
        for w in pl.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if (y0 > q.1) != (y1 > q.1) && q.0 < x0 + (q.1 - y0) * (x1 - x0) / (y1 - y0) {
                odd = !odd;
            }
        }
        odd
    };
    (0..loops.len())
        .map(|i| {
            let q = flat(b.verts[b.edges[loops[i][0].0].v[0]].p);
            let depth = (0..loops.len())
                .filter(|&j| j != i && inside(q, &polys[j]))
                .count();
            if depth % 2 == 0 { areas[i] } else { -areas[i] }
        })
        .sum()
}

/// [`Kernel::plane_section`]: the closed curves the plane through `o` square to `n` cuts from
/// the boundary of the solids of `body`, each one polyline round a loop of the cut (its edges'
/// chords within `chord` of their curves) ending on its first point.
pub fn plane_section(
    k: &NativeKernel,
    body: BodyId,
    o: P3,
    n: V3,
    chord: f64,
) -> Result<Vec<Vec<P3>>> {
    if !(chord.is_finite() && chord > 0.0) {
        return Err(KernelError::InvalidParameter(
            "the chord tolerance must be above zero".into(),
        ));
    }
    let n = unit_normal(&n)?;
    let b = k.get(body)?;
    let (cut, faces) = match caps(b, o, n) {
        Ok(Some(c)) => c,
        Ok(None) => return Ok(Vec::new()),
        // The cut's faces not made: the section's edges chained into its loops.
        Err(_) => {
            let (sec, loops) = section_loops(b, o, n)?;
            return Ok(loop_polylines(&sec, &loops, chord));
        }
    };
    let mut out = Vec::new();
    for f in faces {
        for lp in &cut.faces[f].loops {
            let mut pts: Vec<P3> = Vec::new();
            for c in lp {
                if !cut.edges[c.edge].degenerate {
                    pts.extend(edge_run(&cut, c.edge, c.fwd, chord));
                }
            }
            if let Some(&first) = pts.first() {
                pts.push(first);
                out.push(pts);
            }
        }
    }
    Ok(out)
}

/// [`Kernel::section_area`]: the area of the cut the plane through `o` square to `n` makes
/// through the solids of `body`, 0 where it misses them.
pub fn section_area(k: &NativeKernel, body: BodyId, o: P3, n: V3) -> Result<f64> {
    let n = unit_normal(&n)?;
    let b = k.get(body)?;
    let (cut, faces) = match caps(b, o, n) {
        Ok(Some(c)) => c,
        Ok(None) => return Ok(0.0),
        // The cut's faces not made: the area its loops of edges enclose.
        Err(_) => {
            let (sec, loops) = section_loops(b, o, n)?;
            return Ok(loops_area(&sec, &loops, o, n));
        }
    };
    Ok(faces
        .into_iter()
        .map(|f| super::mass::face_area(&cut, f).0.abs())
        .sum())
}

/// The Euler characteristic of a face as a surface with boundary: a disk with holes, `2 − L` for
/// `L` loops (a closed periodic face is cut open by its seams, which its loops run along); a
/// face without loops is the whole closed surface.
fn face_euler(b: &Body, fi: usize) -> i64 {
    let f = &b.faces[fi];
    if f.loops.is_empty() {
        return if f.surf.u_periodic() && f.surf.v_periodic() {
            0
        } else {
            2
        };
    }
    2 - f.loops.len() as i64
}

/// The Euler characteristic of the surface shell `si` of `b` makes: V − E + Σ χ(face), point
/// edges (poles, apexes) not counted as edges.
fn shell_euler(b: &Body, si: usize) -> i64 {
    let mut verts = Vec::new();
    let mut edges = Vec::new();
    let mut faces = 0;
    for &fi in &b.shells[si].faces {
        faces += face_euler(b, fi);
        for c in b.faces[fi].loops.iter().flatten() {
            let e = &b.edges[c.edge];
            verts.extend(e.v);
            if !e.degenerate {
                edges.push(c.edge);
            }
        }
    }
    for l in [&mut verts, &mut edges] {
        l.sort_unstable();
        l.dedup();
    }
    verts.len() as i64 - edges.len() as i64 + faces
}

/// [`Kernel::betti_numbers`]: the separate pieces, tunnels and voids of the solids of `body` as
/// one point set. Each solid is a manifold, `χ = χ(∂)/2 = b0 − b1 + b2` with its voids its
/// inner shells; solids that touch are one piece, and touching round in a ring makes a tunnel.
pub fn betti_numbers(k: &NativeKernel, body: BodyId) -> Result<[u64; 3]> {
    let en = k.entry(body)?;
    let b = &*en.body;
    let ns = b.solids.len();
    if ns == 0 {
        return Ok([0, 0, 0]);
    }
    let mut b1: i64 = 0;
    let mut b2: i64 = 0;
    for s in &b.solids {
        let chi2: i64 = s.shells.iter().map(|&sh| shell_euler(b, sh)).sum();
        let voids = s.shells.len() as i64 - 1;
        b2 += voids;
        b1 += 1 + voids - chi2 / 2;
    }
    // The solids that touch: they share a vertex, or come within a point's tolerance.
    let verts_of: Vec<Vec<usize>> = (0..ns)
        .map(|s| {
            let mut v: Vec<usize> = b
                .solid_faces(s)
                .into_iter()
                .flat_map(|f| {
                    b.faces[f]
                        .loops
                        .iter()
                        .flatten()
                        .flat_map(|c| b.edges[c.edge].v)
                })
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    let polys = en.polys();
    let of_solid = |s: usize| faces_pieces(b, polys, &b.solid_faces(s));
    let boxes: Vec<Bbox> = (0..ns)
        .map(|s| {
            let mut bb = Bbox::empty();
            for f in b.solid_faces(s) {
                bb.union(&b.face_bbox(f));
            }
            bb.grow(tol::POINT)
        })
        .collect();
    let mut parent: Vec<usize> = (0..ns).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut contacts: i64 = 0;
    for i in 0..ns {
        for j in i + 1..ns {
            if !boxes[i].overlaps(&boxes[j]) {
                continue;
            }
            let shared = verts_of[i]
                .iter()
                .any(|v| verts_of[j].binary_search(v).is_ok());
            let touch = shared
                || extremum(&of_solid(i), &of_solid(j), Sense::Min)
                    .is_some_and(|x| x.distance <= tol::POINT);
            if touch {
                contacts += 1;
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let b0 = (0..ns).filter(|&i| root(&mut parent, i) == i).count() as i64;
    // Each contact past a tree of them closes a loop through the solids it joins.
    b1 += contacts - ns as i64 + b0;
    Ok([b0 as u64, b1.max(0) as u64, b2.max(0) as u64])
}

/// [`Kernel::interference`] of two solid bodies: the volume they share and the gap between
/// them.
pub fn interference(k: &mut NativeKernel, a: BodyId, b: BodyId) -> Result<Interference> {
    for id in [a, b] {
        if k.get(id)?.solids.is_empty() {
            return Err(KernelError::InvalidInput(
                "interference is of solid bodies".into(),
            ));
        }
    }
    // After springrep's `overlap::between` (MIT): what they share first, the gap only where
    // they share nothing (a distance search between crossing boundaries is the dearer half).
    let gap = |k: &NativeKernel| -> Result<f64> {
        Ok(super::distance::kernel_distance(
            k,
            &DistanceTarget::Solid(a),
            &DistanceTarget::Solid(b),
            Sense::Min,
        )?
        .distance)
    };
    if k.surely_apart(a, b)? {
        return Ok(Interference {
            volume: 0.0,
            gap: gap(k)?,
        });
    }
    let volume = match k.boolean(BoolOp::Intersect, a, &[b]) {
        Ok(r) => {
            let v = r
                .bodies
                .iter()
                .map(|&x| k.get(x).map_or(0.0, |x| super::mass::body_volume(x)))
                .sum::<f64>();
            for x in r.bodies {
                k.release(x);
            }
            v
        }
        Err(KernelError::EmptyResult(_)) => 0.0,
        Err(e) => return Err(e),
    };
    // Solids that only touch share a face's worth of roundoff, not a volume.
    let scale = [a, b]
        .iter()
        .map(|&x| k.get(x).map_or(0.0, |x| x.bbox().diagonal()))
        .fold(0.0, f64::max);
    if volume <= tol::REL_POINT * tol::POINT * scale * scale {
        return Ok(Interference {
            volume: 0.0,
            gap: gap(k)?,
        });
    }
    Ok(Interference { volume, gap: 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The area of the section's loops of edges (the path taken where the cut's faces aren't
    /// made) against the closed form: a block with a round hole, and a ball.
    #[test]
    fn section_loops_enclose_the_cut() {
        let mut k = NativeKernel::new();
        let p = |x, y, z| P3::new(x, y, z);
        let block = k
            .primitive(&Primitive::Box {
                min: p(-10.0, -10.0, 0.0),
                max: p(10.0, 10.0, 5.0),
            })
            .unwrap()
            .bodies[0];
        let rod = k
            .primitive(&Primitive::Cylinder {
                base: p(0.0, 0.0, -1.0),
                axis: nalgebra::Vector3::z_axis(),
                radius: 3.0,
                height: 7.0,
            })
            .unwrap()
            .bodies[0];
        let holed = k.boolean(BoolOp::Subtract, block, &[rod]).unwrap().bodies[0];
        let (o, n) = (p(0.0, 0.0, 2.0), V3::z());
        let (sec, loops) = section_loops(k.get(holed).unwrap(), o, n).unwrap();
        assert_eq!(loops.len(), 2);
        let want = 400.0 - std::f64::consts::PI * 9.0;
        let got = loops_area(&sec, &loops, o, n);
        assert!((got - want).abs() < 1e-9 * want, "{got} against {want}");
        assert!((section_area(&k, holed, o, n).unwrap() - want).abs() < 1e-9 * want);
        let ball = k
            .primitive(&Primitive::Sphere {
                center: P3::origin(),
                radius: 5.0,
                axis: nalgebra::Vector3::z_axis(),
                seam: nalgebra::Vector3::x_axis(),
            })
            .unwrap()
            .bodies[0];
        let n = V3::new(1.0, 2.0, 2.0) / 3.0;
        let o = P3::from(n * 3.0);
        let (sec, loops) = section_loops(k.get(ball).unwrap(), o, n).unwrap();
        let want = std::f64::consts::PI * 16.0;
        let got = loops_area(&sec, &loops, o, n);
        assert!((got - want).abs() < 1e-9 * want, "{got} against {want}");
    }
}
