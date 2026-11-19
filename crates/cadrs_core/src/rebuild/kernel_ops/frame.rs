//! Rebuilding the Frame feature (`crate::frame`, issue #266): the path's curves gathered into
//! members, each member's section extruded (straight) or swept (curved) with run-ons where a
//! corner needs them, the corners cut by planes (mitres and butts) or by the member run on
//! (copes), and every member made a part. A child of `rebuild::kernel_ops`.

use super::advanced::sketch_of;
use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{
    BodyKind, BoolOp, CurveKind, Extent, Kernel, Loop, PathCurve, Plane, Profile, Region,
    SplitTool, SweepControl, SweepSpec,
};
use nalgebra::{Point3, Unit, Vector3};

use crate::advanced::PathRef;
use crate::frame::{self, CornerType, EndCut, FrameFeature, MemberInfo, ProfileFamily};

type P = Point3<f64>;
type V = Vector3<f64>;

/// Curves meeting at an angle below this are tangent (radians).
const TANGENT: f64 = 0.5 * std::f64::consts::PI / 180.0;

/// One curve of the path.
struct Seg {
    curve: PathCurve,
    start: P,
    end: P,
    /// Unit tangents at the start and end, the way the curve runs.
    t0: V,
    t1: V,
    length: f64,
    line: bool,
    /// The section's up: the curve's sketch normal, else the world's Z.
    up: V,
}

/// A member: curves run end to end (each with whether it is run backwards).
struct Member {
    segs: Vec<(usize, bool)>,
    start: P,
    end: P,
    /// Unit tangents at its start and end, the way it runs.
    t0: V,
    t1: V,
    closed: bool,
    straight: bool,
    length: f64,
    up: V,
}

impl Member {
    /// The section's right and up at an end (0: start, 1: end), square to the member.
    fn axes(&self, end: usize) -> (V, V) {
        let t = if end == 0 { self.t0 } else { self.t1 };
        axes(t, self.up)
    }

    /// The way into the member from an end.
    fn inward(&self, end: usize) -> V {
        if end == 0 { self.t0 } else { -self.t1 }
    }

    fn point(&self, end: usize) -> P {
        if end == 0 { self.start } else { self.end }
    }
}

/// A section plane's right and up for a curve running along `t`: up is `up` square to `t` (the
/// world's Y, else X, where `up` runs along it), right `up × t`.
fn axes(t: V, up: V) -> (V, V) {
    let mut y = up - t * up.dot(&t);
    for alt in [V::y(), V::x()] {
        if y.norm() > 1e-6 {
            break;
        }
        y = alt - t * alt.dot(&t);
    }
    let y = y.normalize();
    (y.cross(&t), y)
}

/// Any unit vector square to `n`.
fn square_to(n: &V) -> V {
    let a = if n.x.abs() < 0.9 { V::x() } else { V::y() };
    (a - n * a.dot(n)).normalize()
}

/// The furthest the placed section reaches along the 3D direction `d`, from the path, at an end
/// whose section axes are `(x, y)`.
fn reach(regions: &[Region], (x, y): (V, V), d: &V) -> f64 {
    frame::support(regions, [d.dot(&x), d.dot(&y)])
}

/// What happens at one end of a member.
#[derive(Default, Clone)]
struct EndPlan {
    /// How far it runs on past its curve's end.
    run_on: f64,
    /// Planes it is cut by: a point and the normal of the side kept.
    planes: Vec<(P, V)>,
    /// Members whose shape cuts it (copes), and a point and the way into the member (to keep the
    /// piece on its side).
    copes: Vec<(usize, P, V)>,
    cut: Option<EndCut>,
}

