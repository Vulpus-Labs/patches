# ADR 0060 — Structural parameter flag and `ModuleShape` reduction

**Date:** 2026-04-27
**Status:** Proposed
**Related:**
[ADR 0011 — Descriptor-first module v2](0011-descriptor-first-module-v2.md),
[ADR 0012 — Planner v2 graph diffing](0012-planner-v2-graph-diffing.md),
[ADR 0044 — Dynamic module loading and reload](0044-dynamic-module-loading-reload.md),
[ADR 0046 — Typed parameter keys](0046-typed-parameter-keys.md)

## Context

`ModuleShape` currently carries three fields:

```rust
pub struct ModuleShape {
    pub channels: usize,
    pub length: usize,
    pub high_quality: bool,
}
```

The three fields are not the same kind of thing.

- `channels` **shapes the descriptor**. It changes port counts and
  hash identity. Two instances with different `channels` are different
  module *types* as far as the planner, cable router, and type checker
  are concerned. It belongs in shape because the descriptor cannot
  exist without it.

- `high_quality` and `length` **do not shape the descriptor**. Ports,
  parameters, ranges, and identity are unchanged. They only size
  internal buffers at construction time — FFT bins and processing
  budget in `pitch_shift`, interpolator state in `delay` /
  `stereo_delay`. They ride in `ModuleShape` because there was no
  other slot for "construction-time-only param": they cannot be regular
  `ParameterDescriptor` entries because audio-thread
  `update_parameters` cannot reallocate FFT buffers.

  `length` is doubly suspect — its docstring claims it is for
  "sequencer-style modules pre-allocated step/slot count" but the only
  module that actually reads `shape.length` is `pitch_shift`, where
  it controls FFT processing budget. The sequencer use case never
  materialised. Every other module sets `length: 0`.

A related symptom: `convolution_reverb` and `convolution_reverb/stereo`
are the only modules that override `Module::update_parameters`. They
do so not for custom validation but because they need the unpacked
`ParameterMap` to read `ir_data: FloatBuffer` on the control thread,
build a `NonUniformConvolver`, and swap it in — work that cannot
happen via the audio-thread `ParamView` path. The IR buffer is a
structural input by every criterion (read once at construction,
audio thread never touches it, editing requires non-RT work) but
there is no slot for "structural param that isn't shape," so it
rides as a regular param with a hand-rolled override.

This conflation costs us:

- Every `ModuleShape` literal in tests and harnesses spells out
  `high_quality: false` even when irrelevant.
- The DSL surface for choosing `high_quality` is shape syntax, not
  parameter syntax, which is wrong for the user — it reads as a type
  parameter when it is a quality knob.
- The LSP and any future GUI parameter editor have no uniform handle
  on "params that require a rebuild on edit": they would need to
  special-case shape fields.

## Decision

### 1. Reduce `ModuleShape` to `channels`

`ModuleShape` retains only `channels`. Both `length` and `high_quality`
are removed. With one field left, `ModuleShape` could plausibly be
inlined to a bare `usize`, but keeping the struct preserves room for
future genuinely descriptor-shaping fields and keeps `describe(shape)`
signatures stable.

### 2. Segregate structural and realtime params physically

Structural params cannot be a flag on a unified parameter table. The
realtime pipeline (ADR 0045) deliberately removed string-typed and
otherwise non-packable values so that `ParamFrame` is fixed-size,
numeric-only, and allocation-free on the audio thread. File paths and
other structured construction-time inputs need to come back, but they
must not touch the realtime carrier.

So the descriptor splits into two parameter tables:

```rust
pub struct ModuleDescriptor {
    pub realtime_params: Vec<ParameterDescriptor>,   // packed, numeric
    pub structural_params: Vec<ParameterDescriptor>, // free-form, ctrl-thread
    // ports, name, etc.
}
```

- **Realtime params** keep today's behaviour: declared via the
  existing `float_param` / `int_param` / `enum_param` / `bool_param`
  builders, fed into `compute_layout`, packed into `ParamFrame`,
  consumed via `ParamView` by `update_validated_parameters` on either
  the control or audio thread. The packer can statically refuse
  non-packable types, restoring the audio-thread invariant.
