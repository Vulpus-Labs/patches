#!/usr/bin/env bash
# Build a combined HTML report of `just push` phase timings.
#
# Inputs (all under target/cargo-timings/):
#   push-phases.log              one line per phase: "<phase>\t<seconds>"
#   cargo-timing-build.html      cargo --timings output for build phase
#   cargo-timing-test.html       cargo --timings output for test phase
#
# Output:
#   target/cargo-timings/push-report.html

set -euo pipefail

DIR="target/cargo-timings"
LOG="$DIR/push-phases.log"
OUT="$DIR/push-report.html"

if [[ ! -f "$LOG" ]]; then
  echo "no phase log at $LOG" >&2
  exit 1
fi

rows=""
total=0
while IFS=$'\t' read -r phase secs; do
  rows+="<tr><td>${phase}</td><td class=\"num\">${secs}s</td></tr>"
  # bash float add via awk
  total=$(awk -v a="$total" -v b="$secs" 'BEGIN { printf "%.2f", a + b }')
done < "$LOG"

build_link=""
test_link=""
[[ -f "$DIR/cargo-timing-build.html" ]] && build_link='<li><a href="cargo-timing-build.html">cargo build --timings</a></li>'
[[ -f "$DIR/cargo-timing-test.html"  ]] && test_link='<li><a href="cargo-timing-test.html">cargo test --timings</a></li>'

cat > "$OUT" <<HTML
<!doctype html>
<meta charset="utf-8">
<title>just push — timing report</title>
<style>
  body { font: 14px/1.4 system-ui, sans-serif; margin: 2rem; max-width: 60rem; }
  h1 { margin-top: 0; }
  table { border-collapse: collapse; margin: 1rem 0; }
  th, td { padding: 4px 12px; border-bottom: 1px solid #ddd; text-align: left; }
  td.num { text-align: right; font-variant-numeric: tabular-nums; }
  tfoot td { font-weight: bold; border-top: 2px solid #888; border-bottom: none; }
  ul { line-height: 1.8; }
  small { color: #666; }
</style>
<h1>just push — timing report</h1>
<small>generated $(date '+%Y-%m-%d %H:%M:%S')</small>
<h2>Phase wall times</h2>
<table>
  <thead><tr><th>Phase</th><th>Wall time</th></tr></thead>
  <tbody>${rows}</tbody>
  <tfoot><tr><td>total</td><td class="num">${total}s</td></tr></tfoot>
</table>
<h2>Per-crate breakdown</h2>
<ul>
  ${build_link}
  ${test_link}
</ul>
HTML

echo "wrote $OUT"
