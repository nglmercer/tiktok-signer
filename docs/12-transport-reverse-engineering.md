# 12 — Getting the WebSocket URI from zero

Question this answers: how do we obtain a working `/webcast/im/ws_proxy/` URI with **no browser and
no devtools** — building it from first principles rather than copying one the player made?

Read [11](11-webview-removal.md) first for how the browser-free signer works and what it already
does.

## Answered, 2026-08-18: the client builds the URI itself

The socket URI is constructible from first principles, and `im/fetch` is not on the path at all.

The live room page configures its IM SDK like this — read from
`static/js/async/main___live-container/(anchorName).live/page.<hash>.js`:

```js
s.config({ host, socketHost: "wss://webcast-ws.tiktok.com", wsDirect: "1",
           fetchBeforeWsSuccess: "1", aid, appName, liveId, versionCode: "270000", ... })
```

and the SDK's `start()` branches on exactly that:

```js
this.isDirectSocket()
  ? ("1" === this._config.fetchBeforeWsSuccess && this._initialization.start({...i, fetchRule: 1}),
     s.createClient({...i}))
  : this._initialization.start({...i, fetchRule: 1})
```

When `wsDirect` is `"1"` and a `socketHost` is set, `createClient()` builds the URI itself:

```text
wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?<query>&X-Gnarly=<signature>
```

`<query>` is the SDK's own serialization of the config plus its browser block, and the signature is
`byted_acrawler.registerWsSigner()({ "X-MS-Q": <query>, "X-MS-STUB": "" })["X-Gnarly"]`. No
`push_server`, no `route_params`, no `wrss`/`imprp`. The `im/fetch` call still goes out under
`fetchBeforeWsSuccess`, but only as a best-effort first page of messages — nothing depends on its
answer, which is why it can return 200 with zero bytes to a request with nothing wrong with it.

Three properties of the query are load-bearing, because the signature covers its bytes verbatim:

- **Nothing is percent-encoded.** `browser_version` keeps its spaces and parentheses, `tz_name` its
  slash. Encoding them signs bytes the server never sees.
- **`version_code` appears twice**, `180800` then `270000`: the SDK's browser block supplies one
  under a snake_case key and the page config the other under a camelCase one, and the serializer
  emits both.
- **Order is the SDK's**, not alphabetical.

Verified against live rooms: the socket opens in about 1.3 s, and after the `im_enter_room`
`PushFrame` it pushes chat, gifts, likes and member events — 30 frames and 135 KB in the first 20
seconds. Both implementations do this now:

```sh
node scripts/headless/ws-direct.mjs /tmp/webmssdk.js <room_id> 20
cargo run -p ttl-live-discovery --example live-check
```

`DirectSocketParams` (`crates/ttl-sign-core/src/params.rs`) builds the query; a unit test pins its
bytes against `TTL_PRINT_QUERY=1 node scripts/headless/ws-direct.mjs`, so a change in the player's
serializer fails a test rather than a connection. `scripts/headless/room-page-scan.mjs` is what found
the trail: the room page's rehydration data carries the `live_im_sdk_socket_link` experiment and the
`imFrontier` host.

### Keeping it from silently rotting

None of the above is documented or versioned by TikTok, and each fact fails differently when it
moves: an encoding serializer makes every signature wrong while everything still looks healthy, a
changed query key gives a successful handshake with no frames, a moved proto field gives frames that
decode into nothing. All three are expensive to diagnose from the outside — that is most of what the
search below cost.

So the facts are read back out of the shipped app and pinned:

```sh
node scripts/headless/player-audit.mjs            # 55 facts vs fixtures/research/player-transport-v1.json
cargo test -p ttl-sign-core                        # the fixture vs DirectSocketParams
node scripts/headless/ws-direct.mjs /tmp/webmssdk.js <room_id> 20   # and the socket vs reality
```

The audit needs no room, no session and no bundle, so it can run on a schedule; the other two are the
static and live halves of the same check. Note that room pages now answer a bare client with a
1155-byte anti-bot shell, which is why the audit reads `https://www.tiktok.com/live` instead — the
same app bundle, no creator required.

