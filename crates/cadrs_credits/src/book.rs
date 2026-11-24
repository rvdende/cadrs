//! The bank's books: every wallet's balance, as a state machine. [`Book::decide`] says what an
//! ask does (an [`Effect`], or why not); [`Book::apply`] does it. Everything the books hold
//! follows from the effects in order, which is what the bank writes down.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::refusal as why;
use crate::{
    AccountId, Ask, CLAIM_AFTER, Credits, FAUCET, FAUCET_EVERY, Hold, HoldState, Kind, MAX_AMOUNT,
    MAX_HELD_BETWEEN, MAX_HOLDS_LISTED, MAX_MOVES_A_DAY, MAX_NOTE, MAX_OPEN_HOLDS, MAX_SPENDERS,
    MAX_VIEWERS, MoveId, Movement, PAGE, Statement,
};

const DAY: u64 = 24 * 60 * 60 * 1000;

/// Who may spend from an organization's wallet, and who may look, as its key last said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Access {
    pub spenders: Vec<AccountId>,
    pub viewers: Vec<AccountId>,
    /// When the organization said so (its clock): a later list replaces an earlier one.
    pub issued: u64,
}

/// What changes the books.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Move(Movement),
    Access { wallet: AccountId, access: Access },
}

/// What an account may do with a wallet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Nothing,
    Look,
    Spend,
}

#[derive(Clone, Debug, Default)]
struct Wallet {
    balance: Credits,
    held_out: Credits,
    held_in: Credits,
    /// When the faucet last gave.
    faucet: Option<u64>,
    /// The movements it is part of, oldest first (their `seq`).
    history: Vec<u64>,
    /// The held payments from or for it that wait.
    open: Vec<MoveId>,
    /// When it paid, lately (the day's count).
    paid: VecDeque<u64>,
    access: Option<Access>,
}

/// What an ask comes to.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// This changes the books.
    Effect(Effect),
    /// Asked before, and done then: the movement it was.
    Already(Movement),
    /// Nothing to do: what was said is how things are.
    Unchanged,
    Statement(Statement),
}

#[derive(Clone, Debug, Default)]
pub struct Book {
    wallets: BTreeMap<AccountId, Wallet>,
    movements: Vec<Movement>,
    holds: BTreeMap<MoveId, Hold>,
    ids: BTreeMap<MoveId, u64>,
}

impl Book {
    /// How many movements there have been.
    pub fn len(&self) -> usize {
        self.movements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.movements.is_empty()
    }

    pub fn balance(&self, wallet: &AccountId) -> Credits {
        self.wallets.get(wallet).map_or(0, |w| w.balance)
    }

    pub fn hold(&self, id: &MoveId) -> Option<&Hold> {
        self.holds.get(id)
    }

    pub fn movement(&self, id: &MoveId) -> Option<&Movement> {
        self.ids
            .get(id)
            .and_then(|seq| self.movements.get(*seq as usize))
    }

    /// Every credit there is: in balances and held. It equals what the faucet gave.
    pub fn total(&self) -> u128 {
        self.wallets
            .values()
            .map(|w| u128::from(w.balance) + u128::from(w.held_out))
            .sum()
    }

    /// What the faucet gave in all.
    pub fn minted(&self) -> u128 {
        self.movements
            .iter()
            .filter(|m| m.kind == Kind::Faucet)
            .map(|m| u128::from(m.amount))
            .sum()
    }

    /// What `account` may do with `wallet`: its own, or an organization's that names it.
    pub fn may(&self, account: &AccountId, wallet: &AccountId) -> Level {
        if account == wallet {
            return Level::Spend;
        }
        match self.wallets.get(wallet).and_then(|w| w.access.as_ref()) {
            Some(access) if access.spenders.contains(account) => Level::Spend,
            Some(access) if access.viewers.contains(account) => Level::Look,
            _ => Level::Nothing,
        }
    }

    fn is_organization(&self, wallet: &AccountId) -> bool {
        self.wallets.get(wallet).is_some_and(|w| w.access.is_some())
    }

