---
id: "0349"
title: HostTransport module
priority: medium
created: 2026-04-12
depends: "0348"
---

## Summary

Add a `HostTransport` module that reads the `HOST_TRANSPORT` backplane slot and unpacks it into named mono outputs. This is a convenience module for patches that want to route transport signals to generative or unsequenced parts of the patch. Sequenced modules like `MasterSequencer` read the backplane directly and do not need this module.

## Acceptance criteria

- [ ] Module registered as `HostTransport` in the module registry
- [ ] Mono outputs: `playing`, `tempo`, `beat`, `bar`, `beat_trigger`, `bar_trigger`, `tsig_num`, `tsig_denom`
- [ ] Each output reads from the corresponding lane of `HOST_TRANSPORT` backplane poly slot
- [ ] Doc comment follows the module documentation standard
- [ ] Tests verify outputs reflect backplane values
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- Follows the same pattern as `AudioIn`: fixed backplane input, user-connectable outputs.
- Useful for controlling generative patches, gating effects on/off with transport, tempo-synced LFOs, etc.
