//! `cadquery.selectors`: the selector classes, natively, and selectors written in Python (a
//! subclass of `Selector` overriding `filter`).

use crate::conv::{Py3, num, shape_list, shapes, v3};
use cadrs_kernel::cadquery::select::NthKey;
use cadrs_kernel::cadquery::{Selector, Shape};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

/// `cadquery.selectors.Selector`: the base class (keeps everything) and every native selector.
#[pyclass(name = "Selector", module = "cadquery.selectors", subclass, dict)]
pub struct PySelector {
    pub native: Option<Selector>,
    /// A combination of selectors that may be written in Python: (op, left, right).
    pub combo: Option<(u8, Py<PyAny>, Option<Py<PyAny>>)>,
}

/// The shapes a selector argument keeps: a string, a native selector, or any object with a
/// `filter` method.
pub fn selector_filter(
    py: Python<'_>,
    s: &Bound<'_, PyAny>,
    objs: Vec<Shape>,
) -> PyResult<Vec<Shape>> {
    if let Ok(st) = s.extract::<String>() {
        return Selector::parse(&st).py()?.filter(&objs).py();
    }
    if let Ok(ps) = s.cast::<PySelector>() {
        let base_filter = py.get_type::<PySelector>().getattr("filter")?;
        let own_filter = s.get_type().getattr("filter")?;
        if own_filter.is(&base_filter) {
            let p = ps.borrow();
            if let Some(n) = &p.native {
                return n.filter(&objs).py();
            }
            if let Some((op, l, r)) = &p.combo {
                let left = selector_filter(py, l.bind(py), objs.clone())?;
                let key = |v: &Vec<Shape>| {
                    v.iter()
                        .map(|s| s.key())
                        .collect::<std::collections::HashSet<_>>()
                };
                return Ok(match op {
                    0 => {
                        let rk = key(&selector_filter(py, r.as_ref().unwrap().bind(py), objs)?);
                        left.into_iter().filter(|s| rk.contains(&s.key())).collect()
                    }
                    1 => {
                        let mut v = left;
                        let seen = key(&v);
                        v.extend(
                            selector_filter(py, r.as_ref().unwrap().bind(py), objs)?
                                .into_iter()
                                .filter(|s| !seen.contains(&s.key())),
                        );
                        v
                    }
                    2 => {
                        let rk = key(&selector_filter(py, r.as_ref().unwrap().bind(py), objs)?);
                        left.into_iter()
                            .filter(|s| !rk.contains(&s.key()))
                            .collect()
                    }
                    _ => {
                        let lk = key(&left);
                        objs.into_iter()
                            .filter(|s| !lk.contains(&s.key()))
                            .collect()
                    }
                });
            }
            return Ok(objs);
        }
    }
    let list = shape_list(py, objs)?;
    let out = s.call_method1("filter", (list,))?;
    shapes(&out)
}

fn native(sel: Selector) -> PyClassInitializer<PySelector> {
    PyClassInitializer::from(PySelector {
        native: Some(sel),
        combo: None,
    })
}

fn combo(op: u8, l: Py<PyAny>, r: Option<Py<PyAny>>) -> PyClassInitializer<PySelector> {
    PyClassInitializer::from(PySelector {
        native: None,
        combo: Some((op, l, r)),
    })
}

#[pymethods]
impl PySelector {
    #[new]
    #[pyo3(signature = (*args, **kw))]
    fn new(args: &Bound<'_, PyTuple>, kw: Option<&Bound<'_, PyDict>>) -> Self {
        let _ = (args, kw);
        Self {
            native: Some(Selector::All),
            combo: None,
        }
    }
    #[pyo3(name = "filter")]
    fn filter(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        objectList: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let objs = shapes(objectList)?;
        // (Through the dispatch above, as if called from a workplane.)
        let p = slf.borrow();
        let kept = if let Some(n) = &p.native {
            n.filter(&objs).py()?
        } else {
            drop(p);
            let base = py.get_type::<PySelector>();
            let _ = base;
            selector_filter(py, slf.as_any(), objs)?
        };
        shape_list(py, kept)
    }
    fn __and__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        Ok(Py::new(
            slf.py(),
            combo(
                0,
                slf.clone().into_any().unbind(),
                Some(other.clone().unbind()),
            ),
        )?
        .into_any())
    }
    fn __add__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        Ok(Py::new(
            slf.py(),
            combo(
                1,
                slf.clone().into_any().unbind(),
                Some(other.clone().unbind()),
            ),
        )?
        .into_any())
    }
    fn __sub__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        Ok(Py::new(
            slf.py(),
            combo(
                2,
                slf.clone().into_any().unbind(),
                Some(other.clone().unbind()),
            ),
        )?
        .into_any())
    }
    fn __neg__(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        Ok(Py::new(slf.py(), combo(3, slf.clone().into_any().unbind(), None))?.into_any())
    }
}

