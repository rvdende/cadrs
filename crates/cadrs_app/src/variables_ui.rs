//! Variables in the app (P3F.4; `intro-to-parametric-cad.md` P5.2, P5.3, X2): the **Variable**
//! feature's dialog, the **Variable table** panel on the right strip, and the variables every
//! numeric field reads (`#name`).
//!
//! - **Variable** (the toolbar's Variable button, or Edit on a Variable row): a feature dialog
//!   with the Type (Length, Angle, Number, Any), Name, Value (an expression with units, which
//!   may name the variables above it) and Description, and the value it evaluates to. The
//!   feature is listed as "#name". Names: `variable-dialog`, `variable-type`,
//!   `variable-name`, `variable-value`, `variable-description`, `variable-evaluated`.
//! - **Variable table** (`variables-panel`): one row per variable of the Part Studio, in list
//!   order: the name, the value as an editable expression (`variable-row-<name>`: typing
//!   `50 mm` there is one undo step that updates every use) with what it evaluates to, and the
//!   description. A variable in error (used before it is defined) shows red with why.
//! - [`ActiveVariables`]: the active Part Studio's variables and their values, for the value
//!   fields of every dialog (sketch dimensions, depths, counts, hole sizes, offsets…). A field
//!   typed as an expression naming a variable keeps the expression (the feature's `*_expr`, a
//!   sketch's dimension expressions); a variable used above its definition fails that feature
//!   in the rebuild, with why ([`cadrs_core::variables::check`]).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::assembly::commands::{AddMateFeature, SetMateFeature};
use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
use cadrs_core::commands::{AddFeature, ReplaceFeature, SetFeature};
use cadrs_core::variables::{self, VariableFeature, VariableType};
use cadrs_core::{ElementId, ElementKind, Feature, FeatureId, FeatureKind};
use cadrs_sketch::units::{Quantity, Units, VarValue};
use cadrs_ui::{
    FeatureDialog, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit,
    NumberFieldState, TabStrip, TabStripSelect, Theme,
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
                (sync_active_variables, variable_keys, sync_variable_dialog, sync_variable_table)
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
            .add_observer(on_table_value)
            .add_observer(on_asm_variable_button);
    }
}

/// The active Part Studio's variables, top to bottom, with their values: what `#name` reads in
/// a value field.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ActiveVariables(pub Vec<(String, VarValue)>);

impl ActiveVariables {
    /// Evaluates a field's text: `units`' bare numbers, these variables.
    pub fn eval(&self, units: &Units, text: &str, q: Quantity) -> Result<f64, cadrs_sketch::units::ParseError> {
        units.eval_vars(text, q, &self.0)
    }
}

fn sync_active_variables(doc: Option<Res<ActiveDocument>>, units: Res<crate::WorkspaceUnits>, mut vars: ResMut<ActiveVariables>) {
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
            Some(asm) => cadrs_core::assembly::vars::defined(asm, &units.0),
            None if matches!(e.kind, ElementKind::PartStudio { .. }) => variables::defined(&e.active_features(), &units.0),
            None => Vec::new(),
        })
        .unwrap_or_default();
    if vars.0 != want {
        vars.0 = want;
    }
}

