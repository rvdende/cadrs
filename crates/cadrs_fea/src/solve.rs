//! The linear static analysis: mesh the bodies, assemble K u = f over the free degrees of
//! freedom, tie bonded bodies, factor K by sparse Cholesky (faer, supernodal with an AMD
//! ordering) and recover stresses.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use faer::sparse::{SparseColMat, SymbolicSparseColMat};
use rayon::prelude::*;

use crate::element::{self, NODE_L};
use crate::geom::{V3, add, dot, len, normalize, scale, sub};
use crate::mesh::{self, BoundaryTri, MeshError, TetMesh};
use crate::surface::{Surface, SurfaceIndex};

/// An isotropic linear-elastic material. Moduli in MPa (N/mm²).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub youngs: f64,
    pub poisson: f64,
}

/// One part to analyse: its closed surface (mm) and material.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub name: String,
    pub surface: Surface,
    pub material: Material,
}

/// What a load does to its faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadKind {
    /// The faces don't move.
    Fixed,
    /// A total force (N), spread over the faces' area.
    Force(V3),
    /// A total force (N) along the faces' normals, pushing into the part.
    NormalForce(f64),
    /// A pressure (MPa) on the faces, pushing into the part.
    Pressure(f64),
}

/// A load on faces of one or more bodies.
#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub name: String,
    /// (body index, CAD faces of it) — face numbers as in the body's [`Surface::faces`].
    pub targets: Vec<(usize, Vec<u32>)>,
    pub kind: LoadKind,
}

/// Two bodies bonded where they touch (a mate with a Simulation connection; in a Part Studio,
/// parts that touch).
#[derive(Debug, Clone, PartialEq)]
pub struct Bond {
    pub name: String,
    pub a: usize,
    pub b: usize,
    /// An error if they don't touch (a mate asks for it); otherwise a bond that finds no
    /// contact is left out.
    pub required: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub loads: Vec<Load>,
    pub bonds: Vec<Bond>,
}

/// How to solve.
#[derive(Debug, Clone)]
pub struct Options {
    /// About how many elements to make, over all bodies.
    pub target_elements: usize,
    /// The element size (mm), overriding `target_elements`.
    pub element_size: Option<f64>,
    /// Set to stop the solve between stages.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for Options {
    fn default() -> Self {
        Self { target_elements: 6000, element_size: None, cancel: None }
    }
}

/// Where the solve is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Meshing,
    Assembling,
    Factoring,
    Solving,
    Stresses,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Meshing => "Meshing",
            Stage::Assembling => "Assembling",
            Stage::Factoring => "Factoring",
            Stage::Solving => "Solving",
            Stage::Stresses => "Computing stresses",
        }
    }
}

/// Why a solve failed, in words for the Simulation panel.
#[derive(Debug, Clone, PartialEq)]
pub enum FeaError {
    NoBodies,
    Mesh { body: String, error: MeshError },
    NoFixed,
    EmptyLoad(String),
    Unbonded(String),
    Free(String),
    Solver(String),
    Cancelled,
}

impl std::fmt::Display for FeaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeaError::NoBodies => write!(f, "Nothing to analyse: there are no parts."),
            FeaError::Mesh { body, error } => write!(f, "{body} couldn't be meshed: {error}."),
            FeaError::NoFixed => write!(f, "Nothing holds the model: add a Fixed load."),
            FeaError::EmptyLoad(l) => write!(f, "{l} has no faces on the mesh."),
            FeaError::Unbonded(b) => write!(f, "{b}: the two parts don't touch, so they can't be bonded."),
            FeaError::Free(b) => write!(f, "{b} is free to move: fix it or connect it with a Simulation connection."),
            FeaError::Solver(e) => write!(f, "The solver failed: {e}."),
            FeaError::Cancelled => write!(f, "Cancelled."),
        }
    }
}

/// One body's results. Nodal values are indexed like `mesh.nodes`.
#[derive(Debug, Clone, Default)]
pub struct BodyResult {
    pub name: String,
    pub mesh: TetMesh,
    /// mm.
    pub displacement: Vec<V3>,
    /// MPa (xx, yy, zz, xy, yz, zx), averaged over the elements at each node.
    pub stress: Vec<[f64; 6]>,
    /// MPa.
    pub von_mises: Vec<f64>,
    pub material: Option<Material>,
}

