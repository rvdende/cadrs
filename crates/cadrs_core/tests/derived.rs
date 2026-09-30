//! P3G.4: the Derived feature (`derived-and-linking-gaps.md` P3G.4 "Done when"; DV1.5,
//! DV3.1–DV3.8; ex-dv1 and ex-dv3 values).
//!
//! The source is the linked-documents block ([`cadrs_core::samples::linked_block`]): a
//! 50 × 30 × 25 box with its corner at the origin, so per copy V = 37 500 mm³, area
//! 2(1500 + 1250 + 750) = 7 000 mm², centroid (25, 15, 12.5), and about its own centroid
//! Ixx = V(30² + 25²)/12, Iyy = V(50² + 25²)/12, Izz = V(50² + 30²)/12 (unit density). Every
//! expected value is computed here from the dimensions, not read back from the model.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::command::Command;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, DuplicateElement, EditSketch, SetExtrude};
use cadrs_core::derived::{self, AddDerived, DerivedFeature, DerivedPlacement, DerivedSelection, SetDerived};
use cadrs_core::document::{BooleanOp, EdgeRef, Element, ExtrudeFeature, Feature, FeatureKind};
use cadrs_core::external::{LinkError, Resolver, SourceRef};
use cadrs_core::history_log::{HistoryLog, Origin, VersionId};
use cadrs_core::link_update::{self, RefSite, Target, UpdateReferences};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use cadrs_core::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::linked_block as lb;
use cadrs_core::{Document, DocumentMeta, ElementId, FeatureId, History, Part, Store};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

const V: f64 = lb::LENGTH * lb::WIDTH * lb::HEIGHT;
const AREA: f64 = 2.0 * (lb::LENGTH * lb::WIDTH + lb::LENGTH * lb::HEIGHT + lb::WIDTH * lb::HEIGHT);

/// The source's extra sketch: a 20 × 10 rectangle at (0, 40)–(20, 50), not extruded.
const RECT: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0041);
/// The source's mate connector at the block's top-face centre (25, 15, 25), owned by the block.
const TOP: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0042);
/// The host's mate connector at (0, 0, 100).
const LOC: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0043);
const HOST_DOC: cadrs_core::DocumentId = cadrs_core::DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0500);
const HOST: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0501);
const DERIVED: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0502);

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-derived-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

#[track_caller]
fn close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-6 * want.abs().max(1.0), "{what}: got {got}, want {want}");
}

fn run(doc: &mut Document, h: &mut History, c: &dyn Command) {
    h.execute(doc, c).unwrap_or_else(|e| panic!("{}: {e}", c.label()));
}

/// A mate connector at `offset` from the origin (Part Studio axes), owned by `owner`.
fn connector(offset: [f64; 3], owner: Option<cadrs_core::PartId>) -> FeatureKind {
    FeatureKind::MateConnector(MateConnectorFeature {
        origin: Some(ConnectorOrigin::Origin),
        offset,
        offset_expr: offset.map(|x| format!("{x} mm")),
        owner_on: owner.is_some(),
        owner,
        ..MateConnectorFeature::default()
    })
}

