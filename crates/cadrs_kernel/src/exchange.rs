//! Data exchange (P3F.2): importing and exporting neutral CAD files, and mesh files.
//!
//! - **B-rep files** go through the backend: [`crate::Kernel::import_model`] reads STEP or
//!   IGES into bodies with the file's assembly structure ([`ImportedModel`]: each distinct part
//!   once, and its occurrences with their placements and names), and
//!   [`crate::Kernel::export_model`] writes bodies (optionally as an assembly of instances) to
//!   STEP or IGES with product names.
//! - **Mesh files** are written here, from a [`TriMesh`] (the backend's tessellation at the
//!   tolerance asked for): [`write_stl_binary`], [`write_stl_ascii`] and [`write_obj`]. They
//!   need no backend support.
//! - [`mesh_volume`]: the volume a closed triangle mesh encloses (the divergence theorem), to
//!   check a mesh export against the solid.

use std::fmt::Write as _;

use nalgebra::{Point3, Vector3};

use crate::{BodyId, Motion, TriMesh};

/// A neutral B-rep file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExchangeFormat {
    /// STEP (ISO 10303, AP214).
    Step,
    /// IGES 5.3 (solids as MSBO solids).
    Iges,
}

impl ExchangeFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExchangeFormat::Step => "STEP",
            ExchangeFormat::Iges => "IGES",
        }
    }

    /// The format a file name's extension says (`.step`/`.stp`, `.iges`/`.igs`).
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        match ext.as_str() {
            "step" | "stp" => Some(ExchangeFormat::Step),
            "iges" | "igs" => Some(ExchangeFormat::Iges),
            _ => None,
        }
    }
}

/// A part of an imported file: its body (where the file puts the part's own coordinates) and
/// its product name (empty when the file gives none).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPart {
    pub body: BodyId,
    pub name: String,
}

/// One use of an imported part: which part, where (the file's coordinates), and the
/// instance's name (empty when the file gives none).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedOccurrence {
    pub part: usize,
    pub placement: Motion,
    pub name: String,
}

/// An imported file. A file without assemblies has one occurrence per part, at the identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedModel {
    /// The top-level product's name (empty when the file gives none).
    pub name: String,
    pub parts: Vec<ImportedPart>,
    pub occurrences: Vec<ImportedOccurrence>,
}

impl ImportedModel {
    /// True when some part is used more than once or away from the identity: the file is an
    /// assembly, not just a list of parts.
    pub fn is_assembly(&self) -> bool {
        let mut seen = vec![0usize; self.parts.len()];
        for o in &self.occurrences {
            if let Some(n) = seen.get_mut(o.part) {
                *n += 1;
            }
            let moved = (o.placement.linear - nalgebra::Matrix3::identity()).norm() > 1e-12 || o.placement.translation.norm() > 1e-12;
            if moved {
                return true;
            }
        }
        seen.iter().any(|n| *n > 1)
    }
}

/// An instance to export: which of the exported parts, where, and its name.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportInstance {
    pub part: usize,
    pub placement: Motion,
    pub name: String,
}

/// The triangles of a mesh as corner positions (skipping triangles with a bad index).
pub fn triangles(mesh: &TriMesh) -> impl Iterator<Item = [Point3<f64>; 3]> + '_ {
    mesh.indices.iter().filter_map(|t| {
        let p = |i: u32| mesh.positions.get(i as usize).copied();
        Some([p(t[0])?, p(t[1])?, p(t[2])?])
    })
}

fn facet_normal(t: &[Point3<f64>; 3]) -> Vector3<f64> {
    let n = (t[1] - t[0]).cross(&(t[2] - t[0]));
    let l = n.norm();
    if l > 0.0 { n / l } else { Vector3::zeros() }
}

