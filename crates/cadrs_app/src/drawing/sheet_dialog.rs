//! Sheet properties (D2.8, X5): opened from a right-click on empty sheet space ("Sheet
//! properties…") or from a sheet's context menu in the Sheets flyout ("Properties…"). It edits
//! only that sheet: its name, scale, size and orientation, border, zones and title block, and
//! the referenced part or assembly the title block reads. OK applies it as one undoable step.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementKind;
use cadrs_core::commands::EditDrawing;
use cadrs_drawing::{DrawingOp, ObjectRef, Orientation, Scale, SheetFormat, SheetId, SheetProps, SheetSize};
use cadrs_ui::prelude::*;
use cadrs_ui::Button;
use cadrs_ui::{CheckboxState, DialogClose, Select, SelectState, form_row};

use super::{DrawingUi, active_drawing};
use crate::{ActiveDocument, AppState};

pub struct SheetDialogPlugin;

impl Plugin for SheetDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_sheet_menu_action).add_observer(on_orientation_change);
    }
}

/// The dialog, for `sheet`; `refs[i]` is the reference option `i` stands for.
#[derive(Component, Clone)]
struct SheetPropsDialog {
    sheet: SheetId,
    refs: Vec<Option<ObjectRef>>,
    scales: Vec<Scale>,
}

/// The anchor of the sheet's right-click menu.
#[derive(Component)]
struct SheetSpaceMenu;

