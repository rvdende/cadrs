//! P3B.8 (`intro-to-assemblies.md` A1.7, A1.8, A2.4, A16.2, X16): rigid Part Studio instances,
//! Named positions (and following one), Exploded views, Replicate and Items.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::bom::{self, BomOptions, BomView};
use cadrs_core::assembly::commands::{DeleteInstances, DeleteMateFeatures, InsertInstance, MoveInstances, SetStudioParts};
use cadrs_core::assembly::explode::{self, ExplodeMotion, ExplodeStep, ExplodedView, ExplodedViewId, SetExplodedView};
use cadrs_core::assembly::items::{DeleteItems, Item, ItemId, SetItem};
use cadrs_core::assembly::mate::{Dof, MateKind};
use cadrs_core::assembly::positions::{self, NamedPositionId, SetFollowPosition, SetNamedPosition};
use cadrs_core::assembly::solver::{Drive, SolveOptions};
use cadrs_core::assembly::structure::{self, occurrences};
use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::commands::{AddElement, NewElementKind};
use cadrs_core::properties::PropertyOwner;
use cadrs_core::samples::{flange as fl, step_stool as ss};
use cadrs_core::{Document, ElementId, History};

const IN: f64 = 25.4;

fn close(what: &str, got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (±{tol})");
}

fn builds(doc: &Document) -> HashMap<ElementId, Arc<cadrs_core::rebuild::Build>> {
    doc.elements
        .iter()
        .filter(|e| e.assembly_model().is_none())
        .map(|e| (e.id, cadrs_core::rebuild::build(e.features())))
        .collect()
}

/// The solver's model and its source solids for the assembly `el`.
fn flat(doc: &Document, el: ElementId) -> (assembly::Assembly, HashMap<InstanceId, Arc<cadrs_core::Solid>>) {
    let b = builds(doc);
    let asm = doc.element(el).unwrap().assembly_model().unwrap();
    (structure::solver_model(doc, asm), assembly::occurrence_solids(doc, asm, |e| b.get(&e).cloned()))
}

/// The CoM (in) of the whole assembly `el`.
fn com(doc: &Document, el: ElementId) -> [f64; 3] {
    let b = builds(doc);
    let asm = doc.element(el).unwrap().assembly_model().unwrap();
    let (parts, props) = assembly::instance_parts(doc, asm, |e| b.get(&e).cloned());
    let ids: Vec<InstanceId> = asm.instances.iter().map(|i| i.id).collect();
    let c = assembly::mass_report(&parts, &props, &ids).unwrap().mass.unwrap().center_of_mass;
    [c.x / IN, c.y / IN, c.z / IN]
}

fn pose_close(a: &Pose, b: &Pose) -> bool {
    (0..3).all(|i| (a.translation[i] - b.translation[i]).abs() < 1e-6 && (0..3).all(|j| (a.rotation[i][j] - b.rotation[i][j]).abs() < 1e-9))
}

// ---------------------------------------------------------------------------------------------
// A2.4 Rigid Part Studio instance

const RIGID: InstanceId = InstanceId::from_u128(0x3b08_9000);

fn stool_with_rigid_studio() -> (Document, History) {
    let mut doc = ss::document().unwrap();
    let mut h = History::default();
    let inst = Instance::studio(RIGID, ss::BASE_STUDIO, vec![ss::BASE_FRAME_BAR, ss::CROSS_BAR, ss::BACK_FOOT], Pose::translation([300.0, 0.0, 0.0]));
    h.execute(&mut doc, &InsertInstance { element: ss::ASSEMBLY, instance: inst }).unwrap();
    (doc, h)
}

