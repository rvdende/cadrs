//! Onshape's **Modify joint** feature (P3I.3; `simultaneous-sheet-metal.md` SM6.4, SM13.3,
//! SM13.4; `help/feature-tools/modify-joint-02.png`): one joint of a Sheet metal model made a
//! **Bend** (its own radius, or the model's; its own K factor, bend allowance or bend
//! deduction, or the model's), a **Rip** (Edge joint, Butt joint – Direction 1 or 2) or left a
//! **Tangent** joint. It isn't on the toolbar: editing a row of the Sheet metal table makes one
//! ([`table_edit`], [`PutModifyJoint`]), or edits the joint's Modify joint if it has one; it can
//! then be edited like any feature.
//!
//! The rebuild (`rebuild/kernel_ops/sheetmetal_joint.rs`) makes the edit in the model's
//! definition ([`cadrs_sheetmetal::definition::Definition::edit_joint`]) and refolds its parts in
//! place through the one sheet metal pipeline (same part ids and face names).
//!
//! A new Modify joint goes right after the last feature that changed the model (the Sheet
//! metal model, its Modify joints, Flanges, Bends, Tabs, …: the context's `editors`), not at the
//! end of the list as in Onshape: in cadrs the ordinary features after a Sheet metal model act
//! on its folded parts, so they must come after the joint changes to keep acting on the
//! refolded parts, and a joint a Flange made only exists after the Flange. **Move up / Move down** (the table order, SM13.4) is kept
//! on the Sheet metal model feature itself ([`SetTableOrder`]).

use cadrs_sheetmetal::joint_edit::{JointChange, JointEdit};
use cadrs_sheetmetal::params::{RangeError, range};
use cadrs_sheetmetal::{BendCalc, BendValue, Joint, JointId, JointKind, Params, RipStyle};
use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, ElementKind, Feature, FeatureKind};
use crate::ids::{ElementId, FeatureId};
use crate::sheetmetal::plain;
use crate::sheetmetal_tools::SheetMetalTool;

/// The joint type select (SM6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JointType {
    #[default]
    Bend,
    Rip,
    Tangent,
}

impl JointType {
    pub const ALL: [JointType; 3] = [JointType::Bend, JointType::Rip, JointType::Tangent];

    pub fn label(self) -> &'static str {
        match self {
            JointType::Bend => "Bend",
            JointType::Rip => "Rip",
            JointType::Tangent => "Tangent",
        }
    }
}

/// A Modify joint feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModifyJointFeature {
    /// The Sheet metal model the joint is in.
    pub model: FeatureId,
    /// The joint ("Joint" field); `None` while nothing is picked.
    pub joint: Option<JointId>,
    pub joint_type: JointType,
    pub rip_style: RipStyle,
    /// Use model bend radius.
    pub use_model_radius: bool,
    /// Bend radius (mm).
    pub radius: f64,
    pub radius_expr: String,
    /// Use model K Factor (the model's calculation and value).
    pub use_model_value: bool,
    /// Bend calculation.
    pub calc: BendCalc,
    /// K Factor (unitless), Bend allowance or Bend deduction (mm).
    pub value: f64,
    pub value_expr: String,
}

impl Default for ModifyJointFeature {
    fn default() -> Self {
        let p = Params::default();
        ModifyJointFeature {
            model: FeatureId(uuid::Uuid::nil()),
            joint: None,
            joint_type: JointType::Bend,
            rip_style: RipStyle::EdgeJoint,
            use_model_radius: true,
            radius: p.bend_radius,
            radius_expr: format!("{} mm", plain(p.bend_radius)),
            use_model_value: true,
            calc: BendCalc::KFactor,
            value: p.k_factor,
            value_expr: plain(p.k_factor),
        }
    }
}

/// The range a joint's own value must be in, for its calculation (SM6.4: a K factor from −1.5
/// to 1; an allowance above 0; a deduction from 0).
pub fn value_range(calc: BendCalc) -> (f64, f64, bool) {
    match calc {
        BendCalc::KFactor => (range::JOINT_K_FACTOR.0, range::JOINT_K_FACTOR.1, false),
        BendCalc::BendAllowance => (0.0, f64::INFINITY, true),
        BendCalc::BendDeduction => (0.0, f64::INFINITY, false),
    }
}

