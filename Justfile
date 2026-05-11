# Tiered validation profiles. See ADR 0067.
#
# Usage:
#   just inner [-p crate ...]   fast unit tests on inner-loop crate set + extras
#   just commit [-p crate ...]  inner + clippy on the same scope
#   just push                   full workspace build/test/clippy + forbidden-edge lint
#   just smoke                  push + integration / plugin scanner / LSP / CLAP / slow suites
#
# Pass extra crates as raw cargo flags, e.g. `just inner -p patches-svg`.

inner_crates := "-p patches-core -p patches-modules -p patches-dsp -p patches-engine"

# Fast inner loop: per-iteration unit tests. No clippy.
inner *EXTRA:
    cargo test {{inner_crates}} {{EXTRA}}

# Pre-commit: inner + clippy on the touched scope.
commit *EXTRA:
    cargo test {{inner_crates}} {{EXTRA}}
    cargo clippy {{inner_crates}} {{EXTRA}} -- -D warnings

# Pre-push: full workspace gate.
# Phase wall times + per-crate cargo --timings, assembled into
# target/cargo-timings/push-report.html.
push:
    ./tools/run-push.sh

# Smoke: push + slow / integration / plugin scanner / LSP / CLAP suites.
smoke: push
    cargo test -p patches-integration-tests
    cargo test -p patches-clap
    cargo test -p patches-lsp
