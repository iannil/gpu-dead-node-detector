#!/bin/bash
#
# Build script for npu-check AscendCL micro-benchmark
#
# Prerequisites:
# - CANN Toolkit installed (provides AscendCL)
# - Environment variables set: ASCEND_HOME, LD_LIBRARY_PATH
#
# Usage:
#   ./build.sh           # Build with default settings
#   ./build.sh --debug   # Build with debug symbols
#   ./build.sh --clean   # Clean build artifacts

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"
OUTPUT_BIN="npu-check"

# Default CANN paths
ASCEND_HOME="${ASCEND_HOME:-/usr/local/Ascend/ascend-toolkit/latest}"
ASCEND_AICPU_PATH="${ASCEND_HOME}"
ASCEND_OPP_PATH="${ASCEND_HOME}/opp"

# Compiler settings
CXX="${CXX:-g++}"
CXXFLAGS="-std=c++11 -Wall -Wextra"
DEBUG_FLAGS="-g -O0 -DDEBUG"
RELEASE_FLAGS="-O2 -DNDEBUG"

# Parse arguments
BUILD_TYPE="release"
CLEAN=0

while [[ $# -gt 0 ]]; do
    case $1 in
        --debug)
            BUILD_TYPE="debug"
            shift
            ;;
        --clean)
            CLEAN=1
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--debug] [--clean] [--help]"
            echo ""
            echo "Options:"
            echo "  --debug    Build with debug symbols"
            echo "  --clean    Clean build artifacts"
            echo "  --help     Show this help"
            echo ""
            echo "Environment variables:"
            echo "  ASCEND_HOME    Path to CANN toolkit (default: /usr/local/Ascend/ascend-toolkit/latest)"
            echo "  CXX            C++ compiler (default: g++)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Clean if requested
if [[ $CLEAN -eq 1 ]]; then
    echo "Cleaning build artifacts..."
    rm -rf "${BUILD_DIR}"
    rm -f "${SCRIPT_DIR}/${OUTPUT_BIN}"
    echo "Clean complete."
    exit 0
fi

# Check for CANN toolkit
if [[ ! -d "${ASCEND_HOME}" ]]; then
    echo "Error: CANN toolkit not found at ${ASCEND_HOME}"
    echo "Please install CANN toolkit or set ASCEND_HOME environment variable"
    exit 1
fi

# Check for AscendCL header
ACL_INCLUDE="${ASCEND_HOME}/include"
if [[ ! -f "${ACL_INCLUDE}/acl/acl.h" ]]; then
    echo "Error: AscendCL header not found at ${ACL_INCLUDE}/acl/acl.h"
    exit 1
fi

# Check for AscendCL library
ACL_LIB="${ASCEND_HOME}/lib64"
if [[ ! -f "${ACL_LIB}/libascendcl.so" ]]; then
    # Try alternative path
    ACL_LIB="${ASCEND_HOME}/acllib/lib64"
    if [[ ! -f "${ACL_LIB}/libascendcl.so" ]]; then
        echo "Error: AscendCL library not found"
        exit 1
    fi
fi

# Create build directory
mkdir -p "${BUILD_DIR}"

# Set compiler flags based on build type
if [[ "${BUILD_TYPE}" == "debug" ]]; then
    CXXFLAGS="${CXXFLAGS} ${DEBUG_FLAGS}"
    echo "Building in debug mode..."
else
    CXXFLAGS="${CXXFLAGS} ${RELEASE_FLAGS}"
    echo "Building in release mode..."
fi

# Compile
echo "Compiling npu_check.cpp..."
${CXX} ${CXXFLAGS} \
    -I"${ACL_INCLUDE}" \
    -L"${ACL_LIB}" \
    -o "${BUILD_DIR}/${OUTPUT_BIN}" \
    "${SCRIPT_DIR}/npu_check.cpp" \
    -lascendcl \
    -lrt \
    -Wl,-rpath,"${ACL_LIB}"

# Copy to script directory
cp "${BUILD_DIR}/${OUTPUT_BIN}" "${SCRIPT_DIR}/${OUTPUT_BIN}"

echo ""
echo "Build complete: ${SCRIPT_DIR}/${OUTPUT_BIN}"
echo ""
echo "To install:"
echo "  sudo cp ${SCRIPT_DIR}/${OUTPUT_BIN} /usr/local/bin/"
echo ""
echo "To test:"
echo "  ./${OUTPUT_BIN} -v"
