//! The trusting bank between devices: each keeps its own books, tells the other side with a
//! note, and believes the notes it gets.

use cadrs_credits::book::Effect;
use cadrs_credits::refusal as why;
use cadrs_credits::trusting::{MAX_NOTES_A_DAY, Trusting};
use cadrs_credits::{
    About, AccountId, Answer, Ask, Asker, Bank, CLAIM_AFTER, Credits, Event, FAUCET, FAUCET_EVERY,
    Kind, MoveId, Movement, PAGE, Refusal, Statement, new_id, order_hold, order_hold_id, settle_id,
};
use cadrs_social::keys::{Account, SecretKey};

const T0: u64 = 1_800_000_000_000;

/// Someone's device, with the bank it keeps: their own wallet, and what was handed over to be
/// kept for each wallet.
struct Device {
    account: Account,
    key: SecretKey,
    bank: Trusting,
    kept: Vec<(AccountId, Effect)>,
    told: Vec<(AccountId, Vec<u8>)>,
    news: Vec<(AccountId, Movement)>,
}

impl Device {
    fn new() -> Self {
        let account = Account::generate();
        let key = SecretKey::generate();
        let cert = account.certify(key.public(), "laptop", T0);
        let mut bank = Trusting::new(SecretKey::from_bytes(&key.to_bytes()), cert);
        bank.restore(account.id(), false, &[]);
        Self {
            account,
            key,
            bank,
            kept: Vec::new(),
            told: Vec::new(),
            news: Vec::new(),
        }
    }

    fn id(&self) -> AccountId {
        self.account.id()
    }

    /// Asks about `wallet` as its owner (or, `may_spend` false, as someone who may look).
    fn ask_as(
        &mut self,
        wallet: AccountId,
        may_spend: bool,
        ask: Ask,
        now: u64,
    ) -> Result<Answer, String> {
        let by = Asker {
            account: self.id(),
            may_spend,
        };
        let ticket = self.bank.ask(by, wallet, ask, now);
        let mut answer = None;
        for event in self.bank.poll(now) {
            match event {
                Event::Answered(t, a) if t == ticket => answer = Some(a),
                Event::Answered(..) => panic!("an answer to nothing asked"),
                Event::Keep { wallet, effect } => self.kept.push((wallet, effect)),
                Event::Tell { to, note } => self.told.push((to, note)),
                Event::Credited { wallet, movement } => self.news.push((wallet, movement)),
            }
        }
        answer.expect("an answer")
    }

    fn moved(&mut self, wallet: AccountId, ask: Ask, now: u64) -> Result<Movement, String> {
        match self.ask_as(wallet, true, ask, now)? {
            Answer::Moved(movement) => Ok(movement),
            other => panic!("a movement, not {other:?}"),
        }
    }

    fn statement(&mut self, wallet: AccountId, now: u64) -> Statement {
        let ask = Ask::Statement {
            before: None,
            holds: Vec::new(),
        };
        match self.ask_as(wallet, true, ask, now) {
            Ok(Answer::Statement(statement)) => statement,
            other => panic!("a statement, not {other:?}"),
        }
    }

    fn balance(&mut self, wallet: AccountId) -> Credits {
        self.statement(wallet, T0).balance
    }

    fn faucet(&mut self, now: u64) -> Result<Movement, String> {
        let me = self.id();
        self.moved(me, Ask::Faucet { id: new_id() }, now)
    }

    /// Takes a note, with what it leads to.
    fn receive(&mut self, note: &[u8], now: u64) -> Result<(), String> {
        let taken = self.bank.receive(note, now);
        for event in self.bank.poll(now) {
            match event {
                Event::Keep { wallet, effect } => self.kept.push((wallet, effect)),
                Event::Credited { wallet, movement } => self.news.push((wallet, movement)),
                other => panic!("a note leads to nothing else: {other:?}"),
            }
        }
        taken
    }

    /// Hands everything this device was told to tell `other` over to it.
    fn deliver(&mut self, other: &mut Device, now: u64) {
        let theirs = other.id();
        let notes: Vec<Vec<u8>> = self
            .told
            .extract_if(.., |(to, _)| *to == theirs)
            .map(|(_, note)| note)
            .collect();
        for note in notes {
            other.receive(&note, now).expect("believed");
        }
    }
}

fn transfer(to: AccountId, amount: Credits) -> Ask {
    Ask::Transfer {
        id: new_id(),
        to,
        amount,
        note: String::new(),
        about: None,
        payout: false,
    }
}

