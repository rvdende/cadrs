//! P3E.4 (TD12.4–TD12.7, X9): workspaces, branches and merge on the document history, with the
//! branch-and-merge stand-in ([`cadrs_core::samples::gasket`]): version V1 → branch "Alternate
//! Gasket Thickness" → the gasket 2 → 1 mm in the branch → merge into Main, replacing the
//! Gasket tab → Restore to before the merge.

use cadrs_core::assembly::InstanceSource;
use cadrs_core::command::Command;
use cadrs_core::history_log::{HistoryLog, Origin, RestoreDocument, WorkspaceId, document_hash};
use cadrs_core::samples::gasket as g;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::workspace_merge::{self as wm, MergeWorkspace, TabChange};
use cadrs_core::{Document, DocumentMeta, History, Store};

const BRANCH: &str = "Alternate Gasket Thickness";

struct Session {
    doc: Document,
    undo: History,
    log: HistoryLog,
    time: i64,
}

impl Session {
    fn new() -> Self {
        let doc = g::document().unwrap();
        let log = HistoryLog::start(&doc, 1_000, "me");
        Self { doc, undo: History::default(), log, time: 1_000 }
    }

    fn commit(&mut self) {
        self.time += 10;
        let label = self.undo.undo_label().unwrap_or_default().to_string();
        let origin = match label.strip_prefix(wm::MERGE_LABEL) {
            Some(src) => Origin::Merge(src.to_string()),
            None => match label.strip_prefix("Restore to ") {
                Some(e) => Origin::Restore(e.to_string()),
                None => Origin::Command(label),
            },
        };
        self.log.record(&self.doc, origin, self.time, "me");
    }

    fn run(&mut self, c: &dyn Command) {
        self.undo.execute(&mut self.doc, c).unwrap();
        self.commit();
    }

    fn edit(&mut self, f: impl FnOnce(&mut DocHistory)) {
        f(&mut DocHistory(&mut self.doc, &mut self.undo));
        self.commit();
    }

    /// Opens workspace `ws`: its state and a fresh undo stack.
    fn switch(&mut self, ws: WorkspaceId) {
        self.doc = self.log.switch_to(ws).unwrap();
        self.undo = History::default();
    }
}

/// V1, then the branch from it, switched to, with the gasket at 1 mm and the Manifold's
/// material changed. Returns the branch.
fn branched(s: &mut Session) -> WorkspaceId {
    let v1 = s.log.create_version("V1", "Gasket 2 mm", s.time, "me");
    let b = s.log.branch(v1, BRANCH, "", s.time + 5, "me").unwrap();
    s.switch(b);
    s.edit(|h| g::set_depth(h, g::GASKET, g::GASKET_EXTRUDE, g::THIN_GASKET_T).unwrap());
    s.run(&cadrs_core::commands::RenameElement { id: g::MANIFOLD, name: "Manifold (cast)".into() });
    b
}

fn gasket_volume(doc: &Document) -> f64 {
    let features = doc.element(g::GASKET).unwrap().features();
    let build = cadrs_core::rebuild::build(features);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let part = build.parts.iter().find(|p| p.id == g::GASKET_PART).expect("the gasket part keeps its id");
    part.mass.as_ref().map(|m| m.volume).expect("a kernel")
}

#[test]
fn a_branch_starts_as_its_version() {
    let mut s = Session::new();
    s.edit(|h| g::set_depth(h, g::MANIFOLD, g::MANIFOLD_EXTRUDE, 25.0).unwrap());
    let at_v1 = s.doc.clone();
    let v1 = s.log.create_version("V1", "", s.time, "me");
    s.edit(|h| g::set_depth(h, g::MANIFOLD, g::MANIFOLD_EXTRUDE, 30.0).unwrap());
    let b = s.log.branch(v1, "", "", s.time, "me").unwrap();
    assert_eq!(s.log.workspace_name(b), "Branch 1");
    assert_eq!(s.log.current_workspace(), WorkspaceId::MAIN, "branching doesn't switch");
    assert_eq!(s.log.workspace_head(b).unwrap(), at_v1, "a copy of V1, not of Main's head");
    s.switch(b);
    assert_eq!(s.doc, at_v1);
    assert_eq!(s.log.entries.len(), 1);
    assert_eq!(s.log.entries[0].label, "Branch from V1");
}

#[test]
fn an_edit_in_a_branch_leaves_main_unchanged() {
    let mut s = Session::new();
    let main_before = document_hash(&s.doc);
    let b = branched(&mut s);
    let main = s.log.workspace_head(WorkspaceId::MAIN).unwrap();
    assert_eq!(document_hash(&main), main_before, "Main's state is untouched");
    assert_ne!(document_hash(&s.doc), main_before);
    assert_eq!(s.log.workspace_entries(b).unwrap().len(), 3, "Start and two changes of its own");
    assert_eq!(s.log.workspace_entries(WorkspaceId::MAIN).unwrap().len(), 1, "Main has only its Start");
    let area = g::gasket_area();
    assert!((gasket_volume(&main) - area * g::GASKET_T).abs() < 1e-3);
    assert!((gasket_volume(&s.doc) - area * g::THIN_GASKET_T).abs() < 1e-3);
    // Back to Main: its own state, then switching again gives the branch back as it was.
    let branch_head = s.doc.clone();
    s.switch(WorkspaceId::MAIN);
    assert_eq!(document_hash(&s.doc), main_before);
    s.switch(b);
    assert_eq!(s.doc, branch_head);
}

