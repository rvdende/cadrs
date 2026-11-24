# cadrs user journeys

Ten people, from someone drawing their first rectangle to a manufacturer running factories on
three continents. Each journey says who they are, what they came to do, the steps they take, and
when it counts as done. Under each: what cadrs does today, and what is missing.

The aim they add up to: one system for design (CAD), machining (CAM), running a shop (MES),
planning materials (MRP) and running the business (ERP), where the companies in a supply chain
deal with each other directly, peer to peer, with no platform in the middle.

Marks: **✔** works today · **◐** partly there · **✗** not built.

---

## Money, for now: credits

Every journey that involves a price uses **credits**. Real payment is out of scope for now;
everything around it is not.

- **Faucet.** A page (in the app and at cadrs.net) where an account takes 1000 credits, once an
  hour. A countdown shows when the next 1000 are due.
- **Balance.** In the account menu, always visible; an organization has a balance of its own,
  which its admins spend from.
- **Prices.** Anything offered has a price in credits: a quote, an order, a design, a
  shipment, a purchase order line.
- **Paying.** One confirm step ("Pay 724 credits to Cape Waterjet for order R-0007"). The payer's
  balance drops and the payee's rises within a second or two, on every device of both, with a
  notification on the payee's side. No refresh.
- **History.** Each account and organization has a ledger: what came in, what went out, for
  what (linked to the order, design or purchase it paid for), exportable.
- **Held payments.** An order can hold the buyer's credits when it is accepted and release
  them when the delivery is signed for, or return them if the shop declines.

**Built (2026-11-23), on the desktop app and in the browser:** the faucet, wallets for people
and organizations, transfers and payouts with a confirm step, prices in credits, Accept and pay
with the credits held until the delivery is signed for (or returned), history with what each
movement was for, live notices. Not yet: the faucet as a page of its own at cadrs.net (it is in
the app, which is the site), paying from the phone page, milestones. See `docs/work-list.md` and
`crates/cadrs_credits/README.md`.

The ledger is kept by a bank that is a cadrs peer (the seed), which signs a receipt for every
movement; a ledger nobody keeps can replace it behind the same asks.

---

## 1. Thandi draws her first shape

**Who:** a hobbyist who has never used CAD. She wants a cutting template for a wooden shelf
bracket: an L shape with two holes.

**Journey**
1. Opens cadrs.net in a browser. No account, no install.
2. Presses New document and is in a sketch on the Top plane within two clicks.
3. Draws the L with the line tool; the shape closes and fills, so she can see it is a region.
4. Adds two circles, types 120 and 80 for the legs and 6 for the holes; the shape resizes.
5. Makes a mistake, presses undo, tries again.
6. Exports the outline (DXF or SVG) or prints it at 1:1.

**Done when** she has a correctly dimensioned file in under ten minutes without reading
documentation.

**Today**
- ✔ Browser build at cadrs.net, documents, sketches, lines, circles, dimensions, constraints,
  undo.
- ◐ Getting started: there are sample documents, but no guided first sketch and no hints when
  a shape doesn't close or a dimension over-constrains it.
- ◐ 2D output: drawings exist; a one-step "export this sketch as DXF/SVG" and 1:1 printing need
  checking and probably building.

---

## 2. Marco makes a part and has it made

**Who:** a maker with a 3D printer, who sometimes needs a part cut from steel.

**Journey**
1. Sketches a profile, extrudes it, adds holes and fillets. Checks sizes with Measure.
2. For plastic: sends the part to his slicer and prints it.
3. For steel: opens Marketplace › Shops, searches "waterjet steel", sorts by distance, opens a
   shop's page and presses Request a quote.
4. The dialog lists his parts (thickness, cut length, material). He sends it.
5. A notification arrives with the quote: 724 credits, valid 14 days. He accepts and pays; the
   credits are held.
6. He watches the order move: In production, Ready. He collects it, signs for delivery on his
   phone; the credits go to the shop. He leaves a review.

**Done when** a stranger's shop has made his part and been paid, with no e-mail or phone call.

**Today**
- ✔ Part Studio modelling, measuring, slicer hand-off.
- ✔ Marketplace of shops with search and distance; Request a quote from a shop's page; instant
  quotes; accept; order stages; signing for delivery; review; notifications; the phone page.
- ✗ Paying: accepting an order moves no credits.
- ◐ Reviews exist but aren't shown in the marketplace until they can be verified.

