//! Credits: the money cadrs uses until there is real payment. Play money, kept properly.
//!
//! A **bank** ([`bank::Bank`]) keeps every wallet's balance and signs a receipt for every
//! movement. It is a cadrs peer like any other (cadrs's own seed is one): a device asks it over
//! the social connection (`cadrs_social::post::Post::Bank`) and it answers, and tells the
//! devices of whoever was paid as it happens (`Post::Credited`).
//!
//! - A **wallet** belongs to an account: a person's, or an organization's. A person's wallet
//!   is spent from by that person's devices. An organization's is spent from by the accounts
//!   its key names ([`Ask::Access`]): its owners and admins; its other members may look.
//! - The **faucet** gives a person [`FAUCET`] credits, once every [`FAUCET_EVERY`].
//! - A **transfer** moves credits from one wallet to another at once. A **payout** is a
//!   transfer from an organization to a person, called what it is.
//! - A **held payment** takes credits out of the payer's balance for a payee, for an order:
//!   the payer releases them (the goods arrived), or the payee returns them (it won't be
//!   made). Neither can take them alone the other way; a payee may claim a hold nobody
//!   settled after [`CLAIM_AFTER`].
//!
//! What the bank decides is in [`book::Book`], a plain state machine over [`book::Effect`]s;
//! the bank writes each effect down before it counts ([`bank::Journal`]).
//!
//! # What anyone can make the bank do
//! An ask is a few hundred bytes ([`MAX_ASKED`]), signed by the device that sends it, for this
//! bank and this minute ([`CLOCK_SLACK`]); a device asks so many times a minute
//! ([`bank::ASKS_A_MINUTE`]) and a wallet pays so many times a day ([`MAX_MOVES_A_DAY`]).
//! Accounts cost nothing to make, so anyone can farm the faucet with many of them: these are
//! play credits, and that is accepted until they are worth something.
//!
//! # Who keeps the ledger
//! The bank does, and has to be trusted with it: it could refuse someone or make credits.
//! Every movement comes with a receipt it signed, so what it said can be shown to anyone. A
//! ledger no one keeps (peer to peer) can take the bank's place behind the same asks.

pub use cadrs_social::keys::{AccountId, DeviceId};
use cadrs_social::keys::{DeviceCert, Growing, SecretKey, Signed, SignedBytes};
use serde::{Deserialize, Serialize};

pub mod bank;
pub mod book;
pub mod refusal;

pub use refusal::Refusal;

/// An amount of credits. There are no fractions of one.
pub type Credits = u64;

/// What names a movement: chosen by whoever asks for it, so asking twice moves once.
pub type MoveId = [u8; 16];

/// What the faucet gives.
pub const FAUCET: Credits = 1000;
/// How long between two takes from the faucet, in milliseconds: an hour.
pub const FAUCET_EVERY: u64 = 60 * 60 * 1000;
/// The longest note on a movement, in characters.
pub const MAX_NOTE: usize = 140;
/// The most one movement carries.
pub const MAX_AMOUNT: Credits = 1_000_000_000;
/// How far a device's clock may be from the bank's, in milliseconds.
pub const CLOCK_SLACK: u64 = 10 * 60 * 1000;
/// How many movements a statement lists at most.
pub const PAGE: usize = 50;
/// How many accounts may spend from an organization's wallet, and how many may look.
pub const MAX_SPENDERS: usize = 64;
pub const MAX_VIEWERS: usize = 1000;
/// How many payments a wallet may have held at once, as their payer.
pub const MAX_OPEN_HOLDS: usize = 200;
/// How many payments one wallet may have held for one other at once. (What is held *for* a
/// wallet by everyone together has no ceiling: one that had would let anyone with a few
/// credits fill it and stop a shop from being paid.)
pub const MAX_HELD_BETWEEN: usize = 20;
/// How many held payments a statement lists: the largest, so that many small ones from
/// strangers don't push an order's out of sight.
pub const MAX_HOLDS_LISTED: usize = 200;
/// How long a held payment waits for its payer before the payee may claim it, in
/// milliseconds: thirty days.
pub const CLAIM_AFTER: u64 = 30 * 24 * 60 * 60 * 1000;
/// How many movements a wallet may pay in a day.
pub const MAX_MOVES_A_DAY: usize = 2000;
/// The most bytes an ask may be: an organization's longest list of who may spend and look
/// ([`MAX_SPENDERS`], [`MAX_VIEWERS`]) fits, with room.
pub const MAX_ASKED: usize = 60 << 10;
/// How many held payments an ask for a statement may name ([`Ask::Statement`]).
pub const MAX_HOLDS_NAMED: usize = 200;

