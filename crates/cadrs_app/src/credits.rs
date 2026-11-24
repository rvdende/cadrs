//! Credits in the app: the user's wallet and their organizations', as the bank says they are
//! (`cadrs_credits`). The app knows a bank only as [`cadrs_credits::Bank`]: this module asks
//! it, keeps what it answered for the interface to show, and does the errands it asks for.
//!
//! - [`refresh`] asks for a wallet's statement; [`faucet`], [`pay`], [`hold`], [`release`] and
//!   [`give_back`] move credits. Each says what happened in a toast, and [`Credits::version`]
//!   counts up when what a wallet shows changed, so what draws it draws it again.
//! - The bank is [`cadrs_credits::trusting::Trusting`] unless another was put in its place
//!   ([`use_bank`]): there is no bank, each device keeps its own wallets, and whoever is paid
//!   is told with a note and believes it. Test credits; a real ledger (Solana) takes its place
//!   behind the same trait.
//! - The bank's errands ([`cadrs_credits::Event`]): what it wants kept is written to a file
//!   beside the user's keys (their own wallet) or into the organization's document (an
//!   organization's, so that its members' devices share one wallet); what it wants told is
//!   sent to the other side's devices as `Post::Credited`, again later when they weren't
//!   reached; what arrived is said in a toast and a notice.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use cadrs_credits::book::Effect;
use cadrs_credits::{
    About, Answer, Ask, Asker, Bank, Credits as Amount, Event, Movement, Refusal, Statement, Ticket,
};
use cadrs_net::bevy_iroh::iroh::EndpointId;
use cadrs_net::bevy_iroh::{Iroh, IrohTask};
use cadrs_social::keys::{AccountId, DeviceId};
use cadrs_social::post::Post;
use cadrs_ui::i18n::{tr, tr_with};

use crate::notifications::{Notice, Notifications};
use crate::social::Social;

/// How often the wallets on screen are asked for again, in seconds.
const REFRESH_EVERY: f64 = 60.0;
/// How long before someone who wasn't reached is told again what they were paid, in seconds:
/// soon at first (they were away for a moment), then less and less often, up to a minute.
/// And how many such notes wait at most (the oldest are dropped first).
const TELL_AGAIN_AFTER: f64 = 3.0;
const TELL_AGAIN_AT_MOST: f64 = 60.0;
const MAX_UNTOLD: usize = 500;
/// The file the user's own wallet is kept in, beside their keys.
const WALLET_FILE: &str = "credits.bin";

pub struct CreditsPlugin;

impl Plugin for CreditsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Credits>()
            .add_systems(Update, (poll, tell, keep_fresh).chain());
    }
}

/// The bank the app asks: whatever keeps the credits.
#[derive(Resource)]
pub struct TheBank(pub Box<dyn Bank>);

/// Puts `bank` in the place of the trusting one (a real ledger's client).
pub fn use_bank(world: &mut World, bank: Box<dyn Bank>) {
    world.insert_resource(TheBank(bank));
    let mut credits = world.resource_mut::<Credits>();
    credits.restored.clear();
    credits.changed();
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
    then: Option<Then>,
}

/// A note for someone who was paid, on its way or waiting to be sent again.
struct Untold {
    to: AccountId,
    note: Vec<u8>,
    tasks: Vec<IrohTask<Result<Vec<u8>, String>>>,
    /// When it was last sent, and how long to wait before sending it again.
    tried: Option<(f64, f64)>,
}

