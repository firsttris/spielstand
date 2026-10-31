# syntax=docker/dockerfile:1

# The web UI, built once on the build machine's platform.
FROM --platform=$BUILDPLATFORM node:26-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/pnpm-lock.yaml ./
# Node 25 and later no longer ship corepack: install the pnpm of "packageManager" with npm
RUN npm install -g "$(node -p 'require("./package.json").packageManager')"
RUN --mount=type=cache,id=pnpm,target=/root/.local/share/pnpm/store pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# Server and agent for the target platform. The agent is linked against the glibc of Debian
# bookworm (2.36), older than that of Bazzite, SteamOS or any current desktop distribution.
FROM rust:1-bookworm AS rust
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY install/ install/
COPY --from=web /src/web/dist web/dist
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-target,target=/src/target \
    cargo build --release --locked -p spielstand-server -p spielstand-agent \
 && mkdir -p /out \
 && cp target/release/spielstand-server /out/ \
 && cp target/release/spielstand-agent "/out/spielstand-agent-$(uname -m)-linux"

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates rclone tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --home-dir /data spielstand \
 && mkdir -p /data && chown spielstand /data
COPY --from=rust /out/spielstand-server /usr/local/bin/
# Served at /download/agent/<arch>; the other architecture comes from the GitHub release.
COPY --from=rust /out/spielstand-agent-* /usr/share/spielstand/agent/
ENV SPIELSTAND_DATA=/data \
    SPIELSTAND_LISTEN=0.0.0.0:8467 \
    SPIELSTAND_AGENT_DIR=/usr/share/spielstand/agent \
    RCLONE_CONFIG=/data/rclone/rclone.conf
USER spielstand
VOLUME /data
EXPOSE 8467
HEALTHCHECK --interval=30s --timeout=5s CMD ["spielstand-server", "health"]
ENTRYPOINT ["tini", "--", "spielstand-server"]
CMD ["serve"]
