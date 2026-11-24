//! The organization screen's own-affairs modules: the Overview (who it is, whether it is in the
//! marketplace and what it still lacks to be, what waits to be done), its Profile (name, logo,
//! what it offers) and its Locations.
//!
//! An organization has no place of its own: it has locations, none for one that only sells
//! designs. Each has an address, says whether things are made or collected there, and may be
//! shown in the marketplace (never more precisely than a kilometre).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_org::profile::{Missing, ProfileEdit, missing};
use cadrs_org::quality::IssueStatus;
use cadrs_org::{Address, Location, Offers, Op, Profile, Role, Stage, new_id};
use cadrs_ui::checkbox::{Checkbox, CheckboxState};
use cadrs_ui::dialog::Dialog;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::TextInput;
use cadrs_ui::switch::{Switch, SwitchChange};
use cadrs_ui::theme::Theme;
use cadrs_ui::{Button, Column};

use crate::AppState;
use crate::organization::{
    ModuleView, OpenOrg, OrgDialog, OrgTab, ask_remove, cell_button, close_dialogs, dialog_buttons,
    field, input, label, muted, number, page_head, row, status_tag, subheading, switch_on, t_text,
    table, text_row, write,
};

fn checked(world: &mut World, name: &str) -> bool {
    let mut q = world.query::<(&Name, &CheckboxState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .is_some_and(|(_, s)| s.checked)
}

/// A button that shows module `tab`.
fn go(b: &mut ChildSpawnerCommands, t: &Theme, name: impl Into<String>, text: String, tab: OrgTab) {
    b.spawn((
        Button::new(name.into())
            .label(text)
            .small()
            .ghost()
            .build(t),
        observe(move |_: On<Activate>, mut open: ResMut<OpenOrg>| open.show(tab)),
    ));
}

/// What a listing needs, each with the module where it is set and whether it is there.
fn checklist(v: &ModuleView) -> Vec<(Missing, &'static str, OrgTab, bool)> {
    let lacking = missing(&v.org);
    let mut all = vec![
        (Missing::Logo, "org-need-logo", OrgTab::Profile),
        (Missing::Tagline, "org-need-tagline", OrgTab::Profile),
        (Missing::Offers, "org-need-offers", OrgTab::Profile),
        (Missing::Email, "org-need-email", OrgTab::Profile),
    ];
    if v.org.profile.offers.manufacturing {
        all.push((
            Missing::ProductionLocation,
            "org-need-location",
            OrgTab::Locations,
        ));
        all.push((Missing::Machine, "org-need-machine", OrgTab::Machines));
    }
    all.into_iter()
        .map(|(what, key, tab)| (what, key, tab, !lacking.contains(&what)))
        .collect()
}

const GREEN: Color = Color::srgb(0.18, 0.56, 0.36);

/// What the organization offers, as tags.
fn offer_tags(b: &mut ChildSpawnerCommands, t: &Theme, offers: Offers) {
    for (on, name, key) in [
        (
            offers.manufacturing,
            "org-offers-manufacturing-tag",
            "org-offers-manufacturing",
        ),
        (
            offers.designs,
            "org-offers-designs-tag",
            "org-offers-designs",
        ),
    ] {
        if on {
            b.spawn(status_tag(t, name.into(), tr(key), t.foreground));
        }
    }
}

pub(crate) fn overview(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    let profile = &org.profile;
    let admin =
        v.me.and_then(|me| org.members.get(&me))
            .is_some_and(|role| *role >= Role::Admin);
    // Who it is.
    b.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(16.0),
        margin: UiRect::bottom(Val::Px(8.0)),
        ..default()
    })
    .with_children(|head| {
        head.spawn(crate::org_logo::logo(
            "org-overview-logo",
            t,
            v.logo.clone(),
            &profile.name,
            72.0,
        ));
        head.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|who| {
            who.spawn((
                Name::new("org-page-title"),
                t.text(
                    profile.name.clone(),
                    t.font_xl,
                    FontWeight::BOLD,
                    t.foreground,
                ),
            ));
            if !profile.tagline.is_empty() {
                who.spawn((
                    Name::new("org-overview-tagline"),
                    t.text(
                        profile.tagline.clone(),
                        t.font_base,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                ));
            }
            row(who, |tags| {
                offer_tags(tags, t, profile.offers);
                if let Some((stars, n)) = org.rating() {
                    tags.spawn(t.text(
                        format!("★ {stars:.1} ({n})"),
                        t.font_sm,
                        FontWeight::MEDIUM,
                        t.foreground,
                    ));
                }
            });
        });
    });

    // In the marketplace, or what it still lacks to be.
    subheading(b, t, "org-marketplace");
    let needs = checklist(v);
    let complete = needs.iter().all(|(.., done)| *done);
    row(b, |r| {
        let (name, key, color) = if org.listed() {
            ("org-listing-listed", "org-listing-listed", GREEN)
        } else if profile.listed {
            ("org-listing-waiting", "org-listing-waiting", t.link)
        } else {
            (
                "org-listing-private",
                "org-listing-private",
                t.muted_foreground,
            )
        };
        r.spawn(status_tag(t, name.into(), tr(key), color));
        if admin {
            r.spawn((
                Switch::new("org-listed")
                    .label(tr("org-listed-switch"))
                    .on(profile.listed)
                    .build(t),
                observe(|ev: On<SwitchChange>, mut commands: Commands| {
                    let on = ev.on;
                    commands.queue(move |w: &mut World| set_listed(w, on));
                }),
            ));
        }
    });
    muted(
        b,
        t,
        tr(if org.listed() {
            "org-listing-listed-about"
        } else if complete {
            "org-listing-ready-about"
        } else {
            "org-listing-needs-about"
        }),
    );
    if org.listed() && !v.holds_key {
        muted(b, t, tr("org-listing-other-device"));
    }
    for (i, (_, key, tab, done)) in needs.iter().enumerate() {
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-need-{i}")),
                cadrs_ui::icon(
                    if *done { "check" } else { "remove-circle" },
                    14.0,
                    if *done { GREEN } else { t.muted_foreground },
                ),
            ));
            r.spawn(t.text(
                tr(key),
                t.font_base,
                FontWeight::NORMAL,
                if *done {
                    t.muted_foreground
                } else {
                    t.foreground
                },
            ));
            if !*done && admin {
                go(r, t, format!("org-need-go-{i}"), tr("org-need-go"), *tab);
            }
        });
    }

    // What waits.
    subheading(b, t, "org-at-a-glance");
    let count = |stages: &[Stage]| {
        org.requests
            .values()
            .filter(|r| stages.contains(&r.stage))
            .count()
    };
    let open_issues = org
        .issues
        .values()
        .filter(|i| i.status != IssueStatus::Closed)
        .count();
    let tiles = [
        (
            "requests",
            "org-glance-requests",
            count(&[Stage::Requested]),
            OrgTab::Requests,
        ),
        (
            "orders",
            "org-glance-orders",
            count(&[Stage::Accepted, Stage::InProduction, Stage::Ready]),
            OrgTab::Requests,
        ),
        ("issues", "org-glance-issues", open_issues, OrgTab::Issues),
        (
            "machines",
            "org-tab-machines",
            org.machines.len(),
            OrgTab::Machines,
        ),
        (
            "locations",
            "org-tab-locations",
            org.locations.len(),
            OrgTab::Locations,
        ),
        (
            "members",
            "org-tab-members",
            org.members.len(),
            OrgTab::Members,
        ),
    ];
    b.spawn(Node {
        column_gap: Val::Px(10.0),
        row_gap: Val::Px(10.0),
        flex_wrap: FlexWrap::Wrap,
        ..default()
    })
    .with_children(|grid| {
        for (key, label_key, n, tab) in tiles {
            grid.spawn((
                Name::new(format!("org-glance-{key}")),
                bevy::ui_widgets::Button,
                bevy::picking::hover::Hovered::default(),
                Node {
                    width: Val::Px(132.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius)),
                    ..default()
                },
                BorderColor::all(t.border),
                observe(move |_: On<Activate>, mut open: ResMut<OpenOrg>| open.show(tab)),
            ))
            .with_children(|tile| {
                tile.spawn((
                    Name::new(format!("org-glance-{key}-count")),
                    t.text(n.to_string(), t.font_xl, FontWeight::BOLD, t.foreground),
                    Pickable::IGNORE,
                ));
                tile.spawn((
                    t.text(
                        tr(label_key),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Pickable::IGNORE,
                ));
            });
        }
    });

    // What is in the wallet, and what came in this month: a link to the Wallet.
    if let Some(statement) = v.wallet.as_ref().and_then(|w| w.statement.as_ref()) {
        let received = v.wallet.as_ref().map_or(0, |w| w.received_this_month());
        let mut line = tr_with(
            "org-overview-wallet",
            &[
                ("amount", &cadrs_credits::show(statement.balance)),
                ("received", &cadrs_credits::show(received)),
            ],
        );
        if statement.held_in > 0 {
            line.push_str(" · ");
            line.push_str(&tr_with(
                "wallet-held-in",
                &[("amount", &cadrs_credits::show(statement.held_in))],
            ));
        }
        subheading(b, t, "org-tab-wallet");
        go(b, t, "org-overview-wallet", line, OrgTab::Wallet);
    }

    if let Some((link, qr)) = &v.share {
        subheading(b, t, "org-share");
        muted(b, t, tr("org-share-help"));
        if let Some(qr) = qr {
            b.spawn((
                Name::new("org-share-qr"),
                ImageNode::new(qr.clone()),
                Node {
                    width: Val::Px(180.0),
                    height: Val::Px(180.0),
                    ..default()
                },
            ));
        }
        let copied = link.clone();
        row(b, |r| {
            r.spawn((
                Button::new("org-share-copy")
                    .label(tr("market-shop-copy"))
                    .icon("copy")
                    .build(t),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    let link = copied.clone();
                    commands.queue(move |w: &mut World| {
                        if let Some(mut clip) = w.get_resource_mut::<bevy::clipboard::Clipboard>() {
                            let _ = clip.set_text(link);
                        }
                        crate::linked::toast(w, tr("org-share-copied"));
                    });
                }),
            ));
            // The link is some hundreds of characters: its start says what it is.
            let start: String = link.chars().take(44).collect();
            r.spawn((
                Name::new("org-share-link"),
                t.text(
                    format!("{start}…"),
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
            ));
        });
    }
    if let Some(account) = org.org {
        muted(b, t, format!("{} {}", tr("org-id"), account));
    }
}

