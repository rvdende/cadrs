//! The rows of the Sheet metal table (SM13): the **Bends** table (#, Name, Radius, Angle, Bend
//! direction, and the K Factor / Bend allowance / Bend deduction column named after the model's
//! Bend calculation) and the **Other joints** table (Name, Type, Style).

use serde::{Deserialize, Serialize};

use crate::bend::BendValue;
use crate::model::{JointId, JointKind, Model, RipStyle};
use crate::params::BendCalc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BendDirection {
    Up,
    Down,
}

impl BendDirection {
    pub fn label(self) -> &'static str {
        match self {
            BendDirection::Up => "Up",
            BendDirection::Down => "Down",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BendRow {
    pub joint: JointId,
    /// 1-based row number (the # column).
    pub number: usize,
    pub name: String,
    /// Inner radius (mm).
    pub radius: f64,
    /// Bend angle (degrees).
    pub angle_deg: f64,
    pub direction: BendDirection,
    /// The bend's value in the model's calculation (`None` where it is undefined: a deduction
    /// of a bend of 180° or more, shown as "–").
    pub value: Option<BendValue>,
    /// The value is the bend's own (not the model's).
    pub value_overridden: bool,
    /// Radius and value can be edited (not for hems, SM4.5; not for arcs extruded as bends,
    /// whose radius comes from the sketch, SM2.3).
    pub editable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JointType {
    Rip,
    Tangent,
}

impl JointType {
    pub fn label(self) -> &'static str {
        match self {
            JointType::Rip => "Rip",
            JointType::Tangent => "Tangent",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointRow {
    pub joint: JointId,
    pub name: String,
    pub kind: JointType,
    /// Rips only.
    pub style: Option<RipStyle>,
}

/// Both tables of a model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// The last column's title: "K Factor", "Bend allowance (mm)" or "Bend deduction (mm)".
    pub value_column: String,
    pub bends: Vec<BendRow>,
    pub joints: Vec<JointRow>,
}

/// The tables in table (joint) order.
pub fn table(m: &Model) -> Table {
    let calc = m.params.bend_calc;
    let value_column = match calc {
        BendCalc::KFactor => calc.label().to_string(),
        _ => format!("{} (mm)", calc.label()),
    };
    let mut t = Table {
        value_column,
        ..Default::default()
    };
    for j in &m.joints {
        match &j.kind {
            JointKind::Bend(b) => {
                let up = b.toward_material != m.params.flip_direction_up;
                t.bends.push(BendRow {
                    joint: j.id,
                    number: t.bends.len() + 1,
                    name: j.name.clone(),
                    radius: b.radius,
                    angle_deg: b.angle.to_degrees(),
                    direction: if up { BendDirection::Up } else { BendDirection::Down },
                    value: b.value_or_model(&m.params).to_calc(calc, b.radius, m.params.thickness, b.angle),
                    value_overridden: b.value.is_some(),
                    editable: !b.hem,
                });
            }
            JointKind::Rip { style, .. } => t.joints.push(JointRow {
                joint: j.id,
                name: j.name.clone(),
                kind: JointType::Rip,
                style: Some(*style),
            }),
            JointKind::Tangent { .. } => t.joints.push(JointRow {
                joint: j.id,
                name: j.name.clone(),
                kind: JointType::Tangent,
                style: None,
            }),
        }
    }
    t
}
