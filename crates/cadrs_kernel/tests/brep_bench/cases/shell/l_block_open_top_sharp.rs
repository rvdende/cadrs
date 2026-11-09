//! An L-shaped plate shelled with its top open, the wall at its inside corner kept sharp. The outline
//! through (0, 0), (20, 0), (20, 5), (5, 5), (5, 15) and (0, 15), extruded 8 along +z, shelled inward
//! to a wall 1 thick, open at its top, every pair of moved faces extended until they meet, the
//! concave edge's included: the cavity is the polygon through (1, 1), (19, 1), (19, 4), (4, 4),
//! (4, 14) and (1, 14), from z = 1 up.
//!
//! The plate is the outline on the Top plane extruded 8; cadrs's Shell feature (inward, sharp
//! corners), the top picked as the face to remove.
use crate::Refusal;
use crate::bench::{extrude, p3, polygon, sketch, xy};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outline = polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 5.0),
        (5.0, 5.0),
        (5.0, 15.0),
        (0.0, 15.0),
    ]);
    let plate = extrude(k, &sketch(xy(0.0), vec![outline]), 8.0)?;
    shell_inward(k, plate, &[p3(2.0, 2.0, 8.0)], 1.0, Joins::Sharp)
}