#[test]
fn a_rigid_part_studio_instance_moves_as_one() {
    let (mut doc, mut h) = stool_with_rigid_studio();
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let occ: Vec<_> = occurrences(&doc, &asm).into_iter().filter(|o| o.top == RIGID).collect();
    assert_eq!(occ.len(), 3, "one occurrence per part");
    assert_eq!(asm.instance(RIGID).unwrap().name(&assembly::source_part_name(&doc, &InstanceSource::Studio { element: ss::BASE_STUDIO }, None)), "Base Frame Bar <1>");
    // The solver holds its parts together: a rigid group.
    let (model, solids) = flat(&doc, ss::ASSEMBLY);
    assert!(model.mates.iter().any(|f| f.name == "Rigid Part Studio" && f.instances().len() == 3));
    // Dragging one part drags all three.
    let pull = assembly::solver::Pull { instance: occ[1].id, point: [0.0, -50.8, 114.3], target: [0.0, -50.8 + 40.0, 114.3 + 10.0], view: None };
    let sol = assembly::drag(&model, &solids, &[pull]);
    let moved: Vec<Pose> = occ.iter().map(|o| sol.poses.iter().find(|(i, _)| *i == o.id).unwrap().1).collect();
    assert!(!pose_close(&moved[0], &occ[0].pose), "it moved");
    assert!(moved.iter().all(|p| pose_close(p, &moved[0])), "all its parts moved as one");
    h.execute(&mut doc, &MoveInstances { element: ss::ASSEMBLY, poses: sol.changed(&model), label: "Drag".into() }).unwrap();
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert!(pose_close(&asm.instance(RIGID).unwrap().pose, &moved[0]), "the instance takes the placement");
    // Its world mass: the three parts.
    let b = builds(&doc);
    let (parts, _) = assembly::instance_parts(&doc, asm, |e| b.get(&e).cloned());
    assert_eq!(parts.iter().filter(|p| InstanceId::of_part(p.id) == RIGID).count(), 3);
}

#[test]
fn a_rigid_part_studio_instance_is_edited_and_stays_live() {
    let (mut doc, mut h) = stool_with_rigid_studio();
    // Edit: take the Back Foot out.
    h.execute(&mut doc, &SetStudioParts { element: ss::ASSEMBLY, instance: RIGID, parts: vec![ss::BASE_FRAME_BAR, ss::CROSS_BAR] }).unwrap();
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!(occurrences(&doc, asm).iter().filter(|o| o.top == RIGID).count(), 2);
    assert!(h.execute(&mut doc, &SetStudioParts { element: ss::ASSEMBLY, instance: RIGID, parts: vec![] }).is_err(), "it needs a part");
    // Edit: back in.
    h.execute(&mut doc, &SetStudioParts { element: ss::ASSEMBLY, instance: RIGID, parts: vec![ss::BASE_FRAME_BAR, ss::CROSS_BAR, ss::BACK_FOOT] }).unwrap();
    assert_eq!(occurrences(&doc, doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap()).iter().filter(|o| o.top == RIGID).count(), 3);
    // Undo, undo: two parts, then three... one undo step each.
    h.undo(&mut doc).expect("undo");
    assert_eq!(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().instance(RIGID).unwrap().parts.len(), 2);
    // Live: the studio's current rebuild is what the instance shows (a renamed part is renamed).
    h.execute(&mut doc, &cadrs_core::commands::RenamePart { element: ss::BASE_STUDIO, part: ss::CROSS_BAR, name: "Step".into() }).unwrap();
    let b = builds(&doc);
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(&doc, asm, |e| b.get(&e).cloned());
    assert!(parts.iter().any(|p| InstanceId::of_part(p.id) == RIGID && p.name == "Step <1>"), "{:?}", parts.iter().map(|p| &p.name).collect::<Vec<_>>());
    // In the BOM its parts are items.
    let bom = bom::compute(&doc, ss::ASSEMBLY, &BomOptions::default(), &doc.units.clone(), |e| b.get(&e).cloned()).unwrap();
    let cross = bom.rows.iter().find(|r| r.key.owner == PropertyOwner::Part { element: ss::BASE_STUDIO, part: ss::CROSS_BAR }).unwrap();
    assert_eq!(cross.quantity, 2, "the Cross Bar instance and the rigid studio's");
}

