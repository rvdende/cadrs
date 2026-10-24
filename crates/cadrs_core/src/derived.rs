//! P3G.4: the **Derived** Part Studio feature (`derived-and-linking.md` DV3, DV1.5; the gap
//! list's P3G.4 row).
//!
//! - **What it holds.** A [`DerivedFeature`] names its source Part Studio with the P3G.1
//!   [`SourceRef`]: this document's workspace (live, DV3.4), a version of this document, or a
//!   version of another document (always a version, DV1.1). A version reference keeps the
//!   frozen copy of the source in [`Document::linked`] (like a linked instance, so update, where
//!   used, pinning and the Reference manager treat it the same way: [`crate::link_update`]
//!   `RefSite::Derived`) and names it in [`DerivedFeature::copy`]. Either way the feature also
//!   carries the source's features ([`DerivedFeature::studio`]) and part settings
//!   ([`DerivedFeature::props`]) as they are at the reference, so any rebuild of the host's
//!   feature list (the app's, an assembly's, a drawing's, an export's) builds the derived parts
//!   without looking anything up, and the rebuild cache's key (the feature's parameters)
//!   changes exactly when the source does.
//! - **Workspace references follow the source live** ([`refresh`]): after every command, undo
//!   and redo ([`crate::command::History`]) the embedded features of each same-document
//!   workspace reference are brought up to date, chains in order.
//! - **Options** (DV3.3): what to derive ([`DerivedSelection`]: the whole Part Studio, or parts,
//!   sketches, planes and mate connectors), **Locations** (mate connectors of the host, or its
//!   origin; one copy per location, the origin when empty), **Placement** (*Base origin*: the
//!   source origin goes onto each location; *Base mate connector*: a mate connector of the
//!   source goes onto it), **Include mate connectors** (the explicit connectors on the derived
//!   parts) and **Include properties** (off: only the name, material and appearance come over).
//!   Configurations are out of scope (user decision 2026-09-29); cadrs Part Studios have no
//!   composite parts.
//! - **Identity.** Derived geometry is namespaced like pattern instances: a derived part's id is
//!   `(the Derived feature, a hash of the source part and the copy)` ([`derived_part`]), its
//!   faces are `FaceOrigin::Instance { of: source op, face: source face, instance: copy }` under
//!   the Derived feature's id ([`derived_face`]), and derived sketches, planes and connectors get
//!   ids of their own ([`derived_entity`]). So a derived copy of a duplicated studio never
//!   collides with the host's own ids, and a fillet on a derived edge survives source edits
//!   that keep the face.
//! - **Rules** (DV3.7, DV1.5; [`check`]): a Part Studio can't derive itself, the same Part
//!   Studio can't be derived twice in one Part Studio, and circular chains are refused, across
//!   tabs, versions and documents.
//! - **The Onshape importer** (merged from main, which had its own Derived feature for
//!   `importDerived`): [`DerivedFeature::new`] makes a whole-Part-Studio reference at the
//!   workspace, of this document (followed live like any workspace reference) or of another one.
//!   Another document's workspace is outside DV1.1 (only versions of other documents), so it is
//!   filled once, when its [`DerivedFeature::studio`] is empty, from the loader set with
//!   [`set_document_loader`] (the app and the importer set it to their Store): a frozen copy, as
//!   Onshape pins a version, that the Reference manager doesn't list. [`part_id`], [`face_name`]
//!   and [`edge_name`] are the ids and names of the single copy (copy 0), which the importer's
//!   derived-part queries map source faces and edges through; [`source_parts`] lists the source
//!   parts for its part selection.
//! - **Main's pre-merge format** (schema 4 documents and histories written by the importer
//!   before the merge): `document` (`None`: this one), `element` (the Part Studio; `None` until
//!   picked), `version` (unused), `parts` (source part ids; empty: all of them),
//!   `include_mate_connectors` (default on) and `placement: AtOrigin`; its snapshot of the
//!   source was never saved. It reads (`DerivedRepr`) as a whole-Part-Studio or selected-parts
//!   reference at the workspace with Include mate connectors as saved; the source's features
//!   are filled in when the document is resolved ([`resolve_document`]: this document's tabs
//!   live, another document's once from the Store), and [`DerivedFeature::legacy_names`] keeps
//!   that format's part ids and face names ([`legacy_part_id`], [`legacy_face_name`]) so the
//!   features after it still find what they referenced. It is saved in this format.

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind, Feature, FeatureKind, PartProps};
use crate::external::{LinkError, LinkedElement, RefAt, Resolver, SourceRef, check_cycle};
use crate::history_log::HistoryLog;
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::mate::ConnectorRef;
use cadrs_kernel::naming::{FaceName, FaceOrigin, face_hash, stable_hash};

/// What a Derived feature brings in (DV3.2, DV3.3 step 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedSelection {
    /// The whole Part Studio: every part, sketch, plane (and, with Include mate connectors,
    /// every mate connector).
    #[serde(default)]
    pub all: bool,
    /// Parts, by their id in the source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartId>,
    /// Sketches (source feature ids).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketches: Vec<FeatureId>,
    /// Plane features.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<FeatureId>,
    /// Mate connector features.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<FeatureId>,
}

impl Default for DerivedSelection {
    fn default() -> Self {
        Self {
            all: true,
            parts: Vec::new(),
            sketches: Vec::new(),
            planes: Vec::new(),
            connectors: Vec::new(),
        }
    }
}

impl DerivedSelection {
    /// Nothing picked.
    pub fn is_empty(&self) -> bool {
        !self.all
            && self.parts.is_empty()
            && self.sketches.is_empty()
            && self.planes.is_empty()
            && self.connectors.is_empty()
    }
}

/// How the copies are placed (DV3.3 step 4).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum DerivedPlacement {
    /// The source's origin onto each location (the default). Main's pre-merge format called it
    /// `AtOrigin`.
    #[default]
    #[serde(alias = "AtOrigin")]
    BaseOrigin,
    /// A mate connector of the source (explicit, or implicit on a source entity) onto each
    /// location; `None` while the dialog waits for it.
    BaseConnector(Option<ConnectorRef>),
}

