//! 3-axis finishing: Parallel (raster passes over the surfaces, each point dropped onto the
//! model by the cutter's shape) and Constant Z (waterlines round the model at levels).

use crate::dropcutter::{TriIndex, drop_line};
use crate::geom::{P2, P3, dist2};
use crate::region::{self, Region};
use crate::toolpath::{FeedKind, Toolpath};

use super::Input;
use super::link::{Builder, Cut};
use super::mill::materials_above;
use super::path::{Contour, hatch, nearest_order, region_contours};
use super::{CuttingMethod, Direction};

/// Drops 3D points within tolerance of each other in a line (fewer, longer moves).
fn simplify3(pts: &[P3], tol: f64) -> Vec<P3> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (pts[a], pts[b]);
        let d = crate::geom::sub(pb, pa);
        let l2 = crate::geom::dot(d, d).max(1e-18);
        let mut worst = (0.0, a);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let t = (crate::geom::dot(crate::geom::sub(*p, pa), d) / l2).clamp(0.0, 1.0);
            let q = crate::geom::add(pa, crate::geom::scale(d, t));
            let e = crate::geom::dist(*p, q);
            if e > worst.0 {
                worst = (e, i);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    pts.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

/// The finishing area: the picked faces' footprint, else the model's, within the stock.
fn finish_area(input: &Input) -> Region {
    let r = input.tool.radius();
    let d = &input.params.drive;
    let base = if d.faces.is_empty() {
        input.model.silhouette()
    } else {
        let tris = d
            .faces
            .iter()
            .filter_map(|n| input.model.face(n))
            .flat_map(|f| f.triangles.iter())
            .map(|&t| {
                input.model.triangles[t as usize]
                    .iter()
                    .map(|p| [p[0], p[1]])
                    .collect::<Vec<P2>>()
            });
        region::union_all(tris)
    };
    region::intersection(
        &region::offset(&base, r * 0.5),
        &region::offset(&input.stock.footprint(), r),
    )
}

/// Parallel: raster lines over the area at the angle, each sampled and dropped onto the model.
pub fn parallel(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let area = finish_area(input);
    if area.is_empty() {
        return Err("Nothing to finish inside the stock".into());
    }
    let zig = p.cutting_method == CuttingMethod::Zigzag;
    let lines = hatch(&area, p.angle, p.step_over, zig);
    follow(input, lines, zig)
}

/// Spiral: one spiral from the area's middle outward, a step over apart each turn, dropped
/// onto the model (no links but where it leaves the area): for round, shallow shapes.
pub fn spiral(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let area = finish_area(input);
    if area.is_empty() {
        return Err("Nothing to finish inside the stock".into());
    }
    let b = crate::geom::Aabb::of(area.iter().flatten().map(|q| [q[0], q[1], 0.0]));
    let c = [(b.min[0] + b.max[0]) / 2.0, (b.min[1] + b.max[1]) / 2.0];
    let reach = area
        .iter()
        .flatten()
        .map(|q| dist2(*q, c))
        .fold(0.0, f64::max);
    // r = step·θ/2π, sampled so each chord strays less than the tolerance.
    let step = p.step_over;
    let mut pts = vec![c];
    let mut theta: f64 = 0.0;
    while step * theta / std::f64::consts::TAU < reach + step {
        let r = (step * theta / std::f64::consts::TAU).max(step * 0.25);
        let dth = (8.0 * p.tolerance.max(0.001) / r).sqrt().clamp(0.005, 0.5);
        theta += dth;
        let r = step * theta / std::f64::consts::TAU;
        // Climb: material outside the growing spiral on the right, so clockwise.
        let a = if p.direction == Direction::Climb {
            -theta
        } else {
            theta
        };
        pts.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
    }
    let pieces = region::clip_open(&[pts], &area);
    follow(input, pieces, true)
}

/// Scallop: rings offset in from the area's edge a step over apart, dropped onto the model,
/// so the step over (and the cusp it leaves) stays even along the shapes on shallow ground.
/// The area is where the surface is shallower than the steep angle: walls are Constant Z's.
pub fn scallop(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let shallow = shallow_area(input);
    let area = region::intersection(&finish_area(input), &shallow);
    if area.is_empty() {
        return Err("No surface is shallower than the steep angle".into());
    }
    let mut rings = Vec::new();
    let mut cur = area.clone();
    for _ in 0..10_000 {
        if cur.is_empty() {
            break;
        }
        // Climb: the material inward of each ring on its right, so clockwise outer rings.
        for c in super::mill::orient(region_contours(&cur), p.direction, false) {
            rings.push(c.walk());
        }
        cur = region::simplify(
            &region::offset(&cur, -p.step_over),
            region::ARC_TOLERANCE * 0.5,
        );
    }
    follow(input, rings, false)
}

/// Where the model's surface is shallower than the steep angle, seen from above.
fn shallow_area(input: &Input) -> Region {
    let cos = input.params.steep_angle.clamp(0.0, 90.0).to_radians().cos();
    let tris = input.model.triangles.iter().filter_map(|t| {
        let n = crate::geom::cross(crate::geom::sub(t[1], t[0]), crate::geom::sub(t[2], t[0]));
        let l = crate::geom::len(n);
        (l > 1e-12 && n[2] / l >= cos - 1e-9).then(|| t.iter().map(|q| [q[0], q[1]]).collect())
    });
    // (Grown by half the cutter's radius: its centre runs past a shallow face's edge.)
    region::offset(&region::union_all(tris), input.tool.radius() * 0.5)
}

/// Pencil: along the model's concave corners and fillets tighter than the tool, where it
/// touches two surfaces at once and the passes over them leave the most: the creases of the
/// surface the cutter's tip rides on (its drop-cutter surface), found on a grid and chained.
pub fn pencil(input: &Input) -> Result<Toolpath, String> {
    let area = finish_area(input);
    if area.is_empty() {
        return Err("Nothing to finish inside the stock".into());
    }
    let idx = TriIndex::new(&input.model.triangles);
    let r = input.tool.radius();
    let g = (r / 4.0).clamp(0.05, 0.5);
    let bb = crate::geom::Aabb::of(area.iter().flatten().map(|q| [q[0], q[1], 0.0]));
    let nx = ((bb.max[0] - bb.min[0]) / g).ceil() as usize + 1;
    let ny = ((bb.max[1] - bb.min[1]) / g).ceil() as usize + 1;
    if nx * ny > 4_000_000 {
        return Err("The area is too big for pencil passes at this tool size".into());
    }
    let floor = input.bottom() - 1e3;
    let pts: Vec<P2> = (0..ny)
        .flat_map(|j| (0..nx).map(move |i| [bb.min[0] + i as f64 * g, bb.min[1] + j as f64 * g]))
        .collect();
    let zs = crate::dropcutter::drop_points(input.tool, &idx, &pts, floor);
    let z = |i: usize, j: usize| zs[j * nx + i];
    // A crease: the tip's surface bends up on both sides by more than the threshold (its
    // slopes either side differing by over 20°), most across one direction.
    let thresh = g * (20f64).to_radians().tan();
    let mut bend = vec![0.0f64; nx * ny];
    let dirs: [(isize, isize, f64); 4] = [
        (1, 0, 1.0),
        (0, 1, 1.0),
        (1, 1, std::f64::consts::SQRT_2),
        (1, -1, std::f64::consts::SQRT_2),
    ];
    let mut across = vec![0usize; nx * ny];
    for j in 1..ny - 1 {
        for i in 1..nx - 1 {
            let c = z(i, j);
            for (k, (dx, dy, l)) in dirs.iter().enumerate() {
                let a = z((i as isize - dx) as usize, (j as isize - dy) as usize);
                let b = z((i as isize + dx) as usize, (j as isize + dy) as usize);
                // Both sides up (a valley), by how much the slope turns.
                if a > c && b > c {
                    let d = (a + b - 2.0 * c) / l;
                    if d > bend[j * nx + i] {
                        bend[j * nx + i] = d;
                        across[j * nx + i] = k;
                    }
                }
            }
        }
    }
    // Keep the crease's middle: a cell bending most across its direction.
    let mut keep = vec![false; nx * ny];
    for j in 1..ny - 1 {
        for i in 1..nx - 1 {
            let v = bend[j * nx + i];
            if v < thresh {
                continue;
            }
            let (dx, dy, _) = dirs[across[j * nx + i]];
            let a = bend[((j as isize - dy) as usize) * nx + (i as isize - dx) as usize];
            let b = bend[((j as isize + dy) as usize) * nx + (i as isize + dx) as usize];
            let q = pts[j * nx + i];
            keep[j * nx + i] = v >= a && v >= b && region::contains(&area, q);
        }
    }
    // Chain neighbouring kept cells into lines.
    let mut used = vec![false; nx * ny];
    let mut lines: Vec<Vec<P2>> = Vec::new();
    let neighbours = |i: usize, j: usize| {
        let mut v = Vec::with_capacity(8);
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                let (a, b) = (i as isize + dx, j as isize + dy);
                if (dx, dy) != (0, 0) && a >= 0 && b >= 0 && (a as usize) < nx && (b as usize) < ny
                {
                    v.push((a as usize, b as usize));
                }
            }
        }
        v
    };
    let degree = |i: usize, j: usize, keep: &[bool]| {
        neighbours(i, j)
            .iter()
            .filter(|(a, b)| keep[b * nx + a])
            .count()
    };
    // Ends first (cells with one neighbour), then what's left (loops).
    let mut starts: Vec<(usize, usize)> = (0..nx * ny)
        .filter(|k| keep[*k])
        .map(|k| (k % nx, k / nx))
        .collect();
    starts.sort_by_key(|(i, j)| degree(*i, *j, &keep) != 1);
    for (i0, j0) in starts {
        if used[j0 * nx + i0] {
            continue;
        }
        let mut line = vec![pts[j0 * nx + i0]];
        used[j0 * nx + i0] = true;
        let (mut i, mut j) = (i0, j0);
        while let Some((a, b)) = neighbours(i, j)
            .into_iter()
            .filter(|(a, b)| keep[b * nx + a] && !used[b * nx + a])
            // Straight on first (orthogonal neighbours before diagonal ones).
            .min_by_key(|(a, b)| (*a != i) as u8 + (*b != j) as u8)
        {
            used[b * nx + a] = true;
            line.push(pts[b * nx + a]);
            (i, j) = (a, b);
        }
        if line.len() >= 3 && polyline_len(&line) >= r {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return Err("The model has no concave corners tighter than the tool".into());
    }
    let start = [bb.min[0], bb.min[1]];
    let ordered: Vec<Vec<P2>> =
        nearest_order(lines.into_iter().map(Contour::open).collect(), start)
            .into_iter()
            .map(|c| c.pts)
            .collect();
    follow(input, ordered, false)
}

