//! Onshape geometry references (queries) to cadrs references.
//!
//! Onshape names an entity by its history (`query`): what operation made it and from what
//! (sketch entities, other faces). cadrs names faces the same way (`cadrs_kernel::naming`):
//! a cap of a region (the region keyed by its sketch and boundary curves), the side swept by a
//! sketch curve. So a reference is translated by reading what the query says it was made from,
//! mapping Onshape's feature and sketch entity ids to cadrs's, and finding the face in the
//! cadrs model (rebuilt up to the feature that refers to it) with that history.

use std::collections::{BTreeSet, HashMap};

use cadrs_core::document::{FaceRef, RegionRef};
use cadrs_core::ids::{FeatureId, PartId};
use cadrs_core::parts::Part;
use cadrs_kernel::naming::FaceOrigin;
use cadrs_sketch::region::regions_shared;
use cadrs_sketch::{CurveId, Sketch, Vec3};
use serde_json::Value as Json;

use crate::query::{self, Value};
use crate::sketch::SketchMap;

/// A decoded query parameter: the query tree, or for `qSketchRegion(id + "<feature>")`, the
/// whole sketch.
#[derive(Debug, Clone)]
pub enum Pick {
    Query(Value),
    /// Every region of this Onshape sketch feature.
    WholeSketch(String),
    /// Only deterministic ids (no query string): not resolvable offline.
    Opaque(Vec<String>),
}

impl Pick {
    /// The default plane this pick is, if it is one (by deterministic id or by query:
    /// `Top.planeOp`, …).
    pub fn default_plane(&self) -> Option<cadrs_sketch::PlaneRef> {
        use cadrs_sketch::PlaneRef;
        let name = match self {
            Pick::Opaque(ids) => match ids.first().map(String::as_str) {
                Some(crate::sketch::TOP_ID) => "Top",
                Some(crate::sketch::FRONT_ID) => "Front",
                Some(crate::sketch::RIGHT_ID) => "Right",
                _ => return None,
            },
            Pick::Query(q) => q.get("operationId").and_then(Value::as_str)?.strip_suffix(".planeOp")?,
            Pick::WholeSketch(_) => return None,
        };
        match name {
            "Top" => Some(PlaneRef::Top),
            "Front" => Some(PlaneRef::Front),
            "Right" => Some(PlaneRef::Right),
            _ => None,
        }
    }

    /// Whether this pick is the Part Studio's origin.
    pub fn is_origin(&self) -> bool {
        match self {
            Pick::Opaque(ids) => ids.first().map(String::as_str) == Some(crate::sketch::ORIGIN_ID),
            Pick::Query(q) => q.get("operationId").and_then(Value::as_str).is_some_and(|o| o.starts_with("Origin.")),
            Pick::WholeSketch(_) => false,
        }
    }
}

/// The picks of a feature parameter.
pub fn picks(param: Option<&Json>) -> Vec<Pick> {
    let Some(p) = param else { return Vec::new() };
    p["queries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| {
            let ids = q["deterministicIds"].as_array().into_iter().flatten().filter_map(Json::as_str).map(String::from).collect();
            let Some(s) = q["queryString"].as_str() else { return Pick::Opaque(ids) };
            if let Some(i) = s.find("qSketchRegion(id + \"") {
                let rest = &s[i + "qSketchRegion(id + \"".len()..];
                if let Some(end) = rest.find('"') {
                    return Pick::WholeSketch(rest[..end].to_string());
                }
            }
            match query::decode(s) {
                Ok(Some(v)) => Pick::Query(v),
                _ => Pick::Opaque(ids),
            }
        })
        .collect()
}

/// The Onshape feature id an operation id belongs to (`"FmJm…_0.opExtrude"` → `"FmJm…_0"`).
pub fn op_feature(op: &str) -> &str {
    op.split('.').next().unwrap_or(op)
}

fn op_of(q: &Value) -> Option<&str> {
    q.get("operationId")?.as_str()
}

