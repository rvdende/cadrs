//! Snapping to part edges in the sketch plane (`docs/IDEAS_TRACING.md`).
//!
//! A sketch on a part face has that plane's part edges as imprints, each linked to its edge
//! ([`crate::Imprint::link`]). [`External::of`] makes a copy of the sketch with those edges
//! added as construction curves, so inference ([`crate::infer`]) snaps to their ends, centres,
//! midpoints and bodies like to the sketch's own geometry. The copy is only for snapping: the
//! edges are not in the sketch.
//!
//! [`External::commit`] turns an edit made against the copy into one for the sketch: the part
//! edges the new geometry touches are used first (construction, linked, fixed: they follow the
//! part), and constraints that refer to the copy's curves refer to the used ones instead.

use std::collections::HashMap;

use crate::constraint::{CurveSpec, PointSpec};
use crate::ops::SketchOp;
use crate::projection::Projected;
use crate::{ConstraintOf, CurveId, CurveKind, ImprintShape, Link, PointId, Sketch, Vec2};

/// A sketch with its snappable part edges added (see the module docs).
#[derive(Debug, Clone)]
pub struct External {
    /// The sketch and the edges, as construction curves.
    pub sketch: Sketch,
    /// The added curves: the shape to use and the edge it links to.
    curves: HashMap<CurveId, (Projected, Link)>,
}

fn projected(shape: ImprintShape) -> Projected {
    match shape {
        ImprintShape::Line(a, b) => Projected::Line(a, b),
        ImprintShape::Circle(c, r) => Projected::Circle(c, r),
        ImprintShape::Arc { center, radius, start_angle, sweep } => {
            let at = |t: f64| center + Vec2::new(t.cos(), t.sin()) * radius;
            Projected::Arc { center, start: at(start_angle), end: at(start_angle + sweep) }
        }
    }
}

impl External {
    /// The sketch with the linked imprints it hasn't used yet; `None` when there are none.
    pub fn of(s: &Sketch) -> Option<Self> {
        let used: Vec<Link> = s
            .constraints
            .values()
            .filter_map(|c| match *c {
                ConstraintOf::Use(_, l) => Some(l),
                _ => None,
            })
            .collect();
        let mut sketch = s.clone();
        let mut curves = HashMap::new();
        for im in &s.imprint {
            let Some(link) = im.link.filter(|l| !used.contains(l)) else { continue };
            let shape = projected(im.shape);
            if shape.degenerate() {
                continue;
            }
            // Edges meeting at a vertex share its point (one snap target there).
            let kind = match shape {
                Projected::Line(a, b) => CurveKind::Line { a: sketch.ensure_point(a), b: sketch.ensure_point(b) },
                Projected::Circle(c, radius) => CurveKind::Circle { center: sketch.ensure_point(c), radius },
                Projected::Arc { center, start, end } => CurveKind::Arc {
                    center: sketch.ensure_point(center),
                    start: sketch.ensure_point(start),
                    end: sketch.ensure_point(end),
                },
                _ => continue,
            };
            let id = sketch.curves.insert(crate::Curve { kind, construction: true });
            curves.insert(id, (shape, link));
        }
        (!curves.is_empty()).then_some(Self { sketch, curves })
    }

    /// True for a curve that is a part edge (not in the sketch).
    pub fn is_edge(&self, c: CurveId) -> bool {
        self.curves.contains_key(&c)
    }

