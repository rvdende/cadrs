//! P3.5: materials, mass, centre of mass and inertia (PS10.4, X7), and the palette colours
//! parts are made with (PS9.1), through the document's commands and the rebuild. Every expected
//! value is derived by hand in the test's comment.
//!
//! **Convention** (recorded in `crates/cadrs_kernel/README.md`): the inertia tensor is about the centre of
//! mass, with axes parallel to the Part Studio's, as Onshape's Mass and section properties
//! panel gives it when no mate connector is set for the reference frame ("Mass moments of
//! inertia … computed at the center of mass, aligned with the default coordinate system"); the
//! diagonal holds the moments (`Lxx = ∫(y² + z²) dm`) and the products carry the tensor's minus
//! sign (`Lxy = −∫xy dm`).
#![cfg(feature = "occt")]

use cadrs_core::appearance::{PALETTE, palette, part_appearance};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetPartMaterial};
use cadrs_core::document::{BooleanOp, Document, ExtrudeFeature};
use cadrs_core::material::{self, Material};
use cadrs_core::parts::mass_report;
use cadrs_core::rebuild;
use cadrs_core::samples::{self, EXTRUDE_1_SEEDS, EXTRUDE_2_SEEDS};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, PartId};
use cadrs_sketch::units::{LengthUnit, MassUnit, Units};
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3.5");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.d.element(self.el).unwrap().feature(f).unwrap().sketch().unwrap().geometry.clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn extrude(&mut self, sketch: FeatureId, seeds: &[Vec2], depth: f64, op: BooleanOp) -> FeatureId {
        let regions = samples::region_refs(sketch, &self.g(sketch), seeds);
        assert_eq!(regions.len(), seeds.len());
        let extrude = ExtrudeFeature { op, ..samples::extrude_of(regions, depth) };
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude, label: "Extrude".into() })
            .unwrap();
        f
    }

    fn parts(&self) -> Vec<Part> {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }

    fn assign(&mut self, parts: Vec<PartId>, m: Option<Material>) {
        self.h.execute(&mut self.d, &SetPartMaterial { element: self.el, parts, material: m }).unwrap();
    }

    fn props(&self) -> Vec<cadrs_core::PartProps> {
        self.d.element(self.el).unwrap().part_props().to_vec()
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

/// A 100 mm steel cube (ρ 7.85 g/cm³ = 7850 kg/m³ = 7.85e-6 kg/mm³): V = 10⁶ mm³, so
/// m = 7.85 kg; about its centre (50, 50, 50) Ixx = Iyy = Izz = m(b² + c²)/12 =
/// 7.85·(100² + 100²)/12 = 13 083.333 kg·mm², no products. Before a material: no mass (the
/// panel leaves it blank). In pounds and inches: 7.85 / 0.45359237 = 17.306 lb and
/// 13 083.333 / 0.45359237 / 25.4² = 13 083.333 / 292.640 = 44.708 lb·in².
#[test]
fn steel_cube_mass_and_inertia() {
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 100.0)]);
    doc.extrude(s, &[Vec2::new(50.0, 50.0)], 100.0, BooleanOp::New);
    let parts = doc.parts();
    let cube = &parts[0];
    let r = mass_report(&[cube], &doc.props()).unwrap();
    close(r.volume, 1e6, 1e-6);
    assert!(r.mass.is_none(), "no material: no mass");
    let steel = material::library("Steel").unwrap();
    assert_eq!(steel.density, 7850.0);
    doc.assign(vec![cube.id], Some(steel));
    let r = mass_report(&[cube], &doc.props()).unwrap();
    let m = r.mass.expect("a mass with a material");
    close(m.mass, 7.85, 1e-9);
    for i in 0..3 {
        close(m.center_of_mass[i], 50.0, 1e-9);
        close(m.inertia[(i, i)], 13_083.333_333, 1e-6);
    }
    for (a, b) in [(0, 1), (0, 2), (1, 2), (1, 0), (2, 0), (2, 1)] {
        close(m.inertia[(a, b)], 0.0, 1e-6);
    }
    // The panel's readouts: metric (the default) and imperial.
    let mm = Units::default();
    assert_eq!(mm.mass(m.mass), "7.850 kg");
    assert_eq!(mm.inertia(m.inertia[(0, 0)]), "13083.333");
    let inch = Units::new(LengthUnit::Inch, 3).with_mass(MassUnit::Pound);
    assert_eq!(inch.mass(m.mass), "17.306 lb");
    assert_eq!(inch.inertia(m.inertia[(0, 0)]), "44.708");
    assert_eq!(Units::default().with_mass(MassUnit::Gram).mass(m.mass), "7850.000 g");
    assert_eq!(Units::default().with_mass(MassUnit::Ounce).mass(m.mass), "276.901 oz");
    // Removing the material undoes to no mass.
    doc.assign(vec![cube.id], None);
    assert!(mass_report(&[cube], &doc.props()).unwrap().mass.is_none());
}

