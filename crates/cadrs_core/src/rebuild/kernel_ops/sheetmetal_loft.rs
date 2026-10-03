//! Rebuilding the Sheet metal loft (`crate::sheetmetal_loft`, P3I.9, SM19.2). A child of
//! `rebuild::kernel_ops`, beside the Sheet metal model's rebuild (`sheetmetal.rs`), whose fold
//! helpers it uses.
//!
//! 1. Each profile becomes a 3D polyline: a region's exact boundary, a face's outer loop, sketch
//!    curves or part edges joined end to end, or a point. Arcs are cut by the chordal tolerance.
//! 2. [`cadrs_sheetmetal::loft::loft`] lays it out (planar walls, facet joints, rips, bends).
//! 3. **New** makes a sheet metal model of its own (its context named after the loft, its
//!    definition the laid-out model); **Add** adds the walls and joints to the definition of the
//!    active model of the Merge scope's part as a step (its settings, its context), their part
//!    one with the merge scope's.
//! 4. Both fold through the one sheet metal pipeline (`sheetmetal/refold.rs`): each loft wall a
//!    mitred slab ([`cadrs_sheetmetal::loft::wall_slab`], made solid by the kernel's
//!    `mesh_solid`), bends as cylindrical shells, fused per part.
//!
//! The view's connection manipulators need the profiles and the matched connections: they go
//! out in the feature's `arrows` ([`encode_guides`]).

use super::sheetmetal::key_of;
use super::*;
use cadrs_sheetmetal::definition::{Definition, StepEdit};
use cadrs_sheetmetal::loft::{ConnectionIn, LoftOpts, ProfileIn, arc_pieces, loft};
use cadrs_sheetmetal::model::{JointNamer, P3};
use cadrs_sheetmetal::{FlatPattern, WallId};
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

    pub(in crate::rebuild) fn sheet_metal_loft(&mut self, before: &[Feature], id: FeatureId, name: &str, x: &SheetMetalLoftFeature, state: &Arc<State>) -> Result<Output, String> {
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
        // New: a model of its own (its walls fixed as the loft laid them, folded as slabs).
        // Add: the loft's walls and joints a step of the active model's definition, its part one
        // with the merge scope's.
        let mut o = match &target {
            None => {
                let ctx = SheetMetalContext {
                    feature: id,
                    name: name.to_string(),
                    model: built.model.clone(),
                    flat: FlatPattern::default(),
                    parts: Vec::new(),
                    active: true,
                    wall_keys: built.walls.clone(),
                    joint_keys: built.joints.clone(),
                    def: Some(Definition::fixed(built.model.clone(), true)),
                    owners: Vec::new(),
                    editors: vec![id],
                    forms: Vec::new(),
                    corner_broken: false,
                    hole_marks: Vec::new(),
                };
                self.refold(id, name, state, &[], None, ctx, &[])?
            }
            Some(c) => {
                let ci = state.sheet_metal.iter().position(|x| x.feature == c.feature).expect("found");
                let scope_wall = c.parts.iter().find(|(p, _)| x.merge_scope.contains(p)).and_then(|(_, ws)| ws.first().copied());
                let built = &built;
                self.edit_sheet_metal(id, name, state, ci, |ctx| {
                    let m = &ctx.model;
                    let wall_base = m.walls.iter().map(|w| w.id.0 + 1).max().unwrap_or(0);
                    let joint_base = m.joints.iter().map(|j| j.id.0 + 1).max().unwrap_or(0);
                    let mut namer = JointNamer::default();
                    namer.taken = m.joints.iter().map(|j| j.name.clone()).collect();
                    let mut walls = Vec::new();
                    for w in &built.model.walls {
                        let mut w = w.clone();
                        let old = w.id;
                        w.id = WallId(wall_base + old.0);
                        if let Some((k, _)) = built.walls.iter().find(|(_, id)| *id == old) {
                            ctx.wall_keys.push((*k, w.id));
                        }
                        walls.push(w);
                    }
                    let mut joints = Vec::new();
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
                        joints.push(j);
                    }
                    let def = ctx.def.as_mut().expect("checked");
                    def.slabs.extend(walls.iter().map(|w| w.id));
                    if let (Some(a), Some(w)) = (walls.first(), scope_wall) {
                        def.merges.push((a.id, w));
                    }
                    def.push(name, StepEdit::AddWalls { walls, joints });
                    Ok(None)
                })?
            }
        };
        if o.error.is_none() {
            o.warning = o.warning.or(warning);
        }
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
