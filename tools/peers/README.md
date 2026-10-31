# peers.cadrs.net

The public documents rendezvous: a Cloudflare Worker that keeps a short list of the cadrs that
are online, so one coming online has peers to join the public room through
(`cadrs_net::public`). It stores only cadrs IDs and when each last checked in: no documents,
listings, stars, names or addresses. Peers find each other's addresses by ID through iroh.

- `POST /v1/here` `{id, time, signature}`: a check-in. `id` is the cadrs ID (the hex Ed25519
  public key), `time` Unix seconds (within five minutes of the Worker's clock), `signature` the
  hex Ed25519 signature of `cadrs rendezvous 1\0<id>\0<time>` by that key. The answer is
  `{peers: [id, …]}`: up to 20 others seen in the last 15 minutes, at random.
- `GET /v1/peers`: the same list, without checking in.

cadrs checks in every ten minutes while it takes part in public documents (Preferences ›
Collaboration). `CADRS_RENDEZVOUS=<url>` points it elsewhere, `CADRS_RENDEZVOUS=0` turns it off.
Without the Worker, peers still start from the seeds, their contacts and the peers they met
before.

One Durable Object (SQLite) holds the list. A check-in every ten minutes is about 150 requests
a day per online user: the free plan's 100,000 a day covers several hundred.

## Run it locally

```sh
npx -y wrangler@4 dev --port 8787 --local
node test.mjs                        # the check-ins it takes and refuses
CADRS_RENDEZVOUS=http://127.0.0.1:8787 cargo run -r -- --public-seed --data-dir /tmp/seed-a
```

## Deploy

Needs a Cloudflare API token that may edit Workers scripts and Durable Objects on the account,
and add the custom domain on the `cadrs.net` zone:

```sh
CLOUDFLARE_API_TOKEN=… CLOUDFLARE_ACCOUNT_ID=… npx -y wrangler@4 deploy
node test.mjs https://peers.cadrs.net
```
