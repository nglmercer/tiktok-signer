# 11 — Removing the WebView

Question this answers: what would it actually take to run this project in production with no
browser, and in what order should that be attempted?

Short answer: **the WebView is not one dependency, it is three**, and only one of them is the
signing problem everyone talks about. Removing it is tractable, but the realistic first move is not
the one the research doc has been pointing at.

Read [09](09-signing-research.md) first for the confirmed signing boundary and the L0–L5 ladder
used throughout this document.

## What the WebView is actually doing

`ttl-sign-webview::Signer` is the only `SignerBackend` that works live today. Its surface splits
into three independent problems:

| # | Dependency | What needs it | Removable by |
|---|---|---|---|
| 1 | **Signing** | `fetch`, `fetch_with`, `sign_url`, `sign_ws_uri`, `page_ws_fetch_result`, and — indirectly — `room_info` and `gift_list`, which the page's patched `fetch` signs for free | executing or reproducing the webmssdk transform |
| 2 | **Identity and session bootstrap** | `ttwid` / `msToken` cookie acquisition, `rotate_guest_identity`, `challenge` / `wait_for_challenge`, `reload` | a native cookie/challenge lifecycle |
| 3 | ~~Rendered-DOM discovery~~ | `live_channels` | **solved** — `/api/search/live/full/` serves the same list as signable JSON |

Problem 3 looked like the hard one: `live_channels` reads the **rendered** DOM, and the `/live`
page genuinely ships no channel data — its HTML contains no `wss://`, no `push_server`, and its
embedded rehydration blob is A/B configuration. The conclusion drawn from that, *that no signing
progress could ever make it native*, was wrong. The DOM is one source of the list, not the only
one: `/api/search/live/full/` returns the same rooms as JSON, and it signs like any other endpoint.

The lesson worth keeping: "the page renders this" establishes that one route needs a browser, not
that every route does. `room_lookup` was already native because it is **unsigned**; `live_channels`
is native because it is merely **signed**.

So the honest framing is: solving signing gets you most of the way, but a browser-free build still
needs answers for 2 and 3.

## Three candidate tracks for signing

The research doc currently frames convergence as recovering a deterministic function — a pure Rust
reimplementation of what the VM computes. That is one of three options, and it is the hardest.

### Track A — Headless JS runtime (execute the bundle, drop the browser)

Embed a JavaScript engine and run the real `webmssdk.js` against a synthetic environment shim.
Chromium/WebKit, the event loop, the renderer, and the ~200 MB of browser go away; the vendor's JS
stays.

- **Effort:** low-to-medium. Days-to-weeks, not months.
- **Fidelity:** maximal — it is the same code the oracle runs.
- **Resilience to bundle drift:** high. A new bundle version generally just works; only a *new*
  environment API would need shimming.
- **Cost:** you are still executing third-party JS, and the shim must be complete enough that the
  SDK cannot tell it apart from a browser. The bundle already uses `TextDecoder`, typed arrays, and
  an inflate step; `document.cookie`, `navigator`, `screen`, `location`, `crypto.getRandomValues`,
  `Date`, and storage all appear in the trace's capability flags.
- **Engine choice:** QuickJS via `rquickjs` is the pragmatic default — small, ES2020, no V8 build
  toolchain, and no `unsafe`-heavy embedding story. `boa_engine` is pure Rust but slower and less
  complete; `deno_core`/`rusty_v8` is the heaviest option and reintroduces a large native build.

**This is what "remove the WebView" realistically means in the near term.** It removes the browser
without pretending the algorithm has been recovered.

### Track B — Native VM interpreter (execute the bytecode, drop the JS)

Implement the webmssdk bytecode VM in Rust, but **only the opcodes reachable from the confirmed
signing routes** — which is exactly what `fixtures/research/signing-subgraph-v1.json` exists to
enumerate. An unreached opcode is not implemented; an unsupported opcode fails explicitly rather
than approximating.

- **Effort:** medium-to-high, but *bounded by reachability* rather than by the full 355-handler
  table. The subgraph work exists to make this bound measurable.
- **Fidelity:** high, and verifiable step-by-step against the oracle's VM trace.
- **Resilience to bundle drift:** medium. Opcode semantics are more stable than offsets; the
  bundle's frame offsets are pinned per version and would need re-extraction.
- **Cost:** you must still supply the same environment surface as Track A, because the opcodes read
  `window`, `document`, storage, and crypto.

