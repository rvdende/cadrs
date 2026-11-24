//! A water-cutting shop's life in its organization document, over simulated peers: founding it,
//! adding a box and a member, setting up machines and prices, a request for a quote that
//! becomes an order, two members racing for the last sheet, delivery with a co-signed receipt,
//! and the client's review.

use cadrs_org::capability::{MachineCaps, MaterialLimit, PartSummary, Process};
use cadrs_org::profile::{Missing, ProfileEdit, missing};
use cadrs_org::quality::{Disposition, IssueAbout, IssueStatus};
use cadrs_org::quote::{Money, RateCard, estimate};
use cadrs_org::{
    Address, BlobRef, Location, Logo, Material, Offers, Op, Org, Profile, Quote, Reservation,
    Review, Role, Stage, new_id, order_ref,
};
use cadrs_social::keys::{Account, SecretKey};
use cadrs_social::log::{Anyone, Kind, Log, LogId, Outcome, Witnesses};
use cadrs_social::receipts::Receipt;

struct Peer {
    key: SecretKey,
    log: Log,
}

struct Shop {
    org: Account,
    owner: Account,
    laptop: Peer,
    shop_box: Peer,
    worker: Account,
    worker_pc: Peer,
    wall: u64,
}

impl Shop {
    fn new() -> Self {
        let id = LogId::generate();
        let peer = || Peer {
            key: SecretKey::generate(),
            log: Log::new(id),
        };
        Self {
            org: Account::generate(),
            owner: Account::generate(),
            laptop: peer(),
            shop_box: peer(),
            worker: Account::generate(),
            worker_pc: peer(),
            wall: 1_800_000_000_000,
        }
    }

    fn peers(&mut self) -> [&mut Peer; 3] {
        [&mut self.laptop, &mut self.shop_box, &mut self.worker_pc]
    }

    fn write(&mut self, who: usize, op: Op) {
        self.wall += 1_000;
        let wall = self.wall;
        let peer = &mut self.peers()[who];
        peer.log.append(&peer.key, Kind::Data(op.encode()), wall);
    }

    fn sync_all(&mut self) {
        let wall = self.wall;
        for from in 0..3 {
            for to in 0..3 {
                if from != to {
                    let heads = self.peers()[to].log.heads().clone();
                    let send = self.peers()[from].log.since(&heads);
                    for entry in send {
                        self.peers()[to].log.insert(entry, &Anyone, wall).unwrap();
                    }
                }
            }
        }
    }

    fn state(&self, log: &Log) -> (Org, Vec<String>) {
        let mut org = Org::default();
        let outcomes = log.replay(&Witnesses::new(), &mut org);
        let conflicts = outcomes
            .into_iter()
            .filter_map(|(_, o)| match o {
                Outcome::Conflicted(why) => Some(why),
                Outcome::Applied => None,
            })
            .collect();
        (org, conflicts)
    }
}

const LAPTOP: usize = 0;
const BOX: usize = 1;
const WORKER: usize = 2;

