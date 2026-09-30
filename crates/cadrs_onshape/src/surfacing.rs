//! Onshape's Thicken, Helix, Fill and Sweep features (`reference/onshape/surfacing.md`) onto
//! cadrs's. A child of `features`, so it shares its parameter helpers.

use cadrs_core::advanced::{PathRef, ProfileControl, SweepFeature};
use cadrs_core::command::CommandError;
use cadrs_core::document::{BodyType, BooleanOp, FeatureKind, ThinWall};
use cadrs_core::surfacing::{Continuity, FillEdge, FillFeature, HelixFeature, HelixPath, HelixType, ThickenFeature};
use cadrs_sketch::units::Quantity;
use serde_json::Value;

use super::op_of;
use crate::import::PartStudio;
use crate::query::Value as Q;
use crate::refs::{self, Pick, op_feature};
use crate::report::FeatureReport;
use crate::sketch::param;

impl PartStudio<'_> {
    /// A length, or `None` (with a note) when it can't be evaluated.
    fn length_or(&self, f: &Value, id: &str, fallback: f64) -> (f64, String) {
        self.quantity(f, id, Quantity::Length).unwrap_or((fallback, format!("{fallback} mm")))
    }

    // -----------------------------------------------------------------------------------------
    // Thicken

    pub(crate) fn thicken(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = ThickenFeature { op: op_of(&Self::text(f, "operationType")), ..Default::default() };
        let mut lost = 0;
        let parts = self.parts();
        for p in refs::picks(param(f, "entities")) {
            match p {
                Pick::WholeSketch(s) => match self.features.get(&s) {
                    Some(id) => x.sketches.push(*id),
                    None => lost += 1,
                },
                Pick::Query(q) => {
                    if q.get("entityType").and_then(Q::as_str) == Some("BODY") {
                        continue;
                    }
                    let rs = self.region_of(&q);
                    if !rs.is_empty() {
                        x.regions.extend(rs);
                        continue;
                    }
                    let model = self.model(&parts);
                    match model.face(&q) {
                        Some(face) if !x.faces.iter().any(|g| g.face == face.face) => x.faces.push(face),
                        Some(_) => {}
                        None => {
                            lost += 1;
                            if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
                                eprintln!("LOSTFACE {} | {}", f["name"], self.describe(&q));
                            }
                        }
                    }
                }
                Pick::Opaque(_) => lost += 1,
            }
        }
        // Whole surface bodies.
        let has_bodies = refs::picks(param(f, "entities")).iter().any(|p| matches!(p, Pick::Query(q) if q.get("entityType").and_then(Q::as_str) == Some("BODY")));
        if has_bodies {
            let mut body_lost = 0;
            let ids = self.parts_param_where(f, "entities", "BODY", &mut body_lost);
            x.parts.extend(ids);
            lost += body_lost;
        }
        if x.is_empty() {
            return Err(CommandError::Invalid(format!("none of its {} selections could be translated", refs::picks(param(f, "entities")).len())));
        }
        Self::lost_note(fr, lost, "selections");
        x.mid_plane = Self::flag(f, "midplane");
        if x.mid_plane {
            (x.thickness1, x.thickness1_expr) = self.length_or(f, "thickness", 5.0);
            x.thickness2 = 0.0;
            x.thickness2_expr = "0 mm".into();
        } else {
            (x.thickness1, x.thickness1_expr) = self.length_or(f, "thickness1", 5.0);
            (x.thickness2, x.thickness2_expr) = self.length_or(f, "thickness2", 0.0);
        }
        x.flip = Self::flag(f, "oppositeDirection");
        x.keep_tools = Self::flag(f, "keepTools");
        if x.op != BooleanOp::New {
            let mut l = 0;
            x.merge_scope = self.parts_param(f, "booleanScope", &mut l);
            x.merge_all = Self::flag(f, "defaultScope");
        }
        self.add(f, fid, "Thicken", FeatureKind::Thicken(x), fr)?;
        self.debug_parts(f);
        Ok(())
    }

    /// The parts a parameter's picks of `entity_type` name.
    fn parts_param_where(&mut self, f: &Value, id: &str, entity_type: &str, lost: &mut usize) -> Vec<cadrs_core::ids::PartId> {
        let mut only = f.clone();
        if let Some(params) = only["parameters"].as_array_mut()
            && let Some(p) = params.iter_mut().find(|p| p["parameterId"].as_str() == Some(id))
            && let Some(qs) = p["queries"].as_array_mut()
        {
            qs.retain(|q| {
                let s = q["queryString"].as_str().unwrap_or_default();
                crate::query::decode(s).ok().flatten().is_some_and(|v| v.get("entityType").and_then(Q::as_str) == Some(entity_type))
            });
        }
        self.parts_param(&only, id, lost)
    }

    // -----------------------------------------------------------------------------------------
    // Helix

    pub(crate) fn helix(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = HelixFeature::default();
        match Self::text(f, "axisType").as_str() {
            "SURFACE" | "" => {
                x.helix_type = HelixType::CylinderCone;
                let parts = self.parts();
                let q = refs::picks(param(f, "entities")).into_iter().find_map(|p| match p {
                    Pick::Query(q) => Some(q),
                    _ => None,
                });
                x.face = q.and_then(|q| self.model(&parts).face(&q));
                if x.face.is_none() {
                    return Err(CommandError::Invalid("its face could not be translated".into()));
                }
            }
            "AXIS" => {
                x.helix_type = HelixType::Axis;
                x.axis = self.axis_param(f, "axis");
                (x.radius, x.radius_expr) = self.length_or(f, "startRadius", 25.0);
                if x.axis.is_none() {
                    return Err(CommandError::Invalid("its axis could not be translated".into()));
                }
            }
            other => {
                x.helix_type = HelixType::Circle;
                x.axis = self.axis_param(f, "edge");
                if x.axis.is_none() {
                    return Err(CommandError::Invalid(format!("its {} could not be translated", other.to_lowercase())));
                }
            }
        }
        x.path = match Self::text(f, "pathType").as_str() {
            "PITCH" => HelixPath::Pitch,
            "TURNS_PITCH" => HelixPath::TurnsAndPitch,
            _ => HelixPath::Turns,
        };
        let e = Self::expr(f, "revolutions");
        x.revolutions = crate::import::eval_expr(&e, Quantity::Count, &self.vars).unwrap_or(4.0);
        x.revolutions_expr = format!("{}", x.revolutions);
        (x.pitch, x.pitch_expr) = self.length_or(f, "helicalPitch", 25.0);
        (x.height, x.height_expr) = self.length_or(f, "height", 25.0);
        if Self::text(f, "startType") == "START_POINT" {
            fr.notes.push("start point not imported: start angle 0".into());
        } else if let Ok((a, ae)) = self.quantity(f, "startAngle", Quantity::Angle) {
            x.start_angle = a;
            x.start_angle_expr = ae;
        }
        if Self::text(f, "endType") == "END_POINT" {
            fr.notes.push("end point not imported: the face's height".into());
        }
        if Self::flag(f, "endRadToggle") {
            fr.notes.push("end radius not imported".into());
        }
        x.clockwise = Self::text(f, "handedness") != "CCW";
        x.flip = Self::flag(f, "oppositeDirection");
        let id = self.feature_id(fid);
        self.add(f, fid, "Helix", FeatureKind::Helix(x), fr)?;
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            let b = cadrs_core::rebuild::build(self.s.doc.element(self.el).map(|e| e.features()).unwrap_or(&[]));
            eprintln!("HELIX {} {:?}", f["name"], b.curves.get(&id));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Fill

    pub(crate) fn fill(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = FillFeature { add: Self::text(f, "surfaceOperationType") == "ADD", ..Default::default() };
        let items: Vec<Value> = param(f, "edges").and_then(|p| p["items"].as_array()).cloned().unwrap_or_default();
        let mut lost = 0;
        let mut total = 0;
        let parts = self.parts();
        for item in &items {
            let continuity = match param(item, "continuity").and_then(|p| p["value"].as_str()) {
                Some("G1") => Continuity::Tangency,
                Some("G2") => Continuity::Curvature,
                _ => Continuity::Position,
            };
            for p in refs::picks(param(item, "entities")) {
                total += 1;
                let Pick::Query(q) = p else {
                    lost += 1;
                    continue;
                };
                if let Some((sketch, curve)) = self.sketch_curve(&q) {
                    x.edges.push(FillEdge::SketchCurve { sketch, curve });
                    x.continuity.push(continuity);
                    continue;
                }
                // The loop a sweep's free profile vertex traces: the surface's open boundary
                // through where the vertex is.
                let traced = self.swept_vertex_point(&q).map(|(op, at)| self.model(&parts).boundary_loop_through(op, at)).unwrap_or_default();
                if !traced.is_empty() {
                    for e in traced {
                        if !x.edges.iter().any(|g| matches!(g, FillEdge::Edge(h) if h.edge == e.edge)) {
                            x.edges.push(FillEdge::Edge(e));
                            x.continuity.push(continuity);
                        }
                    }
                    continue;
                }
                let mut found = self.model(&parts).edges(&q);
                if found.len() > 1 {
                    found = self.surface_cap_edges(&q, &parts, found);
                }
                if found.is_empty() {
                    lost += 1;
                }
                for e in found {
                    if !x.edges.iter().any(|g| matches!(g, FillEdge::Edge(h) if h.edge == e.edge)) {
                        x.edges.push(FillEdge::Edge(e));
                        x.continuity.push(continuity);
                    }
                }
            }
        }
        if x.edges.is_empty() {
            return Err(CommandError::Invalid(format!("none of its {total} boundary selections could be translated")));
        }
        Self::lost_note(fr, lost, "boundary selections");
        if x.continuity.iter().any(|c| *c != Continuity::Position) {
            fr.notes.push("tangency and curvature continuity built as position".into());
        }
        if Self::flag(f, "addGuides") {
            fr.notes.push("guides not imported".into());
        }
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            for e in &x.edges {
                if let FillEdge::Edge(r) = e
                    && let Some(p) = parts.iter().find(|p| p.solid.edge(&r.edge).is_some())
                {
                    let pts = &p.solid.edge(&r.edge).expect("found").points;
                    eprintln!("FILL {} edge {:?} → {:?}", f["name"], pts.first(), pts.last());
                }
            }
        }
        if x.add && !Self::flag(f, "defaultSurfaceScope") {
            let mut l = 0;
            x.merge_scope = self.parts_param(f, "booleanSurfaceScope", &mut l);
        }
        self.add(f, fid, "Fill", FeatureKind::Fill(x), fr)?;
        self.debug_parts(f);
        Ok(())
    }

    /// The parts after a feature (`CADRS_ONSHAPE_DEBUG`).
    fn debug_parts(&mut self, f: &Value) {
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            eprintln!("AFTER {}", f["name"]);
            for p in self.parts() {
                eprintln!("   part {:?} {:?} {:.1} {:?}", p.kind, p.id.index, p.mass.as_ref().map_or(0.0, |m| m.volume), p.solid.bounds());
            }
        }
    }

    /// A `CAP_EDGE` of a surface extrude (it has no cap faces, so the query finds the side
    /// face's edges): its open edges at the end the query names (`isStart`: on the sketch
    /// plane, else the far end).
    fn surface_cap_edges(&self, q: &Q, parts: &[cadrs_core::parts::Part], found: Vec<cadrs_core::EdgeRef>) -> Vec<cadrs_core::EdgeRef> {
        if q.get("queryType").and_then(Q::as_str) != Some("CAP_EDGE") {
            return found;
        }
        let Some(op) = q.get("operationId").and_then(Q::as_str).and_then(|o| self.features.get(op_feature(o))) else { return found };
        let el = self.s.doc.element(self.el);
        let Some(FeatureKind::Extrude(x)) = el.and_then(|e| e.feature(*op)).map(|f| &f.kind) else { return found };
        let Some(frame) = x.sketches().first().and_then(|s| el?.feature(*s)?.sketch()?.plane).map(|p| p.frame()) else { return found };
        let model = self.model(parts);
        let open: Vec<cadrs_core::EdgeRef> = found.iter().copied().filter(|e| model.is_open_edge(e)).collect();
        if open.is_empty() {
            return found;
        }
        let height = |e: &cadrs_core::EdgeRef| frame.distance(e.seed).abs();
        let start = q.get("isStart").and_then(Q::as_bool).unwrap_or(false);
        let best = open.iter().map(height).fold(if start { f64::MAX } else { f64::MIN }, |a, h| if start { a.min(h) } else { a.max(h) });
        open.into_iter().filter(|e| (height(e) - best).abs() < 1e-6 * (1.0 + best.abs())).collect()
    }

    /// For a `SWEPT_EDGE` of a Sweep made by a profile sketch vertex: the sweep's op and where
    /// the vertex is (world mm).
    fn swept_vertex_point(&self, q: &Q) -> Option<(uuid::Uuid, [f64; 3])> {
        if q.get("queryType").and_then(Q::as_str) != Some("SWEPT_EDGE") {
            return None;
        }
        let op = *self.features.get(op_feature(q.get("operationId")?.as_str()?))?;
        let doc = &self.s.doc;
        let el = doc.element(self.el)?;
        if !matches!(el.feature(op)?.kind, FeatureKind::Sweep(_)) {
            return None;
        }
        for d in q.get("disambiguationData").map(Q::items).unwrap_or_default() {
            for o in d.get("originals").map(Q::items).unwrap_or_default() {
                if o.get("entityType").and_then(Q::as_str) != Some("VERTEX") || o.get("queryType").and_then(Q::as_str) != Some("SKETCH_ENTITY") {
                    continue;
                }
                let (Some(sop), Some(id)) = (o.get("operationId").and_then(Q::as_str), o.get("sketchEntityId").and_then(Q::as_str)) else { continue };
                let Some((map, g)) = self.sketches.get(op_feature(sop)) else { continue };
                let Some(p) = map.points.get(id).and_then(|p| g.points.get(*p)) else { continue };
                let Some(plane) = el.feature(map.feature).and_then(|f| f.sketch()).and_then(|s| s.plane) else { continue };
                return Some((op.0, plane.frame().to_world(p.pos)));
            }
        }
        None
    }

    // -----------------------------------------------------------------------------------------
    // Sweep

    pub(crate) fn sweep(&mut self, f: &Value, fid: &str, fr: &mut FeatureReport) -> Result<(), CommandError> {
        let mut x = SweepFeature {
            body: match Self::text(f, "bodyType").as_str() {
                "SURFACE" => BodyType::Surface,
                "THIN" => BodyType::Thin,
                _ => BodyType::Solid,
            },
            ..Default::default()
        };
        x.op = if x.body == BodyType::Surface { BooleanOp::New } else { op_of(&Self::text(f, "operationType")) };
        if x.body == BodyType::Thin {
            let (t1, e1) = self.length_or(f, "thickness1", 5.0);
            let (t2, e2) = self.length_or(f, "thickness2", 0.0);
            x.thin = ThinWall { thickness1: t1, thickness1_expr: e1, thickness2: t2, thickness2_expr: e2, flip_wall: Self::flag(f, "flipWall"), mid_plane: Self::flag(f, "midplane") };
            if x.thin.mid_plane {
                (x.thin.thickness1, x.thin.thickness1_expr) = self.length_or(f, "thickness", 5.0);
            }
        }
        // The profiles: regions, faces or whole sketches; a surface's sketch curves (whole
        // sketches).
        let mut lost = 0;
        let key = if x.body == BodyType::Solid { "profiles" } else if x.body == BodyType::Surface { "surfaceProfiles" } else { "wallShape" };
        let mut picks = refs::picks(param(f, key));
        if x.body == BodyType::Thin {
            picks.extend(refs::picks(param(f, "profiles")));
        }
        let parts = self.parts();
        for p in picks {
            match p {
                Pick::WholeSketch(s) => match self.features.get(&s) {
                    Some(id) => x.sketches.push(*id),
                    None => lost += 1,
                },
                Pick::Query(q) => {
                    if let Some((sketch, _)) = self.sketch_curve(&q) {
                        if !x.sketches.contains(&sketch) {
                            x.sketches.push(sketch);
                        }
                        continue;
                    }
                    let rs = self.region_of(&q);
                    if !rs.is_empty() {
                        x.regions.extend(rs);
                        continue;
                    }
                    match self.model(&parts).face(&q) {
                        Some(face) => x.faces.push(face),
                        None => lost += 1,
                    }
                }
                Pick::Opaque(_) => lost += 1,
            }
        }
        if x.regions.is_empty() && x.sketches.is_empty() && x.faces.is_empty() {
            return Err(CommandError::Invalid("none of its profiles could be translated".into()));
        }
        // The path: sketch curves, part edges, or a curve feature's body (a Helix).
        let mut path_lost = 0;
        for p in refs::picks(param(f, "path")) {
            let Pick::Query(q) = p else {
                path_lost += 1;
                continue;
            };
            if q.get("entityType").and_then(Q::as_str) == Some("BODY")
                && let Some(op) = q.get("operationId").and_then(Q::as_str)
                && let Some(id) = self.features.get(op_feature(op))
            {
                x.path.push(PathRef::Curve(*id));
                continue;
            }
            if let Some((sketch, curve)) = self.sketch_curve(&q) {
                x.path.push(PathRef::SketchCurve { sketch, curve });
                continue;
            }
            let found = self.model(&parts).edges(&q);
            if found.is_empty() {
                path_lost += 1;
            }
            x.path.extend(found.into_iter().map(PathRef::Edge));
        }
        if x.path.is_empty() {
            return Err(CommandError::Invalid("its path could not be translated".into()));
        }
        Self::lost_note(fr, lost, "profile selections");
        Self::lost_note(fr, path_lost, "path selections");
        x.control = match Self::text(f, "profileControl").as_str() {
            "KEEP_ORIENTATION" => ProfileControl::KeepOrientation,
            "LOCK_DIRECTION" => {
                x.lock_direction = self.direction_param(f, "lockDirectionQuery");
                ProfileControl::LockDirection
            }
            "NONE" | "" => ProfileControl::None,
            other => {
                fr.notes.push(format!("profile control {} imported as none", other.to_lowercase()));
                ProfileControl::None
            }
        };
        if Self::flag(f, "hasTwist") {
            fr.notes.push("twist not imported".into());
        }
        if Self::flag(f, "hasScale") {
            fr.notes.push("scale not imported".into());
        }
        if x.op != BooleanOp::New {
            let mut l = 0;
            x.merge_scope = self.parts_param(f, "booleanScope", &mut l);
            x.merge_all = Self::flag(f, "defaultScope");
        }
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            eprintln!("SWEEP {} path {:?} op {:?} scope {:?}", f["name"], x.path, x.op, x.merge_scope);
        }
        self.add(f, fid, "Sweep", FeatureKind::Sweep(x), fr)?;
        if std::env::var_os("CADRS_ONSHAPE_DEBUG").is_some() {
            for p in self.parts() {
                eprintln!("   part {:?} {:.1} {:?}", p.id.index, p.mass.as_ref().map_or(0.0, |m| m.volume), p.solid.bounds());
            }
        }
        Ok(())
    }
}
