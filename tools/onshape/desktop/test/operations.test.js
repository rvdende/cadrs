// The operation analysis: each Onshape feature with the parameters that matter next to what the
// importer did with it, links to the document, and what to report.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { buildOperations, keyParams, docLink } = require('../operations');

function tempDir(tag) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `cadrs-ops-${tag}-`));
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(value));
  return file;
}

const REPORT = {
  dry_run: false,
  documents: [
    {
      name: 'capac toner',
      onshape_id: '9a7ea23f008a3a2f40c80125',
      cadrs_id: 'cadrs-1',
      failed: false,
      counts: { features: { full: 1, partial: 1, skipped: 0, suppressed: 0 }, parts: { matched: 0, total: 1 } },
      elements: [
        {
          name: 'Capac1',
          kind: 'partstudio',
          counts: { full: 1, partial: 1, skipped: 0, suppressed: 0 },
          parts: [{ name: 'Part 1', onshape_volume: 41042.6, cadrs_volume: null, relative_error: null, match: false }],
          features: [
            { name: 'Extrude 1', kind: 'extrude', outcome: 'full', notes: [] },
            { name: 'Revolve 1', kind: 'revolve', outcome: 'partial', notes: ['a note'] },
          ],
        },
      ],
      defects: [
        { key: 'onshape-import:part:missing-part', severity: 'high', feature: 'Part 1', feature_kind: 'part', outcome: 'missing', reason: 'part is missing', element: 'Capac1', element_kind: 'partstudio', document: 'capac toner' },
      ],
    },
  ],
};

function fixture() {
  const root = tempDir('fixture');
  const runDir = path.join(root, 'runs', '2026-11-26_12-00-00');
  const rawDir = path.join(root, 'raw');
  fs.mkdirSync(runDir, { recursive: true });
  writeJson(path.join(runDir, 'import-report.json'), REPORT);
  const doc = '9a7ea23f008a3a2f40c80125';
  writeJson(path.join(rawDir, doc, 'elements.json'), {
    elements: [{ id: 'el-1', name: 'Capac1', elementType: 'PARTSTUDIO' }],
  });
  writeJson(path.join(rawDir, doc, 'workspaces.json'), { currentWorkspaceId: 'ws-1' });
  writeJson(path.join(rawDir, doc, 'el-1', 'features.json'), {
    features: [
      { name: 'Extrude 1', featureType: 'extrude', parameters: [{ parameterId: 'depth', expression: '12 mm' }, { parameterId: 'operationType', enumValue: 'NEW' }] },
      { name: 'Revolve 1', featureType: 'revolve', parameters: [{ parameterId: 'angle', expression: '30 deg' }, { parameterId: 'operationType', enumValue: 'ADD' }] },
    ],
  });
  return { root, runDir, rawDir, doc };
}

test('operations.js: parameters, links and the table', () => {
  assert.equal(keyParams({ parameters: [{ parameterId: 'depth', expression: '12 mm' }, { parameterId: 'symmetric', value: false }, { parameterId: 'operationType', enumValue: 'NEW' }] }), 'operationType=NEW, depth=12 mm');
  assert.equal(docLink('abc', 'ws', 'el'), 'https://cad.onshape.com/documents/abc/w/ws/e/el?renderMode=0');
  const { runDir, rawDir } = fixture();
  const { markdown, file } = buildOperations({ runDir, rawDir, appVersion: '0.1.8' });
  assert.equal(file, path.join(runDir, 'operations.md'));
  assert.match(markdown, /# Operation analysis/);
  assert.match(markdown, /capac toner/);
  assert.match(markdown, /https:\/\/cad\.onshape\.com\/documents\/9a7ea23f008a3a2f40c80125/);
  assert.match(markdown, /### Capac1/);
  assert.match(markdown, /Extrude 1.*12 mm.*full/);
  assert.match(markdown, /Revolve 1.*30 deg.*partial.*a note/);
  assert.match(markdown, /Part 1: 41042\.6 mm³ in Onshape, \*\*MISSING\*\*/);
  assert.match(markdown, /### What to report/);
  assert.match(markdown, /part is missing/);
  assert.match(markdown, /full revolve in Onshape has 360°/);
});

test('operations.js: a missing report says so instead of throwing', () => {
  const root = tempDir('missing');
  const runDir = path.join(root, 'run');
  fs.mkdirSync(runDir, { recursive: true });
  const { markdown, file } = buildOperations({ runDir, rawDir: path.join(root, 'raw') });
  assert.match(markdown, /No `import-report\.json`/);
  assert.equal(file, path.join(runDir, 'operations.md'));
});
