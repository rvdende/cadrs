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
   paste a shared document's URL / id and press Enter.
3. **Convert** — *Convert selected* downloads each document with `scrape.js`, runs
   `cadrs-onshape import`, and shows progress and a live log. Cancelling stops the download; a
   second click replaces a previous import of the same document (ids derive from Onshape's, so
   nothing is duplicated).
4. **Read the report** — the *Report* tab has the full importer report, the "what is missing"
   summary, the first failure of each Part Studio, and a GitHub-ready bug report. They are also
   saved under `~/work/cadrs_onshape/` (`import-report.txt`, `import-report.json`,
   `missing-summary.txt`, `bug-report.md`).
5. **Open tickets** — the *Tickets* tab groups every translation gap by a stable key and writes
   one Markdown issue draft per group (see [Tickets](#tickets)).
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
| `import-report.txt` | the importer's human-readable report, one block per document |
| `import-report.json` | the same run as JSON: features, parts, defects, ticket groups |
| `missing-summary.txt` | totals and the most common reasons features did not come across |
| `bug-report.md` | a GitHub-ready report built from the last import |
| `tickets/` | one Markdown issue draft per ticket group, plus `INDEX.md` |
| `compare-report.md` | the last Compare tab report |
| `featurescript-analysis.md` | the last FeatureScript analysis |

The cadrs documents themselves go into the cadrs app's store (its usual
`~/.local/share/cadrs/documents` on Linux), with the document names and Part Studios from
Onshape.

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
