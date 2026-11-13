//! What travels to a device over cadrs's social connection: a letter, a step of pairing, or
//! mailbox traffic. One connection, one [`Post`], one answer.

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
    },
}

/// The answer to [`Post::SyncLog`]: the entries the asker lacks, and the answerer's heads (so
/// the asker can send what the answerer lacks).
pub type Synced = (Vec<Entry>, BTreeSet<EntryId>);

/// A mailbox's answer to [`Post::Collect`]: the envelopes, each with the storage receipt the
/// mailbox signed, for the collector to countersign.
pub type Collected = Vec<(Signed<Envelope>, HalfSigned)>;

impl Post {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("posts encode")
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}
