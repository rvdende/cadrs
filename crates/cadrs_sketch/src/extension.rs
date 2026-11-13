//! Where constraints touch geometry off the curves themselves, so the sketch can show it.
//!
//! A tangent between a line and a circle holds where the circle touches the line's infinite
//! extension, which may be well past the line's end; a point on a line or arc may sit on its
//! extension too. [`touch_point`] says where a tangent touches (its glyph goes there) and
//! [`extensions`] the dashed pieces that carry a line or arc on to such a point, as Onshape
//! draws them, so the two never look detached.

use std::f64::consts::TAU;

use crate::geom::{ArcGeom, norm_angle};
use crate::{Constraint, ConstraintId, CurveId, CurveKind, CurveRef, PointRef, Sketch, Vec2};

/// A piece that carries a curve on to a point off it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extension {
    /// From the line's nearer end (first) on to the point (second).
    Line(Vec2, Vec2),
    /// Round the arc's circle, from its nearer end to the point (counter-clockwise).
    Arc(ArcGeom),
}

impl Extension {
    /// The piece as a polyline (arcs in steps of about 5°).
    pub fn polyline(&self) -> Vec<Vec2> {
        match *self {
            Extension::Line(a, b) => vec![a, b],
            Extension::Arc(g) => {
                let n = ((g.sweep.abs() / 5f64.to_radians()).ceil() as usize).clamp(2, 72);
                (0..=n)
                    .map(|i| g.point_at(g.start_angle + g.sweep * i as f64 / n as f64))
                    .collect()
            }
        }
    }
}

/// A circle or arc: its center and radius.
pub fn round(s: &Sketch, c: CurveId) -> Option<(Vec2, f64)> {
    match s.curves.get(c)?.kind {
        CurveKind::Circle { center, radius } => Some((s.pos(center), radius)),
        CurveKind::Arc { .. } => s.arc_geom(c).map(|g| (g.center, g.radius)),
        _ => None,
    }
}

fn line(s: &Sketch, c: CurveId) -> Option<(Vec2, Vec2)> {
    match s.curves.get(c)?.kind {
        CurveKind::Line { a, b } => Some((s.pos(a), s.pos(b))),
        _ => None,
    }
}

/// Where a tangent constraint between two curves that share no point touches: for a line and
/// a circle or arc, the foot of the center on the line; for two circles or arcs, the point on
/// the line through their centers that lies on both. `None` for other constraints, curves
/// joined at a point (the glyph is at that point) and ellipses or splines.
pub fn touch_point(s: &Sketch, c: &Constraint) -> Option<Vec2> {
    let Constraint::Tangent(CurveRef::Curve(a), CurveRef::Curve(b)) = *c else {
        return None;
    };
    let pa = s.curve_points(a);
    if s.curve_points(b).iter().any(|p| pa.contains(p)) {
        return None;
    }
    match (line(s, a), line(s, b), round(s, a), round(s, b)) {
        (Some(l), _, _, Some(r)) | (_, Some(l), Some(r), _) => foot(l, r.0),
        (_, _, Some((c1, r1)), Some((c2, r2))) => {
            let d = c2 - c1;
            let len = d.length();
            if len < 1e-9 {
                return None;
            }
            let u = d / len;
            [c1 + u * r1, c1 - u * r1].into_iter().min_by(|p, q| {
                let e = |x: &Vec2| (x.distance(c2) - r2).abs();
                e(p).total_cmp(&e(q))
            })
        }
        _ => None,
    }
}

/// The foot of `p` on the infinite line through `l`.
fn foot((a, b): (Vec2, Vec2), p: Vec2) -> Option<Vec2> {
    let d = b - a;
    let len2 = d.dot(d);
    (len2 > 1e-18).then(|| a + d * ((p - a).dot(d) / len2))
}

/// The piece carrying curve `c` on to `p`, if `p` (on the curve's line or circle) is off the
/// curve itself: past a line's end, or outside an arc's sweep. Circles and other curves need
/// none.
pub fn extension_to(s: &Sketch, c: CurveId, p: Vec2) -> Option<Extension> {
    const EPS: f64 = 1e-6;
    match s.curves.get(c)?.kind {
        CurveKind::Line { .. } => {
            let (a, b) = line(s, c)?;
            let d = b - a;
            let len = d.length();
            if len < 1e-9 {
                return None;
            }
            let t = (p - a).dot(d) / len;
            if t < -EPS {
                Some(Extension::Line(a, p))
            } else if t > len + EPS {
                Some(Extension::Line(b, p))
            } else {
                None
            }
        }
        CurveKind::Arc { .. } => {
            let g = s.arc_geom(c)?.to_ccw();
            if g.sweep >= TAU - 1e-9 || p.distance(g.center) < 1e-9 {
                return None;
            }
            let angle = (p - g.center).angle();
            let past_start = norm_angle(angle - g.start_angle);
            if past_start <= g.sweep + EPS / g.radius.max(1e-9) {
                return None;
            }
            // On from the end, or back from the start: whichever is shorter.
            let after_end = past_start - g.sweep;
            let before_start = TAU - past_start;
            Some(Extension::Arc(if after_end <= before_start {
                ArcGeom {
                    start_angle: g.start_angle + g.sweep,
                    sweep: after_end,
                    ..g
                }
            } else {
                ArcGeom {
                    start_angle: angle,
                    sweep: before_start,
                    ..g
                }
            }))
        }
        _ => None,
    }
}

