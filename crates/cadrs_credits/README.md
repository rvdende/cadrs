# cadrs_credits

Credits: the money cadrs uses until there is real payment. Play money, kept properly: nothing
is made or lost, every movement has a receipt, and what a stranger can ask for is bounded.

## The pieces

| Piece | Where | What it is |
|---|---|---|
| The books | `book::Book` | Every wallet's balance, as a state machine. `decide` says what an ask does (an `Effect`, or why not); `apply` does it. |
| The bank | `bank::Bank` | The books, the key that signs their receipts, and the journal every effect is written to before it counts. |
| A bank on a node | `cadrs_net::bank` | Answers `Post::Bank` and tells the devices of whoever was paid (`Post::Credited`). The seed peer keeps one (`cadrs --public-seed`, books in `<data dir>/bank/`), so the default bank's id is the seed's cadrs ID. |
| In the app | `cadrs_app::credits`, `wallet_ui` | Asks the bank, keeps what it answered, takes what it is told; the Wallet screen, Transfer and Pay out, Accept and pay. |

`CADRS_BANK=<cadrs ID>` asks another bank; `CADRS_BANK=here` keeps one in the app, in memory (a
scenario's, a test's: `Custom("org bank")`); `CADRS_BANK=0` none.

## The rules

- A **wallet** belongs to an account. A person's is spent from by that person's devices (a
  request is signed by the device, with the account's certificate for it). An organization's is
  spent from by the accounts its own key names (`Ask::Access`: owners and admins spend, other
  members look); the device that holds the organization's key says so whenever roles change.
- The **faucet** gives a person 1000 credits, once an hour. Not an organization.
- A **transfer** moves credits at once. A **payout** is a transfer from an organization to a
  person, called what it is.
- A **held payment** (an order's) leaves the payer's balance and waits: the payer releases it
  to the payee (the goods arrived), or the payee returns it (it won't be made). Neither takes it
  alone the other way. A payee may claim a hold nobody settled after thirty days.
  An order's hold has an id worked out from the order (`order_hold_id`), and settling one has
  an id worked out from the hold (`settle_id`): asked for as often as it takes, done once.
- **Ids are each wallet's own.** The bank keeps what a wallet calls a movement in that
  wallet's name (`scoped_id`), so nobody can use up an id someone else will ask with, though
  both sides can work those out. A held payment is named by what its payer called it
  (`order_hold`), which is how the payee returns it and how either side asks what became of it
  (`Ask::Statement { holds }`, answered in `Statement::named`).
- The **journal** is read back strictly. A last record torn by a crash is cut off and kept
  beside it; anything else that doesn't read (a failed checksum with records after it, a length
  that can't be, an unreadable tail with a whole record anywhere in it, something a later cadrs
  wrote) stops the bank from starting and changes nothing, because reading on would quietly
  move everyone's balances.
- Asking twice moves once: every movement has an id its asker chose.
- Every movement comes with a **receipt** the bank signed, and the bank tells the devices that
  looked at either wallet in the last week as it happens.

## What anyone can make the bank do

| | Limit |
|---|---|
| An ask | 60 KiB (an organization's longest access list fits; everything else is a few hundred bytes), signed by the device that sends it (not one that overheard it), for this bank, within ten minutes of its clock |
| A device | 240 asks a minute |
| A wallet | 2000 payments a day; as a payer, 200 payments held at once and 20 for any one payee; a note of 140 characters |
| What is held *for* a wallet | No ceiling (one would let anyone with a few credits fill it and stop a shop being paid). A statement lists the 200 largest, largest first, and says of up to 200 more that the ask names, which is how a shop always sees its own orders' payments. The shop returns those that are for none of its orders in one go, and isn't told of them as they arrive |
| An amount | 1 to 1,000,000,000 credits |
| An organization's access list | 64 spenders, 1000 viewers, said by its own key, newest wins |

Not stopped: accounts cost nothing, so anyone can farm the faucet with many of them, and each
movement is a line in the journal for good. These are play credits; before they are worth
anything, the faucet goes or gains a cost, and the journal gets snapshots.

## Who keeps the ledger

The bank does, and has to be trusted with it: it could refuse someone, or make credits. What it
said can always be shown (the receipts). A ledger nobody keeps can take its place behind the
same asks (`Ask`, `Answer`), which is why the app talks to "a bank" by id and not to a server.

## Who may spend an organization's credits, and where that falls short

The bank believes the last list the organization's own key signed. Only a device that holds
that key can say it (the founder's, and any device the key was handed to), and it says it when
the organization's members change and once every run. So:

- **A removed admin can spend until the key's holder is next online.** Removing them changes
  the organization's document at once, on every member's device; the bank learns of it only
  from the key. If the founder's laptop is off for a week, so is the update. Until an
  organization's key can sign from more than one place (or the bank can read the organization's
  document), an organization that lets an admin go should open cadrs on the device that holds
  its key.
- **A device the organization's key certified is the organization**, as far as the bank can
  tell (an owner's own phone, given the key's certificate so it can speak for the shop): it
  spends whatever the list says, like the key itself. That certificate is handed out by the
  key's holder only.

## Not here yet

Device revocation at the bank (a revoked device can still spend until its account's key is
changed), a second place for an organization's key (see above), disputes over a held payment,
fees, interest, a real currency, journal snapshots.

## Tests

`cargo test -r -p cadrs_credits` (the bank from the outside: faucet timing, transfers, an
organization's access, held payments, strangers' holds, forgery and replay, a torn and a damaged
journal, and four thousand random asks that never make or lose a credit); `cargo test -r -p
cadrs_app --test credits_flow` (three running apps over local iroh: a bank, a client and a shop,
through a paid order, with an answer lost and the bank away when the delivery is signed for);
`cargo run -r -- --headless --scenario wallet`.
