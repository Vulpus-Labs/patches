---
id: "0883"
title: Cut patches-drums repo
priority: medium
created: 2026-05-11
---

## Summary

Move patches-drums (extracted in-monorepo by ticket 0873) into its
own repo. cdylib+rlib bundle of seven drum modules + drum DSP
primitives.

Depends on ticket 0888 (patches-sdk + patches-core + patches-ffi-common
published to crates.io). Sibling to 0882 (vintage cut) and 0884 (fft
cut); can run in parallel after 0888.

## Acceptance criteria

- [ ] New repo `patches-drums` initialised; single crate workspace.
- [ ] Deps: `patches-sdk = "0.7"` (crates.io) + `patches-dsp` (git
      tag from the main `patches` repo).
- [ ] `cargo build`, `cargo test`, `cargo clippy` green.
- [ ] cdylib build produces a loadable bundle; `descriptor_hash`
      matches ABI v12.
- [ ] CI scaffolded; release build of cdylib uploaded as artefact.
- [ ] `v0.7.0` tagged.
- [ ] Main repo: remove patches-drums workspace member; host
      consumes via PluginScanner search path.
- [ ] Main repo `just push` green.

## Notes

Drum DSP primitives (envelope, sweep, metallic, burst, saturate) live
inside this crate as `src/primitives/` per ticket 0873. They are not
re-exported through patches-sdk and not used by anything else.

If a future module crate wants a drum primitive (DecayEnvelope etc.),
options:

- Copy the kernel (small, well-tested).
- Promote to patches-dsp (foundation) at that point.
- Split a `patches-drums-primitives` rlib out of the drum repo.

YAGNI today.

Out of scope:

- Reordering drum modules' descriptors.
- Adding new drums.
