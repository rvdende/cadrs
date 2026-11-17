//! Find a shop (the documents page's app menu): the shops heard of in the public room, on a
//! world map and in a list, nearest first, filtered by what they cut and how thick. Adding one to
//! the contacts makes it a choice in Request a quote….

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_social::place::{Frame, Place};
use cadrs_ui::Button;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::theme::Theme;
use cadrs_ui::world_map::{self, Pin};

use crate::social::Social;

#[derive(Component)]
struct FindDialog;

/// The search the dialog shows.
#[derive(Resource, Clone, Default)]
struct Search {
    material: String,
    thickness: String,
}

/// A shop in the results: its name, what it does, how far, and its card.
struct Row {
    name: String,
    what: String,
    distance: String,
    card: cadrs_social::keys::Signed<cadrs_social::contact::Card>,
    known: bool,
}

fn field(world: &mut World, name: &str) -> String {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

fn surface(place: &Place) -> Option<(f64, f64)> {
    match place.frame {
        Some(Frame::Surface { lat, lon, .. }) => Some((lat, lon)),
        _ => None,
    }
}

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<FindDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

/// Opens (or refreshes) Find a shop.
pub fn open(world: &mut World) {
    crate::social::ensure_loaded(world);
    let search = world.get_resource::<Search>().cloned().unwrap_or_default();
    let social = world.resource::<Social>();
    let me = social.my_place.clone();
    let thickness = search
        .thickness
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok();
    let material = (!search.material.trim().is_empty()).then(|| search.material.trim().to_owned());
    let found = social
        .directory
        .search(me.as_ref(), None, material.as_deref(), thickness);
    let theme = world.resource::<Theme>().clone();
    let mut pins: Vec<Pin> = found
        .iter()
        .filter_map(|f| {
            let place = f.listing.place.as_ref()?;
            let (lat, lon) = surface(place)?;
            Some(Pin {
                lat,
                lon,
                color: theme.primary,
                accuracy_m: place.accuracy_m.unwrap_or(0.0),
            })
        })
        .collect();
    if let Some((lat, lon)) = me.as_ref().and_then(surface) {
        pins.push(Pin {
            lat,
            lon,
            color: Color::srgb(0.85, 0.2, 0.2),
            accuracy_m: 0.0,
        });
    }
    let rows: Vec<Row> = found
        .iter()
        .map(|f| {
            let l = f.listing;
            let mut what: Vec<String> = l
                .offers
                .iter()
                .map(|o| {
                    let thickest = o.materials.iter().map(|m| m.1).fold(0.0, f64::max);
                    format!("{} ≤ {thickest} mm", o.process.label())
                })
                .collect();
            what.dedup();
            if let Some(region) = l.place.as_ref().and_then(|p| p.region.as_ref()) {
                what.insert(0, region.label());
            }
            if let Some((stars, n)) = l.rating {
                what.push(format!("★ {stars:.1} ({n})"));
            }
            Row {
                name: l.name().to_owned(),
                what: what.join(" · "),
                distance: f
                    .distance_m
                    .map(|d| format!("{:.0} km", d / 1000.0))
                    .unwrap_or_default(),
                card: l.card.clone(),
                known: social.contacts.get(&l.account()).is_some(),
            }
        })
        .collect();
    let (lat, lon) = me
        .as_ref()
        .and_then(surface)
        .map_or((String::new(), String::new()), |(a, b)| {
            (a.to_string(), b.to_string())
        });
    let map = world.get_resource_mut::<Assets<Image>>().map(|mut images| {
        images.add(world_map::image(
            720,
            360,
            theme.background,
            theme.muted_foreground,
            &pins,
        ))
    });
    close(world);
    let t = theme.clone();
    let tb = theme.clone();
    let count = rows.len();
    world.spawn((
        Dialog::new("find-shop-dialog")
            .title(tr("find-shop-title"))
            .width(760.0)
            .body(move |b| {
                let t = &tb;
                if let Some(map) = map {
                    b.spawn((
                        Name::new("find-shop-map"),
                        ImageNode::new(map),
                        Node {
                            width: Val::Px(720.0),
                            height: Val::Px(360.0),
                            max_width: Val::Percent(100.0),
                            ..default()
                        },
                    ));
                }
                b.spawn(Node {
                    column_gap: Val::Px(8.0),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|r| {
                    for (name, value, width) in [
                        ("find-shop-material", search.material.clone(), 180.0),
                        ("find-shop-thickness", search.thickness.clone(), 110.0),
                        ("find-shop-lat", lat.clone(), 110.0),
                        ("find-shop-lon", lon.clone(), 110.0),
                    ] {
                        r.spawn(
                            TextInput::new(name)
                                .value(value)
                                .placeholder(tr(&format!("{name}-placeholder")))
                                .width(Val::Px(width))
                                .build(t),
                        );
                    }
                });
                b.spawn((
                    Name::new("find-shop-count"),
                    t.text(
                        tr_with("find-shop-count", &[("count", &count.to_string())]),
                        t.font_sm,
                        FontWeight::BOLD,
                        t.foreground,
                    ),
                ));
                if rows.is_empty() {
                    b.spawn(t.paragraph(tr("find-shop-none"), t.font_sm, t.muted_foreground));
                }
                for (i, row) in rows.into_iter().enumerate() {
                    b.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            Name::new(format!("find-shop-{i}")),
                            t.text(row.name, t.font_base, FontWeight::MEDIUM, t.foreground),
                        ));
                        r.spawn(t.text(row.distance, t.font_sm, FontWeight::BOLD, t.foreground));
                        r.spawn(t.text(
                            row.what,
                            t.font_sm,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ));
                        if row.known {
                            r.spawn(t.text(
                                tr("find-shop-in-contacts"),
                                t.font_sm,
                                FontWeight::NORMAL,
                                t.muted_foreground,
                            ));
                        } else {
                            let card = row.card;
                            r.spawn((
                                Button::new(format!("find-shop-add-{i}"))
                                    .label(tr("find-shop-add"))
                                    .small()
                                    .build(t),
                                observe(move |_: On<Activate>, mut commands: Commands| {
                                    let card = card.clone();
                                    commands.queue(move |w: &mut World| {
                                        let name = card.value.name.clone();
                                        crate::social::add_contact(w, card);
                                        crate::linked::toast(
                                            w,
                                            tr_with("social-added", &[("name", &name)]),
                                        );
                                        open(w);
                                    });
                                }),
                            ));
                        }
                    });
                }
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("find-shop-search")
                        .label(tr("find-shop-search"))
                        .primary()
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(search_again)),
                ));
                f.spawn((
                    Button::new("find-shop-close")
                        .label(tr("common-close"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(close)),
                ));
            })
            .build(&t),
        FindDialog,
        DespawnOnExit(crate::AppState::Landing),
    ));
}

/// Search again with what the fields say (and remember where the user is).
fn search_again(world: &mut World) {
    let material = field(world, "find-shop-material");
    let thickness = field(world, "find-shop-thickness");
    let number = |s: String| s.trim().replace(',', ".").parse::<f64>().ok();
    let lat = number(field(world, "find-shop-lat"));
    let lon = number(field(world, "find-shop-lon"));
    let place = match (lat, lon) {
        (Some(lat), Some(lon)) if (-90.0..=90.0).contains(&lat) => Some(Place::earth(lat, lon)),
        _ => None,
    };
    crate::social::set_my_place(world, place);
    world.insert_resource(Search {
        material,
        thickness,
    });
    open(world);
}
