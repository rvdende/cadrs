//! Part Studio features other than sketches and extrudes: Boolean, Fillet, Chamfer, Revolve,
//! Mirror, Linear and Circular pattern. Each maps its Onshape parameters onto the cadrs
//! feature of the same name and adds it through the command layer.

use cadrs_core::advanced::{LoftFeature, LoftProfile};
use cadrs_core::applied::{ChamferFeature, ChamferType, EdgeOrFace, FilletFeature};
use cadrs_core::command::CommandError;
use cadrs_core::commands::{AddFeature, RenameFeature};
use cadrs_core::document::{AxisRef, BodyType, BooleanFeature, BooleanKind, BooleanOp, DirectionRef, FeatureKind, RevolveFeature, RevolveType};
use cadrs_core::ids::{FeatureId, PartId};
use cadrs_core::derived::DerivedFeature;
use cadrs_core::ids::{DocumentId, ElementId};
use cadrs_core::import::{AddImport, ImportUnit};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use crate::ids::stable_u128;
use cadrs_core::transform::{SecondaryAxis, TransformFeature, TransformType};
use std::collections::HashMap;
use cadrs_core::pattern::{MirrorFeature, MirrorPlane, PatternFeature, PatternKind, PatternType};
use cadrs_sketch::units::Quantity;
use cadrs_sketch::CurveId;
use serde_json::Value;

use crate::import::{PartStudio, eval_expr};
use crate::query::Value as Q;
use crate::refs::{self, Pick, op_feature};
use crate::report::{FeatureReport, Outcome};
use crate::sketch::param;
use crate::studio::Studio;

#[path = "surfacing.rs"]
mod surfacing;

