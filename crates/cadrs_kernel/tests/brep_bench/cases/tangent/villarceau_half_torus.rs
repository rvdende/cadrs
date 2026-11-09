//! A solid torus about the z axis, its tube's middle circle of radius 5 and the tube of radius
//! 3, cut by the bitangent plane 3x − 4z = 0, keeping what lies on and below it (z ≤ 3x/4).
//!
//! The torus is cadrs's torus primitive, its seam in the plane x = 0 on the side y > 0. What lies
//! above the plane is cut away with a block: a
//! square of side 40 sketched on the plane itself, extruded 20 along its normal (−3, 0, 4)/5,
//! away from the kept side, reaching past the torus.
use crate::Refusal;
use crate::bench::{cut, extrude, p3, plane, rectangle, sketch, torus, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let ring = torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 3.0)?;
    let on = plane(p3(0.0, 0.0, 0.0), v3(4.0, 0.0, 3.0), v3(-3.0, 0.0, 4.0));
    let above = extrude(
        k,
        &sketch(on, vec![rectangle(-20.0, -20.0, 20.0, 20.0)]),
        20.0,
    )?;
    cut(k, ring, above)
}
