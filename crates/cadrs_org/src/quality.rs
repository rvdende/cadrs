//! Quality: issues.
//!
//! An [`Issue`] is the one record for anything that went wrong or might: a part out of
//! tolerance, a machine cutting badly, a delivery a client sent back. Anyone in the
//! organization opens one, about the order, job, part, machine or location it concerns; it has
//! an owner, and it is closed with a [`Disposition`] and the reasoning behind it, so that later
//! someone can see not only what was decided but why. (The shape follows how SpaceX's and
//! Tesla's own systems are described: one ticket anyone near the hardware can open, an owner,
//! a disposition, a written rationale.)

use cadrs_social::keys::AccountId;
use serde::{Deserialize, Serialize};

use crate::Id;

/// The longest title, in characters.
pub const MAX_TITLE: usize = 120;

/// What an issue is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueAbout {
    /// A request for a quote, or the order it became.
    Request(Id),
    Job(Id),
    Part(Id),
    Machine(Id),
    Location(Id),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IssueStatus {
    #[default]
    Open,
    InProgress,
    Closed,
}

/// What was decided about the thing the issue is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    /// Made right by the normal process, done again.
    Rework,
    /// Made usable by a repair that isn't the normal process.
    Repair,
    /// Accepted as it is.
    UseAsIs,
    Scrap,
    ReturnToSupplier,
    /// Nothing was wrong after all.
    NoFault,
}

impl Disposition {
    pub const ALL: [Disposition; 6] = [
        Disposition::Rework,
        Disposition::Repair,
        Disposition::UseAsIs,
        Disposition::Scrap,
        Disposition::ReturnToSupplier,
        Disposition::NoFault,
    ];
}

/// Something that went wrong, who looks after it, and what was decided.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// Its number in the organization, from 1, in the order the issues were opened.
    pub number: u32,
    pub title: String,
    pub details: String,
    pub about: Option<IssueAbout>,
    pub status: IssueStatus,
    pub owner: Option<AccountId>,
    pub disposition: Option<Disposition>,
    /// Why that disposition: required to close.
    pub rationale: String,
    pub opened_by: AccountId,
    /// Milliseconds since the Unix epoch.
    pub opened: u64,
    pub closed: Option<u64>,
}
