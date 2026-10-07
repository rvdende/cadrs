//! Generated 3D models: a part's body as coloured boxes and cylinders in footprint coordinates
//! (mm, X/Y as the footprint, Z up from the board surface). The footprint generators in
//! the built-in libraries make one with every footprint, so a board shows real-looking parts with
//! no model files. [`mesh`] turns a body into triangles, one group per colour.

use serde::{Deserialize, Serialize};

/// An sRGB colour.
pub type Rgb = [u8; 3];

/// Colours parts are made of.
pub mod colors {
    use super::Rgb;
    /// Moulded plastic: IC packages, connector housings.
    pub const PLASTIC: Rgb = [38, 38, 42];
    /// Tinned terminals and leads.
    pub const TIN: Rgb = [196, 198, 202];
    /// Gold-plated pins.
    pub const GOLD: Rgb = [214, 175, 72];
    /// Ceramic capacitor body.
    pub const CERAMIC: Rgb = [127, 94, 78];
    /// Thick-film resistor body (its top).
    pub const RESISTOR: Rgb = [30, 30, 30];
    /// Inductor / ferrite body.
    pub const FERRITE: Rgb = [70, 70, 74];
    /// White plastic: LED packages, JST housings.
    pub const WHITE: Rgb = [235, 235, 228];
    /// LED lens.
    pub const LED_RED: Rgb = [220, 40, 40];
    /// Electrolytic can and its sleeve.
    pub const ALUMINIUM: Rgb = [200, 202, 206];
    pub const SLEEVE: Rgb = [30, 50, 120];
    /// Axial resistor body.
    pub const AXIAL: Rgb = [210, 190, 140];
    /// A module's shield can.
    pub const SHIELD: Rgb = [180, 182, 186];
    /// A module's own board.
    pub const PCB: Rgb = [30, 110, 50];
}

/// An axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// A primitive solid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Solid {
    /// An axis-aligned box between two corners.
    Box { min: [f64; 3], max: [f64; 3] },
    /// A cylinder from `base` along `axis` (positive direction).
    Cylinder { base: [f64; 3], axis: Axis, radius: f64, length: f64 },
}

/// One coloured primitive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub solid: Solid,
    pub color: Rgb,
}

/// A generated model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub parts: Vec<Part>,
}

impl Body {
    pub fn cuboid(&mut self, min: [f64; 3], max: [f64; 3], color: Rgb) -> &mut Self {
        self.parts.push(Part { solid: Solid::Box { min, max }, color });
        self
    }

    pub fn cylinder(&mut self, base: [f64; 3], axis: Axis, radius: f64, length: f64, color: Rgb) -> &mut Self {
        self.parts.push(Part { solid: Solid::Cylinder { base, axis, radius, length }, color });
        self
    }

    /// The box around every part (mm), if there is one.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut out: Option<([f64; 3], [f64; 3])> = None;
        for p in &self.parts {
            let (lo, hi) = match p.solid {
                Solid::Box { min, max } => (min, max),
                Solid::Cylinder { base, axis, radius, length } => {
                    let mut lo = [base[0] - radius, base[1] - radius, base[2] - radius];
                    let mut hi = [base[0] + radius, base[1] + radius, base[2] + radius];
                    let k = axis as usize;
                    lo[k] = base[k];
                    hi[k] = base[k] + length;
                    (lo, hi)
                }
            };
            out = Some(match out {
                None => (lo, hi),
                Some((a, b)) => ([a[0].min(lo[0]), a[1].min(lo[1]), a[2].min(lo[2])], [b[0].max(hi[0]), b[1].max(hi[1]), b[2].max(hi[2])]),
            });
        }
        out
    }
}

/// Triangles of one colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub color: Rgb,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Outline polylines: a box's 12 edges, a cylinder's two end circles.
    pub edges: Vec<Vec<[f32; 3]>>,
}

impl Mesh {
    fn quad(&mut self, c: [[f64; 3]; 4], n: [f64; 3]) {
        let i = self.positions.len() as u32;
        for p in c {
            self.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            self.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
        }
        self.indices.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
    }
}

/// Segments around a cylinder.
const SEGMENTS: usize = 20;

