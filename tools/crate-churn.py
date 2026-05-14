#!/usr/bin/env python3
"""Per-crate churn report from git history.

For each top-level workspace dir, report:
  - total commits that touched any file in the dir
  - distinct files touched
  - days since last touch
  - commits in last 7 / 14 / 30 days
  - share of all observed commits (%)
  - mean files touched per commit (within that crate)

Run from repo root: `python3 tools/crate-churn.py [--since YYYY-MM-DD]`
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

# Top-level dirs treated as "crates" (workspace members + adjacent areas).
# Anything else is bucketed as "<other>".
TRACKED_PREFIXES = (
    "patches-",
    "test-plugins",
    "docs",
    "tickets",
    "epics",
    "adr",
    "tools",
)


@dataclass
class CrateStats:
    commits: set[str] = field(default_factory=set)
    files: set[str] = field(default_factory=set)
    file_touches: int = 0  # commit×file pairs
    last_ts: int = 0
    first_ts: int = 0
    by_window: dict[int, int] = field(default_factory=lambda: {7: 0, 14: 0, 30: 0})


def crate_of(path: str) -> str | None:
    head = path.split("/", 1)[0]
    if head.startswith(TRACKED_PREFIXES) or head in TRACKED_PREFIXES:
        return head
    return None


def parse_log(since: str | None) -> dict[str, CrateStats]:
    cmd = ["git", "log", "--pretty=format:COMMIT%x09%H%x09%ct", "--name-only"]
    if since:
        cmd.append(f"--since={since}")
    out = subprocess.check_output(cmd, text=True, errors="replace")

    now = int(time.time())
    stats: dict[str, CrateStats] = defaultdict(CrateStats)

    current_sha: str | None = None
    current_ts: int = 0
    seen_in_commit: set[tuple[str, str]] = set()  # (crate, sha) dedupe guard

    for line in out.splitlines():
        if not line:
            continue
        if line.startswith("COMMIT\t"):
            _, sha, ts = line.split("\t")
            current_sha = sha
            current_ts = int(ts)
            continue
        if current_sha is None:
            continue
        crate = crate_of(line)
        if crate is None:
            continue
        s = stats[crate]
        s.commits.add(current_sha)
        s.files.add(line)
        s.file_touches += 1
        if current_ts > s.last_ts:
            s.last_ts = current_ts
        if s.first_ts == 0 or current_ts < s.first_ts:
            s.first_ts = current_ts
        age_days = (now - current_ts) // 86_400
        for w in s.by_window:
            if age_days <= w:
                # avoid double-counting same commit in same window
                key = (crate, current_sha, w)
                if key not in seen_in_commit:
                    seen_in_commit.add(key)
                    s.by_window[w] += 1

    return stats


def format_report(stats: dict[str, CrateStats]) -> str:
    now = int(time.time())
    total_commits = sum(len(s.commits) for s in stats.values())
    rows = []
    for crate, s in stats.items():
        days_since = (now - s.last_ts) // 86_400 if s.last_ts else -1
        files_per_commit = s.file_touches / max(1, len(s.commits))
        share_pct = 100.0 * len(s.commits) / max(1, total_commits)
        rows.append(
            (
                crate,
                len(s.commits),
                len(s.files),
                days_since,
                s.by_window[7],
                s.by_window[14],
                s.by_window[30],
                share_pct,
                files_per_commit,
            )
        )

    rows.sort(key=lambda r: r[1], reverse=True)  # by total commits desc

    header = (
        "crate",
        "commits",
        "files",
        "last(d)",
        "7d",
        "14d",
        "30d",
        "share%",
        "files/cmt",
    )
    widths = [
        max(len(str(r[i])) for r in [header, *rows]) for i in range(len(header))
    ]

    def fmt_row(r):
        parts = []
        for i, v in enumerate(r):
            if isinstance(v, float):
                v = f"{v:.1f}"
            parts.append(str(v).ljust(widths[i]) if i == 0 else str(v).rjust(widths[i]))
        return "  ".join(parts)

    lines = [fmt_row(header), "  ".join("-" * w for w in widths)]
    lines.extend(fmt_row(r) for r in rows)
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--since", help="git --since cutoff (e.g. 2025-01-01)")
    args = ap.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    try:
        subprocess.check_call(
            ["git", "-C", str(repo_root), "rev-parse", "--git-dir"],
            stdout=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        print("not in a git repo", file=sys.stderr)
        return 1

    import os

    os.chdir(repo_root)
    stats = parse_log(args.since)
    if not stats:
        print("no commits found", file=sys.stderr)
        return 1
    print(format_report(stats))
    return 0


if __name__ == "__main__":
    sys.exit(main())
