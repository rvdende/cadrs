//! Explicit mate connectors (P3B.7, `intro-to-assemblies.md` A22–A23, A16.3, X5) on the step
//! stool stand-in (`samples::step_stool`): Part Studio connectors travel with their part (and
//! follow a studio edit by persistent names), Between entities, sketch-curve origins, Realign,
//! editing a mate's connector, the assembly's own connectors, and mates to the Origin.

use cadrs_core::assembly::commands::{AddMateFeature, SetInstancesFixed, SetLocalConnector};
use cadrs_core::assembly::connector::{
    ConnectorEdit, EntityRef, ImplicitPoint, LocalConnector, LocalConnectorId, MateConnector, implicit_points,
};
use cadrs_core::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
use cadrs_core::assembly::{self, InstanceId};
use cadrs_core::commands::{AddFeature, SetExtrude};
use cadrs_core::document::{EdgeRef, FaceRef, FeatureKind};
use cadrs_core::mate::{ConnectorOrigin, MateConnectorFeature};
use cadrs_core::samples::step_stool as ss;
use cadrs_core::{Document, FeatureId, History};

const IN: f64 = 25.4;

fn close3(what: &str, got: [f64; 3], want: [f64; 3]) {
    for k in 0..3 {
        assert!((got[k] - want[k]).abs() < 1e-6, "{what}: got {got:?}, want {want:?}");
    }
}

fn inch(p: [f64; 3]) -> [f64; 3] {
    p.map(|x| x / IN)
}

fn doc() -> (Document, History) {
    (ss::document().unwrap(), History::default())
}

/// The planar face of the Large Frame Bar at z = `z` in (its top).
fn top_face(doc: &Document, z: f64) -> FaceRef {
    let b = cadrs_core::rebuild::build(doc.element(ss::LARGE_STUDIO).unwrap().features());
    let s = &b.part(ss::LARGE_FRAME_BAR).unwrap().solid;
    let f = s
        .faces
        .iter()
        .find(|f| f.plane.is_some() && f.loops.iter().flatten().all(|p| (p[2] / IN - z).abs() < 1e-9))
        .expect("the top face");
    FaceRef { part: ss::LARGE_FRAME_BAR, face: f.name, seed: f.loops[0][0] }
}

fn add_connector(doc: &mut Document, h: &mut History, el: cadrs_core::ElementId, id: FeatureId, x: MateConnectorFeature) {
    h.execute(doc, &AddFeature { element: el, feature: id, base_name: "Mate connector".into(), kind: FeatureKind::MateConnector(x) }).unwrap();
}

#[test]
fn a_part_studio_connector_travels_with_its_part_and_follows_a_studio_edit() {
    // A22.2, X5: a connector on the Large Frame Bar's top face (z 24 in) is carried by the part
    // into its instance; lengthening the bar to 30 in moves it (the face's persistent name).
    let (mut doc, mut h) = doc();
    let mc = FeatureId::from_u128(0x77);
    let top = top_face(&doc, 24.0);
    add_connector(&mut doc, &mut h, ss::LARGE_STUDIO, mc, MateConnectorFeature { origin: Some(ConnectorOrigin::Face(top)), ..Default::default() });
    let solids = ss::solids(&doc);
    let carried = solids[&ss::LARGE].connectors.iter().find(|c| c.feature == mc).expect("carried by the part");
    close3("on the top", inch(carried.frame.origin), [0.0, 0.5, 24.0]);
    // The instance's mate connector on it, and in the assembly (the instance at its place).
    let c = MateConnector::explicit(ss::LARGE, mc, Default::default());
    close3("instance frame", inch(c.local_frame(Some(&solids[&ss::LARGE])).origin), [0.0, 0.5, 24.0]);
    // Only the owner carries it.
    assert!(solids[&ss::BASE].connectors.is_empty());
    // The studio edit: Extrude 1 30 in deep.
    let ext = doc.element(ss::LARGE_STUDIO).unwrap().features().iter().find(|f| f.extrude().is_some()).unwrap().clone();
    let mut e = ext.extrude().unwrap().clone();
    e.depth = 30.0 * IN;
    e.depth_expr = "30 in".into();
    h.execute(&mut doc, &SetExtrude { element: ss::LARGE_STUDIO, feature: ext.id, extrude: e, label: "Depth".into() }).unwrap();
    let solids = ss::solids(&doc);
    close3("followed the edit", inch(c.local_frame(Some(&solids[&ss::LARGE])).origin), [0.0, 0.5, 30.0]);
}

