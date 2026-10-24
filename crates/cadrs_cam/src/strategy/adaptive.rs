//! Adaptive clearing: roughing at a constant radial engagement, so the tool can take its full
//! flute length deep at a light step over (high-speed machining).
//!
//! At each level, the region the tool's centre has cleared grows outward by the step over at
//! a time, inside where the centre may go (clear of the part by the wall stock). It starts from
//! the air (off the stock, where the tool goes down freely) or, in a closed pocket, from a
//! helix bored where the pocket is widest. Each growth is the new region's boundary minus what
//! the last one ran along, and only where the tool touches material: so every cut takes the
//! step over and no more, offsets round the region's corners as they grow, and walls are run
//! along once. The tool goes down in cleared ground and links stay down inside it.
//!
//! The levels are deep (the step down, up to the flutes' length) with fine ones between them
//! (the fine step down): after each deep level, the fine levels above it clear only the
//! terraces it couldn't reach where the part slopes, so 3D shapes are roughed close without
//! clearing every layer again.

use crate::geom::{P2, dist2};
use crate::region::{self, Region};
use crate::toolpath::{FeedKind, Toolpath};

use super::Input;
use super::link::{Builder, Cut};
use super::mill::{free_centres_in, keep_out_z, material_left, materials_above, orient};
use super::path::{Contour, nearest_order, region_contours};

pub fn adaptive(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let r = input.tool.radius();
    let mb = input.model.bounds();
    let top = input.top().min(input.stock.bounds.max[2]);
    let bottom = input.bottom().max(mb.min[2] - 1e3);
    // The centres may go a little past the stock, so it starts in the air round it and
    // works in from there.
    let footprint = input.stock.footprint();
    let limit = region::offset(&footprint, r + 2.0 * p.step_over);
    if limit.is_empty() {
        return Err("The stock has no area to clear".into());
    }
    // The deep levels, and between each and the one above, the fine ones that follow the
    // shape where the deep level couldn't reach (a slope, a dish).
    let deep = input.levels(top, bottom, true);
    let fine = p.fine_step_down;
    let mut plan: Vec<(f64, Option<usize>)> = Vec::new();
    let mut prev = top;
    for (k, z) in deep.iter().enumerate() {
        plan.push((*z, None));
        if fine > 0.0 {
            let mut f = prev - fine;
            while f > z + fine * 0.25 {
                plan.push((f, Some(k)));
                f -= fine;
            }
        }
        prev = *z;
    }
    let zs: Vec<f64> = plan.iter().map(|(z, _)| keep_out_z(input, *z)).collect();
    let solids = materials_above(input, &zs);
    let mut b = Builder::new(input);
    // Each deep level's free centres (what it cleared, for the fine levels above it).
    let mut deep_free: Vec<Region> = vec![Vec::new(); deep.len()];
    let mut above = top;
    for ((z, under), solid) in plan.into_iter().zip(&solids) {
        let free = free_centres_in(input, solid, &limit);
        // The material at this level: the stock, or what the operations before left; at a
        // fine level, only what the deep level under it couldn't reach.
        let mut material = match input.rest.filter(|_| p.rest) {
            Some(m) => material_left(m, z),
            None => footprint.clone(),
        };
        if let Some(k) = under {
            material = region::difference(&material, &region::offset(&deep_free[k], r));
        }
        // Centres from which the tool touches it.
        let reach = region::offset(&material, r - 1e-3);
        let start = b.at.map_or([mb.min[0], mb.min[1]], |a| [a[0], a[1]]);
        let faces: Vec<Region> = region::faces(&free)
            .into_iter()
            .filter(|f| f.area() > 1e-4)
            .map(|f| f.rings())
            .collect();
        // A fine level's material is as thick as the fine step down, a deep one's the deep.
        let top_here = match under {
            Some(_) => z + fine,
            None => above,
        };
        for face in order_by_distance(faces, start) {
            if region::intersection(&face, &reach).is_empty() {
                continue;
            }
            clear(&mut b, input, &face, &reach, z, top_here);
        }
        if under.is_none() {
            if let Some(k) = deep.iter().position(|d| (*d - z).abs() < 1e-12) {
                deep_free[k] = free;
            }
            above = z;
        }
    }
    if !b.moved() {
        return Err("Nothing is left to clear at these heights".into());
    }
    Ok(b.finish())
}

