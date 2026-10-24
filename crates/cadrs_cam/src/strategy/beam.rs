//! Laser, plasma and waterjet: Cut (through-cutting, kerf compensated), Etch (marking along
//! edges) and Fill (laser hatching of faces).
//!
//! A cut is ordered inside out: contours nested deeper are cut first (a hole before the part
//! round it, a part in a hole before the hole), so nothing falls before it's cut. Each contour
//! is pierced in the scrap (beside the cut, through a lead-in) at the pierce height, the beam
//! dwells for the pierce delay, drops to its cut height and runs round with the part on its
//! right (the good side of a clockwise-swirl plasma torch), past the start by the overlap.
//! Microjoints leave short uncut bridges that hold the parts in the sheet; after each bridge
//! the beam pierces again in the scrap and leads back onto the contour.
//!
//! Leads and pierces are checked against the parts: a lead keeps the beam's centre a kerf
//! (half its width plus the wall stock) off every part, and a pierce, whose crater is wider
//! than the kerf, a kerf width more. A lead that doesn't fit is tried shorter; a hole too small
//! for any is pierced in its middle, run straight to the edge.

use crate::geom::{P2, P3, dist2, lerp2, ring_area2, ring_contains};
use crate::machine::MachineKind;
use crate::region::{self, Region};
use crate::toolpath::{FeedKind, Step, Toolpath};

use super::link::{Builder, arc_path};
use super::path::{
    Contour, chain, hatch, inner_point, lead_arc, nearest_order, nearest_order_indexed,
    region_contours, sub_walk,
};
use super::{CuttingMethod, Direction, Input, LeadKind, Side};

/// Heights of a beam cut: the top of the sheet, travel, pierce and cut heights.
struct Z {
    travel: f64,
    pierce: f64,
    cut: f64,
}

fn heights(input: &Input) -> Z {
    let top = input.top();
    let b = &input.tool.beam;
    let (pierce, cut) = match input.machine {
        MachineKind::Plasma => (top + b.pierce_height, top + b.cut_height),
        MachineKind::Waterjet => (top + b.cut_height, top + b.cut_height),
        _ => (top + b.cut_height, top + b.cut_height),
    };
    let travel = (top + input.params.links.clearance).max(pierce);
    Z {
        travel,
        pierce,
        cut,
    }
}

/// The contours a cut follows, kerf compensated, each with whether it's a hole (scrap inside).
/// Every one has the part on its left (a hole's ring runs clockwise, its scrap on the right),
/// as [`cut`] expects.
fn cut_contours(input: &Input) -> Result<Vec<(Contour, bool)>, String> {
    let p = input.params;
    let k = input.tool.radius() + p.wall_stock;
    let d = &p.drive;
    let mut out = Vec::new();
    if !d.edges.is_empty() {
        let polys: Vec<Vec<P3>> = d
            .edges
            .iter()
            .filter_map(|n| input.model.edge(n))
            .map(|e| e.points.clone())
            .collect();
        let z = input.selection_z().unwrap_or(input.model.bounds().max[2]);
        for c in chain(&polys, 1e-3) {
            if p.side == Side::On {
                out.push((c, false));
                continue;
            }
            if !c.closed {
                // The kerf off the edge, away from the part (the part on its left).
                let (mut pts, free_left) = input.offset_open_chain(&c, z, k, p.side);
                if free_left {
                    pts.reverse();
                }
                out.push((Contour::open(pts), false));
                continue;
            }
            let part_inside = input.part_inside(&c, z);
            let mut ring = c.pts.clone();
            if ring_area2(&ring) < 0.0 {
                ring.reverse();
            }
            let outside = match p.side {
                Side::Outside => true,
                Side::Inside => false,
                _ => part_inside,
            };
            let off = region::offset(&[ring], if outside { k } else { -k });
            for cc in region_contours(&off) {
                // Part inside an outside cut; scrap inside an inside cut (its ring, round the
                // scrap counter-clockwise, turned to have the part on its left).
                if outside {
                    out.push((cc, false));
                } else {
                    out.push((cc.reversed(), true));
                }
            }
        }
        if out.is_empty() {
            return Err("The picked edges make no contour".into());
        }
        return Ok(out);
    }
    let sil = input.model.silhouette();
    if sil.is_empty() {
        return Err("The job has no bodies to cut".into());
    }
    let off = match p.side {
        Side::On => sil,
        // Inside the part's outline is the part: inside its through holes only.
        Side::Inside => region::offset(&region::holes(&sil), -k),
        _ => region::offset(&sil, k),
    };
    for c in region_contours(&off) {
        if p.side == Side::Inside {
            // Round a hole's scrap: turned to have the part on its left.
            out.push((c.reversed(), true));
        } else {
            let hole = !c.ccw();
            out.push((c, hole));
        }
    }
    Ok(out)
}

