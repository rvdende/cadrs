# cadrs_credits

Credits: the money cadrs uses until there is real payment. Test money, to try the whole of
buying and selling with: prices, paying, payments held for an order, wallets for people and
organizations.

## A bank is a trait

Everything in cadrs that touches credits (the Wallet screens, Transfer and Pay out, Accept and
pay, an organization's pages) knows only `Bank`:

```rust
pub trait Bank: Send + Sync {
    fn ask(&mut self, by: Asker, wallet: AccountId, ask: Ask, now: u64) -> Ticket;
    fn poll(&mut self, now: u64) -> Vec<Event>;
    fn receive(&mut self, note: &[u8], now: u64) -> Result<(), String>;
    fn restore(&mut self, wallet: AccountId, organization: bool, kept: &[Effect]);
}
```

- **`ask`** is a statement (balance, history, held payments), the faucet, a transfer, a hold, or
  a held payment's release, return or claim. The answer comes later, from `poll`, under the
  ticket: a ledger across a network takes its time.
- **`poll`** says what happened: answers, credits that arrived, and the errands a bank that
  keeps nothing itself needs run (`Event::Tell`: a note for the other side; `Event::Keep`:
  something to store and hand back through `restore`). A bank with a ledger of its own emits
  neither.
- The app's side is `cadrs_app::credits` (asks, keeps the answers for the screens, runs the
  errands) and `wallet_ui`. Another bank is put in with `cadrs_app::credits::use_bank`.

## Today's bank trusts everyone

`trusting::Trusting` is no bank at all:

- Each device keeps the books of its own wallets (`book::Book`): the user's in a file beside
  their keys, an organization's in the organization's document (`cadrs_org::Op::Credits`), so
  that its members' devices share one wallet.
- A payment is told to the other side with a note the paying device signs (`write_note`, sent
  as `Post::Credited`), and whoever gets a note believes it. cadrs sends a note again, less and
  less often, while the other side can't be reached.
- The faucet gives a person 1000 credits an hour. Not an organization.

What that means, said plainly:

- **Nobody checks that the payer had the credits.** A changed cadrs can send notes for credits
  it never had, and they are believed. (There is a test that does exactly that, so that nobody
  takes this for more than it is.)
- **Two devices of one account each have a balance of their own.** An organization's wallet is
  shared, because it is kept in its document.
- **Who may spend an organization's credits is decided by the app** (its owners and admins),
  not enforced by anything: any member's device can write to the organization's document.
- **A note that never arrives is credits that didn't.** Notes wait in memory while cadrs runs;
  one still waiting when cadrs closes is not sent again.

What it does keep to, so that honest use behaves like the real thing:

- A note is believed only when it gives to a wallet kept here (credits, a held payment, a
  release, a return), never when it takes from one. A claim of a held payment is believed only
  for a hold these books know, after its thirty days.
- Asking twice moves once. A movement has an id its asker chose, kept in the asker's wallet's
  name (`scoped_id`), so nobody can use up an id someone else will ask with. An order's hold has
  an id worked out from the order (`order_hold_id`, `order_hold`), and settling one an id worked
  out from the hold (`settle_id`).
- A held payment leaves the payer's balance and waits: the payer releases it, or the payee
  returns it; neither takes it alone the other way. A payee may claim one nobody settled after
  thirty days.
- A wallet takes 500 notes a day (each is kept for good), a note is at most 4 KiB, a payer has
  at most 200 payments held and 20 for any one payee, and a statement lists the 200 largest
  held payments and says of any it is asked after by name.

## Replacing it (Solana)

Implement `Bank`:

| | Trusting | A Solana client |
|---|---|---|
| A wallet | An account's id | The same Ed25519 key is a Solana address |
| Faucet | Added locally, once an hour | A devnet airdrop, then a token mint |
| Transfer, payout | A local entry and a note | A token transfer |
| Held payment | Entries and notes | An escrow program |
| Statement | The local books | RPC queries |
| Credits arriving | A note from the payer | An account subscription (`Event::Credited`) |
| `Event::Tell`, `Event::Keep`, `receive`, `restore` | How it gets by without a ledger | Not needed: nothing to tell, nothing to keep |

The screens, the order flow and `cadrs_app::credits` don't change.

## Tests

`cargo test -r -p cadrs_credits` (two devices exchanging notes: the faucet, payments, what is
and isn't believed, held payments, an organization's wallet beside its owner's, restoring from
what was kept); `cargo test -r -p cadrs_app --test credits_flow` (two running apps over local
iroh: a client and a shop, through a paid order, with a note delivered late);
`cargo run -r -- --headless --scenario wallet`.
