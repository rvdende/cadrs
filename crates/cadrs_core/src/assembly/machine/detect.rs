//! Proposing a machine's axes from its kinematic tree ([`detect`]).
//!
//! The axes are the joints that move one mount and not the other, each driven by itself: the
//! tool side's from the base outward, then the table side's. A row the definition already has
//! for a joint is kept as the user left it (name, sign, zero, typed travel, rates); a joint
//! without one gets a proposal:
//!
//! - **Name.** A machine with slides and at most three turns is lettered: a slide X, Y or Z by
//!   the assembly axis it runs along (a second one along the same axis U, V or W), a turn A, B
//!   or C by the axis it turns about. A joint more than [`SQUARE`] degrees off every axis takes
//!   the nearest one's letter, with a warning. A machine with no slides, or more than three
//!   turns, is an arm: J1, J2, … from the base.
//! - **Sign.** A rising value moves the tool the plus way relative to the work, so an axis
//!   that carries the table is reversed against its own motion: an X table moves to −X as X
//!   rises, and a rotary table turns left-handed. An arm's joints keep their mates' signs.
//! - **Zero** is the mate's value now, so the machine reads 0 on every new axis where the
//!   document has it; it parks there too.
//! - **Travel** is the mate's limits. A turn without limits is continuous; a slide without
//!   them gets a travel of ±[`UNLIMITED`] and an issue asking for the real one.

use super::super::mate::{Dof, MateId};
use super::derive::{Motion, Role, Tree, TreeJoint};
use super::{Issue, JointKind, MachineAxis, MachineDef, Side, from_mate_units};

/// How far (degrees) a joint may be off an assembly axis and still be along it.
pub const SQUARE: f64 = 1.0;

/// The travel (mm, each way from zero) of a slide whose mate has no limits.
pub const UNLIMITED: f64 = 1e6;

/// A joint that is driven by itself and moves neither mount: what an actuator can be made of.
#[derive(Debug, Clone, PartialEq)]
pub struct FreeJoint {
    pub mate: MateId,
    pub dof: Dof,
    pub kind: JointKind,
    /// The mate's name.
    pub name: String,
}

/// What [`detect`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// The machine's axes: the tool side's from the base outward, then the table side's.
    pub axes: Vec<MachineAxis>,
    /// The joints left for actuators.
    pub off_path: Vec<FreeJoint>,
    /// The tree's issues, then those of the axes.
    pub issues: Vec<Issue>,
}

/// The assembly axis `d` is nearest to (0 X, 1 Y, 2 Z), whether it runs the plus way along
/// it, and how far off it is (degrees).
fn nearest_axis(d: [f64; 3]) -> (usize, bool, f64) {
    let k = (0..3)
        .max_by(|a, b| d[*a].abs().total_cmp(&d[*b].abs()))
        .unwrap_or(2);
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-300);
    let off = (d[k].abs() / len).clamp(0.0, 1.0).acos().to_degrees();
    (k, d[k] >= 0.0, off)
}

/// The travel of an axis whose mate runs from `lo` to `hi` (mm or radians): the least and
/// greatest axis value (mm or degrees).
fn travel(joint: &TreeJoint, zero: f64, reversed: bool, (lo, hi): (f64, f64)) -> (f64, f64) {
    let s = if reversed { -1.0 } else { 1.0 };
    let a = s * from_mate_units(joint.dof, lo - zero);
    let b = s * from_mate_units(joint.dof, hi - zero);
    (a.min(b), a.max(b))
}