/// Where a cut's beam may go, from the parts (a cut whose kerf is outside them).
struct Room {
    /// The beam's centre a kerf off every part: leads keep to it.
    free: Region,
    /// A kerf width further: where it may pierce.
    pierce: Region,
}

/// The [`Room`] round the job's parts.
fn room(input: &Input) -> Option<Room> {
    let k = input.tool.radius() + input.params.wall_stock;
    let sil = input.model.silhouette();
    let all = [super::everywhere(input)];
    Some(Room {
        free: region::difference(&all, &region::offset(&sil, k - 1e-3)),
        pierce: region::difference(&all, &region::offset(&sil, k + input.tool.radius())),
    })
}

/// The [`Room`] inside a hole's cut (`c`, the beam's centre round its scrap): the beam may go
/// anywhere inside it, and pierce a kerf in from it.
fn room_in(c: &Contour, kerf: f64) -> Room {
    let mut ring = c.pts.clone();
    if ring_area2(&ring) < 0.0 {
        ring.reverse();
    }
    Room {
        // (A hair out: a lead ends on the contour itself; the part is a kerf further.)
        free: region::offset(&[ring.clone()], 1e-3),
        pierce: region::offset(&[ring], -kerf),
    }
}

/// The lead onto a contour at `pt` (direction `t`) from the scrap on `side`: where to pierce,
/// and the arc's centre and turn (`None`: straight). Tried at the lead radius, then shorter;
/// in a hole (`hole`: its ring) too small for any, from its middle.
fn lead_for(
    input: &Input,
    room: Option<&Room>,
    pt: P2,
    t: P2,
    side: f64,
    hole: Option<&[P2]>,
) -> Option<(P2, Option<(P2, bool)>)> {
    let l = &input.params.links;
    if l.lead_in == LeadKind::None || l.lead_radius <= 0.0 {
        return None;
    }
    let kerf = input.tool.radius();
    let fits = |pierce: P2, path: &[P2]| match room {
        Some(r) => {
            region::contains(&r.pierce, pierce)
                && path
                    .windows(2)
                    .all(|w| region::segment_inside(&r.free, w[0], w[1]))
        }
        None => true,
    };
    let mut radius = l.lead_radius;
    while radius >= kerf.max(0.05) - 1e-9 {
        let lead = match l.lead_in {
            LeadKind::Arc => {
                let (s, ctr, cw) = lead_arc(pt, t, side, radius, 90.0);
                (s, Some((ctr, cw)), arc_path(s, pt, ctr, cw))
            }
            _ => {
                let n = [-t[1] * side, t[0] * side];
                let s = [pt[0] + n[0] * radius, pt[1] + n[1] * radius];
                (s, None, vec![s, pt])
            }
        };
        if fits(lead.0, &lead.2) {
            return Some((lead.0, lead.1));
        }
        radius /= 2.0;
    }
    // A small hole: pierce in its middle and go straight to the edge.
    let ring = hole?;
    let mid = inner_point(ring);
    (dist2(mid, pt) > kerf && fits(mid, &[mid, pt])).then_some((mid, None))
}

