//! Sketch diagnostics (P3D.2): what Onshape's Profile inspector and Constraint manager show
//! (`reference/onshape/training/inspection-and-repair.md` IR1, IR2).
//!
//! - [`loose_ends`]: endpoints of regular (non-construction) lines and arcs that join nothing,
//!   so a profile can't close into a region, clustered where they sit close together ("Loose
//!   ends (2)" for a small gap).
//! - [`items`]: every constraint and dimension of a sketch as the Constraint manager lists it:
//!   its type, a numbered name ("Parallel 1", "Equal 1"), the entities it uses, its mode
//!   (internal, external: a Use or Pierce link to geometry outside the sketch; in-context) and
//!   its status (solved, driven, error). Curve ends that share a point are coincident by
//!   construction (no record, see [`crate::constraint`]); they are listed too, as Coincident
//!   rows that can't be deleted from the list.
//! - [`EntityNames`]: "Line 2", "Circle 1", … for the entity rows.

use std::collections::{HashMap, HashSet};

use crate::solve::Source;
use crate::{
    ConstraintId, ConstraintOf, CurveId, CurveKind, CurveRef, DimensionId, DimensionKind, Orient, PointId, PointRef,
    Sketch, Vec2,
};

// ---------------------------------------------------------------------------------------------
// Loose ends

/// One loose end: the point and where it is (sketch mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LooseEnd {
    pub point: PointId,
    pub pos: Vec2,
}

/// Loose ends close together (one row of the Profile inspector).
#[derive(Debug, Clone, PartialEq)]
pub struct LooseEndGroup {
    pub ends: Vec<LooseEnd>,
}

impl LooseEndGroup {
    /// The middle of the group's ends (where the view zooms to).
    pub fn center(&self) -> Vec2 {
        let n = self.ends.len().max(1) as f64;
        let (x, y) = self.ends.iter().fold((0.0, 0.0), |(x, y), e| (x + e.pos.x, y + e.pos.y));
        Vec2::new(x / n, y / n)
    }

    /// The row's label: "Loose end", "Loose ends (2)".
    pub fn label(&self) -> String {
        if self.ends.len() == 1 { "Loose end".into() } else { format!("Loose ends ({})", self.ends.len()) }
    }
}

/// How close (mm) two loose ends must be to share a row: 5 % of the sketch's size, between
/// 0.01 mm and 5 mm. The Conrod's gap of 0.02 in (0.508 mm) in a sketch some 40 mm across
/// makes one "Loose ends (2)" row; ends at opposite sides of a profile stay apart.
pub fn cluster_distance(s: &Sketch) -> f64 {
    let mut pts = s.points.values().map(|p| p.pos);
    let Some(first) = pts.next() else { return 0.01 };
    let (lo, hi) = pts.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    (lo.distance(hi) * 0.05).clamp(0.01, 5.0)
}

/// The loose ends of `s`, grouped (see [`cluster_distance`]), in the order their curves were
/// drawn.
pub fn loose_ends(s: &Sketch) -> Vec<LooseEndGroup> {
    loose_ends_within(s, cluster_distance(s))
}

