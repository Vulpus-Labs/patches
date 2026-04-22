#!/usr/bin/env bash
# FFI audio-path grep gate (ADR 0045 Spike 7 Phase F, ticket 0627).
#
# Asserts that the three audio-thread entry points in
# `patches-ffi/src/loader.rs` never construct heap allocations and never
# reach the JSON codec. A naive intra-function grep is sufficient because
# the ABI functions are small and the real no-alloc enforcement is the
# allocator trap in patches-alloc-trap.
#
# Exits non-zero on violation; designed to run in CI and via the
# integration test `ffi_grep_gate` in patches-ffi/tests.

set -euo pipefail

FILE="${1:-patches-ffi/src/loader.rs}"

if [[ ! -f "$FILE" ]]; then
    echo "ffi-grep-gate: cannot read $FILE" >&2
    exit 2
fi

FNS=("update_validated_parameters" "set_ports" "process")
FORBIDDEN=(
    "json::"
    "Vec::new"
    "Vec::with_capacity"
    "Box::new"
    "String::"
)

fail=0

extract_fn() {
    local fname="$1"
    awk -v fname="$fname" '
        $0 ~ ("fn "fname"\\(") { depth = 0; inside = 1 }
        inside {
            print
            for (i = 1; i <= length($0); i++) {
                c = substr($0, i, 1)
                if (c == "{") depth++
                else if (c == "}") {
                    depth--
                    if (depth == 0) { inside = 0; exit }
                }
            }
        }
    ' "$FILE"
}

for fn in "${FNS[@]}"; do
    body="$(extract_fn "$fn")"
    if [[ -z "$body" ]]; then
        echo "ffi-grep-gate: could not locate fn $fn in $FILE" >&2
        fail=1
        continue
    fi
    for pat in "${FORBIDDEN[@]}"; do
        if grep -F -- "$pat" <<<"$body" >/dev/null; then
            echo "ffi-grep-gate: forbidden '$pat' found inside $fn()" >&2
            grep -F -n -- "$pat" <<<"$body" >&2 || true
            fail=1
        fi
    done
done

if [[ $fail -ne 0 ]]; then
    exit 1
fi

echo "ffi-grep-gate: OK — no JSON / alloc in audio entry points"
