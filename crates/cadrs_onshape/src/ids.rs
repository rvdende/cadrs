//! Stable cadrs ids derived from Onshape ids, so importing a document again replaces the
//! earlier import (same document id, same feature ids) instead of adding a copy.

use cadrs_kernel::naming::stable_hash;

/// A UUID-sized value from `parts` (two independent 64-bit hashes).
pub fn stable_u128(parts: &[&str]) -> u128 {
    let joined = parts.join("\u{1f}");
    let hi = stable_hash(format!("cadrs-onshape/hi/{joined}").as_bytes());
    let lo = stable_hash(format!("cadrs-onshape/lo/{joined}").as_bytes());
    // Version 8 (custom) UUID layout, so these never collide with random v4 ids.
    let v = (u128::from(hi) << 64) | u128::from(lo);
    (v & !(0xf << 76) & !(0x3 << 62)) | (0x8 << 76) | (0x2 << 62)
}
