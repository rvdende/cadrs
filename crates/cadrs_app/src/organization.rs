//! Organizations (shops): their own screen, instead of the CAD view.
//!
//! An organization is kept like a document (its `document.ron` has no tabs, and its metadata's
//! `organization` marks it; its content is the signed log `org.log` beside it, `cadrs_org`, and
//! its logo and the documents clients sent in `blobs/`), but it isn't one of the user's
//! documents: it is opened from the account menu ([`crate::identity`]), where the user's
//! organizations are listed and new ones are made ([`crate::org_create`]).
//!
//! Opening one shows [`AppState::Organization`]: the top bar, and under it the organization's
//! modules in a side navigation (in groups, as an ERP's are: Sales, Production, Inventory,
//! Quality, Organization) with the chosen module beside it. Every change goes through
//! [`write`] ([`cadrs_org::session::Session::write`] refuses what wouldn't apply), then the log
//! is saved, the listing follows and the other members' devices hear of it.

use std::path::PathBuf;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, ScrollArea, observe};
use cadrs_core::library::OrgRef;
use cadrs_core::{Document, DocumentFile, DocumentId, DocumentMeta};
use cadrs_org::capability::{MachineCaps, MaterialLimit, Process};
use cadrs_org::quality::IssueStatus;
use cadrs_org::quote::{Money, RateCard};
use cadrs_org::session::Session;
use cadrs_org::{Location, Logo, Material, Offers, Op, Profile, Role, Stage, new_id};
use cadrs_social::keys::PublicKey;
use cadrs_social::keystore::DeviceKeys;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::switch::SwitchState;
use cadrs_ui::tab_strip::{TabStrip, TabStripState};
use cadrs_ui::theme::Theme;
use cadrs_ui::{Button, Column, SideNav, SideNavSelect, TableHeader, TableRow};

use crate::{AppState, DocumentStore, UserProfile};

mod machines;
mod members;
mod pricing;
mod requests;

pub(crate) use machines::process_name;
use machines::*;
use members::*;
use pricing::*;
use requests::*;
pub use requests::{advance, deliver, instant_quote};
pub(crate) use requests::{request_number, stage_key, status_tag, tell_client};

pub struct OrganizationPlugin;

impl Plugin for OrganizationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::org_logo::LogoImages>()
            .init_resource::<OrgForm>()
            .add_observer(machines::on_process)
            .add_observer(machines::on_material_picked)
            .add_systems(OnEnter(AppState::Organization), spawn_screen)
            .add_systems(
                Update,
                (track_form, refresh_body)
                    .chain()
                    .run_if(in_state(AppState::Organization)),
            )
            .add_systems(
                Update,
                sweep_now_and_then.run_if(in_state(AppState::Organization)),
            )
            .add_systems(OnExit(AppState::Organization), |mut c: Commands| {
                c.remove_resource::<OpenOrg>()
            });
        crate::org_create::plugin(app);
    }
}

/// The organization's modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OrgTab {
    #[default]
    Overview,
    Requests,
    Clients,
    Jobs,
    Machines,
    Stock,
    Parts,
    Issues,
    Profile,
    Locations,
    Members,
    Prices,
    Wallet,
    Chat,
}

/// A module of the side navigation: its key (the item is `org-nav-<key>`), the catalog key of
/// its name, its icon, and the catalog key of the group it opens (the first of each group).
pub(crate) struct Module {
    pub tab: OrgTab,
    pub key: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub group: Option<&'static str>,
}

/// The modules in the navigation's order. The groups follow what an ERP calls them, so that as
/// modules are added (purchasing, shipping, inspections) each has a place to go.
pub(crate) const MODULES: [Module; 14] = [
    Module {
        tab: OrgTab::Overview,
        key: "overview",
        label: "org-tab-overview",
        icon: "dashboard",
        group: None,
    },
    Module {
        tab: OrgTab::Requests,
        key: "requests",
        label: "org-tab-requests",
        icon: "receipt",
        group: Some("org-group-sales"),
    },
    Module {
        tab: OrgTab::Clients,
        key: "clients",
        label: "org-tab-clients",
        icon: "users",
        group: None,
    },
    Module {
        tab: OrgTab::Jobs,
        key: "jobs",
        label: "org-tab-jobs",
        icon: "job-schedule",
        group: Some("org-group-production"),
    },
    Module {
        tab: OrgTab::Machines,
        key: "machines",
        label: "org-tab-machines",
        icon: "shop-machines",
        group: None,
    },
    Module {
        tab: OrgTab::Stock,
        key: "stock",
        label: "org-tab-stock",
        icon: "stock",
        group: Some("org-group-inventory"),
    },
    Module {
        tab: OrgTab::Parts,
        key: "parts",
        label: "org-tab-parts",
        icon: "part",
        group: None,
    },
    Module {
        tab: OrgTab::Issues,
        key: "issues",
        label: "org-tab-issues",
        icon: "quality-issue",
        group: Some("org-group-quality"),
    },
    Module {
        tab: OrgTab::Profile,
        key: "profile",
        label: "org-tab-profile",
        icon: "shop",
        group: Some("org-group-organization"),
    },
    Module {
        tab: OrgTab::Locations,
        key: "locations",
        label: "org-tab-locations",
        icon: "map-pin",
        group: None,
    },
    Module {
        tab: OrgTab::Members,
        key: "members",
        label: "org-tab-members",
        icon: "user",
        group: None,
    },
    Module {
        tab: OrgTab::Prices,
        key: "prices",
        label: "org-tab-prices",
        icon: "price-list",
        group: None,
    },
    Module {
        tab: OrgTab::Wallet,
        key: "wallet",
        label: "org-tab-wallet",
        icon: "wallet",
        group: None,
    },
    Module {
        tab: OrgTab::Chat,
        key: "chat",
        label: "org-tab-chat",
        icon: "chat",
        group: None,
    },
];

