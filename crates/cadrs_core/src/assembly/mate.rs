//! Mate features (P3B.2, P3B.3; `intro-to-assemblies.md` A6–A13, X6): the Mate Features list of
//! an assembly, in creation order: **mates** (a type and two [`MateConnector`]s, with an
//! optional offset and limits; Tangent's two entities; Width's pair and tabs) and **groups**
//! (instances that move as one). A mate or group can be **suppressed** (the solver skips it).
//!
//! A connector mate holds when the first connector (after its flip / reorient) sits on the
//! second connector moved by the offset, `F₁ = F₂ · O`, up to the motion the type allows,
//! measured in the mate's frame ([`MateType::dof`], A7). Its **position** is
//! `D = F₁⁻¹ · (F₂ · O)`: a slider's travel is D's Z translation, a pin slot's D's X, a planar's
//! D's X and Y, a revolute's angle D's turn about Z ([`dof_value`]); they are zero where the mate
//! was made (so **Reset** goes back there) and the limits bound them.

use cadrs_sketch::Vec3;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::connector::{ConnectorFrame, MateConnector};
use super::{InstanceId, Pose};

/// Identifies a mate feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MateId(pub Uuid);

impl MateId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for MateId {
    fn default() -> Self {
        Self::new()
    }
}

/// The mate types (A7), in the mate dialog's dropdown order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MateType {
    /// No motion (A7.1).
    Fastened,
    /// Rotate about Z (A7.2).
    Revolute,
    /// Translate along Z (A7.3).
    Slider,
    /// Translate along and rotate about Z (A7.4).
    Cylindrical,
    /// Rotate about Z and translate along X (A7.5): the first connector is the slot's, X along
    /// the slot.
    PinSlot,
    /// Translate along X and Y and rotate about Z (A7.6).
    Planar,
    /// Rotate about X, Y and Z (A7.7).
    Ball,
    /// Translate along X, Y and Z and rotate about Z (A7.8): the Z axes stay parallel.
    Parallel,
    /// Two entities kept tangent (A7.9, A11): no connectors; the two "connectors" are the picked
    /// surfaces ([`super::connector::ConnectorAnchor::Surface`]).
    Tangent,
    /// Tabs kept centred between two width connectors (A7.10, A12): `connectors` are the Width
    /// pair, [`Mate::tabs`] the Tab connectors (one or two).
    Width,
}

/// A degree of freedom a mate allows (in the mate's frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dof {
    /// Translation along Z.
    Z,
    /// Rotation about Z.
    Angle,
    /// Translation along X (Pin slot, Planar, Parallel).
    X,
    /// Translation along Y (Planar, Parallel).
    Y,
}

impl Dof {
    /// "X", "Y", "Z", "Z angle" (Animate's DOF pick, menus).
    pub fn label(self) -> &'static str {
        match self {
            Dof::X => "X",
            Dof::Y => "Y",
            Dof::Z => "Z",
            Dof::Angle => "Z angle",
        }
    }

    pub fn is_angle(self) -> bool {
        self == Dof::Angle
    }
}

impl MateType {
    pub const ALL: [MateType; 10] = [
        MateType::Fastened,
        MateType::Revolute,
        MateType::Slider,
        MateType::Cylindrical,
        MateType::PinSlot,
        MateType::Planar,
        MateType::Ball,
        MateType::Parallel,
        MateType::Tangent,
        MateType::Width,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MateType::Fastened => "Fastened",
            MateType::Revolute => "Revolute",
            MateType::Slider => "Slider",
            MateType::Cylindrical => "Cylindrical",
            MateType::PinSlot => "Pin slot",
            MateType::Planar => "Planar",
            MateType::Ball => "Ball",
            MateType::Parallel => "Parallel",
            MateType::Tangent => "Tangent",
            MateType::Width => "Width",
        }
    }

    /// The motions it allows that can be driven (Animate, Reset, the Animate dialog's DOF pick;
    /// A7). A Ball's are its three turns; only the one about Z is driven.
    pub fn dof(self) -> &'static [Dof] {
        match self {
            MateType::Fastened | MateType::Tangent | MateType::Width => &[],
            MateType::Revolute | MateType::Ball => &[Dof::Angle],
            MateType::Slider => &[Dof::Z],
            MateType::Cylindrical => &[Dof::Z, Dof::Angle],
            MateType::PinSlot => &[Dof::X, Dof::Angle],
            MateType::Planar => &[Dof::X, Dof::Y, Dof::Angle],
            MateType::Parallel => &[Dof::X, Dof::Y, Dof::Z, Dof::Angle],
        }
    }

    /// The DOF it has Limits for (A6.10, A9.3): Onshape's Revolute, Slider, Cylindrical, Pin slot
    /// and Planar.
    pub fn limit_dofs(self) -> &'static [Dof] {
        match self {
            MateType::Revolute | MateType::Slider | MateType::Cylindrical | MateType::PinSlot | MateType::Planar => self.dof(),
            _ => &[],
        }
    }

    /// Whether it has a Limits option.
    pub fn has_limits(self) -> bool {
        !self.limit_dofs().is_empty()
    }

    /// Whether it has an Offset option (every connector mate but Width).
    pub fn has_offset(self) -> bool {
        !matches!(self, MateType::Tangent | MateType::Width)
    }

    /// Whether it joins two mate connectors (all but Tangent, which takes two entities).
    pub fn uses_connectors(self) -> bool {
        self != MateType::Tangent
    }

    /// The degrees of freedom it leaves between its instances (A7); Tangent's depend on the
    /// entities (None). Width's are those of one instance's tabs centred between fixed width
    /// connectors: slide in the centre plane and turn about its normal.
    pub fn dof_count(self) -> Option<u32> {
        Some(match self {
            MateType::Fastened => 0,
            MateType::Revolute | MateType::Slider => 1,
            MateType::Cylindrical | MateType::PinSlot => 2,
            MateType::Planar | MateType::Ball | MateType::Width => 3,
            MateType::Parallel => 4,
            MateType::Tangent => return None,
        })
    }
}

