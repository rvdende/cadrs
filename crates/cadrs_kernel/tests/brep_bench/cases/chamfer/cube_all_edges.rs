//!
//! The cube is a square extruded, and its twelve edges are chamfered in one Chamfer feature,
//! equal distance 2, its corners flat (`ChamferCorners::Flat`: each cut by the plane through the
//! three points where the chamfers' lines on its faces meet).
use crate::Refusal;
use crate::bench::{all_edges, block, p3};
use crate::shared::chamfer::chamfer_with;
use cadrs_kernel::{BodyId, ChamferCorners, ChamferOpts, ChamferSpec, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let edges = all_edges(k, cube)?;
    let opts = ChamferOpts {
        corners: ChamferCorners::Flat,
        ..ChamferOpts::new(ChamferSpec::EqualDistance(2.0))
    };
    chamfer_with(k, cube, &edges, &opts)
}