impl Rebuilder {
    pub(in crate::rebuild) fn frame(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        x: &FrameFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let op = id.0;
        // The section.
        let (raw, custom_name) = match x.family {
            ProfileFamily::Custom => {
                let sk = x.sketch.ok_or("Select the sketch of the custom profile")?;
                let (groups, _) = sweep_groups(before, &[], &[sk], BodyType::Solid);
                let regions: Vec<Region> = groups
                    .iter()
                    .flat_map(|g| crate::brep::profile(g).regions)
                    .collect();
                if regions.is_empty() {
                    return Err("The custom profile's sketch has no closed regions".into());
                }
                let name = before.iter().find(|f| f.id == sk).map(|f| f.name.clone());
                (regions, name)
            }
            f => (
                frame::section(f, &x.size).ok_or("Select a profile size")?,
                None,
            ),
        };
        let section = frame::place(&raw, x.anchor, x.mirror, x.angle.to_radians());
        let r_section = frame::radius(&section);
        if !(r_section > 0.0 && r_section.is_finite()) {
            return Err("The profile has no size".into());
        }
        let profile_label = x.profile_label(custom_name.as_deref());

        // The path's curves.
        let mut segs: Vec<Seg> = Vec::new();
        let mut missing = 0usize;
        for p in &x.path {
            let curves = match self.path_curves(before, state, std::slice::from_ref(p)) {
                Ok(c) => c,
                Err(_) => {
                    missing += 1;
                    continue;
                }
            };
            for c in curves {
                segs.push(self.frame_seg(c, p)?);
            }
        }
        if segs.is_empty() {
            return Err("The frame's curves no longer exist".into());
        }
        let size = segs
            .iter()
            .flat_map(|s| [s.start, s.end])
            .fold(0.0f64, |m, p| m.max(p.coords.amax()));
        let tol = 1e-6 * (1.0 + size);
        let members = members(&segs, x.merge_tangent, tol);

        // The corners.
        let resolved: Vec<Option<P>> = x
            .overrides
            .iter()
            .map(|o| match o.point {
                Some(sp) => sketch_of(before, sp.sketch)
                    .and_then(|(g, frame)| g.points.get(sp.point).map(|q| frame.to_world(q.pos)))
                    .or(Some(o.at))
                    .map(|a| P::new(a[0], a[1], a[2])),
                None => Some(P::new(o.at[0], o.at[1], o.at[2])),
            })
            .collect();
        let mut used_override = vec![false; x.overrides.len()];
        let mut plans: Vec<[EndPlan; 2]> = vec![Default::default(); members.len()];
        let margin = 0.02 * r_section + 1e-3;
        // Member ends grouped by where they are.
        let mut corners: Vec<(P, Vec<(usize, usize)>)> = Vec::new();
        for (i, m) in members.iter().enumerate() {
            if m.closed {
                continue;
            }
            for end in 0..2 {
                let p = m.point(end);
                match corners
                    .iter_mut()
                    .find(|(q, _)| (q - p).norm() < tol * 10.0)
                {
                    Some((_, list)) => list.push((i, end)),
                    None => corners.push((p, vec![(i, end)])),
                }
            }
        }
        let override_tol = 1e-4 * (1.0 + size);
        let mut problems: Vec<String> = Vec::new();
        for (v, ends) in &corners {
            let ov = resolved
                .iter()
                .position(|r| r.is_some_and(|r| (r - v).norm() < override_tol));
            if let Some(i) = ov {
                used_override[i] = true;
            }
            let (kind, flip) = match ov {
                Some(i) => (x.overrides[i].corner, x.overrides[i].flip),
                None => (x.corner, x.flip),
            };
            if kind == CornerType::None {
                continue;
            }
            // A straight member running through the point: the ends here butt against it.
            let passing = members.iter().position(|m| {
                if !m.straight || m.closed {
                    return false;
                }
                let along = (v - m.start).dot(&m.t0);
                let off = (v - m.start) - m.t0 * along;
                off.norm() < tol * 10.0 && along > tol * 10.0 && along < m.length - tol * 10.0
            });
            if let Some(a) = passing {
                let am = &members[a];
                for &(b, end) in ends {
                    plan_butt(
                        &section,
                        (a, None),
                        am.t0,
                        am.axes(0),
                        &members[b],
                        (b, end),
                        *v,
                        kind == CornerType::CopedButt,
                        margin,
                        &mut plans,
                    );
                }
                continue;
            }
            match ends.len() {
                0 | 1 => {}
                2 => {
                    let (e0, e1) = (ends[0], ends[1]);
                    let (a, b) = (members[e0.0].inward(e0.1), members[e1.0].inward(e1.1));
                    // Running straight on: two pieces end to end.
                    if a.dot(&b) < -TANGENT.cos() {
                        continue;
                    }
                    if a.dot(&b) > TANGENT.cos() {
                        problems.push("Two members fold back on each other at a corner".into());
                        continue;
                    }
                    match kind {
                        CornerType::Miter => {
                            let n = (a - b).normalize();
                            for (e, n) in [(e0, n), (e1, -n)] {
                                let m = &members[e.0];
                                let into = m.inward(e.1);
                                let k = into.dot(&n);
                                let over = reach(&section, m.axes(e.1), &n) / k;
                                let plan = &mut plans[e.0][e.1];
                                plan.run_on = plan.run_on.max(over.max(0.0) + margin);
                                plan.planes.push((*v, n));
                                plan.cut = Some(angle_cut(k));
                            }
                        }
                        _ => {
                            let (through, other) = if flip { (e1, e0) } else { (e0, e1) };
                            let tm = &members[through.0];
                            plan_butt(
                                &section,
                                (through.0, Some(through.1)),
                                tm.inward(through.1),
                                tm.axes(through.1),
                                &members[other.0],
                                other,
                                *v,
                                kind == CornerType::CopedButt,
                                margin,
                                &mut plans,
                            );
                        }
                    }
                }
                _ => {
                    // The first member in the path runs on (the next with flip); the others
                    // are cut against it.
                    let mut order = ends.clone();
                    order.sort();
                    if flip {
                        order.rotate_left(1);
                    }
                    let through = order[0];
                    let tm = &members[through.0];
                    for &other in &order[1..] {
                        plan_butt(
                            &section,
                            (through.0, Some(through.1)),
                            tm.inward(through.1),
                            tm.axes(through.1),
                            &members[other.0],
                            other,
                            *v,
                            kind == CornerType::CopedButt,
                            margin,
                            &mut plans,
                        );
                    }
                }
            }
        }
        if let Some(p) = problems.first() {
            return Err(p.clone());
        }

        // The members' bodies, run on and cut by their planes.
        let mut bodies: Vec<(BodyId, BodyNames)> = Vec::new();
        let release_all = |k: &mut Self, bodies: &[(BodyId, BodyNames)]| {
            for (b, _) in bodies {
                k.kernel.release(*b);
            }
        };
        for (i, m) in members.iter().enumerate() {
            let made = (|| -> Result<(BodyId, BodyNames), String> {
                let base = (i as u64 + 1) << 24;
                let mut b = self.frame_member(state, op, m, &segs, &section, &plans[i], base)?;
                let mut cut_source = base + 0x00ff_0000;
                for end in 0..2 {
                    for (o, n) in plans[i][end].planes.clone() {
                        cut_source += 1;
                        b = self.frame_cut(op, b, o, n, cut_source)?;
                    }
                }
                Ok(b)
            })();
            match made {
                Ok(b) => bodies.push(b),
                Err(e) => {
                    release_all(self, &bodies);
                    return Err(format!("Member {} failed: {e}", i + 1));
                }
            }
        }
        // Copes, by the members they butt against as those are now.
        for i in 0..members.len() {
            for end in 0..2 {
                for (by, at, way) in plans[i][end].copes.clone() {
                    let tool = (bodies[by].0, bodies[by].1.clone());
                    let b = (bodies[i].0, bodies[i].1.clone());
                    // (`frame_cope` releases `b` either way.)
                    match self.frame_cope(op, b, (tool.0, &tool.1), at, way) {
                        Ok(c) => bodies[i] = c,
                        Err(e) => {
                            for (j, (b, _)) in bodies.iter().enumerate() {
                                if j != i {
                                    self.kernel.release(*b);
                                }
                            }
                            return Err(format!("Member {} failed: {e}", i + 1));
                        }
                    }
                }
            }
        }

        // The parts, and what the cut list says of them.
        let mut next = (**state).clone();
        let mut placed: Vec<Placed> = Vec::new();
        let mut infos: Vec<MemberInfo> = Vec::new();
        for (i, (body, names)) in bodies.into_iter().enumerate() {
            let m = &members[i];
            let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
            let part = Self::new_id(id, &next, &taken);
            let volume = self
                .kernel
                .mass_properties(body)
                .map(|p| p.volume)
                .unwrap_or(0.0);
            let length = if m.straight {
                self.kernel
                    .vertices(body)
                    .ok()
                    .and_then(|vs| {
                        let along: Vec<f64> =
                            vs.iter().map(|v| (v.point - m.start).dot(&m.t0)).collect();
                        let lo = along.iter().copied().fold(f64::INFINITY, f64::min);
                        let hi = along.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                        (hi > lo).then_some(hi - lo)
                    })
                    .unwrap_or(m.length)
            } else {
                m.length
            };
            infos.push(MemberInfo {
                feature: id,
                part,
                profile: profile_label.clone(),
                length,
                ends: [0, 1].map(|e| {
                    if m.closed {
                        EndCut::Square
                    } else {
                        plans[i][e].cut.unwrap_or(EndCut::Square)
                    }
                }),
                curved: !m.straight,
                volume,
            });
            placed.push((
                part,
                Piece {
                    body,
                    names,
                    from: HashSet::new(),
                    volume,
                },
            ));
        }
        let mut all = (*next.frames).clone();
        all.retain(|m| m.feature != id);
        all.extend(infos);
        next.frames = Arc::new(all);
        let geoms = state.geoms.clone();
        let mut out = self.finish(id, placed, next, op, geoms, PartKind::Solid)?;
        let mut warnings = Vec::new();
        if missing > 0 {
            warnings.push(if missing == 1 {
                "A selected curve no longer exists".to_string()
            } else {
                format!("{missing} selected curves no longer exist")
            });
        }
        let unused = used_override.iter().filter(|u| !**u).count();
        if unused > 0 {
            warnings.push(if unused == 1 {
                "A corner override isn't at a corner of the frame".to_string()
            } else {
                format!("{unused} corner overrides aren't at corners of the frame")
            });
        }
        if !warnings.is_empty() {
            out.warning = Some(warnings.join("; "));
        }
        Ok(out)
    }

