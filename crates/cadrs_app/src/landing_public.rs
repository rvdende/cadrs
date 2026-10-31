//! The documents page's **Public documents** panel, beside the documents: what anyone using
//! cadrs published, as this cadrs heard of it ([`crate::collab::public`]).
//!
//! - A caption (PUBLIC DOCUMENTS and the count, as the documents'), **Stars** | **Recent** for
//!   the order, a search box, and a line saying where cadrs stands: offline (with **Connect**,
//!   which also takes part), online but not taking part (with **Turn on**), looking for peers,
//!   or how many are in the room.
//! - A row a document: its picture, name, owner and when it was published. Clicking it opens it
//!   read-only; ★ stars it (or takes the star back) and shows how many star it; Fork saves it as
//!   a document of one's own.
//! - Hidden on a narrow window.
//!
//! Names: `public-card`, `public-heading`, `public-count`, `public-sort`, `public-search`,
//! `public-status`, `public-connect`, `public-turn-on`, `public-list`, and per row (by its
//! place) `public-row-<i>`, `public-name-<i>`, `public-star-<i>`, `public-fork-<i>`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, ScrollArea, observe};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::public::{PublicDoc, RoomState, Shared};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, IconButton, ListItem, Segmented, SegmentedSelect};

use super::thumbnail_image;
use crate::collab::public;
use crate::{AppClock, AppState};

/// The most rows drawn: a search finds the rest.
const MAX_ROWS: usize = 200;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PublicPanel>()
        .init_resource::<PublicThumbs>()
        .add_observer(on_sort)
        .add_systems(
            Update,
            (sync_search, rebuild)
                .chain()
                .run_if(in_state(AppState::Landing)),
        );
}

/// The panel's search and order (kept while a document is open).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct PublicPanel {
    pub search: String,
    /// Newest first, instead of most starred.
    pub recent: bool,
}

/// The pictures, decoded once each (by their hash).
#[derive(Resource, Default)]
struct PublicThumbs(HashMap<[u8; 32], Option<Handle<Image>>>);

#[derive(Component)]
struct PublicList;

#[derive(Component)]
struct PublicStatus;

#[derive(Component)]
struct PublicCount;

/// The panel, right of the documents.
pub(super) fn public_card(p: &mut ChildSpawnerCommands, t: &Theme, panel: &PublicPanel) {
    p.spawn((
        Name::new("public-card"),
        Node {
            width: Val::Px(380.0),
            flex_shrink: 0.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            padding: UiRect::new(Val::Px(12.0), Val::Px(8.0), Val::Px(6.0), Val::Px(8.0)),
            ..cadrs_ui::glass::panel_node(t)
        },
        cadrs_ui::glass::panel(t),
        cadrs_ui::spring::Appear::fly_up().delay(0.1),
        cadrs_ui::responsive::HideBelow(800.0),
    ))
    .with_children(|card| {
        card.spawn((
            Name::new("public-heading"),
            Node {
                flex_shrink: 0.0,
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                padding: UiRect::left(Val::Px(4.0)),
                ..default()
            },
        ))
        .with_children(|h| {
            // As the documents' caption.
            h.spawn((
                Name::new("public-title"),
                t.text(
                    tr("shell-public-title").to_uppercase(),
                    t.font_sm,
                    FontWeight::BOLD,
                    t.muted_foreground,
                ),
            ));
            h.spawn((
                Name::new("public-count"),
                PublicCount,
                t.text("", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
            ));
            h.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            h.spawn(
                Segmented::new("public-sort")
                    .option(
                        tr("shell-public-by-stars"),
                        None,
                        tr("shell-public-by-stars-tooltip"),
                    )
                    .option(
                        tr("shell-public-recent"),
                        None,
                        tr("shell-public-recent-tooltip"),
                    )
                    .selected(usize::from(panel.recent))
                    .size(22.0, 12.0)
                    .build(t),
            );
        });
        card.spawn(
            TextInput::new("public-search")
                .placeholder(tr("shell-public-search"))
                .leading_icon("search")
                .height(28.0)
                .cleanable()
                .value(panel.search.clone())
                .build(t),
        );
        card.spawn((
            Name::new("public-status"),
            PublicStatus,
            Node {
                flex_shrink: 0.0,
                min_height: Val::Px(24.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::left(Val::Px(4.0)),
                ..default()
            },
        ));
        card.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|wrap| {
            let list = wrap
                .spawn((
                    Name::new("public-list"),
                    PublicList,
                    ScrollArea,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::right(Val::Px(10.0)),
                        ..default()
                    },
                ))
                .id();
            wrap.spawn(cadrs_ui::vertical_scrollbar(
                t,
                "public-list-scrollbar",
                list,
            ));
        });
    });
}

