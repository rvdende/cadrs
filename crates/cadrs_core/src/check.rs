//! Checking a document without the app (`cadrs --document <name> --check`): rebuilds each Part
//! Studio and lists what its feature list would show in error or warning, and why, in more
//! detail than a tooltip has room for.
//!
//! A feature is in error when its parameters can't be built ([`Feature::problem`]), when the
//! rebuild fails it, or, for a sketch, when its constraints conflict, a Use or Pierce reference
//! is broken, or the face it lies on is gone; the same tests the feature list colours rows red
//! by. A warning is the rebuild's yellow state.

use std::time::{Duration, Instant};

use crate::document::{Document, ElementKind, Feature};

/// How bad an issue is: the feature list's red or yellow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One thing wrong with one feature.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub feature: String,
    pub severity: Severity,
    /// Why, and what exactly (which constraints, which inputs).
    pub detail: String,
}

/// A Part Studio's check.
#[derive(Debug, Clone)]
pub struct StudioCheck {
    pub name: String,
    pub features: usize,
    pub parts: usize,
    pub elapsed: Duration,
    pub issues: Vec<Issue>,
    /// Each sketch and the degrees of freedom it has left (0: fully defined).
    pub sketch_dof: Vec<(String, usize)>,
}

/// Rebuilds every Part Studio of `doc` (its features above the rollback bar), or only the one
/// named `studio`, and lists the features in error or warning.
pub fn check(doc: &Document, studio: Option<&str>) -> Vec<StudioCheck> {
    doc.elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .filter(|e| studio.is_none_or(|s| e.name.eq_ignore_ascii_case(s)))
        .map(|e| check_features(&e.name, &e.active_features()))
        .collect()
}

/// [`check`] for one feature list.
pub fn check_features(name: &str, features: &[Feature]) -> StudioCheck {
    let started = Instant::now();
    let build = crate::rebuild::build(features);
    let mut issues = Vec::new();
    let mut sketch_dof = Vec::new();
    let mut add = |f: &Feature, severity, detail: String| issues.push(Issue { feature: f.name.clone(), severity, detail });
    for (i, f) in features.iter().enumerate() {
        if let Some(p) = f.problem() {
            add(f, Severity::Error, format!("can't be built: {p}"));
        }
        if let Some(e) = build.error(f.id) {
            add(f, Severity::Error, format!("rebuild failed: {e}"));
        }
        if let Some(w) = build.warning(f.id) {
            add(f, Severity::Warning, w.to_string());
        }
        let missing = build.missing_inputs(f.id);
        if !missing.is_empty() {
            let at: Vec<String> = missing.iter().map(|i| (i + 1).to_string()).collect();
            add(f, Severity::Warning, format!("inputs that no longer resolve: selection {}", at.join(", ")));
        }
        let Some(sk) = f.sketch() else { continue };
        let g = &sk.geometry;
        if !g.broken.is_empty() {
            let kinds: Vec<String> = g.broken.iter().filter_map(|c| g.constraints.get(*c)).map(variant).collect();
            add(f, Severity::Error, format!("{} broken reference(s) (their source is gone): {}", g.broken.len(), counted(&kinds)));
        }
        let a = cadrs_sketch::solve::analyze(g);
        sketch_dof.push((f.name.clone(), a.dof));
        if a.has_conflicts() {
            let mut kinds: Vec<String> = a.conflicting.iter().filter_map(|c| g.constraints.get(*c)).map(variant).collect();
            kinds.extend(a.conflicting_dimensions.iter().filter_map(|d| g.dimensions.get(*d)).map(|d| format!("{:?} dimension {}", d.kind, d.value)));
            let mut detail = format!("conflicting constraints: {}", counted(&kinds));
            for c in &a.conflicting {
                detail.push_str(&format!("\n             - {}", describe(g, *c)));
            }
            add(f, Severity::Error, detail);
        }
        if sk.plane.is_some() && crate::parts::sketch_face_lost_in(features, i, &build.parts) {
            add(f, Severity::Error, "the face this sketch is on no longer exists".into());
        }
    }
    StudioCheck { name: name.to_string(), features: features.len(), parts: build.parts.len(), elapsed: started.elapsed(), issues, sketch_dof }
}

/// A part feature's name and the parts after it: name, volume (mm³), centre of mass (mm).
pub type Step = (String, Vec<(String, f64, Option<[f64; 3]>)>);

