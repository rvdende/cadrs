//! Assembly structure (P3B.4, `intro-to-assemblies.md` A16.2, A16.3, A17, A18, A21.4–A21.13) on
//! the Ex2 end state (`samples::pneumatic_ex2`, `fixtures/pneumatic_cylinder_ex2.cadrs`):
//! subassemblies (Move to new, Create empty, drag in and out, Dissolve, rigid and flexible, Fix
//! not carried, cycles refused), folders in both lists, and the Top Cap subassembly's
//! self-check of `intro-to-assemblies-gaps.md` ("Ex3 Assembly Structure"), to 1e-4.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::commands::{AddMateFeature, InsertInstance, MoveInstances, SetInstancesFixed, SetInstancesHidden};
use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::folders::{CreateAssemblyFolder, DeleteAssemblyFolder, FolderList, MoveListItems, UnpackAssemblyFolder, item, mate_item};
use cadrs_core::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateType};
use cadrs_core::assembly::solver::{Drive, SolveOptions};
use cadrs_core::assembly::structure::{
    DissolveSubassembly, MoveIntoSubassembly, MoveOutOfSubassembly, MoveToNewSubassembly, SetInstancesSuppressed, SetSubassemblyFlexible, derive, occurrences, solver_model,
};
use cadrs_core::assembly::{self, Assembly, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::samples::pneumatic as pc;
use cadrs_core::samples::pneumatic_ex2 as ex2;
use cadrs_core::{Document, ElementId, FeatureId, History, PartId, Solid};

const IN: f64 = 25.4;
const LB: f64 = 0.453_592_37;

fn close(what: &str, got: f64, want: f64, rel: f64) {
    let tol = rel * want.abs().max(1e-9);
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (±{tol})");
}

struct Ex3 {
    doc: Document,
    h: History,
    build: Arc<cadrs_core::rebuild::Build>,
}

const TOP_SUB_EL: ElementId = ElementId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0301);
const TOP_SUB: InstanceId = InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_3001);
const REAR_SUB_EL: ElementId = ElementId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0302);
const REAR_SUB: InstanceId = InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_3002);
const PISTON_SUB_EL: ElementId = ElementId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0303);
const PISTON_SUB: InstanceId = InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_3003);

impl Ex3 {
    fn new() -> Self {
        let doc = ex2::document().unwrap();
        let build = cadrs_core::rebuild::build(doc.element(pc::STUDIO).unwrap().features());
        Self { doc, h: History::default(), build }
    }

    fn model(&self, el: ElementId) -> &Assembly {
        self.doc.element(el).unwrap().assembly_model().unwrap()
    }

    fn run(&mut self, cmd: &dyn cadrs_core::Command) {
        self.h.execute(&mut self.doc, cmd).unwrap_or_else(|e| panic!("{}: {e}", cmd.label()));
    }

    /// Every part's world placement, by (part, its own instance) (the <n> changes when it moves
    /// into a subassembly).
    fn world(&self, el: ElementId) -> HashMap<(PartId, InstanceId), Pose> {
        occurrences(&self.doc, self.model(el)).into_iter().map(|o| ((o.part, o.child.unwrap_or(o.id)), o.pose)).collect()
    }

    fn solids(&self, el: ElementId) -> HashMap<InstanceId, Arc<Solid>> {
        let b = self.build.clone();
        assembly::occurrence_solids(&self.doc, self.model(el), |_| Some(b.clone()))
    }

    /// Solves the whole assembly; the placements that change.
    fn resolve(&self, el: ElementId) -> Vec<(InstanceId, Pose)> {
        let flat = solver_model(&self.doc, self.model(el));
        let s = assembly::solve(&flat, &self.solids(el), &SolveOptions::default());
        assert!(s.converged, "residual {}", s.residual);
        s.changed(&flat)
    }

    fn mass(&self, el: ElementId) -> cadrs_core::parts::MassReport {
        let b = self.build.clone();
        let model = self.model(el);
        let (parts, props) = assembly::instance_parts(&self.doc, model, |_| Some(b.clone()));
        let ids: Vec<InstanceId> = model.instances.iter().map(|i| i.id).collect();
        assembly::mass_report(&parts, &props, &ids).unwrap()
    }