impl PartStudio<'_> {
    fn text(f: &Value, id: &str) -> String {
        param(f, id).and_then(|p| p["value"].as_str()).unwrap_or_default().to_string()
    }

    fn flag(f: &Value, id: &str) -> bool {
        param(f, id).and_then(|p| p["value"].as_bool()).unwrap_or(false)
    }

    fn expr(f: &Value, id: &str) -> String {
        param(f, id).and_then(|p| p["expression"].as_str()).unwrap_or_default().to_string()
    }

    /// A length or angle parameter's value (mm or degrees) and its expression as cadrs keeps
    /// it (variables inlined).
    fn quantity(&self, f: &Value, id: &str, q: Quantity) -> Result<(f64, String), CommandError> {
        let e = Self::expr(f, id);
        let v = eval_expr(&e, q, &self.vars).ok_or_else(|| CommandError::Invalid(format!("cannot evaluate {id} {e:?}")))?;
        let unit = if q == Quantity::Angle { "deg" } else { "mm" };
        Ok((v, if e.contains('#') { format!("{v} {unit}") } else { e }))
    }

    fn count(&self, f: &Value, id: &str) -> Result<u32, CommandError> {
        let e = Self::expr(f, id);
        eval_expr(&e, Quantity::Count, &self.vars)
            .map(|v| v.round().max(1.0) as u32)
            .ok_or_else(|| CommandError::Invalid(format!("cannot evaluate {id} {e:?}")))
    }

    /// The parts a body parameter picks (the bodies an operation made, or has since merged
    /// into).
    pub(crate) fn parts_param(&mut self, f: &Value, id: &str, lost: &mut usize) -> Vec<PartId> {
        let parts = self.parts();
        let doc = self.s.doc.element(self.el).map(|e| e.features().to_vec()).unwrap_or_default();
        // Each pick's candidate parts and, where several, how strongly its topology points at
        // each.
        let mut picks: Vec<(Vec<PartId>, HashMap<PartId, f64>)> = Vec::new();
        for p in refs::picks(param(f, id)) {
            let Pick::Query(q) = p else {
                *lost += 1;
                continue;
            };
            let mut found = body_parts_in(&q, &self.features, &parts, &doc);
            // A piece of a split body: every part the splitting operation touched is a candidate.
            if q.get("queryType").and_then(Q::as_str) == Some("SPLIT")
                && let Some(op) = q.get("operationId").and_then(Q::as_str).and_then(|o| self.features.get(op_feature(o)))
            {
                for p in parts.iter().filter(|p| p.features.contains(op) || p.id.feature == *op) {
                    if !found.contains(&p.id) {
                        found.push(p.id);
                    }
                }
            }
            // Nothing by history (a later boolean merged the body into another part): the part
            // its named faces and edges are on now.
            if found.is_empty() {
                found = parts.iter().map(|p| p.id).collect();
            }
            let votes = if found.len() > 1 { self.topology_votes(&q, &parts, &found) } else { HashMap::new() };
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_BODIES").is_some() {
                let short = |p: &PartId| format!("{}#{}", &p.feature.0.to_string()[..4], p.index);
                eprintln!("BODY {} {id}: {} candidates {:?} votes {:?}", f["name"], self.describe(&q), found.iter().map(short).collect::<Vec<_>>(), votes.iter().map(|(p, v)| (short(p), *v)).collect::<Vec<_>>());
            }
            picks.push((found, votes));
        }
        // Picks of several candidates get distinct parts, strongest evidence first (a boolean's
        // tools are different bodies).
        let mut chosen: Vec<Option<PartId>> = picks.iter().map(|(found, _)| (found.len() == 1).then(|| found[0])).collect();
        let mut offers: Vec<(f64, usize, PartId)> =
            picks.iter().enumerate().filter(|(i, _)| chosen[*i].is_none()).flat_map(|(i, (_, v))| v.iter().map(move |(p, s)| (*s, i, *p))).collect();
        offers.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, i, p) in offers {
            if chosen[i].is_none() && !chosen.contains(&Some(p)) {
                chosen[i] = Some(p);
            }
        }
        let mut out = Vec::new();
        for (i, c) in chosen.into_iter().enumerate() {
            let ids: Vec<PartId> = match c {
                Some(p) => vec![p],
                // No evidence: the candidates by history, unless that was every part.
                None if picks[i].0.len() < parts.len() => picks[i].0.clone(),
                None => Vec::new(),
            };
            if ids.is_empty() {
                *lost += 1;
            }
            for id in ids {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// How strongly the entities in the query's TOPOLOGY disambiguation point at each of the
    /// candidate parts `among`: each entity shares one vote between the candidates it is on.
    fn topology_votes(&self, q: &Q, parts: &[cadrs_core::parts::Part], among: &[PartId]) -> HashMap<PartId, f64> {
        let model = self.model(parts);
        let mut votes: HashMap<PartId, f64> = HashMap::new();
        for d in q.get("disambiguationData").map(Q::items).unwrap_or_default() {
            if d.get("disambiguationType").and_then(Q::as_str) != Some("TOPOLOGY") {
                continue;
            }
            for e in d.get("entities").map(Q::items).unwrap_or_default() {
                let e = match e {
                    Q::Array(pair) if !pair.is_empty() => &pair[0],
                    e => e,
                };
                let mut on: Vec<PartId> = Vec::new();
                for ent in model.eval(e) {
                    let (crate::eval::Ent::Face(pi, _) | crate::eval::Ent::Edge(pi, _)) = ent;
                    let id = parts[pi].id;
                    if among.contains(&id) && !on.contains(&id) {
                        on.push(id);
                    }
                }
                for id in &on {
                    *votes.entry(*id).or_default() += 1.0 / on.len() as f64;
                }
            }
        }
        votes
    }

    /// The edges (or faces, for all their edges) a parameter picks.
    fn edges_param(&mut self, f: &Value, id: &str, lost: &mut usize) -> Vec<EdgeOrFace> {
        let parts = self.parts();
        let model = self.model(&parts);
        let mut out = Vec::new();
        for p in refs::picks(param(f, id)) {
            let Pick::Query(q) = p else {
                *lost += 1;
                continue;
            };
            let found: Vec<EdgeOrFace> = if q.get("entityType").and_then(Q::as_str) == Some("FACE") {
                model.face(&q).map(EdgeOrFace::Face).into_iter().collect()
            } else {
                model.edges(&q).into_iter().map(EdgeOrFace::Edge).collect()
            };
            if found.is_empty() {
                *lost += 1;
                if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                    eprintln!("LOSTEDGE {} {id} | {}", f["name"], self.describe(&q));
                    let derived: Vec<&Q> = match q.get("derivedFrom") {
                        Some(Q::Array(a)) => a.iter().collect(),
                        Some(v) => vec![v],
                        None => Vec::new(),
                    };
                    for d in derived {
                        let found = model.eval(d);
                        let names: Vec<String> = found
                            .iter()
                            .filter_map(|e| match e {
                                crate::eval::Ent::Face(p, i) => Some(format!("{:?}", parts[*p].solid.faces[*i].name.origin)),
                                _ => None,
                            })
                            .collect();
                        eprintln!("      side {} → {:?}", self.describe(d), names);
                    }
                }
            } else if std::env::var_os("CADRS_ONSHAPE_DEBUG_EDGES").is_some() {
                eprintln!("EDGES {} {id}: {} | {}", f["name"], found.len(), self.describe(&q));
                for e in &found {
                    if let EdgeOrFace::Edge(r) = e {
                        eprintln!("      {:?} at {:?}", r.edge.faces.map(|n| n.origin), r.seed.map(|v| (v * 100.0).round() / 100.0));
                    }
                }
            }
            out.extend(found);
        }
        out
    }

    /// A sketch curve a query names directly (a sketch line picked as an axis or direction).
    fn sketch_curve(&self, q: &Q) -> Option<(FeatureId, CurveId)> {
        if q.get("queryType")?.as_str()? != "SKETCH_ENTITY" {
            return None;
        }
        let sketch = op_feature(q.get("operationId")?.as_str()?);
        let (map, _) = self.sketches.get(sketch)?;
        Some((map.feature, *map.curves.get(q.get("sketchEntityId")?.as_str()?)?))
    }

    /// A Mate connector feature a query picks (its axes: `<feature>.mateConnectorOp`).
    fn connector_of(&self, q: &Q) -> Option<ConnectorRef> {
        let op = q.get("operationId")?.as_str()?;
        if !op.ends_with(".mateConnectorOp") {
            return None;
        }
        self.features.get(op_feature(op)).map(|id| ConnectorRef::Feature(*id))
    }

    /// An axis parameter: a sketch line or circle, a part edge, or a face of revolution.
    fn axis_param(&mut self, f: &Value, id: &str) -> Option<AxisRef> {
        let q = refs::picks(param(f, id)).into_iter().find_map(|p| match p {
            Pick::Query(q) => Some(q),
            _ => None,
        })?;
        if let Some((sketch, curve)) = self.sketch_curve(&q) {
            return Some(AxisRef::SketchCurve { sketch, curve });
        }
        if let Some(c) = self.connector_of(&q) {
            return Some(AxisRef::Connector(c));
        }
        let parts = self.parts();
        let model = self.model(&parts);
        if let Some(e) = model.edges(&q).into_iter().next() {
            return Some(AxisRef::Edge(e));
        }
        model.face(&q).map(AxisRef::Face)
    }

    /// A direction parameter: a sketch line, a part edge, a face's normal or a default plane's.
    pub(crate) fn direction_param(&mut self, f: &Value, id: &str) -> Option<DirectionRef> {
        let pick = refs::picks(param(f, id)).into_iter().next()?;
        if let Some(p) = pick.default_plane() {
            return Some(DirectionRef::PlaneNormal(p));
        }
        let Pick::Query(q) = pick else { return None };
        if let Some((sketch, curve)) = self.sketch_curve(&q) {
            return Some(DirectionRef::SketchLine { sketch, curve });
        }
        if let Some(c) = self.connector_of(&q) {
            return Some(DirectionRef::Connector(c));
        }
        let parts = self.parts();
        let model = self.model(&parts);
        if let Some(e) = model.edges(&q).into_iter().next() {
            return Some(DirectionRef::Edge(e));
        }
        model.face(&q).map(DirectionRef::FaceNormal)
    }

    /// Adds a finished feature and names it as in Onshape.
    pub(crate) fn add(&mut self, f: &Value, fid: &str, base: &str, kind: FeatureKind, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let id = self.feature_id(fid);
        self.s.run(&AddFeature { element: self.el, feature: id, base_name: base.into(), kind })?;
        if let Some(name) = f["name"].as_str() {
            self.s.run(&RenameFeature { element: self.el, feature: id, name: name.to_string() })?;
        }
        self.features.insert(fid.to_string(), id);
        self.parts();
        if let Some(e) = self.rebuild_error(id) {
            fr.notes.push(format!("rebuild error: {e}"));
        }
        if !fr.notes.is_empty() && fr.outcome == Outcome::Full {
            fr.outcome = Outcome::Partial;
        }
        Ok(())
    }

    fn lost_note(fr: &mut FeatureReport, lost: usize, what: &str) {
        if lost > 0 {
            fr.notes.push(format!("{lost} {what} not translated"));
        }
    }

    pub(crate) fn boolean(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut lost = 0;
        let op = match Self::text(f, "operationType").as_str() {
            "SUBTRACTION" => BooleanKind::Subtract,
            "INTERSECTION" => BooleanKind::Intersect,
            _ => BooleanKind::Union,
        };
        let tools = self.parts_param(f, "tools", &mut lost);
        let targets = if op == BooleanKind::Subtract { self.parts_param(f, "targets", &mut lost) } else { Vec::new() };
        if tools.is_empty() || (op == BooleanKind::Subtract && targets.is_empty()) {
            return Err(CommandError::Invalid("its parts could not be translated".into()));
        }
        Self::lost_note(fr, lost, "part selections");
        if Self::flag(f, "offset") {
            fr.notes.push("offset not imported".into());
        }
        let b = BooleanFeature { op, tools, targets, keep_tools: Self::flag(f, "keepTools"), offset: None };
        self.add(f, fid, "Boolean", FeatureKind::Boolean(b), fr)
    }

    pub(crate) fn fillet(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        if Self::text(f, "filletType") == "FULL_ROUND" {
            return Err(CommandError::Invalid("full round fillets are not imported yet".into()));
        }
        let mut lost = 0;
        let entities = self.edges_param(f, "entities", &mut lost);
        if entities.is_empty() {
            return Err(CommandError::Invalid("none of its edges could be translated".into()));
        }
        Self::lost_note(fr, lost, "edge selections");
        if Self::text(f, "crossSection") != "CIRCULAR" {
            fr.notes.push(format!("{} cross section imported as circular", Self::text(f, "crossSection").to_lowercase()));
        }
        let (size, size_expr) = self.quantity(f, "radius", Quantity::Length)?;
        let x = FilletFeature {
            entities,
            size,
            size_expr,
            tangent_propagation: Self::flag(f, "tangentPropagation"),
            allow_overflow: param(f, "allowEdgeOverflow").and_then(|p| p["value"].as_bool()).unwrap_or(true),
            ..FilletFeature::default()
        };
        self.add(f, fid, "Fillet", FeatureKind::Fillet(x), fr)
    }

    pub(crate) fn chamfer(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut lost = 0;
        let entities = self.edges_param(f, "entities", &mut lost);
        if entities.is_empty() {
            return Err(CommandError::Invalid("none of its edges could be translated".into()));
        }
        Self::lost_note(fr, lost, "edge selections");
        let mut x = ChamferFeature { entities, tangent_propagation: Self::flag(f, "tangentPropagation"), flip: Self::flag(f, "oppositeDirection"), ..ChamferFeature::default() };
        match Self::text(f, "chamferType").as_str() {
            "OFFSET_ANGLE" => {
                x.kind = ChamferType::DistanceAngle;
                (x.distance, x.distance_expr) = self.quantity(f, "width", Quantity::Length)?;
                (x.angle, x.angle_expr) = self.quantity(f, "angle", Quantity::Angle)?;
            }
            "TWO_OFFSETS" => {
                x.kind = ChamferType::TwoDistances;
                (x.distance, x.distance_expr) = self.quantity(f, "width1", Quantity::Length)?;
                (x.distance2, x.distance2_expr) = self.quantity(f, "width2", Quantity::Length)?;
            }
            _ => {
                x.kind = ChamferType::EqualDistance;
                (x.distance, x.distance_expr) = self.quantity(f, "width", Quantity::Length)?;
            }
        }
        self.add(f, fid, "Chamfer", FeatureKind::Chamfer(x), fr)
    }

    pub(crate) fn revolve(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = RevolveFeature { op: op_of(&Self::text(f, "operationType")), ..RevolveFeature::default() };
        let mut lost = 0;
        for p in refs::picks(param(f, "entities")) {
            match p {
                Pick::WholeSketch(s) => match self.features.get(&s) {
                    Some(id) => x.sketches.push(*id),
                    None => lost += 1,
                },
                Pick::Query(q) => {
                    let rs = self.region_of(&q);
                    if !rs.is_empty() {
                        x.regions.extend(rs);
                    } else if let Some(r) = self.face_of(&q) {
                        x.faces.push(r);
                    } else if let Some(fr2) = self.face_region(&q, &mut fr.notes) {
                        // A region bounded only by the sketch plane's face edges: that face.
                        match fr2 {
                            crate::import::FaceOrRegion::Face(r) => x.faces.push(r),
                            crate::import::FaceOrRegion::Region(r) => x.regions.push(r),
                        }
                    } else {
                        lost += 1;
                    }
                }
                Pick::Opaque(_) => lost += 1,
            }
        }
        if x.regions.is_empty() && x.sketches.is_empty() && x.faces.is_empty() {
            return Err(CommandError::Invalid("its regions could not be translated".into()));
        }
        Self::lost_note(fr, lost, "region selections");
        x.axis = Some(self.axis_param(f, "axis").ok_or_else(|| CommandError::Invalid("its axis could not be translated".into()))?);
        x.flip = Self::flag(f, "oppositeDirection");
        if Self::flag(f, "fullRevolve") {
            x.kind = RevolveType::Full;
        } else {
            x.kind = if Self::flag(f, "symmetric") { RevolveType::Symmetric } else { RevolveType::Blind };
            (x.angle, x.angle_expr) = self.quantity(f, "angle", Quantity::Angle)?;
        }
        if x.op != BooleanOp::New {
            let mut lost = 0;
            x.merge_scope = self.parts_param(f, "booleanScope", &mut lost);
            x.merge_all = Self::flag(f, "defaultScope");
        }
        self.add(f, fid, "Revolve", FeatureKind::Revolve(x), fr)
    }

    pub(crate) fn mirror(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        if Self::text(f, "patternType") != "PART" {
            return Err(CommandError::Invalid("feature and face mirrors are not imported yet".into()));
        }
        let mut lost = 0;
        let parts = self.parts_param(f, "entities", &mut lost);
        if parts.is_empty() {
            return Err(CommandError::Invalid("its parts could not be translated".into()));
        }
        Self::lost_note(fr, lost, "part selections");
        let plane = match refs::picks(param(f, "mirrorPlane")).into_iter().next() {
            Some(p) if p.default_plane().is_some() => p.default_plane().map(MirrorPlane::Plane),
            Some(Pick::Query(q)) => self.face_of(&q).map(MirrorPlane::Face),
            _ => None,
        };
        let plane = plane.ok_or_else(|| CommandError::Invalid("its mirror plane could not be translated".into()))?;
        let op = op_of(&Self::text(f, "operationType"));
        let mut lost = 0;
        let merge_scope = if op == BooleanOp::New { Vec::new() } else { self.parts_param(f, "booleanScope", &mut lost) };
        let x = MirrorFeature { mirror_type: PatternType::Part, parts, plane: Some(plane), op, merge_all: Self::flag(f, "defaultScope"), merge_scope, ..MirrorFeature::default() };
        self.add(f, fid, "Mirror", FeatureKind::Mirror(x), fr)
    }

    pub(crate) fn pattern(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        if Self::text(f, "patternType") != "PART" {
            return Err(CommandError::Invalid("feature and face patterns are not imported yet".into()));
        }
        let circular = f["featureType"].as_str() == Some("circularPattern");
        let mut lost = 0;
        let parts = self.parts_param(f, "entities", &mut lost);
        if parts.is_empty() {
            return Err(CommandError::Invalid("its parts could not be translated".into()));
        }
        Self::lost_note(fr, lost, "part selections");
        let mut x = PatternFeature { kind: if circular { PatternKind::Circular } else { PatternKind::Linear }, pattern_type: PatternType::Part, parts, ..PatternFeature::default() };
        x.op = op_of(&Self::text(f, "operationType"));
        x.first.count = self.count(f, "instanceCount")?;
        x.first.flip = Self::flag(f, "oppositeDirection");
        if circular {
            let axis = match self.axis_param(f, "axis") {
                Some(a) => Some(a),
                // A mate connector, possibly one made in the dialog (a subfeature).
                None => {
                    let subs: HashMap<String, Value> =
                        f["subFeatures"].as_array().into_iter().flatten().filter_map(|s| Some((s["featureId"].as_str()?.to_string(), s.clone()))).collect();
                    self.connector_ref(f, "axis", &subs, fr).map(AxisRef::Connector)
                }
            };
            x.axis = Some(axis.ok_or_else(|| CommandError::Invalid("its axis could not be translated".into()))?);
            (x.angle, x.angle_expr) = self.quantity(f, "angle", Quantity::Angle)?;
            x.equal_spacing = Self::flag(f, "equalSpace");
        } else {
            x.first.direction = Some(self.direction_param(f, "directionOne").ok_or_else(|| CommandError::Invalid("its direction could not be translated".into()))?);
            (x.first.distance, x.first.distance_expr) = self.quantity(f, "distance", Quantity::Length)?;
            // Onshape's Centered puts `count - 1` instances on each side of the seed (its
            // instances are named -1, 1, …); cadrs's spreads `count` instances in all about it.
            let centered = |d: &mut cadrs_core::pattern::LinearDirection| {
                d.centered = true;
                d.count = 2 * d.count - 1;
            };
            if Self::flag(f, "isCentered") {
                centered(&mut x.first);
            }
            if Self::flag(f, "hasSecondDir") {
                x.second_on = true;
                x.second.direction = self.direction_param(f, "directionTwo");
                (x.second.distance, x.second.distance_expr) = self.quantity(f, "distanceTwo", Quantity::Length)?;
                x.second.count = self.count(f, "instanceCountTwo")?;
                x.second.flip = Self::flag(f, "oppositeDirectionTwo");
                if Self::flag(f, "isCenteredTwo") {
                    centered(&mut x.second);
                }
            }
        }
        if x.op != BooleanOp::New {
            let mut lost = 0;
            x.merge_scope = self.parts_param(f, "booleanScope", &mut lost);
            x.merge_all = Self::flag(f, "defaultScope");
        }
        self.add(f, fid, if circular { "Circular pattern" } else { "Linear pattern" }, FeatureKind::Pattern(x), fr)
    }
}

