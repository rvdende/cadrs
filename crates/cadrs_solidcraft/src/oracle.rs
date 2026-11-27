//! SolidCraft's oracle: what a part must measure.
//!
//! The comparison rules are SolidCraft's own (`apps/solidcraft-cli/src/oracle.rs`): body count,
//! volume and area within a relative 1e-3 unless the case's `expected.json` says otherwise, the
//! centre of mass and the bounding box within 1e-3 of the bounding box's diagonal, the face
//! count where the geometry fixes it, and a STEP export read back with the same volume and area.
//! cadrs also checks the mesh it draws: each face's triangles cover its exact area within 1e-2.

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use cadrs_core::import::{ImportAs, ImportFormat, ImportIds, imported_document};
use cadrs_core::parts::PartKind;
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
use cadrs_core::{ElementId, Feature, Part};

use crate::geom::{V3, cross, norm, sub, v3};
use crate::recipe::{Recipe, Stop};

/// Volume and area: relative tolerance unless the case gives one (SolidCraft's default).
pub const REL_TOL: f64 = 1e-3;
/// Centre of mass and bounding box: this fraction of the bounding box's diagonal.
pub const ABS_FRAC: f64 = 1e-3;
/// The mesh: each face's triangles cover its exact area within this fraction of it (curved
/// faces' chords keep the triangles a little short of it).
pub const MESH_TOL: f64 = 1e-2;

/// What the part measures: totals over its solid bodies.
pub struct Measured {
    pub bodies: usize,
    pub volume: f64,
    pub area: f64,
    pub com: V3,
    pub bbox: (V3, V3),
    pub faces: usize,
    /// The worst relative difference between a face's triangles' area and its exact area: the
    /// mesh drawn covers each face, no more and no less.
    mesh: f64,
}

/// The parts a case measures: its solids, or (a surfacing case's `"part"` in
/// `expected.json`) the one of that name; its target's and its STEP's surfaces then, which hold
/// that part alone.
#[derive(Debug, Clone, Copy)]
pub enum Pick<'a> {
    Solids,
    Named(&'a str),
    Surfaces,
}

impl Pick<'_> {
    pub fn takes(self, p: &Part) -> bool {
        match self {
            Pick::Solids => p.kind == PartKind::Solid,
            Pick::Named(n) => p.name == n,
            Pick::Surfaces => p.kind == PartKind::Surface,
        }
    }

    /// What the part's target and its STEP round trip measure.
    pub fn exported(self) -> Self {
        match self {
            Pick::Named(_) => Pick::Surfaces,
            p => p,
        }
    }
}

