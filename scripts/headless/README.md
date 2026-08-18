# Headless signer probes

Research tooling that runs the real `webmssdk` bundle **without a browser**, against a synthetic
environment shim, with the transport stubbed. Nothing is sent and no signed value is printed —
only parameter names and byte lengths.

These probes answered the feasibility question behind Track A in
[`docs/11-webview-removal.md`](../../docs/11-webview-removal.md): the bundle loads, exposes the
identical SDK surface, and produces the complete fetch suffix outside a browser.

## Getting the bundle

The bundle is a public static asset. It is **not committed** — third-party source stays out of the
repository. Fetch it and verify it against the pinned digest:

```sh
curl -s -o /tmp/webmssdk.js \
  https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
sha256sum /tmp/webmssdk.js   # must match fixtures/research/webmssdk-profile-2026-08-13.json
```

A digest mismatch means the bundle drifted; re-run the probes before trusting any prior finding.

## Probes

```sh
# Which SDK functions load, and what each signing route produces.
node scripts/headless/sign-probe.mjs /tmp/webmssdk.js        # no stored token
node scripts/headless/sign-probe.mjs /tmp/webmssdk.js 124    # with a 124-byte synthetic token

# Regenerate the committed environment surface.
node scripts/headless/emit-surface.mjs /tmp/webmssdk.js \
  fixtures/research/environment-surface-v1.json
```

`shim.mjs` is the synthetic browser: a `with`-scoped `Proxy` whose `has` trap returns true, so
every free identifier the bundle resolves is recorded by path. Unknown properties return
`undefined` and are recorded as missing — that list is what a shim has to grow to satisfy.

## What the probes establish

- The bundle evaluates with no browser and exposes all nine `byted_acrawler` functions.
- `frontierSign` returns a 16-byte `X-Bogus`, matching the oracle's recorded shape.
- The patched `fetch` appends `X-Dynosaur → msToken → X-Bogus → X-Gnarly`, in that order, with
  `X-Bogus=1` — matching the oracle.
- The patch is gated on `window._mssdk._enablePathListRegex`, built from `init`'s `enablePathList`.
  With no matching path, `fetch` is patched but appends nothing.
- `msToken` is a **verbatim passthrough** of `localStorage['xmst']`: a stored token of length *n*
  produces `msToken` of length *n*. It is not computed.

The environment shim is deliberately deterministic — `crypto.getRandomValues` returns a fixed
sequence — so repeated runs are comparable. Real signing needs real entropy.
