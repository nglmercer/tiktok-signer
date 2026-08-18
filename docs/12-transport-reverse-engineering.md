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

## The lead that opened it: signatures shorter than the oracle's

The starting observation was that the shim's *computed* signatures were short while its
*passthrough* values were exact — `X-Gnarly` 324 against a recorded 332, `X-Dynosaur` 384 against
388/392/444, with `msToken` and `X-Bogus` correct. That pointed at the signing input rather than
the algorithm, and the committed profile still held the target: a **1274-byte** canonical input for
`X-Gnarly` at baseline, surviving the oracle's own removal.

Phases A and B below closed that gap. `X-Gnarly` is now 332 bytes, exactly the recorded value, and
`X-Dynosaur` lands inside the recorded set. The transport is still refused, which is the useful
part: it means the remaining difference is **not** the signature.

## Plan

### Phase A — Measure the canonical input, not just the output — **done**

`scripts/headless/canonical-input.mjs` patches the VM's call wrapper to record the argument length
at each route entry and prints it beside the oracle's recorded value. Lengths only; no value is
retained. The committed subgraph already names entry 48886 and records its input as a 1274-byte
string, so the comparison needed no new oracle.

**Gate met.** It also established the shape of the input: the canonical string tracks the query
**one byte for one byte** (padding the query by 50 bytes moved the input by exactly 50), and
`document.cookie` does not enter it at all — 679 bytes of real session cookies changed nothing.
The earlier "cookie moves X-Gnarly by +8" observation was the `msToken` *query* parameter moving,
not the cookie jar.

That immediately explained most of the gap: the probe's hand-written query was 659 bytes while the
project's own `FetchParams` builds 741. With the real parameter set, the fetch-composition input
matched the oracle **exactly** (786 = 786) and `X-Gnarly` came within 4 bytes.

### Phase B — Close the gap by bisection — **done**

The canonical input is assembled from environment values. Vary one at a time — `navigator`
properties, `screen`, `Intl` timezone, `document.cookie` contents, `location` — and watch the input
length. The controlled corpus already records how the oracle's length moved under the same
mutations (cookie +8, query duplicate +28, timezone −4, platform −9), so each experiment has an
expected answer rather than a guess.

Eight bytes is a small, specific gap: one absent property, or one value shorter than a browser's.

**Gate met: `X-Gnarly` is 332 bytes, exactly the oracle's recorded value.**

The remaining 4 bytes were not where the plan guessed. Bisecting `devicePixelRatio`, screen
metrics, `hardwareConcurrency`, language, platform, `location.href`, and `document.referrer` moved
the input by zero. `X-Gnarly` signs over the query *including* `X-Dynosaur`, so its 4-byte deficit
was inherited: `X-Dynosaur` was 384 against a recorded 388/392/444.

The cause was the canvas. The shim's `getContext` returned `null`, so the SDK's WebGL collection —
it keeps a `WEBGL` field in its state — came back empty, which **shortens** the fingerprint instead
of failing. Giving the shim a WebGL and 2D context put both signatures in the oracle's
distribution:

| Field | Oracle | Before | After |
|---|---|---|---|
| `X-Gnarly` | 332 | 324 | **332** |
| `X-Dynosaur` | 388 / 392 / 444 | 384 | **392** |

Real entropy was also restored — `crypto.getRandomValues` had been a fixed sequence for
reproducibility, which is wrong for anything but differential work. `TTL_DETERMINISTIC` brings it
back when comparing runs.

### Phase C — Re-run the transport — **not achieved**

With matching signature lengths, re-issue the XHR-signed request. If it still fails, the difference
is outside the signature and the next suspects are ordered: the `ttwid` value's provenance, the
`device_id`/`ttwid` binding, and request headers a browser sends that Node does not (`sec-ch-ua`,
`sec-fetch-*`, `accept`).

**Gate not met.** With signatures in the oracle's distribution, `/webcast/im/fetch/` still answers
403 over the XHR route and an empty 200 over `frontierSign`. Adding the client hints a Chromium XHR
sends (`accept`, `sec-ch-ua*`, `sec-fetch-*`) changed nothing.

So signature *shape* is now right and the request is still refused. What that rules out matters:
the remaining difference is not the signing input, since the one measurable proxy for it now
matches exactly. The open suspects, in order: the `ttwid` value's provenance (ours comes from a
plain page GET, not from a browser that ran the anti-bot flow), the `device_id` to `ttwid` binding,
and whether this account or address is simply refused on this endpoint after the volume of testing
here.

The honest reading is that this is no longer a signing problem, and the next evidence has to come
from a request that differs in identity rather than in signature.

### Phase D — Only then, the socket

`ttl_sign_core::ws_uri::fetch_result_from_ws_uri` and `ttl-live-ws` already handle everything after
`push_server` exists, and `live-check` decodes five seconds of events. No work is expected here; it
is listed so the finish line is explicit.

## Tools

| Tool | What it does |
|---|---|
| `scripts/headless/xhr-transport.mjs` | Drives `im/fetch` over a real `XMLHttpRequest`, so the SDK's XHR signing hook runs; reports the parameters and headers it adds |
| `scripts/headless/sign-probe.mjs` | Signature lengths per route — the measurement Phase B iterates on |
| `scripts/headless/transport.mjs` | The `frontierSign` route, for comparison |
| `scripts/headless/emit-surface.mjs` | The environment surface, which is what Phase B mutates |
| `scripts/headless/find-live.mjs` | Live rooms to test against |
| `cargo run -p ttl-live-discovery --example live-check` | The end-to-end flow, including the socket and event decode |

## What would make this unnecessary

A `push_server` obtained any other way is a complete substitute, because everything downstream is
built. `live-check --ws-uri` accepts one directly. That is the pragmatic fallback if the phases
above stall, and it is the honest reason this document exists: the reverse-engineering is a way to
stop needing a browser **once**, not a prerequisite for the rest of the system.
