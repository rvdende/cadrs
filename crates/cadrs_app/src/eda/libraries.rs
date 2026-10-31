//! The user's own libraries: folders of library folders (as `libraries/`) that parts are
//! added to without rebuilding cadrs, by the online part search ([`super::browser`]) or by
//! hand. Loaded when the app starts and again after each addition.
//!
//! The folder is `$CADRS_USER_LIBRARIES`, else `<data dir>/cadrs/libraries` (Linux:
//! `~/.local/share/cadrs/libraries`); scenarios and headless runs use `libraries/` in their
//! output folder, so they never read or write the user's. Model files the parts name are read
//! into the blob store, so a placed part's 3D model is saved with the document.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use cadrs_eda::library::{Library, LibraryTable, Scope};

/// The library parts downloaded from the catalogue go into.
pub const ONLINE_LIBRARY: &str = "LCSC";

/// The library a studio's components are added to (right-click a component → Add to my
/// library).
pub const MY_LIBRARY: &str = "My_parts";

#[derive(Resource, Debug, Default)]
pub struct UserLibraries {
    /// The folder of library folders (`None`: there's no data folder; nothing is kept).
    pub dir: Option<PathBuf>,
    pub libraries: Vec<Library>,
    /// Files that didn't read, from the last load.
    pub errors: Vec<String>,
}

impl UserLibraries {
    /// The user's folder (see the module docs).
    pub fn default_dir() -> Option<PathBuf> {
        std::env::var_os("CADRS_USER_LIBRARIES")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                directories::BaseDirs::new().map(|d| d.data_dir().join("cadrs").join("libraries"))
            })
    }

    pub fn load(dir: Option<PathBuf>) -> UserLibraries {
        let mut u = UserLibraries { dir, ..default() };
        u.reload();
        u
    }

    /// Reads the folder again.
    pub fn reload(&mut self) {
        let Some(dir) = &self.dir else { return };
        let (mut libs, errors) =
            cadrs_eda::library::load_libraries(std::slice::from_ref(dir), Scope::Global);
        for e in &errors {
            warn!("user libraries: {e}");
        }
        attach_model_files(&mut libs);
        self.libraries = libs;
        self.errors = errors;
    }

    /// A library's folder.
    pub fn library_dir(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(name))
    }

    /// Whether one of these libraries has the part already: a symbol of its symbol's name, or
    /// (a footprint-only part) a footprint of its footprint's.
    pub fn has_part(&self, c: &cadrs_eda::Component) -> bool {
        let fps = || self.libraries.iter().flat_map(|l| &l.footprints);
        match (&c.symbol, &c.footprint) {
            (Some(s), _) => self
                .libraries
                .iter()
                .flat_map(|l| &l.symbols)
                .any(|x| x.name() == s.name()),
            (None, Some(f)) => fps().any(|x| x.name() == f.name()),
            (None, None) => true,
        }
    }

    /// Writes a component into [`MY_LIBRARY`]: its symbol (naming its footprint there), its
    /// footprint, and the 3D model files it has, beside them. Returns the symbol's (else the
    /// footprint's) library id.
    pub fn add_part(&mut self, c: &cadrs_eda::Component) -> Result<String, String> {
        let dir = self
            .library_dir(MY_LIBRARY)
            .ok_or_else(|| cadrs_ui::i18n::tr("pcb-library-no-folder"))?;
        cadrs_eda::library::ensure_library(&dir, "Parts added from PCB Studios' components")?;
        let mut fp = c.footprint.clone();
        if let Some(f) = &mut fp {
            let stem = cadrs_eda::library::file_stem(f.name());
            for (i, m) in f.models.iter_mut().enumerate() {
                let Some(bytes) = m.blob.as_ref().and_then(|h| cadrs_core::blobs::get(h)) else {
                    continue;
                };
                let ext = cadrs_core::pcb::model::model_ext(m);
                let file = if i == 0 {
                    format!("{stem}.{ext}")
                } else {
                    format!("{stem}_{i}.{ext}")
                };
                let path = dir.join(file);
                cadrs_fs::write(&path, &bytes[..])
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                m.source = path.to_string_lossy().into_owned();
            }
            cadrs_eda::library::write_footprint(&dir, f)?;
        }
        let id = match (&c.symbol, &fp) {
            (Some(s), _) => {
                let mut s = s.clone();
                if let Some(f) = &fp {
                    cadrs_eda::lib_edit::set_symbol_field(
                        &mut s,
                        cadrs_eda::symbol::fields::FOOTPRINT,
                        &format!("{MY_LIBRARY}:{}", f.name()),
                    );
                }
                cadrs_eda::library::write_symbol(&dir, &s)?;
                format!("{MY_LIBRARY}:{}", s.name())
            }
            (None, Some(f)) => format!("{MY_LIBRARY}:{}", f.name()),
            (None, None) => {
                return Err(cadrs_ui::i18n::tr_with(
                    "pcb-library-part-empty",
                    &[("name", &c.name)],
                ));
            }
        };
        self.reload();
        Ok(id)
    }

    /// Adds these libraries' parts to a table.
    pub fn add_to(&self, t: &mut LibraryTable) {
        for l in &self.libraries {
            t.merge(l.clone());
        }
    }
}

/// Reads each footprint's model file (a path to one) into the blob store and points the
/// model at it.
pub fn attach_model_files(libs: &mut [Library]) {
    let mut read: std::collections::HashMap<String, Option<String>> = Default::default();
    for f in libs.iter_mut().flat_map(|l| l.footprints.iter_mut()) {
        for m in &mut f.models {
            if m.blob.is_some() || m.source.is_empty() || !Path::new(&m.source).is_absolute() {
                continue;
            }
            m.blob = read
                .entry(m.source.clone())
                .or_insert_with(|| {
                    cadrs_fs::read(&m.source)
                        .ok()
                        .map(cadrs_core::blobs::insert)
                })
                .clone();
        }
    }
}

pub fn register(app: &mut App) {
    if !app.world().contains_resource::<UserLibraries>() {
        app.insert_resource(UserLibraries::load(UserLibraries::default_dir()));
    }
}
