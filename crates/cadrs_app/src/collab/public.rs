//! Public documents ([`cadrs_net::public`]): Share ▸ **Public** publishes a document as it is,
//! the documents page's Public documents panel lists everyone's (`crate::landing`), and from
//! there a document is starred, viewed or forked.
//!
//! - **Publish** packs the document as a share's snapshot does (with its imported files) and
//!   lists it, signed with this user's key; [`cadrs_core::collab::Published`] in its metadata
//!   remembers the revision. **Publish again** makes the next revision; **Make private**
//!   withdraws it. Only the owner publishes, and a fork is its forker's own document.
//! - **View** opens the revision read-only, unsaved, with a banner (`public-view-banner`)
//!   whose **Fork** and **Close** do what they say.
//! - **Fork** saves the revision as a new document of this user's, with
//!   [`cadrs_core::collab::ForkedFrom`] naming where it came from, and opens it.
//! - The user's name (the account menu's Edit) goes with their public documents as a signed
//!   profile, said again whenever it changes, so peers show the name as it is now.
//! - The peers to start from in the public room ([`PublicPeers`]) are the seeds (cadrs's own
//!   and `public/seeds.txt`, one cadrs ID a line, from the Preferences dialog) and the
//!   contacts; the Preferences' *Public documents* switch says whether to take part at all.
//!
//! Names: `public-view-banner`, `public-view-banner-text`, `public-view-fork`,
//! `public-view-close`.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::collab::{self, ForkedFrom, Published, Snapshot};
use cadrs_core::{DocumentId, DocumentMeta};
use cadrs_net::public::{
    self as net, Draft, Key, Listing, Origin, PublicEnabled, PublicPeers, RevisionFetch, Shared,
};
use cadrs_ui::Button;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

use super::sharing::{self, ContactBook};
use crate::preferences_ui::LocalPreferences;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

/// The folder, next to the documents, where the public directory and revisions are kept.
pub const DIR: &str = "public";
/// The user's own seed peers, one cadrs ID a line.
const SEEDS_FILE: &str = "seeds.txt";

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Shared>()
        .init_resource::<Fetches>()
        .init_resource::<Seeds>()
        // The room is joined with the seeds and contacts (`cadrs_net::public::PublicSystems`
        // run only where cadrs can go online).
        .add_systems(
            Update,
            (open_directory, keep_peers)
                .chain()
                .before(cadrs_net::public::PublicSystems),
        )
        .add_systems(Update, (poll_fetches, keep_profile))
        .add_systems(
            Update,
            sync_view_banner.run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<PublicView>();
        });
}

/// The user's own seed peers (`public/seeds.txt`).
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct Seeds(pub Vec<String>);

impl Seeds {
    fn path(root: &std::path::Path) -> std::path::PathBuf {
        root.join(DIR).join(SEEDS_FILE)
    }

    fn load(root: &std::path::Path) -> Self {
        let text = cadrs_fs::read_to_string(Self::path(root)).unwrap_or_default();
        Self::parse(&text)
    }

    /// The cadrs IDs in `text`: one a line, or separated by commas or spaces.
    pub fn parse(text: &str) -> Self {
        let mut out: Vec<String> = Vec::new();
        for id in text.split(|c: char| c.is_whitespace() || c == ',') {
            let id = id.trim();
            if !id.is_empty() && !out.iter().any(|x| x == id) {
                out.push(id.to_string());
            }
        }
        Self(out)
    }

    pub fn save(&self, root: &std::path::Path) -> std::io::Result<()> {
        cadrs_fs::create_dir_all(root.join(DIR))?;
        let mut text = self.0.join("\n");
        text.push('\n');
        cadrs_fs::write(Self::path(root), text)
    }
}

