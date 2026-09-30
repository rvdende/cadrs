//! Smooth fillet corners of the OCCT backend (Final, PS14.6, [`Kernel::fillet_smooth`]).
//!
//! OCCT's fillet (`ChFi3d`) closes a corner where three fillets meet with the exact sphere
//! (equal radii on square faces) or a `GeomFill_ConstrainedFilling` patch bounded by the
//! fillets' own ends, and has no setback. So the smooth corner is built afterwards, on the
//! filleted solid: a ball about the corner's vertex, of radius `ρ = √(setback² + r²)` (the
//! fillets' contact lines on the faces meet its sphere `setback` from the vertex), is cut away,
//! which leaves one spherical face where the corner was, bounded by the section of every face
//! round the corner: the flat (or curved) faces and the fillets. That face is replaced by an
//! N-sided filling (`BRepOffsetAPI_MakeFilling`, fork `Shape::try_fill_face`) through the same
//! edges, G1 to each neighbouring face, and the faces are sewn back into a solid.

use std::f64::consts::{PI, TAU};

use glam::dvec3;
use nalgebra::{Point2, Point3, Vector3};
use opencascade::primitives::Shape;

use super::{OcctKernel, clone_shape, face_count, occt, profile_faces, to_glam};
use crate::{
    BodyId, Curve2, EdgeId, FilletProfile, FilletSize, FilletSpec, History, InputFace, Kernel, KernelError, Loop, OpResult,
    Origin, Plane, Region, Result,
};

/// The largest angle (radians) between the patch and a neighbouring face along their edge that
/// still counts as tangent (G1): 1°.
pub const G1_ANGLE: f64 = PI / 180.0;

/// The filling (`BRepOffsetAPI_MakeFilling`): degree 3, 30 points on each boundary edge, 2
/// iterations, OCCT's default tolerances, and an approximation of degree up to 10 in up to 30
/// segments. On the conformance cube's corner the patch then meets its neighbours within
/// 0.37° everywhere, the worst at the hole's corners (0.95° with OCCT's default 8 and 9;
/// degree 4 or 5 surfaces bulge wildly).
const FILL_PARAMS: [f64; 9] = [3.0, 30.0, 2.0, 1e-5, 1e-4, 0.01, 0.1, 10.0, 30.0];

/// Where a face of the working solid came from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Src {
    /// The face of the filleted solid with this index.
    Fillet(usize),
    /// The patch of the corner with this index.
    Patch(usize),
    /// A face of a ball (while cutting).
    Ball,
}

impl OcctKernel {
    pub(super) fn fillet_smooth_full(&mut self, body: BodyId, edges: &[EdgeId], spec: &FilletSpec, setback: f64) -> Result<OpResult> {
        let r = match (spec.size, spec.profile) {
            (FilletSize::Radius(r), FilletProfile::Circular) => r,
            _ => {
                return Err(KernelError::InvalidParameter(
                    "Smooth fillet corners need a circular fillet with a radius".into(),
                ));
            }
        };
        if !(setback.is_finite() && setback > 0.0) {
            return Err(KernelError::InvalidParameter("The corner setback must be positive".into()));
        }
        // The edges with their tangent chains (as the fillet does), and the corners where three
        // or more of them meet.
        let mut all: Vec<EdgeId> = Vec::new();
        for &e in edges {
            for c in self.tangent_chain(body, e)? {
                if !all.contains(&c) {
                    all.push(c);
                }
            }
        }
        let vertices = self.vertices(body)?;
        let corners: Vec<_> = vertices
            .iter()
            .filter(|v| v.edges.iter().filter(|e| all.contains(e)).count() >= 3)
            .cloned()
            .collect();
        let whole = self.fillet_with(body, edges, spec)?;
        let filleted = whole.bodies[0];
        if corners.is_empty() {
            return Ok(whole);
        }
        let rho = (setback * setback + r * r).sqrt();
        // Into the material from each corner: along its filleted edges.
        let infos = self.edges(body)?;
        let corner_dirs: Vec<(Point3<f64>, Vector3<f64>)> = corners
            .iter()
            .map(|v| {
                let mut d = Vector3::zeros();
                for e in v.edges.iter().filter(|e| all.contains(e)) {
                    if let Some(i) = infos.iter().find(|i| i.id == *e) {
                        let t = if (i.start - v.point).norm() <= (i.end - v.point).norm() { i.start_tangent } else { -i.end_tangent };
                        d += t.normalize();
                    }
                }
                (v.point, d)
            })
            .collect();
        let result = self.smooth_corners(filleted, &corner_dirs, rho);
        let (shape, sources) = match result {
            Ok(x) => x,
            Err(e) => {
                self.release(filleted);
                return Err(e);
            }
        };
        // The history, through the fillet's.
        let mut history = History::default();
        for (i, s) in sources.iter().enumerate() {
            let face = crate::FaceId(i as u64);
            match *s {
                Some(Src::Fillet(j)) => {
                    let j = crate::FaceId(j as u64);
                    for (f, input) in &whole.history.modified {
                        if *f == j {
                            history.modified.push((face, *input));
                        }
                    }
                    for (f, origin) in &whole.history.generated {
                        if *f == j {
                            history.generated.push((face, *origin));
                        }
                    }
                }
                Some(Src::Patch(k)) => history.generated.push((face, Origin::FromVertex { body, vertex: corners[k].id })),
                _ => {}
            }
        }
        let n = face_count(self.body(body)?)?;
        for i in 0..n {
            let input = InputFace { body, face: crate::FaceId(i as u64) };
            if !history.modified.iter().any(|(_, x)| *x == input) {
                history.deleted.push(input);
            }
        }
        self.release(filleted);
        self.insert(shape, history)
    }