/// Asks for the organization to be shown in the marketplace, or not to be. Only that is
/// written: someone changing the tagline at the same moment keeps theirs.
fn set_listed(world: &mut World, listed: bool) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    if open.session.org().profile.listed != listed {
        write(
            world,
            Op::EditProfile(ProfileEdit {
                listed: Some(listed),
                ..Default::default()
            }),
        );
    }
}

pub(crate) fn profile(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let p = &v.org.profile;
    page_head(b, t, "org-tab-profile", "org-profile-about");
    label(b, t, "org-logo");
    row(b, |r| {
        r.spawn(crate::org_logo::logo(
            "org-profile-logo",
            t,
            v.logo.clone(),
            &p.name,
            96.0,
        ));
        r.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Button::new("org-logo-change")
                    .label(tr("org-logo-choose"))
                    .icon("image")
                    .build(t),
                observe(|_: On<Activate>, mut commands: Commands| {
                    commands.queue(|w: &mut World| {
                        crate::org_create::pick_logo(w, crate::org_create::LOGO_FOR_PROFILE)
                    });
                }),
            ));
            muted(c, t, tr("org-logo-help"));
        });
    });
    label(b, t, "org-name-label");
    input(b, t, "org-profile-name", p.name.clone(), 360.0);
    label(b, t, "org-tagline");
    input(b, t, "org-profile-tagline", p.tagline.clone(), 620.0);
    muted(b, t, tr("org-tagline-help"));
    label(b, t, "org-about");
    b.spawn(
        TextInput::new("org-profile-about")
            .value(p.about.clone())
            .placeholder(tr("org-profile-about-placeholder"))
            .width(Val::Px(620.0))
            .lines(5)
            .max_characters(cadrs_org::profile::MAX_ABOUT)
            .build(t),
    );
    label(b, t, "org-offers");
    b.spawn(
        Checkbox::new("org-profile-manufacturing")
            .label(tr("org-offers-manufacturing-long"))
            .checked(p.offers.manufacturing)
            .build(t),
    );
    b.spawn(
        Checkbox::new("org-profile-designs")
            .label(tr("org-offers-designs-long"))
            .checked(p.offers.designs)
            .build(t),
    );
    label(b, t, "org-email");
    input(b, t, "org-profile-email", p.email.clone(), 360.0);
    label(b, t, "org-website");
    input(
        b,
        t,
        "org-profile-website",
        p.website.clone().unwrap_or_default(),
        360.0,
    );
    b.spawn((
        Button::new("org-profile-save")
            .label(tr("common-save"))
            .primary()
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(save_profile);
        }),
    ));

    // Leaving it behind on this device. The organization itself goes on with its other members.
    subheading(b, t, "org-remove");
    muted(b, t, tr("org-remove-about"));
    b.spawn((
        Button::new("org-remove")
            .label(tr("org-remove-ellipsis"))
            .icon("delete")
            .danger()
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(open_remove);
        }),
    ));
}

