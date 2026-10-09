//! Persistent naming: names for faces, edges and vertices that survive rebuilds (P3.2).
//!
//! A kernel's [`FaceId`], [`EdgeId`] and [`VertexId`] are indices into one body and change with
//! every rebuild. Documents store names instead:
//!
//! - A **face** is named by the operation that made it ([`OpId`], the feature's id) and its
//!   origin in that operation's [`History`]: the side swept by profile curve 7 of region 0
//!   ("side of c7"), a region's start or end cap, the face made from an input edge (a fillet).
//!   A face that a later operation only trims keeps its name. When an operation splits a face,
//!   the pieces get the name with a `split` number, in a stable order (by centroid). When faces
//!   are merged into one (the kernel merges neighbouring faces on one surface after every
//!   operation, as Parasolid does: two adjacent regions' caps, a boss flush with a face), the
//!   face takes the smallest of their names and the others become its aliases
//!   ([`BodyNames::aliases`]), so references to any of them still find it.
//! - An **edge** is named by the (sorted) names of the two faces it separates, plus an index
//!   when several separate chains of edges lie between the same two faces (ordered by
//!   position). Edges that continue each other between the same two faces (a sketch curve cut
//!   into pieces) share one name.
//! - A **vertex** is named by the three smallest names of the faces around it, plus an index.
//!   Points where fewer than three faces meet (a circle's seam) are not vertices.
//!
//! [`resolve`] finds what a stored name (first put through the body's aliases:
//! [`canonical_face`], [`canonical_edge`]) refers to after a rebuild: the entity with that exact
//! name, else one with the same base name (a face that has since been split, an edge that has
//! been re-indexed), nearest to where the reference last was, else, as a geometric fallback, an
//! entity at that position. Anything else is a [`Lost`] reference: an error, never a guess.

use std::collections::HashMap;

use nalgebra::Point3;
use serde::{Deserialize, Serialize};

use crate::{
    BodyId, EdgeId, EdgeInfo, FaceId, FaceInfo, History, Kernel, Origin, Result, VertexId,
    VertexInfo,
};

/// The operation (feature) a name belongs to.
pub type OpId = uuid::Uuid;

/// Where a face came from, within the operation that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FaceOrigin {
    /// A cap of profile region `region` (the region's `source`: `cadrs_core` uses a key of the
    /// region's sketch and boundary curves) (`end`: where the sweep ends, the far end of a
    /// blind extrude; otherwise where it starts).
    Cap { region: u64, end: bool },
    /// The face swept by profile curve `curve` (a sketch curve id) of region `region`.
    Side { region: u64, curve: u64 },
    /// Made from an input edge (a fillet or chamfer face); `edge` is [`stable_hash`] of the
    /// edge's name.
    FromEdge { edge: u64 },
    /// Made from an input vertex; `vertex` is [`stable_hash`] of the vertex's name.
    FromVertex { vertex: u64 },
    /// A face the history says nothing about: the `index`-th such face of the body. Not
    /// stable across rebuilds.
    Unnamed { index: u32 },
    /// A copy a pattern or mirror made (P3.8): of the face named `face` ([`face_hash`] of its
    /// name) made by the operation `of`, in instance `instance` (1, 2, … in the pattern's
    /// order; a mirror's copy is 1).
    Instance { of: OpId, face: u64, instance: u32 },
    /// A face of a body read from a file (the Import feature): face `face` (in the kernel's
    /// face order, which is the same for the same file) of the file's `body`-th body. Stable
    /// across rebuilds of the same file.
    Imported { body: u32, face: u32 },
}

/// A persistent face name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FaceName {
    pub op: OpId,
    pub origin: FaceOrigin,
    /// 0, or which piece of a face that a later operation split.
    #[serde(default)]
    pub split: u32,
}

impl FaceName {
    pub fn new(op: OpId, origin: FaceOrigin) -> Self {
        Self {
            op,
            origin,
            split: 0,
        }
    }

    /// The name before any splits.
    pub fn base(&self) -> Self {
        Self { split: 0, ..*self }
    }

