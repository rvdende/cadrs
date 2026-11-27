// Unit tests for the import report parser and the GitHub bug report builder.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const { parseReport, build, timeline } = require('../bugreport');

const fixtures = path.join(__dirname, 'fixtures');
const reportText = fs.readFileSync(path.join(fixtures, 'import-report.txt'), 'utf8');
const rawDir = path.join(fixtures, 'raw');
const docId = 'aaaaaaaaaaaaaaaaaaaaaaaa';

test('parseReport: documents, Part Studios, assemblies, blobs and durations', () => {
  const docs = parseReport(reportText);
  assert.equal(docs.length, 2);
  assert.equal(docs[0].name, 'Test Cartridge');
  assert.equal(docs[0].id, docId);
  assert.equal(docs[0].studios.length, 3);
  assert.deepEqual(docs[0].studios.map((s) => s.kind), ['partstudio', 'partstudio', 'assembly']);
  assert.equal(docs[0].studios[0].name, 'holder');
  assert.equal(docs[0].studios[0].summary, '2 full, 2 partial, 3 skipped; parts 0/2 match Onshape');
  assert.deepEqual(docs[0].blobs, ['test.step']);
  assert.equal(docs[0].seconds, 1.2);
  assert.equal(docs[1].seconds, 0.3);
});

test('parseReport: partial, skipped and suppressed features', () => {
  const holder = parseReport(reportText)[0].studios[0];
  const statuses = holder.issues.map((i) => `${i.status}:${i.name}:${i.fn}`);
  assert.deepEqual(statuses, [
    'partial:Extrude 1:extrude',
    'SKIPPED:Extrude 2:extrude',
    'SKIPPED:Chamfer 1:chamfer',
    'SKIPPED:Chamfer 2:chamfer',
    'partial:Fillet 1:fillet',
    'SKIPPED:Draft 1:draft',
    'suppressed:Sketch 2:newSketch',
  ]);
  assert.deepEqual(holder.issues[0].reasons, ['up-to face not translated: blind instead']);
  const suppressed = holder.issues.find((i) => i.suppressed);
  assert.equal(suppressed.suppressed, true);
  assert.deepEqual(suppressed.reasons, []);
});

test('parseReport: result mismatches, with the box continuation on the same line', () => {
  const holder = parseReport(reportText)[0].studios[0];
  assert.equal(holder.mismatches.length, 4);
  assert.match(holder.mismatches[0], /^placed {5}Part 1: box differs/);
  assert.match(holder.mismatches[1], /^volume {5}Plate: 42\.8 % off .*; box differs/);
  assert.match(holder.mismatches[2], /^faces {6}Plate: 98 vs 59/);
  assert.match(holder.mismatches[3], /^missing {4}part Part 4/);
});

test('parseReport: an assembly with unplaced instances is a mismatch', () => {
  const assembly = parseReport(reportText)[0].studios[2];
  assert.equal(assembly.kind, 'assembly');
  assert.deepEqual(assembly.mismatches, ['only 1/3 instances placed']);
});

test('parseReport: empty and unrelated text do not throw', () => {
  assert.deepEqual(parseReport(''), []);
  assert.deepEqual(parseReport('not a report\n'), []);
});

test('timeline: features anchored by issue name and by studio name', () => {
  const byIssue = timeline(rawDir, { id: docId }, { issues: [{ name: 'Extrude 2' }] });
  assert.equal(byIssue.length, 5);
  assert.deepEqual(byIssue.map((f) => f.name), ['Sketch 1', 'Extrude 1', 'Extrude 2', 'Chamfer 1', 'Draft 1']);
  assert.match(byIssue[2].info, /operationType=REMOVE/);
  assert.equal(byIssue[2].suppressed, false);

  const byStudio = timeline(rawDir, { id: docId }, { name: 'Mount', issues: [] });
  assert.equal(byStudio.length, 3);
  assert.equal(byStudio[2].name, 'Extrude 2');

  assert.deepEqual(timeline(rawDir, { id: 'bbbbbbbbbbbbbbbbbbbbbbbb' }, { name: 'Plate', issues: [] }), []);
  assert.deepEqual(timeline(null, { id: docId }, { issues: [{ name: 'Extrude 2' }] }), []);
});

test('build: Markdown bug report with timelines, without suppressed features', () => {
  const r = build(reportText, rawDir, '9.9.9');
  assert.match(r.title, /^Onshape import: extrude/);
  assert.ok(r.count >= 6);
  assert.match(r.markdown, /## Onshape import defects/);
  assert.match(r.markdown, /- Tool: cadrs Onshape importer 9\.9\.9/);
  assert.match(r.markdown, /### Document: Test Cartridge/);
  assert.match(r.markdown, /\*\*SKIPPED: Extrude 2\*\* \(`extrude`\)/);
  assert.match(r.markdown, /\*\*partial: Extrude 1\*\*/);
  assert.doesNotMatch(r.markdown, /Sketch 2/);
  assert.match(r.markdown, /Feature timeline in Onshape:/);
  assert.match(r.markdown, /\| 3 \| Extrude 2 \| `extrude` \|/);
  assert.match(r.markdown, /Result differs from Onshape:/);
  assert.match(r.markdown, /### Steps to reproduce/);
});

test('build: no defects', () => {
  const clean = '# Clean (cccccccccccccccccccccccc)\n  [partstudio] S: 1 full, 0 partial, 0 skipped; parts 1/1 match Onshape\n  (0.1 s)\n';
  const r = build(clean, null, '1.0.0');
  assert.equal(r.count, 0);
  assert.equal(r.title, 'Onshape import: no defects');
  assert.match(r.markdown, /No defects: every feature imported and every part matches Onshape\./);
  const none = build('', null, '1.0.0');
  assert.equal(none.count, 0);
  assert.match(none.markdown, /No import report found\./);
});
