//! A rod, the cylinder of radius 5 about the z axis from z = 0 to z = 20, with a helical groove
//! ground by a torus (radii 6 and 2, centered (11.5, 0, 0), its axis leaning by the helix angle)
//! carried along the screw about the z axis that rises 30 a turn, from a turn of −108° to +348°.
//!
//! The wheel is cadrs's torus primitive; its screw is cadrs's sweep of a solid over part of a
//! screw (`Kernel::cut_swept`: turned about the z axis from −108° to +348° while rising 30 a
//! turn), cut from the rod.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use crate::shared::swept::{cut_swept, rod, screw, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let h = (std::f64::consts::TAU * 5.5 / 30.0).atan();
    let wheel = torus(k, p3(11.5, 0.0, 0.0), v3(0.0, h.cos(), -h.sin()), 6.0, 2.0)?;
    let r = rod(k)?;
    cut_swept(k, r, wheel, &screw(z_axis(), 30.0, -108.0, 348.0))
}
