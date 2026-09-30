//! The end state of "Exercise: Pneumatic Cylinder" (A15) on the stand-in, as "Exercise: Assembly
//! structure" (A21) starts from it: the Part Studio "Cylinder parts" and the Assembly "Cylinder
//! assembly" with 17 instances and 14 mates (Group 1, Revolute 1, Fastened 1–11, Slider 1), the
//! Barrel fixed and the piston at its Reset position (`intro-to-assemblies-gaps.md`, "Ex2
//! Pneumatic Cylinder" and "Ex3 Assembly Structure"). Built through the assembly commands and
//! the solver, as `course_asm_ex2_pneumatic` builds it through the UI, with fixed ids, so
//! `fixtures/pneumatic_cylinder_ex2.cadrs` is regenerated exactly
//! (`cadrs_core/tests/course_ex3_structure.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use super::pneumatic as pc;
use crate::assembly::commands::{AddMateFeature, InsertInstance, MoveInstances, SetInstancesFixed};
use crate::assembly::connector::{EntityRef, ImplicitPoint, MateConnector, implicit_points};
use crate::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, next_name};
use crate::assembly::solver::{Drive, SolveOptions};
use crate::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, NewElementKind};
use crate::document::Document;
use crate::ids::{ElementId, PartId};
use crate::rebuild::Build;
use crate::solid::Solid;

const IN: f64 = pc::IN;

/// The Assembly "Cylinder assembly".
pub const ASSEMBLY: ElementId = ElementId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_0201);

const fn inst(n: u128) -> InstanceId {
    InstanceId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_1000 + n)
}

pub const BARREL: InstanceId = inst(1);
pub const REAR_CAP: InstanceId = inst(2);
pub const RETAINING_PLATE: InstanceId = inst(3);
pub const TOP_CAP: InstanceId = inst(4);
pub const REAR_CAP_MOUNT: InstanceId = inst(5);
/// O-Ring 0.125 <1>–<4>: two on the Rear Cap, two on the Top Cap.
pub const ORINGS_125: [InstanceId; 4] = [inst(6), inst(7), inst(8), inst(9)];
pub const RODS: [InstanceId; 4] = [inst(10), inst(11), inst(12), inst(13)];
pub const PISTON_ROD: InstanceId = inst(14);
pub const ORINGS_185: [InstanceId; 3] = [inst(15), inst(16), inst(17)];

const fn mate_id(n: u128) -> MateId {
    MateId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_2000 + n)
}

pub const GROUP_1: MateId = mate_id(1);
pub const REVOLUTE_1: MateId = mate_id(2);
pub const SLIDER_1: MateId = mate_id(20);

struct Ex2<'a> {
    doc: &'a mut Document,
    h: &'a mut History,
    build: Arc<Build>,
    next_mate: u128,
}

/// The circular edge of `s` centred at `c` (in) with diameter `d` (in), and its centre as an
/// implicit connector (a circular edge has only that point, A6.6).
fn centre(s: &Solid, c: [f64; 3], d: f64) -> Result<crate::assembly::connector::ImplicitConnector, CommandError> {
    let e = s
        .edges
        .iter()
        .find(|e| e.circle.is_some_and(|k| (k.radius * 2.0 / IN - d).abs() < 1e-6 && (0..3).all(|i| (k.center[i] / IN - c[i]).abs() < 1e-6)))
        .ok_or_else(|| CommandError::Invalid(format!("no Ø{d} circle at {c:?}")))?
        .name;
    implicit_points(s, &EntityRef::Edge(e))
        .into_iter()
        .find(|p| matches!(p.point, ImplicitPoint::CircleCenter(_)))
        .ok_or_else(|| CommandError::Invalid("no centre".into()))
}

