//! `Shape` and its kinds (`Vertex`, `Edge`, `Wire`, `Face`, `Shell`, `Solid`, `CompSolid`,
//! `Compound`), with their constructors.

use crate::conv::{
    Py3, V3, err, num, shape, shape_list, shapes, tuple3, type_name, v3, v3_opt, vector,
};
use crate::geom::{PyBoundBox, PyLocation, PyPlane};
use crate::sel::selector_filter;
use cadrs_kernel::cadquery::ops::{self, LoftItem, SweepMode};
use cadrs_kernel::cadquery::{EdgeGeom, Location, Plane, Shape, ShapeType, make};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

/// `cadquery.Shape`.
#[pyclass(name = "Shape", module = "cadquery", subclass, dict)]
pub struct PyShape {
    pub s: Shape,
}

macro_rules! kind {
    ($rs:ident, $name:literal) => {
        #[doc = concat!("`cadquery.", $name, "`.")]
        #[pyclass(name = $name, module = "cadquery", extends = PyShape, subclass, dict)]
        pub struct $rs {}

        #[pymethods]
        impl $rs {
            #[new]
            #[pyo3(signature = (obj))]
            fn py_new(obj: &Bound<'_, PyAny>) -> PyResult<PyClassInitializer<Self>> {
                Ok(PyClassInitializer::from(PyShape { s: shape(obj)? }).add_subclass(Self {}))
            }
        }
    };
}
kind!(PyVertex, "Vertex");
kind!(PyEdge, "Edge");
kind!(PyWire, "Wire");
kind!(PyFace, "Face");
kind!(PyShell, "Shell");
kind!(PySolid, "Solid");
kind!(PyCompSolid, "CompSolid");
kind!(PyCompound, "Compound");

/// A shape as the Python object of its kind.
pub fn wrap(py: Python<'_>, s: Shape) -> PyResult<Py<PyAny>> {
    let t = s.shape_type();
    let base = PyClassInitializer::from(PyShape { s });
    Ok(match t {
        ShapeType::Vertex => Py::new(py, base.add_subclass(PyVertex {}))?.into_any(),
        ShapeType::Edge => Py::new(py, base.add_subclass(PyEdge {}))?.into_any(),
        ShapeType::Wire => Py::new(py, base.add_subclass(PyWire {}))?.into_any(),
        ShapeType::Face => Py::new(py, base.add_subclass(PyFace {}))?.into_any(),
        ShapeType::Shell => Py::new(py, base.add_subclass(PyShell {}))?.into_any(),
        ShapeType::Solid => Py::new(py, base.add_subclass(PySolid {}))?.into_any(),
        ShapeType::CompSolid => Py::new(py, base.add_subclass(PyCompSolid {}))?.into_any(),
        ShapeType::Compound => Py::new(py, base.add_subclass(PyCompound {}))?.into_any(),
    })
}

fn kind_name(o: &Bound<'_, PyAny>) -> String {
    type_name(o)
}

