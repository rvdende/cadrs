//! The three machines as assemblies of [`super::parts`]: [`mill3`], [`mill4`] and [`mill5`].
//! Every instance sits at the identity (the parts are modelled at home), the Base is fixed,
//! and each axis is a mate named after it (each machine's `AXES`):
//!
//! | Mate | Type | Carries, on | At, along / about | Limits | In |
//! |---|---|---|---|---|---|
//! | `Y` | Slider | Saddle, on the Base | (0, 0, −110), +Y | ±150 | all |
//! | `X` | Slider | Table, on the Saddle | (0, 0, −50), +X | ±250 | all |
//! | `Z` | Slider | Head, on the Column | (0, 310, 650), +Z | −350…0 | 3- and 4-axis |
//! | `A` | Revolute | Chuck, on the Rotary body | (−140, 0, 100), +X | none | 4-axis |
//! | `Magazine` | Revolute | Carousel, on the Magazine bracket | (360, 360, 500), +Z | none | 4-axis |
//! | `C` | Revolute | Platter, on the Rotary table base | (0, 0, 80), +Z | none | 5-axis |
//! | `Z` | Slider | Z slide, on the Column | (0, 310, 650), +Z | −300…0 | 5-axis |
//! | `B` | Revolute | Swivel head, on the Z slide | (0, 170, 600), +Y | ±100° | 5-axis |
//!
//! The rest is held by Fastened mates ("Fastened 1", …): the Column to the Base, the Spindle
//! and the Motor to the head that carries them, the 4th axis's body and the rotary table's
//! base to the Table, the Magazine bracket to the Column.
//!
//! A mate's two connectors are one frame at home. The first is on the part the mate carries
//! and the second on the part nearer the Base: the solver's snap (behind Animate, Reset, Apply
//! limit position and the named positions) places the first connector's instance, and that
//! must be the carried part, or a turn of a rotary axis throws its carrier round instead. A
//! mate's value is where the second connector is in the first's frame, so the frame's Z points
//! **against** the axis's direction: the value is 0 at home, and a positive value moves what
//! the mate carries along (or counter-clockwise about) the direction in the table.
//!
//! The 4-axis mill has the named positions "Pocket 1" … "Pocket 8": the `Magazine` turned
//! 45° on for each (they hold only that mate's value, so applying one leaves the axes alone).

use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed};
use crate::assembly::connector::{ConnectorFrame, MateConnector};
use crate::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateType};
use crate::assembly::positions::{MateValue, NamedPosition, NamedPositionId, SetNamedPosition};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::CommandError;
use crate::ids::{ElementId, PartId};
use crate::studio::Studio;

use super::parts;
use super::{MACHINE_PARTS, PREFIX};

const ALONG_X: [f64; 3] = [1.0, 0.0, 0.0];
const ALONG_Y: [f64; 3] = [0.0, 1.0, 0.0];
const ALONG_Z: [f64; 3] = [0.0, 0.0, 1.0];

/// The instance numbered `n` of the mill with `axes` axes.
const fn inst(axes: u128, n: u128) -> InstanceId {
    InstanceId::from_u128(PREFIX | (5 << 92) | (axes << 8) | n)
}

/// The mate numbered `n` of the mill with `axes` axes.
const fn mate(axes: u128, n: u128) -> MateId {
    MateId::from_u128(PREFIX | (6 << 92) | (axes << 8) | n)
}

/// A machine axis: the mate that carries `child` on `parent`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub mate: MateId,
    pub name: &'static str,
    /// The instance nearer the Base (the mate's second connector's).
    pub parent: InstanceId,
    /// The instance the axis moves (the mate's first connector's).
    pub child: InstanceId,
    /// A point on the axis at home, in the assembly's coordinates.
    pub origin: [f64; 3],
    /// The way a positive value moves `child`: along it (linear) or counter-clockwise about it
    /// (rotary).
    pub direction: [f64; 3],
    /// A Revolute, valued in radians ([`Dof::Angle`]); else a Slider, in mm ([`Dof::Z`]).
    pub rotary: bool,
    /// The travel: mm, or degrees for a rotary axis (`None`: it turns without end).
    pub limits: Option<(f64, f64)>,
}

impl Axis {
    /// The mate's degree of freedom that is the axis's value.
    pub fn dof(&self) -> Dof {
        if self.rotary { Dof::Angle } else { Dof::Z }
    }
}

/// `Y`: the Saddle on the Base.
const fn y(axes: u128) -> Axis {
    Axis {
        mate: mate(axes, 1),
        name: "Y",
        parent: inst(axes, 1),
        child: inst(axes, 3),
        origin: [0.0, 0.0, -110.0],
        direction: ALONG_Y,
        rotary: false,
        limits: Some((-150.0, 150.0)),
    }
}