/// Each part feature of a Part Studio and the parts after it, by name, volume (mm³) and centre
/// of mass (mm): its model built step by step, to compare with another CAD's rolled back to the
/// same feature (`cadrs --check --steps`).
pub fn steps(features: &[Feature]) -> Vec<Step> {
    let mut out = Vec::new();
    for n in 1..=features.len() {
        let f = &features[n - 1];
        if !f.is_part_feature() {
            continue;
        }
        let b = crate::rebuild::build(&features[..n]);
        // A surface has no volume (even a closed one), as Onshape's mass properties report.
        let mut parts: Vec<(String, f64, Option<[f64; 3]>)> = b
            .parts
            .iter()
            .map(|p| if p.kind == crate::parts::PartKind::Solid { (p.name.clone(), p.solid.volume(), p.solid.centroid()) } else { (p.name.clone(), 0.0, None) })
            .collect();
        parts.sort_by(|a, b| b.1.total_cmp(&a.1));
        out.push((f.name.clone(), parts));
    }
    out
}

/// The Part Studio of `doc` named `studio` (any case), or its only one / first one.
pub fn studio_features(doc: &Document, studio: Option<&str>) -> Option<(String, Vec<Feature>)> {
    doc.elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .find(|e| studio.is_none_or(|s| e.name.eq_ignore_ascii_case(s)))
        .map(|e| (e.name.clone(), e.active_features()))
}

