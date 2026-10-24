//! Height maps (Z-maps): a grid of heights over XY.
//!
//! - **The part's** ([`HeightMap::of_triangles`]): the highest point of the part over each
//!   cell, for the drop-cutter ([`HeightMap::drop_cutter`]): how low a cutter of any shape
//!   ([`crate::tool::Tool::height_at`]) can go at a point without touching the part. The 3D
//!   finishing strategies and the gouge check use it.
//! - **The stock's** ([`HeightMap::block`]): the top of the material over each cell. The
//!   simulation ([`HeightMap::cut`]) lowers it under the cutter along each move: Verify's
//!   in-process stock. A stock map may carry each column's bottom too (`floor`): a column is
//!   material from its floor to its top, gone when a cut reaches the floor. That's what a
//!   setup machined from above leaves, and what the next setup gets, turned over when it's
//!   flipped ([`HeightMap::reframed`]).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::geom::{Aabb, P2, P3};
use crate::model::{height_on, point_in_tri2};
use crate::tool::Tool;

/// No material over the cell.
pub const EMPTY: f32 = f32::NEG_INFINITY;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeightMap {
    /// The centre of cell (0, 0).
    pub origin: P2,
    /// Cell size (mm).
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    /// Row-major heights (`EMPTY` where there's nothing).
    pub z: Vec<f32>,
    /// Each column's bottom (a stock map carried between setups); `None`: the stock's bottom.
    #[serde(default)]
    pub floor: Option<Vec<f32>>,
}

impl HeightMap {
    /// An empty map covering `min`–`max` with cells of `cell` (at most about 4 million cells:
    /// the cell grows for big areas).
    pub fn new(min: P2, max: P2, cell: f64) -> HeightMap {
        let (w, h) = ((max[0] - min[0]).max(cell), (max[1] - min[1]).max(cell));
        let mut cell = cell.max(1e-3);
        while (w / cell).ceil() * (h / cell).ceil() > 4.0e6 {
            cell *= 1.25;
        }
        let nx = (w / cell).ceil() as usize + 1;
        let ny = (h / cell).ceil() as usize + 1;
        HeightMap {
            origin: min,
            cell,
            nx,
            ny,
            z: vec![EMPTY; nx * ny],
            floor: None,
        }
    }

    /// An empty map on the same grid.
    pub fn blank(&self) -> HeightMap {
        HeightMap {
            origin: self.origin,
            cell: self.cell,
            nx: self.nx,
            ny: self.ny,
            z: vec![EMPTY; self.nx * self.ny],
            floor: None,
        }
    }

    /// A block of stock: `bounds`' top over its footprint.
    pub fn block(bounds: &Aabb, cell: f64) -> HeightMap {
        let mut m = HeightMap::new(
            [bounds.min[0], bounds.min[1]],
            [bounds.max[0], bounds.max[1]],
            cell,
        );
        m.z.fill(bounds.max[2] as f32);
        m
    }

    /// The highest point of the triangles over each cell's centre, on a map covering
    /// `min`–`max`: the part as the simulation compares the stock with.
    pub fn of_triangles(tris: &[[P3; 3]], min: P2, max: P2, cell: f64) -> HeightMap {
        let mut m = HeightMap::new(min, max, cell);
        m.rasterize(tris, false);
        m
    }

    /// The same with every cell an edge passes near raised to the edge (walls and slivers
    /// thinner than a cell kept, edges never between cells): a cutter dropped on it never goes
    /// lower than on the true surface, at the price of up to a cell's worth too high.
    pub fn of_triangles_covering(tris: &[[P3; 3]], min: P2, max: P2, cell: f64) -> HeightMap {
        let mut m = HeightMap::new(min, max, cell);
        m.rasterize(tris, true);
        m
    }

