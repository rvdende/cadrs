//! The kinematic tree of an assembly: its rigid bodies and the joints its mates make between
//! them, from the base outward ([`derive`]), and the exact forward kinematics down that tree
//! ([`Tree::motions`], [`Tree::poses`]).
//!
//! **Bodies.** Instances that cannot move against each other are one body: the members of a
//! group (a rigid subassembly is a group in the solver's model) and the two sides of a
//! Fastened mate. The **base** is every body with a fixed instance, and whatever is fastened
//! to the Origin, taken as one.
//!
//! **Joints.** A mate between two bodies is an edge. Breadth-first from the base, in the Mate
//! Features list's order, the first mate to reach a body carries it (so the tree is the same
//! every time), and becomes one or two joints, each a slide along a line or a turn about one:
//!
//! | mate | joints, from the body nearer the base |
//! |---|---|
//! | Revolute | a turn |
//! | Slider | a slide |
//! | Cylindrical | a slide, then a turn about the same line |
//! | Pin slot | the slot's side nearer the base: a slide along the slot, then a turn about the pin; the pin's side nearer: the turn, then the slide |
//!
//! A Planar, Ball, Parallel, Tangent or Width mate is none of these: what it carries is held
//! to its parent where it is now, which is an error when a mount hangs from it.
//!
//! **Where a joint is.** With F₁ the mate's first connector, G its second moved by the offset
//! and D the mate's position (`G = F₁ · D`, see [`super::super::mate`]): when the first
//! connector's body is the one nearer the base, raising the value by Δ moves the other body
//! by `F₁ · D(Δ) · F₁⁻¹`, a slide along F₁'s Z (a pin slot: its X) or a turn about it. When
//! the second connector's body is the nearer one, the first's moves by `G · D(Δ)⁻¹ · G⁻¹`:
//! the same line, the other way. So each joint is kept as a line in the assembly's
//! coordinates at the placements the document has now, with the mate's value there, and a
//! body is placed by the motions of the joints between it and the base, each for the change
//! of its mate's value from now. At the values the document has, every instance is exactly
//! where it is.
//!
//! **Mates off the tree.** A mate between two bodies the tree already joins either agrees
//! with the tree's motion (a second rail beside the first: the joints between its bodies are
//! moved a few ways and the mate still holds), and is **redundant**, or it does not, and
//! closes a **loop** the tree cannot follow (a four-bar linkage): an error when one of the
//! loop's joints moves a mount, else its joints are held where they are.
//!
//! **Relations.** A Gear, Rack and pinion, Screw or Linear relation between two joints of the
//! tree makes one follow the other, by the relation's ratio from the values they have now.
//! The one on the way to a mount leads; a relation between two such joints is an error (the
//! two would be one axis).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use cadrs_sketch::Vec3;

use super::super::connector::{ConnectorAnchor, ConnectorFrame, MateConnector};
use super::super::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateType, dof_value};
use super::super::{Assembly, InstanceId, Pose};
use super::{Issue, JointKind, MachineDef, Side};
use crate::solid::Solid;

/// Gives a connector's frame in its instance's coordinates (flip, reorient and edits
/// applied), or `None` when what it was picked on is gone.
pub type Frames<'a> = dyn Fn(&MateConnector) -> Option<ConnectorFrame> + 'a;

/// [`Frames`] without the parts: a connector at a fixed frame, the Origin and the assembly's
/// own connectors resolve, to the frames they store; one picked on a face or an edge is lost.
/// For assemblies built in code.
pub fn stored_frames(c: &MateConnector) -> Option<ConnectorFrame> {
    c.resolves(None).then(|| c.local_frame(None))
}

/// [`Frames`] on the source parts as they are now (`solids`: by occurrence, see
/// [`super::super::occurrence_solids`]).
pub fn frames_on(
    solids: &HashMap<InstanceId, Arc<Solid>>,
) -> impl Fn(&MateConnector) -> Option<ConnectorFrame> + '_ {
    |c| {
        let s = solids.get(&c.instance).map(|s| &**s);
        c.resolves(s).then(|| c.local_frame(s))
    }
}

/// How far a mate's rows may be off and still count as holding (mm; an angle weighs like
/// [`TURN`] mm a radian).
const HELD: f64 = 1e-6;
/// The length (mm) a radian weighs like, as the solver's.
const TURN: f64 = super::super::solver::L;

/// An instance with its placement in the document and the body it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub id: InstanceId,
    pub pose: Pose,
    pub body: usize,
}

