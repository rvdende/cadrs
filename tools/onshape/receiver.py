#!/usr/bin/env python3
"""Local sink for the in-browser Onshape scraper (tools/scrape.js).

The scraper runs inside a logged-in cad.onshape.com tab and POSTs each response here:

    POST /save?path=<relative path>   body = raw bytes, written to raw/<relative path>
    GET  /have?path=<relative path>   200 if the file already exists (lets a rerun resume)
    GET  /raw?path=<relative path>    a stored file back (a rerun reads listings it already has)
    GET  /scrape.js                   the scraper itself, so the tab always runs the latest copy
    POST /log                         body appended to scrape.log

Listens on 127.0.0.1 only. Data goes to `--root` (default `~/work/cadrs_onshape`):

    python3 tools/onshape/receiver.py [--root <dir>] [port]
"""
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

HERE = os.path.dirname(os.path.abspath(__file__))
ARGS = sys.argv[1:]
ROOT = os.path.expanduser(ARGS[ARGS.index("--root") + 1]) if "--root" in ARGS else os.path.expanduser("~/work/cadrs_onshape")
RAW = os.path.join(ROOT, "raw")
PORT = next((int(a) for a in ARGS if a.isdigit()), 8765)
ALLOWED_ORIGINS = {"https://cad.onshape.com"}


def safe_path(rel):
    rel = rel.lstrip("/")
    full = os.path.normpath(os.path.join(RAW, rel))
    if not full.startswith(RAW + os.sep):
        raise ValueError("path escapes raw/")
    return full


class Handler(BaseHTTPRequestHandler):
    def cors(self):
        origin = self.headers.get("Origin", "")
        if origin in ALLOWED_ORIGINS:
            self.send_header("Access-Control-Allow-Origin", origin)
            self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            self.send_header("Access-Control-Allow-Headers", "Content-Type")
            self.send_header("Access-Control-Allow-Private-Network", "true")

    def reply(self, code, body=b""):
        self.send_response(code)
        self.cors()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.reply(204)

    def do_GET(self):
        url = urlparse(self.path)
        if url.path == "/have":
            try:
                p = safe_path(parse_qs(url.query)["path"][0])
            except (KeyError, ValueError):
                return self.reply(400)
            return self.reply(200 if os.path.exists(p) else 404)
        if url.path == "/raw":
            try:
                p = safe_path(parse_qs(url.query)["path"][0])
            except (KeyError, ValueError):
                return self.reply(400)
            if not os.path.isfile(p):
                return self.reply(404)
            with open(p, "rb") as f:
                return self.reply(200, f.read())
        if url.path == "/ping":
            return self.reply(200, b"pong")
        if url.path == "/scrape.js":
            with open(os.path.join(HERE, "scrape.js"), "rb") as f:
                return self.reply(200, f.read())
        self.reply(404)

    def do_POST(self):
        if self.headers.get("Origin", "") not in ALLOWED_ORIGINS:
            return self.reply(403)
        url = urlparse(self.path)
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if url.path == "/log":
            with open(os.path.join(ROOT, "scrape.log"), "ab") as f:
                f.write(body + b"\n")
            return self.reply(200)
        if url.path == "/save":
            try:
                p = safe_path(parse_qs(url.query)["path"][0])
            except (KeyError, ValueError):
                return self.reply(400)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            tmp = p + ".part"
            with open(tmp, "wb") as f:
                f.write(body)
            os.replace(tmp, p)
            return self.reply(200)
        self.reply(404)

    def log_message(self, fmt, *args):
        pass


if __name__ == "__main__":
    os.makedirs(RAW, exist_ok=True)
    print(f"receiver on 127.0.0.1:{PORT}, writing to {RAW}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
