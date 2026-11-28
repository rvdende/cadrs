//! Conversions between Python values and the module's types, and its errors as exceptions.

use crate::geom::{PyLocation, PyPlane, PyVector};
use crate::shapes::{PyShape, wrap};
use cadrs_kernel::cadquery::{CqError, Obj, Shape};
use nalgebra::Vector3;
use pyo3::exceptions::{
    PyIndexError, PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyList, PySequence, PyString, PyTuple};

pub type V3 = Vector3<f64>;

/// A module error as the exception CadQuery raises.
pub fn err(e: CqError) -> PyErr {
    match e {
        CqError::Value(m) => PyValueError::new_err(m),
        CqError::Index(m) => PyIndexError::new_err(m),
        CqError::NotImplemented(m) => PyNotImplementedError::new_err(m),
        // As OCP's: StdFail_NotDone and Standard_Failure (each straight from Exception), so a
        // program's `except OCP.Standard.Standard_Failure` catches what it catches of OCCT's.
        CqError::Kernel(m) | CqError::Empty(m) => ocp_err("StdFail", "StdFail_NotDone", m),
        CqError::Failure(m) => ocp_err("Standard", "Standard_Failure", m),
    }
}

/// The shim's exception `OCP.<module>.<name>` with message `m` (a RuntimeError where the shim
/// can't be imported).
fn ocp_err(module: &str, name: &str, m: String) -> PyErr {
    Python::attach(|py| {
        py.import(format!("OCP.{module}"))
            .and_then(|md| md.getattr(name))
            .and_then(|t| t.call1((m.clone(),)))
            .map(PyErr::from_value)
            .unwrap_or_else(|_| PyRuntimeError::new_err(format!("{name}: {m}")))
    })
}

/// `Result` of the module into Python's.
pub trait Py3<T> {
    /// As a Python result.
    fn py(self) -> PyResult<T>;
}

impl<T> Py3<T> for Result<T, CqError> {
    fn py(self) -> PyResult<T> {
        self.map_err(err)
    }
}

/// A number (int, float or anything with `__float__`).
pub fn num(o: &Bound<'_, PyAny>) -> PyResult<f64> {
    o.extract::<f64>()
        .map_err(|_| PyTypeError::new_err(format!("a number is required, not {}", type_name(o))))
}

/// The type's name of a value.
pub fn type_name(o: &Bound<'_, PyAny>) -> String {
    o.get_type()
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "?".into())
}

/// A vector from a `Vector`, a vertex, a 2- or 3-sequence of numbers, or an object with
/// `X()`, `Y()`, `Z()` (OCP's `gp_Pnt`).
pub fn v3(o: &Bound<'_, PyAny>) -> PyResult<V3> {
    if let Ok(v) = o.cast::<PyVector>() {
        return Ok(v.borrow().v);
    }
    if let Ok(s) = o.cast::<PyShape>() {
        return Ok(s.borrow().s.center());
    }
    if !o.is_instance_of::<PyString>()
        && let Ok(seq) = o.cast::<PySequence>()
    {
        let n = seq.len()?;
        if n == 2 || n == 3 {
            let x = num(&seq.get_item(0)?)?;
            let y = num(&seq.get_item(1)?)?;
            let z = if n == 3 { num(&seq.get_item(2)?)? } else { 0.0 };
            return Ok(V3::new(x, y, z));
        }
    }
    // Anything indexable of length 2 or 3 (numpy arrays).
    if !o.is_instance_of::<PyString>()
        && let Ok(n) = o.len()
        && (n == 2 || n == 3)
        && let (Ok(x), Ok(y)) = (o.get_item(0), o.get_item(1))
    {
        let z = if n == 3 { num(&o.get_item(2)?)? } else { 0.0 };
        return Ok(V3::new(num(&x)?, num(&y)?, z));
    }
    if o.hasattr("X")? && o.hasattr("Y")? && o.hasattr("Z")? {
        return Ok(V3::new(
            num(&o.call_method0("X")?)?,
            num(&o.call_method0("Y")?)?,
            num(&o.call_method0("Z")?)?,
        ));
    }
    if o.hasattr("toTuple")? {
        return v3(&o.call_method0("toTuple")?);
    }
    Err(PyTypeError::new_err(format!(
        "Expected three floats, OCC gp_, or 3-tuple, not {}",
        type_name(o)
    )))
}

