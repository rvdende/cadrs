//! Rebuilding the Sheet metal loft (`crate::sheetmetal_loft`, P3I.9, SM19.2). A child of
//! `rebuild::kernel_ops`, beside the Sheet metal model's rebuild (`sheetmetal.rs`), whose fold
//! helpers it uses.
//!
//! 1. Each profile becomes a 3D polyline: a region's exact boundary, a face's outer loop, sketch
//!    curves or part edges joined end to end, or a point. Arcs are cut by the chordal tolerance.
//! 2. [`cadrs_sheetmetal::loft::loft`] lays it out (planar walls, facet joints, rips, bends).
//! 3. **New** makes a sheet metal model of its own (its context named after the loft); **Add**
//!    adds the walls to the active model of the Merge scope's part (its settings, its context)
//!    and joins the folded walls to that part.
//! 4. The folded solid: each planar wall a mitred slab ([`cadrs_sheetmetal::loft::wall_slab`],
//!    made solid by the kernel's `mesh_solid`), bends as cylindrical shells, fused per part.
//!
//! The view's connection manipulators need the profiles and the matched connections: they go
//! out in the feature's `arrows` ([`encode_guides`]).

use super::sheetmetal::{flat_error, key_of, removed_of};
use super::*;
use cadrs_kernel::Kernel;
use cadrs_sheetmetal::flat::PieceSource;
use cadrs_sheetmetal::loft::{ConnectionIn, LoftOpts, ProfileIn, arc_pieces, loft, wall_slab};
use cadrs_sheetmetal::model::{JointNamer, P3};
use cadrs_sheetmetal::{FlatPattern, Model, WallId, flatten};
use cadrs_sketch::region::Piece;

use crate::sheetmetal::SheetMetalContext;
use crate::sheetmetal_loft::{LoftItem, SheetMetalLoftFeature, SmLoftOp};

fn p3(a: [f64; 3]) -> P3 {
    P3::new(a[0], a[1], a[2])
}

/// A sketch piece as points in its plane (arcs by the chordal tolerance).
fn piece_points(pc: &Piece, tol: f64) -> Vec<cadrs_sketch::Vec2> {
    match pc {
        Piece::Line(a, b) => vec![*a, *b],
        Piece::Arc(g) => {
            let n = arc_pieces(g.radius, g.sweep, tol);
            (0..=n).map(|k| g.point_at(g.start_angle + g.sweep * k as f64 / n as f64)).collect()
        }
        Piece::Ellipse { g, t0, sweep } => (0..=32).map(|k| g.point_at(t0 + sweep * k as f64 / 32.0)).collect(),
        Piece::Bezier(b) => (0..=32).map(|k| b.point_at(k as f64 / 32.0)).collect(),
    }
}

/// Polylines joined end to end into one (reversing pieces as needed); closed when it comes back
/// to its start.
pub(crate) fn join(mut lines: Vec<Vec<P3>>, tol: f64) -> (Vec<P3>, bool) {
    lines.retain(|l| !l.is_empty());
    if lines.is_empty() {
        return (Vec::new(), false);
    }
    let mut out = lines.remove(0);
    loop {
        let (s, e) = (out[0], out[out.len() - 1]);
        let Some((i, rev, front)) = lines.iter().enumerate().find_map(|(i, l)| {
            let (a, b) = (l[0], l[l.len() - 1]);
            if (a - e).norm() <= tol {
                Some((i, false, false))
            } else if (b - e).norm() <= tol {
                Some((i, true, false))
            } else if (b - s).norm() <= tol {
                Some((i, false, true))
            } else if (a - s).norm() <= tol {
                Some((i, true, true))
            } else {
                None
            }
        }) else {
            break;
        };
        let mut l = lines.remove(i);
        if rev {
            l.reverse();
        }
        if front {
            l.pop();
            l.extend(out);
            out = l;
        } else {
            out.extend(l.into_iter().skip(1));
        }
    }
    let closed = out.len() > 2 && (out[0] - out[out.len() - 1]).norm() <= tol;
    if closed {
        out.pop();
    }
    (out, closed)
}

