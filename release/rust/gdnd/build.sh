#!/bin/bash
# GDND Release Build Script
#
# Builds the GDND binary and gpu-check from source, copies to release directory.
#
# Usage:
#   ./build.sh              # Build release binaries
#   ./build.sh --docker     # Build Docker image
#   ./build.sh --clean      # Clean build artifacts

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
SRC_DIR="${PROJECT_ROOT}/src/rust/gdnd"
RELEASE_DIR="${SCRIPT_DIR}"
BIN_DIR="${RELEASE_DIR}/bin"

# Parse arguments
BUILD_DOCKER=false
CLEAN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --docker)
            BUILD_DOCKER=true
            shift
            ;;
        --clean)
            CLEAN=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--docker] [--clean] [--help]"
            echo ""
            echo "Options:"
            echo "  --docker    Build Docker image after compiling"
            echo "  --clean     Clean build artifacts"
            echo "  --help      Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [ "$CLEAN" = true ]; then
    echo "Cleaning build artifacts..."
    rm -rf "${BIN_DIR}"
    cd "${SRC_DIR}" && cargo clean
    echo "Clean complete."
    exit 0
fi

echo "=== GDND Release Build ==="
echo "Source:  ${SRC_DIR}"
echo "Release: ${RELEASE_DIR}"

# Create bin directory
mkdir -p "${BIN_DIR}"

# Build Rust binary
echo ""
echo "Building Rust binary..."
cd "${SRC_DIR}"
cargo build --release --package gdnd

# Copy binary
cp "${SRC_DIR}/target/release/gdnd" "${BIN_DIR}/gdnd"
echo "Binary copied to: ${BIN_DIR}/gdnd"

# Build gpu-check if CUDA is available
if command -v nvcc &> /dev/null; then
    echo ""
    echo "Building gpu-check CUDA binary..."
    cd "${SRC_DIR}/gpu-check"
    ./build.sh
    cp "${SRC_DIR}/gpu-check/bin/gpu-check" "${BIN_DIR}/gpu-check"
    echo "gpu-check copied to: ${BIN_DIR}/gpu-check"
else
    echo ""
    echo "Warning: nvcc not found, skipping gpu-check build"
fi

# Show results
echo ""
echo "=== Build Complete ==="
ls -lh "${BIN_DIR}"

# Build Docker image if requested
if [ "$BUILD_DOCKER" = true ]; then
    echo ""
    echo "Building Docker image..."
    cd "${RELEASE_DIR}"
    docker build -t gdnd:latest -f deploy/Dockerfile "${SRC_DIR}"
    echo ""
    echo "Docker image built: gdnd:latest"
    docker images gdnd:latest
fi

echo ""
echo "Release artifacts in: ${RELEASE_DIR}"
