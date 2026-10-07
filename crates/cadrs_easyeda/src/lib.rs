//! Parts from the JLCPCB/LCSC catalogue, for the libraries: search JLCPCB's parts
//! ([`api::search`]), fetch a part's symbol, footprint and 3D model from the JLCEDA/EasyEDA
//! official library ([`api::component`]), convert them ([`convert`]) and write them into a
//! library folder ([`import`]), where the app picks them up like any other library part.
//!
//! The parts come from the JLCEDA/EasyEDA official library, whose terms ask that its parts
//! say so: every imported symbol and footprint names [`SOURCE`].

pub mod api;
pub mod convert;
mod path;

pub use api::{Hit, search};
pub use serde_json;
pub use convert::{Converted, PartInfo, convert};

use std::path::{Path, PathBuf};

/// Where imported parts come from (their "Source" field and footprint description).
pub const SOURCE: &str = "JLCEDA/EasyEDA Official Library (https://easyeda.com, https://lceda.cn)";

/// What [`import`] wrote.
#[derive(Clone, Debug)]
pub struct Imported {
    /// `library:name` of the new symbol and footprint.
    pub symbol: String,
    pub footprint: String,
    /// The 3D model file, when there was one.
    pub model: Option<PathBuf>,
    /// What didn't come across (shapes cadrs doesn't read, a model that wouldn't download).
    pub warnings: Vec<String>,
}

/// An LCSC part number ("C2764087", any case, with or without spaces), or `None`.
pub fn lcsc_number(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_uppercase();
    (s.len() > 1 && s.starts_with('C') && s[1..].bytes().all(|b| b.is_ascii_digit())).then_some(s)
}

/// Fetches the part `lcsc` and writes its symbol, footprint and STEP model into the library
/// folder `dir` (made if missing; the library is named after the folder). `info` adds what the
/// catalogue search knew (description, datasheet).
pub fn import(lcsc: &str, dir: &Path, info: &PartInfo) -> Result<Imported, String> {
    let lcsc = lcsc_number(lcsc).ok_or_else(|| format!("\"{lcsc}\" isn't an LCSC part number (C followed by digits)"))?;
    let result = api::component(&lcsc)?;
    let lib = library_name(dir)?;
    let mut warnings = vec![];
    let mut models = None;
    if let Some(m) = convert(&result, &lib, info)?.model {
        let obj = api::model_obj(&m.uuid).map_err(|e| warnings.push(format!("3D model extent: {e}"))).ok();
        let step = api::model_step(&m.uuid).map_err(|e| warnings.push(format!("3D model: {e}"))).ok();
        models = Some((obj, step));
    }
    let mut r = write_part(&result, dir, info, models)?;
    r.warnings.extend(warnings);
    Ok(r)
}

fn library_name(dir: &Path) -> Result<String, String> {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| "the library folder has no name".into())
}

/// Writes a part (EasyEDA's data, already fetched) into the library folder `dir`, with its 3D
/// model when given (the OBJ for its extent, the STEP file).
pub fn write_part(result: &serde_json::Value, dir: &Path, info: &PartInfo, models: Option<(Option<String>, Option<Vec<u8>>)>) -> Result<Imported, String> {
    let lib = library_name(dir)?;
    let mut c = convert(result, &lib, info)?;
    cadrs_eda::library::ensure_library(dir, "Parts from the JLCPCB/LCSC catalogue (JLCEDA/EasyEDA official library)")?;
    let mut model_file = None;
    if let (Some(m), Some((obj, step))) = (&c.model, models) {
        match step {
            Some(bytes) => {
                let path = dir.join(format!("{}.step", cadrs_eda::library::file_stem(c.footprint.name())));
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                c.footprint.models.push(convert::place_model(m, &path.to_string_lossy(), obj.as_deref()));
                model_file = Some(path);
            }
            // The box of its extent stands in.
            None if obj.is_some() => c.footprint.models.push(convert::place_model(m, "", obj.as_deref())),
            None => {}
        }
    }
    cadrs_eda::library::write_symbol(dir, &c.symbol)?;
    cadrs_eda::library::write_footprint(dir, &c.footprint)?;
    Ok(Imported { symbol: format!("{lib}:{}", c.symbol.name()), footprint: format!("{lib}:{}", c.footprint.name()), model: model_file, warnings: c.warnings })
}

/// A catalogue entry from a part's EasyEDA data (what an offline search lists).
pub fn hit_of(result: &serde_json::Value) -> Hit {
    let para = &result["dataStr"]["head"]["c_para"];
    let p = |k: &str| convert::latin(para[k].as_str().unwrap_or(""));
    Hit {
        lcsc: result["lcsc"]["number"].as_str().map(str::to_string).unwrap_or_else(|| p("Supplier Part")),
        mpn: p("Manufacturer Part"),
        manufacturer: p("Manufacturer"),
        package: p("package"),
        description: result["description"].as_str().unwrap_or("").to_string(),
        category: result["tags"].as_array().and_then(|t| t.first()).and_then(|t| t.as_str()).unwrap_or("").to_string(),
        basic: p("JLCPCB Part Class") == "Basic Part",
        ..Default::default()
    }
}
