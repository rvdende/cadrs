// Turns an import report into one GitHub issue draft per translation defect: features that were
// skipped or only partly imported, and results that differ from Onshape. Defects are grouped by
// (feature kind, normalized reason) so all documents that hit the same importer gap share one
// draft, with a stable key that can be used to find an existing issue.
//
//   node tickets.js [import-report.txt] [--raw <scraped raw dir>] [--out <dir>] [--json]
//
// Writes `<out>/<dedup key>.md` per group plus `<out>/INDEX.md`; the defaults are
// `$CADRS_ONSHAPE_ROOT/import-report.txt` and `$CADRS_ONSHAPE_ROOT/tickets`. Offline: the drafts
// are plain files, no network is used.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { parseReport, timeline } = require('./bugreport');

const PREFIX = 'onshape-import';
const REPO = 'https://github.com/rvdende/cadrs';
const ISSUES = `${REPO}/issues`;

const defaultRoot = () => process.env.CADRS_ONSHAPE_ROOT || path.join(os.homedir(), 'work', 'cadrs_onshape');
const defaultReport = () => path.join(defaultRoot(), 'import-report.txt');
const today = () => new Date().toISOString().slice(0, 10);

// The same normalization as the importer's `report::normalize_reason`: run-specific numbers
// and quoted names are blanked, so "none of its 1 selections" and "… 42 selections" group
// together. Also lowercases, like the Rust side, so text-parsed and JSON keys match.
//   "none of its 23 selections could be translated" -> "none of its n selections could be translated"
function normalizeReason(reason) {
  const text = String(reason == null ? '' : reason);
  let out = '';
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c >= '0' && c <= '9') {
      // Consume digits and dots ("42.8", "1.5").
      while (i + 1 < text.length && /[0-9.]/.test(text[i + 1])) i++;
      out += 'N';
    } else if (c === '"') {
      while (i + 1 < text.length && text[++i] !== '"') { /* skip to the closing quote */ }
      out += '"…"';
    } else {
      out += c.toLowerCase();
    }
  }
  return out.split(/\s+/).filter(Boolean).join(' ').replace(/[.,;]+$/, '');
}

// The same slug as the importer's `report::ticket_key` (ASCII alphanumerics, one dash between
// groups, at most 72 characters).
function slugify(value, max = 72) {
  let slug = '';
  for (const c of String(value)) {
    if (/[a-z0-9]/i.test(c)) slug += c.toLowerCase();
    else if (!slug.endsWith('-')) slug += '-';
  }
  return slug.replace(/^-+|-+$/g, '').slice(0, max).replace(/-+$/g, '');
}

// Stable across runs and documents; the same id the Rust `report::ticket_key` makes.
function dedupKey(kind, reason) {
  return `${PREFIX}:${String(kind).toLowerCase()}:${slugify(normalizeReason(reason)) || 'unknown'}`;
}

function fileName(key) {
  return key.replace(/:/g, '-') + '.md';
}

// Result lines of the report, e.g. "missing    part Part 4 (2346.4 mm³)" or
// "volume     Plate: 42.8 % off (…)" (the report appends "; box differs …" to those lines).
// The keys are the ones the importer's JSON report uses for the same gaps.
const PART_KEYS = {
  'missing-part': 'part missing in cadrs',
  'volume-mismatch': 'volume differs from Onshape',
  'face-count-mismatch': 'face count differs from Onshape',
  'bounding-box-mismatch': 'placement or bounding box differs from Onshape',
  'surface-area-mismatch': 'surface area differs from Onshape',
  'instances-not-placed': 'instances of the assembly could not be placed',
  'mates-not-imported': 'mates of the assembly could not be placed',
};

