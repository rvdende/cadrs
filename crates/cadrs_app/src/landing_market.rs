//! The documents page's **Marketplace** panel, beside the documents: what others offer.
//!
//! - **Designs**: the public documents ([`super::public_panel`]), to open, star and fork.
//! - **Shops**: the organizations listed in the marketplace, as this cadrs heard of them
//!   ([`cadrs_org::directory`]): each with its logo, name, tagline, what it makes with, where,
//!   and its rating. A search box narrows them (every word, in the name, tagline, description,
//!   places, processes or materials); **Map…** opens Find a shop. Clicking a shop opens its
//!   page: all it says of itself, and **Add to contacts**, which is what Request a quote offers.
//!
//! An organization gets here by being complete and asking to be listed (its Overview); nothing
//! else is shown, so everything in the list has a logo, a tagline and a way to reach it.
//!
//! Names: `market-card`, `market-kind`, `market-designs`, `market-shops`, `market-shop-count`,
//! `market-search`, `market-map`, `market-shop-list`, `market-shops-empty`, per shop (by its
//! place) `market-shop-<i>`, `market-shop-name-<i>`, `market-shop-logo-<i>`; the shop's page is
//! `market-shop-dialog` with `market-shop-add`, `market-shop-copy` and `market-shop-close`.

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, ScrollArea, observe};
use cadrs_org::directory::{Query as ShopQuery, ShopListing};
use cadrs_social::contact::Card;
use cadrs_social::keys::AccountId;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, ListItem, Segmented, SegmentedSelect};

use super::public_panel::{self, PublicPanel};
use crate::AppState;
use crate::social::Social;

/// The most shops drawn: a search finds the rest.
const MAX_ROWS: usize = 100;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Market>()
        .add_observer(on_kind)
        .add_systems(
            Update,
            (sync_search, show_pane, rebuild)
                .chain()
                .run_if(in_state(AppState::Landing)),
        );
}

/// What the panel shows (kept while a document is open).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Market {
    /// Shops, instead of designs.
    pub shops: bool,
    pub search: String,
}

/// One of the panel's two bodies: the shops', or the designs'.
#[derive(Component)]
struct Pane {
    shops: bool,
}

#[derive(Component)]
struct ShopList;

#[derive(Component)]
struct ShopCount;

#[derive(Component)]
struct ShopDialog;

fn pane_display(shown: bool) -> Display {
    if shown { Display::Flex } else { Display::None }
}

/// The panel, right of the documents.
pub(super) fn market_card(
    p: &mut ChildSpawnerCommands,
    t: &Theme,
    panel: &PublicPanel,
    market: &Market,
) {
    p.spawn((
        Name::new("market-card"),
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
            Name::new("market-heading"),
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
            h.spawn(icon("marketplace", 16.0, t.muted_foreground));
            // As the documents' caption.
            h.spawn((
                Name::new("market-title"),
                t.text(
                    tr("market-title").to_uppercase(),
                    t.font_sm,
                    FontWeight::BOLD,
                    t.muted_foreground,
                ),
            ));
            h.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            h.spawn(
                Segmented::new("market-kind")
                    .option(tr("market-designs"), None, tr("market-designs-tooltip"))
                    .option(tr("market-shops"), None, tr("market-shops-tooltip"))
                    .selected(usize::from(market.shops))
                    .size(24.0, 12.0)
                    .build(t),
            );
        });
        card.spawn((
            Name::new("market-designs"),
            Pane { shops: false },
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                display: pane_display(!market.shops),
                ..default()
            },
        ))
        .with_children(|pane| public_panel::public_card(pane, t, panel));
        card.spawn((
            Name::new("market-shops"),
            Pane { shops: true },
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                display: pane_display(market.shops),
                ..default()
            },
        ))
        .with_children(|pane| shops_pane(pane, t, market));
    });
}

