//! Ported from springrep's tests/seams.rs (MIT, https://github.com/springcraft-ai/springrep):
//! faces whose seam does not run straight: two lengths of one cylinder joined, each with its seam
//! at its own angle, are one face round the cylinder whose seam jogs where they meet. Its outline
//! in the parameter plane spans more than one period, and a hole in it lies in one of them.

mod springrep_port;

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// A rod of radius 5 from z = 0 to 30: 10 mm with its seam at +x under 20 mm with its seam at −x.
fn rod30(k: &mut NativeKernel) -> BodyId {
    let low = cylinder(k, &world(), 5.0, 10.0);
    let f = frame(P3::new(0.0, 0.0, 10.0), V3::z(), -V3::x());
    let high = cylinder(k, &f, 5.0, 20.0);
    union(k, low, high)
}

/// A blind hole of radius 0.5, 2 mm deep, drilled radially at height 20 and `angle` round from +x.
fn drill(k: &mut NativeKernel, angle: f64) -> BodyId {
    let out = V3::new(angle.cos(), angle.sin(), 0.0);
    let f = frame(P3::new(0.0, 0.0, 20.0) + out * 3.0, out, V3::z());
    cylinder(k, &f, 0.5, 4.0)
}

/// The volume a closed triangle mesh bounds.
fn mesh_volume(m: &TriMesh) -> f64 {
    m.indices
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m.positions[i as usize].coords);
            a.dot(&b.cross(&c)) / 6.0
        })
        .sum()
}

#[test]
fn a_hole_in_a_face_whose_seam_jogs_meshes_wherever_round_it_is() {
    let mut k = NativeKernel::new();
    let rod = rod30(&mut k);
    valid(&k, rod);
    // The two sides are one face: with the two ends, three.
    assert_eq!(faces(&k, rod), 3);
    let opt = Tessellation {
        deflection: 0.01,
        angle: 0.2,
    };
    for angle in [0.2, -0.2, PI - 0.2, PI + 0.2] {
        let d = drill(&mut k, angle);
        let cut = difference(&mut k, rod, d);
        assert!(issues(&k, cut).is_empty(), "{angle}: {:?}", issues(&k, cut));
        // As a file gives it, with each edge's place on the face found on its own.
        let text = k.export_step(&[cut]).unwrap();
        let read = k.import_step(&text).unwrap();
        assert_eq!(read.len(), 1, "{angle}");
        let drilled = read[0];
        let m = k
            .tessellate(drilled, opt)
            .unwrap_or_else(|e| panic!("{angle}: {e:?}"));
        let report = cadrs_kernel::testkit::tess::check_tri_mesh(&m);
        assert!(report.is_watertight(), "{angle}: {report:?}");
        // Chords 0.01 mm deep leave out under 2 · 0.01 / 5 of the rod's π · 5² · 30.
        let exact = volume(&k, drilled);
        assert!((exact - PI * 750.0).abs() < 2.0, "{angle}: {exact}");
        let mv = mesh_volume(&m);
        assert!(
            (exact * (1.0 - 0.004)..exact).contains(&mv),
            "{angle}: mesh {mv} vs exact {exact}"
        );
    }
}
