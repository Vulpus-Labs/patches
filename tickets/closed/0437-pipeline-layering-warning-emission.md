---
id: "0437"
title: Emit PV#### pipeline-layering warnings at overlap sites
priority: low
created: 2026-04-15
---

## Summary

Ticket 0432 added `RenderedDiagnostic::pipeline_violation` and a `PV####`
severity-Warning code family so a later pipeline stage firing on input
an earlier stage accepted can be surfaced as a pipeline bug (not a
user-patch error). The mechanism is in place but nothing emits these
warnings yet — we landed the converter without identifying concrete
overlap sites.

Audit the pipeline stages for checks that overlap: cases where stage N
would reject an input that stage N−1 already accepted. Wrap at least
one such check to emit a `PV####` warning when the discrepancy fires,
with a test that exercises it by fabricating a crafted FlatPatch
whose layering invariant is violated.

## Acceptance criteria

- [ ] At least one concrete layering invariant identified and
      documented in the code (e.g. descriptor_bind finds something
      structural checks should have caught).
- [ ] The violation emits a `PV####` warning via
      `RenderedDiagnostic::pipeline_violation`; the code registry
      includes stable code + label.
- [ ] Unit test constructs the violating input and asserts a warning
      (not an error) is produced with the expected code.
- [ ] Doc comment on `pipeline_violation` updated with a list of
      active emission sites.
- [ ] `cargo test -p patches-diagnostics`, `cargo clippy` clean.

## Notes

Independent of tickets 0433, 0435, 0436, 0438. This is exploratory:
if an audit finds no overlap worth flagging, the ticket closes with a
note that the mechanism is retained for future use. Don't invent
synthetic overlaps to justify the ticket.

Candidate overlap sites to investigate:

- Structural-checks pass and descriptor_bind both inspect module names
  and cable agreement — does descriptor_bind ever reject something a
  clean structural pass would have caught?
- Orphan `port_refs` validation (descriptor_bind) vs. any earlier
  expansion-boundary checks on port references.

Part of E081.
