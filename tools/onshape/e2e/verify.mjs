#!/usr/bin/env node
// End-to-end verification of the Onshape importer: the built binary, the report JSON, the
// desktop app's tests and its packaged build.
//
//   node tools/onshape/e2e/verify.mjs [--keep] [--verbose] [--help]
//
// The import/compare chain needs a scraped Onshape document and (for compare) Onshape's STEP
// export of the same Part Studio. Point it at your own, which never leaves your machine:
//
//   CADRS_E2E_RAW=~/work/cadrs_onshape/raw \
//   CADRS_E2E_STEP=~/work/cadrs_onshape/Part\ Studio\ 1.step \
//   node tools/onshape/e2e/verify.mjs
//
// Without them the import/compare steps are skipped (the report/ticket checks still run on a
// synthetic report, so the checker itself is exercised).
//
// Environment:
//   CADRS_ONSHAPE_BIN       the importer binary (default <repo>/target/release/cadrs-onshape)
//   CADRS_E2E_RAW           scraped raw dir (the folder holding <document id>/document.json)
//   CADRS_E2E_STEP          Onshape's STEP export of the document's Part Studio
//   CADRS_E2E_DOCUMENT      document name or id to import (default: every scraped document)
//   CADRS_E2E_JSON=0        skip the JSON-report checks (for a binary without `--json`)
//   CADRS_E2E_SKIP_APP=1    skip the desktop app / packaged app checks
//
// No npm packages, node >= 18, offline. All paths are resolved from this file, so it runs from
// any working directory. The scraped raw folder is copied to a temp dir first (the importer
// writes `import-report.txt` beside it), and everything under the temp dir is removed on exit
// unless --keep is given.

import { spawnSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

const args = process.argv.slice(2);
const keep = args.includes('--keep');
const verbose = args.includes('--verbose') || args.includes('-v');
if (args.includes('--help') || args.includes('-h')) {
  console.log(
    'usage: node tools/onshape/e2e/verify.mjs [--keep] [--verbose]\n' +
      '  CADRS_ONSHAPE_BIN, CADRS_E2E_RAW, CADRS_E2E_STEP, CADRS_E2E_DOCUMENT,\n' +
      '  CADRS_E2E_JSON=0, CADRS_E2E_SKIP_APP=1 are honoured.',
  );
  process.exit(0);
}

const BIN = resolve(
  process.env.CADRS_ONSHAPE_BIN || join(REPO, 'target', 'release', 'cadrs-onshape'),
);
const RAW = process.env.CADRS_E2E_RAW ? resolve(process.env.CADRS_E2E_RAW) : null;
const STEP = process.env.CADRS_E2E_STEP ? resolve(process.env.CADRS_E2E_STEP) : null;
const FILTER = process.env.CADRS_E2E_DOCUMENT || null;
const JSON_CHECKS = process.env.CADRS_E2E_JSON !== '0';
const SKIP_APP = process.env.CADRS_E2E_SKIP_APP === '1';
const HAS_DOCUMENT = Boolean(RAW && STEP);

const results = [];
let jsonMode = JSON_CHECKS; // turned off if this binary has no --json
let usageText = '';
let tempRoot = null;
let dataDir = null;
let rawCopy = null;
let reports = []; // the parsed --json reports of the full import

class Fail extends Error {}

function check(condition, message) {
  if (!condition) throw new Fail(message);
}

function close(a, b, relative) {
  return Math.abs(a - b) <= Math.abs(b) * relative;
}

function describe(value) {
  return typeof value === 'string' ? value : JSON.stringify(value);
}

function tail(text, lines = 12) {
  return text.trim().split('\n').slice(-lines).join('\n');
}

function run(command, argv, options = {}) {
  const result = spawnSync(command, argv, {
    encoding: 'utf8',
    cwd: REPO,
    timeout: options.timeout ?? 240_000,
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, ...options.env },
  });
  if (verbose) {
    console.log(`\n$ ${command} ${argv.join(' ')}`);
    if (result.stdout) console.log(result.stdout.trimEnd());
    if (result.stderr) console.log(result.stderr.trimEnd());
  }
  return result;
}

function succeeded(result, what) {
  if (result.error) throw new Fail(`${what}: ${result.error.message}`);
  check(
    result.status === 0,
    `${what} exited ${result.status}\n${tail(result.stderr || result.stdout || '')}`,
  );
  return result;
}

