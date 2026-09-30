//! The Measure tool's geometry (P3E.3, TD6.6, PS2.11), like Onshape's Measure tool: what the
//! selected entities measure, alone or as a pair.
//!
//! - **One entity**: a vertex or point gives its X, Y, Z; an edge its length (a circle or arc
//!   also its radius and diameter); a face its area (a cylinder or sphere also its radius and
//!   diameter).
//! - **Several edges** or **several faces**: the total length or area.
//! - **Two entities**: the distance between them, [`Mode::Minimum`] (the default),
//!   [`Mode::Maximum`] or [`Mode::CenterToCenter`] (offered when one of them has a centre: a
//!   circle, an arc, a cylinder, a sphere), with the two points it runs between (drawn in the
//!   view with its X, Y, Z components), and the angle between them when both have a direction
//!   (a straight edge, a planar face, a cylinder's axis, a circle's plane).
//!
//! Distances are found on the parts' display mesh (a bounding-volume tree over its points,
//! segments and triangles), then refined on the exact surfaces the kernel reports (planes,
//! lines, circles, cylinders, spheres), so a Ø40 cylinder reads 20 from its axis rather than the
//! mesh's chord. A refined point must stay on the entity (within the mesh's sag), so a partial
//! cylinder or arc is never measured past its end.

use crate::solid::{Solid, SolidEdge, SolidFace, add, closest_on_triangle, cross, dot, len, normalize, scale, sub};
use cadrs_kernel::SurfaceKind;
use cadrs_sketch::{PlaneFrame, Vec3};

/// How far the display mesh can stray from the exact surface (mm): the tessellation's
/// deflection (`brep::tessellation`) with a margin.
const MESH_SAG: f64 = 0.06;

/// The largest angle between neighbouring points of a curved edge's polyline (5°).
const ANGLE_STEP: f64 = std::f64::consts::PI / 36.0;

/// Half the size of an unbounded plane's stand-in square (mm).
const UNBOUNDED: f64 = 1.0e5;

/// Which distance two entities measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Minimum,
    Maximum,
    CenterToCenter,
}

/// What an entity is, for the per-entity values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    /// A vertex, a sketch point or the origin.
    Point,
    Edge,
    Face,
    /// A whole part.
    Part,
    /// A default plane or a Plane feature (unbounded).
    Plane,
}

/// The exact geometry an entity lies on, where the kernel reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Exact {
    Point(Vec3),
    /// A straight segment.
    Segment(Vec3, Vec3),
    Circle { center: Vec3, normal: Vec3, radius: f64 },
    Plane { origin: Vec3, normal: Vec3 },
    Cylinder { origin: Vec3, dir: Vec3, radius: f64 },
    Sphere { center: Vec3, radius: f64 },
}

impl Exact {
    /// The point of the (unbounded) geometry nearest `q`.
    fn nearest(&self, q: Vec3) -> Vec3 {
        match *self {
            Exact::Point(p) => p,
            Exact::Segment(a, b) => closest_on_segment(q, a, b),
            Exact::Circle { center, normal, radius } => {
                let n = normalize(normal);
                let off = sub(q, center);
                let inplane = sub(off, scale(n, dot(off, n)));
                if len(inplane) < 1e-12 {
                    return q;
                }
                add(center, scale(normalize(inplane), radius))
            }
            Exact::Plane { origin, normal } => {
                let n = normalize(normal);
                sub(q, scale(n, dot(sub(q, origin), n)))
            }
            Exact::Cylinder { origin, dir, radius } => {
                let d = normalize(dir);
                let foot = add(origin, scale(d, dot(sub(q, origin), d)));
                let r = sub(q, foot);
                if len(r) < 1e-12 {
                    return q;
                }
                add(foot, scale(normalize(r), radius))
            }
            Exact::Sphere { center, radius } => {
                let r = sub(q, center);
                if len(r) < 1e-12 {
                    return q;
                }
                add(center, scale(normalize(r), radius))
            }
        }
    }

    /// The point of the (unbounded) geometry farthest from `q`, where there is one.
    fn farthest(&self, q: Vec3) -> Option<Vec3> {
        match *self {
            Exact::Point(p) => Some(p),
            Exact::Segment(a, b) => Some(if len(sub(q, a)) >= len(sub(q, b)) { a } else { b }),
            Exact::Circle { center, normal, radius } => {
                let n = normalize(normal);
                let off = sub(q, center);
                let inplane = sub(off, scale(n, dot(off, n)));
                (len(inplane) > 1e-12).then(|| sub(center, scale(normalize(inplane), radius)))
            }
            Exact::Sphere { center, radius } => {
                let r = sub(q, center);
                (len(r) > 1e-12).then(|| sub(center, scale(normalize(r), radius)))
            }
            Exact::Plane { .. } | Exact::Cylinder { .. } => None,
        }
    }

    /// How far the mesh of an entity on this geometry can be from it.
    fn sag(&self) -> f64 {
        match *self {
            Exact::Point(_) | Exact::Segment(..) | Exact::Plane { .. } => 1e-6,
            Exact::Circle { radius, .. } | Exact::Cylinder { radius, .. } | Exact::Sphere { radius, .. } => {
                (radius * (1.0 - (ANGLE_STEP / 2.0).cos())).max(MESH_SAG) * 1.5
            }
        }
    }
}

