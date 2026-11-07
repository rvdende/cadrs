//! glTF binary files (`.glb`): read as triangle meshes for Import, and written by Export.
//!
//! - **Reading** ([`read`]): the default scene's nodes with meshes, each node's transform
//!   (its parents' included) applied, one [`GlbMesh`] per triangle primitive (triangle strips
//!   and fans are unrolled; points and lines are skipped), named after its node, else its mesh,
//!   with its material's base colour. The numbers are the file's (glTF says metres, +Y up:
//!   Import scales and turns them, [`crate::import::ImportFeature::scale`]).
//! - **Writing** ([`write`]): a node per part with its mesh (positions, normals, triangles) and
//!   a material of its colour, in the units and orientation given (Export writes metres, +Y up,
//!   as glTF says).
//!
//! glTF colours are linear; cadrs's [`crate::appearance::Appearance`] is sRGB
//! ([`srgb_to_linear`], [`linear_to_srgb`]).

use crate::import::Triangle;

/// A triangle primitive of a node.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbMesh {
    /// The node's name, else its mesh's, else "Mesh N".
    pub name: String,
    /// In the scene's coordinates (the file's units).
    pub triangles: Vec<Triangle>,
    /// The material's base colour factor (linear RGBA); `None` without a material.
    pub color: Option<[f32; 4]>,
}

/// The triangle primitives of a `.glb` file's default scene (else its first).
pub fn read(bytes: &[u8]) -> Result<Vec<GlbMesh>, String> {
    let g = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a glTF file: {e}"))?;
    let blob = g.blob.as_deref();
    let scene = g
        .default_scene()
        .or_else(|| g.scenes().next())
        .ok_or("the file has no scene")?;
    let mut out = Vec::new();
    let mut stack: Vec<(gltf::Node, nalgebra::Matrix4<f64>)> = scene
        .nodes()
        .map(|n| (n, nalgebra::Matrix4::identity()))
        .collect();
    stack.reverse();
    while let Some((node, parent)) = stack.pop() {
        let local = node.transform().matrix();
        let m = parent * nalgebra::Matrix4::from_fn(|r, c| local[c][r] as f64);
        if let Some(mesh) = node.mesh() {
            let name = node
                .name()
                .or(mesh.name())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Mesh {}", mesh.index() + 1));
            for prim in mesh.primitives() {
                let reader = prim.reader(|b| match b.source() {
                    gltf::buffer::Source::Bin => blob,
                    gltf::buffer::Source::Uri(_) => None,
                });
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let pts: Vec<[f64; 3]> = positions
                    .map(|p| {
                        let v =
                            m * nalgebra::Vector4::new(p[0] as f64, p[1] as f64, p[2] as f64, 1.0);
                        [v.x, v.y, v.z]
                    })
                    .collect();
                let idx: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..pts.len() as u32).collect(),
                };
                let corners: Vec<[u32; 3]> = match prim.mode() {
                    gltf::mesh::Mode::Triangles => idx.as_chunks::<3>().0.to_vec(),
                    gltf::mesh::Mode::TriangleStrip => (2..idx.len())
                        .map(|k| {
                            if k % 2 == 0 {
                                [idx[k - 2], idx[k - 1], idx[k]]
                            } else {
                                [idx[k - 1], idx[k - 2], idx[k]]
                            }
                        })
                        .collect(),
                    gltf::mesh::Mode::TriangleFan => (2..idx.len())
                        .map(|k| [idx[0], idx[k - 1], idx[k]])
                        .collect(),
                    _ => continue,
                };
                let triangles: Vec<Triangle> = corners
                    .iter()
                    .filter_map(|t| {
                        let p = |i: u32| pts.get(i as usize).copied();
                        Some([p(t[0])?, p(t[1])?, p(t[2])?])
                    })
                    .collect();
                if triangles.is_empty() {
                    continue;
                }
                let color = prim
                    .material()
                    .index()
                    .map(|_| prim.material().pbr_metallic_roughness().base_color_factor());
                out.push(GlbMesh {
                    name: name.clone(),
                    triangles,
                    color,
                });
            }
        }
        for child in node.children().collect::<Vec<_>>().into_iter().rev() {
            stack.push((child, m));
        }
    }
    if out.is_empty() {
        return Err("the file has no triangle meshes".into());
    }
    Ok(out)
}

/// A part to write: its name, mesh (in the file's units and orientation) and linear colour.
pub struct GlbPart<'a> {
    pub name: &'a str,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub color: [f32; 4],
}

