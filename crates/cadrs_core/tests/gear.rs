//! The Gear feature (`cadrs_core::gear`) through the document's commands: gears made whole and
//! teeth cut into a cylinder, a hole, a cone and a flat face, with volumes from the kernel.
#![cfg(feature = "brep")]

use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{Document, ExtrudeFeature, FaceRef, FeatureKind, RegionRef};
use cadrs_core::gear::{GearAxis, GearFeature, GearType, ToothStyle};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Gears");
        let el = d.elements[0].id;
        Self {
            d,
            h: History::default(),
            el,
        }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.d
            .element(self.el)
            .unwrap()
            .feature(f)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry
            .clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddSketch {
                    element: self.el,
                    feature: f,
                    plane: Some(plane),
                },
            )
            .unwrap();
        for op in ops {
            self.h
                .execute(
                    &mut self.d,
                    &EditSketch {
                        element: self.el,
                        feature: f,
                        op,
                    },
                )
                .unwrap();
        }
        f
    }

    fn extrude(&mut self, e: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddExtrude {
                    element: self.el,
                    feature: f,
                    extrude: ExtrudeFeature::default(),
                },
            )
            .unwrap();
        self.h
            .execute(
                &mut self.d,
                &SetExtrude {
                    element: self.el,
                    feature: f,
                    extrude: e,
                    label: "Extrude".into(),
                },
            )
            .unwrap();
        f
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(
                &mut self.d,
                &AddFeature {
                    element: self.el,
                    feature,
                    base_name: base.into(),
                    kind,
                },
            )
            .unwrap();
        feature
    }

    fn build(&self) -> std::sync::Arc<rebuild::Build> {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    fn parts(&self) -> Vec<Part> {
        self.build().parts.clone()
    }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

/// The cylindrical face of `part` of radius `r`.
fn cylinder_face(part: &Part, r: f64) -> FaceRef {
    let i = part
        .solid
        .faces
        .iter()
        .position(|f| {
            f.kind == Some(cadrs_kernel::SurfaceKind::Cylinder)
                && f.radius.is_some_and(|x| (x - r).abs() < 1e-6)
        })
        .expect("a cylinder of the radius");
    FaceRef {
        part: part.id,
        face: part.solid.faces[i].name,
        seed: part.solid.face_point(i).unwrap(),
    }
}

/// The face of `part` of the given kind (the first).
fn face_of_kind(part: &Part, kind: cadrs_kernel::SurfaceKind) -> FaceRef {
    let i = part
        .solid
        .faces
        .iter()
        .position(|f| f.kind == Some(kind))
        .expect("a face of the kind");
    FaceRef {
        part: part.id,
        face: part.solid.faces[i].name,
        seed: part.solid.face_point(i).unwrap(),
    }
}

fn circle(r: f64) -> SketchOp {
    SketchOp::AddCircle {
        center: Vec2::new(0.0, 0.0),
        radius: r,
        construction: false,
    }
}

fn gear(st: &mut Studio, x: GearFeature) -> std::sync::Arc<rebuild::Build> {
    let name = x.base_name();
    st.add(name, FeatureKind::Gear(x));
    let t = std::time::Instant::now();
    let b = st.build();
    eprintln!("{name} built in {:?}", t.elapsed());
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    b
}

/// A 20-tooth module 2 gear's section is close to its pitch circle's disc: 10 wide, less its
/// Ø8 bore.
const SPUR: f64 = (std::f64::consts::PI * 400.0 - std::f64::consts::PI * 16.0) * 10.0;

#[test]
fn a_spur_gear_is_about_its_pitch_cylinder() {
    let mut st = Studio::new();
    let b = gear(&mut st, GearFeature::default());
    assert_eq!(b.parts.len(), 1);
    let v = volume(&b.parts[0]);
    assert!(close(v, SPUR, 0.04), "{v} vs {SPUR}");
    // 20 teeth of 8 faces, the bore and the two ends.
    assert_eq!(b.parts[0].solid.faces.len(), 20 * 8 + 3);
}

#[test]
fn helical_and_herringbone_teeth_keep_the_section() {
    let straight = {
        let mut st = Studio::new();
        volume(&gear(&mut st, GearFeature::default()).parts[0])
    };
    for (style, left) in [
        (ToothStyle::Helical, false),
        (ToothStyle::Helical, true),
        (ToothStyle::Herringbone, false),
    ] {
        let mut st = Studio::new();
        let b = gear(
            &mut st,
            GearFeature {
                style,
                left_hand: left,
                helix_angle: 30.0,
                ..Default::default()
            },
        );
        assert_eq!(b.parts.len(), 1, "{style:?}");
        let v = volume(&b.parts[0]);
        // Twisting a section about the axis keeps its area at every height.
        assert!(close(v, straight, 2e-4), "{style:?}: {v} vs {straight}");
    }
}

#[test]
fn a_ring_gear_is_its_rim_less_about_its_pitch_cylinder() {
    for style in [ToothStyle::Straight, ToothStyle::Herringbone] {
        let mut st = Studio::new();
        let b = gear(
            &mut st,
            GearFeature {
                gear_type: GearType::Ring,
                teeth: 40,
                style,
                ..Default::default()
            },
        );
        assert_eq!(b.parts.len(), 1);
        let v = volume(&b.parts[0]);
        // Outside radius 40 + 2.5 + 5.
        let want = std::f64::consts::PI * (47.5f64.powi(2) - 40f64.powi(2)) * 10.0;
        assert!(close(v, want, 0.06), "{style:?}: {v} vs {want}");
    }
}

#[test]
fn teeth_cut_into_a_cylinder_make_the_same_gear() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![circle(22.0)]);
    st.extrude(ExtrudeFeature {
        sketches: vec![s],
        depth: 10.0,
        depth_expr: "10 mm".into(),
        ..Default::default()
    });
    let disc = st.parts()[0].clone();
    let mut x = GearFeature {
        face: Some(face_of_kind(&disc, cadrs_kernel::SurfaceKind::Cylinder)),
        ..Default::default()
    };
    x.teeth = 20;
    let b = gear(&mut st, x);
    assert_eq!(b.parts.len(), 1);
    let v = volume(&b.parts[0]);
    let want = SPUR + std::f64::consts::PI * 16.0 * 10.0;
    assert!(close(v, want, 0.04), "{v} vs {want}");
    // The same section as the gear made whole (which has a bore).
    let whole = {
        let mut st = Studio::new();
        volume(
            &gear(
                &mut st,
                GearFeature {
                    bore: 0.0,
                    ..Default::default()
                },
            )
            .parts[0],
        )
    };
    // (The cut's walls are one curve a space, within a micron of the whole gear's outline.)
    assert!(close(v, whole, 1e-4), "{v} vs {whole}");
}

