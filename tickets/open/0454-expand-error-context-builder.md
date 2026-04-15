---
id: "0454"
title: ExpandError context builder to reduce error boilerplate
priority: low
created: 2026-04-15
---

## Summary

`ExpandError::new` construction in `expand.rs` is repeated dozens of
times with `format!` calls that fold in template/song/pattern/param
context manually. The result is dense blocks that obscure surrounding
logic. A small context-aware builder (threaded through the expander
or attached to the current binding env) would drop each site to a
single line.

## Acceptance criteria

- [ ] Expander carries an error-context helper that auto-fills current
      template / song / pattern / param names.
- [ ] Each `ExpandError::new` site in `expand.rs` is one line of intent
      (message + missing variable).
- [ ] Error messages remain equivalent (tests may need message
      comparison updates but semantics unchanged).
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E083. Pairs well with 0450 — landing the split first makes the context
threading obvious.