fn hold(id: MoveId, to: AccountId, amount: Credits) -> Ask {
    Ask::Hold {
        id,
        to,
        amount,
        note: String::new(),
        about: Some(About::order(7, "R-0007")),
    }
}

#[test]
fn the_faucet_gives_a_thousand_once_an_hour() {
    let mut thandi = Device::new();
    let me = thandi.id();
    assert_eq!(thandi.statement(me, T0).faucet_at, Some(T0));
    let got = thandi.faucet(T0).unwrap();
    assert_eq!((got.kind, got.amount, got.to), (Kind::Faucet, FAUCET, me));
    let after = thandi.statement(me, T0 + 1);
    assert_eq!(
        (after.balance, after.faucet_at),
        (1000, Some(T0 + FAUCET_EVERY))
    );
    let early = thandi.faucet(T0 + FAUCET_EVERY - 1).unwrap_err();
    assert_eq!(Refusal::of(&early), Some(Refusal::FaucetNotYet));
    thandi.faucet(T0 + FAUCET_EVERY).unwrap();
    assert_eq!(thandi.balance(me), 2000);
    // Kept, to be given back when cadrs next starts; the faucet tells nobody.
    assert_eq!(thandi.kept.len(), 2);
    assert!(thandi.told.is_empty());
}

#[test]
fn a_payment_is_told_to_the_other_side_who_believes_it() {
    let (mut marco, mut sipho) = (Device::new(), Device::new());
    let (m, s) = (marco.id(), sipho.id());
    marco.faucet(T0).unwrap();
    let ask = Ask::Transfer {
        id: new_id(),
        to: s,
        amount: 724,
        note: "  for the bracket ".into(),
        about: Some(About::order(7, "R-0007")),
        payout: false,
    };
    let paid = marco.moved(m, ask.clone(), T0 + 1).unwrap();
    assert_eq!(
        (paid.amount, paid.note.as_str(), paid.by),
        (724, "for the bracket", m)
    );
    assert_eq!(marco.balance(m), 276);
    // One note, for Sipho. Until it arrives he has nothing.
    assert_eq!(marco.told.len(), 1);
    assert_eq!(sipho.balance(s), 0);
    let note = marco.told[0].1.clone();
    marco.deliver(&mut sipho, T0 + 2);
    assert_eq!(sipho.balance(s), 724);
    let (wallet, news) = sipho.news.pop().expect("news of it");
    assert_eq!((wallet, news.change_for(&s), news.from), (s, 724, Some(m)));
    assert_eq!(news.about.as_ref().and_then(About::order_id), Some(7));
    // Told again (sent twice, to be sure): once.
    sipho.receive(&note, T0 + 3).unwrap();
    assert_eq!(sipho.balance(s), 724);
    assert!(sipho.news.is_empty());
    // Asked again with the same id: done once, and nobody is told twice.
    assert_eq!(marco.moved(m, ask, T0 + 4).unwrap(), paid);
    assert_eq!(marco.balance(m), 276);
    assert!(marco.told.is_empty());
    // What can't be paid isn't.
    for (amount, refusal) in [(277, Refusal::NotEnough), (0, Refusal::Amount)] {
        let refused = marco.moved(m, transfer(s, amount), T0 + 5).unwrap_err();
        assert_eq!(Refusal::of(&refused), Some(refusal));
    }
    let own = marco.moved(m, transfer(m, 1), T0 + 5).unwrap_err();
    assert_eq!(Refusal::of(&own), Some(Refusal::SameWallet));
}

