//! `Vector`, `Plane`, `Location` and `BoundBox`.

use crate::conv::{Py3, V3, num, type_name, v3, vector};
use crate::shapes::{PyShape, wrap};
use cadrs_kernel::cadquery::{BoundBox, Location, Plane};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyTuple, PyType};

/// `cadquery.Vector`.
#[pyclass(name = "Vector", module = "cadquery", subclass)]
#[derive(Clone)]
pub struct PyVector {
    pub v: V3,
}

fn fmt_num(x: f64) -> String {
    let r = format!("{x:?}");
    if r == "-0.0" { "-0.0".into() } else { r }
}

#[pymethods]
impl PyVector {
    #[new]
    #[pyo3(signature = (*args))]
    fn new(args: &Bound<'_, PyTuple>) -> PyResult<Self> {
        let v = match args.len() {
            0 => V3::zeros(),
            1 => v3(&args.get_item(0)?)?,
            2 => V3::new(num(&args.get_item(0)?)?, num(&args.get_item(1)?)?, 0.0),
            3 => V3::new(
                num(&args.get_item(0)?)?,
                num(&args.get_item(1)?)?,
                num(&args.get_item(2)?)?,
            ),
            _ => {
                return Err(PyTypeError::new_err(
                    "Expected three floats, OCC gp_, or 3-tuple",
                ));
            }
        };
        Ok(Self { v })
    }
    #[getter]
    fn x(&self) -> f64 {
        self.v.x
    }
    #[setter]
    fn set_x(&mut self, x: f64) {
        self.v.x = x;
    }
    #[getter]
    fn y(&self) -> f64 {
        self.v.y
    }
    #[setter]
    fn set_y(&mut self, y: f64) {
        self.v.y = y;
    }
    #[getter]
    fn z(&self) -> f64 {
        self.v.z
    }
    #[setter]
    fn set_z(&mut self, z: f64) {
        self.v.z = z;
    }
    #[getter(Length)]
    fn length(&self) -> f64 {
        self.v.norm()
    }
    #[getter]
    fn wrapped(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    #[pyo3(name = "toTuple")]
    fn to_tuple(&self) -> (f64, f64, f64) {
        (self.v.x, self.v.y, self.v.z)
    }
    #[pyo3(name = "toPnt")]
    fn to_pnt(&self) -> Self {
        self.clone()
    }
    #[pyo3(name = "toDir")]
    fn to_dir(&self) -> Self {
        Self {
            v: self.v.normalize(),
        }
    }
    #[pyo3(name = "toVec")]
    fn to_vec(&self) -> Self {
        self.clone()
    }
    #[allow(non_snake_case)]
    fn X(&self) -> f64 {
        self.v.x
    }
    #[allow(non_snake_case)]
    fn Y(&self) -> f64 {
        self.v.y
    }
    #[allow(non_snake_case)]
    fn Z(&self) -> f64 {
        self.v.z
    }
    fn cross(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            v: self.v.cross(&v3(o)?),
        })
    }
    fn dot(&self, o: &Bound<'_, PyAny>) -> PyResult<f64> {
        Ok(self.v.dot(&v3(o)?))
    }
    fn sub(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self { v: self.v - v3(o)? })
    }
    fn add(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self { v: self.v + v3(o)? })
    }
    fn multiply(&self, s: f64) -> Self {
        Self { v: self.v * s }
    }
    fn normalized(&self) -> PyResult<Self> {
        let n = self.v.norm();
        if n == 0.0 {
            return Err(PyValueError::new_err("Cannot normalize null vector"));
        }
        Ok(Self { v: self.v / n })
    }
    #[pyo3(name = "Center")]
    fn center(&self) -> Self {
        self.clone()
    }
    #[pyo3(name = "getAngle")]
    fn get_angle(&self, o: &Bound<'_, PyAny>) -> PyResult<f64> {
        let w = v3(o)?;
        Ok(self.v.cross(&w).norm().atan2(self.v.dot(&w)))
    }
    #[pyo3(name = "getSignedAngle", signature = (o, normal=None))]
    fn get_signed_angle(
        &self,
        o: &Bound<'_, PyAny>,
        normal: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<f64> {
        let w = v3(o)?;
        let n = match normal {
            Some(n) => v3(n)?,
            None => V3::z(),
        };
        let a = self.v.cross(&w).norm().atan2(self.v.dot(&w));
        Ok(if self.v.cross(&w).dot(&n) < 0.0 {
            -a
        } else {
            a
        })
    }
    #[pyo3(name = "projectToLine")]
    fn project_to_line(&self, line: &Bound<'_, PyAny>) -> PyResult<Self> {
        let l = v3(line)?;
        Ok(Self {
            v: l * (self.v.dot(&l) / l.norm_squared()),
        })
    }
    #[pyo3(name = "distanceToPlane")]
    fn distance_to_plane(&self, p: PyRef<'_, PyPlane>) -> f64 {
        (self.v - p.p.origin).dot(&p.p.z_dir).abs()
    }
    #[pyo3(name = "signedDistanceFromPlane")]
    fn signed_distance(&self, p: PyRef<'_, PyPlane>) -> f64 {
        (self.v - p.p.origin).dot(&p.p.z_dir)
    }
    #[pyo3(name = "projectToPlane")]
    fn project_to_plane(&self, p: PyRef<'_, PyPlane>) -> Self {
        let n = p.p.z_dir;
        Self {
            v: self.v - n * (self.v - p.p.origin).dot(&n),
        }
    }
    fn transform(&self, _m: &Bound<'_, PyAny>) -> PyResult<Self> {
        Err(PyTypeError::new_err(
            "Vector.transform with a Matrix is not supported",
        ))
    }
    fn __add__(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        self.add(o)
    }
    fn __radd__(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(0.0) = o.extract::<f64>() {
            return Ok(self.clone());
        }
        self.add(o)
    }
    fn __sub__(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        self.sub(o)
    }
    fn __rsub__(&self, o: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self { v: v3(o)? - self.v })
    }
    fn __mul__(&self, s: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            v: self.v * num(s)?,
        })
    }
    fn __rmul__(&self, s: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            v: self.v * num(s)?,
        })
    }
    fn __truediv__(&self, s: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            v: self.v / num(s)?,
        })
    }
    fn __neg__(&self) -> Self {
        Self { v: -self.v }
    }
    fn __abs__(&self) -> f64 {
        self.v.norm()
    }
    fn __eq__(&self, o: &Bound<'_, PyAny>) -> bool {
        match o.cast::<PyVector>() {
            Ok(w) => (self.v - w.borrow().v).norm() <= 1e-5,
            Err(_) => false,
        }
    }
    fn __hash__(&self) -> u64 {
        // (Equal vectors compare within 1e-5, so they hash by their rounded coordinates.)
        let r = |x: f64| (x * 1e4).round() as i64 as u64;
        r(self.v.x)
            .wrapping_mul(31)
            .wrapping_add(r(self.v.y))
            .wrapping_mul(31)
            .wrapping_add(r(self.v.z))
    }
    fn __repr__(&self) -> String {
        format!(
            "Vector: ({}, {}, {})",
            fmt_num(self.v.x),
            fmt_num(self.v.y),
            fmt_num(self.v.z)
        )
    }
    fn __str__(&self) -> String {
        self.__repr__()
    }
    fn __copy__(&self) -> Self {
        self.clone()
    }
    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.clone()
    }
    fn __reduce__(slf: &Bound<'_, Self>) -> PyResult<(Py<PyAny>, (f64, f64, f64))> {
        let v = slf.borrow().v;
        Ok((slf.get_type().into_any().unbind(), (v.x, v.y, v.z)))
    }
}

