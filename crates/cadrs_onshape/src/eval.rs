//! Evaluates Onshape queries against the cadrs model.
//!
//! A decoded query ([`crate::query`]) says how Onshape found an entity: the operation that made
//! it (`operationId`), what kind of result it was (`queryType`: a `CAP_FACE`, a `SWEPT_FACE`,
//! the `INTERSECT`ion of two faces, …) and, where that is ambiguous, disambiguation data (made
//! from these sketch entities, next to these edges). The same questions can be asked of the
//! cadrs model rebuilt up to the feature that refers to it, since cadrs names faces by the
//! operation and sketch curves that made them and edges by their two faces:
//!
//! - candidates: the faces (or edges) the cadrs feature mapped from `operationId` made, of the
//!   kind the query type says (caps, sides), loosely where cadrs names them differently;
//! - `ORIGINAL_DEPENDENCY`: keep those made from the listed sketch entities;
//! - `TOPOLOGY`, `TRUE_DEPENDENCY`: keep those next to the listed entities (evaluated in turn);
//! - `INTERSECT`: the edges between the faces the operands evaluate to; `MERGE`, `SPLIT`,
//!   `COPY`: what they were derived from.
//!
//! Filters that would leave nothing are ignored, so a partly translatable query still finds
//! its entity when the rest is unambiguous.

use std::collections::{BTreeSet, HashMap, HashSet};

use cadrs_core::document::{EdgeRef, FaceRef};
use cadrs_core::ids::FeatureId;
use cadrs_core::parts::Part;
use cadrs_kernel::naming::{FaceName, FaceOrigin};
use cadrs_sketch::Sketch;

use crate::query::Value;
use crate::refs::{op_feature, region_with, sketch_entities};
use crate::sketch::SketchMap;

/// A face or edge of the model: (part index, face or edge index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ent {
    Face(usize, usize),
    Edge(usize, usize),
}

/// A Derived feature's source Part Studio as the importer imported it: what its queries are
/// evaluated against (Onshape names geometry of derived parts by the source studio's history).
pub struct SourceCtx {
    pub features: HashMap<String, FeatureId>,
    pub sketches: HashMap<String, (SketchMap, Sketch)>,
    pub parts: Vec<Part>,
}

type Sources = std::sync::Mutex<HashMap<(String, String), std::sync::Arc<SourceCtx>>>;

fn sources() -> &'static Sources {
    static S: std::sync::OnceLock<Sources> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// The imported Part Studio (Onshape document and element ids), if it was imported in this run.
pub fn source(doc: &str, element: &str) -> Option<std::sync::Arc<SourceCtx>> {
    sources().lock().ok()?.get(&(doc.to_string(), element.to_string())).cloned()
}

/// Records an imported Part Studio for Derived features that come after it.
pub fn set_source(doc: &str, element: &str, ctx: SourceCtx) {
    if let Ok(mut s) = sources().lock() {
        s.insert((doc.to_string(), element.to_string()), std::sync::Arc::new(ctx));
    }
}

/// Onshape part id → (cadrs part, its offset in mm).
pub type PartMap = HashMap<String, (cadrs_core::ids::PartId, [f64; 3])>;

type PartMaps = std::sync::Mutex<HashMap<(String, String), std::sync::Arc<PartMap>>>;

fn part_maps() -> &'static PartMaps {
    static S: std::sync::OnceLock<PartMaps> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// Which cadrs part each Onshape part (by part id) of an imported Part Studio became (the parts
/// matched by volume and box, see the Part Studio's `name_parts`), and how far (mm) the cadrs part
/// sits from Onshape's, if it was imported in this run.
pub fn part_map(doc: &str, element: &str) -> Option<std::sync::Arc<PartMap>> {
    part_maps().lock().ok()?.get(&(doc.to_string(), element.to_string())).cloned()
}