- **Structural params** live in a separate control-thread carrier
  (`ParameterMap`-shaped, free-form). They support all the realtime
  types *plus* string-typed values (file paths, plugin URIs, anything
  else construction needs). They never appear in `ParamFrame` layout,
  never reach the audio thread, never need to be allocation-free.

A structural parameter:

- Is declared via builders like `.structural_string_param("ir_path")`,
  `.structural_bool_param("high_quality", false)`,
  `.structural_int_param("fft_size", 128, 4096, 1024)`.
- Is read by `Module::prepare` from the initial structural carrier
  passed alongside `shape` and (numeric) initial params; used for
  sizing internal allocations and decoding file-backed inputs.
- Is **invisible** to the audio thread by construction — it has no
  layout slot.
- When edited on the control thread, triggers a rebuild of *that
  instance only*: the planner constructs a new `Box<dyn Module>` with
  the new structural values, hands it over the existing arc-table
  swap path, and retires the old instance. The descriptor is unchanged
  so no graph rewire, no cable reallocation, no port re-binding.

Structural values are construction-time only by definition: a
structural edit means "build a new instance and swap." So they are
constructor arguments, not a post-construction call. The `Module`
trait absorbs structural into `prepare`:

```rust
fn prepare(
    audio_environment: &AudioEnvironment,
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    structural: &StructuralParams,
) -> Result<Self, BuildError> where Self: Sized;
```

Two consequences:

- `prepare` becomes fallible (file decode, IR partitioning, any
  structural-driven init can fail). Today's infallible `prepare`
  reflects the fact that there is nothing it does that *can* fail;
  fusing structural in forces honesty.
- There is no "prepared but not structurally configured" intermediate
  state; the instance is either fully constructed or does not exist.
  Eliminates a class of "did I remember to apply structural?" bugs.

The existing `update_validated_parameters(&ParamView)` keeps its
contract unchanged: realtime, audio-thread-safe, no strings. There is
no separate `apply_structural` trait method — structural edits go
through instance rebuild.

`high_quality` becomes a structural `bool` param on the modules that
use it (`pitch_shift`, `delay`, `stereo_delay`). `length` (used only
by `pitch_shift` for FFT processing budget) becomes a structural `int`
param on `pitch_shift` alone. `ir_path` (a structural string param)
on `convolution_reverb` replaces the `File` / `FloatBuffer` /
`FloatBufferId` route — the module reads the path in `prepare`,
decodes and partitions the IR, builds the `NonUniformConvolver`. The
bespoke `update_parameters` override is retired.

### 3. ABI surface for structural params

Structural params cross the FFI on the control thread, so allocation
and copying are unconstrained. They cannot ride `ParamFrame` (numeric,
fixed-layout, no strings).

Structural values are absorbed into the existing `prepare` ABI entry
point — not a separate call. Construction is one fallible step:

```c
int32_t prepare(
    const AudioEnvironment* env,
    const uint8_t* descriptor_blob, size_t descriptor_len,
    uint64_t instance_id,
    const uint8_t* structural_blob, size_t structural_len,
    /* out */ void** instance_out,
    /* out */ char* error_buf, size_t error_cap
);
```

The structural blob is a positional packed encoding, slot order
matching `descriptor.structural_params`:

The blob is a positional packed encoding, slot order matching
`descriptor.structural_params`:

```text
[u16 slot_count]
  for each slot:
    [u8  type_tag]      // 0=bool, 1=i64, 2=f64, 3=string (utf-8), ...
    [u32 value_len]
    [value_len bytes]   // little-endian for numeric, utf-8 for string
```

Symmetric with the realtime path: `ParamFrame` carries the realtime
slots, `StructuralBlob` carries the structural slots, both
schema-from-descriptor. The plugin SDK provides decode helpers
mirroring the existing `ParamView` helpers for the realtime side.

Call sequence:

1. Host calls `prepare(env, descriptor, instance_id, structural_blob)`
   — plugin allocates, reads structural values (file paths, sizes,
   quality flags), decodes / preallocates / builds DSP state. Returns
   error on file-not-found, decode failure, etc.
2. Host calls `update_validated_parameters(instance, initial_frame)` —
   plugin applies realtime params.
3. Audio thread takes over.