This is the durable target: no vendor JS, no browser, and a model the repository can test.

### Track C — Algorithm recovery (reimplement the transform)

Recover the mathematical transform behind each field and write it directly in Rust.

- **Effort:** high, and **not bounded** — it may be infeasible within reasonable time.
- **Fidelity:** total, if achieved.
- **Resilience:** highest, if the transform is stable across versions.
- **Cost:** this is the only track that can fail outright after significant investment.

Track C should be attempted **opportunistically, never as the plan**: if Track B's execution traces
reveal that a route reduces to a recognizable primitive, take it. Do not schedule it.

### Recommendation

Run **A now**, **B as the durable goal**, **C only if B hands it to you**. A and B share the
environment shim, so A is not throwaway work — it is Phase 2 of B.

## Plan

### Phase 0 — Environment surface fingerprint *(prerequisite for A and B; do this first)*

Everything downstream needs the same artifact and nobody has it yet: **the exact set of browser
properties the bundle touches while signing a fetch.**

Instrument the existing WebView run with `Proxy` traps on `window`, `document`, `navigator`,
`screen`, `location`, `localStorage`/`sessionStorage`, and `crypto`, then record every property
read during one patched-fetch sign. Emit a sanitized fixture — property paths, access counts, value
*type classes*, and byte lengths, never values — as
`fixtures/research/environment-surface-v1.json`.

This is cheap, needs one authorized run, and it is the single artifact that decides whether Track A
is a week or a quarter. It also resolves the `sdk_state` and `randomness` dependency candidates
that the subgraph currently cannot distinguish.

**Gate:** the fixture lists every touched property; `missing_shim_coverage` asserts the Phase 2
shim covers all of them, so a missing shim is a test failure with a property name attached rather
than a runtime mystery.

**Status: complete — and it did not need the browser.** The bundle is a public static asset, so it
can be executed directly against a synthetic shim with the transport stubbed. `scripts/headless/`
does exactly that, and `fixtures/research/environment-surface-v1.json` is the committed result.

The surface is **69 properties**, of which only nine are outside `window`:

```text
document.addEventListener  document.cookie      document.createElement
document.createEvent       document.readyState  localStorage.getItem
location.href              navigator.connection navigator.userAgent
```

That is the entire browser dependency of the signing path. It is far smaller than the phase
assumed, which is the answer to "is Track A a week or a quarter".

`ttl-sign-env-surface` (the in-page Proxy recorder) remains available and is still the right tool
for confirming that a *live page* touches nothing the headless run missed. It is now a
cross-check rather than the only route.

### Phase 1 — Per-opcode attribution *(already the stated next blocker in [09](09-signing-research.md))*

One authorized `ttl-sign-vm-trace` run reduced with `ttl-sign-subgraph`, upgrading the committed
fixture from `derived_from_profile_v1` to `extracted_from_vm_trace`. Track B cannot be scoped
without it: the handler set *is* the work estimate.

**Gate:** `ttl-sign-subgraph-diff` shows what the real extraction added; the provenance test flips
to the stricter branch automatically.

### Phase 2 — Track A: `ttl-sign-jsvm`

**Feasibility is no longer in question.** Running the bundle headless under Node already produces
the complete fetch suffix — `X-Dynosaur → msToken → X-Bogus → X-Gnarly`, with `X-Bogus=1` — with
all nine SDK functions exposed and a 69-property shim. The remaining work is porting that shim to
an embedded engine and checking the values against the oracle, not discovering whether it can work.

New crate embedding QuickJS, with the Phase 0 surface implemented as a shim. Wire it as a
`SigningAlgorithm` **behind the existing `NativeBackend`**, not as a fourth backend — the
`SignerBackend` contract tests, the `UnsupportedAlgorithm` default, and the "never fabricate,
never silently reject" guarantees then all apply unchanged.

Two behaviours the shim must reproduce, both discovered by executing rather than reading:

- **The path allowlist.** `init` builds `_mssdk._enablePathListRegex` from `enablePathList`, and
  the patched `fetch` signs only matching URLs. A missing entry means `fetch` is patched and
  appends nothing — indistinguishable from a broken signer.
- **`msToken` is a passthrough** of `localStorage['xmst']`, not a computation. The shim must supply
  a real stored token; there is nothing to implement.

Convergence, reported on the existing ladder and not skipped:

- **L0/L1** — canonical input and route shapes match the oracle. Reuses the existing differential.
- **L2** — deterministic intermediates match, verified against the sanitized VM trace shapes.
- **L3** — one complete field (`X-Gnarly` first) reproduces against an authorized oracle value held
  outside the repository.
- **L4** — the full ordered suffix `X-Dynosaur → msToken → X-Bogus → X-Gnarly`.
- **L5** — an authorized live test accepts a transport signed without the WebView.

**Gate:** `ttl-sign-oracle-replay` already runs WebView and a candidate in one process and exits 2
on mismatch. Point it at the JS-VM algorithm; that is the L3–L4 gate with no new tooling.

### Phase 3 — Native identity and session lifecycle

Independent of the signing track and required by all of them:

- `ttwid` acquisition without a page load, and the guest-identity rotation that
  `rotate_guest_identity` performs today.
- **`xmst` acquisition.** Now known to be the whole of `msToken`. This moved from the signing
  workstream to this one, and it is on the critical path for a working native transport.
- `msToken` cookie lifecycle — note the confirmed observation that the cookie *feeds* the msToken
  route, so the cookie and the signed field are coupled and must be modelled together.
- Challenge detection and surfacing. The WebView can show a challenge to a human; a headless build
  can only detect one and fail loudly. Decide the product behaviour here explicitly — a silent
  retry loop against an anti-bot challenge is the worst possible outcome.

**Gate:** a native guest identity reaches L5 on a room that the WebView can also reach, and a
challenge produces a distinct, testable error rather than a timeout.

### Phase 4 — Discovery without a renderer

`live_channels` is the one capability with no signing answer. Options, in order of preference:

1. Drop it from the browser-free build. Most consumers already know which creator they want, and
   `room_lookup` (unsigned) resolves `unique_id` → `room_id` natively today.
2. Find an unsigned or page-signed JSON endpoint behind the `/live` page's client-side rendering
   and use it directly.
3. Keep a browser for discovery only, as an optional feature — explicitly *not* on the signing
   path.

**Gate:** the browser-free build compiles and serves without `ttl-sign-webview` in its dependency
tree. The existing dependency-tree gate from [08](08-headless-migration.md) already enforces this
shape.

**Status: option 1 implemented.** `ttl-live-discovery` performs the unsigned `unique_id` →
`room_id` lookup natively, sharing [`ttl_sign_core::room`] with the WebView path so the two cannot
disagree about what "live" means. The crate states the boundary as data — `requirement()` maps each
discovery operation to `None`, `Signature`, or `Renderer` — so "signing progress does not make
`live_channels` native" is a test rather than a paragraph. CI asserts the crate's dependency tree
contains no `wry`.

Options 2 and 3 remain open. Nothing in the browser-free path needs them: a caller that knows which
creator it wants is fully served today.

### Phase 5 — Track B: native VM interpreter

With Phase 1's handler set and Phase 0's environment surface in hand, implement the reachable
opcodes. Reuse the Phase 2 shim as the environment provider — the interpreter's `window` reads
resolve exactly the way the JS VM's did.

Build it route by route, `X-Gnarly` first, and keep the JS-VM algorithm as the differential oracle
so every opcode is validated against a running implementation rather than against a guess.

**Gate:** the native interpreter and the JS VM produce identical output for the whole authorized
corpus; unsupported opcodes fail explicitly.

### Phase 6 — Decommission

**Done. The `ttl-sign-webview` crate is deleted** and CI asserts no workspace target pulls in a
browser engine under any feature combination.

What went with it: the interactive `login` example (provide the session file directly instead),
the browser probes (`endpoint-probe`, `ws-probe`, `page-probe`, `limit-probe`, `fetch-dump`,
`schema-check`), and the six page-driven lab binaries (`ttl-sign-oracle`, `ttl-sign-oracle-replay`,
`ttl-sign-url-oracle`, `ttl-sign-trace`, `ttl-sign-paired-trace`, `ttl-sign-vm-trace`,
`ttl-sign-env-surface`). `scripts/headless/` covers signing, the environment surface, transport,
and discovery; the paired URL and trace differentials have no replacement, so the oracle-vs-oracle
comparisons that produced this document can no longer be re-run.

`cargo run -p ttl-live-discovery --example live-check` is the replacement end-to-end check.