#[test]
fn between_entities_gives_the_midpoint_and_a_sketch_circle_its_centre() {
    // A22.4 (A24.5): the lug hole's inner rim (x −4.5) and the other lug's inner face (x 4.5):
    // (0, −0.5, 23), Z along +X. A22.8 (A24.8): the Hole Positions circle's centre, the same
    // frame. Both owned by their parts (A22.5).
    let (mut doc, mut h) = doc();
    ss::add_connectors_and_hinge(&mut doc, &mut h).unwrap();
    let solids = ss::solids(&doc);
    for (i, f) in [(ss::BASE, ss::BASE_CONNECTOR), (ss::LARGE, ss::LARGE_CONNECTOR)] {
        let c = solids[&i].connectors.iter().find(|c| c.feature == f).expect("owned");
        close3("origin", inch(c.frame.origin), [0.0, -0.5, 23.0]);
        close3("Z", c.frame.normal(), [1.0, 0.0, 0.0]);
    }
    // The hinge holds at 0°: the frames coincide in the assembly.
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
    let m = asm.mate(ss::HINGE).unwrap().mate().unwrap();
    let w = |c: &MateConnector| c.local_frame(Some(&solids[&c.instance])).moved(&asm.instance(c.instance).unwrap().pose);
    close3("coincide", w(&m.connectors[0]).origin, w(&m.connectors[1]).origin);
    // The base frame turns about the hinge only.
    let dofs = assembly::instance_dofs(&cadrs_core::assembly::structure::solver_model(&doc, asm), &solids);
    assert_eq!(dofs[&ss::BASE], 1);
}

#[test]
fn realign_turns_the_secondary_axis_onto_an_edge() {
    // A22.6: a connector on the top face (Z up, X along model X) realigned so X runs along the
    // face's short edge (model Y).
    let (mut doc, mut h) = doc();
    let top = top_face(&doc, 24.0);
    let b = cadrs_core::rebuild::build(doc.element(ss::LARGE_STUDIO).unwrap().features());
    let s = &b.part(ss::LARGE_FRAME_BAR).unwrap().solid;
    let edge = s
        .edges
        .iter()
        .find(|e| e.circle.is_none() && e.points.iter().all(|p| (p[2] / IN - 24.0).abs() < 1e-9 && (p[0] / IN - 6.0).abs() < 1e-9))
        .expect("the short top edge at x 6");
    let mc = FeatureId::from_u128(0x78);
    let x = MateConnectorFeature {
        origin: Some(ConnectorOrigin::Face(top)),
        realign: true,
        secondary_axis: Some(ConnectorOrigin::Edge(EdgeRef { part: ss::LARGE_FRAME_BAR, edge: edge.name, seed: edge.midpoint() })),
        ..Default::default()
    };
    add_connector(&mut doc, &mut h, ss::LARGE_STUDIO, mc, x);
    let f = cadrs_core::rebuild::build(doc.element(ss::LARGE_STUDIO).unwrap().features()).connectors[&mc];
    assert!(f.u[1].abs() > 1.0 - 1e-9, "X along Y: {:?}", f.u);
    close3("Z still up", f.normal(), [0.0, 0.0, 1.0]);
}

#[test]
fn editing_a_mates_connector_realigns_moves_flips_and_reorients() {
    // A23.3–A23.4 on an implicit connector (the top face's centroid of the Large Frame Bar):
    // Realign X to the short edge (Y), Reorient (X → −X… a quarter turn), Flip, Move.
    let (doc, _) = doc();
    let solids = ss::solids(&doc);
    let s = &solids[&ss::LARGE];
    let face = s.faces.iter().find(|f| f.plane.is_some() && f.loops.iter().flatten().all(|p| (p[2] / IN - 24.0).abs() < 1e-9)).unwrap();
    let p = implicit_points(s, &EntityRef::Face(face.name)).into_iter().find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_))).unwrap();
    let mut c = MateConnector::implicit(ss::LARGE, &p);
    assert!((c.local_frame(Some(s)).x[0] - 1.0).abs() < 1e-9);
    let edge = s
        .edges
        .iter()
        .find(|e| e.circle.is_none() && e.points.iter().all(|q| (q[2] / IN - 24.0).abs() < 1e-9 && (q[0] / IN - 6.0).abs() < 1e-9))
        .unwrap();
    c.edit = ConnectorEdit { secondary: Some((EntityRef::Edge(edge.name), [0.0, 1.0, 0.0])), ..Default::default() };
    let f = c.local_frame(Some(s));
    assert!(f.x[1].abs() > 1.0 - 1e-9, "X along the edge: {:?}", f.x);
    // Flip: Z down; Move 1 in along its (flipped) Z: below the face.
    c.flip = true;
    c.edit.translation = [0.0, 0.0, IN];
    let g = c.local_frame(Some(s));
    close3("flipped", g.z, [0.0, 0.0, -1.0]);
    close3("moved", inch(g.origin), [0.0, 0.5, 23.0]);
    // Reorient: X a quarter turn about Z.
    c.reorient = 1;
    let r = c.local_frame(Some(s));
    assert!(r.x[0].abs() > 1.0 - 1e-9, "X turned: {:?}", r.x);
    // A rotation of the Move turns X about Z too.
    c.reorient = 0;
    c.edit.rotation = std::f64::consts::FRAC_PI_2;
    let t = c.local_frame(Some(s));
    assert!(t.x[0].abs() > 1.0 - 1e-9, "rotated: {:?}", t.x);
}

