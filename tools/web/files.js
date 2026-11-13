// The page's files for cadrs (`cadrs_fs::web`): every file and folder the app writes, kept in
// IndexedDB as `path → { data: Uint8Array | null (a folder), mtime }`. `openFiles` reads them
// all before the app starts; the app then hands over each change, written behind in batches.
//
// The files cadrs ships with (the built-in libraries, the samples) come from `bundle.tar.gz`
// beside the page, unpacked under `/` and never written to IndexedDB unless the app changes them.
//
// Without IndexedDB (a private window may have none) the files last as long as the tab.

const DB = "cadrs";
const STORE = "files";

function request(r) {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function openDb() {
  const open = indexedDB.open(DB, 1);
  open.onupgradeneeded = () => open.result.createObjectStore(STORE);
  return request(open);
}

// Hands the browser `data` as a download called `name`.
function download(name, data) {
  const url = URL.createObjectURL(new Blob([data]));
  const a = Object.assign(document.createElement("a"), { href: url, download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

// The files of a ustar archive: `[path, Uint8Array, mtime]`, and their folders (`null`).
function untar(tar) {
  const text = (a, b) => new TextDecoder().decode(tar.subarray(a, b)).replace(/\0.*$/s, "");
  const files = [];
  const dirs = new Set();
  for (let at = 0; at + 512 <= tar.length; ) {
    const name = text(at, at + 100);
    if (!name) break;
    const prefix = text(at + 345, at + 500);
    const size = parseInt(text(at + 124, at + 136).trim() || "0", 8);
    const path = "/" + (prefix ? `${prefix}/${name}` : name).replace(/\/$/, "");
    const type = text(at + 156, at + 157);
    if (type === "0" || type === "") files.push([path, tar.slice(at + 512, at + 512 + size), 0]);
    for (let d = path.lastIndexOf("/"); d > 0; d = path.lastIndexOf("/", d - 1)) dirs.add(path.slice(0, d));
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return [...[...dirs].map((d) => [d, null, 0]), ...files];
}

async function bundle() {
  try {
    const r = await fetch(new URL("bundle.tar.gz", import.meta.url));
    if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
    const tar = r.body.pipeThrough(new DecompressionStream("gzip"));
    return untar(new Uint8Array(await new Response(tar).arrayBuffer()));
  } catch (e) {
    console.warn("cadrs: no bundled libraries and samples:", e);
    return [];
  }
}

// `bundled: false` skips the libraries and samples (the phone page has no use for them).
export async function openFiles({ bundled = true } = {}) {
  let db = null;
  // The bundle's files first: the ones kept in IndexedDB replace them.
  const shipped = bundled ? bundle() : Promise.resolve([]);
  let loaded = [];
  try {
    db = await openDb();
    const tx = db.transaction(STORE, "readonly");
    const [keys, values] = await Promise.all([
      request(tx.objectStore(STORE).getAllKeys()),
      request(tx.objectStore(STORE).getAll()),
    ]);
    loaded = keys.map((k, i) => [k, values[i].data, values[i].mtime]);
    // Ask the browser not to clear them when the disk fills up.
    navigator.storage?.persist?.();
  } catch (e) {
    console.warn("cadrs: no IndexedDB, documents last as long as this tab:", e);
  }
  loaded = [...(await shipped), ...loaded];

  // Changes since the last write: path → value (undefined: removed).
  let queued = new Map();
  let scheduled = false;
  function flush() {
    scheduled = false;
    if (!db || !queued.size) return;
    const tx = db.transaction(STORE, "readwrite");
    const store = tx.objectStore(STORE);
    for (const [path, value] of queued) {
      if (value === undefined) store.delete(path);
      else store.put(value, path);
    }
    tx.onerror = () => console.error("cadrs: cannot keep files:", tx.error);
    queued = new Map();
  }
  function queue(path, value) {
    queued.set(path, value);
    if (!scheduled) {
      scheduled = true;
      setTimeout(flush, 0);
    }
  }
  addEventListener("pagehide", flush);

  return {
    loaded,
    put: (path, data, mtime) => queue(path, { data, mtime }),
    remove: (path) => queue(path, undefined),
    download,
  };
}