/// An optional vector (None stays None).
pub fn v3_opt(o: Option<&Bound<'_, PyAny>>) -> PyResult<Option<V3>> {
    match o {
        None => Ok(None),
        Some(o) if o.is_none() => Ok(None),
        Some(o) => v3(o).map(Some),
    }
}

/// A Python `Vector`.
pub fn vector(py: Python<'_>, v: V3) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, PyVector { v })?.into_any())
}

/// A stack object as Python.
pub fn obj_py(py: Python<'_>, o: &Obj) -> PyResult<Py<PyAny>> {
    match o {
        Obj::Vector(v) => vector(py, *v),
        Obj::Location(l) => Ok(Py::new(py, PyLocation { l: *l })?.into_any()),
        Obj::Shape(s) => wrap(py, s.clone()),
    }
}

/// A Python value as a stack object (vectors, tuples, locations, shapes).
pub fn py_obj(o: &Bound<'_, PyAny>) -> PyResult<Obj> {
    if let Ok(s) = o.cast::<PyShape>() {
        return Ok(Obj::Shape(s.borrow().s.clone()));
    }
    if let Ok(l) = o.cast::<PyLocation>() {
        return Ok(Obj::Location(l.borrow().l));
    }
    if let Ok(p) = o.cast::<PyPlane>() {
        return Ok(Obj::Vector(p.borrow().p.origin));
    }
    Ok(Obj::Vector(v3(o)?))
}

/// A shape argument.
pub fn shape(o: &Bound<'_, PyAny>) -> PyResult<Shape> {
    match o.cast::<PyShape>() {
        Ok(s) => Ok(s.borrow().s.clone()),
        Err(_) => {
            // OCP's stand-ins (`shape.wrapped`, `BRepBuilderAPI_MakeEdge(...).Edge()`) hold one.
            if let Ok(inner) = o.getattr("_cq_shape") {
                return shape(&inner);
            }
            Err(PyTypeError::new_err(format!(
                "a Shape is required, not {}",
                type_name(o)
            )))
        }
    }
}

/// Shapes from a list (or any iterable) of shapes.
pub fn shapes(o: &Bound<'_, PyAny>) -> PyResult<Vec<Shape>> {
    let mut v = Vec::new();
    for item in o.try_iter()? {
        v.push(shape(&item?)?);
    }
    Ok(v)
}

/// A list of shapes as Python.
pub fn shape_list(py: Python<'_>, v: Vec<Shape>) -> PyResult<Py<PyAny>> {
    let items: Vec<Py<PyAny>> = v
        .into_iter()
        .map(|s| wrap(py, s))
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, items)?.into_any().unbind())
}

/// `centered`: a bool or a tuple of bools.
pub fn centered3(o: Option<&Bound<'_, PyAny>>, default: bool) -> PyResult<(bool, bool, bool)> {
    match o {
        None => Ok((default, default, default)),
        Some(o) => {
            if let Ok(b) = o.extract::<bool>() {
                return Ok((b, b, b));
            }
            let t = o
                .cast::<PySequence>()
                .map_err(|_| PyTypeError::new_err("centered must be a bool or a tuple"))?;
            let n = t.len()?;
            let g = |i: usize| -> PyResult<bool> {
                if i < n {
                    t.get_item(i)?.is_truthy()
                } else {
                    Ok(default)
                }
            };
            Ok((g(0)?, g(1)?, g(2)?))
        }
    }
}

/// Positional points: each a vector (plane coordinates) or a location.
pub fn points(o: &Bound<'_, PyAny>) -> PyResult<Vec<V3>> {
    let mut v = Vec::new();
    for item in o.try_iter()? {
        v.push(v3(&item?)?);
    }
    Ok(v)
}

/// A tuple of floats.
pub fn tuple3(py: Python<'_>, v: V3) -> PyResult<Py<PyAny>> {
    Ok(PyTuple::new(py, [v.x, v.y, v.z])?.into_any().unbind())
}
