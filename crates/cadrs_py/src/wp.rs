//! `cadquery.Workplane`.

use crate::conv::{
    Py3, V3, centered3, num, obj_py, points, py_obj, shape, shapes, type_name, v3, v3_opt, vector,
};
use crate::geom::{PyLocation, PyPlane};
use crate::sel::selector_filter;
use crate::shapes::{PyShape, wrap};
use cadrs_kernel::cadquery::{
    Combine, CqError, Location, Obj, Plane, Shape, ShapeType, Until, Workplane,
};
use pyo3::exceptions::{PyIndexError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PySlice, PyString, PyTuple};

/// `cadquery.Workplane`.
#[pyclass(name = "Workplane", module = "cadquery", subclass, dict)]
#[derive(Clone)]
pub struct PyWorkplane {
    pub w: Workplane,
}

type R = PyResult<PyWorkplane>;

fn ok(w: Workplane) -> R {
    Ok(PyWorkplane { w })
}

fn combine(o: Option<&Bound<'_, PyAny>>, default: Combine) -> PyResult<Combine> {
    let Some(o) = o else { return Ok(default) };
    if let Ok(s) = o.extract::<String>() {
        return match s.as_str() {
            "a" => Ok(Combine::Fuse),
            "s" | "cut" => Ok(Combine::Cut),
            _ => Err(PyValueError::new_err(format!("Unknown combine mode {s}"))),
        };
    }
    Ok(if o.is_truthy()? {
        Combine::Fuse
    } else {
        Combine::No
    })
}

fn centered2(o: Option<&Bound<'_, PyAny>>) -> PyResult<(bool, bool)> {
    let (a, b, _) = centered3(o, true)?;
    Ok((a, b))
}

/// The shapes a workplane or shape argument stands for.
fn shapes_of(o: &Bound<'_, PyAny>) -> PyResult<(Vec<Shape>, Option<Workplane>)> {
    if let Ok(w) = o.cast::<PyWorkplane>() {
        let w = w.borrow().w.clone();
        return Ok((w.shapes(), Some(w)));
    }
    if let Ok(s) = o.cast::<PyShape>() {
        return Ok((vec![s.borrow().s.clone()], None));
    }
    Err(PyValueError::new_err(format!(
        "Cannot use type '{}'",
        type_name(o)
    )))
}

impl PyWorkplane {
    fn select(
        &self,
        py: Python<'_>,
        kind: ShapeType,
        selector: Option<&Bound<'_, PyAny>>,
        tag: Option<&str>,
    ) -> R {
        match selector {
            Some(s) if !s.is_none() => {
                if let Ok(st) = s.extract::<String>() {
                    let sel = cadrs_kernel::cadquery::Selector::parse(&st).py()?;
                    return ok(self.w.select(kind, Some(&sel), tag).py()?);
                }
                let all = self.w.select(kind, None, tag).py()?;
                let kept = selector_filter(py, s, all.shapes())?;
                ok(self
                    .w
                    .new_object(kept.into_iter().map(Obj::Shape).collect()))
            }
            _ => ok(self.w.select(kind, None, tag).py()?),
        }
    }

    fn objs_list(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let v: Vec<Py<PyAny>> = self
            .w
            .objects
            .iter()
            .map(|o| obj_py(py, o))
            .collect::<PyResult<_>>()?;
        Ok(PyList::new(py, v)?.into_any().unbind())
    }

    /// Pending wires of a callable's results, then combined (`each` and `eachpoint`).
    fn finish(&self, res: Vec<Shape>, mode: Combine, clean: bool) -> R {
        for r in &res {
            if r.shape_type() == ShapeType::Wire && !r.for_construction {
                self.w.add_pending_wire(r.clone());
            }
        }
        ok(self.w.combine_with_base(res, mode, clean).py()?)
    }
}

#[pymethods]
impl PyWorkplane {
    #[new]
    #[pyo3(signature = (inPlane=None, origin=None, obj=None))]
    #[allow(non_snake_case)]
    fn new(
        inPlane: Option<&Bound<'_, PyAny>>,
        origin: Option<&Bound<'_, PyAny>>,
        obj: Option<&Bound<'_, PyAny>>,
    ) -> R {
        let o = v3_opt(origin)?.unwrap_or_default();
        let mut obj = obj.filter(|x| !x.is_none()).cloned();
        let plane = match inPlane {
            None => Plane::named("XY", o).py()?,
            Some(p) if p.is_none() => Plane::named("XY", o).py()?,
            Some(p) => {
                if let Ok(pl) = p.cast::<PyPlane>() {
                    pl.borrow().p
                } else if let Ok(s) = p.extract::<String>() {
                    Plane::named(&s, o).py()?
                } else if p.cast::<PyShape>().is_ok()
                    || p.cast::<PyLocation>().is_ok()
                    || p.cast::<crate::geom::PyVector>().is_ok()
                {
                    obj = Some(p.clone());
                    Plane::named("XY", o).py()?
                } else {
                    return Err(PyValueError::new_err(format!(
                        "Provided value {} is not a valid work plane",
                        p.repr()?
                    )));
                }
            }
        };
        let mut w = Workplane::new(plane);
        if let Some(ob) = obj
            && ob.is_truthy()?
        {
            w.objects.push(py_obj(&ob)?);
        }
        ok(w)
    }

    // ------------------------------------------------------------ the stack

