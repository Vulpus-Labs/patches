# ADR 0032 — Include directives for multi-file patches

**Date:** 2026-04-12
**Status:** accepted

---

## Context

All templates, patterns, songs, and the patch block must currently live in
a single `.patches` file. As patches grow — particularly with tracker
material (patterns and songs) — files become unwieldy. Users want to
split reusable definitions into library files and compose them via
includes, while keeping the single-`patch`-block invariant.

Key requirements:

- Templates, patterns, and songs can be defined in separate files.
- Exactly one `patch {}` block exists, in the "master" file.
- The master file is the entry point for loading and hot-reload.
- Included files should also trigger hot-reload when modified.
- Diamond dependencies (A includes B and C, both include D) must not
  duplicate D's definitions.
- Include cycles must be detected and rejected.

## Decision

### Syntax: `include` keyword

Use `include "path/to/file.patches"` rather than `#include`. The `#`
character already introduces comments in the grammar
(`COMMENT = _{ "#" ~ (!"\n" ~ ANY)* }`), so `#include` would be silently
consumed as a comment. A keyword-based directive is consistent with the
DSL's existing style (`template`, `pattern`, `song`, `patch`).

Paths are relative to the directory of the file containing the directive.

### Two grammar root rules

The pest grammar gains a second root rule for library files:

```pest
include_directive = { "include" ~ string_lit }

file         = { SOI ~ (include_directive | template | pattern_block | song_block)* ~ patch ~ EOI }
include_file = { SOI ~ (include_directive | template | pattern_block | song_block)* ~ EOI }
```

`file` is used for the master file (requires `patch {}`); `include_file`
is used for included files (no `patch {}` allowed). This is a
parse-level structural constraint rather than a post-hoc validation.

### New loader layer between parser and expander

The pipeline becomes:

```text
Source text  →  Parser (per-file)  →  ASTs
                                       ↓
                              Loader (resolve includes, merge)
                                       ↓
                              Merged File (single AST)
                                       ↓
                              Expander  →  FlatPatch  →  …
```

The loader:

- Accepts a file-reading closure (`Fn(&Path) -> Result<String, io::Error>`)
  so it can be tested with in-memory file maps.
- Performs depth-first traversal of include directives.
- Tracks a `visited: HashSet<PathBuf>` to deduplicate diamond dependencies.
- Tracks a `stack: Vec<PathBuf>` to detect cycles.
- Merges all templates, patterns, and songs into the master `File` struct.
- Reports name collisions (duplicate template/pattern/song names across files).
- Returns the merged `File` plus the full set of loaded paths (for
  hot-reload watching).

### Hot-reload watches all dependencies

`patches-player` currently polls the mtime of a single file. With includes,
it watches all paths returned by the loader. On any change, the entire
include tree is re-resolved (an include may have been added or removed),
and the dependency set is refreshed.

### CLAP plugin impact

The CLAP plugin loads `.patches` from file paths (user "Browse" button) and
stores `dsl_source` + `base_dir`. It calls `parse()` → `expand()` →
`build_with_base_dir()` in `compile_and_push_plan()`. With includes:

- When a file path is available, use the loader instead of `parse()`.
- Reload is manual (button click), so no dependency tracking is needed —
  the full include tree is re-resolved each time.
- State persistence continues to save the master file path + source. On
  restore, if original files exist on disk, the loader resolves includes
  normally. If files are missing, fall back to parsing the saved source
  (master only, graceful degradation).

### LSP impact

The LSP already has multi-file navigation groundwork:

- `NavigationIndex` stores `(Url, Span)` per definition.
- `goto_definition` returns cross-file results.
- A `cross_file_resolution` test validates the architecture.
- The server's goto-definition handler has an explicit placeholder comment
  for when includes land.

When the master file contains `include` directives, the LSP parses the
tree-sitter CST for `include_directive` nodes, resolves paths, reads and
analyses included files, and inserts them into the document map. The
existing `NavigationIndex::rebuild()` then picks up their definitions
automatically.

## Alternatives considered

### `#include` (C-style preprocessor)

Rejected because `#` is the comment character. Would require either
changing the comment syntax (breaking change) or a pre-parse text
substitution step that loses span accuracy for included content.

### Pre-parse textual substitution

Simpler to implement but loses per-file span tracking, making error
messages less useful and LSP integration harder. The post-parse loader
approach preserves file-origin information throughout the pipeline.

### `import` with namespacing

A namespace-qualified import system (e.g. `import "lib.patches" as lib`,
then `lib.voice`) would prevent name collisions structurally but adds
significant complexity to the expander. The simpler flat-merge approach
with name-collision detection is sufficient for the current use case.
Namespaced imports can be added later as a non-breaking extension.