/// A cutter whose surface is `tool`'s grown by `s` all round (a flat end's becomes a bull
/// nose with corner `s`), and how far its tip sits under `tool`'s: dropped onto the model, it
/// leaves `s` of stock along the surface's normal, on walls as on floors. Cones are left as
/// they are (`s` straight up).
pub(super) fn offset_cutter(tool: &crate::tool::Tool, s: f64) -> (crate::tool::Tool, f64) {
    use crate::tool::ToolKind::*;
    let mut t = tool.clone();
    if s <= 0.0 {
        return (t, 0.0);
    }
    match tool.kind {
        BallMill => {
            t.diameter += 2.0 * s;
            t.corner_radius = t.diameter / 2.0;
        }
        EndMill | FaceMill => {
            t.kind = BullMill;
            t.diameter += 2.0 * s;
            t.corner_radius = s;
        }
        BullMill => {
            t.diameter += 2.0 * s;
            t.corner_radius += s;
        }
        _ => return (t, s),
    }
    // The grown cutter's tip is `s` under the true one's (its surface `s` out along the
    // normal everywhere, the bottom too).
    (t, s)
}

fn polyline_len(pts: &[P2]) -> f64 {
    pts.windows(2).map(|w| dist2(w[0], w[1])).sum()
}

/// Runs the tool along each line, every point dropped onto the model: down at the first line
/// and wherever the next starts far off, else (`link_close`) along the surface from the last
/// line's end.
fn follow(input: &Input, lines: Vec<Vec<P2>>, link_close: bool) -> Result<Toolpath, String> {
    let p = input.params;
    let idx = TriIndex::new(&input.model.triangles);
    let bottom = input.bottom();
    let top = input.top();
    // Samples every half radius at most, split where the surface needs it (exact drops).
    let step = (input.tool.radius() / 2.0).clamp(0.05, 1.0);
    let min_step = (p.tolerance * 2.0).max(0.01);
    let mut b = Builder::new(input);
    // Half the tolerance for the drops, half for simplifying them: within it in all.
    let tol = p.tolerance * 0.5;
    // The stock to leave, all round: the wall stock as an offset of the surface (a cutter
    // grown by it, dropped, then lifted by it), the floor stock's extra straight up.
    let (dropped, lift) = offset_cutter(input.tool, p.wall_stock.max(0.0));
    let extra = (p.floor_stock - p.wall_stock).max(0.0);
    let drop = |pts: &[P2]| -> Vec<P3> {
        drop_line(
            &dropped,
            &idx,
            pts,
            bottom - lift - extra,
            step,
            tol,
            min_step,
        )
        .into_iter()
        .map(|q| [q[0], q[1], (q[2] + lift + extra).max(bottom)])
        .collect()
    };
    let mut prev_end: Option<P3> = None;
    let mut skipped_high = false;
    for l in lines {
        if l.len() < 2 {
            continue;
        }
        let pts = drop(&l);
        if pts.iter().all(|q| q[2] > top + 1e-6) {
            skipped_high = true;
            continue;
        }
        let pts = simplify3(&pts, tol);
        let start = pts[0];
        // A link that follows the surface from the last line's end, when it's short.
        let linked = match prev_end {
            Some(e)
                if link_close
                    && dist2([e[0], e[1]], [start[0], start[1]])
                        <= (p.step_over * 3.0).max(p.links.small_gap) =>
            {
                for q in drop(&[[e[0], e[1]], [start[0], start[1]]]).iter().skip(1) {
                    b.feed(*q, FeedKind::Link);
                }
                true
            }
            _ => false,
        };
        if !linked {
            // Down over the material really there (the stock, or what the operations before
            // left): never a rapid into it.
            let material_top = input
                .material_top([start[0], start[1]], input.tool.radius())
                .max(start[2]);
            let cut = Cut {
                material_top,
                direct_ok: false,
                free: None,
                free_side: None,
                tabs: None,
                ramp: false,
                clear: None,
            };
            b.travel_to([start[0], start[1]], start[2], &cut);
            b.feed(start, FeedKind::Plunge);
        }
        for q in pts.iter().skip(1) {
            b.feed(*q, FeedKind::Cut);
        }
        prev_end = pts.last().copied();
    }
    if skipped_high {
        b.warnings
            .push("Passes entirely above the top height were left out".into());
    }
    if !b.moved() {
        return Err("Nothing to finish between the heights".into());
    }
    Ok(b.finish())
}