    /// A path curve's ends, tangents and length.
    fn frame_seg(&mut self, c: PathCurve, from: &PathRef) -> Result<Seg, String> {
        let up = match (&c, from) {
            (PathCurve::Sketch { plane, .. }, _) => plane.normal.into_inner(),
            _ => V::z(),
        };
        let wire = self
            .kernel
            .create_wire(std::slice::from_ref(&c))
            .map_err(|e| format!("A frame curve can't be read: {e}"))?;
        let b = wire.bodies[0];
        let edges = self.kernel.edges(b);
        self.kernel.release(b);
        let edges = edges.map_err(|e| e.to_string())?;
        let e = edges.first().ok_or("A frame curve has no length")?;
        Ok(Seg {
            curve: c,
            start: e.start,
            end: e.end,
            t0: e.start_tangent.normalize(),
            t1: e.end_tangent.normalize(),
            length: e.length,
            line: e.curve == CurveKind::Line,
            up,
        })
    }

    /// A member's body before its corners are cut: its section extruded along it, or swept
    /// along its curves, run on at its ends as planned.
    #[allow(clippy::too_many_arguments)]
    fn frame_member(
        &mut self,
        state: &State,
        op: cadrs_kernel::OpId,
        m: &Member,
        segs: &[Seg],
        section: &[Region],
        plan: &[EndPlan; 2],
        base: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        let (r0, r1) = if m.closed {
            (0.0, 0.0)
        } else {
            (plan[0].run_on, plan[1].run_on)
        };
        let origin = m.start - m.t0 * r0;
        let (x, _) = m.axes(0);
        let plane = Plane {
            origin,
            x_dir: Unit::new_normalize(x),
            normal: Unit::new_normalize(m.t0),
        };
        let profile = Profile::new(plane, with_sources(section, base));
        let made = if m.straight {
            let length = (m.end - m.start).norm() + r0 + r1;
            self.kernel.extrude(&profile, Extent::Blind(length))
        } else {
            let mut path = Vec::new();
            if r0 > 0.0 {
                path.push(line(origin, m.start));
            }
            path.extend(m.segs.iter().map(|(i, _)| segs[*i].curve.clone()));
            if r1 > 0.0 {
                path.push(line(m.end, m.end + m.t1 * r1));
            }
            let spec = SweepSpec {
                body: BodyKind::Solid,
                path,
                control: SweepControl::None,
                corners: Default::default(),
                faces: Vec::new(),
            };
            self.kernel.sweep_with(&profile, &spec)
        }
        .map_err(|e| e.to_string())?;
        self.name_new(state, op, &made).map_err(|e| e.to_string())
    }