/// Splits a closed walk into the pieces between `count` gaps of `width`, evenly spaced along
/// it (the first half a spacing from the start).
fn microjoint_pieces(walk: &[P2], count: usize, width: f64) -> Vec<Vec<P2>> {
    let total: f64 = walk.windows(2).map(|w| dist2(w[0], w[1])).sum();
    if count == 0 || width * count as f64 >= total * 0.5 {
        return vec![walk.to_vec()];
    }
    let spacing = total / count as f64;
    let mut pieces = Vec::new();
    let mut s = 0.0;
    for i in 0..count {
        let c = spacing * (i as f64 + 0.5);
        pieces.push(sub_walk(walk, s, c - width / 2.0));
        s = c + width / 2.0;
    }
    pieces.push(sub_walk(walk, s, total));
    pieces.retain(|p| p.len() >= 2);
    pieces
}

/// Pierce, beam on, down to the cut height.
fn pierce(b: &mut Builder, xy: P2, z: &Z, power: f64, delay: f64) {
    b.up_to(z.travel);
    b.rapid([xy[0], xy[1], z.travel]);
    b.rapid([xy[0], xy[1], z.pierce]);
    b.steps.push(Step::BeamOn { power });
    if delay > 0.0 {
        b.steps.push(Step::Dwell(delay));
    }
    if (z.pierce - z.cut).abs() > 1e-9 {
        b.feed([xy[0], xy[1], z.cut], FeedKind::Plunge);
    }
}

fn beam_off(b: &mut Builder, z: &Z) {
    b.steps.push(Step::BeamOff);
    b.up_to(z.travel);
}

/// Cut: through-cutting with kerf compensation, inside first.
pub fn cut(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let t = input.tool;
    let contours = cut_contours(input)?;
    let rings: Vec<Vec<P2>> = contours
        .iter()
        .filter(|(c, _)| c.closed)
        .map(|(c, _)| c.pts.clone())
        .collect();
    // Deepest nesting first, then nearest.
    let mut by_depth: Vec<(usize, Contour, bool)> = contours
        .into_iter()
        .map(|(c, hole)| {
            // How many other contours it lies in (its own boundary says nothing).
            let d = if c.closed {
                rings
                    .iter()
                    .filter(|r| **r != c.pts)
                    .filter(|r| ring_contains(r, c.pts[0]))
                    .count()
            } else {
                usize::MAX
            };
            (d, c, hole)
        })
        .collect();
    by_depth.sort_by_key(|x| std::cmp::Reverse(x.0));
    let z = heights(input);
    let room = room(input);
    let mut b = Builder::new(input);
    let power = (p.beam.power / 100.0).clamp(0.0, 1.0);
    let delay = t.beam.pierce_delay;
    let passes = p.beam.passes.max(1);
    let mut from: P2 = {
        let mb = input.model.bounds();
        [mb.min[0], mb.min[1]]
    };
    let mut i = 0;
    while i < by_depth.len() {
        let d = by_depth[i].0;
        let group: Vec<(Contour, bool)> = by_depth
            .iter()
            .skip(i)
            .take_while(|x| x.0 == d)
            .map(|x| (x.1.clone(), x.2))
            .collect();
        i += group.len();
        let holes: Vec<bool> = group.iter().map(|g| g.1).collect();
        let cs: Vec<Contour> = group.into_iter().map(|g| g.0).collect();
        for (k, c) in nearest_order_indexed(cs, from) {
            let hole = holes[k];
            // The region's rings have the part (and its kerf) on their left; the part on the
            // right (climb) is every ring reversed.
            let c = if c.closed && p.direction == Direction::Climb {
                c.reversed().starting_near(from)
            } else if c.closed {
                c.starting_near(from)
            } else if p.direction == Direction::Climb {
                c.reversed()
            } else {
                c
            };
            // A hole's pierce and lead stay in its own scrap.
            let hole_room = (hole && c.closed).then(|| room_in(&c, t.radius()));
            let feed =
                if hole && p.beam.small_hole > 0.0 && hole_diameter(&c.pts) < p.beam.small_hole {
                    p.speeds.feed * p.beam.small_hole_feed
                } else {
                    p.speeds.feed
                };
            for pass in 0..passes {
                let zc = z.cut - pass as f64 * p.beam.pass_depth;
                let zz = Z {
                    travel: z.travel,
                    pierce: z.pierce - pass as f64 * p.beam.pass_depth,
                    cut: zc,
                };
                cut_one(
                    &mut b,
                    input,
                    hole_room.as_ref().or(room.as_ref()),
                    &c,
                    hole,
                    &zz,
                    power,
                    delay,
                    feed,
                );
            }
            from = b.at.map_or(from, |a| [a[0], a[1]]);
        }
    }
    if input.inside_into_material() {
        b.warnings.push(super::mill::INSIDE_INTO_MATERIAL.into());
    }
    let mut tp = b.finish();
    tp.steps.retain(|s| !matches!(s, Step::Spindle { .. }));
    Ok(tp)
}

