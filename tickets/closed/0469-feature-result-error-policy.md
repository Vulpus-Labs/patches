---
id: "0469"
title: Trace silent failure points in LSP feature handlers
priority: low
created: 2026-04-15
---

## Summary

LSP feature handlers in `patches-lsp/src/workspace/mod.rs:543-630`
return `None`/`vec![]` on several failure modes (document not
found, expansion context unavailable, pipeline incomplete)
without any logging. A client sees the feature degrade
gracefully; a developer debugging why a feature misbehaves has
no signal to follow.

The original scoping of this ticket proposed a full
`FeatureResult<T, FeatureError>` type. That's a lot of type
surface for what's really an observability gap.

## Acceptance criteria

- [ ] Every early-return `None` / `Vec::new()` in feature handler
      entry points (completions, hover, peek_expansion,
      inlay_hints, goto_definition) carries a `tracing::debug!`
      (or `trace!`) line naming the failure mode and URI.
- [ ] No new error type introduced. Wire-level return shapes
      unchanged.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Narrowed from the original "FeatureResult" framing after
reality-check: the problem is silent failure, not type design.
Add logging, move on.
