# cadrs_org

An organization (a shop, a team, a publisher of designs) as a document its members share: a
signed log of operations, with no server. This crate has no Bevy and no kernel in it, so the app,
the phone page and the tests all use the same model.

## The model

An organization is a `cadrs_social::log::Log` whose entries carry `Op`s. `Org` is what replaying
them in the log's order gives; it is also the log's reducer, so every operation is checked against
the state before it (is the author a member, may their role do this, is there stock left, does the
name fit) and is either applied or answered `Conflicted`, the same way on every device.

| What                    | Where                 | Notes |
|-------------------------|-----------------------|-------|
| Members, roles, devices | `lib.rs`              | Member runs the work; Admin also sets up machines, prices, profile; Owner also names owners. At least one owner always. |
| Profile                 | `profile.rs`          | Name, tagline, description, logo, what it offers (manufacturing, designs), e-mail, website, whether it asks to be listed. Changed a part at a time (`Op::EditProfile`), so two people changing different parts both keep theirs. |
| Locations               | `profile.rs`          | None to many. Each: address, production and pickup flags, an optional point, and whether and how precisely the marketplace shows it (never under 1 km). An organization has no place of its own. |
| Machines                | `capability.rs`       | What each can make (process, bed, materials and thicknesses) and the location it stands at. A request is matched against them. |
| Stock and reservations  | `lib.rs`              | Sheet on hand; what is held for an order. |
| Prices                  | `quote.rs`            | A rate card: machine time by machine, sheet by material, setup, minimum. Instant quotes come from it. |
| Requests and orders     | `lib.rs`, `letters.rs`| A request for a quote, numbered as it arrives (R-0007), with the client's document as a blob; quotes with ids; the stages to a delivery both sides sign. Only so many may wait unanswered (`MAX_WAITING`, `MAX_WAITING_PER_CLIENT`, `MAX_WAITING_BYTES`); a declined request's document is let go. |
| Clients, parts, jobs    | `lib.rs`              | Clients kept by hand or met by a request; parts with what goes into them; jobs on machines. |
| Issues                  | `quality.rs`          | The one record for anything that went wrong: about a request, job, part, machine or location; an owner; closed only with a disposition and the reasoning. Numbered (I-0007). |
| Blobs                   | `blob.rs`             | Files beside the log, named by their hash: logos, the documents requests carry. Members' devices fetch the ones they lack; anyone may fetch what a listing names. |
| Listing                 | `directory.rs`        | What the marketplace shows: signed by the organization, at most 3000 bytes (cut down to fit), held to what an organization may say (`ShopListing::valid`), replaced by a newer one, taken out by a signed withdrawal, forgotten after 7 days unsaid. |
| Refusals                | `refusal.rs`          | The reducer's reasons the app says in the person's language, as a kind and not a turn of phrase. |
| Countries               | `country.rs`          | ISO 3166-1 alpha-2 codes and English names, for addresses. |

### What a listing needs

An organization is in the marketplace when it asks to be (`Profile::listed`) and lacks nothing
(`profile::missing`): a logo, a tagline, what it offers, an e-mail address, and, if it
manufactures, a location where things are made (with its country) and a machine. `Org::listed()`
is both. The app asks for most of this when the organization is made, and its Overview lists the
rest with a button to where each is set. Strangers judge a shop by its listing; an empty one helps
nobody.

## What a stranger can make a device do

Organizations are reached by people their members have never met: that is what a marketplace is.
What arrives from them is held to this.

- **Sizes, at the doors.** Limits on fields are forgotten one field at a time, so the sizes are
  also held where things come in, whatever they hold: an operation is at most `MAX_OP` (128 KiB)
  as written, or the reducer refuses it and it never reaches the log; a letter weighs at most
  `letters::MAX_LETTER` besides the document a request carries (`MAX_SNAPSHOT`); a post that
  carries no document or log is at most 64 KiB (`cadrs_social::post::Post::max_len`); a contact
  card says a name of 60 characters and names 64 devices at the most (`Card::valid`), wherever
  it comes from. A reply dials a handful of the devices a card names, never all of them, and
  one that finds nobody online is sealed for four of them and left at two mailboxes at the
  most.
  `tests/hostile.rs` sends letters and listings made at random, every field as large as it
  goes, through the same doors, and weighs what was kept.

