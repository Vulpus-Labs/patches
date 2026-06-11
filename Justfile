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

# Fast inner loop: per-iteration unit tests. No clippy. No doctests.
inner *EXTRA:
    cargo test --tests {{inner_crates}} {{EXTRA}}

# Pre-commit: inner + clippy on the touched scope. No doctests.
commit *EXTRA:
    cargo test --tests {{inner_crates}} {{EXTRA}}
    cargo clippy {{inner_crates}} {{EXTRA}} -- -D warnings
    @just _sweep

# Pre-push: full workspace gate.
# Phase wall times + per-crate cargo --timings, assembled into
# target/cargo-timings/push-report.html.
push:
    ./tools/run-push.sh
    @just _sweep

# Smoke: push + expensive suites push doesn't run. No doctests anywhere.
#
# `push` already runs `cargo test --workspace --tests` (see run-push.sh),
# which covers integration-tests / clap / lsp with default features — so
# re-running them here added nothing (ticket 1002). ADR 0067 defines smoke
# as push + *extra*. The genuine extra is the allocator-trap run: the
# integration suite with `audio-thread-allocator-trap` armed, which aborts
# if any module allocates on the audio thread (ticket 0997). This is the
# only tier that self-detects that defect class.
smoke: push
    cargo test --tests -p patches-integration-tests --features audio-thread-allocator-trap
    @just _sweep

# Mutation testing on patches-planner (E161 / 0984). Wrapper traps
# EXIT/INT/TERM and removes mutants.out/ on every exit path; pass
# --keep-output to retain artefacts for triage.
# Install: cargo install cargo-mutants. See MUTANTS.md.
mutants *EXTRA:
    ./tools/run-mutants.sh {{EXTRA}}

# Manual sweep: drop target/ artefacts not touched in 7 days.
# Keeps hot incremental cache; reclaims stale test-bin churn.
sweep:
    @just _sweep

_sweep:
    @command -v cargo-sweep >/dev/null 2>&1 \
        && cargo sweep --time 7 \
        || echo "cargo-sweep not installed — skipping. Install: cargo install cargo-sweep"
