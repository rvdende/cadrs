//! Verify: the stock as the toolpaths leave it, and what went wrong on the way.
//!
//! - **Mills and routers** ([`simulate_mill`]): the stock is a height map lowered under each
//!   cutter along its feed moves. Rapids that would run into material, cuts deeper than the
//!   tool's flutes (the holder or shank rubbing) and cells cut below the part (gouges) are
//!   reported, with the material left over the part (excess).
//! - **Beam machines** ([`simulate_sheet`]): the kerf is the area swept by the beam along its
//!   cuts; the sheet minus the kerf falls into pieces. Kerf over the part is damage; pieces that
//!   are parts come free.

use serde::{Deserialize, Serialize};

use crate::geom::{P2, P3, dist2};
use crate::heightmap::{EMPTY, HeightMap};
use crate::model::Model;
use crate::region::{self, Region};
use crate::strategy::Stock;
use crate::tool::Tool;
use crate::toolpath::Segment;

/// What a mill simulation found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MillResult {
    /// The in-process stock after the operations.
    pub stock: HeightMap,
    /// Volume removed (mm³).
    pub removed: f64,
    /// Where cuts went under the part (up to [`MAX_REPORTED`]), and how many cells.
    pub gouges: Vec<P3>,
    pub gouge_cells: usize,
    /// The deepest gouge (mm).
    pub gouge_depth: f64,
    /// Rapids into material: (operation, where).
    pub rapid_hits: Vec<(usize, P3)>,
    /// Cuts with more material over the tip than the flutes are long, or with the holder
    /// (or arbor) meeting the stock: (operation, where).
    pub holder_hits: Vec<(usize, P3)>,
    /// Moves (rapids too) with the tool or its holder in the workholding: (operation, where).
    pub fixture_hits: Vec<(usize, P3)>,
    /// The most material left on the part (mm).
    pub excess: f64,
}

pub const MAX_REPORTED: usize = 200;

/// One operation to simulate: its tool and its motion.
pub struct SimOp<'a> {
    pub tool: &'a Tool,
    pub segments: &'a [Segment],
}

/// The cell size a simulation of this stock uses for a `quality` from 0 (fast) to 1 (fine).
pub fn sim_cell(stock: &Stock, quality: f64) -> f64 {
    let s = stock.bounds.size();
    let area = (s[0] * s[1]).max(1.0);
    // From about 250k cells to about 2.5M.
    let cells = 250_000.0 + 2_250_000.0 * quality.clamp(0.0, 1.0);
    (area / cells).sqrt().max(0.02)
}

/// The highest stock in a disc (the tool's footprint).
fn max_in_disc(map: &HeightMap, c: P2, r: f64) -> f32 {
    let n = (r / map.cell).ceil() as isize;
    let Some((ci, cj)) = map.cell_of(c) else {
        return EMPTY;
    };
    let mut m = EMPTY;
    for dj in -n..=n {
        for di in -n..=n {
            if ((di * di + dj * dj) as f64).sqrt() * map.cell > r {
                continue;
            }
            let (i, j) = (ci as isize + di, cj as isize + dj);
            if i < 0 || j < 0 || i >= map.nx as isize || j >= map.ny as isize {
                continue;
            }
            m = m.max(map.at(i as usize, j as usize));
        }
    }
    m
}

/// Runs the operations over the stock (see the module docs).
pub fn simulate_mill(stock: &Stock, model: &Model, ops: &[SimOp], cell: f64) -> MillResult {
    simulate_mill_with(stock, model, &[], ops, cell)
}

/// [`simulate_mill`] with the workholding's triangles: any move taking the tool or its holder
/// into them is a collision.
pub fn simulate_mill_with(
    stock: &Stock,
    model: &Model,
    fixtures: &[[P3; 3]],
    ops: &[SimOp],
    cell: f64,
) -> MillResult {
    simulate_mill_on(stock.height_map(cell), model, fixtures, ops)
}