function parseMismatch(line) {
  const part = (key) => ({ kind: 'part', key: `${PREFIX}:part:${key}`, reason: PART_KEYS[key] });
  const assembly = (key) => ({ kind: 'assembly', key: `${PREFIX}:assembly:${key}`, reason: PART_KEYS[key] });
  if (/^missing\s+part\s+/.test(line)) return part('missing-part');
  if (/^volume\s+/.test(line)) return part('volume-mismatch');
  if (/^faces\s+/.test(line)) return part('face-count-mismatch');
  if (/^area\s+/.test(line)) return part('surface-area-mismatch');
  if (/^placed\s+/.test(line)) return part('bounding-box-mismatch');
  if (/^(bbox|center)\s+/.test(line)) return part('bounding-box-mismatch');
  if (/^only\s+\d+\/\d+\s+instances placed/.test(line)) return assembly('instances-not-placed');
  if (/^only\s+\d+\/\d+\s+mates placed/.test(line)) return assembly('mates-not-imported');
  return null;
}

// A key from the JSON report: the suffix after the kind tells which fixed reason it is.
function reasonForKey(key) {
  const suffix = String(key).split(':').pop();
  if (PART_KEYS[suffix]) return PART_KEYS[suffix];
  return null;
}

// Importer severity when the source does not carry one (JSON reports do).
function defaultSeverity(key, status) {
  if (/:part:missing-part$/.test(key) || /:part:volume-mismatch$/.test(key)) return 'high';
  if (/:assembly:instances-not-placed$/.test(key)) return 'high';
  if (status === 'skipped') return 'high';
  return 'medium';
}

// Every defect occurrence in the report, grouped by key. `source` is the report text or the
// importer's aggregate JSON report (--json). Pure: no files written.
function collect(source, opts = {}) {
  const groups = new Map();
  const add = (key, kind, rawReason, status, where) => {
    const reason = reasonForKey(key) || normalizeReason(rawReason) || '(no reason given)';
    let g = groups.get(key);
    if (!g) {
      g = {
        key, kind, reason: '', severity: 'medium',
        statuses: { skipped: 0, partial: 0, mismatch: 0 },
        occurrences: 0, features: new Set(), documents: new Map(), studios: new Set(),
        featureNames: new Map(), studioStatuses: new Map(), examples: [], exampleKeys: new Set(),
        rawReasons: new Map(), lastReason: '',
      };
      groups.set(key, g);
    }
    const st = status === 'skipped' || status === 'partial' ? status : 'mismatch';
    const severity = where.severity || defaultSeverity(key, st);
    if (severity === 'high') g.severity = 'high';
    g.statuses[st] += 1;
    g.occurrences += 1;
    if (where.feature) g.features.add(`${where.docId}/${where.studio}/${where.feature}`);
    g.studios.add(`${where.docId}/${where.studio}`);
    const doc = g.documents.get(where.docId) || { name: where.docName, studios: new Set(), occurrences: 0 };
    doc.studios.add(where.studio);
    doc.occurrences += 1;
    g.documents.set(where.docId, doc);
    const rawKey = String(rawReason == null ? '' : rawReason).trim();
    g.rawReasons.set(rawKey, (g.rawReasons.get(rawKey) || 0) + 1);
    if (rawKey) g.lastReason = rawKey;
    if (where.feature) {
      const sk = `${where.docId}/${where.studio}`;
      if (!g.featureNames.has(sk)) g.featureNames.set(sk, new Set());
      g.featureNames.get(sk).add(where.feature);
      if (!g.studioStatuses.has(sk)) g.studioStatuses.set(sk, new Map());
      g.studioStatuses.get(sk).set(where.feature, st);
    }
    const exKey = `${where.docId}/${where.studio}/${where.feature || ''}`;
    if (g.examples.length < 10 && !g.exampleKeys.has(exKey)) {
      g.exampleKeys.add(exKey);
      g.examples.push({ docName: where.docName, docId: where.docId, studio: where.studio, feature: where.feature || '', status: st, reason: rawKey });
    }
  };

  if (source && typeof source === 'object') {
    // aggregate JSON from `cadrs-onshape import --json <file>`: the importer already grouped the
    // defects and used the same keys.
    for (const doc of source.documents || []) {
      if (!doc || doc.failed) continue;
      const docName = doc.name || '(failing document)';
      const docId = doc.onshape_id || '';
      for (const defect of doc.defects || []) {
        const key = defect.key || dedupKey(defect.feature_kind || 'unknown', defect.reason);
        const kind = defect.feature_kind === 'part' || defect.feature_kind === 'assembly' || defect.feature_kind === 'element'
          ? defect.feature_kind
          : (String(key).split(':')[1] || defect.feature_kind || 'unknown');
        add(key, kind, defect.reason, defect.outcome, {
          docName: defect.document || docName,
          docId,
          studio: defect.element || '',
          feature: defect.feature || '',
          severity: defect.severity,
        });
      }
    }
  } else {
    for (const doc of parseReport(source || '')) {
      for (const studio of doc.studios) {
        for (const issue of studio.issues) {
          if (issue.suppressed) continue;
          const status = issue.status.toLowerCase();
          const reasons = issue.reasons.length
            ? issue.reasons
            : [status === 'partial' ? 'imported with something dropped' : 'not imported'];
          for (const reason of reasons) {
            add(dedupKey(issue.fn, reason), issue.fn, reason, status, { docName: doc.name, docId: doc.id, studio: studio.name, feature: issue.name });
          }
        }
        for (const line of studio.mismatches) {
          const mm = parseMismatch(line);
          if (!mm) continue;
          add(mm.key, mm.kind, mm.reason, 'mismatch', { docName: doc.name, docId: doc.id, studio: studio.name, feature: '' });
        }
      }
    }
  }
  // Display the original wording (the most common one; latest wins a tie), not the normalized
  // form used for the stable key and file name. Part/assembly groups have a fixed label.
  for (const g of groups.values()) {
    const fixed = reasonForKey(g.key);
    if (fixed) {
      g.reason = fixed;
      continue;
    }
    let best = '', bestN = -1;
    for (const [raw, n] of g.rawReasons) if (n >= bestN && raw) { bestN = n; best = raw; }
    g.reason = best || g.lastReason || '(no reason given)';
  }
  return [...groups.values()].sort((a, b) => b.occurrences - a.occurrences || a.key.localeCompare(b.key));
}

