//! The face–face rule of [`super::topo::CheckLevel::Full`]: two faces of one shell don't cross
//! each other away from the edges they share.
//!
//! Only pairs that could cross are tested: faces of the same shell whose boxes (grown by their
//! tolerances) overlap, found with a [`Bvh`], that share no edge (neighbours meet along their
//! edge by construction, and how they meet there is the edges' check). Two tests, each cheap:
//! - an edge of one face piercing the other face's interior with its middle (crossing its
//!   surface, not touching it, clear of its boundary by more than the faces' tolerances, not at
//!   the edge's end; on a B-spline or swept face found by a bounded sampled search, not the full
//!   curve–surface intersection): where faces cross, their intersection curve runs out through
//!   an edge of one of them unless it closes inside both;
//! - for two elementary surfaces (planes, cylinders, cones, spheres, tori), whose intersection
//!   has a closed form, that curve itself ([`super::ssi`]) sampled: a point inside both faces,
//!   clear of their boundaries, is a crossing. This also finds closed crossings.
//!
//! A closed crossing of B-spline or swept faces that meets no edge isn't looked for: the
//! numeric intersection costs too much for a check run on every test result. A point counts
//! only when it lies on both surfaces, so an intersection the engine got wrong can't make a
//! valid body fail.

use rayon::prelude::*;

use super::bvh::Bvh;
use super::classify::{
    InFace, PolyBoxes, face_polygons, in_face_boxed, periodic_candidates_boxed, poly_boxes,
};
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;

/// Points sampled along each crossing curve.
const SAMPLES: usize = 24;

/// The faces of a shell that cross each other away from their shared edges, one problem per
/// pair (at most the first crossing point found).
pub fn face_crossings(b: &Body) -> Vec<String> {
    face_crossings_of(b, None)
}

/// [`face_crossings`] of the pairs with a face `changed` marks (all pairs for `None`).
pub fn face_crossings_of(b: &Body, changed: Option<&[bool]>) -> Vec<String> {
    let marked = |f: usize| changed.is_none_or(|c| c.get(f).copied().unwrap_or(true));
    face_crossings_where(b, &|i, j| marked(i) || marked(j))
}

/// [`face_crossings`] of the pairs `(i, j)`, `i < j`, that `wanted` takes.
pub fn face_crossings_where(
    b: &Body,
    wanted: &(dyn Fn(usize, usize) -> bool + Sync),
) -> Vec<String> {
    face_crossings_in(b, wanted, &mut vec![None; b.faces.len()])
}

