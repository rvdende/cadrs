//! Portable, editable `.cadrs` ZIP packages. Library storage remains unchanged.
use crate::{
    Document, DocumentId, DocumentMeta, Store, blobs,
    store::{DocumentFile, SCHEMA_VERSION, StoreError},
};
use cadrs_fs::File;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
};

const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 16_384;
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
}

/// Writes current in-memory state, required source assets, and available thumbnails atomically.
pub fn export(
    path: &Path,
    doc: &Document,
    meta: &DocumentMeta,
    store: &Store,
) -> Result<(), String> {
    write_new(path, &pack(doc, meta, store, true)?)
}

/// The package of `doc` as [`export`] writes it: the bytes of a `.cadrs` file. `thumbnails`:
/// with the stored document's thumbnails (a package of some of its tabs goes without).
pub fn pack(
    doc: &Document,
    meta: &DocumentMeta,
    store: &Store,
    thumbnails: bool,
) -> Result<Vec<u8>, String> {
    let mut entries = BTreeMap::new();
    entries.insert(
        "manifest.ron".to_string(),
        ron::to_string(&Manifest {
            format: "cadrs".into(),
            version: 1,
        })
        .map_err(|e| e.to_string())?
        .into_bytes(),
    );
    let file = DocumentFile {
        version: SCHEMA_VERSION,
        meta: meta.clone(),
        document: doc.clone(),
    };
    entries.insert(
        "document.ron".into(),
        ron::ser::to_string_pretty(&file, Default::default())
            .map_err(|e| e.to_string())?
            .into_bytes(),
    );
    for (hash, ext) in blobs::used_by(doc) {
        let bytes = blobs::bytes_for(&hash, &store.blobs_dir(doc.id))?;
        entries.insert(format!("blobs/{}", blobs::file_name(&hash, &ext)), bytes);
    }
    for name in ["thumbnail.png", "thumbnail-large.png"]
        .into_iter()
        .filter(|_| thumbnails)
    {
        let p = store.doc_dir(doc.id).join(name);
        match cadrs_fs::read(p) {
            Ok(bytes) => {
                entries.insert(name.into(), bytes);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if entries.len() > MAX_ENTRIES
        || entries.values().map(|b| b.len() as u64).sum::<u64>() > MAX_BYTES
    {
        return Err("Document exceeds native package limits".into());
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Writes `bytes` to the new file `path`.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    // Written beside it and then renamed, so no one sees half of it; never over a file there.
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let written = File::create_new(&temp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| {
            if cadrs_fs::exists(path) {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "File exists",
                ))
            } else {
                cadrs_fs::rename(&temp, path)
            }
        });
    if written.is_err() {
        let _ = cadrs_fs::remove_file(&temp);
    }
    written.map_err(|e| e.to_string())
}

/// A `.cadrs` package read: its document, the assets it needs (file name and bytes) and its
/// thumbnails.
pub struct Package {
    pub file: DocumentFile,
    pub assets: Vec<(String, Vec<u8>)>,
    pub thumbnails: Vec<(&'static str, Vec<u8>)>,
}

/// Reads and validates the bytes of a `.cadrs` package ([`pack`]).
pub fn unpack(bytes: &[u8]) -> Result<Package, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Archive exceeds size limit".into());
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() > MAX_ENTRIES {
        return Err("Too many archive entries".into());
    }
    let mut entries = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if name.contains('\\')
            || name.split('/').any(|s| s == ".." || s == ".")
            || entry.enclosed_name().is_none()
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("Unsafe archive path".into());
        }
        if entry.is_dir() {
            continue;
        }
        let remaining = MAX_BYTES
            .checked_sub(total)
            .ok_or("Archive exceeds size limit")?;
        if entry.size() > remaining {
            return Err("Archive exceeds size limit".into());
        }
        let mut bytes = Vec::new();
        entry
            .take(remaining + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        total += bytes.len() as u64;
        if total > MAX_BYTES {
            return Err("Archive exceeds size limit".into());
        }
        if entries.insert(name, bytes).is_some() {
            return Err("Duplicate archive entry".into());
        }
    }
    let manifest: Manifest =
        ron::de::from_bytes(entries.get("manifest.ron").ok_or("Missing manifest.ron")?)
            .map_err(|e| e.to_string())?;
    if manifest.format == "cadrs" && manifest.version > 1 {
        return Err(crate::store::newer_reason());
    }
    if manifest.format != "cadrs" || manifest.version != 1 {
        return Err("Unsupported cadrs archive version or format".into());
    }
    let text = std::str::from_utf8(entries.get("document.ron").ok_or("Missing document.ron")?)
        .map_err(|e| e.to_string())?;
    let file = Store::parse_document(text).map_err(reason)?;
    let mut assets = Vec::new();
    for (hash, ext) in blobs::used_by(&file.document) {
        let name = blobs::file_name(&hash, &ext);
        let bytes = entries
            .get(&format!("blobs/{name}"))
            .ok_or_else(|| format!("Missing required asset {name}"))?;
        if blobs::hash_of(bytes) != hash {
            return Err(format!("Asset {name} failed validation"));
        }
        assets.push((name, bytes.clone()));
    }
    let thumbnails = ["thumbnail.png", "thumbnail-large.png"]
        .into_iter()
        .filter_map(|n| entries.remove(n).map(|b| (n, b)))
        .collect::<Vec<_>>();
    Ok(Package {
        file,
        assets,
        thumbnails,
    })
}

/// Reads the `.cadrs` file `path`: a package, or a legacy text document (read with the existing
/// schema migrations and adjacent assets).
pub fn read(path: &Path) -> Result<Package, String> {
    let mut source = File::open(path).map_err(|e| e.to_string())?;
    let mut magic = [0; 2];
    source.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic == b"PK" {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        return unpack(&bytes);
    }
    if source.metadata().map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err("Document exceeds size limit".into());
    }
    let file = Store::load_path(path).map_err(reason)?;
    let mut assets = Vec::new();
    for (hash, ext) in blobs::used_by(&file.document) {
        let bytes = blobs::get(&hash).ok_or_else(|| format!("Missing required asset {hash}"))?;
        if blobs::hash_of(&bytes) != hash {
            return Err(format!("Asset {hash} failed validation"));
        }
        assets.push((blobs::file_name(&hash, &ext), bytes.to_vec()));
    }
    Ok(Package {
        file,
        assets,
        thumbnails: Vec::new(),
    })
}

