//! **New organization…** (the account menu): the dialog that makes one.
//!
//! An organization is what strangers judge a shop by in the marketplace, so making one asks for
//! more than a name: a logo (a square picture of at least 512 px, kept at 512), a name, a line
//! that says what it does, what it offers (manufacturing, designs or both), where it makes
//! things if it manufactures (a country and a city: its first location), and an e-mail address
//! clients can write to. Each field says what is wrong with it, under it, when Create is
//! pressed; nothing is made until all are right.
//!
//! The same logo picker serves the organization's Profile module ([`pick_logo`] with
//! [`LOGO_FOR_PROFILE`]).

use std::path::Path;

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_org::profile::{MAX_NAME, MAX_TAGLINE, MIN_NAME, plausible_email};
use cadrs_org::{Address, Location, Logo, Offers, Op};
use cadrs_ui::checkbox::{Checkbox, CheckboxChange, CheckboxState};
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::TextInput;
use cadrs_ui::theme::Theme;
use cadrs_ui::{Button, FilePicked};

use crate::AppState;
use crate::org_logo::{LogoImages, Processed};
use crate::organization::{NewOrg, OpenOrg, field};

/// The file picker's tag for the new organization's logo.
pub const LOGO_FOR_NEW: &str = "org-logo-new";
/// The file picker's tag for the open organization's logo (its Profile module).
pub const LOGO_FOR_PROFILE: &str = "org-logo-profile";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Draft>()
        .init_resource::<LogoJobs>()
        .add_systems(Update, (on_file_picked, finish_logos, recheck))
        .add_observer(on_offer_change)
        .add_observer(on_close);
}

/// What the dialog holds that isn't in its fields: the logo picked.
#[derive(Resource, Default)]
struct Draft {
    logo: Option<Processed>,
    /// Create was pressed with something wrong: from then on each field's line follows what
    /// is typed, so a mistake goes as it is put right.
    checking: bool,
}

#[derive(Component)]
struct NewOrgDialog;

/// Where the logo's picture (or its placeholder) is shown.
#[derive(Component)]
struct LogoPreview;

/// The fields asked only of an organization that manufactures.
#[derive(Component)]
struct LocationFields;

/// The line under a field that says what is wrong with it.
#[derive(Component)]
struct FieldError(&'static str);

/// The dialog's fields that can be wrong, in the order they are shown.
#[cfg(test)]
const FIELDS: [&str; 6] = ["logo", "name", "tagline", "offers", "location", "email"];

/// What a person typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Typed {
    pub name: String,
    pub tagline: String,
    pub manufacturing: bool,
    pub designs: bool,
    pub country: String,
    pub city: String,
    pub email: String,
}

/// What is wrong with what was typed: (the field, why), in the catalogs' words. Empty: nothing.
pub(crate) fn problems(typed: &Typed, has_logo: bool) -> Vec<(&'static str, String)> {
    let mut wrong = Vec::new();
    if !has_logo {
        wrong.push(("logo", tr("org-new-needs-logo")));
    }
    let name = typed.name.trim().chars().count();
    if !(MIN_NAME..=MAX_NAME).contains(&name) {
        wrong.push(("name", tr("org-name-length")));
    }
    let tagline = typed.tagline.trim().chars().count();
    if tagline == 0 {
        wrong.push(("tagline", tr("org-new-needs-tagline")));
    } else if tagline > MAX_TAGLINE {
        wrong.push(("tagline", tr("org-tagline-length")));
    }
    if !typed.manufacturing && !typed.designs {
        wrong.push(("offers", tr("org-new-needs-offers")));
    }
    if typed.manufacturing {
        let country = typed.country.trim();
        if country.is_empty() || typed.city.trim().is_empty() {
            wrong.push(("location", tr("org-new-needs-location")));
        } else if cadrs_org::country::find(country).is_none() {
            wrong.push((
                "location",
                tr_with("org-location-country-unknown", &[("text", country)]),
            ));
        }
    }
    if typed.email.trim().is_empty() {
        wrong.push(("email", tr("org-new-needs-email")));
    } else if !plausible_email(typed.email.trim()) {
        wrong.push(("email", tr("org-email-bad")));
    }
    wrong
}

