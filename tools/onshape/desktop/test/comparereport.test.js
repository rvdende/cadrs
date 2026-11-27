// Unit tests for the cadrs-vs-Onshape comparison report.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const { report } = require('../comparereport');

const src = fs.readFileSync(path.join(__dirname, 'fixtures', 'compare.fs'), 'utf8');

function makeCmp(over = {}) {
  const part = { name: 'Part 1', volume: 1000.5, area: 600.25, faces: 6, kinds: { Plane: 6 }, bbox: [[0, 0, 0], [10, 10, 10]] };
  return {
    document: 'Test Cartridge',
    cadrs_file: '/tmp/cart.cadrs',
    step_file: '/tmp/cart.step',
    studios: [{
      name: 'holder',
      rebuild_ms: 12.5,
      features: [{ n: 1, name: 'Extrude 1', kind: 'Extrude' }, { n: 2, name: 'Fillet 1', kind: 'Fillet' }],
      parts: [part],
    }],
    step: [{ ...part, ...over }],
  };
}

test('report: matching steps and geometry has no defects', () => {
  const r = report({ fs: src, decoded: null, cmp: makeCmp() });
  assert.deepEqual(r.defects, []);
  assert.match(r.markdown, /^# cadrs vs Onshape: comparison report/);
  assert.match(r.markdown, /- Document: Test Cartridge/);
  assert.match(r.markdown, /## Steps/);
  assert.match(r.markdown, /\| 1 \| Extrude 1 \| `extrude` \| Extrude 1 \| Extrude \| match \|/);
  assert.match(r.markdown, /\| 2 \| Fillet 1 \| `fillet` \| Fillet 1 \| Fillet \| match \|/);
  assert.match(r.markdown, /## Geometry \(cadrs vs Onshape STEP\)/);
  assert.match(r.markdown, /No defects: steps and geometry match Onshape\./);
  assert.match(r.markdown, /cadrs rebuild time: 13 ms/);
});

test('report: geometry differences become defects', () => {
  const r = report({ fs: src, decoded: null, cmp: makeCmp({ volume: 1100, faces: 8, kinds: { Plane: 6, Cylinder: 2 } }) });
  assert.ok(r.defects.some((d) => /Part "Part 1" volume .* vs Onshape/.test(d)));
  assert.ok(r.defects.some((d) => /Part "Part 1" faces — Cylinder: cadrs 0, STEP 2/.test(d)));
  assert.match(r.markdown, /DIFFERENT/);
  assert.match(r.markdown, /defect\(s\):/);
});

test('report: a cadrs part with no STEP body is a defect', () => {
  const cmp = makeCmp();
  cmp.studios[0].parts.push({ name: 'Extra', volume: 5, area: 5, faces: 6, kinds: {}, bbox: null });
  const r = report({ fs: '', decoded: null, cmp });
  assert.ok(r.defects.some((d) => /cadrs part "Extra" has no STEP body/.test(d)));
  assert.match(r.markdown, /\| Extra \| body \| present \| ABSENT \|/);
});

test('report: a STEP body with no cadrs part is a defect', () => {
  const cmp = makeCmp();
  cmp.step.push({ name: 'Step extra', volume: 1, area: 1, faces: 6, kinds: {}, bbox: null });
  const r = report({ fs: '', decoded: null, cmp });
  assert.ok(r.defects.some((d) => /STEP has a body cadrs does not \(Step extra\)/.test(d)));
  assert.match(r.markdown, /\| STEP body 2 \| body \| ABSENT \| present \|/);
});

test('report: missing Onshape steps are defects and the fs-less table still renders', () => {
  const cmp = makeCmp();
  cmp.studios[0].features = [{ n: 1, name: 'Extrude 1', kind: 'Sketch' }];
  const r = report({ fs: src, decoded: null, cmp });
  assert.ok(r.defects.some((d) => /Step "Extrude 1": Onshape Extrude, cadrs Sketch/.test(d)));
  assert.match(r.markdown, /different kind/);
});

test('report: decoded FeatureScript queries and solved sketches are included', () => {
  const decoded = {
    queries: { q1: { summary: 'face 12 of the first body' } },
    initialGuess: { sketch1: { line1: [1, 2, 3, 4] } },
  };
  const r = report({ fs: src, decoded, cmp: makeCmp() });
  assert.match(r.markdown, /## Decoded FeatureScript references/);
  assert.match(r.markdown, /`q1`: face 12 of the first body/);
  assert.match(r.markdown, /## Solved sketch geometry \(initial guess\)/);
  assert.match(r.markdown, /\| sketch1 \| line1 \| 1, 2, 3, 4 \|/);
});

test('report: a file with no Part Studio is a defect, not a crash', () => {
  const r = report({ fs: '', decoded: null, cmp: { document: 'x', cadrs_file: 'a', step_file: 'b', studios: [], step: [] } });
  assert.deepEqual(r.defects, ['no Part Studio']);
  assert.match(r.markdown, /The cadrs file has no Part Studio\./);
});
