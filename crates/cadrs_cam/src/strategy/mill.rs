//! 2.5-axis milling and roughing: Face, 2-axis rough, Rough (3D), 2-axis profile, 2-axis
//! chamfer, Engrave and Flatlands.
//!
//! Everything works on regions at depths. The material at or above a depth is the model's
//! shadow from above there ([`crate::model::Model::shadow_above`]) together with its exact
//! section (the kernel's, when the studio worked it out); the tool's centre must stay its
//! radius plus the wall stock away from it. Roughing clears what's left inside the stock with
//! rings offset by the step over (or parallel passes), from the inside out so the tool enters
//! where it has room.

use crate::geom::{P2, ring_area2, ring_contains};
use crate::heightmap::{EMPTY, HeightMap};
use crate::model::Surface;
use crate::region::{self, Face, Region};
use crate::toolpath::Toolpath;

use super::link::{Builder, Cut};
use super::path::{Contour, chain, hatch, nearest_order, nearest_order_indexed, region_contours};
use super::{CuttingMethod, Direction, Input, MachineBy, Side, SubPattern};

/// The material at or above `z` seen from above.
pub fn material_above(input: &Input, z: f64) -> Region {
    materials_above(input, &[z]).pop().unwrap_or_default()
}

/// [`material_above`] at many heights at once (the shadows worked out together).
pub fn materials_above(input: &Input, zs: &[f64]) -> Vec<Region> {
    input
        .model
        .shadows(zs)
        .into_iter()
        .zip(zs)
        .map(|(shadow, z)| match input.model.sections.get(*z) {
            Some(exact) => region::union(&shadow, exact),
            None => shadow,
        })
        .collect()
}

/// The height whose material keeps the tool out at depth `z` (the floor stock above floors).
pub(super) fn keep_out_z(input: &Input, z: f64) -> f64 {
    z - input.params.floor_stock + 1e-4
}

/// Where the tool's centre may go at depth `z` without touching the model (keeping the wall
/// and floor stock), within `limit`; `solid` is the material at [`keep_out_z`].
pub(super) fn free_centres_in(input: &Input, solid: &Region, limit: &Region) -> Region {
    let p = input.params;
    let r = input.tool.radius();
    // Padded by the offset's tolerance: its round corners' chords cut inside the true arc.
    let mut keep_out = region::offset(solid, r + p.wall_stock + region::ARC_TOLERANCE);
    // The workholding: its whole footprint, the tool and its clearance off it.
    let fixtures = fixture_footprint(input);
    if !fixtures.is_empty() {
        let clear = r + p.collision.shank_clearance.max(0.5);
        keep_out = region::union(&keep_out, &region::offset(&fixtures, clear));
    }
    region::difference(limit, &keep_out)
}

/// The workholding's footprint from above (worked out once for a generation's fixtures).
pub(super) fn fixture_footprint(input: &Input) -> Region {
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(u64, Region)>> = Mutex::new(None);
    if input.fixtures.is_empty() {
        return Vec::new();
    }
    // By content (hashing is far cheaper than the union).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for t in input.fixtures {
        for p in t {
            for v in p {
                v.to_bits().hash(&mut h);
            }
        }
    }
    let key = h.finish();
    // (The lock only round the lookups: toolpaths calculated at once don't wait on the union.)
    if let Some((k, r)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && *k == key
    {
        return r.clone();
    }
    let r = crate::model::Model::from_triangles("fixtures", input.fixtures.to_vec()).silhouette();
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, r.clone()));
    r
}

fn free_centres(input: &Input, z: f64, limit: &Region) -> Region {
    let solid = material_above(input, keep_out_z(input, z));
    free_centres_in(input, &solid, limit)
}

/// The stock's footprint grown so the tool clears its edges (centres up to just inside the
/// radius past them).
pub(super) fn stock_limit(input: &Input) -> Region {
    let r = input.tool.radius();
    let grow = (r - (input.params.step_over * 0.5).min(r * 0.5)).max(0.0);
    region::offset(&input.stock.footprint(), grow)
}

/// Where the in-process stock still has material above `z`, as a region (cell runs).
pub fn material_left(map: &HeightMap, z: f64) -> Region {
    let mut rects = Vec::new();
    let h = map.cell * 0.5;
    for j in 0..map.ny {
        let mut i = 0;
        while i < map.nx {
            let v = map.at(i, j);
            if v != EMPTY && v as f64 > z + 0.01 {
                let i0 = i;
                while i < map.nx && {
                    let v = map.at(i, j);
                    v != EMPTY && v as f64 > z + 0.01
                } {
                    i += 1;
                }
                let a = map.point(i0, j);
                let b = map.point(i - 1, j);
                rects.push(region::rect([a[0] - h, a[1] - h], [b[0] + h, b[1] + h]));
            } else {
                i += 1;
            }
        }
    }
    region::union_all(rects)
}

/// Contours ordered for climb or conventional milling: `material_inside` when the material is
/// on the inside of the region's rings (a profile), else outside (a pocket).
pub(super) fn orient(cs: Vec<Contour>, dir: Direction, material_inside: bool) -> Vec<Contour> {
    // Region rings are counter-clockwise outside, clockwise for holes: the region on the left.
    // Climb keeps the material on the right.
    let keep = (dir == Direction::Climb) != material_inside;
    cs.into_iter()
        .map(|c| if keep { c } else { c.reversed() })
        .collect()
}

/// The free side (for leads) of contours oriented by [`orient`]: the material's opposite.
fn free_side(dir: Direction) -> f64 {
    // Climb: material on the right, so free on the left.
    if dir == Direction::Climb { 1.0 } else { -1.0 }
}

/// Rings to clear a face, innermost first.
fn offset_rings(face: &Face, step: f64, dir: Direction) -> Vec<Vec<Contour>> {
    let mut rings = Vec::new();
    let mut cur = face.rings();
    for _ in 0..10_000 {
        if cur.is_empty() {
            break;
        }
        rings.push(orient(region_contours(&cur), dir, false));
        cur = region::offset(&cur, -step);
    }
    rings.reverse();
    rings
}