/// The shapes a selector keeps of a list: one shape, or a compound of them (`Shape._filter`).
fn filtered(
    py: Python<'_>,
    objs: Vec<Shape>,
    sel: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<Shape>> {
    match sel {
        None => Ok(objs),
        Some(s) if s.is_none() => Ok(objs),
        Some(s) => selector_filter(py, s, objs),
    }
}

fn one_or_compound(py: Python<'_>, v: Vec<Shape>) -> PyResult<Py<PyAny>> {
    if v.len() == 1 {
        wrap(py, v.into_iter().next().unwrap())
    } else {
        wrap(py, Shape::compound(v))
    }
}

fn single(py: Python<'_>, v: Vec<Shape>) -> PyResult<Py<PyAny>> {
    match v.into_iter().next() {
        Some(s) => wrap(py, s),
        None => Err(PyValueError::new_err("No Shape available for selection")),
    }
}

/// A location argument: a `Location`, a vector or tuple (a translation), or a shape's placement.
fn location(o: &Bound<'_, PyAny>) -> PyResult<Location> {
    if let Ok(l) = o.cast::<PyLocation>() {
        return Ok(l.borrow().l);
    }
    if let Ok(p) = o.cast::<PyPlane>() {
        return Ok(p.borrow().p.location());
    }
    Ok(Location::translation(v3(o)?))
}

/// Edges of an edge or wire argument.
fn edge_geoms(o: &Bound<'_, PyAny>) -> PyResult<Vec<EdgeGeom>> {
    Ok(shape(o)?.edge_geoms())
}

fn wire_list(o: &Bound<'_, PyAny>) -> PyResult<Vec<Vec<EdgeGeom>>> {
    let mut v = Vec::new();
    for w in o.try_iter()? {
        v.push(edge_geoms(&w?)?);
    }
    Ok(v)
}

#[pymethods]
impl PyShape {
    #[new]
    #[pyo3(signature = (obj=None))]
    fn new(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        match obj {
            Some(o) => Ok(Self { s: shape(o)? }),
            None => Err(PyTypeError::new_err("Shape() needs a shape")),
        }
    }

    #[getter(forConstruction)]
    fn for_construction(&self) -> bool {
        self.s.for_construction
    }
    #[setter(forConstruction)]
    fn set_for_construction(&mut self, v: bool) {
        self.s.for_construction = v;
    }
    #[getter]
    fn label(&self) -> String {
        self.s.label.clone()
    }
    #[setter]
    fn set_label(&mut self, v: String) {
        self.s.label = v;
    }
    /// The OCCT shape stand-in (`OCP` shims take it).
    #[getter]
    fn wrapped(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        Ok(py
            .import("OCP._shim")?
            .call_method1("TopoDS_Shape", (slf,))?
            .unbind())
    }

    #[pyo3(name = "ShapeType")]
    fn shape_type(&self) -> &'static str {
        self.s.shape_type().name()
    }
    #[pyo3(name = "geomType")]
    fn geom_type(&self) -> &'static str {
        self.s.geom_type().name()
    }
    #[pyo3(name = "isNull")]
    fn is_null(&self) -> bool {
        false
    }
    #[pyo3(name = "IsNull")]
    fn is_null2(&self) -> bool {
        false
    }
    #[pyo3(name = "isValid")]
    fn is_valid(&self) -> bool {
        self.s.is_valid()
    }
    #[pyo3(name = "isSame")]
    fn is_same(&self, o: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.s.is_same(&shape(o)?))
    }
    #[pyo3(name = "isEqual")]
    fn is_equal(&self, o: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.is_same(o)
    }
    #[pyo3(name = "hashCode")]
    fn hash_code(&self) -> u64 {
        self.__hash__()
    }
    fn __hash__(&self) -> u64 {
        let k = self.s.key();
        k.0.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add((k.1 as u64) << 56)
            .wrapping_add(k.2.wrapping_mul(0x85EB_CA6B))
    }
    fn __eq__(&self, o: &Bound<'_, PyAny>) -> bool {
        o.cast::<PyShape>()
            .is_ok_and(|s| self.s.is_same(&s.borrow().s))
    }
    fn __bool__(&self) -> bool {
        match self.s.shape_type() {
            ShapeType::Compound => !self.s.children().is_empty(),
            _ => true,
        }
    }
    fn __repr__(&self) -> String {
        format!(
            "<cadquery.occ_impl.shapes.{} object>",
            self.s.shape_type().name()
        )
    }

    // ------------------------------------------------------------ measures

    #[pyo3(name = "Volume", signature = (tol=None))]
    fn volume(&self, tol: Option<f64>) -> PyResult<f64> {
        let _ = tol;
        self.s.volume().py()
    }
    #[pyo3(name = "Area")]
    fn area(&self) -> PyResult<f64> {
        self.s.area().py()
    }
    #[pyo3(name = "Length")]
    fn length(&self) -> f64 {
        self.s.length()
    }
    #[pyo3(name = "Center")]
    fn center(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.s.center())
    }
    #[pyo3(name = "CenterOfBoundBox", signature = (tolerance=None))]
    fn center_of_bound_box(&self, py: Python<'_>, tolerance: Option<f64>) -> PyResult<Py<PyAny>> {
        let _ = tolerance;
        vector(py, self.s.bounding_box().py()?.center())
    }
    #[pyo3(name = "BoundingBox", signature = (tolerance=None, optimal=None))]
    fn bounding_box(&self, tolerance: Option<f64>, optimal: Option<bool>) -> PyResult<PyBoundBox> {
        let _ = (tolerance, optimal);
        Ok(PyBoundBox {
            b: self.s.bounding_box().py()?,
        })
    }
    #[staticmethod]
    #[pyo3(name = "CombinedCenter")]
    fn combined_center(py: Python<'_>, objs: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let v = shapes(objs)?;
        let mut m = V3::zeros();
        let mut total = 0.0;
        for s in &v {
            let w = s.volume().py()?;
            m += s.center() * w;
            total += w;
        }
        vector(py, m / total)
    }
    #[staticmethod]
    #[pyo3(name = "CombinedCenterOfBoundBox")]
    fn combined_center_bb(py: Python<'_>, objs: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let v = shapes(objs)?;
        let mut m = V3::zeros();
        for s in &v {
            m += s.bounding_box().py()?.center();
        }
        vector(py, m / v.len() as f64)
    }
    #[staticmethod]
    #[pyo3(name = "computeMass")]
    fn compute_mass(o: &Bound<'_, PyAny>) -> PyResult<f64> {
        shape(o)?.volume().py()
    }
    #[staticmethod]
    #[pyo3(name = "centerOfMass")]
    fn center_of_mass(py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        vector(py, shape(o)?.center())
    }
    #[pyo3(name = "Closed")]
    fn closed(&self) -> bool {
        self.s.is_closed()
    }

    // ------------------------------------------------------------ topology

    #[pyo3(name = "Vertices")]
    fn vertices_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.vertices())
    }
    #[pyo3(name = "Edges")]
    fn edges_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.edges())
    }
    #[pyo3(name = "Wires")]
    fn wires_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.wires())
    }
    #[pyo3(name = "Faces")]
    fn faces_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.faces())
    }
    #[pyo3(name = "Shells")]
    fn shells_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.shells())
    }
    #[pyo3(name = "Solids")]
    fn solids_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.solids())
    }
    #[pyo3(name = "Compounds")]
    fn compounds_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.entities(ShapeType::Compound))
    }
    #[pyo3(name = "CompSolids")]
    fn compsolids_all(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, vec![])
    }
    #[pyo3(signature = (selector=None))]
    fn vertices(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        one_or_compound(py, filtered(py, self.s.vertices(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn edges(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        one_or_compound(py, filtered(py, self.s.edges(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn wires(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        one_or_compound(py, filtered(py, self.s.wires(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn faces(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        one_or_compound(py, filtered(py, self.s.faces(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn shells(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        one_or_compound(py, filtered(py, self.s.shells(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn solids(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        one_or_compound(py, filtered(py, self.s.solids(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn vertex(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        single(py, filtered(py, self.s.vertices(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn edge(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        single(py, filtered(py, self.s.edges(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn wire(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        single(py, filtered(py, self.s.wires(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn face(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        single(py, filtered(py, self.s.faces(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn shell(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        single(py, filtered(py, self.s.shells(), selector)?)
    }
    #[pyo3(signature = (selector=None))]
    fn solid(&self, py: Python<'_>, selector: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        single(py, filtered(py, self.s.solids(), selector)?)
    }
    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let items = match self.s.shape_type() {
            ShapeType::Wire => self.s.edges(),
            ShapeType::Compound => self.s.children(),
            ShapeType::Solid => self.s.shells(),
            ShapeType::Shell => self.s.faces(),
            ShapeType::Face => self.s.wires(),
            ShapeType::Edge => self.s.vertices(),
            _ => vec![],
        };
        let l = shape_list(py, items)?;
        Ok(l.bind(py).try_iter()?.into_any().unbind())
    }
    fn __len__(&self) -> usize {
        match self.s.shape_type() {
            ShapeType::Compound => self.s.children().len(),
            _ => 1,
        }
    }
    fn __getitem__(&self, py: Python<'_>, i: isize) -> PyResult<Py<PyAny>> {
        let c = self.s.children();
        let n = c.len() as isize;
        let j = if i < 0 { n + i } else { i };
        if j < 0 || j >= n {
            return Err(pyo3::exceptions::PyIndexError::new_err(
                "index out of range",
            ));
        }
        wrap(py, c[j as usize].clone())
    }
    fn size(&self) -> usize {
        self.__len__()
    }

    // ------------------------------------------------------------ moves

    fn translate(&self, py: Python<'_>, v: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.translate(v3(v)?).py()?)
    }
    fn rotate(
        &self,
        py: Python<'_>,
        a: &Bound<'_, PyAny>,
        b: &Bound<'_, PyAny>,
        deg: f64,
    ) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.rotate(v3(a)?, v3(b)?, deg).py()?)
    }
    fn scale(&self, py: Python<'_>, f: f64) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.scale(f).py()?)
    }
    #[pyo3(signature = (mirrorPlane=None, basePointVector=None))]
    #[allow(non_snake_case)]
    fn mirror(
        &self,
        py: Python<'_>,
        mirrorPlane: Option<&Bound<'_, PyAny>>,
        basePointVector: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let n = match mirrorPlane {
            None => V3::z(),
            Some(m) => match m.extract::<String>() {
                Ok(s) => match s.as_str() {
                    "XY" | "YX" => V3::z(),
                    "XZ" | "ZX" => V3::y(),
                    "YZ" | "ZY" => V3::x(),
                    _ => return Err(PyValueError::new_err(format!("Invalid mirror plane {s}"))),
                },
                Err(_) => v3(m)?,
            },
        };
        let b = v3_opt(basePointVector)?.unwrap_or_default();
        wrap(py, self.s.mirror(n, b).py()?)
    }
    #[pyo3(signature = (*args, **kw))]
    fn moved(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        if args.len() == 1 && kw.is_none_or(|k| k.is_empty()) {
            let a = args.get_item(0)?;
            if let Ok(l) = a.cast::<PyList>() {
                let v: Vec<Shape> = l
                    .iter()
                    .map(|x| self.s.moved(&location(&x)?).py())
                    .collect::<PyResult<_>>()?;
                return wrap(py, Shape::compound(v));
            }
            if let Ok(t) = a.cast::<PyTuple>()
                && t.len() > 0
                && t.get_item(0)?.cast::<PyLocation>().is_ok()
            {
                let v: Vec<Shape> = t
                    .iter()
                    .map(|x| self.s.moved(&location(&x)?).py())
                    .collect::<PyResult<_>>()?;
                return wrap(py, Shape::compound(v));
            }
            return wrap(py, self.s.moved(&location(&a)?).py()?);
        }
        if args.len() >= 2 && args.get_item(0)?.cast::<PyLocation>().is_ok() {
            let v: Vec<Shape> = args
                .iter()
                .map(|x| self.s.moved(&location(&x)?).py())
                .collect::<PyResult<_>>()?;
            return wrap(py, Shape::compound(v));
        }
        let loc = Py::new(py, PyLocation::py_new(args, kw)?)?;
        wrap(py, self.s.moved(&loc.borrow(py).l).py()?)
    }
    fn located(&self, py: Python<'_>, loc: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.moved(&location(loc)?).py()?)
    }
    #[pyo3(name = "move", signature = (*args, **kw))]
    fn move_(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let loc = if args.len() == 1 && kw.is_none_or(|k| k.is_empty()) {
            location(&args.get_item(0)?)?
        } else {
            PyLocation::py_new(args, kw)?.l
        };
        let moved = slf.borrow().s.moved(&loc).py()?;
        slf.borrow_mut().s = moved;
        Ok(slf.clone().into_any().unbind())
    }
    fn locate(slf: &Bound<'_, Self>, loc: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let l = location(loc)?;
        let moved = slf.borrow().s.moved(&l).py()?;
        slf.borrow_mut().s = moved;
        Ok(slf.clone().into_any().unbind())
    }
    fn location(&self) -> PyLocation {
        PyLocation {
            l: Location::default(),
        }
    }
    #[pyo3(signature = (mesh=false))]
    fn copy(&self, py: Python<'_>, mesh: bool) -> PyResult<Py<PyAny>> {
        let _ = mesh;
        wrap(py, self.s.clone())
    }
    fn __copy__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.clone())
    }
    fn __deepcopy__(&self, py: Python<'_>, _memo: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.clone())
    }
    fn clean(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        wrap(py, ops::clean(&self.s).py()?)
    }
    fn fix(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.clone())
    }

    // ------------------------------------------------------------ booleans

    #[pyo3(signature = (*others, glue=false, tol=None))]
    fn fuse(
        &self,
        py: Python<'_>,
        others: &Bound<'_, PyTuple>,
        glue: bool,
        tol: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        let _ = (glue, tol);
        wrap(py, ops::fuse(&self.s, &shapes(others)?).py()?)
    }
    #[pyo3(signature = (*others, tol=None))]
    fn cut(
        &self,
        py: Python<'_>,
        others: &Bound<'_, PyTuple>,
        tol: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        let _ = tol;
        wrap(py, ops::cut(&self.s, &shapes(others)?).py()?)
    }
    #[pyo3(signature = (*others, tol=None))]
    fn intersect(
        &self,
        py: Python<'_>,
        others: &Bound<'_, PyTuple>,
        tol: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        let _ = tol;
        wrap(py, ops::intersect(&self.s, &shapes(others)?).py()?)
    }
    fn __add__(&self, py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, ops::fuse(&self.s, &[shape(o)?]).py()?)
    }
    fn __sub__(&self, py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, ops::cut(&self.s, &[shape(o)?]).py()?)
    }
    fn __mul__(&self, py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, ops::intersect(&self.s, &[shape(o)?]).py()?)
    }
    #[pyo3(signature = (*others))]
    fn split(&self, py: Python<'_>, others: &Bound<'_, PyTuple>) -> PyResult<Py<PyAny>> {
        let tools = shapes(others)?;
        let inside = ops::intersect(&self.s, &tools).py()?;
        let outside = ops::cut(&self.s, &tools).py()?;
        wrap(py, Shape::compound(vec![outside, inside]))
    }

    // ------------------------------------------------------------ solids

    #[pyo3(name = "isInside", signature = (point, tolerance=1e-6))]
    fn is_inside(&self, point: &Bound<'_, PyAny>, tolerance: f64) -> PyResult<bool> {
        ops::is_inside(&self.s, v3(point)?, tolerance).py()
    }
    #[pyo3(signature = (radius, edgeList))]
    #[allow(non_snake_case)]
    fn fillet(
        &self,
        py: Python<'_>,
        radius: f64,
        edgeList: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        wrap(py, ops::fillet(&self.s, radius, &shapes(edgeList)?).py()?)
    }
    #[pyo3(signature = (length, length2, edgeList))]
    #[allow(non_snake_case)]
    fn chamfer(
        &self,
        py: Python<'_>,
        length: f64,
        length2: Option<f64>,
        edgeList: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        wrap(
            py,
            ops::chamfer(&self.s, length, length2, &shapes(edgeList)?).py()?,
        )
    }
    #[pyo3(signature = (faceList, thickness, tolerance=1e-4, kind="arc"))]
    #[allow(non_snake_case)]
    fn hollow(
        &self,
        py: Python<'_>,
        faceList: &Bound<'_, PyAny>,
        thickness: f64,
        tolerance: f64,
        kind: &str,
    ) -> PyResult<Py<PyAny>> {
        let _ = tolerance;
        let faces = if faceList.is_none() {
            vec![]
        } else {
            shapes(faceList)?
        };
        wrap(
            py,
            ops::hollow(&self.s, &faces, thickness, kind == "intersection").py()?,
        )
    }
    #[pyo3(name = "outerShell")]
    fn outer_shell(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        single(py, self.s.shells())
    }

    // ------------------------------------------------------------ curves

    #[pyo3(name = "startPoint")]
    fn start_point(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.s.start_point().py()?)
    }
    #[pyo3(name = "endPoint")]
    fn end_point(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.s.end_point().py()?)
    }
    #[pyo3(name = "tangentAt", signature = (locationParam=0.5, mode="length"))]
    #[allow(non_snake_case)]
    fn tangent_at(&self, py: Python<'_>, locationParam: f64, mode: &str) -> PyResult<Py<PyAny>> {
        vector(py, self.s.tangent_at(locationParam, mode == "length").py()?)
    }
    #[pyo3(name = "positionAt", signature = (d, mode="length"))]
    fn position_at(&self, py: Python<'_>, d: f64, mode: &str) -> PyResult<Py<PyAny>> {
        vector(py, self.s.position_at(d, mode == "length").py()?)
    }
    #[pyo3(signature = (ds, mode="length"))]
    fn positions(&self, py: Python<'_>, ds: &Bound<'_, PyAny>, mode: &str) -> PyResult<Py<PyAny>> {
        let mut out = Vec::new();
        for d in ds.try_iter()? {
            out.push(vector(
                py,
                self.s.position_at(num(&d?)?, mode == "length").py()?,
            )?);
        }
        Ok(PyList::new(py, out)?.into_any().unbind())
    }
    #[pyo3(name = "paramAt")]
    fn param_at(&self, d: &Bound<'_, PyAny>) -> PyResult<f64> {
        if let Ok(x) = d.extract::<f64>() {
            let g = self.s.edge_geoms();
            if g.len() == 1 {
                return Ok(g[0].param_at_fraction(x));
            }
            return Ok(x);
        }
        let q = v3(d)?;
        let g = self.s.edge_geoms();
        g.first()
            .map(|e| e.param_of(q))
            .ok_or_else(|| PyValueError::new_err("no curve"))
    }
    fn radius(&self) -> PyResult<f64> {
        self.s.radius().py()
    }
    #[pyo3(name = "arcCenter")]
    fn arc_center(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.s.arc_center().py()?)
    }
    #[pyo3(name = "IsClosed")]
    fn is_closed(&self) -> bool {
        self.s.is_closed()
    }
    fn close(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut g = self.s.edge_geoms();
        if self.s.is_closed() {
            return wrap(py, self.s.clone());
        }
        let (a, b) = (g.last().unwrap().end(), g[0].start());
        g.push(EdgeGeom::line(a, b).py()?);
        wrap(py, Shape::wire(g))
    }
    #[pyo3(signature = (n))]
    fn sample(&self, py: Python<'_>, n: &Bound<'_, PyAny>) -> PyResult<(Py<PyAny>, Py<PyAny>)> {
        let k = n.extract::<usize>().unwrap_or(16).max(1);
        let closed = self.s.is_closed();
        let m = k;
        let mut pts = Vec::new();
        let mut ts = Vec::new();
        for i in 0..m {
            let f = if closed {
                i as f64 / k as f64
            } else if k > 1 {
                i as f64 / (k - 1) as f64
            } else {
                0.0
            };
            pts.push(vector(py, self.s.position_at(f, true).py()?)?);
            ts.push(f);
        }
        Ok((
            PyList::new(py, pts)?.into_any().unbind(),
            PyList::new(py, ts)?.into_any().unbind(),
        ))
    }

    // ------------------------------------------------------------ faces

    #[pyo3(name = "normalAt", signature = (locationVector=None, v=None))]
    #[allow(non_snake_case)]
    fn normal_at(
        &self,
        py: Python<'_>,
        locationVector: Option<&Bound<'_, PyAny>>,
        v: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        if let (Some(u), Some(v)) = (locationVector, v) {
            let p = self.s.face_point_at(num(u)?, v).py()?;
            let n = self.s.normal_at(Some(p)).py()?;
            return Ok(PyTuple::new(py, [vector(py, n)?, vector(py, p)?])?
                .into_any()
                .unbind());
        }
        vector(py, self.s.normal_at(v3_opt(locationVector)?).py()?)
    }
    #[pyo3(name = "outerWire")]
    fn outer_wire(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        wrap(py, self.s.outer_wire().py()?)
    }
    #[pyo3(name = "innerWires")]
    fn inner_wires(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        shape_list(py, self.s.inner_wires().py()?)
    }
    #[pyo3(name = "uvBounds")]
    fn uv_bounds(&self) -> PyResult<(f64, f64, f64, f64)> {
        self.s.uv_bounds().py()
    }
    #[pyo3(name = "toPln")]
    fn to_pln(&self) -> PyResult<PyPlane> {
        let n = self.s.normal_at(None).py()?;
        Ok(PyPlane {
            p: Plane::new(self.s.center(), None, n).py()?,
        })
    }

    // ------------------------------------------------------------ export

    #[pyo3(name = "exportStep", signature = (fileName, **kw))]
    #[allow(non_snake_case)]
    fn export_step(
        &self,
        fileName: &str,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<bool> {
        let _ = kw;
        let bytes = cadrs_kernel::cadquery::ops::export_step(&self.s).py()?;
        std::fs::write(fileName, bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(true)
    }
    #[pyo3(name = "exportStl", signature = (fileName, *args, **kw))]
    #[allow(non_snake_case)]
    fn export_stl(
        &self,
        fileName: &str,
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<bool> {
        let _ = (args, kw);
        let bytes = cadrs_kernel::cadquery::ops::export_stl(&self.s).py()?;
        std::fs::write(fileName, bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(true)
    }
}

// ---------------------------------------------------------------- constructors

#[pymethods]
impl PyVertex {
    #[staticmethod]
    #[pyo3(name = "makeVertex")]
    fn make_vertex(py: Python<'_>, x: f64, y: f64, z: f64) -> PyResult<Py<PyAny>> {
        wrap(py, Shape::vertex(V3::new(x, y, z)))
    }
    #[pyo3(name = "toTuple")]
    fn to_tuple(slf: PyRef<'_, Self>) -> (f64, f64, f64) {
        let c = slf.as_super().s.center();
        (c.x, c.y, c.z)
    }
    #[getter(X)]
    fn x(slf: PyRef<'_, Self>) -> f64 {
        slf.as_super().s.center().x
    }
    #[getter(Y)]
    fn y(slf: PyRef<'_, Self>) -> f64 {
        slf.as_super().s.center().y
    }
    #[getter(Z)]
    fn z(slf: PyRef<'_, Self>) -> f64 {
        slf.as_super().s.center().z
    }
}

fn deg_default(o: Option<f64>, d: f64) -> f64 {
    o.unwrap_or(d)
}

#[pymethods]
impl PyEdge {
    #[staticmethod]
    #[pyo3(name = "makeLine")]
    fn make_line(
        py: Python<'_>,
        v1: &Bound<'_, PyAny>,
        v2: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        wrap(py, Shape::edge(EdgeGeom::line(v3(v1)?, v3(v2)?).py()?))
    }
    #[staticmethod]
    #[pyo3(name = "makeCircle", signature = (radius, pnt=None, dir=None, angle1=None, angle2=None, orientation=true))]
    fn make_circle(
        py: Python<'_>,
        radius: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        angle1: Option<f64>,
        angle2: Option<f64>,
        orientation: bool,
    ) -> PyResult<Py<PyAny>> {
        let c = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(
            py,
            Shape::edge(
                make::circle(
                    radius,
                    c,
                    d,
                    deg_default(angle1, 360.0),
                    deg_default(angle2, 360.0),
                    orientation,
                )
                .py()?,
            ),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeEllipse", signature = (x_radius, y_radius, pnt=None, dir=None, xdir=None, angle1=360.0, angle2=360.0, sense=1))]
    #[allow(clippy::too_many_arguments)]
    fn make_ellipse(
        py: Python<'_>,
        x_radius: f64,
        y_radius: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        xdir: Option<&Bound<'_, PyAny>>,
        angle1: f64,
        angle2: f64,
        sense: i64,
    ) -> PyResult<Py<PyAny>> {
        let c = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        let x = v3_opt(xdir)?.unwrap_or(V3::x());
        wrap(
            py,
            Shape::edge(
                make::ellipse(x_radius, y_radius, c, d, x, angle1, angle2, sense == 1).py()?,
            ),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeSpline", signature = (listOfVector, tangents=None, periodic=false, parameters=None, scale=true, tol=1e-6))]
    #[allow(non_snake_case)]
    fn make_spline(
        py: Python<'_>,
        listOfVector: &Bound<'_, PyAny>,
        tangents: Option<&Bound<'_, PyAny>>,
        periodic: bool,
        parameters: Option<Vec<f64>>,
        scale: bool,
        tol: f64,
    ) -> PyResult<Py<PyAny>> {
        let _ = tol;
        let pts = crate::conv::points(listOfVector)?;
        let tg = tangents_arg(tangents)?;
        wrap(
            py,
            Shape::edge(
                make::spline(&pts, tg.as_deref(), periodic, parameters.as_deref(), scale).py()?,
            ),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeSplineApprox", signature = (listOfVector, tol=1e-3, smoothing=None, minDeg=1, maxDeg=6))]
    #[allow(non_snake_case)]
    fn make_spline_approx(
        py: Python<'_>,
        listOfVector: &Bound<'_, PyAny>,
        tol: f64,
        smoothing: Option<&Bound<'_, PyAny>>,
        minDeg: i64,
        maxDeg: i64,
    ) -> PyResult<Py<PyAny>> {
        let _ = (smoothing, minDeg, maxDeg);
        let pts = crate::conv::points(listOfVector)?;
        wrap(py, Shape::edge(make::spline_approx(&pts, tol).py()?))
    }
    #[staticmethod]
    #[pyo3(name = "makeThreePointArc")]
    fn make_three_point_arc(
        py: Python<'_>,
        v1: &Bound<'_, PyAny>,
        v2: &Bound<'_, PyAny>,
        v3_: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        wrap(
            py,
            Shape::edge(make::three_point_arc(v3(v1)?, v3(v2)?, v3(v3_)?).py()?),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeTangentArc")]
    fn make_tangent_arc(
        py: Python<'_>,
        v1: &Bound<'_, PyAny>,
        v2: &Bound<'_, PyAny>,
        v3_: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        wrap(
            py,
            Shape::edge(make::tangent_arc(v3(v1)?, v3(v2)?, v3(v3_)?).py()?),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeBezier")]
    fn make_bezier(py: Python<'_>, points: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(
            py,
            Shape::edge(make::bezier(&crate::conv::points(points)?).py()?),
        )
    }
}

/// `tangents`: a list of vectors (or Nones).
pub fn tangents_arg(o: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Vec<Option<V3>>>> {
    match o {
        None => Ok(None),
        Some(o) if o.is_none() => Ok(None),
        Some(o) => {
            let mut v = Vec::new();
            for t in o.try_iter()? {
                let t = t?;
                v.push(if t.is_none() { None } else { Some(v3(&t)?) });
            }
            Ok(if v.is_empty() { None } else { Some(v) })
        }
    }
}

#[pymethods]
impl PyWire {
    #[staticmethod]
    #[pyo3(name = "makePolygon", signature = (listOfVertices, forConstruction=false, close=false))]
    #[allow(non_snake_case)]
    fn make_polygon(
        py: Python<'_>,
        listOfVertices: &Bound<'_, PyAny>,
        forConstruction: bool,
        close: bool,
    ) -> PyResult<Py<PyAny>> {
        let pts = crate::conv::points(listOfVertices)?;
        wrap(
            py,
            Shape::wire(make::polygon(&pts, close).py()?).construction(forConstruction),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeCircle")]
    fn make_circle(
        py: Python<'_>,
        radius: f64,
        center: &Bound<'_, PyAny>,
        normal: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        wrap(
            py,
            Shape::wire(vec![
                make::circle(radius, v3(center)?, v3(normal)?, 360.0, 360.0, true).py()?,
            ]),
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeEllipse", signature = (x_radius, y_radius, center, normal, xDir, angle1=360.0, angle2=360.0, rotation_angle=0.0, closed=true))]
    #[allow(non_snake_case, clippy::too_many_arguments)]
    fn make_ellipse(
        py: Python<'_>,
        x_radius: f64,
        y_radius: f64,
        center: &Bound<'_, PyAny>,
        normal: &Bound<'_, PyAny>,
        xDir: &Bound<'_, PyAny>,
        angle1: f64,
        angle2: f64,
        rotation_angle: f64,
        closed: bool,
    ) -> PyResult<Py<PyAny>> {
        let (c, n) = (v3(center)?, v3(normal)?);
        let e = make::ellipse(x_radius, y_radius, c, n, v3(xDir)?, angle1, angle2, true).py()?;
        let mut g = vec![e.clone()];
        if angle1 != angle2 && closed {
            g.push(EdgeGeom::line(e.end(), e.start()).py()?);
        }
        let mut w = Shape::wire(g);
        if rotation_angle != 0.0 {
            w = w.rotate(c, c + n, rotation_angle).py()?;
        }
        wrap(py, w)
    }
    #[staticmethod]
    #[pyo3(name = "makeHelix", signature = (pitch, height, radius, center=None, dir=None, angle=360.0, lefthand=false))]
    #[allow(clippy::too_many_arguments)]
    fn make_helix(
        py: Python<'_>,
        pitch: f64,
        height: f64,
        radius: f64,
        center: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        angle: f64,
        lefthand: bool,
    ) -> PyResult<Py<PyAny>> {
        let c = v3_opt(center)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(
            py,
            Shape::wire(make::helix(pitch, height, radius, c, d, angle, lefthand).py()?),
        )
    }
    #[staticmethod]
    #[pyo3(name = "assembleEdges")]
    #[allow(non_snake_case)]
    fn assemble_edges(py: Python<'_>, listOfEdges: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let g: Vec<EdgeGeom> = shapes(listOfEdges)?
            .iter()
            .flat_map(|s| s.edge_geoms())
            .collect();
        wrap(py, Shape::wire(make::assemble(g).py()?))
    }
    #[staticmethod]
    #[pyo3(signature = (listOfWires, tol=1e-9))]
    #[allow(non_snake_case)]
    fn combine(py: Python<'_>, listOfWires: &Bound<'_, PyAny>, tol: f64) -> PyResult<Py<PyAny>> {
        let g: Vec<EdgeGeom> = shapes(listOfWires)?
            .iter()
            .flat_map(|s| s.edge_geoms())
            .collect();
        let w: Vec<Shape> = make::chain_edges(g, Some(tol.max(1e-7)))
            .into_iter()
            .map(Shape::wire)
            .collect();
        shape_list(py, w)
    }
    #[pyo3(name = "offset2D", signature = (d, kind="arc"))]
    fn offset2d(slf: PyRef<'_, Self>, py: Python<'_>, d: f64, kind: &str) -> PyResult<Py<PyAny>> {
        let g = slf.as_super().s.edge_geoms();
        let pl = make::wires_plane(std::slice::from_ref(&g), None).py()?;
        let v: Vec<Shape> = ops::offset_wires(&g, &pl, d, kind == "intersection")
            .py()?
            .into_iter()
            .map(Shape::wire)
            .collect();
        shape_list(py, v)
    }
    fn stitch(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        other: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let mut g = slf.as_super().s.edge_geoms();
        g.extend(shape(other)?.edge_geoms());
        wrap(py, Shape::wire(make::assemble(g).py()?))
    }
}

#[pymethods]
impl PyFace {
    #[staticmethod]
    #[pyo3(name = "makeFromWires", signature = (outerWire, innerWires=None))]
    #[allow(non_snake_case)]
    fn make_from_wires(
        py: Python<'_>,
        outerWire: &Bound<'_, PyAny>,
        innerWires: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let outer = edge_geoms(outerWire)?;
        let inner = match innerWires {
            Some(w) if !w.is_none() => wire_list(w)?,
            _ => vec![],
        };
        let mut all = vec![outer.clone()];
        all.extend(inner.iter().cloned());
        let pl = make::wires_plane(&all, None).py()?;
        wrap(py, make::face(&pl, &outer, &inner).py()?)
    }
    #[staticmethod]
    #[pyo3(name = "makePlane", signature = (length=None, width=None, basePnt=None, dir=None))]
    #[allow(non_snake_case)]
    fn make_plane(
        py: Python<'_>,
        length: Option<f64>,
        width: Option<f64>,
        basePnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let o = v3_opt(basePnt)?.unwrap_or_default();
        let n = v3_opt(dir)?.unwrap_or(V3::z());
        let pl = Plane::new(o, None, n).py()?;
        let (l, w) = (length.unwrap_or(1e4), width.unwrap_or(1e4));
        let (x, y, _) = pl.frame();
        let pts: Vec<V3> = [
            (-0.5, -0.5),
            (0.5, -0.5),
            (0.5, 0.5),
            (-0.5, 0.5),
            (-0.5, -0.5),
        ]
        .iter()
        .map(|(a, b)| o + x * (a * w) + y * (b * l))
        .collect();
        wrap(
            py,
            make::face(&pl, &make::polygon(&pts, false).py()?, &[]).py()?,
        )
    }
}

#[pymethods]
impl PySolid {
    #[staticmethod]
    #[pyo3(name = "makeBox", signature = (length, width, height, pnt=None, dir=None))]
    fn make_box(
        py: Python<'_>,
        length: f64,
        width: f64,
        height: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(py, make::make_box(length, width, height, p, d).py()?)
    }
    #[staticmethod]
    #[pyo3(name = "makeCylinder", signature = (radius, height, pnt=None, dir=None, angleDegrees=360.0))]
    #[allow(non_snake_case)]
    fn make_cylinder(
        py: Python<'_>,
        radius: f64,
        height: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        angleDegrees: f64,
    ) -> PyResult<Py<PyAny>> {
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(py, make::cylinder(radius, height, p, d, angleDegrees).py()?)
    }
    #[staticmethod]
    #[pyo3(name = "makeCone", signature = (radius1, radius2, height, pnt=None, dir=None, angleDegrees=360.0))]
    #[allow(non_snake_case)]
    fn make_cone(
        py: Python<'_>,
        radius1: f64,
        radius2: f64,
        height: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        angleDegrees: f64,
    ) -> PyResult<Py<PyAny>> {
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(
            py,
            make::cone(radius1, radius2, height, p, d, angleDegrees).py()?,
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeSphere", signature = (radius, pnt=None, dir=None, angleDegrees1=0.0, angleDegrees2=90.0, angleDegrees3=360.0))]
    #[allow(non_snake_case)]
    fn make_sphere(
        py: Python<'_>,
        radius: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        angleDegrees1: f64,
        angleDegrees2: f64,
        angleDegrees3: f64,
    ) -> PyResult<Py<PyAny>> {
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(
            py,
            make::sphere(radius, p, d, angleDegrees1, angleDegrees2, angleDegrees3).py()?,
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeTorus", signature = (radius1, radius2, pnt=None, dir=None, angleDegrees1=0.0, angleDegrees2=360.0))]
    #[allow(non_snake_case)]
    fn make_torus(
        py: Python<'_>,
        radius1: f64,
        radius2: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
        angleDegrees1: f64,
        angleDegrees2: f64,
    ) -> PyResult<Py<PyAny>> {
        let _ = (angleDegrees1, angleDegrees2);
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(py, make::torus(radius1, radius2, p, d).py()?)
    }
    #[staticmethod]
    #[pyo3(name = "makeWedge", signature = (dx, dy, dz, xmin, zmin, xmax, zmax, pnt=None, dir=None))]
    #[allow(clippy::too_many_arguments)]
    fn make_wedge(
        py: Python<'_>,
        dx: f64,
        dy: f64,
        dz: f64,
        xmin: f64,
        zmin: f64,
        xmax: f64,
        zmax: f64,
        pnt: Option<&Bound<'_, PyAny>>,
        dir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let p = v3_opt(pnt)?.unwrap_or_default();
        let d = v3_opt(dir)?.unwrap_or(V3::z());
        wrap(
            py,
            make::wedge(dx, dy, dz, xmin, zmin, xmax, zmax, p, d).py()?,
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeLoft", signature = (listOfWire, ruled=false))]
    #[allow(non_snake_case)]
    fn make_loft(
        py: Python<'_>,
        listOfWire: &Bound<'_, PyAny>,
        ruled: bool,
    ) -> PyResult<Py<PyAny>> {
        wrap(py, ops::loft(&loft_items(listOfWire)?, ruled).py()?)
    }
    #[staticmethod]
    #[pyo3(name = "extrudeLinear", signature = (*args, **kw))]
    fn extrude_linear(
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let get = |name: &str, i: usize| -> PyResult<Option<Bound<'_, PyAny>>> {
            if let Some(kw) = kw
                && let Some(v) = kw.get_item(name)?
            {
                return Ok(Some(v));
            }
            if i < args.len() {
                Ok(Some(args.get_item(i)?))
            } else {
                Ok(None)
            }
        };
        let first = get("face", 0)?
            .or(get("outerWire", 0)?)
            .ok_or_else(|| PyTypeError::new_err("extrudeLinear needs a face"))?;
        let s0 = shape(&first)?;
        let (face, vec, taper) = if s0.shape_type() == ShapeType::Face {
            let v = get("vecNormal", 1)?
                .ok_or_else(|| PyTypeError::new_err("extrudeLinear needs a vector"))?;
            let t = get("taper", 2)?
                .map(|t| num(&t))
                .transpose()?
                .unwrap_or(0.0);
            (s0, v3(&v)?, t)
        } else {
            let inner = get("innerWires", 1)?
                .map(|w| wire_list(&w))
                .transpose()?
                .unwrap_or_default();
            let v = get("vecNormal", 2)?
                .ok_or_else(|| PyTypeError::new_err("extrudeLinear needs a vector"))?;
            let t = get("taper", 3)?
                .map(|t| num(&t))
                .transpose()?
                .unwrap_or(0.0);
            let mut all = vec![s0.edge_geoms()];
            all.extend(inner);
            let f = make::faces_from_wires(&all, None).py()?;
            (f.into_iter().next().unwrap(), v3(&v)?, t)
        };
        wrap(py, ops::extrude(&[face], vec, taper).py()?)
    }
    #[staticmethod]
    #[pyo3(signature = (*args))]
    fn revolve(py: Python<'_>, args: &Bound<'_, PyTuple>) -> PyResult<Py<PyAny>> {
        let s0 = shape(&args.get_item(0)?)?;
        let (face, rest) = if s0.shape_type() == ShapeType::Face {
            (s0, 1)
        } else {
            let mut all = vec![s0.edge_geoms()];
            all.extend(wire_list(&args.get_item(1)?)?);
            (
                make::faces_from_wires(&all, None)
                    .py()?
                    .into_iter()
                    .next()
                    .unwrap(),
                2,
            )
        };
        let deg = num(&args.get_item(rest)?)?;
        let a = v3(&args.get_item(rest + 1)?)?;
        let b = v3(&args.get_item(rest + 2)?)?;
        wrap(py, ops::revolve(&[face], deg, a, b).py()?)
    }
    #[staticmethod]
    #[pyo3(signature = (*args, **kw))]
    fn sweep(
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let kwv = |name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
            match kw {
                Some(k) => k.get_item(name),
                None => Ok(None),
            }
        };
        let s0 = shape(&args.get_item(0)?)?;
        let (face, mut i) = if s0.shape_type() == ShapeType::Face {
            (s0, 1)
        } else {
            let mut all = vec![s0.edge_geoms()];
            all.extend(wire_list(&args.get_item(1)?)?);
            (
                make::faces_from_wires(&all, None)
                    .py()?
                    .into_iter()
                    .next()
                    .unwrap(),
                2,
            )
        };
        let path = match kwv("path")? {
            Some(p) => p,
            None => {
                i += 1;
                args.get_item(i - 1)?
            }
        };
        let arg = |name: &str, j: usize| -> PyResult<Option<Bound<'_, PyAny>>> {
            if let Some(v) = kwv(name)? {
                return Ok(Some(v));
            }
            if j < args.len() {
                Ok(Some(args.get_item(j)?))
            } else {
                Ok(None)
            }
        };
        let solid = arg("makeSolid", i)?
            .map(|v| v.is_truthy())
            .transpose()?
            .unwrap_or(true);
        let frenet = arg("isFrenet", i + 1)?
            .map(|v| v.is_truthy())
            .transpose()?
            .unwrap_or(false);
        let mode = arg("mode", i + 2)?;
        let transition = arg("transitionMode", i + 3)?
            .map(|v| v.extract::<String>())
            .transpose()?
            .unwrap_or_else(|| "transformed".into());
        let m = match mode {
            Some(m) if !m.is_none() && m.cast::<PyShape>().is_err() => SweepMode::Fixed(v3(&m)?),
            _ if frenet => SweepMode::Frenet,
            _ => SweepMode::Corrected,
        };
        wrap(
            py,
            ops::sweep(
                &[face],
                &shape(&path)?.edge_geoms(),
                solid,
                m,
                transition == "round",
            )
            .py()?,
        )
    }
    #[staticmethod]
    #[pyo3(name = "makeSolid")]
    fn make_solid(py: Python<'_>, shell: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, ops::solid_of_shell(&shape(shell)?).py()?)
    }
    #[staticmethod]
    #[pyo3(name = "isSolid")]
    fn is_solid(o: &Bound<'_, PyAny>) -> bool {
        match o.cast::<PyShape>() {
            Ok(s) => {
                let s = &s.borrow().s;
                s.shape_type() == ShapeType::Solid
                    || (s.shape_type() == ShapeType::Compound && !s.solids().is_empty())
            }
            Err(_) => false,
        }
    }
}

/// Loft sections: wires (or faces' outer wires) and vertices.
fn loft_items(o: &Bound<'_, PyAny>) -> PyResult<Vec<LoftItem>> {
    let mut v = Vec::new();
    for s in shapes(o)? {
        v.push(match s.shape_type() {
            ShapeType::Vertex => LoftItem::Point(s.center()),
            ShapeType::Face => LoftItem::Wire(s.outer_wire().py()?.edge_geoms()),
            _ => LoftItem::Wire(s.edge_geoms()),
        });
    }
    Ok(v)
}

#[pymethods]
impl PyShell {
    #[staticmethod]
    #[pyo3(name = "makeShell")]
    #[allow(non_snake_case)]
    fn make_shell(py: Python<'_>, listOfFaces: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, ops::sew(&shapes(listOfFaces)?).py()?)
    }
}

#[pymethods]
impl PyCompound {
    #[staticmethod]
    #[pyo3(name = "makeCompound")]
    #[allow(non_snake_case)]
    fn make_compound(py: Python<'_>, listOfShapes: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        wrap(py, Shape::compound(shapes(listOfShapes)?))
    }
    #[staticmethod]
    #[pyo3(name = "makeText", signature = (*args, **kw))]
    fn make_text(
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        Ok(py
            .import("cadquery._text")?
            .getattr("compound_make_text")?
            .call(args, kw)?
            .unbind())
    }
    fn __bool__(slf: PyRef<'_, Self>) -> bool {
        !slf.as_super().s.children().is_empty()
    }
}

/// `wiresToFaces(wires)`.
#[pyfunction(name = "wiresToFaces")]
pub fn wires_to_faces(py: Python<'_>, wires: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    shape_list(py, make::faces_from_wires(&wire_list(wires)?, None).py()?)
}

/// `sortWiresByBuildOrder(wires)`.
#[pyfunction(name = "sortWiresByBuildOrder")]
pub fn sort_wires_by_build_order(py: Python<'_>, wires: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let groups = make::wires_to_faces(&wire_list(wires)?, None).py()?;
    let mut out = Vec::new();
    for (_, o, h) in groups {
        let mut v = vec![wrap(py, Shape::wire(o))?];
        for w in h {
            v.push(wrap(py, Shape::wire(w))?);
        }
        out.push(PyList::new(py, v)?.into_any().unbind());
    }
    Ok(PyList::new(py, out)?.into_any().unbind())
}

/// `edgesToWires(edges, tol)`.
#[pyfunction(name = "edgesToWires", signature = (edges, tol=1e-6))]
pub fn edges_to_wires(py: Python<'_>, edges: &Bound<'_, PyAny>, tol: f64) -> PyResult<Py<PyAny>> {
    let g: Vec<EdgeGeom> = shapes(edges)?.iter().flat_map(|s| s.edge_geoms()).collect();
    shape_list(
        py,
        make::chain_edges(g, Some(tol))
            .into_iter()
            .map(Shape::wire)
            .collect(),
    )
}

/// `loft(sections, cap, ruled)`.
#[pyfunction(name = "loft", signature = (s, cap=true, ruled=false))]
pub fn loft_fn(
    py: Python<'_>,
    s: &Bound<'_, PyAny>,
    cap: bool,
    ruled: bool,
) -> PyResult<Py<PyAny>> {
    let _ = cap;
    wrap(py, ops::loft(&loft_items(s)?, ruled).py()?)
}

/// `_cadrs.text(font_file, text, size, height, halign, valign, plane)`: text's glyphs, flat or
/// extruded.
#[pyfunction(name = "text")]
#[allow(clippy::too_many_arguments)]
pub fn text_fn(
    py: Python<'_>,
    font_file: &str,
    text: &str,
    size: f64,
    height: f64,
    halign: &str,
    valign: &str,
    plane: PyRef<'_, PyPlane>,
) -> PyResult<Py<PyAny>> {
    let font =
        std::fs::read(font_file).map_err(|e| PyValueError::new_err(format!("{font_file}: {e}")))?;
    wrap(
        py,
        make::text(&font, text, size, height, halign, valign, &plane.p).py()?,
    )
}

/// Registers the classes.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyShape>()?;
    m.add_class::<PyVertex>()?;
    m.add_class::<PyEdge>()?;
    m.add_class::<PyWire>()?;
    m.add_class::<PyFace>()?;
    m.add_class::<PyShell>()?;
    m.add_class::<PySolid>()?;
    m.add_class::<PyCompSolid>()?;
    m.add_class::<PyCompound>()?;
    let _ = (err, tuple3, kind_name);
    Ok(())
}