/// A new random [`MoveId`].
pub fn new_id() -> MoveId {
    cadrs_social::random_bytes()
}

/// What kind of movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// From the faucet.
    Faucet,
    /// From one wallet to another.
    Transfer,
    /// From an organization to a person.
    Payout,
    /// Taken out of the payer's balance and held for the payee.
    Held,
    /// A held payment given to the payee by the payer.
    Released,
    /// A held payment given back to the payer by the payee.
    Returned,
    /// A held payment the payee took after waiting [`CLAIM_AFTER`].
    Claimed,
    /// A kind a later cadrs knows.
    #[serde(other)]
    Other,
}

/// What a movement is for: an order, a design. `kind` and `id` are for cadrs to find it again
/// ("order", the request's id); `label` is for people ("R-0007").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct About {
    pub kind: String,
    pub id: String,
    pub label: String,
}

impl About {
    pub fn order(id: u128, label: impl Into<String>) -> Self {
        Self {
            kind: "order".into(),
            id: format!("{id:032x}"),
            label: label.into(),
        }
    }

    /// The order this is about, if it is about one.
    pub fn order_id(&self) -> Option<u128> {
        (self.kind == "order")
            .then(|| u128::from_str_radix(&self.id, 16).ok())
            .flatten()
    }

    fn valid(&self) -> bool {
        self.kind.chars().count() <= 16
            && self.id.chars().count() <= 64
            && self.label.chars().count() <= 80
    }
}

/// Credits that moved, or were held.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Movement {
    /// Its place among everything the bank did.
    pub seq: u64,
    /// Its id as the bank keeps it ([`scoped_id`] of what its asker called it).
    pub id: MoveId,
    /// When, by the bank's clock, in milliseconds since the Unix epoch.
    pub time: u64,
    pub kind: Kind,
    /// The payer; none from the faucet.
    #[serde(default)]
    pub from: Option<AccountId>,
    /// The payee.
    pub to: AccountId,
    pub amount: Credits,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub about: Option<About>,
    /// Whose doing: the person (an organization's wallet is spent from by its people).
    pub by: AccountId,
    /// The held payment this settles.
    #[serde(default)]
    pub hold: Option<MoveId>,
}

impl Movement {
    /// What the movement did to `wallet`'s balance: credits in (positive) or out.
    pub fn change_for(&self, wallet: &AccountId) -> i128 {
        let amount = i128::from(self.amount);
        let payer = self.from.as_ref() == Some(wallet);
        let payee = &self.to == wallet;
        match self.kind {
            Kind::Faucet if payee => amount,
            Kind::Transfer | Kind::Payout if payer => -amount,
            Kind::Transfer | Kind::Payout if payee => amount,
            Kind::Held if payer => -amount,
            Kind::Released | Kind::Claimed if payee => amount,
            Kind::Returned if payer => amount,
            _ => 0,
        }
    }

    /// The other side of the movement, seen from `wallet`.
    pub fn other(&self, wallet: &AccountId) -> Option<AccountId> {
        if &self.to == wallet {
            self.from
        } else {
            Some(self.to)
        }
    }
}

/// Whether a held payment still waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldState {
    Open,
    Released,
    Returned,
}

/// A payment held for an order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    /// The id of the movement that held it.
    pub id: MoveId,
    pub from: AccountId,
    pub to: AccountId,
    pub amount: Credits,
    #[serde(default)]
    pub about: Option<About>,
    pub time: u64,
    pub state: HoldState,
}

