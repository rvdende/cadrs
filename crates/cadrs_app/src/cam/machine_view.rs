//! The machine in a CAM Studio's 3D view (issue #278): when the active job's machine is an
//! assembly of the document ([`cadrs_core::cam::MachineSource`]), its parts are drawn round
//! the work and move with the preview.
//!
//! **Where it stands.** The CAM view is fixed to the work: parts, stock and toolpaths are in
//! model coordinates. So the machine moves round the work, the table under it included. With
//! the setup's frame S (setup → model), the setup's place on the machine W
//! ([`cadrs_core::cam::program::SetupRun::work`]: setup → the table mount's coordinates), the
//! table mount T(q) (mount → assembly) at the axis values q and an instance's placement P(q)
//! (instance → assembly), the instance is drawn at `S ∘ W⁻¹ ∘ T(q)⁻¹ ∘ P(q)` ([`place`]).
//!
//! **Relative to the machine.** That is [`Relative::Part`]. With [`Relative::Machine`] (the
//! default: what a machinist sees standing at the machine) the machine's base stays where it
//! stands at home and the work turns with the table: the whole scene of the part-relative view
//! is moved by `W(q) = M(home) ∘ M(q)⁻¹` ([`work_moved`]), with M(q) the machine's coordinates
//! in the model's at the axis values q ([`machine_in_model`]). An instance is then drawn at
//! `M(home) ∘ P(q)`, and every thing of the work (the parts and the workholding, the stock, the
//! toolpaths, the triads, the tool in the spindle) at W. [`place_work`] works W out before the
//! parts are placed; it is the identity without a preview (the machine is at home then), with
//! the machine hidden and while a dialog is open, so picking always finds the parts where their
//! solids are.
//!
//! **What moves it.** While a preview runs, the axis values at its time: the run's
//! [`cadrs_cam::kinematics::AxisTrack`] has them at the end of every move, and between two
//! the cutter location of that instant is solved on from the move's start ([`axes_at`]). The
//! magazine actuator turns to the pocket of the tool in the spindle. Without a preview the
//! machine stands at home under the selected setup (else the job's first).
//!
//! **What is drawn.** One mesh an instance of the assembly, from its Part Studio's rebuild
//! ([`crate::assembly::AssemblyParts`] keeps those), with its edges; enclosure instances
//! see-through. They are no parts of the tab ([`crate::parts::PartCache`]): nothing picks
//! them, and zoom to fit and the selection do not know of them. A frame only sets their
//! `Transform`s. The toolbar's Show machine and the preview bar's Machine check hide them.
//!
//! **Collisions.** [`machine_collisions`] steps the machine through the run (on the preview's
//! analysis worker) and tests the instances that move with the tool against those that move
//! with the table, the workholding, the part and what stands still (the base, the column),
//! and the tool in the spindle with its holder against the table's side and the workholding
//! ([`cadrs_cam::collide`]); pairs of the machine's own that touch with it at home are
//! forgiven. While the preview's time is inside a collision its instances are drawn red.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::gizmos::GizmoAsset;
use bevy::gizmos::config::{GizmoLineConfig, GizmoLineJoint};
use bevy::gizmos::retained::Gizmo;
use bevy::prelude::*;
use cadrs_core::assembly::connector::ConnectorFrame;
use cadrs_core::assembly::machine::derive::Role;
use cadrs_core::assembly::machine::{self, ActuatorId, Resolved};
use cadrs_core::assembly::{InstanceId, Pose};
use cadrs_core::cam::cadrs_cam::collide::Scene;
use cadrs_core::cam::cadrs_cam::cutter::{ARBOR_COLOR, HOLDER_COLOR, cutter_mesh};
use cadrs_core::cam::cadrs_cam::kinematics::{AxisTrack, Chain, Cl, Xf};
use cadrs_core::cam::cadrs_cam::{Frame, P3, Tool};
use cadrs_core::cam::{CamId, program};
use cadrs_core::{ElementId, Solid};

use super::{CamUi, active_cam};
use crate::part_shading::PartShading;
use crate::parts::FaceBase;
use crate::{ActiveDocument, AppState};

pub struct MachineViewPlugin;

impl Plugin for MachineViewPlugin {
    fn build(&self, app: &mut App) {
        // ([`place_work`] runs in the preview's chain, after its time moves on and before the
        // parts are placed.)
        app.init_resource::<MachineView>()
            .add_systems(
                Update,
                (load_machine, move_machine)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                place_on_work
                    .after(move_machine)
                    .after(super::view::ViewSet)
                    .after(super::preview::PreviewSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                OnExit(AppState::Document),
                |mut view: ResMut<MachineView>| *view = MachineView::default(),
            );
    }
}

// ---------------------------------------------------------------------------------------------
// Where the machine stands

/// The placement that takes a frame's own coordinates to those it is given in.
pub fn frame_pose(f: &Frame) -> Pose {
    Pose {
        rotation: [
            [f.x[0], f.y[0], f.z[0]],
            [f.x[1], f.y[1], f.z[1]],
            [f.x[2], f.y[2], f.z[2]],
        ],
        translation: f.origin,
    }
}

/// The machine's (its assembly's) coordinates in the model's: `setup` is the setup's frame in
/// the model, `work` the setup's frame in the table mount's coordinates, `table_mount` the
/// mount's connector in the assembly at the axis values shown.
pub fn machine_in_model(setup: &Frame, work: &Frame, table_mount: &ConnectorFrame) -> Pose {
    table_mount
        .pose()
        .inverse()
        .then(&frame_pose(work).inverse())
        .then(&frame_pose(setup))
}

/// Where an instance of the machine is drawn in the model's coordinates: `instance` its
/// placement in the assembly at the axis values shown (see the module).
pub fn place(setup: &Frame, work: &Frame, table_mount: &ConnectorFrame, instance: &Pose) -> Pose {
    instance.then(&machine_in_model(setup, work, table_mount))
}

/// What the CAM view keeps still while the machine moves: the part (the machine moves round
/// it) or the machine (the work turns with its table). View state, not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Relative {
    Part,
    /// The default: with a machine from an assembly it is what a machinist expects; without
    /// one there is no machine to keep still, and the setting does nothing.
    #[default]
    Machine,
}

/// How far the work moves in the view to keep the machine still (see the module): the model's
/// coordinates of the part-relative view to those of the machine-relative one. `home_table`
/// and `table` are the table mount's connector at home and at the axis values shown; the
/// identity when they are the same.
pub fn work_moved(
    setup: &Frame,
    work: &Frame,
    home_table: &ConnectorFrame,
    table: &ConnectorFrame,
) -> Pose {
    machine_in_model(setup, work, table)
        .inverse()
        .then(&machine_in_model(setup, work, home_table))
}

/// The machine's axis values part of the way through move `segment` of a run, the tool's tip
/// then at `tip` with its axis along `axis` (the setup's coordinates) and `length` from its
/// tip to the spindle's gauge line; `work` is the setup's frame on the table mount.
pub fn axes_at(
    chain: &Chain,
    track: &AxisTrack,
    work: &Frame,
    segment: usize,
    tip: P3,
    axis: P3,
    length: f64,
) -> Vec<f64> {
    track.at(chain, segment, &Cl::new(tip, axis).out_of(work), length)
}

/// The magazine's value (mm or degrees) that brings tool `number`'s pocket to the spindle,
/// with the magazine: `None` for a machine without one, or without a pocket of that number.
pub fn magazine_for(def: &machine::MachineDef, number: u32) -> Option<(ActuatorId, f64)> {
    let id = def.tool_change.magazine?;
    def.tool_change
        .pockets
        .iter()
        .find(|p| p.number == number)
        .map(|p| (id, p.value))
}