/// Sizes and timings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub elements: usize,
    pub nodes: usize,
    pub dofs: usize,
    pub element_size: f64,
    /// Nodes tied per bond.
    pub bonded_nodes: Vec<usize>,
    pub mesh_seconds: f64,
    pub assemble_seconds: f64,
    pub factor_seconds: f64,
    pub total_seconds: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Solution {
    pub bodies: Vec<BodyResult>,
    pub stats: Stats,
}

impl Solution {
    /// The largest displacement magnitude (mm).
    pub fn max_displacement(&self) -> f64 {
        self.bodies.iter().flat_map(|b| b.displacement.iter()).map(|u| len(*u)).fold(0.0, f64::max)
    }

    /// The largest von Mises stress (MPa).
    pub fn max_von_mises(&self) -> f64 {
        self.bodies.iter().flat_map(|b| b.von_mises.iter()).copied().fold(0.0, f64::max)
    }

    /// The element of `body` containing `p` and `p`'s volume coordinates in it (the nearest
    /// one, for a point on or just off the surface).
    pub fn locate(&self, body: usize, p: V3) -> Option<(usize, [f64; 4])> {
        let m = &self.bodies.get(body)?.mesh;
        let mut best: Option<(usize, [f64; 4], f64)> = None;
        for (ti, t) in m.tets.iter().enumerate() {
            let x: [V3; 4] = std::array::from_fn(|k| m.nodes[t[k] as usize]);
            let (gl, _) = element::grad_l(&x);
            let d = sub(p, x[0]);
            let l2 = dot(gl[1], d);
            let l3 = dot(gl[2], d);
            let l4 = dot(gl[3], d);
            let l = [1.0 - l2 - l3 - l4, l2, l3, l4];
            let worst = l.iter().copied().fold(f64::MAX, f64::min);
            if best.is_none_or(|b| worst > b.2) {
                best = Some((ti, l, worst));
            }
            if worst >= 0.0 {
                break;
            }
        }
        best.map(|(t, l, _)| (t, l))
    }

    /// The stress (MPa) at `p` in `body`, from the element containing it (not averaged).
    pub fn stress_at(&self, body: usize, p: V3) -> Option<[f64; 6]> {
        let (ti, l) = self.locate(body, p)?;
        let b = &self.bodies[body];
        let t = b.mesh.tets[ti];
        let x: [V3; 4] = std::array::from_fn(|k| b.mesh.nodes[t[k] as usize]);
        let (gl, _) = element::grad_l(&x);
        let u: [V3; 10] = std::array::from_fn(|k| b.displacement[t[k] as usize]);
        let mat = b.material?;
        let (lambda, mu) = element::lame(mat.youngs, mat.poisson);
        Some(element::stress(element::strain(&gl, l, &u), lambda, mu))
    }

    /// The displacement (mm) at `p` in `body`, interpolated in the element containing it.
    pub fn displacement_at(&self, body: usize, p: V3) -> Option<V3> {
        let (ti, l) = self.locate(body, p)?;
        let b = &self.bodies[body];
        let n = element::shape(l);
        let t = b.mesh.tets[ti];
        Some((0..10).fold([0.0; 3], |acc, k| add(acc, scale(b.displacement[t[k] as usize], n[k]))))
    }
}

fn check(opts: &Options) -> Result<(), FeaError> {
    if opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(FeaError::Cancelled);
    }
    Ok(())
}