impl PartStudio<'_> {
    /// What a mate connector origin (or axis) query picks: a sketch point or curve, a face or
    /// an edge of the model.
    fn connector_origin(&mut self, q: &Q) -> Option<ConnectorOrigin> {
        if q.get("queryType").and_then(Q::as_str) == Some("SKETCH_ENTITY") {
            let sketch = op_feature(q.get("operationId")?.as_str()?);
            let (map, _) = self.sketches.get(sketch)?;
            let id = q.get("sketchEntityId")?.as_str()?;
            if let Some(p) = map.points.get(id) {
                return Some(ConnectorOrigin::SketchPoint { sketch: map.feature, point: *p });
            }
            return map.curves.get(id).map(|c| ConnectorOrigin::SketchCurve { sketch: map.feature, curve: *c });
        }
        let parts = self.parts();
        let model = self.model(&parts);
        match q.get("entityType").and_then(Q::as_str) {
            Some("FACE") => model.face(q).map(ConnectorOrigin::Face),
            Some("EDGE") => model.edges(q).into_iter().next().map(ConnectorOrigin::Edge),
            Some("VERTEX") => model.vertex(q).map(ConnectorOrigin::Vertex),
            _ => None,
        }
    }

    fn connector_param(&mut self, f: &Value, id: &str) -> Option<ConnectorOrigin> {
        let pick = refs::picks(param(f, id)).into_iter().next()?;
        if pick.is_origin() {
            return Some(ConnectorOrigin::Origin);
        }
        match pick {
            Pick::Query(q) => {
                let r = self.connector_origin(&q).or_else(|| self.connector_by_geometry(f, id));
                if r.is_none() && std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                    let parts = self.parts();
                    let found = self.model(&parts).eval(&q);
                    eprintln!("CONNECTOR {} {id} | {} → {} entities", f["name"], self.describe(&q), found.len());
                    let op = q.get("operationId").and_then(Q::as_str).map(op_feature).and_then(|o| self.features.get(o)).map(|f| f.0);
                    for p in &parts {
                        for face in &p.solid.faces {
                            if Some(face.name.op) == op {
                                eprintln!("      face of op: {:?}", face.name.origin);
                            }
                        }
                    }
                }
                r
            }
            _ => None,
        }
    }

    pub(crate) fn mate_connector(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        if Self::text(f, "originType") != "ON_ENTITY" {
            return Err(CommandError::Invalid(format!("origin type {} not imported yet", Self::text(f, "originType"))));
        }
        let origin = self.connector_param(f, "originQuery").ok_or_else(|| CommandError::Invalid("its origin could not be translated".into()))?;
        let mut x = MateConnectorFeature { origin: Some(origin), ..MateConnectorFeature::default() };
        // On a sketch line, Onshape's connector has Z along the line (cadrs's implicit one has
        // the sketch's axes): align it.
        if let ConnectorOrigin::SketchCurve { sketch, curve } = origin
            && self
                .s
                .doc
                .element(self.el)
                .and_then(|e| e.feature(sketch))
                .and_then(|f| f.sketch())
                .and_then(|s| s.geometry.curves.get(curve))
                .is_some_and(|c| matches!(c.kind, cadrs_sketch::CurveKind::Line { .. }))
        {
            x.alignment = Some(DirectionRef::SketchLine { sketch, curve });
        }
        if Self::flag(f, "realign") {
            x.realign = true;
            x.primary_axis = self.connector_param(f, "primaryAxisQuery");
            x.secondary_axis = self.connector_param(f, "secondaryAxisQuery");
        }
        let mut lost = 0;
        if let Some(owner) = self.parts_param(f, "ownerPart", &mut lost).first() {
            x.owner_on = true;
            x.owner = Some(*owner);
        }
        x.flip_primary = Self::flag(f, "flipPrimary");
        x.reorient = match Self::text(f, "secondaryAxisType").as_str() {
            "PLUS_Y" => 1,
            "MINUS_X" => 2,
            "MINUS_Y" => 3,
            _ => 0,
        };
        x.move_on = Self::flag(f, "transform");
        if x.move_on {
            for (i, id) in ["translationX", "translationY", "translationZ"].iter().enumerate() {
                (x.offset[i], x.offset_expr[i]) = self.quantity(f, id, Quantity::Length)?;
            }
            (x.rotation, x.rotation_expr) = self.quantity(f, "rotation", Quantity::Angle)?;
            if Self::text(f, "rotationType") != "ABOUT_Z" {
                fr.notes.push(format!("rotation {} imported as about Z", Self::text(f, "rotationType")));
            }
        }
        // The point Onshape inferred on the entity (a face's corner): the second origin query.
        let inferred = param(f, "secondaryOriginQuery").and_then(|p| p["queries"].as_array()).is_some_and(|a| !a.is_empty());
        if inferred {
            x.at = self.connector_param(f, "secondaryOriginQuery");
        }
        if Self::text(f, "entityInferenceType") == "POINT" && x.at.is_none() && !matches!(origin, ConnectorOrigin::SketchPoint { .. } | ConnectorOrigin::Vertex(_)) {
            fr.notes.push("placed at cadrs's implicit point of the entity (Onshape picked a point on it)".into());
        }
        self.add(f, fid, "Mate connector", FeatureKind::MateConnector(x), fr)
    }

    pub(crate) fn delete_bodies(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut lost = 0;
        let parts = self.parts_param(f, "entities", &mut lost);
        if parts.is_empty() {
            return Err(CommandError::Invalid("its parts could not be translated".into()));
        }
        Self::lost_note(fr, lost, "part selections");
        self.add(f, fid, "Delete part", FeatureKind::DeletePart(cadrs_core::document::DeletePartFeature { parts }), fr)
    }
}