    /// What `ask` about `wallet` by `by` comes to at `now` (the bank's clock). `own_key`: the
    /// wallet's own key signed the ask, not a device of someone's.
    pub fn decide(
        &self,
        wallet: &AccountId,
        by: &AccountId,
        own_key: bool,
        ask: &Ask,
        time: u64,
        now: u64,
    ) -> Result<Decision, &'static str> {
        let level = self.may(by, wallet);
        let spend = || {
            if level == Level::Spend {
                Ok(())
            } else {
                Err(why::NOT_YOURS)
            }
        };
        // Ids are the asking wallet's own: what it calls a movement is kept in its name, so
        // nobody uses up an id another wallet will ask with.
        let movement = |id: &MoveId, kind, from, to, amount, note: &str, about, hold| Movement {
            seq: self.movements.len() as u64,
            id: crate::scoped_id(wallet, id),
            time: now,
            kind,
            from,
            to,
            amount,
            note: note.trim().to_owned(),
            about,
            by: *by,
            hold,
        };
        // Asked twice is done once: the same id for the same thing is answered with what was
        // done; for anything else it is refused.
        let already = |id: &MoveId, kind: &[Kind], payer: Option<&AccountId>| match self
            .movement(&crate::scoped_id(wallet, id))
        {
            // Only a side of the movement gets its receipt this way: knowing an id isn't enough.
            Some(m)
                if kind.contains(&m.kind)
                    && (payer.is_none() || m.from.as_ref() == payer)
                    && (m.from.as_ref() == Some(wallet) || &m.to == wallet) =>
            {
                Some(Ok(Decision::Already(m.clone())))
            }
            Some(_) => Some(Err(why::ID_USED)),
            None => None,
        };
        let effect = |m: Movement| Ok(Decision::Effect(Effect::Move(m)));
        match ask {
            Ask::Statement { before, holds } => {
                if level < Level::Look {
                    return Err(why::NOT_YOURS_TO_SEE);
                }
                let mut statement = self.statement(wallet, level == Level::Spend, *before, now);
                // The held payments it asked after, if this wallet is a side of them.
                statement.named = holds
                    .iter()
                    .take(crate::MAX_HOLDS_NAMED)
                    .filter_map(|id| self.holds.get(id))
                    .filter(|h| &h.from == wallet || &h.to == wallet)
                    .cloned()
                    .collect();
                Ok(Decision::Statement(statement))
            }
            Ask::Faucet { id } => {
                if by != wallet {
                    return Err(why::NOT_YOURS);
                }
                if own_key || self.is_organization(wallet) {
                    return Err(why::FAUCET_PEOPLE);
                }
                if let Some(done) = already(id, &[Kind::Faucet], None) {
                    return done;
                }
                let due = self.faucet_due(wallet);
                if due.is_some_and(|due| due > now) {
                    return Err(why::FAUCET_NOT_YET);
                }
                self.room(wallet, FAUCET)?;
                effect(movement(
                    id,
                    Kind::Faucet,
                    None,
                    *wallet,
                    FAUCET,
                    "",
                    None,
                    None,
                ))
            }
            Ask::Transfer {
                id,
                to,
                amount,
                note,
                about,
                payout,
            } => {
                spend()?;
                let kind = if *payout && self.is_organization(wallet) {
                    Kind::Payout
                } else {
                    Kind::Transfer
                };
                if let Some(done) = already(id, &[Kind::Transfer, Kind::Payout], Some(wallet)) {
                    return done;
                }
                self.payable(wallet, to, *amount, note, about.as_ref(), now)?;
                self.room(to, *amount)?;
                effect(movement(
                    id,
                    kind,
                    Some(*wallet),
                    *to,
                    *amount,
                    note,
                    about.clone(),
                    None,
                ))
            }
            Ask::Hold {
                id,
                to,
                amount,
                note,
                about,
            } => {
                spend()?;
                if let Some(done) = already(id, &[Kind::Held], Some(wallet)) {
                    return done;
                }
                self.payable(wallet, to, *amount, note, about.as_ref(), now)?;
                self.room(to, *amount)?;
                // What a payer has held has ceilings, in all and for any one payee. What is
                // held *for* a wallet has none: nobody fills a shop's.
                let mine = self.wallets.get(wallet).map_or(&[][..], |w| &w.open[..]);
                let from_here =
                    |id: &&MoveId| self.holds.get(*id).is_some_and(|h| &h.from == wallet);
                if mine.iter().filter(from_here).count() >= MAX_OPEN_HOLDS {
                    return Err(why::TOO_MANY_HELD);
                }
                let between = mine
                    .iter()
                    .filter(|id| {
                        self.holds
                            .get(*id)
                            .is_some_and(|h| &h.from == wallet && &h.to == to)
                    })
                    .count();
                if between >= MAX_HELD_BETWEEN {
                    return Err(why::TOO_MANY_BETWEEN);
                }
                effect(movement(
                    id,
                    Kind::Held,
                    Some(*wallet),
                    *to,
                    *amount,
                    note,
                    about.clone(),
                    None,
                ))
            }
            Ask::Release { id, hold } | Ask::Return { id, hold } | Ask::Claim { id, hold } => {
                spend()?;
                let kind = match ask {
                    Ask::Release { .. } => Kind::Released,
                    Ask::Return { .. } => Kind::Returned,
                    _ => Kind::Claimed,
                };
                if let Some(done) = already(id, &[kind], None) {
                    return done;
                }
                let held = self.holds.get(hold).ok_or(why::NO_HOLD)?;
                // The payer gives; the payee gives back, or takes after a long wait.
                match kind {
                    Kind::Released if &held.from != wallet => return Err(why::NOT_PAYER),
                    Kind::Returned | Kind::Claimed if &held.to != wallet => {
                        return Err(why::NOT_PAYEE);
                    }
                    _ => {}
                }
                if held.state != HoldState::Open {
                    return Err(why::SETTLED);
                }
                if kind == Kind::Claimed && now < held.time.saturating_add(CLAIM_AFTER) {
                    return Err(why::TOO_EARLY);
                }
                effect(movement(
                    id,
                    kind,
                    Some(held.from),
                    held.to,
                    held.amount,
                    "",
                    held.about.clone(),
                    Some(*hold),
                ))
            }
            Ask::Access { spenders, viewers } => {
                if !own_key || by != wallet {
                    return Err(why::ACCESS);
                }
                if spenders.len() > MAX_SPENDERS || viewers.len() > MAX_VIEWERS {
                    return Err(why::ACCESS);
                }
                let known = self.wallets.get(wallet).and_then(|w| w.access.as_ref());
                if known.is_some_and(|known| known.issued > time) {
                    return Err(why::OLD_ACCESS);
                }
                let mut access = Access {
                    spenders: spenders.clone(),
                    viewers: viewers.clone(),
                    issued: time,
                };
                access.spenders.sort();
                access.spenders.dedup();
                access.viewers.sort();
                access.viewers.dedup();
                // The same list again (said at every start) is not written down again.
                if known.is_some_and(|known| {
                    known.spenders == access.spenders && known.viewers == access.viewers
                }) {
                    return Ok(Decision::Unchanged);
                }
                Ok(Decision::Effect(Effect::Access {
                    wallet: *wallet,
                    access,
                }))
            }
        }
    }

    /// When the faucet next gives to `wallet`; none if it never gave.
    fn faucet_due(&self, wallet: &AccountId) -> Option<u64> {
        let last = self.wallets.get(wallet)?.faucet?;
        Some(last.saturating_add(FAUCET_EVERY))
    }

    /// A wallet's balance has a ceiling, so that no sum of them overflows.
    fn room(&self, wallet: &AccountId, amount: Credits) -> Result<(), &'static str> {
        let Some(w) = self.wallets.get(wallet) else {
            return Ok(());
        };
        let all = u128::from(w.balance) + u128::from(w.held_in) + u128::from(amount);
        if all > u128::from(MAX_AMOUNT) * 1000 {
            return Err(why::TOO_FULL);
        }
        Ok(())
    }

    fn payable(
        &self,
        wallet: &AccountId,
        to: &AccountId,
        amount: Credits,
        note: &str,
        about: Option<&crate::About>,
        now: u64,
    ) -> Result<(), &'static str> {
        if amount == 0 || amount > MAX_AMOUNT {
            return Err(why::AMOUNT);
        }
        if to == wallet {
            return Err(why::SAME_WALLET);
        }
        if note.trim().chars().count() > MAX_NOTE || about.is_some_and(|a| !a.valid()) {
            return Err(why::NOTE);
        }
        let from = self.wallets.get(wallet);
        if from.map_or(0, |w| w.balance) < amount {
            return Err(why::NOT_ENOUGH);
        }
        let today = from.map_or(0, |w| {
            w.paid
                .iter()
                .filter(|t| t.saturating_add(DAY) > now)
                .count()
        });
        if today >= MAX_MOVES_A_DAY {
            return Err(why::TOO_MANY_TODAY);
        }
        Ok(())
    }

    /// `wallet` as its owner sees it: the balance and a page of movements, newest first.
    pub fn statement(
        &self,
        wallet: &AccountId,
        may_spend: bool,
        before: Option<u64>,
        now: u64,
    ) -> Statement {
        let Some(w) = self.wallets.get(wallet) else {
            return Statement {
                faucet_at: Some(now),
                time: now,
                may_spend,
                ..Statement::default()
            };
        };
        let end = match before {
            Some(before) => w.history.partition_point(|seq| *seq < before),
            None => w.history.len(),
        };
        let start = end.saturating_sub(PAGE);
        let organization = w.access.is_some();
        Statement {
            balance: w.balance,
            held_out: w.held_out,
            held_in: w.held_in,
            faucet_at: (!organization).then(|| w.faucet.map_or(now, |t| t + FAUCET_EVERY)),
            time: now,
            movements: w.history[start..end]
                .iter()
                .rev()
                .filter_map(|seq| self.movements.get(*seq as usize).cloned())
                .collect(),
            more: start > 0,
            holds: {
                // The largest first, and only so many: an order's payment isn't pushed out
                // of the list by strangers' single credits.
                let mut open: Vec<&Hold> =
                    w.open.iter().filter_map(|id| self.holds.get(id)).collect();
                open.sort_by(|a, b| b.amount.cmp(&a.amount).then(b.time.cmp(&a.time)));
                open.into_iter().take(MAX_HOLDS_LISTED).cloned().collect()
            },
            open_holds: w.open.len() as u32,
            named: Vec::new(),
            may_spend,
            organization,
        }
    }

    /// Does `effect`. What [`Book::decide`] gave always applies; one read back from what the
    /// bank wrote does too. Sums never wrap: an effect that couldn't have been decided leaves
    /// a balance at its floor or ceiling rather than a wrong one elsewhere.
    pub fn apply(&mut self, effect: &Effect) {
        let m = match effect {
            Effect::Access { wallet, access } => {
                self.wallets.entry(*wallet).or_default().access = Some(access.clone());
                return;
            }
            Effect::Move(m) => m,
        };
        if self.ids.contains_key(&m.id) {
            return;
        }
        let seq = self.movements.len() as u64;
        let amount = m.amount;
        let to = m.to;
        match (m.kind, m.from) {
            (Kind::Faucet, _) => {
                let w = self.wallets.entry(to).or_default();
                w.balance = w.balance.saturating_add(amount);
                w.faucet = Some(m.time);
            }
            (Kind::Transfer | Kind::Payout, Some(from)) => {
                let w = self.wallets.entry(from).or_default();
                w.balance = w.balance.saturating_sub(amount);
                w.paid.push_back(m.time);
                let w = self.wallets.entry(to).or_default();
                w.balance = w.balance.saturating_add(amount);
            }
            (Kind::Held, Some(from)) => {
                let w = self.wallets.entry(from).or_default();
                w.balance = w.balance.saturating_sub(amount);
                w.held_out = w.held_out.saturating_add(amount);
                w.paid.push_back(m.time);
                w.open.push(m.id);
                let w = self.wallets.entry(to).or_default();
                w.held_in = w.held_in.saturating_add(amount);
                w.open.push(m.id);
                self.holds.insert(
                    m.id,
                    Hold {
                        id: m.id,
                        from,
                        to,
                        amount,
                        about: m.about.clone(),
                        time: m.time,
                        state: HoldState::Open,
                    },
                );
            }
            (Kind::Released | Kind::Claimed | Kind::Returned, Some(from)) => {
                let Some(held) = m.hold.and_then(|id| self.holds.get_mut(&id)) else {
                    return;
                };
                if held.state != HoldState::Open {
                    return;
                }
                let back = m.kind == Kind::Returned;
                held.state = if back {
                    HoldState::Returned
                } else {
                    HoldState::Released
                };
                let id = held.id;
                let w = self.wallets.entry(from).or_default();
                w.held_out = w.held_out.saturating_sub(amount);
                w.open.retain(|open| open != &id);
                if back {
                    w.balance = w.balance.saturating_add(amount);
                }
                let w = self.wallets.entry(to).or_default();
                w.held_in = w.held_in.saturating_sub(amount);
                w.open.retain(|open| open != &id);
                if !back {
                    w.balance = w.balance.saturating_add(amount);
                }
            }
            _ => return,
        }
        // The day's count keeps a day.
        for wallet in m.from.iter().chain([&to]) {
            if let Some(w) = self.wallets.get_mut(wallet) {
                while w
                    .paid
                    .front()
                    .is_some_and(|t| t.saturating_add(DAY) <= m.time)
                {
                    w.paid.pop_front();
                }
                if w.history.last() != Some(&seq) {
                    w.history.push(seq);
                }
            }
        }
        self.ids.insert(m.id, seq);
        self.movements.push(Movement { seq, ..m.clone() });
    }
}