/// PS10 on the Control Arm (PS6, Extrude 2 Add: one part): in Polypropylene (the course's
/// 0.033 lb/in³ = 913.437 kg/m³, see `material`) its mass is V × ρ = 368 749.705 mm³ ×
/// 913.437e-9 kg/mm³ = 0.336 830 kg (0.337 kg; 0.336 830 / 0.453 592 37 = 0.743 lb). Its centre of mass lies on y = 0
/// (the arm is symmetric about the x axis), and the tensor is symmetric with positive moments;
/// Lzz, about the vertical axis through the centre, is the largest (the arm is long and flat).
#[test]
fn control_arm_in_polypropylene() {
    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Top, vec![samples::control_arm_geometry()]);
    doc.extrude(s1, &EXTRUDE_1_SEEDS, samples::EXTRUDE_1_DEPTH, BooleanOp::New);
    doc.extrude(s1, &EXTRUDE_2_SEEDS, samples::EXTRUDE_2_DEPTH, BooleanOp::Add);
    let parts = doc.parts();
    assert_eq!(parts.len(), 1);
    let arm = &parts[0];
    let v = arm.mass.unwrap().volume;
    close(v, 368_749.705, 0.01);
    doc.assign(vec![arm.id], material::library("Polypropylene"));
    let m = mass_report(&[arm], &doc.props()).unwrap().mass.unwrap();
    close(m.mass, v * material::POLYPROPYLENE_DENSITY * 1e-9, 1e-12);
    close(m.mass, 0.336_830, 1e-6);
    assert_eq!(Units::default().mass(m.mass), "0.337 kg");
    assert_eq!(Units::new(LengthUnit::Inch, 3).with_mass(MassUnit::Pound).mass(m.mass), "0.743 lb");
    close(m.center_of_mass.y, 0.0, 1e-6);
    // One material: the volume centroid.
    assert!((m.center_of_mass - arm.mass.unwrap().center_of_mass).norm() < 1e-9);
    let i = m.inertia;
    close(i[(0, 1)], i[(1, 0)], 1e-9);
    close(i[(0, 2)], i[(2, 0)], 1e-9);
    // Symmetric about y = 0: no xy or yz product.
    close(i[(0, 1)], 0.0, 1e-6);
    close(i[(1, 2)], 0.0, 1e-6);
    assert!(i[(2, 2)] > i[(1, 1)] && i[(1, 1)] > i[(0, 0)] && i[(0, 0)] > 0.0);
    // The kernel's unit-density tensor times ρ.
    close(i[(2, 2)], arm.mass.unwrap().inertia[(2, 2)] * material::POLYPROPYLENE_DENSITY * 1e-9, 1e-9);
}

