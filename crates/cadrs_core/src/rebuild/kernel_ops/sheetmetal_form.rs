//! Rebuilding the sheet metal **Form** and **Tag (Form)** features (`crate::sheetmetal_form`,
//! P3I.9, SM20). A child of `rebuild::kernel_ops`.
//!
//! - **Tag (Form)** checks its parts and origin and changes nothing.
//! - **Form**: the form's Part Studio (a library form made for the model's thickness and the
//!   dialog's variables, or a document form's features with its variables set) is built inside
//!   this rebuild ([`Rebuilder::sub_build`], as Derived does). Each location gives a frame on its
//!   target face: the location projected onto the face, Z out of the face (into the sheet's other
//!   side with the opposite direction arrow, from the other face), X along the location's X.
//!   The tag's origin connector goes onto it; the form's footprint (its parts seen along Z) must
//!   keep clear of the wall's joints, rips, corners and edges ([`check_footprint`]), else the
//!   feature fails. Each copy is kept with its model (`SheetMetalContext::forms`, placed
//!   relative to its wall) and the model refolds through the one sheet metal pipeline
//!   (`sheetmetal/refold.rs`), which unites the add parts with the folded part, cuts the remove
//!   parts from it and puts each copy into its flat-pattern part's `forms` (the tag sketch's
//!   construction curves, or the footprint, and the centermark) — again at every later refold,
//!   on the wall wherever it has moved.

use super::*;
use cadrs_kernel::{Kernel, Motion};
use cadrs_sheetmetal::forms::{FormLine, check_footprint, hull};
use cadrs_sheetmetal::model::{P3, Surface, V3};
use cadrs_sheetmetal::poly::P2;
use cadrs_sketch::PlaneFrame;

use super::derived::motion_between;
use crate::mate::ConnectorRef;
use super::sheetmetal::refold::local_to_wall;
use crate::sheetmetal::{FormCopy, FormStep};
use crate::sheetmetal_form::{FormFeature, FormLocation, TagFormFeature, form_studio, tag_of};

fn v(a: [f64; 3]) -> V3 {
    V3::new(a[0], a[1], a[2])
}

fn moved(m: &Motion, p: [f64; 3]) -> P3 {
    P3::from(m.linear * v(p) + m.translation)
}

/// A sketch's construction curves as polylines in world coordinates.
fn construction_lines(sk: &crate::document::SketchFeature) -> Vec<(Vec<[f64; 3]>, bool)> {
    let Some(frame) = sk.plane.map(|p| p.frame()) else { return Vec::new() };
    let g = &sk.geometry;
    let mut out: Vec<(Vec<[f64; 3]>, bool)> = Vec::new();
    for (id, c) in g.curves.iter().filter(|(_, c)| c.construction) {
        let pts: (Vec<cadrs_sketch::Vec2>, bool) = match c.kind {
            cadrs_sketch::CurveKind::Line { a, b } => (vec![g.pos(a), g.pos(b)], false),
            cadrs_sketch::CurveKind::Circle { center, radius } => {
                let o = g.pos(center);
                ((0..48).map(|k| {
                    let a = k as f64 / 48.0 * std::f64::consts::TAU;
                    cadrs_sketch::Vec2::new(o.x + radius * a.cos(), o.y + radius * a.sin())
                }).collect(), true)
            }
            cadrs_sketch::CurveKind::Arc { .. } => match g.arc_geom(id) {
                Some(a) => ((0..=24).map(|k| a.point_at(a.start_angle + a.sweep * k as f64 / 24.0)).collect(), false),
                None => continue,
            },
            _ => continue,
        };
        out.push((pts.0.into_iter().map(|q| frame.to_world(q)).collect(), pts.1));
    }
    // Lines that meet end to end make one outline (a rectangle's four sides).
    let size = out.iter().flat_map(|(l, _)| l.iter()).map(|p| v(*p).norm()).fold(1.0, f64::max);
    let tol = 1e-6 * size;
    let (mut closed, mut open): (Vec<_>, Vec<_>) = out.into_iter().partition(|(_, c)| *c);
    while !open.is_empty() {
        let mut line: Vec<[f64; 3]> = open.remove(0).0;
        loop {
            let end = v(line[line.len() - 1]);
            let Some(i) = open.iter().position(|(l, _)| (v(l[0]) - end).norm() <= tol || (v(l[l.len() - 1]) - end).norm() <= tol) else { break };
            let (mut l, _) = open.remove(i);
            if (v(l[0]) - end).norm() > tol {
                l.reverse();
            }
            line.extend(l.into_iter().skip(1));
        }
        let is_loop = line.len() > 2 && (v(line[0]) - v(line[line.len() - 1])).norm() <= tol;
        if is_loop {
            line.pop();
        }
        closed.push((line, is_loop));
    }
    closed
}