#[derive(Resource, Default)]
pub struct Credits {
    pub wallets: BTreeMap<AccountId, Wallet>,
    /// Counts up whenever what a wallet shows changed.
    pub version: u32,
    /// The wallets on screen now (the open wallet dialog's, an open organization's).
    watched: BTreeSet<AccountId>,
    asking: BTreeMap<Ticket, Asking>,
    /// The held payments each wallet's statement is asked to say of: its orders' own.
    named: BTreeMap<AccountId, Vec<cadrs_credits::MoveId>>,
    /// The wallets the bank was given what is kept for them, and how much of it there was.
    restored: BTreeMap<AccountId, usize>,
    /// What is kept for the user's own wallet (the file's contents).
    mine: Vec<Effect>,
    untold: Vec<Untold>,
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

/// The bank is there (the trusting one, made with this device's keys, unless another was put
/// in its place), and keeps the user's own wallet. False without keys.
fn ensure_bank(world: &mut World) -> bool {
    crate::social::ensure_loaded(world);
    let Some(me) = me(world) else {
        return false;
    };
    if !world.contains_resource::<TheBank>() {
        let Some((device, cert)) = world
            .resource::<Social>()
            .keys()
            .map(|keys| (keys.device.clone(), keys.cert.clone()))
        else {
            return false;
        };
        let bank = cadrs_credits::trusting::Trusting::new(device, cert);
        world.insert_resource(TheBank(Box::new(bank)));
        world.resource_mut::<Credits>().restored.clear();
    }
    if !world.resource::<Credits>().restored.contains_key(&me) {
        let kept: Vec<Effect> = crate::social::load_file(world, WALLET_FILE);
        world.resource_mut::<TheBank>().0.restore(me, false, &kept);
        let mut credits = world.resource_mut::<Credits>();
        credits.restored.insert(me, kept.len());
        credits.mine = kept;
    }
    true
}

/// What is kept for organization `account`'s wallet, in its document: the open one's, else
/// read from disk.
fn org_kept(world: &World, account: &AccountId) -> Option<Vec<Vec<u8>>> {
    if let Some(open) = world.get_resource::<crate::organization::OpenOrg>()
        && open.session.org().org.as_ref() == Some(account)
    {
        return Some(open.session.org().credits.clone());
    }
    let social = world.get_resource::<Social>()?;
    let mine = social.orgs.iter().find(|o| &o.account == account)?;
    crate::social::org_state(world, mine).map(|org| org.credits)
}

/// How much is kept for organization `account`'s wallet, without reading it all when the
/// organization is the open one.
fn org_kept_len(world: &World, account: &AccountId) -> Option<usize> {
    match world.get_resource::<crate::organization::OpenOrg>() {
        Some(open) if open.session.org().org.as_ref() == Some(account) => {
            Some(open.session.org().credits.len())
        }
        _ => org_kept(world, account).map(|kept| kept.len()),
    }
}

/// The bank keeps organization `account`'s wallet as its document has it now: given to the
/// bank the first time, and again when the document has more (another member's device paid,
/// or was paid). True when the bank was given anything.
fn sync_org(world: &mut World, account: AccountId) -> bool {
    if !is_my_org(world, &account) || !world.contains_resource::<TheBank>() {
        return false;
    }
    let known = world.resource::<Credits>().restored.get(&account).copied();
    if known.is_some() && org_kept_len(world, &account) == known {
        return false;
    }
    let Some(kept) = org_kept(world, &account) else {
        return false;
    };
    let effects: Vec<Effect> = kept
        .iter()
        .filter_map(|bytes| cadrs_social::codec::decode(bytes).ok())
        .collect();
    world
        .resource_mut::<TheBank>()
        .0
        .restore(account, true, &effects);
    world
        .resource_mut::<Credits>()
        .restored
        .insert(account, kept.len());
    true
}

/// Who asks about `wallet`: the user, who may spend from their own wallet and from an
/// organization's of which they are an owner or admin, and may look at one they are a member
/// of.
fn asker(world: &World, me: AccountId, wallet: &AccountId) -> Asker {
    let may_spend = &me == wallet || {
        let role = |org: &cadrs_org::Org| org.members.get(&me).copied();
        let role = match world.get_resource::<crate::organization::OpenOrg>() {
            Some(open) if open.session.org().org.as_ref() == Some(wallet) => {
                role(open.session.org())
            }
            _ => world
                .resource::<Social>()
                .orgs
                .iter()
                .find(|o| &o.account == wallet)
                .and_then(|mine| crate::social::org_state(world, mine))
                .and_then(|org| role(&org)),
        };
        role.is_some_and(|role| role >= cadrs_org::Role::Admin)
    };
    Asker {
        account: me,
        may_spend,
    }
}

/// A refusal in the user's language, when it is one of the bank's own; else as it was said.
pub fn reason(error: &str) -> String {
    let key = match Refusal::of(error) {
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
        None => return error.to_owned(),
    };
    tr(key)
}

/// Asks the bank `ask` about `wallet`, as the user. False when it couldn't be asked (no
/// keys), with `then` told why. The answer comes in a later frame ([`poll`]).
pub fn ask(world: &mut World, wallet: AccountId, ask: Ask, then: Option<Then>) -> bool {
    if !ensure_bank(world) {
        let why = tr("social-no-keys");
        let mut credits = world.resource_mut::<Credits>();
        let w = credits.wallets.entry(wallet).or_default();
        if w.error.as_deref() != Some(why.as_str()) {
            w.error = Some(why.clone());
            credits.changed();
        }
        if let Some(then) = then {
            then(world, Err(&why));
        }
        return false;
    }
    let Some(me) = me(world) else {
        return false;
    };
    sync_org(world, wallet);
    let older = matches!(
        ask,
        Ask::Statement {
            before: Some(_),
            ..
        }
    );
    let by = asker(world, me, &wallet);
    let now = cadrs_social::wall_millis();
    let ticket = world.resource_mut::<TheBank>().0.ask(by, wallet, ask, now);
    let mut credits = world.resource_mut::<Credits>();
    credits.wallets.entry(wallet).or_default().asking = true;
    credits.asking.insert(
        ticket,
        Asking {
            wallet,
            older,
            then,
        },
    );
    true
}

/// The bank's answer to an ask about `wallet` (or why there is none) is taken in.
fn took(
    world: &mut World,
    wallet: AccountId,
    older: bool,
    answer: Result<Answer, String>,
    then: Option<Then>,
) {
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
            Ok(Answer::Moved(movement)) => {
                w.error = None;
                // Both sides' wallets, if this device shows them, are asked for again.
                again.extend(movement.from);
                again.push(movement.to);
            }
            Err(why) => w.error = Some(reason(why)),
        }
        // The faucet's wait is compared as time left, to the minute: the clock moved on.
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
        // Only what shows differently is news. (An answer that changes nothing must not count:
        // what draws a wallet also asks about it, and would ask again for ever.)
        if minute(before) != minute(shown(w)) {
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

/// What the bank has to say is heard, and its errands done.
fn poll(world: &mut World) {
    if !world.contains_resource::<TheBank>() {
        return;
    }
    let now = cadrs_social::wall_millis();
    let events = world.resource_mut::<TheBank>().0.poll(now);
    for event in events {
        match event {
            Event::Answered(ticket, answer) => {
                if let Some(asked) = world.resource_mut::<Credits>().asking.remove(&ticket) {
                    took(world, asked.wallet, asked.older, answer, asked.then);
                }
            }
            Event::Keep { wallet, effect } => keep(world, wallet, effect),
            Event::Tell { to, note } => {
                let mut credits = world.resource_mut::<Credits>();
                if credits.untold.len() >= MAX_UNTOLD {
                    credits.untold.remove(0);
                }
                credits.untold.push(Untold {
                    to,
                    note,
                    tasks: Vec::new(),
                    tried: None,
                });
            }
            Event::Credited { wallet, movement } => arrived(world, wallet, &movement),
        }
    }
}

/// Keeps `effect` for `wallet`: in the user's own file, or in the organization's document
/// (where its other members' devices find it).
fn keep(world: &mut World, wallet: AccountId, effect: Effect) {
    if me(world) == Some(wallet) {
        let mut credits = world.resource_mut::<Credits>();
        credits.mine.push(effect);
        let count = credits.mine.len();
        credits.restored.insert(wallet, count);
        let kept = std::mem::take(&mut world.resource_mut::<Credits>().mine);
        crate::social::save_file(world, WALLET_FILE, &kept);
        world.resource_mut::<Credits>().mine = kept;
        return;
    }
    let op = cadrs_org::Op::Credits(cadrs_social::codec::encode(&effect));
    match crate::social::write_org(world, &wallet, op) {
        // The bank has it already: what is kept and what the bank knows stay level.
        Ok(()) => {
            if let Some(count) = world.resource_mut::<Credits>().restored.get_mut(&wallet) {
                *count += 1;
            }
        }
        Err(why) => warn!("an organization's wallet couldn't be kept: {why}"),
    }
}

/// Where an account's devices are, as far as this device knows: a contact's card, a shop's
/// listing, a member of the open organization, a client by the card their request came with.
fn devices_of(world: &World, account: &AccountId) -> Vec<DeviceId> {
    let social = world.resource::<Social>();
    if let Some(card) = social.contacts.get(account) {
        return card.devices.clone();
    }
    if let Some(shop) = social.directory.get(account) {
        return shop.card.value.devices.clone();
    }
    let Some(open) = world.get_resource::<crate::organization::OpenOrg>() else {
        return Vec::new();
    };
    let org = open.session.org();
    let members: Vec<DeviceId> = org.devices.of(account).iter().map(|c| c.device).collect();
    if !members.is_empty() {
        return members;
    }
    org.requests
        .values()
        .filter(|r| &r.client == account)
        .find_map(|r| r.card.as_ref().map(|card| card.value.devices.clone()))
        .unwrap_or_default()
}

/// Whoever was paid is told: each note is sent to the other side's devices, and again after a
/// while when none of them took it.
fn tell(world: &mut World) {
    if world.resource::<Credits>().untold.is_empty() {
        return;
    }
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    let mut untold = std::mem::take(&mut world.resource_mut::<Credits>().untold);
    untold.retain_mut(|waiting| {
        // One device taking it is enough.
        let mut taken = false;
        waiting.tasks.retain_mut(|task| match task.try_take() {
            Some(Ok(_)) => {
                taken = true;
                false
            }
            Some(Err(_)) => false,
            None => true,
        });
        if taken {
            return false;
        }
        let due =
            waiting.tasks.is_empty() && waiting.tried.is_none_or(|(at, wait)| now - at >= wait);
        if due {
            let wait = waiting.tried.map_or(TELL_AGAIN_AFTER, |(_, wait)| {
                (wait * 2.0).min(TELL_AGAIN_AT_MOST)
            });
            waiting.tried = Some((now, wait));
            let devices = devices_of(world, &waiting.to);
            if let Some(iroh) = world.get_resource::<Iroh>() {
                let hints = world.get_resource::<crate::social::PeerAddrs>();
                let post = Post::Credited(waiting.note.clone()).encode();
                waiting.tasks = crate::social::dialed(&devices)
                    .iter()
                    .filter_map(|device| EndpointId::from_bytes(&device.0).ok())
                    .map(|device| {
                        let addr = hints
                            .and_then(|h| h.0.get(&device).cloned())
                            .unwrap_or_else(|| device.into());
                        cadrs_net::social::start_send(iroh, addr, post.clone())
                    })
                    .collect();
            }
        }
        true
    });
    // What the bank asked to be told meanwhile comes after what waited.
    let mut credits = world.resource_mut::<Credits>();
    untold.append(&mut credits.untold);
    credits.untold = untold;
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

/// The wallets on screen are asked for again now and then, and an organization's at once
/// when its document has more for its wallet (another member's device paid, or was paid).
fn keep_fresh(world: &mut World, mut last: Local<Option<f64>>) {
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if last.is_some_and(|t| now - t < 1.0) {
        return;
    }
    *last = Some(now);
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
    let watched: Vec<AccountId> = world
        .resource::<Credits>()
        .watched
        .iter()
        .copied()
        .collect();
    for wallet in watched {
        let news = sync_org(world, wallet);
        let due = world
            .resource::<Credits>()
            .wallet(&wallet)
            .is_none_or(|w| !w.asking && w.asked_at.is_none_or(|t| now - t >= REFRESH_EVERY));
        if news || due {
            refresh(world, wallet);
        }
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

/// Credits arrived in `wallet` by someone else's doing (the bank said so): said in a toast
/// and a notice, and the wallet asked for again.
fn arrived(world: &mut World, wallet: AccountId, movement: &Movement) {
    let change = movement.change_for(&wallet);
    let held_for_us = movement.kind == cadrs_credits::Kind::Held && movement.to == wallet;
    // Anyone can hold credits for a shop: one that says no order it is for is no news (it
    // shows in the wallet, to be returned), so strangers can't make a shop's devices ring.
    let stray = held_for_us && movement.about.as_ref().and_then(About::order_id).is_none();
    // The other side's doing is news; one's own was said already when it was asked for.
    if (change > 0 || held_for_us) && !stray && Some(movement.by) != me(world) {
        let amount = cadrs_credits::show(movement.amount);
        let from = movement
            .other(&wallet)
            .map_or_else(|| tr("credits-the-faucet"), |a| name_of(world, &a));
        let to = name_of(world, &wallet);
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
            id: format!("credits-{}-{}", short(&wallet), movement.seq),
            icon: "wallet",
            title,
            body,
            unread: true,
        });
    }
    if shown(world, &wallet) {
        refresh(world, wallet);
    }
}

/// A note about credits arrived (`Post::Credited`): given to the bank, which believes it or
/// says why not. The wallets it could be for are the user's own and their organizations'.
pub fn credited(world: &mut World, bytes: &[u8]) -> Result<(), String> {
    if !ensure_bank(world) {
        return Err("this cadrs has no wallet yet".into());
    }
    let orgs: Vec<AccountId> = world
        .resource::<Social>()
        .orgs
        .iter()
        .map(|o| o.account)
        .collect();
    for org in orgs {
        sync_org(world, org);
    }
    let now = cadrs_social::wall_millis();
    world.resource_mut::<TheBank>().0.receive(bytes, now)?;
    // What it led to (to be kept, to be said) is done now, not a frame later: whoever sent it
    // is answered once it is kept.
    poll(world);
    Ok(())
}

fn is_my_org(world: &World, account: &AccountId) -> bool {
    world
        .get_resource::<Social>()
        .is_some_and(|s| s.orgs.iter().any(|o| &o.account == account))
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
