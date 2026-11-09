//! Martin Newell's teapot made a solid: the 32 bicubic Bézier patches of the usual data set
//! (Newell's 28 and Frank Crow's 4 of the bottom), in Newell's coordinates, z up, the union of four
//! solids each bounded by some of the patches and closed by flat faces: the body (rim, upper and
//! lower body, bottom; the disk inside the rim's inner edge in z = 2.4), the lid (knob and lid; the
//! disk inside the lid's edge in z = 2.4), the spout (its base in x = 1.7, its mouth in z = 2.4
//! inside the lip) and the handle (each end's loop, in its plane).
//!
//! Each patch is cadrs's face on a B-spline surface given by its net (`Kernel::bspline_face`: a
//! Bézier patch is the B-spline of one span), whose rows on the axis collapse to poles; each flat
//! face is cadrs's Fill of its loop of cubic Bézier curves, sketched on its plane (`Kernel::fill`);
//! each solid its faces sewn (`Kernel::sew_solid`); the teapot the four united.
use crate::Refusal;
use crate::bench::{P3, V3, union};
use crate::shared::sweep::first;
use cadrs_kernel::{
    BodyId, BsplineSurface, Curve2, DefaultKernel, FillCurve, FillSpec, Kernel, Plane,
};
use nalgebra::{Point2, Unit};

type Row = [[f64; 3]; 4];
type Patch = [Row; 4];

/// The rings' profiles (r, z).
const RIM: [[f64; 2]; 4] = [[1.4, 2.4], [1.3375, 2.53125], [1.4375, 2.53125], [1.5, 2.4]];
const UPPER_BODY: [[f64; 2]; 4] = [[1.5, 2.4], [1.75, 1.875], [2.0, 1.35], [2.0, 0.9]];
const LOWER_BODY: [[f64; 2]; 4] = [[2.0, 0.9], [2.0, 0.45], [1.5, 0.225], [1.5, 0.15]];
const BOTTOM: [[f64; 2]; 4] = [[1.5, 0.15], [1.5, 0.075], [1.425, 0.0], [0.0, 0.0]];
const KNOB: [[f64; 2]; 4] = [[0.0, 3.15], [0.8, 3.15], [0.0, 2.85], [0.2, 2.7]];
const LID: [[f64; 2]; 4] = [[0.2, 2.7], [0.4, 2.55], [1.3, 2.55], [1.3, 2.4]];

/// The spout's and the handle's patches on the side y ≤ 0.
const SPOUT: [Patch; 2] = [
    [
        [
            [1.7, 0.0, 1.425],
            [1.7, -0.66, 1.425],
            [1.7, -0.66, 0.6],
            [1.7, 0.0, 0.6],
        ],
        [
            [2.6, 0.0, 1.425],
            [2.6, -0.66, 1.425],
            [3.1, -0.66, 0.825],
            [3.1, 0.0, 0.825],
        ],
        [
            [2.3, 0.0, 2.1],
            [2.3, -0.25, 2.1],
            [2.4, -0.25, 2.025],
            [2.4, 0.0, 2.025],
        ],
        [
            [2.7, 0.0, 2.4],
            [2.7, -0.25, 2.4],
            [3.3, -0.25, 2.4],
            [3.3, 0.0, 2.4],
        ],
    ],
    [
        [
            [2.7, 0.0, 2.4],
            [2.7, -0.25, 2.4],
            [3.3, -0.25, 2.4],
            [3.3, 0.0, 2.4],
        ],
        [
            [2.8, 0.0, 2.475],
            [2.8, -0.25, 2.475],
            [3.525, -0.25, 2.49375],
            [3.525, 0.0, 2.49375],
        ],
        [
            [2.9, 0.0, 2.475],
            [2.9, -0.15, 2.475],
            [3.45, -0.15, 2.5125],
            [3.45, 0.0, 2.5125],
        ],
        [
            [2.8, 0.0, 2.4],
            [2.8, -0.15, 2.4],
            [3.2, -0.15, 2.4],
            [3.2, 0.0, 2.4],
        ],
    ],
];
const HANDLE: [Patch; 2] = [
    [
        [
            [-1.6, 0.0, 2.025],
            [-1.6, -0.3, 2.025],
            [-1.5, -0.3, 2.25],
            [-1.5, 0.0, 2.25],
        ],
        [
            [-2.3, 0.0, 2.025],
            [-2.3, -0.3, 2.025],
            [-2.5, -0.3, 2.25],
            [-2.5, 0.0, 2.25],
        ],
        [
            [-2.7, 0.0, 2.025],
            [-2.7, -0.3, 2.025],
            [-3.0, -0.3, 2.25],
            [-3.0, 0.0, 2.25],
        ],
        [
            [-2.7, 0.0, 1.8],
            [-2.7, -0.3, 1.8],
            [-3.0, -0.3, 1.8],
            [-3.0, 0.0, 1.8],
        ],
    ],
    [
        [
            [-2.7, 0.0, 1.8],
            [-2.7, -0.3, 1.8],
            [-3.0, -0.3, 1.8],
            [-3.0, 0.0, 1.8],
        ],
        [
            [-2.7, 0.0, 1.575],
            [-2.7, -0.3, 1.575],
            [-3.0, -0.3, 1.35],
            [-3.0, 0.0, 1.35],
        ],
        [
            [-2.5, 0.0, 1.125],
            [-2.5, -0.3, 1.125],
            [-2.65, -0.3, 0.9375],
            [-2.65, 0.0, 0.9375],
        ],
        [
            [-2.0, 0.0, 0.9],
            [-2.0, -0.3, 0.9],
            [-1.9, -0.3, 0.6],
            [-1.9, 0.0, 0.6],
        ],
    ],
];

