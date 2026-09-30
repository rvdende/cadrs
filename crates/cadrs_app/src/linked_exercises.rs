//! P3G.5: scenario set-ups for the exercises of stage 3G (`derived-and-linking-gaps.md` "What
//! each exercise needs"), on the stand-in fixtures that `cadrs_core/tests/course_linked.rs`
//! checks:
//!
//! - `linked-fixture store <name> [folder]`: writes `fixtures/<name>.cadrs` (and its
//!   `<name>.history.ron`, with its versions) into the document store, with its thumbnail, as a
//!   document the user owns and opened an hour ago; `folder` puts it in the library folder
//!   "Linked parts". A document already in the store is left as it is (its edits kept).
//! - `linked-fixture open <name> [tab name]`: stores it when it isn't yet and opens it (on that
//!   tab), saving the open document first, as the documents page does.
//! - `linked-ex dv1`: ex-dv1's Derived 1 in the open "Block derived" (A's newest version, at the
//!   origin and at Mate connector 1): the state ex-dv2 and ex-dv5 start from.
//! - `linked-ex dv4-mate`: ex-dv4's Fastened 1 in the open "Block assembly": the linked Block's
//!   bottom-face centre on Base <1>'s top-face centre.
//! - `linked-ex dv4-redraw`: A's Sketch 1 redrawn (every face renamed) and its next version,
//!   in the store (the lost-face update of ex-dv4).
//! - `linked-ex hexapod-mates`: ER Ex1 steps 8–10 after the first mate made in the UI: a
//!   Revolute for every other piston on the next free baseplate hole, then Fastened 1–6 from the
//!   Eyes to the Topplate holes.
//! - `linked-ex hexapod-revolute`: ER Ex1 step 6's Revolute 1 (the first piston's UJoint on
//!   the first baseplate hole).

use bevy::prelude::*;
use cadrs_core::samples::linked_block as lb;
use cadrs_core::samples::piston as ps;

use crate::{ActiveDocument, AppClock, DocumentStore, UserProfile};

/// The library folder the linked sources go in (ER1.2 locations).
const FOLDER: cadrs_core::FolderId = cadrs_core::FolderId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0f01);