/// Maps a cylinder's local frame (u, v across, w along) to x, y, z.
fn frame(axis: Axis, u: f64, v: f64, w: f64) -> [f64; 3] {
    match axis {
        Axis::X => [w, u, v],
        Axis::Y => [v, w, u],
        Axis::Z => [u, v, w],
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn solid_into(m: &mut Mesh, s: &Solid) {
    match *s {
        Solid::Box { min: a, max: b } => {
            let p = |x: f64, y: f64, z: f64| [x, y, z];
            m.quad([p(a[0], a[1], b[2]), p(b[0], a[1], b[2]), p(b[0], b[1], b[2]), p(a[0], b[1], b[2])], [0.0, 0.0, 1.0]);
            m.quad([p(a[0], b[1], a[2]), p(b[0], b[1], a[2]), p(b[0], a[1], a[2]), p(a[0], a[1], a[2])], [0.0, 0.0, -1.0]);
            m.quad([p(a[0], a[1], a[2]), p(b[0], a[1], a[2]), p(b[0], a[1], b[2]), p(a[0], a[1], b[2])], [0.0, -1.0, 0.0]);
            m.quad([p(b[0], b[1], a[2]), p(a[0], b[1], a[2]), p(a[0], b[1], b[2]), p(b[0], b[1], b[2])], [0.0, 1.0, 0.0]);
            m.quad([p(a[0], b[1], a[2]), p(a[0], a[1], a[2]), p(a[0], a[1], b[2]), p(a[0], b[1], b[2])], [-1.0, 0.0, 0.0]);
            m.quad([p(b[0], a[1], a[2]), p(b[0], b[1], a[2]), p(b[0], b[1], b[2]), p(b[0], a[1], b[2])], [1.0, 0.0, 0.0]);
            let f = |x: f64, y: f64, z: f64| [x as f32, y as f32, z as f32];
            for z in [a[2], b[2]] {
                m.edges.push(vec![f(a[0], a[1], z), f(b[0], a[1], z), f(b[0], b[1], z), f(a[0], b[1], z), f(a[0], a[1], z)]);
            }
            for (x, y) in [(a[0], a[1]), (b[0], a[1]), (b[0], b[1]), (a[0], b[1])] {
                m.edges.push(vec![f(x, y, a[2]), f(x, y, b[2])]);
            }
        }
        Solid::Cylinder { base, axis, radius: r, length: l } => {
            let ring = |k: usize| {
                let t = std::f64::consts::TAU * k as f64 / SEGMENTS as f64;
                (t.cos(), t.sin())
            };
            for k in 0..SEGMENTS {
                let ((c0, s0), (c1, s1)) = (ring(k), ring(k + 1));
                // The side: normals point out (one per edge pair, smooth enough at 20 sides).
                let i = m.positions.len() as u32;
                for (c, s, w) in [(c0, s0, 0.0), (c1, s1, 0.0), (c1, s1, l), (c0, s0, l)] {
                    let p = add(base, frame(axis, r * c, r * s, w));
                    let n = frame(axis, c, s, 0.0);
                    m.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                    m.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
                }
                m.indices.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
                // The caps: fans from the centre.
                for (w, dir) in [(0.0, -1.0), (l, 1.0)] {
                    let n = frame(axis, 0.0, 0.0, dir);
                    let i = m.positions.len() as u32;
                    for (u, v) in [(0.0, 0.0), (r * c0, r * s0), (r * c1, r * s1)] {
                        let p = add(base, frame(axis, u, v, w));
                        m.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                        m.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
                    }
                    if dir > 0.0 { m.indices.extend([i, i + 1, i + 2]) } else { m.indices.extend([i, i + 2, i + 1]) }
                }
            }
            for w in [0.0, l] {
                let ring: Vec<[f32; 3]> = (0..=SEGMENTS)
                    .map(|k| {
                        let (c, s) = ring(k);
                        let q = add(base, frame(axis, r * c, r * s, w));
                        [q[0] as f32, q[1] as f32, q[2] as f32]
                    })
                    .collect();
                m.edges.push(ring);
            }
        }
    }
}

/// The body's triangles, one mesh per colour (in the order colours first appear).
pub fn mesh(body: &Body) -> Vec<Mesh> {
    let mut out: Vec<Mesh> = vec![];
    for p in &body.parts {
        let i = match out.iter().position(|m| m.color == p.color) {
            Some(i) => i,
            None => {
                out.push(Mesh { color: p.color, ..Default::default() });
                out.len() - 1
            }
        };
        solid_into(&mut out[i], &p.solid);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Bodies of common packages (the footprint generators call these)

/// A two-terminal chip (resistor, capacitor, inductor): `l` × `w` × `h`, terminals `t` long at
/// each end along X, centred on the origin.
pub fn chip(l: f64, w: f64, h: f64, t: f64, body: Rgb) -> Body {
    let mut b = Body::default();
    let (x, y) = (l / 2.0, w / 2.0);
    b.cuboid([-x + t, -y, 0.0], [x - t, y, h], body);
    b.cuboid([-x, -y, 0.0], [-x + t, y, h], colors::TIN);
    b.cuboid([x - t, -y, 0.0], [x, y, h], colors::TIN);
    b
}

/// A chip LED: a white package with a coloured lens on top.
pub fn chip_led(l: f64, w: f64, h: f64, t: f64) -> Body {
    let mut b = chip(l, w, h * 0.6, t, colors::WHITE);
    b.cuboid([-l / 2.0 + t, -w / 2.0 * 0.8, h * 0.6], [l / 2.0 - t, w / 2.0 * 0.8, h], colors::LED_RED);
    b
}

/// A moulded body `l` × `w` × `h` raised `standoff` off the board, with gull-wing leads:
/// each lead from (`x`, `y`) on the pad out to the body edge, `lw` wide.
pub fn gull_wing(l: f64, w: f64, h: f64, standoff: f64, leads: &[(f64, f64)], lw: f64, lead_h: f64) -> Body {
    let mut b = Body::default();
    b.cuboid([-l / 2.0, -w / 2.0, standoff], [l / 2.0, w / 2.0, h], colors::PLASTIC);
    for &(x, y) in leads {
        // Leads leave the body's long sides (|x| beyond the body) or its ends (|y| beyond).
        if x.abs() > l / 2.0 - 1e-9 {
            let (x0, x1) = if x < 0.0 { (x, -l / 2.0) } else { (l / 2.0, x) };
            b.cuboid([x0.min(x1), y - lw / 2.0, 0.0], [x0.max(x1), y + lw / 2.0, lead_h], colors::TIN);
        } else {
            let (y0, y1) = if y < 0.0 { (y, -w / 2.0) } else { (w / 2.0, y) };
            b.cuboid([x - lw / 2.0, y0.min(y1), 0.0], [x + lw / 2.0, y0.max(y1), lead_h], colors::TIN);
        }
    }
    b
}

/// A no-lead package (QFN/DFN): a square body with terminal strips under its edges.
pub fn no_lead(l: f64, w: f64, h: f64, terminals: &[(f64, f64, f64, f64)]) -> Body {
    let mut b = Body::default();
    b.cuboid([-l / 2.0, -w / 2.0, 0.02], [l / 2.0, w / 2.0, h], colors::PLASTIC);
    for &(x, y, tw, th) in terminals {
        b.cuboid([x - tw / 2.0, y - th / 2.0, 0.0], [x + tw / 2.0, y + th / 2.0, 0.03], colors::TIN);
    }
    b
}

/// A pin header: a black spacer per pin (`pitch` square, 2.5 mm high) and a gold square pin
/// 0.64 mm wide from 3 mm below the board to `pin_top` above.
pub fn pin_header(pins: &[(f64, f64)], pitch: f64, pin_top: f64) -> Body {
    let mut b = Body::default();
    for &(x, y) in pins {
        let s = pitch / 2.0 - 0.02;
        b.cuboid([x - s, y - s, 0.0], [x + s, y + s, 2.5], colors::PLASTIC);
    }
    for &(x, y) in pins {
        b.cuboid([x - 0.32, y - 0.32, -3.0], [x + 0.32, y + 0.32, pin_top], colors::GOLD);
    }
    b
}

/// A vertical shrouded connector (JST style): a housing box around the pins.
pub fn housing(min: [f64; 2], max: [f64; 2], h: f64, pins: &[(f64, f64)], color: Rgb) -> Body {
    let mut b = Body::default();
    b.cuboid([min[0], min[1], 0.0], [max[0], max[1], h], color);
    for &(x, y) in pins {
        b.cuboid([x - 0.32, y - 0.32, -3.0], [x + 0.32, y + 0.32, h - 1.0], colors::TIN);
    }
    b
}

/// An upright cylinder (electrolytic can, crystal can, LED) at `c`.
pub fn can(c: [f64; 2], radius: f64, h: f64, color: Rgb) -> Body {
    let mut b = Body::default();
    b.cylinder([c[0], c[1], 0.0], Axis::Z, radius, h, color);
    b
}

/// An axial part lying along X from `x0` to `x1` with leads to the pads at 0 and `pitch`.
pub fn axial(pitch: f64, length: f64, diameter: f64, color: Rgb) -> Body {
    let mut b = Body::default();
    let r = diameter / 2.0;
    let x0 = (pitch - length) / 2.0;
    b.cylinder([x0, 0.0, r], Axis::X, r, length, color);
    b.cuboid([0.0 - 0.3, -0.3, -3.0], [0.3, 0.3, r], colors::TIN);
    b.cuboid([pitch - 0.3, -0.3, -3.0], [pitch + 0.3, 0.3, r], colors::TIN);
    b.cuboid([0.0, -0.3, r - 0.3], [x0, 0.3, r + 0.3], colors::TIN);
    b.cuboid([x0 + length, -0.3, r - 0.3], [pitch, 0.3, r + 0.3], colors::TIN);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meshes_close_and_group_by_colour() {
        let b = chip(2.0, 1.25, 0.6, 0.4, colors::CERAMIC);
        let m = mesh(&b);
        assert_eq!(m.len(), 2, "body and tin");
        // Three boxes, 6 faces × 2 triangles each.
        assert_eq!(m.iter().map(|m| m.indices.len() / 3).sum::<usize>(), 36);
        let (lo, hi) = b.bounds().unwrap();
        assert_eq!((lo, hi), ([-1.0, -0.625, 0.0], [1.0, 0.625, 0.6]));
        let c = can([0.0, 0.0], 2.5, 5.0, colors::ALUMINIUM);
        assert_eq!(mesh(&c)[0].indices.len() / 3, SEGMENTS * 4);
        assert_eq!(c.bounds().unwrap().1[2], 5.0);
    }
}
