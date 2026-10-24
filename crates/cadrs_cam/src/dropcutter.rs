//! The exact drop-cutter: how low a cutter can come down at a point before it touches the
//! model's triangles.
//!
//! For each triangle near the point the cutter is tested against its **facet** (the contact
//! point where the cutter's surface is parallel to the triangle's plane: on the bottom disc's
//! rim for a flat end mill, under the ball's centre for a ball mill, on the torus of a bull
//! mill, on either rim of a cone), its **vertices** and its **edges** (the part of each edge
//! within the cutter's radius, sampled and the best sample refined by golden-section search).
//! The highest contact wins. A grid of triangle bins over XY keeps each drop to the triangles
//! near it.
//!
//! [`drop_line`] samples a path adaptively: segments whose middle drops more than the tolerance
//! away from the straight line between their ends are split, so the result follows the surface
//! within the tolerance with as few points as it needs.

use rayon::prelude::*;

use crate::geom::{P2, P3, cross, normalize, sub};
use crate::model::point_in_tri2;
use crate::tool::{Tool, ToolKind};

/// Triangles binned over XY.
pub struct TriIndex {
    tris: Vec<[P3; 3]>,
    /// Unit normals.
    normals: Vec<P3>,
    origin: P2,
    cell: f64,
    nx: usize,
    ny: usize,
    bins: Vec<Vec<u32>>,
}

