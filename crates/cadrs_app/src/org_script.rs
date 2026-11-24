//! Set-ups for the organization and marketplace scenarios (`Custom("org …")`): what a scenario
//! can't do with clicks, because it needs a picture file or other people.
//!
//! - `org logo [<width> <height>]`: a picture is picked for the New organization dialog's logo
//!   (512 × 512 unless given; a small or oblong one shows what the dialog says of it).
//! - `org market`: three shops are heard of in the marketplace, with their logos.
//! - `org request`: a client asks the open organization for a quote.
//! - `org bank`: this app keeps a bank of its own, in memory (the network is off in
//!   scenarios), so the wallet works.
//! - `org pay <credits>`: a client pays the open organization that much, as the bank would
//!   tell this device.
//! - `org stray <n>`: a stranger holds `n` single credits for the open organization, for no
//!   order of its.
//! - `org accept`: the client of the newest quoted request accepts and pays: the quote's
//!   credits are held for the organization, and the request becomes an order.

use bevy::prelude::*;
use cadrs_org::capability::{PartSummary, Process};
use cadrs_org::directory::{Offer, ShopListing};
use cadrs_org::{BlobRef, Offers, Op, new_id};
use cadrs_social::contact::{Card, Kind};
use cadrs_social::keys::{Account, SecretKey};
use cadrs_social::place::{Place, Region};

