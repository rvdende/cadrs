// Unit and CLI tests for the GitHub ticket generator: grouping, stable dedup keys, drafts,
// INDEX.md and the offline end-to-end `node tickets.js <report>` path.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const tickets = require('../tickets');

const fixtures = path.join(__dirname, 'fixtures');
const reportFile = path.join(fixtures, 'import-report.txt');
const rawDir = path.join(fixtures, 'raw');
const reportText = fs.readFileSync(reportFile, 'utf8');
const cli = path.join(__dirname, '..', 'tickets.js');

test('dedupKey: stable prefix, normalized reason, numbers collapsed', () => {
  assert.equal(
    tickets.dedupKey('extrude', 'none of its 1 selections could be translated'),
    'onshape-import:extrude:none-of-its-n-selections-could-be-translated',
  );
  assert.equal(
    tickets.dedupKey('extrude', 'none of its 42 selections could be translated'),
    'onshape-import:extrude:none-of-its-n-selections-could-be-translated',
  );
  assert.equal(tickets.dedupKey('Boolean Bodies', '  "Plate" is missing  '), 'onshape-import:boolean bodies:is-missing');
  assert.match(tickets.dedupKey('part', 'volume differs from Onshape'), /^onshape-import:part:[a-z0-9-]+$/);
});

test('normalizeReason: numbers and quoted names are replaced, spacing collapsed', () => {
  assert.equal(tickets.normalizeReason('none of its 12 selections'), 'none of its N selections');
  assert.equal(tickets.normalizeReason('face "Part 4" not found'), 'face "…" not found');
  assert.equal(tickets.normalizeReason('  2  edge   selections '), 'N edge selections');
  assert.equal(tickets.normalizeReason(undefined), '');
});

test('parseMismatch: report result lines map to defect kinds and reasons', () => {
  assert.deepEqual(tickets.parseMismatch('missing    part Part 4 (2346.4 mm³)'), { kind: 'part', key: 'onshape-import:part:missing-part', reason: 'part missing in cadrs' });
  assert.equal(tickets.parseMismatch('volume     Plate: 42.8 % off (1 vs 2)').key, 'onshape-import:part:volume-mismatch');
  assert.equal(tickets.parseMismatch('volume     Plate: 42.8 % off (1 vs 2)').reason, 'volume differs from Onshape');
  assert.equal(tickets.parseMismatch('faces      Plate: 98 vs 59').key, 'onshape-import:part:face-count-mismatch');
  assert.equal(tickets.parseMismatch('placed     Part 1: box differs (mm): x- -9.00').key, 'onshape-import:part:bounding-box-mismatch');
  assert.equal(tickets.parseMismatch('only 1/3 instances placed').kind, 'assembly');
  assert.equal(tickets.parseMismatch('box differs (mm): x- -9.00'), null);
  assert.equal(tickets.parseMismatch('some unrelated line'), null);
});

test('collect: groups by (feature kind, normalized reason)', () => {
  const groups = tickets.collect(reportText);
  assert.equal(groups.length, 11);
  const extrude = groups.find((g) => g.key === 'onshape-import:extrude:none-of-its-n-selections-could-be-translated');
  assert.ok(extrude);
  assert.equal(extrude.reason, 'none of its 1 selections could be translated');
  assert.equal(extrude.severity, 'high');
  assert.equal(extrude.occurrences, 3);
  assert.deepEqual(extrude.statuses, { skipped: 3, partial: 0, mismatch: 0 });
  assert.equal(extrude.documents.size, 2);
  assert.equal(extrude.studios.size, 3);
  assert.equal(extrude.features.size, 3);

  const partial = groups.find((g) => g.key === 'onshape-import:extrude:up-to-face-not-translated-blind-instead');
  assert.ok(partial);
  assert.equal(partial.occurrences, 2);
  assert.deepEqual(partial.statuses, { skipped: 0, partial: 2, mismatch: 0 });

  const partVolume = groups.find((g) => g.key === 'onshape-import:part:volume-mismatch');
  assert.ok(partVolume);
  assert.equal(partVolume.statuses.mismatch, 1);

  const assembly = groups.find((g) => g.kind === 'assembly');
  assert.ok(assembly);
  assert.equal(assembly.reason, 'instances of the assembly could not be placed');

  // Suppressed features are not defects.
  assert.ok(!groups.some((g) => g.reason.includes('Sketch 2')));
  // Sorted by occurrences, descending.
  for (let i = 1; i < groups.length; i++) assert.ok(groups[i - 1].occurrences >= groups[i].occurrences);
});