impl PartStudio<'_> {
    pub(crate) fn loft(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = LoftFeature::default();
        let surface = Self::text(f, "bodyType") == "SURFACE";
        x.body = if surface { BodyType::Surface } else { BodyType::Solid };
        x.op = if surface { BooleanOp::New } else { op_of(&Self::text(f, "operationType")) };
        let items: Vec<Value> = param(f, "sheetProfilesArray").and_then(|p| p["items"].as_array()).cloned().unwrap_or_default();
        if param(f, "wireProfilesArray").and_then(|p| p["items"].as_array()).is_some_and(|a| !a.is_empty()) {
            return Err(CommandError::Invalid("wire (point or curve) profiles are not imported yet".into()));
        }
        for item in &items {
            let mut regions: Vec<(FeatureId, cadrs_core::document::RegionRef)> = Vec::new();
            let mut profile = None;
            for p in refs::picks(param(item, "sheetProfileEntities")) {
                match p {
                    Pick::WholeSketch(s) => profile = self.features.get(&s).map(|id| LoftProfile::Sketch(*id)),
                    Pick::Query(q) => {
                        let rs = self.region_of(&q);
                        if !rs.is_empty() {
                            regions.extend(rs.into_iter().map(|r| (r.sketch, r)));
                        } else if let Some(face) = self.face_of(&q) {
                            profile = Some(LoftProfile::Face(face));
                        }
                    }
                    Pick::Opaque(_) => {}
                }
            }
            if let Some((sketch, _)) = regions.first() {
                let sketch = *sketch;
                profile = Some(LoftProfile::Regions { sketch, regions: regions.into_iter().map(|(_, r)| r).collect() });
            }
            x.profiles.push(profile.ok_or_else(|| CommandError::Invalid("a profile could not be translated".into()))?);
        }
        if x.profiles.len() < 2 {
            return Err(CommandError::Invalid("fewer than two profiles".into()));
        }
        if Self::text(f, "startCondition") != "DEFAULT" || Self::text(f, "endCondition") != "DEFAULT" {
            fr.notes.push("end conditions not imported".into());
        }
        if param(f, "guidesArray").and_then(|p| p["items"].as_array()).is_some_and(|a| !a.is_empty()) {
            fr.notes.push("guides not imported".into());
        }
        if x.op != BooleanOp::New {
            let mut lost = 0;
            x.merge_scope = self.parts_param(f, "booleanScope", &mut lost);
            x.merge_all = Self::flag(f, "defaultScope");
        }
        self.add(f, fid, "Loft", FeatureKind::Loft(x), fr)
    }
}

