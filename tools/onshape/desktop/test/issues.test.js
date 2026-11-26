// General cadrs issue drafts: the body text and the attachment files are two separate streams
// (issue.md + attachments/), the index lists them, and the GitHub URL only carries the title.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const issues = require('../issues');

function tempDir(tag) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `cadrs-issues-${tag}-`));
  return dir;
}

function writeFile(dir, name, content) {
  const file = path.join(dir, name);
  fs.writeFileSync(file, content);
  return file;
}

test('issues.js: a draft keeps the text and the attachments apart', () => {
  const root = tempDir('draft');
  const attach = writeFile(root, 'test model.cadrs', 'zip-bytes');
  const draft = issues.createDraft({
    title: 'Extrude of selected sketch regions fills the holes',
    body: '### What happened\n\nSelected 4 ring regions; the result is a full disk.\n',
    files: [attach],
    root,
    date: new Date('2026-11-26T07:30:12Z'),
  });

  assert.match(draft.dir, /2026-11-26-07-30-12-extrude-of-selected-sketch-regions-fills-the-holes$/);
  const body = fs.readFileSync(draft.bodyFile, 'utf8');
  assert.match(body, /### What happened/);
  assert.match(body, /Selected 4 ring regions/);
  // The attachment is listed but is not embedded in the body: the file itself is separate.
  assert.match(body, /### Attachments/);
  assert.match(body, /`test model\.cadrs` \(9 bytes\)/);
  assert.ok(!body.includes('zip-bytes'), 'file contents are not in the body');
  assert.equal(draft.attachmentsDir, path.join(draft.dir, 'attachments'));
  const copied = path.join(draft.attachmentsDir, 'test model.cadrs');
  assert.equal(fs.readFileSync(copied, 'utf8'), 'zip-bytes');
  assert.deepEqual(draft.attachments.map((a) => a.name), ['test model.cadrs']);

  const meta = JSON.parse(fs.readFileSync(draft.issueFile, 'utf8'));
  assert.equal(meta.title, 'Extrude of selected sketch regions fills the holes');
  assert.deepEqual(meta.attachments, [{ name: 'test model.cadrs', bytes: 9 }]);
  assert.match(meta.url, /github\.com\/rvdende\/cadrs\/issues\/new\?title=Extrude/);
  assert.equal(meta.link, '');
  assert.ok(fs.existsSync(path.join(draft.dir, 'attachments.zip')), 'a zip of every attachment is made for GitHub');
  assert.equal(draft.archive.name, 'attachments.zip');

  const index = fs.readFileSync(draft.index, 'utf8');
  assert.match(index, /cadrs issue drafts/);
  assert.match(index, /Extrude of selected sketch regions fills the holes/);
  assert.match(index, /\(2026-11-26-07-30-12-extrude-of-selected-sketch-regions-fills-the-holes\/issue\.md\)/);
});

test('issues.js: two issues with the same title get different folders', () => {
  const root = tempDir('dup');
  const a = issues.createDraft({ title: 'Same title', body: 'one', root });
  const b = issues.createDraft({ title: 'Same title', body: 'two', root });
  assert.notEqual(a.dir, b.dir);
  assert.match(path.basename(b.dir), /-same-title-2$/);
  const index = fs.readFileSync(b.index, 'utf8');
  assert.equal((index.match(/Same title/g) || []).length, 2, 'both drafts are indexed');
});

test('issues.js: a title and a description are required, and a bad attachment leaves no draft', () => {
  const root = tempDir('bad');
  assert.throws(() => issues.createDraft({ title: '', body: 'x', root }), /title/);
  assert.throws(() => issues.createDraft({ title: 'x', body: '  ', root }), /description/);
  assert.throws(
    () => issues.createDraft({ title: 'x', body: 'y', files: [path.join(root, 'missing.png')], root }),
    /ENOENT|no such file/i,
  );
  const base = issues.issuesDir(root);
  assert.ok(!fs.existsSync(base) || fs.readdirSync(base).every((name) => name === 'INDEX.md' || name === 'missing.png'));
  assert.ok(!fs.existsSync(base) || fs.readdirSync(base).filter((name) => !name.includes('.')).length === 0);
});

test('issues.js: the body rendering and the GitHub URL are what the UI shows', () => {
  assert.equal(issues.renderBody('hello', []), 'hello\n');
  const withFiles = issues.renderBody('hello', [{ name: 'a b.png', bytes: 3 }]);
  assert.match(withFiles, /hello\n\n### Attachments[\s\S]*`a b\.png` \(3 bytes\)/);
  assert.equal(
    issues.githubUrl('Annulus becomes a disk & holes'),
    'https://github.com/rvdende/cadrs/issues/new?title=Annulus%20becomes%20a%20disk%20%26%20holes',
  );
  assert.equal(issues.slugify('Extrude: rings → disks (bug #2)'), 'extrude-rings-disks-bug-2');
});

test('issues.js: the CLI writes the same draft and prints JSON', () => {
  const root = tempDir('cli');
  const attach = writeFile(root, 'screen.png', 'png');
  const result = spawnSync(
    process.execPath,
    [
      path.join(__dirname, '..', 'issues.js'),
      '--title', 'A cadrs bug',
      '--body', 'It happens when I do X.',
      '--attach', attach,
      '--root', root,
      '--json',
    ],
    { encoding: 'utf8' },
  );
  assert.equal(result.status, 0, result.stderr);
  const draft = JSON.parse(result.stdout);
  assert.equal(draft.title, 'A cadrs bug');
  assert.equal(draft.attachments[0].name, 'screen.png');
  assert.match(fs.readFileSync(draft.bodyFile, 'utf8'), /It happens when I do X\./);
});

test('issues.js: a bad CLI argument is a usage error, not a crash', () => {
  const result = spawnSync(process.execPath, [path.join(__dirname, '..', 'issues.js'), '--nope'], {
    encoding: 'utf8',
  });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /unknown argument: --nope/);
});