/// [`loose_ends`] with ends closer than `cluster` (mm) grouped.
pub fn loose_ends_within(s: &Sketch, cluster: f64) -> Vec<LooseEndGroup> {
    let regular = |c: CurveId| s.curves.get(c).is_some_and(|c| !c.construction);
    // How many regular curve ends each point is.
    let mut ends: Vec<PointId> = Vec::new();
    let mut count: HashMap<PointId, usize> = HashMap::new();
    for (id, c) in &s.curves {
        if c.construction {
            continue;
        }
        let Some((a, b)) = s.curve_ends(id) else { continue };
        for p in [a, b] {
            if !count.contains_key(&p) {
                ends.push(p);
            }
            *count.entry(p).or_default() += 1;
        }
        // A curve closed on itself (both ends one point) is joined.
        if a == b {
            *count.entry(a).or_default() += 1;
        }
    }
    // Points held on other geometry: a coincident record to another end, or a point on a curve.
    let mut held: HashSet<PointId> = HashSet::new();
    for c in s.constraints.values() {
        match *c {
            ConstraintOf::Coincident(PointRef::Point(a), PointRef::Point(b)) => {
                if count.contains_key(&b) || s.curves.values().any(|k| !k.construction && crate::curve_points(&k.kind).contains(&b)) {
                    held.insert(a);
                }
                if count.contains_key(&a) || s.curves.values().any(|k| !k.construction && crate::curve_points(&k.kind).contains(&a)) {
                    held.insert(b);
                }
            }
            ConstraintOf::PointOnCurve(PointRef::Point(p), CurveRef::Curve(k)) if regular(k) => {
                held.insert(p);
            }
            _ => {}
        }
    }
    let mut loose: Vec<LooseEnd> = Vec::new();
    for p in ends {
        if count.get(&p).copied().unwrap_or(0) != 1 || held.contains(&p) {
            continue;
        }
        let pos = s.pos(p);
        // On another regular curve (a T-junction), or on an imprinted edge of the face the
        // sketch lies on: joined.
        let own: Vec<CurveId> = s.curves_at(p).collect();
        let on_curve = s.curves.iter().any(|(k, c)| {
            !c.construction && !own.contains(&k) && distance_to_curve(s, k, pos).is_some_and(|d| d < 1e-6)
        });
        let on_imprint = s.imprint.iter().any(|i| distance_to_imprint(&i.shape, pos) < 1e-6);
        if on_curve || on_imprint {
            continue;
        }
        loose.push(LooseEnd { point: p, pos });
    }
    // Single-linkage clusters, in the order found.
    let mut groups: Vec<LooseEndGroup> = Vec::new();
    let mut taken = vec![false; loose.len()];
    for i in 0..loose.len() {
        if taken[i] {
            continue;
        }
        taken[i] = true;
        let mut group = vec![loose[i]];
        let mut k = 0;
        while k < group.len() {
            for j in 0..loose.len() {
                if !taken[j] && group[k].pos.distance(loose[j].pos) <= cluster {
                    taken[j] = true;
                    group.push(loose[j]);
                }
            }
            k += 1;
        }
        groups.push(LooseEndGroup { ends: group });
    }
    groups
}

fn distance_to_curve(s: &Sketch, c: CurveId, p: Vec2) -> Option<f64> {
    Some(match s.curves.get(c)?.kind {
        CurveKind::Line { a, b } => crate::geom::dist_point_segment(p, s.pos(a), s.pos(b)),
        CurveKind::Circle { center, radius } => (s.pos(center).distance(p) - radius).abs(),
        CurveKind::Arc { .. } => s.arc_geom(c)?.distance(p),
        CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => s.ellipse_geom(c)?.distance(p),
        CurveKind::Spline { .. } => crate::spline::nearest(&s.spline_spans(c)?, p)?.2,
        CurveKind::Bezier { .. } => s.bezier_geom(c)?.distance(p),
    })
}

fn distance_to_imprint(shape: &crate::ImprintShape, p: Vec2) -> f64 {
    match *shape {
        crate::ImprintShape::Line(a, b) => crate::geom::dist_point_segment(p, a, b),
        crate::ImprintShape::Circle(c, r) => (c.distance(p) - r).abs(),
        crate::ImprintShape::Arc {
            center,
            radius,
            start_angle,
            sweep,
        } => crate::geom::ArcGeom {
            center,
            radius,
            start_angle,
            sweep,
        }
        .distance(p),
    }
}

// ---------------------------------------------------------------------------------------------
// Entity names

/// Something a constraint row refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityRef {
    Curve(CurveId),
    Point(PointId),
    Origin,
    XAxis,
    YAxis,
}

impl From<PointRef> for EntityRef {
    fn from(p: PointRef) -> Self {
        match p {
            PointRef::Point(k) => EntityRef::Point(k),
            PointRef::Origin => EntityRef::Origin,
        }
    }
}

impl From<CurveRef> for EntityRef {
    fn from(c: CurveRef) -> Self {
        match c {
            CurveRef::Curve(k) => EntityRef::Curve(k),
            CurveRef::XAxis => EntityRef::XAxis,
            CurveRef::YAxis => EntityRef::YAxis,
        }
    }
}