/// A sketch on Top in `el` with the closed polyline `pts` (or a circle), returning its id.
fn sketch(doc: &mut Document, h: &mut History, el: ElementId, id: FeatureId, op: SketchOp) {
    run(doc, h, &AddSketch { element: el, feature: id, plane: Some(PlaneRef::Top) });
    run(doc, h, &EditSketch { element: el, feature: id, op });
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

/// Document A with the extra sketch and the top connector.
fn source() -> Document {
    let mut a = lb::document().unwrap();
    let mut h = History::default();
    sketch(&mut a, &mut h, lb::STUDIO, RECT, rect(0.0, 40.0, 20.0, 50.0));
    run(&mut a, &mut h, &AddFeature { element: lb::STUDIO, feature: TOP, base_name: "Mate connector".into(), kind: connector([25.0, 15.0, 25.0], Some(lb::PART)) });
    a
}

/// Saves `doc` with a history whose only version is "V1".
fn save_with_v1(store: &Store, doc: &Document) -> VersionId {
    store.create(doc, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(doc, 1_000, "me");
    let v = log.create_version("", "", 1_001, "me");
    log.save(store).unwrap();
    v
}

/// Edits the stored A's depth and makes the next version.
fn edit_source(store: &Store, depth: f64) -> VersionId {
    let file = store.load(lb::DOCUMENT).unwrap();
    let mut doc = file.document;
    let mut h = History::default();
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::STUDIO, depth).unwrap();
    store.save(&doc, &file.meta).unwrap();
    let mut log = HistoryLog::load(store, lb::DOCUMENT).unwrap().unwrap();
    log.record(&doc, Origin::Command("Edit Extrude 1".into()), 2_000, "me");
    let v = log.create_version("", "", 2_001, "me");
    log.save(store).unwrap();
    v
}

/// Document B: "Block derived" with "Part Studio 1" holding a mate connector at (0, 0, 100).
fn host() -> (Document, History) {
    let mut b = Document::empty("Block derived");
    b.id = HOST_DOC;
    let mut el = Element::part_studio("Part Studio 1");
    el.id = HOST;
    b.elements.push(el);
    let mut h = History::default();
    run(&mut b, &mut h, &AddFeature { element: HOST, feature: LOC, base_name: "Mate connector".into(), kind: connector([0.0, 0.0, 100.0], None) });
    (b, h)
}

fn build(doc: &Document, el: ElementId) -> std::sync::Arc<cadrs_core::rebuild::Build> {
    cadrs_core::rebuild::build(doc.element(el).unwrap().features())
}

/// Derives A's Block at `v` into B with `d`'s options.
fn derive(store: &Store, b: &mut Document, h: &mut History, v: VersionId, d: DerivedFeature) -> Result<(), cadrs_core::CommandError> {
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v);
    let got = derived::resolve(&mut res, b, None, d, r).map_err(cadrs_core::CommandError::from)?;
    h.execute(b, &AddDerived { element: HOST, feature: DERIVED, derived: got.derived, links: got.links })
}

fn parts_of(build: &cadrs_core::rebuild::Build, feature: FeatureId) -> Vec<&Part> {
    build.parts.iter().filter(|p| p.feature == feature).collect()
}

fn total(parts: &[&Part]) -> cadrs_kernel::MassProperties {
    cadrs_core::parts::combined_mass(parts.iter().copied()).unwrap()
}