---

## 3. Aiko sells her designs

**Who:** an industrial designer who publishes parametric designs (a drawer organiser system) and
wants to be paid for them.

**Journey**
1. Creates an organization that offers Designs only: logo, tagline, no location.
2. Publishes three documents under it, each with pictures, a description and a price: one free,
   two at 150 credits. She can offer a preview that opens read-only.
3. A buyer finds them under Marketplace › Designs, or on her organization's page.
4. The buyer pays 150 credits; Aiko's balance rises as it happens and she gets a notification.
   The buyer can now fork the document and edit their copy.
5. She releases a new version; buyers are told and get it.
6. Her dashboard shows views, stars, sales and income by design.

**Done when** she earns credits from a design while she is offline, and can see which design
earned them.

**Today**
- ✔ Organizations that offer designs only; public documents with stars and forks; versions.
- ◐ A shop's page lists the public documents of the device that says its listing; a document
  isn't yet published *as* the organization.
- ✗ Priced designs, paid forks, previews, delivery while the seller is offline (needs a seed or
  mailbox to hold the purchase), update notices to buyers, sales dashboard.

---

## 4. Dev builds a product with electronics

**Who:** an engineer designing a sensor unit: a circuit board in a machined enclosure.

**Journey**
1. Draws the schematic, picking parts from the JLCPCB library; lays out the board; the
   autorouter routes it.
2. Derives the board into a Part Studio and models the enclosure around it; checks that
   connectors and mounting holes line up.
3. Builds an assembly: board, enclosure, screws. Gets a bill of materials with quantities.
4. From the BOM, in one go: orders the boards from a PCB shop, the enclosure from a machine
   shop, and the screws from a supplier. Three requests, one screen to follow them.
5. Pays each on acceptance; follows all three to delivery.

**Done when** one product with electronic and mechanical parts is ordered from three suppliers
out of its own bill of materials.

**Today**
- ✔ Schematic and PCB layout, JLCPCB parts, autorouter, board-to-enclosure fit checks,
  assemblies.
- ◐ A request for a quote is made per Part Studio, to one shop at a time, for cut parts. Boards,
  bought-in parts and assemblies can't be requested.
- ✗ BOM as a thing you order from; one view of several orders for one product; paying.

---

## 5. Lena programs a 5-axis machine

**Who:** a CNC programmer at a precision shop with a trunnion 5-axis mill. She is a professional
and judges the tool by whether the simulation matches the machine.

**Journey**
1. Builds (or imports) the machine as an assembly; cadrs derives its axes from the mates. Sets
   travel limits, feeds, the tool changer and the post.
2. Fills the tool library: cutters, holders, stick-out, feeds and speeds by material.
3. Imports the customer's STEP file. Defines stock, fixtures and two setups.
4. Programs roughing (adaptive), finishing, drilling, and a 5-axis swarf and contour pass.
5. Simulates with the whole machine: material removal, collisions between holder, fixture and
   table, axis limits, remaining stock compared with the model.
6. Posts the program. Saves the setup sheet (tools, offsets, fixture picture) for the operator.
7. The job appears on the shop's schedule with its machine time, from the program.

**Done when** the program runs on the real machine first time, and the time on the schedule is
the time it took.

**Today**
- ✔ Machines with kinematics derived from assembly mates or taken from a catalogue, including
  five axes and arms; cutters; setups; adaptive, finishing, hole, rotary and multi-axis
  strategies; material-removal simulation; collision checks; posts with rotary axes.
- ◐ Tool library with feeds by material, setup sheets, comparing remaining stock with the
  model: need checking against what a programmer expects.
- ✗ The link from a program to the organization's job (machine time into the schedule and the
  quote), and from the CAM machine to the organization's machine record.

---

## 6. Sipho runs a one-man cutting shop

**Who:** the owner of a waterjet shop. He does everything himself, often from his phone.

**Journey**
1. Account menu › New organization: logo, name, tagline, what he offers, where, e-mail.
2. Adds his machine, his sheet stock and his prices. The Overview says he can be listed; he
   turns it on.
3. A request arrives (R-0007). He opens the client's document, checks the part against his
   machine, presses Quote; the price comes from his rates.
4. The client accepts and pays; credits are held. He reserves two sheets, schedules the job,
   starts it, marks it ready.
