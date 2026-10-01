//! P3G.1: linked documents in the app (`derived-and-linking-gaps.md` DV1, DV2, ER1–ER3, ER6.2,
//! ER X2, ER X3, ER X8; the model is [`cadrs_core::external`]).
//!
//! - **The resolver** ([`LinkResolver`]): reads other documents' versions from the store (cached
//!   per file change) and makes the frozen copies a link needs. [`LinkStatus`] keeps whether
//!   each linked document can be reached (trashed, deleted, unreadable: DV1.7), refreshed when
//!   the document changes and every few seconds; the Instances list shows it on the link icon.
//! - **The Other documents browser** (ER1.2, ER1.3, ER1.6), shared by the assembly Insert dialog
//!   and the drawing Insert view browser ([`Browse`], [`spawn_browser`]): the search field (a
//!   document's name, or its id pasted: cadrs's "Copy link" gives the id), the locations **My
//!   documents**, **Recently opened**, **Created by me** and the library's folders, then a
//!   location's documents with their thumbnails and newest version ("V1", or "No versions").
//!   Picking one opens it at its newest version ([`Opened`]: the document at that version and
//!   the copies of each of its tabs), with a back arrow, **Create version** and the **Version
//!   graph** ([`version_graph`]) to pick an older version. A document without versions offers
//!   Create version (DV1.8, ER1.5); the new version is then opened.
//! - **Create version** (ER2.2, ER6.2): the top bar's icon beside the document name opens the
//!   "Create version from Main" dialog (Name, Description, **Create**, **Create version and edit
//!   properties**, **Cancel**); the top bar's versions counter is live.
//!
//! Stand-in icons (icon-rs has no version-link glyphs yet, see `docs/icon-migration.md`): the
//! linked instance icon is `link`, Create version `versions`, Version graph `branches`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::external::{self, LinkSnapshot, LinkState, LinkedElement, Resolver, SourceRef};
use cadrs_core::history_log::VersionId;
use cadrs_core::{Document, DocumentId, ElementId, ElementKind, FolderId};
use cadrs_ui::prelude::*;
use cadrs_ui::{BrowserSearch, DocumentRow, VersionGraph, VersionNode, VersionNodeKind, location_row};

use crate::history_panel::DocLog;
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub struct LinkedPlugin;

impl Plugin for LinkedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LinkStatus>()
            .init_resource::<DocThumbs>()
            .init_resource::<LibrarySnapshot>()
            .add_systems(Update, (sync_versions_counter, refresh_link_status, land_library_reads).run_if(in_state(AppState::Document)))
            .add_systems(Update, close_error_toasts_on_tab_change.run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut s: ResMut<LinkStatus>| *s = LinkStatus::default())
            .add_observer(on_top_bar)
            .add_observer(on_version_dialog_button)
            .add_observer(on_browser_location)
            .add_observer(on_browser_document);
    }
}

// ---------------------------------------------------------------------------------------------
// The resolver and the link states

/// The resolver of the document store (see [`cadrs_core::external::Resolver`]).
#[derive(Resource)]
pub struct LinkResolver(pub Resolver);

/// The resolver, made on first use for the current document store.
pub fn resolver(world: &mut World) -> Mut<'_, LinkResolver> {
    let store = world.resource::<DocumentStore>().0.clone();
    if world.get_resource::<LinkResolver>().is_none_or(|r| r.0.store() != &store) {
        world.insert_resource(LinkResolver(Resolver::new(store)));
    }
    world.resource_mut::<LinkResolver>()
}

/// Whether each document the open document links to can be reached (DV1.7).
#[derive(Resource, Debug, Default, Clone)]
pub struct LinkStatus {
    pub states: HashMap<DocumentId, LinkState>,
    checked: Option<(u64, f64)>,
    /// P3G.2: each use's icon state and what is newer ("V2 is available", see
    /// [`crate::reference_manager`]).
    pub icons: HashMap<cadrs_core::link_update::RefSite, (LinkIcon, String)>,
    /// Tabs with an unpinned reference that has an update (their tab badge, ER2.3).
    pub stale_tabs: HashMap<ElementId, LinkIcon>,
    /// The document references a version or another document (the toolbar's Update all).
    pub has_refs: bool,
    /// Bumped by [`Self::invalidate`]: the reference states are recomputed at once.
    pub epoch: u64,
}

impl LinkStatus {
    /// Checks the linked documents again on the next frame (after the library changed).
    pub fn invalidate(&mut self) {
        self.checked = None;
        self.epoch += 1;
    }

    pub fn state(&self, document: DocumentId) -> LinkState {
        self.states.get(&document).copied().unwrap_or(LinkState::Ok)
    }
}

/// Every document the open document links to (its copies' sources).
pub fn linked_documents(doc: &Document) -> Vec<DocumentId> {
    let mut out: Vec<DocumentId> = Vec::new();
    for l in &doc.linked {
        let d = l.source.document_or(doc.id);
        if d != doc.id && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

fn refresh_link_status(world: &mut World) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    // The linked copies' ids (cheap; they change when links are added or removed).
    let mut bytes = doc.doc.id.0.as_bytes().to_vec();
    for l in &doc.doc.linked {
        bytes.extend_from_slice(l.id().0.as_bytes());
    }
    let key = cadrs_kernel_hash(&bytes);
    let now = world.resource::<Time>().elapsed_secs_f64();
    if world.resource::<LinkStatus>().checked.is_some_and(|(k, t)| k == key && now - t < 3.0) {
        return;
    }
    let documents = linked_documents(&doc.doc);
    // The resolver reads a document again only when its file changed.
    let states: HashMap<DocumentId, LinkState> = {
        let mut r = resolver(world);
        documents.into_iter().map(|d| (d, r.0.state(d))).collect()
    };
    let mut status = world.resource_mut::<LinkStatus>();
    if states != status.states {
        status.states = states;
    }
    status.bypass_change_detection().checked = Some((key, now));
}

fn cadrs_kernel_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The state a linked reference's icon shows (ER X2: plain, blue, blue with an arrow, thumbtack;
/// DV1.7: red while the source can't be reached).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LinkIcon {
    /// Up to date.
    #[default]
    Plain,
    /// Its source has a newer version: the blue badge (ER2.3).
    Update,
    /// A reference further down the chain has a newer version (ER4.2).
    Transitive,
    /// Pinned (ER5.1).
    Pinned,
    /// Pinned, with a newer version: the icon changes, but not to blue (ER5.2).
    PinnedStale,
    /// The source can't be reached (DV1.7).
    Unreachable,
}