#[test]
fn a_rigid_part_studio_needs_a_part_studio_and_parts() {
    let mut doc = ss::document().unwrap();
    let mut h = History::default();
    let bad = Instance::studio(RIGID, ss::ASSEMBLY, vec![ss::CROSS_BAR], Pose::IDENTITY);
    assert!(h.execute(&mut doc, &InsertInstance { element: ss::ASSEMBLY, instance: bad }).is_err());
    let empty = Instance::studio(RIGID, ss::BASE_STUDIO, vec![], Pose::IDENTITY);
    assert!(h.execute(&mut doc, &InsertInstance { element: ss::ASSEMBLY, instance: empty }).is_err());
}

// ---------------------------------------------------------------------------------------------
// Named positions (A1.8) on the Ex4 stool, and following one (A16.2)

const OPEN: NamedPositionId = NamedPositionId::from_u128(0x3b08_0001);
const CLOSED: NamedPositionId = NamedPositionId::from_u128(0x3b08_0002);

/// The hinged stool with "Open" (at −42.5°) and "Closed" (0°) captured, left closed.
fn stool_with_positions() -> (Document, History) {
    let mut doc = ss::hinged_document().unwrap();
    let mut h = History::default();
    let (model, solids) = flat(&doc, ss::ASSEMBLY);
    // Apply limit position: −42.5°.
    let drive = Drive { mate: ss::HINGE, dof: Dof::Angle, value: ss::OPEN_ANGLE.to_radians() };
    let sol = assembly::solve(&model, &solids, &SolveOptions { movers: vec![ss::BASE], snap: Some(ss::HINGE), drives: vec![drive], ..Default::default() });
    h.execute(&mut doc, &MoveInstances { element: ss::ASSEMBLY, poses: sol.changed(&model), label: "Apply limit".into() }).unwrap();
    let (_, solids) = flat(&doc, ss::ASSEMBLY);
    let open = positions::capture(&doc, ss::ASSEMBLY, OPEN, "Open", &solids).unwrap();
    assert_eq!(open.values.len(), 1, "Revolute 1's angle (Fastened 1 has none): {:?}", open.values);
    h.execute(&mut doc, &SetNamedPosition { element: ss::ASSEMBLY, position: open }).unwrap();
    // Reset: 0°.
    let (model, solids) = flat(&doc, ss::ASSEMBLY);
    let drive = Drive { mate: ss::HINGE, dof: Dof::Angle, value: 0.0 };
    let sol = assembly::solve(&model, &solids, &SolveOptions { movers: vec![ss::BASE], snap: Some(ss::HINGE), drives: vec![drive], ..Default::default() });
    h.execute(&mut doc, &MoveInstances { element: ss::ASSEMBLY, poses: sol.changed(&model), label: "Reset".into() }).unwrap();
    let (_, solids) = flat(&doc, ss::ASSEMBLY);
    let closed = positions::capture(&doc, ss::ASSEMBLY, CLOSED, "Closed", &solids).unwrap();
    h.execute(&mut doc, &SetNamedPosition { element: ss::ASSEMBLY, position: closed }).unwrap();
    (doc, h)
}

fn apply(doc: &mut Document, h: &mut History, el: ElementId, id: NamedPositionId) {
    let (model, solids) = flat(doc, el);
    let np = doc.element(el).unwrap().assembly_model().unwrap().named_position(id).unwrap().clone();
    let sol = positions::solve_to(&model, &solids, &np);
    assert!(sol.converged, "{}", sol.residual);
    let poses = sol.changed(&model);
    if !poses.is_empty() {
        h.execute(doc, &MoveInstances { element: el, poses, label: "Apply named position".into() }).unwrap();
    }
}

