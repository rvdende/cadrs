//! `cadrs --install`: installs this executable for the current user, so a checkout can be the
//! source of truth for the cadrs on a machine. `cargo install-local` (an alias in
//! `.cargo/config.toml`) builds in release and runs it.
//!
//! The executable is copied with the built-in `libraries/` beside it (the app reads them there),
//! so rebuilding the checkout never pulls the executable from under a running install. The new
//! install is assembled beside the old one and then swapped in.
//!
//! - **Linux**: `~/.local/opt/cadrs/` (the executable stripped: the debug symbols are a third of
//!   it), `~/.local/bin/cadrs` linked to it, and `~/.local/share/applications/cadrs.desktop` with
//!   the icon. The window's Wayland app id is `cadrs` (see `cadrs_harness::default_plugins`),
//!   which ties the window to this entry. The entry names the files cadrs opens (its
//!   `MimeType`), so a file manager offers cadrs for them; `~/.local/share/mime/packages/cadrs.xml`
//!   gives the system the types it may lack (a STEP file is otherwise plain text to it).
//! - **macOS**: an app bundle, `~/Applications/cadrs.app` (`Contents/MacOS/` the executable,
//!   `Contents/Resources/` `libraries/` and `cadrs.icns`, `Contents/Info.plist`), so Launchpad and
//!   Spotlight find it. `libraries/` isn't in `Contents/MacOS/`, which is for code: `codesign`
//!   takes a folder there with a dot in its name (`Connector_PinSocket_2.54mm`) for a bundle and
//!   refuses the app. Not stripped: that would break the linker's ad-hoc code signature, which
//!   Apple Silicon needs to run it.
//! - **Windows**: `%LOCALAPPDATA%\Programs\cadrs\` and a Start Menu shortcut (made through
//!   PowerShell's `WScript.Shell`); the shortcut takes its icon from the executable's own icon
//!   resource (`crates/cadrs/build.rs`).
//!
//! `cadrs --bundle <dir>` (macOS) assembles the same app bundle at `<dir>` without installing
//! it: the macOS release packs it (`tools/release/package.sh macos`).

use std::path::{Path, PathBuf};

/// The app icon, as the window and the Windows executable have it (`cadrs_app::app_icon`).
#[cfg(any(target_os = "linux", target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../../../assets/icons/cadrs.png");
#[cfg(target_os = "linux")]
const DESKTOP: &str = include_str!("../../../tools/linux/cadrs.desktop");
/// The types of the files cadrs opens that the system may not know (STEP, BREP, `.cadrs`).
#[cfg(target_os = "linux")]
const MIME: &str = include_str!("../../../tools/linux/cadrs-mime.xml");

fn err(what: &str, p: &Path, e: std::io::Error) -> String {
    format!("cannot {what} {}: {e}", p.display())
}

fn env_dir(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} is not set"))
}

/// Installs this build. Returns where it went.
pub fn run() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    let libraries = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libraries");
    let root = install_root()?;

    let staging = root.with_extension("new");
    let old = root.with_extension("old");
    for d in [&staging, &old] {
        if d.exists() {
            std::fs::remove_dir_all(d).map_err(|e| err("remove", d, e))?;
        }
    }
    assemble(&staging, &exe, &libraries)?;
    if root.exists() {
        std::fs::rename(&root, &old).map_err(|e| err("move", &root, e))?;
    }
    std::fs::rename(&staging, &root).map_err(|e| err("move", &staging, e))?;
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| err("remove", &old, e))?;
    }
    launcher(
        &root
            .join(bin_dir())
            .join(format!("cadrs{}", std::env::consts::EXE_SUFFIX)),
    )?;
    Ok(root)
}