/// Smooths a region's sharp convex corners by `radius` (an opening).
fn smooth(r: &Region, radius: f64) -> Region {
    if radius <= 0.0 {
        return r.clone();
    }
    region::offset(&region::offset(r, -radius), radius)
}

/// Clears one face of free centres at `z`.
fn clear_face(b: &mut Builder, face: &Face, z: f64, material_top: f64) {
    let p = b.input.params;
    let dir = p.direction;
    // The face is where the centre may go: links stay down only inside it.
    let free = face.rings();
    match p.sub_pattern {
        SubPattern::Offset => {
            let rings = offset_rings(face, p.step_over, dir);
            let mut first = true;
            // Within a step over of the rings cut so far: a link through there takes no more
            // than a ring does (not a slot through ground no ring has reached, as across a
            // pocket's neck between two lobes' first rings).
            let mut near_cut: Region = Vec::new();
            for level in rings {
                let start = b.at.map_or([0.0, 0.0], |a| [a[0], a[1]]);
                for c in nearest_order(level, start) {
                    let reach = region::intersection(&free, &near_cut);
                    let cut = Cut {
                        material_top,
                        // Between rings the tool crosses what the inner rings cleared.
                        direct_ok: !first,
                        free: Some(&reach),
                        free_side: None,
                        tabs: None,
                        ramp: true,
                        clear: None,
                    };
                    b.cut(&c, z, &cut);
                    first = false;
                    near_cut = region::union(
                        &near_cut,
                        &region::stroke(&c.walk(), 2.0 * p.step_over + 2e-3),
                    );
                }
            }
        }
        SubPattern::Parallel => {
            // Passes inside the area the final ring leaves, then the ring.
            let inner = region::offset(&face.rings(), -p.step_over * 0.5);
            let zig = p.cutting_method == CuttingMethod::Zigzag;
            let lines = hatch(&inner, p.angle, p.step_over, zig);
            let mut first = true;
            for l in lines {
                let c = Contour::open(l);
                let cut = Cut {
                    material_top,
                    direct_ok: !first && zig,
                    free: Some(&free),
                    free_side: None,
                    tabs: None,
                    ramp: true,
                    clear: None,
                };
                b.cut(&c, z, &cut);
                first = false;
            }
            let start = b.at.map_or([0.0, 0.0], |a| [a[0], a[1]]);
            for c in nearest_order(orient(region_contours(&face.rings()), dir, false), start) {
                let cut = Cut {
                    material_top,
                    direct_ok: false,
                    free: None,
                    free_side: None,
                    tabs: None,
                    ramp: true,
                    clear: None,
                };
                b.cut(&c, z, &cut);
            }
        }
    }
}

/// The selected faces' regions (planar faces' outlines), with their heights.
fn selected_faces(input: &Input) -> Vec<(Region, f64)> {
    input
        .params
        .drive
        .faces
        .iter()
        .filter_map(|n| input.model.face(n))
        .filter_map(|f| {
            let z = f.z().or_else(|| {
                let pts: Vec<f64> = f.loops.iter().flatten().map(|p| p[2]).collect();
                pts.iter().copied().reduce(f64::min)
            })?;
            let rings: Vec<Vec<P2>> = f
                .loops
                .iter()
                .map(|l| l.iter().map(|p| [p[0], p[1]]).collect())
                .collect();
            // Outer ring counter-clockwise, holes clockwise.
            let mut norm: Region = Vec::new();
            for (i, mut r) in rings.into_iter().enumerate() {
                let ccw = ring_area2(&r) > 0.0;
                if (i == 0) != ccw {
                    r.reverse();
                }
                norm.push(r);
            }
            Some((region::normalize(&norm), z))
        })
        .collect()
}

/// Face: flat passes over the stock's top (or the selected faces), down to the bottom height.
/// Each level's passes keep to where the tool's centre is clear of the part there; over the
/// whole stock they run off its edges (plunging outside it), over picked faces they ramp in.
pub fn face(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let r = input.tool.radius();
    let sel = selected_faces(input);
    let picked = !sel.is_empty();
    let areas: Region = if picked {
        sel.into_iter().flat_map(|(r, _)| r).collect()
    } else {
        input.stock.footprint()
    };
    let top = input.top();
    let bottom = input.bottom() + p.floor_stock;
    let levels = input.levels(top, bottom, false);
    let zig = p.cutting_method == CuttingMethod::Zigzag;
    let mut b = Builder::new(input);
    let covered = region::offset(&areas, (r - p.step_over * 0.5).max(0.0));
    // Over the whole stock, run on and off by the radius plus a little so the face is cut
    // clean; over picked faces, stay over them.
    let ext = if picked { 0.0 } else { r + 2.0 };
    let zs: Vec<f64> = levels.iter().map(|z| keep_out_z(input, *z)).collect();
    let solids = materials_above(input, &zs);
    let mut material_top = top;
    for (z, solid) in levels.into_iter().zip(&solids) {
        let free = free_centres_in(input, solid, &region::offset(&covered, ext));
        let lines = hatch(
            &region::intersection(&free, &covered),
            p.angle,
            p.step_over,
            zig,
        );
        let mut first = true;
        for mut l in lines {
            if ext > 0.0 && l.len() >= 2 {
                let n = l.len();
                let (a, e) = (l[0], l[n - 1]);
                let d = super::path::unit([e[0] - a[0], e[1] - a[1]]);
                let a2 = [a[0] - d[0] * ext, a[1] - d[1] * ext];
                let e2 = [e[0] + d[0] * ext, e[1] + d[1] * ext];
                if region::segment_inside(&free, a2, a) {
                    l[0] = a2;
                }
                if region::segment_inside(&free, e, e2) {
                    l[n - 1] = e2;
                }
            }
            let cut = Cut {
                material_top,
                direct_ok: zig && !first,
                free: Some(&free),
                free_side: None,
                tabs: None,
                // Entries off the stock plunge; over picked faces they ramp.
                ramp: picked,
                clear: None,
            };
            b.cut(&Contour::open(l), z, &cut);
            first = false;
        }
        material_top = z;
    }
    if !b.moved() {
        return Err("The tool doesn't fit anywhere to face".into());
    }
    Ok(b.finish())
}

