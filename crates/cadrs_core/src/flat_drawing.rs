//! Flat pattern views' geometry (P3I.7, SM16): a sheet metal part's flat pattern
//! ([`cadrs_sheetmetal::flat::FlatPart`], kept in the rebuild's [`SheetMetalContext`]) as a
//! drawing view's edges, with persistent names (see [`cadrs_drawing::flat_view`]).
//!
//! A drawing view with [`cadrs_drawing::View::flat`] set asks for its geometry with
//! [`crate::views::ViewRequest::flat`]: the rebuild worker rebuilds the studio as for any view
//! and, instead of projecting the folded solid, lays out the flat ([`flat_geometry`]). So flat
//! views are cached, generated in the background and updated (the drawing's Update rule,
//! P3C.6) exactly like part views.
//!
//! Naming: each outline edge is keyed by the first flat piece (a wall or a bend region) whose
//! edge it lies on and that edge's index, so it keeps its name while the model's dimensions
//! change; holes by the piece edge of their first side; tangent and bend lines by their joint;
//! slits by their relief; forms by their Form feature and copy; counterbores' and countersinks'
//! outer diameters by their hole.
//!
//! Besides the outline, cut-outs, slits and bends, the view shows (SM16.3) the forms' outlines
//! and centermarks (`FlatPart::forms`, as the Part Studio's flat view), the outer diameters of
//! counterbored and countersunk holes ([`SheetMetalContext::hole_marks`], at the feature's own
//! cut-outs) and a centermark on
//! every round hole; runs of outline edges on one circle (corner break rounds, round reliefs)
//! are true arcs.

use cadrs_drawing::flat_view::{FlatBendInfo, FlatFormInfo, FlatInput, FlatLoop, circle_of, find_arcs, flat_projection, key_of, simplify_loop};
use cadrs_sheetmetal::flat::{FlatPart, PieceSource};
use cadrs_sheetmetal::poly::Seg2;

use crate::ids::PartId;
use crate::sheetmetal::SheetMetalContext;
use crate::views::{ViewGeometry, ViewRequest};

type P2 = [f64; 2];

fn p2(p: &cadrs_sheetmetal::poly::P2) -> P2 {
    [p.x, p.y]
}

fn seg(s: &Seg2) -> [P2; 2] {
    [p2(&s.a), p2(&s.b)]
}

fn source_code(s: PieceSource) -> u64 {
    match s {
        PieceSource::Wall(w) => (1 << 32) | w.0 as u64,
        PieceSource::Bend(j) => (2 << 32) | j.0 as u64,
    }
}

