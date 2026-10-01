//! One scraped Onshape document to one cadrs document.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::command::CommandError;
use cadrs_core::appearance::Appearance;
use cadrs_core::commands::{AddElement, AddExtrude, NewElementKind, RenameFeature, RenamePart, SetExtrude, SetPartAppearance, SetPartMaterial};
use cadrs_core::document::{BodyType, BooleanOp, Document, EndCondition, EndType, ExtrudeFeature, Offset, ThinWall, UpTo};
use cadrs_core::ids::{DocumentId, ElementId, FeatureId};
use cadrs_core::library::DocumentMeta;
use cadrs_core::parts::Part;
use cadrs_sketch::units::{Quantity, eval};
use cadrs_sketch::{PlaneRef, Sketch};
use serde_json::Value;

use crate::ids::stable_u128;
use crate::raw::{RawDocument, RawElement, read_json};
use crate::refs::{self, Pick};
use crate::report::{DocumentReport, ElementReport, FeatureReport, Outcome, PartCheck};
use crate::sketch::{self, SketchMap, param};
use crate::studio::{DocStudio, Studio};

/// The imported document, its metadata and what happened.
pub struct Imported {
    pub doc: Document,
    pub meta: DocumentMeta,
    pub report: DocumentReport,
}

/// How to run an import.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Onshape feature ids to leave out (features whose rebuild hung in an earlier attempt).
    pub skip: std::collections::HashSet<String>,
    /// Where to write the id of the feature being imported, so a supervisor that has to kill
    /// a hung import knows which feature to skip next time.
    pub progress: Option<std::path::PathBuf>,
    /// Import only this Part Studio (Onshape element id) and those it derives from in the same
    /// document (for another document's Derived feature, which only needs its source).
    pub only: Option<String>,
}

/// The skip list and progress file of the import a supervisor runs, for the imports it makes
/// of other documents along the way ([`Options::nested`]).
static SUPERVISED: std::sync::Mutex<Option<(std::collections::HashSet<String>, Option<std::path::PathBuf>)>> = std::sync::Mutex::new(None);

impl Options {
    /// The options of an import of another document made during this one (a Derived feature's
    /// source, a linked instance's document): under the same supervision, so each of its
    /// features has the time limit to itself (it isn't counted against the feature that needed
    /// it) and a feature of it that hung is skipped next time.
    pub fn nested(only: Option<String>) -> Self {
        let (skip, progress) = SUPERVISED.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
        Options { skip, progress, only }
    }
}

/// Runs a nested import ([`Options::nested`]); afterwards the progress file names the feature
/// that asked for it again (a hang after it must skip that one, not the nested import's last).
pub(crate) fn nested_import<T>(f: impl FnOnce() -> T) -> T {
    let path = SUPERVISED.lock().ok().and_then(|s| s.as_ref().and_then(|(_, p)| p.clone()));
    let before = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let out = f();
    if let (Some(p), Some(b)) = (path, before) {
        std::fs::write(p, b).ok();
    }
    out
}

/// Imports `raw` as a new cadrs document (with a stable id, so a later import replaces it).
pub fn import_document(raw: &RawDocument, user: &str, options: &Options) -> Imported {
    if options.progress.is_some()
        && let Ok(mut s) = SUPERVISED.lock()
        && s.is_none()
    {
        *s = Some((options.skip.clone(), options.progress.clone()));
    }
    let json = raw.json().unwrap_or(Value::Null);
    let mut doc = Document::empty(raw.name.clone());
    doc.id = DocumentId::from_u128(stable_u128(&[&raw.id]));
    let mut report = DocumentReport { name: raw.name.clone(), onshape_id: raw.id.clone(), ..Default::default() };
    let now = timestamp(json["modifiedAt"].as_str()).unwrap_or(0);
    let mut meta = DocumentMeta::new(user, timestamp(json["createdAt"].as_str()).unwrap_or(now));
    meta.modified = now;

    let mut s = DocStudio::new(doc);
    let elements = raw.elements();
    let vars = variables(&elements);
    // Tabs first, in Onshape's order; then the Part Studios, those others derive from first.
    let mut reports: Vec<Option<ElementReport>> = Vec::new();
    let mut studios = Vec::new();
    for (i, el) in elements.iter().enumerate() {
        let mut er = ElementReport { name: el.name.clone(), kind: el.kind.clone(), ..Default::default() };
        let id = ElementId::from_u128(stable_u128(&[&raw.id, &el.id]));
        match el.kind.as_str() {
            "PARTSTUDIO" => {
                if let Err(e) = s.run(&AddElement { id, kind: NewElementKind::PartStudio, name: Some(el.name.clone()), after: None }) {
                    er.notes.push(format!("cannot add the tab: {e}"));
                } else {
                    er.imported = true;
                    studios.push((i, id));
                }
            }
            "ASSEMBLY" => {
                if s.run(&AddElement { id, kind: NewElementKind::Assembly, name: Some(el.name.clone()), after: None }).is_ok() {
                    er.imported = true;
                }
            }
            "BILLOFMATERIALS" | "VARIABLESTUDIO" => {
                reports.push(None);
                continue;
            }
            _ => er.notes.push("no cadrs equivalent yet".into()),
        }
        reports.push(Some(er));
    }
    let needed = options.only.as_ref().map(|e| needed_studios(&elements, e));
    for (i, id) in derive_order(&elements, &studios) {
        let el = &elements[i];
        if needed.as_ref().is_some_and(|n| !n.contains(&el.id)) {
            continue;
        }
        let mut er = reports[i].take().unwrap_or_default();
        PartStudio::new(&mut s, id, &raw.id, el, vars.clone(), &mut er, options).run();
        reports[i] = Some(er);
    }
    // Assemblies once their Part Studios are there (not for a lone studio another document needs).
    if options.only.is_none() {
        crate::assembly::import_assemblies(&mut s, raw, &elements, &mut reports);
    }
    report.elements.extend(reports.into_iter().flatten());
    if s.doc.elements.is_empty() {
        s.run(&AddElement { id: ElementId::from_u128(stable_u128(&[&raw.id, "empty"])), kind: NewElementKind::PartStudio, name: Some("Part Studio 1".into()), after: None })
            .ok();
    }
    report.cadrs_id = Some(s.doc.id.0.to_string());
    Imported { doc: s.doc, meta, report }
}

/// The Part Studios in an order where a studio another one derives from (in this document)
/// comes first; otherwise tab order.
/// Part Studio `el` and the studios of this document it derives from, directly or not.
fn needed_studios(elements: &[RawElement], el: &str) -> Vec<String> {
    let mut out = vec![el.to_string()];
    let mut i = 0;
    while i < out.len() {
        if let Some(e) = elements.iter().find(|e| e.id == out[i]) {
            for src in same_doc_sources(e) {
                if !out.contains(&src) {
                    out.push(src);
                }
            }
        }
        i += 1;
    }
    out
}

