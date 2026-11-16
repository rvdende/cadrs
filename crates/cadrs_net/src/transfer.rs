//! Asking a peer about a shared document: one QUIC stream, dialled to the peer's key, on an
//! ALPN of our own (bevy_iroh's direct messages stop at a megabyte; a document, or an edit
//! that pastes one in, need not).
//!
//! Of the owner:
//! - **Join**: the document as it is now ([`Welcome`]: the access granted, the edit it is at,
//!   the document).
//! - **Propose**: an edit to number ([`Verdict`]). One at a time per peer, so they arrive in
//!   the order they were made; the numbered edit itself reaches everyone over the room.
//! - **AddMember**: share the document with someone else (for a member who may share).
//! - **Leave**: "Unshare with me".
//!
//! Of anyone: **Invite**, the owner telling someone the document is shared with them (and its
//! link, again whenever the room changes).
//!
//! Every request names the document and the room's topic, which only a holder of the link
//! knows. The app decides ([`Requests`]).

use std::time::Duration;

use bevy::prelude::*;
use bevy_iroh::{
    Iroh, IrohTask,
    iroh::{
        Endpoint, EndpointAddr, EndpointId,
        endpoint::Connection,
        protocol::{AcceptError, ProtocolHandler},
    },
};
use cadrs_core::collab::comments::{CommentOp, Comments};
use cadrs_core::collab::sync::Change;
use cadrs_core::collab::{self, Access, Sharing, Snapshot};
use cadrs_core::{DocumentId, ElementId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::{mpsc, oneshot};

use crate::link::ShareLink;

/// Versioned, so a peer with another framing fails to connect rather than misreads.
pub const ALPN: &[u8] = b"cadrs/doc/2";

/// The largest request (an edit) and the largest answer (a document), packed.
const MAX_MESSAGE: usize = 1 << 30;
/// How long the owner's app has to answer before the peer is told it did not.
const ANSWER_WITHIN: Duration = Duration::from_secs(60);
/// How long reaching the owner may take. A dial can hang rather than fail (an owner who went
/// offline and came back on new addresses), and a peer waits for each answer before it asks
/// again, so it would never ask again.
const REACH_WITHIN: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize, Deserialize)]
struct Wire {
    document: DocumentId,
    topic: [u8; 32],
    ask: WireAsk,
}

#[derive(Debug, Serialize, Deserialize)]
enum WireAsk {
    Join,
    Propose {
        epoch: u64,
        id: u64,
        base: u64,
        /// The packed [`Change`].
        change: Vec<u8>,
    },
    Invite(Invite),
    AddMember {
        id: String,
        name: String,
        access: Access,
    },
    Leave,
    /// The packed [`CommentOp`] (postcard can't carry its optional fields).
    Comment(Vec<u8>),
    CreateVersion {
        epoch: u64,
        name: String,
        description: String,
    },
    Lock {
        element: ElementId,
        take: bool,
    },
    Unlock {
        element: ElementId,
    },
    /// The packed [`Snapshot`].
    Handover(Vec<u8>),
    /// The packed [`Ownership`].
    TakeOwnership(Vec<u8>),
    Status,
}

/// A document given to a new owner (Transfer ownership): as it is, with who it is shared with
/// and its comments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ownership {
    pub snapshot: Snapshot,
    pub sharing: Sharing,
    pub comments: Comments,
    /// The owner it came from.
    pub from_name: String,
}

/// The owner telling someone a document is shared with them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    /// The link, which names the room as it is now.
    pub link: String,
    pub owner_name: String,
    pub access: Access,
    /// The share dialog's message; empty for a link sent again.
    pub message: String,
    /// True for a link sent again (the room changed): no news for the recipient.
    pub update: bool,
}

/// What the owner sends someone they let in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub access: Access,
    pub owner_name: String,
    /// The owner's session: edits are numbered afresh each time the owner opens the document.
    pub epoch: u64,
    /// The document after edit `snapshot.seq`.
    pub snapshot: Snapshot,
    /// Its comments.
    #[serde(default)]
    pub comments: Comments,
}

