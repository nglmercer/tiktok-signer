# 07 — Deployment

## The constraint

The signer is a real browser. On Linux `wry` is WebKitGTK and the window comes from GTK,
which needs a connection to an X11 or Wayland display **even when the window is hidden**
(`EngineConfig::visible` defaults to `false`). On a headless VPS there is no display, so
window creation fails at startup:

```text
could not create window (is a display available? otherwise use Xvfb)
```

This is not a configuration mistake to be worked around. The project's whole premise is
that TikTok's own `webmssdk.js` signs the requests and owns the WebSocket, so the browser
engine is the product, not an implementation detail.

What a server does **not** need is a desktop environment. GNOME, KDE, a session manager,
and a login screen are all irrelevant. The requirement is an X server — `Xvfb`, ~30 MB of
RAM — plus the GTK and WebKit runtime libraries.

## Docker

```sh
docker compose up -d --build
curl http://127.0.0.1:8080/healthz
```

The image is built by [`Dockerfile`](../Dockerfile) in two stages: `rust:1.90-bookworm`
with `libwebkit2gtk-4.1-dev` to compile, `debian:bookworm-slim` with the runtime libraries
and `xvfb` to run. The builder is well above the workspace's `rust-version = "1.82"`
because the locked dependency tree is not: it needs the 2024 edition (1.85) and `time`
0.3.55 (1.88).

`docker-entrypoint.sh` starts `Xvfb` on `:99`, waits for its socket to appear — the WebView
fails to build if it starts first — and `exec`s the server so the stop signal reaches it
directly. That signal is **SIGINT**, set by `STOPSIGNAL`: the server's graceful shutdown is
wired to `tokio::signal::ctrl_c`, so a default `docker stop` SIGTERM would kill it without
running `Signer::shutdown`. A systemd unit needs the same, `KillSignal=SIGINT`.

Verified 2026-08-10: the image builds, the WebView comes up under `Xvfb`, the bridge
reports ready, `/healthz` answers `ready: true`, and `docker stop` logs `shutdown
requested` in well under a second.

Three container-specific settings are baked into the image:

| Variable | Reason |
|---|---|
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | No GPU; the DMA-BUF renderer hangs or crashes without one |
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | Avoids the accelerated compositor path |
| `LIBGL_ALWAYS_SOFTWARE=1` | Forces software GL |

`shm_size` is raised to 512 MB because WebKit's multi-process model uses shared memory and
Docker's 64 MB default causes renderer crashes.

### Media decoding

`gstreamer1.0-libav` is deliberately **not** installed. Without codecs WebKit cannot decode
the room's video and audio, which the signer never reads — installing them only spends CPU
and bandwidth. The resulting log lines are warnings, not failures:

```text
(WebKitWebProcess:723297): GStreamer-WARNING **: ../gst/gstpad.c:1605: pad has no probe with id `1'
GStreamer element fakevideosink not found. Please install it
WebKit wasn't able to find a WebVTT encoder. Subtitles handling will be degraded
Unable to get session D-Bus address: Failed to execute child process "dbus-launch"
```

All four are expected in this image and none of them stop the signer. The `dbus-launch`
line is WebKit looking for a desktop session bus that a server has no reason to run.

## Bare VPS

```sh
sudo apt install -y xvfb libwebkit2gtk-4.1-0 libgtk-3-0 ca-certificates
# to build on the same machine, add:
# libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config build-essential
```

Run `Xvfb` as its own unit rather than wrapping the server in `xvfb-run`, so the display
survives a server restart:

```ini
# /etc/systemd/system/xvfb.service
[Unit]
Description=Virtual framebuffer for the TikTok sign server

[Service]
ExecStart=/usr/bin/Xvfb :99 -screen 0 1280x800x24 -nolisten tcp
Restart=always

[Install]
WantedBy=multi-user.target
```

```ini
# /etc/systemd/system/ttl-sign-server.service
[Unit]
Description=TikTok sign server
After=network-online.target xvfb.service
Requires=xvfb.service

[Service]
User=signer
Environment=DISPLAY=:99
Environment=WEBKIT_DISABLE_DMABUF_RENDERER=1
Environment=WEBKIT_DISABLE_COMPOSITING_MODE=1
Environment=LIBGL_ALWAYS_SOFTWARE=1
Environment=TTL_BIND=127.0.0.1:8080
Environment=RUST_LOG=ttl_sign_server=info,ttl_sign_webview=info
ExecStart=/usr/local/bin/ttl-sign-server
KillSignal=SIGINT
Restart=always

[Install]
WantedBy=multi-user.target
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `TTL_BIND` | `127.0.0.1:8080` | Listen address; `0.0.0.0:8080` inside a container |
| `TTL_MAX_CONCURRENT` | `2` | Concurrent signing requests |
| `TTL_LANDING_URL` | `https://www.tiktok.com/live` | Page loaded at startup |
| `TTL_SESSION_ID` | — | `sessionid` cookie; takes precedence over the file |
| `TTL_SESSION_FILE` | `$XDG_CONFIG_HOME/ttl-signer/session` | Saved session path |
| `TTL_CONTACT_US` | — | Reported by `/healthz` |
| `RUST_LOG` | `ttl_sign_server=info,ttl_sign_webview=info` | Log filter |
| `DISPLAY` | `:99` in the image | X server to connect to |

The server has **no authentication**. Bind it to loopback and put a reverse proxy or a
firewall in front; anything that can reach it can sign with the deployed identity.

## Sessions on a headless host

Prefer a guest — see the README. Nothing below is needed to listen to a public room.

When an account is genuinely required, the login flow cannot run on the server: it opens a
visible window and waits for a person. Log in on a workstation, then copy the file:

```sh
cargo run -p ttl-sign-webview --example login          # on a workstation
scp ~/.config/ttl-signer/session vps:/srv/ttl/session  # mode 0600, outside the repository
```

Mount it read-only at `/home/signer/.config/ttl-signer/session`, or point `TTL_SESSION_FILE`
at it.

## Verification challenges

`Signer::set_window_visible(true)` hands a captcha to a person. On a headless host there is
nobody to hand it to, so plan for one of these instead:

- Rotate the guest identity on a refusal — `signer.rotate_guest_identity()` — which is the
  supported answer and the reason to stay a guest.
- Attach `x11vnc` to `:99` and answer the challenge over VNC. Do not expose that port.
- Re-run the login on a workstation and replace the session file.

## Operational notes

Latency observed in validation is 6–12 s per signature, dominated by page navigation.
Clients must not treat that as a hang. See
[06 — Risks and operations](06-risks-and-ops.md) for rate limits, session hygiene, and
WebView recycling.
