//! A water-cutting shop's life in its organization document, over simulated peers: founding it,
//! adding a box and a member, setting up machines and prices, a request for a quote that
//! becomes an order, two members racing for the last sheet, delivery with a co-signed receipt,
//! and the client's review.

use cadrs_org::capability::{MachineCaps, MaterialLimit, PartSummary, Process};
use cadrs_org::quote::{Money, RateCard, estimate};
use cadrs_org::{Material, Op, Org, Profile, Reservation, Review, Role, Stage, new_id, order_ref};
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
            open_for_quotes: true,
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
        hourly: vec![("Waterjet".into(), Money::from_major(1200.0))],
        sheet_per_m2: vec![("mild steel".into(), 10.0, Money::from_major(1000.0))],
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
                deadline: None,
                card: None,
            },
        );
        let (org, _) = shop.state(&shop.shop_box.log);
        let estimate = estimate(
            &org.machines.values().cloned().collect::<Vec<_>>(),
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
                    total: estimate.total,
                    currency: estimate.currency,
                    lines: estimate.lines,
                    valid_until: shop.wall + 7 * 86_400_000,
                    note: String::new(),
                },
            },
        );
        shop.write(BOX, Op::Accept { request });
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
}