    /// The origin in words: "end cap", "side of c7", ... (the app puts the feature's name in
    /// front: "Extrude 1 / end cap").
    pub fn describe(&self) -> String {
        let mut what = match self.origin {
            FaceOrigin::Cap { end: true, .. } => "end cap".to_string(),
            FaceOrigin::Cap { end: false, .. } => "start cap".to_string(),
            // A sketch curve id's slot index (its low 32 bits).
            FaceOrigin::Side { curve, .. } => format!("side of c{}", curve & 0xffff_ffff),
            FaceOrigin::FromEdge { edge } => format!("face from edge {edge:016x}"),
            FaceOrigin::FromVertex { vertex } => format!("face from vertex {vertex:016x}"),
            FaceOrigin::Unnamed { index } => format!("unnamed face {index}"),
            FaceOrigin::Instance { face, instance, .. } => format!("instance {instance} of face {face:016x}"),
            FaceOrigin::Imported { body, face } => format!("imported face {face} of body {body}"),
        };
        if self.split > 0 {
            what += &format!(", piece {}", self.split);
        }
        what
    }

    /// False for [`FaceOrigin::Unnamed`] faces, whose names don't survive a rebuild.
    pub fn is_stable(&self) -> bool {
        !matches!(self.origin, FaceOrigin::Unnamed { .. })
    }
}

/// A persistent edge name: the faces on either side, and which chain of edges between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeName {
    /// Sorted.
    pub faces: [FaceName; 2],
    #[serde(default)]
    pub index: u32,
}

impl EdgeName {
    /// The edge between `a` and `b` (in either order).
    pub fn new(a: FaceName, b: FaceName, index: u32) -> Self {
        let faces = if a <= b { [a, b] } else { [b, a] };
        Self { faces, index }
    }

    /// The name with index 0 and its faces' base names.
    pub fn base(&self) -> Self {
        Self::new(self.faces[0].base(), self.faces[1].base(), 0)
    }

    /// True if `face` is on one side of it.
    pub fn touches(&self, face: &FaceName) -> bool {
        self.faces.contains(face)
    }

    /// The operation that made it (the later of its faces' operations is not known here: the
    /// first face's).
    pub fn op(&self) -> OpId {
        self.faces[0].op
    }
}

/// A persistent vertex name: the three smallest names of the faces around it, and an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VertexName {
    /// Sorted.
    pub faces: [FaceName; 3],
    #[serde(default)]
    pub index: u32,
}

impl VertexName {
    pub fn base(&self) -> Self {
        let mut faces = self.faces.map(|f| f.base());
        faces.sort();
        Self { faces, index: 0 }
    }
}

/// The names of a body's faces, edges and vertices, indexed by their kernel ids.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyNames {
    /// By [`FaceId`].
    pub faces: Vec<FaceName>,
    /// By [`EdgeId`]; `None` for an edge inside a named face (a seam, or where two pieces of a
    /// face with the same name meet) and for free edges.
    pub edges: Vec<Option<EdgeName>>,
    /// By [`VertexId`]; `None` where fewer than three faces meet.
    pub vertices: Vec<Option<VertexName>>,
    /// Names of faces that were merged into another face (the kernel merges neighbouring faces
    /// on one surface, as Parasolid does): `(merged-away name, the name of the face it is now
    /// part of)`. A merged face takes the smallest of its pieces' names; references to the
    /// others still find it through this list ([`Self::canonical_face`]). Carried on through
    /// later operations while that face lasts.
    pub aliases: Vec<(FaceName, FaceName)>,
}

/// The name `name` is known by now: the face it was merged into, else itself.
pub fn canonical_face(aliases: &[(FaceName, FaceName)], name: &FaceName) -> FaceName {
    aliases.iter().find(|(a, _)| a == name).map_or(*name, |(_, p)| *p)
}

/// An edge name with each of its faces by the name it is known by now ([`canonical_face`]).
pub fn canonical_edge(aliases: &[(FaceName, FaceName)], name: &EdgeName) -> EdgeName {
    EdgeName::new(canonical_face(aliases, &name.faces[0]), canonical_face(aliases, &name.faces[1]), name.index)
}

/// A vertex name with each of its faces by the name it is known by now ([`canonical_face`]).
pub fn canonical_vertex(aliases: &[(FaceName, FaceName)], name: &VertexName) -> VertexName {
    let mut faces = name.faces.map(|f| canonical_face(aliases, &f));
    faces.sort();
    VertexName { faces, index: name.index }
}

