//! Rebuilding the sheet metal features after a Sheet metal model (P3I.5,
//! `crate::sheetmetal_tools`) and the ordinary features that act on an active model as sheet
//! metal (SM1.6, SM12). A child of the Sheet metal model's rebuild, so it shares its helpers
//! (the folded solid, the flat check).
//!
//! Every one of them adds a **step** to the definition of the active model its picks are on
//! (`cadrs_sheetmetal::definition`: a Bend, Jog, Tab, cut, corner break, face copy or relief
//! override, replayed by `cadrs_sheetmetal::model_edit` on the built model), then the one sheet
//! metal pipeline ([`Rebuilder::edit_sheet_metal`], `refold.rs`) makes the model, the flat
//! pattern and the folded parts again: the parts keep their ids and names (matched by their
//! walls), parts no flat part continues go, new ones are added. Picks are matched to the model by
//! where they were picked (the seed point), since a refold renames the faces.
//!
//! - **Finish sheet metal model** marks the models of its parts finished: the sheet metal
//!   features after it refuse them, the ordinary ones act on the solids, and the flat and table
//!   stay as they were (SM10). Rolling back or suppressing it brings the active model back by
//!   itself (the rebuild simply doesn't see it).
//! - **Extrude → Remove** whose targets are all active sheet metal parts cuts the walls it
//!   crosses perpendicular to them (SM1.6, SM12.1); **Fillet** and **Chamfer** of corner edges
//!   (through the thickness) of active sheet metal parts are corner breaks; **Face pattern** and
//!   **Face mirror** of walls copy them with their bends (SM12.2). Anything else falls through
//!   to the ordinary feature.

use super::*;
use cadrs_sheetmetal::definition::StepEdit;
use cadrs_sheetmetal::model_edit::{self as model_edit, BendSpec, CornerBreakKind, CutTool, EditError, JogSpec, Placement, Region3};
use cadrs_sheetmetal::model::{BendReliefOverride, CornerOverride};

use crate::applied::{ChamferType, EdgeOrFace, FilletMeasurement};
use crate::sheetmetal_tools::{
    AngleControl, BendFeature, CornerBreakFeature, JogBounding, JogFeature, LineRef, SheetMetalTool, TabFeature,
};

fn err(e: EditError) -> String {
    e.message().into()
}

/// The seed of a feature's persistent wall and joint ids.
fn seed_of(id: FeatureId) -> u64 {
    naming::stable_hash(id.0.as_bytes())
}

/// How close a pick must be to the definition (mm).
const PICK_TOL: f64 = 0.05;

/// The active model holding `part`.
fn context_of(state: &State, part: PartId) -> Option<usize> {
    state.sheet_metal.iter().position(|c| c.active && c.parts.iter().any(|(p, _)| *p == part))
}

fn need_context(state: &State, part: PartId) -> Result<usize, String> {
    context_of(state, part).ok_or_else(|| {
        if state.sheet_metal.iter().any(|c| c.parts.iter().any(|(p, _)| *p == part)) {
            "The sheet metal model is finished: its parts are ordinary solids now".to_string()
        } else {
            "Select a face of an active sheet metal part".to_string()
        }
    })
}

/// Every unchanged output field.
fn output(state: Arc<State>, owned: Vec<BodyId>) -> Output {
    Output {
        state,
        error: None,
        warning: None,
        contacts: None,
        owned,
        stage: None,
        axis: None,
        arrows: Vec::new(),
        dots: None,
        uses: Vec::new(),
    }
}

/// A line's two end points in space.
fn line_points(before: &[Feature], state: &State, l: &LineRef) -> Result<(P3, P3), String> {
    match l {
        LineRef::Sketch(c) => {
            let sk = before.iter().find(|f| f.id == c.sketch).and_then(|f| f.sketch()).ok_or("The bend line no longer exists")?;
            let frame = sk.plane.ok_or("The bend line's sketch has no plane")?.frame();
            let g = &sk.geometry;
            match g.curves.get(c.curve).map(|cv| &cv.kind) {
                Some(cadrs_sketch::CurveKind::Line { a, b }) => Ok((p3(frame.to_world(g.pos(*a))), p3(frame.to_world(g.pos(*b))))),
                Some(_) => Err("The bend line must be a line".into()),
                None => Err("The bend line no longer exists".into()),
            }
        }
        LineRef::Edge(e) => {
            let pts = edge_points(state, e).ok_or("The bend line no longer exists")?;
            Ok((p3(pts[0]), p3(pts[pts.len() - 1])))
        }
    }
}