On structural edit, the planner runs `prepare` again with the new
blob to build a fresh instance, then swaps via the existing arc-table
path and retires the old instance. No new mechanism.

### 4. Three tiers, named

| Tier | Lives in | Changes descriptor? | Edit cost |
|------|----------|---------------------|-----------|
| Shape (`channels` only) | `ModuleShape` | yes | full graph rebuild (different module type) |
| Structural param | `ParameterDescriptor { structural: true }` | no | swap one `Box<dyn Module>` |
| Realtime param | `ParameterDescriptor { structural: false }` | no | audio-thread `update_parameters` |

## Consequences

### Positive

- `ModuleShape` is honest: every field shapes the descriptor.
- `high_quality` (and any future construction-time knob) goes through
  the normal parameter pipeline: typed keys, DSL parameter syntax,
  LSP completions, GUI editing.
- The planner's existing instance-swap path (used for hot-reload, ADR
  0044) is reused for structural param edits. No new mechanism.
- Audio thread is unaffected: structural params are simply skipped in
  `update_parameters`.
- **`FileProcessor` and the file-resolution planner pass become
  deletable.** Currently `file("path.wav")` produces a
  `ParameterValue::File(PathBuf)`, which the planner's
  [`resolve_file_params`](../patches-planner/src/builder/mod.rs) walks
  before build, calling a per-module `FileProcessor` registered in the
  `Registry` to decode the file into a `Vec<f32>`, replacing the entry
  with `ParameterValue::FloatBuffer(Arc<[f32]>)`. The audio thread then
  reads it via a `FloatBufferId` slot in `ParamFrame`. This whole
  pipeline exists because file params were squeezed into the
  realtime-param pipeline that demands packed numeric frames, and the
  decoded data had to be produced somewhere before the frame was built.

  With structural params, file params become `structural` and the file
  path travels as a string-typed structural param value. The planner's
  control-thread structural-update call hands the path to the module's
  `prepare` (or the structural-update hook for live edits); the module
  decodes/preprocesses the file inside its own code, stashing whatever
  internal state it likes (raw samples, partitioned FFTs,
  `NonUniformConvolver`, wavetable banks). The audio thread sees the
  resolved DSP state, never a buffer id, never a path. Three things go
  away: `resolve_file_params` (no host-side preprocessing pass), the
  `FileProcessor` trait and registry (no host-side decoder lookup), and
  the `FloatBufferId` route through `ParamFrame` for file payloads
  (`File` and `FloatBuffer` `ParameterValue` variants can be retired
  along with their buffer slots in the layout). Modules that need
  unchanged-since-last-build fast paths handle that themselves by
  comparing the new path/mtime to cached state.

  This is also what makes file-backed plugins FFI-able for the first
  time: the plugin owns its decoder, the ABI just carries the
  structural slot containing the path.

### Negative

- Module authors must know which tier each construction-time value
  belongs in. Mistake mode: marking a descriptor-shaping value as
  structural would let the descriptor lie about its own ports.
  Mitigation: documentation, and `ModuleShape` stays the only knob
  that affects port counts so the type system catches the common case.
- One more flag on `ParameterDescriptor`. Acceptable; it pays for the
  removal of `high_quality` from `ModuleShape` and clears a category
  of future special-casing.
- **No cross-instance sharing of decoded structural inputs.** Each
  module instance decodes its own copy in `prepare`; two ConvReverbs
  with the same `ir_path` will read the file twice, FFT it twice, and
  hold two `NonUniformConvolver`s. Not a regression — today's
  `resolve_file_params` is also per-instance — but the new model makes
  the non-sharing explicit rather than incidental. Realistic
  frequency is low (distinct reverbs almost always want distinct IRs;
  stereo is already one instance, two channels). Future mitigations
  if needed: host-exposed decoded-bytes cache via an ABI helper, or
  module-side static `Arc` cache. Deferred until profiling shows it
  matters.

### Migration

- Remove `length` and `high_quality` from `ModuleShape`; update every
  literal (`ModuleShape { channels: _, length: _, high_quality: _ }` →
  `ModuleShape { channels: _ }`). Most call sites become much shorter.
- Add `structural` flag to `ParameterDescriptor`; default `false`
  preserves all existing param behaviour.
