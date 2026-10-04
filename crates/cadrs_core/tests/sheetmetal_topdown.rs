//! Top-down sheet metal (P3I.8, SM18) and sheet metal parts as parts (X3) on the Heating
//! Mantle stand-in (`samples::sheetmetal_topdown`): the space allocation part derived at the
//! workspace, an Enclosure converted around it (two faces excluded, Keep input part) and a Cover
//! thickened on the other two; the models named after their parts; editing the master in its own
//! studio updates both. The parts rename, take a material, measure and export like any part,
//! through refolds.
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::commands::SetPartMaterial;
use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
use cadrs_core::rebuild::{Build, Rebuilder};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::sheetmetal_topdown as td;
use cadrs_core::sheetmetal_joint::{PutModifyJoint, TableEdit, table_edit};
use cadrs_core::{Document, FeatureId, Part};

fn build(doc: &Document) -> Arc<Build> {
    let b = cadrs_core::rebuild::build(&doc.element(td::STUDIO).unwrap().active_features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    b
}

/// The part with this display name (its rename).
fn part<'a>(b: &'a Build, name: &str) -> &'a Part {
    let (doc, _) = td::document().unwrap();
    let props = doc.element(td::STUDIO).unwrap().part_props().to_vec();
    let id = b.parts.iter().find(|p| cadrs_core::parts::display_name(p, &props) == name || p.name == name).map(|p| p.id);
    let id = id.or(match name {
        "Enclosure" => Some(td::ENCLOSURE_PART),
        "Cover" => Some(td::COVER_PART),
        _ => None,
    });
    b.parts.iter().find(|p| Some(p.id) == id).unwrap_or_else(|| panic!("no part {name}: {:?}", b.parts.iter().map(|p| &p.name).collect::<Vec<_>>()))
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().unwrap().volume
}

fn flat_extent(b: &Build, model: FeatureId) -> (f64, f64) {
    let ctx = b.sheet_metal.iter().find(|c| c.feature == model).unwrap();
    let (lo, hi) = ctx.flat.parts[0].bounds().unwrap();
    (hi.x - lo.x, hi.y - lo.y)
}

#[test]
fn an_enclosure_and_a_cover_around_a_derived_master_follow_its_edits() {
    let (mut doc, mut h) = td::document().unwrap();
    let b = build(&doc);
    assert_eq!(b.parts.len(), 3, "the master is kept, plus the two sheet metal parts");
    part(&b, "Space Envelope");
    let names: Vec<String> = b
        .sheet_metal
        .iter()
        .map(|c| doc.element(td::STUDIO).unwrap().feature(c.feature).unwrap().name.clone())
        .collect();
    assert_eq!(names, ["Enclosure", "Cover"], "the contexts are named after their features");
    let enc = &b.sheet_metal[0];
    assert_eq!(enc.model.walls.len(), 9, "the T's bottom and its eight sides: the top and the angled face excluded");
    assert_eq!(enc.model.joints.iter().filter(|j| j.bend().is_some()).count(), 8);
    // The notches' inside corners are rips: their walls don't fold onto each other.
    assert!(enc.model.joints.iter().filter(|j| matches!(j.kind, cadrs_sheetmetal::JointKind::Rip { .. })).count() >= 2);
    assert!(enc.flat.is_ok() && enc.flat.parts.len() == 1);
    let cov = &b.sheet_metal[1];
    assert_eq!((cov.model.walls.len(), cov.model.joints.iter().filter(|j| j.bend().is_some()).count()), (2, 1));
    let (ve, vc) = (volume(part(&b, "Enclosure")), volume(part(&b, "Cover")));
    let (fe, fc) = (flat_extent(&b, td::ENCLOSURE), flat_extent(&b, td::COVER));
    // The master 30 mm deeper (wider enclosure), in its own studio: both follow.
    td::set_depth(&mut DocHistory(&mut doc, &mut h), td::DEPTH + 30.0).unwrap();
    let b = build(&doc);
    let (ve2, vc2) = (volume(part(&b, "Enclosure")), volume(part(&b, "Cover")));
    assert!(ve2 > ve + 1.0 && vc2 > vc + 1.0, "{ve} → {ve2}, {vc} → {vc2}");
    let (fe2, fc2) = (flat_extent(&b, td::ENCLOSURE), flat_extent(&b, td::COVER));
    let grew = |a: (f64, f64), b: (f64, f64)| ((b.0 - a.0) - 30.0).abs() < 1e-6 || ((b.1 - a.1) - 30.0).abs() < 1e-6;
    assert!(grew(fe, fe2) && grew(fc, fc2), "{fe:?} → {fe2:?}, {fc:?} → {fc2:?}");
    // The cover's flat grows only along the bend; its sheet by 30 × its flat length × T.
    h.undo(&mut doc);
    let b = build(&doc);
    assert!((volume(part(&b, "Enclosure")) - ve).abs() < 1e-6 * ve);
}

