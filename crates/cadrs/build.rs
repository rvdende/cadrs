//! The Windows executable's icon (issue #14): `assets/icons/cadrs.ico` compiled into a resource
//! with MinGW's `windres` and linked into the binary, so Explorer, shortcuts and the taskbar show
//! it. With it the version resource (#200): the name and version Explorer's Details, SmartScreen
//! and the code signing (SignPath's checks of the file, `tools/windows/README.md`) read. Other targets need nothing here (the window icon is set at run time by
//! `cadrs_app::app_icon`).

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let ico = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/cadrs.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "gnu" {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let rc = out.join("cadrs.rc");
    let ico = ico.canonicalize().expect("assets/icons/cadrs.ico");
    let icon = format!(
        "1 ICON \"{}\"\n",
        ico.display().to_string().replace('\\', "/")
    );
    std::fs::write(&rc, icon + &version_info()).expect("writing cadrs.rc");
    let obj = out.join("cadrs-icon.o");
    // The cross toolchain's windres, else a native one (MSYS2 on Windows).
    let windres = ["x86_64-w64-mingw32-windres", "windres"]
        .into_iter()
        .find(|w| {
            Command::new(w)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        });
    let Some(windres) = windres else {
        println!("cargo:warning=windres not found: cadrs.exe is built without its icon");
        return;
    };
    let ok = Command::new(windres)
        .arg(&rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        println!("cargo:warning=windres failed: cadrs.exe is built without its icon");
        return;
    }
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}

/// The `VERSIONINFO` resource: cadrs and the crate's version (the workspace's).
fn version_info() -> String {
    let v = |k: &str| std::env::var(k).unwrap_or_else(|_| "0".into());
    let (major, minor, patch) = (
        v("CARGO_PKG_VERSION_MAJOR"),
        v("CARGO_PKG_VERSION_MINOR"),
        v("CARGO_PKG_VERSION_PATCH"),
    );
    let version = v("CARGO_PKG_VERSION");
    format!(
        r#"1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "cadrs"
      VALUE "FileDescription", "cadrs"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "cadrs"
      VALUE "LegalCopyright", "MIT OR Apache-2.0"
      VALUE "OriginalFilename", "cadrs.exe"
      VALUE "ProductName", "cadrs"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
