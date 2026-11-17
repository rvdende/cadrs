//! Organization documents (shops): their own screen, with tabs, instead of the CAD view.
//!
//! An organization document is listed on the documents page like any document (its
//! `document.ron` has no tabs, and its metadata's `organization` marks it); its content is the
//! signed log `org.log` beside it (`cadrs_org`). Opening one shows [`AppState::Organization`]:
//! the top bar, the organization's name, and tabs for its overview, members, machines, stock
//! and requests for quotes. Every change goes through [`cadrs_org::session::Session::write`],
//! which refuses what wouldn't apply, then the log is saved.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, ScrollArea, observe};
use cadrs_core::library::OrgRef;
use cadrs_core::{Document, DocumentFile, DocumentId, DocumentMeta};
use cadrs_org::capability::{MachineCaps, MaterialLimit, Process};
use cadrs_org::quote::{Money, RateCard};
use cadrs_org::session::Session;
use cadrs_org::{Material, Op, Profile, Role, Stage, new_id};
use cadrs_social::keys::PublicKey;
use cadrs_social::keystore::DeviceKeys;
use cadrs_social::log::LogId;
use cadrs_social::place::{Place, Region};
use cadrs_ui::Button;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::tr;
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::switch::{Switch, SwitchState};
use cadrs_ui::tab_strip::{TabStrip, TabStripSelect, TabStripState};
use cadrs_ui::theme::Theme;

use crate::{AppState, DocumentStore, UserProfile};

pub struct OrganizationPlugin;

impl Plugin for OrganizationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Organization), spawn_screen)
            .add_systems(
                Update,
                refresh_body.run_if(in_state(AppState::Organization)),
            )
            .add_systems(OnExit(AppState::Organization), |mut c: Commands| {
                c.remove_resource::<OpenOrg>()
            });
    }
}

/// The organization's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OrgTab {
    #[default]
    Overview,
    Members,
    Machines,
    Stock,
    Prices,
    Requests,
    Jobs,
    Clients,
    Parts,
    Chat,
}

const TABS: [(OrgTab, &str); 10] = [
    (OrgTab::Overview, "org-tab-overview"),
    (OrgTab::Members, "org-tab-members"),
    (OrgTab::Machines, "org-tab-machines"),
    (OrgTab::Stock, "org-tab-stock"),
    (OrgTab::Prices, "org-tab-prices"),
    (OrgTab::Requests, "org-tab-requests"),
    (OrgTab::Jobs, "org-tab-jobs"),
    (OrgTab::Clients, "org-tab-clients"),
    (OrgTab::Parts, "org-tab-parts"),
    (OrgTab::Chat, "org-tab-chat"),
];

/// The open organization document.
#[derive(Resource)]
pub struct OpenOrg {
    pub id: DocumentId,
    pub session: Session,
    log_path: PathBuf,
    pub tab: OrgTab,
    /// The channel the Chat tab shows.
    pub channel: String,
}

#[derive(Component)]
struct OrgBody;

#[derive(Component)]
pub(crate) struct OrgDialog;

/// The key file of the identity in use, and its name.
pub(crate) fn identity_key(world: &World) -> (PathBuf, String) {
    let root = world.resource::<DocumentStore>().0.root().to_path_buf();
    match world.get_resource::<crate::identity::IdentityList>() {
        Some(list) => (list.0.key_path(&root), list.0.active().name.clone()),
        None => (
            root.join(cadrs_core::identity::DEFAULT_KEY),
            world.resource::<UserProfile>().display_name.clone(),
        ),
    }
}

fn device_keys(world: &World) -> Result<(DeviceKeys, PathBuf), String> {
    let (path, name) = identity_key(world);
    DeviceKeys::load_or_create(&path, &name, cadrs_social::wall_millis())
        .map(|keys| (keys, path))
        .map_err(|e| e.to_string())
}

/// Creates an organization document called `name` and opens it.
pub fn create(world: &mut World, name: &str) {
    let name = match name.trim() {
        "" => tr("org-untitled"),
        n => n.to_owned(),
    };
    let (keys, key_path) = match device_keys(world) {
        Ok(k) => k,
        Err(why) => {
            crate::linked::toast(world, format!("{}: {why}", tr("org-cannot-create")));
            return;
        }
    };
    let now = cadrs_social::wall_millis();
    let (session, org_account) = Session::found(keys.device.clone(), keys.cert.clone(), &name, now);
    let store = world.resource::<DocumentStore>().0.clone();
    let user = world.resource::<UserProfile>().id.clone();
    let doc = Document::empty(name.clone());
    let mut meta = DocumentMeta::new(&user, world.resource::<crate::AppClock>().now());
    meta.last_opened = meta.created.into();
    meta.organization = Some(OrgRef {
        log: session.log().id().0,
    });
    if let Err(e) = store.create(&doc, &meta) {
        crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-create")));
        return;
    }
    // The organization's own key: kept beside the identity's keys.
    let (org_key, _) = crate::social::org_files(&key_path, &org_account.id());
    let saved = cadrs_fs::create_dir_all(org_key.parent().expect("a folder"))
        .and_then(|()| cadrs_fs::write(&org_key, org_account.key().to_bytes()));
    if let Err(e) = saved {
        warn!("cannot keep the organization's key: {e}");
    }
    draw_picture(world, doc.id, &session.org().profile);
    let log_path = cadrs_org::file::path(&store.doc_dir(doc.id));
    if let Err(e) = cadrs_org::file::save(&log_path, session.log()) {
        crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-create")));
        return;
    }
    let log_id = session.log().id();
    crate::social::add_org(world, doc.id, &name, org_account.id(), log_id, Vec::new());
    world.insert_resource(OpenOrg {
        id: doc.id,
        session,
        log_path,
        tab: OrgTab::Overview,
        channel: "general".into(),
    });
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Organization);
}

