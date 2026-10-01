// The instance folders of the Onshape assembly open in this tab, as JSON. Onshape's REST API
// doesn't return them (neither `definition` nor `features` lists ASSEMBLY_FOLDER features), so
// this reads the web app's instance list: it opens every folder, then nests the rows by their
// indent. Run it in the tab's console (or through the browser extension) and save the result as
// `<raw>/<did>/<eid>/folders.json`:
//
//   {"folders": [{"id": "F/T8Bc…", "name": "pi pico", "parent": null, "members": ["MQiH…", …]}]}
//
// `members` are the folder's direct children: instance ids (as in definition.json) and folder ids.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const rows = () => [...document.querySelectorAll('#assembly-tree .os-list-item')];
  const x = (e) => e.querySelector('.os-list-item-name')?.getBoundingClientRect().x ?? 0;
  const isFolder = (e) => e.getAttribute('feature-type') === 'ASSEMBLY_FOLDER' && e.getAttribute('data-id') !== 'rootMateItem';
  const click = (el) => {
    const r = el.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, button: 0 };
    for (const t of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
      el.dispatchEvent(new (t.startsWith('pointer') ? PointerEvent : MouseEvent)(t, o));
    }
  };
  // Open closed folders (a folder is open when the next row is indented under it) until none is.
  for (let pass = 0; pass < 20; pass++) {
    const rs = rows();
    const closed = rs.filter((e, i) => isFolder(e) && !(rs[i + 1] && x(rs[i + 1]) > x(e) + 5));
    const toOpen = closed.filter((e) => e.querySelector('.node-expander'));
    if (!toOpen.length) break;
    toOpen.forEach((e) => click(e.querySelector('.node-expander')));
    await sleep(800);
  }
  const rs = rows();
  const folders = [];
  const stack = []; // open folders: {x, folder}
  for (const e of rs) {
    const id = e.getAttribute('data-id');
    if (id === 'rootMateItem') break; // the mate features follow the instances
    const ex = x(e);
    while (stack.length && ex <= stack[stack.length - 1].x + 5) stack.pop();
    const parent = stack.length ? stack[stack.length - 1].folder : null;
    if (parent && id !== 'Origin') parent.members.push(id);
    if (isFolder(e)) {
      const name = (e.querySelector('.os-list-item-name')?.textContent ?? '').trim().replace(/\s*\(\d+\)$/, '');
      const f = { id, name, parent: parent ? parent.id : null, members: [] };
      folders.push(f);
      stack.push({ x: ex, folder: f });
    }
  }
  return JSON.stringify({ folders });
})();
