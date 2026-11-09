//! What the cadrs implementations of the shell cases share: cadrs's Shell feature
//! (`Kernel::shell_with`, Onshape's dialog), its openings picked as a user picks faces in the
//! viewport, by a point on each.
use crate::Refusal;
use crate::bench::{P3, face_at};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, ShellSpec};

/// How the walls of a shell are joined.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Joins {
    /// Rounded (cadrs's default, OCCT's arc join): outward, a cylinder round each convex edge
    /// and a ball at each corner; inward, the cavity rounded at the body's concave edges.
    Rounded,
    /// Sharp: the moved faces extended until they meet.
    Sharp,
}

/// The body that `spec` makes of `body`: one body, the result's first.
fn run(k: &mut DefaultKernel, body: BodyId, spec: &ShellSpec) -> Result<BodyId, Refusal> {
    let r = k.shell_with(body, spec)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the shell made no body"))
}

/// `body` shelled inward to walls `thickness` thick, open at the faces through `openings`.
pub fn shell_inward(
    k: &mut DefaultKernel,
    body: BodyId,
    openings: &[P3],
    thickness: f64,
    joins: Joins,
) -> Result<BodyId, Refusal> {
    let remove = openings
        .iter()
        .map(|&p| face_at(k, body, p))
        .collect::<Result<_, _>>()?;
    let spec = ShellSpec {
        remove,
        thickness,
        outward: false,
        hollow: false,
        sharp: joins == Joins::Sharp,
    };
    run(k, body, &spec)
}

/// `body` shelled outward to walls `thickness` thick, open at the faces through `openings`.
pub fn shell_outward(
    k: &mut DefaultKernel,
    body: BodyId,
    openings: &[P3],
    thickness: f64,
    joins: Joins,
) -> Result<BodyId, Refusal> {
    let remove = openings
        .iter()
        .map(|&p| face_at(k, body, p))
        .collect::<Result<_, _>>()?;
    let spec = ShellSpec {
        remove,
        thickness,
        outward: true,
        hollow: false,
        sharp: joins == Joins::Sharp,
    };
    run(k, body, &spec)
}

/// `body` hollowed to walls `thickness` thick with no opening: the inside a closed void.
pub fn hollow(k: &mut DefaultKernel, body: BodyId, thickness: f64) -> Result<BodyId, Refusal> {
    let spec = ShellSpec {
        remove: vec![],
        thickness,
        outward: false,
        hollow: true,
        sharp: false,
    };
    run(k, body, &spec)
}
