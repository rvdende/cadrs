// General cadrs issue drafts: bugs and non-conformities that have nothing to do with an Onshape
// import. A draft keeps the two things GitHub wants apart anyway, as separate streams:
//
//   <root>/issues/<date>-<slug>/issue.md        the body to paste into the issue
//   <root>/issues/<date>-<slug>/attachments/    the files to drag into it (GitHub cannot attach
//                                               files from a URL, so they stay separate)
//   <root>/issues/<date>-<slug>/issue.json      title, labels, body, attachment names, URL
//   <root>/issues/INDEX.md                      every draft, newest first
//
// CLI:  node issues.js --title "…" --body-file body.md [--attach file]… [--root <dir>] [--labels a,b]
//       node issues.js --title "…" --body "…" --attach screenshot.png --json
const { spawnSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const GITHUB = 'https://github.com/rvdende/cadrs/issues/new';

/** Where drafts live: `$CADRS_ONSHAPE_ROOT` (the importer's data folder) or ~/work/cadrs_onshape. */
function defaultRoot(env = process.env) {
  return env.CADRS_ONSHAPE_ROOT || path.join(os.homedir(), 'work', 'cadrs_onshape');
}

function issuesDir(root) {
  return path.join(root, 'issues');
}

/** A folder-safe, readable version of the title (no dates or ids: those go in front). */
function slugify(title) {
  const slug = String(title || '')
    .normalize('NFKD')
    .replace(/[^\x00-\x7F]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60)
    .replace(/-+$/g, '');
  return slug || 'issue';
}

/** `2026-11-26-073012`: sortable and unique enough for drafts made in the same minute. */
function stamp(date = new Date()) {
  return date.toISOString().slice(0, 19).replace(/[:T]/g, '-');
}

function uniqueName(name, used) {
  const clean = String(name).replace(/[\\/:*?"<>|]/g, '-') || 'attachment';
  const ext = path.extname(clean);
  const base = ext ? clean.slice(0, -ext.length) : clean;
  let candidate = clean;
  for (let n = 2; used.has(candidate); n += 1) candidate = `${base}-${n}${ext}`;
  used.add(candidate);
  return candidate;
}

/** The new-issue page, with the title prefilled. GitHub takes the body from the clipboard: it
 *  cannot be prefilled from a URL of a sensible length, and attachments cannot be prefilled at
 *  all, which is why they are a separate stream. */
function githubUrl(title) {
  return `${GITHUB}?title=${encodeURIComponent(String(title || 'cadrs issue'))}`;
}

/** The body as it is written to `issue.md`: the user's text, then the attachment list (the files
 *  themselves stay in `attachments/`). */
function renderBody(body, attachments, archive) {
  const parts = [String(body || '').trimEnd()];
  if (attachments.length) {
    parts.push(
      '',
      '### Attachments',
      '',
      'Saved next to this draft; drag them into the issue (GitHub cannot attach files from a URL):',
      '',
      ...attachments.map((a) => `- \`${a.name}\` (${a.bytes} bytes)`),
    );
    if (archive) {
      parts.push(
        '',
        `GitHub accepts images, PDFs and ZIPs: \`${archive.name}\` contains every file above and can be dropped in as one attachment.`,
      );
    }
  }
  return parts.join('\n').replace(/\n+$/, '') + '\n';
}

/** Adds `row` to `<issues dir>/INDEX.md`, newest first, creating the file if needed. */
function updateIndex(base, row) {
  const file = path.join(base, 'INDEX.md');
  const line = `| ${row.created.slice(0, 10)} | ${row.title.replace(/\|/g, '\\|')} | ${row.attachments.length || ''} | [${path.basename(row.dir)}](${path.basename(row.dir)}/issue.md) | ${row.labels.join(', ')} |`;
  let text = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
  if (!text.trim()) {
    text = '# cadrs issue drafts\n\nOne folder per issue: `issue.md` is the text, `attachments/` the files to attach, `issue.json` the metadata.\n\n| Date | Title | Attachments | Draft | Labels |\n|---|---|---|---|---|\n';
  }
  const headerEnd = text.indexOf('\n', text.indexOf('|---'));
  const header = text.slice(0, headerEnd + 1);
  const lines = text
    .slice(headerEnd + 1)
    .split('\n')
    .filter((l) => l.trim().startsWith('|'));
  lines.unshift(line);
  lines.sort((a, b) => (a < b ? 1 : a > b ? -1 : 0)); // newest first (the date leads the row)
  fs.writeFileSync(file, `${header}${lines.join('\n')}\n`);
  return file;
}

/** Writes one draft. Returns the paths and the details the UI shows. */
function createDraft(options = {}) {
  const title = String(options.title || '').trim();
  const link = String(options.link || '').trim();
  const body = String(options.body || '');
  const files = options.files || [];
  const labels = (options.labels || []).map(String).filter(Boolean);
  const root = options.root || defaultRoot(options.env);
  const date = options.date || new Date();
  if (!title) throw new Error('a title is needed');
  if (!body.trim()) throw new Error('a description is needed');

  // Check everything before a directory is created, so a bad attachment leaves no half draft.
  const sources = files.map((file) => {
    const source = path.resolve(String(file));
    const stat = fs.statSync(source);
    if (!stat.isFile()) throw new Error(`not a file: ${source}`);
    return { source, bytes: stat.size };
  });

  const base = issuesDir(root);
  fs.mkdirSync(base, { recursive: true });
  const slug = slugify(title);
  const stem = `${stamp(date)}-${slug}`;
  let dir = path.join(base, stem);
  for (let n = 2; fs.existsSync(dir); n += 1) dir = path.join(base, `${stem}-${n}`);
  fs.mkdirSync(dir, { recursive: true });

  const attachments = [];
  const used = new Set();
  for (const { source, bytes } of sources) {
    const name = uniqueName(path.basename(source), used);
    const destDir = path.join(dir, 'attachments');
    fs.mkdirSync(destDir, { recursive: true });
    const dest = path.join(destDir, name);
    fs.copyFileSync(source, dest);
    attachments.push({ name, bytes, path: dest });
  }

  let archive = null;
  if (attachments.length) {
    const zipFile = path.join(dir, 'attachments.zip');
    const zipped = spawnSync('zip', ['-q', '-j', zipFile, ...attachments.map((a) => a.path)], { encoding: 'utf8' });
    if (!zipped.error && zipped.status === 0) {
      archive = { name: 'attachments.zip', bytes: fs.statSync(zipFile).size, path: zipFile };
    }
  }
  const text = renderBody(link ? `${body}\n\n### Link\n- ${link}` : body, attachments, archive);
  const bodyFile = path.join(dir, 'issue.md');
  fs.writeFileSync(bodyFile, text);
  const url = githubUrl(title);
  const issue = {
    title,
    labels,
    link,
    created: date.toISOString(),
    url,
    body: text,
    attachments: attachments.map((a) => ({ name: a.name, bytes: a.bytes })),
    archive: archive ? { name: archive.name, bytes: archive.bytes } : null,
  };
  const issueFile = path.join(dir, 'issue.json');
  fs.writeFileSync(issueFile, `${JSON.stringify(issue, null, 2)}\n`);
  const index = updateIndex(base, { ...issue, dir, attachments });
  return {
    title,
    labels,
    dir,
    bodyFile,
    issueFile,
    index,
    url,
    body: text,
    attachmentsDir: attachments.length ? path.join(dir, 'attachments') : null,
    attachments,
    archive,
  };
}

function parseArgs(argv) {
  const out = { files: [], labels: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const value = () => {
      i += 1;
      if (i >= argv.length) throw new Error(`${arg} needs a value`);
      return argv[i];
    };
    if (arg === '--title') out.title = value();
    else if (arg === '--body') out.body = value();
    else if (arg === '--body-file') out.bodyFile = value();
    else if (arg === '--body-stdin') out.bodyStdin = true;
    else if (arg === '--attach') out.files.push(value());
    else if (arg === '--root') out.root = value();
    else if (arg === '--labels') out.labels = value().split(',').map((l) => l.trim()).filter(Boolean);
    else if (arg === '--link') out.link = value();
    else if (arg === '--json') out.json = true;
    else if (arg === '--help' || arg === '-h') out.help = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  return out;
}

function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (e) {
    console.error(e.message);
    return 2;
  }
  if (args.help) {
    console.log(
      'usage: node issues.js --title "…" (--body "…" | --body-file file | --body-stdin)\n' +
        '                     [--attach file]… [--root <importer data dir>] [--labels a,b]\n' +
        'Creates <root>/issues/<date>-<slug>/{issue.md,issue.json,attachments/} and prints it as JSON.',
    );
    return 0;
  }
  try {
    let body = args.body;
    if (args.bodyFile) body = fs.readFileSync(args.bodyFile, 'utf8');
    if (args.bodyStdin) body = `${body || ''}${fs.readFileSync(0, 'utf8')}`;
    const draft = createDraft({
      title: args.title,
      body,
      files: args.files,
      labels: args.labels,
      link: args.link,
      root: args.root,
    });
    if (args.json) console.log(JSON.stringify(draft, null, 2));
    else {
      console.log(`draft: ${draft.dir}`);
      console.log(`text:  ${draft.bodyFile}`);
      if (draft.attachmentsDir) console.log(`files: ${draft.attachmentsDir}`);
      console.log(`open:  ${draft.url}`);
    }
    return 0;
  } catch (e) {
    console.error(e.message);
    return 1;
  }
}

module.exports = {
  GITHUB,
  defaultRoot,
  issuesDir,
  slugify,
  stamp,
  githubUrl,
  renderBody,
  createDraft,
  main,
};

if (require.main === module) process.exitCode = main();
