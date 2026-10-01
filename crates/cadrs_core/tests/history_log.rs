//! P3D.3 (IR5.6, X7, T9, TD12): the persisted document history. Every committed change is an
//! entry; any entry's state rebuilds exactly (by hash and by value) from the full copies and
//! the deltas; Restore is a new, undoable entry; the log survives a save and a reload.

use cadrs_core::commands::{AddElement, AddSketch, EditSketch, NewElementKind, RenameDocument, RenameElement};
use cadrs_core::history_log::{HistoryLog, Origin, RestoreDocument, SNAPSHOT_EVERY, document_hash};
use cadrs_core::{Command, Document, ElementId, FeatureId, History};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Session {
    doc: Document,
    undo: History,
    log: HistoryLog,
    time: i64,
    /// The document after each entry, as it really was.
    states: Vec<Document>,
}

impl Session {
    fn new() -> Self {
        let doc = Document::new("History");
        let log = HistoryLog::start(&doc, 1_000, "me");
        Self { states: vec![doc.clone()], doc, undo: History::new(10_000), log, time: 1_000 }
    }

    fn commit(&mut self, origin: Origin) {
        self.time += 7;
        if self.log.record(&self.doc, origin, self.time, "me").is_some() {
            self.states.push(self.doc.clone());
        }
    }

    fn run(&mut self, c: &dyn Command) {
        self.undo.execute(&mut self.doc, c).unwrap();
        let label = self.undo.undo_label().unwrap_or_default().to_string();
        self.commit(Origin::Command(label));
    }

    fn undo(&mut self) {
        let label = self.undo.undo(&mut self.doc).unwrap();
        self.commit(Origin::Undo(label));
    }
}

/// About 200 steps of every kind: sketches drawn line by line, tabs added and renamed, the
/// document renamed, undo and redo.
fn two_hundred_steps() -> Session {
    let mut s = Session::new();
    let ps = s.doc.elements[0].id;
    let mut sketch = FeatureId::new();
    s.run(&AddSketch { element: ps, feature: sketch, plane: Some(PlaneRef::Top) });
    for i in 0..196 {
        match i % 25 {
            7 => s.run(&RenameDocument { name: format!("History {i}") }),
            11 => s.run(&AddElement { id: ElementId::new(), kind: NewElementKind::PartStudio, name: None, after: None }),
            13 => s.run(&RenameElement { id: ps, name: format!("Studio {i}") }),
            17 => s.undo(),
            19 => {
                sketch = FeatureId::new();
                s.run(&AddSketch { element: ps, feature: sketch, plane: Some(PlaneRef::Front) });
            }
            _ => {
                let x = i as f64;
                s.run(&EditSketch {
                    element: ps,
                    feature: sketch,
                    op: SketchOp::AddPolyline {
                        points: vec![Vec2::new(x, 0.0), Vec2::new(x, 10.0 + x)],
                        closed: false,
                        construction: false,
                        label: "Add line",
                    },
                });
            }
        }
    }
    s
}

#[test]
fn every_entry_rebuilds_exactly() {
    let s = two_hundred_steps();
    assert!(s.log.len() >= 190, "{} entries", s.log.len());
    assert_eq!(s.states.len(), s.log.len());
    // Full copies every SNAPSHOT_EVERY entries, so a state is at most that many deltas away.
    let snaps = s.log.snapshot_indices();
    assert_eq!(snaps.len(), (s.log.len() - 1) / SNAPSHOT_EVERY + 1);
    for (k, want) in s.states.iter().enumerate() {
        let got = s.log.state_at(k).unwrap();
        assert_eq!(document_hash(&got), s.log.entries[k].hash, "entry {k}");
        assert_eq!(document_hash(&got), document_hash(want), "entry {k}");
        assert_eq!(&got, want, "entry {k}");
    }
    // Entries read like Onshape's: tab :: action : feature.
    let labels: Vec<&str> = s.log.entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels[0], "Start");
    assert_eq!(labels[1], "Part Studio 1 :: Insert : Sketch 1");
    assert_eq!(labels[2], "Part Studio 1 :: Edit : Sketch 1");
    assert!(labels.contains(&"Part Studio 2 :: Insert"), "{labels:?}");
    assert!(labels.iter().any(|l| l.starts_with("Studio 13 :: Rename") || *l == "Part Studio 1 :: Rename"), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains(":: Undo")), "{labels:?}");
}

#[test]
fn restoring_an_entry_reproduces_it_and_is_undoable() {
    let mut s = two_hundred_steps();
    for k in [0, 1, 37, SNAPSHOT_EVERY, 150, s.log.len() - 2] {
        let before = s.doc.clone();
        let n = s.log.len();
        let state = s.log.state_at(k).unwrap();
        let entry = s.log.entries[k].label.clone();
        s.undo.execute(&mut s.doc, &RestoreDocument { state: Box::new(state), entry: entry.clone() }).unwrap();
        s.commit(Origin::Restore(entry.clone()));
        // A new entry, whose state is entry k's.
        assert_eq!(s.log.len(), n + 1, "restore {k}");
        assert_eq!(s.log.entries[n].action, "Restore");
        assert_eq!(s.log.entries[n].label, format!("Restore : {entry}"));
        assert_eq!(s.log.entries[n].hash, s.log.entries[k].hash, "restore {k} reproduces entry {k}");
        assert_eq!(document_hash(&s.log.state_at(n).unwrap()), s.log.entries[k].hash);
        // Undo takes it back (one step), as a new entry of its own.
        s.undo();
        assert_eq!(s.doc, before);
        assert_eq!(s.log.entries.last().unwrap().hash, document_hash(&before));
    }
}