/// What a device asks of the bank about a wallet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ask {
    /// [`FAUCET`] credits, if an hour has passed.
    Faucet { id: MoveId },
    /// The balance and the movements before `before` (the newest when none), newest first.
    /// `holds` names held payments to say of, whatever became of them (an order's, by its own
    /// id): the asker's orders are then never hidden behind other people's.
    Statement {
        #[serde(default)]
        before: Option<u64>,
        #[serde(default)]
        holds: Vec<MoveId>,
    },
    Transfer {
        id: MoveId,
        to: AccountId,
        amount: Credits,
        #[serde(default)]
        note: String,
        #[serde(default)]
        about: Option<About>,
        /// From an organization to a person, as a payout.
        #[serde(default)]
        payout: bool,
    },
    /// Holds credits of this wallet for `to`.
    Hold {
        id: MoveId,
        to: AccountId,
        amount: Credits,
        #[serde(default)]
        note: String,
        #[serde(default)]
        about: Option<About>,
    },
    /// The payer gives a held payment to its payee.
    Release { id: MoveId, hold: MoveId },
    /// The payee gives a held payment back.
    Return { id: MoveId, hold: MoveId },
    /// The payee takes a held payment nobody settled in [`CLAIM_AFTER`].
    Claim { id: MoveId, hold: MoveId },
    /// An organization's key says who may spend from its wallet and who may look. Signed by
    /// that key itself.
    Access {
        spenders: Vec<AccountId>,
        viewers: Vec<AccountId>,
    },
}

/// An ask, as signed: about which wallet, of which bank, when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub bank: AccountId,
    pub wallet: AccountId,
    /// The asker's clock, milliseconds since the Unix epoch.
    pub time: u64,
    pub ask: Ask,
}

impl Growing for Request {
    const CONTEXT: &'static str = "cadrs credits request 1";
}

/// What goes to the bank: the request signed by the device that asks, and the account's
/// certificate for that device (none when the account's own key signs, as an organization's
/// does for [`Ask::Access`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub request: SignedBytes<Request>,
    #[serde(default)]
    pub cert: Option<Signed<DeviceCert>>,
}

/// The bank's word that a movement happened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub bank: AccountId,
    pub movement: Movement,
}

impl Growing for Receipt {
    const CONTEXT: &'static str = "cadrs credits receipt 1";
}

/// A wallet as its owner sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// What can be spent.
    pub balance: Credits,
    /// Held for others, out of this wallet.
    #[serde(default)]
    pub held_out: Credits,
    /// Held for this wallet by others.
    #[serde(default)]
    pub held_in: Credits,
    /// When the faucet gives again (now or before: it does), by the bank's clock; none for a
    /// wallet the faucet doesn't serve (an organization's).
    #[serde(default)]
    pub faucet_at: Option<u64>,
    /// The bank's clock.
    pub time: u64,
    /// Movements, newest first.
    #[serde(default)]
    pub movements: Vec<Movement>,
    /// There are older ones: ask with `before` the last one's `seq`.
    #[serde(default)]
    pub more: bool,
    /// The payments held from or for this wallet that still wait: the largest
    /// [`MAX_HOLDS_LISTED`] of them, largest first.
    #[serde(default)]
    pub holds: Vec<Hold>,
    /// How many wait in all.
    #[serde(default)]
    pub open_holds: u32,
    /// The held payments the ask named that this wallet is a side of, whatever became of them.
    #[serde(default)]
    pub named: Vec<Hold>,
    /// The asker may spend from this wallet (not only look).
    #[serde(default)]
    pub may_spend: bool,
    /// The wallet is an organization's.
    #[serde(default)]
    pub organization: bool,
}

/// The bank's answer to an ask it took.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Answer {
    /// A movement happened (or had already, asked for twice).
    Receipt(SignedBytes<Receipt>),
    Statement(Statement),
    /// Done, with nothing to show ([`Ask::Access`]).
    Done,
}

/// An ask about `wallet` from a device, ready to send to `bank`.
pub fn ask(
    device: &SecretKey,
    cert: &Signed<DeviceCert>,
    bank: AccountId,
    wallet: AccountId,
    ask: Ask,
    now: u64,
) -> Vec<u8> {
    let request = Request {
        bank,
        wallet,
        time: now,
        ask,
    };
    cadrs_social::codec::encode(&Asked {
        request: SignedBytes::new(device, &request),
        cert: Some(cert.clone()),
    })
}

/// An ask about an account's own wallet signed by the account's key (an organization's, for
/// [`Ask::Access`]).
pub fn ask_as(key: &SecretKey, bank: AccountId, ask: Ask, now: u64) -> Vec<u8> {
    let request = Request {
        bank,
        wallet: key.public(),
        time: now,
        ask,
    };
    cadrs_social::codec::encode(&Asked {
        request: SignedBytes::new(key, &request),
        cert: None,
    })
}

/// A receipt's movement, if `bank` signed it.
pub fn open_receipt(receipt: &SignedBytes<Receipt>, bank: &AccountId) -> Result<Movement, String> {
    if &receipt.signer != bank {
        return Err("the receipt is not this bank's".into());
    }
    let receipt = receipt
        .open()
        .map_err(|_| "the receipt doesn't verify".to_string())?;
    if &receipt.bank != bank {
        return Err("the receipt is for another bank".into());
    }
    Ok(receipt.movement)
}

