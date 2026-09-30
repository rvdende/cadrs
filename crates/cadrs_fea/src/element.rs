//! The 10-node (quadratic) tetrahedron for isotropic linear elasticity.
//!
//! Shape functions in volume coordinates L₁…L₄ (straight edges, so the map is affine and the
//! gradients ∇Lₖ are constant): corner i, Nᵢ = Lᵢ(2Lᵢ − 1); mid-edge (i, j), Nᵢⱼ = 4LᵢLⱼ. Their
//! gradients are linear, so the stiffness integrand is quadratic and the 4-point Gauss rule
//! (a = 0.585410196624969, b = 0.138196601125011, weight V/4 each) integrates it exactly.
//!
//! Units: lengths in mm, forces in N, so moduli and stresses are in MPa (N/mm²).

use crate::geom::{V3, cross, dot, sub};
use crate::mesh::TET_EDGES;

/// Lamé's constants from Young's modulus and Poisson's ratio.
pub fn lame(e: f64, nu: f64) -> (f64, f64) {
    let lambda = e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let mu = e / (2.0 * (1.0 + nu));
    (lambda, mu)
}

/// ∇L₁…∇L₄ and the volume of the tetrahedron (a, b, c, d).
pub fn grad_l(x: &[V3; 4]) -> ([V3; 4], f64) {
    let e1 = sub(x[1], x[0]);
    let e2 = sub(x[2], x[0]);
    let e3 = sub(x[3], x[0]);
    let det = dot(e1, cross(e2, e3));
    // The rows of J⁻¹ (J = [e1 e2 e3] as columns) are the gradients of ξ, η, ζ = L₂, L₃, L₄.
    let g2 = crate::geom::scale(cross(e2, e3), 1.0 / det);
    let g3 = crate::geom::scale(cross(e3, e1), 1.0 / det);
    let g4 = crate::geom::scale(cross(e1, e2), 1.0 / det);
    let g1 = [-(g2[0] + g3[0] + g4[0]), -(g2[1] + g3[1] + g4[1]), -(g2[2] + g3[2] + g4[2])];
    ([g1, g2, g3, g4], det / 6.0)
}

/// The gradients of the ten shape functions at volume coordinates `l`.
pub fn grad_n(gl: &[V3; 4], l: [f64; 4]) -> [V3; 10] {
    let mut g = [[0.0; 3]; 10];
    for i in 0..4 {
        let s = 4.0 * l[i] - 1.0;
        g[i] = [gl[i][0] * s, gl[i][1] * s, gl[i][2] * s];
    }
    for (k, &(i, j)) in TET_EDGES.iter().enumerate() {
        for c in 0..3 {
            g[4 + k][c] = 4.0 * (l[j] * gl[i][c] + l[i] * gl[j][c]);
        }
    }
    g
}

/// The shape functions' values at volume coordinates `l`.
pub fn shape(l: [f64; 4]) -> [f64; 10] {
    let mut n = [0.0; 10];
    for i in 0..4 {
        n[i] = l[i] * (2.0 * l[i] - 1.0);
    }
    for (k, &(i, j)) in TET_EDGES.iter().enumerate() {
        n[4 + k] = 4.0 * l[i] * l[j];
    }
    n
}

const GA: f64 = 0.585_410_196_624_968_5;
const GB: f64 = 0.138_196_601_125_010_5;
/// The 4-point rule's points (volume coordinates).
pub const GAUSS4: [[f64; 4]; 4] = [[GA, GB, GB, GB], [GB, GA, GB, GB], [GB, GB, GA, GB], [GB, GB, GB, GA]];

/// The natural coordinates of the ten nodes.
pub const NODE_L: [[f64; 4]; 10] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
    [0.5, 0.5, 0.0, 0.0],
    [0.0, 0.5, 0.5, 0.0],
    [0.5, 0.0, 0.5, 0.0],
    [0.5, 0.0, 0.0, 0.5],
    [0.0, 0.5, 0.0, 0.5],
    [0.0, 0.0, 0.5, 0.5],
];

/// The 30×30 element stiffness (row-major, dof 3·node + axis):
/// K_ab(i, j) = ∫ λ ∂ᵢNₐ ∂ⱼN_b + μ ∂ⱼNₐ ∂ᵢN_b + μ δᵢⱼ ∇Nₐ·∇N_b dV.
pub fn stiffness(x: &[V3; 4], lambda: f64, mu: f64) -> Box<[f64; 900]> {
    let (gl, vol) = grad_l(x);
    let w = vol / 4.0;
    let mut k = Box::new([0.0; 900]);
    for l in GAUSS4 {
        let g = grad_n(&gl, l);
        for a in 0..10 {
            for b in 0..10 {
                let ga = g[a];
                let gb = g[b];
                let d = mu * dot(ga, gb);
                for i in 0..3 {
                    let row = (3 * a + i) * 30 + 3 * b;
                    for j in 0..3 {
                        let mut v = lambda * ga[i] * gb[j] + mu * ga[j] * gb[i];
                        if i == j {
                            v += d;
                        }
                        k[row + j] += w * v;
                    }
                }
            }
        }
    }
    k
}

