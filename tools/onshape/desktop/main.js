// Chromium-based desktop app: an Onshape browser (persistent login) next to a panel to pick
// documents, scrape them with the existing scrape.js, import them with cadrs-onshape and read
// the report. No console snippets, no separate receiver.
const { dialog, app, BrowserWindow, WebContentsView, ipcMain, shell, session } = require('electron');
const { spawn, spawnSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { startReceiver } = require('./receiver');
const { build: buildBugReport } = require('./bugreport');
const { compare: compareFs } = require('./fsanalyze');
const { report: compareReport } = require('./comparereport');
const { toHtml } = require('./mdhtml');
const { summarize, roots: rootsReport, perDocument, perDocumentFromJson, formatPerDocument } = require('./reports');
const { readReportFiles } = require('./reportfiles');
const tickets = require('./tickets');
const { chromeUserAgent, applyGpuSwitches, softwareMode, softwareRelaunchArgs } = require('./chrome');
const issues = require('./issues');

const ROOT = process.env.CADRS_ONSHAPE_ROOT || path.join(os.homedir(), 'work', 'cadrs_onshape');
const RAW = path.join(ROOT, 'raw');
const REPO = path.resolve(__dirname, '..', '..', '..');
const PANEL_WIDTH = 460;
const PARTITION = 'persist:onshape';
const ONSHAPE = 'https://cad.onshape.com';

// Onshape checks the browser it is given and wants WebGPU; Electron presents its own user agent
// and Linux Chromium keeps WebGPU off behind a flag. Present plain Chrome and ask for the GPU
// features before the app is ready. Troubleshooting: CADRS_ONSHAPE_GPU=0 (no switches),
// CADRS_ONSHAPE_DISABLE_GPU=1 (software rendering), CADRS_ONSHAPE_EXTRA_FEATURES=a,b.
const BROWSER_UA = chromeUserAgent();
const SOFTWARE = softwareMode();
app.userAgentFallback = BROWSER_UA;
if (SOFTWARE) {
  // Asked for explicitly (`--disable-gpu` or CADRS_ONSHAPE_DISABLE_GPU=1): SwiftShader draw.
  app.commandLine.appendSwitch('disable-gpu');
} else {
  applyGpuSwitches(app.commandLine);
}

// The page talks to the local receiver; Chromium's private-network checks would block that.
app.commandLine.appendSwitch('disable-features', 'PrivateNetworkAccessSendPreflights,PrivateNetworkAccessForWorkers,BlockInsecurePrivateNetworkRequests,LocalNetworkAccessChecks');

let win, panel, onshape, receiver, importer = null, cancelled = false, busy = false;

const resource = (name) => (app.isPackaged ? path.join(process.resourcesPath, name) : path.join(__dirname, '..', name));
const ticketsDir = () => path.join(ROOT, 'tickets');
const issuesDir = () => path.join(ROOT, 'issues');

// python3 is optional now (reports and tickets are JS); only fs_decode.py and the report-summary
// fallback use it, and the UI degrades gracefully when it is missing.
let pythonOk = null;
function pythonAvailable() {
  if (pythonOk === null) {
    const r = spawnSync('python3', ['--version'], { encoding: 'utf8' });
    pythonOk = !r.error && !r.status;
  }
  return pythonOk;
}

// The cadrs app binary, when it is around: the "Open cadrs app" button is hidden/disabled
// otherwise. The importer writes normal cadrs documents to the app's document store.
function cadrsBinary() {
  const candidates = [];
  if (process.env.CADRS_BIN) candidates.push(process.env.CADRS_BIN);
  if (app.isPackaged) candidates.push(path.join(process.resourcesPath, 'bin', 'cadrs'));
  candidates.push(path.join(REPO, 'target', 'release', 'cadrs'), path.join(REPO, 'target', 'debug', 'cadrs'));
  return candidates.find((c) => { try { return fs.existsSync(c) && fs.statSync(c).isFile(); } catch { return false; } }) || null;
}

function send(ev) {
  if (panel && !panel.webContents.isDestroyed()) panel.webContents.send('event', ev);
}

// A GPU process that crashes (or an Onshape renderer that goes away) loses the page's WebGL
// context: Onshape then says "the WebGL rendering context has been lost. Refresh the browser."
// Reload the pane automatically a few times, and let the panel offer a software-rendering
// restart when it keeps happening.
let gpuCrashes = [];
let reloadAfterCrash = null;
function onGpuGone(what, details = {}) {
  const now = Date.now();
  gpuCrashes = gpuCrashes.filter((t) => now - t < 5 * 60 * 1000);
  gpuCrashes.push(now);
  const reason = details.reason || 'unknown';
  console.error(`onshape: ${what} gone (${reason}); ${gpuCrashes.length} time(s) in 5 min`);
  send({ type: 'gpu-crash', what, reason, count: gpuCrashes.length, software: SOFTWARE });
  if (SOFTWARE) return; // software rendering losing its context is not fixed by reloading
  if (reloadAfterCrash) clearTimeout(reloadAfterCrash);
  if (gpuCrashes.length <= 3 && onshape && !onshape.webContents.isDestroyed()) {
    reloadAfterCrash = setTimeout(() => {
      reloadAfterCrash = null;
      try {
        onshape.webContents.reload();
      } catch (e) {
        console.error('onshape: cannot reload after a GPU crash:', e);
      }
    }, 1500);
  }
}
app.on('child-process-gone', (_event, details) => {
  if (details && details.type === 'GPU') onGpuGone('the GPU process', details);
});
// The page's own signal (capture listener in onshape-preload.js): a lost WebGL context does not
// always kill the GPU process, and this is exactly what Onshape's "Refresh the browser" is about.
ipcMain.on('onshape-context-lost', () => onGpuGone('the WebGL context', { reason: 'the page lost it' }));
ipcMain.on('onshape-context-restored', () => {
  if (reloadAfterCrash) {
    clearTimeout(reloadAfterCrash);
    reloadAfterCrash = null;
  }
  send({ type: 'gpu-recovered' });
});

function layout() {
  const [w, h] = win.getContentSize();
  panel.setBounds({ x: 0, y: 0, width: PANEL_WIDTH, height: h });
  onshape.setBounds({ x: PANEL_WIDTH, y: 0, width: Math.max(w - PANEL_WIDTH, 0), height: h });
}

async function createWindow() {
  receiver = await startReceiver(ROOT);
  const onshapeSession = session.fromPartition(PARTITION);
  onshapeSession.setUserAgent(BROWSER_UA);
  keepLogin(onshapeSession);
  allowReceiver(onshapeSession, receiver.port);
  win = new BrowserWindow({ width: 1500, height: 950, title: 'cadrs Onshape Importer' });
  panel = new WebContentsView({
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true },
  });
  onshape = new WebContentsView({
    webPreferences: { partition: PARTITION, preload: path.join(__dirname, 'onshape-preload.js'), contextIsolation: true, sandbox: true },
  });
  onshape.webContents.setUserAgent(BROWSER_UA);
  win.contentView.addChildView(panel);
  win.contentView.addChildView(onshape);
  layout();
  win.on('resize', layout);
  panel.webContents.loadFile(path.join(__dirname, 'ui', 'index.html'));
  onshape.webContents.loadURL(ONSHAPE);
  for (const ev of ['did-navigate', 'did-navigate-in-page', 'page-title-updated']) onshape.webContents.on(ev, announceCurrent);
  for (const ev of ['did-navigate', 'did-navigate-in-page', 'did-stop-loading']) onshape.webContents.on(ev, sendNav);
  onshape.webContents.on('did-finish-load', () => {
    sendNav();
    send({ type: 'gpu-recovered' });
    if (onshapeReady()) sendGpuStatus();
  });
  onshape.webContents.on('before-input-event', onshapeShortcut);
  onshape.webContents.on('render-process-gone', (_e, details) => onGpuGone('the Onshape renderer', details));
  panel.webContents.on('render-process-gone', () => {
    if (panel && !panel.webContents.isDestroyed()) panel.webContents.reload();
  });
  // Links out of Onshape (help pages, etc.) open in the system browser.
  onshape.webContents.setWindowOpenHandler(({ url }) => {
    if (new URL(url).hostname.endsWith('onshape.com')) onshape.webContents.loadURL(url);
    else shell.openExternal(url);
    return { action: 'deny' };
  });
}

