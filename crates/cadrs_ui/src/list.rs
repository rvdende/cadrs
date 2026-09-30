//! List rows and grid cards, modeled on gpui-component's `ListItem` (`list/list_item.rs`):
//! clickable rows with hover and selected states. Clicking triggers
//! `bevy::ui_widgets::Activate` on the row.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::icon::{icon, icon_in};
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// Builder for a list row. Either give it an icon/label/detail, or custom content.
pub struct ListItem {
    name: Cow<'static, str>,
    icon: Option<Cow<'static, str>>,
    label: Option<String>,
    detail: Option<String>,
    content: Option<SpawnFn>,
    height: Option<f32>,
    selected: bool,
    disabled: bool,
    force: Option<VisualState>,
    indicator: bool,
    disclosure: Option<bool>,
    padding_left: Option<f32>,
    trailing: Option<Cow<'static, str>>,
    weight: FontWeight,
    icon_size: f32,
}

impl ListItem {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            icon: None,
            label: None,
            detail: None,
            content: None,
            height: None,
            selected: false,
            disabled: false,
            force: None,
            indicator: false,
            disclosure: None,
            padding_left: None,
            trailing: None,
            weight: FontWeight::NORMAL,
            icon_size: 16.0,
        }
    }

    /// The label's font weight.
    pub fn weight(mut self, w: FontWeight) -> Self {
        self.weight = w;
        self
    }

    /// The leading icon's size (default 16 px).
    pub fn icon_size(mut self, s: f32) -> Self {
        self.icon_size = s;
        self
    }

    /// Shows a small chevron in the left padding: `Some(false)` collapsed (▸), `Some(true)`
    /// expanded (▾). For tree-like rows such as the sidebar's "Teams".
    pub fn disclosure(mut self, open: Option<bool>) -> Self {
        self.disclosure = open;
        self
    }

    /// Overrides the left padding (default 12 px).
    pub fn padding_left(mut self, px: f32) -> Self {
        self.padding_left = Some(px);
        self
    }

    /// An icon at the right end of the row.
    pub fn trailing_icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.trailing = Some(i.into());
        self
    }

    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(i.into());
        self
    }

    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }

    /// Secondary text, right-aligned.
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }

    /// Custom row content, spawned after the icon and label.
    pub fn content(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.content = Some(Box::new(f));
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }

    /// Shows a blue bar on the left edge when selected, like Onshape's sidebar filters.
    pub fn selection_indicator(mut self) -> Self {
        self.indicator = true;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let ListItem {
            name,
            icon: icon_name,
            label,
            detail,
            content,
            height,
            selected,
            disabled,
            force,
            indicator,
            disclosure,
            padding_left,
            trailing,
            weight,
            icon_size,
        } = self;
        let fg = theme.foreground;
        let muted = theme.muted_foreground;
        let (selected_bg, selected_fg) = if indicator {
            (theme.background, theme.link)
        } else {
            (theme.list_selected, theme.foreground)
        };
        let visuals = Visuals {
            background: StateColors::new(
                Color::NONE,
                theme.list_hover,
                theme.list_active,
                Color::NONE,
            )
            .with_selected(selected_bg),
            border: StateColors::all(Color::NONE).with_selected(if indicator {
                theme.list_selected_bar
            } else {
                Color::NONE
            }),
            foreground: StateColors::new(fg, fg, fg, theme.disabled_foreground)
                .with_selected(selected_fg),
            focus_ring: theme.focus_ring,
        };
        let font = theme.font(theme.font_md, weight);
        let detail_font = theme.font(theme.font_base, FontWeight::NORMAL);
        let detail_color = theme.muted_foreground;
        let gap = theme.space[4];
        (
            Name::new(name.into_owned()),
            Node {
                height: Val::Px(height.unwrap_or(theme.list_row_height)),
                padding: UiRect::new(
                    Val::Px(padding_left.unwrap_or(theme.space[5])),
                    Val::Px(theme.space[5]),
                    Val::ZERO,
                    Val::ZERO,
                ),
                border: UiRect::left(Val::Px(if indicator { 3.0 } else { 0.0 })),
                align_items: AlignItems::Center,
                column_gap: Val::Px(gap),
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            visuals,
            InitState {
                disabled,
                selected,
                force,
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                if let Some(open) = disclosure {
                    p.spawn((
                        icon_in(
                            if open { "chevron-down" } else { "chevron-right" },
                            14.0,
                            fg,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                ..default()
                            },
                        ),
                        Pickable::IGNORE,
                    ));
                }
                if let Some(i) = icon_name {
                    p.spawn((icon(i, icon_size, fg), InheritFg, Pickable::IGNORE));
                }
                if let Some(l) = label {
                    p.spawn((
                        Text::new(l),
                        font,
                        TextColor(fg),
                        TextLayout::no_wrap(),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
                if let Some(f) = content {
                    f(p);
                }
                if let Some(t) = trailing {
                    p.spawn((
                        icon_in(
                            t,
                            16.0,
                            muted,
                            Node {
                                margin: UiRect::left(Val::Auto),
                                ..default()
                            },
                        ),
                        Pickable::IGNORE,
                    ));
                }
                if let Some(d) = detail {
                    p.spawn((
                        Text::new(d),
                        detail_font,
                        TextColor(detail_color),
                        TextLayout::no_wrap(),
                        Pickable::IGNORE,
                        Node {
                            margin: UiRect::left(Val::Auto),
                            ..default()
                        },
                    ));
                }
            })),
        )
    }
}

