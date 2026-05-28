# syntax=docker/dockerfile:1
# ─────────────────────────────────────────────────────────────────────────────
# Stage 0 — Get Cross-Compilation Helpers
# ─────────────────────────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM tonistiigi/xx AS xx

# ─────────────────────────────────────────────────────────────────────────────
# Stage 1 — Builder
# ─────────────────────────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM rust:1.95-bookworm AS builder
COPY --from=xx / /

# Disable CPU Jitter entropy in aws-lc-sys to avoid cross-compilation errors
ENV AWS_LC_SYS_NO_JITTER_ENTROPY=1
ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /build

# Host dependencies for compilation
RUN apt-get update && apt-get install -y --no-install-recommends \
    clang \
    lld \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy cargo manifests
COPY Cargo.toml Cargo.lock ./

# Create stub directories
RUN mkdir -p \
    crates/graphon-core/src \
    crates/graphon-application/src \
    crates/graphon-infrastructure/src \
    crates/graphon-server/src

COPY crates/graphon-core/Cargo.toml           crates/graphon-core/Cargo.toml
COPY crates/graphon-application/Cargo.toml    crates/graphon-application/Cargo.toml
COPY crates/graphon-infrastructure/Cargo.toml crates/graphon-infrastructure/Cargo.toml
COPY crates/graphon-server/Cargo.toml         crates/graphon-server/Cargo.toml

# Minimal stubs to pre-compile dependencies
RUN echo "fn main() {}" > crates/graphon-server/src/main.rs && \
    echo "pub fn init() {}" > crates/graphon-core/src/lib.rs && \
    echo "pub fn init() {}" > crates/graphon-application/src/lib.rs && \
    echo "pub fn init() {}" > crates/graphon-infrastructure/src/lib.rs

# Setup target platform
ARG TARGETPLATFORM
RUN apt-get update && xx-apt-get install -y --no-install-recommends \
    gcc \
    libc6-dev \
    && rm -rf /var/lib/apt/lists/*

RUN xx-clang --setup-target-triple

# Pre-compile dependencies
RUN PKG_CONFIG=xx-pkg-config xx-cargo build --release -p graphon-server 2>&1 | tail -5 || true

# Remove stub build artifacts to force rebuild of real code
RUN rm -rf \
    target/*/release/.fingerprint/graphon-* \
    target/*/release/deps/graphon_* \
    target/*/release/graphon-*

# Copy actual source code
COPY crates/ crates/

# Build real code
RUN PKG_CONFIG=xx-pkg-config xx-cargo build --release -p graphon-server && \
    cp target/$(xx-cargo --print-target-triple)/release/graphon-server ./graphon-server

# ─────────────────────────────────────────────────────────────────────────────
# Stage 2 — Runtime
# ─────────────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy build output
COPY --from=builder /build/graphon-server /app/graphon-server

# Run as non-root user
RUN useradd --system --uid 1001 --no-create-home graphon && \
    mkdir -p /data && chown graphon:graphon /data
USER graphon

ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

ENTRYPOINT ["/app/graphon-server"]