// Tells the panel which document is open in the Onshape pane.
function announceCurrent() {
  const id = documentId(onshape.webContents.getURL());
  if (!id) return;
  const name = onshape.webContents.getTitle().replace(/\s*\|\s*Onshape.*$/i, '').replace(/\s*-\s*Onshape.*$/i, '').trim();
  send({ type: 'current', id, name: name || `Document ${id}` });
}

// ---- the Onshape pane: navigation state, shortcuts and GPU diagnostics --------------------
function navState() {
  const wc = onshape && onshape.webContents;
  if (!wc || wc.isDestroyed()) return { canGoBack: false, canGoForward: false, url: '', title: '' };
  return {
    canGoBack: wc.canGoBack(),
    canGoForward: wc.canGoForward(),
    url: wc.getURL(),
    title: wc.getTitle(),
  };
}

function sendNav() {
  send({ type: 'nav', ...navState() });
}

async function gpuStatus() {
  const wc = onshape && onshape.webContents;
  if (!wc || wc.isDestroyed()) return { error: 'the Onshape pane is not open' };
  if (!onshapeReady()) return { error: 'Onshape is not loaded yet - open cad.onshape.com first' };
  try {
    const canvas = await wc.executeJavaScript(`(() => {
      const canvas = document.createElement('canvas');
      const gl2 = canvas.getContext('webgl2');
      const gl = gl2 || canvas.getContext('webgl');
      const debug = gl && gl.getExtension('WEBGL_debug_renderer_info');
      return {
        webgpu: typeof navigator.gpu !== 'undefined',
        webgl: !!gl,
        webgl2: !!gl2,
        renderer: gl ? (debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : 'unknown') : 'no WebGL',
        vendor: debug ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL) : '',
        ua: navigator.userAgent,
      };
    })()`, true);
    let adapter = 'no navigator.gpu';
    if (canvas.webgpu) {
      adapter = await wc.executeJavaScript(`(async () => {
        try {
          const a = await navigator.gpu.requestAdapter();
          if (!a) return 'no adapter (WebGPU is there, but no GPU driver accepted it)';
          const i = a.info || {};
          return [i.vendor, i.architecture, i.device, i.description].filter(Boolean).join(' ') || 'adapter available';
        } catch (e) { return 'requestAdapter failed: ' + e.message; }
      })()`, true);
    }
    return { ...canvas, adapter };
  } catch (e) {
    return { error: String(e && e.message ? e.message : e) };
  }
}

