#!/usr/bin/env python3
"""Align markdown tables so every column has uniform width.

Usage:
    python3 tools/align-tables.py FILE [FILE ...]
    python3 tools/align-tables.py --check FILE [FILE ...]

Without --check, files are rewritten in place.
With --check, exits non-zero if any file would change (useful in CI).
"""

import re
import sys


def is_separator(row: list[str]) -> bool:
    """True if every cell matches the --- separator pattern."""
    return all(re.fullmatch(r":?-+:?", cell.strip()) for cell in row)


def parse_row(line: str) -> list[str] | None:
    """Split a pipe-delimited table row into cell contents, or None."""
    stripped = line.strip()
    if not stripped.startswith("|") or not stripped.endswith("|"):
        return None
    inner = stripped[1:-1]
    return [cell.strip() for cell in inner.split("|")]


def format_table(rows: list[list[str]]) -> list[str]:
    """Produce aligned table lines from parsed rows."""
    ncols = max(len(r) for r in rows)
    # Pad short rows.
    for r in rows:
        while len(r) < ncols:
            r.append("")

    widths = [max(len(r[c]) for r in rows) for c in range(ncols)]

    lines = []
    for row in rows:
        if is_separator(row):
            cells = ["-" * w for w in widths]
            lines.append("| " + " | ".join(cells) + " |")
        else:
            cells = [row[c].ljust(widths[c]) for c in range(ncols)]
            lines.append("| " + " | ".join(cells) + " |")
    return lines


def align_tables(text: str) -> str:
    """Find and realign all markdown tables in text."""
    lines = text.split("\n")
    out: list[str] = []
    i = 0
    while i < len(lines):
        row = parse_row(lines[i])
        if row is None:
            out.append(lines[i])
            i += 1
            continue

        # Accumulate consecutive table rows.
        table_rows: list[list[str]] = [row]
        j = i + 1
        while j < len(lines):
            r = parse_row(lines[j])
            if r is None:
                break
            table_rows.append(r)
            j += 1

        # Only realign if there's a separator row (actual table, not a
        # stray pipe line).
        if len(table_rows) >= 2 and is_separator(table_rows[1]):
            out.extend(format_table(table_rows))
        else:
            out.extend(lines[i:j])
        i = j

    return "\n".join(out)


def main() -> int:
    args = sys.argv[1:]
    check = False
    if "--check" in args:
        check = True
        args.remove("--check")

    if not args:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    dirty = False
    for path in args:
        with open(path, "r") as f:
            original = f.read()
        result = align_tables(original)
        if result != original:
            if check:
                print(f"would reformat: {path}", file=sys.stderr)
                dirty = True
            else:
                with open(path, "w") as f:
                    f.write(result)
                print(f"reformatted: {path}", file=sys.stderr)

    return 1 if dirty else 0


if __name__ == "__main__":
    sys.exit(main())