impl TriIndex {
    pub fn new(tris: &[[P3; 3]]) -> TriIndex {
        let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in tris.iter().flatten() {
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        if tris.is_empty() {
            min = [0.0; 2];
            max = [1.0; 2];
        }
        let (w, h) = ((max[0] - min[0]).max(1e-3), (max[1] - min[1]).max(1e-3));
        // About as many bins as triangles, at most 512 × 512.
        let n = (tris.len() as f64).sqrt().clamp(1.0, 512.0);
        let cell = (w.max(h) / n).max(0.05);
        let nx = (w / cell).ceil() as usize + 1;
        let ny = (h / cell).ceil() as usize + 1;
        let mut bins = vec![Vec::new(); nx * ny];
        for (k, t) in tris.iter().enumerate() {
            let x0 = t.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
            let x1 = t.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
            let y0 = t.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
            let y1 = t.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
            let (i0, i1) = (
                ((x0 - min[0]) / cell) as usize,
                ((x1 - min[0]) / cell) as usize,
            );
            let (j0, j1) = (
                ((y0 - min[1]) / cell) as usize,
                ((y1 - min[1]) / cell) as usize,
            );
            for j in j0..=j1.min(ny - 1) {
                for i in i0..=i1.min(nx - 1) {
                    bins[j * nx + i].push(k as u32);
                }
            }
        }
        let normals = tris
            .iter()
            .map(|t| normalize(cross(sub(t[1], t[0]), sub(t[2], t[0]))))
            .collect();
        TriIndex {
            tris: tris.to_vec(),
            normals,
            origin: min,
            cell,
            nx,
            ny,
            bins,
        }
    }

    /// The triangles whose boxes come within `r` of `p` (each once).
    fn near(&self, p: P2, r: f64, out: &mut Vec<u32>) {
        out.clear();
        let i0 = ((p[0] - r - self.origin[0]) / self.cell).floor().max(0.0) as usize;
        let j0 = ((p[1] - r - self.origin[1]) / self.cell).floor().max(0.0) as usize;
        let i1 = ((p[0] + r - self.origin[0]) / self.cell).floor();
        let j1 = ((p[1] + r - self.origin[1]) / self.cell).floor();
        if i1 < 0.0 || j1 < 0.0 {
            return;
        }
        let (i1, j1) = (
            (i1 as usize).min(self.nx - 1),
            (j1 as usize).min(self.ny - 1),
        );
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.extend_from_slice(&self.bins[j * self.nx + i]);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// The plane's height over `q` (its normal's z must not be 0).
fn plane_z(p0: P3, n: P3, q: P2) -> f64 {
    p0[2] - (n[0] * (q[0] - p0[0]) + n[1] * (q[1] - p0[1])) / n[2]
}

/// The tip height where the cutter touches a facet's plane, if the contact point lies on the
/// facet.
fn facet(tool: &Tool, t: &[P3; 3], n: P3, p: P2) -> Option<f64> {
    if n[2] < 1e-9 {
        return None;
    }
    let r = tool.radius();
    let nxy = (n[0] * n[0] + n[1] * n[1]).sqrt();
    let u = if nxy > 1e-12 {
        [n[0] / nxy, n[1] / nxy]
    } else {
        [0.0, 0.0]
    };
    // A disc of radius `rho` at `h` above the tip, touching with its rim's lowest point toward
    // the plane.
    let disc = |rho: f64, h: f64| -> Option<f64> {
        let cc = [p[0] - rho * u[0], p[1] - rho * u[1]];
        point_in_tri2(cc, t).then(|| plane_z(t[0], n, cc) - h)
    };
    match tool.kind {
        ToolKind::BallMill => {
            let cc = [p[0] - r * n[0], p[1] - r * n[1]];
            point_in_tri2(cc, t).then(|| plane_z(t[0], n, cc) + r * n[2] - r)
        }
        ToolKind::BullMill => {
            let rc = tool.corner_radius.clamp(0.0, r);
            let ring = r - rc;
            let cc = [
                p[0] - ring * u[0] - rc * n[0],
                p[1] - ring * u[1] - rc * n[1],
            ];
            point_in_tri2(cc, t).then(|| plane_z(t[0], n, cc) + rc * n[2] - rc)
        }
        ToolKind::ChamferMill | ToolKind::TaperMill | ToolKind::Drill => {
            let tip = tool.tip_diameter / 2.0;
            let top = tool.height_at(r).unwrap_or(0.0);
            [disc(tip, 0.0), disc(r, top)]
                .into_iter()
                .flatten()
                .reduce(f64::max)
        }
        _ => disc(r, 0.0),
    }
}

/// The tip height where the cutter touches a point.
fn vertex(tool: &Tool, v: P3, p: P2) -> Option<f64> {
    let d = (v[0] - p[0]).hypot(v[1] - p[1]);
    tool.height_at(d).map(|h| v[2] - h)
}

/// The tip height where the cutter touches a segment (its part within the radius).
fn edge(tool: &Tool, a: P3, b: P3, p: P2) -> Option<f64> {
    let r = tool.radius();
    // Where the segment's XY is within r of p: |a + t d − p|² ≤ r².
    let d = [b[0] - a[0], b[1] - a[1]];
    let f = [a[0] - p[0], a[1] - p[1]];
    let qa = d[0] * d[0] + d[1] * d[1];
    if qa < 1e-18 {
        return None;
    }
    let qb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
    let qc = f[0] * f[0] + f[1] * f[1] - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = ((-qb - s) / (2.0 * qa)).max(0.0);
    let t1 = ((-qb + s) / (2.0 * qa)).min(1.0);
    if t1 < t0 {
        return None;
    }
    let at = |t: f64| -> f64 {
        let q = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ];
        vertex(tool, q, p).unwrap_or(f64::NEG_INFINITY)
    };
    // Coarse samples, then refine round the best.
    let n = 16;
    let mut best = (f64::NEG_INFINITY, t0);
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let z = at(t);
        if z > best.0 {
            best = (z, t);
        }
    }
    let h = (t1 - t0) / n as f64;
    let (mut lo, mut hi) = ((best.1 - h).max(t0), (best.1 + h).min(t1));
    let g = 0.618_033_988_749_895;
    let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
    let (mut f1, mut f2) = (at(x1), at(x2));
    for _ in 0..40 {
        if f1 > f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - g * (hi - lo);
            f1 = at(x1);
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + g * (hi - lo);
            f2 = at(x2);
        }
    }
    let z = best.0.max(f1).max(f2);
    z.is_finite().then_some(z)
}

