//! The trusting bank: no bank at all. Each device keeps the books of its own wallets (the
//! user's, and their organizations'), and tells whoever it pays with a note signed by the
//! device; whoever gets a note believes it and adds it to theirs.
//!
//! It is for test credits, to try buying and selling with. What it doesn't do is what a real
//! ledger is for:
//! - **Nobody checks that the payer had the credits.** A changed cadrs can send notes for
//!   credits it never had, and they are believed.
//! - **Each device has its own idea of a wallet.** Two devices of one account each keep a
//!   balance of their own. (An organization's wallet is the exception: what is kept for it is
//!   kept in the organization's document, which its members' devices share.)
//! - **A note that doesn't arrive is credits that didn't.** cadrs tells the other side when
//!   it can reach them, and again later when it can't.
//!
//! What it does keep to, so that honest use behaves like the real thing: a note is believed
//! only when it gives to a wallet kept here, never when it takes from one; asking twice moves
//! once; a held payment is released by its payer and returned by its payee; and what arrives
//! from strangers in a day has a ceiling ([`MAX_NOTES_A_DAY`]), because every note is kept.

use std::collections::BTreeMap;

use cadrs_social::keys::{DeviceCert, SecretKey, Signed};

use crate::book::{Book, Decision, Effect};
use crate::refusal as why;
use crate::{
    AccountId, Answer, Ask, Asker, Bank, CLAIM_AFTER, Event, HoldState, Kind, MAX_AMOUNT, MAX_NOTE,
    Movement, Ticket,
};

/// How many notes a wallet takes in a day: each one is kept for good.
pub const MAX_NOTES_A_DAY: usize = 500;

const DAY: u64 = 24 * 60 * 60 * 1000;

pub struct Trusting {
    /// This device's key, which signs the notes, and its account's certificate for it.
    device: SecretKey,
    cert: Signed<DeviceCert>,
    books: BTreeMap<AccountId, Book>,
    /// When each wallet took a note, lately.
    taken: BTreeMap<AccountId, Vec<u64>>,
    events: Vec<Event>,
    next: Ticket,
}

impl Trusting {
    pub fn new(device: SecretKey, cert: Signed<DeviceCert>) -> Self {
        Self {
            device,
            cert,
            books: BTreeMap::new(),
            taken: BTreeMap::new(),
            events: Vec::new(),
            next: 1,
        }
    }

    /// The books of a wallet kept here.
    pub fn book(&self, wallet: &AccountId) -> Option<&Book> {
        self.books.get(wallet)
    }

    /// Does `effect` in `wallet`'s books and hands it over to be kept. The movement as the
    /// books now have it (with its place among them), if it was new.
    fn record(&mut self, wallet: AccountId, effect: &Effect) -> Option<Movement> {
        let Effect::Move(movement) = effect else {
            return None;
        };
        let book = self.books.get_mut(&wallet)?;
        if book.movement(&movement.id).is_some() {
            return None;
        }
        book.apply(effect);
        let recorded = book.movement(&movement.id).cloned()?;
        self.events.push(Event::Keep {
            wallet,
            effect: effect.clone(),
        });
        Some(recorded)
    }

    fn answer(
        &mut self,
        by: Asker,
        wallet: AccountId,
        ask: &Ask,
        now: u64,
    ) -> Result<Answer, String> {
        let book = self.books.get(&wallet).ok_or(why::NOT_OURS)?;
        let effect = match book.decide(&wallet, &by, ask, now)? {
            Decision::Statement(statement) => return Ok(Answer::Statement(statement)),
            Decision::Already(movement) => return Ok(Answer::Moved(movement)),
            Decision::Effect(effect) => effect,
        };
        let moved = self.record(wallet, &effect).ok_or(why::ID_USED)?;
        // The other side: told, or, when its wallet is kept here too (the user and their own
        // organization), done there as well.
        let other = if moved.to == wallet {
            moved.from
        } else {
            Some(moved.to)
        };
        if let Some(other) = other.filter(|other| *other != wallet) {
            if self.books.contains_key(&other) {
                if let Some(theirs) = self.record(other, &effect) {
                    self.events.push(Event::Credited {
                        wallet: other,
                        movement: theirs,
                    });
                }
            } else {
                let note = crate::write_note(&self.device, &self.cert, &moved);
                self.events.push(Event::Tell { to: other, note });
            }
        }
        Ok(Answer::Moved(moved))
    }