/// Solves the linear static problem.
pub fn solve(model: &Model, opts: &Options, progress: &(dyn Fn(Stage, f32) + Sync)) -> Result<Solution, FeaError> {
    let t0 = Instant::now();
    if model.bodies.is_empty() {
        return Err(FeaError::NoBodies);
    }
    if !model.loads.iter().any(|l| l.kind == LoadKind::Fixed) {
        return Err(FeaError::NoFixed);
    }
    progress(Stage::Meshing, 0.0);
    let meshes = mesh_bodies(model, opts)?;
    check(opts)?;
    let mesh_seconds = t0.elapsed().as_secs_f64();
    let h = meshes.iter().map(|m| m.h).fold(0.0, f64::max);

    // Global node numbering: body after body.
    let mut offset = Vec::with_capacity(meshes.len());
    let mut n_nodes = 0usize;
    for m in &meshes {
        offset.push(n_nodes);
        n_nodes += m.nodes.len();
    }
    let n_dof = 3 * n_nodes;

    // Fixed degrees of freedom and loads' faces.
    let mut fixed = vec![false; n_dof];
    let mut rhs = vec![0.0; n_dof];
    let mut held = vec![false; meshes.len()];
    for load in &model.loads {
        let tris: Vec<(usize, &BoundaryTri)> = load
            .targets
            .iter()
            .flat_map(|(b, faces)| meshes.get(*b).into_iter().flat_map(move |m| m.on_faces(faces).map(move |t| (*b, t))))
            .collect();
        if tris.is_empty() {
            return Err(FeaError::EmptyLoad(load.name.clone()));
        }
        let area: f64 = tris.iter().map(|(b, t)| len(meshes[*b].area_vector(t))).sum();
        for (b, t) in &tris {
            let m = &meshes[*b];
            let av = m.area_vector(t);
            // Uniform traction on a 6-node triangle: the corners get nothing, each mid-edge node
            // a third of the triangle's share.
            let nodal = match load.kind {
                LoadKind::Fixed => {
                    for &n in &t.nodes {
                        let g = offset[*b] + n as usize;
                        fixed[3 * g..3 * g + 3].iter_mut().for_each(|f| *f = true);
                    }
                    held[*b] = true;
                    continue;
                }
                LoadKind::Force(f) => scale(f, len(av) / area / 3.0),
                LoadKind::NormalForce(f) => scale(av, -f / area / 3.0),
                LoadKind::Pressure(p) => scale(av, -p / 3.0),
            };
            for &n in &t.nodes[3..] {
                let g = offset[*b] + n as usize;
                for k in 0..3 {
                    rhs[3 * g + k] += nodal[k];
                }
            }
        }
    }

    // Bonds: tie the nodes of `a` that touch `b` to `b`'s surface.
    let mut ties: Vec<Tie> = Vec::new();
    let mut bonded_nodes = Vec::new();
    let mut linked: Vec<(usize, usize)> = Vec::new();
    for bond in &model.bonds {
        let (Some(ma), Some(mb)) = (meshes.get(bond.a), meshes.get(bond.b)) else { continue };
        // Both ways: each side's surface nodes follow the other side's faces, so neither mesh
        // can open or overlap between the other's nodes (a one-way tie leaves a stress spike
        // along the joint when the meshes don't match).
        let tol = 0.05 * h.max(1e-9);
        let ab = tie_nodes(ma, mb, tol);
        let ba = tie_nodes(mb, ma, tol);
        bonded_nodes.push(ab.len() + ba.len());
        if ab.len() < 3 && ba.len() < 3 {
            if bond.required {
                return Err(FeaError::Unbonded(bond.name.clone()));
            }
            continue;
        }
        linked.push((bond.a, bond.b));
        for (slave, master, found) in [(bond.a, bond.b, ab), (bond.b, bond.a, ba)] {
            for (s, masters) in found {
                ties.push(Tie { slave: offset[slave] + s as usize, masters: masters.map(|(n, w)| (offset[master] + n as usize, w)) });
            }
        }
    }
    // Every body must be held, directly or through bonds.
    let mut group: Vec<usize> = (0..meshes.len()).collect();
    fn root(g: &mut [usize], mut x: usize) -> usize {
        while g[x] != x {
            g[x] = g[g[x]];
            x = g[x];
        }
        x
    }
    for &(a, b) in &linked {
        let (ra, rb) = (root(&mut group, a), root(&mut group, b));
        group[ra] = rb;
    }
    for b in 0..meshes.len() {
        let r = root(&mut group, b);
        if !(0..meshes.len()).any(|o| held[o] && root(&mut group, o) == r) {
            return Err(FeaError::Free(model.bodies[b].name.clone()));
        }
    }
    check(opts)?;

    // Assembly.
    progress(Stage::Assembling, 0.0);
    let t1 = Instant::now();
    let mut free_index = vec![usize::MAX; n_dof];
    let mut n_free = 0;
    for d in 0..n_dof {
        if !fixed[d] {
            free_index[d] = n_free;
            n_free += 1;
        }
    }
    let e_max = model.bodies.iter().map(|b| b.material.youngs).fold(0.0, f64::max);
    let penalty = 1e4 * e_max * h;
    let (k, nd) = assemble(&meshes, &offset, &model.bodies, &ties, &free_index, n_free, penalty, opts, progress)?;
    let assemble_seconds = t1.elapsed().as_secs_f64();
    check(opts)?;

    progress(Stage::Factoring, 0.0);
    let t2 = Instant::now();
    let b = factor_and_solve(&k, &nd, &free_index, &rhs, opts, progress)?;
    let factor_seconds = t2.elapsed().as_secs_f64();
    let mut u = vec![0.0; n_dof];
    for d in 0..n_dof {
        if free_index[d] != usize::MAX {
            u[d] = b[free_index[d]];
        }
    }
    if u.iter().any(|v| !v.is_finite()) {
        return Err(FeaError::Solver("the displacements aren't finite".into()));
    }

    progress(Stage::Stresses, 0.0);
    let mut bodies = Vec::with_capacity(meshes.len());
    let mut elements = 0;
    for (bi, m) in meshes.into_iter().enumerate() {
        let mat = model.bodies[bi].material;
        let disp: Vec<V3> = (0..m.nodes.len()).map(|n| {
            let g = offset[bi] + n;
            [u[3 * g], u[3 * g + 1], u[3 * g + 2]]
        }).collect();
        let stress = nodal_stress(&m, &disp, mat);
        let von_mises = stress.iter().map(|s| element::von_mises(*s)).collect();
        elements += m.tets.len();
        bodies.push(BodyResult { name: model.bodies[bi].name.clone(), mesh: m, displacement: disp, stress, von_mises, material: Some(mat) });
    }
    let sol = Solution {
        bodies,
        stats: Stats {
            elements,
            nodes: n_nodes,
            dofs: n_free,
            element_size: h,
            bonded_nodes,
            mesh_seconds,
            assemble_seconds,
            factor_seconds,
            total_seconds: t0.elapsed().as_secs_f64(),
        },
    };
    // A mechanism the factorization didn't catch shows as absurd displacements.
    let size = sol.bodies.iter().filter_map(|b| b.mesh.nodes.iter().try_fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
        Some(([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])]))
    })).map(|(lo, hi)| len(sub(hi, lo))).fold(0.0, f64::max);
    if sol.max_displacement() > 1e6 * size.max(1.0) {
        return Err(FeaError::Free("A part".into()));
    }
    Ok(sol)
}

