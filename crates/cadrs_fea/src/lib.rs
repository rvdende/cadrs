//! cadrs_fea: linear static finite-element analysis (P3F.5; `intro-to-parametric-cad.md` P3.5,
//! `intro-to-assemblies.md` A1.7 Loads, A1.8 Simulation, A6.3 Simulation connection). No Bevy.
//!
//! - [`mesh`]: a part's closed surface (the kernel's tessellation) → a quadratic tetrahedral
//!   mesh, by Delaunay tetrahedralization ([`delaunay`], exact predicates) of the refined
//!   surface points and an interior lattice, carved to the part.
//! - [`element`]: the 10-node tetrahedron (exact 4-point integration).
//! - [`solve`]: loads (Fixed faces, a force spread over faces, a normal force, a pressure),
//!   bonds between bodies (penalty ties where they touch), assembly of the sparse stiffness
//!   over the free degrees of freedom, a supernodal sparse Cholesky factorization (`faer`),
//!   nodal stresses and von Mises.
//!
//! Units: mm, N, MPa.

pub mod delaunay;
pub mod element;
pub mod geom;
pub mod mesh;
pub mod solve;
pub mod surface;

pub use mesh::{BoundaryTri, MeshError, TetMesh};
pub use solve::{Body, BodyResult, Bond, FeaError, Load, LoadKind, Material, Model, Options, Solution, Stage, Stats, solve};
pub use surface::Surface;