/// A PCB Studio board as a Derived feature's source (`docs/PCB_IN_PART_STUDIO.md`): which
/// board, its layout as at the reference (the schematic is left out: the 3D model, the skins and
/// the layout view need only the layout) and how it comes in ([`BoardOptions`]). The rebuild
/// makes the board's solid and a mesh part per component ([`crate::pcb::board3d`]).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedBoard {
    pub board: crate::pcb::BoardId,
    #[serde(default)]
    pub board_name: String,
    pub layout: cadrs_eda::board::Board,
    /// [`crate::pcb::board3d::layout_hash`] of `layout`: the rebuild's cache key sees it instead
    /// of the layout (kept with it by [`DerivedBoard::set_layout`], worked out again when a
    /// document is read).
    #[serde(skip)]
    pub hash: u64,
    #[serde(default)]
    pub options: crate::pcb::board3d::BoardOptions,
    /// Placed on this sketch's plane (a board made from the sketch, [`crate::pcb::outline_link`])
    /// rather than on the feature's locations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_sketch: Option<FeatureId>,
    /// Why the board's outline no longer follows its sketch (the link's problem, kept in step by
    /// the refresh): the feature's warning says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_problem: Option<String>,
    /// Why the board no longer follows its PCB Studio (the studio or the board deleted; kept
    /// in step by the refresh): the feature keeps its last layout, and its warning says why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_problem: Option<String>,
}

impl DerivedBoard {
    pub fn new(board: crate::pcb::BoardId, name: &str, layout: cadrs_eda::board::Board) -> Self {
        let mut d = DerivedBoard {
            board,
            board_name: name.to_string(),
            layout: Default::default(),
            hash: 0,
            options: Default::default(),
            on_sketch: None,
            outline_problem: None,
            source_problem: None,
        };
        d.set_layout(layout);
        d
    }

    /// Takes a new layout and its hash.
    pub fn set_layout(&mut self, layout: cadrs_eda::board::Board) {
        let hash = crate::pcb::board3d::layout_hash(&layout);
        self.set_layout_hashed(layout, hash);
    }

    /// Takes a new layout whose hash is known (the PCB Studio board's).
    pub fn set_layout_hashed(&mut self, layout: cadrs_eda::board::Board, hash: u64) {
        self.hash = hash;
        self.layout = layout;
    }
}

/// What the rebuild's cache key sees: the board, the layout's hash and the options that change
/// the parts (not the 3D / layout view, which only changes what is drawn).
impl std::fmt::Debug for DerivedBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let o = &self.options;
        f.debug_struct("DerivedBoard")
            .field("board", &self.board)
            .field("board_name", &self.board_name)
            .field("hash", &self.hash)
            .field("components", &o.components)
            .field("exclude", &o.exclude)
            .field("skins", &o.skins)
            .field("seat", &o.seat)
            .field("anchor", &o.anchor)
            .field("off_board", &o.off_board)
            .field("on_sketch", &self.on_sketch)
            .field("outline_problem", &self.outline_problem)
            .field("source_problem", &self.source_problem)
            .finish()
    }
}

/// A Derived feature (see the module docs). It also reads main's pre-merge format (see the
/// module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "DerivedRepr")]
pub struct DerivedFeature {
    /// The source Part Studio; `None` while the dialog waits for one.
    #[serde(default)]
    pub source: Option<SourceRef>,
    /// The frozen copy in [`Document::linked`] (a version reference).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<ElementId>,
    /// The source tab's, document's and version's names ("Block", "Block source", "V1"; the
    /// version empty at the workspace).
    #[serde(default)]
    pub source_name: String,
    #[serde(default)]
    pub document_name: String,
    #[serde(default)]
    pub version_name: String,
    /// The source's (built) features at the reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studio: Vec<Feature>,
    /// The source's part settings at the reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<PartProps>,
    #[serde(default)]
    pub selection: DerivedSelection,
    /// Mate connectors of this Part Studio (explicit or implicit, the origin included); one copy
    /// each; empty: one copy at the origin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<ConnectorRef>,
    #[serde(default)]
    pub placement: DerivedPlacement,
    /// The mate connectors on the derived parts come too.
    #[serde(default)]
    pub include_connectors: bool,
    /// Every property comes over; off: only the name, material and appearance.
    #[serde(default = "yes")]
    pub include_properties: bool,
    /// Read from main's pre-merge format (see the module docs): its single copy keeps that
    /// format's part ids and face names ([`legacy_part_id`], [`legacy_face_name`]), so the
    /// features after it in a saved document still find the derived parts, faces and edges.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_names: bool,
    /// A PCB Studio board instead of a Part Studio (the source is then the PCB Studio tab).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<Box<DerivedBoard>>,
}

fn yes() -> bool {
    true
}

/// What a saved Derived feature can hold: this format's fields and those of main's pre-merge
/// format (see the module docs), read into a [`DerivedFeature`] by `From`.
#[derive(Deserialize)]
#[serde(rename = "DerivedFeature")]
struct DerivedRepr {
    #[serde(default)]
    source: Option<SourceRef>,
    #[serde(default)]
    copy: Option<ElementId>,
    #[serde(default)]
    source_name: String,
    #[serde(default)]
    document_name: String,
    #[serde(default)]
    version_name: String,
    #[serde(default)]
    studio: Vec<Feature>,
    #[serde(default)]
    props: Vec<PartProps>,
    #[serde(default)]
    selection: DerivedSelection,
    #[serde(default)]
    locations: Vec<ConnectorRef>,
    #[serde(default)]
    placement: DerivedPlacement,
    #[serde(default)]
    include_connectors: bool,
    #[serde(default = "yes")]
    include_properties: bool,
    #[serde(default)]
    legacy_names: bool,
    #[serde(default)]
    board: Option<Box<DerivedBoard>>,
    // Main's pre-merge fields (not flattened: RON can't read flattened structs).
    #[serde(default)]
    document: Option<DocumentId>,
    #[serde(default)]
    element: Option<ElementId>,
    #[serde(default)]
    #[allow(dead_code)]
    version: Option<String>,
    #[serde(default)]
    parts: Vec<PartId>,
    #[serde(default, deserialize_with = "present")]
    include_mate_connectors: Option<bool>,
}