- Add `.structural()` builder on the existing param builders. Add
  `high_quality` structural bool param to `pitch_shift`, `delay`,
  `stereo_delay`; add `length` structural int param to `pitch_shift`.
  Read each in `new()` from the initial frame instead of
  `descriptor.shape.*`.
- Wire planner: on structural param edit, queue an instance rebuild
  rather than an audio-thread param update. Detection point is the
  diff between old and new param frames at hot-reload / control-thread
  boundary.
- DSL grammar: with `ModuleShape` reduced to `channels`, the
  `shape_block` collapses to a single positional arg —
  either a scalar (`int` or `<param_ref>`) or an alias list:

  ```text
  shape_block = { "(" ~ (scalar | alias_list)? ~ ")" }
  ```

  Examples:

  ```text
  module mix: StereoMixer(8)
  module mix: StereoMixer([drums, bass, guitar])
  module mix: Mixer(<channels>)            # template passthrough
  ```

  The `shape_arg` rule and the `channels:` named-key form are deleted.
  Template-passthrough lands uniformly at `Foo(<channels>)` regardless
  of whether the bound value is a numeric arity or an alias list — an
  alias list *is* an int with provenance (length sets count, names
  label channels). Variable-arity templates (ADR 0019) are preserved
  unchanged: their `int` params bind either form.

  Structural params take the same syntax as any other param (named,
  in the params block); `high_quality` and `length` move from the
  (former) shape position — where they were never expressible anyway,
  always defaulted — to the params block.
- Retire the `File → FloatBuffer → FloatBufferId` pipeline:
  delete `resolve_file_params` and the `FileProcessor` trait/registry,
  remove the `File` and `FloatBuffer` `ParameterValue` variants and
  their buffer slots in `ParamFrame` / layout, drop the
  `fetch_buffer_*` accessors on `ParamView`. File paths flow as
  string-typed structural params; modules decode in `prepare` /
  structural-update.
- Migrate `convolution_reverb` (and any future file-backed module) to
  declare `ir_data` as a structural string param, decode in `prepare`,
  delete the bespoke `update_parameters` override.

## Future work: warm-state migration

Structural edits replace one instance with another: the new instance
starts cold. For most cases this is correct — a different IR file is
a different convolver, a different FFT window is a different latency
profile. For some cases it is a regression — bumping `high_quality`
on a long delay erases the delay-line tail, even though the user's
intent was a quality-only tweak.

Three escape hatches, in increasing cost:

1. **Lift to realtime.** If the value can be honoured on the audio
   thread (interpolation, ramping, lock-free swap), declare it
   `realtime` and accept the audio-thread cost. Right answer for any
   "knob the user expects to twiddle live."
2. **Split the module.** Structural shell + realtime inner. Edits to
   the inner preserve state by definition.
3. **Warm-state hand-off.** New instance constructed off-thread in
   `prepare`, then a control-thread call decants survivable state
   from the old instance into the new before the audio-thread swap.
   Bulk memcpy lives on the control thread, not the audio thread.

The third option is the open design question. Naive "audio-thread
`migrate_from`" only works for tiny state — copying a 2 s stereo
convolution tail (~768 KB at 48 kHz) during the swap blocks the next
sample. The realistic shape is a three-phase protocol:

- Audio thread, last tick before swap: writes a snapshot into a
  pre-allocated scratch buffer (sized by the new module up-front).
- Control thread: `new.adopt_warm_state(&snapshot)` interprets the
  scratch into the new instance's representation. Bulk memcpy lives
  here, off the audio thread.
- Audio thread, swap tick: pointer swap, old to cleanup ring.

The per-module protocol — a serde for warm state — is non-trivial,
and most plausible consumers (delay lines, convolution tails, FDN
matrices) have substantial state. Even the small cases (biquad
histories, ramp positions) only pay off when the user expectation
demands continuity.

Deferred until a concrete user-facing surprise motivates it. A
follow-up ADR would specify: which structural edits are eligible
(descriptor-hash equality between old and new), the snapshot
allocation contract, the failure mode (migrate-not-supported falls
back to today's drop-and-replace), and whether the new module's
`prepare` runs before or after `adopt_warm_state`.