/// Every sketch entity id of sketch `sketch` the query mentions anywhere.
pub fn sketch_entities(q: &Value, sketch: &str, out: &mut BTreeSet<String>) {
    match q {
        Value::Typed(_, v) => sketch_entities(v, sketch, out),
        Value::Map(m) => {
            let is_entity = m.iter().any(|(k, v)| k == "queryType" && v.as_str() == Some("SKETCH_ENTITY"));
            if is_entity
                && let (Some(op), Some(id)) = (m.iter().find(|(k, _)| k == "operationId").and_then(|(_, v)| v.as_str()), m.iter().find(|(k, _)| k == "sketchEntityId").and_then(|(_, v)| v.as_str()))
                && op_feature(op) == sketch
            {
                out.insert(id.to_string());
            }
            for (_, v) in m {
                sketch_entities(v, sketch, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|v| sketch_entities(v, sketch, out)),
        _ => {}
    }
}

/// Every Onshape sketch feature id the query mentions as a sketch-entity source.
pub fn sketches_in(q: &Value, out: &mut BTreeSet<String>) {
    match q {
        Value::Typed(_, v) => sketches_in(v, out),
        Value::Map(m) => {
            if m.iter().any(|(k, v)| k == "queryType" && v.as_str() == Some("SKETCH_ENTITY"))
                && let Some(op) = m.iter().find(|(k, _)| k == "operationId").and_then(|(_, v)| v.as_str())
            {
                out.insert(op_feature(op).to_string());
            }
            for (_, v) in m {
                sketches_in(v, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|v| sketches_in(v, out)),
        _ => {}
    }
}

/// The cadrs curves of Onshape sketch entities (unknown ones left out).
fn curves_of(ids: &BTreeSet<String>, map: &SketchMap) -> BTreeSet<CurveId> {
    ids.iter().filter_map(|i| map.curves.get(i).copied()).collect()
}

/// The region of sketch `g` that `curves` describe: the one whose boundary (outer or holes)
/// has the most of them, then the fewest others, then the smallest. Onshape's region queries
/// list the edges around a region but also neighbouring ones (construction lines, the edges of
/// a point's curves), so not every curve named is on the boundary. With no curves named, the
/// sketch's only region, if it has one.
pub fn region_with(sketch: FeatureId, g: &Sketch, curves: &BTreeSet<CurveId>) -> Option<RegionRef> {
    let all = regions_shared(g);
    if curves.is_empty() {
        return (all.len() == 1).then(|| RegionRef::new(sketch, &all[0]));
    }
    let boundary = |r: &cadrs_sketch::region::Region| -> BTreeSet<CurveId> { r.curves.iter().chain(r.hole_curves.iter().flatten()).copied().collect() };
    all.iter()
        .map(|r| {
            let b = boundary(r);
            let hits = curves.intersection(&b).count();
            (r, hits, b.len() - hits)
        })
        .filter(|(_, hits, _)| *hits > 0)
        .max_by(|(a, ha, xa), (b, hb, xb)| ha.cmp(hb).then(xb.cmp(xa)).then(region_area(b).total_cmp(&region_area(a))))
        .map(|(r, _, _)| RegionRef::new(sketch, r))
}

/// How many of `sides` (a sketch line and ±1) region `r` agrees with, if +1 means the region
/// is on the line's left (going from its start to its end), and if it means the right.
pub fn side_agreement(g: &Sketch, r: &cadrs_sketch::region::Region, sides: &[(CurveId, f64)]) -> (usize, usize) {
    let (mut left, mut right) = (0, 0);
    for (c, sign) in sides {
        let Some(on_left) = side_of(g, r, *c) else { continue };
        let plus_left = *sign > 0.0;
        if on_left == plus_left {
            left += 1;
        } else {
            right += 1;
        }
    }
    (left, right)
}

/// Whether region `r` lies on the left of line `c` (at its middle), if `c` is a line on its
/// boundary.
fn side_of(g: &Sketch, r: &cadrs_sketch::region::Region, c: CurveId) -> Option<bool> {
    let pieces = || r.outer_pieces.iter().zip(&r.curves).chain(r.hole_pieces.iter().flatten().zip(r.hole_piece_curves.iter().flatten()));
    let (a, b) = match g.curves.get(c)?.kind {
        cadrs_sketch::CurveKind::Line { a, b } => (a, b),
        // A circle or arc runs counter-clockwise (as Onshape's do, and cadrs stores arcs): its
        // left is towards its centre. Tested beside the middle of the region's piece of it.
        cadrs_sketch::CurveKind::Circle { center, .. } | cadrs_sketch::CurveKind::Arc { center, .. } => {
            let ctr = g.points.get(center)?.pos;
            let m = pieces().filter(|(_, pc)| **pc == c).find_map(|(p, _)| match p {
                cadrs_sketch::region::Piece::Arc(a) => Some(a.mid()),
                _ => None,
            })?;
            let to = ctr - m;
            let len = to.length();
            if len < 1e-9 {
                return None;
            }
            let eps = (len * 1e-3).min(1e-2);
            let inward = r.contains(m + to * (eps / len));
            let outward = r.contains(m - to * (eps / len));
            return (inward != outward).then_some(inward);
        }
        cadrs_sketch::CurveKind::Spline { .. } => return spline_side_of(g, r, c),
        _ => return None,
    };
    let (a, b) = (g.points.get(a)?.pos, g.points.get(b)?.pos);
    let d = b - a;
    let len = d.length();
    if len < 1e-9 {
        return None;
    }
    let n = cadrs_sketch::Vec2::new(-d.y / len, d.x / len);
    // Test beside the part of the line the region is bounded by (a line can run past it or be
    // split by other curves), else beside the line's middle.
    let pieces = r.outer_pieces.iter().zip(&r.curves).chain(r.hole_pieces.iter().flatten().zip(r.hole_piece_curves.iter().flatten()));
    let m = pieces
        .filter(|(_, pc)| **pc == c)
        .map(|(p, _)| (p.start(), p.end()))
        .max_by(|x, y| x.0.distance(x.1).total_cmp(&y.0.distance(y.1)))
        .map(|(p, q)| cadrs_sketch::Vec2::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0))
        .unwrap_or_else(|| cadrs_sketch::Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0));
    let eps = (len * 1e-3).min(1e-2);
    let l = r.contains(m + n * eps);
    let rt = r.contains(m - n * eps);
    (l != rt).then_some(l)
}

/// Whether region `r` lies on the left of spline `c` (seen along the spline's direction), at
/// the middle of the longest piece of its boundary on the spline.
fn spline_side_of(g: &Sketch, r: &cadrs_sketch::region::Region, c: CurveId) -> Option<bool> {
    use cadrs_sketch::spline;
    let spans = g.spline_spans(c)?;
    let pieces = r.outer_pieces.iter().zip(&r.curves).chain(r.hole_pieces.iter().flatten().zip(r.hole_piece_curves.iter().flatten()));
    let piece = pieces.filter(|(_, pc)| **pc == c).map(|(p, _)| *p).max_by(|x, y| x.start().distance(x.end()).total_cmp(&y.start().distance(y.end())))?;
    let m = match piece {
        cadrs_sketch::region::Piece::Bezier(b) => b.point_at(0.5),
        p => p.start().midpoint(p.end()),
    };
    let (i, t, _) = spline::nearest(&spans, m)?;
    let d = spline::bez_tangent(&spans[i], t);
    let n = d.perp();
    let size = spans.iter().map(|b| b[0].distance(b[3])).sum::<f64>();
    let eps = (size * 1e-4).min(1e-2);
    let l = r.contains(m + n * eps);
    let rt = r.contains(m - n * eps);
    (l != rt).then_some(l)
}

/// The sketch entity a region query's boundary entry is itself (a sketch edge, or the imprint of
/// one), not those its own disambiguation mentions.
pub fn own_sketch_entity(q: &Value, sketch: &str) -> Option<String> {
    let of = |v: &Value| -> Option<String> {
        if v.get("queryType")?.as_str()? == "SKETCH_ENTITY" && op_feature(v.get("operationId")?.as_str()?) == sketch {
            v.get("sketchEntityId")?.as_str().map(String::from)
        } else {
            None
        }
    };
    if let Some(id) = of(q) {
        return Some(id);
    }
    match q.get("derivedFrom")? {
        Value::Array(a) if a.len() == 1 => of(&a[0]),
        Value::Array(_) => None,
        v => of(v),
    }
}

/// [`region_with`], ranking candidates first by the side of each named line they are on (+1:
/// its left, going from its start to its end), then by how many of the curves the region's
/// own boundary entries name (`direct`) they have, then by all named curves. A candidate must
/// be on the named side of at least as many named lines on its boundary as it is on the other
/// side of, and have at least half the direct
/// curves (or, failing that, half of all named curves): with none such, the face Onshape means
/// isn't one of cadrs's regions (it can be one Onshape splits differently), and nothing is
/// returned rather than a wrong region.
pub fn region_with_sides(sketch: FeatureId, g: &Sketch, curves: &BTreeSet<CurveId>, direct: &BTreeSet<CurveId>, sides: &[(CurveId, f64)], entries: usize) -> Option<RegionRef> {
    if (sides.is_empty() && direct.is_empty()) || curves.is_empty() {
        return region_with(sketch, g, curves);
    }
    let all = regions_shared(g);
    let boundary = |r: &cadrs_sketch::region::Region| -> BTreeSet<CurveId> { r.curves.iter().chain(r.hole_curves.iter().flatten()).copied().collect() };
    if std::env::var_os("CADRS_ONSHAPE_DEBUG_REGIONS").is_some() {
        for (i, r) in all.iter().enumerate() {
            let b = boundary(r);
            let (agree, disagree) = side_agreement(g, r, sides);
            eprintln!(
                "   cand {i}: area {:.1} boundary {} hits {}/{} direct {}/{} sides +{agree} -{disagree}",
                region_area(r),
                b.len(),
                curves.intersection(&b).count(),
                curves.len(),
                direct.intersection(&b).count(),
                direct.len()
            );
        }
    }
    all.iter()
        .filter_map(|r| {
            let b = boundary(r);
            let hits = curves.intersection(&b).count();
            let (agree, disagree) = side_agreement(g, r, sides);
            let direct_hits = direct.intersection(&b).count();
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_CANDS").is_some() {
                eprintln!("CAND boundary {} curves ({} imprinted), hits {hits}/{}, direct {direct_hits}/{}, sides +{agree} -{disagree}, area {:.2}", b.len(), b.iter().filter(|c| cadrs_sketch::is_synthetic(**c)).count(), curves.len(), direct.len(), r.area());
            }
            // Tier 1: half the direct curves; tier 0: half of all named curves (the direct ones
            // may not bound any cadrs region, imprinted face edges do).
            let tier = if 2 * direct_hits >= direct.len() {
                1
            } else if 2 * hits >= curves.len() {
                0
            } else {
                return None;
            };
            // Sides are a vote: a line cadrs split or runs the other way can disagree on its own.
            // Onshape lists every edge around the region: a boundary of that many edges fits.
            let size_fit = entries > 0 && b.len() == entries;
            (hits > 0 && disagree <= agree).then_some((r, tier, agree as i64 - disagree as i64, size_fit, direct_hits, hits, b.len() - hits))
        })
        .max_by(|(a, ta, sa, fa, da, ha, xa), (b, tb, sb, fb, db, hb, xb)| {
            ta.cmp(tb).then(sa.cmp(sb)).then(fa.cmp(fb)).then(da.cmp(db)).then(ha.cmp(hb)).then(xb.cmp(xa)).then(region_area(b).total_cmp(&region_area(a)))
        })
        .map(|(r, ..)| RegionRef::new(sketch, r))
        // Every candidate on the wrong side of the named lines: the sides can't be trusted here
        // (a sketch line along an edge of the face it is drawn on is named by the imprinted
        // edge, whose direction follows the face's loop), so choose without them.
        .or_else(|| if sides.is_empty() { None } else { region_with_sides(sketch, g, curves, direct, &[], entries) })
}

/// When no region of `g` has every one of `direct` (the sketch curves a query names) on its
/// boundary but several regions are bounded only by them and imprinted face edges, and
/// together have them all: those regions (Onshape's one region, which cadrs cuts with the face
/// edges the sketch lies across). Otherwise empty.
pub fn pieces_bounded_by(sketch: FeatureId, g: &Sketch, direct: &BTreeSet<CurveId>) -> Vec<RegionRef> {
    if direct.len() < 3 {
        return Vec::new();
    }
    let all = regions_shared(g);
    let drawn = |r: &cadrs_sketch::region::Region| -> BTreeSet<CurveId> {
        r.curves.iter().chain(r.hole_curves.iter().flatten()).copied().filter(|c| !cadrs_sketch::is_synthetic(*c)).collect()
    };
    if all.iter().any(|r| direct.is_subset(&drawn(r))) {
        return Vec::new();
    }
    let pieces: Vec<&cadrs_sketch::region::Region> = all.iter().filter(|r| {
        let d = drawn(r);
        !d.is_empty() && d.is_subset(direct) && r.curves.iter().chain(r.hole_curves.iter().flatten()).any(|c| cadrs_sketch::is_synthetic(*c))
    }).collect();
    let covered: BTreeSet<CurveId> = pieces.iter().flat_map(|r| drawn(r)).collect();
    if pieces.len() > 1 && covered == *direct {
        pieces.into_iter().map(|r| RegionRef::new(sketch, r)).collect()
    } else {
        Vec::new()
    }
}

fn region_area(r: &cadrs_sketch::region::Region) -> f64 {
    r.area().abs()
}

/// A sketch-region pick (an extrude's "entities"): the face the sketch's imprint made, named
/// by its adjacent sketch edges.
pub fn region_of(q: &Value, sketches: &HashMap<String, (SketchMap, Sketch)>) -> Option<RegionRef> {
    let op = op_of(q)?;
    let sketch = op_feature(op);
    let (map, g) = sketches.get(sketch)?;
    let mut ids = BTreeSet::new();
    sketch_entities(q, sketch, &mut ids);
    region_with(map.feature, g, &curves_of(&ids, map))
}

/// A face as the query describes it, to look up in the cadrs model.
#[derive(Debug, Clone)]
pub struct FacePattern {
    /// The cadrs feature that made it.
    pub op: uuid::Uuid,
    pub kind: PatternKind,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    /// A cap: `end` (far end) or start; the region's boundary curves if known.
    Cap { end: bool, region: Option<u64> },
    /// The side swept by one of these curves.
    Side { curves: BTreeSet<u64> },
}

/// The face pattern a face query describes, if it is one we can translate. Faces a later
/// operation merged or split keep their origin's pattern (cadrs keeps the name of a face a
/// boolean only trims, and numbers the pieces of a split one).
pub fn face_pattern(q: &Value, features: &HashMap<String, FeatureId>, sketches: &HashMap<String, (SketchMap, Sketch)>) -> Option<FacePattern> {
    face_patterns(q, features, sketches).into_iter().next()
}

/// Every face pattern the query could mean (a merge of several faces gives each).
pub fn face_patterns(q: &Value, features: &HashMap<String, FeatureId>, sketches: &HashMap<String, (SketchMap, Sketch)>) -> Vec<FacePattern> {
    if q.get("entityType").and_then(Value::as_str) != Some("FACE") {
        return Vec::new();
    }
    let Some(qtype) = q.get("queryType").and_then(Value::as_str) else { return Vec::new() };
    if matches!(qtype, "MERGE" | "SPLIT") {
        return derived(q).iter().flat_map(|d| face_patterns(d, features, sketches)).collect();
    }
    let Some(op) = op_of(q) else { return Vec::new() };
    let Some(&feature) = features.get(op_feature(op)) else { return Vec::new() };
    let (curves, region) = sources(q, sketches);
    let kind = match qtype {
        "CAP_FACE" => PatternKind::Cap { end: !q.get("isStart").and_then(Value::as_bool).unwrap_or(true), region },
        "SWEPT_FACE" => PatternKind::Side { curves },
        _ => return Vec::new(),
    };
    vec![FacePattern { op: feature.0, kind }]
}

/// What a query was derived from (`derivedFrom`, one or several).
fn derived(q: &Value) -> Vec<&Value> {
    match q.get("derivedFrom") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v) => vec![v],
        None => Vec::new(),
    }
}