/// The frames of a Form's locations.
fn location_frames(before: &[Feature], state: &State, locs: &[FormLocation]) -> Result<Vec<PlaneFrame>, String> {
    let mut out = Vec::new();
    for l in locs {
        match l {
            FormLocation::Connector(c) => out.push(super::super::connector_frame(before, state, c)?),
            FormLocation::SketchPoints(s) => {
                let sk = before.iter().find(|f| f.id == *s).and_then(|f| f.sketch()).ok_or("A location sketch no longer exists")?;
                let frame = sk.plane.map(|p| p.frame()).ok_or("A location sketch has no plane")?;
                for p in crate::hole::hole_vertices(&sk.geometry) {
                    let q = frame.to_world(sk.geometry.pos(p));
                    out.push(PlaneFrame { origin: q, u: frame.u, v: frame.v });
                }
            }
        }
    }
    Ok(out)
}

impl Rebuilder {
    pub(in crate::rebuild) fn tag_form(&mut self, id: FeatureId, x: &TagFormFeature, state: &Arc<State>) -> Result<Output, String> {
        let _ = id;
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        if x.add.iter().chain(&x.remove).any(|p| state.part(*p).is_none()) {
            return Err("A tagged part no longer exists".into());
        }
        if let Some(ConnectorRef::Feature(f)) = x.origin
            && !state.connectors.contains_key(&f)
        {
            return Err("The form origin mate connector no longer exists".into());
        }
        Ok(Output { state: state.clone(), error: None, warning: None, contacts: None, owned: Vec::new(), stage: None, axis: None, arrows: Vec::new(), dots: None, uses: Vec::new() })
    }

