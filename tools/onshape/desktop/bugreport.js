// Builds a GitHub-ready bug report (Markdown) from the importer's report and the scraped
// Part Studio features: what Onshape has, what cadrs could not import, and why.
const fs = require('fs');
const os = require('os');
const path = require('path');

// One report block:
//   # Document name (id)
//     [partstudio] Studio: 3 full, 1 partial, 2 skipped; parts 4/5 match Onshape
//         SKIPPED    Extrude 2 (extrude)
//                    - none of its 1 selections could be translated
//         missing    part Part 4 (2346.4 mm³)
//     [assembly] Assembly: instances 4/7 placed; mates 0/0
//     [blob] part.step (not imported)
//   (0.7 s)
function parseReport(text) {
  const docs = [];
  let doc = null, ps = null, item = null;
  for (const raw of String(text || '').split('\n')) {
    const line = raw.trim();
    let m;
    if ((m = raw.match(/^# (.+) \(([0-9a-f]+)\)$/))) {
      doc = { name: m[1], id: m[2], studios: [], other: [], blobs: [], seconds: null };
      docs.push(doc); ps = item = null;
    } else if (!doc) continue;
    else if ((m = line.match(/^\[(partstudio|assembly)\] (.+?)(?:: (.*))?$/))) {
      ps = { kind: m[1], name: m[2], summary: m[3] || '', issues: [], mismatches: [] };
      doc.studios.push(ps); item = null;
      if (m[1] === 'assembly') {
        const inst = /instances (\d+)\/(\d+) placed/.exec(ps.summary);
        if (inst && Number(inst[1]) < Number(inst[2])) ps.mismatches.push(`only ${inst[1]}/${inst[2]} instances placed`);
        const mates = /mates (\d+)\/(\d+)/.exec(ps.summary);
        if (mates && Number(mates[1]) < Number(mates[2])) ps.mismatches.push(`only ${mates[1]}/${mates[2]} mates placed`);
      }
    } else if ((m = line.match(/^\[blob\] (.+?)(?: \(not imported\))?$/))) {
      doc.blobs.push(m[1]); ps = item = null;
    } else if ((m = line.match(/^\(([\d.]+) s\)$/))) {
      doc.seconds = Number(m[1]); ps = item = null;
    } else if (!ps) {
      if (line && !line.startsWith('!')) doc.other.push(line);
    } else if ((m = line.match(/^(SKIPPED|PARTIAL|partial|suppressed)\s+(.+?) \((\w+)\)$/))) {
      item = { status: m[1], name: m[2], fn: m[3], reasons: [], suppressed: m[1].toLowerCase() === 'suppressed' };
      ps.issues.push(item);
    } else if (line.startsWith('- ') && item) {
      item.reasons.push(line.slice(2));
    } else if (/^(volume|faces|area|bbox|center|missing|placed)\s/.test(line)) {
      ps.mismatches.push(line); item = null;
    } else if (/^box differs\b/.test(line)) {
      // Continuation of the previous result mismatch: keep it on the same row.
      if (ps.mismatches.length) ps.mismatches[ps.mismatches.length - 1] += `; ${line}`;
      else ps.mismatches.push(line);
      item = null;
    } else if (line && !line.startsWith('!')) {
      ps.mismatches.push(line); item = null;
    }
  }
  return docs;
}

// Scraped features.json per element (cached: the report and the tickets tab read the same files).
const featuresCache = new Map();
function featuresFor(rawDir, docId) {
  const dir = path.join(rawDir, docId);
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const sub of fs.readdirSync(dir)) {
    const file = path.join(dir, sub, 'features.json');
    if (!fs.existsSync(file)) continue;
    let feats;
    try {
      feats = JSON.parse(fs.readFileSync(file, 'utf8')).features || [];
    } catch { continue; }
    out.push({ sub, feats });
  }
  featuresCache.set(dir, out);
  return out;
}

function loadedFeatures(rawDir, docId) {
  const dir = path.join(rawDir, docId);
  return featuresCache.has(dir) ? featuresCache.get(dir) : featuresFor(rawDir, docId);
}

// The Onshape Part Studio's feature list, with the parameters that matter for a translation
// defect: looked up by the issue names in the report, else by the studio name via elements.json.
function mapFeatures(sub, feats) {
  return feats.map((f, i) => {
    const p = {};
    for (const q of f.parameters || []) {
      const v = q.expression ?? q.value ?? q.enumValue ?? (Array.isArray(q.queries) ? `${q.queries.length} queries` : undefined);
      if (v !== undefined && v !== '' && v !== false && q.parameterId) p[q.parameterId] = v;
    }
    const keys = ['operationType', 'patternType', 'instanceCount', 'angle', 'depth', 'distance', 'equalSpace', 'symmetric'];
    const info = keys.filter((k) => k in p).map((k) => `${k}=${p[k]}`).join(', ');
    return { n: i + 1, name: f.name, type: f.featureType, info, suppressed: !!f.suppressed, id: sub };
  });
}

const elementsCache = new Map();
function elementIdFor(rawDir, docId, studioName) {
  const file = path.join(rawDir, docId, 'elements.json');
  if (!elementsCache.has(file)) {
    let els = [];
    try { els = JSON.parse(fs.readFileSync(file, 'utf8')); } catch { els = []; }
    elementsCache.set(file, els);
  }
  const el = (elementsCache.get(file) || []).find((e) => e.name === studioName);
  return el ? el.id : null;
}

function timeline(rawDir, doc, studio) {
  if (!rawDir || !doc || !doc.id) return [];
  const wanted = new Set(((studio && studio.issues) || []).map((i) => i.name));
  const elements = loadedFeatures(rawDir, doc.id);
  if (wanted.size) {
    for (const { sub, feats } of elements) {
      if (feats.some((f) => wanted.has(f.name))) return mapFeatures(sub, feats);
    }
  }
  if (studio && studio.name) {
    const id = elementIdFor(rawDir, doc.id, studio.name);
    const element = id ? elements.find((e) => e.sub === id) : null;
    if (element) return mapFeatures(element.sub, element.feats);
  }
  return [];
}

function build(reportText, rawDir, appVersion) {
  const docs = parseReport(reportText);
  const bad = docs.map((d) => ({
    ...d,
    studios: d.studios.filter((s) => s.issues.some((i) => !i.suppressed) || s.mismatches.length),
  })).filter((d) => d.studios.length);
  const L = ['## Onshape import defects', ''];
  L.push(`- Tool: cadrs Onshape importer ${appVersion || ''}`.trimEnd());
  L.push(`- OS: ${os.type()} ${os.release()} (${os.arch()})`);
  L.push(`- Date: ${new Date().toISOString().slice(0, 10)}`, '');
  if (!bad.length) {
    L.push(docs.length ? 'No defects: every feature imported and every part matches Onshape.' : 'No import report found.');
    return { markdown: L.join('\n') + '\n', title: 'Onshape import: no defects', count: 0 };
  }
  const fns = new Set();
  let count = 0;
  for (const d of bad) {
    L.push(`### Document: ${d.name}`, '');
    for (const s of d.studios) {
      L.push(`#### ${s.kind === 'assembly' ? 'Assembly' : 'Part Studio'}: ${s.name}`, '', s.summary, '');
      const tl = timeline(rawDir, d, s);
      if (tl.length) {
        L.push('Feature timeline in Onshape:', '', '| # | Feature | Type | Key parameters | cadrs |', '|---|---|---|---|---|');
        for (const f of tl) {
          const hit = s.issues.find((i) => i.name === f.name);
          L.push(`| ${f.n} | ${f.name} | \`${f.type}\` | ${f.info || ''} | ${hit ? hit.status.toLowerCase() : 'ok'} |`);
        }
        L.push('');
      }
      for (const i of s.issues) {
        if (i.suppressed) continue;
        count++; fns.add(i.fn);
        L.push(`**${i.status}: ${i.name}** (\`${i.fn}\`)`, ...i.reasons.map((r) => `- ${r}`), '');
      }
      if (s.mismatches.length) {
        count += s.mismatches.length;
        L.push('Result differs from Onshape:', '', '```', ...s.mismatches, '```', '');
      }
    }
  }
  L.push('### Steps to reproduce', '', '1. Create the Part Studio above in Onshape (timeline and parameters listed).',
    '2. Scrape it with the cadrs Onshape importer and import it.', '3. Compare with Onshape\'s volume and face count.', '',
    '_Attach the .cadrs file and the Onshape STEP export if available._');
  const list = [...fns].slice(0, 3).join(', ');
  return { markdown: L.join('\n') + '\n', title: `Onshape import: ${list || 'result mismatch'}`, count };
}

module.exports = { build, parseReport, timeline, featuresFor };
