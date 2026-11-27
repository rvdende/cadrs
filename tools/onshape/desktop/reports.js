// JS ports of tools/onshape/summarize.py and roots.py, so the app no longer needs python3 for
// the import report summaries. Same output text as the Python scripts (the tests compare them
// when python3 is available), plus a per-document summary for the UI.
//
//   node reports.js summarize [import-report.txt] [--top N]
//   node reports.js roots [import-report.txt] [--show N]
//
// Default report: $CADRS_ONSHAPE_ROOT/import-report.txt (or ~/work/cadrs_onshape/...).
const fs = require('fs');
const os = require('os');
const path = require('path');

const defaultRoot = () => process.env.CADRS_ONSHAPE_ROOT || path.join(os.homedir(), 'work', 'cadrs_onshape');
const defaultReport = () => path.join(defaultRoot(), 'import-report.txt');

// Python's format(x, ".0f") rounds halves to even; JS Math.round rounds them up.
function fixed0(x) {
  const floor = Math.floor(x);
  if (Math.abs(x - floor) === 0.5) return String(floor % 2 === 0 ? floor : floor + 1);
  return String(Math.round(x));
}
const pad = (s, n) => String(s).padStart(n);
const padEnd = (s, n) => String(s).padEnd(n);
const countOf = (text, needle) => text.split(needle).length - 1;