/// `cadquery.Plane`.
#[pyclass(name = "Plane", module = "cadquery", subclass)]
#[derive(Clone)]
pub struct PyPlane {
    pub p: Plane,
}

fn named_plane(
    name: &str,
    origin: Option<&Bound<'_, PyAny>>,
    x_dir: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyPlane> {
    let o = match origin {
        Some(o) => v3(o)?,
        None => V3::zeros(),
    };
    let mut p = Plane::named(name, o).py()?;
    if let Some(x) = x_dir
        && !x.is_none()
    {
        p = Plane::of_axes(p.origin, v3(x)?, p.z_dir);
    }
    Ok(PyPlane { p })
}

#[pymethods]
impl PyPlane {
    #[new]
    #[pyo3(signature = (origin=None, xDir=None, normal=None))]
    #[allow(non_snake_case)]
    fn new(
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
        normal: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        if let Some(o) = origin
            && let Ok(l) = o.cast::<PyLocation>()
        {
            return Ok(Self {
                p: Plane::of_location(&l.borrow().l),
            });
        }
        let o = match origin {
            Some(o) => v3(o)?,
            None => V3::zeros(),
        };
        let n = match normal {
            Some(n) if !n.is_none() => v3(n)?,
            _ => V3::z(),
        };
        let x = match xDir {
            Some(x) if !x.is_none() => Some(v3(x)?),
            _ => None,
        };
        Ok(Self {
            p: Plane::new(o, x, n).py()?,
        })
    }
    #[classmethod]
    #[pyo3(signature = (stdName, origin=None))]
    #[allow(non_snake_case)]
    fn named(
        _c: &Bound<'_, PyType>,
        stdName: &str,
        origin: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane(stdName, origin, None)
    }
    #[classmethod]
    #[pyo3(name = "XY", signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn xy(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("XY", origin, xDir)
    }
    #[classmethod]
    #[pyo3(name = "YZ", signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn yz(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("YZ", origin, xDir)
    }
    #[classmethod]
    #[pyo3(name = "ZX", signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn zx(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("ZX", origin, xDir)
    }
    #[classmethod]
    #[pyo3(name = "XZ", signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn xz(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("XZ", origin, xDir)
    }
    #[classmethod]
    #[pyo3(name = "YX", signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn yx(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("YX", origin, xDir)
    }
    #[classmethod]
    #[pyo3(name = "ZY", signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn zy(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("ZY", origin, xDir)
    }
    #[classmethod]
    #[pyo3(signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn front(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("front", origin, xDir)
    }
    #[classmethod]
    #[pyo3(signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn back(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("back", origin, xDir)
    }
    #[classmethod]
    #[pyo3(signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn left(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("left", origin, xDir)
    }
    #[classmethod]
    #[pyo3(signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn right(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("right", origin, xDir)
    }
    #[classmethod]
    #[pyo3(signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn top(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("top", origin, xDir)
    }
    #[classmethod]
    #[pyo3(signature = (origin=None, xDir=None))]
    #[allow(non_snake_case)]
    fn bottom(
        _c: &Bound<'_, PyType>,
        origin: Option<&Bound<'_, PyAny>>,
        xDir: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        named_plane("bottom", origin, xDir)
    }
    #[getter]
    fn origin(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.p.origin)
    }
    #[setter]
    fn set_origin(&mut self, o: &Bound<'_, PyAny>) -> PyResult<()> {
        self.p.origin = v3(o)?;
        Ok(())
    }
    #[getter(xDir)]
    fn x_dir(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.p.x_dir)
    }
    #[setter(xDir)]
    fn set_x_dir(&mut self, o: &Bound<'_, PyAny>) -> PyResult<()> {
        self.p = Plane::of_axes(self.p.origin, v3(o)?, self.p.z_dir);
        Ok(())
    }
    #[getter(yDir)]
    fn y_dir(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.p.y_dir)
    }
    #[getter(zDir)]
    fn z_dir(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.p.z_dir)
    }
    #[setter(zDir)]
    fn set_z_dir(&mut self, o: &Bound<'_, PyAny>) -> PyResult<()> {
        self.p = Plane::of_axes(self.p.origin, self.p.x_dir, v3(o)?.normalize());
        Ok(())
    }
    #[pyo3(name = "setOrigin2d")]
    fn set_origin_2d(&mut self, x: f64, y: f64) {
        self.p.set_origin_2d(x, y);
    }
    #[pyo3(name = "toWorldCoords")]
    fn to_world_coords(&self, py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        vector(py, self.p.to_world(v3(o)?))
    }
    #[pyo3(name = "toLocalCoords")]
    fn to_local_coords(&self, py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        if let Ok(s) = o.cast::<PyShape>() {
            let m = self.p.to_local_map();
            return wrap(py, s.borrow().s.transformed(&m).py()?);
        }
        vector(py, self.p.to_local(v3(o)?))
    }
    #[pyo3(signature = (rotate=None))]
    fn rotated(&self, rotate: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let r = match rotate {
            Some(r) => v3(r)?,
            None => V3::zeros(),
        };
        Ok(Self {
            p: self.p.rotated(r).py()?,
        })
    }
    #[getter]
    fn location(&self) -> PyLocation {
        PyLocation {
            l: self.p.location(),
        }
    }
    #[pyo3(name = "mirrorInPlane", signature = (shapes, axis="X"))]
    fn mirror_in_plane(
        &self,
        py: Python<'_>,
        shapes: &Bound<'_, PyAny>,
        axis: &str,
    ) -> PyResult<Py<PyAny>> {
        let (x, y, _) = self.p.frame();
        let a = if axis == "Y" { y } else { x };
        let o = self.p.origin;
        // A half turn about the in-plane axis (OCCT's mirror in an axis).
        let m = 2.0 * a * a.transpose() - nalgebra::Matrix3::identity();
        let aff = cadrs_kernel::cadquery::Affine { m, t: o - m * o };
        let v: Vec<_> = crate::conv::shapes(shapes)?
            .iter()
            .map(|s| s.transformed(&aff))
            .collect::<Result<_, _>>()
            .py()?;
        crate::conv::shape_list(py, v)
    }
    fn __eq__(&self, o: &Bound<'_, PyAny>) -> bool {
        o.cast::<PyPlane>()
            .is_ok_and(|q| self.p.same(&q.borrow().p))
    }
    fn __repr__(&self) -> String {
        let t = |v: V3| format!("({}, {}, {})", fmt_num(v.x), fmt_num(v.y), fmt_num(v.z));
        format!(
            "Plane(origin={}, xDir={}, normal={})",
            t(self.p.origin),
            t(self.p.x_dir),
            t(self.p.z_dir)
        )
    }
    fn __copy__(&self) -> Self {
        self.clone()
    }
    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.clone()
    }
}

