//! What the cadrs implementations of the spline cases share: a planar face bounded by curves a
//! sketch cannot hold (B-splines and rational Béziers in space), and the solids an Extrude and a
//! Revolve make of it.
//!
//! cadrs's sketch curves are lines, arcs, circles, ellipses and polynomial cubic Béziers. A
//! (rational) B-spline enters the kernel as a curve of a wire body (`create_wire`, as cadrs's
//! Part Studio passes a curve feature's curve), and the planar face it bounds is its Fill (a
//! planar chain is filled with its plane); a solid Extrude or Revolve then takes that face as its
//! input, as Onshape's do a face.
use crate::Refusal;
use crate::bench::{V3, unit};
use crate::shared::sweep::first;
use cadrs_kernel::{
    Axis, BodyId, DefaultKernel, ExtrudeSpec, FaceInput, FillCurve, FillSpec, Kernel, PathCurve,
    Plane, Profile, RevolveSpec,
};

/// The planar face filling the closed chain `curves` (joined end to end into a wire), as the
/// face input of a feature.
pub fn filled(k: &mut DefaultKernel, curves: &[PathCurve]) -> Result<FaceInput, Refusal> {
    let wire = first(k.create_wire(curves)?)?;
    let edges = k.edges(wire)?;
    let boundary = edges
        .iter()
        .map(|e| FillCurve::Edge {
            body: wire,
            edge: e.id,
        })
        .collect();
    let sheet = first(k.fill(&FillSpec {
        curves: boundary,
        source: 1,
    })?)?;
    let face = k
        .faces(sheet)?
        .first()
        .map(|f| f.id)
        .ok_or("the fill made no face")?;
    Ok(FaceInput {
        body: sheet,
        face,
        source: 1,
    })
}

/// The solid `face` (on `plane`) makes extruded `depth` along `dir`.
pub fn extrude_face(
    k: &mut DefaultKernel,
    plane: Plane,
    face: FaceInput,
    dir: V3,
    depth: f64,
) -> Result<BodyId, Refusal> {
    let mut spec = ExtrudeSpec::blind(unit(dir), depth);
    spec.faces = vec![face];
    first(k.extrude_with(&Profile::new(plane, vec![]), &spec)?)
}

/// The solid `face` (on `plane`) makes revolved a whole turn about `axis`.
pub fn revolve_face(
    k: &mut DefaultKernel,
    plane: Plane,
    face: FaceInput,
    axis: Axis,
) -> Result<BodyId, Refusal> {
    let mut spec = RevolveSpec::solid(axis, None);
    spec.faces = vec![face];
    first(k.revolve_with(&Profile::new(plane, vec![]), &spec)?)
}
