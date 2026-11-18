//! The machine definition of an assembly (#278, `cadrs_core::assembly::machine`): the axes
//! detected on sample machines, the tree's forward kinematics against the assembly solver,
//! the chain handed to CAM against both, what cannot be a machine, and the commands.

use cadrs_cam::kinematics::{Chain, JointKind, Side};
use cadrs_core::assembly::commands::{DeleteInstances, DeleteMateFeatures};
use cadrs_core::assembly::connector::{
    ConnectorAnchor, ConnectorFrame, EntityRef, ImplicitPoint, MateConnector,
};
use cadrs_core::assembly::machine::commands::SetMachineDef;
use cadrs_core::assembly::machine::derive::{Link, Motion, Role};
use cadrs_core::assembly::machine::detect::UNLIMITED;
use cadrs_core::assembly::machine::samples::{self, Builder, Position, Sample, frame};
use cadrs_core::assembly::machine::{
    Actuator, ActuatorId, Issue, MachineAxis, MachineDef, Pocket, Resolved, Severity, ToolChange,
    derive, detect, flat_model, resolve, resolve_model, stored_frames,
};
use cadrs_core::assembly::mate::{Dof, MateFeature, MateId, MateKind, MateLimits, MateType};
use cadrs_core::assembly::relation::{Relation, RelationType};
use cadrs_core::assembly::solver::{self, Drive, SolveOptions};
use cadrs_core::assembly::structure::derive as occurrence;
use cadrs_core::assembly::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::document::{Document, Element};
use cadrs_core::solid::{FaceName, FaceOrigin};
use cadrs_core::{Command, ElementId, History};

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];
const DOWN: [f64; 3] = [0.0, 0.0, -1.0];

/// Our placements agree with the solver's to this (mm). The solver stops once its rows are
/// within 1e-8 (a turn weighing 10 mm a radian), so a part a metre out on an arm is its own
/// 1e-6 mm from exact: its placements are a little looser than ours, which are checked
/// against its rows directly ([`HELD`]).
const NEAR: f64 = 1e-5;
/// Rotations agree to this (each entry of the matrix).
const TURNED: f64 = 1e-8;
/// The solver's rows (mm; a turn weighing 10 mm a radian) at our placements: every mate, every
/// driven value and every relation holds to this.
const HELD: f64 = 1e-9;
/// Our own numbers agree to this (mm, or an entry of a rotation).
const EXACT: f64 = 1e-9;

fn frame_of(c: &MateConnector) -> ConnectorFrame {
    c.local_frame(None)
}

/// Numbers in (0, 1), the same every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    /// Axis values within every axis's travel (a continuous turn: within a turn either way; a
    /// slide without limits: within 200 mm).
    fn within(&mut self, axes: &[MachineAxis]) -> Vec<f64> {
        axes.iter()
            .map(|a| {
                if a.continuous {
                    self.between(-360.0, 360.0)
                } else if a.max >= UNLIMITED {
                    self.between(-200.0, 200.0)
                } else {
                    self.between(a.min, a.max)
                }
            })
            .collect()
    }
}

fn pose_gap(a: &Pose, b: &Pose) -> (f64, f64) {
    let moved = (0..3)
        .map(|i| (a.translation[i] - b.translation[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    let turned = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| (a.rotation[i][j] - b.rotation[i][j]).abs())
        .fold(0.0, f64::max);
    (moved, turned)
}

fn assert_same_poses(what: &str, ours: &[(InstanceId, Pose)], theirs: &[(InstanceId, Pose)]) {
    assert_eq!(ours.len(), theirs.len(), "{what}");
    for (id, p) in ours {
        let q = theirs
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, q)| q)
            .unwrap_or_else(|| panic!("{what}: no instance {id}"));
        let (moved, turned) = pose_gap(p, q);
        assert!(
            moved < NEAR && turned < TURNED,
            "{what}: instance {id} is {moved:e} mm and {turned:e} off\n ours {p:?}\n theirs {q:?}"
        );
    }
}

