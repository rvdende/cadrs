//! The drop-cutter along any direction: how far a leaning cutter can come down its own axis
//! before it touches the model, and what a ray along a direction hits.
//!
//! The 3-axis drop-cutter ([`crate::dropcutter`]) bins triangles over XY because its cutter
//! always comes down Z. A cutter on a rotary axis or a 5-axis head comes from anywhere, so the
//! triangles here are binned in a 3D grid ([`TriGrid`]); a drop takes those near the cutter's
//! cylinder, turns them into the cutter's own frame (the tool along +Z) and tests them as the
//! 3-axis one does ([`crate::dropcutter::touch`]): the same exact contact, whatever the lean.

use crate::dropcutter::touch;
use crate::geom::{Aabb, Frame, P3, add, cross, dot, normalize, scale, sub};
use crate::tool::Tool;

/// Triangles binned in a grid of cubes.
pub struct TriGrid {
    tris: Vec<[P3; 3]>,
    /// Unit normals.
    normals: Vec<P3>,
    min: P3,
    cell: f64,
    n: [usize; 3],
    bins: Vec<Vec<u32>>,
}

/// What a ray hits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// How far along the ray.
    pub t: f64,
    pub at: P3,
    /// The triangle's unit normal (out of the part).
    pub normal: P3,
    pub triangle: u32,
}

