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

## The lead: our signatures are eight bytes short

The committed oracle profile records what a real browser produced. The headless shim produces
something systematically different:

| Field | Oracle | Headless shim | Delta |
|---|---|---|---|
| `X-Dynosaur` | 388 / 392 / 444 | 384 | −4 or more |
| `X-Gnarly` | 332 | 324 | **−8** |
| `msToken` | 124–172 | 124 | equal (it is a passthrough) |
| `X-Bogus` | 1 | 1 | equal |

Both *computed* signatures are short; both *passthrough* values match. That points at the signing
input, not the algorithm: the shim is feeding the VM a canonical input that differs from a real
browser's, and the service rejects the result.

The oracle profile also records the canonical input length for `X-Gnarly`: **1274 bytes** at
baseline, moving predictably under cookie, query, timezone, and platform mutations. That is a
target to converge on, and it survives the removal of the oracle itself.

This reframes the problem. It is not "find the missing parameter" — it is **make the synthetic
environment produce the same signing input a browser does**, which is exactly the convergence
ladder [09](09-signing-research.md) already defines, now with a concrete numeric target.

## Plan

### Phase A — Measure the canonical input, not just the output

Instrument the shim to capture what the VM receives at the `X-Gnarly` route (entry 48886) and
report its **length only**. The committed subgraph already names that entry and records its input
as a 1274-byte string, so the comparison needs no new oracle.

**Gate:** the probe prints our canonical input length beside the recorded 1274.

### Phase B — Close the gap by bisection

The canonical input is assembled from environment values. Vary one at a time — `navigator`
properties, `screen`, `Intl` timezone, `document.cookie` contents, `location` — and watch the input
length. The controlled corpus already records how the oracle's length moved under the same
mutations (cookie +8, query duplicate +28, timezone −4, platform −9), so each experiment has an
expected answer rather than a guess.

Eight bytes is a small, specific gap: one absent property, or one value shorter than a browser's.

**Gate:** canonical input reaches 1274 bytes for the baseline environment, and `X-Gnarly` reaches
332.

### Phase C — Re-run the transport

With matching signature lengths, re-issue the XHR-signed request. If it still fails, the difference
is outside the signature and the next suspects are ordered: the `ttwid` value's provenance, the
`device_id`/`ttwid` binding, and request headers a browser sends that Node does not (`sec-ch-ua`,
`sec-fetch-*`, `accept`).

**Gate:** a non-empty protobuf with `push_server`.

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