impl LinkIcon {
    /// Shows that an update is available (the tab badge follows these).
    pub fn is_update(self) -> bool {
        matches!(self, LinkIcon::Update | LinkIcon::Transitive)
    }
}

/// What a linked icon belongs to (its click opens the Reference manager for it, ER5.4).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    /// An instance of the active assembly.
    Instance(cadrs_core::assembly::InstanceId),
    /// A drawing's source (a sheet's reference in the Sheets pane).
    Drawing(ElementId, ElementId),
    /// P3G.4: a Derived feature of the active Part Studio.
    Derived(cadrs_core::FeatureId),
}

/// The blue of the update badge.
pub const UPDATE_BLUE: Color = Color::srgb(0.118, 0.482, 0.886);
/// The transitive badge's arrow (a darker blue, so it reads inside the ring).
pub const TRANSITIVE_ARROW: Color = Color::srgb(0.043, 0.310, 0.620);

/// A linked reference's icon (14 px, see [`LinkIcon`]), with its tooltip; a click opens the
/// Reference manager for it.
pub fn spawn_link_icon(p: &mut ChildSpawnerCommands, name: String, icon: LinkIcon, tip: &str, target: LinkTarget) {
    let node = Node { flex_shrink: 0.0, margin: UiRect::left(Val::Auto), width: Val::Px(14.0), height: Val::Px(14.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
    let dark = Color::srgb_u8(0x26, 0x26, 0x26);
    let mut e = match icon {
        LinkIcon::Plain => p.spawn((Name::new(name), cadrs_ui::icon::icon_in("link", 14.0, dark, node))),
        LinkIcon::Unreachable => p.spawn((Name::new(name), cadrs_ui::icon::icon_in("link", 14.0, Color::srgb_u8(0xc6, 0x28, 0x28), node), cadrs_ui::icon::SolidTint)),
        // P3G.3: a bold dark thumbtack (`lesson-pinning-references.png`), drawn (icon-rs has none).
        LinkIcon::Pinned => {
            let mut e = p.spawn((Name::new(name), node));
            e.with_child(cadrs_ui::glyphs::thumbtack(14.0, Color::srgb_u8(0x14, 0x14, 0x14)));
            e
        }
        LinkIcon::Update | LinkIcon::Transitive | LinkIcon::PinnedStale => {
            // A round badge: blue with the white version icon (update), a solid blue ring with a
            // dark blue down arrow (further down), a grey ring round the pin (pinned, newer
            // version).
            let (fill, ring, width) = match icon {
                LinkIcon::Update => (UPDATE_BLUE, UPDATE_BLUE, 1.5),
                LinkIcon::Transitive => (Color::WHITE, UPDATE_BLUE, 2.0),
                _ => (Color::WHITE, Color::srgb_u8(0x9a, 0x9a, 0x9a), 1.5),
            };
            let mut e = p.spawn((
                Name::new(name),
                Node { border: UiRect::all(Val::Px(width)), border_radius: BorderRadius::MAX, ..node },
                BackgroundColor(fill),
                BorderColor::all(ring),
            ));
            match icon {
                LinkIcon::Update => e.with_child((cadrs_ui::icon::icon("link", 10.0, Color::WHITE), cadrs_ui::icon::SolidTint, Pickable::IGNORE)),
                LinkIcon::Transitive => e.with_child(cadrs_ui::glyphs::down_arrow(10.0, TRANSITIVE_ARROW)),
                _ => e.with_child(cadrs_ui::glyphs::thumbtack(9.0, Color::srgb_u8(0x55, 0x55, 0x55))),
            };
            e
        }
    };
    e.insert((Tooltip::new(tip.to_string()), target, Pickable::default()));
    e.observe(|mut click: On<Pointer<Click>>, q: Query<&LinkTarget>, mut commands: Commands| {
        if click.button != PointerButton::Primary {
            return;
        }
        click.propagate(false);
        if let Ok(t) = q.get(click.entity).copied() {
            commands.queue(move |world: &mut World| crate::reference_manager::open_for_icon(world, t));
        }
    });
}

/// The icon and tooltip of a linked instance `inst` of assembly `asm` (ER1.9, ER X2; DV1.7).
pub fn instance_badge(doc: &Document, status: &LinkStatus, r: &SourceRef, asm: ElementId, inst: &cadrs_core::assembly::Instance) -> (String, LinkIcon) {
    let site = cadrs_core::link_update::RefSite::Instance { element: asm, instance: inst.id };
    link_badge(doc, status, r, inst.source.element(), site)
}

/// The icon and tooltip of the reference `r` used at `site` through the copy `element`.
pub fn link_badge(doc: &Document, status: &LinkStatus, r: &SourceRef, element: ElementId, site: cadrs_core::link_update::RefSite) -> (String, LinkIcon) {
    let copy = doc.linked_element(element);
    let what = copy.map(|l| l.describe()).unwrap_or_else(|| "a linked element".into());
    let this = r.document_or(doc.id) == doc.id;
    let state = if this { LinkState::Ok } else { status.state(r.document_or(doc.id)) };
    let head = if this { format!("Linked document: {what} (this document)") } else { format!("Linked document: {what}") };
    if let Some(m) = state.message() {
        return (format!("{head}\n{m}\nThe instance keeps the geometry of its version."), LinkIcon::Unreachable);
    }
    let icon = status.icons.get(&site).map(|(i, _)| *i).unwrap_or(if r.pinned { LinkIcon::Pinned } else { LinkIcon::Plain });
    let extra = status.icons.get(&site).map(|(_, t)| t.clone()).unwrap_or_default();
    let tail = match icon {
        LinkIcon::Update => format!("\n{extra}. Click to update."),
        LinkIcon::Transitive => format!("\n{extra}. Click to update."),
        LinkIcon::Pinned => "\nPinned: Update all skips it.".to_string(),
        LinkIcon::PinnedStale => format!("\nPinned: Update all skips it. {extra}."),
        LinkIcon::Plain if this => "\nIt does not change when the tab is edited.".to_string(),
        _ => String::new(),
    };
    (format!("{head}{tail}"), icon)
}

// ---------------------------------------------------------------------------------------------
// A source opened at a version

/// A document opened in a browser: the document at a version and the copies of its tabs.
#[derive(Clone)]
pub struct Opened {
    pub document: DocumentId,
    pub name: String,
    /// The open document itself (the Current document tab at a version).
    pub this: bool,
    /// The version read; `None`: the document has no versions yet (only its name is shown).
    pub version: Option<(VersionId, String)>,
    /// The document at that version.
    pub doc: Arc<Document>,
    /// Each tab's copy and everything it needs, by the tab's id in the source.
    pub roots: Arc<Vec<(ElementId, Arc<LinkSnapshot>)>>,
    /// Every copy (for building and showing them before they are inserted).
    pub links: Arc<Vec<LinkedElement>>,
}

impl PartialEq for Opened {
    fn eq(&self, o: &Self) -> bool {
        self.document == o.document && self.this == o.this && self.version.as_ref().map(|v| v.0) == o.version.as_ref().map(|v| v.0) && self.name == o.name
    }
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Opened({} {:?})", self.name, self.version.as_ref().map(|v| &v.1))
    }
}

impl Opened {
    /// The copies of tab `source` (its id in the source).
    pub fn snapshot(&self, source: ElementId) -> Option<&Arc<LinkSnapshot>> {
        self.roots.iter().find(|(e, _)| *e == source).map(|(_, s)| s)
    }

    /// The reference to tab `source` at this version.
    pub fn reference(&self, source: ElementId) -> Option<SourceRef> {
        let (v, _) = self.version.as_ref()?;
        Some(SourceRef::version((!self.this).then_some(self.document), source, *v))
    }

    /// "V1", or "No versions".
    pub fn version_label(&self) -> String {
        self.version.as_ref().map(|v| v.1.clone()).unwrap_or_else(|| "No versions".into())
    }
}

/// The copies of every tab (not drawings) of `doc` at version `v`.
fn snapshots(doc: &Document, document: Option<DocumentId>, v: VersionId, name: &str) -> (Vec<(ElementId, Arc<LinkSnapshot>)>, Vec<LinkedElement>) {
    let mut roots = Vec::new();
    let mut links: Vec<LinkedElement> = Vec::new();
    for el in doc.elements.iter().filter(|e| !matches!(e.kind, ElementKind::Drawing(_))) {
        let Ok(s) = external::snapshot(doc, SourceRef::version(document, el.id, v), name) else { continue };
        for l in &s.links {
            if links.iter().all(|x| x.id() != l.id()) {
                links.push(l.clone());
            }
        }
        roots.push((el.id, Arc::new(s)));
    }
    (roots, links)
}

/// `doc` with the copies `links` it doesn't hold yet (to show linked items before they are
/// inserted: ghosts, thumbnails, drawing views being placed).
pub fn with_links(doc: &Document, links: &[LinkedElement]) -> Document {
    let mut d = doc.clone();
    for l in links {
        if d.linked_element(l.id()).is_none() {
            d.linked.push(l.clone());
        }
    }
    d
}

/// Opens `document` at `version` (its newest when `None`). The open document itself needs a
/// version (its workspace is the Current document tab without one).
pub fn open(world: &mut World, document: DocumentId, version: Option<VersionId>) -> Result<Opened, String> {
    let this_id = world.resource::<ActiveDocument>().doc.id;
    if document == this_id {
        let log = world.resource::<DocLog>().log.as_ref().ok_or("The document has no history")?;
        let v = version.or_else(|| log.versions().last().map(|v| v.id())).ok_or("The document has no versions")?;
        let name = log.version(v).map(|x| x.name().to_string()).unwrap_or_default();
        let doc = log.document_at_version(v).ok_or("The version is missing")?;
        let (roots, links) = snapshots(&doc, None, v, &name);
        let title = world.resource::<ActiveDocument>().doc.name.clone();
        return Ok(Opened { document, name: title, this: true, version: Some((v, name)), doc: Arc::new(doc), roots: Arc::new(roots), links: Arc::new(links) });
    }
    let mut r = resolver(world);
    let state = r.0.state(document);
    if let Some(m) = state.message() {
        return Err(m.to_string());
    }
    let versions = r.0.versions(document);
    let title = r.0.store().load(document).map(|f| f.document.name).map_err(|e| e.to_string())?;
    let Some(v) = version.or_else(|| versions.last().map(|v| v.id())) else {
        let doc = r.0.store().load(document).map_err(|e| e.to_string())?.document;
        return Ok(Opened { document, name: title, this: false, version: None, doc: Arc::new(doc), roots: Arc::default(), links: Arc::default() });
    };
    let name = versions.iter().find(|x| x.id() == v).map(|x| x.name().to_string()).unwrap_or_default();
    let doc = r.0.document_at(document, v).map_err(|e| e.to_string())?;
    let (roots, links) = snapshots(&doc, Some(document), v, &name);
    Ok(Opened { document, name: title, this: false, version: Some((v, name)), doc, roots: Arc::new(roots), links: Arc::new(links) })
}

/// The versions of `document`, oldest first: the open document's from its log, another's from
/// the store.
pub fn versions_of(world: &mut World, document: DocumentId) -> Vec<(VersionId, String, String)> {
    versions_with_auto(world, document).into_iter().map(|(id, n, sub, _)| (id, n, sub)).collect()
}

/// [`versions_of`], with whether each is an auto version (P3G.2, ER4.7).
pub fn versions_with_auto(world: &mut World, document: DocumentId) -> Vec<(VersionId, String, String, bool)> {
    let this_id = world.resource::<ActiveDocument>().doc.id;
    let clock = *world.resource::<AppClock>();
    let list: Vec<cadrs_core::history_log::Version> = if document == this_id {
        world.resource::<DocLog>().log.as_ref().map(|l| l.versions().to_vec()).unwrap_or_default()
    } else {
        resolver(world).0.versions(document)
    };
    let profile = world.resource::<UserProfile>().clone();
    list.iter()
        .map(|v| {
            let sub = format!("{} · {}", profile.display(v.user()), clock.format(v.time()));
            (v.id(), v.name().to_string(), if v.auto() { format!("Auto version · {sub}") } else { sub }, v.auto())
        })
        .collect()
}

/// What a click on node `index` of a [`version_graph`] picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphPick {
    Workspace,
    Version(VersionId),
}