/// The four patches of a ring: row i from the profile's point i, (r, 0, z), (r, −0.56r, z),
/// (0.56r, −r, z), (0, −r, z) (the knob's row 1 0.45 where the rule gives 0.448), each patch a
/// quarter turn on from +x toward −y.
fn ring(profile: &[[f64; 2]; 4], knob: bool) -> Vec<Patch> {
    let first: Patch = std::array::from_fn(|i| {
        let [r, z] = profile[i];
        let k = if knob && i == 1 { 0.5625 } else { 0.56 };
        [[r, 0.0, z], [r, -k * r, z], [k * r, -r, z], [0.0, -r, z]]
    });
    let turn = |[x, y, z]: [f64; 3], q: usize| (0..q).fold([x, y, z], |[x, y, z], _| [y, -x, z]);
    (0..4)
        .map(|q| first.map(|row| row.map(|p| turn(p, q))))
        .collect()
}

/// A patch mirrored in the plane y = 0, each row reversed so that it runs on from the patch.
fn mirrored(p: &Patch) -> Patch {
    p.map(|row| {
        std::array::from_fn(|j| {
            let [x, y, z] = row[3 - j];
            [x, -y, z]
        })
    })
}

fn pt([x, y, z]: [f64; 3]) -> P3 {
    P3::new(x, y, z)
}

/// The face on a Bézier patch.
fn patch(k: &mut DefaultKernel, p: &Patch) -> Result<BodyId, Refusal> {
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let surface = BsplineSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.clone(),
        knots_v: knots,
        poles: p
            .iter()
            .map(|row| row.iter().map(|&q| pt(q)).collect())
            .collect(),
        weights: Vec::new(),
    };
    first(k.bspline_face(&surface, None)?)
}

/// The flat face inside the closed loop of cubic Bézier curves `rows`, on the plane through the
/// first row's start square to `normal`.
fn cap(k: &mut DefaultKernel, rows: &[Row], normal: V3) -> Result<BodyId, Refusal> {
    let o = pt(rows[0][0]);
    let n = normal.normalize();
    let x = (pt(rows[0][3]) - o).normalize();
    let x = (x - n * x.dot(&n)).normalize();
    let y = n.cross(&x);
    let plane = Plane {
        origin: o,
        x_dir: Unit::new_normalize(x),
        normal: Unit::new_normalize(n),
    };
    let at = |q: [f64; 3]| {
        let d = pt(q) - o;
        Point2::new(d.dot(&x), d.dot(&y))
    };
    let curves = rows
        .iter()
        .map(|r| FillCurve::Sketch {
            plane,
            curve: Curve2::Bezier {
                poles: r.map(at),
                source: None,
            },
        })
        .collect();
    first(k.fill(&FillSpec { curves, source: 1 })?)
}

/// The solid `patches` and `caps` bound.
fn solid(k: &mut DefaultKernel, patches: &[Patch], caps: Vec<BodyId>) -> Result<BodyId, Refusal> {
    let mut faces = patches
        .iter()
        .map(|p| patch(k, p))
        .collect::<Result<Vec<_>, _>>()?;
    faces.extend(caps);
    first(k.sew_solid(&faces, 1e-6)?)
}

/// The normal of the plane of a row (through its ends and its second control point).
fn normal(row: &Row) -> V3 {
    (pt(row[1]) - pt(row[0])).cross(&(pt(row[3]) - pt(row[0])))
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let up = V3::new(0.0, 0.0, 1.0);
    let rim = ring(&RIM, false);
    let lid_ring = ring(&LID, false);
    let body_patches = [
        rim.clone(),
        ring(&UPPER_BODY, false),
        ring(&LOWER_BODY, false),
        ring(&BOTTOM, false),
    ]
    .concat();
    let body_cap = cap(k, &rim.iter().map(|p| p[0]).collect::<Vec<_>>(), up)?;
    let body = solid(k, &body_patches, vec![body_cap])?;
    let lid_patches = [ring(&KNOB, true), lid_ring.clone()].concat();
    let lid_cap = cap(k, &lid_ring.iter().map(|p| p[3]).collect::<Vec<_>>(), up)?;
    let lid = solid(k, &lid_patches, vec![lid_cap])?;
    let mut parts = Vec::new();
    for h in [&SPOUT, &HANDLE] {
        let halves = [h[0], h[1], mirrored(&h[0]), mirrored(&h[1])];
        let (start, end) = ([h[0][0], mirrored(&h[0])[0]], [h[1][3], mirrored(&h[1])[3]]);
        let caps = vec![
            cap(k, &start, normal(&h[0][0]))?,
            cap(k, &end, normal(&h[1][3]))?,
        ];
        parts.push(solid(k, &halves, caps)?);
    }
    let mut teapot = union(k, body, lid)?;
    for part in parts {
        teapot = union(k, teapot, part)?;
    }
    Ok(teapot)
}