/// Whether segment `e` lies along `s` and overlaps it.
fn overlaps(s: [P2; 2], e: [P2; 2]) -> bool {
    let d = [s[1][0] - s[0][0], s[1][1] - s[0][1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-9 {
        return false;
    }
    let tol = 1e-6 * l.max(1.0);
    let off = |p: P2| ((p[0] - s[0][0]) * d[1] - (p[1] - s[0][1]) * d[0]).abs() / l;
    if off(e[0]) > tol || off(e[1]) > tol {
        return false;
    }
    let t = |p: P2| ((p[0] - s[0][0]) * d[0] + (p[1] - s[0][1]) * d[1]) / l;
    let (a, b) = (t(e[0]).min(t(e[1])), t(e[0]).max(t(e[1])));
    b.min(l) - a.max(0.0) > tol
}

/// The naming key of an outline segment: the first piece edge it lies on, else `fallback`.
fn edge_key(part: &FlatPart, s: [P2; 2], fallback: u64) -> u64 {
    let mut pieces: Vec<_> = part.pieces.iter().collect();
    pieces.sort_by_key(|p| source_code(p.source));
    for p in pieces {
        let loops = std::iter::once(&p.polygon.outer).chain(p.polygon.holes.iter());
        for (li, l) in loops.enumerate() {
            let n = l.len();
            for i in 0..n {
                if overlaps([p2(&l[i]), p2(&l[(i + 1) % n])], s) {
                    return key_of(&[source_code(p.source), li as u64, i as u64]);
                }
            }
        }
    }
    key_of(&[0xdead, fallback])
}

/// A point in wall `w`'s own 2D on the flat of `part` (as forms are placed).
fn hole_on_flat(ctx: &SheetMetalContext, part: &FlatPart, w: cadrs_sheetmetal::WallId, q: cadrs_sheetmetal::poly::P2) -> Option<P2> {
    let place = part.placement(w)?;
    let wall = ctx.model.wall(w)?;
    Some(p2(&place.apply(wall.flat_local(&ctx.model.params, q))))
}

/// The flat view input of `part` in sheet metal model `ctx`.
pub fn flat_input(ctx: &SheetMetalContext, part: PartId) -> Option<FlatInput> {
    let walls = &ctx.parts.iter().find(|(p, _)| *p == part)?.1;
    let flat = ctx.flat.parts.iter().find(|f| f.walls.iter().any(|w| walls.contains(w)))?;
    let table = cadrs_sheetmetal::table::table(&ctx.model);
    let mut input = FlatInput { thickness: ctx.model.params.thickness, ..Default::default() };
    let mut li = 0u64;
    for poly in &flat.outline {
        for l in std::iter::once(&poly.outer).chain(poly.holes.iter()) {
            li += 1;
            let raw: Vec<P2> = l.iter().map(p2).collect();
            if raw.len() < 2 {
                continue;
            }
            if let Some((center, radius)) = circle_of(&raw) {
                let key = edge_key(flat, [raw[0], raw[1]], li << 16);
                input.loops.push(FlatLoop::Circle { center, radius, key });
                // A counterbored or countersunk hole: its outer diameter too (the Hole feature's
                // own cut-out: its centre where the feature's hole goes through the wall).
                let mine = |m: &&crate::sheetmetal::HoleMark| {
                    (m.radius - radius).abs() <= 1e-3 * m.radius.max(1.0)
                        && m.at.iter().filter_map(|(w, q)| hole_on_flat(ctx, flat, *w, *q)).any(|c| (c[0] - center[0]).hypot(c[1] - center[1]) <= 1e-3 * radius.max(1.0))
                };
                if let Some(m) = ctx.hole_marks.iter().find(mine) {
                    input.hole_marks.push((center, m.outer, key_of(&[key, 0xC0])));
                }
                continue;
            }
            let points = simplify_loop(&raw, 1e-6);
            let n = points.len();
            let keys = (0..n).map(|i| edge_key(flat, [points[i], points[(i + 1) % n]], (li << 16) | i as u64)).collect();
            let arcs = find_arcs(&points);
            input.loops.push(FlatLoop::Polygon { points, keys, arcs });
        }
    }
    for (i, c) in flat.cuts.iter().enumerate() {
        if let Some(s) = c.slit {
            // Keyed by the relief that made it (its corner's or bend end's joints).
            let what: Vec<u64> = format!("{:?}", c.source).bytes().map(u64::from).collect();
            input.slits.push((seg(&s), key_of(&[key_of(&what), i as u64])));
        }
    }
    for (i, f) in flat.forms.iter().enumerate() {
        input.forms.push(FlatFormInfo {
            lines: f.lines.iter().map(|l| (l.points.iter().map(p2).collect(), l.closed)).collect(),
            center: p2(&f.center),
            key: key_of(&[f.source, i as u64]),
        });
    }
    for b in &flat.bends {
        let row = table.bends.iter().find(|r| r.joint == b.joint);
        input.bends.push(FlatBendInfo {
            joint: b.joint.0,
            name: b.name.clone(),
            up: b.up,
            angle_deg: row.map(|r| r.angle_deg).unwrap_or(90.0),
            radius: row.map(|r| r.radius).unwrap_or(ctx.model.params.bend_radius),
            center: seg(&b.center),
            center_visible: b.center_visible.iter().map(seg).collect(),
            tangent_visible: b.tangent_visible.iter().map(seg).collect(),
        });
    }
    Some(input)
}

/// The sheet metal model `part` belongs to, if any.
pub fn context_of(contexts: &[SheetMetalContext], part: PartId) -> Option<&SheetMetalContext> {
    contexts.iter().find(|c| c.parts.iter().any(|(p, _)| *p == part))
}

/// The parts of a rebuild that have a flat pattern (the Insert view dialog's Flat patterns
/// filter, SM16.2).
pub fn flat_parts(contexts: &[SheetMetalContext]) -> Vec<PartId> {
    contexts.iter().filter(|c| c.flat.is_ok()).flat_map(|c| c.parts.iter().map(|(p, _)| *p)).collect()
}

/// A flat pattern view's geometry (see the module docs).
pub fn flat_geometry(contexts: &[SheetMetalContext], req: &ViewRequest) -> Result<ViewGeometry, String> {
    let part = req.part.ok_or("A flat pattern view shows one sheet metal part")?;
    let ctx = context_of(contexts, part).ok_or("The part is not a sheet metal part")?;
    let input = flat_input(ctx, part).ok_or("The part has no flat pattern")?;
    let f = &req.frame;
    let frame = cadrs_drawing::Frame3::new([f.dir.x, f.dir.y, f.dir.z], [f.x.x, f.x.y, f.x.z]);
    let (projection, edges, data) = flat_projection(ctx.feature.0, &input, &frame);
    let bounds = projection.bounds().map(|(lo, hi)| ([lo.x, lo.y], [hi.x, hi.y]));
    Ok(ViewGeometry { projection, parts: vec![part], bounds, edges, flat: Some(data), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FeatureId;
    use cadrs_drawing::flat_view::{FlatEdgeKind, kind_of};
    use cadrs_sheetmetal::{Params, RipStyle, flatten, samples};

    fn context(model: cadrs_sheetmetal::Model) -> (SheetMetalContext, PartId) {
        let feature = FeatureId::new();
        let flat = flatten(&model);
        let part = PartId::new(feature, 0);
        let walls = model.walls.iter().map(|w| w.id).collect();
        (
            SheetMetalContext { feature, name: String::new(), model, flat, parts: vec![(part, walls)], active: true, wall_keys: Vec::new(), joint_keys: Vec::new(), def: None, owners: Vec::new(), editors: Vec::new(), forms: Vec::new(), corner_broken: false, hole_marks: Vec::new() },
            part,
        )
    }

    fn request(part: PartId, view: cadrs_drawing::NamedView) -> ViewRequest {
        ViewRequest {
            part: Some(part),
            frame: view.frame().view_frame(),
            options: cadrs_kernel::ProjectOptions { tolerance: 0.01, hidden: true },
            shaded: false,
            props: Vec::new(),
            appearances: Vec::new(),
            cut: None,
            intersections: false,
            flat: true,
        }
    }

    #[test]
    fn the_flat_view_is_the_flat_pattern() {
        let (ctx, part) = context(samples::open_box(Params::default(), RipStyle::EdgeJoint).unwrap());
        let g = flat_geometry(std::slice::from_ref(&ctx), &request(part, cadrs_drawing::NamedView::Top)).unwrap();
        let fp = &ctx.flat.parts[0];
        // The view's bounds are the flat's.
        let (lo, hi) = fp.bounds().unwrap();
        let (vlo, vhi) = g.bounds.unwrap();
        for (a, b) in [(vlo[0], lo.x), (vlo[1], lo.y), (vhi[0], hi.x), (vhi[1], hi.y)] {
            assert!((a - b).abs() < 1e-9, "{vlo:?} {vhi:?} vs {lo:?} {hi:?}");
        }
        // Every outline edge lies on the flat's outline, and the outline's length is all there.
        let outline: Vec<[P2; 2]> = fp
            .outline
            .iter()
            .flat_map(|p| std::iter::once(&p.outer).chain(p.holes.iter()))
            .flat_map(|l| (0..l.len()).map(move |i| [p2(&l[i]), p2(&l[(i + 1) % l.len()])]))
            .collect();
        let perimeter: f64 = outline.iter().map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])).sum();
        let mut drawn = 0.0;
        for e in &g.projection.edges {
            let name = e.source.unwrap().edge_name.unwrap();
            if kind_of(&name) != Some(FlatEdgeKind::Outline) {
                continue;
            }
            let s = [[e.points[0].x, e.points[0].y], [e.points[1].x, e.points[1].y]];
            drawn += (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
            assert!(outline.iter().any(|o| overlaps(*o, s) || overlaps(s, *o)), "{s:?} is not on the outline");
            assert!(g.edges.contains_key(&name));
        }
        assert!((drawn - perimeter).abs() < 1e-6, "{drawn} vs {perimeter}");
        // One bend line per visible centre line piece, with the flat's up flags.
        let data = g.flat.as_ref().unwrap();
        assert_eq!(data.bends.len(), fp.bends.len());
        for b in &fp.bends {
            let info = data.bend(b.joint.0).unwrap();
            assert_eq!(info.up, b.up);
            assert_eq!(data.lines_of(b.joint.0).len(), b.center_visible.len());
            assert!((info.radius - ctx.model.params.bend_radius).abs() < 1e-12);
            assert!((info.angle_deg - 90.0).abs() < 1e-9);
        }
        let tangents = g.projection.edges.iter().filter(|e| e.class == cadrs_kernel::ProjClass::Smooth).count();
        assert_eq!(tangents, fp.bends.iter().map(|b| b.tangent_visible.len()).sum::<usize>());
    }

    #[test]
    fn names_survive_a_change_of_size() {
        let mut p = Params::default();
        let (a, part) = context(samples::open_box(p, RipStyle::EdgeJoint).unwrap());
        p.bend_radius *= 2.0;
        let (mut b, _) = context(samples::open_box(p, RipStyle::EdgeJoint).unwrap());
        b.feature = a.feature;
        b.parts[0].0 = part;
        let ga = flat_geometry(std::slice::from_ref(&a), &request(part, cadrs_drawing::NamedView::Top)).unwrap();
        let gb = flat_geometry(std::slice::from_ref(&b), &request(part, cadrs_drawing::NamedView::Top)).unwrap();
        let names = |g: &ViewGeometry| {
            let mut v: Vec<_> = g.edges.keys().copied().collect();
            v.sort();
            v
        };
        assert_eq!(names(&ga), names(&gb));
        assert_ne!(ga.projection, gb.projection);
    }

    #[test]
    fn a_part_without_a_flat_is_refused() {
        let (ctx, _) = context(samples::open_box(Params::default(), RipStyle::EdgeJoint).unwrap());
        let other = PartId::new(FeatureId::new(), 0);
        assert!(flat_geometry(std::slice::from_ref(&ctx), &request(other, cadrs_drawing::NamedView::Top)).is_err());
        assert_eq!(flat_parts(std::slice::from_ref(&ctx)).len(), 1);
    }
}
