// The operation analysis: for one conversion run, what Onshape had (feature type and the
// parameters that matter) next to what the importer did with it (full/partial/skipped, the notes,
// and whether the part rebuilt). This is the report to read before opening a ticket: it names the
// operations, the numbers and the first failure, and links the Onshape document.
const fs = require('fs');
const path = require('path');

const PARAMS = [
  'operationType', 'angle', 'depth', 'distance', 'thickness1', 'thickness2', 'radius',
  'patternType', 'instanceCount', 'equalSpace', 'symmetric', 'flip', 'throughAll', 'offset',
  'density', 'wallThickness',
];

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

/** `pid = expression`, or the enum/boolean value, for the parameters worth showing. */
function keyParams(feature) {
  const found = new Map();
  for (const p of feature.parameters || []) {
    const id = p.parameterId;
    if (!id || !PARAMS.includes(id)) continue;
    let value = p.expression;
    if (value === undefined || value === '' || value === null) value = p.enumValue ?? p.value;
    if (value === undefined || value === '' || value === null || value === false) continue;
    found.set(id, value);
  }
  // A stable order (the parameter list's order in the UI), not the JSON's.
  return PARAMS.filter((id) => found.has(id)).map((id) => `${id}=${found.get(id)}`).join(', ');
}

function readFeatureList(rawDir, docId, elementId) {
  if (!docId || !elementId) return null;
  const file = path.join(rawDir, docId, elementId, 'features.json');
  return readJson(file);
}

function docLink(docId, workspaceId, elementId) {
  if (!docId) return '';
  const base = `https://cad.onshape.com/documents/${docId}`;
  if (workspaceId && elementId) return `${base}/w/${workspaceId}/e/${elementId}?renderMode=0`;
  return base;
}

function scrapedAt(rawDir, docId, elementId) {
  try {
    return fs.statSync(path.join(rawDir, docId, elementId, 'features.json')).mtime;
  } catch {
    return null;
  }
}

function fmtDate(date) {
  return date ? new Date(date).toISOString().slice(0, 19).replace('T', ' ') : 'unknown';
}

