//! Display meshes of a board (P3H.3): [`crate::board_geometry`]'s bodies tessellated into
//! triangles and edge polylines, in the board frame (mm), with their class and colour. The PCB
//! Studio tab builds this once per board (it owns its own kernel) and caches it, so switching
//! boards only re-uploads the meshes.

use cadrs_kernel::{Kernel, Motion, Tessellation};
use nalgebra::{Point3, Vector3};

use crate::board::{ItemId, PcbBoard};
use crate::colors::{BodyClass, Rgba};

/// One body's display mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyMesh {
    pub name: String,
    pub class: BodyClass,
    pub color: Rgba,
    pub item: Option<ItemId>,
    pub positions: Vec<[f32; 3]>,
    /// Per vertex, unit length.
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// The body's edges as polylines.
    pub edges: Vec<Vec<[f32; 3]>>,
}

impl BodyMesh {
    /// The body moved by `m` (positions, normals and edges).
    pub fn moved(&self, m: &Motion) -> BodyMesh {
        let p = |q: &[f32; 3]| {
            let r = m.point(&Point3::new(q[0] as f64, q[1] as f64, q[2] as f64));
            [r.x as f32, r.y as f32, r.z as f32]
        };
        let n = |q: &[f32; 3]| {
            let r = m.vector(&Vector3::new(q[0] as f64, q[1] as f64, q[2] as f64));
            [r.x as f32, r.y as f32, r.z as f32]
        };
        BodyMesh {
            positions: self.positions.iter().map(p).collect(),
            normals: self.normals.iter().map(n).collect(),
            edges: self.edges.iter().map(|e| e.iter().map(p).collect()).collect(),
            ..self.clone()
        }
    }

    /// The box around the body, if it has any vertex.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        BoardMesh { bodies: vec![self.clone()], warnings: vec![] }.bounds()
    }

    /// The body split into two by the direction its faces face: those facing up (the top) and
    /// the rest (the sides and the bottom). The component view colours them apart (PCB11.8: a
    /// dark red top, bright red sides). Edges go with the sides.
    pub fn split_top(&self) -> (BodyMesh, BodyMesh) {
        let mut top = BodyMesh { positions: vec![], normals: vec![], indices: vec![], edges: vec![], ..self.clone() };
        let mut rest = BodyMesh { positions: vec![], normals: vec![], indices: vec![], edges: self.edges.clone(), ..self.clone() };
        for t in self.indices.chunks(3) {
            let up = t.iter().all(|i| self.normals.get(*i as usize).is_some_and(|n| n[2] > 0.7));
            let dst = if up { &mut top } else { &mut rest };
            for i in t {
                dst.indices.push(dst.positions.len() as u32);
                dst.positions.push(self.positions[*i as usize]);
                dst.normals.push(self.normals[*i as usize]);
            }
        }
        (top, rest)
    }
}

/// A part's display mesh (a custom representation, X10) as a body, in the part's frame.
pub fn solid_body(solid: &cadrs_core::Solid, name: &str, class: BodyClass, color: Rgba, item: Option<ItemId>) -> BodyMesh {
    let f = |p: &[f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
    BodyMesh {
        name: name.to_string(),
        class,
        color,
        item,
        positions: solid.positions.iter().map(f).collect(),
        normals: solid.normals.iter().map(f).collect(),
        indices: solid.indices.clone(),
        edges: solid.edges.iter().map(|e| e.points.iter().map(f).collect()).collect(),
    }
}

/// A package's generic box (From ECAD data) in the package frame, for the component view
/// (PCB4.5, PCB11.8).
pub fn package_mesh_in(kernel: &mut dyn Kernel, pkg: &cadrs_idf::Package) -> Result<BodyMesh, String> {
    let body = crate::geometry::package_body(kernel, pkg).map_err(|e| e.to_string())?;
    let m = kernel.tessellate(body, tessellation()).map_err(|e| e.to_string());
    kernel.release(body);
    let m = m?;
    let f = |p: &nalgebra::Point3<f64>| [p.x as f32, p.y as f32, p.z as f32];
    let class = BodyClass::Component(crate::colors::component_kind(&pkg.name));
    Ok(BodyMesh {
        name: pkg.name.clone(),
        class,
        color: class.color(),
        item: None,
        positions: m.positions.iter().map(f).collect(),
        normals: m.normals.iter().map(|n| [n.x as f32, n.y as f32, n.z as f32]).collect(),
        indices: m.indices.iter().flatten().copied().collect(),
        edges: m.edges.iter().map(|(_, pts)| pts.iter().map(f).collect()).collect(),
    })
}

/// [`package_mesh_in`] with a kernel of its own (OpenCascade).
#[cfg(feature = "occt")]
pub fn package_mesh(pkg: &cadrs_idf::Package) -> Result<BodyMesh, String> {
    let mut k = cadrs_kernel::backend::occt::OcctKernel::new();
    package_mesh_in(&mut k, pkg)
}

/// A whole board's display meshes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardMesh {
    pub bodies: Vec<BodyMesh>,
    /// From [`crate::BoardGeometry::warnings`] and tessellation failures.
    pub warnings: Vec<String>,
}

impl BoardMesh {
    /// The corners of the box around every body (for zoom to fit); empty without bodies.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut it = self.bodies.iter().flat_map(|b| b.positions.iter());
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        }))
    }

    /// The number of triangles.
    pub fn triangles(&self) -> usize {
        self.bodies.iter().map(|b| b.indices.len() / 3).sum()
    }
}

/// How finely boards are tessellated: 0.02 mm and 5° (holes and rounded corners stay round at
/// board scale).
pub fn tessellation() -> Tessellation {
    Tessellation { deflection: 0.02, angle: std::f64::consts::PI / 36.0 }
}

/// Builds and tessellates every body of `pcb` in `kernel`, releasing the bodies afterwards.
pub fn board_mesh_in(kernel: &mut dyn Kernel, pcb: &PcbBoard) -> Result<BoardMesh, String> {
    let g = crate::board_geometry(kernel, pcb).map_err(|e| e.to_string())?;
    let mut out = BoardMesh { bodies: Vec::with_capacity(g.bodies.len()), warnings: g.warnings.clone() };
    let f = |p: &nalgebra::Point3<f64>| [p.x as f32, p.y as f32, p.z as f32];
    for b in &g.bodies {
        match kernel.tessellate(b.body, tessellation()) {
            Ok(m) => out.bodies.push(BodyMesh {
                name: b.name.clone(),
                class: b.class,
                color: b.color,
                item: b.item,
                positions: m.positions.iter().map(f).collect(),
                normals: m.normals.iter().map(|n| [n.x as f32, n.y as f32, n.z as f32]).collect(),
                indices: m.indices.iter().flatten().copied().collect(),
                edges: m.edges.iter().map(|(_, pts)| pts.iter().map(f).collect()).collect(),
            }),
            Err(e) => out.warnings.push(format!("{}: {e}", b.name)),
        }
    }
    g.release(kernel);
    Ok(out)
}

/// [`board_mesh_in`] with a kernel of its own (OpenCascade).
#[cfg(feature = "occt")]
pub fn board_mesh(pcb: &PcbBoard) -> Result<BoardMesh, String> {
    let mut k = cadrs_kernel::backend::occt::OcctKernel::new();
    board_mesh_in(&mut k, pcb)
}