/// The cadrs curve sources (as face names hold them) of the sketch entities a query
/// mentions, and the region they bound, if they bound one.
fn sources(q: &Value, sketches: &HashMap<String, (SketchMap, Sketch)>) -> (BTreeSet<u64>, Option<u64>) {
    let mut srcs = BTreeSet::new();
    sketches_in(q, &mut srcs);
    let mut curves = BTreeSet::new();
    let mut region = None;
    for s in &srcs {
        if let Some((map, g)) = sketches.get(s) {
            let mut ids = BTreeSet::new();
            sketch_entities(q, s, &mut ids);
            let cs = curves_of(&ids, map);
            curves.extend(cs.iter().map(|c| cadrs_core::brep::curve_source(*c)));
            if region.is_none() {
                region = region_with(map.feature, g, &cs).map(|r| r.key());
            }
        }
    }
    (curves, region)
}

/// An edge as the query describes it: the faces on its two sides.
#[derive(Debug, Clone)]
pub struct EdgePattern {
    pub a: Vec<FacePattern>,
    pub b: Vec<FacePattern>,
}

/// The edge pattern an edge query describes, if it is one we can translate.
pub fn edge_pattern(q: &Value, features: &HashMap<String, FeatureId>, sketches: &HashMap<String, (SketchMap, Sketch)>) -> Option<EdgePattern> {
    if q.get("entityType")?.as_str()? != "EDGE" {
        return None;
    }
    let qtype = q.get("queryType")?.as_str()?;
    match qtype {
        "MERGE" | "SPLIT" => derived(q).into_iter().find_map(|d| edge_pattern(d, features, sketches)),
        "INTERSECT" => {
            let faces: Vec<Vec<FacePattern>> = derived(q).into_iter().map(|d| face_patterns(d, features, sketches)).collect();
            match faces.as_slice() {
                [a, b] if !a.is_empty() && !b.is_empty() => Some(EdgePattern { a: a.clone(), b: b.clone() }),
                _ => None,
            }
        }
        "CAP_EDGE" | "SWEPT_EDGE" => {
            let op = features.get(op_feature(op_of(q)?))?.0;
            let (curves, _) = sources(q, sketches);
            let side = |c: u64| FacePattern { op, kind: PatternKind::Side { curves: BTreeSet::from([c]) } };
            if qtype == "CAP_EDGE" {
                let end = !q.get("isStart").and_then(Value::as_bool).unwrap_or(true);
                Some(EdgePattern {
                    a: vec![FacePattern { op, kind: PatternKind::Cap { end, region: None } }],
                    b: curves.iter().map(|c| side(*c)).collect(),
                })
            } else {
                let mut it = curves.iter();
                let (c1, c2) = (*it.next()?, *it.next()?);
                Some(EdgePattern { a: vec![side(c1)], b: vec![side(c2)] })
            }
        }
        _ => None,
    }
}