function statusText(statuses) {
  const parts = [];
  if (statuses.skipped) parts.push(`skipped ${statuses.skipped}`);
  if (statuses.partial) parts.push(`partial ${statuses.partial}`);
  if (statuses.mismatch) parts.push(`result mismatch ${statuses.mismatch}`);
  return parts.join(', ');
}

function statusLabel(group) {
  const active = ['skipped', 'partial', 'mismatch'].filter((s) => group.statuses[s]);
  return active.length === 1 ? active[0] : 'mixed';
}

function suggestedTitle(group) {
  const what = group.kind === 'part' ? 'part geometry' : group.kind === 'assembly' ? 'assembly' : `\`${group.kind}\``;
  const title = `Onshape import: ${what} — ${group.reason || group.key}`;
  return title.length > 150 ? title.slice(0, 147) + '…' : title;
}

function searchUrl(title) {
  const q = encodeURIComponent(`is:issue in:title "${title.replace(/"/g, '')}"`);
  return `${ISSUES}?q=${q}`;
}

// The Onshape timeline of the first affected Part Studio, when the scraped raw data is there.
function timelineFor(group, rawDir) {
  if (!rawDir) return null;
  const ex = group.examples.find((e) => e.docId && e.studio);
  if (!ex) return null;
  const names = group.featureNames.get(`${ex.docId}/${ex.studio}`);
  const features = timeline(rawDir, { id: ex.docId }, { name: ex.studio, issues: [...(names || [])].map((name) => ({ name })) });
  return features.length ? { docName: ex.docName, docId: ex.docId, studio: ex.studio, features } : null;
}

