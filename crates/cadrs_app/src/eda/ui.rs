//! Pieces the Schematic and Layout tools share: committing an edit (one undo step), reading
//! dialog fields, the libraries a board sees, and the tool strip at the view's right edge.

use std::borrow::Cow;

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use cadrs_core::ElementId;
use cadrs_core::pcb::{BoardId, SetDesign};
use cadrs_eda::Design;
use cadrs_eda::library::{Library, LibraryTable, Scope};
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;

use super::{Eda2d, Mode};
use crate::ActiveDocument;
use crate::viewport::ViewportArea;

/// The shown board's design (a copy to edit).
pub fn current(world: &World) -> Option<(ElementId, BoardId, Design)> {
    let (el, b, _) = world.resource::<Eda2d>().board()?;
    let mut d = super::design(world.resource::<ActiveDocument>(), el, b)?.clone();
    d.ensure_sheet();
    Some((el, b, d))
}

/// Applies `f` to the shown board's design and stores the result as one undo step named
/// `label`. Errors from `f` (and from the command) come back as a toast.
pub fn commit(world: &mut World, label: &str, f: impl FnOnce(&mut Design) -> Result<(), String>) -> bool {
    let Some((element, board, mut d)) = current(world) else { return false };
    if let Err(e) = f(&mut d) {
        toast(world, &e);
        return false;
    }
    let cmd = SetDesign { element, board, design: Box::new(d), label: label.into() };
    let r = world.resource_mut::<ActiveDocument>().execute(&cmd);
    if let Err(e) = r {
        toast(world, &e.to_string());
        return false;
    }
    true
}

pub fn toast(world: &mut World, msg: &str) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::show_toast_for(&mut commands, &theme, msg.to_string(), 3.0);
    world.flush();
}

/// The text of a dialog's text field (`TextInput::new(name)` makes `<name>-field`).
pub fn text_value(world: &mut World, name: &str) -> String {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world).find(|(n, _)| n.as_str() == field).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

/// Sets the text of the text input named `name`.
pub fn set_text_value(world: &mut World, name: &str, value: &str) {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == field) {
        t.editor_mut().set_text(value);
    }
}

/// The libraries a board in this studio sees: the built-in ones, and the studio's components
/// as the project library named after the studio.
pub fn libraries(world: &World) -> LibraryTable {
    let mut t = LibraryTable::builtin();
    let Some((el, _, _)) = world.resource::<Eda2d>().0 else { return t };
    let doc = world.resource::<ActiveDocument>();
    let Some(e) = doc.doc.element(el) else { return t };
    let Some(s) = e.pcb() else { return t };
    let mut lib = Library::new(project_library_name(&e.name), Scope::Project);
    for c in &s.components {
        if let Some(sym) = &c.component.symbol {
            let mut sym = sym.clone();
            // A symbol without a default footprint takes its component's.
            if let Some(fp) = &c.component.footprint
                && sym.field(cadrs_eda::symbol::fields::FOOTPRINT).is_none_or(|f| f.value().is_empty())
            {
                cadrs_eda::lib_edit::set_symbol_field(&mut sym, cadrs_eda::symbol::fields::FOOTPRINT, &format!("{}:{}", lib.name, fp.name()));
            }
            lib.put_symbol(sym);
        }
        if let Some(fp) = &c.component.footprint {
            lib.put_footprint(fp.clone());
        }
    }
    t.add(lib);
    t
}

