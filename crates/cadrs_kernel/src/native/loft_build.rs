//! Lofts (P3.7, PS20).
//!
//! - **Exact skinning** (no end conditions, sections with the same number of curves): each
//!   section curve becomes a rational B-spline (lines and conics exactly), made compatible by
//!   knot insertion; two sections give the ruled surface between them (planes, cones and
//!   prisms exactly), more are interpolated through the sections' poles. Faces of the loft:
//!   one per section curve, the caps the first and last sections (a face section is its own
//!   cap), a point section an apex.
//! - **Sampled interpolation** (end conditions, or sections that don't match): every section is
//!   sampled with [`crate::loft::sample_sections`], interpolated by a cubic B-spline (all with
//!   the same knots), and the columns of poles by cubics through the sections with the end
//!   derivatives of the conditions (as the OCCT backend's interpolated loft).

use std::sync::Arc;

use super::build::{Built, fix_senses};
use super::curve::Curve;
use super::features::Tagged;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface, dehom, hom};
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Face, Shell, Solid};
use crate::loft::{SectionCurves, sample_sections, section_loop};
use crate::*;

/// One curve of a section in 3D, in the loop's direction, with its source.
#[derive(Debug, Clone)]
struct Part {
    c: Curve,
    t0: f64,
    t1: f64,
    rev: bool,
    src: u64,
}

impl Part {
    fn start(&self) -> P3 {
        self.c.point(if self.rev { self.t1 } else { self.t0 })
    }
    fn end(&self) -> P3 {
        self.c.point(if self.rev { self.t0 } else { self.t1 })
    }
    /// An exact B-spline of the part in its direction of travel, on [0, 1].
    fn nurbs(&self) -> NurbsCurve {
        let n = self.c.to_nurbs(self.t0, self.t1);
        let n = if self.rev { n.reversed() } else { n };
        let n = n.reparam(0.0, 1.0);
        // Lines as quadratics so they match conics.
        if n.degree == 1 && n.cps.len() == 2 {
            let (a, b) = (dehom(&n.cps[0]), dehom(&n.cps[1]));
            return NurbsCurve::bezier(&[a, nalgebra::center(&a, &b), b], &[1.0, 1.0, 1.0]);
        }
        n
    }
}

#[derive(Debug, Clone)]
enum Sec {
    Loop {
        parts: Vec<Part>,
        plane: Option<Plane>,
        face: Option<FaceInput>,
    },
    Point(P3),
}

fn invalid(why: &str) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// A section as 3D parts.
fn section(k: &NativeKernel, s: &LoftSection) -> Result<Sec> {
    match s {
        LoftSection::Point(p) => Ok(Sec::Point(*p)),
        LoftSection::Profile(p) => {
            let curves = section_loop(p)?;
            let m = super::build::plane_map(&p.plane, &V3::zeros(), 0.0);
            let mut parts = Vec::new();
            for (i, c) in curves.iter().enumerate() {
                let src = c.source().unwrap_or(unsourced_curve(0, i));
                let pc = super::profile::piece_of(
                    c,
                    super::profile::Src::Curve {
                        region: 0,
                        curve: src,
                    },
                )?;
                parts.push(Part {
                    c: pc.c.transformed(&m),
                    t0: pc.t0,
                    t1: pc.t1,
                    rev: pc.rev,
                    src,
                });
            }
            Ok(Sec::Loop {
                parts,
                plane: Some(p.plane),
                face: None,
            })
        }
        LoftSection::Face(fi) => {
            let b = k.get(fi.body)?;
            let f = b
                .faces
                .get(fi.face.0 as usize)
                .ok_or_else(|| invalid("unknown face"))?;
            // The outer loop, run so the face's material is on the left seen from outside.
            let lp = &f.loops[0];
            let mut parts = Vec::new();
            for c in lp {
                let e = &b.edges[c.edge];
                if e.degenerate {
                    continue;
                }
                parts.push(Part {
                    c: e.curve.clone(),
                    t0: e.t[0],
                    t1: e.t[1],
                    rev: !c.fwd,
                    src: c.edge as u64,
                });
            }
            let plane = match &f.surf {
                Surface::Plane { f: fr } => {
                    let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
                    Some(Plane {
                        origin: fr.o,
                        x_dir: nalgebra::Unit::new_normalize(fr.x),
                        normal: nalgebra::Unit::new_normalize(n),
                    })
                }
                _ => None,
            };
            Ok(Sec::Loop {
                parts,
                plane,
                face: Some(*fi),
            })
        }
    }
}

fn centroid(s: &Sec) -> P3 {
    match s {
        Sec::Point(p) => *p,
        Sec::Loop { parts, .. } => {
            let mut acc = V3::zeros();
            let mut n = 0.0;
            for p in parts {
                for k in 0..16 {
                    let t = p.t0 + (p.t1 - p.t0) * k as f64 / 16.0;
                    acc += p.c.point(t).coords;
                    n += 1.0;
                }
            }
            P3::from(acc / n)
        }
    }
}

/// Signed area of a loop about an axis (Newell), to orient sections the same way round.
fn turning(parts: &[Part], axis: &V3) -> f64 {
    let mut pts = Vec::new();
    for p in parts {
        for k in 0..32 {
            let s = k as f64 / 32.0;
            let t = if p.rev {
                p.t1 + (p.t0 - p.t1) * s
            } else {
                p.t0 + (p.t1 - p.t0) * s
            };
            pts.push(p.c.point(t));
        }
    }
    let n = pts.len();
    let mut a = V3::zeros();
    for i in 0..n {
        a += pts[i].coords.cross(&pts[(i + 1) % n].coords);
    }
    a.dot(axis)
}

fn reverse_parts(parts: &[Part]) -> Vec<Part> {
    parts
        .iter()
        .rev()
        .map(|p| Part {
            rev: !p.rev,
            ..p.clone()
        })
        .collect()
}

/// Makes curves compatible: the same degree (all quadratic or all cubic) and knots.
/// Curves of one degree given the same knots (by knot insertion); false when the degrees differ.
pub(crate) fn make_compatible(curves: &mut [NurbsCurve]) -> bool {
    compatible(curves)
}

fn compatible(curves: &mut [NurbsCurve]) -> bool {
    let deg = curves.iter().map(|c| c.degree).max().unwrap_or(1);
    if curves.iter().any(|c| c.degree != deg) {
        return false;
    }
    // Union of the interior knots (with multiplicity).
    let mut all: Vec<f64> = Vec::new();
    for c in curves.iter() {
        let mut ks: Vec<f64> = c.knots[deg + 1..c.knots.len() - deg - 1].to_vec();
        // merge with multiplicities
        let mut merged: Vec<f64> = Vec::new();
        let (mut i, mut j) = (0, 0);
        ks.sort_by(f64::total_cmp);
        while i < all.len() || j < ks.len() {
            if j >= ks.len() || (i < all.len() && all[i] < ks[j] - tol::ROUNDOFF_FINE) {
                merged.push(all[i]);
                i += 1;
            } else if i >= all.len() || ks[j] < all[i] - tol::ROUNDOFF_FINE {
                merged.push(ks[j]);
                j += 1;
            } else {
                merged.push(all[i]);
                i += 1;
                j += 1;
            }
        }
        all = merged;
    }
    for c in curves.iter_mut() {
        let mut have: Vec<f64> = c.knots[deg + 1..c.knots.len() - deg - 1].to_vec();
        for &k in &all {
            let count_all = all
                .iter()
                .filter(|x| (*x - k).abs() < tol::ROUNDOFF_FINE)
                .count();
            let count_have = have
                .iter()
                .filter(|x| (*x - k).abs() < tol::ROUNDOFF_FINE)
                .count();
            if count_have < count_all {
                *c = c.insert_knot(k);
                have.push(k);
            }
        }
    }
    true
}

/// The loft's lateral surfaces from per-section compatible curves (homogeneous poles
/// interpolated across the sections at `v`).
pub(crate) fn skin(curves: &[NurbsCurve], v: &[f64]) -> NurbsSurface {
    let ncp = curves[0].cps.len();
    if curves.len() == 2 {
        // Ruled.
        let cps: Vec<Vec<H>> = (0..ncp)
            .map(|i| vec![curves[0].cps[i], curves[1].cps[i]])
            .collect();
        return NurbsSurface {
            du: curves[0].degree,
            dv: 1,
            ku: curves[0].knots.clone(),
            kv: vec![0.0, 0.0, 1.0, 1.0],
            cps,
        };
    }
    // Interpolate each column of homogeneous poles (as points in 4D: x, y, z and w separately):
    // one span through up to nine sections (OCCT's approximation comes out so), else cubic.
    let q = if curves.len() <= 9 {
        curves.len() - 1
    } else {
        3
    };
    let mut cols: Vec<Vec<H>> = Vec::new();
    let mut kv = Vec::new();
    for i in 0..ncp {
        let xyz: Vec<P3> = curves
            .iter()
            .map(|c| P3::new(c.cps[i].x, c.cps[i].y, c.cps[i].z))
            .collect();
        let ww: Vec<P3> = curves
            .iter()
            .map(|c| P3::new(c.cps[i].w, 0.0, 0.0))
            .collect();
        let a = NurbsCurve::interpolate(&xyz, v, q, None, None);
        let b = NurbsCurve::interpolate(&ww, v, q, None, None);
        kv = a.knots.clone();
        cols.push(
            a.cps
                .iter()
                .zip(&b.cps)
                .map(|(p, w)| H::new(p.x, p.y, p.z, w.x))
                .collect(),
        );
    }
    NurbsSurface {
        du: curves[0].degree,
        dv: q,
        ku: curves[0].knots.clone(),
        kv,
        cps: cols,
    }
}