/// The Part Studios of this document that `el` derives from.
fn same_doc_sources(el: &RawElement) -> Vec<String> {
    el.features()
        .and_then(|f| f["features"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|f| f["featureType"].as_str() == Some("importDerived"))
        .filter_map(|f| {
            let ns = crate::sketch::param(f, "partStudio")?["namespace"].as_str()?;
            match crate::features::namespace_ref(ns) {
                (None, Some(e)) => Some(e),
                _ => None,
            }
        })
        .collect()
}

fn derive_order(elements: &[RawElement], studios: &[(usize, ElementId)]) -> Vec<(usize, ElementId)> {
    let sources = |el: &RawElement| -> Vec<String> {
        el.features()
            .and_then(|f| f["features"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|f| f["featureType"].as_str() == Some("importDerived"))
            .filter_map(|f| {
                let ns = crate::sketch::param(f, "partStudio")?["namespace"].as_str()?;
                match crate::features::namespace_ref(ns) {
                    (None, Some(e)) => Some(e),
                    _ => None,
                }
            })
            .collect()
    };
    let mut left: Vec<(usize, ElementId)> = studios.to_vec();
    let mut out = Vec::new();
    while !left.is_empty() {
        let ready = left.iter().position(|(i, _)| {
            sources(&elements[*i]).iter().all(|src| !left.iter().any(|(j, _)| elements[*j].id == *src))
        });
        // A cycle: take them in tab order (the Derived features will report it).
        out.push(left.remove(ready.unwrap_or(0)));
    }
    out
}

/// The imported Part Studio `element` of scraped document `doc` (under `root`): from this run,
/// else imported now (only that studio and its same-document sources, not saved).
pub(crate) fn load_source(root: &std::path::Path, doc: &str, element: &str) -> Option<Arc<crate::eval::SourceCtx>> {
    if let Some(s) = crate::eval::source(doc, element) {
        return Some(s);
    }
    static BUSY: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    if BUSY.lock().ok()?.contains(&doc.to_string()) {
        return None;
    }
    let raw = crate::raw::documents(root).into_iter().find(|d| d.id == doc)?;
    BUSY.lock().ok()?.push(doc.to_string());
    let _ = nested_import(|| import_document(&raw, "import", &Options::nested(Some(element.to_string()))));
    BUSY.lock().ok()?.retain(|d| d != doc);
    crate::eval::source(doc, element)
}

/// Seconds since the epoch of an Onshape timestamp (`2026-09-14T08:31:12.345Z`).
fn timestamp(s: Option<&str>) -> Option<i64> {
    let s = s?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.trim_end_matches('Z').split(':');
    let (hh, mm) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let ss = t.next()?.split('.').next()?.parse::<i64>().ok()?;
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// A variable: its kind and expression.
#[derive(Debug, Clone)]
pub struct Var {
    kind: String,
    expr: String,
}

/// The document's Variable Studio variables.
fn variables(elements: &[RawElement]) -> HashMap<String, Var> {
    let mut out = HashMap::new();
    for el in elements.iter().filter(|e| e.kind == "VARIABLESTUDIO") {
        let Some(v) = read_json(&el.dir.join("variables.json")) else { continue };
        for set in v.as_array().into_iter().flatten() {
            for var in set["variables"].as_array().into_iter().flatten() {
                if let (Some(name), Some(expr)) = (var["name"].as_str(), var["expression"].as_str()) {
                    out.insert(name.to_string(), Var { kind: var["type"].as_str().unwrap_or("ANY").to_string(), expr: expr.to_string() });
                }
            }
        }
    }
    out
}

/// Evaluates an Onshape expression: `#name` variables are replaced by their values (with
/// units), then cadrs's evaluator reads it. Lengths in mm, angles in degrees.
pub fn eval_expr(expr: &str, q: Quantity, vars: &HashMap<String, Var>) -> Option<f64> {
    let table: HashMap<String, String> = vars.iter().map(|(k, v)| (k.clone(), v.expr.clone())).collect();
    if let Ok(x) = crate::expr::eval(expr, &table) {
        let plain = x.len == 0 && !x.angle;
        return match q {
            Quantity::Length if x.len == 1 || plain => Some(x.v),
            Quantity::Angle if x.angle => Some(x.v.to_degrees()),
            Quantity::Angle if plain => Some(x.v),
            Quantity::Count if plain => Some(x.v),
            _ => None,
        };
    }
    eval(&normalize(&substitute(expr, vars, 0)?), q).ok()
}

/// Onshape's unit spellings in cadrs's: units are values there (`9.6*mm`, `2 * inch`).
fn normalize(expr: &str) -> String {
    const UNITS: [(&str, &str); 14] = [
        ("millimeter", "mm"),
        ("centimeter", "cm"),
        ("meter", "m"),
        ("inch", "in"),
        ("foot", "ft"),
        ("feet", "ft"),
        ("degree", "deg"),
        ("radian", "rad"),
        ("mm", "mm"),
        ("cm", "cm"),
        ("in", "in"),
        ("ft", "ft"),
        ("deg", "deg"),
        ("rad", "rad"),
    ];
    let mut out = String::with_capacity(expr.len());
    let b = expr.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // `*` followed by a unit word: drop the `*`.
        if b[i] == b'*' {
            let rest = expr[i + 1..].trim_start();
            let word: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
            let unit = UNITS.iter().find(|(from, _)| word == *from || word == format!("{from}s")).map(|(_, to)| *to).or((word == "m").then_some("m"));
            if let Some(u) = unit {
                out.push(' ');
                out.push_str(u);
                i = expr.len() - rest.len() + word.len();
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn substitute(expr: &str, vars: &HashMap<String, Var>, depth: usize) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let mut out = String::new();
    let mut chars = expr.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '#' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_alphanumeric() || n == '_' {
                name.push(n);
                chars.next();
            } else {
                break;
            }
        }
        let var = vars.get(&name)?;
        let inner = substitute(&var.expr, vars, depth + 1)?;
        let text = match var.kind.as_str() {
            "LENGTH" => format!("({} mm)", eval(&inner, Quantity::Length).ok()?),
            "ANGLE" => format!("({} deg)", eval(&inner, Quantity::Angle).ok()?),
            "NUMBER" => format!("({})", eval(&inner, Quantity::Count).or_else(|_| eval(&inner, Quantity::Angle)).ok()?),
            // Anything: a length if it reads as one with units, else a plain number.
            _ => match eval(&inner, Quantity::Length) {
                Ok(v) if has_unit(&inner) => format!("({v} mm)"),
                Ok(v) => format!("({v})"),
                Err(_) => format!("({} deg)", eval(&inner, Quantity::Angle).ok()?),
            },
        };
        out.push_str(&text);
    }
    Some(out)
}

fn has_unit(s: &str) -> bool {
    ["mm", "cm", "m", "in", "ft", "\""].iter().any(|u| s.contains(u))
}

/// A Part Studio being imported.
pub(crate) struct PartStudio<'a> {
    pub(crate) s: &'a mut DocStudio,
    pub(crate) el: ElementId,
    doc_id: &'a str,
    raw: &'a RawElement,
    pub(crate) vars: HashMap<String, Var>,
    report: &'a mut ElementReport,
    /// Onshape feature id → cadrs feature.
    pub(crate) features: HashMap<String, FeatureId>,
    /// Onshape sketch feature id → its id maps and final geometry.
    pub(crate) sketches: HashMap<String, (SketchMap, Sketch)>,
    /// The model as of the last rebuild, and how many features it covers.
    /// (The features it was built from: an edit that keeps their count must rebuild too.)
    built: Option<(Vec<cadrs_core::document::Feature>, Arc<cadrs_core::rebuild::Build>)>,
    options: &'a Options,
    /// Its Derived features' sources (to resolve queries on derived parts).
    pub(crate) derived: crate::eval::DerivedSources,
}

impl<'a> PartStudio<'a> {
    fn new(
        s: &'a mut DocStudio,
        el: ElementId,
        doc_id: &'a str,
        raw: &'a RawElement,
        vars: HashMap<String, Var>,
        report: &'a mut ElementReport,
        options: &'a Options,
    ) -> Self {
        Self { s, el, doc_id, raw, vars, report, features: HashMap::new(), sketches: HashMap::new(), built: None, options, derived: HashMap::new() }
    }

    /// A query model of `parts` with this studio's features, sketches and derived sources.
    pub(crate) fn model<'m>(&'m self, parts: &'m [Part]) -> crate::eval::Model<'m> {
        let doc = self.s.doc.element(self.el).map(|e| e.features()).unwrap_or(&[]);
        crate::eval::Model::new(parts, &self.features, &self.sketches).with_derived(&self.derived).with_doc(doc)
    }

    /// The folder of all scraped documents.
    pub(crate) fn raw_root(&self) -> Option<std::path::PathBuf> {
        Some(self.raw.dir.parent()?.parent()?.to_path_buf())
    }

    /// This document's Onshape id.
    pub(crate) fn doc_id(&self) -> &str {
        self.doc_id
    }

    /// A Blob tab of this document: its file name and bytes.
    pub(crate) fn blob(&self, element: &str) -> Option<(String, Vec<u8>)> {
        let doc_dir = self.raw.dir.parent()?;
        let elements = read_json(&doc_dir.join("elements.json"))?;
        let name = elements.as_array()?.iter().find(|e| e["id"].as_str() == Some(element))?["name"].as_str()?.to_string();
        let bytes = std::fs::read(doc_dir.join(element).join("blob.bin")).ok()?;
        Some((name, bytes))
    }

    pub(crate) fn feature_id(&self, onshape: &str) -> FeatureId {
        FeatureId::from_u128(stable_u128(&[self.doc_id, &self.raw.id, onshape]))
    }

    /// The parts as the features so far build them.
    pub(crate) fn parts(&mut self) -> Vec<Part> {
        let feats = self.s.doc.element(self.el).map(|e| e.features().to_vec()).unwrap_or_default();
        let fresh = matches!(&self.built, Some((f, _)) if *f == feats);
        if !fresh {
            // Debugging a rebuild that hangs: the features about to be built, as RON.
            if let Some(path) = std::env::var_os("CADRS_ONSHAPE_DUMP")
                && let Ok(text) = ron::ser::to_string(&feats)
            {
                std::fs::write(path, text).ok();
            }
            let started = std::time::Instant::now();
            let build = cadrs_core::rebuild::build(&feats);
            if std::env::var_os("CADRS_ONSHAPE_TRACE").is_some() {
                eprintln!("    rebuild of {} features: {:.3} s", feats.len(), started.elapsed().as_secs_f64());
            }
            self.built = Some((feats, build));
        }
        self.built.as_ref().map(|(_, b)| b.parts.clone()).unwrap_or_default()
    }

    /// A query in brief, for debugging: `TYPE:QUERY(op feature, imported?)` with what it was
    /// derived from.
    pub(crate) fn describe(&self, q: &crate::query::Value) -> String {
        use crate::query::Value as Q;
        let t = |k: &str| q.get(k).and_then(Q::as_str).unwrap_or("?").to_string();
        let op = q.get("operationId").and_then(Q::as_str).unwrap_or("");
        let fid = crate::refs::op_feature(op);
        let known = if op.is_empty() {
            String::new()
        } else if self.sketches.contains_key(fid) {
            "(sketch)".into()
        } else if self.features.contains_key(fid) {
            "(imported)".into()
        } else {
            "(NOT imported)".into()
        };
        let mut s = format!("{}:{}{}", t("entityType"), t("queryType"), known);
        let derived: Vec<&Q> = match q.get("derivedFrom") {
            Some(Q::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
            None => Vec::new(),
        };
        if !derived.is_empty() {
            s += &format!("<{}>", derived.iter().map(|d| self.describe(d)).collect::<Vec<_>>().join(","));
        }
        s
    }

    /// What a region query names: its sketch (Onshape and cadrs ids), the cadrs sketch, and
    /// the curves around the region: the sketch entities it mentions and, where it names model
    /// edges next to the region (a sketch curve crossing the face it is drawn on is cut by the
    /// face's edges, which cadrs imprints on the sketch), the imprinted edges they match.
    #[allow(clippy::type_complexity)]
    fn region_curves(
        &mut self,
        q: &crate::query::Value,
    ) -> Option<(String, FeatureId, SketchMap, cadrs_core::document::SketchFeature, std::collections::BTreeSet<cadrs_sketch::CurveId>)> {
        use crate::query::Value as Q;
        let op = q.get("operationId").and_then(Q::as_str)?;
        let sk = refs::op_feature(op).to_string();
        let (map, _) = self.sketches.get(&sk)?;
        let (sid, map) = (map.feature, map.clone());
        let feature = self.s.doc.element(self.el)?.feature(sid)?.sketch()?.clone();
        let g = &feature.geometry;
        let mut ids = std::collections::BTreeSet::new();
        refs::sketch_entities(q, &sk, &mut ids);
        let mut curves: std::collections::BTreeSet<cadrs_sketch::CurveId> = ids.iter().filter_map(|i| map.curves.get(i).copied()).collect();
        if !g.imprint.is_empty()
            && let Some(plane) = feature.plane
        {
            let frame = plane.frame();
            let n = cross(frame.u, frame.v);
            let mut edges = Vec::new();
            model_edges(q, &sk, &mut edges);
            let parts = self.parts();
            let model = self.model(&parts);
            for e in edges {
                for ent in model.eval(e) {
                    let crate::eval::Ent::Edge(pi, ei) = ent else { continue };
                    let pts: Vec<cadrs_sketch::Vec2> = parts[pi].solid.edges[ei]
                        .points
                        .iter()
                        .filter_map(|p| {
                            let d = [p[0] - frame.origin[0], p[1] - frame.origin[1], p[2] - frame.origin[2]];
                            (dot(d, n).abs() < 1e-3).then(|| cadrs_sketch::Vec2::new(dot(d, frame.u), dot(d, frame.v)))
                        })
                        .collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    for im in &g.imprint {
                        if pts.iter().all(|p| imprint_distance(&im.shape, *p) < 1e-3) {
                            curves.insert(im.id);
                        }
                    }
                }
            }
        }
        Some((sk, sid, map, feature, curves))
    }

    /// A region query that names no drawn sketch curve: a region bounded only by the edges of
    /// the face(s) the sketch lies on, which Onshape offers but cadrs doesn't (a face's own
    /// outline is not a sketch region there). It is that face: the coplanar face whose
    /// outline has the imprinted edges the query names (or the sketch's own face).
    pub(crate) fn face_region(&mut self, q: &crate::query::Value, notes: &mut Vec<String>) -> Option<FaceOrRegion> {
        let (_, sid, _, feature, curves) = self.region_curves(q)?;
        let g = &feature.geometry;
        // Drawn curves named too: only when the named face edges can say which face (cadrs found
        // no region with those curves, so they don't bound it; Onshape names neighbours too).
        let drawn_named = curves.iter().any(|c| g.curves.get(*c).is_some_and(|c| !c.construction));
        if drawn_named && !g.imprint.iter().any(|im| curves.contains(&im.id)) {
            return None;
        }
        let plane = feature.plane?;
        let frame = plane.frame();
        let n = cross(frame.u, frame.v);
        let parts = self.parts();
        let wanted: Vec<&cadrs_sketch::Imprint> = g.imprint.iter().filter(|im| curves.contains(&im.id)).collect();
        let mut best: Option<(usize, usize, usize)> = None;
        for (pi, part) in parts.iter().enumerate() {
            for (fi, face) in part.solid.faces.iter().enumerate() {
                let Some(pl) = face.plane else { continue };
                let fnrm = cross(pl.u, pl.v);
                let d = [pl.origin[0] - frame.origin[0], pl.origin[1] - frame.origin[1], pl.origin[2] - frame.origin[2]];
                if dot(fnrm, n) < 1.0 - 1e-6 || dot(d, n).abs() > 1e-3 {
                    continue;
                }
                let score = if wanted.is_empty() {
                    usize::from(matches!(plane, PlaneRef::Face(fp) if fp.face == face.name))
                } else {
                    let loops: Vec<Vec<cadrs_sketch::Vec2>> = face
                        .loops
                        .iter()
                        .map(|l| {
                            l.iter()
                                .map(|p| {
                                    let d = [p[0] - frame.origin[0], p[1] - frame.origin[1], p[2] - frame.origin[2]];
                                    cadrs_sketch::Vec2::new(dot(d, frame.u), dot(d, frame.v))
                                })
                                .collect()
                        })
                        .collect();
                    wanted.iter().filter(|im| on_loops(&loops, imprint_point(&im.shape))).count()
                };
                if score > 0 && best.is_none_or(|(s, _, _)| score > s) {
                    best = Some((score, pi, fi));
                }
            }
        }
        let (_, pi, fi) = best?;
        let part = &parts[pi];
        let face = &part.solid.faces[fi];
        let to2 = |p: &[f64; 3]| {
            let d = [p[0] - frame.origin[0], p[1] - frame.origin[1], p[2] - frame.origin[2]];
            cadrs_sketch::Vec2::new(dot(d, frame.u), dot(d, frame.v))
        };
        let loops: Vec<Vec<cadrs_sketch::Vec2>> = face.loops.iter().map(|l| l.iter().map(to2).collect()).collect();
        // The named edges all on one hole of the face: the region inside that hole, which
        // cadrs has once the hole's outline is drawn in the sketch.
        let outer = (0..loops.len()).max_by(|a, b| polygon_area(&loops[*a]).abs().total_cmp(&polygon_area(&loops[*b]).abs()));
        let hole = (0..loops.len()).filter(|i| Some(*i) != outer).find(|i| {
            !wanted.is_empty() && wanted.iter().all(|im| on_loops(std::slice::from_ref(&loops[*i]), imprint_point(&im.shape)))
        });
        if let Some(h) = hole {
            let outline: Vec<cadrs_sketch::ImprintShape> =
                g.imprint.iter().filter(|im| on_loops(std::slice::from_ref(&loops[h]), imprint_point(&im.shape))).map(|im| im.shape).collect();
            let before: std::collections::HashSet<cadrs_sketch::CurveId> = g.curves.keys().collect();
            let ops: Vec<cadrs_sketch::SketchOp> = outline.iter().map(drawn).collect();
            self.s.run(&cadrs_core::commands::EditSketch { element: self.el, feature: sid, op: cadrs_sketch::SketchOp::Batch(ops) }).ok()?;
            let g2 = sketch::sketch_of(self.s, self.el, sid).ok()?;
            let new: std::collections::BTreeSet<cadrs_sketch::CurveId> = g2.curves.keys().filter(|c| !before.contains(c)).collect();
            let r = refs::region_with(sid, &g2, &new)?;
            notes.push("a region inside a hole of the face was selected: the hole's outline is drawn in the sketch".into());
            return Some(FaceOrRegion::Region(r));
        }
        Some(FaceOrRegion::Face(cadrs_core::document::FaceRef { part: part.id, face: face.name, seed: part.solid.face_point(fi).unwrap_or([0.0; 3]) }))
    }

    /// The sketch region a region query picks. Besides the sketch entities around it, the
    /// query may name model edges next to it (a sketch curve crossing the face it is drawn on
    /// is cut by the face's edges, which cadrs imprints on the sketch): those are found in the
    /// model and matched to the sketch's imprinted edges.
    pub(crate) fn region_of(&mut self, q: &crate::query::Value) -> Vec<cadrs_core::document::RegionRef> {
        self.region_of_one(q).into_iter().flatten().collect()
    }

    /// See [`Self::region_of`]: `Some(regions)` (several when Onshape's one region is cut in
    /// pieces in cadrs).
    fn region_of_one(&mut self, q: &crate::query::Value) -> Option<Vec<cadrs_core::document::RegionRef>> {
        use crate::query::Value as Q;
        let (sk, sid, map, feature, curves) = self.region_curves(q)?;
        let g = &feature.geometry;
        // The side of each named sketch line the region is on (the ±1 of Onshape's topology
        // disambiguation).
        let mut sides: Vec<(cadrs_sketch::CurveId, f64)> = Vec::new();
        // The curves the region's own boundary entries name (not the ones nested in their
        // disambiguation, which name neighbours).
        let mut direct: std::collections::BTreeSet<cadrs_sketch::CurveId> = std::collections::BTreeSet::new();
        // How many edges the query lists around the region (its boundary's size in Onshape).
        let mut entries = 0;
        for d in q.get("disambiguationData").map(Q::items).unwrap_or_default() {
            if d.get("disambiguationType").and_then(Q::as_str) != Some("TOPOLOGY") {
                continue;
            }
            for e in d.get("entities").map(Q::items).unwrap_or_default() {
                entries += 1;
                let (eq, sign) = match e {
                    Q::Array(pair) => (pair.first(), pair.get(1).and_then(Q::as_f64)),
                    other => (Some(other), None),
                };
                let Some(c) = eq.and_then(|eq| refs::own_sketch_entity(eq, &sk)).and_then(|i| map.curves.get(&i).copied()) else { continue };
                direct.insert(c);
                if let Some(sign) = sign {
                    sides.push((c, if map.reversed.contains(&c) { -sign } else { sign }));
                }
            }
        }
        if std::env::var_os("CADRS_ONSHAPE_SIDE_STATS").is_some() {
            let r0 = refs::region_with(sid, g, &curves);
            if let Some(r0) = r0.as_ref().and_then(|r| r.resolve(g)) {
                let (l, rt) = refs::side_agreement(g, &r0, &sides);
                eprintln!("SIDES left {l} right {rt} of {}", sides.len());
            }
        }
        // A region Onshape bounds by sketch curves alone that cadrs cuts in pieces with a face's
        // imprinted edges: all the pieces.
        let pieces = refs::pieces_bounded_by(sid, g, &direct);
        if pieces.len() > 1 {
            return Some(pieces);
        }
        let r = refs::region_with_sides(sid, g, &curves, &direct, &sides, entries);
        if std::env::var_os("CADRS_ONSHAPE_DEBUG_REGIONS").is_some() {
            eprintln!("REGIONSKETCH curves {} imprint {:?} regions {} plane {:?}", g.curves.len(), g.imprint.iter().map(|i| i.shape).collect::<Vec<_>>(), cadrs_sketch::region::regions(g).len(), feature.plane.map(|p| std::mem::discriminant(&p)));
            for (_, c) in &g.curves { eprintln!("   curve {:?} construction {}", c.kind, c.construction); }
            for (id, p) in &g.points { eprintln!("   point {id:?} {:?}", p.pos); }
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_SHAPES").is_some() {
                for (_, c) in &g.curves {
                    match c.kind {
                        cadrs_sketch::CurveKind::Line { a, b } => eprintln!("    line {:?} {:?}", g.points[a].pos, g.points[b].pos),
                        cadrs_sketch::CurveKind::Arc { center, start, end } => {
                            eprintln!("    arc {:?} {:?} {:?}", g.points[center].pos, g.points[start].pos, g.points[end].pos)
                        }
                        cadrs_sketch::CurveKind::Circle { center, radius } => eprintln!("    circle {:?} {radius}", g.points[center].pos),
                        _ => {}
                    }
                }
                for im in &g.imprint {
                    eprintln!("    imprint {:?}", im.shape);
                }
            }
            let rev: HashMap<cadrs_sketch::CurveId, &String> = map.curves.iter().map(|(k, v)| (*v, k)).collect();
            let name = |c: &cadrs_sketch::CurveId| rev.get(c).map(|s| s.to_string()).unwrap_or_else(|| format!("{c:?}"));
            eprintln!("REGION wants {:?}", curves.iter().map(name).collect::<Vec<_>>());
            if let Some(r) = &r {
                eprintln!("       got  {:?}", r.curves.iter().map(name).collect::<Vec<_>>());
            }
        }
        r.map(|r| vec![r])
    }

    /// The error the last rebuild reported for feature `id`, if any.
    pub(crate) fn rebuild_error(&self, id: FeatureId) -> Option<String> {
        let (_, b) = self.built.as_ref()?;
        b.errors.iter().find(|(f, _)| *f == id).map(|(_, e)| e.clone())
    }

    /// The face a query refers to, in the model as built so far.
    pub(crate) fn face_of(&mut self, q: &crate::query::Value) -> Option<cadrs_core::document::FaceRef> {
        let parts = self.parts();
        self.model(&parts).face(q)
    }

    fn run(mut self) {
        let Some(json) = self.raw.features() else {
            self.report.notes.push("features.json missing".into());
            return;
        };
        let sketches_json = self.raw.sketches().unwrap_or(Value::Null);
        let trace = std::env::var_os("CADRS_ONSHAPE_TRACE").is_some();
        for f in json["features"].as_array().into_iter().flatten() {
            let started = std::time::Instant::now();
            let name = f["name"].as_str().unwrap_or_default().to_string();
            let kind = f["featureType"].as_str().unwrap_or_default().to_string();
            let fid = f["featureId"].as_str().unwrap_or_default().to_string();
            let mut fr = FeatureReport { name, kind: kind.clone(), outcome: Outcome::Full, notes: Vec::new() };
            if f["suppressed"].as_bool() == Some(true) {
                fr.outcome = Outcome::Suppressed;
                self.report.features.push(fr);
                continue;
            }
            if self.options.skip.contains(&fid) {
                fr.outcome = Outcome::Skipped;
                fr.notes.push("left out: its rebuild hung or crashed in cadrs (a kernel bug to fix)".into());
                self.report.features.push(fr);
                continue;
            }
            if let Some(p) = &self.options.progress {
                std::fs::write(p, &fid).ok();
            }
            let result = match kind.as_str() {
                "newSketch" => self.sketch(f, &fid, &sketches_json, &mut fr),
                "extrude" => self.extrude(f, &fid, &mut fr),
                "booleanBodies" => self.boolean(f, &fid, &mut fr),
                "fillet" => self.fillet(f, &fid, &mut fr),
                "chamfer" => self.chamfer(f, &fid, &mut fr),
                "revolve" => self.revolve(f, &fid, &mut fr),
                "mirror" => self.mirror(f, &fid, &mut fr),
                "linearPattern" | "circularPattern" => self.pattern(f, &fid, &mut fr),
                "mateConnector" => self.mate_connector(f, &fid, &mut fr),
                "deleteBodies" => self.delete_bodies(f, &fid, &mut fr),
                "loft" => self.loft(f, &fid, &mut fr),
                "hole" => self.hole(f, &fid, &mut fr),
                "transform" => self.transform(f, &fid, &mut fr),
                "importForeign" => self.import_foreign(f, &fid, &mut fr),
                "importDerived" => self.import_derived(f, &fid, &mut fr),
                "thicken" => self.thicken(f, &fid, &mut fr),
                "helix" => self.helix(f, &fid, &mut fr),
                "fill" => self.fill(f, &fid, &mut fr),
                "sweep" => self.sweep(f, &fid, &mut fr),
                "assignVariable" => {
                    self.variable(f, &mut fr);
                    Ok(())
                }
                _ => {
                    fr.outcome = Outcome::Skipped;
                    fr.notes.push("no importer for this feature type yet".into());
                    Ok(())
                }
            };
            if let Err(e) = result {
                fr.outcome = Outcome::Skipped;
                fr.notes.push(e.to_string());
            }
            if trace {
                eprintln!("  {:>8.3} s  {} ({}) {:?}", started.elapsed().as_secs_f64(), fr.name, fr.kind, fr.outcome);
                if std::env::var_os("CADRS_ONSHAPE_TRACE_PARTS").is_some() {
                    for p in self.parts() {
                        let b = bbox_of(p.solid.positions.iter().copied()).unwrap_or_default();
                        eprintln!("             {:<12} vol {:>10.1}  box [{:.9} {:.9} {:.9}] .. [{:.9} {:.9} {:.9}]", p.name, p.mass.as_ref().map_or(0.0, |m| m.volume), b[0], b[1], b[2], b[3], b[4], b[5]);
                    }
                }
            }
            self.report.features.push(fr);
        }
        self.name_parts();
        let parts = self.parts();
        crate::eval::set_source(self.doc_id, &self.raw.id, crate::eval::SourceCtx { features: self.features.clone(), sketches: self.sketches.clone(), parts });
    }

    fn variable(&mut self, f: &Value, fr: &mut FeatureReport) {
        let name = param(f, "name").and_then(|p| p["value"].as_str()).unwrap_or_default().to_string();
        let kind = param(f, "variableType").and_then(|p| p["value"].as_str()).unwrap_or("ANY").to_string();
        let value_param = match kind.as_str() {
            "LENGTH" => "lengthValue",
            "ANGLE" => "angleValue",
            "NUMBER" => "numberValue",
            _ => "anyValue",
        };
        match param(f, value_param).and_then(|p| p["expression"].as_str()) {
            Some(expr) if !name.is_empty() => {
                self.vars.insert(name, Var { kind, expr: expr.to_string() });
                fr.notes.push("cadrs has no variables yet: its value is used where it appears".into());
                fr.outcome = Outcome::Partial;
            }
            _ => {
                fr.outcome = Outcome::Skipped;
                fr.notes.push("variable without a value expression".into());
            }
        }
    }

    fn sketch(&mut self, f: &Value, fid: &str, sketches: &Value, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let solved = sketch::Solved::find(sketches, fid).ok_or_else(|| CommandError::Invalid("not in sketches.json".into()))?;
        let plane = match sketch::default_plane(f) {
            Some(p) => p,
            None => {
                // The faces the query could mean, keeping the first whose plane is the one
                // Onshape's sketch lies on (a query that matches several side faces must not
                // put the sketch on the wrong one).
                let (n, o) = sketch::world_plane(&solved).ok_or_else(|| CommandError::Invalid("sketch without a matrix".into()))?;
                let feats = self.s.doc.element(self.el).map(|e| e.features().to_vec()).unwrap_or_default();
                let parts = self.parts();
                let mut by_query = None;
                // A face the query names that is parallel and close, if none is exact (a model
                // a little off from Onshape's): the sketch still lands where Onshape's is.
                let mut near = None;
                for p in refs::picks(param(f, "sketchPlane")) {
                    let Pick::Query(q) = p else { continue };
                    // A region of another sketch: that sketch's plane.
                    if let Some(op) = q.get("operationId").and_then(crate::query::Value::as_str)
                        && let Some((map, _)) = self.sketches.get(refs::op_feature(op))
                        && let Some(pl) = feats.iter().find(|x| x.id == map.feature).and_then(|x| x.sketch()).and_then(|x| x.plane)
                        && plane_matches(&pl, n, o)
                    {
                        by_query = Some(pl);
                        break;
                    }
                    let model = self.model(&parts);
                    // Of the faces on the sketch's plane, the one the sketch's geometry is on.
                    let pts = sketch::world_points(&solved);
                    let mut best: Option<(usize, PlaneRef)> = None;
                    for e in model.eval(&q) {
                        let crate::eval::Ent::Face(pi, fi) = e else { continue };
                        if let Some(pl) = plane_on(&feats, &parts[pi], fi) {
                            if plane_matches(&pl, n, o) {
                                let score = points_on_face(&parts[pi], fi, &pts);
                                if best.as_ref().is_none_or(|(s, _)| score > *s) {
                                    best = Some((score, pl));
                                }
                            } else if near.is_none() && plane_near(&pl, n, o, 1.0) {
                                near = Some(pl);
                            }
                        }
                    }
                    if let Some((_, pl)) = best {
                        by_query = Some(pl);
                    }
                    if by_query.is_none() && std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                        eprintln!("PLANE {} | {}", f["name"], self.describe(&q));
                        let qop = q.get("operationId").and_then(crate::query::Value::as_str).map(refs::op_feature).and_then(|o| self.features.get(o)).map(|f| f.0);
                        for p in &parts {
                            for face in &p.solid.faces {
                                if Some(face.name.op) == qop {
                                    let fp = face.plane.map(|pl| (cross(pl.u, pl.v), dot([o[0] - pl.origin[0], o[1] - pl.origin[1], o[2] - pl.origin[2]], cross(pl.u, pl.v))));
                                    eprintln!("      op face {:?} {fp:?}", face.name.origin);
                                }
                            }
                        }
                        let model = self.model(&parts);
                        for e in model.eval(&q) {
                            let crate::eval::Ent::Face(pi, fi) = e else { continue };
                            let face = &parts[pi].solid.faces[fi];
                            let fp = face.plane.map(|pl| (cross(pl.u, pl.v), dot([o[0] - pl.origin[0], o[1] - pl.origin[1], o[2] - pl.origin[2]], cross(pl.u, pl.v))));
                            eprintln!("      cand {:?} plane(n, dist)={fp:?} face_plane={}; want n={n:?}", face.name.origin, cadrs_core::parts::face_plane(&feats, FeatureId(face.name.op), face.name).is_some());
                        }
                    }
                }
                by_query
                    .or_else(|| self.face_plane(&solved))
                    .or(near)
                    .ok_or_else(|| CommandError::Invalid("sketch plane is not a default plane or a planar face of the model".into()))?
            }
        };
        let id = self.feature_id(fid);
        let parts = self.parts();
        let map = sketch::import(self.s, self.el, id, f, &solved, plane, &parts, fr)?;
        let g = sketch::sketch_of(self.s, self.el, id)?;
        self.features.insert(fid.to_string(), id);
        self.sketches.insert(fid.to_string(), (map, g));
        Ok(())
    }

    /// A planar face of the model the sketch lies on (same plane, facing the same way).
    fn face_plane(&mut self, solved: &sketch::Solved) -> Option<PlaneRef> {
        let (n, o) = sketch::world_plane(solved)?;
        let parts = self.parts();
        let feats = self.s.doc.element(self.el)?.features().to_vec();
        // Of the faces on that plane, the one the sketch's geometry is on.
        let pts = sketch::world_points(solved);
        let mut best: Option<(usize, PlaneRef)> = None;
        for part in &parts {
            for (fi, f) in part.solid.faces.iter().enumerate() {
                let Some(pl) = f.plane else { continue };
                let fnrm = cross(pl.u, pl.v);
                let d = [o[0] - pl.origin[0], o[1] - pl.origin[1], o[2] - pl.origin[2]];
                if dot(fnrm, n) > 1.0 - 1e-6 && dot(d, fnrm).abs() < 1e-4
                    && let Some(p) = plane_on(&feats, part, fi)
                {
                    let score = points_on_face(part, fi, &pts);
                    if best.as_ref().is_none_or(|(s, _)| score > *s) {
                        best = Some((score, p));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// The cadrs sketch whose curve an edge query names (a `SKETCH_ENTITY` edge of a sketch's
    /// wire operation).
    fn edge_sketch(&self, q: &crate::query::Value) -> Option<FeatureId> {
        let op = q.get("operationId").and_then(crate::query::Value::as_str)?;
        if !op.ends_with("wireOp") || q.get("entityType").and_then(crate::query::Value::as_str) != Some("EDGE") {
            return None;
        }
        self.features.get(refs::op_feature(op)).copied()
    }

    fn extrude(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = ExtrudeFeature::default();
        let text = |id: &str| param(f, id).and_then(|p| p["value"].as_str()).unwrap_or_default().to_string();
        let flag = |id: &str| param(f, id).and_then(|p| p["value"].as_bool()).unwrap_or(false);
        let expr = |id: &str| param(f, id).and_then(|p| p["expression"].as_str()).unwrap_or_default().to_string();

        x.body = match text("bodyType").as_str() {
            "SURFACE" => BodyType::Surface,
            "THIN" => BodyType::Thin,
            _ => BodyType::Solid,
        };
        if x.body == BodyType::Thin {
            // Onshape: Thickness 1 and 2 (flipped by Flip wall), or Mid plane's Thickness.
            let len = |id: &str| eval_expr(&expr(id), Quantity::Length, &self.vars);
            if flag("midplane") {
                if let Some(t) = len("thickness") {
                    x.thin = ThinWall { thickness1: t, thickness1_expr: format!("{t} mm"), thickness2: 0.0, thickness2_expr: "0 mm".into(), flip_wall: false, mid_plane: true };
                }
            } else if let (Some(t1), Some(t2)) = (len("thickness1"), len("thickness2")) {
                x.thin = ThinWall {
                    thickness1: t1,
                    thickness1_expr: format!("{t1} mm"),
                    thickness2: t2,
                    thickness2_expr: format!("{t2} mm"),
                    flip_wall: flag("flipWall"),
                    mid_plane: false,
                };
            } else {
                fr.notes.push("thin walls imported with default thickness".into());
            }
        }
        x.op = match text("operationType").as_str() {
            "ADD" => BooleanOp::Add,
            "REMOVE" => BooleanOp::Remove,
            "INTERSECT" => BooleanOp::Intersect,
            _ => BooleanOp::New,
        };
        if x.body == BodyType::Surface {
            x.op = BooleanOp::New;
        }

        // What to extrude.
        let mut entities = refs::picks(param(f, if x.body == BodyType::Surface { "surfaceEntities" } else { "entities" }));
        if x.body == BodyType::Thin {
            // Open sketch curves given a wall.
            entities.extend(refs::picks(param(f, "wallShape")));
        }
        let mut lost = 0;
        for p in &entities {
            match p {
                Pick::WholeSketch(s) => match self.features.get(s) {
                    Some(id) => x.sketches.push(*id),
                    None => lost += 1,
                },
                Pick::Query(q) => {
                    let rs = self.region_of(q);
                    if !rs.is_empty() {
                        for r in rs {
                            if !x.regions.iter().any(|x| x.key() == r.key()) {
                                x.regions.push(r);
                            }
                        }
                    } else if let Some(r) = self.face_of(q) {
                        if !x.faces.iter().any(|f| f.face == r.face) {
                            x.faces.push(r);
                        }
                    } else if let Some(fr2) = self.face_region(q, &mut fr.notes) {
                        match fr2 {
                            FaceOrRegion::Face(r) => {
                                if !x.faces.iter().any(|f| f.face == r.face) {
                                    x.faces.push(r);
                                }
                            }
                            FaceOrRegion::Region(r) => {
                                if !x.regions.iter().any(|x| x.key() == r.key()) {
                                    x.regions.push(r);
                                }
                            }
                        }
                    } else if x.body != BodyType::Solid
                        && let Some(sk) = self.edge_sketch(q)
                    {
                        // A sketch curve (a surface or a thin wall along open curves): the
                        // sketch's curves.
                        if !x.sketches.contains(&sk) {
                            x.sketches.push(sk);
                        }
                    } else {
                        lost += 1;
                        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                            eprintln!("LOST {} | {}", f["name"], self.describe(q));
                            let op = q.get("operationId").and_then(crate::query::Value::as_str).unwrap_or_default();
                            let sk = crate::refs::op_feature(op);
                            if let Some((map, g)) = self.sketches.get(sk) {
                                let mut ids = std::collections::BTreeSet::new();
                                crate::refs::sketch_entities(q, sk, &mut ids);
                                let mapped: Vec<String> = ids.iter().map(|i| format!("{i}{}", if map.curves.contains_key(i) { "" } else { "(unmapped)" })).collect();
                                eprintln!("      wants {mapped:?}; sketch has {} curves, {} regions", g.curves.len(), cadrs_sketch::region::regions(g).len());
                            }
                        }
                    }
                }
                Pick::Opaque(_) => lost += 1,
            }
        }
        if x.is_empty() {
            return Err(CommandError::Invalid(format!("none of its {} selections could be translated", entities.len())));
        }
        if lost > 0 {
            fr.notes.push(format!("{lost} of {} selections not translated", entities.len()));
        }

        // First end.
        x.flip = flag("oppositeDirection");
        let end = text("endBound");
        // Onshape shows and applies "Symmetric" only for Blind and Through all.
        x.symmetric = flag("symmetric") && matches!(end.as_str(), "BLIND" | "THROUGH_ALL");
        let depth = expr("depth");
        let (end_type, up_to) = self.end_bound(&end, f, "endBound", fr);
        x.end = end_type;
        x.up_to = up_to;
        if x.end == EndType::Blind {
            x.depth = eval_expr(&depth, Quantity::Length, &self.vars).ok_or_else(|| CommandError::Invalid(format!("cannot evaluate depth {depth:?}")))?;
            x.depth_expr = if depth.contains('#') { format!("{} mm", x.depth) } else { depth.clone() };
            // Onshape takes a negative depth as the other direction.
            if x.depth < 0.0 {
                x.depth = -x.depth;
                x.depth_expr = format!("{} mm", x.depth);
                x.flip = !x.flip;
            }
        }
        // Up to a default plane (cadrs goes up to faces, parts and vertices): the same as blind to
        // where the plane is.
        if end == "UP_TO_SURFACE"
            && x.up_to.is_none()
            && let Some((d, flip)) = self.depth_to_plane(&x, f, "endBoundEntityFace", x.flip)
        {
            x.depth = d;
            x.depth_expr = format!("{d} mm");
            x.flip = flip;
            fr.notes.retain(|n| !n.starts_with("up-to face not translated"));
            fr.notes.push("up to a default plane: blind to it".into());
        }
        if flag("hasOffset")
            && let Some(v) = eval_expr(&expr("offsetDistance"), Quantity::Length, &self.vars)
        {
            x.offset = Some(Offset { value: v, expr: format!("{v} mm"), flip: flag("offsetOppositeDirection") });
        }
        if flag("startOffset") {
            if text("startOffsetBound") == "BLIND" {
                if let Some(v) = eval_expr(&expr("startOffsetDistance"), Quantity::Length, &self.vars) {
                    x.start_offset = Some(Offset { value: v, expr: format!("{v} mm"), flip: flag("startOffsetOppositeDirection") });
                }
            } else {
                fr.notes.push("start offset to an entity not imported".into());
            }
        }
        if flag("hasExtrudeDirection") {
            match self.direction_param(f, "extrudeDirection") {
                Some(d) => x.direction = Some(d),
                None => fr.notes.push("custom extrude direction not translated".into()),
            }
        }
        if flag("hasDraft") {
            fr.notes.push("draft not imported".into());
        }

        // Second end.
        if flag("hasSecondDirection") && !x.symmetric {
            let (end, up_to) = self.end_bound(&text("secondDirectionBound"), f, "secondDirectionBound", fr);
            let mut second = EndCondition { end, up_to, ..x.first_end() };
            if end == EndType::Blind {
                let d = expr("secondDirectionDepth");
                second.depth = eval_expr(&d, Quantity::Length, &self.vars).unwrap_or(x.depth);
                second.depth_expr = format!("{} mm", second.depth);
            }
            // The second direction runs against the first.
            if text("secondDirectionBound") == "UP_TO_SURFACE"
                && second.up_to.is_none()
                && let Some((d, flip)) = self.depth_to_plane(&x, f, "secondDirectionBoundEntityFace", !x.flip)
                && flip != x.flip
            {
                second.depth = d;
                second.depth_expr = format!("{d} mm");
                fr.notes.retain(|n| !n.starts_with("up-to face not translated"));
            }
            second.offset = None;
            // Each end's flag turns it against the sketch normal (the second's is on by default,
            // the first's off): equal flags put both ends on the same side of the sketch plane.
            // (Onshape data: a part whose ends are 27 and 24 mm with both flags on spans 3 mm.)
            if flag("oppositeDirection") == flag("secondDirectionOppositeDirection") {
                // Both ends on the same side of the sketch plane: the body between them, the
                // nearer end as a starting offset.
                if x.end == EndType::Blind && second.end == EndType::Blind && x.start_offset.is_none() && (x.depth - second.depth).abs() > 1e-9 {
                    let (near, far) = (x.depth.min(second.depth), x.depth.max(second.depth));
                    x.start_offset = Some(Offset { value: near, expr: format!("{near} mm"), flip: false });
                    x.depth = far - near;
                    x.depth_expr = format!("{} mm", x.depth);
                } else {
                    fr.notes.push("second end on the first's side not imported".into());
                }
            } else {
                x.second = Some(second);
            }
        }

        // Boolean scope.
        if x.op != BooleanOp::New {
            if flag("defaultScope") {
                x.merge_all = true;
            } else {
                // The same lookup as every other part parameter (split pieces, copies, merged
                // bodies).
                let mut lost = 0;
                let scope = self.parts_param(f, "booleanScope", &mut lost);
                if lost > 0 {
                    fr.notes.push(format!("{lost} merge scope part(s) not translated"));
                }
                if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                    eprintln!("SCOPE {} {:?}", f["name"], scope);
                }
                x.merge_scope = scope;
            }
        }

        let id = self.feature_id(fid);
        let volume_before = total_volume(&self.parts());
        self.s.run(&AddExtrude { element: self.el, feature: id, extrude: ExtrudeFeature::default() })?;
        self.s.run(&SetExtrude { element: self.el, feature: id, extrude: x.clone(), label: "Extrude".into() })?;
        // A Remove that removes nothing fails in Onshape, so one that does here goes the wrong
        // way: an edge picked as the direction runs the other way in cadrs.
        if x.op == BooleanOp::Remove && x.direction.is_some() {
            self.parts();
            let unchanged = (total_volume(&self.parts()) - volume_before).abs() < 1e-6 * volume_before.max(1.0);
            if unchanged || self.rebuild_error(id).is_some() {
                let mut flipped = x.clone();
                flipped.flip = !flipped.flip;
                self.s.run(&SetExtrude { element: self.el, feature: id, extrude: flipped.clone(), label: "Extrude".into() })?;
                self.parts();
                if self.rebuild_error(id).is_none() && (total_volume(&self.parts()) - volume_before).abs() >= 1e-6 * volume_before.max(1.0) {
                    x = flipped;
                } else {
                    self.s.run(&SetExtrude { element: self.el, feature: id, extrude: x.clone(), label: "Extrude".into() })?;
                }
            }
        }
        if let Some(name) = f["name"].as_str() {
            self.s.run(&RenameFeature { element: self.el, feature: id, name: name.to_string() })?;
        }
        self.features.insert(fid.to_string(), id);
        // It must build. Onshape's "Up to face" goes to the face's surface; cadrs's needs the
        // profile to land on the face itself: to a parallel plane, the same extrude is a blind
        // one that deep.
        self.parts();
        if self.rebuild_error(id).is_some()
            && let Some(blind) = self.up_to_as_blind(&x).or_else(|| self.up_to_other_faces(&x, f))
        {
            if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                eprintln!("UPTO blind depth {} flip {}", blind.depth, blind.flip);
            }
            self.s.run(&SetExtrude { element: self.el, feature: id, extrude: blind, label: "Extrude".into() })?;
            self.parts();
            if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                eprintln!("UPTO blind -> {:?}", self.rebuild_error(id));
            }
            if self.rebuild_error(id).is_none() {
                fr.notes.push("up to face imported as a blind depth (the profile goes past the face's edges)".into());
            }
        }
        if let Some(e) = self.rebuild_error(id) {
            fr.notes.push(format!("rebuild error: {e}"));
        }
        if !fr.notes.is_empty() {
            fr.outcome = Outcome::Partial;
        }
        Ok(())
    }

    /// [`Self::up_to_as_blind`] with the other faces the "up to" query could mean (a merged
    /// face's histories include faces of other shapes; the one the extrude meets is planar and
    /// parallel to the sketch).
    fn up_to_other_faces(&mut self, x: &ExtrudeFeature, f: &Value) -> Option<ExtrudeFeature> {
        if x.end != EndType::UpToFace {
            return None;
        }
        let parts = self.parts();
        let mut faces = Vec::new();
        for p in refs::picks(param(f, "endBoundEntityFace")) {
            let Pick::Query(q) = p else { continue };
            for e in self.model(&parts).eval(&q) {
                if let crate::eval::Ent::Face(pi, fi) = e {
                    let part = &parts[pi];
                    faces.push(cadrs_core::document::FaceRef { part: part.id, face: part.solid.faces[fi].name, seed: part.solid.face_point(fi).unwrap_or([0.0; 3]) });
                }
            }
        }
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            for fc in &faces {
                let pl = parts.iter().find(|p| p.id == fc.part).and_then(|p| p.solid.face(&fc.face)).and_then(|x| x.plane);
                eprintln!("UPTO cand {:?} plane {:?}", fc.face.origin, pl.map(|p| (cross(p.u, p.v), p.origin)));
            }
        }
        faces.into_iter().find_map(|face| {
            let mut y = x.clone();
            y.up_to = Some(UpTo::Face(face));
            self.up_to_as_blind(&y)
        })
    }

    /// An "Up to face" extrude to a planar face parallel to its sketch as the blind extrude
    /// that reaches the face's plane.
    fn up_to_as_blind(&mut self, x: &ExtrudeFeature) -> Option<ExtrudeFeature> {
        let Some(UpTo::Face(target)) = x.up_to else { return None };
        if x.end != EndType::UpToFace || x.symmetric {
            return None;
        }
        let sketch = x.regions.first().map(|r| r.sketch).or_else(|| x.sketches.first().copied())?;
        let frame = self.s.doc.element(self.el)?.feature(sketch)?.sketch()?.plane?.frame();
        let n = cross(frame.u, frame.v);
        let n = if x.flip { [-n[0], -n[1], -n[2]] } else { n };
        let parts = self.parts();
        let part = parts.iter().find(|p| p.id == target.part)?;
        let face = part.solid.face(&target.face)?.plane?;
        let fnrm = cross(face.u, face.v);
        if dot(fnrm, n).abs() < 1.0 - 1e-9 {
            return None;
        }
        let d = [face.origin[0] - frame.origin[0], face.origin[1] - frame.origin[1], face.origin[2] - frame.origin[2]];
        let t = dot(d, n);
        if t.abs() < 1e-6 {
            return None;
        }
        let mut b = x.clone();
        b.end = EndType::Blind;
        b.up_to = None;
        b.depth = t.abs();
        b.depth_expr = format!("{} mm", t.abs());
        if t < 0.0 {
            b.flip = !b.flip;
        }
        Some(b)
    }

    /// For an end "up to" a default plane (parameter `key`): the blind depth from the extrude's
    /// sketch plane to it along its direction (`flip`: against the sketch normal), and the
    /// direction that reaches it.
    fn depth_to_plane(&self, x: &ExtrudeFeature, f: &Value, key: &str, flip: bool) -> Option<(f64, bool)> {
        let target = refs::picks(param(f, key)).into_iter().find_map(|p| p.default_plane())?;
        let sketch = x.regions.first().map(|r| r.sketch).or_else(|| x.sketches.first().copied())?;
        let from = self.s.doc.element(self.el)?.feature(sketch)?.sketch()?.plane?.frame();
        let to = target.frame();
        let nt = cross(to.u, to.v);
        let mut d = cross(from.u, from.v);
        if flip {
            d = [-d[0], -d[1], -d[2]];
        }
        let along = dot(d, nt);
        if along.abs() < 1e-9 {
            return None;
        }
        let gap = [to.origin[0] - from.origin[0], to.origin[1] - from.origin[1], to.origin[2] - from.origin[2]];
        let t = dot(gap, nt) / along;
        if t.abs() < 1e-9 {
            return None;
        }
        Some(if t > 0.0 { (t, flip) } else { (-t, !flip) })
    }

    /// An end condition: its type and, for "up to", its target.
    pub(crate) fn end_bound(&mut self, bound: &str, f: &Value, prefix: &str, fr: &mut FeatureReport) -> (EndType, Option<UpTo>) {
        match bound {
            "THROUGH_ALL" => (EndType::ThroughAll, None),
            "UP_TO_NEXT" => (EndType::UpToNext, None),
            "UP_TO_SURFACE" => {
                let key = if prefix == "endBound" { "endBoundEntityFace" } else { "secondDirectionBoundEntityFace" };
                let target = refs::picks(param(f, key)).into_iter().find_map(|p| match p {
                    Pick::Query(q) => self.face_of(&q),
                    _ => None,
                });
                if std::env::var_os("CADRS_ONSHAPE_TRACE").is_some()
                    && let Some(t) = &target
                {
                    let parts = self.parts();
                    if let Some(p) = parts.iter().find(|p| p.id == t.part) {
                        let planar = p.solid.face(&t.face).map(|f| f.plane.is_some());
                        eprintln!("    [up to] face {:?} planar {planar:?}; its part has {} faces", t.face.origin, p.solid.faces.len());
                    }
                }
                match target {
                    Some(face) => (EndType::UpToFace, Some(UpTo::Face(face))),
                    None => {
                        fr.notes.push("up-to face not translated: blind instead".into());
                        (EndType::Blind, None)
                    }
                }
            }
            "BLIND" | "SYMMETRIC" | "" => (EndType::Blind, None),
            other => {
                fr.notes.push(format!("end type {other} not imported: blind instead"));
                (EndType::Blind, None)
            }
        }
    }

    /// Names the parts and sets their materials as in Onshape, matching parts by volume and
    /// centroid, and records how well each matched.
    fn name_parts(&mut self) {
        if std::env::var_os("CADRS_ONSHAPE_DEBUG_FACES").is_some() {
            let names: HashMap<uuid::Uuid, String> = self.s.doc.element(self.el).map(|e| e.features().iter().map(|f| (f.id.0, f.name.clone())).collect()).unwrap_or_default();
            for part in self.parts() {
                let mut planes: HashMap<(i64, i64, i64, i64), Vec<String>> = HashMap::new();
                for f in &part.solid.faces {
                    let Some(pl) = f.plane else { continue };
                    let n = cross(pl.u, pl.v);
                    let k = ((n[0] * 1000.0).round() as i64, (n[1] * 1000.0).round() as i64, (n[2] * 1000.0).round() as i64, (dot(n, pl.origin) * 1000.0).round() as i64);
                    planes.entry(k).or_default().push(format!("{}:{:?}", names.get(&f.name.op).cloned().unwrap_or_default(), f.name.origin).chars().take(60).collect());
                }
                for (k, v) in planes.iter().filter(|(_, v)| v.len() > 1) {
                    eprintln!("COPLANAR {} {:?}: {} faces {:?}", part.name, k, v.len(), v);
                }
            }
        }
        let Some(mp) = read_json(&self.raw.dir.join("massproperties.json")) else { return };
        let parts_json = read_json(&self.raw.dir.join("parts.json")).unwrap_or(Value::Null);
        let details = read_json(&self.raw.dir.join("bodydetails.json")).unwrap_or(Value::Null);
        let parts = self.parts();
        // Onshape's solid parts: id, name, volume (and its tolerance band), bounding box of
        // their vertices (the centroid is zero for parts without a material, so it can't match).
        struct Os {
            pid: String,
            name: String,
            volume: f64,
            band: (f64, f64),
            bbox: Option<[f64; 6]>,
            info: Option<Value>,
        }
        let mut onshape = Vec::new();
        // Composite parts (an Import with "Create composite"): Onshape's one composite against
        // the parts the Import made here, summed.
        let imports: Vec<(String, FeatureId)> = self
            .raw
            .features()
            .and_then(|f| f["features"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|f| f["featureType"].as_str() == Some("importForeign") && param(f, "createComposite").and_then(|p| p["value"].as_bool()) == Some(true))
            .filter_map(|f| {
                let name = param(f, "compositeName").and_then(|p| p["value"].as_str()).unwrap_or_default().to_string();
                Some((name, *self.features.get(f["featureId"].as_str()?)?))
            })
            .collect();
        let mut in_composites: std::collections::HashSet<cadrs_core::ids::PartId> = std::collections::HashSet::new();
        for (pid, b) in mp["bodies"].as_object().into_iter().flatten() {
            let info = parts_json.as_array().and_then(|a| a.iter().find(|p| p["partId"].as_str() == Some(pid)));
            if info.and_then(|p| p["bodyType"].as_str()) != Some("composite") {
                continue;
            }
            let name = info.and_then(|p| p["name"].as_str()).unwrap_or(pid).to_string();
            let vol = b["volume"].as_array().and_then(|v| v.first()).and_then(Value::as_f64).unwrap_or(0.0) * 1e9;
            let volume = |p: &&Part| p.mass.as_ref().map(|m| m.volume).unwrap_or_else(|| p.solid.volume());
            let features: Vec<FeatureId> = imports.iter().filter(|(n, _)| *n == name || imports.len() == 1).map(|(_, f)| *f).collect();
            let mut mine: Vec<&Part> = parts.iter().filter(|p| features.contains(&p.id.feature) || p.features.iter().any(|f| features.contains(f))).collect();
            if mine.is_empty() {
                // A composite brought in by a Derived feature: the feature whose parts add up to
                // it best.
                let mut groups: HashMap<FeatureId, Vec<&Part>> = HashMap::new();
                for p in parts.iter().filter(|p| !in_composites.contains(&p.id)) {
                    groups.entry(p.id.feature).or_default().push(p);
                }
                if let Some((_, g)) = groups
                    .into_iter()
                    .filter(|(_, g)| g.len() > 1 || imports.is_empty())
                    .map(|(f, g)| {
                        let sum: f64 = g.iter().map(volume).sum();
                        ((sum - vol).abs() / vol.abs().max(1e-9), (f, g))
                    })
                    .filter(|(d, _)| *d < 0.05)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, x)| x)
                {
                    mine = g;
                }
            }
            in_composites.extend(mine.iter().map(|p| p.id));
            let sum: f64 = mine.iter().map(volume).sum();
            self.report.parts.push(PartCheck { name, onshape_volume: vol, cadrs_volume: (!mine.is_empty()).then_some(sum), bbox_note: None });
        }
        for (pid, b) in mp["bodies"].as_object().into_iter().flatten() {
            if pid == "-all-" {
                continue;
            }
            let info = parts_json.as_array().and_then(|a| a.iter().find(|p| p["partId"].as_str() == Some(pid))).cloned();
            if info.as_ref().is_some_and(|p| p["bodyType"].as_str().is_some_and(|t| t != "solid")) {
                continue;
            }
            let v = |i: usize| b["volume"].as_array().and_then(|v| v.get(i)).and_then(Value::as_f64).unwrap_or(0.0) * 1e9;
            let body = details["bodies"].as_array().and_then(|a| a.iter().find(|x| x["id"].as_str() == Some(pid)));
            let bbox = body.and_then(|b| bbox_of(b["vertices"].as_array()?.iter().filter_map(|v| {
                let p = &v["point"];
                Some([p["x"].as_f64()? * 1000.0, p["y"].as_f64()? * 1000.0, p["z"].as_f64()? * 1000.0])
            })));
            onshape.push(Os { pid: pid.clone(), name: info.as_ref().and_then(|p| p["name"].as_str()).unwrap_or(pid).to_string(), volume: v(0), band: (v(1), v(2)), bbox, info });
        }
        let cadrs: Vec<(usize, f64, Option<[f64; 6]>)> = parts
            .iter()
            .enumerate()
            .filter(|(_, p)| !in_composites.contains(&p.id))
            .map(|(i, p)| (i, p.mass.as_ref().map(|m| m.volume).unwrap_or_else(|| p.solid.volume()), bbox_of(p.solid.positions.iter().copied())))
            .collect();
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            for o in &onshape {
                eprintln!("PARTS onshape {}: {:.1} mm³ {:?}", o.name, o.volume, o.bbox);
            }
            for (_, v, bb) in &cadrs {
                eprintln!("PARTS cadrs: {v:.1} mm³ {bb:?}");
            }
        }
        // Pair them best first: close boxes, close volumes.
        let mut pairs = Vec::new();
        for (oi, o) in onshape.iter().enumerate() {
            for (ci, (_, v, bb)) in cadrs.iter().enumerate() {
                let dv = (v - o.volume).abs() / o.volume.abs().max(1e-9);
                let db = match (o.bbox, bb) {
                    (Some(a), Some(b)) => (0..6).map(|k| (a[k] - b[k]).abs()).sum::<f64>(),
                    _ => 0.0,
                };
                let size = o.bbox.map(|a| (a[3] - a[0]) + (a[4] - a[1]) + (a[5] - a[2])).unwrap_or(1.0).max(1e-6);
                if dv < 0.5 || db / size < 0.2 {
                    pairs.push((db / size + dv, oi, ci));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut matched: HashMap<usize, usize> = HashMap::new();
        let mut used = std::collections::HashSet::new();
        for (_, oi, ci) in pairs {
            if !matched.contains_key(&oi) && used.insert(ci) {
                matched.insert(oi, ci);
            }
        }
        // For assemblies: which part each Onshape part became, and how far it sits from
        // Onshape's. A STEP import here places each part where the file's assembly puts it, while
        // Onshape keeps a part in its own coordinates and moves it in its assemblies; the vertex
        // boxes give that move (when they differ by a translation).
        let map: HashMap<String, (cadrs_core::ids::PartId, [f64; 3])> = matched
            .iter()
            .map(|(oi, ci)| {
                let part = &parts[cadrs[*ci].0];
                let ours = bbox_of(part.solid.vertices.iter().map(|v| v.point));
                (onshape[*oi].pid.clone(), (part.id, offset(onshape[*oi].bbox, ours)))
            })
            .collect();
        crate::eval::set_part_map(self.doc_id, &self.raw.id, map);
        // Onshape's part colours, one command per colour.
        let mut colours: Vec<(Appearance, Vec<cadrs_core::ids::PartId>)> = Vec::new();
        for (oi, o) in onshape.iter().enumerate() {
            let m = matched.get(&oi).map(|ci| &cadrs[*ci]);
            let mut check = PartCheck { name: o.name.clone(), onshape_volume: o.volume, cadrs_volume: m.map(|c| c.1), bbox_note: None };
            // Within Onshape's own tolerance band counts as equal.
            if let Some(c) = m
                && c.1 >= o.band.0.min(o.volume)
                && c.1 <= o.band.1.max(o.volume)
            {
                check.cadrs_volume = Some(o.volume);
            }
            if let (Some(c), Some(a)) = (m, o.bbox)
                && let Some(b) = c.2
            {
                let d: Vec<String> = ["x-", "y-", "z-", "x+", "y+", "z+"]
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| (a[*k] - b[*k]).abs() > 0.01)
                    .map(|(k, n)| format!("{n} {:+.2}", b[k] - a[k]))
                    .collect();
                if !d.is_empty() {
                    check.bbox_note = Some(format!("box differs (mm): {}", d.join(", ")));
                }
            }
            if let Some(c) = m {
                let part = &parts[c.0];
                self.s.run(&RenamePart { element: self.el, part: part.id, name: o.name.clone() }).ok();
                let material = o.info.as_ref().and_then(|i| i["material"]["displayName"].as_str()).and_then(cadrs_core::material::library);
                if material.is_some() {
                    self.s.run(&SetPartMaterial { element: self.el, parts: vec![part.id], material }).ok();
                }
                if let Some(a) = o.info.as_ref().and_then(|i| appearance(&i["appearance"])) {
                    match colours.iter_mut().find(|(c, _)| *c == a) {
                        Some((_, ps)) => ps.push(part.id),
                        None => colours.push((a, vec![part.id])),
                    }
                }
            }
            self.report.parts.push(check);
        }
        for (a, parts) in colours {
            self.s.run(&SetPartAppearance { element: self.el, parts, appearance: Some(a) }).ok();
        }
    }
}

/// How far box `b` sits from box `a` when it is `a` moved (zero otherwise, or without both).
fn offset(a: Option<[f64; 6]>, b: Option<[f64; 6]>) -> [f64; 3] {
    let (Some(a), Some(b)) = (a, b) else { return [0.0; 3] };
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    if (0..3).all(|k| (b[k + 3] - a[k + 3] - d[k]).abs() < 1e-3) { d } else { [0.0; 3] }
}

/// An Onshape part appearance (`{"color": {"red", "green", "blue"}, "opacity"}`, 0–255).
fn appearance(v: &Value) -> Option<Appearance> {
    let c = &v["color"];
    let ch = |k: &str| c[k].as_u64().map(|x| x.min(255) as u8);
    let alpha = v["opacity"].as_u64().map_or(255, |x| x.min(255) as u8);
    Some(Appearance::rgb(ch("red")?, ch("green")?, ch("blue")?).with_alpha(alpha))
}

/// The model edges a region query names (edges not made by sketch `sketch`), outermost ones:
/// what they were derived from is not searched further.
fn model_edges<'q>(q: &'q crate::query::Value, sketch: &str, out: &mut Vec<&'q crate::query::Value>) {
    use crate::query::Value as Q;
    match q {
        Q::Typed(_, v) => {
            let is_edge = q.get("entityType").and_then(Q::as_str) == Some("EDGE");
            let qtype = q.get("queryType").and_then(Q::as_str).unwrap_or_default();
            let op = q.get("operationId").and_then(Q::as_str).map(refs::op_feature);
            if is_edge && !matches!(qtype, "SKETCH_ENTITY" | "IMPRINT") && op.is_some_and(|o| o != sketch) {
                out.push(q);
                return;
            }
            model_edges(v, sketch, out);
        }
        Q::Map(m) => m.iter().for_each(|(_, v)| model_edges(v, sketch, out)),
        Q::Array(a) => a.iter().for_each(|v| model_edges(v, sketch, out)),
        _ => {}
    }
}