fn order_by_distance(mut faces: Vec<Region>, mut from: P2) -> Vec<Region> {
    let mut out = Vec::new();
    while !faces.is_empty() {
        let near = |f: &Region| {
            f.iter()
                .flatten()
                .map(|q| dist2(*q, from))
                .fold(f64::INFINITY, f64::min)
        };
        let i = (0..faces.len())
            .min_by(|a, b| near(&faces[*a]).total_cmp(&near(&faces[*b])))
            .unwrap();
        let f = faces.swap_remove(i);
        from = f[0][0];
        out.push(f);
    }
    out
}

/// Clears one connected face of free centres at `z` (the material's top `above`).
fn clear(b: &mut Builder, input: &Input, face: &Region, reach: &Region, z: f64, above: f64) {
    let p = input.params;
    let step = p.step_over;
    // Where it starts: the air in the face, else a helix.
    let air = region::difference(face, reach);
    let mut cleared = if region::area(&air) > step * step {
        air
    } else {
        match helix(b, input, face, z, above) {
            Some(c) => c,
            None => {
                b.warnings
                    .push("A pocket too narrow to bore a helix into was left".into());
                return;
            }
        }
    };
    let total = region::area(face);
    for _ in 0..100_000 {
        // (Simplified within the offsets' own tolerance: each round offset adds a joint at
        // every vertex, which would double them every step.)
        let next = region::simplify(
            &region::intersection(&region::offset(&cleared, step), face),
            region::ARC_TOLERANCE * 0.5,
        );
        let grown = region::area(&next) - region::area(&cleared);
        if grown < step * 1e-3 {
            break;
        }
        // The new boundary, climb (material outside it on the right), away from what the
        // last boundary ran along, where there's material to touch.
        let rings: Vec<Vec<P2>> = orient(region_contours(&next), p.direction, false)
            .iter()
            .map(|c| c.walk())
            .collect();
        let fresh = region::difference(reach, &region::offset(&cleared, step * 0.5));
        let runs = region::clip_open(&rings, &fresh);
        let from = b.at.map_or(face[0][0], |a| [a[0], a[1]]);
        for run in nearest_order(runs.into_iter().map(Contour::open).collect(), from) {
            if run.pts.len() < 2 {
                continue;
            }
            go_down_near(b, face, &cleared, &next, run.pts[0], z, above);
            for q in run.pts.iter().skip(1) {
                b.feed([q[0], q[1], z], FeedKind::Cut);
            }
        }
        cleared = next;
        if region::area(&cleared) >= total - 1e-6 {
            break;
        }
    }
    // Once round the face's edge where it touches material: the growth only cuts where at
    // least half a step is left, so slivers up to that could stand along the walls (uneven
    // stock for the finish, and in the way of the hops over them). At most a step from what's
    // cleared, so never more than the step over.
    let rings: Vec<Vec<P2>> = orient(region_contours(face), p.direction, false)
        .iter()
        .map(|c| c.walk())
        .collect();
    let near = region::intersection(reach, &region::offset(&cleared, step));
    let runs = region::clip_open(&rings, &near);
    let from = b.at.map_or(face[0][0], |a| [a[0], a[1]]);
    for run in nearest_order(runs.into_iter().map(Contour::open).collect(), from) {
        if run.pts.len() < 2 {
            continue;
        }
        go_down_near(b, face, &cleared, face, run.pts[0], z, above);
        for q in run.pts.iter().skip(1) {
            b.feed([q[0], q[1], z], FeedKind::Cut);
        }
    }
}

