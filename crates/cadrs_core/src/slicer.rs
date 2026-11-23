//! Sending parts to a slicer installed on this machine (issue #176): finding PrusaSlicer,
//! OrcaSlicer, Bambu Studio, UltiMaker Cura, SuperSlicer or Creality Print, and opening the parts
//! in one, in the first of the formats it reads ([`Slicer::formats`]): STEP where it reads
//! that (its curves exact, so it meshes them as finely as it likes: no facets of ours on a
//! print), else a fine STL. The files go to a folder
//! of cadrs's own ([`send_dir`]) and the slicer is started with them: nothing is uploaded, no
//! file association changes, no printer is spoken to. Not in a browser (a page can't start
//! programs; it downloads the mesh instead).

use std::path::{Path, PathBuf};

use crate::rebuild::exchange::ModelFormat;

/// Binary STL: what every slicer reads.
const STL: ModelFormat = ModelFormat::Stl { binary: true };

/// Reads STEP, STL and OBJ (the formats cadrs writes that it imports), STEP best.
const STEP_STL_OBJ: &[ModelFormat] = &[ModelFormat::Step, STL, ModelFormat::Obj];

/// A slicer to send meshes to: its name, and how to start it with files (`program args…
/// <files> trailing…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slicer {
    /// Which one ([`KNOWN`]'s index, or [`CUSTOM`]): what the preferences remember.
    pub id: u8,
    pub name: String,
    pub program: PathBuf,
    /// Before the files.
    pub args: Vec<String>,
    /// After the files (Flatpak's `@@` that closes its forwarded files).
    pub trailing: Vec<String>,
    /// The formats cadrs writes that it imports, the one to send first.
    pub formats: Vec<ModelFormat>,
}

/// The id of a program of one's own ([`custom`]).
pub const CUSTOM: u8 = u8::MAX;

/// A slicer cadrs knows: its name, the formats it imports (best first), its commands on the PATH (Linux,
/// macOS), its Flatpak id, its macOS app and its usual Windows executables (under Program Files).
struct Known {
    name: &'static str,
    formats: &'static [ModelFormat],
    commands: &'static [&'static str],
    flatpak: &'static str,
    mac_app: &'static str,
    windows: &'static [&'static str],
}

const KNOWN: &[Known] = &[
    Known {
        name: "PrusaSlicer",
        formats: STEP_STL_OBJ,
        commands: &["prusa-slicer", "PrusaSlicer"],
        flatpak: "com.prusa3d.PrusaSlicer",
        mac_app: "PrusaSlicer.app",
        windows: &["Prusa3D\\PrusaSlicer\\prusa-slicer.exe"],
    },
    Known {
        name: "OrcaSlicer",
        formats: STEP_STL_OBJ,
        commands: &["orca-slicer", "OrcaSlicer"],
        flatpak: "io.github.softfever.OrcaSlicer",
        mac_app: "OrcaSlicer.app",
        windows: &["OrcaSlicer\\orca-slicer.exe"],
    },
    Known {
        name: "Bambu Studio",
        formats: STEP_STL_OBJ,
        commands: &["bambu-studio", "BambuStudio"],
        flatpak: "com.bambulab.BambuStudio",
        mac_app: "BambuStudio.app",
        windows: &["Bambu Studio\\bambu-studio.exe"],
    },
    Known {
        name: "UltiMaker Cura",
        // STEP only with a plugin.
        formats: &[STL, ModelFormat::Obj],
        commands: &["cura", "UltiMaker-Cura"],
        flatpak: "com.ultimaker.cura",
        mac_app: "UltiMaker Cura.app",
        windows: &[
            "UltiMaker Cura *\\UltiMaker-Cura.exe",
            "Ultimaker Cura *\\Ultimaker-Cura.exe",
        ],
    },
    Known {
        name: "SuperSlicer",
        formats: STEP_STL_OBJ,
        commands: &["superslicer", "SuperSlicer"],
        flatpak: "com.superslicer.SuperSlicer",
        mac_app: "SuperSlicer.app",
        windows: &["SuperSlicer\\superslicer.exe"],
    },
    Known {
        name: "Creality Print",
        formats: STEP_STL_OBJ,
        commands: &["CrealityPrint", "crealityprint"],
        flatpak: "io.github.crealityofficial.CrealityPrint",
        mac_app: "CrealityPrint.app",
        windows: &["Creality\\Creality Print *\\CrealityPrint.exe"],
    },
];