    fn names(&self, el: ElementId) -> Vec<String> {
        self.model(el).mates.iter().map(|m| m.name.clone()).collect()
    }

    /// A21.4: Retaining Plate, Top Cap, O-Ring 0.125 <3>, <4> → Move to new subassembly.
    fn top_cap_subassembly(&mut self) {
        let [_, _, r3, r4] = ex2::ORINGS_125;
        self.run(&MoveToNewSubassembly {
            element: ex2::ASSEMBLY,
            instances: vec![ex2::RETAINING_PLATE, ex2::TOP_CAP, r3, r4],
            new_element: TOP_SUB_EL,
            instance: TOP_SUB,
            name: None,
            after: None,
        });
    }
}

fn same_pose(a: &Pose, b: &Pose) -> bool {
    (0..3).all(|i| (a.translation[i] - b.translation[i]).abs() < 1e-7 && (0..3).all(|j| (a.rotation[i][j] - b.rotation[i][j]).abs() < 1e-9))
}

fn same_world(a: &HashMap<(PartId, InstanceId), Pose>, b: &HashMap<(PartId, InstanceId), Pose>) {
    assert_eq!(a.len(), b.len());
    for (k, p) in a {
        assert!(same_pose(p, &b[k]), "{k:?} moved: {p:?} vs {:?}", b[k]);
    }
}

