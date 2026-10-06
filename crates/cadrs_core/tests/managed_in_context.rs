//! MIC.1 (`managed-in-context-design.md`): several named assembly contexts per Part Studio,
//! frozen geometry, hide/show state, the primary instance, which features use which context, and
//! extruding up to the context. On the Edit in context stand-in (`samples::p3b9`): a Base plate
//! 140 × 90 × 10 mm (z −10…0) with two Ø10 holes at x ±40, and a Cover 120 × 80 × 6 mm (z 0…6).

use cadrs_core::assembly::commands::{DeleteInstances, InsertInstance, MoveInstances, SetInstancesHidden};
use cadrs_core::assembly::context::{self, AddContext, ContextStatus, RemoveContext, RenameContext, SetPrimaryInstance, UpdateContext};
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::commands::{AddExtrude, AddSketch, EditSketch, SetExtrude};
use cadrs_core::samples::p3b9;
use cadrs_core::{BooleanOp, Document, EndType, ExtrudeFeature, FeatureId, FeatureKind, History, PartId, UpTo};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};
use std::f64::consts::PI;

const STUDIO: cadrs_core::ElementId = p3b9::COVER_STUDIO;
const ASM: cadrs_core::ElementId = p3b9::CONTEXT_ASSEMBLY;

/// The context's Base solid's top (max z).
fn base_top(doc: &Document, ctx: context::ContextNo) -> f64 {
    let el = doc.element(STUDIO).unwrap();
    let c = el.context(ctx).unwrap();
    let (parts, _) = context::parts_of(doc, c, |_, f| Some(cadrs_core::rebuild::build(f)));
    parts[0].solid.vertices.iter().map(|v| v.point[2]).fold(f64::MIN, f64::max)
}

fn add_context(doc: &mut Document, h: &mut History, instance: InstanceId) -> context::ContextNo {
    let id = doc.element(STUDIO).unwrap().next_context_id();
    let ctx = context::snapshot_as(doc, ASM, instance, id).unwrap();
    h.execute(doc, &AddContext { studio: STUDIO, context: ctx }).unwrap();
    id
}

fn update(doc: &mut Document, h: &mut History, id: context::ContextNo) {
    let ctx = doc.element(STUDIO).unwrap().context(id).unwrap().clone();
    let now = context::resnapshot(doc, STUDIO, &ctx).unwrap();
    h.execute(doc, &UpdateContext { studio: STUDIO, context: now }).unwrap();
}

fn status(doc: &Document, id: context::ContextNo) -> ContextStatus {
    let el = doc.element(STUDIO).unwrap();
    context::status(doc, STUDIO, el.context(id).unwrap())
}

#[test]
fn a_context_freezes_the_geometry_until_updated() {
    // MC1.2, MC1.6: editing the Base's studio changes nothing in the Cover's context until
    // Update context.
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let id = add_context(&mut doc, &mut h, p3b9::COVER);
    assert!((base_top(&doc, id) - 0.0).abs() < 1e-9);
    assert_eq!(status(&doc, id), ContextStatus::UpToDate);
    // The Base becomes 4 mm thick (z −10…−6).
    let ex = FeatureId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_0211);
    let mut x = match &doc.element(p3b9::CONTEXT_BASE_STUDIO).unwrap().feature(ex).unwrap().kind {
        FeatureKind::Extrude(x) => x.clone(),
        _ => panic!("the Base's extrude"),
    };
    x.depth = 4.0;
    x.depth_expr = "4 mm".into();
    h.execute(&mut doc, &SetExtrude { element: p3b9::CONTEXT_BASE_STUDIO, feature: ex, extrude: x, label: "Extrude".into() }).unwrap();
    assert!((base_top(&doc, id) - 0.0).abs() < 1e-9, "still the snapshot");
    assert_eq!(status(&doc, id), ContextStatus::OutOfDate, "an update is available");
    update(&mut doc, &mut h, id);
    assert!((base_top(&doc, id) + 6.0).abs() < 1e-9, "{}", base_top(&doc, id));
    assert_eq!(status(&doc, id), ContextStatus::UpToDate);
}