/// The slicers installed here, in [`KNOWN`]'s order, each once (the first way found to start
/// it: on Linux the PATH, Flatpak, then the desktop entries app launchers list, which is how an
/// AppImage kept anywhere is found). `CADRS_SLICER=<program>` adds a program of one's own first.
pub fn find() -> Vec<Slicer> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("CADRS_SLICER").filter(|p| !p.is_empty()) {
        out.push(custom(PathBuf::from(p)));
    }
    let path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let entries = if cfg!(target_os = "linux") {
        desktop_entries()
    } else {
        Vec::new()
    };
    for (i, k) in KNOWN.iter().enumerate() {
        if let Some(s) = find_known(i as u8, k, &path, &entries) {
            out.push(s);
        }
    }
    out
}

fn find_known(id: u8, k: &Known, path: &[PathBuf], entries: &[DesktopEntry]) -> Option<Slicer> {
    let slicer = |program: PathBuf, args: Vec<String>, trailing: Vec<String>| Slicer {
        id,
        name: k.name.to_string(),
        program,
        args,
        trailing,
        formats: k.formats.to_vec(),
    };
    if cfg!(target_os = "windows") {
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .chain(std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs")))
            .collect();
        return roots
            .iter()
            .flat_map(|root| k.windows.iter().flat_map(move |w| windows_paths(root, w)))
            .find(|p| p.is_file())
            .map(|p| slicer(p, Vec::new(), Vec::new()));
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
                Vec::new(),
            ));
        }
    }
    let on_path = k
        .commands
        .iter()
        .flat_map(|c| path.iter().map(move |d| d.join(c)))
        .find(|p| p.is_file());
    if let Some(p) = on_path {
        return Some(slicer(p, Vec::new(), Vec::new()));
    }
    if cfg!(target_os = "linux") && flatpak_installed(k.flatpak) {
        // `--file-forwarding … @@ files @@`: the sandboxed slicer may read the files.
        return Some(slicer(
            "flatpak".into(),
            vec![
                "run".into(),
                "--file-forwarding".into(),
                k.flatpak.to_string(),
                "@@".into(),
            ],
            vec!["@@".into()],
        ));
    }
    let entry = entries.iter().find(|e| {
        let name = squash(&e.name);
        name == squash(k.name) || k.commands.iter().any(|c| squash(c) == name)
    })?;
    let launch = entry.launch(path)?;
    Some(slicer(launch.0, launch.1, launch.2))
}

/// A name without case, spaces or punctuation ("Creality Print" and "CrealityPrint" are one).
fn squash(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// An application's desktop entry (a `.desktop` file, freedesktop.org's): its name and its
/// command line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopEntry {
    name: String,
    exec: String,
}

/// The applications' desktop entries, the user's first (`$XDG_DATA_HOME/applications`, then
/// each of `$XDG_DATA_DIRS`'s), one per file name as launchers do.
fn desktop_entries() -> Vec<DesktopEntry> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|d| !d.is_empty())
        .map(|d| std::env::split_paths(&d).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for dir in data_home.into_iter().chain(data_dirs) {
        let Ok(files) = std::fs::read_dir(dir.join("applications")) else {
            continue;
        };
        for f in files.flatten() {
            let file = f.file_name();
            if !file.to_string_lossy().ends_with(".desktop") || !seen.insert(file) {
                continue;
            }
            if let Some(e) = std::fs::read_to_string(f.path())
                .ok()
                .and_then(|text| parse_desktop_entry(&text))
            {
                out.push(e);
            }
        }
    }
    out
}

/// The `[Desktop Entry]` group's `Name` and `Exec` (none for a hidden entry, one taken away).
fn parse_desktop_entry(text: &str) -> Option<DesktopEntry> {
    let (mut name, mut exec, mut in_entry) = (None, None, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else {
            continue;
        };
        match key.trim() {
            "Name" => name = Some(value.trim().to_string()),
            "Exec" => exec = Some(value.trim().to_string()),
            "Hidden" if value.trim() == "true" => return None,
            _ => {}
        }
    }
    Some(DesktopEntry {
        name: name?,
        exec: exec?,
    })
}