#[test]
fn ex_dv1_two_copies_at_the_origin_and_at_a_connector() {
    let store = temp_store("dv1");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    let d = DerivedFeature {
        selection: DerivedSelection { all: false, parts: vec![lb::PART], ..DerivedSelection::default() },
        locations: vec![ConnectorRef::Implicit(ConnectorOrigin::Origin), ConnectorRef::Feature(LOC)],
        ..DerivedFeature::default()
    };
    derive(&store, &mut b, &mut h, v1, d).unwrap();
    let f = b.element(HOST).unwrap().feature(DERIVED).unwrap();
    assert_eq!(f.name, "Derived 1");
    let build = build(&b, HOST);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let parts = parts_of(&build, DERIVED);
    assert_eq!(parts.len(), 2, "two copies");
    let m = total(&parts);
    close("volume", m.volume, 2.0 * V);
    close("area", m.surface_area, 2.0 * AREA);
    // The copies sit at z 0..25 and 100..125: the centroid z is 12.5 + 50.
    close("centroid x", m.center_of_mass.x, lb::LENGTH / 2.0);
    close("centroid y", m.center_of_mass.y, lb::WIDTH / 2.0);
    close("centroid z", m.center_of_mass.z, lb::HEIGHT / 2.0 + 50.0);
    // Inertia about the combined centroid: each copy's own plus V·50² (they sit ±50 in z).
    let c = m.center_of_mass;
    let i = parts.iter().map(|p| p.mass.unwrap().inertia_about(c)).fold(nalgebra::Matrix3::zeros(), |a, b| a + b);
    let own_x = V * (lb::WIDTH.powi(2) + lb::HEIGHT.powi(2)) / 12.0;
    let own_y = V * (lb::LENGTH.powi(2) + lb::HEIGHT.powi(2)) / 12.0;
    let own_z = V * (lb::LENGTH.powi(2) + lb::WIDTH.powi(2)) / 12.0;
    close("Ixx", i[(0, 0)], 2.0 * own_x + 2.0 * V * 50.0f64.powi(2));
    close("Iyy", i[(1, 1)], 2.0 * own_y + 2.0 * V * 50.0f64.powi(2));
    close("Izz", i[(2, 2)], 2.0 * own_z);
    close("Ixx value", i[(0, 0)], 197_031_250.0);
    close("Iyy value", i[(1, 1)], 207_031_250.0);
    close("Izz value", i[(2, 2)], 21_250_000.0);
    for (r, cc) in [(0, 1), (0, 2), (1, 2)] {
        assert!(i[(r, cc)].abs() < 1e-3, "product {r}{cc}: {}", i[(r, cc)]);
    }
    // Names: the source's, unique in the host; the host's own numbering carries on after them.
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Part 1", "Part 1 (2)"]);
    let props = b.element(HOST).unwrap().part_props();
    assert!(parts.iter().all(|p| cadrs_core::parts::part_material(p, props).is_some_and(|m| m.name == "Aluminum - 6061")), "the source's material comes over");
    let g = b.element(HOST).unwrap().feature(DERIVED).unwrap().clone();
    let s = FeatureId::new();
    sketch(&mut b, &mut h, HOST, s, rect(200.0, 0.0, 210.0, 10.0));
    let geom = b.element(HOST).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let e = FeatureId::new();
    run(&mut b, &mut h, &AddExtrude { element: HOST, feature: e, extrude: ExtrudeFeature::default() });
    let regions = cadrs_core::samples::region_refs(s, &geom, &[Vec2::new(205.0, 5.0)]);
    run(&mut b, &mut h, &SetExtrude { element: HOST, feature: e, extrude: cadrs_core::samples::extrude_of(regions, 10.0), label: "Extrude".into() });
    let build = cadrs_core::rebuild::build(b.element(HOST).unwrap().features());
    let new = build.parts.iter().find(|p| p.feature == e).unwrap();
    assert_eq!(new.name, "Part 2", "no clash with the derived Part 1");
    // The linked copy is the document's, the feature uses it.
    assert_eq!(b.linked.len(), 1);
    let FeatureKind::Derived(dd) = &g.kind else { unreachable!() };
    assert_eq!(dd.copy, Some(b.linked[0].id()));
    assert_eq!(dd.describe(), "Block source › Block (V1)");
}

#[test]
fn base_mate_connector_placement_puts_the_top_centre_on_the_location() {
    // The source's connector at the top-face centre (25, 15, 25) goes onto (0, 0, 100): the
    // block moves by (−25, −15, 75), its centroid (25, 15, 12.5) to (0, 0, 87.5).
    let store = temp_store("base");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    let d = DerivedFeature {
        selection: DerivedSelection { all: false, parts: vec![lb::PART], ..DerivedSelection::default() },
        locations: vec![ConnectorRef::Feature(LOC)],
        placement: DerivedPlacement::BaseConnector(Some(ConnectorRef::Feature(TOP))),
        include_connectors: true,
        ..DerivedFeature::default()
    };
    derive(&store, &mut b, &mut h, v1, d).unwrap();
    let build = build(&b, HOST);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let parts = parts_of(&build, DERIVED);
    assert_eq!(parts.len(), 1);
    let m = parts[0].mass.unwrap();
    close("volume", m.volume, V);
    close("x", m.center_of_mass.x, 0.0);
    close("y", m.center_of_mass.y, 0.0);
    close("z", m.center_of_mass.z, 100.0 - lb::HEIGHT + lb::HEIGHT / 2.0);
    close("z value", m.center_of_mass.z, 87.5);
    // Include mate connectors: the top connector comes with the part, at (0, 0, 100).
    let out = &build.derived[&DERIVED];
    assert_eq!(out.connectors.len(), 1);
    let c = build.connectors[&out.connectors[0].0];
    close("connector z", c.origin[2], 100.0);
    assert_eq!(parts[0].solid.connectors.len(), 1, "the connector travels with the derived part");
}