/// Opens the directory kept with the documents, once there is a store.
fn open_directory(
    shared: Res<Shared>,
    store: Option<Res<DocumentStore>>,
    clock: Res<AppClock>,
    mut seeds: ResMut<Seeds>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(store) = store else {
        return;
    };
    *done = true;
    let root = store.0.root().to_path_buf();
    shared.open(root.join(DIR), clock.now());
    *seeds = Seeds::load(&root);
}

/// Who the public room starts from, and whether to take part: kept with the preferences, the
/// seeds and the contacts (only in runs that can go online).
fn keep_peers(
    prefs: Res<LocalPreferences>,
    seeds: Res<Seeds>,
    contacts: Option<Res<ContactBook>>,
    peers: Option<ResMut<PublicPeers>>,
    enabled: Option<ResMut<PublicEnabled>>,
) {
    let (Some(mut peers), Some(mut enabled)) = (peers, enabled) else {
        return;
    };
    let contacts_changed = contacts.as_ref().is_some_and(|c| c.is_changed());
    if !(prefs.is_changed() || seeds.is_changed() || contacts_changed || peers.is_added()) {
        return;
    }
    let want = prefs.0.public_documents;
    if enabled.0 != want {
        enabled.0 = want;
    }
    let mut ids = seeds.0.clone();
    if let Some(c) = &contacts {
        ids.extend(c.0.0.iter().map(|c| c.id.clone()));
    }
    let addrs = PublicPeers::seeds(&ids);
    if peers.0 != addrs {
        peers.0 = addrs;
    }
}

/// The user's name for their public documents ([`cadrs_net::public::Profile`]): said again
/// whenever it changes (the account menu's Edit), or cadrs goes online, while they have public
/// documents, so every peer shows the name as it is now.
fn keep_profile(world: &mut World, mut said: Local<Option<(String, bool)>>) {
    let name = world.resource::<UserProfile>().display_name.clone();
    let me = cadrs_net::cadrs_id(world.get_resource::<cadrs_net::bevy_iroh::Iroh>());
    let now = (name.clone(), me.is_some());
    if said.as_ref() == Some(&now) {
        return;
    }
    *said = Some(now);
    let Some(me) = me else {
        return;
    };
    let publishes = !world
        .resource::<Shared>()
        .lock()
        .directory
        .signed_by(&me)
        .listings
        .is_empty();
    if publishes && let Err(why) = net::say_profile(world, &name) {
        warn!("cannot say the profile's name: {why}");
    }
}

// ---------------------------------------------------------------------------------------------
// Publishing

/// Publishes document `id` as it is now (the open copy if it is open): its first revision, or
/// the next.
pub fn publish(world: &mut World, id: DocumentId) -> Result<Listing, String> {
    let (doc, mut meta) = sharing::read(world, id).ok_or_else(|| tr("shell-collab-only-saved"))?;
    if let Some(s) = &meta.shared {
        return Err(tr_with(
            "shell-public-only-owner",
            &[("owner", s.owner_name.as_str())],
        ));
    }
    let store = world.resource::<DocumentStore>().0.clone();
    let snapshot = Snapshot::of(&doc, 0, &store)?;
    let bytes = collab::pack(&snapshot)?;
    let picture = picture_now(world, id)
        .or_else(|| cadrs_fs::read(store.thumbnail_path(id)).ok())
        .unwrap_or_default();
    let owner_name = world.resource::<UserProfile>().display_name.clone();
    let draft = Draft {
        document: id,
        name: doc.name.clone(),
        description: meta.description.clone(),
        owner_name: owner_name.clone(),
        picture,
        forked_from: meta.forked_from.as_ref().map(|f| Origin {
            document: f.document,
            owner: f.owner.clone(),
            owner_name: f.owner_name.clone(),
            name: f.name.clone(),
        }),
        previous: meta.published.as_ref().map_or(0, |p| p.revision),
    };
    let listing = net::publish(world, draft, bytes)?;
    // The name the panel shows for all of this user's documents.
    if let Err(why) = net::say_profile(world, &owner_name) {
        warn!("cannot say the profile's name: {why}");
    }
    meta.published = Some(Published {
        revision: listing.revision,
        time: listing.published,
    });
    sharing::write_meta(world, id, meta)?;
    Ok(listing)
}

