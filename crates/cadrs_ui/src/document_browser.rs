//! The **Other documents** browser of Insert-style dialogs (P3G.1, ER1.2, ER1.3, ER1.6;
//! `external-references/lesson-inserting-linked-documents.png`, `ex1-step4.png` B and C): the
//! pieces the assembly Insert dialog and the drawing Insert view browser put together.
//!
//! - [`BrowserSearch`]: a filter button and a search field ("Search or paste document id"),
//!   with a magnifier; names `<name>-filter` and the field `<name>` (`<name>-field`).
//! - [`location_row`]: a location (My documents, Recently opened, Created by me, a folder) with
//!   its icon, a clickable list row.
//! - [`DocumentRow`]: a document: a 34 px thumbnail (or a placeholder), its name in bold and a
//!   muted subtitle under it (the newest version, "V1", or "No versions").
//! - [`OpenedHeader`]: the document picked: a back arrow (`<name>-back`) to the list, its name
//!   and "↳ V1" (the version the dialog reads) under it, and the **Create version**
//!   (`<name>-create-version`) and **Version graph** (`<name>-graph`, pressed while the graph
//!   shows) buttons at the right.
//!
//! Rows are buttons: clicking one triggers `Activate` on it; the app knows what it stands for.
//! Modeled on gpui-component's `List` / `ListItem` and its `Input` with a prefix.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::button::IconButton;
use crate::ellipsis::Ellipsis;
use crate::icon::icon;
use crate::input::TextInput;
use crate::list::ListItem;
use crate::style::{InitState, StateColors, Visuals};
use crate::theme::Theme;
use crate::tooltip::Tooltip;

/// The filter button and the search field.
#[derive(Debug, Clone)]
pub struct BrowserSearch {
    name: Cow<'static, str>,
    placeholder: String,
    value: String,
}

impl BrowserSearch {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), placeholder: "Search or paste document id".into(), value: String::new() }
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let BrowserSearch { name, placeholder, value } = self;
        (
            Node {
                padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(6.0), Val::Px(4.0)),
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                p.spawn(IconButton::new(format!("{name}-filter"), "filter").icon_size(15.0).tooltip("Filter").build(&t));
                p.spawn(Node { flex_grow: 1.0, align_items: AlignItems::Center, ..default() }).with_children(|f| {
                    f.spawn(TextInput::new(name.clone()).placeholder(placeholder).value(value).height(26.0).width(Val::Percent(100.0)).build(&t));
                    f.spawn((
                        crate::icon::icon_in("search", 14.0, t.muted_foreground, Node { position_type: PositionType::Absolute, right: Val::Px(6.0), ..default() }),
                        Pickable::IGNORE,
                    ));
                });
            })),
        )
    }
}

/// A location row: its icon and label (22 px, as `lesson-inserting-linked-documents.png`).
pub fn location_row(name: impl Into<Cow<'static, str>>, icon_name: &'static str, label: impl Into<String>, theme: &Theme) -> impl Bundle {
    ListItem::new(name).icon(icon_name).icon_size(15.0).label(label).height(24.0).padding_left(10.0).build(theme)
}

/// A document in the browser's list.
#[derive(Debug, Clone)]
pub struct DocumentRow {
    name: Cow<'static, str>,
    title: String,
    subtitle: String,
    thumbnail: Option<Handle<Image>>,
    selected: bool,
    muted: bool,
    tooltip: Option<String>,
    icon: &'static str,
}

impl DocumentRow {
    pub fn new(name: impl Into<Cow<'static, str>>, title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self { name: name.into(), title: title.into(), subtitle: subtitle.into(), thumbnail: None, selected: false, muted: false, tooltip: None, icon: "file" }
    }

