//! Legacy sheet metal (P3I.8, SM17): a folded part imported from STEP (cadrs's own C-channel
//! exported as a plain solid, `samples::sheetmetal_legacy`) made active sheet metal again by
//! **Sheet metal model → Thicken** of one face with **Tangent propagation**, the bend cylinders
//! picked as **Edges or cylinders to bend**, the import then removed with **Delete part**. The
//! result is the part it came from: the same volume, flat and bends. Two parts: a C-channel, and
//! a Case (an open box with relieved corners: propagation goes round the corner gaps).
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

/// Which legacy part: its features, its STEP export, the Thicken's settings and its bends' count.
struct Legacy {
    features: fn() -> Vec<Feature>,
    step: fn(&mut Rebuilder) -> Result<Vec<u8>, String>,
    params: fn() -> cadrs_sheetmetal::Params,
    bends: usize,
}

const CHANNEL: Legacy = Legacy { features: legacy::channel, step: legacy::step, params: legacy::params, bends: 4 };
// The Thicken's own corner relief stays Simple: the import's corners already have their reliefs.
const CASE: Legacy = Legacy { features: legacy::case, step: legacy::case_step, params: legacy::params, bends: 4 };
const CASE_ROUND: Legacy = Legacy { features: legacy::case_round, step: legacy::case_round_step, params: legacy::params, bends: 4 };

/// The import, a Thicken of its outer base face (with or without the bends) and a Delete part.
fn thickened(l: &Legacy, bends: bool) -> (Build, Build) {
    let (o, b) = thickened_with(l, bends, (l.params)());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    (o, b)
}

/// [`thickened`] with the Thicken's settings `p`, its errors left to the caller.
fn thickened_with(l: &Legacy, bends: bool, p: cadrs_sheetmetal::Params) -> (Build, Build) {
    let original = Rebuilder::new().rebuild(&(l.features)());
    assert!(original.errors.is_empty(), "{:?}", original.errors);
    let bytes = (l.step)(&mut Rebuilder::new()).expect("the STEP file");
    let import = Feature { id: FeatureId::new(), name: "Import 1".into(), kind: FeatureKind::Import(ImportFeature::from_file("legacy.step", bytes).unwrap()) };
    let b = Rebuilder::new().rebuild(std::slice::from_ref(&import));
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 1);
    let part = b.parts[0].clone();
    let sm = original.parts.iter().find(|p| original.sheet_metal[0].parts.iter().any(|(q, _)| *q == p.id)).expect("the sheet metal part");
    assert!((volume(&part) - volume(sm)).abs() < 1e-6 * volume(&part), "the STEP keeps the solid");
    // The outer face of the base: flat, facing down, the lowest.
    let s = &part.solid;
    let base = (0..s.faces.len())
        .filter(|i| s.faces[*i].plane.is_some_and(|p| p.normal()[2] < -0.999))
        .min_by(|a, b| s.faces[*a].plane.unwrap().origin[2].total_cmp(&s.faces[*b].plane.unwrap().origin[2]))
        .expect("the base's outer face");
    // The bends' outer cylinders (radius r + t).
    let cyls: Vec<usize> = (0..s.faces.len())
        .filter(|i| s.faces[*i].kind == Some(SurfaceKind::Cylinder) && s.faces[*i].radius.is_some_and(|r| (r - (p.bend_radius + p.thickness)).abs() < 1e-6))
        .collect();
    assert_eq!(cyls.len(), l.bends, "the bends' outer cylinders");
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
    (original, b)
}

/// The re-made part matches the original: walls, bends of the original radius, flat, volume.
fn same_as_original(l: &Legacy, walls: usize) {
    let (original, b) = thickened(l, true);
    assert_eq!(b.parts.len(), 1, "the import is deleted, the sheet metal part stays");
    let ctx = &b.sheet_metal[0];
    assert!(ctx.active);
    // One pick took the whole outer skin.
    assert_eq!(ctx.model.walls.len(), walls);
    let r = (l.params)().bend_radius;
    let bends: Vec<f64> = ctx.model.joints.iter().filter_map(|j| j.bend().map(|b| b.radius)).collect();
    assert_eq!(bends.len(), l.bends);
    assert!(bends.iter().all(|x| (x - r).abs() < 1e-6), "{bends:?}");
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
    let (w, h) = flat_size(&b);
    let (w0, h0) = flat_size(&original);
    assert!((w - w0).abs() < 1e-6 && (h - h0).abs() < 1e-6, "{w} × {h} vs {w0} × {h0}");
    let v = volume(&b.parts[0]);
    let sm = original.parts.iter().find(|p| original.sheet_metal[0].parts.iter().any(|(q, _)| *q == p.id)).unwrap();
    let v0 = volume(sm);
    assert!((v - v0).abs() < 1e-6 * v0, "{v} vs {v0}");
}

#[test]
fn an_imported_channel_thickened_with_its_bends_is_the_channel_again() {
    // 5 flats (lips, sides, base) and 4 bends, R3.
    same_as_original(&CHANNEL, 5);
}

#[test]
fn an_imported_case_is_taken_round_its_corner_gaps() {
    // The Case: the base and four walls, which meet only through the bends (the corners are
    // open): one pick still takes all five.
    same_as_original(&CASE, 5);
}

#[test]
fn an_imported_case_with_round_corner_reliefs_is_the_case_again() {
    // The lesson's Case has round holes at its corners. Thickened again, the holes are
    // recognised as corner reliefs: the bends run their full length (the bend ends the holes cut
    // off join them), the walls get their sharp corners back, and each hole becomes its
    // corner's Round – Sized relief, so the flat and the volume are the original's.
    same_as_original(&CASE_ROUND, 5);
    let (original, b) = thickened(&CASE_ROUND, true);
    assert!(b.warnings.is_empty(), "no folding slivers: {:?}", b.warnings);
    let (ctx, ctx0) = (&b.sheet_metal[0], &original.sheet_metal[0]);
    let (a, a0) = (ctx.flat.parts[0].area(), ctx0.flat.parts[0].area());
    assert!((a - a0).abs() < 1e-6 * a0, "flat area {a} vs {a0}");
    assert_eq!(ctx.model.corner_overrides.len(), 4, "the four holes are the corners' reliefs");
    for o in &ctx.model.corner_overrides {
        assert_eq!(o.relief.kind, cadrs_sheetmetal::CornerReliefKind::RoundSized);
    }
}

#[test]
fn without_the_bends_the_cylinders_are_rolled_walls() {
    let (_, b) = thickened(&CHANNEL, false);
    let ctx = &b.sheet_metal[0];
    // SM17.2: no bend centre lines, only tangent joints where the bends are.
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 0);
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Tangent { .. })).count(), 8);
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
    // The Case the same way (`sm_p3i8_legacy` 02): two tangent joints per bend, one flat.
    let (_, b) = thickened(&CASE, false);
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 0);
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Tangent { .. })).count(), 8);
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
}
