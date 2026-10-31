//! Variables in the app (P3F.4; `intro-to-parametric-cad.md` P5.2, P5.3, X2): the **Variable**
//! feature's dialog and the variables every numeric field reads (`#name`).
//!
//! - **Variable** (the toolbar's Variable button, or Edit on a Variable row): a feature dialog
//!   with the Type (Length, Angle, Number, Any), Name, Value (an expression with units, which
//!   may name the variables above it) and Description, and the value it evaluates to. The
//!   feature is listed as "#name". Names: `variable-dialog`, `variable-type`,
//!   `variable-name`, `variable-value`, `variable-description`, `variable-evaluated`.
//! - [`ActiveVariables`]: the active Part Studio's variables and their values, for the value
//!   fields of every dialog (sketch dimensions, depths, counts, hole sizes, offsets…). A field
//!   typed as an expression naming a variable keeps the expression (the feature's `*_expr`, a
//!   sketch's dimension expressions); a variable used above its definition fails that feature
//!   in the rebuild, with why ([`cadrs_core::variables::check`]).
//! - Typing `#` in a dialog's value field lists the variables the feature being edited can use,
//!   then New variable… ([`cadrs_ui::FieldVariables`]; the sketch's dimension editor lists them
//!   too, `crate::sketch_dimension`). New variable… inserts a Variable holding the field's value
//!   above the feature and puts its name in the field.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::assembly::commands::{AddMateFeature, SetMateFeature};
use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
use cadrs_core::commands::{AddFeature, MoveFeature, ReplaceFeature, SetFeature};
use cadrs_core::variables::{self, VariableFeature, VariableType};
use cadrs_core::{ElementId, ElementKind, Feature, FeatureId, FeatureKind};
use cadrs_sketch::units::{Quantity, Units, VarValue};
use cadrs_ui::ShortcutKey;
use cadrs_ui::{
    FeatureDialog, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, FieldVariables,
    ListedVariable, NewVariable, NumberField, NumberFieldCommit, NumberFieldEdit, NumberFieldState,
    TabStrip, TabStripSelect, Theme,
};

use crate::parts::PartCache;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct VariablesPlugin;

impl Plugin for VariablesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveVariables>()
            .add_systems(
                Update,
                (
                    sync_active_variables,
                    sync_field_variables,
                    variable_keys,
                    sync_variable_dialog,
                )
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |world: &mut World| {
                if world.contains_resource::<VariableSession>() {
                    finish(world);
                }
            })
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_type)
            .add_observer(on_field)
            .add_observer(on_field_new_variable)
            .add_observer(on_asm_variable_button);
    }
}

/// The active Part Studio's variables, top to bottom, with their values: what `#name` reads in
/// a value field.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ActiveVariables(pub Vec<(String, VarValue)>);

impl ActiveVariables {
    /// Evaluates a field's text: `units`' bare numbers, these variables.
    pub fn eval(
        &self,
        units: &Units,
        text: &str,
        q: Quantity,
    ) -> Result<f64, cadrs_sketch::units::ParseError> {
        units.eval_vars(text, q, &self.0)
    }
}

fn sync_active_variables(
    doc: Option<Res<ActiveDocument>>,
    units: Res<crate::WorkspaceUnits>,
    mut vars: ResMut<ActiveVariables>,
) {
    let Some(doc) = doc else {
        if !vars.0.is_empty() {
            vars.0.clear();
        }
        return;
    };
    if !doc.is_changed() && !units.is_changed() {
        return;
    }
    let want = doc
        .active_element()
        .map(|e| match e.assembly_model() {
            // P3F.4 (A1.8): an assembly's own variables, for its mate offsets.
            Some(asm) => cadrs_core::assembly::vars::defined(&e.variable_base(), asm, &units.0),
            // The Variable Studios' variables first, all of them (the rebuild's list has only
            // the ones named so far).
            None if matches!(e.kind, ElementKind::PartStudio { .. }) => {
                variables::defined_with(&e.variable_base(), &e.active_features(), &units.0)
            }
            None => Vec::new(),
        })
        .unwrap_or_default();
    if vars.0 != want {
        vars.0 = want;
    }
}

/// The value of `text` for a field of the feature being edited: a length, angle or count read
/// with the workspace units and the Part Studio's variables. (Errors are shown on the field.)
pub fn eval_field(
    world: &World,
    text: &str,
    q: Quantity,
) -> Result<f64, cadrs_sketch::units::ParseError> {
    let units = world.resource::<crate::WorkspaceUnits>().0;
    match world.get_resource::<ActiveVariables>() {
        Some(v) => v.eval(&units, text, q),
        None => units.eval(text, q),
    }
}

