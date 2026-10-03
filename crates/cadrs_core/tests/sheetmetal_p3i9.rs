//! P3I.9 through the document's commands: the Sheet metal loft (its flat against its folded
//! volume), the Form (placement on sketch points, the flat's form outlines, the rule that forms
//! keep clear of edges and joints) and the forms library.
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddFeature, AddSketch, EditSketch, SetFeature};
use cadrs_core::document::{Document, FaceRef, FeatureKind, RegionRef};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_form::{FormFeature, FormLocation, FormPick, FormSource, LIBRARY_NAME, LibraryForm, TagFormFeature};
use cadrs_core::sheetmetal_loft::{LoftConnection, LoftItem, RegionRefKey, SheetMetalLoftFeature};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::{JointKind, Params};
use cadrs_sketch::{FeaturePlane, PlaneFrame, PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Sheet metal P3I.9");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn add(&mut self, name: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: name.into(), kind }).unwrap();
        feature
    }

    fn set(&mut self, feature: FeatureId, kind: FeatureKind) {
        self.h.execute(&mut self.d, &SetFeature { element: self.el, feature, kind, label: "Edit".into() }).unwrap();
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    #[track_caller]
    fn ok(&self) -> std::sync::Arc<Build> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    fn region(&self, sketch: FeatureId, seed: Vec2) -> RegionRef {
        let g = self.d.element(self.el).unwrap().feature(sketch).unwrap().sketch().unwrap().geometry.clone();
        cadrs_core::samples::region_refs(sketch, &g, &[seed]).pop().expect("a region")
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn params() -> Params {
    Params { thickness: 1.5, bend_radius: 2.0, ..SheetMetalModelFeature::default_params() }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn face_near(part: &Part, p: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let d = |c: [f64; 3]| (0..3).map(|i| (c[i] - p[i]).powi(2)).sum::<f64>();
    let i = (0..s.faces.len()).min_by(|a, b| d(s.faces[*a].center.unwrap()).total_cmp(&d(s.faces[*b].center.unwrap()))).unwrap();
    FaceRef { part: part.id, face: s.faces[i].name, seed: s.faces[i].center.unwrap() }
}

/// A 100 × 80 rectangle on Top and a Ø60 circle 60 above it.
fn rect_and_circle(st: &mut Studio) -> (RegionRef, RegionRef) {
    let a = st.sketch(PlaneRef::Top, vec![rect(-50.0, -40.0, 50.0, 40.0)]);
    let frame = PlaneFrame { origin: [0.0, 0.0, 60.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
    let plane = cadrs_core::plane::PlaneFeature {
        entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Top)],
        offset: 60.0,
        offset_expr: "60 mm".into(),
        ..Default::default()
    };
    let pf = st.add("Plane", FeatureKind::Plane(plane));
    let b = st.sketch(PlaneRef::Feature(FeaturePlane::new(pf.0, frame)), vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 30.0, construction: false }]);
    (st.region(a, Vec2::new(0.0, 0.0)), st.region(b, Vec2::new(0.0, 0.0)))
}

#[test]
fn a_rectangle_to_circle_loft_flattens_and_its_flat_matches_its_volume() {
    let mut st = Studio::new();
    let (r1, r2) = rect_and_circle(&mut st);
    let p = params();
    let x = SheetMetalLoftFeature {
        profile1: vec![LoftItem::Region(RegionRefKey::of(&r1))],
        profile2: vec![LoftItem::Region(RegionRefKey::of(&r2))],
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    let f = st.add("Sheet metal loft", FeatureKind::SheetMetalLoft(x.clone()));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).expect("its context");
    assert!(ctx.flat.is_ok(), "{:?}", ctx.flat.errors);
    assert_eq!(ctx.flat.parts.len(), 1, "one rip opens the closed loft");
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Rip { .. })).count(), 1);
    let part = b.parts.iter().find(|q| q.id.feature == f).expect("the loft's part");
    let flat_volume = ctx.flat.parts[0].area() * p.thickness;
    let v = volume(part);
    assert!((v - flat_volume).abs() / v < 0.02, "folded {v} vs flat × T {flat_volume}");
    // Fewer pieces with a coarser chordal tolerance.
    let walls = ctx.model.walls.len();
    st.set(f, FeatureKind::SheetMetalLoft(SheetMetalLoftFeature { chordal_tolerance: 4.0, ..x.clone() }));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).unwrap();
    assert!(ctx.model.walls.len() < walls);
    // The matched start (the rebuild's guides: a header, the matched connections, then each
    // profile's points) as parameters along the profiles.
    let g = &b.arrows[&f];
    let (nc, n1) = (g[0].0[0] as usize, g[0].0[1] as usize);
    let (a, bb) = (g[1].0, g[1].1);
    let p1: Vec<[f64; 3]> = g[1 + nc..1 + nc + n1].iter().map(|x| x.0).collect();
    let p2: Vec<[f64; 3]> = g[1 + nc + n1..].iter().map(|x| x.0).collect();
    let t_of = |pts: &[[f64; 3]], q: [f64; 3]| {
        let d = |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let n = pts.len();
        let total: f64 = (0..n).map(|i| d(pts[i], pts[(i + 1) % n])).sum();
        let k = (0..n).min_by(|x, y| d(pts[*x], q).total_cmp(&d(pts[*y], q))).unwrap();
        (0..k).map(|i| d(pts[i], pts[i + 1])).sum::<f64>() / total
    };
    let (s1, s2) = (t_of(&p1, a), t_of(&p2, bb));
    // Two ripped connections split it in two parts (one rip only opens the closed loft).
    st.set(
        f,
        FeatureKind::SheetMetalLoft(SheetMetalLoftFeature {
            connections_on: true,
            connections: vec![LoftConnection { t1: s1, t2: s2, rip: true }, LoftConnection { t1: (s1 + 0.5).fract(), t2: (s2 + 0.5).fract(), rip: true }],
            ..x
        }),
    );
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).unwrap();
    assert!(ctx.flat.is_ok());
    assert_eq!(ctx.flat.parts.len(), 2, "{:?}", ctx.model.joints.iter().map(|j| &j.name).collect::<Vec<_>>());
}

