//! Letters between clients and shops (`cadrs_org::letters`): one QUIC stream to one of the
//! recipient's devices, on an ALPN of our own. The connection is encrypted to that device's key,
//! so a letter delivered this way is read by nobody in between.
//!
//! The bytes are a [`Post`]. A node that keeps a mailbox ([`enable_mailbox`]: the seed peer,
//! a shop's always-on box) answers deposits and collections itself; everything else goes to the
//! app ([`Letters`]), which takes it (with an answer, perhaps) or refuses it and says why.

use std::time::Duration;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy_iroh::iroh::{
    EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use bevy_iroh::{Iroh, IrohTask};
use cadrs_social::keys::SecretKey;
use cadrs_social::mailbox::{Mailbox, Terms};
use cadrs_social::post::Post;
use tokio::sync::{mpsc, oneshot};

/// Versioned, so a peer with another framing fails to connect rather than misreads.
pub const ALPN: &[u8] = b"cadrs/social/1";

/// The largest letter (a request for a quote carries its parts' document).
const MAX_LETTER: usize = 64 << 20;
/// How long the app has to take a letter.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);
/// How long reaching the recipient may take.
const REACH_WITHIN: Duration = Duration::from_secs(15);

/// A letter that arrived. Answer it with [`Letter::take`] or [`Letter::refuse`]; one dropped
/// unanswered is refused.
#[derive(Debug)]
pub struct Letter {
    pub from: EndpointId,
    pub bytes: Vec<u8>,
    reply: oneshot::Sender<Result<Vec<u8>, String>>,
}

impl Letter {
    pub fn take(self) {
        let _ = self.reply.send(Ok(Vec::new()));
    }

    /// Takes it, answering with `answer`.
    pub fn take_with(self, answer: Vec<u8>) {
        let _ = self.reply.send(Ok(answer));
    }

    pub fn refuse(self, why: impl Into<String>) {
        let _ = self.reply.send(Err(why.into()));
    }
}

/// The letters that arrived and wait for the app.
#[derive(Resource)]
pub struct Letters(mpsc::UnboundedReceiver<Letter>);

impl Letters {
    pub fn drain(&mut self) -> Vec<Letter> {
        let mut out = Vec::new();
        while let Ok(letter) = self.0.try_recv() {
            out.push(letter);
        }
        out
    }
}

/// A mailbox this node keeps for others, with the node's key (it signs the storage receipts)
/// and the file it is kept in.
pub struct NodeMailbox {
    key: SecretKey,
    mailbox: Mailbox,
    file: Option<PathBuf>,
}

impl NodeMailbox {
    fn save(&self) {
        if let Some(file) = &self.file {
            let bytes = postcard::to_allocvec(&self.mailbox).expect("mailboxes encode");
            let tmp = file.with_extension("tmp");
            let written = cadrs_fs::write(&tmp, bytes).and_then(|()| cadrs_fs::rename(&tmp, file));
            if let Err(e) = written {
                warn!("cannot keep the mailbox in {}: {e}", file.display());
            }
        }
    }

    /// Envelopes held, and their bytes.
    pub fn held(&self) -> (usize, u64) {
        (self.mailbox.len(), self.mailbox.bytes())
    }
}

/// This node's mailbox, when it keeps one.
#[derive(Resource, Clone, Default)]
pub struct Mailboxes(pub Arc<Mutex<Option<NodeMailbox>>>);

/// Keeps a mailbox on this node, signed for with `key` (the node's own key, so senders reach it
/// by its cadrs ID), kept in `file` (read back if it is there).
pub fn enable_mailbox(world: &mut World, key: SecretKey, file: Option<PathBuf>, terms: Terms) {
    let mut mailbox = file
        .as_ref()
        .and_then(|f| cadrs_fs::read(f).ok())
        .and_then(|b| postcard::from_bytes::<Mailbox>(&b).ok())
        .unwrap_or_else(|| Mailbox::new(terms));
    mailbox.terms = terms;
    let shared = world
        .get_resource_or_insert_with(Mailboxes::default)
        .0
        .clone();
    *shared.lock().unwrap_or_else(|e| e.into_inner()) = Some(NodeMailbox { key, mailbox, file });
}

pub(crate) fn handler(mailboxes: Mailboxes) -> (Handler, Letters) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Handler { tx, mailboxes }, Letters(rx))
}

