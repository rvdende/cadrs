# Social branch (PR #243): release plan

Goal (user, 2026-11-23): after the research, implement what was learned on the `social` branch and
have an agent rate the work at least 9.5/10. Local notes file (docs/ is git-ignored).

## User feedback (explicit)
- "New organization" moves from the documents page's Create menu to the account dropdown.
- The New organization dialog requires more than a name; at minimum a 512x512 logo.
- An organization has 0..n physical locations (digital-only sellers have none): no single
  location on the Overview.
- A marketplace section on the documents page (public documents sit on the right today).
- It becomes an ERP + MES + QMS; Carbon (crbnos/carbon, AGPL: ideas only) is the reference for
  data and panels. Shopify and Thangs for the seller's set-up. Warp Drive / Warp for philosophy.

## Verified blockers
- postcard can't take a new field or op (tested): ops, letters and local files move to named
  MessagePack (`cadrs_social::codec`); signed statements stay postcard, versioned by context.
- The shop drops the document a request carries (keeps only its hash): keep it as a blob.

## Scope, in order
A. Model (cadrs_org, cadrs_social)
 1. codec (named MessagePack) for Op, letters::Message, local stores; unknown ops counted.
 2. Profile: name, tagline (<=150), about, logo blob, offers (manufacturing / designs), contact
    email, website, listed; no place.
 3. Locations (0..n): name, roles (production, pickup, office), address, point, phone, public +
    accuracy. Machines and stock reference a location.
 4. Blobs beside the log (logo 512 PNG, request snapshots), synced between members' devices.
 5. ShopListing: tagline, offers kinds, logo thumbnail hash, public places, website; size cap.
 6. Accept names the quote it accepts; valid_until enforced.
 7. Completeness (what a listing needs); text limits.
B. App
 1. Account menu: my organizations + New organization…; none in Create; org documents not in the
    documents list.
 2. New organization dialog: logo (>=512 square, cropped/resized), name, tagline, what it offers,
    first location when it manufactures, contact email; inline validation.
 3. Organization screen: side navigation in groups (cadrs_ui::SideNav, shared with Preferences);
    Overview as a dashboard with the completeness checklist and listing switch; Profile;
    Locations; machines with a location; requests open the sent document; no rebuild while typing.
 4. Marketplace panel on the documents page: Designs (public documents) | Shops (cards with logo,
    tagline, processes, place, rating; search); a shop's page; map.
 5. Robustness: org key 0600; a local file that doesn't decode is kept aside, not overwritten.
C. Phone page keeps building against the model.
D. crates/cadrs_org/README.md: the model, how it evolves, the module map (ERP/MES/QMS) and what
   is deferred.
E. Tests and scenarios for all of the above; social_flow keeps passing.
F. Review agent rates it; iterate to >= 9.5.

## Status
(see the bottom of this file; update as steps land)

## Warp Drive / Warp lessons (research, 2026-11-23)
- One record graph next to the model; issue tickets as the universal quality object (anyone
  opens one against a part/order/machine: location, owner, disposition rework/repair/use-as-is/
  scrap, rationale); as-designed vs as-built; keep shop-floor execution separable; costs
  searchable per job; a scripting API (ours: MCP).
- Stretch after the core: a Quality > Issues module (Op::SetIssue) as the first QMS slice.

## Status log
- A1 done: cadrs_social::codec (named MessagePack) + keys::SignedBytes; tests pass.
- A2-A7 done in cadrs_org/cadrs_social: profile.rs (Profile, Offers, Logo, Location, Address,
  missing()), country.rs, blob.rs (put/get/answer/take), quality.rs (Issue), directory.rs
  rewritten (SignedBytes listing, places, logo, trim to 3000 bytes, Query, prune), letters via
  SignedBytes (Refused::Newer, Accept names quote id, stage only forward), Op via codec
  (newer_ops), limits, Post via codec + WantBlobs/PublicBlob, org.log v2 (unreadable = error).
  Tests: cadrs_org 35 unit + 6 shop_life; cadrs_social 49+5; cadrs_mobile 4.
- Phone page (cadrs_mobile) compiles: keeps request documents as blobs, answers WantBlobs.
- NEXT: make cadrs_app compile (social.rs, org_sync.rs, organization.rs, org_tabs.rs,
  find_shop.rs, request_quote.rs), then UI (B1-B5).