#[test]
fn named_position_open_restores_the_stool_at_its_limit() {
    let (mut doc, mut h) = stool_with_positions();
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
    let open = asm.named_position(OPEN).unwrap();
    close("Open's hinge angle (deg)", open.value(ss::HINGE, Dof::Angle).unwrap().to_degrees(), ss::OPEN_ANGLE, 1e-6);
    let c = com(&doc, ss::ASSEMBLY);
    assert_eq!(format!("{:.3} {:.3}", c[1], c[2]), "-0.033 11.642", "closed");
    // Apply "Open": the gap list's CoM (0, −2.422, 12.755) in.
    apply(&mut doc, &mut h, ss::ASSEMBLY, OPEN);
    let c = com(&doc, ss::ASSEMBLY);
    // The gap list's closed form (0, −2.4218873, 12.7546071) in, to 1e−4.
    assert!(c[0].abs() < 1e-4, "CoM x {}", c[0]);
    close("CoM y open", c[1], -2.4218873, 1e-4);
    close("CoM z open", c[2], 12.7546071, 1e-4);
    assert_eq!(format!("{:.3} {:.3}", c[1], c[2]), "-2.422 12.755");
    // And back: "Closed".
    apply(&mut doc, &mut h, ss::ASSEMBLY, CLOSED);
    let c = com(&doc, ss::ASSEMBLY);
    close("CoM y closed", c[1], -0.0333131, 1e-4);
    close("CoM z closed", c[2], 11.6421217, 1e-4);
}

#[test]
fn named_positions_update_rename_delete_and_round_trip() {
    let (mut doc, mut h) = stool_with_positions();
    // Rename.
    let mut np = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().named_position(OPEN).unwrap().clone();
    np.name = "Opened".into();
    h.execute(&mut doc, &SetNamedPosition { element: ss::ASSEMBLY, position: np.clone() }).unwrap();
    assert_eq!(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().named_position(OPEN).unwrap().name, "Opened");
    np.name = " ".into();
    assert!(h.execute(&mut doc, &SetNamedPosition { element: ss::ASSEMBLY, position: np }).is_err());
    // Update "Opened" to the current (closed) state: its angle is 0 now.
    let (_, solids) = flat(&doc, ss::ASSEMBLY);
    let now = positions::capture(&doc, ss::ASSEMBLY, OPEN, "Opened", &solids).unwrap();
    h.execute(&mut doc, &SetNamedPosition { element: ss::ASSEMBLY, position: now }).unwrap();
    assert!(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().named_position(OPEN).unwrap().value(ss::HINGE, Dof::Angle).unwrap().abs() < 1e-7);
    h.undo(&mut doc).expect("undo");
    // Round trip through the store format.
    let text = ron::ser::to_string_pretty(&ss::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    let back: cadrs_core::DocumentFile = ron::from_str(&text).unwrap();
    assert_eq!(back.document, doc);
    // Delete.
    h.execute(&mut doc, &positions::DeleteNamedPosition { element: ss::ASSEMBLY, id: OPEN }).unwrap();
    assert_eq!(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().named_positions.len(), 1);
    h.undo(&mut doc).expect("undo");
    assert_eq!(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().named_positions.len(), 2);
}

#[test]
fn a_subassembly_follows_a_named_position_of_its_tab() {
    let (mut doc, mut h) = stool_with_positions();
    let top = ElementId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_0301);
    h.execute(&mut doc, &AddElement { id: top, kind: NewElementKind::Assembly, name: Some("Garage".into()), after: None }).unwrap();
    let sub = InstanceId::from_u128(0x3b08_7000);
    h.execute(&mut doc, &InsertInstance { element: top, instance: Instance::new(sub, InstanceSource::Assembly { element: ss::ASSEMBLY }, Pose::IDENTITY) }).unwrap();
    let closed = com(&doc, top);
    close("closed in the parent", closed[2], 11.6421217, 1e-4);
    // Follow "Open": the stool shows open in the parent, its own tab stays closed.
    h.execute(&mut doc, &SetFollowPosition { element: top, instance: sub, follow: Some(OPEN) }).unwrap();
    let c = com(&doc, top);
    close("CoM y following Open", c[1], -2.4218873, 1e-4);
    close("CoM z following Open", c[2], 12.7546071, 1e-4);
    close("the tab itself", com(&doc, ss::ASSEMBLY)[2], 11.6421217, 1e-4);
    // Lock (follow nothing): the tab's placements again.
    h.execute(&mut doc, &SetFollowPosition { element: top, instance: sub, follow: None }).unwrap();
    close("locked", com(&doc, top)[2], 11.6421217, 1e-4);
    assert!(h.execute(&mut doc, &SetFollowPosition { element: top, instance: sub, follow: Some(NamedPositionId::from_u128(99)) }).is_err());
}

