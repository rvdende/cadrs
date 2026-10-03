//! Parameter rows for feature dialogs, modeled on gpui-component's `Select`, `NumberInput` and
//! `Slider`, styled like Onshape's Extrude dialog (`reference/onshape/screens/22`):
//!
//! - [`Select`]: a dropdown with the value on the left, a ▾ on the right and a thin line under
//!   it ("Blind ▾"). Clicking opens a menu of the options (some can be disabled); choosing one
//!   triggers [`SelectChange`] on the select.
//! - [`NumberField`]: a value with units and expressions ("Depth  25 mm"): a small grey label,
//!   the value right-aligned over a thin line (blue while focused), and an optional icon after
//!   it. Enter (or leaving the field) triggers [`NumberFieldCommit`] with the text; Esc triggers
//!   [`NumberFieldCancel`] and leaves the field. The owner evaluates the text and sets [`NumberFieldState`]: its
//!   `text` is shown whenever the field is not being edited, and `error` turns it red.
//! - [`OptionRow`]: an option with an optional collapsed ▸ in front and a checkbox ("▸ ☐
//!   Direction"), disabled when the option is not available.
//! - [`Slider`]: a thin track with a round knob (Onshape's detail slider). Dragging it, or
//!   pressing on the track, moves the knob and triggers [`SliderChange`].

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontWeight, TextCursorStyle, TextEdit};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::icon::{icon, icon_in};
use crate::input::TextInputField;
use crate::menu::{Menu, MenuAction, MenuItem, open_menu};
use crate::style::{InheritFg, StateColors, Visuals};
use crate::theme::Theme;

pub struct DialogFieldsPlugin;

impl Plugin for DialogFieldsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_select_activate)
            .add_observer(on_select_menu_action)
            .add_observer(on_number_key)
            .add_observer(on_number_press)
            .add_observer(on_slider_drag)
            .add_observer(on_slider_press)
            .add_systems(
                PostUpdate,
                (
                    sync_selects,
                    // The blur commit must see what was typed before the sync puts the state's
                    // text back into the field it left (unordered, a Tab out of a field lost the
                    // typed value on some frames: the reflector's tapped depth 10.02, P3.10).
                    (commit_on_blur, sync_number_fields).chain(),
                    sync_sliders,
                )
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// The thin line under a select or a number field.
fn underline() -> Color {
    Color::srgb_u8(0xd9, 0xd9, 0xd9)
}

fn label_grey() -> Color {
    Color::srgb_u8(0x6b, 0x6b, 0x6b)
}

fn chevron_grey() -> Color {
    Color::srgb_u8(0xb4, 0xb4, 0xb4)
}

// ---------------------------------------------------------------------------------------------
// Form row

/// A settings row for a dialog (Onshape's "Workspace units" dialog): a label in a fixed-width
/// column on the left and the control (spawn it as the row's child) filling the rest.
pub fn form_row(
    theme: &Theme,
    name: impl Into<Cow<'static, str>>,
    label: impl Into<String>,
    label_width: f32,
) -> impl Bundle {
    let t = theme.clone();
    let label = label.into();
    (
        Name::new(name.into().into_owned()),
        Node {
            height: Val::Px(32.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            ..default()
        },
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn((
                t.text(label, t.font_base, FontWeight::MEDIUM, t.foreground),
                Node {
                    width: Val::Px(label_width),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
        })),
    )
}

// ---------------------------------------------------------------------------------------------
// Select

/// A select's options and the chosen one.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SelectState {
    pub options: Vec<(String, bool)>,
    pub selected: usize,
}

/// An option of a select was chosen. Targets the select.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectChange {
    pub entity: Entity,
    pub index: usize,
}

#[derive(Component, Debug, Clone, Copy)]
struct SelectValue(Entity);

/// Which way a select's list opens (below by default).
#[derive(Component, Debug, Clone, Copy)]
struct SelectSide(bevy::ui_widgets::popover::PopoverSide);

/// Builder for a dropdown select.
pub struct Select {
    name: Cow<'static, str>,
    options: Vec<(String, bool)>,
    selected: usize,
    width: Val,
    side: bevy::ui_widgets::popover::PopoverSide,
    bordered: bool,
}