/// The machine's axes for the mounts `tree` was derived with: the rows of `def` for the
/// joints that are still axes, and proposals for the rest (see the module).
pub fn detect(tree: &Tree, def: &MachineDef) -> Detection {
    let mut issues = tree.issues.clone();
    // The joints that are axes, in the order of the rows.
    let mut joints: Vec<(usize, Side)> = Vec::new();
    for (mount, role, side) in [
        (&tree.tool, Role::Tool, Side::Tool),
        (&tree.table, Role::Table, Side::Table),
    ] {
        for j in mount.iter().flat_map(|m| m.path.iter().copied()) {
            if tree.joints[j].role == role && tree.joints[j].motion == Motion::Free {
                joints.push((j, side));
            }
        }
    }
    let kept = |j: usize| def.axis(tree.joints[j].mate, tree.joints[j].dof);
    let slides = joints
        .iter()
        .filter(|(j, _)| !tree.joints[*j].is_rotary())
        .count();
    let arm = slides == 0 || joints.len() - slides > 3;
    // The names in use: the kept rows' first, so no proposal takes one.
    let mut names: Vec<String> = joints
        .iter()
        .filter_map(|(j, _)| kept(*j).map(|a| a.name.clone()))
        .collect();
    let mut free_name = |candidates: &mut dyn Iterator<Item = String>| -> String {
        let mut name = format!("Axis {}", names.len() + 1);
        for n in candidates {
            if !names.contains(&n) {
                name = n;
                break;
            }
        }
        names.push(name.clone());
        name
    };

    let mut axes: Vec<MachineAxis> = Vec::with_capacity(joints.len());
    for (place, (j, side)) in joints.iter().enumerate() {
        let joint = &tree.joints[*j];
        let rotary = joint.is_rotary();
        let mut axis = match kept(*j) {
            Some(a) => MachineAxis {
                side: *side,
                ..a.clone()
            },
            None => {
                let (k, plus, off) = nearest_axis(joint.dir);
                let (name, reversed) = if arm {
                    let name = free_name(&mut (place + 1..).map(|n| format!("J{n}")));
                    (name, false)
                } else {
                    let letters: [&str; 2] = if rotary {
                        [["A", "B", "C"][k], ""]
                    } else {
                        [["X", "Y", "Z"][k], ["U", "V", "W"][k]]
                    };
                    let first = letters[0];
                    let name = free_name(
                        &mut letters
                            .iter()
                            .filter(|l| !l.is_empty())
                            .map(|l| l.to_string())
                            .chain((2..).map(|n| format!("{first}{n}"))),
                    );
                    if off > SQUARE {
                        issues.push(
                            Issue::warning(format!(
                                "{} is {off:.1}° off the assembly's {} axis: it is named {name} \
                                 all the same",
                                joint.name,
                                ["X", "Y", "Z"][k]
                            ))
                            .mates([joint.mate]),
                        );
                    }
                    // The tool goes the plus way as the value rises: the table the other.
                    (name, plus == (*side == Side::Table))
                };
                let (rapid, max_feed) = if rotary {
                    (7_200.0, 3_600.0)
                } else {
                    let or = |v: f64, d: f64| if v.is_finite() && v > 0.0 { v } else { d };
                    (
                        or(def.controller.rapid_rate, 10_000.0),
                        or(def.controller.max_feed, 5_000.0),
                    )
                };
                MachineAxis {
                    mate: joint.mate,
                    dof: joint.dof,
                    name,
                    side: *side,
                    reversed,
                    zero: joint.present,
                    home: 0.0,
                    min: 0.0,
                    max: 0.0,
                    limits_from_mate: true,
                    continuous: false,
                    rapid,
                    max_feed,
                }
            }
        };
        if axis.limits_from_mate {
            match joint.limits {
                Some(limits) => {
                    (axis.min, axis.max) = travel(joint, axis.zero, axis.reversed, limits);
                    axis.continuous = false;
                }
                None if rotary => {
                    (axis.min, axis.max) = (0.0, 0.0);
                    axis.continuous = true;
                }
                None => {
                    (axis.min, axis.max) = (-UNLIMITED, UNLIMITED);
                    axis.continuous = false;
                    issues.push(
                        Issue::warning(format!(
                            "{} ({}) has no limits: type its travel",
                            axis.name, joint.name
                        ))
                        .mates([joint.mate]),
                    );
                }
            }
            // It parks within its travel.
            if !axis.continuous {
                axis.home = axis.home.clamp(axis.min, axis.max);
            }
        }
        axes.push(axis);
    }

    // The definition's rows that are axes no more.
    for a in &def.axes {
        if !axes.iter().any(|b| b.mate == a.mate && b.dof == a.dof) {
            issues.push(
                Issue::warning(format!(
                    "{} is no longer an axis: its mate does not move the tool or the table by \
                     itself",
                    a.name
                ))
                .mates([a.mate]),
            );
        }
    }
    // Its actuators that cannot be moved.
    for a in &def.actuators {
        let free = tree.joint_of(a.mate, a.dof).is_some_and(|j| {
            let joint = &tree.joints[j];
            joint.motion == Motion::Free && matches!(joint.role, Role::Off | Role::Both)
        });
        if !free {
            issues.push(
                Issue::warning(format!(
                    "{} cannot be moved: its mate is not a joint of the machine that moves by \
                     itself",
                    a.name
                ))
                .mates([a.mate]),
            );
        }
    }
    let off_path = tree
        .free_off_path()
        .map(|(_, j)| FreeJoint {
            mate: j.mate,
            dof: j.dof,
            kind: j.kind,
            name: j.name.clone(),
        })
        .collect();
    Detection {
        axes,
        off_path,
        issues,
    }
}