#[test]
fn a_hole_cut_into_a_derived_part() {
    // A Ø10 hole through the derived block: 37 500 − π·5²·25 = 35 536.505.
    let store = temp_store("hole");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    derive(&store, &mut b, &mut h, v1, DerivedFeature::default()).unwrap();
    let s = FeatureId::new();
    sketch(&mut b, &mut h, HOST, s, SketchOp::AddCircle { center: Vec2::new(25.0, 15.0), radius: 5.0, construction: false });
    let geom = b.element(HOST).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let regions = cadrs_core::samples::region_refs(s, &geom, &[Vec2::new(25.0, 15.0)]);
    let e = FeatureId::new();
    run(&mut b, &mut h, &AddExtrude { element: HOST, feature: e, extrude: ExtrudeFeature::default() });
    let x = ExtrudeFeature { op: BooleanOp::Remove, ..cadrs_core::samples::extrude_of(regions, 40.0) };
    run(&mut b, &mut h, &SetExtrude { element: HOST, feature: e, extrude: x, label: "Extrude".into() });
    let build = build(&b, HOST);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let parts = parts_of(&build, DERIVED);
    assert_eq!(parts.len(), 1);
    close("volume", parts[0].mass.unwrap().volume, V - PI * 25.0 * lb::HEIGHT);
    close("volume value", parts[0].mass.unwrap().volume, 35_536.504_591);
}

#[test]
fn a_derived_sketch_extrudes() {
    // The source's 20 × 10 rectangle, derived at (0, 0, 100) and extruded 5: 1 000 mm³ with its
    // centroid (10, 45, 102.5).
    let store = temp_store("sketch");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    let d = DerivedFeature {
        selection: DerivedSelection { all: false, sketches: vec![RECT], ..DerivedSelection::default() },
        locations: vec![ConnectorRef::Feature(LOC)],
        ..DerivedFeature::default()
    };
    derive(&store, &mut b, &mut h, v1, d).unwrap();
    let build0 = build(&b, HOST);
    assert!(build0.errors.is_empty(), "{:?}", build0.errors);
    assert!(parts_of(&build0, DERIVED).is_empty(), "only the sketch");
    let out = &build0.derived[&DERIVED];
    assert_eq!(out.sketches.len(), 1);
    let sid = out.sketches[0].0;
    assert_ne!(sid, RECT, "a derived sketch has an id of its own");
    let dsk = build0.derived_sketches.iter().find(|f| f.id == sid).unwrap();
    let regions = cadrs_core::samples::region_refs(sid, &dsk.sketch().unwrap().geometry, &[Vec2::new(10.0, 45.0)]);
    assert_eq!(regions.len(), 1);
    let e = FeatureId::new();
    run(&mut b, &mut h, &AddExtrude { element: HOST, feature: e, extrude: ExtrudeFeature::default() });
    run(&mut b, &mut h, &SetExtrude { element: HOST, feature: e, extrude: cadrs_core::samples::extrude_of(regions, 5.0), label: "Extrude".into() });
    let build = build(&b, HOST);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let p = build.parts.iter().find(|p| p.feature == e).unwrap();
    let m = p.mass.unwrap();
    close("volume", m.volume, 20.0 * 10.0 * 5.0);
    close("x", m.center_of_mass.x, 10.0);
    close("y", m.center_of_mass.y, 45.0);
    close("z", m.center_of_mass.z, 102.5);
}

#[test]
fn include_properties_off_copies_only_the_name_material_and_appearance() {
    let store = temp_store("props");
    let mut a = source();
    let mut ha = History::default();
    let owner = PropertyOwner::Part { element: lb::STUDIO, part: lb::PART };
    run(
        &mut a,
        &mut ha,
        &SetProperties {
            owners: vec![owner],
            values: vec![
                (PropertyKey::Name, PropertyValue::Text("Block body".into())),
                (PropertyKey::PartNumber, PropertyValue::Text("BLK-001".into())),
                (PropertyKey::Description, PropertyValue::Text("The block".into())),
                (PropertyKey::Appearance, PropertyValue::Appearance(Some(cadrs_core::Appearance::rgb(200, 30, 30)))),
            ],
            label: "Properties".into(),
        },
    );
    let v1 = save_with_v1(&store, &a);
    for include in [true, false] {
        let (mut b, mut h) = host();
        let d = DerivedFeature { include_properties: include, ..DerivedFeature::default() };
        derive(&store, &mut b, &mut h, v1, d).unwrap();
        let build = build(&b, HOST);
        let parts = parts_of(&build, DERIVED);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "Block body", "the name comes over either way");
        let el = b.element(HOST).unwrap();
        let pp = el.part_prop(parts[0].id).expect("the derived part's settings");
        assert_eq!(pp.material.as_ref().map(|m| m.name.as_str()), Some("Aluminum - 6061"));
        assert_eq!(pp.appearance, Some(cadrs_core::Appearance::rgb(200, 30, 30)));
        if include {
            assert_eq!(pp.properties.part_number.as_deref(), Some("BLK-001"));
            assert_eq!(pp.properties.description.as_deref(), Some("The block"));
        } else {
            assert_eq!(pp.properties.part_number, None);
            assert_eq!(pp.properties.description, None);
        }
    }
}