/// The Version graph (ER1.7, ER3.2) of `document`: Main (selectable only for the open document
/// itself: another document is linked at a version), the versions newest first, Start.
pub fn version_graph(world: &mut World, name: &str, document: DocumentId, selected: Option<VersionId>) -> (VersionGraph, Vec<GraphPick>) {
    let this = document == world.resource::<ActiveDocument>().doc.id;
    let versions = versions_with_auto(world, document);
    let mut g = VersionGraph::new(name.to_string());
    let mut picks = Vec::new();
    let mut main = VersionNode::new("Main", VersionNodeKind::Workspace).selectable(this).selected(this && selected.is_none());
    if !this {
        main = main.reason("Another document is referenced at a version: pick one below");
    }
    g = g.node(main);
    picks.push(GraphPick::Workspace);
    for (id, n, sub, auto) in versions.iter().rev() {
        let kind = if *auto { VersionNodeKind::AutoVersion } else { VersionNodeKind::Version };
        g = g.node(VersionNode::new(n.clone(), kind).subtitle(sub.clone()).selected(selected == Some(*id)));
        picks.push(GraphPick::Version(*id));
    }
    g = g.node(VersionNode::new("Start", VersionNodeKind::Start).selectable(false));
    picks.push(GraphPick::Workspace);
    (g, picks)
}

