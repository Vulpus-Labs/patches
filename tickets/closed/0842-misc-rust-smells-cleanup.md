---
id: "0842"
title: Misc small Rust smells (cleanup pass)
priority: low
created: 2026-05-08
epic: E139
---

## Summary

A handful of small, independent quality nudges from the 2026-05-08 style
survey. Each is a 5–30 line change. Bundled into a single ticket because
none warrants its own and they share a `just push` cycle.

## Sites

- [patches-core/src/qname.rs:120](../../patches-core/src/qname.rs#L120)
  — `impl PartialEq<String> for QName` exists; `impl PartialEq<&str>`
  does not. Borrowed comparisons currently allocate. Add the borrowed
  impl (and `impl PartialEq<QName> for &str` for symmetry).
- [patches-core/src/midi_io.rs:123-127](../../patches-core/src/midi_io.rs#L123)
  — `[MidiEvent { bytes: [0; 3] }; MAX_STASH]` repeated across
  constructors. Add `impl Default for MidiEvent` (or a `const ZERO`)
  and use it.
- [patches-host/src/runtime.rs:187-191](../../patches-host/src/runtime.rs#L187)
  — generation wraparound uses `wrapping_add(1)` plus check-zero.
  Replace with `gen.checked_add(1).unwrap_or(1)` (explicit) or wrap the
  field in `NonZeroU64` and use `NonZeroU64::saturating_add` /
  `from(1)` reset.
- [patches-host/src/runtime.rs:275-280](../../patches-host/src/runtime.rs#L275)
  — `push_blocking` magic `Duration::from_millis(10)`. Promote to
  `pub const PLAN_RING_PUSH_RETRY_MS: u64 = 10;` with a one-line
  comment on chosen value.
- [patches-lsp/src/tree_nav.rs:262-271](../../patches-lsp/src/tree_nav.rs#L262)
  — manual `loop { match parent }` ancestor walk. Replace with
  `std::iter::successors(Some(node), |n| n.parent()).find(...)`.
- [patches-lsp/src/workspace/lifecycle.rs:79-83](../../patches-lsp/src/workspace/lifecycle.rs#L79)
  — `.map(...).unwrap_or_default()` chain over a `HashMap` lookup that
  immediately collects. Either `cloned().collect()` directly on the
  entry or extract a `non_root_publish_for(uri) -> Vec<_>` helper.
- [patches-diagnostics/src/lib.rs:377-424](../../patches-diagnostics/src/lib.rs#L377)
  — repeated `Option<(usize, usize)>` token-offset returns. Define
  `struct TokenSpan { start: u32, len: u32 }` (or reuse `patches-core`'s
  `Span` if it fits). Halves payload on 64-bit and types the
  `start..start+len` invariant.
- [patches-interpreter/src/descriptor_bind/mod.rs:161-165](../../patches-interpreter/src/descriptor_bind/mod.rs#L161)
  — name→index map: collect names, sort, re-collect into HashMap.
  Single pass: `HashMap::from_iter(sorted.into_iter().enumerate().map(|(i,n)|(n,i)))`.

## Acceptance criteria

- [ ] Each site changed or explicitly skipped with a one-line note in
      the PR (e.g. "QName: impl added; gen wraparound: deferred,
      `NonZeroU64` migration is a separate ticket")
- [ ] No behaviour changes; this is purely representation/idiom
- [ ] `just push` clean

## Notes

Each item is independent. If any item turns out to be more involved than
expected (e.g. `Span` newtype propagates wider than `patches-diagnostics`),
split it out as a follow-up rather than blocking the ticket.

Closes the trailing items from the E139 style survey.
