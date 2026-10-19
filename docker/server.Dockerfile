# board-server with the built web tool.

FROM node:24-trixie-slim AS web
RUN npm install --global pnpm@12.5.1
WORKDIR /src
COPY . .
RUN pnpm install --frozen-lockfile --filter "board-web..." && pnpm --filter board-web run build

FROM rust:1.99-trixie AS build
WORKDIR /src
COPY . .
# Build with the image's toolchain, not a fresh download of "stable".
RUN rm rust-toolchain.toml
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p board-server && cp target/release/board-server /board-server

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data board \
    && mkdir -p /data && chown board /data
COPY --from=build /board-server /usr/local/bin/board-server
COPY --from=web /src/apps/board-web/dist /usr/local/share/board/web
USER board
ENV BOARD_HOME=/data \
    HOME=/data \
    BOARD_BIND=0.0.0.0:5280 \
    BOARD_PREVIEW_BIND=0.0.0.0:5281 \
    BOARD_REMOTE_BIND=0.0.0.0:5284 \
    BOARD_CONTAINER=true
EXPOSE 5280 5281 5284
VOLUME /data
CMD ["board-server"]