fn vec_gap(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// The assembly solver's placements with the mates at `values` (mm, radians), reached from
/// the document's placements in steps small enough for it to follow, as an animation does.
fn solved(flat: &Assembly, r: &Resolved, values: &[(MateId, Dof, f64)]) -> Vec<(InstanceId, Pose)> {
    let from: Vec<f64> = values
        .iter()
        .map(|(m, d, _)| r.tree.joints[r.tree.joint_of(*m, *d).expect("a joint")].present)
        .collect();
    let far = values
        .iter()
        .zip(&from)
        .map(|((_, d, v), f)| (v - f).abs() / if d.is_angle() { 0.2 } else { 40.0 })
        .fold(0.0, f64::max);
    let steps = far.ceil().max(1.0) as usize;
    let mut model = flat.clone();
    for s in 1..=steps {
        let t = s as f64 / steps as f64;
        let drives = values
            .iter()
            .zip(&from)
            .map(|((mate, dof, v), f)| Drive {
                mate: *mate,
                dof: *dof,
                value: f + (v - f) * t,
            })
            .collect();
        let sol = solver::solve(
            &model,
            &frame_of,
            &SolveOptions {
                drives,
                ..Default::default()
            },
        );
        assert!(sol.converged, "the solver left {:e}", sol.residual);
        for (id, p) in &sol.poses {
            model.instance_mut(*id).unwrap().pose = *p;
        }
    }
    model.instances.iter().map(|i| (i.id, i.pose)).collect()
}

/// How far the solver finds `poses` from holding every mate and relation of `flat` with the
/// mates at `values`: its largest row there (mm; a turn weighing 10 mm a radian). It must
/// find nothing to do: the placements are judged as given.
fn solver_rows(
    what: &str,
    flat: &Assembly,
    poses: &[(InstanceId, Pose)],
    values: &[(MateId, Dof, f64)],
) -> f64 {
    let mut model = flat.clone();
    for (id, p) in poses {
        model.instance_mut(*id).unwrap().pose = *p;
    }
    let drives = values
        .iter()
        .map(|(mate, dof, value)| Drive {
            mate: *mate,
            dof: *dof,
            value: *value,
        })
        .collect();
    let sol = solver::solve(
        &model,
        &frame_of,
        &SolveOptions {
            drives,
            ..Default::default()
        },
    );
    assert_eq!(sol.iterations, 0, "{what}: the solver moved our placements");
    sol.residual
}

/// Every joint that moves by itself with the value it has at the axis values `q` and the
/// actuator values `actuators` (mm or degrees, by mate): what the solver is told to hold.
fn driven(r: &Resolved, q: &[f64], actuators: &[(MateId, Dof, f64)]) -> Vec<(MateId, Dof, f64)> {
    let of_axes = r.mate_values(q);
    r.tree
        .joints
        .iter()
        .filter(|j| j.motion == Motion::Free)
        .map(|j| {
            let is_axis = r.axes.iter().any(|a| a.mate == j.mate && a.dof == j.dof);
            let v = if is_axis {
                of_axes
                    .iter()
                    .find(|(m, d, _)| *m == j.mate && *d == j.dof)
                    .map(|v| v.2)
                    .expect("an axis has a mate value")
            } else {
                actuators
                    .iter()
                    .find(|(m, d, _)| *m == j.mate && *d == j.dof)
                    .map_or(j.present, |(_, _, v)| {
                        if j.dof.is_angle() { v.to_radians() } else { *v }
                    })
            };
            (j.mate, j.dof, v)
        })
        .collect()
}

/// The key test: at `count` random positions within the axes' travel, the tree's forward
/// kinematics place every instance where the assembly solver does with the same mate values
/// driven, and the chain's mounts are where the mount connectors are on those placements.
fn check_against_the_solver(what: &str, flat: &Assembly, def: &MachineDef, count: usize) {
    let r = resolve_model(flat, def, &stored_frames)
        .unwrap_or_else(|issues| panic!("{what}: {issues:?}"));
    let chain = r.chain().clone();
    assert_eq!(chain.len(), r.axes.len(), "{what}");
    let present: Vec<(InstanceId, Pose)> = flat.instances.iter().map(|i| (i.id, i.pose)).collect();
    assert_same_poses(what, &r.poses(&r.present(), &[]), &present);
    let mut random = Random(0x9e37_79b9_7f4a_7c15 ^ what.len() as u64);
    let mut out = Vec::new();
    for n in 0..count {
        let q = random.within(&r.axes);
        let what = format!("{what}, position {n} {q:?}");
        let values = driven(&r, &q, &[]);
        r.poses_into(&q, &[], &mut out);
        // The tree by itself, given the mates' values, places them the same.
        assert_same_poses(&what, &r.tree.poses(&r.mate_values(&q)), &out);
        // Exactly: the solver's own rows vanish at our placements.
        let rows = solver_rows(&what, flat, &out, &values);
        assert!(
            rows < HELD,
            "{what}: the solver's rows are {rows:e} at our placements"
        );
        // And they are the placements the solver arrives at from where the document is.
        let theirs = solved(flat, &r, &values);
        assert_same_poses(&what, &out, &theirs);

        // The mounts, from the solver's placements: the chain's tool mount has the
        // connector's origin and X with its Z turned round; the table mount is the connector.
        let world = |c: &MateConnector| {
            let pose = theirs
                .iter()
                .find(|(i, _)| *i == c.instance)
                .map(|(_, p)| *p)
                .unwrap_or_default();
            c.local_frame(None).moved(&pose)
        };
        let tool = world(def.tool_mount.as_ref().unwrap());
        let table = world(def.table_mount.as_ref().unwrap());
        let (t, b) = chain.mounts(&q);
        assert!(vec_gap(t.origin, tool.origin) < NEAR, "{what}: tool mount");
        assert!(
            vec_gap(t.z, [-tool.z[0], -tool.z[1], -tool.z[2]]) < TURNED,
            "{what}: tool Z"
        );
        assert!(vec_gap(t.x, tool.x) < TURNED, "{what}: tool X");
        assert!(
            vec_gap(b.origin, table.origin) < NEAR,
            "{what}: table mount"
        );
        assert!(vec_gap(b.z, table.z) < TURNED, "{what}: table Z");
        assert!(vec_gap(b.x, table.x) < TURNED, "{what}: table X");
        // The chain and the tree are one model: they agree to rounding.
        let (ours_tool, ours_table) = r.mounts(&q);
        let (ours_tool, ours_table) = (ours_tool.unwrap(), ours_table.unwrap());
        assert!(vec_gap(t.origin, ours_tool.origin) < EXACT, "{what}");
        assert!(vec_gap(t.x, ours_tool.x) < EXACT, "{what}");
        assert!(vec_gap(t.z, [-ours_tool.z[0], -ours_tool.z[1], -ours_tool.z[2]]) < EXACT);
        assert!(vec_gap(b.origin, ours_table.origin) < EXACT, "{what}");
        assert!(vec_gap(b.x, ours_table.x) < EXACT, "{what}");
        assert!(vec_gap(b.z, ours_table.z) < EXACT, "{what}");
    }
}

/// What `detect` made of a sample: each axis's name, side and sign.
fn rows(axes: &[MachineAxis]) -> Vec<(&str, Side, bool)> {
    axes.iter()
        .map(|a| (a.name.as_str(), a.side, a.reversed))
        .collect()
}

fn travel(axes: &[MachineAxis], name: &str) -> (f64, f64) {
    let a = axes.iter().find(|a| a.name == name).unwrap();
    (a.min, a.max)
}

fn assert_travel(axes: &[MachineAxis], name: &str, want: (f64, f64)) {
    let (lo, hi) = travel(axes, name);
    assert!(
        (lo - want.0).abs() < 1e-9 && (hi - want.1).abs() < 1e-9,
        "{name}: {lo} to {hi}, not {want:?}"
    );
}

use Side::{Table, Tool};

#[test]
fn a_router_has_x_y_z_on_the_tool_and_a_under_the_work() {
    let s = samples::router();
    let axes = &s.def().axes;
    // Y's mate has its first connector on the carriage, so the carriage goes −Y as the mate
    // rises; Z's mate plunges; A turns the work, so it is reversed against its own motion.
    assert_eq!(
        rows(axes),
        [
            ("X", Tool, false),
            ("Y", Tool, true),
            ("Z", Tool, true),
            ("A", Table, true)
        ]
    );
    assert_travel(axes, "X", (-300.0, 300.0));
    assert_travel(axes, "Y", (-250.0, 150.0));
    assert_travel(axes, "Z", (-120.0, 0.0));
    let a = s.def().axis_named("A").unwrap();
    assert!(a.continuous && a.limits_from_mate);
    assert!(axes.iter().all(|a| a.home == 0.0));
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    let chain = r.chain();
    assert!(chain.is_mill());
    assert_eq!(chain.tool.len(), 3);
    assert_eq!(chain.table.len(), 1);
    // The chain's own directions carry the signs: X+ is the tool to +X, and A+ turns the
    // work left-handed about X.
    assert!(vec_gap(chain.tool[0].dir, X) < 1e-12);
    assert!(vec_gap(chain.tool[1].dir, Y) < 1e-12);
    assert!(vec_gap(chain.tool[2].dir, Z) < 1e-12);
    assert!(vec_gap(chain.table[0].dir, [-1.0, 0.0, 0.0]) < 1e-12);
    assert!(vec_gap(chain.table[0].origin, [0.0, 0.0, 50.0]) < 1e-9);
    assert!(vec_gap(chain.tool_mount.origin, [0.0, 0.0, 200.0]) < 1e-9);
    assert!(vec_gap(chain.tool_mount.z, Z) < 1e-12);
    assert!(vec_gap(chain.table_mount.z, Z) < 1e-12);
    // The controller's summary.
    assert_eq!(r.machine.travel, [600.0, 400.0, 120.0]);
}

#[test]
fn a_trunnion_mill_has_z_on_the_tool_and_the_rest_under_the_work() {
    for (s, tilt) in [(samples::trunnion_ac(), "A"), (samples::trunnion_bc(), "B")] {
        let axes = &s.def().axes;
        // The table's X mate has its first connector on the table: the table goes −X as it
        // rises, which is the tool going +X, so X alone is not reversed.
        assert_eq!(
            rows(axes),
            [
                ("Z", Tool, false),
                ("Y", Table, true),
                ("X", Table, false),
                (tilt, Table, true),
                ("C", Table, true)
            ]
        );
        // Z was built 25 mm below its mate's zero: it reads 0 there.
        let z = s.def().axis_named("Z").unwrap();
        assert!((z.zero + 25.0).abs() < 1e-9);
        assert_travel(axes, "Z", (-375.0, 25.0));
        assert_travel(axes, tilt, (-120.0, 120.0));
        assert!(s.def().axis_named("C").unwrap().continuous);
        let r = s.resolve().unwrap();
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert!(r.chain().is_mill());
        // The column is one body with the base, and the spindle with the head.
        let base = r.tree.body_of(s.part("Base")).unwrap();
        assert_eq!(r.tree.body_of(s.part("Column")), Some(base));
        assert_eq!(r.tree.bodies[base].link, Link::Base);
        assert_eq!(
            r.tree.body_of(s.part("Spindle")),
            r.tree.body_of(s.part("Head"))
        );
    }
}

#[test]
fn a_fork_head_gantry_has_five_axes_on_the_tool() {
    let s = samples::gantry();
    // C's mate turns about −Z.
    assert_eq!(
        rows(&s.def().axes),
        [
            ("X", Tool, false),
            ("Y", Tool, false),
            ("Z", Tool, false),
            ("C", Tool, true),
            ("B", Tool, false)
        ]
    );
    assert_travel(&s.def().axes, "C", (-360.0, 360.0));
    assert_travel(&s.def().axes, "B", (-100.0, 100.0));
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert!(r.chain().table.is_empty());
    assert!(r.chain().is_mill());
}

#[test]
fn an_arm_is_numbered_from_the_base() {
    let s = samples::arm();
    let axes = &s.def().axes;
    assert_eq!(
        rows(axes),
        [
            ("J1", Tool, false),
            ("J2", Tool, false),
            ("J3", Tool, false),
            ("J4", Tool, false),
            ("J5", Tool, false),
            ("J6", Tool, false)
        ]
    );
    // An arm's joints read as their mates do.
    assert_travel(axes, "J2", (-42.0, 90.0));
    assert_travel(axes, "J3", (-89.0, 52.0));
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    let chain = r.chain();
    assert!(!chain.is_mill());
    // J2's mate has its first connector on the link it carries: the chain's line is the
    // other way round.
    assert!(vec_gap(chain.tool[0].dir, Z) < 1e-12);
    assert!(vec_gap(chain.tool[1].dir, [0.0, -1.0, 0.0]) < 1e-12);
    assert!(vec_gap(chain.tool[2].dir, Y) < 1e-12);
    assert!(vec_gap(chain.tool[1].origin, [64.2, 0.0, 169.77]) < 1e-9);
}

#[test]
fn forward_kinematics_place_every_sample_where_the_solver_does() {
    for s in samples::all() {
        check_against_the_solver(s.name, &s.assembly, s.def(), 20);
    }
}

/// CAM's inverse kinematics take the chain: a position's cutter location solves back onto
/// the same location.
#[test]
fn the_chain_of_a_mill_solves_back() {
    for s in [
        samples::router(),
        samples::trunnion_ac(),
        samples::trunnion_bc(),
        samples::gantry(),
    ] {
        let r = s.resolve().unwrap();
        let chain: &Chain = r.chain();
        let mut random = Random(0x1234_5678_9abc_def1);
        for _ in 0..10 {
            let q = random.within(&r.axes);
            let cl = chain.locate(&q, 60.0);
            let back = chain
                .solve(&cl, 60.0, &q)
                .unwrap_or_else(|e| panic!("{}: {e}", s.name));
            let again = chain.locate(&back, 60.0);
            assert!(vec_gap(again.tip, cl.tip) < 1e-6, "{}", s.name);
            assert!(vec_gap(again.axis, cl.axis) < 1e-9, "{}", s.name);
        }
    }
}

#[test]
fn an_axis_parked_away_from_zero_moves_the_chain_there() {
    let mut s = samples::trunnion_ac();
    let mut def = s.def().clone();
    for (name, home) in [("Z", -100.0), ("A", 30.0), ("X", 40.0)] {
        def.axes.iter_mut().find(|a| a.name == name).unwrap().home = home;
    }
    s.assembly.machine = Some(Box::new(def));
    let r = s.resolve().unwrap();
    let chain = r.chain();
    assert_eq!(chain.home(), r.home());
    assert_eq!(chain.joint(chain.index("Z").unwrap()).home, -100.0);
    // The chain is described at home: the tool mount is 100 mm lower than it is modelled.
    assert!(vec_gap(chain.tool_mount.origin, [0.0, 0.0, 200.0]) < 1e-9);
    check_against_the_solver("parked", &s.assembly, s.def(), 8);
}

/// A machine with one mate from the base to the head and the work on the base.
fn one_mate(
    t: MateType,
    first_on_base: bool,
    at: ConnectorFrame,
    now: Position,
    limits: MateLimits,
) -> Builder {
    let mut b = Builder::new();
    let base = b.base("Base");
    let head = b.part(
        "Head",
        Pose::rotation_about([10.0, 20.0, 30.0], [1.0, 2.0, 3.0], 0.4),
    );
    let (first, second) = if first_on_base {
        (base, head)
    } else {
        (head, base)
    };
    b.mate("Quill", t, first, second, at, now, Some(limits));
    b.tool_mount(head, frame([0.0, 0.0, 100.0], DOWN));
    b.table_mount(base, frame([0.0; 3], Z));
    b
}

#[test]
fn a_cylindrical_mate_is_a_slide_then_a_turn() {
    let limits = MateLimits {
        z: Some((-80.0, 60.0)),
        angle: Some((-2.0, 2.5)),
        ..MateLimits::default()
    };
    let now = Position {
        z: 12.0,
        angle: 0.3,
        ..Position::default()
    };
    for first_on_base in [true, false] {
        let s = one_mate(
            MateType::Cylindrical,
            first_on_base,
            frame([15.0, -5.0, 300.0], Z),
            now,
            limits,
        )
        .finish("Quill");
        let r = s.resolve().unwrap();
        let kinds: Vec<(Dof, JointKind)> = r.tree.joints.iter().map(|j| (j.dof, j.kind)).collect();
        assert_eq!(
            kinds,
            [(Dof::Z, JointKind::Linear), (Dof::Angle, JointKind::Rotary)]
        );
        // One slide and one turn about Z: Z and C, both of the one mate.
        let names: Vec<&str> = r.axes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Z", "C"]);
        assert!(r.axes.iter().all(|a| a.mate == s.mate("Quill")));
        // The head rises with the mate when the base has the first connector.
        assert_eq!(r.axes[0].reversed, !first_on_base);
        assert!((r.axes[0].zero - 12.0).abs() < 1e-9);
        check_against_the_solver("cylindrical", &s.assembly, s.def(), 12);
    }
}

#[test]
fn a_pin_slot_slides_along_the_slot_and_turns_about_the_pin() {
    let limits = MateLimits {
        x: Some((-50.0, 70.0)),
        angle: Some((-1.5, 1.5)),
        ..MateLimits::default()
    };
    let now = Position {
        x: 18.0,
        angle: -0.4,
        ..Position::default()
    };
    for slot_on_base in [true, false] {
        let s = one_mate(
            MateType::PinSlot,
            slot_on_base,
            ConnectorFrame::new([20.0, 0.0, 250.0], Z, X),
            now,
            limits,
        )
        .finish("Slot");
        let r = s.resolve().unwrap();
        let kinds: Vec<JointKind> = r.tree.joints.iter().map(|j| j.kind).collect();
        // The slot fixed: the pin slides, then turns. The pin fixed: the slot's part turns
        // about it, and slides along its own slot.
        if slot_on_base {
            assert_eq!(kinds, [JointKind::Linear, JointKind::Rotary]);
        } else {
            assert_eq!(kinds, [JointKind::Rotary, JointKind::Linear]);
        }
        check_against_the_solver("pin slot", &s.assembly, s.def(), 12);
    }
}

fn relation(b: &mut Builder, name: &str, r: Relation) -> MateId {
    let id = MateId::from_u128(0x7e1a_0000 + b.assembly.mates.len() as u128);
    b.assembly
        .mates
        .push(MateFeature::new(id, name, MateKind::Relation(r)));
    id
}

/// A plain mill (X and Y under the work, Z over it) to hang other things on: the builder and
/// its Base, Table and Head.
fn plain_mill() -> (Builder, InstanceId, InstanceId, InstanceId) {
    let mut b = Builder::new();
    let base = b.base("Base");
    let saddle = b.part("Saddle", Pose::translation([0.0, 0.0, -100.0]));
    let table = b.part("Table", Pose::translation([0.0, 0.0, -50.0]));
    let head = b.part("Head", Pose::translation([0.0, 100.0, 400.0]));
    b.slider(
        "Y",
        base,
        saddle,
        frame([0.0, 0.0, -100.0], Y),
        (-150.0, 150.0),
    );
    b.slider(
        "X",
        saddle,
        table,
        frame([0.0, 0.0, -50.0], X),
        (-250.0, 250.0),
    );
    b.slider(
        "Z",
        base,
        head,
        frame([0.0, 200.0, 400.0], Z),
        (-300.0, 0.0),
    );
    b.tool_mount(head, frame([0.0, 0.0, 300.0], DOWN));
    b.table_mount(table, frame([0.0; 3], Z));
    (b, base, table, head)
}

#[test]
fn a_screw_and_a_linear_relation_make_followers() {
    let (mut b, _, table, _) = plain_mill();
    // A vise on the table: its screw is a Cylindrical mate with a 5 mm pitch, and its two
    // jaws are sliders tied to close on each other.
    let screw = b.part("Screw", Pose::translation([80.0, 0.0, 30.0]));
    let jaw1 = b.part("Jaw 1", Pose::translation([-30.0, 0.0, 20.0]));
    let jaw2 = b.part("Jaw 2", Pose::translation([30.0, 0.0, 20.0]));
    let turn = b.mate(
        "Vise screw",
        MateType::Cylindrical,
        table,
        screw,
        frame([0.0, 0.0, 30.0], X),
        Position::default(),
        None,
    );
    let s1 = b.slider(
        "Jaw slide 1",
        table,
        jaw1,
        frame([0.0, 0.0, 20.0], X),
        (-40.0, 40.0),
    );
    let s2 = b.slider(
        "Jaw slide 2",
        table,
        jaw2,
        frame([0.0, 0.0, 20.0], X),
        (-40.0, 40.0),
    );
    relation(
        &mut b,
        "Screw 1",
        Relation {
            distance: 5.0,
            ..Relation::new(RelationType::Screw, vec![turn])
        },
    );
    relation(
        &mut b,
        "Linear 1",
        Relation {
            reverse: true,
            ..Relation::new(RelationType::Linear, vec![s1, s2])
        },
    );
    let mut s = b.finish("Mill with a vise");
    let (screw_id, jaws_id) = (ActuatorId::from_u128(1), ActuatorId::from_u128(2));
    let mut def = s.def().clone();
    def.actuators = vec![
        Actuator {
            id: screw_id,
            name: "Vise screw".into(),
            mate: turn,
            dof: Dof::Angle,
            positions: vec![],
            rest: 0,
        },
        Actuator {
            id: jaws_id,
            name: "Jaws".into(),
            mate: s1,
            dof: Dof::Z,
            positions: vec![("Open".into(), 0.0), ("Closed".into(), 25.0)],
            rest: 0,
        },
    ];
    s.assembly.machine = Some(Box::new(def));
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert_eq!(
        rows(&r.axes),
        [("Z", Tool, false), ("Y", Table, true), ("X", Table, true)]
    );
    let joint = |m: MateId, d: Dof| &r.tree.joints[r.tree.joint_of(m, d).unwrap()];
    // The screw's turn leads its slide: 5 mm a turn. The second jaw follows the first, the
    // other way.
    let Motion::Follows { joint: lead, ratio } = joint(turn, Dof::Z).motion else {
        panic!("the screw's slide follows its turn");
    };
    assert_eq!(lead, r.tree.joint_of(turn, Dof::Angle).unwrap());
    assert!((ratio - 5.0 / std::f64::consts::TAU).abs() < 1e-12);
    let Motion::Follows { joint: lead, ratio } = joint(s2, Dof::Z).motion else {
        panic!("the second jaw follows the first");
    };
    assert_eq!(lead, r.tree.joint_of(s1, Dof::Z).unwrap());
    assert!((ratio + 1.0).abs() < 1e-12);
    // Two turns of the screw and the jaws closed by 25, with the axes somewhere: where the
    // solver puts everything with only the leaders driven.
    let q = [-120.0, 60.0, -90.0];
    let ours = r.poses(&q, &[(screw_id, 720.0), (jaws_id, 25.0)]);
    let theirs = solved(
        &s.assembly,
        &r,
        &driven(&r, &q, &[(turn, Dof::Angle, 720.0), (s1, Dof::Z, 25.0)]),
    );
    assert_same_poses("vise", &ours, &theirs);
    let values = driven(&r, &q, &[(turn, Dof::Angle, 720.0), (s1, Dof::Z, 25.0)]);
    assert!(solver_rows("vise", &s.assembly, &ours, &values) < HELD);
    let at = |id: InstanceId| ours.iter().find(|(i, _)| *i == id).unwrap().1.translation;
    // (The table is at X −90 reversed, Y 60 reversed: the screw went 10 along X on it.)
    assert!((at(screw)[0] - (80.0 + 90.0 + 10.0)).abs() < 1e-9);
    assert!((at(jaw1)[0] - (-30.0 + 90.0 + 25.0)).abs() < 1e-9);
    assert!((at(jaw2)[0] - (30.0 + 90.0 - 25.0)).abs() < 1e-9);
    // With nothing said the jaws are at rest: open.
    let rest = r.poses(&q, &[]);
    let jaw_at_rest = rest.iter().find(|(i, _)| *i == jaw1).unwrap().1.translation;
    assert!((jaw_at_rest[0] - 60.0).abs() < 1e-9);
}

#[test]
fn a_pinion_follows_the_axis_it_drives() {
    let (mut b, base, _, _) = plain_mill();
    let pinion = b.part("Pinion", Pose::translation([0.0, -80.0, -100.0]));
    let turn = b.revolute(
        "Pinion bearing",
        base,
        pinion,
        frame([0.0, -80.0, -100.0], X),
        None,
    );
    let y = b.assembly.mates.iter().find(|f| f.name == "Y").unwrap().id;
    relation(
        &mut b,
        "Rack and pinion 1",
        Relation {
            distance: 40.0,
            ..Relation::new(RelationType::RackPinion, vec![turn, y])
        },
    );
    let s = b.finish("Mill with a rack");
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    // The pinion is no axis and no actuator: it turns once for every 40 mm of Y.
    assert_eq!(
        rows(&r.axes),
        [("Z", Tool, false), ("Y", Table, true), ("X", Table, true)]
    );
    assert!(detect(&r.tree, s.def()).off_path.is_empty());
    let q = [-50.0, 100.0, 30.0];
    let values = r.mate_values(&q);
    let angle = values.iter().find(|v| v.0 == turn).unwrap().2;
    // Y reads 100 where its mate is at −100 (the table's axis is reversed).
    assert!((angle - (-100.0 / 40.0 * std::f64::consts::TAU)).abs() < 1e-9);
    // Only the axes are driven: the solver's relation turns the pinion the same.
    let theirs = solved(&s.assembly, &r, &driven(&r, &q, &[]));
    assert_same_poses("rack", &r.poses(&q, &[]), &theirs);
    let values = driven(&r, &q, &[]);
    assert!(solver_rows("rack", &s.assembly, &r.poses(&q, &[]), &values) < HELD);
}

#[test]
fn a_relation_between_two_axes_is_an_error() {
    let (mut b, _, _, _) = plain_mill();
    let (x, y) = (
        b.assembly.mates.iter().find(|f| f.name == "X").unwrap().id,
        b.assembly.mates.iter().find(|f| f.name == "Y").unwrap().id,
    );
    let rel = relation(
        &mut b,
        "Linear 1",
        Relation::new(RelationType::Linear, vec![x, y]),
    );
    let s = b.finish("Tied");
    let issues = s.resolve().unwrap_err();
    let e = error_naming(&issues, rel);
    assert!(
        e.text.contains("Linear 1") && e.text.contains("X and Y"),
        "{}",
        e.text
    );

    // A screw on the way to the tool: the quill would slide and turn at once.
    let mut b = one_mate(
        MateType::Cylindrical,
        true,
        frame([0.0, 0.0, 300.0], Z),
        Position::default(),
        MateLimits::default(),
    );
    let quill = b.assembly.mates[0].id;
    let rel = relation(
        &mut b,
        "Screw 1",
        Relation::new(RelationType::Screw, vec![quill]),
    );
    let issues = b.finish("Screwed").resolve().unwrap_err();
    assert!(
        error_naming(&issues, rel)
            .text
            .contains("slide and the turn")
    );
}

fn error_naming(issues: &[Issue], mate: MateId) -> &Issue {
    issues
        .iter()
        .find(|i| i.severity == Severity::Error && i.mates.contains(&mate))
        .unwrap_or_else(|| panic!("no error names the mate: {issues:?}"))
}

fn warning_naming(issues: &[Issue], mate: MateId) -> &Issue {
    issues
        .iter()
        .find(|i| i.severity == Severity::Warning && i.mates.contains(&mate))
        .unwrap_or_else(|| panic!("no warning names the mate: {issues:?}"))
}

#[test]
fn a_second_rail_is_redundant() {
    let (mut b, base, _, head) = plain_mill();
    // The base is held by a Fastened mate to the Origin, not by Fix.
    b.assembly.instances[0].fixed = false;
    b.mate(
        "To origin",
        MateType::Fastened,
        base,
        InstanceId::ORIGIN,
        frame([0.0; 3], Z),
        Position::default(),
        None,
    );
    // A second Z rail beside the first, and a Cylindrical guide rod along it too.
    let rail = b.slider(
        "Z rail 2",
        base,
        head,
        frame([150.0, 200.0, 380.0], Z),
        (-300.0, 0.0),
    );
    let rod = b.mate(
        "Guide rod",
        MateType::Cylindrical,
        head,
        base,
        frame([-150.0, 200.0, 420.0], DOWN),
        Position::default(),
        None,
    );
    let s = b.finish("Two rails");
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert_eq!(r.tree.redundant, [rail, rod]);
    assert!(r.tree.loops.is_empty());
    assert_eq!(
        rows(&r.axes),
        [("Z", Tool, false), ("Y", Table, true), ("X", Table, true)]
    );
    check_against_the_solver("two rails", &s.assembly, s.def(), 6);
}

/// Adds a four-bar linkage on `ground`: a crank and a rocker pinned to it, a coupler pinned
/// to both. Returns the coupler and the four pins (the loop is closed by the last).
fn four_bar(b: &mut Builder, ground: InstanceId) -> (InstanceId, [MateId; 4]) {
    let crank = b.part("Crank", Pose::IDENTITY);
    let coupler = b.part("Coupler", Pose::IDENTITY);
    let rocker = b.part("Rocker", Pose::IDENTITY);
    let pin = |b: &mut Builder, name: &str, a, c, at: [f64; 3]| {
        b.revolute(name, a, c, frame(at, Y), None)
    };
    let p1 = pin(b, "Pin 1", ground, crank, [400.0, 0.0, 0.0]);
    let p2 = pin(b, "Pin 2", crank, coupler, [400.0, 0.0, 60.0]);
    let p3 = pin(b, "Pin 3", ground, rocker, [520.0, 0.0, 0.0]);
    let p4 = pin(b, "Pin 4", coupler, rocker, [540.0, 0.0, 90.0]);
    (coupler, [p1, p2, p3, p4])
}

#[test]
fn a_loop_is_an_error_on_the_way_to_a_mount_and_frozen_off_it() {
    // Off the way: the linkage is held, with a warning, and the machine works.
    let (mut b, base, _, _) = plain_mill();
    let (_, pins) = four_bar(&mut b, base);
    let s = b.finish("Mill with a linkage");
    let r = s.resolve().unwrap();
    let w = warning_naming(&r.issues, pins[3]);
    assert!(
        w.text.contains("Pin 4") && w.text.contains("close a loop"),
        "{}",
        w.text
    );
    assert_eq!(r.tree.loops.len(), 1);
    assert!(!r.tree.loops[0].on_path);
    assert_eq!(r.tree.loops[0].mates[0], pins[3]);
    for p in &pins[..3] {
        assert!(r.tree.loops[0].mates.contains(p));
        assert_eq!(
            r.tree.joints[r.tree.joint_of(*p, Dof::Angle).unwrap()].motion,
            Motion::Frozen
        );
    }
    // Nothing of it is offered as an actuator.
    assert!(detect(&r.tree, s.def()).off_path.is_empty());
    check_against_the_solver("linkage", &s.assembly, s.def(), 4);

    // The tool on the coupler: an error naming the loop's mates.
    let (mut b, base, _, _) = plain_mill();
    let (coupler, pins) = four_bar(&mut b, base);
    b.tool_mount(coupler, frame([470.0, 0.0, 75.0], DOWN));
    let issues = b.finish("Linkage head").resolve().unwrap_err();
    let e = error_naming(&issues, pins[3]);
    assert!(pins.iter().all(|p| e.mates.contains(p)), "{e:?}");
    assert!(e.text.contains("close a loop"), "{}", e.text);
}

#[test]
fn a_planar_mate_is_an_error_on_the_way_to_a_mount_and_held_off_it() {
    let (mut b, base, table, _) = plain_mill();
    let puck = b.part("Puck", Pose::translation([100.0, 0.0, 0.0]));
    let planar = b.mate(
        "Planar 1",
        MateType::Planar,
        table,
        puck,
        frame([100.0, 0.0, 0.0], Z),
        Position::default(),
        None,
    );
    let ball = b.part("Lamp", Pose::translation([-300.0, 300.0, 200.0]));
    let socket = b.mate(
        "Ball 1",
        MateType::Ball,
        base,
        ball,
        frame([-300.0, 300.0, 200.0], Z),
        Position::default(),
        None,
    );
    let s = b.clone().finish("Mill with a puck");
    let r = s.resolve().unwrap();
    assert!(warning_naming(&r.issues, planar).text.contains("Planar"));
    assert!(warning_naming(&r.issues, socket).text.contains("Ball"));
    let body = r.tree.body_of(puck).unwrap();
    assert_eq!(r.tree.bodies[body].link, Link::Held(planar));
    // The puck rides with the table.
    let q = [0.0, 0.0, 75.0];
    let at = r.poses(&q, &[]);
    let puck_at = at.iter().find(|(i, _)| *i == puck).unwrap().1.translation;
    assert!(vec_gap(puck_at, [25.0, 0.0, 0.0]) < 1e-9);

    // The work on the puck: the Planar mate is between the base and the table mount.
    b.table_mount(puck, frame([100.0, 0.0, 10.0], Z));
    let issues = b.finish("Work on a puck").resolve().unwrap_err();
    let e = error_naming(&issues, planar);
    assert!(
        e.text.contains("Planar 1") && e.text.contains("table"),
        "{}",
        e.text
    );
}

#[test]
fn a_machine_needs_a_base_and_its_mounts() {
    // Nothing fixed.
    let (mut b, _, _, _) = plain_mill();
    b.assembly.instances[0].fixed = false;
    let issues = b.finish("Loose").resolve().unwrap_err();
    assert!(
        issues
            .iter()
            .any(|i| i.is_error() && i.text == "Fix the machine's base"),
        "{issues:?}"
    );

    // No mounts picked.
    let (b, _, _, _) = plain_mill();
    let def = MachineDef::default();
    let issues = resolve_model(&b.assembly, &def, &stored_frames).unwrap_err();
    for what in ["Pick the tool mount", "Pick the table mount"] {
        assert!(
            issues.iter().any(|i| i.is_error() && i.text == what),
            "{issues:?}"
        );
    }

    // A mount picked on a face that is gone (here: no parts at all).
    let (b, _, _, head) = plain_mill();
    let mut def = b.def();
    let face = FaceName::new(uuid::Uuid::nil(), FaceOrigin::Unnamed { index: 3 });
    def.tool_mount = Some(MateConnector {
        anchor: ConnectorAnchor::Implicit {
            point: ImplicitPoint::FaceCentroid(face),
            owner: EntityRef::Face(face),
        },
        ..MateConnector::at(head, frame([0.0, 0.0, 300.0], DOWN))
    });
    let issues = resolve_model(&b.assembly, &def, &stored_frames).unwrap_err();
    let lost = issues.iter().find(|i| i.is_error()).unwrap();
    assert!(lost.text.contains("tool mount has lost"), "{}", lost.text);
    assert_eq!(lost.instances, [head]);

    // A mount on a part no mate joins to the base.
    let (mut b, _, _, _) = plain_mill();
    let loose = b.part("Loose", Pose::translation([0.0, 0.0, 500.0]));
    b.tool_mount(loose, frame([0.0, 0.0, 500.0], DOWN));
    let issues = b.finish("Loose head").resolve().unwrap_err();
    assert!(
        issues
            .iter()
            .any(|i| i.is_error() && i.text.contains("No mate joins the tool mount")),
        "{issues:?}"
    );

    // Both mounts on the base: nothing to drive.
    let (mut b, base, _, _) = plain_mill();
    b.tool_mount(base, frame([0.0, 0.0, 300.0], DOWN));
    b.table_mount(base, frame([0.0; 3], Z));
    let issues = b.finish("Still").resolve().unwrap_err();
    assert!(
        issues
            .iter()
            .any(|i| i.is_error() && i.text.contains("no axes"))
    );
}

#[test]
fn a_carriage_under_both_mounts_is_not_an_axis() {
    let mut b = Builder::new();
    let base = b.base("Base");
    let sled = b.part("Sled", Pose::IDENTITY);
    let table = b.part("Table", Pose::IDENTITY);
    let head = b.part("Head", Pose::IDENTITY);
    let w = b.slider(
        "Sled slide",
        base,
        sled,
        frame([0.0; 3], Y),
        (-500.0, 500.0),
    );
    b.slider("X", sled, table, frame([0.0; 3], X), (-100.0, 100.0));
    b.slider("Z", sled, head, frame([0.0, 0.0, 300.0], Z), (-100.0, 0.0));
    b.tool_mount(head, frame([0.0, 0.0, 200.0], DOWN));
    b.table_mount(table, frame([0.0; 3], Z));
    let mut s = b.finish("On a sled");
    let r = s.resolve().unwrap();
    assert!(warning_naming(&r.issues, w).text.contains("together"));
    assert_eq!(rows(&r.axes), [("Z", Tool, false), ("X", Table, true)]);
    assert_eq!(
        r.tree.joints[r.tree.joint_of(w, Dof::Z).unwrap()].role,
        Role::Both
    );
    // It can be an actuator: both mounts ride on it.
    let id = ActuatorId::from_u128(7);
    let mut def = s.def().clone();
    def.actuators.push(Actuator {
        id,
        name: "Sled".into(),
        mate: w,
        dof: Dof::Z,
        positions: vec![("In".into(), 0.0), ("Out".into(), 400.0)],
        rest: 0,
    });
    s.assembly.machine = Some(Box::new(def));
    let r = s.resolve().unwrap();
    let at = r.poses(&[-20.0, 30.0], &[(id, 400.0)]);
    let place = |i: InstanceId| at.iter().find(|(k, _)| *k == i).unwrap().1.translation;
    assert!(vec_gap(place(s.part("Head")), [0.0, 400.0, -20.0]) < 1e-9);
    assert!(vec_gap(place(s.part("Table")), [-30.0, 400.0, 0.0]) < 1e-9);
}

#[test]
fn detect_keeps_the_users_rows_and_warns_of_what_it_guessed() {
    // A slide without limits, and one a few degrees off square.
    let mut b = Builder::new();
    let base = b.base("Base");
    let table = b.part("Table", Pose::IDENTITY);
    let head = b.part("Head", Pose::IDENTITY);
    let lean = [
        0.0,
        (5.0f64).to_radians().sin(),
        (5.0f64).to_radians().cos(),
    ];
    let z = b.slider(
        "Leaning",
        base,
        head,
        frame([0.0, 0.0, 300.0], lean),
        (-100.0, 0.0),
    );
    let x = b.mate(
        "Endless",
        MateType::Slider,
        base,
        table,
        frame([0.0; 3], X),
        Position::default(),
        None,
    );
    b.tool_mount(head, frame([0.0, 0.0, 200.0], DOWN));
    b.table_mount(table, frame([0.0; 3], Z));
    let def = b.def();
    let tree = derive(&b.assembly, &def, &stored_frames);
    let found = detect(&tree, &def);
    assert_eq!(rows(&found.axes), [("Z", Tool, false), ("X", Table, true)]);
    assert!(warning_naming(&found.issues, z).text.contains("5.0° off"));
    assert!(
        warning_naming(&found.issues, x)
            .text
            .contains("type its travel")
    );
    assert_travel(&found.axes, "X", (-UNLIMITED, UNLIMITED));

    // The user renames Z, turns it round, types a travel and moves its zero; X gets its
    // travel typed. Detect again: the rows stay as typed, and the travel warning is gone.
    let mut def = def;
    def.axes = found.axes;
    def.axes[0] = MachineAxis {
        name: "W".into(),
        reversed: true,
        zero: -10.0,
        min: -5.0,
        max: 80.0,
        limits_from_mate: false,
        rapid: 1234.0,
        ..def.axes[0].clone()
    };
    def.axes[1] = MachineAxis {
        min: -400.0,
        max: 400.0,
        limits_from_mate: false,
        ..def.axes[1].clone()
    };
    let again = detect(&tree, &def);
    assert_eq!(again.axes, def.axes);
    assert!(!again.issues.iter().any(|i| i.text.contains("travel")));

    // A row whose mate no longer moves a mount is dropped, with a warning.
    def.axes.push(MachineAxis {
        mate: MateId::from_u128(99),
        name: "Q".into(),
        ..def.axes[1].clone()
    });
    let again = detect(&tree, &def);
    assert_eq!(again.axes.len(), 2);
    assert!(
        again
            .issues
            .iter()
            .any(|i| i.text.starts_with("Q is no longer an axis"))
    );

    // A second slide along X is U; a third X2. (The tool's side is named first.)
    let (mut b, _, _, head) = plain_mill();
    let ram = b.part("Ram", Pose::IDENTITY);
    let quill = b.part("Quill", Pose::IDENTITY);
    b.slider(
        "Ram slide",
        head,
        ram,
        frame([0.0, 0.0, 350.0], X),
        (-50.0, 50.0),
    );
    b.slider(
        "Quill slide",
        ram,
        quill,
        frame([0.0, 0.0, 340.0], X),
        (-10.0, 10.0),
    );
    b.tool_mount(quill, frame([0.0, 0.0, 300.0], DOWN));
    let names: Vec<String> = b
        .finish("Rams")
        .def()
        .axes
        .iter()
        .map(|a| a.name.clone())
        .collect();
    assert_eq!(names, ["Z", "X", "U", "Y", "X2"]);
}

/// A document with a slide unit (a rail and a carriage on a slider) as a subassembly of a
/// machine: the unit's rail fastened to the base, a head on a Z slider on its carriage.
fn with_a_subassembly(flexible: bool) -> (Document, ElementId, InstanceId, MateId) {
    let mut unit = Builder::new();
    let rail = unit.base("Rail");
    let carriage = unit.part("Carriage", Pose::translation([0.0, 0.0, 20.0]));
    let slide = unit.slider(
        "Slide",
        rail,
        carriage,
        frame([0.0, 0.0, 20.0], X),
        (-100.0, 100.0),
    );
    let mut doc = Document::empty("Machine with a slide unit");
    let mut unit_tab = Element::assembly("Slide unit");
    unit_tab.id = ElementId::from_u128(0x5ab_0001);
    unit_tab.assembly = unit.assembly;
    let unit_id = unit_tab.id;
    doc.elements.push(unit_tab);

    // The machine: ids of its own, clear of the unit's.
    let part = |n: u128, pose: Pose| {
        let mut i = Instance::new(
            InstanceId::from_u128(0x77_0000 + n),
            InstanceSource::Part {
                element: ElementId::from_u128(0x5ab_00ff),
                part: cadrs_core::PartId::new(cadrs_core::FeatureId::from_u128(n), 0),
            },
            pose,
        );
        i.index = 1;
        i
    };
    let mut base = part(1, Pose::IDENTITY);
    base.fixed = true;
    let head = part(2, Pose::translation([0.0, 0.0, 300.0]));
    let placed = Pose::rotation_about([0.0; 3], Z, std::f64::consts::FRAC_PI_2)
        .then(&Pose::translation([0.0, 0.0, 100.0]));
    let mut sub = Instance::new(
        InstanceId::from_u128(0x77_0003),
        InstanceSource::Assembly { element: unit_id },
        placed,
    );
    sub.index = 1;
    sub.flexible = flexible;
    let (rail_here, carriage_here) = (occurrence(sub.id, rail), occurrence(sub.id, carriage));
    // Connectors from world frames: the unit's parts are where its tab has them, moved by
    // the subassembly's placement.
    let on = |instance: InstanceId, pose: Pose, at: &ConnectorFrame| {
        MateConnector::at(instance, at.moved(&pose.inverse()))
    };
    let carriage_pose = Pose::translation([0.0, 0.0, 20.0]).then(&placed);
    let at_rail = frame([0.0, 0.0, 100.0], Z);
    let at_head = frame([0.0, 50.0, 300.0], Z);
    let mut z = cadrs_core::assembly::mate::Mate::new(
        MateType::Slider,
        on(carriage_here, carriage_pose, &at_head),
        on(head.id, head.pose, &at_head),
    );
    z.limits = Some(MateLimits {
        z: Some((-150.0, 50.0)),
        ..MateLimits::default()
    });
    let z_id = MateId::from_u128(0x77_1002);
    let mates = vec![
        MateFeature::new(
            MateId::from_u128(0x77_1001),
            "Fastened 1",
            MateKind::Mate(cadrs_core::assembly::mate::Mate::new(
                MateType::Fastened,
                on(base.id, base.pose, &at_rail),
                on(rail_here, placed, &at_rail),
            )),
        ),
        MateFeature::new(z_id, "Z", MateKind::Mate(z)),
    ];
    let def = MachineDef {
        tool_mount: Some(on(head.id, head.pose, &frame([0.0, 0.0, 250.0], DOWN))),
        table_mount: Some(on(base.id, base.pose, &frame([0.0; 3], Z))),
        ..MachineDef::default()
    };
    let head_id = head.id;
    let mut tab = Element::assembly("Machine");
    tab.id = ElementId::from_u128(0x5ab_0002);
    tab.assembly = Assembly {
        instances: vec![base, head, sub],
        mates,
        machine: Some(Box::new(def)),
        ..Default::default()
    };
    let id = tab.id;
    doc.elements.push(tab);
    let slide_here = MateId(occurrence(InstanceId::from_u128(0x77_0003), InstanceId(slide.0)).0);
    (doc, id, head_id, slide_here)
}

#[test]
fn a_flexible_subassemblys_mate_is_an_axis_and_a_rigid_ones_is_not() {
    // Flexible: the unit's slider acts in the machine, under its id there. The unit is
    // turned a quarter about Z, so its slide runs along Y.
    let (doc, el, head, slide) = with_a_subassembly(true);
    let r = resolve(&doc, el, &stored_frames).unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert_eq!(rows(&r.axes), [("Y", Tool, false), ("Z", Tool, false)]);
    assert_eq!(r.axes[0].mate, slide);
    assert_travel(&r.axes, "Y", (-100.0, 100.0));
    let asm = doc.element(el).unwrap().assembly_model().unwrap();
    let flat = flat_model(&doc, asm);
    check_against_the_solver("flexible", &flat, asm.machine.as_deref().unwrap(), 10);
    let at = r.poses(&[40.0, -30.0], &[]);
    let head_at = at.iter().find(|(i, _)| *i == head).unwrap().1.translation;
    assert!(vec_gap(head_at, [0.0, 40.0, 270.0]) < 1e-9);

    // Rigid: the unit is one body with the base, and Z is the only axis.
    let (doc, el, _, _) = with_a_subassembly(false);
    let r = resolve(&doc, el, &stored_frames).unwrap();
    assert_eq!(rows(&r.axes), [("Z", Tool, false)]);
    let asm = doc.element(el).unwrap().assembly_model().unwrap();
    let flat = flat_model(&doc, asm);
    assert_eq!(flat.instances.len(), 4);
    let bodies: Vec<usize> = r.tree.instances.iter().map(|p| p.body).collect();
    // The base, the head, and the unit's two parts with the base.
    assert_eq!(bodies, [0, 1, 0, 0]);
    check_against_the_solver("rigid", &flat, asm.machine.as_deref().unwrap(), 6);
}

fn machine_of(doc: &Document, el: ElementId) -> Option<&MachineDef> {
    doc.element(el)
        .and_then(|e| e.assembly_model())
        .and_then(|a| a.machine.as_deref())
}

fn set(el: ElementId, def: MachineDef) -> SetMachineDef {
    SetMachineDef {
        element: el,
        def: Some(Box::new(def)),
        label: "Set up machine".into(),
    }
}

#[test]
fn set_machine_def_is_one_undo_step() {
    let s = samples::trunnion_ac();
    let def = s.def().clone();
    let (mut doc, el) = s.document();
    doc.element_mut(el).unwrap().assembly.machine = None;
    let mut h = History::default();
    h.execute(&mut doc, &set(el, def.clone())).unwrap();
    assert_eq!(machine_of(&doc, el), Some(&def));
    assert_eq!(h.undo_label(), Some("Set up machine"));
    // An edit: X parks at 50.
    let mut edited = def.clone();
    edited.axes.iter_mut().find(|a| a.name == "X").unwrap().home = 50.0;
    let edit = SetMachineDef {
        label: "Edit axis X".into(),
        ..set(el, edited.clone())
    };
    assert_eq!(edit.label(), "Edit axis X");
    h.execute(&mut doc, &edit).unwrap();
    assert_eq!(machine_of(&doc, el), Some(&edited));
    h.undo(&mut doc);
    assert_eq!(machine_of(&doc, el), Some(&def));
    h.undo(&mut doc);
    assert_eq!(machine_of(&doc, el), None);
    h.redo(&mut doc);
    h.redo(&mut doc);
    assert_eq!(machine_of(&doc, el), Some(&edited));
    // Removing it.
    h.execute(
        &mut doc,
        &SetMachineDef {
            element: el,
            def: None,
            label: "Remove machine".into(),
        },
    )
    .unwrap();
    assert_eq!(machine_of(&doc, el), None);
    assert!(resolve(&doc, el, &stored_frames).is_err());
    h.undo(&mut doc);
    // The stored definition resolves as the sample does.
    let r = resolve(&doc, el, &stored_frames).unwrap();
    assert_eq!(r.axes, edited.axes);
    // A chain handed in with the controller is not kept: it is derived.
    let mut with_chain = def.clone();
    with_chain.controller.chain = Some(r.chain().clone());
    h.execute(&mut doc, &set(el, with_chain)).unwrap();
    assert!(machine_of(&doc, el).unwrap().controller.chain.is_none());
}

#[test]
fn set_machine_def_refuses_what_does_not_fit() {
    let s = samples::router();
    let (mut doc, el) = s.document();
    let good = s.def().clone();
    let mut h = History::default();
    let mut refuse = |change: &dyn Fn(&mut MachineDef), why: &str| {
        let mut def = good.clone();
        change(&mut def);
        let before = doc.clone();
        let e = h.execute(&mut doc, &set(el, def)).unwrap_err();
        assert!(e.to_string().contains(why), "{e} (wanted: {why})");
        assert_eq!(doc, before);
    };
    refuse(&|d| d.axes[1].name = "X".into(), "two axes are named X");
    refuse(&|d| d.axes[0].name = "  ".into(), "an axis needs a name");
    refuse(
        &|d| d.axes[0].mate = MateId::from_u128(5),
        "mate of axis X is not there",
    );
    refuse(
        &|d| d.axes[0].dof = Dof::Angle,
        "mate of axis X is not there",
    );
    refuse(
        &|d| d.axes[0].min = d.axes[0].max,
        "travel from less to more",
    );
    refuse(&|d| d.axes[0].max = f64::INFINITY, "not a number");
    refuse(&|d| d.axes[2].zero = f64::NAN, "not a number");
    refuse(&|d| d.axes[0].continuous = true, "is a slide");
    refuse(&|d| d.axes[0].rapid = -1.0, "rates");
    refuse(&|d| d.axes.push(d.axes[0].clone()), "two axes are named X");
    refuse(
        &|d| {
            let mut twice = d.axes[0].clone();
            twice.name = "U".into();
            d.axes.push(twice);
        },
        "already an axis",
    );
    refuse(
        &|d| d.tool_mount.as_mut().unwrap().instance = InstanceId::from_u128(4),
        "tool mount is on an instance that is not there",
    );
    refuse(&|d| d.enclosure.push(InstanceId::from_u128(4)), "enclosure");
    refuse(
        &|d| d.tool_change.position.push(("Q".into(), 0.0)),
        "names an axis Q",
    );
    refuse(
        &|d| d.tool_change.magazine = Some(ActuatorId::from_u128(1)),
        "not one of the actuators",
    );
    refuse(
        &|d| {
            d.tool_change.pockets = vec![
                Pocket {
                    number: 1,
                    value: 0.0,
                },
                Pocket {
                    number: 1,
                    value: 45.0,
                },
            ]
        },
        "numbered 1",
    );
    let a_mate = good.axes[3].mate;
    refuse(
        &|d| {
            d.actuators.push(Actuator {
                id: ActuatorId::from_u128(1),
                name: "Chuck".into(),
                mate: a_mate,
                dof: Dof::Angle,
                positions: vec![],
                rest: 0,
            })
        },
        "already an axis or an actuator",
    );
    // A continuous turn needs no travel.
    assert!(good.axes[3].continuous && good.axes[3].min == good.axes[3].max);
    h.execute(&mut doc, &set(el, good.clone())).unwrap();
    // Not on a Part Studio.
    let mut studio = Element::part_studio("Parts");
    studio.id = ElementId::from_u128(0xbad);
    doc.elements.push(studio);
    assert!(
        h.execute(&mut doc, &set(ElementId::from_u128(0xbad), good))
            .is_err()
    );
}

/// The 4th-axis router with a door (an actuator), a tool carousel (the magazine, with
/// pockets) and an enclosure.
fn router_with_extras() -> (Sample, [MateId; 2], [ActuatorId; 2]) {
    let mut b = Builder::new();
    let base = b.base("Base");
    let gantry = b.part("Gantry", Pose::IDENTITY);
    let head = b.part("Head", Pose::IDENTITY);
    let door = b.part("Door", Pose::IDENTITY);
    let carousel = b.part("Carousel", Pose::IDENTITY);
    b.slider(
        "X",
        base,
        gantry,
        frame([0.0, 0.0, 100.0], X),
        (-300.0, 300.0),
    );
    b.slider(
        "Z",
        gantry,
        head,
        frame([0.0, 0.0, 250.0], Z),
        (-120.0, 0.0),
    );
    let door_slide = b.slider(
        "Door slide",
        base,
        door,
        frame([0.0, -400.0, 0.0], X),
        (0.0, 600.0),
    );
    let turn = b.revolute(
        "Carousel turn",
        base,
        carousel,
        frame([400.0, 300.0, 300.0], Z),
        None,
    );
    b.tool_mount(head, frame([0.0, 0.0, 200.0], DOWN));
    b.table_mount(base, frame([0.0; 3], Z));
    let mut s = b.finish("Router with a door");
    let ids = [ActuatorId::from_u128(0xd00), ActuatorId::from_u128(0xca0)];
    let mut def = s.def().clone();
    def.actuators = vec![
        Actuator {
            id: ids[0],
            name: "Door".into(),
            mate: door_slide,
            dof: Dof::Z,
            positions: vec![("Closed".into(), 0.0), ("Open".into(), 600.0)],
            rest: 0,
        },
        Actuator {
            id: ids[1],
            name: "Magazine".into(),
            mate: turn,
            dof: Dof::Angle,
            positions: vec![],
            rest: 0,
        },
    ];
    def.enclosure = vec![door];
    def.tool_change = ToolChange {
        position: vec![("Z".into(), 0.0), ("X".into(), 300.0)],
        magazine: Some(ids[1]),
        pockets: (0..8)
            .map(|k| Pocket {
                number: k + 1,
                value: 45.0 * k as f64,
            })
            .collect(),
    };
    s.assembly.machine = Some(Box::new(def));
    (s, [door_slide, turn], ids)
}

#[test]
fn actuators_move_what_the_axes_do_not() {
    let (s, mates, ids) = router_with_extras();
    let r = s.resolve().unwrap();
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    // Both were on offer before they were named.
    let offered: Vec<MateId> = detect(&r.tree, &MachineDef::default())
        .off_path
        .iter()
        .map(|j| j.mate)
        .collect();
    assert_eq!(offered, mates);
    let at = r.poses(&[100.0, -50.0], &[(ids[0], 600.0), (ids[1], 90.0)]);
    let pose = |name: &str| at.iter().find(|(i, _)| *i == s.part(name)).unwrap().1;
    assert!(vec_gap(pose("Door").translation, [600.0, 0.0, 0.0]) < 1e-9);
    assert!(vec_gap(pose("Head").translation, [100.0, 0.0, -50.0]) < 1e-9);
    // The carousel a quarter turn about (400, 300): its origin goes to (700, −100).
    assert!(vec_gap(pose("Carousel").translation, [700.0, -100.0, 0.0]) < 1e-9);
    let theirs = solved(
        &s.assembly,
        &r,
        &driven(
            &r,
            &[100.0, -50.0],
            &[(mates[0], Dof::Z, 600.0), (mates[1], Dof::Angle, 90.0)],
        ),
    );
    assert_same_poses("door and carousel", &at, &theirs);
    // The tool change position: Z up and X at its end, wherever the machine was.
    let change = r.at_named(&[100.0, -50.0], &s.def().tool_change.position);
    assert_eq!(change, [300.0, 0.0]);
}

#[test]
fn deleting_a_mate_or_an_instance_drops_what_the_machine_had_on_it() {
    let (s, mates, ids) = router_with_extras();
    let (mut doc, el) = s.document();
    let mut h = History::default();
    let whole = machine_of(&doc, el).unwrap().clone();

    // The Z mate: its axis goes, and its entry in the tool change position.
    h.execute(
        &mut doc,
        &DeleteMateFeatures {
            element: el,
            mates: vec![s.mate("Z")],
        },
    )
    .unwrap();
    let def = machine_of(&doc, el).unwrap();
    assert_eq!(rows(&def.axes), [("X", Tool, false)]);
    assert_eq!(def.tool_change.position, [("X".to_string(), 300.0)]);
    assert_eq!(def.actuators.len(), 2);
    assert!(def.tool_mount.is_some());
    h.undo(&mut doc);
    assert_eq!(machine_of(&doc, el), Some(&whole));

    // The carousel's mate: the magazine and its pockets go; the door stays.
    h.execute(
        &mut doc,
        &DeleteMateFeatures {
            element: el,
            mates: vec![mates[1]],
        },
    )
    .unwrap();
    let def = machine_of(&doc, el).unwrap();
    assert_eq!(def.actuators.len(), 1);
    assert_eq!(def.actuators[0].id, ids[0]);
    assert_eq!(def.tool_change.magazine, None);
    assert!(def.tool_change.pockets.is_empty());
    assert_eq!(def.axes.len(), 2);
    h.undo(&mut doc);

    // The head and the door: the tool mount, Z (the head's mate goes with it), the door's
    // actuator and the enclosure's entry.
    h.execute(
        &mut doc,
        &DeleteInstances {
            element: el,
            instances: vec![s.part("Head"), s.part("Door")],
        },
    )
    .unwrap();
    let def = machine_of(&doc, el).unwrap();
    assert!(def.tool_mount.is_none() && def.table_mount.is_some());
    assert_eq!(rows(&def.axes), [("X", Tool, false)]);
    assert_eq!(def.actuators.len(), 1);
    assert_eq!(def.actuators[0].id, ids[1]);
    assert!(def.enclosure.is_empty());
    assert_eq!(def.tool_change.magazine, Some(ids[1]));
    h.undo(&mut doc);
    assert_eq!(machine_of(&doc, el), Some(&whole));
}

/// Whether RON text has an infinity or a NaN written as a number.
fn has_no_number(text: &str) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| matches!(word, "inf" | "NaN" | "nan" | "infinity"))
}