async function sendGpuStatus() {
  send({ type: 'gpu', status: await gpuStatus() });
}

// The Onshape pane has no browser chrome: give it the usual shortcuts.
function onshapeShortcut(event, input) {
  if (input.type !== 'keyDown') return;
  const wc = onshape.webContents;
  const key = input.key;
  if (input.alt && key === 'ArrowLeft') {
    if (wc.canGoBack()) wc.goBack();
    event.preventDefault();
  } else if (input.alt && key === 'ArrowRight') {
    if (wc.canGoForward()) wc.goForward();
    event.preventDefault();
  } else if (input.alt && key === 'Home') {
    wc.loadURL(ONSHAPE);
    event.preventDefault();
  } else if ((input.control || input.meta) && key.toLowerCase() === 'r') {
    wc.reload();
    event.preventDefault();
  } else if (key === 'F5') {
    wc.reload();
    event.preventDefault();
  }
}

// Onshape's Content-Security-Policy would block fetches to the receiver: allow just that origin.
function allowReceiver(ses, port) {
  const sink = `http://127.0.0.1:${port}`;
  ses.webRequest.onHeadersReceived((details, cb) => {
    const headers = { ...details.responseHeaders };
    for (const k of Object.keys(headers)) {
      if (k.toLowerCase() !== 'content-security-policy') continue;
      headers[k] = headers[k].map((policy) => {
        const dirs = policy.split(';').map((d) => d.trim()).filter(Boolean);
        const has = dirs.some((d) => d.startsWith('connect-src'));
        return dirs.map((d) => (d.startsWith('connect-src') || (!has && d.startsWith('default-src')) ? `${d} ${sink}` : d)).join('; ');
      });
    }
    cb({ responseHeaders: headers });
  });
}

// Chromium drops session cookies on exit; give Onshape's a 30-day life so the login survives restarts.
function keepLogin(ses) {
  const extend = (c) => {
    if (!c.session || !c.domain.endsWith('onshape.com')) return;
    const host = c.domain.replace(/^\./, '');
    ses.cookies.set({
      url: `https://${host}${c.path}`, name: c.name, value: c.value, domain: c.domain, path: c.path,
      secure: c.secure, httpOnly: c.httpOnly, sameSite: c.sameSite, expirationDate: Date.now() / 1000 + 30 * 86400,
    }).catch(() => {});
  };
  ses.cookies.on('changed', (_e, c, _cause, removed) => { if (!removed) extend(c); });
  ses.cookies.get({}).then((all) => all.forEach(extend));
}

