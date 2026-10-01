//! Session snapshots ([`cadrs_core::rebuild::persist`]): a new session restored from the
//! snapshot of a rebuild has every output cached (nothing is computed again) and makes exactly
//! what building made (parts, their face, edge and vertex names, errors, planes, connectors,
//! Derived features); an edit after restoring recomputes what it does in the session that built
//! everything and makes the same; the same outputs always give the same bytes.
#![cfg(feature = "occt")]

use cadrs_core::commands::AddFeature;
use cadrs_core::derived::{AddDerived, DerivedFeature, DerivedPlacement, DerivedSelection};
use cadrs_core::document::{Element, Feature, FeatureKind};
use cadrs_core::external::{Resolver, SourceRef};
use cadrs_core::history_log::HistoryLog;
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use cadrs_core::rebuild::{Build, Rebuilder};
use cadrs_core::samples::linked_block as lb;
use cadrs_core::samples::{hand_brake, phone_case};
use cadrs_core::{Document, DocumentMeta, ElementId, FeatureId, History, Store};

/// What a build made, for comparing two builds.
fn summary(b: &Build) -> String {
    let mut out = String::new();
    for p in &b.parts {
        let s = &p.solid;
        out += &format!(
            "{} {:?} {:?} {:.9?} faces {:?} edges {:?} vertices {:?} tris {} connectors {}\n",
            p.name,
            p.id,
            p.kind,
            p.mass.map(|m| (m.volume, m.center_of_mass)),
            s.faces.iter().map(|f| (f.name, f.plane)).collect::<Vec<_>>(),
            s.edges.iter().map(|e| e.name).collect::<Vec<_>>(),
            s.vertices.iter().map(|v| v.name).collect::<Vec<_>>(),
            s.indices.len() / 3,
            s.connectors.len(),
        );
    }
    let sorted = |v: Vec<String>| {
        let mut v = v;
        v.sort();
        v.join("; ")
    };
    out += &format!("errors {:?}\nwarnings {:?}\n", b.errors, b.warnings);
    out += &format!("planes {}\n", sorted(b.planes.iter().map(|(k, f)| format!("{k:?} {f:?}")).collect()));
    out += &format!("connectors {}\n", sorted(b.connectors.iter().map(|(k, f)| format!("{k:?} {f:?}")).collect()));
    out += &format!("derived {}\n", sorted(b.derived.iter().map(|(k, d)| format!("{k:?} {d:?}")).collect()));
    out + &format!("derived sketches {:?}", b.derived_sketches.iter().map(|f| (f.id, &f.name)).collect::<Vec<_>>())
}

/// Builds `features`, restores its snapshot into a new session, and checks the two agree, then
/// edits with `edit` in both and checks again.
fn round_trip(features: Vec<Feature>, edit: impl Fn(&mut Vec<Feature>)) {
    let mut a = Rebuilder::new();
    let built = a.rebuild(&features);
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    let blob = a.snapshot(&a.snapshot_keys(&features)).unwrap();
    assert_eq!(a.snapshot(&a.snapshot_keys(&features)).unwrap(), blob, "the same outputs give the same bytes");

    let mut b = Rebuilder::new();
    let restored = b.restore(&blob).unwrap();
    assert!(restored > 0);
    let again = b.rebuild(&features);
    assert_eq!(again.computed, 0, "every output was restored");
    assert_eq!(summary(&again), summary(&built));
    // Snapshotting the restored session gives the same tables, meshes and outputs, and bodies of
    // the same sizes (OCCT's per-shape flags, such as "checked", can differ between a body built
    // and one read back; the geometry doesn't).
    let again_blob = b.snapshot(&b.snapshot_keys(&features)).unwrap();
    let bodies_at = |b: &[u8]| {
        let ron_end = 20 + u64::from_le_bytes(b[12..20].try_into().unwrap()) as usize;
        ron_end + 8 + u64::from_le_bytes(b[ron_end..ron_end + 8].try_into().unwrap()) as usize
    };
    assert!(again_blob[..bodies_at(&again_blob)] == blob[..bodies_at(&blob)], "a restored session snapshots to the same outputs");
    assert_eq!(again_blob.len(), blob.len(), "and the same bodies");

    // An edit: the same features recomputed in both sessions, and the same result.
    let mut edited = features.clone();
    edit(&mut edited);
    let after_a = a.rebuild(&edited);
    let after_b = b.rebuild(&edited);
    assert!(after_a.computed > 0, "the edit recomputes something");
    assert_eq!(after_b.computed, after_a.computed, "only what the edit touches is recomputed");
    assert_eq!(summary(&after_b), summary(&after_a));
}