    /// The part edges through a point of the copy.
    fn edges_at(&self, p: PointId) -> impl Iterator<Item = CurveId> + '_ {
        self.curves.keys().copied().filter(move |k| self.sketch.curve_points(*k).contains(&p))
    }

    /// A point of the copy that only part edges use (not one of the sketch's).
    fn edge_point(&self, base: &Sketch, p: PointId) -> bool {
        !base.points.contains_key(p) && self.edges_at(p).next().is_some()
    }

    /// Where a part edge will be once used, for a constraint to find it.
    fn spec(&self, c: CurveId) -> Option<CurveSpec> {
        Some(match self.curves.get(&c)?.0 {
            Projected::Line(a, b) => CurveSpec::Between(a, b),
            Projected::Circle(c, r) => CurveSpec::Circle(c, r),
            Projected::Arc { start, end, .. } => CurveSpec::Between(start, end),
            _ => return None,
        })
    }

    /// The edit `op`, made against [`Self::sketch`], for `base` (the sketch the copy was made
    /// from): the part edges it touches are used first, and its constraints refer to them.
    /// Anything else is returned as it is.
    pub fn commit(&self, base: &Sketch, op: SketchOp) -> SketchOp {
        // Which edges it touches: try it on the copy (without solving).
        let mut trial = self.sketch.clone();
        let mut needed: Vec<CurveId> = Vec::new();
        let need = |c: CurveId, needed: &mut Vec<CurveId>| {
            if !needed.contains(&c) {
                needed.push(c);
            }
        };
        // A point placed on an edge's point is that point (the edge's, once used).
        let mut on_edge_points: Vec<Vec2> = Vec::new();
        visit(&op, &mut |o| {
            if let SketchOp::AddPoint { pos } = o
                && let Some(p) = self.sketch.point_at(*pos, crate::MERGE_EPS)
                && self.edge_point(base, p)
            {
                on_edge_points.push(*pos);
                if let Some(c) = self.edges_at(p).next() {
                    need(c, &mut needed);
                }
            }
        });
        let op = if on_edge_points.is_empty() { op } else { without_points(op, &on_edge_points) };
        if op.apply_raw(&mut trial).is_ok() {
            for k in trial.curves.keys() {
                if self.sketch.curves.contains_key(k) {
                    continue;
                }
                for p in trial.curve_points(k) {
                    if self.edge_point(base, p)
                        && let Some(e) = self.edges_at(p).next()
                    {
                        need(e, &mut needed);
                    }
                }
            }
            for (k, c) in &trial.constraints {
                if self.sketch.constraints.contains_key(k) {
                    continue;
                }
                for r in c.curves() {
                    if let crate::CurveRef::Curve(e) = r
                        && self.is_edge(e)
                    {
                        need(e, &mut needed);
                    }
                }
                for r in c.points() {
                    if let crate::PointRef::Point(p) = r
                        && self.edge_point(base, p)
                        && let Some(e) = self.edges_at(p).next()
                    {
                        need(e, &mut needed);
                    }
                }
            }
        }
        if needed.is_empty() {
            return op;
        }
        let items: Vec<(Projected, Link)> = needed.iter().filter_map(|c| self.curves.get(c).cloned()).collect();
        let op = self.rewrite(op);
        match op {
            SketchOp::Batch(ops) if ops.is_empty() => SketchOp::UseConstruction { items },
            SketchOp::Batch(mut ops) => {
                ops.insert(0, SketchOp::UseConstruction { items });
                SketchOp::Batch(ops)
            }
            op => SketchOp::Batch(vec![SketchOp::UseConstruction { items }, op]),
        }
    }

    /// The part edges a dimension made against [`Self::sketch`] measures, or whose ends it
    /// measures from, to use (construction, linked) before it goes into `base`, the sketch the
    /// copy was made from. Empty when it measures only the sketch's own geometry.
    pub fn dimension_uses(&self, base: &Sketch, d: &crate::DimensionKind) -> Vec<(Projected, Link)> {
        let mut edges: Vec<CurveId> = d.curves().into_iter().filter(|c| self.is_edge(*c)).collect();
        for p in d.points() {
            if self.edge_point(base, p)
                && let Some(c) = self.edges_at(p).next()
            {
                edges.push(c);
            }
        }
        let mut items: Vec<(Projected, Link)> = Vec::new();
        for c in edges {
            if let Some(item) = self.curves.get(&c)
                && !items.iter().any(|(_, l)| *l == item.1)
            {
                items.push(item.clone());
            }
        }
        items
    }

    /// A dimension made against [`Self::sketch`], for `used`: `base` (the sketch the copy was
    /// made from) after the edges of [`Self::dimension_uses`] were used. Part edges become the
    /// used curves linked to them, and their ends the used curves' points there. `None` if an
    /// edge it measures wasn't used.
    pub fn relinked_dimension(&self, base: &Sketch, used: &Sketch, d: crate::DimensionKind) -> Option<crate::DimensionKind> {
        let used_curve = |link: Link| {
            used.constraints.values().find_map(|c| match *c {
                ConstraintOf::Use(crate::CurveRef::Curve(k), l) if l == link => Some(k),
                _ => None,
            })
        };
        let cv = |c: CurveId| {
            if !self.is_edge(c) {
                return Some(c);
            }
            used_curve(self.curves.get(&c)?.1)
        };
        let pt = |p: PointId| {
            if !self.edge_point(base, p) {
                return Some(p);
            }
            let at = self.sketch.pos(p);
            self.edges_at(p).find_map(|e| {
                let k = used_curve(self.curves.get(&e)?.1)?;
                used.curve_points(k).into_iter().find(|q| used.pos(*q).distance(at) <= crate::MERGE_EPS)
            })
        };
        d.map_ids(pt, cv)
    }

    /// `op` with its constraints' references to part edges by position.
    fn rewrite(&self, op: SketchOp) -> SketchOp {
        match op {
            SketchOp::Batch(ops) => SketchOp::Batch(ops.into_iter().map(|o| self.rewrite(o)).collect()),
            SketchOp::AddConstraints(specs) => SketchOp::AddConstraints(
                specs
                    .into_iter()
                    .map(|c| {
                        c.map(Some::<PointSpec>, |r| {
                            Some(match r {
                                CurveSpec::Id(k) => self.spec(k).unwrap_or(r),
                                r => r,
                            })
                        })
                        .unwrap_or(c)
                    })
                    .collect(),
            ),
            op => op,
        }
    }
}