/// [`face_crossings_where`] with the faces' polygons ([`face_polygons`]) kept in `polys` (one
/// per face, those it has already used, the others made as needed): for the check's later
/// tests.
pub(crate) fn face_crossings_in(
    b: &Body,
    wanted: &(dyn Fn(usize, usize) -> bool + Sync),
    polys: &mut [Option<Vec<Vec<P2>>>],
) -> Vec<String> {
    let n = b.faces.len();
    if n < 2 || b.shells.is_empty() {
        return Vec::new();
    }
    let mut shell_of: Vec<Option<usize>> = vec![None; n];
    for (si, s) in b.shells.iter().enumerate() {
        for &f in &s.faces {
            shell_of[f].get_or_insert(si);
        }
    }
    // Each face's largest tolerance (its edges' and vertices'), and its edges.
    let tol: Vec<f64> = b
        .faces
        .iter()
        .map(|f| {
            f.loops
                .iter()
                .flatten()
                .map(|c| {
                    let e = &b.edges[c.edge];
                    e.tol.max(b.verts[e.v[0]].tol).max(b.verts[e.v[1]].tol)
                })
                .fold(super::tol::POINT, f64::max)
        })
        .collect();
    let edges: Vec<std::collections::BTreeSet<usize>> = b
        .faces
        .iter()
        .map(|f| {
            f.loops
                .iter()
                .flatten()
                .filter(|c| !b.edges[c.edge].degenerate)
                .map(|c| c.edge)
                .collect()
        })
        .collect();
    let boxes: Vec<Bbox> = (0..n)
        .map(|fi| {
            if shell_of[fi].is_some() {
                b.face_bbox(fi).grow(tol[fi])
            } else {
                Bbox::empty()
            }
        })
        .collect();
    let size = b.bbox().diagonal().max(1.0);
    let bvh = Bvh::new(boxes.clone());
    let mut pairs: Vec<(usize, usize)> = bvh
        .pairs(&bvh)
        .into_iter()
        .filter(|&(i, j)| {
            i < j && shell_of[i] == shell_of[j] && wanted(i, j) && edges[i].is_disjoint(&edges[j])
        })
        .collect();
    if pairs.is_empty() {
        return Vec::new();
    }
    pairs.sort_unstable();
    // The polygons of the faces in a pair only.
    let mut paired = vec![false; n];
    for &(i, j) in &pairs {
        paired[i] = true;
        paired[j] = true;
    }
    for (fi, f) in b.faces.iter().enumerate() {
        if paired[fi] && fi < polys.len() && polys[fi].is_none() {
            polys[fi] = Some(face_polygons(b, f));
        }
    }
    let polys: &[Option<Vec<Vec<P2>>>] = polys;
    // Their boxes, for the many points tested against each ([`in_face_boxed`]).
    let pboxes: Vec<PolyBoxes> = (0..n)
        .into_par_iter()
        .map(|fi| match polys.get(fi).and_then(|p| p.as_deref()) {
            Some(p) if paired[fi] => poly_boxes(p, &b.faces[fi].surf),
            _ => Vec::new(),
        })
        .collect();
    // Edge `e` against face `y`'s surface over [t0, t1] comes up from each of e's faces paired
    // with y: its roots once.
    let roots: Roots = std::sync::Mutex::new(std::collections::HashMap::new());
    pairs
        .par_iter()
        .filter_map(|&(i, j)| {
            let ov = Bbox {
                min: boxes[i].min.sup(&boxes[j].min),
                max: boxes[i].max.inf(&boxes[j].max),
            };
            // Clear of the boundaries by the faces' tolerances, and by a part in 10⁶ of the body.
            let margin = 2.0 * tol[i].max(tol[j]) + tol::REL_POINT * size;
            let p = crossing(b, (polys, &pboxes), (i, j), &ov, margin, &roots)?;
            Some(format!(
                "faces {i} and {j} cross away from their edges near ({:.6}, {:.6}, {:.6})",
                p.x, p.y, p.z
            ))
        })
        .collect()
}

/// The uv box of a face's polygons.
fn uv_box(polys: &[Vec<P2>]) -> Option<((f64, f64), (f64, f64))> {
    let mut u = (f64::INFINITY, f64::NEG_INFINITY);
    let mut v = (f64::INFINITY, f64::NEG_INFINITY);
    for q in polys.iter().flatten() {
        u = (u.0.min(q.x), u.1.max(q.x));
        v = (v.0.min(q.y), v.1.max(q.y));
    }
    (u.0 <= u.1 && v.0 <= v.1).then_some((u, v))
}