/// A bare `bool` read as `Some` (so a missing one is told apart).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    bool::deserialize(d).map(Some)
}

impl From<DerivedRepr> for DerivedFeature {
    fn from(r: DerivedRepr) -> Self {
        let mut d = DerivedFeature {
            source: r.source,
            copy: r.copy,
            source_name: r.source_name,
            document_name: r.document_name,
            version_name: r.version_name,
            studio: r.studio,
            props: r.props,
            selection: r.selection,
            locations: r.locations,
            placement: r.placement,
            include_connectors: r.include_connectors,
            include_properties: r.include_properties,
            legacy_names: r.legacy_names,
            board: r.board.map(|mut b| {
                b.hash = crate::pcb::board3d::layout_hash(&b.layout);
                b
            }),
        };
        let is_legacy = r.document.is_some()
            || r.element.is_some()
            || !r.parts.is_empty()
            || r.include_mate_connectors.is_some();
        if d.source.is_none() && is_legacy {
            d.source = r.element.map(|element| SourceRef {
                document: r.document,
                at: RefAt::Workspace,
                element,
                pinned: false,
            });
            d.selection = if r.parts.is_empty() {
                DerivedSelection::default()
            } else {
                DerivedSelection {
                    all: false,
                    parts: r.parts,
                    ..DerivedSelection::default()
                }
            };
            d.include_connectors = r.include_mate_connectors.unwrap_or(true);
            d.legacy_names = true;
        }
        d
    }
}

impl Default for DerivedFeature {
    fn default() -> Self {
        Self {
            source: None,
            copy: None,
            source_name: String::new(),
            document_name: String::new(),
            version_name: String::new(),
            studio: Vec::new(),
            props: Vec::new(),
            selection: DerivedSelection::default(),
            locations: Vec::new(),
            placement: DerivedPlacement::BaseOrigin,
            include_connectors: false,
            include_properties: true,
            legacy_names: false,
            board: None,
        }
    }
}

impl DerivedFeature {
    /// Why it can't be built, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        if self.source.is_none() {
            return Some("Select a Part Studio or a board to derive");
        }
        if self.selection.is_empty() {
            return Some("Select parts, sketches, planes or mate connectors to derive");
        }
        if self.placement == DerivedPlacement::BaseConnector(None) {
            return Some("Select the base mate connector");
        }
        None
    }

    /// How many copies it makes (one per location; one at the origin without locations).
    pub fn copies(&self) -> usize {
        self.locations.len().max(1)
    }

    /// "Block source › Block (V1)", "Block (this document)".
    pub fn describe(&self) -> String {
        let source = match &self.board {
            Some(b) => format!("{} › {}", self.source_name, b.board_name),
            None => self.source_name.clone(),
        };
        match (self.document_name.is_empty(), self.version_name.is_empty()) {
            (true, _) => source,
            (false, true) => format!("{} › {}", self.document_name, source),
            (false, false) => format!(
                "{} › {} ({})",
                self.document_name, source, self.version_name
            ),
        }
    }

    /// Takes the board's layout and names from the PCB Studio `el` (the source tab, or its
    /// copy): the board [`DerivedBoard::board`] names, else the studio's shown one. `Err` when
    /// the studio has no such board, or it isn't designed in cadrs.
    pub fn fill_board_from(
        &mut self,
        el: &Element,
        document: &str,
        version: &str,
    ) -> Result<(), String> {
        let studio = el
            .pcb()
            .ok_or_else(|| format!("The tab {} is not a PCB Studio", el.name))?;
        let want = self
            .board
            .as_ref()
            .map(|b| b.board)
            .or(studio.active)
            .or(studio.boards.first().map(|b| b.id));
        let b = want
            .and_then(|id| studio.board(id))
            .ok_or_else(|| format!("{} has no such board", el.name))?;
        // (An IDF board: its outline, holes and package boxes.)
        let (layout, hash) = b.derived_layout();
        // A board taken for the first time (or another board) comes in with its outline's centre
        // where it is now on the location (fixed: later outline edits don't move it); one
        // already derived keeps its options.
        let mut options = self
            .board
            .as_ref()
            .map(|x| x.options.clone())
            .unwrap_or_default();
        if self.board.as_ref().is_some_and(|x| x.board != b.id) {
            options.anchor = crate::pcb::board3d::BoardAnchor::Pending;
        }
        if options.anchor == crate::pcb::board3d::BoardAnchor::Pending {
            let [x, y] = crate::pcb::board3d::outline_center(&layout);
            options.anchor = crate::pcb::board3d::BoardAnchor::At { x, y };
        }
        let mut db = DerivedBoard::new(b.id, b.name(), Default::default());
        db.set_layout_hashed(layout.into_owned(), hash);
        db.options = options;
        db.on_sketch = self.board.as_ref().and_then(|x| x.on_sketch);
        self.board = Some(Box::new(db));
        self.studio = Vec::new();
        self.props = Vec::new();
        self.source_name = el.name.clone();
        self.document_name = document.to_string();
        self.version_name = version.to_string();
        Ok(())
    }

    pub fn includes_part(&self, p: PartId) -> bool {
        self.selection.all || self.selection.parts.contains(&p)
    }

    pub fn includes_sketch(&self, s: FeatureId) -> bool {
        self.selection.all || self.selection.sketches.contains(&s)
    }

    pub fn includes_plane(&self, s: FeatureId) -> bool {
        self.selection.all || self.selection.planes.contains(&s)
    }

    /// Whether the source's mate connector `c` (owned by the part `owner`, if any) comes over.
    pub fn includes_connector(&self, c: FeatureId, owner: Option<PartId>) -> bool {
        self.selection.connectors.contains(&c)
            || (self.include_connectors
                && (self.selection.all || owner.is_some_and(|o| self.includes_part(o))))
    }

    /// The features of this Part Studio it refers to (its locations' connectors).
    pub fn parents(&self) -> Vec<FeatureId> {
        self.locations.iter().filter_map(|c| c.parent()).collect()
    }

    /// Takes the source's features, part settings and names from `el` (the source tab, or its
    /// copy), `document` and `version`.
    pub fn fill_from(&mut self, el: &Element, document: &str, version: &str) {
        self.studio = el.active_features();
        self.props = el.part_props().to_vec();
        self.source_name = el.name.clone();
        self.document_name = document.to_string();
        self.version_name = version.to_string();
    }
}

