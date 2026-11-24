# Work list (2026-11-23)

What is left to do, in the order I would do it. Sources: the open state of branch `social`
(PR #243), `docs/user-journeys.md` (the ten journeys and their gap table) and the deferred list
in `crates/cadrs_org/README.md`. Sizes are relative: S a sitting, M a few, L a feature of its
own, XL several features.

Marks: ☐ to do, ◐ partly there, ✔ done.

## Decisions needed from you

These change what gets built; each has my default, which I will use unless you say otherwise.

1. **Who keeps the credit ledger.** Built: a bank that is a cadrs peer, kept by the seed
   (`cadrs --public-seed`), signing a receipt for every movement, behind asks a peer-to-peer
   ledger can replace later. (Not a Cloudflare Worker as first written: one implementation in
   Rust, on the transport and keys cadrs already has.) **The seed on the Unraid server has to be
   updated to this build for it to work for everyone** (`tools/seed/README.md`); until then
   `CADRS_BANK=here` or a seed run locally.
2. **Prices in credits only, or a currency as well.** Default: credits only for now. A rate
   card's currency becomes "credits" and the three-letter currency field is hidden.
3. **Who may spend an organization's credits.** Default: owners and admins transfer and pay
   out; members see the balance and history; a payout needs no second approval.
4. **Does an organization use the faucet.** Default: no. People take credits and transfer them
   in; an organization earns the rest.
5. **Library-only material names.** Today a name that isn't in the list can still be typed.
   Default: keep that.

## 0. Finish this branch (PR #243)

- ☐ Commit the work on `social` (everything is uncommitted) and push. **S**
- ◐ Golden screenshot tests: run, and they say nothing. The local baselines (`tests/golden/`,
  git-ignored) are from a much older interface (another top bar and layout), so all of them
  fail, in scenarios this branch never touches too. Re-blessing them (`CADRS_BLESS=1`) is
  yours to decide; after that they are worth running again. **S**
- ☐ Check PETG's library values against the Eastman Eastar 6763 data sheet (they are from
  memory). Tried twice online: Eastman's page doesn't render the table to a fetch, MatWeb,
  makeitfrom and Curbell refuse. Needs the PDF, or your eyes. **S**
- ✔ Find a shop: the material filter is the shared dropdown (any material, the Part Studio's,
  and what the shops say they work in).
- ☐ The dialog shows a stray scrollbar while a dropdown's list is open over its footer. **S**
- ☐ Marketplace panel is hidden under 800 px wide; give it a place on narrow windows. **S**
- ☐ One more review round on the round 6 fixes and the material work (last rating 9.2). **S**
- ☐ Release notes `tools/release/notes/v<version>.md`, version bump, tag. **S**

Not blocking the release, from the README's deferred list: a cost to being listed or a way to
vouch (flooding the marketplace with made-up shops), organization key backup, provable reviews,
removing a request from the log, log compaction, encryption of an organization's document.

## 1. Credits

Seven of the ten journeys stop at "and pays". This is the first thing to build.

### 1a. The bank (`cadrs_credits`, `cadrs_net::bank`, in the seed)

- ✔ Wallets by account (a person's or an organization's), whole credits, no overdraft, every
  movement with both sides, an amount, a note and what it was for (`book::Book`).
- ✔ The faucet: 1000 credits to the asking person, once an hour; the statement says when next.
- ✔ Transfers, signed by the payer's device, asked twice and paid once (`MoveId`). An order's
  hold and its settling have ids worked out from the order, so a lost answer or a second press
  holds once (round 7, S1).
- ✔ Statements: balance and history a page at a time, for the wallet's own devices and an
  organization's members.
- ✔ Held payments: hold, release (payer), return (payee), claim after thirty days.
- ✔ A receipt for every movement, signed by the bank.
- ✔ Live: the bank tells the devices that looked at either wallet (`Post::Credited`); a
  payment shows on the other side in well under a second in the three-app test.
- ✔ Organization wallets: the organization's key says who spends and who looks
  (`Ask::Access`), again whenever roles change.
- ✔ Limits (`crates/cadrs_credits/README.md`); faucet farming with free accounts is accepted
  and written down.
- ✔ Durable: an append-only journal, each record checksummed. A last record torn by a crash
  is cut off and kept; any other damage, or something a later cadrs wrote, stops the bank from
  starting and changes nothing (review round 7, B1).
- ✔ Round 8: ids are each wallet's own at the bank (nobody uses up an id the other side will
  ask with); a tail with a whole record in it is damage, not a torn write; a statement says of
  the held payments it is asked after by name, so strangers' larger holds can't hide an
  order's; the longest access list fits in an ask; strangers' holds raise no notice.
- ✔ Nobody stops a shop being paid: what one payer holds for one payee has a ceiling, what is
  held for a shop has none, a statement lists the largest holds first (round 7, B2).
- ☐ **Deploy: update the seed on the Unraid server** (yours to do or to tell me to do).
- ☐ A preference for the bank's ID (today `CADRS_BANK`). **S**
- ☐ Device revocation at the bank; journal snapshots. **M**

### 1b. The crate

- ✔ Types, what gets signed, `ask`/`read_answer`/`read_credited`.
- ✔ One implementation for the seed, the app's own bank (scenarios, tests) and the tests.
- ◐ Forward compatibility is tested (a later kind of movement still reads); fixed-bytes wire
  tests as `cadrs_org/tests/wire.rs` are not written. **S**

### 1c. A person's wallet

