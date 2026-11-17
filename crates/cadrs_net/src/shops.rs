//! The shop directory in the public room: shops that take requests from anyone say so, signed
//! (`cadrs_org::directory::ShopListing`), and peers keep what they hear.
//!
//! Listings are a message of their own in the public documents' room ([`crate::public`]), so the
//! room's catalogs (which peers and seeds already exchange) stay as they are, and an older peer
//! that doesn't know the message ignores it. A shop says its listing again every
//! [`SAY_EVERY`] seconds, for peers that joined since; the app keeps the directory on disk.
//! The bytes are the app's; this only carries them.

use bevy::prelude::*;
use bevy::time::Real;
use bevy_iroh::message::MessageAppExt;
use bevy_iroh::prelude::{NetSender, PeerJoined, Received};
use serde::{Deserialize, Serialize};

use crate::public::{PublicRoom, PublicSystems};

/// How often a shop says its listing again, in seconds.
pub const SAY_EVERY: f64 = 600.0;
/// The soonest it says it again for someone who just joined the room, in seconds.
const FOR_NEWCOMERS_EVERY: f64 = 5.0;

/// A shop's signed listing, said in the room.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopSaid(pub Vec<u8>);

/// What this node says, and what it heard.
#[derive(Resource, Default)]
pub struct Shops {
    /// This node's own shops' listings (set by the app; said when they change and now and then).
    pub mine: Vec<Vec<u8>>,
    /// Listings heard, for the app to take.
    pub heard: Vec<Vec<u8>>,
    said: Vec<Vec<u8>>,
    said_at: f64,
    /// Someone joined the room since it last said them.
    newcomer: bool,
}

impl Shops {
    pub fn take_heard(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.heard)
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<Shops>()
        .add_net_message::<ShopSaid>()
        .add_systems(Update, (hear, say).chain().after(PublicSystems));
}

fn hear(
    mut said: MessageReader<Received<ShopSaid>>,
    room: Res<PublicRoom>,
    mut shops: ResMut<Shops>,
) {
    let public = room.entity();
    for m in said.read().filter(|m| Some(m.room) == public) {
        shops.heard.push(m.msg.0.clone());
    }
}

fn say(
    net: NetSender,
    room: Res<PublicRoom>,
    mut shops: ResMut<Shops>,
    mut joined: MessageReader<PeerJoined>,
    time: Res<Time<Real>>,
) {
    let newcomer = joined.read().count() > 0;
    let Some(public) = room.joined_entity() else {
        // Out of the room: say them all again on joining.
        shops.said.clear();
        return;
    };
    let now = time.elapsed_secs_f64();
    let for_newcomer = (newcomer || shops.newcomer) && !shops.mine.is_empty();
    if for_newcomer && now - shops.said_at < FOR_NEWCOMERS_EVERY {
        // Soon: remember to.
        shops.newcomer = true;
        return;
    }
    if !for_newcomer && shops.mine == shops.said && now - shops.said_at < SAY_EVERY {
        return;
    }
    shops.newcomer = false;
    for listing in &shops.mine {
        net.broadcast(public, &ShopSaid(listing.clone()));
    }
    shops.said = shops.mine.clone();
    shops.said_at = now;
}