/// The install's files in `dir`: the executable `exe` and `libraries/` (macOS: the bundle).
fn assemble(dir: &Path, exe: &Path, libraries: &Path) -> Result<(), String> {
    let bin = dir.join(bin_dir());
    std::fs::create_dir_all(&bin).map_err(|e| err("create", &bin, e))?;
    let installed = bin.join(format!("cadrs{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(exe, &installed).map_err(|e| err("copy", exe, e))?;
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("strip").arg(&installed).status();
    let resources = dir.join(resources_dir());
    copy_dir(libraries, &resources.join("libraries")).map_err(|e| err("copy", libraries, e))?;
    #[cfg(target_os = "macos")]
    write_bundle(dir)?;
    Ok(())
}

/// `cadrs --bundle <app>`: the macOS app bundle `<app>` (`cadrs.app`) of this executable and
/// the built-in `libraries/` (`libraries`, else the checkout's), as `--install` makes it, without
/// installing it.
pub fn bundle(app: &Path, libraries: Option<&Path>) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("--bundle makes a macOS app bundle: run it on macOS".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libraries");
    let libraries = libraries.unwrap_or(&checkout);
    if app.exists() {
        std::fs::remove_dir_all(app).map_err(|e| err("remove", app, e))?;
    }
    assemble(app, &exe, libraries)
}

/// The folder the install replaces as a whole.
fn install_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    return Ok(env_dir("HOME")?.join(".local/opt/cadrs"));
    #[cfg(target_os = "macos")]
    return Ok(env_dir("HOME")?.join("Applications/cadrs.app"));
    #[cfg(target_os = "windows")]
    return Ok(env_dir("LOCALAPPDATA")?.join("Programs").join("cadrs"));
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return Err("--install supports Linux, macOS and Windows".into());
}

/// Where the executable goes inside the install.
fn bin_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "Contents/MacOS"
    } else {
        ""
    }
}

/// Where `libraries/` goes inside the install.
fn resources_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "Contents/Resources"
    } else {
        ""
    }
}

