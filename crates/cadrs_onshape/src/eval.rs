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

use cadrs_core::document::{EdgeRef, FaceRef, VertexRef};
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

/// The queries of the three faces a vertex query's vertex is the corner of: a `CAP_VERTEX`'s
/// cap face and the side faces its two sketch curves swept, each wrapped in the copies around it.
fn vertex_faces(q: &Value, depth: usize) -> Option<Vec<Value>> {
    if depth > 8 {
        return None;
    }
    let s = |x: &str| Value::Str(x.into());
    let map = |kv: Vec<(&str, Value)>| Value::Map(kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    match q.get("queryType")?.as_str()? {
        "COPY" => {
            let inner = q.get("derivedFrom")?;
            let inner = inner.items().first().unwrap_or(inner);
            let fields = match q {
                Value::Typed(_, v) => v.as_ref(),
                v => v,
            };
            let Value::Map(fields) = fields else { return None };
            let faces = vertex_faces(inner, depth + 1)?;
            Some(
                faces
                    .into_iter()
                    .map(|f| {
                        let mut m: Vec<(String, Value)> = fields.iter().filter(|(k, _)| k != "entityType" && k != "derivedFrom").cloned().collect();
                        m.push(("entityType".into(), s("FACE")));
                        m.push(("derivedFrom".into(), f));
                        Value::Map(m)
                    })
                    .collect(),
            )
        }
        "CAP_VERTEX" => {
            let op = q.get("operationId")?.clone();
            let mut originals: Vec<Value> = q
                .get("disambiguationData")
                .map(Value::items)
                .unwrap_or_default()
                .iter()
                .filter(|d| d.get("disambiguationType").and_then(Value::as_str) == Some("ORIGINAL_DEPENDENCY"))
                .flat_map(|d| d.get("originals").map(Value::items).unwrap_or_default().iter().cloned())
                .collect();
            if originals.len() != 2 {
                // The INTERSECT of the two curves it was derived from.
                let from = q.get("derivedFrom")?;
                let from = from.items().first().unwrap_or(from);
                originals = from.get("derivedFrom")?.items().to_vec();
            }
            if originals.len() != 2 {
                return None;
            }
            let mut cap = vec![("queryType", s("CAP_FACE")), ("entityType", s("FACE")), ("operationId", op.clone())];
            if let Some(start) = q.get("isStart") {
                cap.push(("isStart", start.clone()));
            }
            let mut out = vec![map(cap)];
            for o in originals {
                let dis = map(vec![("disambiguationType", s("ORIGINAL_DEPENDENCY")), ("originals", Value::Array(vec![o]))]);
                out.push(map(vec![
                    ("queryType", s("SWEPT_FACE")),
                    ("entityType", s("FACE")),
                    ("operationId", op.clone()),
                    ("disambiguationData", Value::Array(vec![dis])),
                ]));
            }
            Some(out)
        }
        _ => None,
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

    /// A face's name and the names of the faces merged into it (the kernel merges neighbouring
    /// faces on one surface, keeping one name; Onshape's queries can name any of them).
    fn face_names(&self, e: Ent) -> Vec<FaceName> {
        let Ent::Face(p, f) = e else { return Vec::new() };
        let s = &self.parts[p].solid;
        let name = s.faces[f].name;
        std::iter::once(name).chain(s.face_aliases.iter().filter(|a| a.face == name).map(|a| a.name)).collect()
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

    /// Whether face `i` of `part` (lying in the plane the line `a`–`b` swept along `n`) covers
    /// part of that sweep: points along the line, a little way off the sketch plane either way
    /// (the face's edge at the sketch plane may have been trimmed since).
    fn covers_sweep(part: &Part, i: usize, a: [f64; 3], b: [f64; 3], n: [f64; 3]) -> bool {
        [0.5, 0.25, 0.75].iter().any(|t| {
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            [1e-3, -1e-3, 1e-2, -1e-2, 0.1, -0.1, 0.5, -0.5, 1.0, -1.0, 1.5, -1.5, 2.0, -2.0, 3.0, -3.0, 5.0, -5.0].iter().any(|s| part.solid.face_contains(i, [p[0] + n[0] * s, p[1] + n[1] * s, p[2] + n[2] * s]))
        })
    }

    /// The faces lying where the sketch lines in `originals` were swept (the side faces an
    /// extrude of them made), found by geometry: the face in the plane through the line and
    /// its sketch's normal, over the line. For a face a later boolean merged into a coplanar one
    /// of another feature, keeping only that one's name (Onshape keeps both histories).
    fn swept_by_geometry(&self, originals: &[Value]) -> Vec<Ent> {
        let unit = |v: [f64; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-300);
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let mut out = Vec::new();
        for o in originals {
            if o.get("queryType").and_then(Value::as_str) != Some("SKETCH_ENTITY") {
                continue;
            }
            let (Some(op), Some(id)) = (o.get("operationId").and_then(Value::as_str), o.get("sketchEntityId").and_then(Value::as_str)) else { continue };
            let Some((map, g)) = self.sketches.get(op_feature(op)) else { continue };
            let Some(cadrs_sketch::CurveKind::Line { a, b }) = map.curves.get(id).and_then(|c| g.curves.get(*c)).map(|c| c.kind) else { continue };
            let Some(frame) = self.doc.iter().find(|f| f.id == map.feature).and_then(|f| f.sketch()?.plane).map(|p| p.frame()) else { continue };
            let (Some(pa), Some(pb)) = (g.points.get(a), g.points.get(b)) else { continue };
            let (wa, wb) = (frame.to_world(pa.pos), frame.to_world(pb.pos));
            let n = unit(frame.normal());
            let m = unit(cross([wb[0] - wa[0], wb[1] - wa[1], wb[2] - wa[2]], n));
            let mid = [(wa[0] + wb[0]) / 2.0, (wa[1] + wb[1]) / 2.0, (wa[2] + wb[2]) / 2.0];
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                eprintln!("  swept line {id}: {wa:?} → {wb:?}, plane normal {m:?}");
            }
            for (p, part) in self.parts.iter().enumerate() {
                for (i, face) in part.solid.faces.iter().enumerate() {
                    let Some(pl) = face.plane else { continue };
                    let fnrm = unit(pl.normal());
                    let off = [mid[0] - pl.origin[0], mid[1] - pl.origin[1], mid[2] - pl.origin[2]];
                    if dot(fnrm, m).abs() < 1.0 - 1e-6 || dot(off, fnrm).abs() > 1e-4 {
                        continue;
                    }
                    // Over the line: just off the sketch plane, to either side.
                    let over = Self::covers_sweep(part, i, wa, wb, n);
                    if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                        eprintln!("  in plane: {:?} over the line {over} (line middle {mid:?}, sweep {n:?})", face.name.origin);
                    }
                    if over && !out.contains(&Ent::Face(p, i)) {
                        out.push(Ent::Face(p, i));
                    }
                }
            }
        }
        out
    }

    /// For a face query that is copies (by mirrors) and merges of a side face swept from a
    /// sketch line: where that face is now, by geometry. The line is reflected by each mirror
    /// along the way, and the face sought lies in the plane through it and its sweep, over it.
    /// Names can't find such a face once its original is gone (a later boolean consumed or
    /// renamed it) while its copy lives on. `None` when the query isn't such a chain.
    fn chain_by_geometry(&self, q: &Value) -> Option<Vec<Ent>> {
        self.chain_geo(q, Vec::new(), false)
    }

    /// [`Self::chain_by_geometry`] from `q` down, with the mirrors met above it (outermost
    /// first: a point on the plane and its unit normal), and whether there was one.
    fn chain_geo(&self, q: &Value, mut mirrors: Vec<([f64; 3], [f64; 3])>, mut copied: bool) -> Option<Vec<Ent>> {
        type V3 = [f64; 3];
        let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let unit = |v: V3| {
            let l = dot(v, v).sqrt().max(1e-300);
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let mut node = q;
        loop {
            let qtype = node.get("queryType").and_then(Value::as_str)?;
            let derived = match node.get("derivedFrom") {
                Some(Value::Array(a)) if a.len() == 1 => Some(&a[0]),
                Some(Value::Array(_)) => None,
                Some(v) => Some(v),
                None => None,
            };
            match qtype {
                "COPY" => {
                    let fid = node.get("operationId").and_then(Value::as_str).and_then(|o| self.features.get(op_feature(o)))?;
                    let f = self.doc.iter().find(|f| f.id == *fid)?;
                    match &f.kind {
                        cadrs_core::document::FeatureKind::Mirror(m) => {
                            mirrors.push(self.mirror_frame(m)?);
                            copied = true;
                        }
                        // A boolean's own copy of a face (an extrude's): where it was.
                        cadrs_core::document::FeatureKind::Extrude(_) | cadrs_core::document::FeatureKind::Boolean(_) => {}
                        _ => return None,
                    }
                    node = derived?;
                }
                "MERGE" => match derived {
                    Some(d) => node = d,
                    // Several faces merged into one: whichever of them leads to a swept line.
                    None => {
                        let Some(Value::Array(all)) = node.get("derivedFrom") else { return None };
                        return all.iter().find_map(|d| self.chain_geo(d, mirrors.clone(), copied).filter(|f| !f.is_empty()));
                    }
                },
                "SWEPT_FACE" => {
                    // Only worth it where a copy is involved (else the names do).
                    if !copied {
                        return None;
                    }
                    let originals = node.get("disambiguationData").map(Value::items).unwrap_or_default().iter().find(|d| d.get("disambiguationType").and_then(Value::as_str) == Some("ORIGINAL_DEPENDENCY")).map(|d| d.get("originals").map(Value::items).unwrap_or_default())?;
                    let o = originals.iter().find(|o| o.get("queryType").and_then(Value::as_str) == Some("SKETCH_ENTITY"))?;
                    let (op, id) = (o.get("operationId").and_then(Value::as_str)?, o.get("sketchEntityId").and_then(Value::as_str)?);
                    let (map, g) = self.sketches.get(op_feature(op))?;
                    let cadrs_sketch::CurveKind::Line { a, b } = g.curves.get(*map.curves.get(id)?)?.kind else { return None };
                    let frame = self.doc.iter().find(|f| f.id == map.feature).and_then(|f| f.sketch()?.plane)?.frame();
                    let (mut wa, mut wb) = (frame.to_world(g.points.get(a)?.pos), frame.to_world(g.points.get(b)?.pos));
                    let mut n = unit(frame.normal());
                    // Innermost mirror first.
                    for (o, m) in mirrors.iter().rev() {
                        let reflect = |p: V3| {
                            let d = 2.0 * dot(sub(p, *o), *m);
                            [p[0] - d * m[0], p[1] - d * m[1], p[2] - d * m[2]]
                        };
                        let dn = 2.0 * dot(n, *m);
                        n = [n[0] - dn * m[0], n[1] - dn * m[1], n[2] - dn * m[2]];
                        (wa, wb) = (reflect(wa), reflect(wb));
                    }
                    let plane_n = unit(cross(sub(wb, wa), n));
                    let mid = [(wa[0] + wb[0]) / 2.0, (wa[1] + wb[1]) / 2.0, (wa[2] + wb[2]) / 2.0];
                    let mut out = Vec::new();
                    // The faces in that plane, and how near each comes to the line's middle.
                    let mut near: Vec<(f64, Ent)> = Vec::new();
                    for (p, part) in self.parts.iter().enumerate() {
                        for (i, face) in part.solid.faces.iter().enumerate() {
                            let Some(pl) = face.plane else { continue };
                            let fnrm = unit(pl.normal());
                            if dot(fnrm, plane_n).abs() < 1.0 - 1e-6 || dot(sub(mid, pl.origin), fnrm).abs() > 1e-4 {
                                continue;
                            }
                            let over = Self::covers_sweep(part, i, wa, wb, n);
                            if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                                eprintln!("  chain in plane: {:?} over {over}", self.face_names(Ent::Face(p, i)).iter().map(|n| n.origin).collect::<Vec<_>>());
                            }
                            if over {
                                out.push(Ent::Face(p, i));
                            } else {
                                let d = face.loops.iter().flatten().map(|q| dot(sub(*q, mid), sub(*q, mid)).sqrt()).fold(f64::MAX, f64::min);
                                near.push((d, Ent::Face(p, i)));
                            }
                        }
                    }
                    // None over the line (the face it was merged with reshaped the part there):
                    // the face in that plane nearest it.
                    if out.is_empty()
                        && let Some((_, e)) = near.iter().min_by(|a, b| a.0.total_cmp(&b.0))
                    {
                        out.push(*e);
                    }
                    if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                        eprintln!("CHAIN {id} through {} mirror(s): line {wa:?} → {wb:?}, found {}", mirrors.len(), out.len());
                    }
                    return Some(out);
                }
                _ => return None,
            }
        }
    }

    /// A mirror's plane, as a point on it and its unit normal: a default or feature plane, or a
    /// part face (found by its name now).
    fn mirror_frame(&self, m: &cadrs_core::pattern::MirrorFeature) -> Option<([f64; 3], [f64; 3])> {
        let frame = match m.plane? {
            cadrs_core::pattern::MirrorPlane::Plane(p) => p.frame(),
            cadrs_core::pattern::MirrorPlane::Face(r) => match self.all_faces().find(|e| self.face_names(*e).contains(&r.face)) {
                Some(Ent::Face(p, i)) => self.parts[p].solid.faces[i].plane?,
                // Gone since: where it was when the mirror was made.
                _ => {
                    let at = self.doc.iter().position(|f| matches!(&f.kind, cadrs_core::document::FeatureKind::Mirror(x) if x == m))?;
                    let b = cadrs_core::rebuild::build(&self.doc[..at]);
                    let found = b.parts.iter().find_map(|p| {
                        let name = p.solid.canonical_face(&r.face);
                        p.solid.faces.iter().find(|f| f.name == name)
                    });
                    found?.plane?
                }
            },
            cadrs_core::pattern::MirrorPlane::Connector(_) => return None,
        };
        let n = frame.normal();
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-300);
        Some((frame.origin, [n[0] / l, n[1] / l, n[2] / l]))
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
        // A mirrored copy of a side face swept from a sketch line: by geometry, when it finds it.
        if entity_type == "FACE"
            && matches!(qtype, "COPY" | "MERGE")
            && let Some(found) = self.chain_by_geometry(q)
            && !found.is_empty()
        {
            return found;
        }
        let mut cands: Vec<Ent> = match qtype {
            "COPY" => self.copies(q, &derived(), depth),
            "MERGE" | "SPLIT" => {
                // A merge is the one entity all its histories describe: what they have in common,
                // else (cadrs kept only one of the names) what the most precise of them describes
                // (a side face found by the curve that swept it, not every copy a pattern made).
                let sets: Vec<Vec<Ent>> = derived().into_iter().map(|d| self.eval_depth(d, depth + 1)).collect();
                if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                    for (i, s) in sets.iter().enumerate() {
                        let names: Vec<String> = s.iter().take(6).map(|e| format!("{:?}", self.face_name(*e).map(|n| n.origin))).collect();
                        eprintln!("MERGE depth {depth} branch {i}: {} candidate(s) {}", s.len(), names.join(" ; "));
                    }
                }
                let common: Vec<Ent> = match sets.split_first() {
                    Some((first, rest)) if qtype == "MERGE" => first.iter().copied().filter(|e| rest.iter().all(|r| r.contains(e))).collect(),
                    _ => Vec::new(),
                };
                let none_found = sets.iter().all(Vec::is_empty);
                let mut v: Vec<Ent> = if !common.is_empty() {
                    common
                } else if qtype == "MERGE" {
                    sets.into_iter().filter(|s| !s.is_empty()).min_by_key(Vec::len).unwrap_or_default()
                } else {
                    sets.into_iter().flatten().collect()
                };
                // None of its histories found (a face whose name a boolean merged away): the
                // faces of the operation that merged it, narrowed by its own disambiguation
                // below (cadrs named the merged face after the operation's piece of it).
                if none_found
                    && entity_type == "FACE"
                    && let Some(f) = q.get("operationId").and_then(Value::as_str).and_then(|o| self.features.get(op_feature(o)))
                {
                    let op = f.0;
                    v = self.all_faces().filter(|e| self.face_names(*e).iter().any(|n| n.op == op)).collect();
                }
                v.sort();
                v.dedup();
                v
            }
            "INTERSECT" => {
                let sides: Vec<HashSet<Ent>> = derived().into_iter().map(|d| self.eval_depth(d, depth + 1).into_iter().collect()).collect();
                if sides.len() < 2 {
                    return Vec::new();
                }
                if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                    for (i, s) in sides.iter().enumerate() {
                        eprintln!("INTERSECT side {i}: {:?}", s.iter().map(|e| self.face_names(*e).iter().map(|n| n.origin).collect::<Vec<_>>()).collect::<Vec<_>>());
                    }
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
                // An imported file's entity by Onshape's own tag for it, which cadrs can't
                // read: only when the import made just the one.
                if qtype == "IMPORT" {
                    let all: Vec<Ent> = match entity_type {
                        "FACE" => self.all_faces().filter(|e| self.face_names(*e).iter().any(|n| n.op == op)).collect(),
                        _ => Vec::new(),
                    };
                    return if all.len() == 1 { all } else { Vec::new() };
                }
                match entity_type {
                    "FACE" => self.all_faces().filter(|e| self.face_names(*e).iter().any(|n| n.op == op)).collect(),
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
            "SWEPT_FACE" => cands = narrow(cands, &|e| self.face_names(e).iter().any(|n| Some(n.op) == op_id && cap_end(n).is_none())),
            "CAP_EDGE" => {
                let end = is_start.map(|s| !s);
                cands = narrow(cands, &|e| {
                    self.adjacent(e).iter().flat_map(|f| self.face_names(*f)).any(|n| Some(n.op) == op_id && cap_end(&n).is_some_and(|x| end.is_none_or(|end| end == x)))
                });
            }
            "SWEPT_EDGE" => {
                cands = narrow(cands, &|e| {
                    // (A face's merged-away names count: the side a boolean merged into another.)
                    let faces = self.adjacent(e);
                    faces.len() == 2 && faces.iter().all(|f| self.face_names(*f).iter().any(|n| Some(n.op) == op_id && cap_end(n).is_none()))
                });
            }
            _ => {}
        }

        // Disambiguation.
        for d in q.get("disambiguationData").map(Value::items).unwrap_or_default() {
            let kind = d.get("disambiguationType").and_then(Value::as_str).unwrap_or_default();
            // One candidate left is the answer, unless the sketch curve it was swept from says
            // otherwise (the face it means was merged into another feature's and renamed).
            if cands.is_empty() || (cands.len() == 1 && (kind != "ORIGINAL_DEPENDENCY" || qtype != "SWEPT_FACE")) {
                break;
            }
            match kind {
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
                    let by_name = |e: Ent| {
                        self.face_names(e).iter().any(|n| match n.origin {
                            FaceOrigin::Side { curve, .. } => curves.contains(&curve),
                            FaceOrigin::Cap { region: r, .. } => region.is_some_and(|x| x == r),
                            FaceOrigin::FromEdge { edge } => from_curves.contains(&edge),
                            _ => false,
                        })
                    };
                    // Curves of a sketch the extrude didn't sweep (another sketch's edges imprinted
                    // on its profile's plane): their ids are that sketch's, so a face named after
                    // the same id is another curve's. Found by where the curve swept to.
                    let foreign = op_id.and_then(|op| self.doc.iter().find(|f| f.id.0 == op)).and_then(|f| match &f.kind {
                        cadrs_core::document::FeatureKind::Extrude(x) => Some(x.sketches()),
                        _ => None,
                    }).is_some_and(|swept| {
                        !swept.is_empty()
                            && originals.iter().filter_map(|o| o.get("operationId").and_then(Value::as_str)).filter_map(|o| self.sketches.get(op_feature(o))).all(|(m, _)| !swept.contains(&m.feature))
                    });
                    // A side face no face is named after any more: where the curve swept to.
                    if qtype == "SWEPT_FACE" && (foreign || !cands.iter().any(|e| matches!(e, Ent::Face(..)) && by_name(*e))) {
                        let found = self.swept_by_geometry(originals);
                        if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() {
                            eprintln!("SWEPT by geometry: {} candidate(s) had no matching name; found {} face(s) {:?}", cands.len(), found.len(), found.iter().map(|e| self.face_name(*e).map(|n| n.origin)).collect::<Vec<_>>());
                        }
                        if !found.is_empty() {
                            cands = found;
                            continue;
                        }
                        // Not there by name nor by place: say so (a merge above it then looks
                        // at its own operation's faces) rather than guess another side face.
                        return Vec::new();
                    }
                    // Curve ids are a sketch's own: an edge along side faces the query's own
                    // operation swept from them, when there is one (not another sketch's curve
                    // with the same id); the edge whose faces were swept from the most of them
                    // (an edge where two of the curves meet, not one along just one of them).
                    let coverage = |e: Ent| {
                        let names: Vec<FaceName> = self.adjacent(e).iter().flat_map(|f| self.face_names(*f)).filter(|n| Some(n.op) == op_id).collect();
                        curves.iter().filter(|c| names.iter().any(|n| matches!(n.origin, FaceOrigin::Side { curve, .. } if curve == **c))).count()
                    };
                    let best = cands.iter().copied().filter(|e| matches!(e, Ent::Edge(..))).map(coverage).max().unwrap_or(0);
                    let own: Vec<Ent> = if best == 0 {
                        Vec::new()
                    } else {
                        cands.iter().copied().filter(|e| matches!(e, Ent::Edge(..)) && coverage(*e) == best).collect()
                    };
                    if !own.is_empty() {
                        cands = own;
                        continue;
                    }
                    cands = narrow(cands, &|e| match e {
                        Ent::Face(..) => by_name(e),
                        // An edge made from these curves: a side face of it is made from one.
                        Ent::Edge(..) => self.adjacent(e).iter().flat_map(|f| self.face_names(*f)).any(|n| match n.origin {
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
        if std::env::var_os("CADRS_ONSHAPE_DEBUG_MERGE").is_some() && matches!(qtype, "MERGE" | "COPY") {
            let names: Vec<String> = cands.iter().take(6).map(|e| format!("{:?}", self.face_names(*e).iter().map(|n| n.origin).collect::<Vec<_>>())).collect();
            eprintln!("{qtype} depth {depth} result: {} {}", cands.len(), names.join(" ; "));
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
                        // By any of their names (a merged face answers to its pieces' names).
                        let seed_names = self.face_names(*seed);
                        out.extend(self.all_faces().filter(|f| self.face_names(*f).iter().any(|n| seed_names.iter().any(|s| copy_of(n, s, k)))));
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

    /// The vertex `q` refers to: a cap corner of an extrude (`CAP_VERTEX`, where the cap meets
    /// the side faces its two sketch curves swept), or a copy of one (a derived part's), found
    /// as the vertex of the three faces the same queries name.
    pub fn vertex(&self, q: &Value) -> Option<VertexRef> {
        let mut sets: Vec<HashSet<Ent>> = vertex_faces(q, 0)?.iter().map(|f| self.eval(f).into_iter().collect()).collect();
        let is_start = q.get("isStart").and_then(Value::as_bool);
        // The cap face of an extrude here (not a copy's): only by its name, since a cap that
        // merged into the face the extrude started from leaves none (its corner is still there).
        let op_id = q.get("operationId").and_then(Value::as_str).and_then(|o| self.features.get(op_feature(o))).map(|f| f.0);
        let direct = q.get("queryType").and_then(Value::as_str) == Some("CAP_VERTEX");
        if direct && let Some(cap) = sets.first_mut() {
            cap.retain(|e| {
                self.face_names(*e)
                    .iter()
                    .any(|n| Some(n.op) == op_id && matches!(n.origin, FaceOrigin::Cap { end, .. } if is_start.is_none_or(|s| s != end)))
            });
        }
        let on = |p: usize, v: &cadrs_core::solid::SolidVertex, s: &HashSet<Ent>| {
            s.iter().any(|e| matches!(*e, Ent::Face(fp, _) if fp == p) && self.face_names(*e).iter().any(|n| v.name.faces.contains(n)))
        };
        let at = |p: usize, v: &cadrs_core::solid::SolidVertex| VertexRef { part: self.parts[p].id, vertex: v.name, point: v.point };
        if sets.iter().all(|s| !s.is_empty())
            && let Some(found) = self
                .parts
                .iter()
                .enumerate()
                .find_map(|(p, part)| part.solid.vertices.iter().find(|v| sets.iter().all(|s| on(p, v, s))).map(|v| at(p, v)))
        {
            return Some(found);
        }
        if !direct || sets.len() != 3 || sets[1..].iter().any(HashSet::is_empty) {
            return None;
        }
        // No cap by name: the corner of the two side faces nearest the sketch plane (the start
        // cap) or farthest from it (the end cap).
        let sketch_op = q
            .get("disambiguationData")
            .map(Value::items)
            .unwrap_or_default()
            .iter()
            .flat_map(|d| d.get("originals").map(Value::items).unwrap_or_default())
            .find_map(|o| o.get("operationId").and_then(Value::as_str))?;
        let (map, _) = self.sketches.get(op_feature(sketch_op))?;
        let frame = self.doc.iter().find(|f| f.id == map.feature).and_then(|f| f.sketch()?.plane)?.frame();
        let (n, o) = (frame.normal(), frame.origin);
        let dist = |v: &cadrs_core::solid::SolidVertex| ((v.point[0] - o[0]) * n[0] + (v.point[1] - o[1]) * n[1] + (v.point[2] - o[2]) * n[2]).abs();
        let sides = (&sets[1], &sets[2]);
        let corners = self
            .parts
            .iter()
            .enumerate()
            .flat_map(|(p, part)| part.solid.vertices.iter().filter(move |v| on(p, v, sides.0) && on(p, v, sides.1)).map(move |v| (p, v)));
        let pick = if is_start == Some(false) {
            corners.max_by(|a, b| dist(a.1).total_cmp(&dist(b.1)))
        } else {
            corners.min_by(|a, b| dist(a.1).total_cmp(&dist(b.1)))
        };
        pick.map(|(p, v)| at(p, v))
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