/// The variables a feature of a Part Studio sees: the Variable Studios', then the Variables
/// above it (P3F.4; the rebuild gives it no others). A name defined twice is listed once, with
/// the value that wins.
pub fn defined_before(
    world: &World,
    element: ElementId,
    feature: FeatureId,
) -> Vec<(String, VarValue)> {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element))
    else {
        return Vec::new();
    };
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let features = el.active_features();
    let at = features
        .iter()
        .position(|f| f.id == feature)
        .unwrap_or(features.len());
    let mut out: Vec<(String, VarValue)> = Vec::new();
    for (name, v) in variables::defined_with(&el.variable_base(), &features[..at], &units) {
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = v,
            None => out.push((name, v)),
        }
    }
    out
}

/// The Part Studio feature whose dialog (or sketch) is open: it can use the variables above it,
/// and a new one for its fields goes there.
pub fn edited_feature(world: &World) -> Option<(ElementId, FeatureId)> {
    if let Some(s) = world.get_resource::<crate::sketch::SketchSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::extrude::ExtrudeSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::applied::AppliedSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::boolean::BooleanSession>() {
        return Some((s.element, s.feature));
    }
    if let Some(s) = world.get_resource::<crate::sheetmetal_joint_ui::ModifyJointSession>() {
        return Some((s.element, s.feature));
    }
    // An assembly's Variable is a mate feature: not above anything in a feature list.
    if let Some(s) = world.get_resource::<VariableSession>()
        && s.mate.is_none()
    {
        return Some((s.element, s.feature));
    }
    None
}

/// What `#` lists in the dialogs' value fields ([`FieldVariables`]): the variables the feature
/// being edited can use, then New variable…; or, without one (an assembly's mates), the active
/// tab's. Worked out again when the document, the units or the feature edited change.
#[allow(clippy::type_complexity)]
fn sync_field_variables(
    world: &mut World,
    mut seen: Local<
        Option<(
            Option<(ElementId, FeatureId)>,
            Option<bevy::ecs::change_detection::Tick>,
            Option<bevy::ecs::change_detection::Tick>,
        )>,
    >,
) {
    let edited = edited_feature(world);
    let doc = world
        .get_resource_ref::<ActiveDocument>()
        .map(|d| d.last_changed());
    let units = world
        .get_resource_ref::<crate::WorkspaceUnits>()
        .map(|u| u.last_changed());
    let now = (edited, doc, units);
    if seen.as_ref() == Some(&now) {
        return;
    }
    *seen = Some(now);
    let vars = match edited {
        Some((element, feature)) => defined_before(world, element, feature),
        None => world.resource::<ActiveVariables>().0.clone(),
    };
    let u = world.resource::<crate::WorkspaceUnits>().0;
    let want = FieldVariables {
        variables: vars
            .into_iter()
            .map(|(name, v)| ListedVariable {
                name,
                value: u.with_unit(v.value, v.quantity),
            })
            .collect(),
        new_variable: edited.is_some(),
    };
    let mut have = world.resource_mut::<FieldVariables>();
    if *have != want {
        *have = want;
    }
}

/// New variable… in a dialog's value field: a Variable holding the field's value, inserted above
/// the feature being edited (so it can use it), and its name put in the field.
fn on_field_new_variable(
    ev: On<NewVariable>,
    q_edit: Query<&NumberFieldEdit>,
    mut commands: Commands,
) {
    let field = ev.entity;
    let Ok(&NumberFieldEdit(root)) = q_edit.get(field) else {
        return;
    };
    let wanted = ev.name.clone();
    commands.queue(move |world: &mut World| {
        let Some((element, feature)) = edited_feature(world) else {
            return;
        };
        let text = world
            .get::<NumberFieldState>(root)
            .map(|s| s.text.clone())
            .unwrap_or_default();
        let u = world.resource::<crate::WorkspaceUnits>().0;
        let vars = defined_before(world, element, feature);
        // The value the field holds, as a value of its own kind: "25 mm", "30 deg", "4".
        let v = cadrs_sketch::units::eval_any(&text, u.length.mm(), &vars).unwrap_or(VarValue {
            value: 0.0,
            quantity: Quantity::Length,
        });
        let var_type = match v.quantity {
            Quantity::Length => VariableType::Length,
            Quantity::Angle => VariableType::Angle,
            Quantity::Count => VariableType::Number,
        };
        let name = free_name(world, element, &wanted);
        let var = VariableFeature::typed(var_type, &name, &u.with_unit(v.value, v.quantity));
        if insert_variable_above(world, element, feature, var) {
            cadrs_ui::complete_variable(world, field, &name);
        }
    });
}