/// Asks before the organization is removed from this device.
fn open_remove(world: &mut World) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let name = open.session.org().profile.name.clone();
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-remove-dialog")
            .title(tr_with("org-remove-title", &[("name", &name)]))
            .width(520.0)
            .body(move |b| {
                b.spawn(tb.paragraph(tr("org-remove-confirm"), tb.font_base, tb.foreground));
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                // Says what it does, in the colour of something that isn't lightly undone.
                f.spawn((
                    Button::new("org-remove-ok")
                        .label(tr("common-remove"))
                        .danger()
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(crate::organization::remove_from_device);
                    }),
                ));
                f.spawn((
                    Button::new("org-remove-cancel")
                        .label(tr("common-cancel"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_dialogs);
                    }),
                ));
            })
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

/// What the Profile form holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ProfileForm {
    pub name: String,
    pub tagline: String,
    pub about: String,
    pub offers: Offers,
    pub email: String,
    pub website: String,
}

/// What saving the form changes: only the fields that differ from `baseline`, the profile the
/// form was made from. A field the person didn't touch still holds its old value; comparing
/// with the profile as it is now would write that old value over what someone else set since.
/// Why not, in the catalogs' words, when what was typed can't be a profile.
pub(crate) fn form_edit(baseline: &Profile, form: &ProfileForm) -> Result<ProfileEdit, String> {
    let typed = Profile {
        name: form.name.trim().to_owned(),
        tagline: form.tagline.trim().to_owned(),
        about: form.about.trim().to_owned(),
        offers: form.offers,
        email: form.email.trim().to_owned(),
        website: Some(form.website.trim().to_owned()).filter(|w| !w.is_empty()),
        ..baseline.clone()
    };
    // Said in the person's language before the document says it in its own.
    match profile_problem(&typed) {
        Some(why) => Err(why),
        None => Ok(ProfileEdit::between(baseline, &typed)),
    }
}

