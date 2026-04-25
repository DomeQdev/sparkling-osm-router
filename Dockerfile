# syntax=docker/dockerfile:1.7

# ---------- Build stage ----------
FROM rust:1.83-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        git \
        ca-certificates \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY rs ./rs
COPY server ./server

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release -p sparkling-osm-router-server && \
    cp target/release/sparkling-osm-router-server /usr/local/bin/sparkling-osm-router-server

# ---------- Runtime stage ----------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/local/bin/sparkling-osm-router-server /usr/local/bin/sparkling-osm-router-server

RUN mkdir -p /data
VOLUME ["/data"]

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/sparkling-osm-router-server"]
