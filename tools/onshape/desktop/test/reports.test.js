// Unit tests for the report summaries: the JS ports must match summarize.py and roots.py
// byte for byte (checked directly when python3 is available).
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const reports = require('../reports');

const fixtures = path.join(__dirname, 'fixtures');
const reportFile = path.join(fixtures, 'import-report.txt');
const reportText = fs.readFileSync(reportFile, 'utf8');
const python = spawnSync('python3', ['--version'], { encoding: 'utf8' });
const hasPython = !python.error && python.status === 0;
const onshapeDir = path.resolve(__dirname, '..', '..');
const pySummarize = path.join(onshapeDir, 'summarize.py');
const pyRoots = path.join(onshapeDir, 'roots.py');

test('summarize: totals, skipped kinds and reasons', () => {
  const out = reports.summarize(reportText);
  const lines = out.split('\n');
  assert.equal(lines[0], 'documents 2 (0 failed), part studios 3 (0 fully imported with every part matching)');
  assert.equal(lines[1], 'features 10: 3 full (30 %), 2 partial, 5 skipped; 0 left out because the rebuild hung or crashed');
  assert.equal(lines[2], 'parts matching Onshape\'s volume: 1/5 (20 %)');
  assert.match(out, /\nskipped features by type:\n/);
  assert.match(out, /^ {6}3 {2}extrude$/m);
  assert.match(out, /^ {6}2 {2}chamfer$/m);
  assert.match(out, /^ {6}1 {2}draft$/m);
  assert.match(out, /skipped {2}extrude {10}none of its N selections could be translated/);
  assert.match(out, /partial {2}extrude {10}up-to face not translated: blind instead/);
  assert.equal(lines[lines.length - 1], '', 'output ends with a newline like the python script');
});

test('summarize: --top limits the listings', () => {
  const out = reports.summarize(reportText, { top: 1 });
  assert.equal(out.split('\n').filter((l) => /^ {6}\d+ {2}\w+$/.test(l)).length, 1);
  assert.equal(out.split('\n').filter((l) => /^ {6}\d+ {2}(skipped|partial) /.test(l)).length, 1);
});

test('summarize: matches python3 summarize.py', { skip: !hasPython || !fs.existsSync(pySummarize) }, () => {
  const py = spawnSync('python3', [pySummarize, reportFile], { encoding: 'utf8' });
  assert.equal(py.status, 0, py.stderr);
  assert.equal(reports.summarize(reportText), py.stdout);
});

test('roots: first failure per Part Studio, grouped and counted', () => {
  const out = reports.roots(reportText);
  const lines = out.split('\n');
  assert.equal(lines[0], '  3  extrude        none of its N selections could be translated');
  assert.equal(lines[1].trim(), 'Test Cartridge (aaaaaaaaaaaaaaaaaaaaaaaa) / holder / Extrude 2');
  assert.equal(lines[2].trim(), 'Test Cartridge (aaaaaaaaaaaaaaaaaaaaaaaa) / Mount / Extrude 2');
  assert.equal(lines[3].trim(), 'Other (bbbbbbbbbbbbbbbbbbbbbbbb) / Plate / Extrude 1');
  // Only the first SKIPPED feature of each studio is a "root": the later thicken is not listed.
  assert.doesNotMatch(out, /thicken/);
});

test('roots: --show limits the examples, empty report gives empty output', () => {
  const out = reports.roots(reportText, { show: 1 });
  assert.equal(out.trim().split('\n').length, 2); // group line + one example
  assert.equal(reports.roots(''), '');
});

test('roots: matches python3 roots.py', { skip: !hasPython || !fs.existsSync(pyRoots) }, () => {
  const py = spawnSync('python3', [pyRoots, reportFile], { encoding: 'utf8', env: { ...process.env, ROOTS_SHOW: '10' } });
  assert.equal(py.status, 0, py.stderr);
  assert.equal(reports.roots(reportText, { show: 10 }), py.stdout);
});

test('perDocument: per-document and per-studio counters for the UI', () => {
  const docs = reports.perDocument(reportText);
  assert.equal(docs.length, 2);
  assert.equal(docs[0].name, 'Test Cartridge');
  assert.equal(docs[0].id, 'aaaaaaaaaaaaaaaaaaaaaaaa');
  assert.equal(docs[0].partStudios, 2);
  assert.equal(docs[0].assemblies, 1);
  assert.equal(docs[0].featureDefects, 8);
  assert.equal(docs[0].resultMismatches, 5);
  assert.deepEqual(docs[0].blobs, ['test.step']);
  const holder = docs[0].studios[0];
  assert.equal(holder.name, 'holder');
  assert.equal(holder.full, 2);
  assert.equal(holder.partial, 2);
  assert.equal(holder.skipped, 3);
  assert.equal(holder.partsMatched, 0);
  assert.equal(holder.parts, 2);
  assert.equal(docs[1].studios[0].name, 'Plate');

  const text = reports.formatPerDocument(docs);
  assert.match(text, /Per-document import results: 2 document\(s\), 3 Part Studio\(s\), 1 assembly\(ies\)/);
  assert.match(text, /Test Cartridge {2}\(aaaaaaaaaaaaaaaaaaaaaaaa\)/);
  assert.match(text, /holder: 2 full, 2 partial, 3 skipped; parts 0\/2 match Onshape; 6 feature issue\(s\), 4 result difference\(s\)/);
  assert.match(text, /1 blob\(s\) not imported: test\.step/);
  assert.equal(reports.formatPerDocument([]), 'No import report found yet.\n');
});

test('perDocumentFromJson: the importer JSON report feeds the per-document summary', () => {
  const json = JSON.parse(fs.readFileSync(path.join(fixtures, 'import-report.json'), 'utf8'));
  const docs = reports.perDocumentFromJson(json);
  assert.equal(docs.length, 2, 'the failed document is skipped');
  assert.equal(docs[0].name, 'Test Cartridge');
  assert.equal(docs[0].id, 'aaaaaaaaaaaaaaaaaaaaaaaa');
  assert.equal(docs[0].partStudios, 1);
  assert.equal(docs[0].assemblies, 1);
  assert.equal(docs[0].featureDefects, 2);
  assert.equal(docs[0].resultMismatches, 3);
  const holder = docs[0].studios[0];
  assert.equal(holder.name, 'holder');
  assert.equal(holder.full, 2);
  assert.equal(holder.partial, 2);
  assert.equal(holder.skipped, 3);
  assert.equal(holder.partsMatched, 0);
  assert.equal(docs[0].studios[1].instances.placed, 1);
  const text = reports.formatPerDocument(docs);
  assert.match(text, /holder: 2 full, 2 partial, 3 skipped; parts 0\/2 match Onshape; 2 feature issue\(s\), 2 result difference\(s\)/);
  assert.match(text, /assembly Holder: 1\/3 instances placed; 1 result difference\(s\)/);
  assert.deepEqual(reports.perDocumentFromJson(null), []);
  assert.deepEqual(reports.perDocumentFromJson({}), []);
});

test('defaultReport follows CADRS_ONSHAPE_ROOT', () => {
  const old = process.env.CADRS_ONSHAPE_ROOT;
  process.env.CADRS_ONSHAPE_ROOT = '/tmp/example-root';
  try {
    assert.equal(reports.defaultReport(), '/tmp/example-root/import-report.txt');
  } finally {
    if (old === undefined) delete process.env.CADRS_ONSHAPE_ROOT;
    else process.env.CADRS_ONSHAPE_ROOT = old;
  }
});