/// The value's range error, if it is out of range (the table cell's red and tooltip, X5).
pub fn value_error(calc: BendCalc, value: f64) -> Option<RangeError> {
    let (min, max, min_exclusive) = value_range(calc);
    let ok = value.is_finite() && if min_exclusive { value > min } else { value >= min } && value <= max;
    (!ok).then(|| RangeError { field: calc.label().to_string(), value, min, max, min_exclusive })
}

/// A radius's range error (a bend radius is at least 0).
pub fn radius_error(radius: f64) -> Option<RangeError> {
    (!(radius >= 0.0 && radius.is_finite())).then(|| RangeError { field: "Bend radius".into(), value: radius, min: 0.0, max: f64::INFINITY, min_exclusive: false })
}

impl ModifyJointFeature {
    /// The out-of-range value or radius, with its message.
    pub fn range_error(&self) -> Option<RangeError> {
        if self.joint_type != JointType::Bend {
            return None;
        }
        if !self.use_model_radius
            && let Some(e) = radius_error(self.radius)
        {
            return Some(e);
        }
        if !self.use_model_value {
            return value_error(self.calc, self.value);
        }
        None
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.joint.is_none() {
            return Some("Select a joint");
        }
        self.range_error().map(|_| "A value is out of range")
    }

    /// The joint's own value, if it has one.
    pub fn bend_value(&self) -> Option<BendValue> {
        let v = match self.calc {
            BendCalc::KFactor => BendValue::KFactor(self.value),
            BendCalc::BendAllowance => BendValue::Allowance(self.value),
            BendCalc::BendDeduction => BendValue::Deduction(self.value),
        };
        (!self.use_model_value).then_some(v)
    }

    /// What it does to its joint (`None` for Tangent, which leaves a tangent joint as it is).
    pub fn edit(&self) -> Option<JointEdit> {
        let joint = self.joint?;
        let change = match self.joint_type {
            JointType::Bend => JointChange::Bend { radius: (!self.use_model_radius).then_some(self.radius), value: self.bend_value() },
            JointType::Rip => JointChange::Rip { style: self.rip_style },
            JointType::Tangent => return None,
        };
        Some(JointEdit { joint, change })
    }

    /// Its parent: the Sheet metal model.
    pub fn parents(&self) -> Vec<FeatureId> {
        vec![self.model]
    }

    /// A Modify joint that leaves `joint` as it is now (what a table edit starts from).
    pub fn of_joint(model: FeatureId, joint: &Joint, p: &Params) -> Self {
        let mut x = ModifyJointFeature { model, joint: Some(joint.id), calc: p.bend_calc, ..Default::default() };
        let mv = BendValue::from_params(p);
        x.value = mv.value();
        x.value_expr = value_expr(p.bend_calc, x.value);
        x.radius = p.bend_radius;
        x.radius_expr = format!("{} mm", plain(p.bend_radius));
        match &joint.kind {
            JointKind::Bend(b) => {
                x.joint_type = JointType::Bend;
                x.use_model_radius = b.model_radius;
                x.radius = b.radius;
                x.radius_expr = format!("{} mm", plain(b.radius));
                if let Some(v) = b.value {
                    x.use_model_value = false;
                    x.calc = v.calc();
                    x.value = v.value();
                    x.value_expr = value_expr(x.calc, x.value);
                }
            }
            JointKind::Rip { style, .. } => {
                x.joint_type = JointType::Rip;
                x.rip_style = *style;
            }
            JointKind::Tangent { .. } => x.joint_type = JointType::Tangent,
        }
        x
    }
}

/// A value's expression: a K factor plain, a length in mm.
pub fn value_expr(calc: BendCalc, v: f64) -> String {
    match calc {
        BendCalc::KFactor => plain(v),
        _ => format!("{} mm", plain(v)),
    }
}

