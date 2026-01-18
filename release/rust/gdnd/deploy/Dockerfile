# Multi-stage Dockerfile for GDND
# Target image size: < 50MB
#
# Build context should be /src/rust/gdnd (source directory)
# Usage: docker build -t gdnd:latest -f /release/rust/gdnd/deploy/Dockerfile /src/rust/gdnd

# Stage 1: Build Rust binary
FROM rust:1.75-alpine AS rust-builder

RUN apk add --no-cache musl-dev pkgconf openssl-dev openssl-libs-static

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY gdnd/Cargo.toml gdnd/Cargo.toml
COPY gdnd-core/Cargo.toml gdnd-core/Cargo.toml
COPY gdnd-k8s/Cargo.toml gdnd-k8s/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p gdnd/src gdnd-core/src gdnd-k8s/src && \
    echo "fn main() {}" > gdnd/src/main.rs && \
    echo "pub fn dummy() {}" > gdnd-core/src/lib.rs && \
    echo "pub fn dummy() {}" > gdnd-k8s/src/lib.rs

# Build dependencies
RUN cargo build --release --package gdnd 2>/dev/null || true

# Copy actual source code
COPY gdnd/src gdnd/src
COPY gdnd-core/src gdnd-core/src
COPY gdnd-k8s/src gdnd-k8s/src

# Touch source files to invalidate cache
RUN touch gdnd/src/main.rs gdnd-core/src/lib.rs gdnd-k8s/src/lib.rs

# Build final binary
RUN cargo build --release --package gdnd && \
    strip /build/target/release/gdnd

# Stage 2: Build CUDA gpu-check binary
FROM nvidia/cuda:12.2.0-devel-ubuntu22.04 AS cuda-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY gpu-check/gpu_check.cu .

# Build for multiple GPU architectures (V100, T4, A100, A10, H100)
RUN nvcc -O3 \
    -gencode arch=compute_70,code=sm_70 \
    -gencode arch=compute_75,code=sm_75 \
    -gencode arch=compute_80,code=sm_80 \
    -gencode arch=compute_86,code=sm_86 \
    -gencode arch=compute_90,code=sm_90 \
    -gencode arch=compute_90,code=compute_90 \
    -o gpu-check gpu_check.cu && \
    strip gpu-check

# Stage 3: Final minimal image
FROM nvidia/cuda:12.2.0-base-ubuntu22.04

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /usr/share/doc /usr/share/man /usr/share/locale

# Copy binaries
COPY --from=rust-builder /build/target/release/gdnd /usr/local/bin/gdnd
COPY --from=cuda-builder /build/gpu-check /usr/local/bin/gpu-check

# Copy default configuration
COPY configs/config.yaml /etc/gdnd/config.yaml

# Create non-root user
RUN groupadd -r gdnd && useradd -r -g gdnd gdnd

# Expose metrics port
EXPOSE 9100

# Set entrypoint
ENTRYPOINT ["/usr/local/bin/gdnd"]
CMD ["--config", "/etc/gdnd/config.yaml"]