pub fn measure(parts: &[Part], pick: Pick) -> Measured {
    let solids: Vec<&Part> = parts.iter().filter(|p| pick.takes(p)).collect();
    let (mut volume, mut area, mut faces, mut mesh) = (0.0, 0.0, 0, 0.0f64);
    let mut moment = [0.0; 3];
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in &solids {
        faces += p.solid.faces.len();
        let s = &p.solid;
        if p.kind == PartKind::Surface {
            // A surface has no volume: its faces' area alone.
            area += s.faces.iter().filter_map(|f| f.area).sum::<f64>();
        } else {
            let m = p.mass.expect("mass properties");
            volume += m.volume;
            area += m.surface_area;
            let c = [m.center_of_mass.x, m.center_of_mass.y, m.center_of_mass.z];
            for k in 0..3 {
                moment[k] += m.volume * c[k];
            }
        }
        if std::env::var("DBG_FACES").is_ok() {
            for f in &p.solid.faces {
                eprintln!(
                    "DBG {:?} r {:?} area {:?} c {:?} name {:?}",
                    f.kind, f.radius, f.area, f.center, f.name
                );
            }
        }
        let s = &p.solid;
        for f in &s.faces {
            let Some(exact) = f.area else { continue };
            let triangles: f64 = (f.first_triangle..f.first_triangle + f.triangle_count)
                .map(|t| {
                    let q = |k: usize| s.positions[s.indices[3 * t + k] as usize];
                    0.5 * norm(cross(sub(q(1), q(0)), sub(q(2), q(0))))
                })
                .sum();
            mesh = mesh.max((triangles - exact).abs() / exact.max(1e-9));
        }
        for q in &p.solid.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    Measured {
        bodies: solids.len(),
        volume,
        area,
        com: moment.map(|x| x / volume),
        bbox: (lo, hi),
        faces,
        mesh,
    }
}

/// One comparison: what, the worst error (relative for totals, mm for positions) and whether
/// it passes.
pub struct Check {
    pub what: &'static str,
    pub error: f64,
    pub ok: bool,
}

pub struct Outcome {
    pub case: String,
    pub source: String,
    /// `Some(pass)` when built and compared; `None` when not translated (the reason in `note`).
    pub pass: Option<bool>,
    pub checks: Vec<Check>,
    pub note: String,
    /// What was measured, when it failed.
    pub measured: String,
}

/// A recipe's feature list measured against its `expected.json`: one comparison per value the
/// case gives. `want` is the case's expected values (any already filled in from `target.step`).
pub fn compare_features(
    case: String,
    source: String,
    features: Vec<Feature>,
    want: &Value,
    pick: Pick,
    notes: Vec<String>,
) -> Outcome {
    let mut outcome = Outcome {
        case,
        source,
        pass: None,
        checks: Vec::new(),
        note: String::new(),
        measured: String::new(),
    };
    let features = Arc::new(features);
    let mut rb = Rebuilder::new();
    let build = rb.rebuild(&features);
    if let Some((_, e)) = build.errors.first() {
        outcome.pass = Some(false);
        outcome.note = format!("failed: {e}");
        return outcome;
    }
    let got = measure(&build.parts, pick);
    if matches!(pick, Pick::Named(_)) && got.bodies == 0 {
        outcome.pass = Some(false);
        outcome.note = format!("failed: no part to measure ({pick:?})");
        return outcome;
    }
    let only: Option<Vec<String>> = want["compare"].as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    });
    let wanted = |k: &str| {
        want.get(k).is_some_and(|v| !v.is_null())
            && only.as_ref().is_none_or(|o| o.iter().any(|x| x == k))
    };
    let rel = want["tolerance"]["rel"].as_f64().unwrap_or(REL_TOL);
    let want_bbox = v3(&want["bbox"]["min"]).zip(v3(&want["bbox"]["max"]));
    let diag = |b: (V3, V3)| norm(sub(b.1, b.0));
    let abs = want["tolerance"]["abs_mm"].as_f64().unwrap_or_else(|| {
        ABS_FRAC
            * want_bbox
                .map(diag)
                .unwrap_or_else(|| diag(got.bbox))
                .max(1.0)
    });
    let mut checks = Vec::new();
    if wanted("body_count") {
        let ok = want["body_count"].as_u64() == Some(got.bodies as u64);
        checks.push(Check {
            what: "bodies",
            error: (got.bodies as f64 - want["body_count"].as_f64().unwrap_or(0.0)).abs(),
            ok,
        });
    }
    for (key, what, g) in [
        ("volume_mm3", "volume", got.volume),
        ("area_mm2", "area", got.area),
    ] {
        if wanted(key) {
            let w = want[key].as_f64().unwrap_or(f64::NAN);
            let e = (g - w).abs() / w.abs().max(1.0);
            checks.push(Check {
                what,
                error: e,
                ok: e <= rel,
            });
        }
    }
    let far = |g: V3, w: V3| (0..3).map(|k| (g[k] - w[k]).abs()).fold(0.0, f64::max);
    if wanted("center_of_mass") {
        let e = far(
            got.com,
            v3(&want["center_of_mass"]).unwrap_or([f64::NAN; 3]),
        );
        checks.push(Check {
            what: "centre of mass",
            error: e,
            ok: e <= abs,
        });
    }
    if wanted("bbox")
        && let Some((lo, hi)) = want_bbox
    {
        let e = far(got.bbox.0, lo).max(far(got.bbox.1, hi));
        checks.push(Check {
            what: "bounding box",
            error: e,
            ok: e <= abs,
        });
    }
    if wanted("faces") {
        let w = want["faces"].as_u64().unwrap_or(0) as usize;
        checks.push(Check {
            what: "faces",
            error: (got.faces as f64 - w as f64).abs(),
            ok: got.faces == w,
        });
    }
    // The mesh drawn: each face's triangles cover the face.
    checks.push(Check {
        what: "mesh",
        error: got.mesh,
        ok: got.mesh <= MESH_TOL,
    });
    // STEP: written and read back with the same volume and area.
    let (ok, e) = step_round_trip(&features, &mut rb, &got, pick);
    checks.push(Check {
        what: "STEP round trip",
        error: e,
        ok,
    });
    outcome.pass = Some(checks.iter().all(|c| c.ok));
    if outcome.pass == Some(false) {
        outcome.measured = format!(
            "got  bodies {} volume {:.4} area {:.4} com {:.4?} bbox {:.4?} faces {} mesh {:.1e}\n  \
             want bodies {} volume {:.4} area {:.4} com {:.4?} bbox {:.4?} faces {}",
            got.bodies,
            got.volume,
            got.area,
            got.com,
            got.bbox,
            got.faces,
            got.mesh,
            want["body_count"],
            want["volume_mm3"].as_f64().unwrap_or(f64::NAN),
            want["area_mm2"].as_f64().unwrap_or(f64::NAN),
            v3(&want["center_of_mass"]).unwrap_or([f64::NAN; 3]),
            want_bbox.unwrap_or(([f64::NAN; 3], [f64::NAN; 3])),
            want["faces"],
        );
    }
    let bad: Vec<String> = checks
        .iter()
        .filter(|c| !c.ok)
        .map(|c| format!("{} off by {:.3e}", c.what, c.error))
        .collect();
    let mut notes = notes;
    notes.extend(bad);
    outcome.note = notes.join("; ");
    outcome.checks = checks;
    outcome
}

