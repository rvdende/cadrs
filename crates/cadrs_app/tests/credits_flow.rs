//! Credits between three running apps over real iroh on this machine (no relay): a bank, a
//! client and a shop. The client takes from the faucet and pays the shop; the shop's device is
//! told as it happens; the shop pays out to its owner; an order's payment is held, and released
//! when the client says so.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use cadrs_app::credits::{self, BankAt, Credits, CreditsPlugin, Payment};
use cadrs_app::notifications::Notifications;
use cadrs_app::organization::{NewOrg, OpenOrg, advance, create, deliver, instant_quote, write};
use cadrs_app::social::{PeerAddrs, Social, SocialPlugin, add_contact, ensure_loaded, send};
use cadrs_app::{AppClock, AppState, DocumentStore, UserProfile};
use cadrs_core::Store;
use cadrs_credits::{About, Kind};
use cadrs_net::NetPlugin;
use cadrs_net::bevy_iroh::{Iroh, Relays};
use cadrs_social::keys::{AccountId, PublicKey, SecretKey};

const PATIENCE: Duration = Duration::from_secs(30);

fn root(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cadrs-credits-flow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app(name: &str) -> (App, PathBuf) {
    let root = root(name);
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        NetPlugin {
            identity: Some(
                root.join("documents")
                    .join(cadrs_core::identity::DEFAULT_KEY),
            ),
            display_name: name.into(),
            relays: Relays::Disabled,
            autostart: true,
        },
        SocialPlugin,
        CreditsPlugin,
        cadrs_app::wallet_ui::WalletUiPlugin,
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
    ensure_loaded(app.world_mut());
    (app, root)
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
            for (i, a) in apps.iter_mut().enumerate() {
                let mut q = a.world_mut().query::<&Text>();
                let texts: Vec<String> = q.iter(a.world()).map(|t| t.0.clone()).collect();
                eprintln!("app {i} says: {texts:?}");
            }
            panic!("gave up waiting: {what}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn me(app: &App) -> AccountId {
    credits::me(app.world()).expect("keys")
}

fn balance(app: &App, wallet: &AccountId) -> Option<u64> {
    app.world().resource::<Credits>().balance(wallet)
}

fn logo() -> cadrs_app::org_logo::Processed {
    let mut picture = Vec::new();
    image::RgbaImage::from_fn(512, 512, |x, y| {
        image::Rgba([(x / 2) as u8, (y / 2) as u8, 180, 255])
    })
    .write_to(
        &mut std::io::Cursor::new(&mut picture),
        image::ImageFormat::Png,
    )
    .unwrap();
    cadrs_app::org_logo::process(&picture).expect("a logo")
}

/// The bank is the third app's node: its books signed with the node's own key, as the seed's.
/// The other two ask it by its id and know where it is; it knows where they are.
fn bank_for(bank: &mut App, bank_root: &std::path::Path, others: &mut [&mut App]) -> AccountId {
    let key = std::fs::read(
        bank_root
            .join("documents")
            .join(cadrs_core::identity::DEFAULT_KEY),
    )
    .ok()
    .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
    .map(|b| SecretKey::from_bytes(&b))
    .expect("the node's key");
    cadrs_net::bank::host(bank.world_mut(), key, Some(&bank_root.join("bank"))).unwrap();
    let iroh = bank.world().resource::<Iroh>();
    let (bank_node, bank_addr) = (iroh.id(), iroh.addr().unwrap());
    let id = PublicKey(*bank_node.as_bytes());
    assert_eq!(
        bank.world().resource::<cadrs_net::bank::Hosted>().0.id(),
        id,
        "the bank's id is its node's"
    );
    for other in others.iter_mut() {
        other.insert_resource(BankAt { id, here: false });
        other
            .world_mut()
            .get_resource_or_insert_with(PeerAddrs::default)
            .0
            .insert(bank_node, bank_addr.clone());
        let iroh = other.world().resource::<Iroh>();
        let (node, addr) = (iroh.id(), iroh.addr().unwrap());
        bank.world_mut()
            .get_resource_or_insert_with(cadrs_net::bank::Addrs::default)
            .0
            .insert(node, addr);
    }
    id
}

#[test]
fn credits_move_between_people_and_a_shop_as_they_happen() {
    let (mut bank, bank_root) = app("Bank");
    let (mut marco, _) = app("Marco");
    let (mut sipho, _) = app("Sipho");
    bank_for(&mut bank, &bank_root, &mut [&mut marco, &mut sipho]);
    let (marco_id, sipho_id) = (me(&marco), me(&sipho));

    // Sipho founds his shop: its key tells the bank who may spend its credits.
    create(
        sipho.world_mut(),
        NewOrg {
            name: "Cape Waterjet".into(),
            tagline: "Waterjet cutting in steel, aluminium and glass".into(),
            offers: cadrs_org::Offers {
                manufacturing: true,
                designs: false,
            },
            email: "sales@shop.example".into(),
            logo: logo(),
            location: None,
        },
    );
    let shop = sipho
        .world()
        .resource::<OpenOrg>()
        .session
        .org()
        .org
        .expect("the organization's account");
    credits::watch(sipho.world_mut(), shop);
    credits::refresh(sipho.world_mut(), sipho_id);
    until(
        &mut [&mut bank, &mut sipho],
        "the shop's wallet is its owner's to spend",
        |a| {
            let credits = a[1].world().resource::<Credits>();
            // Until the bank has the organization's word, the wallet isn't the owner's to see.
            let seen = credits
                .wallet(&shop)
                .and_then(|w| w.statement.as_ref())
                .is_some_and(|s| s.may_spend && s.organization);
            if !seen && credits.wallet(&shop).is_some_and(|w| !w.asking) {
                credits::refresh(a[1].world_mut(), shop);
            }
            seen
        },
    );

    // Marco takes from the faucet: a thousand, and an hour to wait.
    credits::faucet(marco.world_mut());
    until(&mut [&mut bank, &mut marco], "the faucet gave", |a| {
        balance(a[1], &marco_id) == Some(1000)
    });
    let statement = marco.world().resource::<Credits>().wallets[&marco_id]
        .statement
        .clone()
        .unwrap();
    assert!(statement.faucet_at.is_some_and(|at| at > statement.time));
    credits::faucet(marco.world_mut());
    until(&mut [&mut bank, &mut marco], "not twice in the hour", |a| {
        let credits = a[1].world().resource::<Credits>();
        credits.wallets[&marco_id].error.is_some() && !credits.wallets[&marco_id].asking
    });
    assert_eq!(
        bank.world()
            .resource::<cadrs_net::bank::Hosted>()
            .0
            .book()
            .balance(&marco_id),
        1000
    );

    // He pays the shop 300. Sipho's device hears of it from the bank, without asking: the
    // shop's balance, and a notice.
    let before = sipho.world().resource::<Notifications>().items.len();
    let paid = credits::pay(
        marco.world_mut(),
        Payment {
            from: marco_id,
            to: shop,
            to_name: "Cape Waterjet".into(),
            amount: 300,
            note: "deposit".into(),
            about: None,
            payout: false,
        },
        None,
    );
    assert!(paid);
    let asked = Instant::now();
    until(
        &mut [&mut bank, &mut marco, &mut sipho],
        "the shop was paid",
        |a| balance(a[1], &marco_id) == Some(700) && balance(a[2], &shop) == Some(300),
    );
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "as it happens: {:?}",
        asked.elapsed()
    );
    assert_eq!(
        sipho.world().resource::<Notifications>().items.len(),
        before + 1
    );
    let shown = &sipho.world().resource::<Credits>().wallets[&shop].movements[0];
    assert_eq!(
        (shown.amount, shown.note.as_str(), shown.from),
        (300, "deposit", Some(marco_id))
    );

    // More than he has: refused, said, and nothing moved.
    credits::pay(
        marco.world_mut(),
        Payment {
            from: marco_id,
            to: shop,
            to_name: "Cape Waterjet".into(),
            amount: 5000,
            note: String::new(),
            about: None,
            payout: false,
        },
        None,
    );
    until(&mut [&mut bank, &mut marco], "too much is refused", |a| {
        a[1].world().resource::<Credits>().wallets[&marco_id]
            .error
            .is_some()
    });
    assert_eq!(
        bank.world()
            .resource::<cadrs_net::bank::Hosted>()
            .0
            .book()
            .balance(&shop),
        300
    );

    // The shop pays its owner out: from the organization's wallet to a person's.
    credits::pay(
        sipho.world_mut(),
        Payment {
            from: shop,
            to: sipho_id,
            to_name: "Sipho".into(),
            amount: 120,
            note: "November".into(),
            about: None,
            payout: true,
        },
        None,
    );
    until(
        &mut [&mut bank, &mut sipho],
        "the owner was paid out",
        |a| balance(a[1], &shop) == Some(180) && balance(a[1], &sipho_id) == Some(120),
    );
    let payout = &sipho.world().resource::<Credits>().wallets[&sipho_id].movements[0];
    assert_eq!(
        (payout.kind, payout.from, payout.by),
        (Kind::Payout, Some(shop), sipho_id)
    );
    // Marco can't spend the shop's credits, nor see them.
    credits::pay(
        marco.world_mut(),
        Payment {
            from: shop,
            to: marco_id,
            to_name: "Marco".into(),
            amount: 100,
            note: String::new(),
            about: None,
            payout: false,
        },
        None,
    );
    until(
        &mut [&mut bank, &mut marco],
        "a stranger spends nothing of the shop's",
        |a| {
            a[1].world()
                .resource::<Credits>()
                .wallet(&shop)
                .is_some_and(|w| w.error.is_some())
        },
    );
    assert_eq!(
        bank.world()
            .resource::<cadrs_net::bank::Hosted>()
            .0
            .book()
            .balance(&shop),
        180
    );

    // An order: Marco's credits are held for the shop, which sees them on their way.
    let order = cadrs_org::new_id();
    credits::hold(
        marco.world_mut(),
        Payment {
            from: marco_id,
            to: shop,
            to_name: "Cape Waterjet".into(),
            amount: 500,
            note: String::new(),
            about: Some(About::order(order, "R-0001")),
            payout: false,
        },
        None,
    );
    until(
        &mut [&mut bank, &mut marco, &mut sipho],
        "the payment is held",
        |a| {
            credits::held_for(a[1].world(), &marco_id, order).is_some()
                && credits::held_for(a[2].world(), &shop, order).is_some()
        },
    );
    assert_eq!(balance(&marco, &marco_id), Some(200));
    assert_eq!(balance(&sipho, &shop), Some(180));
    let held = credits::held_for(sipho.world(), &shop, order).unwrap();
    assert_eq!((held.amount, held.from, held.to), (500, marco_id, shop));
    // The part arrives: he releases it, and the shop has it.
    credits::release(marco.world_mut(), marco_id, held.id, None);
    until(
        &mut [&mut bank, &mut marco, &mut sipho],
        "the held payment is released",
        |a| {
            balance(a[2], &shop) == Some(680)
                && credits::held_for(a[1].world(), &marco_id, order).is_none()
        },
    );
    let paid = credits::paid_for(sipho.world(), &shop, order).unwrap();
    assert_eq!(paid.kind, Kind::Released);

    // The books add up, and are on file.
    let book = bank.world().resource::<cadrs_net::bank::Hosted>().0.book();
    assert_eq!(book.total(), 1000);
    assert_eq!(
        book.balance(&marco_id) + book.balance(&shop) + book.balance(&sipho_id),
        1000
    );
    assert!(bank_root.join("bank").join("credits.journal").exists());
}

fn introduce(a: &mut App, b: &App) {
    let addr = b.world().resource::<Iroh>().addr().expect("online");
    a.world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(addr.id, addr);
}

/// An order from request to paid: the designer asks, the shop quotes 724, the designer accepts
/// and pays (the credits are held), the shop makes it and hands it over, the designer signs for
/// it, and the credits are the shop's. A payment held for an order that won't be made goes back.
#[test]
fn an_order_is_paid_for_when_it_is_signed_for() {
    use cadrs_app::credits::OrderPayment;
    use cadrs_org::capability::{MachineCaps, MaterialLimit, PartSummary, Process};
    use cadrs_org::letters::{Delivery, Message, Sent};
    use cadrs_org::quote::{Money, RateCard};
    use cadrs_org::{Op, Stage, new_id};

    let (mut bank, bank_root) = app("OrderBank");
    let (mut designer, _) = app("Designer");
    let (mut shop, _) = app("Shop");
    bank_for(&mut bank, &bank_root, &mut [&mut designer, &mut shop]);
    introduce(&mut shop, &designer);
    introduce(&mut designer, &shop);
    let me = me(&designer);

    // The shop: a waterjet, a sheet, prices in credits.
    assert!(create(
        shop.world_mut(),
        NewOrg {
            name: "Cape Waterjet".into(),
            tagline: "Waterjet cutting in steel, aluminium and glass".into(),
            offers: cadrs_org::Offers {
                manufacturing: true,
                designs: false,
            },
            email: "sales@shop.example".into(),
            logo: logo(),
            location: None,
        }
    ));
    let jet = new_id();
    for op in [
        Op::SetMachine {
            id: jet,
            machine: Some(MachineCaps {
                name: "Waterjet".into(),
                process: Process::Waterjet,
                envelope: [3000.0, 1500.0, 0.0],
                materials: vec![MaterialLimit {
                    material: "Steel - A36".into(),
                    max_thickness: 100.0,
                    speeds: vec![(10.0, 300.0)],
                }],
                kerf: 1.0,
                min_hole_ratio: 0.5,
                tolerance: 0.1,
                pierce_seconds: 4.0,
                location: None,
            }),
        },
        Op::SetMaterial {
            id: new_id(),
            material: Some(cadrs_org::Material {
                name: "Steel - A36".into(),
                thickness: 10.0,
                sheet: [2500.0, 1250.0],
            }),
        },
        Op::SetRates(RateCard {
            currency: cadrs_org::quote::CREDITS.into(),
            setup: Money::from_major(250.0),
            minimum: Money::from_major(500.0),
            hourly: vec![cadrs_org::quote::MachineRate::new(
                jet,
                Money::from_major(1200.0),
            )],
            sheet_per_m2: vec![cadrs_org::quote::SheetPrice::new(
                "Steel - A36",
                10.0,
                Money::from_major(1000.0),
            )],
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
    let card = shop.world().resource::<Social>().org_card(&org).unwrap();
    credits::watch(shop.world_mut(), org);

    // The designer has a thousand credits, adds the shop and asks for a quote.
    credits::faucet(designer.world_mut());
    until(&mut [&mut bank, &mut designer], "the faucet gave", |a| {
        balance(a[1], &me) == Some(1000)
    });
    assert!(add_contact(designer.world_mut(), card.clone()));
    let my_card = designer
        .world()
        .resource::<Social>()
        .my_card("Designer")
        .unwrap();
    let id = new_id();
    let parts = vec![(
        PartSummary {
            name: "bracket".into(),
            flat_thickness: Some(10.0),
            size: [200.0, 80.0, 10.0],
            min_hole: Some(8.5),
            min_inside_radius: Some(3.0),
            cut_length: 900.0,
            pierces: 3,
            area: 12_000.0,
            material: Some("Steel - A36".into()),
            tolerance: None,
        },
        6,
    )];
    designer.world_mut().resource_mut::<Social>().book.record(
        id,
        Sent {
            shop: org,
            shop_name: "Cape Waterjet".into(),
            parts: parts.clone(),
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
    let message = Message::Request {
        id,
        parts,
        notes: String::new(),
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
    assert_eq!(
        (quote.total.credits(), quote.currency.as_str()),
        (724, "CRD")
    );

    // The shop asks after its order's payment by the hold's own id, which both sides can work
    // out: it is told of it however many other payments are held for the shop.
    let order_hold = cadrs_credits::order_hold(&me, id, quote.id);
    credits::name_holds(shop.world_mut(), org, vec![order_hold]);

    // Accept and pay: asked once; nothing moves before the answer.
    cadrs_app::request_quote::accept_and_pay(designer.world_mut(), id);
    for _ in 0..5 {
        designer.update();
        bank.update();
    }
    assert_eq!(
        bank.world()
            .resource::<cadrs_net::bank::Hosted>()
            .0
            .book()
            .len(),
        1
    );
    assert_eq!(
        designer.world().resource::<Social>().book.sent[&id].stage,
        Stage::Quoted
    );
    cadrs_app::wallet_ui::confirmed(designer.world_mut());
    // Pressed again before the bank's answer is back (or after it was lost): the order's hold
    // has one id, so the bank holds once.
    cadrs_app::request_quote::accept_and_pay(designer.world_mut(), id);
    cadrs_app::wallet_ui::confirmed(designer.world_mut());
    // Held for the shop, and only then is the shop told: it sees the order and the payment.
    until(
        &mut [&mut bank, &mut designer, &mut shop],
        "accepted, with the payment held",
        |a| {
            let accepted = a[2].world().resource::<OpenOrg>().session.org().requests[&id].stage
                == Stage::Accepted;
            accepted
                && matches!(
                    credits::payment_of(a[1].world(), &me, id),
                    Some(OrderPayment::Held(_))
                )
                && matches!(
                    credits::payment_of(a[2].world(), &org, id),
                    Some(OrderPayment::Held(_))
                )
        },
    );
    assert_eq!(balance(&designer, &me), Some(276));
    assert_eq!(
        bank.world()
            .resource::<cadrs_net::bank::Hosted>()
            .0
            .book()
            .len(),
        2,
        "the faucet and one hold, however often it was asked for"
    );
    assert_eq!(balance(&shop, &org), Some(0), "held is not yet the shop's");
    let named = |app: &App| {
        app.world().resource::<Credits>().wallets[&org]
            .statement
            .as_ref()
            .map(|s| s.named.clone())
            .unwrap_or_default()
    };
    assert_eq!(named(&shop).len(), 1);
    assert_eq!(
        (named(&shop)[0].id, named(&shop)[0].state),
        (order_hold, cadrs_credits::HoldState::Open)
    );
    let held = shop.world().resource::<Credits>().wallets[&org]
        .statement
        .clone()
        .unwrap();
    assert_eq!(held.held_in, 724);

    // Made, ready, handed over: the designer signs for it, and the shop is paid.
    advance(shop.world_mut(), id, Stage::InProduction);
    advance(shop.world_mut(), id, Stage::Ready);
    deliver(shop.world_mut(), id);
    until(
        &mut [&mut shop, &mut designer],
        "the delivery waits to be signed for",
        |a| {
            matches!(
                a[1].world().resource::<Social>().book.sent[&id].delivery,
                Some(Delivery::ToSign(_))
            )
        },
    );
    // The bank can't be reached when the designer signs: the order is signed for all the same,
    // the payment stays held, and it is released when the bank is back, without being asked.
    let real = *designer.world().resource::<BankAt>();
    designer.insert_resource(BankAt {
        id: SecretKey::generate().public(),
        here: false,
    });
    cadrs_app::request_quote::sign_delivery(designer.world_mut(), id);
    until(
        &mut [&mut bank, &mut designer, &mut shop],
        "signed for, the bank away",
        |a| {
            a[2].world().resource::<OpenOrg>().session.org().requests[&id].stage == Stage::Completed
        },
    );
    let books = |bank: &App| {
        bank.world()
            .resource::<cadrs_net::bank::Hosted>()
            .0
            .book()
            .balance(&org)
    };
    assert_eq!(books(&bank), 0, "still held");
    assert!(matches!(
        credits::payment_of(designer.world(), &me, id),
        Some(OrderPayment::Held(_))
    ));
    designer.insert_resource(real);
    until(
        &mut [&mut bank, &mut designer, &mut shop],
        "signed for and paid",
        |a| {
            balance(a[2], &org) == Some(724)
                && a[2].world().resource::<OpenOrg>().session.org().requests[&id].stage
                    == Stage::Completed
                && credits::payment_of(a[1].world(), &me, id) == Some(OrderPayment::Paid(724))
        },
    );
    assert_eq!(
        credits::payment_of(shop.world(), &org, id),
        Some(OrderPayment::Paid(724))
    );
    assert_eq!(named(&shop)[0].state, cadrs_credits::HoldState::Released);
    assert_eq!(balance(&designer, &me), Some(276));

    // Another order's payment, for something the shop won't make: it gives the credits back.
    let other = new_id();
    credits::hold(
        designer.world_mut(),
        Payment {
            from: me,
            to: org,
            to_name: "Cape Waterjet".into(),
            amount: 200,
            note: String::new(),
            about: Some(About::order(other, "plate")),
            payout: false,
        },
        None,
    );
    until(
        &mut [&mut bank, &mut designer, &mut shop],
        "the second payment is held",
        |a| credits::held_for(a[2].world(), &org, other).is_some(),
    );
    let hold = credits::held_for(shop.world(), &org, other).unwrap();
    credits::give_back(shop.world_mut(), org, hold.id, None);
    until(
        &mut [&mut bank, &mut designer, &mut shop],
        "the payment went back",
        |a| {
            balance(a[1], &me) == Some(276)
                && credits::payment_of(a[1].world(), &me, other)
                    == Some(OrderPayment::Returned(200))
        },
    );
    let book = bank.world().resource::<cadrs_net::bank::Hosted>().0.book();
    assert_eq!(
        (book.balance(&me), book.balance(&org), book.total()),
        (276, 724, 1000)
    );
}

/// What a device is told was paid is the bank's own word, from the bank's own device: a receipt
/// sent on by anyone else (the other side of an old payment has one) is not news, and one the
/// bank already told is news once.
#[test]
fn only_the_bank_says_what_was_paid() {
    use cadrs_social::keys::Account;
    let (mut bank, bank_root) = app("WordBank");
    let (mut sipho, _) = app("WordSipho");
    let bank_id = bank_for(&mut bank, &bank_root, &mut [&mut sipho]);
    let sipho_id = me(&sipho);
    credits::refresh(sipho.world_mut(), sipho_id);
    until(&mut [&mut bank, &mut sipho], "the wallet is known", |a| {
        balance(a[1], &sipho_id) == Some(0)
    });
    // Someone pays Sipho, asked of the bank in person; the receipt is theirs to keep.
    let payer = Account::generate();
    let device = SecretKey::generate();
    let now = cadrs_social::wall_millis();
    let cert = payer.certify(device.public(), "laptop", now);
    let mut asked = |ask: cadrs_credits::Ask| {
        let bytes = cadrs_credits::ask(&device, &cert, bank_id, payer.id(), ask, now);
        let handled = bank
            .world_mut()
            .resource_mut::<cadrs_net::bank::Hosted>()
            .0
            .handle(&bytes, Some(device.public()), now)
            .unwrap();
        cadrs_credits::read_answer(&handled.answer, &bank_id).unwrap()
    };
    asked(cadrs_credits::Ask::Faucet {
        id: cadrs_credits::new_id(),
    });
    let cadrs_credits::Answer::Receipt(receipt) = asked(cadrs_credits::Ask::Transfer {
        id: cadrs_credits::new_id(),
        to: sipho_id,
        amount: 5,
        note: String::new(),
        about: None,
        payout: false,
    }) else {
        panic!("a receipt");
    };
    let word = cadrs_social::codec::encode(&receipt);
    let notices = |app: &App| app.world().resource::<Notifications>().items.len();
    let before = notices(&sipho);
    // From the payer's device, or from nobody (out of a mailbox): not taken.
    for from in [Some(device.public()), None] {
        assert!(credits::credited(sipho.world_mut(), &word, from).is_err());
    }
    assert_eq!(notices(&sipho), before);
    // From the bank: news, once.
    credits::credited(sipho.world_mut(), &word, Some(bank_id)).unwrap();
    assert_eq!(notices(&sipho), before + 1);
    until(&mut [&mut bank, &mut sipho], "the wallet has it", |a| {
        balance(a[1], &sipho_id) == Some(5)
    });
    let asking = |app: &App| app.world().resource::<Credits>().wallets[&sipho_id].asking;
    for _ in 0..50 {
        credits::credited(sipho.world_mut(), &word, Some(bank_id)).unwrap();
        assert!(
            !asking(&sipho),
            "told again is nothing to ask the bank about"
        );
    }
    assert_eq!(notices(&sipho), before + 1);
}

/// A bank that doesn't answer (away, or a seed from before there were banks) is asked now and
/// then, not over and over: what draws an organization's wallet also tells the bank who may
/// spend from it and asks after its orders, and must not be drawn again by the failure of its
/// own asking. (That loop made the organization's screen again every frame: its navigation
/// took no clicks and its QR code flickered.)
#[test]
fn a_bank_that_does_not_answer_is_not_asked_over_and_over() {
    let (mut sipho, _) = app("QuietSipho");
    sipho.insert_resource(BankAt {
        id: SecretKey::generate().public(),
        here: false,
    });
    assert!(create(
        sipho.world_mut(),
        NewOrg {
            name: "Cape Waterjet".into(),
            tagline: "Waterjet cutting in steel, aluminium and glass".into(),
            offers: cadrs_org::Offers {
                manufacturing: true,
                designs: false,
            },
            email: "sales@shop.example".into(),
            logo: logo(),
            location: None,
        }
    ));
    let shop = sipho
        .world()
        .resource::<OpenOrg>()
        .session
        .org()
        .org
        .unwrap();
    let org = sipho.world().resource::<OpenOrg>().session.org().clone();
    // The screen, as `organization::refresh_body` draws a module that shows the wallet: again
    // whenever what the bank said changed, telling and asking each time it draws.
    let mut drawn = 0;
    let mut seen = None;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let version = sipho.world().resource::<Credits>().version;
        if seen != Some(version) {
            seen = Some(version);
            drawn += 1;
            credits::say_access(sipho.world_mut(), shop, &org);
            credits::name_holds(sipho.world_mut(), shop, vec![cadrs_credits::new_id()]);
            credits::watch(sipho.world_mut(), shop);
        }
        sipho.update();
    }
    let credits = sipho.world().resource::<Credits>();
    assert!(drawn <= 4, "drawn {drawn} times in three seconds");
    assert!(credits.asked <= 8, "asked {} times", credits.asked);
    // And it says, once, that the bank didn't answer.
    assert!(credits.wallets[&shop].error.is_some());
    assert!(credits.wallets[&shop].statement.is_none());
}
