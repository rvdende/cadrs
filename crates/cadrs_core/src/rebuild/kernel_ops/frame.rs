//! Rebuilding the Frame feature (`crate::frame`, issue #266): the path's curves gathered into
//! members and turned to run head to tail, each member's section extruded (straight) or swept
//! (curved) with run-ons where a corner needs them, the corners cut by planes (mitres and
//! butts) or by the member run on (copes), trims by other parts, end caps and gussets, and
//! every member and plate made a part. A child of `rebuild::kernel_ops`.
//!
//! A member's part id and its faces' names come from the path curve it starts with (the
//! sketch curve, the edge), not from where it is in the list, so adding a curve to the path or
//! merging tangent ones leaves the other members' references alone.

use super::advanced::sketch_of;
use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{
    BodyKind, BoolOp, CurveKind, Extent, Kernel, Loop, PathCurve, Plane, Profile, Region,
    SplitTool, SweepControl, SweepSpec, Tessellation,
};
use nalgebra::{Point3, Unit, Vector3};

use crate::advanced::PathRef;
use crate::frame::{self, CornerType, EndCut, FrameFeature, MemberInfo, ProfileFamily};

type P = Point3<f64>;
type V = Vector3<f64>;
/// A member's end: the member and 0 (its start) or 1 (its end).
type End = (usize, usize);

/// Curves meeting at an angle below this are tangent (radians).
const TANGENT: f64 = 0.5 * std::f64::consts::PI / 180.0;

/// A circular arc of the path: its centre, the unit normal it turns about (counter-clockwise),
/// its radius and the angle it sweeps.
#[derive(Clone, Copy)]
struct Arc3 {
    center: P,
    normal: V,
    radius: f64,
}

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
    /// A circular arc's circle.
    arc: Option<Arc3>,
    /// The section's up: the curve's sketch normal, else the world's Z.
    up: V,
    /// Who it is: from the path reference and its place among the reference's curves.
    key: u64,
}

/// A member: curves run end to end (each with whether it is run backwards).
#[derive(Clone)]
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
    key: u64,
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

    /// The same member run the other way.
    fn reverse(&mut self) {
        self.segs.reverse();
        for s in &mut self.segs {
            s.1 = !s.1;
        }
        std::mem::swap(&mut self.start, &mut self.end);
        let (t0, t1) = (self.t0, self.t1);
        self.t0 = -t1;
        self.t1 = -t0;
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
    /// Members whose outline cuts it (copes), and a point and the way into the member (to keep
    /// the pieces on its side).
    copes: Vec<(usize, P, V)>,
    cut: Option<EndCut>,
}