impl BodyNames {
    /// The name `name` is known by now on this body ([`canonical_face`]).
    pub fn canonical_face(&self, name: &FaceName) -> FaceName {
        canonical_face(&self.aliases, name)
    }

    pub fn face(&self, id: FaceId) -> Option<FaceName> {
        self.faces.get(id.0 as usize).copied()
    }

    pub fn edge(&self, id: EdgeId) -> Option<EdgeName> {
        self.edges.get(id.0 as usize).copied().flatten()
    }

    pub fn vertex(&self, id: VertexId) -> Option<VertexName> {
        self.vertices.get(id.0 as usize).copied().flatten()
    }

    /// Every face with this name (a face swept by a sketch curve cut into pieces is several
    /// kernel faces with one name), or, for a merged-away name, the face it was merged into.
    pub fn faces_named(&self, name: &FaceName) -> Vec<FaceId> {
        let find = |name: &FaceName| -> Vec<FaceId> {
            (0..self.faces.len())
                .filter(|&i| self.faces[i] == *name)
                .map(|i| FaceId(i as u64))
                .collect()
        };
        match find(name) {
            ids if ids.is_empty() && !self.aliases.is_empty() => find(&self.canonical_face(name)),
            ids => ids,
        }
    }

    /// Every edge with this name (a chain of edges); its faces may be named by merged-away
    /// names.
    pub fn edges_named(&self, name: &EdgeName) -> Vec<EdgeId> {
        let find = |name: &EdgeName| -> Vec<EdgeId> {
            (0..self.edges.len())
                .filter(|&i| self.edges[i] == Some(*name))
                .map(|i| EdgeId(i as u64))
                .collect()
        };
        match find(name) {
            ids if ids.is_empty() && !self.aliases.is_empty() => find(&canonical_edge(&self.aliases, name)),
            ids => ids,
        }
    }

    pub fn vertex_named(&self, name: &VertexName) -> Option<VertexId> {
        let find = |name: &VertexName| (0..self.vertices.len()).find(|&i| self.vertices[i] == Some(*name)).map(|i| VertexId(i as u64));
        find(name).or_else(|| (!self.aliases.is_empty()).then(|| find(&canonical_vertex(&self.aliases, name))).flatten())
    }
}

/// FNV-1a over bytes: a hash that never changes between Rust or cadrs versions, for names that
/// are stored in documents.
pub fn stable_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn face_bytes(f: &FaceName, out: &mut Vec<u8>) {
    out.extend_from_slice(f.op.as_bytes());
    let (tag, a, b) = match f.origin {
        FaceOrigin::Cap { region, end } => (0u8, region, end as u64),
        FaceOrigin::Side { region, curve } => (1, region, curve),
        FaceOrigin::FromEdge { edge } => (2, edge, 0),
        FaceOrigin::FromVertex { vertex } => (3, vertex, 0),
        FaceOrigin::Unnamed { index } => (4, index as u64, 0),
        FaceOrigin::Instance { of, face, instance } => {
            out.extend_from_slice(of.as_bytes());
            (5, face, instance as u64)
        }
        FaceOrigin::Imported { body, face } => (6, body as u64, face as u64),
    };
    out.push(tag);
    out.extend_from_slice(&a.to_le_bytes());
    out.extend_from_slice(&b.to_le_bytes());
    out.extend_from_slice(&f.split.to_le_bytes());
}

/// [`stable_hash`] of a face name.
pub fn face_hash(f: &FaceName) -> u64 {
    let mut bytes = Vec::with_capacity(64);
    face_bytes(f, &mut bytes);
    stable_hash(&bytes)
}

/// [`stable_hash`] of an edge name.
pub fn edge_hash(e: &EdgeName) -> u64 {
    let mut bytes = Vec::with_capacity(96);
    for f in &e.faces {
        face_bytes(f, &mut bytes);
    }
    bytes.extend_from_slice(&e.index.to_le_bytes());
    stable_hash(&bytes)
}