// ---------------------------------------------------------------------------------------------
// The Other documents browser

/// Which dialog a browser belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The assembly Insert dialog.
    Insert,
    /// The drawing Insert view browser.
    InsertView,
    /// P3G.3: the Move to document dialog's Other documents (the target document).
    Move,
    /// P3G.4: the Derived dialog's Other documents (the source Part Studio).
    Derived,
}

impl Owner {
    /// The browser's name prefix.
    pub fn prefix(self) -> &'static str {
        match self {
            Owner::Insert => "insert-other",
            Owner::InsertView => "insert-view-other",
            Owner::Move => "move-other",
            Owner::Derived => "derived-other",
        }
    }
}

/// A browser location (ER1.2; cadrs's local stand-ins, ER X8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Location {
    /// The list of locations.
    #[default]
    Top,
    Mine,
    Recent,
    Created,
    Folder(FolderId),
    /// P3E.1 (ER1.2): the documents with a label.
    Label(cadrs_core::LabelId),
}

/// An Other documents browser's state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Browse {
    pub location: Location,
    pub search: String,
    pub opened: Option<Opened>,
    pub graph: bool,
}

/// A document in the browser's list.
#[derive(Debug, Clone, PartialEq)]
pub struct DocListRow {
    pub id: DocumentId,
    pub name: String,
    pub subtitle: String,
    pub versions: bool,
    /// How many versions it has (P3G.4: the Move dialog's rows say "1 version", as its card).
    pub count: usize,
}

/// What a browser shows before a document is opened: the library's folders and, for a location
/// or a search, its documents. `loading` until the library's first read lands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BrowserList {
    pub folders: Vec<(FolderId, String)>,
    /// P3E.1 (ER1.2): the library's labels, by name.
    pub labels: Vec<(cadrs_core::LabelId, String)>,
    pub rows: Vec<DocListRow>,
    pub loading: bool,
}

/// What the browsers list, read on a background thread so opening one never waits on the disk:
/// the library (each document's `entry.ron`, or the whole document where that is missing or
/// stale) and each document's versions (its history log). A browser shows the last read at
/// once, and asks for a new one when that is more than ten seconds old; the browsers' lists
/// rebuild when one that changed lands ([`LibrarySnapshot::generation`]). Thumbnails are decoded there too.
#[derive(Resource, Default)]
pub struct LibrarySnapshot {
    data: Option<Arc<LibraryData>>,
    read_at: Option<std::time::Instant>,
    reading: Option<Pending<LibraryData>>,
    decoding: Option<Pending<Vec<Decoded>>>,
    /// Thumbnails asked for while a decode was running.
    wanted: Vec<DocumentId>,
    /// Bumped when a read or a batch of thumbnails lands.
    pub generation: u64,
}

#[derive(PartialEq)]
struct LibraryData {
    lib: cadrs_core::Library,
    /// Each document's newest version's name and how many it has.
    versions: HashMap<DocumentId, (Option<String>, usize)>,
}

/// A document's thumbnail, decoded (`None`: it has none).
type Decoded = (DocumentId, Option<image::RgbaImage>);