fn save_profile(world: &mut World) {
    let form = ProfileForm {
        name: field(world, "org-profile-name"),
        tagline: field(world, "org-profile-tagline"),
        about: field(world, "org-profile-about"),
        email: field(world, "org-profile-email"),
        website: field(world, "org-profile-website"),
        offers: Offers {
            manufacturing: checked(world, "org-profile-manufacturing"),
            designs: checked(world, "org-profile-designs"),
        },
    };
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let baseline = world
        .get_resource::<crate::organization::OrgForm>()
        .and_then(|made| made.profile.clone())
        .unwrap_or_else(|| open.session.org().profile.clone());
    match form_edit(&baseline, &form) {
        Err(why) => crate::linked::toast(world, why),
        Ok(edit) => {
            if edit.is_empty() || write(world, Op::EditProfile(edit)) {
                crate::linked::toast(world, tr("org-saved"));
            }
        }
    }
}

/// Why a profile can't be saved, in the catalogs' words.
pub(crate) fn profile_problem(profile: &Profile) -> Option<String> {
    use cadrs_org::profile::{MAX_NAME, MAX_TAGLINE, MIN_NAME, plausible_email};
    let name = profile.name.trim().chars().count();
    if !(MIN_NAME..=MAX_NAME).contains(&name) {
        return Some(tr("org-name-length"));
    }
    if profile.tagline.chars().count() > MAX_TAGLINE {
        return Some(tr("org-tagline-length"));
    }
    if !profile.email.is_empty() && !plausible_email(&profile.email) {
        return Some(tr("org-email-bad"));
    }
    profile.problem().map(str::to_owned)
}

/// The location the dialog edits; none for a new one.
#[derive(Resource)]
struct EditingLocation(Option<cadrs_org::Id>);