/// A 120 × 80 plate (a Thicken of a sketch region, 1.5 thick, z 0..1.5) and a sketch of points.
fn plate(st: &mut Studio, points: &[(f64, f64)]) -> (FeatureId, FeatureId, Part) {
    let s = st.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 120.0, 80.0)]);
    let r = st.region(s, Vec2::new(60.0, 40.0));
    let p = params();
    let sm = SheetMetalModelFeature { operation: SheetMetalOp::Thicken, regions: vec![r], params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    let f = st.add("Sheet metal model", FeatureKind::SheetMetalModel(sm));
    let part = st.ok().parts.iter().find(|q| q.id.feature == f).unwrap().clone();
    let pts = st.sketch(PlaneRef::Top, points.iter().map(|(x, y)| SketchOp::AddPoint { pos: Vec2::new(*x, *y) }).collect());
    (f, pts, part)
}

fn louver(locations: FeatureId, target: FaceRef) -> FormFeature {
    FormFeature {
        form: Some(FormPick { source: FormSource::Library(LibraryForm::Louver), name: "Louver".into(), document_name: LIBRARY_NAME.into(), studio: vec![] }),
        variables: LibraryForm::Louver.variables(),
        locations: vec![FormLocation::SketchPoints(locations)],
        targets: vec![target],
        flip: false,
    }
}