/// The point `t` (0..1 of its length) along a profile.
pub(crate) fn along(p: &ProfileIn, t: f64) -> P3 {
    let pts = &p.points;
    if pts.len() < 2 {
        return pts.first().copied().unwrap_or_else(P3::origin);
    }
    let n = if p.closed { pts.len() } else { pts.len() - 1 };
    let seg = |i: usize| (pts[i], pts[(i + 1) % pts.len()]);
    let total: f64 = (0..n).map(|i| (seg(i).1 - seg(i).0).norm()).sum();
    let mut want = t.clamp(0.0, 1.0) * total;
    for i in 0..n {
        let (a, b) = seg(i);
        let l = (b - a).norm();
        if want <= l || i + 1 == n {
            return a + (b - a) * (want / l.max(1e-300)).min(1.0);
        }
        want -= l;
    }
    pts[0]
}

/// The guides the dialog's manipulators use, as the feature's `arrows`: a count header
/// `([connections, profile 1 points, profile 2 points, closed], 0)`, each connection
/// `(point on profile 1, point on profile 2)`, then each profile's points `(point, 0)`.
pub fn encode_guides(p1: &ProfileIn, p2: &ProfileIn, connections: &[ConnectionIn]) -> Vec<(Vec3, Vec3)> {
    let a = |p: P3| [p.x, p.y, p.z];
    let mut v = vec![([connections.len() as f64, p1.points.len() as f64, p2.points.len() as f64], [f64::from(u8::from(p1.closed)), 0.0, 0.0])];
    v.extend(connections.iter().map(|c| (a(c.a), a(c.b))));
    v.extend(p1.points.iter().chain(&p2.points).map(|p| (a(*p), [0.0; 3])));
    v
}

/// A folded flat-pattern part: its walls, body, names and the pieces' volume.
type FoldedPart = (Vec<WallId>, BodyId, BodyNames, f64);

