#!/usr/bin/env bash
# Run the `just push` validation gate with per-phase timing.
#
# Emits:
#   target/cargo-timings/push-phases.log         tab-separated <phase>\t<seconds>
#   target/cargo-timings/cargo-timing-build.html cargo --timings (build phase)
#   target/cargo-timings/cargo-timing-test.html  cargo --timings (test phase)
#   target/cargo-timings/push-report.html        combined report
#
# Exits non-zero on the first failing phase, but always tries to write a
# partial report so you can see what completed.

set -uo pipefail

DIR="target/cargo-timings"
mkdir -p "$DIR"
LOG="$DIR/push-phases.log"
: > "$LOG"

EXIT=0

run_phase() {
  local name="$1"; shift
  local log="$DIR/push-$name.log"
  echo "==> $name (logging to $log)"
  local start end secs
  start=$(python3 -c 'import time; print(time.time())')
  # Stream stdout+stderr to log; surface only failures verbatim.
  "$@" > "$log" 2>&1
  local rc=$?
  end=$(python3 -c 'import time; print(time.time())')
  secs=$(python3 -c "print(f'{${end} - ${start}:.2f}')")
  printf '%s\t%s\n' "$name" "$secs" >> "$LOG"
  if [[ $rc -ne 0 ]]; then
    EXIT=$rc
    echo "phase '$name' FAILED (exit $rc) — tail of $log:" >&2
    tail -n 80 "$log" >&2
    return $rc
  fi
  echo "    done in ${secs}s"
}

# Phase 1: build
if run_phase build cargo build --workspace --timings; then
  cp "$DIR/cargo-timing.html" "$DIR/cargo-timing-build.html"
fi

# Phase 2: test (only if build passed)
if [[ $EXIT -eq 0 ]] && run_phase test cargo test --workspace --timings; then
  cp "$DIR/cargo-timing.html" "$DIR/cargo-timing-test.html"
fi

# Phase 3: clippy
[[ $EXIT -eq 0 ]] && run_phase clippy cargo clippy --workspace --all-targets -- -D warnings

# Phase 4: forbidden-edges
[[ $EXIT -eq 0 ]] && run_phase forbid cargo run -q -p patches-forbidden-edges --bin forbidden-edges

# Always write the report from whatever phases completed.
tools/push-report.sh || true

exit $EXIT