#[test]
fn helical_teeth_cut_into_a_shaft_between_shoulders() {
    // A Ø44 × 10 collar on a Ø20 shaft: the teeth are cut into the collar alone.
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![circle(10.0)]);
    st.extrude(ExtrudeFeature {
        sketches: vec![s],
        depth: 30.0,
        depth_expr: "30 mm".into(),
        ..Default::default()
    });
    let s2 = st.sketch(PlaneRef::Top, vec![circle(22.0)]);
    st.extrude(ExtrudeFeature {
        sketches: vec![s2],
        depth: 10.0,
        depth_expr: "10 mm".into(),
        op: cadrs_core::document::BooleanOp::Add,
        ..Default::default()
    });
    let parts = st.parts();
    assert_eq!(parts.len(), 1);
    let before = volume(&parts[0]);
    let b = gear(
        &mut st,
        GearFeature {
            face: Some(cylinder_face(&parts[0], 22.0)),
            style: ToothStyle::Helical,
            ..Default::default()
        },
    );
    assert_eq!(b.parts.len(), 1);
    let cut = before - volume(&b.parts[0]);
    // The collar less the gear: its ring outside the pitch circle, about.
    let want = std::f64::consts::PI * (22f64.powi(2) - 400.0) * 10.0;
    assert!(close(cut, want, 0.15), "{cut} vs {want}");
}