/// A result a background thread fills in.
type Pending<T> = Arc<std::sync::Mutex<Option<T>>>;

fn spawn_read<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    let slot: Pending<T> = Arc::default();
    let out = slot.clone();
    std::thread::spawn(move || {
        let v = f();
        if let Ok(mut s) = out.lock() {
            *s = Some(v);
        }
    });
    slot
}

fn take<T>(p: &Pending<T>) -> Option<T> {
    p.lock().ok()?.take()
}

impl LibrarySnapshot {
    /// Asks for a new read at the next [`browser_list`] even if the last is fresh: a browser
    /// that just opened lists what this session wrote since (a new document, folder or
    /// version; P3E.2 merge).
    pub fn invalidate(&mut self) {
        self.read_at = None;
    }

    /// Starts a new read of `store` unless one is running or the last is fresh.
    fn refresh(&mut self, store: &cadrs_core::Store) {
        if self.reading.is_some() || self.read_at.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(10)) {
            return;
        }
        let store = store.clone();
        self.reading = Some(spawn_read(move || {
            let (lib, _) = store.list();
            let versions = lib
                .entries
                .iter()
                .filter(|e| e.meta.trashed.is_none())
                .filter_map(|e| {
                    let log = cadrs_core::history_log::HistoryLog::load(&store, e.id).ok()??;
                    let v = log.versions();
                    Some((e.id, (v.last().map(|v| v.name().to_string()), v.len())))
                })
                .collect();
            LibraryData { lib, versions }
        }));
    }

    /// Starts decoding `ids`' thumbnails, or keeps them for when the running decode ends.
    fn decode(&mut self, store: &cadrs_core::Store, ids: Vec<DocumentId>) {
        for id in ids {
            if !self.wanted.contains(&id) {
                self.wanted.push(id);
            }
        }
        if self.decoding.is_some() || self.wanted.is_empty() {
            return;
        }
        let store = store.clone();
        let ids = std::mem::take(&mut self.wanted);
        self.decoding = Some(spawn_read(move || ids.into_iter().map(|id| (id, store.read_thumbnail(id))).collect()));
    }
}

/// Takes in the library reads and thumbnails that finished.
fn land_library_reads(world: &mut World) {
    let (read, decoded) = {
        let mut snap = world.resource_mut::<LibrarySnapshot>();
        let read = snap.reading.as_ref().and_then(take);
        if read.is_some() {
            snap.reading = None;
        }
        let decoded = snap.decoding.as_ref().and_then(take);
        if decoded.is_some() {
            snap.decoding = None;
        }
        (read, decoded)
    };
    if read.is_none() && decoded.is_none() {
        return;
    }
    let decoded_any = decoded.is_some();
    if let Some(d) = decoded {
        for (id, img) in d {
            let h = img.map(|img| thumbnail_image(&mut world.resource_mut::<Assets<Image>>(), img));
            world.resource_mut::<DocThumbs>().0.insert(id, h);
        }
        let store = world.resource::<DocumentStore>().0.clone();
        world.resource_mut::<LibrarySnapshot>().decode(&store, Vec::new());
    }
    let mut snap = world.resource_mut::<LibrarySnapshot>();
    let mut changed = decoded_any;
    if let Some(d) = read {
        snap.read_at = Some(std::time::Instant::now());
        // The same as before: nothing to rebuild (a browser keeps its focus while typing).
        if snap.data.as_deref() != Some(&d) {
            snap.data = Some(Arc::new(d));
            changed = true;
        }
    }
    if changed {
        snap.generation += 1;
    }
}

/// The browser's list for `b`, from the last read of the library (asking for a new one).
pub fn browser_list(world: &mut World, b: &Browse) -> BrowserList {
    let store = world.resource::<DocumentStore>().0.clone();
    let user = world.resource::<UserProfile>().id.clone();
    let this = world.resource::<ActiveDocument>().doc.id;
    let mut snap = world.resource_mut::<LibrarySnapshot>();
    snap.refresh(&store);
    let Some(data) = snap.data.clone() else {
        return BrowserList { loading: true, ..default() };
    };
    let lib = &data.lib;
    let folders = lib.folders.iter().map(|f| (f.id, f.name.clone())).collect();
    let labels = lib.labels_by_name().into_iter().map(|l| (l.id, l.name)).collect();
    let q = b.search.trim().to_lowercase();
    let entries: Vec<cadrs_core::DocumentEntry> = if !q.is_empty() {
        // A pasted id finds its document (ER1.3); otherwise names containing the text.
        let pasted = uuid::Uuid::parse_str(q.trim_start_matches("cadrs://").rsplit('/').next().unwrap_or(&q)).ok().map(DocumentId);
        lib.entries.iter().filter(|e| e.meta.trashed.is_none() && (Some(e.id) == pasted || e.name.to_lowercase().contains(&q))).cloned().collect()
    } else {
        use cadrs_core::{Filter, SortDir, SortKey};
        match b.location {
            Location::Top => Vec::new(),
            Location::Mine => lib.view(Filter::OwnedByMe, &user, "", SortKey::Modified, SortDir::Descending),
            Location::Recent => lib.view(Filter::RecentlyOpened, &user, "", SortKey::Modified, SortDir::Descending),
            Location::Created => lib.view(Filter::CreatedByMe, &user, "", SortKey::Modified, SortDir::Descending),
            Location::Folder(f) => lib.in_folder(f),
            Location::Label(l) => lib.with_label(l),
        }
    };
    let rows = entries
        .into_iter()
        .filter(|e| e.id != this)
        .map(|e| {
            let (latest, count) = data.versions.get(&e.id).cloned().unwrap_or((None, 0));
            DocListRow { id: e.id, name: e.name.clone(), subtitle: latest.clone().unwrap_or_else(|| "No versions".into()), versions: latest.is_some(), count }
        })
        .collect();
    BrowserList { folders, labels, rows, loading: false }
}

