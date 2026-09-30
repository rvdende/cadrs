//! Search (PCB3.3): the toolbar's Search field finds boards and components by name.
//!
//! A term matches (case-insensitively, anywhere in the text) a board's name, or a component's
//! designator, package or part number. The hits come in stepping order: the active board first
//! (its name, then its components in designator order), then the other boards in list order.
//! [`SearchState`] steps through them with up/down (wrapping) and shows "n of m".

use super::board::ItemId;
use super::{BoardId, PcbStudio};
use crate::library::natural_cmp;

/// One search result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Board(BoardId),
    Component(BoardId, ItemId),
}

impl Hit {
    pub fn board(self) -> BoardId {
        match self {
            Hit::Board(b) | Hit::Component(b, _) => b,
        }
    }

    pub fn item(self) -> Option<ItemId> {
        match self {
            Hit::Component(_, i) => Some(i),
            Hit::Board(_) => None,
        }
    }
}

/// Every hit of `term` in `studio`, in stepping order (see the module docs), with `active` the
/// board shown. An empty term finds nothing.
pub fn search(studio: &PcbStudio, active: Option<BoardId>, term: &str) -> Vec<Hit> {
    let t = term.trim().to_lowercase();
    if t.is_empty() {
        return vec![];
    }
    let has = |s: &str| s.to_lowercase().contains(&t);
    let mut order: Vec<&super::StudioBoard> = studio.boards.iter().collect();
    order.sort_by_key(|b| Some(b.id) != active);
    let mut out = Vec::new();
    for b in order {
        if has(b.name()) {
            out.push(Hit::Board(b.id));
        }
        let mut comps: Vec<(ItemId, &cadrs_idf::Placement)> =
            b.board.components().filter(|(_, p)| has(&p.refdes) || has(&p.package) || has(&p.part_number)).collect();
        comps.sort_by(|x, y| natural_cmp(&x.1.refdes, &y.1.refdes));
        out.extend(comps.into_iter().map(|(i, _)| Hit::Component(b.id, i)));
    }
    out
}

/// A search in progress: the term, its hits and the current one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchState {
    pub term: String,
    pub hits: Vec<Hit>,
    /// Index into `hits`.
    pub current: usize,
}

impl SearchState {
    /// Runs a search; the first hit is current.
    pub fn run(studio: &PcbStudio, active: Option<BoardId>, term: &str) -> SearchState {
        SearchState { term: term.trim().to_string(), hits: search(studio, active, term), current: 0 }
    }

    pub fn is_active(&self) -> bool {
        !self.term.is_empty()
    }

    pub fn current(&self) -> Option<Hit> {
        self.hits.get(self.current).copied()
    }

    /// Down: the next hit (after the last, the first).
    pub fn step_down(&mut self) -> Option<Hit> {
        if self.hits.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.hits.len();
        self.current()
    }

    /// Up: the previous hit (before the first, the last).
    pub fn step_up(&mut self) -> Option<Hit> {
        if self.hits.is_empty() {
            return None;
        }
        self.current = (self.current + self.hits.len() - 1) % self.hits.len();
        self.current()
    }

    /// "3 of 13", or "No results".
    pub fn counter(&self) -> String {
        if self.hits.is_empty() { "No results".into() } else { format!("{} of {}", self.current + 1, self.hits.len()) }
    }
}