/// The entities' display names: each kind numbered in the order drawn ("Line 1", "Line 2",
/// "Circle 1", "Arc 1", "Ellipse 1", "Point 1").
#[derive(Debug, Clone, Default)]
pub struct EntityNames {
    curves: HashMap<CurveId, String>,
    points: HashMap<PointId, String>,
}

impl EntityNames {
    pub fn new(s: &Sketch) -> Self {
        let mut n: HashMap<&'static str, usize> = HashMap::new();
        let mut next = |kind: &'static str| {
            let c = n.entry(kind).or_default();
            *c += 1;
            format!("{kind} {c}")
        };
        let mut curves = HashMap::new();
        for (id, c) in &s.curves {
            let kind = match c.kind {
                CurveKind::Line { .. } => "Line",
                CurveKind::Circle { .. } => "Circle",
                CurveKind::Arc { .. } => "Arc",
                CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => "Ellipse",
                CurveKind::Spline { .. } => "Spline",
                CurveKind::Bezier { .. } => "Bézier curve",
            };
            curves.insert(id, next(kind));
        }
        let mut points = HashMap::new();
        for id in s.points.keys() {
            points.insert(id, next("Point"));
        }
        Self { curves, points }
    }

    pub fn name(&self, e: EntityRef) -> String {
        match e {
            EntityRef::Curve(c) => self.curves.get(&c).cloned().unwrap_or_else(|| "Curve".into()),
            EntityRef::Point(p) => self.points.get(&p).cloned().unwrap_or_else(|| "Point".into()),
            EntityRef::Origin => "Origin".into(),
            EntityRef::XAxis => "X axis".into(),
            EntityRef::YAxis => "Y axis".into(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Constraint manager items

/// A constraint or dimension type: one toggle of the Constraint manager's Type grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemType {
    Coincident,
    Concentric,
    Parallel,
    Tangent,
    Horizontal,
    Vertical,
    Perpendicular,
    Equal,
    Midpoint,
    Normal,
    Pierce,
    Symmetric,
    Fix,
    Projected,
    Offset,
    Curvature,
    Pattern,
    Distance,
    Angle,
    Radius,
    Diameter,
    Count,
}

impl ItemType {
    /// Every type, in the grid's order.
    pub const ALL: [ItemType; 22] = [
        ItemType::Coincident,
        ItemType::Concentric,
        ItemType::Parallel,
        ItemType::Tangent,
        ItemType::Horizontal,
        ItemType::Vertical,
        ItemType::Perpendicular,
        ItemType::Equal,
        ItemType::Midpoint,
        ItemType::Normal,
        ItemType::Pierce,
        ItemType::Symmetric,
        ItemType::Fix,
        ItemType::Projected,
        ItemType::Offset,
        ItemType::Curvature,
        ItemType::Pattern,
        ItemType::Distance,
        ItemType::Angle,
        ItemType::Radius,
        ItemType::Diameter,
        ItemType::Count,
    ];

    /// Its name as rows number it ("Coincident 3").
    pub fn label(self) -> &'static str {
        match self {
            ItemType::Coincident => "Coincident",
            ItemType::Concentric => "Concentric",
            ItemType::Parallel => "Parallel",
            ItemType::Tangent => "Tangent",
            ItemType::Horizontal => "Horizontal",
            ItemType::Vertical => "Vertical",
            ItemType::Perpendicular => "Perpendicular",
            ItemType::Equal => "Equal",
            ItemType::Midpoint => "Midpoint",
            ItemType::Normal => "Normal",
            ItemType::Pierce => "Pierce",
            ItemType::Symmetric => "Symmetric",
            ItemType::Fix => "Fix",
            ItemType::Projected => "Use",
            ItemType::Offset => "Offset",
            ItemType::Curvature => "Curvature",
            ItemType::Pattern => "Pattern",
            ItemType::Distance => "Distance",
            ItemType::Angle => "Angle",
            ItemType::Radius => "Radius",
            ItemType::Diameter => "Diameter",
            ItemType::Count => "Count",
        }
    }

    /// True for the dimension types.
    pub fn is_dimension(self) -> bool {
        matches!(self, ItemType::Distance | ItemType::Angle | ItemType::Radius | ItemType::Diameter | ItemType::Count)
    }
}

/// Where a constraint's geometry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Only this sketch's own geometry.
    Internal,
    /// Geometry projected from outside the sketch (a Use or Pierce link).
    External,
    /// Geometry from another Part Studio in an assembly's context (cadrs has none yet).
    InContext,
}

