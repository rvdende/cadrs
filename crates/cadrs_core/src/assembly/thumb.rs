//! Small isometric renders of parts for the Insert dialog's tree (A2.3, `ex1-step3.png`): the
//! part shaded as the viewport shades it (a head light from the viewer's upper right), with
//! dark edges, on a transparent ground. A plain software z-buffer, rendered at twice the size
//! and filtered down, so it needs no GPU and is the same on every machine.

use image::{Rgba, RgbaImage};

use crate::solid::Solid;

/// The isometric view (Onshape's: from front, right and top).
const RIGHT: [f64; 3] = [std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2, 0.0];
const UP: [f64; 3] = [-0.408_248_290_463_863, 0.408_248_290_463_863, 0.816_496_580_927_726];
const BACK: [f64; 3] = [0.577_350_269_189_626, -0.577_350_269_189_626, 0.577_350_269_189_626];

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Renders `solids` (each with its sRGB base colour) into a `size`×`size` image.
pub fn render(solids: &[(&Solid, [u8; 3])], size: u32) -> RgbaImage {
    let ss = 2;
    let n = (size * ss) as usize;
    // Screen coordinates of every point: (right, up, depth toward the viewer).
    let proj = |p: [f64; 3]| (dot(p, RIGHT), dot(p, UP), dot(p, BACK));
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (s, _) in solids {
        for p in &s.positions {
            let (x, y, _) = proj(*p);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    let mut out = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    if x0 > x1 {
        return out;
    }
    let margin = 0.08 * n as f64;
    let scale = (n as f64 - 2.0 * margin) / (x1 - x0).max(y1 - y0).max(1e-9);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let to_px = |p: [f64; 3]| {
        let (x, y, z) = proj(p);
        (n as f64 / 2.0 + (x - cx) * scale, n as f64 / 2.0 - (y - cy) * scale, z * scale)
    };
    let mut depth = vec![f64::MIN; n * n];
    let mut color = vec![[0f32; 4]; n * n];
    for (s, base) in solids {
        for tri in s.indices.chunks_exact(3) {
            let v: Vec<(f64, f64, f64)> = tri.iter().map(|i| to_px(s.positions[*i as usize])).collect();
            let nrm = s.normals.get(tri[0] as usize).copied().unwrap_or([0.0, 0.0, 1.0]);
            let b = (0.705 + 0.146 * dot(nrm, RIGHT) + 0.185 * dot(nrm, UP) + 0.25 * dot(nrm, BACK)).clamp(0.3, 1.1);
            let c = [
                (base[0] as f64 / 255.0 * b).min(1.0) as f32,
                (base[1] as f64 / 255.0 * b).min(1.0) as f32,
                (base[2] as f64 / 255.0 * b).min(1.0) as f32,
                1.0,
            ];
            let (a, bb, cc) = (v[0], v[1], v[2]);
            let area = (bb.0 - a.0) * (cc.1 - a.1) - (cc.0 - a.0) * (bb.1 - a.1);
            if area.abs() < 1e-12 {
                continue;
            }
            let minx = a.0.min(bb.0).min(cc.0).floor().max(0.0) as usize;
            let maxx = (a.0.max(bb.0).max(cc.0).ceil() as usize).min(n - 1);
            let miny = a.1.min(bb.1).min(cc.1).floor().max(0.0) as usize;
            let maxy = (a.1.max(bb.1).max(cc.1).ceil() as usize).min(n - 1);
            for py in miny..=maxy {
                for px in minx..=maxx {
                    let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                    let w0 = ((bb.0 - x) * (cc.1 - y) - (cc.0 - x) * (bb.1 - y)) / area;
                    let w1 = ((cc.0 - x) * (a.1 - y) - (a.0 - x) * (cc.1 - y)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * a.2 + w1 * bb.2 + w2 * cc.2;
                    let k = py * n + px;
                    if z > depth[k] {
                        depth[k] = z;
                        color[k] = c;
                    }
                }
            }
        }
    }
    // Edges: dark lines where they are not behind a face.
    let edge = [0.16f32, 0.18, 0.2, 1.0];
    for (s, _) in solids {
        for e in &s.edges {
            for w in e.points.windows(2) {
                let (a, b) = (to_px(w[0]), to_px(w[1]));
                let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).max(1);
                for i in 0..=steps {
                    let t = i as f64 / steps as f64;
                    let (x, y, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
                    if x < 0.0 || y < 0.0 || x >= n as f64 || y >= n as f64 {
                        continue;
                    }
                    let k = y as usize * n + x as usize;
                    if depth[k] == f64::MIN || z >= depth[k] - 0.02 * n as f64 {
                        color[k] = edge;
                    }
                }
            }
        }
    }
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for dy in 0..ss {
                for dx in 0..ss {
                    let c = color[((y * ss + dy) as usize) * n + (x * ss + dx) as usize];
                    for i in 0..3 {
                        acc[i] += c[i] * c[3];
                    }
                    acc[3] += c[3];
                }
            }
            let k = (ss * ss) as f32;
            let a = acc[3] / k;
            let px = if acc[3] > 0.0 {
                [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3], a]
            } else {
                [0.0; 4]
            };
            out.put_pixel(x, y, Rgba(px.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)));
        }
    }
    out
}