#[test]
fn a_note_is_believed_when_it_gives_never_when_it_takes() {
    let (mut marco, mut sipho, mut thief) = (Device::new(), Device::new(), Device::new());
    let (m, s) = (marco.id(), sipho.id());
    marco.faucet(T0).unwrap();
    let paid = marco.moved(m, transfer(s, 100), T0).unwrap();
    // The thief writes notes of their own, signed by their own device, as cadrs would.
    let cert = thief.account.certify(thief.key.public(), "laptop", T0);
    let forged = |movement: Movement| cadrs_credits::write_note(&thief.key, &cert, &movement);
    let theirs = |movement: Movement| Movement {
        by: thief.id(),
        ..movement
    };
    // "Marco pays the thief": taken to Marco, it would take from his wallet.
    let take = theirs(Movement {
        id: new_id(),
        to: thief.id(),
        ..paid.clone()
    });
    assert_eq!(marco.receive(&forged(take), T0).unwrap_err(), why::NOT_OURS);
    // "Marco's hold was returned to the thief", "claimed by the thief": about no hold of his.
    for kind in [Kind::Claimed, Kind::Returned] {
        let note = forged(theirs(Movement {
            id: new_id(),
            kind,
            from: Some(m),
            to: thief.id(),
            hold: Some(new_id()),
            ..paid.clone()
        }));
        let taken = marco.receive(&note, T0);
        // A return gives to Marco and is believed, for a hold he doesn't have: nothing moves.
        if kind == Kind::Claimed {
            assert_eq!(taken.unwrap_err(), why::NO_HOLD);
        }
        assert_eq!(marco.balance(m), 900);
    }
    // Someone else's word passed off as one's own: the note says whose doing it is.
    let not_theirs = forged(paid.clone());
    assert_eq!(sipho.receive(&not_theirs, T0).unwrap_err(), why::SIGNATURE);
    // A certificate the thief wrote for Marco's device proves nothing.
    let stolen = thief.account.certify(marco.key.public(), "mine now", T0);
    let note = cadrs_credits::write_note(&marco.key, &stolen, &paid);
    assert_eq!(sipho.receive(&note, T0).unwrap_err(), why::SIGNATURE);
    // Garbage, and a lot of it.
    assert!(sipho.receive(b"not a note", T0).is_err());
    assert_eq!(
        sipho.receive(&vec![0x81; 1 << 20], T0).unwrap_err(),
        why::TOO_LARGE
    );
    assert_eq!(sipho.balance(s), 0);
    // There is no ledger, so credits the thief never had are believed: said here so that
    // nobody takes the trusting bank for more than it is.
    let from_nothing = forged(theirs(Movement {
        id: new_id(),
        from: Some(thief.id()),
        to: s,
        amount: 5000,
        ..paid
    }));
    sipho.receive(&from_nothing, T0).unwrap();
    assert_eq!(sipho.balance(s), 5000);
    assert_eq!(thief.balance(thief.id()), 0);
}

#[test]
fn a_held_payment_is_released_by_the_payer_or_returned_by_the_payee() {
    let (mut marco, mut shop) = (Device::new(), Device::new());
    let (m, s) = (marco.id(), shop.id());
    marco.faucet(T0).unwrap();
    // An order's hold has the order's own id: asked for twice, held once, and both sides can
    // work out what it is called.
    let (request, quote) = (0x1234_u128, 0x99_u128);
    let asked = order_hold_id(request, quote);
    let held = marco.moved(m, hold(asked, s, 724), T0 + 1).unwrap();
    assert_eq!(marco.moved(m, hold(asked, s, 724), T0 + 2).unwrap(), held);
    assert_eq!(held.id, order_hold(&m, request, quote));
    marco.deliver(&mut shop, T0 + 2);
    let (mine, theirs) = (marco.statement(m, T0 + 3), shop.statement(s, T0 + 3));
    assert_eq!((mine.balance, mine.held_out), (276, 724));
    assert_eq!(
        (theirs.balance, theirs.held_in, theirs.holds.len()),
        (0, 724, 1)
    );
    // The shop asks after its order's hold by name.
    let ask = Ask::Statement {
        before: None,
        holds: vec![order_hold(&m, request, quote), new_id()],
    };
    let Ok(Answer::Statement(named)) = shop.ask_as(s, true, ask, T0 + 3) else {
        panic!("a statement");
    };
    assert_eq!(named.named.len(), 1);
    assert_eq!(named.named[0].amount, 724);
    // Neither takes it alone the other way, and the shop can't claim it yet.
    let settle = |kind| settle_id(&held.id, kind);
    let release = Ask::Release {
        id: settle(Kind::Released),
        hold: held.id,
    };
    let give_back = |id| Ask::Return { id, hold: held.id };
    let claim = Ask::Claim {
        id: settle(Kind::Claimed),
        hold: held.id,
    };
    assert_eq!(
        shop.moved(s, release.clone(), T0 + 4).unwrap_err(),
        why::NOT_PAYER
    );
    assert_eq!(
        marco.moved(m, give_back(new_id()), T0 + 4).unwrap_err(),
        why::NOT_PAYEE
    );
    assert_eq!(shop.moved(s, claim, T0 + 4).unwrap_err(), why::TOO_EARLY);
    // The goods arrive: Marco releases it, as often as it takes, and the shop is paid once.
    for at in 0..3 {
        let paid = marco.moved(m, release.clone(), T0 + 5 + at).unwrap();
        assert_eq!((paid.kind, paid.hold), (Kind::Released, Some(held.id)));
    }
    marco.deliver(&mut shop, T0 + 9);
    assert_eq!(shop.balance(s), 724);
    assert_eq!(
        shop.moved(s, give_back(new_id()), T0 + 10).unwrap_err(),
        why::SETTLED
    );

    // Another, which the shop won't make: it gives the credits back.
    let second = marco.moved(m, hold(new_id(), s, 200), T0 + 11).unwrap();
    marco.deliver(&mut shop, T0 + 11);
    let ask = Ask::Return {
        id: settle_id(&second.id, Kind::Returned),
        hold: second.id,
    };
    shop.moved(s, ask, T0 + 12).unwrap();
    assert_eq!(marco.balance(m), 76);
    shop.deliver(&mut marco, T0 + 13);
    assert_eq!(marco.balance(m), 276);

    // A third, never settled: after thirty days the shop takes it, and Marco believes that
    // only then.
    let third = marco.moved(m, hold(new_id(), s, 76), T0 + 14).unwrap();
    marco.deliver(&mut shop, T0 + 14);
    let late = T0 + 14 + CLAIM_AFTER;
    let ask = Ask::Claim {
        id: settle_id(&third.id, Kind::Claimed),
        hold: third.id,
    };
    assert_eq!(
        shop.moved(s, ask.clone(), late - 1).unwrap_err(),
        why::TOO_EARLY
    );
    shop.moved(s, ask, late).unwrap();
    assert_eq!(shop.balance(s), 800);
    let note = shop.told.pop().unwrap().1;
    assert_eq!(marco.receive(&note, late - 1).unwrap_err(), why::TOO_EARLY);
    marco.receive(&note, late).unwrap();
    assert_eq!(marco.statement(m, late).held_out, 0);

    // A release whose hold's own note never arrived still pays the shop.
    let mut other_shop = Device::new();
    let o = other_shop.id();
    marco.faucet(late).unwrap();
    let lost = marco.moved(m, hold(new_id(), o, 500), late).unwrap();
    marco.told.clear();
    let ask = Ask::Release {
        id: settle_id(&lost.id, Kind::Released),
        hold: lost.id,
    };
    marco.moved(m, ask, late + 1).unwrap();
    marco.deliver(&mut other_shop, late + 2);
    assert_eq!(other_shop.balance(o), 500);
}