/// What a region query can turn out to be.
pub(crate) enum FaceOrRegion {
    Face(cadrs_core::document::FaceRef),
    Region(cadrs_core::document::RegionRef),
}

/// A polygon's signed area.
fn polygon_area(p: &[cadrs_sketch::Vec2]) -> f64 {
    (0..p.len()).map(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        a.x * b.y - b.x * a.y
    }).sum::<f64>() / 2.0
}

/// The sketch op drawing an imprinted edge as a sketch curve.
fn drawn(shape: &cadrs_sketch::ImprintShape) -> cadrs_sketch::SketchOp {
    use cadrs_sketch::{ImprintShape, SketchOp, Vec2};
    match *shape {
        ImprintShape::Line(a, b) => SketchOp::AddPolyline { points: vec![a, b], closed: false, construction: false, label: "Add line" },
        ImprintShape::Circle(c, r) => SketchOp::AddCircle { center: c, radius: r, construction: false },
        ImprintShape::Arc { center, radius, start_angle, sweep } => {
            let at = |t: f64| Vec2::new(center.x + radius * t.cos(), center.y + radius * t.sin());
            SketchOp::AddArc { center, start: at(start_angle), end: at(start_angle + sweep), construction: false }
        }
    }
}

/// A point on an imprinted edge (its middle).
fn imprint_point(shape: &cadrs_sketch::ImprintShape) -> cadrs_sketch::Vec2 {
    use cadrs_sketch::ImprintShape;
    match *shape {
        ImprintShape::Line(a, b) => cadrs_sketch::Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
        ImprintShape::Circle(c, r) => cadrs_sketch::Vec2::new(c.x + r, c.y),
        ImprintShape::Arc { center, radius, start_angle, sweep } => {
            let t = start_angle + sweep / 2.0;
            cadrs_sketch::Vec2::new(center.x + radius * t.cos(), center.y + radius * t.sin())
        }
    }
}