/// The organization what was typed describes (checked with [`problems`] first).
pub(crate) fn new_org(typed: &Typed, logo: Processed) -> NewOrg {
    let location = typed.manufacturing.then(|| Location {
        name: tr("org-first-location-name"),
        production: true,
        pickup: false,
        address: Address {
            city: typed.city.trim().to_owned(),
            country: cadrs_org::country::find(&typed.country)
                .unwrap_or_default()
                .to_owned(),
            ..Default::default()
        },
        point: None,
        phone: String::new(),
        public: true,
        public_accuracy_m: 5_000.0,
    });
    NewOrg {
        name: typed.name.trim().to_owned(),
        tagline: typed.tagline.trim().to_owned(),
        offers: Offers {
            manufacturing: typed.manufacturing,
            designs: typed.designs,
        },
        email: typed.email.trim().to_owned(),
        logo,
        location,
    }
}

fn checked(world: &mut World, name: &str) -> bool {
    let mut q = world.query::<(&Name, &CheckboxState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .is_some_and(|(_, s)| s.checked)
}

/// Opens the New organization dialog (once).
pub fn open(world: &mut World) {
    let mut q = world.query_filtered::<(), With<NewOrgDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    *world.resource_mut::<Draft>() = Draft::default();
    let state = *world.resource::<State<AppState>>().get();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("new-organization-dialog")
            .title(tr("org-new"))
            .width(620.0)
            .body(move |b| body(b, &tb))
            .footer(move |f| {
                f.spawn((
                    Button::new("new-organization-ok")
                        .label(tr("org-new-create"))
                        .primary()
                        .build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(create)),
                ));
                f.spawn((
                    Button::new("new-organization-cancel")
                        .label(tr("common-cancel"))
                        .build(&tf),
                    observe(
                        |_: On<Activate>,
                         q: Query<Entity, With<NewOrgDialog>>,
                         mut commands: Commands| {
                            for e in &q {
                                commands.trigger(DialogClose { entity: e });
                            }
                        },
                    ),
                ));
            })
            .build(&theme),
        NewOrgDialog,
        DespawnOnExit(state),
    ));
    world.flush();
}

/// A field's label, the field (`control`), a note and the line for what is wrong with it.
fn field_block(
    b: &mut ChildSpawner,
    t: &Theme,
    key: &'static str,
    label: &str,
    control: impl FnOnce(&mut ChildSpawner),
) {
    b.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|c| {
        c.spawn(t.text(tr(label), t.font_sm, FontWeight::BOLD, t.foreground));
        control(c);
        c.spawn((
            Name::new(format!("org-new-{key}-error")),
            FieldError(key),
            t.text("", t.font_sm, FontWeight::NORMAL, t.danger),
            Node {
                display: Display::None,
                ..default()
            },
        ));
    });
}

fn note(b: &mut ChildSpawner, t: &Theme, key: &str) {
    b.spawn((t.paragraph_within(tr(key), t.font_sm, t.muted_foreground, 560.0),));
}

fn body(b: &mut ChildSpawner, t: &Theme) {
    note(b, t, "org-new-help");
    field_block(b, t, "logo", "org-logo", |c| {
        c.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn((
                Name::new("org-new-logo"),
                LogoPreview,
                Node {
                    width: Val::Px(88.0),
                    height: Val::Px(88.0),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(14.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BorderColor::all(t.border),
                BackgroundColor(t.list_hover),
            ))
            .with_child(cadrs_ui::icon("image", 28.0, t.muted_foreground));
            r.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|side| {
                side.spawn((
                    Button::new("org-new-logo-choose")
                        .label(tr("org-logo-choose"))
                        .icon("image")
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| pick_logo(w, LOGO_FOR_NEW));
                    }),
                ));
                note(side, t, "org-logo-help");
            });
        });
    });
    field_block(b, t, "name", "org-name-label", |c| {
        c.spawn(
            TextInput::new("org-new-name")
                .placeholder(tr("org-name-placeholder"))
                .autofocus()
                .width(Val::Px(360.0))
                .build(t),
        );
    });
    field_block(b, t, "tagline", "org-tagline", |c| {
        c.spawn(
            TextInput::new("org-new-tagline")
                .placeholder(tr("org-tagline-placeholder"))
                .max_characters(MAX_TAGLINE)
                .build(t),
        );
        note(c, t, "org-tagline-help");
    });
    field_block(b, t, "offers", "org-offers", |c| {
        c.spawn(
            Checkbox::new("org-new-manufacturing")
                .label(tr("org-offers-manufacturing-long"))
                .checked(true)
                .build(t),
        );
        c.spawn(
            Checkbox::new("org-new-designs")
                .label(tr("org-offers-designs-long"))
                .build(t),
        );
    });
    b.spawn((
        Name::new("org-new-location"),
        LocationFields,
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ))
    .with_children(|l| {
        field_block(l, t, "location", "org-new-where", |c| {
            c.spawn(Node {
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(
                    TextInput::new("org-new-country")
                        .placeholder(tr("org-location-country-placeholder"))
                        .width(Val::Px(240.0))
                        .build(t),
                );
                r.spawn(
                    TextInput::new("org-new-city")
                        .placeholder(tr("org-location-city-placeholder"))
                        .width(Val::Px(240.0))
                        .build(t),
                );
            });
            note(c, t, "org-new-where-help");
        });
    });
    field_block(b, t, "email", "org-email", |c| {
        c.spawn(
            TextInput::new("org-new-email")
                .placeholder(tr("org-email-placeholder"))
                .width(Val::Px(360.0))
                .build(t),
        );
    });
}

