//! Credits between two running apps over real iroh on this machine (no relay): a client and a
//! shop, each keeping its own wallets (there is no bank: `cadrs_credits::trusting`). The client
//! takes from the faucet and pays the shop; the shop's device is told as it happens and keeps
//! the payment in the organization's document; the shop pays its owner out; an order's payment
//! is held, and released when the client signs for the delivery.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use cadrs_app::credits::{self, Credits, CreditsPlugin, Payment};
use cadrs_app::notifications::Notifications;
use cadrs_app::organization::{NewOrg, OpenOrg, advance, create, deliver, instant_quote, write};
use cadrs_app::social::{PeerAddrs, Social, SocialPlugin, add_contact, ensure_loaded, send};
use cadrs_app::{AppClock, AppState, DocumentStore, UserProfile};
use cadrs_core::Store;
use cadrs_credits::{About, Kind};
use cadrs_net::NetPlugin;
use cadrs_net::bevy_iroh::{Iroh, Relays};
use cadrs_social::keys::AccountId;

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

fn introduce(a: &mut App, b: &App) {
    let addr = b.world().resource::<Iroh>().addr().expect("online");
    a.world_mut()
        .get_resource_or_insert_with(PeerAddrs::default)
        .0
        .insert(addr.id, addr);
}

fn shop_called(shop: &mut App, name: &str) -> AccountId {
    assert!(create(
        shop.world_mut(),
        NewOrg {
            name: name.into(),
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
    shop.world()
        .resource::<OpenOrg>()
        .session
        .org()
        .org
        .expect("the organization's account")
}

fn payment(from: AccountId, to: AccountId, amount: u64) -> Payment {
    Payment {
        from,
        to,
        to_name: "them".into(),
        amount,
        note: String::new(),
        about: None,
        payout: false,
    }
}

#[test]
fn credits_move_between_a_person_and_a_shop_as_they_happen() {
    let (mut marco, _) = app("Marco");
    let (mut sipho, _) = app("Sipho");
    let (marco_id, sipho_id) = (me(&marco), me(&sipho));
    let shop = shop_called(&mut sipho, "Cape Waterjet");
    let card = sipho.world().resource::<Social>().org_card(&shop).unwrap();
    credits::watch(sipho.world_mut(), shop);
    credits::refresh(sipho.world_mut(), sipho_id);

    // Marco takes from the faucet: a thousand, and an hour to wait.
    credits::faucet(marco.world_mut());
    until(&mut [&mut marco], "the faucet gave", |a| {
        balance(a[0], &marco_id) == Some(1000)
    });
    let statement = marco.world().resource::<Credits>().wallets[&marco_id]
        .statement
        .clone()
        .unwrap();
    assert!(statement.faucet_at.is_some_and(|at| at > statement.time));
    credits::faucet(marco.world_mut());
    until(&mut [&mut marco], "not twice in the hour", |a| {
        a[0].world().resource::<Credits>().wallets[&marco_id]
            .error
            .is_some()
    });
    assert_eq!(balance(&marco, &marco_id), Some(1000));

    // He pays the shop 300 while his cadrs doesn't know where the shop's device is: his
    // wallet has paid, and the shop is told once it can be reached, without anything pressed.
    assert!(add_contact(marco.world_mut(), card));
    let before = sipho.world().resource::<Notifications>().items.len();
    let deposit = Payment {
        note: "deposit".into(),
        ..payment(marco_id, shop, 300)
    };
    assert!(credits::pay(marco.world_mut(), deposit, None));
    until(&mut [&mut marco, &mut sipho], "paid on his side", |a| {
        balance(a[0], &marco_id) == Some(700)
    });
    for _ in 0..20 {
        marco.update();
        sipho.update();
    }
    assert_eq!(balance(&sipho, &shop), Some(0), "not told yet");
    introduce(&mut marco, &sipho);
    introduce(&mut sipho, &marco);
    until(&mut [&mut marco, &mut sipho], "the shop was told", |a| {
        balance(a[1], &shop) == Some(300)
    });
    assert_eq!(
        sipho.world().resource::<Notifications>().items.len(),
        before + 1
    );
    let shown = &sipho.world().resource::<Credits>().wallets[&shop].movements[0];
    assert_eq!(
        (shown.amount, shown.note.as_str(), shown.from),
        (300, "deposit", Some(marco_id))
    );
    // The shop's wallet is in the organization's document, where its other members' devices
    // find it.
    let kept = |app: &App| {
        app.world()
            .resource::<OpenOrg>()
            .session
            .org()
            .credits
            .len()
    };
    assert_eq!(kept(&sipho), 1);

    // From here on they reach each other at once.
    let asked = Instant::now();
    assert!(credits::pay(
        marco.world_mut(),
        payment(marco_id, shop, 100),
        None
    ));
    until(&mut [&mut marco, &mut sipho], "paid again", |a| {
        balance(a[1], &shop) == Some(400)
    });
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );

    // More than he has: refused, said, and nothing moved.
    credits::pay(marco.world_mut(), payment(marco_id, shop, 5000), None);
    until(&mut [&mut marco], "too much is refused", |a| {
        a[0].world().resource::<Credits>().wallets[&marco_id]
            .error
            .is_some()
    });
    assert_eq!(balance(&marco, &marco_id), Some(600));

    // The shop pays its owner out: from the organization's wallet to a person's, both kept on
    // this device.
    let payout = Payment {
        note: "November".into(),
        payout: true,
        ..payment(shop, sipho_id, 120)
    };
    credits::pay(sipho.world_mut(), payout, None);
    until(&mut [&mut sipho], "the owner was paid out", |a| {
        balance(a[0], &shop) == Some(280) && balance(a[0], &sipho_id) == Some(120)
    });
    let paid = &sipho.world().resource::<Credits>().wallets[&sipho_id].movements[0];
    assert_eq!(
        (paid.kind, paid.from, paid.by),
        (Kind::Payout, Some(shop), sipho_id)
    );
    assert_eq!(kept(&sipho), 3);
    // Marco can't spend the shop's credits: its wallet isn't his to ask about.
    credits::pay(marco.world_mut(), payment(shop, marco_id, 100), None);
    until(
        &mut [&mut marco],
        "a stranger spends nothing of the shop's",
        |a| {
            a[0].world()
                .resource::<Credits>()
                .wallet(&shop)
                .is_some_and(|w| w.error.is_some())
        },
    );
    for _ in 0..20 {
        marco.update();
        sipho.update();
    }
    assert_eq!(balance(&sipho, &shop), Some(280));

    // What draws a wallet also asks about it: with nothing happening, nothing is drawn again.
    let mut drawn = 0;
    let mut seen = None;
    for _ in 0..200 {
        let version = sipho.world().resource::<Credits>().version;
        if seen != Some(version) {
            seen = Some(version);
            drawn += 1;
            credits::name_holds(sipho.world_mut(), shop, vec![cadrs_credits::new_id()]);
            credits::watch(sipho.world_mut(), shop);
            credits::refresh(sipho.world_mut(), shop);
        }
        sipho.update();
    }
    assert!(drawn <= 3, "drawn {drawn} times with nothing happening");
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

    let (mut designer, _) = app("Designer");
    let (mut shop, _) = app("Shop");
    introduce(&mut shop, &designer);
    introduce(&mut designer, &shop);
    let me = me(&designer);

    // The shop: a waterjet, a sheet, prices in credits.
    let org = shop_called(&mut shop, "Cape Waterjet");
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
    let card = shop.world().resource::<Social>().org_card(&org).unwrap();
    credits::watch(shop.world_mut(), org);

    // The designer has a thousand credits, adds the shop and asks for a quote.
    credits::faucet(designer.world_mut());
    until(&mut [&mut designer], "the faucet gave", |a| {
        balance(a[0], &me) == Some(1000)
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
        shop.update();
    }
    assert_eq!(balance(&designer, &me), Some(1000));
    assert_eq!(
        designer.world().resource::<Social>().book.sent[&id].stage,
        Stage::Quoted
    );
    cadrs_app::wallet_ui::confirmed(designer.world_mut());
    // Pressed again before anything came back: the order's hold has one id, so it is held once.
    cadrs_app::request_quote::accept_and_pay(designer.world_mut(), id);
    cadrs_app::wallet_ui::confirmed(designer.world_mut());
    // Held for the shop, and only then is the shop told: it sees the order and the payment.
    until(
        &mut [&mut designer, &mut shop],
        "accepted, with the payment held",
        |a| {
            let accepted = a[1].world().resource::<OpenOrg>().session.org().requests[&id].stage
                == Stage::Accepted;
            accepted
                && matches!(
                    credits::payment_of(a[0].world(), &me, id),
                    Some(OrderPayment::Held(_))
                )
                && matches!(
                    credits::payment_of(a[1].world(), &org, id),
                    Some(OrderPayment::Held(_))
                )
        },
    );
    assert_eq!(balance(&designer, &me), Some(276));
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
        (
            named(&shop)[0].id,
            named(&shop)[0].amount,
            named(&shop)[0].state
        ),
        (order_hold, 724, cadrs_credits::HoldState::Open)
    );

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
    cadrs_app::request_quote::sign_delivery(designer.world_mut(), id);
    until(
        &mut [&mut designer, &mut shop],
        "signed for and paid",
        |a| {
            balance(a[1], &org) == Some(724)
                && a[1].world().resource::<OpenOrg>().session.org().requests[&id].stage
                    == Stage::Completed
                && credits::payment_of(a[0].world(), &me, id) == Some(OrderPayment::Paid(724))
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
            about: Some(About::order(other, "plate")),
            ..payment(me, org, 200)
        },
        None,
    );
    until(
        &mut [&mut designer, &mut shop],
        "the second payment is held",
        |a| credits::held_for(a[1].world(), &org, other).is_some(),
    );
    let hold = credits::held_for(shop.world(), &org, other).unwrap();
    credits::give_back(shop.world_mut(), org, hold.id, None);
    until(
        &mut [&mut designer, &mut shop],
        "the payment went back",
        |a| {
            balance(a[0], &me) == Some(276)
                && credits::payment_of(a[0].world(), &me, other)
                    == Some(OrderPayment::Returned(200))
        },
    );
    assert_eq!(balance(&shop, &org), Some(724));
}