/// Records [`part_map`] for an imported Part Studio.
pub fn set_part_map(doc: &str, element: &str, map: PartMap) {
    if let Ok(mut s) = part_maps().lock() {
        s.insert((doc.to_string(), element.to_string()), std::sync::Arc::new(map));
    }
}

/// The `<feature>.derived.*.merge.` prefix of the first operation id in `q` that has one.
fn derived_prefix(q: &Value) -> Option<String> {
    match q {
        Value::Str(s) => s.find(".derived.*.merge.").map(|i| s[..i + ".derived.*.merge.".len()].to_string()),
        Value::Typed(_, v) => derived_prefix(v),
        Value::Map(m) => m.iter().find_map(|(_, v)| derived_prefix(v)),
        Value::Array(a) => a.iter().find_map(derived_prefix),
        _ => None,
    }
}

/// `q` with `prefix` taken off every string that starts with it.
fn strip_prefix(q: &Value, prefix: &str) -> Value {
    match q {
        Value::Str(s) => Value::Str(s.strip_prefix(prefix).unwrap_or(s).to_string()),
        Value::Typed(t, v) => Value::Typed(t.clone(), Box::new(strip_prefix(v, prefix))),
        Value::Map(m) => Value::Map(m.iter().map(|(k, v)| (k.clone(), strip_prefix(v, prefix))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(|v| strip_prefix(v, prefix)).collect()),
        other => other.clone(),
    }
}

/// A Derived feature's source, loaded the first time a query needs it (loading another
/// document's studio means importing it).
pub struct LazySource {
    pub root: std::path::PathBuf,
    pub doc: String,
    pub element: String,
    cell: std::sync::OnceLock<Option<std::sync::Arc<SourceCtx>>>,
}

impl LazySource {
    pub fn new(root: std::path::PathBuf, doc: String, element: String) -> Self {
        Self { root, doc, element, cell: std::sync::OnceLock::new() }
    }

    pub fn get(&self) -> Option<std::sync::Arc<SourceCtx>> {
        self.cell.get_or_init(|| crate::import::load_source(&self.root, &self.doc, &self.element)).clone()
    }
}

/// Onshape Derived feature id → the cadrs feature and its source.
pub type DerivedSources = HashMap<String, (FeatureId, std::sync::Arc<LazySource>)>;

/// What queries are evaluated against.
pub struct Model<'a> {
    pub parts: &'a [Part],
    /// Onshape feature id → cadrs feature.
    pub features: &'a HashMap<String, FeatureId>,
    /// Onshape sketch feature id → its id maps and geometry.
    pub sketches: &'a HashMap<String, (SketchMap, Sketch)>,
    /// Per part: each face's edges, each edge's faces (indices), built once (imported meshes
    /// have thousands of faces).
    face_edges: Vec<Vec<Vec<usize>>>,
    edge_faces: Vec<Vec<Vec<usize>>>,
    /// The Part Studio's features, for numbering copies (see [`Model::with_doc`]).
    doc: &'a [cadrs_core::document::Feature],
    /// The studio's Derived features' sources.
    derived: Option<&'a DerivedSources>,
}

impl<'a> Model<'a> {
    pub fn new(parts: &'a [Part], features: &'a HashMap<String, FeatureId>, sketches: &'a HashMap<String, (SketchMap, Sketch)>) -> Self {
        let mut face_edges = Vec::with_capacity(parts.len());
        let mut edge_faces = Vec::with_capacity(parts.len());
        for part in parts {
            let s = &part.solid;
            let index: HashMap<FaceName, usize> = s.faces.iter().enumerate().map(|(i, f)| (f.name, i)).collect();
            let mut fe = vec![Vec::new(); s.faces.len()];
            let mut ef = Vec::with_capacity(s.edges.len());
            for (e, edge) in s.edges.iter().enumerate() {
                let faces: Vec<usize> = edge.name.faces.iter().filter_map(|n| index.get(n).copied()).collect();
                for &f in &faces {
                    fe[f].push(e);
                }
                ef.push(faces);
            }
            face_edges.push(fe);
            edge_faces.push(ef);
        }
        Self { parts, features, sketches, face_edges, edge_faces, doc: &[], derived: None }
    }