    pub(in crate::rebuild) fn sheet_metal_form(&mut self, before: &[Feature], id: FeatureId, name: &str, x: &FormFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let pick = x.form.as_ref().ok_or("Select a form Part Studio")?;
        // The targets: faces of active sheet metal parts, by part.
        struct Target {
            ctx: usize,
            point: P3,
            normal: V3,
        }
        let mut targets: Vec<Target> = Vec::new();
        for f in &x.targets {
            let (part, ids) = face_ids(state, f).ok_or("A target face no longer exists")?;
            let body = part.body.ok_or("A target face no longer exists")?;
            let info = self.kernel.faces(body).map_err(|e| e.to_string())?.into_iter().find(|i| Some(&i.id) == ids.first()).ok_or("A target face no longer exists")?;
            let Some(pl) = info.plane else { return Err("Forms go on flat faces of sheet metal walls".into()) };
            let ctx = state
                .sheet_metal
                .iter()
                .position(|c| c.active && c.parts.iter().any(|(p, _)| *p == part.part.id))
                .ok_or("The target face isn't on an active sheet metal part")?;
            let n = pl.normal.into_inner();
            targets.push(Target { ctx, point: P3::new(info.center.x, info.center.y, info.center.z), normal: V3::new(n.x, n.y, n.z) });
        }
        let thickness = state.sheet_metal[targets[0].ctx].model.params.thickness;
        let studio = form_studio(pick, &x.variables, thickness)?;
        let tag = tag_of(&studio).cloned().ok_or_else(|| format!("{} has no Tag (Form) feature", pick.name))?;
        let (sb, sstate) = self.sub_build(&studio)?;
        let origin = tag.origin.ok_or("The form has no origin mate connector")?;
        let base = crate::mate::frame(&origin, &studio, &sb.parts, &sstate.connectors).map_err(|e| format!("The form's origin: {e}"))?;
        let base_motion = motion_between(&base, &PlaneFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] });
        // The form in its own frame: outline lines and footprint.
        let local = |p: [f64; 3]| moved(&base_motion, p);
        let mut lines: Vec<(Vec<P3>, bool)> = Vec::new();
        if let Some(sk) = tag.sketch.and_then(|s| studio.iter().find(|f| f.id == s)).and_then(|f| f.sketch()) {
            lines = construction_lines(sk).into_iter().map(|(l, c)| (l.into_iter().map(local).collect(), c)).collect();
        }
        let tool_parts: Vec<&PartState> = tag.add.iter().chain(&tag.remove).filter_map(|p| sstate.part(*p)).collect();
        if tool_parts.is_empty() {
            return Err(format!("{}'s tagged parts don't build", pick.name));
        }
        let foot: Vec<P2> = tool_parts.iter().flat_map(|p| p.part.solid.positions.iter()).map(|q| {
            let l = local(*q);
            P2::new(l.x, l.y)
        }).collect();
        let footprint = hull(&foot);
        if lines.is_empty() {
            lines.push((footprint.outer.iter().map(|q| P3::new(q.x, q.y, 0.0)).collect(), true));
        }
        // Each location onto its target face.
        let frames = location_frames(before, state, &x.locations)?;
        if frames.is_empty() {
            return Err("The locations have no points".into());
        }
        // Each copy on its wall, kept with the model so every refold applies it again.
        let mut by_ctx: Vec<(usize, Vec<FormCopy>)> = Vec::new();
        // Where each copy lands (the dialog draws a triad there): per copy (origin, Z), (X, 0).
        let mut marks: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for fr in frames.iter() {
            let o = v(fr.origin);
            // The nearest target face's plane.
            let ti = (0..targets.len())
                .min_by(|a, b| {
                    let d = |t: &Target| (o - t.point.coords).dot(&t.normal).abs();
                    d(&targets[*a]).total_cmp(&d(&targets[*b]))
                })
                .expect("targets");
            let t = &targets[ti];
            let n = if x.flip { -t.normal } else { t.normal };
            let on = o - t.normal * (o - t.point.coords).dot(&t.normal) - if x.flip { t.normal * thickness } else { V3::zeros() };
            let ux = {
                let u = v(fr.u);
                let w = u - n * u.dot(&n);
                if w.norm() > 1e-6 { w.normalize() } else { let vv = v(fr.v); (vv - n * vv.dot(&n)).normalize() }
            };
            let to = crate::mate::frame_from([on.x, on.y, on.z], [n.x, n.y, n.z], [ux.x, ux.y, ux.z]);
            marks.push(([on.x, on.y, on.z], [n.x, n.y, n.z]));
            marks.push(([ux.x, ux.y, ux.z], [0.0; 3]));
            let m = motion_between(&base, &to);
            // The wall it lands on, and the rules.
            let model = &state.sheet_metal[t.ctx].model;
            let tt = model.params.thickness;
            let center = moved(&m, base.origin);
            let wall = model
                .walls
                .iter()
                .filter(|w| matches!(w.surface, Surface::Planar { .. }))
                .filter(|w| {
                    let wn = w.surface.normal().unwrap_or_default();
                    let Surface::Planar { origin, .. } = w.surface else { return false };
                    let d = (center - origin).dot(&wn);
                    wn.dot(&n).abs() > 1.0 - 1e-6 && ((d).abs() < 1e-4 * (1.0 + tt) || (d - tt).abs() < 1e-4 * (1.0 + tt) || (d + tt).abs() < 1e-4 * (1.0 + tt))
                })
                .min_by(|a, b| {
                    let inside = |w: &cadrs_sheetmetal::Wall| !w.outline.contains(w.surface.local(center));
                    inside(a).cmp(&inside(b))
                })
                .ok_or("Forms go on the flat faces of sheet metal walls (not rolled walls or bends)")?;
            let wall_id = wall.id;
            let to_wall = |p: P3| wall.surface.local(p);
            let size = wall.outline.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
            // A point in the form's own frame, placed by this copy, in the wall's 2D.
            let place = |q: V3| {
                let w = base_motion.linear.transpose() * (q - base_motion.translation);
                to_wall(moved(&m, [w.x, w.y, w.z]))
            };
            let fp: Vec<P2> = footprint.outer.iter().map(|q| place(V3::new(q.x, q.y, 0.0))).collect();
            check_footprint(model, wall_id, &cadrs_sheetmetal::poly::Polygon::new(fp.clone()), 1e-6 * size).map_err(|e| e.message())?;
            // The flat: outline and centermark, in the wall's 2D.
            let flines: Vec<FormLine> = lines
                .iter()
                .map(|(l, closed)| FormLine {
                    points: l.iter().map(|p| place(p.coords)).collect(),
                    closed: *closed,
                })
                .collect();
            let wn = wall.surface.normal().unwrap_or_default();
            let up = (n.dot(&wn) > 0.0) != model.params.flip_direction_up;
            let local = local_to_wall(model, wall_id, &m).ok_or("Forms go on the flat faces of sheet metal walls")?;
            let copy = FormCopy { wall: wall_id, local, center: to_wall(center), lines: flines, up, footprint: fp };
            match by_ctx.iter_mut().find(|(c, _)| *c == t.ctx) {
                Some((_, v)) => v.push(copy),
                None => by_ctx.push((t.ctx, vec![copy])),
            }
        }
        // Each model refolded with its copies.
        let mut current = state.clone();
        let mut owned = Vec::new();
        let mut last: Option<Output> = None;
        for (ci, copies) in by_ctx {
            let step = FormStep { feature: id, name: name.to_string(), pick: pick.clone(), variables: x.variables.clone(), thickness, copies };
            let o = self.edit_sheet_metal(id, name, &current, ci, |ctx| {
                ctx.forms.push(step);
                Ok(None)
            })?;
            if o.error.is_some() {
                return Ok(o);
            }
            owned.extend(o.owned.iter().copied());
            current = o.state.clone();
            last = Some(o);
        }
        let mut o = last.ok_or("The locations have no points")?;
        o.owned = owned;
        o.arrows = marks;
        Ok(o)
    }
}