#[test]
fn deriving_one_studio_twice_or_itself_is_refused() {
    let store = temp_store("twice");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    derive(&store, &mut b, &mut h, v1, DerivedFeature::default()).unwrap();
    let mut res = Resolver::new(store.clone());
    let got = derived::resolve(&mut res, &b, None, DerivedFeature::default(), SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1)).unwrap();
    let undo = h.undo_len();
    let e = h.execute(&mut b, &AddDerived { element: HOST, feature: FeatureId::new(), derived: got.derived, links: got.links }).unwrap_err();
    assert!(e.to_string().contains("is derived already in Part Studio 1 by Derived 1"), "{e}");
    assert_eq!(h.undo_len(), undo, "nothing added");
    // A Part Studio deriving itself.
    let mut a = lb::document().unwrap();
    let mut ha = History::default();
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), SourceRef { document: None, at: cadrs_core::external::RefAt::Workspace, element: lb::STUDIO, pinned: false }).unwrap();
    let e = ha.execute(&mut a, &AddDerived { element: lb::STUDIO, feature: FeatureId::new(), derived: got.derived, links: got.links }).unwrap_err();
    assert_eq!(e.to_string(), "Block can't derive itself");
}

#[test]
fn ex_dv3_a_workspace_derive_follows_the_source() {
    // Part Studio 2 derives "Block" at the workspace: 37 500; the block's depth 25 → 30 makes
    // it 50·30·30 = 45 000 at once (no update); undo brings 37 500 back.
    let mut a = lb::document().unwrap();
    let mut h = History::default();
    let ps2 = Element::part_studio("Part Studio 2");
    let ps2_id = ps2.id;
    a.elements.push(ps2);
    let mut res = Resolver::new(temp_store("ws"));
    let r = SourceRef { document: None, at: cadrs_core::external::RefAt::Workspace, element: lb::STUDIO, pinned: false };
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), r).unwrap();
    assert!(got.links.is_empty(), "a workspace reference needs no copy");
    run(&mut a, &mut h, &AddDerived { element: ps2_id, feature: DERIVED, derived: got.derived, links: got.links });
    let vol = |a: &Document| total(&parts_of(&build(a, ps2_id), DERIVED)).volume;
    close("before", vol(&a), V);
    lb::set_height(&mut DocHistory(&mut a, &mut h), lb::STUDIO, 30.0).unwrap();
    close("after the edit", vol(&a), lb::LENGTH * lb::WIDTH * 30.0);
    close("after the edit value", vol(&a), 45_000.0);
    h.undo(&mut a);
    close("undone", vol(&a), V);
    h.redo(&mut a);
    close("redone", vol(&a), 45_000.0);
    // Where it's used: a workspace use of this document, no version reference.
    assert!(link_update::uses(&a).is_empty());
    let ws = link_update::workspace_uses(&a, Some(ps2_id));
    assert_eq!(ws.len(), 1);
    assert_eq!(ws[0].site, RefSite::Derived { element: ps2_id, feature: DERIVED });
}