#[test]
fn sheet_metal_parts_are_parts_through_refolds() {
    let (mut doc, mut h) = td::document().unwrap();
    let b = build(&doc);
    let enc = part(&b, "Enclosure").clone();
    // Material: Carbon steel; the mass is its density times the volume.
    let steel = cadrs_core::material::library("Carbon Steel").expect("carbon steel in the library");
    h.execute(&mut doc, &SetPartMaterial { element: td::STUDIO, parts: vec![enc.id], material: Some(steel.clone()) }).unwrap();
    let props = doc.element(td::STUDIO).unwrap().part_props().iter().find(|p| p.part == enc.id).cloned().expect("its properties");
    assert_eq!(props.material.as_ref().map(|m| m.name.as_str()), Some(steel.name.as_str()));
    // A table edit refolds the enclosure: the part keeps its id, its name and its material.
    let ctx = &b.sheet_metal[0];
    let j = ctx.model.joints.iter().find(|j| j.bend().is_some()).unwrap().clone();
    let x = table_edit(td::ENCLOSURE, None, &j, &ctx.model.params, &TableEdit::Radius(4.0, "4 mm".into()));
    h.execute(&mut doc, &PutModifyJoint { element: td::STUDIO, feature: FeatureId::new(), joint: x, after: ctx.editors.clone(), label: "Edit radius".into() }).unwrap();
    let b = build(&doc);
    let after = part(&b, "Enclosure");
    assert_eq!(after.id, enc.id);
    assert!((volume(after) - volume(&enc)).abs() > 1e-3, "the radius changed the part");
    let props = doc.element(td::STUDIO).unwrap().part_props().iter().find(|p| p.part == enc.id).cloned().unwrap();
    assert!(props.material.is_some());
    // It exports like any part.
    let features = Arc::new(doc.element(td::STUDIO).unwrap().active_features());
    let item = ExportItem { features, part: enc.id, name: "Enclosure".into(), pose: None, source: (td::STUDIO, enc.id), source_name: "Enclosure".into() };
    let files = Rebuilder::new().export_files(&ExportRequest::new(ModelFormat::Step, "Enclosure", vec![item])).unwrap();
    let text = String::from_utf8_lossy(&files[0].bytes);
    assert!(text.contains("MANIFOLD_SOLID_BREP") || text.contains("BREP_WITH_VOIDS"), "a solid in the STEP file");
}

#[test]
fn a_derived_sheet_metal_part_brings_its_model_along() {
    use cadrs_core::derived::{AddDerived, DerivedFeature, DerivedSelection};
    use cadrs_core::document::Element;
    use cadrs_core::external::{RefAt, SourceRef};
    let (mut doc, mut h) = td::document().unwrap();
    let el = Element::part_studio("Part Studio 2");
    let host = el.id;
    doc.elements.push(el);
    let mut d = DerivedFeature { selection: DerivedSelection { all: false, parts: vec![td::ENCLOSURE_PART], ..Default::default() }, ..Default::default() };
    d.fill_from(doc.element(td::STUDIO).unwrap(), "", "");
    d.source = Some(SourceRef { document: None, at: RefAt::Workspace, element: td::STUDIO, pinned: false });
    let feature = FeatureId::new();
    h.execute(&mut doc, &AddDerived { element: host, feature, derived: d, links: Vec::new() }).unwrap();
    let b = cadrs_core::rebuild::build(&doc.element(host).unwrap().active_features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 1);
    let props = doc.element(host).unwrap().part_props().to_vec();
    assert_eq!(cadrs_core::parts::display_name(&b.parts[0], &props), "Enclosure", "the source's rename comes along");
    // The Enclosure's model came along, named after it, with its flat and its part.
    assert_eq!(b.sheet_metal.len(), 1);
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.name, "Enclosure");
    assert!(ctx.active, "at its own place it stays active sheet metal");
    assert_eq!(ctx.parts[0].0, b.parts[0].id);
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
    let src = build(&doc);
    assert_eq!(src.sheet_metal[0].flat.parts[0].bounds(), ctx.flat.parts[0].bounds());
    // A table edit in the host refolds the derived part in place.
    let j = ctx.model.joints.iter().find(|j| j.bend().is_some()).unwrap().clone();
    let x = table_edit(ctx.feature, None, &j, &ctx.model.params, &TableEdit::Radius(4.0, "4 mm".into()));
    h.execute(&mut doc, &PutModifyJoint { element: host, feature: FeatureId::new(), joint: x, after: ctx.editors.clone(), label: "Edit radius".into() }).unwrap();
    let b2 = cadrs_core::rebuild::build(&doc.element(host).unwrap().active_features());
    assert!(b2.errors.is_empty(), "{:?}", b2.errors);
    assert_eq!(b2.parts[0].id, b.parts[0].id);
    assert!((volume(&b2.parts[0]) - volume(&b.parts[0])).abs() > 1e-3);
}
