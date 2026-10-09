// In-browser Onshape scraper. Runs inside a logged-in cad.onshape.com tab, which authenticates
// every /api request with the session cookie, and POSTs each response to tools/onshape/receiver.py.
//
// Load it in the tab with:
//   eval(await (await fetch('http://127.0.0.1:8765/scrape.js')).text()); cadrsScrape.start()
// then poll `cadrsScrape.state`. A rerun skips every file already on disk.
//
// Layout under raw/:
//   _index/documents-<offset>.json          the document list pages
//   <did>/document.json, workspaces.json, versions.json, history.json, elements.json,
//         externalreferences.json, thumbnail.png
//   <did>/<eid>/...                         per element, depending on its type (see ELEMENT_FETCHES)
//   <did>/versions/<vid>/<eid>/features.json  a Part Studio at a version another document pins
(() => {
  const SINK = 'http://127.0.0.1:8765';
  // The REST API version: the one Onshape calls current (`/api/versions`), looked up when a scrape
  // starts; v17 until then (as of 2026-10-09; its responses are shaped as v10's, which the raw
  // data was first scraped with).
  let API = '/api/v17';
  const PACE_MS = 300;
  const JSON_ACCEPT = 'application/json;charset=UTF-8; qs=0.09';

  const state = { phase: 'idle', docsTotal: 0, docsDone: 0, requests: 0, skipped: 0, errors: [], current: '' };

  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const sink = (route, path, init) =>
    fetch(`${SINK}/${route}${path ? '?path=' + encodeURIComponent(path) : ''}`, { targetAddressSpace: 'loopback', ...init });

  async function log(msg) {
    try { await sink('log', null, { method: 'POST', body: `${new Date().toISOString()} ${msg}` }); } catch (e) { /* sink down */ }
  }

  async function have(path) {
    return (await sink('have', path)).status === 200;
  }

  // GET an Onshape endpoint, retrying on throttling. Returns the Response, or null on failure.
  async function get(url, accept) {
    for (let attempt = 0; attempt < 5; attempt++) {
      await sleep(PACE_MS);
      state.requests++;
      let r;
      try {
        // A request can hang forever; give each a minute.
        r = await fetch(url, { credentials: 'include', headers: { Accept: accept || JSON_ACCEPT }, signal: AbortSignal.timeout(60000) });
      } catch (e) {
        await log(`NET ${url} ${e}`);
        await sleep(2000 * (attempt + 1));
        continue;
      }
      if (r.status === 429 || r.status >= 500) {
        const wait = Number(r.headers.get('Retry-After')) * 1000 || 5000 * (attempt + 1);
        await log(`HTTP ${r.status} ${url}, retry in ${wait} ms`);
        await sleep(wait);
        continue;
      }
      if (!r.ok) {
        state.errors.push(`${r.status} ${url}`);
        await log(`HTTP ${r.status} ${url}`);
        return null;
      }
      return r;
    }
    state.errors.push(`gave up ${url}`);
    return null;
  }

  // Fetch `url` and store it at `path` unless it is already on disk. Returns parsed JSON for
  // JSON files (reading the stored copy on a rerun), else true/false.
  async function save(path, url, { binary = false, accept } = {}) {
    state.current = path;
    if (await have(path)) {
      state.skipped++;
      if (binary) return true;
      const r = await sink('raw', path);
      return r.ok ? parseJson(path, await r.text()) : null;
    }
    const r = await get(url, accept);
    if (!r) return binary ? false : null;
    let body;
    try {
      body = binary ? await r.arrayBuffer() : await r.text();
    } catch (e) {
      state.errors.push(`body ${url}: ${e}`);
      await log(`BODY ${url} ${e}`);
      return binary ? false : null;
    }
    const w = await sink('save', path, { method: 'POST', body });
    if (!w.ok) throw new Error(`sink refused ${path}: ${w.status}`);
    return binary ? true : parseJson(path, body);
  }

  // Some endpoints answer 200 with an empty body; keep going rather than lose the document.
  function parseJson(path, text) {
    try {
      return JSON.parse(text);
    } catch (e) {
      log(`BADJSON ${path} (${text.length} bytes)`);
      return null;
    }
  }

  const dwe = (d, w, e) => `d/${d}/w/${w}/e/${e}`;

  // What to fetch for each element type: [file name, url builder, options].
  const ELEMENT_FETCHES = {
    PARTSTUDIO: [
      ['features.json', (d, w, e) => `${API}/partstudios/${dwe(d, w, e)}/features`],
      // The same, with each query's entities as Onshape's topology ids (`geometryIds`): what
      // bodydetails.json lists, for queries the importer can't evaluate (an imported file's
      // entities by Onshape's own tags).
      // (Only the unversioned API fills in `geometryIds`; the numbered ones, v5 to v17, leave them out.)
      ['features-geometry.json', (d, w, e) => `/api/partstudios/${dwe(d, w, e)}/features?includeGeometryIds=true`],
      ['sketches.json', (d, w, e) => `${API}/partstudios/${dwe(d, w, e)}/sketches?includeGeometry=true`],
      // With surfaces: sheet bodies (a board's silkscreen and soldermask) are parts too.
      ['bodydetails.json', (d, w, e) => `${API}/partstudios/${dwe(d, w, e)}/bodydetails?includeSurfaces=true`],
      ['parts.json', (d, w, e) => `${API}/parts/${dwe(d, w, e)}?withThumbnails=false&includePropertyDefaults=true`],
      ['massproperties.json', (d, w, e) => `${API}/partstudios/${dwe(d, w, e)}/massproperties?massAsGroup=false`],
      ['metadata-parts.json', (d, w, e) => `${API}/metadata/${dwe(d, w, e)}/p?depth=1`],
      ['configuration.json', (d, w, e) => `${API}/elements/${dwe(d, w, e)}/configuration`],
      // The parasolid and stl exports redirect off cad.onshape.com, which a page fetch cannot
      // follow, so reference geometry comes from bodydetails and massproperties instead.
    ],
    ASSEMBLY: [
      ['definition.json', (d, w, e) => `${API}/assemblies/${dwe(d, w, e)}?includeMateFeatures=true&includeMateConnectors=true&includeNonSolids=true`],
      ['features.json', (d, w, e) => `${API}/assemblies/${dwe(d, w, e)}/features`],
      ['configuration.json', (d, w, e) => `${API}/elements/${dwe(d, w, e)}/configuration`],
    ],
    FEATURESTUDIO: [
      ['featurestudio.json', (d, w, e) => `${API}/featurestudios/${dwe(d, w, e)}`],
    ],
    VARIABLESTUDIO: [
      ['variables.json', (d, w, e) => `${API}/variables/${dwe(d, w, e)}/variables`],
    ],
    BLOB: [
      ['blob.bin', (d, w, e) => `${API}/blobelements/${dwe(d, w, e)}`, { binary: true, accept: '*/*' }],
    ],
  };

  async function scrapeDocument(doc) {
    const d = doc.id;
    const w = doc.defaultWorkspace && doc.defaultWorkspace.id;
    await save(`${d}/document.json`, `${API}/documents/${d}`);
    await save(`${d}/workspaces.json`, `${API}/documents/d/${d}/workspaces`);
    await save(`${d}/versions.json`, `${API}/documents/d/${d}/versions`);
    if (!w) { await log(`no default workspace for ${d}`); return; }
    await save(`${d}/history.json`, `${API}/documents/d/${d}/w/${w}/documenthistory`);
    const refs = await save(`${d}/externalreferences.json`, `${API}/documents/d/${d}/w/${w}/externalreferences`);
    // The versions of other documents its elements reference (a Derived feature pins one): their
    // Part Studios' feature lists then, under the referenced document's folder, so its import
    // can keep those versions in its history.
    for (const list of Object.values((refs && refs.elementExternalReferences) || {})) {
      for (const r of list || []) {
        if (r.type !== 'version' || !r.documentId || !r.id) continue;
        for (const e of r.referencedElements || []) {
          await save(`${r.documentId}/versions/${r.id}/${e}/features.json`, `${API}/partstudios/d/${r.documentId}/v/${r.id}/e/${e}/features`);
        }
      }
    }
    await save(`${d}/thumbnail.png`, `${API}/thumbnails/d/${d}/w/${w}/s/300x170`, { binary: true, accept: 'image/png' });
    const elements = await save(`${d}/elements.json`, `${API}/documents/d/${d}/w/${w}/elements`);
    for (const el of elements || []) {
      for (const [file, url, opts] of ELEMENT_FETCHES[el.elementType] || []) {
        await save(`${d}/${el.id}/${file}`, url(d, w, el.id), opts);
      }
    }
  }

  async function listDocuments() {
    const docs = [];
    for (let offset = 0; ; offset += 20) {
      const page = await save(`_index/documents-${String(offset).padStart(5, '0')}.json`,
        `${API}/documents?filter=0&limit=20&offset=${offset}&sortColumn=createdAt&sortOrder=asc`);
      if (!page) break;
      docs.push(...page.items);
      if (!page.next || page.items.length === 0) break;
    }
    return docs;
  }

  // The API version Onshape marks current; the default above if it can't say.
  async function useCurrentApi() {
    try {
      const r = await fetch('/api/versions', { credentials: 'include', headers: { Accept: JSON_ACCEPT } });
      const current = r.ok && ((await r.json()).availableVersions || []).find(v => v.current);
      if (current && /^v\d+$/.test(current.urlSafeName)) API = `/api/${current.urlSafeName}`;
    } catch (e) { /* keep the default */ }
    await log(`api ${API}`);
  }

  async function start({ only } = {}) {
    if (state.phase === 'running') return 'already running';
    state.phase = 'running';
    state.errors = [];
    (async () => {
      try {
        if ((await (await sink('ping')).text()) !== 'pong') throw new Error('receiver not answering');
        await useCurrentApi();
        await log('scrape start');
        let docs = await listDocuments();
        if (only) docs = docs.filter(x => only.includes(x.id));
        state.docsTotal = docs.length;
        for (const doc of docs) {
          state.current = doc.name;
          try { await scrapeDocument(doc); } catch (e) { state.errors.push(`${doc.id} ${e}`); await log(`ERR ${doc.id} ${e}`); }
          state.docsDone++;
          if (state.phase === 'stopping') break;
        }
        await log(`scrape end: ${state.docsDone}/${state.docsTotal} docs, ${state.requests} requests, ${state.errors.length} errors`);
        state.phase = 'done';
      } catch (e) {
        state.errors.push(String(e));
        state.phase = 'failed';
      }
    })();
    return 'started';
  }

  window.cadrsScrape = { state, start, stop: () => { state.phase = 'stopping'; } };
})();