/// Whether `p` lies on one of the closed polylines `loops`.
fn on_loops(loops: &[Vec<cadrs_sketch::Vec2>], p: cadrs_sketch::Vec2) -> bool {
    loops.iter().any(|l| {
        (0..l.len()).any(|i| {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            imprint_distance(&cadrs_sketch::ImprintShape::Line(a, b), p) < 1e-2
        })
    })
}

/// How far `p` is from an imprinted edge's curve.
fn imprint_distance(shape: &cadrs_sketch::ImprintShape, p: cadrs_sketch::Vec2) -> f64 {
    use cadrs_sketch::ImprintShape;
    match *shape {
        ImprintShape::Line(a, b) => {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = (dx * dx + dy * dy).max(1e-18);
            let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
            p.distance(cadrs_sketch::Vec2::new(a.x + t * dx, a.y + t * dy))
        }
        ImprintShape::Circle(c, r) | ImprintShape::Arc { center: c, radius: r, .. } => (p.distance(c) - r).abs(),
    }
}

/// A sketch plane on face `fi` of `part` as the model is now (the app's face plane: the
/// feature that made the face if it is a part feature, else the part's; the face's frame and a
/// point on it). Unlike `parts::face_plane` it doesn't rebuild up to that feature, so a face a
/// later feature split (and renamed) still works.
fn plane_on(features: &[cadrs_core::document::Feature], part: &Part, fi: usize) -> Option<PlaneRef> {
    let face = &part.solid.faces[fi];
    let frame = face.plane?;
    let maker = FeatureId(face.name.op);
    let feature = if features.iter().any(|f| f.id == maker && f.is_part_feature()) { maker } else { part.id.feature };
    Some(PlaneRef::Face(cadrs_sketch::FacePlane {
        feature: feature.0,
        face: face.name,
        origin: frame.origin,
        u: frame.u,
        v: frame.v,
        seed: part.solid.face_point(fi),
    }))
}