#[test]
fn the_definition_is_saved_with_the_document_and_old_documents_load_without_one() {
    let (s, _, _) = router_with_extras();
    let (doc, el) = s.document();
    let pretty = || ron::ser::PrettyConfig::default();

    // RON, as the store writes documents.
    let text = ron::ser::to_string_pretty(&doc, pretty()).unwrap();
    assert!(text.contains("machine: Some("));
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, doc);
    assert_eq!(machine_of(&back, el), Some(s.def()));
    // No infinity is written (the store's RON would not read one back the same everywhere):
    // every number of a definition is finite, the unlimited travel included.
    assert!(!has_no_number(&text), "{text}");
    // MessagePack, as a shared document travels.
    let packed = cadrs_core::collab::pack(&doc).unwrap();
    let unpacked: Document = cadrs_core::collab::unpack(&packed).unwrap();
    assert_eq!(unpacked, doc);

    // A document without the field: an assembly written before there were machines.
    let mut plain = doc.clone();
    plain.element_mut(el).unwrap().assembly.machine = None;
    let old = ron::ser::to_string_pretty(&plain, pretty()).unwrap();
    assert!(!old.contains("machine"));
    let loaded: Document = ron::from_str(&old).unwrap();
    assert_eq!(machine_of(&loaded, el), None);
    assert_eq!(loaded, plain);
    // An assembly with nothing but a machine definition is not empty (it is written).
    let only = Assembly {
        machine: Some(Box::new(MachineDef::default())),
        ..Default::default()
    };
    assert!(!only.is_empty() && Assembly::default().is_empty());

    // A slide without limits is given a finite travel, which is what gets stored.
    let mut b = Builder::new();
    let base = b.base("Base");
    let head = b.part("Head", Pose::IDENTITY);
    b.mate(
        "Endless",
        MateType::Slider,
        base,
        head,
        frame([0.0; 3], Z),
        Position::default(),
        None,
    );
    b.tool_mount(head, frame([0.0, 0.0, 100.0], DOWN));
    b.table_mount(base, frame([0.0; 3], Z));
    let endless = b.finish("Endless");
    assert!(
        endless
            .def()
            .axes
            .iter()
            .all(|a| a.min.is_finite() && a.max.is_finite())
    );
    let (doc, _) = endless.document();
    let text = ron::ser::to_string_pretty(&doc, pretty()).unwrap();
    assert!(!has_no_number(&text));
    assert_eq!(ron::from_str::<Document>(&text).unwrap(), doc);
}

