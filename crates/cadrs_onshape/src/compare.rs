//! 1:1 comparison of a cadrs document with a STEP file (Onshape's export of the same Part
//! Studio): the features, and each part's volume, area, box and faces, as JSON.
use std::collections::BTreeMap;
use std::path::Path;

use cadrs_core::document::ElementKind;
use cadrs_kernel::{DefaultKernel, Kernel, SurfaceKind};
use serde_json::{Value, json};

fn kinds<I: IntoIterator<Item = Option<SurfaceKind>>>(it: I) -> Value {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for k in it {
        *m.entry(k.map_or("Other".into(), |k| format!("{k:?}")))
            .or_default() += 1;
    }
    json!(m)
}

fn body(
    name: String,
    volume: f64,
    area: f64,
    bb: Option<([f64; 3], [f64; 3])>,
    faces: usize,
    kinds: Value,
) -> Value {
    json!({
        "name": name, "volume": volume, "area": area, "faces": faces, "kinds": kinds,
        "bbox": bb.map(|(a, b)| json!([a, b])),
    })
}

/// Reads `cadrs_file` and `step_file` and measures both.
pub fn compare(cadrs_file: &Path, step_file: &Path) -> Result<Value, String> {
    let pkg = cadrs_core::native::read(cadrs_file)?;
    let doc = &pkg.file.document;
    let mut studios = Vec::new();
    for el in &doc.elements {
        if !matches!(el.kind, ElementKind::PartStudio { .. }) {
            continue;
        }
        let feats = &el.features()[..el.rollback_index()];
        let list: Vec<Value> = feats
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let k = format!("{:?}", f.kind);
                json!({ "n": i + 1, "name": f.name, "kind": k.split(['(', ' ', '{']).next().unwrap_or("") })
            })
            .collect();
        let t = std::time::Instant::now();
        let built = cadrs_core::rebuild::build(&el.features_to_build());
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let parts: Vec<Value> = built
            .parts
            .iter()
            .map(|p| {
                let (v, a) = p
                    .mass
                    .as_ref()
                    .map_or((p.solid.volume(), 0.0), |m| (m.volume, m.surface_area));
                body(
                    p.name.clone(),
                    v,
                    a,
                    p.solid.bounds(),
                    p.solid.faces.len(),
                    kinds(p.solid.faces.iter().map(|f| f.kind)),
                )
            })
            .collect();
        studios
            .push(json!({ "name": el.name, "features": list, "parts": parts, "rebuild_ms": ms }));
    }

    let bytes = std::fs::read(step_file).map_err(|e| e.to_string())?;
    let mut k = DefaultKernel::new();
    let ids = k.import_step(&bytes).map_err(|e| e.to_string())?;
    let mut step = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let m = k.mass_properties(*id).map_err(|e| e.to_string())?;
        let bb = k
            .bounding_box(*id)
            .ok()
            .map(|b| ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]));
        let fk = k.face_kinds(*id).map_err(|e| e.to_string())?;
        step.push(body(
            format!("STEP body {}", i + 1),
            m.volume,
            m.surface_area,
            bb,
            fk.len(),
            kinds(fk.into_iter().map(Some)),
        ));
    }
    Ok(json!({
        "document": doc.name, "cadrs_file": cadrs_file.display().to_string(),
        "step_file": step_file.display().to_string(), "studios": studios, "step": step,
    }))
}
