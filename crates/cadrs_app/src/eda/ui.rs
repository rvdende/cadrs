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
/// `label`. Errors from `f` (and from the command) come back as a toast. Zones are poured
/// again whenever the board changed (in the same step): they are always shown filled.
pub fn commit(
    world: &mut World,
    label: &str,
    f: impl FnOnce(&mut Design) -> Result<(), String>,
) -> bool {
    let Some((element, board, mut d)) = current(world) else {
        return false;
    };
    let unfilled = |b: &cadrs_eda::board::Board| {
        let mut b = b.clone();
        cadrs_eda::zone::unfill_all(&mut b);
        b
    };
    let before = (!d.board.zones.is_empty()).then(|| unfilled(&d.board));
    if let Err(e) = f(&mut d) {
        toast(world, &e);
        return false;
    }
    let changed = match &before {
        Some(b) => *b != unfilled(&d.board),
        None => !d.board.zones.is_empty(),
    };
    if changed && d.board.zones.iter().any(|z| z.keepout.is_none()) {
        // A zone drawn on no net takes the net of what is now wired to it.
        cadrs_eda::zone::adopt_nets(&mut d.board);
        cadrs_eda::zone::fill_all(&mut d.board);
    }
    let cmd = SetDesign {
        element,
        board,
        design: Box::new(d),
        label: label.into(),
    };
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
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

/// Sets the text of the text input named `name`.
pub fn set_text_value(world: &mut World, name: &str, value: &str) {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == field) {
        t.editor_mut().set_text(value);
    }
}

/// The libraries a board in this studio sees: the built-in ones, the user's
/// ([`super::libraries`]), and the studio's components as the project library named after the
/// studio.
pub fn libraries(world: &World) -> LibraryTable {
    let mut t = shared_libraries(world);
    let Some((el, _, _)) = world.resource::<Eda2d>().0 else {
        return t;
    };
    let doc = world.resource::<ActiveDocument>();
    let Some(e) = doc.doc.element(el) else {
        return t;
    };
    let Some(s) = e.pcb() else { return t };
    let mut lib = Library::new(project_library_name(&e.name), Scope::Project);
    for c in &s.components {
        if let Some(sym) = &c.component.symbol {
            let mut sym = sym.clone();
            // A symbol without a default footprint takes its component's.
            if let Some(fp) = &c.component.footprint
                && sym
                    .field(cadrs_eda::symbol::fields::FOOTPRINT)
                    .is_none_or(|f| f.value().is_empty())
            {
                cadrs_eda::lib_edit::set_symbol_field(
                    &mut sym,
                    cadrs_eda::symbol::fields::FOOTPRINT,
                    &format!("{}:{}", lib.name, fp.name()),
                );
            }
            lib.put_symbol(sym);
        }
        if let Some(fp) = &c.component.footprint {
            lib.put_footprint(fp.clone());
        }
    }
    t.add(lib);
    // The footprints the shown board holds that no library has (a design brought in from
    // KiCad with libraries cadrs doesn't ship).
    if let Some((_, _, d)) = current(world) {
        t.add_design_footprints(&d);
    }
    t
}

/// The libraries every studio sees alike: the built-in ones and the user's.
pub fn shared_libraries(world: &World) -> LibraryTable {
    let mut t = LibraryTable::builtin();
    if let Some(u) = world.get_resource::<super::libraries::UserLibraries>() {
        u.add_to(&mut t);
    }
    t
}

/// A library id and what it becomes.
type Renamed = (String, String);