### One unexplained acceptance, 2026-08-18

While re-testing the tooling, `im-fetch-bisect.mjs` recorded a single **ACCEPTED** row: the ordinary
signed `baseline` variant, answered 200 with 120,290 bytes carrying a real `wss://` push_server. The
same variant, same shim revision, same room and same session was refused with 403 on the next two
attempts, and `xhr-transport.mjs` was refused against that room in between. So `im/fetch` can answer
a signed request — it did once — but nothing about the request explains when, and it is not
reproducible from here. The row is in `fixtures/research/bisect-ledger.json` rather than in a memory,
which is the point of the ledger.

Sampled again the same day, five consecutive `baseline` attempts against a live room: **5 × 403**,
identical signature lengths each time. So the acceptance rate is one in eight-plus, not one in two,
and nothing in the request distinguishes the accepted attempt from the refused ones. It is recorded
rather than explained.

It changes nothing operationally: the direct socket does not need that response.

Everything below is the record of the search that preceded this, kept because its measurements stand
on their own: `im/fetch` is unsigned by the page, `room/enter` verifies our signature and accepts it,
and three read endpoints verify nothing at all.

## What the player actually does

The live web player's transport client is a lazy-loaded chunk,
`static/js/async/9894.<hash>.js`, reachable from the room page's webpack manifest. Its shape,
read directly from that chunk:

```text
/webcast/im/fetch/                          the bootstrap request
/webcast/im/ws_proxy/ws_reuse_supplement/   the socket it upgrades to
```

It polls `im/fetch` with `fetchRule: 1`, and when a response arrives with `fetch_type === 1` and a
non-empty `push_server`, it creates the socket client from `push_server`, `route_params`, and
`heartbeat_duration`. So the WS URI looked **not** independently constructible: `wrss` and `imprp` come from that
response. That reading was of the polling path only. The same chunk's `createClient()` builds a URI
with neither, under `wsDirect` — see the section above.

Two details that matter and are not guessable:

- The request goes out over **`XMLHttpRequest`**, with `withCredentials = true` and
  `Content-Type: application/x-www-form-urlencoded; charset=UTF-8`. Never `fetch`.
- webmssdk hooks XHR and `fetch` **separately** — `XHRSignTime` and `fetchSignTime` are distinct
  fields in its state, and its XHR hook is confirmed present at runtime (it appears in a stack
  trace as `_doRestOfXHRSend`).

`scripts/headless/xhr-transport.mjs` runs that exact route: a real `XMLHttpRequest` in the sandbox,
so the SDK's hooks operate on something that behaves like the browser's.

## Where it stands

Every signing route was tried against `/webcast/im/fetch/`, with an authenticated session:

| Route | Result |
|---|---|
| patched `fetch` suffix | 403 |
| **XHR-signed** (same four parameters) | **403** |
| public `frontierSign` X-Bogus | 200, empty body |
| any of the above, `room_id=1` (nonexistent) | identical |
| **no signature at all** | **200** |

That last row was not measured until 2026-08-18, and it is the answer: the page does not sign this
endpoint. Read on for why, and read the two sections after it for what the earlier rows were actually
showing. Everything between here and there is kept as the record of a diagnosis that went wrong.

The XHR route adds the same four parameters as the fetch route and — confirmed directly, by pointing
the hooked XHR at a dead local port and reading back what it set — **no headers of its own at all**.
So a browser's `im/fetch` request differs from ours in the query and the four signature values, and
in nothing else; there is no header we are failing to send. fetch-versus-XHR is not the difference
either. Feeding back the `msToken` the service issues on rejection
(a real 124-byte token, confirmed non-empty in the signed query) does not change it.

## What the length work established, and what it was worth

The first lead was that the shim's *computed* signatures were short while its *passthrough* values
were exact — `X-Gnarly` 324 against a recorded 332, `X-Dynosaur` 384 against 388/392/444, with
`msToken` and `X-Bogus` correct. Two phases of measurement closed that gap, and both findings stand
as facts about the signer even though the conclusion drawn from them did not:

- **The canonical input tracks the query one byte for one byte.** Padding the query by 50 bytes moved
  the input by exactly 50, and `document.cookie` does not enter it at all — 679 bytes of real session
  cookies changed nothing. The earlier "cookie moves `X-Gnarly` by +8" reading was the `msToken`
  *query parameter* moving, not the jar. `scripts/headless/canonical-input.mjs` measures this.
- **The last 4 bytes were the canvas.** The shim's `getContext` returned `null`, so the SDK's WebGL
  collection came back empty — which *shortens* the fingerprint instead of failing. Giving the shim a
  WebGL and 2D context put `X-Gnarly` at exactly 332 and `X-Dynosaur` at 392, inside the recorded
  set. Bisecting `devicePixelRatio`, screen metrics, `hardwareConcurrency`, language, platform,
  `location.href` and `document.referrer` had moved the input by zero.
- **Real entropy was restored.** `crypto.getRandomValues` had been a fixed sequence for
  reproducibility, which is wrong for anything but differential work; `TTL_DETERMINISTIC` brings the
  fixed sequence back when runs need to be comparable.

What that work did *not* buy is any change in the service's answer — see below. A 324-byte `X-Gnarly`
and the converged 332-byte one are refused identically, so length convergence turned out to be a
proxy that measured nothing the verifier cares about.

## Correction, 2026-08-18: the premise was false

This document used to argue from an asymmetry — `room/info` accepts the suffix that `im/fetch`
refuses — to the conclusion that our signatures are the right *shape* and the wrong *value*. That
argument assumed `room/info` verifies the signature.

It does not. A one-character tamper inside `X-Gnarly`, preserving length and alphabet, returns
byte-identical data. So does removing the signature entirely. So does never signing at all:

| Request to `room/info` | Result |
|---|---|
| signed, untouched | 200, `status_code=0` |
| `X-Gnarly` tampered, one character | 200, `status_code=0` |
| `X-Dynosaur` tampered, one character | 200, `status_code=0` |
| signature removed | 200, `status_code=0` |
| never signed | 200, `status_code=0` |

`gift/list` and `/api/search/live/full/` behave the same way. Reproduce it with:

```sh
node scripts/headless/verify-probe.mjs /tmp/webmssdk.js <room_id> all
```

Three consequences, and they matter more than anything above:

1. **Our suffix has never been validated by anything.** Every "the signer works" result in this
   repository came from an endpoint that does not check. The signing path has been carrying a
   success signal that was never a signal.
2. **`im/fetch` looked like the only verifier available.** It turned out not to be a verifier at all;
   `/webcast/room/enter/` is, and it accepts our signature. See below.
3. **"Right shape, wrong value" is unsupported.** It may still be true. Nothing measured implies it.

Those endpoints are now called unsigned, which removed a signer subprocess from every read.

## What varying the inputs said

At this point `im/fetch` was believed to be the only endpoint that evaluates a signature. It is not
an evaluator of one at all — `room/enter` is the verifier, established further down — so read this
section as "what the endpoint does when handed a parameter it does not expect".

`scripts/headless/im-fetch-bisect.mjs` walks the inputs against `im/fetch`, and
`scripts/headless/value-differential.mjs` establishes what those inputs are. With the clock,
`performance`, `Math.random` and entropy pinned, our signer is reproducible byte for byte, so one
mutation at a time yields the dependency map — no oracle required:

| Signature | Reads |
|---|---|
| `X-Gnarly` | `navigator.userAgent`, the canvas/WebGL fingerprint, the query, the clock, `xmst` |
| `X-Dynosaur` | `navigator.userAgent`, the canvas/WebGL fingerprint, the query, the clock |
| `msToken` | `xmst` only, verbatim |
| `X-Bogus` | nothing in the sweep |