function ticketMarkdown(group, opts = {}) {
  const title = suggestedTitle(group);
  const status = statusLabel(group);
  const labels = ['onshape-import', 'translation'];
  if (status !== 'mixed') labels.push(status);
  const tl = timelineFor(group, opts.rawDir);
  const L = [];
  L.push(`# ${title}`, '');
  L.push('<!-- Generated from an import report by the cadrs Onshape importer. Search for this key before opening a new issue. -->', '');
  L.push(`- **Dedup key:** \`${group.key}\``);
  L.push(`- **Suggested title:** ${title}`);
  L.push(`- **Suggested labels:** ${labels.map((l) => `\`${l}\``).join(', ')}`);
  L.push(`- **Severity:** ${group.severity}`);
  L.push(`- **Occurrences:** ${group.occurrences} (${statusText(group.statuses)}) in ${group.documents.size} document(s), ${group.studios.size} Part Studio(s)`);
  L.push(`- **Check for an existing issue first:** [search issues with this title](${searchUrl(title)})`);
  L.push('', '## What happens', '');
  if (group.kind === 'part') {
    L.push(`An imported part does not match Onshape: **${group.reason}**. Onshape's volume/face/position data is in the report; the affected Part Studios are listed below.`);
  } else if (group.kind === 'assembly') {
    L.push(`The assembly was not fully reproduced: **${group.reason}**.`);
  } else {
    L.push(`Onshape features of type \`${group.kind}\` are not fully translated. Most common wording in the import report: **${group.reason}**.`);
  }
  L.push('', '## Affected documents and Part Studios', '', '| Document | Part Studio(s) | Occurrences |', '|---|---|---|');
  for (const [id, doc] of group.documents) {
    L.push(`| ${doc.name} (\`${id}\`) | ${[...doc.studios].join(', ')} | ${doc.occurrences} |`);
  }
  L.push('', '## Examples', '');
  for (const ex of group.examples.slice(0, 8)) {
    const where = ex.feature ? `\`${ex.feature}\` in ${ex.docName} / ${ex.studio}` : `a part in ${ex.docName} / ${ex.studio}`;
    L.push(`- ${where} — ${ex.status}${ex.reason ? `: ${ex.reason}` : ''}`);
  }
  const shown = Math.min(8, group.examples.length);
  if (group.occurrences > shown) L.push(`- …and ${group.occurrences - shown} more occurrence(s)`);
  L.push('');
  if (tl) {
    L.push(`## Feature timeline in Onshape (${tl.docName} / ${tl.studio})`, '', '| # | Feature | Type | Key parameters | cadrs |', '|---|---|---|---|---|');
    const rows = tl.features.slice(0, 40);
    const statuses = group.studioStatuses.get(`${tl.docId}/${tl.studio}`) || new Map();
    for (const f of rows) {
      L.push(`| ${f.n} | ${f.name} | \`${f.type}\` | ${f.info || ''} | ${statuses.get(f.name) || (f.suppressed ? 'suppressed' : 'ok')} |`);
    }
    if (tl.features.length > rows.length) L.push(`| | … ${tl.features.length - rows.length} more | | | |`);
    L.push('');
  } else {
    L.push('_No Onshape feature timeline for this group (no raw data for the affected studio, or nothing to anchor it); the report lines above are all there is._', '');
  }
  L.push('## Steps to reproduce', '');
  const first = group.examples[0];
  if (first) {
    L.push(`1. In Onshape, open the document "${first.docName}" and the Part Studio "${first.studio}".`);
    L.push(`2. Scrape it with the cadrs Onshape importer (or \`cadrs-onshape import ${first.docId} --raw <raw dir>\`) and import it.`);
    L.push(`3. Look at the feature${first.feature ? ` "${first.feature}"` : ' or part'} in the import report: ${first.status} — ${first.reason}.`);
  } else {
    L.push('1. Import an Onshape document with the cadrs Onshape importer.');
    L.push('2. Look at the import report for this defect.');
  }
  L.push('4. Compare the imported document with Onshape\'s STEP export (volume, face count, position).');
  L.push('', '### Notes', '');
  L.push(`- Dedup key (stable, put it in the issue title or body): \`${group.key}\``);
  L.push('- This draft was generated offline from the importer\'s report; attach the `import-report.txt` lines and, if available, the `.cadrs` file.');
  if (opts.appVersion || opts.date) L.push(`- Generated by cadrs Onshape importer ${opts.appVersion || ''} on ${opts.date || today()}`.trimEnd());
  L.push('');
  return { title, labels, markdown: L.join('\n'), key: group.key, file: fileName(group.key), status, severity: group.severity, statuses: group.statuses, occurrences: group.occurrences, documents: [...group.documents].map(([id, d]) => ({ id, name: d.name, studios: [...d.studios] })), examples: group.examples, features: group.features.size, kind: group.kind, reason: group.reason };
}

