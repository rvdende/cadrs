//! What the cadrs implementations of the chamfer cases share: the Chamfer feature on picked
//! edges, and the octahedron.
use crate::Refusal;
use crate::bench::{P3, V3, cut, extrude, p3, polygon, round, sketch, square_to, union, xy};
use cadrs_kernel::{
    BodyId, BodyKind, ChamferOpts, ChamferSpec, DefaultKernel, EdgeId, Kernel, LoftEnd,
    LoftSection, LoftSpec,
};

/// `body` with `edges` chamfered by `spec` in one Chamfer feature.
pub fn chamfer(
    k: &mut DefaultKernel,
    body: BodyId,
    edges: &[EdgeId],
    spec: ChamferSpec,
) -> Result<BodyId, Refusal> {
    chamfer_with(k, body, edges, &ChamferOpts::new(spec))
}

/// `body` with `edges` chamfered with the Chamfer dialog's options.
pub fn chamfer_with(
    k: &mut DefaultKernel,
    body: BodyId,
    edges: &[EdgeId],
    opts: &ChamferOpts,
) -> Result<BodyId, Refusal> {
    let r = k.chamfer_with(body, edges, opts)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the chamfer made no body"))
}

/// The octahedron |x| + |y| + |z| ≤ `a`: the square through its four corners in the plane z = 0
/// lofted to a point at each of its other two corners, (0, 0, a) and (0, 0, −a), the two
/// pyramids united.
pub fn octahedron(k: &mut DefaultKernel, a: f64) -> Result<BodyId, Refusal> {
    let mut pyramid = |tip: f64| -> Result<BodyId, Refusal> {
        let square = sketch(
            xy(0.0),
            vec![polygon(&[(a, 0.0), (0.0, a), (-a, 0.0), (0.0, -a)])],
        );
        let spec = LoftSpec {
            body: BodyKind::Solid,
            sections: vec![
                LoftSection::Profile(square),
                LoftSection::Point(p3(0.0, 0.0, tip)),
            ],
            start: LoftEnd::default(),
            end: LoftEnd::default(),
            source: 1,
        };
        let r = k.loft_with(&spec)?;
        r.bodies
            .first()
            .copied()
            .ok_or_else(|| Refusal::from("the loft made no body"))
    };
    let upper = pyramid(a)?;
    let lower = pyramid(-a)?;
    union(k, upper, lower)
}
