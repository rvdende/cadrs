//! KiCad project import (the `kicad` feature): Import ECAD files also takes a `.kicad_pro`,
//! `.kicad_pcb` or `.kicad_sch`, and the project (schematic, layout, rules) becomes a native
//! board of the studio. Everything KiCad-specific stays in `cadrs_kicad`; dropping the feature
//! removes this module and the extensions from the picker.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use cadrs_core::pcb::AddBoard;

use crate::ActiveDocument;

/// The file extensions Import ECAD files offers for KiCad.
pub const EXTENSIONS: [&str; 3] = ["kicad_pro", "kicad_pcb", "kicad_sch"];

/// Whether `p` is a KiCad project file.
pub fn is_kicad(p: &Path) -> bool {
    p.extension().is_some_and(|x| EXTENSIONS.iter().any(|e| x == *e))
}

/// One board per project among `paths` (a project picked as both `.kicad_pro` and
/// `.kicad_pcb` is imported once). Returns the imported names (with their component counts),
/// the warnings and the errors.
pub fn import(world: &mut World, paths: &[PathBuf]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut projects: Vec<PathBuf> = vec![];
    for p in paths.iter().filter(|p| is_kicad(p)) {
        let key = p.with_extension("");
        if !projects.iter().any(|q| q.with_extension("") == key) {
            projects.push(p.clone());
        }
    }
    let (mut names, mut warnings, mut errors) = (vec![], vec![], vec![]);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return (names, warnings, errors) };
    let Some(element) = doc.active_element().filter(|e| e.pcb().is_some()).map(|e| e.id) else {
        errors.push("Open a PCB Studio tab to import into".into());
        return (names, warnings, errors);
    };
    for path in projects {
        let project = match cadrs_kicad::read_project(&path) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let n = project.design.board.footprints.len();
        let mut project = project;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        warnings.extend(load_models(&mut project.design, &dir));
        let cmd = AddBoard {
            element,
            name: Some(project.name.clone()),
            design: Box::new(project.design),
            imported_from: Some(path.with_extension("kicad_pro").to_string_lossy().into_owned()),
        };
        match doc.execute(&cmd) {
            Ok(()) => {
                let name = doc.active_element().and_then(|e| e.pcb()).and_then(|s| s.active_board()).map(|b| b.name().to_string()).unwrap_or_default();
                names.push(format!("{name} ({n} components)"));
                warnings.extend(project.warnings);
            }
            Err(e) => errors.push(e.to_string()),
        }
    }
    (names, warnings, errors)
}

/// Reads the 3D model files the footprints name into the document (blobs), so the board shows
/// them: the VRML next to a STEP when there is one (it has colours), else what KiCad's paths
/// resolve to. A model file that isn't found gives way to the generated body of the built-in
/// footprint of the same name, when there is one. Returns a warning per model file not found.
fn load_models(design: &mut cadrs_eda::Design, project_dir: &Path) -> Vec<String> {
    let mut read: std::collections::HashMap<String, Option<(String, String)>> = Default::default();
    let mut missing = vec![];
    let builtin = cadrs_eda::library::LibraryTable::builtin();
    for f in &mut design.board.footprints {
        for m in &mut f.footprint.models {
            let got = read
                .entry(m.source.clone())
                .or_insert_with(|| {
                    let p = cadrs_kicad::resolve_model(&m.source, project_dir)?;
                    let wrl = p.with_extension("wrl");
                    let p = if wrl.is_file() { wrl } else { p };
                    let bytes = std::fs::read(&p).ok()?;
                    Some((cadrs_core::blobs::insert(bytes), p.to_string_lossy().into_owned()))
                })
                .clone();
            match got {
                Some((hash, path)) => {
                    m.blob = Some(hash);
                    m.source = path;
                }
                None => {
                    if m.body.is_none() {
                        m.body = builtin.footprint(&f.footprint.id).and_then(|b| b.models.first()).and_then(|b| b.body.clone());
                    }
                    if !missing.contains(&m.source) {
                        missing.push(m.source.clone());
                    }
                }
            }
        }
    }
    missing.into_iter().map(|s| format!("3D model not found: {s}")).collect()
}