impl Select {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            options: Vec::new(),
            selected: 0,
            width: Val::Auto,
            side: bevy::ui_widgets::popover::PopoverSide::Bottom,
            bordered: false,
        }
    }

    /// A boxed select (a 1 px grey border all round, as the Material dialog's dropdowns,
    /// P3.6) instead of the underlined one.
    pub fn bordered(mut self) -> Self {
        self.bordered = true;
        self
    }

    /// Opens the list above the select (for one near the bottom of a dialog, so the list does
    /// not cover the dialog's buttons).
    pub fn open_up(mut self) -> Self {
        self.side = bevy::ui_widgets::popover::PopoverSide::Top;
        self
    }

    /// Adds an option (`enabled: false` shows it greyed in the menu).
    pub fn option(mut self, label: impl Into<String>, enabled: bool) -> Self {
        self.options.push((label.into(), enabled));
        self
    }

    pub fn selected(mut self, i: usize) -> Self {
        self.selected = i;
        self
    }

    pub fn width(mut self, w: Val) -> Self {
        self.width = w;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let value = self
            .options
            .get(self.selected)
            .map(|o| o.0.clone())
            .unwrap_or_default();
        let bordered = self.bordered;
        let visuals = Visuals {
            background: if bordered {
                StateColors::new(t.background, t.list_hover, t.list_active, t.background)
            } else {
                StateColors::new(Color::NONE, t.ghost_hover, t.ghost_active, Color::NONE).with_selected(t.ghost_hover)
            },
            border: StateColors::all(if bordered { Color::srgb_u8(0xc8, 0xc8, 0xc8) } else { underline() }),
            foreground: StateColors::new(
                t.tool_foreground,
                t.tool_foreground,
                t.tool_foreground,
                t.disabled_foreground,
            ),
            focus_ring: t.focus_ring,
        };
        (
            Name::new(self.name.into_owned()),
            SelectSide(self.side),
            SelectState {
                options: self.options,
                selected: self.selected,
            },
            if bordered {
                Node {
                    width: self.width,
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    height: Val::Px(26.0),
                    padding: UiRect::horizontal(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                }
            } else {
                Node {
                    width: self.width,
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    height: Val::Px(24.0),
                    padding: UiRect::new(Val::Px(3.0), Val::Px(4.0), Val::ZERO, Val::ZERO),
                    align_items: AlignItems::Center,
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                }
            },
            WidgetButton,
            Hovered::default(),
            visuals,
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let select = p.target_entity();
                let (size, weight) = if bordered { (12.5, FontWeight::NORMAL) } else { (t.font_base, FontWeight::MEDIUM) };
                // A long value ("Start from selected plane") is cut with "…" before the caret
                // (P3.10 judge: it ran under it).
                p.spawn((
                    SelectValue(select),
                    t.text(value, size, weight, t.tool_foreground),
                    InheritFg,
                    crate::ellipsis::Ellipsis::default(),
                    Node {
                        flex_grow: 1.0,
                        margin: UiRect::right(Val::Px(3.0)),
                        ..crate::ellipsis::Ellipsis::node()
                    },
                    Pickable::IGNORE,
                ))
                .insert(TextLayout::new(Justify::Left, LineBreak::NoWrap));
                if bordered {
                    p.spawn((icon("caret-down", 12.0, t.foreground), Pickable::IGNORE));
                } else {
                    p.spawn((icon("caret-down-filled", 10.0, t.tool_foreground), Pickable::IGNORE));
                }
            })),
        )
    }
}

fn on_select_activate(
    a: On<Activate>,
    q: Query<(&SelectState, &ComputedNode, &Name, Option<&SelectSide>)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok((state, node, name, side)) = q.get(a.entity) else {
        return;
    };
    let width = node.size().x * node.inverse_scale_factor();
    let mut menu = Menu::new(format!("{name}-menu"))
        .min_width(width.max(120.0))
        .item_height(24.0)
        .text_only();
    if let Some(side) = side {
        menu = menu.side(side.0);
    }
    for (i, (label, enabled)) in state.options.iter().enumerate() {
        menu = menu.item(MenuItem::new(format!("{name}-option-{i}"), label.clone()).disabled(!enabled));
    }
    open_menu(&mut commands, a.entity, menu.build(&theme));
}