/// A right-click on the sheet: its menu.
pub fn open_sheet_menu(world: &mut World, pos: Vec2) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let menu = Menu::new("sheet-space-menu")
        .min_width(190.0)
        .item(MenuItem::new("sheet-space-properties", "Sheet properties…").icon("info"))
        .item(MenuItem::new("sheet-space-drawing-properties", "Drawing properties…").icon("tool"))
        .separator()
        .item(MenuItem::new("sheet-space-insert-view", "Insert view…").icon("part"))
        .item(MenuItem::new("sheet-space-fit", "Zoom to fit").shortcut("F").icon("find"));
    let anchor = open_context_menu(&mut commands, pos, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((SheetSpaceMenu, DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_sheet_menu_action(
    ev: On<MenuAction>,
    q: Query<(), With<SheetSpaceMenu>>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) {
        return;
    }
    match ev.item.as_str() {
        "sheet-space-properties" => commands.queue(|w: &mut World| {
            let sheet = w.get_resource::<ActiveDocument>().and_then(|doc| {
                let (id, d) = active_drawing(doc)?;
                let ui = w.resource::<DrawingUi>();
                d.sheets.get(ui.sheet_index(id, d)).map(|s| s.id)
            });
            if let Some(s) = sheet {
                open_sheet_properties(w, s);
            }
        }),
        "sheet-space-insert-view" => commands.queue(super::view_tools::open_insert_view),
        "sheet-space-drawing-properties" => commands.queue(|w: &mut World| {
            w.resource_mut::<DrawingUi>().props_open = true;
        }),
        "sheet-space-fit" => commands.queue(|w: &mut World| {
            let rect = *w.resource::<crate::viewport::ViewportRect>();
            let Some((sheet, size)) = w.get_resource::<ActiveDocument>().and_then(|doc| {
                let (id, d) = active_drawing(doc)?;
                let ui = w.resource::<DrawingUi>();
                d.sheets.get(ui.sheet_index(id, d)).map(|s| ((id, s.id), s.size_mm()))
            }) else {
                return;
            };
            let mut ui = w.resource_mut::<DrawingUi>();
            let area = super::sheet_area(&rect, &ui);
            ui.views.insert(sheet, super::fitted_view(size, area));
        }),
        _ => {}
    }
}

/// Opens Sheet properties for `sheet` of the active drawing.
pub fn open_sheet_properties(world: &mut World, sheet: SheetId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some((_, d)) = active_drawing(doc) else {
        return;
    };
    let Some(s) = d.sheet(sheet).cloned() else {
        return;
    };
    // Reference choices: none, then every Part Studio and Assembly of the document.
    let mut refs: Vec<Option<ObjectRef>> = vec![None];
    let mut ref_labels = vec!["None".to_string()];
    for el in &doc.doc.elements {
        if matches!(el.kind, ElementKind::PartStudio { .. } | ElementKind::Assembly) {
            refs.push(Some(ObjectRef {
                element: el.id.0,
                part: None,
            }));
            ref_labels.push(el.name.clone());
        }
    }
    let ref_selected = match s.reference {
        None => 0,
        Some(r) => refs
            .iter()
            .position(|x| x.map(|x| x.element) == Some(r.element))
            .unwrap_or_else(|| {
                refs.push(Some(r));
                ref_labels.push(super::reference_props(&doc.doc, Some(r)).name.unwrap_or_default());
                refs.len() - 1
            }),
    };
    let mut scales: Vec<Scale> = Scale::COMMON.to_vec();
    if !scales.contains(&s.scale) {
        scales.push(s.scale);
    }
    let scale_selected = scales.iter().position(|x| *x == s.scale).unwrap_or(0);
    let size_selected = SheetSize::ALL.iter().position(|x| *x == s.format.size).unwrap_or(0);
    let orient_selected = Orientation::ALL
        .iter()
        .position(|x| *x == s.format.orientation)
        .unwrap_or(0);
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let dialog = SheetPropsDialog {
        sheet,
        refs,
        scales: scales.clone(),
    };
    let name = s.name.clone();
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("sheet-properties-dialog")
            .title(format!("Sheet properties: {name}"))
            .width(440.0)
            .body(move |b| {
                let t = &tb;
                let lw = 150.0;
                b.spawn(form_row(t, "sheet-props-name-row", "Name", lw)).with_child(
                    TextInput::new("sheet-props-name").value(name).width(Val::Px(220.0)).build(t),
                );
                let mut sc = Select::new("sheet-props-scale").width(Val::Px(220.0));
                for x in &scales {
                    sc = sc.option(x.label(), true);
                }
                b.spawn(form_row(t, "sheet-props-scale-row", "Scale", lw))
                    .with_child(sc.selected(scale_selected).build(t));
                let mut sz = Select::new("sheet-props-size").width(Val::Px(220.0));
                for x in SheetSize::ALL {
                    sz = sz.option(x.oriented_label(s.format.orientation), true);
                }
                b.spawn(form_row(t, "sheet-props-size-row", "Size", lw))
                    .with_child(sz.selected(size_selected).build(t));
                let mut or = Select::new("sheet-props-orientation").width(Val::Px(220.0));
                for x in Orientation::ALL {
                    or = or.option(x.label(), true);
                }
                b.spawn(form_row(t, "sheet-props-orientation-row", "Orientation", lw))
                    .with_child(or.selected(orient_selected).build(t));
                b.spawn((
                    t.text("Border and zones", t.font_base, FontWeight::SEMIBOLD, t.foreground),
                    Node {
                        margin: UiRect::top(Val::Px(6.0)),
                        ..default()
                    },
                ));
                b.spawn(Checkbox::new("sheet-props-border").label("Show border").checked(s.border).build(t));
                b.spawn(Checkbox::new("sheet-props-zones").label("Show zones").checked(s.zones).build(t));
                b.spawn(
                    Checkbox::new("sheet-props-title-block")
                        .label("Show title block")
                        .checked(s.title_block)
                        .build(t),
                );
                let mut rf = Select::new("sheet-props-reference").width(Val::Px(220.0)).open_up();
                for l in ref_labels {
                    rf = rf.option(l, true);
                }
                b.spawn(form_row(t, "sheet-props-reference-row", "Referenced object", lw))
                    .with_child(rf.selected(ref_selected).build(t));
                b.spawn((
                    t.text(
                        "The title block and parametric notes read this part's or assembly's properties.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node {
                        max_width: Val::Px(400.0),
                        ..default()
                    },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("sheet-props-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_sheet_properties);
                    }),
                ));
                f.spawn((
                    Button::new("sheet-props-cancel").label("Cancel").build(t),
                    observe(
                        |_: On<Activate>, q: Query<Entity, With<SheetPropsDialog>>, mut commands: Commands| {
                            for e in &q {
                                commands.trigger(DialogClose { entity: e });
                            }
                        },
                    ),
                ));
            })
            .build(&theme),
        dialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// The Size list shows width × height the way the chosen orientation turns the sheet.
fn on_orientation_change(ev: On<cadrs_ui::SelectChange>, q: Query<&Name>, mut q_sel: Query<(&Name, &mut SelectState)>) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "sheet-props-orientation") {
        return;
    }
    let orientation = Orientation::ALL[ev.index.min(1)];
    for (n, mut state) in &mut q_sel {
        if n.as_str() == "sheet-props-size" {
            for (i, size) in SheetSize::ALL.iter().enumerate() {
                if let Some(o) = state.options.get_mut(i) {
                    o.0 = size.oriented_label(orientation);
                }
            }
        }
    }
}

