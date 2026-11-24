//! Set-ups for the organization and marketplace scenarios (`Custom("org …")`): what a scenario
//! can't do with clicks, because it needs a picture file or other people.
//!
//! - `org logo [<width> <height>]`: a picture is picked for the New organization dialog's logo
//!   (512 × 512 unless given; a small or oblong one shows what the dialog says of it).
//! - `org market`: three shops are heard of in the marketplace, with their logos.
//! - `org request`: a client asks the open organization for a quote.
//! - `org bank`: nothing, any more (there is no bank: every cadrs keeps its own wallets).
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
        // (There is no bank to set up any more: every cadrs keeps its own wallets.)
        Some("bank") => {}
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

/// Someone else's cadrs, for a scenario: an account with a device, which pays by sending the
/// notes another cadrs would.
struct Payer {
    account: Account,
    device: SecretKey,
}

impl Payer {
    fn new() -> Self {
        Self {
            account: Account::generate(),
            device: SecretKey::generate(),
        }
    }

    /// The scripted client who last asked for a quote, else someone new.
    fn client(world: &World) -> Self {
        match world.get_resource::<ScriptClient>() {
            Some(client) => Self {
                account: Account::from_key(client.account.key().clone()),
                device: SecretKey::from_bytes(&client.device.to_bytes()),
            },
            None => Self::new(),
        }
    }

    /// Tells this cadrs of a movement of the payer's doing, as its note would arrive.
    fn tells(
        &self,
        world: &mut World,
        kind: cadrs_credits::Kind,
        id: cadrs_credits::MoveId,
        to: cadrs_social::keys::AccountId,
        amount: u64,
        about: Option<cadrs_credits::About>,
    ) {
        let now = cadrs_social::wall_millis();
        let cert = self.account.certify(self.device.public(), "laptop", now);
        let movement = cadrs_credits::Movement {
            seq: 0,
            // What the payer's own cadrs would call it.
            id: cadrs_credits::scoped_id(&self.account.id(), &id),
            time: now,
            kind,
            from: Some(self.account.id()),
            to,
            amount,
            // A plain payment says what it is; a held one says its order.
            note: if kind == cadrs_credits::Kind::Transfer {
                "deposit".to_owned()
            } else {
                String::new()
            },
            about,
            by: self.account.id(),
            hold: None,
        };
        let bytes = cadrs_credits::write_note(&self.device, &cert, &movement);
        if let Err(why) = crate::credits::credited(world, &bytes) {
            warn!("the scripted payment wasn't taken: {why}");
        }
    }
}

fn open_shop(world: &World) -> Option<cadrs_social::keys::AccountId> {
    world
        .get_resource::<crate::organization::OpenOrg>()
        .and_then(|open| open.session.org().org)
}

/// A client pays the open organization `amount`, as their note would arrive.
fn client_pays(world: &mut World, amount: u64) {
    let Some(shop) = open_shop(world) else {
        return;
    };
    Payer::new().tells(
        world,
        cadrs_credits::Kind::Transfer,
        cadrs_credits::new_id(),
        shop,
        amount,
        None,
    );
}

/// A stranger holds `count` single credits for the open organization, about nothing it knows.
fn stranger_holds(world: &mut World, count: u32) {
    let Some(shop) = open_shop(world) else {
        return;
    };
    let stranger = Payer::new();
    for _ in 0..count {
        stranger.tells(
            world,
            cadrs_credits::Kind::Held,
            cadrs_credits::new_id(),
            shop,
            1,
            None,
        );
    }
}

/// The client of the newest quoted request accepts its quote and pays: the credits are held
/// for the organization (their note arrives), and the acceptance arrives as its letter would.
fn client_accepts(world: &mut World) {
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
    // Whoever asked for it pays for it, under the order's own id, as their cadrs does.
    Payer::client(world).tells(
        world,
        cadrs_credits::Kind::Held,
        cadrs_credits::order_hold_id(id, quote.id),
        shop,
        quote.total.credits(),
        Some(cadrs_credits::About::order(
            id,
            crate::organization::request_number(number),
        )),
    );
    let _ = crate::organization::take_op(
        world,
        Op::Accept {
            request: id,
            quote: quote.id,
            at: cadrs_social::wall_millis(),
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