fn xf_of(p: &Pose) -> Xf {
    Xf {
        r: p.rotation,
        t: p.translation,
    }
}

// ---------------------------------------------------------------------------------------------
// Collisions

/// A thing of the machine for the collision check: an instance with its surface in its own
/// coordinates.
#[derive(Clone)]
pub struct ScenePart {
    pub instance: InstanceId,
    pub name: String,
    pub solid: Arc<Solid>,
}

/// The machine for the collision check of a run.
#[derive(Clone)]
pub struct MachineScene {
    pub resolved: Arc<Resolved>,
    pub parts: Vec<ScenePart>,
    /// The definition's magazine and pockets ([`magazine_for`]).
    pub def: Arc<machine::MachineDef>,
}

/// Two things of the machine (or one and the work) run into one another from `from` to `to`
/// (minutes of the run).
#[derive(Debug, Clone, PartialEq)]
pub struct MachineHit {
    pub from: f64,
    pub to: f64,
    /// Their names: "Swivel head", "Vise".
    pub a: String,
    pub b: String,
    /// The machine's instances among them (drawn red meanwhile).
    pub instances: Vec<InstanceId>,
    /// The sample (of those given) it starts at.
    pub sample: usize,
}

/// One position of the machine in a run: when, and its axis values and magazine.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub t: f64,
    pub q: Vec<f64>,
    pub tool_number: u32,
    /// The tool in the spindle then, among the [`ToolShape`]s given (`None`: none of them).
    pub tool: Option<usize>,
}

/// Something standing on the table for the collision check, in the model's coordinates.
#[derive(Debug, Clone)]
pub struct Fixed {
    pub name: String,
    pub tris: Vec<[P3; 3]>,
    /// Workholding (a vise, a chuck) and not the part: the tool assembly is checked against
    /// it (against the part and the stock the simulation checks the tool).
    pub workholding: bool,
}

/// A tool assembly for the collision check: its named pieces (the tool with its shank, and
/// its holder with its arbor), each a surface with the tip at the origin and the axis +Z, and
/// how far up the axis the spindle's gauge line is.
#[derive(Debug, Clone)]
pub struct ToolShape {
    pub pieces: Vec<(String, Vec<[P3; 3]>)>,
    pub gauge: f64,
}

impl ToolShape {
    /// The shape of `tool` ([`cutter_mesh`]): the cutter and shank are `name`, the holder and
    /// arbor `holder_name`. A beam has none.
    pub fn of(tool: &Tool, name: &str, holder_name: &str) -> Option<ToolShape> {
        if tool.is_beam() {
            return None;
        }
        let mesh = cutter_mesh(tool, 24);
        let at = |i: u32| {
            let p = mesh.positions[i as usize];
            [p[0] as f64, p[1] as f64, p[2] as f64]
        };
        let (mut cutter, mut holder) = (Vec::new(), Vec::new());
        for t in mesh.indices.as_chunks::<3>().0 {
            let color = mesh.colors[t[0] as usize];
            let tri = [at(t[0]), at(t[1]), at(t[2])];
            if color == HOLDER_COLOR || color == ARBOR_COLOR {
                holder.push(tri);
            } else {
                cutter.push(tri);
            }
        }
        let pieces = [(name, cutter), (holder_name, holder)]
            .into_iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(n, t)| (n.to_string(), t))
            .collect();
        Some(ToolShape {
            pieces,
            gauge: tool.gauge_length(),
        })
    }
}

/// Where a tool's own coordinates (tip at the origin, axis +Z) are in the assembly's, the
/// tool in the spindle: its gauge line `gauge` up its axis on the tool mount, whose Z points
/// out of the spindle, down the tool.
fn tool_xf(mount: &ConnectorFrame, gauge: f64) -> Xf {
    let (o, z) = (mount.origin, mount.z);
    let tip = [
        o[0] + z[0] * gauge,
        o[1] + z[1] * gauge,
        o[2] + z[2] * gauge,
    ];
    Xf::of_frame(&Frame::from_zx(tip, [-z[0], -z[1], -z[2]], mount.x))
}

/// Somewhere nothing is: where a tool that is not in the spindle is kept.
const AWAY: Xf = Xf {
    r: Xf::IDENTITY.r,
    t: [1.0e9, 1.0e9, 1.0e9],
};

/// A solid's surface as triangles.
pub(crate) fn triangles(s: &Solid) -> Vec<[P3; 3]> {
    s.indices
        .chunks(3)
        .filter_map(|t| match t {
            [a, b, c] => Some([
                s.positions[*a as usize],
                s.positions[*b as usize],
                s.positions[*c as usize],
            ]),
            _ => None,
        })
        .collect()
}

/// How an instance of the machine moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moves {
    /// With one of the mounts alone.
    With(machine::Side),
    /// With both (a carriage under the tool and the table).
    Both,
    /// With neither: the base, the column, and what only an actuator moves.
    Still,
}

/// How each instance of the machine moves, and the body of the tree it belongs to.
fn sides(r: &Resolved) -> HashMap<InstanceId, (Moves, usize)> {
    let tree = &r.tree;
    tree.instances
        .iter()
        .map(|i| {
            let roles: Vec<Role> = tree
                .path(i.body)
                .into_iter()
                .map(|j| tree.joints[j].role)
                .collect();
            let moves = if roles.contains(&Role::Both) {
                Moves::Both
            } else if roles.contains(&Role::Tool) {
                Moves::With(machine::Side::Tool)
            } else if roles.contains(&Role::Table) {
                Moves::With(machine::Side::Table)
            } else {
                Moves::Still
            };
            (i.id, (moves, i.body))
        })
        .collect()
}

/// What carries a thing of the collision scene.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Carrier {
    Instance(InstanceId),
    /// The table: the workholding and the part.
    Work,
    /// The spindle, while tool `0` of those given is in it.
    Tool(usize),
}

