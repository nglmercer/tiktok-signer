# 07 — Deploying the sign server

The signer no longer runs a browser. It executes the real `webmssdk` bundle under a synthetic
environment in Node, so a deployment needs no WebKit, no GTK, and no X server — the three things
this document used to be almost entirely about.

What it does need: the Rust binary, Node 19.7 or newer (for `Headers.getSetCookie`), the signer
script, the signing bundle, and an account session.

**`TTL_SIGNER=embedded` removes the Node requirement too.** The same sandbox then runs in a QuickJS
context inside the server process — no subprocess per signature, and 70–89 ms instead of 95–105 ms.
It is opt-in for now; see [13 — Embedded runtime](13-embedded-runtime.md) for the measurements and
the parity test behind it. With it set, a deployment is the binary, the bundle and the session.

## Container

```sh
docker compose up --build
curl -fsS http://127.0.0.1:8080/healthz
```

The image is a Rust builder plus a `node:22-bookworm-slim` runtime. The bundle is fetched at build
time from its public static URL and pinned into the image; override `WEBMSSDK_URL` at build time,
or `TTL_BUNDLE` at run time, to use another.

There is no entrypoint script any more. The server is PID 1 and receives the stop signal directly.
`STOPSIGNAL SIGINT` is set because graceful shutdown is wired to `tokio::signal::ctrl_c`, and
`docker stop` would otherwise send SIGTERM, which the process would take as an unhandled kill.

## Bare metal

```sh
sudo apt install -y nodejs ca-certificates    # node >= 19.7
cargo build --release -p ttl-sign-server --bin ttl-sign-headless-server --features headless

curl -s -o /opt/webmssdk.js \
  https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
```

```ini
# /etc/systemd/system/ttl-sign-server.service
[Unit]
Description=TikTok LIVE sign server
After=network-online.target
Wants=network-online.target

[Service]
User=signer
Environment=TTL_BIND=127.0.0.1:8080
Environment=TTL_BUNDLE=/opt/webmssdk.js
Environment=TTL_SIGN_SCRIPT=/opt/headless/sign-url.mjs
Environment=RUST_LOG=ttl_sign_server=info,ttl_sign_headless=info
ExecStart=/usr/local/bin/ttl-sign-headless-server
KillSignal=SIGINT
Restart=always

[Install]
WantedBy=multi-user.target
```

Copy `scripts/headless/` to `/opt/headless/`; the signer script imports `shim.mjs` from beside it.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `TTL_BIND` | `127.0.0.1:8080` | Listen address; `0.0.0.0:8080` inside a container |
| `TTL_MAX_CONCURRENT` | `4` | Concurrent signing requests |
| `TTL_BUNDLE` | `/tmp/webmssdk.js` | Signing bundle path |
| `TTL_SIGN_SCRIPT` | `scripts/headless/sign-url.mjs` | Signer entry point |
| `TTL_SESSION_FILE` | `$XDG_CONFIG_HOME/ttl-signer/session` | Session path |
| `RUST_LOG` | `ttl_sign_server=info,ttl_sign_headless=info` | Log filter |

The server has **no authentication**. Bind it to loopback and put a reverse proxy or a firewall in
front; anything that can reach it can sign with the deployed identity.

## Sessions

`/webcast/im/fetch/` answers a guest with an empty body, so an account session is **required**, not
optional. The server refuses to start without one rather than serving requests that would all come
back empty.

There is no login command: the interactive flow opened a real browser window, which is exactly
what was removed. Export the cookies from a browser where you are already logged in and write them
as a cookie header:

```sh
install -m 600 /dev/null /srv/ttl/session
printf 'sessionid=...; sessionid_ss=...; sid_tt=...; ttwid=...' > /srv/ttl/session
```

Mount it read-only at `/home/signer/.config/ttl-signer/session`, or point `TTL_SESSION_FILE` at it.

`sessionid` **is** the account. Everything the server does is attributed to it, and it is not a
remedy for rate limiting. Treat the file as a credential: mode `0600`, outside the repository, and
rotated by logging out in the browser that issued it.

## Verification challenges

A challenge used to be answerable by making the window visible to a person. There is no window
now, so a challenge surfaces as a failed request and nothing else. Do not build a retry loop
around one: that turns a block into a hammering client. Rotate the identity, or stop.

## Operational notes

Signing is one subprocess launch plus one HTTPS round trip — roughly 1 s in validation, against
6–12 s for the old page navigation. Each request spawns a Node process, so `TTL_MAX_CONCURRENT`
bounds process count as well as concurrency.

`/webcast/im/fetch/` intermittently answers 200 with an empty body, which the server reports as a
rejection rather than an error. That behaviour was measured identically on the WebView path before
it was removed, so it is upstream. See [06 — Risks and operations](06-risks-and-ops.md) for rate
limits and session hygiene, and [11 — Removing the WebView](11-webview-removal.md) for the
evidence behind this architecture.
