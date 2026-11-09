//! A ball of radius 10 about the origin and a post, a cylinder of radius 4 on the z axis from
//! z = 0 to z = 15, joined into one solid; the circle where the post leaves the ball (radius 4
//! at z = √84) rounded to radius 2, adding material: a torus about the z axis tangent to the
//! ball and to the post.
//!
//! The ball is cadrs's sphere primitive, the post a circle extruded, the two united; the circle
//! where they meet is picked by a point on it and filleted, radius 2.
use crate::Refusal;
use crate::bench::{ball, cylinder, edge_at, p3, union, v3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 10.0)?;
    let post = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 15.0)?;
    let body = union(k, b, post)?;
    let root = edge_at(k, body, p3(4.0, 0.0, 84f64.sqrt()))?;
    fillet(k, body, &[root], 2.0)
}
