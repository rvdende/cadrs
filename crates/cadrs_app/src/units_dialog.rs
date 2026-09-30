//! The "Workspace units" dialog (X1), opened from the document menu (☰), like Onshape's:
//! the length unit (Millimeter, Centimeter, Meter, Inch, Foot, Yard), the mass unit
//! (Kilogram, Gram, Pound, Ounce; X8, P3.5) and the decimals
//! length and angle dimensions show. OK changes the document's units as one undoable step
//! ([`cadrs_core::commands::SetUnits`]); they are saved with the document. Values stay stored in
//! millimetres: dimension values, live labels, quick-dimension boxes and the area readout
//! display in the chosen unit, and a bare number typed into a value box is read in it.

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::commands::SetUnits;
use cadrs_sketch::units::{LengthUnit, MAX_DECIMALS, MassUnit, Units};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, DialogClose, Select, SelectState, form_row};

use crate::{ActiveDocument, AppState, WorkspaceUnits};

/// The units dialog's root.
#[derive(Component)]
struct UnitsDialog;

/// Keeps [`WorkspaceUnits`] in sync with the active document.
pub(crate) fn sync_units(doc: Option<Res<ActiveDocument>>, mut units: ResMut<WorkspaceUnits>) {
    let want = doc.map(|d| d.doc.units).unwrap_or_default();
    if units.0 != want {
        units.0 = want;
    }
}

/// How a decimals choice reads in the dialog ("0.123" for 3).
pub fn decimals_label(d: u8) -> String {
    if d == 0 {
        "0".into()
    } else {
        format!("0.{}", &"123456789"[..d as usize])
    }
}

/// Opens the dialog with the document's current units.
pub fn open_units_dialog(world: &mut World) {
    let Some(units) = world.get_resource::<ActiveDocument>().map(|d| d.doc.units) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("workspace-units-dialog")
            .title("Workspace units")
            .width(400.0)
            .body(move |b| {
                let t = &tb;
                let mut length = Select::new("units-length").width(Val::Px(180.0));
                for u in LengthUnit::ALL {
                    length = length.option(u.label(), true);
                }
                let selected = LengthUnit::ALL
                    .iter()
                    .position(|u| *u == units.length)
                    .unwrap_or(0);
                b.spawn(form_row(t, "units-length-row", "Length units", 150.0))
                    .with_child(length.selected(selected).build(t));
                // Angles are always degrees (shown for parity with Onshape's dialog).
                b.spawn(form_row(t, "units-angle-row", "Angle units", 150.0))
                    .with_child(
                        Select::new("units-angle")
                            .width(Val::Px(180.0))
                            .option("Degree", true)
                            .option("Radian", false)
                            .build(t),
                    );
                // Mass (X8, P3.5): the unit of Mass properties.
                let mut mass = Select::new("units-mass").width(Val::Px(180.0));
                for u in MassUnit::ALL {
                    mass = mass.option(u.label(), true);
                }
                let mass_selected = MassUnit::ALL.iter().position(|u| *u == units.mass).unwrap_or(0);
                b.spawn(form_row(t, "units-mass-row", "Mass units", 150.0))
                    .with_child(mass.selected(mass_selected).build(t));
                // The decimal lists open upward so they do not cover OK and Cancel.
                let decimals_select = |name: &'static str, selected: u8| {
                    let mut d = Select::new(name).width(Val::Px(180.0)).open_up();
                    for k in 0..=MAX_DECIMALS {
                        d = d.option(decimals_label(k), true);
                    }
                    d.selected(selected.min(MAX_DECIMALS) as usize)
                };
                b.spawn(form_row(t, "units-decimals-row", "Length decimal places", 150.0))
                    .with_child(decimals_select("units-decimals", units.decimals).build(t));
                b.spawn(form_row(t, "units-angle-decimals-row", "Angle decimal places", 150.0))
                    .with_child(
                        decimals_select("units-angle-decimals", units.angle_decimals).build(t),
                    );
                // Room under the rows, so the length list (opening down from the first row)
                // ends above OK and Cancel (T2 judge).
                b.spawn((
                    Name::new("units-dialog-spacer"),
                    Node {
                        height: Val::Px(40.0),
                        ..default()
                    },
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("units-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(save_units);
                    }),
                ));
                f.spawn((
                    Button::new("units-cancel").label("Cancel").build(t),
                    observe(
                        |_: On<Activate>,
                         q: Query<Entity, With<UnitsDialog>>,
                         mut commands: Commands| {
                            for e in &q {
                                commands.trigger(DialogClose { entity: e });
                            }
                        },
                    ),
                ));
            })
            .build(&theme),
        UnitsDialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// OK: the chosen units become the document's (one undo step).
fn save_units(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<UnitsDialog>>();
    let Some(dialog) = q.iter(world).next() else {
        return;
    };
    let mut qs = world.query::<(&Name, &SelectState)>();
    let mut length = None;
    let mut mass = None;
    let mut decimals = None;
    let mut angle_decimals = None;
    for (name, s) in qs.iter(world) {
        match name.as_str() {
            "units-length" => length = LengthUnit::ALL.get(s.selected).copied(),
            "units-mass" => mass = MassUnit::ALL.get(s.selected).copied(),
            "units-decimals" => decimals = Some(s.selected as u8),
            "units-angle-decimals" => angle_decimals = Some(s.selected as u8),
            _ => {}
        }
    }
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let current = doc.doc.units;
        let units = Units::new(
            length.unwrap_or(current.length),
            decimals.unwrap_or(current.decimals),
        )
        .with_angle_decimals(angle_decimals.unwrap_or(current.angle_decimals))
        .with_mass(mass.unwrap_or(current.mass));
        if let Err(e) = doc.execute(&SetUnits { units }) {
            warn!("cannot change the units: {e}");
        }
    }
    world.trigger(DialogClose { entity: dialog });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_read_like_onshape() {
        assert_eq!(decimals_label(0), "0");
        assert_eq!(decimals_label(3), "0.123");
        assert_eq!(decimals_label(6), "0.123456");
    }
}