/// Where an entity's centre is, for a centre-to-centre distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Center {
    Point(Vec3),
    /// An axis (a point on it and its direction), unbounded.
    Axis(Vec3, Vec3),
}

/// An entity's direction, for angles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    /// Along a line (a straight edge, an axis).
    Line(Vec3),
    /// Normal to a plane (a planar face, a plane, a circle's plane).
    Normal(Vec3),
}

/// A measured entity: its mesh (points, segments, triangles) and what is known of it exactly.
#[derive(Debug, Clone)]
pub struct Entity {
    pub kind: EntityKind,
    points: Vec<Vec3>,
    segments: Vec<[Vec3; 2]>,
    triangles: Vec<[Vec3; 3]>,
    pub exact: Option<Exact>,
    /// Its exact length (edges).
    pub length: Option<f64>,
    /// Its exact area (faces).
    pub area: Option<f64>,
    /// The radius of a circular edge, a cylinder or a sphere.
    pub radius: Option<f64>,
    pub center: Option<Center>,
    pub direction: Option<Direction>,
}

impl Entity {
    fn empty(kind: EntityKind) -> Self {
        Self {
            kind,
            points: Vec::new(),
            segments: Vec::new(),
            triangles: Vec::new(),
            exact: None,
            length: None,
            area: None,
            radius: None,
            center: None,
            direction: None,
        }
    }

    /// A vertex, a sketch point or the origin.
    pub fn point(p: Vec3) -> Self {
        Self {
            points: vec![p],
            exact: Some(Exact::Point(p)),
            center: Some(Center::Point(p)),
            ..Self::empty(EntityKind::Point)
        }
    }

    /// A polyline (a sketch curve shown in the Part Studio); straight when it has two points.
    pub fn polyline(points: &[Vec3]) -> Self {
        let mut e = Self {
            segments: points.windows(2).map(|w| [w[0], w[1]]).collect(),
            length: Some(points.windows(2).map(|w| len(sub(w[1], w[0]))).sum()),
            ..Self::empty(EntityKind::Edge)
        };
        if let [a, b] = points
            && len(sub(*b, *a)) > 1e-12
        {
            e.exact = Some(Exact::Segment(*a, *b));
            e.direction = Some(Direction::Line(normalize(sub(*b, *a))));
        }
        if points.len() == 1 {
            e.points = points.to_vec();
        }
        e
    }

    /// An edge of a part.
    pub fn edge(edge: &SolidEdge) -> Self {
        Self::curve(&edge.points, edge.circle)
    }

    /// An edge's polyline, on its exact circle if it has one (else straight if it is).
    pub fn curve(pts: &[Vec3], circle: Option<crate::solid::EdgeCircle>) -> Self {
        let mut e = Self::polyline(pts);
        e.exact = None;
        e.direction = None;
        if let Some(c) = circle {
            let normal = normalize(c.normal);
            e.exact = Some(Exact::Circle { center: c.center, normal, radius: c.radius });
            e.radius = Some(c.radius);
            e.center = Some(Center::Point(c.center));
            e.direction = Some(Direction::Normal(normal));
            e.length = Some(arc_length(pts, c.center, normal, c.radius));
        } else if let (Some(a), Some(b)) = (pts.first(), pts.last())
            && len(sub(*b, *a)) > 1e-9
            && pts.iter().all(|p| distance_to_line(*p, *a, *b) < 1e-6)
        {
            e.exact = Some(Exact::Segment(*a, *b));
            e.direction = Some(Direction::Line(normalize(sub(*b, *a))));
            e.length = Some(len(sub(*b, *a)));
        }
        e
    }

    /// A face of a part.
    pub fn face(solid: &Solid, face: &SolidFace) -> Self {
        let triangles = (face.first_triangle..face.first_triangle + face.triangle_count)
            .map(|t| [0, 1, 2].map(|k| solid.positions[solid.indices[3 * t + k] as usize]))
            .collect::<Vec<_>>();
        let area = face.area.or_else(|| Some(triangles.iter().map(|t| len(cross(sub(t[1], t[0]), sub(t[2], t[0]))) / 2.0).sum()));
        let mut e = Self {
            triangles,
            area,
            ..Self::empty(EntityKind::Face)
        };
        if let Some(p) = face.plane {
            let n = normalize(p.normal());
            e.exact = Some(Exact::Plane { origin: p.origin, normal: n });
            e.direction = Some(Direction::Normal(n));
        } else if let Some((origin, dir)) = face.axis {
            let dir = normalize(dir);
            match (face.kind, face.radius) {
                (Some(SurfaceKind::Cylinder), Some(r)) => {
                    e.exact = Some(Exact::Cylinder { origin, dir, radius: r });
                    e.radius = Some(r);
                    e.center = Some(Center::Axis(origin, dir));
                    e.direction = Some(Direction::Line(dir));
                }
                (Some(SurfaceKind::Sphere), Some(r)) => {
                    e.exact = Some(Exact::Sphere { center: origin, radius: r });
                    e.radius = Some(r);
                    e.center = Some(Center::Point(origin));
                }
                (Some(SurfaceKind::Cone), _) => {
                    e.center = Some(Center::Axis(origin, dir));
                    e.direction = Some(Direction::Line(dir));
                }
                _ => {}
            }
        }
        e
    }

