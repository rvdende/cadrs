//! The **Modify joint** dialog (P3I.3, SM6.4; `help/feature-tools/modify-joint-02.png`,
//! `sheetmetaljoint-dialog.png`): the feature the Sheet metal table makes, edited from the
//! feature list (double-click, or Edit):
//!
//! - **Joint**: the joint ("Bend A"); with the field active, clicking a bend or rip of the
//!   model picks another joint of it.
//! - The type: **Bend**, **Rip** or **Tangent**. A rip has its style (**Edge joint**, **Butt
//!   joint - Direction 1 / 2**, the butt joints for 90° joints only).
//! - A bend: **Use model bend radius** (off: **Bend radius**); **Use model K Factor** (off:
//!   **Bend calculation** — K Factor, Bend allowance, Bend deduction — and its value, red with
//!   its range when out of it).
//!
//! While it is open its joint shows selected in the model (all of its faces, in the selection
//! amber), as Onshape shows a dialog's picks.
//!
//! ✓ or Enter keeps it, ✕ or Esc reverts the edit. Every change is a command; accepting squashes
//! them into one undo step.
//!
//! Names: `modify-joint-dialog`, `smj-joint-field`, `smj-type`, `smj-rip-style`,
//! `smj-model-radius(-checkbox)`, `smj-radius`, `smj-model-k(-checkbox)`, `smj-calc`,
//! `smj-value`.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::commands::ReplaceFeature;
use cadrs_core::sheetmetal_joint::{JointType, ModifyJointFeature, value_expr};
use cadrs_core::{ElementId, Feature, FeatureId, FeatureKind};
use cadrs_sheetmetal::{BendCalc, RipStyle};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit, NumberFieldState, OptionRow, Select, SelectChange,
    SelectionList, SelectionListRemove, SelectionListState,
};

use crate::parts::PartCache;
use crate::viewport::{PickRequest, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ModifyJointUiPlugin;

impl Plugin for ModifyJointUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (joint_picks, joint_keys, sync_dialog).chain().before(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(Update, show_joint.after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |world: &mut World| {
                if world.contains_resource::<ModifyJointSession>() {
                    finish(world);
                }
            })
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_checkbox)
            .add_observer(on_select)
            .add_observer(on_number)
            .add_observer(on_remove);
    }
}

/// The Modify joint whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct ModifyJointSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub mark: usize,
    pub before: Option<Feature>,
}

#[derive(Component)]
struct JointDialog;

/// The layout the dialog was built for (it is rebuilt when it changes).
#[derive(Component, Debug, Clone, PartialEq)]
struct BuiltFor(String);

#[derive(Component)]
struct JointField;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Num {
    Radius,
    Value,
}

fn close_others(world: &mut World) {
    if world.contains_resource::<crate::applied::AppliedSession>() {
        crate::applied::finish(world);
    }
    if world.contains_resource::<crate::composite_ui::CompositeSession>() {
        crate::composite_ui::finish(world);
    }
    if world.contains_resource::<crate::boolean::BooleanSession>() {
        crate::boolean::finish(world);
    }
    if world.contains_resource::<crate::extrude::ExtrudeSession>() {
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
}

/// Opens a Modify joint for editing (a feature row's double-click or Edit).
pub fn edit(world: &mut World, feature: FeatureId) {
    if world.get_resource::<ModifyJointSession>().is_some_and(|s| s.feature == feature) {
        return;
    }
    if world.contains_resource::<ModifyJointSession>() {
        finish(world);
    }
    close_others(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element() else { return };
    let element = el.id;
    let Some(before) = el.feature(feature).filter(|f| matches!(f.kind, FeatureKind::ModifyJoint(_))).cloned() else { return };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    world.resource_mut::<Selection>().0.clear();
    world.insert_resource(ModifyJointSession { element, feature, mark, before: Some(before) });
}

fn end(world: &mut World) {
    world.remove_resource::<ModifyJointSession>();
    world.resource_mut::<Selection>().0.clear();
}

fn current(world: &World, s: &ModifyJointSession) -> Option<Feature> {
    world.get_resource::<ActiveDocument>()?.doc.element(s.element)?.feature(s.feature).cloned()
}

fn params(world: &World) -> Option<ModifyJointFeature> {
    let s = world.get_resource::<ModifyJointSession>()?;
    match current(world, s)?.kind {
        FeatureKind::ModifyJoint(x) => Some(x),
        _ => None,
    }
}

fn set(world: &mut World, x: ModifyJointFeature, label: &str) {
    let Some(s) = world.get_resource::<ModifyJointSession>().cloned() else { return };
    let Some(mut f) = current(world, &s) else { return };
    let kind = FeatureKind::ModifyJoint(x);
    if f.kind == kind {
        return;
    }
    f.kind = kind;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    if let Err(e) = doc.execute(&ReplaceFeature { element: s.element, feature: f, label: label.into() }) {
        warn!("cannot change the modify joint: {e}");
    }
}

fn update(world: &mut World, label: &str, f: impl FnOnce(&mut ModifyJointFeature)) {
    if let Some(mut x) = params(world) {
        f(&mut x);
        set(world, x, label);
    }
}

pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<ModifyJointSession>().cloned() else { return };
    let Some(f) = current(world, &s).filter(|f| f.is_valid()) else { return };
    let label = format!("Edit {}", f.name);
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, label);
    }
    end(world);
}

pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<ModifyJointSession>().cloned() else { return };
    let cur = current(world, &s);
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let name = cur.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
        if let (Some(before), Some(f)) = (s.before.clone(), cur)
            && f != before
        {
            let _ = doc.execute(&ReplaceFeature { element: s.element, feature: before, label: format!("Cancel {name}") });
        }
    }
    end(world);
}

pub fn finish(world: &mut World) {
    accept(world);
    if world.contains_resource::<ModifyJointSession>() {
        cancel(world);
    }
}

// ---------------------------------------------------------------------------------------------
// Input

/// A click on a bend or rip of the same model picks it as the joint.
fn joint_picks(mut picks: MessageReader<PickRequest>, session: Option<Res<ModifyJointSession>>, cache: Res<PartCache>, mut commands: Commands) {
    if session.is_none() {
        picks.clear();
        return;
    }
    for p in picks.read() {
        let Some(pick) = p.0 else { continue };
        let Some((model, joint)) = crate::sheetmetal_table::joint_of_pick(&cache, pick) else { continue };
        commands.queue(move |world: &mut World| {
            update(world, "Select joint", |x| {
                if x.model == model {
                    x.joint = Some(joint);
                }
            })
        });
    }
}

fn joint_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<ModifyJointSession>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    focus: Res<bevy::input_focus::InputFocus>,
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

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<JointDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<JointDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(ev.entity) else { return };
    let on = ev.checked;
    match n.as_str() {
        "smj-model-radius-checkbox" => commands.queue(move |world: &mut World| update(world, "Use model bend radius", |x| x.use_model_radius = on)),
        "smj-model-k-checkbox" => commands.queue(move |world: &mut World| update(world, "Use model K Factor", |x| x.use_model_value = on)),
        _ => {}
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(ev.entity) else { return };
    let i = ev.index;
    match n.as_str() {
        "smj-type" => commands.queue(move |world: &mut World| {
            if let Some(t) = JointType::ALL.get(i).copied() {
                update(world, "Joint type", |x| x.joint_type = t);
            }
        }),
        "smj-rip-style" => commands.queue(move |world: &mut World| {
            if let Some(s) = RipStyle::ALL.get(i).copied() {
                update(world, "Rip style", |x| x.rip_style = s);
            }
        }),
        "smj-calc" => commands.queue(move |world: &mut World| {
            if let Some(c) = BendCalc::ALL.get(i).copied() {
                update(world, "Bend calculation", |x| {
                    if x.calc != c {
                        // A new calculation starts from the model's default for it.
                        let p = cadrs_sheetmetal::Params::default();
                        x.calc = c;
                        x.value = match c {
                            BendCalc::KFactor => p.k_factor,
                            BendCalc::BendAllowance => p.bend_allowance,
                            BendCalc::BendDeduction => p.bend_deduction,
                        };
                        x.value_expr = value_expr(c, x.value);
                    }
                });
            }
        }),
        _ => {}
    }
}

fn on_number(ev: On<NumberFieldCommit>, q: Query<&Num>, mut commands: Commands) {
    let Ok(n) = q.get(ev.entity) else { return };
    let (n, text) = (*n, ev.text.clone());
    commands.queue(move |world: &mut World| {
        let Some(x) = params(world) else { return };
        let units = world.get_resource::<crate::WorkspaceUnits>().map(|u| u.0).unwrap_or_default();
        let vars = world.get_resource::<crate::variables_ui::ActiveVariables>().cloned().unwrap_or_default();
        let q = match n {
            Num::Radius => Quantity::Length,
            Num::Value if x.calc == BendCalc::KFactor => Quantity::Count,
            Num::Value => Quantity::Length,
        };
        let Some((v, expr)) = crate::sheetmetal_ui::parse(&text, q, &units, &vars) else { return };
        match n {
            Num::Radius => update(world, "Bend radius", |x| {
                x.radius = v;
                x.radius_expr = expr;
            }),
            Num::Value => update(world, x.calc.label(), |x| {
                x.value = v;
                x.value_expr = expr;
            }),
        }
    });
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<(), With<JointField>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| update(world, "Remove joint", |x| x.joint = None));
    }
}

