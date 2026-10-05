//! On-disk document storage.
//!
//! Layout: `<root>/<uuid>/document.ron` (a versioned [`DocumentFile`]),
//! `<root>/<uuid>/entry.ron` (its name and metadata, so listing the documents doesn't parse
//! every document) and `<root>/<uuid>/thumbnail.png`. The default root is `<data dir>/cadrs/documents`, where the
//! data dir comes from the `directories` crate (`~/.local/share` on Linux, `%APPDATA%` on
//! Windows). Tests and scenarios pass their own root so they never touch real documents.
//!
//! Files are written to a temporary name and renamed into place, so a crash never leaves a
//! half-written document.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::ids::DocumentId;
use crate::library::{DocumentEntry, DocumentMeta, FolderEntry, LabelEntry, Library, Timestamp};
use crate::thumbnail;

/// The current `document.ron` schema version. Bump it when the format changes and add a
/// migration in [`Store::load`].
///
/// - 1 (M0–M2): sketches were `Sketch((plane, points, curves))`.
/// - 2 (M3): sketch features hold `SketchFeature((plane: Option, disable_imprinting,
///   geometry: (points, curves)))`, so a sketch can exist before its plane is chosen.
/// - 3 (M9): features can be `Extrude((regions, body, op, end, depth, depth_expr, flip))`, and
///   a sketch plane can be a part's face, `Face((feature, face, origin, u, v))`. Version 2
///   files have neither, so they read as they are.
/// - 4 (P3.2): faces and edges are referred to by persistent name (`cadrs_kernel::naming`):
///   a face plane's `face` and the Use/Pierce links' `edge` and `face` are `FaceName`s and
///   `EdgeName`s instead of the prism mesh's `FaceTag`s and `EdgeTag`s. Version 3 files
///   convert losslessly (`cadrs_sketch::legacy`).
///
/// Later milestones add feature kinds and optional fields within version 4 (P3.3: `Boolean`,
/// `DeletePart`, the full extrude; P3.4: `Revolve`, the `Diametral` dimension), so older version
/// 4 files read as they are. P3H.3's `PcbStudio` element kind is additive too: files without
/// one read unchanged.
/// - 5 (P3G.1): external references. Additive only: a document's `linked` elements (frozen
///   copies of referenced elements, [`crate::external`]), an instance's `link`, and a
///   document's `folder` in its metadata. Version 4 files read as they are (every new field
///   defaults), so the migration only changes the number.
pub const SCHEMA_VERSION: u32 = 5;

pub const DOCUMENT_FILE: &str = "document.ron";
pub const THUMBNAIL_FILE: &str = "thumbnail.png";
/// A document's listing entry, a cache of `document.ron`'s head ([`EntryFile`]).
pub const ENTRY_FILE: &str = "entry.ron";
/// Folders (and, since P3E.1, the labels) live in `<root>/folders.ron`.
pub const FOLDERS_FILE: &str = "folders.ron";
/// Permanently deleted documents wait in `<root>/.deleted/` until the app exits, so undo works.
pub const DELETED_DIR: &str = ".deleted";

/// The contents of `folders.ron`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoldersFile {
    pub version: u32,
    pub folders: Vec<FolderEntry>,
    /// P3E.1: the library's labels (additive; older files have none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelEntry>,
}

/// The contents of `document.ron`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentFile {
    pub version: u32,
    pub meta: DocumentMeta,
    pub document: Document,
}

impl DocumentFile {
    pub fn entry(&self) -> DocumentEntry {
        DocumentEntry {
            id: self.document.id,
            name: self.document.name.clone(),
            meta: self.meta.clone(),
        }
    }
}

/// Only the version field, read first so newer files fail with a clear error.
#[derive(Deserialize)]
#[serde(rename = "DocumentFile")]
struct VersionProbe {
    version: u32,
}

/// The contents of `entry.ron`: what the documents page lists, and the size and modified time
/// of the `document.ron` it was taken from. It is only used while those still match, so a
/// document written some other way is read in full again.
#[derive(Serialize, Deserialize)]
struct EntryFile {
    doc_len: u64,
    doc_modified_ns: u64,
    id: DocumentId,
    name: String,
    meta: DocumentMeta,
}

/// The size and modified time (ns since the epoch) of a file.
fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let ns = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some((m.len(), ns as u64))
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Parse(String),
    UnsupportedVersion(u32),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "{e}"),
            StoreError::Parse(e) => write!(f, "invalid document file: {e}"),
            StoreError::UnsupportedVersion(v) => write!(
                f,
                "document schema version {v} is newer than this cadrs supports ({SCHEMA_VERSION})"
            ),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

