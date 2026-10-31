// peers.cadrs.net: who is online in the public documents room, so a cadrs coming online has
// peers to start from (`cadrs_net::public`, the client side). It keeps only cadrs IDs and when
// each last checked in: no documents, listings, stars, names or addresses (iroh finds a peer by
// its ID).
//
// - POST /v1/here {id, time, signature}: a check-in, signed with the key whose public half is
//   the cadrs ID (Ed25519 over `cadrs rendezvous 1\0<id>\0<time>`, the time within five
//   minutes of ours), so nobody checks in someone else. Answers like GET, without the asker.
// - GET /v1/peers: {peers: [id, …]}, up to MAX of those seen in the last FRESH seconds, at random.
//
// One Durable Object (SQLite) holds the list.

/** How long a check-in counts: a cadrs checks in every ten minutes while online. */
const FRESH = 15 * 60;
/** How many peers an answer names. */
const MAX = 20;
/** How far a check-in's clock may be from ours. */
const SKEW = 5 * 60;
const CONTEXT = "cadrs rendezvous 1\0";

const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, POST, OPTIONS",
  "access-control-allow-headers": "content-type",
  "access-control-max-age": "86400",
};

export class Peers {
  constructor(state) {
    this.sql = state.storage.sql;
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS peers (id TEXT PRIMARY KEY, seen INTEGER NOT NULL)",
    );
  }

  async fetch(request) {
    const url = new URL(request.url);
    const now = Math.floor(Date.now() / 1000);
    this.sql.exec("DELETE FROM peers WHERE seen < ?", now - FRESH);
    const me = url.searchParams.get("id") ?? "";
    if (request.method === "POST" && me) {
      this.sql.exec(
        "INSERT INTO peers (id, seen) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET seen = excluded.seen",
        me,
        now,
      );
    }
    const peers = this.sql
      .exec("SELECT id FROM peers WHERE id != ? ORDER BY random() LIMIT ?", me, MAX)
      .toArray()
      .map((r) => r.id);
    return Response.json({ peers });
  }
}

const hex = (s, bytes) =>
  typeof s === "string" && s.length === bytes * 2 && /^[0-9a-f]+$/.test(s)
    ? Uint8Array.from(s.match(/../g), (b) => parseInt(b, 16))
    : null;

const reply = (body, status = 200) =>
  Response.json(body, { status, headers: CORS });

/** The check-in's cadrs ID, if its signature and time hold. */
async function checkIn(request) {
  const text = await request.text();
  if (text.length > 1024) return null;
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return null;
  }
  const { id, time, signature } = body ?? {};
  const key = hex(id, 32);
  const sig = hex(signature, 64);
  const now = Math.floor(Date.now() / 1000);
  if (!key || !sig || !Number.isSafeInteger(time) || Math.abs(now - time) > SKEW) return null;
  try {
    const pub = await crypto.subtle.importKey("raw", key, { name: "Ed25519" }, false, [
      "verify",
    ]);
    const message = new TextEncoder().encode(`${CONTEXT}${id}\0${time}`);
    return (await crypto.subtle.verify({ name: "Ed25519" }, pub, sig, message)) ? id : null;
  } catch {
    return null;
  }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") return new Response(null, { headers: CORS });
    const room = env.PEERS.get(env.PEERS.idFromName("public"));
    if (url.pathname === "/v1/here" && request.method === "POST") {
      const id = await checkIn(request);
      if (!id) return reply({ error: "not a valid check-in" }, 400);
      const answer = await room.fetch(`https://peers/here?id=${id}`, { method: "POST" });
      return reply(await answer.json());
    }
    if (url.pathname === "/v1/peers" && request.method === "GET") {
      const answer = await room.fetch("https://peers/peers");
      return reply(await answer.json());
    }
    return reply({ error: "not found" }, 404);
  },
};
