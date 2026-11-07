# Direct LIVE connection API

`ttl-live-api` is a short-lived connection-ticket broker. Its only public operation is
`POST /v1/connect` with `{ "uniqueId": "@creator" }`.

The broker normalizes the handle, resolves the current room with `ttl-live-discovery`, caches the
result briefly, coalesces simultaneous lookups for the same creator, and asks the warm embedded
signer for a fresh direct WebSocket descriptor. It never opens or owns a persistent TikTok LIVE
socket. The Node SDK uses the returned Cookie/User-Agent metadata to connect directly to TikTok.

## Run

The binary takes CLI flags, with environment variables as fallback and built-in defaults last
(flags > env > defaults). The bundle defaults to `./webmssdk.js`; relative paths resolve
against the current working directory:

```powershell
# easiest for local debugging — no env setup
cargo run -p ttl-live-api -- --bundle ./webmssdk.js --bind 127.0.0.1:8080
cargo run -p ttl-live-api -- --help
```

The env equivalents are `SIGNING_BUNDLE` (or `TTL_BUNDLE`), `HTTP_BIND` (or `TTL_BIND`),
and `API_KEYS`. The broker binds `127.0.0.1:8080` unless told otherwise; binding a
non-loopback address without `API_KEYS` prints a startup warning, since anyone who can reach
the port could mint tickets.
An existing session can be supplied with
`TTL_SESSION_COOKIE` or `TTL_SESSION_FILE`; otherwise the service bootstraps one anonymous web
identity at startup.

```powershell
$env:SIGNING_BUNDLE = "C:\path\to\webmssdk.js"
cargo run -p ttl-live-api
```

The embedded runtime is QuickJS, the only engine in the repository. Tune bind address, cache TTLs, semaphores, timeouts, and
quotas with the environment variables documented by `AppConfig::from_env` in
`crates/ttl-live-api/src/config.rs`.

With `API_KEYS` configured as `secret=customer`, clients authenticate with a bearer token:

```powershell
curl.exe -X POST http://127.0.0.1:8080/v1/connect `
  -H "Authorization: Bearer secret" `
  -H "Content-Type: application/json" `
  -d '{"uniqueId":"@creator"}'
```

`/healthz`, `/readyz`, and `/metrics` are available for service operation. `readyz` reflects
successful startup, not ongoing signer health — runtime degradation surfaces per request as
`SIGNER_UNAVAILABLE`/`SIGN_FAILED`. Signed URLs, cookies,
signatures, and API keys are not written to structured logs.

## SDK

```powershell
cd packages/live-sdk
npm install
npm test
```

The SDK package is Node.js-first because the direct handshake needs Cookie and User-Agent headers.
It accepts only a unique ID, reuses the repository's generated protobufs and frame helpers, ACKs
push batches before emitting normalized events, and obtains a fresh broker descriptor for every
reconnect.

## Browser check (Bun, HTML only)

`examples/live-sdk-bun/` is a single page served directly by Bun with no JS server: the only
backend is this Rust broker. The page reads the unique ID from its own query string, fetches a
ticket from `POST /v1/connect`, then opens the broker's WebSocket relay at
`GET /v1/live?uniqueId=` and reports everything with `console.log`/`console.error` only:

```powershell
bun install --cwd examples/live-sdk-bun
bun ./examples/live-sdk-bun/index.html --console
# open http://localhost:3000/?uniqueId=@creator
```

`--console` mirrors the browser logs into the terminal. A page socket cannot set the
Cookie/User-Agent headers TikTok demands (a direct browser socket dies with an immediate
1006), so `/v1/live` dials TikTok with the ticket headers and pipes raw frames both ways —
the page still owns the protocol (enter-room, heartbeats, ACKs, decode). Offline creators
answer the relay with the same JSON contract as `/v1/connect`, so no socket is opened for
them. The broker stamps permissive CORS headers so the page can reach it cross-origin; it
binds loopback by default and should not be exposed publicly with those headers.