    /// A body cut by a plane: the side `n` points to kept.
    fn frame_cut(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        o: P,
        n: V,
        source: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        let n = n.normalize();
        let plane = Plane {
            origin: o,
            x_dir: Unit::new_normalize(square_to(&n)),
            normal: Unit::new_normalize(n),
        };
        let side = |k: &Self, b: BodyId| {
            k.kernel
                .mass_properties(b)
                .map(|m| (m.center_of_mass - o).dot(&n))
                .unwrap_or(0.0)
        };
        let split = self.kernel.split(body, &SplitTool::Plane(plane), source);
        let r = match split {
            Ok(r) if !r.bodies.is_empty() => r,
            _ => {
                // The plane misses it: all of it on one side.
                return if side(self, body) > 0.0 {
                    Ok((body, names))
                } else {
                    self.kernel.release(body);
                    Err("a corner cut leaves nothing of it".into())
                };
            }
        };
        let cut = r.bodies[0];
        let cut_names = naming::name_body(&self.kernel, cut, op, &r.history, &[(body, &names)])
            .map_err(|e| e.to_string());
        self.kernel.release(body);
        let cut_names = match cut_names {
            Ok(n) => n,
            Err(e) => {
                self.kernel.release(cut);
                return Err(e);
            }
        };
        let pieces = self.split(cut, op, &[(cut, &cut_names)]);
        self.kernel.release(cut);
        self.keep_piece(pieces?, |k, b| side(k, b))
            .ok_or_else(|| "a corner cut leaves nothing of it".to_string())
    }

