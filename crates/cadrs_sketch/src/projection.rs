//! Projected ("used") geometry (S20): curves whose shape comes from outside the sketch.
//!
//! `cadrs_core` works out where a [`crate::Link`] puts its curve in the sketch plane (a
//! [`Projected`] shape) and writes it with [`Sketch::set_projected`] whenever the document
//! regenerates; the solver holds such curves fixed ([`crate::ConstraintOf::Use`]).

use crate::constraint::{ConstraintOf, CurveRef, PointRef};
use crate::geom::ArcGeom;
use crate::{ConstraintId, Curve, CurveId, CurveKind, Link, PointId, Sketch, Vec2};

/// A shape in sketch coordinates that a link projects to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projected {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Counter-clockwise from `start` to `end` around `center`.
    Arc { center: Vec2, start: Vec2, end: Vec2 },
    /// An ellipse: center, the end of its major semi-axis, its minor radius (a circle seen at
    /// an angle).
    Ellipse { center: Vec2, major: Vec2, minor: f64 },
    /// The curve `distance` outside such an ellipse (P3.7: a part edge made from an offset
    /// ellipse).
    EllipseOffset { center: Vec2, major: Vec2, minor: f64, distance: f64 },
    /// Part of such an ellipse (an arc seen at an angle), from `start` counter-clockwise to
    /// `end`; `minor` is positive.
    EllipseArc { center: Vec2, major: Vec2, minor: f64, start: Vec2, end: Vec2 },
    /// A point (a pierce).
    Point(Vec2),
}

impl Projected {
    /// A 3-point arc (through `mid`), turned counter-clockwise.
    pub fn arc_through(start: Vec2, mid: Vec2, end: Vec2) -> Option<Projected> {
        let g = crate::geom::arc_through(start, end, mid)?;
        let g: ArcGeom = g.to_ccw();
        Some(Projected::Arc {
            center: g.center,
            start: g.start(),
            end: g.end(),
        })
    }

    /// True if it has collapsed (nothing to draw).
    pub fn degenerate(&self) -> bool {
        match *self {
            Projected::Line(a, b) => a.distance(b) < 1e-6,
            Projected::Circle(_, r) => r < 1e-6,
            Projected::Arc { center, start, end } => {
                center.distance(start) < 1e-6 || start.distance(end) < 1e-6
            }
            Projected::Ellipse { center, major, minor } | Projected::EllipseOffset { center, major, minor, .. } => {
                center.distance(major) < 1e-6 || minor.abs() < 1e-6
            }
            Projected::EllipseArc { center, major, minor, start, end } => {
                center.distance(major) < 1e-6 || minor.abs() < 1e-6 || start.distance(end) < 1e-6
            }
            Projected::Point(_) => false,
        }
    }
}

impl Sketch {
    /// Adds the curve a link projects to, with its Use constraint. Its points are its own
    /// (projected edges that meet do not share a point, so deleting one link frees just that
    /// curve). Returns the new curve, or `None` if the link is used already or the shape is a
    /// point.
    pub fn add_projected(&mut self, shape: Projected, link: Link) -> Option<CurveId> {
        if self.constraints.values().any(|c| matches!(c, ConstraintOf::Use(_, l) if *l == link)) {
            return None;
        }
        if shape.degenerate() {
            return None;
        }
        let kind = match shape {
            Projected::Line(a, b) => CurveKind::Line {
                a: self.add_point(a),
                b: self.add_point(b),
            },
            Projected::Circle(c, r) => CurveKind::Circle {
                center: self.add_point(c),
                radius: r,
            },
            Projected::Arc { center, start, end } => CurveKind::Arc {
                center: self.add_point(center),
                start: self.add_point(start),
                end: self.add_point(end),
            },
            Projected::Ellipse { center, major, minor } => CurveKind::Ellipse {
                center: self.add_point(center),
                major: self.add_point(major),
                minor,
            },
            Projected::EllipseOffset { center, major, minor, distance } => CurveKind::EllipseOffset {
                center: self.add_point(center),
                major: self.add_point(major),
                minor,
                distance,
            },
            Projected::EllipseArc { center, major, minor, start, end } => CurveKind::EllipseArc {
                center: self.add_point(center),
                major: self.add_point(major),
                minor,
                start: self.add_point(start),
                end: self.add_point(end),
            },
            Projected::Point(_) => return None,
        };
        let id = self.curves.insert(Curve {
            kind,
            construction: false,
        });
        self.constraints.insert(ConstraintOf::Use(CurveRef::Curve(id), link));
        Some(id)
    }

