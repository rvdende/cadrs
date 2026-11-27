// Reads a Part Studio's FeatureScript (pasted or from a file): the feature timeline, the
// functions used and the workload, then lines it up with Onshape's regeneration times and
// cadrs's per-feature import times.
const FEATURE = /^(\s*)features\.(\w+)\s*=\s*function\(id\)/;
const NAME = /"Feature Name"\s*:\s*"([^"]+)"/;
const CTX = /^\s*(\w+)\(context,\s*id \+ "(\w+)",\s*\{(.*)\}\);?\s*$/;
const SK = /^\s*(sk\w+)\(sketch,/;
const PARAM = /"(\w+)"\s*:\s*((?:[A-Za-z]+\.)?[A-Za-z_0-9.]+(?:\([^()]*\))?|"[^"]*"|\[[^\]]*\]|-?[\d.]+(?:\s*\*\s*\w+)?)/g;
const SHOW = ['operationType', 'patternType', 'instanceCount', 'angle', 'depth', 'distance', 'equalSpace', 'symmetric'];

function parseFs(src) {
  const steps = [], stack = [];
  for (const line of src.split('\n')) {
    let m = line.match(FEATURE);
    if (m) {
      const depth = m[1].length;
      while (stack.length && stack[stack.length - 1].depth >= depth) stack.pop();
      const s = { id: m[2], depth, name: null, nested: stack.length > 0, calls: [], sketch: {}, params: {} };
      steps.push(s); stack.push(s);
      continue;
    }
    if (!stack.length) continue;
    const cur = stack[stack.length - 1];
    if (line === ' '.repeat(cur.depth + 4) + '};') { stack.pop(); continue; }
    if ((m = line.match(NAME)) && !cur.name) cur.name = m[1];
    if ((m = line.match(CTX))) {
      cur.calls.push(m[1]);
      for (const [, k, v] of m[3].matchAll(PARAM)) if (SHOW.includes(k)) cur.params[k] = v.replace(/^"|"$/g, '');
    } else if (/newSketch\(context/.test(line)) cur.calls.push('newSketch');
    else if ((m = line.match(SK))) {
      cur.sketch[m[1]] = (cur.sketch[m[1]] || 0) + 1;
      const t = line.match(/"constraintType"\s*:\s*ConstraintType\.(\w+)/);
      if (m[1] === 'skConstraint' && t) cur.sketch['constraint:' + t[1]] = (cur.sketch['constraint:' + t[1]] || 0) + 1;
    }
  }
  steps.forEach((s, i) => { s.n = i + 1; s.name = s.name || s.id; });
  return steps;
}

// "Extrude 2 9ms", the Regeneration times panel pasted as text, or "Extrude 2=9".
function parseRegen(text) {
  const out = {};
  for (const raw of (text || '').split('\n')) {
    const m = raw.trim().match(/^(?:\d+\s+)?(.+?)\s*(?:=|\s)\s*(\d+(?:\.\d+)?)\s*(ms|s)?$/);
    if (m && !/^total/i.test(m[1])) out[m[1].trim()] = parseFloat(m[2]) * (m[3] === 's' ? 1000 : 1);
  }
  return out;
}

// CADRS_ONSHAPE_TRACE lines: " 0.018 s  Extrude 1 (extrude) Full"
function parseTrace(text) {
  const out = {};
  for (const m of (text || '').matchAll(/^\s*(\d+\.\d+) s\s+(.+?) \((\w+)\) (Full|Partial|Skipped)\b/gm)) {
    out[m[2]] = { ms: parseFloat(m[1]) * 1000, status: m[4] };
  }
  return out;
}

const fmt = (v) => (v === undefined ? '' : v >= 100 ? v.toFixed(0) : v.toFixed(1));

function compare(src, regenText, traceText) {
  const steps = parseFs(src);
  const regen = parseRegen(regenText), trace = parseTrace(traceText);
  const L = ['## FeatureScript timeline and workload', ''];
  L.push('| # | Feature | Function | Operation | Onshape ms | cadrs ms | cadrs / Onshape | cadrs |', '|---|---|---|---|---|---|---|---|');
  let on = 0, ca = 0;
  for (const s of steps) {
    const o = regen[s.name], c = trace[s.name];
    if (o !== undefined && !s.nested) on += o;
    if (c && !s.nested) ca += c.ms;
    const ratio = o && c ? `${(c.ms / o).toFixed(1)}x` : '';
    const calls = [...new Set(s.calls)].join(', ') || '-';
    L.push(`| ${s.n} | ${s.nested ? '↳ ' : ''}${s.name} | \`${calls}\` | ${s.params.operationType?.split('.').pop() || ''} | ${fmt(o)} | ${fmt(c?.ms)} | ${ratio} | ${c ? c.status.toLowerCase() : s.nested ? 'part of parent' : trace && Object.keys(trace).length ? 'not run' : '?'} |`);
  }
  if (on || ca) L.push(`| | **Total** | | | ${fmt(on)} | ${fmt(ca)} | ${on && ca ? (ca / on).toFixed(1) + 'x' : ''} | |`);
  L.push('', '### Features');
  for (const s of steps) {
    const p = Object.entries(s.params).map(([k, v]) => `${k}=${v}`).join(', ');
    const sk = Object.entries(s.sketch).map(([k, v]) => `${k} x${v}`).join(', ');
    L.push(`- ${s.n}. **${s.name}** \`${[...new Set(s.calls)].join(', ') || '-'}\`${p ? ' — ' + p : ''}${sk ? ' — ' + sk : ''}`);
  }
  const fns = {};
  for (const s of steps) {
    for (const c of s.calls) fns[c] = (fns[c] || 0) + 1;
    for (const [k, v] of Object.entries(s.sketch)) if (!k.startsWith('constraint:')) fns[k] = (fns[k] || 0) + v;
  }
  L.push('', '### Functions used (calls)', '', Object.entries(fns).sort((a, b) => b[1] - a[1]).map(([k, v]) => `\`${k}\` x${v}`).join(', '), '');
  return { steps, markdown: L.join('\n') + '\n', onshapeMs: on, cadrsMs: ca };
}

module.exports = { parseFs, parseRegen, parseTrace, compare };
