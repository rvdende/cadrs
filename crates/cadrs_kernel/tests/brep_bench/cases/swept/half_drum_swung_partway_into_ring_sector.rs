//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by half a drum (radius 1 about the upright through (10, 0), from z = −0.5 to 0.5, the
//! half with y ≤ 0) turned about the z axis from −140° to +30°, flat face first.
//!
//! The half drum is the half disc sketched and extruded; what it passes through is cadrs's sweep
//! of a solid (one turn), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{arc, chain, cut, extrude, line, p2, sketch, xy};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let half = chain(vec![
        arc(p2(10.0, 0.0), 1.0, 180.0, 180.0),
        line(p2(11.0, 0.0), p2(9.0, 0.0)),
    ]);
    let tool = extrude(k, &sketch(xy(-0.5), vec![half]), 1.0)?;
    let swept = swung(k, tool, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}