/// A binary STL file: an 80-byte header, the triangle count, then per triangle its normal,
/// three corners (f32, little-endian) and a zero attribute word. Lengths as given (mm).
pub fn write_stl_binary(mesh: &TriMesh, header: &str) -> Vec<u8> {
    let tris: Vec<[Point3<f64>; 3]> = triangles(mesh).collect();
    let mut out = Vec::with_capacity(84 + tris.len() * 50);
    let mut h = [b' '; 80];
    for (i, b) in header.bytes().take(80).enumerate() {
        h[i] = b;
    }
    // A binary STL must not start with "solid" (readers would take it for ASCII).
    if h.starts_with(b"solid") {
        h[0] = b'S';
    }
    out.extend_from_slice(&h);
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in &tris {
        let n = facet_normal(t);
        for v in [n.x, n.y, n.z] {
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        for p in t {
            for v in [p.x, p.y, p.z] {
                out.extend_from_slice(&(v as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// An ASCII STL file (`solid <name>` … `endsolid <name>`).
pub fn write_stl_ascii(mesh: &TriMesh, name: &str) -> String {
    let name: String = name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    let mut s = format!("solid {name}\n");
    for t in triangles(mesh) {
        let n = facet_normal(&t);
        let _ = writeln!(s, "  facet normal {:e} {:e} {:e}", n.x, n.y, n.z);
        s.push_str("    outer loop\n");
        for p in &t {
            let _ = writeln!(s, "      vertex {:e} {:e} {:e}", p.x, p.y, p.z);
        }
        s.push_str("    endloop\n  endfacet\n");
    }
    let _ = writeln!(s, "endsolid {name}");
    s
}

/// A Wavefront OBJ file: one object (`o <name>`) per mesh with its vertices (`v`), vertex
/// normals (`vn`) and triangles (`f v//vn`), numbered across the whole file.
pub fn write_obj(meshes: &[(&str, &TriMesh)]) -> String {
    let mut s = String::from("# cadrs OBJ export (millimetres)\n");
    let mut base = 1usize;
    for (name, mesh) in meshes {
        let name: String = name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
        let _ = writeln!(s, "o {name}");
        for p in &mesh.positions {
            let _ = writeln!(s, "v {} {} {}", p.x, p.y, p.z);
        }
        let normals = mesh.normals.len() == mesh.positions.len();
        if normals {
            for n in &mesh.normals {
                let _ = writeln!(s, "vn {} {} {}", n.x, n.y, n.z);
            }
        }
        for t in &mesh.indices {
            let [a, b, c] = t.map(|i| i as usize + base);
            if normals {
                let _ = writeln!(s, "f {a}//{a} {b}//{b} {c}//{c}");
            } else {
                let _ = writeln!(s, "f {a} {b} {c}");
            }
        }
        base += mesh.positions.len();
    }
    s
}

/// The volume a closed, outward-oriented triangle mesh encloses: Σ (a · (b × c)) / 6 over its
/// triangles (the divergence theorem; exact for a mesh of flat faces).
pub fn mesh_volume(tris: impl IntoIterator<Item = [Point3<f64>; 3]>) -> f64 {
    tris.into_iter().map(|[a, b, c]| a.coords.dot(&b.coords.cross(&c.coords))).sum::<f64>() / 6.0
}

/// The triangles of a binary STL file.
pub fn read_stl_binary(bytes: &[u8]) -> Option<Vec<[Point3<f64>; 3]>> {
    let n = u32::from_le_bytes(bytes.get(80..84)?.try_into().ok()?) as usize;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let rec = bytes.get(84 + k * 50..84 + (k + 1) * 50)?;
        let f = |i: usize| f32::from_le_bytes(rec[i * 4..i * 4 + 4].try_into().unwrap()) as f64;
        let p = |j: usize| Point3::new(f(3 + j * 3), f(4 + j * 3), f(5 + j * 3));
        out.push([p(0), p(1), p(2)]);
    }
    Some(out)
}

/// The triangles of an ASCII STL file.
pub fn read_stl_ascii(text: &str) -> Vec<[Point3<f64>; 3]> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for line in text.lines() {
        let mut w = line.split_whitespace();
        if w.next() == Some("vertex") {
            let v: Vec<f64> = w.filter_map(|x| x.parse().ok()).collect();
            if v.len() == 3 {
                cur.push(Point3::new(v[0], v[1], v[2]));
            }
            if cur.len() == 3 {
                out.push([cur[0], cur[1], cur[2]]);
                cur.clear();
            }
        }
    }
    out
}

/// The triangles of an OBJ file (faces with more corners are fanned).
pub fn read_obj(text: &str) -> Vec<[Point3<f64>; 3]> {
    let mut vs = Vec::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let v: Vec<f64> = w.take(3).filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    vs.push(Point3::new(v[0], v[1], v[2]));
                }
            }
            Some("f") => {
                let idx: Vec<usize> = w.filter_map(|x| x.split('/').next()?.parse::<usize>().ok()).collect();
                for k in 1..idx.len().saturating_sub(1) {
                    let g = |i: usize| vs.get(i.wrapping_sub(1)).copied();
                    if let (Some(a), Some(b), Some(c)) = (g(idx[0]), g(idx[k]), g(idx[k + 1])) {
                        out.push([a, b, c]);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube as 12 outward triangles.
    fn cube() -> TriMesh {
        let p = |x: f64, y: f64, z: f64| Point3::new(x, y, z);
        let positions = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.), p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)];
        let indices = vec![
            [0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4],
            [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7],
        ];
        TriMesh { normals: vec![Vector3::zeros(); 8], positions, indices, ..TriMesh::default() }
    }

    #[test]
    fn mesh_files_round_trip_with_the_volume() {
        let m = cube();
        assert!((mesh_volume(triangles(&m)) - 1.0).abs() < 1e-12);
        let bin = write_stl_binary(&m, "solid cube");
        assert_eq!(bin.len(), 84 + 12 * 50);
        assert_ne!(&bin[..5], b"solid");
        let t = read_stl_binary(&bin).unwrap();
        assert_eq!(t.len(), 12);
        assert!((mesh_volume(t) - 1.0).abs() < 1e-6);
        let ascii = read_stl_ascii(&write_stl_ascii(&m, "the cube"));
        assert_eq!(ascii.len(), 12);
        assert!((mesh_volume(ascii) - 1.0).abs() < 1e-9);
        let obj = read_obj(&write_obj(&[("a", &m), ("b", &m)]));
        assert_eq!(obj.len(), 24);
        assert!((mesh_volume(obj) - 2.0).abs() < 1e-9);
    }
}