/// The open organization.
#[derive(Resource)]
pub struct OpenOrg {
    pub id: DocumentId,
    pub session: Session,
    log_path: PathBuf,
    pub tab: OrgTab,
    /// The channel the Chat module shows.
    pub channel: String,
    /// Counts up when what is shown must be made again whatever the person is doing: a change
    /// they made themselves. (A change from another device waits while they type.)
    pub nonce: u32,
}

impl OpenOrg {
    /// Shows module `tab`.
    pub fn show(&mut self, tab: OrgTab) {
        if self.tab != tab {
            self.tab = tab;
            self.nonce += 1;
        }
    }

    /// The folder its log and blobs are in.
    pub fn dir(&self) -> PathBuf {
        self.log_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default()
    }
}

#[derive(Component)]
struct OrgBody;

/// The navigation's holder, made again as its counts change.
#[derive(Component)]
struct OrgNav;

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

/// What a new organization starts with ([`crate::org_create`] asks for it).
#[derive(Clone, Debug)]
pub struct NewOrg {
    pub name: String,
    pub tagline: String,
    pub offers: Offers,
    pub email: String,
    /// Its logo: the 512 px picture and the 128 px one.
    pub logo: crate::org_logo::Processed,
    /// Where it makes things, when it manufactures.
    pub location: Option<Location>,
}

/// Writes `bytes` so that only this user reads them (a key): 0600 where there are such things.
fn write_secret(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        cadrs_fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        cadrs_fs::write(path, bytes)
    }
}

/// Creates the organization `new` describes and opens it. False (with a toast saying why)
/// when it couldn't be made; nothing is left behind then.
pub fn create(world: &mut World, new: NewOrg) -> bool {
    let fail = |world: &mut World, why: String| {
        crate::linked::toast(world, format!("{}: {why}", tr("org-cannot-create")));
        false
    };
    let (keys, key_path) = match device_keys(world) {
        Ok(k) => k,
        Err(why) => return fail(world, why),
    };
    let now = cadrs_social::wall_millis();
    let name = new.name.trim().to_owned();
    let (mut session, org_account) =
        Session::found(keys.device.clone(), keys.cert.clone(), &name, now);
    // Everything it starts with, tried before anything is written to disk.
    let logo = Logo {
        full: cadrs_org::BlobRef::of(&new.logo.full),
        thumb: cadrs_org::BlobRef::of(&new.logo.thumb),
    };
    let mut ops = Vec::new();
    if let Some(location) = new.location.clone() {
        ops.push(Op::SetLocation {
            id: new_id(),
            location: Some(location),
        });
    }
    ops.push(Op::SetProfile(Profile {
        name: name.clone(),
        tagline: new.tagline.trim().to_owned(),
        about: String::new(),
        logo: Some(logo),
        offers: new.offers,
        email: new.email.trim().to_owned(),
        website: None,
        listed: false,
    }));
    for op in ops {
        if let Err(why) = session.write(op, cadrs_social::wall_millis()) {
            return fail(world, why);
        }
    }
    let store = world.resource::<DocumentStore>().0.clone();
    let user = world.resource::<UserProfile>().id.clone();
    let doc = Document::empty(name.clone());
    let mut meta = DocumentMeta::new(&user, world.resource::<crate::AppClock>().now());
    meta.last_opened = meta.created.into();
    meta.organization = Some(OrgRef {
        log: session.log().id().0,
    });
    if let Err(e) = store.create(&doc, &meta) {
        return fail(world, e.to_string());
    }
    let dir = store.doc_dir(doc.id);
    // The organization's own key (beside the identity's keys), its logo and its log. Without
    // any of them it isn't one: undo what was made.
    let (org_key, _) = crate::social::org_files(&key_path, &org_account.id());
    let log_path = cadrs_org::file::path(&dir);
    let kept = write_secret(&org_key, &org_account.key().to_bytes())
        .and_then(|()| cadrs_org::blob::put(&dir, &new.logo.full).map(|_| ()))
        .and_then(|()| cadrs_org::blob::put(&dir, &new.logo.thumb).map(|_| ()))
        .and_then(|()| cadrs_org::file::save(&log_path, session.log()));
    if let Err(e) = kept {
        let _ = cadrs_fs::remove_file(&org_key);
        let _ = cadrs_fs::remove_dir_all(&dir);
        return fail(world, e.to_string());
    }
    let log_id = session.log().id();
    crate::social::add_org(world, doc.id, &name, org_account.id(), log_id, Vec::new());
    crate::social::org_changed(world, doc.id, session.org());
    world.insert_resource(OpenOrg {
        id: doc.id,
        session,
        log_path,
        tab: OrgTab::Overview,
        channel: "general".into(),
        nonce: 0,
    });
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Organization);
    true
}

/// Opens the organization kept as document `id` (from the account menu).
pub fn open_by_id(world: &mut World, id: DocumentId) {
    let store = world.resource::<DocumentStore>().0.clone();
    match store.load(id) {
        Ok(file) => open(world, &file),
        Err(e) => crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-open"))),
    }
}