/// The points of the edge nearest a reference's seed (on its part, or any part).
fn edge_points(state: &State, e: &crate::document::EdgeRef) -> Option<Vec<Vec3>> {
    let parts: Vec<&PartState> = match state.part(e.part) {
        Some(p) => vec![p],
        None => state.parts.iter().collect(),
    };
    parts
        .iter()
        .flat_map(|p| p.part.solid.edges.iter())
        .min_by(|a, b| a.distance(e.seed).total_cmp(&b.distance(e.seed)))
        .filter(|x| x.points.len() >= 2)
        .map(|x| x.points.clone())
}

/// A reference's direction: an edge's, or a flat face's normal.
fn reference_direction(state: &State, r: &EdgeOrFace) -> Result<V3, String> {
    let lost = || "The reference no longer exists".to_string();
    match r {
        EdgeOrFace::Edge(e) => {
            let pts = edge_points(state, e).ok_or_else(lost)?;
            let d = p3(pts[pts.len() - 1]) - p3(pts[0]);
            (d.norm() > 1e-9).then(|| d.normalize()).ok_or_else(lost)
        }
        EdgeOrFace::Face(f) => {
            let (part, i) = face_index(state, f).ok_or_else(lost)?;
            let pl = part.part.solid.faces[i].plane.ok_or("The reference face must be flat")?;
            Ok(v3(pl.normal()).normalize())
        }
    }
}

/// An angle in (0, π]: the representative of `a` modulo π.
fn half_turn(a: f64) -> f64 {
    let x = a.rem_euclid(std::f64::consts::PI);
    if x < 1e-9 { std::f64::consts::PI } else { x }
}

impl Rebuilder {
    /// A Bend's (or a Jog's) settings on its model: (the model's context, the spec).
    fn bend_spec(&self, before: &[Feature], state: &State, b: &BendFeature) -> Result<(usize, BendSpec), String> {
        let face = b.face.ok_or("Select a sheet metal face to bend")?;
        let ci = need_context(state, face.part)?;
        let model = &state.sheet_metal[ci].model;
        let (wall, other) = model_edit::wall_at(model, p3(face.seed), PICK_TOL).ok_or("The face to bend isn't a flat sheet metal wall")?;
        let line = line_points(before, state, b.line.as_ref().ok_or("Select a bend line")?)?;
        let t = model.params.thickness;
        let mut spec = BendSpec {
            wall,
            line,
            hold_opposite: b.hold_opposite,
            alignment: b.alignment,
            angle: b.angle.to_radians(),
            // The bend turns towards the picked face (its outward side).
            toward_material: other != b.opposite,
            line_height: if other { t } else { 0.0 },
            radius: (!b.use_model_radius).then_some(b.radius),
            k_factor: (!b.use_model_k).then_some(b.k_factor),
        };
        if b.control != AngleControl::BendAngle {
            let dir = reference_direction(state, b.reference.as_ref().ok_or("Select a reference")?)?;
            let (c, side) = model_edit::bend_frame(model, &spec).map_err(err)?;
            // The bent wall's direction (cos θ·c + sin θ·side) along the edge, or across the
            // face's normal.
            let theta0 = match b.reference {
                Some(EdgeOrFace::Face(_)) => half_turn((-dir.dot(&c)).atan2(dir.dot(&side)) + std::f64::consts::PI),
                _ => half_turn(dir.dot(&side).atan2(dir.dot(&c))),
            };
            spec.angle = match b.control {
                AngleControl::AlignToGeometry => theta0,
                _ => theta0 + b.angle.to_radians(),
            };
            if !(spec.angle > 1f64.to_radians() - 1e-9 && spec.angle < 359f64.to_radians() + 1e-9) {
                return Err("The bend angle must be between 1 and 359 degrees".into());
            }
        }
        Ok((ci, spec))
    }