/// An edit made in the Sheet metal table (SM13.3, SM13.4).
#[derive(Debug, Clone, PartialEq)]
pub enum TableEdit {
    /// A bend's radius (mm) and its expression as typed.
    Radius(f64, String),
    /// A bend's own value in the model's calculation, as typed.
    Value(f64, String),
    ConvertToRip,
    ConvertToBend,
    RipStyle(RipStyle),
}

impl TableEdit {
    /// The undo label.
    pub fn label(&self, joint: &str) -> String {
        match self {
            TableEdit::Radius(..) => format!("Edit radius of {joint}"),
            TableEdit::Value(..) => format!("Edit bend value of {joint}"),
            TableEdit::ConvertToRip => format!("Convert {joint} to rip"),
            TableEdit::ConvertToBend => format!("Convert {joint} to bend"),
            TableEdit::RipStyle(s) => format!("{joint}: {}", s.label()),
        }
    }
}

/// The Modify joint a table edit gives: the joint's own Modify joint (`existing`) changed, or a
/// new one that keeps the joint as it is (`joint`, `params`: the model as built) but for the
/// edit.
pub fn table_edit(model: FeatureId, existing: Option<&ModifyJointFeature>, joint: &Joint, params: &Params, edit: &TableEdit) -> ModifyJointFeature {
    let mut x = match existing {
        Some(e) => e.clone(),
        None => ModifyJointFeature::of_joint(model, joint, params),
    };
    match edit {
        TableEdit::Radius(r, expr) => {
            x.joint_type = JointType::Bend;
            x.use_model_radius = false;
            x.radius = *r;
            x.radius_expr = expr.clone();
        }
        TableEdit::Value(v, expr) => {
            x.joint_type = JointType::Bend;
            x.use_model_value = false;
            x.calc = params.bend_calc;
            x.value = *v;
            x.value_expr = expr.clone();
        }
        TableEdit::ConvertToRip => x.joint_type = JointType::Rip,
        TableEdit::ConvertToBend => {
            x.joint_type = JointType::Bend;
            // A rip made a bend takes the model's radius and value unless it had its own.
            if existing.is_none() {
                x.use_model_radius = true;
                x.use_model_value = true;
            }
        }
        TableEdit::RipStyle(s) => {
            x.joint_type = JointType::Rip;
            x.rip_style = *s;
        }
    }
    x
}

/// The **Bend** or **Jog** feature that made a joint, if one did (SM13.3: the table edits that
/// feature, not a Modify joint). `ctx` is the joint's model as built.
pub fn bend_feature_of<'a>(features: &'a [Feature], ctx: &crate::sheetmetal::SheetMetalContext, joint: JointId) -> Option<&'a Feature> {
    let owner = ctx.owner(crate::sheetmetal::PieceKey::Joint(joint));
    if owner == ctx.feature {
        return None;
    }
    features.iter().find(|f| f.id == owner && matches!(f.kind, FeatureKind::SheetMetalTool(SheetMetalTool::Bend(_) | SheetMetalTool::Jog(_))))
}

