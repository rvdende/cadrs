//! cadrs_kicad: KiCad projects → cadrs_eda.
//!
//! A deliberate crutch: it only converts. Nothing else in cadrs depends on it except the app's
//! `kicad` feature, so deleting it leaves the rest building.
//!
//! - [`read_project`]: a `.kicad_pro` (or its folder, `.kicad_sch` or `.kicad_pcb`) into a
//!   [`cadrs_eda::Design`], with net classes and rules from the project file.
//! - [`resolve_model`]: where a footprint's 3D model file is on this machine.

pub mod board;
mod common;
pub mod schematic;
pub mod sexpr;

use cadrs_eda::Design;
use cadrs_eda::board::NetClass;
use cadrs_eda::units::mm;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    Io(PathBuf, std::io::Error),
    Parse(sexpr::ParseError),
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Error::Parse(e) => write!(f, "{e}"),
            Error::Format(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<sexpr::ParseError> for Error {
    fn from(e: sexpr::ParseError) -> Self {
        Error::Parse(e)
    }
}

pub use board::read_board;
pub use schematic::read_schematic;

/// A KiCad project read into cadrs.
#[derive(Debug)]
pub struct Project {
    /// The project's name (its file stem).
    pub name: String,
    pub dir: PathBuf,
    pub design: Design,
    /// What could not be imported, for the user.
    pub warnings: Vec<String>,
}

/// Reads a project: `path` is a `.kicad_pro`, `.kicad_sch` or `.kicad_pcb` file, or a folder
/// holding one project. The schematic and board beside it are read when present.
pub fn read_project(path: &Path) -> Result<Project, Error> {
    let io = |p: &Path, e| Error::Io(p.to_path_buf(), e);
    let (dir, stem) = if path.is_dir() {
        let pro = std::fs::read_dir(path)
            .map_err(|e| io(path, e))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "kicad_pro" || x == "kicad_pcb" || x == "kicad_sch"))
            .ok_or_else(|| Error::Format(format!("no KiCad project in {}", path.display())))?;
        (path.to_path_buf(), pro.file_stem().unwrap_or_default().to_string_lossy().into_owned())
    } else {
        (path.parent().unwrap_or(Path::new(".")).to_path_buf(), path.file_stem().unwrap_or_default().to_string_lossy().into_owned())
    };
    let file = |ext: &str| dir.join(format!("{stem}.{ext}"));
    let mut warnings = vec![];
    let mut design = Design::default();
    let sch = file("kicad_sch");
    if sch.exists() {
        let text = std::fs::read_to_string(&sch).map_err(|e| io(&sch, e))?;
        design.schematic = read_schematic(&text, &mut warnings)?;
    } else {
        warnings.push(format!("no schematic ({})", sch.display()));
    }
    let pcb = file("kicad_pcb");
    if pcb.exists() {
        let text = std::fs::read_to_string(&pcb).map_err(|e| io(&pcb, e))?;
        design.board = read_board(&text, &mut warnings)?;
    } else {
        warnings.push(format!("no board ({})", pcb.display()));
    }
    let pro = file("kicad_pro");
    if let Ok(text) = std::fs::read_to_string(&pro) {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => read_rules(&v, &mut design.board.rules),
            Err(e) => warnings.push(format!("{}: {e}", pro.display())),
        }
    }
    Ok(Project { name: stem, dir, design, warnings })
}

