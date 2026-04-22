# integration-tests fixtures

## `vintage_baseline.patches`

Fixed-input patch exercising `VChorus` through the in-process
`patches_modules::default_registry()`. Input signal is a deterministic
220 Hz sine from the built-in `Osc`; `hiss` is pinned to `0.0` so the
PRNG path is silent and no randomness enters the output.

### Artifacts

- `vintage_baseline.patches` — DSL source (committed).
- `vintage_baseline.f32` — stereo f32 LE interleaved (`[l0, r0, l1, r1, ...]`).
  8192 stereo frames @ 44 100 Hz.
- `vintage_baseline.sha256` — hex SHA-256 of the `.f32` bytes.

### Render invocation

```
cargo test -p patches-integration-tests --test vintage_baseline \
    -- --ignored regenerate_vintage_baseline
```

The regenerator writes both the raw bytes and the hash. The default
non-ignored test (`vintage_baseline_matches_golden`) renders again and
asserts byte-for-byte equality plus matching SHA-256.

### Parity oracle

Ticket 0628 installs this as the in-process baseline. Ticket 0629
retargets the render path through the `patches-vintage` bundle once
Phase E lands; any bit drift fails the same assertion.
