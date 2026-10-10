//! [`SketchOp`]: the edits drawing tools make. `cadrs_core` wraps each one in a command, so
//! every sketch edit is undoable.
//!
//! Every edit ends by solving the sketch ([`crate::solve::solve`]), so geometry always satisfies
//! its constraints and dimensions (conflicting ones are left unsolved, and drawn red).

use serde::{Deserialize, Serialize};

use crate::constraint::{Constraint, ConstraintOf, CurveRef, PointRef};
use crate::solve::{self, Source};
use crate::{
    ConstraintId, ConstraintSpec, Curve, CurveId, CurveKind, Dimension, DimensionId, PointId,
    Sketch, Vec2,
};

/// The undo-menu label of an edit that came in as data (JSON), which has none.
fn default_label() -> &'static str {
    "Sketch edit"
}

/// One edit of a sketch's geometry. Serialized as data (`{"type": "add_circle", …}`), so an
/// assistant can make any edit the sketch engine makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SketchOp {
    /// Lines through `points` in order (closing back to the first if `closed`). A point placed
    /// where one already exists shares it, so chained lines and rectangle corners connect.
    AddPolyline {
        points: Vec<Vec2>,
        closed: bool,
        construction: bool,
        /// Shown in the undo menu ("Add line", "Add rectangle").
        #[serde(skip_deserializing, default = "default_label")]
        label: &'static str,
    },
    AddCircle {
        center: Vec2,
        radius: f64,
        construction: bool,
    },
    /// A center-point rectangle: four corner lines and the center point, held midway between
    /// opposite corners by a hidden constraint (as the course's `ex2-step13.png` shows it: a
    /// hollow center point, no diagonals), so placing the center places the rectangle.
    AddCenterRectangle {
        center: Vec2,
        corners: [Vec2; 4],
        construction: bool,
    },
    /// An arc counter-clockwise from `start` to `end` around `center`. `end` is projected onto
    /// the circle through `start`.
    AddArc {
        center: Vec2,
        start: Vec2,
        end: Vec2,
        construction: bool,
    },
    /// Deletes curves (and the points only they used), points (and the curves using them),
    /// dimensions and constraints.
    Delete {
        curves: Vec<CurveId>,
        points: Vec<PointId>,
        dimensions: Vec<DimensionId>,
        constraints: Vec<ConstraintId>,
    },
    /// Makes curves construction geometry, or regular geometry.
    SetConstruction {
        curves: Vec<CurveId>,
        construction: bool,
    },
    /// Moves points.
    MovePoints { moves: Vec<(PointId, Vec2)> },
    /// Sets point positions and circle radii (the end of a drag, already solved).
    SetGeometry {
        points: Vec<(PointId, Vec2)>,
        radii: Vec<(CurveId, f64)>,
    },
    /// Adds constraints between existing entities (the constraint tools). Two points made
    /// coincident become one point once solved, like curves drawn from a shared end.
    AddConstraint {
        constraints: Vec<Constraint>,
        /// Shown in the undo menu ("Add horizontal").
        #[serde(skip_deserializing, default = "default_label")]
        label: &'static str,
    },
    /// Records a driving dimension (replacing one of the same kind on the same geometry). The
    /// tool's moves and radii are a good first guess; the solve that ends every edit makes the
    /// dimension (and every constraint) hold.
    SetDimension {
        dimension: Dimension,
        moves: Vec<(PointId, Vec2)>,
        radii: Vec<(CurveId, f64)>,
    },
    /// Gives an existing dimension a new value (the Dimension tool's edit box).
    SetDimensionValue { id: DimensionId, value: f64 },
    /// Makes a dimension driving (solved, with its current measured value) or driven.
    SetDimensionDriven { id: DimensionId, driven: bool },
    /// Keeps (or with `None` drops) the expression a dimension's value was typed as (P3F.4,
    /// `#piston_d + #clearance`); the value itself is set by [`SketchOp::SetDimensionValue`].
    SetDimensionExpr { id: DimensionId, expr: Option<String> },
    /// Moves a dimension's label (see [`Dimension::offset`] and [`Dimension::along`]).
    MoveDimensionLabel {
        id: DimensionId,
        offset: f64,
        along: f64,
    },
    /// Adds constraints described by positions (see [`ConstraintSpec`]). Specs that do not
    /// resolve (the geometry is not there) and duplicates are skipped.
    AddConstraints(Vec<ConstraintSpec>),
    /// Mirrors curves in a line (the Mirror tool), linking each copy to its source with a
    /// Symmetric constraint (see [`crate::edit::mirror`]).
    Mirror { axis: CurveId, curves: Vec<CurveId> },
    /// Offsets a chain of curves (from [`crate::edit::chain_of`]) by `distance`, to the left of
    /// its direction of travel or the right, with a driving offset dimension whose label sits
    /// at `label` (its `offset`, `along`). See [`crate::edit::offset`].
    Offset {
        chain: Vec<(CurveId, bool)>,
        distance: f64,
        left: bool,
        label: (f64, f64),
    },
    /// Scales the whole sketch about `center` (the first dimension, S13.3; see
    /// [`crate::edit::scale`]).
    Scale { center: Vec2, factor: f64 },
    /// A sketch point on its own (S10.1). Refused where a point already is.
    AddPoint { pos: Vec2 },
    /// An ellipse (S8): its center, the end of its major axis and its minor radius.
    AddEllipse {
        center: Vec2,
        major: Vec2,
        minor: f64,
        construction: bool,
    },
    /// A cubic Bézier curve (S12.14, Final): its end `points[0]`, control points `points[1]`
    /// and `points[2]`, and end `points[3]`. An end placed on an existing point shares it (so a
    /// curve drawn from another's end joins it).
    AddBezier { points: [Vec2; 4], construction: bool },
    /// A polygon on a construction circle (S7, see [`crate::entity::add_polygon`]).
    AddPolygon {
        center: Vec2,
        radius: f64,
        angle: f64,
        sides: u32,
        inscribed: bool,
        construction: bool,
    },
    /// A slot round a line or arc (S6, see [`crate::entity::slot`]).
    Slot {
        source: CurveId,
        width: f64,
        equal_to: Option<CurveId>,
        construction: bool,
    },
    /// A sketch fillet at a corner (S9.1, see [`crate::entity::fillet`]).
    Fillet {
        corner: PointId,
        radius: f64,
        equal_to: Option<CurveId>,
    },
    /// A sketch chamfer at a corner (S9.2, see [`crate::entity::chamfer`]).
    Chamfer {
        corner: PointId,
        d1: f64,
        d2: f64,
        /// The extensions of a chamfer made earlier in the same use: linked to it.
        equal_to: Option<[CurveId; 2]>,
    },
    /// Trims curves at points on them (S17.1: one pick, or every curve a drag crossed, as one
    /// step) and deletes standalone points (see [`crate::modify::trim_all`]).
    Trim {
        picks: Vec<(CurveId, Vec2)>,
        points: Vec<PointId>,
    },
    /// Extends a line's or arc's free end to `to`, on curve `by` (S17.3, see
    /// [`crate::modify::extend`]).
    Extend {
        curve: CurveId,
        end: PointId,
        to: Vec2,
        by: Option<CurveId>,
    },
    /// Splits a line or arc at points on it, or a circle at two or more (S18.1, see
    /// [`crate::modify::split`]).
    Split { curve: CurveId, at: Vec<Vec2> },
    /// A text entity (S16.1): its box from the lower-left corner `origin` along `dir` (unit),
    /// `height` from the baseline to the capitals' top; the width follows from the text.
    AddText {
        origin: Vec2,
        dir: Vec2,
        height: f64,
        style: crate::text::TextStyle,
    },
    /// New text or style for a text entity (S16.3, Edit text): its box keeps its lower-left
    /// corner, direction and height.
    EditText {
        id: crate::TextId,
        style: crate::text::TextStyle,
    },
    /// Makes `line` normal to a plane (S12.10, Final re-audit): the plane's trace in the sketch
    /// (`trace`, from `link`) is used as a construction line, and the line is held square to
    /// it by a Normal constraint.
    NormalToPlane { line: CurveId, trace: (Vec2, Vec2), link: crate::Link },
    /// Projects shapes into the sketch, each linked to its source (S20, Use).
    Use {
        items: Vec<(crate::projection::Projected, crate::Link)>,
    },
    /// [`SketchOp::Use`] as construction geometry, skipping what is used already: the part
    /// edges a sketch tool snapped to ([`crate::external`]).
    UseConstruction {
        items: Vec<(crate::projection::Projected, crate::Link)>,
    },
    /// Pastes a copied sketch's geometry, constraints and dimensions (P3D.1, the feature
    /// menu's Copy sketch), moved by `offset` (see [`crate::edit::paste`]).
    Paste { sketch: Box<Sketch>, offset: Vec2 },
    /// P3D.4 (IR6.10): offsets a part face's outer loop (see [`crate::face_offset`]): the
    /// loop is used as construction geometry and offset like [`SketchOp::Offset`].
    OffsetLoop {
        items: Vec<(crate::projection::Projected, crate::Link)>,
        distance: f64,
        left: bool,
        label: (f64, f64),
    },
    /// Several edits as one command (a tool adding geometry together with its automatic
    /// constraints). The label is the first edit's.
    Batch(Vec<SketchOp>),
    /// A spline through `points` ([`crate::spline`]): closed (periodic) or open, with optional
    /// end derivatives. A point placed where one already exists shares it.
    AddSpline {
        points: Vec<Vec2>,
        periodic: bool,
        start_tangent: Option<Vec2>,
        end_tangent: Option<Vec2>,
        construction: bool,
    },
    /// Sets (or clears) an open spline's end derivatives: its handles.
    SetSplineTangents {
        curve: CurveId,
        start: Option<Vec2>,
        end: Option<Vec2>,
    },
}