/// Every extension the sketch's constraints need: tangents touching past a line's end or
/// outside an arc, and points on a line's or arc's extension. With the constraint and the
/// curve each one carries on.
pub fn extensions(s: &Sketch) -> Vec<(ConstraintId, CurveId, Extension)> {
    let mut out = Vec::new();
    for (k, c) in &s.constraints {
        let (at, curves) = match *c {
            Constraint::Tangent(CurveRef::Curve(a), CurveRef::Curve(b)) => {
                (touch_point(s, c), vec![a, b])
            }
            Constraint::PointOnCurve(PointRef::Point(p), CurveRef::Curve(on)) => {
                (s.points.get(p).map(|p| p.pos), vec![on])
            }
            _ => continue,
        };
        let Some(at) = at else { continue };
        for curve in curves {
            if let Some(e) = extension_to(s, curve, at) {
                out.push((k, curve, e));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SketchOp;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    fn line_and_circle(s: &mut Sketch, center: Vec2) -> (CurveId, CurveId) {
        SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(10.0, 0.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }
        .apply(s)
        .unwrap();
        SketchOp::AddCircle {
            center,
            radius: 5.0,
            construction: false,
        }
        .apply(s)
        .unwrap();
        let mut ids = s.curves.keys();
        (ids.next().unwrap(), ids.next().unwrap())
    }

    #[test]
    fn tangent_past_a_lines_end_is_extended() {
        let mut s = Sketch::new();
        let (l, c) = line_and_circle(&mut s, v(30.0, 5.0));
        let t = Constraint::Tangent(CurveRef::Curve(l), CurveRef::Curve(c));
        assert_eq!(touch_point(&s, &t), Some(v(30.0, 0.0)));
        assert_eq!(
            extension_to(&s, l, v(30.0, 0.0)),
            Some(Extension::Line(v(10.0, 0.0), v(30.0, 0.0)))
        );
        // Over the line: none.
        assert_eq!(extension_to(&s, l, v(4.0, 0.0)), None);
        // A circle needs none.
        assert_eq!(extension_to(&s, c, v(30.0, 0.0)), None);
    }

    #[test]
    fn circles_touch_on_their_center_line() {
        let mut s = Sketch::new();
        for center in [v(0.0, 0.0), v(10.0, 0.0)] {
            SketchOp::AddCircle {
                center,
                radius: 5.0,
                construction: false,
            }
            .apply(&mut s)
            .unwrap();
        }
        let ids: Vec<CurveId> = s.curves.keys().collect();
        let t = Constraint::Tangent(CurveRef::Curve(ids[0]), CurveRef::Curve(ids[1]));
        let p = touch_point(&s, &t).unwrap();
        assert!(p.distance(v(5.0, 0.0)) < 1e-9);
    }

    #[test]
    fn a_point_past_an_arcs_end_is_reached_the_short_way() {
        let mut s = Sketch::new();
        // A quarter arc from (10, 0) round to (0, 10).
        SketchOp::AddArc {
            center: v(0.0, 0.0),
            start: v(10.0, 0.0),
            end: v(0.0, 10.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let arc = s.curves.keys().next().unwrap();
        // Just past the end (135°): on from the end.
        let p = Vec2::from_angle(135f64.to_radians()) * 10.0;
        let Some(Extension::Arc(g)) = extension_to(&s, arc, p) else {
            panic!("no extension");
        };
        assert!((g.sweep - 45f64.to_radians()).abs() < 1e-9);
        assert!(g.start().distance(v(0.0, 10.0)) < 1e-9);
        // Just before the start (-30°): back from the start.
        let p = Vec2::from_angle(-30f64.to_radians()) * 10.0;
        let Some(Extension::Arc(g)) = extension_to(&s, arc, p) else {
            panic!("no extension");
        };
        assert!((g.sweep - 30f64.to_radians()).abs() < 1e-9);
        assert!(g.end().distance(v(10.0, 0.0)) < 1e-9);
        // On the arc: none.
        assert_eq!(extension_to(&s, arc, Vec2::from_angle(0.5) * 10.0), None);
    }
}