impl PartStudio<'_> {
    /// A mate connector a Transform (or other feature) parameter picks: a Mate connector
    /// feature, possibly one stored inside the feature itself (Onshape keeps connectors made in
    /// a dialog as subfeatures; they are imported as Mate connector features just before it).
    fn connector_ref(&mut self, f: &Value, id: &str, subs: &HashMap<String, Value>, fr: &mut FeatureReport) -> Option<ConnectorRef> {
        let p = param(f, id)?;
        let q = p["queries"].as_array()?.first()?;
        let qs = q["queryString"].as_str().unwrap_or_default();
        // `qCreatedBy(id + "<feature>", …)` or a query on `<feature>.mateConnectorOp`.
        let fid = if let Some(i) = qs.find("id + \"") {
            qs[i + 6..].split('"').next().map(String::from)
        } else {
            match crate::query::decode(qs).ok().flatten() {
                Some(v) => v.get("operationId").and_then(Q::as_str).map(|o| op_feature(o).to_string()),
                None => None,
            }
        }?;
        if let Some(id) = self.features.get(&fid) {
            return Some(ConnectorRef::Feature(*id));
        }
        let sub = subs.get(&fid)?.clone();
        let mut sr = FeatureReport { name: sub["name"].as_str().unwrap_or_default().into(), kind: "mateConnector".into(), outcome: Outcome::Full, notes: Vec::new() };
        match self.mate_connector(&sub, &fid, &mut sr) {
            Ok(()) => self.features.get(&fid).map(|id| ConnectorRef::Feature(*id)),
            Err(e) => {
                fr.notes.push(format!("its mate connector could not be imported: {e}"));
                None
            }
        }
    }

    pub(crate) fn transform(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let kind = Self::text(f, "transformType");
        // Onshape's enum says TRANSFORM_MATE_CONNECTORS; cadrs keeps the singular.
        let t = TransformType::from_onshape(kind.trim_end_matches('S'))
            .or_else(|| TransformType::from_onshape(&kind)).ok_or_else(|| CommandError::Invalid(format!("transform type {kind} not imported yet")))?;
        let mut lost = 0;
        let parts = self.parts_param(f, "entities", &mut lost);
        // Mate connectors (a feature's, or a Transform's copy of one) are transformed too.
        let connectors = if parts.is_empty() { self.connector_entities(f) } else { Vec::new() };
        if parts.is_empty() && connectors.is_empty() {
            return Err(CommandError::Invalid("its parts could not be translated".into()));
        }
        Self::lost_note(fr, lost.saturating_sub(connectors.len()), "part selections");
        let subs: HashMap<String, Value> =
            f["subFeatures"].as_array().into_iter().flatten().filter_map(|s| Some((s["featureId"].as_str()?.to_string(), s.clone()))).collect();
        let mut x = TransformFeature::new(t);
        x.parts = parts;
        x.connectors = connectors;
        x.copy = Self::flag(f, "makeCopy");
        x.flip = Self::flag(f, "oppositeDirection");
        let missing = |what: &str| CommandError::Invalid(format!("its {what} could not be translated"));
        match t {
            TransformType::TranslateXyz => {
                (x.dx, x.dx_expr) = self.quantity(f, "dx", Quantity::Length)?;
                (x.dy, x.dy_expr) = self.quantity(f, "dy", Quantity::Length)?;
                (x.dz, x.dz_expr) = self.quantity(f, "dz", Quantity::Length)?;
            }
            TransformType::Rotate => {
                x.axis = Some(self.axis_param(f, "transformAxis").ok_or_else(|| missing("axis"))?);
                (x.angle, x.angle_expr) = self.quantity(f, "angle", Quantity::Angle)?;
            }
            TransformType::TranslateByDistance => {
                x.direction = Some(self.direction_param(f, "transformDirection").or_else(|| self.direction_param(f, "transformAxis")).ok_or_else(|| missing("direction"))?);
                (x.distance, x.distance_expr) = self.quantity(f, "distance", Quantity::Length)?;
            }
            TransformType::TranslateByLine => {
                x.line = Some(self.direction_param(f, "transformLine").or_else(|| self.direction_param(f, "transformAxis")).ok_or_else(|| missing("line"))?);
            }
            TransformType::MateConnectors => {
                x.from = Some(self.connector_ref(f, "baseConnector", &subs, fr).ok_or_else(|| missing("base mate connector"))?);
                x.to = Some(self.connector_ref(f, "destinationConnector", &subs, fr).ok_or_else(|| missing("destination mate connector"))?);
                x.flip_primary = Self::flag(f, "oppositeDirectionMateAxis");
                x.secondary = SecondaryAxis::from_onshape(&Self::text(f, "secondaryAxisType")).unwrap_or_default();
                self.solve_base_connector(&x);
            }
            TransformType::ScaleUniformly => {
                if !param(f, "uniform").and_then(|p| p["value"].as_bool()).unwrap_or(true) {
                    return Err(CommandError::Invalid("non-uniform scale is not imported yet".into()));
                }
                (x.scale, x.scale_expr) = self.quantity(f, "scale", Quantity::Count)?;
                if param(f, "scalePoint").and_then(|p| p["queries"].as_array()).is_some_and(|a| !a.is_empty()) {
                    x.scale_point = Some(self.connector_ref(f, "scalePoint", &subs, fr).ok_or_else(|| missing("scale point"))?);
                }
            }
            TransformType::CopyInPlace => {}
        }
        self.add(f, fid, "Transform", FeatureKind::Transform(x), fr)
    }
}

/// A reference parameter's `namespace`: `d<doc>::v<version>::e<element>::m<microversion>`, or
/// `e<element>::m<microversion>` within this document. Returns (document, element).
pub(crate) fn namespace_ref(ns: &str) -> (Option<String>, Option<String>) {
    let mut doc = None;
    let mut el = None;
    for part in ns.split("::") {
        match part.split_at_checked(1) {
            Some(("d", rest)) => doc = Some(rest.to_string()),
            Some(("e", rest)) => el = Some(rest.to_string()),
            _ => {}
        }
    }
    (doc, el)
}

impl PartStudio<'_> {
    /// Onshape's Import (`importForeign`): the CAD file is a Blob tab of this document.
    pub(crate) fn import_foreign(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let ns = param(f, "blobData").and_then(|p| p["namespace"].as_str()).unwrap_or_default();
        let (_, el) = namespace_ref(ns);
        let el = el.ok_or_else(|| CommandError::Invalid("no file reference".into()))?;
        let (name, bytes) = self.blob(&el).ok_or_else(|| CommandError::Invalid(format!("the file (tab {el}) was not scraped")))?;
        if Self::flag(f, "createComposite") {
            fr.notes.push("composite part imported as separate parts".into());
        }
        let units = if Self::flag(f, "specifyUnits") {
            match Self::text(f, "unit").as_str() {
                "CENTIMETER" => Some(ImportUnit::Centimeter),
                "METER" => Some(ImportUnit::Meter),
                "INCH" => Some(ImportUnit::Inch),
                "FOOT" => Some(ImportUnit::Foot),
                _ => Some(ImportUnit::Millimeter),
            }
        } else {
            None
        };
        let id = self.feature_id(fid);
        self.s.run(&AddImport { element: self.el, feature: id, file_name: name, bytes: std::sync::Arc::new(bytes), y_axis_up: Self::flag(f, "yAxisIsUp"), units })?;
        // Without Flatten, Onshape's Part Studio holds each distinct part of a STEP assembly
        // once, in its own coordinates (its assembly places them), and its assemblies' instances
        // are placed for that: read it the same way.
        let current = self.s.doc.element(self.el).and_then(|e| e.feature(id)).and_then(|x| match &x.kind {
            FeatureKind::Import(x) => Some(x.clone()),
            _ => None,
        });
        if let Some(mut x) = current
            && !Self::flag(f, "flatten")
            && x.format == cadrs_core::import::ImportFormat::Step
        {
            x.structure = Some(cadrs_core::import::ImportMode::AtOrigin);
            self.s.run(&cadrs_core::commands::SetFeature { element: self.el, feature: id, kind: FeatureKind::Import(x), label: "Import".into() })?;
        }
        self.finish_added(f, fid, id, fr)
    }

    /// Onshape's Derived (`importDerived`): parts of another Part Studio, of this document or
    /// another (imported to the same store, with ids derived the same way).
    pub(crate) fn import_derived(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let p = param(f, "partStudio").ok_or_else(|| CommandError::Invalid("no Part Studio reference".into()))?;
        let (doc, el) = namespace_ref(p["namespace"].as_str().unwrap_or_default());
        let el = el.ok_or_else(|| CommandError::Invalid("no Part Studio reference".into()))?;
        let src_doc = doc.clone().unwrap_or_else(|| self.doc_id().to_string());
        // Another document's: at the version Onshape pins, when that document's import has it.
        let version = p["namespace"].as_str().unwrap_or_default().split("::").find_map(|s| s.strip_prefix('v')).map(String::from);
        let pinned = match (&doc, &version, &self.options.store) {
            (Some(d), Some(v), Some(store)) => {
                let document = DocumentId::from_u128(stable_u128(&[d]));
                let vid = crate::import::version_id(d, v);
                cadrs_core::history_log::HistoryLog::load(store, document).ok().flatten().filter(|log| log.version(vid).is_some()).map(|_| (store.clone(), document, vid))
            }
            _ => None,
        };
        if doc.is_some() && pinned.is_none() {
            fr.notes.push("from another document, at its current state (Onshape pins a version)".into());
        }
        if Self::text(f, "placement") != "AT_ORIGIN" && !Self::text(f, "placement").is_empty() {
            fr.notes.push(format!("placement {} imported as at origin", Self::text(f, "placement")));
        }
        let document = doc.map(|d| DocumentId::from_u128(stable_u128(&[&d])));
        let element = ElementId::from_u128(stable_u128(&[&src_doc, &el]));
        // The parts it picks: those the picked bodies' features made (or went into) in the source
        // studio, found among the source's parts once the reference resolves.
        let mut wanted = Vec::new();
        for q in p["partQuery"]["queries"].as_array().into_iter().flatten() {
            let Some(v) = q["queryString"].as_str().and_then(|s| crate::query::decode(s).ok().flatten()) else { continue };
            if let Some(op) = v.get("operationId").and_then(Q::as_str) {
                wanted.push(FeatureId::from_u128(stable_u128(&[&src_doc, &el, op_feature(op)])));
            }
        }
        let mut x = DerivedFeature::new(document, element);
        x.include_connectors = Self::flag(f, "includeMateConnectors");
        let id = self.feature_id(fid);
        match pinned {
            Some((store, document, vid)) => {
                let r = cadrs_core::external::SourceRef::version(Some(document), element, vid);
                let mut res = cadrs_core::external::Resolver::new(store);
                let got = cadrs_core::derived::resolve(&mut res, &self.s.doc, None, x, r).map_err(CommandError::from)?;
                self.s.run(&cadrs_core::derived::AddDerived { element: self.el, feature: id, derived: got.derived, links: got.links })?;
            }
            None => self.s.run(&AddFeature::derived(self.el, id, x))?,
        }
        if let Some(root) = self.raw_root() {
            let lazy = crate::eval::LazySource::new(root, src_doc.clone(), el.clone());
            self.derived.insert(fid.to_string(), (id, std::sync::Arc::new(lazy)));
        }
        if !wanted.is_empty() {
            let resolved = self.s.doc.element(self.el).and_then(|e| e.feature(id)).and_then(|f| match &f.kind {
                FeatureKind::Derived(x) => Some((**x).clone()),
                _ => None,
            });
            if let Some(mut x) = resolved {
                let source = cadrs_core::derived::source_parts(&x).unwrap_or_default();
                let picked: Vec<PartId> = source.iter().map(|(p, _)| *p).filter(|p| wanted.contains(&p.feature)).collect();
                if picked.is_empty() {
                    fr.notes.push("its part selection could not be matched: all parts derived".into());
                } else if picked.len() < source.len() {
                    x.selection = cadrs_core::derived::DerivedSelection { all: false, parts: picked, ..Default::default() };
                    self.s.run(&cadrs_core::commands::SetFeature { element: self.el, feature: id, kind: FeatureKind::Derived(Box::new(x)), label: "Derived".into() })?;
                }
            }
        }
        self.finish_added(f, fid, id, fr)
    }

    /// Names a feature added by its own command as in Onshape and checks it built.
    fn finish_added(&mut self, f: &Value, fid: &str, id: FeatureId, fr: &mut FeatureReport) -> Result<(), CommandError> {
        if let Some(name) = f["name"].as_str() {
            self.s.run(&RenameFeature { element: self.el, feature: id, name: name.to_string() })?;
        }
        self.features.insert(fid.to_string(), id);
        self.parts();
        if let Some(e) = self.rebuild_error(id) {
            fr.notes.push(format!("rebuild error: {e}"));
        }
        if !fr.notes.is_empty() && fr.outcome == Outcome::Full {
            fr.outcome = Outcome::Partial;
        }
        Ok(())
    }
}

