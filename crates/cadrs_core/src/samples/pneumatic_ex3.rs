//! The end state of "Exercise: Assembly structure" (A21) on the stand-in, as the BOM lessons
//! (A20, P3B.6) start from it: the Ex2 end state ([`super::pneumatic_ex2`]) with six hex cap
//! screws 1/4-28 x 0.75 on the Retaining Plate, the Top Cap and Rear Cap subassemblies (the
//! Piston subassembly made, dissolved and its tab deleted), eight hex nuts 3/8-16 on the rod
//! holes (on the Top Cap and on the Rear Cap's flange), and the Hardware folder holding the 14 fasteners (`ex3-drawing.png`: Instances (26)).
//! Built through the same commands as `course_asm_ex3_structure` builds it through the UI, with
//! fixed ids, so `fixtures/pneumatic_cylinder_ex3.cadrs` is regenerated exactly
//! (`cadrs_core/tests/assembly_bom.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use super::pneumatic_ex2 as ex2;
use crate::assembly::commands::SetInstancesFixed;
use crate::assembly::folders::{CreateAssemblyFolder, FolderList, MoveListItems, item};
use crate::assembly::mate::MateId;
use crate::assembly::standard::{HoleSite, StandardPart, StandardSpec, Stacking, plan_insert, site_of_edge};
use crate::assembly::structure::{DissolveSubassembly, MoveIntoSubassembly, MoveOutOfSubassembly, MoveToNewSubassembly, derive};
use crate::assembly::{self, InstanceId};
use crate::command::{Command, CommandError, History};
use crate::commands::{DeleteElement, RenameElement};
use crate::document::Document;
use crate::ids::{ElementId, FeatureId};
use crate::rebuild::Build;
use crate::solid::Solid;

const IN: f64 = 25.4;

/// The "Top Cap subassembly" tab and its instance in Cylinder assembly.
pub const TOP_SUB_EL: ElementId = ElementId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0301);
pub const TOP_SUB: InstanceId = InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_3001);
/// The "Rear Cap subassembly" tab and its instance.
pub const REAR_SUB_EL: ElementId = ElementId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0302);
pub const REAR_SUB: InstanceId = InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_3002);
/// The Piston subassembly (made, dissolved, its tab deleted).
pub const PISTON_SUB_EL: ElementId = ElementId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0303);
pub const PISTON_SUB: InstanceId = InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_3003);
/// The Hardware folder.
pub const HARDWARE: FeatureId = FeatureId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_4001);

/// The six hex cap screws.
pub const fn screw(k: usize) -> InstanceId {
    InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_5000 + k as u128)
}

/// The eight hex nuts (four on the Top Cap, four on the Rear Cap's flange).
pub const fn nut(k: usize) -> InstanceId {
    InstanceId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_6000 + k as u128)
}

const fn fastener_mate(n: u128) -> MateId {
    MateId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_7000 + n)
}

/// The screws' configuration (A21.2): ANSI inch hex cap screw 1/4-28 x 0.75, Stainless Steel.
pub fn screw_spec() -> StandardSpec {
    let mut s = StandardSpec::new("ANSI inch", "Bolts & screws", "Hex bolts", "Hex cap screw").expect("in the library");
    s.size = "1/4-28".into();
    s.material = "Stainless Steel".into();
    s.normalize();
    s
}

/// The nuts' configuration (A21.14): ANSI inch hex nut 3/8-16, Chamfered, Stainless Steel.
pub fn nut_spec() -> StandardSpec {
    let mut s = StandardSpec::new("ANSI inch", "Nuts", "Hex nuts", "Hex nut").expect("in the library");
    s.size = "3/8-16".into();
    s.material = "Stainless Steel".into();
    s.normalize();
    s
}

struct Ex3<'a> {
    doc: &'a mut Document,
    h: &'a mut History,
    builds: HashMap<ElementId, Arc<Build>>,
}

impl Ex3<'_> {
    fn run(&mut self, cmd: &dyn Command) -> Result<(), CommandError> {
        self.h.execute(self.doc, cmd)
    }

    fn solids(&mut self) -> HashMap<InstanceId, Arc<Solid>> {
        let asm = self.doc.element(ex2::ASSEMBLY).and_then(|e| e.assembly_model()).cloned().unwrap_or_default();
        for o in assembly::structure::occurrences(self.doc, &asm) {
            if !self.builds.contains_key(&o.element)
                && let Some(el) = self.doc.element(o.element)
            {
                self.builds.insert(o.element, crate::rebuild::build(el.features()));
            }
        }
        let builds = self.builds.clone();
        assembly::occurrence_solids(self.doc, &asm, |e| builds.get(&e).cloned())
    }

    /// The circular edges of an occurrence's part with diameter `dia` (in) centred at `z` (in).
    fn sites(&mut self, occurrence: InstanceId, dia: f64, z: f64) -> Result<Vec<HoleSite>, CommandError> {
        let solid = self.solids().get(&occurrence).cloned().ok_or_else(|| CommandError::Invalid("no such occurrence".into()))?;
        let mut out: Vec<HoleSite> = solid
            .edges
            .iter()
            .filter(|e| e.circle.is_some_and(|c| (2.0 * c.radius - dia * IN).abs() < 1e-3 && (c.center[2] - z * IN).abs() < 1e-3))
            .filter_map(|e| site_of_edge(&solid, occurrence, &e.name))
            .collect();
        out.sort_by_key(|a| a.edge.index);
        Ok(out)
    }

    /// Inserts `spec` on `sites` with the instance ids `ids` and mate ids from `mate0`.
    fn insert(&mut self, spec: &StandardSpec, sites: &[HoleSite], ids: &[InstanceId], mate0: u128) -> Result<(), CommandError> {
        let solids = self.solids();
        let part = StandardPart::new(spec)?;
        let mut cmd = plan_insert(self.doc, ex2::ASSEMBLY, part, sites, false, Stacking::Plain, &solids)?;
        for (k, ins) in cmd.inserts.iter_mut().enumerate() {
            ins.instance = ids[k];
            ins.mate = fastener_mate(mate0 + k as u128);
        }
        self.run(&cmd)
    }
}