function readJson(path, what) {
  check(existsSync(path), `${what}: ${path} was not written`);
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    throw new Fail(`${what}: ${path} is not JSON (${error.message})`);
  }
}

async function step(name, fn) {
  const started = process.hrtime.bigint();
  try {
    const value = await fn();
    const ms = Number(process.hrtime.bigint() - started) / 1e6;
    if (value && typeof value === 'object' && 'skip' in value) {
      results.push({ name, status: 'SKIP', detail: value.skip, ms });
    } else {
      results.push({ name, status: 'PASS', detail: value || '', ms });
    }
  } catch (error) {
    const ms = Number(process.hrtime.bigint() - started) / 1e6;
    results.push({
      name,
      status: 'FAIL',
      detail: error instanceof Fail ? error.message : String(error?.stack || error),
      ms,
    });
  }
}

function skip(name, reason) {
  results.push({ name, status: 'SKIP', detail: reason, ms: 0 });
}

// --- report JSON (see crates/cadrs_onshape/src/report.rs) --------------------------------

const OUTCOMES = ['full', 'partial', 'skipped', 'suppressed'];

/** The parts of the report that hold for any document: shape, counts, defects and tickets. */
function validateReportShape(json, { dryRun }) {
  check(json.kind === 'cadrs-onshape-import', `kind is ${describe(json.kind)}`);
  check(json.schema === 1, `schema is ${describe(json.schema)}`);
  check(typeof json.tool === 'string' && json.tool.startsWith('cadrs-onshape'), 'tool');
  check(typeof json.user === 'string' && json.user.length > 0, 'user');
  check(json.dry_run === dryRun, `dry_run is ${describe(json.dry_run)}, wanted ${dryRun}`);
  check(Array.isArray(json.documents) && json.documents.length >= 1, 'documents[]');
  check(Array.isArray(json.tickets), 'tickets[]');
  check(typeof json.summary === 'object' && json.summary !== null, 'summary');

  let features = { total: 0, full: 0, partial: 0, skipped: 0, suppressed: 0 };
  let parts = { matched: 0, total: 0 };
  let defects = 0;
  let partStudios = 0;
  for (const doc of json.documents) {
    if (doc.failed) continue;
    check(typeof doc.name === 'string' && doc.name.length > 0, 'document name');
    check(typeof doc.onshape_id === 'string' && doc.onshape_id.length > 0, 'document onshape_id');
    check(Array.isArray(doc.elements), `${doc.name}: elements[]`);
    check(Array.isArray(doc.defects), `${doc.name}: defects[]`);
    for (const outcome of OUTCOMES) {
      check(
        typeof doc.counts?.features?.[outcome] === 'number',
        `${doc.name}: counts.features.${outcome}`,
      );
    }
    check(typeof doc.counts?.parts?.matched === 'number', `${doc.name}: counts.parts.matched`);
    check(typeof doc.counts?.parts?.total === 'number', `${doc.name}: counts.parts.total`);
    check(
      Array.isArray(doc.elements) &&
        doc.elements.some((element) => element.kind === 'partstudio'),
      `${doc.name}: no partstudio element`,
    );
    for (const element of doc.elements) {
      check(typeof element.name === 'string', 'element name');
      check(typeof element.kind === 'string', 'element kind');
      check(typeof element.imported === 'boolean', `${element.name}: imported`);
      check(Array.isArray(element.features), `${element.name}: features[]`);
      check(Array.isArray(element.parts), `${element.name}: parts[]`);
      for (const outcome of OUTCOMES) {
        check(
          typeof element.counts?.[outcome] === 'number',
          `${element.name}: counts.${outcome}`,
        );
      }
      for (const feature of element.features) {
        check(typeof feature.name === 'string', 'feature name');
        check(typeof feature.kind === 'string', 'feature kind');
        check(
          OUTCOMES.includes(feature.outcome),
          `${feature.name}: outcome ${describe(feature.outcome)}`,
        );
        check(Array.isArray(feature.notes), `${feature.name}: notes[]`);
      }
      partStudios += element.kind === 'partstudio' ? 1 : 0;
      for (const part of element.parts) {
        check(typeof part.name === 'string', 'part name');
        check(typeof part.onshape_volume === 'number', `${part.name}: onshape_volume`);
        check(
          part.cadrs_volume === null || typeof part.cadrs_volume === 'number',
          `${part.name}: cadrs_volume`,
        );
        check(typeof part.match === 'boolean', `${part.name}: match`);
        check(part.faces === null || (Array.isArray(part.faces) && part.faces.length === 2), `${part.name}: faces`);
      }
      for (const outcome of OUTCOMES) {
        features[outcome] += element.counts[outcome];
      }
      features.total += element.features.length;
      parts.matched += element.parts.filter((part) => part.match).length;
      parts.total += element.parts.length;
    }
    defects += doc.defects.length;
    checkDefectTickets(json, doc);
  }
  check(
    json.summary.part_studios === partStudios,
    `summary.part_studios ${json.summary.part_studios} vs ${partStudios}`,
  );
  check(json.summary.features.total === features.total, 'summary.features.total');
  check(json.summary.parts.total === parts.total, 'summary.parts.total');
  check(json.summary.parts.matched === parts.matched, 'summary.parts.matched');
  check(json.summary.defects === defects, 'summary.defects');
  check(json.summary.tickets === json.tickets.length, 'summary.tickets vs tickets[]');
  return json;
}