#[test]
fn mates_to_the_origin_hold_and_count_dof() {
    // A16.3: the Large Frame Bar, not fixed, Revolute to the Origin (1 DOF), then Fastened to it
    // (0 DOF; the root shows "fastened to the origin"). Its top face's centroid lands on the
    // Origin.
    let (mut doc, mut h) = doc();
    h.execute(&mut doc, &SetInstancesFixed { element: ss::ASSEMBLY, instances: vec![ss::LARGE], fixed: false }).unwrap();
    let solids = ss::solids(&doc);
    let s = &solids[&ss::LARGE];
    let face = s.faces.iter().find(|f| f.plane.is_some() && f.loops.iter().flatten().all(|p| (p[2] / IN - 24.0).abs() < 1e-9)).unwrap();
    let p = implicit_points(s, &EntityRef::Face(face.name)).into_iter().find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_))).unwrap();
    let model = |t: MateType| {
        let mut asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
        let m = Mate::new(t, MateConnector::implicit(ss::LARGE, &p), MateConnector::origin());
        asm.mates.push(MateFeature::new(MateId::from_u128(0x99), "m", MateKind::Mate(m)));
        asm
    };
    let rev = model(MateType::Revolute);
    let sol = assembly::solve(&rev, &solids, &cadrs_core::assembly::solver::SolveOptions { snap: Some(MateId::from_u128(0x99)), ..Default::default() });
    assert!(sol.converged, "{}", sol.residual);
    let pose = sol.poses.iter().find(|(i, _)| *i == ss::LARGE).unwrap().1;
    close3("on the origin", pose.apply(p.frame.origin), [0.0; 3]);
    let mut solved = rev.clone();
    solved.instance_mut(ss::LARGE).unwrap().pose = pose;
    assert_eq!(assembly::instance_dofs(&solved, &solids)[&ss::LARGE], 1);
    let mut fas = model(MateType::Fastened);
    fas.instance_mut(ss::LARGE).unwrap().pose = pose;
    assert_eq!(assembly::instance_dofs(&fas, &solids)[&ss::LARGE], 0);
    assert_eq!(fas.fastened_to_origin(), vec![ss::LARGE]);
    assert!(rev.fastened_to_origin().is_empty());
    // Through the command layer, with the Origin as an instance.
    let f = fas.mates.last().unwrap().clone();
    h.execute(&mut doc, &AddMateFeature { element: ss::ASSEMBLY, feature: f, poses: vec![(ss::LARGE, pose)] }).unwrap();
    // It holds in the flattened solver model too (top level).
    let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
    let flat = cadrs_core::assembly::structure::solver_model(&doc, asm);
    assert!(flat.mates.iter().any(|m| m.mate().is_some_and(|m| m.connectors[1].is_origin())));
}

