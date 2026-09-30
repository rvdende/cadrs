//! Thicken, Fill and sewing of the OCCT backend ([`Kernel::thicken`], [`Kernel::fill`],
//! [`Kernel::sew_solid`]; Onshape's surfacing features).
//!
//! **Thicken** (`BRepOffsetAPI_MakeThickSolid::MakeThickSolidBySimple`, fork
//! `Shape::try_thicken_h`): each face (of a surface body, a picked face, a sketch region's face)
//! is offset along its normal (a positive offset) and the other way; the slabs are fused.
//!
//! **Fill**: the boundary chained into a closed wire. A planar chain is filled with a planar
//! face on it; a chain of four curves (or three) with a B-spline surface through a Coons patch
//! of them (bilinearly blended boundaries, sampled on a grid and interpolated by the fork's
//! `Shape::try_loft_solid`). Other chains are refused.

use glam::DVec3;
use opencascade::primitives::{Edge, Face, Shape, Wire};
use opencascade::safe::LoftDerivative;

use super::sweep::chain;
use super::{OcctKernel, curve_edge, face_count, faces_of, generated, occt, profile_faces, select_edges};
use crate::{FillCurve, FillSpec, KernelError, OpResult, Origin, Result, ThickenSpec};

fn invalid(why: impl Into<String>) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

impl OcctKernel {
    pub(super) fn thicken_full(&mut self, spec: &ThickenSpec) -> Result<OpResult> {
        if !(spec.along >= 0.0 && spec.against >= 0.0 && spec.along + spec.against > 1e-6) {
            return Err(invalid("The thickness must be greater than zero"));
        }
        // Each face is thickened on its own and the slabs fused: OCCT's simple thickening of
        // several faces together (a surface's shell, or faces sewn only by shared edges) can
        // come out as a fraction of the wall.
        let mut sheets: Vec<Shape> = Vec::new();
        for b in &spec.bodies {
            sheets.extend(faces_of(self.body(*b)?).iter().map(Shape::from));
        }
        for f in &spec.faces {
            let all = faces_of(self.body(f.body)?);
            sheets.push(all.get(f.face.0 as usize).map(Shape::from).ok_or_else(|| invalid("A face to thicken no longer exists"))?);
        }
        for p in &spec.profiles {
            for f in profile_faces(&p.plane, &p.regions)? {
                sheets.push(Shape::from(&f));
            }
        }
        if sheets.is_empty() {
            return Err(invalid("Select surfaces or faces to thicken"));
        }
        let mut solids: Vec<Shape> = Vec::new();
        // A slab made inside out (OCCT's simple thickening can give one) is sewn again, which
        // orients it.
        let slab = |sheet: &Shape, d: f64| -> Result<Shape> {
            let s = sheet.try_thicken_h(d).map_err(occt)?.0;
            if s.mass_properties().volume < 0.0 {
                Shape::try_sew_solid(&[&s], 1e-6).map_err(occt)
            } else {
                Ok(s)
            }
        };
        for sheet in &sheets {
            if spec.along > 1e-9 {
                solids.push(slab(sheet, spec.along)?);
            }
            if spec.against > 1e-9 {
                solids.push(slab(sheet, -spec.against)?);
            }
        }
        let mut acc = solids.remove(0);
        for s in solids {
            acc = acc.try_union(&s).map_err(occt)?;
        }
        let n = face_count(&acc)?;
        let tags = (0..n).map(|i| Some(Origin::ProfileCurve { region: spec.source, curve: i as u64 })).collect();
        self.insert(acc, generated(tags))
    }

    pub(super) fn fill_full(&mut self, spec: &FillSpec) -> Result<OpResult> {
        let mut edges: Vec<Edge> = Vec::new();
        for c in &spec.curves {
            match c {
                FillCurve::Edge { body, edge } => edges.extend(select_edges(self.body(*body)?, &[*edge])?),
                FillCurve::Sketch { plane, curve } => {
                    edges.push(curve_edge(plane, curve, curve.start(), curve.end(), curve.is_closed())?)
                }
            }
        }
        if edges.is_empty() {
            return Err(invalid("Select edges or curves to fill"));
        }
        let (path, closed) = chain(edges).map_err(|_| invalid("The boundary must be one chain of connected curves"))?;
        if !closed {
            return Err(invalid("The boundary is not closed"));
        }
        let lines: Vec<Vec<DVec3>> = path.iter().map(|e| e.points()).collect();
        let pts: Vec<DVec3> = lines.iter().flatten().copied().collect();
        let face = match best_plane(&pts) {
            Some(_) => {
                let oriented = path.iter().map(|e| e.oriented()).collect::<Result<Vec<_>>>()?;
                let wire = Wire::try_from_edges(&oriented).map_err(occt)?;
                Shape::from(&Face::try_from_wires(&wire, &[]).map_err(occt)?)
            }
            None => coons(&lines)?,
        };
        let n = face_count(&face)?;
        let tags = (0..n).map(|_| Some(Origin::StartCap { region: spec.source })).collect();
        self.insert(face, generated(tags))
    }