/// Opens a stored organization document (`file`'s metadata marks it one).
pub fn open(world: &mut World, file: &DocumentFile) {
    let store = world.resource::<DocumentStore>().0.clone();
    let id = file.document.id;
    let log_path = cadrs_org::file::path(&store.doc_dir(id));
    let log = match cadrs_org::file::load(&log_path) {
        Ok((log, bad)) => {
            if bad > 0 {
                warn!("{bad} entries of {} didn't verify", log_path.display());
            }
            log
        }
        Err(e) => {
            let Some(org) = file.meta.organization else {
                return;
            };
            warn!("cannot read {}: {e}; starting empty", log_path.display());
            cadrs_social::log::Log::new(LogId(org.log))
        }
    };
    let (keys, _) = match device_keys(world) {
        Ok(k) => k,
        Err(why) => {
            crate::linked::toast(world, why);
            return;
        }
    };
    crate::social::ensure_loaded(world);
    world.insert_resource(OpenOrg {
        id,
        session: Session::open(log, keys.device),
        log_path,
        tab: OrgTab::Overview,
        channel: "general".into(),
    });
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Organization);
}

/// The organization's picture on the documents page: a world map with its pin, where it says
/// it is.
fn draw_picture(world: &World, doc: DocumentId, profile: &Profile) {
    use cadrs_core::thumbnail::{THUMB_LARGE_H, THUMB_LARGE_W};
    let sea = Color::srgb_u8(0xe8, 0xee, 0xf5);
    let pins: Vec<cadrs_ui::world_map::Pin> = profile
        .place
        .as_ref()
        .and_then(|p| match p.frame {
            Some(cadrs_social::place::Frame::Surface { lat, lon, .. }) => Some((lat, lon)),
            _ => None,
        })
        .map(|(lat, lon)| cadrs_ui::world_map::Pin {
            lat,
            lon,
            color: Color::srgb_u8(0x1f, 0x5f, 0xbf),
            accuracy_m: 0.0,
        })
        .into_iter()
        .collect();
    // The map is 2:1; the picture a little taller, so it sits in the middle.
    let (w, h) = (THUMB_LARGE_W, THUMB_LARGE_W / 2);
    let map = cadrs_ui::world_map::image(w, h, sea, Color::srgb_u8(0x55, 0x60, 0x70), &pins);
    let Some(map) = map
        .data
        .and_then(|data| image::RgbaImage::from_raw(w, h, data))
    else {
        return;
    };
    let mut picture =
        image::RgbaImage::from_pixel(w, THUMB_LARGE_H, image::Rgba([0xe8, 0xee, 0xf5, 0xff]));
    image::imageops::overlay(&mut picture, &map, 0, ((THUMB_LARGE_H - h) / 2) as i64);
    let store = world.resource::<DocumentStore>().0.clone();
    if let Err(e) = store.write_thumbnail(doc, &picture) {
        warn!("cannot draw the organization's picture: {e}");
    }
}

/// Writes `op` to the open organization and saves it; a toast says why when it doesn't apply.
pub fn write(world: &mut World, op: Op) -> bool {
    let profile_changes = matches!(op, Op::SetProfile(_));
    let Some(mut open) = world.get_resource_mut::<OpenOrg>() else {
        return false;
    };
    match open.session.write(op, cadrs_social::wall_millis()) {
        Ok(_) => {
            let saved = cadrs_org::file::save(&open.log_path, open.session.log());
            if let Err(e) = saved {
                crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-save")));
            }
            if profile_changes && let Some(open) = world.get_resource::<OpenOrg>() {
                let (doc, profile) = (open.id, open.session.org().profile.clone());
                draw_picture(world, doc, &profile);
            }
            // Its public listing follows (when it takes requests from anyone), and the other
            // members' devices hear of it now.
            crate::social::relist(world);
            crate::org_sync::sync_now(world);
            true
        }
        Err(why) => {
            crate::linked::toast(world, why);
            false
        }
    }
}

