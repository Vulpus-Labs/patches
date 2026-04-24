# E117 — Mutation testing (kernel)

Rollup for the E117 mutation-testing epic. Per-crate runs are tracked
in tickets 0681–0685; this note collects cross-crate patterns and
recommendations as they emerge.

## Setup (ticket 0680)

- Tool: `cargo-mutants` v27.
- Install: `cargo install cargo-mutants --locked`.
- Config: `.cargo/mutants.toml` — `timeout_multiplier = 2.0`,
  `minimum_test_timeout = 30`, and `exclude_globs` covering all
  non-kernel crates plus `**/tests.rs` / `**/tests/**` / benches /
  examples.
- Output: `mutants.out/` at workspace root (gitignored).

## Invocation

Per-crate (the expected form for 0681–0685):

```bash
cargo mutants -p patches-core
cargo mutants -p patches-dsp
cargo mutants -p patches-dsl
cargo mutants -p patches-interpreter
cargo mutants -p patches-engine
```

Scoped to a single file for triage or smoke testing:

```bash
cargo mutants -p patches-core --file 'patches-core/src/modules/parameter_map.rs'
```

Useful flags:

- `--list` — enumerate mutants without running.
- `--no-shuffle` — deterministic order (helpful when comparing runs).
- `--jobs N` — parallel test jobs.
- `--in-place` — avoid cloning the repo; faster, but blocks other
  cargo work in the tree.

## Smoke-run result (0680)

Ran on `patches-core/src/modules/parameter_map.rs` as pipeline check:
45 mutants, 63s total, 21 missed / 8 caught / 16 unviable. The high
MISSED ratio is itself signal — flag `parameter_map.rs` as a candidate
hotspot when 0681 runs in full.

## Triage rubric

- Arithmetic / boundary mutants in DSP: treat as real gaps.
- Constant / default swaps, `Display::fmt` returning `Ok(Default)`,
  `kind_name -> "xyzzy"`: typically benign (tests don't assert on
  kind strings or Display output).
- Return-value replacements: check observability at the callsite.
- Iterator replacements (`iter::empty`, `iter::once(("xyzzy", …))`):
  real if the iterator feeds downstream logic; benign if only used in
  debug paths.

## Per-crate sections

(Filled in as 0681–0685 complete.)

### patches-core (0681)

Run: `cargo mutants -p patches-core --jobs 4 --no-shuffle`.
Result: 891 mutants / 11 min — **455 caught / 268 missed / 154 unviable / 14 timeouts**.
Effective catch rate (excluding unviable): 62%.

**Top MISSED by ratio** (excluding `test_support/`):

| File                           | Missed | Total | Ratio |
| ------------------------------ | ------ | ----- | ----- |
| `modules/module.rs`            | 26     | 31    | 84%   |
| `modules/instance_id.rs`       | 3      | 5     | 60%   |
| `random_walk.rs`               | 10     | 17    | 59%   |
| `cables/gate.rs`               | 14     | 26    | 54%   |
| `params.rs`                    | 21     | 42    | 50%   |
| `midi_io.rs`                   | 16     | 32    | 50%   |
| `modules/parameter_map.rs`     | 21     | 45    | 47%   |
| `source_map.rs`                | 9      | 24    | 38%   |
| `modules/module_descriptor.rs` | 6      | 18    | 33%   |
| `cable_pool.rs`                | 8      | 25    | 32%   |
| `param_frame/view.rs`          | 20     | 70    | 29%   |

**Structural finding — module.rs (84%)**: the `Module` trait's default
method bodies (default `update_parameters`, `wants_periodic`,
`as_tracker_data_receiver`, `set_ports`) are not exercised by any
in-crate test. Concrete implementors live in `patches-modules` and
aren't visible from this run. Fix path: add an in-crate fake `Module`
used only to pin default-method behavior. Not a per-method gap —
whole-pattern gap.

**Excluded going forward**: `**/test_support/**` (63 missed mutants
were in test harness code — meaningless signal). Added to
`.cargo/mutants.toml`.

**Benign pattern observations**:

- `kind_name -> ""` / `-> "xyzzy"` on `ParameterValue` and
  `ParameterKind` survive everywhere. Diagnostic/display-only; not
  worth testing.
- `Display::fmt -> Ok(Default)` survives. Same category.
- Iterator-return mutants (`::std::iter::empty()`) in parameter_map
  are real misses — they mean iteration contents are unverified.

**Follow-up tickets** (non-blocking for epic close):

- 0686 — `ParameterMap` accessor/mutator tests
- 0687 — `Module` default-method coverage via test-harness fake
- 0688 — `params.rs` coverage
- 0689 — `midi_io.rs` coverage
- 0690 — `cables/gate.rs` + `cables/trigger.rs` coverage
- 0691 — `random_walk.rs` coverage
- 0692 — `param_frame/view.rs` coverage

### patches-dsp (0682)

### patches-dsl (0683)

### patches-interpreter (0684)

### patches-engine (0685)
