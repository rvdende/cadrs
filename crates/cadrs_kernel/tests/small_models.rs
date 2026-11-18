//! Parts a few microns across: the kernel's merge distances and guards are lengths in mm, the
//! size of such a part, so its operations run on it scaled up to a working size and scale the
//! result back (booleans in `boolean.rs`, the others in `NativeKernel::at_working_size`, a
//! STEP file's reading in `step.rs`). What a part a thousandth of a mm across gets is what the
//! same part a thousand times larger gets, a thousand times smaller.

use cadrs_kernel::native::*;
use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};

/// A cube of side `s` at the origin.
fn cube(k: &mut NativeKernel, s: f64) -> BodyId {
    k.primitive(&Primitive::block(Point3::origin(), Point3::new(s, s, s)))
        .unwrap()
        .first()
        .unwrap()
}

/// The cube's edges along z (four of them).
fn uprights(k: &NativeKernel, b: BodyId) -> Vec<EdgeId> {
    let body = k.get(b).unwrap();
    (0..body.edges.len())
        .filter(|&e| {
            let ed = &body.edges[e];
            let d = ed.point(ed.t[1]) - ed.point(ed.t[0]);
            d.cross(&Vector3::z()).norm() < 1e-9 * d.norm()
        })
        .map(|e| EdgeId(e as u64))
        .collect()
}

#[test]
fn a_micron_cubes_edges_rounded_chamfered_and_hollowed() {
    for s in [1.0, 1e-3] {
        let mut k = NativeKernel::new();
        let c = cube(&mut k, s);
        let edges = uprights(&k, c);
        assert_eq!(edges.len(), 4);
        // Rounded: each edge loses (1 − π/4) r² of its length.
        let r = 0.1 * s;
        let out = k.fillet(c, &edges, r).unwrap().bodies[0];
        assert!(k.get(out).unwrap().check().is_empty(), "side {s}");
        let want = s.powi(3) - 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * r * r * s;
        let v = k.mass_properties(out).unwrap().volume;
        assert!(
            (v - want).abs() < 1e-9 * want,
            "side {s}: rounded {v}, want {want}"
        );
        // Chamfered: each loses d²/2 of its length.
        let out = k
            .chamfer(c, &edges, ChamferSpec::EqualDistance(r))
            .unwrap()
            .bodies[0];
        assert!(k.get(out).unwrap().check().is_empty(), "side {s}");
        let want = s.powi(3) - 4.0 * 0.5 * r * r * s;
        let v = k.mass_properties(out).unwrap().volume;
        assert!(
            (v - want).abs() < 1e-9 * want,
            "side {s}: chamfered {v}, want {want}"
        );
        // Hollowed through its top, walls a tenth thick.
        let top = {
            let b = k.get(c).unwrap();
            (0..b.faces.len())
                .find(|&f| (b.faces[f].surf.eval(0.0, 0.0).z - s).abs() < 1e-9 * s)
                .map(|f| FaceId(f as u64))
                .unwrap()
        };
        let spec = ShellSpec {
            remove: vec![top],
            thickness: r,
            outward: false,
            hollow: false,
            sharp: true,
        };
        let out = k.shell_with(c, &spec).unwrap().bodies[0];
        assert!(k.get(out).unwrap().check().is_empty(), "side {s}");
        let want = s.powi(3) - (s - 2.0 * r).powi(2) * (s - r);
        let v = k.mass_properties(out).unwrap().volume;
        assert!(
            (v - want).abs() < 1e-9 * want,
            "side {s}: hollowed {v}, want {want}"
        );
        // The history names the cube given, not the one worked on.
        let r = k.fillet(c, &edges, r).unwrap();
        assert!(r.history.modified.iter().all(|(_, from)| from.body == c));
    }
}

/// A micron cube with a rounded edge written to STEP and read back: the same solid, where
/// reading it at its own size joined points 1e-5 mm apart (a hundredth of it).
#[test]
fn a_micron_part_survives_step() {
    let mut k = NativeKernel::new();
    let s = 1e-3;
    let c = cube(&mut k, s);
    let edges = uprights(&k, c);
    let part = k.fillet(c, &edges, 0.02 * s).unwrap().bodies[0];
    let want = k.mass_properties(part).unwrap().volume;
    let faces = k.get(part).unwrap().faces.len();
    let bytes = k.export_step(&[part]).unwrap();
    let read = k.import_step(&bytes).unwrap();
    assert_eq!(read.len(), 1);
    let b = k.get(read[0]).unwrap();
    assert!(b.check().is_empty(), "{:?}", b.check());
    assert_eq!(b.faces.len(), faces);
    let v = k.mass_properties(read[0]).unwrap().volume;
    assert!(
        (v - want).abs() < 1e-9 * want,
        "read back {v}, written {want}"
    );
}

/// The same through IGES (whose solids name their own vertices and edges, so this held
/// already; the trimmed surfaces that have to be sewn are `iges`' own test).
#[test]
fn a_micron_part_survives_iges() {
    let mut k = NativeKernel::new();
    let s = 1e-3;
    let c = cube(&mut k, s);
    let edges = uprights(&k, c);
    let part = k.fillet(c, &edges, 0.02 * s).unwrap().bodies[0];
    let want = k.mass_properties(part).unwrap().volume;
    let faces = k.get(part).unwrap().faces.len();
    let bytes = cadrs_kernel::native::iges::write(&[(&**k.get(part).unwrap(), "part")]);
    let read = cadrs_kernel::native::iges::read(&bytes).unwrap();
    assert_eq!(read.parts.len(), 1);
    let b = &read.parts[0].body;
    assert!(b.check().is_empty(), "{:?}", b.check());
    assert_eq!(b.faces.len(), faces);
    let v = cadrs_kernel::native::mass::body_volume(b);
    assert!(
        (v - want).abs() < 1e-9 * want,
        "read back {v}, written {want}"
    );
}
