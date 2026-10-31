// Checks peers.cadrs.net (or `wrangler dev`): `node test.mjs [base URL]`.
// Two made-up peers check in and each is told of the other; a stale check-in, one for someone
// else's ID and one with a bad signature are refused.

const base = process.argv[2] ?? "http://127.0.0.1:8787";
const enc = new TextEncoder();
const hex = (b) => Buffer.from(b).toString("hex");

async function peer() {
  const keys = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const id = hex(await crypto.subtle.exportKey("raw", keys.publicKey));
  const sign = async (who, time) =>
    hex(
      await crypto.subtle.sign(
        { name: "Ed25519" },
        keys.privateKey,
        enc.encode(`cadrs rendezvous 1\0${who}\0${time}`),
      ),
    );
  return { id, sign };
}

async function here(body) {
  const r = await fetch(`${base}/v1/here`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return { status: r.status, body: await r.json() };
}

function check(what, ok) {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) process.exitCode = 1;
}

const now = Math.floor(Date.now() / 1000);
const [a, b] = [await peer(), await peer()];
const first = await here({ id: a.id, time: now, signature: await a.sign(a.id, now) });
check("a checks in", first.status === 200 && !first.body.peers.includes(a.id));
const second = await here({ id: b.id, time: now, signature: await b.sign(b.id, now) });
check("b checks in and is told of a", second.status === 200 && second.body.peers.includes(a.id));
const listed = await (await fetch(`${base}/v1/peers`)).json();
check("both are listed", listed.peers.includes(a.id) && listed.peers.includes(b.id));
const stale = await here({ id: a.id, time: now - 3600, signature: await a.sign(a.id, now - 3600) });
check("a stale check-in is refused", stale.status === 400);
const forged = await here({ id: b.id, time: now, signature: await a.sign(b.id, now) });
check("someone else's ID is refused", forged.status === 400);
const bent = await here({ id: a.id, time: now + 1, signature: await a.sign(a.id, now) });
check("a signature for another time is refused", bent.status === 400);
const cors = await fetch(`${base}/v1/peers`, { method: "OPTIONS" });
check("a page may ask (CORS)", cors.headers.get("access-control-allow-origin") === "*");