/// Gives a copy of schematic items the parts its symbols use that only the shown studio has
/// (none of the built-in or user libraries does): the studio's components of those symbols or
/// footprints (by name), and footprints only its board holds (as components of their own). The
/// copy then names them all in the studio's project library.
pub fn carry_parts(world: &World, clip: &mut cadrs_eda::sch_edit::SchClip) {
    let Some((el, _, _)) = world.resource::<Eda2d>().0 else {
        return;
    };
    let doc = world.resource::<ActiveDocument>();
    let Some(e) = doc.doc.element(el) else { return };
    let Some(s) = e.pcb() else { return };
    let lib = project_library_name(&e.name);
    let (shared, here) = (shared_libraries(world), libraries(world));
    let name_of = |id: &str| id.rsplit_once(':').map_or(id, |(_, n)| n).to_string();
    let mut parts: Vec<cadrs_eda::Component> = vec![];
    // (old id, new id) of symbols and of footprints.
    let (mut syms, mut fps): (Vec<Renamed>, Vec<Renamed>) = (vec![], vec![]);
    let mut take = |c: &cadrs_eda::Component| {
        if !parts.iter().any(|p| p.name == c.name) {
            parts.push(c.clone());
        }
    };
    let used: Vec<(String, String)> = clip
        .symbols
        .iter()
        .map(|p| {
            (
                p.symbol.clone(),
                p.field(cadrs_eda::symbol::fields::FOOTPRINT)
                    .map(|f| f.value().to_string())
                    .unwrap_or_default(),
            )
        })
        .chain(clip.definitions.iter().map(|d| {
            (
                d.id.clone(),
                d.field(cadrs_eda::symbol::fields::FOOTPRINT)
                    .map(|f| f.value().to_string())
                    .unwrap_or_default(),
            )
        }))
        .collect();
    for (sym, fp) in used {
        if shared.symbol(&sym).is_none() {
            let n = name_of(&sym);
            if let Some(c) = s
                .components
                .iter()
                .find(|c| c.component.symbol.as_ref().is_some_and(|x| x.name() == n))
            {
                take(&c.component);
                syms.push((sym.clone(), format!("{lib}:{n}")));
            }
        }
        if fp.is_empty() || shared.footprint(&fp).is_some() {
            continue;
        }
        let n = name_of(&fp);
        if let Some(c) = s.components.iter().find(|c| {
            c.component
                .footprint
                .as_ref()
                .is_some_and(|x| x.name() == n)
        }) {
            take(&c.component);
        } else if let Some(f) = here.footprint(&fp) {
            // A footprint only this board holds: a component of its own.
            let mut f = f.clone();
            f.id = n.clone();
            take(&cadrs_eda::Component {
                name: n.clone(),
                symbol: None,
                footprint: Some(f),
            });
        } else {
            continue;
        }
        fps.push((fp.clone(), format!("{lib}:{n}")));
    }
    let map = |m: &[(String, String)], id: &str| {
        m.iter()
            .find(|(o, n)| o == id && o != n)
            .map(|(_, n)| n.clone())
    };
    clip.rename_parts(&|id| map(&syms, id), &|id| map(&fps, id));
    clip.parts = parts;
    clip.library = lib;
}

/// Readies a copy for pasting into the shown studio: its parts named in this studio's project
/// library, and the ones the studio lacks added to its Components (each an undo step; the
/// caller folds them into the paste's). Returns the copy as it pastes here and the history
/// length before any part was added.
pub fn adopt_parts(
    world: &mut World,
    clip: &cadrs_eda::sch_edit::SchClip,
) -> (cadrs_eda::sch_edit::SchClip, usize) {
    let mark = world.resource::<ActiveDocument>().history.undo_len();
    let mut clip = clip.clone();
    let Some((el, _, _)) = world.resource::<Eda2d>().0 else {
        return (clip, mark);
    };
    let Some(e) = world.resource::<ActiveDocument>().doc.element(el) else {
        return (clip, mark);
    };
    let Some(s) = e.pcb() else {
        return (clip, mark);
    };
    let lib = project_library_name(&e.name);
    let missing: Vec<cadrs_eda::Component> = clip
        .parts
        .iter()
        .filter(|p| !s.components.iter().any(|c| c.component.name == p.name))
        .cloned()
        .collect();
    if !clip.library.is_empty() {
        let from = clip.library.clone();
        let map = |id: &str| cadrs_eda::rename::reprefix(id, &from, &lib);
        clip.rename_parts(&map, &map);
    }
    for mut p in missing {
        // Its symbol's default footprint: its own, as this studio's library has it.
        if let (Some(sym), Some(fp)) = (p.symbol.as_mut(), &p.footprint) {
            cadrs_eda::lib_edit::set_symbol_field(
                sym,
                cadrs_eda::symbol::fields::FOOTPRINT,
                &format!("{lib}:{}", fp.name()),
            );
        }
        let cmd = cadrs_core::pcb::AddComponent {
            element: el,
            name: None,
            value: Some(Box::new(p)),
        };
        if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&cmd) {
            warn!("paste: add component: {e}");
        }
    }
    (clip, mark)
}

