#!/usr/bin/env bash
# Bootstrap optional dev-loop speedups. Idempotent; safe to re-run.
#
# Wires:
#   - .cargo/config.toml      copied from .example if lld present (60×+ link speedup)
#   - cargo-sweep             installed if absent (used by `just sweep`, commit/push/smoke)
#
# Anything missing prints an install hint and is skipped, not fatal.

set -uo pipefail

cd "$(dirname "$0")/.."

echo "==> dev setup"

# 1. lld linker + cargo config
if [[ -f .cargo/config.toml ]]; then
    echo "    .cargo/config.toml already present — leaving it alone"
elif command -v ld64.lld >/dev/null 2>&1; then
    cp .cargo/config.toml.example .cargo/config.toml
    echo "    wrote .cargo/config.toml (lld detected at $(command -v ld64.lld))"
else
    cat <<EOF
    skip: ld64.lld not found. To enable 60×+ faster linking on macOS arm64:
        brew install lld
        cp .cargo/config.toml.example .cargo/config.toml
EOF
fi

# 2. cargo-sweep
if command -v cargo-sweep >/dev/null 2>&1; then
    echo "    cargo-sweep already installed ($(cargo-sweep --version))"
else
    echo "    installing cargo-sweep..."
    cargo install cargo-sweep
fi

echo "==> done"
