// Minimal markdown to HTML for the reports: headings, tables, lists, paragraphs, `code`, _italic_.
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const inline = (s) => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/(^|\s)_([^_]+)_(?=\s|$|[.,])/g, '$1<em>$2</em>');
const cells = (l) => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());

function toHtml(md, title = 'Report') {
  const out = [], lines = md.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    let m;
    if ((m = /^(#{1,4})\s+(.*)/.exec(l))) out.push(`<h${m[1].length}>${inline(m[2])}</h${m[1].length}>`);
    else if (l.startsWith('|')) {
      const head = cells(l);
      i += 1; // separator row
      const rows = [];
      while (lines[i + 1] && lines[i + 1].startsWith('|')) rows.push(cells(lines[++i]));
      out.push('<table><thead><tr>' + head.map((c) => `<th>${inline(c)}</th>`).join('') + '</tr></thead><tbody>' +
        rows.map((r) => '<tr>' + r.map((c) => `<td>${inline(c)}</td>`).join('') + '</tr>').join('') + '</tbody></table>');
    } else if (/^\s*-\s+/.test(l)) {
      const items = [l];
      while (/^\s*-\s+/.test(lines[i + 1] || '')) items.push(lines[++i]);
      out.push('<ul>' + items.map((x) => `<li>${inline(x.replace(/^\s*-\s+/, ''))}</li>`).join('') + '</ul>');
    } else if (l.trim()) out.push(`<p>${inline(l)}</p>`);
  }
  return `<!doctype html><meta charset="utf-8"><title>${esc(title)}</title><style>
body{font:12px/1.45 sans-serif;margin:24px;color:#111}table{border-collapse:collapse;margin:8px 0;width:100%}
th,td{border:1px solid #bbb;padding:3px 6px;text-align:left;vertical-align:top;word-break:break-word}th{background:#eee}
code{background:#f2f2f2;padding:0 3px}h1{font-size:20px}h2{font-size:16px;margin-top:18px}</style>${out.join('\n')}`;
}

module.exports = { toHtml };
