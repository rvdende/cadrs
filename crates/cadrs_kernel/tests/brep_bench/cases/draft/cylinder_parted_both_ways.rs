//!
//! The cylinder is a circle on the Top plane extruded 20; its side is split at the parting line by
//! cadrs's Split of faces (the plane z = 10), and each half drafted 3° with cadrs's Draft about
//! the neutral plane z = 10, the upper with pull +z and then the lower with pull −z (each half on
//! its own: the two are tangent, so no tangent propagation).
use crate::Refusal;
use crate::bench::{cylinder, face_at, p3, plane, unit, v3};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, DraftSpec, Kernel, SplitTool};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let parting = p3(0.0, 0.0, 10.0);
    let cyl = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 10.0, 20.0)?;
    let side = face_at(k, cyl, p3(10.0, 0.0, 5.0))?;
    let tool = SplitTool::Plane(plane(parting, v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)));
    let mut body = first(k.split_faces(cyl, &[side], &tool)?)?;
    for (pull, at) in [
        (v3(0.0, 0.0, 1.0), p3(10.0, 0.0, 15.0)),
        (v3(0.0, 0.0, -1.0), p3(10.0, 0.0, 5.0)),
    ] {
        let half = face_at(k, body, at)?;
        let spec = DraftSpec {
            faces: vec![half],
            angle: 3f64.to_radians(),
            pull: unit(pull),
            neutral: parting,
            neutral_normal: None,
            tangent_propagation: false,
        };
        body = first(k.draft(body, &spec)?)?;
    }
    Ok(body)
}
