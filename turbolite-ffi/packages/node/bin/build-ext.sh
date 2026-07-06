#!/bin/sh
# Build the turbolite loadable extension for Node.js with HTTPS support.
#
# Usage: sh bin/build-ext.sh
#
# Builds turbolite-ffi with features: loadable-extension,cli-s3,https,zstd
# Output: turbolite.so (Linux) or turbolite.dylib (macOS) in this directory.
#
# Requirements: cargo must be on PATH (https://rustup.rs)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
NODE_PKG="$(cd "$SCRIPT_DIR/.." && pwd)"
FFI_ROOT="$(cd "$NODE_PKG/../.." && pwd)"
WS_ROOT="$(cd "$FFI_ROOT/.." && pwd)"

# Override ../cinch-target (author-local path) with a standard workspace target.
TARGET_DIR="$WS_ROOT/target"
export CARGO_TARGET_DIR="$TARGET_DIR"

echo "turbolite build-ext: features=loadable-extension,cli-s3,https,zstd"
echo "  crate : $FFI_ROOT"
echo "  target: $TARGET_DIR"

cd "$FFI_ROOT"
cargo build --release --lib --no-default-features \
  --features loadable-extension,cli-s3,https,zstd

# Copy platform-specific binary to Node package root.
if [ -f "$TARGET_DIR/release/libturbolite_ffi.dylib" ]; then
  cp "$TARGET_DIR/release/libturbolite_ffi.dylib" "$NODE_PKG/turbolite.dylib"
  echo "turbolite build-ext: -> $NODE_PKG/turbolite.dylib"
fi
if [ -f "$TARGET_DIR/release/libturbolite_ffi.so" ]; then
  cp "$TARGET_DIR/release/libturbolite_ffi.so" "$NODE_PKG/turbolite.so"
  echo "turbolite build-ext: -> $NODE_PKG/turbolite.so"
fi