/// `X`: the Table on the Saddle.
const fn x(axes: u128) -> Axis {
    Axis {
        mate: mate(axes, 2),
        name: "X",
        parent: inst(axes, 3),
        child: inst(axes, 4),
        origin: [0.0, 0.0, -50.0],
        direction: ALONG_X,
        rotary: false,
        limits: Some((-250.0, 250.0)),
    }
}

/// `Z`: the instance `child` on the Column's face, `down` below home at the most.
const fn z(axes: u128, child: InstanceId, down: f64) -> Axis {
    Axis {
        mate: mate(axes, 3),
        name: "Z",
        parent: inst(axes, 2),
        child,
        origin: [0.0, parts::COLUMN_FACE, 650.0],
        direction: ALONG_Z,
        rotary: false,
        limits: Some((-down, 0.0)),
    }
}

/// A rotary axis.
const fn rotary(
    mate: MateId,
    name: &'static str,
    (parent, child): (InstanceId, InstanceId),
    origin: [f64; 3],
    direction: [f64; 3],
    limits: Option<(f64, f64)>,
) -> Axis {
    Axis {
        mate,
        name,
        parent,
        child,
        origin,
        direction,
        rotary: true,
        limits,
    }
}

/// One machine being put together: where its instances and mates go.
struct Machine<'a> {
    s: &'a mut dyn Studio,
    element: ElementId,
    axes: u128,
    /// The Fastened mates made so far.
    fastened: u128,
}

impl Machine<'_> {
    /// Inserts `part` at the identity as the instance `id`.
    fn insert(&mut self, id: InstanceId, part: PartId) -> Result<(), CommandError> {
        self.s.run(&InsertInstance {
            element: self.element,
            instance: Instance::new(
                id,
                InstanceSource::Part {
                    element: MACHINE_PARTS,
                    part,
                },
                Pose::IDENTITY,
            ),
        })
    }

    /// A mate of `child` (its first connector) to `parent`, both connectors the frame at
    /// `origin` with its Z along `z`.
    #[allow(clippy::too_many_arguments)]
    fn join(
        &mut self,
        id: MateId,
        name: &str,
        mate_type: MateType,
        (parent, child): (InstanceId, InstanceId),
        origin: [f64; 3],
        z: [f64; 3],
        limits: Option<MateLimits>,
    ) -> Result<(), CommandError> {
        // The frame's X: the model's X, or its Y for a Z along X.
        let x = if z[0] != 0.0 { ALONG_Y } else { ALONG_X };
        let frame = ConnectorFrame::new(origin, z, x);
        let mut m = Mate::new(
            mate_type,
            MateConnector::at(child, frame),
            MateConnector::at(parent, frame),
        );
        m.limits = limits;
        self.s.run(&AddMateFeature {
            element: self.element,
            feature: MateFeature::new(id, name, MateKind::Mate(m)),
            poses: Vec::new(),
        })
    }

    /// A Fastened mate of `child` to `parent` at `origin`, named "Fastened n".
    fn fasten(
        &mut self,
        parent: InstanceId,
        child: InstanceId,
        origin: [f64; 3],
    ) -> Result<(), CommandError> {
        self.fastened += 1;
        self.join(
            mate(self.axes, 0x10 + self.fastened),
            &format!("Fastened {}", self.fastened),
            MateType::Fastened,
            (parent, child),
            origin,
            ALONG_Z,
            None,
        )
    }

    /// The Slider or Revolute of the axis `a`: its frame's Z against the axis's direction, so
    /// a positive value moves the child along it (see the module docs).
    fn axis(&mut self, a: &Axis) -> Result<(), CommandError> {
        let against = a.direction.map(|c| if c == 0.0 { 0.0 } else { -c });
        let (mate_type, limits) = if a.rotary {
            (
                MateType::Revolute,
                a.limits.map(|(lo, hi)| MateLimits {
                    angle: Some((lo.to_radians(), hi.to_radians())),
                    ..MateLimits::default()
                }),
            )
        } else {
            (
                MateType::Slider,
                a.limits.map(|l| MateLimits {
                    z: Some(l),
                    ..MateLimits::default()
                }),
            )
        };
        self.join(
            a.mate,
            a.name,
            mate_type,
            (a.parent, a.child),
            a.origin,
            against,
            limits,
        )
    }

    /// What every machine starts with: the fixed Base with the Column on it, the Saddle on
    /// `Y` and the Table on `X`.
    fn frame_and_table(&mut self) -> Result<(), CommandError> {
        let a = self.axes;
        let (base, column, saddle, table) = (inst(a, 1), inst(a, 2), inst(a, 3), inst(a, 4));
        self.insert(base, parts::BASE)?;
        self.insert(column, parts::COLUMN)?;
        self.insert(saddle, parts::SADDLE)?;
        self.insert(table, parts::TABLE)?;
        self.s.run(&SetInstancesFixed {
            element: self.element,
            instances: vec![base],
            fixed: true,
        })?;
        self.fasten(base, column, [0.0, parts::COLUMN_FACE + 100.0, -110.0])?;
        self.axis(&y(a))?;
        self.axis(&x(a))
    }

    /// The 3- and 4-axis machines' head: the Head on `Z`, the Spindle and the Motor on it.
    fn head(&mut self) -> Result<(), CommandError> {
        let a = self.axes;
        let (head, spindle, motor) = (inst(a, 5), inst(a, 6), inst(a, 7));
        self.insert(head, parts::HEAD)?;
        self.insert(spindle, parts::SPINDLE)?;
        self.insert(motor, parts::MOTOR)?;
        self.axis(&z(a, head, 350.0))?;
        self.fasten(head, spindle, [0.0, 0.0, 500.0])?;
        self.fasten(head, motor, [0.0, 0.0, 800.0])
    }
}