Nothing else reaches the signature — not platform, screen, language, `plugins`, `webdriver`,
`referrer`, `location.href`, nor `document.cookie`. Also dead, measured the same day: the SDK
performs no `mssdk` `/web/common` handshake under any `init` shape, `report()` sends nothing, and
`setTTWid` / `setTTWebid` / `setTTWebidV2` neither populate `_mssdk._sharedCache` nor move the
signature, so the ttwid-binding thesis is gone.

Fifteen variants against the live endpoint, one outcome. Every dated row is in
`fixtures/research/bisect-ledger.json`:

| Variant | Result |
|---|---|
| baseline | 403 |
| three different parameter sets | 403 |
| canvas absent — a 324-byte `X-Gnarly` instead of 332 | 403 |
| Windows UA, query moved to match | 403 |
| Linux UA against a Windows query | 403 |
| `msToken` absent, and stripped after signing | 403 |
| fixed entropy instead of real | 403 |
| without the Chromium client hints | 403 |
| `X-Bogus=1` placeholder removed | 403 |
| a genuine 16-byte `X-Bogus` in its place | 403 |
| `X-Dynosaur` removed, `X-Gnarly` kept | 403 |
| `X-Gnarly` removed, `X-Dynosaur` kept | 403 |
| `frontierSign` `X-Bogus` alone, no suffix | **200, empty** |

Either computed signature, alone, is enough to turn an empty 200 into a 403 — and its content makes
no difference. A signature 8 bytes shorter fares identically to the converged one, which means the
length convergence Phases A and B achieved bought nothing measurable.

Identity was already ruled out: a request with no cookies at all is refused exactly like a fully
authenticated one.

## The answer: the page does not sign this endpoint

Read on 2026-08-18, out of the live page's own `static/js/main.*.js`, where the app calls
`byted_acrawler.init`. The signing allowlist is per host **and per method**, and for
`webcast.tiktok.com` it is seven GET paths and twenty-two POST paths — wallet, KYC, `room/chat`,
`room/enter`, `room/leave`, `room/ping/audience`. In full, the GET list:

```text
/webcast/wallet_api_tiktok/periodic_payout_onboarding/
/webcast/wallet_api_tiktok/payment_instrument_bind_url/
/webcast/wallet_api_tiktok/payment/payment_methods
/webcast/wallet_api_tiktok/notifycenter/notices/
/webcast/wallet_api_tiktok/income_plus/get_user_region_info/
/webcast/wallet_api_tiktok/income_plus/account_steps/
/webcast/wallet_api_tiktok/income_plus/user/
```

`/webcast/im/fetch/` is not there. Neither is `room/info` nor `gift/list`, which is independently why
the tamper test found those unverified. **A browser sends the transport request with no signature at
all**, and the service confirms it:

| Form | Result |
|---|---|
| unsigned | **200** |
| `X-Bogus` alone | 200, empty |
| with `X-Gnarly` or `X-Dynosaur`, any content, any length | **403** |

So the 403 was never about the value. It was a signature the endpoint does not expect, which is
exactly why no content dimension moved it — the one observation that "wrong value" could not explain.

## The signature is correct, and `room/enter` proves it

`/webcast/room/enter/` *is* on the POST allowlist, and it is the only endpoint measured here that
answers differently depending on a signature:

| `POST /webcast/room/enter/` | Result |
|---|---|
| unsigned | 403 |
| `X-Bogus` alone | 403 |
| **the full computed suffix** | **200** |

An endpoint that refuses unsigned and unsigned-plus-`X-Bogus`, and accepts our `X-Dynosaur` +
`msToken` + `X-Bogus` + `X-Gnarly`, is verifying the suffix and accepting it. The signature this
project computes is **right**. Nothing needed fixing in it, and the premise this document argued from
for two commits — that the value was wrong and needed a known-good capture to diff against — was
false in both directions. `scripts/headless/enter-then-fetch.mjs` runs the comparison.

That also retires the capture as a blocker. `import-capture.mjs` stays, because a capture is still
the cheapest way to check the query byte for byte, but nothing is waiting on it.

## The query, from the chunk that builds it