fn shops_pane(pane: &mut ChildSpawnerCommands, t: &Theme, market: &Market) {
    pane.spawn(Node {
        flex_shrink: 0.0,
        height: Val::Px(30.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        padding: UiRect::left(Val::Px(4.0)),
        ..default()
    })
    .with_children(|h| {
        h.spawn(t.text(
            tr("market-shops").to_uppercase(),
            t.font_sm,
            FontWeight::BOLD,
            t.muted_foreground,
        ));
        h.spawn((
            Name::new("market-shop-count"),
            ShopCount,
            t.text("", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
        ));
        h.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        h.spawn((
            Button::new("market-map")
                .label(tr("market-map"))
                .icon("map-pin")
                .icon_size(14.0)
                .small()
                .ghost()
                .tooltip(tr("market-map-tooltip"))
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(crate::find_shop::open);
            }),
        ));
    });
    pane.spawn(
        TextInput::new("market-search")
            .placeholder(tr("market-search"))
            .leading_icon("search")
            .height(28.0)
            .cleanable()
            .value(market.search.clone())
            .build(t),
    );
    pane.spawn(Node {
        flex_grow: 1.0,
        min_height: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        ..default()
    })
    .with_children(|wrap| {
        let list = wrap
            .spawn((
                Name::new("market-shop-list"),
                ShopList,
                ScrollArea,
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::right(Val::Px(10.0)),
                    ..default()
                },
            ))
            .id();
        wrap.spawn(cadrs_ui::vertical_scrollbar(
            t,
            "market-shop-list-scrollbar",
            list,
        ));
    });
}

fn on_kind(ev: On<SegmentedSelect>, q: Query<&Name>, mut market: ResMut<Market>) {
    if q.get(ev.entity).map(Name::as_str) == Ok("market-kind") {
        market.shops = ev.index == 1;
    }
}

fn show_pane(market: Res<Market>, mut panes: Query<(&Pane, &mut Node)>) {
    for (pane, mut node) in &mut panes {
        let display = pane_display(pane.shops == market.shops);
        if node.display != display {
            node.display = display;
        }
    }
}

#[allow(clippy::type_complexity)]
fn sync_search(
    mut market: ResMut<Market>,
    q: Query<(&Name, &EditableText), (With<TextInputField>, Changed<EditableText>)>,
) {
    for (name, text) in &q {
        if name.as_str() == "market-search-field" {
            let v = text.value().to_string();
            if market.search != v {
                market.search = v;
            }
        }
    }
}

/// What the shops' list shows, to rebuild it only when that changes.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    /// The list's entity: a new page (back from a document) is filled again.
    list: Entity,
    search: String,
    /// When what is known of shops, contacts and logos last changed.
    social: u32,
    language: String,
}

/// What a shop makes with and where it is, on one line: "Waterjet, Laser · Stellenbosch, South
/// Africa". A shop that only publishes designs says so.
pub(crate) fn summary(listing: &ShopListing) -> String {
    let mut parts = Vec::new();
    let processes: Vec<String> = listing
        .processes()
        .into_iter()
        .map(crate::organization::process_name)
        .collect();
    if !processes.is_empty() {
        parts.push(processes.join(", "));
    } else if listing.kinds.designs {
        parts.push(tr("org-offers-designs"));
    }
    if let Some(place) = listing.where_label() {
        parts.push(place);
    }
    parts.join(" · ")
}

/// The picture of a listed shop's logo if its bytes are here; else it is asked for, and none
/// for now.
fn logo_of(
    world: &mut World,
    card: &Card,
    blob: Option<cadrs_org::BlobRef>,
) -> Option<Handle<Image>> {
    let blob = blob?;
    let picture = crate::org_logo::handle(world, &blob);
    // Not here yet: asked for. (Here and not a logo: neither drawn nor asked for again.)
    if picture.is_none()
        && !world
            .resource::<crate::org_logo::LogoImages>()
            .seen(&blob.hash)
    {
        crate::social::want_logo(world, card, blob);
    }
    picture
}

/// How many logos are made into pictures in one frame, at the most: the rest of a long list
/// show their initials until the next frames get to them.
const LOGOS_A_FRAME: usize = 8;