#[test]
fn merge_replaces_exactly_the_chosen_tabs() {
    let mut s = Session::new();
    let b = branched(&mut s);
    let source = s.doc.clone();
    s.switch(WorkspaceId::MAIN);
    let dest = s.doc.clone();
    let base = s.log.merge_base(b, WorkspaceId::MAIN);
    let changed = wm::changed_tabs(base.as_ref(), &source, &dest);
    let ids: Vec<_> = changed.iter().map(|c| (c.element, c.change)).collect();
    assert_eq!(ids, vec![(g::GASKET, TabChange::Changed), (g::MANIFOLD, TabChange::Changed)], "the assembly didn't change");
    // Replace the Gasket, keep the Manifold.
    let merged = wm::merge(&dest, &source, &[g::GASKET]);
    let hash = |d: &Document, id| ron::to_string(d.element(id).unwrap()).unwrap();
    assert_eq!(hash(&merged, g::GASKET), hash(&source, g::GASKET), "replaced from the branch");
    assert_eq!(hash(&merged, g::MANIFOLD), hash(&dest, g::MANIFOLD), "kept");
    assert_eq!(hash(&merged, g::ASSEMBLY), hash(&dest, g::ASSEMBLY), "kept");
    assert_eq!(merged.elements.iter().map(|e| e.id).collect::<Vec<_>>(), dest.elements.iter().map(|e| e.id).collect::<Vec<_>>(), "tab order");
    // The kept assembly's gasket instance still names a part of the replaced studio.
    let asm = merged.element(g::ASSEMBLY).unwrap().assembly_model().unwrap();
    let inst = asm.instances.iter().find(|i| i.id == g::GASKET_INSTANCE).unwrap();
    let InstanceSource::Part { element, part } = &inst.source else { panic!("a part instance") };
    assert_eq!((*element, *part), (g::GASKET, g::GASKET_PART));
    assert!((gasket_volume(&merged) - g::gasket_area() * g::THIN_GASKET_T).abs() < 1e-3, "the instance's part is the 1 mm gasket");
}

#[test]
fn a_merge_is_one_entry_and_restore_undoes_it() {
    let mut s = Session::new();
    let b = branched(&mut s);
    let source = s.doc.clone();
    s.switch(WorkspaceId::MAIN);
    let before = document_hash(&s.doc);
    let k_before = s.log.head_index();
    let merged = wm::merge(&s.doc, &source, &[g::GASKET, g::MANIFOLD]);
    let n = s.log.len();
    s.run(&MergeWorkspace { state: Box::new(merged.clone()), source: s.log.workspace_name(b) });
    assert_eq!(s.log.len(), n + 1, "one entry");
    let e = s.log.entries.last().unwrap();
    assert_eq!((e.action.as_str(), e.label.as_str()), ("Merge", "Merge from Alternate Gasket Thickness"));
    assert_eq!(e.hash, document_hash(&merged));
    // Undo is one step too.
    assert_eq!(s.undo.undo_len(), 1);
    // TD12.7: Restore to the entry before the merge.
    let state = s.log.state_at(k_before).unwrap();
    let label = s.log.entries[k_before].label.clone();
    s.run(&RestoreDocument { state: Box::new(state), entry: label });
    assert_eq!(document_hash(&s.doc), before, "Main is back at its pre-merge state");
    assert_eq!(s.log.entries.last().unwrap().hash, before);
    // The branch is unchanged by all this.
    assert_eq!(s.log.workspace_head(b).unwrap(), source);
}

#[test]
fn workspaces_survive_save_and_reload() {
    let store = Store::new(std::env::temp_dir().join(format!("cadrs-workspaces-{}-{}", std::process::id(), uuid::Uuid::new_v4())));
    let mut s = Session::new();
    let b = branched(&mut s);
    // A version in the branch.
    let v2 = s.log.create_version("V2", "1 mm", s.time, "me");
    let mut meta = DocumentMeta::new("me", 1_000);
    meta.workspace = Some(s.log.current_name());
    store.save(&s.doc, &meta).unwrap();
    s.log.save(&store).unwrap();
    let back = HistoryLog::load(&store, g::DOCUMENT).unwrap().unwrap();
    assert_eq!(back.current_workspace(), b);
    assert_eq!(back.current_name(), BRANCH);
    assert_eq!(back.branches().len(), 1);
    assert_eq!(back.workspace_head(WorkspaceId::MAIN), s.log.workspace_head(WorkspaceId::MAIN));
    assert_eq!(back.workspace_head(b).unwrap(), s.doc);
    assert_eq!(back.version(v2).unwrap().workspace(), b);
    assert_eq!(back.document_at_version(v2).unwrap(), s.doc);
    assert_eq!(back.document_at_version(back.versions()[0].id()).unwrap(), back.workspace_head(WorkspaceId::MAIN).unwrap());
    let file = store.load(g::DOCUMENT).unwrap();
    assert_eq!(file.meta.workspace_name(), BRANCH);
    assert_eq!(file.document, s.doc, "the document file holds the open workspace");
    let _ = std::fs::remove_dir_all(store.root());
}

