//! P3B.9 (`intro-to-assemblies.md` A1.3, A1.4, X15, X16): relations between mates (Gear, Rack
//! and pinion, Screw, Linear), Check interference, Replace instances, Edit in context, Where
//! used and Add mate connector to instance origin.

use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateType, dof_value};
use cadrs_core::assembly::relation::{Relation, RelationType};
use cadrs_core::assembly::solver::{self, Drive, Pull, SolveOptions};
use cadrs_core::assembly::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::{ElementId, FeatureId, PartId};

use std::f64::consts::{PI, TAU};

fn src() -> InstanceSource {
    InstanceSource::Part { element: ElementId::from_u128(1), part: PartId::new(FeatureId::from_u128(1), 0) }
}

fn inst(n: u128, fixed: bool) -> Instance {
    let mut i = Instance::new(InstanceId::from_u128(n), src(), Pose::IDENTITY);
    i.index = n as u32;
    i.fixed = fixed;
    i
}

const GROUND: u128 = 1;

/// A mate of `t` between instance `n` (its origin frame, Z along `z`) and the fixed ground at
/// `at`, named `m<n>`.
fn mate_to_ground(n: u128, t: MateType, at: [f64; 3], z: [f64; 3]) -> MateFeature {
    let f = ConnectorFrame::new([0.0; 3], z, [1.0, 0.0, 0.0]);
    let g = ConnectorFrame::new(at, z, [1.0, 0.0, 0.0]);
    let m = Mate::new(t, MateConnector::at(InstanceId::from_u128(n), f), MateConnector::at(InstanceId::from_u128(GROUND), g));
    MateFeature::new(MateId::from_u128(n), format!("m{n}"), MateKind::Mate(m))
}

fn relation(n: u128, r: Relation) -> MateFeature {
    MateFeature::new(MateId::from_u128(n), format!("r{n}"), MateKind::Relation(r))
}

/// The ground and instances 2, 3 placed where their mates put them at zero.
fn assembly(mates: Vec<MateFeature>) -> Assembly {
    let mut asm = Assembly { instances: vec![inst(GROUND, true), inst(2, false), inst(3, false)], mates, ..Default::default() };
    let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
    assert!(s.converged, "{}", s.residual);
    apply(&mut asm, &s);
    asm
}

fn frame_of(c: &MateConnector) -> ConnectorFrame {
    c.local_frame(None)
}

fn apply(asm: &mut Assembly, s: &solver::Solution) {
    for (id, p) in &s.poses {
        asm.instance_mut(*id).unwrap().pose = *p;
    }
}

/// A mate's position along `dof` at the assembly's placements.
fn value(asm: &Assembly, mate: u128, dof: Dof) -> f64 {
    let m = asm.mate(MateId::from_u128(mate)).unwrap().mate().unwrap();
    let w = |c: &MateConnector| c.local_frame(None).moved(&asm.instance(c.instance).unwrap().pose);
    dof_value(&w(&m.connectors[0]), &m.target(&w(&m.connectors[1])), dof)
}

/// Drives `mate`'s `dof` to `to` in `steps` steps (as Animate does), solving each from the last.
fn drive(asm: &mut Assembly, mate: u128, dof: Dof, from: f64, to: f64, steps: usize) {
    for k in 1..=steps {
        let v = from + (to - from) * k as f64 / steps as f64;
        let opts = SolveOptions { drives: vec![Drive { mate: MateId::from_u128(mate), dof, value: v }], ..Default::default() };
        let s = solver::solve(asm, &frame_of, &opts);
        assert!(s.converged, "step {k}: {}", s.residual);
        apply(asm, &s);
    }
}

fn close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-6, "{what}: got {got}, want {want}");
}

