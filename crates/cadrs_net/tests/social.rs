//! Letters between two peers in one process, over real iroh with no relay: one is taken, one is
//! refused with the app's reason.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy_iroh::{Iroh, Relays};
use cadrs_net::NetPlugin;
use cadrs_net::social::{self, Letters};

const PATIENCE: Duration = Duration::from_secs(30);

fn peer(name: &str) -> App {
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
    app
}

/// The shop's side: takes letters that start with "rfq", refuses the rest.
fn serve(mut letters: ResMut<Letters>, mut got: ResMut<Got>) {
    for letter in letters.drain() {
        if letter.bytes.starts_with(b"rfq") {
            got.0.push(letter.bytes.clone());
            letter.take();
        } else {
            letter.refuse("not a letter");
        }
    }
}

#[derive(Resource, Default)]
struct Got(Vec<Vec<u8>>);

#[test]
fn a_letter_is_taken_and_a_bad_one_refused() {
    let mut shop = peer("Shop");
    shop.init_resource::<Got>().add_systems(Update, serve);
    let mut designer = peer("Designer");
    for _ in 0..3 {
        shop.update();
        designer.update();
    }
    let to = shop
        .world()
        .resource::<Iroh>()
        .addr()
        .expect("the shop is online");
    let big = [b"rfq".as_slice(), &vec![7u8; 3 << 20]].concat();
    let mut good = social::start_send(designer.world().resource::<Iroh>(), to.clone(), big.clone());
    let mut bad = social::start_send(designer.world().resource::<Iroh>(), to, b"junk".to_vec());
    let (mut good_answer, mut bad_answer) = (None, None);
    let start = Instant::now();
    while good_answer.is_none() || bad_answer.is_none() {
        shop.update();
        designer.update();
        good_answer = good_answer.or_else(|| good.try_take());
        bad_answer = bad_answer.or_else(|| bad.try_take());
        assert!(
            start.elapsed() < PATIENCE,
            "gave up waiting for the answers"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(good_answer, Some(Ok(Vec::new())));
    assert_eq!(bad_answer, Some(Err("not a letter".into())));
    assert_eq!(shop.world().resource::<Got>().0, vec![big]);
}

/// The phone app's node (`cadrs_social_net`, no Bevy) and the desktop's (`cadrs_net`) speak
/// the same protocol: a post from the phone reaches the desktop's app and its answer comes back.
#[test]
fn the_phone_node_talks_to_the_desktop() {
    let mut desktop = peer("Desktop");
    desktop.init_resource::<Got>().add_systems(Update, serve);
    for _ in 0..3 {
        desktop.update();
    }
    let to = desktop.world().resource::<Iroh>().addr().unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (phone, _) = rt
        .block_on(cadrs_social_net::Node::spawn([9; 32], false))
        .unwrap();
    let to = cadrs_social_net::iroh::EndpointAddr::from_parts(
        cadrs_social_net::iroh::EndpointId::from_bytes(to.id.as_bytes()).unwrap(),
        to.addrs.iter().cloned().collect::<Vec<_>>(),
    );
    let answer = std::sync::Arc::new(std::sync::Mutex::new(None));
    let a = answer.clone();
    rt.spawn(async move {
        let r = phone.send(to, b"rfq from a phone").await;
        *a.lock().unwrap() = Some(r);
    });
    let start = Instant::now();
    while answer.lock().unwrap().is_none() {
        desktop.update();
        assert!(start.elapsed() < PATIENCE, "the desktop never answered");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(answer.lock().unwrap().clone(), Some(Ok(Vec::new())));
    assert_eq!(
        desktop.world().resource::<Got>().0,
        vec![b"rfq from a phone".to_vec()]
    );
}