/// A table edit of a bend a Bend or Jog feature made (SM13.3): that feature with its own
/// radius or K factor set ("Modifying this value … updates the bend radius value if using the
/// Bend feature"). A value typed as a bend allowance or deduction (the model's calculation) is
/// turned into the K factor that gives it, for this bend's radius, thickness and angle (a Bend
/// takes a K factor only). Converting such a bend to a rip is refused.
pub fn bend_feature_edit(feature: &Feature, joint: &Joint, params: &Params, edit: &TableEdit) -> Result<Feature, String> {
    let Some(b) = joint.bend() else { return Err("Not a bend".into()) };
    let mut f = feature.clone();
    let bf = match &mut f.kind {
        FeatureKind::SheetMetalTool(SheetMetalTool::Bend(x)) => x,
        FeatureKind::SheetMetalTool(SheetMetalTool::Jog(x)) => &mut x.bend,
        _ => return Err("Not a Bend or Jog feature".into()),
    };
    match edit {
        TableEdit::Radius(r, expr) => {
            bf.use_model_radius = false;
            bf.radius = *r;
            bf.radius_expr = expr.clone();
        }
        TableEdit::Value(v, expr) => {
            let k = match params.bend_calc {
                BendCalc::KFactor if (0.0..=1.0).contains(v) => *v,
                BendCalc::KFactor => return Err(format!("{}'s K Factor must be between 0 and 1", feature.name)),
                calc => {
                    let typed = if calc == BendCalc::BendAllowance { BendValue::Allowance(*v) } else { BendValue::Deduction(*v) };
                    let what = format!("a {} of {}", calc.label().to_lowercase(), plain(*v));
                    match typed.to_calc(BendCalc::KFactor, b.radius, params.thickness, b.angle) {
                        Some(BendValue::KFactor(k)) if (0.0..=1.0).contains(&k) => k,
                        Some(BendValue::KFactor(k)) if k.is_finite() => {
                            return Err(format!("{what} needs a K Factor of {}; {}'s must be between 0 and 1", plain((k * 1e4).round() / 1e4), feature.name));
                        }
                        _ => return Err(format!("No K Factor gives {what}")),
                    }
                }
            };
            bf.use_model_k = false;
            bf.k_factor = k;
            bf.k_expr = if params.bend_calc == BendCalc::KFactor { expr.clone() } else { plain((k * 1e6).round() / 1e6) };
        }
        TableEdit::ConvertToRip | TableEdit::ConvertToBend | TableEdit::RipStyle(_) => {
            return Err(format!("{} is made by {}: it can't be made a rip", joint.name, feature.name));
        }
    }
    Ok(f)
}

/// The Modify joint of `joint` in model `model`, if there is one (the last, if several).
pub fn modify_joint_of(features: &[Feature], model: FeatureId, joint: JointId) -> Option<&Feature> {
    features.iter().rev().find(|f| matches!(&f.kind, FeatureKind::ModifyJoint(x) if x.model == model && x.joint == Some(joint)))
}

/// Where a new Modify joint of `model` goes: after the model, its Modify joints and `editors`
/// (the other features that changed it, from its context).
pub fn insert_index(features: &[Feature], model: FeatureId, editors: &[FeatureId]) -> Option<usize> {
    let i = features
        .iter()
        .rposition(|f| f.id == model || editors.contains(&f.id) || matches!(&f.kind, FeatureKind::ModifyJoint(x) if x.model == model))?;
    Some(i + 1)
}

/// Puts a Modify joint in place: sets feature `feature` if it exists, else inserts it as
/// "Modify joint N" after the model's other sheet metal features (one undo step).
#[derive(Debug, Clone)]
pub struct PutModifyJoint {
    pub element: ElementId,
    pub feature: FeatureId,
    pub joint: ModifyJointFeature,
    /// The features that changed the model so far (its context's `editors`): a new Modify joint
    /// goes after them.
    pub after: Vec<FeatureId>,
    pub label: String,
}

impl Command for PutModifyJoint {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let name = el.next_feature_name("Modify joint");
        let ElementKind::PartStudio { features, rollback, .. } = &mut el.kind else {
            return Err(CommandError::Invalid("features need a Part Studio".into()));
        };
        let kind = FeatureKind::ModifyJoint(self.joint.clone());
        if let Some(f) = features.iter_mut().find(|f| f.id == self.feature) {
            f.kind = kind;
        } else {
            let at = insert_index(features, self.joint.model, &self.after).ok_or_else(|| CommandError::Invalid("the sheet metal model is gone".into()))?;
            features.insert(at, Feature { id: self.feature, name, kind });
            if let Some(r) = rollback
                && *r >= at
            {
                *r += 1;
            }
        }
        crate::commands::refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Move up / Move down in the table (SM13.4): the model's table order.
#[derive(Debug, Clone)]
pub struct SetTableOrder {
    pub element: ElementId,
    pub model: FeatureId,
    pub order: Vec<JointId>,
    pub label: String,
}

impl Command for SetTableOrder {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        let f = el
            .features_mut()
            .and_then(|fs| fs.iter_mut().find(|f| f.id == self.model))
            .ok_or_else(|| CommandError::Invalid("the sheet metal model is gone".into()))?;
        let FeatureKind::SheetMetalModel(x) = &mut f.kind else {
            return Err(CommandError::Invalid("not a sheet metal model".into()));
        };
        x.table_order = self.order.clone();
        crate::commands::refresh_studio(doc, self.element);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sheetmetal::model::Bend;
    use cadrs_sheetmetal::poly::Seg2;
    use cadrs_sheetmetal::poly::P2;

