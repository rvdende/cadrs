//! Placeholder thumbnails: a small isometric part (one or two blocks) in the light blue-grey
//! part shading, varied by a seed so each document looks different. M2 replaces these with
//! real renders of the first Part Studio.

use image::{Rgba, RgbaImage};

/// Thumbnail size in pixels (2x the 60x34 the list shows, so it stays crisp).
pub const THUMB_W: u32 = 120;
pub const THUMB_H: u32 = 68;
/// The large thumbnail's size: 2x the 150 px high the details panel shows it, in the same
/// shape as the small one.
pub const THUMB_LARGE_W: u32 = 540;
pub const THUMB_LARGE_H: u32 = 306;

const TOP: [f32; 3] = [0.80, 0.84, 0.89];
const LEFT: [f32; 3] = [0.56, 0.63, 0.71];
const RIGHT: [f32; 3] = [0.67, 0.74, 0.81];
const EDGE: [f32; 3] = [0.19, 0.23, 0.28];

/// A small deterministic PRNG (SplitMix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `lo..hi`.
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() >> 40) as f32 / (1u64 << 24) as f32 * (hi - lo)
    }
}

#[derive(Clone, Copy)]
struct Block {
    min: [f32; 3],
    max: [f32; 3],
}

/// Isometric projection (Z up), screen y pointing down.
fn project(p: [f32; 3]) -> (f32, f32) {
    let c = 30f32.to_radians().cos();
    let s = 30f32.to_radians().sin();
    ((p[0] - p[1]) * c, (p[0] + p[1]) * s - p[2])
}

/// The three visible faces of a block as screen polygons, with their colors.
/// A screen-space quad and its color.
type Face = ([(f32, f32); 4], [f32; 3]);

fn faces(b: &Block) -> [Face; 3] {
    let [x0, y0, z0] = b.min;
    let [x1, y1, z1] = b.max;
    let p = |x, y, z| project([x, y, z]);
    [
        // The +X face (lower right on screen), the +Y face (lower left) and the top.
        (
            [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
            RIGHT,
        ),
        (
            [p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1)],
            LEFT,
        ),
        (
            [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
            TOP,
        ),
    ]
}

fn inside(poly: &[(f32, f32); 4], x: f32, y: f32) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (ax, ay) = poly[i];
        let (bx, by) = poly[(i + 1) % 4];
        let cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax);
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn seg_dist(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (px, py) = (a.0 + t * dx - x, a.1 + t * dy - y);
    (px * px + py * py).sqrt()
}

fn block(min: [f32; 3], max: [f32; 3]) -> Block {
    Block { min, max }
}