// All tickets for a report (text or parsed JSON), and the folder state for `previous`.
function buildTickets(source, opts = {}) {
  const groups = collect(source, opts);
  const tickets = groups.map((g) => ticketMarkdown(g, opts));
  const previous = tickets.map((t) => !!(opts.outDir && fs.existsSync(path.join(opts.outDir, t.file))));
  tickets.forEach((t, i) => { t.previous = previous[i]; t.n = i + 1; });
  const totals = tickets.reduce((a, t) => {
    a.occurrences += t.occurrences;
    a.statuses.skipped += t.statuses.skipped;
    a.statuses.partial += t.statuses.partial;
    a.statuses.mismatch += t.statuses.mismatch;
    return a;
  }, { occurrences: 0, statuses: { skipped: 0, partial: 0, mismatch: 0 } });
  const documents = new Set(tickets.flatMap((t) => t.documents.map((d) => d.id)));
  return { tickets, totals, documents: documents.size, appVersion: opts.appVersion || '', date: opts.date || today(), reportFile: opts.reportFile || '', rawDir: opts.rawDir || '' };
}

function indexMarkdown(result, opts = {}) {
  const { tickets, totals } = result;
  const fresh = tickets.filter((t) => !t.previous).length;
  const L = ['# Onshape import tickets', ''];
  L.push(`Generated ${result.date} by the cadrs Onshape importer${result.appVersion ? ' ' + result.appVersion : ''}.`);
  L.push('');
  L.push(`- ${tickets.length} defect group(s), ${totals.occurrences} occurrence(s): ${statusText(totals.statuses) || 'none'}`);
  L.push(`- ${result.documents} document(s) affected${result.reportFile ? `; source report: \`${result.reportFile}\`` : ''}`);
  L.push(`- Drafts: ${fresh} new, ${tickets.length - fresh} refreshed (a draft already existed).`);
  L.push('- **Check for an existing issue with the same title (or dedup key) before opening a new one.**');
  L.push('- Each file is named after its stable dedup key; re-running the generator updates the same files.');
  L.push('');
  L.push('| # | Occurrences | Status | Kind | Reason | Draft | Suggested title |');
  L.push('|---|---|---|---|---|---|---|');
  for (const t of tickets) {
    L.push(`| ${t.n} | ${t.occurrences} | ${statusText(t.statuses)} | ${t.kind} | ${t.reason} | [${t.file}](${t.file}) | ${t.title} |`);
  }
  L.push('');
  L.push('## How to open these', '');
  L.push('1. Search GitHub for the title or key above; if an issue exists, add the affected documents to it instead.');
  L.push('2. Otherwise copy a draft into a new issue (the Tickets tab has "Copy issue").');
  L.push('3. Open GitHub with the prefilled title and paste the body from the clipboard.');
  L.push('');
  return L.join('\n');
}

// Writes the drafts and INDEX.md; creates the folder if needed. Returns the same result object
// with paths and per-ticket new/updated state.
function writeTickets(outDir, source, opts = {}) {
  const dir = outDir || path.join(defaultRoot(), 'tickets');
  const reportFile = opts.reportFile || defaultReport();
  const result = buildTickets(source, { ...opts, outDir: dir, reportFile });
  const index = indexMarkdown(result, opts);
  fs.mkdirSync(dir, { recursive: true });
  const files = [];
  for (const t of result.tickets) {
    const file = path.join(dir, t.file);
    fs.writeFileSync(file, t.markdown);
    t.path = file;
    files.push(file);
  }
  const indexFile = path.join(dir, 'INDEX.md');
  fs.writeFileSync(indexFile, index);
  return { ...result, dir, index, indexFile, files };
}

function main(argv) {
  const args = [...argv];
  const flag = (name) => {
    const i = args.indexOf(name);
    if (i < 0) return null;
    const v = args[i + 1];
    args.splice(i, v === undefined ? 1 : 2);
    return v === undefined ? null : v;
  };
  if (args.includes('-h') || args.includes('--help')) {
    console.log('usage: node tickets.js [import-report.txt|import-report.json] [--raw <scraped raw dir>] [--out <dir>] [--json]');
    console.log('Writes one Markdown issue draft per defect group plus INDEX.md.');
    console.log('Reads the text report, or the importer JSON report when the path is .json / starts with {.');
    return 0;
  }
  const rawDir = flag('--raw');
  const outDir = flag('--out');
  const json = args.includes('--json');
  // Default source: the machine-readable JSON report when it is current, else the text report.
  const textDefault = defaultReport();
  const jsonDefault = path.join(defaultRoot(), 'import-report.json');
  const mtime = (file) => { try { return fs.statSync(file).mtimeMs; } catch { return null; } };
  let file = args.find((a) => !a.startsWith('--'));
  if (!file) {
    if (fs.existsSync(jsonDefault) && (!fs.existsSync(textDefault) || mtime(jsonDefault) >= mtime(textDefault))) {
      file = jsonDefault;
    } else {
      file = textDefault;
    }
  }
  if (!fs.existsSync(file)) {
    console.error(`no import report: ${file}`);
    return 1;
  }
  let source;
  try {
    const text = fs.readFileSync(file, 'utf8');
    // `cadrs-onshape import --json <file>` (or a text report): auto-detect by content/extension.
    if (/\.json$/i.test(file) || text.trimStart().startsWith('{')) {
      try {
        source = JSON.parse(text);
      } catch (e) {
        // Old/broken JSON next to a usable text report: fall back to parsing the text.
        if (fs.existsSync(textDefault) && path.resolve(textDefault) !== path.resolve(file)) {
          console.error(`warning: ${file}: ${e.message}; using ${textDefault}`);
          source = fs.readFileSync(textDefault, 'utf8');
        } else {
          throw e;
        }
      }
    } else {
      source = text;
    }
  } catch (e) {
    console.error(`cannot read ${file}: ${e.message}`);
    return 1;
  }
  const opts = { rawDir: rawDir || path.join(path.dirname(path.resolve(file)), 'raw'), reportFile: path.resolve(file), appVersion: process.env.CADRS_ONSHAPE_VERSION || '' };
  const root = outDir || path.join(defaultRoot(), 'tickets');
  const result = writeTickets(root, source, opts);
  const fresh = result.tickets.filter((t) => !t.previous).length;
  if (json) {
    console.log(JSON.stringify({
      dir: result.dir, indexFile: result.indexFile, files: result.files,
      tickets: result.tickets.map((t) => ({ key: t.key, title: t.title, occurrences: t.occurrences, file: t.file, previous: t.previous })),
      summary: { groups: result.tickets.length, occurrences: result.totals.occurrences, documents: result.documents, fresh, refreshed: result.tickets.length - fresh },
    }, null, 2));
  } else {
    console.log(`Wrote ${result.tickets.length} ticket draft(s) to ${result.dir} (${fresh} new, ${result.tickets.length - fresh} refreshed)`);
    console.log(`Occurrences: ${result.totals.occurrences} (${statusText(result.totals.statuses)}); documents: ${result.documents}`);
    console.log(`Index: ${result.indexFile}`);
    for (const t of result.tickets.slice(0, 5)) console.log(`  ${t.occurrences.toString().padStart(4)}  ${t.title}\n        ${t.file}`);
    if (result.tickets.length > 5) console.log(`  … and ${result.tickets.length - 5} more in INDEX.md`);
  }
  return 0;
}

module.exports = { collect, buildTickets, indexMarkdown, writeTickets, ticketMarkdown, dedupKey, normalizeReason, suggestedTitle, parseMismatch, fileName, main, defaultRoot, defaultReport };

if (require.main === module) process.exitCode = main(process.argv.slice(2));