// summarize.py: totals and the most common reasons features were skipped or only partly imported.
function summarize(text, opts = {}) {
  const top = opts.top === undefined ? 30 : Number(opts.top);
  const docs = (text.match(/^# /gm) || []).length;
  const studios = [...text.matchAll(/\[partstudio\] .*?: (\d+) full, (\d+) partial, (\d+) skipped(?:; parts (\d+)\/(\d+) match)?/g)]
    .map((m) => ({
      full: Number(m[1]), partial: Number(m[2]), skipped: Number(m[3]),
      matched: m[4] === undefined ? null : Number(m[4]),
      parts: m[5] === undefined ? null : Number(m[5]),
    }));
  const full = studios.reduce((a, s) => a + s.full, 0);
  const partial = studios.reduce((a, s) => a + s.partial, 0);
  const skipped = studios.reduce((a, s) => a + s.skipped, 0);
  const matched = studios.reduce((a, s) => a + (s.matched || 0), 0);
  const parts = studios.reduce((a, s) => a + (s.parts || 0), 0);
  const clean = studios.filter((s) => s.skipped === 0 && s.parts && s.matched === s.parts).length;
  const failed = countOf(text, 'import failed or kept hanging');
  const hung = countOf(text, 'rebuild hung');
  const out = [];
  out.push(`documents ${docs} (${failed} failed), part studios ${studios.length} (${clean} fully imported with every part matching)`);
  const total = full + partial + skipped;
  if (total) {
    out.push(`features ${total}: ${full} full (${fixed0((100 * full) / total)} %), ${partial} partial, ${skipped} skipped; ${hung} left out because the rebuild hung or crashed`);
  }
  if (parts) out.push(`parts matching Onshape's volume: ${matched}/${parts} (${fixed0((100 * matched) / parts)} %)`);

  const byKind = new Map();
  const reasons = new Map();
  let current = null;
  for (const line of text.split('\n')) {
    let m = /\s+(SKIPPED|partial)\s+.*\((\w+)\)$/.exec(line);
    if (m) {
      current = [m[1].toLowerCase(), m[2]];
      if (current[0] === 'skipped') byKind.set(current[1], (byKind.get(current[1]) || 0) + 1);
      continue;
    }
    m = /\s+- (.*)/.exec(line);
    if (m && current) {
      const reason = m[1].replace(/\d+(\.\d+)?/g, 'N').replace(/"[^"]*"/g, '"…"');
      const key = `${current[0]}\u0000${current[1]}\u0000${reason}`;
      const entry = reasons.get(key) || { status: current[0], fn: current[1], reason, n: 0 };
      entry.n += 1;
      reasons.set(key, entry);
    } else if (!line.startsWith('                 ')) {
      current = null;
    }
  }
  out.push('', 'skipped features by type:');
  for (const [k, n] of [...byKind.entries()].sort((a, b) => b[1] - a[1]).slice(0, top)) {
    out.push(`  ${pad(n, 5)}  ${k}`);
  }
  out.push('', 'reasons:');
  for (const e of [...reasons.values()].sort((a, b) => b.n - a.n).slice(0, top)) {
    out.push(`  ${pad(e.n, 5)}  ${padEnd(e.status, 8)} ${padEnd(e.fn, 16)} ${e.reason}`);
  }
  return out.join('\n') + '\n';
}

// roots.py: the first feature each Part Studio could not import, grouped by reason.
function roots(text, opts = {}) {
  const show = opts.show === undefined ? Number(process.env.ROOTS_SHOW || 3) : Number(opts.show);
  const groups = new Map();
  for (const block of text.split('\n# ')) {
    const lines = block.split('\n');
    const doc = lines[0].replace(/^[# ]+/, '');
    let studio = null;
    for (let i = 0; i < lines.length; i++) {
      let m = /^\s+\[partstudio\] (.*?):/.exec(lines[i]);
      if (m) { studio = m[1]; continue; }
      m = /^\s+SKIPPED\s+(.*) \((\w+)\)$/.exec(lines[i]);
      if (m && studio) {
        const reason = (i + 1 < lines.length ? lines[i + 1].trim().replace(/^[- ]+/, '') : '').replace(/\d+/g, 'N');
        const key = `${m[2]}\u0000${reason}`;
        const entry = groups.get(key) || { kind: m[2], reason, where: [] };
        entry.where.push(`${doc} / ${studio} / ${m[1]}`);
        groups.set(key, entry);
        studio = null;
      }
    }
  }
  const out = [];
  for (const g of [...groups.values()].sort((a, b) => b.where.length - a.where.length)) {
    out.push(`${pad(g.where.length, 3)}  ${padEnd(g.kind, 14)} ${g.reason}`);
    for (const w of g.where.slice(0, show)) out.push(`       ${w}`);
  }
  return out.length ? out.join('\n') + '\n' : '';
}

// Per-document results for the UI: what each document/Part Studio got, without scraping through
// the whole report by hand. Uses the same parser as the bug report.
function perDocument(text) {
  const { parseReport } = require('./bugreport');
  return parseReport(text).map((d) => {
    const studios = d.studios.map((s) => {
      const stats = /(\d+) full, (\d+) partial, (\d+) skipped(?:; parts (\d+)\/(\d+) match)?/.exec(s.summary || '') || [];
      const assembly = /instances (\d+)\/(\d+) placed/.exec(s.summary || '');
      return {
        kind: s.kind,
        name: s.name,
        summary: s.summary,
        full: stats[1] === undefined ? 0 : Number(stats[1]),
        partial: stats[2] === undefined ? 0 : Number(stats[2]),
        skipped: stats[3] === undefined ? 0 : Number(stats[3]),
        partsMatched: stats[4] === undefined ? null : Number(stats[4]),
        parts: stats[5] === undefined ? null : Number(stats[5]),
        instances: assembly ? { placed: Number(assembly[1]), total: Number(assembly[2]) } : null,
        featureDefects: s.issues.filter((i) => i.status.toLowerCase() !== 'suppressed').length,
        resultMismatches: s.mismatches.filter((m) => !/^box differs/.test(m)).length,
      };
    });
    return {
      name: d.name,
      id: d.id,
      studios,
      partStudios: studios.filter((s) => s.kind !== 'assembly').length,
      assemblies: studios.filter((s) => s.kind === 'assembly').length,
      featureDefects: studios.reduce((a, s) => a + s.featureDefects, 0),
      resultMismatches: studios.reduce((a, s) => a + s.resultMismatches, 0),
      blobs: d.blobs || [],
    };
  });
}

// The importer's JSON report (`cadrs-onshape import --json <file>`, aggregate schema 1) has the
// same information in machine-readable form; used when the app finds `import-report.json` next
// to the text report. Falls back to perDocument(text) if it is missing or malformed.
function perDocumentFromJson(aggregate) {
  if (!aggregate || !Array.isArray(aggregate.documents)) return [];
  return aggregate.documents.filter((d) => d && !d.failed).map((d) => {
    const elements = Array.isArray(d.elements) ? d.elements : [];
    const defects = Array.isArray(d.defects) ? d.defects : [];
    const studios = elements.map((e) => {
      // ElementReport's counts are flat (features only); the older documents-level counts are
      // nested under "features".
      const features = (e.counts && (e.counts.features || e.counts)) || {};
      const parts = Array.isArray(e.parts) ? e.parts : [];
      const assembly = e.assembly || null;
      return {
        kind: e.kind === 'assembly' ? 'assembly' : 'partstudio',
        name: e.name || '',
        summary: `${features.full || 0} full, ${features.partial || 0} partial, ${features.skipped || 0} skipped`,
        full: features.full || 0,
        partial: features.partial || 0,
        skipped: features.skipped || 0,
        partsMatched: typeof e.parts_matched === 'number' ? e.parts_matched : null,
        parts: parts.length || null,
        instances: assembly && assembly.instances ? { placed: assembly.instances.placed || 0, total: assembly.instances.total || 0 } : null,
        featureDefects: defects.filter((x) => x.element === e.name && x.feature_kind !== 'part' && x.feature_kind !== 'assembly').length,
        resultMismatches: defects.filter((x) => x.element === e.name && (x.feature_kind === 'part' || x.feature_kind === 'assembly')).length,
      };
    });
    return {
      name: d.name || '',
      id: d.onshape_id || '',
      studios,
      partStudios: studios.filter((s) => s.kind !== 'assembly').length,
      assemblies: studios.filter((s) => s.kind === 'assembly').length,
      featureDefects: defects.filter((x) => x.feature_kind !== 'part' && x.feature_kind !== 'assembly').length,
      resultMismatches: defects.filter((x) => x.feature_kind === 'part' || x.feature_kind === 'assembly').length,
      blobs: [],
      failed: !!d.failed,
    };
  });
}

function formatPerDocument(docs) {
  if (!docs.length) return 'No import report found yet.\n';
  const partStudios = docs.reduce((a, d) => a + d.partStudios, 0);
  const assemblies = docs.reduce((a, d) => a + d.assemblies, 0);
  const out = [`Per-document import results: ${docs.length} document(s), ${partStudios} Part Studio(s), ${assemblies} assembly(ies).`, ''];
  for (const d of docs) {
    out.push(`${d.name}  (${d.id})`);
    if (!d.studios.length) out.push('  (no Part Studios or assemblies in the report)');
    for (const s of d.studios) {
      if (s.kind === 'assembly') {
        out.push(`  assembly ${s.name}: ${s.instances ? `${s.instances.placed}/${s.instances.total} instances placed` : s.summary}${s.resultMismatches ? `; ${s.resultMismatches} result difference(s)` : ''}`);
        continue;
      }
      const parts = s.parts === null ? '' : `; parts ${s.partsMatched}/${s.parts} match Onshape`;
      const defects = [];
      if (s.featureDefects) defects.push(`${s.featureDefects} feature issue(s)`);
      if (s.resultMismatches) defects.push(`${s.resultMismatches} result difference(s)`);
      out.push(`  ${s.name}: ${s.full} full, ${s.partial} partial, ${s.skipped} skipped${parts}; ${defects.length ? defects.join(', ') : 'no feature or result defects'}`);
    }
    if (d.blobs.length) out.push(`  ${d.blobs.length} blob(s) not imported: ${d.blobs.slice(0, 5).join(', ')}${d.blobs.length > 5 ? ', …' : ''}`);
    out.push('');
  }
  return out.join('\n');
}

function main(argv) {
  const cmd = argv[0] || 'summarize';
  if (cmd === '-h' || cmd === '--help') {
    console.log('usage: node reports.js summarize [import-report.txt] [--top N]');
    console.log('       node reports.js roots [import-report.txt] [--show N]');
    return 0;
  }
  const args = argv.slice(1);
  let file = null, top = 30, show = Number(process.env.ROOTS_SHOW || 3);
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--top') top = Number(args[++i]);
    else if (args[i] === '--show') show = Number(args[++i]);
    else if (!args[i].startsWith('--')) file = args[i];
  }
  file = file || defaultReport();
  if (!fs.existsSync(file)) {
    console.error(`no import report: ${file}`);
    return 1;
  }
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (e) {
    console.error(`cannot read ${file}: ${e.message}`);
    return 1;
  }
  process.stdout.write(cmd === 'roots' ? roots(text, { show }) : summarize(text, { top }));
  return 0;
}

module.exports = { summarize, roots, perDocument, perDocumentFromJson, formatPerDocument, defaultReport, defaultRoot, main };

if (require.main === module) process.exitCode = main(process.argv.slice(2));
