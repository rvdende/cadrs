//! The box from the origin to (20, 10, 10), the edge where its top meets its end x = 20 blended
//! with a conic of rho 0.5 (a parabola) tangent to both faces 3 from the edge.
//!
//! cadrs's Fillet with a conic cross section of rho 0.5, size 3: its contacts are the circular
//! fillet's of radius 3, 3 from the edge on each of two square faces.
use crate::Refusal;
use crate::bench::{block, edge_at, p3};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, FilletProfile, FilletSpec, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 10.0, 10.0))?;
    let edge = edge_at(k, b, p3(20.0, 5.0, 10.0))?;
    let spec = FilletSpec {
        profile: FilletProfile::Conic { rho: 0.5 },
        ..FilletSpec::radius(3.0)
    };
    first(k.fillet_with(b, &[edge], &spec)?)
}