/// The lowest tip height at `p` that doesn't cut into the triangles (`floor` when nothing is
/// under the cutter).
pub fn drop_point(tool: &Tool, idx: &TriIndex, p: P2, floor: f64, scratch: &mut Vec<u32>) -> f64 {
    let r = tool.radius();
    idx.near(p, r, scratch);
    let mut z = floor;
    for &k in scratch.iter() {
        let t = &idx.tris[k as usize];
        let n = idx.normals[k as usize];
        // Quick reject: the triangle's highest point can't beat what we have.
        let top = t[0][2].max(t[1][2]).max(t[2][2]);
        if top <= z {
            continue;
        }
        if let Some(h) = facet(tool, t, n, p) {
            z = z.max(h);
        }
        for i in 0..3 {
            if let Some(h) = vertex(tool, t[i], p) {
                z = z.max(h);
            }
            if let Some(h) = edge(tool, t[i], t[(i + 1) % 3], p) {
                z = z.max(h);
            }
        }
    }
    z
}

/// Drops many points (in parallel).
pub fn drop_points(tool: &Tool, idx: &TriIndex, pts: &[P2], floor: f64) -> Vec<f64> {
    pts.par_iter()
        .map_init(Vec::new, |scratch, p| {
            drop_point(tool, idx, *p, floor, scratch)
        })
        .collect()
}

