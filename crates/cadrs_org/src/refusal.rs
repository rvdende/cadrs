//! Why the document refused an operation, for those the app says in the person's own language.
//!
//! A refusal travels as words ([`cadrs_social::log::Outcome::Conflicted`]): every peer gives the
//! same ones, and a log's replay shows them. The words the app translates are constants here,
//! used by the reducer, and [`Refusal::of`] tells which one a message is, so the app matches on
//! a kind and not on a turn of phrase that someone might reword.

/// After the role: "a Member may not do this".
pub const ROLE: &str = "may not do this";
pub const LAST_OWNER: &str = "an organization keeps at least one owner";
pub const LOCATION_IN_USE: &str = "machines stand at this location: move them first";
pub const MACHINE_IN_USE: &str = "jobs are scheduled on this machine";
/// After how many: "only 2 sheets are free".
pub const NO_STOCK: &str = "sheets are free";
pub const NOT_A_MANUFACTURER: &str = "this organization doesn't make parts for others";
pub const BLOCKED: &str = "this organization doesn't take requests from that account";
pub const TOO_MANY_OF_YOURS: &str = "that many requests of yours wait for an answer already";
pub const TOO_MANY_WAITING: &str = "too many requests wait for an answer there already";
pub const TOO_MANY_TODAY: &str = "that many requests were taken today already";
pub const QUOTE_REPLACED: &str = "that quote was replaced by a newer one";
pub const QUOTE_EXPIRED: &str = "the quote had run out";
pub const ISSUE_CLOSE: &str = "closing an issue says what was decided, and why";
pub const TOO_LARGE: &str = "larger than an operation may be";

/// A refusal the app has words of its own for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Role,
    LastOwner,
    LocationInUse,
    MachineInUse,
    NoStock,
    NotAManufacturer,
    Blocked,
    TooManyOfYours,
    TooManyWaiting,
    TooManyToday,
    QuoteReplaced,
    QuoteExpired,
    IssueClose,
    TooLarge,
}

impl Refusal {
    pub const ALL: [(Refusal, &'static str); 14] = [
        (Refusal::Role, ROLE),
        (Refusal::LastOwner, LAST_OWNER),
        (Refusal::LocationInUse, LOCATION_IN_USE),
        (Refusal::MachineInUse, MACHINE_IN_USE),
        (Refusal::NoStock, NO_STOCK),
        (Refusal::NotAManufacturer, NOT_A_MANUFACTURER),
        (Refusal::Blocked, BLOCKED),
        (Refusal::TooManyOfYours, TOO_MANY_OF_YOURS),
        (Refusal::TooManyWaiting, TOO_MANY_WAITING),
        (Refusal::TooManyToday, TOO_MANY_TODAY),
        (Refusal::QuoteReplaced, QUOTE_REPLACED),
        (Refusal::QuoteExpired, QUOTE_EXPIRED),
        (Refusal::IssueClose, ISSUE_CLOSE),
        (Refusal::TooLarge, TOO_LARGE),
    ];

    /// Which refusal `why` is, if it is one of these.
    pub fn of(why: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, words)| why.contains(words))
            .map(|(kind, _)| *kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each message is its own kind and no other's, also inside a longer sentence.
    #[test]
    fn every_refusal_is_told_apart() {
        for (kind, words) in Refusal::ALL {
            assert_eq!(Refusal::of(words), Some(kind));
            assert_eq!(Refusal::of(&format!("only 2 {words}; 5")), Some(kind));
        }
        assert_eq!(Refusal::of("a Member may not do this"), Some(Refusal::Role));
        assert_eq!(Refusal::of("no such request"), None);
    }
}