/// The total volume of the solid parts (mm³).
fn total_volume(parts: &[Part]) -> f64 {
    parts.iter().map(|p| p.mass.as_ref().map(|m| m.volume).unwrap_or_else(|| p.solid.volume())).sum()
}

/// How many of `pts` (world, near the face's plane) lie inside face `fi` of `part` (its
/// boundary loops, even-odd), seen along its normal.
fn points_on_face(part: &Part, fi: usize, pts: &[[f64; 3]]) -> usize {
    let face = &part.solid.faces[fi];
    let Some(frame) = face.plane else { return 0 };
    let to2 = |p: &[f64; 3]| {
        let d = [p[0] - frame.origin[0], p[1] - frame.origin[1], p[2] - frame.origin[2]];
        (dot(d, frame.u), dot(d, frame.v))
    };
    let loops: Vec<Vec<(f64, f64)>> = face.loops.iter().map(|l| l.iter().map(to2).collect()).collect();
    pts.iter()
        .filter(|p| {
            let (x, y) = to2(p);
            let mut inside = false;
            for l in &loops {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1) {
                        inside = !inside;
                    }
                }
            }
            inside
        })
        .count()
}

/// Whether sketch plane `p` faces along `n` and lies within `tol` mm of `o`.
fn plane_near(p: &PlaneRef, n: [f64; 3], o: [f64; 3], tol: f64) -> bool {
    let f = p.frame();
    let fnrm = cross(f.u, f.v);
    let d = [o[0] - f.origin[0], o[1] - f.origin[1], o[2] - f.origin[2]];
    dot(fnrm, n) > 1.0 - 1e-6 && dot(d, fnrm).abs() < tol
}