/** Every skipped or partial feature is a defect, and each defect is in a ticket whose count
 *  is >= 1, with all defects accounted for. Shared by the real run and the synthetic check
 *  below, so the checking itself is known to catch a bad report. */
function checkDefectTickets(json, doc) {
  const partialOrSkipped = [];
  for (const element of doc.elements) {
    for (const feature of element.features || []) {
      if (feature.outcome === 'partial' || feature.outcome === 'skipped') {
        partialOrSkipped.push({ element: element.name, feature });
      }
    }
  }
  for (const { element, feature } of partialOrSkipped) {
    check(
      doc.defects.some(
        (d) => d.element === element && d.feature === feature.name && d.outcome === feature.outcome,
      ),
      `defect missing for ${element} / ${feature.name} (${feature.outcome})`,
    );
  }
  for (const defect of doc.defects) {
    check(typeof defect.key === 'string' && defect.key.length > 0, 'defect.key');
    check(defect.severity === 'high' || defect.severity === 'medium', `defect.severity ${defect.severity}`);
  }
  const grouped = new Map(json.tickets.map((t) => [t.key, t]));
  for (const defect of doc.defects) {
    const ticket = grouped.get(defect.key);
    check(ticket, `no ticket for defect key ${defect.key}`);
    check(ticket.count >= 1, `ticket ${ticket.key} count ${ticket.count}`);
    check(ticket.severity === 'high' || ticket.severity === 'medium', `ticket severity ${ticket.severity}`);
    check(Array.isArray(ticket.examples) && ticket.examples.length >= 1, `ticket ${ticket.key} examples`);
  }
  const ticketCount = json.tickets.reduce((n, t) => n + t.count, 0);
  check(
    ticketCount === doc.defects.length,
    `tickets cover ${ticketCount} defects, document has ${doc.defects.length}`,
  );
}

/** A report with one skipped and one partial feature must be accepted; the same report with a
 *  feature missing from `defects`, an ungrouped defect   or a zero-count ticket must be
 *  rejected. */
function reportSelfCheck() {
  const key = 'onshape-import:extrude:unsupported-thing';
  const defect = (feature, outcome) => ({
    key,
    severity: 'high',
    document: 'synthetic',
    element: 'Part Studio 1',
    element_kind: 'partstudio',
    feature,
    feature_kind: 'extrude',
    outcome,
    reason: 'r',
  });
  const defects = [defect('Skipped 1', 'skipped'), defect('Partial 1', 'partial')];
  const report = {
    summary: { defects: defects.length, tickets: 1 },
    tickets: [
      {
        key,
        severity: 'high',
        count: defects.length,
        reason: 'r',
        feature_kinds: ['extrude'],
        outcomes: ['partial', 'skipped'],
        examples: defects.map((d) => ({
          document: d.document,
          element: d.element,
          feature: d.feature,
          outcome: d.outcome,
          reason: d.reason,
        })),
      },
    ],
    documents: [],
  };
  const documentWith = (defects) => ({
    name: 'synthetic',
    onshape_id: 'synthetic',
    cadrs_id: 'synthetic',
    defects,
    elements: [
      {
        name: 'Part Studio 1',
        kind: 'partstudio',
        features: [
          { name: 'Skipped 1', kind: 'extrude', outcome: 'skipped', notes: [] },
          { name: 'Partial 1', kind: 'extrude', outcome: 'partial', notes: ['approximated'] },
          { name: 'Full 1', kind: 'extrude', outcome: 'full', notes: [] },
        ],
        parts: [],
      },
    ],
  });
  const doc = documentWith(defects);
  checkDefectTickets(report, doc);
  const rejected = (fn, why) => {
    try {
      fn();
    } catch {
      return true;
    }
    throw new Fail(`the report check accepted ${why}`);
  };
  rejected(() => checkDefectTickets(report, documentWith([defects[0]])), 'a feature missing from defects');
  rejected(() => checkDefectTickets({ ...report, tickets: [] }, doc), 'ungrouped defects');
  rejected(
    () => checkDefectTickets({ ...report, tickets: [{ ...report.tickets[0], count: 0 }] }, doc),
    'a zero-count ticket',
  );
  return 'synthetic report: grouping accepted, missing defect / ungrouped / count 0 rejected';
}

