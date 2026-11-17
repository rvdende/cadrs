//! cadrs's social connection (`cadrs/social/1`) on a plain iroh endpoint, without Bevy: what the
//! phone app (`cadrs_mobile`) talks to shops, mailboxes and its own account's devices with. The
//! desktop app speaks the same protocol through `cadrs_net::social`.
//!
//! One connection carries one post (`cadrs_social::post::Post`, as bytes) and its answer: a
//! status byte (0 taken, 1 refused), then the answer or the reason.

use std::time::Duration;

use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey};
use tokio::sync::{mpsc, oneshot};

pub use iroh;

/// The protocol, as `cadrs_net::social` speaks it.
pub const ALPN: &[u8] = b"cadrs/social/1";
/// The largest post (a request for a quote carries its parts' document).
const MAX_POST: usize = 64 << 20;
const ANSWER_WITHIN: Duration = Duration::from_secs(30);
const REACH_WITHIN: Duration = Duration::from_secs(15);

/// A post that arrived. Answer it with [`Incoming::take`] or [`Incoming::refuse`]; one dropped
/// unanswered is refused.
#[derive(Debug)]
pub struct Incoming {
    pub from: EndpointId,
    pub bytes: Vec<u8>,
    reply: oneshot::Sender<Result<Vec<u8>, String>>,
}

impl Incoming {
    pub fn take(self, answer: Vec<u8>) {
        let _ = self.reply.send(Ok(answer));
    }

    pub fn refuse(self, why: impl Into<String>) {
        let _ = self.reply.send(Err(why.into()));
    }
}

#[derive(Debug, Clone)]
struct Handler {
    tx: mpsc::UnboundedSender<Incoming>,
}

impl ProtocolHandler for Handler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let from = connection.remote_id();
        let Ok((mut send, mut recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let answer = match recv.read_to_end(MAX_POST).await {
            Ok(bytes) => {
                let (reply, answer) = oneshot::channel();
                if self.tx.send(Incoming { from, bytes, reply }).is_err() {
                    Err("cadrs is closing".to_string())
                } else {
                    match n0_future::time::timeout(ANSWER_WITHIN, answer).await {
                        Ok(Ok(r)) => r,
                        Ok(Err(_)) => Err("The post was not taken".into()),
                        Err(_) => Err("cadrs did not answer".into()),
                    }
                }
            }
            Err(e) => Err(format!("Cannot read the post: {e}")),
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

/// This device on the network.
#[derive(Debug, Clone)]
pub struct Node {
    endpoint: Endpoint,
    lookup: iroh::address_lookup::memory::MemoryLookup,
    _router: std::sync::Arc<Router>,
}

impl Node {
    /// Goes online as the device with key `secret` (its 32 bytes). `relays`: through n0's
    /// relays (a phone always is; tests stay on the machine). Posts that arrive come out of the
    /// receiver.
    pub async fn spawn(
        secret: [u8; 32],
        relays: bool,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Incoming>), String> {
        let lookup = iroh::address_lookup::memory::MemoryLookup::new();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&secret))
            .relay_mode(if relays {
                RelayMode::Default
            } else {
                RelayMode::Disabled
            })
            .address_lookup(lookup.clone())
            .bind()
            .await
            .map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::unbounded_channel();
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, Handler { tx })
            .spawn();
        Ok((
            Self {
                endpoint,
                lookup,
                _router: std::sync::Arc::new(router),
            },
            rx,
        ))
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Where this device can be reached now.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Remembers where a device is (one met on this network, or in a test).
    pub fn remember(&self, addr: EndpointAddr) {
        self.lookup.add_endpoint_info(addr);
    }

    /// Sends a post to device `to` and waits for its answer.
    pub async fn send(&self, to: EndpointAddr, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let who = to.id.fmt_short();
        let reach = async {
            let conn = self
                .endpoint
                .connect(to, ALPN)
                .await
                .map_err(|e| e.to_string())?;
            let streams = conn.open_bi().await.map_err(|e| e.to_string())?;
            Ok::<_, String>((conn, streams))
        };
        let (conn, (mut send, mut recv)) = n0_future::time::timeout(REACH_WITHIN, reach)
            .await
            .map_err(|_| "timed out".to_string())
            .and_then(|r| r)
            .map_err(|e| format!("Cannot reach {who}: {e}"))?;
        send.write_all(bytes)
            .await
            .map_err(|e| format!("Lost {who}: {e}"))?;
        let _ = send.finish();
        let reply = recv
            .read_to_end(MAX_POST)
            .await
            .map_err(|e| format!("Lost {who}: {e}"))?;
        conn.close(0u32.into(), b"done");
        match reply.split_first() {
            Some((0, answer)) => Ok(answer.to_vec()),
            Some((_, why)) => Err(String::from_utf8_lossy(why).into_owned()),
            None => Err(format!("{who} sent nothing")),
        }
    }
}