/// The owner's answer to a proposed edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Numbered: it reaches everyone as this edit.
    Accepted(u64),
    /// Refused, and why (someone else changed the same thing first, view access, …).
    Refused(String),
    /// Made from a copy the owner no longer has (an old session, or too far behind): fetch
    /// the document again.
    Resync,
    /// The owner does not have the document open: nobody numbers edits.
    Closed,
}

/// What a peer asks.
#[derive(Debug)]
pub enum Ask {
    Join,
    Propose {
        epoch: u64,
        id: u64,
        base: u64,
        change: Change,
    },
    Invite(Invite),
    AddMember {
        id: String,
        name: String,
        access: Access,
    },
    Leave,
    /// A change to the comments (for someone who may comment).
    Comment(CommentOp),
    /// Make a version of the document as it is (for someone who may edit).
    CreateVersion {
        epoch: u64,
        name: String,
        description: String,
    },
    /// Open a tab for oneself only (`take`: from whoever has it).
    Lock {
        element: ElementId,
        take: bool,
    },
    /// Let the others edit it again.
    Unlock {
        element: ElementId,
    },
    /// From whoever kept the edits in order while the owner was away: the document as it is
    /// now, for the owner to take on.
    Handover(Snapshot),
    /// To a member, from the owner: the document is theirs now.
    TakeOwnership(Box<Ownership>),
    /// Whether the owner has the document open (answered `Accepted(1)`, or `Accepted(0)`).
    Status,
}

/// A peer's request. The app checks the topic and the peer against the document's sharing and
/// answers; one dropped unanswered is a refusal.
#[derive(Debug)]
pub struct Request {
    pub from: EndpointId,
    pub document: DocumentId,
    pub topic: [u8; 32],
    pub ask: Ask,
    reply: oneshot::Sender<Result<Vec<u8>, String>>,
}

impl Request {
    /// Answers a Join with the document, or the reason it is refused (shown to the peer).
    pub fn welcome(self, welcome: Result<Welcome, String>) {
        let _ = self.reply.send(welcome.and_then(|w| collab::pack(&w)));
    }

    /// Answers a Propose, an Invite, an AddMember or a Leave.
    pub fn verdict(self, verdict: Verdict) {
        let _ = self.reply.send(collab::pack(&verdict));
    }

    /// Refuses whatever it asks, with the reason.
    pub fn refuse(self, why: impl Into<String>) {
        let _ = self.reply.send(Err(why.into()));
    }
}

/// The requests that arrived and are not answered yet.
#[derive(Resource)]
pub struct Requests(mpsc::UnboundedReceiver<Request>);

impl Requests {
    pub fn drain(&mut self) -> Vec<Request> {
        let mut out = Vec::new();
        while let Ok(r) = self.0.try_recv() {
            out.push(r);
        }
        out
    }
}

/// The accept side, and the [`Requests`] it feeds.
pub(crate) fn handler() -> (Handler, Requests) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Handler { tx }, Requests(rx))
}

#[derive(Debug, Clone)]
pub(crate) struct Handler {
    tx: mpsc::UnboundedSender<Request>,
}

impl Handler {
    async fn answer(&self, from: EndpointId, request: &[u8]) -> Result<Vec<u8>, String> {
        let wire: Wire = postcard::from_bytes(request).map_err(|_| "Not a request".to_string())?;
        let ask = match wire.ask {
            WireAsk::Join => Ask::Join,
            WireAsk::Propose {
                epoch,
                id,
                base,
                change,
            } => Ask::Propose {
                epoch,
                id,
                base,
                change: collab::unpack(&change)?,
            },
            WireAsk::Invite(i) => Ask::Invite(i),
            WireAsk::AddMember { id, name, access } => Ask::AddMember { id, name, access },
            WireAsk::Leave => Ask::Leave,
            WireAsk::Comment(op) => Ask::Comment(collab::unpack(&op)?),
            WireAsk::CreateVersion {
                epoch,
                name,
                description,
            } => Ask::CreateVersion {
                epoch,
                name,
                description,
            },
            WireAsk::Lock { element, take } => Ask::Lock { element, take },
            WireAsk::Unlock { element } => Ask::Unlock { element },
            WireAsk::Handover(s) => Ask::Handover(collab::unpack(&s)?),
            WireAsk::TakeOwnership(o) => Ask::TakeOwnership(Box::new(collab::unpack(&o)?)),
            WireAsk::Status => Ask::Status,
        };
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Request {
                from,
                document: wire.document,
                topic: wire.topic,
                ask,
                reply,
            })
            .map_err(|_| "The owner's cadrs is closing".to_string())?;
        match n0_future::time::timeout(ANSWER_WITHIN, answer).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("The owner's cadrs refused the request".into()),
            Err(_) => Err("The owner's cadrs did not answer".into()),
        }
    }
}