fn uuid_of(bytes: &[u8]) -> uuid::Uuid {
    let hi = stable_hash(bytes);
    let mut b = bytes.to_vec();
    b.push(0xd5);
    let lo = stable_hash(&b);
    uuid::Uuid::from_u128((u128::from(hi) << 64) | u128::from(lo))
}

impl DerivedFeature {
    /// The id copy `k` of the source part `source` gets from this feature (`feature`):
    /// [`derived_part`], or [`legacy_part_id`] for the one copy of a legacy one.
    pub fn part_of(&self, feature: FeatureId, source: PartId, k: usize) -> PartId {
        if self.legacy_names && k == 0 {
            legacy_part_id(feature, source)
        } else {
            derived_part(feature, source, k)
        }
    }

    /// The instance number copy `k`'s faces get ([`derived_face`]: `k + 1`; a legacy feature's
    /// one copy: 0).
    pub fn instance(&self, k: usize) -> u32 {
        if self.legacy_names && k == 0 {
            0
        } else {
            k as u32 + 1
        }
    }

    /// The name copy `k` gives the source face `n`.
    pub fn face_of(&self, feature: FeatureId, n: &FaceName, k: usize) -> FaceName {
        FaceName::new(
            feature.0,
            FaceOrigin::Instance {
                of: n.op,
                face: face_hash(n),
                instance: self.instance(k),
            },
        )
    }
}

/// The part id main's pre-merge Derived feature gave the source part `source`.
pub fn legacy_part_id(feature: FeatureId, source: PartId) -> PartId {
    let mut bytes = source.feature.0.as_bytes().to_vec();
    bytes.extend_from_slice(&source.index.to_le_bytes());
    PartId::new(feature, (stable_hash(&bytes) & 0x7fff_ffff) as u32)
}

/// The face name main's pre-merge Derived feature gave the source face `source` (instance 0).
pub fn legacy_face_name(feature: FeatureId, source: &FaceName) -> FaceName {
    FaceName::new(
        feature.0,
        FaceOrigin::Instance {
            of: source.op,
            face: face_hash(source),
            instance: 0,
        },
    )
}

/// The id of copy `k` (0, 1, …) of the source part `source` made by the Derived feature
/// `feature`: the feature is the Derived one, so the host never has it otherwise.
pub fn derived_part(feature: FeatureId, source: PartId, k: usize) -> PartId {
    let mut b = b"cadrs-derived-part:".to_vec();
    b.extend_from_slice(source.feature.0.as_bytes());
    b.extend_from_slice(&source.index.to_le_bytes());
    b.extend_from_slice(&(k as u64).to_le_bytes());
    PartId::new(feature, stable_hash(&b) as u32)
}

/// The id of copy `k` of the source sketch, plane or mate connector `source` made by the Derived
/// feature `feature`.
pub fn derived_entity(feature: FeatureId, source: FeatureId, k: usize) -> FeatureId {
    let mut b = b"cadrs-derived:".to_vec();
    b.extend_from_slice(feature.0.as_bytes());
    b.extend_from_slice(source.0.as_bytes());
    b.extend_from_slice(&(k as u64).to_le_bytes());
    FeatureId(uuid_of(&b))
}

/// The name copy `k` of the Derived feature `feature` gives the source face named `n` (as a
/// pattern instance's: [`FaceOrigin::Instance`] under the Derived feature, instance `k + 1`).
pub fn derived_face(feature: FeatureId, n: &FaceName, k: usize) -> FaceName {
    FaceName::new(
        feature.0,
        FaceOrigin::Instance {
            of: n.op,
            face: face_hash(n),
            instance: k as u32 + 1,
        },
    )
}

/// What a Derived feature brought in, as the rebuild made it (the Feature list's children,
/// DV3.5): each copy's parts, sketches, planes and mate connectors, with their names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DerivedOutput {
    pub parts: Vec<PartId>,
    pub sketches: Vec<(FeatureId, String)>,
    pub planes: Vec<(FeatureId, String)>,
    pub connectors: Vec<(FeatureId, String)>,
    /// A derived board's copies (`docs/PCB_IN_PART_STUDIO.md`): each copy's board part and
    /// where its board frame is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boards: Vec<BoardCopy>,
    /// A derived board's component parts and the footprints they show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprints: Vec<(PartId, uuid::Uuid)>,
    /// A derived board's components that run into a part before the feature: the component's
    /// part, the other part and how far ([`crate::pcb::fit::collisions`]). The feature's
    /// warning says so; the app names them as the Parts list does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collisions: Vec<crate::pcb::fit::Collision<PartId>>,
    /// Components a later Transform moved apart from their board: the motion after their
    /// place on the layout (an export makes their solids there).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<(PartId, cadrs_kernel::Motion)>,
}

impl DerivedOutput {
    /// How a later Transform moved component `part` apart from its board, if it did.
    pub fn moved_apart(&self, part: PartId) -> Option<cadrs_kernel::Motion> {
        self.moved.iter().find(|(p, _)| *p == part).map(|(_, m)| *m)
    }

    /// The component parts of the derived board copy whose board part is `board` (they follow
    /// it in [`DerivedOutput::parts`], up to the next copy's board part).
    pub fn components_of(&self, board: PartId) -> Vec<PartId> {
        let Some(start) = self.parts.iter().position(|p| *p == board) else {
            return vec![];
        };
        self.parts[start + 1..]
            .iter()
            .take_while(|p| !self.boards.iter().any(|b| b.part == **p))
            .copied()
            .collect()
    }
}

