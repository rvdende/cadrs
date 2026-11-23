// Reads the last import report from the data folder: the text report the importer has always
// written, plus the aggregate JSON report newer importer builds write with `--json <file>`.
//
// The JSON report is machine-readable (defect keys, severities, examples), so the app uses it
// when it is there; the text report stays the fallback for old reports and old importer builds.
// Everything here is plain fs so it can be unit-tested without Electron.
const fs = require('fs');
const path = require('path');

function statTime(file) {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return null;
  }
}

// { root, textFile, jsonFile, text, aggregate, jsonError, warnings }
// - `aggregate` is null when there is no (usable) JSON report, so callers parse `text`.
// - a JSON report older than the text report is ignored: it belongs to an earlier run (for
//   example, the packaged binary was rebuilt without --json support in between).
function readReportFiles(root) {
  const textFile = path.join(root, 'import-report.txt');
  const jsonFile = path.join(root, 'import-report.json');
  let text = '';
  try {
    text = fs.existsSync(textFile) ? fs.readFileSync(textFile, 'utf8') : '';
  } catch {
    text = '';
  }
  let aggregate = null;
  let jsonError = null;
  const warnings = [];
  if (fs.existsSync(jsonFile)) {
    try {
      aggregate = JSON.parse(fs.readFileSync(jsonFile, 'utf8'));
    } catch (e) {
      jsonError = `cannot read ${jsonFile}: ${e.message}`;
    }
    if (aggregate !== null) {
      if (!aggregate || typeof aggregate !== 'object' || !Array.isArray(aggregate.documents)) {
        jsonError = `${jsonFile} is not an aggregate import report (no documents array)`;
        aggregate = null;
      } else if (fs.existsSync(textFile)) {
        const jsonTime = statTime(jsonFile);
        const textTime = statTime(textFile);
        if (jsonTime !== null && textTime !== null && jsonTime < textTime) {
          warnings.push('ignored a stale import-report.json (older than import-report.txt)');
          aggregate = null;
        }
      }
    }
  }
  return { root, textFile, jsonFile, text, aggregate, jsonError, warnings };
}

module.exports = { readReportFiles };
