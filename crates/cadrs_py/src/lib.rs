//! `_cadrs`: the native part of the `cadquery` package cadrs gives Python (see
//! `python/cadquery`): [`cadrs_kernel::cadquery`]'s Workplane, shapes, selectors and geometry as
//! CadQuery's classes, so CadQuery programs run on cadrs's kernel unchanged.

mod conv;
mod geom;
mod sel;
mod shapes;
mod wp;

use pyo3::prelude::*;

#[pymodule]
fn _cadrs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<geom::PyVector>()?;
    m.add_class::<geom::PyPlane>()?;
    m.add_class::<geom::PyLocation>()?;
    m.add_class::<geom::PyBoundBox>()?;
    shapes::register(m)?;
    sel::register(m)?;
    m.add_class::<wp::PyWorkplane>()?;
    m.add_function(wrap_pyfunction!(shapes::wires_to_faces, m)?)?;
    m.add_function(wrap_pyfunction!(shapes::sort_wires_by_build_order, m)?)?;
    m.add_function(wrap_pyfunction!(shapes::edges_to_wires, m)?)?;
    m.add_function(wrap_pyfunction!(shapes::loft_fn, m)?)?;
    m.add_function(wrap_pyfunction!(shapes::text_fn, m)?)?;
    m.add_function(wrap_pyfunction!(set_tolerance, m)?)?;
    m.add_function(wrap_pyfunction!(tolerance, m)?)?;
    m.add_function(wrap_pyfunction!(set_rules, m)?)?;
    m.add_function(wrap_pyfunction!(rules, m)?)?;
    Ok(())
}

/// `set_tolerance(point)`: the merge distance (mm) the kernel's operations run with from now on,
/// or `"parasolid"` for Parasolid's (and Onshape's) 1e-5 mm, or `"default"`. Also from the
/// environment: `CADRS_TOLERANCE=parasolid`.
#[pyfunction]
fn set_tolerance(point: &Bound<'_, PyAny>) -> PyResult<()> {
    let name = match point.extract::<f64>() {
        Ok(x) => x.to_string(),
        Err(_) => point.extract::<String>()?,
    };
    let t = cadrs_kernel::cadquery::tolerances_named(&name).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!("no tolerance named {name:?}"))
    })?;
    cadrs_kernel::cadquery::set_tolerances(t)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

/// `tolerance()`: the merge distance (mm) the kernel's operations run with.
#[pyfunction]
fn tolerance() -> f64 {
    cadrs_kernel::cadquery::tolerances().point
}

/// `set_rules(name)`: whose rules the kernel follows where kernels legitimately differ,
/// `"parasolid"` (Parasolid's and Onshape's: its 1e-5 mm precision, and solids that only graze
/// one another left separate by a union) or `"cadrs"`. Also from the environment:
/// `CADRS_RULES=parasolid`.
#[pyfunction]
fn set_rules(name: &str) -> PyResult<()> {
    let r = cadrs_kernel::cadquery::rules_named(name).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!("no rules named {name:?}"))
    })?;
    cadrs_kernel::cadquery::set_rules(r)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

/// `rules()`: `"parasolid"` or `"cadrs"`.
#[pyfunction]
fn rules() -> &'static str {
    match cadrs_kernel::cadquery::rules() {
        cadrs_kernel::native::tol::KernelRules::Parasolid => "parasolid",
        cadrs_kernel::native::tol::KernelRules::Cadrs => "cadrs",
    }
}
