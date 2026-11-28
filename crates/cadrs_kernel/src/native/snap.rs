//! Faces of a boolean's tool lying a hair off a face of its target, moved onto that face's
//! surface for a boolean that failed without (after springrep's `boolean::snap`, MIT). Faces that
//! cross at an angle below [`TANGENT`] stay within the tolerance of each other for millimetres
//! either side of where they cross, so the boolean cannot place their crossing: a box turned a
//! microradian against its copy, or a spline prism's copy turned so. Moved onto one surface, the
//! faces are one face where they overlap, as they were meant to be, and the volume moved is at
//! most the gap times the face's area. Faces that cross at a steeper angle are left alone.

use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;

/// How far apart two faces that cross below [`TANGENT`] may lie and still be moved onto one
/// surface, mm.
const SNAP_GRAZING: f64 = 1e-4;

/// The sine of the angle at or below which two surfaces graze: within the point tolerance of
/// each other for a tenth of a millimetre either side of where they cross.
const TANGENT: f64 = 1e-6;

/// How far apart two faces may lie all over and be moved onto one surface whether they cross or
/// not, mm: twice the point tolerance, past the gaps the boolean reads one way here and the
/// other there.
fn near() -> f64 {
    2.0 * tol::point()
}

/// How far moving faces may move a vertex, in multiples of how far they were moved: further means
/// a neighbour meets the face at an angle so shallow that its corner slides along it.
const MOST_MOVED: f64 = 10.0;

/// Whether `s` and `t` are surfaces of one kind that might lie a hair apart, before any sampling.
fn alike(s: &Surface, t: &Surface) -> bool {
    let parallel = |a: &V3, b: &V3| a.cross(b).norm() <= 1e-3;
    match (s, t) {
        (Surface::Plane { f: p }, Surface::Plane { f: q }) => parallel(&p.z, &q.z),
        (Surface::Cylinder { f: p, r: a }, Surface::Cylinder { f: q, r: b }) => {
            (a - b).abs() <= SNAP_GRAZING && parallel(&p.z, &q.z)
        }
        (
            Surface::Extrusion {
                c: Curve::Nurbs(_),
                d: a,
            },
            Surface::Extrusion {
                c: Curve::Nurbs(_),
                d: b,
            },
        ) => parallel(a, b),
        _ => false,
    }
}

/// The signed distance from `s` to `p` along its natural normal.
fn signed(s: &Surface, p: &P3) -> f64 {
    if let Some(d) = s.signed_distance(p) {
        return d;
    }
    let (u, v) = s.project(p, None);
    let q = s.eval(u, v);
    let d = (p - q).norm();
    if (p - q).dot(&s.normal(u, v)) < 0.0 {
        -d
    } else {
        d
    }
}

/// Points along every edge of face `f`: where two surfaces of one kind a hair apart are furthest
/// apart over the face (their gap is linear across a plane).
fn boundary_points(b: &Body, f: usize) -> Vec<P3> {
    let mut out = Vec::new();
    for c in b.faces[f].loops.iter().flatten() {
        let e = &b.edges[c.edge];
        if e.degenerate {
            out.push(b.verts[e.v[0]].p);
            continue;
        }
        for k in 0..=16 {
            out.push(e.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 16.0));
        }
    }
    out
}

/// How far a face of `sb` through `pts` lies from `sa` at most, where it is to be moved onto it:
/// within [`SNAP_GRAZING`] where it crosses `sa` (lies on both sides of it) with the two
/// surfaces' normals within [`TANGENT`] of each other everywhere, and further off than the point
/// tolerance (within that, the boolean takes them for one surface already).
fn snaps(sa: &Surface, sb: &Surface, pts: &[P3], before: bool) -> Option<f64> {
    let mut d = Vec::with_capacity(pts.len());
    for p in pts {
        let x = signed(sa, p);
        if x.abs() > SNAP_GRAZING {
            return None;
        }
        d.push(x);
    }
    let worst = d.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if worst == 0.0 {
        return None;
    }
    let grazing = |within: f64| {
        pts.iter().all(|p| {
            let (ua, va) = sa.project(p, None);
            let (ub, vb) = sb.project(p, None);
            let (na, nb) = (sa.normal(ua, va), sb.normal(ub, vb));
            match (na.try_normalize(tol::TINY), nb.try_normalize(tol::TINY)) {
                (Some(na), Some(nb)) => na.dot(&nb) > 0.0 && na.cross(&nb).norm() <= within,
                _ => false,
            }
        })
    };
    // Within a hair all over (near the point tolerance, where the boolean takes them for one
    // surface in some places and not in others), crossing or not.
    // (Their normals then need only run the same way to well within a degree's thousandth: a
    // copy turned a microradian and lying within the tolerance all over.)
    if worst <= near() && grazing(1e-4) {
        // (Before any boolean only past half the point tolerance: nearer, the boolean takes the
        // two for one surface everywhere.)
        return (!before || worst > 0.5 * tol::point()).then_some(worst);
    }
    if before || worst <= tol::point() {
        return None;
    }
    let crosses = d.iter().any(|&x| x > 0.5 * worst) && d.iter().any(|&x| x < -0.5 * worst);
    (crosses && grazing(TANGENT)).then_some(worst)
}