impl PartStudio<'_> {
    /// Onshape's Hole: holes at sketch points, simple, counterbored or countersunk, blind, through
    /// all or up to next.
    pub(crate) fn hole(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        use cadrs_core::applied::{HoleFeature, HolePoint};
        use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStart, HoleStyle, Length};
        let mut x = HoleFeature::default();
        let mut lost = 0;
        for p in refs::picks(param(f, "locations")) {
            let Pick::Query(q) = p else {
                lost += 1;
                continue;
            };
            let found = (|| {
                if q.get("queryType").and_then(Q::as_str) != Some("SKETCH_ENTITY") {
                    return None;
                }
                let (map, _) = self.sketches.get(op_feature(q.get("operationId")?.as_str()?))?;
                let point = *map.points.get(q.get("sketchEntityId")?.as_str()?)?;
                Some(HolePoint { sketch: map.feature, point })
            })();
            match found {
                Some(h) if !x.points.contains(&h) => x.points.push(h),
                Some(_) => {}
                None => lost += 1,
            }
        }
        if x.points.is_empty() {
            return Err(CommandError::Invalid("its locations could not be translated".into()));
        }
        // Onshape drills each hole square to the face at its point (`locationSignatures`); cadrs
        // along the sketch's normal. Only holes that agree can be imported.
        let normals: Vec<[f64; 3]> = x
            .points
            .iter()
            .filter_map(|h| self.s.doc.element(self.el)?.feature(h.sketch)?.sketch()?.plane)
            .map(|p| {
                let fr = p.frame();
                [fr.u[1] * fr.v[2] - fr.u[2] * fr.v[1], fr.u[2] * fr.v[0] - fr.u[0] * fr.v[2], fr.u[0] * fr.v[1] - fr.u[1] * fr.v[0]]
            })
            .collect();
        let sigs = param(f, "locationSignatures").and_then(|p| p["expression"].as_str().or_else(|| p["value"].as_str())).unwrap_or_default().to_string();
        for dir in sigs.split("\"direction\"").skip(1) {
            let nums: Vec<f64> = dir.split(']').next().unwrap_or_default().trim_start_matches([' ', ':', '[']).split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if nums.len() == 3 && !normals.iter().any(|n| (n[0] * nums[0] + n[1] * nums[1] + n[2] * nums[2]).abs() > 1.0 - 1e-6) {
                return Err(CommandError::Invalid("its holes go square to faces, not along the sketch's normal (cadrs drills along the normal)".into()));
            }
        }
        Self::lost_note(fr, lost, "locations");
        let len = |this: &Self, id: &str| -> Result<Length, CommandError> {
            let (v, _) = this.quantity(f, id, Quantity::Length)?;
            Ok(Length::mm(v))
        };
        let deg = |this: &Self, id: &str| -> Result<Length, CommandError> {
            let (v, _) = this.quantity(f, id, Quantity::Angle)?;
            Ok(Length::deg(v))
        };
        let v3 = |id: &str| if param(f, &format!("{id}V3")).is_some() { format!("{id}V3") } else { id.to_string() };
        let mut spec = HoleSpec::new(String::new());
        spec.style = match Self::text(f, "style").as_str() {
            "C_BORE" => HoleStyle::Counterbore,
            "C_SINK" => HoleStyle::Countersink,
            _ => HoleStyle::Simple,
        };
        spec.diameter = len(self, &v3("holeDiameter"))?;
        spec.size = spec.diameter.expr.clone();
        spec.depth = len(self, &v3("holeDepth"))?;
        spec.tip_angle = deg(self, &v3("tipAngle")).unwrap_or(Length::deg(118.0));
        if spec.style == HoleStyle::Counterbore {
            spec.cbore_diameter = len(self, &v3("cBoreDiameter"))?;
            spec.cbore_depth = len(self, &v3("cBoreDepth"))?;
        }
        if spec.style == HoleStyle::Countersink {
            spec.csink_diameter = len(self, &v3("cSinkDiameter"))?;
            spec.csink_angle = deg(self, &v3("cSinkAngle"))?;
        }
        spec.end = match Self::text(f, "endStyle").as_str() {
            "THROUGH" => HoleEnd::ThroughAll,
            "UP_TO_NEXT" => HoleEnd::UpToNext,
            "BLIND" => HoleEnd::Blind,
            other => {
                fr.notes.push(format!("end {other} imported as blind"));
                HoleEnd::Blind
            }
        };
        spec.start = if Self::text(f, "startStyle") == "SKETCH" { HoleStart::SketchPlane } else { HoleStart::Part };
        if Self::text(f, "holeType") == "TAPPED" || Self::text(f, "styleV2").contains("TAP") {
            fr.notes.push("tapped hole imported as drilled".into());
        }
        x.spec = spec;
        x.flip = Self::flag(f, "oppositeDirection");
        let mut lost = 0;
        x.merge_scope = self.parts_param(f, "scope", &mut lost);
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            eprintln!("HOLESCOPE {:?}", x.merge_scope);
        }
        self.add(f, fid, "Hole", FeatureKind::Hole(x), fr)
    }
}