/// Validates a complete package, then installs a fresh library copy in one directory rename.
pub fn import(
    path: &Path,
    store: &Store,
    user: &str,
    now: crate::library::Timestamp,
) -> Result<DocumentFile, String> {
    let Package {
        mut file,
        assets,
        thumbnails,
    } = read(path)?;
    file.document.id = DocumentId::new();
    file.meta = DocumentMeta::new(user, now);
    file.meta.last_opened = Some(now);
    cadrs_fs::create_dir_all(store.root()).map_err(|e| e.to_string())?;
    let stage = Staging(
        store
            .root()
            .join(format!(".import-{}", uuid::Uuid::new_v4().simple())),
    );
    cadrs_fs::create_dir(&stage.0).map_err(|e| e.to_string())?;
    let staging = Store::new(&stage.0);
    // Persist source bytes before saving; Store::save may also find these in its cache.
    for (name, bytes) in &assets {
        let dir = staging.blobs_dir(file.document.id);
        cadrs_fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        cadrs_fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
    }
    staging
        .create(&file.document, &file.meta)
        .map_err(|e| e.to_string())?;
    for (name, bytes) in thumbnails {
        cadrs_fs::write(staging.doc_dir(file.document.id).join(name), bytes)
            .map_err(|e| e.to_string())?;
    }
    cadrs_fs::rename(
        staging.doc_dir(file.document.id),
        store.doc_dir(file.document.id),
    )
    .map_err(|e| e.to_string())?;
    for (_, bytes) in assets {
        blobs::insert(bytes);
    }
    Ok(file)
}

/// A folder in the store that an import is put together in, removed with what is left in it.
struct Staging(std::path::PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = cadrs_fs::remove_dir_all(&self.0);
    }
}

/// A document's read error as the import reports it.
fn reason(e: StoreError) -> String {
    if e.is_newer() {
        crate::store::newer_reason()
    } else {
        e.to_string()
    }
}