/// [`simulate_mill_with`] from a stock already cut (`map`: what the operations before left).
pub fn simulate_mill_on(
    mut map: HeightMap,
    model: &Model,
    fixtures: &[[P3; 3]],
    ops: &[SimOp],
) -> MillResult {
    let fixture_idx = (!fixtures.is_empty()).then(|| crate::dropcutter::TriIndex::new(fixtures));
    let mut fixture_hits = Vec::new();
    let mut scratch = Vec::new();
    // The part on exactly the stock's grid.
    let mut part = map.blank();
    part.rasterize(&model.triangles, false);
    let mut removed = 0.0;
    let mut rapid_hits = Vec::new();
    let mut holder_hits = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        let r = op.tool.radius();
        let flutes = op.tool.cutting_length.max(0.1);
        // The cutter for the rapids' check (see there).
        let mut lean = op.tool.clone();
        lean.diameter =
            (lean.diameter - map.cell * std::f64::consts::FRAC_1_SQRT_2).max(lean.diameter * 0.5);
        lean.corner_radius = lean.corner_radius.min(lean.diameter / 2.0);
        // The holder and arbor: discs above the stick-out (the shank is the flutes' check).
        let holder: Vec<(f64, f64)> = crate::strategy::collision::assembly_discs(
            op.tool,
            &crate::strategy::collision::Collision {
                shank_clearance: 0.0,
                holder_clearance: 0.0,
                ..Default::default()
            },
        )
        .into_iter()
        .filter(|d| d.1 >= op.tool.overall_length - 1e-9)
        .collect();
        // Against the workholding: the cutter (a hair smaller: running along it is fine) and
        // the holder's discs, all the way along every move.
        if let Some(fidx) = &fixture_idx {
            let mut cutter = op.tool.clone();
            cutter.diameter = (cutter.diameter - 0.002).max(1e-3);
            if cutter.kind == crate::tool::ToolKind::BallMill {
                cutter.corner_radius = cutter.diameter / 2.0;
            }
            let discs: Vec<(Tool, f64)> = holder
                .iter()
                .map(|(hr, hh)| (Tool::new(crate::tool::ToolKind::EndMill, 2.0 * hr), *hh))
                .collect();
            let step = (r / 2.0).clamp(0.05, 1.0);
            for s in op.segments {
                let n =
                    ((crate::geom::dist(s.from, s.to) / step).ceil() as usize).clamp(1, 100_000);
                for i in 0..=n {
                    let p = crate::geom::lerp(s.from, s.to, i as f64 / n as f64);
                    let into = |t: &Tool, h: f64, scratch: &mut Vec<u32>| {
                        crate::dropcutter::drop_point(
                            t,
                            fidx,
                            [p[0], p[1]],
                            f64::NEG_INFINITY,
                            scratch,
                        ) > p[2] + h + 0.01
                    };
                    if into(&cutter, 0.0, &mut scratch)
                        || discs.iter().any(|(t, h)| into(t, *h, &mut scratch))
                    {
                        if fixture_hits.len() < MAX_REPORTED {
                            fixture_hits.push((k, p));
                        }
                        break;
                    }
                }
            }
        }
        for s in op.segments {
            let len =
                dist2([s.from[0], s.from[1]], [s.to[0], s.to[1]]).max((s.to[2] - s.from[2]).abs());
            let n = ((len / map.cell).ceil() as usize).clamp(1, 100_000);
            if s.is_rapid() {
                // The cutter's own shape running into material anywhere along it (straight up
                // too: up through a core left standing is a crash). Narrower by the stamp's
                // own error (a cell's half diagonal across): the map can't place a wall closer,
                // and a rapid tangent to one (a wall's stock) isn't a hit.
                for i in 0..=n {
                    let t = i as f64 / n as f64;
                    let p = crate::geom::lerp(s.from, s.to, t);
                    if map.cuts_into(&lean, p, 0.01) {
                        if rapid_hits.len() < MAX_REPORTED {
                            rapid_hits.push((k, p));
                        }
                        break;
                    }
                }
                continue;
            }
            // The flutes must reach over the material standing where the tool goes: checked
            // piece by piece as the move cuts its way, so a pass down a wall (whose flutes
            // clear the wall above them as they go) isn't taken for the shank rubbing it.
            let len3 = crate::geom::dist(s.from, s.to);
            let pieces = ((len3 / (flutes / 4.0).max(map.cell)).ceil() as usize).clamp(1, 10_000);
            let mut hit = false;
            let mut from = s.from;
            for i in 1..=pieces {
                let p = crate::geom::lerp(s.from, s.to, i as f64 / pieces as f64);
                if !hit {
                    let top = max_in_disc(&map, [p[0], p[1]], r * 0.95);
                    let holder_hit = holder.iter().any(|(hr, hh)| {
                        let t = max_in_disc(&map, [p[0], p[1]], *hr);
                        t != EMPTY && t as f64 > p[2] + hh + 0.01
                    });
                    if holder_hit || (top != EMPTY && top as f64 - p[2] > flutes + 0.01) {
                        if holder_hits.len() < MAX_REPORTED {
                            holder_hits.push((k, p));
                        }
                        hit = true;
                    }
                }
                removed += map.cut(op.tool, from, p);
                from = p;
            }
        }
    }
    // Gouges and excess against the part.
    let mut gouges = Vec::new();
    let mut gouge_cells = 0;
    let mut gouge_depth = 0.0f64;
    let mut excess = 0.0f64;
    let tol = (map.cell * 0.5).max(0.01) as f32;
    // A gouge is under the part pulled in by a cell: the meshes' chords and a wall's place
    // within its cell aren't gouges.
    let solid = part.eroded();
    for j in 0..map.ny {
        for i in 0..map.nx {
            let (s, p) = (map.at(i, j), part.at(i, j));
            if p == EMPTY || s == EMPTY {
                continue;
            }
            let q = solid.at(i, j);
            if q != EMPTY && s < q - tol {
                gouge_cells += 1;
                gouge_depth = gouge_depth.max((q - s) as f64);
                if gouges.len() < MAX_REPORTED {
                    let q = map.point(i, j);
                    gouges.push([q[0], q[1], s as f64]);
                }
            } else {
                excess = excess.max((s - p) as f64);
            }
        }
    }
    MillResult {
        stock: map,
        removed,
        gouges,
        gouge_cells,
        gouge_depth,
        rapid_hits,
        holder_hits,
        fixture_hits,
        excess,
    }
}