/// Meshes every body with one element size.
fn mesh_bodies(model: &Model, opts: &Options) -> Result<Vec<TetMesh>, FeaError> {
    let volume: f64 = model.bodies.iter().map(|b| b.surface.volume().abs()).sum();
    let target = opts.target_elements.max(10) as f64;
    // A Delaunay mesh of a cubic lattice has about 5.5 tetrahedra per cell.
    let mut h = opts.element_size.unwrap_or_else(|| (5.5 * volume / target).cbrt());
    let run = |h: f64| -> Result<Vec<TetMesh>, FeaError> {
        model
            .bodies
            .par_iter()
            .map(|b| mesh::mesh(&b.surface, h).map_err(|error| FeaError::Mesh { body: b.name.clone(), error }))
            .collect()
    };
    let mut meshes = run(h)?;
    if opts.element_size.is_none() {
        // Correct the size while the count is far off (surface points add elements the lattice
        // estimate doesn't count, most on small or thin parts).
        for _ in 0..3 {
            let n: usize = meshes.iter().map(|m| m.tets.len()).sum();
            let ratio = n as f64 / target;
            if (0.75..=1.35).contains(&ratio) {
                break;
            }
            h *= ratio.cbrt();
            meshes = run(h)?;
        }
    }
    Ok(meshes)
}

/// A tie: the slave node's displacement follows the master face's at the touching point.
struct Tie {
    slave: usize,
    masters: [(usize, f64); 6],
}

