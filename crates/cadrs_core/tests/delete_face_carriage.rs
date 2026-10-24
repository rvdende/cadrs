//! #22: Delete face on parts like the issue's 3D-printed carriage core (an imported STEP with
//! no history), guessed from its description since the file isn't available: an Ø8 through
//! bore with a printing teardrop (two flat "V" faces at 45° over the bore) that runs into a
//! hex-nut recess or crosses a bearing slot. Deleting the V faces should heal the bore round,
//! so each part's result is checked against the same part built with a round bore. Each case
//! runs on the features directly and on the part exported to STEP and imported back (faces
//! with no feature history, as the reporter's).
//!
//! The kernel heals them by extending the faces round the teardrop over the gap
//! (`cadrs_kernel::native::defeature`). `cargo test -r -p cadrs_core --test
//! delete_face_carriage -- --ignored write_steps` writes the parts to `target/carriage/` to try
//! in the app.
#![cfg(feature = "brep")]

use std::f64::consts::FRAC_1_SQRT_2;
use std::sync::Arc;

use cadrs_core::direct_edit::{DirectEditFeature, DirectEditKind};
use cadrs_core::document::{FaceRef, Feature, FeatureKind};
use cadrs_core::import::{ImportAs, ImportFormat, ImportIds, imported_document};
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
use cadrs_core::samples::carriage::{self, BORE, Case, features};
use cadrs_core::{ElementId, FeatureId, Part};

#[track_caller]
fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "got {actual}, expected {expected} ± {tol}"
    );
}

/// The faces of the teardrop's two flat sides: planar, their normals at 45° in the x–z plane.
fn v_faces(part: &Part) -> Vec<FaceRef> {
    let s = &part.solid;
    (0..s.faces.len())
        .filter(|&i| {
            s.faces[i].plane.is_some_and(|p| {
                let n = p.normal();
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                (n[0].abs() / l - FRAC_1_SQRT_2).abs() < 1e-6
                    && (n[2].abs() / l - FRAC_1_SQRT_2).abs() < 1e-6
            })
        })
        .map(|i| FaceRef {
            part: part.id,
            face: s.faces[i].name,
            seed: s.face_point(i).unwrap(),
        })
        .collect()
}

fn delete_faces(faces: Vec<FaceRef>) -> Feature {
    Feature {
        id: FeatureId::new(),
        name: "Delete face".into(),
        kind: FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::DeleteFace,
            faces,
            ..Default::default()
        }),
        suppress_by: None,
    }
}

/// The part's features exported to STEP and imported back: one Import feature.
fn via_step(features: Vec<Feature>) -> Vec<Feature> {
    let features = Arc::new(features);
    let mut r = Rebuilder::new();
    let build = r.rebuild(&features);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let items = build
        .parts
        .iter()
        .map(|p| ExportItem {
            features: features.clone(),
            part: p.id,
            name: p.name.clone(),
            pose: None,
            source: (ElementId::from_u128(1), p.id),
            source_name: p.name.clone(),
        })
        .collect();
    let files = r
        .export_files(&ExportRequest::new(ModelFormat::Step, "Carriage", items))
        .unwrap();
    let text = String::from_utf8(files[0].bytes.clone()).unwrap();
    let plan = r.plan_import(ImportFormat::Step, text.as_bytes()).unwrap();
    let doc = imported_document(
        &plan,
        "Carriage.step",
        text,
        ImportAs::PartStudio,
        ImportIds::fresh(),
    );
    doc.elements[0].features().to_vec()
}

