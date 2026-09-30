//! Pieces of the documents page's **Details** panel and labels (P3E.1, TD3.3, TD3.7, TD3.8;
//! `reference/onshape/training/test-drive/lesson-documents-page.png`), modeled on
//! gpui-component's `Sheet` content, `Tag` and `Sidebar` rail:
//!
//! - [`LabelChip`]: a label as a small rounded tag, a tint of its colour with a darker border
//!   and dark text (the documents list's Labels column and the details panel).
//! - [`side_tab`]: an icon button of the vertical rail next to the panel (Info, Versions,
//!   Where used): selected while its tab shows.
//! - [`details_header`]: the panel's header, the title in bold and a grey × (`<name>`).
//! - [`details_caption`] / [`details_value`]: a field's bold caption and its value under it,
//!   and [`details_divider`] between fields.
//!
//! The app builds the panel from these (it owns the data), like [`crate::document_browser`].

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::FontWeight;

use crate::button::{IconButton, visuals_for};
use crate::style::StateColors;
use crate::theme::Theme;

/// A label shown as a tag.
#[derive(Debug, Clone)]
pub struct LabelChip {
    name: Cow<'static, str>,
    label: String,
    colour: [u8; 3],
}

impl LabelChip {
    /// `colour` is the label's sRGB colour.
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>, colour: [u8; 3]) -> Self {
        Self { name: name.into(), label: label.into(), colour }
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let [r, g, b] = self.colour;
        let base = Color::srgb_u8(r, g, b);
        let fill = base.with_alpha(0.22);
        let font = theme.font(theme.font_xs, FontWeight::MEDIUM);
        let fg = theme.foreground;
        let label = self.label;
        (
            Name::new(self.name.into_owned()),
            Node {
                height: Val::Px(18.0),
                padding: UiRect::horizontal(Val::Px(6.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(9.0)),
                ..default()
            },
            BackgroundColor(fill),
            BorderColor::all(base),
            Pickable::IGNORE,
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                p.spawn((
                    Node { width: Val::Px(6.0), height: Val::Px(6.0), border_radius: BorderRadius::MAX, ..default() },
                    BackgroundColor(base),
                    Pickable::IGNORE,
                ));
                p.spawn((Text::new(label), font, TextColor(fg), TextLayout::no_wrap(), Pickable::IGNORE));
            })),
        )
    }
}

/// An icon button of the details rail: dark icon, a light blue fill while `selected`.
pub fn side_tab(theme: &Theme, name: impl Into<Cow<'static, str>>, icon: &'static str, tooltip: &str, selected: bool) -> impl Bundle {
    (IconButton::new(name, icon).icon_size(16.0).tooltip(tooltip).selected(selected).build(theme), SideTab)
}

/// Gives a ghost button the rail's look (set on insert, over the button's own visuals).
#[derive(Component, Debug, Clone, Copy, Default)]
#[component(on_insert = on_side_tab)]
pub struct SideTab;

fn on_side_tab(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    let Some(t) = world.get_resource::<Theme>().cloned() else { return };
    let mut v = visuals_for(&t, crate::ButtonVariant::Ghost);
    v.foreground = StateColors::all(t.foreground).with_selected(t.link);
    v.background = v.background.with_selected(t.list_selected);
    if let Some(mut visuals) = world.get_mut::<crate::style::Visuals>(ctx.entity) {
        *visuals = v;
    }
}

/// The panel header: `title` in bold and a × named `close`.
pub fn details_header(theme: &Theme, close: impl Into<Cow<'static, str>>, title: &str) -> impl Bundle {
    let t = theme.clone();
    let title = title.to_string();
    let close = close.into();
    (
        Node {
            height: Val::Px(34.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::new(Val::Px(12.0), Val::Px(6.0), Val::ZERO, Val::ZERO),
            ..default()
        },
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(t.text(title, t.font_base, FontWeight::BOLD, t.foreground));
            p.spawn(Node { flex_grow: 1.0, ..default() });
            let mut v = visuals_for(&t, crate::ButtonVariant::Ghost);
            v.foreground = StateColors::all(t.muted_foreground);
            p.spawn(IconButton::new(close, "close").icon_size(14.0).small().tooltip("Close").build(&t)).insert(v);
        })),
    )
}

/// A field caption (bold).
pub fn details_caption(theme: &Theme, text: impl Into<String>) -> impl Bundle {
    (
        theme.text(text.into(), theme.font_base, FontWeight::BOLD, theme.foreground),
        Node { margin: UiRect::top(Val::Px(8.0)), ..default() },
    )
}

/// A field value (regular, under its caption).
pub fn details_value(theme: &Theme, text: impl Into<String>) -> impl Bundle {
    (
        theme.text(text.into(), theme.font_base, FontWeight::NORMAL, theme.foreground),
        Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() },
    )
}

/// A thin line between fields.
pub fn details_divider(theme: &Theme) -> impl Bundle {
    (Node { height: Val::Px(1.0), flex_shrink: 0.0, ..default() }, BackgroundColor(theme.separator))
}