/// The boundary nodes of `a` lying on `b`'s surface (within `tol`, on faces facing each other),
/// each with the 6 nodes of `b`'s triangle there and their shape-function weights.
fn tie_nodes(a: &TetMesh, b: &TetMesh, tol: f64) -> Vec<(u32, [(u32, f64); 6])> {
    let corner_tris: Vec<[u32; 3]> = b.boundary.iter().map(|t| [t.nodes[0], t.nodes[1], t.nodes[2]]).collect();
    let faces: Vec<u32> = (0..b.boundary.len() as u32).collect();
    let index = SurfaceIndex::new(&b.nodes, &corner_tris, &faces, b.h.max(tol * 4.0));
    // Outward normals of a's boundary triangles at each of their nodes.
    let mut normals: std::collections::HashMap<u32, Vec<V3>> = std::collections::HashMap::new();
    for t in &a.boundary {
        let n = normalize(a.area_vector(t));
        for &v in &t.nodes {
            normals.entry(v).or_default().push(n);
        }
    }
    let mut keys: Vec<u32> = normals.keys().copied().collect();
    keys.sort_unstable();
    let mut out = Vec::new();
    for v in keys {
        let p = a.nodes[v as usize];
        let Some((d, _, tri, w)) = index.closest(p, 1) else { continue };
        if d > tol {
            continue;
        }
        let bt = &b.boundary[tri as usize];
        let nb = normalize(b.area_vector(bt));
        if !normals[&v].iter().any(|na| dot(*na, nb) < -0.5) {
            continue;
        }
        // Quadratic triangle shape functions at barycentric (w0, w1, w2).
        let n = [
            w[0] * (2.0 * w[0] - 1.0),
            w[1] * (2.0 * w[1] - 1.0),
            w[2] * (2.0 * w[2] - 1.0),
            4.0 * w[0] * w[1],
            4.0 * w[1] * w[2],
            4.0 * w[2] * w[0],
        ];
        out.push((v, std::array::from_fn(|k| (bt.nodes[k], n[k]))));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    meshes: &[TetMesh],
    offset: &[usize],
    bodies: &[Body],
    ties: &[Tie],
    free_index: &[usize],
    n_free: usize,
    penalty: f64,
    opts: &Options,
    progress: &(dyn Fn(Stage, f32) + Sync),
) -> Result<(SparseColMat<usize, f64>, Vec<usize>), FeaError> {
    let n_nodes = free_index.len() / 3;
    // Node adjacency (lower: for node b, the nodes a ≥ b it couples to).
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); n_nodes];
    for (bi, m) in meshes.iter().enumerate() {
        for t in &m.tets {
            for &x in t {
                for &y in t {
                    let (gx, gy) = (offset[bi] + x as usize, offset[bi] + y as usize);
                    if gx >= gy {
                        adj[gy].push(gx as u32);
                    }
                }
            }
        }
    }
    for tie in ties {
        let group: Vec<usize> = std::iter::once(tie.slave).chain(tie.masters.iter().map(|m| m.0)).collect();
        for &x in &group {
            for &y in &group {
                if x >= y {
                    adj[y].push(x as u32);
                }
            }
        }
    }
    adj.par_iter_mut().for_each(|l| {
        l.sort_unstable();
        l.dedup();
    });
    // Columns over free dofs.
    let mut col_ptr = Vec::with_capacity(n_free + 1);
    let mut row_idx: Vec<usize> = Vec::new();
    col_ptr.push(0usize);
    for nb in 0..n_nodes {
        for j in 0..3 {
            let c = free_index[3 * nb + j];
            if c == usize::MAX {
                continue;
            }
            for &na in &adj[nb] {
                let na = na as usize;
                for i in 0..3 {
                    if na == nb && i < j {
                        continue;
                    }
                    let r = free_index[3 * na + i];
                    if r != usize::MAX {
                        row_idx.push(r);
                    }
                }
            }
            col_ptr.push(row_idx.len());
        }
    }
    // A nested-dissection order of the free dofs (each node's together), offered to the
    // factorization next to the minimum-degree one.
    let coords: Vec<V3> = meshes.iter().flat_map(|m| m.nodes.iter().copied()).collect();
    let nd: Vec<usize> = nested_dissection(&coords, &adj)
        .iter()
        .flat_map(|&n| (0..3).map(move |a| 3 * n as usize + a))
        .map(|d| free_index[d])
        .filter(|&f| f != usize::MAX)
        .collect();
    drop(adj);
    let mut values = vec![0.0; row_idx.len()];
    let mut add = |rd: usize, cd: usize, v: f64| {
        let (r, c) = (free_index[rd], free_index[cd]);
        if r == usize::MAX || c == usize::MAX {
            return;
        }
        let (r, c) = if r >= c { (r, c) } else { (c, r) };
        let s = &row_idx[col_ptr[c]..col_ptr[c + 1]];
        if let Ok(k) = s.binary_search(&r) {
            values[col_ptr[c] + k] += v;
        }
    };
    let total: usize = meshes.iter().map(|m| m.tets.len()).sum();
    let mut done = 0usize;
    for (bi, m) in meshes.iter().enumerate() {
        let (lambda, mu) = element::lame(bodies[bi].material.youngs, bodies[bi].material.poisson);
        for chunk in m.tets.chunks(4096) {
            let ks: Vec<Box<[f64; 900]>> = chunk
                .par_iter()
                .map(|t| {
                    let x: [V3; 4] = std::array::from_fn(|k| m.nodes[t[k] as usize]);
                    element::stiffness(&x, lambda, mu)
                })
                .collect();
            for (t, ke) in chunk.iter().zip(&ks) {
                let g: [usize; 10] = std::array::from_fn(|k| offset[bi] + t[k] as usize);
                for a in 0..10 {
                    for b in 0..10 {
                        if g[a] < g[b] {
                            continue;
                        }
                        for i in 0..3 {
                            for j in 0..3 {
                                if a == b && i < j {
                                    continue;
                                }
                                add(3 * g[a] + i, 3 * g[b] + j, ke[(3 * a + i) * 30 + 3 * b + j]);
                            }
                        }
                    }
                }
            }
            done += chunk.len();
            progress(Stage::Assembling, done as f32 / total.max(1) as f32);
            check(opts)?;
        }
    }
    for tie in ties {
        // Penalty on (u_s − Σ wₖ u_mₖ)² per axis.
        let c: Vec<(usize, f64)> = std::iter::once((tie.slave, 1.0)).chain(tie.masters.iter().map(|&(n, w)| (n, -w))).collect();
        for &(x, cx) in &c {
            for &(y, cy) in &c {
                if x < y {
                    continue;
                }
                for i in 0..3 {
                    add(3 * x + i, 3 * y + i, penalty * cx * cy);
                }
            }
        }
    }
    let symbolic = SymbolicSparseColMat::new_checked(n_free, n_free, col_ptr, None, row_idx);
    Ok((SparseColMat::new(symbolic, values), nd))
}

