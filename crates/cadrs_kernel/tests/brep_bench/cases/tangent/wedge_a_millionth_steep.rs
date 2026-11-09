//! A cube from (−5, −5, −10) to (5, 5, 0) with everything below the plane z = y·10⁻⁶ cut away by a
//! block on that plane: the wedge under the half of the top face where y < 0, 5·10⁻⁶ thick at
//! its end.
use crate::Refusal;
use crate::bench::{block, p3, v3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(-5.0, -5.0, -10.0), p3(5.0, 5.0, 0.0))?;
    cut_beyond(k, cube, p3(0.0, 0.0, 0.0), v3(0.0, 1e-6, -1.0))
}
