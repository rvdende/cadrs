//! STEP and IGES import and export for the OCCT backend (P3F.2), through the fork's XDE
//! bindings (`opencascade::xde`): assembly structure and product names come with the shapes.
//! OCCT reads and writes files, so the bytes go through a temporary file; its readers and
//! writers keep global state (units, schema), so calls are serialised. Imported shapes that
//! are not valid are healed with `ShapeFix_Shape` first.

use opencascade::primitives::Shape;
use opencascade::xde;

use super::{OcctKernel, face_count};
use crate::exchange::{ExchangeFormat, ExportInstance, ImportedModel, ImportedOccurrence, ImportedPart};
use crate::{BodyId, History, KernelError, Motion, Result};

static EXCHANGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn temp_path(ext: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("cadrs-exchange-{}-{n}.{ext}", std::process::id()))
}

fn ext(format: ExchangeFormat) -> &'static str {
    match format {
        ExchangeFormat::Step => "step",
        ExchangeFormat::Iges => "iges",
    }
}

fn motion(m: &[f64; 12]) -> Motion {
    Motion {
        linear: nalgebra::Matrix3::new(m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]),
        translation: nalgebra::Vector3::new(m[3], m[7], m[11]),
    }
}

pub(super) fn import_model(k: &mut OcctKernel, format: ExchangeFormat, bytes: &[u8]) -> Result<ImportedModel> {
    let failed = |why: String| KernelError::OperationFailed(format!("{} import: {why}", format.label()));
    let path = temp_path(ext(format));
    std::fs::write(&path, bytes).map_err(|e| failed(e.to_string()))?;
    let model = {
        let _guard = EXCHANGE.lock().unwrap_or_else(|e| e.into_inner());
        match format {
            ExchangeFormat::Step => xde::read_step(&path),
            ExchangeFormat::Iges => xde::read_iges(&path),
        }
    };
    let _ = std::fs::remove_file(&path);
    let model = model.map_err(|e| failed(e.to_string()))?;
    let mut parts = Vec::new();
    // A part with no faces (a product with no geometry) is dropped with its occurrences.
    let mut index = Vec::new();
    for p in model.parts {
        if face_count(&p.shape)? == 0 {
            index.push(None);
            continue;
        }
        index.push(Some(parts.len()));
        // Imported shapes are often not quite valid: heal those (the result is kept only if
        // it is valid).
        let shape = if p.shape.is_valid().unwrap_or(false) {
            p.shape
        } else {
            match xde::fix_shape(&p.shape) {
                Ok(fixed) if fixed.is_valid().unwrap_or(false) => fixed,
                _ => p.shape,
            }
        };
        let r = k.insert(shape, History::default())?;
        parts.push(ImportedPart { body: r.bodies[0], name: p.name });
    }
    if parts.is_empty() {
        return Err(failed("the file holds no shapes".into()));
    }
    let occurrences = model
        .occurrences
        .into_iter()
        .filter_map(|o| Some(ImportedOccurrence { part: (*index.get(o.part)?)?, placement: motion(&o.matrix), name: o.name }))
        .collect();
    Ok(ImportedModel { name: model.name, parts, occurrences })
}

pub(super) fn export_model(
    k: &OcctKernel,
    format: ExchangeFormat,
    name: &str,
    parts: &[(BodyId, String)],
    instances: &[ExportInstance],
) -> Result<Vec<u8>> {
    let failed = |why: String| KernelError::OperationFailed(format!("{} export: {why}", format.label()));
    if parts.is_empty() {
        return Err(failed("nothing to export".into()));
    }
    let shapes: Vec<(&Shape, &str)> = parts.iter().map(|(b, n)| Ok((k.body(*b)?, n.as_str()))).collect::<Result<_>>()?;
    let path = temp_path(ext(format));
    let result = {
        let _guard = EXCHANGE.lock().unwrap_or_else(|e| e.into_inner());
        (|| -> std::result::Result<(), opencascade::Error> {
            let mut w = xde::Writer::new(name)?;
            for (s, n) in &shapes {
                w.add_part(s, n)?;
            }
            for i in instances {
                if i.part >= shapes.len() {
                    return Err(opencascade::Error::Occt(format!("instance {} of a part not exported", i.name)));
                }
                w.add_instance(i.part, &i.placement.rows(), &i.name)?;
            }
            match format {
                ExchangeFormat::Step => w.write_step(&path),
                ExchangeFormat::Iges => w.write_iges(&path),
            }
        })()
    };
    let bytes = result.map_err(|e| failed(e.to_string())).and_then(|_| std::fs::read(&path).map_err(|e| failed(e.to_string())));
    let _ = std::fs::remove_file(&path);
    bytes
}
