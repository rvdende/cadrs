//! Native CAD files turned into STEP by [cadmpeg](https://github.com/cadmpeg/cadmpeg), for Import.
//!
//! cadmpeg reads the files of other CAD programs (SolidWorks, Fusion, Inventor, CATIA, Creo, NX,
//! Solid Edge, Rhino, FreeCAD, …) into its neutral model and writes that as STEP. Import keeps
//! the STEP it wrote as the feature's file, so a rebuild reads it with cadrs's own STEP import
//! and doesn't need cadmpeg (nor change when cadmpeg does).
//!
//! The formats cadrs reads itself (STEP, IGES) stay with cadrs's readers; [`extensions`] lists
//! the others, as cadmpeg's catalog names them.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, View};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_registry::{DETECTION_PREFIX_LEN, Format, InputCatalog, ResolvedSource};

/// The extensions cadrs reads without cadmpeg.
const OWN: &[&str] = &["step", "stp", "p21", "iges", "igs"];

/// The (lowercase) extensions of the native files cadmpeg reads for Import, other than those
/// cadrs reads itself, in cadmpeg's catalog order.
pub fn extensions() -> Vec<&'static str> {
    static EXTENSIONS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    EXTENSIONS
        .get_or_init(|| {
            let mut out: Vec<&'static str> = Vec::new();
            for d in InputCatalog::with_builtins().descriptors() {
                for e in d.extensions() {
                    // The neutral CADIR document isn't a CAD program's file.
                    if OWN.contains(e) || matches!(*e, "cadir" | "json") || out.contains(e) {
                        continue;
                    }
                    out.push(e);
                }
            }
            out
        })
        .clone()
}

/// The native formats for a file picker's filters, by program: its name ("SolidWorks") and its
/// extensions, in cadmpeg's catalog order. Formats that share their extensions are one group
/// (Creo and NX `.prt`: "Creo / NX").
pub fn groups() -> Vec<(String, Vec<&'static str>)> {
    let mut out: Vec<(String, Vec<&'static str>)> = Vec::new();
    for d in InputCatalog::with_builtins().descriptors() {
        let exts: Vec<&'static str> = d
            .extensions()
            .iter()
            .copied()
            .filter(|e| extensions().contains(e))
            .collect();
        if exts.is_empty() {
            continue;
        }
        let name = program(&d.format_id().to_string());
        if let Some(g) = out.iter_mut().find(|g| g.1 == exts) {
            g.0 = format!("{} / {name}", g.0);
        } else {
            out.push((name, exts));
        }
    }
    out
}

/// The program (or format) a cadmpeg format id is the files of; the id itself for one cadrs
/// doesn't know yet.
fn program(id: &str) -> String {
    match id {
        "fcstd" => "FreeCAD",
        "f3d" => "Fusion",
        "inventor" => "Inventor",
        "solidedge" => "Solid Edge",
        "ironcad" => "IronCAD",
        "sldprt" => "SolidWorks",
        "catia" => "CATIA V5",
        "creo" => "Creo",
        "nx" => "NX",
        "rhino" => "Rhino",
        "sat" => "ACIS",
        "pvz" => "Creo View",
        "edrawings" => "eDrawings",
        "usd" => "OpenUSD",
        "onshape" => "Onshape bundle",
        other => return other.to_string(),
    }
    .to_string()
}

/// Whether cadmpeg reads a file with this name for Import (by its extension).
pub fn reads(file_name: &str) -> bool {
    std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions().contains(&e.to_ascii_lowercase().as_str()))
}

/// A file turned into STEP.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The STEP file's bytes.
    pub step: Vec<u8>,
    /// cadmpeg's name for the format it read ("sldprt", "f3d", …).
    pub format: String,
}

/// Reads a native CAD file with cadmpeg and writes it as STEP. The error says why it couldn't
/// (the format not recognized, a decode or export failure), for a toast.
pub fn to_step(file_name: &str, bytes: &[u8]) -> Result<Converted, String> {
    let catalog = InputCatalog::with_builtins();
    let policy = DecodePolicy::default();
    let arena = DecodeArena::new();
    // STEP writes the model: no design history is needed.
    let depth = DecodeDepth::Geometry;
    let ctx = DecodeContext::new(&arena, &policy, depth);
    let prefix = &bytes[..bytes.len().min(DETECTION_PREFIX_LEN)];
    let mut source = Cursor::new(bytes);
    let resolved = catalog
        .resolve_seekable_source(&ctx, View::over_retained(prefix), &mut source, None)
        .map_err(|e| format!("{file_name}: {e}"))?;
    let codec = match resolved {
        ResolvedSource::Native { codec, .. } => codec,
        ResolvedSource::Cadir | ResolvedSource::Unrecognized => {
            return Err(format!("{file_name}: not a CAD file cadmpeg reads"));
        }
    };
    let format = codec.id().to_string();
    let _ = ctx.finish_session();
    let options = DecodeOptions {
        depth,
        policy,
        ..Default::default()
    };
    source.set_position(0);
    let decoded = codec
        .decode(&mut source, &options)
        .map_err(|e| format!("{file_name}: {e}"))?;
    let (ir, _report, fidelity) = decoded.into_parts();
    let encoder = cadmpeg_registry::build_encoder(Format::Step);
    let plan = encoder
        .plan(EncodeInput::new(&ir, Some(&fidelity)))
        .map_err(|e| format!("{file_name}: {e}"))?;
    let mut step = Vec::new();
    plan.write_to(&mut step)
        .map_err(|e| format!("{file_name}: {e}"))?;
    if step.is_empty() {
        return Err(format!("{file_name}: cadmpeg found no geometry to import"));
    }
    Ok(Converted { step, format })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_lists_the_native_formats_not_steps_or_iges() {
        let e = extensions();
        for native in ["sldprt", "f3d", "ipt", "catpart", "3dm", "fcstd"] {
            assert!(e.contains(&native), "{native} missing from {e:?}");
        }
        for own in OWN {
            assert!(!e.contains(own), "{own} is cadrs's own");
        }
        assert!(reads("Bracket.SLDPRT"));
        let g = groups();
        assert!(g.iter().any(|(n, e)| n == "SolidWorks" && e == &["sldprt"]));
        assert!(
            g.iter().any(|(n, e)| n == "Creo / NX" && e == &["prt"]),
            "{g:?}"
        );
        // Every extension is in one group.
        let mut all: Vec<&str> = g.iter().flat_map(|(_, e)| e.clone()).collect();
        all.sort();
        let mut want = extensions();
        want.sort();
        assert_eq!(all, want);
        assert!(!reads("bracket.step"));
    }
}