#[test]
fn a_two_to_one_gear_turns_the_driven_revolute_back_half_as_far() {
    // Two gears on Revolutes about +Z, 30 mm apart; Gear 2 : 1 from m2 (the driver) to m3.
    let gear = Relation { ratio: (2.0, 1.0), ..Relation::new(RelationType::Gear, vec![MateId::from_u128(2), MateId::from_u128(3)]) };
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Revolute, [0.0; 3], [0.0, 0.0, 1.0]),
        mate_to_ground(3, MateType::Revolute, [30.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        relation(4, gear.clone()),
    ]);
    // The driver turns 90°: the driven one −45°.
    drive(&mut asm, 2, Dof::Angle, 0.0, PI / 2.0, 1);
    close("driver", value(&asm, 2, Dof::Angle), PI / 2.0);
    close("driven", value(&asm, 3, Dof::Angle), -PI / 4.0);
    // Two gears, one motion between them: each keeps 1 DOF (the relation takes one of the two).
    let dofs = solver::dof_counts(&asm, &frame_of);
    assert_eq!(dofs[&InstanceId::from_u128(2)], 1);
    assert_eq!(dofs[&InstanceId::from_u128(3)], 1);
    // A whole turn of the driver (as Animate plays it): the driven gear half a turn back.
    drive(&mut asm, 2, Dof::Angle, PI / 2.0, TAU, 16);
    close("driven after a turn", value(&asm, 3, Dof::Angle).abs(), PI);
    // Reverse direction: the same way round.
    let mut rev = gear;
    rev.reverse = true;
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Revolute, [0.0; 3], [0.0, 0.0, 1.0]),
        mate_to_ground(3, MateType::Revolute, [30.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        relation(4, rev),
    ]);
    drive(&mut asm, 2, Dof::Angle, 0.0, PI / 2.0, 1);
    close("reversed", value(&asm, 3, Dof::Angle), PI / 4.0);
    // Driving the second gear moves the first (a relation works both ways).
    drive(&mut asm, 3, Dof::Angle, PI / 4.0, -PI / 8.0, 3);
    close("back-driven", value(&asm, 2, Dof::Angle), -PI / 4.0);
}

#[test]
fn a_drag_turns_both_gears() {
    // A16.4 with a relation: pulling the driver's rim turns it, and the driven gear follows.
    let gear = Relation { ratio: (2.0, 1.0), ..Relation::new(RelationType::Gear, vec![MateId::from_u128(2), MateId::from_u128(3)]) };
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Revolute, [0.0; 3], [0.0, 0.0, 1.0]),
        mate_to_ground(3, MateType::Revolute, [30.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        relation(4, gear),
    ]);
    let pull = Pull { instance: InstanceId::from_u128(2), point: [10.0, 0.0, 0.0], target: [7.0, 7.0, 0.0], view: None };
    let s = solver::drag(&asm, &frame_of, &[pull]);
    apply(&mut asm, &s);
    let (a, b) = (value(&asm, 2, Dof::Angle), value(&asm, 3, Dof::Angle));
    assert!(a.abs() > 0.3, "the driver turned: {a}");
    close("the driven gear followed", b, -a / 2.0);
}

#[test]
fn rack_and_pinion_moves_two_pi_r_per_turn() {
    // A pinion of pitch radius r = 12 mm on a Revolute about +Z; the rack on a Slider along +X.
    let r = 12.0;
    let rp = Relation { distance: TAU * r, ..Relation::new(RelationType::RackPinion, vec![MateId::from_u128(2), MateId::from_u128(3)]) };
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Revolute, [0.0; 3], [0.0, 0.0, 1.0]),
        mate_to_ground(3, MateType::Slider, [0.0, -r, 0.0], [1.0, 0.0, 0.0]),
        relation(4, rp.clone()),
    ]);
    // A quarter turn: πr/2.
    drive(&mut asm, 2, Dof::Angle, 0.0, PI / 2.0, 2);
    close("a quarter turn", value(&asm, 3, Dof::Z), PI * r / 2.0);
    // On to a whole turn: 2πr (the pinion's angle is back at 0).
    drive(&mut asm, 2, Dof::Angle, PI / 2.0, TAU, 12);
    close("a whole turn", value(&asm, 3, Dof::Z), TAU * r);
    close("the pinion", value(&asm, 2, Dof::Angle).abs() % TAU, 0.0);
    // Driving the rack back turns the pinion back.
    drive(&mut asm, 3, Dof::Z, TAU * r, TAU * r - r, 2);
    close("the pinion turned back 1 rad", value(&asm, 2, Dof::Angle), -1.0);
    // Reverse direction: the rack the other way.
    let mut rev = rp;
    rev.reverse = true;
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Revolute, [0.0; 3], [0.0, 0.0, 1.0]),
        mate_to_ground(3, MateType::Slider, [0.0, -r, 0.0], [1.0, 0.0, 0.0]),
        relation(4, rev),
    ]);
    drive(&mut asm, 2, Dof::Angle, 0.0, PI / 2.0, 2);
    close("reversed", value(&asm, 3, Dof::Z), -PI * r / 2.0);
}

