//! Sweeps made one after another and joined end to end ([`crate::Kernel::sweep_chain`]): a
//! herringbone gear's two halves, each twisting its own way.

use super::path::{glue, sweep_law_tagged};
use crate::native::features::{Tagged, tagged_union};
use crate::native::kernel::NativeKernel;
use crate::*;

/// Each piece swept by its law ([`super::path::sweep_with_law`]) and joined to the one before:
/// where its start cap is the other's end cap the two caps go and their edges become one (no
/// boolean); pieces that meet any other way are united.
pub fn sweep_chain(
    k: &mut NativeKernel,
    pieces: &[(Profile, SweepSpec, SweepLaw)],
) -> Result<OpResult> {
    let mut joined: Option<Tagged> = None;
    for (profile, spec, law) in pieces {
        let piece = sweep_law_tagged(k, profile, spec, law)?;
        joined = Some(match joined {
            None => piece,
            Some(so_far) => match glue(&so_far, &piece) {
                Some(g) => g,
                None => tagged_union(&so_far, &piece)?,
            },
        });
    }
    let body = joined.ok_or_else(|| KernelError::InvalidParameter("Nothing to sweep".into()))?;
    Ok(body.into_result(k))
}