fn rebuild(world: &mut World, mut last: Local<Option<Shown>>) {
    let mut lists = world.query_filtered::<Entity, With<ShopList>>();
    let Some(list) = lists.iter(world).next() else {
        *last = None;
        return;
    };
    let market = world.resource::<Market>().clone();
    if !market.shops {
        // Nothing is read, and no logo asked for, until the shops are looked at.
        return;
    }
    crate::social::ensure_loaded(world);
    let shown = Shown {
        list,
        search: market.search.clone(),
        social: world.resource_ref::<Social>().last_changed().get(),
        language: cadrs_ui::i18n::language().to_string(),
    };
    if last.as_ref() == Some(&shown) {
        return;
    }
    let social = world.resource::<Social>();
    let total = social.directory.len();
    let found: Vec<ShopListing> = social
        .directory
        .search(&ShopQuery {
            text: market.search.trim(),
            near: social.my_place.as_ref(),
            ..Default::default()
        })
        .into_iter()
        .take(MAX_ROWS)
        .map(|f| f.listing.clone())
        .collect();
    let mut made = 0;
    let logos: Vec<Option<Handle<Image>>> = found
        .iter()
        .map(|l| {
            let new = l.logo.is_some_and(|blob| {
                !world
                    .resource::<crate::org_logo::LogoImages>()
                    .seen(&blob.hash)
            });
            if new && made >= LOGOS_A_FRAME {
                return None;
            }
            let picture = logo_of(world, &l.card.value, l.logo);
            made += usize::from(new && picture.is_some());
            picture
        })
        .collect();
    // Asking for a logo notes that it was asked for: that isn't news to show. With logos still
    // to make, the list is built again next frame.
    *last = (made < LOGOS_A_FRAME).then(|| Shown {
        social: world.resource_ref::<Social>().last_changed().get(),
        ..shown
    });

    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    let mut counts = world.query_filtered::<Entity, With<ShopCount>>();
    if let Some(c) = counts.iter(world).next() {
        world.entity_mut(c).insert(Text::new(if total == 0 {
            String::new()
        } else {
            total.to_string()
        }));
    }
    world.entity_mut(list).despawn_children();
    world.entity_mut(list).with_children(|l| {
        if found.is_empty() {
            empty(l, t, total > 0);
            return;
        }
        for (i, (listing, logo)) in found.into_iter().zip(logos).enumerate() {
            row(l, t, i, listing, logo);
        }
    });
}

/// No shops to list: none heard of yet, or none matching the search.
fn empty(l: &mut ChildSpawner, t: &Theme, searching: bool) {
    let (title, text) = if searching {
        ("market-empty-search", "market-empty-search-text")
    } else {
        ("market-empty", "market-empty-text")
    };
    l.spawn((
        Name::new("market-shops-empty"),
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            padding: UiRect::axes(Val::Px(16.0), Val::Px(28.0)),
            ..default()
        },
    ))
    .with_children(|e| {
        e.spawn(icon("shop", 28.0, t.muted_foreground));
        e.spawn(t.text(tr(title), t.font_md, FontWeight::MEDIUM, t.foreground));
        e.spawn(t.text(tr(text), t.font_sm, FontWeight::NORMAL, t.muted_foreground))
            .insert(TextLayout::justify(Justify::Center));
    });
}

