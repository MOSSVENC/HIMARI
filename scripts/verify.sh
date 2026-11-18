#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# Prefer the rustup-managed toolchain when the login shell has /usr/bin first.
if [[ -d "$HOME/.cargo/bin" ]]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

echo "==> rustfmt"
cargo fmt --all -- --check

echo "==> unsafe boundary"
./scripts/check-unsafe.sh

echo "==> host tests"
cargo test --locked --workspace