#[derive(Clone)]
pub(crate) struct Handler {
    tx: mpsc::UnboundedSender<Letter>,
    mailboxes: Mailboxes,
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("social::Handler")
    }
}

impl Handler {
    /// Answers mailbox traffic here, if this node keeps a mailbox; `None` for the app.
    fn mailbox(&self, bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
        let post = Post::decode(bytes)?;
        if !matches!(post, Post::Deposit(_) | Post::Collect { .. }) {
            return None;
        }
        let mut guard = self.mailboxes.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some(node) = guard.as_mut() else {
            return Some(Err("This node keeps no mailbox".into()));
        };
        let now = cadrs_social::wall_millis();
        node.mailbox.expire(now);
        let answer = match post {
            Post::Deposit(envelope) => node
                .mailbox
                .deposit(envelope, now)
                .map(|_| Vec::new())
                .map_err(|e| format!("The mailbox refused it: {e:?}")),
            Post::Collect { request, cert } => node
                .mailbox
                .collect(&node.key, &request, &cert, now)
                .map(|got| postcard::to_allocvec(&got).expect("encodes"))
                .map_err(|e| format!("The mailbox refused: {e:?}")),
            _ => unreachable!(),
        };
        if answer.is_ok() {
            node.save();
        }
        Some(answer)
    }
}

impl ProtocolHandler for Handler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let from = connection.remote_id();
        let Ok((mut send, mut recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let answer = match recv.read_to_end(MAX_LETTER).await {
            Ok(bytes) => match self.mailbox(&bytes) {
                Some(answer) => answer,
                None => {
                    let (reply, answer) = oneshot::channel();
                    if self.tx.send(Letter { from, bytes, reply }).is_err() {
                        Err("cadrs is closing".to_string())
                    } else {
                        match tokio::time::timeout(ANSWER_WITHIN, answer).await {
                            Ok(Ok(r)) => r,
                            Ok(Err(_)) => Err("The letter was not taken".into()),
                            Err(_) => Err("cadrs did not answer".into()),
                        }
                    }
                }
            },
            Err(e) => Err(format!("Cannot read the letter: {e}")),
        };
        let reply = match answer {
            Ok(bytes) => [&[0u8][..], &bytes].concat(),
            Err(why) => [&[1u8][..], why.as_bytes()].concat(),
        };
        if send.write_all(&reply).await.is_ok() {
            let _ = send.finish();
        }
        connection.closed().await;
        Ok(())
    }
}

/// Sends a post to device `to` (its bare id is enough online: discovery finds it); `Ok` with
/// its answer once its cadrs took it.
pub fn start_send(
    iroh: &Iroh,
    to: EndpointAddr,
    bytes: Vec<u8>,
) -> IrohTask<Result<Vec<u8>, String>> {
    #[cfg(not(target_arch = "wasm32"))]
    let endpoint: Result<_, String> = Ok(iroh.endpoint());
    #[cfg(target_arch = "wasm32")]
    let endpoint = iroh
        .endpoint()
        .ok_or_else(|| "Not connected to the network yet".to_string());
    iroh.spawn(async move {
        let endpoint = endpoint?;
        let who = to.id.fmt_short();
        let reach = async {
            let conn = endpoint
                .connect(to, ALPN)
                .await
                .map_err(|e| e.to_string())?;
            let streams = conn.open_bi().await.map_err(|e| e.to_string())?;
            Ok::<_, String>((conn, streams))
        };
        let (conn, (mut send, mut recv)) = tokio::time::timeout(REACH_WITHIN, reach)
            .await
            .map_err(|_| "timed out".to_string())
            .and_then(|r| r)
            .map_err(|e| format!("Cannot reach {who}: {e}"))?;
        send.write_all(&bytes)
            .await
            .map_err(|e| format!("Lost {who}: {e}"))?;
        let _ = send.finish();
        let reply = recv
            .read_to_end(MAX_LETTER)
            .await
            .map_err(|e| format!("Lost {who}: {e}"))?;
        conn.close(0u32.into(), b"done");
        match reply.split_first() {
            Some((0, answer)) => Ok(answer.to_vec()),
            Some((_, why)) => Err(String::from_utf8_lossy(why).into_owned()),
            None => Err(format!("{who} sent nothing")),
        }
    })
}
