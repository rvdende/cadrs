//! A CadQuery-compatible modelling layer on the native kernel.
//!
//! CadQuery (<https://github.com/CadQuery/cadquery>, Apache-2.0) scripts build models through a
//! fluent [`Workplane`] over a shape layer (OCCT's topology: vertices, edges, wires, faces,
//! shells, solids, compounds). This module gives the same two layers on cadrs's kernel, with
//! CadQuery's semantics (ported from its `cq.py`, `selectors.py` and `occ_impl`), so its
//! programs run unchanged: `crates/cadrs_py` exposes them to Python as `import cadquery`.
//!
//! - [`geom`]: `Vector`s (nalgebra's), [`Plane`], [`Location`], [`BoundBox`].
//! - [`shape`]: [`Shape`]: solids, shells, faces and compounds are bodies of one session the
//!   module keeps; a face or solid picked out of one is a reference to it until an operation
//!   needs it on its own; edges, wires and vertices are exact curves and points.
//! - [`make`]: the constructors (`Edge.makeLine`, `Wire.makePolygon`, `Solid.makeBox`, …).
//! - [`ops`]: the operations on shapes (booleans, extrude, revolve, sweep, loft, fillets, …).
//! - [`select`]: CadQuery's selector strings (`">Z"`, `"|X and not %CIRCLE"`, `">Z[-2]"`, …).
//! - [`workplane`]: [`Workplane`], the fluent API.

pub mod geom;
pub mod make;
pub mod ops;
pub mod select;
pub mod shape;
pub mod workplane;

/// An affine map (the kernel's): a rotation or reflection, scaled, then a translation.
pub use crate::native::math::Affine;
pub use geom::{BoundBox, Location, Plane};
pub use select::Selector;
pub use shape::EdgeGeom;
pub use shape::{GeomType, Shape, ShapeType};
pub use workplane::{Combine, Obj, Until, Workplane};

use crate::native::NativeKernel;
use crate::native::topo::Body;
use crate::{BodyId, Kernel, KernelError};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// What a CadQuery call raises: the Python exception's kind and its message.
#[derive(Debug, Clone, PartialEq)]
pub enum CqError {
    /// `ValueError`: a bad argument, or nothing to work on.
    Value(String),
    /// `IndexError`.
    Index(String),
    /// `NotImplementedError`: what this module doesn't do (yet).
    NotImplemented(String),
    /// `RuntimeError` (StdFail_NotDone): the kernel refused the operation.
    Kernel(String),
    /// An operation whose result is empty (a cut taking everything; OCCT gives an empty
    /// compound, which the module's booleans give too).
    Empty(String),
}

impl std::fmt::Display for CqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CqError::Value(m) | CqError::Index(m) | CqError::NotImplemented(m) => f.write_str(m),
            CqError::Kernel(m) => write!(f, "kernel: {m}"),
            CqError::Empty(m) => write!(f, "empty: {m}"),
        }
    }
}

impl std::error::Error for CqError {}

impl From<KernelError> for CqError {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::EmptyResult(_) => CqError::Empty(e.to_string()),
            e => CqError::Kernel(e.to_string()),
        }
    }
}

/// A result of this module.
pub type CqResult<T> = Result<T, CqError>;

/// `Err(CqError::Value(..))` from a format string.
macro_rules! value_err {
    ($($t:tt)*) => { Err($crate::cadquery::CqError::Value(format!($($t)*))) };
}
pub(crate) use value_err;

/// The session every shape's body lives in.
static SESSION: LazyLock<Mutex<NativeKernel>> = LazyLock::new(|| Mutex::new(NativeKernel::new()));

/// Bodies whose last shape went away, released at the session's next use (a shape may go away
/// while the session is in use).
static RELEASED: Mutex<Vec<BodyId>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` on the session (never from inside another `session` call).
pub(crate) fn session<R>(f: impl FnOnce(&mut NativeKernel) -> R) -> R {
    let mut k = lock(&SESSION);
    let dead = std::mem::take(&mut *lock(&RELEASED));
    for id in dead {
        k.release(id);
    }
    f(&mut k)
}

/// A body of the session, released when the last shape holding it goes.
#[derive(Debug)]
pub(crate) struct Handle(pub(crate) BodyId);

impl Drop for Handle {
    fn drop(&mut self) {
        lock(&RELEASED).push(self.0);
    }
}

/// A shared [`Handle`].
pub(crate) type BodyRef = Arc<Handle>;

/// Takes ownership of a body the session made.
pub(crate) fn adopt(id: BodyId) -> BodyRef {
    Arc::new(Handle(id))
}

/// Adds a body to the session.
pub(crate) fn insert(body: Body) -> BodyRef {
    adopt(session(|k| k.insert(body)))
}

/// The B-rep of a body of the session.
pub(crate) fn brep(b: &BodyRef) -> Arc<Body> {
    session(|k| k.get(b.0).cloned()).expect("a live handle's body is in the session")
}

/// Runs a kernel operation that makes one body.
pub(crate) fn op1(
    f: impl FnOnce(&mut NativeKernel) -> crate::Result<crate::OpResult>,
) -> CqResult<BodyRef> {
    let r = session(f)?;
    let mut bodies = r.bodies.into_iter();
    let first = bodies
        .next()
        .ok_or_else(|| CqError::Kernel("the operation made nothing".into()))?;
    let rest: Vec<BodyId> = bodies.collect();
    if !rest.is_empty() {
        // Several bodies: one compound of them.
        let all: Vec<BodyId> = std::iter::once(first).chain(rest.iter().copied()).collect();
        let c = session(|k| {
            let c = k.compound(&all);
            for id in &all {
                k.release(*id);
            }
            c
        })?;
        return Ok(adopt(c.bodies[0]));
    }
    Ok(adopt(first))
}

#[cfg(test)]
mod tests;
