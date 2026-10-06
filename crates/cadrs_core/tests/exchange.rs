//! P3F.2: import and export (`essential-tips-gaps.md`, `intro-to-parametric-cad-gaps.md`).
//!
//! - STEP export → import of the Control Arm keeps V = 368 749.705 mm³ (1e−3).
//! - A 100 × 60 × 25 box as binary STL, ASCII STL and OBJ: 12 triangles enclosing
//!   150 000 mm³ (the divergence theorem, exact for flat faces).
//! - A two-part STEP assembly (a box used twice, a pin once) imports as 2 parts and 3
//!   instances at the placements it was written with; flattened, the parts are where the
//!   instances were.
//! - IGES keeps the box's area and volume (1e−3).
//! - DXF of a planar face: the box's top is 4 lines, a cylinder's top 1 circle.
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::assembly::Pose;
use cadrs_core::document::{Feature, FeatureKind, SketchFeature};
use cadrs_core::import::{ImportAs, ImportFormat, ImportIds, import_elements, imported_document};
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
use cadrs_core::{Document, ElementId, FeatureId, samples};
use cadrs_kernel::exchange::{mesh_volume, read_obj, read_stl_ascii, read_stl_binary};
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

#[track_caller]
fn close(actual: f64, expected: f64, tol: f64) {
    assert!((actual - expected).abs() <= tol, "got {actual}, expected {expected} ± {tol}");
}

fn sketch(g: Sketch) -> Feature {
    Feature {
        id: FeatureId::new(),
        name: "Sketch 1".into(),
        kind: FeatureKind::Sketch(SketchFeature { plane: Some(PlaneRef::Top), disable_imprinting: false, geometry: g }),
        suppress_by: None,
    }
}

fn extrude(s: &Feature, seeds: &[Vec2], depth: f64) -> Feature {
    let g = &s.sketch().unwrap().geometry;
    Feature {
        id: FeatureId::new(),
        name: "Extrude".into(),
        kind: FeatureKind::Extrude(samples::extrude_of(samples::region_refs(s.id, g, seeds), depth)),
        suppress_by: None,
    }
}

/// The Control Arm of PS6 with both extrudes New (as `kernel_rebuild.rs`).
fn control_arm() -> Vec<Feature> {
    let s = sketch(samples::control_arm_sketch());
    let e1 = extrude(&s, &[v(0.0, 26.0), v(60.0, 0.0), v(107.5 + 14.0, 0.0)], 40.0);
    let e2 = extrude(&s, &[v(-60.0, 0.0), v(-107.5 - 14.0, 0.0)], 25.0);
    vec![s, e1, e2]
}

fn rect_studio(w: f64, h: f64, depth: f64) -> Vec<Feature> {
    let mut g = Sketch::new();
    SketchOp::AddPolyline { points: vec![v(0.0, 0.0), v(w, 0.0), v(w, h), v(0.0, h)], closed: true, construction: false, label: "Add line" }
        .apply(&mut g)
        .unwrap();
    let s = sketch(g);
    let e = extrude(&s, &[v(w / 2.0, h / 2.0)], depth);
    vec![s, e]
}

fn pin_studio() -> Vec<Feature> {
    let mut g = Sketch::new();
    SketchOp::AddCircle { center: v(0.0, 0.0), radius: 10.0, construction: false }.apply(&mut g).unwrap();
    let s = sketch(g);
    let e = extrude(&s, &[v(0.0, 0.0)], 10.0);
    vec![s, e]
}

/// Every part of a studio as an export item where it is.
fn items(features: &Arc<Vec<Feature>>, r: &mut Rebuilder, studio: u128) -> Vec<ExportItem> {
    let build = r.rebuild(features);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    build
        .parts
        .iter()
        .map(|p| ExportItem {
            features: features.clone(),
            part: p.id,
            name: p.name.clone(),
            pose: None,
            source: (ElementId::from_u128(studio), p.id),
            source_name: p.name.clone(),
        })
        .collect()
}

