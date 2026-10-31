//! Sharing and collaboration between cadrs peers, with no server: iroh through
//! [`bevy_iroh`] (QUIC dialled by public key, hole punching with relay fallback, gossip rooms,
//! signed typed messages).
//!
//! - [`NetPlugin`] makes this node: its identity is a key kept next to the documents
//!   ([`IDENTITY_FILE`]), so peers recognise it from run to run, and its public key is the
//!   user's **cadrs ID** ([`cadrs_id`]). It goes online only when asked ([`start`], or at
//!   once with [`NetPlugin::autostart`]) and off again with [`stop`] (issue #148).
//! - A document is shared by a [`ShareLink`]; whoever opens one fetches the document from its
//!   owner over [`transfer`], and proposes their edits the same way. The owner's app decides
//!   through [`Requests`], and its numbered edits ([`Sequenced`]) reach everyone over
//!   the document's room ([`session`]).
//! - Public documents ([`public`]): a room every online cadrs is in, signed listings of the
//!   documents their owners published, and a protocol any peer answers with what it heard.
//! - Only bevy_iroh's transport, rooms and typed messages are used; its entity replication of
//!   `Transform` is off and its voice and video features are not built.

use std::path::PathBuf;

pub mod link;
pub mod public;
pub mod session;
pub mod transfer;

use bevy::prelude::*;
use bevy_iroh::iroh::protocol::DynProtocolHandler;
use bevy_iroh::{Identity, Iroh, IrohPlugin, Relays, StartError, StartIroh, StopIroh};

pub use bevy_iroh;
pub use link::ShareLink;
pub use session::{
    AccessChanged, CommentChanged, Doing, Draft, EdaOf, LocksChanged, Picked, Ping, Pointer,
    PointerAt, Presence, Sequenced, SketchPlane, VersionCreated, View,
};

/// A question to another peer on its way: polled from a system until the answer (or why
/// there is none) arrives.
pub type Answer<T> = bevy_iroh::IrohTask<Result<T, String>>;
pub use transfer::{Ask, Invite, Ownership, Request, Requests, Verdict, Welcome};

/// The node's secret key, in the documents folder (32 raw bytes, mode 0600). Each data
/// directory is a peer of its own, so two instances with their own `--data-dir` can share
/// with each other on one machine.
pub const IDENTITY_FILE: &str = "identity.key";

/// This node. Added by the app itself, not by scenarios, headless runs or tests. Nothing
/// touches the network until the node starts: the endpoint binds and reaches the relays as
/// soon as it does.
pub struct NetPlugin {
    /// Where the identity is kept; `None` is a fresh key each run.
    pub identity: Option<PathBuf>,
    /// What peers see of this user.
    pub display_name: String,
    /// n0's relays for the app; none in tests, which stay on the machine.
    pub relays: Relays,
    /// Go online at once (the user's *Connect when cadrs starts*); otherwise only on
    /// [`start`].
    pub autostart: bool,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let identity = self
            .identity
            .clone()
            .map_or(Identity::Ephemeral, Identity::File);
        let (handler, requests) = transfer::handler();
        // The public directory: the app may have made it already (to show what was heard
        // before going online).
        let public = app
            .world()
            .get_resource::<public::Shared>()
            .cloned()
            .unwrap_or_default();
        let public_handler = public::handler(public.clone());
        let mut iroh = IrohPlugin::default()
            .with_identity(identity)
            .with_relays(self.relays.clone())
            .with_display_name(self.display_name.clone())
            .with_autostart(self.autostart)
            // A handler for each start: they all feed the same `Requests`.
            .with_protocol_factory(transfer::ALPN, move || {
                Box::new(handler.clone()) as Box<dyn DynProtocolHandler>
            })
            .with_protocol_factory(public::ALPN, move || {
                Box::new(public_handler.clone()) as Box<dyn DynProtocolHandler>
            });
        // Documents travel as their own messages, never as replicated entities.
        iroh.replicate_transform = false;
        app.add_plugins(iroh)
            .insert_resource(requests)
            .insert_resource(public)
            .insert_resource(CanConnect);
        if let Some(id) = cadrs_id(app.world().get_resource::<Iroh>()) {
            info!("cadrs ID: {id}");
        }
        session::register(app);
        public::register(app);
    }
}

/// Present when this run can go online: [`NetPlugin`] is added, whether or not the node is
/// running. Absent in scenarios, headless runs, tests and with `CADRS_NET=0`.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CanConnect;

/// Takes this node online, as the identity whose key is at `identity` (the one in use now: it
/// can change while offline), under `display_name`. Does nothing while it is online already.
pub fn start(
    world: &mut World,
    identity: Option<PathBuf>,
    display_name: String,
) -> Result<(), String> {
    if world.contains_resource::<Iroh>() {
        return Ok(());
    }
    if !world.contains_resource::<CanConnect>() {
        return Err("The network is off for this run".into());
    }
    world.commands().queue(StartIroh {
        identity: identity.map(Identity::File),
        display_name: Some(display_name),
    });
    world.flush();
    match cadrs_id(world.get_resource::<Iroh>()) {
        Some(id) => {
            info!("online; cadrs ID: {id}");
            Ok(())
        }
        None => Err(world
            .get_resource::<StartError>()
            .map_or_else(|| "The network did not start".to_string(), |e| e.0.clone())),
    }
}

/// Takes this node offline: every room is left (see [`StopIroh`]) and nothing reaches the
/// network until [`start`].
pub fn stop(world: &mut World) {
    if world.contains_resource::<Iroh>() {
        world.commands().queue(StopIroh);
        world.flush();
        info!("offline");
    }
}

/// This user's cadrs ID: the node's public key, what someone shares a document with. `None`
/// while networking is off.
pub fn cadrs_id(iroh: Option<&Iroh>) -> Option<String> {
    iroh.map(|i| i.id().to_string())
}