fn studio(doc: &Document) -> Vec<Feature> {
    doc.elements.iter().find(|e| !e.features().is_empty()).unwrap().active_features()
}

/// Deepens the last extrude by 1 mm.
fn deepen_last_extrude(features: &mut Vec<Feature>) {
    let e = features.iter_mut().rev().find_map(|f| match &mut f.kind {
        FeatureKind::Extrude(e) => Some(e),
        _ => None,
    });
    let e = e.expect("an extrude");
    e.depth += 1.0;
    e.depth_expr = format!("{} mm", e.depth);
}

#[test]
fn hand_brake_restores_exactly() {
    round_trip(studio(&hand_brake::document().unwrap()), deepen_last_extrude);
}

#[test]
fn phone_case_restores_exactly() {
    round_trip(studio(&phone_case::document().unwrap()), deepen_last_extrude);
}

const TOP: FeatureId = FeatureId::from_u128(0x3a63_0000_0000_0000_0000_0000_0000_0042);
const LOC: FeatureId = FeatureId::from_u128(0x3a63_0000_0000_0000_0000_0000_0000_0043);
const HOST: ElementId = ElementId::from_u128(0x3a63_0000_0000_0000_0000_0000_0000_0501);
const DERIVED: FeatureId = FeatureId::from_u128(0x3a63_0000_0000_0000_0000_0000_0000_0502);

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

/// A host deriving the linked block onto a connector at (0, 0, 100) by its top connector: the
/// source is rebuilt inside the host's rebuild, so its outputs are in the snapshot too.
fn derived_host() -> Vec<Feature> {
    let store = Store::new(std::env::temp_dir().join(format!("cadrs-snap-{}-{}", std::process::id(), uuid::Uuid::new_v4())));
    let mut a = lb::document().unwrap();
    let mut h = History::default();
    h.execute(&mut a, &AddFeature { element: lb::STUDIO, feature: TOP, base_name: "Mate connector".into(), kind: connector([25.0, 15.0, 25.0], Some(lb::PART)) }).unwrap();
    store.create(&a, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(&a, 1_000, "me");
    let v = log.create_version("", "", 1_001, "me");
    log.save(&store).unwrap();
    let mut b = Document::empty("Host");
    let mut el = Element::part_studio("Part Studio 1");
    el.id = HOST;
    b.elements.push(el);
    let mut h = History::default();
    h.execute(&mut b, &AddFeature { element: HOST, feature: LOC, base_name: "Mate connector".into(), kind: connector([0.0, 0.0, 100.0], None) }).unwrap();
    let d = DerivedFeature {
        selection: DerivedSelection { all: false, parts: vec![lb::PART], ..DerivedSelection::default() },
        locations: vec![ConnectorRef::Feature(LOC)],
        placement: DerivedPlacement::BaseConnector(Some(ConnectorRef::Feature(TOP))),
        include_connectors: true,
        ..DerivedFeature::default()
    };
    let mut res = Resolver::new(store.clone());
    let got = cadrs_core::derived::resolve(&mut res, &b, None, d, SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v)).unwrap();
    h.execute(&mut b, &AddDerived { element: HOST, feature: DERIVED, derived: got.derived, links: got.links }).unwrap();
    let _ = std::fs::remove_dir_all(store.root());
    b.element(HOST).unwrap().features().to_vec()
}

#[test]
fn a_derived_feature_restores_exactly() {
    // The edit moves the host's connector: the Derived feature after it is recomputed, its
    // source from the restored outputs.
    round_trip(derived_host(), |f| {
        if let Some(FeatureKind::MateConnector(c)) = f.iter_mut().find(|f| f.id == LOC).map(|f| &mut f.kind) {
            c.offset[2] = 120.0;
            c.offset_expr[2] = "120 mm".into();
        }
    });
}

#[test]
fn a_broken_snapshot_restores_nothing() {
    let features = studio(&hand_brake::document().unwrap());
    let mut a = Rebuilder::new();
    let built = a.rebuild(&features);
    let blob = a.snapshot(&a.snapshot_keys(&features)).unwrap();
    let mut b = Rebuilder::new();
    assert!(b.restore(&blob[..blob.len() / 2]).is_err(), "cut short");
    assert!(b.restore(b"not a snapshot").is_err());
    assert_eq!(b.cached(), 0, "nothing was added");
    let again = b.rebuild(&features);
    assert_eq!(again.computed, built.computed, "everything is built again");
    assert_eq!(summary(&again), summary(&built));
}
