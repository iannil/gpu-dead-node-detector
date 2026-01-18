#!/bin/bash
# Build script for gpu-check CUDA binary
#
# Requirements:
# - CUDA Toolkit (nvcc compiler)
# - gcc/g++
#
# Usage:
#   ./build.sh              # Build for current architecture
#   ./build.sh --all        # Build for multiple architectures

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="${SCRIPT_DIR}/bin"
OUTPUT_BIN="${OUTPUT_DIR}/gpu-check"

# CUDA architecture targets
# Default: Volta (V100), Turing (T4), Ampere (A100, A10), Hopper (H100)
CUDA_ARCHS="70;75;80;86;90"

# Compiler flags
NVCC_FLAGS="-O3 -lineinfo"
LDFLAGS="-lcudart"

# Parse arguments
BUILD_ALL=false
DEBUG=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --all)
            BUILD_ALL=true
            shift
            ;;
        --debug)
            DEBUG=true
            NVCC_FLAGS="-g -G -O0"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--all] [--debug] [--help]"
            echo ""
            echo "Options:"
            echo "  --all     Build for all supported GPU architectures"
            echo "  --debug   Build with debug symbols"
            echo "  --help    Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Check for nvcc
if ! command -v nvcc &> /dev/null; then
    echo "Error: nvcc not found. Please install CUDA Toolkit."
    exit 1
fi

# Create output directory
mkdir -p "${OUTPUT_DIR}"

# Get CUDA version
CUDA_VERSION=$(nvcc --version | grep "release" | sed -E 's/.*release ([0-9]+\.[0-9]+).*/\1/')
echo "CUDA Version: ${CUDA_VERSION}"

# Build architecture flags
if [ "$BUILD_ALL" = true ]; then
    ARCH_FLAGS=""
    IFS=';' read -ra ARCHS <<< "$CUDA_ARCHS"
    for arch in "${ARCHS[@]}"; do
        ARCH_FLAGS="${ARCH_FLAGS} -gencode arch=compute_${arch},code=sm_${arch}"
    done
    # Add PTX for forward compatibility
    LAST_ARCH="${ARCHS[-1]}"
    ARCH_FLAGS="${ARCH_FLAGS} -gencode arch=compute_${LAST_ARCH},code=compute_${LAST_ARCH}"
else
    # Auto-detect current GPU architecture
    if command -v nvidia-smi &> /dev/null; then
        GPU_ARCH=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -1 | tr -d '.')
        if [ -n "$GPU_ARCH" ]; then
            ARCH_FLAGS="-arch=sm_${GPU_ARCH}"
            echo "Detected GPU architecture: sm_${GPU_ARCH}"
        fi
    fi

    # Fallback to sm_70 if detection failed
    if [ -z "$ARCH_FLAGS" ]; then
        ARCH_FLAGS="-arch=sm_70"
        echo "Using default architecture: sm_70"
    fi
fi

echo "Building gpu-check..."
echo "  Source: ${SCRIPT_DIR}/gpu_check.cu"
echo "  Output: ${OUTPUT_BIN}"
echo "  Flags: ${NVCC_FLAGS} ${ARCH_FLAGS}"

nvcc ${NVCC_FLAGS} ${ARCH_FLAGS} \
    -o "${OUTPUT_BIN}" \
    "${SCRIPT_DIR}/gpu_check.cu" \
    ${LDFLAGS}

# Set executable permissions
chmod +x "${OUTPUT_BIN}"

# Print binary info
echo ""
echo "Build complete!"
ls -lh "${OUTPUT_BIN}"

# Show linked libraries
echo ""
echo "Linked libraries:"
ldd "${OUTPUT_BIN}" 2>/dev/null || otool -L "${OUTPUT_BIN}" 2>/dev/null || true

# Quick test (if GPU available)
if command -v nvidia-smi &> /dev/null && nvidia-smi &> /dev/null; then
    echo ""
    echo "Running quick test..."
    if "${OUTPUT_BIN}" -v; then
        echo "Test passed!"
    else
        echo "Test failed with exit code: $?"
    fi
fi
