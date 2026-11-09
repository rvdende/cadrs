//!
//! The octahedron is two square pyramids, each the square through its corners in z = 0 lofted
//! to a point, united; its twelve edges are chamfered in one Chamfer feature, equal distance √6,
//! its corners flat (`ChamferCorners::Flat`: each cut by the plane through the four points where
//! the chamfers' lines on its faces meet).
use crate::Refusal;
use crate::bench::all_edges;
use crate::shared::chamfer::{chamfer_with, octahedron};
use cadrs_kernel::{BodyId, ChamferCorners, ChamferOpts, ChamferSpec, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let o = octahedron(k, 10.0)?;
    let edges = all_edges(k, o)?;
    let opts = ChamferOpts {
        corners: ChamferCorners::Flat,
        ..ChamferOpts::new(ChamferSpec::EqualDistance(6f64.sqrt()))
    };
    chamfer_with(k, o, &edges, &opts)
}
