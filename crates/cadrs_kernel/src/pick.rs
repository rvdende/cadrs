//! Picking a body's faces and edges as a user picks them in the viewport: by a point on each, or
//! by what they are. [`Pick`] is on every [`Kernel`].

use crate::*;
use nalgebra::{Point3, Vector3};

/// Faces and edges picked by a point on them or by what they are, on any kernel.
pub trait Pick: Kernel {
    /// The face of `body` nearest `point` (a point on it, away from its edges).
    fn face_at(&self, body: BodyId, point: Point3<f64>) -> Result<FaceId> {
        let mut best: Option<(f64, FaceId)> = None;
        for f in self.faces(body)? {
            let d = self.min_distance(
                &DistanceTarget::Point(point),
                &DistanceTarget::Face(body, f.id),
            )?;
            if best.is_none_or(|(b, _)| d.distance < b) {
                best = Some((d.distance, f.id));
            }
        }
        best.map(|(_, f)| f)
            .ok_or_else(|| KernelError::OperationFailed("the body has no faces".into()))
    }

    /// The faces of `body` through each of `points`.
    fn faces_at(&self, body: BodyId, points: &[Point3<f64>]) -> Result<Vec<FaceId>> {
        points.iter().map(|&p| self.face_at(body, p)).collect()
    }

    /// The edge of `body` nearest `point` (a point on it, away from its ends); a seam is no edge
    /// to pick.
    fn edge_at(&self, body: BodyId, point: Point3<f64>) -> Result<EdgeId> {
        let mut best: Option<(f64, EdgeId)> = None;
        for e in self.edges(body)? {
            if e.is_seam() {
                continue;
            }
            let d = self.min_distance(
                &DistanceTarget::Point(point),
                &DistanceTarget::Edge(body, e.id),
            )?;
            if best.is_none_or(|(b, _)| d.distance < b) {
                best = Some((d.distance, e.id));
            }
        }
        best.map(|(_, e)| e)
            .ok_or_else(|| KernelError::OperationFailed("the body has no edges".into()))
    }

    /// The edges of `body` through each of `points`.
    fn edges_at(&self, body: BodyId, points: &[Point3<f64>]) -> Result<Vec<EdgeId>> {
        points.iter().map(|&p| self.edge_at(body, p)).collect()
    }

    /// The edges of `body`, its seams left out, that `keep` keeps.
    fn edges_where(&self, body: BodyId, keep: &dyn Fn(&EdgeInfo) -> bool) -> Result<Vec<EdgeId>> {
        Ok(self
            .edges(body)?
            .into_iter()
            .filter(|e| !e.is_seam() && keep(e))
            .map(|e| e.id)
            .collect())
    }

    /// Every edge of `body` but its seams.
    fn edges_but_seams(&self, body: BodyId) -> Result<Vec<EdgeId>> {
        self.edges_where(body, &|_| true)
    }

    /// The faces of `body` that `keep` keeps.
    fn faces_where(&self, body: BodyId, keep: &dyn Fn(&FaceInfo) -> bool) -> Result<Vec<FaceId>> {
        Ok(self
            .faces(body)?
            .into_iter()
            .filter(|f| keep(f))
            .map(|f| f.id)
            .collect())
    }

    /// Where `point` lies on `body`: the face, edge or vertex nearest it.
    fn location_at(&self, body: BodyId, point: Point3<f64>) -> Result<DistanceLocation> {
        Ok(self
            .min_distance(&DistanceTarget::Point(point), &DistanceTarget::Body(body))?
            .b_at)
    }

    /// The face of `body` through `point` and its outward normal there (which way a sheet's
    /// Thicken arrow points, say).
    fn normal_at(&self, body: BodyId, point: Point3<f64>) -> Result<(FaceId, Vector3<f64>)> {
        let face = self.face_at(body, point)?;
        let n = self.face_normals_at(body, face, &[point])?;
        n.first()
            .map(|n| (face, *n))
            .ok_or_else(|| KernelError::OperationFailed("the face has no normal there".into()))
    }

    /// The planar face filling the closed chain `curves` (joined end to end into a wire, the
    /// wire filled), as the face input of an Extrude or a Revolve: a face bounded by curves a
    /// sketch doesn't hold (curves in space).
    fn planar_face(&mut self, curves: &[PathCurve]) -> Result<FaceInput> {
        let wire = self.create_wire(curves)?.first()?;
        let edges: Vec<EdgeId> = self.edges(wire)?.iter().map(|e| e.id).collect();
        let sheet = self.fill(&FillSpec::edges(wire, &edges))?.first()?;
        let face = self
            .faces(sheet)?
            .first()
            .map(|f| f.id)
            .ok_or_else(|| KernelError::OperationFailed("the fill made no face".into()))?;
        Ok(FaceInput::new(sheet, face))
    }
}

impl<K: Kernel + ?Sized> Pick for K {}
