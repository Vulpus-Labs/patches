#!/usr/bin/env bash
# Run cargo-mutants against patches-planner with strict cleanup discipline
# (E161 / ticket 0984).
#
# cargo-mutants writes per-mutation logs, the working diff, and JSON
# outcomes into ./mutants.out/ at the repo root. Worker mode also stages
# transient build directories under $TMPDIR. This wrapper guarantees both
# locations are cleaned on every exit path (success, failure, surviving
# mutations, SIGINT, SIGTERM, per-mutation timeout) so a mutation pass
# never leaves stale artefacts behind to silently consume disk.
#
# Usage:
#   tools/run-mutants.sh                         # full pass on patches-planner
#   tools/run-mutants.sh --shard 0/4             # shard 1/4 of the pass
#   tools/run-mutants.sh --file 'state/alloc.rs' # subset by source file
#   tools/run-mutants.sh --keep-output           # skip the cleanup trap
#                                                #   (for triage iteration)
#
# Any extra arguments are forwarded to `cargo mutants` ahead of the
# fixed `-- --test properties` test-selector.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

KEEP_OUTPUT=0
EXTRA_ARGS=()
for arg in "$@"; do
    case "$arg" in
        --keep-output) KEEP_OUTPUT=1 ;;
        *)             EXTRA_ARGS+=("$arg") ;;
    esac
done

cleanup() {
    local code=$?
    if [[ "$KEEP_OUTPUT" == "1" ]]; then
        printf '\n[run-mutants] --keep-output: retaining mutants.out/ for triage (exit %d)\n' "$code"
        return $code
    fi
    if [[ -d "$REPO_ROOT/mutants.out" ]]; then
        printf '\n[run-mutants] removing mutants.out/ (exit %d)\n' "$code"
        rm -rf "$REPO_ROOT/mutants.out" "$REPO_ROOT/mutants.out.old"
    fi
    # cargo-mutants worker mode writes transient build copies into
    # $TMPDIR/cargo-mutants-*.tmp. The tool removes them on its own
    # normal exit; a Ctrl-C during a worker copy can leave one behind.
    # Best-effort sweep: only directories matching the exact prefix.
    if [[ -n "${TMPDIR:-}" ]]; then
        find "$TMPDIR" -maxdepth 1 -type d -name 'cargo-mutants-*.tmp' \
            -mmin -240 -exec rm -rf {} + 2>/dev/null || true
    fi
    return $code
}

trap cleanup EXIT
trap 'trap - EXIT; cleanup; exit 130' INT
trap 'trap - EXIT; cleanup; exit 143' TERM

# Default: default-worker mode with `--jobs 4` (chosen by the measurement
# in MUTANTS.md). Per-mutation timeout 180s — covers the slow
# `replay_holds_single_build_invariants` property without letting a
# runaway hang the pass. No `--` test-selector: every patches-planner
# test runs as the kill surface (properties target + the unit tests
# under src/). The lock_in / structural / partition / backplane_bind /
# state suites are load-bearing for `classify_nodes` and `allocate_buffers`
# — restricting to `--test properties` previously hid ~9 mutations the
# unit tests already cover.
#
# `cargo mutants` exits non-zero when mutations survive; capture the
# code, let the trap run, and exit with it. Avoid `exec` — that
# replaces this shell and the trap never fires.
cargo mutants \
    -p patches-planner \
    --jobs 4 \
    --timeout 180 \
    "${EXTRA_ARGS[@]}"
RC=$?
exit $RC