macro_rules! sel_class {
    ($rs:ident, $name:literal) => {
        #[doc = concat!("`cadquery.selectors.", $name, "`.")]
        #[pyclass(name = $name, module = "cadquery.selectors", extends = PySelector, subclass, dict)]
        pub struct $rs {}
    };
}
sel_class!(PyStringSyntax, "StringSyntaxSelector");
sel_class!(PyDirection, "DirectionSelector");
sel_class!(PyParallel, "ParallelDirSelector");
sel_class!(PyPerpendicular, "PerpendicularDirSelector");
sel_class!(PyType, "TypeSelector");
sel_class!(PyDirMinMax, "DirectionMinMaxSelector");
sel_class!(PyDirNth, "DirectionNthSelector");
sel_class!(PyCenterNth, "CenterNthSelector");
sel_class!(PyRadiusNth, "RadiusNthSelector");
sel_class!(PyLengthNth, "LengthNthSelector");
sel_class!(PyAreaNth, "AreaNthSelector");
sel_class!(PyNearest, "NearestToPointSelector");
sel_class!(PyBox, "BoxSelector");
sel_class!(PyAnd, "AndSelector");
sel_class!(PySum, "SumSelector");
sel_class!(PySub, "SubtractSelector");
sel_class!(PyInv, "InverseSelector");

#[pymethods]
impl PyStringSyntax {
    #[new]
    #[pyo3(signature = (selectorString))]
    #[allow(non_snake_case)]
    fn new(selectorString: &str) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::parse(selectorString).py()?).add_subclass(Self {}))
    }
}