#[test]
fn an_organizations_wallet_is_kept_beside_its_owners() {
    let mut sipho = Device::new();
    let me = sipho.id();
    let shop = Account::generate().id();
    sipho.bank.restore(shop, true, &[]);
    sipho.faucet(T0).unwrap();
    // The faucet is a person's; a member who may only look can't spend.
    let refused = sipho
        .moved(shop, Ask::Faucet { id: new_id() }, T0)
        .unwrap_err();
    assert_eq!(Refusal::of(&refused), Some(Refusal::FaucetPeople));
    sipho.moved(me, transfer(shop, 600), T0 + 1).unwrap();
    // Both wallets are kept here: done in both, told to nobody, news for the shop.
    assert!(sipho.told.is_empty());
    assert_eq!((sipho.balance(me), sipho.balance(shop)), (400, 600));
    assert_eq!(sipho.news.pop().map(|(wallet, _)| wallet), Some(shop));
    let look = sipho
        .ask_as(shop, false, transfer(me, 1), T0 + 2)
        .unwrap_err();
    assert_eq!(Refusal::of(&look), Some(Refusal::NotYours));
    let seen = sipho.ask_as(
        shop,
        false,
        Ask::Statement {
            before: None,
            holds: vec![],
        },
        T0 + 2,
    );
    let Ok(Answer::Statement(seen)) = seen else {
        panic!("a statement");
    };
    assert_eq!(
        (seen.balance, seen.may_spend, seen.organization),
        (600, false, true)
    );
    // A payout, called what it is.
    let ask = Ask::Transfer {
        id: new_id(),
        to: me,
        amount: 250,
        note: "November".into(),
        about: None,
        payout: true,
    };
    assert_eq!(sipho.moved(shop, ask, T0 + 3).unwrap().kind, Kind::Payout);
    assert_eq!((sipho.balance(me), sipho.balance(shop)), (650, 350));
    // A wallet that isn't kept here can't be asked about.
    let elsewhere = Account::generate().id();
    assert_eq!(
        sipho.moved(elsewhere, transfer(me, 1), T0).unwrap_err(),
        why::NOT_OURS
    );

    // cadrs starts again: what was handed over to be kept is the books.
    let kept = |wallet: AccountId, device: &Device| -> Vec<Effect> {
        device
            .kept
            .iter()
            .filter(|(w, _)| *w == wallet)
            .map(|(_, effect)| effect.clone())
            .collect()
    };
    let cert = sipho.account.certify(sipho.key.public(), "laptop", T0);
    let mut again = Trusting::new(SecretKey::from_bytes(&sipho.key.to_bytes()), cert);
    again.restore(me, false, &kept(me, &sipho));
    again.restore(shop, true, &kept(shop, &sipho));
    let balance = |bank: &Trusting, wallet: &AccountId| bank.book(wallet).unwrap().balance(wallet);
    assert_eq!((balance(&again, &me), balance(&again, &shop)), (650, 350));
    assert_eq!(again.book(&shop).unwrap().len(), 2);
}