/// The feature named `name` of a Part Studio of `doc` written out (`cadrs --check --feature
/// <name>`): its parameters, and for a mirror on a part face, where that face is.
pub fn describe_feature(doc: &Document, studio: Option<&str>, name: &str) -> Result<String, String> {
    use std::fmt::Write;
    let (sname, features) = studio_features(doc, studio).ok_or("no such Part Studio")?;
    let at = features.iter().position(|f| f.name == name).ok_or_else(|| format!("no feature \"{name}\" in {sname}"))?;
    let f = &features[at];
    let mut out = String::new();
    let text = format!("{:?}", f.kind);
    let _ = writeln!(out, "{name} in {sname}:\n  {}", if text.len() > 4000 { format!("{}…", &text[..4000]) } else { text });
    // The parts after it, with their volumes and bounding boxes (of their edges).
    if f.is_part_feature() {
        let b = crate::rebuild::build(&features[..=at]);
        for p in &b.parts {
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for q in p.solid.edges.iter().flat_map(|e| e.points.iter()) {
                for i in 0..3 {
                    lo[i] = lo[i].min(q[i]);
                    hi[i] = hi[i].max(q[i]);
                }
            }
            let r = |v: [f64; 3]| format!("{:.2} {:.2} {:.2}", v[0], v[1], v[2]);
            let _ = writeln!(out, "  after it: {} V {:.1} min {} max {}", p.name, p.solid.volume(), r(lo), r(hi));
        }
    }
    // A fillet's or chamfer's edges, where they are in the parts before it.
    let edges: Vec<crate::document::EdgeRef> = match &f.kind {
        crate::document::FeatureKind::Fillet(x) => x.entities.iter().filter_map(|e| match e {
            crate::applied::EdgeOrFace::Edge(r) => Some(*r),
            _ => None,
        }).collect(),
        crate::document::FeatureKind::Chamfer(x) => x.entities.iter().filter_map(|e| match e {
            crate::applied::EdgeOrFace::Edge(r) => Some(*r),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    };
    if !edges.is_empty() {
        let b = crate::rebuild::build(&features[..at]);
        for r in &edges {
            match b.parts.iter().find_map(|p| p.solid.edge(&r.edge).map(|e| (p, e))) {
                Some((p, e)) => {
                    let (a, z) = (e.points.first().copied().unwrap_or_default(), e.points.last().copied().unwrap_or_default());
                    let r3 = |v: [f64; 3]| format!("({:.3} {:.3} {:.3})", v[0], v[1], v[2]);
                    let _ = writeln!(out, "  edge on {}: {} → {}, {} points{}", p.name, r3(a), r3(z), e.points.len(), if e.circle.is_some() { ", circular" } else { "" });
                    for n in r.edge.faces {
                        let _ = writeln!(out, "    face {:?}", n.origin);
                    }
                }
                None => {
                    let _ = writeln!(out, "  edge not found: {:?}", r.edge);
                }
            }
        }
    }
    // A mate connector's frame (or the one a Transform copied).
    if matches!(&f.kind, crate::document::FeatureKind::MateConnector(_)) || matches!(&f.kind, crate::document::FeatureKind::Transform(t) if t.copies() && !t.connectors.is_empty()) {
        let b = crate::rebuild::build(&features[..=at]);
        match b.connectors.get(&f.id) {
            Some(c) => {
                let r = |v: [f64; 3]| format!("({:.3} {:.3} {:.3})", v[0], v[1], v[2]);
                let _ = writeln!(out, "  frame: origin {} X {} Y {} Z {}", r(c.origin), r(c.u), r(c.v), r(c.normal()));
            }
            None => {
                let _ = writeln!(out, "  no frame");
            }
        }
    }
    // The part faces it refers to, where they are in the parts before it.
    let mut refs: Vec<(&str, &crate::document::FaceRef)> = Vec::new();
    match &f.kind {
        crate::document::FeatureKind::Mirror(m) => {
            if let Some(crate::pattern::MirrorPlane::Face(r)) = &m.plane {
                refs.push(("mirror plane", r));
            }
        }
        crate::document::FeatureKind::Extrude(e) => {
            refs.extend(e.faces.iter().map(|r| ("face", r)));
            if let Some(crate::document::UpTo::Face(r)) = &e.up_to {
                refs.push(("up to", r));
            }
        }
        _ => {}
    }
    if !refs.is_empty() {
        let b = crate::rebuild::build(&features[..at]);
        // Every face, and every merged-away name, the referenced faces' operations made.
        let mut ops: Vec<uuid::Uuid> = refs.iter().map(|(_, r)| r.face.op).collect();
        ops.sort();
        ops.dedup();
        for op in ops {
            let maker = features.iter().find(|x| x.id.0 == op).map_or("?", |x| x.name.as_str());
            for p in &b.parts {
                for x in p.solid.faces.iter().filter(|x| x.name.op == op) {
                    let _ = writeln!(out, "  {maker} face on {}: {:?}{}", p.name, x.name.origin, x.plane.map_or(" (curved)".to_string(), |pl| format!(", normal {:?} through {:?}", pl.normal(), pl.origin)));
                }
                for a in p.solid.face_aliases.iter().filter(|a| a.name.op == op) {
                    let _ = writeln!(out, "  {maker} face merged into {:?} on {}: {:?}", a.face.origin, p.name, a.name.origin);
                }
            }
        }
        for (what, r) in refs {
            let face = b.parts.iter().flat_map(|p| p.solid.faces.iter().map(move |x| (p, x))).find(|(_, x)| x.name == r.face);
            let maker = features.iter().find(|x| x.id.0 == r.face.op).map_or("?", |x| x.name.as_str());
            let what = format!("{what} (made by {maker})");
            match face {
                Some((p, x)) => match x.plane {
                    Some(pl) => {
                        let _ = writeln!(out, "  {what}: {:?} on {} — flat, origin {:?}, normal {:?}", r.face.origin, p.name, pl.origin, pl.normal());
                    }
                    None => {
                        let _ = writeln!(out, "  {what}: {:?} on {} — curved", r.face.origin, p.name);
                    }
                },
                None => {
                    let _ = writeln!(out, "  {what}: {:?} — not found by its name in the parts before it", r.face.origin);
                }
            }
        }
    }
    Ok(out)
}

/// The sketch named `name` of the Part Studios of `doc`, written out to compare with another
/// CAD's (`cadrs --check --sketch <name>`): its plane, curves (with their ends, mm), constraints,
/// regions (with their areas) and the regions the extrudes after it take.
/// `studio` picks the Part Studio by name; without it the sketch's name must be in only one.
pub fn describe_sketch(doc: &Document, studio: Option<&str>, name: &str) -> Result<String, String> {
    use std::fmt::Write;
    let with: Vec<_> = doc
        .elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .filter(|e| studio.is_none_or(|s| e.name.eq_ignore_ascii_case(s)))
        .filter(|e| e.active_features().iter().any(|f| f.name == name && f.sketch().is_some()))
        .collect();
    let el = match with.as_slice() {
        [e] => *e,
        [] => return Err(format!("no sketch \"{name}\"{}", studio.map_or(String::new(), |s| format!(" in Part Studio \"{s}\"")))),
        many => {
            let names: Vec<&str> = many.iter().map(|e| e.name.as_str()).collect();
            return Err(format!("\"{name}\" is in {} Part Studios ({}): pass --part-studio <name>", many.len(), names.join(", ")));
        }
    };
    let features = el.active_features();
    let Some((f, sk)) = features.iter().find(|f| f.name == name).and_then(|f| Some((f, f.sketch()?))) else { return Err(format!("no sketch \"{name}\"")) };
    let g = &sk.geometry;
    let mut out = String::new();
    let _ = writeln!(out, "{name} in {}, on {:?}", el.name, sk.plane);
    let pos = |p: cadrs_sketch::PointId| g.points.get(p).map_or("?".to_string(), |q| format!("({:.3}, {:.3})", q.pos.x, q.pos.y));
    // What can still move.
    let a = cadrs_sketch::solve::analyze(g);
    let under = cadrs_sketch::solve::Status::Under;
    let mut free_points: Vec<String> = a.points.iter().filter(|(_, s)| **s == under).map(|(p, _)| pos(*p)).collect();
    free_points.sort();
    let mut free_curves: Vec<String> = a.curves.iter().filter(|(_, s)| **s == under).map(|(c, _)| format!("{c:?}")).collect();
    free_curves.sort();
    let _ = writeln!(out, "  degrees of freedom left: {}; free points {}; free curves {}", a.dof, free_points.join(" "), free_curves.join(" "));
    let _ = writeln!(out, "  curves ({}):", g.curves.len());
    for (id, c) in &g.curves {
        let what = match c.kind {
            cadrs_sketch::CurveKind::Line { a, b } => format!("line {} → {}", pos(a), pos(b)),
            cadrs_sketch::CurveKind::Circle { center, radius } => format!("circle at {} r {radius:.3}", pos(center)),
            cadrs_sketch::CurveKind::Arc { center, start, end } => format!("arc at {} from {} to {} (counter-clockwise)", pos(center), pos(start), pos(end)),
            k => format!("{k:?}").split(['(', ' ', '{']).next().unwrap_or_default().to_string(),
        };
        let _ = writeln!(out, "    {id:?} {what}{}", if c.construction { "  [construction]" } else { "" });
    }
    let _ = writeln!(out, "  constraints ({}):", g.constraints.len());
    let kinds: Vec<String> = g.constraints.values().map(variant).collect();
    let _ = writeln!(out, "    {}", counted(&kinds));
    for (id, c) in &g.constraints {
        if matches!(c, cadrs_sketch::ConstraintOf::Use(..) | cadrs_sketch::ConstraintOf::Pierce(..)) {
            let _ = writeln!(out, "    {id:?} {}", describe(g, id));
        }
    }
    // The parts before it with flat faces parallel to its plane and near it: what imprints it.
    if let Some(plane) = sk.plane {
        let frame = plane.frame();
        let at = features.iter().position(|x| x.id == f.id).unwrap_or(0);
        let b = crate::rebuild::build(&features[..at]);
        let n = frame.normal();
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-300);
        let _ = writeln!(out, "  faces in or near its plane (parts before it):");
        for p in &b.parts {
            for face in &p.solid.faces {
                let (Some(fp), Some(q)) = (face.plane, face.loops.first().and_then(|l| l.first())) else { continue };
                let m = fp.normal();
                let ml = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-300);
                let parallel = ((m[0] * n[0] + m[1] * n[1] + m[2] * n[2]) / (ml * nl)).abs() > 1.0 - 1e-9;
                let d = frame.distance(*q) / nl;
                if parallel && d.abs() < 2.0 {
                    let _ = writeln!(out, "    {} ({:.1} mm³): {:?} at {d:+.6} mm{}", p.name, p.solid.volume(), face.name.origin, if d.abs() < 1e-6 { ", imprinted" } else { "" });
                }
            }
        }
    }
    let _ = writeln!(out, "  imprint ({}):", g.imprint.len());
    for i in &g.imprint {
        let _ = writeln!(out, "    {:?} {:?}", i.id, i.shape);
    }
    let regions = cadrs_sketch::region::regions_shared(g);
    let _ = writeln!(out, "  regions ({}):", regions.len());
    for (i, r) in regions.iter().enumerate() {
        let (lo, hi) = r.outer.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p.x), lo[1].min(p.y)], [hi[0].max(p.x), hi[1].max(p.y)]));
        let mut curves: Vec<String> = r.curves.iter().map(|c| if g.curves.contains_key(*c) { format!("{c:?}") } else { format!("{c:?} (imprint)") }).collect();
        curves.dedup();
        let _ = writeln!(out, "    #{i}: area {:.3} mm², ({:.3}, {:.3}) to ({:.3}, {:.3}), {} hole(s), bounded by {}", r.area(), lo[0], lo[1], hi[0], hi[1], r.holes.len(), curves.join(", "));
    }
    for x in &features {
        let crate::document::FeatureKind::Extrude(e) = &x.kind else { continue };
        for r in e.regions.iter().filter(|r| r.sketch == f.id) {
            let found = r.resolve(g).and_then(|got| regions.iter().position(|q| q.curves == got.curves && (q.area() - got.area()).abs() < 1e-9));
            let _ = writeln!(out, "  {} takes region {} (seed ({:.3}, {:.3}), {} curve(s))", x.name, found.map_or("none".to_string(), |i| format!("#{i}")), r.seed.x, r.seed.y, r.curves.len());
        }
        if e.sketches.contains(&f.id) {
            let _ = writeln!(out, "  {} takes the whole sketch", x.name);
        }
    }
    Ok(out)
}