/// A `.glb` file of `parts`, each a node with its mesh and a material of its colour (parts of
/// one colour share one).
pub fn write(parts: &[GlbPart], generator: &str) -> Vec<u8> {
    use serde_json::{Value, json};
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();
    // A buffer view of `data` (4-byte aligned), and an accessor of it.
    let mut push = |bin: &mut Vec<u8>, data: &[u8], target: u32, accessor: Value| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.push(json!({
            "buffer": 0,
            "byteOffset": bin.len(),
            "byteLength": data.len(),
            "target": target,
        }));
        bin.extend_from_slice(data);
        let mut a = accessor;
        a["bufferView"] = json!(views.len() - 1);
        accessors.push(a);
        accessors.len() - 1
    };
    for part in parts {
        if part.positions.is_empty() || part.indices.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &part.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let floats = |v: &[[f32; 3]]| -> Vec<u8> {
            v.iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
                .collect()
        };
        const ARRAY_BUFFER: u32 = 34962;
        const ELEMENT_ARRAY_BUFFER: u32 = 34963;
        let position = push(
            &mut bin,
            &floats(&part.positions),
            ARRAY_BUFFER,
            json!({
                "componentType": 5126,
                "count": part.positions.len(),
                "type": "VEC3",
                "min": lo,
                "max": hi,
            }),
        );
        let mut attributes = json!({ "POSITION": position });
        if part.normals.len() == part.positions.len() {
            let normal = push(
                &mut bin,
                &floats(&part.normals),
                ARRAY_BUFFER,
                json!({ "componentType": 5126, "count": part.normals.len(), "type": "VEC3" }),
            );
            attributes["NORMAL"] = json!(normal);
        }
        let index_bytes: Vec<u8> = part.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let indices = push(
            &mut bin,
            &index_bytes,
            ELEMENT_ARRAY_BUFFER,
            json!({ "componentType": 5125, "count": part.indices.len(), "type": "SCALAR" }),
        );
        let material = match colors.iter().position(|c| *c == part.color) {
            Some(k) => k,
            None => {
                let mut m = json!({
                    "name": format!("Colour {}", colors.len() + 1),
                    "pbrMetallicRoughness": {
                        "baseColorFactor": part.color,
                        "metallicFactor": 0.0,
                        "roughnessFactor": 0.5,
                    },
                });
                if part.color[3] < 1.0 {
                    m["alphaMode"] = json!("BLEND");
                }
                materials.push(m);
                colors.push(part.color);
                colors.len() - 1
            }
        };
        meshes.push(json!({
            "name": part.name,
            "primitives": [{ "attributes": attributes, "indices": indices, "material": material }],
        }));
        nodes.push(json!({ "name": part.name, "mesh": meshes.len() - 1 }));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let mut root = json!({
        "asset": { "version": "2.0", "generator": generator },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
    });
    if !bin.is_empty() {
        root["buffers"] = json!([{ "byteLength": bin.len() }]);
    }
    let mut text = serde_json::to_vec(&root).unwrap_or_default();
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

/// An sRGB channel (0–1) as linear light.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear channel (0–1) as sRGB.
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A glTF base colour (linear RGBA) as an appearance.
pub fn appearance_of(c: [f32; 4]) -> crate::appearance::Appearance {
    let u = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    crate::appearance::Appearance {
        rgb: [
            u(linear_to_srgb(c[0])),
            u(linear_to_srgb(c[1])),
            u(linear_to_srgb(c[2])),
        ],
        alpha: u(c[3]),
    }
}

/// An appearance as a glTF base colour (linear RGBA).
pub fn color_of(a: crate::appearance::Appearance) -> [f32; 4] {
    let f = |x: u8| x as f32 / 255.0;
    [
        srgb_to_linear(f(a.rgb[0])),
        srgb_to_linear(f(a.rgb[1])),
        srgb_to_linear(f(a.rgb[2])),
        f(a.alpha),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit tetrahedron, written and read back: one node, its triangles and colour.
    #[test]
    fn writes_and_reads_back() {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let indices = vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        let part = GlbPart {
            name: "Tet",
            positions,
            normals: Vec::new(),
            indices,
            color: [0.5, 0.25, 0.125, 1.0],
        };
        let bytes = write(&[part], "cadrs test");
        assert_eq!(&bytes[..4], b"glTF");
        let meshes = read(&bytes).unwrap();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].name, "Tet");
        assert_eq!(meshes[0].triangles.len(), 4);
        assert_eq!(
            meshes[0].triangles[1],
            [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
        );
        assert_eq!(meshes[0].color, Some([0.5, 0.25, 0.125, 1.0]));
    }

    /// Colours survive the trip through linear light.
    #[test]
    fn colours_round_trip() {
        let a = crate::appearance::Appearance {
            rgb: [12, 128, 250],
            alpha: 255,
        };
        assert_eq!(appearance_of(color_of(a)), a);
    }
}