/// A derivative field along a section end (first, and second derivatives with curvature).
/// A vector at each point of a section (a derivative the loft must match there).
type VecField = Box<dyn Fn(&P3) -> V3>;
/// A loft end's first derivative field and, for curvature, its second.
type Field = (VecField, Option<VecField>);
/// Sampled end derivatives (rows of the sample grid): first, and second for curvature.
type EndDerivs = (Vec<Vec<V3>>, Option<Vec<Vec<V3>>>);

/// Per-section geometry for the end conditions.
struct SecGeo {
    centers: Vec<P3>,
    normals: Vec<V3>,
    radii: Vec<f64>,
}

impl SecGeo {
    fn of(secs: &[Sec], axis: &V3) -> SecGeo {
        let centers: Vec<P3> = secs.iter().map(centroid).collect();
        let normals = secs
            .iter()
            .map(|s| match s {
                Sec::Loop {
                    plane: Some(pl), ..
                } => {
                    let n = pl.normal.into_inner();
                    if n.dot(axis) >= 0.0 { n } else { -n }
                }
                _ => axis.normalize(),
            })
            .collect();
        let radii = secs
            .iter()
            .zip(&centers)
            .map(|(s, c)| match s {
                Sec::Point(_) => 0.0,
                Sec::Loop { parts, .. } => {
                    let mut acc = 0.0;
                    let mut n = 0.0;
                    for p in parts {
                        for k in 0..16 {
                            acc += (p.c.point(p.t0 + (p.t1 - p.t0) * k as f64 / 16.0) - c).norm();
                            n += 1.0;
                        }
                    }
                    acc / n
                }
            })
            .collect();
        SecGeo {
            centers,
            normals,
            radii,
        }
    }
}

/// The derivative field of an end condition (the OCCT backend's conventions: magnitude × the
/// length through the section centres; a tangent condition flares from the centre, or towards
/// it when the next section is smaller; at the last section the loft arrives).
#[allow(clippy::too_many_arguments)]
fn field(
    k: &NativeKernel,
    end: &LoftEnd,
    sec: &LoftSection,
    g: &SecGeo,
    i: usize,
    toward: usize,
    total: f64,
    first: bool,
) -> Result<Option<Field>> {
    let n = g.normals[i];
    let len = end.magnitude * total;
    let c = g.centers[i];
    let sign = if g.radii[toward] < g.radii[i] {
        -1.0
    } else {
        1.0
    } * if first { 1.0 } else { -1.0 };
    let radial = move |normal: V3, length: f64| -> VecField {
        let nn = normal.normalize();
        Box::new(move |p: &P3| {
            let d = p - c;
            let r = d - nn * d.dot(&nn);
            if r.norm() < tol::DEGENERATE {
                V3::zeros()
            } else {
                r.normalize() * length
            }
        })
    };
    Ok(Some(match end.condition {
        LoftCondition::Default => return Ok(None),
        LoftCondition::NormalToProfile => (Box::new(move |_: &P3| n * len), None),
        LoftCondition::TangentToProfile => (radial(n, sign * len), None),
        LoftCondition::NormalDirection(d) | LoftCondition::TangentDirection(d) => {
            let d = V3::new(d[0], d[1], d[2]);
            if d.norm() < tol::DEGENERATE || !d.iter().all(|x| x.is_finite()) {
                return Err(invalid("A loft direction needs a non-zero vector"));
            }
            let along = if n.dot(&d).abs() > tol::ROUNDOFF {
                n
            } else {
                let cc = g.centers[toward] - g.centers[i];
                if first { cc } else { -cc }
            };
            let d = if along.dot(&d) < 0.0 {
                -d.normalize()
            } else {
                d.normalize()
            };
            if matches!(end.condition, LoftCondition::NormalDirection(_)) {
                (Box::new(move |_: &P3| d * len), None)
            } else {
                (radial(d, sign * len), None)
            }
        }
        LoftCondition::MatchTangent | LoftCondition::MatchCurvature => {
            let LoftSection::Face(fi) = sec else {
                return Err(invalid(
                    "Match tangent and Match curvature need a face as the profile (its neighbouring faces)",
                ));
            };
            let b = k.get(fi.body)?.clone();
            let face = fi.face.0 as usize;
            let ef = b.edge_faces();
            let mut nb: Vec<(usize, usize)> = Vec::new();
            for co in b.faces[face].loops.iter().flatten() {
                if let Some(&gf) = ef[co.edge].iter().find(|&&x| x != face) {
                    nb.push((co.edge, gf));
                }
            }
            if nb.is_empty() {
                return Err(invalid("The face has no neighbouring faces to match"));
            }
            let b = std::sync::Arc::new(b);
            let at = {
                let b = b.clone();
                let nb = nb.clone();
                move |p: &P3| -> (V3, usize, (f64, f64)) {
                    let mut best: Option<(f64, usize, usize)> = None;
                    for &(e, gf) in &nb {
                        let ed = &b.edges[e];
                        let t = ed.curve.closest(p, ed.t[0], ed.t[1]);
                        let dd = (ed.point(t) - p).norm();
                        if best.is_none_or(|(bd, ..)| dd < bd) {
                            best = Some((dd, e, gf));
                        }
                    }
                    let (_, e, gf) = best.unwrap();
                    let ed = &b.edges[e];
                    let t = ed.curve.closest(p, ed.t[0], ed.t[1]);
                    let tan = ed.curve.tangent(t).normalize();
                    let f = &b.faces[gf];
                    let (u, v) = f.surf.project(p, None);
                    let nrm = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
                    let mut dir = tan.cross(&nrm).normalize();
                    if dir.dot(&n) < 0.0 {
                        dir = -dir;
                    }
                    (dir, gf, (u, v))
                }
            };
            let at = std::sync::Arc::new(at);
            let a1 = at.clone();
            let d1: VecField = Box::new(move |p: &P3| a1(p).0 * len);
            let d2: Option<VecField> = if end.condition == LoftCondition::MatchCurvature {
                let a2 = at.clone();
                let b2 = b.clone();
                Some(Box::new(move |p: &P3| {
                    let (dir, gf, (u, v)) = a2(p);
                    let f = &b2.faces[gf];
                    let s = f.surf.d2(u, v);
                    let (su, sv) = (s[1], s[2]);
                    let (e1, f1, g1) = (su.dot(&su), su.dot(&sv), sv.dot(&sv));
                    let nn = su.cross(&sv).normalize();
                    let (l2, m2, n2) = (s[3].dot(&nn), s[4].dot(&nn), s[5].dot(&nn));
                    let Some((du, dv)) = solve2(e1, f1, f1, g1, dir.dot(&su), dir.dot(&sv)) else {
                        return V3::zeros();
                    };
                    let num = l2 * du * du + 2.0 * m2 * du * dv + n2 * dv * dv;
                    let den = e1 * du * du + 2.0 * f1 * du * dv + g1 * dv * dv;
                    let kappa = if den.abs() > tol::TINY {
                        num / den
                    } else {
                        0.0
                    };
                    nn * (kappa * len * len)
                }))
            } else {
                None
            };
            (d1, d2)
        }
    }))
}

/// Pole vectors D_i with Σ R_i(u) D_i fitting `f` along the curve (least squares over dense
/// samples; exact when `f` is affine in position).
fn fit_poles(c: &NurbsCurve, f: &dyn Fn(&P3) -> V3) -> Vec<V3> {
    let n = c.cps.len();
    let (a, b) = c.range();
    let m = (n * 6).max(24);
    let mut rows = nalgebra::DMatrix::<f64>::zeros(m, n);
    let mut rhs = nalgebra::DMatrix::<f64>::zeros(m, 3);
    for j in 0..m {
        let u = a + (b - a) * (j as f64 + 0.5) / m as f64;
        // Rational basis values: R_i = N_i w_i / Σ N w.
        let span = super::nurbs::find_span(n - 1, c.degree, u, &c.knots);
        let nd = super::nurbs::basis_ders(span, u, c.degree, 0, &c.knots);
        let mut wsum = 0.0;
        for (n, cp) in nd[0].iter().zip(&c.cps[span - c.degree..=span]) {
            wsum += n * cp.w;
        }
        for (k, n) in nd[0].iter().enumerate() {
            let i = span - c.degree + k;
            rows[(j, i)] = n * c.cps[i].w / wsum;
        }
        let v = f(&c.point(u));
        rhs[(j, 0)] = v.x;
        rhs[(j, 1)] = v.y;
        rhs[(j, 2)] = v.z;
    }
    let svd = rows.svd(true, true);
    let sol = svd
        .solve(&rhs, tol::ROUNDOFF_FINE)
        .unwrap_or_else(|_| nalgebra::DMatrix::zeros(n, 3));
    (0..n)
        .map(|i| V3::new(sol[(i, 0)], sol[(i, 1)], sol[(i, 2)]))
        .collect()
}