/// Net classes and board rules from the project file's JSON.
fn read_rules(v: &serde_json::Value, rules: &mut cadrs_eda::board::Rules) {
    let len = |x: &serde_json::Value| x.as_f64().map(mm);
    if let Some(classes) = v.pointer("/net_settings/classes").and_then(|c| c.as_array()) {
        let d = NetClass::default();
        rules.net_classes = classes
            .iter()
            .map(|c| NetClass {
                name: c["name"].as_str().unwrap_or("Default").into(),
                clearance: len(&c["clearance"]).unwrap_or(d.clearance),
                track_width: len(&c["track_width"]).unwrap_or(d.track_width),
                via_diameter: len(&c["via_diameter"]).unwrap_or(d.via_diameter),
                via_drill: len(&c["via_drill"]).unwrap_or(d.via_drill),
                diff_pair_width: len(&c["diff_pair_width"]).unwrap_or(d.diff_pair_width),
                diff_pair_gap: len(&c["diff_pair_gap"]).unwrap_or(d.diff_pair_gap),
                patterns: vec![],
            })
            .collect();
    }
    if let Some(pats) = v.pointer("/net_settings/netclass_patterns").and_then(|c| c.as_array()) {
        for p in pats {
            let (Some(class), Some(pat)) = (p["netclass"].as_str(), p["pattern"].as_str()) else { continue };
            if let Some(c) = rules.net_classes.iter_mut().find(|c| c.name == class) {
                c.patterns.push(pat.into());
            }
        }
    }
    if let Some(map) = v.pointer("/net_settings/netclass_assignments").and_then(|c| c.as_object()) {
        rules.net_class_of = map.iter().filter_map(|(n, c)| Some((n.clone(), c.as_str()?.to_string()))).collect();
    }
    if let Some(r) = v.pointer("/board/design_settings/rules") {
        let set = |key: &str, slot: &mut i64| {
            if let Some(x) = len(&r[key]) {
                *slot = x;
            }
        };
        set("min_clearance", &mut rules.min_clearance);
        set("min_track_width", &mut rules.min_track_width);
        set("min_via_diameter", &mut rules.min_via_diameter);
        set("min_through_hole_diameter", &mut rules.min_drill);
        set("min_via_annular_width", &mut rules.min_annular_ring);
        set("min_copper_edge_clearance", &mut rules.copper_edge_clearance);
        set("min_hole_clearance", &mut rules.hole_clearance);
    }
}

/// Where a footprint's 3D model `source` (as written in the board) is on this machine.
/// `${KIPRJMOD}` is the project folder; `${KICADn_3DMODEL_DIR}` the environment variable or
/// the installed libraries (`/usr/share/kicad*/3dmodels`). A `.wrl` with a `.step` beside it
/// resolves to the `.step`; a missing absolute path (another machine's) is looked for by file
/// name in the project's folders.
pub fn resolve_model(source: &str, project_dir: &Path) -> Option<PathBuf> {
    let mut path = source.to_string();
    while let Some(start) = path.find("${") {
        let end = start + path[start..].find('}')?;
        let var = &path[start + 2..end];
        let value = if var == "KIPRJMOD" {
            project_dir.to_string_lossy().into_owned()
        } else if let Ok(v) = std::env::var(var) {
            v
        } else if var.starts_with("KICAD") && var.ends_with("3DMODEL_DIR") {
            model_dirs().into_iter().next()?.to_string_lossy().into_owned()
        } else {
            return None;
        };
        path.replace_range(start..=end, &value);
    }
    let mut candidates = vec![PathBuf::from(&path)];
    // Other installed library versions.
    if let Some(rest) = source.split_once("3DMODEL_DIR}").map(|(_, r)| r.trim_start_matches('/')) {
        candidates.extend(model_dirs().into_iter().map(|d| d.join(rest)));
    }
    // By file name in the project.
    if let Some(name) = Path::new(&path).file_name() {
        candidates.extend(find_files(project_dir, name, 3));
    }
    let with_step = |p: &Path| -> Vec<PathBuf> {
        let mut v = vec![];
        for ext in ["step", "stp", "STEP", "STP"] {
            v.push(p.with_extension(ext));
        }
        v.push(p.to_path_buf());
        v
    };
    candidates.iter().flat_map(|c| with_step(c)).find(|p| p.is_file())
}

/// The installed 3D model libraries, newest first.
fn model_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir("/usr/share")
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("kicad")))
        .map(|p| p.join("3dmodels"))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.reverse();
    dirs
}

fn find_files(dir: &Path, name: &std::ffi::OsStr, depth: u32) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                out.extend(find_files(&p, name, depth - 1));
            }
        } else if p.file_stem() == Path::new(name).file_stem()
            && p.extension().is_some_and(|x| ["step", "stp", "wrl", "vrml"].contains(&x.to_string_lossy().to_lowercase().as_str()))
        {
            out.push(p);
        }
    }
    out
}
