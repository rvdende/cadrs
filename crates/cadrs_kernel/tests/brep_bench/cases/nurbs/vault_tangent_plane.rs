//! A block with a barrel-vaulted top, from which everything above the plane z = 16 is cut
//! away: that plane touches the vault along its crown, so the cut removes nothing.
//!
//! The block stands on the square −12 ≤ x, y ≤ 12 from the floor z = 0; its top is the rational
//! B-spline surface of degree 1 along x and 2 along y, knots (0, 0, 1, 1) and
//! (0, 0, 0, 1, 1, 1), whose rows are the rational quadratic arcs through (x, −12, 10),
//! (x, 0, 26) and (x, 12, 10) with weights 1, 3/5 and 1, for x = −12 and 12. It is the square
//! extruded 10, its flat top replaced by that surface (Replace face). It is cut with a box whose
//! floor is the plane z = 16 and which reaches past the block on every other side.
use crate::Refusal;
use crate::bench::{block, cut, p3};
use crate::shared::nurbs::{barrel_vault_surface, domed_block};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let top = barrel_vault_surface()?;
    let vault = domed_block(k, (-12.0, -12.0), (12.0, 12.0), 10.0, top)?;
    let lid = block(k, p3(-20.0, -20.0, 16.0), p3(20.0, 20.0, 30.0))?;
    cut(k, vault, lid)
}