#[test]
fn hidden_instances_are_not_in_the_context() {
    // MC2.2, MC4.5: the snapshot takes the hide/show state.
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let id = add_context(&mut doc, &mut h, p3b9::COVER);
    assert_eq!(doc.element(STUDIO).unwrap().context(id).unwrap().parts.len(), 1);
    h.execute(&mut doc, &SetInstancesHidden { element: ASM, instances: vec![p3b9::CONTEXT_BASE], hidden: true }).unwrap();
    assert_eq!(status(&doc, id), ContextStatus::OutOfDate);
    update(&mut doc, &mut h, id);
    assert!(doc.element(STUDIO).unwrap().context(id).unwrap().parts.is_empty());
}

#[test]
fn several_contexts_have_their_own_ids_names_and_features() {
    // MC1.7, MC2.13–MC2.15: two contexts at two positions of the Base; each feature knows its
    // context.
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let a = add_context(&mut doc, &mut h, p3b9::COVER);
    h.execute(&mut doc, &MoveInstances { element: ASM, poses: vec![(p3b9::CONTEXT_BASE, Pose::translation([0.0, 0.0, -20.0]))], label: "Move".into() }).unwrap();
    let b = add_context(&mut doc, &mut h, p3b9::COVER);
    assert_eq!((a, b), (0, 1));
    let el = doc.element(STUDIO).unwrap();
    assert_eq!(el.contexts.iter().map(|c| c.label()).collect::<Vec<_>>(), ["Context 1", "Context 2"]);
    let (pa, pb) = (el.context(a).unwrap().parts[0].id, el.context(b).unwrap().parts[0].id);
    assert_ne!(pa, pb, "the same instance in two contexts is two parts");
    assert!(context::is_context(pa) && context::is_context(pb));
    assert_eq!(pa, context::context_id(0, p3b9::CONTEXT_BASE), "context 0 keeps the ids of the single context");
    assert!((base_top(&doc, a) - 0.0).abs() < 1e-9 && (base_top(&doc, b) + 20.0).abs() < 1e-9);
    // A sketch on the Base's top face in each context.
    let sketch_on = |doc: &mut Document, h: &mut History, ctx: context::ContextNo, n: u128| {
        let el = doc.element(STUDIO).unwrap();
        let c = el.context(ctx).unwrap();
        let (parts, _) = context::parts_of(doc, c, |_, f| Some(cadrs_core::rebuild::build(f)));
        let s = &parts[0].solid;
        let top = s.vertices.iter().map(|v| v.point[2]).fold(f64::MIN, f64::max);
        let face = s
            .faces
            .iter()
            .enumerate()
            .find(|(i, f)| f.plane.is_some_and(|p| (p.origin[2] - top).abs() < 1e-9) && s.face_normal(*i).is_some_and(|n| n[2] > 0.999))
            .map(|(_, f)| f.name)
            .unwrap();
        let plane = context::face_plane_on(s, parts[0].feature, face).unwrap();
        let sk = FeatureId::from_u128(n);
        h.execute(doc, &AddSketch { element: STUDIO, feature: sk, plane: Some(plane) }).unwrap();
        sk
    };
    let ska = sketch_on(&mut doc, &mut h, a, 0x4c01);
    let skb = sketch_on(&mut doc, &mut h, b, 0x4c02);
    let el = doc.element(STUDIO).unwrap();
    assert_eq!(context::feature_contexts(el, el.feature(ska).unwrap()), [a]);
    assert_eq!(context::feature_contexts(el, el.feature(skb).unwrap()), [b]);
    assert!(context::is_referenced(el, a) && context::is_referenced(el, b));
    h.execute(&mut doc, &RenameContext { studio: STUDIO, id: b, name: "Lowered".into() }).unwrap();
    assert_eq!(doc.element(STUDIO).unwrap().context(b).unwrap().label(), "Lowered");
    // Deleting a context keeps its sketch where it was.
    let z = |doc: &Document| match doc.element(STUDIO).unwrap().feature(skb).unwrap().sketch().unwrap().plane {
        Some(PlaneRef::Face(fp)) => fp.origin[2],
        _ => f64::NAN,
    };
    assert!((z(&doc) + 20.0).abs() < 1e-9);
    h.execute(&mut doc, &RemoveContext { studio: STUDIO, id: b }).unwrap();
    assert!(doc.element(STUDIO).unwrap().context(b).is_none());
    assert!((z(&doc) + 20.0).abs() < 1e-9);
    // The next new context doesn't reuse a number in use.
    assert_eq!(doc.element(STUDIO).unwrap().next_context_id(), 1);
}