const onshapeReady = () => {
  try {
    return new URL(onshape.webContents.getURL()).origin === ONSHAPE;
  } catch (e) {
    return false;
  }
};

// Document ids from pasted ids or Onshape URLs (/documents/<id>/...).
function documentId(s) {
  const m = /documents\/([0-9a-f]{24})/.exec(s) || /^([0-9a-f]{24})$/.exec(s.trim());
  return m ? m[1] : null;
}

// scrape.js's requests to the receiver, made from here (no CORS or page policy involved).
ipcMain.handle('sink', async (_e, { route, path: rel, method, body }) => {
  const url = `http://127.0.0.1:${receiver.port}/${route}${rel ? '?path=' + encodeURIComponent(rel) : ''}`;
  const init = { method, headers: { Origin: ONSHAPE } };
  if (method === 'POST') init.body = typeof body === 'string' ? body : Buffer.from(body);
  const r = await fetch(url, init);
  const buf = Buffer.from(await r.arrayBuffer());
  return { status: r.status, body: route === 'raw' ? new Uint8Array(buf) : buf.toString() };
});

ipcMain.handle('info', () => ({
  root: ROOT, raw: RAW, loggedIn: onshapeReady(), python: pythonAvailable(),
  cadrsBin: cadrsBinary(), ticketsDir: ticketsDir(), issuesDir: issuesDir(),
  reportFile: path.join(ROOT, 'import-report.txt'),
  versions: {
    app: app.getVersion(),
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    platform: `${os.type()} ${os.release()} (${os.arch()})`,
  },
}));
ipcMain.handle('current', announceCurrent);

ipcMain.handle('open-onshape', (_e, url) => {
  const id = url && documentId(url);
  onshape.webContents.loadURL(id ? `${ONSHAPE}/documents/${id}` : ONSHAPE);
});

ipcMain.handle('open-path', (_e, which) => {
  if (which === 'tickets') {
    fs.mkdirSync(ticketsDir(), { recursive: true });
    return shell.openPath(ticketsDir());
  }
  return shell.openPath(which === 'raw' ? RAW : ROOT);
});