/// Skinning with end derivatives: homogeneous poles interpolated across the sections, with the
/// derivative w·D on the weighted coordinates and none on the weights (so S_v = D exactly at
/// the end sections).
fn skin_conditioned(
    curves: &[NurbsCurve],
    v: &[f64],
    d1: [Option<Vec<V3>>; 2],
    d2: [Option<Vec<V3>>; 2],
) -> NurbsSurface {
    let ncp = curves[0].cps.len();
    let last = curves.len() - 1;
    let mut cols: Vec<Vec<H>> = Vec::new();
    let mut kv = Vec::new();
    let mut dv = 3;
    for i in 0..ncp {
        let xyz: Vec<P3> = curves
            .iter()
            .map(|c| P3::new(c.cps[i].x, c.cps[i].y, c.cps[i].z))
            .collect();
        let ww: Vec<P3> = curves
            .iter()
            .map(|c| P3::new(c.cps[i].w, 0.0, 0.0))
            .collect();
        let (w0, w1) = (curves[0].cps[i].w, curves[last].cps[i].w);
        let dx = [
            d1[0].as_ref().map(|d| d[i] * w0),
            d1[1].as_ref().map(|d| d[i] * w1),
        ];
        let ddx = [
            d2[0].as_ref().map(|d| d[i] * w0),
            d2[1].as_ref().map(|d| d[i] * w1),
        ];
        let dw = [
            d1[0].as_ref().map(|_| V3::zeros()),
            d1[1].as_ref().map(|_| V3::zeros()),
        ];
        let ddw = [
            d2[0].as_ref().map(|_| V3::zeros()),
            d2[1].as_ref().map(|_| V3::zeros()),
        ];
        let deg = if ddx[0].is_some() || ddx[1].is_some() {
            5
        } else {
            3
        };
        let a = NurbsCurve::interpolate_ext(&xyz, v, deg, dx, ddx);
        let b = NurbsCurve::interpolate_ext(&ww, v, deg, dw, ddw);
        kv = a.knots.clone();
        dv = a.degree;
        cols.push(
            a.cps
                .iter()
                .zip(&b.cps)
                .map(|(p, w)| H::new(p.x, p.y, p.z, w.x))
                .collect(),
        );
    }
    NurbsSurface {
        du: curves[0].degree,
        dv,
        ku: curves[0].knots.clone(),
        kv,
        cps: cols,
    }
}

/// The iso-curve u = const or v = const of a surface whose knots are clamped, as an exact
/// B-spline (a row or a column of poles).
pub(crate) fn iso(s: &NurbsSurface, along_u: bool, at_end: bool) -> NurbsCurve {
    if along_u {
        let j = if at_end { s.cps[0].len() - 1 } else { 0 };
        NurbsCurve {
            degree: s.du,
            knots: s.ku.clone(),
            cps: s.cps.iter().map(|row| row[j]).collect(),
        }
    } else {
        let i = if at_end { s.cps.len() - 1 } else { 0 };
        NurbsCurve {
            degree: s.dv,
            knots: s.kv.clone(),
            cps: s.cps[i].clone(),
        }
    }
}

fn line2(o: P2, d: V2) -> Curve {
    Curve::Line {
        o: P3::new(o.x, o.y, 0.0),
        d: V3::new(d.x, d.y, 0.0),
    }
}

/// Builds the solid (or sheet) from lateral surfaces, one per part (u ∈ [0, 1], v ∈ [0, 1],
/// v = 0 the first section). `caps`: the first and last sections' cap surfaces (None for a
/// point or a sheet). `point_ends`: sections that are points.
fn assemble(
    surfs: Vec<NurbsSurface>,
    closed_u: bool,
    srcs: &[u64],
    caps: [Option<(Surface, Option<FaceInput>)>; 2],
    points: [bool; 2],
    solid: bool,
    source: u64,
) -> Result<Built> {
    assemble_rows(
        vec![surfs],
        closed_u,
        false,
        srcs,
        caps,
        points,
        solid,
        source,
    )
}

/// The loft's body from rows of surfaces (bottom to top; each row one surface per section
/// part, all rows sharing their u knots), the caps and the point ends.
#[allow(clippy::too_many_arguments)]
fn assemble_rows(
    rows: Vec<Vec<NurbsSurface>>,
    closed_u: bool,
    closed_v: bool,
    srcs: &[u64],
    caps: [Option<(Surface, Option<FaceInput>)>; 2],
    points: [bool; 2],
    solid: bool,
    source: u64,
) -> Result<Built> {
    // Degenerate sections (from a caller's NaN or coincident data) make surfaces that can't be
    // evaluated: an error, not a panic further on.
    if rows.is_empty() || rows[0].is_empty() {
        return Err(KernelError::InvalidGeometry {
            op: "loft",
            why: "the loft has no surfaces".into(),
        });
    }
    for s in rows.iter().flatten() {
        s.validate().map_err(|e| KernelError::InvalidResult {
            op: "loft",
            why: format!("the loft's surface isn't valid ({})", e.message()),
        })?;
    }
    let n = rows[0].len();
    let nr = rows.len();
    let mut b = Body::default();
    // Corner vertices at each level (0 bottom … nr top) and joint (u = 0 of part i).
    let nj = if closed_u { n } else { n + 1 };
    let corner = |r: usize, j: usize| -> P3 {
        let (row, top) = if r < nr {
            (&rows[r], false)
        } else {
            (&rows[nr - 1], true)
        };
        let (s, at_end) = if j < n {
            (&row[j], false)
        } else {
            (&row[n - 1], true)
        };
        let c = iso(s, false, at_end);
        let (a, bb) = c.range();
        c.point(if top { bb } else { a })
    };
    let mut vs: Vec<Vec<usize>> = Vec::new();
    for r in 0..=nr {
        // A loft back to its first section ends on the first level.
        if r == nr && closed_v {
            vs.push(vs[0].clone());
            continue;
        }
        let point = (r == 0 && points[0]) || (r == nr && points[1]);
        let mut level: Vec<usize> = Vec::new();
        for j in 0..nj {
            let p = corner(r, j);
            level.push(if point && j > 0 {
                level[0]
            } else {
                b.add_vertex(p)
            });
        }
        vs.push(level);
    }
    // Rails (v-curves) at the joints, per row.
    let rails: Vec<Vec<usize>> = (0..nr)
        .map(|r| {
            (0..nj)
                .map(|j| {
                    let (s, at_end) = if j < n {
                        (&rows[r][j], false)
                    } else {
                        (&rows[r][n - 1], true)
                    };
                    let c = iso(s, false, at_end);
                    let (a, bb) = c.range();
                    b.add_edge(Curve::Nurbs(Arc::new(c)), a, bb, vs[r][j], vs[r + 1][j])
                })
                .collect()
        })
        .collect();
    // A flat patch running into a point (a ruled triangle): a plane face of three edges, no
    // degenerate edge (as OCCT's ruled loft).
    let triangle = |r: usize, i: usize| -> Option<Surface> {
        let at_point = (r == 0 && points[0]) || (r + 1 == nr && points[1]);
        if !at_point {
            return None;
        }
        let pts: Vec<P3> = rows[r][i].cps.iter().flatten().map(dehom).collect();
        let o = pts[0];
        let far = pts
            .iter()
            .max_by(|a, b| (*a - o).norm().total_cmp(&(*b - o).norm()))?;
        let x = (far - o).try_normalize(tol::DEGENERATE)?;
        let nrm = pts
            .iter()
            .map(|p| x.cross(&(p - o)))
            .max_by(|a, b| a.norm().total_cmp(&b.norm()))?
            .try_normalize(tol::DEGENERATE)?;
        let size = (far - o).norm();
        if pts
            .iter()
            .any(|p| (p - o).dot(&nrm).abs() > tol::REL * size.max(1.0))
        {
            return None;
        }
        Some(Surface::Plane {
            f: Frame {
                o,
                x,
                y: nrm.cross(&x),
                z: nrm,
            },
        })
    };
    // Section edges at each level (none at a point under triangles).
    let mut levels: Vec<Vec<usize>> = Vec::new();
    for r in 0..=nr {
        if r == nr && closed_v {
            levels.push(levels[0].clone());
            continue;
        }
        let point = (r == 0 && points[0]) || (r == nr && points[1]);
        let mut es = Vec::new();
        for i in 0..n {
            let j = if closed_u { (i + 1) % n } else { i + 1 };
            let c = if r < nr {
                iso(&rows[r][i], true, false)
            } else {
                iso(&rows[nr - 1][i], true, true)
            };
            let (ua, ub) = c.range();
            let flat = point && triangle(if r == 0 { 0 } else { nr - 1 }, i).is_some();
            es.push(if flat {
                usize::MAX
            } else if point {
                b.add_degenerate(b.verts[vs[r][0]].p, ua, ub, vs[r][0])
            } else {
                b.add_edge(Curve::Nurbs(Arc::new(c)), ua, ub, vs[r][i], vs[r][j])
            });
        }
        levels.push(es);
    }
    let mut faces_out: Vec<(usize, u64)> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (i, s) in row.iter().enumerate() {
            let j = if closed_u { (i + 1) % n } else { i + 1 };
            if let Some(surf) = triangle(r, i) {
                let mut lp = Vec::new();
                for (e, fwd) in [
                    (levels[r][i], true),
                    (rails[r][j], true),
                    (levels[r + 1][i], false),
                    (rails[r][i], false),
                ] {
                    if e == usize::MAX {
                        continue;
                    }
                    let ed = &b.edges[e];
                    lp.push(Coedge {
                        edge: e,
                        fwd,
                        pc: super::pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None),
                    });
                }
                b.faces.push(Face {
                    surf,
                    sense: true,
                    loops: vec![lp],
                });
                faces_out.push((
                    b.faces.len() - 1,
                    srcs.get(i).copied().unwrap_or(unsourced_curve(0, i)),
                ));
                continue;
            }
            let ((u0, u1), (v0, v1)) = s.range();
            let lp = vec![
                Coedge {
                    edge: levels[r][i],
                    fwd: true,
                    pc: line2(P2::new(0.0, v0), V2::new(1.0, 0.0)),
                },
                Coedge {
                    edge: rails[r][j],
                    fwd: true,
                    pc: line2(P2::new(u1, 0.0), V2::new(0.0, 1.0)),
                },
                Coedge {
                    edge: levels[r + 1][i],
                    fwd: false,
                    pc: line2(P2::new(0.0, v1), V2::new(1.0, 0.0)),
                },
                Coedge {
                    edge: rails[r][i],
                    fwd: false,
                    pc: line2(P2::new(u0, 0.0), V2::new(0.0, 1.0)),
                },
            ];
            b.faces.push(Face {
                surf: Surface::Nurbs(Arc::new(s.clone())),
                sense: true,
                loops: vec![lp],
            });
            faces_out.push((
                b.faces.len() - 1,
                srcs.get(i).copied().unwrap_or(unsourced_curve(0, i)),
            ));
        }
    }
    let bottom = levels[0].clone();
    let top = levels[nr].clone();
    let n = b.faces.len();
    fix_senses(&mut b, 0..n);
    let mut origins: Vec<(usize, Origin)> = faces_out
        .iter()
        .map(|&(f, c)| {
            (
                f,
                Origin::ProfileCurve {
                    region: source,
                    curve: c,
                },
            )
        })
        .collect();
    if solid {
        for (end, cap) in caps.iter().enumerate() {
            let Some((surf, _)) = cap else { continue };
            let edges = if end == 0 { &bottom } else { &top };
            // The sides use the bottom edges forward and the top edges backward; the caps use
            // them the other way: the bottom in reverse order, backward; the top in order, forward.
            let order: Vec<usize> = if end == 0 {
                edges.iter().rev().copied().collect()
            } else {
                edges.clone()
            };
            let mut lp: Vec<Coedge> = order
                .iter()
                .map(|&e| {
                    let ed = &b.edges[e];
                    let pc = super::pcurve::pcurve(surf, &ed.curve, ed.t[0], ed.t[1], None);
                    Coedge {
                        edge: e,
                        fwd: end == 1,
                        pc,
                    }
                })
                .collect();
            // Make the p-curves continuous on periodic caps.
            let mut at: Option<P2> = None;
            for c in lp.iter_mut() {
                let ed = &b.edges[c.edge];
                let (t0, t1) = super::topo::co_range(ed, c.fwd);
                if let Some(a) = at {
                    let q = c.pc.point(t0);
                    let du = if surf.u_periodic() {
                        ((a.x - q.x) / TAU).round() * TAU
                    } else {
                        0.0
                    };
                    let dv = if surf.v_periodic() {
                        ((a.y - q.y) / TAU).round() * TAU
                    } else {
                        0.0
                    };
                    if du != 0.0 || dv != 0.0 {
                        c.pc = c.pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                    }
                }
                let q = c.pc.point(t1);
                at = Some(P2::new(q.x, q.y));
            }
            b.faces.push(Face {
                surf: surf.clone(),
                sense: true,
                loops: vec![lp],
            });
            let fi = b.faces.len() - 1;
            fix_senses(&mut b, fi..fi + 1);
            origins.push((
                fi,
                if end == 0 {
                    Origin::StartCap { region: source }
                } else {
                    Origin::EndCap { region: source }
                },
            ));
        }
    }
    let nf = b.faces.len();
    b.shells.push(Shell {
        faces: (0..nf).collect(),
        closed: solid,
    });
    if solid {
        b.solids.push(Solid { shells: vec![0] });
        // Outward: flip everything if the volume came out negative.
        if super::mass::body_volume(&b) < 0.0 {
            for f in &mut b.faces {
                Body::reverse_face(f);
            }
        }
        // Each face's sense must agree with the solid: re-fix the lateral faces' orientation
        // against the volume sign is enough for a consistent shell.
    }
    Ok(Built { body: b, origins })
}