    #[getter]
    fn objects(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.objs_list(py)
    }
    #[setter]
    fn set_objects(&mut self, v: &Bound<'_, PyAny>) -> PyResult<()> {
        self.w.objects = v
            .try_iter()?
            .map(|o| py_obj(&o?))
            .collect::<PyResult<_>>()?;
        Ok(())
    }
    #[getter]
    fn plane(&self) -> PyPlane {
        PyPlane { p: self.w.plane }
    }
    #[setter]
    fn set_plane(&mut self, p: PyRef<'_, PyPlane>) {
        self.w.plane = p.p;
    }
    #[getter]
    fn parent(&self) -> Option<PyWorkplane> {
        self.w.parent.as_ref().map(|p| PyWorkplane {
            w: p.as_ref().clone(),
        })
    }
    #[getter]
    fn ctx(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    #[getter(pendingWires)]
    fn pending_wires(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        crate::conv::shape_list(py, self.w.ctx().pending_wires.clone())
    }
    #[getter(pendingEdges)]
    fn pending_edges(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        crate::conv::shape_list(py, self.w.ctx().pending_edges.clone())
    }
    #[setter(pendingWires)]
    fn set_pending_wires(&self, v: &Bound<'_, PyAny>) -> PyResult<()> {
        self.w.ctx().pending_wires = shapes(v)?;
        Ok(())
    }
    #[setter(pendingEdges)]
    fn set_pending_edges(&self, v: &Bound<'_, PyAny>) -> PyResult<()> {
        self.w.ctx().pending_edges = shapes(v)?;
        Ok(())
    }
    #[getter(firstPoint)]
    fn first_point(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.w.ctx().first_point {
            Some(p) => vector(py, p),
            None => Ok(py.None()),
        }
    }
    #[setter(firstPoint)]
    fn set_first_point(&self, v: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.w.ctx().first_point = v3_opt(v)?;
        Ok(())
    }
    #[getter]
    fn tolerance(&self) -> f64 {
        1e-4
    }
    #[pyo3(name = "popPendingWires", signature = (errorOnEmpty=true))]
    #[allow(non_snake_case)]
    fn pop_pending_wires(&self, py: Python<'_>, errorOnEmpty: bool) -> PyResult<Py<PyAny>> {
        let w = std::mem::take(&mut self.w.ctx().pending_wires);
        if errorOnEmpty && w.is_empty() {
            return Err(PyValueError::new_err("No pending wires present"));
        }
        crate::conv::shape_list(py, w)
    }
    #[pyo3(name = "popPendingEdges", signature = (errorOnEmpty=true))]
    #[allow(non_snake_case)]
    fn pop_pending_edges(&self, py: Python<'_>, errorOnEmpty: bool) -> PyResult<Py<PyAny>> {
        let w = std::mem::take(&mut self.w.ctx().pending_edges);
        if errorOnEmpty && w.is_empty() {
            return Err(PyValueError::new_err("No pending edges present"));
        }
        crate::conv::shape_list(py, w)
    }
    fn tag(&self, name: &str) -> PyWorkplane {
        PyWorkplane {
            w: self.w.tagged(name),
        }
    }
    fn val(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        obj_py(py, &self.w.val())
    }
    fn vals(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.objs_list(py)
    }
    fn size(&self) -> usize {
        self.w.objects.len()
    }
    fn all(&self) -> Vec<PyWorkplane> {
        self.w
            .objects
            .iter()
            .map(|o| PyWorkplane {
                w: self.w.new_object(vec![o.clone()]),
            })
            .collect()
    }
    fn add(slf: PyRefMut<'_, Self>, obj: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let mut slf = slf;
        if let Ok(w) = obj.cast::<PyWorkplane>() {
            let o = w.borrow().w.clone();
            slf.w.objects.extend(o.objects.iter().cloned());
            slf.w.merge_tags(&o);
        } else if let Ok(l) = obj.cast::<PyList>() {
            for x in l.iter() {
                let o = py_obj(&x)?;
                slf.w.objects.push(o);
            }
        } else {
            let o = py_obj(obj)?;
            slf.w.objects.push(o);
        }
        Ok(slf.into_pyobject(obj.py())?.into_any().unbind())
    }
    #[pyo3(name = "newObject")]
    fn new_object(&self, objlist: &Bound<'_, PyAny>) -> R {
        let v = objlist
            .try_iter()?
            .map(|o| py_obj(&o?))
            .collect::<PyResult<_>>()?;
        ok(self.w.new_object(v))
    }
    fn first(&self) -> R {
        ok(self.w.first())
    }
    fn item(&self, i: i64) -> R {
        ok(self.w.item(i).py()?)
    }
    fn last(&self) -> R {
        ok(self.w.last().py()?)
    }
    #[pyo3(signature = (n=1))]
    fn end(&self, n: usize) -> R {
        ok(self.w.end(n).py()?)
    }
    #[pyo3(name = "findSolid", signature = (searchStack=true, searchParents=true))]
    #[allow(non_snake_case)]
    fn find_solid(
        &self,
        py: Python<'_>,
        searchStack: bool,
        searchParents: bool,
    ) -> PyResult<Py<PyAny>> {
        match self.w.find_solid_opt(searchStack, searchParents) {
            Some(s) => wrap(py, s),
            None => Err(PyValueError::new_err(format!(
                "Cannot find a solid {}in the parent chain",
                if searchStack { "on the stack or " } else { "" }
            ))),
        }
    }
    #[pyo3(name = "_findType", signature = (types, searchStack=true, searchParents=true))]
    #[allow(non_snake_case)]
    fn find_type(
        &self,
        py: Python<'_>,
        types: &Bound<'_, PyAny>,
        searchStack: bool,
        searchParents: bool,
    ) -> PyResult<Py<PyAny>> {
        let _ = types;
        match self.w.find_solid_opt(searchStack, searchParents) {
            Some(s) => wrap(py, s),
            None => Ok(py.None()),
        }
    }
    #[pyo3(name = "largestDimension")]
    fn largest_dimension(&self) -> PyResult<f64> {
        self.w.largest_dimension().py()
    }
    fn __len__(&self) -> usize {
        self.w.objects.len()
    }
    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut v = Vec::new();
        for o in &self.w.objects {
            if let Obj::Shape(s) = o {
                if s.shape_type() == ShapeType::Compound {
                    for c in s.children() {
                        v.push(wrap(py, c)?);
                    }
                } else {
                    v.push(wrap(py, s.clone())?);
                }
            }
        }
        Ok(PyList::new(py, v)?.try_iter()?.into_any().unbind())
    }
    fn __getitem__(&self, item: &Bound<'_, PyAny>) -> R {
        let n = self.w.objects.len() as isize;
        if let Ok(i) = item.extract::<isize>() {
            let j = if i < 0 { n + i } else { i };
            if j < 0 || j >= n {
                return Err(PyIndexError::new_err("list index out of range"));
            }
            return ok(self.w.new_object(vec![self.w.objects[j as usize].clone()]));
        }
        if let Ok(s) = item.cast::<PySlice>() {
            let ind = s.indices(n)?;
            let mut v = Vec::new();
            let mut i = ind.start;
            while (ind.step > 0 && i < ind.stop) || (ind.step < 0 && i > ind.stop) {
                v.push(self.w.objects[i as usize].clone());
                i += ind.step;
            }
            return ok(self.w.new_object(v));
        }
        let mut v = Vec::new();
        for i in item.try_iter()? {
            let i: isize = i?.extract()?;
            let j = if i < 0 { n + i } else { i };
            v.push(
                self.w
                    .objects
                    .get(j as usize)
                    .cloned()
                    .ok_or_else(|| PyIndexError::new_err("list index out of range"))?,
            );
        }
        ok(self.w.new_object(v))
    }

    // ------------------------------------------------------------ selection

    #[pyo3(signature = (selector=None, tag=None))]
    fn vertices(
        &self,
        py: Python<'_>,
        selector: Option<&Bound<'_, PyAny>>,
        tag: Option<&str>,
    ) -> R {
        self.select(py, ShapeType::Vertex, selector, tag)
    }
    #[pyo3(signature = (selector=None, tag=None))]
    fn faces(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>, tag: Option<&str>) -> R {
        self.select(py, ShapeType::Face, selector, tag)
    }
    #[pyo3(signature = (selector=None, tag=None))]
    fn edges(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>, tag: Option<&str>) -> R {
        self.select(py, ShapeType::Edge, selector, tag)
    }
    #[pyo3(signature = (selector=None, tag=None))]
    fn wires(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>, tag: Option<&str>) -> R {
        self.select(py, ShapeType::Wire, selector, tag)
    }
    #[pyo3(signature = (selector=None, tag=None))]
    fn solids(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>, tag: Option<&str>) -> R {
        self.select(py, ShapeType::Solid, selector, tag)
    }
    #[pyo3(signature = (selector=None, tag=None))]
    fn shells(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>, tag: Option<&str>) -> R {
        self.select(py, ShapeType::Shell, selector, tag)
    }
    #[pyo3(signature = (selector=None, tag=None))]
    fn compounds(
        &self,
        py: Python<'_>,
        selector: Option<&Bound<'_, PyAny>>,
        tag: Option<&str>,
    ) -> R {
        self.select(py, ShapeType::Compound, selector, tag)
    }