#[test]
fn derive_cycles_are_refused() {
    // In one document: Part Studio 2 derives Block; Block deriving Part Studio 2 is refused.
    let mut a = lb::document().unwrap();
    let mut h = History::default();
    let ps2 = Element::part_studio("Part Studio 2");
    let ps2_id = ps2.id;
    a.elements.push(ps2);
    let mut res = Resolver::new(temp_store("cyc"));
    let ws = |e: ElementId| SourceRef { document: None, at: cadrs_core::external::RefAt::Workspace, element: e, pinned: false };
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), ws(lb::STUDIO)).unwrap();
    run(&mut a, &mut h, &AddDerived { element: ps2_id, feature: FeatureId::new(), derived: got.derived, links: got.links });
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), ws(ps2_id)).unwrap();
    let e = h.execute(&mut a, &AddDerived { element: lb::STUDIO, feature: FeatureId::new(), derived: got.derived, links: got.links }).unwrap_err();
    assert_eq!(e.to_string(), "Circular reference: Block source › Block → Block source › Part Studio 2 → Block source");
    // Across documents (ex-dv5's shape): B derives A's Block at V1 and is versioned; A's Block
    // deriving B's Part Studio 1 at that version is refused, and nothing is added.
    let store = temp_store("cyc2");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut hb) = host();
    derive(&store, &mut b, &mut hb, v1, DerivedFeature::default()).unwrap();
    let bv1 = save_with_v1(&store, &b);
    let mut a = store.load(lb::DOCUMENT).unwrap().document;
    let mut ha = History::default();
    let mut res = Resolver::new(store.clone());
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), SourceRef::version(Some(HOST_DOC), HOST, bv1)).unwrap();
    let e = ha.execute(&mut a, &AddDerived { element: lb::STUDIO, feature: FeatureId::new(), derived: got.derived, links: got.links }).unwrap_err();
    assert_eq!(e.to_string(), "Circular reference: Block source › Block → Block derived › Part Studio 1 → Block source");
    assert!(a.linked.is_empty());
    assert!(matches!(LinkError::Circular(String::new()), LinkError::Circular(_)));
}

#[test]
fn update_to_a_new_version_brings_the_new_volume_and_undo_restores_it() {
    // A@V1 (25 high) derived, then A edited to 40 and versioned V2: B still reads 37 500 and
    // the use is out of date; Update makes it 50·30·40 = 60 000; undo brings 37 500 back.
    let store = temp_store("update");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    derive(&store, &mut b, &mut h, v1, DerivedFeature::default()).unwrap();
    let v2 = edit_source(&store, lb::EDITED_HEIGHT);
    let vol = |b: &Document| total(&parts_of(&build(b, HOST), DERIVED)).volume;
    close("unchanged", vol(&b), V);
    let uses = link_update::uses(&b);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].site, RefSite::Derived { element: HOST, feature: DERIVED });
    let mut res = Resolver::new(store.clone());
    let st = link_update::staleness(&b, &uses[0], &mut |d| res.latest(d).map(|v| (v.id(), v.name().to_string())));
    assert_eq!(st.newer.as_ref().map(|x| x.0), Some(v2));
    let c = link_update::change_for(&mut res, &b, None, &uses[0], Target::Latest).unwrap().unwrap();
    run(&mut b, &mut h, &UpdateReferences { changes: vec![c], label: "Update".into() });
    close("updated", vol(&b), lb::LENGTH * lb::WIDTH * lb::EDITED_HEIGHT);
    assert_eq!(b.linked.len(), 1, "the V1 copy is dropped");
    assert_eq!(derived::derived_of(&b, HOST, DERIVED).unwrap().version_name, "V2");
    h.undo(&mut b);
    close("undone", vol(&b), V);
    // Pin, where used.
    run(&mut b, &mut h, &link_update::SetPinned { sites: vec![RefSite::Derived { element: HOST, feature: DERIVED }], pinned: true });
    assert!(derived::derived_of(&b, HOST, DERIVED).unwrap().source.unwrap().pinned);
    let usages = link_update::usages_in(&b, lb::DOCUMENT);
    assert_eq!(usages.len(), 1);
    assert_eq!((usages[0].tab.as_str(), usages[0].element.as_str(), usages[0].version.as_str()), ("Part Studio 1", "Block", "V1"));
}

