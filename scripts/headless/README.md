# Headless signer probes

Research tooling that runs the real `webmssdk` bundle **without a browser**, against a synthetic
environment shim, with the transport stubbed. Nothing is sent and no signed value is printed —
only parameter names and byte lengths.

These probes answered the feasibility question behind Track A in
[`docs/11-webview-removal.md`](../../docs/11-webview-removal.md): the bundle loads, exposes the
identical SDK surface, and produces the complete fetch suffix outside a browser.

## Getting the bundle

The bundle is a public static asset. It is **not committed** — third-party source stays out of the
repository. Fetch it and verify it against the pinned digest:

```sh
curl -s -o /tmp/webmssdk.js \
  https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
sha256sum /tmp/webmssdk.js   # must match fixtures/research/webmssdk-profile-2026-08-13.json
```

A digest mismatch means the bundle drifted; re-run the probes before trusting any prior finding.

## Probes

```sh
# Which SDK functions load, and what each signing route produces.
node scripts/headless/sign-probe.mjs /tmp/webmssdk.js        # no stored token
node scripts/headless/sign-probe.mjs /tmp/webmssdk.js 124    # with a 124-byte synthetic token

# Regenerate the committed environment surface.
node scripts/headless/emit-surface.mjs /tmp/webmssdk.js \
  fixtures/research/environment-surface-v1.json
```

`shim.mjs` is the synthetic browser: a `with`-scoped `Proxy` whose `has` trap returns true, so
every free identifier the bundle resolves is recorded by path. Unknown properties return
`undefined` and are recorded as missing — that list is what a shim has to grow to satisfy.

## What the probes establish

- The bundle evaluates with no browser and exposes all nine `byted_acrawler` functions.
- `frontierSign` returns a 16-byte `X-Bogus`, matching the oracle's recorded shape.
- The patched `fetch` appends `X-Dynosaur → msToken → X-Bogus → X-Gnarly`, in that order, with
  `X-Bogus=1` — matching the oracle.
- The patch is gated on `window._mssdk._enablePathListRegex`, built from `init`'s `enablePathList`.
  With no matching path, `fetch` is patched but appends nothing.
- `msToken` is a **verbatim passthrough** of `localStorage['xmst']`: a stored token of length *n*
  produces `msToken` of length *n*. It is not computed.

The environment shim is deliberately deterministic — `crypto.getRandomValues` returns a fixed
sequence — so repeated runs are comparable. Real signing needs real entropy.

## Live verification (authorized use only)

`native-check.mjs` walks the same flow as `cargo run -p ttl-live-discovery --example live-check`,
at the HTTP level. Unlike the other probes it **sends real signed requests**, so point it only at a
room you are authorized to test:

```sh
node scripts/headless/native-check.mjs /tmp/webmssdk.js <unique_id>
```

It prints status codes, byte counts, and parameter names only — never a signed URL, cookie value,
or token.

Measured on 2026-08-18 against a live room:

- `unique_id` → `room_id`: **200**, no signature needed.
- `GET /@user/live`: issues the guest identity cookies. No browser required.
- `/webcast/room/info/`: **200, `status_code=0`**, real viewer counts.
- `/webcast/gift/list/`: **200, 673 gifts** — identical to the WebView run.
- `/webcast/im/fetch/`: **403**, even holding the full guest identity and a service-issued token.

The first `im/fetch` rejection is still useful: the response issues a 124-byte `msToken`, which is
how a fresh client obtains one.

Because `room/info` and `gift/list` succeed with the same machinery, the signature is demonstrably
valid and `im/fetch` is gated on the account session. See `docs/11-webview-removal.md`.

## Finding live channels

`find-live.mjs` lists channels that are live right now, without a browser:

```sh
node scripts/headless/find-live.mjs /tmp/webmssdk.js          # keyword defaults to "live"
node scripts/headless/find-live.mjs /tmp/webmssdk.js music 20
```

```text
10 live now (keyword "live")

  viewers  room_id              user
     2336  7675315929085037333  @josecompartiedopalabra
           Oración de fe 🙏
     1933  7675333220732275457  @_elizaayala
```

This replaces the last renderer-bound capability. The WebView reads live channels out of the
rendered `/live` DOM because that page ships no channel data in its HTML — but
`/api/search/live/full/` returns the same rooms as JSON, and the headless signer can sign it. The
response carries the room id, viewer count, title, and owner directly, so no follow-up lookup is
needed.

Rows are ready to feed straight into the other probes:

```sh
node scripts/headless/native-check.mjs /tmp/webmssdk.js josecompartiedopalabra
```

Note that `/webcast/feed/` — the endpoint the `/live` page itself uses — answers `200` with an
empty body for a guest identity, both on `webcast.tiktok.com` and `webcast.us.tiktok.com`, even
when the issued `x-ms-token` is fed back. The search endpoint is the working route.

## Transport bootstrap

`transport.mjs` signs `/webcast/im/fetch/` and reads the push_server out of the response:

```sh
node scripts/headless/transport.mjs /tmp/webmssdk.js <unique_id>
```

```text
session: 9 cookies, authenticated=true
room_id=7675336599840393991
sup_ws_ds_opt=0: http=200 bytes=74670 PUSH_SERVER
  push_server: wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/
```

It requires an **authenticated account session** — a guest gets an empty 200, and
`/webcast/room/ping/audience/` reports `"User doesn't login"`. The session comes from the same
file the WebView uses (`TTL_SESSION_FILE`, else `~/.config/ttl-signer/session`). Cookie values are
never printed.

The endpoint often answers 200 with an empty body. When it does, the WebView oracle returns
`Rejected(EmptyBody)` for the same room at the same time — the two paths succeed together and fail
together, so that was upstream behaviour rather than a bug here.

That comparison is no longer runnable: the WebView oracle has been deleted. It is recorded in
`docs/11-webview-removal.md`, which is now the only evidence that the two paths agreed.

## Signing one URL (subprocess protocol)

`sign-url.mjs` signs a single URL and prints it on stdout, so Rust can reach the signer without
embedding a JavaScript engine:

```sh
node scripts/headless/sign-url.mjs /tmp/webmssdk.js "<url>" fetch      # room/info, gift/list
node scripts/headless/sign-url.mjs /tmp/webmssdk.js "<url>" frontier   # im/fetch
```

The product argument is explicit because the two are **not** interchangeable — see
`im-fetch-probe.mjs`. Credentials travel in the environment (`TTL_COOKIE`, `TTL_XMST`,
`TTL_USER_AGENT`) so they never appear in the process table.

stdout carries the signed URL and nothing else: the bundle prints while it loads, so its console
is redirected to stderr. `ttl_live_discovery::CommandSigner` drives this, and
`cargo run -p ttl-live-discovery --example discover -- <unique_id>` is the end-to-end path from
Rust with no browser.

## Transport reverse engineering

`xhr-transport.mjs` drives `/webcast/im/fetch/` the way the live player does — over a real
`XMLHttpRequest` with `withCredentials`, not `fetch`:

```sh
node scripts/headless/xhr-transport.mjs /tmp/webmssdk.js <unique_id>
```

The player's transport client (`static/js/async/9894.*.js`, reachable from the room page's webpack
manifest) uses XHR exclusively, and webmssdk hooks XHR and `fetch` on separate paths — `XHRSignTime`
and `fetchSignTime` are distinct fields in its state. The sandbox therefore needs a working
`XMLHttpRequest`, including the whole event-handler surface: the SDK reassigns `onabort` and friends
while wrapping `send`, and a missing slot fails as "cannot read properties of undefined".

Both routes currently end in 403, and the frontierSign route in an empty 200. The open lead is that
our computed signatures are shorter than the oracle's — `X-Gnarly` 324 against a recorded 332 — which
points at the signing input rather than the algorithm. See `docs/12-transport-reverse-engineering.md`
for the plan.

## Canonical input convergence

`canonical-input.mjs` measures what the VM receives at each signing route and prints it beside the
oracle's recorded value:

```sh
TTL_URL="$(cargo run -q -p ttl-sign-core --example print-fetch-url -- <room_id>)" \
  node scripts/headless/canonical-input.mjs /tmp/webmssdk.js 124
```

```text
route              entry   input(ours)  input(oracle)   delta   output(ours)  output(oracle)
X-Gnarly           48886          1278           1274      +4            332             332
X-Dynosaur         55188             4              4      +0            392             388
fetch composition  58628           786            786      +0              -               1
```

Two facts it established: the canonical string tracks the **query** one byte for one byte, and
`document.cookie` does not enter it at all. Use the project's `FetchParams` query rather than a
hand-written one — that difference alone was 82 bytes.

`TTL_ENV` overrides shim properties as JSON (`{"navigator.platform":"Win32"}`) for bisection,
`TTL_PAD` appends a known-length parameter, and `TTL_NO_WEBGL` restores the old empty-canvas
behaviour that made the signatures short.
