//! Hit-testing and box selection in screen space.
//!
//! Both take `to_screen`, which maps a sketch point (mm) to screen pixels, so tolerances are in
//! pixels whatever the zoom and view direction. Curves are tested as polylines in screen space
//! (lines stay lines; circles and arcs are finely tessellated, so an obliquely viewed circle
//! is tested as the ellipse it appears as).

use crate::geom::{ArcGeom, dist_point_segment, segment_hits_box};
use crate::{ConstraintId, CurveId, CurveKind, DimensionId, PointId, Sketch, Vec2};

/// Something in a sketch that can be hovered and selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entity {
    Point(PointId),
    Curve(CurveId),
    Dimension(DimensionId),
    /// A constraint (its glyph).
    Constraint(ConstraintId),
    /// The sketch origin (the plane's origin, not a sketch point).
    Origin,
    /// A text entity (S16), picked on its outlines.
    Text(crate::TextId),
    /// Geometry outside the sketch picked in it: a part vertex ([`crate::Link::Vertex`]),
    /// which a constraint tool takes as the point it projects to (used first if it isn't; see
    /// [`crate::constraint::fit_op`]).
    Link(crate::Link),
}

/// A hit-test result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub entity: Entity,
    /// Distance from the cursor, in pixels.
    pub distance: f64,
}

/// Points win over curves within this many pixels.
pub const POINT_TOLERANCE_PX: f64 = 7.0;
/// Curves are hit within this many pixels.
pub const CURVE_TOLERANCE_PX: f64 = 5.0;

/// Maximum angle per segment when tessellating circles and arcs for picking.
const PICK_STEP: f64 = std::f64::consts::PI / 90.0;

/// The curve as a polyline in sketch coordinates (closed curves repeat their first point).
pub fn curve_polyline(s: &Sketch, id: CurveId) -> Vec<Vec2> {
    let Some(c) = s.curves.get(id) else {
        return Vec::new();
    };
    match c.kind {
        CurveKind::Line { a, b } => vec![s.pos(a), s.pos(b)],
        CurveKind::Circle { center, radius } => ArcGeom {
            center: s.pos(center),
            radius,
            start_angle: 0.0,
            sweep: std::f64::consts::TAU,
        }
        .tessellate(PICK_STEP, 16),
        CurveKind::Arc { .. } => s
            .arc_geom(id)
            .map(|g| g.tessellate(PICK_STEP, 4))
            .unwrap_or_default(),
        CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => s
            .ellipse_geom(id)
            .map(|g| g.tessellate(PICK_STEP, 16))
            .unwrap_or_default(),
        CurveKind::EllipseArc { .. } => s
            .ellipse_arc_geom(id)
            .map(|g| g.tessellate(PICK_STEP, 4))
            .unwrap_or_default(),
        CurveKind::Spline { .. } => s.spline_spans(id).map(|sp| crate::spline::tessellate(&sp, 8)).unwrap_or_default(),
        CurveKind::Bezier { .. } => s
            .bezier_geom(id)
            .map(|g| g.tessellate(PICK_STEP, 32))
            .unwrap_or_default(),
    }
}