/// Opens a stored organization (`file`'s metadata marks it one).
pub fn open(world: &mut World, file: &DocumentFile) {
    let store = world.resource::<DocumentStore>().0.clone();
    let id = file.document.id;
    let Some(org) = file.meta.organization else {
        return;
    };
    let log_path = cadrs_org::file::path(&store.doc_dir(id));
    let log = match cadrs_org::file::load(&log_path) {
        Ok((log, bad)) => {
            if bad > 0 {
                warn!("{bad} entries of {} didn't verify", log_path.display());
            }
            log
        }
        // Just joined, nothing synced yet: empty.
        Err(_) if !cadrs_fs::is_file(&log_path) => {
            cadrs_social::log::Log::new(cadrs_social::log::LogId(org.log))
        }
        // There, but not readable: opening it would end in saving an empty log over it.
        Err(e) => {
            warn!("cannot read {}: {e}", log_path.display());
            crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-open")));
            return;
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
        nonce: 0,
    });
    // (From an organization to another: the screen is made again for it.)
    let state = *world.resource::<State<AppState>>().get();
    if state == AppState::Organization {
        respawn_screen(world);
    } else {
        world
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Organization);
    }
}

/// Writes `op` to the open organization and saves it; a toast says why when it doesn't apply.
pub fn write(world: &mut World, op: Op) -> bool {
    write_as(world, op, true).is_ok()
}

/// Writes an operation that arrived (a client's letter) into the open organization: as
/// [`write`], but it isn't the person's own doing, so the module they are in isn't made again
/// under their hands while it holds something unsaved.
pub(crate) fn take_op(world: &mut World, op: Op) -> Result<(), String> {
    write_as(world, op, false)
}

fn write_as(world: &mut World, op: Op, own: bool) -> Result<(), String> {
    let Some(mut open) = world.get_resource_mut::<OpenOrg>() else {
        return Err("no organization is open".into());
    };
    // What the marketplace shows of it comes from these.
    let listing = matches!(
        op,
        Op::SetProfile(_) | Op::EditProfile(_) | Op::SetLocation { .. } | Op::SetMachine { .. }
    );
    let declined = matches!(op, Op::Decline { .. });
    match open.session.write(op, cadrs_social::wall_millis()) {
        Ok(_) => {
            if own {
                open.nonce += 1;
            }
            let saved = cadrs_org::file::save(&open.log_path, open.session.log());
            let (doc, dir, org) = (open.id, open.dir(), open.session.org().clone());
            if let Err(e) = saved {
                crate::linked::toast(world, format!("{}: {e}", tr("org-cannot-save")));
            }
            // The account menu's entry, its public listing (when what it shows changed) and
            // the other members' devices follow.
            crate::social::org_changed(world, doc, &org);
            if listing {
                crate::social::relist(world);
            }
            // A declined request's document is let go (a day later: `blob::sweep`).
            if declined {
                cadrs_org::blob::sweep(&dir, &org, cadrs_social::wall_millis());
            }
            crate::org_sync::sync_now(world);
            Ok(())
        }
        Err(why) => {
            if own {
                crate::linked::toast(world, refusal(&why));
            }
            // For whoever sent what this was made from: the document's own words.
            Err(why)
        }
    }
}

/// How often the open organization's folder is swept of what it no longer wants kept, in
/// seconds.
const SWEEP_EVERY: f64 = 600.0;

/// Sweeps the open organization's blobs when it is opened and every so often after: a declined
/// request's document goes a day after the decline, whether or not anything else happens.
fn sweep_now_and_then(
    open: Option<Res<OpenOrg>>,
    time: Res<Time<bevy::time::Real>>,
    mut last: Local<Option<(DocumentId, f64)>>,
) {
    let Some(open) = open else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if last.is_some_and(|(doc, at)| doc == open.id && now - at < SWEEP_EVERY) {
        return;
    }
    *last = Some((open.id, now));
    cadrs_org::blob::sweep(&open.dir(), open.session.org(), cadrs_social::wall_millis());
}

/// What waits for the answer to "Remove …?".
#[derive(Resource)]
struct PendingRemove(Op);

/// What `op` removes, named as its list names it.
fn removed_name(world: &World, op: &Op) -> Option<String> {
    let org = world.get_resource::<OpenOrg>()?.session.org();
    Some(match op {
        Op::SetLocation { id, .. } => org.locations.get(id)?.name.clone(),
        Op::SetMachine { id, .. } => org.machines.get(id)?.name.clone(),
        Op::SetMaterial { id, .. } => {
            let m = org.materials.get(id)?;
            format!("{} {} mm", m.name, m.thickness)
        }
        Op::SetClient { id, .. } => org.clients.get(id)?.name.clone(),
        Op::SetPart { id, .. } => org.parts.get(id)?.name.clone(),
        Op::SetJob { id, .. } => {
            let job = org.jobs.get(id)?;
            let machine = org.machines.get(&job.machine).map(|m| m.name.as_str());
            format!("{} · {}", tr("org-issue-about-job"), machine.unwrap_or("—"))
        }
        Op::SetMember { account, .. } => Names::read(world).of(account),
        _ => return None,
    })
}

/// Asks before `op` removes something: what is removed is gone for every member, and there is
/// no taking it back from here. **Remove** writes it.
pub(crate) fn ask_remove(world: &mut World, op: Op) {
    crate::social::ensure_loaded(world);
    let Some(name) = removed_name(world, &op) else {
        return;
    };
    world.insert_resource(PendingRemove(op));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-confirm-remove")
            .title(tr_with("org-confirm-remove-title", &[("name", &name)]))
            .width(480.0)
            .body(move |b| {
                b.spawn(tb.paragraph(tr("org-confirm-remove-text"), tb.font_base, tb.foreground));
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("org-confirm-remove-ok")
                        .label(tr("common-remove"))
                        .danger()
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            let pending = w.remove_resource::<PendingRemove>();
                            close_dialogs(w);
                            if let Some(PendingRemove(op)) = pending {
                                write(w, op);
                            }
                        });
                    }),
                ));
                f.spawn((
                    Button::new("org-confirm-remove-cancel")
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

/// Removes the open organization from this device: its document goes to the documents' trash
/// (to be restored from there, or deleted for good) and the documents page comes back. While
/// this device says its listing in the marketplace, it has to leave the marketplace first:
/// nothing would be left to take the listing down, or to answer those who find it.
pub(crate) fn remove_from_device(world: &mut World) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let (doc, org) = (open.id, open.session.org());
    let name = org.profile.name.clone();
    let says_listing = org.listed()
        && org.org.is_some_and(|account| {
            world
                .resource::<crate::social::Social>()
                .org_card(&account)
                .is_some()
        });
    close_dialogs(world);
    if says_listing {
        crate::linked::toast(world, tr("org-remove-unlist-first"));
        return;
    }
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<crate::AppClock>().now();
    world
        .resource_mut::<crate::landing::DocLibrary>()
        .execute(&store, &cadrs_core::library::TrashEntry { id: doc, now });
    world.remove_resource::<OpenOrg>();
    crate::social::refresh_orgs(world);
    crate::social::relist(world);
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Landing);
    crate::linked::toast(world, tr_with("org-removed", &[("name", &name)]));
}