    /// The filleted solid with each corner (a vertex position and the direction into the
    /// material there) smoothed: the ball of radius `rho` about it cut away and the cut face
    /// filled. The faces' sources.
    fn smooth_corners(&self, filleted: BodyId, corners: &[(Point3<f64>, Vector3<f64>)], rho: f64) -> Result<(Shape, Vec<Option<Src>>)> {
        let mut shape = clone_shape(self.body(filleted)?);
        let mut sources: Vec<Option<Src>> = (0..face_count(&shape)?).map(|i| Some(Src::Fillet(i))).collect();
        for (k, &(v, inward)) in corners.iter().enumerate() {
            let fail = |why: String| KernelError::OperationFailed(format!("Smooth fillet corners: the corner at ({:.3}, {:.3}, {:.3}) {why}", v.x, v.y, v.z));
            let ball = ball(v, rho, inward)?;
            let (cut, h) = shape.try_subtract_h(&ball).map_err(|e| fail(format!("could not be cut back ({e})")))?;
            let theirs = vec![Some(Src::Ball); face_count(&ball)?];
            let mut next: Vec<Option<Src>> = vec![None; face_count(&cut)?];
            for (outs, src) in h.faces.iter().zip(sources.iter().chain(&theirs)) {
                for &f in outs {
                    if let Some(slot) = next.get_mut(f) {
                        *slot = slot.or(*src);
                    }
                }
            }
            let holes: Vec<usize> = next.iter().enumerate().filter(|(_, s)| **s == Some(Src::Ball)).map(|(i, _)| i).collect();
            let [hole] = holes[..] else {
                return Err(fail(format!("left {} cut faces where one was expected", holes.len())));
            };
            // G1 (MakeFilling refuses G2 against these supports: "the continuity is not G0 G1
            // or G2"). See FILL_PARAMS.
            let (filled, from, errors) = cut.try_fill_face(hole, 1, 1e-4, &FILL_PARAMS).map_err(|e| fail(format!("could not be filled ({e})")))?;
            if errors[1] > G1_ANGLE {
                return Err(fail(format!("could not be filled tangent to its neighbours ({:.2}°)", errors[1].to_degrees())));
            }
            // The patch stays in the ball (MakeFilling can return a wildly bulging surface that
            // still meets its constraints).
            let patch = from.iter().position(Option::is_none).ok_or_else(|| fail("lost its patch".into()))?;
            let faces = filled.sub_shapes(opencascade::safe::SubKind::Face).map_err(occt)?;
            let (lo, hi) = faces.get(patch).ok_or_else(|| fail("lost its patch".into()))?.bbox().map_err(occt)?;
            let reach = rho * 1.02;
            let c = to_glam(v.coords);
            if (lo - c).min_element() < -reach || (hi - c).max_element() > reach {
                return Err(fail("gave a patch that bulges out of the corner".into()));
            }
            sources = from.iter().map(|o| match o {
                Some(i) => next.get(*i).copied().flatten(),
                None => Some(Src::Patch(k)),
            }).collect();
            shape = filled;
        }
        if !shape.is_valid().map_err(occt)? {
            return Err(KernelError::OperationFailed("Smooth fillet corners: the result is not a valid solid".into()));
        }
        Ok((shape, sources))
    }
}

/// A ball: a half disc turned about its diameter. Its seam and poles are kept away from the
/// cap the corner's material cuts from it: the seam on the far side from `inward` (the
/// direction from the vertex into the material), the poles square to it.
fn ball(center: Point3<f64>, radius: f64, inward: Vector3<f64>) -> Result<Shape> {
    let d = if inward.norm() > 1e-9 { inward.normalize() } else { Vector3::z() };
    let helper = if d.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
    let axis = d.cross(&helper).normalize();
    let x_dir = -d;
    let normal = x_dir.cross(&axis);
    let plane = Plane { origin: center, x_dir: nalgebra::Unit::new_normalize(x_dir), normal: nalgebra::Unit::new_normalize(normal) };
    let half = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc { center: Point2::origin(), radius, start_angle: -PI / 2.0, sweep: PI, source: None },
                Curve2::Line { a: Point2::new(0.0, radius), b: Point2::new(0.0, -radius), source: None },
            ],
        },
        holes: Vec::new(),
        source: None,
    };
    let face = profile_faces(&plane, &[half])?;
    let (ball, _) = face[0].try_revolve_h(to_glam(center.coords), dvec3(axis.x, axis.y, axis.z), TAU).map_err(occt)?;
    Ok(ball)
}
