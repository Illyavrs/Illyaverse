# syntax=docker/dockerfile:1
FROM rust:1.96-bookworm AS builder
WORKDIR /illyaverse
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
COPY tools/kaleidoscope ./tools/kaleidoscope
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/illyaverse/target,sharing=locked \
    cargo build --locked --release -p resonance && install -m 0755 target/release/resonance /usr/local/bin/resonance

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 illya && useradd --uid 10001 --gid 10001 --no-create-home illya \
    && mkdir -p /data && chown 10001:10001 /data
COPY --from=builder /usr/local/bin/resonance /usr/local/bin/resonance
USER 10001:10001
WORKDIR /data
EXPOSE 8081
ENTRYPOINT ["/usr/local/bin/resonance"]