/// The edited joint's faces show selected while the dialog is open
/// ([`crate::viewport::ExtraHighlight::selected`]).
fn show_joint(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<ModifyJointSession>>,
    cache: Res<PartCache>,
    mut extra: ResMut<crate::viewport::ExtraHighlight>,
    mut last: Local<Option<(cadrs_core::sheetmetal_joint::ModifyJointFeature, u64)>>,
) {
    let x = match (doc.as_deref(), session.as_deref()) {
        (Some(d), Some(s)) => d.doc.element(s.element).and_then(|e| e.feature(s.feature)).and_then(|f| match &f.kind {
            FeatureKind::ModifyJoint(x) => Some(x.clone()),
            _ => None,
        }),
        _ => None,
    };
    let Some(x) = x else {
        if last.take().is_some() && !extra.selected.is_empty() {
            extra.selected.clear();
        }
        return;
    };
    let key = (x.clone(), cache.generation);
    if last.as_ref() == Some(&key) {
        return;
    }
    let picks = match (x.joint, cache.sheet_metal.iter().find(|c| c.feature == x.model)) {
        (Some(j), Some(ctx)) => crate::sheetmetal_table::joint_picks(&cache, ctx, j),
        _ => Vec::new(),
    };
    if extra.selected != picks {
        extra.selected = picks;
    }
    *last = Some(key);
}

// ---------------------------------------------------------------------------------------------
// Dialog

fn labelled_select(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, options: &[(&str, bool)], selected: usize) {
    let mut s = Select::new(name.to_string());
    for (o, en) in options {
        s = s.option(*o, *en);
    }
    b.spawn(Node { height: Val::Px(28.0), margin: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(1.0), Val::Px(1.0)), align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() })
        .with_children(|r| {
            if !label.is_empty() {
                r.spawn((t.text(label, 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(100.0), flex_shrink: 0.0, ..default() }));
            }
            r.spawn(s.selected(selected).build(t)).entry::<Node>().and_modify(|mut n| n.flex_grow = 1.0);
        });
}

/// The joint's name in its model (from the last rebuild).
fn joint_name(cache: &PartCache, x: &ModifyJointFeature) -> Option<String> {
    let ctx = cache.sheet_metal.iter().find(|c| c.feature == x.model)?;
    Some(ctx.model.joint(x.joint?)?.name.clone())
}

