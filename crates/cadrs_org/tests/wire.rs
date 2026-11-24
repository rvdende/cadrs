//! The written form of an organization's operations: fixed where it is written today, and
//! readable across versions in both directions (`cadrs_social::codec`'s rules).

use cadrs_org::capability::PartSummary;
use cadrs_org::directory::{Offer, ShopListing};
use cadrs_org::letters::Message;
use cadrs_org::{Address, Location, Offers, Op};
use cadrs_social::contact::{Card, Kind};
use cadrs_social::keys::{Account, SecretKey};
use serde::{Deserialize, Serialize};

fn workshop() -> Op {
    Op::SetLocation {
        id: 7,
        location: Some(Location {
            name: "Workshop".into(),
            production: true,
            pickup: false,
            address: Address {
                city: "Stellenbosch".into(),
                country: "ZA".into(),
                ..Default::default()
            },
            point: Some((-33.5, 18.75)),
            phone: String::new(),
            public: true,
            public_accuracy_m: 5000.0,
        }),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What this version writes for an operation. If this fails, the format changed: every
/// organization's log on every device was written the old way, so the change must be one the
/// rules allow (a new field with a default, a new variant) and the old bytes must still read
/// (the next test); only then is the expected text here brought up to date.
#[test]
fn the_written_form_of_an_operation_is_fixed() {
    let written = workshop().encode();
    assert_eq!(hex(&written), WORKSHOP, "the written form changed");
    assert_eq!(Op::decode(&written).unwrap(), workshop());
}

/// MessagePack with names: `{"SetLocation": {"id": <16 bytes>, "location": {"name": …}}}`.
const WORKSHOP: &str = concat!(
    "81ab5365744c6f636174696f6e82a26964c41000000000000000000000000000000007a86c6f6361",
    "74696f6e88a46e616d65a8576f726b73686f70aa70726f64756374696f6ec3a67069636b7570c2a7",
    "6164647265737386a56c696e6531a0a56c696e6532a0a463697479ac5374656c6c656e626f736368",
    "a6726567696f6ea0ab706f7374616c5f636f6465a0a7636f756e747279a25a41a5706f696e7492cb",
    "c040c00000000000cb4032c00000000000a570686f6e65a0a67075626c6963c3b17075626c69635f",
    "61636375726163795f6dcb40b3880000000000",
);

/// The operation as a cadrs wrote it before locations had a phone number, a pickup flag or an
/// address beyond the city.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum OlderOp {
    SetLocation {
        id: u128,
        location: Option<OlderLocation>,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct OlderLocation {
    name: String,
    production: bool,
    address: OlderAddress,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct OlderAddress {
    city: String,
}

/// Fields are added, never renamed or removed: what an older cadrs wrote reads with the new
/// fields at their defaults, and what this one writes reads in an older cadrs, which takes the
/// fields it knows.
#[test]
fn older_and_newer_versions_read_each_other() {
    let older = OlderOp::SetLocation {
        id: 7,
        location: Some(OlderLocation {
            name: "Workshop".into(),
            production: true,
            address: OlderAddress {
                city: "Stellenbosch".into(),
            },
        }),
    };
    let read = Op::decode(&cadrs_social::codec::encode(&older)).expect("an older op reads");
    let Op::SetLocation {
        id: 7,
        location: Some(location),
    } = read
    else {
        panic!("not the location: {read:?}");
    };
    assert_eq!(location.name, "Workshop");
    assert!(location.production && !location.pickup && !location.public);
    assert_eq!(location.address.city, "Stellenbosch");
    assert_eq!(
        (location.address.country.as_str(), location.point),
        ("", None)
    );

    let back: OlderOp =
        cadrs_social::codec::decode(&workshop().encode()).expect("a newer op reads");
    assert_eq!(back, older);

    // An operation an older cadrs has never heard of is told apart from damage.
    #[derive(Serialize)]
    enum NewerOp {
        ScheduleMaintenance { machine: u128 },
    }
    let unknown = cadrs_social::codec::encode(&NewerOp::ScheduleMaintenance { machine: 1 });
    assert!(Op::decode(&unknown).unwrap_err().is_newer());
    assert!(!Op::decode(&[0xc1, 0x00]).unwrap_err().is_newer());
}

/// Keys that are the same every time: what they sign is too.
fn fixed() -> (Account, SecretKey, Account) {
    (
        Account::from_key(SecretKey::from_bytes(&[1; 32])),
        SecretKey::from_bytes(&[2; 32]),
        Account::from_key(SecretKey::from_bytes(&[3; 32])),
    )
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

fn card(
    of: &Account,
    device: &SecretKey,
    kind: Kind,
    name: &str,
) -> cadrs_social::keys::Signed<Card> {
    Card {
        account: of.id(),
        kind,
        name: name.into(),
        devices: vec![device.public()],
        mailboxes: vec![],
        issued: 1_000,
    }
    .sign(of.key())
}

/// The size and BLAKE3 of some bytes, as the expected values below are written.
fn mark(bytes: &[u8]) -> (usize, String) {
    (bytes.len(), hex(blake3::hash(bytes).as_bytes()))
}

/// The other things written to disk or sent to peers, signed with fixed keys so that their
/// bytes are fixed too: the founding of an organization and a request in its log, a letter, a
/// listing in the marketplace, and the log's file. A change here is a change to what every
/// other cadrs has to read: see [`the_written_form_of_an_operation_is_fixed`].
#[test]
fn what_is_written_and_sent_is_fixed() {
    let (owner, device, org) = fixed();
    let cert = owner.certify(device.public(), "laptop", 1_000);
    let found = Op::Found {
        name: "Cape Waterjet".into(),
        org: org.id(),
        founder: cert.clone(),
    };
    let request = Op::Request {
        id: 42,
        client: owner.id(),
        parts: vec![(bracket(), 6)],
        notes: "brushed".into(),
        snapshot: [9; 32],
        snapshot_len: 1234,
        deadline: None,
        card: Some(card(&owner, &device, Kind::Person, "Alex")),
    };
    let letter = cadrs_org::letters::write(
        &device,
        &cert,
        org.id(),
        Message::Request {
            id: 42,
            parts: vec![(bracket(), 6)],
            notes: "brushed".into(),
            deadline: None,
            snapshot: b"a packed document".to_vec(),
            client: card(&owner, &device, Kind::Person, "Alex"),
        },
        2_000,
    );
    let mut listing = ShopListing::withdrawal(
        card(&org, &device, Kind::Organization, "Cape Waterjet"),
        3_000,
    );
    listing.withdrawn = false;
    listing.tagline = "Waterjet cutting in steel, aluminium and glass".into();
    listing.kinds = Offers {
        manufacturing: true,
        designs: false,
    };
    listing.offers = vec![Offer {
        process: cadrs_org::capability::Process::Waterjet,
        envelope: [3000.0, 1500.0, 0.0],
        materials: vec![("mild steel".into(), 100.0)],
    }];
    listing.email = "sales@shop.example".into();
    let said = cadrs_org::directory::sign(org.key(), &listing).unwrap();

    let mut log = cadrs_social::log::Log::new(cadrs_social::log::LogId([5; 32]));
    log.append(
        &device,
        cadrs_social::log::Kind::Data(found.encode()),
        1_000,
    );
    log.append(
        &device,
        cadrs_social::log::Kind::Data(request.encode()),
        2_000,
    );
    let dir = std::env::temp_dir().join(format!("cadrs-org-wire-{}", std::process::id()));
    let path = cadrs_org::file::path(&dir);
    cadrs_org::file::save(&path, &log).unwrap();
    let file = std::fs::read(&path).unwrap();

    let written = [
        ("founding", mark(&found.encode())),
        ("request", mark(&request.encode())),
        ("letter", mark(&letter)),
        ("listing", mark(&said)),
        ("log file", mark(&file)),
    ];
    let got: Vec<(&str, usize, &str)> = written
        .iter()
        .map(|(what, (len, hash))| (*what, *len, hash.as_str()))
        .collect();
    assert_eq!(got, WRITTEN, "a written form changed");

    // And each reads back as what was written.
    assert_eq!(Op::decode(&found.encode()).unwrap(), found);
    assert_eq!(Op::decode(&request.encode()).unwrap(), request);
    let (read, from) = cadrs_org::letters::read(&letter, &[org.id()]).unwrap();
    assert_eq!((from, read.sent), (owner.id(), 2_000));
    let mut heard = cadrs_org::directory::Directory::default();
    assert_eq!(heard.hear(&said, 3_000), Ok(true));
    assert_eq!(heard.get(&org.id()), Some(&listing));
    let (reread, skipped) = cadrs_org::file::load(&path).unwrap();
    assert_eq!((reread.len(), skipped), (2, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// (what, its size in bytes, the BLAKE3 of its bytes).
const WRITTEN: [(&str, usize, &str); 5] = [
    (
        "founding",
        368,
        "7c925555348a2933eed8af696ee0798a7c2a0455151397fcaf1c02d2f56500dd",
    ),
    (
        "request",
        679,
        "d1b561a23787627dbb262fb75f393d0c2ea8eaeedca60ea935ee24d142f87093",
    ),
    (
        "letter",
        1091,
        "b0544c7e8618b9472c453392f8e99f503d171be7acaf1a1d1a093206b9b709d5",
    ),
    (
        // 741 bytes before listings said which devices publish for them (`publishers`, a new
        // field with a default: `a_listing_from_before_publishers_still_reads`).
        "listing",
        753,
        "3d539ebf27f8ddc9b0e644d7edfa4d23fd711f200c9888461e9d26dceddefe3b",
    ),
    (
        "log file",
        1614,
        "277a2336bc7585fa46065a42f71faf0a01667f2ef0f2cec3a5013f35f9a914dd",
    ),
];

/// A listing as it was written before it said which devices publish for it.
#[derive(Serialize)]
struct OlderListing {
    card: cadrs_social::keys::Signed<Card>,
    tagline: String,
    about: String,
    kinds: Offers,
    logo: Option<cadrs_org::BlobRef>,
    logo_full: Option<cadrs_org::BlobRef>,
    places: Vec<cadrs_social::place::Place>,
    offers: Vec<Offer>,
    email: String,
    website: Option<String>,
    rating: Option<(f64, usize)>,
    issued: u64,
    withdrawn: bool,
}

/// The listing gained a field after it was first written: one written before reads, with the
/// field at its default.
#[test]
fn a_listing_from_before_publishers_still_reads() {
    let (_, device, org) = fixed();
    let older = OlderListing {
        card: card(&org, &device, Kind::Organization, "Cape Waterjet"),
        tagline: "Waterjet cutting".into(),
        about: String::new(),
        kinds: Offers {
            manufacturing: true,
            designs: false,
        },
        logo: None,
        logo_full: None,
        places: vec![],
        offers: vec![],
        email: "sales@shop.example".into(),
        website: None,
        rating: None,
        issued: 3_000,
        withdrawn: false,
    };
    let read: ShopListing =
        cadrs_social::codec::decode(&cadrs_social::codec::encode(&older)).expect("it reads");
    assert_eq!(read.tagline, "Waterjet cutting");
    assert!(read.publishers.is_empty() && read.publishing_devices().is_empty());
    assert!(read.valid());
}