    /// Raises cells to the triangles' heights: at their centres, and with `covering` along
    /// their edges too.
    pub fn rasterize(&mut self, tris: &[[P3; 3]], covering: bool) {
        let (nx, ny, cell, o) = (self.nx, self.ny, self.cell, self.origin);
        // Rows in parallel: each row takes the triangles whose y range covers it.
        let rows: Vec<Vec<f32>> = (0..ny)
            .into_par_iter()
            .map(|j| {
                let y = o[1] + j as f64 * cell;
                let mut row: Vec<f32> = self.z[j * nx..(j + 1) * nx].to_vec();
                for t in tris {
                    let (ymin, ymax) = (
                        t[0][1].min(t[1][1]).min(t[2][1]),
                        t[0][1].max(t[1][1]).max(t[2][1]),
                    );
                    if y < ymin - cell || y > ymax + cell {
                        continue;
                    }
                    let xmin = t[0][0].min(t[1][0]).min(t[2][0]);
                    let xmax = t[0][0].max(t[1][0]).max(t[2][0]);
                    let i0 = (((xmin - o[0]) / cell).floor().max(0.0)) as usize;
                    let i1 = ((((xmax - o[0]) / cell).ceil()) as isize).min(nx as isize - 1);
                    if i1 < 0 {
                        continue;
                    }
                    for (i, c) in row.iter_mut().enumerate().take(i1 as usize + 1).skip(i0) {
                        let p = [o[0] + i as f64 * cell, y];
                        if point_in_tri2(p, t)
                            && let Some(h) = height_on(t, p)
                        {
                            *c = c.max(h as f32);
                        }
                    }
                    if !covering {
                        continue;
                    }
                    // Edges: walls and slivers thinner than a cell still show.
                    for k in 0..3 {
                        let (a, b) = (t[k], t[(k + 1) % 3]);
                        if (a[1] - y).abs() >= cell
                            && (b[1] - y).abs() >= cell
                            && (a[1] - y).signum() == (b[1] - y).signum()
                        {
                            continue;
                        }
                        let n = (((b[0] - a[0]).abs().max((b[1] - a[1]).abs()) / (cell * 0.5))
                            .ceil() as usize)
                            .max(1);
                        for s in 0..=n {
                            let f = s as f64 / n as f64;
                            let q = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                            // The cells either side of the edge (on this row and the rows next
                            // to it): an edge never falls between cells, so a cutter dropped
                            // beside it can't slip past (a cell's worth on the safe side).
                            if ((q[1] - y) / cell).abs() >= 1.0 {
                                continue;
                            }
                            let x = (q[0] - o[0]) / cell;
                            let z = (a[2] + (b[2] - a[2]) * f) as f32;
                            for i in [x.floor(), x.ceil()] {
                                if i >= 0.0 && (i as usize) < nx {
                                    let c = &mut row[i as usize];
                                    *c = c.max(z);
                                }
                            }
                        }
                    }
                }
                row
            })
            .collect();
        for (j, r) in rows.into_iter().enumerate() {
            self.z[j * nx..(j + 1) * nx].copy_from_slice(&r);
        }
    }