impl SketchOp {
    /// Shown in the undo menu.
    pub fn label(&self) -> String {
        match self {
            SketchOp::AddPolyline { label, .. } => (*label).into(),
            SketchOp::AddCircle { .. } => "Add circle".into(),
            SketchOp::AddCenterRectangle { .. } => "Add rectangle".into(),
            SketchOp::AddArc { .. } => "Add arc".into(),
            SketchOp::Delete { .. } => "Delete".into(),
            SketchOp::SetConstruction { construction: true, .. } => "Make construction".into(),
            SketchOp::SetConstruction { .. } => "Make regular geometry".into(),
            SketchOp::MovePoints { .. } => "Move".into(),
            SketchOp::SetGeometry { .. } => "Drag".into(),
            SketchOp::AddConstraint { label, .. } => (*label).into(),
            SketchOp::SetDimension { .. } => "Dimension".into(),
            SketchOp::SetDimensionValue { .. } => "Edit dimension".into(),
            SketchOp::MoveDimensionLabel { .. } => "Move dimension".into(),
            SketchOp::SetDimensionDriven { driven: false, .. } => {
                "Change to driving dimension".into()
            }
            SketchOp::SetDimensionDriven { .. } => "Change to driven dimension".into(),
            SketchOp::SetDimensionExpr { .. } => "Edit dimension".into(),
            SketchOp::AddConstraints(_) => "Add constraints".into(),
            SketchOp::Mirror { .. } => "Mirror".into(),
            SketchOp::Offset { .. } => "Offset".into(),
            SketchOp::Scale { .. } => "Scale sketch".into(),
            SketchOp::AddPoint { .. } => "Add point".into(),
            SketchOp::AddEllipse { .. } => "Add ellipse".into(),
            SketchOp::AddBezier { .. } => "Add Bézier curve".into(),
            SketchOp::AddPolygon { .. } => "Add polygon".into(),
            SketchOp::Slot { .. } => "Slot".into(),
            SketchOp::Fillet { .. } => "Sketch fillet".into(),
            SketchOp::Chamfer { .. } => "Sketch chamfer".into(),
            SketchOp::Trim { .. } => "Trim".into(),
            SketchOp::Extend { .. } => "Extend".into(),
            SketchOp::Split { .. } => "Split".into(),
            SketchOp::AddText { .. } => "Add text".into(),
            SketchOp::EditText { .. } => "Edit text".into(),
            SketchOp::Use { .. } | SketchOp::UseConstruction { .. } => "Use".into(),
            SketchOp::NormalToPlane { .. } => "Add normal".into(),
            SketchOp::Paste { .. } => "Paste sketch".into(),
            SketchOp::OffsetLoop { .. } => "Offset".into(),
            // The edit itself, not the part edges it uses on the way.
            SketchOp::Batch(ops) => ops
                .iter()
                .find(|o| !matches!(o, SketchOp::UseConstruction { .. }))
                .or(ops.first())
                .map_or_else(|| "Edit".into(), |o| o.label()),
            SketchOp::AddSpline { .. } => "Add spline".into(),
            SketchOp::SetSplineTangents { .. } => "Spline handle".into(),
        }
    }