    /// With the Part Studio's features, so `COPY` queries find the copy Onshape numbered (a
    /// pattern's instances are numbered differently in cadrs).
    pub fn with_doc(mut self, doc: &'a [cadrs_core::document::Feature]) -> Self {
        self.doc = doc;
        self
    }

    /// Resolves queries on derived parts through their sources.
    pub fn with_derived(mut self, derived: &'a DerivedSources) -> Self {
        self.derived = Some(derived);
        self
    }
}

impl Model<'_> {
    fn face_name(&self, e: Ent) -> Option<FaceName> {
        match e {
            Ent::Face(p, f) => Some(self.parts[p].solid.faces[f].name),
            Ent::Edge(..) => None,
        }
    }

    /// The persistent hash of an edge's name (what `FaceOrigin::FromEdge` holds).
    fn edge_hash(&self, e: Ent) -> Option<u64> {
        match e {
            Ent::Edge(p, i) => Some(cadrs_kernel::naming::edge_hash(&self.parts[p].solid.edges[i].name)),
            Ent::Face(..) => None,
        }
    }

    fn all_faces(&self) -> impl Iterator<Item = Ent> + '_ {
        self.parts.iter().enumerate().flat_map(|(p, part)| (0..part.solid.faces.len()).map(move |f| Ent::Face(p, f)))
    }

    fn all_edges(&self) -> impl Iterator<Item = Ent> + '_ {
        self.parts.iter().enumerate().flat_map(|(p, part)| (0..part.solid.edges.len()).map(move |e| Ent::Edge(p, e)))
    }

    /// The faces of edge `e`, or the edges of face `f`: what is next to it.
    fn adjacent(&self, e: Ent) -> Vec<Ent> {
        match e {
            Ent::Edge(p, i) => self.edge_faces[p][i].iter().map(|&f| Ent::Face(p, f)).collect(),
            Ent::Face(p, i) => self.face_edges[p][i].iter().map(|&e| Ent::Edge(p, e)).collect(),
        }
    }

    /// Faces or edges touching `e` through a shared edge or face (a face's neighbouring faces,
    /// an edge's faces' edges) and `e`'s own adjacent entities.
    fn near(&self, e: Ent) -> HashSet<Ent> {
        let mut out: HashSet<Ent> = self.adjacent(e).into_iter().collect();
        for a in self.adjacent(e) {
            out.extend(self.adjacent(a));
        }
        out
    }

    /// The cadrs curve sources (as face names hold them) of the sketch entities `q` names
    /// directly (its own `originals`, not those of queries it refers to).
    fn original_curves(&self, originals: &[Value]) -> BTreeSet<u64> {
        let mut out = BTreeSet::new();
        for o in originals {
            if o.get("queryType").and_then(Value::as_str) != Some("SKETCH_ENTITY") {
                continue;
            }
            let (Some(op), Some(id)) = (o.get("operationId").and_then(Value::as_str), o.get("sketchEntityId").and_then(Value::as_str)) else { continue };
            if let Some((map, _)) = self.sketches.get(op_feature(op))
                && let Some(c) = map.curves.get(id)
            {
                out.insert(cadrs_core::brep::curve_source(*c));
            }
        }
        out
    }

    /// The region key the sketch entities in `originals` bound, if they bound one.
    fn original_region(&self, originals: &[Value]) -> Option<u64> {
        let first = originals.iter().find_map(|o| o.get("operationId").and_then(Value::as_str))?;
        let sketch = op_feature(first);
        let (map, g) = self.sketches.get(sketch)?;
        let mut ids = BTreeSet::new();
        for o in originals {
            sketch_entities(o, sketch, &mut ids);
        }
        let curves = ids.iter().filter_map(|i| map.curves.get(i).copied()).collect();
        region_with(map.feature, g, &curves).map(|r| r.key())
    }

    /// The entities `q` refers to (possibly several, possibly none).
    pub fn eval(&self, q: &Value) -> Vec<Ent> {
        self.eval_depth(q, 0)
    }

    fn eval_depth(&self, q: &Value, depth: usize) -> Vec<Ent> {
        if depth > 12 {
            return Vec::new();
        }
        let entity_type = q.get("entityType").and_then(Value::as_str).unwrap_or_default();
        let qtype = q.get("queryType").and_then(Value::as_str).unwrap_or_default();
        let derived = || -> Vec<&Value> {
            match q.get("derivedFrom") {
                Some(Value::Array(a)) => a.iter().collect(),
                Some(v) => vec![v],
                None => Vec::new(),
            }
        };
        let mut cands: Vec<Ent> = match qtype {
            "COPY" => self.copies(q, &derived(), depth),
            "MERGE" | "SPLIT" => {
                // A merge is the one entity all its histories describe: what they have in common,
                // else (cadrs kept only one of the names) any of them.
                let sets: Vec<Vec<Ent>> = derived().into_iter().map(|d| self.eval_depth(d, depth + 1)).collect();
                let common: Vec<Ent> = match sets.split_first() {
                    Some((first, rest)) if qtype == "MERGE" => first.iter().copied().filter(|e| rest.iter().all(|r| r.contains(e))).collect(),
                    _ => Vec::new(),
                };
                let mut v: Vec<Ent> = if common.is_empty() { sets.into_iter().flatten().collect() } else { common };
                v.sort();
                v.dedup();
                v
            }
            "INTERSECT" => {
                let sides: Vec<HashSet<Ent>> = derived().into_iter().map(|d| self.eval_depth(d, depth + 1).into_iter().collect()).collect();
                if sides.len() < 2 {
                    return Vec::new();
                }
                // The edges whose faces include one of each operand.
                self.all_edges()
                    .filter(|e| {
                        let faces: HashSet<Ent> = self.adjacent(*e).into_iter().collect();
                        sides.iter().all(|s| s.iter().any(|f| faces.contains(f)))
                    })
                    .collect()
            }
            _ => {
                let Some(op) = q.get("operationId").and_then(Value::as_str) else { return Vec::new() };
                let Some(f) = self.features.get(op_feature(op)) else { return Vec::new() };
                let op = f.0;
                match entity_type {
                    "FACE" => self.all_faces().filter(|e| self.face_name(*e).is_some_and(|n| n.op == op)).collect(),
                    "EDGE" => self
                        .all_edges()
                        .filter(|e| {
                            let Ent::Edge(p, i) = *e else { return false };
                            self.parts[p].solid.edges[i].name.faces.iter().any(|n| n.op == op)
                        })
                        .collect(),
                    _ => Vec::new(),
                }
            }
        };
        if cands.is_empty() {
            return cands;
        }

        // Narrow by what the query type says the entity is.
        let is_start = q.get("isStart").and_then(Value::as_bool);
        let op_id = q.get("operationId").and_then(Value::as_str).and_then(|o| self.features.get(op_feature(o))).map(|f| f.0);
        let narrow = |cands: Vec<Ent>, keep: &dyn Fn(Ent) -> bool| -> Vec<Ent> {
            let kept: Vec<Ent> = cands.iter().copied().filter(|e| keep(*e)).collect();
            if kept.is_empty() { cands } else { kept }
        };
        let cap_end = |n: &FaceName| match n.origin {
            FaceOrigin::Cap { end, .. } => Some(end),
            _ => None,
        };
        match qtype {
            "CAP_FACE" => {
                let end = is_start.map(|s| !s);
                cands = narrow(cands, &|e| self.face_name(e).and_then(|n| cap_end(&n)).is_some_and(|x| end.is_none_or(|end| end == x)));
            }
            "SWEPT_FACE" => cands = narrow(cands, &|e| self.face_name(e).is_some_and(|n| cap_end(&n).is_none())),
            "CAP_EDGE" => {
                let end = is_start.map(|s| !s);
                cands = narrow(cands, &|e| {
                    self.adjacent(e).iter().filter_map(|f| self.face_name(*f)).any(|n| Some(n.op) == op_id && cap_end(&n).is_some_and(|x| end.is_none_or(|end| end == x)))
                });
            }
            "SWEPT_EDGE" => {
                cands = narrow(cands, &|e| {
                    let faces: Vec<FaceName> = self.adjacent(e).iter().filter_map(|f| self.face_name(*f)).collect();
                    faces.len() == 2 && faces.iter().all(|n| Some(n.op) == op_id && cap_end(n).is_none())
                });
            }
            _ => {}
        }

        // Disambiguation.
        for d in q.get("disambiguationData").map(Value::items).unwrap_or_default() {
            if cands.len() <= 1 {
                break;
            }
            match d.get("disambiguationType").and_then(Value::as_str).unwrap_or_default() {
                "ORIGINAL_DEPENDENCY" => {
                    let originals = d.get("originals").map(Value::items).unwrap_or_default();
                    let curves = self.original_curves(originals);
                    let region = self.original_region(originals);
                    // A face an extrude of faces made from an edge (`FromEdge`): the edge is made
                    // from these curves when a side face along it is.
                    let from_curves: HashSet<u64> = self
                        .all_edges()
                        .filter(|e| self.adjacent(*e).iter().filter_map(|f| self.face_name(*f)).any(|n| matches!(n.origin, FaceOrigin::Side { curve, .. } if curves.contains(&curve))))
                        .filter_map(|e| self.edge_hash(e))
                        .collect();
                    cands = narrow(cands, &|e| match e {
                        Ent::Face(..) => self.face_name(e).is_some_and(|n| match n.origin {
                            FaceOrigin::Side { curve, .. } => curves.contains(&curve),
                            FaceOrigin::Cap { region: r, .. } => region.is_some_and(|x| x == r),
                            FaceOrigin::FromEdge { edge } => from_curves.contains(&edge),
                            _ => false,
                        }),
                        // An edge made from these curves: a side face of it is made from one.
                        Ent::Edge(..) => self.adjacent(e).iter().filter_map(|f| self.face_name(*f)).any(|n| match n.origin {
                            FaceOrigin::Side { curve, .. } => curves.contains(&curve),
                            FaceOrigin::FromEdge { edge } => from_curves.contains(&edge),
                            _ => false,
                        }),
                    });
                    // Caps: the region's own cap when the region is known.
                }
                "TOPOLOGY" | "TRUE_DEPENDENCY" => {
                    let mut near_all: Option<HashSet<Ent>> = None;
                    let refs: Vec<&Value> = match d.get("entities").or_else(|| d.get("derivedFrom")) {
                        Some(Value::Array(a)) => a.iter().collect(),
                        Some(v) => vec![v],
                        None => Vec::new(),
                    };
                    for r in refs {
                        // `[query, n]` pairs or plain queries.
                        let (qr, sign) = match r {
                            Value::Array(pair) if pair.len() == 2 => (&pair[0], pair[1].as_f64().unwrap_or(1.0)),
                            v => (v, 1.0),
                        };
                        if sign < 0.0 {
                            continue;
                        }
                        let found = self.eval_depth(qr, depth + 1);
                        if found.is_empty() {
                            continue;
                        }
                        // A face made from one of these edges (an extrude of faces' side).
                        let hashes: HashSet<u64> = found.iter().filter_map(|f| self.edge_hash(*f)).collect();
                        if !hashes.is_empty() {
                            let from: Vec<Ent> = cands
                                .iter()
                                .copied()
                                .filter(|c| self.face_name(*c).is_some_and(|n| matches!(n.origin, FaceOrigin::FromEdge { edge } if hashes.contains(&edge))))
                                .collect();
                            if !from.is_empty() {
                                cands = from;
                                continue;
                            }
                        }
                        let near: HashSet<Ent> = found.iter().flat_map(|f| self.near(*f).into_iter().chain([*f])).collect();
                        near_all = Some(match near_all {
                            None => near,
                            Some(prev) => prev.intersection(&near).copied().collect(),
                        });
                    }
                    if let Some(near) = near_all {
                        cands = narrow(cands, &|e| near.contains(&e));
                    }
                }
                _ => {}
            }
        }
        cands
    }

    /// A `COPY` query: the copies a pattern, mirror or transform (its `operationId`) made of
    /// what it was derived from, in instance `instanceName` if that matches cadrs's numbering,
    /// else in any instance.
    /// A `COPY` by a Derived feature: the entities its source query finds in the source studio,
    /// under their derived names here.
    fn derived_copies(&self, fid: FeatureId, ctx: &SourceCtx, derived: &[&Value]) -> Vec<Ent> {
        use cadrs_core::derived::face_name;
        let src = Model::new(&ctx.parts, &ctx.features, &ctx.sketches);
        // Inside, Onshape names the source's operations `<derived feature>.derived.*.merge.<op>`.
        let Some(prefix) = derived.iter().find_map(|d| derived_prefix(d)) else { return Vec::new() };
        let derived: Vec<Value> = derived.iter().map(|d| strip_prefix(d, &prefix)).collect();
        let mut out = Vec::new();
        for d in &derived {
            for e in src.eval(d) {
                match e {
                    Ent::Face(p, f) => {
                        let n = face_name(fid, &ctx.parts[p].solid.faces[f].name);
                        out.extend(self.all_faces().filter(|x| self.face_name(*x) == Some(n)));
                    }
                    Ent::Edge(p, i) => {
                        let [a, b] = ctx.parts[p].solid.edges[i].name.faces;
                        let (a, b) = (face_name(fid, &a), face_name(fid, &b));
                        out.extend(self.all_edges().filter(|x| {
                            let Ent::Edge(p2, j) = *x else { return false };
                            let [c, d] = self.parts[p2].solid.edges[j].name.faces;
                            (c == a && d == b) || (c == b && d == a)
                        }));
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn copies(&self, q: &Value, derived: &[&Value], depth: usize) -> Vec<Ent> {
        use cadrs_kernel::naming::face_hash;
        if let Some(op) = q.get("operationId").and_then(Value::as_str)
            && let Some((fid, lazy)) = self.derived.and_then(|m| m.get(op_feature(op)))
        {
            return match lazy.get() {
                Some(ctx) => self.derived_copies(*fid, &ctx, derived),
                None => Vec::new(),
            };
        }
        let Some(op) = q.get("operationId").and_then(Value::as_str).and_then(|o| self.features.get(op_feature(o))).map(|f| f.0) else {
            return Vec::new();
        };
        let instance: Option<u32> = q
            .get("instanceName")
            .and_then(Value::as_str)
            .and_then(|s| crate::features::cadrs_instance(self.doc, FeatureId(op), s));
        let seeds: Vec<Ent> = derived.iter().flat_map(|d| self.eval_depth(d, depth + 1)).collect();
        // A copy of seed face `s` in instance `k` (any instance when `k` is None).
        let copy_of = |n: &FaceName, s: &FaceName, k: Option<u32>| {
            n.op == op
                && matches!(n.origin, FaceOrigin::Instance { of, face, instance } if of == s.op && face == face_hash(s) && k.is_none_or(|k| k == instance))
        };
        for k in [instance, None] {
            let mut out = Vec::new();
            for seed in &seeds {
                match *seed {
                    Ent::Face(..) => {
                        let s = self.face_name(*seed).expect("a face");
                        out.extend(self.all_faces().filter(|f| self.face_name(*f).is_some_and(|n| copy_of(&n, &s, k))));
                    }
                    Ent::Edge(p, i) => {
                        let [a, b] = self.parts[p].solid.edges[i].name.faces;
                        out.extend(self.all_edges().filter(|e| {
                            let Ent::Edge(p2, j) = *e else { return false };
                            let [c, d] = self.parts[p2].solid.edges[j].name.faces;
                            (copy_of(&c, &a, k) && copy_of(&d, &b, k)) || (copy_of(&c, &b, k) && copy_of(&d, &a, k))
                        }));
                    }
                }
            }
            if !out.is_empty() {
                out.sort();
                out.dedup();
                return out;
            }
        }
        // No copies named that way (the operation kept the seeds' names): the seeds themselves.
        seeds
    }

    /// The face `q` refers to, as a reference.
    pub fn face(&self, q: &Value) -> Option<FaceRef> {
        self.eval(q).into_iter().find_map(|e| match e {
            Ent::Face(p, f) => {
                let part = &self.parts[p];
                Some(FaceRef { part: part.id, face: part.solid.faces[f].name, seed: part.solid.face_point(f).unwrap_or([0.0; 3]) })
            }
            Ent::Edge(..) => None,
        })
    }

    /// The edges `q` refers to, as references.
    /// The open-boundary loop of the surface the operation `op` made that passes through
    /// `point` (world mm): its edges bounding only one face, chained end to end. The loop a
    /// sweep's free profile vertex traces (Onshape's `SWEPT_EDGE` of a vertex).
    pub fn boundary_loop_through(&self, op: uuid::Uuid, point: [f64; 3]) -> Vec<EdgeRef> {
        let d = |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        for (p, part) in self.parts.iter().enumerate() {
            let s = &part.solid;
            let open: Vec<usize> = (0..s.edges.len())
                .filter(|&i| self.open_edge(p, i) && self.edge_faces[p][i].first().is_some_and(|&f| s.faces[f].name.op == op))
                .filter(|&i| s.edges[i].points.len() >= 2)
                .collect();
            let Some(&start) = open.iter().find(|&&i| s.edges[i].distance(point) < 1e-3) else { continue };
            // Chain from the edge through the point both ways.
            let ends = |i: usize| (s.edges[i].points[0], *s.edges[i].points.last().expect("two points"));
            let mut chain = vec![start];
            for dir in [true, false] {
                let (a, b) = ends(start);
                let mut at = if dir { b } else { a };
                loop {
                    let next = open.iter().copied().find(|&j| !chain.contains(&j) && {
                        let (x, y) = ends(j);
                        d(x, at) < 1e-4 || d(y, at) < 1e-4
                    });
                    let Some(j) = next else { break };
                    let (x, y) = ends(j);
                    at = if d(x, at) < 1e-4 { y } else { x };
                    chain.push(j);
                }
            }
            return chain
                .into_iter()
                .map(|i| {
                    let edge = &s.edges[i];
                    EdgeRef { part: part.id, edge: edge.name, seed: edge.points[edge.points.len() / 2] }
                })
                .collect();
        }
        Vec::new()
    }

    /// True if the edge bounds only one face (the open edge of a surface).
    pub fn is_open_edge(&self, r: &EdgeRef) -> bool {
        self.parts.iter().enumerate().any(|(p, part)| {
            part.solid.edges.iter().position(|e| e.name == r.edge).is_some_and(|i| self.open_edge(p, i))
        })
    }

    /// True if edge `i` of part `p` bounds one face only (its name may list that face twice).
    fn open_edge(&self, p: usize, i: usize) -> bool {
        let f = &self.edge_faces[p][i];
        !f.is_empty() && f.iter().all(|x| *x == f[0])
    }

    pub fn edges(&self, q: &Value) -> Vec<EdgeRef> {
        self.eval(q)
            .into_iter()
            .filter_map(|e| match e {
                Ent::Edge(p, i) => {
                    let part = &self.parts[p];
                    let edge = &part.solid.edges[i];
                    Some(EdgeRef { part: part.id, edge: edge.name, seed: edge.points.get(edge.points.len() / 2).copied().unwrap_or([0.0; 3]) })
                }
                Ent::Face(..) => None,
            })
            .collect()
    }
}
