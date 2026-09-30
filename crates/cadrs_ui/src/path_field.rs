//! A path field: a text input with a browse icon button at its right end, modeled on
//! gpui-component's `Input` with a suffix button (and a browser's file input). P3H.3: the PCB
//! Studio settings' "Component library" and "Create new component documents in this folder"
//! fields (`v2-settings-select-folder-poster.png`).
//!
//! The field's `Name` is `<name>`; its text input is `<name>-input` (the editable text
//! `<name>-input-field`) and the button `<name>-browse`. Clicking the button triggers
//! [`PathFieldBrowse`] on the field root; the owner opens a picker and puts the result back
//! with [`set_path_field`]. [`path_field_value`] reads the text.
//!
//! P3H.4: a **read-only** field ([`PathFieldBuilder::read_only`]) shows a short display name
//! (a document's name) in a box that can't be typed in, with the full location in a tooltip
//! ([`PathFieldBuilder::value_tooltip`]); the owner keeps the value and shows a new one with
//! [`set_path_field_display`]. Its text is `<name>-value`.

use std::borrow::Cow;

use bevy::prelude::*;
use bevy::text::{EditableText, TextEdit};
use bevy::ui_widgets::{Activate, observe};

use crate::button::IconButton;
use crate::input::{TextInput, TextInputField};
use crate::theme::Theme;

/// The browse button of the path field `name` was clicked. Targets the field root.
#[derive(EntityEvent, Clone, Debug)]
pub struct PathFieldBrowse {
    pub entity: Entity,
    pub name: String,
}

/// Marks a path field root.
#[derive(Component, Clone, Debug)]
pub struct PathField {
    pub name: String,
}

/// Builder for a path field.
pub struct PathFieldBuilder {
    name: Cow<'static, str>,
    value: String,
    placeholder: Option<String>,
    icon: Cow<'static, str>,
    tooltip: String,
    height: f32,
    read_only: bool,
    value_tooltip: Option<String>,
}

/// The display box of a read-only path field; its tooltip shows the full value.
#[derive(Component, Clone, Debug)]
pub struct PathFieldDisplay;

impl PathField {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(name: impl Into<Cow<'static, str>>) -> PathFieldBuilder {
        PathFieldBuilder {
            name: name.into(),
            value: String::new(),
            placeholder: None,
            icon: "folder".into(),
            tooltip: "Browse…".into(),
            height: 30.0,
            read_only: false,
            value_tooltip: None,
        }
    }
}

impl PathFieldBuilder {
    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = Some(p.into());
        self
    }

    /// The browse button's icon (default `folder`).
    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = i.into();
        self
    }

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = t.into();
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    /// Shows the value in a box that can't be typed in (only the browse button changes it).
    pub fn read_only(mut self, r: bool) -> Self {
        self.read_only = r;
        self
    }

    /// The tooltip of the value (a read-only field's full location).
    pub fn value_tooltip(mut self, t: impl Into<String>) -> Self {
        self.value_tooltip = Some(t.into());
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.into_owned();
        let read_only = self.read_only;
        let (shown, is_placeholder) = match (&self.value, &self.placeholder) {
            (v, Some(p)) if v.is_empty() => (p.clone(), true),
            (v, _) => (v.clone(), false),
        };
        let value_tip = self.value_tooltip.clone();
        let mut input = TextInput::new(format!("{name}-input")).value(self.value).height(self.height).width(Val::Percent(100.0));
        if let Some(p) = self.placeholder {
            input = input.placeholder(p);
        }
        let n = name.clone();
        let (icon, tip) = (self.icon, self.tooltip);
        let h = self.height;
        (
            Name::new(name.clone()),
            PathField { name: name.clone() },
            Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() },
            Children::spawn(bevy::ecs::spawn::SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                if read_only {
                    let fg = if is_placeholder { t.muted_foreground } else { t.foreground };
                    let mut b = p.spawn((
                        Name::new(format!("{n}-input")),
                        PathFieldDisplay,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            height: Val::Px(h),
                            padding: UiRect::horizontal(Val::Px(8.0)),
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(t.radius)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(t.input_disabled_background),
                        BorderColor::all(t.input_border),
                    ));
                    if let Some(tip) = value_tip {
                        b.insert(crate::tooltip::Tooltip::new(tip));
                    }
                    b.with_child((
                        Name::new(format!("{n}-value")),
                        t.text(shown, t.font_base, bevy::text::FontWeight::NORMAL, fg),
                        Pickable::IGNORE,
                    ));
                } else {
                    p.spawn(input.build(&t)).entry::<Node>().and_modify(|mut node| {
                        node.flex_grow = 1.0;
                        node.min_width = Val::Px(0.0);
                    });
                }
                p.spawn((
                    IconButton::new(format!("{n}-browse"), icon).icon_size(18.0).tooltip(tip).build(&t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.trigger(PathFieldBrowse { entity: root, name: n.clone() });
                    }),
                ))
                .entry::<Node>()
                .and_modify(move |mut node| {
                    node.width = Val::Px(h);
                    node.height = Val::Px(h);
                    node.flex_shrink = 0.0;
                });
            })),
        )
    }
}

/// The text of the path field `name`.
pub fn path_field_value(world: &mut World, name: &str) -> Option<String> {
    let want = format!("{name}-input-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world).find(|(n, _)| n.as_str() == want).map(|(_, t)| t.value().to_string())
}

/// Replaces the text of the path field `name`.
pub fn set_path_field(world: &mut World, name: &str, value: &str) {
    let want = format!("{name}-input-field");
    let mut q = world.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == want) {
        t.queue_edit(TextEdit::SelectAll);
        t.queue_edit(TextEdit::Insert(value.to_string().into()));
    }
}

/// Shows a new value in the read-only path field `name`, with its full location as the tooltip.
pub fn set_path_field_display(world: &mut World, name: &str, text: &str, tooltip: &str) {
    let want = format!("{name}-value");
    let fg = world.resource::<Theme>().foreground;
    let mut q = world.query::<(&Name, &mut Text, &mut TextColor)>();
    if let Some((_, mut t, mut c)) = q.iter_mut(world).find(|(n, _, _)| n.as_str() == want) {
        t.0 = text.to_string();
        c.0 = fg;
    }
    let want = format!("{name}-input");
    let mut q = world.query_filtered::<(Entity, &Name), With<PathFieldDisplay>>();
    if let Some((e, _)) = q.iter(world).find(|(_, n)| n.as_str() == want) {
        world.entity_mut(e).insert(crate::tooltip::Tooltip::new(tooltip));
    }
}