impl ProtocolHandler for Handler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let from = connection.remote_id();
        let Ok((mut send, mut recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let answer = match recv.read_to_end(MAX_MESSAGE).await {
            Ok(request) => self.answer(from, &request).await,
            Err(e) => Err(format!("Cannot read the request: {e}")),
        };
        // One byte of status, then the packed answer or the reason.
        let reply = match answer {
            Ok(bytes) => [&[0u8][..], &bytes].concat(),
            Err(why) => {
                debug!("cadrs_net: refused {}: {why}", from.fmt_short());
                [&[1u8][..], why.as_bytes()].concat()
            }
        };
        if send.write_all(&reply).await.is_ok() {
            let _ = send.finish();
        }
        connection.closed().await;
        Ok(())
    }
}

/// Sends `wire` to `owner` and reads the answer.
async fn ask<T: DeserializeOwned>(
    endpoint: Result<Endpoint, String>,
    owner: EndpointAddr,
    wire: Wire,
) -> Result<T, String> {
    let endpoint = endpoint?;
    let who = owner.id.fmt_short();
    let reach = async {
        let conn = endpoint
            .connect(owner, ALPN)
            .await
            .map_err(|e| e.to_string())?;
        let streams = conn.open_bi().await.map_err(|e| e.to_string())?;
        Ok::<_, String>((conn, streams))
    };
    let (conn, (mut send, mut recv)) = n0_future::time::timeout(REACH_WITHIN, reach)
        .await
        .map_err(|_| "timed out".to_string())
        .and_then(|r| r)
        .map_err(|e| format!("Cannot reach the owner ({who}): {e}"))?;
    let request = postcard::to_stdvec(&wire).map_err(|e| e.to_string())?;
    send.write_all(&request)
        .await
        .map_err(|e| format!("Lost the owner ({who}): {e}"))?;
    let _ = send.finish();
    let reply = recv
        .read_to_end(MAX_MESSAGE)
        .await
        .map_err(|e| format!("Lost the owner ({who}): {e}"))?;
    conn.close(0u32.into(), b"done");
    match reply.split_first() {
        Some((0, packed)) => collab::unpack(packed),
        Some((_, why)) => Err(String::from_utf8_lossy(why).into_owned()),
        None => Err(format!("The owner ({who}) sent nothing")),
    }
}

/// Asks the link's owner for the document.
pub async fn fetch(endpoint: Endpoint, link: ShareLink) -> Result<Welcome, String> {
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Join,
    };
    ask(Ok(endpoint), link.owner, wire).await
}

/// The endpoint to dial from. In a page it binds a few frames after startup, not before it.
fn dial_from(iroh: &Iroh) -> Result<Endpoint, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(iroh.endpoint());
    #[cfg(target_arch = "wasm32")]
    iroh.endpoint()
        .ok_or_else(|| "Not connected to the network yet".to_string())
}

/// [`fetch`] on the network runtime: poll the task from a system.
pub fn start_fetch(iroh: &Iroh, link: ShareLink) -> IrohTask<Result<Welcome, String>> {
    let endpoint = dial_from(iroh);
    iroh.spawn(async move { fetch(endpoint?, link).await })
}

/// Sends edit `id` (made from edit `base` of session `epoch`) to the link's owner. `Err` is a
/// failure to ask (the owner unreachable), not a refusal.
pub fn start_propose(
    iroh: &Iroh,
    link: &ShareLink,
    epoch: u64,
    id: u64,
    base: u64,
    change: &Change,
) -> IrohTask<Result<Verdict, String>> {
    let wire = collab::pack(change).map(|change| Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Propose {
            epoch,
            id,
            base,
            change,
        },
    });
    let (endpoint, owner) = (dial_from(iroh), link.owner.clone());
    iroh.spawn(async move { ask(endpoint, owner, wire?).await })
}

