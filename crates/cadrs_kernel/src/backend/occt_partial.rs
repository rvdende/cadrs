//! Partial fillets of the OCCT backend (P3.11, PS14.6, [`Kernel::fillet_partial`]).
//!
//! `BRepFilletAPI_MakeFillet` always runs a contour to the ends of its edges (and on along
//! tangent edges), and splitting the edge first doesn't stop it (the pieces are tangent, so the
//! contour runs on across the split). So the partial fillet is built from the whole edge's
//! fillet instead: the material that fillet removes (a convex edge) or adds (a concave one) is
//! cut down to the part between the planes square to the edge at the two bounds, and only that
//! part is cut from (or added to) the body. The fillet face is exactly the whole fillet's face
//! between the bounds, and each bound gets a flat end face square to the edge (Onshape's partial
//! fillet stops the same way). A curved edge is taken in pieces that each turn less than 60°,
//! so every piece lies in the wedge between its two planes.
//!
//! At a bound of 0 or 1 whose vertex has no tangent neighbour the plane is moved out past the
//! vertex, so the fillet ends there as the whole edge's fillet does.

use std::f64::consts::PI;

use nalgebra::{Point2, Point3, Unit, Vector3};
use opencascade::primitives::Shape;

use super::{OcctKernel, ToolFaces, face_count, occt, profile_faces, to_glam, to_na};
use crate::{
    BodyId, Curve2, EdgeId, FilletSize, FilletSpec, FilletProfile, Kernel, KernelError, Loop, OpResult,
    Origin, Plane, Region, Result, SurfaceKind,
};

/// Samples along the edge for its points and tangents at the bounds.
const SAMPLES: usize = 1024;

/// The largest turn of the edge's tangent within one piece.
const PIECE_TURN: f64 = PI / 3.0;