/// The cap surface of a section: its plane, or (a face section) the face's own surface.
fn cap_surface(k: &NativeKernel, s: &Sec) -> Option<(Surface, Option<FaceInput>)> {
    match s {
        Sec::Point(_) => None,
        Sec::Loop {
            plane: Some(pl),
            face,
            ..
        } => Some((
            Surface::Plane {
                f: super::build::plane_frame(pl, V3::zeros()),
            },
            *face,
        )),
        Sec::Loop {
            plane: None,
            face: Some(fi),
            ..
        } => {
            let b = k.get(fi.body).ok()?;
            Some((b.faces[fi.face.0 as usize].surf.clone(), Some(*fi)))
        }
        _ => None,
    }
}

/// Exact skinning; None when the sections don't match.
fn loft_exact(
    k: &NativeKernel,
    spec: &LoftSpec,
    secs: &mut [Sec],
    solid: bool,
) -> Result<Option<Built>> {
    loft_exact_with(k, spec, secs, solid, true, false)
}

/// A solid loft through sections given as loops of profile pieces in their planes, whose
/// pieces already correspond in order and direction (the same shape at stations along an
/// edge): no alignment, no re-chaining. A section with no pieces is a point, its plane's
/// origin (an apex: at the first or last section only).
pub fn loft_pieces(
    k: &NativeKernel,
    sections: &[(Vec<super::profile::Piece>, Plane)],
    source: u64,
) -> Result<Built> {
    let mut secs: Vec<Sec> = Vec::new();
    for (pieces, plane) in sections {
        if pieces.is_empty() {
            secs.push(Sec::Point(plane.origin));
            continue;
        }
        let m = super::build::plane_map(plane, &V3::zeros(), 0.0);
        let parts = pieces
            .iter()
            .enumerate()
            .map(|(i, pc)| Part {
                c: pc.c.transformed(&m),
                t0: pc.t0,
                t1: pc.t1,
                rev: pc.rev,
                src: unsourced_curve(0, i),
            })
            .collect();
        secs.push(Sec::Loop {
            parts,
            plane: Some(*plane),
            face: None,
        });
    }
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source,
    };
    loft_exact_with(k, &spec, &mut secs, true, false, false)?
        .ok_or_else(|| invalid("The sections don't match"))
}

/// A section of [`loft_wires`]: a wire (edges as curves between parameters, chained in order) or
/// a point.
#[derive(Debug, Clone)]
pub enum WireSection {
    Wire(Vec<(Curve, f64, f64)>),
    Point(P3),
}

/// OCCT's `thrusections`: a loft through wires, closed (a solid when `solid` and the sections
/// are planar, else a shell) or open (a sheet). `ruled` joins neighbouring sections by ruled
/// surfaces instead of one smooth surface through all of them.
pub fn loft_wires(
    k: &NativeKernel,
    sections: &[WireSection],
    solid: bool,
    ruled: bool,
) -> Result<Built> {
    let mut secs: Vec<Sec> = Vec::new();
    for s in sections {
        match s {
            WireSection::Point(p) => secs.push(Sec::Point(*p)),
            WireSection::Wire(w) => {
                let parts: Vec<Part> = w
                    .iter()
                    .enumerate()
                    .map(|(i, (c, t0, t1))| Part {
                        c: c.clone(),
                        t0: t0.min(*t1),
                        t1: t0.max(*t1),
                        rev: t1 < t0,
                        src: unsourced_curve(0, i),
                    })
                    .collect();
                let plane = wire_plane(&parts);
                secs.push(Sec::Loop {
                    parts,
                    plane,
                    face: None,
                });
            }
        }
    }
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 0,
    };
    let closed = secs.iter().all(|s| match s {
        Sec::Loop { parts, .. } => {
            (parts[0].start() - parts[parts.len() - 1].end()).norm() < tol::POINT
        }
        Sec::Point(_) => true,
    });
    let solid = solid
        && closed
        && secs
            .iter()
            .all(|s| matches!(s, Sec::Point(_) | Sec::Loop { plane: Some(_), .. }));
    let counts: Vec<usize> = secs
        .iter()
        .filter_map(|s| {
            if let Sec::Loop { parts, .. } = s {
                Some(parts.len())
            } else {
                None
            }
        })
        .collect();
    // OCCT re-matches the sections by rays when their edge counts differ or every one is
    // tangent-continuous all round (C1: the same derivative either side of each vertex).
    let smooth = |parts: &[Part]| {
        (0..parts.len()).all(|i| {
            let (a, b) = (&parts[i], &parts[(i + 1) % parts.len()]);
            let da = a.c.tangent(if a.rev { a.t0 } else { a.t1 }) * if a.rev { -1.0 } else { 1.0 };
            let db = b.c.tangent(if b.rev { b.t1 } else { b.t0 }) * if b.rev { -1.0 } else { 1.0 };
            (a.end() - b.start()).norm() < 2e-7 && (da - db).norm() <= 2e-7 * (1.0 + da.norm())
        })
    };
    let all_smooth = secs.iter().all(|s| match s {
        Sec::Loop { parts, .. } => smooth(parts),
        Sec::Point(_) => true,
    });
    let differ = counts.iter().any(|n| *n != counts[0]);
    if closed && (differ || all_smooth) {
        // (Smooth sections that aren't planar keep their own edges.)
        match polar_match(&mut secs) {
            Ok(()) => {
                return loft_exact_with(k, &spec, &mut secs, solid, false, ruled)?
                    .ok_or_else(|| invalid("The sections don't match"));
            }
            Err(e) if differ => return Err(e),
            Err(_) => {}
        }
    }
    loft_exact_with(k, &spec, &mut secs, solid, true, ruled)?.ok_or(KernelError::Unsupported(
        "thrusections of wires with different numbers of edges",
    ))
}

