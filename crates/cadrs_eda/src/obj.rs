//! Wavefront OBJ models, as EasyEDA's model server sends them (the 3D models of the JLCPCB
//! catalogue's parts): `v` points, `f` polygons, and colours from materials (`newmtl`, `Kd`),
//! which EasyEDA writes into the file itself (`usemtl` picks one). Read into coloured triangle
//! meshes ([`crate::model3d::Mesh`]), one per material, in mm.

use crate::model3d::Mesh;
use std::collections::HashMap;

/// The model's meshes, one per colour used.
pub fn read(text: &str) -> Result<Vec<Mesh>, String> {
    let mut points: Vec<[f32; 3]> = vec![];
    let mut colors: HashMap<String, [u8; 3]> = HashMap::new();
    let mut current_mtl: Option<String> = None;
    let mut in_mtl: Option<String> = None;
    // Faces gathered per material.
    let mut faces: Vec<(Option<String>, Vec<Vec<usize>>)> = vec![];
    let num = |s: &str| s.parse::<f32>().map_err(|_| format!("bad number {s:?}"));
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f32> = it.take(3).map(num).collect::<Result<_, _>>()?;
                if c.len() == 3 {
                    points.push([c[0], c[1], c[2]]);
                }
            }
            Some("newmtl") => in_mtl = it.next().map(str::to_string),
            Some("Kd") => {
                if let Some(m) = &in_mtl {
                    let c: Vec<f32> = it.take(3).filter_map(|s| s.parse().ok()).collect();
                    if c.len() == 3 {
                        // `Kd` is linear light; the meshes carry sRGB.
                        let b = |v: f32| {
                            let v = v.clamp(0.0, 1.0);
                            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
                            (s * 255.0).round() as u8
                        };
                        colors.insert(m.clone(), [b(c[0]), b(c[1]), b(c[2])]);
                    }
                }
            }
            Some("endmtl") => in_mtl = None,
            Some("usemtl") => current_mtl = it.next().map(str::to_string),
            Some("f") => {
                // `i`, `i/t`, `i//n`, `i/t/n`; 1-based, or negative from the end.
                let idx: Vec<usize> = it
                    .filter_map(|v| v.split('/').next()?.parse::<i64>().ok())
                    .filter_map(|i| if i > 0 { Some(i as usize - 1) } else if i < 0 { points.len().checked_sub((-i) as usize) } else { None })
                    .collect();
                if idx.len() >= 3 {
                    match faces.iter_mut().find(|(m, _)| *m == current_mtl) {
                        Some((_, f)) => f.push(idx),
                        None => faces.push((current_mtl.clone(), vec![idx])),
                    }
                }
            }
            _ => {}
        }
    }
    if points.is_empty() {
        return Err("no points: not an OBJ model".into());
    }
    let mut out = vec![];
    for (mtl, polys) in faces {
        let color = mtl.as_ref().and_then(|m| colors.get(m)).copied().unwrap_or([180, 180, 180]);
        let mut m = Mesh { color, ..Default::default() };
        for poly in polys {
            if poly.iter().any(|&i| i >= points.len()) {
                continue;
            }
            // A fan, flat-shaded.
            for k in 1..poly.len() - 1 {
                let tri = [points[poly[0]], points[poly[k]], points[poly[k + 1]]];
                let (a, b, c) = (tri[0], tri[1], tri[2]);
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                let base = m.positions.len() as u32;
                for p in tri {
                    m.positions.push(p);
                    m.normals.push([n[0] / l, n[1] / l, n[2] / l]);
                }
                m.indices.extend([base, base + 1, base + 2]);
            }
        }
        if !m.indices.is_empty() {
            out.push(m);
        }
    }
    if out.is_empty() {
        return Err("no faces in the OBJ model".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn faces_by_material_colour() {
        let obj = "newmtl 1\nKd 1.0 1.0 1.0\nendmtl\nnewmtl 2\nKd 0.65 0.52 0.0\nendmtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nusemtl 1\nf 1// 2// 3// 4//\nusemtl 2\nf 1 2 3\n";
        let m = super::read(obj).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].color, [255, 255, 255]);
        assert_eq!(m[0].indices.len(), 6);
        // 0.65 linear is 0.83 in sRGB.
        assert_eq!(m[1].color, [211, 191, 0]);
        assert_eq!(m[0].normals[0], [0.0, 0.0, 1.0]);
    }
}
