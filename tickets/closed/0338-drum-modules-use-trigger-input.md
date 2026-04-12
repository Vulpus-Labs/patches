---
id: "0338"
title: Refactor drum modules to use TriggerInput
priority: medium
created: 2026-04-12
---

## Summary

Replace the `prev_trigger: f32` + `in_trigger: MonoInput` pattern in all
drum modules with `TriggerInput`. Use `self.in_trigger.tick(pool)` for edge
detection and `self.in_trigger.value()` where the raw trigger value is still
needed (e.g. for `amp_env.tick(trigger)`).

## Acceptance criteria

- [ ] `kick.rs`: `prev_trigger` removed, `in_trigger` changed to `TriggerInput`
- [ ] `snare.rs`: same
- [ ] `hihat.rs` (`OpenHiHat`): same
- [ ] `hihat.rs` (`ClosedHiHat`): `prev_trigger` and `prev_choke` both become `TriggerInput`
- [ ] `cymbal.rs`: same as kick
- [ ] `claves.rs`: same as kick
- [ ] `clap_drum.rs`: same as kick
- [ ] `tom.rs`: same as kick
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

Depends on 0335. See ADR 0030. Epic E062.