/// Gives closed planar sections the same number of edges, as OCCT's polar method: each
/// section's vertices are carried to its neighbours (moved with the centres, turned with the
/// planes) and split them where the ray from the neighbour's centre meets it (or reuse a vertex
/// within 1% of an edge's end); back from the last section, then forward from the first. Each
/// loop then starts at the vertex the first loop's start leads to, all the same way round.
fn polar_match(secs: &mut [Sec]) -> Result<()> {
    let idx: Vec<usize> = (0..secs.len())
        .filter(|&i| matches!(secs[i], Sec::Loop { .. }))
        .collect();
    let loops = |secs: &[Sec], i: usize| -> Vec<Part> {
        if let Sec::Loop { parts, .. } = &secs[i] {
            parts.clone()
        } else {
            Vec::new()
        }
    };
    // Centres (the length-weighted centroid, a lone circle's centre) and axes.
    let mut pos = Vec::new();
    let mut axes = Vec::new();
    for &i in &idx {
        let parts = loops(secs, i);
        let Sec::Loop {
            plane: Some(pl), ..
        } = &secs[i]
        else {
            return Err(KernelError::Unsupported(
                "thrusections of sections with different numbers of edges that aren't planar",
            ));
        };
        let c = match &parts[..] {
            [
                Part {
                    c: Curve::Circle { c, .. } | Curve::Ellipse { c, .. },
                    ..
                },
            ] => *c,
            _ => {
                let (mut acc, mut len) = (V3::zeros(), 0.0);
                for p in &parts {
                    for k in 0..32 {
                        let (a, b) = (
                            p.t0 + (p.t1 - p.t0) * k as f64 / 32.0,
                            p.t0 + (p.t1 - p.t0) * (k + 1) as f64 / 32.0,
                        );
                        let l = p.c.length(a, b);
                        acc += p.c.point(0.5 * (a + b)).coords * l;
                        len += l;
                    }
                }
                P3::from(acc / len)
            }
        };
        pos.push(c);
        axes.push(pl.normal.into_inner());
    }
    let n = idx.len();
    // The loft's direction orients the axes and the loops.
    let along = pos[n - 1] - pos[0];
    let along = if along.norm() > tol::ROUNDOFF {
        along
    } else {
        axes[0]
    };
    for a in axes.iter_mut() {
        if a.dot(&along) < 0.0 {
            *a = -*a;
        }
    }
    for (k, &i) in idx.iter().enumerate() {
        if let Sec::Loop { parts, .. } = &mut secs[i]
            && turning(parts, &axes[k]) < 0.0
        {
            *parts = reverse_parts(parts);
        }
    }
    let carry = |p: &P3, from: usize, to: usize| -> P3 {
        let q = p + (pos[to] - pos[from]);
        let r = nalgebra::Rotation3::rotation_between(&axes[from], &axes[to])
            .unwrap_or_else(nalgebra::Rotation3::identity);
        pos[to] + r * (q - pos[to])
    };
    // Where the ray from section k's centre towards `target` meets its loop: (part, parameter).
    let hit = |parts: &[Part], k: usize, target: &P3| -> Option<(usize, f64)> {
        let d = target - pos[k];
        let ax = axes[k];
        let side = |p: &P3| (p - pos[k]).cross(&d).dot(&ax);
        let mut best: Option<(f64, usize, f64)> = None;
        for (pi, p) in parts.iter().enumerate() {
            const N: usize = 64;
            let t_at = |j: usize| p.t0 + (p.t1 - p.t0) * j as f64 / N as f64;
            for j in 0..N {
                let (mut a, mut b) = (t_at(j), t_at(j + 1));
                let (mut fa, fb) = (side(&p.c.point(a)), side(&p.c.point(b)));
                if fa * fb > 0.0 {
                    continue;
                }
                for _ in 0..60 {
                    let m = 0.5 * (a + b);
                    let fm = side(&p.c.point(m));
                    if fa * fm <= 0.0 {
                        b = m;
                    } else {
                        a = m;
                        fa = fm;
                    }
                }
                let t = 0.5 * (a + b);
                let q = p.c.point(t);
                if (q - pos[k]).dot(&d) <= 0.0 {
                    continue;
                }
                let dist = (q - target).norm();
                if best.is_none_or(|(bd, ..)| dist < bd) {
                    best = Some((dist, pi, t));
                }
            }
        }
        best.map(|(_, pi, t)| (pi, t))
    };
    // Splits loop k at the ray towards `target` (unless a vertex is within 1%).
    let split = |secs: &mut [Sec], k: usize, target: &P3| {
        let Sec::Loop { parts, .. } = &mut secs[idx[k]] else {
            return;
        };
        let Some((pi, t)) = hit(parts, k, target) else {
            return;
        };
        let p = parts[pi].clone();
        let tol = 0.01 * (p.t1 - p.t0).abs();
        if (t - p.t0).abs() < tol || (p.t1 - t).abs() < tol {
            return;
        }
        let (a, b) = (Part { t1: t, ..p.clone() }, Part { t0: t, ..p.clone() });
        let pieces = if p.rev { [b, a] } else { [a, b] };
        parts.splice(pi..pi + 1, pieces);
    };
    for k in (1..n).rev() {
        for v in loops(secs, idx[k]).iter().map(|p| p.start()) {
            let target = carry(&v, k, k - 1);
            split(secs, k - 1, &target);
        }
    }
    for (k, &ik) in idx.iter().enumerate().take(n.saturating_sub(1)) {
        for v in loops(secs, ik).iter().map(|p| p.start()) {
            let target = carry(&v, k, k + 1);
            split(secs, k + 1, &target);
        }
    }
    // Start each loop where the first one's start leads.
    let mut start = loops(secs, idx[0])[0].start();
    for k in 1..n {
        start = carry(&start, k - 1, k);
        let parts = loops(secs, idx[k]);
        let best = (0..parts.len()).min_by(|&a, &b| {
            let ang = |r: usize| {
                let u = (parts[r].start() - pos[k]).normalize();
                -u.dot(&(start - pos[k]).normalize())
            };
            ang(a).total_cmp(&ang(b))
        });
        if let (Some(best), Sec::Loop { parts, .. }) = (best, &mut secs[idx[k]]) {
            parts.rotate_left(best);
            start = parts[0].start();
        }
    }
    let counts: Vec<usize> = idx.iter().map(|&i| loops(secs, i).len()).collect();
    if counts.iter().any(|c| *c != counts[0]) {
        return Err(invalid("The sections can't be matched edge for edge"));
    }
    Ok(())
}

/// The plane of a planar loop of parts (Newell's normal), if it is planar.
fn wire_plane(parts: &[Part]) -> Option<Plane> {
    let mut pts = Vec::new();
    for p in parts {
        for k in 0..32 {
            pts.push(p.c.point(p.t0 + (p.t1 - p.t0) * k as f64 / 32.0));
        }
    }
    let n = pts.len();
    let mut nrm = V3::zeros();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        nrm += a.coords.cross(&b.coords);
    }
    let nrm = nrm.try_normalize(tol::DEGENERATE)?;
    let o = pts[0];
    let size = pts.iter().map(|p| (p - o).norm()).fold(0.0, f64::max);
    if pts
        .iter()
        .any(|p| (p - o).dot(&nrm).abs() > 1e-7 * size.max(1.0))
    {
        return None;
    }
    let x = (pts[n / 3] - o) - nrm * (pts[n / 3] - o).dot(&nrm);
    Some(Plane {
        origin: o,
        x_dir: nalgebra::Unit::try_new(x, tol::DEGENERATE)?,
        normal: nalgebra::Unit::new_normalize(nrm),
    })
}