/// How a constraint or dimension stands after the solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Solved,
    /// A driven (reference) dimension: measured, not solved.
    Driven,
    /// It could not be satisfied (the solver left it unsolved) or its link is broken.
    Error,
}

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemId {
    Constraint(ConstraintId),
    Dimension(DimensionId),
    /// Curve ends sharing this point (no record; not deletable from the list).
    Shared(PointId),
}

/// One row of the Constraint manager.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: ItemId,
    pub ty: ItemType,
    /// "Parallel 1", "Equal 1", "Distance 2".
    pub name: String,
    pub entities: Vec<EntityRef>,
    pub mode: Mode,
    pub status: Status,
    /// For an external constraint: the feature its linked geometry comes from.
    pub source: Option<uuid::Uuid>,
}

impl Item {
    /// Whether the list may delete it (not a shared end).
    pub fn deletable(&self) -> bool {
        !matches!(self.id, ItemId::Shared(_))
    }
}

/// The type of a stored constraint (`None` for the hidden bookkeeping ones: a rectangle's
/// centre, a polygon's corners, a text box's aspect).
pub fn constraint_type(c: &crate::Constraint) -> Option<ItemType> {
    use ConstraintOf::*;
    Some(match c {
        Coincident(..) | PointOnCurve(..) => ItemType::Coincident,
        Midpoint(..) => ItemType::Midpoint,
        Horizontal(_) => ItemType::Horizontal,
        Vertical(_) => ItemType::Vertical,
        Parallel(..) => ItemType::Parallel,
        Perpendicular(..) => ItemType::Perpendicular,
        Tangent(..) => ItemType::Tangent,
        Equal(..) => ItemType::Equal,
        Normal(..) => ItemType::Normal,
        Concentric(..) => ItemType::Concentric,
        FixPoint(_) | FixCurve(_) => ItemType::Fix,
        SymmetricPoints(..) | SymmetricCurves(..) => ItemType::Symmetric,
        EqualOffset(..) => ItemType::Offset,
        Use(..) => ItemType::Projected,
        Pierce(..) => ItemType::Pierce,
        Curvature(..) => ItemType::Curvature,
        Center(..) | EqualDistance(..) | TextAspect(_) => return None,
    })
}

/// The type of a dimension.
pub fn dimension_type(k: &DimensionKind) -> ItemType {
    match k {
        DimensionKind::Angle { .. } => ItemType::Angle,
        DimensionKind::Radius { .. } | DimensionKind::EllipseRadius { .. } => ItemType::Radius,
        DimensionKind::Diameter { .. } | DimensionKind::Diametral { .. } => ItemType::Diameter,
        DimensionKind::Sides { .. } => ItemType::Count,
        _ => ItemType::Distance,
    }
}

