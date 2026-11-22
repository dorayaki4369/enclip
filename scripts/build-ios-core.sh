#!/bin/sh
# iOS アプリが使う Rust 中核（crates/enclip-ffi）を XCFramework と Swift のバインディングにする。
# 出力: apps/ios/Frameworks/EnclipCore.xcframework、apps/ios/Shared/Generated/enclip_ffi.swift
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

PROFILE=release
LIB=libenclip_ffi.a
OUT_DIR="$ROOT/apps/ios"
WORK="$ROOT/target/ios-core"

cargo build -p enclip-ffi --lib --profile "$PROFILE" --target aarch64-apple-ios
cargo build -p enclip-ffi --lib --profile "$PROFILE" --target aarch64-apple-ios-sim

rm -rf "$WORK"
mkdir -p "$WORK/bindings" "$WORK/headers"
cargo run -q -p enclip-ffi --features bindgen --bin uniffi-bindgen -- generate \
  --library "target/aarch64-apple-ios/$PROFILE/$LIB" --language swift --out-dir "$WORK/bindings"

cp "$WORK/bindings/enclip_ffiFFI.h" "$WORK/headers/"
cp "$WORK/bindings/enclip_ffiFFI.modulemap" "$WORK/headers/module.modulemap"

rm -rf "$OUT_DIR/Frameworks/EnclipCore.xcframework"
mkdir -p "$OUT_DIR/Frameworks" "$OUT_DIR/Shared/Generated"
xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/$PROFILE/$LIB" -headers "$WORK/headers" \
  -library "target/aarch64-apple-ios-sim/$PROFILE/$LIB" -headers "$WORK/headers" \
  -output "$OUT_DIR/Frameworks/EnclipCore.xcframework"
cp "$WORK/bindings/enclip_ffi.swift" "$OUT_DIR/Shared/Generated/enclip_ffi.swift"
echo "built $OUT_DIR/Frameworks/EnclipCore.xcframework"