/// The assembly "3-axis mill".
pub mod mill3 {
    use super::*;

    pub const ASSEMBLY: ElementId = crate::samples::cnc_machines::MILL_3;
    pub const BASE: InstanceId = inst(3, 1);
    pub const COLUMN: InstanceId = inst(3, 2);
    pub const SADDLE: InstanceId = inst(3, 3);
    pub const TABLE: InstanceId = inst(3, 4);
    pub const HEAD: InstanceId = inst(3, 5);
    pub const SPINDLE: InstanceId = inst(3, 6);
    pub const MOTOR: InstanceId = inst(3, 7);
    /// The axes, in the Mate Features list's order.
    pub const AXES: [Axis; 3] = [y(3), x(3), z(3, HEAD, 350.0)];
    pub const Y: MateId = AXES[0].mate;
    pub const X: MateId = AXES[1].mate;
    pub const Z: MateId = AXES[2].mate;

    /// Fills the empty assembly [`ASSEMBLY`].
    pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
        let mut m = Machine {
            s,
            element: ASSEMBLY,
            axes: 3,
            fastened: 0,
        };
        m.frame_and_table()?;
        m.head()
    }
}

/// The assembly "4-axis mill": the 3-axis machine with a rotary chuck on the table and a
/// carousel of tools beside the column.
pub mod mill4 {
    use super::*;

    pub const ASSEMBLY: ElementId = crate::samples::cnc_machines::MILL_4;
    pub const BASE: InstanceId = inst(4, 1);
    pub const COLUMN: InstanceId = inst(4, 2);
    pub const SADDLE: InstanceId = inst(4, 3);
    pub const TABLE: InstanceId = inst(4, 4);
    pub const HEAD: InstanceId = inst(4, 5);
    pub const SPINDLE: InstanceId = inst(4, 6);
    pub const MOTOR: InstanceId = inst(4, 7);
    pub const ROTARY_BODY: InstanceId = inst(4, 8);
    pub const CHUCK: InstanceId = inst(4, 9);
    pub const MAGAZINE_BRACKET: InstanceId = inst(4, 0xa);
    pub const CAROUSEL: InstanceId = inst(4, 0xb);
    /// The axes and the tool magazine's turn, in the Mate Features list's order.
    pub const AXES: [Axis; 5] = [
        y(4),
        x(4),
        z(4, HEAD, 350.0),
        rotary(
            mate(4, 4),
            "A",
            (ROTARY_BODY, CHUCK),
            parts::CHUCK_AXIS,
            ALONG_X,
            None,
        ),
        rotary(
            mate(4, 5),
            "Magazine",
            (MAGAZINE_BRACKET, CAROUSEL),
            parts::CAROUSEL_AXIS,
            ALONG_Z,
            None,
        ),
    ];
    pub const Y: MateId = AXES[0].mate;
    pub const X: MateId = AXES[1].mate;
    pub const Z: MateId = AXES[2].mate;
    pub const A: MateId = AXES[3].mate;
    pub const MAGAZINE: MateId = AXES[4].mate;

    /// The named position "Pocket `k`" (1 to 8).
    pub const fn pocket(k: u128) -> NamedPositionId {
        NamedPositionId::from_u128(PREFIX | (7 << 92) | k)
    }

    /// The `Magazine`'s angle (radians, in −π…π) in the named position "Pocket `k`".
    pub fn pocket_angle(k: usize) -> f64 {
        let step = 360.0 / parts::POCKETS as f64;
        let a = step * (k as f64 - 1.0);
        (if a > 180.0 { a - 360.0 } else { a }).to_radians()
    }