/// [`stable_hash`] of a vertex name.
pub fn vertex_hash(v: &VertexName) -> u64 {
    let mut bytes = Vec::with_capacity(144);
    for f in &v.faces {
        face_bytes(f, &mut bytes);
    }
    bytes.extend_from_slice(&v.index.to_le_bytes());
    stable_hash(&bytes)
}

/// A key that orders points the same way on every rebuild of nearly the same geometry
/// (coordinates rounded to 1 µm).
fn position_key(p: &Point3<f64>) -> [i64; 3] {
    [p.x, p.y, p.z].map(|c| (c * 1e3).round() as i64)
}

/// Names the faces of a body an operation `op` made, from its `history` and the names of its
/// input bodies. `faces` are the body's faces (for the order of split pieces).
pub fn name_faces(
    op: OpId,
    history: &History,
    inputs: &[(BodyId, &BodyNames)],
    faces: &[FaceInfo],
) -> Vec<FaceName> {
    name_faces_aliased(op, history, inputs, faces).0
}

/// [`name_faces`], and the body's aliases ([`BodyNames::aliases`]): the other names of faces
/// that have several (faces the operation merged: a merged face is named after the smallest of
/// its pieces' names), and the inputs' aliases whose face is still there.
pub fn name_faces_aliased(
    op: OpId,
    history: &History,
    inputs: &[(BodyId, &BodyNames)],
    faces: &[FaceInfo],
) -> (Vec<FaceName>, Vec<(FaceName, FaceName)>) {
    let n = faces.len();
    // Every name each face could have.
    let mut names: Vec<Vec<FaceName>> = vec![Vec::new(); n];
    let set = |names: &mut Vec<Vec<FaceName>>, f: FaceId, name: FaceName| {
        if let Some(slot) = names.get_mut(f.0 as usize)
            && !slot.contains(&name)
        {
            slot.push(name);
        }
    };
    let input = |body: BodyId| inputs.iter().find(|(b, _)| *b == body).map(|(_, n)| *n);
    for (f, origin) in &history.generated {
        let o = match *origin {
            Origin::ProfileCurve { region, curve } => Some(FaceOrigin::Side { region, curve }),
            Origin::StartCap { region } => Some(FaceOrigin::Cap { region, end: false }),
            Origin::EndCap { region } => Some(FaceOrigin::Cap { region, end: true }),
            Origin::FromEdge { body, edge } => input(body)
                .and_then(|n| n.edge(edge))
                .map(|e| FaceOrigin::FromEdge { edge: edge_hash(&e) }),
            Origin::FromVertex { body, vertex } => input(body)
                .and_then(|n| n.vertex(vertex))
                .map(|v| FaceOrigin::FromVertex { vertex: vertex_hash(&v) }),
        };
        if let Some(o) = o {
            set(&mut names, *f, FaceName::new(op, o));
        }
    }
    // Modified faces keep their input's name; the pieces of a split face are numbered.
    let mut pieces: HashMap<(BodyId, FaceId), Vec<FaceId>> = HashMap::new();
    let mut order: Vec<(BodyId, FaceId)> = Vec::new();
    for (f, from) in &history.modified {
        let key = (from.body, from.face);
        let list = pieces.entry(key).or_insert_with(|| {
            order.push(key);
            Vec::new()
        });
        if !list.contains(f) {
            list.push(*f);
        }
    }
    for key in order {
        let Some(name) = input(key.0).and_then(|n| n.face(key.1)) else {
            continue;
        };
        let mut outs = pieces.remove(&key).unwrap_or_default();
        if outs.len() == 1 {
            set(&mut names, outs[0], name);
            continue;
        }
        outs.sort_by_key(|f| {
            faces
                .get(f.0 as usize)
                .map(|i| position_key(&i.center))
                .unwrap_or([i64::MAX; 3])
        });
        for (k, f) in outs.into_iter().enumerate() {
            let split = name.split.saturating_mul(16).saturating_add(k as u32 + 1);
            set(&mut names, f, FaceName { split, ..name });
        }
    }
    // Several sources for one face (faces merged into one): the smallest name, so the choice
    // is stable; the others become aliases of it.
    let mut unnamed = 0;
    let mut merged: Vec<(FaceName, FaceName)> = Vec::new();
    let out: Vec<FaceName> = names
        .into_iter()
        .map(|mut n| {
            n.sort();
            let Some(&first) = n.first() else {
                unnamed += 1;
                return FaceName::new(op, FaceOrigin::Unnamed { index: unnamed - 1 });
            };
            merged.extend(n[1..].iter().map(|a| (*a, first)));
            first
        })
        .collect();
    let primary: std::collections::HashSet<FaceName> = out.iter().copied().collect();
    let bases: std::collections::HashSet<FaceName> = out.iter().map(FaceName::base).collect();
    let mut aliases: Vec<(FaceName, FaceName)> = Vec::new();
    let mut push = |a: FaceName, p: FaceName| {
        if a != p && !primary.contains(&a) && !aliases.iter().any(|(x, _)| *x == a) {
            aliases.push((a, p));
        }
    };
    for (a, p) in &merged {
        push(*a, *p);
    }
    // The inputs' aliases, through this operation's merges, while their face lasts (or its
    // pieces, found by base name when resolving).
    for (_, input) in inputs {
        for (a, p) in &input.aliases {
            let p = canonical_face(&merged, p);
            if primary.contains(&p) || bases.contains(&p.base()) {
                push(*a, p);
            }
        }
    }
    (out, aliases)
}

