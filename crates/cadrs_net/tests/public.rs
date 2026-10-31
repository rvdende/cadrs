//! Public documents between peers in one process, over real iroh with no relay: an owner
//! publishes, another peer finds it in the public room, fetches it and stars it; a seed keeps
//! it, and a newcomer gets it from the seed once the owner is gone.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy_iroh::{Iroh, Relays};
use cadrs_core::DocumentId;
use cadrs_net::NetPlugin;
use cadrs_net::public::{
    self, Draft, KeepAll, PublicPeers, PublicRoom, Rendezvous, RevisionFetch, Shared,
};

const PATIENCE: Duration = Duration::from_secs(60);

fn peer(name: &str, start_from: &[&App]) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        NetPlugin {
            identity: None,
            display_name: name.into(),
            relays: Relays::Disabled,
            autostart: true,
        },
    ));
    let peers = start_from
        .iter()
        .map(|a| a.world().resource::<Iroh>().addr().unwrap())
        .collect();
    app.insert_resource(PublicPeers(peers));
    // Never the real rendezvous.
    app.insert_resource(Rendezvous(None));
    app
}

/// Runs every app until `done` says so.
fn until(apps: &mut [&mut App], what: &str, mut done: impl FnMut(&mut [&mut App]) -> bool) {
    let start = Instant::now();
    loop {
        for a in apps.iter_mut() {
            a.update();
        }
        if done(apps) {
            eprintln!("{what} in {:?}", start.elapsed());
            return;
        }
        assert!(start.elapsed() < PATIENCE, "gave up waiting: {what}");
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn shared(app: &App) -> Shared {
    app.world().resource::<Shared>().clone()
}

fn fetched(apps: &mut [&mut App], who: usize, mut f: RevisionFetch) -> Result<Vec<u8>, String> {
    let s = shared(apps[who]);
    let mut out = None;
    until(apps, "the revision", |_| {
        out = f.poll(&s);
        out.is_some()
    });
    out.unwrap()
}

#[test]
fn a_published_document_is_found_fetched_starred_and_outlives_its_owner_on_a_seed() {
    let mut owner = peer("Owner", &[]);
    let mut viewer = peer("Viewer", &[&owner]);
    let mut seed = peer("Seed", &[&owner]);
    seed.insert_resource(KeepAll);

    let document = DocumentId::new();
    let revision = b"a packed snapshot, as the app makes one".repeat(1000);
    // A picture as big as a real one: far past what gossip carries, so it goes by its hash.
    let picture: Vec<u8> = (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let listing = public::publish(
        owner.world_mut(),
        Draft {
            document,
            name: "Bracket".into(),
            description: "A bracket".into(),
            picture: picture.clone(),
            forked_from: None,
            previous: 0,
        },
        revision.clone(),
    )
    .unwrap();
    assert_eq!(listing.revision, 1);
    let key = listing.key();

    // The viewer and the seed hear of it (asking the owner for its catalog on joining).
    until(
        &mut [&mut owner, &mut viewer, &mut seed],
        "the listing",
        |a| {
            shared(a[1]).lock().directory.get(&key).is_some()
                && shared(a[2]).lock().directory.get(&key).is_some()
        },
    );
    let heard = shared(&viewer)
        .lock()
        .directory
        .get(&key)
        .unwrap()
        .listing
        .clone();
    assert_eq!(heard, listing);
    until(
        &mut [&mut owner, &mut viewer, &mut seed],
        "the picture",
        |a| shared(a[1]).lock().directory.docs("")[0].picture.as_ref() == Some(&picture),
    );

    // The viewer fetches it from the owner.
    let f = public::fetch(viewer.world_mut(), &key).unwrap();
    let got = fetched(&mut [&mut owner, &mut viewer, &mut seed], 1, f).unwrap();
    assert_eq!(got, revision);

    // A star reaches the owner over the room.
    public::star(viewer.world_mut(), &key, true).unwrap();
    until(&mut [&mut owner, &mut viewer, &mut seed], "the star", |a| {
        shared(a[0]).lock().directory.stars(&key) == 1
            && shared(a[2]).lock().directory.stars(&key) == 1
    });

    // The seed keeps the revision of its own accord.
    until(
        &mut [&mut owner, &mut viewer, &mut seed],
        "the seed's copy",
        |a| shared(a[2]).lock().has_revision(&listing.hash),
    );

    // The owner goes offline; a newcomer starting from the seed still gets it.
    cadrs_net::stop(owner.world_mut());
    let mut newcomer = peer("Newcomer", &[&seed]);
    until(
        &mut [&mut seed, &mut newcomer],
        "the newcomer's listing",
        |a| shared(a[1]).lock().directory.get(&key).is_some(),
    );
    assert_eq!(shared(&newcomer).lock().directory.stars(&key), 1);
    assert_eq!(
        shared(&newcomer).lock().directory.docs("")[0].picture,
        Some(picture)
    );
    let f = public::fetch(newcomer.world_mut(), &key).unwrap();
    let got = fetched(&mut [&mut seed, &mut newcomer], 1, f).unwrap();
    assert_eq!(got, revision);
}

#[test]
fn a_withdrawn_document_leaves_everyone_s_list() {
    let mut owner = peer("Owner", &[]);
    let mut viewer = peer("Viewer", &[&owner]);
    let document = DocumentId::new();
    let draft = Draft {
        document,
        name: "Bracket".into(),
        description: String::new(),
        picture: Vec::new(),
        forked_from: None,
        previous: 0,
    };
    let listing = public::publish(owner.world_mut(), draft.clone(), b"one".to_vec()).unwrap();
    let key = listing.key();
    until(&mut [&mut owner, &mut viewer], "the listing", |a| {
        shared(a[1]).lock().directory.get(&key).is_some()
    });
    // Published again: the next revision replaces it.
    let again = public::publish(
        owner.world_mut(),
        Draft {
            previous: 1,
            ..draft
        },
        b"two".to_vec(),
    )
    .unwrap();
    assert_eq!(again.revision, 2);
    until(&mut [&mut owner, &mut viewer], "revision 2", |a| {
        shared(a[1])
            .lock()
            .directory
            .get(&key)
            .is_some_and(|h| h.listing.revision == 2)
    });
    public::withdraw(owner.world_mut(), document).unwrap();
    until(&mut [&mut owner, &mut viewer], "the withdrawal", |a| {
        shared(a[1]).lock().directory.get(&key).is_none()
    });
}

#[test]
fn peers_the_room_does_not_join_still_reach_each_other_through_a_seed() {
    // Both alone in the room: neither starts from the other.
    let mut owner = peer("Owner", &[]);
    // The owner is a seed too.
    owner.insert_resource(KeepAll);
    let mut viewer = peer("Viewer", &[]);
    until(&mut [&mut owner, &mut viewer], "both in the room", |a| {
        a.iter()
            .all(|a| public::room_state(a.world()) == public::RoomState::Joined(0))
    });
    let picture = b"a picture".to_vec();
    let listing = public::publish(
        owner.world_mut(),
        Draft {
            document: DocumentId::new(),
            name: "Bracket".into(),
            description: String::new(),
            picture: picture.clone(),
            forked_from: None,
            previous: 0,
        },
        b"one".to_vec(),
    )
    .unwrap();
    let key = listing.key();
    // The viewer is given the owner as a seed: asked at once, picture and all.
    let addr = owner.world().resource::<Iroh>().addr().unwrap();
    viewer.insert_resource(PublicPeers(vec![addr]));
    until(
        &mut [&mut owner, &mut viewer],
        "the listing from the seed",
        |a| shared(a[1]).lock().directory.get(&key).is_some(),
    );
    let doc = shared(&viewer).lock().directory.docs("").remove(0);
    assert_eq!(doc.picture, Some(picture));
    // Its star goes straight to its seed, gossip or not.
    public::star(viewer.world_mut(), &key, true).unwrap();
    until(
        &mut [&mut owner, &mut viewer],
        "the star told to the seed",
        |a| shared(a[0]).lock().directory.stars(&key) == 1,
    );
    // What the viewer publishes is told to its seed, which keeps the revision too.
    let theirs = public::publish(
        viewer.world_mut(),
        Draft {
            document: DocumentId::new(),
            name: "Clip".into(),
            description: String::new(),
            picture: Vec::new(),
            forked_from: None,
            previous: 0,
        },
        b"two".to_vec(),
    )
    .unwrap();
    until(
        &mut [&mut owner, &mut viewer],
        "the seed's copy of a told one",
        |a| shared(a[0]).lock().has_revision(&theirs.hash),
    );
}

/// A stand-in for the rendezvous (`tools/peers`): answers each check-in whose signature holds
/// with `others`, and counts them.
fn rendezvous(others: Vec<String>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let good = std::sync::Arc::new(AtomicUsize::new(0));
    let count = good.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let check_in: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let id = check_in["id"].as_str().unwrap().to_string();
            let time = check_in["time"].as_i64().unwrap();
            let sig: [u8; 64] = data_encoding::HEXLOWER
                .decode(check_in["signature"].as_str().unwrap().as_bytes())
                .unwrap()
                .try_into()
                .unwrap();
            let key: bevy_iroh::iroh::PublicKey = id.parse().unwrap();
            let message = format!("cadrs rendezvous 1\0{id}\0{time}");
            let holds = key
                .verify(
                    message.as_bytes(),
                    &bevy_iroh::iroh::Signature::from_bytes(&sig),
                )
                .is_ok();
            if holds {
                count.fetch_add(1, Ordering::SeqCst);
            }
            let answer = serde_json::json!({ "peers": others }).to_string();
            let status = if holds { "200 OK" } else { "400 Bad Request" };
            let mut out = stream;
            let _ = write!(
                out,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (url, good)
}

#[test]
fn a_peer_checks_in_and_starts_from_the_peers_the_rendezvous_names() {
    let other = bevy_iroh::iroh::SecretKey::generate().public();
    let (url, good) = rendezvous(vec![other.to_string()]);
    let mut app = peer("Newcomer", &[]);
    app.insert_resource(Rendezvous(Some(url)));
    until(&mut [&mut app], "the check-in's answer", |a| {
        a[0].world().resource::<PublicRoom>().found() == [other]
    });
    assert_eq!(good.load(std::sync::atomic::Ordering::SeqCst), 1);
    // It joins the room after (through the peer named, which isn't there).
    until(&mut [&mut app], "the room", |a| {
        public::room_state(a[0].world()) == public::RoomState::Joined(0)
    });
}