/// `b` with each of its faces that grazes a face of `a` a hair off it ([`snaps`]) moved onto that
/// face's surface; None where none does, or where moving them does not keep `b` a valid solid
/// with every vertex within [`MOST_MOVED`] times the gap of where it was. All at once where they
/// fit together, else one at a time, keeping each that does.
///
/// `before`: for a boolean not yet tried, only faces within [`near()`] all over (where the boolean
/// would read the gap one way here and the other there); else, for a boolean refused, those
/// crossing at a grazing angle too.
pub fn snapped(a: &Body, b: &Body, before: bool) -> Option<Body> {
    let reach = b.bbox().grow(SNAP_GRAZING);
    let a_faces: Vec<(usize, Bbox)> = (0..a.faces.len())
        .map(|f| (f, a.face_bbox(f).grow(SNAP_GRAZING)))
        .filter(|(_, bb)| bb.overlaps(&reach))
        .collect();
    let mut moved: Vec<(usize, Surface)> = Vec::new();
    let mut gap: f64 = 0.0;
    for fb in 0..b.faces.len() {
        let sb = &b.faces[fb].surf;
        let bb = b.face_bbox(fb);
        let mut pts: Option<Vec<P3>> = None;
        for (fa, ab) in &a_faces {
            let sa = &a.faces[*fa].surf;
            if !ab.overlaps(&bb) || !alike(sa, sb) {
                continue;
            }
            let pts = pts.get_or_insert_with(|| boundary_points(b, fb));
            if super::env_flag!("CADRS_SNAP_DEBUG") {
                let d: Vec<f64> = pts.iter().map(|p| signed(sa, p)).collect();
                let w = d.iter().fold(0.0f64, |m, x| m.max(x.abs()));
                eprintln!(
                    "snap? b face {fb} a face {fa}: worst {w:.3e} -> {:?}",
                    snaps(sa, sb, pts, before)
                );
            }
            if let Some(worst) = snaps(sa, sb, pts, before) {
                moved.push((fb, sa.clone()));
                gap = gap.max(worst);
                break;
            }
        }
    }
    if moved.is_empty() {
        return None;
    }
    if super::env_flag!("CADRS_SNAP_DEBUG") {
        eprintln!(
            "snap: {} faces, gap {gap:.3e}, whole {}",
            moved.len(),
            moved_whole(b, &moved, gap).is_some()
        );
    }
    if let Some(out) = moved_whole(b, &moved, gap) {
        return Some(out);
    }
    let mut kept: Vec<(usize, Surface)> = Vec::new();
    let mut out = None;
    for m in &moved {
        let mut trial = kept.clone();
        trial.push(m.clone());
        if let Some(o) = moved_whole(b, &trial, gap) {
            kept = trial;
            out = Some(o);
        }
    }
    out
}

/// `b` with the faces of `moved` on their new surfaces, each keeping the side its material was
/// on, where that keeps it a valid solid with no vertex moved further than [`MOST_MOVED`] times
/// `gap`.
fn moved_whole(b: &Body, moved: &[(usize, Surface)], gap: f64) -> Option<Body> {
    let mut surfs: Vec<Surface> = b.faces.iter().map(|f| f.surf.clone()).collect();
    for (f, s) in moved {
        surfs[*f] = s.clone();
    }
    let out = super::local::replace_surfaces_checked(b, &surfs, &[], false);
    if super::env_flag!("CADRS_SNAP_DEBUG")
        && let Err(e) = &out
    {
        eprintln!("snap: replacing the surfaces: {e}");
    }
    let mut out = out.ok()?;
    // (The new surface's natural normal may run the other way: the face keeps its outward one.)
    for (f, s) in moved {
        let p = boundary_points(b, *f).first().copied()?;
        let old = &b.faces[*f].surf;
        let ((ua, va), (ub, vb)) = (old.project(&p, None), s.project(&p, None));
        if old.normal(ua, va).dot(&s.normal(ub, vb)) < 0.0 {
            out.faces[*f].sense = !b.faces[*f].sense;
        }
    }
    let far = b
        .verts
        .iter()
        .zip(&out.verts)
        .map(|(v, w)| (v.p - w.p).norm())
        .fold(0.0, f64::max);
    let issues = out.check_with(super::topo::CheckLevel::Full);
    if super::env_flag!("CADRS_SNAP_DEBUG") {
        eprintln!(
            "snap: vertices moved {far:.3e} (at most {:.3e}), {issues:?}",
            MOST_MOVED * gap
        );
    }
    (far <= MOST_MOVED * gap && issues.is_empty()).then_some(out)
}