/// One copy of a derived board: its board part, and its **board frame** in the Part Studio (the
/// layout's origin with its x and y axes, on the board's bottom face; the top face is the
/// board's thickness along `u × v`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardCopy {
    pub part: PartId,
    pub frame: cadrs_sketch::PlaneFrame,
    pub thickness: f64,
}

// ---------------------------------------------------------------------------------------------
// Resolving a source

/// A source resolved for a Derived feature: the feature filled from it and the copies it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub derived: DerivedFeature,
    pub links: Vec<LinkedElement>,
}

/// Points `d` at `r` (resolved through `res`; `log` is `doc`'s own history, for versions of this
/// document): its features, part settings and names from the source, and the copies it needs.
pub fn resolve(
    res: &mut Resolver,
    doc: &Document,
    log: Option<&HistoryLog>,
    mut d: DerivedFeature,
    r: SourceRef,
) -> Result<Resolved, LinkError> {
    let this = r.document_or(doc.id) == doc.id;
    let r = SourceRef {
        document: if this { None } else { r.document },
        ..r
    };
    if r.at == RefAt::Workspace {
        if !this {
            return Err(LinkError::NoVersion);
        }
        let el = doc
            .elements
            .iter()
            .find(|e| e.id == r.element)
            .ok_or_else(|| LinkError::NotFound("The Part Studio".into()))?;
        if matches!(el.kind, ElementKind::PcbStudio(_)) {
            d.fill_board_from(el, "", "")
                .map_err(LinkError::NotLinkable)?;
            d.source = Some(r);
            d.copy = None;
            return Ok(Resolved {
                derived: d,
                links: Vec::new(),
            });
        }
        if !matches!(el.kind, ElementKind::PartStudio { .. }) {
            return Err(LinkError::NotLinkable(format!("The tab {}", el.name)));
        }
        d.board = None;
        d.fill_from(el, "", "");
        d.source = Some(r);
        d.copy = None;
        return Ok(Resolved {
            derived: d,
            links: Vec::new(),
        });
    }
    let snap = res.resolve(r, doc, log)?;
    let root = snap
        .root_link()
        .ok_or_else(|| LinkError::NotFound("The Part Studio".into()))?;
    let doc_name = if this {
        doc.name.clone()
    } else {
        root.document_name.clone()
    };
    if matches!(root.element.kind, ElementKind::PcbStudio(_)) {
        d.fill_board_from(&root.element, &doc_name, &root.version_name)
            .map_err(LinkError::NotLinkable)?;
        d.source = Some(r);
        d.copy = Some(snap.root);
        return Ok(Resolved {
            derived: d,
            links: snap.links,
        });
    }
    if !matches!(root.element.kind, ElementKind::PartStudio { .. }) {
        return Err(LinkError::NotLinkable(format!(
            "The tab {}",
            root.element.name
        )));
    }
    d.board = None;
    d.fill_from(&root.element, &doc_name, &root.version_name);
    d.source = Some(r);
    d.copy = Some(snap.root);
    Ok(Resolved {
        derived: d,
        links: snap.links,
    })
}

// ---------------------------------------------------------------------------------------------
// Rules

/// DV3.7, DV1.5: refuses deriving the host itself, deriving one Part Studio twice in a Part
/// Studio, and circular chains (through workspace references in this document and through the
/// copies `links` of a version reference).
pub fn check(
    doc: &Document,
    element: ElementId,
    feature: FeatureId,
    d: &DerivedFeature,
    links: &[LinkedElement],
) -> Result<(), LinkError> {
    let Some(r) = d.source else { return Ok(()) };
    let host = doc
        .elements
        .iter()
        .find(|e| e.id == element)
        .ok_or_else(|| LinkError::NotFound("The Part Studio".into()))?;
    let source_doc = r.document_or(doc.id);
    if source_doc == doc.id && r.element == element {
        if r.at == RefAt::Workspace {
            return Err(LinkError::Refused(format!(
                "{} can't derive itself",
                host.name
            )));
        }
        // A version of itself: its later features would be built from an older copy of itself.
        return Err(LinkError::Circular(format!(
            "{} › {} → {} › {} ({}) → {}",
            doc.name, host.name, doc.name, host.name, d.version_name, doc.name
        )));
    }
    for f in host.features() {
        if f.id == feature {
            continue;
        }
        if let FeatureKind::Derived(o) = &f.kind
            && let Some(or) = o.source
            && or.document_or(doc.id) == source_doc
            && or.element == r.element
            && o.board.as_ref().map(|b| b.board) == d.board.as_ref().map(|b| b.board)
        {
            if let Some(b) = &d.board {
                return Err(LinkError::Refused(format!(
                    "{} is derived already in {} by {}: a board can be derived only once in a Part Studio (add locations for more copies)",
                    b.board_name, host.name, f.name
                )));
            }
            let what = if d.source_name.is_empty() {
                "This Part Studio".to_string()
            } else {
                d.source_name.clone()
            };
            return Err(LinkError::Refused(format!(
                "{what} is derived already in {} by {}: a Part Studio can be derived only once in a Part Studio",
                host.name, f.name
            )));
        }
    }
    if !links.is_empty() {
        let root = d.copy.and_then(|c| links.iter().find(|l| l.id() == c));
        check_cycle(doc, element, links, root)?;
    }
    // Same-document workspace chains, and the copies along them.
    if source_doc == doc.id && r.at == RefAt::Workspace {
        let mut seen: Vec<ElementId> = Vec::new();
        let mut stack = vec![(r.element, vec![r.element])];
        while let Some((e, path)) = stack.pop() {
            if seen.contains(&e) {
                continue;
            }
            seen.push(e);
            let Some(el) = doc.elements.iter().find(|x| x.id == e) else {
                continue;
            };
            for f in el.features() {
                let FeatureKind::Derived(o) = &f.kind else {
                    continue;
                };
                let Some(or) = o.source else { continue };
                if or.document_or(doc.id) == doc.id && or.at == RefAt::Workspace {
                    if or.element == element {
                        let names: Vec<String> = std::iter::once(element)
                            .chain(path.iter().copied())
                            .map(|x| format!("{} › {}", doc.name, name_of(doc, x)))
                            .collect();
                        return Err(LinkError::Circular(format!(
                            "{} → {}",
                            names.join(" → "),
                            doc.name
                        )));
                    }
                    let mut p = path.clone();
                    p.push(or.element);
                    stack.push((or.element, p));
                } else if let Some(c) = o.copy {
                    let chain: Vec<LinkedElement> = crate::link_update::closure(doc, c)
                        .into_iter()
                        .cloned()
                        .collect();
                    check_cycle(doc, element, &chain, chain.first())?;
                }
            }
        }
    }
    Ok(())
}

