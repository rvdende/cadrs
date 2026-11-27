# Onshape import tools

Moving Onshape documents into cadrs with their feature history, not as STEP files.

1. **Scrape** each document's raw REST JSON to `~/work/cadrs_onshape/raw/` (the default data
   folder): feature lists, solved sketches, parts, mass properties, variables, thumbnails.
2. **Import** it with `cadrs-onshape` (crate `crates/cadrs_onshape`). The importer replays every
   Part Studio's features through the cadrs command layer and writes normal cadrs documents
   into the app's document store.
3. **Check** the result against Onshape's own STEP export, feature by feature and part by part,
   and turn every gap into a ticket (see [Tickets](#tickets)).

Only scrape documents you own or have the right to copy; see [Legal and privacy](#legal-and-privacy).

## The transfer app (recommended)

```sh
tools/onshape/build.sh --run     # build the importer + JS checks, then start the app from source
tools/onshape/build.sh           # the same, then package: desktop/dist/ (AppImage + unpacked dir)
tools/onshape/build.sh --test    # only the app's checks (node tests, plus the importer's tests if asked)
```

Requirements: Node.js 18+, a Rust toolchain (the repo's `rust-toolchain.toml`), and the usual
Linux Electron libraries. `python3` is optional: it is only used to decode a Part Studio's
FeatureScript query strings in the Compare tab (the importer itself never needs it).

Start it, leave it and come back to it:

```sh
tools/onshape/onshape.sh start      # in the background
tools/onshape/onshape.sh status     # exit 0 = running, 3 = stopped
tools/onshape/onshape.sh stop       # stops what this script started
tools/onshape/onshape.sh restart
tools/onshape/onshape.sh logs -f    # the app's log (default: last 50 lines)
tools/onshape/onshape.sh run        # foreground, for debugging
```

`start` runs the newest packaged AppImage (`dist/*.AppImage`, extracted to `/tmp` on first run),
or Electron from the checkout's `node_modules` when there is no package. The app is a GUI, so
start it from a desktop session (`DISPLAY`/`WAYLAND_DISPLAY`); the script says so and stops with
an error otherwise. It never kills a process it did not start: the pid file also records the
process's start time, and both must still match. The pid, log and last command live in
`${CADRS_ONSHAPE_STATE:-${XDG_STATE_HOME:-~/.local/state}/cadrs-onshape-importer}/`. Overrides:
`CADRS_ONSHAPE_APPIMAGE`, `CADRS_ONSHAPE_APPIMAGE_EXTRACT=0` (run the AppImage directly, needs
FUSE), `CADRS_ONSHAPE_ELECTRON_ARGS` (default `--no-sandbox`), `CADRS_ONSHAPE_STATE`, and
`CADRS_ONSHAPE_APP_CMD` (start another command, for tests).


A Chromium (Electron) window: Onshape on the right (log in once, the session is kept), the
importer panel on the left. One transfer is:

1. **Log in** to Onshape in the right-hand pane. The app remembers the session for 30 days.
2. **Pick documents** — *Load my documents* lists your account's documents (latest first), or
   paste a shared document's URL / id and press Enter. Documents already scraped (`raw/<id>/`) or
   imported (in the last report) are marked **scraped** / **imported**; an imported one cannot be
   ticked again unless **re-download** is on, so a test run stays on the projects you picked.
3. **Convert** — *Convert selected* downloads each document with `scrape.js`, runs
   `cadrs-onshape import`, and shows progress and a live log. Cancelling stops the download; a
   second click replaces a previous import of the same document (ids derive from Onshape's, so
   nothing is duplicated).
4. **Read the report** — the *Report* tab has the full importer report, the "what is missing"
   summary, the first failure of each Part Studio, and a GitHub-ready bug report. They are also
   saved under `~/work/cadrs_onshape/` (`import-report.txt`, `import-report.json`,
   `missing-summary.txt`, `bug-report.md`).
5. **Open tickets** — the *Tickets* tab groups every translation gap by a stable key and writes
   one Markdown issue draft per group (see [Tickets](#tickets)); the *New ticket* tab is for any
   other cadrs bug (see [New ticket](#new-ticket-any-cadrs-bug-not-just-an-import)).
6. **Dig deeper** — the *FeatureScript* tab lines up the Part Studio's FeatureScript timeline and
   functions with Onshape's regeneration times and cadrs's import times; the *Compare* tab puts
   the imported `.cadrs` next to Onshape's STEP export of the same Part Studio (steps, volume,
   area, bounding box, face counts and face kinds) and exports a Markdown/PDF report or a folder
   with the inputs.
7. **Open in cadrs** — *Open output folder* shows where everything landed; the imported documents
   are in the cadrs app's document store, so they open like any other cadrs document.

Where things are written (default `~/work/cadrs_onshape/`, or `$CADRS_ONSHAPE_ROOT`):

| Path | What |
|---|---|
| `raw/<document id>/` | the scraped REST JSON, one folder per Onshape document |
| `runs/<timestamp>/` | one folder **per conversion**: that run's reports and exports |
| `runs/<timestamp>/import-report.txt` | the importer's human-readable report, one block per document |
| `runs/<timestamp>/import-report.json` | the same run as JSON: features, parts, defects, ticket groups |
| `runs/<timestamp>/missing-summary.txt` | totals and the most common reasons features did not come across |
| `runs/<timestamp>/operations.md` | **operation analysis**: every Onshape feature, its parameters, what cadrs did, what to report |
| `runs/<timestamp>/bug-report.md` | a GitHub-ready report built from that run |
| `runs/<timestamp>/tickets/` | one Markdown issue draft per ticket group, plus `INDEX.md` |
| `runs/<timestamp>/cadrs/`, `steps/` | exported `.cadrs` packages and rebuilt STEP files |
| `runs/<timestamp>/compare-report.md`, `featurescript-analysis.md` | the Compare and FeatureScript outputs of that run |
| `issues/` | general cadrs tickets (the *New ticket* tab), one folder per draft |
| `logs/scrape.log` | the scraper's log |

Nothing report-like lands in the data root any more. Each conversion gets its own `runs/`
folder; the Report tab has a run selector and **Open run folder**, and every report button reads
the selected run. The **Operation analysis** button opens `operations.md`: the feature-by-feature
table (Onshape type and parameters such as `angle=30 deg`, `depth=12 mm`, outcome, notes, links to
the Onshape document) plus a "What to report" list — this is the report to read before opening a
ticket. The cadrs documents themselves go into the cadrs app's store (its usual
`~/.local/share/cadrs/documents` on Linux), with the document names and Part Studios from
Onshape.

### Where the files are (raw scrape, cadrs documents, `.cadrs` packages)

- **Scraped REST JSON** — the importer's input: `<root>/raw/<Onshape document id>/` with
  `document.json`, `elements.json` and, per element, `features.json`, `sketches.json`,
  `bodydetails.json`, `massproperties.json`, `parts.json`, … This is exactly the folder the
  `tree.md` in a scraped document lists; **there are no `.cadrs` files here**.
- **The imported cadrs documents** — the cadrs app's document store, the same one the app opens:
  `~/.local/share/cadrs/documents/<cadrs id>/` on Linux. Each is a folder with `document.ron`,
  `entry.ron` (the name), a thumbnail, blobs and history — not a `.cadrs` package.
  `cadrs --list-documents` lists them; `import-report.json` has each document's `cadrs_id`.
- **Portable `.cadrs` packages** — a zip of `document.ron`, `manifest.ron` and the thumbnail, made
  only when exported. A successful *Convert* writes these automatically into `<run>/cadrs/` (an
  incomplete/defective import is still worth comparing, so it is exported anyway); *Report →
  Export .cadrs files* writes them again on demand. From the command line:

  ```sh
  cadrs-onshape export "Capac2" --out ~/work/cadrs_onshape/runs/<run>/cadrs/Capac2.cadrs
  cadrs-onshape export --all --out ~/work/cadrs_onshape/runs/<run>/cadrs
  ```

  `--data-dir <store>` picks another store; without `--out`, one document goes to `./<name>.cadrs`
  and several/`--all` to `./cadrs-exports/`. Add `--out` paths to an issue with the *New ticket*
  tab's **Attach files…** (or the import *Tickets* drafts' attachment list).
  Before exporting, the app checks the last report: it must not be a dry run, every document must
  still be in the store, and it must have parts — otherwise it **refuses with the reason** and
  starts no rebuild. Exports run in child processes with progress and **Cancel export**; a
  document that does not finish within 2 min (.cadrs) or 3 min (STEP) is refused with a message
  that a very large Part Studio belongs on the command line (`CADRS_EXPORT_TIMEOUT_MS` overrides
  the limit).
- **A dry run** — `--dry-run` or the app's *dry run* checkbox — only builds and reports: it writes
  no document into the store, so there is nothing to export until it is unchecked and converted.

### Rebuilt from Onshape's data — and how to compare with Onshape's STEP

The import is not a data copy: every feature of the Part Studio is replayed through the cadrs
command layer and **rebuilt by the cadrs kernel** while it is imported (that is how the report
knows each part's volume, area and face count and compares them with Onshape's own mass
properties). A `.cadrs` package contains that feature history, not a cached STEP: it is rebuilt
again when opened or exported.

To compare a comparison with Onshape's own export:

- **Directly**: `cadrs-onshape compare <file.cadrs> <Onshape STEP>` rebuilds the document and
  measures it against the STEP (volume, area, box, faces, face kinds) as JSON — the app's
  *Compare* tab runs the same code.
- **As STEP files**: `cadrs-onshape step <name or id>… --out <dir>` (or the app's *Report →
  Export STEP files*, `<run>/steps/`) rebuilds each Part Studio and writes STEP, one file per
  Part Studio; open them next to Onshape's STEP in any CAD tool.

Example on the real import fixture: the four 5-feature Part Studio rebuilds to 88 651.8177 mm³,
22 658.7370 mm², 10 faces, box `[-50,-50,0]..[50,50,25]`; our `.cadrs` and Onshape's STEP both
measure exactly that, so the two STEP files agree.


### If the Onshape pane does not draw (WebGPU/WebGL) or is stuck on a help page

Electron puts its own name in the user agent, and Linux Chromium leaves WebGPU off and can
blocklist older GPUs, so Onshape can decide the browser cannot draw and send you to its graphics
help pages. The app presents a plain Chrome user agent, asks Chromium for WebGPU
(`ignore-gpu-blocklist`, `enable-unsafe-webgpu`, and `enable-features=Vulkan` on Linux) before the
window opens, and the pane has its own navigation: **← → ⟳ ⌂**, `Alt+Left`/`Alt+Right`,
`Ctrl+R`/`F5`, `Alt+Home`. Clicking a document's name opens it in the pane.

**Check GPU** (under the navigation buttons) runs the probes Onshape uses and shows what Chromium
sees: `WebGPU yes/no (adapter) · WebGL2 yes/no · renderer`. If `WebGL2` is NO, Electron cannot
draw Onshape on that machine: update the Mesa/Vulkan drivers, or scrape with the manual path
(`receiver.py` plus the console snippet in [Scraping](#scraping-manual-without-the-app)) from a
normal browser and import the downloaded folder. Troubleshooting switches:
`CADRS_ONSHAPE_DISABLE_GPU=1` (software rendering), `CADRS_ONSHAPE_GPU=0` (start with no GPU
switches at all), `CADRS_ONSHAPE_EXTRA_FEATURES=…` (extra Chromium features).

**Onshape says "the WebGL rendering context has been lost"** — a GPU process reset, which older
Intel/Mesa drivers do on heavy documents. The app notices it (the page's own
`webglcontextlost` signal, `child-process-gone` for the GPU process, `render-process-gone` for the
pane), shows a banner and reloads Onshape automatically up to three times in five minutes. After that — or straight away, if you prefer — press **Restart in
software mode**: the same app is relaunched with `--disable-gpu` (SwiftShader), keeping the
Onshape login; drawing is slower but stable. From a terminal that is
`CADRS_ONSHAPE_DISABLE_GPU=1 admin_local_tools/onshape.sh restart`.

## Tickets

The importer's report says exactly why a feature fell short ("none of its 1 selections could be
translated", "no importer for this feature type yet", "2 × DISTANCE (unsupported references)
constraint dropped" …). Every such reason is a **defect** with a stable key,
`onshape-import:<feature kind>:<normalized reason>`, so the same gap in ten documents is one
ticket, not ten. `import-report.json` carries the defects and the groups
(`tickets`: key, severity, count, examples); the *Tickets* tab renders and exports them.

Each draft has the title, severity and count, the affected documents/Part Studios/features, the
feature timeline where the raw scrape is available, steps to reproduce, and what to attach
(`.cadrs`, Onshape STEP, `.fs`, the scraped folder). *Copy issue* puts it on the clipboard and
*Open GitHub issue* opens the new-issue page; nothing is posted automatically. Only attach files
you are allowed to share, and check for an existing issue with the same title first — the draft's
key is in its body.

Standalone use:

```sh
node tools/onshape/desktop/tickets.js ~/work/cadrs_onshape/import-report.txt \
    --raw ~/work/cadrs_onshape/raw --out ~/work/cadrs_onshape/tickets
# the same from a checkout without starting the app:
node tools/onshape/tickets.mjs ~/work/cadrs_onshape/import-report.json
```

The "what is missing" summaries the app shows are also standalone
(`node tools/onshape/summarize.mjs`, `node tools/onshape/roots.mjs`); the Python
`summarize.py`/`roots.py` remain for anyone using them directly.

## New ticket: any cadrs bug, not just an import

The *New ticket* tab is for problems that are not about an Onshape import (sketch/feature bugs,
wrong results, UI issues). It keeps the two things GitHub needs as **separate streams**:

- the **text** (what happened, expected, steps) in `issue.md` — copied to the clipboard and pasted
  as the issue body;
- the **attachments** copied into `attachments/` next to it — drag them into the issue, because
  GitHub cannot attach files through a URL.

A **template** dropdown starts from the repo's issue structure: *Bug report* (Summary / Steps to
reproduce / Expected / Actual / Environment / Evidence) or *Feature request (cadrs)* (Summary /
Related issues / Current behavior / Evidence / Expected behavior / Acceptance criteria /
Implementation notes / Why this matters); the same structure exists on GitHub as the **CADRS
feature request** issue form. The tab writes **Type** (Bug / Missing feature / Improvement /
Other), **Where** (cadrs.net web app / GitHub build / Onshape importer / Other) and an optional
**Link** (an Onshape share URL or a repro link) into the body and as suggested labels. GitHub
accepts images, PDFs and ZIPs only, so the draft also writes `attachments.zip` with every attached
file (a `.cadrs`/`.step` can be dropped in as that one zip). A draft is a folder under
`<root>/issues/` (`$CADRS_ONSHAPE_ROOT`, by default `~/work/cadrs_onshape/issues/`):

```
<root>/issues/2026-11-26-07-30-12-extrude-of-selected-sketch-regions/
  issue.md        the body (with an ### Attachments list at the end)
  attachments/    the files themselves
  issue.json      title, labels, body, attachment names, the GitHub URL
<root>/issues/INDEX.md
```

The same works without the app:

```sh
node tools/onshape/desktop/issues.js --title "Extrude of selected regions fills the holes" \
    --body-file body.md --attach test_26nov.cadrs --attach before.png --root ~/work/cadrs_onshape
# then: open the printed GitHub URL, paste issue.md as the body, drag attachments/ into it
# (with gh authenticated: gh issue create --repo rvdende/cadrs --title "…" --body-file issue.md)
```

## Scraping (manual, without the app)

These tools are for exporting **your own** Onshape documents, with their feature history, so
you can keep working on them in cadrs. You are responsible for using them within Onshape's
terms of use; only scrape documents you own or have the right to copy, and respect the rate
limits below. cadrs is not affiliated with or endorsed by PTC or Onshape.

The scraper runs inside your logged-in cad.onshape.com tab, so its requests are made with your
own session, and only see what your account can see.

```sh
python3 tools/onshape/receiver.py            # 127.0.0.1:8765, writes to ~/work/cadrs_onshape
```

Then, in the browser console of a cad.onshape.com tab:

```js
eval(await (await fetch('http://127.0.0.1:8765/scrape.js', {targetAddressSpace: 'loopback'})).text());
cadrsScrape.start();          // or start({only: ['<document id>', …]})
cadrsScrape.state             // progress
```

Chrome asks once to allow access to the local network. Files already on disk are skipped, so
a rerun resumes where the last one stopped. Onshape also limits session requests: on
2026-09-29 it answered HTTP 429 with a `Retry-After` of about 23.5 hours after roughly 1,280
requests. Spread large scrapes over days.

`python3 tools/onshape/survey.py` lists which feature, sketch-entity and constraint types the
scraped documents use.

## Importing from the command line

```sh
cargo run -r -p cadrs_onshape -- import                     # every scraped document
cargo run -r -p cadrs_onshape -- import cabinet --dry-run   # build and report, don't save
cargo run -r -p cadrs_onshape -- import --json report.json  # also write the JSON report
cargo run -r -p cadrs_onshape -- list [--json]                       # what is in the store
cargo run -r -p cadrs_onshape -- export "Capac2" --out Capac2.cadrs  # a portable .cadrs package
cargo run -r -p cadrs_onshape -- step "Capac2" --out steps/          # rebuilt STEP, one per Part Studio
```

- Imported documents keep ids derived from Onshape's, so importing again replaces them.
- Each document is imported in a child process with a time limit. If a rebuild hangs in the
  kernel, the feature it hung on is left out and the document is imported again without it
  (`--skip <feature ids>` does that by hand; `--timeout <s>` sets the limit).
- The report goes to stdout and to `<raw>/../import-report.txt`; `--json <file>` writes the same
  run as JSON for tools and tests. `CADRS_ONSHAPE_TRACE=1` prints each feature's time.
  `CADRS_ONSHAPE_DEBUG=1` prints the selections and sketch planes that could not be translated.
- `cadrs-onshape compare <file.cadrs> <file.step>` rebuilds a document and measures it against a
  STEP file, as JSON (the Compare tab's engine).
- `cadrs-onshape check-queries` decodes every geometry query in the scraped feature lists.

## Verifying an import

Nothing private is committed: the end-to-end check runs against a document of your own.

```sh
cargo test -r -p cadrs_onshape                                   # report unit tests
node --test tools/onshape/desktop/test/*.test.js                  # the app's checks (63 tests)
CADRS_E2E_RAW=~/work/cadrs_onshape/raw \
CADRS_E2E_STEP="/path/on a scraped document's Part Studio.step" \
  node tools/onshape/e2e/verify.mjs                              # the whole chain, one command
```

`tools/onshape/e2e/verify.mjs` copies the scrape to a temporary folder (so no report lands in the
repo), imports it with `--dry-run` and for real, validates the JSON report (shape, counts,
defects and tickets), re-imports to check the cadrs id stays the same, and compares the rebuilt
document with `CADRS_E2E_STEP` (volume, area, faces). It also runs the app's node tests and
checks a packaged build when present; it prints a PASS/SKIP/FAIL table and exits non-zero on
failure. Without `CADRS_E2E_RAW`/`CADRS_E2E_STEP` the document steps are skipped, but the
report/ticket self-check, the app tests and the package check still run. Use it before opening a
PR that touches the importer, so a regression in features, geometry or the report is caught
without hitting Onshape repeatedly.

## What does not come across

The importer replays Onshape's **built-in** feature types (sketches and their geometry and
constraints, extrude, revolve, sweep, loft, fillet, chamfer, shell, draft, hole, patterns,
mirror, booleans, planes, mate connectors, thicken, split, frames, sheet metal, and the
query/reference forms they use). Custom FeatureScript features, very new feature types, some
selection forms, and parts of assemblies can be reported as partial or skipped; the report and
the tickets say which feature and why. If you hit one, open a ticket with the report excerpt and
a minimal document — that is exactly what makes the next version translate more.

## SolidCraft recipes

SolidCraft (<https://github.com/tangency-labs/solidcraft>, MIT) describes parts as JSON operation
lists (`recipe.json`). The same operations are already exercised by cadrs' reference-part suite,
so a translator is included:

```sh
cargo run -r -p cadrs_solidcraft -- import <recipe.json> --data-dir <cadrs store>
cargo run -r -p cadrs_solidcraft -- check <recipe.json>   # compare with expected.json
```

See `crates/cadrs_solidcraft/README.md` for the supported operation set, the coverage over
SolidCraft's 90 reference recipes, and why the result is a cadrs document (editable features)
rather than a replay of SolidCraft's internal operation list.

## Legal and privacy

- Only scrape documents you own or are allowed to copy. The scraper runs with your own Onshape
  session and sees only what your account sees; you are responsible for respecting Onshape's
  terms of service and request limits.
- The app never uploads your documents anywhere; everything is local. `import-report.json`,
  `tickets/` and the error reports include document and feature names — review them before
  pasting them into a public issue.
- cadrs is not affiliated with or endorsed by PTC or Onshape.