    fn bend(radius: f64, model_radius: bool) -> Joint {
        let s = Seg2::new(P2::new(0.0, 0.0), P2::new(10.0, 0.0));
        Joint {
            id: JointId(7),
            name: "Bend A".into(),
            a: cadrs_sheetmetal::WallId(1),
            b: cadrs_sheetmetal::WallId(2),
            kind: JointKind::Bend(Bend { on_a: s, on_b: s, angle: 1.0, toward_material: false, radius, model_radius, value: None, hem: false }),
        }
    }

    #[test]
    fn a_table_edit_starts_from_the_joint_as_it_is() {
        let p = Params::default();
        let m = FeatureId::new();
        let j = bend(1.0, true);
        let x = table_edit(m, None, &j, &p, &TableEdit::Value(0.3, "0.3".into()));
        assert_eq!(x.joint, Some(JointId(7)));
        assert!(x.use_model_radius, "the radius stays the model's");
        assert!(!x.use_model_value);
        assert_eq!(x.bend_value(), Some(BendValue::KFactor(0.3)));
        assert_eq!(x.problem(), None);
        // Then its radius: the same feature, edited.
        let y = table_edit(m, Some(&x), &j, &p, &TableEdit::Radius(2.5, "2.5 mm".into()));
        assert_eq!(y.edit(), Some(JointEdit { joint: JointId(7), change: JointChange::Bend { radius: Some(2.5), value: Some(BendValue::KFactor(0.3)) } }));
        // Made a rip and back: the bend's own values are kept.
        let r = table_edit(m, Some(&y), &j, &p, &TableEdit::ConvertToRip);
        assert_eq!(r.edit().map(|e| e.change), Some(JointChange::Rip { style: RipStyle::EdgeJoint }));
        let b = table_edit(m, Some(&r), &j, &p, &TableEdit::ConvertToBend);
        assert_eq!(b.edit(), y.edit());
        // A bend from a fillet keeps its own radius when only its value changes.
        let own = table_edit(m, None, &bend(4.0, false), &p, &TableEdit::Value(0.4, "0.4".into()));
        assert!(!own.use_model_radius);
        assert_eq!(own.radius, 4.0);
    }

    #[test]
    fn an_out_of_range_value_is_a_problem_with_its_range() {
        let p = Params::default();
        let x = table_edit(FeatureId::new(), None, &bend(1.0, true), &p, &TableEdit::Value(1.2, "1.2".into()));
        assert_eq!(x.problem(), Some("A value is out of range"));
        assert_eq!(x.range_error().unwrap().message(), "K Factor must be between -1.5 and 1");
        assert!(value_error(BendCalc::KFactor, -1.5).is_none());
        assert!(value_error(BendCalc::BendAllowance, 0.0).is_some());
        assert!(value_error(BendCalc::BendDeduction, 0.0).is_none());
        // A rip doesn't care about the bend values.
        let r = table_edit(FeatureId::new(), Some(&x), &bend(1.0, true), &p, &TableEdit::ConvertToRip);
        assert_eq!(r.problem(), None);
    }

    #[test]
    fn round_trips_and_reads_old_documents() {
        let x = ModifyJointFeature { joint: Some(JointId(3)), joint_type: JointType::Rip, rip_style: RipStyle::ButtDirection2, ..Default::default() };
        let back: ModifyJointFeature = ron::from_str(&ron::to_string(&x).unwrap()).unwrap();
        assert_eq!(back, x);
        let old: ModifyJointFeature = ron::from_str("()").unwrap();
        assert_eq!(old.joint_type, JointType::Bend);
    }
}