#[test]
fn the_primary_instance_anchors_and_is_needed_to_update() {
    // MC3.2–MC3.4: a second Cover instance 30 mm up; the context is anchored on the first. With
    // the first deleted it can't update until the second is set as primary, and then the Base
    // is placed relative to the second.
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let cover2 = InstanceId::from_u128(0x4c00_0001);
    h.execute(&mut doc, &InsertInstance { element: ASM, instance: Instance::new(cover2, InstanceSource::Part { element: STUDIO, part: p3b9::COVER_PART }, Pose::translation([0.0, 0.0, 30.0])) }).unwrap();
    let id = add_context(&mut doc, &mut h, p3b9::COVER);
    assert_eq!(doc.element(STUDIO).unwrap().context(id).unwrap().parts.len(), 1, "the other Cover isn't context (same studio)");
    h.execute(&mut doc, &DeleteInstances { element: ASM, instances: vec![p3b9::COVER] }).unwrap();
    assert_eq!(status(&doc, id), ContextStatus::NoPrimary);
    let ctx = doc.element(STUDIO).unwrap().context(id).unwrap().clone();
    assert!(context::resnapshot(&doc, STUDIO, &ctx).is_err());
    // Only a part of this studio can be primary.
    assert!(h.execute(&mut doc, &SetPrimaryInstance { studio: STUDIO, contexts: vec![id], instance: p3b9::CONTEXT_BASE }).is_err());
    h.execute(&mut doc, &SetPrimaryInstance { studio: STUDIO, contexts: vec![id], instance: cover2 }).unwrap();
    assert_eq!(status(&doc, id), ContextStatus::OutOfDate);
    update(&mut doc, &mut h, id);
    assert!((base_top(&doc, id) + 30.0).abs() < 1e-9, "relative to the second Cover: {}", base_top(&doc, id));
}