#[test]
fn strangers_notes_have_a_ceiling_and_a_long_history_comes_in_pages() {
    let (mut sipho, mut stranger) = (Device::new(), Device::new());
    let (s, x) = (sipho.id(), stranger.id());
    stranger.faucet(T0).unwrap();
    for _ in 0..MAX_NOTES_A_DAY {
        stranger.moved(x, transfer(s, 1), T0).unwrap();
    }
    stranger.deliver(&mut sipho, T0);
    assert_eq!(sipho.balance(s), MAX_NOTES_A_DAY as u64);
    stranger.moved(x, transfer(s, 1), T0).unwrap();
    let note = stranger.told.pop().unwrap().1;
    let refused = sipho.receive(&note, T0 + 1).unwrap_err();
    assert_eq!(Refusal::of(&refused), Some(Refusal::TooMany));
    // The next day there is room again.
    sipho.receive(&note, T0 + 24 * 60 * 60 * 1000).unwrap();
    // Newest first, a page at a time.
    let mut seen = Vec::new();
    let mut before = None;
    loop {
        let ask = Ask::Statement {
            before,
            holds: vec![],
        };
        let Ok(Answer::Statement(page)) = sipho.ask_as(s, true, ask, T0) else {
            panic!("a statement");
        };
        assert!(page.movements.len() <= PAGE);
        assert!(page.movements.windows(2).all(|w| w[0].seq > w[1].seq));
        before = page.movements.last().map(|m| m.seq);
        seen.extend(page.movements);
        if !page.more {
            break;
        }
    }
    assert_eq!(seen.len(), MAX_NOTES_A_DAY + 1);
}

/// Whatever one device asks of its own books, in whatever order: nothing goes below zero and
/// no credit appears from nowhere (only the faucet makes them).
#[test]
fn a_devices_own_books_add_up_whatever_is_asked() {
    let mut device = Device::new();
    let me = device.id();
    let shop = Account::generate().id();
    device.bank.restore(shop, true, &[]);
    let others: Vec<AccountId> = (0..3).map(|_| Account::generate().id()).collect();
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut holds: Vec<MoveId> = Vec::new();
    let mut now = T0;
    for _ in 0..3000 {
        now += next(20 * 60 * 1000);
        let wallet = [me, shop][next(2) as usize];
        let to = [me, shop, others[next(3) as usize]][next(3) as usize];
        let id = new_id();
        let amount = [0, 1, 7, 250, 999, 1000, 5000, u64::MAX][next(8) as usize];
        let some_hold = holds
            .get(next(1000) as usize % holds.len().max(1))
            .copied()
            .unwrap_or(id);
        let ask = match next(7) {
            0 | 1 => Ask::Faucet { id },
            2 | 3 => Ask::Transfer {
                id,
                to,
                amount,
                note: String::new(),
                about: None,
                payout: next(2) == 0,
            },
            4 => {
                holds.push(cadrs_credits::scoped_id(&wallet, &id));
                hold(id, to, amount)
            }
            5 => Ask::Release {
                id,
                hold: some_hold,
            },
            _ => Ask::Return {
                id,
                hold: some_hold,
            },
        };
        let _ = device.moved(wallet, ask, now);
        // Every credit in the two wallets kept here, held or not, and everything paid away,
        // came from the faucet.
        let total = |book: &cadrs_credits::book::Book| (book.total(), book.minted());
        let mine = total(device.bank.book(&me).unwrap());
        assert_eq!(mine.0, mine.1, "the person's own books");
    }
    assert!(device.bank.book(&me).unwrap().len() > 300);
}