/// Factors K (lower triangle; supernodal Cholesky) and solves K x = f; returns x over the free
/// dofs. The elimination order is the nested dissection `nd` (new → old) unless its factor is
/// much bigger than the approximate-minimum-degree one's: nested dissection fills more but
/// leaves big, independent supernodes that factor in parallel (the 24k-element cantilever:
/// 108 M against 77 M entries, 3.1 s against 9.5 s), while on a mesh its planes cut badly it can
/// fill several times more.
fn factor_and_solve(
    k: &SparseColMat<usize, f64>,
    nd: &[usize],
    free_index: &[usize],
    rhs: &[f64],
    opts: &Options,
    progress: &(dyn Fn(Stage, f32) + Sync),
) -> Result<Vec<f64>, FeaError> {
    use faer::dyn_stack::{MemBuffer, MemStack};
    use faer::sparse::linalg::cholesky::{SymmetricOrdering, factorize_symbolic_cholesky};
    let n = k.nrows();
    let failed = |e: faer::sparse::FaerError| FeaError::Solver(format!("{e:?}"));
    let amd = factorize_symbolic_cholesky(k.symbolic(), faer::Side::Lower, SymmetricOrdering::Amd, Default::default()).map_err(failed)?;
    let symbolic = if nd.len() == n {
        let mut inv = vec![0usize; n];
        for (new, &old) in nd.iter().enumerate() {
            inv[old] = new;
        }
        let perm = faer::perm::PermRef::new_checked(nd, &inv, n);
        let dissected = factorize_symbolic_cholesky(k.symbolic(), faer::Side::Lower, SymmetricOrdering::Custom(perm), Default::default()).map_err(failed)?;
        if (dissected.len_val() as f64) < 1.6 * amd.len_val() as f64 { dissected } else { amd }
    } else {
        amd
    };
    check(opts)?;
    let par = factor_parallelism();
    let mut l_values = vec![0.0f64; symbolic.len_val()];
    let mut mem = MemBuffer::try_new(symbolic.factorize_numeric_llt_scratch::<f64>(par, Default::default()))
        .map_err(|_| FeaError::Solver("out of memory".into()))?;
    let llt = symbolic
        .factorize_numeric_llt(&mut l_values, k.as_ref(), faer::Side::Lower, Default::default(), par, MemStack::new(&mut mem), Default::default())
        .map_err(|_| FeaError::Free("A part".into()))?;
    drop(mem);
    check(opts)?;
    progress(Stage::Solving, 0.0);
    let mut b = faer::Mat::<f64>::zeros(n, 1);
    for (d, &f) in free_index.iter().enumerate() {
        if f != usize::MAX {
            b[(f, 0)] = rhs[d];
        }
    }
    let mut mem = MemBuffer::try_new(symbolic.solve_in_place_scratch::<f64>(1, par)).map_err(|_| FeaError::Solver("out of memory".into()))?;
    llt.solve_in_place_with_conj(faer::Conj::No, b.as_mut(), par, MemStack::new(&mut mem));
    Ok((0..n).map(|i| b[(i, 0)]).collect())
}