/// Every constraint and dimension of `s` as a Constraint manager row, in the order they were
/// made, each type numbered from 1. `conflicting` is the sketch's conflicting set
/// ([`crate::solve::conflict_set`]: what the solver left unsolved and every constraint or
/// dimension whose removal alone lets it solve); those rows, and Use or Pierce links whose
/// source is gone ([`Sketch::broken`]), are errors.
pub fn items(s: &Sketch, conflicting: &[Source]) -> Vec<Item> {
    let conflict: HashSet<Source> = conflicting.iter().copied().collect();
    // Curves and points projected from outside (Use, Pierce), with their source feature.
    let mut linked_curves: HashMap<CurveId, uuid::Uuid> = HashMap::new();
    let mut linked_points: HashMap<PointId, uuid::Uuid> = HashMap::new();
    for c in s.constraints.values() {
        match *c {
            ConstraintOf::Use(CurveRef::Curve(k), link) => {
                linked_curves.insert(k, link.feature());
                for p in s.curve_points(k) {
                    linked_points.insert(p, link.feature());
                }
            }
            ConstraintOf::Pierce(PointRef::Point(p), link) => {
                linked_points.insert(p, link.feature());
            }
            _ => {}
        }
    }
    let source_of = |entities: &[EntityRef]| -> Option<uuid::Uuid> {
        entities.iter().find_map(|e| match e {
            EntityRef::Curve(k) => linked_curves.get(k).copied(),
            EntityRef::Point(p) => linked_points.get(p).copied(),
            _ => None,
        })
    };
    let mut numbers: HashMap<ItemType, usize> = HashMap::new();
    let mut name = |ty: ItemType| {
        let n = numbers.entry(ty).or_default();
        *n += 1;
        format!("{} {n}", ty.label())
    };
    let mut out = Vec::new();
    // Shared curve ends first: the coincidences the drawing made.
    for p in s.points.keys() {
        let users: Vec<CurveId> = s.curves_at(p).collect();
        if users.len() < 2 {
            continue;
        }
        let entities: Vec<EntityRef> = users.into_iter().map(EntityRef::Curve).collect();
        let source = source_of(&entities);
        out.push(Item {
            id: ItemId::Shared(p),
            ty: ItemType::Coincident,
            name: name(ItemType::Coincident),
            mode: if source.is_some() { Mode::External } else { Mode::Internal },
            source,
            entities,
            status: Status::Solved,
        });
    }
    for (id, c) in &s.constraints {
        if s.quiet.constraints.contains(&id) {
            continue;
        }
        let Some(ty) = constraint_type(c) else { continue };
        let mut entities: Vec<EntityRef> = Vec::new();
        match *c {
            ConstraintOf::Horizontal(Orient::Line(l)) | ConstraintOf::Vertical(Orient::Line(l)) => entities.push(l.into()),
            _ => {
                entities.extend(c.curves().into_iter().map(EntityRef::from));
                entities.extend(c.points().into_iter().map(EntityRef::from));
            }
        }
        let direct = match *c {
            ConstraintOf::Use(_, link) | ConstraintOf::Pierce(_, link) => Some(link.feature()),
            _ => None,
        };
        let source = direct.or_else(|| source_of(&entities));
        let status = if conflict.contains(&Source::Constraint(id)) || s.broken.contains(&id) {
            Status::Error
        } else {
            Status::Solved
        };
        out.push(Item {
            id: ItemId::Constraint(id),
            ty,
            name: name(ty),
            entities,
            mode: if source.is_some() { Mode::External } else { Mode::Internal },
            status,
            source,
        });
    }
    for (id, d) in &s.dimensions {
        let ty = dimension_type(&d.kind);
        let mut entities: Vec<EntityRef> = d.kind.curves().into_iter().map(EntityRef::Curve).collect();
        entities.extend(d.kind.points().into_iter().map(EntityRef::Point));
        let source = source_of(&entities);
        let status = if conflict.contains(&Source::Dimension(id)) {
            Status::Error
        } else if d.driven {
            Status::Driven
        } else {
            Status::Solved
        };
        out.push(Item {
            id: ItemId::Dimension(id),
            ty,
            name: name(ty),
            entities,
            mode: if source.is_some() { Mode::External } else { Mode::Internal },
            status,
            source,
        });
    }
    out
}

/// The Constraint manager's filters: empty sets let everything through.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub types: HashSet<ItemType>,
    pub modes: HashSet<Mode>,
    pub statuses: HashSet<Status>,
    /// Only rows using one of these entities ("Automatically select constraints" off with a
    /// selection); `None`: all.
    pub entities: Option<HashSet<EntityRef>>,
}

impl Filter {
    pub fn matches(&self, item: &Item) -> bool {
        (self.types.is_empty() || self.types.contains(&item.ty))
            && (self.modes.is_empty() || self.modes.contains(&item.mode))
            && (self.statuses.is_empty() || self.statuses.contains(&item.status))
            && self.entities.as_ref().is_none_or(|set| item.entities.iter().any(|e| set.contains(e)))
    }
}