#[test]
fn an_extrude_goes_up_to_a_context_face_and_follows_an_update() {
    // MC1.3: in the Cover studio, a Ø6 circle on Top extruded down (flipped) up to the Base's
    // bottom face (z −10): 10 mm long, 90π mm³. The Base goes 5 mm down in the assembly: after
    // Update context the pin is 15 mm long.
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let id = add_context(&mut doc, &mut h, p3b9::COVER);
    let c = doc.element(STUDIO).unwrap().context(id).unwrap().clone();
    let (parts, _) = context::parts_of(&doc, &c, |_, f| Some(cadrs_core::rebuild::build(f)));
    let s = parts[0].solid.clone();
    let bottom = s
        .faces
        .iter()
        .enumerate()
        .find(|(i, f)| f.plane.is_some_and(|p| (p.origin[2] + 10.0).abs() < 1e-9) && s.face_normal(*i).is_some_and(|n| n[2] < -0.999))
        .map(|(i, f)| (f.name, s.face_point(i).unwrap()))
        .unwrap();
    let sk = FeatureId::from_u128(0x4c11);
    h.execute(&mut doc, &AddSketch { element: STUDIO, feature: sk, plane: Some(PlaneRef::Top) }).unwrap();
    h.execute(&mut doc, &EditSketch { element: STUDIO, feature: sk, op: SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 3.0, construction: false } }).unwrap();
    let g = doc.element(STUDIO).unwrap().feature(sk).unwrap().sketch().unwrap().geometry.clone();
    let regions = cadrs_core::samples::region_refs(sk, &g, &[Vec2::new(0.0, 0.0)]);
    let mut x = cadrs_core::samples::extrude_of(regions, 1.0);
    x.op = BooleanOp::New;
    x.flip = true;
    x.end = EndType::UpToFace;
    x.up_to = Some(UpTo::Face(cadrs_core::FaceRef { part: PartId::new(parts[0].feature, 0), face: bottom.0, seed: bottom.1 }));
    let ex = FeatureId::from_u128(0x4c12);
    h.execute(&mut doc, &AddExtrude { element: STUDIO, feature: ex, extrude: ExtrudeFeature::default() }).unwrap();
    h.execute(&mut doc, &SetExtrude { element: STUDIO, feature: ex, extrude: x, label: "Extrude".into() }).unwrap();
    let pin = PartId::new(ex, 0);
    let volume = |doc: &Document| {
        let b = cadrs_core::rebuild::build(&doc.element(STUDIO).unwrap().active_features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b.part(pin).and_then(|p| p.mass.as_ref()).map(|m| m.volume).unwrap_or(f64::NAN)
    };
    let el = doc.element(STUDIO).unwrap();
    assert_eq!(context::feature_contexts(el, el.feature(ex).unwrap()), [id], "the extrude is in context");
    assert!((volume(&doc) - 90.0 * PI).abs() < 1e-6, "{}", volume(&doc));
    // The context isn't a part of the studio.
    let b = cadrs_core::rebuild::build(&doc.element(STUDIO).unwrap().active_features());
    assert!(b.parts.iter().all(|p| !context::is_context(p.feature)));
    h.execute(&mut doc, &MoveInstances { element: ASM, poses: vec![(p3b9::CONTEXT_BASE, Pose::translation([0.0, 0.0, -5.0]))], label: "Move".into() }).unwrap();
    assert!((volume(&doc) - 90.0 * PI).abs() < 1e-6, "not until Update context");
    update(&mut doc, &mut h, id);
    assert!((volume(&doc) - 135.0 * PI).abs() < 1e-6, "{}", volume(&doc));
    // Undo the update: 10 mm again.
    h.undo(&mut doc).unwrap();
    assert!((volume(&doc) - 90.0 * PI).abs() < 1e-6);
}

#[test]
fn documents_with_a_single_context_still_read() {
    // A document written before several contexts has `context: Some(...)` on the Part Studio.
    let mut doc = p3b9::context_document().unwrap();
    let ctx = context::snapshot(&doc, ASM, p3b9::COVER).unwrap();
    doc.element_mut(STUDIO).unwrap().contexts = vec![ctx.clone()];
    let text = ron::to_string(doc.element(STUDIO).unwrap()).unwrap();
    let ctx_text = ron::to_string(&ctx).unwrap();
    let old = text.replace(&format!("contexts:[{ctx_text}]"), &format!("context:Some({ctx_text})"));
    assert_ne!(old, text, "the old form was made");
    let el: cadrs_core::Element = ron::from_str(&old).unwrap();
    assert_eq!(el.contexts, vec![ctx]);
    let none = text.replace(&format!("contexts:[{ctx_text}]"), "context:None");
    let el: cadrs_core::Element = ron::from_str(&none).unwrap();
    assert!(el.contexts.is_empty());
}

