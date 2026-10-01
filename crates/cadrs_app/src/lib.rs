//! cadrs_app: the Bevy plugins that make up the application (states, landing screen, document
//! shell, viewport), built from `cadrs_ui` components.

// Bevy's `AsBindGroup` derive (`part_shading`) nests deeper than the default 128.
#![recursion_limit = "256"]

pub mod analysis;
pub mod appearance;
pub mod advanced;
pub mod assembly;
pub mod advanced_dialog;
pub mod applied;
pub mod applied_dialog;
pub mod camera;
pub mod create_selection;
pub mod cursor;
pub mod document;
pub mod drawing;
pub mod draft_ui;
pub mod boolean;
pub mod derived_ui;
pub mod extrude;
pub mod extrude_dialog;
pub mod export_dialog;
pub mod feature_folders;
pub mod feature_list;
pub mod feature_menu;
pub mod revolve;
pub mod revolve_dialog;
pub mod gallery;
pub mod history_panel;
pub mod import_dialog;
pub mod import_file;
pub mod landing;
pub mod linked;
pub mod linked_exercises;
pub mod linked_session;
pub mod move_document;
pub mod tab_folders;
pub mod tab_manager;
pub mod rebuild_indicator;
pub mod reference_manager;
pub mod manipulator;
pub mod mass_props;
pub mod measure;
pub mod material_dialog;
pub mod panel_tab;
pub mod part_shading;
pub mod parts;
pub mod pattern;
pub mod pattern_dialog;
pub mod pcb;
pub mod preferences_ui;
pub mod surfacing_ui;
pub mod transform_ui;
pub mod properties_dialog;
pub mod parts_list;
pub mod plane_display;
pub mod region_select;
pub mod render_ui;
pub mod export_image;
pub mod repair;
pub mod replace_reference;
pub mod selection_readout;
pub mod script;
pub mod search_tools;
pub mod shortcuts;
pub mod simulation_ui;
pub mod sketch;
pub mod sketch_constrain;
pub mod sketch_diagnostics;
pub mod sketch_dimension;
pub mod sketch_draw;
pub mod sketch_edit_tools;
pub mod sketch_entity_tools;
pub mod sketch_glyphs;
pub mod sketch_links;
pub mod sketch_text;
pub mod sketch_modify_tools;
pub mod sketch_tools;
pub mod thumbnail;
pub mod tool_input;
pub mod composite_ui;
pub mod scale_ui;
pub mod threads_ui;
pub mod units_dialog;
pub mod variables_ui;
pub mod view_cube;
pub mod viewport_menu;
pub mod viewport;
pub mod view_options;
pub mod section_view;
pub mod hidden_edges;
pub mod workspaces;

use bevy::prelude::*;
use cadrs_core::{Document, DocumentMeta, Element, ElementId, History, Store, Timestamp};

/// Top-level screens.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// The documents page.
    #[default]
    Landing,
    /// An open document (Part Studio / Assembly tabs).
    Document,
    /// Hidden: the `cadrs_ui` component gallery (`--scenario ui_gallery`).
    Gallery,
}

impl AppState {
    /// Parses a state name as used by scenarios (`"landing"`, `"document"`, `"gallery"`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "landing" => Some(AppState::Landing),
            "document" => Some(AppState::Document),
            "gallery" => Some(AppState::Gallery),
            _ => None,
        }
    }
}

/// The state to start in, if not [`AppState::Landing`] (set by the harness from a scenario).
#[derive(Resource, Debug, Clone)]
pub struct StartState(pub String);

/// Where documents are stored. `main` sets it from `--data-dir`, `CADRS_DATA_DIR`, the
/// scenario's private data dir, or the platform default.
#[derive(Resource, Debug, Clone)]
pub struct DocumentStore(pub Store);

/// The current time. Scenarios pin it so dates in screenshots never change.
#[derive(Resource, Debug, Clone, Copy)]
pub struct AppClock {
    fixed: Option<Timestamp>,
    /// Seconds east of UTC used to display dates.
    pub utc_offset: i64,
}