/// Names a body's edges from its face names (see the module docs). `tol`: how close two edge
/// ends must be to count as joined (mm).
pub fn name_edges(faces: &[FaceName], edges: &[EdgeInfo], tol: f64) -> Vec<Option<EdgeName>> {
    let face = |f: Option<FaceId>| f.and_then(|f| faces.get(f.0 as usize)).copied();
    let mut groups: HashMap<EdgeName, Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        let (a, b) = match (face(e.faces[0]), face(e.faces[1])) {
            (Some(a), Some(b)) if a != b => (a, b),
            // A free edge (the boundary of a surface): named by its one face, twice.
            (Some(a), None) | (None, Some(a)) => (a, a),
            // A seam, or an edge inside one named face.
            _ => continue,
        };
        groups.entry(EdgeName::new(a, b, 0)).or_default().push(i);
    }
    let mut out = vec![None; edges.len()];
    for (pair, members) in groups {
        // Chains: edges joined end to end.
        let mut chain: Vec<usize> = (0..members.len()).collect();
        fn root(c: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while c[r] != r {
                r = c[r];
            }
            c[i] = r;
            r
        }
        for a in 0..members.len() {
            for b in a + 1..members.len() {
                let (ea, eb) = (&edges[members[a]], &edges[members[b]]);
                let joined = [ea.start, ea.end]
                    .iter()
                    .any(|p| [eb.start, eb.end].iter().any(|q| (p - q).norm() < tol));
                if joined {
                    let (ra, rb) = (root(&mut chain, a), root(&mut chain, b));
                    chain[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        let mut chains: HashMap<usize, Vec<usize>> = HashMap::new();
        for (k, &m) in members.iter().enumerate() {
            let r = root(&mut chain, k);
            chains.entry(r).or_default().push(m);
        }
        let mut chains: Vec<Vec<usize>> = chains.into_values().collect();
        let key = |c: &Vec<usize>| c.iter().map(|&e| position_key(&edges[e].mid)).min();
        chains.sort_by_key(key);
        for (index, c) in chains.into_iter().enumerate() {
            for e in c {
                out[e] = Some(EdgeName {
                    index: index as u32,
                    ..pair
                });
            }
        }
    }
    out
}

/// Names a body's vertices from its face names (see the module docs).
pub fn name_vertices(
    faces: &[FaceName],
    edges: &[EdgeInfo],
    vertices: &[VertexInfo],
) -> Vec<Option<VertexName>> {
    let mut keyed: Vec<(usize, [FaceName; 3])> = Vec::new();
    for (i, v) in vertices.iter().enumerate() {
        let mut around: Vec<FaceName> = v
            .edges
            .iter()
            .filter_map(|e| edges.get(e.0 as usize))
            .flat_map(|e| e.faces.into_iter().flatten())
            .filter_map(|f| faces.get(f.0 as usize).copied())
            .collect();
        around.sort();
        around.dedup();
        if around.len() >= 3 {
            keyed.push((i, [around[0], around[1], around[2]]));
        } else if !around.is_empty()
            && v.edges.iter().filter_map(|e| edges.get(e.0 as usize)).any(|e| e.faces.iter().flatten().count() < 2)
        {
            // A sheet's corner (on an edge with one face): its one or two faces, the last
            // repeated; told apart from the others there by `index`.
            let last = around[around.len() - 1];
            keyed.push((i, [around[0], *around.get(1).unwrap_or(&last), last]));
        }
    }
    let mut groups: HashMap<[FaceName; 3], Vec<usize>> = HashMap::new();
    for (i, k) in &keyed {
        groups.entry(*k).or_default().push(*i);
    }
    let mut out = vec![None; vertices.len()];
    for (k, mut members) in groups {
        members.sort_by_key(|&i| position_key(&vertices[i].point));
        for (index, i) in members.into_iter().enumerate() {
            out[i] = Some(VertexName {
                faces: k,
                index: index as u32,
            });
        }
    }
    out
}

/// Names every face, edge and vertex of `body`, which operation `op` made with `history` from
/// the `inputs` bodies.
pub fn name_body(
    k: &dyn Kernel,
    body: BodyId,
    op: OpId,
    history: &History,
    inputs: &[(BodyId, &BodyNames)],
) -> Result<BodyNames> {
    let faces = k.faces(body)?;
    let edges = k.edges(body)?;
    let vertices = k.vertices(body)?;
    let (face_names, aliases) = name_faces_aliased(op, history, inputs, &faces);
    let tol = joint_tolerance(&edges);
    Ok(BodyNames {
        edges: name_edges(&face_names, &edges, tol),
        vertices: name_vertices(&face_names, &edges, &vertices),
        faces: face_names,
        aliases,
    })
}

/// How close edge ends must be to count as joined: 1e-7 of the body's size (at least 1e-9 mm).
pub fn joint_tolerance(edges: &[EdgeInfo]) -> f64 {
    let size = edges
        .iter()
        .flat_map(|e| [e.start, e.end])
        .fold(0.0f64, |m, p| m.max(p.x.abs()).max(p.y.abs()).max(p.z.abs()));
    (size * 1e-7).max(1e-9)
}

// ---------------------------------------------------------------------------------------------
// Resolving

/// A name with a base form (see [`FaceName::base`], [`EdgeName::base`]).
pub trait Named: Copy + Eq {
    fn base_name(&self) -> Self;
}

impl Named for FaceName {
    fn base_name(&self) -> Self {
        self.base()
    }
}

impl Named for EdgeName {
    fn base_name(&self) -> Self {
        self.base()
    }
}

impl Named for VertexName {
    fn base_name(&self) -> Self {
        self.base()
    }
}

/// How a reference was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// By its exact name.
    Exact,
    /// By its base name: a split face or a re-indexed edge (the candidate nearest the hint).
    Renamed,
    /// By position only (the geometric fallback).
    Geometric,
}

/// A reference that no longer resolves: an error, rather than a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lost;

impl std::fmt::Display for Lost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lost reference")
    }
}

