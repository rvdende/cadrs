//! A bank from the outside: what devices ask and what it answers.

use cadrs_credits::bank::{ASKS_A_MINUTE, Bank};
use cadrs_credits::refusal as why;
use cadrs_credits::{
    About, AccountId, Answer, Ask, CLAIM_AFTER, CLOCK_SLACK, Credits, FAUCET, FAUCET_EVERY, Kind,
    MAX_AMOUNT, MoveId, Movement, PAGE, Refusal, Statement, new_id,
};
use cadrs_social::keys::{Account, DeviceCert, SecretKey, Signed};

const T0: u64 = 1_800_000_000_000;

/// Someone with one device.
struct Person {
    account: Account,
    device: SecretKey,
    cert: Signed<DeviceCert>,
}

impl Person {
    fn new() -> Self {
        let account = Account::generate();
        let device = SecretKey::generate();
        let cert = account.certify(device.public(), "laptop", T0);
        Self {
            account,
            device,
            cert,
        }
    }

    fn id(&self) -> AccountId {
        self.account.id()
    }

    fn asked(&self, bank: &Bank, wallet: AccountId, ask: Ask, now: u64) -> Vec<u8> {
        cadrs_credits::ask(&self.device, &self.cert, bank.id(), wallet, ask, now)
    }

    /// Asks about `wallet` from this person's device.
    fn ask(
        &self,
        bank: &mut Bank,
        wallet: AccountId,
        ask: Ask,
        now: u64,
    ) -> Result<Answer, String> {
        let bytes = self.asked(bank, wallet, ask, now);
        let handled = bank.handle(&bytes, Some(self.device.public()), now)?;
        cadrs_credits::read_answer(&handled.answer, &bank.id())
    }

    fn moved(
        &self,
        bank: &mut Bank,
        wallet: AccountId,
        ask: Ask,
        now: u64,
    ) -> Result<Movement, String> {
        match self.ask(bank, wallet, ask, now)? {
            Answer::Receipt(receipt) => cadrs_credits::open_receipt(&receipt, &bank.id()),
            other => panic!("a receipt, not {other:?}"),
        }
    }

    fn faucet(&self, bank: &mut Bank, now: u64) -> Result<Movement, String> {
        self.moved(bank, self.id(), Ask::Faucet { id: new_id() }, now)
    }

    fn statement(&self, bank: &mut Bank, wallet: AccountId, now: u64) -> Statement {
        match self.ask(
            bank,
            wallet,
            Ask::Statement {
                before: None,
                holds: Vec::new(),
            },
            now,
        ) {
            Ok(Answer::Statement(statement)) => statement,
            other => panic!("a statement, not {other:?}"),
        }
    }

    fn pay(
        &self,
        bank: &mut Bank,
        to: AccountId,
        amount: Credits,
        now: u64,
    ) -> Result<Movement, String> {
        self.moved(bank, self.id(), transfer(new_id(), to, amount), now)
    }
}

/// What the bank calls the hold `payer` asked for as `id`.
fn held_as(payer: &Person, id: MoveId) -> MoveId {
    cadrs_credits::scoped_id(&payer.id(), &id)
}

fn transfer(id: MoveId, to: AccountId, amount: Credits) -> Ask {
    Ask::Transfer {
        id,
        to,
        amount,
        note: String::new(),
        about: None,
        payout: false,
    }
}

fn bank() -> Bank {
    Bank::in_memory(SecretKey::generate())
}

/// An organization: its key says who spends and who looks.
fn organization(bank: &mut Bank, spenders: &[&Person], viewers: &[&Person], now: u64) -> Account {
    let org = Account::generate();
    let ask = Ask::Access {
        spenders: spenders.iter().map(|p| p.id()).collect(),
        viewers: viewers.iter().map(|p| p.id()).collect(),
    };
    let bytes = cadrs_credits::ask_as(org.key(), bank.id(), ask, now);
    bank.handle(&bytes, None, now)
        .expect("the organization's own word");
    org
}

#[test]
fn the_faucet_gives_a_thousand_once_an_hour() {
    let mut bank = bank();
    let thandi = Person::new();
    // Nothing yet, and the faucet is ready.
    let before = thandi.statement(&mut bank, thandi.id(), T0);
    assert_eq!((before.balance, before.faucet_at), (0, Some(T0)));
    let got = thandi.faucet(&mut bank, T0).unwrap();
    assert_eq!(
        (got.kind, got.amount, got.from, got.to),
        (Kind::Faucet, FAUCET, None, thandi.id())
    );
    let after = thandi.statement(&mut bank, thandi.id(), T0 + 1);
    assert_eq!(after.balance, 1000);
    assert_eq!(after.faucet_at, Some(T0 + FAUCET_EVERY));
    assert_eq!(after.movements.len(), 1);
    // Not again within the hour, to the last millisecond; then again.
    let early = thandi.faucet(&mut bank, T0 + FAUCET_EVERY - 1).unwrap_err();
    assert_eq!(Refusal::of(&early), Some(Refusal::FaucetNotYet));
    thandi.faucet(&mut bank, T0 + FAUCET_EVERY).unwrap();
    assert_eq!(bank.book().balance(&thandi.id()), 2000);
    // Nobody takes for someone else.
    let marco = Person::new();
    let other = marco.moved(&mut bank, thandi.id(), Ask::Faucet { id: new_id() }, T0);
    assert_eq!(Refusal::of(&other.unwrap_err()), Some(Refusal::NotYours));
}