**Original plan for this phase, kept for the record:** `ttl-sign-headless` implements `SignerBackend` with no browser:
it builds the transport query with `ttl_sign_core::params`, has a `UrlSigner` sign it, and decodes
the response into the same `SignedFetch` every other backend returns. The sign server, the
connector, and the contract tests are unchanged.

```sh
cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless
```

CI asserts that binary's dependency tree contains no `wry`.

The WebView is now **research-only**, and deliberately not deleted. It is the reference oracle the
whole method depends on: the parity checks in this document — including the one that proved an
empty transport response is upstream rather than a defect — are only possible because a known-good
signer is available to compare against. `ttl-sign-lab`'s oracle binaries still require
`--features webview`.

Two limits on the production path, both properties of the endpoint rather than of the signer:

- **An account session is required.** `/webcast/im/fetch/` answers a guest with an empty 200. The
  server refuses to start without a session rather than serving requests that would all come back
  empty.
- **Empty responses happen**, and the WebView sees them on the same rooms at the same moments.
  Verified again while writing this: room `7675358679254649620` returned `EmptyBody` from both the
  headless server and `fetch-dump` within a minute of each other.

## Verified against the live service

The WebView example `cargo run -p ttl-live-discovery --example live-check` is the reference flow.
`scripts/headless/native-check.mjs` runs the same steps with no browser. Measured against a live
room on 2026-08-18:

| Step | WebView | Headless | Result |
|---|---|---|---|
| who is live now | rendered `/live` DOM | signed `/api/search/live/full/` | **200**, live rooms with ids and viewer counts |
| `unique_id` → `room_id` | page fetch | plain HTTP | **200**, same room id |
| guest identity | page navigation | `GET /@user/live` | **issues `ttwid`, `tt_csrf_token`, `tt_chain_token`** |
| `msToken` | page state | `im/fetch` 403 response | **issues a 124-byte token** |
| `/webcast/room/info/` | page-signed | `ttl-live-discovery` + headless signer | **live title and viewer counts, from Rust** |
| `/webcast/gift/list/` | page-signed | `ttl-live-discovery` + headless signer | **668 gifts, from Rust** — same cheapest gift as the WebView run |
| `/webcast/im/fetch/` | signed by the page | headless-signed + session | **74,670-byte protobuf with a `wss://` push_server** |
| event stream | page WebSocket relay | — | blocked on the above |

**This is L5 for the signed REST endpoints.** A native, browser-free, headlessly-signed request was
accepted by the live service and returned real room data. Signing is no longer the obstacle.

### The two signing products are per-route, and this route needs the other one

The patched-fetch suffix is not universally correct. Measured on 2026-08-18:

| Endpoint | patched-fetch suffix | `frontierSign` X-Bogus |
|---|---|---|
| `/webcast/room/info/` | **200** with data | — |
| `/webcast/gift/list/` | **200** with data | — |
| `/webcast/im/fetch/` | **403** | **200** |
| `/webcast/room/enter/` | **403** | **403** |
| `/webcast/room/ping/audience/` | 200 | 200, `"User doesn't login"` |

This corrects a working assumption in [09](09-signing-research.md): the two products are not two
routes to the same signature, and the transport endpoint wants the *public* one. Sending the wrong
product produces a 403 that is indistinguishable from a broken signer, which is why this went
unnoticed while the suffix was assumed to be the transport signature.

### Signed discovery runs from Rust

`ttl-live-discovery` now implements the signed calls as well as the unsigned lookup. It never
signs: `room_info` and `gift_list` take a `UrlSigner`, and `CommandSigner` drives
`scripts/headless/sign-url.mjs` as a subprocess, so Rust reaches the signer without embedding a
JavaScript engine and without a WebView.

```sh
cargo run -p ttl-live-discovery --example discover -- <unique_id>
```

```text
@andygarcia.pesca room_id=7675336599840393991 live=true
room/info  title="¿Llegamos a 30K de seguidores?" viewers=3284 likes=0
gift/list  668 gifts
           Welcome Dallah (1 diamonds, id=919346)
```

That is the same cheapest gift the WebView `live-check` reports for the same room. When an
embedded engine replaces the subprocess, only the `UrlSigner` implementation changes.

### The transport bootstraps headlessly

`scripts/headless/transport.mjs` returned a **74,670-byte protobuf carrying
`wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/`** with no browser. The
WebView oracle returned 74,273 bytes for the same room minutes earlier.

