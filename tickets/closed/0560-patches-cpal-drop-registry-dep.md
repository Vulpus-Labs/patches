---
id: "0560"
title: Drop unused patches-registry dep from patches-cpal
priority: low
created: 2026-04-18
---

## Summary

`patches-cpal/Cargo.toml:14` declares `patches-registry` but no
`.rs` file in the crate imports it. Residue from pre-E089 slim.
Remove.

Part of epic E093.

## Acceptance criteria

- [ ] `patches-registry` removed from `patches-cpal/Cargo.toml`.
- [ ] `cargo build -p patches-cpal` clean.
- [ ] `cargo build` workspace clean.

## Notes

Trivial. Can land independently of the other E093 tickets.
