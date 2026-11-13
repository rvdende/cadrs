//! The social layer of cadrs: accounts and their devices, organizations' signed logs, places,
//! and receipts. No Bevy, no kernel and no network here, so the phone app (`cadrs_mobile`) and
//! the desktop app share it, and every rule can be tested without peers.
//!
//! - [`chat`]: the public chat, with work, rates and blocks against spam.
//! - [`contact`]: contact cards, what a profile's QR code carries.
//! - [`keys`]: Ed25519 keys, signed statements, accounts and the certificates of their devices.
//! - [`keystore`]: a device's keys on disk, beside its network key.
//! - [`hlc`]: the hybrid logical clock that stamps every log entry.
//! - [`log`]: an append-only graph of signed entries, its one order every peer agrees on, and
//!   finality by witnesses.
//! - [`place`]: where someone is, as precisely as they choose to say, on any body.
//! - [`pairing`]: adding a device to an account by scanning a QR code.
//! - [`post`]: what travels to a device: letters, pairing, mailbox traffic.
//! - [`mailbox`]: nodes that hold sealed messages for people who are offline.
//! - [`mls`]: end-to-end encryption for groups of devices, its commits ordered by a log.
//! - [`receipts`]: statements two parties sign: stored data, relayed data, completed orders.
//! - [`seal`]: sealing a message for one device, for mailboxes.
//! - [`reputation`]: what a node has done for others, and the witness weight it earns.

pub mod chat;
pub mod contact;
pub mod hlc;
pub mod keys;
pub mod keystore;
pub mod log;
pub mod mailbox;
pub mod mls;
pub mod pairing;
pub mod place;
pub mod post;
pub mod receipts;
pub mod reputation;
pub mod seal;

/// Milliseconds since the Unix epoch, on every target (a page has no `std` clock).
pub fn wall_millis() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Random bytes from the system (the page's crypto in a browser).
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the system's random numbers");
    bytes
}

pub(crate) fn random32() -> [u8; 32] {
    random_bytes()
}

/// Lowercase hex of some bytes, for ids people see.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