- App plumbing done (not yet compiling as a whole): social.rs (load keeps unreadable aside,
  directory save/load, blobs: public_blob/want_logo/took_logo/logo_bytes, org_state,
  org_changed, listings cached + directory::sign), org_sync.rs (session() never empties an
  unreadable log; ask_blobs/blobs_wanted/took_blobs; Invitations wait: invited/join/decline),
  Post::Invite.inviter; phone only takes invites from own account or contacts.
- cadrs_ui::side_nav (SideNav, SideNavSelect, SideNavState) + used by preferences_dialog.
- cadrs_app/src/org_logo.rs: process() (>=512, near-square, 512 PNG + 128 thumb, JPEG fallback),
  LogoImages, handle(), logo() bundle.
- organization.rs rewritten: MODULES (13, grouped), OpenOrg{nonce}, NewOrg + create(world, NewOrg),
  open_by_id, write (refusal() i18n), spawn_screen (nav panel + body), refresh_body (key; waits
  while typing), ModuleView, table()/text_row()/cell_button(), members (role change/remove),
  machines (edit, location), stock (table), prices (currency required, rates by machine id),
  requests (cards, open document, decline with reason, report issue).
  RateCard.hourly now Vec<(Id, Money)>; estimate(&BTreeMap<Id, MachineCaps>, ..).
- All of the TODO list written and compiling (2026-11-23): org_profile.rs (overview with
  checklist + listing switch, profile, locations with map, remove from this device),
  org_quality.rs, org_create.rs (dialog, inline + live validation, logo picker shared with
  Profile), request_quote::open_snapshot, identity menu (organizations, invitations with
  Join/Decline, New organization…), landing (no Create entry; orgs only in Trash once removed),
  landing_market.rs (Marketplace: Designs | Shops, shop page), find_shop.rs on Query/places,
  en.ftl (~200 keys), icons (dashboard, job-schedule, stock, quality-issue, price-list,
  marketplace), cadrs_org/README.md, tests/wire.rs (golden bytes + cross-version reads).