fn op_of(s: &str) -> BooleanOp {
    match s {
        "ADD" => BooleanOp::Add,
        "REMOVE" => BooleanOp::Remove,
        "INTERSECT" => BooleanOp::Intersect,
        _ => BooleanOp::New,
    }
}

/// The parts a body query names: those the operation made, else those it went into.
/// The cadrs instance number of the copy Onshape names `name` (its `instanceName`) in the
/// pattern, mirror or transform `op`. A cadrs pattern numbers its instances by grid position
/// (`index + 1`, the seed being index 0, or the middle one when centered); Onshape by offset
/// from the seed (1, 2, …, and -1, -2, … on the other side of a centered one). Mirrors and
/// transforms make instance 1.
pub(crate) fn cadrs_instance(doc: &[cadrs_core::document::Feature], op: FeatureId, name: &str) -> Option<u32> {
    let o: i64 = name.parse().ok()?;
    match doc.iter().find(|f| f.id == op).map(|f| &f.kind) {
        Some(FeatureKind::Pattern(x)) if matches!(x.kind, PatternKind::Linear | PatternKind::Circular) => {
            let n = i64::from(x.first.count.max(1));
            let shift = if x.first.centered { (n - 1) / 2 } else { 0 };
            u32::try_from(o + shift + 1).ok()
        }
        _ => u32::try_from(o).ok(),
    }
}