    /// A whole part.
    pub fn part(solid: &Solid) -> Self {
        let triangles = (0..solid.triangle_count())
            .map(|t| [0, 1, 2].map(|k| solid.positions[solid.indices[3 * t + k] as usize]))
            .collect();
        Self {
            triangles,
            ..Self::empty(EntityKind::Part)
        }
    }

    /// A plane (a default plane, a Plane feature): unbounded.
    pub fn plane(frame: &PlaneFrame) -> Self {
        let n = normalize(frame.normal());
        let (u, v) = (normalize(frame.u), normalize(frame.v));
        let c = |a: f64, b: f64| add(frame.origin, add(scale(u, a * UNBOUNDED), scale(v, b * UNBOUNDED)));
        let (p00, p10, p11, p01) = (c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0));
        Self {
            triangles: vec![[p00, p10, p11], [p00, p11, p01]],
            exact: Some(Exact::Plane { origin: frame.origin, normal: n }),
            direction: Some(Direction::Normal(n)),
            ..Self::empty(EntityKind::Plane)
        }
    }

    /// An unbounded line (an axis), for centre-to-centre distances.
    fn line(origin: Vec3, dir: Vec3) -> Self {
        let d = normalize(dir);
        let (a, b) = (sub(origin, scale(d, UNBOUNDED)), add(origin, scale(d, UNBOUNDED)));
        Self {
            segments: vec![[a, b]],
            exact: Some(Exact::Segment(a, b)),
            direction: Some(Direction::Line(d)),
            ..Self::empty(EntityKind::Edge)
        }
    }

    fn prims(&self) -> Vec<Prim> {
        let mut out: Vec<Prim> = self.points.iter().map(|p| Prim::P(*p)).collect();
        out.extend(self.segments.iter().map(|s| Prim::S(*s)));
        out.extend(self.triangles.iter().map(|t| Prim::T(*t)));
        out
    }

    fn vertices(&self) -> Vec<Vec3> {
        let mut out = self.points.clone();
        out.extend(self.segments.iter().flatten());
        out.extend(self.triangles.iter().flatten());
        out
    }

    /// True when it has no geometry to measure.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.segments.is_empty() && self.triangles.is_empty()
    }
}

/// A distance and the points it runs between (on the first entity, on the second).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    pub value: f64,
    pub from: Vec3,
    pub to: Vec3,
}

impl Distance {
    fn between(from: Vec3, to: Vec3) -> Self {
        Self { value: len(sub(to, from)), from, to }
    }

    /// Its X, Y and Z components (absolute).
    pub fn components(&self) -> Vec3 {
        let d = sub(self.to, self.from);
        [d[0].abs(), d[1].abs(), d[2].abs()]
    }
}

/// What the selected entities measure (see the module docs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measurement {
    /// The distance between two entities, in the mode asked for (or the minimum, when it is
    /// centre to centre and neither has a centre).
    pub distance: Option<Distance>,
    /// The mode the distance was measured in.
    pub mode: Mode,
    /// Centre to centre is offered (one of two entities has a centre).
    pub has_center: bool,
    /// The angle between two entities (degrees, 0 to 90).
    pub angle: Option<f64>,
    /// A point's coordinates.
    pub point: Option<Vec3>,
    /// An edge's length, or several edges' total.
    pub length: Option<f64>,
    /// A face's area, or several faces' total.
    pub area: Option<f64>,
    /// A circle's, cylinder's or sphere's radius.
    pub radius: Option<f64>,
}

impl Measurement {
    /// True when there is nothing to show.
    pub fn is_empty(&self) -> bool {
        *self == Measurement { mode: self.mode, ..Default::default() }
    }
}

/// Measures the entities (see the module docs).
pub fn measure(entities: &[Entity], mode: Mode) -> Measurement {
    let entities: Vec<&Entity> = entities.iter().filter(|e| !e.is_empty()).collect();
    let mut m = Measurement { mode, ..Default::default() };
    match entities.as_slice() {
        [] => {}
        [e] => {
            match e.kind {
                EntityKind::Point => m.point = e.points.first().copied(),
                EntityKind::Edge => m.length = e.length,
                EntityKind::Face => m.area = e.area,
                EntityKind::Part | EntityKind::Plane => {}
            }
            m.radius = e.radius;
        }
        [a, b] => {
            m.has_center = (a.center.is_some() && a.kind != EntityKind::Point) || (b.center.is_some() && b.kind != EntityKind::Point);
            let mode = if mode == Mode::CenterToCenter && !m.has_center { Mode::Minimum } else { mode };
            m.mode = mode;
            m.distance = match mode {
                Mode::Minimum => min_distance(a, b),
                Mode::Maximum => max_distance(a, b),
                Mode::CenterToCenter => center_distance(a, b),
            };
            m.angle = angle(a, b);
        }
        many => {
            if many.iter().all(|e| e.kind == EntityKind::Edge) {
                m.length = many.iter().map(|e| e.length).sum();
            } else if many.iter().all(|e| e.kind == EntityKind::Face) {
                m.area = many.iter().map(|e| e.area).sum();
            }
        }
    }
    m
}