/// What joins a body to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The base: it has no parent.
    Base,
    /// A mate that became joints ([`Body::joints`]).
    Joint(MateId),
    /// A mate that is not a slide or a turn: the body is held to its parent where it is.
    Held(MateId),
    /// No mate joins it to the base: it stays where it is.
    Loose,
}

/// A rigid body of the tree. A body's parent comes before it in [`Tree::bodies`].
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub members: Vec<InstanceId>,
    pub parent: Option<usize>,
    pub link: Link,
    /// Its joints in [`Tree::joints`], from its parent outward.
    pub joints: Range<usize>,
}

/// Which mount a joint moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Neither: an actuator, or nothing the machine uses.
    Off,
    Tool,
    Table,
    /// Both: a carriage with the tool and the table on it.
    Both,
}

impl Role {
    /// The side of an axis: the mount it alone moves.
    pub fn side(self) -> Option<Side> {
        match self {
            Role::Tool => Some(Side::Tool),
            Role::Table => Some(Side::Table),
            Role::Off | Role::Both => None,
        }
    }
}

/// What sets a joint's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// It is driven by itself: an axis or an actuator.
    Free,
    /// A relation ties it to the joint `joint`: its mate's value changes `ratio` times as much
    /// as that one's.
    Follows { joint: usize, ratio: f64 },
    /// It is part of a loop the tree cannot follow: held where it is.
    Frozen,
}

/// A slide or a turn between a body and its parent: one degree of freedom of a mate.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeJoint {
    pub mate: MateId,
    /// The mate's name ("Slider 1"), for messages.
    pub name: String,
    pub dof: Dof,
    pub kind: JointKind,
    /// A point of a turn's line, and the way the body slides, or the line it turns about
    /// right-handed, as the mate's value rises: in the assembly's coordinates at the
    /// placements the document has.
    pub origin: Vec3,
    pub dir: Vec3,
    /// The mate's value (mm or radians) at those placements.
    pub present: f64,
    /// The mate's limits for this degree of freedom (mm or radians).
    pub limits: Option<(f64, f64)>,
    /// The body it carries.
    pub body: usize,
    pub role: Role,
    pub motion: Motion,
}

impl TreeJoint {
    /// How it moves what it carries when its mate's value is `delta` more than now.
    pub fn move_by(&self, delta: f64) -> Pose {
        match self.kind {
            JointKind::Linear => Pose::translation([
                self.dir[0] * delta,
                self.dir[1] * delta,
                self.dir[2] * delta,
            ]),
            JointKind::Rotary => Pose::rotation_about(self.origin, self.dir, delta),
        }
    }

    pub fn is_rotary(&self) -> bool {
        self.kind == JointKind::Rotary
    }
}

/// A mount as the tree has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Mount {
    pub body: usize,
    /// The connector's frame in the assembly's coordinates, at the document's placements.
    pub frame: ConnectorFrame,
    /// The joints between the base and it, from the base outward.
    pub path: Vec<usize>,
}

/// Mates that close a loop the tree cannot follow.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    /// The mate that closes it, then the mates round it.
    pub mates: Vec<MateId>,
    /// The tree's joints round it.
    pub joints: Vec<usize>,
    /// One of them moves a mount.
    pub on_path: bool,
}

/// The kinematic tree of an assembly (see the module).
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    /// The bodies, the base first when there is one, every parent before its children.
    pub bodies: Vec<Body>,
    /// The joints, grouped by the body they carry.
    pub joints: Vec<TreeJoint>,
    /// Every instance of the solver's model, in its order.
    pub instances: Vec<Placed>,
    /// Whether anything is fixed: without a base there are no joints.
    pub grounded: bool,
    pub tool: Option<Mount>,
    pub table: Option<Mount>,
    /// Mates left out because the tree's motion already satisfies them (a second rail).
    pub redundant: Vec<MateId>,
    pub loops: Vec<Loop>,
    /// What stands in the way of a machine, and what was left out.
    pub issues: Vec<Issue>,
}