/// A logo-like picture: a white ring and bar on a colour of its own.
pub(crate) fn sample_picture(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    let r = f64::from(width.min(height)) / 2.0;
    let picture = image::RgbaImage::from_fn(width, height, |x, y| {
        let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
        let d = (dx * dx + dy * dy).sqrt() / r;
        let ring = (0.5..0.68).contains(&d);
        let bar = d < 0.5 && (dx + dy).abs() / r < 0.12;
        if ring || bar {
            image::Rgba([255, 255, 255, 255])
        } else {
            image::Rgba([color[0], color[1], color[2], 255])
        }
    });
    let mut png = Vec::new();
    let _ = picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

pub(crate) fn script(world: &mut World, arg: &str) {
    let mut words = arg.split_whitespace();
    match words.next() {
        Some("logo") => {
            let mut side = || words.next().and_then(|w| w.parse::<u32>().ok());
            let (w, h) = (side().unwrap_or(512), side().unwrap_or(512));
            let made = crate::org_logo::process(&sample_picture(w, h, [0x1f, 0x5f, 0xbf]));
            crate::org_create::take_logo(world, crate::org_create::LOGO_FOR_NEW, made);
        }
        Some("market") => market(world),
        Some("request") => request(world),
        Some("bank") => {
            crate::credits::keep_here(world);
        }
        Some("accept") => client_accepts(world),
        Some("stray") => {
            let count = words.next().and_then(|w| w.parse().ok()).unwrap_or(1);
            stranger_holds(world, count);
        }
        Some("pay") => {
            let amount = words.next().and_then(|w| w.parse().ok()).unwrap_or(100);
            client_pays(world, amount);
        }
        other => warn!("org: unknown set-up {other:?}"),
    }
}

/// A client with credits of their own (from the faucet) pays the open organization `amount`,
/// and the bank tells this device, as it would over the network.
fn client_pays(world: &mut World, amount: u64) {
    use cadrs_credits::{Answer, Ask};
    let Some(at) = crate::credits::keep_here(world) else {
        return;
    };
    let Some(shop) = world
        .get_resource::<crate::organization::OpenOrg>()
        .and_then(|open| open.session.org().org)
    else {
        return;
    };
    let now = cadrs_social::wall_millis();
    let client = Account::generate();
    let device = SecretKey::generate();
    let cert = client.certify(device.public(), "laptop", now);
    let asked = |world: &mut World, ask: Ask| {
        let bytes = cadrs_credits::ask(&device, &cert, at.id, client.id(), ask, now);
        cadrs_net::bank::answer(world, &bytes, None)
            .ok()
            .and_then(|answer| cadrs_credits::read_answer(&answer, &at.id).ok())
    };
    // As many takes from the faucet as it needs: a client who saved up.
    let id = cadrs_credits::new_id;
    asked(world, Ask::Faucet { id: id() });
    let transfer = Ask::Transfer {
        id: id(),
        to: shop,
        amount: amount.min(cadrs_credits::FAUCET),
        note: "deposit".into(),
        about: None,
        payout: false,
    };
    if let Some(Answer::Receipt(receipt)) = asked(world, transfer) {
        let _ = crate::credits::credited(world, &cadrs_social::codec::encode(&receipt), None);
    }
}

/// A stranger holds `count` single credits for the open organization, about nothing it knows.
fn stranger_holds(world: &mut World, count: u32) {
    use cadrs_credits::{Answer, Ask};
    let Some(at) = crate::credits::keep_here(world) else {
        return;
    };
    let Some(shop) = world
        .get_resource::<crate::organization::OpenOrg>()
        .and_then(|open| open.session.org().org)
    else {
        return;
    };
    let now = cadrs_social::wall_millis();
    let stranger = Account::generate();
    let device = SecretKey::generate();
    let cert = stranger.certify(device.public(), "laptop", now);
    let asked = |world: &mut World, ask: Ask| {
        let bytes = cadrs_credits::ask(&device, &cert, at.id, stranger.id(), ask, now);
        cadrs_net::bank::answer(world, &bytes, None)
            .ok()
            .and_then(|answer| cadrs_credits::read_answer(&answer, &at.id).ok())
    };
    asked(
        world,
        Ask::Faucet {
            id: cadrs_credits::new_id(),
        },
    );
    for _ in 0..count {
        let hold = Ask::Hold {
            id: cadrs_credits::new_id(),
            to: shop,
            amount: 1,
            note: String::new(),
            about: None,
        };
        if let Some(Answer::Receipt(receipt)) = asked(world, hold) {
            let _ = crate::credits::credited(world, &cadrs_social::codec::encode(&receipt), None);
        }
    }
}

/// The client of the newest quoted request accepts its quote and pays: the credits are held at
/// the bank for the organization (which this device is told), and the acceptance arrives as its
/// letter would.
fn client_accepts(world: &mut World) {
    use cadrs_credits::{About, Answer, Ask};
    let Some(at) = crate::credits::keep_here(world) else {
        return;
    };
    let Some(open) = world.get_resource::<crate::organization::OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let Some(shop) = org.org else {
        return;
    };
    let quoted = org
        .requests
        .iter()
        .filter(|(_, r)| r.stage == cadrs_org::Stage::Quoted)
        .max_by_key(|(_, r)| r.number)
        .and_then(|(id, r)| Some((*id, r.number, r.quote.clone()?)));
    let Some((id, number, quote)) = quoted else {
        warn!("org accept: no quoted request");
        return;
    };
    let now = cadrs_social::wall_millis();
    // Whoever asked for it pays for it.
    let (payer, device) = match world.get_resource::<ScriptClient>() {
        Some(client) => (
            Account::from_key(client.account.key().clone()),
            SecretKey::from_bytes(&client.device.to_bytes()),
        ),
        None => (Account::generate(), SecretKey::generate()),
    };
    let cert = payer.certify(device.public(), "laptop", now);
    let asked = |world: &mut World, ask: Ask| {
        let bytes = cadrs_credits::ask(&device, &cert, at.id, payer.id(), ask, now);
        cadrs_net::bank::answer(world, &bytes, None)
            .ok()
            .and_then(|answer| cadrs_credits::read_answer(&answer, &at.id).ok())
    };
    asked(
        world,
        Ask::Faucet {
            id: cadrs_credits::new_id(),
        },
    );
    let hold = Ask::Hold {
        // The order's own id, as the client's cadrs asks with.
        id: cadrs_credits::order_hold_id(id, quote.id),
        to: shop,
        amount: quote.total.credits().min(cadrs_credits::FAUCET),
        note: String::new(),
        about: Some(About::order(
            id,
            crate::organization::request_number(number),
        )),
    };
    if let Some(Answer::Receipt(receipt)) = asked(world, hold) {
        let _ = crate::credits::credited(world, &cadrs_social::codec::encode(&receipt), None);
    }
    let _ = crate::organization::take_op(
        world,
        Op::Accept {
            request: id,
            quote: quote.id,
            at: now,
        },
    );
}

/// Three shops, as the public room would say them: signed by keys of their own.
fn market(world: &mut World) {
    crate::social::ensure_loaded(world);
    let now = cadrs_social::wall_millis();
    let jet = |process, bed: [f64; 3], materials: &[(&str, f64)]| Offer {
        process,
        envelope: bed,
        materials: materials
            .iter()
            .map(|(m, mm)| ((*m).to_owned(), *mm))
            .collect(),
    };
    let shops = [
        (
            "Cape Waterjet",
            "Waterjet cutting in steel, aluminium and glass",
            "Two 3 × 1.5 m tables. Drawings in by noon are cut the next day.",
            [0x1f, 0x5f, 0xbf],
            Some(("Stellenbosch", "South Africa", -33.93, 18.86)),
            vec![jet(
                Process::Waterjet,
                [3000.0, 1500.0, 0.0],
                &[
                    ("Steel - A36", 100.0),
                    ("Aluminum - 6061", 80.0),
                    ("glass", 25.0),
                ],
            )],
            Some((4.8, 12)),
        ),
        (
            "Rhine Laser",
            "Sheet metal laser cutting and bending, from one part to a thousand",
            "",
            [0xb0, 0x3a, 0x2e],
            Some(("Cologne", "Germany", 50.94, 6.96)),
            vec![
                jet(
                    Process::Laser,
                    [3000.0, 1500.0, 0.0],
                    &[("Steel - A36", 20.0), ("Stainless Steel - 304", 12.0)],
                ),
                jet(Process::PressBrake, [3000.0, 0.0, 0.0], &[]),
            ],
            Some((4.5, 31)),
        ),
        (
            "Open Gantry",
            "Open designs for CNC machines you can build yourself",
            "",
            [0x2e, 0x8b, 0x57],
            None,
            vec![],
            None,
        ),
    ];
    for (name, tagline, about, color, place, offers, rating) in shops {
        let shop = Account::generate();
        let card = Card {
            account: shop.id(),
            kind: Kind::Organization,
            name: name.into(),
            devices: vec![SecretKey::generate().public()],
            mailboxes: vec![],
            issued: now,
        }
        .sign(shop.key());
        let logo = crate::org_logo::process(&sample_picture(512, 512, color)).ok();
        let kept = |world: &mut World, bytes: &[u8]| {
            crate::social::keep_logo(world, bytes);
            BlobRef::of(bytes)
        };
        let listing = ShopListing {
            tagline: tagline.into(),
            about: about.into(),
            kinds: Offers {
                manufacturing: !offers.is_empty(),
                designs: offers.is_empty(),
            },
            logo: logo.as_ref().map(|l| kept(world, &l.thumb)),
            logo_full: logo.as_ref().map(|l| kept(world, &l.full)),
            places: place
                .map(|(city, country, lat, lon)| {
                    Place {
                        region: Some(Region {
                            city: Some(city.into()),
                            area: None,
                            country: Some(country.into()),
                        }),
                        ..Place::earth(lat, lon)
                    }
                    .coarsened(5_000.0)
                })
                .into_iter()
                .collect(),
            offers,
            email: format!("hello@{}.example", name.to_lowercase().replace(' ', "")),
            website: None,
            rating,
            withdrawn: false,
            ..ShopListing::withdrawal(card, now)
        };
        if let Some(bytes) = cadrs_org::directory::sign(shop.key(), &listing) {
            let heard = world
                .resource_mut::<crate::social::Social>()
                .directory
                .hear(&bytes, now);
            if let Err(why) = heard {
                warn!("org market: {name} was refused: {why:?}");
            }
        }
    }
}

/// The scripted client who last asked for a quote: their account's key and their device's.
#[derive(Resource)]
struct ScriptClient {
    account: Account,
    device: SecretKey,
}

/// A client's request for six brackets in 10 mm steel (the Part Studio's Steel - A36), as its letter would bring it.
fn request(world: &mut World) {
    let client = Account::generate();
    let device = SecretKey::generate();
    // Kept for the run: the same client pays for what they asked for (`org accept`).
    world.insert_resource(ScriptClient {
        account: Account::from_key(client.key().clone()),
        device: SecretKey::from_bytes(&device.to_bytes()),
    });
    let card = Card {
        account: client.id(),
        kind: Kind::Person,
        name: "Alex Maker".into(),
        devices: vec![device.public()],
        mailboxes: vec![],
        issued: cadrs_social::wall_millis(),
    }
    .sign(client.key());
    let bracket = PartSummary {
        name: "Gate bracket".into(),
        flat_thickness: Some(10.0),
        size: [200.0, 80.0, 10.0],
        min_hole: Some(8.5),
        min_inside_radius: Some(3.0),
        cut_length: 900.0,
        pierces: 3,
        area: 12_000.0,
        material: Some("Steel - A36".into()),
        tolerance: None,
    };
    // As its letter would bring it: not the person's own doing.
    let _ = crate::organization::take_op(
        world,
        Op::Request {
            id: new_id(),
            client: client.id(),
            parts: vec![(bracket, 6)],
            notes: "Brushed finish, please.".into(),
            snapshot: [0; 32],
            snapshot_len: 0,
            deadline: None,
            card: Some(card),
        },
    );
}