/// [`simulate_mill_on`] for a run with tilted operations (3+2): `frames[k]` is operation
/// `k`'s own frame in the map's coordinates (`None`: along the map's Z). The operations along
/// the map's Z cut it as usual; a tilted one cuts the stock as voxels seen down its own Z (its
/// rapids and holder checked on that), carved back into the map. Positions found come back in
/// the map's coordinates. Gouges are the setup's operations' only: check the tilted ones
/// against the part with [`check_against_part`].
pub fn simulate_mill_tilted(
    start: HeightMap,
    bottom: f32,
    model: &Model,
    fixtures: &[[P3; 3]],
    ops: &[SimOp],
    frames: &[Option<crate::geom::Frame>],
) -> MillResult {
    let mut cur = start;
    let mut removed = 0.0;
    let (mut rapid_hits, mut holder_hits, mut fixture_hits) = (vec![], vec![], vec![]);
    let mut k = 0;
    while k < ops.len() {
        let Some(f) = frames.get(k).copied().flatten() else {
            // A run of operations along the map's Z.
            let end = (k..ops.len())
                .find(|&e| frames.get(e).copied().flatten().is_some())
                .unwrap_or(ops.len());
            let group: Vec<SimOp> = ops[k..end]
                .iter()
                .map(|o| SimOp {
                    tool: o.tool,
                    segments: o.segments,
                })
                .collect();
            let r = simulate_mill_on(cur, model, fixtures, &group);
            removed += r.removed;
            rapid_hits.extend(r.rapid_hits.iter().map(|(i, p)| (i + k, *p)));
            holder_hits.extend(r.holder_hits.iter().map(|(i, p)| (i + k, *p)));
            fixture_hits.extend(r.fixture_hits.iter().map(|(i, p)| (i + k, *p)));
            cur = r.stock;
            k = end;
            continue;
        };
        // A tilted operation: the stock as voxels, seen down its Z.
        let top = cur
            .z
            .iter()
            .copied()
            .filter(|z| *z != EMPTY)
            .fold(bottom, f32::max);
        let longest = (cur.nx.max(cur.ny) as f64 * cur.cell).max((top - bottom) as f64);
        let vcell = (longest / 200.0).min(1.0).max(cur.cell);
        let mut vox = crate::voxels::Voxels::of_height_map(&cur, bottom, vcell);
        let lo = cur.point(0, 0);
        let hi = cur.point(cur.nx - 1, cur.ny - 1);
        let corners = (0..8).map(|c| {
            f.to_local([
                if c & 1 == 0 { lo[0] } else { hi[0] },
                if c & 2 == 0 { lo[1] } else { hi[1] },
                if c & 4 == 0 {
                    bottom as f64
                } else {
                    top as f64
                },
            ])
        });
        let b = crate::geom::Aabb::of(corners);
        let pad = 2.0 * vcell;
        let area = crate::geom::Aabb::new(
            [b.min[0] - pad, b.min[1] - pad, b.min[2]],
            [b.max[0] + pad, b.max[1] + pad, b.max[2]],
        );
        let map = vox.height_map(&f, &area, vcell);
        let segs: Vec<Segment> = ops[k]
            .segments
            .iter()
            .map(|s| Segment {
                from: f.to_local(s.from),
                to: f.to_local(s.to),
                ..*s
            })
            .collect();
        // No part on this map: its gouges are checked exactly elsewhere.
        let r = simulate_mill_on(
            map,
            &Model::from_triangles("none", Vec::new()),
            &[],
            &[SimOp {
                tool: ops[k].tool,
                segments: &segs,
            }],
        );
        removed += r.removed;
        rapid_hits.extend(r.rapid_hits.iter().map(|(_, p)| (k, f.to_world(*p))));
        holder_hits.extend(r.holder_hits.iter().map(|(_, p)| (k, f.to_world(*p))));
        vox.carve(&f, &r.stock);
        // Back onto the map: lower only where the voxels lost material.
        let seen = vox.height_map(
            &crate::geom::Frame::WORLD,
            &crate::geom::Aabb::new([lo[0], lo[1], bottom as f64], [hi[0], hi[1], top as f64]),
            cur.cell,
        );
        for j in 0..cur.ny {
            for i in 0..cur.nx {
                let n = cur.index(i, j);
                let v = seen.sample(cur.point(i, j));
                if cur.z[n] != EMPTY && (v == EMPTY || v < cur.z[n]) {
                    cur.z[n] = v;
                    if v == EMPTY
                        && let Some(fl) = cur.floor.as_mut()
                    {
                        fl[n] = EMPTY;
                    }
                }
            }
        }
        k += 1;
    }
    // The gouges and excess on the final stock.
    let tail = simulate_mill_on(cur, model, &[], &[]);
    MillResult {
        removed,
        rapid_hits,
        holder_hits,
        fixture_hits,
        ..tail
    }
}

