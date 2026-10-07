# syntax=docker/dockerfile:1
FROM rust:1.96-bookworm AS builder
WORKDIR /illyaverse
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
COPY tools/kaleidoscope ./tools/kaleidoscope
COPY migrations ./migrations
# Refresh this package when restored source timestamps predate the shared cache.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/illyaverse/target,sharing=locked \
    cargo clean --release -p sanctuary && \
    cargo build --locked --release -p sanctuary && install -m 0755 target/release/sanctuary /usr/local/bin/sanctuary

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 illya && useradd --uid 10001 --gid 10001 --no-create-home illya \
    && mkdir -p /data/media /data/maps /data/replays && chown -R 10001:10001 /data
COPY --from=builder /usr/local/bin/sanctuary /usr/local/bin/sanctuary
USER 10001:10001
WORKDIR /data
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/sanctuary"]