/// The heights up a cutter's tip its keep-out is sampled at: 0 alone for a flat end mill;
/// for a ball, bull or cone, its profile's height split so the radius steps evenly.
fn flank_heights(input: &Input) -> Vec<f64> {
    let t = input.tool;
    let top = t.height_at(t.radius()).unwrap_or(0.0);
    if top <= 1e-9 {
        return vec![0.0];
    }
    let n = 10;
    (0..=n)
        .map(|i| {
            let a = std::f64::consts::FRAC_PI_2 * i as f64 / n as f64;
            top * (1.0 - a.cos())
        })
        .collect()
}

/// The region the tool's centre can't enter with its tip at `z`, from the material above the
/// heights `hs` up its flank (`solids[i]` at `z + hs[i]`). Each height's material is kept out
/// by the cutter's radius at the next height up, so it errs on the safe side: the tool never
/// cuts into the part, at the cost of a little more stock where the flank is sampled.
fn keep_out(input: &Input, hs: &[f64], solids: &[Region]) -> Region {
    let t = input.tool;
    let w = input.params.wall_stock;
    let mut out: Region = Vec::new();
    for (i, solid) in solids.iter().enumerate() {
        if solid.is_empty() {
            continue;
        }
        let up = hs.get(i + 1).copied().unwrap_or(hs[i]);
        let r = if i + 1 < hs.len() {
            t.radius_at_height(up)
        } else {
            t.radius()
        };
        // Padded by the offset's tolerance (its round corners' chords cut inside the arc).
        out = region::union(&out, &region::offset(solid, r + w + region::ARC_TOLERANCE));
    }
    out
}

