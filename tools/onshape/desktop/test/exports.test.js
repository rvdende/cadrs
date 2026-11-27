// Preflight for the app's exports: the last import must have saved its documents, they must be
// in the store, and they must have parts — otherwise the app refuses with a reason instead of
// starting a long rebuild with nothing to work on.
const { test } = require('node:test');
const assert = require('node:assert/strict');

const { safeName, preflight, cadrsTarget, parseDocumentList } = require('../exports');

const report = (over = {}) => ({
  dry_run: false,
  documents: [
    { name: 'Capac2', onshape_id: '9a7ea23f008a3a2f40c80125', cadrs_id: 'cadrs-1', failed: false, counts: { parts: { matched: 1, total: 1 } } },
  ],
  ...over,
});
const store = new Set(['cadrs-1']);

test('exports.js: a saved import whose documents are in the store is exportable', () => {
  const plan = preflight(report(), store);
  assert.equal(plan.ok, true);
  assert.deepEqual(plan.documents, [{ name: 'Capac2', cadrsId: 'cadrs-1', onshapeId: '9a7ea23f008a3a2f40c80125' }]);
});

test('exports.js: a missing report is refused', () => {
  const plan = preflight(null, store);
  assert.equal(plan.ok, false);
  assert.match(plan.errors.join(' '), /No import-report\.json/);
});

test('exports.js: a dry run is refused with the "uncheck dry run" reason', () => {
  const plan = preflight(report({ dry_run: true }), store);
  assert.equal(plan.ok, false);
  assert.match(plan.errors.join(' '), /dry run/i);
});

test('exports.js: documents the store no longer has are refused by name', () => {
  const plan = preflight(report(), new Set());
  assert.equal(plan.ok, false);
  assert.match(plan.errors.join(' '), /Not in the cadrs store.*Capac2/);
});

test('exports.js: a report with no saved documents, or with no parts, is refused', () => {
  const noIds = report({ documents: [{ name: 'Capac2', failed: false }] });
  assert.equal(preflight(noIds, store).ok, false);
  assert.match(preflight(noIds, store).errors.join(' '), /saved no cadrs documents/);

  const noParts = report({ documents: [{ name: 'Empty', cadrs_id: 'cadrs-1', failed: false, counts: { parts: { matched: 0, total: 0 } } }] });
  const plan = preflight(noParts, store);
  assert.equal(plan.ok, false);
  assert.match(plan.errors.join(' '), /No parts to export.*Empty/);
  assert.deepEqual(plan.documents, []);
});

test('exports.js: failed documents are not exported, and names are made file-safe', () => {
  const mixed = report({
    documents: [
      { name: 'Bad', failed: true },
      { name: 'A/B: "C"', cadrs_id: 'cadrs-1', failed: false, counts: { parts: { total: 2 } } },
    ],
  });
  const plan = preflight(mixed, store);
  assert.equal(plan.ok, true);
  assert.deepEqual(plan.documents.map((d) => d.name), ['A/B: "C"']);
  assert.equal(safeName('A/B: "C"'), 'A-B- -C-');
  assert.equal(safeName('   '), 'document');
});

test('exports.js: .cadrs target names are de-duplicated in one run', () => {
  const used = new Set();
  assert.equal(cadrsTarget('/out', 'Capac2', used), '/out/Capac2.cadrs');
  assert.equal(cadrsTarget('/out', 'Capac2', used), '/out/Capac2 (2).cadrs');
});

test('exports: the document list is read from JSON or the plain older table', () => {
  const json = JSON.stringify({ store: '/x', documents: [{ id: 'aa262148-ca0b-8592-be7b-f93605d722d4', name: 'capac toner' }] });
  assert.deepEqual(parseDocumentList(json), [{ id: 'aa262148-ca0b-8592-be7b-f93605d722d4', name: 'capac toner' }]);
  const plain = '3cfdbf23-838f-4de1-9cf8-83bafbf83e5d  STL2DXF\naa262148-ca0b-8592-be7b-f93605d722d4  capac toner\n';
  assert.deepEqual(parseDocumentList(plain), [
    { id: '3cfdbf23-838f-4de1-9cf8-83bafbf83e5d', name: 'STL2DXF' },
    { id: 'aa262148-ca0b-8592-be7b-f93605d722d4', name: 'capac toner' },
  ]);
  assert.equal(parseDocumentList(''), null);
  assert.equal(parseDocumentList('garbage'), null);
});