impl AppClock {
    /// The system clock and the local timezone.
    pub fn system() -> Self {
        use chrono::Offset;
        Self {
            fixed: None,
            utc_offset: chrono::Local::now().offset().fix().local_minus_utc() as i64,
        }
    }

    /// A clock stuck at `now`, displaying UTC.
    pub fn fixed(now: Timestamp) -> Self {
        Self {
            fixed: Some(now),
            utc_offset: 0,
        }
    }

    pub fn now(&self) -> Timestamp {
        self.fixed.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        })
    }

    /// A "Modified" column date.
    pub fn format(&self, ts: Timestamp) -> String {
        cadrs_core::time::format_modified(ts, self.now(), self.utc_offset)
    }
}

/// The local user. `id` is what documents store as owner; `display_name` is shown in the top
/// bar.
#[derive(Resource, Debug, Clone)]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
}

impl UserProfile {
    /// The OS user.
    pub fn system() -> Self {
        let id = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".into());
        let mut display_name = id.clone();
        if let Some(first) = display_name.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        Self { id, display_name }
    }

    /// How a user id is shown in the list: "me" for the local user.
    pub fn display(&self, user: &str) -> String {
        if user == self.id {
            "me".into()
        } else {
            user.into()
        }
    }
}

/// The active document's workspace units (X1), kept in sync by
/// [`document::DocumentPlugin`]: dimension values, live labels, quick-dimension boxes and the
/// area readout are shown in them, and bare numbers typed into value boxes are read in them.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct WorkspaceUnits(pub cadrs_sketch::units::Units);

/// Where exports go instead of [`cadrs_core::export::default_dir`] (scenarios: their output
/// folder, so an exported BOM lands next to the screenshots).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ExportDirOverride(pub Option<std::path::PathBuf>);

/// The folder an Export… dialog starts with: `$CADRS_EXPORT_DIR`, else [`ExportDirOverride`]
/// (every scenario and headless run sets it to its output folder, so they never write into the
/// user's Downloads), else the Downloads folder. Shown relative to the working directory when it
/// is under it.
pub fn export_dir(world: &World) -> Option<std::path::PathBuf> {
    let env = std::env::var_os("CADRS_EXPORT_DIR").filter(|d| !d.is_empty()).map(std::path::PathBuf::from);
    let dir = env
        .or_else(|| world.get_resource::<ExportDirOverride>().and_then(|d| d.0.clone()))
        .or_else(cadrs_core::export::default_dir)?;
    // "scenarios/../target/…" read as "target/…".
    let mut clean = std::path::PathBuf::new();
    for c in dir.components() {
        match c {
            std::path::Component::ParentDir if clean.file_name().is_some() => {
                clean.pop();
            }
            std::path::Component::CurDir => {}
            other => clean.push(other),
        }
    }
    let rel = std::env::current_dir().ok().and_then(|cwd| clean.strip_prefix(&cwd).ok().map(std::path::Path::to_path_buf));
    Some(rel.filter(|r| !r.as_os_str().is_empty()).unwrap_or(clean))
}

/// The open document and its undo history. Every edit goes through
/// [`ActiveDocument::execute`].
#[derive(Resource, Debug, Clone)]
pub struct ActiveDocument {
    pub doc: Document,
    pub history: History,
    /// The selected tab.
    pub active: Option<ElementId>,
    /// Metadata when the document lives in the [`DocumentStore`]; `None` for scratch documents.
    pub meta: Option<DocumentMeta>,
    /// The document as last saved, to detect changes.
    saved: Document,
    /// The document as it was opened (a new thumbnail is rendered only if it changed).
    opened: Document,
    /// Render a thumbnail when closing even if nothing changed (a document just created).
    pub fresh: bool,
    last_active_index: usize,
    /// The tab that was active before each undo step (parallel to the undo stack), so undoing
    /// a delete or a create brings back the tab the user was on.
    undo_active: Vec<Option<ElementId>>,
    /// The tab that was active before each undone step (parallel to the redo stack).
    redo_active: Vec<Option<ElementId>>,
    /// P3G.3 (DV1.9): set while a linked document is open read-only at a version
    /// ([`linked_session`]): what is shown ("V1 of Block source"). Every command, undo and redo
    /// is refused.
    pub read_only: Option<String>,
    /// How many edits were refused while read-only (the session says so each time).
    pub refused: u32,
}

