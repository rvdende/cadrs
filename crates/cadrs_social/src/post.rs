//! What travels to a device over cadrs's social connection: a letter, a step of pairing,
//! mailbox traffic, or members' devices keeping an organization in step. One connection, one
//! [`Post`], one answer.
//!
//! Posts and their answers are written with their names ([`crate::codec`]): a device on an
//! older cadrs refuses a kind of post it doesn't know (the sender hears why) and reads the ones
//! it does, whatever was added to them since.

use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

use crate::keys::{AccountId, DeviceCert, DeviceId, Signed};
use crate::log::{Entry, EntryId, LogId};
use crate::mailbox::{Collect, Envelope};
use crate::pairing::PairingRequest;
use crate::receipts::HalfSigned;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Post {
    /// A signed letter between people and shops (`cadrs_org::letters`).
    Letter(#[serde(with = "serde_bytes")] Vec<u8>),
    /// A new device asks to join the account.
    Pair(PairingRequest),
    /// The account's certificate for a device that asked to join.
    Grant(Signed<DeviceCert>),
    /// For a mailbox: keep this until its account collects it.
    Deposit(Signed<Envelope>),
    /// For a mailbox: a device of the account collects what is kept for it. Answered with
    /// [`Collected`].
    Collect {
        request: Signed<Collect>,
        cert: Signed<DeviceCert>,
    },
    /// To a fellow member's device: the entries of log `log` this device (with `heads`) lacks.
    /// `cert` says whose device asks (a member's, maybe one the organization doesn't know yet).
    /// Answered with [`Synced`].
    SyncLog {
        log: LogId,
        heads: BTreeSet<EntryId>,
        cert: Signed<DeviceCert>,
    },
    /// To a fellow member's device: entries it lacks (the other half of a sync).
    LogEntries { log: LogId, entries: Vec<Entry> },
    /// To another device of the same account: this device's book of requests for quotes (the
    /// app's, encoded), to merge. `cert` shows the sender is the account's.
    Mirror {
        cert: Signed<DeviceCert>,
        #[serde(with = "serde_bytes")]
        book: Vec<u8>,
    },
    /// To someone just made a member: the organization, its log, and the devices to sync with.
    Invite {
        log: LogId,
        org: AccountId,
        name: String,
        devices: Vec<DeviceId>,
        /// The organization's certificate for the invited device, when the inviter holds the
        /// organization's key and wants it to speak for the organization (an owner's own phone).
        cert: Option<Signed<DeviceCert>>,
        /// Who invites: the inviting device's certificate from its own account, so the invited
        /// can see who it is, and take an invitation from their own account without asking.
        #[serde(default)]
        inviter: Option<Signed<DeviceCert>>,
    },
    /// To a fellow member's device: the blobs of organization `log` (a logo, a request's
    /// document) this device lacks, by hash. `cert` says whose device asks. Answered with
    /// [`Blobs`]: those it has, as many as fit an answer.
    WantBlobs {
        log: LogId,
        cert: Signed<DeviceCert>,
        blobs: Vec<[u8; 32]>,
    },
    /// To a device of a listed organization, from anyone: a picture its listing names (its
    /// logo). Answered with [`Blobs`]: the one picture, or none.
    PublicBlob { org: AccountId, hash: [u8; 32] },
    /// To a bank (`cadrs_credits`; cadrs's seed is one): an ask about a wallet, signed by the
    /// device that sends it. Answered with the bank's answer: a statement, or a receipt it
    /// signed.
    Bank(#[serde(with = "serde_bytes")] Vec<u8>),
    /// From a bank, to a device that looked at a wallet lately: a movement of that wallet as it
    /// happened, as the receipt the bank signed.
    Credited(#[serde(with = "serde_bytes")] Vec<u8>),
}

/// The answer to [`Post::WantBlobs`] and [`Post::PublicBlob`]: blobs' bytes, for the asker to
/// check against the hashes it asked for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Blobs(pub Vec<serde_bytes::ByteBuf>);

/// The answer to [`Post::SyncLog`]: the entries the asker lacks, and the answerer's heads (so
/// the asker can send what the answerer lacks).
pub type Synced = (Vec<Entry>, BTreeSet<EntryId>);

/// A mailbox's answer to [`Post::Collect`]: the envelopes, each with the storage receipt the
/// mailbox signed, for the collector to countersign.
pub type Collected = Vec<(Signed<Envelope>, HalfSigned)>;

impl Post {
    pub fn encode(&self) -> Vec<u8> {
        crate::codec::encode(self)
    }

    /// Reads a post, refusing one larger than its kind may be ([`Post::max_len`]): whoever
    /// takes posts in takes no more than that from anyone.
    pub fn decode(bytes: &[u8]) -> Result<Self, crate::codec::Error> {
        // Told by its kind's name, before anything of it is read: megabytes of something that
        // may only be small aren't decoded to find that out.
        if bytes.len() > MAX_SMALL_POST && !Self::may_be_large(bytes) {
            return Err(crate::codec::Error::Malformed(
                "larger than a post of its kind may be".into(),
            ));
        }
        let post: Self = crate::codec::decode(bytes)?;
        if bytes.len() > post.max_len() {
            return Err(crate::codec::Error::Malformed(
                "larger than a post of its kind may be".into(),
            ));
        }
        Ok(post)
    }

    /// Whether these bytes start as a post of one of the kinds that may be large does: a map of
    /// one entry whose key, the kind's name, is one of theirs.
    fn may_be_large(bytes: &[u8]) -> bool {
        const LARGE: [&str; 4] = ["Letter", "Deposit", "LogEntries", "Mirror"];
        let Some((&0x81, rest)) = bytes.split_first() else {
            return false;
        };
        let Some((&tag, rest)) = rest.split_first() else {
            return false;
        };
        // A short string: 0xa0 with its length in the low five bits.
        let len = usize::from(tag & 0x1f);
        tag & 0xe0 == 0xa0
            && rest
                .get(..len)
                .is_some_and(|name| LARGE.iter().any(|kind| kind.as_bytes() == name))
    }

    /// The most bytes a post of this kind may be. Those that carry documents or logs (a
    /// letter, an envelope for a mailbox, a member's log entries, an account's own book) may be
    /// as large as the network carries, and what is in them has limits of its own; everything
    /// else is a few names and keys, and is held to [`MAX_SMALL_POST`], so that an invitation
    /// or a pairing request can't be megabytes of someone's choosing.
    pub fn max_len(&self) -> usize {
        match self {
            Post::Letter(_) | Post::Deposit(_) | Post::LogEntries { .. } | Post::Mirror { .. } => {
                MAX_POST
            }
            _ => MAX_SMALL_POST,
        }
    }
}

/// How the error of a send starts when the device was reached and refused the post, as
/// distinct from not being reached: what follows is its reason. A refusal isn't cured by trying
/// again or by leaving the post at a mailbox.
pub const REFUSED: &str = "refused: ";

/// The most of a refusal's reason that is kept, in characters: a sentence. The reason is
/// whatever the other device chose to send.
pub const MAX_REASON: usize = 200;

/// A reached device's refusal, as the sender's error: [`REFUSED`] and its reason, cut to
/// [`MAX_REASON`] (what refused may have sent megabytes, to be shown and kept).
pub fn refused(why: &[u8]) -> String {
    // No more is looked at than could matter: a character is at most four bytes.
    let head = &why[..why.len().min(4 * MAX_REASON)];
    let reason: String = String::from_utf8_lossy(head)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_REASON)
        .collect();
    format!("{REFUSED}{reason}")
}

/// The reason a reached device gave for refusing, when `error` is that ([`refused`]).
pub fn refusal(error: &str) -> Option<&str> {
    error.strip_prefix(REFUSED)
}

/// The largest post the network carries, in bytes.
pub const MAX_POST: usize = 64 << 20;
/// The largest post that carries no document or log.
pub const MAX_SMALL_POST: usize = 64 << 10;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Account, SecretKey};
    use crate::log::LogId;

    /// A post that carries no document is small, whatever is put in it: an invitation with a
    /// megabyte for a name isn't read at all.
    #[test]
    fn a_post_is_no_larger_than_its_kind_may_be() {
        let account = Account::generate();
        let device = SecretKey::generate();
        let invite = |name: String| Post::Invite {
            log: LogId([1; 32]),
            org: account.id(),
            name,
            devices: vec![device.public()],
            cert: None,
            inviter: Some(account.certify(device.public(), "laptop", 1)),
        };
        let small = invite("Cape Waterjet".into()).encode();
        assert!(Post::decode(&small).is_ok());
        let huge = invite("N".repeat(MAX_SMALL_POST)).encode();
        assert!(Post::decode(&huge).is_err());
        // The certificate's own name is no way round it.
        let named = Post::Invite {
            log: LogId([1; 32]),
            org: account.id(),
            name: "Trap".into(),
            devices: vec![],
            cert: Some(account.certify(device.public(), &"N".repeat(MAX_SMALL_POST), 1)),
            inviter: None,
        };
        assert!(Post::decode(&named.encode()).is_err());
        // A refusal's reason is a sentence, whatever was sent as one.
        assert_eq!(refused(b"not for us"), "refused: not for us");
        assert_eq!(refusal(&refused(b"not for us")), Some("not for us"));
        let endless = refused(&vec![b'N'; 5 << 20]);
        assert_eq!(endless.chars().count(), REFUSED.len() + MAX_REASON);
        let wide = refused("𝕏".repeat(10_000).as_bytes());
        assert_eq!(wide.chars().count(), REFUSED.len() + MAX_REASON);
        assert_eq!(refused(b"line\none\x07"), "refused: lineone");
        assert_eq!(refusal("Cannot reach abc: timed out"), None);
        // A letter may carry a document.
        let letter = Post::Letter(vec![7; 2 * MAX_SMALL_POST]).encode();
        assert!(Post::decode(&letter).is_ok());
        // Which kinds may be large is told from the first bytes, the same as after reading.
        for (post, large) in [
            (Post::Letter(vec![1]), true),
            (
                Post::LogEntries {
                    log: LogId([1; 32]),
                    entries: vec![],
                },
                true,
            ),
            (invite("Cape Waterjet".into()), false),
            (
                Post::PublicBlob {
                    org: account.id(),
                    hash: [0; 32],
                },
                false,
            ),
        ] {
            assert_eq!(Post::may_be_large(&post.encode()), large);
            assert_eq!(post.max_len() == MAX_POST, large);
        }
    }
}