    pub(in crate::rebuild) fn sheet_metal_tool(&mut self, before: &[Feature], id: FeatureId, name: &str, x: &SheetMetalTool, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let seed = seed_of(id);
        match x {
            SheetMetalTool::Finish(f) => {
                let mut next = (**state).clone();
                let mut all = (*next.sheet_metal).clone();
                let mut missing = 0;
                let mut done = 0;
                for p in &f.parts {
                    match all.iter_mut().find(|c| c.parts.iter().any(|(q, _)| q == p)) {
                        Some(c) => {
                            c.active = false;
                            done += 1;
                        }
                        None => missing += 1,
                    }
                }
                if done == 0 {
                    return Err("Select sheet metal parts".into());
                }
                next.sheet_metal = Arc::new(all);
                let mut o = output(Arc::new(next), Vec::new());
                o.warning = (missing > 0).then(|| "A selected part isn't a sheet metal part".into());
                Ok(o)
            }
            SheetMetalTool::Bend(b) => {
                let (ci, spec) = self.bend_spec(before, state, b)?;
                self.step(id, name, state, ci, StepEdit::Bend { spec, seed })
            }
            SheetMetalTool::Jog(j) => self.jog(before, id, name, j, state),
            SheetMetalTool::Tab(t) => self.tab(before, id, name, t, state),
            SheetMetalTool::Corner(c) => {
                let pick = c.corner.ok_or("Select a corner")?;
                let ci = need_context(state, pick.part())?;
                let ctx = &state.sheet_metal[ci];
                let (bends, _) = model_edit::corner_near(&ctx.model, &ctx.flat, p3(pick.seed())).ok_or("Select a corner where two bends meet")?;
                self.step(id, name, state, ci, StepEdit::CornerRelief(CornerOverride { bends, relief: c.relief }))
            }
            SheetMetalTool::BendRelief(b) => {
                let pick = b.end.ok_or("Select a bend relief")?;
                let ci = need_context(state, pick.part())?;
                let ctx = &state.sheet_metal[ci];
                let ((bend, end), _) = model_edit::bend_end_near(&ctx.model, p3(pick.seed())).ok_or("Select the end of a bend")?;
                self.step(id, name, state, ci, StepEdit::BendRelief(BendReliefOverride { bend, end, relief: b.relief }))
            }
            SheetMetalTool::CornerBreak(c) => {
                let picks: Vec<(PartId, Vec3)> = c.entities.iter().map(|e| (e.part(), e.seed())).collect();
                self.corner_breaks(id, name, &picks, &|beta| corner_kind(c, beta), state)
            }
        }
    }

    /// Adds a step to model `ci`'s definition and refolds it.
    fn step(&mut self, id: FeatureId, name: &str, state: &Arc<State>, ci: usize, edit: StepEdit) -> Result<Output, String> {
        self.edit_sheet_metal(id, name, state, ci, |ctx| {
            ctx.def.as_mut().expect("checked").push(name, edit);
            Ok(None)
        })
    }

    /// A Jog.
    fn jog(&mut self, before: &[Feature], id: FeatureId, name: &str, j: &JogFeature, state: &Arc<State>) -> Result<Output, String> {
        let (ci, spec) = self.bend_spec(before, state, &j.bend)?;
        let model = &state.sheet_metal[ci].model;
        let t = model.params.thickness;
        let offset = match j.bounding {
            JogBounding::Blind => j.offset,
            JogBounding::Thickness => j.factor * t,
            JogBounding::UpToEntity => {
                let target = j.up_to.ok_or("Select the entity to jog up to")?;
                let face = j.bend.face.ok_or("Select a sheet metal face to bend")?;
                let w = model.wall(spec.wall).ok_or("The face to bend isn't a sheet metal wall")?;
                let n = w.surface.normal().ok_or("The face to bend isn't flat")?;
                let side = if spec.toward_material { n } else { -n };
                let d = (p3(target.seed) - p3(face.seed)).dot(&side);
                d + if j.up_to_offset_on { j.up_to_offset } else { 0.0 }
            }
        };
        let s = JogSpec { bend: spec, offset, anchor: j.anchor, preserve_material: j.preserve_material };
        self.step(id, name, state, ci, StepEdit::Jog { spec: s, seed: seed_of(id) })
    }