    /// Moves a projected curve to its shape as it is now. Returns false if the shape is of
    /// another kind than the curve (the source changed type: the link is broken).
    pub fn set_projected(&mut self, curve: CurveId, shape: Projected) -> bool {
        let Some(c) = self.curves.get(curve).copied() else {
            return false;
        };
        let moves: Vec<(PointId, Vec2)>;
        let mut scalar = None;
        match (c.kind, shape) {
            (CurveKind::Line { a, b }, Projected::Line(p, q)) => {
                // Keep the ends paired the nearer way round.
                let (pa, pb) = (self.pos(a), self.pos(b));
                if pa.distance(q) + pb.distance(p) < pa.distance(p) + pb.distance(q) {
                    moves = vec![(a, q), (b, p)];
                } else {
                    moves = vec![(a, p), (b, q)];
                }
            }
            (CurveKind::Circle { center, .. }, Projected::Circle(p, r)) => {
                moves = vec![(center, p)];
                scalar = Some(r);
            }
            (CurveKind::Arc { center, start, end }, Projected::Arc { center: c0, start: s0, end: e0 }) => {
                moves = vec![(center, c0), (start, s0), (end, e0)];
            }
            (CurveKind::Ellipse { center, major, .. }, Projected::Ellipse { center: c0, major: m0, minor }) => {
                moves = vec![(center, c0), (major, m0)];
                scalar = Some(minor);
            }
            (
                CurveKind::EllipseArc { center, major, start, end, .. },
                Projected::EllipseArc { center: c0, major: m0, minor, start: s0, end: e0 },
            ) => {
                moves = vec![(center, c0), (major, m0), (start, s0), (end, e0)];
                scalar = Some(minor);
            }
            (
                CurveKind::EllipseOffset { center, major, .. },
                Projected::EllipseOffset { center: c0, major: m0, minor, distance },
            ) => {
                moves = vec![(center, c0), (major, m0)];
                if let Some(k) = self.curves.get_mut(curve) {
                    k.kind = CurveKind::EllipseOffset { center, major, minor, distance };
                }
            }
            _ => return false,
        }
        for (p, v) in moves {
            if let Some(q) = self.points.get_mut(p) {
                q.pos = v;
            }
        }
        if let Some(v) = scalar
            && let Some(k) = self.curves.get_mut(curve)
        {
            k.kind.set_scalar(v);
        }
        true
    }

    /// Moves a pierced point to where its curve crosses the plane now.
    pub fn set_pierced(&mut self, p: PointId, at: Vec2) {
        if let Some(q) = self.points.get_mut(p) {
            q.pos = at;
        }
    }

    /// Replaces the link of a Use or Pierce constraint (a reference repaired after a rebuild
    /// renamed its source). Other constraints are left alone.
    pub fn set_link(&mut self, k: ConstraintId, link: Link) {
        if let Some(ConstraintOf::Use(_, l) | ConstraintOf::Pierce(_, l)) = self.constraints.get_mut(k) {
            *l = link;
        }
    }

    /// The links of the sketch: each Use and Pierce constraint with what it holds.
    pub fn links(&self) -> Vec<(ConstraintId, LinkTarget, Link)> {
        self.constraints
            .iter()
            .filter_map(|(k, c)| match *c {
                ConstraintOf::Use(CurveRef::Curve(curve), l) => Some((k, LinkTarget::Curve(curve), l)),
                ConstraintOf::Pierce(PointRef::Point(p), l) => Some((k, LinkTarget::Point(p), l)),
                _ => None,
            })
            .collect()
    }

    /// True if the curve is projected (held by a Use constraint).
    pub fn is_projected(&self, c: CurveId) -> bool {
        self.constraints
            .values()
            .any(|x| matches!(x, ConstraintOf::Use(CurveRef::Curve(k), _) if *k == c))
    }

    /// True if a point belongs to a projected curve or is pierced (it is external: fixed).
    pub fn point_is_linked(&self, p: PointId) -> bool {
        self.constraints.values().any(|x| match *x {
            ConstraintOf::Pierce(PointRef::Point(q), _) => q == p,
            ConstraintOf::Use(CurveRef::Curve(k), _) => self.curve_points(k).contains(&p),
            _ => false,
        })
    }

