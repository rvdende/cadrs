//! Credits in the app: the user's wallet and their organizations', as the bank says they are
//! (`cadrs_credits`). The bank is a cadrs peer (cadrs's own seed, unless `CADRS_BANK` names
//! another): this module asks it, keeps what it answered for the interface to show, and takes
//! what it tells this device as it happens (someone paid you).
//!
//! - [`refresh`] asks for a wallet's statement; [`faucet`], [`pay`], [`hold`], [`release`] and
//!   [`give_back`] move credits. Each says what happened in a toast, and [`Credits::version`]
//!   counts up so what shows a wallet draws it again.
//! - `CADRS_BANK=<cadrs ID>` asks another bank, `CADRS_BANK=here` keeps one in this app (in
//!   memory: a scenario's, a test's), `CADRS_BANK=0` none.
//! - An organization's wallet is spent from by its owners and admins: the device that holds
//!   the organization's key tells the bank who they are ([`say_access`]) whenever that changes.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use cadrs_credits::{About, Answer, Ask, Credits as Amount, Movement, Refusal, Statement};
use cadrs_net::bevy_iroh::iroh::EndpointId;
use cadrs_net::bevy_iroh::{Iroh, IrohTask};
use cadrs_social::keys::{AccountId, PublicKey};
use cadrs_social::post::Post;
use cadrs_ui::i18n::{tr, tr_with};

use crate::notifications::{Notice, Notifications};
use crate::social::Social;

/// How often the wallets on screen are asked for again, in seconds (the bank tells a device of
/// each movement as it happens; this is for the ones that didn't arrive).
const REFRESH_EVERY: f64 = 60.0;
/// How often the user's wallet and their organizations' are asked for while nothing shows
/// them, in seconds: often enough that the bank keeps telling this device of what is paid.
const BACKGROUND_EVERY: f64 = 300.0;

pub struct CreditsPlugin;

impl Plugin for CreditsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Credits>()
            .add_systems(Update, (poll, keep_fresh).chain());
    }
}

/// The bank this app asks.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankAt {
    pub id: AccountId,
    /// This app keeps it itself ([`cadrs_net::bank::Hosted`]).
    pub here: bool,
}

/// A wallet as last heard from the bank.
#[derive(Default)]
pub struct Wallet {
    pub statement: Option<Statement>,
    /// Its movements, newest first: every page asked for so far.
    pub movements: Vec<Movement>,
    /// The bank has older ones.
    pub more: bool,
    /// Why the last ask about it failed.
    pub error: Option<String>,
    /// An ask about it is on its way.
    pub asking: bool,
    /// When the statement arrived, by this device's clock (milliseconds since the Unix epoch):
    /// the bank's clock since then is the statement's plus what passed here.
    pub heard_ms: u64,
    asked_at: Option<f64>,
}

impl Wallet {
    pub fn balance(&self) -> Option<Amount> {
        self.statement.as_ref().map(|s| s.balance)
    }
}

/// What to do once the bank answered (or didn't).
pub type Then = Box<dyn FnOnce(&mut World, Result<&Answer, &str>) + Send + Sync>;

struct Asking {
    wallet: AccountId,
    older: bool,
    task: IrohTask<Result<Vec<u8>, String>>,
    then: Option<Then>,
}

#[derive(Resource, Default)]
pub struct Credits {
    pub wallets: BTreeMap<AccountId, Wallet>,
    /// Counts up whenever anything here changed.
    pub version: u32,
    /// The wallets on screen now (the open wallet dialog's, an open organization's).
    watched: BTreeSet<AccountId>,
    asking: Vec<Asking>,
    /// What each organization's key last told the bank: who spends, who looks.
    access_said: BTreeMap<AccountId, (Vec<AccountId>, Vec<AccountId>)>,
    /// The organizations whose list was said (or tried) since this run started.
    access_tried: BTreeSet<AccountId>,
    /// The held payments each wallet's statement is asked to say of: its orders' own.
    named: BTreeMap<AccountId, Vec<cadrs_credits::MoveId>>,
}

impl Credits {
    pub fn wallet(&self, wallet: &AccountId) -> Option<&Wallet> {
        self.wallets.get(wallet)
    }

    pub fn balance(&self, wallet: &AccountId) -> Option<Amount> {
        self.wallets.get(wallet).and_then(Wallet::balance)
    }