fn matches(f: &cadrs_kernel::naming::FaceName, pats: &[FacePattern]) -> bool {
    pats.iter().any(|p| {
        p.op == f.op
            && match (&p.kind, f.origin) {
                (PatternKind::Cap { end, region }, FaceOrigin::Cap { region: r, end: e }) => *end == e && region.is_none_or(|x| x == r),
                (PatternKind::Side { curves }, FaceOrigin::Side { curve, .. }) => curves.contains(&curve),
                _ => false,
            }
    })
}

/// The face of `parts` that matches `pat`: its part, name and a point on it. A cap whose region
/// is known must be that region's; otherwise any cap at that end of the operation will do.
pub fn find_face(parts: &[Part], pat: &FacePattern) -> Option<FaceRef> {
    let exact = |f: &cadrs_kernel::naming::FaceName| matches(f, std::slice::from_ref(pat));
    let loose = |f: &cadrs_kernel::naming::FaceName| match &pat.kind {
        PatternKind::Cap { end, region: Some(_) } => {
            let any = FacePattern { op: pat.op, kind: PatternKind::Cap { end: *end, region: None } };
            matches(f, std::slice::from_ref(&any))
        }
        _ => false,
    };
    for test in [&exact as &dyn Fn(&cadrs_kernel::naming::FaceName) -> bool, &loose] {
        for part in parts {
            if let Some(i) = part.solid.faces.iter().position(|f| test(&f.name)) {
                let seed: Vec3 = part.solid.face_point(i).unwrap_or([0.0; 3]);
                return Some(FaceRef { part: part.id, face: part.solid.faces[i].name, seed });
            }
        }
    }
    None
}

/// The edge of `parts` between faces matching the pattern's two sides.
pub fn find_edge(parts: &[Part], pat: &EdgePattern) -> Option<cadrs_core::document::EdgeRef> {
    for part in parts {
        for e in &part.solid.edges {
            let [f, g] = e.name.faces;
            if (matches(&f, &pat.a) && matches(&g, &pat.b)) || (matches(&g, &pat.a) && matches(&f, &pat.b)) {
                let seed = e.points.get(e.points.len() / 2).copied().unwrap_or([0.0; 3]);
                return Some(cadrs_core::document::EdgeRef { part: part.id, edge: e.name, seed });
            }
        }
    }
    None
}

/// The parts a body query names (the bodies an operation made).
pub fn parts_of(q: &Value, features: &HashMap<String, FeatureId>, parts: &[Part]) -> Vec<PartId> {
    let Some(op) = op_of(q) else { return Vec::new() };
    let Some(f) = features.get(op_feature(op)) else { return Vec::new() };
    parts.iter().filter(|p| p.id.feature == *f).map(|p| p.id).collect()
}