#[allow(clippy::too_many_arguments)]
fn cut_one(
    b: &mut Builder,
    input: &Input,
    room: Option<&Room>,
    c: &Contour,
    hole: bool,
    z: &Z,
    power: f64,
    delay: f64,
    feed: f64,
) {
    let p = input.params;
    let l = &p.links;
    let walk = c.walk();
    // The scrap is on the left with the part on the right.
    let scrap_side = if p.direction == Direction::Climb {
        1.0
    } else {
        -1.0
    };
    let ring = (c.closed && hole).then_some(c.pts.as_slice());
    // Microjoints hold parts, not the scrap in their holes.
    let pieces = if c.closed && !hole && p.beam.microjoints > 0 {
        microjoint_pieces(
            &walk,
            p.beam.microjoints,
            p.beam.microjoint_width + input.tool.diameter,
        )
    } else {
        vec![walk.clone()]
    };
    for (k, piece) in pieces.iter().enumerate() {
        // Each piece (after a bridge too) pierced in the scrap and led on.
        let start = piece[0];
        let lead = if c.closed {
            Contour::open(piece.clone())
                .start_tangent()
                .and_then(|(pt, t)| lead_for(input, room, pt, t, scrap_side, ring))
        } else {
            None
        };
        if k > 0 {
            beam_off(b, z);
        }
        pierce(b, lead.map_or(start, |(s, _)| s), z, power, delay);
        if let Some((_, arc)) = lead {
            match arc {
                Some((ctr, cw)) => b.arc([start[0], start[1], z.cut], ctr, cw, FeedKind::LeadIn),
                None => b.feed([start[0], start[1], z.cut], FeedKind::LeadIn),
            }
        }
        for q in piece.iter().skip(1) {
            b.feed_at([q[0], q[1], z.cut], FeedKind::Cut, feed);
        }
    }
    // Past the start by the overlap (unless microjoints break the loop there).
    if c.closed && l.overlap > 0.0 && pieces.len() == 1 {
        let mut left = l.overlap;
        for w in walk.windows(2) {
            let d = dist2(w[0], w[1]);
            if d >= left {
                let q = lerp2(w[0], w[1], left / d);
                b.feed_at([q[0], q[1], z.cut], FeedKind::Cut, feed);
                break;
            }
            b.feed_at([w[1][0], w[1][1], z.cut], FeedKind::Cut, feed);
            left -= d;
        }
    }
    beam_off(b, z);
}

/// A hole's size for the small-hole feed: the diameter of the largest circle in it (found by
/// shrinking the ring until it vanishes), so a long narrow slot counts as small.
fn hole_diameter(ring: &[P2]) -> f64 {
    let r = vec![ring.to_vec()];
    let a = ring_area2(ring).abs() / 2.0;
    // No circle in it is bigger than the one of its area.
    let (mut lo, mut hi) = (0.0, (a / std::f64::consts::PI).sqrt());
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if region::offset(&r, -mid).is_empty() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    2.0 * lo
}