/// Whether a Variable of a Part Studio below `feature` defines `name` (so a feature above it
/// can't use it yet).
pub fn defined_below(world: &World, element: ElementId, feature: FeatureId, name: &str) -> bool {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element))
    else {
        return false;
    };
    el.features()
        .iter()
        .skip_while(|f| f.id != feature)
        .any(|f| matches!(&f.kind, FeatureKind::Variable(v) if v.name == name))
}

/// A name for a new Variable of `element`: `wanted` if it is a free variable name, else the
/// first free `var_N`.
pub fn free_name(world: &World, element: ElementId, wanted: &str) -> String {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element))
    else {
        return wanted.to_string();
    };
    let taken = |n: &str| {
        el.variable_base().iter().any(|(k, _)| k == n)
            || el
                .features()
                .iter()
                .any(|f| matches!(&f.kind, FeatureKind::Variable(v) if v.name == n))
    };
    if cadrs_sketch::units::is_variable_name(wanted) && !taken(wanted) {
        return wanted.to_string();
    }
    (1..)
        .map(|i| format!("var_{i}"))
        .find(|n| !taken(n))
        .unwrap_or_default()
}

/// Inserts the Variable `v` just above `feature` (one undo step, "Insert #name"); false if it
/// couldn't be.
pub fn insert_variable_above(
    world: &mut World,
    element: ElementId,
    feature: FeatureId,
    v: VariableFeature,
) -> bool {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    let label = format!("Insert #{}", v.name);
    let mark = doc.history.undo_len();
    let id = FeatureId::new();
    if let Err(e) = doc.execute(&AddFeature::variable(element, id, v)) {
        warn!("cannot insert a variable: {e}");
        return false;
    }
    let to = doc
        .doc
        .element(element)
        .and_then(|e| e.features().iter().position(|f| f.id == feature));
    if let Some(to) = to
        && let Err(e) = doc.execute(&MoveFeature {
            element,
            feature: id,
            to,
            label: label.clone(),
        })
    {
        warn!("cannot move the new variable above {feature:?}: {e}");
        // Take the insertion back.
        doc.undo();
        doc.discard_since(mark);
        return false;
    }
    doc.squash_since(mark, label);
    true
}

// ---------------------------------------------------------------------------------------------
// The Variable feature's dialog

/// The Variable whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct VariableSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub is_new: bool,
    pub mark: usize,
    pub before: Option<Feature>,
    /// The value field's text when it doesn't evaluate (shown red with why).
    pub value_error: Option<(String, String)>,
    /// The name field's text when it isn't a name.
    pub name_error: Option<String>,
    /// P3F.4 (A1.8): the Variable is in an assembly's Mate Features list (this mate feature),
    /// not a Part Studio's feature list.
    pub mate: Option<MateId>,
    /// The mate feature as it was (editing an assembly's Variable).
    pub before_mate: Option<MateFeature>,
}

#[derive(Component)]
struct VariableDialog;

/// The type the dialog was built for.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuiltFor(VariableType);

/// Which field a number field of the dialog is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Value,
    Description,
}

/// The line under the fields: what the value evaluates to, or why it can't.
#[derive(Component)]
struct Evaluated;

/// A new name: `var_1`, `var_2`, … not yet used in the Part Studio.
fn fresh_name(features: &[Feature]) -> String {
    (1..)
        .map(|i| format!("var_{i}"))
        .find(|n| {
            !features
                .iter()
                .any(|f| matches!(&f.kind, FeatureKind::Variable(v) if v.name == *n))
        })
        .unwrap_or_default()
}