impl Tree {
    /// The joints between the base and `body`, from the base outward.
    pub fn path(&self, body: usize) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut at = Some(body);
        while let Some(b) = at {
            chain.push(b);
            at = self.bodies[b].parent;
        }
        chain
            .iter()
            .rev()
            .flat_map(|b| self.bodies[*b].joints.clone())
            .collect()
    }

    /// The joint of a mate's degree of freedom.
    pub fn joint_of(&self, mate: MateId, dof: Dof) -> Option<usize> {
        self.joints
            .iter()
            .position(|j| j.mate == mate && j.dof == dof)
    }

    /// The body an instance belongs to.
    pub fn body_of(&self, instance: InstanceId) -> Option<usize> {
        self.instances
            .iter()
            .find(|p| p.id == instance)
            .map(|p| p.body)
    }

    /// The joints that are driven by themselves and move neither mount: what an actuator can
    /// be.
    pub fn free_off_path(&self) -> impl Iterator<Item = (usize, &TreeJoint)> {
        self.joints
            .iter()
            .enumerate()
            .filter(|(_, j)| j.role == Role::Off && j.motion == Motion::Free)
    }

    /// Where every body goes from where it is now when joint `j`'s mate value is `delta(j)`
    /// more than now: `out[b]` moves body `b`'s points, given in the assembly's coordinates
    /// at the document's placements. `out` is as long as [`Tree::bodies`].
    pub fn motions(&self, delta: &dyn Fn(usize) -> f64, out: &mut [Pose]) {
        for (b, body) in self.bodies.iter().enumerate() {
            // The parent's motion, then each joint's out from it: the nearer joint carries
            // the farther one's line with it.
            let mut at = body.parent.map_or(Pose::IDENTITY, |p| out[p]);
            for j in body.joints.clone() {
                let d = delta(j);
                if d != 0.0 {
                    at = self.joints[j].move_by(d).then(&at);
                }
            }
            out[b] = at;
        }
    }

    /// [`Tree::motions`] up to the joint `j`: where the side of `j` nearer the base goes (its
    /// parent's motion and those of the joints of the same mate before it).
    pub fn motion_before(&self, j: usize, delta: &dyn Fn(usize) -> f64) -> Pose {
        let body = &self.bodies[self.joints[j].body];
        let mut at = Pose::IDENTITY;
        for k in (body.joints.start..j).rev() {
            at = at.then(&self.joints[k].move_by(delta(k)));
        }
        let mut up = body.parent;
        while let Some(b) = up {
            for k in self.bodies[b].joints.clone().rev() {
                at = at.then(&self.joints[k].move_by(delta(k)));
            }
            up = self.bodies[b].parent;
        }
        at
    }

    /// Every instance's placement when joint `j`'s mate value is `delta(j)` more than now,
    /// written over `out`. An instance of a body that has not moved keeps its placement to
    /// the bit.
    pub fn place(&self, delta: &dyn Fn(usize) -> f64, out: &mut Vec<(InstanceId, Pose)>) {
        // The bodies' motions live on the stack for any machine of ordinary size: this runs
        // every frame of an animation.
        const ON_STACK: usize = 48;
        let mut few = [Pose::IDENTITY; ON_STACK];
        let mut many = Vec::new();
        let n = self.bodies.len();
        let motions: &mut [Pose] = if n <= ON_STACK {
            &mut few[..n]
        } else {
            many.resize(n, Pose::IDENTITY);
            &mut many
        };
        self.motions(delta, motions);
        out.clear();
        out.extend(self.instances.iter().map(|p| {
            let m = &motions[p.body];
            (
                p.id,
                if *m == Pose::IDENTITY {
                    p.pose
                } else {
                    p.pose.then(m)
                },
            )
        }));
    }

    /// Every instance's placement with the mates at `values` (mm or radians; a mate not
    /// listed stays at its value now, and a joint that follows another goes with it).
    pub fn poses(&self, values: &[(MateId, Dof, f64)]) -> Vec<(InstanceId, Pose)> {
        let own = |j: usize| -> f64 {
            let joint = &self.joints[j];
            values
                .iter()
                .find(|(m, d, _)| *m == joint.mate && *d == joint.dof)
                .map_or(0.0, |(_, _, v)| v - joint.present)
        };
        let delta = |j: usize| -> f64 {
            let (lead, ratio) = self.leader(j);
            match self.joints[lead].motion {
                Motion::Frozen => 0.0,
                _ if lead == j => own(j),
                _ => ratio * own(lead),
            }
        };
        let mut out = Vec::with_capacity(self.instances.len());
        self.place(&delta, &mut out);
        out
    }

    /// The joint that `j` in the end follows (itself when it follows none), and how many
    /// times that one's change `j` changes by.
    pub fn leader(&self, j: usize) -> (usize, f64) {
        let (mut at, mut ratio) = (j, 1.0);
        // (No longer than the joints there are: `derive` lets no joint follow itself.)
        for _ in 0..self.joints.len() {
            match self.joints[at].motion {
                Motion::Follows { joint, ratio: r } => {
                    at = joint;
                    ratio *= r;
                }
                _ => break,
            }
        }
        (at, ratio)
    }

    /// Whether an error stands in the way.
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.is_error())
    }
}