#[test]
fn history_survives_save_and_reload() {
    let s = two_hundred_steps();
    let mut log = s.log.clone();
    log.note_healthy(s.doc.elements[0].id, [FeatureId::from_u128(9)]);
    let dir = std::env::temp_dir().join(format!("cadrs-history-{}", std::process::id()));
    let store = cadrs_core::Store::new(&dir);
    log.save(&store).unwrap();
    let back = HistoryLog::load(&store, log.document).unwrap().expect("the log is there");
    assert_eq!(back.entries, log.entries);
    assert_eq!(back.healthy, log.healthy);
    assert_eq!(back.last_healthy(s.doc.elements[0].id, FeatureId::from_u128(9)), Some(log.head_index()));
    for k in (0..log.len()).step_by(13).chain([log.len() - 1]) {
        assert_eq!(back.state_at(k), log.state_at(k), "entry {k}");
    }
    // Reloaded, it goes on appending where it left off.
    let mut back = back;
    let mut doc = s.doc.clone();
    doc.name = "After reload".into();
    let i = back.record(&doc, Origin::Command("Rename document".into()), 99_999, "me").unwrap();
    assert_eq!(i, log.len());
    assert_eq!(back.state_at(i).unwrap().name, "After reload");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Versions (P3D.3): a named pointer to an entry, whose document never changes after later
/// edits (even a Restore to before it), and which survives a save and a reload.
#[test]
fn a_version_is_immutable_and_persists() {
    let mut s = two_hundred_steps();
    let at = s.log.head_index();
    let want = s.doc.clone();
    let v1 = s.log.create_version("Release 1", "  before the rework ", 5_000, "me");
    let v = s.log.version(v1).unwrap().clone();
    assert_eq!((v.name(), v.description(), v.entry(), v.user()), ("Release 1", "before the rework", at, "me"));
    // Later edits, an undo and a Restore to an earlier entry.
    let ps = s.doc.elements[0].id;
    s.run(&RenameElement { id: ps, name: "Changed".into() });
    s.run(&RenameDocument { name: "After V1".into() });
    s.undo();
    let early = s.log.state_at(3).unwrap();
    s.undo.execute(&mut s.doc, &RestoreDocument { state: Box::new(early), entry: "3".into() }).unwrap();
    s.commit(Origin::Restore("3".into()));
    assert_ne!(s.doc, want);
    assert_eq!(s.log.document_at_version(v1).unwrap(), want);
    assert_eq!(s.log.version(v1).unwrap(), &v);
    // An unnamed version is "V2".
    let v2 = s.log.create_version(" ", "", 6_000, "me");
    assert_eq!(s.log.version(v2).unwrap().name(), "V2");
    assert_eq!(s.log.document_at_version(v2).unwrap(), s.doc);
    // Saved and read back.
    let dir = std::env::temp_dir().join(format!("cadrs-versions-{}", std::process::id()));
    let store = cadrs_core::Store::new(&dir);
    s.log.save(&store).unwrap();
    let back = HistoryLog::load(&store, s.log.document).unwrap().unwrap();
    assert_eq!(back.versions(), s.log.versions());
    assert_eq!(back.document_at_version(v1).unwrap(), want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_part_studio_change_stores_only_its_features() {
    let s = two_hundred_steps();
    let edit = &s.log.entries[2];
    assert_eq!(edit.label, "Part Studio 1 :: Edit : Sketch 1");
    let d = &edit.delta;
    assert!(d.elements.is_empty(), "the studio is a patch, not a copy");
    assert_eq!(d.studios.len(), 1);
    let p = &d.studios[0];
    assert_eq!(p.id, s.doc.elements[0].id);
    assert!(p.shell.is_none() && p.order.is_none(), "only a feature changed");
    assert_eq!(p.features.len(), 1, "just the edited sketch");
    // A rename changes the element's shell, not its features.
    let rename = s.log.entries.iter().find(|e| e.label.contains(":: Rename")).unwrap();
    assert!(rename.delta.studios.iter().all(|p| p.shell.is_some() && p.features.is_empty()));
}

#[test]
fn a_version_1_log_is_rewritten_with_patches_on_load() {
    let s = two_hundred_steps();
    // The log as version 1 wrote it: every changed Part Studio as a whole copy.
    let mut old = s.log.clone();
    old.version = 1;
    for (k, e) in old.entries.iter_mut().enumerate().skip(1) {
        for p in std::mem::take(&mut e.delta.studios) {
            e.delta.elements.push(s.states[k].element(p.id).unwrap().clone());
        }
    }
    assert!(old.entries.iter().all(|e| e.delta.studios.is_empty()));
    let dir = std::env::temp_dir().join(format!("cadrs-history-v1-{}", std::process::id()));
    let store = cadrs_core::Store::new(&dir);
    old.save(&store).unwrap();
    let back = HistoryLog::load(&store, old.document).unwrap().expect("the log is there");
    assert!(back.upgraded());
    assert_eq!(back.version, cadrs_core::history_log::HISTORY_VERSION);
    let deltas = |l: &HistoryLog| l.entries.iter().map(|e| e.delta.clone()).collect::<Vec<_>>();
    assert_eq!(deltas(&back), deltas(&s.log), "the same deltas as written today");
    for (k, want) in s.states.iter().enumerate() {
        assert_eq!(back.state_at(k).as_ref(), Some(want), "entry {k}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