impl std::error::Error for Lost {}

/// Finds the candidate a stored `name` refers to (see the module docs). `distance(i)` is how
/// far candidate `i` is from where the reference last was (`None` if that isn't known or the
/// candidate can't be compared); a geometric match must be within `tol`.
pub fn resolve<N: Named>(
    name: &N,
    candidates: &[N],
    distance: impl Fn(usize) -> Option<f64>,
    tol: f64,
) -> std::result::Result<(usize, Match), Lost> {
    if let Some(i) = candidates.iter().position(|c| c == name) {
        return Ok((i, Match::Exact));
    }
    let base = name.base_name();
    // The candidate nearest the hint, if the hint tells them apart.
    let nearest = |pool: &[usize]| -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for &i in pool {
            let Some(d) = distance(i) else { continue };
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((i, d));
            }
        }
        best
    };
    let same_base: Vec<usize> = (0..candidates.len())
        .filter(|&i| candidates[i].base_name() == base)
        .collect();
    match same_base.as_slice() {
        [] => {}
        [only] => return Ok((*only, Match::Renamed)),
        several => {
            // Several pieces: the one nearest where the reference was; without a hint that
            // tells them apart, fall through (no guessing).
            if let Some((i, _)) = nearest(several) {
                return Ok((i, Match::Renamed));
            }
        }
    }
    let all: Vec<usize> = (0..candidates.len()).collect();
    match nearest(&all) {
        Some((i, d)) if d <= tol => Ok((i, Match::Geometric)),
        _ => Err(Lost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Vector3;

    fn op(n: u128) -> OpId {
        uuid::Uuid::from_u128(n)
    }

    fn info(center: [f64; 3]) -> FaceInfo {
        FaceInfo {
            id: FaceId(0),
            kind: crate::SurfaceKind::Plane,
            plane: None,
            area: 1.0,
            center: Point3::from(center),
            axis: None,
            radius: None,
        }
    }

    fn edge(faces: [u64; 2], a: [f64; 3], b: [f64; 3]) -> EdgeInfo {
        let (a, b) = (Point3::from(a), Point3::from(b));
        EdgeInfo {
            id: EdgeId(0),
            faces: [Some(FaceId(faces[0])), Some(FaceId(faces[1]))],
            length: (b - a).norm(),
            curve: crate::CurveKind::Line,
            start: a,
            end: b,
            mid: nalgebra::center(&a, &b),
            start_tangent: Vector3::x(),
            end_tangent: Vector3::x(),
            circle: None,
        }
    }

    /// Generated faces are named after their origin; faces the history doesn't mention are
    /// unnamed; a face split in two gets numbered pieces in centroid order.
    #[test]
    fn faces_from_history() {
        let a = op(1);
        let h = History {
            generated: vec![
                (FaceId(0), Origin::StartCap { region: 0 }),
                (FaceId(1), Origin::EndCap { region: 0 }),
                (FaceId(2), Origin::ProfileCurve { region: 0, curve: 7 }),
            ],
            ..History::default()
        };
        let faces: Vec<FaceInfo> = (0..4).map(|_| info([0.0; 3])).collect();
        let names = name_faces(a, &h, &[], &faces);
        assert_eq!(names[0].origin, FaceOrigin::Cap { region: 0, end: false });
        assert_eq!(names[1].origin, FaceOrigin::Cap { region: 0, end: true });
        assert_eq!(names[2].origin, FaceOrigin::Side { region: 0, curve: 7 });
        assert_eq!(names[3].origin, FaceOrigin::Unnamed { index: 0 });
        assert_eq!(names[2].describe(), "side of c7");

        // A second operation trims face 1 and splits face 2.
        let input = BodyNames {
            faces: names.clone(),
            ..BodyNames::default()
        };
        let body = BodyId(9);
        let h2 = History {
            modified: vec![
                (FaceId(0), crate::InputFace { body, face: FaceId(1) }),
                (FaceId(1), crate::InputFace { body, face: FaceId(2) }),
                (FaceId(2), crate::InputFace { body, face: FaceId(2) }),
            ],
            deleted: vec![crate::InputFace { body, face: FaceId(0) }],
            ..History::default()
        };
        let faces2 = vec![info([0.0; 3]), info([5.0, 0.0, 0.0]), info([-5.0, 0.0, 0.0])];
        let names2 = name_faces(op(2), &h2, &[(body, &input)], &faces2);
        assert_eq!(names2[0], names[1]);
        // The piece further along x is the second piece.
        assert_eq!(names2[2], FaceName { split: 1, ..names[2] });
        assert_eq!(names2[1], FaceName { split: 2, ..names[2] });
        assert_eq!(names2[1].base(), names[2]);
    }

    /// A face merged from two regions' caps is named after the smaller cap; the other cap's name
    /// is an alias of it, carried through a later operation that keeps the face, and edge names
    /// through the merged-away face are found through it.
    #[test]
    fn merged_faces_keep_their_other_names() {
        let a = op(1);
        let cap = |region| FaceName::new(a, FaceOrigin::Cap { region, end: true });
        let side = FaceName::new(a, FaceOrigin::Side { region: 5, curve: 1 });
        let h = History {
            generated: vec![
                (FaceId(0), Origin::EndCap { region: 5 }),
                (FaceId(0), Origin::EndCap { region: 3 }),
                (FaceId(1), Origin::ProfileCurve { region: 5, curve: 1 }),
            ],
            ..History::default()
        };
        let faces = vec![info([0.0; 3]), info([1.0, 0.0, 0.0])];
        let (names, aliases) = name_faces_aliased(a, &h, &[], &faces);
        assert_eq!(names, vec![cap(3), side]);
        assert_eq!(aliases, vec![(cap(5), cap(3))]);
        let body = BodyNames { faces: names, aliases, ..BodyNames::default() };
        assert_eq!(body.canonical_face(&cap(5)), cap(3));
        assert_eq!(body.canonical_face(&side), side);
        let e = EdgeName::new(cap(5), side, 0);
        assert_eq!(canonical_edge(&body.aliases, &e), EdgeName::new(cap(3), side, 0));

        // A later operation keeps both faces: the alias stays. One that deletes the cap drops it.
        let b = BodyId(1);
        let keep = History {
            modified: vec![
                (FaceId(0), crate::InputFace { body: b, face: FaceId(0) }),
                (FaceId(1), crate::InputFace { body: b, face: FaceId(1) }),
            ],
            ..History::default()
        };
        let (_, kept) = name_faces_aliased(op(2), &keep, &[(b, &body)], &faces);
        assert_eq!(kept, vec![(cap(5), cap(3))]);
        let drop = History {
            modified: vec![(FaceId(0), crate::InputFace { body: b, face: FaceId(1) })],
            deleted: vec![crate::InputFace { body: b, face: FaceId(0) }],
            ..History::default()
        };
        let (_, dropped) = name_faces_aliased(op(2), &drop, &[(b, &body)], &faces[..1]);
        assert!(dropped.is_empty());
    }

    /// Edges between the same two faces are one name where they join, indexed where they don't.
    #[test]
    fn edge_chains_and_indices() {
        let a = op(1);
        let faces = [
            FaceName::new(a, FaceOrigin::Cap { region: 0, end: true }),
            FaceName::new(a, FaceOrigin::Side { region: 0, curve: 1 }),
            FaceName::new(a, FaceOrigin::Side { region: 0, curve: 1 }),
        ];
        let edges = vec![
            // Two pieces of one curve's top edge, joined at x = 5.
            edge([0, 1], [0.0, 0.0, 1.0], [5.0, 0.0, 1.0]),
            edge([2, 0], [5.0, 0.0, 1.0], [9.0, 0.0, 1.0]),
            // Where the two pieces meet: inside one named face.
            edge([1, 2], [5.0, 0.0, 0.0], [5.0, 0.0, 1.0]),
            // Another, separate stretch between the same faces.
            edge([0, 1], [20.0, 0.0, 1.0], [30.0, 0.0, 1.0]),
        ];
        let names = name_edges(&faces, &edges, 1e-9);
        assert_eq!(names[0], names[1]);
        assert_eq!(names[2], None);
        assert_eq!(names[0].unwrap().index, 0);
        assert_eq!(names[3].unwrap().index, 1);
        assert_eq!(names[3].unwrap().base(), names[0].unwrap());
    }

    #[test]
    fn resolving_prefers_exact_then_base_then_position() {
        let a = op(1);
        let n = |split| FaceName {
            split,
            ..FaceName::new(a, FaceOrigin::Side { region: 0, curve: 3 })
        };
        let other = FaceName::new(a, FaceOrigin::Cap { region: 0, end: true });
        let cands = [n(1), n(2), other];
        let d = |i: usize| Some([4.0, 1.0, 9.0][i]);
        assert_eq!(resolve(&n(2), &cands, d, 0.1), Ok((1, Match::Exact)));
        // Unsplit before: the nearest piece.
        assert_eq!(resolve(&n(0), &cands, d, 0.1), Ok((1, Match::Renamed)));
        // A face that is gone: only a candidate at the old position counts.
        let gone = FaceName::new(op(7), FaceOrigin::Cap { region: 0, end: false });
        assert_eq!(resolve(&gone, &cands, d, 0.1), Err(Lost));
        let d0 = |i: usize| Some([4.0, 1.0, 0.0][i]);
        assert_eq!(resolve(&gone, &cands, d0, 0.1), Ok((2, Match::Geometric)));
    }
}