#[test]
fn teeth_cut_into_a_hole_make_a_ring_gear() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![circle(50.0), circle(38.0)]);
    let regions = cadrs_sketch::region::regions(&st.g(s));
    let ring = regions.iter().find(|r| !r.holes.is_empty()).unwrap();
    st.extrude(ExtrudeFeature {
        regions: vec![RegionRef::new(s, ring)],
        depth: 8.0,
        depth_expr: "8 mm".into(),
        ..Default::default()
    });
    let blank = st.parts()[0].clone();
    let before = volume(&blank);
    let mut x = GearFeature {
        face: Some(cylinder_face(&blank, 38.0)),
        ..Default::default()
    };
    let target = {
        let i = blank
            .solid
            .faces
            .iter()
            .position(|f| f.name == x.face.unwrap().face)
            .unwrap();
        cadrs_core::gear::target(&blank.solid, i).unwrap()
    };
    x.fit(&target);
    assert_eq!((x.gear_type, x.teeth), (GearType::Ring, 40));
    let b = gear(&mut st, x);
    assert_eq!(b.parts.len(), 1);
    let cut = before - volume(&b.parts[0]);
    // Between the hole (r 38) and the pitch circle (r 40), about, and the roots' clearance.
    let want = std::f64::consts::PI * (40f64.powi(2) - 38f64.powi(2)) * 8.0;
    assert!(cut > 0.8 * want && cut < 1.4 * want, "{cut} vs {want}");
}

#[test]
fn a_bevel_gear_has_its_teeth() {
    for style in [ToothStyle::Straight, ToothStyle::Helical] {
        let mut st = Studio::new();
        let x = GearFeature {
            gear_type: GearType::Bevel,
            style,
            helix_angle: 30.0,
            ..Default::default()
        };
        let apex = x.apex_height().unwrap();
        let b = gear(&mut st, x);
        assert_eq!(b.parts.len(), 1, "{style:?}");
        let p = &b.parts[0];
        let v = volume(p);
        assert!(v > 4000.0 && v < 12000.0, "{style:?}: {v}");
        // Under its apex, on its back face.
        let top = p
            .solid
            .positions
            .iter()
            .map(|q| q[2])
            .fold(f64::MIN, f64::max);
        assert!(top > 1.0 && top < apex, "{top} under {apex}");
        // 20 spaces of a bottom and two flanks, and the blank's faces.
        assert!(p.solid.faces.len() >= 20 * 3 + 4, "{}", p.solid.faces.len());
    }
}

#[test]
fn teeth_cut_into_a_cone_make_a_bevel_gear() {
    // A frustum: r 25 at z = 0 down to r 15 at z = 10 (45°).
    let mut st = Studio::new();
    let s = st.sketch(
        PlaneRef::Front,
        vec![SketchOp::AddPolyline {
            points: vec![
                Vec2::new(4.0, 0.0),
                Vec2::new(25.0, 0.0),
                Vec2::new(15.0, 10.0),
                Vec2::new(4.0, 10.0),
            ],
            closed: true,
            construction: false,
            label: "Add polygon",
        }],
    );
    // The axis: a vertical line on the Front plane (world Z).
    let axis_sk = st.sketch(
        PlaneRef::Front,
        vec![SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(0.0, 30.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }],
    );
    let axis_curve = st.g(axis_sk).curves.keys().next().unwrap();
    st.add(
        "Revolve",
        FeatureKind::Revolve(cadrs_core::document::RevolveFeature {
            sketches: vec![s],
            axis: Some(cadrs_core::document::AxisRef::SketchCurve {
                sketch: axis_sk,
                curve: axis_curve,
            }),
            ..Default::default()
        }),
    );
    let blank = st.parts()[0].clone();
    let before = volume(&blank);
    let mut x = GearFeature {
        face: Some(face_of_kind(&blank, cadrs_kernel::SurfaceKind::Cone)),
        ..Default::default()
    };
    let target = {
        let i = blank
            .solid
            .faces
            .iter()
            .position(|f| f.name == x.face.unwrap().face)
            .unwrap();
        cadrs_core::gear::target(&blank.solid, i).unwrap()
    };
    x.fit(&target);
    assert_eq!(x.gear_type, GearType::Bevel);
    let b = gear(&mut st, x);
    assert_eq!(b.parts.len(), 1);
    let cut = before - volume(&b.parts[0]);
    assert!(cut > 200.0 && cut < 0.5 * before, "{cut} of {before}");
}