/// Tells `to` that a document is shared with them (or its new link).
pub fn start_invite(
    iroh: &Iroh,
    to: EndpointId,
    link: &ShareLink,
    invite: Invite,
) -> IrohTask<Result<Verdict, String>> {
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Invite(invite),
    };
    iroh.spawn(ask(dial_from(iroh), EndpointAddr::new(to), wire))
}

/// Asks the link's owner to share the document with someone (a member who may share).
pub fn start_add_member(
    iroh: &Iroh,
    link: &ShareLink,
    id: String,
    name: String,
    access: Access,
) -> IrohTask<Result<Verdict, String>> {
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::AddMember { id, name, access },
    };
    iroh.spawn(ask(dial_from(iroh), link.owner.clone(), wire))
}

/// Tells the link's owner this user no longer wants the document ("Unshare with me").
pub fn start_leave(iroh: &Iroh, link: &ShareLink) -> IrohTask<Result<Verdict, String>> {
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Leave,
    };
    iroh.spawn(ask(dial_from(iroh), link.owner.clone(), wire))
}

/// Sends a change to the comments to the link's owner.
pub fn start_comment(
    iroh: &Iroh,
    link: &ShareLink,
    op: CommentOp,
) -> IrohTask<Result<Verdict, String>> {
    let wire = collab::pack(&op).map(|op| Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Comment(op),
    });
    let (endpoint, owner) = (dial_from(iroh), link.owner.clone());
    iroh.spawn(async move { ask(endpoint, owner, wire?).await })
}

/// Asks the link's owner to make a version of the document as it is now.
pub fn start_create_version(
    iroh: &Iroh,
    link: &ShareLink,
    epoch: u64,
    name: String,
    description: String,
) -> IrohTask<Result<Verdict, String>> {
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::CreateVersion {
            epoch,
            name,
            description,
        },
    };
    iroh.spawn(ask(dial_from(iroh), link.owner.clone(), wire))
}

/// Asks the link's owner to open a tab for this user only (`take`: from whoever has it), or
/// (`lock` false) to let the others edit it again.
pub fn start_lock(
    iroh: &Iroh,
    link: &ShareLink,
    element: ElementId,
    lock: bool,
    take: bool,
) -> IrohTask<Result<Verdict, String>> {
    let request = if lock {
        WireAsk::Lock { element, take }
    } else {
        WireAsk::Unlock { element }
    };
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: request,
    };
    iroh.spawn(ask(dial_from(iroh), link.owner.clone(), wire))
}

/// Gives the real owner (back from away) the document as it is now: from whoever kept the
/// edits in order meanwhile.
pub fn start_handover(
    iroh: &Iroh,
    link: &ShareLink,
    snapshot: &Snapshot,
) -> IrohTask<Result<Verdict, String>> {
    let wire = collab::pack(snapshot).map(|s| Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Handover(s),
    });
    let (endpoint, owner) = (dial_from(iroh), link.owner.clone());
    iroh.spawn(async move { ask(endpoint, owner, wire?).await })
}

/// Makes `to` the document's owner (Transfer ownership).
pub fn start_transfer(
    iroh: &Iroh,
    to: EndpointId,
    link: &ShareLink,
    ownership: &Ownership,
) -> IrohTask<Result<Verdict, String>> {
    let wire = collab::pack(ownership).map(|o| Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::TakeOwnership(o),
    });
    let endpoint = dial_from(iroh);
    iroh.spawn(async move { ask(endpoint, EndpointAddr::new(to), wire?).await })
}

/// Asks the link's owner whether they have the document open (a few bytes, where fetching it
/// would be the whole document).
pub fn start_status(iroh: &Iroh, link: &ShareLink) -> IrohTask<Result<Verdict, String>> {
    let wire = Wire {
        document: link.document,
        topic: link.topic,
        ask: WireAsk::Status,
    };
    iroh.spawn(ask(dial_from(iroh), link.owner.clone(), wire))
}
