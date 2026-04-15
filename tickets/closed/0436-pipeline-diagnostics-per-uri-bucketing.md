---
id: "0436"
title: Bucket pipeline diagnostics by source URI
priority: medium
created: 2026-04-15
---

## Summary

Ticket 0432 collapses every pipeline-stage diagnostic onto the root
document's LSP `publishDiagnostics` call: diagnostics whose primary
span lives in an included file are pinned to the root at `(0, 0)` with
an `"in <path>: "` message prefix and a `relatedInformation` link back
to the offending file. This was a pragmatic compromise while the
tree-sitter fallback was still running on every document and providing
its own per-URI diagnostics.

Once 0433 gates the tree-sitter path, included files will stop
receiving TS-semantic diagnostics on clean-pest runs and lose any
positional feedback from the pipeline. Replace the collapse with
proper per-URI bucketing: `DocumentWorkspace::analyse` returns
`Vec<(Url, Vec<Diagnostic>)>` (matching `refresh_from_disk`), the
server publishes each bucket against its own URI, and
`rendered_to_lsp_diagnostic` reverts to plain positional ranges.

## Acceptance criteria

- [ ] `DocumentWorkspace::analyse` returns `Vec<(Url, Vec<Diagnostic>)>`;
      server publishes one `publishDiagnostics` per URI, clearing
      buckets that no longer contain diagnostics.
- [ ] Pipeline diagnostics whose primary span is in an included file
      appear on that file's URI with a real range — no `(0, 0)`
      placeholders, no `"in <path>: "` message prefix.
- [ ] `rendered_to_lsp_diagnostic` takes the target URI / line index
      explicitly; cross-file collapsing path removed.
- [ ] `relatedInformation` still links sibling snippets (expansion
      chains, include chains), but no longer doubles as the primary
      location.
- [ ] Tests cover: (a) a structural error in an include surfaces on
      the include's URI, not the root's; (b) the root gets an empty
      publish when only child-file diagnostics exist; (c) fixing the
      child clears its bucket.
- [ ] `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Sequence with ticket 0433. Landing 0436 before 0433 is safe but
produces duplicate diagnostics on included files (TS-semantic + per-URI
pipeline) until TS is gated. Landing 0433 first creates a coverage
gap on included files until 0436 ships. Aim to land them together or
back-to-back.

Depends on E080. Part of E081.