    /// True if the entity is held by a link whose source is gone (S20.2).
    pub fn curve_is_orphaned(&self, c: CurveId) -> bool {
        self.broken.iter().any(|k| {
            matches!(self.constraints.get(*k), Some(ConstraintOf::Use(CurveRef::Curve(x), _)) if *x == c)
        })
    }

    pub fn point_is_orphaned(&self, p: PointId) -> bool {
        self.broken.iter().any(|k| match self.constraints.get(*k) {
            Some(ConstraintOf::Pierce(PointRef::Point(q), _)) => *q == p,
            Some(ConstraintOf::Use(CurveRef::Curve(x), _)) => self.curve_points(*x).contains(&p),
            _ => false,
        })
    }

    /// Drops text entities whose box lost a line or corner (with the rest of the box and its
    /// aspect constraint), and broken-link marks of constraints that are gone.
    pub fn prune_derived(&mut self) {
        let gone: Vec<crate::TextId> = self
            .texts
            .iter()
            .filter(|(_, t)| {
                t.lines.iter().any(|l| !self.curves.contains_key(*l))
                    || t.corners.iter().any(|p| !self.points.contains_key(*p))
            })
            .map(|(k, _)| k)
            .collect();
        for id in gone {
            if let Some(t) = self.texts.remove(id) {
                for l in t.lines {
                    self.remove_curve(l);
                }
                self.constraints
                    .retain(|_, c| !matches!(c, ConstraintOf::TextAspect(x) if *x == id));
            }
        }
        let live: Vec<ConstraintId> = self
            .broken
            .iter()
            .copied()
            .filter(|k| self.constraints.contains_key(*k))
            .collect();
        if live.len() != self.broken.len() {
            self.broken = live.into_iter().collect();
        }
    }
}

/// What a link holds in the sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    Curve(CurveId),
    Point(PointId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeTag, solve};

    fn link(n: u128) -> Link {
        Link::Edge {
            feature: uuid::Uuid::from_u128(n),
            edge: EdgeTag::Lateral {
                region: 0,
                from: CurveId::default(),
                to: CurveId::default(),
            }
            .to_name(uuid::Uuid::from_u128(n)),
        }
    }

    #[test]
    fn projected_curves_are_fixed_and_follow_their_source() {
        let mut s = Sketch::new();
        let l = s
            .add_projected(Projected::Line(Vec2::ZERO, Vec2::new(10.0, 0.0)), link(1))
            .unwrap();
        // The same link twice is one curve.
        assert!(s.add_projected(Projected::Line(Vec2::ZERO, Vec2::new(10.0, 0.0)), link(1)).is_none());
        let c = s.add_projected(Projected::Circle(Vec2::new(5.0, 5.0), 2.0), link(2)).unwrap();
        let a = solve::analyze(&s);
        assert!(a.fully_constrained(), "{a:?}");
        // The source moves: the projection follows.
        assert!(s.set_projected(l, Projected::Line(Vec2::ZERO, Vec2::new(20.0, 0.0))));
        assert!(s.set_projected(c, Projected::Circle(Vec2::new(1.0, 1.0), 3.0)));
        assert_eq!(s.line_length(l), Some(20.0));
        assert!(!s.set_projected(c, Projected::Line(Vec2::ZERO, Vec2::new(1.0, 0.0))));
        // Deleting the link constraint frees the geometry.
        let k = s.links().into_iter().find(|(_, t, _)| *t == LinkTarget::Curve(l)).unwrap().0;
        s.constraints.remove(k);
        assert!(!s.is_projected(l));
        assert!(solve::analyze(&s).dof > 0);
    }

    #[test]
    fn arcs_through_three_points_turn_counter_clockwise() {
        let p = Projected::arc_through(Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0), Vec2::new(-1.0, 0.0))
            .unwrap();
        let Projected::Arc { center, start, end } = p else { panic!() };
        assert!(center.distance(Vec2::ZERO) < 1e-9);
        // Clockwise through the bottom: stored counter-clockwise from (-1, 0) to (1, 0).
        assert!(start.distance(Vec2::new(-1.0, 0.0)) < 1e-9 && end.distance(Vec2::new(1.0, 0.0)) < 1e-9);
    }
}
