//! P3G.5: the exercises of stage 3G on their stand-ins (`derived-and-linking-gaps.md` "What each
//! exercise needs"): ex-dv1–ex-dv5 (derived-and-linking.md) and ER Ex1 (the Hexapod) and ER Ex2
//! (Move to document) of the Linked Documents course, and the stand-in fixtures.
//!
//! Every expected value is computed here from the stand-ins' dimensions, written out again below
//! (closed forms), not read back from the model or from the samples' constants:
//!
//! - the block: 50 × 30 × 25 with its corner at the origin (V = l·w·h, area 2(lw + lh + wh),
//!   centroid (l/2, w/2, h/2), inertia about its centroid V(b² + c²)/12);
//! - the base plate: 100 × 100 × 10 (x, y 0..100, z 0..10);
//! - the piston, on its axis: a 10 mm cube (z −10..0), a Ø D × 60 cylinder (z 0..60), a Ø6 × L
//!   rod (z 60..60 + L) and an 8 mm cube eye on top (z 60 + L..68 + L); V1 D 15.725, L 28; V2
//!   D 20, L 75;
//! - the plates: Ø200 and Ø160 × 10 discs less six Ø10 holes, V = π(R² − 6·5²)·10;
//! - the Hexapod: the base plate at z 0..10, each piston raised 20 (its UJoint on a hole's top
//!   edge), the top plate on the six eyes (z 88 + L..98 + L); every part centred on its axis and
//!   the pistons 6-fold symmetric about Z, so x̄ = ȳ = 0;
//! - Aluminum 6061: 2.70 g/cm³ = 0.0027 g/mm³.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose, structure};
use cadrs_core::command::Command;
use cadrs_core::derived::{self, AddDerived, DerivedFeature};
use cadrs_core::document::Element;
use cadrs_core::external::{InsertLinked, RefAt, Resolver, SourceRef};
use cadrs_core::history_log::{HistoryLog, Origin, VersionId};
use cadrs_core::link_update::{self as lu, Target, UpdateReferences};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::linked_block as lb;
use cadrs_core::samples::piston as ps;
use cadrs_core::{Document, DocumentId, DocumentMeta, ElementId, FeatureId, History, Part, Store};

/// Aluminum 6061, g/mm³.
const AL: f64 = 0.0027;

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-course-linked-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

#[track_caller]
fn close(what: &str, got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() < tol, "{what}: got {got}, want {want}");
}

fn run(doc: &mut Document, h: &mut History, c: &dyn Command) {
    h.execute(doc, c).unwrap_or_else(|e| panic!("{}: {e}", c.label()));
}

// ---------------------------------------------------------------------------------------------
// Fixtures

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Checks (or, with `CADRS_REGENERATE_FIXTURES=1`, writes) `fixtures/<name>.cadrs` and, when
/// given, its history `fixtures/<name>.history.ron`.
fn check_fixture(name: &str, doc: Document, log: Option<HistoryLog>) {
    let file = lb::file(doc);
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    let path = fixtures().join(format!("{name}.cadrs"));
    let log_path = fixtures().join(format!("{name}.history.ron"));
    if std::env::var_os("CADRS_REGENERATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
        if let Some(l) = &log {
            l.save_path(&log_path).unwrap();
        }
    }
    let on_disk = cadrs_core::Store::load_path(&path).unwrap_or_else(|e| panic!("fixtures/{name}.cadrs: {e}"));
    assert_eq!(on_disk, file, "fixtures/{name}.cadrs is stale (CADRS_REGENERATE_FIXTURES=1)");
    if let Some(l) = log {
        let back = HistoryLog::load_path(&log_path).unwrap_or_else(|e| panic!("fixtures/{name}.history.ron: {e}"));
        assert_eq!(back.entries, l.entries, "fixtures/{name}.history.ron is stale");
        let names = |x: &HistoryLog| x.versions().iter().map(|v| (v.id(), v.name().to_string())).collect::<Vec<_>>();
        assert_eq!(names(&back), names(&l));
        assert_eq!(back.state_at(back.head_index()).unwrap(), file.document);
    }
}

#[test]
fn linked_block_fixture_is_current() {
    let a = lb::source_document().unwrap();
    let log = lb::history_with_v1(&a, lb::SOURCE_V1, "The block, 50 × 30 × 25");
    check_fixture("linked_block_standin", a, Some(log));
}

