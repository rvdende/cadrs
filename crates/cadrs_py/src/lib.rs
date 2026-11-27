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
    Ok(())
}
