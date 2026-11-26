// What the app may export, checked before it starts a long kernel rebuild: the last import must
// have saved its documents, they must still be in the cadrs store, and they must have parts.
//
// The importer writes <root>/import-report.json; a dry run writes it too, but saves no documents
// (`dry_run: true`, documents without `cadrs_id`). The store's document list comes from
// `cadrs-onshape list --json`. Refusing here, with reasons in plain words, is better than starting
// an export that has nothing to work with (or one that hangs on a document that is gone).
const path = require('path');

/** A file-name-safe document name (for `<root>/cadrs/<name>.cadrs`). */
function safeName(name) {
  const s = String(name || 'document')
    .replace(/[\\/:*?"<>|]+/g, '-')
    .replace(/\s+/g, ' ')
    .trim();
  return s || 'document';
}

/** The export plan: `{ ok, errors, documents: [{ name, cadrsId, onshapeId }] }`. */
function preflight(report, storedIds, options = {}) {
  const errors = [];
  const documents = [];
  if (!report || typeof report !== 'object') {
    errors.push('No import-report.json yet: run a conversion first (a dry run reports only).');
    return { ok: false, errors, documents };
  }
  if (report.dry_run) {
    errors.push('The last import was a **dry run**: it only reports and saves nothing. Uncheck "dry run" and Convert again.');
  }
  const saved = (report.documents || []).filter((d) => !d.failed && d.cadrs_id);
  if (!saved.length) {
    errors.push('The last import saved no cadrs documents (nothing has a `cadrs_id`).');
  }
  if (storedIds && saved.length) {
    const missing = saved.filter((d) => !storedIds.has(String(d.cadrs_id)));
    if (missing.length) {
      errors.push(`Not in the cadrs store any more: ${missing.map((d) => d.name || d.cadrs_id).join(', ')}. Import them again.`);
    }
  }
  if (options.requireParts !== false) {
    const empty = saved.filter((d) => {
      const total = d.counts && d.counts.parts ? Number(d.counts.parts.total) : NaN;
      return Number.isFinite(total) && total === 0;
    });
    if (empty.length) {
      errors.push(`No parts to export (the import reported none): ${empty.map((d) => d.name || d.cadrs_id).join(', ')}.`);
    }
  }
  for (const doc of saved) {
    const cadrsId = String(doc.cadrs_id);
    const total = doc.counts && doc.counts.parts ? Number(doc.counts.parts.total) : NaN;
    if (Number.isFinite(total) && total === 0) continue;
    documents.push({ name: doc.name || cadrsId, cadrsId, onshapeId: doc.onshape_id || '' });
  }
  return { ok: errors.length === 0, errors, documents };
}

/** `<dir>/<safe name>.cadrs`, de-duplicated against names already used in this run. */
function cadrsTarget(dir, name, used = new Set()) {
  const base = safeName(name);
  let candidate = path.join(dir, `${base}.cadrs`);
  for (let n = 2; used.has(candidate); n += 1) candidate = path.join(dir, `${base} (${n}).cadrs`);
  used.add(candidate);
  return candidate;
}

/**
 * The documents in a `cadrs-onshape list` result: JSON (`{documents:[...]}`) when the binary
 * supports `list --json`, otherwise the plain `id  name` table of an older binary. `null` when
 * neither can be read, so the preflight skips the store check instead of refusing everything.
 */
function parseDocumentList(text) {
  try {
    const parsed = JSON.parse(String(text || ''));
    if (parsed && Array.isArray(parsed.documents)) {
      const docs = parsed.documents
        .filter((d) => d && d.id)
        .map((d) => ({ id: String(d.id), name: d.name ? String(d.name) : '' }));
      if (docs.length) return docs;
    }
  } catch {
    /* not JSON: an older list prints one line per document */
  }
  const docs = [];
  for (const line of String(text || '').split(/\r?\n/)) {
    const m = line.match(/^\s*([0-9a-fA-F][0-9a-fA-F-]{7,})\s{2,}(.+?)\s*$/);
    if (m) docs.push({ id: m[1], name: m[2] });
  }
  return docs.length ? docs : null;
}

module.exports = { safeName, preflight, cadrsTarget, parseDocumentList };
