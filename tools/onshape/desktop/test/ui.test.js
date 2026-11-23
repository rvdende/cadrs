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
  for (const file of ['main.js', 'chrome.js', 'preload.js', 'tickets.js', 'reports.js', 'reportfiles.js', 'bugreport.js', 'fsanalyze.js', 'comparereport.js', 'mdhtml.js', 'ui/**']) {
    assert.ok(pkg.build.files.includes(file), `packaged file ${file}`);
  }
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