#[test]
fn linked_block_host_fixture_is_current() {
    check_fixture("linked_block_host_standin", lb::host_document().unwrap(), None);
}

#[test]
fn linked_base_fixture_is_current() {
    check_fixture("linked_base_standin", lb::base_document().unwrap(), None);
}

#[test]
fn linked_piston_fixture_is_current() {
    check_fixture("linked_piston_standin", ps::piston_document().unwrap(), None);
}

#[test]
fn linked_hexapod_fixture_is_current() {
    check_fixture("linked_hexapod_standin", ps::project_document().unwrap(), None);
}

#[test]
fn move_to_document_fixture_is_current() {
    check_fixture("move_to_document_standin", ps::move_to_document().unwrap(), None);
}

// ---------------------------------------------------------------------------------------------
// ex-dv1 – ex-dv5

/// The block's dimensions, written out again.
const L: f64 = 50.0;
const W: f64 = 30.0;
const H: f64 = 25.0;

fn block(l: f64, w: f64, h: f64) -> (f64, f64) {
    (l * w * h, 2.0 * (l * w + l * h + w * h))
}

fn save_a(store: &Store) -> VersionId {
    let a = lb::source_document().unwrap();
    store.create(&a, &DocumentMeta::new("me", 1_000)).unwrap();
    let log = lb::history_with_v1(&a, lb::SOURCE_V1, "");
    log.save(store).unwrap();
    lb::SOURCE_V1
}

/// Edits the stored A with `edit` and makes its next version.
fn edit_a(store: &Store, t: i64, edit: impl FnOnce(&mut DocHistory)) -> VersionId {
    let file = store.load(lb::DOCUMENT).unwrap();
    let mut doc = file.document;
    let mut h = History::default();
    edit(&mut DocHistory(&mut doc, &mut h));
    store.save(&doc, &file.meta).unwrap();
    let mut log = HistoryLog::load(store, lb::DOCUMENT).unwrap().unwrap();
    log.record(&doc, Origin::Command("Edit".into()), t, "me");
    let v = log.create_version("", "", t + 1, "me");
    log.save(store).unwrap();
    v
}

const DERIVED: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0b01);

/// B with Derived 1 of A's Block at `v`, at the origin and at B's Mate connector 1 (0, 0, 100).
fn ex_dv1(store: &Store, v: VersionId) -> (Document, History) {
    let mut b = lb::host_document().unwrap();
    let mut h = History::default();
    let d = DerivedFeature { locations: vec![ConnectorRef::Implicit(ConnectorOrigin::Origin), ConnectorRef::Feature(lb::HOST_CONNECTOR)], ..DerivedFeature::default() };
    let mut res = Resolver::new(store.clone());
    let got = derived::resolve(&mut res, &b, None, d, SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v)).unwrap();
    run(&mut b, &mut h, &AddDerived { element: lb::HOST_STUDIO, feature: DERIVED, derived: got.derived, links: got.links });
    (b, h)
}