    /// Applies the edit and solves the sketch. Errors leave the sketch half-edited; the command
    /// layer restores it.
    pub fn apply(&self, s: &mut Sketch) -> Result<(), String> {
        self.pre_move(s);
        self.apply_raw(s)?;
        let mut held = Vec::new();
        self.held(s, &mut held);
        if !held.is_empty() {
            solve_holding(s, &held);
        } else if let Some(w) = self.weights(s) {
            solve_moving_first(s, &w);
        }
        let report = solve::solve(s);
        self.merge_coincident(s, &report.conflicting);
        s.prune_derived();
        Ok(())
    }

    /// A first guess before solving: a new Normal turns a free line about its middle to point
    /// at the circle's center, keeping its length (instead of the solve stretching it).
    fn pre_move(&self, s: &mut Sketch) {
        match self {
            SketchOp::AddConstraint { constraints, .. } => {
                for c in constraints {
                    let ConstraintOf::Normal(CurveRef::Curve(l), CurveRef::Curve(k)) = *c else {
                        continue;
                    };
                    let (Some(CurveKind::Line { a, b }), Some(center)) = (
                        s.curves.get(l).map(|c| c.kind),
                        match s.curves.get(k).map(|c| c.kind) {
                            Some(CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. }) => {
                                Some(s.pos(center))
                            }
                            _ => None,
                        },
                    ) else {
                        continue;
                    };
                    // Only a line whose ends are its own and free.
                    let free = |p: PointId| {
                        s.curves_at(p).count() == 1
                            && !s.constraints.values().any(|x| x.uses_point(p))
                            && !s.dimensions.values().any(|d| d.kind.uses_point(p))
                    };
                    let busy = s.constraints.values().any(|x| {
                        x.uses_curve(l) && !matches!(x, ConstraintOf::Normal(..))
                    });
                    if !free(a) || !free(b) || busy {
                        continue;
                    }
                    let (pa, pb) = (s.pos(a), s.pos(b));
                    let mid = pa.midpoint(pb);
                    let half = pa.distance(pb) / 2.0;
                    let to = center - mid;
                    if to.length() < 1e-9 || half < 1e-9 {
                        continue;
                    }
                    let mut dir = to.normalize();
                    if dir.dot(pb - pa) < 0.0 {
                        dir = -dir;
                    }
                    move_points(s, &[(a, mid - dir * half), (b, mid + dir * half)]);
                }
            }
            SketchOp::Batch(ops) => {
                for op in ops {
                    op.pre_move(s);
                }
            }
            _ => {}
        }
    }

    /// How much each point resists moving in this edit's solve: a dimension's new value moves
    /// its first point first, then its second, and other geometry as little as it can.
    fn weights(&self, s: &Sketch) -> Option<Vec<(PointId, f64)>> {
        match self {
            SketchOp::SetDimensionValue { id, .. } => {
                let pts = s.dimensions.get(*id)?.kind.points();
                if pts.is_empty() {
                    return None;
                }
                Some(
                    pts.iter()
                        .enumerate()
                        .map(|(i, p)| (*p, if i == 0 { 1.0 } else { 4.0 }))
                        .collect(),
                )
            }
            _ => None,
        }
    }

    /// What the edit's solve keeps still when it can: a new Symmetric constraint moves only
    /// the second entity onto the mirror image of the first (neither the first nor the axis
    /// move); a new Normal moves the line, not the curve; a new offset distance moves only the
    /// offset copies, not their sources.
    fn held(&self, s: &Sketch, out: &mut Vec<Constraint>) {
        match self {
            SketchOp::AddConstraint { constraints, .. } => {
                for c in constraints {
                    match *c {
                        ConstraintOf::SymmetricCurves(a, _, l) => {
                            out.push(ConstraintOf::FixCurve(a));
                            out.push(ConstraintOf::FixCurve(l));
                        }
                        ConstraintOf::SymmetricPoints(p, _, l) => {
                            out.push(ConstraintOf::FixPoint(p));
                            out.push(ConstraintOf::FixCurve(l));
                        }
                        // A new Normal turns the line, not the curve.
                        ConstraintOf::Normal(_, c) => out.push(ConstraintOf::FixCurve(c)),
                        _ => {}
                    }
                }
            }
            SketchOp::SetDimensionValue { id, .. } => {
                if let Some(crate::Dimension {
                    kind: crate::DimensionKind::Offset { source, .. },
                    ..
                }) = s.dimensions.get(*id)
                {
                    let source = CurveRef::Curve(*source);
                    out.push(ConstraintOf::FixCurve(source));
                    // The other pieces of the chain offset with it.
                    for c in s.constraints.values() {
                        if let ConstraintOf::EqualOffset(a, _, c, _) = *c
                            && a == source
                        {
                            out.push(ConstraintOf::FixCurve(c));
                        }
                    }
                }
                // A fillet's radius, a chamfer's distance or a slot's width changes the fillet,
                // chamfer or slot, not what it was made on: the corners (virtual sharps) and a
                // slot's spine stay.
                let entity_dim = match s.dimensions.get(*id).map(|d| d.kind) {
                    Some(crate::DimensionKind::Radius { curve } | crate::DimensionKind::Diameter { curve }) => {
                        let mut arcs = vec![curve];
                        for c in s.constraints.values() {
                            if let ConstraintOf::Equal(CurveRef::Curve(a), CurveRef::Curve(b)) = *c {
                                if a == curve {
                                    arcs.push(b);
                                } else if b == curve {
                                    arcs.push(a);
                                }
                            }
                        }
                        for k in arcs {
                            if let Some(CurveKind::Arc { center, .. }) = s.curves.get(k).map(|c| c.kind)
                                && s.curves_at(center).count() > 1
                            {
                                out.push(ConstraintOf::FixPoint(PointRef::Point(center)));
                            }
                        }
                        matches!(s.curves.get(curve).map(|c| c.kind), Some(CurveKind::Arc { .. }))
                    }
                    Some(crate::DimensionKind::Aligned { a, b }) => {
                        s.virtual_sharp(a) || s.virtual_sharp(b)
                    }
                    _ => false,
                };
                if entity_dim {
                    for p in s.points.keys().filter(|p| s.virtual_sharp(*p)) {
                        out.push(ConstraintOf::FixPoint(PointRef::Point(p)));
                    }
                }
            }
            SketchOp::Batch(ops) => {
                for op in ops {
                    op.held(s, out);
                }
            }
            _ => {}
        }
    }

    /// Merges the points of solved point–point coincident constraints this edit added.
    fn merge_coincident(&self, s: &mut Sketch, conflicting: &[Source]) {
        match self {
            SketchOp::AddConstraint { constraints, .. } => {
                for c in constraints {
                    let ConstraintOf::Coincident(PointRef::Point(a), PointRef::Point(b)) = *c
                    else {
                        continue;
                    };
                    let Some(id) = s.constraints.iter().find(|(_, x)| **x == *c).map(|(k, _)| k)
                    else {
                        continue;
                    };
                    if conflicting.contains(&Source::Constraint(id))
                        || !s.points.contains_key(a)
                        || !s.points.contains_key(b)
                        || s.pos(a).distance(s.pos(b)) > 1e-6
                        || s.curves_at(a).any(|k| s.curve_points(k).contains(&b))
                        // A used part vertex stays its own point, coincident (as in Onshape).
                        || s.is_used_vertex(a)
                        || s.is_used_vertex(b)
                    {
                        continue;
                    }
                    s.constraints.remove(id);
                    s.merge_points(a, b);
                }
            }
            SketchOp::Batch(ops) => {
                for op in ops {
                    op.merge_coincident(s, conflicting);
                }
            }
            _ => {}
        }
    }

    /// Applies the edit without solving.
    pub(crate) fn apply_raw(&self, s: &mut Sketch) -> Result<(), String> {
        match self {
            SketchOp::AddSpline { points, periodic, start_tangent, end_tangent, construction } => {
                let mut pts: Vec<Vec2> = Vec::with_capacity(points.len());
                for p in points {
                    if pts.last().is_none_or(|q: &Vec2| q.distance(*p) > crate::MERGE_EPS) {
                        pts.push(*p);
                    }
                }
                if *periodic && pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) <= crate::MERGE_EPS {
                    pts.pop();
                }
                s.add_spline(&pts, *periodic, *start_tangent, *end_tangent, *construction)
                    .map(|_| ())
                    .ok_or_else(|| if *periodic { "a closed spline needs three points".into() } else { "a spline needs two points".into() })
            }
            SketchOp::SetSplineTangents { curve, start, end } => {
                let d = s.splines.get_mut(*curve).ok_or("not a spline")?;
                if d.periodic {
                    return Err("a closed spline has no end handles".into());
                }
                d.start_tangent = *start;
                d.end_tangent = *end;
                Ok(())
            }
            SketchOp::AddPolyline {
                points,
                closed,
                construction,
                ..
            } => {
                if points.len() < 2 {
                    return Err("a line needs two points".into());
                }
                let ids: Vec<PointId> = points.iter().map(|p| s.ensure_point(*p)).collect();
                let n = ids.len();
                let segments = if *closed { n } else { n - 1 };
                for i in 0..segments {
                    let (a, b) = (ids[i], ids[(i + 1) % n]);
                    if a == b {
                        continue;
                    }
                    s.curves.insert(Curve {
                        kind: CurveKind::Line { a, b },
                        construction: *construction,
                    });
                }
                // Points left unused by a degenerate polyline.
                for id in ids {
                    if !s.point_in_use(id) {
                        s.points.remove(id);
                    }
                }
                Ok(())
            }
            SketchOp::AddCircle {
                center,
                radius,
                construction,
            } => {
                if !radius.is_finite() || *radius <= 0.0 {
                    return Err("a circle needs a positive radius".into());
                }
                let center = s.ensure_point(*center);
                s.curves.insert(Curve {
                    kind: CurveKind::Circle {
                        center,
                        radius: *radius,
                    },
                    construction: *construction,
                });
                Ok(())
            }
            SketchOp::AddCenterRectangle {
                center,
                corners,
                construction,
            } => {
                SketchOp::AddPolyline {
                    points: corners.to_vec(),
                    closed: true,
                    construction: *construction,
                    label: "",
                }
                .apply_raw(s)?;
                let c = s.ensure_point(*center);
                let (a, b) = (
                    s.point_at(corners[0], crate::MERGE_EPS),
                    s.point_at(corners[2], crate::MERGE_EPS),
                );
                if let (Some(a), Some(b)) = (a, b) {
                    s.add_constraint(ConstraintOf::Center(
                        PointRef::Point(c),
                        PointRef::Point(a),
                        PointRef::Point(b),
                    ));
                }
                Ok(())
            }
            SketchOp::AddArc {
                center,
                start,
                end,
                construction,
            } => {
                let r = center.distance(*start);
                if r <= 0.0 || center.distance(*end) <= 0.0 || start.distance(*end) < 1e-9 {
                    return Err("degenerate arc".into());
                }
                let end = *center + (*end - *center).normalize() * r;
                let c = s.ensure_point(*center);
                let a = s.ensure_point(*start);
                let b = s.ensure_point(end);
                s.curves.insert(Curve {
                    kind: CurveKind::Arc {
                        center: c,
                        start: a,
                        end: b,
                    },
                    construction: *construction,
                });
                Ok(())
            }
            SketchOp::Delete {
                curves,
                points,
                dimensions,
                constraints,
            } => {
                for d in dimensions {
                    s.dimensions.remove(*d);
                }
                for c in constraints {
                    s.constraints.remove(*c);
                }
                for c in curves {
                    s.remove_curve(*c);
                }
                for p in points {
                    if s.points.contains_key(*p) {
                        s.remove_point(*p);
                    }
                }
                Ok(())
            }
            SketchOp::SetConstruction {
                curves,
                construction,
            } => {
                for c in curves {
                    if let Some(c) = s.curves.get_mut(*c) {
                        c.construction = *construction;
                    }
                }
                Ok(())
            }
            SketchOp::MovePoints { moves } => {
                move_points(s, moves);
                Ok(())
            }
            SketchOp::SetGeometry { points, radii } => {
                move_points(s, points);
                set_radii(s, radii);
                Ok(())
            }
            SketchOp::AddConstraint { constraints, .. } => {
                let mut added = false;
                for c in constraints {
                    added |= s.add_constraint(*c);
                }
                if added {
                    Ok(())
                } else {
                    Err("the constraint is already there".into())
                }
            }
            SketchOp::SetDimension {
                dimension,
                moves,
                radii,
            } => {
                check_value(dimension, dimension.value)?;
                move_points(s, moves);
                set_radii(s, radii);
                let existing = s
                    .dimensions
                    .iter()
                    .find(|(_, d)| d.kind == dimension.kind)
                    .map(|(k, _)| k);
                match existing {
                    Some(k) => s.dimensions[k] = *dimension,
                    None => {
                        s.dimensions.insert(*dimension);
                    }
                }
                Ok(())
            }
            SketchOp::SetDimensionValue { id, value } => {
                let d = s.dimensions.get_mut(*id).ok_or("the dimension is gone")?;
                if d.driven {
                    return Err("a driven dimension cannot be edited".into());
                }
                check_value(d, *value)?;
                if let crate::DimensionKind::Sides { circle, .. } = d.kind {
                    // A polygon's side count rebuilds it (S7.4).
                    return crate::entity::set_polygon_sides(s, circle, value.round() as u32);
                }
                d.value = *value;
                Ok(())
            }
            SketchOp::SetDimensionExpr { id, expr } => {
                s.expressions.retain(|(d, _)| d != id);
                if let Some(e) = expr {
                    s.dimensions.get(*id).ok_or("the dimension is gone")?;
                    s.expressions.push((*id, e.clone()));
                }
                Ok(())
            }
            SketchOp::SetDimensionDriven { id, driven } => {
                let kind = s.dimensions.get(*id).ok_or("the dimension is gone")?.kind;
                let measured = crate::dimension::measure(s, kind);
                let d = &mut s.dimensions[*id];
                if !*driven && let Some(v) = measured {
                    d.value = v;
                }
                d.driven = *driven;
                Ok(())
            }
            SketchOp::MoveDimensionLabel { id, offset, along } => {
                let d = s.dimensions.get_mut(*id).ok_or("the dimension is gone")?;
                d.offset = *offset;
                d.along = *along;
                Ok(())
            }
            SketchOp::AddConstraints(specs) => {
                for spec in specs {
                    if let Some(c) = spec.resolve(s) {
                        s.add_constraint(c);
                    }
                }
                Ok(())
            }
            SketchOp::Mirror { axis, curves } => crate::edit::mirror(s, *axis, curves).map(|_| ()),
            SketchOp::Offset {
                chain,
                distance,
                left,
                label,
            } => crate::edit::offset(s, chain, *distance, *left, *label).map(|_| ()),
            SketchOp::Scale { center, factor } => crate::edit::scale(s, *center, *factor),
            SketchOp::AddPoint { pos } => {
                if s.point_at(*pos, crate::MERGE_EPS).is_some() {
                    return Err("a point is there already".into());
                }
                s.add_point(*pos);
                Ok(())
            }
            SketchOp::AddEllipse {
                center,
                major,
                minor,
                construction,
            } => {
                if center.distance(*major) <= 1e-9 || !minor.is_finite() || *minor <= 1e-9 {
                    return Err("an ellipse needs two radii".into());
                }
                let c = s.ensure_point(*center);
                let m = s.add_point(*major);
                s.curves.insert(Curve {
                    kind: CurveKind::Ellipse {
                        center: c,
                        major: m,
                        minor: *minor,
                    },
                    construction: *construction,
                });
                Ok(())
            }
            SketchOp::AddBezier { points, construction } => {
                if points[0].distance(points[3]) <= 1e-9 {
                    return Err("a Bézier curve needs two different ends".into());
                }
                let a = s.ensure_point(points[0]);
                let c1 = s.add_point(points[1]);
                let c2 = s.add_point(points[2]);
                let b = s.ensure_point(points[3]);
                s.curves.insert(Curve {
                    kind: CurveKind::Bezier { a, c1, c2, b },
                    construction: *construction,
                });
                Ok(())
            }
            SketchOp::AddPolygon {
                center,
                radius,
                angle,
                sides,
                inscribed,
                construction,
            } => crate::entity::add_polygon(
                s,
                *center,
                *radius,
                *angle,
                *sides,
                *inscribed,
                *construction,
            )
            .map(|_| ()),
            SketchOp::Slot {
                source,
                width,
                equal_to,
                construction,
            } => crate::entity::slot(s, *source, *width, *equal_to, *construction).map(|_| ()),
            SketchOp::Fillet {
                corner,
                radius,
                equal_to,
            } => crate::entity::fillet(s, *corner, *radius, *equal_to).map(|_| ()),
            SketchOp::Chamfer {
                corner,
                d1,
                d2,
                equal_to,
            } => crate::entity::chamfer(s, *corner, *d1, *d2, *equal_to).map(|_| ()),
            SketchOp::Trim { picks, points } => crate::modify::trim_all(s, picks, points),
            SketchOp::AddText {
                origin,
                dir,
                height,
                style,
            } => {
                if !height.is_finite() || *height <= 1e-6 || dir.length() < 1e-9 {
                    return Err("a text box needs a height".into());
                }
                if style.text.chars().count() > crate::text::MAX_CHARS {
                    return Err("text is limited to 250 characters".into());
                }
                let dir = dir.normalize();
                let c = crate::text::box_corners(style, *origin, dir, *height);
                let p = c.map(|q| s.ensure_point(q));
                let line = |s: &mut Sketch, a: PointId, b: PointId| {
                    s.curves.insert(Curve {
                        kind: CurveKind::Line { a, b },
                        construction: true,
                    })
                };
                let lines = [
                    line(s, p[0], p[1]),
                    line(s, p[1], p[2]),
                    line(s, p[2], p[3]),
                    line(s, p[3], p[0]),
                ];
                let id = s.texts.insert(crate::text::SketchText {
                    style: style.clone(),
                    corners: p,
                    lines,
                });
                let cr = |k: usize| CurveRef::Curve(lines[k]);
                // The lower edge is Horizontal (S16.2: deleting it lets the text rotate).
                if dir.y.abs() < 1e-9 {
                    s.add_constraint(ConstraintOf::Horizontal(crate::Orient::Line(cr(0))));
                }
                s.add_quiet_constraint(ConstraintOf::Perpendicular(cr(0), cr(3)));
                s.add_quiet_constraint(ConstraintOf::Parallel(cr(0), cr(2)));
                s.add_quiet_constraint(ConstraintOf::Parallel(cr(3), cr(1)));
                s.add_quiet_constraint(ConstraintOf::TextAspect(id));
                for q in p {
                    s.quiet.points.insert(q);
                }
                Ok(())
            }
            SketchOp::EditText { id, style } => {
                if style.text.chars().count() > crate::text::MAX_CHARS {
                    return Err("text is limited to 250 characters".into());
                }
                let t = s.texts.get_mut(*id).ok_or("no such text")?;
                t.style = style.clone();
                let [a, b, c, d] = t.corners;
                let (pa, pb, pd) = (s.pos(a), s.pos(b), s.pos(d));
                let height = pa.distance(pd);
                let dir = (pb - pa).normalize();
                let corners = crate::text::box_corners(style, pa, dir, height);
                move_points(s, &[(b, corners[1]), (c, corners[2]), (d, corners[3])]);
                Ok(())
            }
            SketchOp::NormalToPlane { line, trace, link } => {
                if !matches!(s.curves.get(*line).map(|c| c.kind), Some(CurveKind::Line { .. })) {
                    return Err("Normal to a plane takes a line".into());
                }
                let t = match s.add_projected(crate::projection::Projected::Line(trace.0, trace.1), *link) {
                    Some(t) => t,
                    // Used already: that line.
                    None => s
                        .constraints
                        .values()
                        .find_map(|c| match *c {
                            ConstraintOf::Use(CurveRef::Curve(k), l) if l == *link => Some(k),
                            _ => None,
                        })
                        .ok_or("the plane can't be used")?,
                };
                if let Some(c) = s.curves.get_mut(t) {
                    c.construction = true;
                }
                let c = ConstraintOf::Normal(CurveRef::Curve(*line), CurveRef::Curve(t));
                if !s.add_constraint(c) {
                    return Err("the line is normal to that plane already".into());
                }
                Ok(())
            }
            SketchOp::Use { items } => {
                let mut added = 0;
                for (shape, link) in items {
                    // A part vertex is a point; anything else a curve.
                    let new = match shape {
                        crate::projection::Projected::Point(p) => s.add_projected_point(*p, *link).is_some(),
                        _ => s.add_projected(shape.clone(), *link).is_some(),
                    };
                    added += new as usize;
                }
                if added == 0 {
                    return Err("nothing new to use".into());
                }
                Ok(())
            }
            SketchOp::UseConstruction { items } => {
                for (shape, link) in items {
                    if let crate::projection::Projected::Point(p) = shape {
                        s.add_projected_point(*p, *link);
                        continue;
                    }
                    if let Some(c) = s.add_projected(shape.clone(), *link)
                        && let Some(c) = s.curves.get_mut(c)
                    {
                        c.construction = true;
                    }
                }
                Ok(())
            }
            SketchOp::Extend { curve, end, to, by } => {
                crate::modify::extend(s, *curve, *end, *to, *by)
            }
            SketchOp::Split { curve, at } => crate::modify::split(s, *curve, at).map(|_| ()),
            SketchOp::Paste { sketch, offset } => {
                if sketch.curves.is_empty() && sketch.points.is_empty() {
                    return Err("nothing to paste".into());
                }
                crate::edit::paste(s, sketch, *offset);
                Ok(())
            }
            SketchOp::OffsetLoop { items, distance, left, label } => {
                crate::face_offset::offset_loop(s, items, *distance, *left, *label).map(|_| ())
            }
            SketchOp::Batch(ops) => {
                for op in ops {
                    op.apply_raw(s)?;
                }
                Ok(())
            }
        }
    }
}