- **A listing** is believed only as far as an organization could say it (`ShopListing::valid`:
  an organization's card, its limits on names and lines, sizes that are numbers). What it claims
  decides nothing about what is kept: a full directory drops the shop this device heard from
  longest ago by its own clock, never the user's contacts or own organizations
  (`Directory::protect`). Listings come in at a pace (`directory::Intake`: a bounded queue, so
  many looked at a time, so many shops not heard of before a minute), and only a listing that
  holds counts against it, so junk doesn't keep real shops out. A rating a shop gives itself is
  neither kept nor shown (see the deferred list). A listing is believed for a week: a shop
  offline that long drops out of the marketplace until it is back.

  What this does **not** stop: listings cost nothing to make, so someone who keeps saying
  thousands of validly signed made-up shops crowds the marketplace. The shops a device already
  knows stay while they keep being heard, contacts and own organizations always stay, and a
  search still finds what is there; but a shop the device hasn't heard of yet competes with the
  flood for the minute's allowance, and in a full directory is among the first to go. Likewise
  a few dozen throwaway accounts can fill what may wait at a shop until someone declines it
  (one button). The cure is a cost to being listed or a way to vouch, which isn't here yet
  (deferred item 1).
- **A shop's designs** on its page are the public documents of the devices that said, each
  with its own key, that they publish for it (`directory::Publishes`). The devices a card names
  prove nothing: a card can name anyone's.
- **A logo** is fetched from the shop's devices by its hash, and is kept, decoded and drawn only
  if it is one: a PNG or JPEG of exactly 128 × 128 or 512 × 512 pixels within a logo's size
  (`cadrs_app::org_logo::is_logo`). Only a listed organization's logo is handed to anyone who
  asks; a request's document goes to members' devices only.
- **How fast the log can grow.** What waits has a ceiling, and declining makes room, so how
  much a day can add has one too: so many requests a day, from one client and in all
  (`MAX_REQUESTS_A_DAY`, `MAX_REQUESTS_A_DAY_PER_CLIENT`), each at most `MAX_REQUEST` as written
  (48 KiB), with a few questions from its client before it is an order
  (`MAX_NOTES_BEFORE_ORDER`). With a shop that keeps clearing a list strangers keep filling,
  that is some 17 MB a day, and about 1 MB more for each account the shop blocks that day
  (blocking gives the account's share back to the other strangers, who can use it); nothing
  short of compaction (deferred item 11) takes it out again. Taking a request costs the same
  however many came before (the reducer keeps its counts as it goes), so a long history makes
  the log larger, not each operation's checks slower. However large a log has grown, it still syncs: members' devices
  exchange it a page at a time (`session::SYNC_BUDGET`), so a new device joins an organization
  of any size.

  These shares are the strangers': the people the organization doesn't know yet. Someone it
  knows (kept among its clients, or quoted before: `Org::knows`) has a share of their own, so a
  list or a day that strangers filled doesn't shut them out, and what a blocked account sent
  counts against nobody. What this does **not** stop: enough throwaway accounts use up the
  strangers' share every day, and then someone asking that shop for the first time is refused
  that day (and told so) until the shop blocks the accounts that did it, one by one.
- **A refusal is an answer.** A shop that is reached and doesn't take a request says why (a
  sentence: no more of the reason is read, shown or kept than `post::MAX_REASON`, whatever was
  sent as one), and the client's cadrs shows it: the request is marked declined with the reason, and the letter
  isn't left at a mailbox to be refused again later. What no shop would take (too many parts,
  too large as written) is said before it is sent (`letters::request_problem`).
- **A request for a quote** is taken from whoever has the organization's link, up to the
  ceilings above, and only by an organization that makes parts. Its card must be the
  client's own and a card. It is checked before anything is written, and the asker doesn't become one of the user's contacts. An account can be
  blocked (`Op::SetBlocked`), and everything that waits declined at once.
