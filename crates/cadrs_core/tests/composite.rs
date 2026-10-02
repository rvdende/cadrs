//! P3H.6 (PCB7.9, X9): the Transform's copies of assembly-context parts (on main's Transform,
//! `tests/transform.rs`) and the Composite part. Volumes are worked out by hand: a 10 × 20 × 30 box is 6000 mm³, a 10 × 10 × 5 plate
//! 500 mm³.

use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::managed_context::CreateStudioInContext;
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::command::History;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch};
use cadrs_core::document::{ExtrudeFeature, FeatureKind, Offset};
use cadrs_core::ids::{ElementId, FeatureId, PartId};
use cadrs_core::transform::{CompositeFeature, TransformFeature, TransformType, context_copies};
use cadrs_core::Document;
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x7a4f_0000 + n)
}

/// A block [x0, x1] × [y0, y1] × [z0, z0 + h] as a sketch and a New extrude; returns its part.
fn block(doc: &mut Document, h: &mut History, el: ElementId, n: u128, [x0, y0, x1, y1]: [f64; 4], z0: f64, depth: f64) -> PartId {
    let (sk, ex) = (fid(2 * n), fid(2 * n + 1));
    h.execute(doc, &AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) }).unwrap();
    let pts = vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
    h.execute(doc, &EditSketch { element: el, feature: sk, op: SketchOp::AddPolyline { points: pts, closed: true, construction: false, label: "Add rectangle" } }).unwrap();
    let start_offset = (z0 != 0.0).then(|| Offset { value: z0, expr: format!("{z0} mm"), flip: false });
    let e = ExtrudeFeature { sketches: vec![sk], depth, depth_expr: format!("{depth} mm"), start_offset, ..ExtrudeFeature::default() };
    h.execute(doc, &AddExtrude { element: el, feature: ex, extrude: e }).unwrap();
    PartId::new(ex, 0)
}

fn build(doc: &Document, el: ElementId) -> std::sync::Arc<cadrs_core::rebuild::Build> {
    cadrs_core::rebuild::build(&doc.element(el).unwrap().active_features())
}

fn volume(b: &cadrs_core::rebuild::Build, p: PartId) -> f64 {
    b.part(p).unwrap().mass.unwrap().volume
}

fn min_x(b: &cadrs_core::rebuild::Build, p: PartId) -> f64 {
    b.part(p).unwrap().solid.bounds().unwrap().0[0]
}

fn transform(el: ElementId, n: u128, t: TransformFeature) -> AddFeature {
    AddFeature { element: el, feature: fid(100 + n), base_name: "Transform".into(), kind: FeatureKind::Transform(t) }
}

fn composite(el: ElementId, n: u128, c: CompositeFeature) -> AddFeature {
    AddFeature { element: el, feature: fid(200 + n), base_name: "Composite part".into(), kind: FeatureKind::Composite(c) }
}

#[test]
fn transform_copy_in_place_keeps_the_original() {
    // Copy in place: the original keeps its id and a copy (6000 mm³) sits on top of it.
    let mut doc = Document::new("Transform");
    let mut h = History::default();
    let el = doc.elements[0].id;
    let a = block(&mut doc, &mut h, el, 1, [0.0, 0.0, 10.0, 20.0], 0.0, 30.0);
    h.execute(&mut doc, &transform(el, 1, TransformFeature { parts: vec![a], ..TransformFeature::new(TransformType::CopyInPlace) })).unwrap();
    let b = build(&doc, el);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 2);
    assert!(b.part(a).is_some(), "the original keeps its id");
    let copy = b.parts.iter().find(|p| p.id != a).unwrap();
    assert!((volume(&b, copy.id) - 6000.0).abs() < 1e-6);
    assert_eq!(copy.source, Some(a), "it looks like its original");
    assert!((min_x(&b, copy.id) - 0.0).abs() < 1e-9);
}

#[test]
fn composite_closed_volume_is_the_union() {
    // A 10 × 20 × 30 box and a 10 × 10 × 5 plate on its top face: they touch, so their union's
    // volume is the sum, 6500; the composite is one more part, the members stay.
    let mut doc = Document::new("Composite");
    let mut h = History::default();
    let el = doc.elements[0].id;
    let a = block(&mut doc, &mut h, el, 1, [0.0, 0.0, 10.0, 20.0], 0.0, 30.0);
    let p = block(&mut doc, &mut h, el, 2, [0.0, 0.0, 10.0, 10.0], 30.0, 5.0);
    h.execute(&mut doc, &composite(el, 1, CompositeFeature { parts: vec![a, p], closed: true })).unwrap();
    let b = build(&doc, el);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 1, "closed: one part");
    assert_eq!(b.composites.len(), 1);
    let c = &b.composites[0];
    assert!(c.closed);
    assert_eq!(c.members, vec![a, p]);
    let part = b.part(c.part).unwrap();
    assert_eq!(part.name, "Composite part 1");
    assert!((volume(&b, c.part) - 6500.0).abs() < 1e-6);
    assert!(b.part(a).is_none() && b.part(p).is_none(), "the members aren't listed");
    // The next New part is still "Part 3".
    let q = block(&mut doc, &mut h, el, 3, [50.0, 0.0, 60.0, 10.0], 0.0, 10.0);
    let b = build(&doc, el);
    assert_eq!(b.part(q).unwrap().name, "Part 3");
    // Open: the composite and its members are all listed.
    let open = CompositeFeature { parts: vec![a, p], closed: false };
    let feature = cadrs_core::Feature { kind: FeatureKind::Composite(open), ..doc.element(el).unwrap().feature(fid(201)).unwrap().clone() };
    h.execute(&mut doc, &cadrs_core::commands::ReplaceFeature { element: el, feature, label: "Closed".into() }).unwrap();
    let b = build(&doc, el);
    assert_eq!(b.parts.len(), 4);
    assert!(b.part(a).is_some() && b.part(p).is_some());
    assert!((volume(&b, b.composites[0].part) - 6500.0).abs() < 1e-6);
}