fn name_of(doc: &Document, e: ElementId) -> String {
    doc.elements
        .iter()
        .find(|x| x.id == e)
        .map(|x| x.name.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Commands

fn add_links(doc: &mut Document, links: &[LinkedElement]) {
    for l in links {
        if doc.linked_element(l.id()).is_none() {
            doc.linked.push(l.clone());
        }
    }
}

fn derived_mut(
    doc: &mut Document,
    element: ElementId,
    feature: FeatureId,
) -> Result<&mut DerivedFeature, CommandError> {
    let el = doc
        .element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?;
    let ElementKind::PartStudio { features, .. } = &mut el.kind else {
        return Err(CommandError::Invalid("features need a Part Studio".into()));
    };
    match features
        .iter_mut()
        .find(|f| f.id == feature)
        .map(|f| &mut f.kind)
    {
        Some(FeatureKind::Derived(d)) => Ok(d),
        _ => Err(CommandError::Invalid("Derived feature not found".into())),
    }
}

/// Inserts a Derived feature ("Derived N") with the copies its source needs; one undo step over
/// the whole document (copies are the document's). Refused when it breaks a rule ([`check`]).
#[derive(Debug, Clone)]
pub struct AddDerived {
    pub element: ElementId,
    pub feature: FeatureId,
    pub derived: DerivedFeature,
    pub links: Vec<LinkedElement>,
}

impl Command for AddDerived {
    fn label(&self) -> String {
        "Insert derived".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check(doc, self.element, self.feature, &self.derived, &self.links)?;
        crate::commands::AddFeature {
            element: self.element,
            feature: self.feature,
            base_name: "Derived".into(),
            kind: FeatureKind::Derived(Box::new(self.derived.clone())),
        }
        .apply(doc)?;
        add_links(doc, &self.links);
        Ok(())
    }
}

/// Sets a Derived feature's parameters (its dialog: the source, what it derives, the locations,
/// the placement, the options), with the copies a new source needs; the copies nothing uses any
/// more are dropped. One undo step over the whole document; refused when it breaks a rule.
#[derive(Debug, Clone)]
pub struct SetDerived {
    pub element: ElementId,
    pub feature: FeatureId,
    pub derived: DerivedFeature,
    pub links: Vec<LinkedElement>,
    pub label: String,
}

impl Command for SetDerived {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check(doc, self.element, self.feature, &self.derived, &self.links)?;
        add_links(doc, &self.links);
        let d = derived_mut(doc, self.element, self.feature)?;
        let old = d.copy;
        *d = self.derived.clone();
        if let Some(o) = old.filter(|o| Some(*o) != self.derived.copy) {
            crate::link_update::prune(doc, &[o]);
        }
        Ok(())
    }
}

/// The Derived feature `feature` of `element`, if it is one.
pub fn derived_of(
    doc: &Document,
    element: ElementId,
    feature: FeatureId,
) -> Option<&DerivedFeature> {
    match &doc.element(element)?.feature(feature)?.kind {
        FeatureKind::Derived(d) => Some(d),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping workspace references live, and the derived parts' settings

/// Brings every same-document workspace Derived feature up to date with its source (DV3.4,
/// ex-dv3), chains in order, and every derived part's settings with its source's
/// ([`DerivedFeature::props`]; Include properties). Called by the undo history after every
/// command, undo and redo; cheap when there are no Derived features.
pub fn refresh(doc: &mut Document) {
    // P3I.9: Form features that use a form Part Studio of this document follow it the same way.
    crate::sheetmetal_form::refresh(doc);
    // Board outlines that follow a sketch, before the boards are taken into Part Studios.
    crate::pcb::outline_link::refresh(doc);
    // CAM Studio components that follow a Part Studio's workspace.
    crate::cam::refresh(doc);
    if !any_derived(doc) {
        return;
    }
    refresh_boards(doc);
    // The importer's workspace references to other documents (see the module docs).
    if let Some(l) = global_loader() {
        fill_other_documents(doc, &*l);
    }
    let this = doc.id;
    for _ in 0..8 {
        type Update = (usize, usize, Vec<Feature>, Vec<PartProps>, String);
        let mut updates: Vec<Update> = Vec::new();
        for (i, el) in doc.elements.iter().enumerate() {
            for (j, f) in el.features().iter().enumerate() {
                let FeatureKind::Derived(d) = &f.kind else {
                    continue;
                };
                let Some(r) = d.source else { continue };
                if d.board.is_some()
                    || r.at != RefAt::Workspace
                    || r.document_or(this) != this
                    || r.element == el.id
                {
                    continue;
                }
                let Some(src) = doc.elements.iter().find(|e| {
                    e.id == r.element && matches!(e.kind, ElementKind::PartStudio { .. })
                }) else {
                    continue;
                };
                let features = src.active_features();
                if d.studio != features || d.props != src.part_props() || d.source_name != src.name
                {
                    updates.push((i, j, features, src.part_props().to_vec(), src.name.clone()));
                }
            }
        }
        if updates.is_empty() {
            break;
        }
        for (i, j, features, props, name) in updates {
            if let ElementKind::PartStudio { features: list, .. } = &mut doc.elements[i].kind
                && let FeatureKind::Derived(d) = &mut list[j].kind
            {
                d.studio = features;
                d.props = props;
                d.source_name = name;
            }
        }
    }
    for el in &mut doc.elements {
        sync_props(el);
    }
}

/// Brings every same-document workspace board reference up to date with its board's layout
/// and names (`docs/PCB_IN_PART_STUDIO.md` decision 2: a layout edit anywhere is one
/// `SetDesign` on the PCB Studio, and every Part Studio deriving the board follows).
fn refresh_boards(doc: &mut Document) {
    let this = doc.id;
    /// (tab, feature, the new layout if it changed, the board's name, the studio's name, the
    /// outline link's problem, why the source is gone).
    type Update = (
        usize,
        usize,
        Option<(cadrs_eda::board::Board, u64)>,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let mut updates: Vec<Update> = Vec::new();
    for (i, el) in doc.elements.iter().enumerate() {
        for (j, f) in el.features().iter().enumerate() {
            let FeatureKind::Derived(d) = &f.kind else {
                continue;
            };
            let (Some(r), Some(db)) = (d.source, d.board.as_ref()) else {
                continue;
            };
            if r.at != RefAt::Workspace || r.document_or(this) != this {
                continue;
            }
            // Its studio or board gone: the last layout stays, with why.
            let src = doc.elements.iter().find(|e| e.id == r.element);
            let Some(b) = src.and_then(|s| s.pcb()).and_then(|s| s.board(db.board)) else {
                let why = match src {
                    None => format!(
                        "Its PCB Studio, {}, is gone: the board stays as it was",
                        d.source_name
                    ),
                    Some(s) => format!(
                        "{} is gone from {}: the board stays as it was",
                        db.board_name, s.name
                    ),
                };
                if db.source_problem.as_ref() != Some(&why) {
                    updates.push((
                        i,
                        j,
                        None,
                        db.board_name.clone(),
                        d.source_name.clone(),
                        db.outline_problem.clone(),
                        Some(why),
                    ));
                }
                continue;
            };
            let src = src.expect("found with its board");
            let (layout, hash) = b.derived_layout();
            let layout = (hash != db.hash).then(|| (layout.into_owned(), hash));
            let problem = b.outline_link.as_ref().and_then(|l| l.problem.clone());
            if layout.is_some()
                || b.name() != db.board_name
                || src.name != d.source_name
                || problem != db.outline_problem
                || db.source_problem.is_some()
            {
                updates.push((
                    i,
                    j,
                    layout,
                    b.name().to_string(),
                    src.name.clone(),
                    problem,
                    None,
                ));
            }
        }
    }
    for (i, j, layout, board_name, studio_name, problem, gone) in updates {
        if let ElementKind::PartStudio { features, .. } = &mut doc.elements[i].kind
            && let FeatureKind::Derived(d) = &mut features[j].kind
            && let Some(db) = d.board.as_mut()
        {
            if let Some((l, hash)) = layout {
                db.set_layout_hashed(l, hash);
            }
            db.board_name = board_name;
            db.outline_problem = problem;
            db.source_problem = gone;
            d.source_name = studio_name;
        }
    }
}

/// The host's settings of the parts its Derived features make: the source part's appearance,
/// face appearances, material and (with Include properties) its properties; the host keeps its
/// own rename and hidden flag. Settings of derived parts that are gone are dropped.
fn sync_props(el: &mut Element) {
    let ElementKind::PartStudio {
        features, parts, ..
    } = &mut el.kind
    else {
        return;
    };
    let derived: Vec<(FeatureId, &DerivedFeature)> = features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Derived(d) => Some((f.id, &**d)),
            _ => None,
        })
        .collect();
    let owners: Vec<FeatureId> = derived.iter().map(|(id, _)| *id).collect();
    let mut want: Vec<PartProps> = Vec::new();
    for (id, d) in &derived {
        for p in d.props.iter().filter(|p| d.includes_part(p.part)) {
            for k in 0..d.copies() {
                let part = d.part_of(*id, p.part, k);
                want.push(PartProps {
                    part,
                    name: None,
                    hidden: false,
                    appearance: p.appearance,
                    faces: p
                        .faces
                        .iter()
                        .map(|(n, a)| (d.face_of(*id, n, k), *a))
                        .collect(),
                    material: p.material.clone(),
                    properties: if d.include_properties {
                        p.properties.clone()
                    } else {
                        Default::default()
                    },
                });
            }
        }
    }
    let mut out: Vec<PartProps> = Vec::with_capacity(parts.len());
    for p in parts.iter() {
        if !owners.contains(&p.part.feature) {
            out.push(p.clone());
            continue;
        }
        // A derived part: its source's settings now, the host's rename and hidden flag kept.
        if let Some(w) = want.iter().find(|w| w.part == p.part) {
            out.push(PartProps {
                name: p.name.clone(),
                hidden: p.hidden,
                ..w.clone()
            });
        } else if p.name.is_some() || p.hidden {
            out.push(PartProps {
                part: p.part,
                name: p.name.clone(),
                hidden: p.hidden,
                appearance: None,
                faces: Vec::new(),
                material: None,
                properties: Default::default(),
            });
        }
    }
    for w in want {
        if !out.iter().any(|p| p.part == w.part) {
            out.push(w);
        }
    }
    if *parts != out {
        *parts = out;
    }
}

/// Every Derived feature of `doc`: (its tab, the feature, its parameters).
pub fn all(doc: &Document) -> Vec<(ElementId, FeatureId, &DerivedFeature)> {
    let mut out = Vec::new();
    for el in &doc.elements {
        for f in el.features() {
            if let FeatureKind::Derived(d) = &f.kind {
                out.push((el.id, f.id, &**d));
            }
        }
    }
    out
}

/// The source document of a Derived feature (`this` for a same-document source).
pub fn source_document(d: &DerivedFeature, this: DocumentId) -> Option<DocumentId> {
    d.source.map(|r| r.document_or(this))
}

// ---------------------------------------------------------------------------------------------
// The Onshape importer's API (see the module docs)

impl DerivedFeature {
    /// Every part, sketch and plane of the Part Studio `element` of `document` (`None`: this
    /// document) at its workspace (see the module docs).
    pub fn new(document: Option<DocumentId>, element: ElementId) -> Self {
        Self {
            source: Some(SourceRef {
                document,
                at: RefAt::Workspace,
                element,
                pinned: false,
            }),
            ..Self::default()
        }
    }
}

/// The id the (single) copy of the source part `source` gets: [`derived_part`] copy 0.
pub fn part_id(feature: FeatureId, source: PartId) -> PartId {
    derived_part(feature, source, 0)
}

/// The name the (single) copy gives the source face `source`: [`derived_face`] copy 0.
pub fn face_name(feature: FeatureId, source: &FaceName) -> FaceName {
    derived_face(feature, source, 0)
}

/// The name the (single) copy gives the source edge `source`.
pub fn edge_name(feature: FeatureId, source: &cadrs_kernel::EdgeName) -> cadrs_kernel::EdgeName {
    cadrs_kernel::EdgeName::new(
        face_name(feature, &source.faces[0]),
        face_name(feature, &source.faces[1]),
        source.index,
    )
}

/// Loads a document by id.
pub type DocumentLoader = dyn Fn(DocumentId) -> Option<Document> + Send + Sync;

fn loader_slot() -> &'static std::sync::RwLock<Option<std::sync::Arc<DocumentLoader>>> {
    static SLOT: std::sync::OnceLock<std::sync::RwLock<Option<std::sync::Arc<DocumentLoader>>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(Default::default)
}

/// Sets how workspace references to other documents are filled (the app and the importer: from
/// their Store). `None` removes it.
pub fn set_document_loader(loader: Option<std::sync::Arc<DocumentLoader>>) {
    if let Ok(mut s) = loader_slot().write() {
        *s = loader;
    }
}

/// A loader reading documents from `store`, each parsed once per version of its file.
pub fn store_loader(store: crate::Store) -> std::sync::Arc<DocumentLoader> {
    type Cache =
        std::collections::HashMap<DocumentId, (Option<std::time::SystemTime>, Option<Document>)>;
    let cache: std::sync::Mutex<Cache> = std::sync::Mutex::new(Cache::new());
    std::sync::Arc::new(move |id| {
        let stamp = std::fs::metadata(store.document_path(id))
            .and_then(|m| m.modified())
            .ok();
        if let Ok(c) = cache.lock()
            && let Some((s, d)) = c.get(&id)
            && *s == stamp
        {
            return d.clone();
        }
        let d = store.load(id).ok().map(|f| f.document);
        if let Ok(mut c) = cache.lock() {
            c.insert(id, (stamp, d.clone()));
        }
        d
    })
}

fn global_loader() -> Option<std::sync::Arc<DocumentLoader>> {
    loader_slot().read().ok()?.clone()
}

/// Fills the workspace references to other documents that have no snapshot yet, with `loader`.
fn fill_other_documents(doc: &mut Document, loader: &dyn Fn(DocumentId) -> Option<Document>) {
    let this = doc.id;
    let mut loaded: std::collections::HashMap<DocumentId, Option<Document>> =
        std::collections::HashMap::new();
    for el in &mut doc.elements {
        let ElementKind::PartStudio { features, .. } = &mut el.kind else {
            continue;
        };
        for f in features.iter_mut() {
            let FeatureKind::Derived(d) = &mut f.kind else {
                continue;
            };
            let Some(r) = d.source else { continue };
            if r.at != RefAt::Workspace || !r.is_external(this) || !d.studio.is_empty() {
                continue;
            }
            let other = r.document_or(this);
            let Some(src) = loaded.entry(other).or_insert_with(|| loader(other)) else {
                continue;
            };
            let Some(e) = src
                .elements
                .iter()
                .find(|e| e.id == r.element && matches!(e.kind, ElementKind::PartStudio { .. }))
            else {
                continue;
            };
            d.fill_from(e, &src.name, "");
        }
    }
}

/// [`refresh`], filling workspace references to other documents with the loader set by
/// [`set_document_loader`] (see the module docs).
pub fn resolve_document(doc: &mut Document) {
    refresh(doc);
}

/// [`resolve_document`] with a loader of your own.
pub fn resolve_document_with(doc: &mut Document, loader: &dyn Fn(DocumentId) -> Option<Document>) {
    if !any_derived(doc) {
        return;
    }
    fill_other_documents(doc, loader);
    refresh(doc);
}

/// The source parts a Derived feature can bring in, with their display names (the importer's
/// part selection). Rebuilds the source.
pub fn source_parts(x: &DerivedFeature) -> Result<Vec<(PartId, String)>, String> {
    if x.studio.is_empty() {
        return Err("The source Part Studio has not been loaded".into());
    }
    let build = crate::rebuild::build(&x.studio);
    Ok(build
        .parts
        .iter()
        .map(|p| (p.id, crate::parts::display_name(p, &x.props).to_string()))
        .collect())
}

fn any_derived(doc: &Document) -> bool {
    doc.elements.iter().any(|e| {
        e.features()
            .iter()
            .any(|f| matches!(f.kind, FeatureKind::Derived(_)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_ids_are_stable_and_namespaced() {
        let f = FeatureId::from_u128(1);
        let p = PartId::new(FeatureId::from_u128(2), 0);
        assert_eq!(derived_part(f, p, 0), derived_part(f, p, 0));
        assert_ne!(derived_part(f, p, 0), derived_part(f, p, 1));
        assert_eq!(derived_part(f, p, 0).feature, f);
        assert_ne!(
            derived_entity(f, FeatureId::from_u128(3), 0),
            derived_entity(FeatureId::from_u128(4), FeatureId::from_u128(3), 0)
        );
    }

    #[test]
    fn a_new_derived_feature_waits_for_a_source() {
        let d = DerivedFeature::default();
        assert_eq!(
            d.problem(),
            Some("Select a Part Studio or a board to derive")
        );
        assert_eq!(d.copies(), 1);
        assert!(d.include_properties);
    }
}
