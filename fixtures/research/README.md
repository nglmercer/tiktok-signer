# Controlled research plans

`plan.example.json` is safe configuration, not a captured live observation. Replace its
synthetic room id with a room you are authorized to test, validate the plan, then run one
case per fresh WebView process:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-plan -- fixtures/research/plan.example.json

cargo run -p ttl-sign-lab --bin ttl-sign-oracle --features webview -- \
  fixtures/research/plan.example.json baseline /tmp/ttl-oracle-captures
```

Run the baseline and one experiment at a time. The validator rejects an experiment that
changes zero or multiple dimensions. Captures are written with create-new semantics and
contain a sanitized `case.json` plus a digested `observation.json`; existing captures are
never overwritten.

After capturing the baseline and one variant, produce the classified experiment report:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-observation-diff -- \
  /tmp/ttl-oracle-captures/baseline/observation.json \
  /tmp/ttl-oracle-captures/timezone-lima/observation.json
```

To compare two individual replay cases (including captured `case.json` files), run:

```sh
cargo run -p ttl-sign-lab -- \
  /path/to/oracle/case.json /path/to/candidate/case.json
```

An explicitly live differential can execute the selected case against WebView and one
sanitized replay candidate in the same process:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-oracle-replay --features webview -- \
  fixtures/research/plan.example.json baseline /path/to/candidate/case.json
```

The command returns exit code 2 for a structured mismatch and never prints raw signed URLs
or cookie values.

To isolate the SDK's URL transformation without replaying it from Rust, run:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-url-oracle --features webview -- \
  fixtures/research/plan.example.json baseline /tmp/ttl-oracle-captures
```

This writes only the unsigned and signed query digests, ordered parameter names and value
digests, introduced parameter names, cookie names, and a cookie-header digest. It never
stores or prints the reusable signed URL, signature values, cookie values, or raw declared
signing inputs. The latter are embedded only as equality-preserving SHA-256 digests and byte
lengths. The artifact also records the effective page-derived environment after WebView
startup, so a query-only mutation is not mistaken for a real navigator-environment change.
The WebView's patched `fetch` does issue the browser GET; its response is not consumed by the
URL oracle.

Repeat the identical input to classify stable and entropic slots while identifying and
hashing the loaded public SDK bundle:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-trace --features webview -- \
  fixtures/research/plan.example.json baseline /tmp/ttl-sign-traces 5 1100
```

For query mutations, interleave baseline and experiment calls in the same ephemeral browser
identity:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-paired-trace --features webview -- \
  fixtures/research/plan.example.json query-duplicate-room-id \
  /tmp/ttl-sign-paired 5 1100
```

The paired runner currently accepts exactly one `query_mutation`: `remove`, `duplicate`,
`set`, or `move`. It preserves duplicate occurrences and original encoding. Its differential
does not compare random values, treats overlapping signed-field lengths as compatible, and
does not infer query dependencies from rotating cookie lengths.

Trace the custom bytecode VM with either the public helper or a stubbed patched-fetch
transport:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-vm-trace --features webview -- \
  fixtures/research/plan.example.json baseline frontier

cargo run -p ttl-sign-lab --bin ttl-sign-vm-trace --features webview -- \
  fixtures/research/plan.example.json baseline fetch
```

Fetch mode initializes an ephemeral bundle copy with the page configuration but replaces its
native transport before initialization. It records VM offsets/opcodes and the four output
names and lengths, plus the sanitized `effective_environment` and `clock_ms` reported by the
page; no
signed request is sent and no raw output value is retained. A declared timezone, language, or
screen change is not considered controlled unless that effective environment changes too.

Controlled plans may use `timestamp.mode = "fixed"` and the signing-only
`cookie_mutation` probe (`empty`, `alpha`, or `numeric`). Cookie probes are deliberately a
small fixed vocabulary so a real session token cannot enter a plan or artifact.

After capturing the baseline and one declared one-variable case, localize their first
differences by signing stage:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-url-diff -- \
  /tmp/ttl-oracle-captures/baseline-signing/signing-observation.json \
  /tmp/ttl-oracle-captures/timezone-lima-signing/signing-observation.json
```

The WebView keeps SDK entropy but the research runner can now override `Date.now()` and
zero-argument `Date` construction for a fixed-time case. Consequently, the validator guarantees
one changed input and the VM trace exposes a sanitized `clock_ms` probe; random-value differences
still remain expected unless the route shape or lengths change.

Configured account sessions are referenced only by the enum value `configured` and loaded
from the external session file. They are never embedded in plans or artifacts. Guest is the
default and should be preferred whenever it can answer the research question.

`webmssdk-profile-2026-08-13.json` is the sanitized, machine-readable baseline for the
confirmed SDK bundle, VM offsets, and output shapes. It contains no captured values.

## Route subgraph extraction

A VM trace describes everything the bundle executed. Reduce it to the reachable subgraph of the
confirmed signing routes:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-subgraph -- \
  /tmp/ttl-vm-trace.json \
  --controlled fixtures/research/controlled-observations-2026-08-13.json \
  --output fixtures/research/signing-subgraph-v1.json
```

This binary reads an artifact: it needs no WebView, no `webview` feature, and never signs. Pass
`--route <name>` (repeatable) to extract a single route; the names are `fetch_composition`,
`ms_token`, `x_dynosaur`, `x_gnarly`, and `frontier_x_bogus`.

The output retains frame entries and parents, call edges, handler slots, operand *widths* and
helper kinds, sanitized argument and return shape classes, register read/write counts, and
environment capability flags. It never retains operand values or operand byte strings (they index
the VM string and numeric constant tables), the string table, bundle source, or any signing
material. `ttl-fixture-hygiene` enforces this with the `vm-operand-value` rule.

Ordering never depends on hash-map iteration or discovery order: every collection is sorted by a
total key, so two equivalent traces serialize to identical bytes.

Detect structural Oracle-vs-Oracle regressions between two extractions:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-subgraph-diff -- \
  fixtures/research/signing-subgraph-v1.json /tmp/candidate-subgraph.json
```

Exit code 2 signals a structural difference — a changed reachable frame set, call edge, handler
set, operand shape, argument/return shape class, or environment flag. Entropy never produces one:
signed-value byte lengths, call/step/execution counts, and digests are deliberately excluded, so a
run that differs only in random values reports zero differences.

### Controlled observations

`controlled-observations-2026-08-13.json` is the paired, one-dimension experiment corpus that
dependency classification consumes. Each entry names a route, one dimension from the bounded
vocabulary, and whether the route's sanitized shape moved. Structural capability flags can only
ever produce `candidate_dependency`; only an entry in this corpus can produce
`observed_dependency` or `no_observed_effect`.

### Committed subgraph fixture

`signing-subgraph-v1.json` currently carries `"provenance": "derived_from_profile_v1"`: it was
transcribed from the sanitized `route_frame_map` of the v1 research profile, which records frames,
edges, step counts, and shapes but no per-opcode attribution. Re-running `ttl-sign-subgraph` over
a fresh authorized VM trace replaces it with `extracted_from_vm_trace` and fills in the handler
sets; `ttl-sign-subgraph-diff` then shows exactly what the real extraction added.