5. A part is out of tolerance: he opens an issue from the order, re-cuts it, closes the issue
   with the decision and the reason.
6. The client collects and signs; the credits arrive. Stock is two sheets lower.
7. At month end he looks at income, material used and what is low.

**Done when** he has gone from first request to paid, with stock and money both correct,
without another program.

**Today**
- ✔ All of steps 1 to 3 and 5; reserving stock, jobs, stages, delivery receipts; blocking and
  declining; the phone page.
- ◐ Stock goes down by reservation, not by what a job actually used; no low-stock warning.
- ✗ Payment; income and usage summaries; invoices.

---

## 7. Priya plans materials and buys them

**Who:** the planner at a 40-person fabricator. Her job is that nothing stops for want of
material, and that not too much money sits on shelves.

**Journey**
1. Opens the week's orders. For each, cadrs works out what it needs from its parts and bill of
   materials: sheet, bar, fasteners, bought-in parts.
2. Sees needs against stock, per location, with what is already on order: a list of shortages
   by date.
3. Turns the shortages into purchase orders, grouped by supplier, each line priced. Suppliers
   are other organizations on cadrs; the order goes to them as a request and comes back
   confirmed with a date.
4. Pays on confirmation (or on receipt, as agreed).
5. Goods arrive: she receives them against the purchase order, by location, with the batch
   number from the supplier's delivery. Stock goes up; a short delivery stays open.
6. Later, a faulty part is traced back: which batch, which supplier, which other orders used it.

**Done when** a shortage found on Monday is a confirmed, paid purchase order on Monday, and a
bad batch can be traced in a minute.

**Today**
- ◐ Parts with what goes into them; sheet stock and reservations; clients.
- ✗ Almost all of it: bills of materials taken from assemblies, material needs by date, stock
  per location and for things other than sheet, suppliers, purchase orders (an organization
  can receive requests but not send them as a buyer), receiving, batches and traceability.

This journey is the largest gap, and most of journeys 8 to 10 stand on it.

---

## 8. Elena leads engineering on a vehicle programme

**Who:** chief engineer of a 300-person organization designing an electric truck: chassis,
battery, drivetrain, body, each a team.

**Journey**
1. Organizes people into teams with roles; each team owns its documents. A contractor sees one
   subsystem and nothing else.
2. Teams work in their own documents, linked into a top-level assembly. Several people edit one
   document at once; branches for risky changes.
3. A change to the battery tray goes through a change request: what, why, who must approve
   (structures, manufacturing, purchasing). Approved, it becomes a released version.
4. The top assembly shows which released versions it uses and what is out of date. Its bill of
   materials is the engineering BOM: every part, with mass, material and cost rolled up.
5. Manufacturing takes a released version and sees exactly what changed since the last one.
6. Every issue from the factory about a part comes back to the engineer who owns it.

**Done when** 300 people change one vehicle without overwriting each other, and anyone can say
which version of every part is in the released truck.

**Today**
- ✔ Live editing together, versions, branches and merging, linked documents with update
  notices, organizations with three roles, members' channels, issues.
- ◐ An organization's documents: today an organization is a business record; engineering
  documents belong to people and are shared one by one.
- ✗ Teams and permissions by team and document, change requests and approvals, releases as a
  controlled state, BOM roll-up with mass and cost, where-used across an organization, and a
  size test: hundreds of members and thousands of documents in one peer-to-peer organization.

---

## 9. Kwame runs twelve factories

**Who:** operations director of a contract manufacturer with plants in four countries.

**Journey**
1. Each plant is a location with its machines, people, stock and calendar.
2. An order for 5 000 housings arrives. cadrs proposes which plants can make it (machines,
   capacity, material on hand) and what each would cost and when. He splits it across two.
3. Each plant gets work orders with their operations in sequence (cut, machine, deburr,
   inspect, pack), scheduled on machines against real capacity.
4. On the floor, operators see their queue on a tablet at the machine, start and finish
   operations, record quantities good and scrapped, and raise an issue with a photo.
5. Material moves between plants as transfers; finished goods ship with a carrier, tracked to
   the customer's dock, where the delivery is signed for and the held payment released.
6. His dashboard: orders late or at risk, machine utilisation, scrap, open issues, stock value,
   by plant and overall.

**Done when** he can answer "where is this order and will it be on time" for any order in any
plant from one screen, and the answer is what is happening on the floor.