#[test]
fn a_screw_moves_one_pitch_per_turn() {
    // A nut on a Cylindrical about +Z with a 1.5 mm pitch: two turns, 3 mm.
    let pitch = 1.5;
    let screw = Relation { distance: pitch, ..Relation::new(RelationType::Screw, vec![MateId::from_u128(2)]) };
    let mut asm = assembly(vec![mate_to_ground(2, MateType::Cylindrical, [0.0; 3], [0.0, 0.0, 1.0]), relation(4, screw.clone())]);
    // The cylindrical keeps one DOF (turn and travel together).
    assert_eq!(solver::dof_counts(&asm, &frame_of)[&InstanceId::from_u128(2)], 1);
    drive(&mut asm, 2, Dof::Angle, 0.0, 2.0 * TAU, 24);
    close("two turns", value(&asm, 2, Dof::Z), 2.0 * pitch);
    // Pushed along, it turns: 0.75 mm back is half a turn back.
    drive(&mut asm, 2, Dof::Z, 2.0 * pitch, 2.0 * pitch - 0.75, 3);
    close("half a turn back", value(&asm, 2, Dof::Angle).abs(), PI);
    // A left-hand thread (Reverse) goes the other way.
    let mut left = screw;
    left.reverse = true;
    let mut asm = assembly(vec![mate_to_ground(2, MateType::Cylindrical, [0.0; 3], [0.0, 0.0, 1.0]), relation(4, left)]);
    drive(&mut asm, 2, Dof::Angle, 0.0, TAU, 12);
    close("left hand", value(&asm, 2, Dof::Z), -pitch);
}

#[test]
fn a_linear_relation_scales_the_travel() {
    // Two sliders (along +X and along +Y); Linear with ratio 3 (Reverse: −3).
    let lin = Relation { ratio: (1.0, 3.0), ..Relation::new(RelationType::Linear, vec![MateId::from_u128(2), MateId::from_u128(3)]) };
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Slider, [0.0; 3], [1.0, 0.0, 0.0]),
        mate_to_ground(3, MateType::Slider, [0.0, 40.0, 0.0], [0.0, 1.0, 0.0]),
        relation(4, lin.clone()),
    ]);
    drive(&mut asm, 2, Dof::Z, 0.0, 10.0, 1);
    close("second slider", value(&asm, 3, Dof::Z), 30.0);
    let mut rev = lin;
    rev.reverse = true;
    let mut asm = assembly(vec![
        mate_to_ground(2, MateType::Slider, [0.0; 3], [1.0, 0.0, 0.0]),
        mate_to_ground(3, MateType::Slider, [0.0, 40.0, 0.0], [0.0, 1.0, 0.0]),
        relation(4, rev),
    ]);
    drive(&mut asm, 2, Dof::Z, 0.0, 10.0, 1);
    close("reversed", value(&asm, 3, Dof::Z), -30.0);
}

#[test]
fn relations_check_their_mates() {
    let mates = vec![
        mate_to_ground(2, MateType::Revolute, [0.0; 3], [0.0, 0.0, 1.0]),
        mate_to_ground(3, MateType::Slider, [0.0; 3], [1.0, 0.0, 0.0]),
        mate_to_ground(5, MateType::Fastened, [0.0; 3], [1.0, 0.0, 0.0]),
    ];
    let ids = |v: &[u128]| v.iter().map(|n| MateId::from_u128(*n)).collect::<Vec<_>>();
    let ok = |t: RelationType, v: &[u128]| cadrs_core::assembly::relation::check(&Relation::new(t, ids(v)), &mates).is_none();
    assert!(ok(RelationType::RackPinion, &[2, 3]));
    assert!(!ok(RelationType::RackPinion, &[3, 2]), "the pinion first");
    assert!(!ok(RelationType::Gear, &[2, 3]), "a slider doesn't turn");
    assert!(!ok(RelationType::Gear, &[2, 2]), "two different mates");
    assert!(!ok(RelationType::Linear, &[3, 5]), "a Fastened doesn't slide");
    assert!(!ok(RelationType::Screw, &[2]), "Screw takes a Cylindrical");
    assert!(!ok(RelationType::Gear, &[2]), "two mates");
}