#[pymethods]
impl PyDirection {
    #[new]
    #[pyo3(signature = (vector, tolerance=1e-4))]
    fn new(vector: &Bound<'_, PyAny>, tolerance: f64) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::Direction {
            dir: v3(vector)?,
            tol: tolerance,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyParallel {
    #[new]
    #[pyo3(signature = (vector, tolerance=1e-4))]
    fn new(vector: &Bound<'_, PyAny>, tolerance: f64) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::Parallel {
            dir: v3(vector)?,
            tol: tolerance,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyPerpendicular {
    #[new]
    #[pyo3(signature = (vector, tolerance=1e-4))]
    fn new(vector: &Bound<'_, PyAny>, tolerance: f64) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::Perpendicular {
            dir: v3(vector)?,
            tol: tolerance,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyType {
    #[new]
    #[pyo3(signature = (typeString))]
    #[allow(non_snake_case)]
    fn new(typeString: &str) -> PyClassInitializer<Self> {
        native(Selector::Type(typeString.to_uppercase())).add_subclass(Self {})
    }
}

#[pymethods]
impl PyDirMinMax {
    #[new]
    #[pyo3(signature = (vector, directionMax=true, tolerance=1e-4))]
    #[allow(non_snake_case)]
    fn new(
        vector: &Bound<'_, PyAny>,
        directionMax: bool,
        tolerance: f64,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::Nth {
            key: NthKey::Center(v3(vector)?),
            n: -1,
            max: directionMax,
            tol: tolerance,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyDirNth {
    #[new]
    #[pyo3(signature = (vector, n, directionMax=true, tolerance=1e-4))]
    #[allow(non_snake_case)]
    fn new(
        vector: &Bound<'_, PyAny>,
        n: i64,
        directionMax: bool,
        tolerance: f64,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::DirectionNth {
            dir: v3(vector)?,
            n,
            max: directionMax,
            tol: tolerance,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyCenterNth {
    #[new]
    #[pyo3(signature = (vector, n, directionMax=true, tolerance=1e-4))]
    #[allow(non_snake_case)]
    fn new(
        vector: &Bound<'_, PyAny>,
        n: i64,
        directionMax: bool,
        tolerance: f64,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::Nth {
            key: NthKey::Center(v3(vector)?),
            n,
            max: directionMax,
            tol: tolerance,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyRadiusNth {
    #[new]
    #[pyo3(signature = (n, directionMax=true, tolerance=1e-4))]
    #[allow(non_snake_case)]
    fn new(n: i64, directionMax: bool, tolerance: f64) -> PyClassInitializer<Self> {
        native(Selector::Nth {
            key: NthKey::Radius,
            n,
            max: directionMax,
            tol: tolerance,
        })
        .add_subclass(Self {})
    }
}

#[pymethods]
impl PyLengthNth {
    #[new]
    #[pyo3(signature = (n, directionMax=true, tolerance=1e-4))]
    #[allow(non_snake_case)]
    fn new(n: i64, directionMax: bool, tolerance: f64) -> PyClassInitializer<Self> {
        native(Selector::Nth {
            key: NthKey::Length,
            n,
            max: directionMax,
            tol: tolerance,
        })
        .add_subclass(Self {})
    }
}

#[pymethods]
impl PyAreaNth {
    #[new]
    #[pyo3(signature = (n, directionMax=true, tolerance=1e-4))]
    #[allow(non_snake_case)]
    fn new(n: i64, directionMax: bool, tolerance: f64) -> PyClassInitializer<Self> {
        native(Selector::Nth {
            key: NthKey::Area,
            n,
            max: directionMax,
            tol: tolerance,
        })
        .add_subclass(Self {})
    }
}

#[pymethods]
impl PyNearest {
    #[new]
    #[pyo3(signature = (pnt))]
    fn new(pnt: &Bound<'_, PyAny>) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::NearestToPoint(v3(pnt)?)).add_subclass(Self {}))
    }
}

#[pymethods]
impl PyBox {
    #[new]
    #[pyo3(signature = (point0, point1, boundingbox=false))]
    fn new(
        point0: &Bound<'_, PyAny>,
        point1: &Bound<'_, PyAny>,
        boundingbox: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(native(Selector::InBox {
            p0: v3(point0)?,
            p1: v3(point1)?,
            bbox: boundingbox,
        })
        .add_subclass(Self {}))
    }
}

#[pymethods]
impl PyAnd {
    #[new]
    fn new(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> PyClassInitializer<Self> {
        combo(0, left.clone().unbind(), Some(right.clone().unbind())).add_subclass(Self {})
    }
}

#[pymethods]
impl PySum {
    #[new]
    fn new(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> PyClassInitializer<Self> {
        combo(1, left.clone().unbind(), Some(right.clone().unbind())).add_subclass(Self {})
    }
}

#[pymethods]
impl PySub {
    #[new]
    fn new(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> PyClassInitializer<Self> {
        combo(2, left.clone().unbind(), Some(right.clone().unbind())).add_subclass(Self {})
    }
}

#[pymethods]
impl PyInv {
    #[new]
    fn new(selector: &Bound<'_, PyAny>) -> PyClassInitializer<Self> {
        combo(3, selector.clone().unbind(), None).add_subclass(Self {})
    }
}

/// Registers the classes.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySelector>()?;
    m.add_class::<PyStringSyntax>()?;
    m.add_class::<PyDirection>()?;
    m.add_class::<PyParallel>()?;
    m.add_class::<PyPerpendicular>()?;
    m.add_class::<PyType>()?;
    m.add_class::<PyDirMinMax>()?;
    m.add_class::<PyDirNth>()?;
    m.add_class::<PyCenterNth>()?;
    m.add_class::<PyRadiusNth>()?;
    m.add_class::<PyLengthNth>()?;
    m.add_class::<PyAreaNth>()?;
    m.add_class::<PyNearest>()?;
    m.add_class::<PyBox>()?;
    m.add_class::<PyAnd>()?;
    m.add_class::<PySum>()?;
    m.add_class::<PySub>()?;
    m.add_class::<PyInv>()?;
    let _ = num;
    Ok(())
}