- Added beyond the list: ShopListing::withdrawal (a shop that leaves the marketplace goes at
  once; own shop heard into own directory); SendKind::Grant (organizations are sent to a
  linked device only after it took the grant, so the invite counts as the account's own);
  Theme::paragraph_within; org_script.rs (`Custom("org logo|market|request")`).
- Scenarios: org_create (25 screens, whole flow), marketplace (6). social_flow: 7 tests, joins
  the public room one app at a time (the old way raced), separate temp folders.
- Known and left: a logo is decoded on the main thread (bounded by MAX_SIDE and 20 MB); shop
  rows have no ellipsis; see the README's deferred list.

## Review round 1 (2026-11-23): 7.2/10, and what was done about it
Blockers:
- B1 logos from peers: org_logo::is_logo (PNG/JPEG, exactly 128 or 512 square, size cap) before
  keeping, decoding or drawing; LogoImages remembers non-logos; 8 decodes a frame; the user's
  own pick is processed off the main thread (org_create LogoJobs).
- B2 inviter: counted only when the certificate is the sending device's (app + phone).
- B3 directory: ShopListing::valid, eviction by last-heard (Heard.heard, kept in the file),
  Directory::protect (contacts + own), CLOCK_SLACK 10 min, KEEP_FOR 7 days, ratings neither
  said nor kept nor sorted by; app: 60 new shops a minute, 32 listings a frame, file saved at
  most every 30 s, unlisted logos pruned.
- B4 requests: reducer ceilings (MAX_WAITING_PER_CLIENT 5, MAX_WAITING 200, MAX_WAITING_BYTES
  512 MiB, MAX_SNAPSHOT 32 MiB, MAX_REQUEST_PARTS 200), only a manufacturer takes them, checked
  before the blob is written, no auto-contact, declined requests' blobs swept. Not gated on
  `listed` on purpose: the share link/QR flow of an unlisted shop.
Should:
- S1 ACCEPT_GRACE (3 days). S2 last owner can't be demoted. S3 issue refs checked at opening
  only. S4 OrgForm/track_form: a module with unsaved changes isn't rebuilt by remote changes;
  letters arrive through organization::take_op (not an own change). S5 Op::EditProfile
  (ProfileEdit::between). S6 codec::load_file/save_file/set_aside used by app, phone, mailbox.
  S7 organization::choice (cadrs_ui Select) for growing lists; TextInput::lines. S8 client
  names from the card, logo in the account menu (MenuItem::picture), ask_remove before every
  Remove, Ellipsis on shop rows, "By its city" for a point-less public location. S9 Request a
  quote on a shop's page (request_quote::PreferredShop), its public designs listed. S10
  public_blob without opening the log (MyOrg.listed), relist only on listing ops, refresh_orgs
  only when the set of org documents changed. S11 social_flow::a_stranger_gets_nothing_and_
  leaves_little; wire.rs fixed sizes+hashes; shop_life tests for every reducer fix.
Nits: delete icon, shop-machines icon, cadrs_org::refusal::Refusal, stage names translated in
  notices, Issues button says Edit, empty e-mail message, RateCard tuples -> MachineRate /
  SheetPrice, organization.rs split into organization/{members,machines,pricing,requests}.rs.
Left (in the README's deferred list): moderation, org key backup, provable reviews, request
  removal, compaction; marketplace panel still hidden under 800 px.

## Review rounds 2-5 (2026-11-23): 8.7, 8.8, 9.0, 9.2
- R2: directory::Intake (pacing in the model, charged only for valid new shops), profile save
  diffs against OrgForm.profile, logo prune by hash, navigation always live + news line + chat
  kept live, directory::Publishes (device vouches), Op::SetBlocked + decline all, sweep a day
  after decline, set_aside never overwrites.
- R3: sizes at the doors (MAX_OP in the reducer, letters::MAX_LETTER + Message::problem,
  Post::max_len, Card::valid, social::dialed), client side bounded, tests/hostile.rs.
- R4: Session::since pages (SYNC_BUDGET), daily request caps + MAX_REQUEST, per-side
  conversation caps, mailbox fan-out cut (MAX_SEALED, MAX_MAILBOXES), MAX_LEARNED 20.
- R5: Org::knows (known clients have their own share; blocked accounts count against nobody),
  refusals told apart from unreachable (post::REFUSED, Done::Refused marks the client's book),
  letters::request_problem checked before sending.

## Review round 6 (2026-11-23): 9.2, and what was done about it
- A refusal's reason is bounded: post::MAX_REASON (200 chars, control characters dropped); both
  transports read the status byte, then at most 4 × MAX_REASON bytes of a refusal
  (cadrs_net::social, cadrs_social_net); social::refused cuts it again. Tests with a 5 MB reason.
- Request checks are constant-time: Org.intake (by_time, waiting, quoted, kept) kept up to date
  by Op::Request, advance() and Op::SetClient; shop_life::taking_a_request_costs_the_same_
  however_long_the_history.
- README: the growth figure (about 17 MB a day at the ceilings, 1 MB per blocked account).

## Machines' materials (2026-11-23, user: "petg is not working", "same materials as Part
## Studio, a dropdown with a search", "reuse the same UI")
- Cause: parse_materials wanted "name thickness" and dropped a bare name; a printer's third
  number was stored as kerf and its height left 0, so no 3D part fitted.
- cadrs_ui::SearchSelect (search_select.rs): the Part Studio's material dropdown lifted out
  (closed box, search box, scrolling list, same element names `<name>-choose/-search/-list/
  -item-<slug>`). adds() for a list the owner keeps, typed() offers what was typed when no
  name is that. Enter takes the first hit, Esc closes the list and not the dialog under it
  (Dialog::close_on_escape and material_keys ask search_select::any_open).
- material_dialog.rs uses it (its own list, popup, search and scroll code removed);
  course_ps10_material, sm_e1, sm_e2, course_td_ex1_drill unchanged and passing.
- Machine dialog: rows of materials picked from organization::material_names (the library,
  then names the organization already uses), "cuts up to [ ] mm" per row for sheet cutters
  only; a printer/mill/lathe has X, Y, Z and names. MaterialLimit.max_thickness 0 = not said.
  capability::fit: a non-profile machine that lists materials takes only those.
- The same picker in Stock › Add material and in Request a quote (was a typed field
  defaulting to "mild steel").
- cadrs_core::material::LIBRARY gains PETG (Eastar 6763 figures; not re-checked online, MatWeb
  and makeitfrom refused the fetch). Demo shops and the scripted request use library names.
- Not done: Find a shop's material filter is still typed text.