impl OcctKernel {
    pub(super) fn fillet_partial_full(&mut self, body: BodyId, edge: EdgeId, spec: &FilletSpec, from: f64, to: f64) -> Result<OpResult> {
        if !(from.is_finite() && to.is_finite() && (0.0..=1.0).contains(&from) && (0.0..=1.0).contains(&to)) {
            return Err(KernelError::InvalidParameter("The partial fillet's bounds must lie on the edge".into()));
        }
        if to - from < 1e-6 {
            return Err(KernelError::InvalidParameter("The partial fillet's end must come after its start".into()));
        }
        let size = match spec.size {
            FilletSize::Radius(r) | FilletSize::Width(r) => r,
        };
        let size = size.max(match spec.profile {
            FilletProfile::Asymmetric { second, .. } => second,
            _ => 0.0,
        });
        if !(size.is_finite() && size > 0.0) {
            return Err(KernelError::InvalidParameter("The fillet size must be positive".into()));
        }
        let infos = self.edges(body)?;
        let info = infos
            .iter()
            .find(|e| e.id == edge)
            .cloned()
            .ok_or_else(|| KernelError::OperationFailed(format!("unknown {edge:?}")))?;
        // Which ends continue into tangent edges (the whole fillet runs on there).
        let chain = self.tangent_chain(body, edge)?;
        let tol = 1e-6 * info.length.max(1.0);
        let continues = |p: Point3<f64>| {
            chain.iter().filter(|c| **c != edge).any(|c| {
                infos.iter().any(|i| i.id == *c && ((i.start - p).norm() < tol || (i.end - p).norm() < tol))
            })
        };
        let closed = (info.start - info.end).norm() < tol;
        let (open_start, open_end) = if closed { (false, false) } else { (!continues(info.start), !continues(info.end)) };

        // Points evenly spaced by length, in the direction the edge runs.
        let shape = self.body(body)?;
        let (lo, hi) = shape.bbox().map_err(occt)?;
        let big = 4.0 * (hi - lo).length().max(1.0);
        let mut pts: Vec<Point3<f64>> = shape
            .edge_samples(edge.0 as usize, SAMPLES)
            .map_err(occt)?
            .into_iter()
            .map(|p| Point3::from(to_na(p)))
            .collect();
        if pts.len() != SAMPLES + 1 {
            return Err(KernelError::OperationFailed("the edge could not be sampled".into()));
        }
        if (pts[1] - pts[0]).dot(&info.start_tangent) < 0.0 {
            pts.reverse();
        }
        let at = |s: f64| -> (Point3<f64>, Vector3<f64>) {
            let x = s * SAMPLES as f64;
            let i = (x.floor() as usize).min(SAMPLES - 1);
            let f = x - i as f64;
            let p = pts[i] + (pts[i + 1] - pts[i]) * f;
            let (a, b) = (i.saturating_sub(1), (i + 2).min(SAMPLES));
            let t = if f < 0.5 { pts[i + 1] - pts[a] } else { pts[b] - pts[i] };
            (p, t.normalize())
        };
        // Pieces that each turn less than PIECE_TURN.
        let (i0, i1) = ((from * SAMPLES as f64).floor() as usize, ((to * SAMPLES as f64).ceil() as usize).min(SAMPLES));
        let mut turn = 0.0;
        for i in i0.max(1)..i1 {
            let (a, b) = ((pts[i] - pts[i - 1]).normalize(), (pts[i + 1] - pts[i]).normalize());
            turn += a.dot(&b).clamp(-1.0, 1.0).acos();
        }
        let pieces = ((turn / PIECE_TURN).ceil() as usize).max(1);

        // The whole edge's fillet, and what it takes away and adds.
        let whole = self.fillet_with(body, &[edge], spec)?;
        let filleted = whole.bodies[0];
        let (removed, added) = {
            let o = self.body(body)?;
            let f = self.body(filleted)?;
            let removed = o.try_subtract_h(f).map_err(occt).map(|(s, _)| s);
            let added = f.try_subtract_h(o).map_err(occt).map(|(s, _)| s);
            (removed, added)
        };
        for b in whole.bodies {
            self.release(b);
        }
        let (removed, added) = (removed?, added?);

        // The half-space on the `dir` side of the plane through `p` square to `dir`: a big block.
        let half_space = |p: Point3<f64>, dir: Vector3<f64>| -> Result<Shape> {
            let n = Unit::new_normalize(dir);
            let helper = if n.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
            let x_dir = Unit::new_normalize(helper - n.into_inner() * helper.dot(&n));
            let plane = Plane { origin: p, x_dir, normal: n };
            let c = |x: f64, y: f64| Point2::new(x, y);
            let corners = [c(-big, -big), c(big, -big), c(big, big), c(-big, big)];
            let curves = (0..4).map(|k| Curve2::Line { a: corners[k], b: corners[(k + 1) % 4], source: None }).collect();
            let region = Region { outer: Loop { curves }, holes: Vec::new(), source: None };
            let face = profile_faces(&plane, &[region])?.remove(0);
            face.try_extrude(to_glam(n.into_inner() * big)).map_err(occt)
        };
        let margin = 4.0 * size;
        let mut tools: Vec<(Shape, bool, ToolFaces)> = Vec::new();
        for (material, add) in [(&removed, false), (&added, true)] {
            if face_count(material)? == 0 {
                continue;
            }
            let mut piece_shapes: Vec<Shape> = Vec::new();
            for k in 0..pieces {
                let a = from + (to - from) * k as f64 / pieces as f64;
                let b = from + (to - from) * (k + 1) as f64 / pieces as f64;
                let (mut pa, ta) = at(a);
                let (mut pb, tb) = at(b);
                if k == 0 && a <= 1e-9 && open_start {
                    pa -= ta * margin;
                }
                if k + 1 == pieces && b >= 1.0 - 1e-9 && open_end {
                    pb += tb * margin;
                }
                let h0 = half_space(pa, ta)?;
                let h1 = half_space(pb, -tb)?;
                let (cut, _) = material.try_intersect_h(&h0).map_err(occt)?;
                if face_count(&cut)? == 0 {
                    continue;
                }
                let (cut, _) = cut.try_intersect_h(&h1).map_err(occt)?;
                if face_count(&cut)? > 0 {
                    piece_shapes.push(cut);
                }
            }
            let mut iter = piece_shapes.into_iter();
            if let Some(mut tool) = iter.next() {
                for next in iter {
                    tool = tool.try_union_clean_h(&next).map_err(occt)?.0;
                }
                tools.push((tool, add, ToolFaces::FromEdge(edge)));
            }
        }
        if tools.is_empty() {
            return Err(KernelError::OperationFailed("the partial fillet takes nothing from the edge".into()));
        }
        let mut result = self.apply_tools(body, &tools)?;
        // The flat faces at the bounds are the fillet's end faces.
        let (pa, ta) = at(from);
        let (pb, tb) = at(to);
        let faces = self.faces(result.bodies[0])?;
        let plane_tol = 1e-6 * size.max(1.0);
        for (face, origin) in &mut result.history.generated {
            if !matches!(origin, Origin::FromEdge { .. }) {
                continue;
            }
            let Some(info) = faces.iter().find(|f| f.id == *face) else { continue };
            let Some(plane) = info.plane.filter(|_| info.kind == SurfaceKind::Plane) else { continue };
            let on = |p: Point3<f64>, t: Vector3<f64>| {
                plane.normal.cross(&t).norm() < 1e-6 && (info.center - p).dot(&t).abs() < plane_tol
            };
            if on(pa, ta) {
                *origin = Origin::StartCap { region: 0 };
            } else if on(pb, tb) {
                *origin = Origin::EndCap { region: 0 };
            }
        }
        Ok(result)
    }
}