/// Gets the tool to `to` at depth `z`: along at depth when it's down and the way is inside
/// the region it may move in (`next`: cleared, or a step from it), else down in the cleared
/// ground nearest `to` and across. Within the face it hops just over this level's material
/// (`above`); out of it, by the link settings.
#[allow(clippy::too_many_arguments)]
fn go_down_near(
    b: &mut Builder,
    face: &Region,
    cleared: &Region,
    next: &Region,
    to: P2,
    z: f64,
    above: f64,
) {
    if let Some(at) = b.at
        && (at[2] - z).abs() < 1e-6
        && region::segment_inside(next, [at[0], at[1]], to)
    {
        b.feed([to[0], to[1], z], FeedKind::Link);
        return;
    }
    // The cleared ground's nearest point.
    let e = cleared
        .iter()
        .flatten()
        .copied()
        .min_by(|a, c| dist2(*a, to).total_cmp(&dist2(*c, to)))
        .unwrap_or(to);
    // A hop over this level's material: the face is free of the part at depth, so (the part
    // only narrowing upward) the way over it is too.
    if let Some(at) = b.at
        && (at[2] - z).abs() < 1e-6
        && region::segment_inside(face, [at[0], at[1]], e)
    {
        let p = b.input.params;
        let fd = p.links.feed_distance;
        let hop = above + fd;
        b.rapid([at[0], at[1], hop]);
        b.rapid([e[0], e[1], hop]);
        // At a deep level its whole footprint was cut at this depth when its centre passed
        // there: quickly down to just over it. A fine level's terraces (a fine step thick) can
        // stand right beside the cleared ground: fed down through the fine step instead.
        let fine_level = above - z <= p.fine_step_down + 1e-9;
        if !fine_level {
            b.rapid([e[0], e[1], z + fd]);
        }
        b.feed([e[0], e[1], z], FeedKind::Plunge);
        b.feed([to[0], to[1], z], FeedKind::Cut);
        return;
    }
    let cut = Cut {
        material_top: above,
        direct_ok: true,
        free: Some(cleared),
        free_side: None,
        tabs: None,
        ramp: false,
        clear: None,
    };
    if !b.travel_to(e, z, &cut) {
        b.feed([e[0], e[1], z], FeedKind::Plunge);
    }
    b.feed([to[0], to[1], z], FeedKind::Cut);
}

