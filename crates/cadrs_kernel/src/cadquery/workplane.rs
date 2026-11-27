//! CadQuery's `Workplane` (`cadquery/cq.py`): a stack of objects on a plane, each call making a
//! new workplane whose parent is the one it was called on; pending edges and wires (what the 2D
//! calls draw) are shared along the chain in its context.
//!
//! Calls that take Python callables (`each`, `eachpoint` with a function, `filter`, `map`,
//! `parametricCurve`, …) live in the Python binding, on [`Workplane::new_object`] and the
//! pending-wire helpers here.

use super::geom::{Location, Plane};
use super::make;
use super::ops;
use super::select::Selector;
use super::shape::{EdgeGeom, Shape, ShapeType};
use super::{CqError, CqResult, value_err};
use crate::native::math::V3;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// An object on a workplane's stack.
#[derive(Debug, Clone)]
pub enum Obj {
    /// A point (`Vector`).
    Vector(V3),
    /// A placement (`Location`).
    Location(Location),
    /// A shape.
    Shape(Shape),
}

impl Obj {
    /// The shape, if it is one.
    pub fn shape(&self) -> Option<&Shape> {
        match self {
            Obj::Shape(s) => Some(s),
            _ => None,
        }
    }

    fn center(&self) -> Option<V3> {
        match self {
            Obj::Vector(v) => Some(*v),
            Obj::Shape(s) => Some(s.center()),
            Obj::Location(_) => None,
        }
    }
}

/// How a new solid joins the stack's (`combine`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// Kept apart (`False`).
    No,
    /// United with it (`True`, `'a'`).
    Fuse,
    /// Cut from it (`'cut'`, `'s'`).
    Cut,
}

/// What an extrude goes up to.
#[derive(Debug, Clone)]
pub enum Until {
    /// A distance along the plane's normal.
    Distance(f64),
    /// The next face of the solid (`'next'`).
    Next,
    /// The last (`'last'`).
    Last,
    /// A face.
    Face(Shape),
}

/// The state shared along a chain.
#[derive(Debug, Default)]
pub struct Ctx {
    /// Edges drawn, not yet made into a wire.
    pub pending_edges: Vec<Shape>,
    /// Wires drawn, not yet used.
    pub pending_wires: Vec<Shape>,
    /// Where the edges being drawn started (plane coordinates).
    pub first_point: Option<V3>,
    /// The tagged workplanes.
    pub tags: HashMap<String, Workplane>,
}

/// A workplane (`cadquery.Workplane`).
#[derive(Debug, Clone)]
pub struct Workplane {
    /// The stack.
    pub objects: Vec<Obj>,
    /// The plane.
    pub plane: Plane,
    /// The workplane this one was made from.
    pub parent: Option<Arc<Workplane>>,
    /// The chain's shared state.
    pub ctx: Arc<Mutex<Ctx>>,
    /// Its tag.
    pub tag: Option<String>,
}

/// CadQuery's context tolerance.
const CTX_TOL: f64 = 1e-4;

impl Workplane {
    /// `Workplane(plane)`.
    pub fn new(plane: Plane) -> Self {
        Self {
            objects: vec![],
            plane,
            parent: None,
            ctx: Arc::new(Mutex::new(Ctx::default())),
            tag: None,
        }
    }

    /// `Workplane(name, origin)`.
    pub fn named(name: &str, origin: V3) -> CqResult<Self> {
        Ok(Self::new(Plane::named(name, origin)?))
    }

    /// The shared context.
    pub fn ctx(&self) -> MutexGuard<'_, Ctx> {
        self.ctx.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `newObject(objs)`.
    pub fn new_object(&self, objects: Vec<Obj>) -> Workplane {
        Workplane {
            objects,
            plane: self.plane,
            parent: Some(Arc::new(self.clone())),
            ctx: self.ctx.clone(),
            tag: None,
        }
    }

    fn new_shapes(&self, v: Vec<Shape>) -> Workplane {
        self.new_object(v.into_iter().map(Obj::Shape).collect())
    }

    /// `tag(name)`.
    pub fn tagged(&self, name: &str) -> Workplane {
        let mut w = self.clone();
        w.tag = Some(name.to_string());
        self.ctx().tags.insert(name.to_string(), w.clone());
        w
    }

    fn get_tagged(&self, name: &str) -> CqResult<Workplane> {
        self.ctx()
            .tags
            .get(name)
            .cloned()
            .ok_or_else(|| CqError::Value(format!("No Workplane object named {name} in chain")))
    }

    /// `_mergeTags(other)`.
    pub fn merge_tags(&self, o: &Workplane) {
        if Arc::ptr_eq(&self.ctx, &o.ctx) {
            return;
        }
        let theirs: Vec<(String, Workplane)> = o
            .ctx()
            .tags
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut c = self.ctx();
        for (k, v) in theirs {
            c.tags.entry(k).or_insert(v);
        }
    }

    /// `val()`: the first object, or the plane's origin.
    pub fn val(&self) -> Obj {
        self.objects
            .first()
            .cloned()
            .unwrap_or(Obj::Vector(self.plane.origin))
    }

    /// The shapes on the stack.
    pub fn shapes(&self) -> Vec<Shape> {
        self.objects
            .iter()
            .filter_map(|o| o.shape().cloned())
            .collect()
    }

    /// `first()`.
    pub fn first(&self) -> Workplane {
        self.new_object(self.objects.iter().take(1).cloned().collect())
    }

    /// `item(i)` (negative from the end).
    pub fn item(&self, i: i64) -> CqResult<Workplane> {
        let n = self.objects.len() as i64;
        let j = if i < 0 { n + i } else { i };
        if j < 0 || j >= n {
            return Err(CqError::Index("list index out of range".into()));
        }
        Ok(self.new_object(vec![self.objects[j as usize].clone()]))
    }

    /// `last()`.
    pub fn last(&self) -> CqResult<Workplane> {
        self.item(-1)
    }

    /// `end(n)`.
    pub fn end(&self, n: usize) -> CqResult<Workplane> {
        let mut rv = self.clone();
        for _ in 0..n {
            rv = match &rv.parent {
                Some(p) => p.as_ref().clone(),
                None => return value_err!("Cannot End the chain-- no parents!"),
            };
        }
        Ok(rv)
    }

    /// `_findType((Solid,))`: the solids on the stack (a compound of them), or the parents'.
    pub fn find_solid_opt(&self, stack: bool, parents: bool) -> Option<Shape> {
        if stack {
            let mut rv: Vec<Shape> = Vec::new();
            for o in &self.objects {
                if let Obj::Shape(s) = o {
                    match s.shape_type() {
                        ShapeType::Solid => rv.push(s.clone()),
                        ShapeType::Compound => rv.extend(s.solids()),
                        _ => {}
                    }
                }
            }
            if rv.len() == 1 {
                return rv.pop();
            }
            if !rv.is_empty() {
                // The stack's own shapes when they are the solids (a compound of them).
                return Some(Shape::compound(rv));
            }
        }
        if parents && let Some(p) = &self.parent {
            return p.find_solid_opt(true, true);
        }
        None
    }

    /// `findSolid()`.
    pub fn find_solid(&self) -> CqResult<Shape> {
        self.find_solid_opt(true, true).ok_or_else(|| {
            CqError::Value("Cannot find a solid on the stack or in the parent chain".into())
        })
    }