/// Adds a library part placed on a schematic to the studio's Components (its symbol, and its
/// default footprint when the symbol names one), unless it is one already: a power port, one
/// of the studio's own components, or a part of the same name. Returns whether it added one.
pub fn add_library_component(
    world: &mut World,
    element: ElementId,
    sym: &cadrs_eda::symbol::Symbol,
) -> bool {
    if sym.power {
        return false;
    }
    let lib = libraries(world);
    let doc = world.resource::<ActiveDocument>();
    let Some(e) = doc.doc.element(element) else {
        return false;
    };
    let Some(s) = e.pcb() else { return false };
    let own = format!("{}:", project_library_name(&e.name));
    if sym.id.starts_with(&own)
        || s.components.iter().any(|c| {
            c.component.name == sym.name()
                || c.component
                    .symbol
                    .as_ref()
                    .is_some_and(|x| x.name() == sym.name())
        })
    {
        return false;
    }
    let footprint = sym
        .field(cadrs_eda::symbol::fields::FOOTPRINT)
        .and_then(|f| lib.footprint(f.value()))
        .cloned();
    let value = cadrs_eda::Component {
        name: sym.name().to_string(),
        symbol: Some(sym.clone()),
        footprint,
    };
    let cmd = cadrs_core::pcb::AddComponent {
        element,
        name: None,
        value: Some(Box::new(value)),
    };
    match world.resource_mut::<ActiveDocument>().execute(&cmd) {
        Ok(_) => true,
        Err(e) => {
            warn!("add component: {e}");
            false
        }
    }
}