/// A directory of documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `<data dir>/cadrs/documents` for the current user, if the platform has a data dir.
    pub fn default_root() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.data_dir().join("cadrs").join("documents"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn doc_dir(&self, id: DocumentId) -> PathBuf {
        self.root.join(id.to_string())
    }

    pub fn document_path(&self, id: DocumentId) -> PathBuf {
        self.doc_dir(id).join(DOCUMENT_FILE)
    }

    pub fn thumbnail_path(&self, id: DocumentId) -> PathBuf {
        self.doc_dir(id).join(THUMBNAIL_FILE)
    }

    /// True if the document has no thumbnail, or one saved before `modified` (its last edit):
    /// the app quit with the document open, so the thumbnail was never rendered.
    pub fn thumbnail_outdated(&self, id: DocumentId, modified: Timestamp) -> bool {
        let saved = std::fs::metadata(self.thumbnail_path(id))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        saved.is_none_or(|t| (t.as_secs() as Timestamp) < modified)
    }

    /// Writes `document.ron` for `doc`.
    pub fn save(&self, doc: &Document, meta: &DocumentMeta) -> Result<(), StoreError> {
        let file = DocumentFile {
            version: SCHEMA_VERSION,
            meta: meta.clone(),
            document: doc.clone(),
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        std::fs::create_dir_all(self.doc_dir(doc.id))?;
        let path = self.document_path(doc.id);
        write_atomic(&path, text.as_bytes())?;
        write_entry(&path, &file.entry())?;
        crate::blobs::save_dir(&self.blobs_dir(doc.id), doc)?;
        Ok(())
    }

    /// The folder of a document's imported files ([`crate::blobs`]).
    pub fn blobs_dir(&self, id: DocumentId) -> PathBuf {
        self.doc_dir(id).join(crate::blobs::BLOBS_DIR)
    }

    /// Creates a new document on disk with a placeholder thumbnail.
    pub fn create(&self, doc: &Document, meta: &DocumentMeta) -> Result<DocumentEntry, StoreError> {
        self.save(doc, meta)?;
        self.write_thumbnail(doc.id, &thumbnail::placeholder(thumbnail_seed(&doc.name, meta.created)))?;
        Ok(DocumentEntry {
            id: doc.id,
            name: doc.name.clone(),
            meta: meta.clone(),
        })
    }

    /// Reads a `document.ron`, checking its schema version.
    pub fn load(&self, id: DocumentId) -> Result<DocumentFile, StoreError> {
        Self::load_path(&self.document_path(id))
    }

    /// Reads a `document.ron` and the imported files next to it (into [`crate::blobs`]).
    pub fn load_path(path: &Path) -> Result<DocumentFile, StoreError> {
        if let Some(dir) = path.parent() {
            crate::blobs::load_dir(&dir.join(crate::blobs::BLOBS_DIR))?;
        }
        Self::read_path(path)
    }

    /// Reads a document without loading its imported files (for listing what it holds).
    pub fn read(&self, id: DocumentId) -> Result<DocumentFile, StoreError> {
        Self::read_path(&self.document_path(id))
    }

    /// Reads a `document.ron` only (listing the documents doesn't need their files).
    fn read_path(path: &Path) -> Result<DocumentFile, StoreError> {
        let text = std::fs::read_to_string(path)?;
        // A current file parses in one typed pass (milliseconds). Probing its version first was
        // far slower: ron skips the fields the probe doesn't name value by value, which took
        // seconds for a big document. So only a file that isn't current is probed and migrated.
        if let Ok(file) = ron::from_str::<DocumentFile>(&text)
            && file.version == SCHEMA_VERSION
        {
            return Ok(file);
        }
        let probe: VersionProbe = ron::Options::default()
            .from_str(&text)
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        match probe.version {
            1 => migrate::from_v1(&text),
            2 => migrate::from_v2(&text),
            3 => migrate::from_v3(&text),
            4 => migrate::from_v4(&text),
            SCHEMA_VERSION => ron::from_str(&text).map_err(|e| StoreError::Parse(e.to_string())),
            v => Err(StoreError::UnsupportedVersion(v)),
        }
    }

    /// Reads every document's metadata. Unreadable documents are skipped and reported.
    pub fn list(&self) -> (Library, Vec<(PathBuf, StoreError)>) {
        let mut lib = Library::default();
        let mut errors = Vec::new();
        let Ok(dir) = std::fs::read_dir(&self.root) else {
            return (lib, errors);
        };
        let mut paths: Vec<PathBuf> = dir
            .filter_map(|e| e.ok())
            .map(|e| e.path().join(DOCUMENT_FILE))
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        // Most documents have a current `entry.ron`; parse the rest in full, in parallel, and
        // write their `entry.ron` for next time.
        let cached: Vec<Option<DocumentEntry>> = paths.iter().map(|p| read_entry(p)).collect();
        let missing: Vec<&PathBuf> =
            paths.iter().zip(&cached).filter(|(_, c)| c.is_none()).map(|(p, _)| p).collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunk = missing.len().div_ceil(threads).max(1);
        let mut read: Vec<Result<DocumentEntry, StoreError>> = std::thread::scope(|s| {
            let jobs: Vec<_> = missing
                .chunks(chunk)
                .map(|paths| {
                    s.spawn(move || {
                        paths
                            .iter()
                            .map(|p| {
                                let entry = Self::read_path(p)?.entry();
                                // Only a cache: listing works without it.
                                let _ = write_entry(p, &entry);
                                Ok(entry)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            jobs.into_iter().flat_map(|j| j.join().expect("a document read panicked")).collect::<Vec<_>>()
        })
        .into_iter()
        .rev()
        .collect();
        for (path, cached) in paths.into_iter().zip(cached) {
            match cached.map(Ok).unwrap_or_else(|| read.pop().expect("one read per missing entry")) {
                Ok(e) => lib.entries.push(e),
                Err(e) => errors.push((path, e)),
            }
        }
        let folders = self.root.join(FOLDERS_FILE);
        if folders.is_file() {
            match std::fs::read_to_string(&folders)
                .map_err(StoreError::from)
                .and_then(|t| {
                    ron::from_str::<FoldersFile>(&t).map_err(|e| StoreError::Parse(e.to_string()))
                }) {
                Ok(f) if f.version <= SCHEMA_VERSION => {
                    lib.folders = f.folders;
                    lib.labels = f.labels;
                }
                Ok(f) => errors.push((folders, StoreError::UnsupportedVersion(f.version))),
                Err(e) => errors.push((folders, e)),
            }
        }
        (lib, errors)
    }

    fn deleted_dir(&self, id: DocumentId) -> PathBuf {
        self.root.join(DELETED_DIR).join(id.to_string())
    }

    /// Empties the holding area of permanently deleted documents (call on exit).
    pub fn flush_deleted(&self) -> Result<(), StoreError> {
        let dir = self.root.join(DELETED_DIR);
        if dir.is_dir() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// Writes an entry's name and metadata into its `document.ron`.
    pub fn update_entry(&self, entry: &DocumentEntry) -> Result<(), StoreError> {
        let mut file = Self::read_path(&self.document_path(entry.id))?;
        file.document.name = entry.name.clone();
        file.meta = entry.meta.clone();
        self.save(&file.document, &file.meta)
    }

    /// Writes every entry that differs between `before` and `after` (after a library command,
    /// undo or redo).
    pub fn sync(&self, before: &Library, after: &Library) -> Result<(), StoreError> {
        // Permanently deleted: move aside (undo moves it back).
        for e in &before.entries {
            if after.get(e.id).is_none() && self.doc_dir(e.id).is_dir() {
                std::fs::create_dir_all(self.root.join(DELETED_DIR))?;
                std::fs::rename(self.doc_dir(e.id), self.deleted_dir(e.id))?;
            }
        }
        for e in &after.entries {
            if before.get(e.id).is_none() && !self.doc_dir(e.id).is_dir() {
                if self.deleted_dir(e.id).is_dir() {
                    std::fs::rename(self.deleted_dir(e.id), self.doc_dir(e.id))?;
                } else {
                    continue;
                }
            }
            if before.get(e.id) != Some(e) {
                self.update_entry(e)?;
            }
        }
        if before.folders != after.folders || before.labels != after.labels {
            let file = FoldersFile {
                version: SCHEMA_VERSION,
                folders: after.folders.clone(),
                labels: after.labels.clone(),
            };
            let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
                .map_err(|e| StoreError::Parse(e.to_string()))?;
            std::fs::create_dir_all(&self.root)?;
            write_atomic(&self.root.join(FOLDERS_FILE), text.as_bytes())?;
        }
        Ok(())
    }

    /// Writes a copy of document `source` as a new document `id` called `name`, owned and
    /// created by `user` at `now`, with the source's thumbnail. Returns the new entry (add it to
    /// the library with [`crate::library::AddEntry`]).
    pub fn copy_document(
        &self,
        source: DocumentId,
        id: DocumentId,
        name: &str,
        user: &str,
        now: Timestamp,
    ) -> Result<DocumentEntry, StoreError> {
        let file = self.load(source)?;
        crate::blobs::copy_dir(&self.blobs_dir(source), &self.blobs_dir(id))?;
        let mut doc = file.document;
        doc.id = id;
        doc.name = name.trim().to_string();
        let mut meta = DocumentMeta::new(user, now);
        meta.last_opened = None;
        self.save(&doc, &meta)?;
        match self.read_thumbnail(source) {
            Some(img) => self.write_thumbnail(id, &img)?,
            None => self.write_thumbnail(
                id,
                &thumbnail::placeholder(thumbnail_seed(&doc.name, meta.created)),
            )?,
        }
        Ok(DocumentEntry {
            id,
            name: doc.name,
            meta,
        })
    }

    /// Writes a copy of the document file at `path` (a bundled sample, P3E.1 "Open a copy",
    /// TD4.1) as a new document `id` called `name`, owned and created by `user` at `now`, with
    /// a placeholder thumbnail (the app draws a real one).
    /// Returns the new entry (add it to the library with [`crate::library::AddEntry`]).
    pub fn copy_from_file(
        &self,
        path: &Path,
        id: DocumentId,
        name: &str,
        user: &str,
        now: Timestamp,
    ) -> Result<DocumentEntry, StoreError> {
        let file = Self::load_path(path)?;
        let mut doc = file.document;
        doc.id = id;
        doc.name = name.trim().to_string();
        let meta = DocumentMeta::new(user, now);
        self.create(&doc, &meta)
    }

    pub fn write_thumbnail(&self, id: DocumentId, img: &RgbaImage) -> Result<(), StoreError> {
        std::fs::create_dir_all(self.doc_dir(id))?;
        let mut bytes = Vec::new();
        img.write_to(&mut io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|e| StoreError::Io(io::Error::other(e)))?;
        write_atomic(&self.thumbnail_path(id), &bytes)?;
        Ok(())
    }

    /// The document's thumbnail as RGBA8, if it has one.
    pub fn read_thumbnail(&self, id: DocumentId) -> Option<RgbaImage> {
        image::open(self.thumbnail_path(id))
            .ok()
            .map(|i| i.to_rgba8())
    }

    /// Fills the store with `n` sample documents with fixed ids, names and timestamps (for
    /// scenarios and screenshots). `now` is the newest modification time.
    pub fn seed_samples(&self, n: usize, user: &str, now: Timestamp) -> Result<(), StoreError> {
        const NAMES: [&str; 16] = [
            "Motor mount",
            "Bracket v2",
            "Enclosure lid",
            "Hinge",
            "camera_plate",
            "Gearbox housing",
            "Pi case",
            "Shelf bracket",
            "Pulley 40T",
            "Cable clip",
            "Drone arm",
            "Knob",
            "Spacer 10mm",
            "Tripod adapter",
            "Fan duct",
            "Wall hook",
        ];
        // Minutes before `now`, newest first, spread over a few weeks.
        const AGES_MIN: [i64; 16] = [
            7, 95, 1_310, 1_385, 2_900, 4_420, 7_300, 10_150, 13_020, 17_280, 21_640, 26_000,
            30_200, 34_570, 38_900, 43_300,
        ];
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            let id = DocumentId::from_u128(0xcad0_0000_0000_0000_0000_0000_0000_0000 + i as u128 + 1);
            let name = if i < NAMES.len() {
                NAMES[i].to_string()
            } else {
                format!("Sample {}", i + 1)
            };
            let mut doc = Document::new(name);
            doc.id = id;
            let age = AGES_MIN.get(i).copied().unwrap_or(43_300 + 1_440 * i as i64);
            let modified = now - age * 60;
            let mut meta = DocumentMeta::new(user, modified - 3_600);
            meta.modified = modified;
            self.create(&doc, &meta)?;
        }
        Ok(())
    }
}

/// A stable seed for a placeholder thumbnail (FNV-1a of the name and creation time), so the
/// same document always gets the same picture.
pub fn thumbnail_seed(name: &str, created: Timestamp) -> u64 {
    name.bytes()
        .chain(created.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

/// Readers for older `document.ron` versions, converting to the current model.
mod migrate {
    use cadrs_sketch::{Curve, CurveId, FaceName, FaceOrigin, PlaneRef, Point, PointId, Sketch};
    use serde::Deserialize;
    use slotmap::SlotMap;

    use super::{DocumentFile, SCHEMA_VERSION, StoreError};
    use crate::document as cur;
    use crate::ids::{DocumentId, ElementId, FeatureId};
    use crate::library::DocumentMeta;

    #[derive(Deserialize)]
    #[serde(rename = "DocumentFile")]
    struct FileV1 {
        meta: DocumentMeta,
        document: DocumentV1,
    }

    #[derive(Deserialize)]
    #[serde(rename = "Document")]
    struct DocumentV1 {
        id: DocumentId,
        name: String,
        elements: Vec<ElementV1>,
    }

    #[derive(Deserialize)]
    #[serde(rename = "Element")]
    struct ElementV1 {
        id: ElementId,
        name: String,
        kind: ElementKindV1,
    }

    #[derive(Deserialize)]
    #[serde(rename = "ElementKind")]
    enum ElementKindV1 {
        PartStudio { features: Vec<FeatureV1> },
        Assembly,
    }

    #[derive(Deserialize)]
    #[serde(rename = "Feature")]
    struct FeatureV1 {
        id: FeatureId,
        name: String,
        kind: FeatureKindV1,
    }

    #[derive(Deserialize)]
    #[serde(rename = "FeatureKind")]
    enum FeatureKindV1 {
        Sketch(SketchV1),
    }

    #[derive(Deserialize)]
    #[serde(rename = "Sketch")]
    struct SketchV1 {
        plane: PlaneRef,
        points: SlotMap<PointId, Point>,
        curves: SlotMap<CurveId, Curve>,
    }

    /// Version 3 (M9–P3.1): faces and edges named by `FaceTag` and `EdgeTag`. They read into
    /// the current names with the operation left out and the region as its index in the
    /// extrude's list (`cadrs_sketch::legacy`). Filled in here: the operation is the feature
    /// the reference names (the extrude that made the part), and the region its key.
    pub(super) fn from_v3(text: &str) -> Result<DocumentFile, StoreError> {
        let mut file: DocumentFile = cadrs_sketch::legacy::read_v3(|| ron::from_str(text))
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        for el in &mut file.document.elements {
            let Some(features) = el.features_mut() else {
                continue;
            };
            // Each extrude's region keys, by index.
            let keys: Vec<(uuid::Uuid, Vec<u64>)> = features
                .iter()
                .filter_map(|f| Some((f.id.0, f.extrude()?.regions.iter().map(|r| r.key()).collect())))
                .collect();
            let fix = |name: FaceName, op: uuid::Uuid| -> FaceName {
                if !name.op.is_nil() {
                    return name;
                }
                let key = |i: u64| {
                    keys.iter()
                        .find(|(id, _)| *id == op)
                        .and_then(|(_, k)| k.get(i as usize).copied())
                        // A region the extrude no longer has: a name nothing will match.
                        .unwrap_or(u64::MAX - i)
                };
                let origin = match name.origin {
                    FaceOrigin::Cap { region, end } => FaceOrigin::Cap { region: key(region), end },
                    FaceOrigin::Side { region, curve } => FaceOrigin::Side { region: key(region), curve },
                    o => o,
                };
                FaceName { op, origin, ..name }
            };
            for f in features.iter_mut() {
                let Some(sk) = f.sketch_mut() else { continue };
                if let Some(PlaneRef::Face(fp)) = &mut sk.plane {
                    fp.face = fix(fp.face, fp.feature);
                }
                for (k, _, link) in sk.geometry.links() {
                    sk.geometry.set_link(k, link.map_faces(fix));
                }
            }
        }
        file.version = SCHEMA_VERSION;
        Ok(file)
    }

    /// Version 4 (P3.2–P3F): the same model without external references (P3G.1), so it reads
    /// with the current types; only the version changes.
    pub(super) fn from_v4(text: &str) -> Result<DocumentFile, StoreError> {
        let mut file: DocumentFile =
            ron::from_str(text).map_err(|e| StoreError::Parse(e.to_string()))?;
        file.version = SCHEMA_VERSION;
        Ok(file)
    }

    /// Version 2 (M3–M8): the same model without extrudes and face planes, so it reads with
    /// the current types; only the version changes.
    pub(super) fn from_v2(text: &str) -> Result<DocumentFile, StoreError> {
        let mut file: DocumentFile =
            ron::from_str(text).map_err(|e| StoreError::Parse(e.to_string()))?;
        file.version = SCHEMA_VERSION;
        Ok(file)
    }

    /// Version 1: sketches carried their plane inside the geometry and always had one.
    pub(super) fn from_v1(text: &str) -> Result<DocumentFile, StoreError> {
        let old: FileV1 = ron::from_str(text).map_err(|e| StoreError::Parse(e.to_string()))?;
        let elements = old
            .document
            .elements
            .into_iter()
            .map(|e| cur::Element {
                id: e.id,
                name: e.name,
                assembly: Default::default(),
                contexts: Vec::new(),
                open_context: None,
                simulation: Default::default(),
                named_views: Vec::new(),
                kind: match e.kind {
                    ElementKindV1::PartStudio { features } => cur::ElementKind::PartStudio {
                        features: features
                            .into_iter()
                            .map(|f| cur::Feature {
                                id: f.id,
                                name: f.name,
                                kind: match f.kind {
                                    FeatureKindV1::Sketch(s) => {
                                        cur::FeatureKind::Sketch(cur::SketchFeature {
                                            plane: Some(s.plane),
                                            disable_imprinting: false,
                                            geometry: Sketch {
                                                points: s.points,
                                                curves: s.curves,
                                                ..Sketch::default()
                                            },
                                        })
                                    }
                                },
                                suppress_by: None,
                            })
                            .collect(),
                        parts: Vec::new(),
                        sketch_visibility: Vec::new(),
                        appearances: Vec::new(),
                        curve_appearances: Vec::new(),
                        folders: Vec::new(),
                        suppressed: Vec::new(),
                        rollback: None,
                    },
                    ElementKindV1::Assembly => cur::ElementKind::Assembly,
                },
            })
            .collect();
        Ok(DocumentFile {
            version: SCHEMA_VERSION,
            meta: old.meta,
            document: cur::Document {
                id: old.document.id,
                name: old.document.name,
                elements,
                units: Default::default(),
                custom_colors: Vec::new(),
                material_libraries: Vec::new(),
                standard_content: Vec::new(),
                properties: Default::default(),
                linked: Vec::new(),
                moved: Vec::new(),
                tab_tree: Default::default(),
            },
        })
    }
}

/// The listing entry cached next to `doc_path`, if it is still that file's.
fn read_entry(doc_path: &Path) -> Option<DocumentEntry> {
    let text = std::fs::read_to_string(doc_path.with_file_name(ENTRY_FILE)).ok()?;
    let f: EntryFile = ron::from_str(&text).ok()?;
    (file_stamp(doc_path)? == (f.doc_len, f.doc_modified_ns)).then_some(DocumentEntry {
        id: f.id,
        name: f.name,
        meta: f.meta,
    })
}

/// Caches `entry` next to `doc_path`, stamped with that file's size and modified time.
fn write_entry(doc_path: &Path, entry: &DocumentEntry) -> Result<(), StoreError> {
    let Some((doc_len, doc_modified_ns)) = file_stamp(doc_path) else {
        return Ok(());
    };
    let file = EntryFile {
        doc_len,
        doc_modified_ns,
        id: entry.id,
        name: entry.name.clone(),
        meta: entry.meta.clone(),
    };
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .map_err(|e| StoreError::Parse(e.to_string()))?;
    write_atomic(&doc_path.with_file_name(ENTRY_FILE), text.as_bytes())?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> Store {
        let dir = std::env::temp_dir().join(format!(
            "cadrs-store-test-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        Store::new(dir)
    }

    #[test]
    fn save_load_round_trip() {
        let store = temp_store("rt");
        let doc = Document::new("Bracket");
        let meta = DocumentMeta::new("me", 1_000);
        store.create(&doc, &meta).unwrap();
        let file = store.load(doc.id).unwrap();
        assert_eq!(file.version, SCHEMA_VERSION);
        assert_eq!(file.document, doc);
        assert_eq!(file.meta, meta);
        let text = std::fs::read_to_string(store.document_path(doc.id)).unwrap();
        assert!(text.contains("version: 5"), "{text}");
        assert!(store.read_thumbnail(doc.id).is_some());
        let (lib, errors) = store.list();
        assert!(errors.is_empty());
        assert_eq!(lib.entries.len(), 1);
        assert_eq!(lib.entries[0].name, "Bracket");
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn copies_are_undoable_library_steps() {
        use crate::library::{AddEntry, LibraryHistory};
        let store = temp_store("copy");
        let doc = Document::new("Bracket");
        store.create(&doc, &DocumentMeta::new("me", 1_000)).unwrap();
        let (mut lib, _) = store.list();
        let mut history = LibraryHistory::default();
        let id = DocumentId::new();
        let entry = store
            .copy_document(doc.id, id, "Copy of Bracket", "me", 2_000)
            .unwrap();
        assert_eq!(entry.meta.created, 2_000);
        let copy = store.load(id).unwrap();
        assert_eq!(copy.document.name, "Copy of Bracket");
        assert_eq!(copy.document.elements, doc.elements);
        assert!(store.read_thumbnail(id).is_some());
        let before = lib.clone();
        history.execute(&mut lib, &AddEntry { entry }).unwrap();
        store.sync(&before, &lib).unwrap();
        assert_eq!(store.list().0.entries.len(), 2);
        // Undo sets the copy aside; redo brings it back.
        let after = lib.clone();
        history.undo(&mut lib).unwrap();
        store.sync(&after, &lib).unwrap();
        assert_eq!(store.list().0.entries.len(), 1);
        history.redo(&mut lib).unwrap();
        store.sync(&before, &lib).unwrap();
        assert_eq!(store.list().0.entries.len(), 2);
        assert_eq!(store.load(id).unwrap().document.name, "Copy of Bracket");
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn sketch_features_round_trip() {
        use crate::commands::{AddSketch, AddSketchLine, SetSketchImprinting};
        use crate::ids::FeatureId;
        use cadrs_sketch::{PlaneRef, Vec2};
        let store = temp_store("sketch");
        let mut doc = Document::new("Sketchy");
        let ps = doc.elements[0].id;
        let mut h = crate::History::default();
        let (a, b) = (FeatureId::new(), FeatureId::new());
        h.execute(&mut doc, &AddSketch { element: ps, feature: a, plane: Some(PlaneRef::Front) })
            .unwrap();
        h.execute(
            &mut doc,
            &AddSketchLine { element: ps, feature: a, a: Vec2::ZERO, b: Vec2::new(5.0, 2.0) },
        )
        .unwrap();
        // An accepted sketch without a plane (invalid) persists too.
        h.execute(&mut doc, &AddSketch { element: ps, feature: b, plane: None })
            .unwrap();
        h.execute(
            &mut doc,
            &SetSketchImprinting { element: ps, feature: b, disable_imprinting: true },
        )
        .unwrap();
        let meta = DocumentMeta::new("me", 1_000);
        store.save(&doc, &meta).unwrap();
        let file = store.load(doc.id).unwrap();
        assert_eq!(file.document, doc);
        let features = file.document.elements[0].features();
        assert_eq!(features[1].name, "Sketch 2");
        assert!(features[1].sketch().unwrap().disable_imprinting);
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    /// A version 1 file (sketches with the plane inside the geometry) loads as version 2.
    #[test]
    fn version_1_files_migrate() {
        use crate::ids::{ElementId, FeatureId};
        use cadrs_sketch::{PlaneRef, Sketch, Vec2};
        let store = temp_store("v1");
        let doc_id = DocumentId::from_u128(42);
        let (ps, asm, f) = (ElementId::from_u128(1), ElementId::from_u128(2), FeatureId::from_u128(3));
        let mut geometry = Sketch::new();
        geometry.add_line(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0));
        let inner = ron::to_string(&geometry).unwrap();
        let inner = inner.trim_start_matches('(').trim_end_matches(')');
        let meta = DocumentMeta::new("me", 77);
        let text = format!(
            "(version: 1, meta: {meta}, document: (id: {doc_id}, name: \"Old\", elements: [\
             (id: {ps}, name: \"Part Studio 1\", kind: PartStudio(features: [\
             (id: {f}, name: \"Sketch 1\", kind: Sketch((plane: Right, {inner})))])),\
             (id: {asm}, name: \"Assembly 1\", kind: Assembly)]))",
            meta = ron::to_string(&meta).unwrap(),
            doc_id = ron::to_string(&doc_id).unwrap(),
            ps = ron::to_string(&ps).unwrap(),
            asm = ron::to_string(&asm).unwrap(),
            f = ron::to_string(&f).unwrap(),
        );
        std::fs::create_dir_all(store.doc_dir(doc_id)).unwrap();
        std::fs::write(store.document_path(doc_id), text).unwrap();
        let file = store.load(doc_id).unwrap();
        assert_eq!(file.version, SCHEMA_VERSION);
        assert_eq!(file.meta, meta);
        assert_eq!(file.document.name, "Old");
        assert_eq!(file.document.elements.len(), 2);
        let feature = &file.document.elements[0].features()[0];
        assert_eq!(feature.name, "Sketch 1");
        let s = feature.sketch().unwrap();
        assert_eq!(s.plane, Some(PlaneRef::Right));
        assert!(!s.disable_imprinting);
        assert_eq!(s.geometry, geometry);
        // Saving writes the new version, which loads back unchanged.
        store.save(&file.document, &file.meta).unwrap();
        assert_eq!(store.load(doc_id).unwrap(), file);
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    /// A version 2 file (M3–M8: sketches only) loads and saves as the current version.
    #[test]
    fn version_2_files_migrate() {
        use crate::commands::{AddSketch, AddSketchLine};
        use crate::ids::FeatureId;
        use cadrs_sketch::{PlaneRef, Vec2};
        let store = temp_store("v2");
        let mut doc = Document::new("Old sketch");
        let ps = doc.elements[0].id;
        let mut h = crate::History::default();
        let f = FeatureId::new();
        h.execute(&mut doc, &AddSketch { element: ps, feature: f, plane: Some(PlaneRef::Top) })
            .unwrap();
        h.execute(
            &mut doc,
            &AddSketchLine { element: ps, feature: f, a: Vec2::ZERO, b: Vec2::new(5.0, 2.0) },
        )
        .unwrap();
        let meta = DocumentMeta::new("me", 1_000);
        store.save(&doc, &meta).unwrap();
        // Rewrite it as an M8 build would have: the same text with version 2.
        let path = store.document_path(doc.id);
        let text = std::fs::read_to_string(&path).unwrap().replace("version: 5", "version: 2");
        assert!(!text.contains("Extrude") && !text.contains("Face("));
        std::fs::write(&path, text).unwrap();
        let file = store.load(doc.id).unwrap();
        assert_eq!(file.version, SCHEMA_VERSION);
        assert_eq!(file.document, doc);
        store.save(&file.document, &file.meta).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("version: 5"));
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    /// Extrudes and sketches on faces round-trip through `document.ron`.
    #[test]
    fn extrudes_round_trip() {
        use crate::commands::{AddExtrude, AddSketch, EditSketch};
        use crate::document::{ExtrudeFeature, RegionRef};
        use crate::ids::FeatureId;
        use cadrs_sketch::{PlaneRef, SketchOp, Vec2};
        let store = temp_store("extrude");
        let mut doc = Document::new("Block");
        let ps = doc.elements[0].id;
        let mut h = crate::History::default();
        let (sk, ex, sk2) = (FeatureId::new(), FeatureId::new(), FeatureId::new());
        h.execute(&mut doc, &AddSketch { element: ps, feature: sk, plane: Some(PlaneRef::Top) })
            .unwrap();
        h.execute(
            &mut doc,
            &EditSketch {
                element: ps,
                feature: sk,
                op: SketchOp::AddPolyline {
                    points: vec![
                        Vec2::ZERO,
                        Vec2::new(50.0, 0.0),
                        Vec2::new(50.0, 30.0),
                        Vec2::new(0.0, 30.0),
                    ],
                    closed: true,
                    construction: false,
                    label: "Add rectangle",
                },
            },
        )
        .unwrap();
        let geometry = doc.elements[0].feature(sk).unwrap().sketch().unwrap().geometry.clone();
        let region = cadrs_sketch::region::regions(&geometry).remove(0);
        let extrude = ExtrudeFeature {
            regions: vec![RegionRef::new(sk, &region)],
            depth: 20.0,
            depth_expr: "20 mm".into(),
            flip: true,
            ..ExtrudeFeature::default()
        };
        h.execute(&mut doc, &AddExtrude { element: ps, feature: ex, extrude }).unwrap();
        let features = doc.elements[0].features().to_vec();
        let face = crate::parts::face_plane(&features, ex, crate::parts::cap_name(&features, ex, 0, true).unwrap()).unwrap();
        h.execute(&mut doc, &AddSketch { element: ps, feature: sk2, plane: Some(face) })
            .unwrap();
        let meta = DocumentMeta::new("me", 1_000);
        store.save(&doc, &meta).unwrap();
        let text = std::fs::read_to_string(store.document_path(doc.id)).unwrap();
        assert!(text.contains("Extrude(") && text.contains("Face("), "{text}");
        let file = store.load(doc.id).unwrap();
        assert_eq!(file.document, doc);
        let parts = crate::parts::parts(file.document.elements[0].features());
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "Part 1");
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn newer_schema_is_rejected() {
        let store = temp_store("ver");
        let doc = Document::new("x");
        store.save(&doc, &DocumentMeta::new("me", 1)).unwrap();
        let path = store.document_path(doc.id);
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("version: 5", "version: 99");
        std::fs::write(&path, text).unwrap();
        assert!(matches!(
            store.load(doc.id),
            Err(StoreError::UnsupportedVersion(99))
        ));
        let (lib, errors) = store.list();
        assert!(lib.entries.is_empty());
        assert_eq!(errors.len(), 1);
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn sync_writes_changed_entries() {
        let store = temp_store("sync");
        store.seed_samples(3, "me", 1_000_000).unwrap();
        let (before, _) = store.list();
        assert_eq!(before.entries.len(), 3);
        let mut after = before.clone();
        after.entries[1].name = "Renamed".into();
        after.entries[2].meta.trashed = Some(5);
        store.sync(&before, &after).unwrap();
        let (reloaded, _) = store.list();
        assert_eq!(reloaded, after);
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn listing_uses_entry_files_while_current() {
        let store = temp_store("entry");
        store.seed_samples(2, "me", 1_000_000).unwrap();
        let (lib, _) = store.list();
        let id = lib.entries[0].id;
        assert!(store.doc_dir(id).join(ENTRY_FILE).is_file());
        // A document rewritten behind the store's back is read in full again.
        let path = store.document_path(id);
        let name = &lib.entries[0].name;
        let text = std::fs::read_to_string(&path).unwrap();
        let edited = text.replacen(&format!("name: {name:?}"), "name: \"Edited elsewhere\"", 1);
        std::fs::write(&path, edited).unwrap();
        let (relisted, _) = store.list();
        assert_eq!(relisted.entries[0].name, "Edited elsewhere");
        assert_eq!(relisted.entries[1], lib.entries[1]);
        // Without entry files, listing reads every document and writes them again.
        std::fs::remove_file(store.doc_dir(id).join(ENTRY_FILE)).unwrap();
        assert_eq!(store.list().0, relisted);
        assert!(store.doc_dir(id).join(ENTRY_FILE).is_file());
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn purge_moves_aside_and_undo_restores() {
        use crate::ids::FolderId;
        use crate::library::{CreateFolder, LibraryCommand, LibraryHistory, PurgeEntry, TrashEntry};
        let store = temp_store("purge");
        store.seed_samples(2, "me", 1_000_000).unwrap();
        let (mut lib, _) = store.list();
        let id = lib.entries[0].id;
        let mut h = LibraryHistory::default();
        let step = |lib: &mut Library, h: &mut LibraryHistory, cmd: &dyn LibraryCommand| {
            let before = lib.clone();
            h.execute(lib, cmd).unwrap();
            store.sync(&before, lib).unwrap();
        };
        step(&mut lib, &mut h, &TrashEntry { id, now: 5 });
        step(&mut lib, &mut h, &PurgeEntry { id });
        step(
            &mut lib,
            &mut h,
            &CreateFolder {
                id: FolderId::from_u128(7),
                name: "Jigs".into(),
                user: "me".into(),
                now: 6,
            },
        );
        let (on_disk, _) = store.list();
        assert_eq!(on_disk, lib);
        assert_eq!(on_disk.entries.len(), 1);
        assert_eq!(on_disk.folders[0].name, "Jigs");
        // Undo the folder and the purge.
        let before = lib.clone();
        h.undo(&mut lib);
        h.undo(&mut lib);
        store.sync(&before, &lib).unwrap();
        let (on_disk, _) = store.list();
        assert_eq!(on_disk, lib);
        assert_eq!(on_disk.entries.len(), 2);
        store.flush_deleted().unwrap();
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn seeding_is_deterministic() {
        let a = temp_store("seed-a");
        let b = temp_store("seed-b");
        a.seed_samples(12, "me", 1_790_000_000).unwrap();
        b.seed_samples(12, "me", 1_790_000_000).unwrap();
        let (la, _) = a.list();
        let (lb, _) = b.list();
        assert_eq!(la, lb);
        assert_eq!(la.entries.len(), 12);
        let id = la.entries[0].id;
        assert_eq!(
            std::fs::read(a.thumbnail_path(id)).unwrap(),
            std::fs::read(b.thumbnail_path(id)).unwrap()
        );
        std::fs::remove_dir_all(a.root()).unwrap();
        std::fs::remove_dir_all(b.root()).unwrap();
    }

    #[test]
    fn labels_and_descriptions_survive_a_reload() {
        use crate::ids::LabelId;
        use crate::library::{CreateLabel, DeleteLabel, LibraryCommand, LibraryHistory, SetDescription, SetLabels};
        let store = temp_store("labels");
        store.seed_samples(5, "me", 1_000_000).unwrap();
        let (mut lib, _) = store.list();
        let mut h = LibraryHistory::default();
        let step = |lib: &mut Library, h: &mut LibraryHistory, cmd: &dyn LibraryCommand| {
            let before = lib.clone();
            h.execute(lib, cmd).unwrap();
            store.sync(&before, lib).unwrap();
        };
        let (fixtures, hardware) = (LabelId::from_u128(1), LabelId::from_u128(2));
        let ids: Vec<DocumentId> = lib.entries.iter().map(|e| e.id).collect();
        step(&mut lib, &mut h, &CreateLabel { id: fixtures, name: "Fixtures".into(), colour: [1, 2, 3], assign: vec![ids[0]] });
        step(&mut lib, &mut h, &CreateLabel { id: hardware, name: "Hardware".into(), colour: [4, 5, 6], assign: vec![] });
        step(&mut lib, &mut h, &SetLabels { id: ids[2], labels: vec![hardware, fixtures] });
        step(&mut lib, &mut h, &SetDescription { id: ids[1], description: "Extended hubcap".into() });
        let (reloaded, errors) = store.list();
        assert!(errors.is_empty());
        assert_eq!(reloaded, lib);
        assert_eq!(reloaded.get(ids[2]).unwrap().meta.labels, [hardware, fixtures]);
        assert_eq!(reloaded.get(ids[1]).unwrap().meta.description, "Extended hubcap");
        // Deleting a label, then undoing it, reloads the same way.
        step(&mut lib, &mut h, &DeleteLabel { id: fixtures });
        assert_eq!(store.list().0, lib);
        assert!(store.list().0.get(ids[0]).unwrap().meta.labels.is_empty());
        let before = lib.clone();
        h.undo(&mut lib);
        store.sync(&before, &lib).unwrap();
        let (reloaded, _) = store.list();
        assert_eq!(reloaded.get(ids[0]).unwrap().meta.labels, [fixtures]);
        assert_eq!(reloaded.labels.len(), 2);
        std::fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn a_sample_copy_is_a_new_editable_document() {
        use crate::library::{AddEntry, LibraryHistory};
        let store = temp_store("sample");
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/linked_block_standin.cadrs"));
        let source = Store::load_path(path).unwrap();
        let id = DocumentId::from_u128(77);
        let entry = store.copy_from_file(path, id, "Block source copy", "me", 5_000).unwrap();
        assert_eq!(entry.meta.owned_by, "me");
        let mut lib = Library::default();
        let mut h = LibraryHistory::default();
        h.execute(&mut lib, &AddEntry { entry }).unwrap();
        store.sync(&Library::default(), &lib).unwrap();
        let copy = store.load(id).unwrap();
        assert_eq!(copy.document.name, "Block source copy");
        assert_eq!(copy.document.elements, source.document.elements);
        assert_ne!(copy.document.id, source.document.id);
        std::fs::remove_dir_all(store.root()).unwrap();
    }
}