/// A studio's name as a library name: "PCB Studio 1" → "PCB_Studio_1".
pub fn project_library_name(studio: &str) -> String {
    studio.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

// ---------------------------------------------------------------------------------------------
// The tool strip

/// A tool strip's button: the mode it belongs to and the action it triggers.
#[derive(Component, Clone)]
pub struct StripButton {
    pub mode: Mode,
    pub action: &'static str,
}

#[derive(Component)]
pub struct ToolStrip(pub Mode);

/// (name, icon, tooltip, action) of each button, `None` for a separator.
pub type StripSpec = &'static [Option<(&'static str, &'static str, &'static str, &'static str)>];

/// Keeps the right-edge tool strip of the shown mode on screen.
pub fn sync_strip(world: &mut World, mode: Mode, spec: StripSpec) {
    let show = world.resource::<Eda2d>().0.is_some_and(|(_, _, m)| m == mode);
    let mut q = world.query::<(Entity, &ToolStrip)>();
    let existing = q.iter(world).find(|(_, s)| s.0 == mode).map(|(e, _)| e);
    match (show, existing) {
        (true, None) => {
            let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
            let Some(area) = qa.iter(world).next() else { return };
            let t = world.resource::<Theme>().clone();
            let name: Cow<'static, str> = match mode {
                Mode::Schematic => "eda-schematic-tools".into(),
                _ => "eda-layout-tools".into(),
            };
            world.commands().entity(area).with_children(|vp| {
                vp.spawn((
                    Name::new(name),
                    ToolStrip(mode),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(48.0),
                        right: Val::Px(44.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        padding: UiRect::all(Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(t.background),
                    BoxShadow::default(),
                ))
                .with_children(|s| {
                    for item in spec.iter() {
                        match item {
                            Some((name, icon, tip, action)) => {
                                s.spawn((ToolButton::new(*name, *icon).icon_size(18.0).tooltip(*tip).build(&t), StripButton { mode, action }));
                            }
                            None => {
                                s.spawn((Node { height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(3.0)), ..default() }, BackgroundColor(t.separator)));
                            }
                        }
                    }
                });
            });
            world.flush();
        }
        (false, Some(e)) => {
            world.commands().entity(e).despawn();
            world.flush();
        }
        _ => {}
    }
}

/// A strip button was clicked: (mode, action).
#[derive(Message, Clone, Copy, Debug)]
pub struct StripAction(pub Mode, pub &'static str);

pub fn on_strip_button(a: On<Activate>, q: Query<&StripButton>, mut out: MessageWriter<StripAction>) {
    if let Ok(b) = q.get(a.entity) {
        out.write(StripAction(b.mode, b.action));
    }
}

/// Highlights the strip button of the active tool.
pub fn mark_active(world: &mut World, mode: Mode, active: &str) {
    let mut q = world.query::<(Entity, &StripButton, Has<cadrs_ui::style::Selected>)>();
    let changes: Vec<(Entity, bool)> = q.iter(world).filter(|(_, b, _)| b.mode == mode).filter_map(|(e, b, sel)| ((b.action == active) != sel).then_some((e, b.action == active))).collect();
    for (e, on) in changes {
        if on {
            world.entity_mut(e).insert(cadrs_ui::style::Selected);
        } else {
            world.entity_mut(e).remove::<cadrs_ui::style::Selected>();
        }
    }
}

/// Whether keys should go to the canvas: a 2D view of `mode` shown, nothing typed into, no
/// dialog open.
pub fn keys_for(world: &mut World, mode: Mode) -> bool {
    let shown = world.resource::<Eda2d>().0.is_some_and(|(_, _, m)| m == mode);
    // Typing: a text field has the focus (a closed dialog's button may leave a stale focus).
    let focus = world.resource::<bevy::input_focus::InputFocus>().get();
    let typing = focus.is_some_and(|e| world.get::<cadrs_ui::input::TextInputField>(e).is_some());
    let mut qd = world.query_filtered::<(), With<cadrs_ui::DialogRoot>>();
    shown && !typing && qd.iter(world).next().is_none()
}

/// Sets the text of the text node named `name`.
pub fn set_label(world: &mut World, name: &str, value: &str) {
    let mut q = world.query::<(&Name, &mut Text)>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == name) {
        t.0 = value.to_string();
    }
}

/// The name of the studio the shown view belongs to.
pub fn studio_name(world: &World) -> String {
    let Some((el, _, _)) = world.resource::<Eda2d>().0 else { return String::new() };
    world.resource::<ActiveDocument>().doc.element(el).map(|e| e.name.clone()).unwrap_or_default()
}
