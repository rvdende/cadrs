//! A native board (one with a [`cadrs_eda::Design`]) as a STEP model, as KiCad's File → Export
//! → STEP: the board body (outline, cut-outs, holes) and each footprint's 3D model in place —
//! its model file when the document has it (STEP as is, VRML as a meshed solid), else its
//! generated body ([`cadrs_eda::model3d`]: boxes and cylinders as extruded solids). Footprints
//! without a visible model are left out, as in KiCad.

use cadrs_eda::footprint::Model3d;
use cadrs_eda::model3d::{Axis as MAxis, Solid};
use cadrs_kernel::{BodyId, Curve2, Extent, Kernel, Loop, Motion, Plane, Profile, Region};
use nalgebra::{Point2, Point3, Unit, Vector3};

use crate::board::PcbBoard;
use crate::placement::placement_motion;

fn first(r: cadrs_kernel::OpResult) -> Result<BodyId, String> {
    r.bodies.into_iter().next().ok_or_else(|| "no body".to_string())
}

/// A generated primitive as a kernel solid.
fn primitive(k: &mut dyn Kernel, s: &Solid) -> Result<BodyId, String> {
    match *s {
        Solid::Box { min, max } => {
            let plane = Plane { origin: Point3::new(0.0, 0.0, min[2]), ..Plane::top() };
            let (a, b) = (Point2::new(min[0], min[1]), Point2::new(max[0], max[1]));
            let c = [a, Point2::new(b.x, a.y), b, Point2::new(a.x, b.y)];
            let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
            let region = Region { outer: Loop { curves }, holes: vec![], source: None };
            first(k.extrude(&Profile::new(plane, vec![region]), Extent::Blind((max[2] - min[2]).max(1e-3))).map_err(|e| e.to_string())?)
        }
        Solid::Cylinder { base, axis, radius, length } => {
            let (normal, x_dir) = match axis {
                MAxis::X => (Vector3::x(), Vector3::y()),
                MAxis::Y => (Vector3::y(), Vector3::z()),
                MAxis::Z => (Vector3::z(), Vector3::x()),
            };
            let plane = Plane { origin: Point3::new(base[0], base[1], base[2]), x_dir: Unit::new_normalize(x_dir), normal: Unit::new_normalize(normal) };
            let region = Region { outer: Loop { curves: vec![Curve2::Circle { center: Point2::origin(), radius, source: None }] }, holes: vec![], source: None };
            first(k.extrude(&Profile::new(plane, vec![region]), Extent::Blind(length.max(1e-3))).map_err(|e| e.to_string())?)
        }
    }
}

/// A model file's solids (STEP imported, VRML meshed), in the file's frame and units.
fn file_solids(k: &mut dyn Kernel, bytes: &[u8], ext: &str) -> Result<(Vec<BodyId>, f64), String> {
    match ext.to_ascii_lowercase().as_str() {
        "step" | "stp" => Ok((k.import_step(bytes).map_err(|e| e.to_string())?, 1.0)),
        "wrl" | "vrml" | "obj" => {
            let text = String::from_utf8_lossy(bytes);
            let (meshes, unit) = if ext.eq_ignore_ascii_case("obj") { (cadrs_eda::obj::read(&text)?, 1.0) } else { (cadrs_eda::wrl::read(&text)?, cadrs_eda::wrl::KICAD_UNIT_MM) };
            let mut bodies = vec![];
            for m in meshes {
                let p = |i: u32| {
                    let q = m.positions[i as usize];
                    Point3::new(q[0] as f64, q[1] as f64, q[2] as f64)
                };
                let tris: Vec<[Point3<f64>; 3]> = m.indices.as_chunks::<3>().0.iter().map(|t| [p(t[0]), p(t[1]), p(t[2])]).collect();
                if let Ok(b) = k.mesh_solid(&tris, 1e-4) {
                    bodies.push(b);
                }
            }
            Ok((bodies, unit))
        }
        other => Err(format!("3D models of type .{other} aren't supported")),
    }
}

fn moved(k: &mut dyn Kernel, b: BodyId, m: &Motion) -> Result<BodyId, String> {
    let r = k.transform_motion(b, m).map_err(|e| e.to_string())?;
    k.release(b);
    first(r)
}

/// Bodies with their names (the board, then each part by reference).
pub type NamedBodies = Vec<(String, BodyId)>;