`static/js/async/9894.*.js` builds the transport query itself, in `H(V(props))`, and sends it over a
plain `XMLHttpRequest` with `withCredentials = true` and one header,
`Content-Type: application/x-www-form-urlencoded; charset=UTF-8`. Three differences from what this
repository sent, all now fixed in `ttl_sign_core::params::FetchParams`:

- **`version_code` is `180800`**, a constant in the chunk, not the `270000` the rest of the web app
  uses. Corroborated by the player's own captured socket URI, which carries `180800` too.
- **The initial fetch sends `cursor=0`, `internal_ext=0`, `last_rtt=-1`.** Not empty, and not `0` for
  `last_rtt`. Its builder deletes empty values outright, so `cursor=` is a request no player can
  produce — and we were sending it.
- **`notice=CUSTOM_SIGN_SERVER` is ours**, sent by no browser. Removed.

## What is still not working

`im/fetch` answers **200 with zero bytes**, and so does `room/enter` when accepted. A zero-byte 200 is
not an application refusal — a refusal carries a `status_code` — so this is an edge answering nothing
rather than the service objecting to the request.

Ruled out for the empty body, each measured:

| Suspect | Result |
|---|---|
| the signature, present or absent | both empty (present is also 403) |
| three parameter sets, including the chunk's exact query | empty |
| `resp_content_type=protobuf` versus JSON | empty either way, so there is no error message to read |
| `webcast.us.tiktok.com` | 403 |
| `webcast.tiktokv.com` | empty |
| `x-tt-target-idc` routing header | empty |
| identity, from a full account session to no cookies at all | empty or 403, never data |
| the room | empty across every room tried |

The account is pinned to `alisg` and the rooms tested report idc `my2`, so cross-data-centre routing
remains the standing hypothesis, and it is the one dimension a differently-located session would
settle immediately.

## After `push_server`, nothing is missing

`ttl-live-ws` handles everything downstream, `ttl_sign_core::sanitize_uri` makes the rebuilt socket
URI parseable by a Rust HTTP client, and `live-check` decodes five seconds of events. Listed so the
finish line is explicit: the transport is one accepted `im/fetch` away from working.

## Tools

| Tool | What it does |
|---|---|
| `scripts/headless/verify-probe.mjs` | Does an endpoint verify a signature at all — the tamper test that overturned the premise |
| `scripts/headless/im-fetch-bisect.mjs` | Bisects the signature's inputs against `im/fetch`, with a request budget and a dated ledger; includes the unsigned variants |
| `scripts/headless/enter-then-fetch.mjs` | Signs `room/enter` — the one verifying endpoint — then issues the unsigned transport request |
| `scripts/headless/import-capture.mjs` | Imports one captured signed request and diffs ours against it structurally |
| `scripts/headless/byte-map.mjs` | Per-byte field map of the signature, offline |
| `scripts/headless/value-differential.mjs` | Which inputs the signature value depends on, offline, with a stability gate on the baseline |
| `scripts/headless/canonical-input.mjs` | Signing-input length per route, against the recorded values |
| `scripts/headless/xhr-transport.mjs` | Drives `im/fetch` over a real `XMLHttpRequest`, so the SDK's XHR hook runs |
| `scripts/headless/sign-probe.mjs` | Signature lengths per route |
| `scripts/headless/emit-surface.mjs` | The environment surface the bundle touches |
| `scripts/headless/find-live.mjs` | Live rooms to test against |
| `scripts/headless/lib/xhr.mjs` | The shared XHR implementation, so probe and sender cannot drift |
| `cargo run -p ttl-live-discovery --example live-check` | The end-to-end flow, including the socket and event decode |

Every live tool prints statuses, byte counts and digests only — never a signed URL, cookie or token.

## No fallback any more

A `push_server` obtained any other way used to be a complete substitute: `live-check --ws-uri`
accepted one directly, and everything downstream is built. That flag and its parser are gone, by
decision — the transport is `im/fetch` or nothing, so this document is the critical path rather than
an optimization.