/// The thumbnails of a browser's rows that are decoded; the others are decoded on a background
/// thread and the browser rebuilds when they land.
pub fn browser_thumbs(world: &mut World, rows: &[DocListRow]) -> HashMap<DocumentId, Handle<Image>> {
    let thumbs = world.resource::<DocThumbs>();
    let missing: Vec<DocumentId> = rows.iter().map(|r| r.id).filter(|id| !thumbs.0.contains_key(id)).collect();
    let out = rows.iter().filter_map(|r| Some((r.id, thumbs.0.get(&r.id)?.clone()?))).collect();
    if !missing.is_empty() {
        let store = world.resource::<DocumentStore>().0.clone();
        world.resource_mut::<LibrarySnapshot>().decode(&store, missing);
    }
    out
}

/// A decoded thumbnail as an image.
fn thumbnail_image(images: &mut Assets<Image>, img: image::RgbaImage) -> Handle<Image> {
    let (w, h) = img.dimensions();
    images.add(Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        img.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

/// Document thumbnails for the browser rows.
#[derive(Resource, Default)]
pub struct DocThumbs(HashMap<DocumentId, Option<Handle<Image>>>);

impl DocThumbs {
    /// Reads `id`'s thumbnail again next time (P3G.3: a document made by Move to document).
    pub fn forget(&mut self, id: DocumentId) {
        self.0.remove(&id);
    }
}

/// The thumbnails of `rows` (read from the store once).
pub fn thumbs_for(world: &mut World, rows: &[DocListRow]) -> HashMap<DocumentId, Handle<Image>> {
    let store = world.resource::<DocumentStore>().0.clone();
    let mut out = HashMap::new();
    for r in rows {
        let cached = world.resource::<DocThumbs>().0.get(&r.id).cloned();
        let h = match cached {
            Some(h) => h,
            None => {
                let h = store.read_thumbnail(r.id).map(|img| thumbnail_image(&mut world.resource_mut::<Assets<Image>>(), img));
                world.resource_mut::<DocThumbs>().0.insert(r.id, h.clone());
                h
            }
        };
        if let Some(h) = h {
            out.insert(r.id, h);
        }
    }
    out
}

/// A location row of a browser.
#[derive(Component, Debug, Clone, Copy)]
pub struct BrowserLoc {
    pub owner: Owner,
    pub location: Location,
}

/// A document row of a browser.
#[derive(Component, Debug, Clone, Copy)]
pub struct BrowserDoc {
    pub owner: Owner,
    pub id: DocumentId,
}

/// A name slug ("Block source" → "block-source").
pub fn slug(s: &str) -> String {
    crate::assembly::insert::slug(s)
}

/// The browser before a document is picked: the search field, then the locations or a
/// location's (or the search's) documents.
pub fn spawn_browser(p: &mut ChildSpawnerCommands, t: &Theme, owner: Owner, b: &Browse, list: &BrowserList, thumbs: &HashMap<DocumentId, Handle<Image>>) {
    let prefix = owner.prefix();
    p.spawn(BrowserSearch::new(format!("{prefix}-search")).value(b.search.clone()).build(t));
    spawn_browser_list(p, t, owner, b, list, thumbs);
}

/// The part of the browser under its search field: the locations, or the documents.
pub fn spawn_browser_list(p: &mut ChildSpawnerCommands, t: &Theme, owner: Owner, b: &Browse, list: &BrowserList, thumbs: &HashMap<DocumentId, Handle<Image>>) {
    let prefix = owner.prefix();
    let body = Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_height: Val::Px(0.0), max_height: Val::Px(360.0), overflow: Overflow::scroll_y(), padding: UiRect::vertical(Val::Px(2.0)), ..default() };
    p.spawn((Name::new(format!("{prefix}-list")), body)).with_children(|l| {
        if b.search.trim().is_empty() && b.location == Location::Top {
            let mut locs: Vec<(String, &'static str, String, Location)> = vec![
                ("mine".into(), "user", "My documents".into(), Location::Mine),
                ("recent".into(), "clock", "Recently opened".into(), Location::Recent),
                ("created".into(), "user", "Created by me".into(), Location::Created),
            ];
            for (id, n) in &list.folders {
                locs.push((format!("folder-{}", slug(n)), "folder", n.clone(), Location::Folder(*id)));
            }
            for (id, n) in &list.labels {
                locs.push((format!("label-{}", slug(n)), "tag", n.clone(), Location::Label(*id)));
            }
            for (key, icon, label, loc) in locs {
                l.spawn((location_row(format!("{prefix}-loc-{key}"), icon, label, t), BrowserLoc { owner, location: loc }));
            }
            return;
        }
        if b.search.trim().is_empty() {
            let label = match b.location {
                Location::Mine => "My documents".to_string(),
                Location::Recent => "Recently opened".to_string(),
                Location::Created => "Created by me".to_string(),
                Location::Folder(f) => list.folders.iter().find(|(id, _)| *id == f).map(|(_, n)| n.clone()).unwrap_or_else(|| "Folder".into()),
                Location::Label(l) => list.labels.iter().find(|(id, _)| *id == l).map(|(_, n)| n.clone()).unwrap_or_else(|| "Label".into()),
                Location::Top => String::new(),
            };
            l.spawn((location_row(format!("{prefix}-loc-back"), "chevron-left", label, t), BrowserLoc { owner, location: Location::Top }));
        }
        if list.rows.is_empty() {
            l.spawn((
                t.text(if list.loading { "Loading…" } else { "No documents" }, t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::all(Val::Px(10.0)), ..default() },
            ));
        }
        for r in &list.rows {
            let tip = if r.versions {
                format!("{}\nNewest version: {}", r.name, r.subtitle)
            } else {
                format!("{}\nNo versions yet: a linked document needs a version (create one after opening it)", r.name)
            };
            // The Move dialog's target: how many versions (its card says so too, P3G.3 carried).
            let subtitle = match (owner, r.count) {
                (Owner::Move, 0) => "No versions".to_string(),
                (Owner::Move, 1) => "1 version".to_string(),
                (Owner::Move, n) => format!("{n} versions"),
                _ => r.subtitle.clone(),
            };
            l.spawn((
                DocumentRow::new(format!("{prefix}-doc-{}", slug(&r.name)), r.name.clone(), subtitle)
                    .thumbnail(thumbs.get(&r.id).cloned())
                    .muted(!r.versions)
                    .tooltip(tip)
                    .build(t),
                BrowserDoc { owner, id: r.id },
            ));
        }
    });
}

/// The message and button a document without versions shows (DV1.8, ER1.5).
pub fn spawn_no_version(p: &mut ChildSpawnerCommands, t: &Theme, owner: Owner) {
    let prefix = owner.prefix();
    p.spawn((
        Name::new(format!("{prefix}-no-version")),
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), padding: UiRect::all(Val::Px(10.0)), ..default() },
    ))
    .with_children(|c| {
        c.spawn((
            t.text("This document has no versions. A linked document is referenced at a version, so its later edits never change what you insert. Create a version to use it.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
            Node { max_width: Val::Px(230.0), ..default() },
        ))
        .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
        c.spawn(cadrs_ui::Button::new(format!("{prefix}-no-version-create")).label("Create version").icon("versions").primary().small().build(t));
    });
}