/// What an end is cut against at a corner: the members there that run through it (one, or two
/// in line meeting there), the way they run and their section's axes there, and the end of one
/// of them that runs on to the far side of the others (`None`: they run through the point).
struct Through {
    members: Vec<usize>,
    a: V,
    axes: (V, V),
    runs_on: Option<End>,
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
        // What a cope cuts to: the section's outline filled (a tube's bore, a T-slot's slots).
        let envelope = envelope(&section, x.family);
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
            let who = path_identity(p);
            for (k, c) in curves.into_iter().enumerate() {
                let key = naming::stable_hash(format!("{who}/{k}").as_bytes());
                segs.push(self.frame_seg(c, key)?);
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
        let mut members = members(&segs, x.merge_tangent, tol);
        orient(&mut members, tol);

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
        let corners = corners_of(&members, tol);
        let override_tol = 1e-4 * (1.0 + size);
        // Free ends (for end caps) and two-member corners (for gussets).
        let mut free_ends: Vec<End> = Vec::new();
        let mut gusset_at: Vec<(P, End, End)> = Vec::new();
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
            // A member running through the point (a line's or an arc's span): the ends here
            // are cut against it.
            let passing = members
                .iter()
                .enumerate()
                .find_map(|(i, m)| passes(&segs, m, v, tol * 10.0).map(|(a, axes)| (i, a, axes)));
            match (&passing, ends.as_slice()) {
                (None, [e]) => free_ends.push(*e),
                (None, [e0, e1]) => {
                    let (a, b) = (members[e0.0].inward(e0.1), members[e1.0].inward(e1.1));
                    if a.dot(&b).abs() < TANGENT.cos() {
                        gusset_at.push((*v, *e0, *e1));
                    }
                }
                _ => {}
            }
            if kind == CornerType::None {
                continue;
            }
            let coped = kind == CornerType::CopedButt;
            if let Some((i, a, axes)) = passing {
                let through = Through {
                    members: vec![i],
                    a,
                    axes,
                    runs_on: None,
                };
                for &e in ends {
                    plan_butt(
                        &section, &through, &members, e, *v, coped, margin, &mut plans,
                    );
                }
                continue;
            }
            match ends.as_slice() {
                [] | [_] => {}
                [e0, e1] => {
                    let (a, b) = (members[e0.0].inward(e0.1), members[e1.0].inward(e1.1));
                    // Running straight on: two pieces end to end.
                    if a.dot(&b) < -TANGENT.cos() {
                        continue;
                    }
                    if a.dot(&b) > TANGENT.cos() {
                        return Err("Two members fold back on each other at a corner".into());
                    }
                    if kind == CornerType::Miter {
                        let n = (a - b).normalize();
                        for (e, n) in [(*e0, n), (*e1, -n)] {
                            let m = &members[e.0];
                            let k = m.inward(e.1).dot(&n);
                            let over = reach(&section, m.axes(e.1), &n) / k;
                            let plan = &mut plans[e.0][e.1];
                            plan.run_on = plan.run_on.max(over.max(0.0) + margin);
                            plan.planes.push((*v, n));
                            plan.cut = Some(angle_cut(k));
                        }
                        continue;
                    }
                    // The longer runs on (the first in the path if they are as long).
                    let longer = |p: &End, q: &End| {
                        let (lp, lq) = (members[p.0].length, members[q.0].length);
                        lp > lq + tol || ((lp - lq).abs() <= tol && p.0 < q.0)
                    };
                    let (mut t, mut o) = if longer(e0, e1) {
                        (*e0, *e1)
                    } else {
                        (*e1, *e0)
                    };
                    if flip {
                        std::mem::swap(&mut t, &mut o);
                    }
                    let through = Through {
                        members: vec![t.0],
                        a: members[t.0].inward(t.1),
                        axes: members[t.0].axes(t.1),
                        runs_on: Some(t),
                    };
                    plan_butt(
                        &section, &through, &members, o, *v, coped, margin, &mut plans,
                    );
                }
                _ => {
                    // Two in line make one run through the point (a crossbar's halves), the
                    // others cut against them; else the longest runs on (the next with flip).
                    let inline = ends.iter().enumerate().find_map(|(i, p)| {
                        ends[i + 1..].iter().find_map(|q| {
                            let (a, b) = (members[p.0].inward(p.1), members[q.0].inward(q.1));
                            (a.dot(&b) < -TANGENT.cos()).then_some((*p, *q))
                        })
                    });
                    let through = match inline {
                        Some((p, q)) => Through {
                            members: vec![p.0, q.0],
                            a: members[p.0].inward(p.1),
                            axes: members[p.0].axes(p.1),
                            runs_on: None,
                        },
                        None => {
                            let mut order = ends.clone();
                            order.sort_by(|p, q| {
                                members[q.0]
                                    .length
                                    .total_cmp(&members[p.0].length)
                                    .then(p.0.cmp(&q.0))
                            });
                            let t = order[usize::from(flip)];
                            Through {
                                members: vec![t.0],
                                a: members[t.0].inward(t.1),
                                axes: members[t.0].axes(t.1),
                                runs_on: Some(t),
                            }
                        }
                    };
                    for e in ends {
                        let theirs = through.members.contains(&e.0)
                            && (through.runs_on.is_none_or(|t| t == *e));
                        if !theirs {
                            plan_butt(
                                &section, &through, &members, *e, *v, coped, margin, &mut plans,
                            );
                        }
                    }
                }
            }
        }

