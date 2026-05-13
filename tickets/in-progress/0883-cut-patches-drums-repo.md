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

- [x] New repo `patches-drums` initialised; single crate workspace.
- [x] Deps: `patches-sdk = "0.7"` (crates.io) + `patches-dsp` (git
      rev `95c4f29…` of the main `patches` repo until a v0.7.2+
      tag exists).
- [x] `cargo build`, `cargo test`, `cargo clippy` green (51 tests
      pass).
- [x] cdylib build produces a loadable bundle.
- [ ] CI scaffolded; release build of cdylib uploaded as artefact.
      **User-side.**
- [ ] `v0.7.0` tagged. **User-side.**
- [x] Main repo: remove patches-drums workspace member; host
      consumes via PluginScanner search path.
- [x] Main repo `just push` green.

## Resolution

- Initial cut into `github.com/Vulpus-Labs/patches-drums`;
  subsequently consolidated alongside vintage + fft into
  `github.com/Vulpus-Labs/patches-bundles` as the `patches-drums/`
  member of a four-crate Cargo workspace. The dedicated
  single-bundle repo is being retired.
- Main repo: `patches-drums/` dir removed, workspace member dropped
  from `Cargo.toml`. No bundle-coupled main-repo tests targeted
  drum modules; trim was mechanical.
- Bundle-using examples (`drum_machine`, `song1/`,
  `microtonal/microtonal`) moved to
  `patches-bundles/patches-drums/examples/`.
- CI / `v0.7.0` tag / release artefact remain user-side actions in
  the patches-bundles repo.

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