/// Builds the Ex3 end state into `doc` (the Ex2 end state).
pub fn build_in(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    let mut ex = Ex3 { doc, h, builds: HashMap::new() };
    // A21.2: six hex cap screws on the Retaining Plate's Ø0.266 hole edges (top face z 6.625).
    let sites = ex.sites(ex2::RETAINING_PLATE, 0.266, 6.625)?;
    let screws: Vec<InstanceId> = (0..6).map(screw).collect();
    ex.insert(&screw_spec(), &sites, &screws, 0)?;
    // A21.3: the Hardware folder.
    ex.run(&CreateAssemblyFolder {
        element: ex2::ASSEMBLY,
        list: FolderList::Instances,
        folder: HARDWARE,
        name: Some("Hardware".into()),
        items: screws.iter().map(|s| item(*s)).collect(),
    })?;
    // A21.4–A21.5: Top Cap subassembly (the Top Cap fixed).
    let [r1, r2, r3, r4] = ex2::ORINGS_125;
    ex.run(&MoveToNewSubassembly {
        element: ex2::ASSEMBLY,
        instances: vec![ex2::RETAINING_PLATE, ex2::TOP_CAP, r3, r4],
        new_element: TOP_SUB_EL,
        instance: TOP_SUB,
        name: None,
        after: None,
    })?;
    ex.run(&RenameElement { id: TOP_SUB_EL, name: "Top Cap subassembly".into() })?;
    ex.run(&SetInstancesFixed { element: TOP_SUB_EL, instances: vec![ex2::TOP_CAP], fixed: true })?;
    // A21.6–A21.8: an empty subassembly after the Rear Cap, filled, "Rear Cap subassembly".
    ex.run(&MoveToNewSubassembly { element: ex2::ASSEMBLY, instances: vec![], new_element: REAR_SUB_EL, instance: REAR_SUB, name: None, after: Some(ex2::REAR_CAP) })?;
    ex.run(&MoveIntoSubassembly { element: ex2::ASSEMBLY, sub: REAR_SUB, instances: vec![ex2::REAR_CAP, ex2::REAR_CAP_MOUNT, r1, r2] })?;
    ex.run(&RenameElement { id: REAR_SUB_EL, name: "Rear Cap subassembly".into() })?;
    ex.run(&SetInstancesFixed { element: REAR_SUB_EL, instances: vec![ex2::REAR_CAP], fixed: true })?;
    // A21.9–A21.10: Piston subassembly.
    let mut piston = vec![ex2::PISTON_ROD];
    piston.extend(ex2::ORINGS_185);
    ex.run(&MoveToNewSubassembly { element: ex2::ASSEMBLY, instances: piston, new_element: PISTON_SUB_EL, instance: PISTON_SUB, name: None, after: None })?;
    ex.run(&RenameElement { id: PISTON_SUB_EL, name: "Piston subassembly".into() })?;
    ex.run(&SetInstancesFixed { element: PISTON_SUB_EL, instances: vec![ex2::PISTON_ROD], fixed: true })?;
    // A21.11: the Rear Cap mount out to the top; A21.12–A21.13: dissolve, delete the tab.
    ex.run(&MoveOutOfSubassembly { element: ex2::ASSEMBLY, sub: REAR_SUB, instances: vec![ex2::REAR_CAP_MOUNT], at: Some(0) })?;
    ex.run(&DissolveSubassembly { element: ex2::ASSEMBLY, sub: PISTON_SUB })?;
    ex.run(&DeleteElement { id: PISTON_SUB_EL })?;
    // A21.14: eight hex nuts, on the four rod-hole edges on top of the Top Cap (z 6.5) and the
    // four on top of the Rear Cap's flange (z 0.75), inside their subassemblies: all eight seen
    // from above with the rods shown, where `ex3-step14.png` picks them and `ex3-drawing.png`
    // shows the nuts.
    let mut sites = ex.sites(derive(TOP_SUB, ex2::TOP_CAP), 0.375, 6.5)?;
    sites.extend(ex.sites(derive(REAR_SUB, ex2::REAR_CAP), 0.375, 0.75)?);
    let nuts: Vec<InstanceId> = (0..8).map(nut).collect();
    ex.insert(&nut_spec(), &sites, &nuts, 10)?;
    // A21.15: the nuts dragged into Hardware (after its last item).
    let asm = ex.doc.element(ex2::ASSEMBLY).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(ex2::ASSEMBLY))?;
    let to = asm.instances.iter().filter(|i| !nuts.contains(&i.id)).count();
    ex.run(&MoveListItems {
        element: ex2::ASSEMBLY,
        list: FolderList::Instances,
        items: nuts.iter().map(|n| item(*n)).collect(),
        to,
        folder: Some(HARDWARE),
        label: "Move into Hardware".into(),
    })
}

/// The Ex3 end state as a new document (`fixtures/pneumatic_cylinder_ex3.cadrs`).
pub fn document() -> Result<Document, CommandError> {
    let mut doc = ex2::document()?;
    let mut h = History::default();
    build_in(&mut doc, &mut h)?;
    Ok(doc)
}