/// The offset of a mate (A6.10): a translation (mm) and a turn about one of the mate's axes.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct MateOffset {
    pub translation: Vec3,
    /// 0 = X, 1 = Y, 2 = Z ("Rotate about X").
    pub axis: u8,
    /// Radians.
    pub angle: f64,
}

impl MateOffset {
    /// The offset as a placement in the second connector's frame: translate, then turn.
    pub fn pose(&self) -> Pose {
        let mut a = [0.0; 3];
        a[self.axis.min(2) as usize] = 1.0;
        Pose::rotation_about([0.0; 3], a, self.angle).then(&Pose::translation(self.translation))
    }
}

/// The limits of a mate's motion (A6.10, A9.3): X, Y and Z min / max (mm) and angle min / max
/// (radians).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct MateLimits {
    #[serde(default)]
    pub z: Option<(f64, f64)>,
    #[serde(default)]
    pub angle: Option<(f64, f64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<(f64, f64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<(f64, f64)>,
}

impl MateLimits {
    pub fn of(&self, d: Dof) -> Option<(f64, f64)> {
        match d {
            Dof::Z => self.z,
            Dof::Angle => self.angle,
            Dof::X => self.x,
            Dof::Y => self.y,
        }
    }

    pub fn set(&mut self, d: Dof, v: Option<(f64, f64)>) {
        match d {
            Dof::Z => self.z = v,
            Dof::Angle => self.angle = v,
            Dof::X => self.x = v,
            Dof::Y => self.y = v,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

/// A mate between two connectors (Tangent: two entities; Width: the Width pair, plus the Tab
/// connectors in `tabs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mate {
    pub mate_type: MateType,
    pub connectors: [MateConnector; 2],
    #[serde(default)]
    pub offset: Option<MateOffset>,
    #[serde(default)]
    pub limits: Option<MateLimits>,
    /// Width: the Tab connectors (one or two; A12.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<MateConnector>,
    /// Tangent: **Tangent propagation** (on by default, A11.2).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub propagate: bool,
    /// Tangent: the other side (A11.3's Flip).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// **Simulation connection** (A6.3, P3F.5): the two parts are bonded where they touch in a
    /// simulation ([`crate::simulation`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub simulation: bool,
}

impl Mate {
    pub fn new(mate_type: MateType, a: MateConnector, b: MateConnector) -> Self {
        Self { mate_type, connectors: [a, b], offset: None, limits: None, tabs: Vec::new(), propagate: true, flip: false, simulation: false }
    }

    /// A Width mate: `tabs` (one or two) centred between `widths` (A12).
    pub fn width(tabs: Vec<MateConnector>, widths: [MateConnector; 2]) -> Self {
        Self { tabs, ..Self::new(MateType::Width, widths[0], widths[1]) }
    }

    /// Every connector: the pair, then the tabs.
    pub fn all_connectors(&self) -> impl Iterator<Item = &MateConnector> {
        self.connectors.iter().chain(self.tabs.iter())
    }

    /// The target frame `F₂ · O` for a second connector frame `f2` (any coordinates).
    pub fn target(&self, f2: &ConnectorFrame) -> ConnectorFrame {
        match &self.offset {
            Some(o) => f2.then_local(&o.pose()),
            None => *f2,
        }
    }

    /// The mate's limits for a degree of freedom it has.
    pub fn limit(&self, d: Dof) -> Option<(f64, f64)> {
        self.mate_type.limit_dofs().contains(&d).then(|| self.limits.and_then(|l| l.of(d))).flatten()
    }
}

/// A mate's position for world frames `f1` (the first connector, adjusted) and `g` (the target):
/// D = F₁⁻¹ · G as (Z translation, angle about Z).
pub fn mate_position(f1: &ConnectorFrame, g: &ConnectorFrame) -> (f64, f64) {
    let d = [g.origin[0] - f1.origin[0], g.origin[1] - f1.origin[1], g.origin[2] - f1.origin[2]];
    let z = f1.z[0] * d[0] + f1.z[1] * d[1] + f1.z[2] * d[2];
    let y1 = f1.y();
    let dot = |a: Vec3, b: Vec3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let angle = dot(y1, g.x).atan2(dot(f1.x, g.x));
    (z, angle)
}

/// A mate's position along one DOF for world frames `f1` and `g` (see [`mate_position`]): X, Y
/// or Z of D's translation (mm), or its angle about Z (radians).
pub fn dof_value(f1: &ConnectorFrame, g: &ConnectorFrame, dof: Dof) -> f64 {
    let d = [g.origin[0] - f1.origin[0], g.origin[1] - f1.origin[1], g.origin[2] - f1.origin[2]];
    let dot = |a: Vec3, b: Vec3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    match dof {
        Dof::X => dot(f1.x, d),
        Dof::Y => dot(f1.y(), d),
        Dof::Z => dot(f1.z, d),
        Dof::Angle => mate_position(f1, g).1,
    }
}

/// A mate feature: a mate or a group, with its name ("Fastened 1", "Group 1").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MateFeature {
    pub id: MateId,
    pub name: String,
    pub kind: MateKind,
    /// Suppressed (the mate menu's Suppress, A6.13): kept in the list, ignored by the solver.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// P3F.4: the offset fields typed as expressions naming variables (`#gap + 1 mm`), by
    /// [`OffsetSlot`] (X, Y, Z translation, rotation), kept next to the values in the mate's
    /// offset and re-evaluated when a variable changes ([`super::vars`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exprs: Vec<(OffsetSlot, String)>,
}

/// Which offset field an expression is for (P3F.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OffsetSlot {
    X,
    Y,
    Z,
    /// The rotation (degrees in the expression, radians in the offset).
    Angle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // A few per assembly; boxing would only add indirection.
pub enum MateKind {
    Mate(Mate),
    /// A13: instances with no motion between them.
    Group { instances: Vec<InstanceId> },
    /// P3B.8, X16: copies of a seed instance and its mate on matching geometry
    /// ([`super::replicate`]).
    Replicate(super::replicate::Replicate),
    /// P3B.9, A1.4, X16: a coupling between mates' motions ([`super::relation`]).
    Relation(super::relation::Relation),
    /// P3F.4 (A1.8): a variable, `#name = expression`, for the mates below it
    /// ([`super::vars`]).
    Variable(crate::variables::VariableFeature),
}

impl MateFeature {
    pub fn new(id: MateId, name: impl Into<String>, kind: MateKind) -> Self {
        Self { id, name: name.into(), kind, suppressed: false, exprs: Vec::new() }
    }

    /// The type's label ("Fastened", "Group").
    pub fn type_label(&self) -> &'static str {
        match &self.kind {
            MateKind::Mate(m) => m.mate_type.label(),
            MateKind::Group { .. } => "Group",
            MateKind::Replicate(_) => "Replicate",
            MateKind::Relation(r) => r.relation_type.label(),
            MateKind::Variable(_) => "Variable",
        }
    }

    pub fn mate(&self) -> Option<&Mate> {
        match &self.kind {
            MateKind::Mate(m) => Some(m),
            MateKind::Group { .. } | MateKind::Replicate(_) | MateKind::Relation(_) | MateKind::Variable(_) => None,
        }
    }

    /// A relation (P3B.9).
    pub fn relation(&self) -> Option<&super::relation::Relation> {
        match &self.kind {
            MateKind::Relation(r) => Some(r),
            _ => None,
        }
    }

    pub fn replicate(&self) -> Option<&super::replicate::Replicate> {
        match &self.kind {
            MateKind::Replicate(r) => Some(r),
            _ => None,
        }
    }

    pub fn mate_mut(&mut self) -> Option<&mut Mate> {
        match &mut self.kind {
            MateKind::Mate(m) => Some(m),
            MateKind::Group { .. } | MateKind::Replicate(_) | MateKind::Relation(_) | MateKind::Variable(_) => None,
        }
    }

    /// The instances it involves (a relation: none of its own; see
    /// [`super::Assembly::feature_instances`]).
    pub fn instances(&self) -> Vec<InstanceId> {
        match &self.kind {
            MateKind::Mate(m) => {
                let mut out: Vec<InstanceId> = Vec::new();
                for c in m.all_connectors() {
                    if !out.contains(&c.instance) {
                        out.push(c.instance);
                    }
                }
                out
            }
            MateKind::Group { instances } => instances.clone(),
            MateKind::Replicate(r) => std::iter::once(r.seed).chain(r.instances.iter().copied()).collect(),
            MateKind::Relation(_) | MateKind::Variable(_) => Vec::new(),
        }
    }

    /// Whether it involves `i`.
    pub fn involves(&self, i: InstanceId) -> bool {
        self.instances().contains(&i)
    }
}

/// The next name for a feature of type `label` among `features`: "Fastened 1", "Fastened 2", …
/// (one more than the highest number in use).
pub fn next_name(features: &[MateFeature], label: &str) -> String {
    let n = features
        .iter()
        .filter_map(|f| f.name.strip_prefix(label)?.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{label} {}", n + 1)
}