#[test]
fn a_studio_made_at_a_mate_connector_takes_its_frame() {
    // MC2.9, MC2.11, MC3.3: Create Part Studio in context with its origin at a frame 10 mm along
    // X and turned 90° about Z: the Base comes in relative to that frame; a part inserted from
    // the studio lands there and becomes the primary instance, and an update keeps the context.
    use cadrs_core::assembly::managed_context::{CreateStudioInContext, InsertFromStudio};
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let origin = Pose { rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], translation: [10.0, 0.0, 0.0] };
    let studio = cadrs_core::ElementId::from_u128(0x4c20);
    h.execute(&mut doc, &CreateStudioInContext { assembly: ASM, studio, name: Some("Bracket".into()), origin }).unwrap();
    let ctx = doc.element(studio).unwrap().contexts[0].clone();
    assert_eq!(ctx.instance, InstanceId::ORIGIN);
    assert_eq!(ctx.origin, origin);
    // The Base (identity in the assembly) in the studio's frame: origin⁻¹.
    let base = ctx.parts.iter().find(|p| p.element == p3b9::CONTEXT_BASE_STUDIO).unwrap();
    let back = origin.inverse();
    assert!(base.pose.translation.iter().zip(back.translation).all(|(a, b)| (a - b).abs() < 1e-9), "{:?}", base.pose);
    // A block in the studio, inserted into the assembly.
    let sk = FeatureId::from_u128(0x4c21);
    h.execute(&mut doc, &AddSketch { element: studio, feature: sk, plane: Some(PlaneRef::Top) }).unwrap();
    h.execute(&mut doc, &EditSketch { element: studio, feature: sk, op: SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 2.0, construction: false } }).unwrap();
    let g = doc.element(studio).unwrap().feature(sk).unwrap().sketch().unwrap().geometry.clone();
    let mut x = cadrs_core::samples::extrude_of(cadrs_core::samples::region_refs(sk, &g, &[Vec2::new(0.0, 0.0)]), 3.0);
    x.op = BooleanOp::New;
    let ex = FeatureId::from_u128(0x4c22);
    h.execute(&mut doc, &AddExtrude { element: studio, feature: ex, extrude: ExtrudeFeature::default() }).unwrap();
    h.execute(&mut doc, &SetExtrude { element: studio, feature: ex, extrude: x, label: "Extrude".into() }).unwrap();
    let inst = InstanceId::from_u128(0x4c23);
    h.execute(&mut doc, &InsertFromStudio { studio, parts: vec![PartId::new(ex, 0)], instances: vec![inst] }).unwrap();
    let placed = doc.element(ASM).unwrap().assembly_model().unwrap().instance(inst).unwrap().pose;
    assert_eq!(placed, origin, "where the studio's origin is");
    let ctx = doc.element(studio).unwrap().contexts[0].clone();
    assert_eq!(ctx.instance, inst, "the first inserted part is the primary instance");
    let now = context::resnapshot(&doc, studio, &ctx).unwrap();
    let a = now.parts.iter().find(|p| p.element == p3b9::CONTEXT_BASE_STUDIO).unwrap();
    assert!(a.pose.translation.iter().zip(back.translation).all(|(a, b)| (a - b).abs() < 1e-9), "anchored on the inserted part: {:?}", a.pose);
}

#[test]
fn a_pending_context_is_committed_by_its_first_reference() {
    // MC2.5: Edit in context makes a pending context; a sketch on the Top plane doesn't
    // reference it, a sketch on the Base's face does.
    let mut doc = p3b9::context_document().unwrap();
    // Its own document id: the pending contexts are kept per document.
    doc.id = cadrs_core::DocumentId::from_u128(0x4c30);
    let mut h = History::default();
    let ctx = context::snapshot_as(&doc, ASM, p3b9::COVER, 0).unwrap();
    context::set_pending(doc.id, STUDIO, Some(ctx.clone()));
    assert_eq!(context::with_pending(&doc, STUDIO).len(), 1);
    assert!(!context::solids(&doc, STUDIO).is_empty(), "the regeneration sees it");
    h.execute(&mut doc, &AddSketch { element: STUDIO, feature: FeatureId::from_u128(0x4c31), plane: Some(PlaneRef::Top) }).unwrap();
    assert!(context::pending_to_commit(&doc).is_none(), "not referenced");
    let solids = context::solids(&doc, STUDIO);
    let (cid, s) = solids[0].clone();
    let top = s.faces.iter().enumerate().find(|(i, f)| f.plane.is_some_and(|p| p.origin[2].abs() < 1e-9) && s.face_normal(*i).is_some_and(|n| n[2] > 0.999)).map(|(_, f)| f.name).unwrap();
    let plane = context::face_plane_on(&s, cid, top).unwrap();
    h.execute(&mut doc, &AddSketch { element: STUDIO, feature: FeatureId::from_u128(0x4c32), plane: Some(plane) }).unwrap();
    let (studio, c) = context::pending_to_commit(&doc).expect("referenced now");
    assert_eq!((studio, c.id), (STUDIO, 0));
    h.execute(&mut doc, &AddContext { studio, context: c }).unwrap();
    assert!(context::pending_to_commit(&doc).is_none());
    assert_eq!(doc.element(STUDIO).unwrap().contexts.len(), 1);
    assert_eq!(context::with_pending(&doc, STUDIO).len(), 1, "the pending one is the document's now");
    context::set_pending(doc.id, STUDIO, None);
}

