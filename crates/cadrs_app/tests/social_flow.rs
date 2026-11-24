//! A designer and a water-cutting shop, as two running apps over real iroh on this machine (no
//! relay): the shop founds its organization, sets up a waterjet and prices and shares its card;
//! the designer adds the card and sends a request for a quote; the shop's document gets it and
//! the bell rings; the shop quotes; the designer's book shows the price; the designer accepts;
//! the shop's document shows the order.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use cadrs_app::notifications::Notifications;
use cadrs_app::organization::{NewOrg, OpenOrg, create, instant_quote, write};
use cadrs_app::social::{PeerAddrs, Social, SocialPlugin, add_contact, ensure_loaded, send};
use cadrs_app::{AppClock, AppState, DocumentStore, UserProfile};
use cadrs_core::Store;
use cadrs_net::NetPlugin;
use cadrs_net::bevy_iroh::{Iroh, Relays};
use cadrs_org::capability::{MachineCaps, MaterialLimit, PartSummary, Process};
use cadrs_org::letters::{Message, Sent};
use cadrs_org::quote::{Money, RateCard};
use cadrs_org::{Op, Stage, new_id};

const PATIENCE: Duration = Duration::from_secs(30);

fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadrs-social-flow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app(name: &str) -> App {
    app_starting(name, true)
}

fn app_starting(name: &str, online: bool) -> App {
    let root = root(name);
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        NetPlugin {
            // The same key the app's social side signs with: the device is one id.
            identity: Some(
                root.join("documents")
                    .join(cadrs_core::identity::DEFAULT_KEY),
            ),
            display_name: name.into(),
            relays: Relays::Disabled,
            autostart: online,
        },
        SocialPlugin,
        cadrs_app::public_chat_ui::PublicChatPlugin,
    ))
    .init_state::<AppState>()
    .insert_resource(DocumentStore(Store::new(root.join("documents"))))
    .insert_resource(UserProfile {
        id: name.to_lowercase(),
        display_name: name.into(),
    })
    .insert_resource(AppClock::system())
    .insert_resource(cadrs_ui::theme::Theme::default())
    .init_resource::<Notifications>();
    for _ in 0..3 {
        app.update();
    }
    app
}

