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

**Gate not met**, but the failure is now precisely located.

Holding the converged signature fixed and moving only the identity — `scripts/headless/identity-probe.mjs`
— changes nothing:

| Identity | Result |
|---|---|
| account session + fresh page cookies | 403 |
| page cookies only, no session | 403 |
| `ttwid` alone | 403 |
| **no cookies at all** | 403 |
| session with `device_id=0` | 403 |

Every row carries `X-Gnarly` at 332 bytes. Identity is not the lever: a request with no cookies at
all is refused identically to a fully authenticated one. Chromium client hints (`accept`,
`sec-ch-ua*`, `sec-fetch-*`) change nothing either.

Removing parameters one at a time locates it exactly:

| Request | Result |
|---|---|
| real `X-Bogus` only | **200**, empty |
| `X-Dynosaur` + `X-Bogus` | **403** |
| `X-Gnarly` + `X-Bogus` | **403** |
| full suffix, with or without a real `X-Bogus` | **403** |
| full suffix with `notice=CUSTOM_SIGN_SERVER` removed | **403** |

**Either computed signature present flips an empty 200 into a 403.** The service verifies
`X-Dynosaur` and `X-Gnarly` and rejects ours. An absent signature is merely unauthenticated —
answered, and answered with nothing; a *wrong* one is refused outright.

That is the real conclusion, and it is sharper than "the transport is blocked": the signatures are
now the right **shape** and the wrong **value**. Length convergence was necessary and is not
sufficient.

## What that means for the goal

Reaching a valid value is level L2/L3 on the ladder in [09](09-signing-research.md) — reproducing
deterministic intermediates, then a complete field — and that work needs an oracle to compare
values against, one signed request whose bytes are known-good. The WebView provided exactly that
and has been removed.

So the honest position: from zero, with no browser and no captured URI, the remaining step is
value-level signature convergence, and the tooling for it is a differential against a known-good
signature that this repository no longer has a way to produce. The measurable proxies — canonical
input length, output length, parameter set, identity dimensions — are all exhausted and all match.

Options, in the order they would settle it:

1. **One known-good signed request**, from any source, retained as bytes. That restores the
   differential and makes L2 approachable. It does not need to be repeatable; it needs to exist.
2. **Native execution of the two routes.** The subgraph already names their entries (48886, 55188)
   and the extractor reduces them; implementing their opcodes would let the value be derived rather
   than compared. Bounded by reachability, but a large piece of work.
3. **Accept the captured-URI path** as the supported route, and treat `im/fetch` as closed.

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
