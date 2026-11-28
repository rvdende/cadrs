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
    /// `StdFail_NotDone`: the kernel refused the operation.
    Kernel(String),
    /// `Standard_Failure`: what OCCT's own checks refuse before the kernel works (a fillet of
    /// seam edges only: "There are no suitable edges for chamfer or fillet").
    Failure(String),
    /// An operation whose result is empty (a cut taking everything; OCCT gives an empty
    /// compound, which the module's booleans give too).
    Empty(String),
}

impl std::fmt::Display for CqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CqError::Value(m) | CqError::Index(m) | CqError::NotImplemented(m) => f.write_str(m),
            CqError::Kernel(m) => write!(f, "kernel: {m}"),
            CqError::Failure(m) => write!(f, "failure: {m}"),
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

/// The session every shape's body lives in, following the rules `CADRS_RULES` names
/// ([`rules_named`]) and at the tolerances `CADRS_TOLERANCE` names ([`tolerances_named`]), else
/// the kernel's own.
static SESSION: LazyLock<Mutex<NativeKernel>> = LazyLock::new(|| {
    let mut k = NativeKernel::new();
    if let Ok(name) = std::env::var("CADRS_RULES")
        && let Some(r) = rules_named(&name)
    {
        let _ = k.set_rules(r);
    }
    if let Ok(name) = std::env::var("CADRS_TOLERANCE")
        && let Some(t) = tolerances_named(&name)
    {
        let _ = k.set_tolerances(t);
    }
    Mutex::new(k)
});

/// Tolerances by name: `"parasolid"` (Parasolid's and Onshape's linear precision, 1e-5 mm:
/// [`crate::native::tol::Tolerances::parasolid`]), `"default"`, or a merge distance in mm.
pub fn tolerances_named(name: &str) -> Option<crate::native::tol::Tolerances> {
    use crate::native::tol::Tolerances;
    match name.trim() {
        "parasolid" => Some(Tolerances::parasolid()),
        "default" | "" => Some(Tolerances::default()),
        x => x.parse::<f64>().ok().map(Tolerances::with_point),
    }
}

/// Kernel rules by name: `"parasolid"` ([`crate::native::tol::KernelRules::Parasolid`]) or
/// `"cadrs"`.
pub fn rules_named(name: &str) -> Option<crate::native::tol::KernelRules> {
    use crate::native::tol::KernelRules;
    match name.trim().to_ascii_lowercase().as_str() {
        "parasolid" | "parasolidrules" => Some(KernelRules::Parasolid),
        "cadrs" | "default" | "" => Some(KernelRules::Cadrs),
        _ => None,
    }
}

/// The rules the session follows (the session made first, so `CADRS_RULES` is applied).
pub fn rules() -> crate::native::tol::KernelRules {
    session(|k| k.rules())
}

/// Follows `rules` from now on, their tolerances too ([`NativeKernel::set_rules`]).
pub fn set_rules(r: crate::native::tol::KernelRules) -> CqResult<()> {
    session(|k| k.set_rules(r)).map_err(|e| CqError::Value(e.to_string()))
}

/// Sets the tolerances every operation of the session runs with from now on (see
/// [`crate::native::tol`]); shapes made before keep theirs.
pub fn set_tolerances(t: crate::native::tol::Tolerances) -> CqResult<()> {
    session(|k| k.set_tolerances(t)).map_err(|e| CqError::Value(e.to_string()))
}

/// The tolerances the session's operations run with.
pub fn tolerances() -> crate::native::tol::Tolerances {
    session(|k| k.tolerances())
}

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

/// [`op1`] for an operation on `inputs` whose result may hold several solids: those in the order
/// of what they came from (as OCCT's results list them; programs take `solids()[0]`), each by
/// the earliest input face it continues (the input first, then its face).
pub(crate) fn op_ordered(
    inputs: &[BodyId],
    f: impl FnOnce(&mut NativeKernel) -> crate::Result<crate::OpResult>,
) -> CqResult<BodyRef> {
    let r = session(f)?;
    if r.bodies.len() != 1 {
        let ids = r.bodies.clone();
        return op1(move |_| {
            Ok(crate::OpResult {
                bodies: ids,
                history: r.history,
            })
        });
    }
    let id = r.bodies[0];
    let b = session(|k| k.get(id).cloned())?;
    if b.solids.len() < 2 {
        return Ok(adopt(id));
    }
    let mut key = vec![(usize::MAX, u64::MAX); b.faces.len()];
    for (fid, inp) in &r.history.modified {
        if let (Some(k), Some(slot)) = (
            inputs.iter().position(|x| *x == inp.body),
            key.get_mut(fid.0 as usize),
        ) {
            *slot = (*slot).min((k, inp.face.0));
        }
    }
    let solid_key = |s: &crate::native::topo::Solid| {
        s.shells
            .iter()
            .flat_map(|&sh| b.shells[sh].faces.iter())
            .map(|&f| key[f])
            .min()
            .unwrap_or((usize::MAX, u64::MAX))
    };
    let keys: Vec<_> = b.solids.iter().map(solid_key).collect();
    if keys.windows(2).all(|w| w[0] <= w[1]) {
        return Ok(adopt(id));
    }
    let mut order: Vec<usize> = (0..b.solids.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    let mut nb = (*b).clone();
    nb.solids = order.iter().map(|&i| b.solids[i].clone()).collect();
    let out = insert(nb);
    session(|k| k.release(id));
    Ok(out)
}

#[cfg(test)]
mod tests;