// ---------------------------------------------------------------------------------------------
// Exploded views (A1.8)

#[test]
fn an_exploded_view_leaves_the_assembly_alone_and_its_steps_compose() {
    let mut doc = ss::hinged_document().unwrap();
    let mut h = History::default();
    let before = doc.clone();
    let id = ExplodedViewId::from_u128(0x3b08_0003);
    let mut v = ExplodedView::new(id, explode::next_name(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap()));
    assert_eq!(v.name, "Exploded view 1");
    v.steps.push(ExplodeStep { instances: vec![ss::CROSS], motion: ExplodeMotion::Translate { direction: [0.0, -1.0, 0.0], distance: 100.0 } });
    v.steps.push(ExplodeStep { instances: vec![ss::CROSS, ss::BASE], motion: ExplodeMotion::Translate { direction: [0.0, 0.0, -1.0], distance: 50.0 } });
    let axis = [0.0, ss::HINGE_AXIS[0] * IN, ss::HINGE_AXIS[1] * IN];
    v.steps.push(ExplodeStep { instances: vec![ss::BASE], motion: ExplodeMotion::Rotate { point: axis, axis: [1.0, 0.0, 0.0], angle: -0.5 } });
    h.execute(&mut doc, &SetExplodedView { element: ss::ASSEMBLY, view: v.clone() }).unwrap();
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    // Positions untouched: the instances and mates are as they were.
    assert_eq!(asm.instances, before.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().instances);
    assert_eq!(asm.mates, before.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().mates);
    // 0 %: nothing moves.
    assert!(explode::exploded_poses(&asm, &v, 0.0).is_empty());
    let cross0 = asm.instance(ss::CROSS).unwrap().pose;
    let base0 = asm.instance(ss::BASE).unwrap().pose;
    // A third: the first step done.
    let p = explode::exploded_poses(&asm, &v, 1.0 / 3.0);
    let cross = p.iter().find(|(i, _)| *i == ss::CROSS).unwrap().1;
    close("cross y", cross.translation[1] - cross0.translation[1], -100.0, 1e-9);
    assert!(!p.iter().any(|(i, _)| *i == ss::BASE));
    // Half: step 2 half way.
    let p = explode::exploded_poses(&asm, &v, 0.5);
    let cross = p.iter().find(|(i, _)| *i == ss::CROSS).unwrap().1;
    close("cross z", cross.translation[2] - cross0.translation[2], -25.0, 1e-9);
    // 100 %: the steps compose (the Cross Bar: −100 y, then −50 z; the base: −50 z, then turned).
    let p = explode::exploded_poses(&asm, &v, 1.0);
    let cross = p.iter().find(|(i, _)| *i == ss::CROSS).unwrap().1;
    close("cross y", cross.translation[1] - cross0.translation[1], -100.0, 1e-9);
    close("cross z", cross.translation[2] - cross0.translation[2], -50.0, 1e-9);
    let base = p.iter().find(|(i, _)| *i == ss::BASE).unwrap().1;
    let want = base0.then(&Pose::translation([0.0, 0.0, -50.0])).then(&Pose::rotation_about(axis, [1.0, 0.0, 0.0], -0.5));
    assert!(pose_close(&base, &want), "{base:?} vs {want:?}");
    // Trails: one line per instance per step played, the turn drawn as an arc.
    let t = explode::trails(&asm, &v, 1.0, |_| [0.0, 0.0, 0.0]);
    assert_eq!(t.len(), 4);
    assert_eq!(t[3].len(), 17);
    // Delete and undo.
    h.execute(&mut doc, &explode::DeleteExplodedView { element: ss::ASSEMBLY, id }).unwrap();
    assert!(doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().exploded_views.is_empty());
    h.undo(&mut doc).expect("undo");
    // Deleting an instance takes it out of the steps.
    h.execute(&mut doc, &DeleteInstances { element: ss::ASSEMBLY, instances: vec![ss::CROSS] }).unwrap();
    let v = &doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().exploded_views[0];
    assert_eq!(v.steps.len(), 2, "the Cross Bar's own step went");
}