#[cfg(target_os = "linux")]
fn launcher(installed: &Path) -> Result<(), String> {
    let home = env_dir("HOME")?;
    let bin = home.join(".local/bin");
    let apps = home.join(".local/share/applications");
    let icons = home.join(".local/share/icons/hicolor/256x256/apps");
    let mime = home.join(".local/share/mime");
    let types = mime.join("packages");
    for d in [&bin, &apps, &icons, &types] {
        std::fs::create_dir_all(d).map_err(|e| err("create", d, e))?;
    }
    let link = bin.join("cadrs");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(installed, &link).map_err(|e| err("link", &link, e))?;
    let icon = icons.join("cadrs.png");
    std::fs::write(&icon, ICON).map_err(|e| err("write", &icon, e))?;
    // The icon by its path: a user hicolor theme without an index.theme keeps a stale icon cache
    // that hides a new icon named by theme name.
    let desktop = DESKTOP
        .replace("@EXEC@", &installed.to_string_lossy())
        .replace("@ICON@", &icon.to_string_lossy());
    let entry = apps.join("cadrs.desktop");
    std::fs::write(&entry, desktop).map_err(|e| err("write", &entry, e))?;
    // The file types before the entry that names them. Without these tools (a minimal system)
    // the launcher still works; only "Open with" doesn't offer cadrs.
    let package = types.join("cadrs.xml");
    std::fs::write(&package, MIME).map_err(|e| err("write", &package, e))?;
    for (tool, dir) in [
        ("update-mime-database", &mime),
        ("update-desktop-database", &apps),
    ] {
        let _ = std::process::Command::new(tool)
            .arg(dir)
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}

/// The bundle's `Info.plist` and icon.
#[cfg(target_os = "macos")]
fn write_bundle(bundle: &Path) -> Result<(), String> {
    let contents = bundle.join("Contents");
    let resources = contents.join("Resources");
    std::fs::create_dir_all(&resources).map_err(|e| err("create", &resources, e))?;
    let icns = resources.join("cadrs.icns");
    std::fs::write(&icns, icns_of_png(ICON)).map_err(|e| err("write", &icns, e))?;
    let plist = contents.join("Info.plist");
    std::fs::write(&plist, info_plist(env!("CARGO_PKG_VERSION")))
        .map_err(|e| err("write", &plist, e))
}

/// The bundle is the launcher; Launch Services is told about it so it shows at once.
#[cfg(target_os = "macos")]
fn launcher(installed: &Path) -> Result<(), String> {
    const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    if let Some(bundle) = installed.ancestors().nth(3) {
        let _ = std::process::Command::new(LSREGISTER)
            .arg("-f")
            .arg(bundle)
            .status();
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn launcher(installed: &Path) -> Result<(), String> {
    let programs = env_dir("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs");
    std::fs::create_dir_all(&programs).map_err(|e| err("create", &programs, e))?;
    let script = shortcut_script(&programs.join("cadrs.lnk"), installed);
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
        .map_err(|e| format!("cannot run PowerShell for the Start Menu shortcut: {e}"))?;
    if !status.success() {
        return Err("PowerShell could not make the Start Menu shortcut".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn launcher(_: &Path) -> Result<(), String> {
    Ok(())
}

/// A PowerShell command that makes the shortcut `lnk` to `exe`, with the executable's icon.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn shortcut_script(lnk: &Path, exe: &Path) -> String {
    // A single-quoted PowerShell string takes everything literally but `'`, written twice.
    let q = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
    let dir = exe.parent().unwrap_or(exe);
    format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.WorkingDirectory = {}; $s.IconLocation = {} + ',0'; $s.Description = 'cadrs'; $s.Save()",
        q(lnk),
        q(exe),
        q(dir),
        q(exe)
    )
}

/// A `.icns` file holding `png` (256 × 256) as its `ic08` image: macOS reads PNG data in an icns
/// directly.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn icns_of_png(png: &[u8]) -> Vec<u8> {
    let entry = 8 + png.len() as u32;
    let mut out = Vec::with_capacity(8 + entry as usize);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&(8 + entry).to_be_bytes());
    out.extend_from_slice(b"ic08");
    out.extend_from_slice(&entry.to_be_bytes());
    out.extend_from_slice(png);
    out
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>cadrs</string>
	<key>CFBundleDisplayName</key>
	<string>cadrs</string>
	<key>CFBundleIdentifier</key>
	<string>com.github.rvdende.cadrs</string>
	<key>CFBundleExecutable</key>
	<string>cadrs</string>
	<key>CFBundleIconFile</key>
	<string>cadrs</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.graphics-design</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
"#
    )
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icns_wraps_the_png_with_its_lengths() {
        let png = [1u8, 2, 3];
        let icns = icns_of_png(&png);
        assert_eq!(&icns[0..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
            icns.len()
        );
        assert_eq!(&icns[8..12], b"ic08");
        assert_eq!(u32::from_be_bytes(icns[12..16].try_into().unwrap()), 8 + 3);
        assert_eq!(&icns[16..], &png);
        // The real icon is a PNG, which `ic08` takes as is.
        assert_eq!(
            &include_bytes!("../../../assets/icons/cadrs.png")[1..4],
            b"PNG"
        );
    }

    #[test]
    fn the_plist_names_the_executable_and_icon() {
        let p = info_plist("1.2.3");
        assert!(p.contains("<key>CFBundleExecutable</key>\n\t<string>cadrs</string>"));
        assert!(p.contains("<key>CFBundleIconFile</key>\n\t<string>cadrs</string>"));
        assert!(p.contains("<string>1.2.3</string>"));
    }

    #[test]
    fn the_shortcut_script_quotes_its_paths() {
        let s = shortcut_script(
            Path::new(r"C:\Users\o'neil\cadrs.lnk"),
            Path::new(r"C:\Programs\cadrs\cadrs.exe"),
        );
        assert!(s.contains(r"CreateShortcut('C:\Users\o''neil\cadrs.lnk')"));
        assert!(s.contains(r"$s.TargetPath = 'C:\Programs\cadrs\cadrs.exe'"));
        assert!(s.contains(r"$s.IconLocation = 'C:\Programs\cadrs\cadrs.exe' + ',0'"));
    }
}