/// P3E.4: a workspace's undo and redo stacks, kept while another workspace is open.
#[derive(Default)]
pub struct WorkspaceUndo {
    history: History,
    undo_active: Vec<Option<ElementId>>,
    redo_active: Vec<Option<ElementId>>,
}

/// Why an edit of a read-only document is refused.
pub const READ_ONLY: &str = "This version is open read-only: it can't be edited";

impl ActiveDocument {
    /// A scratch document that is not saved.
    pub fn new(mut doc: Document) -> Self {
        // Derived features take their sources' snapshots (not an edit).
        cadrs_core::derived::resolve_document(&mut doc);
        Self {
            saved: doc.clone(),
            opened: doc.clone(),
            fresh: false,
            active: doc.elements.first().map(|e| e.id),
            doc,
            history: History::default(),
            meta: None,
            last_active_index: 0,
            undo_active: Vec::new(),
            redo_active: Vec::new(),
            read_only: None,
            refused: 0,
        }
    }

    /// A stored document.
    pub fn stored(doc: Document, meta: DocumentMeta) -> Self {
        Self {
            meta: Some(meta),
            ..Self::new(doc)
        }
    }

    pub fn execute(
        &mut self,
        cmd: &dyn cadrs_core::Command,
    ) -> Result<(), cadrs_core::CommandError> {
        if self.read_only.is_some() {
            self.refused += 1;
            return Err(cadrs_core::CommandError::Invalid(READ_ONLY.into()));
        }
        self.remember_active_index();
        let before = self.active;
        let (undo0, redo0) = (self.history.undo_len(), self.history.redo_len());
        let r = self.history.execute(&mut self.doc, cmd);
        // Recorded if the undo stack grew or the redo stack was cleared (at the limit the
        // oldest step is dropped, so the length alone can stay the same).
        let recorded = r.is_ok()
            && (self.history.undo_len() != undo0 || (redo0 > 0 && self.history.redo_len() == 0));
        if recorded {
            self.undo_active.push(before);
            self.redo_active.clear();
            let keep = self.history.undo_len();
            if self.undo_active.len() > keep {
                let extra = self.undo_active.len() - keep;
                self.undo_active.drain(..extra);
            }
        }
        self.fix_active();
        r
    }

    /// Undoes the last command. Returns its label.
    pub fn undo(&mut self) -> Option<String> {
        if self.read_only.is_some() {
            self.refused += 1;
            return None;
        }
        self.remember_active_index();
        let current = self.active;
        let r = self.history.undo(&mut self.doc);
        if r.is_some() {
            if let Some(Some(before)) = self.undo_active.pop()
                && self.doc.element(before).is_some()
            {
                self.active = Some(before);
            }
            self.redo_active.push(current);
        }
        self.fix_active();
        r
    }

    /// Redoes the last undone command. Returns its label.
    pub fn redo(&mut self) -> Option<String> {
        if self.read_only.is_some() {
            self.refused += 1;
            return None;
        }
        self.remember_active_index();
        let current = self.active;
        let r = self.history.redo(&mut self.doc);
        if r.is_some() {
            if let Some(Some(after)) = self.redo_active.pop()
                && self.doc.element(after).is_some()
            {
                self.active = Some(after);
            }
            self.undo_active.push(current);
        }
        self.fix_active();
        r
    }

    /// Merges the undo steps above the first `mark` into one called `label` (see
    /// [`History::squash_since`]), keeping the per-step active tabs in step.
    pub fn squash_since(&mut self, mark: usize, label: impl Into<String>) -> bool {
        let first = self.undo_active.get(mark).copied().flatten();
        if !self.history.squash_since(mark, label) {
            return false;
        }
        self.undo_active.truncate(mark);
        if self.history.undo_len() > mark {
            self.undo_active.push(first.or(self.active));
        }
        self.redo_active.clear();
        true
    }

