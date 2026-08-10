# Sign server in a container.
#
# The engine is a real WebKitGTK browser, so the runtime stage carries GTK, WebKit, and a
# virtual X server. There is no headless mode to fall back on: GTK needs a display even
# when the window is hidden. See docs/07-deploy.md.

# `rust-version` is 1.82, but the locked dependency tree needs more than the workspace
# does: the 2024 edition (1.85) and `time` 0.3.55 (1.88). Pin the builder above both.
FROM rust:1.90-bookworm AS builder

# `libwebkit2gtk-4.1-dev` is what wry links against; protoc is vendored by the build script.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p ttl-sign-server


FROM debian:bookworm-slim AS runtime

# Runtime libraries only. `gstreamer1.0-libav` is deliberately absent: without codecs
# WebKit cannot decode the live video, which saves CPU and bandwidth the signer never
# needed. The GStreamer warnings this produces are harmless.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-0 \
        libgtk-3-0 \
        xvfb \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

# No GPU and no compositor in a container.
ENV WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    LIBGL_ALWAYS_SOFTWARE=1 \
    GDK_BACKEND=x11 \
    DISPLAY=:99 \
    TTL_BIND=0.0.0.0:8080 \
    HOME=/home/signer

# Create the config directory before declaring the volume: Docker would otherwise create
# it as root, leaving the rest of `~/.config` unwritable for the `signer` user.
RUN useradd --create-home --uid 1000 signer \
    && mkdir -p /home/signer/.config/ttl-signer \
    && chown -R signer:signer /home/signer \
    && mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix

COPY --from=builder /src/target/release/ttl-sign-server /usr/local/bin/ttl-sign-server
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

USER signer
EXPOSE 8080

# The server's graceful shutdown is wired to `tokio::signal::ctrl_c`, which is SIGINT only.
# `docker stop` sends SIGTERM by default, which the process would take as an unhandled
# kill; asking for SIGINT instead runs the real shutdown path.
STOPSIGNAL SIGINT

# The session file is mounted read-only; a guest identity needs no volume at all.
VOLUME ["/home/signer/.config/ttl-signer"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=60s \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["ttl-sign-server"]
