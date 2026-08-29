#!/usr/bin/env bash
set -euo pipefail

echo "========================================================"
echo "    Community AI: Android Shared Library (JNI) Build    "
echo "========================================================"

# Check if cargo-ndk is installed
if ! command -v cargo-ndk &> /dev/null; then
    echo "Installing cargo-ndk tool..."
    cargo install cargo-ndk
fi

# Add rust Android cross-compilation targets
echo "Adding Rust Android targets..."
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android

OUTPUT_DIR="platform/android/jniLibs"
mkdir -p "$OUTPUT_DIR"

echo "Compiling community-ffi for Android ARM64 and x86_64..."

# Compile for ARM64 (modern phones) and x86_64 (emulators)
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 -o "$OUTPUT_DIR" build --release -p community-ffi

echo "========================================================"
echo "✅ Android Shared Libraries (.so) built successfully!"
echo "Outputs placed in: $OUTPUT_DIR"
echo "Structure:"
echo "  $OUTPUT_DIR/arm64-v8a/libcommunity_ffi.so"
echo "  $OUTPUT_DIR/armeabi-v7a/libcommunity_ffi.so"
echo "  $OUTPUT_DIR/x86_64/libcommunity_ffi.so"
echo "========================================================"