    /// Merges the steps above `mark` that edit `element`'s contents into one step called
    /// `label` (see [`History::squash_element_since`]); other steps (renames, new tabs) stay.
    pub fn squash_element_since(
        &mut self,
        mark: usize,
        element: ElementId,
        label: impl Into<String>,
    ) {
        let (idx, pushed) = self.history.squash_element_since(mark, element, label);
        let first = idx.first().and_then(|i| self.undo_active.get(*i).copied().flatten());
        for i in idx.iter().rev() {
            if *i < self.undo_active.len() {
                self.undo_active.remove(*i);
            }
        }
        if pushed {
            self.undo_active.push(first.or(self.active));
        }
        self.redo_active.clear();
    }

    /// Reverts and forgets the steps above `mark` that edit `element`'s contents (see
    /// [`History::discard_element_since`]); other steps stay.
    pub fn discard_element_since(&mut self, mark: usize, element: ElementId) {
        let idx = self.history.discard_element_since(&mut self.doc, mark, element);
        for i in idx.iter().rev() {
            if *i < self.undo_active.len() {
                self.undo_active.remove(*i);
            }
        }
        self.redo_active.clear();
        self.fix_active();
    }

    /// Drops the undo steps above the first `mark` (the document already reverted them) and
    /// the redo stack.
    pub fn discard_since(&mut self, mark: usize) {
        self.history.discard_since(mark);
        self.undo_active.truncate(mark);
        self.redo_active.clear();
    }

    /// The selected tab.
    pub fn active_element(&self) -> Option<&Element> {
        self.active
            .and_then(|id| self.doc.element(id))
            .or_else(|| self.doc.elements.first())
    }

    /// Selects `id` if it exists.
    pub fn set_active(&mut self, id: ElementId) {
        if self.doc.element(id).is_some() && self.active != Some(id) {
            self.active = Some(id);
        }
    }

    /// If the active tab went away (deleted, or its creation undone), selects the tab that took
    /// its place, like Onshape.
    fn fix_active(&mut self) {
        if self.active.is_some_and(|id| self.doc.element(id).is_some()) {
            return;
        }
        let i = self
            .last_active_index
            .min(self.doc.elements.len().saturating_sub(1));
        self.active = self.doc.elements.get(i).map(|e| e.id);
    }

    /// Remembers the active tab's position, so [`Self::fix_active`] can pick its neighbor.
    fn remember_active_index(&mut self) {
        if let Some(i) = self.active.and_then(|id| self.doc.element_index(id)) {
            self.last_active_index = i;
        }
    }

    /// P3E.4: opens another workspace's state `doc` with its own undo and redo `undo` (empty
    /// the first time), and returns this workspace's. The active tab stays if it's there. The
    /// document file follows on the next save.
    pub fn switch_workspace(&mut self, doc: Document, undo: WorkspaceUndo) -> WorkspaceUndo {
        self.remember_active_index();
        let old = WorkspaceUndo {
            history: std::mem::replace(&mut self.history, undo.history),
            undo_active: std::mem::replace(&mut self.undo_active, undo.undo_active),
            redo_active: std::mem::replace(&mut self.redo_active, undo.redo_active),
        };
        self.doc = doc;
        self.fix_active();
        old
    }

    /// Saves the document and its metadata now (a workspace switch: the file holds the open
    /// workspace and the metadata its name), without touching the modified time.
    pub fn save_now(&mut self, store: &Store) -> Result<(), cadrs_core::StoreError> {
        let Some(meta) = &self.meta else {
            return Ok(());
        };
        store.save(&self.doc, meta)?;
        self.saved = self.doc.clone();
        Ok(())
    }

    /// True if the document changed since it was opened.
    pub fn changed_since_open(&self) -> bool {
        self.doc != self.opened
    }

    /// True if the document changed since it was last saved.
    pub fn is_dirty(&self) -> bool {
        self.meta.is_some() && self.doc != self.saved
    }