/// A case's expected values from its `target.step` (the part as another CAD system
/// builds it), measured as cadrs reads it: the ones `expected.json` doesn't give itself.

/// One SolidCraft case: its `recipe.json` translated and built, compared with the
/// `expected.json` beside it (and its `target.step`, when a case has one).
pub fn run_recipe_case(dir: &Path) -> Outcome {
    let case = dir.file_name().unwrap().to_string_lossy().to_string();
    let read = |f: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap()
    };
    let mut want = read("expected.json");
    let part = want["part"].as_str().map(str::to_string);
    let pick = part.as_deref().map_or(Pick::Solids, Pick::Named);
    if dir.join("target.step").is_file() {
        add_target_values(dir, &mut want, pick.exported());
    }
    let source = want["source"].as_str().unwrap_or("").to_string();
    let recipe = read("recipe.json");
    let fail = |pass, note: String| Outcome {
        case: case.clone(),
        source: source.clone(),
        pass,
        checks: Vec::new(),
        note,
        measured: String::new(),
    };
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Recipe::from_recipe(&recipe)
    }));
    let (features, notes) = match built {
        Ok(Ok(r)) => (r.features_now(), r.notes().to_vec()),
        Ok(Err(Stop::Unsupported(why))) => {
            return fail(None, format!("not translated yet: {why}"));
        }
        Ok(Err(Stop::Failed(why))) => {
            return fail(Some(false), format!("failed: {why}"));
        }
        Err(_) => return fail(Some(false), "failed: panicked".into()),
    };
    compare_features(case, source, features, &want, pick, notes)
}

pub fn add_target_values(dir: &Path, want: &mut Value, pick: Pick) {
    let text = std::fs::read_to_string(dir.join("target.step")).unwrap();
    let plan = Rebuilder::new()
        .plan_import(ImportFormat::Step, text.as_bytes())
        .expect("the target reads");
    let doc = imported_document(
        &plan,
        "target.step",
        text,
        ImportAs::PartStudio,
        ImportIds::fresh(),
    );
    let build = Rebuilder::new().rebuild(doc.elements[0].features());
    assert!(
        build.errors.is_empty(),
        "the target builds: {:?}",
        build.errors
    );
    let m = measure(&build.parts, pick);
    let measured = serde_json::json!({
        "body_count": m.bodies,
        "volume_mm3": m.volume,
        "area_mm2": m.area,
        "center_of_mass": m.com,
        "bbox": { "min": m.bbox.0, "max": m.bbox.1 },
        "faces": m.faces,
    });
    for (k, v) in measured.as_object().unwrap() {
        if want.get(k).is_none() {
            want[k] = v.clone();
        }
    }
}

/// The part exported as STEP and imported again: the worst relative change of volume or area.
fn step_round_trip(
    features: &Arc<Vec<Feature>>,
    rb: &mut Rebuilder,
    got: &Measured,
    pick: Pick,
) -> (bool, f64) {
    let build = rb.rebuild(features);
    let items: Vec<ExportItem> = build
        .parts
        .iter()
        .filter(|p| pick.takes(p))
        .map(|p| ExportItem {
            features: features.clone(),
            part: p.id,
            name: p.name.clone(),
            pose: None,
            source: (ElementId::from_u128(1), p.id),
            source_name: p.name.clone(),
            color: None,
        })
        .collect();
    let Ok(files) = rb.export_files(&ExportRequest::new(ModelFormat::Step, "part", items)) else {
        return (false, f64::INFINITY);
    };
    let text = String::from_utf8_lossy(&files[0].bytes).into_owned();
    let Ok(plan) = rb.plan_import(ImportFormat::Step, text.as_bytes()) else {
        return (false, f64::INFINITY);
    };
    let doc = imported_document(
        &plan,
        "part.step",
        text,
        ImportAs::PartStudio,
        ImportIds::fresh(),
    );
    let back = Rebuilder::new().rebuild(doc.elements[0].features());
    if !back.errors.is_empty() {
        return (false, f64::INFINITY);
    }
    let m = measure(&back.parts, pick.exported());
    let e = ((m.volume - got.volume).abs() / got.volume.abs().max(1.0))
        .max((m.area - got.area).abs() / got.area.abs().max(1.0));
    (e <= REL_TOL, e)
}
