//! What the cadrs implementations of the loft cases share: cadrs's solid Loft through sketched
//! sections, and the pieces of a loft cadrs's solid Loft cannot make (it takes no guides, and
//! pairs its sections' curves itself), built as Onshape's surfacing would: each wall a surface
//! Loft through curves (with guides, or to a point), each flat end a Fill, sewn into a solid.
use crate::Refusal;
use crate::bench::{line, p2, xy};
use crate::shared::sweep::first;
use cadrs_kernel::{
    BodyId, BodyKind, CurveLoftEnd, CurveLoftSpec, CurveSection, DefaultKernel, FillCurve,
    FillSpec, Kernel, LoftEnd, LoftSection, LoftSpec, PathCurve, Profile,
};

/// The solid lofted through `sections` in order, with no end conditions.
pub fn loft(k: &mut DefaultKernel, sections: Vec<Profile>) -> Result<BodyId, Refusal> {
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: sections.into_iter().map(LoftSection::Profile).collect(),
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    first(k.loft_with(&spec)?)
}

/// The sheet lofted through the curves `sections` in order (each a wire, or a point at either
/// end) along `guides`.
pub fn surface_loft(
    k: &mut DefaultKernel,
    sections: Vec<CurveSection>,
    guides: Vec<Vec<PathCurve>>,
) -> Result<BodyId, Refusal> {
    let spec = CurveLoftSpec {
        sections,
        guides,
        start: CurveLoftEnd::default(),
        end: CurveLoftEnd::default(),
        source: 1,
    };
    first(k.loft_curves(&spec)?)
}

/// The flat sheet filling the polygon through `corners` on the plane z = `z` (its Fill).
pub fn flat(k: &mut DefaultKernel, z: f64, corners: &[(f64, f64)]) -> Result<BodyId, Refusal> {
    let n = corners.len();
    let curves = (0..n)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % n]);
            FillCurve::Sketch {
                plane: xy(z),
                curve: line(p2(a.0, a.1), p2(b.0, b.1)),
            }
        })
        .collect();
    first(k.fill(&FillSpec { curves, source: 1 })?)
}

/// The solid the sheets `pieces` close, sewn along their edges.
pub fn sewn(k: &mut DefaultKernel, pieces: &[BodyId]) -> Result<BodyId, Refusal> {
    first(k.sew_solid(pieces, 1e-6)?)
}

/// The solid lofted through `sections` (each a sketch region, or a point at either end) along
/// `guides`, each from a corner of the first section to a corner of the last (cadrs's Loft with
/// guides).
pub fn guided_loft(
    k: &mut DefaultKernel,
    sections: Vec<LoftSection>,
    guides: Vec<Vec<PathCurve>>,
) -> Result<BodyId, Refusal> {
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections,
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    first(k.loft_guided(&spec, &guides)?)
}