// ---------------------------------------------------------------------------------------------
// Documents: interference, replace, edit in context, where used

mod documents {
    use std::collections::HashMap;
    use std::sync::Arc;

    use cadrs_core::assembly::commands::{AddMateFeature, MoveInstances};
    use cadrs_core::assembly::context::{self, SetStudioContext};
    use cadrs_core::assembly::interference;
    use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
    use cadrs_core::assembly::relation::{Relation, RelationType};
    use cadrs_core::assembly::replace::{self, ReplaceInstances};
    use cadrs_core::assembly::{self, InstanceId, InstanceSource, Pose, structure};
    use cadrs_core::commands::{AddExtrude, AddSketch, EditSketch, SetExtrude};
    use cadrs_core::links::LinkContext;
    use cadrs_core::samples::{flange as fl, p3b9};
    use cadrs_core::{BooleanOp, Document, ElementId, ExtrudeFeature, FeatureId, History};
    use cadrs_sketch::{Link, SketchOp};

    fn builds(doc: &Document) -> HashMap<ElementId, Arc<cadrs_core::rebuild::Build>> {
        doc.elements
            .iter()
            .filter(|e| e.assembly_model().is_none())
            .map(|e| (e.id, cadrs_core::rebuild::build(&e.active_features())))
            .collect()
    }

    #[test]
    fn two_overlapping_boxes_report_their_common_volume() {
        // X15 Check interference: 10 × 20 × 15 = 3000 mm³ for the blocks, π(5² − 4²)·10 = 90π
        // for the Ø10 pin in the Ø8 hole of the 10 mm plate; the plate and the blocks are apart.
        let doc = p3b9::interference_document().unwrap();
        let asm = doc.element(p3b9::INTERFERENCE_ASSEMBLY).unwrap().assembly_model().unwrap();
        let items = interference::items(&doc, asm);
        assert_eq!(items.len(), 4);
        let clashes = interference::check_now(items.clone(), vec![]).unwrap();
        assert_eq!(clashes.len(), 2, "{clashes:?}");
        let rel = |got: f64, want: f64| (got - want).abs() <= 1e-6 * want;
        assert!(rel(clashes[0].volume, p3b9::BLOCKS_OVERLAP), "blocks: {}", clashes[0].volume);
        assert_eq!([clashes[0].a, clashes[0].b], [p3b9::BLOCK_1.part_id(), p3b9::BLOCK_2.part_id()]);
        assert!(rel(clashes[1].volume, p3b9::PIN_OVERLAP), "pin: {} vs {}", clashes[1].volume, p3b9::PIN_OVERLAP);
        // Only the pairs of the instances asked about.
        let only = interference::check_now(items, vec![p3b9::PIN]).unwrap();
        assert_eq!(only.len(), 1);
        assert!(rel(only[0].volume, p3b9::PIN_OVERLAP));
        // Moved apart, no interference.
        let mut doc2 = doc.clone();
        let mut h = History::default();
        h.execute(&mut doc2, &MoveInstances { element: p3b9::INTERFERENCE_ASSEMBLY, poses: vec![(p3b9::BLOCK_2, Pose::translation([0.0, 60.0, 0.0]))], label: "Move".into() }).unwrap();
        let asm2 = doc2.element(p3b9::INTERFERENCE_ASSEMBLY).unwrap().assembly_model().unwrap();
        assert!(interference::check_now(interference::items(&doc2, asm2), vec![p3b9::BLOCK_2]).unwrap().is_empty());
    }