/// A picture of the open document's parts as they are now, framed as the documents page's
/// thumbnails. The stored one is drawn only when a document closes, so one published before it
/// was ever closed would carry a placeholder.
fn picture_now(world: &World, id: DocumentId) -> Option<Vec<u8>> {
    let d = world
        .get_resource::<ActiveDocument>()
        .filter(|d| d.doc.id == id)?;
    let el = d.active.and_then(|e| d.doc.element(e))?;
    let parts = &world.get_resource::<crate::parts::PartCache>()?.parts;
    let list: Vec<(&cadrs_core::Solid, [u8; 3])> = parts
        .iter()
        .map(|p| {
            let look = cadrs_core::appearance::part_appearance(p, el.part_props());
            (&*p.solid, look.rgb)
        })
        .collect();
    if list.is_empty() {
        return None;
    }
    let (w, h) = (300, 170);
    let square = cadrs_core::assembly::thumb::render(&list, h);
    let mut picture = image::RgbaImage::new(w, h);
    image::imageops::overlay(&mut picture, &square, i64::from((w - h) / 2), 0);
    let mut png = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// Makes document `id` private again: its listing is withdrawn everywhere it reaches.
pub fn make_private(world: &mut World, id: DocumentId) -> Result<(), String> {
    let (_, mut meta) = sharing::read(world, id).ok_or_else(|| tr("shell-collab-only-saved"))?;
    net::withdraw(world, id)?;
    meta.published = None;
    sharing::write_meta(world, id, meta)
}

/// How many star document `id` of this user's, as far as this cadrs has heard (`None` while
/// offline: whose it is depends on the key).
pub fn own_stars(world: &World, id: DocumentId) -> Option<usize> {
    let me = cadrs_net::cadrs_id(world.get_resource::<cadrs_net::bevy_iroh::Iroh>())?;
    Some(world.resource::<Shared>().lock().directory.stars(&(me, id)))
}

// ---------------------------------------------------------------------------------------------
// Stars, views and forks

/// This user's cadrs ID, or an offer to connect (with what it is needed for).
fn online_as(world: &mut World) -> Option<String> {
    let id = cadrs_net::cadrs_id(world.get_resource::<cadrs_net::bevy_iroh::Iroh>());
    if id.is_none() {
        crate::network::offer_connect(
            world,
            &tr("shell-public-net-off"),
            &tr("shell-public-offline"),
            |_| {},
        );
    }
    id
}

/// Stars a public document, or takes the star back.
pub fn toggle_star(world: &mut World, key: &Key) {
    let Some(me) = online_as(world) else {
        return;
    };
    let starred = world
        .resource::<Shared>()
        .lock()
        .directory
        .starred_by(key, &me);
    if let Err(why) = net::star(world, key, !starred) {
        crate::linked::toast(world, why);
    }
}

/// What to do with a revision once it arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    View,
    Fork,
}

/// Revisions on their way.
#[derive(Resource, Default)]
struct Fetches(Vec<(RevisionFetch, Listing, Then)>);

fn start(world: &mut World, key: &Key, then: Then) {
    let Some(listing) = world
        .resource::<Shared>()
        .lock()
        .directory
        .get(key)
        .map(|h| h.listing.clone())
    else {
        crate::linked::toast(world, tr("shell-public-gone"));
        return;
    };
    if world
        .resource::<Fetches>()
        .0
        .iter()
        .any(|(f, _, t)| f.key == *key && *t == then)
    {
        return;
    }
    match net::fetch(world, key) {
        Ok(f) => {
            let name = listing.name.as_str();
            let text = match then {
                Then::View => tr_with("shell-public-opening", &[("name", name)]),
                Then::Fork => tr_with("shell-public-forking", &[("name", name)]),
            };
            // Kept here already: no need to say it is coming.
            if !world
                .resource::<Shared>()
                .lock()
                .has_revision(&listing.hash)
            {
                crate::linked::toast(world, text);
            }
            world.resource_mut::<Fetches>().0.push((f, listing, then));
        }
        Err(why) => {
            if world.get_resource::<cadrs_net::bevy_iroh::Iroh>().is_none() {
                online_as(world);
            } else {
                crate::linked::toast(world, why);
            }
        }
    }
}