fn loft_exact_with(
    k: &NativeKernel,
    spec: &LoftSpec,
    secs: &mut [Sec],
    solid: bool,
    align: bool,
    ruled: bool,
) -> Result<Option<Built>> {
    let loops: Vec<usize> = secs
        .iter()
        .filter_map(|s| {
            if let Sec::Loop { parts, .. } = s {
                Some(parts.len())
            } else {
                None
            }
        })
        .collect();
    if loops.is_empty() || loops.iter().any(|n| *n != loops[0]) {
        return Ok(None);
    }
    let count = loops[0];
    let first = centroid(&secs[0]);
    let last = centroid(&secs[secs.len() - 1]);
    let mut axis = last - first;
    // A loft that comes back to its first section: along the way to the second.
    if axis.norm() < tol::ROUNDOFF && secs.len() > 2 {
        axis = centroid(&secs[1]) - first;
    }
    if axis.norm() < tol::ROUNDOFF {
        return Err(invalid("The loft's profiles lie on top of each other"));
    }
    // Open sections (sheets): each the way round that starts nearest the previous one's start.
    let closed_u = secs.iter().all(|s| match s {
        Sec::Loop { parts, .. } => {
            (parts[0].start() - parts[parts.len() - 1].end()).norm() < tol::POINT
        }
        Sec::Point(_) => true,
    });
    if !closed_u {
        if secs.iter().any(|s| matches!(s, Sec::Point(_))) {
            return Ok(None);
        }
        for i in 1..secs.len() {
            let Sec::Loop { parts: prev, .. } = &secs[i - 1] else {
                continue;
            };
            let (a, b) = (prev[0].start(), prev[prev.len() - 1].end());
            let Sec::Loop { parts, .. } = &mut secs[i] else {
                continue;
            };
            let (c, d) = (parts[0].start(), parts[parts.len() - 1].end());
            if (c - b).norm() + (d - a).norm() < (c - a).norm() + (d - b).norm() {
                *parts = reverse_parts(parts);
            }
        }
    }
    // The same way round the axis, each started where it lines up with the previous one.
    for s in secs.iter_mut().filter(|_| align && closed_u) {
        if let Sec::Loop { parts, .. } = s
            && turning(parts, &axis) < 0.0
        {
            *parts = reverse_parts(parts);
        }
    }
    let mut prev_dir: Option<V3> = None;
    // The previous section's part starts, as directions from its centre across the axis.
    let mut prev_starts: Vec<V3> = Vec::new();
    for i in (0..secs.len()).filter(|_| align && closed_u) {
        let c = centroid(&secs[i]);
        if let Sec::Loop { parts, .. } = &mut secs[i] {
            let perp = |d: V3| d - axis.normalize() * d.dot(&axis.normalize());
            let dirs = |parts: &[Part]| -> Vec<V3> {
                parts
                    .iter()
                    .map(|p| {
                        perp(p.start() - c)
                            .try_normalize(tol::DEGENERATE)
                            .unwrap_or_else(V3::zeros)
                    })
                    .collect()
            };
            if count == 1
                && parts[0].c.is_periodic()
                && (parts[0].t1 - parts[0].t0) >= TAU - tol::ANGULAR
            {
                // A whole circle or ellipse: restart it where it lines up.
                if let Some(pd) = prev_dir {
                    let p = &mut parts[0];
                    let target = c + pd;
                    let t = p.c.closest(&target, p.t0 - PI, p.t0 + TAU + PI);
                    let span = p.t1 - p.t0;
                    p.t0 = t;
                    p.t1 = t + span;
                }
            } else if prev_dir.is_some() {
                // The turn that lines every part start up with the previous section's, not just
                // the first: with many close starts (a spline's spans along a wide profile) the
                // first alone is a degree or two from its neighbours and picks one off by one.
                let here = dirs(parts);
                let best = (0..count)
                    .min_by(|&a, &b| {
                        let cost = |r: usize| {
                            (0..count)
                                .map(|j| (here[(j + r) % count] - prev_starts[j]).norm_squared())
                                .sum::<f64>()
                        };
                        cost(a).total_cmp(&cost(b))
                    })
                    .unwrap_or(0);
                parts.rotate_left(best);
            }
            prev_dir = Some(perp(parts[0].start() - c));
            prev_starts = dirs(parts);
        }
    }
    // Compatible curves per part across the sections.
    let n_secs = secs.len();
    // Back to the first section at the end: closed across the sections (no caps).
    let closed_v = n_secs > 2
        && match (&secs[0], &secs[n_secs - 1]) {
            (Sec::Loop { parts: a, .. }, Sec::Loop { parts: b, .. }) => {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(p, q)| {
                        (p.start() - q.start()).norm() < 1e-7 && (p.end() - q.end()).norm() < 1e-7
                    })
            }
            _ => false,
        };
    let solid = solid && !closed_v;
    let mut srcs = Vec::new();
    let mut surfs = Vec::new();
    let mut ruled_cols: Vec<Vec<NurbsSurface>> = Vec::new();
    // v parameters by the distance between centres.
    let cents: Vec<P3> = secs.iter().map(centroid).collect();
    let mut v = vec![0.0];
    for i in 1..n_secs {
        v.push(v[i - 1] + (cents[i] - cents[i - 1]).norm());
    }
    let total = v.last().copied().unwrap_or(0.0);
    if !(total.is_finite() && total > 0.0) {
        return Err(KernelError::InvalidGeometry {
            op: "loft",
            why: "the loft's sections lie on top of each other".into(),
        });
    }
    let v: Vec<f64> = v.iter().map(|x| x / total).collect();
    for pi in 0..count {
        let mut curves: Vec<NurbsCurve> = Vec::new();
        let mut template: Option<NurbsCurve> = None;
        for s in secs.iter() {
            if let Sec::Loop { parts, .. } = s {
                let c = parts[pi].nurbs();
                template.get_or_insert(c.clone());
                curves.push(c);
            } else {
                curves.push(NurbsCurve {
                    degree: 0,
                    knots: vec![],
                    cps: vec![],
                });
            }
        }
        // The same degree all through (lines and conics are quadratics already).
        let top = curves.iter().map(|c| c.degree).max().unwrap_or(1);
        for c in curves.iter_mut().filter(|c| !c.cps.is_empty()) {
            while c.degree < top {
                *c = c.elevated();
            }
        }
        let template = template.map(|mut t| {
            while t.degree < top {
                t = t.elevated();
            }
            t
        });
        // Points: the template's shape collapsed onto the point.
        let Some(tpl) = template else { return Ok(None) };
        for (si, s) in secs.iter().enumerate() {
            if let Sec::Point(p) = s {
                curves[si] = NurbsCurve {
                    degree: tpl.degree,
                    knots: tpl.knots.clone(),
                    cps: tpl.cps.iter().map(|h| hom(p, h.w)).collect(),
                };
            }
        }
        if !compatible(&mut curves) {
            return Ok(None);
        }
        // Points must follow the knot insertions too.
        for (si, s) in secs.iter().enumerate() {
            if let Sec::Point(p) = s {
                let Some(reff) = curves
                    .iter()
                    .enumerate()
                    .find(|(j, _)| matches!(secs[*j], Sec::Loop { .. }))
                    .map(|(_, c)| c.clone())
                else {
                    return Ok(None);
                };
                curves[si] = NurbsCurve {
                    degree: reff.degree,
                    knots: reff.knots.clone(),
                    cps: reff.cps.iter().map(|h| hom(p, h.w)).collect(),
                };
            }
        }
        let Some(src) = secs.iter().find_map(|s| {
            if let Sec::Loop { parts, .. } = s {
                Some(parts[pi].src)
            } else {
                None
            }
        }) else {
            return Ok(None);
        };
        srcs.push(src);
        let conditioned = spec.start.condition != LoftCondition::Default
            || spec.end.condition != LoftCondition::Default;
        if conditioned {
            let geo = SecGeo::of(secs, &axis);
            let d0 = field(k, &spec.start, &spec.sections[0], &geo, 0, 1, total, true)?;
            let d1 = field(
                k,
                &spec.end,
                &spec.sections[n_secs - 1],
                &geo,
                n_secs - 1,
                n_secs - 2,
                total,
                false,
            )?;
            let fit = |f: &Option<Field>, c: &NurbsCurve, second: bool| -> Option<Vec<V3>> {
                let f = f.as_ref()?;
                let g: &dyn Fn(&P3) -> V3 = if second { f.1.as_deref()? } else { &*f.0 };
                Some(fit_poles(c, g))
            };
            let (a0, a1) = (
                fit(&d0, &curves[0], false),
                fit(&d1, &curves[n_secs - 1], false),
            );
            let (b0, b1) = (
                fit(&d0, &curves[0], true),
                fit(&d1, &curves[n_secs - 1], true),
            );
            surfs.push(skin_conditioned(&curves, &v, [a0, a1], [b0, b1]));
        } else if ruled {
            // One ruled surface per neighbouring pair (a face each, as OCCT).
            ruled_cols.push(
                curves
                    .windows(2)
                    .map(|w| skin(w, &[0.0, 1.0]))
                    .collect::<Vec<_>>(),
            );
        } else {
            surfs.push(skin(&curves, &v));
        }
    }
    let caps = [cap_surface(k, &secs[0]), cap_surface(k, &secs[n_secs - 1])];
    let points = [
        matches!(secs[0], Sec::Point(_)),
        matches!(secs[n_secs - 1], Sec::Point(_)),
    ];
    if ruled {
        let rows: Vec<Vec<NurbsSurface>> = (0..n_secs - 1)
            .map(|r| ruled_cols.iter().map(|c| c[r].clone()).collect())
            .collect();
        return Ok(Some(assemble_rows(
            rows,
            closed_u,
            closed_v,
            &srcs,
            caps,
            points,
            solid,
            spec.source,
        )?));
    }
    Ok(Some(assemble_rows(
        vec![surfs],
        closed_u,
        closed_v,
        &srcs,
        caps,
        points,
        solid,
        spec.source,
    )?))
}

