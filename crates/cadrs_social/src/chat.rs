//! The public chat: lines anyone says in the public room, for everyone.
//!
//! A [`Line`] is signed by the device that said it and carries the device's certificate, so it
//! is known whose account said it. Open to anyone means open to spam, so:
//!
//! - **Work:** a line carries a nonce that makes the BLAKE3 hash of the signed line start with
//!   [`WORK_BITS`] zero bits: about 65 000 hashes, nothing for one line, costly for a flood.
//! - **Rate:** a [`Room`] takes at most [`PER_MINUTE`] lines a minute from one account.
//! - **Blocks:** it drops lines from accounts the user blocked (or a list they follow).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::keys::{AccountId, DeviceCert, SecretKey, Signed, Statement};

/// Leading zero bits a line's hash needs.
pub const WORK_BITS: u32 = 16;
/// The most lines one account may say in a minute.
pub const PER_MINUTE: usize = 5;
/// The longest line, in bytes.
pub const MAX_LEN: usize = 500;
/// How many lines a room keeps.
const KEEP: usize = 200;

/// One line of the public chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub cert: Signed<DeviceCert>,
    pub name: String,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub time: u64,
    pub nonce: u64,
}

impl Statement for Line {
    const CONTEXT: &'static str = "cadrs public chat 1";
}

fn zero_bits(hash: &[u8; 32]) -> u32 {
    let mut bits = 0;
    for byte in hash {
        if *byte == 0 {
            bits += 8;
        } else {
            return bits + byte.leading_zeros();
        }
    }
    bits
}

fn work(line: &Line) -> u32 {
    let bytes = postcard::to_allocvec(line).expect("lines encode");
    zero_bits(blake3::hash(&bytes).as_bytes())
}

/// Says `text` as `name`: does the work, then signs. `None` for an empty or too long line.
pub fn say(
    key: &SecretKey,
    cert: &Signed<DeviceCert>,
    name: &str,
    text: &str,
    now: u64,
) -> Option<Signed<Line>> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_LEN {
        return None;
    }
    let mut line = Line {
        cert: cert.clone(),
        name: name.chars().take(40).collect(),
        text: text.to_owned(),
        time: now,
        nonce: 0,
    };
    while work(&line) < WORK_BITS {
        line.nonce += 1;
    }
    Some(Signed::new(key, line))
}

/// Why a line was dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    Forged,
    /// Not enough work done.
    Lazy,
    TooLong,
    /// The account said too much this minute.
    TooFast,
    Blocked,
    /// Already heard.
    Again,
    /// Its time is too far off ours.
    OddTime,
}

/// The public chat as one peer hears it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Room {
    lines: VecDeque<Signed<Line>>,
    pub blocked: BTreeSet<AccountId>,
    /// When each account's recent lines were heard (milliseconds), for the rate.
    #[serde(skip)]
    recent: BTreeMap<AccountId, VecDeque<u64>>,
}