/// Mutable access to a browser's state (the dialog's resource).
pub fn with_browse<R>(world: &mut World, owner: Owner, f: impl FnOnce(&mut Browse) -> R) -> Option<R> {
    match owner {
        Owner::Insert => world.get_resource_mut::<crate::assembly::insert::InsertSession>().map(|mut s| f(&mut s.browse)),
        Owner::InsertView => world.get_resource_mut::<crate::drawing::view_tools::InsertViewState>().map(|mut s| f(&mut s.browse)),
        Owner::Move => world.get_resource_mut::<crate::move_document::MoveSession>().map(|mut s| f(&mut s.browse)),
        Owner::Derived => world.get_resource_mut::<crate::derived_ui::DerivedSession>().map(|mut s| f(&mut s.browse)),
    }
}

fn on_browser_location(a: On<Activate>, q: Query<&BrowserLoc>, mut commands: Commands) {
    let Ok(l) = q.get(a.entity).copied() else { return };
    commands.queue(move |world: &mut World| {
        with_browse(world, l.owner, |b| {
            b.location = l.location;
            b.search.clear();
        });
    });
}

fn on_browser_document(a: On<Activate>, q: Query<&BrowserDoc>, mut commands: Commands) {
    let Ok(d) = q.get(a.entity).copied() else { return };
    if d.owner == Owner::Move {
        commands.queue(move |world: &mut World| crate::move_document::pick_target(world, d.id));
        return;
    }
    commands.queue(move |world: &mut World| open_in(world, d.owner, d.id, None));
}

/// Opens `document` (at `version`, else its newest) in a browser; a document that can't be read
/// says why (DV1.7) and stays closed.
pub fn open_in(world: &mut World, owner: Owner, document: DocumentId, version: Option<VersionId>) {
    match open(world, document, version) {
        Ok(o) => {
            with_browse(world, owner, |b| {
                b.opened = Some(o);
                b.graph = false;
            });
        }
        Err(m) => toast(world, m),
    }
}

/// An error toast with `text` (red, shown until closed: a refused action, DV1.5).
pub fn error_toast(world: &mut World, text: impl Into<String>) {
    let theme = world.resource::<Theme>().clone();
    let text = text.into();
    let mut commands = world.commands();
    cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::error(text).name("error-toast"));
    world.flush();
}

/// P3G.4: removes the error toasts (a refusal's), when the dialog they belong to closes or the
/// tab changes.
pub fn close_error_toasts(world: &mut World) {
    let mut q = world.query::<(Entity, &Name)>();
    let old: Vec<Entity> = q.iter(world).filter(|(_, n)| n.as_str() == "error-toast").map(|(e, _)| e).collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
}

/// Closes the error toasts when the active tab changes.
pub fn close_error_toasts_on_tab_change(world: &mut World, mut last: Local<Option<ElementId>>) {
    let now = world.get_resource::<ActiveDocument>().and_then(|d| d.active);
    if *last != now {
        if last.is_some() {
            close_error_toasts(world);
        }
        *last = now;
    }
}

/// A toast with `text`.
pub fn toast(world: &mut World, text: impl Into<String>) {
    let theme = world.resource::<Theme>().clone();
    let text = text.into();
    let mut commands = world.commands();
    cadrs_ui::show_toast(&mut commands, &theme, text);
    world.flush();
}

// ---------------------------------------------------------------------------------------------
// Create version (ER2.2, ER6.2) and the top bar

/// Which document the Create version dialog makes a version of.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum VersionTarget {
    /// The open document.
    Current,
    /// Another document, from a browser (the new version is opened there).
    Other(DocumentId, Owner),
}