/// Distance in pixels from `cursor` (screen px) to a curve.
pub fn curve_distance_px(
    s: &Sketch,
    id: CurveId,
    cursor: Vec2,
    to_screen: &impl Fn(Vec2) -> Vec2,
) -> f64 {
    let pts: Vec<Vec2> = curve_polyline(s, id).into_iter().map(to_screen).collect();
    pts.windows(2)
        .map(|w| dist_point_segment(cursor, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// The point or curve under `cursor` (screen px). Points win over curves when both are in
/// range (sketch points, then the origin), then the nearest wins.
pub fn hit_test(s: &Sketch, cursor: Vec2, to_screen: impl Fn(Vec2) -> Vec2) -> Option<Hit> {
    let point = s
        .points
        .iter()
        .map(|(k, p)| (k, to_screen(p.pos).distance(cursor)))
        .filter(|(_, d)| *d <= POINT_TOLERANCE_PX)
        // (After the distance test: `hidden_point` looks at every curve.)
        .filter(|(k, _)| !s.hidden_point(*k))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((k, d)) = point {
        return Some(Hit {
            entity: Entity::Point(k),
            distance: d,
        });
    }
    let origin = to_screen(Vec2::ZERO).distance(cursor);
    if origin <= POINT_TOLERANCE_PX {
        return Some(Hit {
            entity: Entity::Origin,
            distance: origin,
        });
    }
    let curve = s
        .curves
        .keys()
        .map(|k| (Entity::Curve(k), curve_distance_px(s, k, cursor, &to_screen)));
    let text = s
        .texts
        .keys()
        .map(|k| (Entity::Text(k), text_distance_px(s, k, cursor, &to_screen)));
    curve
        .chain(text)
        .filter(|(_, d)| *d <= CURVE_TOLERANCE_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(entity, distance)| Hit { entity, distance })
}

/// Distance in pixels from `cursor` to a text's outlines.
pub fn text_distance_px(
    s: &Sketch,
    id: crate::TextId,
    cursor: Vec2,
    to_screen: &impl Fn(Vec2) -> Vec2,
) -> f64 {
    let mut best = f64::INFINITY;
    for c in crate::text::outlines(s, id) {
        let pts: Vec<Vec2> = c.iter().map(|p| to_screen(*p)).collect();
        let n = pts.len();
        for i in 0..n {
            best = best.min(dist_point_segment(cursor, pts[i], pts[(i + 1) % n]));
        }
    }
    best
}

/// Box selection between two screen corners.
///
/// - `crossing == false` (a left-to-right drag, "window"): only entities entirely inside.
/// - `crossing == true` (right-to-left, "crossing"): also entities the box touches.
///
/// Curves' own points (line ends, centers) are selected with them when they are inside the
/// box, as Onshape does.
pub fn box_select(
    s: &Sketch,
    corner_a: Vec2,
    corner_b: Vec2,
    crossing: bool,
    to_screen: impl Fn(Vec2) -> Vec2,
) -> Vec<Entity> {
    let lo = corner_a.min(corner_b);
    let hi = corner_a.max(corner_b);
    let inside = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    let mut out = Vec::new();
    for k in s.curves.keys() {
        let pts: Vec<Vec2> = curve_polyline(s, k).into_iter().map(&to_screen).collect();
        let hit = if crossing {
            pts.windows(2).any(|w| segment_hits_box(w[0], w[1], lo, hi))
        } else {
            pts.iter().all(|p| inside(*p))
        };
        if hit {
            out.push(Entity::Curve(k));
        }
    }
    for (k, p) in &s.points {
        if inside(to_screen(p.pos)) && !s.hidden_point(k) {
            out.push(Entity::Point(k));
        }
    }
    for k in s.texts.keys() {
        let outlines = crate::text::outlines(s, k);
        let pts: Vec<Vec<Vec2>> = outlines
            .iter()
            .map(|c| c.iter().map(|p| to_screen(*p)).collect())
            .collect();
        let hit = if crossing {
            pts.iter().any(|c| {
                (0..c.len()).any(|i| segment_hits_box(c[i], c[(i + 1) % c.len()], lo, hi))
            })
        } else {
            !pts.is_empty() && pts.iter().flatten().all(|p| inside(*p))
        };
        if hit {
            out.push(Entity::Text(k));
        }
    }
    // The origin can be box-selected too (`box_select.md`).
    if inside(to_screen(Vec2::ZERO)) {
        out.push(Entity::Origin);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SketchOp;

    /// 10 px per mm, y up in the sketch and down on screen, origin at (500, 500).
    fn screen(p: Vec2) -> Vec2 {
        Vec2::new(500.0 + p.x * 10.0, 500.0 - p.y * 10.0)
    }

    fn sketch() -> Sketch {
        let mut s = Sketch::new();
        SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        SketchOp::AddCircle {
            center: Vec2::new(40.0, 0.0),
            radius: 5.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        SketchOp::AddArc {
            center: Vec2::new(0.0, 30.0),
            start: Vec2::new(5.0, 30.0),
            end: Vec2::new(-5.0, 30.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        s
    }

    fn curve_of(s: &Sketch, f: impl Fn(&CurveKind) -> bool) -> CurveId {
        s.curves.iter().find(|(_, c)| f(&c.kind)).unwrap().0
    }

    #[test]
    fn points_win_over_curves() {
        let s = sketch();
        let end = s.point_at(Vec2::new(20.0, 0.0), 1e-9).unwrap();
        // 4 px from the endpoint and 0 px from the line: the point wins.
        let h = hit_test(&s, Vec2::new(696.0, 500.0), screen).unwrap();
        assert_eq!(h.entity, Entity::Point(end));
        assert!((h.distance - 4.0).abs() < 1e-9);
    }

    #[test]
    fn line_distance_in_pixels() {
        let s = sketch();
        let line = curve_of(&s, |k| matches!(k, CurveKind::Line { .. }));
        // 3 px above the middle of the line.
        let h = hit_test(&s, Vec2::new(600.0, 497.0), screen).unwrap();
        assert_eq!(h.entity, Entity::Curve(line));
        assert!((h.distance - 3.0).abs() < 1e-9);
        // 6 px away: out of range.
        assert!(hit_test(&s, Vec2::new(600.0, 494.0), screen).is_none());
    }

    #[test]
    fn circle_and_arc_distance() {
        let s = sketch();
        let circle = curve_of(&s, |k| matches!(k, CurveKind::Circle { .. }));
        let arc = curve_of(&s, |k| matches!(k, CurveKind::Arc { .. }));
        // On the circle's top (40, 5) → screen (900, 450), 2 px outside.
        let h = hit_test(&s, Vec2::new(900.0, 448.0), screen).unwrap();
        assert_eq!(h.entity, Entity::Curve(circle));
        assert!((h.distance - 2.0).abs() < 0.05);
        // The circle's center point is hit as a point.
        let c = s.point_at(Vec2::new(40.0, 0.0), 1e-9).unwrap();
        assert_eq!(
            hit_test(&s, Vec2::new(901.0, 500.0), screen).unwrap().entity,
            Entity::Point(c)
        );
        // The arc runs ccw from (5,30) over the top to (-5,30): its top (0,35) is at (500, 150).
        let h = hit_test(&s, Vec2::new(500.0, 153.0), screen).unwrap();
        assert_eq!(h.entity, Entity::Curve(arc));
        // The bottom of that circle (0,25) is not part of the arc.
        assert!(hit_test(&s, Vec2::new(500.0, 250.0), screen).is_none());
        let d = curve_distance_px(&s, arc, Vec2::new(500.0, 250.0), &screen);
        assert!(d > 40.0);
    }

    #[test]
    fn window_versus_crossing() {
        let s = sketch();
        let line = curve_of(&s, |k| matches!(k, CurveKind::Line { .. }));
        let circle = curve_of(&s, |k| matches!(k, CurveKind::Circle { .. }));
        // A box around the left half of the line and nothing else.
        let (a, b) = (Vec2::new(480.0, 480.0), Vec2::new(600.0, 520.0));
        let window = box_select(&s, a, b, false, screen);
        assert!(!window.contains(&Entity::Curve(line)));
        // The line's start point is inside, so it is selected on its own.
        let start = s.point_at(Vec2::ZERO, 1e-9).unwrap();
        assert_eq!(window, vec![Entity::Point(start), Entity::Origin]);
        let crossing = box_select(&s, b, a, true, screen);
        assert!(crossing.contains(&Entity::Curve(line)));
        assert!(!crossing.contains(&Entity::Curve(circle)));
        // A box around the whole line and circle selects both in window mode.
        let all = box_select(&s, Vec2::new(480.0, 400.0), Vec2::new(960.0, 600.0), false, screen);
        assert!(all.contains(&Entity::Curve(line)));
        assert!(all.contains(&Entity::Curve(circle)));
        // The origin is picked when nothing else is near it.
        let away = box_select(&s, Vec2::new(-50.0, 480.0), Vec2::new(-40.0, 490.0), false, screen);
        assert!(away.is_empty());
        let mut s2 = Sketch::new();
        SketchOp::AddCircle {
            center: Vec2::new(40.0, 0.0),
            radius: 5.0,
            construction: false,
        }
        .apply(&mut s2)
        .unwrap();
        assert_eq!(hit_test(&s2, Vec2::new(502.0, 501.0), screen).unwrap().entity, Entity::Origin);
        // A small box inside the circle (not touching it) selects nothing, even crossing.
        let inner = box_select(&s, Vec2::new(880.0, 480.0), Vec2::new(890.0, 490.0), true, screen);
        assert!(inner.is_empty());
    }
}