impl Ex2<'_> {
    fn model(&self) -> &assembly::Assembly {
        self.doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).expect("the assembly")
    }

    fn insert(&mut self, id: InstanceId, part: PartId, pose: Pose) -> Result<(), CommandError> {
        let source = InstanceSource::Part { element: pc::STUDIO, part };
        self.h.execute(self.doc, &InsertInstance { element: ASSEMBLY, instance: Instance::new(id, source, pose) })
    }

    fn solids(&self) -> HashMap<InstanceId, Arc<Solid>> {
        let b = self.build.clone();
        assembly::source_solids(self.model(), |_| Some(b.clone()))
    }

    fn solve_into(&mut self, feature: Option<MateFeature>, opts: SolveOptions) -> Result<(), CommandError> {
        let mut model = self.model().clone();
        if let Some(f) = &feature {
            model.mates.push(f.clone());
        }
        let s = assembly::solve(&model, &self.solids(), &opts);
        if !s.converged {
            return Err(CommandError::Invalid(format!("the solve left {}", s.residual)));
        }
        let poses = s.changed(self.model());
        match feature {
            Some(feature) => self.h.execute(self.doc, &AddMateFeature { element: ASSEMBLY, feature, poses }),
            None if !poses.is_empty() => self.h.execute(self.doc, &MoveInstances { element: ASSEMBLY, poses, label: "Reset".into() }),
            None => Ok(()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mate(&mut self, t: MateType, a: (InstanceId, [f64; 3], f64), b: (InstanceId, [f64; 3], f64), flip: bool, offset_z: f64, limits: Option<MateLimits>) -> Result<MateId, CommandError> {
        let solids = self.solids();
        let mut c1 = MateConnector::implicit(a.0, &centre(&solids[&a.0], a.1, a.2)?);
        let c2 = MateConnector::implicit(b.0, &centre(&solids[&b.0], b.1, b.2)?);
        c1.flip = flip;
        let mut m = Mate::new(t, c1, c2);
        if offset_z != 0.0 {
            m.offset = Some(MateOffset { translation: [0.0, 0.0, offset_z * IN], ..Default::default() });
        }
        m.limits = limits;
        self.next_mate += 1;
        let id = mate_id(self.next_mate);
        let name = next_name(&self.model().mates, t.label());
        self.solve_into(Some(MateFeature::new(id, name, MateKind::Mate(m))), SolveOptions { movers: vec![a.0], snap: Some(id), ..Default::default() })?;
        Ok(id)
    }
}

/// Adds "Cylinder assembly" to the stand-in document `doc` (from [`pc::document`]) through the
/// commands of the exercise (A15.2–A15.19).
pub fn build_in(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    h.execute(doc, &AddElement { id: ASSEMBLY, kind: NewElementKind::Assembly, name: Some("Cylinder assembly".into()), after: None })?;
    let build = crate::rebuild::build(doc.element(pc::STUDIO).ok_or(CommandError::ElementNotFound(pc::STUDIO))?.features());
    let mut ex = Ex2 { doc, h, build, next_mate: 2 };
    // A15.3–A15.5: four parts at their studio positions, the Barrel fixed, the four grouped.
    for (i, p) in [(BARREL, pc::BARREL), (REAR_CAP, pc::REAR_CAP), (RETAINING_PLATE, pc::RETAINING_PLATE), (TOP_CAP, pc::TOP_CAP)] {
        ex.insert(i, p, Pose::IDENTITY)?;
    }
    ex.h.execute(ex.doc, &SetInstancesFixed { element: ASSEMBLY, instances: vec![BARREL], fixed: true })?;
    let group = MateFeature::new(GROUP_1, "Group 1", MateKind::Group { instances: vec![REAR_CAP, RETAINING_PLATE, TOP_CAP, BARREL] });
    ex.solve_into(Some(group), SolveOptions::default())?;
    // A15.6–A15.7: the mount clicked in beside the cylinder; Revolute 1.
    let off = |x: f64, y: f64| Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], 0.4).then(&Pose::translation([x * IN, y * IN, -2.0 * IN]));
    ex.insert(REAR_CAP_MOUNT, pc::REAR_CAP_MOUNT, off(3.0, -2.0))?;
    ex.next_mate = 1;
    ex.mate(MateType::Revolute, (REAR_CAP, [0.0, 0.0, 0.1], 1.0), (REAR_CAP_MOUNT, [0.0, 0.0, 0.1], 1.0), false, 0.0, None)?;
    ex.next_mate = 2;
    // A15.8–A15.10: four O-rings, Fastened 1–4 (the stand-in has no grooves: offsets instead).
    for (k, i) in ORINGS_125.iter().enumerate() {
        ex.insert(*i, pc::ORING_125, off(-3.0, k as f64))?;
    }
    let [r1, r2, r3, r4] = ORINGS_125;
    ex.mate(MateType::Fastened, (r1, [0.0, 0.0, 0.875], 1.5), (REAR_CAP, [0.0, 0.0, 0.75], 1.5), true, 0.0, None)?;
    ex.mate(MateType::Fastened, (r2, [0.0, 0.0, 0.875], 1.5), (REAR_CAP, [0.0, 0.0, 1.25], 1.5), true, -0.25, None)?;
    ex.mate(MateType::Fastened, (r3, [0.0, 0.0, 0.75], 1.5), (TOP_CAP, [0.0, 0.0, 5.75], 1.5), true, 0.0, None)?;
    ex.mate(MateType::Fastened, (r4, [0.0, 0.0, 0.75], 1.5), (TOP_CAP, [0.0, 0.0, 5.25], 1.5), true, 0.25, None)?;
    // A15.11–A15.14: the rods, Fastened 5–8 with Offset Z 0.5 in.
    let holes = [(0.95, 0.95), (-0.95, 0.95), (-0.95, -0.95), (0.95, -0.95)];
    for (k, (x, y)) in holes.iter().enumerate() {
        ex.insert(RODS[k], pc::STRUCTURAL_ROD, off(4.0 + k as f64, 2.0))?;
        ex.mate(MateType::Fastened, (RODS[k], [0.95, 0.95, 7.0], 0.375), (TOP_CAP, [*x, *y, 6.5], 0.375), false, 0.5, None)?;
    }
    // A15.15–A15.16: the Piston & Rod on Slider 1, limits Z −3.25…0 in.
    ex.insert(PISTON_ROD, pc::PISTON_ROD, off(6.0, -3.0))?;
    let limits = Some(MateLimits { z: Some((-pc::TRAVEL * IN, 0.0)), ..Default::default() });
    ex.next_mate = 19;
    ex.mate(MateType::Slider, (PISTON_ROD, [0.0, 0.0, 1.25], 1.5), (REAR_CAP, [0.0, 0.0, 1.25], 1.5), false, 0.0, limits)?;
    // A15.17–A15.18: three O-Ring 0.185, Fastened 9–11.
    for (k, i) in ORINGS_185.iter().enumerate() {
        ex.insert(*i, pc::ORING_185, off(-5.0, k as f64))?;
        ex.mate(MateType::Fastened, (*i, [0.0, 0.0, 1.25], 1.5), (PISTON_ROD, [0.0, 0.0, 2.0], 1.5), false, -0.75 + 0.185 * k as f64, None)?;
    }
    // A15.19: Slider 1 → Reset.
    let reset = SolveOptions { movers: vec![PISTON_ROD], snap: Some(SLIDER_1), drives: vec![Drive { mate: SLIDER_1, dof: Dof::Z, value: 0.0 }], ..Default::default() };
    ex.solve_into(None, reset)
}

/// The Ex2 end state as a new document (`fixtures/pneumatic_cylinder_ex2.cadrs`).
pub fn document() -> Result<Document, CommandError> {
    let mut doc = pc::document()?;
    doc.name = "Exercise: Assembly structure (stand-in)".into();
    doc.id = crate::ids::DocumentId::from_u128(0x3b04_0000_0000_0000_0000_0000_0000_0100);
    let mut h = History::default();
    build_in(&mut doc, &mut h)?;
    Ok(doc)
}