It needs two things, both established by measurement rather than assumption:

- **An authenticated account session.** A guest gets an empty 200 no matter how well signed, and
  `/webcast/room/ping/audience/` says `status_code=20003, "User doesn't login"` outright. The
  session is read from the file the WebView path already uses.
- **The public signing product**, as above. `room/enter` answers 403 under both products, with and
  without a session, and is not required.

**Empty responses are upstream.** This endpoint often answers 200 with an empty body — and when it
does, the WebView oracle returns `Rejected(EmptyBody)` for the same room at the same moment,
verified paired across two rooms and both `sup_ws_ds_opt` values. The two paths succeed together
and fail together, so an empty response is a server-side condition, not a gap in the headless
implementation. Which `sup_ws_ds_opt` value works also varies, so both are tried.

That parity check is the right first step whenever this looks broken:

```sh
node scripts/headless/transport.mjs /tmp/webmssdk.js -- <user>
node scripts/headless/transport.mjs /tmp/webmssdk.js <user>
```

### What is settled, and what is not

`room/info` and `gift/list` succeed with the same signature machinery that `im/fetch` rejects, so
the signature is demonstrably valid. `im/fetch` returns 403 even holding `ttwid`, `tt_csrf_token`,
`tt_chain_token`, and a service-issued `msToken`.

Two things worth noting before anyone attributes this to the signer:

- The WebView **does not call `im/fetch` either**. It relays the page's own WebSocket and
  synthesizes a `FetchResult` from the player's URI. The repository already recorded that
  re-issuing `/webcast/im/fetch/` from Rust behaves differently from issuing it in the page.
- The live page HTML contains no `wss://`, `push_server`, or `im/fetch` data. The transport URI is
  built client-side after load, which is precisely why the page relay exists.

Transport bootstrap is therefore **solved for authenticated sessions**: every step from discovery
through the push_server URI now runs without a browser. What remains is opening the WebSocket with
those parameters from Rust — `ttl-live-ws` already speaks that protocol — and the guest case, which
this endpoint refuses by design.

## Risks and kill criteria

State these now, so the decision to stop is made on evidence rather than on sunk cost.

| Risk | Signal | Response |
|---|---|---|
| **Bundle drift** | The pinned `1.0.0.388` bundle digest changes | Track A absorbs it; Track B needs offset re-extraction. The digest is already pinned and drift is already detectable — keep it that way. |
| **Environment fingerprinting** | Native transports are rejected while the WebView succeeds with identical parameters | The shim is incomplete or detectable. Phase 0's fixture is the debugging tool; expand it rather than guessing. |
| **Anti-bot challenge** | Challenges appear at a higher rate headless than in the WebView | This is a product decision, not an engineering one. Do not build evasion; surface the challenge. |
| **Server-side acceptance** | L4 passes, L5 fails | The signature is structurally right but the identity or session is not. This is Phase 3, not Phase 2 — do not respond by editing the signing code. |
| **Track B does not converge** | An opcode's semantics resist reproduction after a bounded effort | Stop. Track A already removed the browser; B is an improvement, not the requirement. |

The kill criterion worth naming explicitly: **Track A succeeding is the point where "remove the
WebView" is done.** Tracks B and C are about removing vendor JS, which is a different and lesser
goal. Do not let B block shipping A.

## Sequencing summary

```text
Phase 0  environment surface fingerprint   ✅ done headlessly; 69 properties
Phase 1  per-opcode attribution            ← scopes Track B
Phase 2  Track A: JS VM, L0 → L5           ← the browser is gone here
Phase 3  native identity/session lifecycle ← required regardless of track
Phase 4  discovery without a renderer
Phase 5  Track B: native interpreter       ← the vendor JS is gone here
Phase 6  decommission to research-only
```

Phase 0 is done and needed no live run. Phase 2's feasibility is settled by live acceptance of
headless-signed REST requests. The critical path is now Phase 3 — specifically the transport
bootstrap, which is the only step with no demonstrated native path. Phase 3 can
proceed in parallel with Phase 2. Phases 4 and 5 are independent of each other.

## What does not change

Until L5 is demonstrated, `NativeBackend` stays `UnsupportedAlgorithm`, `X-Bogus="1"` stays the
only confirmed constant, no dynamic field is guessed, and no unsupported algorithm is presented as
live-compatible. This plan changes the *route* to convergence, not the standard of evidence.