impl DesktopEntry {
    /// How to start it with files: the program (found on `path` if not a path), the arguments
    /// before the files and those after (around its `%f`/`%F`/`%u`/`%U`; the other field codes
    /// go). None if the program isn't there.
    fn launch(&self, path: &[PathBuf]) -> Option<(PathBuf, Vec<String>, Vec<String>)> {
        let mut words = exec_words(&self.exec).into_iter();
        let program = PathBuf::from(words.next()?);
        let program = if program.components().count() > 1 {
            program.is_file().then_some(program)?
        } else {
            path.iter()
                .map(|d| d.join(&program))
                .find(|p| p.is_file())?
        };
        let (mut before, mut after, mut files_seen) = (Vec::new(), Vec::new(), false);
        for w in words {
            match w.as_str() {
                "%f" | "%F" | "%u" | "%U" => files_seen = true,
                w if w.len() == 2 && w.starts_with('%') => {}
                _ if files_seen => after.push(w.replace("%%", "%")),
                _ => before.push(w.replace("%%", "%")),
            }
        }
        Some((program, before, after))
    }
}

/// An `Exec` line's words: split at spaces outside double quotes, `\` escaping within them.
fn exec_words(exec: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted, mut in_word) = (Vec::new(), String::new(), false, false);
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            '\\' if quoted => word.extend(chars.next()),
            c if c.is_whitespace() && !quoted => {
                if in_word {
                    out.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        out.push(word);
    }
    out
}

/// The paths a Windows install path names under `root`: a `*` in a folder's name is any
/// version ("UltiMaker Cura *\\UltiMaker-Cura.exe" for "UltiMaker Cura 5.8.1").
fn windows_paths(root: &Path, path: &str) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    for part in path.split('\\') {
        out = out
            .into_iter()
            .flat_map(|dir| match part.split_once('*') {
                None => vec![dir.join(part)],
                Some((pre, post)) => std::fs::read_dir(&dir)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| n.starts_with(pre) && n.ends_with(post))
                            .map(|n| dir.join(n))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
    }
    out
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
        trailing: Vec::new(),
        // What it reads isn't known: a mesh.
        formats: vec![STL],
    }
}

/// cadrs's folder for sent parts (`…/cadrs/slicer` in the user's cache).
pub fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "cadrs")
        .map(|d| d.cache_dir().join("slicer"))
        .unwrap_or_else(|| std::env::temp_dir().join("cadrs-slicer"))
}

/// A new folder in `base` for one send (named after the time, `-2`… if taken), so a version still
/// open in the slicer is never overwritten and the files keep the parts' names (which the slicer
/// shows). Sends older than a day are removed first.
pub fn send_dir(base: &Path, now_secs: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(base)?;
    if let Ok(entries) = std::fs::read_dir(base) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 24 * 3600);
            if old {
                let _ = std::fs::remove_dir_all(e.path());
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    for n in 1u32.. {
        let dir = match n {
            1 => base.join(now_secs.to_string()),
            n => base.join(format!("{now_secs}-{n}")),
        };
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("a free folder name")
}

/// Writes `bytes` as `<name>.<extension>` in `dir` (a send's own folder, [`send_dir`]): a new
/// file.
pub fn write_file(
    dir: &Path,
    name: &str,
    extension: &str,
    bytes: &[u8],
) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let path = crate::export::unique_path_ext(dir, &crate::export::sanitize(name), extension);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?
        .write_all(bytes)?;
    Ok(path)
}

impl Slicer {
    /// The format to send it: the first it imports (binary STL if none is listed).
    pub fn send_format(&self) -> ModelFormat {
        self.formats.first().copied().unwrap_or(STL)
    }