/// 2-axis rough and Rough (3D): clears the stock around the model level by level.
pub fn rough(input: &Input, three_d: bool) -> Result<Toolpath, String> {
    let p = input.params;
    let r = input.tool.radius();
    let mb = input.model.bounds();
    let stock_top = input.stock.bounds.max[2];
    let top = input.top().min(stock_top);
    let mut limit = stock_limit(input);
    let mut bottom = input.bottom();
    // 2-axis rough with picked faces: clear above those floors only.
    let sel = selected_faces(input);
    if !three_d && !sel.is_empty() {
        let areas: Region = sel.iter().flat_map(|(r, _)| r.clone()).collect();
        limit = region::intersection(&limit, &region::offset(&areas, r));
        bottom = sel.iter().map(|(_, z)| *z).fold(f64::INFINITY, f64::min) + p.floor_stock;
    }
    if limit.is_empty() {
        return Err("The stock has no area to clear".into());
    }
    let bottom = bottom.max(mb.min[2] - 1e3);
    let levels = input.levels(top, bottom, true);
    let mut b = Builder::new(input);
    // The material above every level, worked out together.
    let zs: Vec<f64> = levels.iter().map(|z| keep_out_z(input, *z)).collect();
    let solids = materials_above(input, &zs);
    // Each level's faces, with the level above's z (the material's top there).
    let mut per_level: Vec<(f64, f64, Vec<Face>)> = Vec::new();
    let mut prev = top;
    for (z, solid) in levels.into_iter().zip(&solids) {
        let mut free = free_centres_in(input, solid, &limit);
        if p.smooth_corners > 0.0 {
            free = smooth(&free, p.smooth_corners);
        }
        if let Some(rest) = input.rest.filter(|_| p.rest || three_d) {
            let left = region::offset(&material_left(rest, z), r);
            free = region::intersection(&free, &left);
        }
        // Too small to fit the tool's centre path: drop (dust from the offsets).
        let faces: Vec<Face> = region::faces(&free)
            .into_iter()
            .filter(|f| f.area() > 1e-4)
            .collect();
        per_level.push((z, prev, faces));
        prev = z;
    }
    match p.machine_by {
        MachineBy::Levels => {
            for (z, above, faces) in &per_level {
                let start = b.at.map_or([mb.min[0], mb.min[1]], |a| [a[0], a[1]]);
                for f in order_faces(faces.clone(), start) {
                    clear_face(&mut b, &f, *z, *above);
                }
            }
        }
        MachineBy::Regions => {
            // Depth first: a face, then the faces under it, level by level.
            let mut done: Vec<Vec<bool>> = per_level
                .iter()
                .map(|(_, _, f)| vec![false; f.len()])
                .collect();
            for i in 0..per_level.first().map_or(0, |l| l.2.len()) {
                region_down(&mut b, &per_level, &mut done, 0, i);
            }
            // Faces not under any face above (islands appearing lower).
            for li in 0..per_level.len() {
                for fi in 0..per_level[li].2.len() {
                    if !done[li][fi] {
                        region_down(&mut b, &per_level, &mut done, li, fi);
                    }
                }
            }
        }
    }
    if !b.moved() {
        return Err("Nothing is left to clear at these heights".into());
    }
    Ok(b.finish())
}

fn region_down(
    b: &mut Builder,
    levels: &[(f64, f64, Vec<Face>)],
    done: &mut [Vec<bool>],
    li: usize,
    fi: usize,
) {
    if done[li][fi] {
        return;
    }
    done[li][fi] = true;
    let (z, above, faces) = &levels[li];
    clear_face(b, &faces[fi], *z, *above);
    if li + 1 >= levels.len() {
        return;
    }
    let outer = &faces[fi].outer;
    for (k, f) in levels[li + 1].2.iter().enumerate() {
        if !done[li + 1][k] && f.outer.iter().any(|p| ring_contains(outer, *p)) {
            region_down(b, levels, done, li + 1, k);
        }
    }
}

