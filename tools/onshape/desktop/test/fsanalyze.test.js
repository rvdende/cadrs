// Unit tests for the FeatureScript analysis: timeline/workload markdown from a pasted .fs.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const { parseFs, parseRegen, parseTrace, compare } = require('../fsanalyze');

const src = fs.readFileSync(path.join(__dirname, 'fixtures', 'sample.fs'), 'utf8');

test('parseFs: steps, names, calls, nested features, parameters and sketch calls', () => {
  const steps = parseFs(src);
  assert.equal(steps.length, 3);
  assert.deepEqual(steps.map((s) => s.n), [1, 2, 3]);
  assert.deepEqual(steps.map((s) => s.name), ['Extrude 1', 'Inner 1', 'Fillet 1']);
  assert.deepEqual(steps.map((s) => s.id), ['extrude', 'inner', 'fillet']);
  assert.deepEqual(steps.map((s) => s.nested), [false, true, false]);
  assert.deepEqual(steps[0].calls, ['newSketch', 'extrude']);
  assert.deepEqual(steps[1].calls, ['chamfer']);
  assert.equal(steps[0].params.operationType, 'NewBodyOperationType.NEW');
  assert.equal(steps[0].params.depth, '10');
  assert.deepEqual(steps[0].sketch, { skPolyline: 1 });
  assert.deepEqual(parseFs(''), []);
});

test('parseRegen: milliseconds and seconds, ignores totals', () => {
  assert.deepEqual(parseRegen('Extrude 1 9ms\nFillet 1=4\n  2  Inner 1 1.5 s\nTotal 20ms'), {
    'Extrude 1': 9, 'Fillet 1': 4, 'Inner 1': 1500,
  });
  assert.deepEqual(parseRegen(''), {});
});

test('parseTrace: cadrs per-feature times and statuses', () => {
  const trace = parseTrace(' 0.018 s  Extrude 1 (extrude) Full\n 0.120 s  Fillet 1 (fillet) Partial\n nope');
  assert.deepEqual(trace['Extrude 1'], { ms: 18, status: 'Full' });
  assert.deepEqual(trace['Fillet 1'], { ms: 120, status: 'Partial' });
  assert.deepEqual(parseTrace(''), {});
});

test('compare: markdown table, totals and nested handling', () => {
  const r = compare(src, 'Extrude 1 9ms\nFillet 1=4', ' 0.018 s  Extrude 1 (extrude) Full\n 0.120 s  Fillet 1 (fillet) Partial');
  assert.equal(r.steps.length, 3);
  // Nested features are part of the parent, so they do not count towards the totals.
  assert.equal(r.onshapeMs, 13);
  assert.equal(r.cadrsMs, 138);
  assert.match(r.markdown, /## FeatureScript timeline and workload/);
  assert.match(r.markdown, /\| 1 \| Extrude 1 \| `newSketch, extrude` \| NEW \| 9\.0 \| 18\.0 \| 2\.0x \| full \|/);
  assert.match(r.markdown, /\| 2 \| ↳ Inner 1 \| `chamfer` .* part of parent \|/);
  assert.match(r.markdown, /\| 3 \| Fillet 1 \| `fillet` \| NEW \| 4\.0 \| 120 \| 30\.0x \| partial \|/);
  assert.match(r.markdown, /\*\*Total\*\*/);
  assert.match(r.markdown, /### Functions used \(calls\)/);
  assert.match(r.markdown, /`extrude` x1/);
});

test('compare: no trace data still produces the table', () => {
  const r = compare(src, '', '');
  assert.equal(r.onshapeMs, 0);
  assert.equal(r.cadrsMs, 0);
  assert.match(r.markdown, /## FeatureScript timeline and workload/);
  assert.doesNotMatch(r.markdown, /\*\*Total\*\*/);
});