/// The loft's end derivative at a section (OCCT backend conventions: magnitude × the length
/// through the section centres).
#[allow(clippy::too_many_arguments)]
fn end_derivative(
    k: &NativeKernel,
    end: &LoftEnd,
    sec: &LoftSection,
    samples: &crate::loft::Samples,
    i: usize,
    toward: usize,
    total: f64,
    first: bool,
) -> Result<Option<EndDerivs>> {
    let n = samples.normals[i];
    let len = end.magnitude * total;
    let c = samples.centers[i];
    let radius = |j: usize| -> f64 {
        let cj = samples.centers[j];
        let pts: Vec<&P3> = samples.patches.iter().flat_map(|p| p[j].iter()).collect();
        pts.iter().map(|p| (*p - cj).norm()).sum::<f64>() / pts.len().max(1) as f64
    };
    let per_sample = |f: &dyn Fn(&P3) -> V3| -> Vec<Vec<V3>> {
        samples
            .patches
            .iter()
            .map(|p| p[i].iter().map(f).collect())
            .collect()
    };
    let radial = |normal: V3, length: f64| {
        let nn = normal.normalize();
        move |p: &P3| {
            let d = p - c;
            let r = d - nn * d.dot(&nn);
            if r.norm() < tol::DEGENERATE {
                V3::zeros()
            } else {
                r.normalize() * length
            }
        }
    };
    let sign_toward = if radius(toward) < radius(i) {
        -1.0
    } else {
        1.0
    };
    // At the last section the loft arrives: a radial derivative flares the other way.
    let end_flip = if first { 1.0 } else { -1.0 };
    Ok(match end.condition {
        LoftCondition::Default => None,
        LoftCondition::NormalToProfile => Some((per_sample(&|_| n * len), None)),
        LoftCondition::TangentToProfile => {
            Some((per_sample(&radial(n, sign_toward * len * end_flip)), None))
        }
        LoftCondition::NormalDirection(d) | LoftCondition::TangentDirection(d) => {
            let d = V3::new(d[0], d[1], d[2]);
            if d.norm() < tol::DEGENERATE || !d.iter().all(|x| x.is_finite()) {
                return Err(invalid("A loft direction needs a non-zero vector"));
            }
            let along = if n.dot(&d).abs() > tol::ROUNDOFF {
                n
            } else {
                let cc = samples.centers[toward] - samples.centers[i];
                if first { cc } else { -cc }
            };
            let d = if along.dot(&d) < 0.0 {
                -d.normalize()
            } else {
                d.normalize()
            };
            if matches!(end.condition, LoftCondition::NormalDirection(_)) {
                Some((per_sample(&|_| d * len), None))
            } else {
                Some((per_sample(&radial(d, sign_toward * len * end_flip)), None))
            }
        }
        LoftCondition::MatchTangent | LoftCondition::MatchCurvature => {
            let LoftSection::Face(fi) = sec else {
                return Err(invalid(
                    "Match tangent and Match curvature need a face as the profile (its neighbouring faces)",
                ));
            };
            let curvature = end.condition == LoftCondition::MatchCurvature;
            Some(match_derivatives(k, fi, samples, i, len, curvature)?)
        }
    })
}

/// Derivatives that continue the faces next to a face section: at each sample, the direction
/// in the neighbour's tangent plane square to the boundary, leaving the first section away
/// from its body (arriving into the last); with curvature, the neighbour's normal curvature in
/// that direction as the second derivative.
fn match_derivatives(
    k: &NativeKernel,
    fi: &FaceInput,
    samples: &crate::loft::Samples,
    i: usize,
    len: f64,
    curvature: bool,
) -> Result<EndDerivs> {
    let b = k.get(fi.body)?;
    let face = fi.face.0 as usize;
    let ef = b.edge_faces();
    // The neighbouring face across each boundary edge of the face.
    let mut neighbours: Vec<(usize, usize)> = Vec::new(); // (edge, other face)
    for c in b.faces[face].loops.iter().flatten() {
        if let Some(&g) = ef[c.edge].iter().find(|&&g| g != face) {
            neighbours.push((c.edge, g));
        }
    }
    let n_sec = samples.normals[i];
    let mut d1 = Vec::new();
    let mut d2 = Vec::new();
    for patch in &samples.patches {
        let pts = &patch[i];
        let mut r1 = Vec::new();
        let mut r2 = Vec::new();
        for p in pts {
            // The nearest boundary edge and its neighbour.
            let mut best: Option<(f64, usize, usize)> = None;
            for &(e, g) in &neighbours {
                let ed = &b.edges[e];
                let t = ed.curve.closest(p, ed.t[0], ed.t[1]);
                let d = (ed.point(t) - p).norm();
                if best.is_none_or(|(bd, ..)| d < bd) {
                    best = Some((d, e, g));
                }
            }
            let Some((_, e, g)) = best else {
                return Err(invalid("The face has no neighbouring faces to match"));
            };
            let ed = &b.edges[e];
            let t = ed.curve.closest(p, ed.t[0], ed.t[1]);
            let tan = ed.curve.tangent(t).normalize();
            let gf = &b.faces[g];
            let (u, v) = gf.surf.project(p, None);
            let nrm = gf.surf.normal(u, v) * if gf.sense { 1.0 } else { -1.0 };
            let mut dir = tan.cross(&nrm).normalize();
            // Leave the first section along the loft (away from the face's body), arrive into
            // the last.
            if dir.dot(&n_sec) < 0.0 {
                dir = -dir;
            }
            r1.push(dir * len);
            if curvature {
                // Normal curvature of the neighbour in direction dir.
                let s = gf.surf.d2(u, v);
                let (su, sv) = (s[1], s[2]);
                let (e1, f1, g1) = (su.dot(&su), su.dot(&sv), sv.dot(&sv));
                let nn = su.cross(&sv).normalize();
                let (l2, m2, n2) = (s[3].dot(&nn), s[4].dot(&nn), s[5].dot(&nn));
                // dir in (du, dv).
                let Some((du, dv)) = solve2(e1, f1, f1, g1, dir.dot(&su), dir.dot(&sv)) else {
                    r2.push(V3::zeros());
                    continue;
                };
                let num = l2 * du * du + 2.0 * m2 * du * dv + n2 * dv * dv;
                let den = e1 * du * du + 2.0 * f1 * du * dv + g1 * dv * dv;
                let kappa = if den.abs() > tol::TINY {
                    num / den
                } else {
                    0.0
                };
                r2.push(nn * (kappa * len * len));
            }
        }
        d1.push(r1);
        d2.push(r2);
    }
    Ok((d1, if curvature { Some(d2) } else { None }))
}

/// The interpolating loft through sampled sections (end conditions, unmatched sections).
fn loft_sampled(k: &NativeKernel, spec: &LoftSpec, solid: bool) -> Result<Built> {
    let mut sc = Vec::new();
    for s in &spec.sections {
        match s {
            LoftSection::Profile(p) => sc.push(SectionCurves::new(p.plane, section_loop(p)?)),
            LoftSection::Face(fi) => sc.push(face_section(k, fi)?),
            LoftSection::Point(_) => {
                return Err(invalid(
                    "End conditions need profiles at both ends, not points",
                ));
            }
        }
    }
    let samples = sample_sections(&sc)?;
    let n = samples.centers.len();
    let mut v = vec![0.0];
    for i in 1..n {
        v.push(v[i - 1] + (samples.centers[i] - samples.centers[i - 1]).norm());
    }
    let total = v[n - 1];
    if total < tol::ROUNDOFF {
        return Err(invalid("The loft's profiles lie on top of each other"));
    }
    let vp: Vec<f64> = v.iter().map(|x| x / total).collect();
    let d0 = end_derivative(
        k,
        &spec.start,
        &spec.sections[0],
        &samples,
        0,
        1,
        total,
        true,
    )?;
    let d1 = end_derivative(
        k,
        &spec.end,
        &spec.sections[n - 1],
        &samples,
        n - 1,
        n - 2,
        total,
        false,
    )?;
    let periodic = samples.periodic;
    let mut surfs = Vec::new();
    for (pi, patch) in samples.patches.iter().enumerate() {
        let m = patch[0].len();
        // Parameters along the section (shared by every section).
        let up: Vec<f64> = if periodic {
            (0..=m).map(|j| j as f64 / m as f64).collect()
        } else {
            (0..m).map(|j| j as f64 / (m - 1) as f64).collect()
        };
        let pts_of = |row: &Vec<P3>| -> Vec<P3> {
            let mut r = row.clone();
            if periodic {
                r.push(row[0]);
            }
            r
        };
        let curves: Vec<NurbsCurve> = patch
            .iter()
            .map(|row| NurbsCurve::interpolate(&pts_of(row), &up, 3, None, None))
            .collect();
        // Derivatives of the poles: interpolate the sample derivatives with the same basis.
        let deriv_poles = |ds: &Option<EndDerivs>, second: bool| -> Option<Vec<V3>> {
            let (a, b) = ds.as_ref()?;
            let rows = if second { b.as_ref()? } else { a };
            let mut row: Vec<P3> = rows[pi].iter().map(|d| P3::from(*d)).collect();
            if periodic {
                row.push(row[0]);
            }
            let c = NurbsCurve::interpolate(&row, &up, 3, None, None);
            Some(c.cps.iter().map(|h| V3::new(h.x, h.y, h.z)).collect())
        };
        let (sd0, sd1) = (deriv_poles(&d0, false), deriv_poles(&d1, false));
        let (ss0, ss1) = (deriv_poles(&d0, true), deriv_poles(&d1, true));
        let ncp = curves[0].cps.len();
        let mut cols: Vec<Vec<H>> = Vec::new();
        let mut kv = Vec::new();
        for i in 0..ncp {
            let col: Vec<P3> = curves.iter().map(|c| dehom(&c.cps[i])).collect();
            let c = NurbsCurve::interpolate_ext(
                &col,
                &vp,
                3,
                [sd0.as_ref().map(|d| d[i]), sd1.as_ref().map(|d| d[i])],
                [ss0.as_ref().map(|d| d[i]), ss1.as_ref().map(|d| d[i])],
            );
            kv = c.knots.clone();
            cols.push(c.cps);
        }
        surfs.push(NurbsSurface {
            du: curves[0].degree,
            dv: 3.min(cols[0].len() - 1),
            ku: curves[0].knots.clone(),
            kv,
            cps: cols,
        });
    }
    let srcs: Vec<u64> = samples
        .sources
        .iter()
        .enumerate()
        .map(|(i, s)| s.unwrap_or(unsourced_curve(0, i)))
        .collect();
    // Caps: planar sections' planes; face sections: the face itself.
    let cap = |s: &LoftSection| -> Option<(Surface, Option<FaceInput>)> {
        match s {
            LoftSection::Profile(p) => Some((
                Surface::Plane {
                    f: super::build::plane_frame(&p.plane, V3::zeros()),
                },
                None,
            )),
            LoftSection::Face(fi) => k
                .get(fi.body)
                .ok()
                .map(|b| (b.faces[fi.face.0 as usize].surf.clone(), Some(*fi))),
            LoftSection::Point(_) => None,
        }
    };
    assemble(
        surfs,
        periodic || true,
        &srcs,
        [cap(&spec.sections[0]), cap(&spec.sections[n - 1])],
        [false, false],
        solid,
        spec.source,
    )
}

