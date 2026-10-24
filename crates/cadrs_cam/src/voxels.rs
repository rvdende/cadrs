//! The stock as voxels (in model coordinates), for carrying it between setups whatever their
//! orientation: each setup (or tilted operation) takes a height map of it in its own frame
//! ([`Voxels::height_map`]), cuts that as the simulation does, and carves the result back
//! ([`Voxels::carve`]). A side setup sees what the first setup left; a 3+2 cut reaches the
//! stock the next operation starts from. Coarser than a height map (a voxel is a fraction of a
//! millimetre on a small part), so the exact column carry ([`HeightMap::reframed`]) is used
//! where the setups' Z only flips.

use rayon::prelude::*;

use crate::geom::{Aabb, Frame, P3};
use crate::heightmap::{EMPTY, HeightMap};
use crate::model::{height_on, point_in_tri2};

#[derive(Debug, Clone)]
pub struct Voxels {
    /// The centre of voxel (0, 0, 0).
    pub origin: P3,
    pub cell: f64,
    pub n: [usize; 3],
    /// Material at each voxel (x fastest, then y, then z).
    pub v: Vec<bool>,
}

impl Voxels {
    /// A closed solid's triangles filled at `cell` (each column filled between its crossings
    /// of the surface, in pairs).
    pub fn of_solid(tris: &[[P3; 3]], cell: f64) -> Voxels {
        let b = Aabb::of(tris.iter().flatten().copied());
        let cell = cell.max(1e-3);
        let n = [0, 1, 2].map(|k| (((b.max[k] - b.min[k]) / cell).ceil() as usize).max(1));
        let origin = [0, 1, 2].map(|k| b.min[k] + cell * 0.5);
        let mut out = Voxels {
            origin,
            cell,
            n,
            v: vec![false; n[0] * n[1] * n[2]],
        };
        let columns: Vec<Vec<bool>> = (0..n[0] * n[1])
            .into_par_iter()
            .map(|c| {
                let (i, j) = (c % n[0], c / n[0]);
                let p = [origin[0] + i as f64 * cell, origin[1] + j as f64 * cell];
                let mut zs: Vec<f64> = tris
                    .iter()
                    .filter(|t| point_in_tri2(p, t))
                    .filter_map(|t| height_on(t, p))
                    .collect();
                zs.sort_by(f64::total_cmp);
                zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                let mut col = vec![false; n[2]];
                for w in zs.chunks(2) {
                    let [lo, hi] = [w[0], *w.get(1).unwrap_or(&w[0])];
                    for (k, f) in col.iter_mut().enumerate() {
                        let z = origin[2] + k as f64 * cell;
                        if z >= lo && z <= hi {
                            *f = true;
                        }
                    }
                }
                col
            })
            .collect();
        for (c, col) in columns.into_iter().enumerate() {
            for (k, f) in col.into_iter().enumerate() {
                out.v[k * n[0] * n[1] + c] = f;
            }
        }
        out
    }

    /// A stock height map (in its own coordinates) as voxels of `cell`: each column filled
    /// from its floor (or `bottom`) to its top, wherever the voxel's centre is.
    pub fn of_height_map(map: &HeightMap, bottom: f32, cell: f64) -> Voxels {
        let cell = cell.max(1e-3);
        let top = map
            .z
            .iter()
            .copied()
            .filter(|z| *z != EMPTY)
            .fold(bottom, f32::max) as f64;
        let floor = |n: usize| map.floor.as_ref().map_or(bottom, |f| f[n].min(f32::MAX));
        let lo = [
            map.origin[0] - map.cell * 0.5,
            map.origin[1] - map.cell * 0.5,
            bottom as f64,
        ];
        let size = [
            map.nx as f64 * map.cell,
            map.ny as f64 * map.cell,
            (top - bottom as f64).max(cell),
        ];
        let n = [0, 1, 2].map(|k| ((size[k] / cell).ceil() as usize).max(1));
        let origin = [0, 1, 2].map(|k| lo[k] + cell * 0.5);
        let mut out = Voxels {
            origin,
            cell,
            n,
            v: vec![false; n[0] * n[1] * n[2]],
        };
        for j in 0..n[1] {
            for i in 0..n[0] {
                let c = out.centre(i, j, 0);
                let Some((ci, cj)) = map.cell_of([c[0], c[1]]) else {
                    continue;
                };
                let m = map.index(ci, cj);
                let (t, f) = (map.z[m], floor(m));
                if t == EMPTY {
                    continue;
                }
                for k in 0..n[2] {
                    let z = origin[2] + k as f64 * cell;
                    if z >= f as f64 - 1e-9 && z <= t as f64 + 1e-9 {
                        out.v[k * n[0] * n[1] + j * n[0] + i] = true;
                    }
                }
            }
        }
        out
    }