fn on_select_menu_action(
    ev: On<MenuAction>,
    mut q: Query<(&mut SelectState, &Name)>,
    mut commands: Commands,
) {
    let Ok((mut state, name)) = q.get_mut(ev.entity) else {
        return;
    };
    let prefix = format!("{name}-option-");
    let Some(i) = ev.item.strip_prefix(&prefix).and_then(|n| n.parse::<usize>().ok()) else {
        return;
    };
    commands.queue(crate::menu::close_all_menus);
    if state.selected != i {
        state.selected = i;
        commands.trigger(SelectChange {
            entity: ev.entity,
            index: i,
        });
    }
}

fn sync_selects(
    q: Query<(Entity, &SelectState), Changed<SelectState>>,
    mut q_value: Query<(&SelectValue, &mut Text)>,
) {
    for (e, s) in &q {
        let want = s.options.get(s.selected).map(|o| o.0.as_str()).unwrap_or("");
        for (v, mut text) in &mut q_value {
            if v.0 == e && text.0 != want {
                text.0 = want.to_string();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Number field

/// What a number field shows while it is not being edited, and whether it is in error.
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct NumberFieldState {
    pub text: String,
    pub error: bool,
}

/// Enter was pressed in a number field, or it lost focus after an edit. Targets the field's
/// root and carries the text.
#[derive(EntityEvent, Debug, Clone)]
pub struct NumberFieldCommit {
    pub entity: Entity,
    pub text: String,
    /// Committed with Enter (not by leaving the field).
    pub enter: bool,
}

/// Esc was pressed in a number field: the shown text goes back to the state's.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct NumberFieldCancel {
    pub entity: Entity,
}

/// The editable text of a number field; points at the root.
#[derive(Component, Debug, Clone, Copy)]
pub struct NumberFieldEdit(pub Entity);

/// The line under the value; points at the root.
#[derive(Component, Debug, Clone, Copy)]
struct NumberFieldLine(Entity);

/// Builder for a number field.
pub struct NumberField {
    name: Cow<'static, str>,
    label: String,
    text: String,
    chevron: bool,
    trailing: Option<(Cow<'static, str>, String)>,
    label_width: f32,
    label_size: f32,
    disabled: bool,
    label_icon: Option<(Cow<'static, str>, Color)>,
}

impl NumberField {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            text: String::new(),
            chevron: false,
            trailing: None,
            label_width: 48.0,
            label_size: 11.0,
            disabled: false,
            label_icon: None,
        }
    }

    /// Greyed out and not editable (a parameter the owner can't use; give the root a tooltip
    /// saying why).
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    /// A small coloured icon right after the label (a mate offset's axis arrow, "X↘", A6.10).
    pub fn label_icon(mut self, icon: impl Into<Cow<'static, str>>, color: Color) -> Self {
        self.label_icon = Some((icon.into(), color));
        self
    }

    /// The value as shown ("25 mm").
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = t.into();
        self
    }

    /// A collapsed ▸ in front (the parameter has more options).
    pub fn chevron(mut self) -> Self {
        self.chevron = true;
        self
    }

    /// The width of the label column (48 px by default).
    pub fn label_width(mut self, w: f32) -> Self {
        self.label_width = w;
        self
    }

    /// The label's font size (11 px by default; a dialog whose label column is wide enough can
    /// match Onshape's feature dialogs, where labels are nearly the values' size, with 12).
    pub fn label_size(mut self, px: f32) -> Self {
        self.label_size = px;
        self
    }

    /// An icon after the value (named `<name>-icon`), with a tooltip.
    pub fn trailing_icon(mut self, icon: impl Into<Cow<'static, str>>, tip: impl Into<String>) -> Self {
        self.trailing = Some((icon.into(), tip.into()));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.into_owned();
        let edit_name = format!("{name}-field");
        let icon_name = format!("{name}-icon");
        let text = self.text;
        let label = self.label;
        let chevron = self.chevron;
        let trailing = self.trailing;
        let label_width = self.label_width;
        let label_size = self.label_size;
        let disabled = self.disabled;
        let label_icon = self.label_icon;
        (
            Name::new(name),
            NumberFieldState {
                text: text.clone(),
                error: false,
            },
            Node {
                height: Val::Px(26.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                p.spawn((
                    icon_in(
                        "chevron-right",
                        10.0,
                        if chevron { chevron_grey() } else { Color::NONE },
                        Node {
                            margin: UiRect::new(Val::Px(3.0), Val::Px(5.0), Val::ZERO, Val::ZERO),
                            ..default()
                        },
                    ),
                    Pickable::IGNORE,
                ));
                let label_color = if disabled { t.disabled_foreground } else { label_grey() };
                p.spawn((
                    Node {
                        width: Val::Px(label_width),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(2.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|l| {
                    // An axis label takes its icon's colour (X red, Y green, Z blue, as the triad).
                    // A disabled field greys its label whatever the icon.
                    let fg = if disabled {
                        label_color
                    } else {
                        label_icon.as_ref().map(|(_, c)| *c).unwrap_or(label_color)
                    };
                    let weight = if label_icon.is_some() { FontWeight::MEDIUM } else { FontWeight::NORMAL };
                    l.spawn((t.text(label, label_size, weight, fg), Pickable::IGNORE));
                    if let Some((i, color)) = label_icon {
                        l.spawn((icon(i, 11.0, color), Pickable::IGNORE));
                    }
                });
                p.spawn((
                    NumberFieldLine(root),
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(20.0),
                        padding: UiRect::right(Val::Px(3.0)),
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(Val::Px(1.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BorderColor::all(underline()),
                ))
                .with_children(|l| {
                let mut edit = l.spawn((
                    Name::new(edit_name),
                    NumberFieldEdit(root),
                    TextInputField,
                    EditableText::new(&text),
                    EditableTextFilter::new(|c: char| !c.is_control()),
                    t.font(12.0, FontWeight::MEDIUM),
                    TextColor(t.tool_foreground),
                    TextLayout::new(Justify::Right, LineBreak::NoWrap),
                    TextCursorStyle {
                        color: t.caret,
                        selection_color: t.selection,
                        unfocused_selection_color: Color::NONE,
                        selected_text_color: Some(t.selection_foreground),
                    },
                    TabIndex(0),
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(16.0),
                        ..default()
                    },
                ));
                if disabled {
                    edit.insert((InteractionDisabled, Pickable::IGNORE)).remove::<TabIndex>();
                }
                });
                if let Some((i, tip)) = trailing {
                    p.spawn((
                        Name::new(icon_name),
                        icon_in(
                            i,
                            16.0,
                            t.tool_foreground,
                            Node {
                                margin: UiRect::left(Val::Px(7.0)),
                                ..default()
                            },
                        ),
                        crate::Tooltip::new(tip),
                    ));
                }
            })),
        )
    }
}

fn on_number_key(
    mut ev: On<FocusedInput<KeyboardInput>>,
    mut q_edit: Query<(&mut EditableText, &NumberFieldEdit)>,
    q_state: Query<&NumberFieldState>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Ok((mut text, edit)) = q_edit.get_mut(ev.focused_entity) else {
        return;
    };
    if ev.input.state != ButtonState::Pressed || text.is_composing() {
        return;
    }
    match ev.input.key_code {
        KeyCode::Enter | KeyCode::NumpadEnter => {
            ev.propagate(false);
            commands.trigger(NumberFieldCommit {
                entity: edit.0,
                text: text.value().to_string(),
                enter: true,
            });
        }
        KeyCode::Escape => {
            // The value goes back and the field is left (a second Esc then closes the
            // dialog, as in Onshape).
            ev.propagate(false);
            if let Ok(state) = q_state.get(edit.0) {
                text.editor_mut().set_text(&state.text);
            }
            commands.trigger(NumberFieldCancel { entity: edit.0 });
            focus.clear();
        }
        _ => {}
    }
}

/// Pressing on a number field (its value or the line under it) focuses it; focusing selects
/// the whole value.
fn on_number_press(
    mut press: On<Pointer<Press>>,
    q_line: Query<&NumberFieldLine>,
    q_edit: Query<(Entity, &NumberFieldEdit), Without<InteractionDisabled>>,
    mut focus: ResMut<InputFocus>,
) {
    let root = if let Ok(l) = q_line.get(press.entity) {
        l.0
    } else if let Ok((_, e)) = q_edit.get(press.entity) {
        e.0
    } else {
        return;
    };
    press.propagate(false);
    if let Some((e, _)) = q_edit.iter().find(|(_, n)| n.0 == root)
        && focus.get() != Some(e)
    {
        focus.set(e, FocusCause::Pressed);
    }
}

/// Leaving a field after typing in it commits what was typed: the text differs from what the
/// field showed when it gained focus. Not from the state's text: an owner may update the state
/// in the frame the field loses focus (a click elsewhere clears the focus before `Update`), so
/// a field that was only tabbed through (the hole's Tap clearance, after its Tapped depth
/// changed) would commit its stale text over the new value (P3.10). Runs before
/// [`sync_number_fields`], which puts the state's text back into a field once it's left.
fn commit_on_blur(
    focus: Res<InputFocus>,
    mut last: Local<Option<Entity>>,
    mut at_focus: Local<Option<String>>,
    q_edit: Query<(&EditableText, &NumberFieldEdit)>,
    mut commands: Commands,
) {
    let now = focus.get();
    if *last == now {
        return;
    }
    if let Some(prev) = *last
        && let Ok((text, edit)) = q_edit.get(prev)
        && at_focus.as_deref().is_some_and(|t| text.value() != t)
    {
        commands.trigger(NumberFieldCommit {
            entity: edit.0,
            text: text.value().to_string(),
            enter: false,
        });
    }
    *at_focus = now.and_then(|e| q_edit.get(e).ok()).map(|(t, _)| t.value().to_string());
    *last = now;
}

/// Shows the state's text while the field is not focused; the line turns blue while it is, and
/// the text red on an error.
fn sync_number_fields(
    theme: Res<Theme>,
    focus: Res<InputFocus>,
    q_state: Query<&NumberFieldState>,
    mut q_edit: Query<(Entity, &NumberFieldEdit, &mut EditableText, &mut TextColor, Has<InteractionDisabled>)>,
    mut q_line: Query<(&NumberFieldLine, &mut BorderColor, &mut Node)>,
    mut focused_before: Local<Option<Entity>>,
) {
    let focused = focus.get();
    for (e, edit, mut text, mut color, disabled) in &mut q_edit {
        let Ok(state) = q_state.get(edit.0) else {
            continue;
        };
        let is_focused = focused == Some(e);
        if !is_focused && text.value().to_string() != state.text {
            text.editor_mut().set_text(&state.text);
        }
        if is_focused && *focused_before != Some(e) {
            // Focusing selects the number, like Onshape.
            text.queue_edit(TextEdit::SelectAll);
        }
        let c = if state.error {
            theme.danger
        } else if disabled {
            theme.disabled_foreground
        } else {
            theme.tool_foreground
        };
        color.set_if_neq(TextColor(c));
        for (line, mut border, mut node) in &mut q_line {
            if line.0 != edit.0 {
                continue;
            }
            let (w, col) = if is_focused {
                (2.0, theme.input_border_focus)
            } else if state.error {
                (1.0, theme.danger)
            } else {
                (1.0, underline())
            };
            border.set_if_neq(BorderColor::all(col));
            if node.border.bottom != Val::Px(w) {
                node.border = UiRect::bottom(Val::Px(w));
            }
        }
    }
    *focused_before = focused;
}

/// Focuses a number field's text and selects it (for scripted scenarios and shortcuts).
pub fn focus_number_field(world: &mut World, root: Entity) {
    let mut q = world.query::<(Entity, &NumberFieldEdit)>();
    let Some(e) = q.iter(world).find(|(_, n)| n.0 == root).map(|(e, _)| e) else {
        return;
    };
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.set(e, FocusCause::Pressed);
    }
}

// ---------------------------------------------------------------------------------------------
// Option row

/// Builder for an option row: `▸ ☐ Label` (the ▸ only when the option has more settings).
pub struct OptionRow {
    name: Cow<'static, str>,
    label: String,
    chevron: bool,
    checked: bool,
    disabled: bool,
    icon: Option<&'static str>,
}

impl OptionRow {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            chevron: false,
            checked: false,
            disabled: false,
            icon: None,
        }
    }

    /// `☐ [icon] Label`: an entity picked from a list (P3G.4, the Derived dialog's parts,
    /// sketches, planes and mate connectors).
    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn chevron(mut self) -> Self {
        self.chevron = true;
        self
    }

    pub fn checked(mut self, c: bool) -> Self {
        self.checked = c;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.into_owned();
        let check_name = format!("{name}-checkbox");
        let label = self.label;
        let (chevron, checked, disabled, glyph) = (self.chevron, self.checked, self.disabled, self.icon);
        (
            Name::new(name),
            Node {
                height: Val::Px(25.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                if chevron {
                    p.spawn((
                        icon_in(
                            "chevron-right",
                            10.0,
                            chevron_grey(),
                            Node {
                                margin: UiRect::new(Val::Px(3.0), Val::Px(5.0), Val::ZERO, Val::ZERO),
                                ..default()
                            },
                        ),
                        Pickable::IGNORE,
                    ));
                }
                match glyph {
                    None => {
                        p.spawn(crate::Checkbox::new(check_name).label(label).checked(checked).disabled(disabled).build(&t));
                    }
                    Some(g) => {
                        p.spawn(crate::Checkbox::new(check_name).checked(checked).disabled(disabled).build(&t));
                        p.spawn((
                            icon_in(g, 14.0, t.muted_foreground, Node { margin: UiRect::horizontal(Val::Px(5.0)), flex_shrink: 0.0, ..default() }),
                            Pickable::IGNORE,
                        ));
                        p.spawn((t.text(label, t.font_sm, FontWeight::NORMAL, t.foreground), Pickable::IGNORE)).insert(TextLayout::no_wrap());
                    }
                }
            })),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Slider

/// A slider's value, 0 to 1.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SliderState {
    pub value: f32,
}

/// The slider was dragged. Targets the slider.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SliderChange {
    pub entity: Entity,
    pub value: f32,
}

#[derive(Component, Debug, Clone, Copy)]
struct SliderKnob(Entity);

/// Builder for a slider.
pub struct Slider {
    name: Cow<'static, str>,
    value: f32,
    width: f32,
    tooltip: Option<String>,
}

const KNOB: f32 = 11.0;

impl Slider {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            value: 0.5,
            width: 104.0,
            tooltip: None,
        }
    }

    pub fn value(mut self, v: f32) -> Self {
        self.value = v.clamp(0.0, 1.0);
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    pub fn build(self, _theme: &Theme) -> impl Bundle {
        let width = self.width;
        let value = self.value;
        let tip = self.tooltip.unwrap_or_default();
        (
            Name::new(self.name.into_owned()),
            SliderState { value },
            crate::Tooltip::new(tip),
            Hovered::default(),
            Node {
                width: Val::Px(width),
                height: Val::Px(KNOB + 2.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let slider = p.target_entity();
                p.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(1.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0x8c, 0x8c, 0x8c)),
                    Pickable::IGNORE,
                ));
                p.spawn((
                    SliderKnob(slider),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(value * (width - KNOB)),
                        width: Val::Px(KNOB),
                        height: Val::Px(KNOB),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                    BorderColor::all(Color::srgb_u8(0x8c, 0x8c, 0x8c)),
                    Pickable::IGNORE,
                ));
            })),
        )
    }
}