/// Bores into a closed face with a helix where it's widest; the cleared centres (a disc).
fn helix(b: &mut Builder, input: &Input, face: &Region, z: f64, above: f64) -> Option<Region> {
    let p = input.params;
    let r = input.tool.radius();
    // The face's deepest point, and how deep it is.
    let s = (region::area(face).sqrt() / 50.0).max(0.01);
    let mut cur = face.clone();
    let mut depth = 0.0;
    for _ in 0..200 {
        let next = region::offset(&cur, -s);
        if next.is_empty() {
            break;
        }
        cur = next;
        depth += s;
    }
    let ring = cur.first()?;
    let n = ring.len() as f64;
    let mut c = [
        ring.iter().map(|q| q[0]).sum::<f64>() / n,
        ring.iter().map(|q| q[1]).sum::<f64>() / n,
    ];
    if !region::contains(face, c) {
        c = ring[0];
    }
    let rh = (0.5 * r).min(depth * 0.9);
    if rh < 0.05 && !input.tool.center_cutting {
        return None;
    }
    let start = [c[0] + rh, c[1]];
    let cut = Cut {
        material_top: above,
        direct_ok: false,
        free: None,
        free_side: None,
        tabs: None,
        ramp: false,
        clear: None,
    };
    b.travel_to(start, z, &cut);
    let from = b.at.map_or(above, |a| a[2]);
    if rh < 0.05 {
        b.feed([c[0], c[1], z], FeedKind::Plunge);
        return Some(vec![region::circle(c, 0.05)]);
    }
    // Down a turn at a time at the ramp angle.
    let pitch =
        (std::f64::consts::TAU * rh * p.links.ramp_angle.clamp(0.5, 45.0).to_radians().tan())
            .max(0.05);
    let mut h = from;
    while h > z + 1e-9 {
        h = (h - pitch).max(z);
        b.arc([start[0], start[1], h], c, false, FeedKind::Ramp);
    }
    // A last turn at depth.
    b.arc([start[0], start[1], z], c, false, FeedKind::Cut);
    Some(region::intersection(&[region::circle(c, rh)], face))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineKind;
    use crate::strategy::testing::{max_gouge, pocket_block, stock_for};
    use crate::strategy::{Params, Pattern, generate};
    use crate::tool::{Tool, ToolKind};
    use crate::toolpath::Step;

    fn run(margin: f64) -> (Toolpath, Tool) {
        let m = pocket_block();
        let stock = stock_for(&m, margin, 2.0);
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let p = Params::new(Pattern::Adaptive, &tool, MachineKind::Mill);
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

    /// The pocket (closed) is bored with a helix; outside, the tool comes down off the stock;
    /// nothing gouges; Verify finds the pocket cleared to the wall stock.
    #[test]
    fn clears_the_pocket_and_round_the_block() {
        let (tp, tool) = run(8.0);
        let (g, at) = max_gouge(&pocket_block(), &tool, &tp);
        assert!(g < 1e-3, "{g} at {at:?}");
        let helix = tp
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    Step::Arc {
                        kind: FeedKind::Ramp,
                        ..
                    }
                )
            })
            .count();
        assert!(helix > 0, "the closed pocket is entered by a helix");
        // Plunges only off the stock or in cleared ground: none plunges into the pocket's
        // uncut middle (a helix does that).
        let m = pocket_block();
        let stock = stock_for(&m, 8.0, 2.0);
        let segs = tp.motion([0.0, 0.0, 50.0], 5000.0);
        let r = crate::sim::simulate_mill(
            &stock,
            &m,
            &[crate::sim::SimOp {
                tool: &tool,
                segments: &segs,
            }],
            0.25,
        );
        assert_eq!(r.gouge_cells, 0);
        assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
        let s = r.stock.sample([30.0, 20.0]);
        assert!((s - 12.3).abs() < 0.4, "the pocket floor at {s}");
        let s = r.stock.sample([-5.0, 20.0]);
        assert!(s < 0.5, "round the block at {s}");
    }

    /// Every cut takes the step over sideways and no more: the stock each straight cut removes,
    /// per mm along and per mm of depth, is about the step over.
    #[test]
    fn engagement_stays_near_the_step_over() {
        let (tp, tool) = run(8.0);
        let m = pocket_block();
        let stock = stock_for(&m, 8.0, 2.0);
        // Below the block's top: how wide a band of material each cut move takes, measured as
        // the stock the move removes per mm of travel over its depth.
        let mut map = stock.height_map(0.2);
        let mut worst: f64 = 0.0;
        let mut at = [0.0, 0.0, 50.0];
        for seg in tp.motion(at, 5000.0) {
            at = seg.to;
            if seg.feed.is_none() || seg.kind != Some(FeedKind::Cut) {
                map.cut(&tool, seg.from, seg.to);
                continue;
            }
            let len = dist2([seg.from[0], seg.from[1]], [seg.to[0], seg.to[1]]);
            let removed = map.cut(&tool, seg.from, seg.to);
            if len > 2.0 && (seg.from[2] - seg.to[2]).abs() < 1e-9 {
                // Removed volume / (length × layer depth) ≈ the radial engagement.
                let width = removed
                    / (len * Params::new(Pattern::Adaptive, &tool, MachineKind::Mill).step_down);
                worst = worst.max(width);
            }
        }
        let _ = at;
        let step = Params::new(Pattern::Adaptive, &tool, MachineKind::Mill).step_over;
        // About the step over everywhere (the height map's cells blur it a little): never a
        // slot or a corner taking more.
        assert!(worst > step * 0.5, "{worst}");
        assert!(
            worst < step * 1.5,
            "a cut {worst} mm wide (step over {step})"
        );
    }

    /// A walled pocket roughed adaptively, from a fresh block, at the defaults: Verify finds no
    /// rapid into the stock (the fine levels' hops feed down beside their terraces; no slivers
    /// are left along the walls to hop over) and no gouge.
    fn default_adaptive_on_a_walled_pocket(cell: f64) {
        use crate::geom::Aabb;
        use crate::model::{Model, box_triangles};
        use crate::sim::{SimOp, simulate_mill_on};
        use crate::strategy::{Input, Stock};
        let mut tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [100.0, 60.0, 10.0]));
        for (lo, hi) in [
            ([0.0, 0.0], [10.0, 60.0]),
            ([40.0, 0.0], [50.0, 60.0]),
            ([10.0, 0.0], [40.0, 10.0]),
            ([10.0, 50.0], [40.0, 60.0]),
        ] {
            tris.extend(box_triangles(&Aabb::new(
                [lo[0], lo[1], 10.0],
                [hi[0], hi[1], 25.0],
            )));
        }
        let m = Model::from_triangles("walled", tris);
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [105.0, 65.0, 28.0]));
        let mut em = Tool::new(ToolKind::EndMill, 6.0);
        em.cutting_length = 30.0;
        em.overall_length = 60.0;
        let p = Params::new(Pattern::Adaptive, &em, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &em,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let segs = tp.motion([0.0, 0.0, 60.0], 5_000.0);
        let r = simulate_mill_on(
            stock.height_map(cell),
            &m,
            &[],
            &[SimOp {
                tool: &em,
                segments: &segs,
            }],
        );
        assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
        assert_eq!(r.gouge_cells, 0, "{}", r.gouge_depth);
    }

    #[test]
    fn a_default_adaptive_rapids_clear_of_the_stock() {
        default_adaptive_on_a_walled_pocket(0.25);
    }

    /// The same on a fine (0.05 mm) grid, where a graze of a tenth would show (slow).
    #[test]
    #[ignore]
    fn a_default_adaptive_rapids_clear_of_the_stock_finely() {
        default_adaptive_on_a_walled_pocket(0.05);
    }
}