/// Steps the machine through `samples` and finds where it runs into itself or the work:
///
/// - what moves with the tool against what moves with the table, the workholding and the
///   part;
/// - what moves with the tool against what stands still (the base, the column), but its
///   neighbour on a joint (a slide touches its ways);
/// - the tool in the spindle (`tools`: cutter, shank, holder and arbor) against what moves
///   with the table and the workholding. Not against the part: the simulation of the stock
///   covers that.
///
/// `setup` and `work` place the machine round the work ([`machine_in_model`]); `fixed` is
/// what stands on the table, in the model's coordinates. Pairs of the machine's own that touch
/// with every axis at home are forgiven (parts modelled touching); a tool is never forgiven.
/// Hits of one pair at samples in a row are one [`MachineHit`].
pub fn machine_collisions(
    scene: &MachineScene,
    setup: &Frame,
    work: &Frame,
    samples: &[Sample],
    fixed: &[Fixed],
    tools: &[ToolShape],
) -> Vec<MachineHit> {
    let r = &*scene.resolved;
    let side = sides(r);
    let tool_side = Moves::With(machine::Side::Tool);
    let table_side = Moves::With(machine::Side::Table);
    let mut s = Scene::default();
    // The machine's instances first (an item each, moved by its own placement), then the
    // work's things, all moved by the work's placement on the machine, then the tools' pieces.
    let mut carrier: Vec<Carrier> = Vec::new();
    for p in &scene.parts {
        s.add(&p.name, triangles(&p.solid), carrier.len());
        carrier.push(Carrier::Instance(p.instance));
    }
    let machine_items = carrier.len();
    for f in fixed {
        s.add(&f.name, f.tris.clone(), carrier.len());
        carrier.push(Carrier::Work);
    }
    for (k, tool) in tools.iter().enumerate() {
        for (name, tris) in &tool.pieces {
            s.add(name, tris.clone(), carrier.len());
            carrier.push(Carrier::Tool(k));
        }
    }
    let moves_of = |c: &Carrier| match c {
        Carrier::Instance(i) => side.get(i).copied(),
        _ => None,
    };
    for a in 0..carrier.len() {
        match carrier[a] {
            Carrier::Instance(_) => {
                let Some((moves, body)) = moves_of(&carrier[a]) else {
                    continue;
                };
                if moves != tool_side {
                    continue;
                }
                for (b, other) in carrier.iter().enumerate() {
                    let against = match other {
                        Carrier::Instance(_) => match moves_of(other) {
                            Some((m, _)) if m == table_side => true,
                            // What stands still, but the body this one slides or turns on.
                            Some((Moves::Still, still)) => {
                                r.tree.bodies[body].parent != Some(still)
                                    && r.tree.bodies[still].parent != Some(body)
                            }
                            _ => false,
                        },
                        Carrier::Work => true,
                        Carrier::Tool(_) => false,
                    };
                    if against {
                        s.pairs.push((a, b));
                    }
                }
            }
            Carrier::Tool(_) => {
                for (b, other) in carrier.iter().enumerate() {
                    let against = match other {
                        Carrier::Instance(_) => {
                            moves_of(other).is_some_and(|(m, _)| m == table_side)
                        }
                        Carrier::Work => fixed[b - machine_items].workholding,
                        Carrier::Tool(_) => false,
                    };
                    if against {
                        s.pairs.push((a, b));
                    }
                }
            }
            Carrier::Work => {}
        }
    }
    if s.pairs.is_empty() {
        return Vec::new();
    }
    let mut poses: Vec<(InstanceId, Pose)> = Vec::new();
    let motions_at =
        |q: &[f64], tool_number: u32, tool: Option<usize>, poses: &mut Vec<(InstanceId, Pose)>| {
            let actuators: Vec<(ActuatorId, f64)> =
                magazine_for(&scene.def, tool_number).into_iter().collect();
            r.poses_into(q, &actuators, poses);
            let by: HashMap<InstanceId, &Pose> = poses.iter().map(|(i, p)| (*i, p)).collect();
            let (tool_mount, table) = r.mounts(q);
            // The work in the assembly's coordinates: the model's seen from the machine.
            let work_xf = table
                .map(|t| xf_of(&machine_in_model(setup, work, &t).inverse()))
                .unwrap_or(Xf::IDENTITY);
            carrier
                .iter()
                .map(|c| match c {
                    Carrier::Instance(i) => by.get(i).map_or(Xf::IDENTITY, |p| xf_of(p)),
                    Carrier::Work => work_xf,
                    Carrier::Tool(k) => match &tool_mount {
                        Some(m) if tool == Some(*k) => tool_xf(m, tools[*k].gauge),
                        _ => AWAY,
                    },
                })
                .collect::<Vec<Xf>>()
        };
    // (No tool in the spindle at home: only the machine's own pairs are forgiven.)
    let home = motions_at(&r.home(), 0, None, &mut poses);
    s.forgive(&home);
    // The open hit of each pair: where in `out` it is.
    let mut open: HashMap<(usize, usize), usize> = HashMap::new();
    let mut out: Vec<MachineHit> = Vec::new();
    for (k, sample) in samples.iter().enumerate() {
        let motions = motions_at(&sample.q, sample.tool_number, sample.tool, &mut poses);
        let hits = s.check(&motions);
        let now: Vec<(usize, usize)> = hits.iter().map(|h| (h.a, h.b)).collect();
        open.retain(|pair, _| now.contains(pair));
        for pair in now {
            match open.get(&pair) {
                Some(at) => out[*at].to = sample.t,
                None => {
                    open.insert(pair, out.len());
                    out.push(MachineHit {
                        from: sample.t,
                        to: sample.t,
                        a: s.items[pair.0].name.clone(),
                        b: s.items[pair.1].name.clone(),
                        instances: [pair.0, pair.1]
                            .iter()
                            .filter_map(|i| match carrier[*i] {
                                Carrier::Instance(id) => Some(id),
                                _ => None,
                            })
                            .collect(),
                        sample: k,
                    });
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The view

/// What the loaded machine was built from: the studio, the job, the assembly, the assembly's
/// machine key and the rebuilds of its Part Studios.
type Key = (ElementId, CamId, ElementId, u64, Vec<usize>);

/// An instance of the machine in the view.
struct Shown {
    instance: InstanceId,
    mesh: Entity,
    edges: Entity,
    plain: Handle<Mesh>,
    /// Its mesh in the collision red.
    red: Handle<Mesh>,
    /// Where it is drawn now (its own coordinates to the model's).
    at: Mat4,
}

struct Loaded {
    key: Key,
    scene: MachineScene,
    shown: Vec<Shown>,
}

/// The machine of the active job in the CAM view.
#[derive(Resource, Default)]
pub struct MachineView {
    loaded: Option<Loaded>,
    /// Its Part Studios are still being rebuilt.
    pending: bool,
    /// What the transforms were last set for.
    last: Option<(Vec<f64>, Option<f64>, Pose, bool)>,
    /// The instances drawn red.
    red: Vec<InstanceId>,
    poses: Vec<(InstanceId, Pose)>,
    /// How far the work is moved in the view this frame ([`work_moved`], [`place_work`]).
    work: Pose,
}

impl MachineView {
    /// How far the work is moved in the view this frame: the identity but while a preview
    /// turns it with a machine kept still ([`Relative::Machine`]).
    pub fn work(&self) -> Pose {
        self.work
    }

    /// [`Self::work`] as a matrix.
    pub fn work_matrix(&self) -> Mat4 {
        mat4(&self.work)
    }

    /// A frame in the model's coordinates where the view draws it ([`Self::work`]).
    pub fn work_frame(&self, f: &Frame) -> Frame {
        let w = &self.work;
        Frame {
            origin: w.apply(f.origin),
            x: w.rotate(f.x),
            y: w.rotate(f.y),
            z: w.rotate(f.z),
        }
    }

    /// The machine for a run's collision check, once its parts are built.
    pub fn scene(&self) -> Option<MachineScene> {
        self.loaded.as_ref().map(|l| l.scene.clone())
    }

    /// Whether the active job's machine is drawn (an assembly's, its parts built).
    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    /// The axis values the machine is shown at.
    pub fn shown_axes(&self) -> Option<&[f64]> {
        self.last.as_ref().map(|l| l.0.as_slice())
    }

    /// Points of the machine where it is drawn (the corners of each instance's box), and its
    /// own Z in the model: what a view of the whole machine is framed by.
    fn extent(&self) -> Option<(Vec<Vec3>, Pose)> {
        let loaded = self.loaded.as_ref()?;
        let to_model = self.last.as_ref()?.2;
        let mut points = Vec::new();
        for (s, part) in loaded.shown.iter().zip(&loaded.scene.parts) {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for p in &part.solid.positions {
                let p = super::view::v3(*p);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            if lo.x > hi.x {
                continue;
            }
            for k in 0..8 {
                let corner = Vec3::new(
                    if k & 1 == 0 { lo.x } else { hi.x },
                    if k & 2 == 0 { lo.y } else { hi.y },
                    if k & 4 == 0 { lo.z } else { hi.z },
                );
                points.push(s.at.transform_point3(corner));
            }
        }
        Some((points, to_model))
    }
}

/// Turns the view to look at the whole machine with the machine's own Z up on the screen
/// (the work may lie any way round on it, and the view is the work's), from `azimuth` round
/// the machine's Z and `elevation` over its table (degrees, as the view's own angles are for
/// the model), and frames the machine. For scripted steps: `cam machine view <az> <el>`.
pub fn view_machine(world: &mut World, azimuth: f32, elevation: f32) {
    let Some((points, to_model)) = world.resource::<MachineView>().extent() else {
        warn!("cam machine view: no machine is shown");
        return;
    };
    let dir = |v: [f64; 3]| super::view::v3(to_model.rotate(v)).normalize_or_zero();
    let (az, el) = (azimuth.to_radians() as f64, elevation.to_radians() as f64);
    // Toward the camera, and up, as the machine has them.
    let back = dir([az.sin() * el.cos(), -az.cos() * el.cos(), el.sin()]);
    let up = dir([0.0, 0.0, 1.0]);
    let size = world.resource::<crate::viewport::ViewportRect>().0.size();
    let mut view = world.resource_mut::<crate::viewport::ViewportView>();
    let mut to = view.target();
    to.azimuth = back.x.atan2(-back.y).to_degrees();
    to.elevation = back.z.clamp(-1.0, 1.0).asin().to_degrees();
    to.roll = 0.0;
    // The roll that brings the machine's Z up the screen: the angle from the level view's up
    // to it about the view direction, whichever way the view counts it.
    let want = (up - back * up.dot(back)).normalize_or_zero();
    let level = to.up();
    let turn = level
        .cross(want)
        .dot(back)
        .atan2(level.dot(want))
        .to_degrees();
    let rolled = |roll: f32| crate::camera::ViewState { roll, ..to };
    to = if rolled(turn).up().dot(want) >= rolled(-turn).up().dot(want) {
        rolled(turn)
    } else {
        rolled(-turn)
    };
    let to = to.fitted(&points, size, 0.8);
    view.animate_to(to);
}

/// A mesh or the edges of the machine in the view.
#[derive(Component)]
pub struct MachineMesh;

/// A thing of the work drawn in the model's coordinates (the stock, a toolpath): its
/// `Transform` is [`MachineView::work`], set by [`place_on_work`].
#[derive(Component)]
pub struct OnWork;

/// Works out how far the work is moved this frame ([`MachineView::work`]) and draws the
/// studio's parts there ([`crate::parts::PartCache::moved`], which also moves their edges).
/// Only while a preview moves a machine from an assembly that the view shows, kept still
/// ([`Relative::Machine`]), and no dialog is open: the parts are picked where their solids
/// are, and a dialog's picks would miss them moved.
#[allow(clippy::too_many_arguments)]
pub(in crate::cam) fn place_work(
    doc: Option<Res<ActiveDocument>>,
    mut view: ResMut<MachineView>,
    ui: Res<CamUi>,
    preview: Option<Res<super::preview::PreviewState>>,
    dialog: Option<Res<super::dialogs::CamDialog>>,
    mut cache: ResMut<crate::parts::PartCache>,
) {
    let in_cam = doc.as_ref().is_some_and(|d| active_cam(d).is_some());
    let w = in_cam
        .then(|| {
            if ui.relative != Relative::Machine || !ui.show_machine || dialog.is_some() {
                return None;
            }
            let loaded = view.loaded.as_ref()?;
            let now = preview.as_ref()?.machine_now()?;
            let r = &*loaded.scene.resolved;
            let (_, Some(home)) = r.mounts(&r.home()) else {
                return None;
            };
            let (_, Some(table)) = r.mounts(&now.q) else {
                return None;
            };
            Some(work_moved(&now.setup, &now.work, &home, &table))
        })
        .flatten()
        .unwrap_or(Pose::IDENTITY);
    if view.work != w {
        view.work = w;
    }
    // (Outside a CAM Studio the parts' own placements are another tab's: left alone.)
    if !in_cam {
        return;
    }
    let want: HashMap<cadrs_core::PartId, Mat4> = if w == Pose::IDENTITY {
        HashMap::new()
    } else {
        let m = mat4(&w);
        cache.parts.iter().map(|p| (p.id, m)).collect()
    };
    if cache.moved != want {
        cache.moved = want;
    }
}

/// Puts the things of the work drawn in the model's coordinates ([`OnWork`]) where the work is
/// drawn this frame.
fn place_on_work(view: Res<MachineView>, mut q: Query<&mut Transform, With<OnWork>>) {
    let at = Transform::from_matrix(view.work_matrix());
    for mut tf in &mut q {
        tf.set_if_neq(at);
    }
}

fn mat4(p: &Pose) -> Mat4 {
    let col = |v: [f64; 3]| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
    Mat4::from_cols(
        col(p.rotate([1.0, 0.0, 0.0])).extend(0.0),
        col(p.rotate([0.0, 1.0, 0.0])).extend(0.0),
        col(p.rotate([0.0, 0.0, 1.0])).extend(0.0),
        col(p.translation).extend(1.0),
    )
}

fn despawn(view: &mut MachineView, commands: &mut Commands) {
    if let Some(l) = view.loaded.take() {
        for s in l.shown {
            commands.entity(s.mesh).try_despawn();
            commands.entity(s.edges).try_despawn();
        }
    }
    view.last = None;
    view.red.clear();
}

/// The active job's machine assembly: the studio, the job and the assembly's element.
fn wanted(doc: &ActiveDocument) -> Option<(ElementId, CamId, ElementId)> {
    let (el, cam) = active_cam(doc)?;
    let job = cam.active()?;
    let src = job.machine_source.as_ref()?;
    job.machine.as_ref()?;
    Some((el, job.id, src.element()))
}

/// Builds the machine's meshes when the active job has a machine from an assembly, and again
/// when the assembly or its parts change; takes them away otherwise.
#[allow(clippy::too_many_arguments)]
fn load_machine(
    doc: Option<Res<ActiveDocument>>,
    mut view: ResMut<MachineView>,
    mut asm_parts: ResMut<crate::assembly::AssemblyParts>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PartShading>>,
    mut gizmos: ResMut<Assets<GizmoAsset>>,
    mut mats: Local<Option<(Handle<PartShading>, Handle<PartShading>)>>,
    mut pending: ResMut<cadrs_ui::PendingWork>,
    mut why: ResMut<cadrs_ui::PendingWhy>,
    mut commands: Commands,
) {
    let Some((doc, (cam_el, job, element))) = doc.and_then(|d| {
        let w = wanted(&d)?;
        Some((d, w))
    }) else {
        if view.loaded.is_some() || view.pending {
            despawn(&mut view, &mut commands);
            view.pending = false;
        }
        return;
    };
    // Looked at again only when something may have changed.
    let same_job = view
        .loaded
        .as_ref()
        .is_some_and(|l| (l.key.0, l.key.1, l.key.2) == (cam_el, job, element));
    if same_job && !doc.is_changed() && !view.pending {
        return;
    }
    let d = &doc.doc;
    let Some(asm) = d.element(element).and_then(|e| e.assembly_model()) else {
        despawn(&mut view, &mut commands);
        return;
    };
    let machine_key = cadrs_core::cam::machine_key(d, element);
    let occ = cadrs_core::assembly::structure::occurrences(d, asm);
    // The rebuilds of its Part Studios, a few milliseconds a frame until they are in.
    let mut sources: Vec<ElementId> = occ.iter().map(|o| o.element).collect();
    sources.sort();
    sources.dedup();
    let mut builds = HashMap::new();
    let mut waiting = false;
    for s in &sources {
        let (b, current) = asm_parts.build_within(d, *s, Some(std::time::Duration::from_millis(4)));
        waiting |= !current;
        if let Some(b) = b {
            builds.insert(*s, b);
        }
    }
    view.pending = waiting;
    if waiting {
        pending.0 = true;
        why.add("cam machine parts");
        return;
    }
    let key: Key = (
        cam_el,
        job,
        element,
        machine_key,
        sources
            .iter()
            .map(|s| builds.get(s).map_or(0, |b| Arc::as_ptr(b) as usize))
            .collect(),
    );
    if view.loaded.as_ref().is_some_and(|l| l.key == key) {
        return;
    }
    despawn(&mut view, &mut commands);
    let Ok(resolved) = machine::resolve(d, element, &machine::stored_frames) else {
        return;
    };
    let Some(def) = asm.machine.as_deref().cloned() else {
        return;
    };
    let (opaque, glass) = mats
        .get_or_insert_with(|| {
            (
                materials.add(PartShading::new(false, false)),
                materials.add(PartShading::new(true, true)),
            )
        })
        .clone();
    let mut scene_parts = Vec::new();
    let mut shown = Vec::new();
    for o in occ.iter().filter(|o| !o.hidden) {
        let Some(part) = builds.get(&o.element).and_then(|b| b.part(o.part)) else {
            continue;
        };
        let source = d.element(o.element);
        let props = source.map(|e| e.part_props().to_vec()).unwrap_or_default();
        let appearances = source
            .map(|e| e.feature_appearances().to_vec())
            .unwrap_or_default();
        let name = cadrs_core::parts::display_name(part, &props).to_string();
        let mut bases = crate::parts::face_bases(part, &props, &appearances);
        if let Some(a) = o.appearance {
            bases = vec![FaceBase::of(a); bases.len()];
        }
        // The enclosure (by the instance, or the subassembly instance it is in): see-through.
        let see_through = def.enclosure.contains(&o.id) || def.enclosure.contains(&o.top);
        if see_through {
            for b in &mut bases {
                b.alpha = b.alpha.min(0.22);
            }
        }
        let red: Vec<FaceBase> = bases
            .iter()
            .map(|b| FaceBase {
                rgb: [232.0, 64.0, 52.0],
                ..*b
            })
            .collect();
        let plain = meshes.add(crate::parts::part_base_mesh(part, see_through, &bases));
        let red = meshes.add(crate::parts::part_base_mesh(part, see_through, &red));
        let slug = name.to_lowercase().replace(' ', "-");
        let mesh = commands
            .spawn((
                Name::new(format!("cam-machine-{slug}")),
                MachineMesh,
                Mesh3d(plain.clone()),
                MeshMaterial3d(if see_through {
                    glass.clone()
                } else {
                    opaque.clone()
                }),
                Transform::IDENTITY,
                Visibility::Hidden,
                crate::parts::part_layers(),
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
            ))
            .id();
        // Its edges, moving with it.
        let mut asset = GizmoAsset::default();
        let edge = if see_through {
            Color::srgba(0.1, 0.1, 0.12, 0.3)
        } else {
            Color::srgba(0.1, 0.1, 0.12, 0.75)
        };
        for e in part.solid.edges.iter().filter(|e| !e.smooth) {
            if e.points.len() >= 2 {
                asset.linestrip(e.points.iter().map(|p| super::view::v3(*p)), edge);
            }
        }
        let edges = commands
            .spawn((
                Name::new(format!("cam-machine-{slug}-edges")),
                MachineMesh,
                Gizmo {
                    handle: gizmos.add(asset),
                    line_config: GizmoLineConfig {
                        width: 1.2,
                        joints: GizmoLineJoint::None,
                        ..default()
                    },
                    depth_bias: -0.0003,
                },
                Transform::from_scale(Vec3::ZERO),
                Visibility::Hidden,
                DespawnOnExit(AppState::Document),
            ))
            .id();
        scene_parts.push(ScenePart {
            instance: o.id,
            name,
            solid: part.solid.clone(),
        });
        shown.push(Shown {
            instance: o.id,
            mesh,
            edges,
            plain,
            red,
            at: Mat4::IDENTITY,
        });
    }
    view.loaded = Some(Loaded {
        key,
        scene: MachineScene {
            resolved: Arc::new(resolved),
            parts: scene_parts,
            def: Arc::new(def),
        },
        shown,
    });
    view.last = None;
}

/// The setup the machine stands under without a preview: the selected setup's (or the
/// selected operation's), else the job's first. Its frame in the model and on the table
/// mount; the model's own when the job has no setup yet.
fn standing(doc: &ActiveDocument, ui: &CamUi, parts: &[cadrs_core::Part]) -> (Frame, Frame) {
    let found = active_cam(doc).and_then(|(_, cam)| {
        let job = cam.active()?;
        let setup = ui
            .selected
            .iter()
            .rev()
            .find_map(|id| {
                job.setups
                    .iter()
                    .find(|s| s.id == *id || s.operations.iter().any(|o| o.id == *id))
            })
            .or_else(|| job.setups.iter().find(|s| !s.suppressed))?;
        let frame = program::setup_frame(cam, job, setup, parts);
        let work = program::stock_shape(cam, job, parts)
            .map(|shape| program::work_on_table(setup, &shape.bounds_in(&frame)))
            .unwrap_or(Frame::WORLD);
        Some((frame, work))
    });
    found.unwrap_or((Frame::WORLD, Frame::WORLD))
}

/// Moves the machine's meshes to where the preview (or home) has it, shows or hides them and
/// reddens what is colliding now.
#[allow(clippy::too_many_arguments)]
fn move_machine(
    doc: Option<Res<ActiveDocument>>,
    view: ResMut<MachineView>,
    ui: Res<CamUi>,
    preview: Option<Res<super::preview::PreviewState>>,
    cache: Res<crate::parts::PartCache>,
    mut q: Query<(&mut Transform, &mut Visibility), With<MachineMesh>>,
    mut q_mesh: Query<&mut Mesh3d, With<MachineMesh>>,
    mut standing_at: Local<Option<(Frame, Frame)>>,
) {
    let view = view.into_inner();
    let (Some(doc), Some(loaded)) = (doc, view.loaded.as_mut()) else {
        *standing_at = None;
        return;
    };
    let r = &*loaded.scene.resolved;
    let show = ui.show_machine;
    // Where it is: the preview's moment, else home under a setup.
    let (q_now, tool_number, setup, work, t) = match preview.as_ref().and_then(|p| p.machine_now())
    {
        Some(now) => {
            *standing_at = None;
            (
                now.q,
                Some(now.tool_number),
                now.setup,
                now.work,
                Some(now.t),
            )
        }
        None => {
            // (Worked out when the studio or the selection changes, not every frame.)
            if standing_at.is_none() || doc.is_changed() || ui.is_changed() || cache.is_changed() {
                *standing_at = Some(standing(&doc, &ui, &cache.parts));
            }
            let (setup, work) = standing_at.unwrap_or((Frame::WORLD, Frame::WORLD));
            (r.home(), None, setup, work, None)
        }
    };
    let magazine = tool_number.and_then(|n| magazine_for(&loaded.scene.def, n));
    let (_, Some(table)) = r.mounts(&q_now) else {
        return;
    };
    // The machine's coordinates where the view draws them: moved with the work by
    // [`MachineView::work`], which keeps the base still when the view is the machine's.
    let to_model = machine_in_model(&setup, &work, &table).then(&view.work);
    let state = (q_now, magazine.map(|m| m.1), to_model, show);
    if view.last.as_ref() != Some(&state) {
        let actuators: Vec<(ActuatorId, f64)> = magazine.into_iter().collect();
        r.poses_into(&state.0, &actuators, &mut view.poses);
        let by: HashMap<InstanceId, &Pose> = view.poses.iter().map(|(i, p)| (*i, p)).collect();
        let visible = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for s in &mut loaded.shown {
            let Some(p) = by.get(&s.instance) else {
                continue;
            };
            s.at = mat4(&p.then(&to_model));
            let at = Transform::from_matrix(s.at);
            // (Retained gizmos are drawn whatever their visibility: hidden edges shrink to
            // nothing instead.)
            let edges_at = if show {
                at
            } else {
                Transform::from_scale(Vec3::ZERO)
            };
            for (e, at) in [(s.mesh, at), (s.edges, edges_at)] {
                if let Ok((mut tf, mut vis)) = q.get_mut(e) {
                    if *tf != at {
                        *tf = at;
                    }
                    if *vis != visible {
                        *vis = visible;
                    }
                }
            }
        }
        view.last = Some(state);
    }
    // What is colliding at the preview's time.
    let red: Vec<InstanceId> = match (t, preview.as_ref()) {
        (Some(t), Some(p)) => {
            let mut v: Vec<InstanceId> = p
                .machine_hits()
                .iter()
                .filter(|h| t >= h.from - 1e-9 && t <= h.to + 1e-9)
                .flat_map(|h| h.instances.iter().copied())
                .collect();
            v.sort();
            v.dedup();
            v
        }
        _ => Vec::new(),
    };
    if red != view.red {
        for s in &loaded.shown {
            if let Ok(mut m) = q_mesh.get_mut(s.mesh) {
                let want = if red.contains(&s.instance) {
                    &s.red
                } else {
                    &s.plain
                };
                if m.0 != *want {
                    m.0 = want.clone();
                }
            }
        }
        view.red = red;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cadrs_core::cam::cadrs_cam::geom::{add, dist, scale};
    use cadrs_core::samples::cnc_machines as cnc;

    fn near(a: P3, b: P3) -> bool {
        dist(a, b) < 1e-6
    }

    /// A frame placed by hand: the instance goes where each step of the chain says.
    #[test]
    fn an_instance_is_placed_through_setup_work_and_mount() {
        // The table mount 100 up the assembly's Z, turned a quarter about Z.
        let mount = ConnectorFrame::new([0.0, 0.0, 100.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        // The setup's origin 10 over the mount, axes as the mount's.
        let work = Frame {
            origin: [0.0, 0.0, 10.0],
            ..Frame::WORLD
        };
        // The setup in the model: at (50, 0, 0), axes as the model's.
        let setup = Frame {
            origin: [50.0, 0.0, 0.0],
            ..Frame::WORLD
        };
        let to_model = machine_in_model(&setup, &work, &mount);
        // The mount's origin is 10 under the setup's origin, in the model.
        assert!(near(to_model.apply([0.0, 0.0, 100.0]), [50.0, 0.0, -10.0]));
        // The assembly's +Y is the mount's X: the setup's, and so the model's, X.
        assert!(near(to_model.rotate([0.0, 1.0, 0.0]), [1.0, 0.0, 0.0]));
        // An instance 20 along the assembly's Y from the mount, lifted 5.
        let instance = Pose::translation([0.0, 20.0, 105.0]);
        let at = place(&setup, &work, &mount, &instance);
        assert!(near(at.apply([0.0; 3]), [70.0, 0.0, -5.0]));
        // With everything the identity, an instance stays where the assembly has it.
        let id = ConnectorFrame::default();
        assert_eq!(
            place(&Frame::WORLD, &Frame::WORLD, &id, &instance),
            instance
        );
    }

    /// Two placements alike: they take the same points to the same places.
    fn pose_near(a: &Pose, b: &Pose) -> bool {
        [
            [0.0; 3],
            [100.0, 0.0, 0.0],
            [0.0, 100.0, 0.0],
            [0.0, 0.0, 100.0],
        ]
        .iter()
        .all(|p| dist(a.apply(*p), b.apply(*p)) < 1e-6)
    }

    /// On the 4-axis mill under the "CAM 4-axis" Worm job's setup, A turned 90°: kept still
    /// (machine relative), the base is drawn where it stands at home and the work turns a
    /// quarter about the chuck's axis with the chuck; part relative the work stays and the
    /// machine moves round it, the base too. At home nothing moves.
    #[test]
    fn keeping_the_machine_still_turns_the_work_with_the_chuck() {
        use cnc::assemblies::mill4 as m;
        use cnc::cam::cam4;
        let (doc, r) = resolved(cnc::MILL_4);
        let cam = doc.element(cam4::STUDIO).unwrap().cam().unwrap();
        let parts = cadrs_core::rebuild::build(cam.part_features())
            .parts
            .clone();
        let (job, setup) = cam.setup(cam4::WORM_SETUP).unwrap();
        let frame = program::setup_frame(cam, job, setup, &parts);
        let shape = program::stock_shape(cam, job, &parts).unwrap();
        let work = program::work_on_table(setup, &shape.bounds_in(&frame));
        let home = r.home();
        let a = r.axes.iter().position(|x| x.name == "A").unwrap();
        let mut q = home.clone();
        q[a] += 90.0;
        let (_, Some(home_table)) = r.mounts(&home) else {
            panic!("the table mount resolves");
        };
        let (_, Some(table)) = r.mounts(&q) else {
            panic!("the table mount resolves");
        };
        // At home the work is not moved.
        let still = work_moved(&frame, &work, &home_table, &home_table);
        assert!(pose_near(&still, &Pose::IDENTITY), "{still:?}");
        let w = work_moved(&frame, &work, &home_table, &table);
        let poses = |q: &[f64]| {
            let mut v = Vec::new();
            r.poses_into(q, &[], &mut v);
            v.into_iter().collect::<HashMap<InstanceId, Pose>>()
        };
        let (at_home, at_q) = (poses(&home), poses(&q));
        let to_home = machine_in_model(&frame, &work, &home_table);
        let to_now = machine_in_model(&frame, &work, &table);
        // Machine relative: an instance at `P(q)` then the part-relative placement then W.
        let drawn = |i: InstanceId| at_q[&i].then(&to_now).then(&w);
        let base_home = at_home[&m::BASE].then(&to_home);
        assert!(pose_near(&drawn(m::BASE), &base_home));
        assert!(pose_near(
            &drawn(m::COLUMN),
            &at_home[&m::COLUMN].then(&to_home)
        ));
        // The chuck turned with A, the work with it: where the chuck is drawn now is where it
        // was at home moved as the work is.
        let chuck_home = at_home[&m::CHUCK].then(&to_home);
        assert!(pose_near(&drawn(m::CHUCK), &chuck_home.then(&w)));
        assert!(!pose_near(&drawn(m::CHUCK), &chuck_home));
        // The work turns a quarter about the chuck's axis: two points on it stay, and W turns
        // by 90° (its rotation's trace is 1 + 2 cos θ).
        let axis = cnc::parts::CHUCK_AXIS;
        for along in [0.0, 150.0] {
            let p = chuck_home.apply([axis[0] + along, axis[1], axis[2]]);
            assert!(near(w.apply(p), p), "{p:?} → {:?}", w.apply(p));
        }
        let rot = w.rotation;
        let cos = (rot[0][0] + rot[1][1] + rot[2][2] - 1.0) / 2.0;
        assert!(cos.abs() < 1e-6, "{cos}");
        // Part relative (no W): the work stays and the machine moves round it, the base too.
        let base_now = at_q[&m::BASE].then(&to_now);
        assert!(!pose_near(&base_now, &base_home));
    }

    pub(crate) fn resolved(element: ElementId) -> (cadrs_core::Document, Resolved) {
        let doc = cnc::document().unwrap();
        let r = machine::resolve(&doc, element, &machine::stored_frames).unwrap();
        (doc, r)
    }

    /// On the sample's 5-axis mill: for a cutter location in a setup, the axis values the
    /// chain solves put the spindle's nose, drawn in the model, at the tool's gauge point,
    /// looking down the tool.
    #[test]
    fn the_spindle_is_drawn_on_the_tool() {
        let (_, r) = resolved(cnc::MILL_5);
        let chain = r.chain().clone();
        let setup = Frame::from_zx([30.0, -20.0, 15.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let work = Frame {
            origin: [0.0, 0.0, 40.0],
            ..Frame::WORLD
        };
        let length = 80.0;
        for (tip, axis) in [
            ([0.0, 0.0, 30.0], [0.0, 0.0, 1.0]),
            ([10.0, 5.0, 25.0], [0.3, 0.2, 0.9]),
            ([-12.0, 8.0, 20.0], [-0.5, 0.1, 0.8]),
        ] {
            let cl = Cl::new(tip, axis);
            let q = chain
                .solve(&cl.out_of(&work), length, &chain.rest())
                .unwrap();
            let (Some(tool), Some(table)) = r.mounts(&q) else {
                panic!("the mounts resolve");
            };
            let to_model = machine_in_model(&setup, &work, &table);
            let gauge = setup.to_world(add(cl.tip, scale(cl.axis, length)));
            assert!(near(to_model.apply(tool.origin), gauge), "{q:?}");
            // The tool mount's Z points out of the spindle: down the tool.
            let down = scale(setup.dir_to_world(cl.axis), -1.0);
            assert!(near(to_model.rotate(tool.z), down), "{q:?}");
            // The table mount is where the setup says the mount is, whatever the axes do.
            assert!(near(
                to_model.apply(table.origin),
                setup.to_world(work.to_local([0.0; 3]))
            ));
        }
    }

    /// Between two rows of a track the machine is solved to the cutter's place at that
    /// moment; at a row it is the row.
    #[test]
    fn axes_between_two_moves_follow_the_cutter() {
        let (_, r) = resolved(cnc::MILL_5);
        let chain = r.chain().clone();
        let work = Frame {
            origin: [0.0, 0.0, 40.0],
            ..Frame::WORLD
        };
        let length = 80.0;
        let a = Cl::new([0.0, 0.0, 30.0], [0.0, 0.0, 1.0]);
        let b = Cl::new([20.0, 0.0, 30.0], [0.2, 0.0, 1.0]);
        let moves = [(a.out_of(&work), length), (b.out_of(&work), length)];
        let track = chain.track(&moves, &chain.rest());
        assert!(track.issues.is_empty(), "{:?}", track.issues);
        // At the end of move 1: its row.
        let end = axes_at(&chain, &track, &work, 1, b.tip, b.axis, length);
        for (x, y) in end.iter().zip(&track.rows[2]) {
            assert!((x - y).abs() < 1e-6, "{end:?} {:?}", track.rows[2]);
        }
        // Half way: the tool's tip is half way, in the setup's coordinates.
        let mid = a.toward(&b, 0.5);
        let q = axes_at(&chain, &track, &work, 1, mid.tip, mid.axis, length);
        let got = chain.locate(&q, length).seen_from(&work);
        assert!(
            near(got.tip, mid.tip) && near(got.axis, mid.axis),
            "{got:?}"
        );
    }

    /// The 4-axis mill's magazine turns to the pocket of the tool's number.
    #[test]
    fn the_magazine_turns_to_the_tools_pocket() {
        let doc = cnc::document().unwrap();
        let def = doc
            .element(cnc::MILL_4)
            .and_then(|e| e.assembly_model())
            .and_then(|a| a.machine.as_deref().cloned())
            .unwrap();
        let (id, v1) = magazine_for(&def, 1).unwrap();
        let (_, v2) = magazine_for(&def, 2).unwrap();
        assert_eq!(Some(id), def.tool_change.magazine);
        assert!((v2 - v1).abs() > 1.0);
        assert!(magazine_for(&def, 99).is_none());
        let other = doc
            .element(cnc::MILL_5)
            .and_then(|e| e.assembly_model())
            .and_then(|a| a.machine.as_deref().cloned())
            .unwrap();
        assert!(magazine_for(&other, 1).is_none());
    }

    /// The machine's instances as boxes round their placements, for the collision test: the
    /// sample's parts are not built here.
    pub(crate) fn boxed(
        doc: &cadrs_core::Document,
        element: ElementId,
        r: Resolved,
    ) -> MachineScene {
        let asm = doc.element(element).unwrap().assembly_model().unwrap();
        let studio = doc.element(cnc::MACHINE_PARTS).unwrap();
        let build = cadrs_core::rebuild::build(&studio.part_features());
        let parts = cadrs_core::assembly::structure::occurrences(doc, asm)
            .into_iter()
            .filter_map(|o| {
                let p = build.part(o.part)?;
                Some(ScenePart {
                    instance: o.id,
                    name: cadrs_core::parts::display_name(p, studio.part_props()).to_string(),
                    solid: p.solid.clone(),
                })
            })
            .collect();
        MachineScene {
            resolved: Arc::new(r),
            parts,
            def: Arc::new(asm.machine.as_deref().cloned().unwrap()),
        }
    }

    /// The head driven down into a block on the table is found, with when and what; clear of
    /// it nothing is.
    #[test]
    fn the_head_into_a_block_on_the_table_is_a_hit() {
        let (doc, r) = resolved(cnc::MILL_3);
        let scene = boxed(&doc, cnc::MILL_3, r.clone());
        assert!(scene.parts.len() >= 5);
        // A block 100 high on the middle of the table (the model's coordinates are the
        // table mount's here: the setup and the work are the identity).
        let block = cadrs_core::cam::cadrs_cam::model::box_triangles(
            &cadrs_core::cam::cadrs_cam::geom::Aabb::new([-40.0, -40.0, 0.0], [40.0, 40.0, 100.0]),
        );
        let fixed = vec![Fixed {
            name: "Block".to_string(),
            tris: block,
            workholding: true,
        }];
        let z = r.axes.iter().position(|a| a.name == "Z").unwrap();
        let at = |t: f64, v: f64| {
            let mut q = r.home();
            q[z] = v;
            Sample {
                t,
                q,
                tool_number: 1,
                tool: None,
            }
        };
        // The spindle's nose is 400 over the table at home: down 250 it is clear, down 320
        // and 340 it is in the block.
        let samples = [
            at(0.0, 0.0),
            at(1.0, -250.0),
            at(2.0, -320.0),
            at(3.0, -340.0),
            at(4.0, -100.0),
        ];
        let w = Frame::WORLD;
        let hits = machine_collisions(&scene, &w, &w, &samples, &fixed, &[]);
        assert!(!hits.is_empty());
        for h in &hits {
            assert_eq!((h.from, h.to, h.b.as_str()), (2.0, 3.0, "Block"), "{h:?}");
            assert_eq!(h.instances.len(), 1);
        }
        assert!(hits.iter().any(|h| h.a == "Spindle"), "{hits:?}");
        let clear = machine_collisions(&scene, &w, &w, &samples[..2], &fixed, &[]);
        assert!(clear.is_empty(), "{clear:?}");
    }

    /// A 10 mm end mill in a holder, 90 from its tip to the gauge line.
    pub(crate) fn held_mill() -> Tool {
        use cadrs_core::cam::cadrs_cam::tool::{Profile, ToolKind};
        let mut tool = Tool::new(ToolKind::EndMill, 10.0);
        tool.overall_length = 50.0;
        tool.holder = Some(Profile::cylinder("Holder", 40.0, 40.0));
        tool
    }

    /// The 5-axis mill's axis values with the tool's tip at `tip` over (or in) the platter,
    /// upright: the setup's frame is the table mount's.
    pub(crate) fn tip_at(r: &Resolved, tool: &Tool, tip: P3) -> Vec<f64> {
        let chain = r.chain();
        chain
            .solve(
                &Cl::new(tip, [0.0, 0.0, 1.0]),
                tool.gauge_length(),
                &chain.rest(),
            )
            .unwrap()
    }

    /// The tool in the spindle is a thing of the machine: its tip driven into the platter is
    /// a hit of the tool's, its holder brought down on a vise one of the holder's, and
    /// neither is one against the part (the simulation's to find) or with another tool in
    /// the spindle.
    #[test]
    fn the_tool_and_its_holder_are_checked() {
        let (doc, r) = resolved(cnc::MILL_5);
        let scene = boxed(&doc, cnc::MILL_5, r.clone());
        let tool = held_mill();
        let shape = ToolShape::of(&tool, "End mill", "End mill holder").unwrap();
        assert_eq!(shape.pieces.len(), 2);
        assert_eq!(shape.gauge, 90.0);
        // The gauge line is on the tool mount, the tip `gauge` down the mount's Z.
        let q = tip_at(&r, &tool, [50.0, 20.0, 60.0]);
        let (Some(mount), Some(table)) = r.mounts(&q) else {
            panic!("the mounts resolve");
        };
        let xf = tool_xf(&mount, shape.gauge);
        let to_model = machine_in_model(&Frame::WORLD, &Frame::WORLD, &table);
        let tip = to_model.apply(xf.point([0.0; 3]));
        assert!(near(tip, [50.0, 20.0, 60.0]), "{tip:?}");
        assert!(near(xf.point([0.0, 0.0, 90.0]), mount.origin));
        let at = |t: f64, tip: P3, tool_in: Option<usize>| Sample {
            t,
            q: tip_at(&r, &tool, tip),
            tool_number: 1,
            tool: tool_in,
        };
        let w = Frame::WORLD;
        let block = |name: &str, workholding: bool| Fixed {
            name: name.to_string(),
            tris: cadrs_core::cam::cadrs_cam::model::box_triangles(
                &cadrs_core::cam::cadrs_cam::geom::Aabb::new(
                    [-80.0, -30.0, 0.0],
                    [-20.0, 30.0, 70.0],
                ),
            ),
            workholding,
        };
        // Clear over the platter, 5 into it, clear again; then over the block, the tip
        // beside it (x −10) and the holder (radius 20, from 50 up the tool) across its top.
        let samples = [
            at(0.0, [50.0, 20.0, 60.0], Some(0)),
            at(1.0, [50.0, 20.0, -5.0], Some(0)),
            at(2.0, [50.0, 20.0, 60.0], Some(0)),
            at(3.0, [-10.0, 0.0, 15.0], Some(0)),
            at(4.0, [50.0, 20.0, 60.0], Some(0)),
        ];
        let tools = [shape];
        let hits = machine_collisions(&scene, &w, &w, &samples, &[block("Vise", true)], &tools);
        let got: Vec<(f64, f64, &str, &str)> = hits
            .iter()
            .map(|h| (h.from, h.to, h.a.as_str(), h.b.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (1.0, 1.0, "End mill", "Platter"),
                (3.0, 3.0, "End mill holder", "Vise")
            ]
        );
        // (The platter is drawn red; the tool is no instance.)
        assert_eq!(hits[0].instances, [cnc::assemblies::mill5::PLATTER]);
        assert_eq!(hits[0].sample, 1);
        // The same block as the part: the tool's holder against it is not this check's.
        let part = machine_collisions(&scene, &w, &w, &samples, &[block("Part", false)], &tools);
        assert_eq!(part.len(), 1, "{part:?}");
        // No tool in the spindle (or no tools at all): the spindle's nose is 90 over the tip,
        // clear of everything.
        let mut bare = samples.clone();
        for s in &mut bare {
            s.tool = None;
        }
        let none = machine_collisions(&scene, &w, &w, &bare, &[block("Vise", true)], &tools);
        assert!(none.is_empty(), "{none:?}");
    }

    /// What moves with the tool is checked against what stands still, but the body it
    /// slides on: on the 5-axis mill the Z slide runs on the column (neighbours), and the
    /// swivel head, carried by the slide, is paired with the column and the base. Through
    /// the whole of the head's swing and the slide's travel nothing of them meets.
    #[test]
    fn the_head_is_checked_against_the_column_and_the_base() {
        use cnc::assemblies::mill5 as m;
        let (doc, r) = resolved(cnc::MILL_5);
        let side = sides(&r);
        let moves = |i: InstanceId| side[&i].0;
        assert_eq!(moves(m::BASE), Moves::Still);
        assert_eq!(moves(m::COLUMN), Moves::Still);
        assert_eq!(moves(m::Z_SLIDE), Moves::With(machine::Side::Tool));
        assert_eq!(moves(m::SWIVEL_HEAD), Moves::With(machine::Side::Tool));
        assert_eq!(moves(m::PLATTER), Moves::With(machine::Side::Table));
        // The slide's body hangs on the column's; the head's on the slide's.
        let body = |i: InstanceId| side[&i].1;
        assert_eq!(
            r.tree.bodies[body(m::Z_SLIDE)].parent,
            Some(body(m::COLUMN))
        );
        assert_ne!(
            r.tree.bodies[body(m::SWIVEL_HEAD)].parent,
            Some(body(m::COLUMN))
        );
        let scene = boxed(&doc, cnc::MILL_5, r.clone());
        let (z, b) = (
            r.axes.iter().position(|a| a.name == "Z").unwrap(),
            r.axes.iter().position(|a| a.name == "B").unwrap(),
        );
        let mut samples = Vec::new();
        for zv in [r.axes[z].min, r.axes[z].home, r.axes[z].max] {
            for bv in [r.axes[b].min, 0.0, r.axes[b].max] {
                let mut q = r.home();
                q[z] = zv;
                q[b] = bv;
                samples.push(Sample {
                    t: samples.len() as f64,
                    q,
                    tool_number: 1,
                    tool: None,
                });
            }
        }
        let w = Frame::WORLD;
        let hits = machine_collisions(&scene, &w, &w, &samples, &[], &[]);
        let still: Vec<&MachineHit> = hits
            .iter()
            .filter(|h| h.b == "Column" || h.b == "Base")
            .collect();
        assert!(still.is_empty(), "{still:?}");
    }
}