// --- steps -------------------------------------------------------------------------------

function preflight() {
  check(existsSync(BIN), `no importer binary at ${BIN} (build: cargo build -r -p cadrs_onshape --offline)`);
  check(statSync(BIN).isFile(), `${BIN} is not a file`);
  // Merely starting it is enough: the CLI prints its usage and exits 2 with no arguments.
  const probe = run(BIN, ['--help'], { timeout: 30_000 });
  check(!probe.error, `cannot execute ${BIN}: ${probe.error?.message}`);
  usageText = `${probe.stdout || ''}\n${probe.stderr || ''}`;
  return `${BIN}${/\B--json\b/.test(usageText) ? ' (has --json)' : ''}`;
}

function documentFiles() {
  check(HAS_DOCUMENT, 'no document given');
  check(existsSync(RAW), `scraped raw dir missing: ${RAW}`);
  check(existsSync(STEP), `STEP export missing: ${STEP}`);
  const documents = readdirSync(RAW).filter(
    (name) => /^[0-9a-f]{24}$/.test(name) && existsSync(join(RAW, name, 'document.json')),
  );
  check(documents.length > 0, `no scraped document under ${RAW}`);
  const step = readFileSync(STEP, 'utf8');
  check(step.startsWith('ISO-10303-21;'), `${STEP} is not a STEP file`);
  return `${documents.length} document(s) in ${RAW} -> ${STEP}`;
}

function importArgs(extra) {
  const argv = ['import'];
  if (FILTER) argv.push(FILTER);
  argv.push('--raw', rawCopy, '--data-dir', dataDir);
  return argv.concat(extra);
}

async function dryRunImport() {
  const jsonPath = join(tempRoot, 'dry-run.json');
  let result = run(BIN, importArgs(['--dry-run'].concat(jsonMode ? ['--json', jsonPath] : [])));
  if (jsonMode && jsonFlagUnsupported(result)) {
    jsonMode = false;
    result = run(BIN, importArgs(['--dry-run']));
  }
  succeeded(result, 'import --dry-run');
  const text = `${result.stdout}\n${result.stderr}`;
  check(/full,\s*\d+\s*partial,\s*\d+\s*skipped/.test(text), `report text lacks the feature counts:\n${tail(result.stdout)}`);
  if (jsonMode) {
    const report = validateReportShape(readJson(jsonPath, 'dry-run report'), { dryRun: true });
    return `${report.documents.length} document(s), JSON schema 1`;
  }
  return 'text report only (no --json)';
}

async function fullImport() {
  const jsonPath = join(tempRoot, 'import.json');
  let result = run(BIN, importArgs(jsonMode ? ['--json', jsonPath] : []));
  if (jsonMode && jsonFlagUnsupported(result)) {
    jsonMode = false;
    result = run(BIN, importArgs([]));
  }
  succeeded(result, 'import');
  check(/match Onshape/.test(`${result.stdout}\n${result.stderr}`), `report text lacks the parts match:\n${tail(result.stdout)}`);
  if (!jsonMode) return 'saved (no --json)';
  const report = validateReportShape(readJson(jsonPath, 'import report'), { dryRun: false });
  reports = report.documents;
  for (const doc of reports) {
    if (doc.failed) continue;
    check(typeof doc.cadrs_id === 'string' && doc.cadrs_id.length > 0, `${doc.name}: cadrs_id`);
    check(
      existsSync(join(dataDir, doc.cadrs_id, 'document.ron')),
      `the import did not write ${join(dataDir, doc.cadrs_id, 'document.ron')}`,
    );
  }
  return `saved ${reports.filter((doc) => !doc.failed).length} document(s)`;
}