impl Rebuilder {
    /// A loft profile as a polyline (or a point).
    fn loft_profile(&self, before: &[Feature], state: &State, items: &[LoftItem], tol: f64) -> Result<ProfileIn, String> {
        let lost = || "A loft profile no longer exists".to_string();
        let mut lines: Vec<Vec<P3>> = Vec::new();
        let mut curves: Vec<(FeatureId, Vec<cadrs_sketch::CurveId>)> = Vec::new();
        let sketch = |s: FeatureId| -> Option<(&crate::document::SketchFeature, cadrs_sketch::PlaneFrame)> {
            let sk = before.iter().find(|f| f.id == s)?.sketch()?;
            Some((sk, sk.plane?.frame()))
        };
        for it in items {
            match it {
                LoftItem::SketchPoint { sketch: s, point } => {
                    let (sk, frame) = sketch(*s).ok_or_else(lost)?;
                    let q = frame.to_world(sk.geometry.points.get(*point).ok_or_else(lost)?.pos);
                    if items.len() == 1 {
                        return Ok(ProfileIn::point(p3(q)));
                    }
                }
                LoftItem::Vertex(v) => {
                    let part = state.part(v.part).ok_or_else(lost)?;
                    let q = part.part.solid.vertex(&v.vertex).map(|x| x.point).unwrap_or(v.point);
                    if items.len() == 1 {
                        return Ok(ProfileIn::point(p3(q)));
                    }
                }
                LoftItem::Region(r) => {
                    let (sk, frame) = sketch(r.sketch).ok_or_else(lost)?;
                    let rs = cadrs_sketch::region::regions(&sk.geometry);
                    let i = cadrs_sketch::region::region_at(&rs, r.seed).ok_or_else(lost)?;
                    let reg = &rs[i];
                    let mut pts: Vec<P3> = Vec::new();
                    if reg.outer_pieces.is_empty() {
                        pts.extend(reg.outer.iter().map(|q| p3(frame.to_world(*q))));
                    } else {
                        for pc in &reg.outer_pieces {
                            let mut ps: Vec<P3> = piece_points(pc, tol).into_iter().map(|q| p3(frame.to_world(q))).collect();
                            if !pts.is_empty() {
                                ps.remove(0);
                            }
                            pts.extend(ps);
                        }
                    }
                    pts.push(pts[0]);
                    lines.push(pts);
                }
                LoftItem::Face(f) => {
                    let (part, _) = face_ids(state, f).ok_or_else(lost)?;
                    let solid = &part.part.solid;
                    let (i, _) = solid.resolve_face(&f.face, None, Some(f.seed)).map_err(|_| lost())?;
                    let face = &solid.faces[i];
                    let size = |l: &Vec<[f64; 3]>| {
                        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                        for p in l {
                            for k in 0..3 {
                                lo[k] = lo[k].min(p[k]);
                                hi[k] = hi[k].max(p[k]);
                            }
                        }
                        (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>()
                    };
                    let outer = face.loops.iter().max_by(|a, b| size(a).total_cmp(&size(b))).ok_or_else(lost)?;
                    let mut pts: Vec<P3> = outer.iter().map(|p| p3(*p)).collect();
                    if let Some(f0) = pts.first().copied() {
                        pts.push(f0);
                    }
                    lines.push(pts);
                }
                LoftItem::Curve(c) => match curves.iter_mut().find(|(s, _)| *s == c.sketch) {
                    Some((_, v)) => v.push(c.curve),
                    None => curves.push((c.sketch, vec![c.curve])),
                },
                LoftItem::Edge(e) => {
                    let (part, _) = super::applied::edge_ids(state, e).ok_or_else(lost)?;
                    let solid = &part.part.solid;
                    let (i, _) = solid.resolve_edge(&e.edge, |x| Some(x.distance(e.seed))).map_err(|_| lost())?;
                    lines.push(solid.edges[i].points.iter().map(|p| p3(*p)).collect());
                }
            }
        }
        for (s, ids) in curves {
            let (sk, frame) = sketch(s).ok_or_else(lost)?;
            let mut g = sk.geometry.clone();
            g.curves.retain(|id, _| ids.contains(&id));
            for ch in crate::rebuild::sketch_chains(s, &g) {
                let mut pts: Vec<P3> = Vec::new();
                for (pc, _) in &ch.pieces {
                    let mut ps: Vec<P3> = piece_points(pc, tol).into_iter().map(|q| p3(frame.to_world(q))).collect();
                    if !pts.is_empty() && !ps.is_empty() && (ps[0] - pts[pts.len() - 1]).norm() > 1e-6 {
                        // A piece running the other way.
                        if (ps[ps.len() - 1] - pts[pts.len() - 1]).norm() <= 1e-6 {
                            ps.reverse();
                        }
                    }
                    if !pts.is_empty() {
                        ps.remove(0);
                    }
                    pts.extend(ps);
                }
                if ch.closed && !pts.is_empty() {
                    pts.push(pts[0]);
                }
                lines.push(pts);
            }
        }
        if lines.is_empty() {
            return Err(if items.len() > 1 { "A point profile must be a single point".into() } else { lost() });
        }
        let size = lines.iter().flatten().map(|p| p.coords.norm()).fold(1.0, f64::max);
        let n = lines.len();
        let (points, closed) = join(lines, 1e-6 * size);
        if points.len() < 2 {
            return Err(lost());
        }
        let _ = n;
        Ok(ProfileIn { points, closed })
    }

    /// Folds the walls `which` of the parts of `flat`: per part with any of them, its walls (mitred
    /// slabs, or extruded) and bends fused, with the pieces' volume.
    fn fold_loft(&mut self, op: cadrs_kernel::OpId, model: &Model, flat: &FlatPattern, which: &[WallId]) -> Result<Vec<FoldedPart>, String> {
        let t = model.params.thickness;
        let mut out = Vec::new();
        for part in flat.parts.iter().filter(|p| p.walls.iter().any(|w| which.contains(w))) {
            let mut made: Vec<(BodyId, BodyNames, f64)> = Vec::new();
            let fail = |this: &mut Self, made: &[(BodyId, BodyNames, f64)], e: String| {
                for (b, _, _) in made {
                    this.kernel.release(*b);
                }
                Err::<Vec<_>, String>(e)
            };
            for piece in &part.pieces {
                let removed = removed_of(part, piece.source);
                let r = match piece.source {
                    PieceSource::Wall(w) => {
                        let Some(wall) = model.wall(w) else { continue };
                        match wall_slab(model, w, &removed) {
                            Some(tris) => {
                                let size = tris.iter().flatten().map(|p| p.coords.norm()).fold(1.0, f64::max);
                                match self.kernel.mesh_solid(&tris, 1e-7 * size) {
                                    Ok(b) => {
                                        let names = naming::name_body(&self.kernel, b, op, &cadrs_kernel::History::default(), &[]).map_err(|e| e.to_string());
                                        let vol = self.kernel.mass_properties(b).map(|m| m.volume).map_err(|e| e.to_string());
                                        match (names, vol) {
                                            (Ok(n), Ok(v)) => Ok(vec![(b, n, v)]),
                                            (Err(e), _) | (_, Err(e)) => {
                                                self.kernel.release(b);
                                                Err(e)
                                            }
                                        }
                                    }
                                    // A slab the kernel can't sew: the plain extrusion.
                                    Err(_) => self.wall_bodies(op, wall, &removed, t),
                                }
                            }
                            None => self.wall_bodies(op, wall, &removed, t),
                        }
                    }
                    PieceSource::Bend(j) => self.bend_body(op, model, j, &removed),
                };
                match r {
                    Ok(v) => made.extend(v),
                    Err(e) => return fail(self, &made, format!("Sheet metal loft wall failed: {e}")),
                }
            }
            if made.is_empty() {
                continue;
            }
            let sum: f64 = made.iter().map(|(_, _, v)| *v).sum();
            let fused = if made.len() == 1 {
                let (b, n, _) = made.pop().expect("one");
                (b, n)
            } else {
                let first = made[0].0;
                let rest: Vec<BodyId> = made[1..].iter().map(|(b, _, _)| *b).collect();
                let r = self.kernel.boolean(BoolOp::Union, first, &rest).and_then(|res| {
                    let body = res.bodies[0];
                    let inputs: Vec<(BodyId, &BodyNames)> = made.iter().map(|(b, n, _)| (*b, n)).collect();
                    Ok((body, naming::name_body(&self.kernel, body, op, &res.history, &inputs)?))
                });
                for (b, _, _) in &made {
                    self.kernel.release(*b);
                }
                r.map_err(|e| format!("Couldn't join the sheet metal loft's walls: {e}"))?
            };
            out.push((part.walls.clone(), fused.0, fused.1, sum));
        }
        Ok(out)
    }

    pub(in crate::rebuild) fn sheet_metal_loft(&mut self, before: &[Feature], id: FeatureId, x: &SheetMetalLoftFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        // Add: the active model the merge scope's part belongs to.
        let target: Option<SheetMetalContext> = match x.op {
            SmLoftOp::New => None,
            SmLoftOp::Add => {
                let ctxs: Vec<&SheetMetalContext> = state.sheet_metal.iter().filter(|c| c.active && x.merge_scope.iter().any(|p| c.parts.iter().any(|(q, _)| q == p))).collect();
                match ctxs.as_slice() {
                    [c] => Some((*c).clone()),
                    [] => return Err("The merge scope must be a part of an active sheet metal model".into()),
                    _ => return Err("The merge scope can only take the parts of one active sheet metal model".into()),
                }
            }
        };
        if let Some(e) = x.params.validate().first().filter(|_| target.is_none()) {
            return Err(e.message());
        }
        let params = target.as_ref().map_or(x.params, |c| c.model.params);
        let tol = x.chordal_tolerance;
        let p1 = self.loft_profile(before, state, &x.profile1, tol)?;
        let p2 = self.loft_profile(before, state, &x.profile2, tol)?;
        let connections: Vec<ConnectionIn> = if x.connections_on {
            x.connections.iter().map(|c| ConnectionIn { a: along(&p1, c.t1), b: along(&p2, c.t2), rip: c.rip }).collect()
        } else {
            Vec::new()
        };
        let opts = LoftOpts { flip_side: x.flip_thickness, connections, bends: true, key: key_of(&id) };
        let built = loft(params, &p1, &p2, &opts).map_err(|e| e.message())?;
        let arrows = encode_guides(&p1, &p2, &built.strip.connections);
        let warning = built.warnings.first().cloned();
        // The definition: the loft's own, or the active model's with the loft's walls added.
        let (model, loft_walls, mut ctx) = match &target {
            None => {
                let m = built.model.clone();
                let walls: Vec<WallId> = m.walls.iter().map(|w| w.id).collect();
                let ctx = SheetMetalContext {
                    feature: id,
                    model: m.clone(),
                    flat: FlatPattern::default(),
                    parts: Vec::new(),
                    active: true,
                    wall_keys: built.walls.clone(),
                    joint_keys: built.joints.clone(),
                    // A loft isn't rebuilt from a recipe: Modify joint edits don't apply to it yet.
                    recipe: None,
                    edits: Vec::new(),
                    table_order: Vec::new(),
                    def: None,
                    owners: Vec::new(),
                };
                (m, walls, ctx)
            }
            Some(c) => {
                let mut m = c.model.clone();
                let wall_base = m.walls.iter().map(|w| w.id.0 + 1).max().unwrap_or(0);
                let joint_base = m.joints.iter().map(|j| j.id.0 + 1).max().unwrap_or(0);
                let mut namer = JointNamer::default();
                namer.taken = m.joints.iter().map(|j| j.name.clone()).collect();
                let mut ctx = c.clone();
                let mut walls = Vec::new();
                for w in &built.model.walls {
                    let mut w = w.clone();
                    let old = w.id;
                    w.id = WallId(wall_base + old.0);
                    walls.push(w.id);
                    if let Some((k, _)) = built.walls.iter().find(|(_, id)| *id == old) {
                        ctx.wall_keys.push((*k, w.id));
                    }
                    m.walls.push(w);
                }
                for j in &built.model.joints {
                    let mut j = j.clone();
                    let old = j.id;
                    j.id = cadrs_sheetmetal::JointId(joint_base + old.0);
                    j.a = WallId(wall_base + j.a.0);
                    j.b = WallId(wall_base + j.b.0);
                    j.name = namer.name(&j.kind);
                    if let Some((k, _)) = built.joints.iter().find(|(_, id)| *id == old) {
                        ctx.joint_keys.push((*k, j.id));
                    }
                    m.joints.push(j);
                }
                ctx.model = m.clone();
                (m, walls, ctx)
            }
        };
        let flat = flatten(&model);
        ctx.flat = flat.clone();
        let keep = |ctx: SheetMetalContext, why: String| -> Result<Output, String> {
            let mut next = (**state).clone();
            let mut all = (*next.sheet_metal).clone();
            all.retain(|c| c.feature != ctx.feature);
            all.push(ctx);
            next.sheet_metal = Arc::new(all);
            Ok(Output { state: Arc::new(next), error: Some(why), warning: None, contacts: None, owned: Vec::new(), stage: None, axis: None, arrows: arrows.clone(), dots: None, uses: Vec::new() })
        };
        if let Some(why) = flat_error(&flat) {
            return if target.is_some() { Err(why) } else { keep(ctx, why) };
        }
        let op = id.0;
        let folded = self.fold_loft(op, &model, &flat, &loft_walls)?;
        let geoms = state.geoms.clone();
        let mut next = State { geoms: geoms.clone(), ..(**state).clone() };
        let mut placed: Vec<Placed> = Vec::new();
        let mut walls_of: Vec<(PartId, Vec<WallId>)> = ctx.parts.clone();
        for (k, (walls, body, names, _)) in folded.into_iter().enumerate() {
            let pieces = match &target {
                None => {
                    let pieces = self.split(body, op, &[(body, &names)]);
                    self.kernel.release(body);
                    let mut v = Vec::new();
                    for (n, pc) in pieces?.into_iter().enumerate() {
                        let want = PartId::new(id, k as u32);
                        let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                        let pid = if n == 0 && next.part(want).is_none() && !taken.contains(&want) { want } else { Self::new_id(id, &next, &taken) };
                        v.push((pid, pc));
                    }
                    v
                }
                Some(_) => {
                    let tool = (body, names);
                    let r = self.add(id, &tool, &x.merge_scope, &mut next);
                    self.kernel.release(body);
                    r?
                }
            };
            for (n, (pid, pc)) in pieces.into_iter().enumerate() {
                let loft_walls: Vec<WallId> = walls.iter().copied().filter(|w| loft_walls.contains(w)).collect();
                match walls_of.iter_mut().find(|(p, _)| *p == pid) {
                    Some((_, ws)) => {
                        for w in &loft_walls {
                            if !ws.contains(w) {
                                ws.push(*w);
                            }
                        }
                    }
                    None if n == 0 => walls_of.push((pid, loft_walls)),
                    None => {}
                }
                placed.push((pid, pc));
            }
        }
        let mut o = self.finish(id, placed, next, op, geoms, PartKind::Solid)?;
        ctx.parts = walls_of;
        let mut next = (*o.state).clone();
        let mut all = (*next.sheet_metal).clone();
        all.retain(|c| c.feature != ctx.feature);
        all.push(ctx);
        next.sheet_metal = Arc::new(all);
        o.state = Arc::new(next);
        o.warning = warning;
        o.arrows = arrows;
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polylines_join_end_to_end() {
        let p = |x: f64, y: f64| P3::new(x, y, 0.0);
        let (l, closed) = join(vec![vec![p(0.0, 0.0), p(1.0, 0.0)], vec![p(1.0, 1.0), p(1.0, 0.0)], vec![p(0.0, 0.0), p(1.0, 1.0)]], 1e-9);
        assert!(closed);
        assert_eq!(l.len(), 3);
        let q = ProfileIn { points: vec![p(0.0, 0.0), p(2.0, 0.0)], closed: false };
        assert!((along(&q, 0.25) - p(0.5, 0.0)).norm() < 1e-12);
    }
}