/// The value of `text` for a field of the feature being edited: a length, angle or count read
/// with the workspace units and the Part Studio's variables. (Errors are shown on the field.)
pub fn eval_field(world: &World, text: &str, q: Quantity) -> Result<f64, cadrs_sketch::units::ParseError> {
    let units = world.resource::<crate::WorkspaceUnits>().0;
    match world.get_resource::<ActiveVariables>() {
        Some(v) => v.eval(&units, text, q),
        None => units.eval(text, q),
    }
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
        .find(|n| !features.iter().any(|f| matches!(&f.kind, FeatureKind::Variable(v) if v.name == *n)))
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
    let Some(el) = doc.active_element().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. })) else {
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
    if world.get_resource::<VariableSession>().is_some_and(|s| s.feature == feature) {
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
    let Some(before) = el.feature(feature).filter(|f| matches!(f.kind, FeatureKind::Variable(_))).cloned() else {
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

fn start(world: &mut World, element: ElementId, feature: FeatureId, is_new: bool, mark: usize, before: Option<Feature>) {
    world.insert_resource(VariableSession { element, feature, is_new, mark, before, value_error: None, name_error: None, mate: None, before_mate: None });
}

fn current(world: &World, s: &VariableSession) -> Option<Feature> {
    world.get_resource::<ActiveDocument>()?.doc.element(s.element)?.feature(s.feature).cloned()
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
    let Some(el) = world.get_resource::<ActiveDocument>().and_then(|d| d.doc.element(s.element).cloned()) else {
        return Vec::new();
    };
    let units = world.resource::<crate::WorkspaceUnits>().0;
    if let (Some(m), Some(asm)) = (s.mate, el.assembly_model()) {
        let mut upto = asm.clone();
        let at = upto.mates.iter().position(|f| f.id == m).unwrap_or(upto.mates.len());
        upto.mates.truncate(at);
        return cadrs_core::assembly::vars::defined(&upto, &units);
    }
    let features = el.active_features();
    let at = features.iter().position(|f| f.id == s.feature).unwrap_or(features.len());
    variables::defined(&features[..at], &units)
}

/// P3F.4 (A1.8): replaces an assembly's Variable, the mates whose offsets name it re-solved
/// (one undo step).
pub fn set_asm_variable(world: &mut World, element: ElementId, id: MateId, v: VariableFeature, _label: &str) -> bool {
    let Some(feature) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element)?.assembly_model()?.mate(id).cloned())
        .map(|f| MateFeature { kind: MateKind::Variable(v), name: f.name.clone(), ..f })
    else {
        return false;
    };
    let feature = MateFeature { name: match &feature.kind { MateKind::Variable(v) => format!("#{}", v.name), _ => feature.name.clone() }, ..feature };
    let poses = solved_with(world, &feature);
    crate::assembly::run(world, &SetMateFeature { element, feature, poses })
}

/// The placements the assembly takes with `feature` in it and its variables refreshed.
fn solved_with(world: &mut World, feature: &MateFeature) -> Vec<(cadrs_core::assembly::InstanceId, cadrs_core::assembly::Pose)> {
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let Some((doc_model, solids)) = crate::assembly::mate_dialog::model_and_solids(world) else { return Vec::new() };
    let mut model = doc_model.clone();
    match model.mates.iter_mut().find(|f| f.id == feature.id) {
        Some(f) => *f = feature.clone(),
        None => model.mates.push(feature.clone()),
    }
    if !cadrs_core::assembly::vars::refresh(&mut model, &units) {
        return Vec::new();
    }
    let sol = cadrs_core::assembly::solve(&model, &solids, &cadrs_core::assembly::solver::SolveOptions::default());
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
    if let Err(e) = doc.execute(&SetFeature { element: s.element, feature: s.feature, kind: FeatureKind::Variable(v), label: label.into() }) {
        warn!("cannot change the variable: {e}");
    }
}

/// P3F.4 (A1.8): a new Variable at the end of the active assembly's Mate Features list (its
/// toolbar's Variable button).
pub fn begin_asm_variable(world: &mut World) {
    if world.contains_resource::<VariableSession>() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element() else { return };
    let Some(asm) = el.assembly_model() else { return };
    let element = el.id;
    let name = (1..).map(|i| format!("var_{i}")).find(|n| !asm.mates.iter().any(|f| matches!(&f.kind, MateKind::Variable(v) if v.name == *n))).unwrap_or_default();
    let mark = doc.history.undo_len();
    let id = MateId::new();
    let feature = MateFeature::new(id, format!("#{name}"), MateKind::Variable(VariableFeature::length(&name, "0 mm")));
    if !crate::assembly::run(world, &AddMateFeature { element, feature, poses: Vec::new() }) {
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

fn on_asm_variable_button(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).map(|n| n.as_str()) == Ok("asm-variable") {
        commands.queue(begin_asm_variable);
    }
}

/// Opens an assembly's Variable for editing (a double-click on its row).
pub fn edit_asm_variable(world: &mut World, id: MateId) {
    if world.contains_resource::<VariableSession>() {
        finish(world);
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element() else { return };
    let element = el.id;
    let Some(before) = el.assembly_model().and_then(|a| a.mate(id)).filter(|f| matches!(f.kind, MateKind::Variable(_))).cloned() else { return };
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
    let Some((name, v)) = world.get_resource::<ActiveDocument>().and_then(|d| edited(d, &s)) else {
        return;
    };
    if v.problem().is_some() || s.value_error.is_some() || s.name_error.is_some() {
        return;
    }
    let label = if s.is_new { format!("Insert {name}") } else { format!("Edit {name}") };
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
        let now = world.get_resource::<ActiveDocument>().and_then(|d| edited(d, &s)).map(|(_, v)| v);
        if let (MateKind::Variable(v), Some(now)) = (&before.kind, now)
            && *v != now
            && let Some(m) = s.mate
        {
            set_asm_variable(world, s.element, m, v.clone(), &format!("Cancel {}", before.name));
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
                let _ = doc.execute(&ReplaceFeature { element: s.element, feature: before, label: format!("Cancel {name}") });
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
        match k.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter => commands.queue(accept),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<VariableDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<VariableDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

fn on_type(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("variable-type") {
        return;
    }
    let Some(t) = VariableType::ALL.get(ev.index).copied() else { return };
    commands.queue(move |world: &mut World| {
        let Some(mut v) = params(world) else { return };
        if v.var_type == t {
            return;
        }
        v.var_type = t;
        // A plain value takes the type's unit ("0 mm" → "0 deg").
        let bare = v.expr.split_whitespace().next().filter(|w| w.parse::<f64>().is_ok()).map(str::to_string);
        if let Some(n) = bare
            && !v.expr.contains('#')
        {
            v.expr = match t {
                VariableType::Length => format!("{n} {}", world.resource::<crate::WorkspaceUnits>().0.length.symbol()),
                VariableType::Angle => format!("{n} deg"),
                VariableType::Number | VariableType::Any => n,
            };
        }
        let s = world.resource::<VariableSession>().clone();
        let vars = above(world, &s);
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let err = v.evaluate(&units, &vars).err().map(|e| (v.expr.clone(), e.to_string()));
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
        let Some(s) = world.get_resource::<VariableSession>().cloned() else { return };
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
                        world.resource_mut::<VariableSession>().value_error = Some((text, e.to_string()));
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
            .and_then(|e| e.assembly_model())
            .and_then(|a| cadrs_core::assembly::vars::check(a, &units.0).into_iter().find(|(id, _)| *id == m).map(|(_, w)| w)),
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
                        let i = VariableType::ALL.iter().position(|k| *k == v.var_type).unwrap_or(0);
                        p.spawn(strip.selected(i).build(&t));
                        p.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(6.0), Val::Px(6.0)),
                            row_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|b| {
                            b.spawn((Field::Name, NumberField::new("variable-name", "Name  #").text(v.name.clone()).label_width(74.0).build(&t)));
                            b.spawn((Field::Value, NumberField::new("variable-value", "Value").text(v.expr.clone()).label_width(74.0).build(&t)));
                            b.spawn((
                                Evaluated,
                                Name::new("variable-evaluated"),
                                t.text(String::new(), 11.0, FontWeight::NORMAL, t.muted_foreground),
                                Node { margin: UiRect::new(Val::Px(78.0), Val::ZERO, Val::ZERO, Val::Px(2.0)), ..default() },
                            ));
                            b.spawn((
                                Field::Description,
                                NumberField::new("variable-description", "Description").text(v.description.clone()).label_width(74.0).build(&t),
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
        let want = FeatureDialogState { title: title.clone(), valid, error: error.is_some() };
        if *st != want {
            *st = want;
        }
    }
    for (field, mut st) in &mut q_fields {
        let want = match field {
            Field::Name => match &s.name_error {
                Some(t) => NumberFieldState { text: t.clone(), error: true },
                None => NumberFieldState { text: v.name.clone(), error: false },
            },
            Field::Value => match &s.value_error {
                Some((t, _)) => NumberFieldState { text: t.clone(), error: true },
                None => NumberFieldState { text: v.expr.clone(), error: error.is_some() },
            },
            Field::Description => NumberFieldState { text: v.description.clone(), error: false },
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
        let c = if red { theme.feature_error } else { theme.muted_foreground };
        if color.0 != c {
            color.0 = c;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Variable table

/// The rows container of the Variables panel (spawned by the panel,
/// [`crate::appearance`]); filled here.
#[derive(Component)]
pub struct VariableTableRows;

/// What the rows show (they are rebuilt when it changes).
#[derive(Component, Debug, Clone, PartialEq)]
struct TableFor(Vec<(FeatureId, String, String, String, String, Option<String>)>);

/// A row's value field: the Variable it edits.
#[derive(Component, Debug, Clone, Copy)]
struct TableValue(FeatureId);

#[allow(clippy::type_complexity)]
fn sync_variable_table(
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    units: Res<crate::WorkspaceUnits>,
    q_rows: Query<(Entity, Option<&TableFor>), With<VariableTableRows>>,
    mut commands: Commands,
) {
    let Some((container, built)) = q_rows.iter().next() else {
        return;
    };
    let Some(doc) = doc else { return };
    let el = doc.active_element();
    let studio = el.is_some_and(|e| matches!(e.kind, ElementKind::PartStudio { .. }));
    let asm_rows: Option<Vec<(FeatureId, String, String, String, String, Option<String>)>> = el.and_then(|e| e.assembly_model()).map(|a| {
        let errs = cadrs_core::assembly::vars::check(a, &units.0);
        a.mates
            .iter()
            .filter_map(|f| match &f.kind {
                MateKind::Variable(v) => Some((
                    FeatureId(f.id.0),
                    format!("#{}", v.name),
                    v.expr.clone(),
                    v.display(&units.0),
                    v.description.clone(),
                    errs.iter().find(|(id, _)| *id == f.id).map(|(_, w)| w.clone()).or_else(|| f.suppressed.then(|| "Suppressed".to_string())),
                )),
                _ => None,
            })
            .collect()
    });
    let rows: Vec<(FeatureId, String, String, String, String, Option<String>)> = asm_rows.unwrap_or_else(|| el
        .map(|e| {
            let off = e.all_suppressed();
            e.features()
                .iter()
                .filter_map(|f| match &f.kind {
                    FeatureKind::Variable(v) => Some((
                        f.id,
                        format!("#{}", v.name),
                        v.expr.clone(),
                        v.display(&units.0),
                        v.description.clone(),
                        cache.errors.get(&f.id).cloned().or_else(|| off.contains(&f.id).then(|| "Suppressed".to_string())),
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default());
    let asm = el.is_some_and(|e| e.assembly_model().is_some());
    let want = TableFor(rows.clone());
    if built == Some(&want) {
        return;
    }
    let t = theme.clone();
    commands.entity(container).despawn_children().insert(want);
    commands.entity(container).with_children(|p| {
        if rows.is_empty() {
            let msg = if studio {
                "No variables in this Part Studio. Add one with the Variable feature (ƒx) in the toolbar."
            } else if asm {
                "No variables in this Assembly. Add one with Variable (ƒx) in the toolbar; mate offsets can use them."
            } else {
                "Variables are defined in Part Studios and Assemblies (the Variable feature)."
            };
            p.spawn((
                Name::new("variables-panel-empty"),
                t.text(msg, 11.5, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::all(Val::Px(12.0)), width: Val::Px(214.0), ..default() },
            ))
            .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            return;
        }
        for (id, name, expr, value, description, error) in rows {
            let row_name = format!("variable-row-{}", name.trim_start_matches('#'));
            // P3F.5 (P3F.3–P3F.4 judge): the name in the foreground colour with its description
            // under it; the value field, and what it evaluates to (or why it fails) under it.
            p.spawn((
                Name::new(format!("{row_name}-line")),
                Node {
                    align_items: AlignItems::FlexStart,
                    column_gap: Val::Px(4.0),
                    padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(3.0), Val::Px(3.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(t.panel_border),
            ))
            .with_children(|r| {
                r.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(76.0), flex_shrink: 0.0, padding: UiRect::top(Val::Px(5.0)), row_gap: Val::Px(2.0), ..default() }).with_children(|c| {
                    c.spawn((Name::new(format!("{row_name}-name")), t.text(name.clone(), 11.5, FontWeight::MEDIUM, t.foreground)));
                    if !description.is_empty() {
                        c.spawn((
                            Name::new(format!("{row_name}-description")),
                            t.text(description.clone(), 10.5, FontWeight::NORMAL, t.muted_foreground),
                            Node { width: Val::Px(76.0), ..default() },
                        ))
                        .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
                    }
                });
                r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(|c| {
                    c.spawn((TableValue(id), NumberField::new(row_name.clone(), "").text(expr.clone()).label_width(0.0).build(&t)));
                    let (line, color) = match &error {
                        Some(why) => (why.clone(), t.feature_error),
                        None if expr.trim() != value => (format!("= {value}"), t.muted_foreground),
                        None => (String::new(), t.muted_foreground),
                    };
                    if !line.is_empty() {
                        c.spawn((Name::new(format!("{row_name}-note")), t.text(line, 10.5, FontWeight::NORMAL, color), Node { width: Val::Px(136.0), ..default() }))
                            .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
                    }
                });
            });
        }
    });
}

/// A value typed in the table: the Variable's new expression, one undo step.
fn on_table_value(ev: On<NumberFieldCommit>, q: Query<&TableValue>, mut commands: Commands) {
    let Ok(TableValue(id)) = q.get(ev.entity).copied() else {
        return;
    };
    let text = ev.text.trim().to_string();
    commands.queue(move |world: &mut World| {
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
        let Some(el) = doc.active_element() else { return };
        let element = el.id;
        // An assembly's Variable (the row's id is its mate feature's).
        if let Some(asm) = el.assembly_model() {
            let m = MateId(id.0);
            let Some(at) = asm.mates.iter().position(|f| f.id == m) else { return };
            let MateKind::Variable(v) = &asm.mates[at].kind else { return };
            let mut upto = asm.clone();
            upto.mates.truncate(at);
            let vars = cadrs_core::assembly::vars::defined(&upto, &units);
            let expr = match (text.parse::<f64>().is_ok(), v.var_type) {
                (true, VariableType::Length) => format!("{text} {}", units.length.symbol()),
                (true, VariableType::Angle) => format!("{text} deg"),
                _ => text.clone(),
            };
            let mut next = VariableFeature { expr, ..v.clone() };
            if next.evaluate(&units, &vars).is_err() {
                let e = table_field(world, id);
                if let Some(mut st) = e.and_then(|e| world.get_mut::<NumberFieldState>(e)) {
                    st.text = text;
                    st.error = true;
                }
                return;
            }
            if next != *v {
                let label = format!("Edit #{}", v.name);
                set_asm_variable(world, element, m, next, &label);
            }
            return;
        }
        let features = el.active_features();
        let Some(at) = features.iter().position(|f| f.id == id) else { return };
        let FeatureKind::Variable(v) = &features[at].kind else { return };
        let vars = variables::defined(&features[..at], &units);
        let expr = match (text.parse::<f64>().is_ok(), v.var_type) {
            (true, VariableType::Length) => format!("{text} {}", units.length.symbol()),
            (true, VariableType::Angle) => format!("{text} deg"),
            _ => text.clone(),
        };
        let mut next = VariableFeature { expr, ..v.clone() };
        if next.evaluate(&units, &vars).is_err() {
            // Red until a good value is typed.
            let e = table_field(world, id);
            if let Some(mut st) = e.and_then(|e| world.get_mut::<NumberFieldState>(e)) {
                st.text = text;
                st.error = true;
            }
            return;
        }
        if next == *v {
            return;
        }
        let label = format!("Edit #{}", v.name);
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
            let _ = doc.execute(&SetFeature { element, feature: id, kind: FeatureKind::Variable(next), label });
        }
    });
}

/// The table row field of a Variable.
fn table_field(world: &mut World, id: FeatureId) -> Option<Entity> {
    let mut q = world.query::<(Entity, &TableValue)>();
    q.iter(world).find(|(_, t)| t.0 == id).map(|(e, _)| e)
}
