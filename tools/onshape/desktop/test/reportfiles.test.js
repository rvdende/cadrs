// Tests for picking the import report source: the machine-readable JSON report when it is
// current, the text report as fallback (old importer builds, stale or broken JSON).
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { readReportFiles } = require('../reportfiles');

const fixtures = path.join(__dirname, 'fixtures');
const textReport = fs.readFileSync(path.join(fixtures, 'import-report.txt'), 'utf8');
const jsonReport = fs.readFileSync(path.join(fixtures, 'import-report.json'), 'utf8');

function tempRoot(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cadrs-reportfiles-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}
function writeAt(file, text, mtimeMs) {
  fs.writeFileSync(file, text);
  if (mtimeMs !== undefined) fs.utimesSync(file, mtimeMs / 1000, mtimeMs / 1000);
}

test('readReportFiles: text report only (old importer) falls back cleanly', (t) => {
  const root = tempRoot(t);
  writeAt(path.join(root, 'import-report.txt'), textReport);
  const r = readReportFiles(root);
  assert.equal(r.text, textReport);
  assert.equal(r.aggregate, null);
  assert.equal(r.jsonError, null);
  assert.deepEqual(r.warnings, []);
});

test('readReportFiles: uses the JSON report when it is newer than the text report', (t) => {
  const root = tempRoot(t);
  writeAt(path.join(root, 'import-report.txt'), textReport, 1000);
  writeAt(path.join(root, 'import-report.json'), jsonReport, 2000);
  const r = readReportFiles(root);
  assert.ok(r.aggregate);
  assert.equal(r.aggregate.documents.length, 3);
  assert.deepEqual(r.warnings, []);
});

test('readReportFiles: ignores a JSON report older than the text report (stale)', (t) => {
  const root = tempRoot(t);
  writeAt(path.join(root, 'import-report.json'), jsonReport, 1000);
  writeAt(path.join(root, 'import-report.txt'), textReport, 2000);
  const r = readReportFiles(root);
  assert.equal(r.aggregate, null);
  assert.equal(r.jsonError, null);
  assert.match(r.warnings.join('\n'), /stale import-report\.json/);
});

test('readReportFiles: falls back to text on malformed JSON', (t) => {
  const root = tempRoot(t);
  writeAt(path.join(root, 'import-report.txt'), textReport, 1000);
  writeAt(path.join(root, 'import-report.json'), '{oops', 2000);
  const r = readReportFiles(root);
  assert.equal(r.aggregate, null);
  assert.match(r.jsonError, /cannot read .*import-report\.json/);
  assert.equal(r.text, textReport);
});

test('readReportFiles: a JSON object without documents is not an aggregate report', (t) => {
  const root = tempRoot(t);
  writeAt(path.join(root, 'import-report.txt'), textReport, 1000);
  writeAt(path.join(root, 'import-report.json'), JSON.stringify({ kind: 'something else' }), 2000);
  const r = readReportFiles(root);
  assert.equal(r.aggregate, null);
  assert.match(r.jsonError, /no documents array/);
});

test('readReportFiles: JSON only (no text report) is still usable', (t) => {
  const root = tempRoot(t);
  writeAt(path.join(root, 'import-report.json'), jsonReport, 2000);
  const r = readReportFiles(root);
  assert.ok(r.aggregate);
  assert.equal(r.text, '');
});

test('readReportFiles: empty folder', (t) => {
  const root = tempRoot(t);
  const r = readReportFiles(root);
  assert.equal(r.text, '');
  assert.equal(r.aggregate, null);
  assert.equal(r.jsonError, null);
});