    // ------------------------------------------------------------ planes

    #[pyo3(signature = (offset=0.0, invert=false, centerOption="ProjectedOrigin", origin=None))]
    #[allow(non_snake_case)]
    fn workplane(
        &self,
        offset: f64,
        invert: bool,
        centerOption: &str,
        origin: Option<&Bound<'_, PyAny>>,
    ) -> R {
        ok(self
            .w
            .workplane(offset, invert, centerOption, v3_opt(origin)?)
            .py()?)
    }
    #[pyo3(name = "copyWorkplane")]
    fn copy_workplane(&self, obj: PyRef<'_, PyWorkplane>) -> R {
        let mut out = obj.w.clone();
        out.parent = Some(std::sync::Arc::new(self.w.clone()));
        out.ctx = self.w.ctx.clone();
        ok(out)
    }
    #[pyo3(name = "workplaneFromTagged")]
    fn workplane_from_tagged(&self, name: &str) -> R {
        ok(self.w.workplane_from_tagged(name).py()?)
    }
    #[pyo3(signature = (rotate=None, offset=None))]
    fn transformed(
        &self,
        rotate: Option<&Bound<'_, PyAny>>,
        offset: Option<&Bound<'_, PyAny>>,
    ) -> R {
        ok(self
            .w
            .transformed(
                v3_opt(rotate)?.unwrap_or_default(),
                v3_opt(offset)?.unwrap_or_default(),
            )
            .py()?)
    }
    fn center(&self, x: f64, y: f64) -> R {
        ok(self.w.center(x, y))
    }
    #[pyo3(name = "pushPoints")]
    fn push_points(&self, pntList: &Bound<'_, PyAny>) -> R {
        let mut v = Vec::new();
        for p in pntList.try_iter()? {
            let p = p?;
            v.push(match p.cast::<PyLocation>() {
                Ok(l) => Obj::Location(l.borrow().l),
                Err(_) => Obj::Vector(v3(&p)?),
            });
        }
        ok(self.w.push_points(v))
    }
    #[pyo3(signature = (xSpacing, ySpacing, xCount, yCount, center=None))]
    #[allow(non_snake_case)]
    fn rarray(
        &self,
        xSpacing: f64,
        ySpacing: f64,
        xCount: i64,
        yCount: i64,
        center: Option<&Bound<'_, PyAny>>,
    ) -> R {
        ok(self
            .w
            .rarray(xSpacing, ySpacing, xCount, yCount, centered2(center)?)
            .py()?)
    }
    #[pyo3(name = "polarArray", signature = (radius, startAngle, angle, count, fill=true, rotate=true))]
    #[allow(non_snake_case)]
    fn polar_array(
        &self,
        radius: f64,
        startAngle: f64,
        angle: f64,
        count: i64,
        fill: bool,
        rotate: bool,
    ) -> R {
        ok(self
            .w
            .polar_array(radius, startAngle, angle, count, fill, rotate)
            .py()?)
    }

    // ------------------------------------------------------------ 2D

