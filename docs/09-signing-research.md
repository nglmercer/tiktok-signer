# 09 — Signing research

This is the active workstream. The immediate target is understanding the page's signing
transformation well enough to state, with evidence, what a native/headless signer would have to
reproduce — **not** to ship a native signer. The WebView remains the reference oracle and the
only supported signing path. LIVE discovery, protobuf transport, and connector compatibility are
out of scope here.

All evidence in this repository is sanitized. No reusable signed URL, signature, device id, raw
signing input, or cookie value is committed. That rule is now **enforced**, not just documented —
see [Artifact hygiene](#artifact-hygiene).

## Scope and stance

- The native signer is intentionally incomplete and must remain **explicitly unsupported** until
  convergence against authorized oracle observations is demonstrated. `NativeBackend::unsupported`
  fails loudly (`SignError::BackendUnavailable`) for every request; it never fabricates a
  transport and never degrades to a rejection that a caller could mistake for a normal server
  refusal. This is covered by regression tests in `ttl-sign-native` and `ttl-sign-server`.
- This document records **what is confirmed at the boundary**, **what remains inferred**, the
  **current convergence level**, and the **exact next blocker**. It deliberately does not
  reproduce a VM instruction/offset map or a step-by-step reproduction recipe. The low-level
  runtime observations live only in the sanitized research fixture and are not restated in prose.

## Confirmed SDK boundary

The page exposes `window.byted_acrawler` (`frontierSign`, `registerWsSigner`, `init`, `report`,
identity setters, mode setters). The signing implementation is a custom bytecode VM bundle
(`webmssdk`), not ordinary minified signing code: it inflates a compressed payload, dispatches
through an opcode table, and uses an encoded string table plus numeric constants. The separately
loaded `@byted/secsdk` bundle is a CSRF SDK and is **not** the signer; resource inspection now
prioritizes the `webmssdk`/`acrawler` candidate before it.

The bundle identity (endpoint, byte length, SHA-256) is pinned in the research fixture so a drift
in the served bundle is detectable. Reproduce the static identity facts with the read-only
inspector:

```sh
node scripts/inspect-webmssdk.mjs /path/to/webmssdk.js
```

## Two signing products, not one

The single most important structural fact for the native boundary: the page has **two different
signing products**, and the public one is not a substitute for the transport one.

- **Public `frontierSign`.** Calling `frontierSign({url})` directly returns only `X-Bogus` (a
  short, fixed-length value). This is a distinct product from the transport signer.
- **Patched `fetch`.** The transport path appends four parameters, always in this order:

  ```text
  X-Dynosaur → msToken → X-Bogus → X-Gnarly
  ```

  Across repeated identical inputs, `X-Dynosaur`, `msToken`, and `X-Gnarly` vary (they carry
  entropy / session state). In every sanitized observation of the patched-fetch path, its
  `X-Bogus` was the stable one-byte literal:

  ```text
  X-Bogus=1
  ```

  This is a composition-stage constant in the sampled fetch route, **not** the cryptographic
  `X-Bogus` product produced by the public `frontierSign` route. The two share a field name and
  nothing else. Because raw values are never retained, the observation asserts the length and
  stability, not a claim about any other byte.

Implication for the native backend: implementing only the public `frontierSign` wrapper cannot
reproduce the fetch signer. The confirmed literal `X-Bogus=1` may be encoded as an assembly-stage
fact (it is, as `FETCH_X_BOGUS_VALUE`), but `msToken`, `X-Dynosaur`, and `X-Gnarly` remain
unimplemented and must not be guessed. A regression test asserts that no dynamic field is ever
encoded as a "confirmed" constant.

## The WebView oracle contract

The WebView is the reference oracle and the only supported signer. Research tooling treats it as
an authority to *observe*, under controlled inputs, never as something to be replaced yet.

- **Controlled experiments.** Each research case changes exactly one input dimension relative to a
  baseline (cookie, timezone, language, platform, query duplicate/removal/emptying, fixed clock,
  …). The plan validator rejects a case that changes zero or several dimensions. The effective
  environment and clock are read back from the page and verified before a run is accepted as
  evidence, so a *declared* environment change that did not take effect is not mistaken for a
  causal result.
- **Same-identity pairing.** Because SDK state is entropic, a fresh-browser comparison alone is
  not causal evidence. Query mutations are run by interleaving baseline and experiment calls in
  one ephemeral browser identity.
- **Sanitized observations.** Captures store equality-preserving SHA-256 digests and byte lengths,
  ordered parameter names, introduced-parameter names, cookie *names*, and fixed-vocabulary value
  classes. They never store or print reusable signed URLs, signature values, cookie values, or raw
  declared signing inputs. Captures use create-new semantics; previous oracle captures are never
  overwritten.
- **Differentials ignore expected entropy.** The trace differential does not compare random
  values, treats overlapping signed-field lengths as compatible, and excludes cookie lengths — so
  a change in a random value alone cannot produce a false structural regression. It still reports
  field presence, order, stability, and disjoint output shapes.

The commands for capturing baselines, running one-variable experiments, and producing classified
differentials are documented in `fixtures/research/README.md`.

## Artifact hygiene

Committed fixtures may only contain hashes, byte lengths, field names, field ordering,
fixed-vocabulary labels, synthetic test identifiers, and call-graph metadata. This is now enforced
by the `ttl-fixture-hygiene` crate, which fails the build if a fixture contains a live secret:

- a session/anti-bot cookie (`msToken`, `ttwid`, `sessionid`, …) with a non-synthetic value;
- a signing query parameter (`X-Gnarly`, `X-Dynosaur`, `_signature`, or `X-Bogus` other than the
  confirmed literal `1`) carrying a captured value;
- a raw signature/signing-field value stored under a signing key instead of a digest + length;
- a reusable signed URL to a real TikTok/ByteDance host;
- a VM artifact that kept raw operand values, operand byte strings, or string/bytecode table
  contents instead of the sanitized widths, opcode slots, and value shapes.

Findings are themselves sanitized — they report the rule, file, line, non-secret field name, and
value length, never the secret. The gate runs in CI and on every `cargo test`
(`crates/ttl-fixture-hygiene/tests/committed_fixtures_are_clean.rs`), and can be run directly:

```sh
cargo run -p ttl-fixture-hygiene -- fixtures
```

It is a one-way filter: a clean scan is necessary, not sufficient. It does not attempt to classify
a bare 19-digit id as "real", because the corpus legitimately uses synthetic 19-digit ids and a
device id only becomes transport-sensitive once embedded in a signed URL, which the URL rule
already catches.

## The VM subgraph model

A full VM trace describes everything the bundle executed. That is too much to reason about and too
much to commit. The repository therefore reduces a trace to the **minimum reachable subgraph** of
each confirmed signing route: the frames a route actually reaches, how they call each other, which
handler slots run, and what shapes cross the boundary. This is a model of *structure*, not of the
algorithm — it says what a native implementation would have to reproduce, not how.

The roots are the confirmed entry points:

| Route | Roots | Phase |
|---|---|---|
| fetch composition | 58628 | invocation |
| `msToken` | 8039, 92825 | eval |
| `X-Dynosaur` | 55188 | invocation |
| `X-Gnarly` | 48886 | invocation |
| public `frontierSign` → `X-Bogus` | 69021 | invocation |

Reachability follows parent → child call edges **within one execution phase**, so bundle-evaluation
frames cannot leak into an invocation route. Intermediate frames such as the `msToken` wrapper
(`91717`) are discovered by the walk rather than hardcoded.

**Retained:** frame entry and parents, call edges, handler/opcode slots, operand widths and helper
kinds, sanitized argument and return shape classes, register read/write counts, and environment
capability flags.

**Dropped by construction:** raw operand values and operand byte strings — they index the VM string
and numeric constant tables — plus the string table itself, decoded string slots, and bundle
source. These fields are never read into the output types, so no future edit can leak them by
forgetting to sanitize, and the hygiene gate refuses them independently
(`vm-operand-value`).

Extraction is a pure function over an already-sanitized artifact: it needs no WebView and builds
without the `webview` feature. Ordering never depends on hash-map iteration or discovery order, so
two equivalent traces serialize to identical bytes.

### Dependency classification

Each route reports its dependencies from a bounded vocabulary — `query`, `cookie`, `clock`,
`timezone`, `language`, `platform`, `screen`, `window`, `document`, `storage`, `crypto`,
`randomness`, `sdk_state`, `constant`, `unknown` — with an explicit evidence level:

| Evidence | Meaning | Source |
|---|---|---|
| `observed_dependency` | A paired, one-dimension controlled experiment moved the route's sanitized shape. | controlled experiment |
| `no_observed_effect` | A controlled experiment changed the dimension and the shape did not move. | controlled experiment |
| `candidate_dependency` | The route *can reach* the input; nothing has demonstrated an effect. | structural |
| `unknown` | Not examined. | — |

The separation is enforced, not conventional: structural capability flags can only ever produce
`candidate_dependency`. Reaching `window` is not the same as depending on it, and a test asserts
that no structural classification claims causality. Only the committed controlled-observation
corpus can produce `observed_dependency` or `no_observed_effect`.

Current classification, from the paired corpus in
`fixtures/research/controlled-observations-2026-08-13.json`:

| Route | `observed_dependency` | `no_observed_effect` |
|---|---|---|
| fetch composition | query, timezone, platform | cookie, clock, language, screen |
| `msToken` | cookie | query, clock, timezone, language, platform, screen |
| `X-Dynosaur` | — | query, cookie, clock, timezone, language, platform, screen |
| `X-Gnarly` | query, cookie, timezone, platform | clock, language, screen |

`X-Dynosaur` shows no controlled dependency while still varying across repeated identical calls,
which places its variation in entropy or SDK state rather than in any input dimension tested so
far. That is a statement about what has been ruled out, not an algorithm claim.

### Structural regression detection

`ttl-sign-subgraph-diff` compares two extractions and exits 2 on a structural difference: a changed
reachable frame set, call edge, handler set, operand width or helper kind, argument or return shape
class, environment flag, route phase, or dependency classification.

Entropy is excluded deliberately, and the exclusions are each asserted by test:

- **Byte lengths.** Signed fields legitimately vary run to run (`msToken` 124–172, `X-Dynosaur`
  388/392/444). Lengths stay in the document as dependency evidence, but a length move is not a
  structural regression.
- **Counts.** Call, step, handler-execution, edge-observation, and register read/write counts
  depend on how many times the page happened to sign.
- **Digests, cookies, session identifiers.** Not in the model at all.

The guarantee that follows, asserted directly: two captures differing only in random values produce
zero structural differences.

## Headless execution, and what it settled

The bundle is a **public static asset**. Running it outside a browser needs no account, no live
room, and no signed request — and it turns out to work. `scripts/headless/` evaluates the real
bundle against a synthetic environment shim with the transport stubbed:

- The bundle evaluates with **no browser** and exposes all nine `byted_acrawler` functions —
  exactly the symbol list this document already pinned.
- `frontierSign` returns a 16-byte `X-Bogus`, matching the oracle's recorded shape.
- The patched `fetch` appends the complete suffix `X-Dynosaur → msToken → X-Bogus → X-Gnarly`,
  in that order, with `X-Bogus=1`.

Two structural facts came out of executing what static analysis could only bound.

### The fetch patch is gated on a path allowlist

`init` builds `window._mssdk._enablePathListRegex` from its `enablePathList` option, and the
patched `fetch` signs only URLs matching it. With no matching entry, `fetch` **is** patched and
appends nothing — a silent no-op that looks identical to a broken signer. Any native or embedded
implementation has to reproduce this gate, and any experiment that forgets it measures nothing.

### `msToken` is not computed

`msToken` is a **verbatim passthrough of `localStorage['xmst']`**. A stored token of length *n*
produces `msToken` of length *n* — confirmed at 124, 132, 152, and 172 bytes, which are precisely
the lengths in the sampled oracle corpus. With no stored token the parameter is present and empty.

This retires the msToken route as a signing problem. There is no algorithm to recover: the field
echoes a token the environment already holds. It also explains the earlier cookie observation —
the synthetic-cookie probe changed the *stored token*, and the length moved with it, which the
corpus recorded as an input dependency without being able to see the mechanism.

What remains for `msToken` is **acquiring** a valid `xmst`, which is an identity/session problem
(see [11](11-webview-removal.md), Phase 3), not a signing one.

## Convergence levels

Progress toward a native signer is reported on this ladder; levels are not skipped when reporting.

| Level | Meaning |
|---|---|
| **L0** | Native and Oracle agree on canonical input: query ordering, duplicate handling, missing/empty fields, environment representation, clock value. |
| **L1** | Agreement on dependency and shape: route reachability, argument classes, input/output lengths and types. |
| **L2** | Agreement on deterministic intermediate values that can be safely represented or hashed. |
| **L3** | Native reproduces one complete signing field (`msToken`, `X-Dynosaur`, or `X-Gnarly`) for an authorized controlled case. |
| **L4** | Native reproduces the complete ordered suffix `X-Dynosaur → msToken → X-Bogus → X-Gnarly`. |
| **L5** | A native-generated transport is accepted by an authorized live test without WebView signing. |

### Current status per route

Evidence to date is boundary and shape observation, not algorithm recovery. Reported conservatively:

Each row is now backed by a committed subgraph in `fixtures/research/signing-subgraph-v1.json`
rather than by prose alone.

| Route | Confirmed | Convergence |
|---|---|---|
| fetch composition / suffix order | Four fields, fixed order; `X-Bogus=1` is a stable one-byte literal in the fetch path. Reaches 59051/59053 from 58628 on a 786-byte canonical input. | **L0–L1**: canonical input handling, route reachability, and output shape are reproducible against sanitized oracle observations; the deterministic constant is encoded. |
| `msToken` | **Resolved**: a verbatim passthrough of `localStorage['xmst']`, confirmed at four token lengths. Not computed. | **L4-capable as a signing field** — there is no transform to reproduce. Blocked on *acquiring* a valid token, which is an identity problem, not a signing one. |
| `X-Dynosaur` | Varies per call; stable output shape from a four-byte typed-array-shaped input; fans out to four child frames. | **L1 partial**: reachability and shape established; no controlled input dimension moves it; algorithm not recovered. |
| `X-Gnarly` | Stable 332-byte output; canonical-input length moves under cookie/query/platform/timezone mutations; fans out to four child frames. | **L1 partial**: reachability, four observed input dependencies, and input/output shape established; algorithm not recovered. |
| public `frontierSign` → `X-Bogus` | Distinct product from the fetch `X-Bogus`; 69021 → 69171 only. | Not a transport route; tracked only to keep it separated from the fetch marker. |

L1 is *established* rather than *partial* for route reachability, argument classes, and output
shapes on every route above — that is what the subgraph fixture and its differential now assert
automatically. It remains **partial** overall because L1 also requires agreement between Native and
Oracle, and no native implementation of these routes exists to agree.

L2 and above are **not** claimed for `X-Dynosaur` or `X-Gnarly`: no deterministic intermediate has
been reproduced for either, and neither has been checked against an authorized oracle value.
`msToken` is the exception, and only because it turned out not to be a computation.

Headless execution reproduces the complete suffix *structurally* — correct fields, correct order,
correct `X-Bogus` constant — but that is not L4. L4 requires agreement with the oracle on the
values, and L5 requires a live acceptance; neither has been tested.

## Why NativeBackend is unimplemented, and the next blocker

The native signer stays unsupported because reproducing `msToken`, `X-Dynosaur`, and `X-Gnarly`
has not converged: current evidence establishes *that* these fields depend on certain inputs and
*what shape* they take, but not a deterministic function that reproduces them. Inferring an
algorithm from output-length coincidences is explicitly disallowed.

**L5 has been reached for the signed REST endpoints.** A headlessly-signed `/webcast/room/info/`
and `/webcast/gift/list/` were accepted by the live service on 2026-08-18, returning real room data
and the same 673-gift table the WebView run reported. Signing is no longer what blocks a
browser-free build; see [11](11-webview-removal.md) for the measured comparison.

`/webcast/im/fetch/` answers 403 under that suffix — but **200 under the public `frontierSign`
product**. The two signing products are per-route rather than interchangeable, and the transport
endpoint wants the public one. One guest run returned a full protobuf with a `wss://` push_server;
most return an empty 200. See [11](11-webview-removal.md) for the measured table.

**Exact next blocker, in order:**

1. **Oracle agreement on values.** Headless execution now produces the full suffix, so the missing
   comparison is no longer structural. Sign one controlled case in the WebView and headlessly with
   the *same* environment and stored token, and compare the fields. This needs one authorized run
   and settles whether the shim is faithful.
2. **A real `xmst`.** `msToken` is solved as a transform but unusable without a valid stored token.
   Acquiring one natively is Phase 3 of [11](11-webview-removal.md).
3. **`X-Dynosaur` and `X-Gnarly` entropy sources.** Both are stable under every controlled input
   dimension tested and vary across repeated identical calls, so their variation comes from
   entropy or SDK state rather than from any input. The headless shim makes this directly testable:
   its `crypto.getRandomValues` is deterministic, and both fields are correspondingly stable.
   Establishing *which* source each field draws from is the next real research step.

Per-opcode attribution remains useful for the native-interpreter track, but it is no longer the
critical path: executing the bundle answered the questions it was meant to bound.

Only opcode semantics reachable from a confirmed route are to be implemented, and an unsupported
handler must fail explicitly rather than approximate.

Note that recovering a deterministic function is only one of three routes to a browser-free build,
and it is the hardest; [11](11-webview-removal.md) lays out the alternatives and argues for
executing the bundle in an embedded JS engine first. The blockers above remain correct for the
native-interpreter track. Until an L2 result exists for at least one
route, the native backend remains `UnsupportedAlgorithm` and must not be presented as
live-compatible.

## Machine-readable baseline

Three sanitized artifacts, all covered by the hygiene gate and none containing captured values:

| File | Contents |
|---|---|
| `webmssdk-profile-2026-08-13.json` | Bundle identity, sanitized route shapes, controlled-probe observations, fixed-vocabulary labels. |
| `signing-subgraph-v1.json` | The versioned, deterministic per-route subgraph document described above. |
| `controlled-observations-2026-08-13.json` | The paired one-dimension experiment corpus that dependency classification consumes. |

The subgraph fixture currently carries `"provenance": "derived_from_profile_v1"`. It was
transcribed from the profile's sanitized `route_frame_map`, which records frames, edges, step
counts, and shapes but **no per-opcode attribution** — so its handler sets are empty, and a test
asserts that a document claiming `extracted_from_vm_trace` provenance cannot have steps without
handler attribution. Re-running `ttl-sign-subgraph` over a fresh authorized VM trace upgrades the
provenance and fills in the handler sets; the differential then shows exactly what the real
extraction added.

Commands for both tools are in `fixtures/research/README.md`.