async function reImport() {
  if (!jsonMode || reports.length === 0) {
    return { skip: 'needs the --json report for the cadrs ids' };
  }
  const jsonPath = join(tempRoot, 'reimport.json');
  succeeded(run(BIN, importArgs(['--json', jsonPath])), 're-import');
  const again = validateReportShape(readJson(jsonPath, 're-import report'), { dryRun: false }).documents;
  for (const doc of reports) {
    const next = again.find((candidate) => candidate.onshape_id === doc.onshape_id);
    check(next, `${doc.name}: missing from the re-import report`);
    check(next.cadrs_id === doc.cadrs_id, `${doc.name}: cadrs_id changed ${doc.cadrs_id} -> ${next.cadrs_id}`);
  }
  return `${again.length} document(s), same cadrs id(s)`;
}

async function compareStep() {
  const doc = reports.find((candidate) => !candidate.failed);
  check(doc, 'no imported document to compare');
  const documentPath = join(dataDir, doc.cadrs_id, 'document.ron');
  const result = run(BIN, ['compare', documentPath, STEP]);
  succeeded(result, 'compare');
  let json;
  try {
    json = JSON.parse(result.stdout);
  } catch (error) {
    throw new Fail(`compare did not print JSON (${error.message}): ${tail(result.stdout)}`);
  }
  check(typeof json.document === 'string', 'compare: document');
  check(Array.isArray(json.studios) && json.studios.length >= 1, 'compare: studios[]');
  check(Array.isArray(json.step) && json.step.length >= 1, 'compare: step[]');
  const studio = json.studios[0];
  check(Array.isArray(studio.parts) && studio.parts.length >= 1, 'compare: parts[]');
  const part = studio.parts[0];
  const step = json.step[0];
  check(typeof part.volume === 'number', 'compare: cadrs volume');
  check(typeof step.volume === 'number', 'compare: STEP volume');
  check(typeof part.faces === 'number', 'compare: cadrs faces');
  check(
    part.faces === step.faces || part.faces > 0,
    `compare: faces ${part.faces} vs STEP ${step.faces}`,
  );
  return `${studio.name}: cadrs ${part.volume.toFixed(2)} mm³ / ${part.faces} faces vs STEP ${step.volume.toFixed(2)} mm³`;
}

function jsonFlagUnsupported(result) {
  const text = `${result.stdout || ''}\n${result.stderr || ''}`.toLowerCase();
  return (
    result.status !== 0 &&
    !(result.stdout || '').trim() &&
    /unknown|unexpected argument|usage:|unrecognized/.test(text)
  );
}

function appTests() {
  if (SKIP_APP) return { skip: 'CADRS_E2E_SKIP_APP=1' };
  const dir = join(REPO, 'tools', 'onshape', 'desktop', 'test');
  if (!existsSync(dir)) return { skip: 'tools/onshape/desktop/test does not exist' };
  const tests = [];
  const walk = (current, depth) => {
    if (depth > 4) return;
    for (const name of readdirSync(current)) {
      if (name === 'node_modules') continue;
      const path = join(current, name);
      if (statSync(path).isDirectory()) walk(path, depth + 1);
      else if (/\.test\.(js|mjs|cjs)$/.test(name)) tests.push(path);
    }
  };
  walk(dir, 0);
  if (tests.length === 0) {
    return { skip: 'no *.test.js under tools/onshape/desktop/test' };
  }
  // Passing the files explicitly works on every node >= 18 (`--test <dir>` does not).
  const result = run(process.execPath, ['--test', ...tests]);
  succeeded(result, 'node --test tools/onshape/desktop/test');
  return `${tests.length} test file(s) passed`;
}

function packagedApp() {
  if (SKIP_APP) return { skip: 'CADRS_E2E_SKIP_APP=1' };
  const dist = join(REPO, 'tools', 'onshape', 'desktop', 'dist');
  if (!existsSync(dist)) {
    return { skip: 'tools/onshape/desktop/dist does not exist (run tools/onshape/build.sh)' };
  }
  const found = { scrape: [], binary: [] };
  const walk = (current, depth) => {
    if (depth > 10) return;
    for (const name of readdirSync(current)) {
      if (name === 'node_modules') continue;
      const path = join(current, name);
      const stat = statSync(path);
      if (stat.isDirectory()) {
        walk(path, depth + 1);
      } else if (name === 'scrape.js') {
        found.scrape.push(path);
      } else if (name === 'cadrs-onshape' || name === 'cadrs-onshape.exe') {
        found.binary.push(path);
      }
    }
  };
  walk(dist, 0);
  check(found.scrape.length > 0, `no scrape.js in ${dist}`);
  check(found.binary.length > 0, `no bin/cadrs-onshape in ${dist}`);
  return `${basename(found.scrape[0])} + ${basename(dirname(found.binary[0]))}/${basename(found.binary[0])}`;
}