fn fixture_path(name: &str, ext: &str) -> std::path::PathBuf {
    let local = std::path::PathBuf::from("fixtures");
    let dir = if local.is_dir() { local } else { std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures")) };
    dir.join(format!("{name}.{ext}"))
}

/// Stores the fixture `name` (see the module docs); returns its document id.
fn store_fixture(world: &mut World, name: &str, folder: bool) -> Option<cadrs_core::DocumentId> {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let file = match cadrs_core::Store::load_path(&fixture_path(name, "cadrs")) {
        Ok(f) => f,
        Err(e) => {
            warn!("linked-fixture {name}: {e}");
            return None;
        }
    };
    let id = file.document.id;
    if store.load(id).is_ok() {
        return Some(id);
    }
    if folder {
        let (lib, _) = store.list();
        if lib.folders.iter().all(|f| f.id != FOLDER) {
            let mut after = lib.clone();
            after.folders.push(cadrs_core::FolderEntry { id: FOLDER, name: "Linked parts".into(), created: now - 7_200, owned_by: user.clone() });
            if let Err(e) = store.sync(&lib, &after) {
                warn!("linked-fixture {name}: {e}");
            }
        }
    }
    let mut meta = cadrs_core::DocumentMeta::new(&user, now - 7_200);
    meta.last_opened = Some(now - 3_600);
    meta.folder = folder.then_some(FOLDER);
    if let Err(e) = store.create(&file.document, &meta) {
        warn!("linked-fixture {name}: {e}");
        return None;
    }
    if let Some(img) = crate::script::studio_thumbnail(&file.document) {
        let _ = store.write_thumbnail(id, &img);
    }
    let history = fixture_path(name, "history.ron");
    if history.is_file() {
        match cadrs_core::history_log::HistoryLog::load_path(&history) {
            Ok(log) if log.document == id => {
                if let Err(e) = log.save(&store) {
                    warn!("linked-fixture {name} history: {e}");
                }
            }
            Ok(_) => warn!("linked-fixture {name}: its history is another document's"),
            Err(e) => warn!("linked-fixture {name} history: {e}"),
        }
    }
    Some(id)
}

/// `linked-fixture …` (see the module docs).
pub fn fixture_script(world: &mut World, arg: &str) {
    let words: Vec<&str> = arg.split_whitespace().collect();
    match words.as_slice() {
        ["store", name, rest @ ..] => {
            store_fixture(world, name, rest.contains(&"folder"));
        }
        ["open", name, tab @ ..] => {
            let Some(id) = store_fixture(world, name, false) else { return };
            crate::move_document::open_document(world, id);
            let tab = tab.join(" ");
            if !tab.is_empty()
                && let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
                && let Some(e) = d.doc.elements.iter().find(|e| e.name == tab).map(|e| e.id)
            {
                d.set_active(e);
            }
        }
        _ => warn!("linked-fixture: unknown {arg:?}"),
    }
}

/// `linked-ex …` (see the module docs).
pub fn script(world: &mut World, arg: &str) {
    use cadrs_core::derived::{AddDerived, DerivedFeature};
    use cadrs_core::external::SourceRef;
    use cadrs_core::mate::{ConnectorOrigin, ConnectorRef};
    match arg.trim() {
        "dv1" => {
            let Some(v) = crate::linked::resolver(world).0.latest(lb::DOCUMENT) else {
                warn!("linked-ex dv1: A has no version");
                return;
            };
            let doc = world.resource::<ActiveDocument>().doc.clone();
            let d = DerivedFeature { locations: vec![ConnectorRef::Implicit(ConnectorOrigin::Origin), ConnectorRef::Feature(lb::HOST_CONNECTOR)], ..DerivedFeature::default() };
            let got = {
                let mut res = crate::linked::resolver(world);
                cadrs_core::derived::resolve(&mut res.0, &doc, None, d, SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v.id()))
            };
            match got {
                Ok(g) => {
                    let f = cadrs_core::FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0b01);
                    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&AddDerived { element: lb::HOST_STUDIO, feature: f, derived: g.derived, links: g.links }) {
                        warn!("linked-ex dv1: {e}");
                    }
                }
                Err(e) => warn!("linked-ex dv1: {e}"),
            }
        }
        "dv4-mate" => {
            let doc = world.resource::<ActiveDocument>().doc.clone();
            let Some(block) = doc.element(lb::BASE_ASSEMBLY).and_then(|e| e.assembly_model()).and_then(|a| a.instances.iter().find(|i| i.link.is_some()).map(|i| i.id)) else {
                warn!("linked-ex dv4-mate: no linked Block in Assembly 1");
                return;
            };
            if let Err(e) = lb::fasten_on_base(&mut *world.resource_mut::<ActiveDocument>(), block) {
                warn!("linked-ex dv4-mate: {e}");
            }
        }
        "dv4-redraw" => {
            use cadrs_core::history_log::{HistoryLog, Origin};
            world.resource_mut::<crate::linked::LinkStatus>().invalidate();
            let store = world.resource::<DocumentStore>().0.clone();
            let now = world.resource::<AppClock>().now();
            let user = world.resource::<UserProfile>().id.clone();
            let Ok(file) = store.load(lb::DOCUMENT) else { return };
            let mut a = file.document;
            let mut h = cadrs_core::History::default();
            if let Err(e) = lb::redraw(&mut cadrs_core::samples::gear_cover::DocHistory(&mut a, &mut h), lb::STUDIO) {
                warn!("linked-ex dv4-redraw: {e}");
                return;
            }
            let mut meta = file.meta;
            meta.modified = now;
            let _ = store.save(&a, &meta);
            if let Ok(Some(mut log)) = HistoryLog::load(&store, lb::DOCUMENT) {
                log.record(&a, Origin::Command("Edit Sketch 1".into()), now - 60, &user);
                log.create_version("", "Sketch 1 redrawn", now - 30, &user);
                let _ = log.save(&store);
            }
        }
        "hexapod-revolute" => {
            let doc = world.resource::<ActiveDocument>().doc.clone();
            let Some(p) = doc.element(ps::PROJECT_HEXAPOD).and_then(|e| e.assembly_model()).and_then(|a| a.instances.iter().find(|i| i.source.is_assembly()).map(|i| i.id)) else {
                warn!("linked-ex hexapod-revolute: no piston");
                return;
            };
            if let Err(e) = ps::revolute_piston(&mut *world.resource_mut::<ActiveDocument>(), ps::PROJECT_HEXAPOD, p, 0) {
                warn!("linked-ex hexapod-revolute: {e}");
            }
        }
        "hexapod-mates" => {
            if let Err(e) = ps::complete_hexapod(&mut *world.resource_mut::<ActiveDocument>(), ps::PROJECT_HEXAPOD) {
                warn!("linked-ex hexapod-mates: {e}");
            }
        }
        other => warn!("linked-ex: unknown {other:?}"),
    }
}
