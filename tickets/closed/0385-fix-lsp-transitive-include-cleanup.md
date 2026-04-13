---
id: "0385"
title: Fix LSP transitive stale-include cleanup
priority: low
created: 2026-04-13
---

## Summary

In `patches-lsp/src/server.rs` lines 201–217, stale-include cleanup only
considers URIs from the current parent's immediate include list. If parent A
includes B which includes C, and B is removed from A's includes, C remains in
`include_loaded` and `documents` because it was never in `new_include_uris`.

## Acceptance criteria

- [ ] Stale-include cleanup walks the transitive closure of includes
- [ ] Removing an include that itself has includes cleans up the entire subtree
- [ ] Documents that are still referenced by other parents are not removed (diamond dependency case)
- [ ] Test: A includes B includes C; remove B from A; verify both B and C are cleaned up

## Notes

This may require tracking which parent loaded each include (a reverse
dependency map), or re-walking the full include tree on each edit to compute
the live set.