fn waterjet() -> MachineCaps {
    MachineCaps {
        name: "Waterjet".into(),
        process: Process::Waterjet,
        envelope: [3000.0, 1500.0, 150.0],
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
fn a_shop_from_founding_to_review() {
    let mut shop = Shop::new();
    let designer = Account::generate();
    let wall = shop.wall;

    // The owner founds the shop from their laptop, then adds the shop's box as a witness.
    let founder = shop.owner.certify(shop.laptop.key.public(), "laptop", wall);
    shop.write(
        LAPTOP,
        Op::Found {
            name: "Cape Waterjet".into(),
            org: shop.org.id(),
            founder,
        },
    );
    let box_cert = shop
        .owner
        .certify(shop.shop_box.key.public(), "shop box", wall);
    shop.write(LAPTOP, Op::AddDevice(box_cert));
    let box_device = shop.shop_box.key.public();
    shop.write(
        LAPTOP,
        Op::SetWitness {
            device: box_device,
            on: true,
        },
    );
    shop.write(
        LAPTOP,
        Op::SetProfile(Profile {
            name: "Cape Waterjet".into(),
            offers: makes_parts(),
            listed: true,
            ..Default::default()
        }),
    );

    // A one-person shop grows: a worker joins.
    shop.write(
        LAPTOP,
        Op::SetMember {
            account: shop.worker.id(),
            role: Some(Role::Member),
        },
    );
    let worker_cert = shop.worker.certify(shop.worker_pc.key.public(), "pc", wall);
    shop.write(LAPTOP, Op::AddDevice(worker_cert));

    // Set-up: a machine, a material with one sheet left, prices.
    let machine = new_id();
    let steel = new_id();
    shop.write(
        LAPTOP,
        Op::SetMachine {
            id: machine,
            machine: Some(waterjet()),
        },
    );
    shop.write(
        LAPTOP,
        Op::SetMaterial {
            id: steel,
            material: Some(Material {
                name: "mild steel".into(),
                thickness: 10.0,
                sheet: [2500.0, 1250.0],
            }),
        },
    );
    shop.write(
        LAPTOP,
        Op::Stock {
            material: steel,
            sheets: 1,
        },
    );
    let rates = RateCard {
        currency: "ZAR".into(),
        setup: Money::from_major(250.0),
        minimum: Money::from_major(500.0),
        hourly: vec![cadrs_org::quote::MachineRate::new(
            machine,
            Money::from_major(1200.0),
        )],
        sheet_per_m2: vec![cadrs_org::quote::SheetPrice::new(
            "mild steel",
            10.0,
            Money::from_major(1000.0),
        )],
        nesting_allowance: 1.25,
    };
    shop.write(LAPTOP, Op::SetRates(rates.clone()));
    shop.sync_all();

    // The worker may not change prices.
    shop.write(WORKER, Op::SetRates(rates.clone()));

    // Two requests arrive at the box; both get quoted and accepted.
    let (first, second) = (new_id(), new_id());
    for request in [first, second] {
        shop.write(
            BOX,
            Op::Request {
                id: request,
                client: designer.id(),
                parts: vec![(bracket(), 6)],
                notes: "brushed".into(),
                snapshot: [3; 32],
                snapshot_len: 1_024,
                deadline: None,
                card: None,
            },
        );
        let (org, _) = shop.state(&shop.shop_box.log);
        let estimate = estimate(
            &org.machines,
            org.rates.as_ref().unwrap(),
            &org.requests[&request].parts,
        )
        .unwrap();
        assert_eq!(estimate.total, Money::from_major(724.0));
        shop.write(
            BOX,
            Op::Quote {
                request,
                quote: cadrs_org::Quote {
                    id: new_id(),
                    total: estimate.total,
                    currency: estimate.currency,
                    lines: estimate.lines,
                    valid_until: shop.wall + 7 * 86_400_000,
                    note: String::new(),
                },
            },
        );
        shop.write(
            BOX,
            Op::Accept {
                request,
                quote: 0,
                at: 0,
            },
        );
    }
    shop.sync_all();

    // Apart, the owner and the worker each reserve the last sheet for a different order.
    shop.write(
        LAPTOP,
        Op::Reserve {
            id: new_id(),
            reservation: Reservation {
                material: steel,
                sheets: 1,
                request: first,
            },
        },
    );
    shop.write(
        WORKER,
        Op::Reserve {
            id: new_id(),
            reservation: Reservation {
                material: steel,
                sheets: 1,
                request: second,
            },
        },
    );
    shop.sync_all();

    // Every peer agrees on which reservation won and which conflicted.
    let states: Vec<(Org, Vec<String>)> =
        [&shop.laptop.log, &shop.shop_box.log, &shop.worker_pc.log]
            .into_iter()
            .map(|log| shop.state(log))
            .collect();
    for (org, conflicts) in &states {
        assert_eq!(org.available(steel), 0);
        assert_eq!(org.reservations.len(), 1);
        assert_eq!(conflicts.len(), 2, "{conflicts:?}");
        assert!(conflicts[0].contains("Member may not"));
        assert!(conflicts[1].contains("only 0 sheets are free"));
        assert_eq!(
            org.reservations.values().next(),
            states[0].0.reservations.values().next()
        );
    }

    // The first order is cut and delivered: shop and client sign the receipt.
    shop.write(
        BOX,
        Op::Advance {
            request: first,
            stage: Stage::InProduction,
        },
    );
    let receipt = Receipt::OrderCompleted {
        shop: shop.org.id(),
        client: designer.id(),
        order: order_ref(first),
        at: shop.wall,
    }
    .sign_first(shop.org.key())
    .unwrap()
    .countersign(designer.key())
    .unwrap();
    shop.write(
        BOX,
        Op::Complete {
            request: first,
            receipt: receipt.clone(),
        },
    );
    // The client's review arrives, with the receipt.
    let review = Review {
        by: designer.id(),
        stars: 5,
        text: "Clean edges, quick.".into(),
        request: first,
    };
    shop.write(
        BOX,
        Op::Review {
            id: new_id(),
            review: review.clone(),
            receipt: receipt.clone(),
        },
    );
    // A review of an order that isn't complete is refused.
    shop.write(
        BOX,
        Op::Review {
            id: new_id(),
            review: Review {
                request: second,
                ..review
            },
            receipt,
        },
    );
    shop.sync_all();

    let (org, conflicts) = shop.state(&shop.worker_pc.log);
    assert_eq!(org.requests[&first].stage, Stage::Completed);
    assert_eq!(org.requests[&second].stage, Stage::Accepted);
    assert_eq!(org.rating(), Some((5.0, 1)));
    assert_eq!(conflicts.len(), 3);
    assert!(conflicts[2].contains("completed"));
    assert_eq!(org.members.len(), 2);

    // The box is the org's witness, weighted by the owner's role.
    let witnesses = org.witnesses(|_| Default::default());
    assert_eq!(witnesses.get(&box_device), Some(&3));
}

#[test]
fn strangers_and_removed_members_change_nothing() {
    let mut shop = Shop::new();
    let wall = shop.wall;
    let founder = shop.owner.certify(shop.laptop.key.public(), "laptop", wall);
    shop.write(
        LAPTOP,
        Op::Found {
            name: "Shop".into(),
            org: shop.org.id(),
            founder,
        },
    );
    // The worker's PC writes before it's a member.
    shop.sync_all();
    shop.write(
        WORKER,
        Op::SetProfile(Profile {
            name: "Hijacked".into(),
            ..Default::default()
        }),
    );
    shop.sync_all();
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(org.profile.name, "Shop");
    assert_eq!(conflicts, vec!["the author is not a member".to_string()]);

    // The only owner can't remove themselves.
    shop.write(
        LAPTOP,
        Op::SetMember {
            account: shop.owner.id(),
            role: None,
        },
    );
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(org.members.len(), 1);
    assert!(conflicts[1].contains("at least one owner"));

    // Nor make themselves less: with no owner, nobody could ever set the organization up again.
    for role in [Role::Admin, Role::Member] {
        shop.write(
            LAPTOP,
            Op::SetMember {
                account: shop.owner.id(),
                role: Some(role),
            },
        );
    }
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(org.members[&shop.owner.id()], Role::Owner);
    assert_eq!(conflicts.len(), 4);
    assert!(
        conflicts[2..]
            .iter()
            .all(|c| c.contains("at least one owner"))
    );
    // With a second owner, the first may step down.
    shop.write(
        LAPTOP,
        Op::SetMember {
            account: shop.worker.id(),
            role: Some(Role::Owner),
        },
    );
    shop.write(
        LAPTOP,
        Op::SetMember {
            account: shop.owner.id(),
            role: Some(Role::Admin),
        },
    );
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(conflicts.len(), 4);
    assert_eq!(org.members[&shop.owner.id()], Role::Admin);
    assert_eq!(org.members[&shop.worker.id()], Role::Owner);
}

/// What an organization offers, for one that makes parts.
fn makes_parts() -> Offers {
    Offers {
        manufacturing: true,
        designs: false,
    }
}

/// A [`founded`] shop that makes parts for others, so it takes requests for quotes.
fn manufacturer() -> Shop {
    let mut shop = founded();
    shop.write(
        LAPTOP,
        Op::EditProfile(ProfileEdit {
            offers: Some(makes_parts()),
            ..Default::default()
        }),
    );
    shop
}

fn asks(client: &Account, snapshot_len: u64) -> Op {
    Op::Request {
        id: new_id(),
        client: client.id(),
        parts: vec![(bracket(), 1)],
        notes: String::new(),
        snapshot: [7; 32],
        snapshot_len,
        deadline: None,
        card: None,
    }
}

/// Anyone with an organization's link can ask it for a quote, so what waits unanswered has a
/// ceiling: so many from one client, so many and so much in all, and nothing at all for an
/// organization that doesn't make parts. Declining makes room, and lets the document go.
#[test]
fn requests_from_strangers_have_a_ceiling() {
    // One that only publishes designs takes no requests.
    let mut publisher = founded();
    publisher.write(LAPTOP, asks(&Account::generate(), 0));
    let (org, conflicts) = publisher.state(&publisher.laptop.log);
    assert!(org.requests.is_empty());
    assert!(conflicts[0].contains("doesn't make parts"), "{conflicts:?}");

    let mut shop = manufacturer();
    let eager = Account::generate();
    for _ in 0..cadrs_org::MAX_WAITING_PER_CLIENT + 2 {
        shop.write(LAPTOP, asks(&eager, 1_000));
    }
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(org.requests.len(), cadrs_org::MAX_WAITING_PER_CLIENT);
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts[0].contains("of yours wait"), "{conflicts:?}");
    // Asking for nothing, for too much, or with a document too large isn't a request.
    let nothing = Op::Request {
        id: new_id(),
        client: Account::generate().id(),
        parts: vec![],
        notes: String::new(),
        snapshot: [0; 32],
        snapshot_len: 0,
        deadline: None,
        card: None,
    };
    shop.write(LAPTOP, nothing);
    shop.write(
        LAPTOP,
        asks(&Account::generate(), cadrs_org::MAX_SNAPSHOT + 1),
    );
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!((org.requests.len(), conflicts.len()), (5, 4));

    // Many clients, each within their own share: the documents together have a ceiling too.
    let large = cadrs_org::MAX_SNAPSHOT;
    let fit = (cadrs_org::MAX_WAITING_BYTES - 5 * 1_000) / large;
    for _ in 0..fit + 3 {
        shop.write(LAPTOP, asks(&Account::generate(), large));
    }
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(org.requests.len() as u64, 5 + fit);
    assert_eq!(conflicts.len(), 4 + 3);
    assert!(
        conflicts[6].contains("too many requests wait"),
        "{conflicts:?}"
    );

    // Declining one makes room for another, and its document is no longer one to keep.
    let declined = *org
        .requests
        .iter()
        .find(|(_, r)| r.snapshot_len == large)
        .unwrap()
        .0;
    shop.write(
        LAPTOP,
        Op::Decline {
            request: declined,
            reason: "not something we cut".into(),
        },
    );
    let asked = Op::Request {
        id: 77,
        client: Account::generate().id(),
        parts: vec![(bracket(), 1)],
        notes: String::new(),
        snapshot: [9; 32],
        snapshot_len: 2_000,
        deadline: None,
        card: None,
    };
    shop.write(LAPTOP, asked);
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(conflicts.len(), 7, "{conflicts:?}");
    assert_eq!(org.requests[&declined].stage, Stage::Declined);
    let document = org.requests[&77].snapshot_blob().unwrap();
    assert!(org.blobs().contains(&document));
    shop.write(
        LAPTOP,
        Op::Decline {
            request: 77,
            reason: String::new(),
        },
    );
    let (org, _) = shop.state(&shop.laptop.log);
    assert!(!org.blobs().contains(&document));
}

/// The card that comes with a request is the client's own and no more than a card (a name, a
/// handful of devices): replies go to the devices it names, and it stays in the log for good.
/// And whatever an operation holds, it is no larger than an entry of the log may be.
#[test]
fn a_requests_card_is_a_card_and_an_entry_has_a_size() {
    let mut shop = manufacturer();
    let carded = |client: &Account, card_of: &Account, signer: &Account, name: String, devices| {
        let card = cadrs_social::contact::Card {
            account: card_of.id(),
            kind: cadrs_social::contact::Kind::Person,
            name,
            devices: vec![SecretKey::generate().public(); devices],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(signer.key());
        Op::Request {
            id: new_id(),
            client: client.id(),
            parts: vec![(bracket(), 1)],
            notes: String::new(),
            snapshot: [0; 32],
            snapshot_len: 0,
            deadline: None,
            card: Some(card),
        }
    };
    let (honest, other) = (Account::generate(), Account::generate());
    let bad = [
        carded(&honest, &honest, &honest, "N".repeat(4_000), 1),
        carded(&honest, &honest, &honest, "Many Devices".into(), 50_000),
        // Someone else's card, and one someone else signed.
        carded(&honest, &other, &other, "Other".into(), 1),
        carded(&honest, &honest, &other, "Forged".into(), 1),
    ];
    let refused = bad.len();
    for op in bad {
        shop.write(LAPTOP, op);
    }
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert!(org.requests.is_empty(), "{conflicts:?}");
    assert_eq!(conflicts.len(), refused);
    shop.write(LAPTOP, carded(&honest, &honest, &honest, "Alex".into(), 2));
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!((org.requests.len(), conflicts.len()), (1, refused));

    // A request is only so large as written, whatever its parts add up to: every part's name
    // within its own limit, in characters of four bytes, is together more than a request.
    let heavy = Op::Request {
        id: new_id(),
        client: Account::generate().id(),
        parts: (0..cadrs_org::MAX_REQUEST_PARTS)
            .map(|_| {
                let mut part = bracket();
                part.name = "𝕏".repeat(cadrs_org::MAX_LABEL);
                part.material = Some("𝕏".repeat(cadrs_org::MAX_LABEL));
                (part, 1)
            })
            .collect(),
        notes: String::new(),
        snapshot: [0; 32],
        snapshot_len: 0,
        deadline: None,
        card: None,
    };
    assert!(heavy.encode().len() > cadrs_org::MAX_REQUEST);
    // And an entry of the log is only so large, whatever operation it is: nobody thought to
    // limit how many prices a rate card lists, and it is refused all the same.
    let endless = Op::SetRates(RateCard {
        currency: "ZAR".into(),
        setup: Money::from_major(250.0),
        minimum: Money::from_major(500.0),
        hourly: vec![],
        sheet_per_m2: (0..5_000)
            .map(|i| {
                cadrs_org::quote::SheetPrice::new(
                    "mild steel",
                    f64::from(i),
                    Money::from_major(1.0),
                )
            })
            .collect(),
        nesting_allowance: 1.25,
    });
    assert!(endless.encode().len() > cadrs_org::MAX_OP);
    for op in [heavy, endless] {
        shop.write(LAPTOP, op);
        let (org, conflicts) = shop.state(&shop.laptop.log);
        assert_eq!(org.requests.len(), 1);
        assert!(org.rates.is_none());
        assert_eq!(
            cadrs_org::refusal::Refusal::of(conflicts.last().unwrap()),
            Some(cadrs_org::refusal::Refusal::TooLarge)
        );
    }

    // A conversation about a request is a conversation's worth, each side its own share; a
    // client's is small until the request is an order.
    let (org, _) = shop.state(&shop.laptop.log);
    let request = *org.requests.keys().next().unwrap();
    let line = "x".repeat(1_000);
    let says = |from_client| Op::Note {
        request,
        from_client,
        text: line.clone(),
    };
    for _ in 0..200 {
        shop.write(LAPTOP, says(true));
        shop.write(LAPTOP, says(false));
    }
    let (org, _) = shop.state(&shop.laptop.log);
    let said = |from_client| -> (usize, usize) {
        let lines = org.requests[&request]
            .chat
            .iter()
            .filter(move |c| c.0 == from_client);
        (lines.clone().count(), lines.map(|c| c.1.len()).sum())
    };
    assert_eq!(said(true), (8, 8_000), "questions before an order");
    assert_eq!(said(false), (65, 65_000), "the shop's share is its own");
    assert!(said(false).1 <= cadrs_org::MAX_NOTE_BYTES);
}

/// The shop clears its list and strangers fill it again, over and over the same day: what
/// waits has its ceiling each time, and what a day adds has one too, however often the list is
/// cleared. The next day takes its share again, and no more.
#[test]
fn a_day_of_floods_adds_a_days_share() {
    let mut shop = manufacturer();
    let strangers: Vec<Account> = (0..80).map(|_| Account::generate()).collect();
    let cycle = |shop: &mut Shop| {
        // Everything that waits is declined, then they all ask again, several times each.
        let (org, _) = shop.state(&shop.laptop.log);
        for (id, request) in &org.requests {
            if request.stage == Stage::Requested {
                shop.write(
                    LAPTOP,
                    Op::Decline {
                        request: *id,
                        reason: String::new(),
                    },
                );
            }
        }
        for round in 0..4 {
            for stranger in &strangers {
                shop.write(LAPTOP, asks(stranger, 1_000 + round));
            }
        }
    };
    for _ in 0..4 {
        cycle(&mut shop);
        let (org, _) = shop.state(&shop.laptop.log);
        let waiting = org
            .requests
            .values()
            .filter(|r| r.stage == Stage::Requested)
            .count();
        assert!(waiting <= cadrs_org::MAX_WAITING);
    }
    let (org, conflicts) = shop.state(&shop.laptop.log);
    let first_day = org.requests.len();
    assert_eq!(
        first_day,
        cadrs_org::MAX_REQUESTS_A_DAY,
        "a day's share, and no more"
    );
    assert!(
        conflicts.iter().any(|c| cadrs_org::refusal::Refusal::of(c)
            == Some(cadrs_org::refusal::Refusal::TooManyToday))
    );
    for stranger in &strangers {
        let theirs = org
            .requests
            .values()
            .filter(|r| r.client == stranger.id())
            .count();
        assert!(theirs <= cadrs_org::MAX_REQUESTS_A_DAY_PER_CLIENT);
    }
    // Each request is at most a request's size as written (`MAX_REQUEST`), so that is what a
    // day of this can add to the log: its share times that, whatever was sent.
    shop.wall += 24 * 60 * 60 * 1000;
    for _ in 0..4 {
        cycle(&mut shop);
    }
    let (org, _) = shop.state(&shop.laptop.log);
    assert!(
        org.requests.len() > first_day,
        "a new day takes requests again"
    );
    assert!(org.requests.len() <= 2 * cadrs_org::MAX_REQUESTS_A_DAY);
}

/// Strangers use up everything strangers share (what may wait, and what a day takes), and the
/// people the organization knows still get through: someone it keeps among its clients, and
/// someone it quoted before. Blocking a flooder gives back what that account used.
#[test]
fn someone_the_shop_knows_is_not_shut_out_by_strangers() {
    let mut shop = manufacturer();
    let regular = Account::generate();
    let kept = Account::generate();
    // The shop quoted the regular before, and keeps the other among its clients.
    let Op::Request { id: earlier, .. } = asks(&regular, 0) else {
        unreachable!()
    };
    shop.write(
        LAPTOP,
        Op::Request {
            id: earlier,
            client: regular.id(),
            parts: vec![(bracket(), 1)],
            notes: String::new(),
            snapshot: [0; 32],
            snapshot_len: 0,
            deadline: None,
            card: None,
        },
    );
    shop.write(
        LAPTOP,
        Op::Quote {
            request: earlier,
            quote: Quote {
                id: 1,
                total: Money::from_major(100.0),
                currency: "ZAR".into(),
                lines: vec![],
                valid_until: u64::MAX,
                note: String::new(),
            },
        },
    );
    shop.write(
        LAPTOP,
        Op::SetClient {
            id: new_id(),
            client: Some(cadrs_org::Client {
                name: "Walk-in Workshop".into(),
                account: Some(kept.id()),
                ..Default::default()
            }),
        },
    );
    let (org, _) = shop.state(&shop.laptop.log);
    assert!(org.knows(&regular.id()) && org.knows(&kept.id()));
    assert!(
        !org.knows(&Account::generate().id()),
        "asking makes nobody known"
    );

    // Strangers take the whole day's share, the shop declining as they come.
    let strangers: Vec<Account> = (0..30).map(|_| Account::generate()).collect();
    let flooder = &strangers[0];
    for round in 0..4 {
        for stranger in &strangers {
            for n in 0..4 {
                shop.write(LAPTOP, asks(stranger, 1_000 + round * 4 + n));
            }
        }
        let (org, _) = shop.state(&shop.laptop.log);
        for (id, request) in &org.requests {
            if request.stage == Stage::Requested {
                shop.write(
                    LAPTOP,
                    Op::Decline {
                        request: *id,
                        reason: String::new(),
                    },
                );
            }
        }
    }
    let taken = |shop: &Shop| shop.state(&shop.laptop.log).0.requests.len();
    assert_eq!(taken(&shop), 1 + cadrs_org::MAX_REQUESTS_A_DAY);
    // A new stranger is refused for the day...
    let newcomer = Account::generate();
    shop.write(LAPTOP, asks(&newcomer, 0));
    assert_eq!(taken(&shop), 1 + cadrs_org::MAX_REQUESTS_A_DAY);
    let (_, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(
        cadrs_org::refusal::Refusal::of(conflicts.last().unwrap()),
        Some(cadrs_org::refusal::Refusal::TooManyToday)
    );
    // ...and the people the shop knows are not.
    shop.write(LAPTOP, asks(&regular, 0));
    shop.write(LAPTOP, asks(&kept, 0));
    assert_eq!(taken(&shop), 3 + cadrs_org::MAX_REQUESTS_A_DAY);
    // Blocking a flooder gives its share back to the strangers: the newcomer gets in.
    shop.write(
        LAPTOP,
        Op::SetBlocked {
            account: flooder.id(),
            blocked: true,
        },
    );
    shop.write(LAPTOP, asks(&newcomer, 0));
    let (org, _) = shop.state(&shop.laptop.log);
    assert!(org.requests.values().any(|r| r.client == newcomer.id()));
}

/// What taking a request costs doesn't grow with how many the organization has had: a shop
/// with eight thousand behind it takes the next as quickly as one with a thousand. (Every
/// operation replays the log, so a check that read the whole history would make the whole
/// organization slower with every request it ever took.)
#[test]
fn taking_a_request_costs_the_same_however_long_the_history() {
    use cadrs_social::log::Reducer;
    // The time to apply 150 more requests to an organization that has had `history` of them:
    // five minutes apart (within a day's share), each declined as it came.
    let cost = |history: usize| {
        let mut shop = manufacturer();
        let clients: Vec<Account> = (0..40).map(|_| Account::generate()).collect();
        for n in 0..history {
            shop.wall += 5 * 60 * 1000;
            let asked = asks(&clients[n % clients.len()], 0);
            let Op::Request { id, .. } = &asked else {
                unreachable!()
            };
            let request = *id;
            shop.write(LAPTOP, asked);
            shop.write(
                LAPTOP,
                Op::Decline {
                    request,
                    reason: String::new(),
                },
            );
        }
        let (mut org, conflicts) = shop.state(&shop.laptop.log);
        assert!(conflicts.is_empty(), "{:?}", conflicts.first());
        assert_eq!(org.requests.len(), history);
        // The entries first, so that only applying them is timed.
        let more: Vec<_> = (0..150)
            .map(|_| {
                shop.wall += 5 * 60 * 1000;
                let wall = shop.wall;
                // Each from someone new: they wait together, within what may wait.
                let op = asks(&Account::generate(), 0);
                let key = shop.laptop.key.clone();
                shop.laptop.log.append(&key, Kind::Data(op.encode()), wall)
            })
            .collect();
        let start = std::time::Instant::now();
        for entry in &more {
            let outcome = org.apply(entry.id(), entry, entry.data().unwrap());
            assert_eq!(outcome, Outcome::Applied);
        }
        start.elapsed()
    };
    // The best of three each, so that a busy machine doesn't decide it.
    let best = |history| (0..3).map(|_| cost(history)).min().unwrap();
    let (short, long) = (best(1_000), best(8_000));
    assert!(
        long < 3 * short + std::time::Duration::from_millis(3),
        "150 requests took {short:?} after 1000 and {long:?} after 8000"
    );
}

/// A declined request's document is let go, a day later: an acceptance written elsewhere at
/// the same moment may still come before the decline, and then it is wanted after all.
#[test]
fn a_declined_requests_document_is_kept_a_day() {
    let dir = std::env::temp_dir().join(format!("cadrs-org-sweep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut shop = manufacturer();
    let document = cadrs_org::blob::put(&dir, b"the client's packed document").unwrap();
    let stray = cadrs_org::blob::put(&dir, b"a logo that was replaced").unwrap();
    shop.write(
        LAPTOP,
        Op::Request {
            id: 5,
            client: Account::generate().id(),
            parts: vec![(bracket(), 1)],
            notes: String::new(),
            snapshot: document.hash,
            snapshot_len: document.len,
            deadline: None,
            card: None,
        },
    );
    let (org, _) = shop.state(&shop.laptop.log);
    // What the log doesn't name goes; a waiting request's document stays.
    assert_eq!(cadrs_org::blob::sweep(&dir, &org, shop.wall), 1);
    assert!(!cadrs_org::blob::has(&dir, &stray.hash));
    assert!(cadrs_org::blob::has(&dir, &document.hash));
    shop.write(
        LAPTOP,
        Op::Decline {
            request: 5,
            reason: String::new(),
        },
    );
    let (org, _) = shop.state(&shop.laptop.log);
    let declined_at = org.requests[&5].declined_at.unwrap();
    assert_eq!(cadrs_org::blob::sweep(&dir, &org, declined_at + 1), 0);
    assert!(cadrs_org::blob::has(&dir, &document.hash), "still there");
    let later = declined_at + cadrs_org::blob::KEEP_DECLINED;
    assert_eq!(cadrs_org::blob::sweep(&dir, &org, later), 1);
    assert!(!cadrs_org::blob::has(&dir, &document.hash));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Someone whose requests aren't wanted is blocked: no more are taken from that account until
/// it is let in again. What it sent before stays, to be declined.
#[test]
fn a_blocked_account_asks_in_vain() {
    let mut shop = manufacturer();
    let (pest, client) = (Account::generate(), Account::generate());
    shop.write(LAPTOP, asks(&pest, 0));
    shop.write(
        LAPTOP,
        Op::SetBlocked {
            account: pest.id(),
            blocked: true,
        },
    );
    shop.write(LAPTOP, asks(&pest, 0));
    shop.write(LAPTOP, asks(&client, 0));
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(
        org.requests.len(),
        2,
        "the one from before, and the other client's"
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        cadrs_org::refusal::Refusal::of(&conflicts[0]),
        Some(cadrs_org::refusal::Refusal::Blocked)
    );
    shop.write(
        LAPTOP,
        Op::SetBlocked {
            account: pest.id(),
            blocked: false,
        },
    );
    shop.write(LAPTOP, asks(&pest, 0));
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!((org.requests.len(), conflicts.len()), (3, 1));
    assert!(org.blocked.is_empty());
}

/// A quote is accepted while it stands. A letter sent in time may arrive late (it waited at a
/// mailbox), so the client's word for when is taken for a few days after, and no longer: a
/// client can't accept last year's price by dating the letter back.
#[test]
fn an_old_quote_is_not_accepted_by_dating_the_letter_back() {
    const DAY: u64 = 24 * 60 * 60 * 1000;
    let quoted = |valid_for: u64| {
        let mut shop = manufacturer();
        let client = Account::generate();
        let Op::Request { id, .. } = asks(&client, 0) else {
            unreachable!()
        };
        shop.write(
            LAPTOP,
            Op::Request {
                id,
                client: client.id(),
                parts: vec![(bracket(), 1)],
                notes: String::new(),
                snapshot: [0; 32],
                snapshot_len: 0,
                deadline: None,
                card: None,
            },
        );
        let valid_until = shop.wall + valid_for;
        shop.write(
            LAPTOP,
            Op::Quote {
                request: id,
                quote: Quote {
                    id: 9,
                    total: Money::from_major(100.0),
                    currency: "ZAR".into(),
                    lines: vec![],
                    valid_until,
                    note: String::new(),
                },
            },
        );
        (shop, id, valid_until)
    };
    let stage = |shop: &Shop, id| shop.state(&shop.laptop.log).0.requests[&id].stage;
    let accept = |request, at| Op::Accept {
        request,
        quote: 9,
        at,
    };

    // Sent in time, arrived a day late: an order.
    let (mut shop, id, valid_until) = quoted(DAY);
    shop.wall = valid_until + DAY;
    shop.write(LAPTOP, accept(id, valid_until - 1));
    assert_eq!(stage(&shop, id), Stage::Accepted);
    // Sent late: not one, whenever it arrived.
    let (mut shop, id, valid_until) = quoted(DAY);
    shop.wall = valid_until + DAY;
    shop.write(LAPTOP, accept(id, valid_until + 1));
    assert_eq!(stage(&shop, id), Stage::Quoted);
    // "Sent in time", arriving a month after the quote ran out: not believed.
    let (mut shop, id, valid_until) = quoted(DAY);
    shop.wall = valid_until + 30 * DAY;
    shop.write(LAPTOP, accept(id, valid_until - 1));
    assert_eq!(stage(&shop, id), Stage::Quoted);
    let (_, conflicts) = shop.state(&shop.laptop.log);
    assert!(conflicts.last().unwrap().contains("had run out"));
}

/// Two admins change different things of the profile at the same time, each on their own
/// device: both changes stand, where setting the whole profile would have undone one.
#[test]
fn two_people_editing_the_profile_both_keep_their_change() {
    let mut shop = manufacturer();
    let wall = shop.wall;
    shop.write(
        LAPTOP,
        Op::SetMember {
            account: shop.worker.id(),
            role: Some(Role::Admin),
        },
    );
    let worker_cert = shop.worker.certify(shop.worker_pc.key.public(), "pc", wall);
    shop.write(LAPTOP, Op::AddDevice(worker_cert));
    shop.sync_all();
    let before = shop.state(&shop.laptop.log).0.profile;

    // The owner writes a tagline while the worker, not having heard of it, sets the e-mail.
    let mut with_tagline = before.clone();
    with_tagline.tagline = "Waterjet cutting in steel".into();
    let mut with_email = before.clone();
    with_email.email = "sales@shop.example".into();
    with_email.website = Some("https://shop.example".into());
    let edit = ProfileEdit::between(&before, &with_tagline);
    assert_eq!(
        edit,
        ProfileEdit {
            tagline: Some("Waterjet cutting in steel".into()),
            ..Default::default()
        }
    );
    assert!(ProfileEdit::between(&before, &before).is_empty());
    shop.write(LAPTOP, Op::EditProfile(edit));
    shop.write(
        WORKER,
        Op::EditProfile(ProfileEdit::between(&before, &with_email)),
    );
    shop.sync_all();
    for log in [&shop.laptop.log, &shop.worker_pc.log] {
        let (org, conflicts) = shop.state(log);
        assert!(conflicts.is_empty(), "{conflicts:?}");
        assert_eq!(org.profile.tagline, "Waterjet cutting in steel");
        assert_eq!(org.profile.email, "sales@shop.example");
        assert_eq!(org.profile.website.as_deref(), Some("https://shop.example"));
        assert!(org.profile.offers.manufacturing, "what it offered stands");
    }
    // An edit is held to what a profile may be, and an empty website is none.
    shop.write(
        LAPTOP,
        Op::EditProfile(ProfileEdit {
            name: Some("C".into()),
            ..Default::default()
        }),
    );
    shop.write(
        LAPTOP,
        Op::EditProfile(ProfileEdit {
            website: Some(String::new()),
            ..Default::default()
        }),
    );
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        (org.profile.name.as_str(), org.profile.website),
        ("Cape Waterjet", None)
    );
}

/// A shop founded by its owner's laptop, synced to the others.
fn founded() -> Shop {
    let mut shop = Shop::new();
    let wall = shop.wall;
    let founder = shop.owner.certify(shop.laptop.key.public(), "laptop", wall);
    shop.write(
        LAPTOP,
        Op::Found {
            name: "Cape Waterjet".into(),
            org: shop.org.id(),
            founder,
        },
    );
    shop
}

fn workshop() -> Location {
    Location {
        name: "Workshop".into(),
        production: true,
        address: Address {
            city: "Stellenbosch".into(),
            country: "ZA".into(),
            ..Default::default()
        },
        point: Some((-33.9357, 18.8611)),
        public: true,
        public_accuracy_m: 2_000.0,
        ..Default::default()
    }
}

/// An organization has as many locations as it works from; machines stand at one, which then
/// can't be removed from under them; and what a listing still lacks is told step by step.
#[test]
fn locations_machines_and_what_a_listing_needs() {
    let mut shop = founded();
    let state = |shop: &Shop| shop.state(&shop.laptop.log);
    let profile = |listed| Profile {
        name: "Cape Waterjet".into(),
        tagline: "Waterjet cutting".into(),
        offers: Offers {
            manufacturing: true,
            designs: false,
        },
        email: "sales@capewaterjet.example".into(),
        logo: Some(Logo {
            full: BlobRef::of(b"512"),
            thumb: BlobRef::of(b"128"),
        }),
        listed,
        ..Default::default()
    };
    // Founded with a name alone: everything else is missing, and it isn't listed.
    assert_eq!(
        missing(&state(&shop).0),
        [
            Missing::Logo,
            Missing::Tagline,
            Missing::Offers,
            Missing::Email
        ]
    );
    shop.write(LAPTOP, Op::SetProfile(profile(true)));
    // It manufactures: it needs somewhere it makes things, and a machine.
    let (org, _) = state(&shop);
    assert_eq!(
        missing(&org),
        [Missing::ProductionLocation, Missing::Machine]
    );
    assert!(org.profile.listed && !org.listed(), "asked, but not yet");

    let (site, warehouse) = (new_id(), new_id());
    shop.write(
        LAPTOP,
        Op::SetLocation {
            id: site,
            location: Some(workshop()),
        },
    );
    shop.write(
        LAPTOP,
        Op::SetLocation {
            id: warehouse,
            location: Some(Location {
                name: "Paarl warehouse".into(),
                pickup: true,
                ..Default::default()
            }),
        },
    );
    let machine = new_id();
    shop.write(
        LAPTOP,
        Op::SetMachine {
            id: machine,
            machine: Some(MachineCaps {
                location: Some(site),
                ..waterjet()
            }),
        },
    );
    let (org, conflicts) = state(&shop);
    assert!(conflicts.is_empty(), "{conflicts:?}");
    assert_eq!(org.locations.len(), 2);
    assert_eq!(missing(&org), []);
    assert!(org.listed());
    assert_eq!(
        org.location(org.machines[&machine].location).unwrap().name,
        "Workshop"
    );
    // The logo's two pictures are blobs the members' devices fetch from one another.
    assert_eq!(org.blobs().len(), 2);
    assert_eq!(org.public_blobs().len(), 2);

    // What every peer refuses: a machine somewhere that isn't, a location that makes no
    // sense, removing a location from under its machines.
    shop.write(
        LAPTOP,
        Op::SetMachine {
            id: new_id(),
            machine: Some(MachineCaps {
                location: Some(new_id()),
                ..waterjet()
            }),
        },
    );
    shop.write(
        LAPTOP,
        Op::SetLocation {
            id: new_id(),
            location: Some(Location {
                point: Some((120.0, 0.0)),
                ..workshop()
            }),
        },
    );
    shop.write(
        LAPTOP,
        Op::SetLocation {
            id: site,
            location: None,
        },
    );
    let (org, conflicts) = state(&shop);
    assert_eq!(conflicts.len(), 3, "{conflicts:?}");
    assert!(conflicts[0].contains("no such location"));
    assert!(conflicts[1].contains("latitude"));
    assert!(conflicts[2].contains("machines stand"));
    assert_eq!(org.locations.len(), 2);

    // The machine moves; then the workshop can go. With nowhere it makes things, the shop
    // drops out of the marketplace by itself.
    shop.write(
        LAPTOP,
        Op::SetMachine {
            id: machine,
            machine: Some(MachineCaps {
                location: Some(warehouse),
                ..waterjet()
            }),
        },
    );
    shop.write(
        LAPTOP,
        Op::SetLocation {
            id: site,
            location: None,
        },
    );
    let (org, conflicts) = state(&shop);
    assert_eq!(conflicts.len(), 3);
    assert_eq!(org.locations.len(), 1);
    assert_eq!(missing(&org), [Missing::ProductionLocation]);
    assert!(!org.listed());
    assert!(org.public_blobs().is_empty());

    // One that only sells designs needs nowhere at all.
    shop.write(
        LAPTOP,
        Op::SetProfile(Profile {
            offers: Offers {
                manufacturing: false,
                designs: true,
            },
            ..profile(true)
        }),
    );
    assert!(state(&shop).0.listed());
    // Every device agrees.
    shop.sync_all();
    assert!(shop.state(&shop.worker_pc.log).0.listed());
}

/// A quality issue is opened by anyone in the organization about something of the
/// organization's, and is closed only with what was decided and why.
#[test]
fn an_issue_is_closed_with_a_decision_and_its_reason() {
    let mut shop = founded();
    let wall = shop.wall;
    shop.write(
        LAPTOP,
        Op::SetMember {
            account: shop.worker.id(),
            role: Some(Role::Member),
        },
    );
    let worker_cert = shop.worker.certify(shop.worker_pc.key.public(), "pc", wall);
    shop.write(LAPTOP, Op::AddDevice(worker_cert));
    let machine = new_id();
    shop.write(
        LAPTOP,
        Op::SetMachine {
            id: machine,
            machine: Some(waterjet()),
        },
    );
    shop.sync_all();

    // The worker, at the machine, opens one; the owner looks after it.
    let issue = new_id();
    let owner = shop.owner.id();
    shop.write(
        WORKER,
        Op::OpenIssue {
            id: issue,
            title: "Taper on thick cuts".into(),
            details: "40 mm plate comes out 0.6 mm wider at the bottom.".into(),
            about: Some(IssueAbout::Machine(machine)),
            owner: Some(owner),
        },
    );
    // About something that isn't here, or owned by a stranger: refused.
    shop.write(
        WORKER,
        Op::OpenIssue {
            id: new_id(),
            title: "Ghost".into(),
            details: String::new(),
            about: Some(IssueAbout::Machine(new_id())),
            owner: None,
        },
    );
    shop.write(
        WORKER,
        Op::OpenIssue {
            id: new_id(),
            title: "Whose".into(),
            details: String::new(),
            about: None,
            owner: Some(Account::generate().id()),
        },
    );
    let update = |status, disposition, rationale: &str| Op::UpdateIssue {
        id: issue,
        status,
        owner: Some(owner),
        disposition,
        rationale: rationale.into(),
    };
    shop.write(WORKER, update(IssueStatus::InProgress, None, ""));
    // Closing without a decision, or without saying why: refused.
    shop.write(WORKER, update(IssueStatus::Closed, None, "fixed"));
    shop.write(
        WORKER,
        update(IssueStatus::Closed, Some(Disposition::Repair), "  "),
    );
    shop.sync_all();
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(conflicts.len(), 4, "{conflicts:?}");
    assert!(conflicts[0].contains("isn't in this organization"));
    assert!(conflicts[1].contains("owner is a member"));
    assert!(conflicts[2].contains("what was decided, and why"));
    assert_eq!(org.issues.len(), 1);
    let open = &org.issues[&issue];
    assert_eq!((open.number, open.status), (1, IssueStatus::InProgress));
    assert_eq!(open.opened_by, shop.worker.id());
    assert_eq!(open.closed, None);

    shop.write(
        LAPTOP,
        update(
            IssueStatus::Closed,
            Some(Disposition::Repair),
            "Worn mixing tube; replaced, and taper is back under 0.1 mm on a test cut.",
        ),
    );
    let (org, _) = shop.state(&shop.laptop.log);
    let closed = &org.issues[&issue];
    assert_eq!(closed.status, IssueStatus::Closed);
    assert_eq!(closed.disposition, Some(Disposition::Repair));
    assert_eq!(closed.closed, Some(shop.wall));
    // The next one is number 2; reopening the first clears when it was closed.
    shop.write(
        LAPTOP,
        Op::OpenIssue {
            id: new_id(),
            title: "Second".into(),
            details: String::new(),
            about: None,
            owner: None,
        },
    );
    shop.write(LAPTOP, update(IssueStatus::Open, None, ""));
    let (org, _) = shop.state(&shop.laptop.log);
    assert_eq!(org.issues.values().map(|i| i.number).max(), Some(2));
    assert_eq!(org.issues[&issue].closed, None);

    // The machine it is about is sold and the worker who opened it leaves: the issue is still
    // there, says what it said, and can be closed.
    let before = shop.state(&shop.laptop.log).1.len();
    shop.write(
        LAPTOP,
        Op::SetMachine {
            id: machine,
            machine: None,
        },
    );
    shop.write(
        LAPTOP,
        update(
            IssueStatus::Closed,
            Some(Disposition::Scrap),
            "The machine was sold.",
        ),
    );
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(conflicts.len(), before, "{conflicts:?}");
    assert!(!org.machines.contains_key(&machine));
    assert_eq!(org.issues[&issue].status, IssueStatus::Closed);
    assert_eq!(org.issues[&issue].about, Some(IssueAbout::Machine(machine)));
}

/// An operation a newer cadrs wrote is counted and left alone; everything else in the document
/// still applies, on the device that knows it and the one that doesn't.
#[test]
fn a_newer_cadrs_operation_is_left_alone() {
    #[derive(serde::Serialize)]
    enum FutureOp {
        SetCertification { name: String, until: u64 },
    }
    let mut shop = founded();
    shop.wall += 1_000;
    let wall = shop.wall;
    let future = cadrs_social::codec::encode(&FutureOp::SetCertification {
        name: "ISO 9001:2015".into(),
        until: 0,
    });
    let peer = &mut shop.laptop;
    peer.log.append(&peer.key, Kind::Data(future), wall);
    peer.log.append(
        &peer.key,
        Kind::Data(b"not an operation at all".to_vec()),
        wall + 1,
    );
    shop.wall += 2;
    shop.write(
        LAPTOP,
        Op::SetLocation {
            id: new_id(),
            location: Some(workshop()),
        },
    );
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(org.newer_ops, 1);
    assert_eq!(org.locations.len(), 1, "what came after still applies");
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts[0].contains("newer cadrs"), "{conflicts:?}");
    assert_eq!(conflicts[1], "not an operation");
}

/// The limits every peer checks, so a document can't be filled with something endless.
#[test]
fn names_and_texts_have_limits() {
    let mut shop = manufacturer();
    let request = new_id();
    shop.write(
        LAPTOP,
        Op::Request {
            id: request,
            client: Account::generate().id(),
            parts: vec![(bracket(), 1)],
            notes: String::new(),
            snapshot: [0; 32],
            snapshot_len: 0,
            deadline: None,
            card: None,
        },
    );
    let long = "x".repeat(cadrs_org::MAX_TEXT + 1);
    let ops = [
        Op::SetProfile(Profile {
            name: "C".into(),
            ..Default::default()
        }),
        Op::SetProfile(Profile {
            name: "Cape Waterjet".into(),
            email: "not an address".into(),
            ..Default::default()
        }),
        Op::SetMachine {
            id: new_id(),
            machine: Some(MachineCaps {
                name: "  ".into(),
                ..waterjet()
            }),
        },
        Op::Note {
            request,
            from_client: false,
            text: long.clone(),
        },
        Op::Say {
            channel: "general".into(),
            text: long.clone(),
        },
        Op::Decline {
            request,
            reason: long,
        },
    ];
    let n = ops.len();
    for op in ops {
        shop.write(LAPTOP, op);
    }
    let (org, conflicts) = shop.state(&shop.laptop.log);
    assert_eq!(conflicts.len(), n, "{conflicts:?}");
    assert_eq!(org.profile.name, "Cape Waterjet");
    assert!(org.machines.is_empty() && org.channels.is_empty());
    // A request with no document sent names no blob.
    assert_eq!(org.requests[&request].snapshot_blob(), None);
    assert_eq!(org.requests[&request].stage, Stage::Requested);
}