    /// The piece scoring highest (its volume breaking ties), the rest released. `None` if none
    /// scores above zero.
    fn keep_piece(
        &mut self,
        pieces: Vec<Piece>,
        score: impl Fn(&Self, BodyId) -> f64,
    ) -> Option<(BodyId, BodyNames)> {
        let scored: Vec<(f64, Piece)> = pieces
            .into_iter()
            .map(|p| (score(self, p.body), p))
            .collect();
        let best = scored
            .iter()
            .enumerate()
            .filter(|(_, (s, _))| *s > 0.0)
            .max_by(|(_, (_, a)), (_, (_, b))| a.volume.total_cmp(&b.volume))
            .map(|(i, _)| i);
        let mut out = None;
        for (i, (_, p)) in scored.into_iter().enumerate() {
            if Some(i) == best {
                out = Some((p.body, p.names));
            } else {
                self.kernel.release(p.body);
            }
        }
        out
    }

    /// A member coped to another: the other subtracted, the piece reaching furthest into the
    /// member (along `way` from `at`) kept.
    fn frame_cope(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        (tool, tool_names): (BodyId, &BodyNames),
        at: P,
        way: V,
    ) -> Result<(BodyId, BodyNames), String> {
        let r = self.kernel.boolean(BoolOp::Subtract, body, &[tool]);
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                self.kernel.release(body);
                return Err(format!("a cope failed: {e}"));
            }
        };
        let cut = r.bodies[0];
        let cut_names = naming::name_body(
            &self.kernel,
            cut,
            op,
            &r.history,
            &[(body, &names), (tool, tool_names)],
        )
        .map_err(|e| e.to_string());
        self.kernel.release(body);
        let cut_names = match cut_names {
            Ok(n) => n,
            Err(e) => {
                self.kernel.release(cut);
                return Err(e);
            }
        };
        let pieces = self.split(cut, op, &[(cut, &cut_names)]);
        self.kernel.release(cut);
        // The furthest along it, whatever its size (bits of the run-on inside the other's
        // hollow or slots are nearer).
        let pieces = pieces?;
        let reach_of = |k: &Self, b: BodyId| {
            k.kernel
                .mass_properties(b)
                .map(|m| (m.center_of_mass - at).dot(&way))
                .unwrap_or(f64::MIN)
        };
        let best = pieces
            .iter()
            .map(|p| reach_of(self, p.body))
            .fold(f64::MIN, f64::max);
        self.keep_piece(
            pieces,
            |k, b| {
                if reach_of(k, b) >= best { 1.0 } else { 0.0 }
            },
        )
        .ok_or_else(|| "a cope leaves nothing of it".to_string())
    }
}

