//! A bank on this node ([`cadrs_credits::bank::Bank`]): it answers what devices ask about
//! their wallets (`Post::Bank`) and tells the devices of whoever was paid as it happens
//! (`Post::Credited`).
//!
//! The seed peer keeps one ([`host`], [`serve`]), with its books in its data dir and the
//! node's own key signing the receipts, so the bank's id is the seed's cadrs ID. The app can
//! keep one too ([`Hosted`] and [`answer`]): a test's, or one for a workshop's own network.

use std::path::Path;

use bevy::prelude::*;
use bevy_iroh::iroh::{EndpointAddr, EndpointId};
use bevy_iroh::{Iroh, IrohTask};
use cadrs_credits::bank::Bank;
use cadrs_social::keys::{PublicKey, SecretKey};
use cadrs_social::post::Post;

/// The bank this node keeps.
#[derive(Resource)]
pub struct Hosted(pub Bank);

/// What the bank is telling devices, on its way. Nobody waits for it: a device that isn't
/// reached sees its wallet the next time it looks.
#[derive(Resource, Default)]
struct Telling(Vec<IrohTask<Result<Vec<u8>, String>>>);

/// Addresses of devices known without discovery (devices on the bank's own network; tests,
/// where there is no relay to find them through).
#[derive(Resource, Default)]
pub struct Addrs(pub std::collections::HashMap<EndpointId, EndpointAddr>);

/// How many tellings may be on their way at once; more are dropped (the devices look again).
const MAX_TELLING: usize = 256;

/// Keeps a bank on this node, its books in `dir` (in memory with none), signing with `key`.
pub fn host(world: &mut World, key: SecretKey, dir: Option<&Path>) -> std::io::Result<()> {
    let bank = match dir {
        Some(dir) => Bank::open(key, dir)?,
        None => Bank::in_memory(key),
    };
    world.insert_resource(Hosted(bank));
    world.init_resource::<Telling>();
    Ok(())
}

/// The bank here answers an ask that arrived from device `from` (none: this app's own).
pub fn answer(world: &mut World, bytes: &[u8], from: Option<PublicKey>) -> Result<Vec<u8>, String> {
    let now = cadrs_social::wall_millis();
    let handled = world
        .get_resource_mut::<Hosted>()
        .ok_or("this cadrs keeps no bank")?
        .0
        .handle(bytes, from, now)?;
    for push in handled.pushes {
        let post = Post::Credited(push.receipt).encode();
        let Some(iroh) = world.get_resource::<Iroh>() else {
            break;
        };
        let tasks: Vec<_> = push
            .to
            .iter()
            .filter_map(|device| EndpointId::from_bytes(&device.0).ok())
            .map(|device| {
                // A known address (a device on this network), else the bare key for
                // discovery to find.
                let addr = world
                    .get_resource::<Addrs>()
                    .and_then(|known| known.0.get(&device).cloned())
                    .unwrap_or_else(|| device.into());
                crate::social::start_send(iroh, addr, post.clone())
            })
            .collect();
        world.init_resource::<Telling>();
        let mut telling = world.resource_mut::<Telling>();
        // Those that are done are forgotten here too, for a bank kept without the plugin.
        telling.0.retain_mut(|task| task.try_take().is_none());
        let room = MAX_TELLING.saturating_sub(telling.0.len());
        telling.0.extend(tasks.into_iter().take(room));
    }
    Ok(handled.answer)
}

/// The seed's: takes what arrives. A bank's asks are answered; anything else isn't a seed's
/// to take (its mailbox answers deposits and collections before they come here).
pub fn serve(world: &mut World) {
    let arrived = match world.get_resource_mut::<crate::social::Letters>() {
        Some(mut letters) => letters.drain(),
        None => return,
    };
    for letter in arrived {
        let from = PublicKey(*letter.from.as_bytes());
        let result = match Post::decode(&letter.bytes) {
            Ok(Post::Bank(ask)) => answer(world, &ask, Some(from)),
            Ok(_) => Err("a seed takes a bank's asks and a mailbox's letters only".into()),
            Err(e) if e.is_newer() => {
                Err("this seed's cadrs is older than yours: it can't take that yet".into())
            }
            Err(_) => Err("not something cadrs sends".into()),
        };
        match result {
            Ok(answer) => letter.take_with(answer),
            Err(why) => letter.refuse(why),
        }
    }
}

/// Forgets the tellings that are done.
fn settle(telling: Option<ResMut<Telling>>) {
    if let Some(mut telling) = telling
        && !telling.0.is_empty()
    {
        telling.0.retain_mut(|task| task.try_take().is_none());
    }
}

/// The seed's bank: [`serve`] and [`settle`], every frame.
pub struct BankPlugin;

impl Plugin for BankPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Telling>()
            .add_systems(Update, (serve, settle).chain());
    }
}
