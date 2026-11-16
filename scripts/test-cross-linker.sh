#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
export PATH="$HOME/.cargo/bin:$PWD/.cargo:$PATH"
mkdir -p target/cross-linker-smoke
rustc --edition=2024 --target x86_64-pc-windows-gnu \
    -C linker=.cargo/zig-cc \
    scripts/test-cross-linker.rs \
    -o target/cross-linker-smoke/synchronization.exe
echo "Windows synchronization imports linked successfully"
