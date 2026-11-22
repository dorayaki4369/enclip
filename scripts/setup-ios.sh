#!/bin/sh
# iOS アプリのプロジェクトを用意する。Rust の中核をビルドし、Xcode プロジェクトを生成する。
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
"$ROOT/scripts/build-ios-core.sh"
cd "$ROOT/apps/ios"
nix --extra-experimental-features 'nix-command flakes' shell nixpkgs#xcodegen -c xcodegen generate