/// The location's fields show only while Manufacturing is ticked.
fn on_offer_change(
    ev: On<CheckboxChange>,
    names: Query<&Name>,
    mut fields: Query<&mut Node, With<LocationFields>>,
) {
    if names.get(ev.entity).map(Name::as_str) != Ok("org-new-manufacturing") {
        return;
    }
    for mut node in &mut fields {
        node.display = if ev.checked {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Closed without making one: the picture picked is forgotten.
fn on_close(ev: On<DialogClose>, q: Query<(), With<NewOrgDialog>>, mut draft: ResMut<Draft>) {
    if q.contains(ev.entity) {
        *draft = Draft::default();
    }
}

fn typed(world: &mut World) -> Typed {
    Typed {
        name: field(world, "org-new-name"),
        tagline: field(world, "org-new-tagline"),
        manufacturing: checked(world, "org-new-manufacturing"),
        designs: checked(world, "org-new-designs"),
        country: field(world, "org-new-country"),
        city: field(world, "org-new-city"),
        email: field(world, "org-new-email"),
    }
}

/// Shows under each field what is wrong with it (nothing under the ones that are right).
fn show_problems(world: &mut World, wrong: &[(&'static str, String)]) {
    let mut q = world.query::<(&FieldError, &mut Text, &mut Node)>();
    for (field, mut text, mut node) in q.iter_mut(world) {
        let why = wrong.iter().find(|(f, _)| *f == field.0).map(|(_, w)| w);
        let (shown, display) = match why {
            Some(why) => (why.clone(), Display::Flex),
            None => (String::new(), Display::None),
        };
        if text.0 != shown {
            text.0 = shown;
        }
        if node.display != display {
            node.display = display;
        }
    }
}

/// After a Create that found something wrong, the lines under the fields follow the fields.
fn recheck(world: &mut World) {
    if !world.resource::<Draft>().checking {
        return;
    }
    let mut dialogs = world.query_filtered::<(), With<NewOrgDialog>>();
    if dialogs.iter(world).next().is_none() {
        return;
    }
    let typed = typed(world);
    let has_logo = world.resource::<Draft>().logo.is_some();
    let mut wrong = problems(&typed, has_logo);
    // A picture that was refused keeps saying why until another is picked.
    if !has_logo {
        wrong.retain(|(field, _)| *field != "logo");
        let mut q = world.query::<(&FieldError, &Text)>();
        let said = q
            .iter(world)
            .find(|(field, _)| field.0 == "logo")
            .map(|(_, text)| text.0.clone())
            .unwrap_or_default();
        wrong.push(("logo", said));
    }
    show_problems(world, &wrong);
}

/// Create: makes the organization when nothing is wrong, else says what is.
fn create(world: &mut World) {
    let typed = typed(world);
    let logo = world.resource::<Draft>().logo.clone();
    let wrong = problems(&typed, logo.is_some());
    show_problems(world, &wrong);
    let (true, Some(logo)) = (wrong.is_empty(), logo) else {
        world.resource_mut::<Draft>().checking = true;
        return;
    };
    if crate::organization::create(world, new_org(&typed, logo)) {
        *world.resource_mut::<Draft>() = Draft::default();
        let mut q = world.query_filtered::<Entity, With<NewOrgDialog>>();
        for e in q.iter(world).collect::<Vec<_>>() {
            world.trigger(DialogClose { entity: e });
        }
    }
}

/// Opens the file prompt for a logo; the pick comes back with `tag`.
pub fn pick_logo(world: &mut World, tag: &'static str) {
    let theme = world.resource::<Theme>().clone();
    let dir = cadrs_ui::file_picker::home_dir().unwrap_or_default();
    let mut commands = world.commands();
    cadrs_ui::file_picker::open_file_picker(
        &mut commands,
        &theme,
        "org-logo-picker",
        &tr("org-logo-choose"),
        tag,
        dir,
        &["png", "jpg", "jpeg", "gif"],
    );
    world.flush();
}

fn on_file_picked(mut picked: MessageReader<FilePicked>, mut commands: Commands) {
    for pick in picked.read() {
        let tag = match pick.tag.as_str() {
            LOGO_FOR_NEW => LOGO_FOR_NEW,
            LOGO_FOR_PROFILE => LOGO_FOR_PROFILE,
            _ => continue,
        };
        let path = pick.path.clone();
        commands.queue(move |w: &mut World| take_logo_file(w, tag, &path));
    }
}

/// Logos being made from picked files, off the main thread: a large photograph takes a
/// moment to read, crop and scale, and the window shouldn't stop for it.
#[derive(Resource)]
struct LogoJobs {
    done: std::sync::Mutex<std::sync::mpsc::Receiver<LogoJob>>,
    send: std::sync::mpsc::Sender<LogoJob>,
}

/// (what it is for, the organization that was open when the picture was picked, the logo).
type LogoJob = (
    &'static str,
    Option<cadrs_core::DocumentId>,
    Result<Processed, crate::org_logo::LogoError>,
);

impl Default for LogoJobs {
    fn default() -> Self {
        let (send, done) = std::sync::mpsc::channel();
        Self {
            done: std::sync::Mutex::new(done),
            send,
        }
    }
}

fn take_logo_file(world: &mut World, tag: &'static str, path: &Path) {
    let send = world.resource::<LogoJobs>().send.clone();
    let path = path.to_owned();
    let open = world.get_resource::<OpenOrg>().map(|o| o.id);
    let work = move || {
        let too_large =
            cadrs_fs::metadata(&path).is_ok_and(|m| m.len() > crate::org_logo::MAX_FILE);
        let made = if too_large {
            Err(crate::org_logo::LogoError::FileTooLarge)
        } else {
            cadrs_fs::read(&path)
                .map_err(|_| crate::org_logo::LogoError::NotAPicture)
                .and_then(|bytes| crate::org_logo::process(&bytes))
        };
        let _ = send.send((tag, open, made));
    };
    // No thread to be had: here and now, then.
    if cadrs_core::background::spawn("logo", work.clone()).is_err() {
        work();
    }
}

/// Takes in the logos that are ready.
fn finish_logos(world: &mut World) {
    let ready: Vec<LogoJob> = {
        let jobs = world.resource::<LogoJobs>();
        let done = jobs.done.lock().unwrap_or_else(|e| e.into_inner());
        done.try_iter().collect()
    };
    for (tag, open, made) in ready {
        // A logo for the Profile of an organization the person has left meanwhile isn't given
        // to whichever is open now.
        if tag == LOGO_FOR_PROFILE && world.get_resource::<OpenOrg>().map(|o| o.id) != open {
            continue;
        }
        take_logo(world, tag, made);
    }
}

/// A picked picture, made into a logo or not: shown in the New organization dialog (with what
/// is wrong with it, if anything), or set as the open organization's logo.
pub fn take_logo(
    world: &mut World,
    tag: &'static str,
    made: Result<Processed, crate::org_logo::LogoError>,
) {
    match (tag, made) {
        (LOGO_FOR_NEW, Ok(logo)) => {
            show_preview(world, &logo);
            world.resource_mut::<Draft>().logo = Some(logo);
            // Only the logo's line: the other fields may not have been filled in yet.
            let mut q = world.query::<(&FieldError, &mut Text, &mut Node)>();
            for (field, mut text, mut node) in q.iter_mut(world) {
                if field.0 == "logo" {
                    text.0.clear();
                    node.display = Display::None;
                }
            }
        }
        (LOGO_FOR_NEW, Err(why)) => {
            let mut q = world.query::<(&FieldError, &mut Text, &mut Node)>();
            for (field, mut text, mut node) in q.iter_mut(world) {
                if field.0 == "logo" {
                    text.0 = why.message();
                    node.display = Display::Flex;
                }
            }
        }
        (_, Ok(logo)) => set_logo(world, &logo),
        (_, Err(why)) => crate::linked::toast(world, why.message()),
    }
}

fn show_preview(world: &mut World, logo: &Processed) {
    let hash = cadrs_org::BlobRef::of(&logo.full).hash;
    let picture = world.resource_scope(|world, mut logos: Mut<LogoImages>| {
        logos.get(&mut world.resource_mut::<Assets<Image>>(), hash, &logo.full)
    });
    let Some(picture) = picture else {
        return;
    };
    let mut q = world.query_filtered::<Entity, With<LogoPreview>>();
    for preview in q.iter(world).collect::<Vec<_>>() {
        world.entity_mut(preview).despawn_related::<Children>();
        world.entity_mut(preview).with_child((
            Name::new("org-new-logo-picture"),
            ImageNode::new(picture.clone()),
            Node {
                width: Val::Px(88.0),
                height: Val::Px(88.0),
                ..default()
            },
        ));
    }
}

/// Sets the open organization's logo: its two pictures kept as blobs, then named in its profile.
fn set_logo(world: &mut World, logo: &Processed) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let dir = open.dir();
    let kept = cadrs_org::blob::put(&dir, &logo.full)
        .and_then(|full| Ok((full, cadrs_org::blob::put(&dir, &logo.thumb)?)));
    let (full, thumb) = match kept {
        Ok(blobs) => blobs,
        Err(e) => {
            crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-save")));
            return;
        }
    };
    // Only the logo is written: what someone else changed of the profile meanwhile stands.
    let edit = cadrs_org::profile::ProfileEdit {
        logo: Some(Logo { full, thumb }),
        ..Default::default()
    };
    if crate::organization::write(world, Op::EditProfile(edit)) {
        crate::linked::toast(world, tr("org-logo-changed"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Typed {
        Typed {
            name: "Cape Waterjet".into(),
            tagline: "Waterjet cutting in steel, aluminium and glass".into(),
            manufacturing: true,
            designs: false,
            country: "South Africa".into(),
            city: "Stellenbosch".into(),
            email: "sales@capewaterjet.example".into(),
        }
    }

    fn wrong(typed: &Typed, has_logo: bool) -> Vec<&'static str> {
        problems(typed, has_logo)
            .into_iter()
            .map(|(f, _)| f)
            .collect()
    }

    #[test]
    fn every_field_is_asked_for() {
        assert_eq!(wrong(&good(), true), Vec::<&str>::new());
        assert_eq!(wrong(&good(), false), ["logo"]);
        // Nothing typed: everything is wrong, in the order the fields are shown.
        let empty = Typed {
            manufacturing: true,
            ..Default::default()
        };
        assert_eq!(
            wrong(&empty, false),
            ["logo", "name", "tagline", "location", "email"]
        );
        let neither = Typed {
            manufacturing: false,
            ..good()
        };
        assert_eq!(wrong(&neither, true), ["offers"]);
        for (field, typed) in [
            (
                "name",
                Typed {
                    name: "C".into(),
                    ..good()
                },
            ),
            (
                "tagline",
                Typed {
                    tagline: "x".repeat(MAX_TAGLINE + 1),
                    ..good()
                },
            ),
            (
                "location",
                Typed {
                    country: "Atlantis".into(),
                    ..good()
                },
            ),
            (
                "location",
                Typed {
                    city: " ".into(),
                    ..good()
                },
            ),
            (
                "email",
                Typed {
                    email: "sales".into(),
                    ..good()
                },
            ),
        ] {
            assert_eq!(wrong(&typed, true), [field], "{typed:?}");
        }
        // Every problem names a field the dialog has a line for.
        for (field, _) in problems(&Typed::default(), false) {
            assert!(FIELDS.contains(&field));
        }
    }

    /// Only one that manufactures is asked where; one that only publishes designs has no
    /// location at all.
    #[test]
    fn designs_alone_need_no_location() {
        let designs = Typed {
            manufacturing: false,
            designs: true,
            country: String::new(),
            city: String::new(),
            ..good()
        };
        assert_eq!(wrong(&designs, true), Vec::<&str>::new());
        let logo = Processed {
            full: vec![1],
            thumb: vec![2],
        };
        assert!(new_org(&designs, logo.clone()).location.is_none());
        let shop = new_org(&good(), logo);
        let location = shop.location.unwrap();
        assert!(location.production && location.public);
        assert_eq!(location.address.country, "ZA");
        assert_eq!(location.address.city, "Stellenbosch");
        assert_eq!(location.problem(), None);
    }
}