    /// A Tab: the profiles added to the walls, then the clearance pockets cut.
    fn tab(&mut self, before: &[Feature], id: FeatureId, name: &str, x: &TabFeature, state: &Arc<State>) -> Result<Output, String> {
        let (groups, lost) = sweep_groups(before, &x.regions, &x.sketches, crate::document::BodyType::Solid);
        let mut regions: Vec<Region3> = Vec::new();
        for g in &groups {
            let f = &g.frame;
            for (_, r) in &g.regions {
                let ring = |l: &[cadrs_sketch::Vec2]| l.iter().map(|q| P2::new(q.x, q.y)).collect::<Vec<_>>();
                regions.push(Region3 {
                    origin: p3(f.origin),
                    x: v3(f.u).normalize(),
                    y: v3(f.v).normalize(),
                    polygon: Polygon::with_holes(ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect()),
                });
            }
        }
        if regions.is_empty() {
            return Err("The tab profile no longer exists".into());
        }
        let mut current = state.clone();
        let mut owned: Vec<BodyId> = Vec::new();
        let mut took = false;
        // The walls each model is to take the tab on.
        let contexts: Vec<usize> = (0..state.sheet_metal.len()).filter(|i| state.sheet_metal[*i].active).collect();
        for ci in contexts {
            let ctx = &current.sheet_metal[ci];
            let walls: Vec<cadrs_sheetmetal::WallId> = if x.flanges.is_empty() {
                ctx.model.walls.iter().map(|w| w.id).collect()
            } else {
                x.flanges
                    .iter()
                    .filter(|f| ctx.parts.iter().any(|(p, _)| *p == f.part))
                    .filter_map(|f| model_edit::wall_at(&ctx.model, p3(f.seed), PICK_TOL).map(|(w, _)| w))
                    .collect()
            };
            if walls.is_empty() {
                continue;
            }
            let mut model = ctx.model.clone();
            match model_edit::add_tab(&mut model, &regions, &walls) {
                Ok(_) => {
                    let o = self.step(id, name, &current, ci, StepEdit::Tab { regions: regions.clone(), walls })?;
                    if o.error.is_some() {
                        return Ok(o);
                    }
                    owned.extend(o.owned);
                    current = o.state;
                    took = true;
                }
                Err(EditError::NoTab) if x.flanges.is_empty() => {}
                Err(e) => return Err(err(e)),
            }
        }
        if !took {
            return Err(err(EditError::NoTab));
        }
        // The clearance pockets: the profile grown by the offset, through the tab's sheet.
        if !x.scope.is_empty() {
            let t = state.sheet_metal.iter().find(|c| c.active).map_or(1.0, |c| c.model.params.thickness);
            let reach = t + x.offset;
            let grown: Vec<Region3> = regions.iter().map(|r| Region3 { polygon: model_edit::grow(&r.polygon, x.offset), ..r.clone() }).collect();
            let tools: Vec<CutTool> = grown.iter().map(|r| CutTool { region: r.clone(), dir: r.normal(), z: Some((-reach, reach)) }).collect();
            let mut plain: Vec<PartId> = Vec::new();
            for p in &x.scope {
                match context_of(&current, *p) {
                    Some(ci) => {
                        let ctx = &current.sheet_metal[ci];
                        let walls: Vec<cadrs_sheetmetal::WallId> = ctx.parts.iter().filter(|(q, _)| q == p).flat_map(|(_, w)| w.clone()).collect();
                        let mut model = ctx.model.clone();
                        let cut = model_edit::cut_walls(&mut model, &tools, Some(&walls)).map_err(err)?;
                        if !cut.is_empty() {
                            let o = self.step(id, name, &current, ci, StepEdit::Cut { tools: tools.clone(), walls: Some(walls) })?;
                            if o.error.is_some() {
                                return Ok(o);
                            }
                            owned.extend(o.owned);
                            current = o.state;
                        }
                    }
                    None => plain.push(*p),
                }
            }
            if !plain.is_empty() {
                let o = self.pocket(id, &grown, reach, &plain, &current)?;
                owned.extend(o.owned);
                current = o.state;
            }
        }
        let mut o = output(current, owned);
        o.warning = (lost > 0).then(|| "A selected tab profile no longer exists".into());
        Ok(o)
    }