    /// The icon shown without a thumbnail (a document's `file` by default; a tab's own, P3G.4).
    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = name;
        self
    }

    pub fn thumbnail(mut self, image: Option<Handle<Image>>) -> Self {
        self.thumbnail = image;
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    /// Greyed: it can't be used (no versions yet, in the trash, …); still clickable.
    pub fn muted(mut self, m: bool) -> Self {
        self.muted = m;
        self
    }

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let DocumentRow { name, title, subtitle, thumbnail, selected, muted, tooltip, icon: glyph } = self;
        let fg = if muted { t.muted_foreground } else { t.foreground };
        (
            Name::new(name.into_owned()),
            Node {
                height: Val::Px(42.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            Visuals {
                background: StateColors::new(Color::NONE, t.list_hover, t.list_active, Color::NONE).with_selected(Color::srgb_u8(0xcf, 0xe3, 0xf7)),
                border: StateColors::all(Color::NONE),
                foreground: StateColors::all(fg),
                focus_ring: t.focus_ring,
            },
            InitState { disabled: false, selected, force: None },
            Tooltip::new(tooltip.unwrap_or_else(|| title.clone())),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let frame = Node {
                    width: Val::Px(34.0),
                    height: Val::Px(34.0),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                };
                match thumbnail {
                    Some(img) => {
                        p.spawn((frame, BorderColor::all(Color::srgb_u8(0xe4, 0xe4, 0xe7)), ImageNode::new(img), Pickable::IGNORE));
                    }
                    None => {
                        p.spawn((
                            frame,
                            BackgroundColor(Color::srgb_u8(0xf3, 0xf4, 0xf6)),
                            BorderColor::all(Color::srgb_u8(0xd4, 0xd4, 0xd8)),
                            Pickable::IGNORE,
                            children![(icon(glyph, 18.0, t.muted_foreground), Pickable::IGNORE)],
                        ));
                    }
                }
                // Title and subtitle cut with "…" where the row is too narrow (the row's tooltip
                // names it).
                p.spawn((Node { flex_direction: FlexDirection::Column, flex_shrink: 1.0, min_width: Val::Px(0.0), overflow: Overflow::clip(), ..default() }, Pickable::IGNORE))
                    .with_children(|c| {
                        let cut = || (TextLayout::no_wrap(), Ellipsis::default(), Ellipsis::node());
                        c.spawn((t.text(title, 12.0, FontWeight::BOLD, fg), Pickable::IGNORE)).insert(cut());
                        c.spawn((t.text(subtitle, 10.5, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE)).insert(cut());
                    });
            })),
        )
    }
}

/// The picked document's header.
#[derive(Debug, Clone)]
pub struct OpenedHeader {
    name: Cow<'static, str>,
    document: String,
    version: String,
    back: bool,
    graph_open: bool,
    create_version: bool,
}

impl OpenedHeader {
    /// `version`: what the dialog reads ("V1", "Main").
    pub fn new(name: impl Into<Cow<'static, str>>, document: impl Into<String>, version: impl Into<String>) -> Self {
        Self { name: name.into(), document: document.into(), version: version.into(), back: true, graph_open: false, create_version: true }
    }

    /// With a back arrow to the list (the Other documents tab; not the Current document's).
    pub fn back(mut self, b: bool) -> Self {
        self.back = b;
        self
    }

    /// The version graph is showing (its button pressed).
    pub fn graph_open(mut self, o: bool) -> Self {
        self.graph_open = o;
        self
    }

    /// Offer Create version.
    pub fn create_version(mut self, c: bool) -> Self {
        self.create_version = c;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let OpenedHeader { name, document, version, back, graph_open, create_version } = self;
        (
            Name::new(format!("{name}-header")),
            Node {
                padding: UiRect::new(Val::Px(if back { 2.0 } else { 8.0 }), Val::Px(6.0), Val::Px(6.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                flex_shrink: 0.0,
                ..default()
            },
            Children::spawn(SpawnWith(move |r: &mut ChildSpawner| {
                if back {
                    r.spawn(IconButton::new(format!("{name}-back"), "chevron-left").icon_size(15.0).tooltip("Back to the documents").build(&t));
                }
                r.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), overflow: Overflow::clip(), ..default() }, Pickable::IGNORE))
                    .with_children(|c| {
                        c.spawn((Name::new(format!("{name}-document")), t.text(document, 12.0, FontWeight::BOLD, t.foreground), Pickable::IGNORE)).insert(TextLayout::no_wrap());
                        c.spawn((Node { align_items: AlignItems::Center, column_gap: Val::Px(3.0), ..default() }, Pickable::IGNORE)).with_children(|m| {
                            m.spawn((t.text("\u{21b3}", 10.5, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
                            m.spawn((Name::new(format!("{name}-version")), t.text(version, 10.5, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
                        });
                    });
                let mut buttons: Vec<(String, &str, &str, bool)> = Vec::new();
                if create_version {
                    buttons.push((format!("{name}-create-version"), "versions", "Create version", false));
                }
                buttons.push((format!("{name}-graph"), "branches", "Version graph", graph_open));
                for (n, i, tip, on) in buttons {
                    r.spawn(IconButton::new(n, i.to_string()).icon_size(16.0).tooltip(tip).selected(on).build(&t))
                        .insert(BorderColor::all(Color::srgb_u8(0xd4, 0xd4, 0xd4)))
                        .entry::<Node>()
                        .and_modify(|mut n| {
                            n.width = Val::Px(26.0);
                            n.height = Val::Px(26.0);
                            n.border = UiRect::all(Val::Px(1.0));
                            n.flex_shrink = 0.0;
                        });
                }
            })),
        )
    }
}