/// Calls `f` for `op` and the edits inside it.
fn visit(op: &SketchOp, f: &mut impl FnMut(&SketchOp)) {
    f(op);
    if let SketchOp::Batch(ops) = op {
        for o in ops {
            visit(o, f);
        }
    }
}

/// `op` without its points added at `at` (the used edges bring them).
fn without_points(op: SketchOp, at: &[Vec2]) -> SketchOp {
    match op {
        SketchOp::Batch(ops) => SketchOp::Batch(
            ops.into_iter()
                .filter(|o| !matches!(o, SketchOp::AddPoint { pos } if at.contains(pos)))
                .map(|o| without_points(o, at))
                .collect(),
        ),
        SketchOp::AddPoint { pos } if at.contains(&pos) => SketchOp::Batch(Vec::new()),
        op => op,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgeName, FaceName, Imprint, synthetic_curve};

    fn link(i: u32) -> Link {
        let f = FaceName::new(uuid::Uuid::nil(), crate::FaceOrigin::Cap { region: 0, end: false });
        Link::Edge { feature: uuid::Uuid::nil(), edge: EdgeName::new(f, f, i) }
    }

    /// A sketch on a 40 × 40 face with a hole of radius 5 at (20, 20).
    fn face() -> Sketch {
        let mut s = Sketch::new();
        let c = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 40.0), Vec2::new(0.0, 40.0)];
        s.imprint = (0..4)
            .map(|i| Imprint {
                id: synthetic_curve(1, i as u32),
                shape: ImprintShape::Line(c[i], c[(i + 1) % 4]),
                link: Some(link(i as u32)),
            })
            .collect();
        s.imprint.push(Imprint {
            id: synthetic_curve(1, 4),
            shape: ImprintShape::Circle(Vec2::new(20.0, 20.0), 5.0),
            link: Some(link(4)),
        });
        s
    }

    fn uses(s: &Sketch) -> usize {
        s.constraints.values().filter(|c| matches!(c, ConstraintOf::Use(..))).count()
    }

    #[test]
    fn a_line_from_a_hole_centre_to_a_corner_uses_both_edges() {
        let base = face();
        let ext = External::of(&base).unwrap();
        // The corner is shared by two edges: one snap point.
        assert_eq!(ext.sketch.curves.len(), 5);
        assert_eq!(ext.sketch.points.len(), 5);
        let op = SketchOp::AddPolyline {
            points: vec![Vec2::new(20.0, 20.0), Vec2::new(40.0, 40.0)],
            closed: false,
            construction: false,
            label: "Add line",
        };
        let op = ext.commit(&base, op);
        assert_eq!(op.label(), "Add line");
        let mut s = base.clone();
        op.apply(&mut s).unwrap();
        // The circle and one edge at the corner, both construction; the line shares their
        // points, which the links fix.
        assert_eq!(uses(&s), 2);
        assert_eq!(s.curves.values().filter(|c| c.construction).count(), 2);
        let line = s.curves.values().find(|c| !c.construction).unwrap();
        let CurveKind::Line { a, b } = line.kind else { panic!() };
        assert_eq!(s.curves_at(a).count(), 2);
        assert_eq!(s.curves_at(b).count(), 2);
        // Used edges are no longer offered.
        assert_eq!(External::of(&s).unwrap().curves.len(), 3);
    }

    #[test]
    fn a_point_on_an_edge_refers_to_the_used_edge() {
        let base = face();
        let ext = External::of(&base).unwrap();
        let circle = ext.curves.iter().find(|(_, (p, _))| matches!(p, Projected::Circle(..))).map(|(k, _)| *k).unwrap();
        let op = SketchOp::Batch(vec![
            SketchOp::AddPoint { pos: Vec2::new(25.0, 20.0) },
            SketchOp::AddConstraints(vec![ConstraintOf::PointOnCurve(PointSpec::At(Vec2::new(25.0, 20.0)), CurveSpec::Id(circle))]),
        ]);
        let mut s = base.clone();
        ext.commit(&base, op).apply(&mut s).unwrap();
        assert_eq!(uses(&s), 1);
        assert!(s.constraints.values().any(|c| matches!(c, ConstraintOf::PointOnCurve(..))));
    }

    #[test]
    fn a_point_at_a_hole_centre_is_the_used_circle_centre() {
        let base = face();
        let ext = External::of(&base).unwrap();
        let mut s = base.clone();
        ext.commit(&base, SketchOp::AddPoint { pos: Vec2::new(20.0, 20.0) }).apply(&mut s).unwrap();
        assert_eq!(uses(&s), 1);
        assert_eq!(s.points.len(), 1);
    }

    #[test]
    fn a_point_level_with_a_corner_uses_an_edge_there() {
        let base = face();
        let ext = External::of(&base).unwrap();
        let (here, corner) = (Vec2::new(60.0, 40.0), Vec2::new(40.0, 40.0));
        let op = SketchOp::Batch(vec![
            SketchOp::AddPoint { pos: here },
            SketchOp::AddConstraints(vec![ConstraintOf::Horizontal(crate::Orient::Points(PointSpec::At(here), PointSpec::At(corner)))]),
        ]);
        let mut s = base.clone();
        ext.commit(&base, op).apply(&mut s).unwrap();
        assert_eq!(uses(&s), 1);
        assert!(s.constraints.values().any(|c| matches!(c, ConstraintOf::Horizontal(_))));
    }

    #[test]
    fn geometry_away_from_the_edges_is_left_alone() {
        let base = face();
        let ext = External::of(&base).unwrap();
        let op = SketchOp::AddCircle { center: Vec2::new(10.0, 10.0), radius: 2.0, construction: false };
        assert_eq!(ext.commit(&base, op.clone()), op);
    }

    /// The Dimension tool on a face: a distance from a sketch line to the face's top edge uses
    /// the edge first (construction, linked) and measures to it; a sketch-only dimension uses
    /// nothing.
    #[test]
    fn a_dimension_to_a_face_edge_uses_the_edge() {
        use crate::{CurveRef, Dimension, DimensionKind, PointRef};
        let mut base = face();
        let line = base.add_line(Vec2::new(5.0, 30.0), Vec2::new(25.0, 30.0));
        let ext = External::of(&base).unwrap();
        // The top edge, (40, 40) to (0, 40), in the copy.
        let top = ext
            .sketch
            .curves
            .keys()
            .find(|k| ext.is_edge(*k) && ext.sketch.curve_ends(*k).is_some_and(|(a, b)| ext.sketch.pos(a).y == 40.0 && ext.sketch.pos(b).y == 40.0))
            .unwrap();
        let Some(CurveKind::Line { a, .. }) = base.curves.get(line).map(|c| c.kind) else { unreachable!() };
        let d = DimensionKind::PointLine { p: PointRef::Point(a), line: CurveRef::Curve(top) };
        let items = ext.dimension_uses(&base, &d);
        assert_eq!(items.len(), 1);
        let mut s = base.clone();
        SketchOp::UseConstruction { items }.apply(&mut s).unwrap();
        let kind = ext.relinked_dimension(&base, &s, d).expect("the edge was used");
        let DimensionKind::PointLine { p, line: CurveRef::Curve(k) } = kind else { panic!("{kind:?}") };
        assert_eq!(p, PointRef::Point(a));
        assert!(s.is_projected(k) && s.curves[k].construction);
        // Setting it moves the sketch line (the edge is fixed): 4 below the top.
        SketchOp::SetDimension { dimension: Dimension::new(kind, 4.0, 3.0), moves: vec![], radii: vec![] }.apply(&mut s).unwrap();
        assert!((s.pos(a).y - 36.0).abs() < 1e-6, "{:?}", s.pos(a));
        // Between the sketch's own points: nothing to use.
        let Some(CurveKind::Line { a, b }) = base.curves.get(line).map(|c| c.kind) else { unreachable!() };
        assert!(ext.dimension_uses(&base, &DimensionKind::Aligned { a, b }).is_empty());
        // From a face corner (an edge's end): that edge is used and the corner is its point.
        let corner = ext.sketch.point_at(Vec2::new(0.0, 40.0), crate::MERGE_EPS).unwrap();
        let d = DimensionKind::Horizontal { a: corner, b: a };
        let items = ext.dimension_uses(&base, &d);
        assert_eq!(items.len(), 1);
        let mut s = base.clone();
        SketchOp::UseConstruction { items }.apply(&mut s).unwrap();
        let DimensionKind::Horizontal { a: c, .. } = ext.relinked_dimension(&base, &s, d).unwrap() else { unreachable!() };
        assert!(s.point_is_linked(c) && s.pos(c).distance(Vec2::new(0.0, 40.0)) < 1e-9);
    }
}