/// Starts a new Variable in the active Part Studio (the toolbar's Variable button).
pub fn begin_variable(world: &mut World) {
    if world.contains_resource::<VariableSession>() {
        return;
    }
    close_other_sessions(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc
        .active_element()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
    else {
        return;
    };
    let element = el.id;
    let name = fresh_name(el.features());
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    let v = VariableFeature::length(&name, "0 mm");
    if let Err(e) = doc.execute(&AddFeature::variable(element, feature, v)) {
        warn!("cannot insert a variable: {e}");
        return;
    }
    start(world, element, feature, true, mark, None);
}

/// Opens an existing Variable for editing.
pub fn edit_variable(world: &mut World, feature: FeatureId) {
    if world
        .get_resource::<VariableSession>()
        .is_some_and(|s| s.feature == feature)
    {
        return;
    }
    if world.contains_resource::<VariableSession>() {
        finish(world);
    }
    close_other_sessions(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el
        .feature(feature)
        .filter(|f| matches!(f.kind, FeatureKind::Variable(_)))
        .cloned()
    else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    start(world, element, feature, false, mark, Some(before));
}

/// Accepts (or cancels) the other feature dialogs, as a new feature does.
fn close_other_sessions(world: &mut World) {
    if world.contains_resource::<crate::extrude::ExtrudeSession>() {
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
    if world.contains_resource::<crate::boolean::BooleanSession>() {
        crate::boolean::finish(world);
    }
}

fn start(
    world: &mut World,
    element: ElementId,
    feature: FeatureId,
    is_new: bool,
    mark: usize,
    before: Option<Feature>,
) {
    world.insert_resource(VariableSession {
        element,
        feature,
        is_new,
        mark,
        before,
        value_error: None,
        name_error: None,
        mate: None,
        before_mate: None,
    });
}

fn current(world: &World, s: &VariableSession) -> Option<Feature> {
    world
        .get_resource::<ActiveDocument>()?
        .doc
        .element(s.element)?
        .feature(s.feature)
        .cloned()
}

/// The edited Variable: its name in the list and its parameters (a Part Studio's feature, or an
/// assembly's mate feature).
fn edited(doc: &ActiveDocument, s: &VariableSession) -> Option<(String, VariableFeature)> {
    let el = doc.doc.element(s.element)?;
    match s.mate {
        Some(m) => {
            let f = el.assembly_model()?.mate(m)?;
            match &f.kind {
                MateKind::Variable(v) => Some((f.name.clone(), v.clone())),
                _ => None,
            }
        }
        None => {
            let f = el.feature(s.feature)?;
            match &f.kind {
                FeatureKind::Variable(v) => Some((f.name.clone(), v.clone())),
                _ => None,
            }
        }
    }
}

fn params(world: &World) -> Option<VariableFeature> {
    let s = world.get_resource::<VariableSession>()?;
    edited(world.get_resource::<ActiveDocument>()?, s).map(|(_, v)| v)
}

/// The variables above the one being edited.
fn above(world: &World, s: &VariableSession) -> Vec<(String, VarValue)> {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(s.element).cloned())
    else {
        return Vec::new();
    };
    let units = world.resource::<crate::WorkspaceUnits>().0;
    if let (Some(m), Some(asm)) = (s.mate, el.assembly_model()) {
        let mut upto = asm.clone();
        let at = upto
            .mates
            .iter()
            .position(|f| f.id == m)
            .unwrap_or(upto.mates.len());
        upto.mates.truncate(at);
        return cadrs_core::assembly::vars::defined(&el.variable_base(), &upto, &units);
    }
    let features = el.active_features();
    let at = features
        .iter()
        .position(|f| f.id == s.feature)
        .unwrap_or(features.len());
    variables::defined_with(&el.variable_base(), &features[..at], &units)
}

/// P3F.4 (A1.8): replaces an assembly's Variable, the mates whose offsets name it re-solved
/// (one undo step).
pub fn set_asm_variable(
    world: &mut World,
    element: ElementId,
    id: MateId,
    v: VariableFeature,
    _label: &str,
) -> bool {
    let Some(feature) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element)?.assembly_model()?.mate(id).cloned())
        .map(|f| MateFeature {
            kind: MateKind::Variable(v),
            name: f.name.clone(),
            ..f
        })
    else {
        return false;
    };
    let feature = MateFeature {
        name: match &feature.kind {
            MateKind::Variable(v) => format!("#{}", v.name),
            _ => feature.name.clone(),
        },
        ..feature
    };
    let poses = solved_with(world, &feature);
    crate::assembly::run(
        world,
        &SetMateFeature {
            element,
            feature,
            poses,
        },
    )
}

/// The placements the assembly takes with `feature` in it and its variables refreshed.
fn solved_with(
    world: &mut World,
    feature: &MateFeature,
) -> Vec<(cadrs_core::assembly::InstanceId, cadrs_core::assembly::Pose)> {
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let Some((doc_model, solids)) = crate::assembly::mate_dialog::model_and_solids(world) else {
        return Vec::new();
    };
    let mut model = doc_model.clone();
    match model.mates.iter_mut().find(|f| f.id == feature.id) {
        Some(f) => *f = feature.clone(),
        None => model.mates.push(feature.clone()),
    }
    let base = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().map(|e| e.variable_base()))
        .unwrap_or_default();
    if !cadrs_core::assembly::vars::refresh(&base, &mut model, &units) {
        return Vec::new();
    }
    let sol = cadrs_core::assembly::solve(
        &model,
        &solids,
        &cadrs_core::assembly::solver::SolveOptions::default(),
    );
    sol.changed(&doc_model)
}