/// Whether a surface is elementary (its intersections have closed forms).
fn elementary(s: &Surface) -> bool {
    matches!(
        s,
        Surface::Plane { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
    )
}

/// The stretch of edge `e` that can meet surface `s` inside `region`, from samples: the
/// samples in the region (grown by the samples' spacing) with one more each side, or `None`
/// when there is none, or when the surface has a signed distance and it keeps one sign along
/// the stretch by more than the curve can travel between samples.
fn near_stretch(e: &super::topo::Edge, region: &Bbox, s: &Surface) -> Option<(f64, f64)> {
    const N: usize = 32;
    let ts: Vec<f64> = (0..=N)
        .map(|k| e.t[0] + (e.t[1] - e.t[0]) * k as f64 / N as f64)
        .collect();
    let ps: Vec<P3> = ts.iter().map(|&t| e.point(t)).collect();
    // (A chord under-measures the arc between samples: half as much again covers the curves
    // of edges sampled this finely.)
    let step = ps
        .windows(2)
        .map(|w| (w[1] - w[0]).norm())
        .fold(0.0, f64::max)
        * 1.5;
    let grown = region.grow(step);
    let inside: Vec<usize> = (0..=N).filter(|&k| grown.contains(&ps[k])).collect();
    let (&first, &last) = (inside.first()?, inside.last()?);
    let (k0, k1) = (first.saturating_sub(1), (last + 1).min(N));
    if let Some(d) = (k0..=k1)
        .map(|k| s.signed_distance(&ps[k]))
        .collect::<Option<Vec<f64>>>()
        && (d.iter().all(|&x| x > step) || d.iter().all(|&x| x < -step))
    {
        return None;
    }
    Some((ts[k0], ts[k1]))
}

/// Where edge `e` passes from one side of surface `s` to the other over [t0, t1]: samples
/// projected onto the surface (each from the one before), their offsets along its normal, and
/// a bisection where the offset changes sign close to the surface. A bounded search for the
/// surfaces without a signed distance, where the full curve–surface intersection costs seconds
/// a pair: it can miss a crossing between two samples' feet, never invent one.
fn sided_roots(e: &super::topo::Edge, t0: f64, t1: f64, s: &Surface) -> Vec<f64> {
    const N: usize = 24;
    let mut hint: Option<(f64, f64)> = None;
    let side = |t: f64, hint: &mut Option<(f64, f64)>| -> (f64, f64) {
        let p = e.point(t);
        let (u, v) = s.project(&p, *hint);
        *hint = Some((u, v));
        let q = s.eval(u, v);
        let n = s.normal(u, v);
        let n = n / n.norm().max(1e-300);
        ((p - q).dot(&n), (p - q).norm())
    };
    let ts: Vec<f64> = (0..=N)
        .map(|k| t0 + (t1 - t0) * k as f64 / N as f64)
        .collect();
    let ds: Vec<(f64, f64)> = ts.iter().map(|&t| side(t, &mut hint)).collect();
    let mut out = Vec::new();
    for k in 0..N {
        let ((da, ga), (db, gb)) = (ds[k], ds[k + 1]);
        // A sign change of the offset where the feet are true (the point's distance is its
        // offset along the normal), not a jump between two feet on different sheets.
        if !(da * db < 0.0)
            || ga > 1.01 * da.abs() + tol::ROUNDOFF
            || gb > 1.01 * db.abs() + tol::ROUNDOFF
        {
            continue;
        }
        let (mut lo, mut hi, mut dlo) = (ts[k], ts[k + 1], da);
        let mut h = hint;
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            let (dm, _) = side(mid, &mut h);
            if (dm < 0.0) == (dlo < 0.0) {
                lo = mid;
                dlo = dm;
            } else {
                hi = mid;
            }
        }
        out.push(0.5 * (lo + hi));
    }
    out
}

/// Roots of an edge on a face's surface over a range of the edge's parameter, by (edge, face, the
/// range's bits).
type Roots = std::sync::Mutex<std::collections::HashMap<(usize, usize, u64, u64), Vec<f64>>>;

/// The faces' polygons (where made) and their boxes, by face.
type Polygons<'a> = (&'a [Option<Vec<Vec<P2>>>], &'a [PolyBoxes]);

/// A point where faces `i` and `j` cross (see the module docs), if any.
fn crossing(
    b: &Body,
    (polys, pboxes): Polygons,
    (i, j): (usize, usize),
    ov: &Bbox,
    margin: f64,
    cache: &Roots,
) -> Option<P3> {
    let polys = |f: usize| {
        (
            polys.get(f).and_then(|p| p.as_deref()).unwrap_or(&[]),
            pboxes.get(f).map_or(&[][..], |x| x.as_slice()),
        )
    };
    let region = ov.grow(margin);
    // Edges of one face piercing the other.
    for (x, y) in [(i, j), (j, i)] {
        let fy = &b.faces[y];
        for c in b.faces[x].loops.iter().flatten() {
            let e = &b.edges[c.edge];
            if e.degenerate
                || !super::boolean::curve_bbox(&e.curve, e.t[0], e.t[1]).overlaps(&region)
            {
                continue;
            }
            let Some((t0, t1)) = near_stretch(e, &region, &fy.surf) else {
                continue;
            };
            let roots = if elementary(&fy.surf) {
                super::ssi::curve_surface(&e.curve, t0, t1, &fy.surf)
            } else {
                let key = (c.edge, y, t0.to_bits(), t1.to_bits());
                let known = cache.lock().ok().and_then(|m| m.get(&key).cloned());
                if let Some(k) = &known
                    && super::topo::check_compare()
                {
                    assert_eq!(
                        *k,
                        sided_roots(e, t0, t1, &fy.surf),
                        "edge {} on face {y}: the roots kept differ from the roots found again",
                        c.edge
                    );
                }
                known.unwrap_or_else(|| {
                    let r = sided_roots(e, t0, t1, &fy.surf);
                    if let Ok(mut m) = cache.lock() {
                        m.insert(key, r.clone());
                    }
                    r
                })
            };
            for t in roots {
                let p = e.point(t);
                // (An edge ending on the other face touches it there, as where a wall ends
                // on a face it doesn't split: a crossing pierces the face with the edge's
                // middle.)
                let at_end =
                    e.v.iter()
                        .any(|&v| (b.verts[v].p - p).norm() <= margin + b.verts[v].tol);
                if at_end || !region.contains(&p) || !inside(b, polys(y), y, &p, margin) {
                    continue;
                }
                // Crossing the surface, not touching it.
                let (u, v) = fy.surf.project(&p, None);
                let (n, d) = (fy.surf.normal(u, v), e.curve.tangent(t));
                if n.dot(&d).abs() > 1e-3 * n.norm() * d.norm() {
                    return Some(p);
                }
            }
        }
    }
    let (fa, fb) = (&b.faces[i], &b.faces[j]);
    if !(elementary(&fa.surf) && elementary(&fb.surf)) {
        return None;
    }
    let (ua, ub) = (uv_box(polys(i).0)?, uv_box(polys(j).0)?);
    let ssi = super::ssi::surface_surface_full(&fa.surf, ua, &fb.surf, ub, ov);
    for c in &ssi.curves {
        if !(c.t1 > c.t0) {
            continue;
        }
        for k in 0..SAMPLES {
            let t = c.t0 + (c.t1 - c.t0) * (k as f64 + 0.5) / SAMPLES as f64;
            let p = c.c.point(t);
            if region.contains(&p)
                && inside(b, polys(i), i, &p, margin)
                && inside(b, polys(j), j, &p, margin)
            {
                return Some(p);
            }
        }
    }
    None
}