// Lists the account's documents through the logged-in Onshape page.
ipcMain.handle('list-documents', async () => {
  if (!onshapeReady()) return { error: 'Open cad.onshape.com in the right-hand pane and log in first.' };
  const code = `(async () => {
    const out = [];
    for (let offset = 0; ; offset += 20) {
      const r = await fetch('/api/documents?filter=0&limit=20&offset=' + offset + '&sortColumn=modifiedAt&sortOrder=desc',
        { credentials: 'include', headers: { Accept: 'application/json' } });
      if (r.status === 401 || r.status === 403) return { error: 'Not logged in to Onshape.' };
      if (!r.ok) return { error: 'Onshape answered HTTP ' + r.status };
      const page = await r.json();
      for (const d of page.items) out.push({ id: d.id, name: d.name, modifiedAt: d.modifiedAt, owner: d.owner && d.owner.name });
      if (!page.next || !page.items.length) break;
    }
    return { documents: out };
  })()`;
  try {
    return await onshape.webContents.executeJavaScript(code);
  } catch (e) {
    return { error: String(e) };
  }
});

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function scrape(ids, refresh) {
  if (refresh) for (const id of ids) fs.rmSync(path.join(RAW, id), { recursive: true, force: true });
  // The page hands data to the receiver through the preload bridge: fetching a local http
  // address from a public https page is blocked in too many ways to rely on.
  const src = fs.readFileSync(resource('scrape.js'), 'utf8');
  const sinkDef = /const sink = \(route, path, init\) =>\s*fetch\([^\n]*\n/;
  if (!sinkDef.test(src)) throw new Error('scrape.js: sink definition not found');
  const js = src.replace(sinkDef, `const sink = async (route, path, init = {}) => {
    const r = await window.cadrsBridge.sink(route, path, init.method || 'GET', init.body);
    return new Response(r.status === 204 ? null : r.body, { status: r.status });
  };\n`);
  await onshape.webContents.executeJavaScript(js);
  await onshape.webContents.executeJavaScript(`cadrsScrape.start({only: ${JSON.stringify(ids)}})`);
  for (;;) {
    await sleep(1000);
    const st = JSON.parse(await onshape.webContents.executeJavaScript('JSON.stringify(cadrsScrape.state)'));
    send({ type: 'scrape', state: st });
    if (cancelled) {
      await onshape.webContents.executeJavaScript('cadrsScrape.stop()');
    }
    if (['done', 'failed'].includes(st.phase)) return st;
  }
}

function importerCommand() {
  const exe = ['cadrs-onshape'];
  const candidates = app.isPackaged
    ? [path.join(process.resourcesPath, 'bin', 'cadrs-onshape')]
    : ['release', 'debug'].map((p) => path.join(REPO, 'target', p, 'cadrs-onshape'));
  const bin = candidates.find((c) => fs.existsSync(c));
  if (bin) return { cmd: bin, args: [], cwd: REPO };
  return { cmd: 'cargo', args: ['run', '--release', '-q', '-p', 'cadrs_onshape', '--'], cwd: REPO };
}

// Older importer builds have no --json flag, so probe the command's usage once and only ask for
// the machine-readable report when it is supported; the text report is always written.
let jsonSupported = null;
function importerSupportsJson() {
  if (jsonSupported !== null) return jsonSupported;
  try {
    const { cmd, args, cwd } = importerCommand();
    const r = spawnSync(cmd, [...args, '--help'], { cwd, encoding: 'utf8', timeout: 10000, maxBuffer: 1 << 20 });
    jsonSupported = /--json\b/.test(`${r.stdout || ''}${r.stderr || ''}`);
  } catch {
    jsonSupported = false;
  }
  return jsonSupported;
}

function runImport(ids, dryRun) {
  const { cmd, args, cwd } = importerCommand();
  // The importer also writes the machine-readable aggregate report: the Tickets tab and the
  // per-document summary read it first and fall back to parsing import-report.txt. Dry runs do
  // not replace the last full report, so they do not ask for JSON.
  const jsonFile = path.join(ROOT, 'import-report.json');
  const jsonArgs = !dryRun && importerSupportsJson() ? ['--json', jsonFile] : [];
  const full = [...args, 'import', ...ids, '--raw', RAW, ...jsonArgs, ...(dryRun ? ['--dry-run'] : [])];
  return new Promise((resolve) => {
    importer = spawn(cmd, full, { cwd, env: process.env });
    const pipe = (s) => s.on('data', (d) => send({ type: 'import-log', text: d.toString() }));
    pipe(importer.stdout);
    pipe(importer.stderr);
    importer.on('error', (e) => {
      send({ type: 'import-log', text: `cannot run importer: ${e.message}\n` });
      resolve(-1);
    });
    importer.on('close', (code) => {
      importer = null;
      resolve(code);
    });
  });
}

function py(script) {
  const r = spawnSync('python3', [resource(script)], { env: { ...process.env, CADRS_ONSHAPE_ROOT: ROOT }, encoding: 'utf8' });
  return r.error ? `(python3 not available: ${r.error.message})` : r.stdout + r.stderr;
}

// The report, plus the "what is missing" summaries, saved next to it. The summaries are the JS
// ports of summarize.py/roots.py; python3 is only a fallback if the JS port ever throws.
function buildReports() {
  const { textFile: file, text: report, aggregate, jsonError, warnings: reportWarnings } = readReportFiles(ROOT);
  let summary = '', roots = '', source = 'js';
  const warnings = [];
  if (jsonError) warnings.push(jsonError);
  warnings.push(...reportWarnings);
  if (report) {
    try {
      summary = summarize(report);
      roots = rootsReport(report);
    } catch (e) {
      warnings.push(`JS report summary failed: ${e.message}`);
      if (pythonAvailable()) {
        summary = py('summarize.py');
        roots = py('roots.py');
        source = 'python';
      } else {
        summary = `(could not summarize the report: ${e.message})`;
      }
    }
  }
  let documents = [];
  let byDocument = '';
  try {
    if (aggregate) {
      documents = perDocumentFromJson(aggregate);
      byDocument = formatPerDocument(documents);
    } else {
      documents = perDocument(report);
      byDocument = formatPerDocument(documents);
    }
  } catch (e) {
    warnings.push(`per-document summary failed: ${e.message}`);
    try {
      documents = perDocument(report);
      byDocument = formatPerDocument(documents);
    } catch { /* leave the per-document summary empty */ }
  }
  if (report) {
    fs.writeFileSync(path.join(ROOT, 'missing-summary.txt'),
      `${summary}\n\nPER-DOCUMENT RESULTS\n${byDocument}\n\nFIRST FAILURE PER PART STUDIO\n${roots}`);
  }
  return { report, summary, roots, byDocument, documents, file, source, warnings };
}

ipcMain.handle('reports', buildReports);

// Ticket drafts: one Markdown GitHub issue per translation defect group, written offline to
// <root>/tickets/ (INDEX.md + one file per stable dedup key).
function buildTicketDrafts() {
  const { textFile, jsonFile, text, aggregate, jsonError, warnings: reportWarnings } = readReportFiles(ROOT);
  if (!text && !aggregate) return { error: jsonError ? `cannot read the import report: ${jsonError}` : 'No import report yet: convert a document first.' };
  // Prefer the importer's JSON report (it already grouped the defects with the same keys);
  // fall back to parsing the text report.
  const source = aggregate || text;
  const reportFile = aggregate ? jsonFile : textFile;
  const r = tickets.writeTickets(ticketsDir(), source, {
    rawDir: RAW, reportFile, appVersion: app.getVersion(), date: new Date().toISOString().slice(0, 10),
  });
  const fresh = r.tickets.filter((t) => !t.previous).length;
  return {
    dir: r.dir,
    indexFile: r.indexFile,
    index: r.index,
    summary: {
      groups: r.tickets.length, occurrences: r.totals.occurrences, ...r.totals.statuses,
      documents: r.documents, fresh, refreshed: r.tickets.length - fresh,
      source: aggregate ? 'json' : 'text', warnings: [...reportWarnings, ...(jsonError ? [`JSON report ignored: ${jsonError}`] : [])],
    },
    tickets: r.tickets.map((t) => ({
      key: t.key, title: t.title, file: t.file, path: t.path, status: t.status, severity: t.severity,
      statuses: t.statuses, kind: t.kind, reason: t.reason, occurrences: t.occurrences, labels: t.labels,
      previous: t.previous, markdown: t.markdown,
      documents: t.documents.map((d) => ({ id: d.id, name: d.name })),
    })),
  };
}

ipcMain.handle('tickets-generate', buildTicketDrafts);

ipcMain.handle('tickets-copy', (_e, text) => {
  require('electron').clipboard.writeText(String(text || ''));
  return true;
});

// Opens GitHub's short new-issue URL with the title prefilled; the body is on the clipboard
// (the URL keeps no body so it stays short).
ipcMain.handle('tickets-open', (_e, { title, markdown, key } = {}) => {
  const url = openIssue(title || 'Onshape import defect', markdown);
  return { url, key: key || null };
});

// FeatureScript analysis: timeline and functions, with Onshape's regeneration times and, when a
// document is given, cadrs's per-feature import times (a traced dry run).
// General cadrs issues (not tied to an Onshape import): the body text and the attachments are
// kept apart. `issues-create` writes <root>/issues/<date>-<slug>/{issue.md,issue.json,attachments/};
// opening GitHub copies the text and opens the new-issue page with the title (GitHub cannot be
// given attachments through a URL, so the panel tells the user to drag them in).
ipcMain.handle('issue-pick-files', async () => {
  const r = await dialog.showOpenDialog({
    title: 'Attach files to a cadrs issue',
    properties: ['openFile', 'multiSelections'],
  });
  return r.canceled ? [] : r.filePaths;
});
ipcMain.handle('issue-create', (_e, { title, body, files } = {}) => {
  try {
    return issues.createDraft({ title, body, files: files || [], root: ROOT });
  } catch (e) {
    return { error: String(e && e.message ? e.message : e) };
  }
});
ipcMain.handle('issue-copy', (_e, text) => {
  require('electron').clipboard.writeText(String(text || ''));
  return { copied: true };
});
ipcMain.handle('issue-open', (_e, { title, body } = {}) => openIssue(title || 'cadrs issue', body || ''));
ipcMain.handle('open-issues', () => {
  fs.mkdirSync(issuesDir(), { recursive: true });
  return shell.openPath(issuesDir());
});

ipcMain.handle('fs-open', async () => {
  const r = await dialog.showOpenDialog({ filters: [{ name: 'FeatureScript', extensions: ['fs', 'txt', '*'] }], properties: ['openFile'] });
  return r.canceled ? null : fs.readFileSync(r.filePaths[0], 'utf8');
});

ipcMain.handle('fs-compare', (_e, { text, regen, docId }) => {
  let trace = '';
  if (docId) {
    const { cmd, args, cwd } = importerCommand();
    const r = spawnSync(cmd, [...args, 'import', docId, '--raw', RAW, '--dry-run'], { cwd, encoding: 'utf8', maxBuffer: 1 << 28, env: { ...process.env, CADRS_ONSHAPE_TRACE: '1' } });
    trace = r.error ? '' : `${r.stdout}\n${r.stderr}`;
  }
  const r = compareFs(text, regen, trace);
  const out = path.join(ROOT, 'featurescript-analysis.md');
  fs.writeFileSync(out, r.markdown);
  return { markdown: r.markdown, file: out, traced: !!trace };
});

// Compare & report: the cadrs file against Onshape's STEP (and FeatureScript, decoded).
ipcMain.handle('pick-file', async (_e, ext) => {
  const store = path.join(os.homedir(), '.local', 'share', 'cadrs', 'documents');
  const imported = ext === 'cadrs';
  const r = await dialog.showOpenDialog({
    title: imported ? 'Pick a .cadrs file, or document.ron of an imported document' : undefined,
    defaultPath: imported && fs.existsSync(store) ? store : undefined,
    filters: [{ name: ext, extensions: imported ? ['cadrs', 'ron'] : [ext] }], properties: ['openFile'],
  });
  return r.canceled ? null : r.filePaths[0];
});

ipcMain.handle('compare-run', (_e, { fsText, cadrsFile, stepFile }) => {
  if (!cadrsFile || !stepFile) return { error: 'Choose both the .cadrs file and the STEP file.' };
  const { cmd, args, cwd } = importerCommand();
  const r = spawnSync(cmd, [...args, 'compare', cadrsFile, stepFile], { cwd, encoding: 'utf8', maxBuffer: 1 << 28 });
  if (r.error || r.status !== 0) return { error: `compare failed: ${r.error ? r.error.message : r.stderr}` };
  let decoded = null, note = null;
  if (fsText && fsText.trim()) {
    if (pythonAvailable()) {
      try {
        const tmp = path.join(os.tmpdir(), `cadrs-fs-${process.pid}.fs`);
        fs.writeFileSync(tmp, fsText);
        const d = spawnSync('python3', [resource('fs_decode.py'), tmp], { encoding: 'utf8', maxBuffer: 1 << 26 });
        fs.rmSync(tmp, { force: true });
        decoded = JSON.parse(d.stdout);
      } catch (e) {
        decoded = null;
        note = `FeatureScript query decoding failed (${e.message}); the comparison report still works.`;
      }
    } else {
      note = 'python3 not found: FeatureScript query decoding skipped (the comparison, steps and geometry report still work).';
    }
  }
  const out = compareReport({ fs: fsText && fsText.trim() ? fsText : '', decoded, cmp: JSON.parse(r.stdout) });
  const file = path.join(ROOT, 'compare-report.md');
  fs.writeFileSync(file, out.markdown);
  return { markdown: out.markdown, defects: out.defects, file, note };
});

async function writePdf(file, markdown, name) {
  const win = new BrowserWindow({ show: false });
  try {
    await win.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(toHtml(markdown, name)));
    fs.writeFileSync(file, await win.webContents.printToPDF({ landscape: true, printBackground: true, pageSize: 'A4' }));
  } finally { win.destroy(); }
}