/// Opens a public document read-only, as it was published.
pub fn view(world: &mut World, key: &Key) {
    start(world, key, Then::View);
}

/// Saves a public document as a new document of this user's, and opens it.
pub fn fork(world: &mut World, key: &Key) {
    start(world, key, Then::Fork);
}

fn poll_fetches(world: &mut World) {
    let shared = world.resource::<Shared>().clone();
    let mut done = Vec::new();
    world
        .resource_mut::<Fetches>()
        .0
        .retain_mut(|(f, listing, then)| match f.poll(&shared) {
            None => true,
            Some(r) => {
                done.push((r, listing.clone(), *then));
                false
            }
        });
    for (r, listing, then) in done {
        let name = listing.name.clone();
        let r = r.and_then(|bytes| match then {
            Then::View => open_view(world, &listing, &bytes),
            Then::Fork => keep_fork(world, &listing, &bytes).map(|id| {
                world.remove_resource::<PublicView>();
                crate::move_document::open_document(world, id);
                crate::landing::reload_library(world);
            }),
        });
        if let Err(why) = r {
            warn!("public document \"{name}\": {why}");
            crate::linked::toast(
                world,
                tr_with(
                    "shell-public-cannot-open",
                    &[("name", name.as_str()), ("why", why.as_str())],
                ),
            );
        }
    }
}

/// The document a revision holds (its imported files put in the cache).
fn unpack(bytes: &[u8]) -> Result<cadrs_core::Document, String> {
    collab::unpack::<Snapshot>(bytes)?.install()
}

/// The read-only view of a public document: which one, for its banner.
#[derive(Resource, Debug, Clone)]
pub struct PublicView {
    pub key: Key,
    pub name: String,
    pub owner_name: String,
    pub revision: u32,
}

fn open_view(world: &mut World, listing: &Listing, bytes: &[u8]) -> Result<(), String> {
    let doc = unpack(bytes)?;
    // Leave whatever is open as it would be left for the documents page.
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = d.save_if_changed(&store, now, &user)
    {
        warn!("cannot save before viewing a public document: {e}");
    }
    let mut active = ActiveDocument::new(doc);
    active.read_only = Some(tr_with(
        "shell-public-view-label",
        &[
            ("name", listing.name.as_str()),
            ("owner", listing.owner_name.as_str()),
        ],
    ));
    world.insert_resource(active);
    world.insert_resource(PublicView {
        key: listing.key(),
        name: listing.name.clone(),
        owner_name: listing.owner_name.clone(),
        revision: listing.revision,
    });
    if *world.resource::<State<AppState>>().get() != AppState::Document {
        world
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Document);
    }
    Ok(())
}