fn body_parts_in(q: &Q, features: &std::collections::HashMap<String, FeatureId>, parts: &[cadrs_core::parts::Part], doc: &[cadrs_core::document::Feature]) -> Vec<PartId> {
    // A copy a pattern, mirror or transform made: the copies (of the seeds it was derived
    // from) in instance `instanceName` (their faces carry the instance number), else all
    // copies of those seeds.
    if q.get("queryType").and_then(Q::as_str) == Some("COPY")
        && let Some(f) = q.get("operationId").and_then(Q::as_str).and_then(|op| features.get(op_feature(op)))
    {
        let derived: Vec<&Q> = match q.get("derivedFrom") {
            Some(Q::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
            None => Vec::new(),
        };
        let seeds: Vec<PartId> = derived.into_iter().flat_map(|d| body_parts_in(d, features, parts, doc)).collect();
        let copies: Vec<&cadrs_core::parts::Part> = parts.iter().filter(|p| p.id.feature == *f && p.source.is_some_and(|s| seeds.contains(&s))).collect();
        let k: Option<u32> = q.get("instanceName").and_then(Q::as_str).and_then(|s| cadrs_instance(doc, *f, s));
        let in_k: Vec<PartId> = copies
            .iter()
            .filter(|p| {
                k.is_some_and(|k| {
                    p.solid.faces.iter().any(|x| matches!(x.name.origin, cadrs_kernel::naming::FaceOrigin::Instance { instance, .. } if instance == k))
                })
            })
            .map(|p| p.id)
            .collect();
        if !in_k.is_empty() {
            return in_k;
        }
        if !copies.is_empty() {
            return copies.iter().map(|p| p.id).collect();
        }
    }
    // A body derived from others (a boolean's result, a copy): what it came from.
    let qtype = q.get("queryType").and_then(Q::as_str);
    if matches!(qtype, Some("MERGE" | "COPY" | "SPLIT")) {
        let derived: Vec<&Q> = match q.get("derivedFrom") {
            Some(Q::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
            None => Vec::new(),
        };
        let mut out: Vec<PartId> = derived.into_iter().flat_map(|d| body_parts_in(d, features, parts, doc)).collect();
        out.dedup();
        // A copy (a pattern's, a mirror's, a Transform's Copy part): the parts the copying
        // feature made of those seeds, in the instance named if there are several.
        if qtype == Some("COPY")
            && let Some(f) = q.get("operationId").and_then(Q::as_str).and_then(|op| features.get(op_feature(op)))
        {
            let made: Vec<&cadrs_core::parts::Part> = parts.iter().filter(|p| p.id.feature == *f).collect();
            let of_seeds: Vec<&cadrs_core::parts::Part> = made.iter().copied().filter(|p| p.source.is_some_and(|s| out.contains(&s))).collect();
            let copies = if of_seeds.is_empty() { made } else { of_seeds };
            if !copies.is_empty() {
                let k: Option<usize> = q.get("instanceName").and_then(Q::as_str).and_then(|s| s.parse().ok());
                // A body of an imported file by Onshape's tag for it (which cadrs can't read):
                // any of the copies, not the instance's (a Derived feature's one instance
                // holds them all).
                let tagged = q.get("derivedFrom").is_some_and(|d| {
                    let d = d.items().first().unwrap_or(d);
                    d.get("importTag").is_some()
                });
                if tagged {
                    return copies.iter().map(|p| p.id).collect();
                }
                if copies.len() > 1
                    && let Some(p) = k.and_then(|k| copies.get(k.saturating_sub(1)))
                {
                    return vec![p.id];
                }
                return copies.iter().map(|p| p.id).collect();
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    let Some(op) = q.get("operationId").and_then(Q::as_str) else { return Vec::new() };
    let Some(f) = features.get(op_feature(op)) else { return Vec::new() };
    let made: Vec<PartId> = parts.iter().filter(|p| p.id.feature == *f).map(|p| p.id).collect();
    if !made.is_empty() {
        return made;
    }
    parts.iter().filter(|p| p.features.contains(f)).map(|p| p.id).collect()
}


impl PartStudio<'_> {
    /// A Transform by mate connectors whose base connector cadrs couldn't place (it is on an
    /// entity of an imported file, named by Onshape's own tag): the vertex and flat face of the
    /// moved part it must be on, found as the ones whose connector carries the part onto where
    /// Onshape's final model has it. Places the connector there; false when none does.
    fn solve_base_connector(&mut self, x: &TransformFeature) -> bool {
        let (Some(ConnectorRef::Feature(base)), Some(ConnectorRef::Feature(dest))) = (x.from, x.to) else { return false };
        let Some(FeatureKind::MateConnector(mc)) = self.s.doc.element(self.el).and_then(|e| e.feature(base)).map(|f| f.kind.clone()) else { return false };
        if mc.origin.is_some() {
            return false;
        }
        let parts = self.parts();
        let Some(dest_frame) = self.connector_frame(dest) else { return false };
        let features: Vec<cadrs_core::document::Feature> = self.s.doc.element(self.el).map(|e| e.features().to_vec()).unwrap_or_default();
        // Onshape's final bodies: their vertices.
        let Some(details) = self.raw_json("bodydetails.json") else { return false };
        let mm = crate::import::point_mm;
        let bodies: Vec<Vec<[f64; 3]>> =
            details["bodies"].as_array().into_iter().flatten().map(|b| b["vertices"].as_array().into_iter().flatten().filter_map(|v| mm(&v["point"])).collect()).collect();
        let summary = |vs: &[[f64; 3]]| {
            let n = vs.len().max(1) as f64;
            let mut c = [0.0; 3];
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for v in vs {
                for i in 0..3 {
                    c[i] += v[i] / n;
                    lo[i] = lo[i].min(v[i]);
                    hi[i] = hi[i].max(v[i]);
                }
            }
            (vs.len(), c, lo, hi)
        };
        let targets: Vec<_> = bodies.iter().map(|b| summary(b)).collect();
        // The destination frame as the transform uses it (flipped, turned).
        let (mut du, mut dv) = (dest_frame.u, dest_frame.v);
        if x.flip_primary {
            dv = [-dv[0], -dv[1], -dv[2]];
        }
        let turn = x.secondary.degrees().to_radians();
        let (s, c) = turn.sin_cos();
        (du, dv) = ([du[0] * c + dv[0] * s, du[1] * c + dv[1] * s, du[2] * c + dv[2] * s], [dv[0] * c - du[0] * s, dv[1] * c - du[1] * s, dv[2] * c - du[2] * s]);
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let dn = cross(du, dv);
        let fits = |a: &(usize, [f64; 3], [f64; 3], [f64; 3]), b: &(usize, [f64; 3], [f64; 3], [f64; 3])| {
            a.0 == b.0 && (0..3).all(|i| (a.1[i] - b.1[i]).abs() < 1e-3 && (a.2[i] - b.2[i]).abs() < 1e-3 && (a.3[i] - b.3[i]).abs() < 1e-3)
        };
        for pid in &x.parts {
            let Some(part) = parts.iter().find(|p| p.id == *pid) else { continue };
            let n = part.solid.vertices.len();
            if !targets.iter().any(|t| t.0 == n) {
                continue;
            }
            for v in &part.solid.vertices {
                for face in &part.solid.faces {
                    if face.plane.is_none() || !v.name.faces.contains(&face.name) {
                        continue;
                    }
                    let fref = cadrs_core::document::FaceRef { part: part.id, face: face.name, seed: v.point };
                    let vref = cadrs_core::document::VertexRef { part: part.id, vertex: v.name, point: v.point };
                    let cand = MateConnectorFeature { origin: Some(ConnectorOrigin::Face(fref)), at: Some(ConnectorOrigin::Vertex(vref)), ..mc.clone() };
                    let Ok(b) = cand.frame(&features, &parts) else { continue };
                    let bn = cross(b.u, b.v);
                    // Where the part's vertices go: into the base frame, out of the destination's.
                    let moved: Vec<[f64; 3]> = part
                        .solid
                        .vertices
                        .iter()
                        .map(|p| {
                            let d = [p.point[0] - b.origin[0], p.point[1] - b.origin[1], p.point[2] - b.origin[2]];
                            let (l0, l1, l2) = (dot(d, b.u), dot(d, b.v), dot(d, bn));
                            [0, 1, 2].map(|i| dest_frame.origin[i] + du[i] * l0 + dv[i] * l1 + dn[i] * l2)
                        })
                        .collect();
                    let got = summary(&moved);
                    if targets.iter().any(|t| fits(t, &got)) {
                        let kind = FeatureKind::MateConnector(cand);
                        if self.s.run(&cadrs_core::commands::SetFeature { element: self.el, feature: base, kind, label: "Mate connector".into() }).is_ok() {
                            // The connector's own report: placed (by the part its Transform moves).
                            let name = self.s.doc.element(self.el).and_then(|e| e.feature(base)).map(|f| f.name.clone()).unwrap_or_default();
                            if let Some(r) = self.report.features.iter_mut().rev().find(|r| r.name == name && r.kind == "mateConnector") {
                                r.outcome = crate::report::Outcome::Full;
                                r.notes = vec!["on an imported entity Onshape names by its own tag: placed where its final model puts the part a Transform moves by it".into()];
                            }
                            return true;
                        }
                        return false;
                    }
                }
            }
        }
        false
    }
}

impl PartStudio<'_> {
    /// A connector parameter's entity by Onshape's topology id for it (`features-geometry.json`)
    /// and where Onshape's final model has that (`bodydetails.json`, and `bodydetails-extra.json`
    /// for sheet bodies), in the owner part's body: for an entity cadrs can't name otherwise (an
    /// imported file's, by Onshape's own tag). A vertex at that point; a flat face in that plane
    /// (through the connector's own vertex, if it has one).
    fn connector_by_geometry(&mut self, f: &Value, param_id: &str) -> Option<ConnectorOrigin> {
        let geometry = self.raw_json("features-geometry.json")?;
        let fid = f["featureId"].as_str()?;
        fn find<'v>(fs: &'v Value, fid: &str) -> Option<&'v Value> {
            for x in fs.as_array().into_iter().flatten() {
                let m = if x["message"].is_object() { &x["message"] } else { x };
                if m["featureId"].as_str() == Some(fid) {
                    return Some(m);
                }
                if let Some(s) = find(&m["subFeatures"], fid) {
                    return Some(s);
                }
            }
            None
        }
        let feature = find(&geometry["features"], fid)?;
        let ids_of = |pid: &str| -> Vec<String> {
            feature["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| if p["message"].is_object() { &p["message"] } else { p })
                .find(|p| p["parameterId"].as_str() == Some(pid))
                .and_then(|p| p["queries"].as_array())
                .map(|qs| {
                    qs.iter()
                        .map(|q| if q["message"].is_object() { &q["message"] } else { q })
                        .flat_map(|q| q["geometryIds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let ids = ids_of(param_id);
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            eprintln!("GEOMETRY {fid} {param_id}: ids {ids:?}");
        }
        let [id] = ids.try_into().ok()?;
        let owner = ids_of("ownerPart").into_iter().next();
        let mut bodies: Vec<Value> = Vec::new();
        for file in ["bodydetails.json", "bodydetails-extra.json"] {
            if let Some(d) = self.raw_json(file) {
                bodies.extend(d["bodies"].as_array().into_iter().flatten().cloned());
            }
        }
        let mm = crate::import::point_mm;
        // The entity, in the owner's body when that is known (ids are a body's own).
        let in_body = |b: &Value| owner.as_deref().is_none_or(|o| b["id"].as_str() == Some(o));
        let entity = |kind: &str, id: &str| bodies.iter().filter(|b| in_body(b)).find_map(|b| b[kind].as_array()?.iter().find(|x| x["id"].as_str() == Some(id)).cloned());
        let parts = self.parts();
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt() < 1e-3;
        let vertex_at = |p: [f64; 3]| {
            parts.iter().find_map(|part| {
                part.solid.vertices.iter().find(|v| near(v.point, p)).map(|v| (part, v))
            })
        };
        if let Some(v) = entity("vertices", &id) {
            let p = mm(&v["point"])?;
            let (part, v) = vertex_at(p)?;
            return Some(ConnectorOrigin::Vertex(cadrs_core::document::VertexRef { part: part.id, vertex: v.name, point: v.point }));
        }
        let face = entity("faces", &id)?;
        let s = &face["surface"];
        if !s["type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("plane")) {
            return None;
        }
        let (o, n) = (mm(&s["origin"])?, mm(&s["normal"]).map(|v| v.map(|x| x / 1000.0))?);
        // The connector's own vertex picks the face among those in the plane.
        let at = ids_of("secondaryOriginQuery").into_iter().next().and_then(|v| entity("vertices", &v)).and_then(|v| mm(&v["point"]));
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        parts.iter().find_map(|part| {
            part.solid.faces.iter().enumerate().find_map(|(i, x)| {
                let pl = x.plane?;
                let pn = pl.normal();
                let len = dot(pn, pn).sqrt();
                if (dot(pn, n) / len).abs() < 1.0 - 1e-6 || (dot([pl.origin[0] - o[0], pl.origin[1] - o[1], pl.origin[2] - o[2]], n)).abs() > 1e-3 {
                    return None;
                }
                let on = match at {
                    Some(p) => part.solid.vertices.iter().any(|v| near(v.point, p) && v.name.faces.contains(&x.name)),
                    None => true,
                };
                on.then(|| ConnectorOrigin::Face(cadrs_core::document::FaceRef { part: part.id, face: x.name, seed: part.solid.face_point(i).unwrap_or(o) }))
            })
        })
    }
}

impl PartStudio<'_> {
    /// The mate connectors a parameter's queries pick: a Mate connector feature's
    /// (`<id>.mateConnectorOp`), or the copy a Transform made of one (`<id>.opPattern`).
    fn connector_entities(&self, f: &Value) -> Vec<ConnectorRef> {
        let mut out = Vec::new();
        for q in param(f, "entities").and_then(|p| p["queries"].as_array()).into_iter().flatten() {
            let Some(v) = q["queryString"].as_str().and_then(|s| crate::query::decode(s).ok().flatten()) else { continue };
            let Some(op) = v.get("operationId").and_then(Q::as_str) else { continue };
            let Some(id) = self.features.get(op_feature(op)) else { continue };
            let kind = self.s.doc.element(self.el).and_then(|e| e.feature(*id)).map(|x| &x.kind);
            let connector = match kind {
                Some(FeatureKind::MateConnector(_)) => true,
                Some(FeatureKind::Transform(t)) => !t.connectors.is_empty() && t.copies(),
                _ => false,
            };
            if connector && !out.contains(&ConnectorRef::Feature(*id)) {
                out.push(ConnectorRef::Feature(*id));
            }
        }
        out
    }
}
