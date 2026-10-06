//! VRML 2.0 (`.wrl`) models, as KiCad's libraries and tools write them: `Shape`s with a
//! `Material` (`diffuseColor`, `transparency`) and an `IndexedFaceSet` (`coord` points,
//! `coordIndex` polygons ended by −1), possibly inside `Transform`/`Group` nodes, with `DEF`/`USE`
//! sharing. Read into coloured triangle meshes ([`crate::model3d::Mesh`]) in the file's units —
//! KiCad's are 0.1 inch ([`KICAD_UNIT_MM`] mm).

use std::collections::HashMap;

use crate::model3d::{Mesh, Rgb};

/// One VRML unit of a KiCad model, in mm.
pub const KICAD_UNIT_MM: f64 = 2.54;

#[derive(Clone, Debug)]
enum Value {
    Node(Box<Node>),
    Use(String),
    Numbers(Vec<f64>),
    /// A word value (TRUE, FALSE, an enum); not needed for the geometry.
    Word,
    Nodes(Vec<Value>),
}

#[derive(Clone, Debug, Default)]
struct Node {
    kind: String,
    fields: Vec<(String, Value)>,
}

impl Node {
    fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

fn tokens(text: &str) -> Vec<&str> {
    let mut out = vec![];
    for line in text.lines() {
        // Comments run to the end of the line ('#' never appears in KiCad's strings).
        let line = line.split('#').next().unwrap_or("");
        let mut start = None;
        for (i, c) in line.char_indices() {
            let sep = c.is_whitespace() || c == ',';
            let punct = matches!(c, '{' | '}' | '[' | ']');
            if sep || punct {
                if let Some(s) = start.take() {
                    out.push(&line[s..i]);
                }
                if punct {
                    out.push(&line[i..i + 1]);
                }
            } else if start.is_none() {
                start = Some(i);
            }
        }
        if let Some(s) = start {
            out.push(&line[s..]);
        }
    }
    out
}

struct Parser<'a> {
    t: Vec<&'a str>,
    i: usize,
    defs: HashMap<String, Value>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.t.get(self.i).copied()
    }

    fn next(&mut self) -> Option<&'a str> {
        let t = self.peek();
        self.i += 1;
        t
    }

    /// A node: `Kind { field value … }` (the kind already read).
    fn node(&mut self, kind: &str) -> Node {
        let mut n = Node { kind: kind.to_string(), fields: vec![] };
        if self.peek() != Some("{") {
            return n;
        }
        self.i += 1;
        while let Some(t) = self.next() {
            if t == "}" {
                break;
            }
            let v = self.value();
            n.fields.push((t.to_string(), v));
        }
        n
    }

    fn value(&mut self) -> Value {
        match self.peek() {
            Some("DEF") => {
                self.i += 1;
                let name = self.next().unwrap_or("").to_string();
                let v = self.value();
                self.defs.insert(name, v.clone());
                v
            }
            Some("USE") => {
                self.i += 1;
                Value::Use(self.next().unwrap_or("").to_string())
            }
            Some("[") => {
                self.i += 1;
                let mut nums = vec![];
                let mut nodes = vec![];
                while let Some(t) = self.peek() {
                    if t == "]" {
                        self.i += 1;
                        break;
                    }
                    if let Ok(x) = t.parse::<f64>() {
                        nums.push(x);
                        self.i += 1;
                    } else {
                        nodes.push(self.value());
                    }
                }
                if nodes.is_empty() { Value::Numbers(nums) } else { Value::Nodes(nodes) }
            }
            Some(t) if t.parse::<f64>().is_ok() => {
                let mut nums = vec![];
                while let Some(x) = self.peek().and_then(|t| t.parse::<f64>().ok()) {
                    nums.push(x);
                    self.i += 1;
                }
                Value::Numbers(nums)
            }
            Some(t) if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && self.t.get(self.i + 1) == Some(&"{") => {
                self.i += 1;
                Value::Node(Box::new(self.node(t)))
            }
            Some(_) => {
                self.i += 1;
                Value::Word
            }
            None => Value::Word,
        }
    }

    fn resolve<'v>(&'v self, v: &'v Value) -> &'v Value {
        match v {
            Value::Use(name) => self.defs.get(name).unwrap_or(v),
            _ => v,
        }
    }
}

