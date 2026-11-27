// Static checks for the desktop UI and its main/preload wiring: the panel is plain HTML with an
// inline script, so compile the script and assert the important IPC surface exists.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const desktop = path.resolve(__dirname, '..');
const appDir = path.resolve(desktop, '..');
const html = fs.readFileSync(path.join(desktop, 'ui', 'index.html'), 'utf8');
const mainSource = fs.readFileSync(path.join(desktop, 'main.js'), 'utf8');
const preloadSource = fs.readFileSync(path.join(desktop, 'preload.js'), 'utf8');
const pkg = JSON.parse(fs.readFileSync(path.join(desktop, 'package.json'), 'utf8'));

test('ui: the inline script has no syntax errors', () => {
  const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]);
  assert.ok(scripts.length >= 1, 'index.html has an inline script');
  for (const code of scripts) {
    assert.doesNotThrow(() => new vm.Script(code, { filename: 'ui/index.html' }), 'inline script compiles');
  }
});

test('ui: Tickets tab and its controls are wired', () => {
  for (const id of ['tickets', 'tGen', 'tSave', 'tOpen', 'tInfo', 'tSummary', 'tList', 'tIndex']) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} exists`);
  }
  assert.match(html, /tickets: 'Tickets'/, 'Tickets tab in the nav');
  assert.match(html, /api\.ticketsGenerate\(/);
  assert.match(html, /api\.ticketCopy\(/);
  assert.match(html, /api\.ticketOpen\(/);
  assert.match(html, /api\.openPath\('tickets'\)/);
});

test('ui: the New ticket tab keeps text and attachments apart', () => {
  assert.match(html, /newissue: 'New ticket'/, 'New ticket tab in the nav');
  for (const id of ['newissue', 'iTitle', 'iWhat', 'iEnv', 'iPick', 'iFiles', 'iClear', 'iSave', 'iCopy', 'iGh', 'iFolder', 'iOut']) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} exists`);
  }
  assert.match(html, /api\.pickIssueFiles\(/);
  assert.match(html, /api\.issueCreate\(/);
  assert.match(html, /api\.issueCopy\(/);
  assert.match(html, /api\.issueOpen\(/);
  assert.match(html, /api\.openIssues\(/);
  assert.match(preloadSource, /pickIssueFiles: \(\) => ipcRenderer\.invoke\('issue-pick-files'\)/);
  assert.match(preloadSource, /issueCreate: \(issue\) => ipcRenderer\.invoke\('issue-create'/);
  for (const channel of ['issue-pick-files', 'issue-create', 'issue-copy', 'issue-open', 'open-issues']) {
    assert.match(mainSource, new RegExp(`ipcMain\\.handle\\('${channel}'`), `handler ${channel}`);
  }
  assert.match(mainSource, /require\('\.\/issues'\)/);
});

test('ui: onboarding, per-document results and guarded buttons exist', () => {
  assert.match(html, /id="onboard"/);
  assert.match(html, /id="loginHint"/);
  assert.match(html, /id="bCadrs"/);
  assert.match(html, /data-r="byDocument"/);
  assert.match(html, /id="bLoadOnboard"/);
  assert.match(html, /Open cadrs app/);
});

test('preload: exposes the tickets and cadrs IPC bridge', () => {
  for (const name of ['ticketsGenerate', 'ticketCopy', 'ticketOpen', 'openCadrs', 'reports', 'openPath']) {
    assert.match(preloadSource, new RegExp(`${name}:`), `preload exposes ${name}`);
  }
});

test('main: registers the tickets, reports and cadrs IPC handlers', () => {
  for (const channel of ['tickets-generate', 'tickets-copy', 'tickets-open', 'open-cadrs', 'reports', 'bug-report', 'compare-run']) {
    assert.match(mainSource, new RegExp(`ipcMain\\.handle\\('${channel.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}'`), `handler ${channel}`);
  }
  assert.match(mainSource, /require\('\.\/reports'\)/);
  assert.match(mainSource, /require\('\.\/tickets'\)/);
  assert.match(mainSource, /require\('\.\/reportfiles'\)/);
  assert.match(mainSource, /importerSupportsJson/);
  assert.match(mainSource, /--json/);
  assert.match(mainSource, /openIssue\(/);
  assert.doesNotMatch(mainSource, /Paste the report from the clipboard/);
  assert.match(mainSource, /python3/);
  assert.match(mainSource, /fs_decode\.py/);
  assert.match(mainSource, /missing-summary\.txt/);
});

test('ui: the Onshape pane has navigation and a GPU status line', () => {
  for (const id of ['nBack', 'nFwd', 'nReload', 'nHome', 'nUrl', 'gpu', 'bGpu']) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} exists`);
  }
  assert.match(html, /api\.nav\(/);
  assert.match(html, /api\.gpu\(/);
  assert.match(html, /describeGpu/);
  assert.match(html, /data-open="\$\{esc\(d\.id\)\}"/);
  assert.match(preloadSource, /nav: \(action\) => ipcRenderer\.invoke\('onshape-nav'/);
  assert.match(preloadSource, /gpu: \(\) => ipcRenderer\.invoke\('onshape-gpu'\)/);
});

test('ui: a lost WebGL context offers reload or a software restart', () => {
  for (const id of ['gpuLoss', 'gpuLossWhy', 'gpuReload', 'gpuSoftware']) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} exists`);
  }
  assert.match(html, /api\.restartSoftware\(\)/);
  assert.match(preloadSource, /restartSoftware: \(\) => ipcRenderer\.invoke\('restart-software'\)/);
  assert.match(mainSource, /ipcMain\.handle\('restart-software'/);
  assert.match(mainSource, /child-process-gone/);
  assert.match(mainSource, /render-process-gone/);
  assert.match(mainSource, /softwareRelaunchArgs/);
  assert.match(mainSource, /onshape-context-lost/);
  assert.match(mainSource, /onshape-context-restored/);
  assert.match(fs.readFileSync(path.join(desktop, 'onshape-preload.js'), 'utf8'), /webglcontextlost/);
});

test('ui: the Report tab exports the imported documents as .cadrs packages', () => {
  for (const id of ['rExport', 'rFolder']) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} exists`);
  }
  assert.match(html, /api\.exportCadrs\(\)/);
  assert.match(html, /api\.openCadrsFolder\(\)/);
  assert.match(preloadSource, /exportCadrs: \(\) => ipcRenderer\.invoke\('export-cadrs'\)/);
  assert.match(preloadSource, /openCadrsFolder: \(\) => ipcRenderer\.invoke\('open-cadrs-folder'\)/);
  assert.match(mainSource, /ipcMain\.handle\('export-cadrs'/);
  assert.match(mainSource, /ipcMain\.handle\('open-cadrs-folder'/);
  assert.match(mainSource, /cadrs_id/);
  assert.match(mainSource, /'export'/);
});

test('ui: the Document list marks imported/scraped documents and the exports can be cancelled', () => {
  assert.match(html, /id="docLegend"/);
  assert.match(html, /api\.state\(\)/);
  assert.match(html, /api\.cancelExport\(\)/);
  assert.match(html, /refreshState/);
  assert.match(html, /importedIds/);
  assert.match(preloadSource, /state: \(\) => ipcRenderer\.invoke\('state'\)/);
  assert.match(preloadSource, /cancelExport: \(\) => ipcRenderer\.invoke\('cancel-export'\)/);
  assert.match(mainSource, /ipcMain\.handle\('state'/);
  assert.match(mainSource, /ipcMain\.handle\('cancel-export'/);
  assert.match(mainSource, /exportTools\.preflight/);
  assert.match(mainSource, /importerAsync/);
});

test('ui: a new ticket says what kind it is and where it happens', () => {
  for (const id of ['iKind', 'iWhere']) assert.match(html, new RegExp(`id="${id}"`));
  assert.match(html, /### Type/);
  assert.match(html, /### Where/);
  assert.match(html, /issueLabels\(\)/);
  assert.match(html, /labels: issueLabels\(\)/);
  assert.match(mainSource, /labels: labels \|\| \[\]/);
});

test('ui: the Report tab also exports rebuilt STEP files', () => {
  for (const id of ['rStep', 'rStepFolder']) {
    assert.match(html, new RegExp(`id="${id}"`), `#${id} exists`);
  }
  assert.match(html, /api\.exportSteps\(\)/);
  assert.match(html, /api\.openStepsFolder\(\)/);
  assert.match(preloadSource, /exportSteps: \(\) => ipcRenderer\.invoke\('export-steps'\)/);
  assert.match(preloadSource, /openStepsFolder: \(\) => ipcRenderer\.invoke\('open-steps-folder'\)/);
  assert.match(mainSource, /ipcMain\.handle\('export-steps'/);
  assert.match(mainSource, /ipcMain\.handle\('open-steps-folder'/);
  assert.match(mainSource, /'step', doc\.cadrsId/);
});

test('ui: each conversion has its own run folder, with an operation analysis', () => {
  for (const id of ['runPick', 'rRunFolder', 'runInfo']) assert.match(html, new RegExp(`id="${id}"`), `#${id}`);
  assert.match(html, /data-r="operations"/);
  assert.match(html, /api\.runs\(\)/);
  assert.match(html, /api\.openRunFolder\(/);
  assert.match(html, /loadReports\(/);
  assert.match(preloadSource, /runs: \(\) => ipcRenderer\.invoke\('runs'\)/);
  assert.match(preloadSource, /openRunFolder: \(run\) => ipcRenderer\.invoke\('open-run-folder'/);
  assert.match(mainSource, /ipcMain\.handle\('runs'/);
  assert.match(mainSource, /ipcMain\.handle\('open-run-folder'/);
  assert.match(mainSource, /makeRun\(\)/);
  assert.match(mainSource, /--report-dir/);
  assert.match(mainSource, /buildOperations/);
  assert.ok(pkg.build.files.includes('operations.js'), 'operations.js packaged');
});

test("ui: New ticket starts from the repo issue templates", () => {
  assert.match(html, /id="iTemplate"/);
  assert.match(html, /Feature request \(cadrs\)/);
  assert.match(html, /ISSUE_TEMPLATES/);
  assert.match(html, /## Acceptance criteria/);
  assert.match(html, /## Current behavior/);
  assert.match(html, /## Related issues \(not the same request\)/);
});

test('main: exports read the report from the run folder (regression: they read the data root)', () => {
  assert.match(mainSource, /function readImportReport\(run = runDir\(\)\)/);
  assert.match(mainSource, /let file = path\.join\(run, 'import-report\.json'\)/);
  assert.doesNotMatch(mainSource, /^\s*const file = path\.join\(ROOT, 'import-report\.json'\);/m);
});

test('main: a closed stdout/stderr (EPIPE) does not crash the app', () => {
  assert.match(mainSource, /for \(const stream of \[process\.stdout, process\.stderr\]\)/);
  assert.match(mainSource, /stream\.on\('error', \(\) => \{\}\)/);
  assert.match(mainSource, /uncaughtException/);
  assert.match(mainSource, /err\.code === 'EPIPE'/);
});

test('ui: the run-import writes .cadrs packages without another click', () => {
  assert.match(mainSource, /cadrsExport = await exportDocuments\('cadrs'\)/);
  assert.match(html, /cadrs packages written automatically/);
});

test('ui: a new ticket can carry a link and a zip of its attachments', () => {
  assert.match(html, /id="iLink"/);
  assert.match(html, /### Link/);
  assert.match(html, /draft\.archive/);
  assert.match(mainSource, /link: link \|\| ''/);
});

test('main: the Onshape pane gets a Chrome identity, GPU switches and navigation IPC', () => {
  assert.match(mainSource, /require\('\.\/chrome'\)/);
  assert.match(mainSource, /app\.userAgentFallback = BROWSER_UA/);
  assert.match(mainSource, /applyGpuSwitches\(app\.commandLine\)/);
  assert.match(mainSource, /ipcMain\.handle\('onshape-nav'/);
  assert.match(mainSource, /ipcMain\.handle\('onshape-gpu'/);
  assert.match(mainSource, /before-input-event/);
  assert.match(mainSource, /send\(\{ type: 'nav'/);
});

test('package.json: test script and packaged files include the new modules', () => {
  assert.equal(pkg.scripts.test, 'node --test "test/**/*.test.js"');
  assert.equal(pkg.scripts.tickets, 'node tickets.js');
  for (const file of ['main.js', 'chrome.js', 'preload.js', 'tickets.js', 'issues.js', 'exports.js', 'reports.js', 'reportfiles.js', 'bugreport.js', 'fsanalyze.js', 'comparereport.js', 'mdhtml.js', 'ui/**']) {
    assert.ok(pkg.build.files.includes(file), `packaged file ${file}`);
  }
});

test('every desktop module parses inside the CommonJS wrapper (no `exports` shadowing)', () => {
  // Electron loads main.js as a CommonJS module, i.e. inside `function (exports, require, module…)`;
  // `const exports = require('./exports')` there is a SyntaxError that `node --check` does not see
  // (it checks as a plain script). Compile each module in the same wrapper scope instead.
  const files = fs.readdirSync(desktop).filter((name) => name.endsWith('.js')).sort();
  assert.ok(files.length >= 10, 'the desktop modules are there');
  for (const name of files) {
    const source = fs.readFileSync(path.join(desktop, name), 'utf8');
    assert.doesNotThrow(
      () => new Function('exports', 'require', 'module', '__filename', '__dirname', source),
      `${name} parses in the CommonJS wrapper`,
    );
  }
  assert.doesNotMatch(mainSource, /\bconst exports\s*=/, 'main.js does not shadow the exports global');
});

test('build.sh: runs the tests before packaging and uses writable caches', () => {
  const build = fs.readFileSync(path.join(appDir, 'build.sh'), 'utf8');
  assert.match(build, /node --test "test\/\*\*\/\*\.test\.js"/);
  assert.match(build, /ELECTRON_BUILDER_CACHE/);
  assert.match(build, /npm_config_cache/);
  assert.match(build, /electron-builder\/out\/cli\/cli\.js/);
  assert.match(build, /--test/);
  assert.match(build, /--run/);
  assert.match(build, /node_modules\/electron/);
  assert.match(build, /run_tests/);
});