/// The board and its parts' models as bodies (named), with warnings for models that couldn't
/// be made. The caller releases the bodies.
pub fn native_board_bodies(k: &mut dyn Kernel, pcb: &PcbBoard, design: &cadrs_eda::Design) -> Result<(NamedBodies, Vec<String>), String> {
    let g = crate::board_geometry(k, pcb).map_err(|e| e.to_string())?;
    let mut out = vec![];
    let mut warnings = g.warnings.clone();
    for b in &g.bodies {
        if b.class == crate::colors::BodyClass::Board {
            out.push((b.name.clone(), b.body));
        } else {
            k.release(b.body);
        }
    }
    let t = pcb.thickness();
    for f in &design.board.footprints {
        let Some(m) = f.footprint.models.iter().find(|m| m.visible && (m.body.is_some() || m.blob.is_some())) else { continue };
        let Some(p) = pcb.board.placements.iter().find(|p| p.refdes == f.reference()) else { continue };
        let place = placement_motion(p, t);
        match part_bodies(k, m) {
            Ok((bodies, unit)) => {
                let motion = crate::mesh::model_motion(m, unit).then(&place);
                for (i, b) in bodies.into_iter().enumerate() {
                    match moved(k, b, &motion) {
                        Ok(b) => out.push((if i == 0 { f.reference().to_string() } else { format!("{} {}", f.reference(), i + 1) }, b)),
                        Err(e) => warnings.push(format!("{}: {e}", f.reference())),
                    }
                }
            }
            Err(e) => warnings.push(format!("{}: {e}", f.reference())),
        }
    }
    Ok((out, warnings))
}

/// A model's solids: its file when the document has it, else its generated body.
fn part_bodies(k: &mut dyn Kernel, m: &Model3d) -> Result<(Vec<BodyId>, f64), String> {
    let ext = std::path::Path::new(&m.source).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(bytes) = m.blob.as_ref().and_then(|h| cadrs_core::blobs::get(h)) {
        let r = file_solids(k, &bytes, &ext);
        if r.as_ref().is_ok_and(|(b, _)| !b.is_empty()) || m.body.is_none() {
            return r;
        }
    }
    let body = m.body.as_ref().ok_or("no model")?;
    let mut v = vec![];
    for part in &body.parts {
        v.push(primitive(k, &part.solid)?);
    }
    Ok((v, 1.0))
}

/// The board as STEP bytes (and the warnings).
#[cfg(feature = "occt")]
pub fn native_board_step(pcb: &PcbBoard, design: &cadrs_eda::Design) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut k = cadrs_kernel::backend::occt::OcctKernel::new();
    let (bodies, warnings) = native_board_bodies(&mut k, pcb, design)?;
    let ids: Vec<BodyId> = bodies.iter().map(|(_, b)| *b).collect();
    let bytes = k.export_step(&ids).map_err(|e| e.to_string());
    for b in ids {
        k.release(b);
    }
    Ok((bytes?, warnings))
}

#[cfg(all(test, feature = "occt"))]
mod tests {
    use super::*;

    #[test]
    fn a_course_board_exports_with_its_parts() {
        let lib = cadrs_eda::library::LibraryTable::builtin();
        let d = cadrs_eda::getting_started::gs18(&lib);
        let pcb = cadrs_core::pcb::design::pcb_board("course", &d);
        let mut k = cadrs_kernel::backend::occt::OcctKernel::new();
        let (bodies, warnings) = native_board_bodies(&mut k, &pcb, &d).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        // The board and the three parts' generated bodies (several primitives each).
        assert!(bodies.iter().any(|(n, _)| n.starts_with("Board")), "{:?}", bodies.iter().map(|b| &b.0).collect::<Vec<_>>());
        for r in ["BT1", "R1", "D1"] {
            assert!(bodies.iter().any(|(n, _)| n == r), "{r}");
        }
        // The LED stands on the board's top face (1.6 mm up), not below it.
        let led = bodies.iter().find(|(n, _)| n == "D1").unwrap().1;
        let bb = k.bounding_box(led).unwrap();
        assert!(bb.max.z > 1.6 + 4.0, "{bb:?}");
        let ids: Vec<BodyId> = bodies.iter().map(|b| b.1).collect();
        let step = k.export_step(&ids).unwrap();
        assert!(String::from_utf8_lossy(&step[..200.min(step.len())]).contains("ISO-10303-21"));
    }
}