pub(crate) fn locations(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-locations", "org-locations-about");
    let columns = vec![
        Column::new("name", tr("org-location-name")).width(180.0),
        Column::new("use", tr("org-location-use")).width(170.0),
        Column::new("where", tr("org-location-where")).width(260.0),
        Column::new("public", tr("org-location-public")).width(150.0),
        Column::new("machines", tr("org-tab-machines")).width(90.0),
        Column::new("actions", "").width(150.0),
    ];
    let rows = org
        .locations
        .iter()
        .enumerate()
        .map(|(i, (id, l))| {
            let id = *id;
            let uses: Vec<String> = [
                (l.production, "org-location-production"),
                (l.pickup, "org-location-pickup"),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, key)| tr(key))
            .collect();
            let public = if l.public && l.point.is_none() {
                // No point was given: the marketplace has only the city to show.
                tr("org-location-public-city")
            } else if l.public {
                tr_with(
                    "org-location-public-within",
                    &[(
                        "km",
                        &format!(
                            "{:.0}",
                            l.public_accuracy_m
                                .max(cadrs_org::profile::MIN_PUBLIC_ACCURACY_M)
                                / 1000.0
                        ),
                    )],
                )
            } else {
                tr("org-location-private")
            };
            let machines = org
                .machines
                .values()
                .filter(|m| m.location == Some(id))
                .count();
            text_row(
                t,
                format!("org-location-{i}"),
                &columns,
                [
                    l.name.clone(),
                    if uses.is_empty() {
                        "—".into()
                    } else {
                        uses.join(", ")
                    },
                    l.address.short(),
                    public,
                    machines.to_string(),
                ],
            )
            .cell(move |c| {
                cell_button(
                    c,
                    format!("org-location-edit-{i}"),
                    tr("common-edit"),
                    false,
                    move |w| open_location(w, Some(id)),
                );
                cell_button(
                    c,
                    format!("org-location-remove-{i}"),
                    tr("common-remove"),
                    false,
                    move |w| ask_remove(w, Op::SetLocation { id, location: None }),
                );
            })
        })
        .collect();
    table(b, t, "org-locations", columns, rows, "org-no-locations");
    b.spawn((
        Button::new("org-add-location")
            .label(tr("org-add-location"))
            .icon("plus")
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(|w: &mut World| open_location(w, None));
        }),
    ));
    if let Some(map) = &v.map {
        b.spawn((
            Name::new("org-locations-map"),
            ImageNode::new(map.clone()),
            Node {
                width: Val::Px(MAP_SIZE.0 as f32),
                height: Val::Px(MAP_SIZE.1 as f32),
                max_width: Val::Percent(100.0),
                margin: UiRect::top(Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
        ));
    }
}

/// The size of the map under the locations.
pub(crate) const MAP_SIZE: (u32, u32) = (720, 360);

/// A map of the world with the organization's locations that say where they are; none when
/// none does.
pub(crate) fn locations_map(org: &cadrs_org::Org) -> Option<Image> {
    let pins: Vec<cadrs_ui::world_map::Pin> = org
        .locations
        .values()
        .filter_map(|l| {
            let (lat, lon) = l.point?;
            Some(cadrs_ui::world_map::Pin {
                lat,
                lon,
                color: Color::srgb_u8(0x1f, 0x5f, 0xbf),
                accuracy_m: 0.0,
            })
        })
        .collect();
    (!pins.is_empty()).then(|| {
        cadrs_ui::world_map::image(
            MAP_SIZE.0,
            MAP_SIZE.1,
            Color::srgb_u8(0xe8, 0xee, 0xf5),
            Color::srgb_u8(0x55, 0x60, 0x70),
            &pins,
        )
    })
}

/// The Add location dialog, or the same for location `id` with what it has.
fn open_location(world: &mut World, id: Option<cadrs_org::Id>) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let location = id
        .and_then(|id| open.session.org().locations.get(&id))
        .cloned()
        .unwrap_or_else(|| Location {
            // A first location is where things are made, and shown.
            production: true,
            public: true,
            public_accuracy_m: 5_000.0,
            ..Default::default()
        });
    world.insert_resource(EditingLocation(id));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    let title = tr(if id.is_some() {
        "org-edit-location"
    } else {
        "org-add-location"
    });
    world.spawn((
        Dialog::new("org-location-dialog")
            .title(title)
            .width(620.0)
            .body(move |b| {
                let t = &tb;
                let l = &location;
                let text = |b: &mut ChildSpawner, name: &'static str, value: &str, width: f32| {
                    let placeholder = tr(&format!("{name}-placeholder"));
                    let mut input = TextInput::new(name).value(value).width(Val::Px(width));
                    if !placeholder.ends_with("-placeholder") {
                        input = input.placeholder(placeholder);
                    }
                    b.spawn(input.build(t));
                };
                b.spawn(t_text(t, "org-location-name"));
                b.spawn(
                    TextInput::new("org-location-name")
                        .value(l.name.clone())
                        .placeholder(tr("org-location-name-placeholder"))
                        .autofocus()
                        .build(t),
                );
                b.spawn(
                    Checkbox::new("org-location-production")
                        .label(tr("org-location-production-long"))
                        .checked(l.production)
                        .build(t),
                );
                b.spawn(
                    Checkbox::new("org-location-pickup")
                        .label(tr("org-location-pickup-long"))
                        .checked(l.pickup)
                        .build(t),
                );
                b.spawn(t_text(t, "org-location-address"));
                text(b, "org-location-line1", &l.address.line1, 560.0);
                text(b, "org-location-line2", &l.address.line2, 560.0);
                b.spawn(Node {
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|r| {
                    text(r, "org-location-city", &l.address.city, 200.0);
                    text(r, "org-location-region", &l.address.region, 180.0);
                    text(r, "org-location-postal", &l.address.postal_code, 120.0);
                });
                b.spawn(t_text(t, "org-location-country"));
                let country = cadrs_org::country::name(&l.address.country).unwrap_or("");
                text(b, "org-location-country", country, 280.0);
                b.spawn(t_text(t, "org-location-phone"));
                text(b, "org-location-phone", &l.phone, 280.0);
                b.spawn(t_text(t, "org-location-point"));
                let (lat, lon) = l
                    .point
                    .map_or((String::new(), String::new()), |(lat, lon)| {
                        (lat.to_string(), lon.to_string())
                    });
                b.spawn(Node {
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|r| {
                    text(r, "org-location-lat", &lat, 160.0);
                    text(r, "org-location-lon", &lon, 160.0);
                });
                b.spawn((t.paragraph_within(
                    tr("org-location-point-help"),
                    t.font_sm,
                    t.muted_foreground,
                    560.0,
                ),));
                b.spawn(
                    Switch::new("org-location-public")
                        .label(tr("org-location-public-switch"))
                        .on(l.public)
                        .build(t),
                );
                b.spawn(Node {
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|r| {
                    r.spawn(t_text(t, "org-location-accuracy"));
                    text(
                        r,
                        "org-location-accuracy",
                        &format!(
                            "{}",
                            l.public_accuracy_m
                                .max(cadrs_org::profile::MIN_PUBLIC_ACCURACY_M)
                                / 1000.0
                        ),
                        90.0,
                    );
                });
            })
            .footer(|f| dialog_buttons(f, "org-location", save_location))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

/// A location from what a person typed; why not, in the catalogs' words, when it can't be one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn location_from(
    name: &str,
    production: bool,
    pickup: bool,
    address: Address,
    country: &str,
    lat: &str,
    lon: &str,
    phone: &str,
    public: bool,
    accuracy_km: Option<f64>,
) -> Result<Location, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tr("org-location-needs-name"));
    }
    let country = match country.trim() {
        "" => String::new(),
        typed => cadrs_org::country::find(typed)
            .ok_or_else(|| tr_with("org-location-country-unknown", &[("text", typed)]))?
            .to_owned(),
    };
    let coordinate = |text: &str| text.trim().replace(',', ".").parse::<f64>().ok();
    let point = match (lat.trim(), lon.trim()) {
        ("", "") => None,
        _ => match (coordinate(lat), coordinate(lon)) {
            (Some(lat), Some(lon))
                if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) =>
            {
                Some((lat, lon))
            }
            _ => return Err(tr("org-location-point-bad")),
        },
    };
    let location = Location {
        name: name.to_owned(),
        production,
        pickup,
        address: Address { country, ..address },
        point,
        phone: phone.trim().to_owned(),
        public,
        public_accuracy_m: accuracy_km.unwrap_or(5.0).max(1.0) * 1000.0,
    };
    match location.problem() {
        Some(why) => Err(why.to_owned()),
        None => Ok(location),
    }
}