// A folder with the compared files (FeatureScript, cadrs, STEP) and the complete report.
ipcMain.handle('export-bundle', async (_e, { markdown, fsText, cadrsFile, stepFile }) => {
  const r = await dialog.showOpenDialog({ title: 'Choose where to create the comparison folder', defaultPath: ROOT, properties: ['openDirectory', 'createDirectory'] });
  if (r.canceled) return null;
  const stamp = new Date().toISOString().replace(/[:T]/g, '-').slice(0, 19);
  const dir = path.join(r.filePaths[0], `compare-${stamp}`);
  fs.mkdirSync(dir, { recursive: true });
  const put = (file, name) => { if (file && fs.existsSync(file)) fs.copyFileSync(file, path.join(dir, name)); };
  put(cadrsFile, 'cadrs-' + path.basename(cadrsFile || ''));
  put(stepFile, 'onshape-' + path.basename(stepFile || ''));
  if (fsText && fsText.trim()) fs.writeFileSync(path.join(dir, 'featurescript.fs'), fsText);
  fs.writeFileSync(path.join(dir, 'report.md'), markdown);
  await writePdf(path.join(dir, 'report.pdf'), markdown, 'report');
  return dir;
});

ipcMain.handle('export-report', async (_e, { markdown, format, name }) => {
  const pdf = format === 'pdf';
  const r = await dialog.showSaveDialog({
    defaultPath: path.join(ROOT, `${name || 'report'}.${pdf ? 'pdf' : 'md'}`),
    filters: [{ name: pdf ? 'PDF' : 'Markdown', extensions: [pdf ? 'pdf' : 'md'] }],
  });
  if (r.canceled || !r.filePath) return null;
  if (!pdf) { fs.writeFileSync(r.filePath, markdown); return r.filePath; }
  await writePdf(r.filePath, markdown, name);
  return r.filePath;
});

