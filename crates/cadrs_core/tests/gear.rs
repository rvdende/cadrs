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
    use cadrs_core::gear::SpiralKind;
    for (style, spiral) in [
        (ToothStyle::Straight, SpiralKind::Equiangular),
        (ToothStyle::Helical, SpiralKind::Equiangular),
        (ToothStyle::Helical, SpiralKind::FaceMilled),
    ] {
        let mut st = Studio::new();
        let x = GearFeature {
            gear_type: GearType::Bevel,
            style,
            spiral,
            helix_angle: 35.0,
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
fn the_gears_sample_builds_its_sixteen_parts() {
    let mut st = Studio::new();
    cadrs_core::samples::gears::build(&mut st.d, &mut st.h, st.el).unwrap();
    let t = std::time::Instant::now();
    let b = st.build();
    eprintln!("the gears sample built in {:?}", t.elapsed());
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    // Three bevels and a pinion, the gear with a hub, a ring with a sun and three planets,
    // a ring, three more, and the bevel pair at the shaft angle `#shaft`.
    assert_eq!(b.parts.len(), 16);
    let gears = cadrs_core::samples::gears::gears();
    assert_eq!(gears.len(), 16);
}

/// The sample's bevel pair whose shaft angle is a variable: changing `#shaft` changes the pitch
/// angles, the pinion's tilt and the shaft angle its localised contact is cut for, as the
/// sample's own numbers for that shaft angle have them.
#[test]
fn the_shaft_angle_variable_drives_the_bevel_pair() {
    use cadrs_core::samples::gears::{SHAFT_PAIR, shaft_pair, shaft_variables};
    let mut st = Studio::new();
    cadrs_core::samples::gears::build(&mut st.d, &mut st.h, st.el).unwrap();
    let shaft = shaft_variables(0.0)[0].0;
    let kind = |st: &Studio, id| st.features().into_iter().find(|f| f.id == id).unwrap().kind;
    for angle in [30.0, 45.0, 60.0, 90.0, 120.0] {
        let FeatureKind::Variable(mut v) = kind(&st, shaft) else {
            panic!("#shaft is a variable");
        };
        v.expr = format!("{angle} deg");
        st.h.execute(
            &mut st.d,
            &cadrs_core::commands::SetFeature {
                element: st.el,
                feature: shaft,
                kind: FeatureKind::Variable(v),
                label: "Variable".into(),
            },
        )
        .unwrap();
        let want = shaft_pair(angle);
        for (id, want) in SHAFT_PAIR.into_iter().zip(want) {
            let FeatureKind::Gear(got) = kind(&st, id) else {
                panic!("a gear");
            };
            for (a, b) in [
                (got.pitch_angle, want.pitch_angle),
                (got.tilt, want.tilt),
                (got.shaft_angle, want.shaft_angle),
                (got.rotation, want.rotation),
            ] {
                assert!((a - b).abs() < 1e-9, "{angle}°: {a} vs {b}");
            }
            assert_eq!(got.teeth, want.teeth);
        }
    }
}

/// The shaft-angle pair in mesh from 30° to 115°, flank on flank as the bevel pair at right
/// angles is (`the_bevel_pair_meshes_without_cutting_into_itself`).
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_bevel_pair_meshes_at_any_shaft_angle() {
    use cadrs_core::samples::gears::{SHAFT_PAIR, shaft_pair};
    let mut worst = Vec::new();
    // (At 120° the gear's pitch angle is 74°, its face cone nearly flat, and the boolean that
    // cuts its spaces fails.)
    for shaft in [30.0, 45.0, 60.0, 75.0, 90.0, 105.0, 115.0] {
        // (Their numbers alone: the variables aren't in this studio.)
        let [g, p] = shaft_pair(shaft).map(|mut g| {
            g.teeth_expr = g.teeth.to_string();
            g.pitch_angle_expr = format!("{} deg", g.pitch_angle);
            g.rotation_expr = format!("{} deg", g.rotation);
            g.shaft_angle_expr = format!("{} deg", g.shaft_angle);
            g.tilt_expr = format!("{} deg", g.tilt);
            g
        });
        let (deep, count) = reach([(SHAFT_PAIR[0], g), (SHAFT_PAIR[1], p)]);
        eprintln!("{shaft}°: {count} vertices inside the other gear, the deepest {deep} mm");
        worst.push((shaft, deep));
    }
    assert!(worst.iter().all(|w| w.1 < 0.1), "{worst:?}");
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

#[path = "support_reach.rs"]
mod support_reach;
use support_reach::reach_of;

/// How deep the gears' meshes reach into each other, the two set in mesh as the sample sets
/// them: the deepest of one's vertices inside the other, and how many are.
fn reach(pair: [(cadrs_core::FeatureId, GearFeature); 2]) -> (f64, usize) {
    let mut st = Studio::new();
    for (_, g) in pair {
        st.add("Gear", FeatureKind::Gear(g));
    }
    reach_of(&st.parts())
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_bevel_pair_meshes_without_cutting_into_itself() {
    use cadrs_core::gear::SpiralKind;
    for spiral in [SpiralKind::FaceMilled, SpiralKind::Equiangular] {
        // As the sample sets them, flank on flank: the display meshes, a few hundredths of a
        // millimetre off their surfaces, may reach that far into each other, no further.
        let pair = |backlash: f64| {
            use cadrs_core::samples::gears::{PAIR, gears};
            let of = |id| gears().into_iter().find(|g| g.0 == id).unwrap();
            let (mut big, mut pinion) = (of(PAIR[0]), of(PAIR[1]));
            for g in [&mut big.1, &mut pinion.1] {
                g.spiral = spiral;
                g.backlash = backlash;
                g.backlash_expr = format!("{backlash} mm");
            }
            [big, pinion]
        };
        let (deep, count) = reach(pair(0.0));
        eprintln!("{spiral:?}: {count} vertices inside the other gear, the deepest {deep} mm");
        assert!(deep < 0.1, "{spiral:?}: {deep} mm into the other gear");
        // With a tenth of a millimetre of backlash each they are clear of each other.
        let (deep, count) = reach(pair(0.1));
        eprintln!("{spiral:?}, with backlash: {count} inside, the deepest {deep} mm");
        assert!(
            deep < 0.02,
            "{spiral:?}: {deep} mm into the other gear with backlash"
        );
    }
}

/// The gears sample's pinion alone, cut with localised contact (its own machines) and as the
/// conjugate of its gear: one solid each, nearly the same (the localised one's teeth are
/// eased off toward their ends by a tenth of a millimetre or two).
#[test]
fn a_face_milled_pinion_builds_with_localised_or_conjugate_contact() {
    use cadrs_core::gear::BevelMember;
    let sample = cadrs_core::samples::gears::gears()
        .into_iter()
        .map(|(_, g)| g)
        .find(|g| g.member == BevelMember::Pinion)
        .unwrap();
    let volumes: Vec<f64> = [BevelMember::Pinion, BevelMember::ConjugatePinion]
        .into_iter()
        .map(|member| {
            let mut st = Studio::new();
            let b = gear(
                &mut st,
                GearFeature {
                    member,
                    ..sample.clone()
                },
            );
            assert_eq!(b.parts.len(), 1, "{member:?}");
            volume(&b.parts[0])
        })
        .collect();
    assert!(volumes[0] < volumes[1], "{volumes:?}");
    assert!(close(volumes[0], volumes[1], 0.01), "{volumes:?}");
}

/// `hypoid.sv`'s pair, each member generated by the face mill's crown: one solid each, every
/// tooth space exact (B-spline flanks), as much removed from each as the crown's tooth or space
/// sweeps (the kernel's own generation of one space, a tooth at a time).
#[test]
fn a_hypoid_pairs_members_are_generated_by_their_crown() {
    use cadrs_core::samples::gears::hypoid_pair;
    for ((g, teeth), want) in hypoid_pair()
        .into_iter()
        .zip([48usize, 24])
        .zip([22270.4550, 14669.5768])
    {
        let mut st = Studio::new();
        let t = std::time::Instant::now();
        let b = gear(&mut st, g.clone());
        eprintln!("{:?} built in {:?}", g.member, t.elapsed());
        assert_eq!(b.parts.len(), 1);
        let p = &b.parts[0];
        let faces = p.solid.faces.len();
        eprintln!("volume {}, {faces} faces", volume(p));
        // A dozen or so faces a space (its two flanks, its root fillets, its root, the end
        // reliefs between; the gear's in two stretches, its fillets meeting toward its toe):
        // exact, not the meshed cut's thousands.
        assert!(faces <= 16 * teeth, "{:?}: {faces} faces", g.member);
        assert!(
            (volume(p) - want).abs() < 1e-2,
            "{:?}: {}",
            g.member,
            volume(p)
        );
    }
}

/// `hypoid.sv`'s pair where it stands, in mesh: with its 0.05 mm of backlash neither member
/// reaches into the other (by their display meshes, a few hundredths of a millimetre off their
/// surfaces), and without it they touch, flank on flank.
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_hypoid_pair_meshes_without_cutting_into_itself() {
    use cadrs_core::samples::gears::hypoid_pair;
    for backlash in [0.05, 0.0] {
        let [g, p] = hypoid_pair().map(|mut g| {
            g.backlash = backlash;
            g.backlash_expr = format!("{backlash} mm");
            g
        });
        let (deep, count) = reach([(FeatureId::new(), g), (FeatureId::new(), p)]);
        eprintln!(
            "backlash {backlash}: {count} vertices inside the other member, the deepest {deep} mm"
        );
    }
}

/// `hypoid.sv`'s pair placed by its shafts alone: the gear's axis a line down the Front plane,
/// the pinion's a line along the Top plane 25 mm to the side, square to it. The shaft angle and
/// the offset are the lines' (the typed ones are ignored), the members are the pair placed in
/// its own frame turned and moved onto them (the same volumes), and they mesh where they stand:
/// clear of each other with their backlash, touching without it. Then the gear's shaft tilted
/// 10° and the pinion's 15 mm away on the other side (100°, the pair mirrored): both follow.
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn a_hypoid_pair_is_placed_by_its_shafts() {
    use cadrs_core::document::AxisRef;
    use cadrs_core::samples::gears::hypoid_pair;
    let line = |st: &mut Studio, plane, a: Vec2, b: Vec2| {
        let sk = st.sketch(
            plane,
            vec![SketchOp::AddPolyline {
                points: vec![a, b],
                closed: false,
                construction: false,
                label: "Add line",
            }],
        );
        let curve = st.g(sk).curves.keys().next().unwrap();
        AxisRef::SketchCurve { sketch: sk, curve }
    };
    let tilt = 10f64.to_radians();
    // (The gear's shaft's far end on the Front plane, the pinion's line's side on the Top.)
    for (gear_end, side) in [
        (Vec2::new(0.0, -30.0), 25.0),
        (Vec2::new(30.0 * tilt.sin(), -30.0 * tilt.cos()), -15.0),
    ] {
        for backlash in [0.05, 0.0] {
            let mut st = Studio::new();
            // The gear's apex toward +z, its back down; the pinion's back toward −x.
            let gear_shaft = line(&mut st, PlaneRef::Front, Vec2::new(0.0, 0.0), gear_end);
            let pinion_shaft = line(
                &mut st,
                PlaneRef::Top,
                Vec2::new(0.0, side),
                Vec2::new(-30.0, side),
            );
            let [mut g, mut p] = hypoid_pair();
            for x in [&mut g, &mut p] {
                // (Typed numbers the shafts override.)
                x.offset = 3.0;
                x.shaft_angle = 70.0;
                x.backlash = backlash;
                x.backlash_expr = format!("{backlash} mm");
            }
            g.shaft = Some(gear_shaft);
            g.mate_shaft = Some(pinion_shaft);
            p.shaft = Some(pinion_shaft);
            p.mate_shaft = Some(gear_shaft);
            st.add("Gear", FeatureKind::Gear(g));
            st.add("Gear", FeatureKind::Gear(p));
            let parts = st.parts();
            assert_eq!(parts.len(), 2);
            let volumes: Vec<f64> = parts.iter().map(volume).collect();
            let (deep, count) = reach_of(&parts);
            eprintln!(
                "offset {side}, backlash {backlash}: volumes {volumes:?}; {count} vertices inside the other member, the deepest {deep} mm"
            );
            // (To the fit of the generated spaces' sheets, which depends a little on where they
            // stand.)
            if side == 25.0 && backlash > 0.0 {
                assert!((volumes[0] - 22270.4550).abs() < 5e-2, "{volumes:?}");
                assert!((volumes[1] - 14669.5768).abs() < 5e-2, "{volumes:?}");
            }
            assert!(deep < 0.02, "{deep} mm into the other member");
            if backlash == 0.0 {
                assert!(count > 0, "the members don't touch: not in mesh");
            }
        }
    }
}

/// A worm drive: its worm (a lathe-cut thread on a core) and its wheel hobbed by it, exact (a
/// few faces a tooth space, not the meshed cut's thousands), in mesh where they stand: clear of
/// each other with their backlash, touching without it. Single-start and two-start.
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn a_worm_drive_meshes_where_it_stands() {
    use cadrs_core::samples::gears::worm_drive;
    for starts in [1u32, 2] {
        for backlash in [0.05, 0.0] {
            let [mut wheel, mut worm] = worm_drive([0.0; 3]);
            for x in [&mut wheel, &mut worm] {
                x.backlash = backlash;
                x.backlash_expr = format!("{backlash} mm");
            }
            wheel.mate_teeth = starts;
            worm.teeth = starts;
            let mut st = Studio::new();
            let t = std::time::Instant::now();
            st.add("Gear", FeatureKind::Gear(wheel));
            st.add("Gear", FeatureKind::Gear(worm));
            let parts = st.parts();
            eprintln!(
                "{starts} start(s), backlash {backlash}: built in {:?}",
                t.elapsed()
            );
            assert_eq!(parts.len(), 2);
            let faces: Vec<usize> = parts.iter().map(|p| p.solid.faces.len()).collect();
            let volumes: Vec<f64> = parts.iter().map(volume).collect();
            let (deep, count) = reach_of(&parts);
            eprintln!(
                "  faces {faces:?}, volumes {volumes:?}; {count} vertices inside the other, the deepest {deep} mm"
            );
            assert!(faces[0] <= 12 * 30, "the wheel's faces: {faces:?}");
            assert!(deep < 0.02, "{deep} mm into the other member");
            if backlash == 0.0 {
                assert!(count > 0, "the worm and its wheel don't touch: not in mesh");
            }
        }
    }
}

/// The gears fixture's Hypoid tab: two shafts, the pinion's moved by two Transforms, and the
/// pair picking the shafts' cylinders. As it stands the two gears mesh; rotating the pinion's
/// shaft 10° about the pivot and moving it 5 mm (a shaft angle of 100°, an offset of 20 mm)
/// regenerates both, still in mesh.
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_hypoid_tab_follows_its_shafts_transforms() {
    use cadrs_core::samples::gears::{HYPOID_MOVES, HYPOID_PAIR, document};
    let doc = document().unwrap();
    let el = doc.elements.iter().find(|e| e.name == "Hypoid").unwrap().id;
    let mut st = Studio {
        d: doc,
        h: History::default(),
        el,
    };
    let gears = |st: &Studio| -> Vec<Part> {
        let b = st.build();
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        assert_eq!(b.parts.len(), 4, "two shafts and two gears");
        HYPOID_PAIR
            .iter()
            .map(|f| b.parts.iter().find(|p| p.id.feature == *f).unwrap().clone())
            .collect()
    };
    let (deep, count) = reach_of(&gears(&st));
    eprintln!("as it stands: {count} vertices inside the other gear, the deepest {deep} mm");
    assert!(deep < 0.02);
    let kind = |st: &Studio, f| st.features().into_iter().find(|x| x.id == f).unwrap().kind;
    let set = |st: &mut Studio, f, k| {
        st.h.execute(
            &mut st.d,
            &cadrs_core::commands::SetFeature {
                element: st.el,
                feature: f,
                kind: k,
                label: "Transform".into(),
            },
        )
        .unwrap();
    };
    let FeatureKind::Transform(mut rotate) = kind(&st, HYPOID_MOVES[0]) else {
        panic!("a transform");
    };
    rotate.angle = 10.0;
    rotate.angle_expr = "10 deg".into();
    set(&mut st, HYPOID_MOVES[0], FeatureKind::Transform(rotate));
    let FeatureKind::Transform(mut slide) = kind(&st, HYPOID_MOVES[1]) else {
        panic!("a transform");
    };
    slide.dy = -5.0;
    slide.dy_expr = "-5 mm".into();
    set(&mut st, HYPOID_MOVES[1], FeatureKind::Transform(slide));
    let moved = gears(&st);
    let (deep, count) = reach_of(&moved);
    eprintln!(
        "moved: volumes {:?}; {count} vertices inside the other gear, the deepest {deep} mm",
        moved.iter().map(volume).collect::<Vec<_>>()
    );
    assert!(deep < 0.02);
}

/// The Hypoid tab never fails: its pinion's shaft turned 15° (where the rounded crown's cut
/// can't be followed at the heel), the pinion is cut by the sharp crown instead, with a
/// warning, not an error. And with Fast preview on both gears, the tab builds at once (their
/// blanks alone).
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_hypoid_tab_falls_back_and_previews_fast() {
    use cadrs_core::samples::gears::{HYPOID_MOVES, HYPOID_PAIR, document};
    let doc = document().unwrap();
    let el = doc.elements.iter().find(|e| e.name == "Hypoid").unwrap().id;
    let mut st = Studio {
        d: doc,
        h: History::default(),
        el,
    };
    let kind = |st: &Studio, f| st.features().into_iter().find(|x| x.id == f).unwrap().kind;
    let set = |st: &mut Studio, f, k| {
        st.h.execute(
            &mut st.d,
            &cadrs_core::commands::SetFeature {
                element: st.el,
                feature: f,
                kind: k,
                label: "Edit".into(),
            },
        )
        .unwrap();
    };
    let FeatureKind::Transform(mut rotate) = kind(&st, HYPOID_MOVES[0]) else {
        panic!("a transform");
    };
    rotate.angle = 15.0;
    rotate.angle_expr = "15 deg".into();
    set(&mut st, HYPOID_MOVES[0], FeatureKind::Transform(rotate));
    let b = rebuild::build(&st.features());
    eprintln!("15°: warnings {:?}", b.warnings);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 4);
    for f in HYPOID_PAIR {
        let FeatureKind::Gear(mut g) = kind(&st, f) else {
            panic!("a gear");
        };
        g.fast_preview = true;
        set(&mut st, f, FeatureKind::Gear(g));
    }
    let t = std::time::Instant::now();
    let b = rebuild::build(&st.features());
    let took = t.elapsed();
    eprintln!("fast preview: built in {took:?}");
    assert!(
        b.errors.is_empty() && b.warnings.is_empty(),
        "{:?} {:?}",
        b.errors,
        b.warnings
    );
    assert_eq!(b.parts.len(), 4);
    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
}