/// Whether `p` lies on face `fi`'s surface (within `margin`) and inside the face, farther than
/// `margin` from each of its edges.
fn inside(
    b: &Body,
    (polys, boxes): (&[Vec<P2>], &[super::classify::PolyBox]),
    fi: usize,
    p: &P3,
    margin: f64,
) -> bool {
    let f = &b.faces[fi];
    let (u, v) = f.surf.project(p, None);
    if !((f.surf.eval(u, v) - p).norm() <= margin) {
        return false;
    }
    // (`in_face` compares squared distances in the surface's scaled (u, v).)
    if !periodic_candidates_boxed(&f.surf, u, v, boxes)
        .into_iter()
        .any(|q| in_face_boxed(polys, boxes, &f.surf, q, margin * margin) == InFace::Inside)
    {
        return false;
    }
    f.loops.iter().flatten().all(|c| {
        let e = &b.edges[c.edge];
        e.degenerate || (e.point(e.curve.closest(p, e.t[0], e.t[1])) - p).norm() > margin
    })
}

#[cfg(test)]
mod tests {
    use super::super::topo::{Body, Shell};
    use super::*;

    /// Two unit-box solids' faces in one shell: no crossings when apart, and crossings when
    /// the second box is moved into the first.
    #[test]
    fn crossing_faces_are_found() {
        let cube = |at: f64| {
            let mut k = super::super::NativeKernel::new();
            match k.box_at(at, at, at, 1.0, 1.0, 1.0) {
                Ok(super::super::draw::Shape::Body(id)) => (**k.get(id).unwrap()).clone(),
                _ => unreachable!(),
            }
        };
        let mut b = cube(0.0);
        assert!(face_crossings(&b).is_empty(), "{:?}", face_crossings(&b));
        // Merge the faces of two overlapping cubes into one shell.
        let mut two: Body = cube(0.0);
        two.append(&cube(0.5));
        two.shells = vec![Shell {
            faces: (0..two.faces.len()).collect(),
            closed: false,
        }];
        two.solids.clear();
        let errs = face_crossings(&two);
        assert!(!errs.is_empty());
        assert!(
            errs.iter()
                .all(|e| e.contains("cross away from their edges")),
            "{errs:?}"
        );
        // Only pairs with a marked face: the first cube's faces marked, crossings still found;
        // none marked, none tested.
        let first: Vec<bool> = (0..two.faces.len()).map(|f| f < 6).collect();
        assert_eq!(face_crossings_of(&two, Some(&first)).len(), errs.len());
        assert!(face_crossings_of(&two, Some(&vec![false; two.faces.len()])).is_empty());
        // Apart, they don't cross.
        b.append(&cube(3.0));
        b.shells = vec![Shell {
            faces: (0..b.faces.len()).collect(),
            closed: false,
        }];
        b.solids.clear();
        assert!(face_crossings(&b).is_empty());
    }
}
