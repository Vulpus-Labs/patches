---
id: "0357"
title: Include loader with cycle detection and deduplication
priority: high
created: 2026-04-12
epic: "E067"
adr: "0032"
depends: ["0356"]
---

## Summary

Add a loader module to `patches-dsl` that resolves `include` directives recursively, detects cycles, deduplicates diamond dependencies, checks for name collisions, and produces a merged `File` AST ready for the expander.

## Design

**New file:** `patches-dsl/src/loader.rs`

```rust
pub struct LoadResult {
    pub file: File,
    pub dependencies: Vec<PathBuf>,
}

pub struct LoadError {
    pub message: String,
    pub include_chain: Vec<(PathBuf, Span)>,
}

pub fn load_with<F>(master_path: &Path, read_file: F) -> Result<LoadResult, LoadError>
where
    F: Fn(&Path) -> Result<String, std::io::Error>,
```

**Algorithm:**

1. Canonicalize master path. Read and parse with `parse()` (requires `patch {}`).
2. DFS traversal of include directives:
   - Resolve paths relative to the directory of the including file.
   - `stack: Vec<PathBuf>` for cycle detection — path on stack means cycle error.
   - `visited: HashSet<PathBuf>` for deduplication — already-visited path is skipped.
   - Included files parsed with `parse_include_file()` (no `patch {}` allowed).
3. Merge: concatenate templates, patterns, songs from all included files into the master `File`. Depth-first order (dependencies before dependents).
4. Name collision check: error if two files define a template, pattern, or song with the same name, reporting both file paths.
5. Return merged `File` + full set of loaded paths (for hot-reload).

**Error reporting:** `LoadError` carries an `include_chain` showing the chain of includes that led to the error, so messages read like:

```
error in "lib/drums.patches" (included from "main.patches" line 3):
  parse error at 45..52: expected `}`
```

## Acceptance criteria

- [ ] Single include: master includes one library; merged result has both sets of templates
- [ ] Transitive includes: A includes B, B includes C; all definitions merge
- [ ] Diamond dependency: A includes B and C; B and C both include D; D loaded once
- [ ] Cycle detection: A includes B, B includes A; returns `LoadError` with cycle message
- [ ] Self-include: A includes itself; returns cycle error
- [ ] Missing file: returns IO error wrapped in `LoadError` with include chain
- [ ] Name collision: two files define template with same name; returns `LoadError` with both paths
- [ ] Included file with `patch {}` block: parse error from `parse_include_file`
- [ ] `load_with` with in-memory file map works (closure-based file reading)
- [ ] Paths in `dependencies` are canonical and include all loaded files
- [ ] Public API exported from `patches-dsl/src/lib.rs`
- [ ] `cargo test -p patches-dsl` and `cargo clippy` pass

## Notes

- The closure-based `read_file` parameter keeps `patches-dsl` testable without filesystem access. Tests use `HashMap<PathBuf, String>` lookups.
- Name collision detection covers templates, patterns, and songs. Module instance names are scoped to the patch block and cannot collide across files.
