//! Sending parts to a slicer installed on this machine (issue #176): finding PrusaSlicer,
//! OrcaSlicer, Bambu Studio, UltiMaker Cura or SuperSlicer, and opening a mesh in one. The mesh
//! goes to a folder of cadrs's own ([`send_path`]) and the slicer is started with it: nothing is
//! uploaded, no file association changes, no printer is spoken to. Not in a browser (a page
//! can't start programs; it downloads the mesh instead).

use std::path::{Path, PathBuf};

/// A slicer to send meshes to: its name, and how to start it with a file (`program args…
/// <file>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slicer {
    /// Which one ([`KNOWN`]'s index, or [`CUSTOM`]): what the preferences remember.
    pub id: u8,
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// The id of a program of one's own ([`custom`]).
pub const CUSTOM: u8 = u8::MAX;

/// A slicer cadrs knows: its name, its commands on the PATH (Linux, macOS), its Flatpak id, its
/// macOS app and its usual Windows executables (under Program Files).
struct Known {
    name: &'static str,
    commands: &'static [&'static str],
    flatpak: &'static str,
    mac_app: &'static str,
    windows: &'static [&'static str],
}

const KNOWN: &[Known] = &[
    Known {
        name: "PrusaSlicer",
        commands: &["prusa-slicer", "PrusaSlicer"],
        flatpak: "com.prusa3d.PrusaSlicer",
        mac_app: "PrusaSlicer.app",
        windows: &["Prusa3D\\PrusaSlicer\\prusa-slicer.exe"],
    },
    Known {
        name: "OrcaSlicer",
        commands: &["orca-slicer", "OrcaSlicer"],
        flatpak: "io.github.softfever.OrcaSlicer",
        mac_app: "OrcaSlicer.app",
        windows: &["OrcaSlicer\\orca-slicer.exe"],
    },
    Known {
        name: "Bambu Studio",
        commands: &["bambu-studio", "BambuStudio"],
        flatpak: "com.bambulab.BambuStudio",
        mac_app: "BambuStudio.app",
        windows: &["Bambu Studio\\bambu-studio.exe"],
    },
    Known {
        name: "UltiMaker Cura",
        commands: &["cura", "UltiMaker-Cura"],
        flatpak: "com.ultimaker.cura",
        mac_app: "UltiMaker Cura.app",
        windows: &["UltiMaker Cura\\UltiMaker-Cura.exe"],
    },
    Known {
        name: "SuperSlicer",
        commands: &["superslicer", "SuperSlicer"],
        flatpak: "com.superslicer.SuperSlicer",
        mac_app: "SuperSlicer.app",
        windows: &["SuperSlicer\\superslicer.exe"],
    },
];

/// The slicers installed here, in [`KNOWN`]'s order, each once (the first way found to start
/// it). `CADRS_SLICER=<program>` adds a program of one's own first.
pub fn find() -> Vec<Slicer> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("CADRS_SLICER").filter(|p| !p.is_empty()) {
        out.push(custom(PathBuf::from(p)));
    }
    let path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for (i, k) in KNOWN.iter().enumerate() {
        if let Some(s) = find_known(i as u8, k, &path) {
            out.push(s);
        }
    }
    out
}

fn find_known(id: u8, k: &Known, path: &[PathBuf]) -> Option<Slicer> {
    let slicer = |program: PathBuf, args: Vec<String>| Slicer {
        id,
        name: k.name.to_string(),
        program,
        args,
    };
    if cfg!(target_os = "windows") {
        let roots = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"];
        return roots
            .iter()
            .filter_map(std::env::var_os)
            .flat_map(|root| k.windows.iter().map(move |w| Path::new(&root).join(w)))
            .find(|p| p.is_file())
            .map(|p| slicer(p, Vec::new()));
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let app = [
            Some(PathBuf::from("/Applications")),
            home.map(|h| h.join("Applications")),
        ]
        .into_iter()
        .flatten()
        .map(|d| d.join(k.mac_app))
        .find(|p| p.is_dir());
        if let Some(app) = app {
            // `open -a <app> <file>`: the app opens the file (a new window or its running one).
            return Some(slicer(
                "open".into(),
                vec!["-a".into(), app.to_string_lossy().into_owned()],
            ));
        }
    }
    let on_path = k
        .commands
        .iter()
        .flat_map(|c| path.iter().map(move |d| d.join(c)))
        .find(|p| p.is_file());
    if let Some(p) = on_path {
        return Some(slicer(p, Vec::new()));
    }
    if cfg!(target_os = "linux") && flatpak_installed(k.flatpak) {
        return Some(slicer(
            "flatpak".into(),
            vec!["run".into(), k.flatpak.to_string()],
        ));
    }
    None
}

/// Whether a Flatpak app is installed, for the user or the system.
fn flatpak_installed(id: &str) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    [
        home.map(|h| h.join(".local/share/flatpak/app")),
        Some(PathBuf::from("/var/lib/flatpak/app")),
    ]
    .into_iter()
    .flatten()
    .any(|d| d.join(id).is_dir())
}

/// A program of one's own as a slicer (named after its file).
pub fn custom(program: PathBuf) -> Slicer {
    let name = program
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Slicer".into());
    Slicer {
        id: CUSTOM,
        name,
        program,
        args: Vec::new(),
    }
}

/// Where a sent mesh goes: cadrs's cache folder (`…/cadrs/slicer`), a new file each time
/// (`<name> <time>.stl`), so a version still open in the slicer is never overwritten. Files there
/// older than a day are removed first.
pub fn send_path(name: &str, now_secs: u64) -> std::io::Result<PathBuf> {
    let dir = directories::ProjectDirs::from("", "", "cadrs")
        .map(|d| d.cache_dir().join("slicer"))
        .unwrap_or_else(|| std::env::temp_dir().join("cadrs-slicer"));
    std::fs::create_dir_all(&dir)?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 24 * 3600);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let stem = crate::export::sanitize(name);
    let stem = if stem.is_empty() { "Part".into() } else { stem };
    Ok(crate::export::unique_path_ext(
        &dir,
        &format!("{stem} {now_secs}"),
        "stl",
    ))
}

impl Slicer {
    /// Starts the slicer with `file` (it isn't waited for; a thread reaps it when it exits).
    pub fn open(&self, file: &Path) -> std::io::Result<()> {
        let mut child = std::process::Command::new(&self.program)
            .args(&self.args)
            .arg(file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let _ = std::thread::Builder::new()
            .name("cadrs-slicer".into())
            .spawn(move || {
                let _ = child.wait();
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_slicer_is_named_after_its_file_and_gets_the_file() {
        let s = custom(PathBuf::from("/opt/slicers/MySlicer.AppImage"));
        assert_eq!(s.name, "MySlicer");
        assert!(s.args.is_empty());
        // `true` takes any arguments and exits: the launch itself works.
        if cfg!(unix) {
            custom(PathBuf::from("true"))
                .open(Path::new("part.stl"))
                .unwrap();
        }
    }

    #[test]
    fn each_send_gets_a_file_of_its_own() {
        let a = send_path("Bracket / v2", 1_700_000_000).unwrap();
        std::fs::write(&a, b"solid").unwrap();
        let b = send_path("Bracket / v2", 1_700_000_000).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.extension().unwrap(), "stl");
        assert!(!a.file_name().unwrap().to_string_lossy().contains('/'));
        let _ = std::fs::remove_file(&a);
    }
}