/// A mate of the model with its connectors found.
struct Found<'a> {
    feature: &'a MateFeature,
    mate: &'a Mate,
    /// Each connector's node (see [`derive`]) and its frame in the assembly's coordinates.
    ends: Vec<(usize, ConnectorFrame)>,
}

impl Found<'_> {
    /// The first connector's frame and the second's moved by the offset.
    fn pair(&self) -> (ConnectorFrame, ConnectorFrame) {
        (self.ends[0].1, self.mate.target(&self.ends[1].1))
    }
}

/// An edge between two bodies (numbered as the union–find leaves them).
struct Edge {
    /// Which of the found mates.
    mate: usize,
    a: usize,
    b: usize,
    /// It is a slide, a turn or two of them.
    jointed: bool,
}

/// Whether a mate type is made of slides and turns.
pub fn is_jointed(t: MateType) -> bool {
    matches!(
        t,
        MateType::Revolute | MateType::Slider | MateType::Cylindrical | MateType::PinSlot
    )
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn neg(a: Vec3) -> Vec3 {
    [-a[0], -a[1], -a[2]]
}

fn norm(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// How far the mate of type `t` is from holding between the frames `f1` and `g` (mm, turns
/// weighing [`TURN`] mm a radian): the largest of the rows its type removes. `None` for a
/// Tangent or a Width, which are not between two frames.
pub fn violation(t: MateType, f1: &ConnectorFrame, g: &ConnectorFrame) -> Option<f64> {
    let d = sub(g.origin, f1.origin);
    let (dx, dy, dz) = (dot(d, f1.x), dot(d, f1.y()), dot(d, f1.z));
    let axis = norm(sub(f1.z, g.z)) * TURN;
    let spin = norm(sub(f1.x, g.x)) * TURN;
    Some(match t {
        MateType::Fastened => norm(d).max(axis).max(spin),
        MateType::Revolute => norm(d).max(axis),
        MateType::Slider => dx.hypot(dy).max(axis).max(spin),
        MateType::Cylindrical => dx.hypot(dy).max(axis),
        MateType::PinSlot => dy.hypot(dz).max(axis),
        MateType::Planar => dz.abs().max(axis),
        MateType::Ball => norm(d),
        MateType::Parallel => axis,
        MateType::Tangent | MateType::Width => return None,
    })
}

/// A connector's instance and frame (its instance's coordinates), and whether what it was
/// picked on is still there; when it is not, the frame is the one it last had. A connector on
/// one of the assembly's own connectors is on that one's instance, at its frame, with the
/// mate's flip, reorient and edits on top (as [`super::super::resolve_local_connectors`]).
fn seen(flat: &Assembly, frames: &Frames, c: &MateConnector) -> (InstanceId, ConnectorFrame, bool) {
    if let ConnectorAnchor::Local { id } = c.anchor
        && let Some(l) = flat.local_connector(id)
    {
        let base = frames(&l.connector);
        let on = MateConnector {
            instance: l.connector.instance,
            frame: base.unwrap_or_else(|| l.connector.local_frame(None)),
            anchor: ConnectorAnchor::Frame,
            ..*c
        };
        return (on.instance, on.local_frame(None), base.is_some());
    }
    match frames(c) {
        Some(f) => (c.instance, f, true),
        None => (c.instance, c.local_frame(None), false),
    }
}

/// "A", "A and B", "A, B and C".
pub(super) fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A sequence of numbers in (0, 1) that is the same every run (xorshift).
struct Sequence(u64);

impl Sequence {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The kinematic tree of `flat` (the solver's model with Replicates expanded,
/// [`super::flat_model`]) for the mounts of `def`, its connectors found by `frames`.
pub fn derive(flat: &Assembly, def: &MachineDef, frames: &Frames) -> Tree {
    let mut issues: Vec<Issue> = Vec::new();

    // The nodes: every instance, and the Origin when a mate uses it.
    let mut nodes: Vec<(InstanceId, Pose, bool)> = flat
        .instances
        .iter()
        .map(|i| (i.id, i.pose, i.fixed))
        .collect();
    let uses_origin = flat.mates.iter().filter(|f| !f.suppressed).any(|f| {
        f.mate()
            .is_some_and(|m| m.all_connectors().any(|c| c.is_origin()))
    });
    if uses_origin {
        nodes.push((InstanceId::ORIGIN, Pose::IDENTITY, true));
    }
    let index: HashMap<InstanceId, usize> =
        nodes.iter().enumerate().map(|(k, n)| (n.0, k)).collect();

    // The mates with both sides in the model, their connectors found.
    let mut lost: Vec<MateId> = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    for f in flat.mates.iter().filter(|f| !f.suppressed) {
        let MateKind::Mate(m) = &f.kind else { continue };
        if m.mate_type == MateType::Width && m.tabs.is_empty() {
            continue;
        }
        let conns: Vec<&MateConnector> = if m.mate_type == MateType::Width {
            m.all_connectors().collect()
        } else {
            m.connectors.iter().collect()
        };
        let mut ends = Vec::with_capacity(conns.len());
        let mut all_found = true;
        for c in &conns {
            let (instance, local, ok) = seen(flat, frames, c);
            let Some(k) = index.get(&instance).copied() else {
                break;
            };
            all_found &= ok;
            ends.push((k, local.moved(&nodes[k].1)));
        }
        // (A connector on an instance the model has not: the solver skips the mate too.)
        if ends.len() < conns.len() {
            continue;
        }
        if !all_found {
            lost.push(f.id);
        }
        found.push(Found {
            feature: f,
            mate: m,
            ends,
        });
    }
    if !lost.is_empty() {
        issues.push(
            Issue::warning(
                "A mate has lost the face or edge it was picked on: it is taken where it last was",
            )
            .mates(lost),
        );
    }

    // Bodies: nodes joined by groups and Fastened mates; everything fixed is one base.
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    fn union(p: &mut [usize], a: usize, b: usize) {
        let (a, b) = (find(p, a), find(p, b));
        if a != b {
            p[a.max(b)] = a.min(b);
        }
    }
    for f in flat.mates.iter().filter(|f| !f.suppressed) {
        if let MateKind::Group { instances } = &f.kind {
            let members: Vec<usize> = instances
                .iter()
                .filter_map(|i| index.get(i).copied())
                .collect();
            for w in members.windows(2) {
                union(&mut parent, w[0], w[1]);
            }
        }
    }
    for m in found
        .iter()
        .filter(|m| m.mate.mate_type == MateType::Fastened)
    {
        union(&mut parent, m.ends[0].0, m.ends[1].0);
    }
    let fixed: Vec<usize> = (0..nodes.len()).filter(|k| nodes[*k].2).collect();
    for w in fixed.windows(2) {
        union(&mut parent, w[0], w[1]);
    }
    let mut raw_of_root: HashMap<usize, usize> = HashMap::new();
    let mut raw_members: Vec<Vec<usize>> = Vec::new();
    let mut raw_of: Vec<usize> = Vec::with_capacity(nodes.len());
    for k in 0..nodes.len() {
        let r = find(&mut parent, k);
        let b = *raw_of_root.entry(r).or_insert_with(|| {
            raw_members.push(Vec::new());
            raw_members.len() - 1
        });
        raw_members[b].push(k);
        raw_of.push(b);
    }
    let base = fixed.first().map(|k| raw_of[*k]);
    if base.is_none() && !nodes.is_empty() {
        issues.push(Issue::error("Fix the machine's base"));
    }

    // The mates that are not held by their own type between their bodies now.
    let unheld: Vec<MateId> = found
        .iter()
        .filter(|m| {
            let (f1, g) = m.pair();
            violation(m.mate.mate_type, &f1, &g).is_some_and(|v| v > HELD)
        })
        .map(|m| m.feature.id)
        .collect();
    if !unheld.is_empty() {
        issues.push(
            Issue::warning(
                "A mate does not hold where its parts are now: solve the assembly first",
            )
            .mates(unheld),
        );
    }

    // Edges, in the mates' order.
    let mut edges: Vec<Edge> = Vec::new();
    for (k, m) in found.iter().enumerate() {
        let t = m.mate.mate_type;
        if t == MateType::Fastened {
            continue;
        }
        let a = raw_of[m.ends[0].0];
        if is_jointed(t) {
            let b = raw_of[m.ends[1].0];
            if a != b {
                edges.push(Edge {
                    mate: k,
                    a,
                    b,
                    jointed: true,
                });
            }
            continue;
        }
        let mut others: Vec<usize> = Vec::new();
        for e in &m.ends[1..] {
            let b = raw_of[e.0];
            if b != a && !others.contains(&b) {
                others.push(b);
                edges.push(Edge {
                    mate: k,
                    a,
                    b,
                    jointed: false,
                });
            }
        }
    }

    // Breadth-first from the base: `order` lists the reached bodies, `via` the edge that
    // reached each and the body it came from.
    let mut via: Vec<Option<(usize, usize)>> = vec![None; raw_members.len()];
    let mut reached = vec![false; raw_members.len()];
    let mut order: Vec<usize> = Vec::new();
    let mut tree_edge = vec![false; edges.len()];
    if let Some(b) = base {
        reached[b] = true;
        order.push(b);
        let mut next = 0;
        while next < order.len() {
            let u = order[next];
            next += 1;
            for (k, e) in edges.iter().enumerate() {
                let v = if e.a == u {
                    e.b
                } else if e.b == u {
                    e.a
                } else {
                    continue;
                };
                if !reached[v] {
                    reached[v] = true;
                    via[v] = Some((k, u));
                    tree_edge[k] = true;
                    order.push(v);
                }
            }
        }
    }
    let joined = order.len();
    order.extend((0..raw_members.len()).filter(|b| !reached[*b]));
    let mut number = vec![0usize; raw_members.len()];
    for (n, raw) in order.iter().enumerate() {
        number[*raw] = n;
    }

    // The bodies and their joints.
    let mut bodies: Vec<Body> = Vec::with_capacity(order.len());
    let mut joints: Vec<TreeJoint> = Vec::new();
    for (n, raw) in order.iter().enumerate() {
        let members = raw_members[*raw]
            .iter()
            .map(|k| nodes[*k].0)
            .filter(|i| *i != InstanceId::ORIGIN)
            .collect();
        let start = joints.len();
        let (up, link) = match via[*raw] {
            _ if n >= joined => (None, Link::Loose),
            None => (None, Link::Base),
            Some((k, from)) => {
                let e = &edges[k];
                let m = &found[e.mate];
                if e.jointed {
                    joints_of(m, e.a == from, n, &mut joints);
                    (Some(number[from]), Link::Joint(m.feature.id))
                } else {
                    (Some(number[from]), Link::Held(m.feature.id))
                }
            }
        };
        bodies.push(Body {
            members,
            parent: up,
            link,
            joints: start..joints.len(),
        });
    }
    let instances: Vec<Placed> = flat
        .instances
        .iter()
        .enumerate()
        .map(|(k, i)| Placed {
            id: i.id,
            pose: i.pose,
            body: number[raw_of[k]],
        })
        .collect();
    let mut tree = Tree {
        bodies,
        joints,
        instances,
        grounded: base.is_some(),
        tool: None,
        table: None,
        redundant: Vec::new(),
        loops: Vec::new(),
        issues: Vec::new(),
    };

    // The mounts and the joints on the way to each.
    let mut mount = |c: &Option<MateConnector>, what: &str, tree: &Tree| -> Option<Mount> {
        let Some(c) = c else {
            issues.push(Issue::error(format!("Pick the {what} mount")));
            return None;
        };
        let (instance, local, ok) = seen(flat, frames, c);
        let Some(k) = index.get(&instance).copied() else {
            issues.push(Issue::error(format!(
                "The {what} mount's part is gone: pick it again"
            )));
            return None;
        };
        if !ok {
            issues.push(
                Issue::error(format!(
                    "The {what} mount has lost the face it was picked on: pick it again"
                ))
                .instances([instance]),
            );
            return None;
        }
        let body = number[raw_of[k]];
        if tree.grounded && tree.bodies[body].link == Link::Loose {
            issues.push(
                Issue::error(format!(
                    "No mate joins the {what} mount's part to the machine's base"
                ))
                .instances([instance]),
            );
            return None;
        }
        Some(Mount {
            body,
            frame: local.moved(&nodes[k].1),
            path: tree.path(body),
        })
    };
    tree.tool = mount(&def.tool_mount, "tool", &tree);
    tree.table = mount(&def.table_mount, "table", &tree);
    for (m, side) in [(&tree.tool, Role::Tool), (&tree.table, Role::Table)] {
        for j in m.iter().flat_map(|m| m.path.iter()) {
            let role = &mut tree.joints[*j].role;
            *role = if *role == Role::Off { side } else { Role::Both };
        }
    }

    // A body held by a mate that is no joint.
    let on_way: Vec<(usize, &str)> = [(&tree.tool, "tool"), (&tree.table, "table")]
        .into_iter()
        .filter_map(|(m, what)| m.as_ref().map(|m| (m.body, what)))
        .collect();
    let above = |tree: &Tree, body: usize, of: usize| -> bool {
        let mut at = Some(of);
        while let Some(b) = at {
            if b == body {
                return true;
            }
            at = tree.bodies[b].parent;
        }
        false
    };
    for (b, body) in tree.bodies.iter().enumerate() {
        let Link::Held(id) = body.link else { continue };
        let Some(m) = found.iter().find(|m| m.feature.id == id) else {
            continue;
        };
        let (name, kind) = (&m.feature.name, m.mate.mate_type.label());
        match on_way.iter().find(|(of, _)| above(&tree, b, *of)) {
            Some((_, what)) => issues.push(
                Issue::error(format!(
                    "{name} is a {kind} mate between the base and the {what}: only Revolute, \
                     Slider, Cylindrical and Pin slot mates can be a machine's axes"
                ))
                .mates([id])
                .instances(body.members.iter().copied()),
            ),
            None => issues.push(
                Issue::warning(format!(
                    "{name} is a {kind} mate, which the machine cannot move: what it holds stays \
                     where it is"
                ))
                .mates([id])
                .instances(body.members.iter().copied()),
            ),
        }
    }

    // The mates the tree did not use: redundant, or a loop.
    let mut tries = Sequence(0x2545_f491_4f6c_dd1d);
    let mut motions = vec![Pose::IDENTITY; tree.bodies.len()];
    for (k, e) in edges.iter().enumerate() {
        if tree_edge[k] || !reached[e.a] || !reached[e.b] {
            continue;
        }
        let m = &found[e.mate];
        let id = m.feature.id;
        if tree.redundant.contains(&id) || tree.loops.iter().any(|l| l.mates[0] == id) {
            continue;
        }
        // Round the loop: up from both bodies to where the two ways to the base meet.
        let (mut a, mut b) = (number[e.a], number[e.b]);
        let mut round: Vec<usize> = Vec::new();
        let mut mates: Vec<MateId> = vec![id];
        while a != b {
            let far = a.max(b);
            round.extend(tree.bodies[far].joints.clone());
            if let Link::Joint(x) | Link::Held(x) = tree.bodies[far].link {
                mates.push(x);
            }
            let up = tree.bodies[far].parent.unwrap_or(0);
            if a == far {
                a = up;
            } else {
                b = up;
            }
        }
        // Does the mate hold however the loop's joints move? (A few moves of them all at
        // once, each a fair way: a mate that survives them is satisfied by the tree itself.)
        let (f1, g) = m.pair();
        let (ba, bb) = (number[raw_of[m.ends[0].0]], number[raw_of[m.ends[1].0]]);
        let holds = round.is_empty()
            || (0..4).all(|_| {
                let moves: Vec<(usize, f64)> = round
                    .iter()
                    .map(|j| {
                        let far = if tree.joints[*j].is_rotary() {
                            0.2 + 0.4 * tries.next()
                        } else {
                            7.0 + 13.0 * tries.next()
                        };
                        (*j, if tries.next() < 0.5 { -far } else { far })
                    })
                    .collect();
                let delta = |j: usize| moves.iter().find(|(k, _)| *k == j).map_or(0.0, |(_, d)| *d);
                tree.motions(&delta, &mut motions);
                violation(
                    m.mate.mate_type,
                    &f1.moved(&motions[ba]),
                    &g.moved(&motions[bb]),
                )
                .is_some_and(|v| v < HELD)
            });
        if holds {
            tree.redundant.push(id);
            continue;
        }
        let on_path = round.iter().any(|j| tree.joints[*j].role != Role::Off);
        tree.loops.push(Loop {
            mates,
            joints: round,
            on_path,
        });
    }
    let name_of = |id: MateId| -> String {
        flat.mate(id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "A mate".into())
    };
    for l in &tree.loops {
        let names: Vec<String> = l.mates.iter().map(|m| name_of(*m)).collect();
        if l.on_path {
            issues.push(
                Issue::error(format!(
                    "{} close a loop that moves the tool or the table: a machine's axes must \
                     each move by themselves",
                    listed(&names)
                ))
                .mates(l.mates.iter().copied()),
            );
        } else {
            issues.push(
                Issue::warning(format!(
                    "{} close a loop, which the machine cannot move: its parts stay where they \
                     are",
                    listed(&names)
                ))
                .mates(l.mates.iter().copied()),
            );
        }
    }
    let frozen: Vec<usize> = tree
        .loops
        .iter()
        .flat_map(|l| l.joints.iter().copied())
        .collect();
    for j in frozen {
        tree.joints[j].motion = Motion::Frozen;
    }

    // Relations: one joint follows the other.
    for f in flat.mates.iter().filter(|f| !f.suppressed) {
        let Some(r) = f.relation() else { continue };
        let Some(types) = super::super::relation::mate_types(r, &flat.mates) else {
            continue;
        };
        let Some(c) = super::super::relation::coupling(r, &types) else {
            continue;
        };
        let (Some(first), Some(second)) = (r.mates.first(), r.mates.get(1).or(r.mates.first()))
        else {
            continue;
        };
        let (Some(j0), Some(j1)) = (
            tree.joint_of(*first, c.dofs[0]),
            tree.joint_of(*second, c.dofs[1]),
        ) else {
            issues.push(
                Issue::warning(format!(
                    "{} is not followed: a mate of it is not one of the machine's joints",
                    f.name
                ))
                .mates([f.id]),
            );
            continue;
        };
        let (r0, r1) = (tree.joints[j0].role, tree.joints[j1].role);
        if r0 != Role::Off && r1 != Role::Off {
            let names = [tree.joints[j0].name.clone(), tree.joints[j1].name.clone()];
            issues.push(
                Issue::error(if j0 != j1 && names[0] != names[1] {
                    format!(
                        "{} ties {}, which both move the tool or the table: only one of them \
                         can be an axis",
                        f.name,
                        listed(&names)
                    )
                } else {
                    format!(
                        "{} ties the slide and the turn of {}, which moves the tool or the \
                         table: an axis slides or turns, not both",
                        f.name, names[0]
                    )
                })
                .mates([f.id, tree.joints[j0].mate, tree.joints[j1].mate]),
            );
            continue;
        }
        // The one that moves a mount leads; then the one the definition names as an
        // actuator; then the relation's first.
        let named = |j: usize| {
            let joint = &tree.joints[j];
            def.actuators
                .iter()
                .any(|a| a.mate == joint.mate && a.dof == joint.dof)
        };
        let lead_second = r1 != Role::Off || (r0 == Role::Off && named(j1) && !named(j0));
        let (lead, follower, k_lead, k_follower) = if lead_second {
            (j1, j0, c.k[1], c.k[0])
        } else {
            (j0, j1, c.k[0], c.k[1])
        };
        let free = tree.joints[follower].motion == Motion::Free;
        if !free || k_follower.abs() < 1e-12 || tree.leader(lead).0 == follower {
            issues.push(
                Issue::warning(format!(
                    "{} is not followed: {} is already moved by something else",
                    f.name, tree.joints[follower].name
                ))
                .mates([f.id]),
            );
            continue;
        }
        tree.joints[follower].motion = Motion::Follows {
            joint: lead,
            ratio: -k_lead / k_follower,
        };
    }

    // A joint under both mounts.
    for j in tree.joints.iter().filter(|j| j.role == Role::Both) {
        issues.push(
            Issue::warning(format!(
                "{} moves the tool and the table together: it is not an axis",
                j.name
            ))
            .mates([j.mate]),
        );
    }
    if tree.tool.is_some()
        && tree.table.is_some()
        && !tree
            .joints
            .iter()
            .any(|j| matches!(j.role, Role::Tool | Role::Table))
    {
        issues.push(Issue::error(
            "No mate moves the tool against the table: the machine has no axes",
        ));
    }
    tree.issues = issues;
    tree
}

/// The joints of the mate `m` for the body numbered `body`, from its parent outward.
/// `first_is_parent`: the mate's first connector is on the parent.
fn joints_of(m: &Found, first_is_parent: bool, body: usize, out: &mut Vec<TreeJoint>) {
    let (f1, g) = m.pair();
    let mut push = |dof: Dof, kind: JointKind, origin: Vec3, dir: Vec3| {
        out.push(TreeJoint {
            mate: m.feature.id,
            name: m.feature.name.clone(),
            dof,
            kind,
            origin,
            dir,
            present: dof_value(&f1, &g, dof),
            limits: m.mate.limit(dof),
            body,
            role: Role::Off,
            motion: Motion::Free,
        });
    };
    use JointKind::{Linear, Rotary};
    // (See the module: the first connector's line when its body is the parent, the target's
    // the other way when not.)
    let (at, z) = if first_is_parent {
        (f1.origin, f1.z)
    } else {
        (g.origin, neg(g.z))
    };
    match m.mate.mate_type {
        MateType::Revolute => push(Dof::Angle, Rotary, at, z),
        MateType::Slider => push(Dof::Z, Linear, at, z),
        MateType::Cylindrical => {
            push(Dof::Z, Linear, at, z);
            push(Dof::Angle, Rotary, at, z);
        }
        // D = slide along the slot's X, then the turn about the pin: the pin is where the
        // target is now.
        MateType::PinSlot if first_is_parent => {
            push(Dof::X, Linear, f1.origin, f1.x);
            push(Dof::Angle, Rotary, g.origin, f1.z);
        }
        // The pin's side is the parent: the slot's body turns about the pin and slides along
        // its own slot, which turns with it.
        MateType::PinSlot => {
            push(Dof::Angle, Rotary, g.origin, neg(g.z));
            push(Dof::X, Linear, f1.origin, neg(f1.x));
        }
        _ => {}
    }
}