#[allow(clippy::type_complexity)]
fn sync_search(
    mut panel: ResMut<PublicPanel>,
    q: Query<(&Name, &EditableText), (With<TextInputField>, Changed<EditableText>)>,
) {
    for (name, text) in &q {
        if name.as_str() == "public-search-field" {
            let v = text.value().to_string();
            if panel.search != v {
                panel.search = v;
            }
        }
    }
}

fn on_sort(ev: On<SegmentedSelect>, q: Query<&Name>, mut panel: ResMut<PublicPanel>) {
    if q.get(ev.entity).map(Name::as_str) == Ok("public-sort") {
        panel.recent = ev.index == 1;
    }
}

/// What the panel shows, to rebuild it only when that changes.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    /// The list's entity: a new page (back from a document) is filled again.
    list: Entity,
    generation: u64,
    panel: PublicPanel,
    room: RoomState,
    me: Option<String>,
    language: String,
}

/// The documents to list: matching the search, in the panel's order.
fn listed(mut docs: Vec<PublicDoc>, panel: &PublicPanel) -> Vec<PublicDoc> {
    let needle = panel.search.trim().to_lowercase();
    if !needle.is_empty() {
        docs.retain(|d| {
            let l = &d.listing;
            [&l.name, &l.owner_name, &l.description]
                .iter()
                .any(|s| s.to_lowercase().contains(&needle))
        });
    }
    docs.sort_by(|a, b| {
        let newer = b.listing.published.cmp(&a.listing.published);
        if panel.recent {
            newer
        } else {
            b.stars.cmp(&a.stars).then(newer)
        }
        .then_with(|| a.listing.name.cmp(&b.listing.name))
    });
    docs
}