// --- main --------------------------------------------------------------------------------

function printTable() {
  const width = Math.max(...results.map((r) => r.name.length), 'STEP'.length);
  console.log(`\n${'STEP'.padEnd(width)}  STATUS  TIME     DETAIL`);
  console.log('-'.repeat(width + 2 + 6 + 9 + 8));
  for (const r of results) {
    const time = `${Math.round(r.ms)}ms`.padStart(7);
    const detail = (r.detail || '').replace(/\s+/g, ' ');
    console.log(`${r.name.padEnd(width)}  ${r.status.padEnd(6)}  ${time}  ${detail}`);
  }
}

async function main() {
  console.log('Onshape importer e2e verification');
  console.log(`  repo     ${REPO}`);
  console.log(`  binary   ${BIN}`);
  console.log(`  raw      ${RAW || '(not given: set CADRS_E2E_RAW)'}`);
  console.log(`  STEP     ${STEP || '(not given: set CADRS_E2E_STEP)'}`);
  console.log(`  node     ${process.version}`);
  if (JSON_CHECKS) console.log('  JSON     on');
  else console.log('  JSON     off (CADRS_E2E_JSON=0)');

  if (Number(process.versions.node.split('.')[0]) < 18) {
    console.error(`node >= 18 is needed, this is ${process.version}`);
    process.exit(1);
  }

  tempRoot = mkdtempSync(join(tmpdir(), 'cadrs-onshape-e2e-'));
  dataDir = join(tempRoot, 'data');
  rawCopy = join(tempRoot, 'raw');
  mkdirSync(dataDir, { recursive: true });

  try {
    await step('preflight', preflight);
    if (jsonMode && !/\B--json\b/.test(usageText)) {
      skip('import --json (report schema)', 'this binary has no --json (its usage does not name it)');
      jsonMode = false;
    }
    if (HAS_DOCUMENT) {
      await step('document files', documentFiles);
      // The importer writes import-report.txt beside the raw folder: never into the repo.
      await step('copy scraped raw to temp', () => {
        cpSync(RAW, rawCopy, { recursive: true });
        return `${RAW} -> ${rawCopy}`;
      });
      const passed = (name) => results.find((r) => r.name === name)?.status === 'PASS';
      if (passed('preflight') && passed('document files') && passed('copy scraped raw to temp')) {
        await step('import --dry-run', dryRunImport);
        await step('import', fullImport);
        await step('re-import (same cadrs id)', reImport);
        await step('compare vs Onshape STEP', compareStep);
      } else {
        for (const name of ['import --dry-run', 'import', 're-import (same cadrs id)', 'compare vs Onshape STEP']) {
          skip(name, 'the binary or the document is missing');
        }
      }
    } else {
      for (const name of ['document files', 'copy scraped raw to temp', 'import --dry-run', 'import', 're-import (same cadrs id)', 'compare vs Onshape STEP']) {
        skip(name, 'set CADRS_E2E_RAW and CADRS_E2E_STEP to a scraped document of your own');
      }
    }
    await step('report defect/ticket grouping', reportSelfCheck);
    await step('desktop app node tests', appTests);
    await step('packaged app resources', packagedApp);

    printTable();
    const failed = results.filter((r) => r.status === 'FAIL');
    const skipped = results.filter((r) => r.status === 'SKIP');
    const passed = results.filter((r) => r.status === 'PASS');
    if (failed.length > 0) {
      console.log(`\nFAIL: ${failed.length} failed, ${passed.length} passed, ${skipped.length} skipped`);
      console.log(`temp dir kept for inspection: ${tempRoot}`);
      process.exitCode = 1;
    } else {
      console.log(`\nPASS: ${passed.length} passed, ${skipped.length} skipped`);
      if (keep) {
        console.log(`temp dir kept: ${tempRoot}`);
      }
    }
  } finally {
    if (!keep && (!process.exitCode || process.exitCode === 0)) {
      rmSync(tempRoot, { recursive: true, force: true });
    }
  }
}

main().catch((error) => {
  console.error(error?.stack || error);
  process.exitCode = 1;
  if (tempRoot) console.error(`temp dir kept for inspection: ${tempRoot}`);
});