/// Constant Z: waterlines round the model at each level, climb or conventional, with leads.
pub fn constant_z(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let mb = input.model.bounds();
    let top = input.top().min(mb.max[2]);
    let bottom = input.bottom().max(mb.min[2]);
    let levels = input.levels(top, bottom, true);
    let mut b = Builder::new(input);
    let side = if p.direction == Direction::Climb {
        1.0
    } else {
        -1.0
    };
    // The material at every height the levels' flanks reach, worked out together.
    let hs = flank_heights(input);
    let zs: Vec<f64> = levels
        .iter()
        .flat_map(|z| hs.iter().map(move |h| z + h))
        .collect();
    let all = materials_above(input, &zs);
    let mut above = top;
    for (li, z) in levels.into_iter().enumerate() {
        let solids = &all[li * hs.len()..(li + 1) * hs.len()];
        let ko = keep_out(input, &hs, solids);
        if ko.is_empty() {
            above = z;
            continue;
        }
        let rings = region_contours(&ko)
            .into_iter()
            // The material is inside the keep-out region: climb keeps it on the right.
            .map(|c| {
                if p.direction == Direction::Climb {
                    c.reversed()
                } else {
                    c
                }
            })
            .collect::<Vec<Contour>>();
        // Where the centre may go at this level: leads keep to it.
        let free = region::difference(&[super::everywhere(input)], &ko);
        let start = b.at.map_or([mb.min[0], mb.min[1]], |a| [a[0], a[1]]);
        for c in nearest_order(rings, start) {
            let cut = Cut {
                material_top: above,
                direct_ok: false,
                free: Some(&free),
                free_side: Some(side),
                tabs: None,
                ramp: false,
                clear: None,
            };
            b.cut(&c, z, &cut);
        }
        above = z;
    }
    if !b.moved() {
        return Err("No walls between the heights".into());
    }
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb;
    use crate::machine::MachineKind;
    use crate::model::{Model, box_triangles};
    use crate::strategy::{Height, HeightRef, Params, Pattern, Stock, generate};
    use crate::tool::{Tool, ToolKind};
    use crate::toolpath::Step;

    /// A dome-ish stepped part: a 40 × 40 × 10 base with a 20 × 20 block up to 20.
    fn stepped() -> Model {
        let mut t = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 40.0, 10.0]));
        t.extend(box_triangles(&Aabb::new(
            [10.0, 10.0, 10.0],
            [30.0, 30.0, 20.0],
        )));
        Model::from_triangles("stepped", t)
    }

    fn run(pattern: Pattern, tool: Tool, f: impl FnOnce(&mut Params)) -> Toolpath {
        let m = stepped();
        let stock = Stock::block(Aabb::new([-2.0, -2.0, 0.0], [42.0, 42.0, 21.0]));
        let mut p = Params::new(pattern, &tool, MachineKind::Mill);
        f(&mut p);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        generate(&input).unwrap()
    }

    #[test]
    fn parallel_ball_never_goes_under_the_surface() {
        let tool = Tool::new(ToolKind::BallMill, 6.0);
        let tp = run(Pattern::Parallel, tool.clone(), |p| {
            p.step_over = 2.0;
            p.heights.bottom = Height::new(HeightRef::ModelBottom, 0.0);
        });
        let m = stepped();
        let idx = TriIndex::new(&m.triangles);
        let mut scratch = Vec::new();
        let mut n = 0;
        for s in &tp.steps {
            if let Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } = s
            {
                let lowest =
                    crate::dropcutter::drop_point(&tool, &idx, [to[0], to[1]], 0.0, &mut scratch);
                assert!(to[2] >= lowest - 1e-9, "{to:?} below {lowest}");
                n += 1;
            }
        }
        assert!(n > 50);
        // It goes over the top of the boss.
        assert!(
            tp.steps
                .iter()
                .any(|s| matches!(s, Step::Feed { to, .. } if (to[2] - 20.0).abs() < 1e-6))
        );
    }

    #[test]
    fn constant_z_rings_round_the_boss() {
        let tp = run(Pattern::ConstantZ, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.step_down = 2.5;
            p.links.lead_in = crate::strategy::LeadKind::None;
            p.links.lead_out = crate::strategy::LeadKind::None;
        });
        let at15: Vec<[f64; 3]> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } if (to[2] - 15.0).abs() < 1e-6 => Some(*to),
                _ => None,
            })
            .collect();
        assert!(!at15.is_empty());
        // 3 mm round the 20 × 20 boss.
        let maxx = at15.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        assert!((maxx - 33.0).abs() < 0.01, "{maxx}");
    }

    /// A ball's waterlines never put its tip under the surface the exact drop-cutter allows
    /// (the flank sampling errs on the safe side), on a part with slopes.
    #[test]
    fn constant_z_ball_never_gouges() {
        // A pyramid frustum: sloped walls at 45° from a 40 × 40 base to a 10 × 10 top 15 up.
        let (b, t, h) = (20.0, 5.0, 15.0);
        let q = |s: f64, z: f64| [[-s, -s, z], [s, -s, z], [s, s, z], [-s, s, z]];
        let (lo, hi) = (q(b, 0.0), q(t, h));
        let mut tris = Vec::new();
        for i in 0..4 {
            let j = (i + 1) % 4;
            tris.push([lo[i], lo[j], hi[j]]);
            tris.push([lo[i], hi[j], hi[i]]);
        }
        tris.push([hi[0], hi[1], hi[2]]);
        tris.push([hi[0], hi[2], hi[3]]);
        tris.push([lo[0], lo[2], lo[1]]);
        tris.push([lo[0], lo[3], lo[2]]);
        let m = Model::from_triangles("frustum", tris);
        let stock = Stock::block(Aabb::new([-25.0, -25.0, 0.0], [25.0, 25.0, 16.0]));
        let tool = Tool::new(ToolKind::BallMill, 6.0);
        let mut p = Params::new(Pattern::ConstantZ, &tool, MachineKind::Mill);
        p.step_down = 1.0;
        p.links.lead_in = crate::strategy::LeadKind::None;
        p.links.lead_out = crate::strategy::LeadKind::None;
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
        let idx = TriIndex::new(&m.triangles);
        let mut scratch = Vec::new();
        let mut n = 0;
        for s in &tp.steps {
            if let Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } = s
            {
                let lowest = crate::dropcutter::drop_point(
                    &tool,
                    &idx,
                    [to[0], to[1]],
                    -100.0,
                    &mut scratch,
                );
                assert!(to[2] >= lowest - 1e-6, "{to:?} below {lowest}");
                n += 1;
            }
        }
        assert!(n > 100);
    }

    #[test]
    fn simplify_keeps_corners() {
        let pts = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
        ];
        assert_eq!(simplify3(&pts, 0.01).len(), 3);
    }

    fn on_pocket(pattern: Pattern, f: impl FnOnce(&mut Params)) -> (Toolpath, Tool) {
        let m = crate::strategy::testing::pocket_block();
        let stock = crate::strategy::testing::stock_for(&m, 5.0, 1.0);
        let tool = Tool::new(ToolKind::BallMill, 6.0);
        let mut p = Params::new(pattern, &tool, MachineKind::Mill);
        f(&mut p);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        (generate(&input).unwrap(), tool)
    }

    fn cut_points(tp: &Toolpath) -> Vec<P3> {
        tp.steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(*to),
                _ => None,
            })
            .collect()
    }

    /// Nothing cuts further into the part than the toolpath's tolerance.
    fn no_gouge(tp: &Toolpath, tool: &Tool) {
        let m = crate::strategy::testing::pocket_block();
        let tol = Params::new(Pattern::Parallel, tool, MachineKind::Mill).tolerance;
        let (g, at) = crate::strategy::testing::max_gouge_within(&m, tool, tp, tol);
        assert!(
            g < 1e-6,
            "{g} mm past the tolerance into the part at {at:?}"
        );
    }

    #[test]
    fn spiral_winds_out_over_the_part() {
        let (tp, tool) = on_pocket(Pattern::Spiral, |p| p.step_over = 1.0);
        no_gouge(&tp, &tool);
        let pts = cut_points(&tp);
        // Down on the pocket's floor and over the block's top.
        assert!(
            pts.iter()
                .any(|q| (q[2] - 12.0).abs() < 1e-6 && q[0] > 20.0 && q[0] < 40.0)
        );
        assert!(pts.iter().any(|q| (q[2] - 20.0).abs() < 1e-6));
        // Outward: the distance from the middle grows along the cut.
        let c = [30.0, 20.0];
        let first = pts
            .iter()
            .take(10)
            .map(|q| dist2([q[0], q[1]], c))
            .fold(0.0, f64::max);
        let last = pts
            .iter()
            .rev()
            .take(10)
            .map(|q| dist2([q[0], q[1]], c))
            .fold(0.0, f64::max);
        assert!(last > first + 10.0, "{first} → {last}");
    }

    #[test]
    fn scallop_rings_cover_the_shallow_ground() {
        let (tp, tool) = on_pocket(Pattern::Scallop, |p| p.step_over = 1.0);
        no_gouge(&tp, &tool);
        let pts = cut_points(&tp);
        assert!(pts.iter().any(|q| (q[2] - 12.0).abs() < 1e-6));
        assert!(pts.iter().any(|q| (q[2] - 20.0).abs() < 1e-6));
        // Rings: many separate passes, each a closed loop or a piece of one.
        let plunges = tp
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    Step::Feed {
                        kind: FeedKind::Plunge,
                        ..
                    }
                )
            })
            .count();
        assert!(plunges > 5, "{plunges}");
    }

    /// The ball runs along the pocket floor's four inside corners (its centre the radius off
    /// both the floor and the wall) and up the four corners between its walls: nowhere else.
    #[test]
    fn pencil_follows_the_pockets_inside_corners() {
        let (tp, tool) = on_pocket(Pattern::Pencil, |_| {});
        no_gouge(&tp, &tool);
        let pts = cut_points(&tp);
        assert!(!pts.is_empty());
        for q in &pts {
            // In the pocket, against a wall (a radius off it, and a grid cell).
            let in_pocket = q[0] > 15.0 && q[0] < 45.0 && q[1] > 10.0 && q[1] < 30.0;
            let off_wall = (q[0] - 15.0)
                .min(45.0 - q[0])
                .min(q[1] - 10.0)
                .min(30.0 - q[1]);
            assert!(in_pocket && off_wall <= 3.75, "a pencil pass at {q:?}");
        }
        // All four sides.
        for side in [
            |q: &P3| (q[0] - 18.0).abs() < 0.3,
            |q: &P3| (q[0] - 42.0).abs() < 0.3,
            |q: &P3| (q[1] - 13.0).abs() < 0.3,
            |q: &P3| (q[1] - 27.0).abs() < 0.3,
        ] {
            assert!(pts.iter().any(|q| side(q) && (q[2] - 12.0).abs() < 0.05));
        }
    }

    /// 0.5 mm of stock on a vertical wall: the ball keeps its surface 0.5 off it (where the
    /// old straight-up stock kept it 0 off on the wall), and 0.5 over the floor.
    #[test]
    fn stock_is_left_along_the_normal() {
        let (tp, tool) = on_pocket(Pattern::Parallel, |p| {
            p.wall_stock = 0.5;
            p.floor_stock = 0.5;
            p.angle = 0.0;
            p.step_over = 0.5;
        });
        let m = crate::strategy::testing::pocket_block();
        // The model grown by 0.5: nothing of the toolpath goes into it.
        let mut grown = tool.clone();
        grown.diameter += 1.0;
        grown.corner_radius = grown.diameter / 2.0;
        let lifted = Toolpath {
            steps: tp
                .steps
                .iter()
                .map(|s| match s {
                    Step::Feed { to, feed, kind } => Step::Feed {
                        to: [to[0], to[1], to[2] - 0.5],
                        feed: *feed,
                        kind: *kind,
                    },
                    Step::Rapid(p) => Step::Rapid([p[0], p[1], p[2] - 0.5]),
                    s => s.clone(),
                })
                .collect(),
            warnings: vec![],
        };
        let (g, at) = crate::strategy::testing::max_gouge_within(&m, &grown, &lifted, 0.01);
        assert!(g < 1e-6, "{g} at {at:?}");
        // On the pocket's floor, 0.5 up.
        let pts = cut_points(&tp);
        assert!(pts.iter().any(|q| (q[2] - 12.5).abs() < 1e-6), "{pts:?}");
    }
}
