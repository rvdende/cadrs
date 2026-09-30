//! The component library and the workspace-level PCB settings (P3H.4; PCB2.1, PCB11.4–PCB11.6,
//! X6, X10).
//!
//! - **Workspace settings** ([`PcbWorkspace`]): the component library document and the folder
//!   for component documents, stored once per document store in `<root>/pcb-workspace.ron`, so
//!   every PCB Studio of every document sees the same library and folder (X6).
//! - **Mappings** ([`ComponentLibrary`]): package → [`Representation`] (None, From ECAD data,
//!   Custom part with its placement). They live in the **library document**: its first PCB
//!   Studio tab's `library` (PCB2.3: the library document holds an empty PCB Studio tab). With
//!   no library document chosen they live in `<root>/pcb-component-library.ron`.
//! - **Caches**: each PCB Studio element keeps a copy of both (`PcbStudio::settings`,
//!   `PcbStudio::library`), changed only through the undoable commands ([`super::SetPcbSettings`],
//!   [`super::SetRepresentation`]); rendering reads the copy. [`LibrarySync`] keeps the copies
//!   and the shared stores in step: a copy a command (or undo/redo) changed is **pushed** to the
//!   workspace file and the library; a studio seen for the first time (a document opened, a tab
//!   added) **pulls** them. Pulling is not an edit, so it adds no undo step.
//!
//! A custom part is placed in the package frame (the `.emp` outline in x/y, the body from z = 0
//! up) by [`PartTransform`]: first the rotation (about X, then Y, then Z, through the part's
//! origin), then the translation. A component shows it at `placement ∘ transform`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use cadrs_kernel::Motion;
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};

use super::{PcbSettings, PcbStudio};
use crate::document::{Document, Element};
use crate::history_log::VersionId;
use crate::ids::{DocumentId, ElementId, PartId};
use crate::library::{DocumentEntry, DocumentMeta, Timestamp};
use crate::store::{Store, StoreError};

/// The workspace settings file, in the store's root.
pub const WORKSPACE_FILE: &str = "pcb-workspace.ron";
/// The default component library (no library document chosen), in the store's root.
pub const DEFAULT_LIBRARY_FILE: &str = "pcb-component-library.ron";
/// The name of a new library document and of its PCB Studio tab.
pub const LIBRARY_DOCUMENT_NAME: &str = "PCB Component Library";

/// How a package is shown in PCB Studio (PCB11.5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Representation {
    /// Not shown in PCB Studio.
    None,
    /// The generic box from the ECAD outline and height (the default).
    #[default]
    FromEcad,
    /// A part from a Part Studio, placed on the footprint.
    Custom(Box<CustomPart>),
}

impl Representation {
    pub fn label(&self) -> &'static str {
        match self {
            Representation::None => "None",
            Representation::FromEcad => "From ECAD data",
            Representation::Custom(_) => "Custom part",
        }
    }

    pub fn custom(&self) -> Option<&CustomPart> {
        match self {
            Representation::Custom(c) => Some(c.as_ref()),
            _ => None,
        }
    }
}

/// Where a custom part comes from: a part of a Part Studio of a stored document, at a version
/// (`None`: the document as it is now).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartSource {
    pub document: DocumentId,
    pub document_name: String,
    pub element: ElementId,
    pub element_name: String,
    pub part: PartId,
    pub part_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
}

impl PartSource {
    /// "Model › Part Studio 1 › Part 1 (V1)".
    pub fn label(&self) -> String {
        let v = self.version_name.as_deref().map(|v| format!(" ({v})")).unwrap_or_default();
        format!("{} › {} › {}{v}", self.document_name, self.element_name, self.part_name)
    }
}

/// How a custom part sits on its footprint: rotated about X, then Y, then Z (degrees, through
/// the part's origin), then moved by `translate` (mm), into the package frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartTransform {
    pub translate: [f64; 3],
    pub rotate: [f64; 3],
}

fn rot(axis: usize, deg: f64) -> Matrix3<f64> {
    let (s, c) = cadrs_idf::geom::sin_cos_deg(deg);
    match axis {
        0 => Matrix3::new(1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c),
        1 => Matrix3::new(c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c),
        _ => Matrix3::new(c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0),
    }
}

impl PartTransform {
    /// The motion from the part's frame into the package frame.
    pub fn motion(&self) -> Motion {
        let [rx, ry, rz] = self.rotate;
        let linear = rot(2, rz) * rot(1, ry) * rot(0, rx);
        Motion { linear, translation: Vector3::from(self.translate) }
    }