/// An orthographic view: the screen's right and up directions and the direction toward the
/// viewer (a right-handed frame).
#[derive(Debug, Clone, Copy)]
struct View {
    right: [f64; 3],
    up: [f64; 3],
    back: [f64; 3],
}

/// The two views of a standard content preview (A19.4, `lesson-standard-content.png`): along
/// the fastener's axis (its Z, from the head side), and from the side with the axis horizontal,
/// head on the left.
const END_VIEW: View = View { right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], back: [0.0, 0.0, 1.0] };
const SIDE_VIEW: View = View { right: [0.0, 0.0, -1.0], up: [1.0, 0.0, 0.0], back: [0.0, -1.0, 0.0] };

/// A fastener's preview as a drawing (P3B.5 judge, `lesson-standard-content.png`): two
/// orthographic line views side by side, the end view and the side view, at one scale, with
/// hidden lines removed (a z-buffer of the faces), silhouettes of curved faces (between
/// rulings whose normals turn away from the viewer) and dark lines on a transparent ground.
pub fn line_views(solid: &Solid, width: u32, height: u32) -> RgbaImage {
    let ss = 3usize;
    let (nw, nh) = (width as usize * ss, height as usize * ss);
    let views = [END_VIEW, SIDE_VIEW];
    let proj = |v: &View, p: [f64; 3]| (dot(p, v.right), dot(p, v.up), dot(p, v.back));
    // Each view's extent, and one scale that fits both side by side.
    let ext: Vec<(f64, f64, f64, f64)> = views
        .iter()
        .map(|v| {
            solid.positions.iter().fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |a, p| {
                let (x, y, _) = proj(v, *p);
                (a.0.min(x), a.1.max(x), a.2.min(y), a.3.max(y))
            })
        })
        .collect();
    let mut out = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    if solid.positions.is_empty() {
        return out;
    }
    let margin = 0.08 * nh as f64;
    let gap = 0.12 * nw as f64;
    let total_w: f64 = ext.iter().map(|e| (e.1 - e.0).max(1e-9)).sum();
    let max_h = ext.iter().map(|e| (e.3 - e.2).max(1e-9)).fold(0.0, f64::max);
    let scale = ((nw as f64 - 2.0 * margin - gap) / total_w).min((nh as f64 - 2.0 * margin) / max_h);
    let used = total_w * scale + gap;
    let mut left = (nw as f64 - used) / 2.0;
    let mut ink = vec![0f32; nw * nh];
    for (v, e) in views.iter().zip(&ext) {
        let cy = (e.2 + e.3) / 2.0;
        let x0 = left;
        let to_px = |p: [f64; 3]| {
            let (x, y, z) = proj(v, p);
            (x0 + (x - e.0) * scale, nh as f64 / 2.0 - (y - cy) * scale, z * scale)
        };
        // The faces' depth.
        let mut depth = vec![f64::MIN; nw * nh];
        for tri in solid.indices.chunks_exact(3) {
            let t: Vec<(f64, f64, f64)> = tri.iter().map(|i| to_px(solid.positions[*i as usize])).collect();
            let (a, b, c) = (t[0], t[1], t[2]);
            let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
            if area.abs() < 1e-12 {
                continue;
            }
            let minx = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
            let maxx = (a.0.max(b.0).max(c.0).ceil().max(0.0) as usize).min(nw - 1);
            let miny = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
            let maxy = (a.1.max(b.1).max(c.1).ceil().max(0.0) as usize).min(nh - 1);
            for py in miny..=maxy {
                for px in minx..=maxx {
                    let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                    let w0 = ((b.0 - x) * (c.1 - y) - (c.0 - x) * (b.1 - y)) / area;
                    let w1 = ((c.0 - x) * (a.1 - y) - (a.0 - x) * (c.1 - y)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                        continue;
                    }
                    let z = w0 * a.2 + w1 * b.2 + w2 * c.2;
                    let k = py * nw + px;
                    depth[k] = depth[k].max(z);
                }
            }
        }
        // The lines: edges, and the silhouettes of curved faces.
        let mut lines: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for ed in &solid.edges {
            for w in ed.points.windows(2) {
                lines.push((w[0], w[1]));
            }
        }
        for w in solid.rulings.windows(2) {
            let (r0, r1) = (&w[0], &w[1]);
            if r0.face != r1.face || r0.run != r1.run {
                continue;
            }
            let (s0, s1) = (dot(r0.normal, v.back), dot(r1.normal, v.back));
            if s0 * s1 > 0.0 || (s0 == 0.0 && s1 == 0.0) {
                continue;
            }
            let t = s0 / (s0 - s1);
            let lerp = |a: [f64; 3], b: [f64; 3]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            lines.push((lerp(r0.start, r1.start), lerp(r0.end, r1.end)));
        }
        let tol = 0.015 * nh as f64;
        let half = ss as f64 * 0.55;
        for (p, q) in lines {
            let (a, b) = (to_px(p), to_px(q));
            let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) * 2.0).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let (x, y, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
                // Visible: not behind a face (the faces it bounds are at its depth).
                let (xi, yi) = (x as isize, y as isize);
                let r = half.ceil() as isize;
                let mut seen = false;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (px, py) = (xi + dx, yi + dy);
                        if px >= 0 && py >= 0 && (px as usize) < nw && (py as usize) < nh {
                            let k = py as usize * nw + px as usize;
                            if depth[k] == f64::MIN || z >= depth[k] - tol {
                                seen = true;
                            }
                        }
                    }
                }
                if !seen {
                    continue;
                }
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (px, py) = (xi + dx, yi + dy);
                        let d = ((px as f64 + 0.5 - x).powi(2) + (py as f64 + 0.5 - y).powi(2)).sqrt();
                        if d <= half && px >= 0 && py >= 0 && (px as usize) < nw && (py as usize) < nh {
                            ink[py as usize * nw + px as usize] = 1.0;
                        }
                    }
                }
            }
        }
        left += (e.1 - e.0).max(1e-9) * scale + gap;
    }
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut a = 0.0;
            for dy in 0..ss {
                for dx in 0..ss {
                    a += ink[(y * ss + dy) * nw + x * ss + dx];
                }
            }
            let a = a / (ss * ss) as f32;
            out.put_pixel(x as u32, y as u32, Rgba([0x30, 0x33, 0x38, (a.clamp(0.0, 1.0) * 255.0).round() as u8]));
        }
    }
    out
}
