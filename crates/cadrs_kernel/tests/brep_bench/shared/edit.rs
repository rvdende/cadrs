//! What the cadrs implementations of the edit cases share: Move face, by a rigid motion or by an
//! offset along the faces' normals.
use crate::Refusal;
use crate::bench::V3;
use cadrs_kernel::{BodyId, DefaultKernel, FaceId, Kernel, Motion, OffsetSpec};

/// `body` with `faces` moved by the translation `v` (Move face, Translate), the faces round them
/// extended or trimmed to meet them.
pub fn move_faces(
    k: &mut DefaultKernel,
    body: BodyId,
    faces: &[FaceId],
    v: V3,
) -> Result<BodyId, Refusal> {
    let r = k.move_faces(body, faces, &Motion::translation(v))?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("Move face made no body"))
}

/// `body` with `face` moved `distance` along its outward normal (Move face, Offset; negative
/// moves it into the material), the other faces kept and trimmed to meet it.
pub fn offset_face(
    k: &mut DefaultKernel,
    body: BodyId,
    face: FaceId,
    distance: f64,
) -> Result<BodyId, Refusal> {
    let spec = OffsetSpec {
        distance: 0.0,
        faces: vec![(face, distance)],
        sharp: true,
    };
    let r = k.offset(body, &spec)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the offset made no body"))
}

/// `body` with each of `faces` moved `distance` along its outward normal at once (Move face,
/// Offset; negative moves them into the material), the other faces kept and trimmed to meet them.
pub fn offset_faces(
    k: &mut DefaultKernel,
    body: BodyId,
    faces: &[FaceId],
    distance: f64,
) -> Result<BodyId, Refusal> {
    let spec = OffsetSpec {
        distance: 0.0,
        faces: faces.iter().map(|&f| (f, distance)).collect(),
        sharp: true,
    };
    let r = k.offset(body, &spec)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the offset made no body"))
}
