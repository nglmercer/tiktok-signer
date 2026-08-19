# 07 — Deploying the sign server

The signer runs neither a browser nor Node. It executes the real `webmssdk` bundle under a
synthetic environment in a JavaScript engine linked into the server process, so a deployment needs
no WebKit, no GTK, no X server — the three things this document used to be almost entirely about —
and no runtime on the host at all.

What it does need, in full: **the Rust binary, the signing bundle, and an account session.**

Which engine is a build choice. QuickJS by default (3.1 MB, ~70–89 ms per signature through the
server); `--features v8` links V8 instead (+67 MB, ~19 ms). Both produce byte-identical signatures —
that is the acceptance test. See [13 — Embedded runtime](13-embedded-runtime.md).

```sh
cargo build --release -p ttl-sign-server --bin ttl-sign-headless-server --features headless
cargo build --release -p ttl-sign-server --bin ttl-sign-headless-server --features headless,v8
```

## Container

```sh
docker compose up --build
curl -fsS http://127.0.0.1:8080/healthz
```

The image is a Rust builder plus a `debian:bookworm-slim` runtime holding the binary, a CA bundle
and `curl` for the healthcheck. The signing bundle is fetched at build time from its public static
URL and pinned into the image; override `WEBMSSDK_URL` at build time, or `TTL_BUNDLE` at run time,
to use another.

There is no entrypoint script any more. The server is PID 1 and receives the stop signal directly.
`STOPSIGNAL SIGINT` is set because graceful shutdown is wired to `tokio::signal::ctrl_c`, and
`docker stop` would otherwise send SIGTERM, which the process would take as an unhandled kill.

## Bare metal

```sh
sudo apt install -y ca-certificates
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
Environment=RUST_LOG=ttl_sign_server=info,ttl_sign_headless=info
ExecStart=/usr/local/bin/ttl-sign-headless-server
KillSignal=SIGINT
Restart=always

[Install]
WantedBy=multi-user.target
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `TTL_BIND` | `127.0.0.1:8080` | Listen address; `0.0.0.0:8080` inside a container |
| `TTL_MAX_CONCURRENT` | `4` | Concurrent signing requests |
| `TTL_BUNDLE` | `/tmp/webmssdk.js` | Signing bundle path |
| `TTL_SESSION_FILE` | `$XDG_CONFIG_HOME/ttl-signer/session` | Session path |
| `RUST_LOG` | `ttl_sign_server=info,ttl_sign_headless=info` | Log filter |

The server has **no authentication**. Bind it to loopback and put a reverse proxy or a firewall in
front; anything that can reach it can sign with the deployed identity.

## Sessions

The message socket refuses a jar-less handshake, so an account session is **required**, not
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

Signing is a call into a warm engine — 70–89 ms under QuickJS, 19 ms under V8, against 6–12 s for
the old page navigation. There is no process per signature any more. The engine holds one context
on one thread and serves requests in arrival order, so `TTL_MAX_CONCURRENT` bounds how many
callers queue rather than how many processes exist.

See [06 — Risks and operations](06-risks-and-ops.md) for rate limits and session hygiene, and
[11 — Removing the WebView](11-webview-removal.md) for the evidence behind this architecture.