/// The angle between two entities with directions (degrees, 0 to 90).
pub fn angle(a: &Entity, b: &Entity) -> Option<f64> {
    let (da, db) = (a.direction?, b.direction?);
    let cos = |x: Vec3, y: Vec3| dot(normalize(x), normalize(y)).abs().min(1.0);
    let deg = match (da, db) {
        (Direction::Line(x), Direction::Line(y)) | (Direction::Normal(x), Direction::Normal(y)) => cos(x, y).acos(),
        (Direction::Line(x), Direction::Normal(y)) | (Direction::Normal(y), Direction::Line(x)) => {
            std::f64::consts::FRAC_PI_2 - cos(x, y).acos()
        }
    }
    .to_degrees();
    Some(deg.clamp(0.0, 90.0))
}

/// The shortest distance between two entities.
pub fn min_distance(a: &Entity, b: &Entity) -> Option<Distance> {
    let (ta, tb) = (Bvh::new(a.prims())?, Bvh::new(b.prims())?);
    let mut best = Distance { value: f64::INFINITY, from: [0.0; 3], to: [0.0; 3] };
    ta.nearest_pair(0, &tb, 0, &mut best);
    if !best.value.is_finite() {
        return None;
    }
    Some(refine_min(a, &ta, b, &tb, best))
}

/// The longest distance between two entities (between their farthest points).
pub fn max_distance(a: &Entity, b: &Entity) -> Option<Distance> {
    let pa: Vec<Prim> = a.vertices().into_iter().map(Prim::P).collect();
    let pb: Vec<Prim> = b.vertices().into_iter().map(Prim::P).collect();
    let (ta, tb) = (Bvh::new(pa)?, Bvh::new(pb)?);
    let mut best = Distance { value: -1.0, from: [0.0; 3], to: [0.0; 3] };
    ta.farthest_pair(0, &tb, 0, &mut best);
    if best.value < 0.0 {
        return None;
    }
    // Curved edges and spheres: the farthest point of the exact curve, where it is on the entity.
    let (ma, mb) = (Bvh::new(a.prims())?, Bvh::new(b.prims())?);
    let (mut from, mut to) = (best.from, best.to);
    for _ in 0..16 {
        let (f0, t0) = (from, to);
        if let Some(q) = b.exact.and_then(|x| x.farthest(from)).filter(|q| on_entity(&mb, b, *q) && len(sub(*q, from)) > len(sub(to, from))) {
            to = q;
        }
        if let Some(q) = a.exact.and_then(|x| x.farthest(to)).filter(|q| on_entity(&ma, a, *q) && len(sub(*q, to)) > len(sub(from, to))) {
            from = q;
        }
        if len(sub(from, f0)) < 1e-12 && len(sub(to, t0)) < 1e-12 {
            break;
        }
    }
    Some(Distance::between(from, to))
}

/// The distance between two entities' centres (a circle's or sphere's centre, a cylinder's
/// axis); an entity without one is measured as it is.
pub fn center_distance(a: &Entity, b: &Entity) -> Option<Distance> {
    let of = |e: &Entity| match e.center {
        Some(Center::Point(p)) => Entity::point(p),
        Some(Center::Axis(o, d)) => Entity::line(o, d),
        None => e.clone(),
    };
    let (ca, cb) = (of(a), of(b));
    let d = min_distance(&ca, &cb)?;
    // Parallel axes: between the axes' points nearest the entities' own middles, not wherever
    // along the (unbounded) lines the search stopped.
    match (a.center, b.center) {
        (Some(Center::Axis(oa, da)), Some(Center::Axis(_, db))) if dot(normalize(da), normalize(db)).abs() > 1.0 - 1e-9 => {
            let mid = a.middle().unwrap_or(oa);
            let from = closest_on_line(mid, oa, da);
            let to = cb.exact.map_or(d.to, |x| x.nearest(from));
            Some(Distance::between(from, to))
        }
        (Some(Center::Axis(oa, da)), Some(Center::Point(p))) => Some(Distance::between(closest_on_line(p, oa, da), p)),
        (Some(Center::Point(p)), Some(Center::Axis(ob, db))) => Some(Distance::between(p, closest_on_line(p, ob, db))),
        _ => Some(d),
    }
}

impl Entity {
    /// The middle of its mesh's bounding box.
    fn middle(&self) -> Option<Vec3> {
        let v = self.vertices();
        let first = *v.first()?;
        let (lo, hi) = v.iter().fold((first, first), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        });
        Some(scale(add(lo, hi), 0.5))
    }
}

/// True if `q` lies on the entity: within its exact geometry's sag of its mesh.
fn on_entity(tree: &Bvh, e: &Entity, q: Vec3) -> bool {
    let tol = e.exact.map_or(1e-6, |x| x.sag());
    tree.distance_to(q) <= tol
}

/// The mesh's closest points moved onto the exact geometry, alternating between the two
/// entities, while they stay on them.
fn refine_min(a: &Entity, ta: &Bvh, b: &Entity, tb: &Bvh, mesh: Distance) -> Distance {
    if a.exact.is_none() && b.exact.is_none() {
        return mesh;
    }
    let (mut from, mut to) = (mesh.from, mesh.to);
    for _ in 0..64 {
        let (f0, t0) = (from, to);
        if let Some(q) = b.exact.map(|x| x.nearest(from)).filter(|q| on_entity(tb, b, *q)) {
            to = q;
        }
        if let Some(q) = a.exact.map(|x| x.nearest(to)).filter(|q| on_entity(ta, a, *q)) {
            from = q;
        }
        if len(sub(from, f0)) < 1e-12 && len(sub(to, t0)) < 1e-12 {
            break;
        }
    }
    let refined = Distance::between(from, to);
    // The exact surface is within the sag of the mesh: a larger change is not a refinement.
    let tol = a.exact.map_or(0.0, |x| x.sag()) + b.exact.map_or(0.0, |x| x.sag()) + 1e-9;
    if (refined.value - mesh.value).abs() <= tol { refined } else { mesh }
}