#[test]
fn transform_copies_context_parts_into_a_closed_composite() {
    // PCB7.9 in small: an assembly with two instances of a 10 × 20 × 30 box (at the origin and
    // 50 mm along X); a Part Studio created in its context copies both context parts in place
    // (Transform, Copy part) and makes them one closed composite part (12000), which goes into
    // the assembly as one instance.
    let mut doc = Document::new("In context");
    let mut h = History::default();
    let studio = doc.elements[0].id;
    let asm = doc.elements[1].id;
    let a = block(&mut doc, &mut h, studio, 1, [0.0, 0.0, 10.0, 20.0], 0.0, 30.0);
    let green = cadrs_core::appearance::Appearance::rgb(34, 170, 34);
    h.execute(&mut doc, &cadrs_core::commands::SetPartAppearance { element: studio, parts: vec![a], appearance: Some(green) }).unwrap();
    for (i, x) in [0.0, 50.0].into_iter().enumerate() {
        let inst = Instance::new(InstanceId::from_u128(0x7a4f_1000 + i as u128), InstanceSource::Part { element: studio, part: a }, Pose::translation([x, 0.0, 0.0]));
        h.execute(&mut doc, &InsertInstance { element: asm, instance: inst }).unwrap();
    }
    let ctx = ElementId::from_u128(0x7a4f_2000);
    h.execute(&mut doc, &CreateStudioInContext { assembly: asm, studio: ctx, name: Some("One part".into()), origin: cadrs_core::assembly::Pose::IDENTITY }).unwrap();
    let picked: Vec<PartId> = doc.element(ctx).unwrap().contexts[0].parts.iter().map(|c| PartId::new(c.id, 0)).collect();
    assert_eq!(picked.len(), 2);
    let (own, copies, sources) = context_copies(&doc, ctx, &picked);
    assert!(own.is_empty());
    assert_eq!(copies.len(), 2);
    assert_eq!(sources.len(), 1, "one source studio, kept once");
    // Context parts must be copied.
    let t = TransformFeature { context: copies.clone(), sources: sources.clone(), ..TransformFeature::new(TransformType::TranslateXyz) };
    assert!(t.problem().is_some());
    let t = TransformFeature { context: copies, sources, ..TransformFeature::new(TransformType::CopyInPlace) };
    h.execute(&mut doc, &transform(ctx, 1, t)).unwrap();
    let b = build(&doc, ctx);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 2);
    let mut xs: Vec<f64> = b.parts.iter().map(|p| min_x(&b, p.id)).collect();
    xs.sort_by(f64::total_cmp);
    assert!((xs[0] - 0.0).abs() < 1e-9 && (xs[1] - 50.0).abs() < 1e-9, "in place: {xs:?}");
    // The copies keep their Part Studio's colour.
    for p in &b.parts {
        assert_eq!(cadrs_core::appearance::part_appearance(p, &[]), green);
    }
    let members: Vec<PartId> = b.parts.iter().map(|p| p.id).collect();
    h.execute(&mut doc, &composite(ctx, 1, CompositeFeature { parts: members, closed: true })).unwrap();
    let b = build(&doc, ctx);
    let c = b.composites[0].part;
    assert_eq!(b.parts.len(), 1, "one part for the whole context");
    // Every face of the composite keeps its member's colour.
    let cp = b.part(c).unwrap();
    assert_eq!(cp.solid.looks.len(), cp.solid.faces.len());
    assert!(cp.solid.faces.iter().all(|f| cadrs_core::appearance::face_appearance(cp, &f.name, &[], &[]).0 == green));
    assert!((volume(&b, c) - 12000.0).abs() < 1e-6);
    // Inserted as one instance.
    let one = InstanceId::from_u128(0x7a4f_3000);
    h.execute(&mut doc, &InsertInstance { element: asm, instance: Instance::new(one, InstanceSource::Part { element: ctx, part: c }, Pose::IDENTITY) }).unwrap();
    assert_eq!(doc.element(asm).unwrap().assembly_model().unwrap().instances.len(), 3);
}