/// One shop: its logo, name and rating, tagline, and what it makes with and where. Opens its
/// page.
fn row(
    l: &mut ChildSpawner,
    t: &Theme,
    i: usize,
    listing: ShopListing,
    logo: Option<Handle<Image>>,
) {
    let account = listing.account();
    let tc = t.clone();
    l.spawn((
        ListItem::new(format!("market-shop-{i}"))
            .height(68.0)
            .padding_left(6.0)
            .content(move |c| {
                let t = &tc;
                c.spawn((
                    crate::org_logo::logo(
                        format!("market-shop-logo-{i}"),
                        t,
                        logo,
                        listing.name(),
                        44.0,
                    ),
                    Pickable::IGNORE,
                ));
                c.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(1.0),
                        margin: UiRect::left(Val::Px(10.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|col| {
                    col.spawn((
                        Node {
                            column_gap: Val::Px(8.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|top| {
                        top.spawn((
                            Name::new(format!("market-shop-name-{i}")),
                            t.text(
                                listing.name().to_owned(),
                                t.font_base,
                                FontWeight::SEMIBOLD,
                                t.foreground,
                            ),
                            Pickable::IGNORE,
                        ));
                    });
                    for (line, color) in [
                        (listing.tagline.clone(), t.foreground),
                        (summary(&listing), t.muted_foreground),
                    ] {
                        if !line.is_empty() {
                            // Cut with "…" where the panel is too narrow for it.
                            col.spawn((
                                t.text(line, t.font_sm, FontWeight::NORMAL, color),
                                cadrs_ui::ellipsis::Ellipsis::default(),
                                cadrs_ui::ellipsis::Ellipsis::node(),
                                Pickable::IGNORE,
                            ));
                        }
                    }
                });
            })
            .build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(move |w: &mut World| open_shop(w, account));
        }),
    ));
}

fn close_shop(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ShopDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

/// A shop's page: all its listing says.
pub(crate) fn open_shop(world: &mut World, account: AccountId) {
    crate::social::ensure_loaded(world);
    let social = world.resource::<Social>();
    let Some(listing) = social.directory.get(&account).cloned() else {
        return;
    };
    let known = social.contacts.get(&account).is_some();
    let mine = social.orgs.iter().any(|o| o.account == account);
    // The large logo for a page; the small one until it has arrived.
    let card = listing.card.value.clone();
    let logo =
        logo_of(world, &card, listing.logo_full).or_else(|| logo_of(world, &card, listing.logo));
    // The designs of the devices that say, with their own keys, that they publish for it: what
    // a publisher has to show. (The devices its card names prove nothing: a card can name
    // anyone's.)
    let devices: Vec<String> = listing
        .publishing_devices()
        .iter()
        .map(|d| d.to_string())
        .collect();
    let designs: Vec<(cadrs_net::public::Key, String)> = world
        .get_resource::<cadrs_net::public::Shared>()
        .map(|shared| shared.lock().directory.docs(""))
        .unwrap_or_default()
        .into_iter()
        .filter(|doc| {
            devices
                .iter()
                .any(|d| d.eq_ignore_ascii_case(&doc.listing.owner))
        })
        .take(MAX_DESIGNS)
        .map(|doc| (doc.listing.key(), doc.listing.name))
        .collect();
    let makes_parts = listing.kinds.manufacturing;
    close_shop(world);
    let state = *world.resource::<State<AppState>>().get();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let signed = listing.card.clone();
    world.spawn((
        Dialog::new("market-shop-dialog")
            .title(listing.name().to_owned())
            .width(640.0)
            .body(move |b| shop_page(b, &tb, &listing, logo, &designs))
            .footer(move |f| {
                let t = &tf;
                if mine {
                    f.spawn(t.text(
                        tr("market-shop-yours"),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ));
                } else {
                    // What one comes to a shop for: it becomes a contact and the shop the
                    // next request for a quote starts on.
                    if makes_parts {
                        let card = signed.clone();
                        f.spawn((
                            Button::new("market-shop-quote")
                                .label(tr("market-shop-quote"))
                                .icon("receipt")
                                .primary()
                                .build(t),
                            observe(move |_: On<Activate>, mut commands: Commands| {
                                let card = card.clone();
                                commands.queue(move |w: &mut World| request_quote(w, card));
                            }),
                        ));
                    }
                    if known {
                        f.spawn(t.text(
                            tr("find-shop-in-contacts"),
                            t.font_sm,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ));
                    }
                }
                if !mine && !known {
                    let card = signed.clone();
                    let mut add = Button::new("market-shop-add")
                        .label(tr("find-shop-add"))
                        .icon("plus");
                    if !makes_parts {
                        add = add.primary();
                    }
                    f.spawn((
                        add.build(t),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            let card = card.clone();
                            commands.queue(move |w: &mut World| {
                                let (name, account) = (card.value.name.clone(), card.value.account);
                                crate::social::add_contact(w, card);
                                crate::linked::toast(
                                    w,
                                    tr_with("social-added", &[("name", &name)]),
                                );
                                open_shop(w, account);
                            });
                        }),
                    ));
                }
                let link = cadrs_social::contact::to_link(&signed);
                f.spawn((
                    Button::new("market-shop-copy")
                        .label(tr("market-shop-copy"))
                        .icon("copy")
                        .tooltip(tr("market-shop-copy-tooltip"))
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        let link = link.clone();
                        commands.queue(move |w: &mut World| {
                            if let Some(mut clip) =
                                w.get_resource_mut::<bevy::clipboard::Clipboard>()
                            {
                                let _ = clip.set_text(link);
                            }
                            crate::linked::toast(w, tr("market-shop-copied"));
                        });
                    }),
                ));
                f.spawn((
                    Button::new("market-shop-close")
                        .label(tr("common-close"))
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(close_shop)),
                ));
            })
            .build(&theme),
        ShopDialog,
        DespawnOnExit(state),
    ));
    world.flush();
}