/// What checking an operation against the part (not the stock) found: for operations a height
/// map along the setup's Z can't cut (a tilted work plane, 3+2), in their own coordinates.
#[derive(Debug, Clone, Default)]
pub struct PartCheck {
    /// Where cuts went into the part (beyond `tol`), and the deepest (mm).
    pub gouges: Vec<P3>,
    pub gouge_depth: f64,
    /// Rapids (other than straight up) through the part.
    pub rapid_hits: Vec<P3>,
    /// The shank (above the flutes), holder or arbor into the part.
    pub holder_hits: Vec<P3>,
    /// Any of the tool or its holder into the workholding.
    pub fixture_hits: Vec<P3>,
}

/// Checks an operation's moves against the part's triangles and the workholding's (all in the
/// moves' coordinates), with the exact drop-cutter: cuts more than `tol` into the part,
/// rapids into it, the shank and holder meeting it, and anything meeting the workholding.
pub fn check_against_part(
    part: &[[P3; 3]],
    fixtures: &[[P3; 3]],
    tool: &Tool,
    segments: &[Segment],
    tol: f64,
) -> PartCheck {
    use crate::dropcutter::{TriIndex, drop_point};
    use crate::strategy::collision::{Collision, assembly_discs};
    let mut out = PartCheck::default();
    let idx = TriIndex::new(part);
    let fidx = (!fixtures.is_empty()).then(|| TriIndex::new(fixtures));
    let mut cutter = tool.clone();
    cutter.diameter = (cutter.diameter - 0.002).max(1e-3);
    if cutter.kind == crate::tool::ToolKind::BallMill {
        cutter.corner_radius = cutter.diameter / 2.0;
    }
    let discs: Vec<(Tool, f64)> = assembly_discs(
        tool,
        &Collision {
            shank_clearance: 0.0,
            holder_clearance: 0.0,
            ..Default::default()
        },
    )
    .into_iter()
    .map(|(r, h)| {
        (
            Tool::new(crate::tool::ToolKind::EndMill, 2.0 * (r - 1e-3).max(1e-3)),
            h,
        )
    })
    .collect();
    let mut scratch = Vec::new();
    let step = (tool.radius() / 2.0).clamp(0.05, 0.5);
    let push = |v: &mut Vec<P3>, p: P3| {
        if v.len() < MAX_REPORTED {
            v.push(p);
        }
    };
    for s in segments {
        let up = s.is_rapid()
            && dist2([s.from[0], s.from[1]], [s.to[0], s.to[1]]) < 1e-9
            && s.to[2] >= s.from[2];
        if up {
            continue;
        }
        let n = ((crate::geom::dist(s.from, s.to) / step).ceil() as usize).clamp(1, 100_000);
        let (mut cut_hit, mut holder_hit, mut fixture_hit) = (false, false, false);
        for i in 0..=n {
            let p = crate::geom::lerp(s.from, s.to, i as f64 / n as f64);
            let xy = [p[0], p[1]];
            let low = drop_point(&cutter, &idx, xy, f64::NEG_INFINITY, &mut scratch);
            if !cut_hit && low > p[2] + tol {
                cut_hit = true;
                if s.is_rapid() {
                    push(&mut out.rapid_hits, p);
                } else {
                    push(&mut out.gouges, p);
                    out.gouge_depth = out.gouge_depth.max(low - p[2]);
                }
            }
            if !holder_hit
                && discs.iter().any(|(t, h)| {
                    *h >= tool.cutting_length - 1e-9
                        && drop_point(t, &idx, xy, f64::NEG_INFINITY, &mut scratch)
                            > p[2] + h + 0.01
                })
            {
                holder_hit = true;
                push(&mut out.holder_hits, p);
            }
            if let Some(f) = &fidx
                && !fixture_hit
                && (drop_point(&cutter, f, xy, f64::NEG_INFINITY, &mut scratch) > p[2] + 0.01
                    || discs.iter().any(|(t, h)| {
                        drop_point(t, f, xy, f64::NEG_INFINITY, &mut scratch) > p[2] + h + 0.01
                    }))
            {
                fixture_hit = true;
                push(&mut out.fixture_hits, p);
            }
        }
    }
    out
}