fn save_location(world: &mut World) {
    let Some(EditingLocation(editing)) = world.remove_resource::<EditingLocation>() else {
        return;
    };
    let text = |world: &mut World, name: &str| field(world, name).trim().to_owned();
    let address = Address {
        line1: text(world, "org-location-line1"),
        line2: text(world, "org-location-line2"),
        city: text(world, "org-location-city"),
        region: text(world, "org-location-region"),
        postal_code: text(world, "org-location-postal"),
        country: String::new(),
    };
    let made = location_from(
        &field(world, "org-location-name"),
        checked(world, "org-location-production"),
        checked(world, "org-location-pickup"),
        address,
        &field(world, "org-location-country"),
        &field(world, "org-location-lat"),
        &field(world, "org-location-lon"),
        &field(world, "org-location-phone"),
        switch_on(world, "org-location-public"),
        number(world, "org-location-accuracy"),
    );
    let location = match made {
        Ok(location) => location,
        Err(why) => {
            crate::linked::toast(world, why);
            world.insert_resource(EditingLocation(editing));
            return;
        }
    };
    let op = Op::SetLocation {
        id: editing.unwrap_or_else(new_id),
        location: Some(location),
    };
    if write(world, op) {
        close_dialogs(world);
    } else {
        world.insert_resource(EditingLocation(editing));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One admin has the Profile open with something typed; another changes the tagline, which
    /// arrives but isn't shown in the first one's form (it holds unsaved input). Saving writes
    /// only what was typed: the other's tagline stands.
    #[test]
    fn saving_a_form_made_earlier_keeps_what_others_changed_since() {
        let baseline = Profile {
            name: "Cape Waterjet".into(),
            tagline: "Waterjet cutting".into(),
            email: "sales@shop.example".into(),
            offers: Offers {
                manufacturing: true,
                designs: false,
            },
            ..Default::default()
        };
        // The form as it was made, with an About typed into it.
        let form = ProfileForm {
            name: baseline.name.clone(),
            tagline: baseline.tagline.clone(),
            about: " Two 3 x 1.5 m tables. ".into(),
            offers: baseline.offers,
            email: baseline.email.clone(),
            website: String::new(),
        };
        let edit = form_edit(&baseline, &form).unwrap();
        assert_eq!(
            edit,
            ProfileEdit {
                about: Some("Two 3 x 1.5 m tables.".into()),
                ..Default::default()
            }
        );
        // Meanwhile the profile moved on.
        let mut now = Profile {
            tagline: "Waterjet and laser cutting".into(),
            listed: true,
            ..baseline.clone()
        };
        edit.apply(&mut now);
        assert_eq!(now.tagline, "Waterjet and laser cutting");
        assert!(now.listed);
        assert_eq!(now.about, "Two 3 x 1.5 m tables.");
        // Nothing typed: nothing written. What can't be a profile is said, not written.
        let untouched = ProfileForm {
            about: String::new(),
            ..form.clone()
        };
        assert!(form_edit(&baseline, &untouched).unwrap().is_empty());
        let bad = ProfileForm {
            email: "sales".into(),
            ..form
        };
        assert!(form_edit(&baseline, &bad).is_err());
    }

    fn typed(country: &str, lat: &str, lon: &str) -> Result<Location, String> {
        location_from(
            " Workshop ",
            true,
            false,
            Address {
                city: "Stellenbosch".into(),
                ..Default::default()
            },
            country,
            lat,
            lon,
            "",
            true,
            Some(0.2),
        )
    }

    #[test]
    fn a_location_is_read_from_what_was_typed() {
        let l = typed("south africa", "-33,9357", "18.8611").unwrap();
        assert_eq!(l.name, "Workshop");
        assert_eq!(l.address.country, "ZA");
        assert_eq!(l.point, Some((-33.9357, 18.8611)));
        // Never shown more precisely than a kilometre.
        assert_eq!(l.public_accuracy_m, 1_000.0);
        // No country and no point is a location all the same.
        let bare = typed("", "", "").unwrap();
        assert_eq!((bare.address.country.as_str(), bare.point), ("", None));
        // What can't be read is said, not guessed.
        assert!(typed("Atlantis", "", "").is_err());
        assert!(typed("ZA", "-33.9", "").is_err());
        assert!(typed("ZA", "95", "18").is_err());
        assert!(
            location_from(
                "  ",
                true,
                false,
                Address::default(),
                "",
                "",
                "",
                "",
                false,
                None
            )
            .is_err()
        );
    }
}
