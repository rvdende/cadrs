//! Onshape import: the Import feature (STEP and STL files stored with the document) and the
//! Derived feature (parts of another Part Studio, of this document or another one).
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::applied::{EdgeOrFace, FilletFeature};
use cadrs_core::commands::{AddElement, AddExtrude, AddFeature, AddSketch, EditSketch, NewElementKind, RenamePart, SetExtrude, SetFeature, SetPartMaterial};
use cadrs_core::derived::{self, AddDerived, DerivedFeature, DerivedSelection};
use cadrs_core::document::{BooleanOp, Document, EdgeRef, ExtrudeFeature, FeatureKind};
use cadrs_core::export::StepRequest;
use cadrs_core::import::{AddImport, ImportFeature, ImportUnit};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::{DocumentMeta, ElementId, FeatureId, History, Part, Store, samples};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

struct Doc {
    d: Document,
    h: History,
}

impl Doc {
    fn new(name: &str) -> Self {
        Self { d: Document::new(name), h: History::default() }
    }

    fn studio(&self, i: usize) -> ElementId {
        self.d.elements.iter().filter(|e| e.features_ref_is_studio()).nth(i).map(|e| e.id).unwrap()
    }

    fn add_studio(&mut self) -> ElementId {
        let id = ElementId::new();
        self.h.execute(&mut self.d, &AddElement { id, kind: NewElementKind::PartStudio, name: None, after: None }).unwrap();
        id
    }

    fn build(&self, el: ElementId) -> Arc<Build> {
        rebuild::build(&self.d.element(el).unwrap().active_features())
    }

    fn parts(&self, el: ElementId) -> Vec<Part> {
        let b = self.build(el);
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }

    /// A box x0..x1 × y0..y1 on Top, `h` high (New).
    fn block(&mut self, el: ElementId, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) -> FeatureId {
        let s = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: el, feature: s, plane: Some(PlaneRef::Top) }).unwrap();
        let v = Vec2::new;
        let op = SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "rect" };
        self.h.execute(&mut self.d, &EditSketch { element: el, feature: s, op }).unwrap();
        let g = self.d.element(el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, &[v((x0 + x1) / 2.0, (y0 + y1) / 2.0)]);
        let e = ExtrudeFeature { op: BooleanOp::New, depth: h, depth_expr: format!("{h} mm"), ..samples::extrude_of(regions, h) };
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddExtrude { element: el, feature: f, extrude: ExtrudeFeature::default() }).unwrap();
        self.h.execute(&mut self.d, &SetExtrude { element: el, feature: f, extrude: e, label: "Extrude".into() }).unwrap();
        f
    }

    fn add(&mut self, el: ElementId, kind: FeatureKind) -> Result<FeatureId, cadrs_core::CommandError> {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: el, feature, base_name: "Feature".into(), kind })?;
        Ok(feature)
    }
}

trait StudioCheck {
    fn features_ref_is_studio(&self) -> bool;
}

impl StudioCheck for cadrs_core::Element {
    fn features_ref_is_studio(&self) -> bool {
        matches!(self.kind, cadrs_core::ElementKind::PartStudio { .. })
    }
}

fn volumes(parts: &[Part]) -> Vec<f64> {
    let mut v: Vec<f64> = parts.iter().map(|p| p.mass.unwrap().volume).collect();
    v.sort_by(f64::total_cmp);
    v
}

