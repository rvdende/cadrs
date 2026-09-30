# Onshape import tools

Moving Onshape documents into cadrs with their feature history, not as STEP files.

1. **Scrape** each document's raw REST JSON to `~/work/cadrs_onshape/raw/` (the default data
   folder): feature lists, solved sketches, parts, mass properties, variables, thumbnails.
2. **Import** it with `cadrs-onshape` (crate `crates/cadrs_onshape`). The importer replays every
   Part Studio's features through the cadrs command layer and writes normal cadrs documents
   into the app's document store.

## Scraping

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

## Importing

```sh
cargo run -p cadrs_onshape -- import                  # every scraped document
cargo run -p cadrs_onshape -- import cabinet --dry-run
python3 tools/onshape/summarize.py                    # what is missing, most common first
```

- Imported documents keep ids derived from Onshape's, so importing again replaces them.
- Each document is imported in a child process with a time limit. If a rebuild hangs in the
  kernel, the feature it hung on is left out and the document is imported again without it.
- The report (`~/work/cadrs_onshape/import-report.txt`) lists every feature that did not come
  across fully and says why. It also compares each part's volume with Onshape's.
- `CADRS_ONSHAPE_TRACE=1` prints each feature's time. `CADRS_ONSHAPE_DEBUG=1` prints the
  selections and sketch planes that could not be translated.

`query.py` is the Python prototype of the `qCompressed` query decoder (`src/query.rs`).
Run it on a `features.json` to read the geometry references a feature makes.