/// The section's curves and regions given sources of their own from `base`, so each member's
/// faces have names of their own.
fn with_sources(section: &[Region], base: u64) -> Vec<Region> {
    let mut next = base;
    let mut tag = |l: &Loop| Loop {
        curves: l
            .curves
            .iter()
            .map(|c| {
                next += 1;
                set_source(c.clone(), next)
            })
            .collect(),
    };
    section
        .iter()
        .enumerate()
        .map(|(i, r)| Region {
            outer: tag(&r.outer),
            holes: r.holes.iter().map(&mut tag).collect(),
            source: Some(base + 0x00f0_0000 + i as u64),
        })
        .collect()
}

fn set_source(mut c: cadrs_kernel::Curve2, s: u64) -> cadrs_kernel::Curve2 {
    use cadrs_kernel::Curve2::*;
    match &mut c {
        Line { source, .. }
        | Arc { source, .. }
        | Circle { source, .. }
        | Ellipse { source, .. }
        | EllipseArc { source, .. }
        | OffsetEllipseArc { source, .. }
        | Bezier { source, .. }
        | Spline { source, .. } => *source = Some(s),
    }
    c
}

/// A straight path curve.
fn line(a: P, b: P) -> PathCurve {
    PathCurve::Bezier3 {
        poles: vec![a, b],
        weights: Vec::new(),
    }
}

/// An end cut on a plane whose normal makes `cos` with the member: its angle from square.
fn angle_cut(cos: f64) -> EndCut {
    let a = cos.abs().clamp(0.0, 1.0).acos().to_degrees();
    if a < 0.05 {
        EndCut::Square
    } else {
        EndCut::Angle(a)
    }
}