#[test]
fn a_fillet_on_a_derived_edge_of_a_duplicated_studio_survives_a_source_edit() {
    // "Block" and its duplicate "Block 2" share their features' uuids. "Block 2" derives
    // "Block" at the workspace onto (0, 0, 100); its derived part and faces have names of their
    // own. An R2 fillet on the derived top edge along x (y 0, z 125) removes (1 − π/4)·2²·50.
    // Block's depth 25 → 30: the fillet still finds its edge (now at z 130).
    let mut a = lb::document().unwrap();
    let mut h = History::default();
    let dup = ElementId::new();
    run(&mut a, &mut h, &DuplicateElement { source: lb::STUDIO, id: dup });
    run(&mut a, &mut h, &AddFeature { element: dup, feature: LOC, base_name: "Mate connector".into(), kind: connector([0.0, 0.0, 100.0], None) });
    let mut res = Resolver::new(temp_store("dup"));
    let r = SourceRef { document: None, at: cadrs_core::external::RefAt::Workspace, element: lb::STUDIO, pinned: false };
    let d = DerivedFeature { locations: vec![ConnectorRef::Feature(LOC)], ..DerivedFeature::default() };
    let got = derived::resolve(&mut res, &a, None, d, r).unwrap();
    run(&mut a, &mut h, &AddDerived { element: dup, feature: DERIVED, derived: got.derived, links: got.links });
    let b0 = build(&a, dup);
    assert!(b0.errors.is_empty(), "{:?}", b0.errors);
    let own = b0.parts.iter().find(|p| p.id == lb::PART).expect("the duplicate's own block").clone();
    let der = parts_of(&b0, DERIVED)[0].clone();
    assert_ne!(own.id, der.id);
    assert!(der.solid.faces.iter().all(|f| own.solid.faces.iter().all(|g| g.name != f.name)), "derived faces are named apart");
    let p = [25.0, 0.0, 125.0];
    let e = der.solid.edges.iter().min_by(|x, y| x.distance(p).total_cmp(&y.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3);
    let fillet = cadrs_core::applied::FilletFeature {
        entities: vec![cadrs_core::applied::EdgeOrFace::Edge(EdgeRef { part: der.id, edge: e.name, seed: p })],
        size: 2.0,
        size_expr: "2 mm".into(),
        ..Default::default()
    };
    let fid = FeatureId::new();
    run(&mut a, &mut h, &AddFeature::fillet(dup, fid, fillet));
    let cut = (1.0 - PI / 4.0) * 4.0 * lb::LENGTH;
    let vol = |a: &Document| {
        let b = build(a, dup);
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b.parts.iter().find(|p| p.id == der.id).unwrap().mass.unwrap().volume
    };
    close("filleted", vol(&a), V - cut);
    lb::set_height(&mut DocHistory(&mut a, &mut h), lb::STUDIO, 30.0).unwrap();
    close("after the source edit", vol(&a), lb::LENGTH * lb::WIDTH * 30.0 - cut);
    // The duplicate's own block is untouched by the edit of "Block".
    let own_after = build(&a, dup).parts.iter().find(|p| p.id == lb::PART).unwrap().mass.unwrap().volume;
    close("own block", own_after, V);
}

#[test]
fn editing_a_derived_feature_swaps_what_it_brings() {
    // SetDerived (the dialog's edits): from the whole studio to the sketch only, then back.
    let store = temp_store("edit");
    let v1 = save_with_v1(&store, &source());
    let (mut b, mut h) = host();
    derive(&store, &mut b, &mut h, v1, DerivedFeature::default()).unwrap();
    let mut d = derived::derived_of(&b, HOST, DERIVED).unwrap().clone();
    d.selection = DerivedSelection { all: false, sketches: vec![RECT], ..DerivedSelection::default() };
    run(&mut b, &mut h, &SetDerived { element: HOST, feature: DERIVED, derived: d, links: Vec::new(), label: "Edit".into() });
    let build1 = build(&b, HOST);
    assert!(parts_of(&build1, DERIVED).is_empty());
    assert_eq!(build1.derived[&DERIVED].sketches.len(), 1);
    h.undo(&mut b);
    let build2 = build(&b, HOST);
    assert_eq!(parts_of(&build2, DERIVED).len(), 1);
    // The whole studio also brings the extra sketch.
    assert_eq!(build2.derived[&DERIVED].sketches.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), ["Sketch 1", "Sketch 2"]);
    let _: Option<&Feature> = None;
}