/// Opens "Create version from Main" for `target`.
pub fn open_create_version_dialog(world: &mut World, target: VersionTarget) {
    if crate::history_panel::editing(world) && target == VersionTarget::Current && !world.contains_resource::<crate::assembly::insert::InsertSession>() {
        return;
    }
    let next = match target {
        VersionTarget::Current => world.resource::<DocLog>().log.as_ref().map(|l| l.next_version_name()).unwrap_or_else(|| "V1".into()),
        VersionTarget::Other(d, _) => format!("V{}", resolver(world).0.versions(d).len() + 1),
    };
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("create-version-dialog")
            .width(560.0)
            .title("Create version from Main")
            .title_font(theme.font_lg, FontWeight::NORMAL)
            .body(move |b| {
                let t = &tb;
                b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                    r.spawn(t.text("Name:", t.font_base, FontWeight::BOLD, t.foreground));
                    r.spawn(TextInput::new("create-version-name").value(next).select_all_on_focus().autofocus().width(Val::Px(300.0)).height(30.0).build(t));
                });
                b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(5.0), margin: UiRect::top(Val::Px(10.0)), ..default() }).with_children(|r| {
                    r.spawn(t.text("Description:", t.font_base, FontWeight::BOLD, t.foreground));
                    r.spawn(t.text("(Maximum of 10000 characters)", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                });
                b.spawn(TextInput::new("create-version-description").placeholder("Description").max_characters(10_000).width(Val::Percent(100.0)).height(54.0).build(t));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(cadrs_ui::Button::new("create-version-create").label("Create").primary().build(t));
                f.spawn(cadrs_ui::Button::new("create-version-edit-properties").label("Create version and edit properties").build(t));
                f.spawn(cadrs_ui::Button::new("create-version-cancel").label("Cancel").build(t));
            })
            .build(&theme),
        target,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn field(world: &mut World, name: &str) -> String {
    let mut q = world.query::<(&Name, &bevy::text::EditableText)>();
    q.iter(world).find(|(n, _)| n.as_str() == name).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn on_version_dialog_button(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    let edit = match n.as_str() {
        "create-version-create" => false,
        "create-version-edit-properties" => true,
        "create-version-cancel" => {
            commands.queue(close_version_dialog);
            return;
        }
        _ => return,
    };
    commands.queue(move |world: &mut World| commit_version_dialog(world, edit));
}

fn close_version_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<VersionTarget>>();
    let dialogs: Vec<Entity> = q.iter(world).collect();
    for d in dialogs {
        world.trigger(DialogClose { entity: d });
    }
}

/// Create: the version is made (of the open document: at its current state; of another: as it
/// is on disk), the dialog closes, and a browser that asked for it opens the new version.
/// "Create version and edit properties" opens the History panel on it as well (its name,
/// description, who and when).
fn commit_version_dialog(world: &mut World, edit: bool) {
    let mut q = world.query::<&VersionTarget>();
    let Some(target) = q.iter(world).next().copied() else { return };
    let name = field(world, "create-version-name-field");
    let description = field(world, "create-version-description-field");
    close_version_dialog(world);
    match target {
        VersionTarget::Current => {
            create_current_version(world, &name, &description);
            let n = world.resource::<DocLog>().log.as_ref().map(|l| l.versions().len()).unwrap_or(0);
            let mut panel = world.resource_mut::<crate::history_panel::HistoryPanel>();
            panel.open = edit;
            if edit && n > 0 {
                panel.selected_version = Some(n - 1);
            }
            // DV1.8: the Insert dialog's version picker selects the new version.
            let (doc_id, newest) = {
                let d = world.resource::<ActiveDocument>().doc.id;
                (d, world.resource::<DocLog>().log.as_ref().and_then(|l| l.versions().last().map(|v| v.id())))
            };
            if let Some(v) = newest
                && world.get_resource::<crate::assembly::insert::InsertSession>().is_some_and(|s| s.current_graph || s.current.is_some())
                && let Ok(o) = open(world, doc_id, Some(v))
                && let Some(mut s) = world.get_resource_mut::<crate::assembly::insert::InsertSession>()
            {
                s.current = Some(o);
                s.current_graph = false;
            }
            if let Some(v) = newest
                && world.get_resource::<crate::drawing::view_tools::InsertViewState>().is_some_and(|s| s.current_graph || s.current.is_some())
                && let Ok(o) = open(world, doc_id, Some(v))
            {
                let mut s = world.resource_mut::<crate::drawing::view_tools::InsertViewState>();
                s.current = Some(o);
                s.current_graph = false;
            }
        }
        VersionTarget::Other(d, owner) => {
            let now = world.resource::<AppClock>().now();
            let user = world.resource::<UserProfile>().id.clone();
            match resolver(world).0.create_version(d, &name, &description, now, &user) {
                Ok(v) => open_in(world, owner, d, Some(v)),
                Err(e) => toast(world, e.to_string()),
            }
        }
    }
}

/// A version of the open document as it is now (DV1.8: also while the Insert dialog is open; its
/// inserts so far are then logged first, so the version holds them).
fn create_current_version(world: &mut World, name: &str, description: &str) {
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let store = world.resource::<DocumentStore>().0.clone();
    let doc = world.resource::<ActiveDocument>();
    let stored = doc.meta.is_some();
    let d = doc.doc.clone();
    let label = doc.history.undo_label().unwrap_or_default().to_string();
    let mut l = world.resource_mut::<DocLog>();
    let Some(log) = l.log.as_mut() else { return };
    if log.head() != &d {
        log.record(&d, cadrs_core::history_log::Origin::Command(label), now, &user);
    }
    log.create_version(name, description, now, &user);
    if stored {
        let _ = log.save(&store);
    }
    l.generation += 1;
}

fn on_top_bar(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).is_ok_and(|n| n.as_str() == "document-create-version") {
        commands.queue(|world: &mut World| open_create_version_dialog(world, VersionTarget::Current));
    }
}

/// The top bar's versions counter follows the document's versions (ER2.2).
fn sync_versions_counter(log: Res<DocLog>, q: Query<(&Name, &Children)>, mut q_text: Query<&mut Text>) {
    if !log.is_changed() {
        return;
    }
    let n = log.log.as_ref().map(|l| l.versions().len()).unwrap_or(0).to_string();
    for (name, children) in &q {
        if name.as_str() != "document-versions" {
            continue;
        }
        for c in children.iter() {
            if let Ok(mut t) = q_text.get_mut(c)
                && t.0 != n
            {
                t.0 = n.clone();
            }
        }
    }
}