#[cfg(feature = "occt")]
#[test]
fn a_linked_parts_context_is_updated_from_the_assembly_with_a_new_version() {
    // MC5: "Block source" (a 50 × 30 × 25 block, saved with V1) is linked into "Block
    // consumer"'s Assembly 1, with a local block of the consumer on top of it (z 25). The linked
    // studio gets a context made in the consumer; the local block moves up 15 mm; Update context
    // from the assembly writes the source's workspace, makes V2 and the instance uses it.
    use cadrs_core::external::{InsertLinked, Resolver, SourceRef};
    use cadrs_core::history_log::HistoryLog;
    use cadrs_core::link_update::{RefSite, Target, UpdateReferences};
    use cadrs_core::samples::gear_cover::DocHistory;
    use cadrs_core::samples::linked_block as lb;
    let store = cadrs_core::Store::new(std::env::temp_dir().join(format!("cadrs-mic-linked-{}-{}", std::process::id(), uuid::Uuid::new_v4())));
    let a = lb::document().unwrap();
    store.create(&a, &cadrs_core::DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(&a, 1_000, "me");
    let v1 = log.create_version("", "", 1_001, "me");
    log.save(&store).unwrap();
    // The consumer: a local Part Studio with a block, and the assembly.
    let mut b = lb::consumer();
    let local = cadrs_core::ElementId::from_u128(0x4c40);
    let mut el = cadrs_core::Element::part_studio("Lid");
    el.id = local;
    b.elements.push(el);
    let mut h = History::default();
    lb::build_in(&mut DocHistory(&mut b, &mut h), local).unwrap();
    let asm = lb::CONSUMER_ASSEMBLY;
    let lid = InstanceId::from_u128(0x4c41);
    h.execute(&mut b, &InsertInstance { element: asm, instance: Instance::new(lid, InstanceSource::Part { element: local, part: lb::PART }, Pose::translation([0.0, 0.0, 25.0])) }).unwrap();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(a.id), lb::STUDIO, v1);
    let snap = res.resolve(r, &b, None).unwrap();
    let block = InstanceId::from_u128(0x4c42);
    let inst = Instance::new(block, InstanceSource::Part { element: snap.root, part: lb::PART }, Pose::IDENTITY);
    h.execute(&mut b, &InsertLinked { element: asm, snapshot: snap, instances: vec![inst], reference: r }).unwrap();
    assert_eq!(context::linked_studio_of(&b, asm, block), Some((a.id, lb::STUDIO)));
    assert_eq!(context::linked_studio_of(&b, asm, lid), None);
    // Edit in context: the snapshot is taken in the consumer and names it.
    let ctx = context::snapshot_linked(&b, asm, block, 0).unwrap();
    assert_eq!(ctx.document, Some(b.id));
    assert_eq!(ctx.parts.len(), 1, "the Lid, not the block itself");
    assert_eq!(ctx.parts[0].pose.translation, [0.0, 0.0, 25.0]);
    // It is created in the source's workspace (a sketch referencing it would do that).
    let mut file = store.load(a.id).unwrap();
    History::default().execute(&mut file.document, &AddContext { studio: lb::STUDIO, context: ctx.clone() }).unwrap();
    store.save(&file.document, &file.meta).unwrap();
    assert_eq!(context::external_status(&b, &ctx), ContextStatus::UpToDate);
    // The Lid goes up 15 mm: an update is available.
    h.execute(&mut b, &MoveInstances { element: asm, poses: vec![(lid, Pose::translation([0.0, 0.0, 40.0]))], label: "Move".into() }).unwrap();
    assert_eq!(context::external_status(&b, &ctx), ContextStatus::OutOfDate);
    // Update context from the assembly.
    let v2 = context::update_linked_context(&mut res, &b, a.id, lb::STUDIO, 0, 3_000, "me").unwrap();
    assert_ne!(v2, v1);
    let file = store.load(a.id).unwrap();
    let now = file.document.element(lb::STUDIO).unwrap().context(0).unwrap().clone();
    assert_eq!(now.parts[0].pose.translation, [0.0, 0.0, 40.0], "the source's workspace took the snapshot");
    assert_eq!(context::external_status(&b, &now), ContextStatus::UpToDate);
    let log = HistoryLog::load(&store, a.id).unwrap().unwrap();
    let v = log.version(v2).unwrap();
    assert!(v.auto() && v.description().contains("Block consumer"), "{v:?}");
    // The primary instance now uses V2.
    let u = cadrs_core::link_update::use_at(&b, RefSite::Instance { element: asm, instance: block }).unwrap();
    let c = cadrs_core::link_update::change_for(&mut res, &b, None, &u, Target::Version(v2)).unwrap().unwrap();
    h.execute(&mut b, &UpdateReferences { changes: vec![c], label: "Update context".into() }).unwrap();
    let link = b.element(asm).unwrap().assembly_model().unwrap().instance(block).unwrap().link.unwrap();
    assert_eq!(link.version_id(), Some(v2));
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn a_new_context_after_a_move_is_up_to_date() {
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let a = add_context(&mut doc, &mut h, p3b9::COVER);
    h.execute(&mut doc, &MoveInstances { element: ASM, poses: vec![(p3b9::CONTEXT_BASE, Pose::translation([15.0, 0.0, 0.0]))], label: "Move".into() }).unwrap();
    let b = add_context(&mut doc, &mut h, p3b9::COVER);
    assert_eq!(status(&doc, a), ContextStatus::OutOfDate);
    assert_eq!(status(&doc, b), ContextStatus::UpToDate);
}

#[test]
fn a_sketch_on_a_context_face_imprints_only_that_context() {
    // Two contexts with the Base 15 mm apart in X: their top faces share the plane z 0. A sketch
    // on the second's face imprints the second's edges only, and references only it.
    let mut doc = p3b9::context_document().unwrap();
    let mut h = History::default();
    let a = add_context(&mut doc, &mut h, p3b9::COVER);
    h.execute(&mut doc, &MoveInstances { element: ASM, poses: vec![(p3b9::CONTEXT_BASE, Pose::translation([15.0, 0.0, 0.0]))], label: "Move".into() }).unwrap();
    let b = add_context(&mut doc, &mut h, p3b9::COVER);
    let c = doc.element(STUDIO).unwrap().context(b).unwrap().clone();
    let (parts, _) = context::parts_of(&doc, &c, |_, f| Some(cadrs_core::rebuild::build(f)));
    let s = &parts[0].solid;
    let top = s.faces.iter().enumerate().find(|(i, f)| f.plane.is_some_and(|p| p.origin[2].abs() < 1e-9) && s.face_normal(*i).is_some_and(|n| n[2] > 0.999)).map(|(_, f)| f.name).unwrap();
    let plane = context::face_plane_on(s, parts[0].feature, top).unwrap();
    let sk = FeatureId::from_u128(0x4c51);
    h.execute(&mut doc, &AddSketch { element: STUDIO, feature: sk, plane: Some(plane) }).unwrap();
    let el = doc.element(STUDIO).unwrap();
    let f = el.feature(sk).unwrap();
    assert_eq!(context::feature_contexts(el, f), [b]);
    let imprint = &f.sketch().unwrap().geometry.imprint;
    assert!(!imprint.is_empty(), "the face's own edges");
    let pa = el.context(a).unwrap().parts[0].id;
    assert!(imprint.iter().all(|i| i.link.is_none_or(|l| FeatureId(l.feature()) != pa)), "nothing from the other context");
}

#[cfg(feature = "occt")]
#[test]
fn the_in_context_fixtures_are_current() {
    // Regenerate with `CADRS_REGENERATE_FIXTURES=1 cargo test -p cadrs_core --test managed_in_context`.
    use cadrs_core::history_log::HistoryLog;
    use cadrs_core::samples::in_context as ic;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let finger = ic::finger_document().unwrap();
    let log = ic::finger_history(&finger);
    for (name, doc, log) in [
        ("mic_slide_standin", ic::slide_document().unwrap(), None),
        ("mic_gripper_finger_standin", finger, Some(log)),
        ("mic_gripper_standin", ic::gripper_document().unwrap(), None),
    ] {
        let path = fixtures.join(format!("{name}.cadrs"));
        let file = ic::file(doc);
        if std::env::var_os("CADRS_REGENERATE_FIXTURES").is_some() {
            std::fs::write(&path, ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap()).unwrap();
            if let Some(l) = &log {
                l.save_path(&fixtures.join(format!("{name}.history.ron"))).unwrap();
            }
        }
        let stored = cadrs_core::Store::load_path(&path).unwrap_or_else(|e| panic!("fixtures/{name}.cadrs: {e}"));
        assert_eq!(stored, file, "fixtures/{name}.cadrs is stale (CADRS_REGENERATE_FIXTURES=1)");
        if let Some(l) = log {
            let back = HistoryLog::load_path(&fixtures.join(format!("{name}.history.ron"))).unwrap();
            assert_eq!(back.entries, l.entries, "fixtures/{name}.history.ron is stale");
        }
    }
}

#[cfg(feature = "occt")]
#[test]
fn the_slide_needs_a_primary_instance_after_carriage_1_is_deleted() {
    // MCX2 on the stand-in: Carriage <1>'s context; Carriage <1> deleted: no update until
    // Carriage <2> is set as primary; then the Rail is 40 mm to the left of the Carriage studio's
    // origin (Carriage <2> is at x 40).
    use cadrs_core::samples::in_context as ic;
    let mut doc = ic::slide_document().unwrap();
    let mut h = History::default();
    let ctx = context::snapshot_as(&doc, ic::SLIDE, ic::CARRIAGE_1, 0).unwrap();
    assert_eq!(ctx.parts.len(), 1, "the Rail (not the other Carriage)");
    assert_eq!(ctx.parts[0].pose.translation, [40.0, 0.0, -10.0]);
    h.execute(&mut doc, &AddContext { studio: ic::CARRIAGE_STUDIO, context: ctx }).unwrap();
    h.execute(&mut doc, &DeleteInstances { element: ic::SLIDE, instances: vec![ic::CARRIAGE_1] }).unwrap();
    let el = doc.element(ic::CARRIAGE_STUDIO).unwrap();
    assert_eq!(context::status(&doc, ic::CARRIAGE_STUDIO, el.context(0).unwrap()), ContextStatus::NoPrimary);
    h.execute(&mut doc, &SetPrimaryInstance { studio: ic::CARRIAGE_STUDIO, contexts: vec![0], instance: ic::CARRIAGE_2 }).unwrap();
    let c = doc.element(ic::CARRIAGE_STUDIO).unwrap().context(0).unwrap().clone();
    let now = context::resnapshot(&doc, ic::CARRIAGE_STUDIO, &c).unwrap();
    assert_eq!(now.parts[0].pose.translation, [-40.0, 0.0, -10.0]);
}