fn set(world: &mut World, v: VariableFeature, label: &str) {
    let Some(s) = world.get_resource::<VariableSession>().cloned() else {
        return;
    };
    if let Some(m) = s.mate {
        set_asm_variable(world, s.element, m, v, label);
        return;
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    if let Err(e) = doc.execute(&SetFeature {
        element: s.element,
        feature: s.feature,
        kind: FeatureKind::Variable(v),
        label: label.into(),
    }) {
        warn!("cannot change the variable: {e}");
    }
}

/// P3F.4 (A1.8): a new Variable at the end of the active assembly's Mate Features list (its
/// toolbar's Variable button).
pub fn begin_asm_variable(world: &mut World) {
    if world.contains_resource::<VariableSession>() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let Some(asm) = el.assembly_model() else {
        return;
    };
    let element = el.id;
    let name = (1..)
        .map(|i| format!("var_{i}"))
        .find(|n| {
            !asm.mates
                .iter()
                .any(|f| matches!(&f.kind, MateKind::Variable(v) if v.name == *n))
        })
        .unwrap_or_default();
    let mark = doc.history.undo_len();
    let id = MateId::new();
    let feature = MateFeature::new(
        id,
        format!("#{name}"),
        MateKind::Variable(VariableFeature::length(&name, "0 mm")),
    );
    if !crate::assembly::run(
        world,
        &AddMateFeature {
            element,
            feature,
            poses: Vec::new(),
        },
    ) {
        return;
    }
    world.insert_resource(VariableSession {
        element,
        feature: FeatureId::new(),
        is_new: true,
        mark,
        before: None,
        value_error: None,
        name_error: None,
        mate: Some(id),
        before_mate: None,
    });
}

fn on_asm_variable_button(
    a: On<bevy::ui_widgets::Activate>,
    q: Query<&Name>,
    mut commands: Commands,
) {
    if q.get(a.entity).map(|n| n.as_str()) == Ok("asm-variable") {
        commands.queue(begin_asm_variable);
    }
}

/// Opens an assembly's Variable for editing (a double-click on its row).
pub fn edit_asm_variable(world: &mut World, id: MateId) {
    if world.contains_resource::<VariableSession>() {
        finish(world);
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el
        .assembly_model()
        .and_then(|a| a.mate(id))
        .filter(|f| matches!(f.kind, MateKind::Variable(_)))
        .cloned()
    else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    world.insert_resource(VariableSession {
        element,
        feature: FeatureId::new(),
        is_new: false,
        mark,
        before: None,
        value_error: None,
        name_error: None,
        mate: Some(id),
        before_mate: Some(before),
    });
}

/// ✓ / Enter: keeps the Variable if it is defined.
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<VariableSession>().cloned() else {
        return;
    };
    let Some((name, v)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| edited(d, &s))
    else {
        return;
    };
    if v.problem().is_some() || s.value_error.is_some() || s.name_error.is_some() {
        return;
    }
    let label = if s.is_new {
        format!("Insert {name}")
    } else {
        format!("Edit {name}")
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, label);
    }
    world.remove_resource::<VariableSession>();
}

/// ✕ / Esc: removes a new Variable or reverts an edit.
pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<VariableSession>().cloned() else {
        return;
    };
    let cur = current(world, &s);
    // An assembly's Variable: the edit undone, its mates re-solved.
    if let (Some(_), Some(before)) = (s.mate, s.before_mate.clone()) {
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
            doc.squash_element_since(s.mark, s.element, format!("Edit {}", before.name));
        }
        let now = world
            .get_resource::<ActiveDocument>()
            .and_then(|d| edited(d, &s))
            .map(|(_, v)| v);
        if let (MateKind::Variable(v), Some(now)) = (&before.kind, now)
            && *v != now
            && let Some(m) = s.mate
        {
            set_asm_variable(
                world,
                s.element,
                m,
                v.clone(),
                &format!("Cancel {}", before.name),
            );
        }
        world.remove_resource::<VariableSession>();
        return;
    }
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let name = cur.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        if s.is_new {
            doc.discard_element_since(s.mark, s.element);
        } else {
            doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
            if let (Some(before), Some(f)) = (s.before.clone(), cur)
                && f != before
            {
                let _ = doc.execute(&ReplaceFeature {
                    element: s.element,
                    feature: before,
                    label: format!("Cancel {name}"),
                });
            }
        }
    }
    world.remove_resource::<VariableSession>();
}