/// The strain (xx, yy, zz, 2xy, 2yz, 2zx engineering shears) at volume coordinates `l` for
/// nodal displacements `u`.
pub fn strain(gl: &[V3; 4], l: [f64; 4], u: &[V3; 10]) -> [f64; 6] {
    let g = grad_n(gl, l);
    // Displacement gradient H[i][j] = ∂uᵢ/∂xⱼ.
    let mut h = [[0.0; 3]; 3];
    for a in 0..10 {
        for i in 0..3 {
            for j in 0..3 {
                h[i][j] += u[a][i] * g[a][j];
            }
        }
    }
    [h[0][0], h[1][1], h[2][2], h[0][1] + h[1][0], h[1][2] + h[2][1], h[2][0] + h[0][2]]
}

/// Stress (xx, yy, zz, xy, yz, zx) from engineering strain.
pub fn stress(eps: [f64; 6], lambda: f64, mu: f64) -> [f64; 6] {
    let tr = eps[0] + eps[1] + eps[2];
    [
        lambda * tr + 2.0 * mu * eps[0],
        lambda * tr + 2.0 * mu * eps[1],
        lambda * tr + 2.0 * mu * eps[2],
        mu * eps[3],
        mu * eps[4],
        mu * eps[5],
    ]
}

/// The von Mises equivalent stress.
pub fn von_mises(s: [f64; 6]) -> f64 {
    let [xx, yy, zz, xy, yz, zx] = s;
    (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2)) + 3.0 * (xy * xy + yz * yz + zx * zx)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: [V3; 4] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn shape_functions_partition_unity_and_are_nodal() {
        for (k, l) in NODE_L.iter().enumerate() {
            let n = shape(*l);
            for (j, v) in n.iter().enumerate() {
                assert!((v - if j == k { 1.0 } else { 0.0 }).abs() < 1e-15);
            }
        }
        let n = shape([0.1, 0.2, 0.3, 0.4]);
        assert!((n.iter().sum::<f64>() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn rigid_motions_are_stress_free_and_the_stiffness_is_symmetric() {
        let x = [[0.0, 0.0, 0.0], [2.0, 0.1, 0.0], [0.3, 1.5, 0.2], [0.1, 0.4, 1.7]];
        let (lambda, mu) = lame(200_000.0, 0.3);
        let k = stiffness(&x, lambda, mu);
        for a in 0..30 {
            for b in 0..30 {
                assert!((k[a * 30 + b] - k[b * 30 + a]).abs() < 1e-6 * k[a * 30 + a].abs());
            }
        }
        // K times a translation or a small rotation (u = ω × x) is zero.
        let nodes: Vec<V3> = (0..10)
            .map(|n| {
                let l = NODE_L[n];
                (0..4).fold([0.0; 3], |acc, i| crate::geom::add(acc, crate::geom::scale(x[i], l[i])))
            })
            .collect();
        for motion in [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]].iter().map(|t| nodes.iter().map(|_| *t).collect::<Vec<V3>>()).chain(std::iter::once(
            nodes.iter().map(|p| cross([0.3, -0.2, 0.5], *p)).collect::<Vec<V3>>(),
        )) {
            for a in 0..30 {
                let f: f64 = (0..30).map(|b| k[a * 30 + b] * motion[b / 3][b % 3]).sum();
                assert!(f.abs() < 1e-6 * k[a * 30 + a], "row {a}: {f}");
            }
        }
    }

    #[test]
    fn a_linear_field_gives_its_exact_strain() {
        // u = (0.001 x, −0.0003 y, 0.002 z + 0.001 x): quadratic elements reproduce it exactly.
        let (gl, v) = grad_l(&UNIT);
        assert!((v - 1.0 / 6.0).abs() < 1e-15);
        let u: [V3; 10] = std::array::from_fn(|n| {
            let l = NODE_L[n];
            let p = (0..4).fold([0.0; 3], |acc, i| crate::geom::add(acc, crate::geom::scale(UNIT[i], l[i])));
            [0.001 * p[0], -0.0003 * p[1], 0.002 * p[2] + 0.001 * p[0]]
        });
        let e = strain(&gl, [0.25; 4], &u);
        let want = [0.001, -0.0003, 0.002, 0.0, 0.0, 0.001];
        for k in 0..6 {
            assert!((e[k] - want[k]).abs() < 1e-15, "{e:?}");
        }
        // Uniaxial stress σ in x: von Mises = |σ|.
        assert!((von_mises([30.0, 0.0, 0.0, 0.0, 0.0, 0.0]) - 30.0).abs() < 1e-12);
    }
}