    #[test]
    fn an_assembly_exports_its_parts_where_they_are() {
        // X15 Export…: the four parts in one STEP file, named as their instances.
        let doc = p3b9::interference_document().unwrap();
        let asm = doc.element(p3b9::INTERFERENCE_ASSEMBLY).unwrap().assembly_model().unwrap();
        let items: Vec<(interference::Item, String)> =
            interference::items(&doc, asm).into_iter().enumerate().map(|(k, it)| (it, format!("Part {}", k + 1))).collect();
        let files = interference::export_step(items.clone(), false, false).wait().unwrap().unwrap();
        assert_eq!(files.len(), 1);
        let text = String::from_utf8_lossy(&files[0].bytes);
        for k in 1..=4 {
            assert!(text.contains(&format!("'Part {k}'")), "product Part {k} named");
        }
        // One file per part.
        let files = interference::export_step(items, false, true).wait().unwrap().unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(files[1].part.as_deref(), Some("Part 2"));
    }

    #[test]
    fn replace_keeps_the_mates_on_matching_faces() {
        let mut doc = p3b9::replace_document().unwrap();
        let mut h = History::default();
        let b = builds(&doc);
        let asm = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
        let old = assembly::source_solids(&asm, |e| b.get(&e).cloned());
        let long = b[&p3b9::LONG_SCREW_STUDIO].part(p3b9::LONG_SCREW_PART).unwrap().solid.clone();
        let dowel = b[&p3b9::DOWEL_STUDIO].part(p3b9::DOWEL_PART).unwrap().solid.clone();
        // The Long Screw has the same Ø0.25 rim under its head: Fastened 1 is kept.
        let (kept, dropped) = replace::plan(&asm, &[fl::SCREW], &old, &long);
        assert_eq!(kept.len(), 1);
        assert!(dropped.is_empty());
        // The Dowel (Ø0.3, no head) has nothing that matches: Fastened 1 goes.
        let (k2, d2) = replace::plan(&asm, &[fl::SCREW], &old, &dowel);
        assert!(k2.is_empty());
        assert_eq!(d2, vec![fl::FASTENED_1]);
        // Replace: the instance is a Long Screw <1>, where the screw was, and its mate holds there.
        let source = InstanceSource::Part { element: p3b9::LONG_SCREW_STUDIO, part: p3b9::LONG_SCREW_PART };
        h.execute(&mut doc, &ReplaceInstances { element: fl::ASSEMBLY, instances: vec![fl::SCREW], source, mates: kept, dropped }).unwrap();
        let asm = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
        let inst = asm.instance(fl::SCREW).unwrap();
        assert_eq!(inst.source, source);
        assert_eq!(inst.name(&assembly::source_part_name(&doc, &inst.source, None)), "Long Screw <1>");
        assert_eq!(asm.mates.len(), 1);
        let b = builds(&doc);
        let solids = assembly::source_solids(&asm, |e| b.get(&e).cloned());
        let sol = assembly::solve(&asm, &solids, &Default::default());
        assert!(sol.converged && sol.changed(&asm).is_empty(), "the mate holds where the screw was: {:?}", sol.changed(&asm));
        // Undo: the screw again, with its mate.
        h.undo(&mut doc).unwrap();
        let asm = doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap();
        assert_eq!(asm.instance(fl::SCREW).unwrap().source.element(), fl::SCREW_STUDIO);
        // Replaced by the Dowel: its mate is removed.
        let source = InstanceSource::Part { element: p3b9::DOWEL_STUDIO, part: p3b9::DOWEL_PART };
        h.execute(&mut doc, &ReplaceInstances { element: fl::ASSEMBLY, instances: vec![fl::SCREW], source, mates: vec![], dropped: d2 }).unwrap();
        assert!(doc.element(fl::ASSEMBLY).unwrap().assembly_model().unwrap().mates.is_empty());
    }