/// Accepts if valid, otherwise cancels.
pub fn finish(world: &mut World) {
    accept(world);
    if world.contains_resource::<VariableSession>() {
        cancel(world);
    }
}

fn variable_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<VariableSession>>,
    focus: Res<InputFocus>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut commands: Commands,
) {
    if session.is_none() {
        keys.clear();
        return;
    }
    for k in keys.read() {
        if k.state != ButtonState::Pressed || !q_dialogs.is_empty() || focus.get().is_some() {
            continue;
        }
        match k.shortcut_code() {
            KeyCode::Enter | KeyCode::NumpadEnter => commands.queue(accept),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

fn on_accept(
    ev: On<FeatureDialogAccept>,
    q: Query<(), With<VariableDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(
    ev: On<FeatureDialogCancel>,
    q: Query<(), With<VariableDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

fn on_type(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("variable-type") {
        return;
    }
    let Some(t) = VariableType::ALL.get(ev.index).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let Some(mut v) = params(world) else { return };
        if v.var_type == t {
            return;
        }
        v.var_type = t;
        // A plain value takes the type's unit ("0 mm" → "0 deg").
        let bare = v
            .expr
            .split_whitespace()
            .next()
            .filter(|w| w.parse::<f64>().is_ok())
            .map(str::to_string);
        if let Some(n) = bare
            && !v.expr.contains('#')
        {
            v.expr = match t {
                VariableType::Length => format!(
                    "{n} {}",
                    world.resource::<crate::WorkspaceUnits>().0.length.symbol()
                ),
                VariableType::Angle => format!("{n} deg"),
                VariableType::Number | VariableType::Any => n,
            };
        }
        let s = world.resource::<VariableSession>().clone();
        let vars = above(world, &s);
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let err = v
            .evaluate(&units, &vars)
            .err()
            .map(|e| (v.expr.clone(), e.to_string()));
        world.resource_mut::<VariableSession>().value_error = err;
        set(world, v, "Variable type");
    });
}

fn on_field(ev: On<NumberFieldCommit>, q: Query<&Field>, mut commands: Commands) {
    let Ok(field) = q.get(ev.entity).copied() else {
        return;
    };
    let text = ev.text.trim().to_string();
    let enter = ev.enter;
    commands.queue(move |world: &mut World| {
        let Some(mut v) = params(world) else { return };
        let Some(s) = world.get_resource::<VariableSession>().cloned() else {
            return;
        };
        match field {
            Field::Name => {
                let name = text.trim_start_matches('#').to_string();
                if !cadrs_sketch::units::is_variable_name(&name) {
                    world.resource_mut::<VariableSession>().name_error = Some(text);
                    return;
                }
                world.resource_mut::<VariableSession>().name_error = None;
                if v.name != name {
                    v.name = name;
                    set(world, v, "Variable name");
                }
            }
            Field::Value => {
                let vars = above(world, &s);
                let units = world.resource::<crate::WorkspaceUnits>().0;
                // A bare number gets the type's unit, as the other fields show it.
                let expr = match (text.parse::<f64>().is_ok(), v.var_type) {
                    (true, VariableType::Length) => format!("{text} {}", units.length.symbol()),
                    (true, VariableType::Angle) => format!("{text} deg"),
                    _ => text.clone(),
                };
                let mut next = VariableFeature { expr, ..v.clone() };
                match next.evaluate(&units, &vars) {
                    Ok(()) => {
                        world.resource_mut::<VariableSession>().value_error = None;
                        if next != v {
                            set(world, next, "Variable value");
                        }
                    }
                    Err(e) => {
                        world.resource_mut::<VariableSession>().value_error =
                            Some((text, e.to_string()));
                        return;
                    }
                }
            }
            Field::Description => {
                if v.description != text {
                    v.description = text;
                    set(world, v, "Variable description");
                }
            }
        }
        if enter {
            world.resource_mut::<InputFocus>().clear();
            accept(world);
        }
    });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_variable_dialog(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<VariableSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    units: Res<crate::WorkspaceUnits>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_dialog: Query<(Entity, &BuiltFor, &mut FeatureDialogState), With<VariableDialog>>,
    mut q_fields: Query<(&Field, &mut NumberFieldState)>,
    mut q_eval: Query<(&mut Text, &mut TextColor), With<Evaluated>>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some((title, v)) = edited(&doc, &s) else {
        return;
    };
    let v = &v;
    let error = match s.mate {
        Some(m) => doc
            .doc
            .element(s.element)
            .and_then(|e| Some((e.variable_base(), e.assembly_model()?)))
            .and_then(|(base, a)| {
                cadrs_core::assembly::vars::check(&base, a, &units.0)
                    .into_iter()
                    .find(|(id, _)| *id == m)
                    .map(|(_, w)| w)
            }),
        None => cache.errors.get(&s.feature).cloned(),
    };
    let valid = v.problem().is_none() && s.value_error.is_none() && s.name_error.is_none();
    let layout = BuiltFor(v.var_type);
    let built = q_dialog.iter().next().map(|(e, k, _)| (e, *k));
    if built.is_none_or(|(_, k)| k != layout) {
        if let Some((e, _)) = built {
            commands.entity(e).try_despawn();
        }
        let Some(area) = q_area.iter().next() else {
            return;
        };
        let t = theme.clone();
        let v = v.clone();
        let dialog = commands
            .spawn((
                VariableDialog,
                layout,
                DespawnOnExit(AppState::Document),
                FeatureDialog::new("variable-dialog")
                    .title(title.clone())
                    .valid(valid)
                    .width(232.0)
                    .body_padding(UiRect::ZERO)
                    .body(move |p| {
                        let mut strip = TabStrip::new("variable-type").compact();
                        for k in VariableType::ALL {
                            strip = strip.tab(k.label());
                        }
                        let i = VariableType::ALL
                            .iter()
                            .position(|k| *k == v.var_type)
                            .unwrap_or(0);
                        p.spawn(strip.selected(i).build(&t));
                        p.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::new(
                                Val::Px(4.0),
                                Val::Px(4.0),
                                Val::Px(6.0),
                                Val::Px(6.0),
                            ),
                            row_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|b| {
                            b.spawn((
                                Field::Name,
                                NumberField::new("variable-name", "Name  #")
                                    .text(v.name.clone())
                                    .label_width(74.0)
                                    .build(&t),
                            ));
                            b.spawn((
                                Field::Value,
                                NumberField::new("variable-value", "Value")
                                    .text(v.expr.clone())
                                    .label_width(74.0)
                                    .variables()
                                    .build(&t),
                            ));
                            b.spawn((
                                Evaluated,
                                Name::new("variable-evaluated"),
                                t.text(String::new(), 11.0, FontWeight::NORMAL, t.muted_foreground),
                                Node {
                                    margin: UiRect::new(
                                        Val::Px(78.0),
                                        Val::ZERO,
                                        Val::ZERO,
                                        Val::Px(2.0),
                                    ),
                                    ..default()
                                },
                            ));
                            b.spawn((
                                Field::Description,
                                NumberField::new("variable-description", "Description")
                                    .text(v.description.clone())
                                    .label_width(74.0)
                                    .build(&t),
                                // Text: the wheel doesn't step it ("3 holes").
                                cadrs_ui::dialog_fields::NoWheelStep,
                            ));
                        });
                    })
                    .build(&theme),
            ))
            .id();
        commands.entity(area).add_child(dialog);
        return;
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState {
            title: title.clone(),
            valid,
            error: error.is_some(),
        };
        if *st != want {
            *st = want;
        }
    }
    for (field, mut st) in &mut q_fields {
        let want = match field {
            Field::Name => match &s.name_error {
                Some(t) => NumberFieldState {
                    text: t.clone(),
                    error: true,
                },
                None => NumberFieldState {
                    text: v.name.clone(),
                    error: false,
                },
            },
            Field::Value => match &s.value_error {
                Some((t, _)) => NumberFieldState {
                    text: t.clone(),
                    error: true,
                },
                None => NumberFieldState {
                    text: v.expr.clone(),
                    error: error.is_some(),
                },
            },
            Field::Description => NumberFieldState {
                text: v.description.clone(),
                error: false,
            },
        };
        if *st != want {
            *st = want;
        }
    }
    let (line, red) = match (&s.name_error, &s.value_error, &error) {
        (Some(_), ..) => ("A name is letters, digits and _".to_string(), true),
        (None, Some((_, why)), _) => (why.clone(), true),
        (None, None, Some(why)) => (why.clone(), true),
        _ => (format!("= {}", v.display(&units.0)), false),
    };
    for (mut text, mut color) in &mut q_eval {
        if text.0 != line {
            text.0 = line.clone();
        }
        let c = if red {
            theme.feature_error
        } else {
            theme.muted_foreground
        };
        if color.0 != c {
            color.0 = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::Document;
    use cadrs_core::document::SketchFeature;

    /// A Part Studio of `#above`, a feature with its dialog open (a Boolean's) and `#below`.
    fn dialog_app() -> (App, ElementId, FeatureId) {
        let var = |name: &str, expr: &str| {
            Feature::new(
                FeatureId::new(),
                format!("#{name}"),
                FeatureKind::Variable(VariableFeature::length(name, expr)),
            )
        };
        let mut doc = Document::new("Variables");
        let element = doc.elements[0].id;
        let feature = FeatureId::new();
        let features = doc.elements[0].features_mut().unwrap();
        features.push(var("above", "10 mm"));
        features.push(Feature::new(
            feature,
            "Sketch 1",
            FeatureKind::Sketch(SketchFeature {
                plane: Some(cadrs_sketch::PlaneRef::Top),
                disable_imprinting: true,
                geometry: cadrs_sketch::Sketch::new(),
            }),
        ));
        features.push(var("below", "4 mm"));
        let mut app = App::new();
        app.insert_resource(ActiveDocument::new(doc))
            .init_resource::<crate::WorkspaceUnits>()
            .init_resource::<ActiveVariables>()
            .init_resource::<FieldVariables>()
            .insert_resource(crate::boolean::BooleanSession {
                element,
                feature,
                is_new: false,
                mark: 0,
                before: None,
                targets_active: false,
                show_final: false,
                offset_faces_active: false,
            })
            .add_observer(on_field_new_variable)
            .add_systems(Update, sync_field_variables);
        (app, element, feature)
    }

    fn names(w: &World, element: ElementId) -> Vec<String> {
        let doc = w.resource::<ActiveDocument>();
        let el = doc.doc.element(element).unwrap();
        el.features().iter().map(|f| f.name.clone()).collect()
    }

    /// A value field of an open dialog, showing `text`.
    fn field(w: &mut World, text: &str) -> Entity {
        let root = w
            .spawn(NumberFieldState {
                text: text.into(),
                error: false,
            })
            .id();
        w.spawn((
            NumberFieldEdit(root),
            bevy::text::EditableText::new("#"),
            cadrs_ui::VariableList::new(Vec::new()),
        ))
        .id()
    }

    #[test]
    fn a_dialog_field_lists_the_variables_above_its_feature() {
        let (mut app, ..) = dialog_app();
        app.update();
        assert_eq!(
            *app.world().resource::<FieldVariables>(),
            FieldVariables {
                variables: vec![ListedVariable {
                    name: "above".into(),
                    value: "10 mm".into(),
                }],
                new_variable: true,
            }
        );
        // No feature being edited: no New variable… (nowhere to put it).
        app.world_mut()
            .remove_resource::<crate::boolean::BooleanSession>();
        app.update();
        assert!(!app.world().resource::<FieldVariables>().new_variable);
    }

    #[test]
    fn a_new_variable_from_a_dialog_field_holds_its_value_above_the_feature() {
        let (mut app, element, _) = dialog_app();
        let w = app.world_mut();
        let steps = w.resource::<ActiveDocument>().history.undo_len();
        let depth = field(w, "25 mm");
        w.trigger(NewVariable {
            entity: depth,
            name: "depth".into(),
        });
        w.flush();
        assert_eq!(
            names(w, element),
            ["#above", "#depth", "Sketch 1", "#below"]
        );
        assert_eq!(w.resource::<ActiveDocument>().history.undo_len(), steps + 1);
        // A count makes a Number; a taken name falls back to var_N.
        let count = field(w, "4");
        w.trigger(NewVariable {
            entity: count,
            name: "below".into(),
        });
        w.flush();
        let doc = w.resource::<ActiveDocument>();
        let vars: Vec<(String, VariableType, String)> = doc
            .doc
            .element(element)
            .unwrap()
            .features()
            .iter()
            .filter_map(|f| match &f.kind {
                FeatureKind::Variable(v) => Some((v.name.clone(), v.var_type, v.expr.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            vars,
            [
                ("above".into(), VariableType::Length, "10 mm".into()),
                ("depth".into(), VariableType::Length, "25 mm".into()),
                ("var_1".into(), VariableType::Number, "4".into()),
                ("below".into(), VariableType::Length, "4 mm".into()),
            ]
        );
    }
}