#[test]
fn louvers_on_sketch_points_cut_and_raise_the_sheet_and_show_in_the_flat() {
    let mut st = Studio::new();
    let (sm, pts, part) = plate(&mut st, &[(40.0, 25.0), (40.0, 55.0), (80.0, 25.0), (80.0, 55.0)]);
    let before = volume(&part);
    let top = face_near(&part, [60.0, 40.0, 1.5]);
    let f = st.add("Form", FeatureKind::Form(louver(pts, top)));
    let b = st.ok();
    let after = b.parts.iter().find(|q| q.id == part.id).expect("the formed part");
    assert_eq!(b.parts.iter().filter(|q| q.id.feature == sm).count(), 1, "still one part");
    assert!((volume(after) - before).abs() > 1.0, "the louvers change the part ({} → {})", before, volume(after));
    // Raised: the hoods stand up to the louver's height above the sheet.
    let top_z = after.solid.positions.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!((top_z - (1.5 + 4.0)).abs() < 0.05, "{top_z}");
    // The flat shows each louver's outline (its 40 × 8 profile) and centermark.
    let ctx = b.sheet_metal.iter().find(|c| c.feature == sm).unwrap();
    let forms = &ctx.flat.parts[0].forms;
    assert_eq!(forms.len(), 4);
    for fm in forms {
        assert_eq!(fm.form, "Louver");
        let l = &fm.lines[0];
        assert!(l.closed);
        let xs: Vec<f64> = l.points.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = l.points.iter().map(|p| p.y).collect();
        let w = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
        let h = ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min);
        let (a, bb) = if w > h { (w, h) } else { (h, w) };
        assert!((a - 40.0).abs() < 1e-6 && (bb - 8.0).abs() < 1e-6, "{w} × {h}");
    }
    let centres: Vec<(f64, f64)> = forms.iter().map(|f| (f.center.x, f.center.y)).collect();
    let spread = |i: usize| centres.iter().map(|c| if i == 0 { c.0 } else { c.1 }).fold(f64::MIN, f64::max) - centres.iter().map(|c| if i == 0 { c.0 } else { c.1 }).fold(f64::MAX, f64::min);
    assert!((spread(0) - 40.0).abs() < 1e-6 && (spread(1) - 30.0).abs() < 1e-6, "{centres:?}");
    // Opposite direction: the hoods go below the sheet instead.
    let mut x = louver(pts, face_near(&part, [60.0, 40.0, 1.5]));
    x.flip = true;
    st.set(f, FeatureKind::Form(x));
    let b = st.ok();
    let after = b.parts.iter().find(|q| q.id == part.id).unwrap();
    let low = after.solid.positions.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    assert!(low < -1.0, "{low}");
}

#[test]
fn a_form_touching_the_edge_of_its_wall_is_an_error() {
    let mut st = Studio::new();
    let (_, pts, part) = plate(&mut st, &[(10.0, 40.0)]);
    let top = face_near(&part, [60.0, 40.0, 1.5]);
    let f = st.add("Form", FeatureKind::Form(louver(pts, top)));
    let b = st.build();
    let why = b.errors.iter().find(|(id, _)| *id == f).map(|(_, w)| w.clone()).unwrap_or_default();
    assert!(why.contains("can't touch"), "{why:?}");
}

#[test]
fn every_library_form_builds_its_tagged_parts() {
    let doc = cadrs_core::samples::sheetmetal_forms::document().unwrap();
    assert_eq!(doc.elements.len(), LibraryForm::ALL.len());
    for form in LibraryForm::ALL {
        let el = doc.element(cadrs_core::samples::sheetmetal_forms::studio_id(form)).unwrap();
        let b = rebuild::build(el.features());
        assert!(b.errors.is_empty(), "{}: {:?}", form.label(), b.errors);
        let tag: &TagFormFeature = cadrs_core::sheetmetal_form::tag_of(el.features()).expect("a tag");
        for p in tag.add.iter().chain(&tag.remove) {
            assert!(b.parts.iter().any(|q| q.id == *p), "{}: part {p:?} missing", form.label());
        }
    }
}

#[test]
fn each_library_form_goes_on_a_plate() {
    for form in LibraryForm::ALL {
        let mut st = Studio::new();
        let (sm, pts, part) = plate(&mut st, &[(60.0, 40.0)]);
        let top = face_near(&part, [60.0, 40.0, 1.5]);
        let mut x = louver(pts, top);
        x.form = Some(FormPick { source: FormSource::Library(form), name: form.label().into(), document_name: LIBRARY_NAME.into(), studio: vec![] });
        x.variables = form.variables();
        st.add("Form", FeatureKind::Form(x));
        let b = st.build();
        assert!(b.errors.is_empty(), "{}: {:?}", form.label(), b.errors);
        assert_eq!(b.parts.iter().filter(|q| q.id.feature == sm).count(), 1, "{}", form.label());
        let ctx = b.sheet_metal.iter().find(|c| c.feature == sm).unwrap();
        assert_eq!(ctx.flat.parts[0].forms.len(), 1);
    }
}