- ✔ The balance in the account menu ("Wallet · 1,000 credits").
- ✔ **Get 1,000 test credits**, with the wait counting down.
- ✔ The Wallet screen: balance, what is held, the faucet, transactions (when, what, with whom,
  what for, amount), Show older.
- ✔ **Transfer…** to a contact, an organization, a shop, a wallet address or a contact link;
  amount, note, one confirm step.
- ✔ A notification and a toast when credits arrive.
- ✔ Copy as CSV; Copy address.
- ✔ Held payments listed in the wallet, each with Release (payer), Return or Claim (payee).
- ✔ Icons: wallet, faucet, send-credits, payout.
- ☐ The phone page: balance, faucet, pay. **M**

### 1d. An organization's wallet

- ✔ A **Wallet** module: the same screen.
- ✔ **Transfer…** and **Pay out…** (members first) for owners and admins.
- ✔ Members see it and are told they can't spend.
- ✔ What strangers hold for the shop that is for none of its orders goes back in one go.
- ◐ A removed admin spends until the device that holds the organization's key is next online
  (said in `crates/cadrs_credits/README.md`; needs the key in a second place). **L**
- ✔ On the Overview: what is in it, received this month, held for it.
- ☐ Income by client and by order. **M**

### 1e. Paying for things

- ✔ Prices in credits: `CRD` is the default currency; any other is paid one for one.
- ✔ **Accept and pay N credits**: asked once, held, then the shop is told.
- ✔ Signing for delivery releases the payment, tried again until the bank has done it, with
  **Release payment** on the order meanwhile (round 7, S2); **Return payment** gives it back;
  **Claim payment** after thirty days.
- ✔ The shop sees a payment that is less than the quote, or not from the order's client, in
  red (round 7, S4).
- ✔ A device takes "you were paid" from the bank's own device only, once (round 7, S3).
- ✔ Both sides see how each order's payment stands (held, paid, returned, none held).
- ✔ Not enough credits is said before anything is asked.
- ✔ Linked both ways by the order's id (`About::order`).

### 1f. Proof

- ✔ `cadrs_credits` tests (18: a damaged journal, strangers' holds, receipts only to a
  movement's sides, an order's hold asked for thrice), `credits_flow` (three three-app tests:
  a paid order with an answer lost and the bank away at signing; only the bank's word taken).
- ✔ Scenarios: `wallet` (13 screenshots), `org_create` (a paid order, a returned payment).
- ◐ Journeys 2 and 6 run end to end on the desktop app; the phone can't pay yet.

## 2. Designs for sale (journey 3)

- ☐ Publish a document as the organization, not as the device that lists it. **M**
- ☐ A price on a public document; a free read-only preview. **M**
- ☐ Buying: pay, then fork; the purchase is kept so a new device has it too. **M**
- ☐ Delivery while the seller is offline (a seed or mailbox holds the purchase). **L**
- ☐ Buyers are told of a new version. **S**
- ☐ Sales dashboard: views, stars, sales and income by design. **M**

## 3. Bill of materials, then buying (journeys 4, 7; the base for 9 and 10)

- ☐ A BOM taken from an assembly into the organization's parts, with quantities. **L**
- ☐ Requests for things other than cut parts: boards, bought-in parts, assemblies. **L**
- ☐ Order from a BOM: several requests at once, one screen to follow them. **L**
- ☐ Suppliers: an organization as a buyer; purchase orders sent as requests and confirmed
  with a date. **L**
- ☐ Stock per location and for everything, not only sheet; low-stock warnings; stock going
  down by what a job used. **L**
- ☐ Material needs by date against stock and what is on order: the shortage list. **L**
- ☐ Receiving against a purchase order; batches; tracing a batch to the orders that used
  it. **L**
- ☐ Invoices and month-end summaries (income, material used). **M**

## 4. From a CAM program to the shop floor (journeys 5, 9)

- ☐ Link a CAM machine to the organization's machine, and a program's time to a job and its
  quote. **M**
- ☐ Tool library with feeds by material, setup sheets, remaining stock against the model:
  check against what a programmer expects, build what is missing. **L**
- ☐ Work orders with operations in sequence; scheduling against capacity. **XL**
- ☐ Operator screen at the machine: queue, start and finish, good and scrapped, an issue with
  a photo. **L**
- ☐ Choosing a location for an order (machines, capacity, material on hand). **L**
- ☐ Transfers of stock between locations. **M**

## 5. Many shops at once (journey 10)

- ☐ One request to several shops; answers side by side. **L**
- ☐ Awarding parts across shops; purchase orders with milestones (credits held and released
  by milestone). **L**
- ☐ Subcontracting chains: the buyer sees who, not what it cost. **L**
- ☐ A part's record: material certificate, batch, measurements, who made and checked it. **L**
- ☐ Holds across organizations when a batch is bad. **L**

## 6. Engineering at scale (journey 8)

- ☐ Documents that belong to an organization. **L**
- ☐ Teams, and permissions by team and document. **XL**
- ☐ Change requests and approvals; a release as a controlled state. **XL**
- ☐ BOM roll-up with mass and cost; where-used across the organization. **L**
- ☐ A size test: hundreds of members and thousands of documents in one organization. **L**

## 7. Shipments, carriers and dashboards (journeys 9, 10)

- ☐ Shipments with a carrier (an organization too), hand-overs signed at each step. **L**
- ☐ Dashboards: late and at-risk orders, machine use, scrap, open issues, stock value, by
  location and overall. **L**

## 8. First five minutes (journey 1), any time

- ☐ A guided first sketch; hints when a shape doesn't close or a dimension over-constrains. **M**
- ☐ Export a sketch as DXF or SVG in one step; print at 1:1. **M**