// GitHub's new-issue form gets a short URL (title only); the body is put on the clipboard,
// so the URL stays well below the length limit and works offline.
function openIssue(title, body) {
  require('electron').clipboard.writeText(String(body || ''));
  const url = `https://github.com/rvdende/cadrs/issues/new?title=${encodeURIComponent(String(title || 'cadrs issue')).slice(0, 2000)}`;
  shell.openExternal(url);
  return url;
}

ipcMain.handle('issue', (_e, text, title) => openIssue(title, text));

// Optional: start the cadrs app (the imported documents are already in its document store).
// Navigation for the Onshape pane (it has no browser chrome of its own) and a GPU report that
// says whether the pane can draw at all: WebGPU/WebGL2 and the renderer Chromium picked.
ipcMain.handle('onshape-nav', (_e, action) => {
  const wc = onshape && onshape.webContents;
  if (!wc || wc.isDestroyed()) return navState();
  try {
    if (action === 'back' && wc.canGoBack()) wc.goBack();
    else if (action === 'forward' && wc.canGoForward()) wc.goForward();
    else if (action === 'reload') wc.reload();
    else if (action === 'home') wc.loadURL(ONSHAPE);
    else if (action === 'devtools') wc.openDevTools({ mode: 'detach' });
  } catch (e) {
    console.error('onshape-nav:', e);
  }
  sendNav();
  return navState();
});
ipcMain.handle('onshape-gpu', () => gpuStatus());

