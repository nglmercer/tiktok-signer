# 09 — Signing research

This is the active workstream. The immediate target is the page's signing transformation,
not LIVE discovery, protobuf transport, or compatibility with `tiktok-live-connector`.
All evidence below is sanitized: no reusable URL, signature, device id, or cookie value is
committed.

## Confirmed SDK boundary (2026-08-13)

The page exposes `window.byted_acrawler` with `frontierSign`, `registerWsSigner`, `init`,
`report`, identity setters, and mode setters. The actual public bundle observed in the page is:

```text
webmssdk/1.0.0.388/webmssdk.js
source bytes:  235357
source SHA-256: dee22566273d398e074df6db40f39cfd827f6b8efd6fc382de03c44c501299ac
```

The separately loaded `@byted/secsdk v1.2.22` bundle is a CSRF SDK, not the signing
implementation. It was initially selected because its path contains `secsdk`; resource
inspection now prioritizes explicit `webmssdk/acrawler` candidates before its 64-resource
limit.

The signing bundle is a custom bytecode VM rather than ordinary minified signing code. It
inflates a 48,610-byte payload into 94,030 bytes, dispatches through an opcode table, and uses
an encoded string table plus numeric constants. The decoded bytecode SHA-256 is
`bc791ca2d4704d407ed36269b9bb758f807377915f90d34d007216de6620e8ff`.

Two exported wrappers identify the relevant VM entry points:

| Product | VM offset | Frame size |
|---|---:|---:|
| `frontierSign` | 69021 | 25 |
| `registerWsSigner` | 69906 | 31 |
| `init` | 94000 | 11 |

Run the read-only static inspector against a locally downloaded bundle to reproduce those
facts:

```sh
node scripts/inspect-webmssdk.mjs /path/to/webmssdk.js
```

## Two signing products, not one

Calling `frontierSign({url})` directly returned only `X-Bogus`, with an observed value length
of 16 bytes. That is not equivalent to the page's patched `fetch` path.

The patched `fetch` path consistently appends these parameters in this order:

```text
X-Dynosaur → msToken → X-Bogus → X-Gnarly
```

Repeated identical inputs show that `X-Dynosaur`, `msToken`, and `X-Gnarly` vary. The patched
path's `X-Bogus` was stable and one byte long in every sanitized observation. Raw values are
intentionally not retained, so the observation does not claim what that byte is.

This distinction is a native-backend requirement: implementing only the public
`frontierSign` wrapper cannot reproduce the fetch signer.

A patched, ephemeral copy of the bundle can expose only its VM tables. The lab wraps the
opcode dispatcher, initializes the copy with the page's existing configuration, and replaces
its native `fetch` with an in-memory response stub. No signed request leaves the iframe. In
the first two runs, the full fetch path began at bytecode offset 61096 and executed 40,723 and
40,768 VM instructions; `frontierSign` executed 1,609. Both stubbed fetch runs still produced
the exact four-field output shape above.

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-vm-trace --features webview -- \
  fixtures/research/plan.example.json baseline frontier

cargo run -p ttl-sign-lab --bin ttl-sign-vm-trace --features webview -- \
  fixtures/research/plan.example.json baseline fetch