/// The length of a circular edge's polyline measured on its circle: the angle it sweeps times
/// the radius (a whole circle: 2πr).
fn arc_length(pts: &[Vec3], center: Vec3, normal: Vec3, radius: f64) -> f64 {
    let n = normalize(normal);
    let sweep: f64 = pts
        .windows(2)
        .map(|w| {
            let (u, v) = (sub(w[0], center), sub(w[1], center));
            let s = dot(cross(u, v), n);
            s.atan2(dot(u, v)).abs()
        })
        .sum();
    radius * sweep
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let d = sub(b, a);
    let t = (dot(sub(p, a), d) / dot(d, d).max(1e-300)).clamp(0.0, 1.0);
    add(a, scale(d, t))
}

fn closest_on_line(p: Vec3, o: Vec3, dir: Vec3) -> Vec3 {
    let d = normalize(dir);
    add(o, scale(d, dot(sub(p, o), d)))
}

fn distance_to_line(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    len(sub(p, closest_on_line(p, a, sub(b, a))))
}

/// The closest points of segments `p1 q1` and `p2 q2` (Ericson, Real-Time Collision Detection
/// 5.1.9).
fn segment_segment(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    const EPS: f64 = 1e-18;
    let (s, t);
    if a <= EPS && e <= EPS {
        return (p1, p2);
    }
    if a <= EPS {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= EPS {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > EPS { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    (add(p1, scale(d1, s)), add(p2, scale(d2, t)))
}

/// Where segment `p q` crosses triangle `abc`, if it does.
fn segment_triangle_hit(p: Vec3, q: Vec3, [a, b, c]: [Vec3; 3]) -> Option<Vec3> {
    let n = cross(sub(b, a), sub(c, a));
    let (dp, dq) = (dot(sub(p, a), n), dot(sub(q, a), n));
    if dp * dq > 0.0 || (dp - dq).abs() < 1e-300 {
        return None;
    }
    let x = add(p, scale(sub(q, p), dp / (dp - dq)));
    let nn = dot(n, n);
    [(a, b), (b, c), (c, a)]
        .iter()
        .all(|(u, v)| dot(cross(sub(*v, *u), sub(x, *u)), n) >= -1e-12 * nn)
        .then_some(x)
}

/// A mesh primitive.
#[derive(Debug, Clone, Copy)]
enum Prim {
    P(Vec3),
    S([Vec3; 2]),
    T([Vec3; 3]),
}

impl Prim {
    fn bounds(&self) -> Aabb {
        match self {
            Prim::P(p) => Aabb::of(&[*p]),
            Prim::S(s) => Aabb::of(s),
            Prim::T(t) => Aabb::of(t),
        }
    }

    /// The closest points of two primitives (on `self`, on `other`).
    fn closest(&self, other: &Prim) -> (Vec3, Vec3) {
        use Prim::*;
        match (*self, *other) {
            (P(p), P(q)) => (p, q),
            (P(p), S([a, b])) => (p, closest_on_segment(p, a, b)),
            (S(_), P(_)) | (T(_), P(_)) | (T(_), S(_)) => {
                let (q, p) = other.closest(self);
                (p, q)
            }
            (P(p), T([a, b, c])) => (p, closest_on_triangle(p, a, b, c)),
            (S([p1, q1]), S([p2, q2])) => segment_segment(p1, q1, p2, q2),
            (S([p, q]), T(t)) => segment_triangle(p, q, t),
            (T(t1), T(t2)) => {
                let mut best = (t1[0], t2[0]);
                let mut d = f64::INFINITY;
                for k in 0..3 {
                    let (x, y) = segment_triangle(t1[k], t1[(k + 1) % 3], t2);
                    let l = len(sub(y, x));
                    if l < d {
                        d = l;
                        best = (x, y);
                    }
                    let (y, x) = segment_triangle(t2[k], t2[(k + 1) % 3], t1);
                    let l = len(sub(y, x));
                    if l < d {
                        d = l;
                        best = (x, y);
                    }
                }
                best
            }
        }
    }
}

/// The closest points of segment `p q` and triangle `t` (on the segment, on the triangle).
fn segment_triangle(p: Vec3, q: Vec3, t: [Vec3; 3]) -> (Vec3, Vec3) {
    if let Some(x) = segment_triangle_hit(p, q, t) {
        return (x, x);
    }
    let mut cands: Vec<(Vec3, Vec3)> = vec![(p, closest_on_triangle(p, t[0], t[1], t[2])), (q, closest_on_triangle(q, t[0], t[1], t[2]))];
    for k in 0..3 {
        cands.push(segment_segment(p, q, t[k], t[(k + 1) % 3]));
    }
    cands.into_iter().min_by(|x, y| len(sub(x.1, x.0)).total_cmp(&len(sub(y.1, y.0)))).unwrap()
}

#[derive(Debug, Clone, Copy)]
struct Aabb {
    lo: Vec3,
    hi: Vec3,
}

impl Aabb {
    fn of(pts: &[Vec3]) -> Self {
        let mut b = Aabb { lo: [f64::INFINITY; 3], hi: [f64::NEG_INFINITY; 3] };
        for p in pts {
            b = b.with(*p);
        }
        b
    }

    fn with(self, p: Vec3) -> Self {
        Aabb {
            lo: [self.lo[0].min(p[0]), self.lo[1].min(p[1]), self.lo[2].min(p[2])],
            hi: [self.hi[0].max(p[0]), self.hi[1].max(p[1]), self.hi[2].max(p[2])],
        }
    }

    fn union(self, o: Aabb) -> Self {
        self.with(o.lo).with(o.hi)
    }

    /// The smallest distance between points of the two boxes.
    fn min_distance(&self, o: &Aabb) -> f64 {
        let d: Vec3 = std::array::from_fn(|k| (o.lo[k] - self.hi[k]).max(self.lo[k] - o.hi[k]).max(0.0));
        len(d)
    }

    /// The largest distance between points of the two boxes.
    fn max_distance(&self, o: &Aabb) -> f64 {
        let d: Vec3 = std::array::from_fn(|k| (o.hi[k] - self.lo[k]).abs().max((self.hi[k] - o.lo[k]).abs()));
        len(d)
    }

    fn center(&self) -> Vec3 {
        scale(add(self.lo, self.hi), 0.5)
    }
}

/// A bounding-volume tree over primitives: node 0 is the root; a leaf holds a few primitives.
struct Bvh {
    prims: Vec<Prim>,
    nodes: Vec<Node>,
}

struct Node {
    bounds: Aabb,
    /// Children (inner node) or a range of `prims` (leaf).
    kind: NodeKind,
}

enum NodeKind {
    Inner(usize, usize),
    Leaf(usize, usize),
}

const LEAF: usize = 4;

impl Bvh {
    fn new(mut prims: Vec<Prim>) -> Option<Self> {
        if prims.is_empty() {
            return None;
        }
        let mut nodes = Vec::with_capacity(2 * prims.len() / LEAF + 1);
        let mut bounds: Vec<Aabb> = prims.iter().map(Prim::bounds).collect();
        build(&mut prims, &mut bounds, 0, &mut nodes);
        Some(Self { prims, nodes })
    }

    /// Updates `best` with the closest pair between node `i` of this tree and node `j` of `o`.
    fn nearest_pair(&self, i: usize, o: &Bvh, j: usize, best: &mut Distance) {
        let (a, b) = (&self.nodes[i], &o.nodes[j]);
        if a.bounds.min_distance(&b.bounds) >= best.value {
            return;
        }
        match (&a.kind, &b.kind) {
            (NodeKind::Leaf(s, e), NodeKind::Leaf(s2, e2)) => {
                for pa in &self.prims[*s..*e] {
                    for pb in &o.prims[*s2..*e2] {
                        let (x, y) = pa.closest(pb);
                        let d = len(sub(y, x));
                        if d < best.value {
                            *best = Distance { value: d, from: x, to: y };
                        }
                    }
                }
            }
            (NodeKind::Inner(l, r), _) if matches!(b.kind, NodeKind::Leaf(..)) || volume(&a.bounds) >= volume(&b.bounds) => {
                let (l, r) = (*l, *r);
                let (dl, dr) = (self.nodes[l].bounds.min_distance(&b.bounds), self.nodes[r].bounds.min_distance(&b.bounds));
                let order = if dl <= dr { [l, r] } else { [r, l] };
                for c in order {
                    self.nearest_pair(c, o, j, best);
                }
            }
            (_, NodeKind::Inner(l, r)) => {
                let (l, r) = (*l, *r);
                let (dl, dr) = (a.bounds.min_distance(&o.nodes[l].bounds), a.bounds.min_distance(&o.nodes[r].bounds));
                let order = if dl <= dr { [l, r] } else { [r, l] };
                for c in order {
                    self.nearest_pair(i, o, c, best);
                }
            }
            (NodeKind::Inner(..), NodeKind::Leaf(..)) => unreachable!(),
        }
    }

    /// Updates `best` with the farthest pair of points between node `i` and node `j` of `o`
    /// (trees of points).
    fn farthest_pair(&self, i: usize, o: &Bvh, j: usize, best: &mut Distance) {
        let (a, b) = (&self.nodes[i], &o.nodes[j]);
        if a.bounds.max_distance(&b.bounds) <= best.value {
            return;
        }
        match (&a.kind, &b.kind) {
            (NodeKind::Leaf(s, e), NodeKind::Leaf(s2, e2)) => {
                for pa in &self.prims[*s..*e] {
                    for pb in &o.prims[*s2..*e2] {
                        let (x, y) = pa.closest(pb);
                        let d = len(sub(y, x));
                        if d > best.value {
                            *best = Distance { value: d, from: x, to: y };
                        }
                    }
                }
            }
            (NodeKind::Inner(l, r), _) if matches!(b.kind, NodeKind::Leaf(..)) || volume(&a.bounds) >= volume(&b.bounds) => {
                let (l, r) = (*l, *r);
                let (dl, dr) = (self.nodes[l].bounds.max_distance(&b.bounds), self.nodes[r].bounds.max_distance(&b.bounds));
                let order = if dl >= dr { [l, r] } else { [r, l] };
                for c in order {
                    self.farthest_pair(c, o, j, best);
                }
            }
            (_, NodeKind::Inner(l, r)) => {
                let (l, r) = (*l, *r);
                let (dl, dr) = (a.bounds.max_distance(&o.nodes[l].bounds), a.bounds.max_distance(&o.nodes[r].bounds));
                let order = if dl >= dr { [l, r] } else { [r, l] };
                for c in order {
                    self.farthest_pair(i, o, c, best);
                }
            }
            (NodeKind::Inner(..), NodeKind::Leaf(..)) => unreachable!(),
        }
    }

    /// The distance from `q` to the nearest primitive.
    fn distance_to(&self, q: Vec3) -> f64 {
        let point = Bvh { prims: vec![Prim::P(q)], nodes: vec![Node { bounds: Aabb::of(&[q]), kind: NodeKind::Leaf(0, 1) }] };
        let mut best = Distance { value: f64::INFINITY, from: q, to: q };
        point.nearest_pair(0, self, 0, &mut best);
        best.value
    }
}

/// A box's size, for choosing which node of a pair to split.
fn volume(b: &Aabb) -> f64 {
    let d = sub(b.hi, b.lo);
    d[0] + d[1] + d[2]
}

/// Builds the subtree over `prims` (whose first primitive is `offset` in the tree's list) and
/// returns its node index.
fn build(prims: &mut [Prim], bounds: &mut [Aabb], offset: usize, nodes: &mut Vec<Node>) -> usize {
    let all = bounds.iter().skip(1).fold(bounds[0], |b, x| b.union(*x));
    let index = nodes.len();
    nodes.push(Node { bounds: all, kind: NodeKind::Leaf(offset, offset + prims.len()) });
    if prims.len() <= LEAF {
        return index;
    }
    // Split at the median of the boxes' centres along the widest axis.
    let d = sub(all.hi, all.lo);
    let axis = if d[0] >= d[1] && d[0] >= d[2] { 0 } else if d[1] >= d[2] { 1 } else { 2 };
    let mut order: Vec<usize> = (0..prims.len()).collect();
    order.sort_by(|&x, &y| bounds[x].center()[axis].total_cmp(&bounds[y].center()[axis]));
    let (p2, b2): (Vec<Prim>, Vec<Aabb>) = order.iter().map(|&k| (prims[k], bounds[k])).unzip();
    prims.copy_from_slice(&p2);
    bounds.copy_from_slice(&b2);
    let mid = prims.len() / 2;
    let (pl, pr) = prims.split_at_mut(mid);
    let (bl, br) = bounds.split_at_mut(mid);
    let l = build(pl, bl, offset, nodes);
    let r = build(pr, br, offset + mid, nodes);
    nodes[index].kind = NodeKind::Inner(l, r);
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
    }

    fn quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> Entity {
        Entity {
            triangles: vec![[a, b, c], [a, c, d]],
            area: Some(len(cross(sub(b, a), sub(d, a)))),
            exact: Some(Exact::Plane { origin: a, normal: normalize(cross(sub(b, a), sub(d, a))) }),
            direction: Some(Direction::Normal(normalize(cross(sub(b, a), sub(d, a))))),
            ..Entity::empty(EntityKind::Face)
        }
    }

    /// A cylinder's side (radius `r`, axis Z from z0 to z1) meshed with `n` facets.
    fn cylinder(center: Vec3, r: f64, z0: f64, z1: f64, n: usize) -> Entity {
        let p = |k: usize, z: f64| {
            let t = std::f64::consts::TAU * k as f64 / n as f64;
            [center[0] + r * t.cos(), center[1] + r * t.sin(), z]
        };
        let triangles = (0..n).flat_map(|k| [[p(k, z0), p(k + 1, z0), p(k + 1, z1)], [p(k, z0), p(k + 1, z1), p(k, z1)]]).collect();
        let axis = [center[0], center[1], z0];
        Entity {
            triangles,
            exact: Some(Exact::Cylinder { origin: axis, dir: [0.0, 0.0, 1.0], radius: r }),
            radius: Some(r),
            center: Some(Center::Axis(axis, [0.0, 0.0, 1.0])),
            direction: Some(Direction::Line([0.0, 0.0, 1.0])),
            ..Entity::empty(EntityKind::Face)
        }
    }

    fn circle(center: Vec3, r: f64, n: usize) -> Entity {
        let pts: Vec<Vec3> = (0..=n)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / n as f64;
                [center[0] + r * t.cos(), center[1] + r * t.sin(), center[2]]
            })
            .collect();
        Entity::curve(&pts, Some(crate::solid::EdgeCircle { center, normal: [0.0, 0.0, 1.0], radius: r }))
    }

    fn line(a: Vec3, b: Vec3) -> Entity {
        Entity::curve(&[a, b], None)
    }

    #[test]
    fn box_edges_faces_and_angles() {
        // A 100 × 60 × 25 box's edges and faces.
        let x = line([0.0; 3], [100.0, 0.0, 0.0]);
        let y = line([0.0; 3], [0.0, 60.0, 0.0]);
        let z = line([0.0; 3], [0.0, 0.0, 25.0]);
        for (e, l) in [(&x, 100.0), (&y, 60.0), (&z, 25.0)] {
            close(measure(std::slice::from_ref(e), Mode::Minimum).length.unwrap(), l, 1e-12);
        }
        let top = quad([0.0, 0.0, 25.0], [100.0, 0.0, 25.0], [100.0, 60.0, 25.0], [0.0, 60.0, 25.0]);
        let bottom = quad([0.0, 0.0, 0.0], [0.0, 60.0, 0.0], [100.0, 60.0, 0.0], [100.0, 0.0, 0.0]);
        let front = quad([0.0; 3], [100.0, 0.0, 0.0], [100.0, 0.0, 25.0], [0.0, 0.0, 25.0]);
        close(measure(std::slice::from_ref(&top), Mode::Minimum).area.unwrap(), 6000.0, 1e-9);
        let m = measure(&[top.clone(), bottom.clone()], Mode::Minimum);
        close(m.distance.unwrap().value, 25.0, 1e-12);
        close(m.distance.unwrap().components()[2], 25.0, 1e-12);
        close(m.angle.unwrap(), 0.0, 1e-9);
        let m = measure(&[top.clone(), front], Mode::Minimum);
        close(m.distance.unwrap().value, 0.0, 1e-12);
        close(m.angle.unwrap(), 90.0, 1e-9);
        // Opposite corners: the maximum distance between the top and bottom faces.
        let m = measure(&[top, bottom], Mode::Maximum);
        close(m.distance.unwrap().value, (100f64.powi(2) + 60f64.powi(2) + 25f64.powi(2)).sqrt(), 1e-9);
        // Two edges meeting at a corner: 90°, 0 apart; a vertex's coordinates.
        let m = measure(&[x, z], Mode::Minimum);
        close(m.angle.unwrap(), 90.0, 1e-9);
        close(m.distance.unwrap().value, 0.0, 1e-12);
        assert_eq!(measure(&[Entity::point([1.0, 2.0, 3.0])], Mode::Minimum).point, Some([1.0, 2.0, 3.0]));
    }

    #[test]
    fn two_boxes_15_apart() {
        // Facing faces of two boxes 15 apart, offset sideways: the minimum is 15 along X.
        let a = quad([10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [10.0, 10.0, 10.0], [10.0, 0.0, 10.0]);
        let b = quad([25.0, 3.0, 2.0], [25.0, 8.0, 2.0], [25.0, 8.0, 7.0], [25.0, 3.0, 7.0]);
        let d = min_distance(&a, &b).unwrap();
        close(d.value, 15.0, 1e-12);
        close(d.components()[0], 15.0, 1e-12);
        close(d.components()[1], 0.0, 1e-12);
    }

    #[test]
    fn cylinder_radius_and_distances_are_exact() {
        // A Ø40 cylinder meshed coarsely (every 10°): its radius, a point's distance from it, and
        // centre to centre between two holes 50 apart.
        let c = cylinder([0.0; 3], 20.0, 0.0, 30.0, 36);
        let m = measure(std::slice::from_ref(&c), Mode::Minimum);
        close(m.radius.unwrap(), 20.0, 1e-12);
        let p = Entity::point([35.0, 7.0, 10.0]);
        let d = min_distance(&p, &c).unwrap();
        close(d.value, (35f64.powi(2) + 7f64.powi(2)).sqrt() - 20.0, 1e-9);
        let c2 = cylinder([50.0, 0.0, 0.0], 5.0, 0.0, 30.0, 36);
        let m = measure(&[c.clone(), c2.clone()], Mode::CenterToCenter);
        assert!(m.has_center);
        close(m.distance.unwrap().value, 50.0, 1e-9);
        // The minimum between them: 50 − 20 − 5, on the exact surfaces.
        close(min_distance(&c, &c2).unwrap().value, 25.0, 1e-9);
    }

    #[test]
    fn circles_measure_on_the_exact_curve() {
        let c = circle([0.0, 0.0, 0.0], 20.0, 72);
        let m = measure(std::slice::from_ref(&c), Mode::Minimum);
        close(m.length.unwrap(), std::f64::consts::TAU * 20.0, 1e-9);
        close(m.radius.unwrap(), 20.0, 1e-12);
        // A point off the circle's plane, beside it: exact, not the polyline's chord.
        let p = Entity::point([0.0, 33.0, 4.0]);
        close(min_distance(&c, &p).unwrap().value, (13f64.powi(2) + 16.0).sqrt(), 1e-9);
        close(max_distance(&c, &p).unwrap().value, (53f64.powi(2) + 16.0).sqrt(), 1e-9);
        // Centre to centre of two circles.
        let c2 = circle([30.0, 40.0, 0.0], 5.0, 72);
        close(measure(&[c, c2], Mode::CenterToCenter).distance.unwrap().value, 50.0, 1e-12);
    }

    #[test]
    fn several_edges_or_faces_total() {
        let a = line([0.0; 3], [3.0, 4.0, 0.0]);
        let b = line([0.0; 3], [0.0, 0.0, 10.0]);
        let c = line([1.0; 3], [1.0, 1.0, 3.0]);
        close(measure(&[a, b, c], Mode::Minimum).length.unwrap(), 17.0, 1e-12);
    }

    #[test]
    fn a_plane_is_unbounded() {
        let top = Entity::plane(&PlaneFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] });
        let p = Entity::point([5000.0, -3000.0, 12.5]);
        close(min_distance(&top, &p).unwrap().value, 12.5, 1e-9);
    }
}