/// What a sheet simulation found.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetResult {
    /// The area the beam burned or blasted away.
    pub kerf: Region,
    /// The sheet's pieces after cutting.
    pub pieces: Vec<region::Face>,
    /// Area of the part the kerf took (mm²): cuts into the part.
    pub damage: f64,
    /// Pieces that are a part (inside its outline), cut free.
    pub parts_free: usize,
    /// Which of `pieces` are a part cut free (the rest is scrap or the skeleton).
    pub part_pieces: Vec<bool>,
    /// Total cut length (mm).
    pub cut_length: f64,
}

/// Runs beam cuts over a sheet (see the module docs).
pub fn simulate_sheet(stock: &Stock, model: &Model, ops: &[SimOp]) -> SheetResult {
    let mut strokes: Vec<Region> = Vec::new();
    let mut cut_length = 0.0;
    for op in ops {
        let w = op.tool.diameter;
        let mut run: Vec<P2> = Vec::new();
        let flush = |run: &mut Vec<P2>, strokes: &mut Vec<Region>| {
            if run.len() >= 2 {
                strokes.push(region::stroke(run, w));
            }
            run.clear();
        };
        for s in op.segments {
            if s.cuts(true) {
                let a = [s.from[0], s.from[1]];
                if run.last().is_none_or(|l| dist2(*l, a) > 1e-9) {
                    flush(&mut run, &mut strokes);
                    run.push(a);
                }
                run.push([s.to[0], s.to[1]]);
                cut_length += dist2(a, [s.to[0], s.to[1]]);
            } else {
                flush(&mut run, &mut strokes);
            }
        }
        flush(&mut run, &mut strokes);
    }
    let kerf = region::union_regions(strokes);
    let sheet = stock.footprint();
    let left = region::difference(&sheet, &kerf);
    let pieces = region::faces(&left);
    let sil = model.silhouette();
    // Within 0.02 mm of the outline is the meshes' own chordal error, not a cut into the part.
    let damage = region::area(&region::intersection(&kerf, &region::offset(&sil, -0.02)));
    // A part piece lies within the part's outline (allowing for the arc tolerance).
    let grown = region::offset(&sil, 0.05);
    let part_pieces: Vec<bool> = pieces
        .iter()
        .map(|f| {
            let inside = region::area(&region::intersection(&f.rings(), &grown));
            (inside - f.area()).abs() < f.area() * 1e-3 + 1e-3
        })
        .collect();
    let parts_free = part_pieces.iter().filter(|p| **p).count();
    SheetResult {
        kerf,
        pieces,
        damage,
        parts_free,
        part_pieces,
        cut_length,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb;
    use crate::machine::MachineKind;
    use crate::model::box_triangles;
    use crate::strategy::{Input, Params, Pattern, generate};
    use crate::tool::ToolKind;

    #[test]
    fn mill_roughing_leaves_stock_and_no_gouges() {
        let m = crate::strategy::testing::pocket_block();
        let stock = crate::strategy::testing::stock_for(&m, 3.0, 1.0);
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let mut p = Params::new(Pattern::Rough3D, &tool, MachineKind::Mill);
        p.step_down = 3.0;
        p.wall_stock = 0.3;
        p.floor_stock = 0.3;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let segs = tp.motion([0.0, 0.0, 40.0], 5000.0);
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
            0.25,
        );
        assert_eq!(r.gouge_cells, 0, "{:?}", &r.gouges[..r.gouges.len().min(5)]);
        assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
        assert!(r.removed > 1000.0);
        // The pocket is roughed to about the floor stock.
        let s = r.stock.sample([30.0, 20.0]);
        assert!((s - 12.3).abs() < 0.35, "{s}");
    }

    /// A multi-depth profile round a block with a square hole: leads only where the tool can
    /// come down, lead-outs the way the tool is going; Verify finds nothing wrong.
    #[test]
    fn profile_round_a_hole_is_clean() {
        let mut t = Vec::new();
        t.extend(box_triangles(&Aabb::new(
            [0.0, 0.0, 0.0],
            [20.0, 40.0, 10.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [40.0, 0.0, 0.0],
            [60.0, 40.0, 10.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [20.0, 0.0, 0.0],
            [40.0, 10.0, 10.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [20.0, 30.0, 0.0],
            [40.0, 40.0, 10.0],
        )));
        let m = Model::from_triangles("block", t);
        let stock = Stock::block(Aabb::new([-2.0, -2.0, 0.0], [62.0, 42.0, 11.0]));
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.step_down = 3.0;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let segs = tp.motion([0.0, 0.0, 40.0], 5000.0);
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
            0.1,
        );
        assert_eq!(r.gouge_cells, 0, "{:?}", &r.gouges[..r.gouges.len().min(4)]);
        assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
        // The hole's walls are cut to the bottom.
        assert!(r.stock.sample([21.0, 20.0]) < 0.01);
    }

    #[test]
    fn a_plunge_through_the_part_gouges() {
        let m = Model::from_triangles(
            "b",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 10.0])),
        );
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 12.0]));
        let tool = Tool::new(ToolKind::EndMill, 4.0);
        let segs = vec![
            Segment {
                from: [10.0, 10.0, 20.0],
                to: [10.0, 10.0, 15.0],
                feed: None,
                kind: None,
                beam: false,
                minutes: 0.0,
            },
            Segment {
                from: [10.0, 10.0, 15.0],
                to: [10.0, 10.0, 5.0],
                feed: Some(100.0),
                kind: None,
                beam: false,
                minutes: 0.1,
            },
            Segment {
                from: [10.0, 10.0, 5.0],
                to: [15.0, 15.0, 3.0],
                feed: None,
                kind: None,
                beam: false,
                minutes: 0.0,
            },
        ];
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
            0.2,
        );
        assert!(r.gouge_cells > 0);
        assert!((r.gouge_depth - 5.0).abs() < 0.05);
        assert_eq!(r.rapid_hits.len(), 1);
    }

    fn feed(from: P3, to: P3) -> Segment {
        Segment {
            from,
            to,
            feed: Some(500.0),
            kind: None,
            beam: false,
            minutes: 0.01,
        }
    }

    /// Flutes 10 long: down a 40 mm wall along 0.3 mm of wall stock (the flutes clear it as
    /// they go) is fine; slotting 20 deep in one go buries the shank.
    #[test]
    fn flutes_too_short_only_where_the_shank_meets_material() {
        let m = Model::from_triangles(
            "wall",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 40.0])),
        );
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [20.3, 20.0, 40.0]));
        let mut tool = Tool::new(ToolKind::EndMill, 6.0);
        tool.cutting_length = 10.0;
        let down = vec![feed([23.0, 10.0, 40.0], [23.0, 10.0, 0.0])];
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &down,
            }],
            0.1,
        );
        assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [60.0, 20.0, 40.0]));
        let slot = vec![
            feed([40.0, 10.0, 45.0], [40.0, 10.0, 20.0]),
            feed([40.0, 10.0, 20.0], [55.0, 10.0, 20.0]),
        ];
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &slot,
            }],
            0.1,
        );
        assert!(!r.holder_hits.is_empty());
    }

    #[test]
    fn sheet_cut_frees_the_part() {
        let m = Model::from_triangles(
            "p",
            box_triangles(&Aabb::new([10.0, 10.0, 0.0], [50.0, 30.0, 2.0])),
        );
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 2.0]));
        let tool = Tool::new(ToolKind::Laser, 0.2);
        let p = Params::new(Pattern::Cut, &tool, MachineKind::Laser);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Laser,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let segs = tp.motion([0.0, 0.0, 0.0], 5000.0);
        let r = simulate_sheet(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
        );
        assert!(r.damage < 0.05, "{}", r.damage);
        assert_eq!(r.parts_free, 1);
        assert_eq!(r.pieces.len(), 2);
        assert!((r.cut_length - 120.0).abs() < 8.0, "{}", r.cut_length);
    }

    /// A Ø30 holder 20 above the tip: down a hole 25 deep (the flutes 30 long), it sits on
    /// the stock.
    #[test]
    fn holder_meets_the_stock() {
        let m = Model::from_triangles(
            "b",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 1.0])),
        );
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [60.0, 20.0, 40.0]));
        let mut tool = Tool::new(ToolKind::EndMill, 6.0);
        tool.cutting_length = 30.0;
        tool.overall_length = 20.0;
        tool.holder = Some(crate::tool::Profile::cylinder("h", 30.0, 40.0));
        let down = vec![feed([40.0, 10.0, 45.0], [40.0, 10.0, 15.0])];
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &down,
            }],
            0.2,
        );
        assert!(!r.holder_hits.is_empty());
        tool.holder = None;
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &down,
            }],
            0.2,
        );
        assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
    }

    /// Checked against the part in its own coordinates: a pass along the top is clean, one
    /// 2 mm into it gouges, a rapid across it hits, a straight-up retract doesn't.
    #[test]
    fn checks_against_the_part() {
        let tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]));
        let tool = Tool::new(ToolKind::BallMill, 6.0);
        let clean = vec![feed([2.0, 10.0, 10.0], [18.0, 10.0, 10.0])];
        let r = check_against_part(&tris, &[], &tool, &clean, 0.01);
        assert!(r.gouges.is_empty() && r.rapid_hits.is_empty(), "{r:?}");
        let deep = vec![feed([2.0, 10.0, 8.0], [18.0, 10.0, 8.0])];
        let r = check_against_part(&tris, &[], &tool, &deep, 0.01);
        assert!((r.gouge_depth - 2.0).abs() < 1e-3, "{r:?}");
        let mut rapid = feed([-5.0, 10.0, 5.0], [25.0, 10.0, 5.0]);
        rapid.feed = None;
        let mut up = feed([10.0, 10.0, 5.0], [10.0, 10.0, 50.0]);
        up.feed = None;
        let r = check_against_part(&tris, &[], &tool, &[rapid, up], 0.01);
        assert_eq!(r.rapid_hits.len(), 1, "{r:?}");
    }

    /// A ring cut round a core (a D6 at radius 5 leaves a Ø4 pin) then a rapid straight up
    /// from the middle: up through the pin is a crash.
    #[test]
    fn a_rapid_up_through_a_core_is_caught() {
        let m = Model::from_triangles(
            "b",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
        );
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]));
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let mut segs = Vec::new();
        let ring = |a: f64| [10.0 + 5.0 * a.cos(), 10.0 + 5.0 * a.sin(), 5.0];
        for i in 0..64 {
            let (a, b) = (
                std::f64::consts::TAU * i as f64 / 64.0,
                std::f64::consts::TAU * (i + 1) as f64 / 64.0,
            );
            segs.push(feed(ring(a), ring(b)));
        }
        let mut up = feed([10.0, 10.0, 5.0], [10.0, 10.0, 20.0]);
        up.feed = None;
        segs.push(up);
        let r = simulate_mill(
            &stock,
            &m,
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
            0.1,
        );
        assert_eq!(r.rapid_hits.len(), 1, "{:?}", r.rapid_hits);
    }

    /// A 3+2 face on the block's +X end, then a plunge (a rapid) down into where it cut: the
    /// rapid is clear; without the tilted cut it would run into the stock.
    #[test]
    fn a_tilted_cut_clears_the_stock_for_the_next_operation() {
        let mut start = HeightMap::new([0.0, 0.0], [40.0, 40.0], 0.5);
        start.z.iter_mut().for_each(|z| *z = 20.0);
        let none = Model::from_triangles("none", Vec::new());
        let seg = |from: P3, to: P3, feed: bool| Segment {
            from,
            to,
            feed: feed.then_some(500.0),
            kind: feed.then_some(crate::toolpath::FeedKind::Cut),
            beam: false,
            minutes: 0.0,
        };
        // Seen from +X (its X the model's Y, its Y the model's Z, its Z the model's X − 40):
        // passes 5 deep over the end, z 10 and up.
        let f = crate::geom::Frame::from_zx([40.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let mut face = Vec::new();
        for y in [12.0, 16.0, 20.0, 24.0] {
            face.push(seg(
                f.to_world([-8.0, y, -5.0]),
                f.to_world([48.0, y, -5.0]),
                true,
            ));
        }
        // (A millimetre clear of the face's wall: the voxels keep up to one of material.)
        let plunge = [seg([38.0, 20.0, 30.0], [38.0, 20.0, 12.0], false)];
        let face_tool = Tool::new(ToolKind::EndMill, 10.0);
        let tool = Tool::new(ToolKind::EndMill, 4.0);
        let ops = [
            SimOp {
                tool: &face_tool,
                segments: &face,
            },
            SimOp {
                tool: &tool,
                segments: &plunge,
            },
        ];
        let r = simulate_mill_tilted(start.clone(), 0.0, &none, &[], &ops, &[Some(f), None]);
        assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
        assert!(r.removed > 0.5 * 5.0 * 40.0 * 10.0, "{}", r.removed);
        // The end is down to about z 10 now; the middle still 20.
        let end = r.stock.sample([38.0, 20.0]);
        assert!(end < 11.5, "{end}");
        assert!((r.stock.sample([20.0, 20.0]) - 20.0).abs() < 1e-3);
        // The plunge alone hits the stock.
        let r = simulate_mill_tilted(start, 0.0, &none, &[], &ops[1..], &[None]);
        assert_eq!(r.rapid_hits.len(), 1);
    }
}
