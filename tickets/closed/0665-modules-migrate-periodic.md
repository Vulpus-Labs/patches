---
id: "0665"
title: Migrate module impls to Module::periodic_update
priority: high
created: 2026-04-24
epic: E114
adr: 0052
depends_on: ["0663"]
---

## Summary

Replace `impl PeriodicUpdate for M` + `fn as_periodic(...) { Some(self) }`
with `const WANTS_PERIODIC: bool = true;` and an inherent
`periodic_update` method on each module's `impl Module for M` block.
Mechanical migration across 15 modules.

## Acceptance criteria

- [ ] All of the following migrated:
      `drive`, `bitcrusher`, `svf`, `pitch_shift`, `kick`, `poly_svf`,
      `convolution_reverb` (mono + stereo), `filter::highpass`,
      `filter::lowpass`, `filter::bandpass`, `fdn_reverb::processor`,
      `poly_filter` (three impls in `poly_filter/mod.rs`).
- [ ] No remaining `impl PeriodicUpdate for` in `patches-modules`.
- [ ] No remaining `fn as_periodic` override in `patches-modules`.
- [ ] Connectivity / idle-gating logic preserved inside
      `periodic_update` bodies — no semantic change.
- [ ] `cargo test -p patches-modules` green.
- [ ] `cargo clippy -p patches-modules` clean.

## Notes

Each impl currently looks like:

```rust
impl PeriodicUpdate for M {
    fn periodic_update(&mut self, pool: &CablePool<'_>) { /* body */ }
}

impl Module for M {
    // ...
    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}
```

After migration:

```rust
impl Module for M {
    // ...
    const WANTS_PERIODIC: bool = true;
    fn periodic_update(&mut self, pool: &CablePool<'_>) { /* body */ }
}
```

Find them with `grep -rn "impl PeriodicUpdate for" patches-modules/src`.