    /// Saves the document if it changed since the last save.
    pub fn save_if_changed(
        &mut self,
        store: &Store,
        now: Timestamp,
        user: &str,
    ) -> Result<(), cadrs_core::StoreError> {
        let Some(meta) = &mut self.meta else {
            return Ok(());
        };
        if self.doc == self.saved {
            return Ok(());
        }
        meta.modified = now;
        meta.modified_by = user.to_string();
        store.save(&self.doc, meta)?;
        self.saved = self.doc.clone();
        Ok(())
    }
}

pub struct CadrsAppPlugin;

impl Plugin for CadrsAppPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<DocumentStore>() {
            let root = std::env::var_os("CADRS_DATA_DIR")
                .map(std::path::PathBuf::from)
                .or_else(Store::default_root)
                .unwrap_or_else(|| std::path::PathBuf::from("cadrs-documents"));
            app.insert_resource(DocumentStore(Store::new(root)));
        }
        if !app.world().contains_resource::<AppClock>() {
            app.insert_resource(AppClock::system());
        }
        if !app.world().contains_resource::<UserProfile>() {
            app.insert_resource(UserProfile::system());
        }
        app.init_state::<AppState>()
            .add_sub_state::<sketch::PartStudioMode>()
            .add_plugins((
                sketch::SketchPlugin,
                sketch_tools::SketchToolsPlugin,
                sketch_draw::SketchDrawPlugin,
                sketch_constrain::SketchConstrainPlugin,
                sketch_dimension::SketchDimensionPlugin,
                viewport::ViewportPlugin,
                view_cube::ViewCubePlugin,
                thumbnail::ThumbnailPlugin,
                landing::LandingPlugin,
                document::DocumentPlugin,
                gallery::GalleryPlugin,
                tool_input::ToolInputPlugin,
                cursor::AppCursorPlugin,
                shortcuts::ShortcutsPlugin,
                script::ScriptPlugin,
            ))
            .add_plugins(rebuild_indicator::RebuildIndicatorPlugin)
            .add_plugins((
                parts::PartsPlugin,
                parts_list::PartsListPlugin,
                boolean::BooleanPlugin,
                composite_ui::CompositeUiPlugin,
                mass_props::MassPropsPlugin,
                extrude::ExtrudePlugin,
                revolve::RevolvePlugin,
                region_select::RegionSelectPlugin,
                selection_readout::SelectionReadoutPlugin,
                sketch_edit_tools::SketchEditToolsPlugin,
                sketch_entity_tools::SketchEntityToolsPlugin,
                sketch_modify_tools::SketchModifyToolsPlugin,
                sketch_links::SketchLinksPlugin,
                sketch_text::SketchTextPlugin,
                viewport_menu::ViewportMenuPlugin,
            ))
            .add_plugins((sketch_diagnostics::SketchDiagnosticsPlugin, feature_menu::FeatureMenuPlugin))
            .add_plugins((history_panel::HistoryPlugin, workspaces::WorkspacesPlugin, repair::RepairPlugin, replace_reference::ReplaceReferencePlugin, panel_tab::PanelTabPlugin))
            .add_plugins((appearance::AppearancePlugin, material_dialog::MaterialDialogPlugin, applied::AppliedPlugin, feature_folders::FeatureFoldersPlugin, feature_list::FeatureListPlugin, search_tools::SearchToolsPlugin, plane_display::PlaneDisplayPlugin, create_selection::CreateSelectionPlugin, pattern::PatternPlugin, export_dialog::ExportDialogPlugin, assembly::AssemblyPlugin, properties_dialog::PropertiesDialogPlugin))
            .add_plugins((drawing::DrawingPlugin, linked::LinkedPlugin, reference_manager::ReferenceManagerPlugin, linked_session::LinkedSessionPlugin, move_document::MoveDocumentPlugin, derived_ui::DerivedPlugin))
            .add_plugins((pcb::PcbPlugin, measure::MeasurePlugin, view_options::ViewOptionsPlugin, section_view::SectionViewPlugin, hidden_edges::HiddenEdgesPlugin))
            .add_plugins((tab_folders::TabFoldersPlugin, tab_manager::TabManagerPlugin, analysis::AnalysisPlugin, preferences_ui::PreferencesPlugin, manipulator::ManipulatorPlugin))
            .add_plugins((variables_ui::VariablesPlugin, scale_ui::ScalePlugin, threads_ui::ThreadsPlugin, simulation_ui::SimulationPlugin, render_ui::RenderUiPlugin, export_image::ExportImagePlugin))
            .add_plugins((import_dialog::ImportDialogPlugin, import_file::ImportFilePlugin))
            .init_resource::<ExportDirOverride>()
            // A long menu that has to be capped keeps clear of the tab strip (Final part 3:
            // `course_asm_triad` 08, `course_asm_std_bulk_edit` 02).
            .insert_resource(cadrs_ui::menu::MenuInsets { top: 0.0, bottom: cadrs_ui::Theme::default().tab_bar_height })
            .add_systems(Startup, (apply_start_state, flush_deleted, set_document_loader));
    }
}

