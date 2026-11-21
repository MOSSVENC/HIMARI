#!/usr/bin/env bash
set -euo pipefail

# Keep the compiler used by the cross-linker reproducible on Linux x86_64.
version="0.14.1"
checksum="24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c"
install_root="${RUNNER_TEMP:-$HOME/.cache}/himari-zig"
archive="$install_root/zig-$version.tar.xz"
compiler="$install_root/zig-x86_64-linux-$version"
mkdir -p "$install_root"
if [[ ! -x "$compiler/zig" ]]; then
    curl --fail --location --retry 3 \
        "https://ziglang.org/download/$version/zig-x86_64-linux-$version.tar.xz" \
        --output "$archive"
    printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check
    tar -xJf "$archive" -C "$install_root"
fi
"$compiler/zig" version
if [[ -n "${GITHUB_PATH:-}" ]]; then
    printf '%s\n' "$compiler" >> "$GITHUB_PATH"
else
    printf 'ZIG=%s/zig\n' "$compiler"
fi
