---
id: "0931"
title: Reorg — host/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/host/` and move the host-facing modules
into it: `host_control`, `host_transport`, `audio_in`, `audio_out`,
`ms_ticker`, `tempo_sync`, `clock`.

## Acceptance criteria

- [ ] `patches-modules/src/host/{mod.rs, host_control.rs,
      host_transport.rs, audio_in.rs, audio_out.rs, ms_ticker.rs,
      tempo_sync.rs, clock.rs}` exist; flat siblings deleted.
- [ ] Public re-exports preserve every `patches_modules::HostControl`,
      `::HostTransport`, `::AudioIn`, `::AudioOut`, `::MsTicker`,
      `::TempoSync`, `::Clock`.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
