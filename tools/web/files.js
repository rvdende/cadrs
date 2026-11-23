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

// Files dragged onto the page from the computer. Without this the browser opens them in place of
// the app. Each drop is read and queued as one `{ files: [[name, Uint8Array]], x, y }` (where on
// `canvas`, in CSS pixels) on `globalThis.cadrsDrops`, which the app empties every frame
// (`cadrs_app::file_drop`) and treats as files dropped on its window. `globalThis.cadrsDragging`
// says whether files are being dragged over the page, for the app to show where they can go.
export function listenForDrops(canvas) {
  globalThis.cadrsDrops ??= [];
  globalThis.cadrsDragging = false;
  const files = (e) => [...(e.dataTransfer?.items ?? [])].some((i) => i.kind === "file");
  // Every element the drag enters and leaves counts, so it is off the page once they balance.
  // A browser sends `dragover` every few hundred milliseconds while a drag is over the page,
  // even a still one: without one for a second, it is gone too (a `dragleave` can go missing).
  let entered = 0;
  let lost = 0;
  const dragging = (on) => {
    globalThis.cadrsDragging = on;
    clearTimeout(lost);
    if (on) lost = setTimeout(() => dragging(false), 1000);
    else entered = 0;
  };
  addEventListener("dragenter", (e) => {
    if (!files(e)) return;
    entered += 1;
    dragging(true);
  });
  addEventListener("dragleave", (e) => {
    if (!files(e)) return;
    entered -= 1;
    if (entered <= 0) dragging(false);
  });
  addEventListener("dragover", (e) => {
    if (!files(e)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    dragging(true);
  });
  addEventListener("drop", async (e) => {
    if (!files(e)) return;
    e.preventDefault();
    dragging(false);
    const rect = canvas.getBoundingClientRect();
    const [x, y] = [e.clientX - rect.left, e.clientY - rect.top];
    const dropped = [...e.dataTransfer.files];
    try {
      const read = await Promise.all(dropped.map(async (f) => [f.name, new Uint8Array(await f.arrayBuffer())]));
      if (read.length) globalThis.cadrsDrops.push({ files: read, x, y });
    } catch (err) {
      console.error("cadrs: cannot read the dropped files:", err);
    }
  });
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

export async function openFiles() {
  let db = null;
  // The bundle's files first: the ones kept in IndexedDB replace them.
  const shipped = bundle();
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