impl Room {
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().map(|l| &l.value)
    }

    /// The account a line was said by.
    pub fn account(line: &Line) -> AccountId {
        line.cert.value.account
    }

    /// Takes a line heard (as bytes from the room) at `now`.
    pub fn hear(&mut self, bytes: &[u8], now: u64) -> Result<(), Dropped> {
        let line: Signed<Line> = postcard::from_bytes(bytes).map_err(|_| Dropped::Forged)?;
        self.add(line, now)
    }

    pub fn add(&mut self, line: Signed<Line>, now: u64) -> Result<(), Dropped> {
        let value = line.verify().map_err(|_| Dropped::Forged)?;
        let cert = value.cert.verify().map_err(|_| Dropped::Forged)?;
        if line.signer != cert.device || value.cert.signer != cert.account {
            return Err(Dropped::Forged);
        }
        if value.text.len() > MAX_LEN {
            return Err(Dropped::TooLong);
        }
        if work(value) < WORK_BITS {
            return Err(Dropped::Lazy);
        }
        if value.time > now + 60_000 || value.time + 3_600_000 < now {
            return Err(Dropped::OddTime);
        }
        let account = cert.account;
        if self.blocked.contains(&account) {
            return Err(Dropped::Blocked);
        }
        if self.lines.contains(&line) {
            return Err(Dropped::Again);
        }
        let recent = self.recent.entry(account).or_default();
        while recent.front().is_some_and(|t| *t + 60_000 <= now) {
            recent.pop_front();
        }
        if recent.len() >= PER_MINUTE {
            return Err(Dropped::TooFast);
        }
        recent.push_back(now);
        // In time order (lines can arrive late).
        let at = self
            .lines
            .iter()
            .position(|l| l.value.time > value.time)
            .unwrap_or(self.lines.len());
        self.lines.insert(at, line);
        while self.lines.len() > KEEP {
            self.lines.pop_front();
        }
        Ok(())
    }

    /// Blocks an account: its lines go, and later ones are dropped.
    pub fn block(&mut self, account: AccountId) {
        self.blocked.insert(account);
        self.lines.retain(|l| l.value.cert.value.account != account);
    }
}

/// The signed line, as bytes for the room.
pub fn encode(line: &Signed<Line>) -> Vec<u8> {
    postcard::to_allocvec(line).expect("lines encode")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Account;

    fn speaker() -> (SecretKey, Signed<DeviceCert>) {
        let account = Account::generate();
        let key = SecretKey::generate();
        let cert = account.certify(key.public(), "laptop", 0);
        (key, cert)
    }

    #[test]
    fn a_line_is_heard_and_kept_in_order() {
        let (key, cert) = speaker();
        let mut room = Room::default();
        let late = say(&key, &cert, "Alex", "second", 2_000).unwrap();
        let early = say(&key, &cert, "Alex", "first", 1_000).unwrap();
        room.hear(&encode(&late), 2_000).unwrap();
        room.hear(&encode(&early), 2_000).unwrap();
        let texts: Vec<&str> = room.lines().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["first", "second"]);
        assert_eq!(room.hear(&encode(&early), 2_000), Err(Dropped::Again));
    }

    #[test]
    fn spam_costs_work_and_is_slowed_and_blocked() {
        let (key, cert) = speaker();
        let mut room = Room::default();
        // A line with no work done.
        let lazy = Signed::new(
            &key,
            Line {
                cert: cert.clone(),
                name: "x".into(),
                text: "buy now".into(),
                time: 0,
                nonce: 0,
            },
        );
        if work(&lazy.value) < WORK_BITS {
            assert_eq!(room.add(lazy, 0), Err(Dropped::Lazy));
        }
        // Five a minute, then no more.
        for i in 0..PER_MINUTE {
            let line = say(&key, &cert, "x", &format!("line {i}"), 10).unwrap();
            room.add(line, 10).unwrap();
        }
        let sixth = say(&key, &cert, "x", "line 6", 10).unwrap();
        assert_eq!(room.add(sixth.clone(), 10), Err(Dropped::TooFast));
        // A minute later it may speak again.
        let later = say(&key, &cert, "x", "a minute on", 60_011).unwrap();
        assert_eq!(room.add(later, 60_011), Ok(()));
        // Blocked: gone, and dropped from then on.
        room.block(cert.value.account);
        assert_eq!(room.lines().count(), 0);
        let again = say(&key, &cert, "x", "hello?", 60_020).unwrap();
        assert_eq!(room.add(again, 60_020), Err(Dropped::Blocked));
    }

    #[test]
    fn a_changed_line_is_refused() {
        let (key, cert) = speaker();
        let mut line = say(&key, &cert, "Alex", "hello", 0).unwrap();
        line.value.text = "goodbye".into();
        assert_eq!(Room::default().add(line, 0), Err(Dropped::Forged));
        assert_eq!(say(&key, &cert, "Alex", "  ", 0), None);
    }
}