**Today**
- ✔ Locations with machines assigned to them; jobs on machines; issues; the phone page for
  quoting.
- ◐ A job is one machine and a time; there are no operations in sequence, no capacity.
- ✗ Work orders and routings, scheduling against capacity, operator screens, quantities and
  scrap, stock per location and transfers, shipments and carriers, dashboards, choosing a
  plant for an order.

---

## 10. Ines sources a rocket engine's parts across a supply chain

**Who:** supply-chain lead at a launch company. An engine has 2 000 parts from 60 suppliers, each
with paperwork that must be right.

**Journey**
1. Selects 40 machined parts from the released engine assembly and sends one request for quotes
   to eight qualified shops at once, with drawings, material and inspection requirements.
2. Compares the answers side by side: price, lead time, each shop's record with her. Awards the
   parts across three shops.
3. Each award is a purchase order with milestones: 30% of the credits on acceptance, 70% on
   delivery with inspection passed. Credits are held and released as milestones are met.
4. Shops subcontract (heat treatment, coating) to their own suppliers through cadrs; she sees
   the chain for her parts, not their prices.
5. Each delivered part comes with its record: material certificate, the batch, measured
   dimensions, who made and inspected it, any issue and how it was closed.
6. A carrier (also an organization on cadrs) quotes and carries each shipment; hand-overs are
   signed, so custody is known at every step.
7. A flaw found in test is traced to one heat-treatment batch; every part from it, wherever it
   is, is on hold within the hour.

**Done when** she can show, for any part on the engine, who made it from what, who touched it,
what it cost and that it was paid for, with every step signed by the company that did it.

**Today**
- ✔ The foundation this needs and few systems have: every organization signs what it says, a
  delivery is a receipt both sides sign, and companies deal directly with no platform between.
- ◐ Requests, quotes and orders work between one client and one shop, for cut parts.
- ✗ Requests to many shops and comparing them, purchase orders with milestones, held and
  released credits, subcontracting chains, part records (certificates, measurements), carriers
  and shipments, holds across organizations.

---

## What the ten add up to

| Area | Today | Needed by journeys |
|---|---|---|
| CAD: sketch, parts, assemblies, drawings, sheet metal, PCB | ✔ strong | 1, 2, 4, 8 |
| CAM: machines to 5 axes, strategies, simulation, posts | ✔ strong | 5 |
| Marketplace, requests, quotes, orders, delivery receipts | ✔ for one client, one shop, cut parts | 2, 6 |
| Organizations: profile, locations, machines, stock, issues | ✔ first version | 6, 9 |
| Credits: faucet, balances, prices, paying, held payments, ledger | ✔ desktop and browser; ✗ phone, milestones | 2, 3, 4, 6, 7, 9, 10 |
| Designs for sale | ✗ | 3 |
| BOM from assemblies; ordering from a BOM | ✗ | 4, 7, 8, 10 |
| Buying: suppliers, purchase orders, receiving, batches | ✗ | 7, 9, 10 |
| Stock per location, all kinds of stock, transfers | ✗ | 7, 9 |
| Work orders, routings, capacity, operator screens | ✗ | 9 |
| Teams, permissions, change requests, releases | ✗ | 8 |
| Requests to many shops, quote comparison, milestones | ✗ | 10 |
| Shipments and carriers | ✗ | 9, 10 |
| Dashboards and reports | ✗ | 3, 6, 9 |
| First-run guidance | ◐ | 1 |

**A build order that follows the dependencies**

1. **Credits** (faucet, balance, pay, held payments, ledger). Seven journeys need it, and it
   makes journeys 2 and 6 complete end to end, which is the first thing worth showing anyone.
2. **Designs for sale.** Small once credits exist; completes journey 3.
3. **BOM from assemblies, then buying** (suppliers, purchase orders, receiving, stock per
   location). The base for journeys 4, 7, 9 and 10.
4. **CAM to schedule link, work orders and operator screens.** Journeys 5 and 9.
5. **Requests to many shops, comparison, milestones.** Journey 10.
6. **Teams, permissions, change requests, releases.** Journey 8; also where peer-to-peer at
   the scale of hundreds of members has to be proven.
7. **Shipments and carriers; dashboards.** Journeys 9 and 10.
8. **First-run guidance** can be done at any point and is cheap; journey 1 is most people's
   first five minutes.