/// A constraint's kind: its variant's name.
fn variant(c: &cadrs_sketch::constraint::Constraint) -> String {
    let s = format!("{c:?}");
    s.split(['(', ' ', '{']).next().unwrap_or_default().to_string()
}

/// A conflicting constraint and what it ties: each curve's shape and where it is, and the other
/// constraints that hold that curve (one of them is usually what disagrees).
fn describe(g: &cadrs_sketch::Sketch, id: cadrs_sketch::ConstraintId) -> String {
    use cadrs_sketch::constraint::CurveRef;
    let Some(c) = g.constraints.get(id) else { return "?".into() };
    let mut curves = Vec::new();
    let _ = c.map(Some, |r| {
        curves.push(r);
        Some(r)
    });
    let shape = |r: CurveRef| -> String {
        let CurveRef::Curve(k) = r else { return format!("{r:?}") };
        let Some(curve) = g.curves.get(k) else { return "a missing curve".into() };
        let held: Vec<String> = g
            .constraints
            .iter()
            .filter(|(o, _)| *o != id)
            .filter(|(_, oc)| {
                let mut on = false;
                let _ = oc.map(Some, |r| {
                    on |= r == CurveRef::Curve(k);
                    Some(r)
                });
                on
            })
            .map(|(_, oc)| variant(oc))
            .collect();
        let held = if held.is_empty() { String::new() } else { format!(", also {}", counted(&held)) };
        match curve.kind {
            cadrs_sketch::CurveKind::Line { a, b } => match (g.points.get(a), g.points.get(b)) {
                (Some(p), Some(q)) => {
                    let d = q.pos - p.pos;
                    format!("line at {:.4}° from ({:.3}, {:.3}) to ({:.3}, {:.3}){held}", d.y.atan2(d.x).to_degrees(), p.pos.x, p.pos.y, q.pos.x, q.pos.y)
                }
                _ => format!("line{held}"),
            },
            k => format!("{}{held}", format!("{k:?}").split(['(', ' ', '{']).next().unwrap_or_default()),
        }
    };
    let what: Vec<String> = curves.into_iter().map(shape).collect();
    format!("{}: {}", variant(c), what.join("; "))
}