/// Etch: along the picked edges (or the picked faces' outlines) at marking power, no
/// compensation.
pub fn etch(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let d = &p.drive;
    let mut cs: Vec<Contour> = Vec::new();
    if !d.edges.is_empty() {
        let polys: Vec<Vec<P3>> = d
            .edges
            .iter()
            .filter_map(|n| input.model.edge(n))
            .map(|e| e.points.clone())
            .collect();
        cs = chain(&polys, 1e-3);
    }
    for f in d.faces.iter().filter_map(|n| input.model.face(n)) {
        for l in &f.loops {
            cs.push(Contour::closed(l.iter().map(|q| [q[0], q[1]]).collect()));
        }
    }
    if cs.is_empty() {
        return Err("Pick the edges or faces to etch".into());
    }
    let z = heights(input);
    let power = (p.beam.power / 100.0).clamp(0.0, 1.0);
    let mut b = Builder::new(input);
    let mb = input.model.bounds();
    for c in nearest_order(cs, [mb.min[0], mb.min[1]]) {
        let walk = c.walk();
        // Marking doesn't need the full pierce delay.
        pierce(&mut b, walk[0], &z, power, 0.0);
        for q in walk.iter().skip(1) {
            b.feed([q[0], q[1], z.cut], FeedKind::Cut);
        }
        beam_off(&mut b, &z);
    }
    let mut tp = b.finish();
    tp.steps.retain(|s| !matches!(s, Step::Spindle { .. }));
    Ok(tp)
}