    /// Fills the empty assembly [`ASSEMBLY`].
    pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
        let mut m = Machine {
            s,
            element: ASSEMBLY,
            axes: 4,
            fastened: 0,
        };
        m.frame_and_table()?;
        m.head()?;
        m.insert(ROTARY_BODY, parts::ROTARY_BODY)?;
        m.insert(CHUCK, parts::CHUCK)?;
        m.insert(MAGAZINE_BRACKET, parts::MAGAZINE_BRACKET)?;
        m.insert(CAROUSEL, parts::CAROUSEL)?;
        m.fasten(TABLE, ROTARY_BODY, [-215.0, 0.0, 0.0])?;
        m.axis(&AXES[3])?;
        let axis = parts::CAROUSEL_AXIS;
        m.fasten(COLUMN, MAGAZINE_BRACKET, [150.0, axis[1], 590.0])?;
        m.axis(&AXES[4])?;
        // The pockets: the carousel turned about its axis, everything else where it is.
        for k in 1..=parts::POCKETS {
            let angle = pocket_angle(k);
            let poses: Vec<(InstanceId, Pose)> =
                m.s.document()
                    .element(ASSEMBLY)
                    .and_then(|e| e.assembly_model())
                    .map(|a| a.instances.iter().map(|i| (i.id, i.pose)).collect())
                    .unwrap_or_default();
            let mut position = NamedPosition {
                id: pocket(k as u128),
                name: format!("Pocket {k}"),
                values: vec![MateValue {
                    mate: MAGAZINE,
                    dof: Dof::Angle,
                    value: angle,
                }],
                poses,
            };
            for (id, pose) in &mut position.poses {
                if *id == CAROUSEL {
                    *pose = Pose::rotation_about(axis, ALONG_Z, angle);
                }
            }
            m.s.run(&SetNamedPosition {
                element: ASSEMBLY,
                position,
            })?;
        }
        Ok(())
    }
}

/// The assembly "5-axis mill": a rotary table under the work and a head that tilts.
pub mod mill5 {
    use super::*;

    pub const ASSEMBLY: ElementId = crate::samples::cnc_machines::MILL_5;
    pub const BASE: InstanceId = inst(5, 1);
    pub const COLUMN: InstanceId = inst(5, 2);
    pub const SADDLE: InstanceId = inst(5, 3);
    pub const TABLE: InstanceId = inst(5, 4);
    pub const SPINDLE: InstanceId = inst(5, 6);
    pub const MOTOR: InstanceId = inst(5, 7);
    pub const ROTARY_TABLE_BASE: InstanceId = inst(5, 0xc);
    pub const PLATTER: InstanceId = inst(5, 0xd);
    pub const Z_SLIDE: InstanceId = inst(5, 0xe);
    pub const SWIVEL_HEAD: InstanceId = inst(5, 0xf);
    /// The axes, in the Mate Features list's order.
    pub const AXES: [Axis; 5] = [
        y(5),
        x(5),
        rotary(
            mate(5, 6),
            "C",
            (ROTARY_TABLE_BASE, PLATTER),
            [0.0, 0.0, 80.0],
            ALONG_Z,
            None,
        ),
        z(5, Z_SLIDE, 300.0),
        rotary(
            mate(5, 7),
            "B",
            (Z_SLIDE, SWIVEL_HEAD),
            parts::HEAD_PIVOT,
            ALONG_Y,
            Some((-100.0, 100.0)),
        ),
    ];
    pub const Y: MateId = AXES[0].mate;
    pub const X: MateId = AXES[1].mate;
    pub const C: MateId = AXES[2].mate;
    pub const Z: MateId = AXES[3].mate;
    pub const B: MateId = AXES[4].mate;

    /// Fills the empty assembly [`ASSEMBLY`].
    pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
        let mut m = Machine {
            s,
            element: ASSEMBLY,
            axes: 5,
            fastened: 0,
        };
        m.frame_and_table()?;
        m.insert(ROTARY_TABLE_BASE, parts::ROTARY_TABLE_BASE)?;
        m.insert(PLATTER, parts::PLATTER)?;
        m.insert(Z_SLIDE, parts::Z_SLIDE)?;
        m.insert(SWIVEL_HEAD, parts::SWIVEL_HEAD)?;
        m.insert(SPINDLE, parts::SPINDLE)?;
        m.insert(MOTOR, parts::MOTOR)?;
        m.fasten(TABLE, ROTARY_TABLE_BASE, [0.0, 0.0, 0.0])?;
        m.axis(&AXES[2])?;
        m.axis(&AXES[3])?;
        m.axis(&AXES[4])?;
        m.fasten(SWIVEL_HEAD, SPINDLE, [0.0, 0.0, 500.0])?;
        m.fasten(SWIVEL_HEAD, MOTOR, [0.0, 0.0, 800.0])
    }
}
