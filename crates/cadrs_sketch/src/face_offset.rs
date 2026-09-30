//! P3D.4 (IR6.10): Offset of a face region. Clicking a part face's region with the Offset
//! tool offsets the face's outer loop: the loop's edges come from the kernel as exact curves
//! (lines, arcs, circles, projected into the sketch), are **used** as construction geometry
//! (so they follow the face, S20), and are offset like a chain of the sketch's own curves
//! ([`crate::edit::offset`]): one driving Offset dimension, the pieces joined at their
//! corners. The offset itself is regular geometry, so it bounds a region.

use crate::edit::{chain_of, offset};
use crate::projection::Projected;
use crate::{CurveId, Link, Sketch};

/// Uses `items` (a closed loop, in order, head to tail) as construction geometry and adds its
/// offset by `distance` to the left of the loop's direction of travel (or the right), with a
/// driving Offset dimension placed at `label`. Returns the new offset curves.
pub fn offset_loop(s: &mut Sketch, items: &[(Projected, Link)], distance: f64, left: bool, label: (f64, f64)) -> Result<Vec<CurveId>, String> {
    let chain = use_loop(s, items)?;
    let made = offset(s, &chain, distance, left, label)?;
    for c in &made {
        if let Some(c) = s.curves.get_mut(*c) {
            c.construction = false;
        }
    }
    Ok(made)
}

/// Uses `items` (a closed loop) as construction geometry, its corners shared, and returns it
/// as a chain ([`chain_of`]): what [`offset_loop`] offsets, and what the Offset tool previews.
pub fn use_loop(s: &mut Sketch, items: &[(Projected, Link)]) -> Result<Vec<(CurveId, bool)>, String> {
    if items.is_empty() {
        return Err("the face has no edges to offset".into());
    }
    let mut used: Vec<CurveId> = Vec::new();
    for (shape, link) in items {
        let id = s.add_projected(*shape, *link).ok_or("the face's edges are already used")?;
        if let Some(c) = s.curves.get_mut(id) {
            c.construction = true;
        }
        used.push(id);
    }
    // The loop's corners are single points, so the pieces form one chain.
    let tol = 1e-6 * loop_size(items).max(1.0);
    let ends: Vec<crate::PointId> = used
        .iter()
        .filter_map(|c| s.curve_ends(*c))
        .flat_map(|(a, b)| [a, b])
        .collect();
    for (i, &p) in ends.iter().enumerate() {
        if !s.points.contains_key(p) {
            continue;
        }
        for &q in &ends[i + 1..] {
            if q != p && s.points.contains_key(q) && s.pos(p).distance(s.pos(q)) <= tol {
                s.merge_points(p, q);
            }
        }
    }
    let (chain, closed) = chain_of(s, used[0]);
    if chain.len() != used.len() || (!closed && used.len() > 1) {
        return Err("the face's outer loop isn't closed".into());
    }
    Ok(chain)
}

/// True if `p` (sketch coordinates) is inside the closed loop `items`.
pub fn loop_contains(items: &[(Projected, Link)], p: crate::Vec2) -> bool {
    let mut s = Sketch::new();
    if use_loop(&mut s, items).is_err() {
        return false;
    }
    for c in s.curves.values_mut() {
        c.construction = false;
    }
    crate::region::regions(&s).iter().any(|r| r.contains(p))
}