/// Solves with the `held` fixes added, if that can be satisfied without conflicts; otherwise
/// leaves the sketch for the normal solve.
fn solve_holding(s: &mut Sketch, held: &[Constraint]) {
    let mut trial = s.clone();
    for c in held {
        if !c.is_trivial() {
            trial.constraints.insert(*c);
        }
    }
    if solve::solve(&mut trial).conflicting.is_empty() {
        for (k, p) in &trial.points {
            if let Some(q) = s.points.get_mut(k) {
                q.pos = p.pos;
            }
        }
        for (k, c) in &trial.curves {
            if let Some(d) = s.curves.get_mut(k) {
                d.kind = c.kind;
            }
        }
    }
}

/// Solves moving only the first of `order`'s points if that is enough, else the first two,
/// else everything with `order`'s weights (the rest heavier), else leaves it to the normal
/// solve.
fn solve_moving_first(s: &mut Sketch, order: &[(PointId, f64)]) {
    for n in 1..=order.len().min(2) {
        let free: Vec<PointId> = order[..n].iter().map(|(p, _)| *p).collect();
        let held: Vec<Constraint> = s
            .points
            .keys()
            .filter(|p| !free.contains(p))
            .map(|p| ConstraintOf::FixPoint(PointRef::Point(p)))
            .collect();
        let mut trial = s.clone();
        for c in &held {
            trial.constraints.insert(*c);
        }
        if solve::solve(&mut trial).conflicting.is_empty() {
            for (k, p) in &trial.points {
                if let Some(q) = s.points.get_mut(k) {
                    q.pos = p.pos;
                }
            }
            for (k, c) in &trial.curves {
                if let Some(d) = s.curves.get_mut(k) {
                    d.kind = c.kind;
                }
            }
            return;
        }
    }
    solve::solve_weighted(s, order, 25.0);
}