#[test]
fn the_ex2_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test assembly_structure`.
    let doc = ex2::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pneumatic_cylinder_ex2.cadrs");
    let text = ron::ser::to_string_pretty(&pc::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/pneumatic_cylinder_ex2.cadrs is out of date");
    let asm = doc.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!((asm.instances.len(), asm.mates.len()), (17, 14));
}

#[test]
fn move_to_new_subassembly_keeps_every_placement_and_takes_the_mates() {
    let mut ex = Ex3::new();
    let before_doc = ex.doc.clone();
    let before = ex.world(ex2::ASSEMBLY);
    ex.top_cap_subassembly();
    assert_eq!(ex.h.undo_len(), 1, "one undo step");
    // A new tab right of the assembly's, "Assembly 1", with the four instances.
    let tab = ex.doc.element(TOP_SUB_EL).unwrap();
    assert_eq!(tab.name, "Assembly 1");
    assert_eq!(ex.doc.element_index(TOP_SUB_EL), ex.doc.element_index(ex2::ASSEMBLY).map(|i| i + 1));
    let child = ex.model(TOP_SUB_EL);
    let [_, _, r3, r4] = ex2::ORINGS_125;
    assert_eq!(child.instances.iter().map(|i| i.id).collect::<Vec<_>>(), vec![ex2::RETAINING_PLATE, ex2::TOP_CAP, r3, r4]);
    // A21.5: 3 mates: the O-rings' two Fastened, and a Fastened that takes over from the group.
    assert_eq!(ex.names(TOP_SUB_EL), ["Fastened 3", "Fastened 4", "Fastened 12"]);
    // The parent: 14 instances (the subassembly where the plate was), 12 mates; the group holds
    // the subassembly; the rods' mates reach into it.
    let parent = ex.model(ex2::ASSEMBLY);
    assert_eq!(parent.instances.len(), 14);
    assert_eq!(parent.instances[2].id, TOP_SUB);
    assert_eq!(parent.mates.len(), 12);
    let group = parent.mate(ex2::GROUP_1).unwrap();
    assert_eq!(group.instances(), vec![ex2::REAR_CAP, ex2::BARREL, TOP_SUB]);
    let rod_mate = parent.mates.iter().find(|m| m.name == "Fastened 5").unwrap();
    assert!(rod_mate.involves(derive(TOP_SUB, ex2::TOP_CAP)));
    // Every part is where it was, and the mates all hold there.
    same_world(&before, &ex.world(ex2::ASSEMBLY));
    assert!(ex.resolve(ex2::ASSEMBLY).is_empty(), "nothing moves on a solve");
    // Undo puts everything back (the new tab too); redo does it again.
    ex.h.undo(&mut ex.doc);
    assert_eq!(ex.doc, before_doc);
    ex.h.redo(&mut ex.doc);
    assert_eq!(ex.model(ex2::ASSEMBLY).instances.len(), 14);
}

#[test]
fn top_cap_subassembly_self_check() {
    let mut ex = Ex3::new();
    ex.top_cap_subassembly();
    // A21.5: rename the tab, Fix the Top Cap.
    ex.run(&cadrs_core::commands::RenameElement { id: TOP_SUB_EL, name: "Top Cap subassembly".into() });
    ex.run(&SetInstancesFixed { element: TOP_SUB_EL, instances: vec![ex2::TOP_CAP], fixed: true });
    // A21.16: its mass properties (in its own tab, the top-level row), positions kept.
    let r = ex.mass(TOP_SUB_EL);
    let m = r.mass.expect("materials");
    close("mass (lb)", m.mass / LB, 0.48362, 1e-4);
    close("V (in³)", r.volume / IN.powi(3), 5.05843, 1e-4);
    close("CoM z (in)", m.center_of_mass.z / IN, 6.03294, 1e-4);
    assert!(m.center_of_mass.x.abs() < 1e-6 && m.center_of_mass.y.abs() < 1e-6);
    // The same instance in the parent measures the same.
    let b = ex.build.clone();
    let (parts, props) = assembly::instance_parts(&ex.doc, ex.model(ex2::ASSEMBLY), |_| Some(b.clone()));
    let sub = assembly::mass_report(&parts, &props, &[TOP_SUB]).unwrap();
    close("in the parent (lb)", sub.mass.unwrap().mass / LB, 0.48362, 1e-4);
    // Its name in the parent follows the tab.
    let name = parts.iter().find(|p| InstanceId::of_part(p.id) == TOP_SUB).map(|p| p.name.clone());
    assert!(name.is_some());
    assert_eq!(assembly::source_part_name(&ex.doc, &InstanceSource::Assembly { element: TOP_SUB_EL }, None), "Top Cap subassembly");
}

/// A21.4–A21.13: the whole structure part of Exercise 3.
#[test]
fn ex3_structure_steps() {
    let mut ex = Ex3::new();
    let before = ex.world(ex2::ASSEMBLY);
    ex.top_cap_subassembly();
    ex.run(&cadrs_core::commands::RenameElement { id: TOP_SUB_EL, name: "Top Cap subassembly".into() });
    ex.run(&SetInstancesFixed { element: TOP_SUB_EL, instances: vec![ex2::TOP_CAP], fixed: true });
    // A21.6: Create empty subassembly (after the Rear Cap), A21.7: drag 4 instances onto it.
    ex.run(&MoveToNewSubassembly { element: ex2::ASSEMBLY, instances: vec![], new_element: REAR_SUB_EL, instance: REAR_SUB, name: None, after: Some(ex2::REAR_CAP) });
    assert_eq!(ex.doc.element(REAR_SUB_EL).unwrap().name, "Assembly 1");
    assert!(ex.model(REAR_SUB_EL).instances.is_empty());
    let [r1, r2, ..] = ex2::ORINGS_125;
    ex.run(&MoveIntoSubassembly { element: ex2::ASSEMBLY, sub: REAR_SUB, instances: vec![ex2::REAR_CAP, ex2::REAR_CAP_MOUNT, r1, r2] });
    // A21.8: "Rear Cap subassembly", Fix the Rear Cap: two Fastened and Revolute 1.
    ex.run(&cadrs_core::commands::RenameElement { id: REAR_SUB_EL, name: "Rear Cap subassembly".into() });
    ex.run(&SetInstancesFixed { element: REAR_SUB_EL, instances: vec![ex2::REAR_CAP], fixed: true });
    // Listed as `ex3-step8.png` lists them: the fastened ones first (P3B.4 judge).
    assert_eq!(ex.names(REAR_SUB_EL), ["Fastened 1", "Fastened 2", "Revolute 1"]);
    same_world(&before, &ex.world(ex2::ASSEMBLY));
    // A21.9–A21.10: Piston & Rod and the three O-Ring 0.185 → "Piston subassembly", Fix.
    let mut piston: Vec<InstanceId> = vec![ex2::PISTON_ROD];
    piston.extend(ex2::ORINGS_185);
    ex.run(&MoveToNewSubassembly { element: ex2::ASSEMBLY, instances: piston.clone(), new_element: PISTON_SUB_EL, instance: PISTON_SUB, name: None, after: None });
    ex.run(&cadrs_core::commands::RenameElement { id: PISTON_SUB_EL, name: "Piston subassembly".into() });
    ex.run(&SetInstancesFixed { element: PISTON_SUB_EL, instances: vec![ex2::PISTON_ROD], fixed: true });
    assert_eq!(ex.names(PISTON_SUB_EL), ["Fastened 9", "Fastened 10", "Fastened 11"]);
    // The slider now holds a part of the Piston subassembly to one of the Rear Cap's.
    let slider = ex.model(ex2::ASSEMBLY).mate(ex2::SLIDER_1).unwrap().clone();
    assert!(slider.involves(derive(PISTON_SUB, ex2::PISTON_ROD)) && slider.involves(derive(REAR_SUB, ex2::REAR_CAP)));
    // A21.11: the Rear Cap mount dragged out to the top: Revolute 1 comes along, reaching in.
    ex.run(&MoveOutOfSubassembly { element: ex2::ASSEMBLY, sub: REAR_SUB, instances: vec![ex2::REAR_CAP_MOUNT], at: Some(0) });
    assert_eq!(ex.model(ex2::ASSEMBLY).instances[0].id, ex2::REAR_CAP_MOUNT);
    assert_eq!(ex.names(REAR_SUB_EL), ["Fastened 1", "Fastened 2"]);
    let rev = ex.model(ex2::ASSEMBLY).mate(ex2::REVOLUTE_1).unwrap();
    assert!(rev.involves(ex2::REAR_CAP_MOUNT) && rev.involves(derive(REAR_SUB, ex2::REAR_CAP)));
    same_world(&before, &ex.world(ex2::ASSEMBLY));
    // The mount keeps its one DOF about the Rear Cap subassembly.
    let flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
    let dofs = assembly::instance_dofs(&flat, &ex.solids(ex2::ASSEMBLY));
    assert_eq!(dofs[&ex2::REAR_CAP_MOUNT], 1);
    // A21.12: Dissolve the Piston subassembly: its instances come back, its tab stays empty.
    ex.run(&DissolveSubassembly { element: ex2::ASSEMBLY, sub: PISTON_SUB });
    assert!(ex.model(PISTON_SUB_EL).instances.is_empty());
    let top: Vec<InstanceId> = ex.model(ex2::ASSEMBLY).instances.iter().map(|i| i.id).collect();
    for p in &piston {
        assert!(top.contains(p));
    }
    // The Fix inside is not carried out (A3.7): the piston slides again.
    assert!(!ex.model(ex2::ASSEMBLY).instance(ex2::PISTON_ROD).unwrap().fixed);
    let flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
    assert_eq!(assembly::instance_dofs(&flat, &ex.solids(ex2::ASSEMBLY))[&ex2::PISTON_ROD], 1);
    same_world(&before, &ex.world(ex2::ASSEMBLY));
    // A21.13: delete the empty tab.
    ex.run(&cadrs_core::commands::DeleteElement { id: PISTON_SUB_EL });
    // The end state (apart from the Hardware, P3B.5): Rear Cap mount, Barrel (fixed), Rear Cap
    // subassembly, Top Cap subassembly, 4 rods, Piston & Rod, 3 O-Ring 0.185.
    let model = ex.model(ex2::ASSEMBLY);
    let order: Vec<InstanceId> = model.instances.iter().map(|i| i.id).collect();
    let mut want = vec![ex2::REAR_CAP_MOUNT, ex2::BARREL, REAR_SUB, TOP_SUB];
    want.extend(ex2::RODS);
    want.extend(piston);
    assert_eq!(order, want);
    assert!(model.instance(ex2::BARREL).unwrap().fixed);
    assert!(ex.resolve(ex2::ASSEMBLY).is_empty());
    // Every step undoes one at a time, back to the Ex2 end state.
    while ex.h.can_undo() {
        ex.h.undo(&mut ex.doc);
    }
    assert_eq!(ex.doc, ex2::document().unwrap());
}

#[test]
fn dissolve_restores_the_flat_structure() {
    let mut ex = Ex3::new();
    let before_doc = ex.doc.clone();
    let before = ex.world(ex2::ASSEMBLY);
    ex.top_cap_subassembly();
    ex.run(&DissolveSubassembly { element: ex2::ASSEMBLY, sub: TOP_SUB });
    let flat = ex.model(ex2::ASSEMBLY);
    let old = before_doc.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap();
    let mut a: Vec<InstanceId> = flat.instances.iter().map(|i| i.id).collect();
    let mut b: Vec<InstanceId> = old.instances.iter().map(|i| i.id).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b, "the same instances, at the top level");
    assert!(flat.instances.iter().all(|i| !i.source.is_assembly()));
    same_world(&before, &ex.world(ex2::ASSEMBLY));
    // Every mate is back as it was; the Fastened that stood for the group inside comes out
    // too, and the group holds what the subassembly held (it moved as one with it).
    for m in &old.mates {
        let now = flat.mate(m.id).unwrap_or_else(|| panic!("{} is gone", m.name));
        match (&m.kind, &now.kind) {
            (MateKind::Group { instances: x }, MateKind::Group { instances: y }) => {
                assert!(x.iter().all(|i| y.contains(i)), "{x:?} in {y:?}");
            }
            _ => assert_eq!(m, now),
        }
    }
    assert_eq!(flat.mates.len(), old.mates.len() + 1);
    // The tab stays, empty (A17.5).
    assert!(ex.model(TOP_SUB_EL).instances.is_empty() && ex.model(TOP_SUB_EL).mates.is_empty());
    assert!(ex.resolve(ex2::ASSEMBLY).is_empty());
    ex.h.undo(&mut ex.doc);
    ex.h.undo(&mut ex.doc);
    assert_eq!(ex.doc, before_doc);
}

#[test]
fn a_fix_in_the_child_is_ignored_in_the_parent() {
    let mut ex = Ex3::new();
    ex.top_cap_subassembly();
    ex.run(&SetInstancesFixed { element: TOP_SUB_EL, instances: vec![ex2::TOP_CAP], fixed: true });
    // The flat model: only the parent's Barrel holds.
    let flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
    assert!(flat.instance(ex2::BARREL).unwrap().fixed);
    assert!(!flat.instance(derive(TOP_SUB, ex2::TOP_CAP)).unwrap().fixed);
    // Without the group, the subassembly moves: a slider in the parent pulls its Top Cap up
    // 1 in, although the Top Cap is fixed in its own tab.
    let group = ex.model(ex2::ASSEMBLY).mate(ex2::GROUP_1).unwrap().clone();
    ex.run(&cadrs_core::assembly::commands::DeleteMateFeatures { element: ex2::ASSEMBLY, mates: vec![group.id] });
    let top = derive(TOP_SUB, ex2::TOP_CAP);
    let here = ConnectorFrame::default();
    let lifted = ConnectorFrame { origin: [0.0, 0.0, IN], ..here };
    let m = Mate::new(MateType::Fastened, MateConnector::at(top, here), MateConnector::at(ex2::BARREL, lifted));
    let f = MateFeature::new(MateId::from_u128(77), "Fastened 99", MateKind::Mate(m));
    let mut flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
    flat.mates.push(f.clone());
    let s = assembly::solve(&flat, &ex.solids(ex2::ASSEMBLY), &SolveOptions { movers: vec![top], ..Default::default() });
    assert!(s.converged);
    let poses = s.changed(&flat);
    ex.run(&AddMateFeature { element: ex2::ASSEMBLY, feature: f, poses });
    let sub = ex.model(ex2::ASSEMBLY).instance(TOP_SUB).unwrap();
    assert!((sub.pose.translation[2] - IN).abs() < 1e-6, "the subassembly moved up 1 in: {:?}", sub.pose);
    // Its tab is untouched.
    assert!(ex.model(TOP_SUB_EL).instance(ex2::TOP_CAP).unwrap().pose.translation[2].abs() < 1e-9);
}

#[test]
fn a_flexible_subassembly_keeps_its_motion_in_the_parent() {
    let mut ex = Ex3::new();
    // The Rear Cap and its mount (Revolute 1) in a subassembly; the Rear Cap fixed there.
    ex.run(&MoveToNewSubassembly {
        element: ex2::ASSEMBLY,
        instances: vec![ex2::REAR_CAP, ex2::REAR_CAP_MOUNT],
        new_element: REAR_SUB_EL,
        instance: REAR_SUB,
        name: None,
        after: None,
    });
    ex.run(&SetInstancesFixed { element: REAR_SUB_EL, instances: vec![ex2::REAR_CAP], fixed: true });
    let mount = derive(REAR_SUB, ex2::REAR_CAP_MOUNT);
    let dofs = |ex: &Ex3| {
        let flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
        assembly::instance_dofs(&flat, &ex.solids(ex2::ASSEMBLY))
    };
    // Rigid (the default): it moves as one with the Rear Cap (held by the group): no DOF.
    assert_eq!(dofs(&ex)[&mount], 0);
    // Flexible (A16.2): Revolute 1 acts here, so the mount turns.
    ex.run(&SetSubassemblyFlexible { element: ex2::ASSEMBLY, instances: vec![REAR_SUB], flexible: true });
    assert_eq!(dofs(&ex)[&mount], 1);
    assert_eq!(dofs(&ex)[&derive(REAR_SUB, ex2::REAR_CAP)], 0);
    // Turning it here is kept on the instance, not in the tab.
    let flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
    let rev = flat.mates.iter().find(|m| m.name == "Revolute 1").unwrap().id;
    let turn = SolveOptions { movers: vec![mount], snap: Some(rev), drives: vec![Drive { mate: rev, dof: Dof::Angle, value: 0.5 }], ..Default::default() };
    let s = assembly::solve(&flat, &ex.solids(ex2::ASSEMBLY), &turn);
    assert!(s.converged);
    let poses = s.changed(&flat);
    assert_eq!(poses.len(), 1);
    ex.run(&MoveInstances { element: ex2::ASSEMBLY, poses: poses.clone(), label: "Turn".into() });
    let sub = ex.model(ex2::ASSEMBLY).instance(REAR_SUB).unwrap();
    assert_eq!(sub.overrides.len(), 1);
    let now = occurrences(&ex.doc, ex.model(ex2::ASSEMBLY)).into_iter().find(|o| o.id == mount).unwrap();
    assert!(same_pose(&now.pose, &poses[0].1));
    let tab_mount = ex.model(REAR_SUB_EL).instance(ex2::REAR_CAP_MOUNT).unwrap().pose;
    assert!(!same_pose(&tab_mount, &poses[0].1));
    // Rigid again: the tab's placement.
    ex.run(&SetSubassemblyFlexible { element: ex2::ASSEMBLY, instances: vec![REAR_SUB], flexible: false });
    assert!(ex.model(ex2::ASSEMBLY).instance(REAR_SUB).unwrap().overrides.is_empty());
    assert_eq!(dofs(&ex)[&mount], 0);
}

#[test]
fn cycles_are_refused() {
    let mut ex = Ex3::new();
    ex.top_cap_subassembly();
    let mut h = History::default();
    // The assembly into itself.
    let own = InstanceSource::Assembly { element: ex2::ASSEMBLY };
    let e = h.execute(&mut ex.doc, &InsertInstance { element: ex2::ASSEMBLY, instance: Instance::new(InstanceId::new(), own, Pose::IDENTITY) });
    assert!(e.is_err());
    // The parent into its subassembly.
    let e = h.execute(&mut ex.doc, &InsertInstance { element: TOP_SUB_EL, instance: Instance::new(InstanceId::new(), own, Pose::IDENTITY) });
    assert!(e.is_err());
    // Another instance of the subassembly is fine (A17.2, the Assemblies tab of Insert).
    let sub = InstanceSource::Assembly { element: TOP_SUB_EL };
    let second = InstanceId::from_u128(0x5ec0);
    h.execute(&mut ex.doc, &InsertInstance { element: ex2::ASSEMBLY, instance: Instance::new(second, sub, Pose::translation([100.0, 0.0, 0.0])) }).unwrap();
    assert_eq!(ex.model(ex2::ASSEMBLY).instance(second).unwrap().index, 2);
    // A subassembly can't take an instance of a tab that holds it.
    let outer_el = ElementId::from_u128(0x0e0e);
    let outer = InstanceId::from_u128(0x0e0f);
    h.execute(&mut ex.doc, &MoveToNewSubassembly { element: ex2::ASSEMBLY, instances: vec![second], new_element: outer_el, instance: outer, name: None, after: None }).unwrap();
    // `outer` holds an instance of TOP_SUB_EL; moving `outer` into TOP_SUB would nest it in itself.
    assert!(h.execute(&mut ex.doc, &MoveIntoSubassembly { element: ex2::ASSEMBLY, sub: TOP_SUB, instances: vec![outer] }).is_err());
}

#[test]
fn suppressed_instances_are_not_drawn_or_solved() {
    let mut ex = Ex3::new();
    let m0 = ex.mass(ex2::ASSEMBLY).mass.unwrap().mass;
    ex.run(&SetInstancesSuppressed { element: ex2::ASSEMBLY, instances: vec![ex2::RODS[0]], suppressed: true });
    assert!(ex.mass(ex2::ASSEMBLY).mass.unwrap().mass < m0);
    let flat = solver_model(&ex.doc, ex.model(ex2::ASSEMBLY));
    assert!(flat.instance(ex2::RODS[0]).is_none());
    assert_eq!(flat.mates.len(), 13, "its mate is left out");
    assert!(ex.resolve(ex2::ASSEMBLY).is_empty());
    ex.h.undo(&mut ex.doc);
    assert!((ex.mass(ex2::ASSEMBLY).mass.unwrap().mass - m0).abs() < 1e-12);
}

#[test]
fn folders_in_both_lists() {
    let mut ex = Ex3::new();
    let el = ex2::ASSEMBLY;
    let rods: Vec<FeatureId> = ex2::RODS.iter().map(|r| item(*r)).collect();
    // Add selection to folder…: the rods (and the Barrel, listed first) gather at the first.
    let f = FeatureId::from_u128(0xf01d);
    ex.run(&CreateAssemblyFolder { element: el, list: FolderList::Instances, folder: f, name: Some("Rods".into()), items: vec![rods[2], item(ex2::BARREL), rods[0]] });
    let model = ex.model(el);
    assert_eq!(model.folders[0].name, "Rods");
    assert_eq!(model.folders[0].features, vec![item(ex2::BARREL), rods[0], rods[2]]);
    assert_eq!(model.instances[0].id, ex2::BARREL);
    assert_eq!(model.instances[1].id, ex2::RODS[0]);
    // Drag the other two rods in (dropped after the folder's last item).
    ex.run(&MoveListItems { element: el, list: FolderList::Instances, items: vec![rods[1], rods[3]], to: 3, folder: Some(f), label: "Move".into() });
    assert_eq!(ex.model(el).folders[0].features.len(), 5);
    // Drag the Barrel back out, to the top.
    ex.run(&MoveListItems { element: el, list: FolderList::Instances, items: vec![item(ex2::BARREL)], to: 0, folder: None, label: "Move".into() });
    assert_eq!(ex.model(el).folders[0].features.len(), 4);
    assert_eq!(ex.model(el).instances[0].id, ex2::BARREL);
    // Reorder the folder (its rows move together) to the end of the list.
    let n = ex.model(el).instances.len();
    ex.run(&MoveListItems { element: el, list: FolderList::Instances, items: rods.clone(), to: n - 4, folder: None, label: "Move folder".into() });
    let model = ex.model(el);
    assert_eq!(model.instances[n - 1].id, ex2::RODS[3]);
    assert_eq!(model.folders[0].features.len(), 4, "the folder moved whole");
    // Hide the folder (its eye): its instances.
    ex.run(&SetInstancesHidden { element: el, instances: ex2::RODS.to_vec(), hidden: true });
    // Unpack keeps the rods; Delete takes them and their mates (Fastened 5–8).
    let before = ex.doc.clone();
    ex.run(&UnpackAssemblyFolder { element: el, list: FolderList::Instances, folder: f });
    assert!(ex.model(el).folders.is_empty());
    assert_eq!(ex.model(el).instances.len(), 17);
    ex.h.undo(&mut ex.doc);
    assert_eq!(ex.doc, before);
    ex.run(&DeleteAssemblyFolder { element: el, list: FolderList::Instances, folder: f });
    assert_eq!(ex.model(el).instances.len(), 13);
    assert_eq!(ex.model(el).mates.len(), 10);
    assert!(ex.model(el).folders.is_empty());
    ex.h.undo(&mut ex.doc);
    assert_eq!(ex.doc, before);
    // An empty folder (New folder, nothing selected), and mate folders (A18.5).
    let empty = FeatureId::from_u128(0xf02d);
    ex.run(&CreateAssemblyFolder { element: el, list: FolderList::Instances, folder: empty, name: None, items: vec![] });
    assert_eq!(ex.model(el).folders[1].name, "Folder 1");
    assert!(ex.model(el).folders[1].features.is_empty());
    let mates: Vec<FeatureId> = ex.model(el).mates.iter().filter(|m| m.name.starts_with("Fastened")).map(|m| mate_item(m.id)).collect();
    let mf = FeatureId::from_u128(0xf03d);
    ex.run(&CreateAssemblyFolder { element: el, list: FolderList::Mates, folder: mf, name: Some("Fasteners".into()), items: mates.clone() });
    assert_eq!(ex.model(el).mate_folders[0].features.len(), 11);
    ex.run(&DeleteAssemblyFolder { element: el, list: FolderList::Mates, folder: mf });
    assert_eq!(ex.model(el).mates.len(), 3);
    // Moving an instance into a subassembly takes it out of its folder.
    ex.h.undo(&mut ex.doc);
    ex.run(&MoveToNewSubassembly { element: el, instances: vec![ex2::RODS[0]], new_element: TOP_SUB_EL, instance: TOP_SUB, name: None, after: None });
    assert_eq!(ex.model(el).folders[0].features.len(), 3);
}

#[test]
fn a_rigid_subassembly_moves_as_one() {
    let mut ex = Ex3::new();
    ex.top_cap_subassembly();
    // Moving one of its parts (the solver's placement) moves the subassembly.
    let plate = derive(TOP_SUB, ex2::RETAINING_PLATE);
    let o = occurrences(&ex.doc, ex.model(ex2::ASSEMBLY)).into_iter().find(|o| o.id == plate).unwrap();
    let up = o.pose.then(&Pose::translation([0.0, 0.0, 10.0]));
    ex.run(&MoveInstances { element: ex2::ASSEMBLY, poses: vec![(plate, up)], label: "Move".into() });
    let sub = ex.model(ex2::ASSEMBLY).instance(TOP_SUB).unwrap();
    assert!((sub.pose.translation[2] - 10.0).abs() < 1e-9);
    let top = occurrences(&ex.doc, ex.model(ex2::ASSEMBLY)).into_iter().find(|o| o.id == derive(TOP_SUB, ex2::TOP_CAP)).unwrap();
    assert!((top.pose.translation[2] - 10.0).abs() < 1e-9);
}
