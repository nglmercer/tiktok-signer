# Sign server in a container. No browser.
#
# The signer runs the real webmssdk bundle under a synthetic environment in Node, so this image
# carries no WebKit, no GTK, and no virtual display — the previous image needed all three because
# the engine was a real browser. See docs/07-deploy.md.

# `rust-version` is 1.82, but the locked dependency tree needs more than the workspace does: the
# 2024 edition (1.85) and `time` 0.3.55 (1.88). Pin the builder above both.
FROM rust:1.90-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p ttl-sign-server \
        --bin ttl-sign-headless-server --features headless


# Node 22: the signer uses `Headers.getSetCookie`, which needs 19.7 or newer.
FROM node:22-bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

# The signing bundle is a public static asset and is deliberately not vendored into the repository.
# Fetching it at build time pins one version into the image; override TTL_BUNDLE to mount another.
ARG WEBMSSDK_URL=https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
RUN curl -fsS -o /opt/webmssdk.js "$WEBMSSDK_URL"

COPY --from=builder /src/target/release/ttl-sign-headless-server /usr/local/bin/ttl-sign-server
COPY scripts/headless /opt/headless

ENV TTL_BIND=0.0.0.0:8080 \
    TTL_BUNDLE=/opt/webmssdk.js \
    TTL_SIGN_SCRIPT=/opt/headless/sign-url.mjs \
    HOME=/home/signer

# Create the config directory before declaring the volume: Docker would otherwise create it as
# root, leaving the rest of `~/.config` unwritable for the `signer` user.
RUN useradd --create-home --uid 1000 signer \
    && mkdir -p /home/signer/.config/ttl-signer \
    && chown -R signer:signer /home/signer

USER signer
EXPOSE 8080

# The server's graceful shutdown is wired to `tokio::signal::ctrl_c`, which is SIGINT only.
# `docker stop` sends SIGTERM by default, which the process would take as an unhandled kill;
# asking for SIGINT instead runs the real shutdown path.
STOPSIGNAL SIGINT

# `/webcast/im/fetch/` refuses guests, so the session file is required rather than optional: mount
# it read-only at /home/signer/.config/ttl-signer/session as a cookie header containing sessionid.
VOLUME ["/home/signer/.config/ttl-signer"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

CMD ["ttl-sign-server"]