    #[test]
    fn a_relation_goes_with_its_mate_and_undo_restores_it() {
        let mut doc = p3b9::relations_document().unwrap();
        let mut h = History::default();
        let asm_el = p3b9::RELATIONS_ASSEMBLY;
        let gear = Relation { ratio: (2.0, 1.0), ..Relation::new(RelationType::Gear, vec![p3b9::REVOLUTE_1, p3b9::REVOLUTE_2]) };
        let id = MateId::from_u128(77);
        h.execute(&mut doc, &AddMateFeature { element: asm_el, feature: MateFeature::new(id, "Gear 1", MateKind::Relation(gear)), poses: vec![] }).unwrap();
        // A relation on a slider is refused.
        let bad = Relation::new(RelationType::Gear, vec![p3b9::REVOLUTE_1, p3b9::SLIDER_1]);
        assert!(h.execute(&mut doc, &AddMateFeature { element: asm_el, feature: MateFeature::new(MateId::from_u128(78), "Gear 2", MateKind::Relation(bad)), poses: vec![] }).is_err());
        // Its gears turn together: drive Revolute 1 90°, Revolute 2 −45°.
        let b = builds(&doc);
        let asm = doc.element(asm_el).unwrap().assembly_model().unwrap().clone();
        let solids = assembly::source_solids(&asm, |e| b.get(&e).cloned());
        let opts = cadrs_core::assembly::solver::SolveOptions {
            drives: vec![cadrs_core::assembly::solver::Drive { mate: p3b9::REVOLUTE_1, dof: cadrs_core::assembly::mate::Dof::Angle, value: std::f64::consts::FRAC_PI_2 }],
            ..Default::default()
        };
        let sol = assembly::solve(&asm, &solids, &opts);
        assert!(sol.converged);
        let turned = |i: InstanceId| {
            let p = sol.poses.iter().find(|(x, _)| *x == i).unwrap().1;
            p.rotation[1][0].atan2(p.rotation[0][0])
        };
        // (A mate's angle is the second connector's turn from the first's: the gear turns the
        // other way round from its mate's value.)
        assert!((turned(p3b9::GEAR_12).abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((turned(p3b9::GEAR_24) + turned(p3b9::GEAR_12) / 2.0).abs() < 1e-6, "the 24T gear turns back half as far");
        // Deleting Revolute 2 deletes the relation; undo brings both back.
        h.execute(&mut doc, &cadrs_core::assembly::commands::DeleteMateFeatures { element: asm_el, mates: vec![p3b9::REVOLUTE_2] }).unwrap();
        assert!(doc.element(asm_el).unwrap().assembly_model().unwrap().mate(id).is_none());
        h.undo(&mut doc).unwrap();
        assert!(doc.element(asm_el).unwrap().assembly_model().unwrap().mate(id).is_some());
        h.undo(&mut doc).unwrap();
        assert!(doc.element(asm_el).unwrap().assembly_model().unwrap().mate(id).is_none(), "undo removes the relation");
        let asm = doc.element(asm_el).unwrap().assembly_model().unwrap();
        assert_eq!(structure::solver_model(&doc, asm).mates.len(), 7);
    }

    #[test]
    fn animating_the_driver_turns_the_gears_opposite_ways() {
        // As the Animate dialog plays Revolute 1 (snap, hold free, one step a frame).
        let mut doc = p3b9::relations_document().unwrap();
        let mut h = History::default();
        let gear = Relation { ratio: (2.0, 1.0), ..Relation::new(RelationType::Gear, vec![p3b9::REVOLUTE_1, p3b9::REVOLUTE_2]) };
        h.execute(&mut doc, &AddMateFeature { element: p3b9::RELATIONS_ASSEMBLY, feature: MateFeature::new(MateId::from_u128(77), "Gear 1", MateKind::Relation(gear)), poses: vec![] }).unwrap();
        let b = builds(&doc);
        let asm = doc.element(p3b9::RELATIONS_ASSEMBLY).unwrap().assembly_model().unwrap().clone();
        let mut model = structure::solver_model(&doc, &asm);
        let solids = assembly::occurrence_solids(&doc, &asm, |e| b.get(&e).cloned());
        for k in 1..=12 {
            let value = std::f64::consts::FRAC_PI_2 * k as f64 / 12.0;
            let opts = cadrs_core::assembly::solver::SolveOptions {
                movers: vec![p3b9::GEAR_12],
                snap: Some(p3b9::REVOLUTE_1),
                drives: vec![cadrs_core::assembly::solver::Drive { mate: p3b9::REVOLUTE_1, dof: cadrs_core::assembly::mate::Dof::Angle, value }],
                snap_only: false,
                hold_free: true,
            };
            let sol = assembly::solve(&model, &solids, &opts);
            for (id, p) in &sol.poses {
                model.instance_mut(*id).unwrap().pose = *p;
            }
        }
        let turn = |i: InstanceId| {
            let p = model.instance(i).unwrap().pose;
            p.rotation[1][0].atan2(p.rotation[0][0])
        };
        let (a, b2) = (turn(p3b9::GEAR_12), turn(p3b9::GEAR_24));
        assert!((a.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "{a}");
        assert!((b2 + a / 2.0).abs() < 1e-6, "12T {a}, 24T {b2}");
    }

    /// The Cover part's hole centres (x, y; mm) on its top face.
    fn cover_holes(doc: &Document) -> Vec<[f64; 2]> {
        let b = cadrs_core::rebuild::build(&doc.element(p3b9::COVER_STUDIO).unwrap().active_features());
        let s = &b.part(p3b9::COVER_PART).unwrap().solid;
        let mut out: Vec<[f64; 2]> = s
            .edges
            .iter()
            .filter_map(|e| e.circle.filter(|c| (c.center[2] - 6.0).abs() < 1e-6).map(|c| [c.center[0], c.center[1]]))
            .collect();
        out.sort_by(|a, b| a[0].total_cmp(&b[0]));
        out
    }

    #[test]
    fn edit_in_context_holes_follow_the_base() {
        // X15 Edit in context: the Cover studio, in the context of Assembly 1, sketches on the
        // Base's top face (context geometry), uses the Base's two hole edges and cuts them through
        // the Cover. The Base moves 15 mm along X in the assembly: after Update context the holes
        // follow.
        let mut doc = p3b9::context_document().unwrap();
        let mut h = History::default();
        let ctx = context::snapshot(&doc, p3b9::CONTEXT_ASSEMBLY, p3b9::COVER).unwrap();
        assert_eq!(ctx.parts.len(), 1, "the Base (not the Cover itself)");
        assert_eq!(ctx.parts[0].name, "Base <1>");
        h.execute(&mut doc, &SetStudioContext { studio: p3b9::COVER_STUDIO, context: Some(ctx) }).unwrap();
        let solids = context::solids(&doc, p3b9::COVER_STUDIO);
        let (cid, s) = solids[0].clone();
        assert!(context::is_context(cid) && !context::is_context(p3b9::COVER_PART.feature));
        // The Base's top face (z 0, facing +Z).
        let top = s
            .faces
            .iter()
            .enumerate()
            .find(|(i, f)| f.plane.is_some_and(|p| p.origin[2].abs() < 1e-9) && s.face_normal(*i).is_some_and(|n| n[2] > 0.999))
            .map(|(_, f)| f.name)
            .unwrap();
        let plane = context::face_plane_on(&s, cid, top).unwrap();
        let sk = FeatureId::from_u128(0x3b09_5001);
        h.execute(&mut doc, &AddSketch { element: p3b9::COVER_STUDIO, feature: sk, plane: Some(plane) }).unwrap();
        let frame = plane.frame();
        let lc = LinkContext { solids: vec![(cid, &*s)], features: &[] };
        let items: Vec<_> = s
            .edges
            .iter()
            .filter(|e| e.circle.is_some_and(|c| c.center[2].abs() < 1e-9))
            .filter_map(|e| {
                let link = Link::Edge { feature: cid.0, edge: e.name };
                Some((lc.shape(link, &frame)?, link))
            })
            .collect();
        assert_eq!(items.len(), 2, "the two hole edges");
        h.execute(&mut doc, &EditSketch { element: p3b9::COVER_STUDIO, feature: sk, op: SketchOp::Use { items } }).unwrap();
        // Extrude the two circles 6 mm up, Remove from the Cover.
        let g = doc.element(p3b9::COVER_STUDIO).unwrap().feature(sk).unwrap().sketch().unwrap().geometry.clone();
        let seeds: Vec<cadrs_sketch::Vec2> = p3b9::HOLES_X.iter().map(|x| frame.to_sketch([*x, 0.0, 0.0])).collect();
        let regions = cadrs_core::samples::region_refs(sk, &g, &seeds);
        assert_eq!(regions.len(), 2);
        let mut x = cadrs_core::samples::extrude_of(regions, 6.0);
        x.op = BooleanOp::Remove;
        x.merge_scope = vec![p3b9::COVER_PART];
        let ex = FeatureId::from_u128(0x3b09_5002);
        h.execute(&mut doc, &AddExtrude { element: p3b9::COVER_STUDIO, feature: ex, extrude: ExtrudeFeature::default() }).unwrap();
        h.execute(&mut doc, &SetExtrude { element: p3b9::COVER_STUDIO, feature: ex, extrude: x, label: "Extrude".into() }).unwrap();
        let holes = cover_holes(&doc);
        assert_eq!(holes.len(), 2, "{holes:?}");
        for (h0, x0) in holes.iter().zip(p3b9::HOLES_X) {
            assert!((h0[0] - x0).abs() < 1e-6 && h0[1].abs() < 1e-6, "{holes:?}");
        }
        // The Base moves in the assembly: the studio keeps its snapshot until Update context.
        h.execute(&mut doc, &MoveInstances { element: p3b9::CONTEXT_ASSEMBLY, poses: vec![(p3b9::CONTEXT_BASE, Pose::translation([15.0, 0.0, 0.0]))], label: "Move".into() }).unwrap();
        assert!((cover_holes(&doc)[0][0] - p3b9::HOLES_X[0]).abs() < 1e-6, "not yet");
        let ctx = context::snapshot(&doc, p3b9::CONTEXT_ASSEMBLY, p3b9::COVER).unwrap();
        h.execute(&mut doc, &SetStudioContext { studio: p3b9::COVER_STUDIO, context: Some(ctx) }).unwrap();
        let holes = cover_holes(&doc);
        assert_eq!(holes.len(), 2, "{holes:?}");
        for (h0, x0) in holes.iter().zip(p3b9::HOLES_X) {
            assert!((h0[0] - (x0 + 15.0)).abs() < 1e-6, "the holes followed: {holes:?}");
        }
        // Undo the update: back where they were.
        h.undo(&mut doc).unwrap();
        assert!((cover_holes(&doc)[0][0] - p3b9::HOLES_X[0]).abs() < 1e-6);
        let features = doc.element(p3b9::COVER_STUDIO).unwrap().features();
        let i = features.iter().position(|f| f.id == sk).unwrap();
        assert!(!cadrs_core::parts::sketch_face_lost(features, i), "a context face is not a lost face");
    }

    #[test]
    fn where_used_lists_the_assemblies() {
        let doc = cadrs_core::samples::pneumatic_ex3::document().unwrap();
        let studio = doc.elements.iter().find(|e| e.assembly_model().is_none()).map(|e| e.id).unwrap();
        let used = context::where_used(&doc, studio, None);
        let names: Vec<&str> = used.iter().map(|(e, _, _)| doc.element(*e).unwrap().name.as_str()).collect();
        assert!(names.contains(&"Cylinder assembly"), "{names:?}");
        assert!(names.contains(&"Top Cap subassembly"), "{names:?}");
        // A subassembly tab is used in its parent.
        let parents = context::assembly_used_in(&doc, cadrs_core::samples::pneumatic_ex3::TOP_SUB_EL);
        assert_eq!(parents.len(), 1);
        assert_eq!(doc.element(parents[0].0).unwrap().name, "Cylinder assembly");
    }

    #[test]
    fn the_p3b9_fixtures_are_current() {
        // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test assembly_p3b9`.
        for (name, doc) in [
            ("relations_standin", p3b9::relations_document().unwrap()),
            ("interference_standin", p3b9::interference_document().unwrap()),
            ("context_standin", p3b9::context_document().unwrap()),
            ("flange_replace", p3b9::replace_document().unwrap()),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/{name}.cadrs"));
            let text = ron::ser::to_string_pretty(&p3b9::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
            if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
                std::fs::write(&path, &text).unwrap();
            }
            let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
            assert_eq!(stored.document, doc, "fixtures/{name}.cadrs is out of date");
        }
    }
}
