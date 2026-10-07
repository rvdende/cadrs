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
        std::env::var_os("CADRS_USER_LIBRARIES").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| directories::BaseDirs::new().map(|d| d.data_dir().join("cadrs").join("libraries")))
    }

    pub fn load(dir: Option<PathBuf>) -> UserLibraries {
        let mut u = UserLibraries { dir, ..default() };
        u.reload();
        u
    }

    /// Reads the folder again.
    pub fn reload(&mut self) {
        let Some(dir) = &self.dir else { return };
        let (mut libs, errors) = cadrs_eda::library::load_libraries(std::slice::from_ref(dir), Scope::Global);
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
            m.blob = read.entry(m.source.clone()).or_insert_with(|| std::fs::read(&m.source).ok().map(cadrs_core::blobs::insert)).clone();
        }
    }
}

pub fn register(app: &mut App) {
    if !app.world().contains_resource::<UserLibraries>() {
        app.insert_resource(UserLibraries::load(UserLibraries::default_dir()));
    }
}
