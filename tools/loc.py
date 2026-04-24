#!/usr/bin/env python3
"""Count Rust LOC split test/impl per crate.

Test code = any .rs file under tests/ dir, OR file named tests.rs / test.rs,
OR inline #[cfg(test)] mod blocks. Impl = rest.
Skips target/.
"""
import os, re, sys
from collections import defaultdict

ROOT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else ".")

def crate_of(path):
    rel = os.path.relpath(path, ROOT)
    return rel.split(os.sep)[0]

def count_lines(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read().splitlines()
    except Exception:
        return []

def split_inline_tests(lines):
    """Return (impl_count, test_count). Detect `#[cfg(test)]` followed by `mod ... {`
    and count the brace-balanced block as tests."""
    test = 0
    impl = 0
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("#[cfg(test)]"):
            # Find next `mod ... {` or `fn`/`impl` attribute-target
            j = i + 1
            while j < n and lines[j].strip() == "":
                j += 1
            if j < n and ("mod " in lines[j] or lines[j].lstrip().startswith("mod ")):
                # find opening brace
                k = j
                while k < n and "{" not in lines[k]:
                    k += 1
                if k >= n:
                    impl += 1
                    i += 1
                    continue
                depth = 0
                start = i
                end = k
                for col in lines[k]:
                    if col == "{": depth += 1
                    elif col == "}": depth -= 1
                while depth > 0 and end + 1 < n:
                    end += 1
                    for col in lines[end]:
                        if col == "{": depth += 1
                        elif col == "}": depth -= 1
                test += (end - start + 1)
                i = end + 1
                continue
        impl += 1
        i += 1
    return impl, test

# crate -> [impl, test]
stats = defaultdict(lambda: [0, 0])

for dirpath, dirnames, filenames in os.walk(ROOT):
    # prune
    dirnames[:] = [d for d in dirnames if d not in ("target", ".git", "node_modules", "dist")]
    for fn in filenames:
        if not fn.endswith(".rs"):
            continue
        full = os.path.join(dirpath, fn)
        rel = os.path.relpath(full, ROOT)
        parts = rel.split(os.sep)
        crate = parts[0]
        # only count under recognizable crate dirs (has Cargo.toml somewhere above OR is known)
        if not os.path.exists(os.path.join(ROOT, crate, "Cargo.toml")):
            # maybe test-plugins subdirs
            if crate == "test-plugins":
                pass
            else:
                continue
        lines = count_lines(full)
        total = len(lines)
        # Whole-file test detection
        is_test_file = (
            "tests" in parts[1:]  # any tests/ dir in path
            or fn in ("tests.rs", "test.rs")
            or fn.endswith("_tests.rs")
            or fn.endswith("_test.rs")
            or "benches" in parts[1:]
        )
        if is_test_file:
            stats[crate][1] += total
        else:
            impl, test = split_inline_tests(lines)
            stats[crate][0] += impl
            stats[crate][1] += test

rows = sorted(stats.items())
wc = max(len(c) for c,_ in rows)
print(f"{'crate'.ljust(wc)}  {'impl':>8}  {'test':>8}  {'total':>8}")
print("-" * (wc + 30))
ti = tt = 0
for crate, (i, t) in rows:
    print(f"{crate.ljust(wc)}  {i:>8}  {t:>8}  {i+t:>8}")
    ti += i; tt += t
print("-" * (wc + 30))
print(f"{'TOTAL'.ljust(wc)}  {ti:>8}  {tt:>8}  {ti+tt:>8}")