/// A studio's name as a library name: "PCB Studio 1" → "PCB_Studio_1".
pub fn project_library_name(studio: &str) -> String {
    cadrs_core::pcb::project_library_name(studio)
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

/// While a tool strip is on the view's right edge, the panel strip (`crate::document`) sits at
/// the bottom of that edge, above the view tools, so the two make one column.
fn lower_panel_strip(world: &mut World) {
    let mut q = world.query::<&ToolStrip>();
    let low = q.iter(world).next().is_some();
    let mut q = world.query::<(&Name, &mut Node)>();
    for (n, mut node) in q.iter_mut(world) {
        if n.as_str() != "right-panel-strip" {
            continue;
        }
        let (top, bottom, margin) = if low {
            (Val::Auto, Val::Px(52.0), Val::ZERO)
        } else {
            (Val::Percent(50.0), Val::Auto, Val::Px(-60.0))
        };
        if node.top != top || node.bottom != bottom || node.margin.top != margin {
            node.top = top;
            node.bottom = bottom;
            node.margin.top = margin;
        }
    }
}

/// Keeps the right-edge tool strip of the shown mode on screen.
pub fn sync_strip(
    world: &mut World,
    q: &mut QueryState<(Entity, &'static ToolStrip)>,
    mode: Mode,
    spec: StripSpec,
) {
    let show = world
        .resource::<Eda2d>()
        .0
        .is_some_and(|(_, _, m)| m == mode);
    let existing = q.iter(world).find(|(_, s)| s.0 == mode).map(|(e, _)| e);
    match (show, existing) {
        (true, None) => {
            let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
            let Some(area) = qa.iter(world).next() else {
                return;
            };
            let t = world.resource::<Theme>().clone();
            // On a board in a Part Studio: under the view cube.
            let top = if world.resource::<Eda2d>().plane().is_some() {
                232.0
            } else {
                48.0
            };
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
                        top: Val::Px(top),
                        // The view's right edge, the panel strip's column (moved down meanwhile,
                        // [`lower_panel_strip`]).
                        right: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        padding: UiRect::all(Val::Px(3.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
                        ..default()
                    },
                    cadrs_ui::glass::panel(&t),
                ))
                .with_children(|s| {
                    for item in spec.iter() {
                        match item {
                            Some((name, icon, tip, action)) => {
                                s.spawn((
                                    ToolButton::new(*name, *icon)
                                        .icon_size(18.0)
                                        .tooltip(*tip)
                                        .build(&t),
                                    StripButton { mode, action },
                                ));
                            }
                            None => {
                                s.spawn((
                                    Node {
                                        height: Val::Px(1.0),
                                        margin: UiRect::vertical(Val::Px(3.0)),
                                        ..default()
                                    },
                                    BackgroundColor(t.separator),
                                ));
                            }
                        }
                    }
                });
            });
            world.flush();
            lower_panel_strip(world);
        }
        (false, Some(e)) => {
            world.commands().entity(e).despawn();
            world.flush();
            lower_panel_strip(world);
        }
        _ => {}
    }
}

/// A strip button was clicked: (mode, action).
#[derive(Message, Clone, Copy, Debug)]
pub struct StripAction(pub Mode, pub &'static str);

pub fn on_strip_button(
    a: On<Activate>,
    q: Query<&StripButton>,
    mut out: MessageWriter<StripAction>,
) {
    if let Ok(b) = q.get(a.entity) {
        out.write(StripAction(b.mode, b.action));
    }
}

/// Highlights the strip button of the active tool.
pub fn mark_active(world: &mut World, mode: Mode, active: &str) {
    // (A strip not shown is not built.)
    if !world
        .resource::<Eda2d>()
        .0
        .is_some_and(|(_, _, m)| m == mode)
    {
        return;
    }
    let mut q = world.query::<(Entity, &StripButton, Has<cadrs_ui::style::Selected>)>();
    let changes: Vec<(Entity, bool)> = q
        .iter(world)
        .filter(|(_, b, _)| b.mode == mode)
        .filter_map(|(e, b, sel)| ((b.action == active) != sel).then_some((e, b.action == active)))
        .collect();
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
    let shown = world
        .resource::<Eda2d>()
        .0
        .is_some_and(|(_, _, m)| m == mode);
    // Typing: a text field has the focus (a closed dialog's button may leave a stale focus).
    let focus = world.resource::<bevy::input_focus::InputFocus>().get();
    if !shown {
        return false;
    }
    let typing = focus.is_some_and(|e| world.get::<cadrs_ui::input::TextInputField>(e).is_some());
    let mut qd = world.query_filtered::<(), With<cadrs_ui::DialogRoot>>();
    !typing && qd.iter(world).next().is_none()
}

/// Sets the text of the text node named `name`.
pub fn set_label(world: &mut World, name: &str, value: &str) {
    let mut q = world.query::<(&Name, &mut Text)>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == name)
        && t.0 != value
    {
        t.0 = value.to_string();
    }
}

/// The name of the studio the shown view belongs to.
pub fn studio_name(world: &World) -> String {
    let Some((el, _, _)) = world.resource::<Eda2d>().0 else {
        return String::new();
    };
    world
        .resource::<ActiveDocument>()
        .doc
        .element(el)
        .map(|e| e.name.clone())
        .unwrap_or_default()
}