/// A cube `s` mm on a side at `o`, as a binary STL (12 outward triangles).
fn cube_stl(o: [f64; 3], s: f64) -> Vec<u8> {
    let c = |x: f64, y: f64, z: f64| [o[0] + x * s, o[1] + y * s, o[2] + z * s];
    let quads = [
        [c(0., 0., 0.), c(0., 1., 0.), c(1., 1., 0.), c(1., 0., 0.)],
        [c(0., 0., 1.), c(1., 0., 1.), c(1., 1., 1.), c(0., 1., 1.)],
        [c(0., 0., 0.), c(1., 0., 0.), c(1., 0., 1.), c(0., 0., 1.)],
        [c(0., 1., 0.), c(0., 1., 1.), c(1., 1., 1.), c(1., 1., 0.)],
        [c(0., 0., 0.), c(0., 0., 1.), c(0., 1., 1.), c(0., 1., 0.)],
        [c(1., 0., 0.), c(1., 1., 0.), c(1., 1., 1.), c(1., 0., 1.)],
    ];
    let tris: Vec<[[f64; 3]; 3]> = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
    let mut out = vec![0u8; 80];
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in tris {
        out.extend_from_slice(&[0u8; 12]);
        for v in t {
            for x in v {
                out.extend_from_slice(&(x as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&[0u8; 2]);
    }
    out
}

fn ascii_stl(bin: &[u8]) -> String {
    let tris = cadrs_core::import::parse_stl(bin).unwrap();
    let mut s = String::from("solid cube\n");
    for t in tris {
        s += "facet normal 0 0 0\nouter loop\n";
        for v in t {
            s += &format!("vertex {} {} {}\n", v[0], v[1], v[2]);
        }
        s += "endloop\nendfacet\n";
    }
    s + "endsolid cube\n"
}

fn temp_store(tag: &str) -> Store {
    let dir = std::env::temp_dir().join(format!("cadrs-import-derived-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Store::new(dir)
}

#[test]
fn import_step_makes_named_parts() {
    // Two boxes written to STEP by the kernel, with products named after them.
    let mut src = Doc::new("src");
    let el = src.studio(0);
    src.block(el, 0.0, 0.0, 10.0, 20.0, 30.0);
    src.block(el, 50.0, 0.0, 55.0, 5.0, 5.0);
    let parts = src.parts(el);
    let req = StepRequest { parts: vec![(parts[0].id, "Base".into()), (parts[1].id, "Peg".into())], y_up: false, individual: false };
    let step = rebuild::export_step(src.d.element(el).unwrap().active_features(), req).wait().unwrap().remove(0).bytes;

    let mut d = Doc::new("import");
    let el = d.studio(0);
    let f = FeatureId::new();
    d.h.execute(&mut d.d, &AddImport { element: el, feature: f, file_name: "boxes.step".into(), bytes: Arc::new(step.clone()), y_axis_up: false, units: None })
        .unwrap();
    let feat = d.d.element(el).unwrap().feature(f).unwrap().clone();
    assert_eq!(feat.name, "Import 1");
    let parts = d.parts(el);
    assert_eq!(parts.len(), 2);
    close(volumes(&parts)[0], 125.0, 1e-6);
    close(volumes(&parts)[1], 6000.0, 1e-6);
    let mut names: Vec<(String, f64)> = parts.iter().map(|p| (p.name.clone(), p.mass.unwrap().volume)).collect();
    names.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(names[0].0, "Peg");
    assert_eq!(names[1].0, "Base");
    // Faces named by body and index under the feature, the same on every rebuild.
    assert!(parts.iter().all(|p| p.solid.faces.iter().all(|f| matches!(f.name.origin, cadrs_sketch::FaceOrigin::Imported { .. }))));
    let again = rebuild::Rebuilder::new().rebuild(&d.d.element(el).unwrap().active_features());
    for (a, b) in parts.iter().zip(&again.parts) {
        assert_eq!(a.solid.faces.iter().map(|f| f.name).collect::<Vec<_>>(), b.solid.faces.iter().map(|f| f.name).collect::<Vec<_>>());
    }
    // Y axis up: the 30 mm tall box stands along -Y... turned so the file's +Y is +Z.
    let FeatureKind::Import(mut x) = feat.kind.clone() else { panic!() };
    x.y_axis_up = true;
    d.h.execute(&mut d.d, &SetFeature { element: el, feature: f, kind: FeatureKind::Import(x), label: "Y up".into() }).unwrap();
    let base = d.parts(el).into_iter().find(|p| p.name == "Base").unwrap();
    let c = base.mass.unwrap().center_of_mass;
    // (5, 10, 15) turned +90° about X: (5, -15, 10).
    close(c.x, 5.0, 1e-6);
    close(c.y, -15.0, 1e-6);
    close(c.z, 10.0, 1e-6);
}

#[test]
fn import_stl_cube_volume_units_and_storage() {
    let bin = cube_stl([0.0, 100.0, 0.0], 10.0);
    let mut d = Doc::new("stl");
    let el = d.studio(0);
    let f = FeatureId::new();
    d.h.execute(&mut d.d, &AddImport { element: el, feature: f, file_name: "cube.stl".into(), bytes: Arc::new(bin.clone()), y_axis_up: false, units: None })
        .unwrap();
    let parts = d.parts(el);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "cube");
    close(parts[0].mass.unwrap().volume, 1000.0, 1e-6);

    // Specify units: centimetres.
    let FeatureKind::Import(x) = d.d.element(el).unwrap().feature(f).unwrap().kind.clone() else { panic!() };
    let cm = ImportFeature { units: Some(ImportUnit::Centimeter), ..x.clone() };
    d.h.execute(&mut d.d, &SetFeature { element: el, feature: f, kind: FeatureKind::Import(cm), label: "units".into() }).unwrap();
    close(d.parts(el)[0].mass.unwrap().volume, 1e6, 1e-3);

    // Y axis up: the cube at y 100..110 lands at z 100..110.
    let up = ImportFeature { y_axis_up: true, ..x.clone() };
    d.h.execute(&mut d.d, &SetFeature { element: el, feature: f, kind: FeatureKind::Import(up), label: "up".into() }).unwrap();
    close(d.parts(el)[0].mass.unwrap().center_of_mass.z, 105.0, 1e-6);

    // An ASCII STL and two cubes in one file: one part each.
    let mut two = cadrs_core::import::parse_stl(&cube_stl([0.0; 3], 10.0)).unwrap();
    two.extend(cadrs_core::import::parse_stl(&cube_stl([20.0, 0.0, 0.0], 5.0)).unwrap());
    let text = ascii_stl(&bin).replace("endsolid cube\n", "")
        + &two[12..].iter().map(|t| format!("facet normal 0 0 0\nouter loop\n{}endloop\nendfacet\n", t.iter().map(|v| format!("vertex {} {} {}\n", v[0], v[1], v[2])).collect::<String>())).collect::<String>()
        + "endsolid cube\n";
    let mut d2 = Doc::new("ascii");
    let el2 = d2.studio(0);
    d2.h.execute(&mut d2.d, &AddImport { element: el2, feature: FeatureId::new(), file_name: "pair.stl".into(), bytes: Arc::new(text.into_bytes()), y_axis_up: false, units: None })
        .unwrap();
    let parts = d2.parts(el2);
    assert_eq!(volumes(&parts).len(), 2);
    close(volumes(&parts)[0], 125.0, 1e-6);
    close(volumes(&parts)[1], 1000.0, 1e-6);
    assert_eq!(parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["pair", "pair (2)"]);

    // Stored with the document, copied with it, and read back.
    let store = temp_store("stl");
    let meta = DocumentMeta::new("me", 0);
    store.save(&d.d, &meta).unwrap();
    let file = store.blobs_dir(d.d.id).join(cadrs_core::blobs::file_name(&x.blob, "stl"));
    assert_eq!(std::fs::read(&file).unwrap(), bin);
    let copy = cadrs_core::DocumentId::new();
    store.copy_document(d.d.id, copy, "copy", "me", 0).unwrap();
    assert!(store.blobs_dir(copy).join(file.file_name().unwrap()).is_file());
    let loaded = store.load(copy).unwrap().document;
    let el = loaded.elements[0].id;
    let b = rebuild::build(&loaded.element(el).unwrap().active_features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn import_errors() {
    let mut d = Doc::new("bad");
    let el = d.studio(0);
    assert!(d.h.execute(&mut d.d, &AddImport { element: el, feature: FeatureId::new(), file_name: "x.txt".into(), bytes: Arc::new(b"hello".to_vec()), y_axis_up: false, units: None }).is_err());
    // An open mesh (a face missing).
    let mut open = cube_stl([0.0; 3], 10.0);
    let n = 10u32;
    open.truncate(84 + 50 * n as usize);
    open[80..84].copy_from_slice(&n.to_le_bytes());
    let f = FeatureId::new();
    d.h.execute(&mut d.d, &AddImport { element: el, feature: f, file_name: "open.stl".into(), bytes: Arc::new(open), y_axis_up: false, units: None }).unwrap();
    assert!(d.build(el).error(f).is_some());
    // A feature with a blob that isn't loaded.
    let g = d.add(el, FeatureKind::Import(ImportFeature { blob: "0".repeat(32), file_name: "gone.step".into(), ..ImportFeature::default() })).unwrap();
    assert!(d.build(el).error(g).unwrap().contains("missing"));
}

/// Two parts in Part Studio 1 (a 10×20×30 box "Base" of steel and a 5×5×5 cube), and Part
/// Studio 2 deriving from it.
fn two_studios() -> (Doc, ElementId, ElementId) {
    let mut d = Doc::new("derive");
    let a = d.studio(0);
    d.block(a, 0.0, 0.0, 10.0, 20.0, 30.0);
    d.block(a, 50.0, 0.0, 55.0, 5.0, 5.0);
    let parts = d.parts(a);
    d.h.execute(&mut d.d, &RenamePart { element: a, part: parts[0].id, name: "Base".into() }).unwrap();
    d.h.execute(&mut d.d, &SetPartMaterial { element: a, parts: vec![parts[0].id], material: cadrs_core::material::library("Steel") }).unwrap();
    let b = d.add_studio();
    (d, a, b)
}

#[test]
fn derive_all_or_one_part_from_this_document() {
    let (mut d, a, b) = two_studios();
    let src = d.parts(a);
    let f = d.add(b, FeatureKind::Derived(Box::new(DerivedFeature::new(None, a)))).unwrap();
    let parts = d.parts(b);
    assert_eq!(volumes(&parts), volumes(&src));
    let base = parts.iter().find(|p| p.name == "Base").expect("the source's name");
    assert_eq!(base.id, derived::part_id(f, src[0].id));
    assert_eq!(parts.iter().find(|p| p.name != "Base").unwrap().name, src[1].name);
    // The source's material comes along.
    let props = d.d.element(b).unwrap().part_props().to_vec();
    assert_eq!(cadrs_core::parts::part_material(base, &props).map(|m| m.name.as_str()), Some("Steel"));

    // A rename in the source reaches the derived part.
    d.h.execute(&mut d.d, &RenamePart { element: a, part: src[0].id, name: "Plate".into() }).unwrap();
    assert!(d.parts(b).iter().any(|p| p.name == "Plate"));

    // Only the cube.
    let FeatureKind::Derived(x) = d.d.element(b).unwrap().feature(f).unwrap().kind.clone() else { panic!() };
    let one = DerivedFeature { selection: DerivedSelection { all: false, parts: vec![src[1].id], ..Default::default() }, ..*x };
    d.h.execute(&mut d.d, &SetFeature { element: b, feature: f, kind: FeatureKind::Derived(Box::new(one)), label: "one".into() }).unwrap();
    let parts = d.parts(b);
    assert_eq!(parts.len(), 1);
    close(parts[0].mass.unwrap().volume, 125.0, 1e-6);

    // An edit of the source (a third box) shows up; the cube keeps its id.
    let cube = parts[0].id;
    let all = DerivedFeature::new(None, a);
    d.h.execute(&mut d.d, &SetFeature { element: b, feature: f, kind: FeatureKind::Derived(Box::new(all)), label: "all".into() }).unwrap();
    d.block(a, -40.0, 0.0, -30.0, 10.0, 10.0);
    let parts = d.parts(b);
    assert_eq!(parts.len(), 3);
    assert!(parts.iter().any(|p| p.id == cube));
}

#[test]
fn derived_face_names_are_stable_and_a_fillet_resolves() {
    let (mut d, a, b) = two_studios();
    let src = d.parts(a);
    let f = d.add(b, FeatureKind::Derived(Box::new(DerivedFeature::new(None, a)))).unwrap();
    let parts = d.parts(b);
    let base = parts.iter().find(|p| p.name == "Base").unwrap().clone();
    // Every face is the source face's derived name.
    let src_base = &src[0];
    let want: Vec<_> = src_base.solid.faces.iter().map(|x| derived::face_name(f, &x.name)).collect();
    let mut got: Vec<_> = base.solid.faces.iter().map(|x| x.name).collect();
    let mut want_sorted = want.clone();
    got.sort();
    want_sorted.sort();
    assert_eq!(got, want_sorted);
    // Edges too.
    for e in &src_base.solid.edges {
        let n = derived::edge_name(f, &e.name);
        assert!(base.solid.edges.iter().any(|x| x.name == n), "{n:?}");
    }
    // Unchanged by an edit of the source elsewhere and by a fresh rebuild.
    d.block(a, -40.0, 0.0, -30.0, 10.0, 10.0);
    let fresh = rebuild::Rebuilder::new().rebuild(&d.d.element(b).unwrap().active_features());
    let base2 = fresh.part(base.id).unwrap();
    let mut got2: Vec<_> = base2.solid.faces.iter().map(|x| x.name).collect();
    got2.sort();
    assert_eq!(got2, want_sorted);

    // A 2 mm fillet on the derived box's top edge along x at y = 0, z = 30.
    let e = base.solid.edges.iter().min_by(|x, y| x.distance([5.0, 0.0, 30.0]).total_cmp(&y.distance([5.0, 0.0, 30.0]))).unwrap();
    assert!(e.distance([5.0, 0.0, 30.0]) < 1e-6);
    let edge = EdgeRef { part: base.id, edge: e.name, seed: [5.0, 0.0, 30.0] };
    let fillet = FilletFeature { entities: vec![EdgeOrFace::Edge(edge)], size: 2.0, size_expr: "2 mm".into(), ..FilletFeature::default() };
    let g = d.add(b, FeatureKind::Fillet(fillet)).unwrap();
    let build = d.build(b);
    assert!(build.error(g).is_none(), "{:?}", build.errors);
    // 6000 − (4 − π)·2²/4 · 10.
    close(build.part(base.id).unwrap().mass.unwrap().volume, 6000.0 - (4.0 - std::f64::consts::PI) * 10.0, 1e-6);
    // Still there after the source changes again.
    d.block(a, -80.0, 0.0, -70.0, 10.0, 10.0);
    assert!(d.build(b).error(g).is_none());
}

#[test]
fn derive_from_another_document() {
    let (src, a, _) = two_studios();
    let store = temp_store("other");
    store.save(&src.d, &DocumentMeta::new("me", 0)).unwrap();
    let mut d = Doc::new("user");
    let el = d.studio(0);
    let f = d.add(el, FeatureKind::Derived(Box::new(DerivedFeature::new(Some(src.d.id), a)))).unwrap();
    // No loader yet: nothing to build from.
    assert!(d.build(el).error(f).unwrap().contains("nothing to derive"));
    let s2 = store.clone();
    derived::resolve_document_with(&mut d.d, &move |id| s2.load(id).ok().map(|f| f.document));
    let parts = d.parts(el);
    assert_eq!(parts.len(), 2);
    close(volumes(&parts)[0], 125.0, 1e-6);
    close(volumes(&parts)[1], 6000.0, 1e-6);
    assert!(parts.iter().any(|p| p.name == "Base"));
    // The snapshot stays through later commands (a link to another document is kept until it
    // is updated).
    d.block(el, 100.0, 0.0, 110.0, 10.0, 10.0);
    assert_eq!(d.parts(el).len(), 3);
    // A part that isn't in the source any more.
    let FeatureKind::Derived(x) = d.d.element(el).unwrap().feature(f).unwrap().kind.clone() else { panic!() };
    let gone = DerivedFeature { selection: DerivedSelection { all: false, parts: vec![cadrs_core::PartId::new(FeatureId::new(), 0)], ..Default::default() }, ..*x };
    d.h.execute(&mut d.d, &SetFeature { element: el, feature: f, kind: FeatureKind::Derived(Box::new(gone)), label: "gone".into() }).unwrap();
    assert!(d.build(el).error(f).is_some());
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn derived_errors_and_cycles() {
    // The merged Derived feature (phase3c's P3G.4) refuses self and circular references when
    // they are made (`derived::check`), rather than failing them at rebuild.
    let (mut d, a, b) = two_studios();
    let add = |d: &mut Doc, el: ElementId, src: ElementId| {
        let feature = FeatureId::new();
        d.h.execute(&mut d.d, &AddDerived { element: el, feature, derived: DerivedFeature::new(None, src), links: Vec::new() }).map(|_| feature)
    };
    // Itself.
    assert!(add(&mut d, a, a).unwrap_err().to_string().contains("derive itself"));
    // A → B → A.
    let fb = add(&mut d, b, a).unwrap();
    assert!(d.build(b).error(fb).is_none());
    assert!(add(&mut d, a, b).unwrap_err().to_string().contains("Circular"));
    assert!(d.build(b).error(fb).is_none());
    // A missing tab, and an assembly: nothing to build from.
    let gone = d.add(b, FeatureKind::Derived(Box::new(DerivedFeature::new(None, ElementId::new())))).unwrap();
    assert!(d.build(b).error(gone).unwrap().contains("nothing to derive"));
    let asm = d.d.elements.iter().find(|e| matches!(e.kind, cadrs_core::ElementKind::Assembly)).map(|e| e.id);
    if let Some(asm) = asm {
        let x = d.add(b, FeatureKind::Derived(Box::new(DerivedFeature::new(None, asm)))).unwrap();
        assert!(d.build(b).error(x).unwrap().contains("nothing to derive"));
    }
    // Saved and loaded: the reference and its snapshot survive.
    let text = ron::to_string(&d.d).unwrap();
    let mut back: Document = ron::from_str(&text).unwrap();
    derived::resolve_document(&mut back);
    let bf = back.element(b).unwrap().active_features();
    let build = rebuild::build(&bf);
    assert!(build.error(fb).is_none());
}

/// The `Derived(...)` value in RON text `text` (its start and end), skipping string contents.
fn derived_span(text: &str) -> (usize, usize) {
    let start = text.find("Derived(").expect("a Derived feature");
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, c) in text[start..].char_indices() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (start, start + i + 1);
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced Derived(...)")
}

/// Main's pre-merge Derived format (schema 4, before the merge with phase3c): a document the
/// old importer saved, with a fillet on a derived edge, loads into the merged model (a
/// whole-Part-Studio reference at the workspace, Include mate connectors as saved, the source's
/// features filled in on resolve) and rebuilds with the fillet still on its edge.
#[test]
fn main_pre_merge_derived_format_loads_and_rebuilds() {
    let (mut d, a, b) = two_studios();
    let src = d.parts(a);
    // The merged model's reading of the old feature, to take the fillet's edge from.
    let legacy = DerivedFeature { include_connectors: true, legacy_names: true, ..DerivedFeature::new(None, a) };
    let f = d.add(b, FeatureKind::Derived(Box::new(legacy))).unwrap();
    let parts = d.parts(b);
    let base = parts.iter().find(|p| p.name == "Base").unwrap().clone();
    // The old ids and names.
    assert_eq!(base.id, derived::legacy_part_id(f, src[0].id));
    let mut got: Vec<_> = base.solid.faces.iter().map(|x| x.name).collect();
    let mut want: Vec<_> = src[0].solid.faces.iter().map(|x| derived::legacy_face_name(f, &x.name)).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    let e = base.solid.edges.iter().min_by(|x, y| x.distance([5.0, 0.0, 30.0]).total_cmp(&y.distance([5.0, 0.0, 30.0]))).unwrap();
    let edge = EdgeRef { part: base.id, edge: e.name, seed: [5.0, 0.0, 30.0] };
    let fillet = FilletFeature { entities: vec![EdgeOrFace::Edge(edge)], size: 2.0, size_expr: "2 mm".into(), ..FilletFeature::default() };
    let g = d.add(b, FeatureKind::Fillet(fillet)).unwrap();
    let filleted = 6000.0 - (4.0 - std::f64::consts::PI) * 10.0;
    close(d.build(b).part(base.id).unwrap().mass.unwrap().volume, filleted, 1e-6);

    // The document as main's importer saved it: the Derived feature in the old format, in a
    // schema 4 file.
    let store = temp_store("legacy");
    store.save(&d.d, &DocumentMeta::new("me", 0)).unwrap();
    let path = store.document_path(d.d.id);
    let text = std::fs::read_to_string(&path).unwrap();
    let (s0, s1) = derived_span(&text);
    let old = format!("Derived((document: None, element: Some((\"{}\")), parts: [], include_mate_connectors: true, placement: AtOrigin))", a.0);
    let old_text = format!("{}{}{}", &text[..s0], old, &text[s1..]).replacen("version: 5", "version: 4", 1);
    assert!(old_text.contains("version: 4") && !old_text.contains("legacy_names"));
    std::fs::write(&path, &old_text).unwrap();

    // It loads through the schema 4 migration, and resolving fills in the source.
    let file = store.load(d.d.id).unwrap();
    let mut back = file.document;
    let FeatureKind::Derived(x) = &back.element(b).unwrap().feature(f).unwrap().kind else { panic!() };
    assert_eq!(x.source.map(|r| (r.document, r.at, r.element)), Some((None, cadrs_core::external::RefAt::Workspace, a)));
    assert!(x.selection.all && x.include_connectors && x.legacy_names && x.include_properties);
    assert_eq!(x.placement, derived::DerivedPlacement::BaseOrigin);
    assert!(x.studio.is_empty());
    derived::resolve_document(&mut back);
    let FeatureKind::Derived(x) = &back.element(b).unwrap().feature(f).unwrap().kind else { panic!() };
    assert_eq!(x.studio, d.d.element(a).unwrap().active_features());
    assert_eq!(x.source_name, d.d.element(a).unwrap().name);
    // It rebuilds: the same parts, the fillet still on its edge, the source's material.
    let build = rebuild::build(&back.element(b).unwrap().active_features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert!(build.error(g).is_none());
    close(build.part(base.id).unwrap().mass.unwrap().volume, filleted, 1e-6);
    assert_eq!(volumes(&build.parts).len(), 2);
    let props = back.element(b).unwrap().part_props().to_vec();
    assert_eq!(cadrs_core::parts::part_material(build.part(base.id).unwrap(), &props).map(|m| m.name.as_str()), Some("Steel"));
    // Saved again, it is in the merged format and keeps its old names.
    let again = ron::to_string(&back).unwrap();
    assert!(!again.contains("include_mate_connectors") && again.contains("legacy_names:true"));
    let reread: Document = ron::from_str(&again).unwrap();
    assert_eq!(reread.element(b).unwrap().active_features(), back.element(b).unwrap().active_features());

    // Selected parts, another document and Include mate connectors off, as a bare RON value.
    let other = cadrs_core::DocumentId::new();
    let text = format!(
        "Derived((document: Some((\"{}\")), element: Some((\"{}\")), version: Some(\"V1\"), parts: [(feature: (\"{}\"), index: {})], include_mate_connectors: false, placement: AtOrigin))",
        other.0, a.0, src[1].id.feature.0, src[1].id.index
    );
    let FeatureKind::Derived(x) = ron::from_str::<FeatureKind>(&text).unwrap() else { panic!() };
    assert_eq!(x.source.map(|r| (r.document, r.element)), Some((Some(other), a)));
    assert!(!x.selection.all && x.selection.parts == vec![src[1].id] && !x.include_connectors && x.legacy_names);
    // A new feature that never picked a studio waits for one.
    let FeatureKind::Derived(x) = ron::from_str::<FeatureKind>("Derived((document: None, element: None, parts: [], include_mate_connectors: true, placement: AtOrigin))").unwrap() else { panic!() };
    assert_eq!(x.problem(), Some("Select a Part Studio to derive"));
    let _ = std::fs::remove_dir_all(store.root());
}

/// The scenario's fixture (`scenarios/onshape_import.ron`): an L bracket, 496 mm² × 20 mm.
#[test]
fn bracket_fixture_volume() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/import_bracket.stl");
    let bytes = std::fs::read(path).unwrap();
    let mut d = Doc::new("fixture");
    let el = d.studio(0);
    d.h.execute(&mut d.d, &AddImport { element: el, feature: FeatureId::new(), file_name: "import_bracket.stl".into(), bytes: Arc::new(bytes), y_axis_up: true, units: None })
        .unwrap();
    let parts = d.parts(el);
    assert_eq!(parts.len(), 1);
    close(parts[0].mass.unwrap().volume, 9920.0, 1e-6);
}