    #[pyo3(name = "moveTo", signature = (x=0.0, y=0.0))]
    fn move_to(&self, x: f64, y: f64) -> R {
        ok(self.w.move_to(x, y))
    }
    #[pyo3(name = "move", signature = (xDist=0.0, yDist=0.0))]
    #[allow(non_snake_case)]
    fn move_(&self, xDist: f64, yDist: f64) -> R {
        ok(self.w.move_by(xDist, yDist).py()?)
    }
    #[pyo3(name = "lineTo", signature = (x, y, forConstruction=false))]
    #[allow(non_snake_case)]
    fn line_to(&self, x: f64, y: f64, forConstruction: bool) -> R {
        ok(self.w.line_to(x, y, forConstruction).py()?)
    }
    #[pyo3(signature = (xDist, yDist, forConstruction=false))]
    #[allow(non_snake_case)]
    fn line(&self, xDist: f64, yDist: f64, forConstruction: bool) -> R {
        ok(self.w.line(xDist, yDist, forConstruction).py()?)
    }
    #[pyo3(name = "vLine", signature = (distance, forConstruction=false))]
    #[allow(non_snake_case)]
    fn vline(&self, distance: f64, forConstruction: bool) -> R {
        ok(self.w.line(0.0, distance, forConstruction).py()?)
    }
    #[pyo3(name = "hLine", signature = (distance, forConstruction=false))]
    #[allow(non_snake_case)]
    fn hline(&self, distance: f64, forConstruction: bool) -> R {
        ok(self.w.line(distance, 0.0, forConstruction).py()?)
    }
    #[pyo3(name = "vLineTo", signature = (yCoord, forConstruction=false))]
    #[allow(non_snake_case)]
    fn vline_to(&self, yCoord: f64, forConstruction: bool) -> R {
        ok(self.w.vline_to(yCoord, forConstruction).py()?)
    }
    #[pyo3(name = "hLineTo", signature = (xCoord, forConstruction=false))]
    #[allow(non_snake_case)]
    fn hline_to(&self, xCoord: f64, forConstruction: bool) -> R {
        ok(self.w.hline_to(xCoord, forConstruction).py()?)
    }
    #[pyo3(name = "polarLine", signature = (distance, angle, forConstruction=false))]
    #[allow(non_snake_case)]
    fn polar_line(&self, distance: f64, angle: f64, forConstruction: bool) -> R {
        ok(self.w.polar_line(distance, angle, forConstruction).py()?)
    }
    #[pyo3(name = "polarLineTo", signature = (distance, angle, forConstruction=false))]
    #[allow(non_snake_case)]
    fn polar_line_to(&self, distance: f64, angle: f64, forConstruction: bool) -> R {
        ok(self
            .w
            .polar_line_to(distance, angle, forConstruction)
            .py()?)
    }
    #[pyo3(signature = (listOfXYTuple, forConstruction=false, includeCurrent=false))]
    #[allow(non_snake_case)]
    fn polyline(
        &self,
        listOfXYTuple: &Bound<'_, PyAny>,
        forConstruction: bool,
        includeCurrent: bool,
    ) -> R {
        ok(self
            .w
            .polyline(&points(listOfXYTuple)?, forConstruction, includeCurrent)
            .py()?)
    }
    fn close(&self) -> R {
        ok(self.w.close().py()?)
    }
    #[pyo3(signature = (forConstruction=false))]
    #[allow(non_snake_case)]
    fn wire(&self, forConstruction: bool) -> R {
        ok(self.w.wire(forConstruction).py()?)
    }
    #[pyo3(name = "consolidateWires")]
    fn consolidate_wires(&self) -> R {
        ok(self.w.consolidate_wires())
    }
    #[pyo3(name = "toPending")]
    fn to_pending(&self) -> R {
        ok(self.w.to_pending())
    }
    #[pyo3(name = "threePointArc", signature = (point1, point2, forConstruction=false))]
    #[allow(non_snake_case)]
    fn three_point_arc(
        &self,
        point1: &Bound<'_, PyAny>,
        point2: &Bound<'_, PyAny>,
        forConstruction: bool,
    ) -> R {
        ok(self
            .w
            .three_point_arc(v3(point1)?, v3(point2)?, forConstruction)
            .py()?)
    }
    #[pyo3(name = "sagittaArc", signature = (endPoint, sag, forConstruction=false))]
    #[allow(non_snake_case)]
    fn sagitta_arc(&self, endPoint: &Bound<'_, PyAny>, sag: f64, forConstruction: bool) -> R {
        ok(self
            .w
            .sagitta_arc(v3(endPoint)?, sag, forConstruction)
            .py()?)
    }
    #[pyo3(name = "radiusArc", signature = (endPoint, radius, forConstruction=false))]
    #[allow(non_snake_case)]
    fn radius_arc(&self, endPoint: &Bound<'_, PyAny>, radius: f64, forConstruction: bool) -> R {
        ok(self
            .w
            .radius_arc(v3(endPoint)?, radius, forConstruction)
            .py()?)
    }
    #[pyo3(name = "tangentArcPoint", signature = (endpoint, forConstruction=false, relative=true))]
    #[allow(non_snake_case)]
    fn tangent_arc_point(
        &self,
        endpoint: &Bound<'_, PyAny>,
        forConstruction: bool,
        relative: bool,
    ) -> R {
        ok(self
            .w
            .tangent_arc_point(v3(endpoint)?, forConstruction, relative)
            .py()?)
    }
    #[pyo3(signature = (listOfXYTuple, forConstruction=false, includeCurrent=false, makeWire=false))]
    #[allow(non_snake_case)]
    fn bezier(
        &self,
        listOfXYTuple: &Bound<'_, PyAny>,
        forConstruction: bool,
        includeCurrent: bool,
        makeWire: bool,
    ) -> R {
        ok(self
            .w
            .bezier(
                &points(listOfXYTuple)?,
                forConstruction,
                includeCurrent,
                makeWire,
            )
            .py()?)
    }
    #[pyo3(signature = (listOfXYTuple, tangents=None, periodic=false, parameters=None, scale=true, tol=None, forConstruction=false, includeCurrent=false, makeWire=false))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn spline(
        &self,
        listOfXYTuple: &Bound<'_, PyAny>,
        tangents: Option<&Bound<'_, PyAny>>,
        periodic: bool,
        parameters: Option<Vec<f64>>,
        scale: bool,
        tol: Option<f64>,
        forConstruction: bool,
        includeCurrent: bool,
        makeWire: bool,
    ) -> R {
        let _ = tol;
        let tg = crate::shapes::tangents_arg(tangents)?;
        ok(self
            .w
            .spline(
                &points(listOfXYTuple)?,
                tg.as_deref(),
                periodic,
                parameters.as_deref(),
                scale,
                forConstruction,
                includeCurrent,
                makeWire,
            )
            .py()?)
    }
    #[pyo3(name = "splineApprox", signature = (points, tol=1e-6, minDeg=1, maxDeg=6, smoothing=None, forConstruction=false, includeCurrent=false, makeWire=false))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn spline_approx(
        &self,
        points: &Bound<'_, PyAny>,
        tol: f64,
        minDeg: i64,
        maxDeg: i64,
        smoothing: Option<&Bound<'_, PyAny>>,
        forConstruction: bool,
        includeCurrent: bool,
        makeWire: bool,
    ) -> R {
        let _ = (minDeg, maxDeg, smoothing);
        ok(self
            .w
            .spline_approx(
                &crate::conv::points(points)?,
                tol,
                forConstruction,
                includeCurrent,
                makeWire,
            )
            .py()?)
    }
    #[pyo3(name = "parametricCurve", signature = (func, N=400, start=0.0, stop=1.0, tol=1e-6, minDeg=1, maxDeg=6, smoothing=None, makeWire=true))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn parametric_curve(
        &self,
        func: &Bound<'_, PyAny>,
        N: usize,
        start: f64,
        stop: f64,
        tol: f64,
        minDeg: i64,
        maxDeg: i64,
        smoothing: Option<&Bound<'_, PyAny>>,
        makeWire: bool,
    ) -> R {
        let _ = (minDeg, maxDeg, smoothing);
        let diff = stop - start;
        let mut pts = Vec::with_capacity(N + 1);
        for t in 0..=N {
            pts.push(v3(&func.call1((start + diff * t as f64 / N as f64,))?)?);
        }
        // (Always pending, construction or not, as CadQuery's.)
        ok(self
            .w
            .spline_approx(&pts, tol, false, false, makeWire)
            .py()?)
    }
    #[pyo3(name = "ellipseArc", signature = (x_radius, y_radius, angle1=360.0, angle2=360.0, rotation_angle=0.0, sense=1, forConstruction=false, startAtCurrent=true, makeWire=false))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn ellipse_arc(
        &self,
        x_radius: f64,
        y_radius: f64,
        angle1: f64,
        angle2: f64,
        rotation_angle: f64,
        sense: i64,
        forConstruction: bool,
        startAtCurrent: bool,
        makeWire: bool,
    ) -> R {
        ok(self
            .w
            .ellipse_arc(
                x_radius,
                y_radius,
                angle1,
                angle2,
                rotation_angle,
                sense,
                forConstruction,
                startAtCurrent,
                makeWire,
            )
            .py()?)
    }
    #[pyo3(signature = (xLen, yLen, centered=None, forConstruction=false))]
    #[allow(non_snake_case)]
    fn rect(
        &self,
        xLen: f64,
        yLen: f64,
        centered: Option<&Bound<'_, PyAny>>,
        forConstruction: bool,
    ) -> R {
        ok(self
            .w
            .rect(xLen, yLen, centered2(centered)?, forConstruction)
            .py()?)
    }
    #[pyo3(signature = (radius, forConstruction=false))]
    #[allow(non_snake_case)]
    fn circle(&self, radius: f64, forConstruction: bool) -> R {
        ok(self.w.circle(radius, forConstruction).py()?)
    }
    #[pyo3(signature = (x_radius, y_radius, rotation_angle=0.0, forConstruction=false))]
    #[allow(non_snake_case)]
    fn ellipse(
        &self,
        x_radius: f64,
        y_radius: f64,
        rotation_angle: f64,
        forConstruction: bool,
    ) -> R {
        ok(self
            .w
            .ellipse(x_radius, y_radius, rotation_angle, forConstruction)
            .py()?)
    }
    #[pyo3(signature = (nSides, diameter, forConstruction=false, circumscribed=false))]
    #[allow(non_snake_case)]
    fn polygon(
        &self,
        nSides: &Bound<'_, PyAny>,
        diameter: f64,
        forConstruction: bool,
        circumscribed: bool,
    ) -> R {
        let n = num(nSides)? as i64;
        ok(self
            .w
            .polygon(n, diameter, forConstruction, circumscribed)
            .py()?)
    }
    #[pyo3(name = "slot2D", signature = (length, diameter, angle=0.0))]
    fn slot2d(&self, length: f64, diameter: f64, angle: f64) -> R {
        ok(self.w.slot2d(length, diameter, angle).py()?)
    }
    #[pyo3(name = "mirrorY")]
    fn mirror_y(&self) -> R {
        ok(self.w.mirror_axis(true).py()?)
    }
    #[pyo3(name = "mirrorX")]
    fn mirror_x(&self) -> R {
        ok(self.w.mirror_axis(false).py()?)
    }
    #[pyo3(name = "offset2D", signature = (d, kind="arc", forConstruction=false))]
    #[allow(non_snake_case)]
    fn offset2d(&self, d: f64, kind: &str, forConstruction: bool) -> R {
        ok(self.w.offset2d(d, kind, forConstruction).py()?)
    }

    // ------------------------------------------------------------ callables

    #[pyo3(signature = (callback, useLocalCoordinates=false, combine=None, clean=true))]
    #[allow(non_snake_case)]
    fn each(
        &self,
        py: Python<'_>,
        callback: &Bound<'_, PyAny>,
        useLocalCoordinates: bool,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        let mode = self::combine(combine, Combine::Fuse)?;
        let mut res = Vec::new();
        for o in &self.w.objects {
            let arg = if useLocalCoordinates {
                match o {
                    Obj::Shape(s) => wrap(py, s.transformed(&self.w.plane.to_local_map()).py()?)?,
                    Obj::Vector(v) => vector(py, self.w.plane.to_local(*v))?,
                    o => obj_py(py, o)?,
                }
            } else {
                obj_py(py, o)?
            };
            let r = callback.call1((arg,))?;
            let mut s = shape(&r)?;
            if useLocalCoordinates {
                s = s.transformed(&self.w.plane.to_world_map()).py()?;
            }
            res.push(s);
        }
        self.finish(res, mode, clean)
    }
    #[pyo3(signature = (arg, useLocalCoordinates=false, combine=None, clean=true))]
    #[allow(non_snake_case)]
    fn eachpoint(
        &self,
        py: Python<'_>,
        arg: &Bound<'_, PyAny>,
        useLocalCoordinates: bool,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        let mode = self::combine(combine, Combine::No)?;
        let locs = self.w.locations();
        let loc = self.w.plane.location();
        let mut res = Vec::new();
        let bases: Option<Vec<Shape>> = if let Ok(w) = arg.cast::<PyWorkplane>() {
            Some(w.borrow().w.shapes())
        } else if let Ok(s) = arg.cast::<PyShape>() {
            Some(vec![s.borrow().s.clone()])
        } else {
            None
        };
        match bases {
            Some(bs) => {
                for b in &bs {
                    for p in &locs {
                        res.push(if useLocalCoordinates {
                            b.moved(p).py()?.moved(&loc).py()?
                        } else {
                            b.moved(&loc.then_after(p)).py()?
                        });
                    }
                }
            }
            None => {
                if !arg.is_callable() {
                    return Err(PyValueError::new_err(format!(
                        "{} is not supported",
                        arg.repr()?
                    )));
                }
                for p in &locs {
                    let l = if useLocalCoordinates {
                        *p
                    } else {
                        loc.then_after(p)
                    };
                    let r = arg.call1((Py::new(py, PyLocation { l })?,))?;
                    let s = shape(&r)?;
                    res.push(if useLocalCoordinates {
                        s.moved(&loc).py()?
                    } else {
                        s
                    });
                }
            }
        }
        self.finish(res, mode, clean)
    }
    #[pyo3(name = "cutEach", signature = (fcn, useLocalCoords=false, clean=true))]
    #[allow(non_snake_case)]
    fn cut_each(
        &self,
        py: Python<'_>,
        fcn: &Bound<'_, PyAny>,
        useLocalCoords: bool,
        clean: bool,
    ) -> R {
        let solid = self.w.find_solid().py()?;
        let r = self.eachpoint(py, fcn, useLocalCoords, None, false)?;
        let mut s = cadrs_kernel::cadquery::ops::cut(&solid, &r.w.shapes()).py()?;
        if clean {
            s = cadrs_kernel::cadquery::ops::clean(&s).py()?;
        }
        ok(self.w.new_object(vec![Obj::Shape(s)]))
    }
    fn filter(&self, f: &Bound<'_, PyAny>) -> R {
        let mut v = Vec::new();
        for o in &self.w.objects {
            if f.call1((obj_py(f.py(), o)?,))?.is_truthy()? {
                v.push(o.clone());
            }
        }
        ok(self.w.new_object(v))
    }
    fn map(&self, f: &Bound<'_, PyAny>) -> R {
        let v = self
            .w
            .objects
            .iter()
            .map(|o| py_obj(&f.call1((obj_py(f.py(), o)?,))?))
            .collect::<PyResult<_>>()?;
        ok(self.w.new_object(v))
    }
    fn apply(&self, py: Python<'_>, f: &Bound<'_, PyAny>) -> R {
        let r = f.call1((self.objs_list(py)?,))?;
        let v = r
            .try_iter()?
            .map(|o| py_obj(&o?))
            .collect::<PyResult<_>>()?;
        ok(self.w.new_object(v))
    }
    fn sort(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> R {
        let l = self.objs_list(py)?;
        let kw = PyDict::new(py);
        kw.set_item("key", key)?;
        let sorted = py
            .import("builtins")?
            .getattr("sorted")?
            .call((l,), Some(&kw))?;
        let v = sorted
            .try_iter()?
            .map(|o| py_obj(&o?))
            .collect::<PyResult<_>>()?;
        ok(self.w.new_object(v))
    }
    fn invoke(slf: &Bound<'_, Self>, f: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let inspect = py.import("inspect")?;
        let n = inspect
            .call_method1("signature", (f,))?
            .getattr("parameters")?
            .len()?;
        if n == 0 {
            f.call0()?;
            return Ok(slf.clone().into_any().unbind());
        }
        if n == 1 {
            let r = f.call1((slf,))?;
            return Ok(if r.is_none() {
                slf.clone().into_any().unbind()
            } else {
                r.unbind()
            });
        }
        Err(PyValueError::new_err(
            "Provided function accepts too many arguments",
        ))
    }

    // ------------------------------------------------------------ 3D

    #[pyo3(signature = (until, combine=None, clean=true, both=false, taper=None))]
    fn extrude(
        &self,
        until: &Bound<'_, PyAny>,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
        both: bool,
        taper: Option<f64>,
    ) -> R {
        let mode = self::combine(combine, Combine::Fuse)?;
        ok(self
            .w
            .extrude(until_arg(until)?, mode, clean, both, taper.unwrap_or(0.0))
            .py()?)
    }
    #[pyo3(name = "cutBlind", signature = (until, clean=true, both=None, taper=None))]
    fn cut_blind(
        &self,
        until: &Bound<'_, PyAny>,
        clean: bool,
        both: Option<&Bound<'_, PyAny>>,
        taper: Option<f64>,
    ) -> R {
        // (A float in `both` is the old signature's taper.)
        let (both, taper) = match both {
            None => (false, taper),
            Some(b) if b.is_instance_of::<pyo3::types::PyBool>() => (b.is_truthy()?, taper),
            Some(b) if b.is_none() => (false, taper),
            Some(b) => (false, Some(num(b)?)),
        };
        ok(self
            .w
            .cut_blind(until_arg(until)?, clean, both, taper.unwrap_or(0.0))
            .py()?)
    }
    #[pyo3(name = "cutThruAll", signature = (clean=true, taper=0.0))]
    fn cut_thru_all(&self, clean: bool, taper: f64) -> R {
        ok(self.w.cut_thru_all(clean, taper).py()?)
    }
    #[pyo3(name = "twistExtrude", signature = (distance, angleDegrees, combine=None, clean=true))]
    #[allow(non_snake_case)]
    fn twist_extrude(
        &self,
        distance: f64,
        angleDegrees: f64,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        ok(self
            .w
            .twist_extrude(
                distance,
                angleDegrees,
                self::combine(combine, Combine::Fuse)?,
                clean,
            )
            .py()?)
    }
    #[pyo3(signature = (angleDegrees=360.0, axisStart=None, axisEnd=None, combine=None, clean=true))]
    #[allow(non_snake_case)]
    fn revolve(
        &self,
        angleDegrees: f64,
        axisStart: Option<&Bound<'_, PyAny>>,
        axisEnd: Option<&Bound<'_, PyAny>>,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        ok(self
            .w
            .revolve(
                angleDegrees,
                v3_opt(axisStart)?,
                v3_opt(axisEnd)?,
                self::combine(combine, Combine::Fuse)?,
                clean,
            )
            .py()?)
    }
    #[pyo3(signature = (path, multisection=false, sweepAlongWires=None, makeSolid=true, isFrenet=false, combine=None, clean=true, transition="right", normal=None, auxSpine=None))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn sweep(
        &self,
        path: &Bound<'_, PyAny>,
        multisection: bool,
        sweepAlongWires: Option<bool>,
        makeSolid: bool,
        isFrenet: bool,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
        transition: &str,
        normal: Option<&Bound<'_, PyAny>>,
        auxSpine: Option<&Bound<'_, PyAny>>,
    ) -> R {
        let _ = auxSpine;
        let multisection = sweepAlongWires.unwrap_or(multisection);
        let p = if let Ok(w) = path.cast::<PyWorkplane>() {
            match w.borrow().w.wire(false).py()?.val() {
                Obj::Shape(s) => s,
                _ => return Err(PyValueError::new_err("Wire or Edge instance required")),
            }
        } else {
            shape(path)?
        };
        ok(self
            .w
            .sweep(
                &p,
                multisection,
                makeSolid,
                isFrenet,
                self::combine(combine, Combine::Fuse)?,
                clean,
                transition,
                v3_opt(normal)?,
            )
            .py()?)
    }
    #[pyo3(signature = (ruled=false, combine=None, clean=true))]
    fn loft(&self, ruled: bool, combine: Option<&Bound<'_, PyAny>>, clean: bool) -> R {
        ok(self
            .w
            .loft(ruled, self::combine(combine, Combine::Fuse)?, clean)
            .py()?)
    }
    #[pyo3(name = "box", signature = (length, width, height, centered=None, combine=None, clean=true))]
    fn box_(
        &self,
        length: f64,
        width: f64,
        height: f64,
        centered: Option<&Bound<'_, PyAny>>,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        ok(self
            .w
            .box_(
                length,
                width,
                height,
                centered3(centered, true)?,
                self::combine(combine, Combine::Fuse)?,
                clean,
            )
            .py()?)
    }
    #[pyo3(signature = (radius, direct=None, angle1=-90.0, angle2=90.0, angle3=360.0, centered=None, combine=None, clean=true))]
    #[allow(clippy::too_many_arguments)]
    fn sphere(
        &self,
        radius: f64,
        direct: Option<&Bound<'_, PyAny>>,
        angle1: f64,
        angle2: f64,
        angle3: f64,
        centered: Option<&Bound<'_, PyAny>>,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        let d = v3_opt(direct)?.unwrap_or(V3::z());
        ok(self
            .w
            .sphere(
                radius,
                d,
                angle1,
                angle2,
                angle3,
                centered3(centered, true)?,
                self::combine(combine, Combine::Fuse)?,
                clean,
            )
            .py()?)
    }
    #[pyo3(signature = (height, radius, direct=None, angle=360.0, centered=None, combine=None, clean=true))]
    #[allow(clippy::too_many_arguments)]
    fn cylinder(
        &self,
        height: f64,
        radius: f64,
        direct: Option<&Bound<'_, PyAny>>,
        angle: f64,
        centered: Option<&Bound<'_, PyAny>>,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        let d = v3_opt(direct)?.unwrap_or(V3::z());
        ok(self
            .w
            .cylinder(
                height,
                radius,
                d,
                angle,
                centered3(centered, true)?,
                self::combine(combine, Combine::Fuse)?,
                clean,
            )
            .py()?)
    }
    #[pyo3(signature = (dx, dy, dz, xmin, zmin, xmax, zmax, pnt=None, dir=None, centered=None, combine=None, clean=true))]
    #[allow(clippy::too_many_arguments)]
    fn wedge(
        &self,
        dx: f64,
        dy: f64,
        dz: f64,
        xmin: f64,
        zmin: f64,
        xmax: f64,
        zmax: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        centered: Option<&Bound<'_, PyAny>>,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        ok(self
            .w
            .wedge(
                V3::new(dx, dy, dz),
                xmin,
                zmin,
                xmax,
                zmax,
                p,
                d,
                centered3(centered, true)?,
                self::combine(combine, Combine::Fuse)?,
                clean,
            )
            .py()?)
    }
    #[pyo3(signature = (diameter, depth=None, clean=true))]
    fn hole(&self, diameter: f64, depth: Option<f64>, clean: bool) -> R {
        ok(self.w.hole(diameter, depth, clean).py()?)
    }
    #[pyo3(name = "cboreHole", signature = (diameter, cboreDiameter, cboreDepth, depth=None, clean=true))]
    #[allow(non_snake_case)]
    fn cbore_hole(
        &self,
        diameter: f64,
        cboreDiameter: f64,
        cboreDepth: f64,
        depth: Option<f64>,
        clean: bool,
    ) -> R {
        ok(self
            .w
            .cbore_hole(diameter, cboreDiameter, cboreDepth, depth, clean)
            .py()?)
    }
    #[pyo3(name = "cskHole", signature = (diameter, cskDiameter, cskAngle, depth=None, clean=true))]
    #[allow(non_snake_case)]
    fn csk_hole(
        &self,
        diameter: f64,
        cskDiameter: f64,
        cskAngle: f64,
        depth: Option<f64>,
        clean: bool,
    ) -> R {
        ok(self
            .w
            .csk_hole(diameter, cskDiameter, cskAngle, depth, clean)
            .py()?)
    }
    #[pyo3(signature = (thickness, kind="arc"))]
    fn shell(&self, thickness: f64, kind: &str) -> R {
        ok(self.w.shell(thickness, kind).py()?)
    }
    fn fillet(&self, radius: f64) -> R {
        ok(self.w.fillet(radius).py()?)
    }
    #[pyo3(signature = (length, length2=None))]
    fn chamfer(&self, length: f64, length2: Option<f64>) -> R {
        ok(self.w.chamfer(length, length2).py()?)
    }

    // ------------------------------------------------------------ booleans

    #[pyo3(signature = (clean=true, glue=false, tol=None))]
    fn combine(&self, clean: bool, glue: bool, tol: Option<f64>) -> R {
        let _ = (glue, tol);
        ok(self.w.combine(clean).py()?)
    }
    #[pyo3(signature = (toUnion=None, clean=None, glue=false, tol=None))]
    #[allow(non_snake_case)]
    fn union(
        &self,
        toUnion: Option<&Bound<'_, PyAny>>,
        clean: Option<&Bound<'_, PyAny>>,
        glue: bool,
        tol: Option<f64>,
    ) -> R {
        let _ = (glue, tol);
        let clean = truthy(clean, true)?;
        match toUnion {
            None => ok(self.w.union_shapes(vec![], clean).py()?),
            Some(o) if o.is_none() => ok(self.w.union_shapes(vec![], clean).py()?),
            Some(o) => {
                if let Ok(w) = o.cast::<PyWorkplane>() {
                    return ok(self.w.union_wp(&w.borrow().w, clean).py()?);
                }
                let s = shape(o).map_err(|_| {
                    PyValueError::new_err(format!("Cannot union type '{}'", type_name(o)))
                })?;
                ok(self.w.union_shapes(vec![s], clean).py()?)
            }
        }
    }
    fn __add__(&self, o: &Bound<'_, PyAny>) -> R {
        self.union(Some(o), None, false, None)
    }
    fn __or__(&self, o: &Bound<'_, PyAny>) -> R {
        self.union(Some(o), None, false, None)
    }
    #[pyo3(signature = (toCut, clean=None, tol=None))]
    #[allow(non_snake_case)]
    fn cut(
        &self,
        toCut: &Bound<'_, PyAny>,
        clean: Option<&Bound<'_, PyAny>>,
        tol: Option<f64>,
    ) -> R {
        let _ = tol;
        let clean = truthy(clean, true)?;
        let (v, w) = shapes_of(toCut).map_err(|_| {
            PyValueError::new_err(format!("Cannot cut type '{}'", type_name(toCut)))
        })?;
        if let Some(w) = w {
            self.w.merge_tags(&w);
        }
        ok(self.w.cut_shapes(v, clean).py()?)
    }
    fn __sub__(&self, o: &Bound<'_, PyAny>) -> R {
        self.cut(o, None, None)
    }
    #[pyo3(signature = (toIntersect, clean=None, tol=None))]
    #[allow(non_snake_case)]
    fn intersect(
        &self,
        toIntersect: &Bound<'_, PyAny>,
        clean: Option<&Bound<'_, PyAny>>,
        tol: Option<f64>,
    ) -> R {
        let _ = tol;
        let clean = truthy(clean, true)?;
        let (v, w) = shapes_of(toIntersect).map_err(|_| {
            PyValueError::new_err(format!(
                "Cannot intersect type '{}'",
                type_name(toIntersect)
            ))
        })?;
        if let Some(w) = w {
            self.w.merge_tags(&w);
        }
        ok(self.w.intersect_shapes(v, clean).py()?)
    }
    fn __mul__(&self, o: &Bound<'_, PyAny>) -> R {
        self.intersect(o, None, None)
    }
    fn __and__(&self, o: &Bound<'_, PyAny>) -> R {
        self.intersect(o, None, None)
    }
    #[pyo3(signature = (*args, **kw))]
    fn split(&self, args: &Bound<'_, PyTuple>, kw: Option<&Bound<'_, PyDict>>) -> R {
        if args.len() == 1
            && (args.get_item(0)?.cast::<PyWorkplane>().is_ok()
                || args.get_item(0)?.cast::<PyShape>().is_ok())
        {
            let (v, _) = shapes_of(&args.get_item(0)?)?;
            return ok(self.w.split_by(v).py()?);
        }
        let get = |name: &str, i: usize| -> PyResult<bool> {
            if let Some(k) = kw
                && let Some(v) = k.get_item(name)?
            {
                return v.is_truthy();
            }
            if i < args.len() {
                args.get_item(i)?.is_truthy()
            } else {
                Ok(false)
            }
        };
        ok(self
            .w
            .split_keep(get("keepTop", 0)?, get("keepBottom", 1)?)
            .py()?)
    }
    fn __truediv__(&self, o: &Bound<'_, PyAny>) -> R {
        let (v, _) = shapes_of(o)?;
        ok(self.w.split_by(v).py()?)
    }
    fn clean(&self) -> R {
        ok(self.w.clean().py()?)
    }

    // ------------------------------------------------------------ moves

    #[pyo3(name = "rotateAboutCenter")]
    #[allow(non_snake_case)]
    fn rotate_about_center(&self, axisEndPoint: &Bound<'_, PyAny>, angleDegrees: f64) -> R {
        ok(self
            .w
            .rotate_about_center(v3(axisEndPoint)?, angleDegrees)
            .py()?)
    }
    #[allow(non_snake_case)]
    fn rotate(
        &self,
        axisStartPoint: &Bound<'_, PyAny>,
        axisEndPoint: &Bound<'_, PyAny>,
        angleDegrees: f64,
    ) -> R {
        ok(self
            .w
            .rotate(v3(axisStartPoint)?, v3(axisEndPoint)?, angleDegrees)
            .py()?)
    }
    fn translate(&self, vec: &Bound<'_, PyAny>) -> R {
        ok(self.w.translate(v3(vec)?).py()?)
    }
    #[pyo3(signature = (mirrorPlane=None, basePointVector=None, union=false))]
    #[allow(non_snake_case)]
    fn mirror(
        &self,
        mirrorPlane: Option<&Bound<'_, PyAny>>,
        basePointVector: Option<&Bound<'_, PyAny>>,
        union: bool,
    ) -> R {
        let mut face: Option<Shape> = None;
        let n = match mirrorPlane {
            None => V3::z(),
            Some(m) => {
                if let Ok(w) = m.cast::<PyWorkplane>() {
                    match w.borrow().w.val() {
                        Obj::Shape(s) if s.shape_type() == ShapeType::Face => {
                            let n = s.normal_at(None).py()?;
                            face = Some(s);
                            n
                        }
                        _ => return Err(PyValueError::new_err("Face required")),
                    }
                } else if let Ok(s) = m.cast::<PyShape>() {
                    let s = s.borrow().s.clone();
                    let n = s.normal_at(None).py()?;
                    face = Some(s);
                    n
                } else if let Ok(st) = m.extract::<String>() {
                    match st.as_str() {
                        "XY" | "YX" => V3::z(),
                        "XZ" | "ZX" => V3::y(),
                        "YZ" | "ZY" => V3::x(),
                        _ => {
                            return Err(PyValueError::new_err(format!(
                                "Invalid mirror plane {st}"
                            )));
                        }
                    }
                } else {
                    v3(m)?
                }
            }
        };
        let base = match (v3_opt(basePointVector)?, &face) {
            (Some(b), _) => b,
            (None, Some(f)) => f.center(),
            (None, None) => V3::zeros(),
        };
        ok(self.w.mirror(n, base, union).py()?)
    }

    // ------------------------------------------------------------ the rest

    #[pyo3(signature = (txt, fontsize, distance, combine=None, clean=true, font="Arial", fontPath=None, kind="regular", halign="center", valign="center"))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn text(
        &self,
        py: Python<'_>,
        txt: &str,
        fontsize: f64,
        distance: f64,
        combine: Option<&Bound<'_, PyAny>>,
        clean: bool,
        font: &str,
        fontPath: Option<&str>,
        kind: &str,
        halign: &str,
        valign: &str,
    ) -> R {
        let mode = self::combine(combine, Combine::Cut)?;
        let t = py.import("cadquery._text")?.call_method1(
            "make_text",
            (
                txt,
                fontsize,
                distance,
                font,
                fontPath,
                kind,
                halign,
                valign,
                PyPlane { p: self.w.plane },
            ),
        )?;
        let s = shape(&t)?;
        ok(self.w.combine_with_base(vec![s], mode, clean).py()?)
    }
    #[pyo3(signature = (height=0.0))]
    fn section(&self, height: f64) -> R {
        ok(self.w.section(height).py()?)
    }
    #[pyo3(name = "interpPlate", signature = (*args, **kw))]
    fn interp_plate(&self, args: &Bound<'_, PyTuple>, kw: Option<&Bound<'_, PyDict>>) -> R {
        let _ = (args, kw);
        Err(CqError::NotImplemented("interpPlate".into())).py()
    }
    #[pyo3(name = "parametricSurface", signature = (*args, **kw))]
    fn parametric_surface(&self, args: &Bound<'_, PyTuple>, kw: Option<&Bound<'_, PyDict>>) -> R {
        let _ = (args, kw);
        Err(CqError::NotImplemented("parametricSurface".into())).py()
    }
    #[pyo3(signature = (*args, **kw))]
    fn sketch(&self, args: &Bound<'_, PyTuple>, kw: Option<&Bound<'_, PyDict>>) -> R {
        let _ = (args, kw);
        Err(CqError::NotImplemented("Workplane.sketch".into())).py()
    }
    #[pyo3(name = "toOCC")]
    fn to_occ(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        Ok(slf.call_method0("val")?.getattr("wrapped")?.unbind())
    }
    #[pyo3(signature = (fname, tolerance=0.1, angularTolerance=0.1, **kw))]
    #[allow(non_snake_case)]
    fn export(
        slf: &Bound<'_, Self>,
        fname: &str,
        tolerance: f64,
        angularTolerance: f64,
        kw: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let _ = (tolerance, angularTolerance, kw);
        let s = slf.borrow().w.shapes();
        let s = if s.len() == 1 {
            s[0].clone()
        } else {
            Shape::compound(s)
        };
        let bytes = if fname.to_lowercase().ends_with(".stl") {
            cadrs_kernel::cadquery::ops::export_stl(&s)
        } else {
            cadrs_kernel::cadquery::ops::export_step(&s)
        }
        .py()?;
        std::fs::write(fname, bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(slf.clone().into_any().unbind())
    }
    fn __repr__(&self) -> String {
        format!(
            "<cadquery.cq.Workplane object with {} objects>",
            self.w.objects.len()
        )
    }
    fn __copy__(&self) -> Self {
        self.clone()
    }
    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.clone()
    }
    #[pyo3(name = "_combineWithBase", signature = (obj, mode=None, clean=false))]
    fn combine_with_base(
        &self,
        obj: &Bound<'_, PyAny>,
        mode: Option<&Bound<'_, PyAny>>,
        clean: bool,
    ) -> R {
        let v = if obj.cast::<PyShape>().is_ok() {
            vec![shape(obj)?]
        } else {
            shapes(obj)?
        };
        ok(self
            .w
            .combine_with_base(v, self::combine(mode, Combine::Fuse)?, clean)
            .py()?)
    }
    #[pyo3(name = "_addPendingWire")]
    fn add_pending_wire(&self, w: &Bound<'_, PyAny>) -> PyResult<()> {
        self.w.add_pending_wire(shape(w)?);
        Ok(())
    }
    #[pyo3(name = "_addPendingEdge")]
    fn add_pending_edge(&self, e: &Bound<'_, PyAny>) -> PyResult<()> {
        self.w.add_pending_edge(shape(e)?).py()
    }
    #[pyo3(name = "_getFaces")]
    fn get_faces(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        crate::conv::shape_list(py, self.w.get_faces().py()?)
    }
}

/// A flag that may be given any object (its truth, as Python takes it).
fn truthy(o: Option<&Bound<'_, PyAny>>, default: bool) -> PyResult<bool> {
    match o {
        None => Ok(default),
        Some(o) => o.is_truthy(),
    }
}

fn until_arg(o: &Bound<'_, PyAny>) -> PyResult<Until> {
    if let Ok(s) = o.cast::<PyString>() {
        return match s.to_str()? {
            "next" => Ok(Until::Next),
            "last" => Ok(Until::Last),
            other => Err(PyValueError::new_err(format!(
                "Do not know how to handle until argument {other}"
            ))),
        };
    }
    if let Ok(s) = o.cast::<PyShape>() {
        return Ok(Until::Face(s.borrow().s.clone()));
    }
    match o.extract::<f64>() {
        Ok(d) => Ok(Until::Distance(d)),
        Err(_) => Err(PyTypeError::new_err(format!(
            "Do not know how to handle until argument of type {}",
            type_name(o)
        ))),
    }
}

#[allow(dead_code)]
fn unused(_: Location) {}