/// Nodal stresses: each element's stress at its nodes, averaged over the elements at a node.
fn nodal_stress(m: &TetMesh, u: &[V3], mat: Material) -> Vec<[f64; 6]> {
    let (lambda, mu) = element::lame(mat.youngs, mat.poisson);
    let per: Vec<[[f64; 6]; 10]> = m
        .tets
        .par_iter()
        .map(|t| {
            let x: [V3; 4] = std::array::from_fn(|k| m.nodes[t[k] as usize]);
            let (gl, _) = element::grad_l(&x);
            let ue: [V3; 10] = std::array::from_fn(|k| u[t[k] as usize]);
            std::array::from_fn(|n| element::stress(element::strain(&gl, NODE_L[n], &ue), lambda, mu))
        })
        .collect();
    let mut sum = vec![[0.0; 6]; m.nodes.len()];
    let mut count = vec![0u32; m.nodes.len()];
    for (t, s) in m.tets.iter().zip(&per) {
        for k in 0..10 {
            let n = t[k] as usize;
            for c in 0..6 {
                sum[n][c] += s[k][c];
            }
            count[n] += 1;
        }
    }
    sum.into_iter().zip(count).map(|(s, c)| s.map(|v| v / c.max(1) as f64)).collect()
}

/// The threads to factor with: at most four (`CADRS_FEA_THREADS` overrides; 1 = sequential).
/// More don't pay: the supernodal factorization's parallel parts are small, and on a machine
/// that is busy with other work every extra thread waits on the others (the 24k-element
/// cantilever: 3.2 s on one thread, 2.3 s on four, 9.4 s on all twelve with the machine loaded).
fn factor_parallelism() -> faer::Par {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    match std::env::var("CADRS_FEA_THREADS").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(cores.min(4)) {
        0 | 1 => faer::Par::Seq,
        n => faer::Par::rayon(n),
    }
}

/// A nested-dissection ordering of the nodes from their positions: split at the median along
/// the longest extent, the second half's nodes touching the first are the separator, both
/// halves recursively, the separator last.
fn nested_dissection(coords: &[V3], lower_adj: &[Vec<u32>]) -> Vec<u32> {
    let n = coords.len();
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (b, list) in lower_adj.iter().enumerate() {
        for &a in list {
            if a as usize != b {
                adj[b].push(a);
                adj[a as usize].push(b as u32);
            }
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut mark = vec![0u32; n];
    let mut stamp = 0u32;
    let mut stack: Vec<(Vec<u32>, bool)> = vec![((0..n as u32).collect(), false)];
    while let Some((nodes, emit)) = stack.pop() {
        if emit || nodes.len() <= 64 {
            order.extend(nodes);
            continue;
        }
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for &v in &nodes {
            let p = coords[v as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap();
        let mut nodes = nodes;
        let m = nodes.len() / 2;
        nodes.select_nth_unstable_by(m, |a, b| coords[*a as usize][axis].total_cmp(&coords[*b as usize][axis]));
        let right = nodes.split_off(m);
        let left = nodes;
        stamp += 1;
        for &v in &left {
            mark[v as usize] = stamp;
        }
        let (sep, rest): (Vec<u32>, Vec<u32>) = right.into_iter().partition(|&v| adj[v as usize].iter().any(|&w| mark[w as usize] == stamp));
        stack.push((sep, true));
        stack.push((rest, false));
        stack.push((left, false));
    }
    order
}