/** The markdown of `<run>/operations.md`; `{ markdown, file }`. */
function buildOperations({ runDir, rawDir, appVersion }) {
  const report = readJson(path.join(runDir, 'import-report.json'));
  const lines = ['# Operation analysis', ''];
  lines.push(`- Run: \`${runDir}\``);
  if (appVersion) lines.push(`- Importer: ${appVersion}`);
  lines.push('', 'Onshape features and what the importer made of each, with the parameters that matter (angle, depth, thickness, …), the outcome and the notes. Read this before opening a ticket: it says what to report.');
  if (!report) {
    lines.push('', 'No `import-report.json` in this run (an old or dry run?). Run a conversion with the current importer to get this analysis.');
    const md = `${lines.join('\n')}\n`;
    return { markdown: md, file: path.join(runDir, 'operations.md') };
  }

  for (const doc of report.documents || []) {
    lines.push('', `## ${doc.name || doc.onshape_id}`, '');
    if (doc.failed) {
      lines.push('- **The import failed**: the document did not load; nothing to analyse.');
      continue;
    }
    // The scraper writes elements.json/workspaces.json as plain arrays, older tools as objects.
    const listOf = (value, key) => (Array.isArray(value) ? value : (value && value[key]) || []);
    const rawElements = listOf(readJson(path.join(rawDir, doc.onshape_id || '', 'elements.json')), 'elements');
    const rawWorkspaces = listOf(readJson(path.join(rawDir, doc.onshape_id || '', 'workspaces.json')), 'workspaces');
    const workspaceId = (rawWorkspaces.find((w) => w.state === 'ACTIVE') || rawWorkspaces[0] || {}).id;
    // A document lists a Part Studio and an Assembly under the same name: keep the Part Studio.
    const byName = new Map(
      rawElements
        .filter((e) => e.elementType === 'PARTSTUDIO' || e.type === 'Part Studio')
        .map((e) => [e.name, e]),
    );
    if (doc.onshape_id) lines.push(`- Onshape document: ${docLink(doc.onshape_id, null, null)}`);
    if (doc.cadrs_id) lines.push(`- cadrs document: \`${doc.cadrs_id}\``);
    lines.push('');

    for (const element of doc.elements || []) {
      if (element.kind !== 'partstudio') continue;
      const raw = byName.get(element.name);
      const rawFeatures = raw ? readFeatureList(rawDir, doc.onshape_id, raw.id) : null;
      const feats = (rawFeatures && rawFeatures.features) || [];
      const byFeature = new Map(feats.map((f) => [f.name, f]));
      lines.push(`### ${element.name}`, '');
      if (raw) lines.push(`- Onshape link: ${docLink(doc.onshape_id, workspaceId, raw.id)}`);
      const when = scrapedAt(rawDir, doc.onshape_id, raw && raw.id);
      lines.push(`- Scraped: ${fmtDate(when)}${rawFeatures ? '' : ' (no scraped feature list found)'}`);
      const counts = element.counts || {};
      if (counts) lines.push(`- Features: ${counts.full || 0} full, ${counts.partial || 0} partial, ${counts.skipped || 0} skipped, ${counts.suppressed || 0} suppressed`);
      for (const part of element.parts || []) {
        const status = part.match ? 'matches Onshape' : part.cadrs_volume == null ? '**MISSING** (no part rebuilt)' : `**differs** (${(100 * (part.relative_error || 0)).toFixed(2)} % volume)`;
        lines.push(`- Part ${part.name}: ${part.onshape_volume?.toFixed?.(1) ?? part.onshape_volume} mm³ in Onshape, ${status}`);
      }
      lines.push('');
      lines.push('| # | Onshape feature | Type | Key parameters | cadrs result | What the importer said |');
      lines.push('|---|---|---|---|---|---|');
      (element.features || []).forEach((f, i) => {
        const rawFeature = byFeature.get(f.name) || {};
        const params = keyParams(rawFeature);
        const notes = (f.notes || []).join('; ').replace(/\|/g, '\\|');
        lines.push(`| ${i + 1} | ${f.name} | \`${f.kind}\` | ${params.replace(/\|/g, '\\|')} | ${f.outcome} | ${notes} |`);
      });
      // A partial revolve in the scrape is the first thing to check against the Onshape document.
      const partialRevolves = feats
        .filter((f) => f.featureType === 'revolve')
        .map((f) => {
          const angle = (f.parameters || []).find((x) => x.parameterId === 'angle');
          const expr = angle && (angle.expression || angle.enumValue);
          const deg = expr ? parseFloat(String(expr)) : NaN;
          return { name: f.name, expr, deg };
        })
        .filter((r) => Number.isFinite(r.deg) && r.deg < 360);
      if (partialRevolves.length) {
        lines.push(
          '',
          `- **Partial revolve in the scraped data**: ${partialRevolves.map((r) => `${r.name} = ${r.expr}`).join(', ')}. A revolve that is Full in Onshape is 360°; if Onshape shows it full, this scrape was taken before the edit — scrape the document again.`,
        );
      }
      const missingRaw = (element.features || []).filter((f) => !byFeature.has(f.name));
      if (missingRaw.length) {
        lines.push('', `The scraped feature list has no parameters for: ${missingRaw.map((f) => f.name).join(', ')} (an older scrape?).`);
      }
      lines.push('');
    }

    const defects = doc.defects || [];
    if (defects.length) {
      lines.push('', '### What to report', '');
      const groups = new Map();
      for (const d of defects) {
        const key = d.key || `${d.feature_kind}:${d.reason}`;
        if (!groups.has(key)) groups.set(key, { ...d, count: 0, examples: [] });
        const g = groups.get(key);
        g.count += 1;
        if (g.examples.length < 4) g.examples.push(`${d.feature || d.element || '?'} (${d.outcome})`);
      }
      for (const g of [...groups.values()].sort((a, b) => b.count - a.count)) {
        lines.push(`- **${g.count}× ${g.feature_kind || g.element_kind}**: ${g.reason} — e.g. ${g.examples.join('; ')}`);
      }
      lines.push('', '_Attach this file, the .cadrs export and Onshape\'s STEP export to the ticket; a screenshot of the dialog helps too._');
    } else if ((element.parts || []).every((p) => p.match)) {
      lines.push('', 'No defects: every feature imported fully and the parts match Onshape.');
    }
  }

  // A parameter most often wrong when the scrape is old: say so plainly.
  lines.push(
    '',
    '### Notes',
    '',
    '- The parameters above are from the **last scrape** (`raw/<document>/<element>/features.json`).',
    '  If a feature was changed in Onshape after that, the numbers here are stale: scrape again.',
    '- A revolve with an angle below 360° is a partial revolve; a full revolve in Onshape has 360°.',
    '  If Onshape shows a full revolve here, the scrape predates the edit.',
    '',
  );
  const markdown = `${lines.join('\n')}\n`;
  return { markdown, file: path.join(runDir, 'operations.md') };
}

module.exports = { buildOperations, keyParams, docLink };