/// The most designs a shop's page lists.
const MAX_DESIGNS: usize = 12;

/// **Request a quote** on a shop's page: the shop joins the contacts and is the one the next
/// request starts on. With a Part Studio open the request's dialog opens at once; from the
/// documents page there are no parts to ask about yet, and it says what comes next.
fn request_quote(world: &mut World, card: cadrs_social::keys::Signed<Card>) {
    let (name, account) = (card.value.name.clone(), card.value.account);
    crate::social::add_contact(world, card);
    world.insert_resource(crate::request_quote::PreferredShop(Some(account)));
    close_shop(world);
    if *world.resource::<State<AppState>>().get() == AppState::Document {
        crate::request_quote::open(world);
    } else {
        crate::linked::toast(world, tr_with("market-shop-quote-next", &[("name", &name)]));
    }
}

/// A machine's bed or envelope: "3000 × 1500 mm", with a height for a machine that has one,
/// and one length for a machine that has only that (a press brake).
fn envelope(e: [f64; 3]) -> String {
    let sides: Vec<String> = e
        .iter()
        .filter(|side| **side > 0.0)
        .map(|side| format!("{side:.0}"))
        .collect();
    if sides.is_empty() {
        String::new()
    } else {
        format!("{} mm", sides.join(" × "))
    }
}