    /// Ordinary parts less the prisms of `regions` (each `reach` either side of its plane).
    fn pocket(&mut self, id: FeatureId, regions: &[Region3], reach: f64, parts: &[PartId], state: &Arc<State>) -> Result<Output, String> {
        let op = id.0;
        let mut tools: Vec<(BodyId, BodyNames)> = Vec::new();
        for (k, r) in regions.iter().enumerate() {
            let n = r.normal();
            let origin = r.origin - n * reach;
            let to_plane = |q: P2| {
                let p = r.point(q) - n * reach;
                nalgebra::Point2::new((p - origin).dot(&r.x), (p - origin).dot(&n.cross(&r.x)))
            };
            let profile = cadrs_kernel::Profile::new(kplane(origin, r.x, n), vec![region_of(&r.polygon, 0x5441_4200 + k as u64, &to_plane)]);
            let made = self.kernel.extrude(&profile, Extent::Blind(2.0 * reach)).map_err(|e| format!("Tab subtraction failed: {e}"));
            let made = made.and_then(|res| {
                let b = res.bodies[0];
                naming::name_body(&self.kernel, b, op, &res.history, &[]).map(|nm| (b, nm)).map_err(|e| e.to_string())
            });
            match made {
                Ok(x) => tools.push(x),
                Err(e) => {
                    for (b, _) in &tools {
                        self.kernel.release(*b);
                    }
                    return Err(e);
                }
            }
        }
        let geoms = state.geoms.clone();
        let mut next = (**state).clone();
        let refs: Vec<&(BodyId, BodyNames)> = tools.iter().collect();
        let mut placed: Vec<Placed> = Vec::new();
        let mut failed = None;
        for p in parts {
            match self.cut(id, *p, BoolOp::Subtract, &refs, &mut next) {
                Ok(v) => placed.extend(v),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        for (b, _) in &tools {
            self.kernel.release(*b);
        }
        if let Some(e) = failed {
            return Err(e);
        }
        self.finish(id, placed, next, op, geoms, PartKind::Solid)
    }

    /// Corner breaks at the picked corners (grouped by model).
    fn corner_breaks(&mut self, id: FeatureId, name: &str, picks: &[(PartId, Vec3)], kind: &dyn Fn(f64) -> CornerBreakKind, state: &Arc<State>) -> Result<Output, String> {
        let mut by_ctx: Vec<(usize, Vec<Vec3>)> = Vec::new();
        for (part, seed) in picks {
            let ci = need_context(state, *part)?;
            match by_ctx.iter_mut().find(|(c, _)| *c == ci) {
                Some((_, v)) => v.push(*seed),
                None => by_ctx.push((ci, vec![*seed])),
            }
        }
        let mut current = state.clone();
        let mut owned = Vec::new();
        for (ci, seeds) in by_ctx {
            let mut model = current.sheet_metal[ci].model.clone();
            let tol = model.params.thickness + 0.5;
            let mut done: Vec<(cadrs_sheetmetal::WallId, P2, CornerBreakKind)> = Vec::new();
            for s in seeds {
                let (wall, at) = model_edit::corner_vertex_at(&model, p3(s), tol).ok_or("Select a corner of a sheet metal wall")?;
                if done.iter().any(|(w, p, _)| *w == wall && (p - at).norm() < 1e-9) {
                    continue;
                }
                let beta = model_edit::corner_beta(&model, wall, at).ok_or("Select a corner of a sheet metal wall")?;
                // Broken here too, so the next pick finds the corners as they now are.
                model_edit::break_corner(&mut model, wall, at, kind(beta)).map_err(err)?;
                done.push((wall, at, kind(beta)));
            }
            let o = self.edit_sheet_metal(id, name, &current, ci, |ctx| {
                ctx.def.as_mut().expect("checked").push(name, StepEdit::CornerBreaks { corners: done });
                ctx.corner_broken = true;
                Ok(None)
            })?;
            if o.error.is_some() {
                return Ok(o);
            }
            owned.extend(o.owned);
            current = o.state;
        }
        Ok(output(current, owned))
    }

    /// P3I.5 (SM1.6, SM12): an ordinary feature that acts on active sheet metal as sheet metal,
    /// rebuilt as an edit of the definition; `None` for everything else.
    pub(in crate::rebuild) fn sheet_metal_aware(&mut self, before: &[Feature], f: &Feature, state: &Arc<State>) -> Option<Result<Output, String>> {
        if !state.sheet_metal.iter().any(|c| c.active) {
            return None;
        }
        match &f.kind {
            // SM1.6: Extrude Add isn't allowed onto an active model (Tab or Flange add material).
            FeatureKind::Extrude(e)
                if e.op == BooleanOp::Add
                    && e.body == crate::document::BodyType::Solid
                    && (e.merge_all || !e.merge_scope.is_empty())
                    && state.parts.iter().any(|p| (e.merge_all || e.merge_scope.contains(&p.part.id)) && context_of(state, p.part.id).is_some()) =>
            {
                Some(Err("Extrude can't add to an active sheet metal part: use Tab or Flange, or Finish sheet metal model first".into()))
            }
            FeatureKind::Extrude(e) => self.sheet_metal_cut(before, f.id, &f.name, e, state),
            FeatureKind::Hole(h) => self.sheet_metal_hole(before, f.id, &f.name, h, state),
            FeatureKind::Fillet(x) if x.kind == crate::applied::FilletType::Edge && !x.asymmetric && !x.variable && !x.partial => {
                let picks = corner_picks(state, &x.entities)?;
                let (size, width) = (x.size, x.measurement == FilletMeasurement::Width);
                Some(self.corner_breaks(f.id, &f.name, &picks, &|beta| CornerBreakKind::Fillet { radius: if width { model_edit::radius_for_width(size, beta) } else { size } }, state))
            }
            FeatureKind::Chamfer(x) => {
                let picks = corner_picks(state, &x.entities)?;
                let c = CornerBreakFeature {
                    chamfer: true,
                    chamfer_type: x.kind,
                    distance: x.distance,
                    distance2: x.distance2,
                    angle: x.angle,
                    flip: x.flip,
                    ..Default::default()
                };
                Some(self.corner_breaks(f.id, &f.name, &picks, &|beta| corner_kind(&c, beta), state))
            }
            FeatureKind::Mirror(x) if x.mirror_type == crate::pattern::PatternType::Face => {
                let (ci, walls) = face_walls(state, &x.faces)?;
                let frame = match x.plane? {
                    crate::pattern::MirrorPlane::Plane(p) => super::super::advanced::plane_of(state, &p)?,
                    crate::pattern::MirrorPlane::Face(fr) => {
                        let (part, _) = face_ids(state, &fr)?;
                        face_frame(part, &fr)?
                    }
                    crate::pattern::MirrorPlane::Connector(_) => return None,
                };
                let place = Placement::Mirror { point: p3(frame.origin), normal: v3(frame.normal()).normalize() };
                Some(self.copy_walls(f.id, &f.name, ci, &walls, &[place], state))
            }
            FeatureKind::Pattern(x) if x.pattern_type == crate::pattern::PatternType::Face => {
                let (ci, walls) = face_walls(state, &x.faces)?;
                let all = match self.pattern_instances(before, x, state, nalgebra::Vector3::zeros()) {
                    Ok(a) => a,
                    Err(e) => return Some(Err(e)),
                };
                let places: Vec<Placement> = all
                    .iter()
                    .filter(|i| !x.is_skipped(i.index[0], i.index[1]))
                    .map(|i| Placement::Affine { linear: i.motion.linear, t: i.motion.translation })
                    .collect();
                Some(self.copy_walls(f.id, &f.name, ci, &walls, &places, state))
            }
            _ => None,
        }
    }

    /// Walls copied to each placement (Face pattern, Face mirror).
    fn copy_walls(&mut self, id: FeatureId, name: &str, ci: usize, walls: &[cadrs_sheetmetal::WallId], places: &[Placement], state: &Arc<State>) -> Result<Output, String> {
        self.step(id, name, state, ci, StepEdit::Copy { walls: walls.to_vec(), places: places.to_vec(), seed: seed_of(id) })
    }

    /// Extrude → Remove through active sheet metal (SM1.6): the walls cut perpendicular.
    fn sheet_metal_cut(&mut self, before: &[Feature], id: FeatureId, name: &str, e: &crate::document::ExtrudeFeature, state: &Arc<State>) -> Option<Result<Output, String>> {
        if e.op != BooleanOp::Remove || !e.faces.is_empty() || e.direction.is_some() || e.body != crate::document::BodyType::Solid {
            return None;
        }
        let solids: Vec<PartId> = state.parts.iter().filter(|p| p.part.kind == PartKind::Solid).map(|p| p.part.id).collect();
        let targets: Vec<PartId> = if e.merge_all || e.merge_scope.is_empty() { solids } else { e.merge_scope.clone() };
        // Only when every part it may cut is active sheet metal.
        if targets.is_empty() || targets.iter().any(|p| context_of(state, *p).is_none()) {
            return None;
        }
        let (groups, _) = sweep_groups(before, &e.regions, &e.sketches, e.body);
        if groups.is_empty() {
            return None;
        }
        const FAR: f64 = 1e7;
        let depth_of = |end: EndType, d: f64| if end == EndType::Blind { d } else { FAR };
        let (z0, z1) = if e.symmetric {
            let d = depth_of(e.end, e.depth) / 2.0;
            (-d, d)
        } else {
            let lo = e.second.as_ref().map_or(0.0, |s| -depth_of(s.end, s.depth));
            (lo, depth_of(e.end, e.depth))
        };
        let mut tools = Vec::new();
        for g in &groups {
            let f = &g.frame;
            let n = v3(f.normal()).normalize();
            let dir = if e.flip { -n } else { n };
            for (_, r) in &g.regions {
                let ring = |l: &[cadrs_sketch::Vec2]| l.iter().map(|q| P2::new(q.x, q.y)).collect::<Vec<_>>();
                let region = Region3 { origin: p3(f.origin), x: v3(f.u).normalize(), y: v3(f.v).normalize(), polygon: Polygon::with_holes(ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect()) };
                // Depths run along the extrude direction from the sketch plane.
                tools.push(CutTool { region, dir, z: Some((z0, z1)) });
            }
        }
        let mut current = state.clone();
        let mut owned = Vec::new();
        let mut any = false;
        let contexts: Vec<usize> = (0..state.sheet_metal.len()).filter(|i| state.sheet_metal[*i].active).collect();
        for ci in contexts {
            let ctx = &current.sheet_metal[ci];
            let walls: Vec<cadrs_sheetmetal::WallId> = ctx.parts.iter().filter(|(p, _)| targets.contains(p)).flat_map(|(_, w)| w.clone()).collect();
            if walls.is_empty() {
                continue;
            }
            let mut model = ctx.model.clone();
            let cut = match model_edit::cut_walls(&mut model, &tools, Some(&walls)) {
                Ok(c) => c,
                Err(x) => return Some(Err(err(x))),
            };
            if cut.is_empty() {
                continue;
            }
            any = true;
            match self.step(id, name, &current, ci, StepEdit::Cut { tools: tools.clone(), walls: Some(walls) }) {
                Ok(o) if o.error.is_some() => return Some(Ok(o)),
                Ok(o) => {
                    owned.extend(o.owned);
                    current = o.state;
                }
                Err(x) => return Some(Err(format!("The cut can't be made in the sheet metal: {x}"))),
            }
        }
        if !any {
            return None;
        }
        Some(Ok(output(current, owned)))
    }
}

impl Rebuilder {
    /// A Hole through active sheet metal (P3I.7, SM16.3): the walls it crosses get round
    /// cut-outs of the hole's diameter, perpendicular to them (in the flat, like an Extrude →
    /// Remove); a counterbore's or countersink's outer diameter is kept for flat pattern
    /// drawing views ([`crate::sheetmetal::HoleMark`]). Holes at mate connectors, or on parts
    /// that aren't all active sheet metal, fall through to the ordinary Hole.
    fn sheet_metal_hole(&mut self, before: &[Feature], id: FeatureId, name: &str, h: &crate::applied::HoleFeature, state: &Arc<State>) -> Option<Result<Output, String>> {
        use crate::hole::{HoleEnd, HoleStart, HoleStyle};
        if h.problem().is_some() || !h.connectors.is_empty() || h.spec.start == HoleStart::SelectedPlane {
            return None;
        }
        let solids: Vec<PartId> = state.parts.iter().filter(|p| p.part.kind == PartKind::Solid).map(|p| p.part.id).collect();
        let targets: Vec<PartId> = if h.merge_scope.is_empty() { solids } else { h.merge_scope.clone() };
        if targets.is_empty() || targets.iter().any(|p| context_of(state, *p).is_none()) {
            return None;
        }
        let points = match super::super::applied::hole_points(before, h) {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => return None,
            Err(e) => return Some(Err(e)),
        };
        let spec = &h.spec;
        let radius = spec.diameter.value / 2.0;
        if radius <= 0.0 {
            return None;
        }
        const FAR: f64 = 1e7;
        // From the sketch plane on (Blind: as deep as set), or (Start from part) wherever the
        // axis meets the sheet.
        let z = match (spec.start, spec.end) {
            (HoleStart::SketchPlane, HoleEnd::Blind) => (-1e-3, spec.depth.value),
            (HoleStart::SketchPlane, _) => (-1e-3, FAR),
            _ => (-FAR, FAR),
        };
        let ring: Vec<P2> = (0..64)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 64.0;
                P2::new(radius * t.cos(), radius * t.sin())
            })
            .collect();
        let tools: Vec<CutTool> = points
            .iter()
            .map(|(_, origin, normal)| {
                let n = V3::new(normal.x, normal.y, normal.z);
                let dir = if h.flip { n } else { -n };
                let x = if dir.x.abs() < 0.9 { V3::x().cross(&dir).normalize() } else { V3::y().cross(&dir).normalize() };
                let y = dir.cross(&x);
                let region = Region3 { origin: P3::new(origin.x, origin.y, origin.z), x, y, polygon: Polygon::new(ring.clone()) };
                CutTool { region, dir, z: Some(z) }
            })
            .collect();
        let outer = match spec.style {
            HoleStyle::Counterbore => Some(spec.cbore_diameter.value / 2.0),
            HoleStyle::Countersink => Some(spec.csink_diameter.value / 2.0),
            HoleStyle::Simple => None,
        }
        .filter(|r| *r > radius);
        let mut current = state.clone();
        let mut owned = Vec::new();
        let mut any = false;
        let contexts: Vec<usize> = (0..state.sheet_metal.len()).filter(|i| state.sheet_metal[*i].active).collect();
        for ci in contexts {
            let ctx = &current.sheet_metal[ci];
            let walls: Vec<cadrs_sheetmetal::WallId> = ctx.parts.iter().filter(|(p, _)| targets.contains(p)).flat_map(|(_, w)| w.clone()).collect();
            if walls.is_empty() {
                continue;
            }
            let mut model = ctx.model.clone();
            match model_edit::cut_walls(&mut model, &tools, Some(&walls)) {
                Ok(c) if c.is_empty() => continue,
                Ok(_) => {}
                Err(x) => return Some(Err(err(x))),
            }
            any = true;
            let edit = StepEdit::Cut { tools: tools.clone(), walls: Some(walls) };
            let r = self.edit_sheet_metal(id, name, &current, ci, |ctx| {
                ctx.def.as_mut().expect("checked").push(name, edit);
                if let Some(outer) = outer {
                    ctx.hole_marks.push(crate::sheetmetal::HoleMark { feature: id, radius, outer });
                }
                Ok(None)
            });
            match r {
                Ok(o) if o.error.is_some() => return Some(Ok(o)),
                Ok(o) => {
                    owned.extend(o.owned);
                    current = o.state;
                }
                Err(x) => return Some(Err(format!("The hole can't be made in the sheet metal: {x}"))),
            }
        }
        if !any {
            return Some(Err("The holes miss the sheet metal".into()));
        }
        Some(Ok(output(current, owned)))
    }
}

