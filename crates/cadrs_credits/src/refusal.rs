//! Why the bank doesn't do what was asked: a sentence it answers with, and the same as a kind
//! the app can say in the user's language.

pub const NOT_ENOUGH: &str = "not enough credits";
pub const FAUCET_NOT_YET: &str = "the faucet gives once an hour";
pub const FAUCET_PEOPLE: &str = "the faucet is for people's wallets";
pub const NOT_YOURS: &str = "that wallet is not yours to spend from";
pub const NOT_YOURS_TO_SEE: &str = "that wallet is not yours to look at";
pub const AMOUNT: &str = "an amount is 1 to 1,000,000,000 credits";
pub const SAME_WALLET: &str = "a wallet doesn't pay itself";
pub const NOTE: &str = "a note is at most 140 characters";
pub const ID_USED: &str = "that payment's id was used for another";
pub const NO_HOLD: &str = "no such held payment";
pub const SETTLED: &str = "that held payment was settled already";
pub const NOT_PAYER: &str = "only the payer releases a held payment";
pub const NOT_PAYEE: &str = "only the payee returns a held payment";
pub const TOO_EARLY: &str = "a held payment is claimed after thirty days";
pub const TOO_MANY_HELD: &str = "too many payments are held from that wallet";
pub const TOO_MANY_BETWEEN: &str = "too many payments are held between those two wallets";
pub const TOO_MANY_TODAY: &str = "that wallet paid too many times today";
pub const TOO_FULL: &str = "that wallet can't hold more credits";
pub const SIGNATURE: &str = "the note's signature doesn't hold";
pub const TOO_LARGE: &str = "larger than a note may be";
pub const NOT_BELIEVED: &str =
    "a note is believed when it pays this wallet, not when it takes from it";
pub const NOT_OURS: &str = "that note is about no wallet kept here";

/// A refusal's kind, told from its sentence (so the app can put it in the user's language;
/// anything else is shown as the bank said it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotEnough,
    FaucetNotYet,
    FaucetPeople,
    NotYours,
    Amount,
    SameWallet,
    Note,
    NoHold,
    Settled,
    NotPayer,
    NotPayee,
    TooEarly,
    TooMany,
}

impl Refusal {
    pub fn of(why: &str) -> Option<Self> {
        Some(match why.trim() {
            NOT_ENOUGH => Self::NotEnough,
            FAUCET_NOT_YET => Self::FaucetNotYet,
            FAUCET_PEOPLE => Self::FaucetPeople,
            NOT_YOURS | NOT_YOURS_TO_SEE => Self::NotYours,
            AMOUNT => Self::Amount,
            SAME_WALLET => Self::SameWallet,
            NOTE => Self::Note,
            NO_HOLD => Self::NoHold,
            SETTLED => Self::Settled,
            NOT_PAYER => Self::NotPayer,
            NOT_PAYEE => Self::NotPayee,
            TOO_EARLY => Self::TooEarly,
            TOO_MANY_HELD | TOO_MANY_BETWEEN | TOO_MANY_TODAY | TOO_FULL => Self::TooMany,
            _ => return None,
        })
    }
}
