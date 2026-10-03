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
    // Planar facets meet at mitred facet joints (no bends: its steep edges fan). The folded part
    // is exactly the walls' mitred slabs...
    assert!(ctx.model.joints.iter().all(|j| j.bend().is_none()));
    let slabs: f64 = ctx.model.walls.iter().map(|w| cadrs_sheetmetal::loft::mesh_volume(&cadrs_sheetmetal::loft::wall_slab(&ctx.model, w.id, &[]).unwrap())).sum();
    assert!((v - slabs).abs() / v < 1e-6, "folded {v} vs its slabs {slabs}");
    // ...and those are the flat times T, plus at each facet joint the mitre's two wedges
    // (L T² tan(φ/2), φ the angle the walls turn by; taken off where the walls turn towards
    // their material), to within the T³ terms where mitres meet at a corner.
    let mut mitres = 0.0;
    for j in &ctx.model.joints {
        let JointKind::Tangent { on_a, .. } = j.kind else { continue };
        let (wa, wb) = (ctx.model.wall(j.a).unwrap(), ctx.model.wall(j.b).unwrap());
        let (na, nb) = (wa.surface.normal().unwrap(), wb.surface.normal().unwrap());
        let phi = na.dot(&nb).clamp(-1.0, 1.0).acos();
        let (a3, b3) = (wa.surface.point(on_a.a), wa.surface.point(on_a.b));
        let d = (b3 - a3).normalize();
        let n = wb.outline.outer.len() as f64;
        let mid_b = wb.surface.point(cadrs_sheetmetal::poly::P2::from(wb.outline.outer.iter().fold(nalgebra::Vector2::zeros(), |s, q| s + q.coords) / n));
        let x = mid_b - a3;
        let into_b = x - d * x.dot(&d);
        let sign = if into_b.dot(&na) > 0.0 { -1.0 } else { 1.0 };
        mitres += sign * (b3 - a3).norm() * p.thickness * p.thickness * (phi / 2.0).tan();
    }
    let corners = ctx.model.walls.iter().map(|w| w.outline.outer.len()).sum::<usize>() as f64 * p.thickness.powi(3);
    assert!((slabs - (flat_volume + mitres)).abs() < corners, "slabs {slabs} vs flat × T {flat_volume} + mitres {mitres} (corner terms ≤ {corners})");
    assert!((v - (flat_volume + mitres)).abs() / v < 2e-3, "{}", (v - (flat_volume + mitres)) / v);
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
fn louvers_on_a_bracket_and_the_flats() {
    // An L bracket (Sheet metal model Extrude of a Front-plane chain: a 140 base and a 50
    // upright, 100 deep, 2 mm), ten louvers on its base.
    let mut st = Studio::new();
    let chain = st.sketch(
        PlaneRef::Front,
        vec![SketchOp::AddPolyline { points: vec![Vec2::new(0.0, 50.0), Vec2::new(0.0, 0.0), Vec2::new(140.0, 0.0)], closed: false, construction: false, label: "Add line" }],
    );
    let p = Params { thickness: 2.0, bend_radius: 2.0, ..SheetMetalModelFeature::default_params() };
    let sm = SheetMetalModelFeature { operation: SheetMetalOp::Extrude, sketches: vec![chain], depth: 100.0, depth_expr: "100 mm".into(), symmetric: true, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    let smf = st.add("Sheet metal model", FeatureKind::SheetMetalModel(sm));
    let b = st.ok();
    let bracket = b.parts.iter().find(|q| q.id.feature == smf).unwrap().clone();
    let pts: Vec<SketchOp> = [50.0, 105.0].iter().flat_map(|x| [-32.0, -16.0, 0.0, 16.0, 32.0].map(|y| SketchOp::AddPoint { pos: Vec2::new(*x, y) })).collect();
    let loc = st.sketch(PlaneRef::Top, pts);
    // The base's upper face.
    let s = &bracket.solid;
    let i = (0..s.faces.len())
        .filter(|i| s.faces[*i].center.is_some_and(|c| c[0] > 20.0 && c[0] < 130.0 && c[1].abs() < 1.0))
        .max_by(|a, b| s.faces[*a].center.unwrap()[2].total_cmp(&s.faces[*b].center.unwrap()[2]))
        .unwrap();
    let top = FaceRef { part: bracket.id, face: s.faces[i].name, seed: s.faces[i].center.unwrap() };
    st.add("Form", FeatureKind::Form(louver(loc, top)));
    let b = st.ok();
    assert_eq!(b.parts.iter().filter(|q| q.id.feature == smf).count(), 1);
    let ctx = b.sheet_metal.iter().find(|c| c.feature == smf).unwrap();
    let forms: usize = ctx.flat.parts.iter().map(|p| p.forms.len()).sum();
    assert_eq!(forms, 10);
    save_flat(&ctx.flat, "Louvers on the bracket's base (Form 1)", "louvers");
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

/// A Form of library `form` (its default variables) on the points of `pts`, on face `target`.
fn library_form(form: LibraryForm, pts: FeatureId, target: FaceRef) -> FormFeature {
    FormFeature {
        form: Some(FormPick { source: FormSource::Library(form), name: form.label().into(), document_name: LIBRARY_NAME.into(), studio: vec![] }),
        variables: form.variables(),
        locations: vec![FormLocation::SketchPoints(pts)],
        targets: vec![target],
        flip: false,
    }
}

#[test]
fn forms_go_into_the_flat_dxf_on_their_own_layers() {
    use cadrs_core::flat_export::{FlatExportOptions, centermark, flat_page};
    use cadrs_drawing::sheet_sketch::Entity;
    let mut st = Studio::new();
    let (sm, pts, part) = plate(&mut st, &[(40.0, 25.0), (40.0, 55.0)]);
    let top = face_near(&part, [60.0, 40.0, 1.5]);
    st.add("Form", FeatureKind::Form(louver(pts, top)));
    let dimple_at = st.sketch(PlaneRef::Top, vec![SketchOp::AddPoint { pos: Vec2::new(90.0, 40.0) }]);
    st.add("Form", FeatureKind::Form(library_form(LibraryForm::Dimple, dimple_at, top)));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == sm).unwrap();
    let flat = &ctx.flat.parts[0];
    assert_eq!(flat.forms.len(), 3);
    let on = FlatExportOptions { form_outlines: true, form_centermarks: true, ..Default::default() };
    let page = flat_page(flat, &[], &on, "Plate");
    for v in cadrs_drawing::dxf::DxfVersion::ALL {
        let back = cadrs_drawing::dxf::read_dxf(&cadrs_drawing::dxf::write_dxf_version(&page, v)).unwrap();
        let on_layer = |name: &str| back.entities.iter().zip(&back.layers).filter(|(_, l)| l.as_str() == name).map(|(e, _)| e.clone()).collect::<Vec<_>>();
        // Each louver's outline is its 40 × 8 profile (four lines), the dimple's a Ø16 circle.
        let outlines = on_layer("FORM_OUTLINES");
        let lines: Vec<&Entity> = outlines.iter().filter(|e| matches!(e, Entity::Line { .. })).collect();
        let circles: Vec<f64> = outlines.iter().filter_map(|e| if let Entity::Circle { radius, .. } = e { Some(*radius) } else { None }).collect();
        assert_eq!((lines.len(), circles.len()), (8, 1), "{outlines:?}");
        assert!((circles[0] - 8.0).abs() < 1e-6, "{circles:?}");
        let length: f64 = lines.iter().map(|e| if let Entity::Line { a, b } = e { (a[0] - b[0]).hypot(a[1] - b[1]) } else { 0.0 }).sum();
        assert!((length - 2.0 * (2.0 * 40.0 + 2.0 * 8.0)).abs() < 1e-6, "{length}");
        // A cross on each form's centre.
        let marks = on_layer("FORM_CENTERMARKS");
        assert_eq!(marks.len(), 6);
        for f in &flat.forms {
            let (h, w) = centermark(f);
            for s in [h, w] {
                assert!(marks.iter().any(|e| matches!(e, Entity::Line { a, b } if (a[0] - s.a.x).abs() < 1e-9 && (a[1] - s.a.y).abs() < 1e-9 && (b[0] - s.b.x).abs() < 1e-9 && (b[1] - s.b.y).abs() < 1e-9)), "{s:?} in {marks:?}");
            }
            let c = (s_mid(&h), s_mid(&w));
            assert!((c.0.0 - f.center.x).abs() < 1e-9 && (c.1.1 - f.center.y).abs() < 1e-9);
        }
    }
    // Off (Onshape's defaults): neither layer.
    let page = flat_page(flat, &[], &FlatExportOptions::default(), "Plate");
    let dxf = cadrs_drawing::dxf::write_dxf(&page);
    assert!(!dxf.contains("FORM_OUTLINES") && !dxf.contains("FORM_CENTERMARKS"));
    // Only the outlines.
    let page = flat_page(flat, &[], &FlatExportOptions { form_outlines: true, ..Default::default() }, "Plate");
    let back = cadrs_drawing::dxf::read_dxf(&cadrs_drawing::dxf::write_dxf(&page)).unwrap();
    assert!(back.layers.iter().any(|l| l == "FORM_OUTLINES") && !back.layers.iter().any(|l| l == "FORM_CENTERMARKS"));
}

fn s_mid(s: &cadrs_sheetmetal::poly::Seg2) -> (f64, f64) {
    ((s.a.x + s.b.x) / 2.0, (s.a.y + s.b.y) / 2.0)
}

#[test]
fn the_dimple_and_the_emboss_add_their_closed_form_volumes() {
    use cadrs_core::samples::sheetmetal_forms::{dimple, emboss};
    let t = params().thickness;
    for form in [LibraryForm::Dimple, LibraryForm::Emboss] {
        let mut st = Studio::new();
        let (_, pts, part) = plate(&mut st, &[(60.0, 40.0)]);
        let before = volume(&part);
        let top = face_near(&part, [60.0, 40.0, 1.5]);
        st.add("Form", FeatureKind::Form(library_form(form, pts, top)));
        let b = st.ok();
        let after = volume(b.parts.iter().find(|q| q.id == part.id).unwrap());
        let v = |n: &str| form.variables().into_iter().find(|x| x.name == n).unwrap().value;
        let expected = match form {
            LibraryForm::Dimple => dimple(v("Diameter"), v("Height"), t).added_volume(),
            _ => emboss(v("Length"), v("Width"), v("Height"), t).added_volume(),
        };
        let added = after - before;
        assert!(expected > 1.0);
        assert!((added - expected).abs() < 1e-6 * before, "{}: added {added} vs {expected}", form.label());
    }
}

#[test]
fn a_current_document_form_follows_its_studio_and_needs_a_thickness_variable() {
    use cadrs_core::commands::{AddElement, NewElementKind};
    let mut st = Studio::new();
    let (sm, pts, part) = plate(&mut st, &[(60.0, 40.0)]);
    let top = face_near(&part, [60.0, 40.0, 1.5]);
    // The user's own form, in a Part Studio of this document (built as the library's emboss).
    let el = ElementId::new();
    st.h.execute(&mut st.d, &AddElement { id: el, kind: NewElementKind::PartStudio, name: Some("My emboss".into()), after: None }).unwrap();
    cadrs_core::samples::sheetmetal_forms::build_in(&mut cadrs_core::studio::DocHistory(&mut st.d, &mut st.h), el, LibraryForm::Emboss, &LibraryForm::Emboss.variables(), 1.0).unwrap();
    let studio = st.d.element(el).unwrap().features().to_vec();
    let x = FormFeature {
        form: Some(FormPick { source: FormSource::Current { element: el }, name: "My emboss".into(), document_name: st.d.name.clone(), studio: studio.clone() }),
        variables: Vec::new(),
        locations: vec![FormLocation::SketchPoints(pts)],
        targets: vec![top],
        flip: false,
    };
    let f = st.add("Form", FeatureKind::Form(x));
    let top_z = |b: &Build| b.parts.iter().find(|q| q.id == part.id).unwrap().solid.positions.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    let b = st.ok();
    assert!((top_z(&b) - (1.5 + 2.5)).abs() < 1e-6, "{}", top_z(&b));
    // The form made taller in its own Part Studio (its add part's extrude, 1 under the origin
    // to 4 above it): the Form follows at the next rebuild.
    let add = cadrs_core::samples::sheetmetal_forms::add_part(LibraryForm::Emboss).feature;
    let FeatureKind::Extrude(mut e) = studio.iter().find(|g| g.id == add).unwrap().kind.clone() else { panic!("an extrude") };
    e.depth = 5.0;
    e.depth_expr = "5 mm".into();
    st.h.execute(&mut st.d, &cadrs_core::commands::SetExtrude { element: el, feature: add, extrude: e, label: "Extrude".into() }).unwrap();
    let b = st.ok();
    assert!((top_z(&b) - (1.5 + 4.0)).abs() < 1e-6, "the edit propagated: {}", top_z(&b));
    let ctx = b.sheet_metal.iter().find(|c| c.feature == sm).unwrap();
    assert_eq!(ctx.flat.parts[0].forms.len(), 1);
    // Without a Length variable named thickness it is no form: a clear error.
    let tv = studio.iter().find(|g| matches!(&g.kind, FeatureKind::Variable(v) if v.name == "thickness")).unwrap().clone();
    let FeatureKind::Variable(mut renamed) = tv.kind.clone() else { unreachable!() };
    renamed.name = "sheet".into();
    st.h.execute(&mut st.d, &SetFeature { element: el, feature: tv.id, kind: FeatureKind::Variable(renamed), label: "Rename".into() }).unwrap();
    let b = st.build();
    let why = b.errors.iter().find(|(id, _)| *id == f).map(|(_, w)| w.clone()).unwrap_or_default();
    assert!(why.contains("thickness") && why.contains("My emboss"), "{why:?}");
}

fn edge_near(part: &Part, p: [f64; 3]) -> cadrs_core::document::EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    cadrs_core::document::EdgeRef { part: part.id, edge: e.name, seed: p }
}

