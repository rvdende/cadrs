//! "Exercise: Pneumatic Cylinder" (P3B.2, `intro-to-assemblies.md` A15) on the stand-in
//! `fixtures/pneumatic_cylinder_standin.cadrs`: the parts against their closed forms, the
//! implicit connector finder on them, and the whole exercise through the assembly commands and
//! the solver (A15.3–A15.20), ending with the self-check values of
//! `intro-to-assemblies-gaps.md` ("Ex2 Pneumatic Cylinder"), to 1e-4.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::Arc;

use cadrs_core::assembly::commands::{AddMateFeature, InsertInstance, MoveInstances, SetInstancesFixed};
use cadrs_core::assembly::connector::{EntityRef, ImplicitConnector, ImplicitPoint, MateConnector, implicit_points};
use cadrs_core::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, next_name};
use cadrs_core::assembly::solver::{Drive, SolveOptions};
use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::commands::{AddElement, NewElementKind, SetExtrude};
use cadrs_core::rebuild::Build;
use cadrs_core::samples::pneumatic as pc;
use cadrs_core::{Document, ElementId, History, PartId, Solid};

const IN: f64 = 25.4;
const LB: f64 = 0.453_592_37;
/// 1 g/cm³ in lb/in³.
const G_CM3: f64 = 0.036_127_292;

fn close(what: &str, got: f64, want: f64, rel: f64) {
    let tol = rel * want.abs().max(1e-9);
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (±{tol})");
}

fn cyl(d: f64, h: f64) -> f64 {
    PI * d * d / 4.0 * h
}

/// Each part's closed form (in³, the z of its centroid in the studio, g/cm³): pieces of boxes and
/// cylinders (holes negative); x = y = 0 by symmetry except the rod at (0.95, 0.95).
///
/// - Rear Cap: box 2.5 × 2.5 × 0.75 (z̄ 0.375) + spigot Ø1.5 × 0.5 (z̄ 1.0) − recess Ø1.0 × 0.1
///   (z̄ 0.05) − 4 holes Ø0.375 × 0.75 (z̄ 0.375) = 5.161193 in³.
/// - Barrel: π(1.0² − 0.875²) × 5 = 3.681554 in³ at z̄ 3.25.
/// - Top Cap: box (z̄ 6.125) + spigot Ø1.5 × 0.5 (z̄ 5.5) − Ø0.5 × 1.25 (z̄ 5.875) − 4 × Ø0.375
///   × 0.75 − 6 × Ø0.266 × 0.75 (z̄ 6.125) = 4.744230 in³.
/// - Retaining Plate: (Ø1.5 − Ø0.5 − 6 × Ø0.266) × 0.125 at z̄ 6.5625 = 0.154671 in³.
/// - Rear Cap mount: Ø1.0 × 0.5 at z̄ −0.15 = 0.392699 in³.
/// - O-Rings: π(0.875² − 0.75²) × h (h = 0.125, 0.185).
/// - Piston & Rod: Ø1.5 × 0.75 (z̄ 1.625) + Ø0.5 × 6 (z̄ 5.0) = 2.503457 in³.
/// - Structural Rod: Ø0.375 × 7.5 at z̄ 3.25 = 0.828349 in³.
fn parts_closed_form() -> Vec<(PartId, f64, f64, f64)> {
    let piece = |ps: &[(f64, f64)]| {
        let v: f64 = ps.iter().map(|p| p.0).sum();
        (v, ps.iter().map(|p| p.0 * p.1).sum::<f64>() / v)
    };
    let ring = |h: f64| cyl(1.75, h) - cyl(1.5, h);
    let list = [
        (pc::REAR_CAP, piece(&[(6.25 * 0.75, 0.375), (cyl(1.5, 0.5), 1.0), (-cyl(1.0, 0.1), 0.05), (-4.0 * cyl(0.375, 0.75), 0.375)]), 2.70),
        (pc::BARREL, piece(&[(cyl(2.0, 5.0) - cyl(1.75, 5.0), 3.25)]), 1.20),
        (
            pc::TOP_CAP,
            piece(&[
                (6.25 * 0.75, 6.125),
                (cyl(1.5, 0.5), 5.5),
                (-cyl(0.5, 1.25), 5.875),
                (-4.0 * cyl(0.375, 0.75), 6.125),
                (-6.0 * cyl(0.266, 0.75), 6.125),
            ]),
            2.70,
        ),
        (pc::RETAINING_PLATE, piece(&[(cyl(1.5, 0.125) - cyl(0.5, 0.125) - 6.0 * cyl(0.266, 0.125), 6.5625)]), 2.70),
        (pc::REAR_CAP_MOUNT, piece(&[(cyl(1.0, 0.5), -0.15)]), 2.70),
        (pc::ORING_125, piece(&[(ring(0.125), 0.8125)]), 1.00),
        (pc::ORING_185, piece(&[(ring(0.185), 1.3425)]), 1.00),
        (pc::PISTON_ROD, piece(&[(cyl(1.5, 0.75), 1.625), (cyl(0.5, 6.0), 5.0)]), 7.85),
        (pc::STRUCTURAL_ROD, piece(&[(cyl(0.375, 7.5), 3.25)]), 7.85),
    ];
    list.iter().map(|(p, (v, z), rho)| (*p, *v, *z, *rho)).collect()
}

