//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! grooved by a bar drafted 2° both ways from the plane of its section, turned about the z axis
//! from −140° to +140°.
//!
//! The bar's section is the rectangle 10 ≤ a ≤ 13, |b| ≤ 2 in the plane through the origin
//! spanned by u = (1, 0, 0) and v = (0, cos 20°, −sin 20°); each half of it is cadrs's solid Loft
//! from that rectangle to the one 4 along ±n (n = u × v) whose sides are drawn in by 4 tan 2°, and
//! the bar is the two united. What it passes through is cut from the block by cadrs's cut of a
//! swept solid (one turn: the block less the bar where it starts and less each of its leading
//! faces' sweeps).
use crate::Refusal;
use crate::bench::{p3, plane, rectangle, sketch, union, v3};
use crate::shared::loft::loft;
use crate::shared::swept::{cut_swept, ring_sector, swing, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let t = 20f64.to_radians();
    let (u, n) = (v3(1.0, 0.0, 0.0), v3(0.0, t.sin(), t.cos()));
    // (The sketch's y axis, n × u, is v.)
    let at = |h: f64| plane(p3(0.0, 0.0, 0.0) + n * h, u, n);
    let d = 4.0 * 2f64.to_radians().tan();
    let section = |h: f64, d: f64| {
        sketch(
            at(h),
            vec![rectangle(10.0 + d, -2.0 + d, 13.0 - d, 2.0 - d)],
        )
    };
    let up = loft(k, vec![section(0.0, 0.0), section(4.0, d)])?;
    let down = loft(k, vec![section(-4.0, d), section(0.0, 0.0)])?;
    let bar = union(k, up, down)?;
    cut_swept(k, block, bar, &swing(z_axis(), -140.0, 140.0))
}