#[test]
fn a_transfer_moves_credits_at_once_and_both_see_it() {
    let mut bank = bank();
    let (marco, sipho) = (Person::new(), Person::new());
    marco.faucet(&mut bank, T0).unwrap();
    // Sipho's device has looked at his wallet: it is told when he is paid.
    sipho.statement(&mut bank, sipho.id(), T0);
    let ask = Ask::Transfer {
        id: new_id(),
        to: sipho.id(),
        amount: 724,
        note: "  for the bracket ".into(),
        about: Some(About::order(7, "R-0007")),
        payout: false,
    };
    let bytes = marco.asked(&bank, marco.id(), ask, T0 + 5);
    let handled = bank
        .handle(&bytes, Some(marco.device.public()), T0 + 5)
        .unwrap();
    assert_eq!(handled.pushes.len(), 1);
    assert_eq!(handled.pushes[0].to, [sipho.device.public()]);
    let told = cadrs_credits::read_credited(&handled.pushes[0].receipt, &bank.id()).unwrap();
    assert_eq!((told.amount, told.note.as_str()), (724, "for the bracket"));
    assert_eq!(told.about.as_ref().and_then(About::order_id), Some(7));
    assert_eq!(told.change_for(&sipho.id()), 724);
    assert_eq!(told.change_for(&marco.id()), -724);
    assert_eq!(told.by, marco.id());
    // A word that isn't this bank's is not believed.
    let other = SecretKey::generate().public();
    assert!(cadrs_credits::read_credited(&handled.pushes[0].receipt, &other).is_err());

    let (m, s) = (
        marco.statement(&mut bank, marco.id(), T0 + 6),
        sipho.statement(&mut bank, sipho.id(), T0 + 6),
    );
    assert_eq!((m.balance, s.balance), (276, 724));
    assert_eq!(m.movements[0].seq, s.movements[0].seq);
    assert_eq!(s.movements[0].other(&sipho.id()), Some(marco.id()));
    // What can't be paid isn't.
    for (amount, why) in [
        (277, Refusal::NotEnough),
        (0, Refusal::Amount),
        (MAX_AMOUNT + 1, Refusal::Amount),
    ] {
        let refused = marco
            .pay(&mut bank, sipho.id(), amount, T0 + 7)
            .unwrap_err();
        assert_eq!(Refusal::of(&refused), Some(why), "{amount}");
    }
    let own = marco.pay(&mut bank, marco.id(), 1, T0 + 7).unwrap_err();
    assert_eq!(Refusal::of(&own), Some(Refusal::SameWallet));
    let long = Ask::Transfer {
        id: new_id(),
        to: sipho.id(),
        amount: 1,
        note: "x".repeat(141),
        about: None,
        payout: false,
    };
    let refused = marco
        .moved(&mut bank, marco.id(), long, T0 + 7)
        .unwrap_err();
    assert_eq!(Refusal::of(&refused), Some(Refusal::Note));
    assert_eq!(bank.book().total(), bank.book().minted());
}

#[test]
fn asked_twice_is_paid_once() {
    let mut bank = bank();
    let (marco, sipho) = (Person::new(), Person::new());
    marco.faucet(&mut bank, T0).unwrap();
    let id = new_id();
    let first = marco
        .moved(&mut bank, marco.id(), transfer(id, sipho.id(), 300), T0)
        .unwrap();
    // The answer was lost; the device asks again, later.
    let again = marco
        .moved(
            &mut bank,
            marco.id(),
            transfer(id, sipho.id(), 300),
            T0 + 9000,
        )
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(bank.book().balance(&sipho.id()), 300);
    assert_eq!(bank.book().len(), 2);
    // The same id for something else, from the same wallet, is refused.
    let hold = Ask::Hold {
        id,
        to: sipho.id(),
        amount: 5,
        note: String::new(),
        about: None,
    };
    assert_eq!(
        marco.moved(&mut bank, marco.id(), hold, T0).unwrap_err(),
        why::ID_USED
    );
    // Someone else's ids are their own: the same one in another wallet's name is another id.
    sipho.faucet(&mut bank, T0).unwrap();
    let theirs = sipho.moved(&mut bank, sipho.id(), transfer(id, marco.id(), 1), T0);
    assert_ne!(theirs.unwrap().id, first.id);
    assert_eq!(first.id, cadrs_credits::scoped_id(&marco.id(), &id));
}