fn volume_of(doc: &Document) -> f64 {
    let build = Rebuilder::new().rebuild(doc.elements[0].features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    build.parts.iter().map(|p| p.mass.unwrap().volume).sum()
}

#[test]
fn control_arm_step_export_then_import_keeps_the_volume() {
    let f = Arc::new(control_arm());
    let mut r = Rebuilder::new();
    let its = items(&f, &mut r, 1);
    assert_eq!(its.len(), 2);
    let before: f64 = r.rebuild(&f).parts.iter().map(|p| p.mass.unwrap().volume).sum();
    close(before, 368_749.705, 1e-3);
    let files = r.export_files(&ExportRequest::new(ModelFormat::Step, "Control Arm", its)).unwrap();
    assert_eq!(files.len(), 1);
    let text = String::from_utf8(files[0].bytes.clone()).unwrap();
    let plan = r.plan_import(ImportFormat::Step, text.as_bytes()).unwrap();
    assert_eq!(plan.parts.len(), 2);
    assert_eq!(plan.parts[0].name, "Part 1");
    let doc = imported_document(&plan, "Control Arm.step", text, ImportAs::PartStudio, ImportIds::fresh());
    close(volume_of(&doc), 368_749.705, 1e-3);
}

#[test]
fn a_box_as_stl_and_obj_is_twelve_triangles_of_150000_cubic_mm() {
    let f = Arc::new(rect_studio(100.0, 60.0, 25.0));
    let mut r = Rebuilder::new();
    let its = items(&f, &mut r, 1);
    for (format, read) in [
        (ModelFormat::Stl { binary: true }, &(|b: &[u8]| read_stl_binary(b).unwrap()) as &dyn Fn(&[u8]) -> Vec<_>),
        (ModelFormat::Stl { binary: false }, &|b: &[u8]| read_stl_ascii(std::str::from_utf8(b).unwrap())),
        (ModelFormat::Obj, &|b: &[u8]| read_obj(std::str::from_utf8(b).unwrap())),
    ] {
        let files = r.export_files(&ExportRequest::new(format, "Box", its.clone())).unwrap();
        let tris = read(&files[0].bytes);
        assert_eq!(tris.len(), 12, "{format:?}");
        close(mesh_volume(tris), 150_000.0, 1e-6);
    }
    // Inches: every length / 25.4, the volume / 25.4³.
    let mut req = ExportRequest::new(ModelFormat::Stl { binary: false }, "Box", its);
    req.scale = 1.0 / 25.4;
    let files = r.export_files(&req).unwrap();
    close(mesh_volume(read_stl_ascii(std::str::from_utf8(&files[0].bytes).unwrap())), 150_000.0 / 25.4f64.powi(3), 1e-6);
}

#[test]
fn a_two_part_step_assembly_imports_as_instances_at_their_placements() {
    let bx = Arc::new(rect_studio(100.0, 60.0, 25.0));
    let pin = Arc::new(pin_studio());
    let mut r = Rebuilder::new();
    let b = items(&bx, &mut r, 1).remove(0);
    let p = items(&pin, &mut r, 2).remove(0);
    let turned = Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2).then(&Pose::translation([200.0, 10.0, 5.0]));
    let lift = Pose::translation([50.0, 30.0, 25.0]);
    let poses = [Pose::IDENTITY, turned, lift];
    let mut its = vec![b.clone(), b, p];
    for (k, (it, pose)) in its.iter_mut().zip(poses).enumerate() {
        it.pose = Some(pose);
        it.name = format!("{} <{}>", it.source_name, k + 1);
    }
    its[0].source_name = "Box".into();
    its[2].source_name = "Pin".into();
    let mut req = ExportRequest::new(ModelFormat::Step, "Two parts", its);
    req.assembly = true;
    let files = r.export_files(&req).unwrap();
    let text = String::from_utf8(files[0].bytes.clone()).unwrap();
    let plan = r.plan_import(ImportFormat::Step, text.as_bytes()).unwrap();
    assert!(plan.is_assembly());
    assert_eq!(plan.name, "Two parts");
    assert_eq!(plan.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Box", "Pin"]);
    assert_eq!(plan.occurrences.len(), 3);
    // Keep assembly structure: 2 parts in a Part Studio, 3 instances where they were.
    let doc = Document::empty("Import");
    let els = import_elements(&doc, &plan, "Two parts.step", text.clone(), ImportAs::Assembly, ImportIds::fresh());
    let asm = els[1].assembly_model().unwrap();
    assert_eq!(asm.instances.len(), 3);
    // P3F.2 judge: each part sits in the Part Studio where its first occurrence is (the pin at
    // `lift`, not at the origin under the box), and the instances are placed relative to that:
    // the box's first at the identity, its second turned, the pin's at the identity.
    let poses = [Pose::IDENTITY, turned, Pose::IDENTITY];
    for (inst, want) in asm.instances.iter().zip(poses) {
        for i in 0..3 {
            close(inst.pose.translation[i], want.translation[i], 1e-9);
            for j in 0..3 {
                close(inst.pose.rotation[i][j], want.rotation[i][j], 1e-9);
            }
        }
    }
    assert_eq!(asm.instances[0].source, asm.instances[1].source, "the box twice");
    let build = Rebuilder::new().rebuild(els[0].features());
    assert_eq!(build.parts.len(), 2);
    close(build.parts[0].mass.unwrap().volume, 150_000.0, 1e-6);
    close(build.parts[1].mass.unwrap().volume, std::f64::consts::PI * 100.0 * 10.0, 1e-6);
    let pin_c = build.parts[1].mass.unwrap().center_of_mass;
    close(pin_c.x, 50.0, 1e-6);
    close(pin_c.y, 30.0, 1e-6);
    close(pin_c.z, 30.0, 1e-6);
    // File is Y axis up: the same file turned +90° about X, parts and instances together.
    let mut up = import_elements(&doc, &plan, "Two parts.step", text.clone(), ImportAs::Assembly, ImportIds::fresh());
    cadrs_core::import::file_is_y_up(&mut up);
    let build = Rebuilder::new().rebuild(up[0].features());
    let c = build.parts[1].mass.unwrap().center_of_mass;
    // (x, y, z) ↦ (x, −z, y): the pin's centre (50, 30, 30) goes to (50, −30, 30).
    close(c.x, 50.0, 1e-6);
    close(c.y, -30.0, 1e-6);
    close(c.z, 30.0, 1e-6);
    let turned_up = up[1].assembly_model().unwrap().instances[1].pose;
    let want = cadrs_core::import::y_up_turn().inverse().then(&turned).then(&cadrs_core::import::y_up_turn());
    for i in 0..3 {
        close(turned_up.translation[i], want.translation[i], 1e-9);
    }
    // Flattened: 3 parts where the instances were (the turned box's centre is at
    // (200 − 30, 10 + 50, 5 + 12.5)).
    let flat = imported_document(&plan, "Two parts.step", text, ImportAs::PartStudio, ImportIds::fresh());
    let build = Rebuilder::new().rebuild(flat.elements[0].features());
    assert_eq!(build.parts.len(), 3);
    let c = build.parts[1].mass.unwrap().center_of_mass;
    close(c.x, 170.0, 1e-6);
    close(c.y, 60.0, 1e-6);
    close(c.z, 17.5, 1e-6);
    let names: Vec<String> = flat.elements[0].part_props().iter().filter_map(|p| p.name.clone()).collect();
    assert_eq!(names, ["Box <1>", "Box <2>", "Pin"]);
}

#[test]
fn iges_keeps_the_box() {
    let f = Arc::new(rect_studio(100.0, 60.0, 25.0));
    let mut r = Rebuilder::new();
    let its = items(&f, &mut r, 1);
    let files = r.export_files(&ExportRequest::new(ModelFormat::Iges, "Box", its)).unwrap();
    let text = String::from_utf8(files[0].bytes.clone()).unwrap();
    let plan = r.plan_import(ImportFormat::Iges, text.as_bytes()).unwrap();
    let doc = imported_document(&plan, "Box.igs", text, ImportAs::PartStudio, ImportIds::fresh());
    let build = Rebuilder::new().rebuild(doc.elements[0].features());
    let area: f64 = build.parts.iter().map(|p| p.mass.unwrap().surface_area).sum();
    close(area, 20_000.0, 1e-3);
    close(volume_of(&doc), 150_000.0, 1e-3);
}

#[test]
fn planar_faces_as_dxf() {
    let mut r = Rebuilder::new();
    // The box's top: 4 lines, no circle; the pin's: 1 circle.
    for (features, want) in [(rect_studio(100.0, 60.0, 25.0), (4, 0, 0, 0)), (pin_studio(), (0, 0, 1, 0))] {
        let build = r.rebuild(&features);
        let solid = &build.parts[0].solid;
        let top = solid
            .faces
            .iter()
            .find(|f| f.plane.is_some_and(|p| p.normal()[2] > 0.99 && p.origin[2] > 1.0))
            .expect("a top face");
        let page = cadrs_core::dxf_export::face_page(solid, &top.name, "Top").unwrap();
        assert_eq!(page.counts(), want);
        let text = cadrs_core::dxf_export::write_dxf(&page, cadrs_drawing::dxf::DxfVersion::R2013);
        let back = cadrs_drawing::dxf::read_dxf(&text).unwrap();
        assert_eq!(cadrs_drawing::dxf::counts(&back), (want.0, want.1, want.2));
        // In millimetres, the face's own outline: the box top 100 × 60 (its extents), the pin
        // Ø20 round its axis.
        assert!(text.contains("$INSUNITS\n 70\n4\n"));
        assert_eq!(back.unit_mm, 1.0);
        let (lo, hi) = cadrs_drawing::dxf::extents(&page);
        for e in &back.entities {
            match e {
                cadrs_drawing::sheet_sketch::Entity::Circle { center, radius } => {
                    close(*radius, 10.0, 1e-9);
                    close(center[0], 0.0, 1e-9);
                    close(center[1], 0.0, 1e-9);
                }
                cadrs_drawing::sheet_sketch::Entity::Line { a, b } => {
                    // Each side is axis-aligned and on the outline.
                    assert!((a[0] - b[0]).abs() < 1e-9 || (a[1] - b[1]).abs() < 1e-9, "{a:?} {b:?}");
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        if want.0 == 4 {
            close(hi[0] - lo[0], 100.0, 1e-9);
            close(hi[1] - lo[1], 60.0, 1e-9);
        } else {
            assert_eq!((lo, hi), ([-10.0, -10.0], [10.0, 10.0]));
        }
    }
}

/// P3F.2 judge: Y axis up turns the model's Z into Y: the 100 × 60 × 25 box's Y extent is 25.
#[test]
fn y_up_turns_the_box_upright() {
    let f = Arc::new(rect_studio(100.0, 60.0, 25.0));
    let mut r = Rebuilder::new();
    let its = items(&f, &mut r, 1);
    let mut extent = |y_up: bool| {
        let mut req = ExportRequest::new(ModelFormat::Stl { binary: false }, "Box", its.clone());
        req.y_up = y_up;
        let files = r.export_files(&req).unwrap();
        let tris = read_stl_ascii(std::str::from_utf8(&files[0].bytes).unwrap());
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for t in &tris {
            for p in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    };
    let z_up = extent(false);
    let y_up = extent(true);
    for (a, b) in z_up.iter().zip([100.0, 60.0, 25.0]) {
        close(*a, b, 1e-6);
    }
    for (a, b) in y_up.iter().zip([100.0, 25.0, 60.0]) {
        close(*a, b, 1e-6);
    }
}

/// P3F.2 judge: individual files: the Control Arm's two parts as two files each named for its
/// part, in STEP and STL; one file otherwise.
#[test]
fn individual_files_for_the_control_arm() {
    let f = Arc::new(control_arm());
    let mut r = Rebuilder::new();
    let its = items(&f, &mut r, 1);
    for format in [ModelFormat::Step, ModelFormat::Stl { binary: true }] {
        let mut req = ExportRequest::new(format, "Control Arm", its.clone());
        assert_eq!(r.export_files(&req).unwrap().len(), 1, "{format:?}");
        req.individual = true;
        let files = r.export_files(&req).unwrap();
        assert_eq!(files.len(), 2, "{format:?}");
        let names: Vec<_> = files.iter().map(|x| x.part.clone().unwrap_or_default()).collect();
        assert_eq!(names, vec!["Part 1".to_string(), "Part 2".to_string()], "{format:?}");
    }
}

/// The import scenarios' file: 2 parts, 3 instances, the second bracket turned half round.
#[test]
fn the_bracket_pair_fixture() {
    let mut r = Rebuilder::new();
    let bytes = samples::bracket_pair::step(&mut r).unwrap();
    let plan = r.plan_import(ImportFormat::Step, &bytes).unwrap();
    assert_eq!(plan.name, "Bracket pair");
    assert_eq!(plan.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Bracket", "Shaft"]);
    assert_eq!(plan.occurrences.len(), 3);
    let want = samples::bracket_pair::placements();
    for (o, w) in plan.occurrences.iter().zip(want) {
        for i in 0..3 {
            close(o.pose.translation[i], w.translation[i], 1e-9);
        }
    }
}