fn studio_build(doc: &Document) -> Arc<Build> {
    cadrs_core::rebuild::build(doc.element(pc::STUDIO).unwrap().features())
}

#[test]
fn the_parts_match_their_closed_forms() {
    let doc = pc::document().unwrap();
    let build = studio_build(&doc);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 9);
    for (part, v, z, _) in parts_closed_form() {
        let p = build.part(part).unwrap_or_else(|| panic!("{part:?} missing"));
        let m = p.mass.expect("mass properties");
        close(&format!("{part:?} volume"), m.volume / IN.powi(3), v, 1e-4);
        close(&format!("{part:?} z̄"), m.center_of_mass.z / IN, z, 1e-4);
    }
}

#[test]
fn the_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test course_pneumatic`.
    let doc = pc::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pneumatic_cylinder_standin.cadrs");
    let text = ron::ser::to_string_pretty(&pc::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/pneumatic_cylinder_standin.cadrs is out of date");
}

/// The circular edge of `s` centred at `c` (in) with diameter `d` (in).
fn circle_edge(s: &Solid, c: [f64; 3], d: f64) -> cadrs_core::solid::EdgeName {
    s.edges
        .iter()
        .find(|e| {
            e.circle.is_some_and(|k| {
                (k.radius * 2.0 / IN - d).abs() < 1e-6 && (0..3).all(|i| (k.center[i] / IN - c[i]).abs() < 1e-6)
            })
        })
        .unwrap_or_else(|| panic!("no Ø{d} circle at {c:?}"))
        .name
}

/// The implicit connector at the centre of that circle (A6.6: a circular edge has only it).
fn centre(s: &Solid, c: [f64; 3], d: f64) -> ImplicitConnector {
    let e = circle_edge(s, c, d);
    let pts = implicit_points(s, &EntityRef::Edge(e));
    assert_eq!(pts.len(), 1, "a circular edge gives one point");
    assert!(matches!(pts[0].point, ImplicitPoint::CircleCenter(_)));
    pts[0]
}

#[test]
fn implicit_points_on_the_parts() {
    let doc = pc::document().unwrap();
    let build = studio_build(&doc);
    let top = &build.part(pc::TOP_CAP).unwrap().solid;
    // A flange hole's top edge: its centre, Z along +Z (the hole's axis), Ø0.375.
    let p = centre(top, [0.95, 0.95, 6.5], 0.375);
    assert!((p.frame.origin[2] / IN - 6.5).abs() < 1e-9);
    assert_eq!(p.frame.z, [0.0, 0.0, 1.0]);
    assert!((p.diameter.unwrap() / IN - 0.375).abs() < 1e-9);
    // The Top Cap's top face: its centroid, every hole's centre, the square's midpoints and
    // corners; Z the outward normal.
    let face = top.faces.iter().find(|f| f.plane.is_some_and(|pl| (pl.origin[2] / IN - 6.5).abs() < 1e-9)).unwrap();
    let pts = implicit_points(top, &EntityRef::Face(face.name));
    let count = |f: fn(&ImplicitPoint) -> bool| pts.iter().filter(|p| f(&p.point)).count();
    assert_eq!(count(|p| matches!(p, ImplicitPoint::FaceCentroid(_))), 1);
    assert_eq!(count(|p| matches!(p, ImplicitPoint::CircleCenter(_))), 11, "Ø0.5, 4 × Ø0.375, 6 × Ø0.266");
    assert_eq!(count(|p| matches!(p, ImplicitPoint::EdgeMidpoint(_))), 4);
    assert_eq!(count(|p| matches!(p, ImplicitPoint::Vertex(_))), 4);
    let c = pts.iter().find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_))).unwrap();
    assert_eq!(c.frame.z, [0.0, 0.0, 1.0]);
    assert!(c.frame.origin[0].abs() < 1e-6 && c.frame.origin[1].abs() < 1e-6);
    // A hole's cylindrical face: the middle of its axis (the centre of the cut space).
    let hole = top.faces.iter().find(|f| f.plane.is_none() && top.edges.iter().any(|e| e.name.touches(&f.name) && e.circle.is_some_and(|k| (k.radius * 2.0 / IN - 0.375).abs() < 1e-6))).unwrap();
    let pts = implicit_points(top, &EntityRef::Face(hole.name));
    let mid = pts.iter().find(|p| matches!(p.point, ImplicitPoint::AxisMiddle(_))).expect("negative-space centre");
    assert!((mid.frame.origin[2] / IN - 6.125).abs() < 1e-9, "{:?}", mid.frame.origin);
}

#[test]
fn a_connector_follows_a_studio_edit() {
    // X5: an implicit connector on the Piston & Rod's top edge re-resolves by name after the rod
    // is made 1 in longer.
    let mut doc = pc::document().unwrap();
    let mut h = History::default();
    let build = studio_build(&doc);
    let s = &build.part(pc::PISTON_ROD).unwrap().solid;
    let p = centre(s, [0.0, 0.0, 8.0], 0.5);
    let conn = MateConnector::implicit(InstanceId::from_u128(1), &p);
    let mut e = doc.element(pc::STUDIO).unwrap().feature(pc::PISTON_ROD_E).unwrap().extrude().unwrap().clone();
    e.depth = 7.0 * IN;
    e.depth_expr = "7 in".into();
    h.execute(&mut doc, &SetExtrude { element: pc::STUDIO, feature: pc::PISTON_ROD_E, extrude: e, label: "Extrude".into() }).unwrap();
    let build = studio_build(&doc);
    let f = conn.local_frame(Some(&build.part(pc::PISTON_ROD).unwrap().solid));
    assert!((f.origin[2] / IN - 9.0).abs() < 1e-9, "{:?}", f.origin);
    // Without the part, the stored frame.
    assert!((conn.local_frame(None).origin[2] / IN - 8.0).abs() < 1e-9);
}

/// The exercise, as `course_asm_ex2_pneumatic` does it through the UI.
struct Ex2 {
    doc: Document,
    h: History,
    asm: ElementId,
    build: Arc<Build>,
    next: u128,
}

impl Ex2 {
    fn new() -> Self {
        let mut doc = pc::document().unwrap();
        let mut h = History::default();
        let asm = ElementId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_0201);
        h.execute(&mut doc, &AddElement { id: asm, kind: NewElementKind::Assembly, name: Some("Cylinder assembly".into()), after: None })
            .unwrap();
        let build = studio_build(&doc);
        Self { doc, h, asm, build, next: 1 }
    }

    fn model(&self) -> &assembly::Assembly {
        self.doc.element(self.asm).unwrap().assembly_model().unwrap()
    }

    fn insert(&mut self, part: PartId, pose: Pose) -> InstanceId {
        let id = InstanceId::from_u128(self.next);
        self.next += 1;
        let source = InstanceSource::Part { element: pc::STUDIO, part };
        self.h.execute(&mut self.doc, &InsertInstance { element: self.asm, instance: Instance::new(id, source, pose) }).unwrap();
        id
    }

    fn solids(&self) -> HashMap<InstanceId, Arc<Solid>> {
        let b = self.build.clone();
        assembly::source_solids(self.model(), |_| Some(b.clone()))
    }

    fn solid(&self, i: InstanceId) -> Arc<Solid> {
        self.solids()[&i].clone()
    }

    fn solve_into(&mut self, feature: Option<MateFeature>, opts: SolveOptions) {
        let mut model = self.model().clone();
        if let Some(f) = &feature {
            model.mates.push(f.clone());
        }
        let s = assembly::solve(&model, &self.solids(), &opts);
        assert!(s.converged, "residual {}", s.residual);
        let poses = s.changed(self.model());
        match feature {
            Some(feature) => {
                self.h.execute(&mut self.doc, &AddMateFeature { element: self.asm, feature, poses }).unwrap();
            }
            None if !poses.is_empty() => {
                self.h.execute(&mut self.doc, &MoveInstances { element: self.asm, poses, label: "Reset".into() }).unwrap();
            }
            None => {}
        }
    }

    /// A mate between the centres of two circular edges (instance, centre in, Ø in).
    #[allow(clippy::too_many_arguments)]
    fn mate(&mut self, t: MateType, a: (InstanceId, [f64; 3], f64), b: (InstanceId, [f64; 3], f64), flip: bool, offset_z: f64, limits: Option<MateLimits>) -> MateId {
        let mut c1 = MateConnector::implicit(a.0, &centre(&self.solid(a.0), a.1, a.2));
        let c2 = MateConnector::implicit(b.0, &centre(&self.solid(b.0), b.1, b.2));
        c1.flip = flip;
        let mut m = Mate::new(t, c1, c2);
        if offset_z != 0.0 {
            m.offset = Some(MateOffset { translation: [0.0, 0.0, offset_z * IN], ..Default::default() });
        }
        m.limits = limits;
        let id = MateId::from_u128(self.next);
        self.next += 1;
        let name = next_name(&self.model().mates, t.label());
        let movers = vec![a.0];
        self.solve_into(Some(MateFeature::new(id, name, MateKind::Mate(m))), SolveOptions { movers, snap: Some(id), ..Default::default() });
        id
    }

    fn mass(&self) -> cadrs_core::parts::MassReport {
        let b = self.build.clone();
        let (parts, props) = assembly::instance_parts(&self.doc, self.model(), |_| Some(b.clone()));
        let ids: Vec<InstanceId> = self.model().instances.iter().map(|i| i.id).collect();
        assembly::mass_report(&parts, &props, &ids).unwrap()
    }
}

/// The self-check: every part at its assembled place (A15 end state; the piston at z 1.25 after
/// Reset). Mass = Σ ρᵢ Vᵢ over 17 instances (4 O-Ring 0.125 at z̄ 0.8125, 1.0625, 5.6875, 5.4375;
/// 3 O-Ring 0.185 at 1.3425, 1.5275, 1.7125; 4 rods at z̄ 3.25), CoM z = Σ ρᵢ Vᵢ z̄ᵢ / mass; the
/// piston and its three O-rings raised by `lift` in.
fn ex2_closed_form(lift: f64) -> (f64, f64, f64) {
    let parts: HashMap<PartId, (f64, f64, f64)> = parts_closed_form().into_iter().map(|(p, v, z, r)| (p, (v, z, r))).collect();
    let mut list: Vec<(f64, f64, f64)> = Vec::new();
    for p in [pc::BARREL, pc::REAR_CAP, pc::RETAINING_PLATE, pc::TOP_CAP, pc::REAR_CAP_MOUNT] {
        list.push(parts[&p]);
    }
    let (v, _, r) = parts[&pc::ORING_125];
    for z in [0.75, 1.0, 5.625, 5.375] {
        list.push((v, z + 0.0625, r));
    }
    for _ in 0..4 {
        list.push(parts[&pc::STRUCTURAL_ROD]);
    }
    let (v, z, r) = parts[&pc::PISTON_ROD];
    list.push((v, z + lift, r));
    let (v, _, r) = parts[&pc::ORING_185];
    for k in 0..3 {
        list.push((v, 1.25 + 0.185 * k as f64 + 0.0925 + lift, r));
    }
    assert_eq!(list.len(), 17);
    let vol: f64 = list.iter().map(|p| p.0).sum();
    let mass: f64 = list.iter().map(|p| p.0 * p.2).sum();
    let cz = list.iter().map(|p| p.0 * p.2 * p.1).sum::<f64>() / mass;
    (vol, mass * G_CM3, cz)
}

#[test]
fn ex2_pneumatic_cylinder_mass_properties() {
    let mut ex = Ex2::new();
    // A15.3: Barrel, Rear Cap, Retaining Plate, Top Cap at their studio positions.
    let barrel = ex.insert(pc::BARREL, Pose::IDENTITY);
    let rear = ex.insert(pc::REAR_CAP, Pose::IDENTITY);
    let plate = ex.insert(pc::RETAINING_PLATE, Pose::IDENTITY);
    let top = ex.insert(pc::TOP_CAP, Pose::IDENTITY);
    // A15.4: Fix the Barrel. A15.5: Group the four.
    ex.h.execute(&mut ex.doc, &SetInstancesFixed { element: ex.asm, instances: vec![barrel], fixed: true }).unwrap();
    let group = MateFeature::new(MateId::from_u128(900), "Group 1", MateKind::Group { instances: vec![rear, plate, top, barrel] });
    ex.solve_into(Some(group), SolveOptions::default());
    // A15.6–A15.7: the Rear Cap mount, clicked in off to the side; Revolute: the recess edge,
    // then the mount's top edge.
    let off = |x: f64, y: f64| Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], 0.4).then(&Pose::translation([x * IN, y * IN, -2.0 * IN]));
    let mount = ex.insert(pc::REAR_CAP_MOUNT, off(3.0, -2.0));
    ex.mate(MateType::Revolute, (rear, [0.0, 0.0, 0.1], 1.0), (mount, [0.0, 0.0, 0.1], 1.0), false, 0.0, None);
    // A15.8–A15.10: four O-rings, each flipped, as the scenario picks them: two by their top inner
    // edge on the Rear Cap's spigot (its base edge; its top edge with Offset Z −0.25 in), two by
    // their bottom inner edge (seen from below) on the Top Cap's spigot (its base edge; its lower
    // edge with Offset Z 0.25 in). The stand-in has no grooves; no edge is picked twice.
    let rings: Vec<InstanceId> = (0..4).map(|k| ex.insert(pc::ORING_125, off(-3.0, k as f64))).collect();
    ex.mate(MateType::Fastened, (rings[0], [0.0, 0.0, 0.875], 1.5), (rear, [0.0, 0.0, 0.75], 1.5), true, 0.0, None);
    ex.mate(MateType::Fastened, (rings[1], [0.0, 0.0, 0.875], 1.5), (rear, [0.0, 0.0, 1.25], 1.5), true, -0.25, None);
    ex.mate(MateType::Fastened, (rings[2], [0.0, 0.0, 0.75], 1.5), (top, [0.0, 0.0, 5.75], 1.5), true, 0.0, None);
    ex.mate(MateType::Fastened, (rings[3], [0.0, 0.0, 0.75], 1.5), (top, [0.0, 0.0, 5.25], 1.5), true, 0.25, None);
    // A15.11–A15.14: a Structural Rod, Fastened with Offset Z 0.5 in, copied three times.
    let holes = [(0.95, 0.95), (-0.95, 0.95), (-0.95, -0.95), (0.95, -0.95)];
    for (k, (x, y)) in holes.iter().enumerate() {
        let rod = ex.insert(pc::STRUCTURAL_ROD, off(4.0 + k as f64, 2.0));
        ex.mate(MateType::Fastened, (rod, [0.95, 0.95, 7.0], 0.375), (top, [*x, *y, 6.5], 0.375), false, 0.5, None);
    }
    // A15.15–A15.16: the Piston & Rod; Slider (the piston's bottom edge, then the Rear Cap's
    // spigot top edge), limits Z −3.25…0 in.
    let piston = ex.insert(pc::PISTON_ROD, off(6.0, -3.0));
    let limits = Some(MateLimits { z: Some((-pc::TRAVEL * IN, 0.0)), ..Default::default() });
    let slider = ex.mate(MateType::Slider, (piston, [0.0, 0.0, 1.25], 1.5), (rear, [0.0, 0.0, 1.25], 1.5), false, 0.0, limits);
    // A15.17–A15.18: three O-Ring 0.185 stacked on the piston from its bottom (their bottom inner
    // edge on the piston's top edge, Offset Z −0.75, −0.565, −0.38 in: at the zero position the
    // piston's bottom edge lies on the Rear Cap's spigot edge).
    for k in 0..3 {
        let ring = ex.insert(pc::ORING_185, off(-5.0, k as f64));
        ex.mate(MateType::Fastened, (ring, [0.0, 0.0, 1.25], 1.5), (piston, [0.0, 0.0, 2.0], 1.5), false, -0.75 + 0.185 * k as f64, None);
    }
    // The end state: 17 instances, 14 mates in creation order.
    assert_eq!(ex.model().instances.len(), 17);
    let names: Vec<&str> = ex.model().mates.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Group 1", "Revolute 1", "Fastened 1", "Fastened 2", "Fastened 3", "Fastened 4", "Fastened 5", "Fastened 6", "Fastened 7",
            "Fastened 8", "Slider 1", "Fastened 9", "Fastened 10", "Fastened 11"
        ]
    );
    // DOF (A16.1): the mount turns, the piston and its O-rings slide; the rest are held.
    let dofs = assembly::instance_dofs(ex.model(), &ex.solids());
    assert_eq!(dofs[&mount], 1);
    assert_eq!(dofs[&piston], 1);
    assert_eq!(dofs[&rings[0]], 0);
    assert_eq!(dofs[&barrel], 0);
    // A15.19: Slider 1 → Reset (its zero position), A15.20: mass properties.
    let reset = SolveOptions { movers: vec![piston], snap: Some(slider), drives: vec![Drive { mate: slider, dof: Dof::Z, value: 0.0 }], ..Default::default() };
    ex.solve_into(None, reset);
    let r = ex.mass();
    let m = r.mass.expect("materials");
    let (v, mass, cz) = ex2_closed_form(0.0);
    close("V (in³)", r.volume / IN.powi(3), v, 1e-4);
    close("mass (lb)", m.mass / LB, mass, 1e-4);
    close("CoM z (in)", m.center_of_mass.z / IN, cz, 1e-4);
    assert!(m.center_of_mass.x.abs() < 1e-6 * IN && m.center_of_mass.y.abs() < 1e-6 * IN);
    close("recorded V", v, 20.62443, 1e-6);
    close("recorded mass", mass, 2.85319, 1e-5);
    close("recorded CoM z", cz, 3.16698, 1e-5);
    // Apply limit position: the slider at −3.25 in lifts the piston and its O-rings 3.25 in.
    let top_limit = SolveOptions { movers: vec![piston], snap: Some(slider), drives: vec![Drive { mate: slider, dof: Dof::Z, value: -pc::TRAVEL * IN }], ..Default::default() };
    ex.solve_into(None, top_limit);
    let m = ex.mass().mass.unwrap();
    let (_, _, cz) = ex2_closed_form(pc::TRAVEL);
    close("CoM z at the limit (in)", m.center_of_mass.z / IN, cz, 1e-4);
    close("recorded CoM z at the limit", cz, 3.99028, 1e-5);
    // Undo goes back one step at a time: Reset's placements.
    ex.h.undo(&mut ex.doc).unwrap();
    close("undone", ex.mass().mass.unwrap().center_of_mass.z / IN, ex2_closed_form(0.0).2, 1e-4);
}
