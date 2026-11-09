//! The solid a regular tetrahedron (corners (5, 1, 1), (5, −1, −1), (3, 1, −1), (3, −1, 1))
//! fills as it is screwed about the z axis, rising 5 a turn, from a turn of 0° to 540°.
//!
//! The tetrahedron is the cube from (3, −1, −1) to (5, 1, 1) less what lies beyond each of its
//! faces' planes (a large block placed on the far side of the plane through three corners, cadrs's
//! affine transform of a box, cut away); its sweep is cadrs's sweep of a solid (a turn and a slide
//! along its axis at once: a screw).
use crate::Refusal;
use crate::bench::{block, cut, p3};
use crate::shared::sweep::first;
use crate::shared::swept::{screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let corners = [
        p3(5.0, 1.0, 1.0),
        p3(5.0, -1.0, -1.0),
        p3(3.0, 1.0, -1.0),
        p3(3.0, -1.0, 1.0),
    ];
    let centre = p3(4.0, 0.0, 0.0);
    let mut solid = block(k, p3(3.0, -1.0, -1.0), p3(5.0, 1.0, 1.0))?;
    for skip in 0..4 {
        let t: Vec<_> = (0..4).filter(|&i| i != skip).map(|i| corners[i]).collect();
        // The face's outward normal (away from the centre) and two directions in its plane.
        let mut n = (t[1] - t[0]).cross(&(t[2] - t[0])).normalize();
        if n.dot(&(centre - t[0])) > 0.0 {
            n = -n;
        }
        let x = (t[1] - t[0]).normalize();
        let y = n.cross(&x);
        // The block from (−10, −10, 0) to (10, 10, 10) in that frame: wholly beyond the plane.
        let unit = block(k, p3(-10.0, -10.0, 0.0), p3(10.0, 10.0, 10.0))?;
        let m = nalgebra::Matrix3::from_columns(&[x, y, n]);
        let beyond = first(k.transform_affine(unit, m, t[0].coords)?)?;
        solid = cut(k, solid, beyond)?;
    }
    screwed(k, solid, z_axis(), 5.0, 0.0, 540.0)
}