/// Stores a public revision as a new document of this user's.
fn keep_fork(world: &mut World, listing: &Listing, bytes: &[u8]) -> Result<DocumentId, String> {
    let mut doc = unpack(bytes)?;
    let id = DocumentId::new();
    doc.id = id;
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let mut meta = DocumentMeta::new(&user, now);
    meta.description = listing.description.clone();
    meta.forked_from = Some(ForkedFrom {
        document: listing.document,
        owner: listing.owner.clone(),
        owner_name: listing.owner_name.clone(),
        name: listing.name.clone(),
        revision: listing.revision,
    });
    store.create(&doc, &meta).map_err(|e| e.to_string())?;
    let bytes = listing.picture.and_then(|h| {
        world
            .resource::<Shared>()
            .lock()
            .directory
            .picture(&h)
            .cloned()
    });
    let picture = bytes
        .and_then(|b| image::load_from_memory(&b).ok())
        .map(|i| i.to_rgba8())
        .unwrap_or_else(|| {
            cadrs_core::thumbnail::placeholder(cadrs_core::store::thumbnail_seed(&doc.name, now))
        });
    if let Err(e) = store.write_thumbnail(id, &picture) {
        warn!("cannot keep the fork's picture: {e}");
    }
    info!(
        "forked \"{}\" (revision {} by {}) as {id}",
        listing.name, listing.revision, listing.owner_name
    );
    Ok(id)
}

// ---------------------------------------------------------------------------------------------
// The view's banner

#[derive(Component)]
struct ViewBanner(String);

fn sync_view_banner(
    view: Option<Res<PublicView>>,
    doc: Option<Res<ActiveDocument>>,
    q: Query<(Entity, &ViewBanner)>,
    area: Query<Entity, With<ViewportArea>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let want = match (&view, &doc) {
        (Some(v), Some(d)) if d.read_only.is_some() && d.doc.id == v.key.1 => {
            Some(format!("{}|{}|{}", v.name, v.owner_name, v.revision))
        }
        _ => None,
    };
    let have = q.iter().next().map(|(e, b)| (e, b.0.clone()));
    if have.as_ref().map(|h| &h.1) == want.as_ref() {
        return;
    }
    if let Some((e, _)) = have {
        commands.entity(e).despawn();
    }
    let (Some(v), Some(key)) = (view, want) else {
        return;
    };
    let Some(area) = area.iter().next() else {
        return;
    };
    let t = &*theme;
    let text = tr_with(
        "shell-public-view-banner",
        &[
            ("name", v.name.as_str()),
            ("owner", v.owner_name.as_str()),
            ("revision", &v.revision.to_string()),
        ],
    );
    let doc_key = v.key.clone();
    // A strip at the bottom of the graphics area, the banner centred in it (as a linked
    // document's).
    let banner = commands
        .spawn((
            ViewBanner(key),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(14.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(40),
        ))
        .with_children(|w| {
            w.spawn((
                Name::new("public-view-banner"),
                Node {
                    max_width: Val::Px(760.0),
                    min_height: Val::Px(34.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(t.popover),
                BorderColor::all(t.border),
            ))
            .with_children(|b| {
                b.spawn(cadrs_ui::icon::icon("public", 16.0, t.foreground));
                b.spawn((
                    Name::new("public-view-banner-text"),
                    t.text(text, t.font_base, FontWeight::NORMAL, t.foreground),
                ));
                b.spawn((
                    Button::new("public-view-fork")
                        .label(tr("shell-public-fork"))
                        .icon("copy")
                        .primary()
                        .small()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        let k = doc_key.clone();
                        commands.queue(move |w: &mut World| fork(w, &k));
                    }),
                ));
                b.spawn((
                    Button::new("public-view-close")
                        .label(tr("common-close"))
                        .small()
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(crate::thumbnail::close_document);
                    }),
                ));
            });
        })
        .id();
    commands.entity(area).add_child(banner);
}

// ---------------------------------------------------------------------------------------------
// Scenarios