#[test]
fn an_organizations_wallet_is_spent_by_those_its_key_names() {
    let mut bank = bank();
    let (owner, admin, member, stranger) =
        (Person::new(), Person::new(), Person::new(), Person::new());
    let shop = organization(&mut bank, &[&owner, &admin], &[&member], T0);
    let shop_id = shop.id();
    // Credits come in from a client.
    stranger.faucet(&mut bank, T0).unwrap();
    stranger.pay(&mut bank, shop_id, 900, T0).unwrap();
    // Everyone the key names sees the wallet; a stranger doesn't.
    for (who, may_spend) in [(&owner, true), (&admin, true), (&member, false)] {
        let seen = who.statement(&mut bank, shop_id, T0 + 1);
        assert_eq!(
            (seen.balance, seen.may_spend, seen.organization),
            (900, may_spend, true)
        );
        assert_eq!(seen.faucet_at, None, "an organization has no faucet");
    }
    let refused = stranger.ask(
        &mut bank,
        shop_id,
        Ask::Statement {
            before: None,
            holds: Vec::new(),
        },
        T0 + 1,
    );
    assert_eq!(refused.unwrap_err(), why::NOT_YOURS_TO_SEE);
    // A payout to a member, by an admin: called a payout, and said to be the admin's doing.
    let payout = Ask::Transfer {
        id: new_id(),
        to: member.id(),
        amount: 250,
        note: "November".into(),
        about: None,
        payout: true,
    };
    let paid = admin
        .moved(&mut bank, shop_id, payout.clone(), T0 + 2)
        .unwrap();
    assert_eq!(
        (paid.kind, paid.from, paid.by),
        (Kind::Payout, Some(shop_id), admin.id())
    );
    assert_eq!(bank.book().balance(&member.id()), 250);
    // A member who may only look can't; nor a stranger; nor is the faucet an organization's.
    for who in [&member, &stranger] {
        let refused = who.moved(&mut bank, shop_id, transfer(new_id(), who.id(), 1), T0 + 3);
        assert_eq!(Refusal::of(&refused.unwrap_err()), Some(Refusal::NotYours));
    }
    let faucet = owner.moved(&mut bank, shop_id, Ask::Faucet { id: new_id() }, T0 + 3);
    assert_eq!(Refusal::of(&faucet.unwrap_err()), Some(Refusal::NotYours));
    // A person's payment flagged as a payout is a transfer: only an organization pays out.
    owner.faucet(&mut bank, T0 + 3).unwrap();
    let Ask::Transfer { to, amount, .. } = payout else {
        unreachable!()
    };
    let own = Ask::Transfer {
        id: new_id(),
        to,
        amount,
        note: String::new(),
        about: None,
        payout: true,
    };
    assert_eq!(
        owner
            .moved(&mut bank, owner.id(), own, T0 + 4)
            .unwrap()
            .kind,
        Kind::Transfer
    );

    // The admin is let go: the key says so, and the wallet is closed to them at once. An
    // older list doesn't bring them back.
    let narrower = Ask::Access {
        spenders: vec![owner.id()],
        viewers: vec![],
    };
    let bytes = cadrs_credits::ask_as(shop.key(), bank.id(), narrower, T0 + 10);
    bank.handle(&bytes, None, T0 + 10).unwrap();
    let refused = admin.moved(
        &mut bank,
        shop_id,
        transfer(new_id(), admin.id(), 600),
        T0 + 11,
    );
    assert_eq!(Refusal::of(&refused.unwrap_err()), Some(Refusal::NotYours));
    let older = Ask::Access {
        spenders: vec![owner.id(), admin.id()],
        viewers: vec![],
    };
    let bytes = cadrs_credits::ask_as(shop.key(), bank.id(), older, T0 + 5);
    assert_eq!(
        bank.handle(&bytes, None, T0 + 11).unwrap_err(),
        why::OLD_ACCESS
    );
    // The admin's device looked at the wallet while it could: it is told nothing now.
    owner.statement(&mut bank, shop_id, T0 + 11);
    let bytes = stranger.asked(
        &bank,
        stranger.id(),
        transfer(new_id(), shop_id, 10),
        T0 + 11,
    );
    let handled = bank
        .handle(&bytes, Some(stranger.device.public()), T0 + 11)
        .unwrap();
    let told: Vec<_> = handled.pushes.iter().flat_map(|p| p.to.clone()).collect();
    assert!(told.contains(&owner.device.public()));
    assert!(
        !told.contains(&admin.device.public()),
        "let go, and still told"
    );
    assert!(!told.contains(&member.device.public()));
    // The same list said again changes nothing, and nothing is written for it.
    let moves = bank.book().len();
    let same = Ask::Access {
        spenders: vec![owner.id()],
        viewers: vec![],
    };
    let bytes = cadrs_credits::ask_as(shop.key(), bank.id(), same, T0 + 12);
    let handled = bank.handle(&bytes, None, T0 + 12).unwrap();
    assert_eq!(
        cadrs_credits::read_answer(&handled.answer, &bank.id()),
        Ok(Answer::Done)
    );
    assert_eq!(bank.book().len(), moves);
    // The longest list an organization may say fits in an ask.
    let everyone =
        |n: usize| -> Vec<AccountId> { (0..n).map(|_| SecretKey::generate().public()).collect() };
    let large = Ask::Access {
        spenders: everyone(cadrs_credits::MAX_SPENDERS),
        viewers: everyone(cadrs_credits::MAX_VIEWERS),
    };
    let big = Account::generate();
    let bytes = cadrs_credits::ask_as(big.key(), bank.id(), large, T0 + 12);
    assert!(
        bytes.len() <= cadrs_credits::MAX_ASKED,
        "{} bytes",
        bytes.len()
    );
    bank.handle(&bytes, None, T0 + 12)
        .expect("the longest list");
    let too_many = Ask::Access {
        spenders: everyone(cadrs_credits::MAX_SPENDERS + 1),
        viewers: vec![],
    };
    let bytes = cadrs_credits::ask_as(big.key(), bank.id(), too_many, T0 + 13);
    assert_eq!(bank.handle(&bytes, None, T0 + 13).unwrap_err(), why::ACCESS);
    // Nobody but the organization's key says who spends: not an owner's device.
    let grab = Ask::Access {
        spenders: vec![stranger.id()],
        viewers: vec![],
    };
    let refused = owner.ask(&mut bank, shop_id, grab, T0 + 12).unwrap_err();
    assert_eq!(refused, why::ACCESS);
    assert_eq!(bank.book().total(), bank.book().minted());
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
fn a_held_payment_is_released_by_the_payer_or_returned_by_the_payee() {
    let mut bank = bank();
    let (marco, owner) = (Person::new(), Person::new());
    let shop = organization(&mut bank, &[&owner], &[], T0).id();
    marco.faucet(&mut bank, T0).unwrap();

    // Held: out of Marco's balance, not yet in the shop's.
    let first = new_id();
    let held = marco
        .moved(&mut bank, marco.id(), hold(first, shop, 724), T0 + 1)
        .unwrap();
    assert_eq!(
        (
            held.kind,
            held.change_for(&marco.id()),
            held.change_for(&shop)
        ),
        (Kind::Held, -724, 0)
    );
    let (m, s) = (
        marco.statement(&mut bank, marco.id(), T0 + 2),
        owner.statement(&mut bank, shop, T0 + 2),
    );
    assert_eq!(
        (m.balance, m.held_out, s.balance, s.held_in),
        (276, 724, 0, 724)
    );
    assert_eq!((m.holds.len(), s.holds.len()), (1, 1));
    assert_eq!(s.holds[0].about.as_ref().and_then(About::order_id), Some(7));
    // Neither takes it alone the other way, and the shop can't claim it yet.
    let first_hold = held_as(&marco, first);
    let release = |id| Ask::Release {
        id,
        hold: first_hold,
    };
    let give_back = |id| Ask::Return {
        id,
        hold: first_hold,
    };
    let claim = |id| Ask::Claim {
        id,
        hold: first_hold,
    };
    assert_eq!(
        owner
            .moved(&mut bank, shop, release(new_id()), T0 + 3)
            .unwrap_err(),
        why::NOT_PAYER
    );
    assert_eq!(
        marco
            .moved(&mut bank, marco.id(), give_back(new_id()), T0 + 3)
            .unwrap_err(),
        why::NOT_PAYEE
    );
    assert_eq!(
        owner
            .moved(&mut bank, shop, claim(new_id()), T0 + 3)
            .unwrap_err(),
        why::TOO_EARLY
    );
    // The goods arrive: Marco releases it, and the shop is paid. Once.
    let paid = marco
        .moved(&mut bank, marco.id(), release(new_id()), T0 + 4)
        .unwrap();
    assert_eq!(
        (paid.kind, paid.change_for(&shop), paid.hold),
        (Kind::Released, 724, Some(first_hold))
    );
    assert_eq!(bank.book().balance(&shop), 724);
    for ask in [release(new_id()), give_back(new_id())] {
        let wallet = if matches!(ask, Ask::Release { .. }) {
            marco.id()
        } else {
            shop
        };
        let who = if wallet == shop { &owner } else { &marco };
        assert_eq!(
            who.moved(&mut bank, wallet, ask, T0 + 5).unwrap_err(),
            why::SETTLED
        );
    }
    let after = marco.statement(&mut bank, marco.id(), T0 + 6);
    assert_eq!(
        (after.balance, after.held_out, after.holds.len()),
        (276, 0, 0)
    );

    // Another order, which the shop won't make: it gives the credits back.
    let second = new_id();
    marco
        .moved(&mut bank, marco.id(), hold(second, shop, 200), T0 + 7)
        .unwrap();
    let back = owner
        .moved(
            &mut bank,
            shop,
            Ask::Return {
                id: new_id(),
                hold: held_as(&marco, second),
            },
            T0 + 8,
        )
        .unwrap();
    assert_eq!(
        (
            back.kind,
            back.change_for(&marco.id()),
            back.change_for(&shop)
        ),
        (Kind::Returned, 200, 0)
    );
    assert_eq!(bank.book().balance(&marco.id()), 276);

    // A third, which Marco never settles: after thirty days the shop may take it.
    let third = new_id();
    marco
        .moved(&mut bank, marco.id(), hold(third, shop, 76), T0 + 9)
        .unwrap();
    let late = T0 + 9 + CLAIM_AFTER;
    let early = owner.moved(
        &mut bank,
        shop,
        Ask::Claim {
            id: new_id(),
            hold: held_as(&marco, third),
        },
        late - 1,
    );
    assert_eq!(Refusal::of(&early.unwrap_err()), Some(Refusal::TooEarly));
    let took = owner
        .moved(
            &mut bank,
            shop,
            Ask::Claim {
                id: new_id(),
                hold: held_as(&marco, third),
            },
            late,
        )
        .unwrap();
    assert_eq!(took.kind, Kind::Claimed);
    assert_eq!(bank.book().balance(&shop), 800);
    let none = owner.moved(
        &mut bank,
        shop,
        Ask::Claim {
            id: new_id(),
            hold: new_id(),
        },
        late,
    );
    assert_eq!(Refusal::of(&none.unwrap_err()), Some(Refusal::NoHold));
    assert_eq!(bank.book().total(), bank.book().minted());
}

#[test]
fn a_request_is_its_devices_this_banks_and_this_minutes() {
    let mut bank = bank();
    let (marco, thief) = (Person::new(), Person::new());
    marco.faucet(&mut bank, T0).unwrap();
    let bytes = marco.asked(&bank, marco.id(), transfer(new_id(), thief.id(), 500), T0);
    // Sent on by another device.
    let overheard = bank.handle(&bytes, Some(thief.device.public()), T0);
    assert_eq!(overheard.unwrap_err(), why::SIGNATURE);
    // Kept for later.
    let later = bank.handle(&bytes, Some(marco.device.public()), T0 + CLOCK_SLACK + 1);
    assert_eq!(Refusal::of(&later.unwrap_err()), Some(Refusal::Clock));
    // Taken to another bank.
    let mut other = Bank::in_memory(SecretKey::generate());
    let elsewhere = other.handle(&bytes, Some(marco.device.public()), T0);
    assert_eq!(elsewhere.unwrap_err(), why::OTHER_BANK);
    // A device with a certificate from another account, or one the thief wrote for it.
    let forged = thief.account.certify(marco.device.public(), "mine now", T0);
    let ask = transfer(new_id(), thief.id(), 500);
    for cert in [thief.cert.clone(), forged] {
        let bytes =
            cadrs_credits::ask(&marco.device, &cert, bank.id(), marco.id(), ask.clone(), T0);
        let refused = bank
            .handle(&bytes, Some(marco.device.public()), T0)
            .unwrap_err();
        assert!(
            refused == why::SIGNATURE || refused == why::NOT_YOURS,
            "{refused}"
        );
    }
    // The thief's own device, saying the wallet is Marco's.
    let refused = thief.moved(&mut bank, marco.id(), ask, T0).unwrap_err();
    assert_eq!(refused, why::NOT_YOURS);
    // Garbage, and megabytes of it.
    assert!(bank.handle(b"not an ask", None, T0).is_err());
    assert_eq!(
        bank.handle(&vec![0x81; 1 << 20], None, T0).unwrap_err(),
        why::TOO_LARGE
    );
    assert_eq!(bank.book().balance(&marco.id()), 1000);
    assert_eq!(bank.book().balance(&thief.id()), 0);
}

#[test]
fn a_device_asks_only_so_often() {
    let mut bank = bank();
    let marco = Person::new();
    for _ in 0..ASKS_A_MINUTE {
        marco.statement(&mut bank, marco.id(), T0);
    }
    let refused = marco.ask(
        &mut bank,
        marco.id(),
        Ask::Statement {
            before: None,
            holds: Vec::new(),
        },
        T0,
    );
    assert_eq!(Refusal::of(&refused.unwrap_err()), Some(Refusal::TooOften));
    // The next minute it may again; another device wasn't held up.
    let other = Person::new();
    other.statement(&mut bank, other.id(), T0);
    marco.statement(&mut bank, marco.id(), T0 + 60_000);
}

#[test]
fn a_long_history_comes_a_page_at_a_time() {
    let mut bank = bank();
    let (marco, sipho) = (Person::new(), Person::new());
    marco.faucet(&mut bank, T0).unwrap();
    for i in 0..120 {
        marco.pay(&mut bank, sipho.id(), 1, T0 + i).unwrap();
    }
    let mut seen = Vec::new();
    let mut before = None;
    loop {
        let Ok(Answer::Statement(page)) = sipho.ask(
            &mut bank,
            sipho.id(),
            Ask::Statement {
                before,
                holds: Vec::new(),
            },
            T0 + 500,
        ) else {
            panic!("a statement");
        };
        assert!(page.movements.len() <= PAGE);
        assert!(
            page.movements.windows(2).all(|w| w[0].seq > w[1].seq),
            "newest first"
        );
        before = page.movements.last().map(|m| m.seq);
        seen.extend(page.movements);
        if !page.more {
            break;
        }
    }
    assert_eq!(seen.len(), 120);
    assert_eq!(bank.book().balance(&sipho.id()), 120);
}

#[test]
fn the_books_are_what_was_written_down() {
    let dir = std::env::temp_dir().join(format!("cadrs-bank-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let key = SecretKey::generate();
    let (marco, owner) = (Person::new(), Person::new());
    let held = new_id();
    let shop = {
        let mut bank = Bank::open(SecretKey::from_bytes(&key.to_bytes()), &dir).unwrap();
        let shop = organization(&mut bank, &[&owner], &[], T0).id();
        marco.faucet(&mut bank, T0).unwrap();
        marco.pay(&mut bank, shop, 100, T0 + 1).unwrap();
        marco
            .moved(&mut bank, marco.id(), hold(held, shop, 300), T0 + 2)
            .unwrap();
        shop
    };
    // The bank stops and starts: the same balances, the same hold, the same hour to wait.
    let mut bank = Bank::open(SecretKey::from_bytes(&key.to_bytes()), &dir).unwrap();
    let m = marco.statement(&mut bank, marco.id(), T0 + 3);
    assert_eq!(
        (m.balance, m.held_out, m.faucet_at),
        (600, 300, Some(T0 + FAUCET_EVERY))
    );
    assert_eq!(m.movements.len(), 3);
    let s = owner.statement(&mut bank, shop, T0 + 3);
    assert_eq!((s.balance, s.held_in, s.may_spend), (100, 300, true));
    marco
        .moved(
            &mut bank,
            marco.id(),
            Ask::Release {
                id: new_id(),
                hold: held_as(&marco, held),
            },
            T0 + 4,
        )
        .unwrap();
    drop(bank);

    // A crash halfway through writing: the half record is cut off (and kept beside), and
    // everything before it stands.
    let journal = dir.join("credits.journal");
    let whole = std::fs::read(&journal).unwrap();
    let mut torn = whole.clone();
    torn.extend_from_slice(&[200, 0, 0, 0, 9, 9, 9, 9, 9, 9, 9, 9, 1, 2, 3]);
    std::fs::write(&journal, &torn).unwrap();
    let mut bank = Bank::open(SecretKey::from_bytes(&key.to_bytes()), &dir).unwrap();
    assert_eq!(bank.book().balance(&shop), 400);
    assert_eq!(std::fs::read(&journal).unwrap(), whole);
    assert!(dir.join("credits.journal.cut").exists());
    marco.pay(&mut bank, shop, 1, T0 + 5).unwrap();
    drop(bank);
    let bank = Bank::open(SecretKey::from_bytes(&key.to_bytes()), &dir).unwrap();
    assert_eq!(bank.book().balance(&shop), 401);
    assert_eq!(bank.book().total(), bank.book().minted());
    assert_eq!(bank.cut, 0);
    drop(bank);
    // Torn again: the first piece kept aside isn't written over.
    let whole = std::fs::read(&journal).unwrap();
    let mut torn = whole.clone();
    torn.extend_from_slice(&[7, 0]);
    std::fs::write(&journal, &torn).unwrap();
    let bank = Bank::open(key, &dir).unwrap();
    assert_eq!(bank.cut, 2);
    assert_eq!(
        std::fs::read(dir.join("credits.journal.cut"))
            .unwrap()
            .len(),
        15
    );
    assert_eq!(
        std::fs::read(dir.join("credits.journal.cut.2")).unwrap(),
        [7, 0]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Damage anywhere but in a torn last record stops the bank from starting, and changes
/// nothing: one flipped bit must not quietly take everyone's balances back to an earlier day.
#[test]
fn a_damaged_journal_is_not_read_past_and_not_cut() {
    let dir = std::env::temp_dir().join(format!("cadrs-bank-damaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let key = SecretKey::generate();
    let open = |dir: &std::path::Path| Bank::open(SecretKey::from_bytes(&key.to_bytes()), dir);
    let people: Vec<Person> = (0..6).map(|_| Person::new()).collect();
    {
        let mut bank = open(&dir).unwrap();
        for person in &people {
            person.faucet(&mut bank, T0).unwrap();
        }
    }
    let journal = dir.join("credits.journal");
    let whole = std::fs::read(&journal).unwrap();
    let record = (whole.len() - 8) / 6;
    // One bit, in the second record of six.
    let mut flipped = whole.clone();
    flipped[8 + record + record / 2] ^= 0x10;
    std::fs::write(&journal, &flipped).unwrap();
    let refused = open(&dir).err().expect("a damaged journal isn't opened");
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidData);
    assert!(refused.to_string().contains("record 2"), "{refused}");
    assert_eq!(std::fs::read(&journal).unwrap(), flipped, "nothing is cut");
    assert!(!dir.join("credits.journal.cut").exists());
    // A length that can't be, in the middle.
    let mut long = whole.clone();
    long[8 + record..8 + record + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(&journal, &long).unwrap();
    assert!(open(&dir).is_err());
    assert_eq!(std::fs::read(&journal).unwrap(), long);
    // A length a little wrong, so that everything after it would look like one torn record.
    let mut short = whole.clone();
    short[8 + record] = short[8 + record].wrapping_add(40);
    std::fs::write(&journal, &short).unwrap();
    assert!(open(&dir).is_err());
    assert_eq!(std::fs::read(&journal).unwrap(), short);
    // A length near the end made longer than what is left: what follows is whole records, not
    // one torn one, and none of them is dropped.
    let mut long_tail = whole.clone();
    let fourth = 8 + 3 * record;
    long_tail[fourth + 1] = long_tail[fourth + 1].wrapping_add(8);
    std::fs::write(&journal, &long_tail).unwrap();
    let refused = open(&dir)
        .err()
        .expect("whole records follow: not a torn write");
    assert!(refused.to_string().contains("record 4"), "{refused}");
    assert_eq!(std::fs::read(&journal).unwrap(), long_tail);
    assert!(!dir.join("credits.journal.cut").exists());
    // Megabytes of something else after the last record are no torn write.
    let mut junk = whole.clone();
    junk.extend(std::iter::repeat_n(0xAB, 200_000));
    std::fs::write(&journal, &junk).unwrap();
    assert!(open(&dir).is_err());
    // A file that isn't a journal at all.
    std::fs::write(&journal, b"hello, I am not a journal").unwrap();
    assert!(open(&dir).is_err());
    // An effect a later cadrs wrote: whole, checksummed, and not this one's to read.
    #[derive(serde::Serialize)]
    enum Later {
        Interest { rate: u32 },
    }
    let later = cadrs_social::codec::encode(&Later::Interest { rate: 3 });
    let mut newer = whole.clone();
    newer.extend_from_slice(&(later.len() as u32).to_le_bytes());
    newer.extend_from_slice(&blake3::hash(&later).as_bytes()[..8]);
    newer.extend_from_slice(&later);
    std::fs::write(&journal, &newer).unwrap();
    let refused = open(&dir).err().expect("not read by an earlier cadrs");
    assert!(refused.to_string().contains("later cadrs"), "{refused}");
    assert_eq!(std::fs::read(&journal).unwrap(), newer);
    // Put back as it was, it is all there.
    std::fs::write(&journal, &whole).unwrap();
    let bank = open(&dir).unwrap();
    assert_eq!((bank.book().len(), bank.book().total()), (6, 6000));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Someone with a few credits can't stop a shop from being paid: what one payer holds for one
/// payee has a ceiling, what is held for a shop by everyone has none, and a statement lists
/// the largest holds first, so an order's isn't lost among strangers' single credits.
#[test]
fn strangers_holds_dont_stop_a_shop_being_paid() {
    use cadrs_credits::{MAX_HELD_BETWEEN, MAX_HOLDS_LISTED};
    let mut bank = bank();
    let (owner, client) = (Person::new(), Person::new());
    let shop = organization(&mut bank, &[&owner], &[], T0).id();
    // Fifteen accounts hold all they may for the shop, a credit at a time.
    let nuisances: Vec<Person> = (0..15).map(|_| Person::new()).collect();
    for nuisance in &nuisances {
        nuisance.faucet(&mut bank, T0).unwrap();
        for _ in 0..MAX_HELD_BETWEEN {
            nuisance
                .moved(&mut bank, nuisance.id(), hold(new_id(), shop, 1), T0)
                .unwrap();
        }
        let more = nuisance.moved(&mut bank, nuisance.id(), hold(new_id(), shop, 1), T0);
        assert_eq!(more.unwrap_err(), why::TOO_MANY_BETWEEN);
    }
    // A client's order is paid for all the same, and the shop sees it first.
    client.faucet(&mut bank, T0).unwrap();
    let order = new_id();
    client
        .moved(&mut bank, client.id(), hold(order, shop, 700), T0 + 1)
        .unwrap();
    let seen = owner.statement(&mut bank, shop, T0 + 2);
    assert_eq!(seen.open_holds as usize, 15 * MAX_HELD_BETWEEN + 1);
    assert_eq!(seen.holds.len(), MAX_HOLDS_LISTED);
    assert_eq!(
        (seen.holds[0].id, seen.holds[0].amount),
        (held_as(&client, order), 700)
    );
    assert_eq!(seen.held_in, 700 + 15 * MAX_HELD_BETWEEN as u64);
    // Two hundred strangers hold more than the order's each: it is no longer among the
    // largest listed, but the shop asks after its orders' holds by name, and is told.
    let rich: Vec<Person> = (0..MAX_HOLDS_LISTED).map(|_| Person::new()).collect();
    for person in &rich {
        person.faucet(&mut bank, T0).unwrap();
        person
            .moved(&mut bank, person.id(), hold(new_id(), shop, 1000), T0 + 2)
            .unwrap();
    }
    let hidden = owner.statement(&mut bank, shop, T0 + 3);
    assert!(
        hidden.holds.iter().all(|h| h.amount == 1000),
        "out of sight"
    );
    let named = held_as(&client, order);
    let asking = |holds: Vec<MoveId>| Ask::Statement {
        before: None,
        holds,
    };
    let Ok(Answer::Statement(told)) =
        owner.ask(&mut bank, shop, asking(vec![named, new_id()]), T0 + 3)
    else {
        panic!("a statement");
    };
    assert_eq!(told.named.len(), 1);
    assert_eq!((told.named[0].id, told.named[0].amount), (named, 700));
    // Nobody is told of a hold they are no side of.
    let Ok(Answer::Statement(theirs)) =
        rich[0].ask(&mut bank, rich[0].id(), asking(vec![named]), T0 + 3)
    else {
        panic!("a statement");
    };
    assert!(theirs.named.is_empty());
    // The shop gives a stranger's back, which makes room for that stranger only.
    let junk = seen.holds[1].id;
    owner
        .moved(
            &mut bank,
            shop,
            Ask::Return {
                id: new_id(),
                hold: junk,
            },
            T0 + 3,
        )
        .unwrap();
    assert_eq!(bank.book().total(), bank.book().minted());
}

/// The receipt of a movement goes to its sides: knowing its id isn't enough to be given it,
/// and nobody can use up an id that someone else will ask with.
#[test]
fn a_receipt_is_for_a_side_of_the_movement() {
    use cadrs_credits::settle_id;
    let mut bank = bank();
    let (marco, sipho, nosy) = (Person::new(), Person::new(), Person::new());
    marco.faucet(&mut bank, T0).unwrap();
    nosy.faucet(&mut bank, T0).unwrap();
    let asked = new_id();
    let held = held_as(&marco, asked);
    marco
        .moved(&mut bank, marco.id(), hold(asked, sipho.id(), 300), T0)
        .unwrap();
    // Sipho's cadrs will return or claim it with ids that follow from the hold. Marco (and
    // anyone) can work those out, and uses them up for something else, in his own name.
    for kind in [Kind::Returned, Kind::Claimed] {
        let id = settle_id(&held, kind);
        marco
            .moved(&mut bank, marco.id(), transfer(id, nosy.id(), 1), T0)
            .unwrap();
        nosy.moved(&mut bank, nosy.id(), transfer(id, marco.id(), 1), T0)
            .unwrap();
    }
    // Someone else, with the very ids of Marco's asks: no receipt of his, and no hold of his.
    let release = settle_id(&held, Kind::Released);
    let theirs = nosy.moved(
        &mut bank,
        nosy.id(),
        Ask::Release {
            id: release,
            hold: held,
        },
        T0 + 1,
    );
    assert_eq!(theirs.unwrap_err(), why::NOT_PAYER);
    let theirs = nosy.moved(&mut bank, nosy.id(), hold(asked, marco.id(), 1), T0 + 1);
    assert_ne!(theirs.unwrap().id, held);
    // Sipho gives it back, with the id his cadrs uses: it wasn't used up.
    let give_back = Ask::Return {
        id: settle_id(&held, Kind::Returned),
        hold: held,
    };
    let back = sipho.moved(&mut bank, sipho.id(), give_back.clone(), T0 + 2);
    assert_eq!(back.unwrap().kind, Kind::Returned);
    // Asking again gets what was done; the payer asking to release it now is told it was
    // settled.
    let again = sipho.moved(&mut bank, sipho.id(), give_back, T0 + 3);
    assert_eq!(again.unwrap().kind, Kind::Returned);
    let late = marco.moved(
        &mut bank,
        marco.id(),
        Ask::Release {
            id: release,
            hold: held,
        },
        T0 + 3,
    );
    assert_eq!(late.unwrap_err(), why::SETTLED);
}

/// An order's hold has one id, whoever works it out and however often: paid once.
#[test]
fn an_orders_hold_is_asked_for_as_often_as_it_takes() {
    use cadrs_credits::{order_hold, order_hold_id, settle_id};
    let mut bank = bank();
    let (marco, sipho) = (Person::new(), Person::new());
    marco.faucet(&mut bank, T0).unwrap();
    let (request, quote) = (0x1234_u128, 0x99_u128);
    let id = order_hold_id(request, quote);
    assert_eq!(id, order_hold_id(request, quote));
    assert_ne!(id, order_hold_id(request, quote + 1));
    // The answer is lost; Accept and pay is pressed again, twice.
    for at in 0..3 {
        let held = marco
            .moved(&mut bank, marco.id(), hold(id, sipho.id(), 700), T0 + at)
            .unwrap();
        assert_eq!(held.kind, Kind::Held);
        // Both sides can work out what the bank calls it.
        assert_eq!(held.id, order_hold(&marco.id(), request, quote));
    }
    let seen = marco.statement(&mut bank, marco.id(), T0 + 5);
    assert_eq!(
        (seen.balance, seen.held_out, seen.holds.len()),
        (300, 700, 1)
    );
    // Released as often as it takes, once.
    let hold_id = order_hold(&marco.id(), request, quote);
    let release = settle_id(&hold_id, Kind::Released);
    assert_ne!(release, settle_id(&hold_id, Kind::Returned));
    for at in 0..3 {
        let ask = Ask::Release {
            id: release,
            hold: order_hold(&marco.id(), request, quote),
        };
        marco
            .moved(&mut bank, marco.id(), ask, T0 + 10 + at)
            .unwrap();
    }
    assert_eq!(bank.book().balance(&sipho.id()), 700);
    assert_eq!(bank.book().len(), 3);
}

/// Whatever is asked, in whatever order, by whoever: no credit appears or vanishes, nothing
/// goes below zero, and a bank started again from its journal has the same books.
#[test]
fn nothing_is_made_or_lost_whatever_is_asked() {
    let dir = std::env::temp_dir().join(format!("cadrs-bank-random-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let key = SecretKey::generate();
    let mut bank = Bank::open(SecretKey::from_bytes(&key.to_bytes()), &dir).unwrap();
    let people: Vec<Person> = (0..6).map(|_| Person::new()).collect();
    let shop = organization(&mut bank, &[&people[0], &people[1]], &[&people[2]], T0).id();
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut holds: Vec<MoveId> = Vec::new();
    let mut ids: Vec<MoveId> = Vec::new();
    let mut now = T0;
    let mut taken = 0;
    for _ in 0..4000 {
        now += next(20 * 60 * 1000);
        let who = &people[next(6) as usize];
        let wallets = [who.id(), shop, people[next(6) as usize].id()];
        let wallet = wallets[next(3) as usize];
        let to = [shop, people[next(6) as usize].id()][next(2) as usize];
        // Sometimes an id used before.
        let id = match (next(10), ids.is_empty()) {
            (0, false) => ids[next(ids.len() as u64) as usize],
            _ => new_id(),
        };
        ids.push(id);
        let amount = [0, 1, 7, 250, 999, 1000, 5000, u64::MAX][next(8) as usize];
        let some_hold = |pick: u64| {
            holds
                .get(pick as usize % holds.len().max(1))
                .copied()
                .unwrap_or(id)
        };
        let ask = match next(8) {
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
                hold: some_hold(next(1000)),
            },
            6 => Ask::Return {
                id,
                hold: some_hold(next(1000)),
            },
            _ => Ask::Claim {
                id,
                hold: some_hold(next(1000)),
            },
        };
        if who.ask(&mut bank, wallet, ask, now).is_ok() {
            taken += 1;
        }
        assert_eq!(bank.book().total(), bank.book().minted());
    }
    assert!(taken > 500, "most of it should not be refused: {taken}");
    let wallets: Vec<AccountId> = people.iter().map(Person::id).chain([shop]).collect();
    let balances =
        |bank: &Bank| -> Vec<Credits> { wallets.iter().map(|w| bank.book().balance(w)).collect() };
    let before = balances(&bank);
    let moves = bank.book().len();
    drop(bank);
    let again = Bank::open(key, &dir).unwrap();
    assert_eq!(balances(&again), before);
    assert_eq!(again.book().len(), moves);
    assert_eq!(again.book().total(), again.book().minted());
    let _ = std::fs::remove_dir_all(&dir);
}

/// What is signed and sent is read by a later cadrs and an earlier one: a movement of a kind
/// this one doesn't know is still a movement.
#[test]
fn a_kind_from_later_is_still_read() {
    #[derive(serde::Serialize)]
    struct Later {
        seq: u64,
        id: MoveId,
        time: u64,
        kind: &'static str,
        to: AccountId,
        amount: Credits,
        by: AccountId,
        fee: u64,
    }
    let who = SecretKey::generate().public();
    let bytes = cadrs_social::codec::encode(&Later {
        seq: 4,
        id: [7; 16],
        time: T0,
        kind: "Interest",
        to: who,
        amount: 12,
        by: who,
        fee: 1,
    });
    let read: Movement = cadrs_social::codec::decode(&bytes).unwrap();
    assert_eq!((read.kind, read.amount, read.from), (Kind::Other, 12, None));
    assert_eq!(read.change_for(&who), 0);
}
