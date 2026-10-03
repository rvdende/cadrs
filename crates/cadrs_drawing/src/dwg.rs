//! DWG through an external converter (P3C.7, X13; decision recorded in `docs/PROGRESS.md`).
//!
//! There is no permissively licensed DWG writer or reader in Rust. DWG export writes our DXF
//! ([`crate::dxf`]) and converts it with a converter found on `PATH`, run as a separate process
//! (never linked): LibreDWG's `dxf2dwg`/`dwg2dxf` (GPL-3) or the ODA File Converter
//! (`ODAFileConverter`, proprietary, installed by the user). Insert DXF/DWG reads a DWG the same
//! way, converted to DXF first. Without a converter the DWG option is disabled and says what to
//! install.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What to install for DWG, for tooltips and errors.
pub const INSTALL_HINT: &str =
    "DWG needs a converter on PATH: install LibreDWG (dxf2dwg and dwg2dxf) or the ODA File Converter (ODAFileConverter)";

/// A DWG converter program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Converter {
    /// LibreDWG's command-line tools.
    LibreDwg { dxf2dwg: Option<PathBuf>, dwg2dxf: Option<PathBuf> },
    /// The ODA File Converter.
    Oda(PathBuf),
}

impl Converter {
    pub fn name(&self) -> &'static str {
        match self {
            Converter::LibreDwg { .. } => "LibreDWG",
            Converter::Oda(_) => "ODA File Converter",
        }
    }

    pub fn can_write(&self) -> bool {
        !matches!(self, Converter::LibreDwg { dxf2dwg: None, .. })
    }

    pub fn can_read(&self) -> bool {
        !matches!(self, Converter::LibreDwg { dwg2dxf: None, .. })
    }
}

/// A program on `PATH` (or at `$CADRS_DWG_CONVERTER_DIR`).
fn which(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("CADRS_DWG_CONVERTER_DIR") {
        dirs.push(d.into());
    }
    if let Some(p) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    for d in dirs {
        for n in [name.to_string(), format!("{name}.exe")] {
            let f = d.join(&n);
            if f.is_file() {
                return Some(f);
            }
        }
    }
    None
}

/// The converter on this machine, if any (LibreDWG first). `CADRS_NO_DWG=1` hides it (tests of
/// the disabled state).
pub fn find_converter() -> Option<Converter> {
    if std::env::var_os("CADRS_NO_DWG").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let (w, r) = (which("dxf2dwg"), which("dwg2dxf"));
    if w.is_some() || r.is_some() {
        return Some(Converter::LibreDwg { dxf2dwg: w, dwg2dxf: r });
    }
    which("ODAFileConverter").map(Converter::Oda)
}

fn run(mut c: Command) -> Result<(), String> {
    let out = c.output().map_err(|e| format!("cannot run the converter: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("the converter failed: {}", err.lines().last().unwrap_or("").trim()))
    }
}

fn temp_dir() -> Result<PathBuf, String> {
    let d = std::env::temp_dir().join(format!("cadrs-dwg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// Writes `dxf` (our DXF text) as the DWG file `out`.
pub fn dxf_to_dwg(conv: &Converter, dxf: &str, out: &Path, version: crate::dxf::DxfVersion) -> Result<(), String> {
    let tmp = temp_dir()?;
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("drawing").to_string();
    let src = tmp.join(format!("{stem}.dxf"));
    std::fs::write(&src, dxf).map_err(|e| e.to_string())?;
    let result = match conv {
        Converter::LibreDwg { dxf2dwg: Some(p), .. } => {
            let mut c = Command::new(p);
            c.arg("-y").arg("-o").arg(out).arg(&src);
            run(c)
        }
        Converter::LibreDwg { dxf2dwg: None, .. } => Err("LibreDWG's dxf2dwg is not installed".into()),
        Converter::Oda(p) => {
            let odir = tmp.join("out");
            let _ = std::fs::create_dir_all(&odir);
            let mut c = Command::new(p);
            let v = match version {
                crate::dxf::DxfVersion::R2013 => "ACAD2013",
                crate::dxf::DxfVersion::R2000 => "ACAD2000",
                crate::dxf::DxfVersion::R2004 => "ACAD2004",
                crate::dxf::DxfVersion::R2007 => "ACAD2007",
                crate::dxf::DxfVersion::R2010 => "ACAD2010",
                crate::dxf::DxfVersion::R2018 => "ACAD2018",
            };
            c.arg(&tmp).arg(&odir).arg(v).arg("DWG").arg("0").arg("1").arg("*.DXF");
            run(c).and_then(|_| {
                std::fs::copy(odir.join(format!("{stem}.dwg")), out).map(|_| ()).map_err(|e| format!("the converter wrote no DWG: {e}"))
            })
        }
    };
    let _ = std::fs::remove_dir_all(&tmp);
    if result.is_ok() && !out.is_file() {
        return Err("the converter wrote no DWG".into());
    }
    result
}

/// Reads the DWG file `dwg` as DXF text.
pub fn dwg_to_dxf(conv: &Converter, dwg: &Path) -> Result<String, String> {
    let tmp = temp_dir()?;
    let stem = dwg.file_stem().and_then(|s| s.to_str()).unwrap_or("drawing").to_string();
    let out = tmp.join(format!("{stem}.dxf"));
    let result = match conv {
        Converter::LibreDwg { dwg2dxf: Some(p), .. } => {
            let mut c = Command::new(p);
            c.arg("-y").arg("-o").arg(&out).arg(dwg);
            run(c)
        }
        Converter::LibreDwg { dwg2dxf: None, .. } => Err("LibreDWG's dwg2dxf is not installed".into()),
        Converter::Oda(p) => {
            let idir = tmp.join("in");
            let _ = std::fs::create_dir_all(&idir);
            std::fs::copy(dwg, idir.join(format!("{stem}.dwg"))).map_err(|e| e.to_string())?;
            let mut c = Command::new(p);
            c.arg(&idir).arg(&tmp).arg("ACAD2013").arg("DXF").arg("0").arg("1").arg("*.DWG");
            run(c)
        }
    }
    .and_then(|_| std::fs::read(&out).map_err(|e| format!("the converter wrote no DXF: {e}")))
    .map(|b| String::from_utf8_lossy(&b).into_owned());
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