fn rebuild(world: &mut World, mut last: Local<Option<Shown>>) {
    let mut lists = world.query_filtered::<Entity, With<PublicList>>();
    let Some(list) = lists.iter(world).next() else {
        *last = None;
        return;
    };
    let shared = world.resource::<Shared>().clone();
    let me = cadrs_net::cadrs_id(world.get_resource::<Iroh>());
    let shown = Shown {
        list,
        generation: shared.generation(),
        panel: world.resource::<PublicPanel>().clone(),
        room: cadrs_net::public::room_state(world),
        me: me.clone(),
        language: cadrs_ui::i18n::language().to_string(),
    };
    if last.as_ref() == Some(&shown) {
        return;
    }
    let all = shared.lock().directory.docs(me.as_deref().unwrap_or(""));
    let total = all.len();
    let docs = listed(all, &shown.panel);
    *last = Some(shown.clone());

    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    let clock = *world.resource::<AppClock>();
    let can_connect = world.contains_resource::<cadrs_net::CanConnect>();
    // The pictures, decoded once each (one still on its way shows the placeholder).
    let thumbs: Vec<Option<Handle<Image>>> = {
        let docs = &docs;
        world.resource_scope(|world, mut cache: Mut<PublicThumbs>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            docs.iter()
                .take(MAX_ROWS)
                .map(|d| {
                    let (Some(hash), Some(bytes)) = (d.listing.picture, &d.picture) else {
                        return None;
                    };
                    cache
                        .0
                        .entry(hash)
                        .or_insert_with(|| {
                            let img = image::load_from_memory(bytes).ok()?;
                            thumbnail_image(&mut images, img.to_rgba8())
                        })
                        .clone()
                })
                .collect()
        })
    };

    let mut counts = world.query_filtered::<Entity, With<PublicCount>>();
    if let Some(c) = counts.iter(world).next() {
        world.entity_mut(c).insert(Text::new(if total == 0 {
            String::new()
        } else {
            total.to_string()
        }));
    }
    let mut statuses = world.query_filtered::<Entity, With<PublicStatus>>();
    if let Some(s) = statuses.iter(world).next() {
        world.entity_mut(s).despawn_children();
        let text = match shown.room {
            RoomState::Offline if can_connect => tr("shell-public-status-offline"),
            RoomState::Offline => tr("shell-public-status-no-network"),
            RoomState::Off => tr("shell-public-status-off"),
            RoomState::Joining => tr("shell-public-status-joining"),
            RoomState::Joined(0) => tr("shell-public-status-alone"),
            RoomState::Joined(n) => {
                tr_with("shell-public-status-peers", &[("count", &n.to_string())])
            }
        };
        let dot = match shown.room {
            RoomState::Joined(n) if n > 0 => Color::srgb_u8(0x22, 0xa0, 0x55),
            RoomState::Joined(_) | RoomState::Joining => Color::srgb_u8(0xe0, 0xa0, 0x20),
            RoomState::Offline | RoomState::Off => t.muted_foreground,
        };
        world.entity_mut(s).with_children(|r| {
            r.spawn((
                Node {
                    width: Val::Px(7.0),
                    height: Val::Px(7.0),
                    border_radius: BorderRadius::MAX,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(dot),
            ));
            r.spawn(t.text(text, t.font_sm, FontWeight::NORMAL, t.muted_foreground))
                .insert(TextLayout::default())
                .entry::<Node>()
                .and_modify(|mut n| n.flex_grow = 1.0);
            if shown.room == RoomState::Offline && can_connect {
                r.spawn((
                    Button::new("public-connect")
                        .label(tr("shell-network-connect"))
                        .icon("network")
                        .icon_size(14.0)
                        .outline()
                        .small()
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        // To find public documents: online, and taking part.
                        commands.queue(|w: &mut World| {
                            crate::preferences_ui::set_public_documents(w, true);
                            crate::network::connect(w);
                        });
                    }),
                ));
            }
            if shown.room == RoomState::Off {
                r.spawn((
                    Button::new("public-turn-on")
                        .label(tr("shell-public-turn-on"))
                        .icon("public")
                        .icon_size(14.0)
                        .outline()
                        .small()
                        .tooltip(tr("shell-public-turn-on-tooltip"))
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            crate::preferences_ui::set_public_documents(w, true);
                        });
                    }),
                ));
            }
        });
    }

    world.entity_mut(list).despawn_children();
    world.entity_mut(list).with_children(|l| {
        if docs.is_empty() {
            empty(l, t, total > 0);
            return;
        }
        for (i, (d, thumb)) in docs.into_iter().zip(thumbs).enumerate() {
            row(l, t, &clock, i, d, thumb);
        }
    });
}

/// Nothing to list: nothing heard of yet, or nothing matching the search.
fn empty(l: &mut ChildSpawner, t: &Theme, searching: bool) {
    let (title, text) = if searching {
        (
            "shell-public-empty-search",
            "shell-public-empty-search-text",
        )
    } else {
        ("shell-landing-empty-public", "shell-public-empty-text")
    };
    l.spawn((
        Name::new("public-empty"),
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            padding: UiRect::axes(Val::Px(16.0), Val::Px(28.0)),
            ..default()
        },
    ))
    .with_children(|e| {
        e.spawn(icon("public", 28.0, t.muted_foreground));
        e.spawn(t.text(tr(title), t.font_md, FontWeight::MEDIUM, t.foreground));
        e.spawn(t.text(tr(text), t.font_sm, FontWeight::NORMAL, t.muted_foreground))
            .insert(TextLayout::justify(Justify::Center));
    });
}