/// A small part made of blocks, one of several shapes (a plate with a boss, an L bracket, a
/// U channel, steps, a post on a base, a cross) with random proportions, so documents look
/// different from each other. Blocks come back to front for the painter's algorithm.
fn shape(rng: &mut Rng) -> Vec<Block> {
    let a = rng.range(1.2, 3.2);
    let b = rng.range(1.0, 3.0);
    let c = rng.range(0.15, 0.5);
    let t = rng.range(0.2, 0.4);
    let mut v = match rng.next() % 6 {
        // A plate, sometimes with a boss in the back corner.
        0 => {
            let mut v = vec![block([0.0, 0.0, 0.0], [a, b, c])];
            if !rng.next().is_multiple_of(3) {
                let w = a * rng.range(0.3, 0.6);
                let d = b * rng.range(0.3, 0.6);
                v.push(block([0.0, 0.0, c], [w, d, c + rng.range(0.2, 0.6)]));
            }
            v
        }
        // An L bracket: a base and a wall along its back edge.
        1 => vec![
            block([0.0, 0.0, 0.0], [a, b, c]),
            block([0.0, 0.0, c], [a, t, c + rng.range(0.8, 1.6)]),
        ],
        // A U channel: a base and two walls.
        2 => {
            let h = c + rng.range(0.5, 1.1);
            vec![
                block([0.0, 0.0, 0.0], [a, b, c]),
                block([0.0, 0.0, c], [t, b, h]),
                block([a - t, 0.0, c], [a, b, h]),
            ]
        }
        // Steps.
        3 => {
            let h = rng.range(0.25, 0.45);
            vec![
                block([0.0, 0.0, 0.0], [a, b, h]),
                block([0.0, 0.0, h], [a * 0.66, b, 2.0 * h]),
                block([0.0, 0.0, 2.0 * h], [a * 0.33, b, 3.0 * h]),
            ]
        }
        // A post on a square base.
        4 => {
            let s = a.max(b) * 0.8;
            let p = s * rng.range(0.25, 0.4);
            let o = (s - p) / 2.0;
            vec![
                block([0.0, 0.0, 0.0], [s, s, c]),
                block([o, o, c], [o + p, o + p, c + rng.range(1.0, 1.8)]),
            ]
        }
        // A cross of two bars.
        _ => {
            let w = rng.range(0.4, 0.7);
            vec![
                block([0.0, (b - w) / 2.0, 0.0], [a, (b + w) / 2.0, c]),
                block([(a - w) / 2.0, 0.0, c], [(a + w) / 2.0, b, 2.0 * c]),
            ]
        }
    };
    // Back to front: farther (smaller x + y) and lower first.
    v.sort_by(|p, q| {
        (p.min[2], p.min[0] + p.min[1])
            .partial_cmp(&(q.min[2], q.min[0] + q.min[1]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

/// Generates a placeholder thumbnail for `seed` (transparent background).
pub fn placeholder(seed: u64) -> RgbaImage {
    let mut rng = Rng(seed);
    let blocks = shape(&mut rng);

    // Fit the projected shape into the image with a margin.
    let mut lo = (f32::MAX, f32::MAX);
    let mut hi = (f32::MIN, f32::MIN);
    for blk in &blocks {
        for (poly, _) in faces(blk) {
            for (x, y) in poly {
                lo = (lo.0.min(x), lo.1.min(y));
                hi = (hi.0.max(x), hi.1.max(y));
            }
        }
    }
    let margin = 1.0;
    let scale = ((THUMB_W as f32 - 2.0 * margin) / (hi.0 - lo.0))
        .min((THUMB_H as f32 - 2.0 * margin) / (hi.1 - lo.1));
    let off = (
        (THUMB_W as f32 - (hi.0 - lo.0) * scale) / 2.0 - lo.0 * scale,
        (THUMB_H as f32 - (hi.1 - lo.1) * scale) / 2.0 - lo.1 * scale,
    );
    // Painter's order: blocks back to front (lower first), faces as listed.
    let polys: Vec<Face> = blocks
        .iter()
        .flat_map(|blk| faces(blk).into_iter())
        .map(|(poly, color)| {
            (
                poly.map(|(x, y)| (x * scale + off.0, y * scale + off.1)),
                color,
            )
        })
        .collect();

    const SS: u32 = 4;
    let mut img = RgbaImage::new(THUMB_W, THUMB_H);
    for py in 0..THUMB_H {
        for px in 0..THUMB_W {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    let mut color: Option<[f32; 3]> = None;
                    for (poly, c) in &polys {
                        if inside(poly, x, y) {
                            let edge = (0..4)
                                .any(|i| seg_dist(x, y, poly[i], poly[(i + 1) % 4]) < 0.55);
                            color = Some(if edge { EDGE } else { *c });
                        }
                    }
                    if let Some(c) = color {
                        acc[0] += c[0];
                        acc[1] += c[1];
                        acc[2] += c[2];
                        acc[3] += 1.0;
                    }
                }
            }
            if acc[3] > 0.0 {
                let n = acc[3];
                let to8 = |v: f32| (v / n * 255.0).round().clamp(0.0, 255.0) as u8;
                let alpha = (n / (SS * SS) as f32 * 255.0).round() as u8;
                img.put_pixel(px, py, Rgba([to8(acc[0]), to8(acc[1]), to8(acc[2]), alpha]));
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_is_deterministic_and_not_empty() {
        let a = placeholder(42);
        let b = placeholder(42);
        assert_eq!(a, b);
        assert_eq!(a.dimensions(), (THUMB_W, THUMB_H));
        let opaque = a.pixels().filter(|p| p[3] > 0).count();
        assert!(opaque > (THUMB_W * THUMB_H / 5) as usize, "{opaque}");
        assert_ne!(placeholder(1), placeholder(2));
    }
}