/// `public …` (scenarios, `crate::script`): `public sample <n>` lists `n` made-up public
/// documents (signed by keys made for them, each revision a small document kept here, with
/// stars), so the panel, View and Fork can be shown without the network; `public view <n>`
/// and `public fork <n>` view or fork the n-th (1-based, by stars).
pub fn script(world: &mut World, args: &str) {
    let words: Vec<&str> = args.split_whitespace().collect();
    match words.as_slice() {
        ["sample", n] => match n.parse::<usize>() {
            Ok(n) => sample(world, n),
            Err(_) => warn!("public sample {n}: not a number"),
        },
        [what @ ("view" | "fork"), n] => {
            let Some(i) = n.parse::<usize>().ok().filter(|i| *i > 0) else {
                warn!("public {what} {n}: a number from 1");
                return;
            };
            let mut docs = world.resource::<Shared>().lock().directory.docs("");
            docs.sort_by(|a, b| {
                b.stars
                    .cmp(&a.stars)
                    .then(a.listing.name.cmp(&b.listing.name))
            });
            let Some(d) = docs.get(i - 1) else {
                warn!("public {what} {i}: only {} listed", docs.len());
                return;
            };
            let key = d.listing.key();
            if *what == "view" {
                view(world, &key);
            } else {
                fork(world, &key);
            }
        }
        _ => warn!("public {args}: sample <n>, view <n> or fork <n>"),
    }
}

/// Made-up public documents, for scenarios.
fn sample(world: &mut World, n: usize) {
    use cadrs_net::bevy_iroh::iroh::SecretKey;
    use cadrs_net::public::{Signed, Star};
    const NAMES: [(&str, &str, &str); 8] = [
        (
            "Desk lamp arm",
            "Mira Okafor",
            "A two-joint arm with friction hinges.",
        ),
        (
            "Raspberry Pi 5 case",
            "Jonas Berg",
            "Snap-fit case with a fan mount.",
        ),
        (
            "Planetary gearbox 4:1",
            "Lena Fischer",
            "Printable, with M3 hardware.",
        ),
        ("Cable clip", "Sam Ito", ""),
        ("Bike phone mount", "Ana Souza", "Fits 22–32 mm handlebars."),
        (
            "Drawer organiser",
            "Tom Hale",
            "Parametric grid: set the cell count.",
        ),
        ("Bracket", "Mira Okafor", "An L bracket with slots."),
        ("Hose adapter", "Priya Nair", ""),
    ];
    let now = world.resource::<AppClock>().now();
    let shared = world.resource::<Shared>().clone();
    let mut state = shared.lock();
    for i in 0..n {
        let (name, owner_name, description) = NAMES[i % NAMES.len()];
        let key = SecretKey::from_bytes(&[i as u8 + 1; 32]);
        let mut doc = cadrs_core::Document::new(name);
        doc.id = DocumentId::from_u128(0x5eed_0000 + i as u128);
        let Ok(bytes) = collab::pack(&Snapshot {
            seq: 0,
            document: doc.clone(),
            blobs: Vec::new(),
        }) else {
            continue;
        };
        let size = bytes.len() as u64;
        let hash = state.keep_revision(bytes);
        let mut png = Vec::new();
        let picture = cadrs_core::thumbnail::placeholder(i as u64 * 7919 + 3);
        let _ = picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
        let listing = Listing {
            document: doc.id,
            owner: key.public().to_string(),
            owner_name: owner_name.into(),
            name: name.into(),
            description: description.into(),
            revision: 1 + (i as u32 % 3),
            published: now - 3600 * (1 + 7 * i as i64),
            hash,
            size,
            picture: Some(*blake3::hash(&png).as_bytes()),
            forked_from: None,
        };
        if let Ok(signed) = Signed::new(&listing, &key) {
            let _ = state.directory.add_listing(&signed, now);
            state.directory.add_picture(png);
        }
        // Stars from made-up people: fewer down the list.
        for s in 0..(n - i) * 3 % 17 {
            let by = SecretKey::from_bytes(&[(100 + s) as u8; 32]);
            let star = Star {
                document: doc.id,
                owner: listing.owner.clone(),
                by: by.public().to_string(),
                starred: true,
                time: now - 60,
            };
            if let Ok(signed) = Signed::new(&star, &by) {
                let _ = state.directory.add_star(&signed, now);
            }
        }
    }
    state.changed();
}