/// A dimension's value must be positive (and an angle less than 180°).
fn check_value(d: &Dimension, v: f64) -> Result<(), String> {
    if !v.is_finite() || v <= 0.0 {
        return Err("a dimension must be positive".into());
    }
    if matches!(d.kind, crate::DimensionKind::Angle { .. }) && v >= 180.0 {
        return Err("an angle must be less than 180°".into());
    }
    if matches!(d.kind, crate::DimensionKind::Sides { .. }) {
        let n = v.round();
        if (v - n).abs() > 1e-6
            || n < crate::entity::MIN_SIDES as f64
            || n > crate::entity::MAX_SIDES as f64
        {
            return Err("a polygon has 3 to 50 sides".into());
        }
    }
    Ok(())
}

fn set_radii(s: &mut Sketch, radii: &[(CurveId, f64)]) {
    for (c, r) in radii {
        if let Some(c) = s.curves.get_mut(*c) {
            c.kind.set_scalar(*r);
        }
    }
}

fn move_points(s: &mut Sketch, moves: &[(PointId, Vec2)]) {
    for (p, pos) in moves {
        if let Some(p) = s.points.get_mut(*p) {
            p.pos = *pos;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DimensionKind;

    #[test]
    fn a_new_normal_turns_the_line_about_its_middle() {
        let mut s = Sketch::new();
        SketchOp::AddCircle { center: Vec2::new(-35.0, 15.0), radius: 7.5, construction: false }
            .apply(&mut s)
            .unwrap();
        let l = s.add_line(Vec2::new(-12.0, -38.0), Vec2::new(8.0, -12.0));
        let before = s.line_length(l).unwrap();
        let (a, b) = s.curve_ends(l).unwrap();
        let mid = s.pos(a).midpoint(s.pos(b));
        let c = s.curves.keys().find(|k| *k != l).unwrap();
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Normal(CurveRef::Curve(l), CurveRef::Curve(c))],
            label: "Add normal",
        }
        .apply(&mut s)
        .unwrap();
        assert!((s.line_length(l).unwrap() - before).abs() < 1e-9);
        assert!(s.pos(a).midpoint(s.pos(b)).distance(mid) < 1e-9);
    }

    #[test]
    fn a_new_dimension_moves_its_first_point() {
        let mut s = Sketch::new();
        let l = s.add_line(Vec2::new(-50.0, -20.0), Vec2::new(40.0, -20.0));
        s.add_constraint(ConstraintOf::Horizontal(crate::Orient::Line(CurveRef::Curve(l))));
        let (left, right) = s.curve_ends(l).unwrap();
        crate::modify::split(&mut s, l, &[Vec2::new(5.0, -20.0)]).unwrap();
        crate::solve::solve(&mut s);
        let mid = s.point_at(Vec2::new(5.0, -20.0), 1e-6).unwrap();
        let d = s.dimensions.insert(Dimension::new(DimensionKind::Horizontal { a: mid, b: left }, 55.0, -10.0));
        SketchOp::SetDimensionValue { id: d, value: 30.0 }.apply(&mut s).unwrap();
        // The split point slid; the ends stayed.
        assert!(s.pos(left).distance(Vec2::new(-50.0, -20.0)) < 1e-6, "{:?}", s.pos(left));
        assert!(s.pos(right).distance(Vec2::new(40.0, -20.0)) < 1e-6);
        assert!(s.pos(mid).distance(Vec2::new(-20.0, -20.0)) < 1e-6, "{:?}", s.pos(mid));
    }

    fn rect(s: &mut Sketch, w: f64, h: f64) {
        SketchOp::AddPolyline {
            points: vec![
                Vec2::ZERO,
                Vec2::new(w, 0.0),
                Vec2::new(w, h),
                Vec2::new(0.0, h),
            ],
            closed: true,
            construction: false,
            label: "Add rectangle",
        }
        .apply(s)
        .unwrap();
    }

    #[test]
    fn rectangle_shares_its_corners() {
        let mut s = Sketch::new();
        rect(&mut s, 10.0, 5.0);
        assert_eq!(s.curves.len(), 4);
        assert_eq!(s.points.len(), 4);
    }

    #[test]
    fn center_rectangle_has_construction_diagonals() {
        let mut s = Sketch::new();
        SketchOp::AddCenterRectangle {
            center: Vec2::new(5.0, 5.0),
            corners: [
                Vec2::ZERO,
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(0.0, 10.0),
            ],
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.points.len(), 5);
        assert_eq!(s.curves.len(), 4);
        assert_eq!(s.curves.values().filter(|c| c.construction).count(), 0);
        let center = s.point_at(Vec2::new(5.0, 5.0), 1e-9).unwrap();
        // The center is held midway between opposite corners.
        assert!(s.constraints.values().any(|c| matches!(
            c,
            ConstraintOf::Center(PointRef::Point(p), ..) if *p == center
        )));
        assert_eq!(crate::region::regions(&s).len(), 1);
    }

    #[test]
    fn chained_lines_share_endpoints() {
        let mut s = Sketch::new();
        let add = |s: &mut Sketch, a: Vec2, b: Vec2| {
            SketchOp::AddPolyline {
                points: vec![a, b],
                closed: false,
                construction: false,
                label: "Add line",
            }
            .apply(s)
            .unwrap()
        };
        add(&mut s, Vec2::ZERO, Vec2::new(10.0, 0.0));
        add(&mut s, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0));
        assert_eq!(s.curves.len(), 2);
        assert_eq!(s.points.len(), 3);
    }

    #[test]
    fn deleting_a_line_keeps_shared_points() {
        let mut s = Sketch::new();
        rect(&mut s, 10.0, 5.0);
        let corner = s.point_at(Vec2::ZERO, 1e-9).unwrap();
        let lines: Vec<CurveId> = s.curves_at(corner).collect();
        SketchOp::Delete {
            curves: vec![lines[0]],
            points: vec![],
            dimensions: vec![],
            constraints: vec![],
        }
        .apply(&mut s)
        .unwrap();
        // Every corner is still used by another line.
        assert_eq!(s.curves.len(), 3);
        assert_eq!(s.points.len(), 4);
        // Delete the other line at that corner: the corner is left unused and goes.
        SketchOp::Delete {
            curves: vec![lines[1]],
            points: vec![],
            dimensions: vec![],
            constraints: vec![],
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.curves.len(), 2);
        assert_eq!(s.points.len(), 3);
        assert!(!s.points.contains_key(corner));
    }

    #[test]
    fn deleting_a_point_removes_its_curves() {
        let mut s = Sketch::new();
        rect(&mut s, 10.0, 5.0);
        let p = s.point_at(Vec2::ZERO, 1e-9).unwrap();
        SketchOp::Delete {
            curves: vec![],
            points: vec![p],
            dimensions: vec![],
            constraints: vec![],
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(s.curves.len(), 2);
        assert_eq!(s.points.len(), 3);
    }

    #[test]
    fn construction_toggle() {
        let mut s = Sketch::new();
        SketchOp::AddCircle {
            center: Vec2::ZERO,
            radius: 5.0,
            construction: true,
        }
        .apply(&mut s)
        .unwrap();
        let c = s.curves.keys().next().unwrap();
        assert!(s.curves[c].construction);
        SketchOp::SetConstruction {
            curves: vec![c],
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        assert!(!s.curves[c].construction);
    }

    #[test]
    fn arcs_project_their_end() {
        let mut s = Sketch::new();
        SketchOp::AddArc {
            center: Vec2::ZERO,
            start: Vec2::new(2.0, 0.0),
            end: Vec2::new(0.0, 5.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let c = s.curves.keys().next().unwrap();
        let g = s.arc_geom(c).unwrap();
        assert!((g.radius - 2.0).abs() < 1e-12);
        assert!(g.end().distance(Vec2::new(0.0, 2.0)) < 1e-12);
    }

    #[test]
    fn dimensions_replace_and_follow_deletes() {
        let mut s = Sketch::new();
        rect(&mut s, 10.0, 5.0);
        let a = s.point_at(Vec2::ZERO, 1e-9).unwrap();
        let b = s.point_at(Vec2::new(10.0, 0.0), 1e-9).unwrap();
        let c = s.point_at(Vec2::new(10.0, 5.0), 1e-9).unwrap();
        let dim = |v| Dimension {
            kind: DimensionKind::Horizontal { a, b },
            value: v,
            offset: -3.0,
            along: 0.0,
            driven: false,
        };
        for v in [50.0, 40.0] {
            SketchOp::SetDimension {
                dimension: dim(v),
                moves: vec![(b, Vec2::new(v, 0.0)), (c, Vec2::new(v, 5.0))],
                radii: vec![],
            }
            .apply(&mut s)
            .unwrap();
        }
        assert_eq!(s.dimensions.len(), 1);
        assert_eq!(s.dimensions.values().next().unwrap().value, 40.0);
        assert_eq!(s.pos(c), Vec2::new(40.0, 5.0));
        s.remove_point(b);
        assert!(s.dimensions.is_empty());
    }

    #[test]
    fn dimensions_toggle_between_driving_and_driven() {
        // S13.7: a driving dimension changed to driven stops holding the geometry; changed
        // back, it drives again from the measured value.
        let mut s = Sketch::new();
        SketchOp::AddPolyline {
            points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        let a = s.point_at(Vec2::ZERO, 1e-9).unwrap();
        let b = s.point_at(Vec2::new(10.0, 0.0), 1e-9).unwrap();
        SketchOp::SetDimension {
            dimension: Dimension {
                kind: DimensionKind::Aligned { a, b },
                value: 30.0,
                offset: 3.0,
                along: 0.0,
                driven: false,
            },
            moves: vec![],
            radii: vec![],
        }
        .apply(&mut s)
        .unwrap();
        assert!((s.pos(a).distance(s.pos(b)) - 30.0).abs() < 1e-6);
        let id = s.dimensions.keys().next().unwrap();
        let driven = SketchOp::SetDimensionDriven { id, driven: true };
        assert_eq!(driven.label(), "Change to driven dimension");
        driven.apply(&mut s).unwrap();
        assert!(s.dimensions[id].driven);
        // Driven: moving the end is no longer held at 30.
        let far = s.pos(a) + (s.pos(b) - s.pos(a)).normalize() * 45.0;
        SketchOp::SetGeometry {
            points: vec![(b, far)],
            radii: vec![],
        }
        .apply(&mut s)
        .unwrap();
        assert!((s.pos(a).distance(s.pos(b)) - 45.0).abs() < 1e-6);
        // A driven dimension cannot be edited.
        assert!(SketchOp::SetDimensionValue { id, value: 5.0 }.apply(&mut s.clone()).is_err());
        // Back to driving: it takes the measured 45 and holds it.
        SketchOp::SetDimensionDriven { id, driven: false }
            .apply(&mut s)
            .unwrap();
        assert!(!s.dimensions[id].driven);
        assert!((s.dimensions[id].value - 45.0).abs() < 1e-6);
    }
}
