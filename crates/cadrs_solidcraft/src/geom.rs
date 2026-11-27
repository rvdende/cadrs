//! Small vector helpers shared by the recipe translation and the oracle: 3D vectors are plain
//! `[f64; 3]`, as cadrs' feature list has them.

use serde_json::Value;

pub type V3 = [f64; 3];

pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: V3) -> V3 {
    scale(a, 1.0 / norm(a).max(1e-300))
}

/// A 3D point from a recipe's `[x, y, z]`, if it is one.
pub(crate) fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array()?;
    Some([
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ])
}
/// A 2D point from a recipe's `[x, y]`, if it is one.
pub(crate) fn v2(v: &Value) -> Option<[f64; 2]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}