    pub(super) fn sew_full(&mut self, bodies: &[crate::BodyId], tol: f64) -> Result<OpResult> {
        let shapes: Vec<&Shape> = bodies.iter().map(|b| self.body(*b)).collect::<Result<_>>()?;
        let solid = Shape::try_sew_solid(&shapes, tol).map_err(occt)?;
        // Sewing makes a solid of an open shell too: it must be valid (closed).
        if !solid.is_valid().map_err(occt)? {
            return Err(invalid("The surfaces don't close"));
        }
        if solid.mass_properties().volume.abs() < 1e-9 {
            return Err(invalid("The surfaces don't enclose a volume"));
        }
        let n = face_count(&solid)?;
        let tags = (0..n).map(|i| Some(Origin::ProfileCurve { region: u64::MAX, curve: i as u64 })).collect();
        let r = self.insert(solid, generated(tags))?;
        // A free edge left (an edge with a face on one side only): the shell is open.
        let open = crate::Kernel::edges(self, r.bodies[0])?.iter().any(|e| e.faces[1].is_none() && e.length > 1e-9);
        if open {
            crate::Kernel::release(self, r.bodies[0]);
            return Err(invalid("The surfaces don't close"));
        }
        Ok(r)
    }
}

/// The plane through points (centroid and Newell normal), if they all lie on it (to 1e-6 of
/// their spread).
fn best_plane(pts: &[DVec3]) -> Option<(DVec3, DVec3)> {
    if pts.len() < 3 {
        return None;
    }
    let c = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
    let mut n = DVec3::ZERO;
    for i in 0..pts.len() {
        let (a, b) = (pts[i] - c, pts[(i + 1) % pts.len()] - c);
        n += a.cross(b);
    }
    let n = n.try_normalize()?;
    let size = pts.iter().map(|p| p.distance(c)).fold(0.0, f64::max).max(1e-9);
    pts.iter().all(|p| (*p - c).dot(n).abs() <= 1e-6 * size.max(1.0)).then_some((c, n))
}

/// Points at `n + 1` even steps of arc length along a polyline.
fn resample(line: &[DVec3], n: usize) -> Vec<DVec3> {
    let mut acc = vec![0.0];
    for w in line.windows(2) {
        acc.push(acc.last().copied().unwrap_or(0.0) + w[0].distance(w[1]));
    }
    let total = *acc.last().unwrap_or(&0.0);
    (0..=n)
        .map(|k| {
            let s = total * k as f64 / n as f64;
            let i = acc.partition_point(|a| *a < s).clamp(1, line.len().max(2) - 1);
            let (a0, a1) = (acc[i - 1], acc[i]);
            let t = if a1 > a0 { (s - a0) / (a1 - a0) } else { 0.0 };
            line[i - 1].lerp(line[i], t)
        })
        .collect()
}

/// A B-spline face through the Coons patch of a boundary of three or four curves (run in
/// order round the loop).
fn coons(lines: &[Vec<DVec3>]) -> Result<Shape> {
    const N: usize = 16;
    let sides: Vec<Vec<DVec3>> = match lines.len() {
        4 => lines.iter().map(|l| resample(l, N)).collect(),
        // Three curves: the fourth side is the point where the third ends.
        3 => {
            let mut v: Vec<Vec<DVec3>> = lines.iter().map(|l| resample(l, N)).collect();
            let p = *v[2].last().expect("points");
            v.push(vec![p; N + 1]);
            v
        }
        _ => {
            return Err(invalid(
                "A boundary that isn't flat can be filled when it has three or four curves",
            ));
        }
    };
    // S(u, v) with C0(u) = side 0, C2(u) = side 2 run backwards, D0(v) = side 3 run backwards,
    // D1(v) = side 1.
    let c0 = &sides[0];
    let d1 = &sides[1];
    let c2: Vec<DVec3> = sides[2].iter().rev().copied().collect();
    let d0: Vec<DVec3> = sides[3].iter().rev().copied().collect();
    let (p00, p10, p01, p11) = (c0[0], c0[N], c2[0], c2[N]);
    let grid: Vec<Vec<DVec3>> = (0..=N)
        .map(|j| {
            let v = j as f64 / N as f64;
            (0..=N)
                .map(|i| {
                    let u = i as f64 / N as f64;
                    c0[i] * (1.0 - v) + c2[i] * v + d0[j] * (1.0 - u) + d1[j] * u
                        - (p00 * ((1.0 - u) * (1.0 - v)) + p10 * (u * (1.0 - v)) + p01 * ((1.0 - u) * v) + p11 * (u * v))
                })
                .collect()
        })
        .collect();
    let vparams: Vec<f64> = (0..=N).map(|j| j as f64 / N as f64).collect();
    Shape::try_loft_solid(&[grid], false, &vparams, LoftDerivative::Free, LoftDerivative::Free, false).map_err(occt)
}