/// A path dropped onto the model, sampled adaptively: no straight move between two of its
/// points strays more than `tol` above or below the surface the cutter follows (checked at
/// the middle, down to `min_step`).
pub fn drop_line(
    tool: &Tool,
    idx: &TriIndex,
    line: &[P2],
    floor: f64,
    step: f64,
    tol: f64,
    min_step: f64,
) -> Vec<P3> {
    let dense = crate::strategy::path::densify(line, step.max(min_step));
    let zs = drop_points(tool, idx, &dense, floor);
    let mut out: Vec<P3> = Vec::with_capacity(dense.len());
    let mut scratch = Vec::new();
    for w in 0..dense.len() {
        let p = [dense[w][0], dense[w][1], zs[w]];
        if let Some(prev) = out.last().copied() {
            refine(
                tool,
                idx,
                prev,
                p,
                floor,
                tol,
                min_step,
                &mut scratch,
                &mut out,
                0,
            );
        }
        out.push(p);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn refine(
    tool: &Tool,
    idx: &TriIndex,
    a: P3,
    b: P3,
    floor: f64,
    tol: f64,
    min_step: f64,
    scratch: &mut Vec<u32>,
    out: &mut Vec<P3>,
    depth: u32,
) {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    if len < min_step * 2.0 || depth > 12 {
        // As fine as it goes: a cliff in the surface the cutter follows (a wall it slides down,
        // the ends' heights apart by more than the tolerance this close) is gone over at the
        // higher end's height, then down, never cut through.
        let zm = drop_point(tool, idx, m, floor, scratch);
        if (a[2] - b[2]).abs() > tol || zm > (a[2] + b[2]) / 2.0 + tol {
            let z = a[2].max(b[2]).max(zm);
            out.push(if a[2] >= b[2] {
                [b[0], b[1], z]
            } else {
                [a[0], a[1], z]
            });
        }
        return;
    }
    let zm = drop_point(tool, idx, m, floor, scratch);
    if (zm - (a[2] + b[2]) / 2.0).abs() <= tol {
        return;
    }
    let mid = [m[0], m[1], zm];
    refine(
        tool,
        idx,
        a,
        mid,
        floor,
        tol,
        min_step,
        scratch,
        out,
        depth + 1,
    );
    out.push(mid);
    refine(
        tool,
        idx,
        mid,
        b,
        floor,
        tol,
        min_step,
        scratch,
        out,
        depth + 1,
    );
}

/// The model's height under a point, for a cutter of nothing (its triangles' tops).
pub fn surface_at(idx: &TriIndex, p: P2) -> Option<f64> {
    let mut v = Vec::new();
    idx.near(p, 0.0, &mut v);
    v.iter()
        .filter_map(|&k| {
            let t = &idx.tris[k as usize];
            let n = idx.normals[k as usize];
            (n[2].abs() > 1e-12 && point_in_tri2(p, t)).then(|| plane_z(t[0], n, p))
        })
        .reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb;
    use crate::model::box_triangles;

    fn boxed() -> TriIndex {
        TriIndex::new(&box_triangles(&Aabb::new(
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 5.0],
        )))
    }

    #[test]
    fn flat_on_top_and_over_the_edge() {
        let t = Tool::new(ToolKind::EndMill, 6.0);
        let idx = boxed();
        let mut s = Vec::new();
        assert!((drop_point(&t, &idx, [5.0, 5.0], -1.0, &mut s) - 5.0).abs() < 1e-12);
        assert!((drop_point(&t, &idx, [12.9, 5.0], -1.0, &mut s) - 5.0).abs() < 1e-12);
        assert_eq!(drop_point(&t, &idx, [13.1, 5.0], -1.0, &mut s), -1.0);
    }

    #[test]
    fn ball_over_the_edge_and_corner_exact() {
        let t = Tool::new(ToolKind::BallMill, 6.0);
        let idx = boxed();
        let mut s = Vec::new();
        let z = drop_point(&t, &idx, [12.0, 5.0], -10.0, &mut s);
        assert!((z - (5.0 - (3.0 - 5f64.sqrt()))).abs() < 1e-9, "{z}");
        // Off the corner diagonally by d: touches the corner vertex.
        let d = 2.0;
        let q = [10.0 + d / 2f64.sqrt(), 10.0 + d / 2f64.sqrt()];
        let z = drop_point(&t, &idx, q, -10.0, &mut s);
        assert!(
            (z - (5.0 - (3.0 - (9.0 - d * d).sqrt()))).abs() < 1e-9,
            "{z}"
        );
    }

    #[test]
    fn ball_on_a_slope_touches_the_facet() {
        // A ramp rising 1 in 2 along X.
        let tris = vec![
            [[0.0, 0.0, 0.0], [20.0, 0.0, 10.0], [20.0, 20.0, 10.0]],
            [[0.0, 0.0, 0.0], [20.0, 20.0, 10.0], [0.0, 20.0, 0.0]],
        ];
        let idx = TriIndex::new(&tris);
        let t = Tool::new(ToolKind::BallMill, 4.0);
        let mut s = Vec::new();
        let z = drop_point(&t, &idx, [10.0, 10.0], -10.0, &mut s);
        // Centre 2/cos(a) above the plane at the centre's x; tip 2 below the centre.
        let a = (0.5f64).atan();
        let expect = 5.0 + 2.0 / a.cos() - 2.0;
        assert!((z - expect).abs() < 1e-9, "{z} {expect}");
    }

    #[test]
    fn bull_and_cone_on_edges() {
        let idx = boxed();
        let mut s = Vec::new();
        let mut bull = Tool::new(ToolKind::BullMill, 6.0);
        bull.corner_radius = 1.0;
        // 2.5 past the edge: inside the corner's torus (flat part is 2).
        let z = drop_point(&bull, &idx, [12.5, 5.0], -10.0, &mut s);
        assert!(
            (z - (5.0 - (1.0 - (1.0f64 - 0.25).sqrt()))).abs() < 1e-6,
            "{z}"
        );
        let v = Tool::new(ToolKind::ChamferMill, 6.0);
        let z = drop_point(&v, &idx, [12.0, 5.0], -10.0, &mut s);
        assert!((z - 3.0).abs() < 1e-6, "{z}");
    }

    #[test]
    fn adaptive_line_follows_within_tolerance() {
        let idx = boxed();
        let t = Tool::new(ToolKind::BallMill, 6.0);
        let pts = drop_line(&t, &idx, &[[-5.0, 5.0], [15.0, 5.0]], -1.0, 2.0, 0.01, 0.01);
        let mut s = Vec::new();
        for w in pts.windows(2) {
            let m = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
            let z = drop_point(&t, &idx, m, -1.0, &mut s);
            assert!(
                ((w[0][2] + w[1][2]) / 2.0 - z).abs() <= 0.01 + 1e-9
                    || (w[1][0] - w[0][0]).abs() < 0.03
            );
        }
    }
}