/// A flat pattern as a PNG in the scenarios' output folder (the flat view panel is P3I.3's).
fn save_flat(flat: &cadrs_sheetmetal::FlatPattern, title: &str, file: &str) {
    let doc = cadrs_sheetmetal::svg::flat_svg(flat, title, 900.0);
    let target = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target").into());
    let dir = format!("{target}/scenarios/sm_p3i9_flats");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(format!("{dir}/{file}.svg"), &doc).unwrap();
    let mut opt = resvg::usvg::Options::default();
    let fonts = opt.fontdb_mut();
    fonts.load_font_file(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/Inter-Regular.ttf")).ok();
    fonts.load_system_fonts();
    fonts.set_sans_serif_family("Inter");
    let tree = resvg::usvg::Tree::from_str(&doc, &opt).unwrap();
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap.save_png(format!("{dir}/{file}.png")).unwrap();
}

#[test]
fn louvers_on_a_converted_box_and_the_flats() {
    // A 120 × 80 × 40 block converted (2 mm, material outside), ten louvers on its top wall.
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 120.0, 80.0)]);
    let r = st.region(s, Vec2::new(60.0, 40.0));
    let e = FeatureId::new();
    st.h.execute(&mut st.d, &cadrs_core::commands::AddExtrude { element: st.el, feature: e, extrude: Default::default() }).unwrap();
    st.h.execute(&mut st.d, &cadrs_core::commands::SetExtrude { element: st.el, feature: e, extrude: cadrs_core::samples::extrude_of(vec![r], 40.0), label: "Extrude".into() }).unwrap();
    let block = st.ok().parts[0].clone();
    let p = Params { thickness: 2.0, bend_radius: 2.0, ..SheetMetalModelFeature::default_params() };
    let sm = SheetMetalModelFeature { parts: vec![block.id], params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    let smf = st.add("Sheet metal model", FeatureKind::SheetMetalModel(sm));
    let b = st.ok();
    let top_part = b.parts.iter().filter(|q| q.id.feature == smf).max_by(|a, b| {
        let z = |q: &Part| q.solid.positions.iter().map(|p| p[2]).fold(f64::MIN, f64::max) + q.solid.positions.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
        z(a).total_cmp(&z(b))
    }).unwrap().clone();
    let pts: Vec<SketchOp> = [35.0, 85.0].iter().flat_map(|x| [16.0, 28.0, 40.0, 52.0, 64.0].map(|y| SketchOp::AddPoint { pos: Vec2::new(*x, y) })).collect();
    let loc = st.sketch(PlaneRef::Top, pts);
    let top = face_near(&top_part, [60.0, 40.0, 42.0]);
    st.add("Form", FeatureKind::Form(louver(loc, top)));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == smf).unwrap();
    let forms: usize = ctx.flat.parts.iter().map(|p| p.forms.len()).sum();
    assert_eq!(forms, 10);
    save_flat(&ctx.flat, "Louvers on the converted box's top wall (Form 1)", "louvers");
    // The loft's flat.
    let mut st = Studio::new();
    let (r1, r2) = rect_and_circle(&mut st);
    let x = SheetMetalLoftFeature {
        profile1: vec![LoftItem::Region(RegionRefKey::of(&r1))],
        profile2: vec![LoftItem::Region(RegionRefKey::of(&r2))],
        params: params(),
        exprs: SheetMetalExprs::of(&params()),
        ..Default::default()
    };
    let f = st.add("Sheet metal loft", FeatureKind::SheetMetalLoft(x));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).unwrap();
    save_flat(&ctx.flat, "Sheet metal loft 1: rectangle to circle", "loft");
}