/// A refusal as the person reads it: the catalog's wording where there is one for it, else the
/// document's own words (which are English, and exact).
pub(crate) fn refusal(why: &str) -> String {
    use cadrs_org::refusal::Refusal;
    let Some(kind) = Refusal::of(why) else {
        return why.to_owned();
    };
    tr(match kind {
        Refusal::Role => "org-refused-role",
        Refusal::LastOwner => "org-refused-last-owner",
        Refusal::LocationInUse => "org-refused-location-in-use",
        Refusal::MachineInUse => "org-refused-machine-in-use",
        Refusal::NoStock => "org-refused-no-stock",
        Refusal::NotAManufacturer => "org-refused-not-a-manufacturer",
        Refusal::Blocked => "org-refused-blocked",
        Refusal::TooManyOfYours => "org-refused-too-many-of-yours",
        Refusal::TooManyWaiting => "org-refused-too-many-waiting",
        Refusal::TooManyToday => "org-refused-too-many-today",
        Refusal::QuoteReplaced => "org-refused-quote-replaced",
        Refusal::QuoteExpired => "org-refused-quote-expired",
        Refusal::IssueClose => "org-refused-issue-close",
        Refusal::TooLarge => "org-refused-too-large",
    })
}

pub(crate) fn field(world: &mut World, name: &str) -> String {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

/// The materials to pick from, wherever the organization names one: the Part Studio's
/// ([`cadrs_core::material::LIBRARY`], what a part's material is picked from, so that a part
/// and a machine or a sheet in stock call a material the same), then the names the
/// organization uses already that are not among them.
pub(crate) fn material_names(org: &cadrs_org::Org) -> Vec<String> {
    let mut names: Vec<String> = cadrs_core::material::LIBRARY
        .iter()
        .map(|m| m.name.to_owned())
        .collect();
    let own = org.materials.values().map(|m| &m.name).chain(
        org.machines
            .values()
            .flat_map(|m| &m.materials)
            .map(|l| &l.material),
    );
    for name in own {
        if !names
            .iter()
            .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            names.push(name.clone());
        }
    }
    names
}

/// The dropdown a material is picked from (the Part Studio's, [`cadrs_ui::SearchSelect`]):
/// `names` found by typing, and what was typed offered too when no name is that, for a
/// material that isn't listed yet.
pub(crate) fn material_picker(name: &'static str, names: Vec<String>) -> cadrs_ui::SearchSelect {
    cadrs_ui::SearchSelect::new(name)
        .options(names)
        .placeholder(tr("render-material-select"))
        .search_placeholder(tr("render-material-search"))
        .empty_text(
            tr("render-material-none-match"),
            tr("render-material-none-yet"),
        )
        .typed(tr("org-material-use-typed"))
}

/// What the dropdown called `name` has chosen.
pub(crate) fn picked(world: &mut World, name: &str) -> Option<String> {
    let select = format!("{name}-select");
    let mut q = world.query::<(&Name, &cadrs_ui::SearchSelectState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == select)
        .and_then(|(_, s)| s.chosen.clone())
}

pub(crate) fn number(world: &mut World, name: &str) -> Option<f64> {
    field(world, name).trim().replace(',', ".").parse().ok()
}