    /// The wallet kept here that a note's movement is for, when the note is one to believe:
    /// it gives to that wallet (credits, a held payment, a held payment's release or return),
    /// or says its payee took a held payment that had waited its thirty days.
    fn believed(&self, movement: &Movement, now: u64) -> Result<AccountId, &'static str> {
        let ours = |wallet: &AccountId| self.books.contains_key(wallet);
        let from = movement.from.ok_or(why::NOT_BELIEVED)?;
        match movement.kind {
            Kind::Transfer | Kind::Payout | Kind::Held | Kind::Released => {
                if !ours(&movement.to) {
                    return Err(why::NOT_OURS);
                }
                // What happens between two wallets kept here was done here, not told.
                if ours(&from) {
                    return Err(why::NOT_BELIEVED);
                }
                Ok(movement.to)
            }
            Kind::Returned | Kind::Claimed => {
                if !ours(&from) {
                    return Err(why::NOT_OURS);
                }
                if ours(&movement.to) {
                    return Err(why::NOT_BELIEVED);
                }
                if movement.kind == Kind::Claimed {
                    // It takes what was held from this wallet for good: only a hold these
                    // books know, that still waits, for that payee, after its thirty days.
                    let held = movement
                        .hold
                        .and_then(|id| self.books.get(&from)?.hold(&id))
                        .ok_or(why::NO_HOLD)?;
                    if held.state != HoldState::Open || held.to != movement.to {
                        return Err(why::SETTLED);
                    }
                    if now < held.time.saturating_add(CLAIM_AFTER) {
                        return Err(why::TOO_EARLY);
                    }
                }
                Ok(from)
            }
            _ => Err(why::NOT_BELIEVED),
        }
    }
}

impl Bank for Trusting {
    fn ask(&mut self, by: Asker, wallet: AccountId, ask: Ask, now: u64) -> Ticket {
        let ticket = self.next;
        self.next += 1;
        let answer = self.answer(by, wallet, &ask, now);
        self.events.push(Event::Answered(ticket, answer));
        ticket
    }

    fn poll(&mut self, _now: u64) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn receive(&mut self, note: &[u8], now: u64) -> Result<(), String> {
        let (_, movement) = crate::read_note(note)?;
        if movement.amount == 0
            || movement.amount > MAX_AMOUNT
            || movement.note.chars().count() > MAX_NOTE
            || movement.about.as_ref().is_some_and(|about| !about.valid())
        {
            return Err(why::AMOUNT.into());
        }
        let wallet = self.believed(&movement, now)?;
        if self
            .books
            .get(&wallet)
            .is_some_and(|book| book.movement(&movement.id).is_some())
        {
            // Told before: once is enough.
            return Ok(());
        }
        let taken = self.taken.entry(wallet).or_default();
        taken.retain(|at| at.saturating_add(DAY) > now);
        if taken.len() >= MAX_NOTES_A_DAY {
            return Err(why::TOO_MANY_TODAY.into());
        }
        taken.push(now);
        // A release for a hold whose own note never arrived: the hold first, as the release
        // says it was, so that the credits aren't lost with the note.
        if matches!(movement.kind, Kind::Released)
            && let Some(hold) = movement.hold
            && self
                .books
                .get(&wallet)
                .is_some_and(|b| b.hold(&hold).is_none())
        {
            let held = Movement {
                id: hold,
                kind: Kind::Held,
                hold: None,
                ..movement.clone()
            };
            self.record(wallet, &Effect::Move(held));
        }
        if let Some(recorded) = self.record(wallet, &Effect::Move(movement)) {
            self.events.push(Event::Credited {
                wallet,
                movement: recorded,
            });
        }
        Ok(())
    }

    fn restore(&mut self, wallet: AccountId, organization: bool, kept: &[Effect]) {
        let mut book = Book::default();
        if organization {
            book.apply(&Effect::Organization { wallet });
        }
        for effect in kept {
            book.apply(effect);
        }
        self.books.insert(wallet, book);
    }
}