fn derived_parts(doc: &Document, el: ElementId) -> Vec<Part> {
    let build = cadrs_core::rebuild::build(doc.element(el).unwrap().features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    build.parts.iter().filter(|p| p.feature == DERIVED).cloned().collect()
}

fn mass_of(parts: &[Part]) -> cadrs_kernel::MassProperties {
    cadrs_core::parts::combined_mass(parts.iter()).unwrap()
}

/// ex-dv1: two copies of the block, at z 0..25 and 100..125.
#[test]
fn ex_dv1_two_copies() {
    let store = temp_store("dv1");
    let v1 = save_a(&store);
    let (b, _) = ex_dv1(&store, v1);
    let parts = derived_parts(&b, lb::HOST_STUDIO);
    assert_eq!(parts.len(), 2, "two copies");
    let m = mass_of(&parts);
    let (v, a) = block(L, W, H);
    close("volume", m.volume, 2.0 * v, 1e-6);
    close("volume value", m.volume, 75_000.0, 1e-6);
    close("area", m.surface_area, 2.0 * a, 1e-6);
    close("area value", m.surface_area, 14_000.0, 1e-6);
    close("x̄", m.center_of_mass.x, L / 2.0, 1e-9);
    close("ȳ", m.center_of_mass.y, W / 2.0, 1e-9);
    // (12.5 + 112.5) / 2: the source's centroid z plus 50.
    close("z̄", m.center_of_mass.z, (H / 2.0 + (100.0 + H / 2.0)) / 2.0, 1e-9);
    close("z̄ value", m.center_of_mass.z, 62.5, 1e-9);
    let c = m.center_of_mass;
    let i = parts.iter().map(|p| p.mass.unwrap().inertia_about(c)).fold(nalgebra::Matrix3::zeros(), |x, y| x + y);
    // Each copy's own inertia plus V·d² for its 50 mm offset in z.
    close("Ixx", i[(0, 0)], 2.0 * (v * (W * W + H * H) / 12.0 + v * 50.0 * 50.0), 1e-3);
    close("Iyy", i[(1, 1)], 2.0 * (v * (L * L + H * H) / 12.0 + v * 50.0 * 50.0), 1e-3);
    close("Izz", i[(2, 2)], 2.0 * v * (L * L + W * W) / 12.0, 1e-3);
    close("Ixx value", i[(0, 0)], 197_031_250.0, 1e-3);
    // The source's Mate connector 1 comes along (Include mate connectors is off: only the
    // copies' own placement uses it) and Sketch 1 and the connector are children.
    assert_eq!(b.linked.len(), 1, "one copy of A's Block at V1");
}

/// ex-dv2: A's depth 25 → 40 and V2; B still reads 75 000 and is out of date; Update brings
/// 2 × 50·30·40 = 120 000, area 2 × 9 400, centroid z (20 + 120)/2 = 70; undo 75 000.
#[test]
fn ex_dv2_update_brings_the_new_version() {
    let store = temp_store("dv2");
    let v1 = save_a(&store);
    let (mut b, mut h) = ex_dv1(&store, v1);
    let v2 = edit_a(&store, 2_000, |s| lb::set_height(s, lb::STUDIO, 40.0).unwrap());
    assert_ne!(v1, v2);
    let (v25, _) = block(L, W, H);
    close("unchanged", mass_of(&derived_parts(&b, lb::HOST_STUDIO)).volume, 2.0 * v25, 1e-6);
    let mut res = Resolver::new(store.clone());
    let uses = lu::uses(&b);
    assert_eq!(uses.len(), 1);
    let stale = lu::staleness(&b, &uses[0], &mut |d: DocumentId| res.latest(d).map(|v| (v.id(), v.name().to_string())));
    assert!(stale.any(), "the update indicator");
    let c = lu::change_for(&mut res, &b, None, &uses[0], Target::Latest).unwrap().unwrap();
    run(&mut b, &mut h, &UpdateReferences { changes: vec![c], label: "Update".into() });
    let m = mass_of(&derived_parts(&b, lb::HOST_STUDIO));
    let (v40, a40) = block(L, W, 40.0);
    close("volume", m.volume, 2.0 * v40, 1e-6);
    close("volume value", m.volume, 120_000.0, 1e-6);
    close("area", m.surface_area, 2.0 * a40, 1e-6);
    close("area value", m.surface_area, 18_800.0, 1e-6);
    close("z̄", m.center_of_mass.z, (20.0 + 120.0) / 2.0, 1e-9);
    h.undo(&mut b);
    close("undone", mass_of(&derived_parts(&b, lb::HOST_STUDIO)).volume, 75_000.0, 1e-6);
}

/// ex-dv3: in A, Part Studio 2 derives Block at the workspace; Sketch 1's length 50 → 60
/// gives 60·30·25 = 45 000 at once, with no version reference to update.
#[test]
fn ex_dv3_a_workspace_derive_follows_a_length_edit() {
    let mut a = lb::source_document().unwrap();
    let mut h = History::default();
    let ps2 = Element::part_studio("Part Studio 2");
    let ps2_id = ps2.id;
    a.elements.push(ps2);
    let mut res = Resolver::new(temp_store("dv3"));
    let r = SourceRef { document: None, at: RefAt::Workspace, element: lb::STUDIO, pinned: false };
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), r).unwrap();
    run(&mut a, &mut h, &AddDerived { element: ps2_id, feature: DERIVED, derived: got.derived, links: got.links });
    close("before", mass_of(&derived_parts(&a, ps2_id)).volume, block(L, W, H).0, 1e-6);
    lb::set_length(&mut DocHistory(&mut a, &mut h), lb::STUDIO, 60.0).unwrap();
    close("after", mass_of(&derived_parts(&a, ps2_id)).volume, block(60.0, W, H).0, 1e-6);
    close("after value", mass_of(&derived_parts(&a, ps2_id)).volume, 45_000.0, 1e-6);
    assert!(lu::uses(&a).is_empty(), "no version reference: nothing to update");
}