/// One public document: picture, name, owner and date (opens it read-only), then ★ and Fork.
fn row(
    l: &mut ChildSpawner,
    t: &Theme,
    clock: &AppClock,
    i: usize,
    d: PublicDoc,
    thumb: Option<Handle<Image>>,
) {
    let listing = d.listing;
    let key = listing.key();
    let fg = t.foreground;
    let muted = t.muted_foreground;
    let name_font = t.font(t.font_base, FontWeight::MEDIUM);
    let detail_font = t.font(t.font_sm, FontWeight::NORMAL);
    let name = listing.name.clone();
    let detail = tr_with(
        "shell-public-row-detail",
        &[
            ("owner", listing.owner_name.as_str()),
            ("date", &clock.format(listing.published)),
        ],
    );
    let tooltip = if listing.description.is_empty() {
        tr("shell-public-open-tooltip")
    } else {
        listing.description.clone()
    };
    let label = format!("public-name-{i}");
    let (k_view, k_star, k_fork) = (key.clone(), key.clone(), key);
    l.spawn((
        Name::new(format!("public-doc-{i}")),
        Node {
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            ..default()
        },
    ))
    .with_children(|r| {
        r.spawn((
            ListItem::new(format!("public-row-{i}"))
                .height(52.0)
                .padding_left(4.0)
                .content(move |c| {
                    let node = Node {
                        width: Val::Px(56.0),
                        height: Val::Px(32.0),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    };
                    match thumb {
                        Some(img) => {
                            c.spawn((ImageNode::new(img), node, Pickable::IGNORE));
                        }
                        None => {
                            c.spawn((node, Pickable::IGNORE))
                                .with_child((icon("part", 20.0, muted), Pickable::IGNORE));
                        }
                    }
                    c.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            overflow: Overflow::clip_x(),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|col| {
                        col.spawn((
                            Name::new(label),
                            Text::new(name),
                            name_font,
                            TextColor(fg),
                            TextLayout::no_wrap(),
                            cadrs_ui::InheritFg,
                            Pickable::IGNORE,
                        ));
                        col.spawn((
                            Text::new(detail),
                            detail_font,
                            TextColor(muted),
                            TextLayout::no_wrap(),
                            Pickable::IGNORE,
                        ));
                    });
                })
                .build(t),
            cadrs_ui::Tooltip::new(tooltip),
            observe(move |_: On<Activate>, mut commands: Commands| {
                let k = k_view.clone();
                commands.queue(move |w: &mut World| public::view(w, &k));
            }),
        ))
        .entry::<Node>()
        .and_modify(|mut n| {
            n.flex_grow = 1.0;
            n.min_width = Val::Px(0.0);
        });
        r.spawn((
            Button::new(format!("public-star-{i}"))
                .label(d.stars.to_string())
                .icon("star")
                .icon_size(14.0)
                .ghost()
                .small()
                .selected(d.starred)
                .tooltip(tr(if d.starred {
                    "shell-public-unstar"
                } else {
                    "shell-public-star"
                }))
                .build(t),
            observe(move |_: On<Activate>, mut commands: Commands| {
                let k = k_star.clone();
                commands.queue(move |w: &mut World| public::toggle_star(w, &k));
            }),
        ));
        r.spawn((
            IconButton::new(format!("public-fork-{i}"), "branches")
                .small()
                .tooltip(tr("shell-public-fork-tooltip"))
                .build(t),
            observe(move |_: On<Activate>, mut commands: Commands| {
                let k = k_fork.clone();
                commands.queue(move |w: &mut World| public::fork(w, &k));
            }),
        ));
    });
}