/// Fill: hatches the picked faces (laser engraving an area).
pub fn fill(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let areas: Region = p
        .drive
        .faces
        .iter()
        .filter_map(|n| input.model.face(n))
        .flat_map(|f| {
            f.loops
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let mut r: Vec<P2> = l.iter().map(|q| [q[0], q[1]]).collect();
                    let ccw = ring_area2(&r) > 0.0;
                    if (i == 0) != ccw {
                        r.reverse();
                    }
                    r
                })
                .collect::<Vec<_>>()
        })
        .collect();
    if areas.is_empty() {
        return Err("Pick the faces to fill".into());
    }
    let area = region::normalize(&areas);
    let zig = p.cutting_method == CuttingMethod::Zigzag;
    let lines = hatch(
        &region::offset(&area, -input.tool.radius()),
        p.angle,
        p.step_over,
        zig,
    );
    let z = heights(input);
    let power = (p.beam.power / 100.0).clamp(0.0, 1.0);
    let mut b = Builder::new(input);
    for l in lines {
        let (a, e) = (l[0], l[l.len() - 1]);
        // Each line on its own: beam on at its start, off at its end.
        b.rapid([a[0], a[1], z.cut]);
        b.steps.push(Step::BeamOn { power });
        b.feed([e[0], e[1], z.cut], FeedKind::Cut);
        b.steps.push(Step::BeamOff);
    }
    let mut tp = b.finish();
    tp.steps.retain(|s| !matches!(s, Step::Spindle { .. }));
    Ok(tp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb;
    use crate::model::{Face, Model, Surface, box_triangles};
    use crate::strategy::{Params, Pattern, Stock, generate};
    use crate::tool::{Tool, ToolKind};

    /// A 100 × 60 × 3 plate with a 20 × 20 square hole (triangles made from three boxes and
    /// two strips round the hole).
    fn plate() -> Model {
        let mut t = Vec::new();
        t.extend(box_triangles(&Aabb::new(
            [0.0, 0.0, 0.0],
            [40.0, 60.0, 3.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [60.0, 0.0, 0.0],
            [100.0, 60.0, 3.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [40.0, 0.0, 0.0],
            [60.0, 20.0, 3.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [40.0, 40.0, 0.0],
            [60.0, 60.0, 3.0],
        )));
        let mut m = Model::from_triangles("plate", t);
        m.faces.push(Face {
            name: "top".into(),
            body: 0,
            surface: Surface::Plane {
                normal: [0.0, 0.0, 1.0],
            },
            loops: vec![vec![
                [10.0, 10.0, 3.0],
                [30.0, 10.0, 3.0],
                [30.0, 30.0, 3.0],
                [10.0, 30.0, 3.0],
            ]],
            triangles: vec![],
        });
        m
    }

    fn run(
        pattern: Pattern,
        machine: MachineKind,
        tool: Tool,
        f: impl FnOnce(&mut Params),
    ) -> Result<Toolpath, String> {
        let m = plate();
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [105.0, 65.0, 3.0]));
        let mut p = Params::new(pattern, &tool, machine);
        f(&mut p);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        generate(&input)
    }

    #[test]
    fn plasma_cuts_the_hole_first_with_pierces() {
        let tp = run(
            Pattern::Cut,
            MachineKind::Plasma,
            Tool::new(ToolKind::Plasma, 1.5),
            |_| {},
        )
        .unwrap();
        let ons: Vec<usize> = tp
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s, Step::BeamOn { .. }))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(ons.len(), 2);
        // The first cut is the hole, counter-clockwise (the part on the right), pierced in the
        // scrap: its feeds stay inside 40..60.
        let first_cut: Vec<P3> = tp.steps[ons[0]..ons[1]]
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(*to),
                _ => None,
            })
            .collect();
        assert!(
            first_cut.iter().all(|p| p[0] > 40.0 && p[0] < 60.0),
            "{first_cut:?}"
        );
        // Kerf: 0.75 inside the hole.
        assert!(first_cut.iter().any(|p| (p[0] - 40.75).abs() < 1e-3));
        // A pierce dwell and the torch at its pierce height.
        assert!(
            tp.steps
                .iter()
                .any(|s| matches!(s, Step::Dwell(d) if (*d - 0.5).abs() < 1e-9))
        );
        assert!(
            tp.steps
                .iter()
                .any(|s| matches!(s, Step::Rapid(p) if (p[2] - 6.8).abs() < 1e-9))
        );
    }

    /// A plate with a round hole (the hole's triangles a fan, as a kernel tessellates it): the
    /// hole is cut before the outline whatever the outline's distance.
    #[test]
    fn round_hole_first() {
        let mut t = Vec::new();
        let n = 48;
        let ring = |r: f64, z: f64| -> Vec<P3> {
            (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    [30.0 + r * a.cos(), 20.0 + r * a.sin(), z]
                })
                .collect()
        };
        // The plate's top as triangles between the outline (a square) and the hole.
        let outer = |i: usize| -> P3 {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let (c, s) = (a.cos(), a.sin());
            let k = 1.0 / c.abs().max(s.abs());
            [30.0 + 30.0 * c * k, 20.0 + 20.0 * s * k, 3.0]
        };
        let hole = ring(8.0, 3.0);
        for i in 0..n {
            let j = (i + 1) % n;
            t.push([hole[i], outer(i), outer(j)]);
            t.push([hole[i], outer(j), hole[j]]);
        }
        let m = Model::from_triangles("plate", t);
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [65.0, 45.0, 3.0]));
        let tool = Tool::new(ToolKind::Plasma, 1.5);
        let p = Params::new(Pattern::Cut, &tool, MachineKind::Plasma);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Plasma,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let first = tp
            .steps
            .iter()
            .find_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(*to),
                _ => None,
            })
            .unwrap();
        // On the hole: 8 − 0.75 from its centre.
        let r = (first[0] - 30.0).hypot(first[1] - 20.0);
        assert!((r - 7.25).abs() < 0.05, "{first:?} r={r}");
    }

    #[test]
    fn part_on_the_right() {
        let tp = run(
            Pattern::Cut,
            MachineKind::Laser,
            Tool::new(ToolKind::Laser, 0.2),
            |p| {
                p.links.lead_in = LeadKind::None;
                p.links.overlap = 0.0;
            },
        )
        .unwrap();
        // The outline (second cut) runs clockwise.
        let ons: Vec<usize> = tp
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s, Step::BeamOn { .. }))
            .map(|(i, _)| i)
            .collect();
        let outline: Vec<P2> = tp.steps[ons[1]..]
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some([to[0], to[1]]),
                _ => None,
            })
            .collect();
        assert!(ring_area2(&outline) < 0.0);
    }

    #[test]
    fn microjoints_break_the_outline() {
        let tp = run(
            Pattern::Cut,
            MachineKind::Laser,
            Tool::new(ToolKind::Laser, 0.2),
            |p| {
                p.beam.microjoints = 4;
            },
        )
        .unwrap();
        let ons = tp
            .steps
            .iter()
            .filter(|s| matches!(s, Step::BeamOn { .. }))
            .count();
        // The hole in one go, the outline in five pieces.
        assert_eq!(ons, 1 + 5);
    }

    #[test]
    fn fill_hatches_the_face() {
        let tp = run(
            Pattern::Fill,
            MachineKind::Laser,
            Tool::new(ToolKind::Laser, 0.2),
            |p| {
                p.drive.faces = vec!["top".into()];
                p.step_over = 1.0;
            },
        )
        .unwrap();
        let lines = tp
            .steps
            .iter()
            .filter(|s| matches!(s, Step::BeamOn { .. }))
            .count();
        assert_eq!(lines, 20);
    }

    #[test]
    fn mill_tools_cant_cut() {
        assert!(
            run(
                Pattern::Cut,
                MachineKind::Mill,
                Tool::new(ToolKind::EndMill, 6.0),
                |_| {}
            )
            .is_err()
        );
    }

    #[test]
    fn microjoint_split() {
        let walk = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let pieces = microjoint_pieces(&walk, 4, 1.0);
        let cut: f64 = pieces
            .iter()
            .map(|p| p.windows(2).map(|w| dist2(w[0], w[1])).sum::<f64>())
            .sum();
        assert!((cut - 36.0).abs() < 1e-6, "{cut} {pieces:?}");
    }

    #[test]
    fn hole_diameter_is_the_inscribed_circle() {
        let slot = region::rect([0.0, 0.0], [98.5, 8.5]);
        assert!(
            (hole_diameter(&slot) - 8.5).abs() < 0.01,
            "{}",
            hole_diameter(&slot)
        );
        let round = region::circle([0.0, 0.0], 5.0);
        assert!((hole_diameter(&round) - 10.0).abs() < 0.02);
    }

    /// The pierces and leads of a cut: every pierce at least a kerf width off the kerf path
    /// (off the parts by `2k`), every lead's beam centre at least `k` off them.
    fn assert_scrap_only(tp: &Toolpath, m: &Model, k: f64) {
        let sil = m.silhouette();
        let near = |q: P2, d: f64| region::contains(&region::offset(&sil, d), q);
        let mut at = [0.0, 0.0, 50.0];
        for (i, s) in tp.steps.iter().enumerate() {
            match s {
                Step::BeamOn { .. } => {
                    assert!(
                        !near([at[0], at[1]], 2.0 * k - 1e-3),
                        "a pierce at {at:?} (step {i})"
                    );
                }
                Step::Feed {
                    to,
                    kind: FeedKind::LeadIn,
                    ..
                }
                | Step::Arc {
                    to,
                    kind: FeedKind::LeadIn,
                    ..
                } => {
                    let from = at;
                    for t in 0..=20 {
                        let f = t as f64 / 20.0;
                        let q = [
                            from[0] + (to[0] - from[0]) * f,
                            from[1] + (to[1] - from[1]) * f,
                        ];
                        // (Straight between the ends: an arc lead bulges further into the
                        // scrap, never towards the part it's tangent to.)
                        assert!(
                            !near(q, k - 2e-3),
                            "a lead over the part at {q:?} (step {i})"
                        );
                    }
                }
                _ => {}
            }
            match s {
                Step::Rapid(p) => at = *p,
                Step::Feed { to, .. } | Step::Arc { to, .. } => at = *to,
                _ => {}
            }
        }
    }

    /// A plate with a 20 × 20 through hole, cut Inside (picked or not) and Automatic (picked or
    /// not): every pierce and lead in scrap (the hole's), never in the part, and the hole run the
    /// same way round whether picked or not.
    #[test]
    fn holes_cut_from_their_scrap_however_chosen() {
        let mut t = Vec::new();
        for (lo, hi) in [
            ([0.0, 0.0], [20.0, 40.0]),
            ([40.0, 0.0], [60.0, 40.0]),
            ([20.0, 0.0], [40.0, 10.0]),
            ([20.0, 30.0], [40.0, 40.0]),
        ] {
            t.extend(box_triangles(&Aabb::new(
                [lo[0], lo[1], 0.0],
                [hi[0], hi[1], 3.0],
            )));
        }
        let mut m = Model::from_triangles("plate", t);
        m.edges.push(crate::model::Edge {
            name: "hole".into(),
            body: 0,
            points: vec![
                [20.0, 10.0, 3.0],
                [40.0, 10.0, 3.0],
                [40.0, 30.0, 3.0],
                [20.0, 30.0, 3.0],
                [20.0, 10.0, 3.0],
            ],
            circle: None,
        });
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [65.0, 45.0, 3.0]));
        let tool = Tool::new(ToolKind::Laser, 0.2);
        let mut turns = Vec::new();
        for (side, picked) in [
            (Side::Inside, false),
            (Side::Inside, true),
            (Side::Auto, true),
            (Side::Auto, false),
        ] {
            let mut p = Params::new(Pattern::Cut, &tool, MachineKind::Laser);
            p.side = side;
            if picked {
                p.drive.edges = vec!["hole".into()];
            }
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
            assert_scrap_only(&tp, &m, 0.1);
            // The hole's cut: the cut moves inside the hole's box.
            let ring: Vec<P2> = tp
                .steps
                .iter()
                .filter_map(|s| match s {
                    Step::Feed {
                        to,
                        kind: FeedKind::Cut,
                        ..
                    } if (19.0..41.0).contains(&to[0]) && (9.0..31.0).contains(&to[1]) => {
                        Some([to[0], to[1]])
                    }
                    _ => None,
                })
                .collect();
            assert!(ring.len() >= 4, "{side:?} {picked}");
            turns.push(ring_area2(&ring) > 0.0);
        }
        assert!(turns.iter().all(|t| *t == turns[0]), "{turns:?}");
    }

    /// A plate with a narrow slot (8.5 × 98.5 as a hole): the 4 mm lead can't fit across its
    /// width, so it's shortened (or the slot pierced in its middle), never pierced by the far
    /// wall.
    #[test]
    fn slot_leads_fit_the_slot() {
        let mut t = Vec::new();
        t.extend(box_triangles(&Aabb::new(
            [0.0, 0.0, 0.0],
            [120.0, 10.0, 3.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [0.0, 18.5, 0.0],
            [120.0, 28.5, 3.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [0.0, 10.0, 0.0],
            [10.0, 18.5, 3.0],
        )));
        t.extend(box_triangles(&Aabb::new(
            [108.5, 10.0, 0.0],
            [120.0, 18.5, 3.0],
        )));
        let m = Model::from_triangles("slotted", t);
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [125.0, 35.0, 3.0]));
        let tool = Tool::new(ToolKind::Plasma, 1.5);
        let mut p = Params::new(Pattern::Cut, &tool, MachineKind::Plasma);
        p.links.lead_radius = 4.0;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Plasma,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        assert_scrap_only(&tp, &m, 0.75);
        // The slot is small: cut at the small-hole feed.
        let slow = p.speeds.feed * p.beam.small_hole_feed;
        assert!(tp.steps.iter().any(|s| matches!(s, Step::Feed { feed, kind: FeedKind::Cut, .. } if (*feed - slow).abs() < 1e-9)) == (8.5 - 1.5 < p.beam.small_hole));
    }

    /// Microjoints: after each bridge the torch pierces again in the scrap, at its pierce
    /// height, and leads back on.
    #[test]
    fn microjoints_repierce_in_the_scrap() {
        let tool = Tool::new(ToolKind::Plasma, 1.5);
        let tp = run(Pattern::Cut, MachineKind::Plasma, tool, |p| {
            p.beam.microjoints = 4;
        })
        .unwrap();
        let ons = tp
            .steps
            .iter()
            .filter(|s| matches!(s, Step::BeamOn { .. }))
            .count();
        // The hole once, the outline in five pieces.
        assert_eq!(ons, 6);
        let leads = tp
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    Step::Arc {
                        kind: FeedKind::LeadIn,
                        ..
                    } | Step::Feed {
                        kind: FeedKind::LeadIn,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(leads, 6);
        // Every pierce at the pierce height.
        for (i, s) in tp.steps.iter().enumerate() {
            if matches!(s, Step::BeamOn { .. }) {
                assert!(
                    matches!(tp.steps[i - 1], Step::Rapid(p) if (p[2] - 6.8).abs() < 1e-9),
                    "{:?}",
                    &tp.steps[i - 2..=i]
                );
            }
        }
        assert_scrap_only(&tp, &plate(), 0.75);
    }
}