impl TriGrid {
    pub fn new(tris: &[[P3; 3]]) -> TriGrid {
        let b = Aabb::of(tris.iter().flatten().copied());
        let (min, size) = if b.is_empty() {
            ([0.0; 3], [1.0; 3])
        } else {
            (b.min, b.size())
        };
        // About as many cubes as triangles, at most 96 a side.
        let longest = size[0].max(size[1]).max(size[2]).max(1e-3);
        let per_side = (tris.len() as f64).cbrt().clamp(1.0, 96.0);
        let cell = (longest / per_side).max(0.05);
        let n = [0, 1, 2].map(|k| (size[k] / cell).ceil() as usize + 1);
        let mut bins = vec![Vec::new(); n[0] * n[1] * n[2]];
        for (k, t) in tris.iter().enumerate() {
            let tb = Aabb::of(t.iter().copied());
            let lo = [0, 1, 2].map(|a| ((tb.min[a] - min[a]) / cell) as usize);
            let hi = [0, 1, 2].map(|a| (((tb.max[a] - min[a]) / cell) as usize).min(n[a] - 1));
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    for x in lo[0]..=hi[0] {
                        bins[(z * n[1] + y) * n[0] + x].push(k as u32);
                    }
                }
            }
        }
        let normals = tris
            .iter()
            .map(|t| normalize(cross(sub(t[1], t[0]), sub(t[2], t[0]))))
            .collect();
        TriGrid {
            tris: tris.to_vec(),
            normals,
            min,
            cell,
            n,
            bins,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn triangle(&self, k: u32) -> &[P3; 3] {
        &self.tris[k as usize]
    }

    pub fn normal(&self, k: u32) -> P3 {
        self.normals[k as usize]
    }

    /// The triangles in the cubes `b` reaches (each once).
    pub fn near(&self, b: &Aabb, out: &mut Vec<u32>) {
        out.clear();
        let mut lo = [0usize; 3];
        let mut hi = [0usize; 3];
        for a in 0..3 {
            let (l, h) = (
                (b.min[a] - self.min[a]) / self.cell,
                (b.max[a] - self.min[a]) / self.cell,
            );
            if h < 0.0 || l >= self.n[a] as f64 {
                return;
            }
            lo[a] = l.max(0.0) as usize;
            hi[a] = (h as usize).min(self.n[a] - 1);
        }
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    out.extend_from_slice(&self.bins[(z * self.n[1] + y) * self.n[0] + x]);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// How far along `axis` from `at` the tip of `tool` rests when the tool comes down its own
    /// axis (pointing along `axis`, away from the part) onto the triangles: the tip is at
    /// `at + axis·h` for the `h` returned, the highest contact between `lo` and `hi` along the
    /// axis, or `floor` where nothing between them is under the cutter.
    pub fn drop_along(
        &self,
        tool: &Tool,
        at: P3,
        axis: P3,
        (lo, hi): (f64, f64),
        floor: f64,
        scratch: &mut Vec<u32>,
    ) -> f64 {
        let frame = Frame::from_zx(at, axis, [1.0, 0.0, 0.0]);
        // The box round the cutter's cylinder over the heights asked for.
        let r = tool.radius();
        let (a, b) = (add(at, scale(frame.z, lo)), add(at, scale(frame.z, hi)));
        let mut reach = Aabb::of([a, b]);
        for k in 0..3 {
            // (A disc of radius r square to the axis reaches r·√(1 − z_k²) along axis k.)
            let e = r * (1.0 - frame.z[k] * frame.z[k]).max(0.0).sqrt();
            reach.min[k] -= e;
            reach.max[k] += e;
        }
        self.near(&reach, scratch);
        let mut h = floor;
        for &k in scratch.iter() {
            let t = &self.tris[k as usize];
            let local = [
                frame.to_local(t[0]),
                frame.to_local(t[1]),
                frame.to_local(t[2]),
            ];
            // (Above the heights asked for: no part of the cut.)
            if local.iter().all(|p| p[2] > hi) {
                continue;
            }
            h = touch(
                tool,
                &local,
                frame.dir_to_local(self.normals[k as usize]),
                [0.0, 0.0],
                h,
            );
        }
        h
    }

    /// The first triangle a ray from `from` along the unit vector `dir` meets within `reach`,
    /// from its outside (`None`: nothing).
    pub fn ray(&self, from: P3, dir: P3, reach: f64, scratch: &mut Vec<u32>) -> Option<RayHit> {
        // The cubes along the ray, a stretch of a few cubes at a time.
        let stride = self.cell * 4.0;
        let mut t0 = 0.0;
        while t0 < reach {
            let t1 = (t0 + stride).min(reach);
            let (a, b) = (add(from, scale(dir, t0)), add(from, scale(dir, t1)));
            self.near(&Aabb::of([a, b]), scratch);
            let mut best: Option<RayHit> = None;
            for &k in scratch.iter() {
                let n = self.normals[k as usize];
                if dot(n, dir) >= 0.0 {
                    continue;
                }
                if let Some(t) = ray_triangle(from, dir, &self.tris[k as usize])
                    && t >= t0 - 1e-9
                    && t <= t1 + 1e-9
                    && best.is_none_or(|b| t < b.t)
                {
                    best = Some(RayHit {
                        t,
                        at: add(from, scale(dir, t)),
                        normal: n,
                        triangle: k,
                    });
                }
            }
            if best.is_some() {
                return best;
            }
            t0 = t1;
        }
        None
    }
}

/// Where along the ray it crosses the triangle (Möller–Trumbore).
fn ray_triangle(o: P3, d: P3, t: &[P3; 3]) -> Option<f64> {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dropcutter::{TriIndex, drop_point};
    use crate::model::box_triangles;
    use crate::tool::ToolKind;

    /// A ball of radius `r` about the origin as triangles (latitude and longitude bands).
    pub fn sphere(r: f64, n: usize) -> Vec<[P3; 3]> {
        let at = |i: usize, j: usize| {
            let (lat, lon) = (
                -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * i as f64 / n as f64,
                std::f64::consts::TAU * j as f64 / (2 * n) as f64,
            );
            [
                r * lat.cos() * lon.cos(),
                r * lat.cos() * lon.sin(),
                r * lat.sin(),
            ]
        };
        let mut t = Vec::new();
        for i in 0..n {
            for j in 0..2 * n {
                let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
                if i > 0 {
                    t.push([a, b, c]);
                }
                if i + 1 < n {
                    t.push([a, c, d]);
                }
            }
        }
        t
    }

    #[test]
    fn a_drop_down_z_is_the_three_axis_drop() {
        let tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 10.0]));
        let (grid, idx) = (TriGrid::new(&tris), TriIndex::new(&tris));
        let mut s = Vec::new();
        for kind in [ToolKind::EndMill, ToolKind::BallMill, ToolKind::BullMill] {
            let mut tool = Tool::new(kind, 8.0);
            tool.corner_radius = if kind == ToolKind::BullMill {
                1.5
            } else {
                tool.corner_radius
            };
            for p in [
                [20.0, 15.0],
                [-3.0, 15.0],
                [41.5, 31.0],
                [-2.0, -2.5],
                [60.0, 60.0],
            ] {
                let want = drop_point(&tool, &idx, p, -5.0, &mut s);
                let got = grid.drop_along(
                    &tool,
                    [p[0], p[1], 0.0],
                    [0.0, 0.0, 1.0],
                    (-5.0, 50.0),
                    -5.0,
                    &mut s,
                );
                assert!(
                    (want - got).abs() < 1e-9,
                    "{kind:?} at {p:?}: {got} for {want}"
                );
            }
        }
    }

    #[test]
    fn a_ball_dropped_toward_a_ball_rests_on_it_from_any_side() {
        let tris = sphere(30.0, 48);
        let grid = TriGrid::new(&tris);
        let tool = Tool::new(ToolKind::BallMill, 10.0);
        let mut s = Vec::new();
        for axis in [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.3, -0.8, 0.2],
            [-0.5, 0.5, -0.7],
        ] {
            let axis = normalize(axis);
            // Dropped along a line through the centre the ball's tip rests on the sphere.
            let h = grid.drop_along(&tool, [0.0; 3], axis, (0.0, 100.0), 0.0, &mut s);
            assert!((h - 30.0).abs() < 0.05, "{axis:?}: {h}");
            // Off to the side by 20 the two balls touch where their centres are 35 apart:
            // the cutter's centre √(35² − 20²) up, its tip a radius below that.
            let side = normalize(cross(axis, [0.1, 0.2, 0.9]));
            let h = grid.drop_along(
                &tool,
                scale(side, 20.0),
                axis,
                (-50.0, 100.0),
                -50.0,
                &mut s,
            );
            let want = (35f64 * 35.0 - 400.0).sqrt() - 5.0;
            assert!((h - want).abs() < 0.05, "{axis:?}: {h} for {want}");
            // Past the ball's side nothing is under the cutter.
            let h = grid.drop_along(
                &tool,
                scale(side, 36.0),
                axis,
                (-50.0, 100.0),
                -50.0,
                &mut s,
            );
            assert_eq!(h, -50.0);
        }
    }

    #[test]
    fn a_ray_meets_the_first_face_from_outside() {
        let tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 10.0]));
        let grid = TriGrid::new(&tris);
        let mut s = Vec::new();
        let hit = grid
            .ray([20.0, 15.0, 50.0], [0.0, 0.0, -1.0], 100.0, &mut s)
            .unwrap();
        assert!((hit.t - 40.0).abs() < 1e-9 && hit.normal == [0.0, 0.0, 1.0]);
        let hit = grid
            .ray([-10.0, 15.0, 5.0], [1.0, 0.0, 0.0], 100.0, &mut s)
            .unwrap();
        assert!((hit.at[0]).abs() < 1e-9 && hit.normal == [-1.0, 0.0, 0.0]);
        assert!(
            grid.ray([20.0, 15.0, 50.0], [0.0, 0.0, 1.0], 100.0, &mut s)
                .is_none()
        );
        assert!(
            grid.ray([20.0, 15.0, 50.0], [0.0, 0.0, -1.0], 30.0, &mut s)
                .is_none()
        );
    }
}