fn loop_size(items: &[(Projected, Link)]) -> f64 {
    let mut lo = crate::Vec2::new(f64::MAX, f64::MAX);
    let mut hi = crate::Vec2::new(f64::MIN, f64::MIN);
    for (p, _) in items {
        let pts: Vec<crate::Vec2> = match *p {
            Projected::Line(a, b) => vec![a, b],
            Projected::Circle(c, r) => vec![c - crate::Vec2::new(r, r), c + crate::Vec2::new(r, r)],
            Projected::Arc { start, end, .. } => vec![start, end],
            _ => vec![],
        };
        for q in pts {
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    (hi - lo).length()
}

/// Orders `items` (a face's edges) head to tail into loops and returns the outer one (the
/// largest): what [`offset_loop`] takes. Circles are loops of their own.
pub fn outer_loop(items: &[(Projected, Link)]) -> Vec<(Projected, Link)> {
    let ends = |p: &Projected| match *p {
        Projected::Line(a, b) => Some((a, b)),
        Projected::Arc { start, end, .. } => Some((start, end)),
        _ => None,
    };
    let mut loops: Vec<Vec<(Projected, Link)>> = Vec::new();
    let mut left: Vec<(Projected, Link)> = Vec::new();
    for it in items {
        if matches!(it.0, Projected::Circle(..)) {
            loops.push(vec![*it]);
        } else if ends(&it.0).is_some() {
            left.push(*it);
        }
    }
    let size = loop_size(items).max(1.0);
    let near = |a: crate::Vec2, b: crate::Vec2| a.distance(b) <= 1e-6 * size;
    while let Some(first) = left.pop() {
        let mut lp = vec![first];
        let (start, mut at) = ends(&first.0).expect("filtered");
        while !near(at, start) {
            let Some(i) = left.iter().position(|(p, _)| ends(p).is_some_and(|(a, b)| near(a, at) || near(b, at))) else {
                break;
            };
            let (p, l) = left.swap_remove(i);
            let (a, b) = ends(&p).expect("filtered");
            // Lines run either way; an arc's ends are its own, so it stays as it is.
            let p = match p {
                Projected::Line(..) if near(b, at) => Projected::Line(b, a),
                other => other,
            };
            at = if near(a, at) { b } else { a };
            lp.push((p, l));
        }
        loops.push(lp);
    }
    loops
        .into_iter()
        .max_by(|a, b| loop_size(a).total_cmp(&loop_size(b)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeTag, Vec2};

    fn link(n: u32) -> Link {
        let op = uuid::Uuid::from_u128(7);
        Link::Edge {
            feature: op,
            edge: EdgeTag::Lateral { region: n, from: CurveId::default(), to: CurveId::default() }.to_name(op),
        }
    }

    /// The Conrod stand-in's web face (inch): full width 1.0 at y = 1 and 0.6 at y = 5.
    fn trapezoid() -> Vec<(Projected, Link)> {
        let p = [Vec2::new(-0.5, 1.0), Vec2::new(0.5, 1.0), Vec2::new(0.3, 5.0), Vec2::new(-0.3, 5.0)];
        // Out of order and some reversed, as a face's edges come.
        vec![
            (Projected::Line(p[2], p[1]), link(0)),
            (Projected::Line(p[3], p[0]), link(1)),
            (Projected::Line(p[0], p[1]), link(2)),
            (Projected::Line(p[3], p[2]), link(3)),
        ]
    }

    #[test]
    fn a_face_loop_offsets_inward_as_one_region() {
        let items = outer_loop(&trapezoid());
        assert_eq!(items.len(), 4);
        let mut s = Sketch::new();
        // The loop runs from (0.3, 5) to (0.5, 1): clockwise? Try both sides and keep the one
        // inside (smaller area).
        let area = |left: bool| {
            let mut t = s.clone();
            offset_loop(&mut t, &items, 0.1, left, (0.0, 0.0)).unwrap();
            let rs = crate::region::regions(&t);
            assert_eq!(rs.len(), 1, "construction loop, one regular offset region");
            rs[0].area()
        };
        let a = area(true).min(area(false));
        // (2·0.39487508 + 2·0.20487508) / 2 × 3.8.
        let w = |y: f64| 0.5 - 0.05 * (y - 1.0) - 0.1 * (1.0f64 + 0.0025).sqrt();
        let expected = (w(1.1) + w(4.9)) * 3.8;
        assert!((a - expected).abs() < 1e-9, "{a} vs {expected}");
        assert!((a - 2.279_051).abs() < 1e-6);
        // The offset has one driving Offset dimension.
        offset_loop(&mut s, &items, 0.1, true, (0.0, 0.0)).unwrap();
        let dims = s.dimensions.values().filter(|d| matches!(d.kind, crate::DimensionKind::Offset { .. })).count();
        assert_eq!(dims, 1);
    }
}