        // The members' bodies, run on and cut by their planes.
        let mut bodies: Vec<(BodyId, BodyNames)> = Vec::new();
        let release = |k: &mut Self, bodies: &[(BodyId, BodyNames)]| {
            for (b, _) in bodies {
                k.kernel.release(*b);
            }
        };
        for (i, m) in members.iter().enumerate() {
            match self.frame_cut_member(state, op, m, &segs, &section, &plans[i], base_of(m.key)) {
                Ok(b) => bodies.push(b),
                Err(e) => {
                    release(self, &bodies);
                    return Err(format!("Member {} failed: {e}", i + 1));
                }
            }
        }
        // Copes, by the outlines of the members they butt against (cut as those are).
        let mut tools: HashMap<usize, (BodyId, BodyNames)> = HashMap::new();
        let mut coping = (|| -> Result<(), String> {
            for i in 0..members.len() {
                for plan in plans[i].clone() {
                    for (by, at, way) in plan.copes {
                        if !tools.contains_key(&by) {
                            let m = &members[by];
                            let base = base_of(m.key) + 0x00e0_0000;
                            let t = self
                                .frame_cut_member(state, op, m, &segs, &envelope, &plans[by], base)
                                .map_err(|e| format!("Member {} failed: {e}", by + 1))?;
                            tools.insert(by, t);
                        }
                        let tool = &tools[&by];
                        let tool = (tool.0, tool.1.clone());
                        let b = (bodies[i].0, bodies[i].1.clone());
                        // (`frame_cope` releases `b` either way.)
                        match self.frame_cope(op, b, (tool.0, &tool.1), at, way, r_section) {
                            Ok(c) => bodies[i] = c,
                            Err(e) => {
                                bodies[i].0 = BodyId(u64::MAX);
                                return Err(format!("Member {} failed: {e}", i + 1));
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        for (_, (b, _)) in tools.drain() {
            self.kernel.release(b);
        }
        if let Err(e) = coping.as_mut() {
            bodies.retain(|(b, _)| b.0 != u64::MAX);
            release(self, &bodies);
            return Err(std::mem::take(e));
        }
        // Trims by other parts: each member less what overlaps them, its largest piece kept.
        for p in &x.trim {
            let Some(tool) = state.part(*p).and_then(|p| p.body) else {
                continue;
            };
            for i in 0..bodies.len() {
                let near = match (
                    self.kernel.bounding_box(tool),
                    self.kernel.bounding_box(bodies[i].0),
                ) {
                    (Ok(a), Ok(b)) => (0..3).all(|k| a.min[k] <= b.max[k] && b.min[k] <= a.max[k]),
                    _ => false,
                };
                if !near {
                    continue;
                }
                let b = (bodies[i].0, bodies[i].1.clone());
                match self.frame_trim(op, b, tool, state) {
                    Ok(t) => bodies[i] = t,
                    Err(e) => {
                        let rest: Vec<(BodyId, BodyNames)> = bodies
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, b)| b.clone())
                            .collect();
                        release(self, &rest);
                        return Err(format!("Member {} failed: {e}", i + 1));
                    }
                }
            }
        }

        // End caps and gussets, parts of their own after the members.
        let mut plates: Vec<((BodyId, BodyNames), String, u64)> = Vec::new();
        let mut plate_made = (|| -> Result<(), String> {
            if x.end_caps && section.iter().any(|r| !r.holes.is_empty()) {
                let label = frame::cap_label(x.cap_thickness, &profile_label);
                for &(m, end) in &free_ends {
                    let mb = &members[m];
                    let key = naming::stable_hash(format!("cap/{}/{end}", mb.key).as_bytes());
                    let b = self.frame_cap(
                        state,
                        op,
                        mb,
                        end,
                        &section,
                        x.cap_thickness,
                        base_of(key),
                    )?;
                    plates.push((b, label.clone(), key));
                }
            }
            if x.gussets {
                let label = frame::gusset_label(x.gusset_size, x.gusset_thickness);
                for &(v, e0, e1) in &gusset_at {
                    let (k0, k1) = (members[e0.0].key, members[e1.0].key);
                    let key = naming::stable_hash(
                        format!("gusset/{}/{}", k0.min(k1), k0.max(k1)).as_bytes(),
                    );
                    let made = self.frame_gusset(
                        state,
                        op,
                        &section,
                        v,
                        (&members[e0.0], e0.1),
                        (&members[e1.0], e1.1),
                        (x.gusset_size, x.gusset_thickness),
                        base_of(key),
                    )?;
                    plates.push((made, label.clone(), key));
                }
            }
            Ok(())
        })();
        if let Err(e) = plate_made.as_mut() {
            for (b, _) in bodies.iter().chain(plates.iter().map(|(b, ..)| b)) {
                self.kernel.release(*b);
            }
            return Err(std::mem::take(e));
        }

        // The parts, and what the cut list says of them.
        let mut next = (**state).clone();
        let mut placed: Vec<Placed> = Vec::new();
        let mut infos: Vec<MemberInfo> = Vec::new();
        let standard = (x.family != ProfileFamily::Custom).then_some(x.family);
        for (i, (body, names)) in bodies.into_iter().enumerate() {
            let m = &members[i];
            let part = part_id(id, m.key, &next, &placed);
            let volume = self
                .kernel
                .mass_properties(body)
                .map(|p| p.volume)
                .unwrap_or(0.0);
            let length = if m.straight {
                self.long_point(body, m).unwrap_or(m.length)
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
                plate: false,
                family: standard,
                size: if standard.is_some() {
                    x.size.clone()
                } else {
                    String::new()
                },
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
        for ((body, names), label, key) in plates {
            let part = part_id(id, key, &next, &placed);
            let volume = self
                .kernel
                .mass_properties(body)
                .map(|p| p.volume)
                .unwrap_or(0.0);
            infos.push(MemberInfo {
                feature: id,
                part,
                profile: label,
                length: 0.0,
                ends: [EndCut::Square; 2],
                curved: false,
                volume,
                plate: true,
                family: None,
                size: String::new(),
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
        let gone = x.trim.iter().filter(|p| state.part(**p).is_none()).count();
        if gone > 0 {
            warnings.push("A part to trim by no longer exists".to_string());
        }
        if !warnings.is_empty() {
            out.warning = Some(warnings.join("; "));
        }
        Ok(out)
    }

    /// A path curve's ends, tangents, length and circle.
    fn frame_seg(&mut self, c: PathCurve, key: u64) -> Result<Seg, String> {
        let up = match &c {
            PathCurve::Sketch { plane, .. } => plane.normal.into_inner(),
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
        let (t0, t1) = (e.start_tangent.normalize(), e.end_tangent.normalize());
        let arc = e.circle.map(|c| {
            // Turning counter-clockwise about the normal the way the curve runs.
            let r0 = e.start - c.center;
            let n = c.normal.into_inner();
            let n = if r0.cross(&t0).dot(&n) >= 0.0 { n } else { -n };
            Arc3 {
                center: c.center,
                normal: n,
                radius: c.radius,
            }
        });
        Ok(Seg {
            curve: c,
            start: e.start,
            end: e.end,
            t0,
            t1,
            length: e.length,
            line: e.curve == CurveKind::Line,
            arc,
            up,
            key,
        })
    }

    /// A member's body: run on and cut by its planes, with `section`.
    #[allow(clippy::too_many_arguments)]
    fn frame_cut_member(
        &mut self,
        state: &State,
        op: cadrs_kernel::OpId,
        m: &Member,
        segs: &[Seg],
        section: &[Region],
        plan: &[EndPlan; 2],
        base: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        let mut b = self.frame_member(state, op, m, segs, section, plan, base)?;
        let mut cut_source = base + 0x00ff_0000;
        for p in plan {
            for &(o, n) in &p.planes {
                cut_source += 1;
                b = self.frame_cut(op, b, o, n, cut_source)?;
            }
        }
        Ok(b)
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

    /// A straight member's overall length (its long point to long point) along it, from its
    /// faces' fine mesh: exact for flat cuts, within the mesh's 1e-3 mm for curved ones (a
    /// round tube's mitre is an ellipse with no vertex at its tip).
    fn long_point(&self, body: BodyId, m: &Member) -> Option<f64> {
        let mesh = self
            .kernel
            .tessellate(
                body,
                Tessellation {
                    deflection: 1e-3,
                    angle: 2f64.to_radians(),
                },
            )
            .ok()?;
        let along = mesh.positions.iter().map(|p| (p - m.start).dot(&m.t0));
        let (lo, hi) = along.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), a| {
            (lo.min(a), hi.max(a))
        });
        (hi > lo).then_some(hi - lo)
    }

    /// An end cap: the section's outer outline (no holes), `thickness` thick, against the end
    /// `end` of the member, outside it.
    #[allow(clippy::too_many_arguments)]
    fn frame_cap(
        &mut self,
        state: &State,
        op: cadrs_kernel::OpId,
        m: &Member,
        end: usize,
        section: &[Region],
        thickness: f64,
        base: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        let out = -m.inward(end);
        let (x, _) = m.axes(end);
        // Seen from outside the start, right is the member's left: x flipped, the section
        // mirrored to match.
        let x = if end == 0 { -x } else { x };
        let solid: Vec<Region> = section
            .iter()
            .map(|r| Region {
                outer: r.outer.clone(),
                holes: Vec::new(),
                source: r.source,
            })
            .collect();
        let solid = if end == 0 {
            frame::place(&solid, frame::Anchor::Origin, true, 0.0)
        } else {
            solid
        };
        let plane = Plane {
            origin: m.point(end),
            x_dir: Unit::new_normalize(x),
            normal: Unit::new_normalize(out),
        };
        let made = self
            .kernel
            .extrude(
                &Profile::new(plane, with_sources(&solid, base)),
                Extent::Blind(thickness),
            )
            .map_err(|e| format!("an end cap failed: {e}"))?;
        self.name_new(state, op, &made).map_err(|e| e.to_string())
    }

    /// A gusset in the angle between the ends `a` and `b` of two members meeting at `v`: a
    /// triangle from where their inner sides meet, its legs `size` along them, `thickness`
    /// thick about the middle of the section across the corner's plane.
    #[allow(clippy::too_many_arguments)]
    fn frame_gusset(
        &mut self,
        state: &State,
        op: cadrs_kernel::OpId,
        section: &[Region],
        v: P,
        (am, ae): (&Member, usize),
        (bm, be): (&Member, usize),
        (size, thickness): (f64, f64),
        base: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        let (a, b) = (am.inward(ae), bm.inward(be));
        let n = a.cross(&b).normalize();
        // Each member's inner side: across it towards the other, as far as the section reaches.
        let na = (b - a * a.dot(&b)).normalize();
        let nb = (a - b * b.dot(&a)).normalize();
        let ha = reach(section, am.axes(ae), &na);
        let hb = reach(section, bm.axes(be), &nb);
        // Where the inner sides meet: v + s a + ha na = v + r b + hb nb, in the corner's plane
        // (u along a, w across it towards b).
        let (u, w) = (a, n.cross(&a));
        let p1 = (na * ha).dot(&w);
        let (bu, bw) = (b.dot(&u), b.dot(&w));
        let c = [(nb * hb).dot(&u), (nb * hb).dot(&w)];
        if bw.abs() < 1e-9 {
            return Err("a gusset's members run in line".into());
        }
        let r = (p1 - c[1]) / bw;
        let s = r * bu + c[0];
        // In the middle of the section across the corner's plane.
        let mid = (reach(section, am.axes(ae), &n) - reach(section, am.axes(ae), &-n)) / 2.0;
        let p0 = v + a * s + w * p1 + n * (mid - thickness / 2.0);
        let plane = Plane {
            origin: p0,
            x_dir: Unit::new_normalize(u),
            normal: Unit::new_normalize(n),
        };
        let pt = |x: f64, y: f64| nalgebra::Point2::new(x, y);
        let corners = [pt(0.0, 0.0), pt(size, 0.0), pt(size * bu, size * bw)];
        let curves = (0..3)
            .map(|i| cadrs_kernel::Curve2::Line {
                a: corners[i],
                b: corners[(i + 1) % 3],
                source: None,
            })
            .collect();
        let region = Region {
            outer: Loop { curves },
            holes: Vec::new(),
            source: None,
        };
        let made = self
            .kernel
            .extrude(
                &Profile::new(plane, with_sources(&[region], base)),
                Extent::Blind(thickness),
            )
            .map_err(|e| format!("a gusset failed: {e}"))?;
        self.name_new(state, op, &made).map_err(|e| e.to_string())
    }

    /// A body cut by a plane: what lies on the side `n` points to kept (every piece of it: a
    /// custom section of several regions).
    fn frame_cut(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        o: P,
        n: V,
        source: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        let n = n.normalize();
        // A plane clear of the body: all of it on one side, nothing to cut.
        if let Ok(bb) = self.kernel.bounding_box(body) {
            let hi = bb.max_along(o, &n);
            let lo = -bb.max_along(o, &-n);
            if lo >= 0.0 {
                return Ok((body, names));
            }
            if hi <= 0.0 {
                self.kernel.release(body);
                return Err("a corner cut leaves nothing of it".into());
            }
        }
        let plane = Plane {
            origin: o,
            x_dir: Unit::new_normalize(square_to(&n)),
            normal: Unit::new_normalize(n),
        };
        let r = match self.kernel.split(body, &SplitTool::Plane(plane), source) {
            Ok(r) if !r.bodies.is_empty() => r,
            Ok(_) => {
                self.kernel.release(body);
                return Err("a corner cut failed".into());
            }
            Err(e) => {
                self.kernel.release(body);
                return Err(format!("a corner cut failed: {e}"));
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
        let side = |k: &Self, b: BodyId| {
            k.kernel
                .mass_properties(b)
                .map(|m| (m.center_of_mass - o).dot(&n))
                .unwrap_or(0.0)
        };
        self.keep_pieces(op, pieces?, |k, b| side(k, b) > 0.0)?
            .ok_or_else(|| "a corner cut leaves nothing of it".to_string())
    }

    /// The pieces `keep` takes, as one body (several gathered), the rest released; `None` if it
    /// takes none.
    fn keep_pieces(
        &mut self,
        op: cadrs_kernel::OpId,
        pieces: Vec<Piece>,
        keep: impl Fn(&Self, BodyId) -> bool,
    ) -> Result<Option<(BodyId, BodyNames)>, String> {
        let (kept, dropped): (Vec<Piece>, Vec<Piece>) =
            pieces.into_iter().partition(|p| keep(self, p.body));
        for p in dropped {
            self.kernel.release(p.body);
        }
        match kept.len() {
            0 => Ok(None),
            1 => Ok(kept.into_iter().next().map(|p| (p.body, p.names))),
            _ => {
                let ids: Vec<BodyId> = kept.iter().map(|p| p.body).collect();
                let made = self.kernel.compound(&ids).map_err(|e| e.to_string());
                let named = made.and_then(|r| {
                    let inputs: Vec<(BodyId, &BodyNames)> =
                        kept.iter().map(|p| (p.body, &p.names)).collect();
                    let b = r.bodies[0];
                    naming::name_body(&self.kernel, b, op, &r.history, &inputs)
                        .map(|n| (b, n))
                        .map_err(|e| e.to_string())
                });
                for p in &kept {
                    self.kernel.release(p.body);
                }
                named.map(Some)
            }
        }
    }

    /// A member coped to another's outline: the outline subtracted, the pieces reaching on into
    /// the member (along `way` from `at`, as far as its furthest within the section's size)
    /// kept: not the bits of the run-on beyond the other member.
    fn frame_cope(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        (tool, tool_names): (BodyId, &BodyNames),
        at: P,
        way: V,
        r_section: f64,
    ) -> Result<(BodyId, BodyNames), String> {
        let r = match self.kernel.boolean(BoolOp::Subtract, body, &[tool]) {
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
        let pieces = self.split(cut, op, &[(cut, &cut_names)])?;
        self.kernel.release(cut);
        let far = |k: &Self, b: BodyId| {
            k.kernel
                .bounding_box(b)
                .map(|bb| bb.max_along(at, &way))
                .unwrap_or(f64::MIN)
        };
        let best = pieces
            .iter()
            .map(|p| far(self, p.body))
            .fold(f64::MIN, f64::max);
        self.keep_pieces(op, pieces, |k, b| {
            let f = far(k, b);
            f > 0.0 && f >= best - 2.0 * r_section
        })?
        .ok_or_else(|| "a cope leaves nothing of it".to_string())
    }

    /// A member trimmed by a part: the part subtracted, the largest piece kept.
    fn frame_trim(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        tool: BodyId,
        state: &State,
    ) -> Result<(BodyId, BodyNames), String> {
        let r = match self.kernel.boolean(BoolOp::Subtract, body, &[tool]) {
            Ok(r) => r,
            // Nothing in common.
            Err(cadrs_kernel::KernelError::EmptyResult(_)) => {
                self.kernel.release(body);
                return Err("a trim leaves nothing of it".into());
            }
            Err(e) => {
                self.kernel.release(body);
                return Err(format!("a trim failed: {e}"));
            }
        };
        let cut = r.bodies[0];
        let tool_names = state
            .parts
            .iter()
            .find(|p| p.body == Some(tool))
            .map(|p| p.names.clone())
            .unwrap_or_default();
        let cut_names = naming::name_body(
            &self.kernel,
            cut,
            op,
            &r.history,
            &[(body, &names), (tool, &tool_names)],
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
        let pieces = self.split(cut, op, &[(cut, &cut_names)])?;
        self.kernel.release(cut);
        let largest = pieces.iter().map(|p| p.volume).fold(f64::MIN, f64::max);
        self.keep_pieces(op, pieces, |k, b| {
            k.kernel
                .mass_properties(b)
                .is_ok_and(|m| m.volume >= largest * (1.0 - 1e-9))
        })?
        .ok_or_else(|| "a trim leaves nothing of it".to_string())
    }
}

/// Who a path reference is, for its curves' keys: the sketch and curve, the edge's name, the
/// sketch or curve feature.
fn path_identity(p: &PathRef) -> String {
    match p {
        PathRef::Edge(e) => format!("edge/{:?}", e.edge),
        PathRef::SketchCurve { sketch, curve } => format!("curve/{:?}/{:?}", sketch.0, curve),
        PathRef::Sketch(s) => format!("sketch/{:?}", s.0),
        PathRef::Curve(f) => format!("feature/{:?}", f.0),
    }
}

/// The base of a member's faces' sources (its curves `base + k`, its regions and cuts above).
fn base_of(key: u64) -> u64 {
    (key & 0xffff_ffff) << 24
}

/// A part id of the feature from `key`, not taken by a part before or one placed already.
fn part_id(id: FeatureId, key: u64, next: &State, placed: &[Placed]) -> PartId {
    let mut i = (key & 0x3fff_ffff) as u32;
    loop {
        let p = PartId::new(id, i);
        if next.part(p).is_none() && !placed.iter().any(|(q, _)| *q == p) {
            return p;
        }
        i = i.wrapping_add(1) & 0x3fff_ffff;
    }
}

/// What a cope cuts to: a T-slot extrusion's box (its slots closed), any other section's
/// outer outlines with their holes filled.
fn envelope(section: &[Region], family: ProfileFamily) -> Vec<Region> {
    if family == ProfileFamily::TSlot {
        let [x0, y0, x1, y1] = frame::bounds(section);
        let p = |x: f64, y: f64| nalgebra::Point2::new(x, y);
        let c = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
        let curves = (0..4)
            .map(|i| cadrs_kernel::Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: None,
            })
            .collect();
        return vec![Region {
            outer: Loop { curves },
            holes: Vec::new(),
            source: None,
        }];
    }
    section
        .iter()
        .map(|r| Region {
            outer: r.outer.clone(),
            holes: Vec::new(),
            source: r.source,
        })
        .collect()
}

/// Where a member runs through `v` (anywhere along it but its own two ends, the joints of its
/// merged curves included): the way it runs there and its section's axes.
fn passes(segs: &[Seg], m: &Member, v: &P, tol: f64) -> Option<(V, (V, V))> {
    if !m.closed && ((v - m.start).norm() < tol || (v - m.end).norm() < tol) {
        return None;
    }
    if m.straight {
        let along = (v - m.start).dot(&m.t0);
        let off = (v - m.start) - m.t0 * along;
        return (off.norm() < tol && along > tol && along < m.length - tol)
            .then(|| (m.t0, axes(m.t0, m.up)));
    }
    for &(i, rev) in &m.segs {
        let s = &segs[i];
        let t = if s.line {
            let d = (s.end - s.start).normalize();
            let along = (v - s.start).dot(&d);
            let off = (v - s.start) - d * along;
            (off.norm() < tol && along > -tol && along < s.length + tol).then_some(d)
        } else if let Some(c) = s.arc {
            let r = v - c.center;
            let in_plane = r.dot(&c.normal).abs() < tol;
            let on = (r.norm() - c.radius).abs() < tol;
            // Within the arc: between its ends the way it turns.
            let x = (s.start - c.center).normalize();
            let y = c.normal.cross(&x);
            let ang = |p: &P| {
                let q = p - c.center;
                q.dot(&y).atan2(q.dot(&x)).rem_euclid(std::f64::consts::TAU)
            };
            let closed = (s.start - s.end).norm() < tol;
            let (end, here) = (ang(&s.end), ang(v));
            let slack = tol / c.radius;
            let within = closed || here < end + slack || here > std::f64::consts::TAU - slack;
            (in_plane && on && within).then(|| c.normal.cross(&r).normalize())
        } else {
            None
        };
        if let Some(t) = t {
            let t = if rev { -t } else { t };
            return Some((t, axes(t, m.up)));
        }
    }
    None
}

/// Member ends grouped by where they are.
fn corners_of(members: &[Member], tol: f64) -> Vec<(P, Vec<End>)> {
    let mut corners: Vec<(P, Vec<End>)> = Vec::new();
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
    corners
}

/// Turns members to run head to tail where two meet (whichever way their curves were drawn),
/// so a section that isn't symmetric left for right sits the same way along a run, and gives
/// a run that lies in a plane that plane's normal as its up (an edge path's members, or a run
/// across sketches, would otherwise each take their own).
fn orient(members: &mut [Member], tol: f64) {
    let corners = corners_of(members, tol);
    let links: Vec<(End, End)> = corners
        .iter()
        .filter_map(|(_, ends)| match ends.as_slice() {
            [a, b] if a.0 != b.0 => Some((*a, *b)),
            _ => None,
        })
        .collect();
    let mut seen = vec![false; members.len()];
    // (The links name ends as they were: a member turned round has them the other way.)
    let mut turned = vec![false; members.len()];
    for start in 0..members.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut run = vec![start];
        let mut queue = vec![start];
        while let Some(m) = queue.pop() {
            for &(a, b) in &links {
                let (mine, other) = if a.0 == m {
                    (a, b)
                } else if b.0 == m {
                    (b, a)
                } else {
                    continue;
                };
                if seen[other.0] {
                    continue;
                }
                // Head to tail: one's end meets the other's start.
                let now = |e: End| e.1 ^ usize::from(turned[e.0]);
                if now(mine) == now(other) {
                    members[other.0].reverse();
                    turned[other.0] = !turned[other.0];
                }
                seen[other.0] = true;
                run.push(other.0);
                queue.push(other.0);
            }
        }
        if run.len() < 2 {
            continue;
        }
        // The run's plane, if it has one.
        let dirs: Vec<V> = run
            .iter()
            .flat_map(|&i| [members[i].t0, members[i].t1])
            .collect();
        let Some(n) = dirs.iter().enumerate().find_map(|(i, a)| {
            dirs[i + 1..].iter().find_map(|b| {
                let c = a.cross(b);
                (c.norm() > 1e-3).then(|| c.normalize())
            })
        }) else {
            continue;
        };
        let p0 = members[run[0]].start;
        let planar = run.iter().all(|&i| {
            let m = &members[i];
            [m.t0, m.t1].iter().all(|t| t.dot(&n).abs() < 1e-6)
                && [m.start, m.end]
                    .iter()
                    .all(|p| (p - p0).dot(&n).abs() < tol * 10.0)
        });
        if !planar {
            continue;
        }
        // The way the first member's own up has it, else upwards.
        let own = members[run[0]].up;
        let sign = if own.dot(&n).abs() > 1e-6 {
            own.dot(&n).signum()
        } else if n.z.abs() > 1e-6 {
            n.z.signum()
        } else if n.y.abs() > 1e-6 {
            n.y.signum()
        } else {
            n.x.signum()
        };
        for &i in &run {
            members[i].up = n * sign;
        }
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
            if used[o] {
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
            key: run.iter().map(|(i, _)| segs[*i].key).min().unwrap_or(0),
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

/// Plans the end `e` of a member cut against the members `through` at `v`: square against
/// their side (a butt) or to their outline (coped); the one of them that ends at `v` (if any)
/// runs on to this member's far side.
#[allow(clippy::too_many_arguments)]
fn plan_butt(
    section: &[Region],
    through: &Through,
    members: &[Member],
    (b, end): End,
    v: P,
    coped: bool,
    margin: f64,
    plans: &mut [[EndPlan; 2]],
) {
    let a = through.a;
    let bm = &members[b];
    let into = bm.inward(end);
    let across = into - a * into.dot(&a);
    if across.norm() < 1e-6 {
        // In line with it: nothing to cut against.
        return;
    }
    let n = across.normalize();
    let k = into.dot(&n);
    let b_axes = bm.axes(end);
    // The through members' side facing this one, and their far side.
    let near = reach(section, through.axes, &n);
    let far = reach(section, through.axes, &-n);
    let mine = reach(section, b_axes, &n);
    let plan = &mut plans[b][end];
    if coped {
        plan.run_on = plan.run_on.max(((mine + far) / k).max(0.0) + margin);
        for &t in &through.members {
            plan.copes.push((t, v, into));
        }
        plan.cut = Some(EndCut::Coped);
    } else {
        plan.run_on = plan.run_on.max(((mine - near) / k).max(0.0) + margin);
        plan.planes.push((v + n * near, n));
        plan.cut = Some(angle_cut(k));
    }
    // The member that ends here runs on to this one's far side: as far along -a as this one's
    // end reaches, its end face sheared across the cut plane at an angle other than square
    // (points v + t·into + q, q in its section, on the plane (p - v)·n = near).
    if let Some((t, t_end)) = through.runs_on {
        let ab = a.dot(&into);
        let w = -near * ab / k + reach(section, b_axes, &(n * (ab / k) - a));
        let plan = &mut plans[t][t_end];
        match plan.planes.iter().position(|(_, m)| (m - a).norm() < 1e-9) {
            Some(i) => {
                // Several cut against it: the furthest of them.
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