pub(crate) fn switch_on(world: &mut World, name: &str) -> bool {
    let mut q = world.query::<(&Name, &SwitchState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .is_some_and(|(_, s)| s.on)
}

/// Which option of the tab strip or the list called `name` is chosen.
pub(crate) fn strip_selected(world: &mut World, name: &str) -> usize {
    let mut strips = world.query::<(&Name, &TabStripState)>();
    if let Some((_, strip)) = strips.iter(world).find(|(n, _)| n.as_str() == name) {
        return strip.selected;
    }
    let mut lists = world.query::<(&Name, &cadrs_ui::dialog_fields::SelectState)>();
    lists
        .iter(world)
        .find(|(n, _)| n.as_str() == name)
        .map_or(0, |(_, list)| list.selected)
}

/// A list to choose one of `options` from, for choices that grow with the organization (its
/// members, machines, locations, orders): it stays one line however many there are. Its
/// options are `<name>-option-<i>`; [`strip_selected`] reads the choice.
pub(crate) fn choice(
    t: &Theme,
    name: &'static str,
    options: impl IntoIterator<Item = String>,
    selected: usize,
) -> impl Bundle {
    let mut list = cadrs_ui::dialog_fields::Select::new(name).bordered();
    for option in options {
        list = list.option(option, true);
    }
    // In a row of its own width: the list fills the row, not the dialog.
    (
        Node {
            width: Val::Px(360.0),
            max_width: Val::Percent(100.0),
            ..default()
        },
        Children::spawn(bevy::ecs::spawn::Spawn(list.selected(selected).build(t))),
    )
}

pub(crate) fn close_dialogs(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<OrgDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

/// The width of the side navigation.
const NAV_WIDTH: f32 = 216.0;

fn spawn_screen(mut commands: Commands, theme: Res<Theme>, profile: Res<UserProfile>) {
    let t = theme.clone();
    commands
        .spawn((
            Name::new("organization"),
            OrgScreen,
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
                    column_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ))
            .with_children(|page| {
                // The navigation: the organization's logo and name over its modules.
                page.spawn((
                    Name::new("org-nav-panel"),
                    OrgNav,
                    Node {
                        width: Val::Px(NAV_WIDTH),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(6.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
                        overflow: Overflow::clip_y(),
                        ..default()
                    },
                    cadrs_ui::glass::panel(&t),
                ));
                page.spawn((
                    Name::new("org-body"),
                    OrgBody,
                    ScrollArea,
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        // Fields and buttons keep their own width.
                        align_items: AlignItems::FlexStart,
                        row_gap: Val::Px(8.0),
                        padding: UiRect::all(Val::Px(20.0)),
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

/// The organization's screen (its root).
#[derive(Component)]
struct OrgScreen;

/// Makes the screen again (another organization was opened over one).
fn respawn_screen(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<OrgScreen>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.entity_mut(e).despawn();
    }
    let spawn = world.register_system(spawn_screen);
    let _ = world.run_system(spawn);
    let _ = world.unregister_system(spawn);
    world.flush();
}

/// The modules that show the organization's wallet: the Wallet itself, the Requests (how each
/// order's payment stands) and the Overview (what is in it).
fn shows_wallet(tab: OrgTab) -> bool {
    matches!(tab, OrgTab::Wallet | OrgTab::Requests | OrgTab::Overview)
}

/// What the screen was last made for: the module, how much of the document there was, the
/// person's own changes, the logos that had arrived, and who was in the contacts.
type Built = (OrgTab, usize, u32, u32, usize, String, u32);

/// Makes the side navigation again: the organization's logo and name, and its modules with
/// what waits in each.
fn make_nav(world: &mut World, nav: Entity, theme: &Theme) {
    let logo = world
        .resource::<OpenOrg>()
        .session
        .org()
        .profile
        .logo
        .and_then(|l| crate::org_logo::handle(world, &l.thumb));
    let open = world.resource::<OpenOrg>();
    let (org, tab) = (open.session.org(), open.tab);
    let name = org.profile.name.clone();
    let open_requests = org
        .requests
        .values()
        .filter(|r| matches!(r.stage, Stage::Requested))
        .count();
    let open_issues = org
        .issues
        .values()
        .filter(|i| i.status != IssueStatus::Closed)
        .count();
    let count = |n: usize| if n == 0 { String::new() } else { n.to_string() };
    let mut modules = SideNav::new("org-nav").selected(MODULES.iter().position(|m| m.tab == tab));
    for module in &MODULES {
        if let Some(group) = module.group {
            modules = modules.group(tr(group));
        }
        modules = modules.item(module.key, module.icon, tr(module.label));
        modules = match module.tab {
            OrgTab::Requests => modules.count(count(open_requests)),
            OrgTab::Issues => modules.count(count(open_issues)),
            _ => modules,
        };
    }
    let nav_bundle = modules.build(theme);
    let t = theme.clone();
    let mut commands = world.commands();
    commands.entity(nav).despawn_related::<Children>();
    commands.entity(nav).with_children(|n| {
        n.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(4.0), Val::Px(8.0)),
            ..default()
        })
        .with_children(|head| {
            head.spawn(crate::org_logo::logo("org-nav-logo", &t, logo, &name, 36.0));
            head.spawn((
                Name::new("org-name"),
                t.text(name.clone(), t.font_md, FontWeight::BOLD, t.foreground),
                cadrs_ui::ellipsis::Ellipsis::default(),
                Node {
                    min_width: Val::Px(0.0),
                    flex_shrink: 1.0,
                    ..default()
                },
            ));
        });
        n.spawn((
            nav_bundle,
            observe(|ev: On<SideNavSelect>, mut open: ResMut<OpenOrg>| {
                if let Some(module) = MODULES.iter().find(|m| m.key == ev.key) {
                    open.show(module.tab);
                }
            }),
        ));
    });
}

/// The line across a module that isn't made again because it holds something unsaved.
#[derive(Component)]
struct NewsWaits;

/// Says, across the top of the module, that something changed elsewhere and isn't shown yet,
/// with a button that shows it (and drops what was typed here).
fn say_news_waits(world: &mut World, body: Entity, theme: &Theme) {
    let mut q = world.query_filtered::<(), With<NewsWaits>>();
    if q.iter(world).next().is_some() {
        return;
    }
    let t = theme.clone();
    let line = world
        .spawn((
            Name::new("org-news-waits"),
            NewsWaits,
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
            BorderColor::all(t.link),
        ))
        .with_children(|line| {
            line.spawn(t.text(
                tr("org-news-waits"),
                t.font_sm,
                FontWeight::NORMAL,
                t.foreground,
            ));
            line.spawn((
                Button::new("org-news-show")
                    .label(tr("org-news-show"))
                    .small()
                    .build(&t),
                observe(|_: On<Activate>, mut form: ResMut<OrgForm>| form.show = true),
            ));
        })
        .id();
    world.entity_mut(body).insert_children(0, &[line]);
}

/// Makes the navigation and the open module again when the organization or the module
/// changes. A change that came from another device while the person is typing in the module
/// waits until they leave the field, so that what they typed isn't thrown away.
fn refresh_body(world: &mut World, mut built: Local<(Option<Built>, Option<Built>)>) {
    // What the navigation and the module were last made from.
    let (nav_built, built) = &mut *built;
    if !world.contains_resource::<OpenOrg>() {
        (*nav_built, *built) = (None, None);
        return;
    }
    let mut q_body = world.query_filtered::<Entity, With<OrgBody>>();
    let mut q_nav = world.query_filtered::<Entity, With<OrgNav>>();
    let (Some(body), Some(nav)) = (q_body.iter(world).next(), q_nav.iter(world).next()) else {
        (*nav_built, *built) = (None, None);
        return;
    };
    let open = world.resource::<OpenOrg>();
    let social = world.resource::<crate::social::Social>();
    let key: Built = (
        open.tab,
        open.session.log().len(),
        open.nonce,
        social.logos_arrived,
        social.contacts.cards.len(),
        open.channel.clone(),
        // What shows the organization's wallet is made again when the bank says something new.
        if shows_wallet(open.tab) {
            world
                .get_resource::<crate::credits::Credits>()
                .map_or(0, |c| c.version)
        } else {
            0
        },
    );
    if built.as_ref() == Some(&key) && nav_built.as_ref() == Some(&key) {
        return;
    }
    let own_change = world.resource::<OrgForm>().show
        || built
            .as_ref()
            .is_none_or(|b| b.0 != key.0 || b.2 != key.2 || b.5 != key.5);
    let chat = key.0 == OrgTab::Chat;
    let typing = typing_in(world, body);
    // What arrives from other devices doesn't make the module again while the person is typing
    // in it, or while it holds something they changed and haven't saved (five prices typed,
    // then a click elsewhere): that would throw it away. The navigation follows at once, and a
    // line across the module says there is news, with a button to show it. Their own Save, or
    // moving to another module, shows everything that arrived meanwhile.
    //
    // The chat is the exception: what others say appears as it arrives, and the message being
    // written stays in its box.
    let hold = !own_change && !chat && (typing || world.resource::<OrgForm>().dirty);
    let draft = (chat && !own_change).then(|| (field(world, "org-chat-input"), typing));
    let nav_due = nav_built.as_ref() != Some(&key);
    *nav_built = Some(key.clone());
    if hold {
        if nav_due {
            let theme = world.resource::<Theme>().clone();
            make_nav(world, nav, &theme);
            say_news_waits(world, body, &theme);
            world.flush();
        }
        return;
    }
    *built = Some(key);
    *world.resource_mut::<OrgForm>() = OrgForm {
        profile: Some(world.resource::<OpenOrg>().session.org().profile.clone()),
        ..Default::default()
    };
    let theme = world.resource::<Theme>().clone();
    let tab = world.resource::<OpenOrg>().tab;
    // What the modules show that isn't in the document: pictures.
    let logo = world
        .resource::<OpenOrg>()
        .session
        .org()
        .profile
        .logo
        .and_then(|l| crate::org_logo::handle(world, &l.thumb));
    let share = (tab == OrgTab::Overview)
        .then(|| share_link(world))
        .flatten();
    let map = (tab == OrgTab::Locations)
        .then(|| crate::org_profile::locations_map(world.resource::<OpenOrg>().session.org()))
        .flatten()
        .map(|image| world.resource_mut::<Assets<Image>>().add(image));
    make_nav(world, nav, &theme);
    // The organization's wallet, while its module shows: asked for, kept fresh, and the bank
    // told who may spend from it (by the device that holds the organization's key).
    let wallet = shows_wallet(tab)
        .then(|| world.resource::<OpenOrg>().session.org().org)
        .flatten()
        .filter(|_| world.contains_resource::<crate::credits::Credits>())
        .map(|account| {
            let org = world.resource::<OpenOrg>().session.org().clone();
            crate::credits::say_access(world, account, &org);
            // Its orders' payments are asked after by name: the newest quoted ones.
            let mut quoted: Vec<(u32, cadrs_credits::MoveId)> = org
                .requests
                .iter()
                .filter_map(|(id, r)| {
                    let quote = r.quote.as_ref()?;
                    Some((
                        r.number,
                        cadrs_credits::order_hold(&r.client, *id, quote.id),
                    ))
                })
                .collect();
            quoted.sort_by_key(|(number, _)| std::cmp::Reverse(*number));
            let holds = quoted.into_iter().map(|(_, hold)| hold).collect();
            crate::credits::name_holds(world, account, holds);
            crate::credits::watch(world, account);
            crate::wallet_ui::WalletView::read(world, account, true)
        });
    let open = world.resource::<OpenOrg>();
    let social = world.resource::<crate::social::Social>();
    let org = open.session.org();
    let t = theme.clone();
    let newer = org.newer_ops;
    // The commands borrow the world: the modules read the organization through a copy.
    let snapshot = ModuleView {
        org: org.clone(),
        channel: open.channel.clone(),
        dir: open.dir(),
        contacts: social.contacts.clone(),
        me: social.keys().map(|k| k.account_id()),
        my_name: world.resource::<crate::UserProfile>().display_name.clone(),
        holds_key: org
            .org
            .is_some_and(|account| social.org_card(&account).is_some()),
        logo,
        share,
        map,
        draft,
        wallet,
    };
    let mut commands = world.commands();
    commands.entity(body).despawn_related::<Children>();
    commands.entity(body).with_children(|b| {
        if newer > 0 {
            banner(b, &t, tr("org-newer-cadrs"));
        }
        let v = &snapshot;
        match tab {
            OrgTab::Overview => crate::org_profile::overview(b, &t, v),
            OrgTab::Profile => crate::org_profile::profile(b, &t, v),
            OrgTab::Locations => crate::org_profile::locations(b, &t, v),
            OrgTab::Members => members(b, &t, v),
            OrgTab::Machines => machines(b, &t, v),
            OrgTab::Stock => stock(b, &t, v),
            OrgTab::Prices => prices(b, &t, v),
            OrgTab::Requests => requests(b, &t, v),
            OrgTab::Jobs => crate::org_tabs::jobs(b, &t, v),
            OrgTab::Clients => crate::org_tabs::clients(b, &t, v),
            OrgTab::Parts => crate::org_tabs::parts(b, &t, v),
            OrgTab::Chat => crate::org_tabs::chat(b, &t, v),
            OrgTab::Wallet => crate::wallet_ui::org_page(b, &t, v),
            OrgTab::Issues => crate::org_quality::issues(b, &t, v),
        }
    });
    world.flush();
}

/// What a module is drawn from: the organization as it is, and what the app knows around it.
pub(crate) struct ModuleView {
    pub org: cadrs_org::Org,
    pub channel: String,
    /// The organization's folder (its blobs).
    pub dir: PathBuf,
    pub contacts: cadrs_social::contact::Contacts,
    /// The user's own account.
    pub me: Option<cadrs_social::keys::AccountId>,
    /// The user's own name, shown for their own account.
    pub my_name: String,
    /// This device holds the organization's key: it can share its card and sign for it.
    pub holds_key: bool,
    /// The logo's small picture, when it is here.
    pub logo: Option<Handle<Image>>,
    /// The organization's card as a link and a QR code (the Overview).
    pub share: Option<(String, Option<Handle<Image>>)>,
    /// The world with the locations that say where they are (Locations).
    pub map: Option<Handle<Image>>,
    /// The chat message being written when the module was made again, and whether the keyboard
    /// was in its box: both are kept.
    pub draft: Option<(String, bool)>,
    /// The organization's wallet as the bank last said (the Wallet module).
    pub wallet: Option<crate::wallet_ui::WalletView>,
}

impl ModuleView {
    /// What to call an account ([`account_label`]).
    pub fn account_name(&self, account: &cadrs_social::keys::AccountId) -> String {
        account_label(&self.contacts, self.me, &self.my_name, account)
    }

    /// What to call a request's client: as [`ModuleView::account_name`], else the name on the
    /// card the request came with.
    pub fn client_name(&self, request: &cadrs_org::Request) -> String {
        if Some(request.client) == self.me || self.contacts.get(&request.client).is_some() {
            return self.account_name(&request.client);
        }
        request
            .card
            .as_ref()
            .map(|c| c.value.name.trim())
            .filter(|name| !name.is_empty())
            .map_or_else(|| short(&request.client), str::to_owned)
    }
}

/// What to call an account: the user's own name for their own, a contact's name, else the start
/// of its id.
pub(crate) fn account_label(
    contacts: &cadrs_social::contact::Contacts,
    me: Option<cadrs_social::keys::AccountId>,
    my_name: &str,
    account: &cadrs_social::keys::AccountId,
) -> String {
    if Some(*account) == me && !my_name.trim().is_empty() {
        return my_name.trim().to_owned();
    }
    contacts
        .get(account)
        .map_or_else(|| short(account), |c| c.name.clone())
}

/// Who is who, for a dialog that names accounts ([`account_label`]).
#[derive(Clone)]
pub(crate) struct Names {
    contacts: cadrs_social::contact::Contacts,
    pub me: Option<cadrs_social::keys::AccountId>,
    my_name: String,
}

impl Names {
    pub fn read(world: &World) -> Self {
        let social = world.resource::<crate::social::Social>();
        Self {
            contacts: social.contacts.clone(),
            me: social.keys().map(|k| k.account_id()),
            my_name: world.resource::<crate::UserProfile>().display_name.clone(),
        }
    }

    pub fn of(&self, account: &cadrs_social::keys::AccountId) -> String {
        account_label(&self.contacts, self.me, &self.my_name, account)
    }
}

/// What the open module's fields held when it was made, to tell whether the person changed
/// any since ([`track_form`]).
#[derive(Resource, Default)]
pub(crate) struct OrgForm {
    /// Frames since the module was made: its fields exist from the second.
    age: u8,
    texts: std::collections::HashMap<Entity, String>,
    ticks: std::collections::HashMap<Entity, bool>,
    /// A field doesn't hold what it was made with.
    pub dirty: bool,
    /// The person asked to see what arrived meanwhile, whatever the module holds.
    show: bool,
    /// The profile as it was when the module was made: what a Save of the Profile form compares
    /// with, so that only what the person changed is written (and not, with it, the older
    /// values of fields someone else changed since).
    pub(crate) profile: Option<Profile>,
}

/// Notes the module's fields as they were made, then whether any changed.
#[allow(clippy::type_complexity)]
fn track_form(
    mut form: ResMut<OrgForm>,
    body: Query<Entity, With<OrgBody>>,
    parents: Query<&ChildOf>,
    texts: Query<(Entity, &EditableText), With<TextInputField>>,
    ticks: Query<(Entity, &cadrs_ui::checkbox::CheckboxState)>,
    changed: Query<
        (),
        Or<(
            Changed<EditableText>,
            Changed<cadrs_ui::checkbox::CheckboxState>,
        )>,
    >,
) {
    let Some(body) = body.iter().next() else {
        return;
    };
    let inside = |entity: Entity| parents.iter_ancestors(entity).any(|e| e == body);
    if form.age < 2 {
        form.age += 1;
        if form.age == 2 {
            form.texts = texts
                .iter()
                .filter(|(e, _)| inside(*e))
                .map(|(e, text)| (e, text.value().to_string()))
                .collect();
            form.ticks = ticks
                .iter()
                .filter(|(e, _)| inside(*e))
                .map(|(e, tick)| (e, tick.checked))
                .collect();
        }
        return;
    }
    if form.dirty || changed.is_empty() {
        return;
    }
    let dirty = texts.iter().any(|(e, text)| {
        form.texts
            .get(&e)
            .is_some_and(|was| *was != text.value().to_string())
    }) || ticks
        .iter()
        .any(|(e, tick)| form.ticks.get(&e).is_some_and(|was| *was != tick.checked));
    if dirty {
        form.dirty = true;
    }
}

/// Whether the keyboard is in a text field of `body`.
fn typing_in(world: &World, body: Entity) -> bool {
    let Some(mut entity) = world.get_resource::<InputFocus>().and_then(|f| f.get()) else {
        return false;
    };
    if world.get::<TextInputField>(entity).is_none() {
        return false;
    }
    for _ in 0..64 {
        if entity == body {
            return true;
        }
        match world.get::<ChildOf>(entity) {
            Some(parent) => entity = parent.parent(),
            None => return false,
        }
    }
    false
}

/// The organization's card as a link and a QR code, when this device holds its key.
fn share_link(world: &mut World) -> Option<(String, Option<Handle<Image>>)> {
    let account = world.resource::<OpenOrg>().session.org().org?;
    let card = world
        .resource::<crate::social::Social>()
        .org_card(&account)?;
    let link = cadrs_social::contact::to_link(&card);
    let qr = cadrs_ui::qr::image(&link).map(|i| world.resource_mut::<Assets<Image>>().add(i));
    Some((link, qr))
}

/// A module's heading, with what it is for under it.
pub(crate) fn page_head(b: &mut ChildSpawnerCommands, t: &Theme, title: &str, about: &str) {
    b.spawn((
        Name::new("org-page-title"),
        t.text(tr(title), t.font_xl, FontWeight::SEMIBOLD, t.foreground),
    ));
    b.spawn((t.paragraph_within(tr(about), t.font_base, t.muted_foreground, 720.0),));
}

/// A smaller heading inside a module.
pub(crate) fn subheading(b: &mut ChildSpawnerCommands, t: &Theme, key: &str) {
    b.spawn((
        t.text(tr(key), t.font_md, FontWeight::BOLD, t.foreground),
        Node {
            margin: UiRect::top(Val::Px(10.0)),
            ..default()
        },
    ));
}

pub(crate) fn muted(b: &mut ChildSpawnerCommands, t: &Theme, text: impl Into<String>) {
    b.spawn((t.paragraph_within(text, t.font_sm, t.muted_foreground, 720.0),));
}

pub(crate) fn label(b: &mut ChildSpawnerCommands, t: &Theme, key: &str) {
    b.spawn(t.text(tr(key), t.font_sm, FontWeight::BOLD, t.foreground));
}

/// A note across the top of a module: something the person should know before they go on.
pub(crate) fn banner(b: &mut ChildSpawnerCommands, t: &Theme, text: String) {
    b.spawn((
        Name::new("org-banner"),
        Node {
            padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            max_width: Val::Px(720.0),
            ..default()
        },
        BackgroundColor(t.list_hover),
        BorderColor::all(t.border),
    ))
    .with_children(|r| {
        r.spawn(cadrs_ui::icon("info", 16.0, t.foreground));
        r.spawn(t.paragraph(text, t.font_sm, t.foreground));
    });
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

/// A list as a table: its header, then a row an item (`rows` gives each row its cells), or
/// `empty` when there are none. The table is `name`, its rows `<name>-row-<i>`.
pub(crate) fn table(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    name: &'static str,
    columns: Vec<Column>,
    rows: Vec<TableRow>,
    empty: &str,
) {
    if rows.is_empty() {
        muted(b, t, tr(empty));
        return;
    }
    let width = cadrs_ui::table::total_width(&columns);
    b.spawn((
        Name::new(name),
        Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(width),
            max_width: Val::Percent(100.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(t.border),
    ))
    .with_children(|table| {
        table.spawn(TableHeader::new(format!("{name}-header"), columns).build(t));
        for row in rows {
            table.spawn(row.build(t));
        }
    });
}

/// A table's row of text cells, to which button cells are added ([`TableRow::cell`]).
pub(crate) fn text_row(
    t: &Theme,
    name: String,
    columns: &[Column],
    cells: impl IntoIterator<Item = String>,
) -> TableRow {
    cells
        .into_iter()
        .fold(TableRow::new(name, columns), |row, cell| {
            row.text_cell(t, cell)
        })
}

/// A small button in a table's cell, running `act` on the world.
pub(crate) fn cell_button(
    c: &mut ChildSpawner,
    name: String,
    label: String,
    primary: bool,
    act: impl Fn(&mut World) + Send + Sync + Clone + 'static,
) {
    let t = c.world().resource::<Theme>().clone();
    let mut button = Button::new(name).label(label).small();
    button = if primary {
        button.primary()
    } else {
        button.ghost()
    };
    c.spawn((
        button.build(&t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            let act = act.clone();
            commands.queue(move |w: &mut World| act(w));
        }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currencies_are_three_letters() {
        assert_eq!(currency_code(" zar "), Some("ZAR".into()));
        assert_eq!(currency_code("EUR"), Some("EUR".into()));
        for bad in ["", "R", "rand", "Z4R", "€€€"] {
            assert_eq!(currency_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn requests_are_numbered_as_people_say_them() {
        assert_eq!(request_number(7), "R-0007");
        assert_eq!(request_number(12345), "R-12345");
    }

    /// Every module of the navigation has a key of its own, and each group opens once.
    #[test]
    fn the_modules_are_distinct() {
        let mut keys: Vec<&str> = MODULES.iter().map(|m| m.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), MODULES.len());
        let groups: Vec<&str> = MODULES.iter().filter_map(|m| m.group).collect();
        let mut distinct = groups.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), groups.len());
        assert_eq!(MODULES[0].tab, OrgTab::Overview);
    }
}