#[test]
fn a_loft_added_on_the_models_edge_bends_onto_it_as_one_part() {
    loft_add_on_an_edge(false);
}

#[test]
fn a_loft_added_on_the_far_faces_edge_bends_onto_it_too() {
    loft_add_on_an_edge(true);
}

fn loft_add_on_an_edge(far: bool) {
    use cadrs_core::sheetmetal_loft::SmLoftOp;
    let mut st = Studio::new();
    let (sm, _, part) = plate(&mut st, &[(10.0, 10.0)]);
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == sm).unwrap();
    // The edge at x = 120 on the face the model is defined on (or on the sheet's other face).
    let cadrs_sheetmetal::model::Surface::Planar { origin, .. } = ctx.model.walls[0].surface else { panic!() };
    let zs: Vec<f64> = part.solid.positions.iter().map(|p| p[2]).collect();
    let (zlo, zhi) = (zs.iter().cloned().fold(f64::MAX, f64::min), zs.iter().cloned().fold(f64::MIN, f64::max));
    let z = if !far { origin.z } else if (origin.z - zlo).abs() < 1e-6 { zhi } else { zlo };
    let edge = edge_near(&part, [120.0, 40.0, z]);
    // Profile 2: a 60 long line 40 above the plate's face and 30 out from its edge.
    let frame = PlaneFrame { origin: [0.0, 0.0, origin.z + 40.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
    let plane = cadrs_core::plane::PlaneFeature {
        entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Top)],
        offset: origin.z + 40.0,
        offset_expr: format!("{} mm", origin.z + 40.0),
        ..Default::default()
    };
    let pf = st.add("Plane", FeatureKind::Plane(plane));
    let s = st.sketch(
        PlaneRef::Feature(FeaturePlane::new(pf.0, frame)),
        vec![SketchOp::AddPolyline { points: vec![Vec2::new(150.0, 10.0), Vec2::new(150.0, 70.0)], closed: false, construction: false, label: "Add line" }],
    );
    let g = st.d.element(st.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let (curve, _) = g.curves.iter().next().unwrap();
    let x = SheetMetalLoftFeature {
        op: SmLoftOp::Add,
        merge_scope: vec![part.id],
        profile1: vec![LoftItem::Edge(edge)],
        profile2: vec![LoftItem::Curve(cadrs_core::sheetmetal::CurveRef { sketch: s, curve })],
        params: params(),
        exprs: SheetMetalExprs::of(&params()),
        ..Default::default()
    };
    let f = st.add("Sheet metal loft", FeatureKind::SheetMetalLoft(x));
    let b = st.ok();
    // One part (the plate's, its id kept), one flat.
    let parts: Vec<&Part> = b.parts.iter().filter(|q| q.id.feature == sm || q.id.feature == f).collect();
    assert_eq!(parts.len(), 1, "{:?}", parts.iter().map(|q| q.id).collect::<Vec<_>>());
    assert_eq!(parts[0].id, part.id);
    let ctx = b.sheet_metal.iter().find(|c| c.feature == sm).unwrap();
    assert!(ctx.flat.is_ok(), "{:?}", ctx.flat.errors);
    assert_eq!(ctx.flat.parts.len(), 1, "the loft wall lies flat with the plate");
    // Joined by a bend of the model's radius, not merely united.
    let plate_wall = ctx.model.walls[0].id;
    let bend = ctx.model.joints.iter().find(|j| (j.a == plate_wall || j.b == plate_wall) && j.bend().is_some()).expect("a bend onto the plate");
    let r = bend.bend().unwrap();
    assert!((r.radius - params().bend_radius).abs() < 1e-9);
    let rise = (40.0f64).atan2(30.0);
    assert!((r.angle - rise).abs() < 1e-6, "{} vs {rise}", r.angle);
    // The folded part has the flat's volume (the bend region's to within its K factor).
    let p = params();
    let flat_volume = ctx.flat.parts[0].area() * p.thickness;
    // (A bend of angle θ, K factor K and length L holds θ (r + T/2) T L of sheet; its flat
    // θ (r + K T) T L.)
    let k_err = rise * (0.5 - p.k_factor) * p.thickness * p.thickness * 80.0;
    let v = volume(parts[0]);
    assert!((v - flat_volume).abs() <= k_err.abs() + 1e-6 * v, "folded {v} vs flat × T {flat_volume} (K error {k_err})");
}