/// Permanently deleted documents are kept aside during a session so undo works; they are
/// removed for good the next time the app starts.
/// Derived features load other documents from the store (Onshape import).
fn set_document_loader(store: Res<DocumentStore>) {
    cadrs_core::derived::set_document_loader(Some(cadrs_core::derived::store_loader(store.0.clone())));
}

fn flush_deleted(store: Res<DocumentStore>) {
    if let Err(e) = store.0.flush_deleted() {
        warn!("cannot remove deleted documents: {e}");
    }
}

fn apply_start_state(
    start: Option<Res<StartState>>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    match AppState::from_name(&start.0) {
        Some(AppState::Document) => {
            commands.insert_resource(ActiveDocument::new(Document::new("Untitled document")));
            next.set(AppState::Document);
        }
        Some(s) => next.set(s),
        None => warn!("unknown start state {:?}", start.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::commands::{AddElement, DeleteElement, NewElementKind};

    #[test]
    fn undoing_a_tab_delete_reactivates_the_tab() {
        let mut d = ActiveDocument::new(Document::new("Doc"));
        let asm = d.doc.elements[1].id;
        d.set_active(asm);
        d.execute(&DeleteElement { id: asm }).unwrap();
        assert_ne!(d.active, Some(asm));
        d.undo();
        assert_eq!(d.active, Some(asm));
        // Redo deletes it again and moves to its neighbor.
        d.redo();
        assert!(d.active.is_some() && d.active != Some(asm));
    }

    /// P3G.3 (DV1.9): a linked document opened read-only refuses every edit, undo and redo, and
    /// counts the refusals; the document is unchanged.
    #[test]
    fn a_read_only_document_refuses_edits() {
        let mut d = ActiveDocument::new(Document::new("Doc"));
        let ps = d.doc.elements[0].id;
        d.read_only = Some("V1 of Doc".into());
        let before = d.doc.clone();
        let r = d.execute(&AddElement { id: ElementId::new(), kind: NewElementKind::Assembly, name: None, after: Some(ps) });
        assert_eq!(r.unwrap_err().to_string(), READ_ONLY);
        assert!(d.execute(&DeleteElement { id: ps }).is_err());
        assert_eq!(d.undo(), None);
        assert_eq!(d.redo(), None);
        assert_eq!(d.refused, 4);
        assert_eq!(d.doc, before, "unchanged");
        assert!(!d.is_dirty());
        // Switching tabs is allowed.
        let asm = d.doc.elements[1].id;
        d.set_active(asm);
        assert_eq!(d.active, Some(asm));
    }

    #[test]
    fn undoing_a_create_goes_back_to_the_previous_tab() {
        let mut d = ActiveDocument::new(Document::new("Doc"));
        let ps = d.doc.elements[0].id;
        let id = ElementId::new();
        d.execute(&AddElement {
            id,
            kind: NewElementKind::Assembly,
            name: None,
            after: Some(ps),
        })
        .unwrap();
        d.set_active(id);
        d.undo();
        assert_eq!(d.active, Some(ps));
        d.redo();
        assert_eq!(d.active, Some(id));
    }
}