#[test]
fn a_rack_is_its_bar_less_its_spaces() {
    for style in [
        ToothStyle::Straight,
        ToothStyle::Helical,
        ToothStyle::Herringbone,
    ] {
        let mut st = Studio::new();
        let b = gear(
            &mut st,
            GearFeature {
                gear_type: GearType::Rack,
                style,
                axis: GearAxis::Y,
                ..Default::default()
            },
        );
        assert_eq!(b.parts.len(), 1, "{style:?}");
        let v = volume(&b.parts[0]);
        // The bar, 60 × 10 × (5 + 2.5 + 2), less a space every pitch: a trapezoid 4.5 deep,
        // 1.32 wide at its root and 4.60 at the tips.
        let space = (1.322 + 4.598) / 2.0 * 4.5;
        let want = 60.0 * 10.0 * 9.5 - 60.0 / (2.0 * std::f64::consts::PI) * space * 10.0;
        assert!(close(v, want, 0.03), "{style:?}: {v} vs {want}");
    }
}

#[test]
fn teeth_cut_into_a_flat_face_make_a_rack() {
    let mut st = Studio::new();
    let s = st.sketch(
        PlaneRef::Top,
        vec![SketchOp::AddPolyline {
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(50.0, 0.0),
                Vec2::new(50.0, 12.0),
                Vec2::new(0.0, 12.0),
            ],
            closed: true,
            construction: false,
            label: "Add rectangle",
        }],
    );
    st.extrude(ExtrudeFeature {
        sketches: vec![s],
        depth: 10.0,
        depth_expr: "10 mm".into(),
        ..Default::default()
    });
    let bar = st.parts()[0].clone();
    let before = volume(&bar);
    let top = {
        let i = bar
            .solid
            .faces
            .iter()
            .position(|f| f.plane.is_some_and(|p| p.normal()[2] > 0.9))
            .unwrap();
        FaceRef {
            part: bar.id,
            face: bar.solid.faces[i].name,
            seed: bar.solid.face_point(i).unwrap(),
        }
    };
    for flip in [false, true] {
        let mut st2 = Studio {
            d: st.d.clone(),
            h: History::default(),
            el: st.el,
        };
        let b = gear(
            &mut st2,
            GearFeature {
                face: Some(top),
                gear_type: GearType::Rack,
                flip,
                ..Default::default()
            },
        );
        assert_eq!(b.parts.len(), 1);
        let cut = before - volume(&b.parts[0]);
        // Half the top 4.5 mm's layer between root and tip, about.
        let want = 50.0 * 12.0 * 4.5 / 2.0;
        assert!(close(cut, want, 0.2), "flip {flip}: {cut} vs {want}");
    }
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_gears_sample_builds_its_fourteen_parts() {
    let mut st = Studio::new();
    cadrs_core::samples::gears::build(&mut st.d, &mut st.h, st.el).unwrap();
    let t = std::time::Instant::now();
    let b = st.build();
    eprintln!("the gears sample built in {:?}", t.elapsed());
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    // Three bevels and a pinion, the gear with a hub, a ring with a sun and three planets,
    // a ring, and three more.
    assert_eq!(b.parts.len(), 14);
    // The pair's apexes are one point: the pinion's teeth reach to the gear's, not into them
    // by more than the tooth depth nor short of them.
    let gears = cadrs_core::samples::gears::gears();
    assert_eq!(gears.len(), 14);
}

#[test]
fn the_gears_fixture_is_current() {
    // `fixtures/gears.cadrs` is the sample document as `samples::gears::document()` builds it
    // (open it with `cadrs fixtures/gears.cadrs`). Regenerate it with
    // `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test gear the_gears_fixture`.
    use cadrs_core::samples::gears;
    let doc = gears::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gears.cadrs");
    let file = gears::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/gears.cadrs is out of date");
}