// ---------------------------------------------------------------------------------------------
// Replicate (X16)

#[test]
fn replicate_puts_a_screw_on_six_matching_holes() {
    let mut doc = fl::document().unwrap();
    let mut h = History::default();
    let seed = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().mate(fl::FASTENED_1).unwrap().mate().unwrap().clone();
    let s = fl::solids(&doc);
    let targets = cadrs_core::assembly::replicate::matching_targets(&s[&fl::FLANGE], &seed.connectors[1]);
    assert_eq!(targets.len(), 6, "the six bolt-circle holes");
    let id = fl::replicate(&mut doc, &mut h).unwrap();
    let asm = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let copies: Vec<&Instance> = asm.instances.iter().filter(|i| i.replicate == Some(id)).collect();
    assert_eq!(copies.len(), 6, "6 instances");
    assert_eq!(copies.iter().map(|i| i.index).collect::<Vec<_>>(), vec![2, 3, 4, 5, 6, 7]);
    // One feature row; the solver sees 6 copied mates.
    assert_eq!(asm.mates.len(), 2);
    let expanded = cadrs_core::assembly::replicate::expand(&asm);
    assert_eq!(expanded.mates.iter().filter(|m| m.name.starts_with("Replicate 1")).count(), 6, "6 mates");
    // Every copy sits on its hole: the mates hold where they are.
    let (model, solids) = flat(&doc, fl::ASSEMBLY);
    let sol = assembly::solve(&model, &solids, &Default::default());
    assert!(sol.converged, "{}", sol.residual);
    assert!(sol.changed(&model).is_empty(), "{:?}", sol.changed(&model));
    let mut centres: Vec<[f64; 2]> = copies.iter().map(|i| [i.pose.translation[0] / IN, i.pose.translation[1] / IN]).collect();
    centres.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let mut want: Vec<[f64; 2]> = fl::hole_centres()[1..].to_vec();
    want.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    for (c, w) in centres.iter().zip(&want) {
        close("x", c[0], w[0], 1e-9);
        close("y", c[1], w[1], 1e-9);
    }
    assert!(copies.iter().all(|i| (i.pose.translation[2] / IN - fl::THICKNESS).abs() < 1e-9), "on the flange's top");
    // The DOF: every copy fully held.
    let dofs = assembly::instance_dofs(&model, &solids);
    assert!(copies.iter().all(|i| dofs.get(&i.id).copied().unwrap_or(0) == 0), "{dofs:?}");
    // Edit: fewer targets (the first two).
    let f = asm.mate(id).unwrap().clone();
    let MateKind::Replicate(r) = &f.kind else { panic!() };
    let mut r2 = r.clone();
    r2.targets.truncate(2);
    r2.instances.truncate(2);
    let kept: Vec<Instance> = asm.instances.iter().filter(|i| r2.instances.contains(&i.id)).cloned().collect();
    let f2 = cadrs_core::assembly::mate::MateFeature { kind: MateKind::Replicate(r2), ..f.clone() };
    h.execute(&mut doc, &cadrs_core::assembly::replicate::SetReplicate { element: fl::ASSEMBLY, feature: f2, instances: kept }).unwrap();
    assert_eq!(doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().instances.len(), 4);
    h.undo(&mut doc).expect("undo");
    // Deleting a copy leaves five; deleting the feature takes its copies.
    h.execute(&mut doc, &DeleteInstances { element: fl::ASSEMBLY, instances: vec![copies[0].id] }).unwrap();
    let a = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!(a.mate(id).unwrap().replicate().unwrap().instances.len(), 5);
    h.undo(&mut doc).expect("undo");
    h.execute(&mut doc, &DeleteMateFeatures { element: fl::ASSEMBLY, mates: vec![id] }).unwrap();
    assert_eq!(doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().instances.len(), 2);
    h.undo(&mut doc).expect("undo");
    // Deleting the seed takes the feature and its copies.
    h.execute(&mut doc, &DeleteInstances { element: fl::ASSEMBLY, instances: vec![fl::SCREW] }).unwrap();
    let a = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!((a.instances.len(), a.mates.len()), (1, 0));
    // The BOM counts 7 screws.
    h.undo(&mut doc).expect("undo");
    let b = builds(&doc);
    let bom = bom::compute(&doc, fl::ASSEMBLY, &BomOptions::default(), &doc.units.clone(), |e| b.get(&e).cloned()).unwrap();
    let screws = bom.rows.iter().find(|r| r.key.owner == PropertyOwner::Part { element: fl::SCREW_STUDIO, part: fl::SCREW_PART }).unwrap();
    assert_eq!(screws.quantity, 7);
}

