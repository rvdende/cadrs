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

/// A circular arc of the path: its centre, the unit normal it turns about (counter-clockwise)
/// and its radius.
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
    /// Any other curve (a spline, an ellipse) as points along it, within 1e-3 mm.
    points: Vec<P>,
    /// The section's up: the curve's sketch normal, else the world's Z.
    up: V,
    /// Who it is: from the path reference and its place among the reference's curves.
    key: u64,
    /// Its path reference's identity (a member override names it).
    who: String,
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
    /// Turned about its curve this much more than the frame (radians; a member override).
    turn: f64,
}

impl Member {
    /// The section's right and up at an end (0: start, 1: end), square to the member.
    fn axes(&self, end: usize) -> (V, V) {
        self.axes_along(if end == 0 { self.t0 } else { self.t1 })
    }

    /// The section's right and up where the member runs along `t` (turned as it is).
    fn axes_along(&self, t: V) -> (V, V) {
        let (x, y) = axes(t, self.up);
        if self.turn == 0.0 {
            return (x, y);
        }
        // Its section turned counter-clockwise (seen back along it) by `turn`.
        let (c, s) = (self.turn.cos(), self.turn.sin());
        (x * c + y * s, y * c - x * s)
    }

    /// The point `d` into the member from an end, along its curves (its lines and arcs
    /// exactly, a spline's as along its chord), and the way in there.
    fn walk(&self, segs: &[Seg], end: usize, d: f64) -> (P, V) {
        let order: Vec<(usize, bool)> = if end == 0 {
            self.segs.clone()
        } else {
            self.segs.iter().rev().map(|&(i, r)| (i, !r)).collect()
        };
        let mut left = d;
        let mut at = (self.point(end), self.inward(end));
        for (i, rev) in order {
            let s = &segs[i];
            let (p0, t0, p1, t1) = if rev {
                (s.end, -s.t1, s.start, -s.t0)
            } else {
                (s.start, s.t0, s.end, s.t1)
            };
            if left >= s.length {
                left -= s.length;
                at = (p1, t1);
                continue;
            }
            return match s.arc {
                Some(c) if !s.line => {
                    let r0 = p0 - c.center;
                    let sign = c.normal.cross(&r0).dot(&t0).signum();
                    let turn = nalgebra::Rotation3::from_axis_angle(
                        &Unit::new_normalize(c.normal),
                        sign * left / c.radius,
                    );
                    (c.center + turn * r0, turn * t0)
                }
                _ if s.line || s.points.len() < 2 => (p0 + t0 * left, t0),
                _ => {
                    let pts: Vec<P> = if rev {
                        s.points.iter().rev().copied().collect()
                    } else {
                        s.points.clone()
                    };
                    along_points(&pts, left)
                }
            };
        }
        (at.0 + at.1 * left, at.1)
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

/// A member as a message names it: by where it runs ("member from (0, 0, 0) to (600, 0, 0)"),
/// as the dialog lists none by number.
fn member_name(m: &Member) -> String {
    let p = |p: P| {
        let n = |v: f64| {
            let v = (v * 10.0).round() / 10.0;
            if v == 0.0 {
                "0".to_string()
            } else {
                format!("{v}")
            }
        };
        format!("({}, {}, {})", n(p.x), n(p.y), n(p.z))
    };
    if m.closed {
        format!("closed member through {}", p(m.start))
    } else {
        format!("member from {} to {}", p(m.start), p(m.end))
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
    /// How far it stops short of its curve's end (a negative end offset), along the curve.
    short: f64,
    /// Planes it is cut by: a point and the normal of the side kept.
    planes: Vec<(P, V)>,
    /// Members whose outline cuts it (copes), and a point and the way into the member (to keep
    /// the pieces on its side).
    copes: Vec<(usize, P, V)>,
    cut: Option<EndCut>,
    /// The plane its cut lies on (its normal), for the cut list to tell cuts apart.
    cut_normal: Option<V>,
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
            ProfileFamily::Custom if x.sketch.is_none() && x.library.is_some() => {
                let l = x.library.as_ref().expect("checked");
                if l.regions.is_empty() {
                    return Err("The library profile has no closed regions".into());
                }
                (l.regions.clone(), Some(l.name.clone()))
            }
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
        // A whole sketch as its curves one by one, so each member is known by its curve (and
        // keeps its id and names when other curves of the sketch come or go).
        let mut refs: Vec<PathRef> = Vec::new();
        for p in &x.path {
            match p {
                PathRef::Sketch(sk) => match sketch_of(before, *sk) {
                    Some((g, _)) => refs.extend(
                        g.curves
                            .iter()
                            .filter(|(_, c)| !c.construction)
                            .map(|(curve, _)| PathRef::SketchCurve { sketch: *sk, curve }),
                    ),
                    None => missing += 1,
                },
                p => refs.push(*p),
            }
        }
        // (A curve picked and its sketch picked too: once.)
        let mut seen: HashSet<String> = HashSet::new();
        refs.retain(|p| seen.insert(path_identity(p)));
        for p in &refs {
            let curves = match self.path_curves(before, state, std::slice::from_ref(p)) {
                Ok(c) => c,
                Err(_) => {
                    missing += 1;
                    continue;
                }
            };
            // (A spline's spans by their place along it.)
            let who = path_identity(p);
            for (k, c) in curves.into_iter().enumerate() {
                let key = naming::stable_hash(format!("{who}/{k}").as_bytes());
                let mut seg = self.frame_seg(c, key)?;
                seg.who = who.clone();
                segs.push(seg);
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
        orient(&mut members, &segs, tol);
        // Members turned by their overrides (by any of their curves).
        let mut used_member = vec![false; x.member_overrides.len()];
        for m in &mut members {
            for (j, o) in x.member_overrides.iter().enumerate() {
                let who = path_identity(&o.curve);
                if m.segs.iter().any(|(i, _)| segs[*i].who == who) {
                    m.turn = o.angle.to_radians();
                    used_member[j] = true;
                }
            }
        }

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
        let margin = 0.02 * r_section + 1e-3;
        let Corners {
            mut plans,
            used_override,
            override_ends,
            free_ends,
            gusset_at,
            notes: corner_notes,
        } = plan_corners(
            x,
            &resolved,
            &segs,
            &members,
            &section,
            margin,
            tol,
            1e-4 * (1.0 + size),
        )?;

        let mut notes = corner_notes;
        // Free ends run on or stop short by the end offset.
        // (A cap set into the end stops the member short by its thickness.)
        let capped = x.end_caps && section.iter().any(|r| !r.holes.is_empty());
        let off = x.end_offset
            - if capped && x.cap_inward {
                x.cap_thickness
            } else {
                0.0
            };
        if off.abs() > 1e-9 {
            for &(m, e) in &free_ends {
                let mem = &members[m];
                let ends = free_ends.iter().filter(|f| f.0 == m).count() as f64;
                if off < 0.0 && -off * ends >= mem.length - tol {
                    return Err(format!(
                        "The end offset leaves nothing of the {}",
                        member_name(mem)
                    ));
                }
                let plan = &mut plans[m][e];
                if off > 0.0 {
                    plan.run_on = plan.run_on.max(off);
                } else {
                    // Along the member's curves: a bent end's cut square to it where it stops.
                    let (at, inward) = mem.walk(&segs, e, -off);
                    plan.planes.push((at, inward));
                    plan.short = -off;
                }
            }
        }

        // A free end inside a part the members are trimmed by runs on through it, for the trim
        // to cut: a tube ending on another's axis is saddled to it (its square end there would
        // touch the other's surface at two points only, and be cut to nothing sound). If the
        // trim leaves the run-on, it is cut off again below.
        let mut reached: Vec<End> = Vec::new();
        if !x.trim.is_empty() {
            let boxes: Vec<_> = x
                .trim
                .iter()
                .filter_map(|p| state.part(*p).and_then(|p| p.body))
                .filter_map(|b| self.kernel.bounding_box(b).ok())
                .collect();
            for &(m, e) in &free_ends {
                let at = members[m].point(e);
                let inside = boxes
                    .iter()
                    .any(|b| (0..3).all(|k| b.min[k] - tol <= at[k] && at[k] <= b.max[k] + tol));
                if inside && plans[m][e].run_on == 0.0 && plans[m][e].short == 0.0 {
                    plans[m][e].run_on = 2.0 * r_section;
                    reached.push((m, e));
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
                    return Err(format!("The {} failed: {e}", member_name(&members[i])));
                }
            }
        }
        // Copes, by the outlines of the members they butt against (cut as those are).
        let mut tools: HashMap<usize, (BodyId, BodyNames)> = HashMap::new();
        let mut coping = (|| -> Result<(), String> {
            for i in 0..members.len() {
                for plan in plans[i].clone() {
                    for (by, at, way) in plan.copes {
                        if let std::collections::hash_map::Entry::Vacant(slot) = tools.entry(by) {
                            let m = &members[by];
                            let base = base_of(m.key) + 0x00e0_0000;
                            let t = self
                                .frame_cut_member(state, op, m, &segs, &envelope, &plans[by], base)
                                .map_err(|e| format!("The {} failed: {e}", member_name(m)))?;
                            slot.insert(t);
                        }
                        let tool = &tools[&by];
                        let tool = (tool.0, tool.1.clone());
                        let b = (bodies[i].0, bodies[i].1.clone());
                        // (`frame_cope` releases `b` either way.)
                        match self.frame_cope(op, b, (tool.0, &tool.1), at, way, r_section) {
                            Ok(c) => bodies[i] = c,
                            Err(e) => {
                                bodies[i].0 = BodyId(u64::MAX);
                                return Err(format!(
                                    "The {} failed: {e}",
                                    member_name(&members[i])
                                ));
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
        // Trims by other parts: each member less what overlaps them, its largest piece kept (or
        // the rest, flipped). An end within reach of a part it was trimmed by is a trimmed end.
        let mut trimmed = vec![[false; 2]; members.len()];
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
                // How far the member reaches past each end, before the trim.
                let past = |k: &Self, body: BodyId| {
                    let bb = k.kernel.bounding_box(body).ok();
                    [0, 1].map(|e| {
                        let m = &members[i];
                        bb.map_or(0.0, |bb| bb.max_along(m.point(e), &-m.inward(e)))
                    })
                };
                let was = past(self, bodies[i].0);
                let b = (bodies[i].0, bodies[i].1.clone());
                // A T-slot member's slots would keep tongues of the trimmed member: cut it at the
                // tool's face (its box's, square to the member's end) too.
                let slotted = state
                    .frames
                    .iter()
                    .any(|m| m.part == *p && m.family == Some(ProfileFamily::TSlot) && !m.plate);
                match self.frame_trim(op, b, tool, state, x.trim_flip) {
                    Ok((mut t, split)) => {
                        if split {
                            notes.push(format!(
                                "A trim split the {} in two: its larger part was kept",
                                member_name(&members[i])
                            ));
                        }
                        // An end it now falls short of was trimmed.
                        let now = past(self, t.0);
                        for e in 0..2 {
                            trimmed[i][e] |= now[e] < was[e] - 1e-6 * (1.0 + size);
                            if !(trimmed[i][e] && slotted) {
                                continue;
                            }
                            let m = &members[i];
                            let (pe, out) = (m.point(e), -m.inward(e));
                            // Where the member first meets the tool, coming out along this end
                            // (rays from its section's outline): a face square to it there,
                            // else (a slanted tool) the trim as it is.
                            let Some(depth) = self.first_contact(tool, m, e, &section, r_section)
                            else {
                                continue;
                            };
                            let face = pe + out * depth;
                            let bound = (!m.straight).then(|| plans[i][e].run_on + 4.0 * r_section);
                            match self.frame_cut(
                                op,
                                t,
                                face,
                                -out,
                                base_of(m.key) + 0xd0 + e as u64,
                                bound,
                            ) {
                                Ok(c) => t = c,
                                Err(err) => {
                                    let rest: Vec<(BodyId, BodyNames)> = bodies
                                        .iter()
                                        .enumerate()
                                        .filter(|(j, _)| *j != i)
                                        .map(|(_, b)| b.clone())
                                        .collect();
                                    release(self, &rest);
                                    return Err(format!(
                                        "The {} failed: {err}",
                                        member_name(&members[i])
                                    ));
                                }
                            }
                        }
                        bodies[i] = t;
                    }
                    Err(e) => {
                        let rest: Vec<(BodyId, BodyNames)> = bodies
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, b)| b.clone())
                            .collect();
                        release(self, &rest);
                        return Err(format!("The {} failed: {e}", member_name(&members[i])));
                    }
                }
            }
        }

        // The ends run on for the trims: trimmed, or (the run-on still there) cut square where
        // the curve ends, as an end the trims leave alone is.
        for &(i, e) in &reached {
            let m = &members[i];
            let (pe, out) = (m.point(e), -m.inward(e));
            let past = self
                .kernel
                .bounding_box(bodies[i].0)
                .map_or(0.0, |bb| bb.max_along(pe, &out));
            // (A cut of curved faces has a loose box: gone is less than half the run-on left.)
            if past <= 0.5 * plans[i][e].run_on {
                trimmed[i][e] = true;
                continue;
            }
            trimmed[i][e] = false;
            let b = (bodies[i].0, bodies[i].1.clone());
            let bound = (!m.straight).then(|| plans[i][e].run_on + 4.0 * r_section);
            match self.frame_cut(op, b, pe, -out, base_of(m.key) + 0xd8 + e as u64, bound) {
                Ok(c) => bodies[i] = c,
                Err(err) => {
                    let rest: Vec<(BodyId, BodyNames)> = bodies
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, b)| b.clone())
                        .collect();
                    release(self, &rest);
                    return Err(format!("The {} failed: {err}", member_name(m)));
                }
            }
        }

        // Trims at planes: each member crossing one split there, its largest piece kept (or
        // the rest, flipped).
        let planes: Vec<(P, V)> = x
            .trim_planes
            .iter()
            .filter_map(|p| super::advanced::plane_of(state, p))
            .map(|f| (P::from(f.origin), V::from(f.normal()).normalize()))
            .collect();
        if planes.len() < x.trim_planes.len() {
            notes.push("A trim plane no longer exists".into());
        }
        for (k, &(o, n)) in planes.iter().enumerate() {
            for i in 0..bodies.len() {
                let m = &members[i];
                let was = [0, 1].map(|e| (m.point(e) - o).dot(&n));
                let b = (bodies[i].0, bodies[i].1.clone());
                let source = base_of(m.key) + 0xc0 + k as u64;
                match self.frame_trim_plane(op, b, (o, n), source, x.trim_flip) {
                    Ok((t, cut)) => {
                        bodies[i] = t;
                        if cut.split {
                            notes.push(format!(
                                "A trim split the {} in two: its larger part was kept",
                                member_name(m)
                            ));
                        }
                        // The ends on the side cut off were trimmed.
                        for e in 0..2 {
                            trimmed[i][e] |= cut.happened && was[e] * cut.kept_side < 0.0;
                        }
                    }
                    Err(e) => {
                        let rest: Vec<(BodyId, BodyNames)> = bodies
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, b)| b.clone())
                            .collect();
                        release(self, &rest);
                        return Err(format!("The {} failed: {e}", member_name(m)));
                    }
                }
            }
        }

        // End caps and gussets, parts of their own after the members.
        let mut plates: Vec<((BodyId, BodyNames), String, u64, frame::PlateKind)> = Vec::new();
        let mut plate_made = (|| -> Result<(), String> {
            if capped {
                let label = frame::cap_label(x.cap_thickness, &profile_label);
                let kind = frame::PlateKind::Cap {
                    thickness: x.cap_thickness,
                };
                // (Not at an end trimmed back: the cap would be left where it was.)
                for &(m, end) in free_ends.iter().filter(|(m, e)| !trimmed[*m][*e]) {
                    let mb = &members[m];
                    let key = naming::stable_hash(format!("cap/{}/{end}", mb.key).as_bytes());
                    let b = self.frame_cap_at(
                        state,
                        op,
                        (mb, &segs),
                        end,
                        &section,
                        x.cap_thickness,
                        off,
                        base_of(key),
                    )?;
                    plates.push((b, label.clone(), key, kind));
                }
            }
            if !gusset_at.is_empty() {
                let kind = frame::PlateKind::Gusset {
                    size: x.gusset_size,
                    thickness: x.gusset_thickness,
                };
                let label = frame::gusset_label(x.gusset_size, x.gusset_thickness);
                for g in &gusset_at {
                    // (Two members can meet at both their ends: the ends are in the key.)
                    let key = naming::stable_hash(g.key.as_bytes());
                    let made = self.frame_gusset(
                        state,
                        op,
                        &section,
                        g.at,
                        g.legs,
                        g.up,
                        (x.gusset_size, x.gusset_thickness, x.gusset_chamfer),
                        x.gusset_position,
                        base_of(key),
                    )?;
                    plates.push((made, label.clone(), key, kind));
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
        // (A custom section's name is its sketch's.)
        let size = match &custom_name {
            Some(n) => n.clone(),
            None => x.size.clone(),
        };
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
                let ends = [0, 1].map(|e| (plans[i][e].run_on, plans[i][e].short));
                self.bent_length(body, m, r_section, ends)
                    .unwrap_or(m.length)
            };
            infos.push(MemberInfo {
                feature: id,
                part,
                profile: profile_label.clone(),
                length,
                ends: [0, 1].map(|e| {
                    if m.closed {
                        EndCut::Square
                    } else if trimmed[i][e] {
                        EndCut::Trimmed
                    } else {
                        plans[i][e].cut.unwrap_or(EndCut::Square)
                    }
                }),
                curved: !m.straight,
                volume,
                plate: false,
                end_normals: [0, 1].map(|e| match plans[i][e].cut_normal {
                    Some(n) if !m.closed && !trimmed[i][e] => {
                        let (ax, ay) = m.axes(e);
                        [n.dot(&ax), n.dot(&ay)]
                    }
                    _ => [0.0; 2],
                }),
                symmetry: frame::symmetry(x),
                family: Some(x.family),
                size: size.clone(),
                plate_kind: None,
                span: [0, 1].map(|e| {
                    let p = m.point(e);
                    [p.x, p.y, p.z]
                }),
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
        for ((body, names), label, key, kind) in plates {
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
                end_normals: [[0.0; 2]; 2],
                symmetry: frame::Symmetry::None,
                family: None,
                size: String::new(),
                plate_kind: Some(kind),
                span: [[0.0; 3]; 2],
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
        let mut corners = (*next.frame_corners).clone();
        corners.retain(|c| c.feature != id);
        corners.push(frame::OverrideCorners {
            feature: id,
            ends: override_ends,
        });
        next.frame_corners = Arc::new(corners);
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
        if used_member.iter().any(|u| !u) {
            warnings.push("A member override's curve isn't in the path".to_string());
        }
        let gone = x.trim.iter().filter(|p| state.part(**p).is_none()).count();
        if gone > 0 {
            warnings.push("A part to trim by no longer exists".to_string());
        }
        notes.dedup();
        warnings.extend(notes);
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
        let edges = match self.kernel.edges(b) {
            Ok(e) => e,
            Err(e) => {
                self.kernel.release(b);
                return Err(e.to_string());
            }
        };
        let Some(e) = edges.first() else {
            self.kernel.release(b);
            return Err("A frame curve has no length".into());
        };
        let points = if e.curve == CurveKind::Line || e.circle.is_some() {
            Vec::new()
        } else {
            let mesh = self.kernel.tessellate(
                b,
                Tessellation {
                    deflection: 1e-3,
                    angle: 2f64.to_radians(),
                },
            );
            let mut pts: Vec<P> = mesh
                .ok()
                .and_then(|m| m.edges.into_iter().next())
                .map(|(_, p)| p)
                .unwrap_or_default();
            // The way the curve runs.
            if pts
                .first()
                .is_some_and(|p| (p - e.end).norm() < (p - e.start).norm())
            {
                pts.reverse();
            }
            pts
        };
        self.kernel.release(b);
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
            points,
            up,
            key,
            who: String::new(),
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
                // A bent member's cut only near its end (a whole plane could cross it
                // elsewhere too): within its run-on and its section's size.
                let bound =
                    (!m.straight).then(|| p.run_on + p.short + 4.0 * frame::radius(section));
                b = self.frame_cut(op, b, o, n, cut_source, bound)?;
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

    /// A bent member's stock length: its path, and at each end what of its body reaches past
    /// the path's end along the member (a mitre's long point, a run-on), or less what it stops
    /// short (square to its curve there).
    fn bent_length(
        &self,
        body: BodyId,
        m: &Member,
        r_section: f64,
        ends: [(f64, f64); 2],
    ) -> Option<f64> {
        // A ring has no ends: its path.
        if m.closed {
            return Some(m.length);
        }
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
        let mut length = m.length;
        for (end, &(run, short)) in ends.iter().enumerate() {
            if short > 0.0 {
                length -= short;
                continue;
            }
            let (p, out) = (m.point(end), -m.inward(end));
            // The body's points near this end (within its section's size and its run-on: a
            // sharp mitre's long point is far out), and within the section's reach of the line
            // it runs out along (not the other leg of a tight hairpin).
            let window = (4.0 * r_section).max(run + 2.0 * r_section);
            let near = mesh
                .positions
                .iter()
                .filter(|q| {
                    let d = *q - p;
                    d.norm() < window && (d - out * d.dot(&out)).norm() <= r_section * 1.001
                })
                .map(|q| (q - p).dot(&out))
                .fold(f64::NEG_INFINITY, f64::max);
            if near.is_finite() {
                length += near;
            }
        }
        Some(length)
    }

    /// An end cap: the section's outer outline (no holes), `thickness` thick, against the end
    /// `end` of the member, outside it.
    #[allow(clippy::too_many_arguments)]
    fn frame_cap_at(
        &mut self,
        state: &State,
        op: cadrs_kernel::OpId,
        (m, segs): (&Member, &[Seg]),
        end: usize,
        section: &[Region],
        thickness: f64,
        offset: f64,
        base: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        // Stopped short: where the member stops along its curves.
        let (at, inward) = if offset < 0.0 {
            m.walk(segs, end, -offset)
        } else {
            (m.point(end) - m.inward(end) * offset, m.inward(end))
        };
        let out = -inward;
        let (x, _) = m.axes_along(if end == 0 { inward } else { out });
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
            origin: at,
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

    /// A gusset in the angle between two legs from `v` (a member end, or one way along a member
    /// running through): a triangle from where their inner sides meet, its legs `size` along
    /// them (its inner corner cut back `chamfer`), `thickness` thick across the corner's plane
    /// in the middle of the section or flush with its top or bottom (`position`; up leaning by
    /// `up`).
    #[allow(clippy::too_many_arguments)]
    fn frame_gusset(
        &mut self,
        state: &State,
        op: cadrs_kernel::OpId,
        section: &[Region],
        v: P,
        ((a, a_axes), (b, b_axes)): (Leg, Leg),
        up: V,
        (size, thickness, chamfer): (f64, f64, f64),
        position: frame::GussetPosition,
        base: u64,
    ) -> Result<(BodyId, BodyNames), String> {
        // Across the corner's plane, the way the section's up is (for Top and Bottom).
        let n = a.cross(&b).normalize();
        // (Where the corner's plane holds the up: upwards, else along +Y, else +X.)
        let lean = [n.dot(&up), n.z, n.y, n.x]
            .into_iter()
            .find(|c| c.abs() > 1e-6)
            .unwrap_or(1.0);
        let n = if lean < 0.0 { -n } else { n };
        // Each member's inner side: across it towards the other, as far as the section reaches.
        let na = (b - a * a.dot(&b)).normalize();
        let nb = (a - b * b.dot(&a)).normalize();
        let ha = reach(section, a_axes, &na);
        let hb = reach(section, b_axes, &nb);
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
        // Across the corner's plane: in the middle of the section, or flush with its top or
        // bottom.
        let (top, bottom) = (reach(section, a_axes, &n), -reach(section, a_axes, &-n));
        let low = match position {
            frame::GussetPosition::Center => (top + bottom) / 2.0 - thickness / 2.0,
            frame::GussetPosition::Top => top - thickness,
            frame::GussetPosition::Bottom => bottom,
        };
        let p0 = v + a * s + w * p1 + n * low;
        let plane = Plane {
            origin: p0,
            x_dir: Unit::new_normalize(u),
            normal: Unit::new_normalize(n),
        };
        let pt = |x: f64, y: f64| nalgebra::Point2::new(x, y);
        // A triangle, its inner corner cut back `chamfer` along each leg; counter-clockwise.
        let mut corners = vec![pt(chamfer, 0.0), pt(size, 0.0), pt(size * bu, size * bw)];
        if chamfer > 0.0 {
            corners.push(pt(chamfer * bu, chamfer * bw));
        }
        if bw < 0.0 {
            corners.reverse();
        }
        let k = corners.len();
        let curves = (0..k)
            .map(|i| cadrs_kernel::Curve2::Line {
                a: corners[i],
                b: corners[(i + 1) % k],
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
    /// custom section of several regions). With `bound`, only within that distance of `o`: a
    /// box that far each way on the other side subtracted.
    fn frame_cut(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        o: P,
        n: V,
        source: u64,
        bound: Option<f64>,
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
        if let Some(d) = bound {
            return self.frame_cut_near(op, (body, names), o, n, source, d);
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

    /// [`Self::frame_cut`] within `d` of `o`: a box `2d` square and `d` deep on the side away
    /// from `n`, its face on the plane, subtracted.
    fn frame_cut_near(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        o: P,
        n: V,
        source: u64,
        d: f64,
    ) -> Result<(BodyId, BodyNames), String> {
        let pt = |x: f64, y: f64| nalgebra::Point2::new(x, y);
        let c = [pt(-d, -d), pt(d, -d), pt(d, d), pt(-d, d)];
        let curves = (0..4)
            .map(|i| cadrs_kernel::Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: Some(source * 8 + i as u64),
            })
            .collect();
        let profile = Profile::new(
            Plane {
                origin: o - n * d,
                x_dir: Unit::new_normalize(square_to(&n)),
                normal: Unit::new_normalize(n),
            },
            vec![Region {
                outer: Loop { curves },
                holes: Vec::new(),
                source: Some(source),
            }],
        );
        let made = self.kernel.extrude(&profile, Extent::Blind(d));
        let tool = match made {
            Ok(r) => r.bodies[0],
            Err(e) => {
                self.kernel.release(body);
                return Err(format!("a corner cut failed: {e}"));
            }
        };
        let r = self.kernel.boolean(BoolOp::Subtract, body, &[tool]);
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                self.kernel.release(tool);
                self.kernel.release(body);
                return Err(format!("a corner cut failed: {e}"));
            }
        };
        let cut = r.bodies[0];
        let none = BodyNames::default();
        let cut_names = naming::name_body(
            &self.kernel,
            cut,
            op,
            &r.history,
            &[(body, &names), (tool, &none)],
        )
        .map_err(|e| e.to_string());
        self.kernel.release(tool);
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
        self.keep_pieces(op, pieces, |_, _| true)?
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

    /// How far past the end `end` of member `m` it first meets `tool`, along it: rays along
    /// the member from its section's outline, well back from the end. `None` if they meet the
    /// tool at depths further apart than a hundredth of the section (a slanted face: no square
    /// cut fits) or not at all.
    fn first_contact(
        &self,
        tool: BodyId,
        m: &Member,
        end: usize,
        section: &[Region],
        r_section: f64,
    ) -> Option<f64> {
        let (pe, out) = (m.point(end), -m.inward(end));
        let (ax, ay) = m.axes(end);
        let back = (m.length / 2.0).min(4.0 * r_section);
        let mut hits = Vec::new();
        for i in 0..24 {
            let a = std::f64::consts::TAU * i as f64 / 24.0;
            let d = [a.cos(), a.sin()];
            // Just inside the outline's furthest point that way.
            let r = frame::support(section, d) * 0.98;
            let q = pe + (ax * d[0] + ay * d[1]) * r - out * back;
            let first = self
                .kernel
                .ray_hits(tool, q, out)
                .ok()?
                .into_iter()
                .map(|h| h.t)
                .filter(|t| *t > 0.0)
                .fold(f64::INFINITY, f64::min);
            if first.is_finite() {
                hits.push(first - back);
            }
        }
        let lo = hits.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = hits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // The lips: most rays meet the tool's face, a few (into its slots) further in.
        let at_face = hits
            .iter()
            .filter(|h| (**h - lo) < 1e-3 * r_section)
            .count();
        (lo.is_finite() && (hi - lo < 1e-2 * r_section || at_face * 2 >= hits.len())).then_some(lo)
    }

    /// A member trimmed at a plane: split there, the largest piece kept (`other_side`: the
    /// rest). A member the plane misses is kept whole.
    fn frame_trim_plane(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        (o, n): (P, V),
        source: u64,
        other_side: bool,
    ) -> Result<((BodyId, BodyNames), PlaneTrim), String> {
        let missed = PlaneTrim {
            happened: false,
            split: false,
            kept_side: 0.0,
        };
        let Ok(bb) = self.kernel.bounding_box(body) else {
            return Ok(((body, names), missed));
        };
        if bb.max_along(o, &n) <= 1e-9 || bb.max_along(o, &-n) <= 1e-9 {
            return Ok(((body, names), missed));
        }
        let plane = Plane {
            origin: o,
            x_dir: Unit::new_normalize(square_to(&n)),
            normal: Unit::new_normalize(n),
        };
        let r = match self.kernel.split(body, &SplitTool::Plane(plane), source) {
            Ok(r) if !r.bodies.is_empty() => r,
            // Its box crosses the plane, it doesn't.
            Ok(_) => return Ok(((body, names), missed)),
            Err(e) => {
                self.kernel.release(body);
                return Err(format!("a trim failed: {e}"));
            }
        };
        let cut = r.bodies[0];
        let cut_names = naming::name_body(&self.kernel, cut, op, &r.history, &[(body, &names)])
            .map_err(|e| e.to_string());
        let cut_names = match cut_names {
            Ok(c) => c,
            Err(e) => {
                self.kernel.release(cut);
                self.kernel.release(body);
                return Err(e);
            }
        };
        let pieces = match self.split(cut, op, &[(cut, &cut_names)]) {
            Ok(p) => p,
            Err(e) => {
                self.kernel.release(cut);
                self.kernel.release(body);
                return Err(e);
            }
        };
        self.kernel.release(cut);
        let side = |k: &Self, b: BodyId| {
            k.kernel
                .mass_properties(b)
                .map(|m| ((m.center_of_mass - o).dot(&n), m.volume))
                .unwrap_or((0.0, 0.0))
        };
        let measured: Vec<(f64, f64)> = pieces.iter().map(|p| side(self, p.body)).collect();
        // Through the plane only at a touch: kept whole.
        let crossing = measured.iter().any(|m| m.0 > 0.0) && measured.iter().any(|m| m.0 < 0.0);
        if !crossing {
            for p in pieces {
                self.kernel.release(p.body);
            }
            return Ok(((body, names), missed));
        }
        self.kernel.release(body);
        let largest = measured.iter().map(|m| m.1).fold(f64::MIN, f64::max);
        let big = measured
            .iter()
            .find(|m| m.1 >= largest * (1.0 - 1e-9))
            .map_or(1.0, |m| m.0.signum());
        let several = measured.iter().filter(|m| m.1 > largest * 1e-3).count() > 2
            || measured
                .iter()
                .filter(|m| m.1 > largest * 1e-3 && m.0 * big > 0.0)
                .count()
                > 1;
        let kept_side = if other_side { -big } else { big };
        let kept = self
            .keep_pieces(op, pieces, |k, b| {
                let (s, v) = side(k, b);
                if other_side {
                    !(s * big > 0.0 && v >= largest * (1.0 - 1e-9))
                } else {
                    s * big > 0.0 && v >= largest * (1.0 - 1e-9)
                }
            })?
            .ok_or_else(|| "a trim leaves nothing of it".to_string())?;
        Ok((
            kept,
            PlaneTrim {
                happened: true,
                split: several,
                kept_side,
            },
        ))
    }

    /// A member trimmed by a part: the part subtracted, the largest piece kept (`other_side`: the
    /// rest).
    fn frame_trim(
        &mut self,
        op: cadrs_kernel::OpId,
        (body, names): (BodyId, BodyNames),
        tool: BodyId,
        state: &State,
        other_side: bool,
    ) -> Result<((BodyId, BodyNames), bool), String> {
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
        // (Pieces of more than a sliver: a member cut through, not just its end.)
        let several = pieces.iter().filter(|p| p.volume > largest * 1e-3).count() > 1;
        self.keep_pieces(op, pieces, |k, b| {
            let is_largest = k
                .kernel
                .mass_properties(b)
                .is_ok_and(|m| m.volume >= largest * (1.0 - 1e-9));
            // The other side: all but the largest (a member the part only nicks keeps all).
            if other_side && several {
                !is_largest
            } else {
                is_largest
            }
        })?
        .ok_or_else(|| "a trim leaves nothing of it".to_string())
        .map(|b| (b, several))
    }
}

/// What a trim at a plane did: whether it cut the member, cut it into more than two pieces
/// (several kept or dropped), and the side kept (+1 the plane's normal's, -1 the other).
struct PlaneTrim {
    happened: bool,
    split: bool,
    kept_side: f64,
}

/// One leg of a gusset: the way it runs from the corner and the section's axes there.
type Leg = (V, (V, V));

/// A gusset to make: where, its two legs, the up it leans by, and who it is (for its id).
struct Gusset {
    at: P,
    legs: (Leg, Leg),
    up: V,
    key: String,
}

/// The corners' plans: each member end's run-on, cut planes and copes, which overrides were
/// used, the free ends (for end caps) and the two-member corners to put gussets in.
struct Corners {
    plans: Vec<[EndPlan; 2]>,
    used_override: Vec<bool>,
    /// How many member ends meet at each override (a member running through counts two).
    override_ends: Vec<u8>,
    free_ends: Vec<End>,
    gusset_at: Vec<Gusset>,
    /// What didn't go as asked (a mitre made a butt).
    notes: Vec<String>,
}

/// Plans every corner of the frame: overrides by where they are; ends on a member's span cut
/// against it; two ends mitred, or butted with the longer running on (of two as long, the one
/// along the first member's direction, so a square frame's opposite sides pair up), flip
/// swapping them; at three or more, two in line run through and the others are cut against
/// them, else the main pair (the widest angle; an override's turn picks the next pair) is
/// joined as two are and the others are cut against both and split between themselves.
#[allow(clippy::too_many_arguments)]
fn plan_corners(
    x: &FrameFeature,
    resolved: &[Option<P>],
    segs: &[Seg],
    members: &[Member],
    section: &[Region],
    margin: f64,
    tol: f64,
    override_tol: f64,
) -> Result<Corners, String> {
    let mut out = Corners {
        plans: vec![Default::default(); members.len()],
        used_override: vec![false; x.overrides.len()],
        override_ends: vec![0; x.overrides.len()],
        free_ends: Vec::new(),
        gusset_at: Vec::new(),
        notes: Vec::new(),
    };
    let plans = &mut out.plans;
    // The direction ties between members as long go by.
    let reference = members.first().map_or(V::x(), |m| m.t0);
    let outline = frame::outline(section);
    let size = frame::radius(section);
    for (v, ends) in &corners_of(members, tol) {
        let ov = resolved
            .iter()
            .position(|r| r.is_some_and(|r| (r - v).norm() < override_tol));
        if let Some(i) = ov {
            out.used_override[i] = true;
        }
        let o = ov.map(|i| &x.overrides[i]);
        let kind = o.map_or(x.corner, |o| o.corner);
        let flip = o.map_or(x.flip, |o| o.flip);
        // (The frame's flip swaps runners only; which pair is joined at a corner of three or
        // more is an override's turn.)
        let turn = o.map_or(0, |o| usize::from(o.turn));
        let gusset = o.and_then(|o| o.gusset).unwrap_or(x.gussets);
        // Members running through the point (anywhere along them but their ends): the ends
        // here are cut against each.
        let crossing: Vec<(usize, V, (V, V))> = members
            .iter()
            .enumerate()
            .filter_map(|(i, m)| passes(segs, m, v, tol * 10.0).map(|(a, axes)| (i, a, axes)))
            .collect();
        let passing = crossing.first().copied();
        if let Some(i) = ov {
            let n = ends.len() + 2 * crossing.len();
            out.override_ends[i] = n.min(u8::MAX.into()) as u8;
        }
        let leg = |e: &End| (members[e.0].inward(e.1), members[e.0].axes(e.1));
        // A gusset between two ends (not in line).
        let pair = |p: &End, q: &End| {
            let (a, b) = (leg(p), leg(q));
            (a.0.dot(&b.0).abs() < TANGENT.cos()).then(|| Gusset {
                at: *v,
                legs: (a, b),
                up: members[p.0].up,
                key: format!(
                    "gusset/{}/{}/{}/{}",
                    members[p.0].key, p.1, members[q.0].key, q.1
                ),
            })
        };
        // Gussets on both sides of an end butting into a run through the point.
        let tee = |e: End, (run, way, axes): (u64, V, (V, V))| {
            [1.0, -1.0].into_iter().filter_map(move |side| {
                let a = leg(&e);
                let b = (way * side, axes);
                (a.0.dot(&b.0).abs() < TANGENT.cos()).then(|| Gusset {
                    at: *v,
                    legs: (a, b),
                    up: members[e.0].up,
                    key: format!(
                        "gusset/tee/{}/{}/{run}/{}",
                        members[e.0].key,
                        e.1,
                        side < 0.0
                    ),
                })
            })
        };
        if gusset {
            match (&passing, ends.as_slice()) {
                (None, [e0, e1]) => out.gusset_at.extend(pair(e0, e1)),
                (Some((i, a, axes)), ends) => {
                    for e in ends {
                        out.gusset_at.extend(tee(*e, (members[*i].key, *a, *axes)));
                    }
                }
                _ => {}
            }
        }
        if let (None, [e]) = (&passing, ends.as_slice()) {
            out.free_ends.push(*e);
        }
        if kind == CornerType::None {
            continue;
        }
        let coped = kind == CornerType::CopedButt;
        if !crossing.is_empty() {
            for &(i, a, axes) in &crossing {
                let through = Through {
                    members: vec![i],
                    a,
                    axes,
                    runs_on: None,
                };
                for &e in ends {
                    plan_butt(section, &through, members, e, *v, coped, margin, plans);
                }
            }
            split_neighbours(section, members, ends, *v, margin, plans);
            continue;
        }
        match ends.as_slice() {
            [] | [_] => {}
            [e0, e1] => {
                if join_two(
                    (*e0, *e1),
                    kind,
                    flip,
                    (section, members, reference, tol, margin, (&outline, size)),
                    *v,
                    plans,
                )? {
                    out.notes.push(MITRE_TO_BUTT.into());
                }
            }
            _ => {
                // Two in line make one run through the point (a crossbar's halves); else the
                // main pair, the two at the widest angle (of pairs as wide, the one lying
                // flattest across the first member's up; an override's turn picks the next),
                // is joined as a corner of two. The others are cut against both, and against
                // each other along their bisectors.
                let inline = ends.iter().enumerate().find_map(|(i, p)| {
                    ends[i + 1..].iter().find_map(|q| {
                        let (a, b) = (members[p.0].inward(p.1), members[q.0].inward(q.1));
                        (a.dot(&b) < -TANGENT.cos()).then_some((*p, *q))
                    })
                });
                let main: Vec<End> = match inline {
                    Some((p, q)) => vec![p, q],
                    None => {
                        let up = members[0].up;
                        let mut pairs: Vec<(End, End, f64, f64)> = Vec::new();
                        for (i, p) in ends.iter().enumerate() {
                            for q in &ends[i + 1..] {
                                let (a, b) = (members[p.0].inward(p.1), members[q.0].inward(q.1));
                                let flat = a.cross(&b).normalize().dot(&up).abs();
                                pairs.push((*p, *q, a.dot(&b), flat));
                            }
                        }
                        pairs.sort_by(|x, y| {
                            // (As whole numbers: -0 and 0 alike.)
                            let round = |c: f64| (c * 1e6).round() as i64;
                            round(x.2)
                                .cmp(&round(y.2))
                                .then(round(y.3).cmp(&round(x.3)))
                        });
                        let (p, q, ..) = pairs[turn % pairs.len()];
                        let butted = join_two(
                            (p, q),
                            kind,
                            flip,
                            (section, members, reference, tol, margin, (&outline, size)),
                            *v,
                            plans,
                        )?;
                        if butted {
                            out.notes.push(MITRE_TO_BUTT.into());
                        }
                        if gusset {
                            out.gusset_at.extend(pair(&p, &q));
                        }
                        vec![p, q]
                    }
                };
                let others: Vec<End> = ends.iter().copied().filter(|e| !main.contains(e)).collect();
                if let (true, Some((p, _))) = (gusset, inline) {
                    let (way, axes) = leg(&p);
                    for e in &others {
                        out.gusset_at.extend(tee(*e, (members[p.0].key, way, axes)));
                    }
                }
                for &m in &main {
                    let mm = &members[m.0];
                    // (In line, either half's way and axes serve.)
                    let through = Through {
                        members: if inline.is_some() {
                            main.iter().map(|e| e.0).collect()
                        } else {
                            vec![m.0]
                        },
                        a: mm.inward(m.1),
                        axes: mm.axes(m.1),
                        runs_on: None,
                    };
                    for &e in &others {
                        plan_butt(section, &through, members, e, *v, coped, margin, plans);
                    }
                    if inline.is_some() {
                        break;
                    }
                }
                split_neighbours(section, members, &others, *v, margin, plans);
            }
        }
    }
    Ok(out)
}

/// Joins two member ends at `v` as the corner `kind` says: mitred, or the one that runs on
/// (the longer; of two as long, the one along `reference`, else along X, else Y; `flip` swaps
/// them) cut across by the other, butted or coped. In line, nothing (two pieces end to end).
fn join_two(
    (e0, e1): (End, End),
    kind: CornerType,
    flip: bool,
    (section, members, reference, tol, margin, (outline, size)): (
        &[Region],
        &[Member],
        V,
        f64,
        f64,
        (&[Vec<[f64; 2]>], f64),
    ),
    v: P,
    plans: &mut [[EndPlan; 2]],
) -> Result<bool, String> {
    let (a, b) = (members[e0.0].inward(e0.1), members[e1.0].inward(e1.1));
    if a.dot(&b) < -TANGENT.cos() {
        return Ok(false);
    }
    if a.dot(&b) > TANGENT.cos() {
        return Err("Two members fold back on each other at a corner".into());
    }
    // A mitre's halves are each other's mirror image across it: where they aren't (an
    // unsymmetric section on members both starting or both ending there, members turned
    // differently), their faces wouldn't meet: butted instead.
    let mut kind = kind;
    let mut butted = false;
    if kind == CornerType::Miter
        && !mitre_meets(
            outline,
            (&members[e0.0], e0.1),
            (&members[e1.0], e1.1),
            size,
        )
    {
        kind = CornerType::Butt;
        butted = true;
    }
    if kind == CornerType::Miter {
        let n = (a - b).normalize();
        for (e, n) in [(e0, n), (e1, -n)] {
            let m = &members[e.0];
            let k = m.inward(e.1).dot(&n);
            let over = reach(section, m.axes(e.1), &n) / k;
            let plan = &mut plans[e.0][e.1];
            plan.run_on = plan.run_on.max(over.max(0.0) + margin);
            plan.planes.push((v, n));
            add_cut(plan, angle_cut(k));
            plan.cut_normal = Some(n);
        }
        return Ok(false);
    }
    // Of two as long: the one along the reference, then along X, then Y (so every corner of a
    // regular frame, a cube's too, picks alike).
    let along = |e: &End| {
        let d = members[e.0].inward(e.1);
        [d.dot(&reference).abs(), d.x.abs(), d.y.abs()]
    };
    let runs_on_first = |p: &End, q: &End| {
        let (lp, lq) = (members[p.0].length, members[q.0].length);
        if (lp - lq).abs() > tol {
            return lp > lq;
        }
        for (ap, aq) in along(p).into_iter().zip(along(q)) {
            if (ap - aq).abs() > 1e-6 {
                return ap > aq;
            }
        }
        p.0 < q.0
    };
    let (mut t, mut o) = if runs_on_first(&e0, &e1) {
        (e0, e1)
    } else {
        (e1, e0)
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
        section,
        &through,
        members,
        o,
        v,
        kind == CornerType::CopedButt,
        margin,
        plans,
    );
    Ok(butted)
}

/// Ends cut against the same members at `v` cut against each other too, on the plane halving
/// the angle between each two of them (a post and a diagonal on one chord), so they don't
/// overlap where they meet.
fn split_neighbours(
    section: &[Region],
    members: &[Member],
    ends: &[End],
    v: P,
    margin: f64,
    plans: &mut [[EndPlan; 2]],
) {
    for (i, p) in ends.iter().enumerate() {
        for q in &ends[i + 1..] {
            let (a, b) = (members[p.0].inward(p.1), members[q.0].inward(q.1));
            if a.dot(&b).abs() > TANGENT.cos() {
                continue;
            }
            let n = (a - b).normalize();
            for (e, n) in [(*p, n), (*q, -n)] {
                let m = &members[e.0];
                let k = m.inward(e.1).dot(&n);
                let over = reach(section, m.axes(e.1), &n) / k;
                let plan = &mut plans[e.0][e.1];
                plan.run_on = plan.run_on.max(over.max(0.0) + margin);
                plan.planes.push((v, n));
                add_cut(plan, angle_cut(k));
            }
        }
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
            .then(|| (m.t0, m.axes_along(m.t0)));
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
            // A spline's or an ellipse's: the piece of its points it lies on.
            s.points.windows(2).find_map(|w| {
                let d = w[1] - w[0];
                let l = d.norm();
                if l < 1e-12 {
                    return None;
                }
                let d = d / l;
                let along = (v - w[0]).dot(&d);
                let off = (v - w[0]) - d * along;
                (off.norm() < tol && along > -tol && along < l + tol).then_some(d)
            })
        };
        if let Some(t) = t {
            let t = if rev { -t } else { t };
            return Some((t, m.axes_along(t)));
        }
    }
    None
}

/// The point `d` along a polyline from its first point, and the way it runs there (past its
/// end: on along its last piece).
fn along_points(pts: &[P], d: f64) -> (P, V) {
    let mut left = d;
    let mut way = (pts[1] - pts[0]).normalize();
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).norm();
        if l < 1e-12 {
            continue;
        }
        way = (w[1] - w[0]) / l;
        if left <= l {
            return (w[0] + way * left, way);
        }
        left -= l;
    }
    (pts[pts.len() - 1] + way * left, way)
}

/// Points along a member the way it runs: its lines' ends and its arcs' every eighth.
fn member_points(m: &Member, segs: &[Seg]) -> Vec<P> {
    let mut out = Vec::new();
    for &(i, rev) in &m.segs {
        let s = &segs[i];
        let mut pts = match s.arc {
            Some(c) if !s.line => {
                let r0 = s.start - c.center;
                let r1 = s.end - c.center;
                let y = c.normal.cross(&r0);
                let mut sweep = r1.dot(&y).atan2(r1.dot(&r0));
                if sweep <= 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                (0..=8)
                    .map(|k| {
                        let a = sweep * k as f64 / 8.0;
                        c.center + r0 * a.cos() + y * a.sin()
                    })
                    .collect::<Vec<P>>()
            }
            _ => vec![s.start, s.end],
        };
        if rev {
            pts.reverse();
        }
        out.extend(pts);
    }
    out
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
fn orient(members: &mut [Member], segs: &[Seg], tol: f64) {
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
        let up = n * sign;
        for &i in &run {
            members[i].up = up;
        }
        // A closed loop (its ends meeting, a third member there or not) runs counter-clockwise
        // about its up, whichever way its curves were drawn: a section to its left lies inside.
        // The ends of the run linked to no other member of it (the links name ends as they
        // were before members were turned).
        let mut open: Vec<P> = Vec::new();
        for &i in &run {
            for e in 0..2 {
                let was = e ^ usize::from(turned[i]);
                if !links.iter().any(|(a, b)| *a == (i, was) || *b == (i, was)) {
                    open.push(members[i].point(e));
                }
            }
        }
        let closed =
            open.is_empty() || (open.len() == 2 && (open[0] - open[1]).norm() < tol * 10.0);
        if closed {
            // Its signed area: the fan from p0 over each member's curves (arcs by points).
            let mut turning = 0.0;
            for &i in &run {
                let pts = member_points(&members[i], segs);
                for w in pts.windows(2) {
                    turning += (w[0] - p0).cross(&(w[1] - p0)).dot(&up);
                }
            }
            if turning < 0.0 {
                for &i in &run {
                    members[i].reverse();
                }
            }
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

/// What a mitre made a butt is reported as.
const MITRE_TO_BUTT: &str = "A mitre whose halves wouldn't meet was butted (members turned \
     differently, or an unsymmetric section on members that can't run head to tail)";

/// Whether two member ends mitred meet face to face: each one's section, mirrored across the
/// mitre plane, lies on the other's (`outline` the section's, `size` its reach).
fn mitre_meets(
    outline: &[Vec<[f64; 2]>],
    p: (&Member, usize),
    q: (&Member, usize),
    size: f64,
) -> bool {
    let n = (p.0.inward(p.1) - q.0.inward(q.1)).normalize();
    // Arcs' chords stray from them by about 1.2e-3 of their radius.
    let tol = 2e-3 * size + 1e-9;
    let onto = |(from, to): ((&Member, usize), (&Member, usize))| {
        let ((fx, fy), (tx, ty)) = (from.0.axes(from.1), to.0.axes(to.1));
        outline.iter().flatten().all(|&[u, w]| {
            let r = fx * u + fy * w;
            let r = r - n * (2.0 * r.dot(&n));
            frame::on_outline(outline, [r.dot(&tx), r.dot(&ty)], tol)
        })
    };
    onto((p, q)) && onto((q, p))
}

/// An end's cut with another plane's: two angles make a compound cut.
fn add_cut(plan: &mut EndPlan, cut: EndCut) {
    plan.cut = Some(match (plan.cut, cut) {
        (None | Some(EndCut::Square), c) => c,
        (Some(c), EndCut::Square) => c,
        (Some(EndCut::Angle(a)), EndCut::Angle(b)) => EndCut::Compound(a, b),
        (Some(c @ (EndCut::Coped | EndCut::Compound(..))), _) => c,
        (_, c) => c,
    });
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
            turn: 0.0,
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
        // Not past their far side (a member wider than them would keep its run-on there).
        plan.planes.push((v - n * far, n));
        plan.cut = Some(EndCut::Coped);
    } else {
        plan.run_on = plan.run_on.max(((mine - near) / k).max(0.0) + margin);
        plan.planes.push((v + n * near, n));
        add_cut(plan, angle_cut(k));
        plan.cut_normal = Some(n);
    }
    // The member that ends here runs on to this one's far side: as far along -a as this one's
    // end reaches, its end face sheared across the cut plane at an angle other than square
    // (points v + t·into + q, q in its section, on the plane (p - v)·n = near).
    if let Some((t, t_end)) = through.runs_on {
        let ab = a.dot(&into);
        let w_at = |c: f64| -c * ab / k + reach(section, b_axes, &(n * (ab / k) - a));
        // Coped, the end reaches down to their far side: covered there too.
        let w = if coped {
            w_at(near).max(w_at(-far))
        } else {
            w_at(near)
        };
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