/// A face's outer loop as a sampled section.
fn face_section(k: &NativeKernel, fi: &FaceInput) -> Result<SectionCurves> {
    let b = k.get(fi.body)?;
    let f = b
        .faces
        .get(fi.face.0 as usize)
        .ok_or_else(|| invalid("unknown face"))?;
    let mut polys = Vec::new();
    for c in &f.loops[0] {
        let e = &b.edges[c.edge];
        if e.degenerate {
            continue;
        }
        let n = if matches!(e.curve, Curve::Line { .. }) {
            1
        } else {
            4096
        };
        let mut pts: Vec<P3> = (0..=n)
            .map(|i| e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / n as f64))
            .collect();
        if !c.fwd {
            pts.reverse();
        }
        polys.push((pts, Some(c.edge as u64)));
    }
    // The plane: the face's, or the loop's best fit.
    let pts: Vec<P3> = polys.iter().flat_map(|(p, _)| p.iter().copied()).collect();
    let c = pts.iter().fold(V3::zeros(), |a, p| a + p.coords) / pts.len() as f64;
    let mut nn = V3::zeros();
    for i in 0..pts.len() {
        nn += (pts[i].coords - c).cross(&(pts[(i + 1) % pts.len()].coords - c));
    }
    let nn = if nn.norm() > tol::DEGENERATE {
        nn.normalize()
    } else {
        V3::z()
    };
    let fr = Frame::from_z(P3::from(c), nn);
    let plane = Plane {
        origin: P3::from(c),
        x_dir: nalgebra::Unit::new_normalize(fr.x),
        normal: nalgebra::Unit::new_normalize(nn),
    };
    Ok(SectionCurves::sampled(plane, polys))
}

pub fn loft_with(k: &mut NativeKernel, spec: &LoftSpec) -> Result<OpResult> {
    if spec.sections.len() < 2 {
        return Err(invalid("Select at least two profiles to loft"));
    }
    for (i, s) in spec.sections.iter().enumerate() {
        if matches!(s, LoftSection::Point(_)) && i != 0 && i + 1 != spec.sections.len() {
            return Err(invalid(
                "A point can only be the first or last loft profile",
            ));
        }
    }
    for e in [&spec.start, &spec.end] {
        if e.condition != LoftCondition::Default && !(e.magnitude.is_finite() && e.magnitude > 0.0)
        {
            return Err(invalid("A loft end's magnitude must be greater than zero"));
        }
    }
    if let BodyKind::Thin { left, right } = spec.body {
        return loft_thin(k, spec, left, right);
    }
    let solid = spec.body == BodyKind::Solid;
    let conditioned = spec.start.condition != LoftCondition::Default
        || spec.end.condition != LoftCondition::Default;
    let mut secs: Vec<Sec> = spec
        .sections
        .iter()
        .map(|s| section(k, s))
        .collect::<Result<_>>()?;
    if conditioned {
        for (end, sec) in [
            (&spec.start, spec.sections.first()),
            (&spec.end, spec.sections.last()),
        ] {
            let matching = matches!(
                end.condition,
                LoftCondition::MatchTangent | LoftCondition::MatchCurvature
            );
            if matching && !matches!(sec, Some(LoftSection::Face(_))) {
                return Err(invalid(
                    "Match tangent and Match curvature need a face as the profile (its neighbouring faces)",
                ));
            }
            if end.condition != LoftCondition::Default && matches!(sec, Some(LoftSection::Point(_)))
            {
                return Err(invalid(
                    "End conditions need profiles at both ends, not points",
                ));
            }
        }
    }
    let mut built = match loft_exact(k, spec, &mut secs, solid)? {
        Some(b) => b,
        None => loft_sampled(k, spec, solid)?,
    };
    crate::native::unify::straighten(&mut built.body);
    // A face section that isn't flat is the loft's own cap; a flat face section's cap lies on the
    // face (both work the same here).
    Ok(Tagged::from_built(built).into_result(k))
}

/// A thin loft: the loft of the sections' outer walls less that of their inner walls.
fn loft_thin(k: &mut NativeKernel, spec: &LoftSpec, left: f64, right: f64) -> Result<OpResult> {
    let mut outer = Vec::new();
    let mut inner = Vec::new();
    for s in &spec.sections {
        let LoftSection::Profile(p) = s else {
            return Err(KernelError::Unsupported("thin lofts of faces or points"));
        };
        let bands = crate::thin::thin_profile(p, left, right)?;
        let regions = super::profile::regions(&bands)?;
        let mf = super::profile::merge_regions(&regions)?;
        if mf.len() != 1 || mf[0].holes.len() != 1 {
            return Err(KernelError::Unsupported(
                "thin lofts of open or branching profiles",
            ));
        }
        let to_profile = |l: &[super::profile::Piece]| -> Result<Profile> {
            // Back to sketch curves for the section (exact circles and lines).
            let curves: Vec<Curve2> = l
                .iter()
                .map(piece_curve2)
                .collect::<Option<Vec<_>>>()
                .ok_or(KernelError::Unsupported("thin lofts of these curves"))?;
            Ok(Profile::new(
                p.plane,
                vec![Region {
                    outer: Loop { curves },
                    holes: vec![],
                    source: None,
                }],
            ))
        };
        outer.push(LoftSection::Profile(to_profile(&mf[0].outer)?));
        inner.push(LoftSection::Profile(to_profile(&mf[0].holes[0])?));
    }
    let a = loft_with(
        k,
        &LoftSpec {
            body: BodyKind::Solid,
            sections: outer,
            ..spec.clone()
        },
    )?;
    let b = loft_with(
        k,
        &LoftSpec {
            body: BodyKind::Solid,
            sections: inner,
            ..spec.clone()
        },
    )?;
    let r = k.boolean(BoolOp::Subtract, a.bodies[0], &[b.bodies[0]])?;
    k.release(a.bodies[0]);
    k.release(b.bodies[0]);
    Ok(r)
}

fn piece_curve2(p: &super::profile::Piece) -> Option<Curve2> {
    let pt = |q: P3| nalgebra::Point2::new(q.x, q.y);
    match &p.c {
        Curve::Line { .. } => Some(Curve2::Line {
            a: p.start(),
            b: p.end(),
            source: None,
        }),
        Curve::Circle { c, x, y, r } => {
            // Angles in the sketch frame.
            let ang = |t: f64| {
                let q = p.c.point(t) - c;
                q.y.atan2(q.x)
            };
            let full = (p.t1 - p.t0) >= TAU - tol::ANGULAR;
            if full {
                return Some(Curve2::Circle {
                    center: pt(*c),
                    radius: *r,
                    source: None,
                });
            }
            let start = ang(if p.rev { p.t1 } else { p.t0 });
            let sweep = (p.t1 - p.t0) * if p.rev { -1.0 } else { 1.0 } * x.cross(y).z.signum();
            Some(Curve2::Arc {
                center: pt(*c),
                radius: *r,
                start_angle: start,
                sweep,
                source: None,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::profile::{Piece, Src};

    fn square(plane: Plane, nan: bool) -> (Vec<Piece>, Plane) {
        let c = |x: f64, y: f64| P3::new(if nan { f64::NAN } else { x }, y, 0.0);
        let corners = [c(0.0, 0.0), c(10.0, 0.0), c(10.0, 10.0), c(0.0, 10.0)];
        let pieces = (0..4)
            .map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                Piece {
                    c: Curve::Line { o: a, d: b - a },
                    t0: 0.0,
                    t1: 1.0,
                    rev: false,
                    src: Src::Curve {
                        region: 0,
                        curve: i as u64,
                    },
                    more: vec![],
                }
            })
            .collect();
        (pieces, plane)
    }

    /// Sections on top of each other, or with NaN coordinates (as a blend's degenerate chain
    /// made in OCCT `blend complex_e7`), are an error, not an index panic in a NURBS evaluator.
    #[test]
    fn degenerate_sections_are_errors() {
        let k = NativeKernel::new();
        let top = Plane::top();
        let up = Plane {
            origin: nalgebra::Point3::new(0.0, 0.0, 20.0),
            ..top
        };
        assert!(loft_pieces(&k, &[square(top, false), square(up, false)], 0).is_ok());
        assert!(loft_pieces(&k, &[square(top, false), square(top, false)], 0).is_err());
        assert!(loft_pieces(&k, &[square(top, true), square(up, true)], 0).is_err());
    }
}