/// Whether the joint is a 90° one (butt joints allowed).
fn ninety(cache: &PartCache, x: &ModifyJointFeature) -> bool {
    let Some(ctx) = cache.sheet_metal.iter().find(|c| c.feature == x.model) else { return true };
    let Some(j) = x.joint.and_then(|j| ctx.model.joint(j)) else { return true };
    let (Some(a), Some(b)) = (ctx.model.wall(j.a), ctx.model.wall(j.b)) else { return true };
    match (a.surface.normal(), b.surface.normal()) {
        (Some(na), Some(nb)) => na.dot(&nb).abs() < 1e-6,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_dialog(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<ModifyJointSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    units: Option<Res<crate::WorkspaceUnits>>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_dialog: Query<(Entity, &BuiltFor, &mut FeatureDialogState), With<JointDialog>>,
    mut q_list: Query<&mut SelectionListState, With<JointField>>,
    mut q_num: Query<(Entity, &Num, &mut NumberFieldState, Option<&Tooltip>)>,
    focus: Res<bevy::input_focus::InputFocus>,
    q_edit: Query<&cadrs_ui::NumberFieldEdit>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(f) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)) else { return };
    let FeatureKind::ModifyJoint(x) = &f.kind else { return };
    let units = units.map(|u| u.0).unwrap_or_default();
    let names: Vec<String> = joint_name(&cache, x).into_iter().collect();
    let error = cache.errors.get(&f.id).cloned();
    let valid = f.is_valid() && error.is_none() && !cache.rebuilding;
    let ninety = ninety(&cache, x);
    let layout = BuiltFor(format!("{:?}{}{}{:?}{}", x.joint_type, x.use_model_radius, x.use_model_value, x.calc, ninety));
    let text_of = |n: Num| match n {
        Num::Radius => units.with_unit(x.radius, Quantity::Length),
        Num::Value if x.calc == BendCalc::KFactor => cadrs_core::sheetmetal::plain(x.value),
        Num::Value => units.with_unit(x.value, Quantity::Length),
    };
    let built = q_dialog.iter().next().map(|(e, k, _)| (e, k.clone()));
    if built.as_ref().is_none_or(|(_, k)| *k != layout) {
        if let Some((e, _)) = built {
            commands.entity(e).try_despawn();
        }
        let Some(area) = q_area.iter().next() else { return };
        let t = theme.clone();
        let x = x.clone();
        let (radius, value) = (text_of(Num::Radius), text_of(Num::Value));
        let dialog = commands
            .spawn((
                JointDialog,
                layout,
                DespawnOnExit(AppState::Document),
                FeatureDialog::new("modify-joint-dialog")
                    .title(f.name.clone())
                    .valid(valid)
                    .body(move |b| {
                        b.spawn((JointField, SelectionList::new("smj-joint-field").placeholder("Joint").items(names).active(true).build(&t)));
                        let types: Vec<(&str, bool)> = JointType::ALL.iter().map(|j| (j.label(), true)).collect();
                        labelled_select(b, &t, "smj-type", "", &types, JointType::ALL.iter().position(|j| *j == x.joint_type).unwrap_or(0));
                        match x.joint_type {
                            JointType::Rip => {
                                let styles: Vec<(&str, bool)> = RipStyle::ALL.iter().map(|s| (s.label(), *s == RipStyle::EdgeJoint || ninety)).collect();
                                labelled_select(b, &t, "smj-rip-style", "", &styles, RipStyle::ALL.iter().position(|s| *s == x.rip_style).unwrap_or(0));
                            }
                            JointType::Bend => {
                                b.spawn(OptionRow::new("smj-model-radius", "Use model bend radius").checked(x.use_model_radius).build(&t));
                                if !x.use_model_radius {
                                    b.spawn((Num::Radius, NumberField::new("smj-radius", "Bend radius").text(radius.clone()).label_width(100.0).build(&t)));
                                }
                                b.spawn(OptionRow::new("smj-model-k", "Use model K Factor").checked(x.use_model_value).build(&t));
                                if !x.use_model_value {
                                    let calcs: Vec<(&str, bool)> = BendCalc::ALL.iter().map(|c| (c.label(), true)).collect();
                                    labelled_select(b, &t, "smj-calc", "Bend calculation", &calcs, BendCalc::ALL.iter().position(|c| *c == x.calc).unwrap_or(0));
                                    b.spawn((Num::Value, NumberField::new("smj-value", x.calc.label()).text(value.clone()).label_width(100.0).build(&t)));
                                }
                            }
                            JointType::Tangent => {}
                        }
                    })
                    .footer(|f| {
                        f.spawn(Node { flex_grow: 1.0, ..default() });
                        f.spawn((Name::new("modify-joint-dialog-help"), cadrs_ui::icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
                    })
                    .build(&theme),
            ))
            .id();
        commands.entity(area).add_child(dialog);
        return;
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState { title: f.name.clone(), valid, error: error.is_some() };
        if *st != want {
            *st = want;
        }
    }
    for mut l in &mut q_list {
        let want = SelectionListState { items: names.clone(), active: true, error: x.joint.is_none(), red_items: false, red: Vec::new() };
        if *l != want {
            *l = want;
        }
    }
    // Numbers: their text, red with the range when out of it.
    let editing = focus.get().and_then(|f| q_edit.get(f).ok()).map(|e| e.0);
    let range = x.range_error().map(|e| e.message());
    for (e, n, mut st, tip) in &mut q_num {
        let err = match n {
            Num::Radius => cadrs_core::sheetmetal_joint::radius_error(x.radius).map(|e| e.message()),
            Num::Value => cadrs_core::sheetmetal_joint::value_error(x.calc, x.value).map(|e| e.message()),
        };
        if editing != Some(e) {
            let want = NumberFieldState { text: text_of(*n), error: err.is_some() };
            if *st != want {
                *st = want;
            }
        }
        match (&err, tip) {
            (Some(m), Some(t)) if t.text == *m => {}
            (Some(m), _) => {
                commands.entity(e).insert(Tooltip::error(m.clone()));
            }
            (None, Some(_)) => {
                commands.entity(e).remove::<Tooltip>();
            }
            (None, None) => {}
        }
    }
    let _ = range;
}