    pub fn index(&self, i: usize, j: usize) -> usize {
        j * self.nx + i
    }

    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.z[self.index(i, j)]
    }

    pub fn point(&self, i: usize, j: usize) -> P2 {
        [
            self.origin[0] + i as f64 * self.cell,
            self.origin[1] + j as f64 * self.cell,
        ]
    }

    /// The cell nearest to `p`, if it's on the map.
    pub fn cell_of(&self, p: P2) -> Option<(usize, usize)> {
        let i = ((p[0] - self.origin[0]) / self.cell).round();
        let j = ((p[1] - self.origin[1]) / self.cell).round();
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.nx && (j as usize) < self.ny)
            .then_some((i as usize, j as usize))
    }

    /// The height at `p` (nearest cell; `EMPTY` off the map).
    pub fn sample(&self, p: P2) -> f32 {
        self.cell_of(p).map_or(EMPTY, |(i, j)| self.at(i, j))
    }

    /// Every cell under the cutter with its tip at `p`: the cell's index and how high the
    /// cutter's surface is above its tip over the cell. Exact (cells whose centres are within
    /// the radius, at their centres' distance) or, `covering`, every cell the cutter overlaps at
    /// the cutter's lowest height over it (a drop never comes out lower than the true one).
    fn stamp(&self, tool: &Tool, p: P2, covering: bool, mut f: impl FnMut(usize, f32)) {
        let r = tool.radius();
        let slack = if covering {
            self.cell * std::f64::consts::FRAC_1_SQRT_2
        } else {
            0.0
        };
        let reach = r + slack;
        let x0 = ((p[0] - reach - self.origin[0]) / self.cell)
            .ceil()
            .max(0.0) as usize;
        let y0 = ((p[1] - reach - self.origin[1]) / self.cell)
            .ceil()
            .max(0.0) as usize;
        let x1 = ((p[0] + reach - self.origin[0]) / self.cell).floor();
        let y1 = ((p[1] + reach - self.origin[1]) / self.cell).floor();
        if x1 < 0.0 || y1 < 0.0 {
            return;
        }
        let (x1, y1) = (
            (x1 as usize).min(self.nx - 1),
            (y1 as usize).min(self.ny - 1),
        );
        for j in y0..=y1 {
            let dy = self.origin[1] + j as f64 * self.cell - p[1];
            for i in x0..=x1 {
                let dx = self.origin[0] + i as f64 * self.cell - p[0];
                let d = (dx * dx + dy * dy).sqrt();
                if d > reach + 1e-12 {
                    continue;
                }
                let at = (d - slack).clamp(0.0, r);
                if let Some(h) = tool.height_at(at) {
                    f(j * self.nx + i, h as f32);
                }
            }
        }
    }

    /// Whether the tool with its tip at `p` would cut more than `tol` into the map's surface
    /// (its own shape, at the cells' centres).
    pub fn cuts_into(&self, tool: &Tool, p: crate::geom::P3, tol: f64) -> bool {
        let mut hit = false;
        let z = p[2] + tol;
        self.stamp(tool, [p[0], p[1]], false, |i, h| {
            let v = self.z[i];
            if v != EMPTY && v as f64 > z + h as f64 {
                hit = true;
            }
        });
        hit
    }

    /// How low the tool's tip can go at `p` without cutting into the map's surface (`floor`
    /// when nothing is under it). Cells the cutter only partly covers count, so it never comes
    /// out lower than the true drop.
    pub fn drop_cutter(&self, tool: &Tool, p: P2, floor: f64) -> f64 {
        let mut z = floor as f32;
        self.stamp(tool, p, true, |i, h| {
            let v = self.z[i];
            if v != EMPTY {
                z = z.max(v - h);
            }
        });
        z as f64
    }

    /// [`HeightMap::drop_cutter`] at many points at once (in parallel).
    pub fn drop_many(&self, tool: &Tool, pts: &[P2], floor: f64) -> Vec<f64> {
        pts.par_iter()
            .map(|p| self.drop_cutter(tool, *p, floor))
            .collect()
    }

    /// Lowers the stock under the tool moving from `a` to `b` (tip positions). Returns the
    /// volume removed (mm³).
    pub fn cut(&mut self, tool: &Tool, a: P3, b: P3) -> f64 {
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let n = ((len / (self.cell * 0.5)).ceil() as usize).max(1);
        let area = (self.cell * self.cell) as f32;
        let mut removed = 0.0f32;
        let mut cells: Vec<(usize, f32)> = Vec::new();
        for s in 0..=n {
            let f = s as f64 / n as f64;
            let p = [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ];
            cells.clear();
            self.stamp(tool, [p[0], p[1]], false, |i, h| cells.push((i, h)));
            for &(i, h) in &cells {
                let surface = p[2] as f32 + h;
                let v = &mut self.z[i];
                if *v != EMPTY && *v > surface {
                    // Through to the column's bottom: nothing left of it.
                    let bottom = self.floor.as_ref().map_or(EMPTY, |f| f[i]);
                    if surface <= bottom + 1e-4 {
                        removed += (*v - bottom) * area;
                        *v = EMPTY;
                    } else {
                        removed += (*v - surface) * area;
                        *v = surface;
                    }
                }
            }
        }
        removed as f64
    }

    /// The stock in another setup's coordinates: `self` in `from`'s (its columns' bottoms
    /// `bottom` where it has no floor), resampled on a grid of `cell` over `area` in `to`'s.
    /// The setups' Z must be the same or opposite (a move, a turn about Z, or a flip): each
    /// column stays one column, turned over by a flip. `None` for any other tilt.
    pub fn reframed(
        &self,
        from: &crate::geom::Frame,
        bottom: f64,
        to: &crate::geom::Frame,
        area: &Aabb,
        cell: f64,
    ) -> Option<HeightMap> {
        let s = crate::geom::dot(from.z, to.z);
        if (s.abs() - 1.0).abs() > 1e-6 {
            return None;
        }
        let mut m = HeightMap::new([area.min[0], area.min[1]], [area.max[0], area.max[1]], cell);
        let mut floor = vec![EMPTY; m.nx * m.ny];
        for j in 0..m.ny {
            for i in 0..m.nx {
                let q = m.point(i, j);
                let a0 = from.to_local(to.to_world([q[0], q[1], 0.0]));
                let Some((ai, aj)) = self.cell_of([a0[0], a0[1]]) else {
                    continue;
                };
                let k = self.index(ai, aj);
                let top = self.z[k];
                if top == EMPTY {
                    continue;
                }
                let fa = self.floor.as_ref().map_or(bottom as f32, |f| f[k]);
                let z0 = a0[2] as f32;
                let (lo, hi) = if s > 0.0 {
                    (fa - z0, top - z0)
                } else {
                    (z0 - top, z0 - fa)
                };
                if hi > lo + 1e-4 {
                    let n = m.index(i, j);
                    m.z[n] = hi;
                    floor[n] = lo;
                }
            }
        }
        m.floor = Some(floor);
        Some(m)
    }

    /// Lowers the stock along a whole motion (feed moves only; rapids are checked by
    /// [`crate::sim`]). Returns the volume removed.
    pub fn cut_motion(&mut self, tool: &Tool, segs: &[crate::toolpath::Segment]) -> f64 {
        segs.iter()
            .filter(|s| !s.is_rapid())
            .map(|s| self.cut(tool, s.from, s.to))
            .sum()
    }

    /// Each cell lowered to the lowest of it and its eight neighbours (an empty neighbour
    /// empties it): the surface pulled in by a cell, for comparisons that shouldn't trip over
    /// a wall's position within a cell.
    pub fn eroded(&self) -> HeightMap {
        let mut out = self.clone();
        for j in 0..self.ny {
            for i in 0..self.nx {
                let mut m = self.at(i, j);
                for (di, dj) in [
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let (x, y) = (i as isize + di, j as isize + dj);
                    if x < 0 || y < 0 || x >= self.nx as isize || y >= self.ny as isize {
                        continue;
                    }
                    m = m.min(self.at(x as usize, y as usize));
                }
                out.z[j * self.nx + i] = m;
            }
        }
        out
    }

    /// The material's volume over the map (mm³) above `base`.
    pub fn volume_above(&self, base: f64) -> f64 {
        let a = self.cell * self.cell;
        self.z
            .iter()
            .filter(|v| **v != EMPTY)
            .map(|v| (*v as f64 - base).max(0.0) * a)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::box_triangles;
    use crate::tool::ToolKind;

    #[test]
    fn rasterize_box_top() {
        let tris = box_triangles(&Aabb::new([2.0, 2.0, 0.0], [8.0, 8.0, 5.0]));
        let m = HeightMap::of_triangles(&tris, [0.0, 0.0], [10.0, 10.0], 0.25);
        assert_eq!(m.sample([5.0, 5.0]), 5.0);
        assert_eq!(m.sample([1.0, 1.0]), EMPTY);
    }

    #[test]
    fn drop_flat_and_ball() {
        let tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 5.0]));
        let m = HeightMap::of_triangles_covering(&tris, [-10.0, -10.0], [20.0, 20.0], 0.1);
        let flat = Tool::new(ToolKind::EndMill, 6.0);
        // Over the top, and overhanging the edge by less than the radius: on the top.
        assert!((m.drop_cutter(&flat, [5.0, 5.0], -1.0) - 5.0).abs() < 1e-6);
        assert!((m.drop_cutter(&flat, [12.0, 5.0], -1.0) - 5.0).abs() < 1e-6);
        // Clear of the box: down to the floor.
        assert_eq!(m.drop_cutter(&flat, [14.0, 5.0], -1.0), -1.0);
        let ball = Tool::new(ToolKind::BallMill, 6.0);
        // 2 mm past the edge, a 3 mm ball touches the edge 3 − √5 up its side.
        // Never lower than the exact drop, at most a cell's slope higher.
        let z = m.drop_cutter(&ball, [12.0, 5.0], -1.0);
        let exact = 5.0 - (3.0 - 5f64.sqrt());
        assert!(z >= exact - 1e-6 && z < exact + 0.1, "{z} {exact}");
    }

    #[test]
    fn cut_removes_slot_volume() {
        let stock = Aabb::new([0.0, 0.0, 0.0], [40.0, 20.0, 10.0]);
        let mut m = HeightMap::block(&stock, 0.1);
        let before = m.volume_above(0.0);
        let t = Tool::new(ToolKind::EndMill, 6.0);
        let removed = m.cut(&t, [5.0, 10.0, 8.0], [35.0, 10.0, 8.0]);
        // A 30 mm slot 6 wide with round ends, 2 deep.
        let expect = (30.0 * 6.0 + std::f64::consts::PI * 9.0) * 2.0;
        assert!(
            (removed - expect).abs() / expect < 0.03,
            "{removed} vs {expect}"
        );
        assert!((before - m.volume_above(0.0) - removed).abs() < 1.0);
        assert!((m.sample([20.0, 10.0]) - 8.0).abs() < 1e-6);
    }

    /// A 20 × 10 × 10 block with a 5 deep slot along X cut from above, then flipped over
    /// (about X): the slot is now at the bottom, the old bottom the flat top; a cut down
    /// through the thin floor over the slot empties those columns.
    #[test]
    fn a_flipped_stock_keeps_its_columns() {
        use crate::geom::Frame;
        let mut m = HeightMap::block(&Aabb::new([0.0, 0.0, 0.0], [20.0, 10.0, 10.0]), 0.5);
        let tool = Tool::new(crate::tool::ToolKind::EndMill, 4.0);
        m.cut(&tool, [0.0, 5.0, 5.0], [20.0, 5.0, 5.0]);
        let up = Frame::WORLD;
        let flipped = Frame::from_zx([0.0, 0.0, 10.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
        // In the flipped frame: x the same, y = −y, z = 10 − z.
        let area = Aabb::new([0.0, -10.0, 0.0], [20.0, 0.0, 10.0]);
        let f = m.reframed(&up, 0.0, &flipped, &area, 0.5).unwrap();
        assert!(
            (f.sample([10.0, -5.0]) - 10.0).abs() < 1e-4,
            "the old bottom on top"
        );
        let fl = f.floor.as_ref().unwrap();
        let (i, j) = f.cell_of([10.0, -5.0]).unwrap();
        assert!(
            (fl[f.index(i, j)] - 5.0).abs() < 1e-4,
            "the slot under 5 of material"
        );
        let (i, j) = f.cell_of([10.0, -9.5]).unwrap();
        assert!(
            fl[f.index(i, j)].abs() < 1e-4,
            "beside the slot, the full 10"
        );
        // Facing down 6 over the slot goes through: no material left there.
        let mut f2 = f.clone();
        f2.cut(&tool, [8.0, -5.0, 4.0], [12.0, -5.0, 4.0]);
        assert_eq!(f2.sample([10.0, -5.0]), EMPTY);
        assert!((f2.sample([10.0, -9.5]) - 10.0).abs() < 1e-4);
        // A tilt can't be carried.
        let side = Frame::from_zx([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(m.reframed(&up, 0.0, &side, &area, 0.5).is_none());
    }
}