/// Builder for a grid card: a thumbnail area and a caption.
pub struct GridItem {
    name: Cow<'static, str>,
    label: String,
    thumbnail: Option<Handle<Image>>,
    selected: bool,
    force: Option<VisualState>,
    size: Vec2,
    icon: Cow<'static, str>,
}

impl GridItem {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            thumbnail: None,
            selected: false,
            force: None,
            size: Vec2::new(168.0, 150.0),
            icon: Cow::Borrowed("part"),
        }
    }

    /// The icon drawn when there is no thumbnail (default "part"; a folder card's "folder").
    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = i.into();
        self
    }

    pub fn thumbnail(mut self, image: Handle<Image>) -> Self {
        self.thumbnail = Some(image);
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let visuals = Visuals {
            background: StateColors::new(
                theme.background,
                theme.list_hover,
                theme.list_active,
                theme.background,
            )
            .with_selected(theme.list_selected),
            border: StateColors::new(
                theme.border,
                theme.border_strong,
                theme.focus_ring,
                theme.border,
            )
            .with_selected(theme.primary),
            foreground: StateColors::all(theme.foreground),
            focus_ring: theme.focus_ring,
        };
        let thumb_bg = Color::srgb_u8(0xf4, 0xf4, 0xf4);
        let placeholder = theme.subtle_foreground;
        let font = theme.font(theme.font_base, FontWeight::MEDIUM);
        let fg = theme.foreground;
        let thumbnail = self.thumbnail;
        let placeholder_icon = self.icon;
        let label = self.label;
        let radius = theme.radius;
        (
            Name::new(self.name.into_owned()),
            Node {
                width: Val::Px(self.size.x),
                height: Val::Px(self.size.y),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(theme.space[3])),
                row_gap: Val::Px(theme.space[3]),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(theme.radius_lg)),
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            visuals,
            InitState {
                disabled: false,
                selected: self.selected,
                force: self.force,
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let mut thumb = p.spawn((
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(radius)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(thumb_bg),
                    Pickable::IGNORE,
                ));
                match thumbnail {
                    Some(img) => {
                        // The image keeps its aspect ratio (square part thumbnails, wide
                        // document thumbnails).
                        thumb.with_child((
                            ImageNode::new(img),
                            Node {
                                height: Val::Percent(100.0),
                                max_width: Val::Percent(100.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    None => {
                        let size = if placeholder_icon == "part" { 24.0 } else { 40.0 };
                        thumb.with_child((icon(placeholder_icon, size, placeholder), Pickable::IGNORE));
                    }
                }
                p.spawn((
                    Text::new(label),
                    font,
                    TextColor(fg),
                    TextLayout::no_wrap(),
                    InheritFg,
                    Pickable::IGNORE,
                ));
            })),
        )
    }
}