    /// `_selectObjects(kind, selector, tag)`.
    pub fn select(
        &self,
        kind: ShapeType,
        sel: Option<&Selector>,
        tag: Option<&str>,
    ) -> CqResult<Workplane> {
        let src = match tag {
            Some(t) => self.get_tagged(t)?,
            None => self.clone(),
        };
        let mut seen = std::collections::HashSet::new();
        let mut all = Vec::new();
        for o in &src.objects {
            if let Obj::Shape(s) = o {
                for e in s.entities(kind) {
                    if seen.insert(e.key()) {
                        all.push(e);
                    }
                }
            }
        }
        let out = match sel {
            Some(s) => s.filter(&all)?,
            None => all,
        };
        Ok(self.new_shapes(out))
    }

    /// `workplane(offset, invert, centerOption, origin)`.
    pub fn workplane(
        &self,
        offset: f64,
        invert: bool,
        center_option: &str,
        origin: Option<V3>,
    ) -> CqResult<Workplane> {
        if !["CenterOfMass", "ProjectedOrigin", "CenterOfBoundBox"].contains(&center_option) {
            return value_err!("Undefined centerOption value provided.");
        }
        let xdir_of = |n: &V3| {
            let xd = V3::z().cross(n);
            if xd.norm() < CTX_TOL { V3::x() } else { xd }
        };
        let (mut center, mut normal, xdir);
        if self.objects.len() > 1 {
            let faces: Vec<&Shape> = self
                .objects
                .iter()
                .filter_map(|o| o.shape())
                .filter(|s| s.shape_type() == ShapeType::Face)
                .collect();
            if faces.len() < self.objects.len()
                || !faces
                    .iter()
                    .all(|f| matches!(f.geom_type().name(), "PLANE" | "CIRCLE"))
            {
                return value_err!("If multiple objects selected, they all must be planar faces.");
            }
            let n0 = faces[0].normal_at(None)?;
            let p0 = faces[0].center();
            for f in &faces[1..] {
                let n1 = f.normal_at(None)?;
                let p1 = f.center();
                let par = (n0.x - n1.x).abs() < CTX_TOL
                    || (n0.y - n1.y).abs() < CTX_TOL
                    || (n0.z - n1.z).abs() < CTX_TOL;
                if !par || n0.dot(&(p0 - p1)).abs() >= CTX_TOL {
                    return value_err!("Selected faces must be co-planar.");
                }
            }
            center = if center_option == "CenterOfBoundBox" {
                faces
                    .iter()
                    .map(|f| f.bounding_box().map(|b| b.center()))
                    .collect::<CqResult<Vec<_>>>()?
                    .iter()
                    .sum::<V3>()
                    / faces.len() as f64
            } else {
                let mut m = V3::zeros();
                let mut a = 0.0;
                for f in &faces {
                    let fa = f.area()?;
                    m += f.center() * fa;
                    a += fa;
                }
                m / a
            };
            normal = n0;
            xdir = xdir_of(&normal);
        } else {
            match self.val() {
                Obj::Shape(s) if s.shape_type() == ShapeType::Face => {
                    center = if center_option == "CenterOfBoundBox" {
                        s.bounding_box()?.center()
                    } else {
                        s.center()
                    };
                    normal = s.normal_at(Some(center))?;
                    xdir = xdir_of(&normal);
                }
                o @ (Obj::Shape(_) | Obj::Vector(_)) => {
                    center = match (&o, center_option) {
                        (Obj::Shape(s), "CenterOfBoundBox") => s.bounding_box()?.center(),
                        _ => o.center().unwrap_or_default(),
                    };
                    let pf = self.parent.as_ref().map(|p| p.val());
                    match pf {
                        Some(Obj::Shape(f)) if f.shape_type() == ShapeType::Face => {
                            normal = f.normal_at(Some(center))?;
                            xdir = xdir_of(&normal);
                        }
                        _ => {
                            normal = self.plane.z_dir;
                            xdir = self.plane.x_dir;
                        }
                    }
                }
                _ => return value_err!("Needs a face or a vertex or point on a work plane"),
            }
        }
        if center_option == "ProjectedOrigin" {
            let orig = origin.unwrap_or(self.plane.origin);
            let n = normal.normalize();
            center = orig - n * (orig - center).dot(&n);
        }
        if invert {
            normal = -normal;
        }
        let off = normal.normalize() * offset;
        let plane = Plane::new(center + off, Some(xdir), normal)?;
        let mut s = Workplane::new(plane);
        s.parent = Some(Arc::new(self.clone()));
        s.ctx = self.ctx.clone();
        Ok(s)
    }

    /// `workplaneFromTagged(name)`.
    pub fn workplane_from_tagged(&self, name: &str) -> CqResult<Workplane> {
        let mut out = self.get_tagged(name)?;
        out.parent = Some(Arc::new(self.clone()));
        out.ctx = self.ctx.clone();
        Ok(out)
    }

    /// `rotateAboutCenter(axisEnd, degrees)`.
    pub fn rotate_about_center(&self, axis: V3, deg: f64) -> CqResult<Workplane> {
        let v = self
            .objects
            .iter()
            .map(|o| match o {
                Obj::Shape(s) => {
                    let c = s.center();
                    Ok(Obj::Shape(s.rotate(c, c + axis, deg)?))
                }
                other => Ok(other.clone()),
            })
            .collect::<CqResult<_>>()?;
        Ok(self.new_object(v))
    }

    /// `rotate(start, end, degrees)`.
    pub fn rotate(&self, a: V3, b: V3, deg: f64) -> CqResult<Workplane> {
        let v = self
            .objects
            .iter()
            .map(|o| match o {
                Obj::Shape(s) => Ok(Obj::Shape(s.rotate(a, b, deg)?)),
                other => Ok(other.clone()),
            })
            .collect::<CqResult<_>>()?;
        Ok(self.new_object(v))
    }

    /// `translate(v)`.
    pub fn translate(&self, d: V3) -> CqResult<Workplane> {
        let v = self
            .objects
            .iter()
            .map(|o| match o {
                Obj::Shape(s) => Ok(Obj::Shape(s.translate(d)?)),
                other => Ok(other.clone()),
            })
            .collect::<CqResult<_>>()?;
        Ok(self.new_object(v))
    }

    /// `mirror(plane normal, base point, union)`.
    pub fn mirror(&self, normal: V3, base: V3, union: bool) -> CqResult<Workplane> {
        let v: Vec<Shape> = self
            .shapes()
            .iter()
            .map(|s| s.mirror(normal, base))
            .collect::<CqResult<_>>()?;
        let n = self.new_shapes(v);
        if union {
            self.union_wp(&n, true)
        } else {
            Ok(n)
        }
    }