/// `cadquery.Location`.
#[pyclass(name = "Location", module = "cadquery", subclass)]
#[derive(Clone)]
pub struct PyLocation {
    pub l: Location,
}

#[pymethods]
impl PyLocation {
    #[new]
    #[pyo3(signature = (*args, **kw))]
    pub fn py_new(
        args: &Bound<'_, PyTuple>,
        kw: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) -> PyResult<Self> {
        let n = args.len();
        let a = |i: usize| args.get_item(i);
        let is_num = |o: &Bound<'_, PyAny>| {
            o.extract::<f64>().is_ok() && !o.is_instance_of::<pyo3::types::PyBool>()
                || o.extract::<f64>().is_ok()
        };
        if let Some(kw) = kw
            && !kw.is_empty()
        {
            let g = |k: &str, i: usize| -> PyResult<f64> {
                match kw.get_item(k)? {
                    Some(v) => num(&v),
                    None if i < n => num(&args.get_item(i)?),
                    None => Ok(0.0),
                }
            };
            return Ok(Self {
                l: Location::from_xyz_angles(
                    V3::new(g("x", 0)?, g("y", 1)?, g("z", 2)?),
                    V3::new(g("rx", 3)?, g("ry", 4)?, g("rz", 5)?),
                ),
            });
        }
        let l = match n {
            0 => Location::default(),
            1 => {
                let o = a(0)?;
                if let Ok(p) = o.cast::<PyPlane>() {
                    Location::of_plane(&p.borrow().p)
                } else if let Ok(l) = o.cast::<PyLocation>() {
                    l.borrow().l
                } else if is_num(&o) {
                    Location::translation(V3::new(num(&o)?, 0.0, 0.0))
                } else {
                    Location::translation(v3(&o)?)
                }
            }
            2 => {
                let (o0, o1) = (a(0)?, a(1)?);
                if let Ok(p) = o0.cast::<PyPlane>() {
                    Location::of_plane_at(&p.borrow().p, v3(&o1)?)
                } else if is_num(&o0) {
                    Location::translation(V3::new(num(&o0)?, num(&o1)?, 0.0))
                } else {
                    Location::from_xyz_angles(v3(&o0)?, v3(&o1)?)
                }
            }
            3 => {
                let o0 = a(0)?;
                if is_num(&o0) {
                    Location::translation(V3::new(num(&o0)?, num(&a(1)?)?, num(&a(2)?)?))
                } else {
                    Location::from_axis_angle(v3(&o0)?, v3(&a(1)?)?, num(&a(2)?)?)
                }
            }
            4..=6 => {
                let g = |i: usize| -> PyResult<f64> {
                    if i < n {
                        num(&args.get_item(i)?)
                    } else {
                        Ok(0.0)
                    }
                };
                Location::from_xyz_angles(
                    V3::new(g(0)?, g(1)?, g(2)?),
                    V3::new(g(3)?, g(4)?, g(5)?),
                )
            }
            _ => return Err(PyTypeError::new_err("Location: too many arguments")),
        };
        Ok(Self { l })
    }
    #[getter]
    fn inverse(&self) -> Self {
        Self {
            l: self.l.inverse(),
        }
    }
    fn __mul__(&self, o: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = o.py();
        if let Ok(other) = o.cast::<PyLocation>() {
            return Ok(Py::new(
                py,
                Self {
                    l: self.l.then_after(&other.borrow().l),
                },
            )?
            .into_any());
        }
        if let Ok(s) = o.cast::<PyShape>() {
            return wrap(py, s.borrow().s.moved(&self.l).py()?);
        }
        Err(PyTypeError::new_err(format!(
            "unsupported operand type for *: 'Location' and '{}'",
            type_name(o)
        )))
    }
    fn __pow__(&self, e: i32, _m: Option<&Bound<'_, PyAny>>) -> Self {
        Self { l: self.l.pow(e) }
    }
    #[pyo3(name = "toTuple")]
    fn to_tuple(&self) -> ((f64, f64, f64), (f64, f64, f64)) {
        let (t, r) = self.l.to_tuple();
        ((t.x, t.y, t.z), (r.x, r.y, r.z))
    }
    #[getter]
    fn plane(&self) -> PyPlane {
        PyPlane {
            p: Plane::of_location(&self.l),
        }
    }
    #[getter]
    fn wrapped(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __repr__(&self) -> String {
        let (t, r) = self.l.to_tuple();
        format!(
            "Location(({}, {}, {}), ({}, {}, {}))",
            t.x, t.y, t.z, r.x, r.y, r.z
        )
    }
    fn __eq__(&self, o: &Bound<'_, PyAny>) -> bool {
        o.cast::<PyLocation>().is_ok_and(|q| {
            let q = q.borrow().l;
            (q.0.m - self.l.0.m).norm() < 1e-9 && (q.0.t - self.l.0.t).norm() < 1e-9
        })
    }
    fn __copy__(&self) -> Self {
        self.clone()
    }
    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.clone()
    }
}

