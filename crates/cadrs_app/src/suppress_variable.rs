//! IR5.5: the feature menu's **Dynamic suppression ▸ Suppress by variable…**. A small dialog
//! picks one of the variables defined above the feature (the Variable features that build,
//! P3F.4) and whether the feature is suppressed while it is 0 (false, the default) or while it
//! isn't; OK sets it as one undo step ([`SetSuppressByVariable`]). From then on the feature is
//! suppressed or built as the variable's value says (in its Variable feature or the Variable
//! table), its row showing the variable in a tag. **Remove suppression variable** clears it
//! (one undo step too).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::FeatureId;
use cadrs_core::commands::SetSuppressByVariable;
use cadrs_core::variables::{self, SuppressByVariable};
use cadrs_sketch::units::{Quantity, Units, VarValue};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, Checkbox, CheckboxState, DialogClose, Select, SelectState, form_row};

use crate::{ActiveDocument, AppState};

/// The dialog's root: the feature it sets, and the variable names its select lists.
#[derive(Component)]
struct SuppressVariableDialog {
    feature: FeatureId,
    names: Vec<String>,
}

/// A variable's value as the picker shows it ("1", "40 mm", "30 deg").
fn value_text(v: VarValue, units: &Units) -> String {
    match v.quantity {
        Quantity::Length => format!("{} {}", units.value(v.value, Quantity::Length), units.length.symbol()),
        Quantity::Angle => format!("{} deg", units.value(v.value, Quantity::Angle)),
        Quantity::Count => {
            let s = format!("{:.6}", v.value);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    }
}

/// Opens the dialog for `feature`, set to its current suppression variable if it has one.
pub fn open(world: &mut World, feature: FeatureId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element() else { return };
    let Some(f) = el.feature(feature) else { return };
    let units = doc.doc.units;
    // The variables that build above it (a suppressed Variable defines nothing).
    let suppressed = el.all_suppressed();
    let building: Vec<cadrs_core::Feature> = el.features().iter().filter(|g| g.id == feature || !suppressed.contains(&g.id)).cloned().collect();
    let scope = variables::in_scope(&building, feature, &units);
    let current = f.suppress_by.clone();
    let feature_name = f.name.clone();
    let names: Vec<String> = scope.iter().map(|(n, _)| n.clone()).collect();
    let options: Vec<String> = scope.iter().map(|(n, v)| format!("#{n}  ({})", value_text(*v, &units))).collect();
    let selected = current
        .as_ref()
        .and_then(|r| cadrs_sketch::units::variable_names(&r.expr).into_iter().next())
        .and_then(|n| names.iter().position(|x| *x == n))
        .unwrap_or(0);
    let invert = current.as_ref().is_some_and(|r| r.invert);
    let empty = names.is_empty();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.commands().spawn((
        Dialog::new("suppress-variable-dialog")
            .title("Suppress by variable")
            .width(380.0)
            .body(move |b| {
                let t = &tb;
                if empty {
                    b.spawn((
                        Name::new("suppress-variable-none"),
                        t.text(format!("No variables are defined above {feature_name}."), t.font_base, FontWeight::NORMAL, t.muted_foreground),
                    ));
                    return;
                }
                let mut select = Select::new("suppress-variable-select").width(Val::Px(200.0));
                for o in &options {
                    select = select.option(o.clone(), true);
                }
                b.spawn(form_row(t, "suppress-variable-row", "Variable", 110.0)).with_child(select.selected(selected).build(t));
                b.spawn(Checkbox::new("suppress-variable-invert").label("Suppress when true (not 0)").checked(invert).build(t));
                b.spawn((
                    Name::new("suppress-variable-hint"),
                    t.text(format!("{feature_name} is suppressed while the variable is 0 (false)."), t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                    Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("suppress-variable-ok").label("OK").primary().disabled(empty).build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept);
                    }),
                ));
                f.spawn((
                    Button::new("suppress-variable-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<SuppressVariableDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        SuppressVariableDialog { feature, names },
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// OK: the picked variable becomes the feature's suppression variable (one undo step).
fn accept(world: &mut World) {
    let mut q = world.query::<(Entity, &SuppressVariableDialog)>();
    let Some((dialog, feature, names)) = q.iter(world).next().map(|(e, d)| (e, d.feature, d.names.clone())) else {
        return;
    };
    let (mut selected, mut invert) = (0, false);
    let mut qs = world.query::<(&Name, Option<&SelectState>, Option<&CheckboxState>)>();
    for (name, s, c) in qs.iter(world) {
        match (name.as_str(), s, c) {
            ("suppress-variable-select", Some(s), _) => selected = s.selected,
            ("suppress-variable-invert", _, Some(c)) => invert = c.checked,
            _ => {}
        }
    }
    let Some(name) = names.get(selected) else { return };
    let rule = SuppressByVariable { invert, ..SuppressByVariable::variable(name) };
    set(world, feature, Some(rule));
    world.trigger(DialogClose { entity: dialog });
}

/// Remove suppression variable: the feature builds again unless it is suppressed otherwise.
pub fn remove(world: &mut World, feature: FeatureId) {
    set(world, feature, None);
}

fn set(world: &mut World, feature: FeatureId, rule: Option<SuppressByVariable>) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active else { return };
    if let Err(e) = doc.execute(&SetSuppressByVariable { element, feature, rule }) {
        warn!("suppress by variable: {e}");
    }
}
