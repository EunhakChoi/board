# charet-server with the built web tool.

FROM node:24-trixie-slim AS web
RUN npm install --global pnpm@12.5.1
WORKDIR /src
COPY . .
RUN pnpm install --frozen-lockfile --filter "charet-web..." && pnpm --filter charet-web run build

FROM rust:1.99-trixie AS build
WORKDIR /src
COPY . .
# Build with the image's toolchain, not a fresh download of "stable".
RUN rm rust-toolchain.toml
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p charet-server && cp target/release/charet-server /charet-server

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data charet \
    && mkdir -p /data && chown charet /data
COPY --from=build /charet-server /usr/local/bin/charet-server
COPY --from=web /src/apps/charet-web/dist /usr/local/share/charet/web
USER charet
ENV CHARET_HOME=/data \
    HOME=/data \
    CHARET_BIND=0.0.0.0:5280 \
    CHARET_PREVIEW_BIND=0.0.0.0:5281 \
    CHARET_REMOTE_BIND=0.0.0.0:5284 \
    CHARET_CONTAINER=true
EXPOSE 5280 5281 5284
VOLUME /data
CMD ["charet-server"]
