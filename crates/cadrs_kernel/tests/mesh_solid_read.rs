//! A solid read from triangles whose coplanar ones are not quite (a file's tessellation, its
//! coordinates to 6 digits): the planar faces' corners stray from their planes, and the
//! edges' and vertices' tolerances hold that.

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::Point3;

#[test]
fn a_mesh_whose_flat_faces_are_nearly_flat_checks_clean() {
    // A 10 mm cube, two triangles a side; the top's corners up to 2.2e-5 off z = 10 (twisted by 2e-5).
    let top = [0.0, 3e-6, -1e-6, 2.2e-5];
    let p = |i: usize| {
        let (x, y, z) = ((i & 1) as f64, ((i >> 1) & 1) as f64, (i >> 2) as f64);
        Point3::new(
            10.0 * x,
            10.0 * y,
            10.0 * z + if z > 0.0 { top[i & 3] } else { 0.0 },
        )
    };
    // Quads (corner indices, x = bit 0, y = bit 1, z = bit 2), each wound outward.
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let tris: Vec<[Point3<f64>; 3]> = quads
        .iter()
        .flat_map(|q| [[p(q[0]), p(q[1]), p(q[2])], [p(q[0]), p(q[2]), p(q[3])]])
        .collect();
    let mut k = NativeKernel::new();
    let id = k.mesh_solid(&tris, 1e-4).unwrap();
    let b = k.get(id).unwrap();
    assert_eq!(b.faces.len(), 6);
    let issues = b.check();
    assert!(issues.is_empty(), "{issues:?}");
}