/// Deletes the V faces of the case's part (built directly, or through STEP) and checks it
/// heals into the round-bore part.
fn heals_round(case: Case, step: bool) {
    let prepare = |f: Vec<Feature>| if step { via_step(f) } else { f };
    let round = Rebuilder::new().rebuild(&prepare(features(case, false)));
    assert!(round.errors.is_empty(), "{:?}", round.errors);
    let expected = round.parts[0].mass.unwrap();
    let mut features = prepare(features(case, true));
    let before = Rebuilder::new().rebuild(&features);
    assert!(before.errors.is_empty(), "{:?}", before.errors);
    assert_eq!(before.parts.len(), 1, "one part");
    let faces = v_faces(&before.parts[0]);
    let want = match case {
        Case::Plain | Case::HexRecess | Case::TightHexRecess => 2,
        Case::BearingSlot | Case::Both => 4,
    };
    assert_eq!(faces.len(), want, "the teardrop's flat faces");
    let teardrop = before.parts[0].mass.unwrap().volume;
    // The teardrop's section is ¾ of the circle and a kite of two r × r right triangles: it
    // takes r²·(1 − π/4) more than the round bore over the bore's 30 mm (less where other
    // cuts already took material).
    let (_, _, r) = BORE;
    let extra = r * r * (1.0 - std::f64::consts::FRAC_PI_4) * 30.0;
    match case {
        Case::Plain => close(teardrop, expected.volume - extra, 1e-6 * extra),
        _ => assert!(
            teardrop < expected.volume - 1e-3 && teardrop > expected.volume - extra - 1e-6,
            "the teardrop takes between 0 and {extra}: {}",
            expected.volume - teardrop
        ),
    }
    features.push(delete_faces(faces));
    let healed = Rebuilder::new().rebuild(&features);
    assert!(
        healed.errors.is_empty(),
        "{case:?} (STEP: {step}): {:?}",
        healed.errors
    );
    let got = healed.parts[0].mass.unwrap();
    close(got.volume, expected.volume, 1e-6 * expected.volume);
    close(
        got.surface_area,
        expected.surface_area,
        1e-6 * expected.surface_area,
    );
}

#[test]
fn a_plain_teardrop_heals_round() {
    heals_round(Case::Plain, false);
}

#[test]
fn a_plain_teardrop_heals_round_after_step() {
    heals_round(Case::Plain, true);
}

#[test]
fn a_teardrop_into_a_hex_recess_heals_round() {
    heals_round(Case::HexRecess, false);
}

#[test]
fn a_teardrop_into_a_hex_recess_heals_round_after_step() {
    heals_round(Case::HexRecess, true);
}

#[test]
fn a_teardrop_past_a_tight_hex_recess_heals_round() {
    heals_round(Case::TightHexRecess, false);
}

#[test]
fn a_teardrop_past_a_tight_hex_recess_heals_round_after_step() {
    heals_round(Case::TightHexRecess, true);
}

#[test]
fn a_teardrop_across_a_bearing_slot_heals_round() {
    heals_round(Case::BearingSlot, false);
}

#[test]
fn a_teardrop_across_a_bearing_slot_heals_round_after_step() {
    heals_round(Case::BearingSlot, true);
}

#[test]
fn a_teardrop_into_a_hex_recess_and_across_a_slot_heals_round() {
    heals_round(Case::Both, false);
}

#[test]
fn a_teardrop_into_a_hex_recess_and_across_a_slot_heals_round_after_step() {
    heals_round(Case::Both, true);
}

/// Writes each case, teardrop and round, to `target/carriage/<case>-<bore>.step`.
#[test]
#[ignore = "writes files: run on demand"]
fn write_steps() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/carriage");
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Rebuilder::new();
    for case in [
        Case::Plain,
        Case::HexRecess,
        Case::TightHexRecess,
        Case::BearingSlot,
        Case::Both,
    ] {
        for teardrop in [true, false] {
            let bytes = carriage::step(&mut r, case, teardrop).unwrap();
            let bore = if teardrop { "teardrop" } else { "round" };
            let path = dir.join(format!("{case:?}-{bore}.step").to_lowercase());
            std::fs::write(&path, bytes).unwrap();
            println!("{}", path.display());
        }
    }
}

#[test]
fn create_selection_finds_the_imported_bore_as_a_hole() {
    // The round bore through STEP (no history): its wall is a face of revolution, and Hole from
    // it takes it alone (the block's faces round it aren't bounded by circles on its axis).
    let build = Rebuilder::new().rebuild(&via_step(features(Case::Plain, false)));
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let s = &build.parts[0].solid;
    let wall = (0..s.faces.len())
        .find(|&i| s.faces[i].kind == Some(cadrs_kernel::SurfaceKind::Cylinder))
        .expect("the bore's wall");
    assert!(
        s.faces[wall].axis.is_some(),
        "an imported cylinder has its axis"
    );
    assert_eq!(s.hole_faces(wall), vec![wall]);
}