// ---------------------------------------------------------------------------------------------
// Items (A1.7)

#[test]
fn items_are_rows_of_the_bom() {
    let mut doc = fl::document().unwrap();
    let mut h = History::default();
    let glue = ItemId::from_u128(0x3b08_0004);
    let mut item = Item::new(glue, "Thread locker", 2);
    item.properties.part_number = Some("TL-242".into());
    item.properties.unit_of_measure = Some("Liter".into());
    h.execute(&mut doc, &SetItem { element: fl::ASSEMBLY, item: item.clone() }).unwrap();
    assert!(h.execute(&mut doc, &SetItem { element: fl::ASSEMBLY, item: Item::new(ItemId::new(), " ", 1) }).is_err());
    assert!(h.execute(&mut doc, &SetItem { element: fl::ASSEMBLY, item: Item::new(ItemId::new(), "Label", 0) }).is_err());
    let b = builds(&doc);
    for view in [BomView::Structured, BomView::Flattened] {
        let mut settings = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().bom.clone();
        settings.view = view;
        let bom = bom::compute_with(&doc, fl::ASSEMBLY, &settings, &BomOptions::default(), &doc.units.clone(), &mut |e| b.get(&e).cloned()).unwrap();
        let row = bom.rows.iter().find(|r| r.key.owner == PropertyOwner::Item { element: fl::ASSEMBLY, item: glue }).expect("the item's row");
        assert_eq!(row.quantity, 2);
        assert_eq!(row.item.as_deref(), Some("3"), "after Flange and Screw");
        assert!(row.cells.iter().any(|c| c == "Thread locker"), "{:?}", row.cells);
        assert!(row.cells.iter().any(|c| c == "TL-242"), "{:?}", row.cells);
    }
    // Its unit of measure is the BOM's Unit of measure cell.
    let owner = PropertyOwner::Item { element: fl::ASSEMBLY, item: glue };
    assert_eq!(cadrs_core::properties::text(&doc, owner, cadrs_core::properties::PropertyKey::UnitOfMeasure, None), "Liter");
    // A BOM edit writes the item's property (two-way).
    h.execute(
        &mut doc,
        &cadrs_core::properties::SetProperties {
            owners: vec![PropertyOwner::Item { element: fl::ASSEMBLY, item: glue }],
            values: vec![(cadrs_core::properties::PropertyKey::Description, cadrs_core::properties::PropertyValue::Text("Medium strength".into()))],
            label: "Edit".into(),
        },
    )
    .unwrap();
    assert_eq!(doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().item(glue).unwrap().properties.description.as_deref(), Some("Medium strength"));
    // Edit the quantity; delete.
    item.quantity = 5;
    h.execute(&mut doc, &SetItem { element: fl::ASSEMBLY, item }).unwrap();
    assert_eq!(doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().item(glue).unwrap().quantity, 5);
    h.execute(&mut doc, &DeleteItems { element: fl::ASSEMBLY, items: vec![glue] }).unwrap();
    assert!(doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().items.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Fixtures

fn fixture_current(name: &str, doc: Document) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/{name}.cadrs"));
    let text = ron::ser::to_string_pretty(&ss::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/{name}.cadrs is out of date");
}

#[test]
fn the_p3b8_fixtures_are_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test assembly_p3b8`.
    fixture_current("flange_standin", fl::document().unwrap());
    fixture_current("step_stool_hinged", ss::hinged_document().unwrap());
}
