//! The public chat's lines in the public room (`cadrs_social::chat`): a message of their own
//! there, as shop listings are ([`crate::shops`]), so older peers ignore them. Each line is said
//! once; the app keeps what it hears.

use bevy::prelude::*;
use bevy_iroh::message::MessageAppExt;
use bevy_iroh::prelude::{NetSender, Received};
use serde::{Deserialize, Serialize};

use crate::public::{PublicRoom, PublicSystems};

/// A signed line of the public chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSaid(pub Vec<u8>);

/// Lines to say, and lines heard.
#[derive(Resource, Default)]
pub struct PublicChat {
    /// Said once the room is joined, then dropped.
    pub to_say: Vec<Vec<u8>>,
    /// For the app to take.
    pub heard: Vec<Vec<u8>>,
}

impl PublicChat {
    pub fn take_heard(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.heard)
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<PublicChat>()
        .add_net_message::<ChatSaid>()
        .add_systems(Update, (hear, say).chain().after(PublicSystems));
}

fn hear(
    mut said: MessageReader<Received<ChatSaid>>,
    room: Res<PublicRoom>,
    mut chat: ResMut<PublicChat>,
) {
    let public = room.entity();
    for m in said.read().filter(|m| Some(m.room) == public) {
        chat.heard.push(m.msg.0.clone());
    }
}

fn say(net: NetSender, room: Res<PublicRoom>, mut chat: ResMut<PublicChat>) {
    let Some(public) = room.joined_entity() else {
        return;
    };
    for line in std::mem::take(&mut chat.to_say) {
        net.broadcast(public, &ChatSaid(line));
    }
}