#[test]
fn an_assembly_connector_stays_in_the_assembly_and_takes_a_mate() {
    // A22.2 (the assembly's Mate connector tool): a Between connector on the Base Frame Bar
    // instance (the lug rim and the other lug's face), a Revolute from the Large Frame Bar's
    // explicit connector to it: the frame turns about the hinge (1 DOF). The Part Studio has no
    // new feature.
    let (mut doc, mut h) = doc();
    ss::add_connectors_and_hinge(&mut doc, &mut h).unwrap();
    let features_before = doc.element(ss::BASE_STUDIO).unwrap().features().len();
    let solids = ss::solids(&doc);
    let s = &solids[&ss::BASE];
    let rim = ss::circle_edge(s, [-4.5, -0.5, 23.0], 0.5).unwrap();
    let face = ss::x_face(s, 4.5, -1.0).unwrap();
    let p = implicit_points(s, &EntityRef::Edge(rim)).into_iter().find(|p| matches!(p.point, ImplicitPoint::CircleCenter(_))).unwrap();
    let mut c = MateConnector::implicit(ss::BASE, &p);
    c.edit.between = Some(EntityRef::Face(face));
    let f = c.local_frame(Some(s));
    close3("midway", inch(f.origin), [0.0, -0.5, 23.0]);
    let id = LocalConnectorId::from_u128(0x55);
    h.execute(&mut doc, &SetLocalConnector { element: ss::ASSEMBLY, connector: LocalConnector { id, name: "Mate connector 1".into(), connector: c, listed_after: None } }).unwrap();
    assert_eq!(doc.element(ss::BASE_STUDIO).unwrap().features().len(), features_before);
    let mut asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    asm.mates.retain(|m| m.id != ss::HINGE);
    let a = asm.mates.len();
    let large = MateConnector::explicit(ss::LARGE, ss::LARGE_CONNECTOR, Default::default());
    let mut local = MateConnector::at(ss::BASE, f);
    local.anchor = cadrs_core::assembly::connector::ConnectorAnchor::Local { id };
    asm.mates.push(MateFeature::new(MateId::from_u128(0x56), "Revolute 2", MateKind::Mate(Mate::new(MateType::Revolute, large, local))));
    assert_eq!(asm.mates.len(), a + 1);
    let dofs = assembly::instance_dofs(&asm, &solids);
    assert_eq!(dofs[&ss::BASE], 1);
    let _ = InstanceId::ORIGIN;
}

#[test]
fn a_realigned_slot_connector_sets_the_pin_slot_direction() {
    // A23.3: a Pin slot whose slot connector's X runs across the slot (part Y) slides the wrong
    // way; realigned to the slot's edge (part X, the direction kept for when there is no part),
    // the plate slides along the slot. The pin (fixed) at the origin, the plate free.
    use cadrs_core::assembly::connector::ConnectorFrame;
    use cadrs_core::assembly::solver::{self, Pull, SolveOptions};
    use cadrs_core::assembly::{Assembly, Instance, InstanceSource, Pose};
    use cadrs_core::solid::{FaceName, FaceOrigin};
    let src = InstanceSource::Part { element: cadrs_core::ElementId::from_u128(1), part: cadrs_core::PartId::new(FeatureId::from_u128(1), 0) };
    let (plate, pin) = (InstanceId::from_u128(1), InstanceId::from_u128(2));
    let mut a = Instance::new(plate, src, Pose::IDENTITY);
    a.index = 1;
    let mut b = Instance::new(pin, src, Pose::IDENTITY);
    b.index = 2;
    b.fixed = true;
    let mut slot = MateConnector::at(plate, ConnectorFrame::new([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]));
    let pinc = MateConnector::at(pin, ConnectorFrame::new([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]));
    let id = MateId::from_u128(9);
    let model = |slot: MateConnector| Assembly {
        instances: vec![a.clone(), b.clone()],
        mates: vec![MateFeature::new(id, "Pin slot 1", MateKind::Mate(Mate::new(MateType::PinSlot, slot, pinc)))],
        ..Default::default()
    };
    let frame = |c: &MateConnector| c.local_frame(None);
    let slide = |asm: &Assembly| {
        let s = solver::drag(asm, &frame, &[Pull { view: None, instance: plate, point: [0.0, 30.0, 0.0], target: [10.0, 30.0, 0.0] }]);
        s.poses.iter().find(|(i, _)| *i == plate).unwrap().1.translation
    };
    // Across (X along part Y): pulled along part X, the plate doesn't slide that way.
    let t = slide(&model(slot));
    assert!(t[0].abs() < 1.0, "across the slot: {t:?}");
    // Realigned to the slot's edge (part X): it slides along X.
    slot.edit.secondary = Some((EntityRef::Face(FaceName::new(uuid::Uuid::nil(), FaceOrigin::Unnamed { index: 0 })), [1.0, 0.0, 0.0]));
    let asm = model(slot);
    let s = solver::solve(&asm, &frame, &SolveOptions { snap: Some(id), hold_free: true, ..Default::default() });
    assert!(s.converged);
    let mut asm = asm;
    for (i, p) in &s.poses {
        asm.instance_mut(*i).unwrap().pose = *p;
    }
    let t = slide(&asm);
    assert!(t[0] > 5.0 && t[1].abs() < 1e-3, "along the slot: {t:?}");
}
