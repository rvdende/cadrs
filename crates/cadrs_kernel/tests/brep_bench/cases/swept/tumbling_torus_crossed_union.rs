//! The regularized union of the two crossed tumbling tori (`swept/tumbling-torus-crossed-forward`
//! and `-reverse`): each swept on its own (cadrs's sweep of a solid), then cadrs's boolean.
use crate::Refusal;
use crate::shared::swept::crossed_tori;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let (a, b) = crossed_tori(k)?;
    crate::bench::union(k, a, b)
}