/// `cadquery.BoundBox`.
#[pyclass(name = "BoundBox", module = "cadquery")]
#[derive(Clone)]
pub struct PyBoundBox {
    pub b: BoundBox,
}

#[pymethods]
impl PyBoundBox {
    #[getter]
    fn xmin(&self) -> f64 {
        self.b.min.x
    }
    #[getter]
    fn ymin(&self) -> f64 {
        self.b.min.y
    }
    #[getter]
    fn zmin(&self) -> f64 {
        self.b.min.z
    }
    #[getter]
    fn xmax(&self) -> f64 {
        self.b.max.x
    }
    #[getter]
    fn ymax(&self) -> f64 {
        self.b.max.y
    }
    #[getter]
    fn zmax(&self) -> f64 {
        self.b.max.z
    }
    #[getter]
    fn xlen(&self) -> f64 {
        self.b.max.x - self.b.min.x
    }
    #[getter]
    fn ylen(&self) -> f64 {
        self.b.max.y - self.b.min.y
    }
    #[getter]
    fn zlen(&self) -> f64 {
        self.b.max.z - self.b.min.z
    }
    #[getter]
    fn center(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        vector(py, self.b.center())
    }
    #[getter(DiagonalLength)]
    fn diagonal(&self) -> f64 {
        self.b.diagonal()
    }
    #[pyo3(signature = (obj, tol=None))]
    fn add(&self, obj: &Bound<'_, PyAny>, tol: Option<f64>) -> PyResult<Self> {
        let _ = tol;
        let other = if let Ok(b) = obj.cast::<PyBoundBox>() {
            b.borrow().b
        } else {
            let p = v3(obj)?;
            BoundBox { min: p, max: p }
        };
        Ok(Self {
            b: self.b.union(&other),
        })
    }
    fn enlarge(&self, tol: f64) -> Self {
        Self {
            b: self.b.enlarged(tol),
        }
    }
    #[pyo3(name = "isInside")]
    fn is_inside(&self, o: PyRef<'_, PyBoundBox>) -> bool {
        self.b.is_inside(&o.b)
    }
    #[staticmethod]
    #[pyo3(name = "findOutsideBox2D")]
    fn find_outside_box_2d(b1: PyRef<'_, PyBoundBox>, b2: PyRef<'_, PyBoundBox>) -> Option<Self> {
        let (a, b) = (b1.b, b2.b);
        if a.min.x < b.min.x && a.max.x > b.max.x && a.min.y < b.min.y && a.max.y > b.max.y {
            return Some(Self { b: a });
        }
        if b.min.x < a.min.x && b.max.x > a.max.x && b.min.y < a.min.y && b.max.y > a.max.y {
            return Some(Self { b });
        }
        None
    }
    fn __repr__(&self) -> String {
        format!(
            "BoundBox({:?} .. {:?})",
            self.b.min.as_slice(),
            self.b.max.as_slice()
        )
    }
}