/// A 4×3 affine transform (column-major 3×3 plus a translation).
#[derive(Clone, Copy)]
struct Affine {
    m: [[f64; 3]; 3],
    t: [f64; 3],
}

impl Affine {
    const ID: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let mut out = self.t;
        for (r, o) in out.iter_mut().enumerate() {
            *o += self.m[r][0] * p[0] + self.m[r][1] * p[1] + self.m[r][2] * p[2];
        }
        out
    }

    fn then(&self, inner: &Affine) -> Affine {
        // self ∘ inner.
        let mut m = [[0.0; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, x) in row.iter_mut().enumerate() {
                *x = (0..3).map(|k| self.m[r][k] * inner.m[k][c]).sum();
            }
        }
        Affine { m, t: self.apply(inner.t) }
    }

    /// A `Transform` node's translation · rotation (axis, angle) · scale.
    fn of(n: &Node) -> Affine {
        let nums = |f: &str| match n.get(f) {
            Some(Value::Numbers(v)) => Some(v.clone()),
            _ => None,
        };
        let t = nums("translation").filter(|v| v.len() == 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
        let s = nums("scale").filter(|v| v.len() == 3).map_or([1.0; 3], |v| [v[0], v[1], v[2]]);
        let r = nums("rotation").filter(|v| v.len() == 4);
        let mut rot = Affine::ID.m;
        if let Some(r) = r {
            let len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
            if len > 0.0 {
                let (x, y, z) = (r[0] / len, r[1] / len, r[2] / len);
                let (c, sn) = (r[3].cos(), r[3].sin());
                let k = 1.0 - c;
                rot = [[c + x * x * k, x * y * k - z * sn, x * z * k + y * sn], [y * x * k + z * sn, c + y * y * k, y * z * k - x * sn], [z * x * k - y * sn, z * y * k + x * sn, c + z * z * k]];
            }
        }
        let mut m = [[0.0; 3]; 3];
        for (rr, row) in m.iter_mut().enumerate() {
            for (cc, v) in row.iter_mut().enumerate() {
                *v = rot[rr][cc] * s[cc];
            }
        }
        Affine { m, t }
    }
}

fn collect(p: &Parser, v: &Value, at: Affine, out: &mut Vec<Mesh>) {
    let v = p.resolve(v);
    match v {
        Value::Nodes(list) => list.iter().for_each(|x| collect(p, x, at, out)),
        Value::Node(n) => match n.kind.as_str() {
            "Transform" => {
                let inner = at.then(&Affine::of(n));
                if let Some(c) = n.get("children") {
                    collect(p, c, inner, out);
                }
            }
            "Group" | "Collision" | "Anchor" | "Billboard" => {
                if let Some(c) = n.get("children") {
                    collect(p, c, at, out);
                }
            }
            "Shape" => shape(p, n, at, out),
            _ => {}
        },
        _ => {}
    }
}

fn node<'v>(p: &'v Parser, v: Option<&'v Value>) -> Option<&'v Node> {
    match p.resolve(v?) {
        Value::Node(n) => Some(n),
        _ => None,
    }
}

fn shape(p: &Parser, n: &Node, at: Affine, out: &mut Vec<Mesh>) {
    let material = node(p, n.get("appearance")).and_then(|a| node(p, a.get("material")));
    let color: Rgb = material
        .and_then(|m| match m.get("diffuseColor") {
            Some(Value::Numbers(c)) if c.len() == 3 => Some([0, 1, 2].map(|i| (c[i].clamp(0.0, 1.0) * 255.0).round() as u8)),
            _ => None,
        })
        .unwrap_or([180, 180, 180]);
    let Some(geom) = node(p, n.get("geometry")) else { return };
    if geom.kind != "IndexedFaceSet" {
        return;
    }
    let Some(coord) = node(p, geom.get("coord")) else { return };
    let Some(Value::Numbers(pts)) = coord.get("point").map(|v| p.resolve(v)) else { return };
    let Some(Value::Numbers(idx)) = geom.get("coordIndex").map(|v| p.resolve(v)) else { return };
    let points: Vec<[f64; 3]> = pts.chunks_exact(3).map(|c| at.apply([c[0], c[1], c[2]])).collect();
    let i = match out.iter().position(|m| m.color == color) {
        Some(i) => i,
        None => {
            out.push(Mesh { color, ..Default::default() });
            out.len() - 1
        }
    };
    let m = &mut out[i];
    let mut face: Vec<usize> = vec![];
    let flush = |face: &mut Vec<usize>, m: &mut Mesh| {
        let f: Vec<[f64; 3]> = face.iter().filter_map(|&k| points.get(k).copied()).collect();
        face.clear();
        if f.len() < 3 {
            return;
        }
        // A fan; the face's normal from Newell's method.
        let mut nrm = [0.0f64; 3];
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            nrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
            nrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
            nrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let l = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt().max(1e-12);
        let n32 = [(nrm[0] / l) as f32, (nrm[1] / l) as f32, (nrm[2] / l) as f32];
        for k in 1..f.len() - 1 {
            for q in [f[0], f[k], f[k + 1]] {
                m.indices.push(m.positions.len() as u32);
                m.positions.push([q[0] as f32, q[1] as f32, q[2] as f32]);
                m.normals.push(n32);
            }
        }
    };
    for &k in idx {
        if k < 0.0 {
            flush(&mut face, m);
        } else {
            face.push(k as usize);
        }
    }
    flush(&mut face, m);
}