fn on_slider_drag(
    ev: On<Pointer<Drag>>,
    mut q: Query<(&mut SliderState, &ComputedNode, &bevy::ui::UiGlobalTransform, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    slide_to(ev.entity, ev.pointer_location.position.x, &mut q, &mut commands);
}

/// Pressing on the track moves the knob there (P3.5: the opacity slider).
fn on_slider_press(
    ev: On<Pointer<Press>>,
    mut q: Query<(&mut SliderState, &ComputedNode, &bevy::ui::UiGlobalTransform, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    if ev.button == PointerButton::Primary {
        slide_to(ev.entity, ev.pointer_location.position.x, &mut q, &mut commands);
    }
}

fn slide_to(
    entity: Entity,
    x: f32,
    q: &mut Query<(&mut SliderState, &ComputedNode, &bevy::ui::UiGlobalTransform, Has<InteractionDisabled>)>,
    commands: &mut Commands,
) {
    let Ok((mut s, node, t, disabled)) = q.get_mut(entity) else {
        return;
    };
    if disabled {
        return;
    }
    let scale = node.inverse_scale_factor();
    let w = node.size().x * scale;
    let left = t.translation.x * scale - w / 2.0;
    let v = ((x - left - KNOB / 2.0) / (w - KNOB)).clamp(0.0, 1.0);
    // A press at (or near) the track's ends is its end: the knob's half at each end is dead
    // space otherwise.
    let v = if v < 0.03 {
        0.0
    } else if v > 0.97 {
        1.0
    } else {
        v
    };
    if (s.value - v).abs() > 1e-4 {
        s.value = v;
        commands.trigger(SliderChange { entity, value: v });
    }
}

fn sync_sliders(
    q: Query<(Entity, &SliderState, &Node), Changed<SliderState>>,
    mut q_knob: Query<(&SliderKnob, &mut Node), Without<SliderState>>,
) {
    for (e, s, n) in &q {
        let Val::Px(w) = n.width else { continue };
        for (k, mut kn) in &mut q_knob {
            if k.0 == e {
                let left = Val::Px(s.value * (w - KNOB));
                if kn.left != left {
                    kn.left = left;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Committed(Vec<String>);

    /// Tabbing out of a field commits the typed text, not the state's old text: the sync that
    /// shows the state's text in unfocused fields used to run before the blur commit on some
    /// frames (the reflector's tapped depth 10.02 came back as 10 mm, P3.10).
    #[test]
    fn leaving_a_field_commits_what_was_typed() {
        for _ in 0..20 {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .init_resource::<InputFocus>()
                .init_resource::<Theme>()
                .init_resource::<Committed>()
                .add_plugins(DialogFieldsPlugin)
                .add_observer(|ev: On<NumberFieldCommit>, mut c: ResMut<Committed>| c.0.push(ev.text.clone()));
            let root = app.world_mut().spawn(NumberFieldState { text: "10 mm".into(), error: false }).id();
            let edit = app
                .world_mut()
                .spawn((NumberFieldEdit(root), EditableText::new("10 mm"), TextColor::WHITE))
                .id();
            let next = app.world_mut().spawn_empty().id();
            app.world_mut().resource_mut::<InputFocus>().set(edit, FocusCause::Pressed);
            app.update();
            app.world_mut().get_mut::<EditableText>(edit).unwrap().editor_mut().set_text("10.02");
            app.update();
            // Tab to the next field.
            app.world_mut().resource_mut::<InputFocus>().set(next, FocusCause::Navigated);
            app.update();
            assert_eq!(app.world().resource::<Committed>().0, vec!["10.02".to_string()]);
        }
    }

    /// A field that was only passed through doesn't commit its old text when its owner updates
    /// the state in the frame it loses focus (the hole's Tap clearance 6.667 overwrote the new
    /// tapped depth once Tapped depth changed it to 6.653, P3.10).
    #[test]
    fn leaving_an_untouched_field_commits_nothing() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<InputFocus>()
            .init_resource::<Theme>()
            .init_resource::<Committed>()
            .add_plugins(DialogFieldsPlugin)
            .add_observer(|ev: On<NumberFieldCommit>, mut c: ResMut<Committed>| c.0.push(ev.text.clone()));
        let root = app.world_mut().spawn(NumberFieldState { text: "6.667".into(), error: false }).id();
        let edit = app
            .world_mut()
            .spawn((NumberFieldEdit(root), EditableText::new("6.667"), TextColor::WHITE))
            .id();
        app.world_mut().resource_mut::<InputFocus>().set(edit, FocusCause::Navigated);
        app.update();
        app.update();
        // A click elsewhere clears the focus, and the owner updates the state in that frame.
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.world_mut().get_mut::<NumberFieldState>(root).unwrap().text = "6.653".into();
        app.update();
        assert!(app.world().resource::<Committed>().0.is_empty());
        assert_eq!(app.world().get::<EditableText>(edit).unwrap().value().to_string(), "6.653");
    }
}