/// The records a "Delete all" of `items` removes: their constraints and dimensions (shared
/// ends are skipped).
pub fn deletion(items: &[&Item]) -> (Vec<ConstraintId>, Vec<DimensionId>) {
    let mut constraints = Vec::new();
    let mut dimensions = Vec::new();
    for i in items {
        match i.id {
            ItemId::Constraint(k) => constraints.push(k),
            ItemId::Dimension(k) => dimensions.push(k),
            ItemId::Shared(_) => {}
        }
    }
    (constraints, dimensions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConstraintSpec, CurveSpec, Dimension, SketchOp};

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    fn polyline(s: &mut Sketch, pts: &[Vec2], closed: bool, construction: bool) {
        SketchOp::AddPolyline {
            points: pts.to_vec(),
            closed,
            construction,
            label: "Add line",
        }
        .apply(s)
        .unwrap();
    }

    /// The Conrod's Sketch 2 in miniature (IR6.7): a notch outline that doesn't close, its last
    /// end 0.02 in (0.508 mm) short of its first, and a spur line off one corner whose far end
    /// is free. Three loose ends: the spur's (alone) and the gap's two (one row).
    fn gapped() -> Sketch {
        let mut s = Sketch::new();
        polyline(
            &mut s,
            &[v(0.0, 0.0), v(30.0, 0.0), v(30.0, 20.0), v(0.0, 20.0), v(0.0, 0.508)],
            false,
            false,
        );
        polyline(&mut s, &[v(30.0, 20.0), v(38.0, 28.0)], false, false);
        s
    }

    #[test]
    fn loose_ends_of_a_gapped_profile_and_a_spur() {
        let s = gapped();
        let groups = loose_ends(&s);
        assert_eq!(groups.len(), 2, "{groups:?}");
        let total: usize = groups.iter().map(|g| g.ends.len()).sum();
        assert_eq!(total, 3);
        let lone = groups.iter().find(|g| g.ends.len() == 1).unwrap();
        assert!(lone.ends[0].pos.distance(v(38.0, 28.0)) < 1e-9);
        assert_eq!(lone.label(), "Loose end");
        let pair = groups.iter().find(|g| g.ends.len() == 2).unwrap();
        assert_eq!(pair.label(), "Loose ends (2)");
        let mut ys: Vec<f64> = pair.ends.iter().map(|e| e.pos.y).collect();
        ys.sort_by(f64::total_cmp);
        assert!((ys[0] - 0.0).abs() < 1e-9 && (ys[1] - 0.508).abs() < 1e-9, "{ys:?}");
        assert!(pair.center().distance(v(0.0, 0.254)) < 1e-9);
    }

    #[test]
    fn closing_the_gap_with_coincident_removes_its_row() {
        // IR1.5 / IR6.8: Coincident merges the two ends into one point.
        let mut s = gapped();
        let a = s.point_at(v(0.0, 0.0), 1e-9).unwrap();
        let b = s.point_at(v(0.0, 0.508), 1e-9).unwrap();
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Coincident(PointRef::Point(a), PointRef::Point(b))],
            label: "Add coincident",
        }
        .apply(&mut s)
        .unwrap();
        let groups = loose_ends(&s);
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].label(), "Loose end");
    }

    #[test]
    fn a_closed_rectangle_has_no_loose_ends() {
        let mut s = Sketch::new();
        polyline(&mut s, &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 25.0), v(0.0, 25.0)], true, false);
        assert!(loose_ends(&s).is_empty());
        // Circles have no ends; a line ending on another line (a T) is joined.
        SketchOp::AddCircle { center: v(20.0, 12.0), radius: 5.0, construction: false }.apply(&mut s).unwrap();
        polyline(&mut s, &[v(20.0, 0.0), v(20.0, -10.0)], false, false);
        let groups = loose_ends(&s);
        assert_eq!(groups.len(), 1, "only the stub's far end: {groups:?}");
        assert!(groups[0].ends[0].pos.distance(v(20.0, -10.0)) < 1e-9);
    }

    #[test]
    fn construction_geometry_is_ignored() {
        let mut s = Sketch::new();
        // An open construction chain: no loose ends.
        polyline(&mut s, &[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)], false, true);
        assert!(loose_ends(&s).is_empty());
        // A regular line ending on a construction line's end is still loose there.
        polyline(&mut s, &[v(10.0, 10.0), v(0.0, 10.0)], false, false);
        let groups = loose_ends(&s);
        let total: usize = groups.iter().map(|g| g.ends.len()).sum();
        assert_eq!(total, 2, "{groups:?}");
    }

    /// Two concentric circles with different diameters made equal (the Conrod's Equal 1,
    /// IR6.4): the Errors filter lists exactly what the solver reports as conflicting.
    #[test]
    fn errors_filter_lists_exactly_the_conflicting_set_and_all_of_it() {
        let mut s = Sketch::new();
        SketchOp::AddCircle { center: v(0.0, 0.0), radius: 15.0, construction: false }.apply(&mut s).unwrap();
        SketchOp::AddCircle { center: v(0.0, 0.0), radius: 10.0, construction: false }.apply(&mut s).unwrap();
        polyline(&mut s, &[v(20.0, 0.0), v(40.0, 0.0), v(40.0, 10.0)], false, false);
        let ids: Vec<CurveId> = s.curves.keys().collect();
        for (c, d) in [(ids[0], 30.0), (ids[1], 20.0)] {
            SketchOp::SetDimension {
                dimension: Dimension::new(DimensionKind::Diameter { curve: c }, d, 0.0),
                moves: vec![],
                radii: vec![],
            }
            .apply(&mut s)
            .unwrap();
        }
        SketchOp::AddConstraints(vec![ConstraintSpec::Horizontal(Orient::Line(CurveSpec::Id(ids[2])))])
            .apply(&mut s)
            .unwrap();
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Equal(CurveRef::Curve(ids[0]), CurveRef::Curve(ids[1]))],
            label: "Add equal",
        }
        .apply(&mut s)
        .unwrap();
        let report = crate::solve::solve(&mut s);
        assert!(!report.conflicting.is_empty(), "the Equal can't hold");
        let set = crate::solve::conflict_set(&s, &report.conflicting);
        let all = items(&s, &set);
        let errors = Filter { statuses: [Status::Error].into(), ..Filter::default() };
        let listed: HashSet<Source> = all
            .iter()
            .filter(|i| errors.matches(i))
            .map(|i| match i.id {
                ItemId::Constraint(k) => Source::Constraint(k),
                ItemId::Dimension(k) => Source::Dimension(k),
                ItemId::Shared(_) => panic!("a shared end is never an error"),
            })
            .collect();
        let expected: HashSet<Source> = set.iter().copied().collect();
        assert_eq!(listed, expected);
        // The whole set, as Onshape reds it: the Equal and both diameters (removing any one of
        // them lets the rest solve), the solver's own pick among them; not the Horizontal.
        assert_eq!(listed.len(), 3, "{listed:?}");
        assert!(report.conflicting.iter().all(|c| listed.contains(c)));
        let names_of = |src: &Source| all.iter().find(|i| match (i.id, *src) {
            (ItemId::Constraint(a), Source::Constraint(b)) => a == b,
            (ItemId::Dimension(a), Source::Dimension(b)) => a == b,
            _ => false,
        }).map(|i| i.name.clone()).unwrap();
        let mut red: Vec<String> = listed.iter().map(names_of).collect();
        red.sort();
        assert_eq!(red, ["Diameter 1", "Diameter 2", "Equal 1"]);
        // The names: one of each, numbered from 1; the shared corner is Coincident 1.
        let names: Vec<&str> = all.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"Equal 1") && names.contains(&"Horizontal 1"), "{names:?}");
        assert!(names.contains(&"Diameter 1") && names.contains(&"Diameter 2"), "{names:?}");
        assert!(names.contains(&"Coincident 1"), "{names:?}");
        let equal = all.iter().find(|i| i.name == "Equal 1").unwrap();
        let en = EntityNames::new(&s);
        let children: Vec<String> = equal.entities.iter().map(|e| en.name(*e)).collect();
        assert_eq!(children, ["Circle 1", "Circle 2"]);
        assert!(all.iter().all(|i| i.mode == Mode::Internal));
        // Delete all of the Errors filter: one op removing exactly those records.
        let filtered: Vec<&Item> = all.iter().filter(|i| errors.matches(i)).collect();
        let (cs, ds) = deletion(&filtered);
        assert_eq!(cs.len() + ds.len(), expected.len());
    }
}