/// Two parts measured together, of different materials: a 10 mm steel cube A at (0..10)³
/// (V 1000, m_A = 7.85e-3 kg, centre (5, 5, 5)) and a 10 mm aluminium 6061 cube B at
/// (20..30) × (0..10) × (0..10) (ρ 2700 kg/m³: m_B = 2.7e-3 kg, centre (25, 5, 5)).
/// M = 10.55e-3 kg; C_x = (7.85·5 + 2.7·25)/10.55 = 106.75/10.55 = 10.118 48; C_y = C_z = 5.
/// Each cube's own Iyy about its centre is m·(10² + 10²)/12 = m·16.667; the parallel-axis term
/// for Iyy is m·dx²: dx_A = −5.118 48, dx_B = 14.881 52, so
/// Iyy = 10.55e-3·16.6667 + 7.85e-3·26.198 87 + 2.7e-3·221.459 55 = 0.175 833 + 0.205 661 +
/// 0.597 941 = 0.979 436 kg·mm²; Ixx has no offset (dy = dz = 0): 10.55e-3·16.6667 = 0.175 833.
/// A third part without a material blanks the mass.
#[test]
fn several_parts_of_different_materials() {
    let mut doc = Doc::new();
    let s = doc.sketch(
        PlaneRef::Top,
        vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 30.0, 10.0), rect(40.0, 0.0, 50.0, 10.0)],
    );
    let e = doc.extrude(s, &[Vec2::new(5.0, 5.0), Vec2::new(25.0, 5.0), Vec2::new(45.0, 5.0)], 10.0, BooleanOp::New);
    let parts = doc.parts();
    assert_eq!(parts.len(), 3);
    let by_x = |x: f64| parts.iter().find(|p| (p.mass.unwrap().center_of_mass.x - x).abs() < 1e-6).unwrap();
    let (a, b, c) = (by_x(5.0), by_x(25.0), by_x(45.0));
    doc.assign(vec![a.id], material::library("Steel"));
    doc.assign(vec![b.id], material::library("Aluminum - 6061"));
    let m = mass_report(&[a, b], &doc.props()).unwrap().mass.unwrap();
    close(m.mass, 10.55e-3, 1e-12);
    close(m.center_of_mass.x, 106.75 / 10.55, 1e-9);
    close(m.center_of_mass.y, 5.0, 1e-9);
    let dxa: f64 = 5.0 - 106.75 / 10.55;
    let dxb: f64 = 25.0 - 106.75 / 10.55;
    let own = 10.55e-3 * 200.0 / 12.0;
    close(m.inertia[(1, 1)], own + 7.85e-3 * dxa * dxa + 2.7e-3 * dxb * dxb, 1e-9);
    close(m.inertia[(1, 1)], 0.979_436, 1e-6);
    close(m.inertia[(0, 0)], own, 1e-9);
    assert!(mass_report(&[a, b, c], &doc.props()).unwrap().mass.is_none(), "C has no material");
    let r = mass_report(&[a, b, c], &doc.props()).unwrap();
    close(r.volume, 3000.0, 1e-6);
    let _ = e;
}

/// PS9.1: parts take the palette's colours in the order they are made, cycling after 8, and
/// deleting a part leaves the others' colours alone.
#[test]
fn palette_colours_are_stable() {
    let mut doc = Doc::new();
    let rects: Vec<SketchOp> = (0..10).map(|i| rect(i as f64 * 20.0, 0.0, i as f64 * 20.0 + 10.0, 10.0)).collect();
    let s = doc.sketch(PlaneRef::Top, rects);
    let mut extrudes = Vec::new();
    for i in 0..10 {
        extrudes.push(doc.extrude(s, &[Vec2::new(i as f64 * 20.0 + 5.0, 5.0)], 10.0, BooleanOp::New));
    }
    let parts = doc.parts();
    let props = doc.props();
    for (i, e) in extrudes.iter().enumerate() {
        let p = parts.iter().find(|p| p.feature == *e).unwrap();
        assert_eq!(p.palette, i as u32);
        assert_eq!(part_appearance(p, &props), palette(i as u32));
    }
    assert_eq!(palette(8), PALETTE[0]);
    // Delete Part 2 (the Parts list's Delete: a Delete part feature): Part 3 keeps the third
    // colour.
    let el = doc.el;
    let p2 = parts.iter().find(|p| p.feature == extrudes[1]).unwrap().id;
    doc.h
        .execute(&mut doc.d, &AddFeature::delete_parts(el, FeatureId::new(), vec![p2]))
        .unwrap();
    let parts = doc.parts();
    assert_eq!(parts.len(), 9);
    let p3 = parts.iter().find(|p| p.feature == extrudes[2]).unwrap();
    assert_eq!(part_appearance(p3, &doc.props()), PALETTE[2]);
}