// "Restart in software mode" from the panel: relaunch this app with software rendering (the
// Onshape login survives in its persistent partition, so the document is still there).
ipcMain.handle('restart-software', () => {
  app.relaunch({ args: softwareRelaunchArgs() });
  app.exit(0);
  return { relaunching: true };
});

ipcMain.handle('open-cadrs', () => {
  const bin = cadrsBinary();
  if (!bin) return { error: 'cadrs app not found: build it with `cargo build --release -p cadrs`, or set CADRS_BIN.' };
  try {
    const child = spawn(bin, [], { cwd: path.dirname(bin), detached: true, stdio: 'ignore' });
    child.unref();
    return { ok: true, bin };
  } catch (e) {
    return { error: `cannot start ${bin}: ${e.message}` };
  }
});

// A GitHub-ready bug report: saved next to the import report and copied to the clipboard.
ipcMain.handle('bug-report', (_e, open) => {
  const file = path.join(ROOT, 'import-report.txt');
  const text = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
  const r = buildBugReport(text, RAW, app.getVersion());
  const out = path.join(ROOT, 'bug-report.md');
  fs.writeFileSync(out, r.markdown);
  require('electron').clipboard.writeText(r.markdown);
  if (open && r.count) openIssue(r.title, r.markdown);
  return { ...r, file: out };
});

ipcMain.handle('cancel', () => {
  cancelled = true;
  if (importer) importer.kill();
});

ipcMain.handle('convert', async (_e, { ids, refresh, dryRun }) => {
  if (busy) return { error: 'A conversion is already running.' };
  if (!onshapeReady()) return { error: 'Log in to Onshape first.' };
  ids = ids.map(documentId).filter(Boolean);
  if (!ids.length) return { error: 'No documents selected.' };
  busy = true;
  cancelled = false;
  try {
    send({ type: 'phase', phase: 'Downloading from Onshape' });
    const st = await scrape(ids, refresh);
    if (st.phase === 'failed' || cancelled) {
      send({ type: 'phase', phase: cancelled ? 'Cancelled' : 'Download failed' });
      return { error: st.errors.join('\n') || 'Cancelled', reports: buildReports() };
    }
    send({ type: 'phase', phase: 'Converting to cadrs' });
    const code = await runImport(ids, dryRun);
    send({ type: 'phase', phase: 'Building report' });
    const reports = buildReports();
    send({ type: 'phase', phase: code === 0 ? 'Done' : `Importer exited with ${code}` });
    return { code, scrapeErrors: st.errors, reports };
  } finally {
    busy = false;
  }
});

app.whenReady().then(createWindow);
app.on('window-all-closed', () => {
  if (receiver) receiver.close();
  app.quit();
});