    /// `shell(thickness, kind)`.
    pub fn shell(&self, t: f64, kind: &str) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let faces: Vec<Shape> = self
            .shapes()
            .into_iter()
            .filter(|s| s.shape_type() == ShapeType::Face)
            .collect();
        let s = ops::hollow(&solid, &faces, t, kind == "intersection")?;
        Ok(self.new_shapes(vec![s]))
    }

    /// `fillet(radius)`.
    pub fn fillet(&self, r: f64) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let edges = self.select(ShapeType::Edge, None, None)?.shapes();
        if edges.is_empty() {
            return value_err!("Fillets requires that edges be selected");
        }
        let s = ops::fillet(&solid, r, &edges)?;
        Ok(self.new_shapes(vec![ops::clean(&s)?]))
    }

    /// `chamfer(length, length2)`.
    pub fn chamfer(&self, l: f64, l2: Option<f64>) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let edges = self.select(ShapeType::Edge, None, None)?.shapes();
        if edges.is_empty() {
            return value_err!("Chamfer requires that edges be selected");
        }
        Ok(self.new_shapes(vec![ops::chamfer(&solid, l, l2, &edges)?]))
    }

    /// `transformed(rotate, offset)`.
    pub fn transformed(&self, rotate: V3, offset: V3) -> CqResult<Workplane> {
        let mut p = self.plane.rotated(rotate)?;
        p.origin = self.plane.to_world(offset);
        let mut ns = self.new_object(vec![Obj::Vector(p.origin)]);
        ns.plane = p;
        Ok(ns)
    }

    /// `rarray(xSpacing, ySpacing, xCount, yCount, center)`.
    pub fn rarray(
        &self,
        xs: f64,
        ys: f64,
        nx: i64,
        ny: i64,
        center: (bool, bool),
    ) -> CqResult<Workplane> {
        if (xs <= 0.0 && ys <= 0.0) || nx < 1 || ny < 1 {
            return value_err!("Spacing and count must be > 0 in at least one direction");
        }
        let mut off = V3::zeros();
        if center.0 {
            off.x -= xs * (nx - 1) as f64 * 0.5;
        }
        if center.1 {
            off.y -= ys * (ny - 1) as f64 * 0.5;
        }
        let mut pts = Vec::new();
        for x in 0..nx {
            for y in 0..ny {
                pts.push(Obj::Vector(
                    V3::new(xs * x as f64, ys * y as f64, 0.0) + off,
                ));
            }
        }
        Ok(self.push_points(pts))
    }

    /// `polarArray(radius, startAngle, angle, count, fill, rotate)`.
    pub fn polar_array(
        &self,
        radius: f64,
        start: f64,
        angle: f64,
        count: i64,
        fill: bool,
        rotate: bool,
    ) -> CqResult<Workplane> {
        if count < 1 {
            return value_err!("At least 1 element required, requested {count}");
        }
        let mut angle = angle;
        if fill {
            if (angle - 360.0 * (angle / 360.0).round()).abs() < 1e-6 {
                angle /= count as f64;
            } else {
                angle = if count > 1 {
                    angle / (count - 1) as f64
                } else {
                    start
                };
            }
        }
        let locs = (0..count)
            .map(|i| {
                let phi = start + angle * i as f64;
                let (x, y) = (
                    radius * phi.to_radians().cos(),
                    radius * phi.to_radians().sin(),
                );
                Obj::Location(if rotate {
                    Location::from_axis_angle(V3::new(x, y, 0.0), V3::z(), phi)
                } else {
                    Location::translation(V3::new(x, y, 0.0))
                })
            })
            .collect();
        Ok(self.push_points(locs))
    }

    /// `pushPoints(points)`: points in plane coordinates (made model ones), locations as given.
    pub fn push_points(&self, pts: Vec<Obj>) -> Workplane {
        let v = pts
            .into_iter()
            .map(|p| match p {
                Obj::Vector(v) => Obj::Vector(self.plane.to_world(v)),
                o => o,
            })
            .collect();
        self.new_object(v)
    }

    /// `center(x, y)`.
    pub fn center(&self, x: f64, y: f64) -> Workplane {
        let o = self.plane.to_world(V3::new(x, y, 0.0));
        let mut n = self.new_object(vec![Obj::Vector(o)]);
        n.plane.set_origin_2d(x, y);
        n
    }

    // ------------------------------------------------------------- 2D drawing

    fn find_from_point(&self, local: bool) -> CqResult<V3> {
        let obj = self
            .objects
            .last()
            .cloned()
            .unwrap_or(Obj::Vector(self.plane.origin));
        let p = match &obj {
            Obj::Vector(v) => *v,
            Obj::Shape(s) if s.shape_type() == ShapeType::Edge => s.end_point()?,
            Obj::Shape(s) if s.shape_type() == ShapeType::Vertex => s.center(),
            Obj::Shape(s) => {
                return value_err!(
                    "Cannot convert object type <class 'cadquery.occ_impl.shapes.{}'> to vector.",
                    s.shape_type().name()
                );
            }
            Obj::Location(_) => {
                return value_err!(
                    "Cannot convert object type <class 'cadquery.occ_impl.geom.Location'> to vector."
                );
            }
        };
        Ok(if local { self.plane.to_local(p) } else { p })
    }

    fn find_from_edge(&self) -> CqResult<Shape> {
        match self.objects.last() {
            Some(Obj::Shape(s)) if s.shape_type() == ShapeType::Edge => Ok(s.clone()),
            _ => Err(CqError::Value(
                "Previous Edge requested, but the previous object was not an Edge.".into(),
            )),
        }
    }

    /// `_addPendingEdge`.
    pub fn add_pending_edge(&self, e: Shape) -> CqResult<()> {
        let start = e.start_point()?;
        let mut c = self.ctx();
        c.pending_edges.push(e);
        if c.first_point.is_none() {
            c.first_point = Some(self.plane.to_local(start));
        }
        Ok(())
    }

    /// `_addPendingWire`.
    pub fn add_pending_wire(&self, w: Shape) {
        self.ctx().pending_wires.push(w);
    }

    fn edge_obj(&self, g: EdgeGeom, construction: bool) -> CqResult<Workplane> {
        let e = Shape::edge(g);
        if !construction {
            self.add_pending_edge(e.clone())?;
        }
        Ok(self.new_shapes(vec![e]))
    }

    /// `lineTo(x, y)`.
    pub fn line_to(&self, x: f64, y: f64, construction: bool) -> CqResult<Workplane> {
        let a = self.find_from_point(false)?;
        let b = self.plane.to_world(V3::new(x, y, 0.0));
        self.edge_obj(EdgeGeom::line(a, b)?, construction)
    }

    /// `line(dx, dy)`.
    pub fn line(&self, dx: f64, dy: f64, construction: bool) -> CqResult<Workplane> {
        let p = self.find_from_point(true)?;
        self.line_to(p.x + dx, p.y + dy, construction)
    }

    /// `vLineTo(y)`.
    pub fn vline_to(&self, y: f64, construction: bool) -> CqResult<Workplane> {
        let p = self.find_from_point(true)?;
        self.line_to(p.x, y, construction)
    }

    /// `hLineTo(x)`.
    pub fn hline_to(&self, x: f64, construction: bool) -> CqResult<Workplane> {
        let p = self.find_from_point(true)?;
        self.line_to(x, p.y, construction)
    }

    /// `polarLine(distance, angle)`.
    pub fn polar_line(&self, d: f64, angle: f64, construction: bool) -> CqResult<Workplane> {
        self.line(
            angle.to_radians().cos() * d,
            angle.to_radians().sin() * d,
            construction,
        )
    }

    /// `polarLineTo(distance, angle)`.
    pub fn polar_line_to(&self, d: f64, angle: f64, construction: bool) -> CqResult<Workplane> {
        self.line_to(
            angle.to_radians().cos() * d,
            angle.to_radians().sin() * d,
            construction,
        )
    }

    /// `moveTo(x, y)`.
    pub fn move_to(&self, x: f64, y: f64) -> Workplane {
        self.new_object(vec![Obj::Vector(self.plane.to_world(V3::new(x, y, 0.0)))])
    }

    /// `move(dx, dy)`.
    pub fn move_by(&self, dx: f64, dy: f64) -> CqResult<Workplane> {
        let p = self.find_from_point(true)?;
        Ok(self.new_object(vec![Obj::Vector(
            self.plane.to_world(p + V3::new(dx, dy, 0.0)),
        )]))
    }

    fn to_world_pts(&self, pts: &[V3], include_current: bool) -> CqResult<Vec<V3>> {
        let mut v: Vec<V3> = pts.iter().map(|p| self.plane.to_world(*p)).collect();
        if include_current {
            v.insert(0, self.find_from_point(false)?);
        }
        Ok(v)
    }

    fn curve_obj(&self, g: EdgeGeom, construction: bool, make_wire: bool) -> CqResult<Workplane> {
        if make_wire {
            let w = Shape::wire(vec![g]);
            if !construction {
                self.add_pending_wire(w.clone());
            }
            Ok(self.new_shapes(vec![w]))
        } else {
            self.edge_obj(g, construction)
        }
    }

    /// `bezier(points, forConstruction, includeCurrent, makeWire)`.
    pub fn bezier(
        &self,
        pts: &[V3],
        construction: bool,
        include_current: bool,
        make_wire: bool,
    ) -> CqResult<Workplane> {
        let p = self.to_world_pts(pts, include_current)?;
        self.curve_obj(make::bezier(&p)?, construction, make_wire)
    }

    /// `spline(points, tangents, periodic, parameters, scale, …)`.
    #[allow(clippy::too_many_arguments)]
    pub fn spline(
        &self,
        pts: &[V3],
        tangents: Option<&[Option<V3>]>,
        periodic: bool,
        params: Option<&[f64]>,
        scale: bool,
        construction: bool,
        include_current: bool,
        make_wire: bool,
    ) -> CqResult<Workplane> {
        let p = self.to_world_pts(pts, include_current)?;
        let t: Option<Vec<Option<V3>>> = tangents.map(|t| {
            t.iter()
                .map(|v| v.map(|v| self.plane.to_world(v) - self.plane.origin))
                .collect()
        });
        let g = make::spline(&p, t.as_deref(), periodic, params, scale)?;
        self.curve_obj(g, construction, make_wire)
    }

    /// `splineApprox(points, tol, …)` and `parametricCurve`'s fit (points in plane coordinates).
    pub fn spline_approx(
        &self,
        pts: &[V3],
        tol: f64,
        construction: bool,
        include_current: bool,
        make_wire: bool,
    ) -> CqResult<Workplane> {
        let p = self.to_world_pts(pts, include_current)?;
        self.curve_obj(make::spline_approx(&p, tol)?, construction, make_wire)
    }

    /// `ellipseArc(x_radius, y_radius, angle1, angle2, rotation_angle, sense, …)`.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse_arc(
        &self,
        rx: f64,
        ry: f64,
        a1: f64,
        a2: f64,
        rotation: f64,
        sense: i64,
        construction: bool,
        start_at_current: bool,
        make_wire: bool,
    ) -> CqResult<Workplane> {
        let center = self.find_from_point(false)?;
        let mut e = Shape::edge(make::ellipse(
            rx,
            ry,
            center,
            self.plane.z_dir,
            self.plane.x_dir,
            a1,
            a2,
            sense == 1,
        )?);
        if rotation != 0.0 {
            e = e.rotate(center, center + self.plane.z_dir, rotation)?;
        }
        if start_at_current {
            let s = e.start_point()?;
            e = e.translate(center - s)?;
        }
        let g = e.edge_geoms().remove(0);
        self.curve_obj(g, construction, make_wire)
    }

    /// `threePointArc(point1, point2)`.
    pub fn three_point_arc(&self, p1: V3, p2: V3, construction: bool) -> CqResult<Workplane> {
        let a = self.find_from_point(false)?;
        let (m, b) = (self.plane.to_world(p1), self.plane.to_world(p2));
        self.edge_obj(make::three_point_arc(a, m, b)?, construction)
    }

    /// `sagittaArc(endPoint, sag)`.
    pub fn sagitta_arc(&self, end: V3, sag: f64, construction: bool) -> CqResult<Workplane> {
        let s = self.find_from_point(true)?;
        let mid = (end + s) * 0.5;
        let mut v = (end - s).normalize() * sag.abs();
        if sag > 0.0 {
            v = V3::new(-v.y, v.x, v.z);
        } else {
            v = V3::new(v.y, -v.x, v.z);
        }
        self.three_point_arc(mid + v, end, construction)
    }

    /// `radiusArc(endPoint, radius)`.
    pub fn radius_arc(&self, end: V3, r: f64, construction: bool) -> CqResult<Workplane> {
        let s = self.find_from_point(true)?;
        let l = (end - s).norm() / 2.0;
        let mut sag = r.abs();
        let d = r * r - l * l;
        if d.abs() >= 1e-6 {
            if d < 0.0 {
                return value_err!("Arc radius is not large enough to reach the end point.");
            }
            sag -= d.sqrt();
        }
        self.sagitta_arc(end, if r > 0.0 { sag } else { -sag }, construction)
    }

    /// `tangentArcPoint(endpoint, forConstruction, relative)`.
    pub fn tangent_arc_point(
        &self,
        end: V3,
        construction: bool,
        relative: bool,
    ) -> CqResult<Workplane> {
        let mut e = end;
        if relative {
            e += self.find_from_point(true)?;
        }
        let e = self.plane.to_world(e);
        let prev = self.find_from_edge()?;
        let g = make::tangent_arc(prev.end_point()?, prev.tangent_at(1.0, true)?, e)?;
        self.edge_obj(g, construction)
    }

    /// `wire(forConstruction)`: the pending edges joined into a pending wire.
    pub fn wire(&self, construction: bool) -> CqResult<Workplane> {
        let edges = {
            let mut c = self.ctx();
            if c.pending_edges.is_empty() {
                return Ok(self.clone());
            }
            std::mem::take(&mut c.pending_edges)
        };
        let geoms: Vec<EdgeGeom> = edges.iter().flat_map(|e| e.edge_geoms()).collect();
        let w = Shape::wire(make::assemble(geoms)?);
        if !construction {
            self.add_pending_wire(w.clone());
        }
        let mut others: Vec<Obj> = self
            .objects
            .iter()
            .filter(|o| !matches!(o, Obj::Shape(s) if s.shape_type() == ShapeType::Edge))
            .cloned()
            .collect();
        others.push(Obj::Shape(w));
        Ok(self.new_object(others))
    }

    /// `_consolidateWires`: the pending edges and wires chained into wires.
    pub fn consolidated(&self) -> Vec<Shape> {
        let c = self.ctx();
        let geoms: Vec<EdgeGeom> = c
            .pending_edges
            .iter()
            .chain(c.pending_wires.iter())
            .flat_map(|e| e.edge_geoms())
            .collect();
        if geoms.is_empty() {
            return vec![];
        }
        make::chain_edges(geoms, Some(1e-9_f64.max(1e-7)))
            .into_iter()
            .map(Shape::wire)
            .collect()
    }

    /// `consolidateWires()`.
    pub fn consolidate_wires(&self) -> Workplane {
        let w = self.consolidated();
        if w.is_empty() {
            return self.clone();
        }
        let r = self.new_shapes(w.clone());
        let mut c = r.ctx();
        c.pending_wires = w;
        c.pending_edges.clear();
        c.first_point = None;
        drop(c);
        r
    }

    /// `mirrorY()` / `mirrorX()`: the drawing and its mirror image in the plane's y (x) axis.
    pub fn mirror_axis(&self, y_axis: bool) -> CqResult<Workplane> {
        let n = self.wire(false)?;
        let consolidated = n.consolidate_wires();
        let wires = consolidated.select(ShapeType::Wire, None, None)?.shapes();
        let (x, y, _) = self.plane.frame();
        let axis = if y_axis { y } else { x };
        // Reflection in the line through the origin along `axis` (within the plane).
        let o = self.plane.origin;
        let m = 2.0 * axis * axis.transpose() - nalgebra::Matrix3::identity();
        // (That also flips the plane's normal; reflect it back across the plane.)
        let z = self.plane.z_dir;
        let m = (nalgebra::Matrix3::identity() - 2.0 * z * z.transpose()) * m;
        let aff = crate::native::math::Affine { m, t: o - m * o };
        let mut out = consolidated.clone();
        for w in wires {
            let mw = w.transformed(&aff)?;
            out.objects.push(Obj::Shape(mw.clone()));
            out.add_pending_wire(mw);
        }
        Ok(out.consolidate_wires())
    }

    /// `eachpoint(f, useLocalCoordinates, combine, clean)` with a shape maker: `f` gets each
    /// point's location (plane coordinates when `local`).
    pub fn eachpoint(
        &self,
        f: &dyn Fn(&Location) -> CqResult<Shape>,
        local: bool,
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let res = self.eachpoint_shapes(f, local)?;
        for r in &res {
            if r.shape_type() == ShapeType::Wire && !r.for_construction {
                self.add_pending_wire(r.clone());
            }
        }
        self.combine_with_base(res, combine, clean)
    }

    /// The locations `eachpoint` places at: each object's (a point's or shape's centre in plane
    /// coordinates, a location as it is), or the plane's origin.
    pub fn locations(&self) -> Vec<Location> {
        let loc = self.plane.location();
        if self.objects.is_empty() {
            return vec![Location::default()];
        }
        self.objects
            .iter()
            .map(|o| match o {
                Obj::Location(l) => *l,
                _ => loc.inverse().then_after(&Location::of_plane_at(
                    &self.plane,
                    o.center().unwrap_or_default(),
                )),
            })
            .collect()
    }

    /// The shapes `f` makes at each location, placed on the plane.
    pub fn eachpoint_shapes(
        &self,
        f: &dyn Fn(&Location) -> CqResult<Shape>,
        local: bool,
    ) -> CqResult<Vec<Shape>> {
        let loc = self.plane.location();
        self.locations()
            .iter()
            .map(|p| {
                if local {
                    f(p)?.moved(&loc)
                } else {
                    f(&loc.then_after(p))
                }
            })
            .collect()
    }

    /// `_combineWithBase(objs, mode, clean)`.
    pub fn combine_with_base(
        &self,
        objs: Vec<Shape>,
        mode: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let mut ns = match mode {
            Combine::No => self.new_shapes(objs),
            _ => {
                let obj = if objs.len() == 1 {
                    objs.into_iter().next().unwrap()
                } else {
                    Shape::compound(objs)
                };
                match mode {
                    Combine::Cut => {
                        let r = match self.find_solid_opt(true, true) {
                            Some(base) => ops::cut(&base, &[obj])?,
                            None => obj,
                        };
                        self.new_shapes(vec![r])
                    }
                    _ => {
                        let r = match self.find_solid_opt(true, true) {
                            Some(base) => ops::fuse(&base, &[obj])?,
                            None => ops::fuse(&obj, &[])?,
                        };
                        self.new_shapes(vec![r])
                    }
                }
            }
        };
        if clean {
            ns.objects = ns
                .objects
                .into_iter()
                .map(|o| match o {
                    Obj::Shape(s) => Ok(Obj::Shape(ops::clean(&s)?)),
                    o => Ok(o),
                })
                .collect::<CqResult<_>>()?;
        }
        Ok(ns)
    }

    fn place_each(&self, base: Shape, construction: bool) -> CqResult<Workplane> {
        let base = base.construction(construction);
        self.eachpoint(&|l: &Location| base.moved(l), true, Combine::No, true)
    }

    /// `rect(xLen, yLen, centered, forConstruction)`.
    pub fn rect(
        &self,
        xl: f64,
        yl: f64,
        centered: (bool, bool),
        construction: bool,
    ) -> CqResult<Workplane> {
        let mut off = V3::zeros();
        if !centered.0 {
            off.x += xl / 2.0;
        }
        if !centered.1 {
            off.y += yl / 2.0;
        }
        let pts: Vec<V3> = [
            (-1.0, -1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
        ]
        .iter()
        .map(|(a, b)| V3::new(a * xl / 2.0, b * yl / 2.0, 0.0) + off)
        .collect();
        self.place_each(Shape::wire(make::polygon(&pts, false)?), construction)
    }

    /// `circle(radius, forConstruction)`.
    pub fn circle(&self, r: f64, construction: bool) -> CqResult<Workplane> {
        self.place_each(
            Shape::wire(vec![make::circle(
                r,
                V3::zeros(),
                V3::z(),
                360.0,
                360.0,
                true,
            )?]),
            construction,
        )
    }

    /// `ellipse(x_radius, y_radius, rotation_angle, forConstruction)`.
    pub fn ellipse(
        &self,
        rx: f64,
        ry: f64,
        rotation: f64,
        construction: bool,
    ) -> CqResult<Workplane> {
        let mut e = Shape::wire(vec![make::ellipse(
            rx,
            ry,
            V3::zeros(),
            V3::z(),
            V3::x(),
            360.0,
            360.0,
            true,
        )?]);
        if rotation != 0.0 {
            e = e.rotate(V3::zeros(), V3::z(), rotation)?;
        }
        self.place_each(e, construction)
    }

    /// `polygon(nSides, diameter, forConstruction, circumscribed)`.
    pub fn polygon(
        &self,
        n: i64,
        diameter: f64,
        construction: bool,
        circumscribed: bool,
    ) -> CqResult<Workplane> {
        if n < 3 {
            return value_err!("a polygon needs three sides or more");
        }
        let a = std::f64::consts::TAU / n as f64;
        let mut r = diameter / 2.0;
        if circumscribed {
            r /= (a / 2.0).cos();
        }
        let pts: Vec<V3> = (0..=n)
            .map(|i| {
                let o = a * i as f64 + if circumscribed { a / 2.0 } else { 0.0 };
                V3::new(r * o.cos(), r * o.sin(), 0.0)
            })
            .collect();
        self.place_each(Shape::wire(make::polygon(&pts, false)?), construction)
    }

    /// `polyline(points, forConstruction, includeCurrent)`.
    pub fn polyline(
        &self,
        pts: &[V3],
        construction: bool,
        include_current: bool,
    ) -> CqResult<Workplane> {
        let (mut start, rest) = if include_current {
            (self.find_from_point(false)?, pts)
        } else {
            match pts.split_first() {
                Some((f, r)) => (self.plane.to_world(*f), r),
                None => return value_err!("polyline needs points"),
            }
        };
        let mut edges = Vec::new();
        for p in rest {
            let end = self.plane.to_world(*p);
            let e = Shape::edge(EdgeGeom::line(start, end)?);
            if !construction {
                self.add_pending_edge(e.clone())?;
            }
            edges.push(e);
            start = end;
        }
        Ok(self.new_shapes(edges))
    }

    /// `close()`.
    pub fn close(&self) -> CqResult<Workplane> {
        let end = self.find_from_point(true)?;
        let start = match self.ctx().first_point {
            Some(p) => p,
            None => return value_err!("No start point specified - cannot close"),
        };
        if (end - start).norm() > 1e-6 {
            self.polyline(&[end, start], false, false)?;
        }
        self.ctx().first_point = None;
        self.wire(false)
    }

    /// `slot2D(length, diameter, angle)`.
    pub fn slot2d(&self, length: f64, diameter: f64, angle: f64) -> CqResult<Workplane> {
        let r = diameter / 2.0;
        let p1 = V3::new(-length / 2.0 + r, diameter / 2.0, 0.0);
        let p2 = p1 + V3::new(length - diameter, 0.0, 0.0);
        let p3 = p1 + V3::new(length - diameter, -diameter, 0.0);
        let p4 = p1 + V3::new(0.0, -diameter, 0.0);
        let arc1 = p2 + V3::new(r, -r, 0.0);
        let arc2 = p4 + V3::new(-r, r, 0.0);
        let mut edges = Vec::new();
        if (p2 - p1).norm() > 0.0 {
            edges.push(EdgeGeom::line(p1, p2)?);
        }
        edges.push(make::three_point_arc(p2, arc1, p3)?);
        if (p4 - p3).norm() > 0.0 {
            edges.push(EdgeGeom::line(p3, p4)?);
        }
        edges.push(make::three_point_arc(p4, arc2, p1)?);
        let slot = Shape::wire(make::assemble(edges)?).rotate(V3::zeros(), V3::z(), angle)?;
        self.place_each(slot, false)
    }

    // ------------------------------------------------------------- 3D

    /// `largestDimension()`.
    pub fn largest_dimension(&self) -> CqResult<f64> {
        Ok(self.find_solid()?.bounding_box()?.diagonal())
    }

    /// `cutEach(f, useLocalCoords, clean)`.
    pub fn cut_each(
        &self,
        f: &dyn Fn(&Location) -> CqResult<Shape>,
        local: bool,
        clean: bool,
    ) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let tools = self.eachpoint_shapes(f, local)?;
        let mut s = ops::cut(&solid, &tools)?;
        if clean {
            s = ops::clean(&s)?;
        }
        Ok(self.new_shapes(vec![s]))
    }

    /// `hole(diameter, depth, clean)`.
    pub fn hole(&self, d: f64, depth: Option<f64>, clean: bool) -> CqResult<Workplane> {
        let depth = match depth {
            Some(x) => x,
            None => self.largest_dimension()?,
        };
        let h = make::cylinder(d / 2.0, depth, V3::zeros(), -V3::z(), 360.0)?;
        self.cut_each(&|l: &Location| h.moved(l), true, clean)
    }

    /// `cboreHole(diameter, cboreDiameter, cboreDepth, depth, clean)`.
    pub fn cbore_hole(
        &self,
        d: f64,
        cd: f64,
        cdepth: f64,
        depth: Option<f64>,
        clean: bool,
    ) -> CqResult<Workplane> {
        let depth = match depth {
            Some(x) => x,
            None => self.largest_dimension()?,
        };
        let hole = make::cylinder(d / 2.0, depth, V3::zeros(), -V3::z(), 360.0)?;
        let cb = make::cylinder(cd / 2.0, cdepth, V3::zeros(), -V3::z(), 360.0)?;
        let r = ops::fuse(&hole, &[cb])?;
        self.cut_each(&|l: &Location| r.moved(l), true, clean)
    }

    /// `cskHole(diameter, cskDiameter, cskAngle, depth, clean)`.
    pub fn csk_hole(
        &self,
        d: f64,
        cd: f64,
        angle: f64,
        depth: Option<f64>,
        clean: bool,
    ) -> CqResult<Workplane> {
        let depth = match depth {
            Some(x) => x,
            None => self.largest_dimension()?,
        };
        let hole = make::cylinder(d / 2.0, depth, V3::zeros(), -V3::z(), 360.0)?;
        let r = cd / 2.0;
        let h = r / (angle / 2.0).to_radians().tan();
        let csk = make::cone(r, 0.0, h, V3::zeros(), -V3::z(), 360.0)?;
        let res = ops::fuse(&hole, &[csk])?;
        self.cut_each(&|l: &Location| res.moved(l), true, clean)
    }

    /// `_getFaces()`: the faces on the stack, or faces of the pending wires.
    pub fn get_faces(&self) -> CqResult<Vec<Shape>> {
        let mut rv = Vec::new();
        for o in &self.objects {
            if let Obj::Shape(s) = o {
                match s.shape_type() {
                    ShapeType::Face => rv.push(s.clone()),
                    ShapeType::Compound => rv.extend(
                        s.children()
                            .into_iter()
                            .filter(|c| c.shape_type() == ShapeType::Face),
                    ),
                    _ => {}
                }
            }
        }
        if rv.is_empty() {
            let wires = {
                let mut c = self.ctx();
                if c.pending_wires.is_empty() {
                    return value_err!("No pending wires present");
                }
                std::mem::take(&mut c.pending_wires)
            };
            let ws: Vec<Vec<EdgeGeom>> = wires.iter().map(|w| w.edge_geoms()).collect();
            rv = make::faces_from_wires(&ws, Some(&self.plane))?;
        }
        Ok(rv)
    }

    /// `_extrude(distance, both, taper)`: the prisms of the faces along the plane's normal.
    fn extrude_faces(&self, d: f64, both: bool, taper: f64) -> CqResult<Shape> {
        let faces = self.get_faces()?;
        if taper != 0.0 {
            for f in &faces {
                if !f.inner_wires()?.is_empty() {
                    return value_err!("Inner wires not allowed with tapered extrusion");
                }
            }
        }
        let e = self.plane.z_dir * d;
        if both {
            if taper != 0.0 {
                let a = ops::extrude(&faces, e, taper)?;
                let b = ops::extrude(&faces, -e, taper)?;
                return ops::fuse(&a, &[b]);
            }
            let back: Vec<Shape> = faces
                .iter()
                .map(|f| f.translate(-e))
                .collect::<CqResult<_>>()?;
            return ops::extrude(&back, e * 2.0, 0.0);
        }
        ops::extrude(&faces, e, taper)
    }

    /// `extrude(until, combine, clean, both, taper)`.
    pub fn extrude(
        &self,
        until: Until,
        combine: Combine,
        clean: bool,
        both: bool,
        taper: f64,
    ) -> CqResult<Workplane> {
        if combine == Combine::Cut {
            return self.cut_blind(until, clean, both, taper);
        }
        let r = match until {
            Until::Distance(d) => self.extrude_faces(d, both, taper)?,
            Until::Next | Until::Last | Until::Face(_) => {
                if combine == Combine::No {
                    return value_err!(
                        "`combine` can't be set to False when extruding until a face"
                    );
                }
                return self.extrude_until(until, both, taper, true, clean);
            }
        };
        self.combine_with_base(vec![r], combine, clean)
    }

    /// Up to the next or last face of the stack's solid, or a face (`_extrude(upToFace=…)`).
    fn extrude_until(
        &self,
        until: Until,
        _both: bool,
        taper: f64,
        additive: bool,
        clean: bool,
    ) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let faces = self.get_faces()?;
        let n = self.plane.z_dir;
        let dir = if additive { n } else { -n };
        let _ = taper;
        let bb = solid.bounding_box()?;
        let mut res = solid.clone();
        for f in &faces {
            // The faces the line from the face's centre along the direction crosses, in order.
            let c = f.center();
            let reach = bb.diagonal() * 2.0 + (c - bb.center()).norm();
            let hits = line_hits(&solid, c, dir, reach)?;
            let hits = if hits.is_empty() {
                line_hits(&solid, c, -dir, reach)?
            } else {
                hits
            };
            if hits.is_empty() {
                return value_err!(
                    "Couldn't find a face to extrude/cut to. Check your workplane orientation."
                );
            }
            let t = match &until {
                Until::Next => {
                    let inside = ops::is_inside(&solid, c, 1e-6)?;
                    let i = if inside && additive && hits.len() > 1 {
                        1
                    } else {
                        0
                    };
                    hits[i.min(hits.len() - 1)]
                }
                Until::Last => *hits.last().unwrap(),
                Until::Face(face) => {
                    let fc = face.center();
                    (fc - c).dot(&dir)
                }
                Until::Distance(d) => *d,
            };
            let prism = ops::extrude(std::slice::from_ref(f), dir * t, 0.0)?;
            res = if additive {
                ops::fuse(&res, &[prism])?
            } else {
                ops::cut(&res, &[prism])?
            };
        }
        if clean {
            res = ops::clean(&res)?;
        }
        Ok(self.new_shapes(vec![res]))
    }

    /// `cutBlind(until, clean, both, taper)`.
    pub fn cut_blind(
        &self,
        until: Until,
        clean: bool,
        both: bool,
        taper: f64,
    ) -> CqResult<Workplane> {
        match until {
            Until::Distance(d) => {
                let tool = self.extrude_faces(d, both, taper)?;
                let solid = self.find_solid()?;
                let mut s = ops::cut(&solid, &[tool])?;
                if clean {
                    s = ops::clean(&s)?;
                }
                Ok(self.new_shapes(vec![s]))
            }
            u => self.extrude_until(u, both, taper, false, clean),
        }
    }

    /// `cutThruAll(clean, taper)`.
    pub fn cut_thru_all(&self, clean: bool, _taper: f64) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let faces = self.get_faces()?;
        let tool = ops::through_prism(&solid, &faces, self.plane.z_dir)?;
        let mut s = ops::cut(&solid, &[tool])?;
        if clean {
            s = ops::clean(&s)?;
        }
        Ok(self.new_shapes(vec![s]))
    }

    /// `twistExtrude(distance, angleDegrees, combine, clean)`.
    pub fn twist_extrude(
        &self,
        d: f64,
        deg: f64,
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let faces = self.get_faces()?;
        let o = self.plane.origin;
        let path = vec![EdgeGeom::line(o, o + self.plane.z_dir * d)?];
        let r = if deg == 0.0 {
            ops::extrude(&faces, self.plane.z_dir * d, 0.0)?
        } else {
            ops::sweep_twisted(&faces, &path, deg.to_radians())?
        };
        self.combine_with_base(vec![r], combine, clean)
    }

    /// `revolve(angleDegrees, axisStart, axisEnd, combine, clean)`.
    pub fn revolve(
        &self,
        deg: f64,
        axis_start: Option<V3>,
        axis_end: Option<V3>,
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let mut deg = deg.rem_euclid(360.0);
        if deg == 0.0 {
            deg = 360.0;
        }
        let start = self.plane.to_world(axis_start.unwrap_or_default());
        let end = match axis_end {
            Some(e) => self.plane.to_world(e),
            None => {
                let s = axis_start.unwrap_or_default();
                if s.y != 0.0 {
                    self.plane.to_world(V3::new(0.0, s.y, 0.0))
                } else {
                    self.plane.to_world(V3::new(0.0, 1.0, 0.0))
                }
            }
        };
        let faces = self.get_faces()?;
        let r = ops::revolve(&faces, deg, start, end)?;
        self.combine_with_base(vec![r], combine, clean)
    }

    /// `sweep(path, multisection, makeSolid, isFrenet, combine, clean, transition, normal)`.
    #[allow(clippy::too_many_arguments)]
    pub fn sweep(
        &self,
        path: &Shape,
        multisection: bool,
        solid: bool,
        frenet: bool,
        combine: Combine,
        clean: bool,
        transition: &str,
        normal: Option<V3>,
    ) -> CqResult<Workplane> {
        if !matches!(path.shape_type(), ShapeType::Wire | ShapeType::Edge) {
            return value_err!("Wire or Edge instance required");
        }
        let mode = match normal {
            Some(n) => ops::SweepMode::Fixed(n),
            None if frenet => ops::SweepMode::Frenet,
            None => ops::SweepMode::Corrected,
        };
        if multisection {
            // The sections: the pending wires, else the faces' outer wires (`Solid.sweep_multi`).
            let pending = std::mem::take(&mut self.ctx().pending_wires);
            let sections: Vec<Vec<EdgeGeom>> = if !pending.is_empty() {
                pending.iter().map(|w| w.edge_geoms()).collect()
            } else {
                self.get_faces()?
                    .iter()
                    .map(|f| f.outer_wire().map(|w| w.edge_geoms()))
                    .collect::<CqResult<_>>()?
            };
            let r = match sections.len() {
                0 => return value_err!("No pending wires present"),
                // One section: the sweep of its face (holes of none).
                1 => {
                    let faces = make::faces_from_wires(&sections, Some(&self.plane))?;
                    ops::sweep(
                        &faces,
                        &path.edge_geoms(),
                        solid,
                        mode,
                        transition == "round",
                    )?
                }
                _ => ops::loft(
                    &sections
                        .into_iter()
                        .map(ops::LoftItem::Wire)
                        .collect::<Vec<_>>(),
                    false,
                )?,
            };
            return self.combine_with_base(vec![r], combine, clean);
        }
        let faces = self.get_faces()?;
        let r = ops::sweep(
            &faces,
            &path.edge_geoms(),
            solid,
            mode,
            transition == "round",
        )?;
        self.combine_with_base(vec![r], combine, clean)
    }

    /// `loft(ruled, combine, clean)`.
    pub fn loft(&self, ruled: bool, combine: Combine, clean: bool) -> CqResult<Workplane> {
        let pending = std::mem::take(&mut self.ctx().pending_wires);
        let items: Vec<ops::LoftItem> = if !pending.is_empty() {
            pending
                .iter()
                .map(|w| ops::LoftItem::Wire(w.edge_geoms()))
                .collect()
        } else {
            let mut v = Vec::new();
            for o in &self.objects {
                if let Obj::Shape(s) = o {
                    for c in s.children() {
                        match c.shape_type() {
                            ShapeType::Face => {
                                v.push(ops::LoftItem::Wire(c.outer_wire()?.edge_geoms()))
                            }
                            ShapeType::Vertex => v.push(ops::LoftItem::Point(c.center())),
                            _ => {}
                        }
                    }
                }
            }
            v
        };
        if items.is_empty() {
            return value_err!("Nothing to loft");
        }
        if items.len() == 1 {
            return value_err!("More than one wire or face is required");
        }
        let r = ops::loft(&items, ruled)?;
        self.combine_with_base(vec![r], combine, clean)
    }

    /// `combine(clean, glue)`: the stack's shapes united.
    pub fn combine(&self, clean: bool) -> CqResult<Workplane> {
        let items = self.shapes();
        let Some((first, rest)) = items.split_first() else {
            return value_err!("pop from empty list");
        };
        let mut s = if rest.is_empty() {
            first.clone()
        } else {
            ops::fuse(first, rest)?
        };
        if clean {
            s = ops::clean(&s)?;
        }
        Ok(self.new_shapes(vec![s]))
    }

    /// `union(shapes)` with the shapes given.
    pub fn union_shapes(&self, new: Vec<Shape>, clean: bool) -> CqResult<Workplane> {
        let r = match self.find_solid_opt(true, true) {
            Some(base) if new.is_empty() => base,
            Some(base) => ops::fuse(&base, &new)?,
            None if new.len() > 1 => ops::fuse(&new[0], &new[1..])?,
            None => match new.into_iter().next() {
                Some(s) => s,
                None => return value_err!("Cannot union type '<class 'NoneType'>'"),
            },
            // (Nothing to add: the base as it is.)
        };
        let r = if clean { ops::clean(&r)? } else { r };
        Ok(self.new_shapes(vec![r]))
    }

    /// `union(workplane)`.
    pub fn union_wp(&self, o: &Workplane, clean: bool) -> CqResult<Workplane> {
        let new = o.select(ShapeType::Solid, None, None)?.shapes();
        if new.is_empty() {
            // The empty solid a revolve about its plane's own normal makes (OCCT's flat solid of
            // no volume) unites to nothing.
            let empty =
                |s: &Shape| matches!(&s.repr, super::shape::Repr::Compound(v) if v.is_empty());
            if !o.shapes().is_empty() && o.shapes().iter().all(empty) {
                self.merge_tags(o);
                return self.union_shapes(vec![], clean);
            }
            return value_err!(
                "Workplane object must have at least one solid on the stack to union!"
            );
        }
        self.merge_tags(o);
        self.union_shapes(new, clean)
    }

    /// `cut(shapes)`.
    pub fn cut_shapes(&self, tools: Vec<Shape>, clean: bool) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let mut s = ops::cut(&solid, &tools)?;
        if clean {
            s = ops::clean(&s)?;
        }
        Ok(self.new_shapes(vec![s]))
    }

    /// `intersect(shapes)`.
    pub fn intersect_shapes(&self, tools: Vec<Shape>, clean: bool) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let mut s = ops::intersect(&solid, &tools)?;
        if clean {
            s = ops::clean(&s)?;
        }
        Ok(self.new_shapes(vec![s]))
    }

    /// `box(length, width, height, centered, combine, clean)`.
    pub fn box_(
        &self,
        l: f64,
        w: f64,
        h: f64,
        centered: (bool, bool, bool),
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let mut off = V3::zeros();
        if centered.0 {
            off.x -= l / 2.0;
        }
        if centered.1 {
            off.y -= w / 2.0;
        }
        if centered.2 {
            off.z -= h / 2.0;
        }
        let b = make::make_box(l, w, h, off, V3::z())?;
        self.eachpoint(&|loc: &Location| b.moved(loc), true, combine, clean)
    }

    /// `sphere(radius, direct, angle1, angle2, angle3, centered, combine, clean)`.
    #[allow(clippy::too_many_arguments)]
    pub fn sphere(
        &self,
        r: f64,
        direct: V3,
        a1: f64,
        a2: f64,
        a3: f64,
        centered: (bool, bool, bool),
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let mut off = V3::zeros();
        if !centered.0 {
            off.x += r;
        }
        if !centered.1 {
            off.y += r;
        }
        if !centered.2 {
            off.z += r;
        }
        let s = make::sphere(r, off, direct, a1, a2, a3)?;
        self.eachpoint(&|loc: &Location| s.moved(loc), true, combine, clean)
    }

    /// `cylinder(height, radius, direct, angle, centered, combine, clean)`.
    #[allow(clippy::too_many_arguments)]
    pub fn cylinder(
        &self,
        h: f64,
        r: f64,
        direct: V3,
        angle: f64,
        centered: (bool, bool, bool),
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let mut off = V3::zeros();
        if !centered.0 {
            off.x += r;
        }
        if !centered.1 {
            off.y += r;
        }
        if centered.2 {
            off.z -= h / 2.0;
        }
        let s = make::cylinder(r, h, off, V3::z(), angle)?;
        let s = s.moved(&Plane::new(V3::zeros(), None, direct)?.location())?;
        self.eachpoint(&|loc: &Location| s.moved(loc), true, combine, clean)
    }

    /// `wedge(dx, dy, dz, xmin, zmin, xmax, zmax, pnt, dir, centered, combine, clean)`.
    #[allow(clippy::too_many_arguments)]
    pub fn wedge(
        &self,
        d: V3,
        xmin: f64,
        zmin: f64,
        xmax: f64,
        zmax: f64,
        pnt: V3,
        dir: V3,
        centered: (bool, bool, bool),
        combine: Combine,
        clean: bool,
    ) -> CqResult<Workplane> {
        let mut off = pnt;
        if centered.0 {
            off.x -= d.x / 2.0;
        }
        if centered.1 {
            off.y -= d.y / 2.0;
        }
        if centered.2 {
            off.z -= d.z / 2.0;
        }
        let w = make::wedge(d.x, d.y, d.z, xmin, zmin, xmax, zmax, off, dir)?;
        self.eachpoint(&|loc: &Location| w.moved(loc), true, combine, clean)
    }

    /// `clean()`.
    pub fn clean(&self) -> CqResult<Workplane> {
        let v = self
            .objects
            .iter()
            .map(|o| match o {
                Obj::Shape(s) => Ok(Obj::Shape(ops::clean(s)?)),
                o => Ok(o.clone()),
            })
            .collect::<CqResult<_>>()?;
        Ok(self.new_object(v))
    }

    /// `toPending()`.
    pub fn to_pending(&self) -> Workplane {
        let mut c = self.ctx();
        for o in &self.objects {
            if let Obj::Shape(s) = o {
                match s.shape_type() {
                    ShapeType::Wire => c.pending_wires.push(s.clone()),
                    ShapeType::Edge => c.pending_edges.push(s.clone()),
                    _ => {}
                }
            }
        }
        drop(c);
        self.clone()
    }

    /// `offset2D(d, kind, forConstruction)`.
    pub fn offset2d(&self, d: f64, kind: &str, construction: bool) -> CqResult<Workplane> {
        let ws = self.consolidated();
        let mut rv = Vec::new();
        for w in &ws {
            let g = w.edge_geoms();
            let pl = make::wires_plane(std::slice::from_ref(&g), Some(&self.plane))?;
            for o in ops::offset_wires(&g, &pl, d, kind == "intersection")? {
                rv.push(Shape::wire(o).construction(construction));
            }
        }
        {
            let mut c = self.ctx();
            c.pending_edges.clear();
            c.pending_wires = if construction { vec![] } else { rv.clone() };
        }
        Ok(self.new_shapes(rv))
    }

    /// `split(keepTop, keepBottom)`: the stack's solid cut by the plane.
    pub fn split_keep(&self, top: bool, bottom: bool) -> CqResult<Workplane> {
        if !top && !bottom {
            return value_err!("You have to keep at least one half");
        }
        let solid = self.find_solid()?;
        let m = solid.bounding_box()?.diagonal() * 10.0;
        let r = make::make_box(m, m, m, V3::new(-m / 2.0, -m / 2.0, 0.0), V3::z())?
            .moved(&self.plane.location())?;
        let below = r.translate(-self.plane.z_dir * m)?;
        let mut rv = Vec::new();
        if top {
            rv.push(ops::cut(&solid, &[below.clone()])?);
        }
        if bottom {
            rv.push(ops::cut(&solid, &[r])?);
        }
        Ok(self.new_shapes(rv))
    }

    /// `split(tool)`: the stack's solid split by a shape.
    pub fn split_by(&self, tools: Vec<Shape>) -> CqResult<Workplane> {
        let solid = self.find_solid()?;
        let inside = ops::intersect(&solid, &tools)?;
        let outside = ops::cut(&solid, &tools)?;
        Ok(self.new_shapes(vec![Shape::compound(vec![outside, inside])]))
    }

    /// `section(height)`.
    pub fn section(&self, _height: f64) -> CqResult<Workplane> {
        Err(CqError::NotImplemented("section".into()))
    }
}

/// Where the line from `p` along unit `d` crosses the solid's faces: the distances along it
/// (ascending, within `reach`).
fn line_hits(solid: &Shape, p: V3, d: V3, reach: f64) -> CqResult<Vec<f64>> {
    use crate::Kernel;
    let body = solid.body()?;
    let hits = super::session(|k| k.ray_hits(body.0, crate::native::math::P3::from(p), d))?;
    let mut t: Vec<f64> = hits
        .into_iter()
        .map(|h| h.t)
        .filter(|t| *t > 1e-9 && *t <= reach)
        .collect();
    t.sort_by(f64::total_cmp);
    t.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    Ok(t)
}
