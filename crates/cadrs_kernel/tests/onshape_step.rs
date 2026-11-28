//! Onshape's STEP files (written by Parasolid) of CADEvolve cases read as Parasolid reads them:
//! each named part's volume against Onshape's own re-import of the same file
//! (`cq2fs step --verify`, crates/cadrs_cq2fs/cases/cadevolve/step/<set>-<k>.reimport.json).

use cadrs_kernel::native::{mass::body_volume, step};

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cadrs_cq2fs/cases/cadevolve/step"
);

fn parasolid_volume(batch: &str, part: &str) -> f64 {
    // (the pretty-printed {"name": .., "vol": ..} of each part)
    let j = std::fs::read_to_string(format!("{DIR}/{batch}.reimport.json")).unwrap();
    let at = j.find(&format!("\"name\": \"{part}\"")).unwrap();
    let rest = &j[at..];
    let v = &rest[rest.find("\"vol\":").unwrap() + 6..];
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || "+-.eE ".contains(c)))
        .unwrap();
    v[..end].trim().parse().unwrap()
}

fn check(batch: &str, parts: &[(&str, f64)]) {
    let m = step::read(&std::fs::read(format!("{DIR}/{batch}.step")).unwrap()).unwrap();
    for &(name, rel) in parts {
        let p = m.parts.iter().find(|p| p.name == name).unwrap();
        let v = body_volume(&p.body);
        let want = parasolid_volume(batch, name);
        assert!(
            ((v - want) / want).abs() <= rel,
            "{batch} {name}: {v} vs Parasolid's {want}"
        );
    }
}

/// 744: a half dome cut along its sphere's axis, its inner sphere's rim a great circle through
/// both poles (one edge passing both; read as the other half sphere, 5987 mm³ for 1138).
/// 297: a sphere with a lattice void (the void read facing out, 6938 for 1439).
#[test]
fn half_dome_and_void_as_parasolid() {
    check(
        "hard100-0",
        &[("c744.1", 1e-7), ("c297.1", 1e-8), ("c47.1", 1e-9)],
    );
}

/// 2472: a sphere with 506 spikes fused on, its face only holes (read as one spike's base,
/// 5.3 mm³ for 4194). Parasolid's tolerant union leaves 4e-4 mm gaps: 1e-6.
#[test]
fn sphere_all_holes_as_parasolid() {
    check("hard100-4", &[("c2472.1", 2e-6)]);
}

/// 3424: a triangular prism with an array of holes on a sphere (1006 for 1094). Its cylinders'
/// spline edges, 4e-4 mm off their faces, re-intersected leave 2.2e-7 (2.3e-8 as written:
/// there the file's curves are Parasolid's own tolerant edges, not an export's approximation).
#[test]
fn prism_with_hole_array_as_parasolid() {
    check("hard100-6", &[("c3424.1", 3e-7)]);
}

/// 2223: countersinks whose cones meet planes and fillets along B-splines 5.6 µm off the cones,
/// and vertices up to 2.4 µm off (Parasolid's STEP approximations): re-intersected through
/// refined vertices, 1.7e-7 (1.4e-5 as written).
#[test]
fn countersinks_reintersected_as_parasolid() {
    check("hard100-3", &[("c2223.1", 3e-7)]);
}

/// 5587: a spherical lattice whose splines cross the cylinders' seams: a vertex moved onto the
/// surfaces leaves a spline it ends not replaced short of it, so neither is (5.3e-4 when the
/// vertices were left where the file put them; 2.1e-5 as written).
#[test]
fn lattice_vertices_and_curves_together_as_parasolid() {
    check("hard100-11", &[("c5587.1", 2e-5)]);
}