/// The CNC machines sample (`samples::cnc_machines`): its three mills, with the tool mount on
/// the spindle's nose and the table mount on the table's top, the chuck's face and the
/// platter's top.
#[test]
fn the_cnc_machines_sample_detects_its_axes() {
    use cadrs_core::samples::cnc_machines::assemblies::{mill3, mill4, mill5};
    use cadrs_core::samples::cnc_machines::parts;
    let doc = cadrs_core::samples::cnc_machines::document().unwrap();
    let nose = frame([0.0, 0.0, parts::SPINDLE_NOSE], DOWN);
    let chuck_face = frame([-95.0, 0.0, parts::CHUCK_AXIS[2]], X);
    let platter_top = frame([0.0, 0.0, parts::PLATTER_TOP], Z);
    /// A mill: its tab, its spindle, where its work goes, and the axes it should have.
    type Row = (&'static str, Side, bool);
    type Work = (InstanceId, ConnectorFrame);
    type Case = (ElementId, InstanceId, Work, &'static [Row]);
    let cases: [Case; 3] = [
        (
            mill3::ASSEMBLY,
            mill3::SPINDLE,
            (mill3::TABLE, frame([0.0; 3], Z)),
            &[("Z", Tool, false), ("Y", Table, true), ("X", Table, true)],
        ),
        (
            mill4::ASSEMBLY,
            mill4::SPINDLE,
            (mill4::CHUCK, chuck_face),
            &[
                ("Z", Tool, false),
                ("Y", Table, true),
                ("X", Table, true),
                ("A", Table, true),
            ],
        ),
        (
            mill5::ASSEMBLY,
            mill5::SPINDLE,
            (mill5::PLATTER, platter_top),
            &[
                ("Z", Tool, false),
                ("B", Tool, false),
                ("Y", Table, true),
                ("X", Table, true),
                ("C", Table, true),
            ],
        ),
    ];
    for (el, spindle, (table, table_at), want) in cases {
        let asm = doc.element(el).unwrap().assembly_model().unwrap();
        let name = &doc.element(el).unwrap().name;
        // Every instance sits at the identity, so a frame of the assembly is the instance's.
        let def = MachineDef {
            tool_mount: Some(MateConnector::at(spindle, nose)),
            table_mount: Some(MateConnector::at(table, table_at)),
            ..MachineDef::default()
        };
        let flat = flat_model(&doc, asm);
        let tree = derive(&flat, &def, &stored_frames);
        let found = detect(&tree, &def);
        assert!(found.issues.is_empty(), "{name}: {:?}", found.issues);
        assert_eq!(rows(&found.axes), want, "{name}");
        // Each axis is the mate of its name.
        for a in &found.axes {
            assert_eq!(asm.mate(a.mate).unwrap().name, a.name, "{name}");
        }
        let r = resolve_model(&flat, &def, &stored_frames).unwrap();
        assert!(r.chain().is_mill(), "{name}");
        assert!(vec_gap(r.chain().tool_mount.origin, [0.0, 0.0, 400.0]) < 1e-9);
        assert!(vec_gap(r.chain().tool_mount.z, Z) < 1e-12);
        check_against_the_solver(name, &flat, &def, 5);
        if el == mill4::ASSEMBLY {
            // The magazine's turn is left for an actuator.
            assert_eq!(found.off_path.len(), 1);
            assert_eq!(found.off_path[0].mate, mill4::MAGAZINE);
            assert_eq!(found.off_path[0].name, "Magazine");
        } else {
            assert!(found.off_path.is_empty(), "{name}");
        }
    }
}