/// The meshes of a VRML file's text, one per colour, in the file's units.
pub fn read(text: &str) -> Result<Vec<Mesh>, String> {
    if !text.trim_start().starts_with("#VRML V2.0") {
        return Err("Not a VRML 2.0 file".into());
    }
    let mut p = Parser { t: tokens(text), i: 0, defs: HashMap::new() };
    let mut top = vec![];
    while let Some(t) = p.peek() {
        let v = p.value();
        if t == "}" || t == "]" {
            continue;
        }
        top.push(v);
    }
    let mut out = vec![];
    for v in &top {
        collect(&p, v, Affine::ID, &mut out);
    }
    out.retain(|m| !m.indices.is_empty());
    if out.is_empty() {
        return Err("No shapes in the VRML file".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shapes_materials_and_transforms() {
        let text = "#VRML V2.0 utf8\n\
            Shape { appearance Appearance { material DEF red Material { diffuseColor 1 0 0 } }\n\
              geometry IndexedFaceSet { coord DEF c Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] } coordIndex [ 0, 1, 2, 3, -1 ] } }\n\
            Transform { translation 0 0 2 children [\n\
              Shape { appearance Appearance { material USE red } geometry IndexedFaceSet { coord USE c coordIndex [ 0 1 2 -1 ] } } ] }\n\
            Shape { appearance Appearance { material Material { diffuseColor 0 0 1 } } # blue\n\
              geometry IndexedFaceSet { coord Coordinate { point [0 0 0 1 0 0 0 1 0] } coordIndex [0 1 2] } }";
        let m = read(text).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].color, [255, 0, 0]);
        // A quad (two triangles) and a triangle moved up by 2.
        assert_eq!(m[0].indices.len(), 9);
        assert_eq!(m[0].positions[6][2], 2.0);
        assert_eq!(m[0].normals[0], [0.0, 0.0, 1.0]);
        assert_eq!(m[1].color, [0, 0, 255]);
        assert!(read("not vrml").is_err());
    }

    #[test]
    fn reads_the_test_fixture() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/eda/test_part.wrl")).unwrap();
        let m = read(&text).unwrap();
        // The body and its two ends (one shared geometry, moved apart).
        assert_eq!(m.len(), 2);
        assert_eq!(m.iter().map(|m| m.indices.len() / 3).sum::<usize>(), 36);
        let xs: Vec<f32> = m[1].positions.iter().map(|p| p[0]).collect();
        let (lo, hi) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
        assert!((hi - 0.7874).abs() < 1e-3 && (lo + 0.7874).abs() < 1e-3, "{lo} {hi}");
    }

    /// The target project's LoRa module model, when it is on this machine.
    #[test]
    fn reads_an_easyeda_model() {
        let path = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("work/desk_power_monitor/hardware/desk_power_monitor_mini32_lora/library/easyeda2kicad.3dshapes/WIRELM-SMD_RA-01SH.wrl");
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let m = read(&text).unwrap();
        let tris: usize = m.iter().map(|m| m.indices.len() / 3).sum();
        assert!(tris > 100, "{tris}");
        // 16 mm wide: ±3.15 units of 2.54 mm.
        let xmax = m.iter().flat_map(|m| m.positions.iter()).map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!((xmax as f64 * KICAD_UNIT_MM - 8.0).abs() < 0.2, "{xmax}");
    }
}
