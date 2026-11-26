// Node port of tools/onshape/receiver.py: the sink scrape.js POSTs each Onshape response to.
const fs = require('fs');
const http = require('http');
const path = require('path');

const ALLOWED_ORIGINS = new Set(['https://cad.onshape.com']);

// Starts on a free loopback port; resolves to {port, close}.
function startReceiver(root) {
  const raw = path.join(root, 'raw');
  fs.mkdirSync(raw, { recursive: true });
  // Scrape logs live with the other logs, not in the data root.
  const logs = path.join(root, 'logs');
  fs.mkdirSync(logs, { recursive: true });
  const safe = (rel) => {
    const full = path.normalize(path.join(raw, rel.replace(/^\/+/, '')));
    if (!full.startsWith(raw + path.sep)) throw new Error('path escapes raw/');
    return full;
  };
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    const origin = req.headers.origin || '';
    const reply = (code, body = '') => {
      const h = { 'Content-Length': Buffer.byteLength(body) };
      if (ALLOWED_ORIGINS.has(origin)) {
        Object.assign(h, {
          'Access-Control-Allow-Origin': origin,
          'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
          'Access-Control-Allow-Headers': 'Content-Type',
          'Access-Control-Allow-Private-Network': 'true',
        });
      }
      res.writeHead(code, h);
      res.end(body);
    };
    const rel = () => url.searchParams.get('path');
    try {
      if (req.method === 'OPTIONS') return reply(204);
      if (req.method === 'GET') {
        if (url.pathname === '/ping') return reply(200, 'pong');
        if (url.pathname === '/have') return reply(fs.existsSync(safe(rel())) ? 200 : 404);
        if (url.pathname === '/raw') {
          const p = safe(rel());
          return fs.existsSync(p) ? reply(200, fs.readFileSync(p)) : reply(404);
        }
        return reply(404);
      }
      if (req.method === 'POST') {
        if (!ALLOWED_ORIGINS.has(origin)) return reply(403);
        const chunks = [];
        req.on('data', (c) => chunks.push(c));
        req.on('end', () => {
          const body = Buffer.concat(chunks);
          try {
            if (url.pathname === '/log') {
              fs.appendFileSync(path.join(logs, 'scrape.log'), body + '\n');
              return reply(200);
            }
            if (url.pathname === '/save') {
              const p = safe(rel());
              fs.mkdirSync(path.dirname(p), { recursive: true });
              fs.writeFileSync(p + '.part', body);
              fs.renameSync(p + '.part', p);
              return reply(200);
            }
          } catch (e) {
            return reply(400);
          }
          reply(404);
        });
        return;
      }
      reply(404);
    } catch (e) {
      reply(400);
    }
  });
  return new Promise((resolve) =>
    server.listen(0, '127.0.0.1', () => resolve({ port: server.address().port, close: () => server.close() })));
}

module.exports = { startReceiver };
