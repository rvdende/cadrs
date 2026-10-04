//! Legacy sheet metal (P3I.8, SM17): a folded part imported from STEP (cadrs's own C-channel
//! exported as a plain solid, `samples::sheetmetal_legacy`) made active sheet metal again by
//! **Sheet metal model → Thicken** of one face with **Tangent propagation**, the bend cylinders
//! picked as **Edges or cylinders to bend**, the import then removed with **Delete part**. The
//! result is the channel it came from: the same volume, flat and bends.
#![cfg(feature = "occt")]

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::document::{DeletePartFeature, FaceRef, FeatureKind};
use cadrs_core::import::ImportFeature;
use cadrs_core::rebuild::{Build, Rebuilder};
use cadrs_core::samples::sheetmetal_legacy as legacy;
use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::{Feature, FeatureId, Part};
use cadrs_kernel::SurfaceKind;
use cadrs_sheetmetal::JointKind;

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn face_ref(part: &Part, i: usize) -> FaceRef {
    let f = &part.solid.faces[i];
    FaceRef { part: part.id, face: f.name, seed: f.center.expect("a centre") }
}

fn flat_size(b: &Build) -> (f64, f64) {
    let (lo, hi) = b.sheet_metal[0].flat.parts[0].bounds().unwrap();
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    (w.max(h), w.min(h))
}

/// The import, a Thicken of its outer base face (with or without the bends) and a Delete part.
fn thickened(bends: bool) -> (Build, Build) {
    let original = Rebuilder::new().rebuild(&legacy::channel());
    assert!(original.errors.is_empty(), "{:?}", original.errors);
    let bytes = legacy::step(&mut Rebuilder::new()).expect("the STEP file");
    let import = Feature { id: FeatureId::new(), name: "Import 1".into(), kind: FeatureKind::Import(ImportFeature::from_file("channel.step", bytes).unwrap()) };
    let b = Rebuilder::new().rebuild(std::slice::from_ref(&import));
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 1);
    let part = b.parts[0].clone();
    assert!((volume(&part) - volume(&original.parts[0])).abs() < 1e-6 * volume(&part), "the STEP keeps the solid");
    // The outer face of the base: flat, facing down, at z = 0.
    let s = &part.solid;
    let base = (0..s.faces.len())
        .find(|i| s.faces[*i].plane.is_some_and(|p| p.normal()[2] < -0.999 && p.origin[2].abs() < 1e-6))
        .expect("the base's outer face");
    // The bends' outer cylinders (radius r + t).
    let cyls: Vec<usize> = (0..s.faces.len())
        .filter(|i| s.faces[*i].kind == Some(SurfaceKind::Cylinder) && s.faces[*i].radius.is_some_and(|r| (r - (legacy::RADIUS + legacy::THICKNESS)).abs() < 1e-6))
        .collect();
    assert_eq!(cyls.len(), 4, "four bends on the C");
    let p = legacy::params();
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Thicken,
        faces: vec![face_ref(&part, base)],
        tangent_propagation: true,
        bends: if bends { cyls.iter().map(|i| EdgeOrFace::Face(face_ref(&part, *i))).collect() } else { Vec::new() },
        // The thickness goes into the part, as the lesson's flip arrow does.
        flip_thickness: true,
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    let sm = Feature { id: FeatureId::new(), name: "Sheet metal model 1".into(), kind: FeatureKind::SheetMetalModel(x) };
    let del = Feature { id: FeatureId::new(), name: "Delete part 1".into(), kind: FeatureKind::DeletePart(DeletePartFeature { parts: vec![part.id] }) };
    let b = Rebuilder::new().rebuild(&[import, sm, del]);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    (original, b)
}

#[test]
fn an_imported_channel_thickened_with_its_bends_is_the_channel_again() {
    let (original, b) = thickened(true);
    assert_eq!(b.parts.len(), 1, "the import is deleted, the sheet metal part stays");
    let ctx = &b.sheet_metal[0];
    assert!(ctx.active);
    // One pick took the whole outer skin: 5 flats and 4 bends, R3.
    assert_eq!(ctx.model.walls.len(), 5);
    let bends: Vec<f64> = ctx.model.joints.iter().filter_map(|j| j.bend().map(|b| b.radius)).collect();
    assert_eq!(bends.len(), 4);
    assert!(bends.iter().all(|r| (r - legacy::RADIUS).abs() < 1e-6), "{bends:?}");
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
    let (w, h) = flat_size(&b);
    let (w0, h0) = flat_size(&original);
    assert!((w - w0).abs() < 1e-6 && (h - h0).abs() < 1e-6, "{w} × {h} vs {w0} × {h0}");
    let v = volume(&b.parts[0]);
    let v0 = volume(&original.parts[0]);
    assert!((v - v0).abs() < 1e-6 * v0, "{v} vs {v0}");
}

#[test]
fn without_the_bends_the_cylinders_are_rolled_walls() {
    let (_, b) = thickened(false);
    let ctx = &b.sheet_metal[0];
    // SM17.2: no bend centre lines, only tangent joints where the bends are.
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 0);
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Tangent { .. })).count(), 8);
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
}
