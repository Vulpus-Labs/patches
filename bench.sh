#!/usr/bin/env bash
# bench.sh — build and run the headless timing benchmark.
#
# Usage:
#   ./bench.sh                         run and print results
#   ./bench.sh save <label>            run and save results to bench-results/<label>.txt
#   ./bench.sh compare <label>         run and diff against a saved result
#
# Results are stored in bench-results/ (gitignored by default).
set -euo pipefail

PATCH="${BENCH_PATCH:-examples/poly_synth_layered.patches}"
RESULTS_DIR="bench-results"

build() {
    echo "==> cargo build --release -p patches-player --bin bench"
    cargo build --release -p patches-player --bin bench 2>&1 | grep -v "^$" || true
}

run_bench() {
    ./target/release/bench "$PATCH"
}

cmd="${1:-run}"

case "$cmd" in
    run)
        build
        echo
        run_bench
        ;;

    save)
        label="${2:?usage: ./bench.sh save <label>}"
        mkdir -p "$RESULTS_DIR"
        out="$RESULTS_DIR/$label.txt"
        build
        echo
        run_bench | tee "$out"
        echo
        echo "==> saved to $out"
        ;;

    compare)
        label="${2:?usage: ./bench.sh compare <label>}"
        baseline="$RESULTS_DIR/$label.txt"
        if [[ ! -f "$baseline" ]]; then
            echo "error: no saved result at $baseline" >&2
            exit 1
        fi
        build
        echo
        current=$(run_bench)
        echo "$current"
        echo
        echo "==> diff vs $label:"
        diff "$baseline" <(echo "$current") || true
        ;;

    *)
        echo "usage: $0 [run|save <label>|compare <label>]" >&2
        exit 1
        ;;
esac