    /// The same placement turned to `rotate`, about the part's middle (the centre of its box,
    /// `pts` in the part's frame), so the part turns in place: the Component pane's Rotate
    /// fields. `translate` changes by `R_old·c − R_new·c`.
    pub fn rotated_in_place(&self, rotate: [f64; 3], pts: &[[f64; 3]]) -> PartTransform {
        if pts.is_empty() {
            return PartTransform { rotate, ..*self };
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in pts {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        let c = Vector3::new((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0);
        let old = PartTransform { translate: [0.0; 3], rotate: self.rotate }.motion().linear;
        let new = PartTransform { translate: [0.0; 3], rotate }.motion().linear;
        let t = Vector3::from(self.translate) + old * c - new * c;
        let clean = |v: f64| if v.abs() < 1e-12 { 0.0 } else { v };
        PartTransform { translate: [clean(t.x), clean(t.y), clean(t.z)], rotate }
    }

    /// Center (PCB11.6): the same rotation, moved so the part (its points `pts`, in the part's
    /// frame) is centred on the footprint's origin in x and y and stands on z = 0.
    pub fn centered(&self, pts: &[[f64; 3]]) -> PartTransform {
        let rotated = PartTransform { translate: [0.0; 3], rotate: self.rotate }.motion();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in pts {
            let q = rotated.point(&nalgebra::Point3::new(p[0], p[1], p[2]));
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
        if pts.is_empty() {
            return PartTransform { translate: [0.0; 3], rotate: self.rotate };
        }
        let clean = |v: f64| if v.abs() < 1e-12 { 0.0 } else { v };
        PartTransform { translate: [clean(-(lo[0] + hi[0]) / 2.0), clean(-(lo[1] + hi[1]) / 2.0), clean(-lo[2])], rotate: self.rotate }
    }
}

/// A custom representation: the part and how it sits on the footprint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomPart {
    pub source: PartSource,
    #[serde(default)]
    pub transform: PartTransform,
}

/// The package → representation mappings (PCB11.4). Packages without an entry are shown From
/// ECAD data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentLibrary {
    #[serde(default)]
    pub mappings: BTreeMap<String, Representation>,
}

impl ComponentLibrary {
    /// The representation of a package.
    pub fn get(&self, package: &str) -> &Representation {
        static DEFAULT: Representation = Representation::FromEcad;
        self.mappings.get(package).unwrap_or(&DEFAULT)
    }

    /// Sets a package's representation (From ECAD data removes the entry).
    pub fn set(&mut self, package: &str, r: Representation) {
        if r == Representation::FromEcad {
            self.mappings.remove(package);
        } else {
            self.mappings.insert(package.to_string(), r);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Workspace settings

/// The contents of `pcb-workspace.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PcbWorkspace {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub settings: PcbSettings,
}

impl PcbWorkspace {
    pub fn path(root: &Path) -> PathBuf {
        root.join(WORKSPACE_FILE)
    }

    /// Reads the workspace settings; a missing or unreadable file gives the defaults.
    pub fn load(root: &Path) -> PcbWorkspace {
        std::fs::read_to_string(Self::path(root)).ok().and_then(|t| ron::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<(), StoreError> {
        write_ron(&Self::path(root), self)
    }
}

fn write_ron<T: Serialize>(path: &Path, v: &T) -> Result<(), StoreError> {
    let text = ron::ser::to_string_pretty(v, ron::ser::PrettyConfig::default()).map_err(|e| StoreError::Parse(e.to_string()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The library document

/// The PCB Studio tab of a library document that holds the mappings: its first one.
pub fn library_element(doc: &Document) -> Option<&Element> {
    doc.elements.iter().find(|e| e.pcb().is_some())
}

/// The mappings a library document holds (none if it has no PCB Studio tab yet).
pub fn library_of(doc: &Document) -> ComponentLibrary {
    library_element(doc).and_then(|e| e.pcb()).map(|s| s.library.clone()).unwrap_or_default()
}

/// Writes the mappings into a library document, adding its PCB Studio tab if it has none.
pub fn set_library_of(doc: &mut Document, lib: &ComponentLibrary) {
    if library_element(doc).is_none() {
        doc.elements.push(Element::pcb_studio(LIBRARY_DOCUMENT_NAME));
    }
    if let Some(s) = doc.elements.iter_mut().find_map(|e| e.pcb_mut()) {
        s.library = lib.clone();
    }
}

/// A new, blank library document (PCB2.3): one empty PCB Studio tab, saved in `store`.
pub fn create_library_document(store: &Store, name: &str, user: &str, now: Timestamp) -> Result<DocumentEntry, StoreError> {
    let mut doc = Document::new(name);
    doc.elements = vec![Element::pcb_studio(LIBRARY_DOCUMENT_NAME)];
    store.create(&doc, &DocumentMeta::new(user, now))
}

/// Where the mappings of `settings` live.
fn library_file(store: &Store) -> PathBuf {
    store.root().join(DEFAULT_LIBRARY_FILE)
}

/// Reads the mappings for `settings`. `open` is the document being edited: when it is the
/// library document itself, its (unsaved) contents are used.
pub fn load_library(store: &Store, settings: &PcbSettings, open: &Document) -> Result<ComponentLibrary, StoreError> {
    match &settings.library {
        Some(l) if l.document == open.id => Ok(library_of(open)),
        Some(l) => Ok(library_of(&store.load(l.document)?.document)),
        None => {
            let p = library_file(store);
            if !p.is_file() {
                return Ok(ComponentLibrary::default());
            }
            let t = std::fs::read_to_string(p)?;
            ron::from_str(&t).map_err(|e| StoreError::Parse(e.to_string()))
        }
    }
}

/// Writes the mappings for `settings` (see [`load_library`] for `open`).
pub fn save_library(store: &Store, settings: &PcbSettings, lib: &ComponentLibrary, open: &mut Document) -> Result<(), StoreError> {
    match &settings.library {
        Some(l) if l.document == open.id => {
            set_library_of(open, lib);
            Ok(())
        }
        Some(l) => {
            let mut f = store.load(l.document)?;
            if library_of(&f.document) == *lib && library_element(&f.document).is_some() {
                return Ok(());
            }
            set_library_of(&mut f.document, lib);
            store.save(&f.document, &f.meta)
        }
        None => write_ron(&library_file(store), lib),
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping the studios' copies in step

/// What one [`LibrarySync::sync`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncOutcome {
    /// Studios whose copies were refreshed from the workspace and library.
    pub pulled: Vec<ElementId>,
    /// The workspace settings were written.
    pub pushed_settings: bool,
    /// The library was written.
    pub pushed_library: bool,
    pub errors: Vec<String>,
}

impl SyncOutcome {
    pub fn changed_document(&self) -> bool {
        !self.pulled.is_empty()
    }
}

/// Keeps every PCB Studio's copy of the workspace settings and the library in step with the
/// shared stores (see the module docs). One per open document.
#[derive(Clone, Debug, Default)]
pub struct LibrarySync {
    /// Per studio, the copies as last seen (after the last push or pull).
    seen: HashMap<ElementId, (PcbSettings, ComponentLibrary)>,
}

fn studio_ids(doc: &Document) -> Vec<ElementId> {
    doc.elements.iter().filter(|e| e.pcb().is_some()).map(|e| e.id).collect()
}

fn copies(s: &PcbStudio) -> (PcbSettings, ComponentLibrary) {
    (s.settings.clone(), s.library.clone())
}

impl LibrarySync {
    /// Forgets every studio (a document was opened or closed): the next sync pulls.
    pub fn reset(&mut self) {
        self.seen.clear();
    }

    /// Pushes the copies commands changed, then pulls into the studios not seen yet (and, after
    /// a push, into the document's other studios). Pulls change `doc` directly (no undo step).
    pub fn sync(&mut self, doc: &mut Document, store: &Store) -> SyncOutcome {
        let mut out = SyncOutcome::default();
        let ids = studio_ids(doc);
        self.seen.retain(|id, _| ids.contains(id));
        // Pushes: copies that changed since they were last seen.
        for id in &ids {
            let Some(cur) = doc.element(*id).and_then(|e| e.pcb()).map(copies) else { continue };
            let Some(prev) = self.seen.get(id).cloned() else { continue };
            if prev == cur {
                continue;
            }
            let mut now = cur.clone();
            if cur.0 != prev.0 {
                let ws = PcbWorkspace { version: 1, settings: cur.0.clone() };
                match ws.save(store.root()) {
                    Ok(()) => out.pushed_settings = true,
                    Err(e) => out.errors.push(format!("PCB settings: {e}")),
                }
            }
            if cur.0.library != prev.0.library && cur.1 == prev.1 {
                // Another library was chosen: show its mappings.
                match load_library(store, &cur.0, doc) {
                    Ok(lib) => {
                        if let Some(s) = doc.element_mut(*id).and_then(|e| e.pcb_mut()) {
                            s.library = lib.clone();
                        }
                        now.1 = lib;
                        out.pulled.push(*id);
                    }
                    Err(e) => out.errors.push(format!("Component library: {e}")),
                }
            } else if cur.1 != prev.1 {
                match save_library(store, &cur.0, &cur.1, doc) {
                    Ok(()) => out.pushed_library = true,
                    Err(e) => out.errors.push(format!("Component library: {e}")),
                }
            }
            self.seen.insert(*id, now);
            // The document's other studios show the same library.
            for other in ids.iter().filter(|o| *o != id) {
                self.seen.remove(other);
            }
        }
        // Pulls: studios not seen yet.
        let unseen: Vec<ElementId> = ids.iter().copied().filter(|id| !self.seen.contains_key(id)).collect();
        if !unseen.is_empty() {
            let settings = PcbWorkspace::load(store.root()).settings;
            let lib = match load_library(store, &settings, doc) {
                Ok(l) => l,
                Err(e) => {
                    out.errors.push(format!("Component library: {e}"));
                    ComponentLibrary::default()
                }
            };
            for id in unseen {
                if let Some(s) = doc.element_mut(id).and_then(|e| e.pcb_mut()) {
                    if s.settings != settings || s.library != lib {
                        s.settings = settings.clone();
                        s.library = lib.clone();
                        out.pulled.push(id);
                    }
                    self.seen.insert(id, (settings.clone(), lib.clone()));
                }
            }
        }
        out
    }
}
