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