```

The report contains only bundle hashes, opcode/offset counts, numeric operand values, call
edges, call phases, bounded result names and lengths, sanitized argument shapes, decoded
field-key slot indices, and fixed-vocabulary value classes such as `literal_one` and
`typed_array`. It excludes source, the raw VM string table, input values, cookies, and output
values. The dispatcher is instrumented in the bundle itself, so frames reached while the SDK is
evaluated are traced as well as frames reached by the later fetch invocation.
The VM trace schema is version 2 for this phase-aware operand/input record.

The opcode catalogue has 355 handler slots. The frontier trace visits 170 handlers and the
fetch trace visits 239. Each visited handler records the number of `N`/`j`/`x` operand-helper
reads, observed operand widths, bounded operand-byte examples, handler tags for register/table
operations, and static flags for VM calls and browser/environment references (`window`,
`document`, storage, crypto, and fetch). The
observed helper widths are `0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 16, 18, 20` bytes.
This is the first executable opcode/operand map; it is intentionally a runtime map of the
confirmed entry paths, not a claim that every unreachable handler belongs to signing.

### Candidate subroutes

The same sanitized report records bounded VM-call returns (`entry`, parent entry, type, and byte
length). Comparing those lengths with the four fields appended by the fetch hook gives the first
usable subroute map:

| Product/field | Candidate VM entry | Observed shape | Confidence |
|---|---:|---|---|
| `frontierSign` → `X-Bogus` | `88`, `1075`, `67569`, `68501` | 16-byte string | direct frontier result-path candidates |
| fetch → `msToken` | `8039`, `92825` | 124-byte baseline / 132-byte cookie-probe string | direct length match; one entry is called through `91717` |
| fetch → `X-Dynosaur` | `55188` | variable 388/392/444-byte string | direct length match across repeats |
| fetch → `X-Gnarly` | `48886` | 332-byte string | direct length match across repeats |
| fetch composition wrappers | `88`, `1075`, `8685` | both Gnarly-shaped and Dynosaur-shaped lengths | wrapper/composition candidates |
| fetch suffix assembly | `58628` | field-key slots 656/657/658/660; `X-Bogus` receives literal "1" | confirmed composition boundary |

The fetch hook's `X-Bogus` output is a stable one-byte marker in these observations. The
sanitized decoder/register trace locates the fetch composition frame at `58628`: it decodes the
four field keys at string slots `656`, `657`, `658`, and `660`, and writes/re-reads a
`literal_one` value for the `X-Bogus` key (slot `658`). This means the sampled fetch route does
not generate a cryptographic X-Bogus value; its confirmed stage is the deterministic constant
`X-Bogus=1`. The public frontier path remains a separate 16-byte signing route and decodes slot
`658` inside nested frame `69171` under entry `69021`. Raw values are never written to the trace
or profile.

The phase-aware call map tightens the route separation:

- `msToken`: frames `56 → 91717 → 92825` (plus direct wrapper `8039`) run during bundle
  evaluation; `94000` is the separate `init` frame. The direct route receives a four-byte string
  shape; its handler reads `window`/storage, and the cookie probe changes its result length from
  124 to 132 bytes.
- `X-Dynosaur`: frame `55188` runs during fetch invocation, receives a four-byte typed-array
  shape, and returns the variable 388/392/444-byte shape.
- `X-Gnarly`: frame `48886` runs during fetch invocation, receives a canonical-input-shaped
  string, and returns the 332-byte shape. Its input length changes under cookie, query,
  platform, and timezone mutations.
- Fetch assembly: frame `58628` runs during invocation and calls `59051`/`59053` while composing
  the four suffix fields.

For the separate public route, `69021` returns an object keyed by `X-Bogus` and calls nested
frame `69171`; the 16-byte string path is `67569 → 68501`. This confirms that the public helper
and the fetch marker are different products even though they share the field name.

## Controlled dependency probes

The research runner now applies browser-visible overrides before document scripts and exposes
sanitized probes for the effective environment and clock. A fixed-clock case reported exactly
`1700000000000` from `Date.now()` and kept the same four-field output shape.

Three fresh-identity repetitions with the fixed, synthetic `msToken` cookie probe produced:

| Route shape | Baseline | Cookie probe |
|---|---:|---:|
| `msToken` / entries `8039`, `92825` | 124 bytes | 132 bytes |
| `X-Dynosaur` / entry `55188` | 388 bytes | 388 bytes |
| `X-Gnarly` / entry `48886` | 332 bytes | 332 bytes |
| fetch `X-Bogus` marker | 1 byte | 1 byte |

This is the first repeatable dependency result for a subroute. The probe value is a fixed
non-secret test token; no cookie or signature value is persisted. The machine-readable record is
in `controlled_probe_observations.cookie_msToken_alpha` in the SDK profile.

The phase-aware trace adds an input-shape result to that matrix. In the baseline, the sampled
`X-Gnarly` route received a 1,274-byte canonical-input-shaped string; the cookie probe changed
that shape to 1,282 bytes while leaving the 332-byte output shape unchanged. A duplicate
`room_id` changed the input shape to 1,302 bytes; removing `notice` changed it to 1,248 bytes;
an empty `region` changed it to 1,272 bytes. The typed-array-shaped Dynosaur input stayed four
bytes in all these cases. These are dependency/shape observations, not algorithm recovery.

The VM-trace report now includes the sanitized `effective_environment` emitted by the page and a
`clock_ms` probe. On the current Linux WebView host, the baseline reports `en`, `en-US`,
`Linux x86_64`, `America/New_York`, `US`, and 1920×1080; the language, timezone, platform, and
screen cases report their requested effective values. Their result shapes remain unchanged,
while the Gnarly input shape moves to 1,270 bytes for `America/Lima` and 1,265 bytes for `Win32`.
The profile stores only these sanitized shapes and effective-environment labels.

## Controlled query observations

Baseline and mutation calls were interleaved in the same incognito WebView identity. Three
repetitions per side produced these structural results:

| Mutation | Reproducible result |
|---|---|
| Duplicate `room_id` | The second occurrence is preserved through signing. |
| Remove `notice` | The field remains absent; the SDK does not restore it. |
| Set `region` empty | The field remains present with a zero-byte value. |
| Move `room_id` first | Input order is preserved; the SDK-added four-field suffix is unchanged. |

Signature digests cannot yet establish whether changing one input changes a particular
entropic output: successive calls rotate SDK state. The trace differential therefore ignores
raw random-value inequality, suppresses overlapping signed-field length sets, and excludes
cookie lengths. It still reports field presence, order, stability, and disjoint output shapes.

## Connector boundary

Installed `tiktok-live-connector` 2.4.3 removes `X-Bogus`, `X-Gnarly`, and `msToken` before
sending the clean URL to Euler's signing route. It contains no implementation of this SDK
algorithm. Connector support is downstream integration work and cannot be used as the
signing source.

## Next technical step

The next native milestone is a VM-oriented extractor/interpreter for the confirmed bundle. The
phase-aware extractor and route map are now in place; the remaining work is dependency isolation
and algorithm convergence:

1. Expand the same controlled matrix to additional cookie probes and browser-visible signals.
2. Record whether each route's *input/output shape* changes; do not infer causality from rotating values or
   from a fresh browser identity.
3. Keep the browser-level emulation for timezone/language/screen and the fixed clock enabled for
   research cases; verify every run through `effective_environment` and `clock_ms` before using
   it as a controlled experiment.
4. Reproduce each deterministic stage against sanitized trace shapes and then against authorized
   oracle values held outside the repository.
5. Encode the confirmed fetch constant `X-Bogus=1` in the native assembly stage, but keep the
   complete backend explicitly unsupported until `X-Dynosaur`, `msToken`, and `X-Gnarly`
   converge.

The machine-readable baseline is
`fixtures/research/webmssdk-profile-2026-08-13.json`.