    /// Starts the slicer with `files` (it isn't waited for: a thread reaps it, and logs a
    /// failed exit, as a launcher's (`open -a`, `flatpak run`) that couldn't start it).
    pub fn open(&self, files: &[PathBuf]) -> std::io::Result<()> {
        let mut child = std::process::Command::new(&self.program)
            .args(&self.args)
            .args(files)
            .args(&self.trailing)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let _ = std::thread::Builder::new()
            .name("cadrs-slicer".into())
            .spawn(move || {
                if let Ok(status) = child.wait()
                    && !status.success()
                {
                    eprintln!("cadrs: the slicer exited with {status}");
                }
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_slicer_is_named_after_its_file_and_gets_the_files() {
        let s = custom(PathBuf::from("/opt/slicers/MySlicer.AppImage"));
        assert_eq!((s.id, s.name.as_str()), (CUSTOM, "MySlicer"));
        // What a program of one's own reads isn't known: it gets a mesh.
        assert_eq!(s.send_format(), STL);
        // `true` takes any arguments and exits: the launch itself works.
        if cfg!(unix) {
            custom(PathBuf::from("true"))
                .open(&[PathBuf::from("part.stl")])
                .unwrap();
        }
    }

    #[test]
    fn each_send_gets_a_folder_of_its_own() {
        let base = std::env::temp_dir().join(format!("cadrs-slicer-test-{}", std::process::id()));
        let a = send_dir(&base, 1_700_000_000).unwrap();
        let b = send_dir(&base, 1_700_000_000).unwrap();
        assert_ne!(a, b);
        let f = write_file(&a, "Bracket / v2", "stl", b"solid").unwrap();
        assert_eq!(f.extension().unwrap(), "stl");
        assert!(
            f.file_stem()
                .unwrap()
                .to_string_lossy()
                .starts_with("Bracket")
        );
        // Never over a file that is there (one still open in the slicer).
        let g = write_file(&a, "Bracket / v2", "stl", b"solid").unwrap();
        assert_ne!(f, g);
        let h = write_file(&a, "Bracket / v2", "step", b"ISO-10303-21;").unwrap();
        assert_eq!(h.extension().unwrap(), "step");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_slicers_that_import_step_get_it() {
        let step: Vec<_> = KNOWN
            .iter()
            .filter(|k| k.formats.first() == Some(&ModelFormat::Step))
            .map(|k| k.name)
            .collect();
        assert_eq!(
            step,
            [
                "PrusaSlicer",
                "OrcaSlicer",
                "Bambu Studio",
                "SuperSlicer",
                "Creality Print"
            ]
        );
        // Every one takes STL, the fallback.
        assert!(KNOWN.iter().all(|k| k.formats.contains(&STL)));
    }

    #[test]
    fn an_appimage_in_a_desktop_entry_is_found() {
        let dir = std::env::temp_dir().join(format!("cadrs-slicer-desk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("CrealityPrint_V7.0.1-x86_64-Release.AppImage");
        std::fs::write(&app, b"").unwrap();
        let text = format!(
            "[Desktop Entry]\nName=CrealityPrint\nName[de]=Anders\nExec=\"{}\" --x %F\n\
             Type=Application\n[Desktop Action New]\nName=New\nExec=other\n",
            app.display()
        );
        let entry = parse_desktop_entry(&text).unwrap();
        assert_eq!(entry.name, "CrealityPrint");
        let creality = KNOWN
            .iter()
            .position(|k| k.name == "Creality Print")
            .unwrap();
        let s = find_known(creality as u8, &KNOWN[creality], &[], &[entry]).unwrap();
        assert_eq!(s.name, "Creality Print");
        assert_eq!(s.program, app);
        assert_eq!((s.args, s.trailing), (vec!["--x".to_string()], Vec::new()));
        // An entry whose program is gone isn't a slicer, nor a hidden one.
        std::fs::remove_file(&app).unwrap();
        assert!(parse_desktop_entry(&text).unwrap().launch(&[]).is_none());
        assert!(parse_desktop_entry("[Desktop Entry]\nName=a\nExec=b\nHidden=true\n").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exec_lines_split_into_words() {
        assert_eq!(
            exec_words(r#"flatpak run "--a b" x\y "q\"z" @@u %U @@"#),
            ["flatpak", "run", "--a b", r"x\y", "q\"z", "@@u", "%U", "@@"]
        );
    }

    #[test]
    fn a_versioned_windows_folder_is_found() {
        let root = std::env::temp_dir().join(format!("cadrs-slicer-win-{}", std::process::id()));
        let dir = root.join("UltiMaker Cura 5.8.1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("UltiMaker-Cura.exe"), b"").unwrap();
        let found = windows_paths(&root, "UltiMaker Cura *\\UltiMaker-Cura.exe");
        assert_eq!(found, [dir.join("UltiMaker-Cura.exe")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