/// Reads the bank's answer, checking a receipt in it is `bank`'s.
pub fn read_answer(bytes: &[u8], bank: &AccountId) -> Result<Answer, String> {
    let answer: Answer = cadrs_social::codec::decode(bytes)
        .map_err(|_| "the bank's answer can't be read".to_string())?;
    if let Answer::Receipt(receipt) = &answer {
        open_receipt(receipt, bank)?;
    }
    Ok(answer)
}

/// Reads what a bank tells a device that was paid (`Post::Credited`): the movement, if `bank`
/// signed it.
pub fn read_credited(bytes: &[u8], bank: &AccountId) -> Result<Movement, String> {
    let receipt: SignedBytes<Receipt> = cadrs_social::codec::decode(bytes)
        .map_err(|_| "the bank's word can't be read".to_string())?;
    open_receipt(&receipt, bank)
}

/// A wallet's address as people pass it on: the account's id, 64 hex digits
/// ([`AccountId`]'s `to_string`). None for anything else.
pub fn parse_address(text: &str) -> Option<AccountId> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(cadrs_social::keys::PublicKey(bytes))
}

/// An id as the bank keeps it: the asker's own id, in the asker's wallet's name. Every wallet
/// has ids of its own this way: nobody can use up an id another wallet will ask with (an
/// order's hold, which both sides can work out), and a held payment is named the same by
/// everyone, by what its payer called it.
pub fn scoped_id(wallet: &AccountId, id: &MoveId) -> MoveId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"cadrs credits id 1\0");
    hasher.update(&wallet.0);
    hasher.update(id);
    let mut scoped = [0; 16];
    scoped.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    scoped
}

/// The hold that pays for an order, as the bank names it: [`order_hold_id`] in the payer's
/// name. Both sides can work it out, to release it or to ask what became of it.
pub fn order_hold(payer: &AccountId, request: u128, quote: u128) -> MoveId {
    scoped_id(payer, &order_hold_id(request, quote))
}

/// The id a payer asks with for the hold that pays for an order: always the same for the same
/// request and quote, so that asking for it again (the answer was lost, the button pressed
/// twice) holds once. The bank names the hold [`order_hold`].
pub fn order_hold_id(request: u128, quote: u128) -> MoveId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"cadrs credits order hold 1\0");
    hasher.update(&request.to_le_bytes());
    hasher.update(&quote.to_le_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    id
}

/// The id of the movement that settles hold `hold` one way (`kind`: release, return, claim):
/// the same every time, so settling is asked for as often as it takes and done once.
pub fn settle_id(hold: &MoveId, kind: Kind) -> MoveId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"cadrs credits settle 1\0");
    hasher.update(hold);
    hasher.update(&[kind as u8]);
    let mut id = [0; 16];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    id
}

/// An amount as people read it: `12,500`.
pub fn show(amount: Credits) -> String {
    let digits = amount.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// An amount typed by a person: digits, with spaces, commas or apostrophes between thousands.
/// None for anything else (a fraction, a sign, nothing).
pub fn parse(text: &str) -> Option<Credits> {
    let digits: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | ',' | '\'' | '\u{a0}' | '\u{202f}'))
        .collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_read_and_show() {
        assert_eq!(show(0), "0");
        assert_eq!(show(999), "999");
        assert_eq!(show(1000), "1,000");
        assert_eq!(show(12_345_678), "12,345,678");
        assert_eq!(parse(" 12,500 "), Some(12_500));
        assert_eq!(parse("1 000"), Some(1000));
        for bad in ["", "-5", "1.5", "ten", "1e3", "99999999999999999999999"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_address_is_an_accounts_id() {
        let account = SecretKey::generate().public();
        assert_eq!(parse_address(&format!(" {account} ")), Some(account));
        assert_eq!(
            parse_address(&account.to_string().to_uppercase()),
            Some(account)
        );
        for bad in ["", "abc", &"g".repeat(64), &"é".repeat(32), &"0".repeat(63)] {
            assert_eq!(parse_address(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_order_is_found_again() {
        let about = About::order(0xabc, "R-0007");
        assert_eq!(about.order_id(), Some(0xabc));
        assert!(about.valid());
        assert_eq!(About::default().order_id(), None);
    }
}
