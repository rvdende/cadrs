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
use cadrs_app::organization::{OpenOrg, create, instant_quote, write};
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

/// Each app knows where the other is (no relay to find it through).
fn introduce(a: &mut App, b: &App) {
    let addr = b.world().resource::<Iroh>().addr().expect("online");
    a.world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(addr.id, addr);
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
    create(shop.world_mut(), "Cape Waterjet");
    let steel = new_id();
    for op in [
        Op::SetMachine {
            id: new_id(),
            machine: Some(MachineCaps {
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
            }),
        },
        Op::SetMaterial {
            id: steel,
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
            hourly: vec![("Waterjet".into(), Money::from_major(1200.0))],
            sheet_per_m2: vec![("mild steel".into(), 10.0, Money::from_major(1000.0))],
            nesting_allowance: 1.25,
        }),
    ] {
        assert!(write(shop.world_mut(), op));
    }
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
    create(shop.world_mut(), "Night Shift Cutting");
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

/// A shop that takes requests from anyone is heard of in the public room: the designer's
/// directory finds it, nearest first, with its place blurred to what the shop chose.
#[test]
fn a_shop_open_for_quotes_is_found_in_the_directory() {
    use cadrs_net::public::{PublicPeers, Rendezvous};
    use cadrs_social::place::Place;

    let mut shop = app("ListedShop");
    shop.insert_resource(Rendezvous(None));
    let mut designer = app("Finder");
    designer.insert_resource(Rendezvous(None));
    let shop_addr = shop.world().resource::<Iroh>().addr().unwrap();
    designer.insert_resource(PublicPeers(vec![shop_addr]));
    introduce(&mut designer, &shop);

    create(shop.world_mut(), "Stellenbosch Waterjet");
    let street = Place::earth(-33.935712, 18.861123);
    assert!(write(
        shop.world_mut(),
        Op::SetProfile(cadrs_org::Profile {
            name: "Stellenbosch Waterjet".into(),
            place: Some(street.clone()),
            public_accuracy_m: 5_000.0,
            open_for_quotes: true,
            ..Default::default()
        })
    ));
    ensure_loaded(designer.world_mut());
    until(
        &mut [&mut shop, &mut designer],
        "the listing is heard",
        |a| a[1].world().resource::<Social>().directory.len() == 1,
    );
    let social = designer.world().resource::<Social>();
    let cape_town = Place::earth(-33.92, 18.42);
    let found = social.directory.search(Some(&cape_town), None, None, None);
    assert_eq!(found[0].listing.name(), "Stellenbosch Waterjet");
    let km = found[0].distance_m.unwrap() / 1000.0;
    assert!((35.0..50.0).contains(&km), "{km} km");
    let shown = found[0].listing.place.clone().unwrap();
    assert_ne!(shown.frame, street.frame, "the street isn't public");
}

/// A one-person shop grows: the owner adds a worker (from the worker's card); the worker's cadrs
/// gets the invitation, makes the organization document, introduces its device (the owner's
/// device adds it) and syncs the document; the worker adds a client, and it syncs back.
#[test]
fn a_new_member_syncs_the_organization() {
    use cadrs_app::org_sync::sync_now;
    use cadrs_org::{Client, Role};

    let mut owner = app("Owner");
    let mut worker = app("Worker");
    introduce(&mut owner, &worker);
    introduce(&mut worker, &owner);
    create(owner.world_mut(), "Growing Shop");
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

/// Someone says hello in the public chat; another app online hears it.
#[test]
fn a_line_in_the_public_chat_is_heard() {
    use cadrs_net::public::{PublicPeers, Rendezvous};
    let mut alex = app("Alex");
    alex.insert_resource(Rendezvous(None));
    let mut sam = app("Sam");
    sam.insert_resource(Rendezvous(None));
    let alex_addr = alex.world().resource::<Iroh>().addr().unwrap();
    sam.insert_resource(PublicPeers(vec![alex_addr]));
    // Both in the room before anyone speaks.
    until(&mut [&mut alex, &mut sam], "both are in the room", |a| {
        a.iter()
            .all(|app| matches!(cadrs_net::public::room_state(app.world()), cadrs_net::public::RoomState::Joined(n) if n > 0))
    });
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
    create(owner.world_mut(), "Pocket Waterjet");
    let steel = new_id();
    for op in [
        Op::SetMachine {
            id: new_id(),
            machine: Some(MachineCaps {
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
            }),
        },
        Op::SetMaterial {
            id: steel,
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
            hourly: vec![("Waterjet".into(), Money::from_major(1200.0))],
            sheet_per_m2: vec![("mild steel".into(), 10.0, Money::from_major(1000.0))],
            nesting_allowance: 1.25,
        }),
    ] {
        assert!(write(owner.world_mut(), op));
    }
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
            deadline: None,
            card: Some(client_card),
        }
    ));

    // The phone: its state and its node.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let phone_root = root("Phone");
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
                let answer = match Post::decode(&post.bytes) {
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
