# Multi-stage Containerfile for rvtools2vse
# Works with podman build and docker build
# Run from the root of your rvtools2vse clone:
# Build: podman build -t rvtools2vse:latest -f Containerfile .
# Or:    docker build -t rvtools2vse:latest -f Containerfile .

# Builder and runtime are both Debian bookworm, so the binary's glibc matches.
# rust:1-bookworm tracks the newest stable Rust (needs 1.88+) on bookworm.
FROM rust:1-bookworm AS builder

WORKDIR /src

# Build from your local clone, so a git pull plus rebuild always picks up changes
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked \
    && strip target/release/rvtools2vse

# Runtime stage: small image with just the binary and needed libs
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libgcc-s1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1000 --shell /bin/bash appuser

COPY --from=builder /src/target/release/rvtools2vse /usr/local/bin/rvtools2vse

USER appuser
WORKDIR /data

ENTRYPOINT ["rvtools2vse"]
CMD ["--help"]