/// A Corner break's shape at a corner of interior angle `beta`.
fn corner_kind(c: &CornerBreakFeature, beta: f64) -> CornerBreakKind {
    if !c.chamfer {
        let r = if c.fillet_measurement == FilletMeasurement::Width { model_edit::radius_for_width(c.size, beta) } else { c.size };
        return CornerBreakKind::Fillet { radius: r };
    }
    let (d1, d2) = match c.chamfer_type {
        ChamferType::EqualDistance => (c.distance, c.distance),
        ChamferType::TwoDistances => (c.distance, c.distance2),
        ChamferType::DistanceAngle => (c.distance, model_edit::chamfer_second(c.distance, c.angle.to_radians(), beta)),
    };
    let (d1, d2) = if c.flip { (d2, d1) } else { (d1, d2) };
    CornerBreakKind::Chamfer { d1, d2 }
}

/// A fillet's or chamfer's edges as sheet metal corners: `None` unless every entity is an edge
/// through the thickness at a wall corner of an active model.
fn corner_picks(state: &State, entities: &[EdgeOrFace]) -> Option<Vec<(PartId, Vec3)>> {
    if entities.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for e in entities {
        let EdgeOrFace::Edge(r) = e else { return None };
        let ci = context_of(state, r.part)?;
        let model = &state.sheet_metal[ci].model;
        let pts = edge_points(state, r)?;
        let (a, b) = (p3(pts[0]), p3(pts[pts.len() - 1]));
        let t = model.params.thickness;
        // Through the thickness: as long as the sheet is thick, at a corner of a wall.
        if ((b - a).norm() - t).abs() > 1e-4 * t.max(1.0) {
            return None;
        }
        let mid = P3::from((a.coords + b.coords) / 2.0);
        model_edit::corner_vertex_at(model, mid, 1e-3 * t.max(1.0) + 1e-6)?;
        out.push((r.part, [mid.x, mid.y, mid.z]));
    }
    Some(out)
}

/// The walls of picked faces, all of one active model.
fn face_walls(state: &State, faces: &[crate::document::FaceRef]) -> Option<(usize, Vec<cadrs_sheetmetal::WallId>)> {
    let mut ci = None;
    let mut walls = Vec::new();
    for f in faces {
        let c = context_of(state, f.part)?;
        if ci.is_some_and(|x| x != c) {
            return None;
        }
        ci = Some(c);
        let (w, _) = model_edit::wall_at(&state.sheet_metal[c].model, p3(f.seed), PICK_TOL)?;
        if !walls.contains(&w) {
            walls.push(w);
        }
    }
    Some((ci?, walls))
}