test('buildTickets: one draft per group, stable keys and files, previous flag', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-'));
  try {
    const first = tickets.writeTickets(dir, reportText, { rawDir, appVersion: '0.1.0', date: '2026-01-02' });
    assert.equal(first.tickets.length, 11);
    assert.equal(first.files.length, 11);
    assert.equal(first.totals.occurrences, 15);
    assert.deepEqual(first.totals.statuses, { skipped: 7, partial: 3, mismatch: 5 });
    assert.ok(fs.existsSync(first.indexFile));
    assert.equal(fs.readdirSync(dir).length, 12, '11 drafts plus INDEX.md');
    assert.equal(new Set(first.tickets.map((t) => t.key)).size, 11, 'dedup keys are unique');
    assert.ok(first.tickets.every((t) => !t.previous));

    const index = fs.readFileSync(first.indexFile, 'utf8');
    assert.match(index, /# Onshape import tickets/);
    assert.match(index, /11 defect group\(s\), 15 occurrence\(s\)/);
    assert.match(index, /Check for an existing issue with the same title/);
    for (const t of first.tickets) {
      assert.match(index, new RegExp(t.file.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
      assert.ok(fs.existsSync(path.join(dir, t.file)), `${t.file} exists`);
      assert.equal(fs.readFileSync(path.join(dir, t.file), 'utf8'), t.markdown);
    }

    const second = tickets.writeTickets(dir, reportText, { rawDir, appVersion: '0.1.0', date: '2026-01-02' });
    assert.equal(second.files.length, 11);
    assert.ok(second.tickets.every((t) => t.previous), 'existing drafts are marked refreshed');
    assert.match(second.index, /0 new, 11 refreshed/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('ticketMarkdown: title, dedup key, affected docs, timeline, reproduce steps, search note', () => {
  const groups = tickets.collect(reportText);
  const extrude = groups.find((g) => g.key === 'onshape-import:extrude:none-of-its-n-selections-could-be-translated');
  const markdown = tickets.ticketMarkdown(extrude, { rawDir, appVersion: '0.1.0', date: '2026-01-02' }).markdown;
  assert.match(markdown, /^# Onshape import: `extrude` — none of its 1 selections could be translated/);
  assert.match(markdown, /Dedup key:.*onshape-import:extrude:none-of-its-n-selections-could-be-translated/);
  assert.match(markdown, /Suggested labels:.*`onshape-import`, `translation`, `skipped`/);
  assert.match(markdown, /Affected documents and Part Studios/);
  assert.match(markdown, /\| Test Cartridge \(`aaaaaaaaaaaaaaaaaaaaaaaa`\) \| holder, Mount \| 2 \|/);
  assert.match(markdown, /## Feature timeline in Onshape \(Test Cartridge \/ holder\)/);
  assert.match(markdown, /\| 3 \| Extrude 2 \| `extrude` .* \| skipped \|/);
  assert.match(markdown, /## Steps to reproduce/);
  assert.match(markdown, /1\. In Onshape, open the document "Test Cartridge"/);
  assert.match(markdown, /4\. Compare the imported document with Onshape/);
  assert.match(markdown, /Check for an existing issue first/);
  assert.match(markdown, /https:\/\/github\.com\/rvdende\/cadrs\/issues\?q=is%3Aissue/);
  assert.match(markdown, /Generated by cadrs Onshape importer 0\.1\.0 on 2026-01-02/);
});

test('ticketMarkdown: without raw data there is no timeline but the draft still renders', () => {
  const groups = tickets.collect(reportText);
  const markdown = tickets.ticketMarkdown(groups[0], {}).markdown;
  assert.match(markdown, /No Onshape feature timeline for this group/);
  assert.doesNotMatch(markdown, /## Feature timeline in Onshape/);
});

test('ticketMarkdown: part defects mention geometry, not features', () => {
  const groups = tickets.collect(reportText);
  const part = groups.find((g) => g.key === 'onshape-import:part:missing-part');
  const markdown = tickets.ticketMarkdown(part, { rawDir }).markdown;
  assert.match(markdown, /part geometry — part missing in cadrs/);
  assert.match(markdown, /An imported part does not match Onshape: \*\*part missing in cadrs\*\*/);
  assert.match(markdown, /a part in Test Cartridge \/ holder — mismatch/);
});

test('CLI: node tickets.js writes drafts + INDEX.md offline (end to end)', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-cli-'));
  try {
    const run = () => spawnSync(process.execPath, [cli, reportFile, '--raw', rawDir, '--out', dir, '--json'], { encoding: 'utf8' });
    const first = run();
    assert.equal(first.status, 0, first.stderr);
    const json = JSON.parse(first.stdout);
    assert.equal(json.summary.groups, 11);
    assert.equal(json.summary.occurrences, 15);
    assert.equal(json.summary.fresh, 11);
    assert.equal(json.tickets.length, 11);
    assert.ok(fs.existsSync(json.indexFile));
    for (const t of json.tickets) assert.ok(fs.existsSync(path.join(dir, t.file)), `${t.file} exists`);

    const second = run();
    assert.equal(second.status, 0, second.stderr);
    const json2 = JSON.parse(second.stdout);
    assert.equal(json2.summary.fresh, 0);
    assert.equal(json2.summary.refreshed, 11);
    assert.equal(fs.readdirSync(dir).length, 12, 're-running does not duplicate files');

    const missing = spawnSync(process.execPath, [cli, path.join(dir, 'nope.txt'), '--out', dir], { encoding: 'utf8' });
    assert.equal(missing.status, 1);
    assert.match(missing.stderr, /no import report/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('collect: reads the importer JSON report (same grouping and keys as the text parser)', () => {
  const json = JSON.parse(fs.readFileSync(path.join(fixtures, 'import-report.json'), 'utf8'));
  const groups = tickets.collect(json);
  assert.equal(groups.length, 4);
  const extrude = groups.find((g) => g.key === 'onshape-import:extrude:none-of-its-n-selections-could-be-translated');
  assert.ok(extrude);
  assert.equal(extrude.occurrences, 3);
  assert.equal(extrude.documents.size, 2);
  assert.equal(extrude.reason, 'none of its 1 selections could be translated');
  assert.equal(extrude.examples.length, 3);
  // Fixed part/assembly keys match the importer's report::ticket_key.
  const volume = groups.find((g) => g.key === 'onshape-import:part:volume-mismatch');
  assert.equal(volume.reason, 'volume differs from Onshape');
  assert.equal(volume.statuses.mismatch, 1);
  const assembly = groups.find((g) => g.key === 'onshape-import:assembly:instances-not-placed');
  assert.equal(assembly.statuses.partial, 1);
  // The failed document is skipped, not a defect.
  assert.ok(!groups.some((g) => g.documents.has('')));
});

test('writeTickets: accepts the JSON report object and writes the same draft layout', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-json-'));
  try {
    const json = JSON.parse(fs.readFileSync(path.join(fixtures, 'import-report.json'), 'utf8'));
    const r = tickets.writeTickets(dir, json, { rawDir, appVersion: '0.1.0', date: '2026-01-02' });
    assert.equal(r.tickets.length, 4);
    assert.equal(r.totals.occurrences, 6);
    assert.equal(r.documents, 2);
    assert.ok(fs.existsSync(r.indexFile));
    assert.equal(fs.readdirSync(dir).length, 5);
    const extrude = r.tickets.find((t) => t.key === 'onshape-import:extrude:none-of-its-n-selections-could-be-translated');
    assert.match(extrude.markdown, /Severity:\*\* high/);
    assert.match(extrude.markdown, /Occurrences:\*\* 3/);
    assert.match(extrude.markdown, /Feature timeline in Onshape \(Test Cartridge \/ holder\)/);
    const volume = r.tickets.find((t) => t.key === 'onshape-import:part:volume-mismatch');
    assert.match(volume.markdown, /Dedup key:\*\* `onshape-import:part:volume-mismatch`/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('CLI: JSON report is auto-detected by extension/content (end to end)', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-json-cli-'));
  try {
    const jsonFile = path.join(fixtures, 'import-report.json');
    const r = spawnSync(process.execPath, [cli, jsonFile, '--raw', rawDir, '--out', dir, '--json'], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr);
    const out = JSON.parse(r.stdout);
    assert.equal(out.summary.groups, 4);
    assert.equal(out.summary.occurrences, 6);
    assert.equal(fs.readdirSync(dir).length, 5);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('CLI: no report JSON -> uses the text report (old importer fallback)', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-root-'));
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-out-'));
  try {
    fs.writeFileSync(path.join(root, 'import-report.txt'), reportText);
    const r = spawnSync(process.execPath, [cli, '--out', out, '--json'], {
      encoding: 'utf8', env: { ...process.env, CADRS_ONSHAPE_ROOT: root },
    });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(r.stdout).summary.groups, 11);
    assert.equal(fs.readdirSync(out).length, 12);
    // The same through the GUI's ticket builder entry point (main.js uses writeTickets the same way).
    assert.equal(tickets.collect(reportText).length, 11);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test('CLI: prefers a current JSON report, falls back to text on broken JSON', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-root-'));
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-out-'));
  try {
    fs.writeFileSync(path.join(root, 'import-report.txt'), reportText);
    fs.writeFileSync(path.join(root, 'import-report.json'), fs.readFileSync(path.join(fixtures, 'import-report.json'), 'utf8'));
    const run = () => spawnSync(process.execPath, [cli, '--out', out, '--json'], {
      encoding: 'utf8', env: { ...process.env, CADRS_ONSHAPE_ROOT: root },
    });
    let r = run();
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(r.stdout).summary.groups, 4, 'the current JSON report wins');

    fs.writeFileSync(path.join(out, 'marker'), 'x');
    fs.rmSync(path.join(out, 'marker'));
    fs.writeFileSync(path.join(root, 'import-report.json'), '{broken');
    r = run();
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stderr, /warning: .*import-report\.json/);
    assert.equal(JSON.parse(r.stdout).summary.groups, 11, 'broken JSON falls back to the text report');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test('CLI: --help and the repo wrapper tickets.mjs work', () => {
  const help = spawnSync(process.execPath, [cli, '--help'], { encoding: 'utf8' });
  assert.equal(help.status, 0);
  assert.match(help.stdout, /usage: node tickets\.js/);
  const wrapper = path.resolve(__dirname, '..', '..', 'tickets.mjs');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-tickets-mjs-'));
  try {
    const r = spawnSync(process.execPath, [wrapper, reportFile, '--out', dir, '--json'], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(r.stdout).summary.groups, 11);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