/// Whether sketch plane `p` is the plane through `o` with normal `n` (world, mm).
fn plane_matches(p: &PlaneRef, n: [f64; 3], o: [f64; 3]) -> bool {
    let f = p.frame();
    let fnrm = cross(f.u, f.v);
    let d = [o[0] - f.origin[0], o[1] - f.origin[1], o[2] - f.origin[2]];
    dot(fnrm, n) > 1.0 - 1e-6 && dot(d, fnrm).abs() < 1e-4
}

/// The bounding box (min xyz, max xyz) of points.
fn bbox_of(points: impl Iterator<Item = [f64; 3]>) -> Option<[f64; 6]> {
    let mut b: Option<[f64; 6]> = None;
    for p in points {
        let x = b.get_or_insert([p[0], p[1], p[2], p[0], p[1], p[2]]);
        for k in 0..3 {
            x[k] = x[k].min(p[k]);
            x[k + 3] = x[k + 3].max(p[k]);
        }
    }
    b
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions_with_variables() {
        let vars: HashMap<String, Var> = [
            ("padding", "LENGTH", "5 mm"),
            ("count", "NUMBER", "5"),
            ("pitch", "ANY", "((42mm*2)-((#padding+#padding)*2))/(#count-1)"),
        ]
        .into_iter()
        .map(|(n, k, e)| (n.to_string(), Var { kind: k.into(), expr: e.into() }))
        .collect();
        assert_eq!(eval_expr("25 mm", Quantity::Length, &vars), Some(25.0));
        assert_eq!(eval_expr("#padding * 2", Quantity::Length, &vars), Some(10.0));
        assert_eq!(eval_expr("#pitch", Quantity::Length, &vars), Some(16.0));
        assert_eq!(eval_expr("#missing", Quantity::Length, &vars), None);
        assert_eq!(eval_expr("9.6*mm", Quantity::Length, &vars), Some(9.6));
        assert_eq!(eval_expr("2 * inch", Quantity::Length, &vars), Some(50.8));
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(Some("1970-01-01T00:00:00Z")), Some(0));
        assert_eq!(timestamp(Some("2026-09-24T15:30:00.000+00:00".replace("+00:00", "Z").as_str())), Some(1_790_263_800));
    }
}
