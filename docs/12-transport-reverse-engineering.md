# 12 — Getting the WebSocket URI from zero

Question this answers: how do we obtain a working `/webcast/im/ws_proxy/` URI with **no browser and
no devtools** — building it from first principles rather than copying one the player made?

Read [11](11-webview-removal.md) first for how the browser-free signer works and what it already
does.

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
`heartbeat_duration`. So the WS URI is **not** independently constructible: `wrss` and `imprp` come
from that response, and a socket opened without them is refused. Verified against both known hosts.

Two details that matter and are not guessable:

- The request goes out over **`XMLHttpRequest`**, with `withCredentials = true` and
  `Content-Type: application/x-www-form-urlencoded; charset=UTF-8`. Never `fetch`.
- webmssdk hooks XHR and `fetch` **separately** — `XHRSignTime` and `fetchSignTime` are distinct
  fields in its state, and its XHR hook is confirmed present at runtime (it appears in a stack
  trace as `_doRestOfXHRSend`).

`scripts/headless/xhr-transport.mjs` runs that exact route: a real `XMLHttpRequest` in the sandbox,
so the SDK's hooks operate on something that behaves like the browser's.

## Where it stands

Every signing route has been tried against `/webcast/im/fetch/`, with an authenticated session:

| Route | Result |
|---|---|
| patched `fetch` suffix | 403 |
| **XHR-signed** (same four parameters) | **403** |
| public `frontierSign` X-Bogus | 200, empty body |
| any of the above, `room_id=1` (nonexistent) | identical |

The XHR route adds the same four parameters as the fetch route and no extra headers, so
fetch-versus-XHR is not the difference. Feeding back the `msToken` the service issues on rejection
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
2. **`im/fetch` is the only verifier available.** It is a binary oracle — refused, answered empty, or
   answered with a `push_server` — and it is the only one.
3. **"Right shape, wrong value" is unsupported.** It may still be true. Nothing measured implies it.

Those endpoints are now called unsigned, which removed a signer subprocess from every read.

## What the only verifier says

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

## Where that leaves it

Content-insensitivity is the important part. If the service were checking our value against a
correct one, some content dimension would be expected to move the verdict; none does. What is
established is narrower than either "the value is wrong" or "the value is right": **no black-box
dimension reachable from here changes the answer.**

### The field layout, measured

The first lever is built: `scripts/headless/byte-map.mjs`. Pin the clock and entropy, move one input,
and diff the signature *by byte position*. The first position that changes says where that input's
contribution begins, and ordering the inputs by it recovers the layout — with no correct signature
anywhere in the process.

| Input | `X-Gnarly` | `X-Dynosaur` |
|---|---|---|
| the clock | from byte 1, avalanches | from byte 1, avalanches |
| the canvas fingerprint | from byte 2 (249 of 332 bytes) | from byte 7 |
| `navigator.userAgent` | from byte 36 | from byte 5 |
| the query | from byte 107 | from byte 6 |
| `xmst` | from byte 128 | absent |

Three things fall out of it:

- **Byte 0 never moves**, in either signature, under any input. It is a container or version tag.
  `X-Dynosaur`'s last byte is likewise fixed.
- **Entropy contributes nothing.** Two runs under real `getRandomValues` are byte-identical, so the
  value is fully determined by the five inputs. The earlier note that real entropy was needed "for
  anything but differential work" was wrong: there is no nonce.
- **The clock avalanches from byte 1**, so the payload is enciphered under something time-derived.
  The later offsets are still ordered — bytes 1–35 respond to the clock but not the user agent — so
  the container is segmented rather than one hash over everything.

The layout points at one input, by elimination: of the five, four have verifiable content — the clock
is real, the user agent is a real Chrome's, the query is what we send, `xmst` is a service-issued
token — and the canvas fingerprint was **invented**. It was also plainly wrong: `toDataURL` returned
20 bytes, a PNG signature followed by a truncated IHDR, undecodable as an image and impossible for
any real canvas. The shim now builds a valid 300×150 RGBA PNG (~1.5 KB base64), with text metrics
that vary by string and real pixels from `getImageData`.

That fixed a genuine defect and did not change the verdict: still 403, with `X-Gnarly` still exactly
332 bytes, since the data URL is hashed rather than embedded. Worth knowing, and worth not repeating.

### What is left

Two levers remain, and neither is a proxy measurement:

1. **Per-byte introspection of the signature.** Pin the clock and entropy, vary one input, and diff
   the output *by byte position*. That yields a field map — which bytes carry the timestamp, which
   the fingerprint, which are structural — and a field map localizes a wrong field without any
   known-good value to compare against.
2. **One known-good signed request** — now the only outstanding one, and the importer is built:

   ```sh
   # Chrome, live room, devtools Network, filter im/fetch, Copy as cURL (or Save all as HAR)
   node scripts/headless/import-capture.mjs /tmp/webmssdk.js --curl /tmp/copied.txt
   ```

   It signs the *captured* query with our signer, so the input is identical and every difference is
   ours, then reports encoding, length, the format byte, and the first differing position. Read
   against the table above, that offset names the field.

   The capture is a replayable capability, so the raw bytes stay in `.local/`, mode 0600 and
   gitignored; only the structure — lengths, encodings, alphabets, differing-position counts — is
   written to `fixtures/research/known-good-signature-v1.json`.

   It does not need to be repeatable. It needs to exist, once.

Native execution of routes 48886 and 55188 remains the third option and remains weeks of work.

## After `push_server`, nothing is missing

`ttl-live-ws` handles everything downstream, `ttl_sign_core::sanitize_uri` makes the rebuilt socket
URI parseable by a Rust HTTP client, and `live-check` decodes five seconds of events. Listed so the
finish line is explicit: the transport is one accepted `im/fetch` away from working.

## Tools

| Tool | What it does |
|---|---|
| `scripts/headless/verify-probe.mjs` | Does an endpoint verify a signature at all — the tamper test that overturned the premise |
| `scripts/headless/im-fetch-bisect.mjs` | Bisects the signature's inputs against `im/fetch`, with a request budget and a dated ledger |
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