/// The path's curves gathered into members: with `merge`, curves meeting tangentially end to
/// end (just two at a point) are one member.
fn members(segs: &[Seg], merge: bool, tol: f64) -> Vec<Member> {
    let n = segs.len();
    // Where each curve's ends are.
    let mut points: Vec<P> = Vec::new();
    let mut at = |p: P| -> usize {
        match points.iter().position(|q| (q - p).norm() < tol * 10.0) {
            Some(i) => i,
            None => {
                points.push(p);
                points.len() - 1
            }
        }
    };
    let ends: Vec<[usize; 2]> = segs.iter().map(|s| [at(s.start), at(s.end)]).collect();
    let mut meeting: Vec<Vec<(usize, usize)>> = vec![Vec::new(); points.len()];
    for (i, e) in ends.iter().enumerate() {
        for k in 0..2 {
            meeting[e[k]].push((i, k));
        }
    }
    // The way out of a curve at one of its ends.
    let out = |i: usize, k: usize| if k == 0 { segs[i].t0 } else { -segs[i].t1 };
    let joined: Vec<bool> = meeting
        .iter()
        .map(|m| {
            merge
                && m.len() == 2
                && m[0].0 != m[1].0
                && out(m[0].0, m[0].1).dot(&out(m[1].0, m[1].1)) < -TANGENT.cos()
        })
        .collect();
    // The other curve end at a joined point.
    let other = |p: usize, me: (usize, usize)| meeting[p].iter().copied().find(|x| *x != me);
    let mut used = vec![false; n];
    let mut out_members = Vec::new();
    for s in 0..n {
        if used[s] {
            continue;
        }
        // Back to where the run starts (its first curve, and whether it runs backwards).
        let (mut cur, mut rev) = (s, false);
        let mut closed = false;
        loop {
            let behind = if rev { 1 } else { 0 };
            let p = ends[cur][behind];
            if !joined[p] {
                break;
            }
            let Some((o, k)) = other(p, (cur, behind)) else {
                break;
            };
            // Arriving at o's end k going backwards: o runs forwards if k is its end.
            cur = o;
            rev = k == 0;
            if cur == s {
                closed = true;
                break;
            }
        }
        let first = (cur, rev);
        let mut run = vec![first];
        used[cur] = true;
        loop {
            let ahead = if rev { 0 } else { 1 };
            let p = ends[cur][ahead];
            if !joined[p] {
                break;
            }
            let Some((o, k)) = other(p, (cur, ahead)) else {
                break;
            };
            if (o, k == 1) == first || used[o] {
                closed = closed || o == first.0;
                break;
            }
            cur = o;
            rev = k == 1;
            used[cur] = true;
            run.push((cur, rev));
        }
        let s0 = &segs[run[0].0];
        let sl = &segs[run[run.len() - 1].0];
        let (start, t0) = if run[0].1 {
            (s0.end, -s0.t1)
        } else {
            (s0.start, s0.t0)
        };
        let (end, t1) = if run[run.len() - 1].1 {
            (sl.start, -sl.t0)
        } else {
            (sl.end, sl.t1)
        };
        let single_closed = run.len() == 1 && (s0.start - s0.end).norm() < tol * 10.0;
        let closed = closed || single_closed;
        let straight = !closed
            && run
                .iter()
                .all(|(i, _)| segs[*i].line && segs[*i].t0.dot(&t0).abs() > 1.0 - 1e-9);
        out_members.push(Member {
            length: run.iter().map(|(i, _)| segs[*i].length).sum(),
            up: s0.up,
            segs: run,
            start,
            end,
            t0,
            t1,
            closed,
            straight,
        });
    }
    out_members
}

/// Plans the end `(b, end)` of member `bm` cut against the member `through` (running along
/// `a` at `v`, its section's axes `a_axes` there): square against its side (a butt) or to
/// its shape (coped). `through.1` is the end of it at `v`, which runs on to the far side of
/// `bm` (`None`: it runs through `v`).
#[allow(clippy::too_many_arguments)]
fn plan_butt(
    section: &[Region],
    through: (usize, Option<usize>),
    a: V,
    a_axes: (V, V),
    bm: &Member,
    (b, end): (usize, usize),
    v: P,
    coped: bool,
    margin: f64,
    plans: &mut [[EndPlan; 2]],
) {
    let into = bm.inward(end);
    let across = into - a * into.dot(&a);
    if across.norm() < 1e-6 {
        // In line with it: nothing to cut against.
        return;
    }
    let n = across.normalize();
    let k = into.dot(&n);
    let b_axes = bm.axes(end);
    // The through member's side facing this one, and its far side.
    let near = reach(section, a_axes, &n);
    let far = reach(section, a_axes, &-n);
    let mine = reach(section, b_axes, &n);
    let plan = &mut plans[b][end];
    if coped {
        plan.run_on = plan.run_on.max(((mine + far) / k).max(0.0) + margin);
        plan.copes.push((through.0, v, into));
        plan.cut = Some(EndCut::Coped);
    } else {
        plan.run_on = plan.run_on.max(((mine - near) / k).max(0.0) + margin);
        plan.planes.push((v + n * near, n));
        plan.cut = Some(angle_cut(k));
    }
    // The through member runs on to this one's far side.
    if let (t, Some(t_end)) = through {
        let w = reach(section, b_axes, &-a);
        let plan = &mut plans[t][t_end];
        let keep = plan.planes.iter().position(|(_, n)| (n - a).norm() < 1e-9);
        match keep {
            Some(i) => {
                // Several butt against it: the furthest of them.
                let (o, _) = plan.planes[i];
                if (v - o).dot(&a) < w {
                    plan.planes[i] = (v - a * w, a);
                }
            }
            None => plan.planes.push((v - a * w, a)),
        }
        plan.run_on = plan.run_on.max(w.max(0.0) + margin);
        plan.cut.get_or_insert(EndCut::Square);
    }
}
