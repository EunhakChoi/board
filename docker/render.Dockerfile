# charet-render with Chromium and fonts (Latin, CJK, emoji).

FROM rust:1.99-trixie AS build
WORKDIR /src
COPY . .
# Build with the image's toolchain, not a fresh download of "stable".
RUN rm rust-toolchain.toml
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p charet-render && cp target/release/charet-render /charet-render

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates chromium \
       fonts-noto-core fonts-noto-cjk fonts-noto-color-emoji \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home charet
COPY --from=build /charet-render /usr/local/bin/charet-render
USER charet
ENV CHARET_RENDER_BIND=0.0.0.0:5283 \
    CHARET_CHROMIUM=/usr/bin/chromium
EXPOSE 5283
CMD ["charet-render"]