- **An invitation** names who invites by the certificate of the device that handed it over, and
  no other (certificates aren't secrets). From the user's own account the organization is joined
  at once; from anyone else it waits to be accepted or declined, and strangers' invitations
  wait only up to a number.
- **A quote** is accepted while it stands; the client's word for when a letter was sent is taken
  only for a few days past that (`ACCEPT_GRACE`).
- **A shop's letters to a client** are held to the same: a client keeps, for each request it
  sent, a quote, a reason and a conversation's worth (`MAX_NOTES` lines and `MAX_NOTE_BYTES` of
  text from each side, as the organization's document does), and nothing about requests it
  never sent unless the shop is one of its contacts, and then only a few
  (`letters::ClientBook::receive_learning`, `MAX_LEARNED`).

## How it changes without breaking

Logs live on every member's device and are never rewritten, so what is written today is read by
every later version, and by older ones still in use.

- **Two encodings.** Things that are *signed as values* (`Signed<T>`: device certificates, cards,
  receipts, log entries, chat lines) are postcard: canonical and frozen, versioned by their
  context string. Everything that must grow (`Op`, letters, posts, listings, the log file, the
  app's and the phone's own files) is MessagePack with field names, through
  `cadrs_social::codec`. A signed thing that must grow is signed as its codec bytes
  (`keys::SignedBytes<T: Growing>`): the signature is over what was sent, not over a re-encoding.
- **Adding a field**: give it `#[serde(default)]`. Old logs read with the default; an older cadrs
  ignores it.
- **Adding an operation**: add the variant. An older cadrs can't read it: `Op::decode` says
  `codec::Error::Unknown` (`is_newer()`), the reducer counts it in `Org::newer_ops` and leaves the
  state alone, and the app says part of the organization was written by a newer cadrs.
- **Never** rename or remove a field or variant, or change what one means. Add a new one.
- `tests/wire.rs` holds what is written today (an operation's bytes; the size and hash of the
  founding, a request, a letter, a listing and the log file, signed with fixed keys) and reads
  across an older and a newer shape; `tests/shop_life.rs` has an operation from the future.
- A pair that is a pair stays a tuple (a part and how many, a thickness and a speed); a record
  that may gain a fact is a struct with named fields (`quote::MachineRate`, `quote::SheetPrice`).
  A listing isn't kept longer than a week, so its shape can change with its context string.
- The log file has a version (`file::VERSION`). A file that can't be read is an error and is left
  where it is: never an empty organization written over it.

## The modules, and where this is going

Two marketplaces set the shape of the profile. From Shopify: what is shown to the
public is gated by a checklist of what a shop still lacks (here `profile::missing`, though making
an organization here asks for more up front than Shopify does, on purpose: an organization exists
to be found); a logo is a square picture kept at 512 × 512; and a shop has locations as records
of their own, which stock and fulfilment point at, instead of an address on the shop. From Thangs, where
everything is a download: a publisher has a picture, a line about itself and no location at all,
so a location is never required of an organization that only offers designs.

The app's organization screen is a side navigation of modules, each a view of part of `Org`. The
grouping follows what ERP, MES and QMS systems converge on (Carbon was the reference for the
layout: a location-centred model, a sidebar of modules, tables and record pages), and three
lessons from the in-house systems at SpaceX (Warp Drive) and Tesla (Warp): one graph of records
beside the design instead of systems that import and export; the issue ticket as the universal
quality record, which anyone may open and which is closed with a decision and a reason; and
shop-floor execution kept separable from planning.

| Group        | Modules today                       | Kind |
|--------------|-------------------------------------|------|
| (top)        | Overview                            | — |
| Sales        | Requests (quotes, orders), Clients  | ERP |
| Production   | Jobs, Machines                      | MES |
| Inventory    | Stock, Parts                        | ERP |
| Quality      | Issues                              | QMS |
| Organization | Profile, Locations, Members, Prices, Chat | — |

Deferred, in the order they are likely to matter:

1. **Moderation, and a cost to being listed.** The limits above bound what strangers cost a
   device; they don't judge, and they don't make a made-up shop expensive (see above). There is
   no reporting or hiding of a listing, no proof of work or vouching for one, and blocking a
   client is by account, which a determined nuisance replaces.
2. **The organization's key.** It lives on the device that founded the organization, and only
   there: a linked device gets the organization's certificate for itself, not the key. There is
   no backup, transfer or rotation of it yet: lose that device and nobody can list the
   organization, certify another device or sign its deliveries.
3. **Reviews that prove themselves.** A review is written into the log by a member's device
   from the client's letter; the operation doesn't carry the client's signature. Members see
   their own rating; the marketplace shows none until a review can be checked by a stranger.
4. **Stock per location.** Stock is the organization's; with more than one production location it
   wants a location on each count and transfers between them (`Location` ids are already what
   machines point at).
5. **Quotes by hand.** Quotes come from the rate card. A quote with lines a person writes, and
   one for a process the rate card doesn't price.
6. **Designs for sale.** A shop's page lists the public documents of the device that says its
   listing (it vouches for itself); other members' devices aren't asked to vouch yet, nothing is
   sold, and a document isn't published *as* the organization.
7. **Tax, invoices, payment.** A quote is a price in a currency. No tax, no invoice numbers, no
   money moves through cadrs.
8. **As-built records.** Jobs don't record what was actually made (serials, measurements, who
   ran it); issues would then point at them.
9. **Removing requests.** A declined request's document is let go a day later, but the request
   stays in the log, as everything does.
10. **Permissions finer than roles**, by module and action.
11. **Log compaction.** The log only grows, and every operation rewrites its file and replays
    it (`Session::write`), which is slow long before it is large; a signed snapshot to start
    from, appending instead of rewriting, and applying an operation without replaying the rest.
12. **Encryption at rest and group keys.** Members' devices hold the log in the clear; letters are
    sealed in transit only.