/// `["A", "B", "A"]` as "2 × A, 1 × B".
fn counted(kinds: &[String]) -> String {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for k in kinds {
        match seen.iter_mut().find(|(x, _)| *x == k.as_str()) {
            Some((_, n)) => *n += 1,
            None => seen.push((k, 1)),
        }
    }
    seen.iter().map(|(k, n)| format!("{n} × {k}")).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_kinds() {
        let k: Vec<String> = ["UseEdge", "Coincident", "UseEdge"].iter().map(|s| s.to_string()).collect();
        assert_eq!(counted(&k), "2 × UseEdge, 1 × Coincident");
    }

    #[test]
    fn a_sketch_without_a_plane_is_reported() {
        use crate::document::{FeatureKind, SketchFeature};
        let f = Feature::new(crate::FeatureId::new(), "Sketch 1", FeatureKind::Sketch(SketchFeature::new(None)));
        let report = check_features("Part Studio 1", &[f]);
        assert!(report.issues.iter().any(|i| i.feature == "Sketch 1" && i.severity == Severity::Error && i.detail.contains("sketch plane")), "{:?}", report.issues);
        // A new document's empty Part Studio has nothing to report.
        let doc = Document::new("Check");
        assert!(check(&doc, None).iter().all(|s| s.issues.is_empty()));
    }
}
