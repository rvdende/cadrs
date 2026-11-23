// Compare & report: an Onshape FeatureScript (optional), the cadrs document and Onshape's STEP
// export of the same Part Studio, side by side: the steps, and each part's geometry 1:1.
const { parseFs } = require('./fsanalyze');

const KIND = {
  newSketch: 'Sketch', extrude: 'Extrude', revolve: 'Revolve', sweep: 'Sweep', loft: 'Loft',
  fillet: 'Fillet', chamfer: 'Chamfer', shell: 'Shell', draft: 'Draft', boolean: 'Boolean',
  circularPattern: 'Pattern', linearPattern: 'Pattern', curvePattern: 'Pattern', mirror: 'Mirror',
  mateConnector: 'MateConnector', opPlane: 'Plane', cPlane: 'Plane', transform: 'Transform',
};
const fnKind = (calls) => calls.map((c) => KIND[c]).find(Boolean) || calls[0] || '?';
const pct = (a, b) => (b ? (100 * (a - b)) / b : a ? 100 : 0);
const f = (v, d = 3) => (typeof v === 'number' ? v.toFixed(d) : '');

function pair(cadrs, step) {
  const left = [...step], out = [];
  for (const p of [...cadrs].sort((a, b) => b.volume - a.volume)) {
    let best = -1;
    left.forEach((s, i) => { if (best < 0 || Math.abs(s.volume - p.volume) < Math.abs(left[best].volume - p.volume)) best = i; });
    out.push([p, best >= 0 ? left.splice(best, 1)[0] : null]);
  }
  for (const s of left) out.push([null, s]);
  return out;
}

function bboxDiff(a, b) {
  if (!a || !b) return null;
  return Math.max(...[0, 1].flatMap((i) => [0, 1, 2].map((k) => Math.abs(a[i][k] - b[i][k]))));
}

// A STEP file splits a full cylinder into two half faces; cadrs keeps one.
function kindNote(c, s) {
  const notes = [];
  for (const k of new Set([...Object.keys(c), ...Object.keys(s)])) {
    const a = c[k] || 0, b = s[k] || 0;
    if (a !== b) notes.push(`${k}: cadrs ${a}, STEP ${b}${k === 'Cylinder' && b === 2 * a ? ' (STEP seam-split halves)' : ''}`);
  }
  return notes;
}