pub(crate) fn field(world: &mut World, name: &str) -> String {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

pub(crate) fn number(world: &mut World, name: &str) -> Option<f64> {
    field(world, name).trim().replace(',', ".").parse().ok()
}

fn switch_on(world: &mut World, name: &str) -> bool {
    let mut q = world.query::<(&Name, &SwitchState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .is_some_and(|(_, s)| s.on)
}

pub(crate) fn strip_selected(world: &mut World, name: &str) -> usize {
    let mut q = world.query::<(&Name, &TabStripState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .map_or(0, |(_, s)| s.selected)
}

pub(crate) fn close_dialogs(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<OrgDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

fn spawn_screen(
    mut commands: Commands,
    theme: Res<Theme>,
    profile: Res<UserProfile>,
    open: Option<Res<OpenOrg>>,
) {
    let t = theme.clone();
    let name = open
        .as_ref()
        .map(|o| o.session.org().profile.name.clone())
        .unwrap_or_default();
    commands
        .spawn((
            Name::new("organization"),
            DespawnOnExit(AppState::Organization),
            bevy::input_focus::tab_navigation::TabGroup::new(0),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(t.background),
            cadrs_ui::glass::wallpaper(),
        ))
        .with_children(|root| {
            crate::landing::top_bar(root, &t, &profile);
            root.spawn((
                Name::new("org-page"),
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ))
            .with_children(|page| {
                page.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(16.0),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|head| {
                    head.spawn((
                        Name::new("org-name"),
                        t.text(name, t.font_xl, FontWeight::BOLD, t.foreground),
                    ));
                    let mut strip = TabStrip::new("org-tabs").compact();
                    for (_, key) in TABS {
                        strip = strip.tab(tr(key));
                    }
                    head.spawn((
                        strip.build(&t),
                        observe(|ev: On<TabStripSelect>, mut open: ResMut<OpenOrg>| {
                            if let Some((tab, _)) = TABS.get(ev.index) {
                                open.tab = *tab;
                            }
                        }),
                    ));
                });
                page.spawn((
                    Name::new("org-body"),
                    OrgBody,
                    ScrollArea,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        // Fields and buttons keep their own width.
                        align_items: AlignItems::FlexStart,
                        row_gap: Val::Px(8.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        overflow: Overflow::scroll_y(),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
                        ..default()
                    },
                    cadrs_ui::glass::panel(&t),
                ));
            });
        });
}

/// Rebuilds the open tab when the organization or the tab changes.
#[allow(clippy::too_many_arguments)]
fn refresh_body(
    mut commands: Commands,
    open: Option<Res<OpenOrg>>,
    body: Query<Entity, With<OrgBody>>,
    mut names: Query<(&Name, &mut Text)>,
    theme: Res<Theme>,
    social: Res<crate::social::Social>,
    mut images: ResMut<Assets<Image>>,
    mut qr: Local<Option<Handle<Image>>>,
    mut built: Local<bool>,
) {
    let Some(open) = open else {
        *built = false;
        return;
    };
    let Ok(body) = body.single() else {
        *built = false;
        return;
    };
    if *built && !open.is_changed() {
        return;
    }
    *built = true;
    if let Some((_, mut text)) = names.iter_mut().find(|(n, _)| n.as_str() == "org-name") {
        text.0 = open.session.org().profile.name.clone();
    }
    let t = theme.clone();
    // The shop's card, as a link and a QR code, on the overview.
    let share = (open.tab == OrgTab::Overview)
        .then(|| social.org_card(&open.session.org().org?))
        .flatten()
        .map(|card| {
            let link = cadrs_social::contact::to_link(&card);
            *qr = cadrs_ui::qr::image(&link).map(|i| images.add(i));
            (link, qr.clone())
        });
    commands.entity(body).despawn_related::<Children>();
    commands.entity(body).with_children(|b| match open.tab {
        OrgTab::Overview => overview(b, &t, &open, share),
        OrgTab::Members => members(b, &t, &open),
        OrgTab::Machines => machines(b, &t, &open),
        OrgTab::Stock => stock(b, &t, &open),
        OrgTab::Prices => prices(b, &t, &open),
        OrgTab::Requests => requests(b, &t, &open),
        OrgTab::Jobs => crate::org_tabs::jobs(b, &t, &open),
        OrgTab::Clients => crate::org_tabs::clients(b, &t, &open, &social),
        OrgTab::Parts => crate::org_tabs::parts(b, &t, &open),
        OrgTab::Chat => crate::org_tabs::chat(b, &t, &open, &social),
    });
}

pub(crate) fn heading(b: &mut ChildSpawnerCommands, t: &Theme, key: &str) {
    b.spawn(t.text(tr(key), t.font_lg, FontWeight::BOLD, t.foreground));
}

pub(crate) fn muted(b: &mut ChildSpawnerCommands, t: &Theme, text: impl Into<String>) {
    b.spawn(t.paragraph(text, t.font_sm, t.muted_foreground));
}

pub(crate) fn label(b: &mut ChildSpawnerCommands, t: &Theme, key: &str) {
    b.spawn(t.text(tr(key), t.font_sm, FontWeight::BOLD, t.foreground));
}

/// A row of things side by side.
pub(crate) fn row(b: &mut ChildSpawnerCommands, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    b.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(12.0),
        flex_wrap: FlexWrap::Wrap,
        row_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(f);
}

pub(crate) fn input(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    name: &'static str,
    value: String,
    width: f32,
) {
    let placeholder = tr(&format!("{name}-placeholder"));
    let mut input = TextInput::new(name).value(value).width(Val::Px(width));
    if !placeholder.ends_with("-placeholder") {
        input = input.placeholder(placeholder);
    }
    b.spawn(input.build(t));
}

pub(crate) fn short(id: &PublicKey) -> String {
    id.to_string()[..12].to_string()
}

fn overview(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    open: &OpenOrg,
    share: Option<(String, Option<Handle<Image>>)>,
) {
    let org = open.session.org();
    let profile = &org.profile;
    heading(b, t, "org-tab-overview");
    if let Some((stars, n)) = org.rating() {
        muted(b, t, format!("★ {stars:.1} ({n})"));
    }
    if let Some(account) = org.org {
        muted(b, t, format!("{} {}", tr("org-id"), account));
    }
    label(b, t, "org-name-label");
    input(b, t, "org-profile-name", profile.name.clone(), 360.0);
    label(b, t, "org-about");
    input(b, t, "org-profile-about", profile.about.clone(), 560.0);
    label(b, t, "org-location");
    let (lat, lon) = match profile.place.as_ref().and_then(|p| p.frame.as_ref()) {
        Some(cadrs_social::place::Frame::Surface { lat, lon, .. }) => {
            (lat.to_string(), lon.to_string())
        }
        _ => (String::new(), String::new()),
    };
    let region = profile
        .place
        .as_ref()
        .and_then(|p| p.region.clone())
        .unwrap_or_default();
    row(b, |r| {
        input(r, t, "org-profile-lat", lat, 140.0);
        input(r, t, "org-profile-lon", lon, 140.0);
        input(
            r,
            t,
            "org-profile-city",
            region.city.unwrap_or_default(),
            180.0,
        );
        input(
            r,
            t,
            "org-profile-country",
            region.country.unwrap_or_default(),
            180.0,
        );
    });
    muted(b, t, tr("org-location-help"));
    label(b, t, "org-public-accuracy");
    input(
        b,
        t,
        "org-profile-accuracy",
        format!("{}", profile.public_accuracy_m / 1000.0),
        140.0,
    );
    b.spawn(
        Switch::new("org-profile-open")
            .label(tr("org-open-for-quotes"))
            .on(profile.open_for_quotes)
            .build(t),
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
    if let Some((link, qr)) = share {
        heading(b, t, "org-share");
        muted(b, t, tr("org-share-help"));
        if let Some(qr) = qr {
            b.spawn((
                Name::new("org-share-qr"),
                ImageNode::new(qr),
                Node {
                    width: Val::Px(200.0),
                    height: Val::Px(200.0),
                    ..default()
                },
            ));
        }
        b.spawn(Node {
            width: Val::Px(560.0),
            max_width: Val::Percent(100.0),
            ..default()
        })
        .with_child(
            cadrs_ui::selectable_text::SelectableText::new("org-share-link")
                .value(link)
                .font(t.font_xs, FontWeight::NORMAL)
                .copy_button()
                .build(t),
        );
    }
}

fn save_profile(world: &mut World) {
    let name = field(world, "org-profile-name");
    let about = field(world, "org-profile-about");
    let lat = number(world, "org-profile-lat");
    let lon = number(world, "org-profile-lon");
    let city = field(world, "org-profile-city");
    let country = field(world, "org-profile-country");
    let accuracy_km = number(world, "org-profile-accuracy").unwrap_or(0.0);
    let open_for_quotes = switch_on(world, "org-profile-open");
    let some = |s: String| (!s.trim().is_empty()).then(|| s.trim().to_owned());
    let region = Region {
        country: some(country),
        area: None,
        city: some(city),
    };
    let place = match (lat, lon) {
        (Some(lat), Some(lon)) if (-90.0..=90.0).contains(&lat) => {
            let mut place = Place::earth(lat, lon);
            place.region = Some(region);
            Some(place)
        }
        _ if region != Region::default() => Some(Place {
            body: cadrs_social::place::Body::Earth,
            frame: None,
            accuracy_m: None,
            region: Some(region),
        }),
        _ => None,
    };
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let profile = Profile {
        name: if name.trim().is_empty() {
            open.session.org().profile.name.clone()
        } else {
            name.trim().to_owned()
        },
        about,
        place,
        public_accuracy_m: accuracy_km.max(0.0) * 1000.0,
        open_for_quotes,
        ..open.session.org().profile.clone()
    };
    if write(world, Op::SetProfile(profile)) {
        crate::linked::toast(world, tr("org-saved"));
    }
}

fn members(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    heading(b, t, "org-tab-members");
    for (i, (account, role)) in org.members.iter().enumerate() {
        let devices = org.devices.of(account);
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        let role = tr(match role {
            Role::Owner => "org-role-owner",
            Role::Admin => "org-role-admin",
            Role::Member => "org-role-member",
        });
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-member-{i}")),
                t.text(
                    short(account),
                    t.font_base,
                    FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            r.spawn(t.text(role, t.font_base, FontWeight::NORMAL, t.foreground));
            r.spawn(t.text(
                names.join(", "),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
        });
    }
    b.spawn((
        Button::new("org-add-member")
            .label(tr("org-add-member"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>,
             mut commands: Commands,
             theme: Res<Theme>,
             social: Res<crate::social::Social>| {
                let t = theme.clone();
                let tb = theme.clone();
                let people: Vec<String> = people(&social).iter().map(|c| c.name.clone()).collect();
                commands.spawn((
                    Dialog::new("org-member-dialog")
                        .title(tr("org-add-member"))
                        .width(560.0)
                        .body(move |b| {
                            b.spawn(t_text(&tb, "org-member-who"));
                            if !people.is_empty() {
                                let mut strip = TabStrip::new("org-member-contact");
                                for p in &people {
                                    strip = strip.tab(p.clone());
                                }
                                b.spawn(strip.build(&tb));
                            }
                            b.spawn(
                                TextInput::new("org-member-link")
                                    .placeholder(tr("org-member-link-placeholder"))
                                    .build(&tb),
                            );
                            b.spawn(
                                TabStrip::new("org-member-role")
                                    .tab(tr("org-role-member"))
                                    .tab(tr("org-role-admin"))
                                    .tab(tr("org-role-owner"))
                                    .build(&tb),
                            );
                        })
                        .footer(|f| dialog_buttons(f, "org-member", add_member))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

pub(crate) fn t_text(t: &Theme, key: &str) -> impl Bundle {
    t.text(tr(key), t.font_sm, FontWeight::BOLD, t.foreground)
}

/// A dialog's OK and Cancel, OK running `ok`.
pub(crate) fn dialog_buttons(f: &mut ChildSpawner, name: &str, ok: fn(&mut World)) {
    let t = f.world().resource::<Theme>().clone();
    f.spawn((
        Button::new(format!("{name}-ok"))
            .label(tr("common-ok"))
            .primary()
            .build(&t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(ok);
        }),
    ));
    f.spawn((
        Button::new(format!("{name}-cancel"))
            .label(tr("common-cancel"))
            .build(&t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(close_dialogs);
        }),
    ));
}

/// The people (not organizations) among the contacts.
fn people(social: &crate::social::Social) -> Vec<cadrs_social::contact::Card> {
    social
        .contacts
        .cards
        .iter()
        .map(|c| c.value.clone())
        .filter(|c| c.kind == cadrs_social::contact::Kind::Person)
        .collect()
}

fn add_member(world: &mut World) {
    crate::social::ensure_loaded(world);
    let link = field(world, "org-member-link");
    let card = if link.trim().is_empty() {
        let i = strip_selected(world, "org-member-contact");
        people(world.resource::<crate::social::Social>())
            .get(i)
            .cloned()
    } else {
        match cadrs_social::contact::from_link(&link) {
            Ok(card) if card.value.kind == cadrs_social::contact::Kind::Person => {
                crate::social::add_contact(world, card.clone());
                Some(card.value)
            }
            _ => None,
        }
    };
    let Some(card) = card else {
        crate::linked::toast(world, tr("org-member-bad-account"));
        return;
    };
    let role =
        [Role::Member, Role::Admin, Role::Owner][strip_selected(world, "org-member-role").min(2)];
    let op = Op::SetMember {
        account: card.account,
        role: Some(role),
    };
    if write(world, op) {
        close_dialogs(world);
        // Their cadrs makes the organization document and fills it by syncing.
        crate::org_sync::invite(world, &card);
    }
}

const PROCESSES: [Process; 8] = [
    Process::Waterjet,
    Process::Laser,
    Process::Plasma,
    Process::Router,
    Process::Mill,
    Process::Lathe,
    Process::PressBrake,
    Process::Printer,
];

fn machines(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    heading(b, t, "org-tab-machines");
    if org.machines.is_empty() {
        muted(b, t, tr("org-no-machines"));
    }
    for (i, (id, m)) in org.machines.iter().enumerate() {
        let id = *id;
        let materials: Vec<String> = m
            .materials
            .iter()
            .map(|l| format!("{} ≤ {} mm", l.material, l.max_thickness))
            .collect();
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-machine-{i}")),
                t.text(
                    m.name.clone(),
                    t.font_base,
                    FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            r.spawn(t.text(
                format!(
                    "{} · {} × {} mm · {}",
                    m.process.label(),
                    m.envelope[0],
                    m.envelope[1],
                    materials.join(", ")
                ),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
            r.spawn((
                Button::new(format!("org-machine-remove-{i}"))
                    .label(tr("common-remove"))
                    .ghost()
                    .small()
                    .build(t),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |w: &mut World| {
                        write(w, Op::SetMachine { id, machine: None });
                    });
                }),
            ));
        });
    }
    b.spawn((
        Button::new("org-add-machine")
            .label(tr("org-add-machine"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                commands.spawn((
                    Dialog::new("org-machine-dialog")
                        .title(tr("org-add-machine"))
                        .width(600.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-machine-name"));
                            b.spawn(TextInput::new("org-machine-name").autofocus().build(t));
                            let mut strip = TabStrip::new("org-machine-process").compact();
                            for p in PROCESSES {
                                strip = strip.tab(p.label());
                            }
                            b.spawn(strip.build(t));
                            b.spawn(t_text(t, "org-machine-bed"));
                            b.spawn(Node {
                                column_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|r| {
                                r.spawn(
                                    TextInput::new("org-machine-x")
                                        .value("3000")
                                        .width(Val::Px(120.0))
                                        .build(t),
                                );
                                r.spawn(
                                    TextInput::new("org-machine-y")
                                        .value("1500")
                                        .width(Val::Px(120.0))
                                        .build(t),
                                );
                                r.spawn(
                                    TextInput::new("org-machine-kerf")
                                        .value("1")
                                        .width(Val::Px(120.0))
                                        .build(t),
                                );
                            });
                            b.spawn(t_text(t, "org-machine-materials"));
                            b.spawn(
                                TextInput::new("org-machine-materials")
                                    .placeholder(tr("org-machine-materials-placeholder"))
                                    .build(t),
                            );
                        })
                        .footer(|f| dialog_buttons(f, "org-machine", add_machine))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

/// "mild steel 100, glass 25" → limits.
fn parse_materials(text: &str) -> Vec<MaterialLimit> {
    text.split([',', ';'])
        .filter_map(|part| {
            let part = part.trim();
            let (name, mm) = part.rsplit_once(' ')?;
            let mm: f64 = mm.trim_end_matches("mm").replace(',', ".").parse().ok()?;
            Some(MaterialLimit {
                material: name.trim().to_owned(),
                max_thickness: mm,
                speeds: Vec::new(),
            })
        })
        .collect()
}

fn add_machine(world: &mut World) {
    let name = field(world, "org-machine-name");
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-machine-needs-name"));
        return;
    }
    let process = PROCESSES[strip_selected(world, "org-machine-process").min(PROCESSES.len() - 1)];
    let x = number(world, "org-machine-x").unwrap_or(0.0);
    let y = number(world, "org-machine-y").unwrap_or(0.0);
    let kerf = number(world, "org-machine-kerf").unwrap_or(0.0);
    let materials = parse_materials(&field(world, "org-machine-materials"));
    let machine = MachineCaps {
        name: name.trim().to_owned(),
        process,
        envelope: [x, y, 0.0],
        materials,
        kerf,
        min_hole_ratio: if process == Process::Plasma { 1.5 } else { 0.5 },
        tolerance: 0.1,
        pierce_seconds: 2.0,
    };
    if write(
        world,
        Op::SetMachine {
            id: new_id(),
            machine: Some(machine),
        },
    ) {
        close_dialogs(world);
    }
}

fn stock(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    heading(b, t, "org-tab-stock");
    if org.materials.is_empty() {
        muted(b, t, tr("org-no-materials"));
    }
    for (i, (id, m)) in org.materials.iter().enumerate() {
        let id = *id;
        let on_hand = org.stock.get(&id).copied().unwrap_or(0);
        let free = org.available(id);
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-material-{i}")),
                t.text(
                    format!("{} {} mm", m.name, m.thickness),
                    t.font_base,
                    FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            r.spawn((
                Name::new(format!("org-material-count-{i}")),
                t.text(
                    format!("{on_hand} ({free} {})", tr("org-free")),
                    t.font_base,
                    FontWeight::NORMAL,
                    t.foreground,
                ),
            ));
            for (delta, label) in [(1i64, "+1"), (-1, "−1")] {
                r.spawn((
                    Button::new(format!("org-material-{i}-{delta}"))
                        .label(label)
                        .small()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| {
                            write(
                                w,
                                Op::Stock {
                                    material: id,
                                    sheets: delta,
                                },
                            );
                        });
                    }),
                ));
            }
        });
    }
    b.spawn((
        Button::new("org-add-material")
            .label(tr("org-add-material"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                commands.spawn((
                    Dialog::new("org-material-dialog")
                        .title(tr("org-add-material"))
                        .width(520.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-material-name"));
                            b.spawn(TextInput::new("org-material-name").autofocus().build(t));
                            b.spawn(t_text(t, "org-material-size"));
                            b.spawn(Node {
                                column_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|r| {
                                r.spawn(
                                    TextInput::new("org-material-thickness")
                                        .value("3")
                                        .width(Val::Px(100.0))
                                        .build(t),
                                );
                                r.spawn(
                                    TextInput::new("org-material-x")
                                        .value("2500")
                                        .width(Val::Px(100.0))
                                        .build(t),
                                );
                                r.spawn(
                                    TextInput::new("org-material-y")
                                        .value("1250")
                                        .width(Val::Px(100.0))
                                        .build(t),
                                );
                            });
                        })
                        .footer(|f| dialog_buttons(f, "org-material", add_material))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

fn add_material(world: &mut World) {
    let name = field(world, "org-material-name");
    let Some(thickness) = number(world, "org-material-thickness").filter(|t| *t > 0.0) else {
        crate::linked::toast(world, tr("org-material-needs-thickness"));
        return;
    };
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-material-needs-name"));
        return;
    }
    let sheet = [
        number(world, "org-material-x").unwrap_or(0.0),
        number(world, "org-material-y").unwrap_or(0.0),
    ];
    if write(
        world,
        Op::SetMaterial {
            id: new_id(),
            material: Some(Material {
                name: name.trim().to_owned(),
                thickness,
                sheet,
            }),
        },
    ) {
        close_dialogs(world);
    }
}

fn money_text(m: Money) -> String {
    format!("{:.2}", m.0 as f64 / 100.0)
}

fn prices(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    let rates = org.rates.clone().unwrap_or_else(|| RateCard {
        currency: "ZAR".into(),
        setup: Money::default(),
        minimum: Money::default(),
        hourly: Vec::new(),
        sheet_per_m2: Vec::new(),
        nesting_allowance: 1.25,
    });
    heading(b, t, "org-tab-prices");
    muted(b, t, tr("org-prices-help"));
    row(b, |r| {
        r.spawn(t_text(t, "org-rates-currency"));
        input(r, t, "org-rates-currency", rates.currency.clone(), 80.0);
        r.spawn(t_text(t, "org-rates-setup"));
        input(r, t, "org-rates-setup", money_text(rates.setup), 110.0);
        r.spawn(t_text(t, "org-rates-minimum"));
        input(r, t, "org-rates-minimum", money_text(rates.minimum), 110.0);
        r.spawn(t_text(t, "org-rates-nesting"));
        input(
            r,
            t,
            "org-rates-nesting",
            format!("{:.0}", (rates.nesting_allowance - 1.0) * 100.0),
            70.0,
        );
    });
    label(b, t, "org-rates-hourly");
    if org.machines.is_empty() {
        muted(b, t, tr("org-no-machines"));
    }
    for (i, m) in org.machines.values().enumerate() {
        let rate = rates
            .hourly
            .iter()
            .find(|(n, _)| *n == m.name)
            .map_or(String::new(), |(_, r)| money_text(*r));
        row(b, |r| {
            r.spawn(
                TextInput::new(format!("org-rate-machine-{i}"))
                    .value(rate)
                    .width(Val::Px(110.0))
                    .build(t),
            );
            r.spawn(t.text(
                m.name.clone(),
                t.font_base,
                FontWeight::NORMAL,
                t.foreground,
            ));
        });
    }
    label(b, t, "org-rates-sheet");
    if org.materials.is_empty() {
        muted(b, t, tr("org-no-materials"));
    }
    for (i, m) in org.materials.values().enumerate() {
        let price = rates
            .sheet_per_m2
            .iter()
            .find(|(n, th, _)| n.eq_ignore_ascii_case(&m.name) && (*th - m.thickness).abs() < 1e-6)
            .map_or(String::new(), |(_, _, p)| money_text(*p));
        row(b, |r| {
            r.spawn(
                TextInput::new(format!("org-rate-material-{i}"))
                    .value(price)
                    .width(Val::Px(110.0))
                    .build(t),
            );
            r.spawn(t.text(
                format!("{} {} mm", m.name, m.thickness),
                t.font_base,
                FontWeight::NORMAL,
                t.foreground,
            ));
        });
    }
    b.spawn((
        Button::new("org-rates-save")
            .label(tr("common-save"))
            .primary()
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(save_prices);
        }),
    ));
}

fn save_prices(world: &mut World) {
    let money = |world: &mut World, name: &str| number(world, name).map(Money::from_major);
    let currency = field(world, "org-rates-currency").trim().to_uppercase();
    let setup = money(world, "org-rates-setup").unwrap_or_default();
    let minimum = money(world, "org-rates-minimum").unwrap_or_default();
    let nesting = 1.0 + number(world, "org-rates-nesting").unwrap_or(25.0).max(0.0) / 100.0;
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let machines: Vec<String> = open
        .session
        .org()
        .machines
        .values()
        .map(|m| m.name.clone())
        .collect();
    let materials: Vec<(String, f64)> = open
        .session
        .org()
        .materials
        .values()
        .map(|m| (m.name.clone(), m.thickness))
        .collect();
    let mut hourly = Vec::new();
    for (i, name) in machines.into_iter().enumerate() {
        if let Some(rate) = money(world, &format!("org-rate-machine-{i}")) {
            hourly.push((name, rate));
        }
    }
    let mut sheet_per_m2 = Vec::new();
    for (i, (name, thickness)) in materials.into_iter().enumerate() {
        if let Some(price) = money(world, &format!("org-rate-material-{i}")) {
            sheet_per_m2.push((name, thickness, price));
        }
    }
    let rates = RateCard {
        currency: if currency.is_empty() {
            "ZAR".into()
        } else {
            currency
        },
        setup,
        minimum,
        hourly,
        sheet_per_m2,
        nesting_allowance: nesting,
    };
    if write(world, Op::SetRates(rates)) {
        crate::linked::toast(world, tr("org-saved"));
    }
}

fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::Requested => "org-stage-requested",
        Stage::Quoted => "org-stage-quoted",
        Stage::Declined => "org-stage-declined",
        Stage::Accepted => "org-stage-accepted",
        Stage::InProduction => "org-stage-in-production",
        Stage::Ready => "org-stage-ready",
        Stage::Completed => "org-stage-completed",
    }
}

fn requests(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    heading(b, t, "org-tab-requests");
    if org.requests.is_empty() {
        muted(b, t, tr("org-no-requests"));
    }
    for (i, (id, r)) in org.requests.iter().enumerate() {
        let id = *id;
        let parts: Vec<String> = r
            .parts
            .iter()
            .map(|(p, n)| format!("{n} × {}", p.name))
            .collect();
        let price = r
            .quote
            .as_ref()
            .map(|q| format!("{} {}", q.currency, q.total.format()))
            .unwrap_or_default();
        let stage = r.stage;
        row(b, |row| {
            row.spawn((
                Name::new(format!("org-request-{i}")),
                t.text(
                    parts.join(", "),
                    t.font_base,
                    FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            row.spawn(t.text(
                short(&r.client),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
            row.spawn((
                Name::new(format!("org-request-stage-{i}")),
                t.text(
                    tr(stage_key(stage)),
                    t.font_sm,
                    FontWeight::BOLD,
                    t.foreground,
                ),
            ));
            row.spawn(t.text(price, t.font_base, FontWeight::NORMAL, t.foreground));
            // The stock held for it.
            let held: Vec<String> = org
                .reservations
                .values()
                .filter(|res| res.request == id)
                .filter_map(|res| {
                    let m = org.materials.get(&res.material)?;
                    Some(format!("{} × {} {} mm", res.sheets, m.name, m.thickness))
                })
                .collect();
            if !held.is_empty() {
                row.spawn((
                    Name::new(format!("org-request-held-{i}")),
                    t.text(
                        held.join(", "),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                ));
            }
            if matches!(stage, Stage::Accepted | Stage::InProduction) && !org.materials.is_empty() {
                row.spawn((
                    Button::new(format!("org-request-reserve-{i}"))
                        .label(tr("org-reserve"))
                        .small()
                        .ghost()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| open_reserve(w, id));
                    }),
                ));
            }
            let messages = r.chat.len();
            row.spawn((
                Button::new(format!("org-request-chat-{i}"))
                    .label(format!("{} ({messages})", tr("chat-messages")))
                    .small()
                    .ghost()
                    .build(t),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |w: &mut World| {
                        crate::chat_ui::open(w, crate::chat_ui::Thread::Shop(id))
                    });
                }),
            ));
            // The order's next step.
            let next = match stage {
                Stage::Accepted => Some(("org-start-production", Some(Stage::InProduction))),
                Stage::InProduction => Some(("org-mark-ready", Some(Stage::Ready))),
                Stage::Ready => Some(("org-delivered", None)),
                _ => None,
            };
            if let Some((label, to)) = next {
                row.spawn((
                    Button::new(format!("org-request-next-{i}"))
                        .label(tr(label))
                        .small()
                        .primary()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| match to {
                            Some(stage) => advance(w, id, stage),
                            None => deliver(w, id),
                        });
                    }),
                ));
            }
            if stage == Stage::Requested {
                row.spawn((
                    Button::new(format!("org-request-quote-{i}"))
                        .label(tr("org-instant-quote"))
                        .small()
                        .primary()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| instant_quote(w, id));
                    }),
                ));
                row.spawn((
                    Button::new(format!("org-request-decline-{i}"))
                        .label(tr("org-decline"))
                        .small()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| {
                            let decline = Op::Decline {
                                request: id,
                                reason: String::new(),
                            };
                            if write(w, decline) {
                                tell_client(
                                    w,
                                    id,
                                    cadrs_org::letters::Message::Decline {
                                        request: id,
                                        reason: String::new(),
                                    },
                                );
                            }
                        });
                    }),
                ));
            }
        });
    }
}

/// The order the Reserve dialog is for.
#[derive(Resource)]
struct Reserving(cadrs_org::Id);

fn open_reserve(world: &mut World, request: cadrs_org::Id) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let materials: Vec<String> = org
        .materials
        .iter()
        .map(|(id, m)| {
            format!(
                "{} {} mm ({} {})",
                m.name,
                m.thickness,
                org.available(*id),
                tr("org-free")
            )
        })
        .collect();
    world.insert_resource(Reserving(request));
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-reserve-dialog")
            .title(tr("org-reserve"))
            .width(520.0)
            .body(move |b| {
                let t = &tb;
                let mut strip = TabStrip::new("org-reserve-material");
                for m in &materials {
                    strip = strip.tab(m.clone());
                }
                b.spawn(strip.build(t));
                b.spawn(t_text(t, "org-reserve-sheets"));
                b.spawn(
                    TextInput::new("org-reserve-sheets")
                        .value("1")
                        .width(Val::Px(100.0))
                        .build(t),
                );
            })
            .footer(|f| dialog_buttons(f, "org-reserve", reserve))
            .build(&t),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
}

fn reserve(world: &mut World) {
    let Some(Reserving(request)) = world.remove_resource::<Reserving>() else {
        return;
    };
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let materials: Vec<cadrs_org::Id> = open.session.org().materials.keys().copied().collect();
    let Some(material) = materials
        .get(strip_selected(world, "org-reserve-material"))
        .copied()
    else {
        return;
    };
    let sheets = number(world, "org-reserve-sheets").unwrap_or(1.0).max(1.0) as u32;
    let op = Op::Reserve {
        id: new_id(),
        reservation: cadrs_org::Reservation {
            material,
            sheets,
            request,
        },
    };
    if write(world, op) {
        close_dialogs(world);
    } else {
        // Refused (not enough free): keep the dialog for another try.
        world.insert_resource(Reserving(request));
    }
}

/// Moves an order on, and tells the client.
pub fn advance(world: &mut World, request: cadrs_org::Id, stage: Stage) {
    if write(world, Op::Advance { request, stage }) {
        tell_client(
            world,
            request,
            cadrs_org::letters::Message::Stage { request, stage },
        );
    }
}

/// Delivered: the organization signs half the order's receipt and sends it to the client to
/// countersign; the order completes when it comes back.
pub fn deliver(world: &mut World, request: cadrs_org::Id) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let (Some(account), Some(client)) = (org.org, org.requests.get(&request).map(|r| r.client))
    else {
        return;
    };
    let Some(key) = crate::social::org_key(world, &account) else {
        crate::linked::toast(world, tr("org-needs-org-key"));
        return;
    };
    // What was held for it is used up.
    let held: Vec<cadrs_org::Id> = org
        .reservations
        .iter()
        .filter(|(_, r)| r.request == request)
        .map(|(id, _)| *id)
        .collect();
    for id in held {
        write(world, Op::Release { id, used: true });
    }
    let receipt = cadrs_social::receipts::Receipt::OrderCompleted {
        shop: account,
        client,
        order: cadrs_org::order_ref(request),
        at: cadrs_social::wall_millis(),
    }
    .sign_first(&key)
    .expect("the organization is the shop");
    tell_client(
        world,
        request,
        cadrs_org::letters::Message::Delivered { request, receipt },
    );
}

/// Sends `message` to the client of request `request`, from the open organization.
pub(crate) fn tell_client(
    world: &mut World,
    request: cadrs_org::Id,
    message: cadrs_org::letters::Message,
) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let (Some(client), Some(account)) = (org.requests.get(&request).map(|r| r.client), org.org)
    else {
        return;
    };
    // From the contacts, else the card the request came with.
    let card = world
        .resource::<crate::social::Social>()
        .contacts
        .get(&client)
        .cloned()
        .or_else(|| {
            world
                .get_resource::<OpenOrg>()?
                .session
                .org()
                .requests
                .get(&request)?
                .card
                .as_ref()
                .map(|c| c.value.clone())
        });
    match card {
        Some(card) => {
            crate::social::send(
                world,
                &card,
                message,
                Some(account),
                tr("social-what-reply"),
            );
        }
        None => crate::linked::toast(world, tr("social-no-client-card")),
    }
}

/// Quotes a request at the price the rate card gives.
pub fn instant_quote(world: &mut World, request: cadrs_org::Id) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let Some(rates) = org.rates.clone() else {
        crate::linked::toast(world, tr("org-no-rates"));
        return;
    };
    let Some(r) = org.requests.get(&request) else {
        return;
    };
    let machines: Vec<MachineCaps> = org.machines.values().cloned().collect();
    match cadrs_org::quote::estimate(&machines, &rates, &r.parts) {
        Ok(estimate) => {
            let quote = cadrs_org::Quote {
                total: estimate.total,
                currency: estimate.currency,
                lines: estimate.lines,
                valid_until: cadrs_social::wall_millis() + 14 * 86_400_000,
                note: String::new(),
            };
            let op = Op::Quote {
                request,
                quote: quote.clone(),
            };
            if write(world, op) {
                tell_client(
                    world,
                    request,
                    cadrs_org::letters::Message::Quote { request, quote },
                );
            }
        }
        Err(cadrs_org::quote::NoPrice::NoMachine { part, why }) => {
            crate::linked::toast(world, format!("{part}: {}", why.join("; ")));
        }
        Err(cadrs_org::quote::NoPrice::NoRate(what)) => {
            crate::linked::toast(world, format!("{}: {what}", tr("org-no-rates")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_parse() {
        let m = parse_materials("mild steel 100, glass 25mm; junk");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].material, "mild steel");
        assert_eq!(m[1].max_thickness, 25.0);
    }
}