fn shop_page(
    b: &mut ChildSpawner,
    t: &Theme,
    listing: &ShopListing,
    logo: Option<Handle<Image>>,
    designs: &[(cadrs_net::public::Key, String)],
) {
    let heading = |b: &mut ChildSpawner, key: &str| {
        b.spawn(t.text(tr(key), t.font_sm, FontWeight::BOLD, t.foreground));
    };
    let paragraph = |b: &mut ChildSpawner, name: &str, text: String, color: Color| {
        b.spawn((
            Name::new(name.to_owned()),
            t.paragraph_within(text, t.font_base, color, 580.0),
        ));
    };
    b.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(16.0),
        ..default()
    })
    .with_children(|head| {
        head.spawn(crate::org_logo::logo(
            "market-shop-logo",
            t,
            logo,
            listing.name(),
            96.0,
        ));
        head.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            flex_shrink: 1.0,
            min_width: Val::Px(0.0),
            ..default()
        })
        .with_children(|who| {
            who.spawn((
                Name::new("market-shop-tagline"),
                t.paragraph_within(listing.tagline.clone(), t.font_md, t.foreground, 460.0),
            ));
            let mut line = Vec::new();
            if listing.kinds.manufacturing {
                line.push(tr("org-offers-manufacturing"));
            }
            if listing.kinds.designs {
                line.push(tr("org-offers-designs"));
            }
            who.spawn(t.text(
                line.join(" · "),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
        });
    });
    if !listing.about.is_empty() {
        paragraph(b, "market-shop-about", listing.about.clone(), t.foreground);
    }
    if !listing.offers.is_empty() {
        heading(b, "market-shop-makes");
        for (i, offer) in listing.offers.iter().enumerate() {
            let materials: Vec<String> = offer
                .materials
                .iter()
                .map(|(material, mm)| cadrs_org::capability::material_label(material, *mm))
                .collect();
            let mut line = crate::organization::process_name(offer.process);
            let bed = envelope(offer.envelope);
            if !bed.is_empty() {
                line.push_str(" · ");
                line.push_str(&bed);
            }
            if !materials.is_empty() {
                line.push_str(" · ");
                line.push_str(&materials.join(", "));
            }
            paragraph(b, &format!("market-shop-offer-{i}"), line, t.foreground);
        }
    }
    let places: Vec<String> = listing
        .places
        .iter()
        .filter_map(|p| p.region.as_ref())
        .map(|r| r.label())
        .filter(|l| !l.is_empty())
        .collect();
    if !places.is_empty() {
        heading(b, "org-tab-locations");
        for (i, place) in places.into_iter().enumerate() {
            paragraph(b, &format!("market-shop-place-{i}"), place, t.foreground);
        }
    }
    if !designs.is_empty() {
        heading(b, "market-designs");
        b.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|list| {
            for (i, (key, name)) in designs.iter().enumerate() {
                let key = key.clone();
                list.spawn((
                    Button::new(format!("market-shop-design-{i}"))
                        .label(name.clone())
                        .icon("public")
                        .small()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        let key = key.clone();
                        commands.queue(move |w: &mut World| {
                            close_shop(w);
                            crate::collab::public::view(w, &key);
                        });
                    }),
                ));
            }
        });
    }
    heading(b, "market-shop-contact");
    let mut contact = vec![listing.email.clone()];
    contact.extend(listing.website.clone());
    contact.retain(|c| !c.is_empty());
    b.spawn(
        cadrs_ui::selectable_text::SelectableText::new("market-shop-contact")
            .value(contact.join("\n"))
            .font(t.font_base, FontWeight::NORMAL)
            .build(t),
    );
    if listing.kinds.manufacturing {
        paragraph(
            b,
            "market-shop-quote-help",
            tr("market-shop-quote-help"),
            t.muted_foreground,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_org::capability::Process;
    use cadrs_org::directory::Offer;
    use cadrs_social::keys::Account;
    use cadrs_social::place::{Place, Region};

    fn listing(offers: Vec<Offer>, designs: bool, city: Option<&str>) -> ShopListing {
        let shop = Account::generate();
        let card = Card {
            account: shop.id(),
            kind: cadrs_social::contact::Kind::Organization,
            name: "Cape Cutters".into(),
            devices: vec![],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(shop.key());
        let mut listing = ShopListing::withdrawal(card, 1);
        listing.withdrawn = false;
        listing.kinds.designs = designs;
        listing.kinds.manufacturing = !offers.is_empty();
        listing.offers = offers;
        listing.places = city
            .map(|city| Place {
                region: Some(Region {
                    city: Some(city.into()),
                    area: None,
                    country: Some("South Africa".into()),
                }),
                ..Place::earth(-33.9, 18.9)
            })
            .into_iter()
            .collect();
        listing
    }

    fn offer(process: Process) -> Offer {
        Offer {
            process,
            envelope: [3000.0, 1500.0, 0.0],
            materials: vec![],
        }
    }

    #[test]
    fn a_shop_is_summed_up_in_a_line() {
        let jets = listing(
            vec![offer(Process::Waterjet), offer(Process::Waterjet)],
            false,
            Some("Stellenbosch"),
        );
        // Each process once, then where.
        let line = summary(&jets);
        assert_eq!(line.matches(" · ").count(), 1, "{line}");
        assert!(line.ends_with("Stellenbosch, South Africa"), "{line}");
        // One that only publishes designs has no machines and no place, and still says something.
        let designs = listing(vec![], true, None);
        assert!(!summary(&designs).is_empty());
        assert_eq!(envelope([3000.0, 1500.0, 0.0]), "3000 × 1500 mm");
        assert_eq!(envelope([300.0, 200.0, 150.0]), "300 × 200 × 150 mm");
        assert_eq!(envelope([3000.0, 0.0, 0.0]), "3000 mm");
        assert_eq!(envelope([0.0; 3]), "");
    }
}