function report({ fs, decoded, cmp }) {
  const L = ['# cadrs vs Onshape: comparison report', ''];
  const defects = [];
  L.push(`- Document: ${cmp.document}`, `- cadrs file: ${cmp.cadrs_file}`, `- STEP file: ${cmp.step_file}`, '');
  const studio = cmp.studios[0];
  if (!studio) { L.push('The cadrs file has no Part Studio.'); return { markdown: L.join('\n') + '\n', defects: ['no Part Studio'] }; }
  if (cmp.studios.length > 1) L.push(`_The file has ${cmp.studios.length} Part Studios; the first, "${studio.name}", is compared._`, '');

  L.push('## Steps', '');
  const cf = studio.features;
  if (fs) {
    const top = parseFs(fs).filter((s) => !s.nested);
    L.push('| # | Onshape feature | Onshape function | cadrs feature | cadrs kind | |', '|---|---|---|---|---|---|');
    const used = new Set();
    top.forEach((s, i) => {
      const k = fnKind(s.calls);
      const m = cf.find((x) => x.name === s.name && !used.has(x.n)) || cf[i] && !used.has(cf[i].n) && cf[i];
      if (m) used.add(m.n);
      const ok = m && m.kind === k;
      if (!ok) defects.push(m ? `Step "${s.name}": Onshape ${k}, cadrs ${m.kind}` : `Step "${s.name}" (${k}) is missing in cadrs`);
      L.push(`| ${i + 1} | ${s.name} | \`${s.calls.join(', ') || '-'}\` | ${m ? m.name : ''} | ${m ? m.kind : ''} | ${ok ? 'match' : m ? 'different kind' : 'MISSING'} |`);
    });
    for (const x of cf.filter((x) => !used.has(x.n))) {
      L.push(`| | | | ${x.name} | ${x.kind} | extra step in cadrs |`);
    }
  } else {
    L.push('| # | cadrs feature | kind |', '|---|---|---|', ...cf.map((x) => `| ${x.n} | ${x.name} | ${x.kind} |`));
  }
  L.push('');

  L.push('## Geometry (cadrs vs Onshape STEP)', '');
  L.push('| Part | Quantity | cadrs | STEP | Difference | |', '|---|---|---|---|---|---|');
  const rows = pair(studio.parts, cmp.step);
  rows.forEach(([c, s], i) => {
    const name = c ? c.name : `STEP body ${i + 1}`;
    if (!c || !s) {
      defects.push(!c ? `STEP has a body cadrs does not (${s.name})` : `cadrs part "${c.name}" has no STEP body`);
      L.push(`| ${name} | body | ${c ? 'present' : 'ABSENT'} | ${s ? 'present' : 'ABSENT'} | | MISMATCH |`);
      return;
    }
    const dv = pct(c.volume, s.volume), da = pct(c.area, s.area), db = bboxDiff(c.bbox, s.bbox);
    const okv = Math.abs(dv) < 0.01, oka = Math.abs(da) < 0.1, okb = db === null || db < 0.01;
    const notes = kindNote(c.kinds, s.kinds);
    const real = notes.filter((n) => !n.includes('seam-split'));
    if (!okv) defects.push(`Part "${name}" volume ${f(c.volume, 1)} vs Onshape ${f(s.volume, 1)} mm³ (${f(dv, 2)} %)`);
    if (!oka) defects.push(`Part "${name}" area ${f(c.area, 1)} vs Onshape ${f(s.area, 1)} mm² (${f(da, 2)} %)`);
    if (!okb) defects.push(`Part "${name}" bounding box differs by up to ${f(db)} mm`);
    for (const n of real) defects.push(`Part "${name}" faces — ${n}`);
    L.push(`| ${name} | volume mm³ | ${f(c.volume, 2)} | ${f(s.volume, 2)} | ${f(dv, 4)} % | ${okv ? 'match' : 'DIFFERENT'} |`);
    L.push(`| | area mm² | ${f(c.area, 2)} | ${f(s.area, 2)} | ${f(da, 4)} % | ${oka ? 'match' : 'DIFFERENT'} |`);
    L.push(`| | bounding box | [${c.bbox ? c.bbox[0].map((x) => f(x, 2)) : ''}] .. [${c.bbox ? c.bbox[1].map((x) => f(x, 2)) : ''}] | [${s.bbox ? s.bbox[0].map((x) => f(x, 2)) : ''}] .. [${s.bbox ? s.bbox[1].map((x) => f(x, 2)) : ''}] | ${db === null ? '' : f(db, 4) + ' mm'} | ${okb ? 'match' : 'DIFFERENT'} |`);
    L.push(`| | faces | ${c.faces} | ${s.faces} | | ${real.length ? 'DIFFERENT' : c.faces === s.faces ? 'match' : 'differs only by STEP seam splitting'} |`);
    for (const n of notes) L.push(`| | face kinds | | | ${n} | |`);
  });
  L.push('', `cadrs rebuild time: ${f(studio.rebuild_ms, 0)} ms`, '');

  if (decoded && Object.keys(decoded.queries || {}).length) {
    L.push('## Decoded FeatureScript references', '', 'What the encoded queries in the FeatureScript select:', '');
    for (const [k, q] of Object.entries(decoded.queries)) L.push(`- \`${k}\`: ${q.summary}`);
    L.push('');
  }
  if (decoded && Object.keys(decoded.initialGuess || {}).length) {
    L.push('## Solved sketch geometry (initial guess)', '', '| Sketch | Entity | Values (x, y, dx, dy, r, …) |', '|---|---|---|');
    for (const [sk, m] of Object.entries(decoded.initialGuess)) for (const [e, v] of Object.entries(m)) L.push(`| ${sk} | ${e} | ${v.join(', ')} |`);
    L.push('');
  }
  L.push('## Result', '', defects.length ? `${defects.length} defect(s):` : 'No defects: steps and geometry match Onshape.', '', ...defects.map((d) => `- ${d}`), '');
  return { markdown: L.join('\n') + '\n', defects };
}

module.exports = { report };