/// Faces in nearest-first order from `from`.
fn order_faces(mut faces: Vec<Face>, mut from: P2) -> Vec<Face> {
    let mut out = Vec::new();
    while !faces.is_empty() {
        let (i, _) = faces
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let d = f
                    .outer
                    .iter()
                    .map(|p| (p[0] - from[0]).hypot(p[1] - from[1]))
                    .fold(f64::INFINITY, f64::min);
                (i, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let f = faces.swap_remove(i);
        from = f.outer[0];
        out.push(f);
    }
    out
}

/// The part's outline contours for a profile: picked edges (chained), else the outline of the
/// model from above. Each with whether it's closed and its height.
fn drive_contours(input: &Input) -> (Vec<(Contour, f64)>, bool) {
    let d = &input.params.drive;
    if !d.edges.is_empty() {
        let polys: Vec<Vec<crate::geom::P3>> = d
            .edges
            .iter()
            .filter_map(|n| input.model.edge(n))
            .map(|e| e.points.clone())
            .collect();
        let zs: Vec<f64> = polys.iter().flatten().map(|p| p[2]).collect();
        let z = zs.iter().copied().fold(f64::INFINITY, f64::min);
        let mut cs: Vec<(Contour, f64)> = chain(&polys, 1e-3).into_iter().map(|c| (c, z)).collect();
        if d.reverse {
            cs = cs.into_iter().map(|(c, z)| (c.reversed(), z)).collect();
        }
        return (cs, true);
    }
    let z = input.model.bounds().min[2];
    (
        region_contours(&input.model.silhouette())
            .into_iter()
            .map(|c| (c, z))
            .collect(),
        false,
    )
}

/// The centre paths of a profile round the contours: picked loops offset to their side (Auto:
/// outside when the part is inside the loop), the outline outside, holes inside; each with
/// whether the material is inside its ring.
fn profile_paths(input: &Input, dist: f64) -> Vec<Contour> {
    let p = input.params;
    let (cs, picked) = drive_contours(input);
    let mut out = Vec::new();
    if !picked {
        // The model's outline: outside the part, inside its through holes.
        let sil = input.model.silhouette();
        let off = match p.side {
            // Inside the part's outline is the part: inside its through holes only.
            Side::Inside => region::offset(&region::holes(&sil), -dist),
            Side::On => sil,
            _ => region::offset(&sil, dist),
        };
        let mut cs = region_contours(&off);
        if p.side != Side::On {
            // The holes' rings (Inside) have the material round them, outside them.
            cs = orient(cs, p.direction, p.side != Side::Inside);
        }
        return cs;
    }
    for (c, z) in cs {
        if p.side == Side::On {
            out.push(c);
            continue;
        }
        if !c.closed {
            // Off the edge, on the side away from the material, the free side where the
            // cutting direction wants it.
            let (mut pts, free_left) = input.offset_open_chain(&c, z, dist, p.side);
            if free_left != (free_side(p.direction) > 0.0) {
                pts.reverse();
            }
            out.push(Contour::open(pts));
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
        let r = vec![ring];
        let off = if outside {
            region::offset(&r, dist)
        } else {
            region::offset(&r, -dist)
        };
        // Material inside the ring for an outside cut; outside it for an inside cut.
        out.extend(orient(region_contours(&off), p.direction, outside));
    }
    out
}

/// Each contour's passes, outermost first, its finishing pass last (`passes`: each pass's
/// contours, the finishing pass last): every rough pass goes with the finishing contour
/// nearest it, so an offset that merged or dropped a contour doesn't shift the pairing.
fn group_passes(passes: &[Vec<Contour>]) -> Vec<Vec<Contour>> {
    let finishing = passes.last().cloned().unwrap_or_default();
    let mut paths_by_contour: Vec<Vec<Contour>> = vec![Vec::new(); finishing.len()];
    for pass in &passes[..passes.len().saturating_sub(1)] {
        for c in pass {
            let Some(q) = c.pts.first() else { continue };
            let near = |f: &Contour| {
                f.walk()
                    .windows(2)
                    .map(|w| crate::geom::dist_to_segment2(*q, w[0], w[1]))
                    .fold(f64::INFINITY, f64::min)
            };
            if let Some(j) = (0..finishing.len())
                .min_by(|a, b| near(&finishing[*a]).total_cmp(&near(&finishing[*b])))
            {
                paths_by_contour[j].push(c.clone());
            }
        }
    }
    for (j, f) in finishing.into_iter().enumerate() {
        paths_by_contour[j].push(f);
    }
    paths_by_contour
}

/// What a profile or cut says when Inside puts the tool on the part's side of picked open edges.
pub(super) const INSIDE_INTO_MATERIAL: &str = "Inside puts the tool on the part's side of the \
     picked edges: it cuts into the part there (Automatic or Outside keeps it off)";

/// 2-axis profile: round the outline (or the picked edges) at depth passes, with finishing
/// passes, leads and tabs.
pub fn profile(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let r = input.tool.radius();
    let top = input.top();
    let bottom = input.bottom() + p.floor_stock;
    if bottom >= top {
        return Err(
            "The bottom height isn't below the top: set Bottom lower (or Top higher)".into(),
        );
    }
    let levels = input.levels(top, bottom, false);
    let passes = p.passes.max(1);
    let tabs =
        p.tabs
            .enabled
            .then_some((bottom + p.tabs.height, p.tabs.count, p.tabs.width + 2.0 * r));
    // Rough passes outward first, the finishing pass last.
    let pass_paths: Vec<Vec<Contour>> = (0..passes)
        .rev()
        .map(|k| {
            let extra = if p.side == Side::On {
                0.0
            } else {
                r + p.wall_stock + region::ARC_TOLERANCE
            };
            profile_paths(input, extra + k as f64 * p.step_over)
        })
        .collect();
    if pass_paths.iter().all(|c| c.is_empty()) {
        if p.side == Side::Inside && p.drive.edges.is_empty() {
            return Err(
                "The part has no through holes to cut inside: pick the edges of a contour to \
                 cut inside it"
                    .into(),
            );
        }
        return Err("No contour to profile".into());
    }
    let side = (p.side != Side::On).then_some(free_side(p.direction));
    let paths_by_contour = group_passes(&pass_paths);
    let by_levels = p.machine_by == MachineBy::Levels;
    let start = [input.model.bounds().min[0], input.model.bounds().min[1]];
    let order: Vec<usize> = {
        let firsts: Vec<Contour> = paths_by_contour
            .iter()
            .map(|v| v.last().cloned().unwrap_or(Contour::open(vec![[0.0, 0.0]])))
            .collect();
        nearest_order_indexed(firsts, start)
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    };
    // Lead-ins only start where the tool can come down: off the stock, or (below the first
    // depth) in the band this contour cut at the depth above.
    let sb = input.stock.bounds;
    let outside = region::difference(
        &[region::rect(
            [sb.min[0] - 1e4, sb.min[1] - 1e4],
            [sb.max[0] + 1e4, sb.max[1] + 1e4],
        )],
        &input.stock.footprint(),
    );
    let run = |b: &mut Builder, ci: usize, li: usize, z: f64, above: f64| {
        b.contour = ci;
        // The band the pass before cut at this depth.
        let mut cut_before: Option<Region> = None;
        for (k, path) in paths_by_contour[ci].iter().enumerate() {
            let near = b.at.map_or(path.pts[0], |a| [a[0], a[1]]);
            let path = if path.closed {
                path.starting_near(near)
            } else {
                path.clone()
            };
            let band = region::stroke(&path.walk(), 2.0 * r - 0.02);
            let mut clear = if li > 0 {
                region::union(&outside, &band)
            } else {
                outside.clone()
            };
            if let Some(before) = &cut_before {
                clear = region::union(&clear, before);
            }
            let cut = Cut {
                material_top: above,
                // To the next pass at depth only through what's cut (checked swept).
                direct_ok: k > 0,
                free: None,
                free_side: side,
                tabs,
                ramp: true,
                clear: Some(&clear),
            };
            b.cut(&path, z, &cut);
            cut_before = Some(band);
        }
    };
    let build = |off: &std::collections::HashSet<usize>| {
        let mut b = Builder::new(input);
        b.comp_off = off.clone();
        if by_levels {
            let mut above = top;
            for (li, z) in levels.iter().enumerate() {
                for &ci in &order {
                    run(&mut b, ci, li, *z, above);
                }
                above = *z;
            }
        } else {
            for &ci in &order {
                let mut above = top;
                for (li, z) in levels.iter().enumerate() {
                    run(&mut b, ci, li, *z, above);
                    above = *z;
                }
            }
        }
        b
    };
    let b = build(&Default::default());
    // Wear compensation at every depth of a contour or at none: a depth with no room for the
    // lead-in it starts on leaves its contour uncompensated throughout.
    let b = if b.comp_missed.is_empty() {
        b
    } else {
        let missed = b.comp_missed.clone();
        let mut b = build(&missed);
        b.warnings.push(format!(
            "Wear compensation needs room for a lead-in at every depth: {} contour{} cut without \
             it (G41/G42) throughout",
            missed.len(),
            if missed.len() == 1 { " is" } else { "s are" }
        ));
        b
    };
    let mut b = b;
    if input.inside_into_material() {
        b.warnings.push(INSIDE_INTO_MATERIAL.into());
    }
    if input.bottom_from_top_pick() {
        b.warnings.push(
            "The picked edges are at the top: cut down to the model's bottom (pick the \
             contour's bottom edges, or set Bottom, to stop higher)"
                .into(),
        );
    }
    Ok(b.finish())
}

/// 2-axis chamfer: a chamfer mill along the top edges (picked, else the top faces' outlines),
/// cutting a chamfer `depth` wide.
pub fn chamfer(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let t = input.tool;
    let half = (t.angle.clamp(1.0, 179.0) / 2.0).to_radians();
    let width = p.depth.max(0.0);
    let depth = width / half.tan() + p.tip_offset;
    // The tip's distance from the edge so the flank cuts `width` into it.
    let off = t.tip_diameter / 2.0 + p.tip_offset * half.tan();
    if t.radius_at_height(depth) < off + width - 1e-9 {
        return Err("The chamfer is wider than the tool can cut".into());
    }
    // Each loop with its height and whether the part is inside it.
    let mut contours: Vec<(Contour, f64, bool)> = Vec::new();
    if !p.drive.edges.is_empty() {
        let (cs, _) = drive_contours(input);
        let edge_z = input.selection_z().unwrap_or(input.model.bounds().max[2]);
        for (c, _) in cs {
            let inside = c.closed && input.part_inside(&c, edge_z);
            contours.push((c, edge_z, inside));
        }
    } else {
        // The outlines of the upward faces at the model's top: a face's first loop has the
        // face inside it, the others are its holes.
        let top = input.model.bounds().max[2];
        for f in &input.model.faces {
            if f.faces_up() && f.z().is_some_and(|z| (z - top).abs() < 1e-3) {
                for (i, l) in f.loops.iter().enumerate() {
                    let c = Contour::closed(l.iter().map(|q| [q[0], q[1]]).collect());
                    contours.push((c, top, i == 0));
                }
            }
        }
        if contours.is_empty() {
            for c in region_contours(&input.model.shadow_above(top - 1e-3)) {
                let inside = c.ccw();
                contours.push((c, top, inside));
            }
        }
    }
    let mut b = Builder::new(input);
    let mut paths = Vec::new();
    for (c, z, part_inside) in contours {
        if !c.closed {
            paths.push((Contour::open(super::path::offset_open(&c.pts, -off)), z));
            continue;
        }
        let mut ring = c.pts.clone();
        if ring_area2(&ring) < 0.0 {
            ring.reverse();
        }
        // Outside the part's outline, inside its holes: away from the material at the edge.
        let r = vec![ring];
        let off_r = if part_inside {
            region::offset(&r, off)
        } else {
            region::offset(&r, -off)
        };
        for cc in orient(region_contours(&off_r), p.direction, part_inside) {
            paths.push((cc, z));
        }
    }
    let start = [input.model.bounds().min[0], input.model.bounds().min[1]];
    let zs: Vec<f64> = paths.iter().map(|(_, z)| *z).collect();
    let cs: Vec<Contour> = paths.into_iter().map(|(c, _)| c).collect();
    let everywhere = [super::everywhere(input)];
    let mut solids: Vec<(f64, Region)> = Vec::new();
    for (i, c) in nearest_order_indexed(cs, start) {
        let z = zs[i] - depth;
        // Leads keep the tip `off` from the material at the tip's depth and, once past the
        // chamfer's width from the path, the flank (`off + width` at the edge) too.
        let solid = match solids.iter().find(|(sz, _)| (sz - z).abs() < 1e-9) {
            Some((_, m)) => m.clone(),
            None => {
                let m = material_above(input, z);
                solids.push((z, m.clone()));
                m
            }
        };
        let near = region::stroke(&c.walk(), 2.0 * width + 2e-3);
        let keep_out = region::union(
            &region::offset(&solid, off - 1e-3),
            &region::difference(&region::offset(&solid, off + width), &near),
        );
        let free = region::difference(&everywhere, &keep_out);
        let cut = Cut {
            material_top: zs[i],
            direct_ok: false,
            free: Some(&free),
            free_side: Some(free_side(p.direction)),
            tabs: None,
            ramp: false,
            clear: None,
        };
        b.cut(&c, z, &cut);
    }
    if !b.moved() {
        return Err("No edges to chamfer".into());
    }
    Ok(b.finish())
}

/// Engrave: along the picked edges at a depth below them, in step downs.
pub fn engrave(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    if p.drive.edges.is_empty() {
        return Err("Pick the edges to engrave".into());
    }
    let (cs, _) = drive_contours(input);
    let z_edge = input.selection_z().unwrap_or(input.model.bounds().max[2]);
    let bottom = z_edge - p.depth.max(0.0);
    let levels = input.levels(z_edge, bottom, false);
    let mut b = Builder::new(input);
    let start = [input.model.bounds().min[0], input.model.bounds().min[1]];
    let ordered = nearest_order(cs.into_iter().map(|(c, _)| c).collect(), start);
    for c in ordered {
        let mut above = z_edge;
        for z in &levels {
            let cut = Cut {
                material_top: above,
                direct_ok: false,
                free: None,
                free_side: None,
                tabs: None,
                ramp: true,
                clear: None,
            };
            b.cut(&c, *z, &cut);
            above = *z;
        }
    }
    Ok(b.finish())
}

/// Flatlands: finishes the model's flat floors (or the picked ones) with rings offset inward.
pub fn flatlands(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let r = input.tool.radius();
    let top = input.top();
    let bottom = input.bottom();
    let mut floors: Vec<(Region, f64)> = selected_faces(input);
    if floors.is_empty() {
        floors = input
            .model
            .faces
            .iter()
            .filter(|f| f.faces_up() && matches!(f.surface, Surface::Plane { .. }))
            .filter_map(|f| {
                let z = f.z()?;
                let rings: Region = f
                    .loops
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
                    .collect();
                Some((region::normalize(&rings), z))
            })
            .filter(|(_, z)| *z <= top + 1e-6 && *z >= bottom - 1e-6)
            .collect();
    }
    if floors.is_empty() {
        return Err("The model has no flat floors between the heights".into());
    }
    floors.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut b = Builder::new(input);
    let limit = stock_limit(input);
    for (area, z) in floors {
        let zc = z + p.floor_stock;
        let reach = region::intersection(&region::offset(&area, r * 0.95), &limit);
        let free = free_centres(input, zc, &reach);
        let start = b.at.map_or([0.0, 0.0], |a| [a[0], a[1]]);
        for f in order_faces(region::faces(&free), start) {
            if f.area() < 1e-4 {
                continue;
            }
            clear_face(&mut b, &f, zc, zc + p.links.feed_distance);
        }
    }
    if !b.moved() {
        b.warnings
            .push("The tool doesn't fit on any of the floors".into());
    }
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineKind;
    use crate::strategy::testing::{pocket_block, stock_for};
    use crate::strategy::{Params, Pattern, generate};
    use crate::tool::{Tool, ToolKind};
    use crate::toolpath::Step;

    fn run(pattern: Pattern, tool: Tool, f: impl FnOnce(&mut Params)) -> Toolpath {
        let m = pocket_block();
        let stock = stock_for(&m, 5.0, 2.0);
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

    fn feeds(tp: &Toolpath) -> Vec<[f64; 3]> {
        tp.steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed { to, .. } | Step::Arc { to, .. } => Some(*to),
                _ => None,
            })
            .collect()
    }

    /// No feed move's tool centre is within the radius of the model's material above the tool
    /// tip (a flat end mill).
    fn assert_no_gouge(tp: &Toolpath, r: f64) {
        let m = pocket_block();
        for p in feeds(tp) {
            // Inside the pocket's walls when below the top.
            if p[2] < 20.0 - 1e-6 {
                let in_pocket = p[0] >= 15.0 + r - 1e-3
                    && p[0] <= 45.0 - r + 1e-3
                    && p[1] >= 10.0 + r - 1e-3
                    && p[1] <= 30.0 - r + 1e-3;
                // At least the radius from the block's outline.
                let dx = (-p[0]).max(p[0] - 60.0).max(0.0);
                let dy = (-p[1]).max(p[1] - 40.0).max(0.0);
                let outside = dx.hypot(dy) >= r - 0.01;
                assert!(in_pocket || outside || p[2] >= 20.0, "gouge at {p:?}");
                if in_pocket {
                    assert!(p[2] >= 12.0 - 1e-6, "below the floor at {p:?}");
                }
            }
            let _ = &m;
        }
    }

    #[test]
    fn rough_clears_the_pocket_without_gouging() {
        let tp = run(Pattern::Rough3D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.wall_stock = 0.0;
            p.floor_stock = 0.0;
            p.step_down = 4.0;
            p.heights.bottom = super::super::Height::new(super::super::HeightRef::ModelTop, -8.0);
        });
        assert_no_gouge(&tp, 3.0);
        // It reaches the pocket floor.
        let pts = feeds(&tp);
        assert!(
            pts.iter()
                .any(|p| (p[2] - 12.0).abs() < 1e-6 && p[0] > 20.0 && p[0] < 40.0)
        );
    }

    #[test]
    fn rough_by_regions_covers_the_same_levels() {
        let a = run(Pattern::Rough2D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.step_down = 4.0;
            p.heights.bottom = super::super::Height::new(super::super::HeightRef::ModelTop, -8.0);
        });
        let b = run(Pattern::Rough2D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.step_down = 4.0;
            p.machine_by = MachineBy::Regions;
            p.heights.bottom = super::super::Height::new(super::super::HeightRef::ModelTop, -8.0);
        });
        let la = a.stats([0.0, 0.0, 50.0], 5000.0).feed_length;
        let lb = b.stats([0.0, 0.0, 50.0], 5000.0).feed_length;
        assert!((la - lb).abs() / la < 0.2, "{la} {lb}");
    }

    #[test]
    fn rough_with_picked_floor_stays_in_pocket() {
        let tp = run(Pattern::Rough2D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.drive.faces = vec!["floor".into()];
            p.wall_stock = 0.0;
            p.floor_stock = 0.0;
        });
        for q in feeds(&tp) {
            if q[2] < 20.0 {
                assert!(
                    q[0] > 15.0 && q[0] < 45.0 && q[1] > 10.0 && q[1] < 30.0,
                    "{q:?}"
                );
            }
        }
        assert!(feeds(&tp).iter().any(|q| (q[2] - 12.0).abs() < 1e-6));
    }

    #[test]
    fn profile_runs_outside_with_tabs() {
        let tp = run(Pattern::Profile2D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.step_down = 10.0;
            p.tabs.enabled = true;
            p.heights.bottom = super::super::Height::new(super::super::HeightRef::ModelBottom, 0.0);
        });
        let pts = feeds(&tp);
        // Cut at the bottom, 3 mm off the outline, with tabs lifted to 1.5.
        assert!(
            pts.iter()
                .any(|p| p[2].abs() < 1e-6 && (p[1] + 3.0).abs() < 1e-3)
        );
        assert!(pts.iter().any(|p| (p[2] - 1.5).abs() < 1e-6));
        for p in &pts {
            let inside = p[0] > 0.0 && p[0] < 60.0 && p[1] > 0.0 && p[1] < 40.0;
            assert!(!inside, "{p:?}");
        }
    }

    /// With tabs and no room for a lead (leads off), the levels under the tabs' top ramp in
    /// back and forth along the contour's start: no plunge deeper than the feed distance.
    #[test]
    fn tabbed_levels_ramp_in() {
        let tp = run(Pattern::Profile2D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.step_down = 5.0;
            p.tabs.enabled = true;
            p.links.lead_in = crate::strategy::LeadKind::None;
            p.links.lead_out = crate::strategy::LeadKind::None;
        });
        let mut at = [0.0, 0.0, 100.0];
        let mut ramps = 0;
        for s in &tp.steps {
            match s {
                Step::Feed { to, kind, .. } => {
                    if *kind == crate::toolpath::FeedKind::Plunge {
                        assert!(
                            at[2] - to[2] <= 1.0 + 1e-6,
                            "a plunge from {at:?} to {to:?}"
                        );
                    }
                    if *kind == crate::toolpath::FeedKind::Ramp {
                        ramps += 1;
                    }
                    at = *to;
                }
                Step::Rapid(p) => at = *p,
                Step::Arc { to, .. } => at = *to,
                _ => {}
            }
        }
        assert!(ramps > 0);
    }

    #[test]
    fn climb_profile_runs_clockwise_round_the_outline() {
        let tp = run(Pattern::Profile2D, Tool::new(ToolKind::EndMill, 6.0), |p| {
            p.step_down = 30.0;
            p.links.lead_in = super::super::LeadKind::None;
            p.links.lead_out = super::super::LeadKind::None;
        });
        let pts: Vec<P2> = feeds(&tp)
            .iter()
            .filter(|p| p[2].abs() < 1e-6)
            .map(|p| [p[0], p[1]])
            .collect();
        assert!(ring_area2(&pts) < 0.0);
    }

    #[test]
    fn flatlands_finishes_the_pocket_floor() {
        let tp = run(
            Pattern::Flatlands,
            Tool::new(ToolKind::EndMill, 6.0),
            |_| {},
        );
        let pts = feeds(&tp);
        let floor: Vec<_> = pts.iter().filter(|p| (p[2] - 12.0).abs() < 1e-6).collect();
        assert!(!floor.is_empty());
        for p in floor {
            assert!(p[0] >= 18.0 - 1e-3 && p[0] <= 42.0 + 1e-3, "{p:?}");
        }
    }

    #[test]
    fn face_covers_the_stock() {
        let tp = run(Pattern::Face, Tool::new(ToolKind::FaceMill, 50.0), |p| {
            p.step_down = 1.0;
        });
        let pts = feeds(&tp);
        let lowest = pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
        assert!((lowest - 20.0).abs() < 1e-6);
        let minx = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        assert!(minx < -5.0 - 25.0);
    }

    #[test]
    fn chamfer_follows_top_edges() {
        let tp = run(
            Pattern::Chamfer2D,
            Tool::new(ToolKind::ChamferMill, 6.0),
            |p| {
                p.depth = 0.5;
                p.tip_offset = 0.5;
            },
        );
        let pts = feeds(&tp);
        // 90° tool: tip 1 mm under the top, 0.5 mm off the edge.
        assert!(pts.iter().all(|p| (p[2] - 19.0).abs() < 1e-6));
        assert!(pts.iter().any(|p| (p[1] + 0.5).abs() < 1e-3));
        // Inside the pocket too (the top face's inner loop).
        assert!(
            pts.iter()
                .any(|p| (p[1] - 10.5).abs() < 1e-3 && p[0] > 20.0)
        );
    }

    #[test]
    fn engrave_needs_edges() {
        let m = pocket_block();
        let stock = stock_for(&m, 5.0, 2.0);
        let tool = Tool::new(ToolKind::ChamferMill, 6.0);
        let p = Params::new(Pattern::Engrave, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        assert!(generate(&input).is_err());
    }

    fn gouge(tp: &Toolpath, tool: &Tool) -> f64 {
        let (g, at) = crate::strategy::testing::max_gouge(&pocket_block(), tool, tp);
        assert!(g < 1e-3, "{g} mm into the part at {at:?}");
        g
    }

    #[test]
    fn face_of_a_picked_floor_keeps_off_its_walls() {
        // The pocket's floor, 8 mm under the top and walled all round.
        let tool = Tool::new(ToolKind::EndMill, 10.0);
        let tp = run(Pattern::Face, tool.clone(), |p| {
            p.drive.faces = vec!["floor".into()];
            p.heights.bottom = crate::strategy::Height {
                from: crate::strategy::HeightRef::Selection,
                offset: 0.0,
            };
            p.step_down = 2.0;
            p.floor_stock = 0.0;
        });
        gouge(&tp, &tool);
        // It reaches the floor.
        assert!(feeds(&tp).iter().any(|p| (p[2] - 12.0).abs() < 1e-6));
    }

    #[test]
    fn links_that_stay_down_never_cross_the_part() {
        // Every gap a "small gap" linked directly: the links must still go round or over the
        // walls, never through them.
        let tool = Tool::new(ToolKind::EndMill, 3.0);
        for sub in [SubPattern::Offset, SubPattern::Parallel] {
            let tp = run(Pattern::Rough2D, tool.clone(), |p| {
                p.sub_pattern = sub;
                p.cutting_method = CuttingMethod::Zigzag;
                p.step_down = 4.0;
                p.links.small_gap = 1e3;
                p.links.small_gaps = crate::strategy::LinkAction::Direct;
            });
            gouge(&tp, &tool);
        }
        let tp = run(Pattern::Profile2D, tool.clone(), |p| {
            p.passes = 3;
            p.links.small_gap = 1e3;
            p.links.small_gaps = crate::strategy::LinkAction::Direct;
        });
        gouge(&tp, &tool);
    }

    #[test]
    fn chamfer_leads_stay_out_of_the_far_wall() {
        // Leads of 12 mm in the 20 mm wide pocket would swing into its far wall.
        let tool = Tool::new(ToolKind::ChamferMill, 6.0);
        let tp = run(Pattern::Chamfer2D, tool, |p| {
            p.depth = 0.5;
            p.tip_offset = 0.5;
            p.links.lead_radius = 12.0;
        });
        let mut at = [0.0; 3];
        let off = 0.5;
        for seg in tp.motion([0.0, 0.0, 50.0], 5000.0) {
            let lead = matches!(
                seg.kind,
                Some(crate::toolpath::FeedKind::LeadIn | crate::toolpath::FeedKind::LeadOut)
            );
            if lead && seg.to[2] < 20.0 {
                for q in [seg.from, seg.to] {
                    // In the pocket: the tip stays `off` from all four walls.
                    if q[0] > 15.0 && q[0] < 45.0 && q[1] > 10.0 && q[1] < 30.0 {
                        assert!(
                            q[0] >= 15.0 + off - 1e-3
                                && q[0] <= 45.0 - off + 1e-3
                                && q[1] >= 10.0 + off - 1e-3
                                && q[1] <= 30.0 - off + 1e-3,
                            "a lead into the wall at {q:?}"
                        );
                    } else {
                        // Outside the block: off its outline.
                        let dx = (-q[0]).max(q[0] - 60.0);
                        let dy = (-q[1]).max(q[1] - 40.0);
                        assert!(dx.max(dy) >= off - 1e-3, "a lead into the block at {q:?}");
                    }
                }
            }
            at = seg.to;
        }
        let _ = at;
    }

    /// A dumbbell pocket (two lobes joined by a neck): the offset rings split at the neck.
    /// The link from one lobe's first ring to the other's never stays down through the neck
    /// (no ring has been there), even with every gap linked directly.
    #[test]
    fn rings_dont_slot_through_a_neck() {
        let mut tris = Vec::new();
        tris.extend(crate::model::box_triangles(&crate::geom::Aabb::new(
            [0.0, 0.0, 0.0],
            [80.0, 40.0, 5.0],
        )));
        // The walls round the dumbbell (two 30 × 30 lobes, a 10 wide neck), up to 15.
        for (a, b) in [
            ([0.0, 0.0], [80.0, 5.0]),
            ([0.0, 35.0], [80.0, 40.0]),
            ([0.0, 0.0], [5.0, 40.0]),
            ([75.0, 0.0], [80.0, 40.0]),
            ([35.0, 0.0], [45.0, 15.0]),
            ([35.0, 25.0], [45.0, 40.0]),
        ] {
            tris.extend(crate::model::box_triangles(&crate::geom::Aabb::new(
                [a[0], a[1], 5.0],
                [b[0], b[1], 15.0],
            )));
        }
        let m = crate::model::Model::from_triangles("dumbbell", tris);
        let stock =
            super::super::Stock::block(crate::geom::Aabb::new([0.0, 0.0, 0.0], [80.0, 40.0, 15.0]));
        let tool = Tool::new(ToolKind::EndMill, 4.0);
        let mut p = Params::new(Pattern::Rough2D, &tool, MachineKind::Mill);
        p.step_down = 10.0;
        p.step_over = 1.6;
        p.wall_stock = 0.0;
        p.floor_stock = 0.0;
        p.links.small_gap = 1e3;
        p.links.small_gaps = crate::strategy::LinkAction::Direct;
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
        // No link at depth crosses the neck's middle (x 37–43) before a ring has: the first
        // move through the neck is a ring's cut, never a link.
        let mut at = [0.0, 0.0, 50.0];
        for s in &tp.steps {
            match s {
                Step::Feed { to, kind, .. } => {
                    let crosses = (at[0] < 37.0 && to[0] > 43.0) || (at[0] > 43.0 && to[0] < 37.0);
                    if crosses && to[2] < 10.0 {
                        assert_ne!(
                            *kind,
                            crate::toolpath::FeedKind::Link,
                            "a link across the neck from {at:?} to {to:?}"
                        );
                    }
                    at = *to;
                }
                Step::Rapid(q) => at = *q,
                Step::Arc { to, .. } => at = *to,
                _ => {}
            }
        }
    }

    /// Two contours (an outline and a small hole) with three passes, the hole's outer rough
    /// pass gone (offset away): the outline's passes stay together, the hole's too.
    #[test]
    fn passes_group_by_contour() {
        let sq = |c: f64, h: f64| {
            Contour::closed(vec![
                [c - h, c - h],
                [c + h, c - h],
                [c + h, c + h],
                [c - h, c + h],
            ])
        };
        let outline = |k: f64| sq(50.0, 40.0 + k);
        let hole = |k: f64| sq(50.0, 5.0 - k);
        // (Each pass lists its contours in its own order: by position they'd mix.)
        let passes = vec![
            vec![outline(8.0)],
            vec![hole(4.0), outline(4.0)],
            vec![outline(0.0), hole(0.0)],
        ];
        let g = group_passes(&passes);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0], vec![outline(8.0), outline(4.0), outline(0.0)]);
        assert_eq!(g[1], vec![hole(4.0), hole(0.0)]);
    }
}