/// Volume and centroid of every instance of the assembly `asm`.
fn assembly_mass(doc: &Document, asm: ElementId) -> (f64, [f64; 3]) {
    let model = doc.element(asm).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(doc, model, |e| doc.element(e).map(|el| cadrs_core::rebuild::build(el.features())));
    let m = cadrs_core::parts::combined_mass(parts.iter()).unwrap();
    (m.volume, [m.center_of_mass.x, m.center_of_mass.y, m.center_of_mass.z])
}

fn update_all(store: &Store, doc: &mut Document, h: &mut History, target: impl Fn(&lu::RefUse) -> Target) {
    let mut res = Resolver::new(store.clone());
    let changes: Vec<_> = lu::uses(doc).iter().filter_map(|u| lu::change_for(&mut res, doc, None, u, target(u)).unwrap()).collect();
    assert!(!changes.is_empty());
    run(doc, h, &UpdateReferences { changes, label: "Update all".into() });
}

/// ex-dv4: C's Assembly 1 inserts A's Block at V1, Fastened bottom-face centre on the plate's
/// top-face centre (the block spans x 25..75, y 35..65, z 10..35); Update to V2 (height 40):
/// the mate still resolves, V 100·100·10 + 50·30·40, z̄ (100 000·5 + 60 000·30)/160 000; then a
/// V3 whose faces are all renamed (Sketch 1 redrawn): the mate's face is lost, and it is an
/// error, not held at its old frame.
#[test]
fn ex_dv4_mates_survive_an_update_and_a_lost_face_is_an_error() {
    let store = temp_store("dv4");
    let v1 = save_a(&store);
    let mut c = lb::base_document().unwrap();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1);
    let snap = res.resolve(r, &c, None).unwrap();
    let blk = InstanceId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0c01);
    let inst = Instance::new(blk, InstanceSource::Part { element: snap.root, part: lb::PART }, Pose::translation([150.0, 0.0, 30.0]));
    run(&mut c, &mut h, &InsertLinked { element: lb::BASE_ASSEMBLY, snapshot: snap, instances: vec![inst], reference: r });
    lb::fasten_on_base(&mut DocHistory(&mut c, &mut h), blk).unwrap();
    let plate = 100.0 * 100.0 * 10.0;
    let expect = |hb: f64| {
        let vb = L * W * hb;
        (plate + vb, (plate * 5.0 + vb * (10.0 + hb / 2.0)) / (plate + vb))
    };
    let (v, c0) = assembly_mass(&c, lb::BASE_ASSEMBLY);
    close("V1 volume", v, expect(H).0, 1e-6);
    close("V1 volume value", v, 137_500.0, 1e-6);
    close("V1 x̄", c0[0], 50.0, 1e-6);
    close("V1 ȳ", c0[1], 50.0, 1e-6);
    close("V1 z̄", c0[2], expect(H).1, 1e-6);
    close("V1 z̄ value", c0[2], 9.7727, 1e-4);
    // V2: the block 40 high.
    edit_a(&store, 2_000, |s| lb::set_height(s, lb::STUDIO, 40.0).unwrap());
    update_all(&store, &mut c, &mut h, |_| Target::Latest);
    let (v, c2) = assembly_mass(&c, lb::BASE_ASSEMBLY);
    close("V2 volume", v, expect(40.0).0, 1e-6);
    close("V2 volume value", v, 160_000.0, 1e-6);
    close("V2 x̄", c2[0], 50.0, 1e-6);
    close("V2 z̄", c2[2], expect(40.0).1, 1e-6);
    close("V2 z̄ value", c2[2], 14.375, 1e-9);
    let model = c.element(lb::BASE_ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let solids = assembly::document_occurrence_solids(&c, lb::BASE_ASSEMBLY);
    assert!(assembly::lost_mates(&structure::solver_model(&c, &model), &solids).is_empty(), "the mate survives: its face persists");
    // V3: the same block, every face renamed. The mate's face is lost: an error.
    edit_a(&store, 3_000, |s| lb::redraw(s, lb::STUDIO).unwrap());
    update_all(&store, &mut c, &mut h, |_| Target::Latest);
    close("V3 volume", assembly_mass(&c, lb::BASE_ASSEMBLY).0, expect(40.0).0, 1e-6);
    let model = c.element(lb::BASE_ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let solids = assembly::document_occurrence_solids(&c, lb::BASE_ASSEMBLY);
    assert_eq!(assembly::lost_mates(&structure::solver_model(&c, &model), &solids), vec![lb::BASE_MATE], "the lost face makes the mate an error");
    h.undo(&mut c);
    let model = c.element(lb::BASE_ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let solids = assembly::document_occurrence_solids(&c, lb::BASE_ASSEMBLY);
    assert!(assembly::lost_mates(&structure::solver_model(&c, &model), &solids).is_empty(), "undo: V2 again, no error");
}

/// ex-dv5: B derives A's Block at V1 and is versioned; A's Block deriving B's Part Studio 1 at
/// that version is refused with the path, and nothing is added.
#[test]
fn ex_dv5_a_circular_derive_is_refused() {
    let store = temp_store("dv5");
    let v1 = save_a(&store);
    let (b, _) = ex_dv1(&store, v1);
    store.create(&b, &DocumentMeta::new("me", 1_500)).unwrap();
    let mut blog = HistoryLog::start(&b, 1_500, "me");
    let bv1 = blog.create_version("", "", 1_501, "me");
    blog.save(&store).unwrap();
    let mut a = store.load(lb::DOCUMENT).unwrap().document;
    let before = a.clone();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let got = derived::resolve(&mut res, &a, None, DerivedFeature::default(), SourceRef::version(Some(lb::HOST_DOCUMENT), lb::HOST_STUDIO, bv1)).unwrap();
    let e = h.execute(&mut a, &AddDerived { element: lb::STUDIO, feature: FeatureId::new(), derived: got.derived, links: got.links }).unwrap_err();
    assert_eq!(e.to_string(), "Circular reference: Block source › Block → Block derived › Part Studio 1 → Block source");
    assert_eq!(a, before, "nothing is added");
}

// ---------------------------------------------------------------------------------------------
// ER Ex1 (Hexapod) and ER Ex2 (Move to document)

/// The piston's parts (volume, centroid z) for a cylinder Ø`d` and a rod `l` long.
fn piston_parts(d: f64, l: f64) -> [(f64, f64); 4] {
    [(10.0f64.powi(3), -5.0), (60.0 * PI * (d / 2.0).powi(2), 30.0), (PI * 3.0f64.powi(2) * l, 60.0 + l / 2.0), (8.0f64.powi(3), 60.0 + l + 4.0)]
}

fn piston_closed_form(d: f64, l: f64) -> (f64, f64) {
    let p = piston_parts(d, l);
    let v: f64 = p.iter().map(|x| x.0).sum();
    (v, p.iter().map(|x| x.0 * x.1).sum::<f64>() / v)
}

fn hexapod_closed_form(d: f64, l: f64) -> (f64, f64) {
    let (vp, zp) = piston_closed_form(d, l);
    let base = PI * (100.0f64.powi(2) - 6.0 * 25.0) * 10.0;
    let top = PI * (80.0f64.powi(2) - 6.0 * 25.0) * 10.0;
    let v = base + top + 6.0 * vp;
    (v, (base * 5.0 + top * (88.0 + l + 5.0) + 6.0 * vp * (zp + 20.0)) / v)
}

/// ER8.check: the Piston Assembly of ER Ex2's stand-in.
#[test]
fn piston_assembly_matches_its_closed_form() {
    let doc = ps::move_to_document().unwrap();
    let (v, c) = assembly_mass(&doc, ps::PISTON_ASSEMBLY);
    let (pv, pz) = piston_closed_form(15.725, 28.0);
    close("volume", v, pv, 1e-6);
    close("volume value", v, 13_956.271, 1e-3);
    close("x̄", c[0], 0.0, 1e-9);
    close("ȳ", c[1], 0.0, 1e-9);
    close("z̄", c[2], pz, 1e-9);
    close("z̄ value", c[2], 32.263, 1e-3);
    close("Al mass", v * AL, 37.682, 1e-3);
    // The same document's Hexapod (pistons placed, not mated): the V1 numbers of ER Ex1.
    let (hv, hz) = hexapod_closed_form(15.725, 28.0);
    let (v, c) = assembly_mass(&doc, ps::HEXAPOD);
    close("Hexapod volume", v, hv, 1e-6);
    close("Hexapod volume value", v, 589_534.041, 1e-3);
    close("Hexapod x̄", c[0], 0.0, 1e-9);
    close("Hexapod ȳ", c[1], 0.0, 1e-9);
    close("Hexapod z̄", c[2], hz, 1e-9);
    close("Hexapod z̄ value", c[2], 50.348, 1e-3);
}

/// Saves the Piston document with V1 (as ER Ex1 steps 1–2 make it).
fn save_piston(store: &Store) -> VersionId {
    let p = ps::piston_document().unwrap();
    store.create(&p, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(&p, 1_000, "me");
    let v = log.create_version("V1", "", 1_001, "me");
    log.save(store).unwrap();
    v
}

/// ER Ex1 steps 11–14: Main Sketch Ø15.725 → Ø20, the rod 28 → 75, V2.
fn edit_piston(store: &Store) -> VersionId {
    let file = store.load(ps::PISTON_DOCUMENT).unwrap();
    let mut doc = file.document;
    let mut h = History::default();
    let mut s = DocHistory(&mut doc, &mut h);
    ps::set_diameter(&mut s, ps::LINKED_PISTON_STUDIO, 20.0).unwrap();
    ps::set_rod_length(&mut s, ps::LINKED_PISTON_STUDIO, 75.0).unwrap();
    store.save(&doc, &file.meta).unwrap();
    let mut log = HistoryLog::load(store, ps::PISTON_DOCUMENT).unwrap().unwrap();
    log.record(&doc, Origin::Command("Edit".into()), 2_000, "me");
    let v = log.create_version("V2", "", 2_001, "me");
    log.save(store).unwrap();
    v
}

fn check_hexapod(what: &str, doc: &Document, d: f64, l: f64) {
    let (v, c) = assembly_mass(doc, ps::PROJECT_HEXAPOD);
    let (hv, hz) = hexapod_closed_form(d, l);
    close(&format!("{what} volume"), v, hv, 1e-6);
    close(&format!("{what} x̄"), c[0], 0.0, 1e-6);
    close(&format!("{what} ȳ"), c[1], 0.0, 1e-6);
    close(&format!("{what} z̄"), c[2], hz, 1e-6);
    // The Topplate sits on the eyes.
    let model = doc.element(ps::PROJECT_HEXAPOD).unwrap().assembly_model().unwrap();
    let top = model.instance(ps::TOP_INSTANCE).unwrap().pose;
    close(&format!("{what} Topplate z"), top.translation[2], 88.0 + l, 1e-6);
    let solids = assembly::document_occurrence_solids(doc, ps::PROJECT_HEXAPOD);
    assert!(assembly::lost_mates(&structure::solver_model(doc, model), &solids).is_empty(), "{what}: every mate resolves");
}

/// ER6.check: the Hexapod with six linked Piston Assembly instances, mated as the course mates
/// them (Revolute on the baseplate holes, Fastened to the Topplate holes), at V1 and after
/// Update all to V2, and back to V1 with a selective update.
#[test]
fn hexapod_stand_in_updates_as_the_course_does() {
    let store = temp_store("hexapod");
    let v1 = save_piston(&store);
    let mut doc = ps::project_document().unwrap();
    let mut h = History::default();
    // Step 4 and 7: six instances of Piston Assembly at V1, dropped beside their holes.
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(ps::PISTON_DOCUMENT), ps::LINKED_PISTON_ASSEMBLY, v1);
    let snap = res.resolve(r, &doc, None).unwrap();
    let instances: Vec<Instance> = (0..6)
        .map(|k| {
            let p = ps::piston_pose(k);
            Instance::new(ps::piston(k), InstanceSource::Assembly { element: snap.root }, Pose::translation([p.translation[0] * 1.1, p.translation[1] * 1.1, p.translation[2] + 15.0]))
        })
        .collect();
    run(&mut doc, &mut h, &InsertLinked { element: ps::PROJECT_HEXAPOD, snapshot: snap, instances, reference: r });
    // Steps 6, 8–10: the twelve mates.
    {
        let mut s = DocHistory(&mut doc, &mut h);
        for k in 0..6 {
            ps::revolute_piston(&mut s, ps::PROJECT_HEXAPOD, ps::piston(k), k).unwrap();
        }
        for k in 0..6 {
            ps::fasten_topplate(&mut s, ps::PROJECT_HEXAPOD, ps::piston(k), k).unwrap();
        }
    }
    check_hexapod("V1", &doc, 15.725, 28.0);
    let (v, c) = assembly_mass(&doc, ps::PROJECT_HEXAPOD);
    close("V1 volume value", v, 589_534.041, 1e-3);
    close("V1 z̄ value", c[2], 50.348, 1e-3);
    close("V1 Al mass", v * AL, 1_591.742, 1e-3);
    // Steps 11–17: the piston edited, V2, Update all.
    let v2 = edit_piston(&store);
    update_all(&store, &mut doc, &mut h, |_| Target::Latest);
    check_hexapod("V2", &doc, 20.0, 75.0);
    let (v, c) = assembly_mass(&doc, ps::PROJECT_HEXAPOD);
    close("V2 volume value", v, 640_689.203, 1e-3);
    close("V2 z̄ value", c[2], 65.964, 1e-3);
    close("V2 Al mass", v * AL, 1_729.861, 1e-3);
    assert!(lu::uses(&doc).iter().all(|u| u.reference.at == RefAt::Version(v2)));
    // Step 17: Selective update back to V1 restores the V1 numbers.
    update_all(&store, &mut doc, &mut h, |_| Target::Version(v1));
    check_hexapod("back to V1", &doc, 15.725, 28.0);
    // Undo twice: the selective update (back at V2), then Update all (V1 as mated).
    h.undo(&mut doc);
    check_hexapod("undo to V2", &doc, 20.0, 75.0);
    h.undo(&mut doc);
    check_hexapod("undo to V1", &doc, 15.725, 28.0);
}

/// Linked instances keep their world poses when the pistons are placed by mates: the stand-in's
/// placements (without mates, ER Ex2) and the mated Hexapod agree.
#[test]
fn the_mated_hexapod_places_the_pistons_where_the_placed_one_does() {
    let store = temp_store("hexapod-poses");
    let v1 = save_piston(&store);
    let mut doc = ps::project_document().unwrap();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(ps::PISTON_DOCUMENT), ps::LINKED_PISTON_ASSEMBLY, v1);
    let snap = res.resolve(r, &doc, None).unwrap();
    let instances: Vec<Instance> = (0..6).map(|k| Instance::new(ps::piston(k), InstanceSource::Assembly { element: snap.root }, Pose::translation([0.0, 0.0, 200.0]))).collect();
    run(&mut doc, &mut h, &InsertLinked { element: ps::PROJECT_HEXAPOD, snapshot: snap, instances, reference: r });
    let mut s = DocHistory(&mut doc, &mut h);
    for k in 0..6 {
        ps::revolute_piston(&mut s, ps::PROJECT_HEXAPOD, ps::piston(k), k).unwrap();
    }
    let model = doc.element(ps::PROJECT_HEXAPOD).unwrap().assembly_model().unwrap();
    for k in 0..6 {
        let p = model.instance(ps::piston(k)).unwrap().pose;
        let want = ps::piston_pose(k);
        for i in 0..3 {
            close(&format!("piston {k} translation {i}"), p.translation[i], want.translation[i], 1e-6);
        }
        close(&format!("piston {k} upright"), p.rotation[2][2], 1.0, 1e-9);
    }
}

/// The lost-mate error ([`assembly::lost_mates`]) never flags a healthy mate: every mate of every
/// assembly in the shipped stand-ins resolves on its parts.
#[test]
fn no_shipped_assembly_has_a_lost_mate() {
    let mut checked = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "cadrs") {
            continue;
        }
        let doc = cadrs_core::Store::load_path(&path).unwrap().document;
        for el in &doc.elements {
            let Some(model) = el.assembly_model() else { continue };
            let solids = assembly::document_occurrence_solids(&doc, el.id);
            let lost = assembly::lost_mates(&structure::solver_model(&doc, model), &solids);
            let names: Vec<&str> = lost.iter().filter_map(|m| model.mate(*m).map(|f| f.name.as_str())).collect();
            assert!(lost.is_empty(), "{} › {}: {names:?} ({lost:?})", path.display(), el.name);
            checked += model.mates.len();
        }
    }
    assert!(checked > 20, "{checked} mates checked");
}