    fn changed(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

/// The user's own account.
pub fn me(world: &World) -> Option<AccountId> {
    world
        .get_resource::<Social>()?
        .keys()
        .map(|k| k.account_id())
}

/// The bank to ask: what `CADRS_BANK` says, else cadrs's own seed. None when there is none.
pub fn bank(world: &mut World) -> Option<BankAt> {
    if let Some(at) = world.get_resource::<BankAt>() {
        return Some(*at);
    }
    let named = std::env::var("CADRS_BANK").ok();
    let at = match named.as_deref().map(str::trim) {
        Some("0") => return None,
        Some("here") => return keep_here(world),
        Some(id) if !id.is_empty() => bank_id(id)?,
        _ => bank_id(cadrs_net::public::DEFAULT_SEEDS.first()?)?,
    };
    let at = BankAt {
        id: at,
        here: false,
    };
    world.insert_resource(at);
    Some(at)
}

fn bank_id(id: &str) -> Option<AccountId> {
    id.parse::<EndpointId>()
        .ok()
        .map(|id| PublicKey(*id.as_bytes()))
}

/// Keeps a bank in this app, in memory, and asks that one: a scenario's, a test's. Its key is
/// made for the run.
pub fn keep_here(world: &mut World) -> Option<BankAt> {
    if !world.contains_resource::<cadrs_net::bank::Hosted>() {
        let key = cadrs_social::keys::SecretKey::generate();
        cadrs_net::bank::host(world, key, None).ok()?;
    }
    let id = world.resource::<cadrs_net::bank::Hosted>().0.id();
    let at = BankAt { id, here: true };
    world.insert_resource(at);
    Some(at)
}

/// A refusal in the user's language, when it is one of the bank's own; else as it was said.
pub fn reason(error: &str) -> String {
    let why = cadrs_social::post::refusal(error).unwrap_or(error);
    let key = match Refusal::of(why) {
        Some(Refusal::NotEnough) => "credits-refused-not-enough",
        Some(Refusal::FaucetNotYet) => "credits-refused-faucet-not-yet",
        Some(Refusal::FaucetPeople) => "credits-refused-faucet-people",
        Some(Refusal::NotYours) => "credits-refused-not-yours",
        Some(Refusal::Amount) => "credits-refused-amount",
        Some(Refusal::SameWallet) => "credits-refused-same-wallet",
        Some(Refusal::Note) => "credits-refused-note",
        Some(Refusal::NoHold) => "credits-refused-no-hold",
        Some(Refusal::Settled) => "credits-refused-settled",
        Some(Refusal::NotPayer) => "credits-refused-not-payer",
        Some(Refusal::NotPayee) => "credits-refused-not-payee",
        Some(Refusal::TooEarly) => "credits-refused-too-early",
        Some(Refusal::TooMany) => "credits-refused-too-many",
        Some(Refusal::Clock) => "credits-refused-clock",
        Some(Refusal::TooOften) => "credits-refused-too-often",
        None if cadrs_social::post::refusal(error).is_some() => return why.to_owned(),
        // Not reached at all.
        None => return tr_with("credits-bank-unreachable", &[("why", why)]),
    };
    tr(key)
}

/// Asks the bank `ask` about `wallet`, as this device. False when it couldn't be asked (no
/// keys, no bank, offline), with `then` told why.
pub fn ask(world: &mut World, wallet: AccountId, ask: Ask, then: Option<Then>) -> bool {
    crate::social::ensure_loaded(world);
    let fail = |world: &mut World, then: Option<Then>, why: String| {
        if let Some(w) = world.resource_mut::<Credits>().wallets.get_mut(&wallet) {
            w.error = Some(why.clone());
        }
        world.resource_mut::<Credits>().changed();
        if let Some(then) = then {
            then(world, Err(&why));
        }
        false
    };
    let Some(at) = bank(world) else {
        return fail(world, then, tr("credits-no-bank"));
    };
    let Some(bytes) = world.resource::<Social>().keys().map(|keys| {
        let now = cadrs_social::wall_millis();
        cadrs_credits::ask(&keys.device, &keys.cert, at.id, wallet, ask.clone(), now)
    }) else {
        return fail(world, then, tr("social-no-keys"));
    };
    let older = matches!(
        ask,
        Ask::Statement {
            before: Some(_),
            ..
        }
    );
    if at.here {
        let answer = cadrs_net::bank::answer(world, &bytes, None);
        took(world, wallet, older, answer, then);
        return true;
    }
    if !world.contains_resource::<Iroh>() && !crate::network::connect(world) {
        return fail(world, then, tr("credits-offline"));
    }
    let Some(iroh) = world.get_resource::<Iroh>() else {
        return fail(world, then, tr("credits-offline"));
    };
    let Ok(to) = EndpointId::from_bytes(&at.id.0) else {
        return fail(world, then, tr("credits-no-bank"));
    };
    // A known address (a bank on this network), else the bare key for discovery to find.
    let addr = world
        .get_resource::<crate::social::PeerAddrs>()
        .and_then(|hints| hints.0.get(&to).cloned())
        .unwrap_or_else(|| to.into());
    let task = cadrs_net::social::start_send(iroh, addr, Post::Bank(bytes).encode());
    let mut credits = world.resource_mut::<Credits>();
    let now_asking = credits.wallets.entry(wallet).or_default();
    now_asking.asking = true;
    // Only a wallet that shows nothing yet says that it is being asked for.
    let news = now_asking.statement.is_none();
    credits.asking.push(Asking {
        wallet,
        older,
        task,
        then,
    });
    if news {
        credits.changed();
    }
    true
}

/// The bank's answer to an ask about `wallet` (or why there is none) is taken in.
fn took(
    world: &mut World,
    wallet: AccountId,
    older: bool,
    answer: Result<Vec<u8>, String>,
    then: Option<Then>,
) {
    let at = world.get_resource::<BankAt>().copied();
    let answer = answer.and_then(|bytes| match at {
        Some(at) => cadrs_credits::read_answer(&bytes, &at.id),
        None => Err(tr("credits-no-bank")),
    });
    let mut again = Vec::new();
    {
        let mut credits = world.resource_mut::<Credits>();
        let w = credits.wallets.entry(wallet).or_default();
        w.asking = false;
        // What it showed before: the same again (a wallet asked for every minute) is no news,
        // and nothing that shows it is drawn again.
        let shown = |w: &Wallet| {
            (
                w.statement.as_ref().map(|s| Statement {
                    time: 0,
                    faucet_at: s.faucet_at.map(|at| at.saturating_sub(s.time)),
                    ..s.clone()
                }),
                w.movements.clone(),
                w.more,
                w.error.clone(),
            )
        };
        let before = shown(w);
        let had_none = w.statement.is_none();
        match &answer {
            Ok(Answer::Statement(statement)) => {
                w.error = None;
                if older {
                    let known: BTreeSet<u64> = w.movements.iter().map(|m| m.seq).collect();
                    w.movements.extend(
                        statement
                            .movements
                            .iter()
                            .filter(|m| !known.contains(&m.seq))
                            .cloned(),
                    );
                    w.more = statement.more;
                } else {
                    // The newest page, in front of the older ones already asked for.
                    let oldest = statement.movements.last().map(|m| m.seq);
                    let kept: Vec<Movement> = w
                        .movements
                        .drain(..)
                        .filter(|m| oldest.is_some_and(|oldest| m.seq < oldest))
                        .collect();
                    if kept.is_empty() {
                        w.more = statement.more;
                    }
                    w.movements = statement.movements.clone();
                    w.movements.extend(kept);
                    w.heard_ms = cadrs_social::wall_millis();
                    w.statement = Some(Statement {
                        movements: Vec::new(),
                        ..statement.clone()
                    });
                }
            }
            Ok(Answer::Receipt(receipt)) => {
                w.error = None;
                // Both sides' wallets, if this device shows them, are asked for again.
                if let Ok(receipt) = receipt.open() {
                    again.extend(receipt.movement.from);
                    again.push(receipt.movement.to);
                }
            }
            Ok(Answer::Done) => w.error = None,
            Err(why) => w.error = Some(reason(why)),
        }
        // The faucet's wait is compared as time left, to the minute: the bank's clock moved on.
        let minute = |s: (Option<Statement>, Vec<Movement>, bool, Option<String>)| {
            let wait =
                s.0.as_ref()
                    .and_then(|s| s.faucet_at)
                    .map(|ms| ms.div_ceil(60_000));
            (
                s.0.map(|s| Statement {
                    faucet_at: None,
                    ..s
                }),
                wait,
                s.1,
                s.2,
                s.3,
            )
        };
        if had_none || minute(before) != minute(shown(w)) {
            credits.changed();
        }
    }
    if let Some(then) = then {
        then(world, answer.as_ref().map_err(String::as_str));
    }
    for wallet in again {
        if shown(world, &wallet) {
            refresh(world, wallet);
        }
    }
}

/// Whether `wallet` is one this device keeps: the user's own, one of their organizations', or
/// one on screen.
fn shown(world: &World, wallet: &AccountId) -> bool {
    me(world).as_ref() == Some(wallet)
        || world.resource::<Credits>().watched.contains(wallet)
        || is_my_org(world, wallet)
}

fn poll(world: &mut World) {
    if world.resource::<Credits>().asking.is_empty() {
        return;
    }
    let mut done = Vec::new();
    world
        .resource_mut::<Credits>()
        .asking
        .retain_mut(|a| match a.task.try_take() {
            Some(answer) => {
                done.push((a.wallet, a.older, answer, a.then.take()));
                false
            }
            None => true,
        });
    for (wallet, older, answer, then) in done {
        took(world, wallet, older, answer, then);
    }
}

/// Asks for `wallet`'s statement: its balance and newest movements.
pub fn refresh(world: &mut World, wallet: AccountId) {
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    world
        .resource_mut::<Credits>()
        .wallets
        .entry(wallet)
        .or_default()
        .asked_at = Some(now);
    let holds = world
        .resource::<Credits>()
        .named
        .get(&wallet)
        .cloned()
        .unwrap_or_default();
    let statement = Ask::Statement {
        before: None,
        holds,
    };
    ask(world, wallet, statement, None);
}

/// Asks for the movements older than those shown.
pub fn older(world: &mut World, wallet: AccountId) {
    let before = world
        .resource::<Credits>()
        .wallet(&wallet)
        .and_then(|w| w.movements.last().map(|m| m.seq));
    if before.is_some() {
        let older = Ask::Statement {
            before,
            holds: Vec::new(),
        };
        ask(world, wallet, older, None);
    }
}

/// The held payments `wallet`'s statements are to say of: its orders' own holds, by the ids
/// both sides can work out ([`cadrs_credits::order_hold`]). Asked after by name, an order's
/// payment is found however many other payments are held for the wallet. The newest of them,
/// if there are more than an ask may name; asked for at once when the list grew.
pub fn name_holds(world: &mut World, wallet: AccountId, mut holds: Vec<cadrs_credits::MoveId>) {
    holds.truncate(cadrs_credits::MAX_HOLDS_NAMED);
    let mut credits = world.resource_mut::<Credits>();
    let known = credits.named.entry(wallet).or_default();
    let news = holds.iter().any(|id| !known.contains(id));
    *known = holds;
    if news && credits.wallets.contains_key(&wallet) {
        refresh(world, wallet);
    }
}

/// `wallet` is on screen: asked for now, and kept fresh while it is.
pub fn watch(world: &mut World, wallet: AccountId) {
    if world.resource_mut::<Credits>().watched.insert(wallet) {
        refresh(world, wallet);
    }
}

/// `wallet` is no longer on screen.
pub fn unwatch(world: &mut World, wallet: &AccountId) {
    world.resource_mut::<Credits>().watched.remove(wallet);
}

/// The user's wallet, and those something shows, are asked for again now and then, and at once
/// when first wanted.
fn keep_fresh(world: &mut World, mut last: Local<Option<f64>>) {
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if last.is_some_and(|t| now - t < 1.0) {
        return;
    }
    *last = Some(now);
    // Nothing is asked before the user has keys (made when the account menu is first opened,
    // or when the first letter arrives).
    let Some(me) = me(world) else {
        return;
    };
    // An organization's wallet is on screen only while the organization is open.
    let open = world
        .get_resource::<crate::organization::OpenOrg>()
        .and_then(|open| open.session.org().org);
    world
        .resource_mut::<Credits>()
        .watched
        .retain(|w| *w == me || Some(*w) == open);
    // A bank across the network is asked in the background only while online; one kept here,
    // only for what is on screen.
    let Some(at) = bank(world) else {
        return;
    };
    let background = !at.here && world.contains_resource::<Iroh>();
    // The organizations whose key this device holds: the bank is told who may spend from each
    // once a run (and whenever its members change), so a list said while the bank was away,
    // or to a bank that lost it, is said again.
    if background || at.here {
        let unsaid: Vec<crate::social::MyOrg> = world
            .resource::<Social>()
            .orgs
            .iter()
            .filter(|o| {
                !world
                    .resource::<Credits>()
                    .access_tried
                    .contains(&o.account)
            })
            .cloned()
            .collect();
        for mine in unsaid {
            world
                .resource_mut::<Credits>()
                .access_tried
                .insert(mine.account);
            if let Some(org) = crate::social::org_state(world, &mine) {
                say_access(world, mine.account, &org);
            }
        }
    }
    let credits = world.resource::<Credits>();
    let mine: Vec<AccountId> = world
        .resource::<Social>()
        .orgs
        .iter()
        .map(|o| o.account)
        .chain([me])
        .collect();
    let due: Vec<AccountId> = mine
        .iter()
        .copied()
        .filter(|_| background)
        .chain(credits.watched.iter().copied())
        .filter(|w| {
            let every = if credits.watched.contains(w) {
                REFRESH_EVERY
            } else {
                BACKGROUND_EVERY
            };
            credits
                .wallet(w)
                .is_none_or(|w| !w.asking && w.asked_at.is_none_or(|t| now - t >= every))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for wallet in due {
        refresh(world, wallet);
    }
}

/// Says what a movement asked for came to: `done` in a toast, or why not.
fn say(done: String) -> Then {
    Box::new(
        move |world: &mut World, answer: Result<&Answer, &str>| match answer {
            Ok(_) => crate::linked::toast(world, done),
            Err(why) => {
                let why = reason(why);
                crate::linked::toast(world, why);
            }
        },
    )
}

/// Takes from the faucet, for the user.
pub fn faucet(world: &mut World) {
    crate::social::ensure_loaded(world);
    let Some(me) = me(world) else {
        crate::linked::toast(world, tr("social-no-keys"));
        return;
    };
    let done = tr_with(
        "credits-faucet-took",
        &[("amount", &cadrs_credits::show(cadrs_credits::FAUCET))],
    );
    let id = cadrs_credits::new_id();
    ask(world, me, Ask::Faucet { id }, Some(say(done)));
}

/// A payment to make: from which wallet, to whom, how much, and what for.
#[derive(Clone, Debug)]
pub struct Payment {
    pub from: AccountId,
    pub to: AccountId,
    /// The payee's name, for what is said.
    pub to_name: String,
    pub amount: Amount,
    pub note: String,
    pub about: Option<About>,
    /// From an organization to a person, as a payout.
    pub payout: bool,
}

/// Pays: the credits move at once. `then` hears how it went, after the toast.
pub fn pay(world: &mut World, payment: Payment, then: Option<Then>) -> bool {
    let done = tr_with(
        if payment.payout {
            "credits-paid-out"
        } else {
            "credits-sent"
        },
        &[
            ("amount", &cadrs_credits::show(payment.amount)),
            ("name", &payment.to_name),
        ],
    );
    let transfer = Ask::Transfer {
        id: cadrs_credits::new_id(),
        to: payment.to,
        amount: payment.amount,
        note: payment.note,
        about: payment.about,
        payout: payment.payout,
    };
    ask(world, payment.from, transfer, Some(both(say(done), then)))
}

/// Holds credits for an order: out of `payment.from`'s balance, for `payment.to`, until
/// released or returned. `then` hears the hold's id in the receipt.
pub fn hold(world: &mut World, payment: Payment, then: Option<Then>) -> bool {
    hold_as(world, payment, cadrs_credits::new_id(), then)
}

/// [`hold`], under an id of the caller's: an order's ([`cadrs_credits::order_hold_id`]), so
/// that asking again (the answer was lost, the button pressed twice) holds once.
pub fn hold_as(
    world: &mut World,
    payment: Payment,
    id: cadrs_credits::MoveId,
    then: Option<Then>,
) -> bool {
    let done = tr_with(
        "credits-held",
        &[
            ("amount", &cadrs_credits::show(payment.amount)),
            ("name", &payment.to_name),
        ],
    );
    let hold = Ask::Hold {
        id,
        to: payment.to,
        amount: payment.amount,
        note: payment.note,
        about: payment.about,
    };
    ask(world, payment.from, hold, Some(both(say(done), then)))
}

/// The payer gives a held payment to its payee.
pub fn release(
    world: &mut World,
    payer: AccountId,
    hold: cadrs_credits::MoveId,
    then: Option<Then>,
) -> bool {
    // Settling has the hold's own id: asked as often as it takes, done once.
    let release = Ask::Release {
        id: cadrs_credits::settle_id(&hold, cadrs_credits::Kind::Released),
        hold,
    };
    ask(
        world,
        payer,
        release,
        Some(both(say(tr("credits-released")), then)),
    )
}

/// [`release`], saying so only when it worked: a try again in the background.
pub fn release_quietly(world: &mut World, payer: AccountId, hold: cadrs_credits::MoveId) -> bool {
    let release = Ask::Release {
        id: cadrs_credits::settle_id(&hold, cadrs_credits::Kind::Released),
        hold,
    };
    let then: Then = Box::new(|world: &mut World, answer: Result<&Answer, &str>| {
        if answer.is_ok() {
            crate::linked::toast(world, tr("credits-released"));
        }
    });
    ask(world, payer, release, Some(then))
}

/// The payee gives a held payment back to its payer.
pub fn give_back(
    world: &mut World,
    payee: AccountId,
    hold: cadrs_credits::MoveId,
    then: Option<Then>,
) -> bool {
    let give_back = Ask::Return {
        id: cadrs_credits::settle_id(&hold, cadrs_credits::Kind::Returned),
        hold,
    };
    ask(
        world,
        payee,
        give_back,
        Some(both(say(tr("credits-returned")), then)),
    )
}

/// The payee takes a held payment its payer left unsettled for thirty days.
pub fn claim(world: &mut World, payee: AccountId, hold: cadrs_credits::MoveId) -> bool {
    let claim = Ask::Claim {
        id: cadrs_credits::settle_id(&hold, cadrs_credits::Kind::Claimed),
        hold,
    };
    ask(world, payee, claim, Some(say(tr("credits-claimed"))))
}

fn both(first: Then, second: Option<Then>) -> Then {
    Box::new(move |world: &mut World, answer: Result<&Answer, &str>| {
        first(world, answer);
        if let Some(second) = second {
            second(world, answer);
        }
    })
}

/// The held payment for order `request` between `payer` and `payee`, as `wallet`'s statement
/// has it (either side's): the one that still waits.
pub fn held_for(
    world: &World,
    wallet: &AccountId,
    request: cadrs_org::Id,
) -> Option<cadrs_credits::Hold> {
    world
        .get_resource::<Credits>()?
        .wallet(wallet)?
        .statement
        .as_ref()?
        .holds
        .iter()
        .find(|h| h.about.as_ref().and_then(About::order_id) == Some(request))
        .cloned()
}

/// What `wallet`'s movements say of order `request`: the newest that is about it.
pub fn paid_for(world: &World, wallet: &AccountId, request: cadrs_org::Id) -> Option<Movement> {
    world
        .get_resource::<Credits>()?
        .wallet(wallet)?
        .movements
        .iter()
        .find(|m| m.about.as_ref().and_then(About::order_id) == Some(request))
        .cloned()
}

/// How an order's payment stands, as a wallet on either side of it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderPayment {
    /// Held: out of the client's balance, not yet in the shop's.
    Held(cadrs_credits::Hold),
    /// The shop has it.
    Paid(Amount),
    /// It went back to the client.
    Returned(Amount),
}

/// How order `request`'s payment stands, from what a wallet's statement says: the held
/// payments it asked after by name (whatever became of them), those that wait, and its
/// movements. The named ones first: they are the order's own hold, which nobody's other
/// payments can push out of the list.
pub fn order_payment(
    named: &[cadrs_credits::Hold],
    holds: &[cadrs_credits::Hold],
    movements: &[Movement],
    request: cadrs_org::Id,
) -> Option<OrderPayment> {
    let about = |about: &Option<About>| about.as_ref().and_then(About::order_id) == Some(request);
    if let Some(hold) = named.iter().find(|h| about(&h.about)) {
        return Some(match hold.state {
            cadrs_credits::HoldState::Open => OrderPayment::Held(hold.clone()),
            cadrs_credits::HoldState::Released => OrderPayment::Paid(hold.amount),
            cadrs_credits::HoldState::Returned => OrderPayment::Returned(hold.amount),
        });
    }
    if let Some(hold) = holds.iter().find(|h| about(&h.about)) {
        return Some(OrderPayment::Held(hold.clone()));
    }
    // Newest first: what happened last is how it stands.
    let last = movements.iter().find(|m| about(&m.about))?;
    use cadrs_credits::Kind;
    match last.kind {
        Kind::Released | Kind::Claimed | Kind::Transfer | Kind::Payout => {
            Some(OrderPayment::Paid(last.amount))
        }
        Kind::Returned => Some(OrderPayment::Returned(last.amount)),
        _ => None,
    }
}

/// How order `request`'s payment stands in `wallet`, as last heard from the bank.
pub fn payment_of(
    world: &World,
    wallet: &AccountId,
    request: cadrs_org::Id,
) -> Option<OrderPayment> {
    let wallet = world.get_resource::<Credits>()?.wallet(wallet)?;
    let statement = wallet.statement.as_ref();
    let named = statement.map_or(&[][..], |s| &s.named[..]);
    let holds = statement.map_or(&[][..], |s| &s.holds[..]);
    order_payment(named, holds, &wallet.movements, request)
}

/// What is wrong with an order's payment, as the shop should see it: held or paid by someone
/// other than the order's client, or less than the quote. The catalog key that says so.
pub fn payment_problem(
    payment: &OrderPayment,
    expected: Amount,
    client: &AccountId,
) -> Option<&'static str> {
    match payment {
        OrderPayment::Held(hold) if &hold.from != client => Some("credits-order-other-payer"),
        OrderPayment::Held(hold) if hold.amount < expected => Some("credits-order-short"),
        OrderPayment::Paid(amount) if *amount < expected => Some("credits-order-short"),
        _ => None,
    }
}

/// The payment's state in words: "724 credits held", "724 credits paid".
pub fn payment_text(payment: &OrderPayment) -> String {
    let (key, amount) = match payment {
        OrderPayment::Held(hold) => ("credits-order-held", hold.amount),
        OrderPayment::Paid(amount) => ("credits-order-paid", *amount),
        OrderPayment::Returned(amount) => ("credits-order-returned", *amount),
    };
    tr_with(key, &[("amount", &cadrs_credits::show(amount))])
}

/// A quote's price as people read it: "724 credits", or for a price in another currency "ZAR
/// 724.00 (724 credits)": it is paid in credits, one for one.
pub fn price_text(currency: &str, total: cadrs_org::quote::Money) -> String {
    let credits = cadrs_credits::show(total.credits());
    if currency == cadrs_org::quote::CREDITS {
        tr_with("credits-price", &[("amount", &credits)])
    } else {
        tr_with(
            "credits-price-in-currency",
            &[
                ("price", &format!("{currency} {}", total.format())),
                ("amount", &credits),
            ],
        )
    }
}

/// A name for an account in what is said about a movement: the user, one of their
/// organizations, a contact, a shop heard of, else the start of its id.
pub fn name_of(world: &World, account: &AccountId) -> String {
    let Some(social) = world.get_resource::<Social>() else {
        return short(account);
    };
    if me(world).as_ref() == Some(account) {
        return crate::social::my_name(world);
    }
    if let Some(org) = social.orgs.iter().find(|o| &o.account == account) {
        return org.name.clone();
    }
    if let Some(card) = social.contacts.get(account) {
        return card.name.clone();
    }
    if let Some(shop) = social.directory.get(account) {
        return shop.name().to_owned();
    }
    // A client of the open organization, by the card their request came with.
    if let Some(name) = world
        .get_resource::<crate::organization::OpenOrg>()
        .and_then(|open| {
            let org = open.session.org();
            org.requests
                .values()
                .filter(|r| &r.client == account)
                .find_map(|r| r.card.as_ref().map(|card| card.value.name.clone()))
        })
    {
        return name;
    }
    short(account)
}

pub fn short(account: &AccountId) -> String {
    account.to_string().chars().take(8).collect()
}

/// The bank told this device of a movement (`Post::Credited`): believed if the bank this app
/// asks signed it and it is about a wallet this device shows.
///
/// `from` is the device it came from: only the bank's own word is taken (a receipt is the
/// bank's whoever sends it, and the other side of an old payment has one to send again and
/// again). A bank kept in this app tells it in person (`from` is none).
pub fn credited(world: &mut World, bytes: &[u8], from: Option<PublicKey>) -> Result<(), String> {
    let at = bank(world).ok_or("this cadrs asks no bank")?;
    if !(at.here && from.is_none()) && from != Some(at.id) {
        return Err("only the bank says what was paid".into());
    }
    let movement = cadrs_credits::read_credited(bytes, &at.id)?;
    let mine: Vec<AccountId> = movement
        .from
        .into_iter()
        .chain([movement.to])
        .filter(|w| shown(world, w) || is_my_org(world, w))
        .collect();
    if mine.is_empty() {
        return Err("not about a wallet of this device".into());
    }
    // Told before, or older than what is known already: no news, and nothing asked.
    let known = |world: &World, wallet: &AccountId| {
        world
            .resource::<Credits>()
            .wallet(wallet)
            .and_then(|w| w.movements.first())
            .is_some_and(|newest| movement.seq <= newest.seq)
    };
    if mine.iter().all(|wallet| known(world, wallet)) {
        return Ok(());
    }
    for wallet in &mine {
        let change = movement.change_for(wallet);
        let held_for_us = movement.kind == cadrs_credits::Kind::Held && &movement.to == wallet;
        // Anyone can hold credits for a shop: one that says no order it is for is no news (it
        // shows in the wallet, to be returned), so strangers can't make a shop's devices ring.
        let stray = held_for_us && movement.about.as_ref().and_then(About::order_id).is_none();
        // The other side's doing is news; one's own was said already when it was asked for.
        if (change > 0 || held_for_us) && !stray && movement.by != me(world).unwrap_or(*wallet) {
            let amount = cadrs_credits::show(movement.amount);
            let from = movement
                .other(wallet)
                .map_or_else(|| tr("credits-the-faucet"), |a| name_of(world, &a));
            let to = name_of(world, wallet);
            let key = if held_for_us {
                "credits-notice-held"
            } else {
                "credits-notice-received"
            };
            let title = tr_with(
                key,
                &[("amount", &amount), ("name", &from), ("wallet", &to)],
            );
            let body = movement
                .about
                .as_ref()
                .map(|a| a.label.clone())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| movement.note.clone());
            crate::linked::toast(world, title.clone());
            world.resource_mut::<Notifications>().push(Notice {
                id: format!("credits-{}", movement.seq),
                icon: "wallet",
                title,
                body,
                unread: true,
            });
        }
        refresh(world, *wallet);
    }
    Ok(())
}

fn is_my_org(world: &World, account: &AccountId) -> bool {
    world
        .get_resource::<Social>()
        .is_some_and(|s| s.orgs.iter().any(|o| &o.account == account))
}

/// Who may spend from an organization's wallet (its owners and admins) and who may look (its
/// other members), from its document.
pub fn access_of(org: &cadrs_org::Org) -> (Vec<AccountId>, Vec<AccountId>) {
    let mut spenders = Vec::new();
    let mut viewers = Vec::new();
    for (account, role) in &org.members {
        if *role >= cadrs_org::Role::Admin {
            spenders.push(*account);
        } else {
            viewers.push(*account);
        }
    }
    spenders.truncate(cadrs_credits::MAX_SPENDERS);
    viewers.truncate(cadrs_credits::MAX_VIEWERS);
    (spenders, viewers)
}

/// Tells the bank who may spend from organization `account`'s wallet, if this device holds the
/// organization's key and hasn't said this list already. (A device that doesn't hold the key
/// can't: the list stays as the key's holder last said it.)
pub fn say_access(world: &mut World, account: AccountId, org: &cadrs_org::Org) {
    let access = access_of(org);
    if world.resource::<Credits>().access_said.get(&account) == Some(&access) {
        return;
    }
    let Some(key) = crate::social::org_key(world, &account) else {
        return;
    };
    let Some(at) = bank(world) else {
        return;
    };
    let said = Ask::Access {
        spenders: access.0.clone(),
        viewers: access.1.clone(),
    };
    let bytes = cadrs_credits::ask_as(&key, at.id, said, cadrs_social::wall_millis());
    if at.here {
        if cadrs_net::bank::answer(world, &bytes, None).is_ok() {
            world
                .resource_mut::<Credits>()
                .access_said
                .insert(account, access);
        }
        return;
    }
    let Some(iroh) = world.get_resource::<Iroh>() else {
        return;
    };
    let Ok(to) = EndpointId::from_bytes(&at.id.0) else {
        return;
    };
    let addr = world
        .get_resource::<crate::social::PeerAddrs>()
        .and_then(|hints| hints.0.get(&to).cloned())
        .unwrap_or_else(|| to.into());
    let task = cadrs_net::social::start_send(iroh, addr, Post::Bank(bytes).encode());
    // Remembered once the bank took it; asked again the next time otherwise.
    let then: Then = Box::new(move |world: &mut World, answer: Result<&Answer, &str>| {
        if answer.is_ok() {
            world
                .resource_mut::<Credits>()
                .access_said
                .insert(account, access);
            if shown(world, &account) {
                refresh(world, account);
            }
        }
    });
    world.resource_mut::<Credits>().asking.push(Asking {
        wallet: account,
        older: false,
        task,
        then: Some(then),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_credits::{Hold, HoldState};
    use cadrs_social::keys::SecretKey;

    /// The shop sees a payment that isn't the order's for what it is: too little, or from
    /// someone else.
    #[test]
    fn a_payment_is_checked_against_the_order() {
        let (client, other, shop) = (
            SecretKey::generate().public(),
            SecretKey::generate().public(),
            SecretKey::generate().public(),
        );
        let held = |from, amount| {
            OrderPayment::Held(Hold {
                id: [1; 16],
                from,
                to: shop,
                amount,
                about: Some(About::order(7, "R-0007")),
                time: 0,
                state: HoldState::Open,
            })
        };
        assert_eq!(payment_problem(&held(client, 696), 696, &client), None);
        assert_eq!(payment_problem(&held(client, 700), 696, &client), None);
        assert_eq!(
            payment_problem(&held(client, 1), 696, &client),
            Some("credits-order-short")
        );
        assert_eq!(
            payment_problem(&held(other, 696), 696, &client),
            Some("credits-order-other-payer")
        );
        assert_eq!(
            payment_problem(&OrderPayment::Paid(100), 696, &client),
            Some("credits-order-short")
        );
        assert_eq!(
            payment_problem(&OrderPayment::Paid(696), 696, &client),
            None
        );
        assert_eq!(
            payment_problem(&OrderPayment::Returned(1), 696, &client),
            None
        );
        // An order's payment is found by what it says it is for, the held one first.
        let OrderPayment::Held(hold) = held(client, 696) else {
            unreachable!()
        };
        assert_eq!(
            order_payment(&[], std::slice::from_ref(&hold), &[], 7),
            Some(held(client, 696))
        );
        assert_eq!(
            order_payment(&[], std::slice::from_ref(&hold), &[], 8),
            None
        );
        // Asked after by name, it is found whatever became of it, before anything else.
        let settled = |state| Hold {
            state,
            ..hold.clone()
        };
        assert_eq!(
            order_payment(&[settled(HoldState::Released)], &[], &[], 7),
            Some(OrderPayment::Paid(696))
        );
        assert_eq!(
            order_payment(
                &[settled(HoldState::Returned)],
                std::slice::from_ref(&hold),
                &[],
                7
            ),
            Some(OrderPayment::Returned(696))
        );
        assert_eq!(
            order_payment(&[settled(HoldState::Open)], &[], &[], 7),
            Some(held(client, 696))
        );
    }
}