    fn centre(&self, i: usize, j: usize, k: usize) -> P3 {
        [
            self.origin[0] + i as f64 * self.cell,
            self.origin[1] + j as f64 * self.cell,
            self.origin[2] + k as f64 * self.cell,
        ]
    }

    /// The material's volume (mm³).
    pub fn volume(&self) -> f64 {
        self.v.iter().filter(|x| **x).count() as f64 * self.cell.powi(3)
    }

    /// Each present voxel's centre.
    fn present(&self) -> impl Iterator<Item = P3> + '_ {
        let [nx, ny, _] = self.n;
        self.v
            .iter()
            .enumerate()
            .filter(|(_, f)| **f)
            .map(move |(idx, _)| {
                let (i, j, k) = (idx % nx, (idx / nx) % ny, idx / (nx * ny));
                self.centre(i, j, k)
            })
    }

    /// A voxel's half extent along each of `frame`'s axes (a cube turned shows more of itself).
    fn extent(&self, frame: &Frame) -> [f64; 3] {
        let h = self.cell * 0.5;
        [frame.x, frame.y, frame.z].map(|a| h * (a[0].abs() + a[1].abs() + a[2].abs()))
    }

    /// The map cells (`i0..=i1`, `j0..=j1`) whose sample points lie within `r` of `q`.
    fn cells_near(m: &HeightMap, q: P3, r: [f64; 2]) -> Option<[usize; 4]> {
        let i0 = ((q[0] - r[0] - m.origin[0]) / m.cell).ceil().max(0.0) as usize;
        let j0 = ((q[1] - r[1] - m.origin[1]) / m.cell).ceil().max(0.0) as usize;
        let i1 = (((q[0] + r[0] - m.origin[0]) / m.cell).floor() as isize).min(m.nx as isize - 1);
        let j1 = (((q[1] + r[1] - m.origin[1]) / m.cell).floor() as isize).min(m.ny as isize - 1);
        (i1 >= i0 as isize && j1 >= j0 as isize).then_some([i0, i1 as usize, j0, j1 as usize])
    }

    /// The material as seen down `frame`'s Z over `area` (in its coordinates): a stock height
    /// map on a grid of `cell`, each column's top and bottom from the voxels in it. It errs on
    /// the side of material: each voxel covers the cells under its whole (turned) extent and a
    /// quarter of a voxel more, up to its top corner.
    pub fn height_map(&self, frame: &Frame, area: &Aabb, cell: f64) -> HeightMap {
        let mut m = HeightMap::new([area.min[0], area.min[1]], [area.max[0], area.max[1]], cell);
        let mut floor = vec![f32::INFINITY; m.nx * m.ny];
        let e = self.extent(frame);
        let reach = [e[0] + self.cell * 0.25, e[1] + self.cell * 0.25];
        for q in self.present().map(|p| frame.to_local(p)) {
            let Some([i0, i1, j0, j1]) = Self::cells_near(&m, q, reach) else {
                continue;
            };
            let (top, bottom) = ((q[2] + e[2]) as f32, (q[2] - e[2]) as f32);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let n = m.index(i, j);
                    if m.z[n] == EMPTY || top > m.z[n] {
                        m.z[n] = top;
                    }
                    floor[n] = floor[n].min(bottom);
                }
            }
        }
        for (z, f) in m.z.iter().zip(floor.iter_mut()) {
            if *z == EMPTY {
                *f = EMPTY;
            }
        }
        m.floor = Some(floor);
        m
    }

    /// Takes away what a height map down `frame`'s Z no longer has: a voxel goes only when the
    /// whole of it is above the cut (or in columns cut through) in every column under it, so
    /// the voxels never hold less material than there is.
    pub fn carve(&mut self, frame: &Frame, map: &HeightMap) {
        let [nx, ny, _] = self.n;
        let e = self.extent(frame);
        let cells: Vec<usize> = self
            .v
            .par_iter()
            .enumerate()
            .filter(|(_, f)| **f)
            .filter_map(|(idx, _)| {
                let (i, j, k) = (idx % nx, (idx / nx) % ny, idx / (nx * ny));
                let q = frame.to_local(self.centre(i, j, k));
                let low = q[2] - e[2];
                let gone = |ci: usize, cj: usize| {
                    let top = map.at(ci, cj);
                    top == EMPTY || low > top as f64 + 1e-6
                };
                let all = match Self::cells_near(map, q, [e[0], e[1]]) {
                    Some([i0, i1, j0, j1]) => {
                        // Off the map at an edge: not cut there.
                        let inside = q[0] - e[0] >= map.origin[0] - map.cell * 0.5
                            && q[1] - e[1] >= map.origin[1] - map.cell * 0.5
                            && q[0] + e[0] <= map.origin[0] + (map.nx as f64 - 0.5) * map.cell
                            && q[1] + e[1] <= map.origin[1] + (map.ny as f64 - 0.5) * map.cell;
                        inside && (j0..=j1).all(|cj| (i0..=i1).all(|ci| gone(ci, cj)))
                    }
                    None => map
                        .cell_of([q[0], q[1]])
                        .is_some_and(|(ci, cj)| gone(ci, cj)),
                };
                all.then_some(idx)
            })
            .collect();
        for idx in cells {
            self.v[idx] = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::box_triangles;
    use crate::tool::{Tool, ToolKind};

    /// A 20 × 20 × 20 block: a slot cut across its top from above, then the block seen from
    /// its side (+Y), along the slot: the slot is open right through.
    #[test]
    fn a_side_setup_sees_the_top_setups_slot() {
        let tris = box_triangles(&Aabb::new([0.0; 3], [20.0, 20.0, 20.0]));
        let mut v = Voxels::of_solid(&tris, 0.5);
        assert!((v.volume() - 8000.0).abs() < 1.0, "{}", v.volume());
        // From above: a 6 wide slot 5 deep along Y at x = 10.
        let top = Frame::WORLD;
        let mut m = v.height_map(&top, &Aabb::new([0.0; 3], [20.0, 20.0, 20.0]), 0.5);
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        m.cut(&tool, [10.0, -5.0, 15.0], [10.0, 25.0, 15.0]);
        v.carve(&top, &m);
        // (Within a voxel column each side: a voxel on the cutter's edge goes.)
        assert!(
            (v.volume() - (8000.0 - 6.0 * 20.0 * 5.0)).abs() < 110.0,
            "{}",
            v.volume()
        );
        // From the +Y side, looking along the slot (its Z the model's +Y, its X the model's
        // +X, its Y the model's −Z): the slot is open right through; beside it, the full 20.
        let side = Frame::from_zx([0.0, 20.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
        let area = Aabb::new([-1.0, -21.0, -20.0], [21.0, 21.0, 0.0]);
        let sm = v.height_map(&side, &area, 0.5);
        let at = |model: P3| {
            let q = side.to_local(model);
            sm.sample([q[0], q[1]])
        };
        // Through the slot's middle (model x 10, z 17.5): nothing.
        assert_eq!(at([10.0, 20.0, 17.5]), EMPTY);
        // Beside it (x 3) and under it (z 10): the block's whole 20 along Y, its top at 0.
        assert!(at([3.0, 20.0, 17.5]).abs() < 0.6);
        assert!(at([10.0, 20.0, 10.0]).abs() < 0.6);
    }

    /// A face cut on a compound angle (45° round Z, then 35° over): seen in that frame again,
    /// the carried stock is never under the true one anywhere, and at most a voxel over it.
    #[test]
    fn a_compound_cut_never_loses_material() {
        let tris = box_triangles(&Aabb::new([0.0; 3], [20.0, 20.0, 20.0]));
        let cell = 0.5;
        let mut v = Voxels::of_solid(&tris, cell);
        let (a, b) = (45f64.to_radians(), 35f64.to_radians());
        let z = [b.sin() * a.cos(), b.sin() * a.sin(), b.cos()];
        let x = crate::geom::normalize([-a.sin(), a.cos(), 0.0]);
        let f = Frame::from_zx([10.0, 10.0, 10.0], z, x);
        let corners = tris.iter().flatten().map(|p| f.to_local(*p));
        let b = Aabb::of(corners);
        let area = Aabb::new(
            [b.min[0] - 2.0, b.min[1] - 2.0, b.min[2]],
            [b.max[0] + 2.0, b.max[1] + 2.0, b.max[2]],
        );
        // Faced flat at the frame's z = 3.
        let face = 3.0;
        let mut m = v.height_map(&f, &area, cell);
        for t in m.z.iter_mut() {
            if *t != EMPTY {
                *t = t.min(face as f32);
            }
        }
        v.carve(&f, &m);
        let after = v.height_map(&f, &area, cell);
        // The block's true material under the face, sampled on a grid (a millimetre in from
        // its faces: the map's cells are points, the nearest one on the silhouette outside).
        let mut checked = 0;
        for i in 2..=38 {
            for j in 2..=38 {
                for k in 2..=38 {
                    let p = [i as f64 * 0.5, j as f64 * 0.5, k as f64 * 0.5];
                    let q = f.to_local(p);
                    if q[2] > face {
                        continue;
                    }
                    let top = after.sample([q[0], q[1]]);
                    assert!(top != EMPTY && top as f64 >= q[2] - 1e-6, "{p:?}: {top}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 10_000);
        // And not much over it.
        for t in after.z.iter().filter(|t| **t != EMPTY) {
            assert!((*t as f64) < face + 2.0 * cell, "{t}");
        }
    }
}