fn until(apps: &mut [&mut App], what: &str, mut done: impl FnMut(&mut [&mut App]) -> bool) {
    let start = Instant::now();
    loop {
        for a in apps.iter_mut() {
            a.update();
        }
        if done(apps) {
            return;
        }
        if start.elapsed() > PATIENCE {
            // What each app said (its toasts) before giving up.
            for (i, a) in apps.iter_mut().enumerate() {
                let mut q = a.world_mut().query::<&Text>();
                let texts: Vec<String> = q.iter(a.world()).map(|t| t.0.clone()).collect();
                eprintln!("app {i} says: {texts:?}");
            }
            panic!("gave up waiting: {what}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Puts `first` in the public room, then `second` through it (no rendezvous: only each other).
/// One after the other: a peer that asks to join through one that isn't in the room yet stays
/// alone, and gossip doesn't let it join later.
fn into_the_room(first: &mut App, second: &mut App) {
    use cadrs_net::public::{PublicPeers, Rendezvous, RoomState, room_state};
    first.insert_resource(Rendezvous(None));
    until(&mut [first], "the first is in the room", |a| {
        matches!(room_state(a[0].world()), RoomState::Joined(_))
    });
    let addr = first.world().resource::<Iroh>().addr().unwrap();
    second.insert_resource(Rendezvous(None));
    second.insert_resource(PublicPeers(vec![addr]));
    until(&mut [first, second], "both are in the room", |a| {
        a.iter()
            .all(|app| matches!(room_state(app.world()), RoomState::Joined(n) if n > 0))
    });
}

/// Each app knows where the other is (no relay to find it through).
fn introduce(a: &mut App, b: &App) {
    let addr = b.world().resource::<Iroh>().addr().expect("online");
    a.world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(addr.id, addr);
}

/// Where the test shops stand: a street in Stellenbosch.
const STREET: (f64, f64) = (-33.935712, 18.861123);

/// What the New organization dialog asks for, filled in: a manufacturing shop called `name`
/// with a logo, a tagline, an e-mail address and a workshop in Stellenbosch.
fn founded(name: &str) -> NewOrg {
    let mut picture = Vec::new();
    image::RgbaImage::from_fn(512, 512, |x, y| {
        image::Rgba([(x / 2) as u8, (y / 2) as u8, 180, 255])
    })
    .write_to(
        &mut std::io::Cursor::new(&mut picture),
        image::ImageFormat::Png,
    )
    .unwrap();
    NewOrg {
        name: name.into(),
        tagline: "Waterjet cutting in steel, aluminium and glass".into(),
        offers: cadrs_org::Offers {
            manufacturing: true,
            designs: false,
        },
        email: "sales@shop.example".into(),
        logo: cadrs_app::org_logo::process(&picture).expect("a logo"),
        location: Some(cadrs_org::Location {
            name: "Workshop".into(),
            production: true,
            address: cadrs_org::Address {
                city: "Stellenbosch".into(),
                country: "ZA".into(),
                ..Default::default()
            },
            point: Some(STREET),
            public: true,
            public_accuracy_m: 5_000.0,
            ..Default::default()
        }),
    }
}

/// A waterjet that cuts mild steel to 100 mm.
fn waterjet() -> MachineCaps {
    MachineCaps {
        name: "Waterjet".into(),
        process: Process::Waterjet,
        envelope: [3000.0, 1500.0, 0.0],
        materials: vec![MaterialLimit {
            material: "mild steel".into(),
            max_thickness: 100.0,
            speeds: vec![(10.0, 300.0)],
        }],
        kerf: 1.0,
        min_hole_ratio: 0.5,
        tolerance: 0.1,
        pierce_seconds: 4.0,
        location: None,
    }
}

/// The machine, the sheet and the prices a shop needs to quote the [`bracket`].
fn set_up(shop: &mut App) {
    let jet = new_id();
    for op in [
        Op::SetMachine {
            id: jet,
            machine: Some(waterjet()),
        },
        Op::SetMaterial {
            id: new_id(),
            material: Some(cadrs_org::Material {
                name: "mild steel".into(),
                thickness: 10.0,
                sheet: [2500.0, 1250.0],
            }),
        },
        Op::SetRates(RateCard {
            currency: "ZAR".into(),
            setup: Money::from_major(250.0),
            minimum: Money::from_major(500.0),
            hourly: vec![cadrs_org::quote::MachineRate::new(
                jet,
                Money::from_major(1200.0),
            )],
            sheet_per_m2: vec![cadrs_org::quote::SheetPrice::new(
                "mild steel",
                10.0,
                Money::from_major(1000.0),
            )],
            nesting_allowance: 1.25,
        }),
    ] {
        assert!(write(shop.world_mut(), op));
    }
}

fn bracket() -> PartSummary {
    PartSummary {
        name: "bracket".into(),
        flat_thickness: Some(10.0),
        size: [200.0, 80.0, 10.0],
        min_hole: Some(8.5),
        min_inside_radius: Some(3.0),
        cut_length: 900.0,
        pierces: 3,
        area: 12_000.0,
        material: Some("mild steel".into()),
        tolerance: None,
    }
}

#[test]
fn a_request_for_a_quote_goes_from_designer_to_shop_and_back() {
    let mut shop = app("Shop");
    let mut designer = app("Designer");
    introduce(&mut shop, &designer);
    introduce(&mut designer, &shop);

    // The shop founds its organization and sets it up.
    assert!(create(shop.world_mut(), founded("Cape Waterjet")));
    set_up(&mut shop);
    let org = shop
        .world()
        .resource::<OpenOrg>()
        .session
        .org()
        .org
        .unwrap();
    let card = shop
        .world()
        .resource::<Social>()
        .org_card(&org)
        .expect("the shop's card");

    // The designer adds the shop and asks for a quote.
    ensure_loaded(designer.world_mut());
    assert!(add_contact(designer.world_mut(), card.clone()));
    let name = "Designer".to_string();
    let my_card = designer
        .world()
        .resource::<Social>()
        .my_card(&name)
        .unwrap();
    let id = new_id();
    let parts = vec![(bracket(), 6)];
    designer.world_mut().resource_mut::<Social>().book.record(
        id,
        Sent {
            shop: org,
            shop_name: "Cape Waterjet".into(),
            parts: parts.clone(),
            notes: "brushed".into(),
            stage: Stage::Requested,
            quote: None,
            decline_reason: None,
            chat: vec![],
            sent: 0,
            delivery: None,
            review: None,
        },
    );
    let message = Message::Request {
        id,
        parts,
        notes: "brushed".into(),
        deadline: None,
        snapshot: b"a packed document".to_vec(),
        client: my_card,
    };
    assert!(send(
        designer.world_mut(),
        &card.value,
        message,
        None,
        "request".into()
    ));

    until(
        &mut [&mut shop, &mut designer],
        "the request reaches the shop",
        |a| {
            a[0].world()
                .resource::<OpenOrg>()
                .session
                .org()
                .requests
                .contains_key(&id)
        },
    );
    assert!(shop.world().resource::<Notifications>().unread());
    // The request survived on disk too.
    let open = shop.world().resource::<OpenOrg>();
    assert_eq!(open.session.org().requests[&id].notes, "brushed");

    // The shop quotes at its prices; the designer's book shows it.
    instant_quote(shop.world_mut(), id);
    until(
        &mut [&mut shop, &mut designer],
        "the quote reaches the designer",
        |a| a[1].world().resource::<Social>().book.sent[&id].stage == Stage::Quoted,
    );
    let quote = designer.world().resource::<Social>().book.sent[&id]
        .quote
        .clone()
        .unwrap();
    assert_eq!(quote.total, Money::from_major(724.0));
    assert!(designer.world().resource::<Notifications>().unread());

    // The designer accepts; the shop's document has the order.
    cadrs_app::request_quote::accept(designer.world_mut(), id);
    until(
        &mut [&mut shop, &mut designer],
        "the acceptance reaches the shop",
        |a| a[0].world().resource::<OpenOrg>().session.org().requests[&id].stage == Stage::Accepted,
    );
}

/// A phone joins the account of a laptop: the laptop offers, the phone asks, both show the same
/// code, the laptop confirms, and the phone's keys now carry the laptop's account.
#[test]
fn a_second_device_joins_the_account() {
    use cadrs_app::pairing_ui::{OfferText, ShownCode, confirm, join_with, open_link};
    let mut laptop = app("Laptop");
    let mut phone = app("Phone");
    introduce(&mut laptop, &phone);
    introduce(&mut phone, &laptop);
    ensure_loaded(laptop.world_mut());
    ensure_loaded(phone.world_mut());
    let account = |a: &App| a.world().resource::<Social>().keys().unwrap().account_id();
    assert_ne!(account(&laptop), account(&phone));

    open_link(laptop.world_mut());
    let offer = laptop.world().resource::<OfferText>().0.clone();
    join_with(phone.world_mut(), &offer);
    let phone_code = phone.world().resource::<ShownCode>().0.clone();
    until(
        &mut [&mut laptop, &mut phone],
        "the laptop hears the phone",
        |a| a[0].world().contains_resource::<ShownCode>(),
    );
    assert_eq!(laptop.world().resource::<ShownCode>().0, phone_code);

    confirm(laptop.world_mut());
    let want = account(&laptop);
    until(&mut [&mut laptop, &mut phone], "the phone joins", |a| {
        account(a[1]) == want
    });
    let keys = phone.world().resource::<Social>().keys().unwrap();
    assert!(
        keys.account.is_none(),
        "a paired device doesn't hold the account key"
    );
}

/// The shop is offline when the designer sends: the request is sealed and left at a mailbox
/// node; the shop comes online, collects it, opens it and has it in its document; the mailbox
/// gets a receipt for keeping it.
#[test]
fn a_request_waits_at_a_mailbox_while_the_shop_is_offline() {
    use cadrs_app::social::{MyMailboxes, collect_now};
    use cadrs_social::keys::{PublicKey, SecretKey};

    // A mailbox node: just the network, keeping a mailbox under its own key.
    let mailbox_root = root("Mailbox");
    let key_file = mailbox_root.join("identity.key");
    let mut node = App::new();
    node.add_plugins((
        MinimalPlugins,
        NetPlugin {
            identity: Some(key_file.clone()),
            display_name: "Mailbox".into(),
            relays: Relays::Disabled,
            autostart: true,
        },
    ));
    node.update();
    let key: [u8; 32] = std::fs::read(&key_file).unwrap().try_into().unwrap();
    cadrs_net::social::enable_mailbox(
        node.world_mut(),
        SecretKey::from_bytes(&key),
        Some(mailbox_root.join("mailbox.bin")),
        Default::default(),
    );
    let node_addr = node.world().resource::<Iroh>().addr().unwrap();
    let node_id = PublicKey(*node_addr.id.as_bytes());

    // The shop is set up offline; its card names the mailbox.
    let mut shop = app_starting("OfflineShop", false);
    shop.insert_resource(MyMailboxes(vec![node_id]));
    assert!(create(shop.world_mut(), founded("Night Shift Cutting")));
    let org = shop
        .world()
        .resource::<OpenOrg>()
        .session
        .org()
        .org
        .unwrap();
    let card = shop.world().resource::<Social>().org_card(&org).unwrap();
    assert_eq!(card.value.mailboxes, vec![node_id]);

    // The designer can reach the mailbox, not the shop.
    let mut designer = app("NightDesigner");
    designer
        .world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(node_addr.id, node_addr.clone());
    ensure_loaded(designer.world_mut());
    add_contact(designer.world_mut(), card.clone());
    let my_card = designer
        .world()
        .resource::<Social>()
        .my_card("Designer")
        .unwrap();
    let id = new_id();
    let message = Message::Request {
        id,
        parts: vec![(bracket(), 2)],
        notes: "no rush".into(),
        deadline: None,
        snapshot: vec![1, 2, 3],
        client: my_card,
    };
    assert!(send(
        designer.world_mut(),
        &card.value,
        message,
        None,
        "request".into()
    ));
    until(
        &mut [&mut node, &mut designer],
        "the request waits at the mailbox",
        |a| {
            a[0].world()
                .resource::<cadrs_net::social::Mailboxes>()
                .0
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .held()
                .0
                == 1
        },
    );

    // The shop comes online and collects it.
    shop.world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(node_addr.id, node_addr);
    let key = shop
        .world()
        .resource::<DocumentStore>()
        .0
        .root()
        .join(cadrs_core::identity::DEFAULT_KEY);
    cadrs_net::start(shop.world_mut(), Some(key), "OfflineShop".into()).unwrap();
    collect_now(shop.world_mut());
    until(
        &mut [&mut node, &mut shop],
        "the shop collects the request",
        |a| {
            a[1].world()
                .resource::<OpenOrg>()
                .session
                .org()
                .requests
                .contains_key(&id)
        },
    );
    assert_eq!(
        shop.world().resource::<OpenOrg>().session.org().requests[&id].notes,
        "no rush"
    );
    // The mailbox gave it up, and the shop holds its receipt for keeping it.
    let held = node
        .world()
        .resource::<cadrs_net::social::Mailboxes>()
        .0
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .held()
        .0;
    assert_eq!(held, 0);
    let receipts = shop.world().resource::<Social>().ledger.receipts().len();
    assert!(
        receipts >= 1,
        "the shop countersigned the mailbox's receipt"
    );
}

/// A shop that is complete and asks to be listed is heard of in the public room: the
/// designer's marketplace finds it, nearest first, with its place blurred to what the shop
/// chose, and fetches its logo from the shop. When the shop leaves the marketplace, it goes.
#[test]
fn a_listed_shop_is_found_in_the_marketplace() {
    use cadrs_org::directory::Query;
    use cadrs_social::place::Place;

    let mut shop = app("ListedShop");
    let mut designer = app("Finder");
    into_the_room(&mut shop, &mut designer);
    introduce(&mut designer, &shop);

    assert!(create(shop.world_mut(), founded("Stellenbosch Waterjet")));
    let listed = |shop: &App| shop.world().resource::<OpenOrg>().session.org().listed();
    let ask = |shop: &mut App, on: bool| {
        let profile = cadrs_org::Profile {
            listed: on,
            ..shop
                .world()
                .resource::<OpenOrg>()
                .session
                .org()
                .profile
                .clone()
        };
        assert!(write(shop.world_mut(), Op::SetProfile(profile)));
    };
    // Asking isn't enough: a shop that manufactures and has no machine isn't shown.
    ask(&mut shop, true);
    assert!(!listed(&shop));
    assert_eq!(
        cadrs_org::profile::missing(shop.world().resource::<OpenOrg>().session.org()),
        [cadrs_org::profile::Missing::Machine]
    );
    assert!(write(
        shop.world_mut(),
        Op::SetMachine {
            id: new_id(),
            machine: Some(waterjet()),
        }
    ));
    assert!(listed(&shop));

    ensure_loaded(designer.world_mut());
    until(
        &mut [&mut shop, &mut designer],
        "the listing is heard",
        |a| a[1].world().resource::<Social>().directory.len() == 1,
    );
    // The shop sees itself in its own marketplace, as others do.
    assert_eq!(shop.world().resource::<Social>().directory.len(), 1);
    let social = designer.world().resource::<Social>();
    let cape_town = Place::earth(-33.92, 18.42);
    let found = social.directory.search(&Query {
        text: "waterjet stellenbosch steel",
        near: Some(&cape_town),
        ..Default::default()
    });
    let listing = found[0].listing.clone();
    assert_eq!(listing.name(), "Stellenbosch Waterjet");
    assert_eq!(
        listing.tagline,
        "Waterjet cutting in steel, aluminium and glass"
    );
    assert_eq!(listing.email, "sales@shop.example");
    // The shop's device vouches, with its own key, that what it publishes is the shop's.
    let shop_device = shop
        .world()
        .resource::<Social>()
        .keys()
        .unwrap()
        .device_id();
    assert_eq!(listing.publishing_devices(), vec![shop_device]);
    let km = found[0].distance_m.unwrap() / 1000.0;
    assert!((30.0..55.0).contains(&km), "{km} km");
    let shown = listing.places[0].clone();
    assert_ne!(
        shown.frame,
        Place::earth(STREET.0, STREET.1).frame,
        "the street isn't public"
    );
    assert_eq!(
        listing.where_label().as_deref(),
        Some("Stellenbosch, South Africa")
    );

    // Its logo isn't in the listing: the designer's cadrs fetches it from the shop, and what
    // arrives is the picture the listing names.
    let logo = listing.logo.expect("a listed shop has a logo");
    assert!(cadrs_app::social::logo_bytes(designer.world(), &logo).is_none());
    cadrs_app::social::want_logo(designer.world_mut(), &listing.card.value, logo);
    until(&mut [&mut shop, &mut designer], "the logo arrives", |a| {
        cadrs_app::social::logo_bytes(a[1].world(), &logo).is_some()
    });
    let bytes = cadrs_app::social::logo_bytes(designer.world(), &logo).unwrap();
    assert_eq!(cadrs_org::BlobRef::of(&bytes), logo);
    let picture = image::load_from_memory(&bytes).unwrap();
    assert_eq!((picture.width(), picture.height()), (128, 128));

    // The shop leaves the marketplace: gone from the designer's, and from its own.
    ask(&mut shop, false);
    until(
        &mut [&mut shop, &mut designer],
        "the shop is withdrawn",
        |a| a[1].world().resource::<Social>().directory.is_empty(),
    );
    assert!(shop.world().resource::<Social>().directory.is_empty());
}

/// A one-person shop grows: the owner adds a worker (from the worker's card); the worker's cadrs
/// gets the invitation, which waits until the worker joins; then it makes the organization
/// document, introduces its device (the owner's
/// device adds it) and syncs the document; the worker adds a client, and it syncs back.
#[test]
fn a_new_member_syncs_the_organization() {
    use cadrs_app::org_sync::sync_now;
    use cadrs_org::{Client, Role};

    let mut owner = app("Owner");
    let mut worker = app("Worker");
    introduce(&mut owner, &worker);
    introduce(&mut worker, &owner);
    assert!(create(owner.world_mut(), founded("Growing Shop")));
    ensure_loaded(worker.world_mut());
    let worker_card = worker
        .world()
        .resource::<Social>()
        .my_card("Worker")
        .unwrap();
    add_contact(owner.world_mut(), worker_card.clone());
    assert!(write(
        owner.world_mut(),
        Op::SetMember {
            account: worker_card.value.account,
            role: Some(Role::Member),
        }
    ));
    assert!(cadrs_app::org_sync::invite(
        owner.world_mut(),
        &worker_card.value
    ));

    // The invitation waits for the worker: nothing is made until they join.
    until(
        &mut [&mut owner, &mut worker],
        "the invitation arrives",
        |a| {
            !a[1]
                .world()
                .resource::<cadrs_app::org_sync::Invitations>()
                .0
                .is_empty()
        },
    );
    assert!(worker.world().resource::<Social>().orgs.is_empty());
    assert!(worker.world().resource::<Notifications>().unread());
    let invitation = worker
        .world()
        .resource::<cadrs_app::org_sync::Invitations>()
        .0[0]
        .clone();
    assert_eq!(invitation.name, "Growing Shop");
    assert_eq!(
        invitation.inviter,
        owner
            .world()
            .resource::<Social>()
            .keys()
            .map(|k| k.account_id())
    );
    assert!(cadrs_app::org_sync::join(
        worker.world_mut(),
        invitation.log
    ));
    assert!(
        worker
            .world()
            .resource::<cadrs_app::org_sync::Invitations>()
            .0
            .is_empty()
    );

    // The worker's cadrs makes the document and syncs it in.
    until(
        &mut [&mut owner, &mut worker],
        "the worker has the shop",
        |a| {
            a[1].world()
                .resource::<Social>()
                .orgs
                .iter()
                .any(|o| o.name == "Growing Shop")
        },
    );
    let doc = worker.world().resource::<Social>().orgs[0].doc;
    let store = worker.world().resource::<DocumentStore>().0.clone();
    let path = cadrs_org::file::path(&store.doc_dir(doc));
    let worker_device = worker
        .world()
        .resource::<Social>()
        .keys()
        .unwrap()
        .device_id();
    until(
        &mut [&mut owner, &mut worker],
        "the worker's device is a member's",
        |a| {
            sync_now(a[1].world_mut());
            let Ok((log, _)) = cadrs_org::file::load(&path) else {
                return false;
            };
            let key = cadrs_social::keys::SecretKey::generate();
            let session = cadrs_org::session::Session::open(log, key);
            session
                .org()
                .member_of(&worker_device, u64::MAX / 2)
                .is_some()
        },
    );

    // The worker opens it and adds a client; the owner's copy gets it.
    let file = store.load(doc).unwrap();
    cadrs_app::organization::open(worker.world_mut(), &file);
    assert!(write(
        worker.world_mut(),
        Op::SetClient {
            id: 99,
            client: Some(Client {
                name: "Walk-in".into(),
                ..Default::default()
            }),
        }
    ));
    until(
        &mut [&mut owner, &mut worker],
        "the owner hears of the client",
        |a| {
            a[0].world()
                .resource::<OpenOrg>()
                .session
                .org()
                .clients
                .contains_key(&99)
        },
    );
}

/// A shop that is reached and doesn't take a request says so, and the client hears it: the
/// request is marked in their list with the reason, not left there as if it were waiting, and
/// the letter isn't left at the shop's mailbox to be refused again later.
#[test]
fn a_refused_request_is_said_to_the_client() {
    use cadrs_app::social::MyMailboxes;
    use cadrs_social::keys::{PublicKey, SecretKey};

    // A reached device's refusal is told apart from not reaching it, by both transports alike.
    assert_eq!(cadrs_social_net::REFUSED, cadrs_social::post::REFUSED);

    // The shop's card names a mailbox, kept by a node of its own.
    let mailbox_root = root("RefusalMailbox");
    let key_file = mailbox_root.join("identity.key");
    let mut node = App::new();
    node.add_plugins((
        MinimalPlugins,
        NetPlugin {
            identity: Some(key_file.clone()),
            display_name: "Mailbox".into(),
            relays: Relays::Disabled,
            autostart: true,
        },
    ));
    node.update();
    let key: [u8; 32] = std::fs::read(&key_file).unwrap().try_into().unwrap();
    cadrs_net::social::enable_mailbox(
        node.world_mut(),
        SecretKey::from_bytes(&key),
        Some(mailbox_root.join("mailbox.bin")),
        Default::default(),
    );
    let node_addr = node.world().resource::<Iroh>().addr().unwrap();
    let node_id = PublicKey(*node_addr.id.as_bytes());

    let mut shop = app("RefusingShop");
    shop.insert_resource(MyMailboxes(vec![node_id]));
    let mut designer = app("RefusedDesigner");
    introduce(&mut shop, &designer);
    introduce(&mut designer, &shop);
    designer
        .world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(node_addr.id, node_addr.clone());
    assert!(create(shop.world_mut(), founded("Choosy Cutters")));
    let org = shop
        .world()
        .resource::<OpenOrg>()
        .session
        .org()
        .org
        .unwrap();
    let card = shop.world().resource::<Social>().org_card(&org).unwrap();
    assert_eq!(card.value.mailboxes, vec![node_id]);

    // The shop takes no requests from this designer.
    ensure_loaded(designer.world_mut());
    let me = designer
        .world()
        .resource::<Social>()
        .keys()
        .unwrap()
        .account_id();
    assert!(write(
        shop.world_mut(),
        Op::SetBlocked {
            account: me,
            blocked: true,
        }
    ));

    // The designer asks all the same, as the Request a quote dialog does.
    assert!(add_contact(designer.world_mut(), card.clone()));
    let my_card = designer
        .world()
        .resource::<Social>()
        .my_card("Designer")
        .unwrap();
    let id = new_id();
    let parts = vec![(bracket(), 6)];
    let message = Message::Request {
        id,
        parts: parts.clone(),
        notes: String::new(),
        deadline: None,
        snapshot: b"a packed document".to_vec(),
        client: my_card,
    };
    assert_eq!(cadrs_org::letters::request_problem(&message), None);
    assert!(send(
        designer.world_mut(),
        &card.value,
        message,
        None,
        "request".into()
    ));
    designer.world_mut().resource_mut::<Social>().book.record(
        id,
        Sent {
            shop: org,
            shop_name: "Choosy Cutters".into(),
            parts,
            notes: String::new(),
            stage: Stage::Requested,
            quote: None,
            decline_reason: None,
            chat: vec![],
            sent: 0,
            delivery: None,
            review: None,
        },
    );
    until(
        &mut [&mut shop, &mut designer, &mut node],
        "the designer hears the refusal",
        |a| a[1].world().resource::<Social>().book.sent[&id].stage == Stage::Declined,
    );
    let sent = designer.world().resource::<Social>().book.sent[&id].clone();
    assert!(sent.decline_reason.is_some_and(|why| !why.is_empty()));
    // The shop has nothing of it, and neither has its mailbox.
    for _ in 0..20 {
        for app in [&mut shop, &mut designer, &mut node] {
            app.update();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        shop.world()
            .resource::<OpenOrg>()
            .session
            .org()
            .requests
            .is_empty()
    );
    let mailboxes = node.world().resource::<cadrs_net::social::Mailboxes>();
    let held = mailboxes
        .0
        .lock()
        .unwrap()
        .as_ref()
        .map(|mailbox| mailbox.held());
    assert_eq!(
        held,
        Some((0, 0)),
        "a refused letter was left at the mailbox"
    );

    // A "shop" that answers a request with megabytes of reason: the designer's cadrs keeps a
    // sentence of it, in the list and on screen, and its book stays a book's size.
    let ranter = Stranger::refusing("N".repeat(5 << 20));
    let ranter_addr = ranter.node.addr();
    let ranter_id =
        cadrs_net::bevy_iroh::iroh::EndpointId::from_bytes(ranter_addr.id.as_bytes()).unwrap();
    designer
        .world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(
            ranter_id,
            cadrs_net::bevy_iroh::iroh::EndpointAddr::from_parts(
                ranter_id,
                ranter_addr.addrs.iter().cloned().collect::<Vec<_>>(),
            ),
        );
    let ranter_card = cadrs_social::contact::Card {
        account: ranter.account.id(),
        kind: cadrs_social::contact::Kind::Organization,
        name: "Loud Laser".into(),
        devices: vec![ranter.key.public()],
        mailboxes: vec![],
        issued: 1,
    }
    .sign(ranter.account.key());
    assert!(add_contact(designer.world_mut(), ranter_card.clone()));
    let loud = new_id();
    let asked = Message::Request {
        id: loud,
        parts: vec![(bracket(), 1)],
        notes: String::new(),
        deadline: None,
        snapshot: Vec::new(),
        client: designer
            .world()
            .resource::<Social>()
            .my_card("Designer")
            .unwrap(),
    };
    assert!(send(
        designer.world_mut(),
        &ranter_card.value,
        asked,
        None,
        "request".into()
    ));
    designer.world_mut().resource_mut::<Social>().book.record(
        loud,
        Sent {
            shop: ranter.account.id(),
            shop_name: "Loud Laser".into(),
            parts: vec![(bracket(), 1)],
            notes: String::new(),
            stage: Stage::Requested,
            quote: None,
            decline_reason: None,
            chat: vec![],
            sent: 0,
            delivery: None,
            review: None,
        },
    );
    until(&mut [&mut designer], "the designer hears the rant", |a| {
        a[0].world().resource::<Social>().book.sent[&loud].stage == Stage::Declined
    });
    let social = designer.world().resource::<Social>();
    let reason = social.book.sent[&loud].decline_reason.clone().unwrap();
    assert!(!reason.is_empty());
    assert!(
        reason.chars().count() <= cadrs_social::post::MAX_REASON,
        "{} characters of reason were kept",
        reason.chars().count()
    );
    assert!(cadrs_social::codec::encode(&social.book).len() < 64 << 10);
    // Nothing on screen is longer than that either (a toast says the reason).
    let mut texts = designer.world_mut().query::<&Text>();
    let longest = texts
        .iter(designer.world())
        .map(|text| text.0.chars().count())
        .max()
        .unwrap_or(0);
    assert!(longest < 1_000, "{longest} characters in one text");

    // What no shop would take is caught before it is sent at all.
    let endless = Message::Request {
        id: new_id(),
        parts: (0..cadrs_org::MAX_REQUEST_PARTS + 1)
            .map(|_| (bracket(), 1))
            .collect(),
        notes: String::new(),
        deadline: None,
        snapshot: Vec::new(),
        client: designer
            .world()
            .resource::<Social>()
            .my_card("Designer")
            .unwrap(),
    };
    assert!(cadrs_org::letters::request_problem(&endless).is_some());
}

/// Someone who isn't known to the shop, with a node of their own that sends whatever they
/// like: what the shop's cadrs answers, and what it keeps.
struct Stranger {
    rt: tokio::runtime::Runtime,
    node: cadrs_social_net::Node,
    key: cadrs_social::keys::SecretKey,
    account: cadrs_social::keys::Account,
    cert: cadrs_social::keys::Signed<cadrs_social::keys::DeviceCert>,
}

impl Stranger {
    fn new() -> Self {
        Self::answering(None)
    }

    /// A stranger whose node refuses whatever it is sent, with `reason` (megabytes of it, if
    /// they like).
    fn refusing(reason: String) -> Self {
        Self::answering(Some(reason))
    }

    fn answering(refusal: Option<String>) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let key = cadrs_social::keys::SecretKey::generate();
        let (node, mut incoming) = rt
            .block_on(cadrs_social_net::Node::spawn(key.to_bytes(), false))
            .unwrap();
        if let Some(reason) = refusal {
            rt.spawn(async move {
                while let Some(post) = incoming.recv().await {
                    post.refuse(reason.clone());
                }
            });
        }
        let account = cadrs_social::keys::Account::generate();
        let cert = account.certify(key.public(), "a stranger's", cadrs_social::wall_millis());
        Self {
            rt,
            node,
            key,
            account,
            cert,
        }
    }

    /// Sends `post` to `app`'s device, running the app meanwhile; its answer, or its refusal.
    fn send(&self, app: &mut App, post: &cadrs_social::post::Post) -> Result<Vec<u8>, String> {
        use std::sync::{Arc, Mutex};
        let addr = app.world().resource::<Iroh>().addr().unwrap();
        let to = cadrs_social_net::iroh::EndpointAddr::from_parts(
            cadrs_social_net::iroh::EndpointId::from_bytes(addr.id.as_bytes()).unwrap(),
            addr.addrs.iter().cloned().collect::<Vec<_>>(),
        );
        self.node.remember(to.clone());
        let result = Arc::new(Mutex::new(None));
        let (r, node, bytes) = (result.clone(), self.node.clone(), post.encode());
        self.rt.spawn(async move {
            let answer = node.send(to, &bytes).await;
            *r.lock().unwrap() = Some(answer);
        });
        let start = Instant::now();
        loop {
            app.update();
            if let Some(answer) = result.lock().unwrap().take() {
                return answer;
            }
            assert!(start.elapsed() < PATIENCE, "the app never answered");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A request for a quote to organization `org`, carrying a document of `size` bytes that is
    /// unlike any other.
    fn request(&self, org: cadrs_social::keys::AccountId, n: u8, size: usize) -> Vec<u8> {
        self.request_as(org, n, size, "Nobody You Know".into(), 1)
    }

    /// As [`Stranger::request`], with a card of the stranger's making: its name, and how many
    /// devices it names to answer.
    fn request_as(
        &self,
        org: cadrs_social::keys::AccountId,
        n: u8,
        size: usize,
        name: String,
        devices: usize,
    ) -> Vec<u8> {
        let card = cadrs_social::contact::Card {
            account: self.account.id(),
            kind: cadrs_social::contact::Kind::Person,
            name,
            devices: vec![self.key.public(); devices],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(self.account.key());
        cadrs_org::letters::write(
            &self.key,
            &self.cert,
            org,
            Message::Request {
                id: new_id(),
                parts: vec![(bracket(), 1)],
                notes: String::new(),
                deadline: None,
                snapshot: vec![n; size],
                client: card,
            },
            cadrs_social::wall_millis(),
        )
    }
}

fn blobs_in(answer: &[u8]) -> Vec<Vec<u8>> {
    let cadrs_social::post::Blobs(blobs) =
        cadrs_social::codec::decode(answer).expect("an answer of blobs");
    blobs.into_iter().map(|b| b.into_vec()).collect()
}

/// A stranger with a node of their own, sending what a well-behaved cadrs never would: the shop
/// keeps only so many of their requests (and nothing of the ones it refuses), hands out nothing
/// but the logo of a listed organization, and is neither joined to an organization nor flooded
/// with invitations by a certificate the stranger copied.
#[test]
fn a_stranger_gets_nothing_and_leaves_little() {
    use cadrs_app::org_sync::Invitations;
    use cadrs_social::post::Post;

    let mut shop = app("CarefulShop");
    assert!(create(shop.world_mut(), founded("Careful Cutters")));
    let (doc, org, log, logo) = {
        let open = shop.world().resource::<OpenOrg>();
        let org = open.session.org();
        (
            open.id,
            org.org.unwrap(),
            open.session.log().id(),
            org.profile.logo.unwrap(),
        )
    };
    let blob_dir = shop
        .world()
        .resource::<DocumentStore>()
        .0
        .doc_dir(doc)
        .join(cadrs_org::blob::DIR);
    let kept = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().count();
    assert_eq!(kept(&blob_dir), 2, "the logo, in its two sizes");
    let stranger = Stranger::new();

    // A request whose card isn't a card (megabytes of name, tens of thousands of devices to
    // answer) is refused before anything is kept: the log is as it was, to the byte.
    let log_file = cadrs_org::file::path(&shop.world().resource::<DocumentStore>().0.doc_dir(doc));
    let log_before = std::fs::metadata(&log_file).unwrap().len();
    let heavy_name = Post::Letter(stranger.request_as(org, 200, 100, "N".repeat(2 << 20), 1));
    let many_devices = Post::Letter(stranger.request_as(org, 201, 100, "Many".into(), 50_000));
    assert!(stranger.send(&mut shop, &heavy_name).is_err());
    assert!(stranger.send(&mut shop, &many_devices).is_err());
    assert!(
        shop.world()
            .resource::<OpenOrg>()
            .session
            .org()
            .requests
            .is_empty()
    );
    assert_eq!(std::fs::metadata(&log_file).unwrap().len(), log_before);
    assert_eq!(kept(&blob_dir), 2);

    // Requests: as many as may wait from one client, and no more. One that is refused leaves
    // nothing behind, and asking doesn't make the stranger one of the user's contacts.
    let most = cadrs_org::MAX_WAITING_PER_CLIENT;
    let mut taken = 0;
    for n in 0..most as u8 + 3 {
        let letter = Post::Letter(stranger.request(org, n, 4_000));
        taken += usize::from(stranger.send(&mut shop, &letter).is_ok());
    }
    assert_eq!(taken, most);
    let requests = shop
        .world()
        .resource::<OpenOrg>()
        .session
        .org()
        .requests
        .clone();
    assert_eq!(requests.len(), most);
    assert_eq!(
        kept(&blob_dir),
        2 + most,
        "no document of a refused request"
    );
    let social = shop.world().resource::<Social>();
    assert!(social.contacts.get(&stranger.account.id()).is_none());
    // The request says who asks all the same.
    let asked = requests.values().next().unwrap();
    assert_eq!(asked.card.as_ref().unwrap().value.name, "Nobody You Know");
    let document = asked.snapshot_blob().unwrap();

    // A request's document is the members': a stranger asking for it by its hash, as a member
    // would or as anyone may ask for a logo, gets nothing.
    let as_member = Post::WantBlobs {
        log,
        cert: stranger.cert.clone(),
        blobs: vec![document.hash, logo.thumb.hash],
    };
    let answer = stranger.send(&mut shop, &as_member);
    assert!(answer.map_or(true, |a| blobs_in(&a).is_empty()));
    let public = |hash| Post::PublicBlob { org, hash };
    assert!(stranger.send(&mut shop, &public(document.hash)).is_err());
    // Nor the logo, while the organization isn't in the marketplace.
    assert!(!shop.world().resource::<OpenOrg>().session.org().listed());
    assert!(stranger.send(&mut shop, &public(logo.thumb.hash)).is_err());
    // Listed: its logo, and still nothing else.
    assert!(write(
        shop.world_mut(),
        Op::SetMachine {
            id: new_id(),
            machine: Some(waterjet()),
        }
    ));
    assert!(write(
        shop.world_mut(),
        Op::EditProfile(cadrs_org::profile::ProfileEdit {
            listed: Some(true),
            ..Default::default()
        })
    ));
    assert!(shop.world().resource::<OpenOrg>().session.org().listed());
    let answer = stranger.send(&mut shop, &public(logo.thumb.hash)).unwrap();
    let blobs = blobs_in(&answer);
    assert_eq!(blobs.len(), 1);
    assert_eq!(cadrs_org::BlobRef::of(&blobs[0]), logo.thumb);
    assert!(stranger.send(&mut shop, &public(document.hash)).is_err());

    // An invitation "from the user's own account": the stranger copied the user's certificate
    // (every letter carries its sender's). It isn't the stranger's device's, so it says nothing
    // about who invites: nothing is joined, and the invitation waits like any stranger's.
    let own_cert = shop
        .world()
        .resource::<Social>()
        .keys()
        .unwrap()
        .cert
        .clone();
    let trap = |n: u8| Post::Invite {
        log: cadrs_social::log::LogId([n; 32]),
        org: stranger.account.id(),
        name: "Trap".into(),
        devices: vec![stranger.key.public()],
        cert: None,
        inviter: Some(own_cert.clone()),
    };
    assert!(stranger.send(&mut shop, &trap(1)).is_ok());
    assert_eq!(
        shop.world().resource::<Social>().orgs.len(),
        1,
        "joined nothing"
    );
    let waiting = shop.world().resource::<Invitations>().0.clone();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].inviter, None, "nobody the user knows invites");
    // An invitation is a few names and keys: one with megabytes in it isn't read.
    let heavy = Post::Invite {
        log: cadrs_social::log::LogId([250; 32]),
        org: stranger.account.id(),
        name: "N".repeat(1 << 20),
        devices: vec![stranger.key.public()],
        cert: None,
        inviter: None,
    };
    assert!(stranger.send(&mut shop, &heavy).is_err());
    assert_eq!(shop.world().resource::<Invitations>().0.len(), 1);
    // Strangers' invitations wait up to a number, and then are refused: they can't push out
    // the ones that wait, nor ring the bell without end.
    let mut waits = 1;
    for n in 2..40 {
        waits += usize::from(stranger.send(&mut shop, &trap(n)).is_ok());
    }
    assert_eq!(waits, 20);
    assert_eq!(shop.world().resource::<Invitations>().0.len(), 20);
    assert_eq!(shop.world().resource::<Invitations>().0[0].log.0, [1; 32]);
    assert_eq!(shop.world().resource::<Social>().orgs.len(), 1);
}

/// Someone says hello in the public chat; another app online hears it.
#[test]
fn a_line_in_the_public_chat_is_heard() {
    let mut alex = app("Alex");
    let mut sam = app("Sam");
    // Both in the room before anyone speaks.
    into_the_room(&mut alex, &mut sam);
    assert!(cadrs_app::public_chat_ui::say_text(
        alex.world_mut(),
        "Hello, makers"
    ));
    until(&mut [&mut alex, &mut sam], "Sam hears it", |a| {
        a[1].world()
            .get_resource::<Social>()
            .is_some_and(|s| s.public_chat.lines().any(|l| l.text == "Hello, makers"))
    });
}

/// The owner links a phone (the phone app's state and node, no Bevy) to the account; the
/// desktop invites it to the shop with the shop's certificate; the phone syncs the shop's
/// document, quotes a client's request from it, and the desktop's document has the quote.
#[test]
fn an_owners_phone_quotes_for_the_shop() {
    use cadrs_app::pairing_ui::{OfferText, ShownCode, confirm, open_link};
    use cadrs_mobile::state::Phone;
    use cadrs_social::post::Post;
    use std::sync::{Arc, Mutex};

    let mut owner = app("ShopOwner");
    assert!(create(owner.world_mut(), founded("Pocket Waterjet")));
    set_up(&mut owner);
    // A client's request, as the desktop took it.
    let client = cadrs_social::keys::Account::generate();
    let client_card = cadrs_social::contact::Card {
        account: client.id(),
        kind: cadrs_social::contact::Kind::Person,
        name: "Alex".into(),
        devices: vec![cadrs_social::keys::SecretKey::generate().public()],
        mailboxes: vec![],
        issued: 1,
    }
    .sign(client.key());
    let id = new_id();
    assert!(write(
        owner.world_mut(),
        Op::Request {
            id,
            client: client.id(),
            parts: vec![(bracket(), 6)],
            notes: String::new(),
            snapshot: [0; 32],
            snapshot_len: 0,
            deadline: None,
            card: Some(client_card),
        }
    ));

    // The phone: its state and its node.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let phone_root = root("OwnersPhone");
    let phone = Arc::new(Mutex::new(
        Phone::load(&phone_root, vec![], cadrs_social::wall_millis()).unwrap(),
    ));
    phone.lock().unwrap().set_name("Owner's phone");
    let secret = phone.lock().unwrap().keys.device.to_bytes();
    let (node, mut incoming) = rt
        .block_on(cadrs_social_net::Node::spawn(secret, false))
        .unwrap();
    let owner_addr = owner.world().resource::<Iroh>().addr().unwrap();
    let owner_on_phone = cadrs_social_net::iroh::EndpointAddr::from_parts(
        cadrs_social_net::iroh::EndpointId::from_bytes(owner_addr.id.as_bytes()).unwrap(),
        owner_addr.addrs.iter().cloned().collect::<Vec<_>>(),
    );
    node.remember(owner_on_phone.clone());
    let phone_addr = node.addr();
    owner
        .world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(
            cadrs_net::bevy_iroh::iroh::EndpointId::from_bytes(phone_addr.id.as_bytes()).unwrap(),
            cadrs_net::bevy_iroh::iroh::EndpointAddr::from_parts(
                cadrs_net::bevy_iroh::iroh::EndpointId::from_bytes(phone_addr.id.as_bytes())
                    .unwrap(),
                phone_addr.addrs.iter().cloned().collect::<Vec<_>>(),
            ),
        );
    // The phone answers what arrives.
    {
        let phone = phone.clone();
        rt.spawn(async move {
            while let Some(post) = incoming.recv().await {
                let from = cadrs_social::keys::PublicKey(*post.from.as_bytes());
                let answer = match Post::decode(&post.bytes).ok() {
                    Some(p) => {
                        phone
                            .lock()
                            .unwrap()
                            .handle(p, Some(from), cadrs_social::wall_millis())
                    }
                    None => Err("junk".into()),
                };
                match answer {
                    Ok(bytes) => post.take(bytes),
                    Err(why) => post.refuse(why),
                }
            }
        });
    }
    // Sends a post from the phone to the owner's desktop, running the desktop meanwhile.
    let send = |owner: &mut App, bytes: Vec<u8>| -> Result<Vec<u8>, String> {
        let result = Arc::new(Mutex::new(None));
        let (r, node, to) = (result.clone(), node.clone(), owner_on_phone.clone());
        rt.spawn(async move {
            let answer = node.send(to, &bytes).await;
            *r.lock().unwrap() = Some(answer);
        });
        let start = Instant::now();
        loop {
            owner.update();
            if let Some(answer) = result.lock().unwrap().take() {
                return answer;
            }
            assert!(start.elapsed() < PATIENCE, "the desktop never answered");
            std::thread::sleep(Duration::from_millis(10));
        }
    };

    // Pairing: the desktop offers, the phone asks, both show one code, the owner confirms.
    open_link(owner.world_mut());
    let offer = owner.world().resource::<OfferText>().0.clone();
    let (_, post, code) = phone
        .lock()
        .unwrap()
        .join(&offer, cadrs_social::wall_millis())
        .unwrap();
    send(&mut owner, post).unwrap();
    assert_eq!(owner.world().resource::<ShownCode>().0, code);
    confirm(owner.world_mut());
    let owner_account = owner
        .world()
        .resource::<Social>()
        .keys()
        .unwrap()
        .account_id();
    let start = Instant::now();
    while !(phone.lock().unwrap().account() == owner_account
        && phone
            .lock()
            .unwrap()
            .shops
            .list
            .iter()
            .any(|s| s.info.cert.is_some()))
    {
        owner.update();
        assert!(
            start.elapsed() < PATIENCE,
            "the phone never joined the shop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // The phone syncs the shop's document from the desktop (which adds the phone's device).
    let posts = {
        let p = phone.lock().unwrap();
        p.shops.sync_posts(p.keys.device_id(), &p.keys.cert)
    };
    for (log, _, post) in posts {
        let answer = send(&mut owner, post).unwrap();
        let back = phone
            .lock()
            .unwrap()
            .shops
            .synced(&log, &answer, cadrs_social::wall_millis());
        if let Some(back) = back {
            send(&mut owner, back).unwrap();
        }
    }
    assert_eq!(
        phone.lock().unwrap().shops.list[0]
            .session
            .org()
            .requests
            .len(),
        1
    );

    // It quotes from the phone; the next sync takes the quote to the desktop.
    let (to, _letter) = {
        let mut p = phone.lock().unwrap();
        let key = p.keys.device.clone();
        p.shops
            .quote(0, id, &key, cadrs_social::wall_millis())
            .unwrap()
    };
    assert_eq!(to.account, client.id());
    let posts = {
        let p = phone.lock().unwrap();
        p.shops.sync_posts(p.keys.device_id(), &p.keys.cert)
    };
    for (log, _, post) in posts {
        let answer = send(&mut owner, post).unwrap();
        let back = phone
            .lock()
            .unwrap()
            .shops
            .synced(&log, &answer, cadrs_social::wall_millis());
        if let Some(back) = back {
            send(&mut owner, back).unwrap();
        }
    }
    let org = owner.world().resource::<OpenOrg>().session.org();
    assert_eq!(org.requests[&id].stage, Stage::Quoted);
    assert_eq!(
        org.requests[&id].quote.as_ref().unwrap().total,
        Money::from_major(724.0)
    );
}