fn apply_sheet_properties(world: &mut World) {
    let mut q = world.query::<(Entity, &SheetPropsDialog)>();
    let Some((entity, dialog)) = q.iter(world).next().map(|(e, d)| (e, d.clone())) else {
        return;
    };
    let mut selects = std::collections::HashMap::new();
    let mut qs = world.query::<(&Name, &SelectState)>();
    for (n, s) in qs.iter(world) {
        selects.insert(n.as_str().to_string(), s.selected);
    }
    let mut checks = std::collections::HashMap::new();
    let mut qc = world.query::<(&Name, &CheckboxState)>();
    for (n, c) in qc.iter(world) {
        checks.insert(n.as_str().to_string(), c.checked);
    }
    let mut qt = world.query::<(&Name, &bevy::text::EditableText)>();
    let new_name = qt
        .iter(world)
        .find(|(n, _)| n.as_str() == "sheet-props-name-field")
        .map(|(_, t)| t.value().to_string());
    let Some((element, sheet)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        Some((id, d.sheet(dialog.sheet)?.clone()))
    }) else {
        return;
    };
    let get = |k: &str| selects.get(k).copied();
    let props = SheetProps {
        format: SheetFormat::new(
            get("sheet-props-size")
                .and_then(|i| SheetSize::ALL.get(i).copied())
                .unwrap_or(sheet.format.size),
            get("sheet-props-orientation")
                .and_then(|i| Orientation::ALL.get(i).copied())
                .unwrap_or(sheet.format.orientation),
        ),
        scale: get("sheet-props-scale")
            .and_then(|i| dialog.scales.get(i).copied())
            .unwrap_or(sheet.scale),
        border: checks.get("sheet-props-border").copied().unwrap_or(sheet.border),
        zones: checks.get("sheet-props-zones").copied().unwrap_or(sheet.zones),
        title_block: checks
            .get("sheet-props-title-block")
            .copied()
            .unwrap_or(sheet.title_block),
        reference: get("sheet-props-reference")
            .and_then(|i| dialog.refs.get(i).copied())
            .unwrap_or(sheet.reference),
    };
    let format_changed = props.format != sheet.format;
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        if let Some(n) = new_name
            && n.trim() != sheet.name
            && !n.trim().is_empty()
        {
            let _ = doc.execute(&EditDrawing {
                element,
                op: DrawingOp::RenameSheet { id: sheet.id, name: n },
            });
        }
        if let Err(e) = doc.execute(&EditDrawing {
            element,
            op: DrawingOp::SetSheetProps { id: sheet.id, props },
        }) {
            warn!("cannot change the sheet: {e}");
        }
    }
    if format_changed {
        // A new size: fit it again.
        world.resource_mut::<DrawingUi>().views.remove(&(element, sheet.id));
    }
    world.trigger(DialogClose { entity });
}