/// A log from before workspaces (Main only; the fixture is even schema 1) reads as Main, and
/// is written in the form it had: none of the workspace fields, the same text on every save.
#[test]
fn an_old_main_only_log_loads_and_saves_unchanged() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/conrod_standin.history.ron"));
    let log = HistoryLog::load_path(path).unwrap();
    assert_eq!(log.current_workspace(), WorkspaceId::MAIN);
    assert!(log.branches().is_empty());
    assert_eq!(log.current_name(), "Main");
    assert!(log.versions().iter().all(|v| v.workspace() == WorkspaceId::MAIN));
    let out = std::env::temp_dir().join(format!("cadrs-old-log-{}-{}.ron", std::process::id(), uuid::Uuid::new_v4()));
    log.save_path(&out).unwrap();
    let first = std::fs::read_to_string(&out).unwrap();
    for field in ["current:", "workspaces:", "parked:", "workspace:"] {
        assert!(!first.contains(field), "{field} written for a Main-only log");
    }
    let again = HistoryLog::load_path(&out).unwrap();
    assert_eq!(again.entries, log.entries);
    again.save_path(&out).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), first);
    let _ = std::fs::remove_file(&out);
    // And an old document file (no workspace in its metadata) lists as Main.
    let meta: DocumentMeta = ron::from_str("(created: 1, modified: 1, created_by: \"a\", modified_by: \"a\", owned_by: \"a\")").unwrap();
    assert_eq!(meta.workspace_name(), "Main");
    assert!(!ron::to_string(&meta).unwrap().contains("workspace"));
}

/// P3E.4 judge: after a merge that replaces the Gasket tab, the kept assembly's gasket
/// instance resolves (through the assembly's occurrences) to the branch's 1 mm part:
/// V = (60·40 − π(10² + 2·4²))·1 = 2400 − 132π = 1985.31 mm³, written out again here.
#[test]
fn the_merged_assemblys_gasket_instance_is_the_1_mm_part() {
    let mut s = Session::new();
    let b = branched(&mut s);
    let source = s.doc.clone();
    s.switch(WorkspaceId::MAIN);
    let merged = wm::merge(&s.doc, &source, &[g::GASKET]);
    s.run(&MergeWorkspace { state: Box::new(merged), source: s.log.workspace_name(b) });
    let asm = s.doc.element(g::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let occ = cadrs_core::assembly::structure::occurrences(&s.doc, &asm).into_iter().find(|o| o.id == g::GASKET_INSTANCE).expect("the gasket occurrence");
    let build = cadrs_core::rebuild::build(s.doc.element(occ.element).unwrap().features());
    let part = build.part(occ.part).expect("the instance's part resolves");
    let want = 2400.0 - 132.0 * std::f64::consts::PI;
    let v = part.mass.as_ref().map(|m| m.volume).expect("a kernel");
    assert!((v - want).abs() < 1e-3, "{v} vs {want}");
    assert_eq!(format!("{v:.2}"), "1985.31");
    // The solid the assembly draws (its tessellation) encloses the same volume.
    let doc = s.doc.clone();
    let solids = cadrs_core::assembly::occurrence_solids(&doc, &asm, |e| Some(cadrs_core::rebuild::build(doc.element(e)?.features())));
    let tess = solids.get(&g::GASKET_INSTANCE).expect("drawn").volume();
    assert!((tess - want).abs() / want < 1e-2, "{tess}");
}

/// P3E.4 judge: a Main-only log written in schema 2 before workspaces existed
/// (`fixtures/main_only_v2.history.ron`, saved by the app before P3E.4) loads and saves
/// byte-identical: no workspace fields appear.
#[test]
fn a_schema_2_main_only_log_saves_byte_identical() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/main_only_v2.history.ron"));
    let original = std::fs::read_to_string(path).unwrap();
    assert!(original.starts_with("(version:2,"), "a schema-2 log");
    let log = HistoryLog::load_path(path).unwrap();
    assert!(!log.upgraded());
    assert_eq!(log.current_workspace(), WorkspaceId::MAIN);
    assert!(log.branches().is_empty());
    let out = std::env::temp_dir().join(format!("cadrs-v2-log-{}-{}.ron", std::process::id(), uuid::Uuid::new_v4()));
    log.save_path(&out).unwrap();
    let saved = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    assert_eq!(saved, original, "saved byte-identical");
}
