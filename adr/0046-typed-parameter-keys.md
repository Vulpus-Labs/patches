# ADR 0046 — Typed parameter keys

**Date:** 2026-04-20
**Status:** Proposed

---

## Context

ADR 0045 introduces `ParamView<'a>` as the read-only parameter access
surface modules see during `update_validated_parameters`. Its getters
in that ADR take `impl Into<ParameterKey>`, where `ParameterKey` is a
`(name, index)` tuple. Nothing in the type system distinguishes a
`Float` parameter's name from an `Int`'s, a `Bool`'s, a `Buffer`'s,
an enum parameter's, or a scalar from an array.

Consequences of the untyped form:

1. A module can call `p.float("delay_ms")` against a parameter whose
   declared kind is `Int`. Perfect-hash lookup on the scalar table
   misses (or, worse, hits a same-name scalar at a different offset
   that happens to align), returning garbage or panicking at runtime.
2. A module can call `p.float("gain")` against an array parameter,
   omitting the channel index. The lookup for `(gain, 0)` succeeds,
   silently returning tap 0's value as if it were the parameter.
3. Descriptor construction (`float_param_multi("gain", n, ...)`) and
   access (`p.float("gain")`) share a string literal and nothing
   else. A typo in either diverges silently until the `ParamLayout`
   build fails to resolve the hash — late, and far from the edit.
4. The `params_enum!` macro in ADR 0045 spike 0 invents a separate
   ergonomic layer for enum access because the base API has nowhere
   to encode the enum type on the key. This leaves enums as a
   special case rather than the uniform top of a kind ladder.

All four failure modes are runtime-only. Three of the four
(type mismatch, missing index, typo) are mechanical mistakes the
compiler could catch if the key carried its kind.

## Goals

The parameter access API must:

1. Make kind mismatches between access site and declaration a
   compile error.
2. Make scalar-vs-array mismatches a compile error.
3. Make parameter-name typos an undefined-identifier error rather
   than a runtime miss.
4. Unify descriptor construction and module access around a single
   source-of-truth for `(name, kind, shape)` triples.
5. Keep per-instance shape (channel count, poly voice count)
   runtime-determined — array length is not a compile-time property.
6. Preserve the zero-allocation, O(1) perfect-hash access performance
   of ADR 0045 §4.

Non-goal: encoding array length in the type system. `Delay::describe`
and similar consult `ModuleShape::channels` at instance build; the
channel count varies per instance and cannot live on a `const`.

## Decision

Introduce kind-typed parameter name constants, declared once per
module via a `module_params!` macro, and consumed by both the
descriptor builder and `ParamView`.

### 1. Typed name types

```rust
pub struct FloatParamName   (&'static str);
pub struct IntParamName     (&'static str);
pub struct BoolParamName    (&'static str);
pub struct EnumParamName<E: ParamEnum>(&'static str, PhantomData<E>);
pub struct BufferParamName  (&'static str);

pub struct FloatParamArray  (&'static str);
pub struct IntParamArray    (&'static str);
pub struct BoolParamArray   (&'static str);
pub struct EnumParamArray<E: ParamEnum>(&'static str, PhantomData<E>);
pub struct BufferParamArray (&'static str);
```

All are `#[repr(transparent)]` over `&'static str` (plus a zero-sized
`PhantomData` for the enum variants). All are `Copy`. Each `new`
constructor is `const fn` so the values sit in `.rodata` and impose
no runtime cost.

Scalar names cannot be indexed; array names produce a `*ParamKey`
only via `.at(i)`:

```rust
impl FloatParamArray {
    pub const fn new(name: &'static str) -> Self { Self(name) }
    pub fn at(self, i: u16) -> FloatParamKey {
        FloatParamKey { name: self.0, index: i }
    }
}
```

Array length is not stored; the bound check happens in the
`ParamView` lookup against the instance's live `ParamLayout`
(which already knows the per-instance channel count).

### 2. `ParamView` exposes a single generic `get<K>`

A `ParamKey` trait ties each key type to its value type via an
associated type; `ParamView::get` dispatches on it.

This API shape relies on the **complete-frame invariant** defined
in ADR 0045 §3: every frame a module receives carries every
declared parameter at its current value. That lets `get` return
`K::Value` directly, never `Option`. If frames were diffs the API
would need `try_get(k) -> Option<K::Value>` and modules would have
to cache prior values themselves; the complete-frame decision
makes the clean form possible.

```rust
pub trait ParamKey {
    type Value;
    fn fetch(self, view: &ParamView<'_>) -> Self::Value;
}

impl ParamKey for FloatParamKey {
    type Value = f32;
    fn fetch(self, v: &ParamView<'_>) -> f32 { v.fetch_float(self) }
}
impl ParamKey for IntParamKey {
    type Value = i64;
    fn fetch(self, v: &ParamView<'_>) -> i64 { v.fetch_int(self) }
}
impl ParamKey for BoolParamKey {
    type Value = bool;
    fn fetch(self, v: &ParamView<'_>) -> bool { v.fetch_bool(self) }
}
impl ParamKey for BufferParamKey {
    type Value = Option<FloatBufferId>;
    fn fetch(self, v: &ParamView<'_>) -> Option<FloatBufferId> {
        v.fetch_buffer(self)
    }
}
impl<E: ParamEnum> ParamKey for EnumParamKey<E> {
    type Value = E;
    fn fetch(self, v: &ParamView<'_>) -> E { v.fetch_enum::<E>(self) }
}

// Scalar names reuse the same trait with index = 0:
impl ParamKey for FloatParamName {
    type Value = f32;
    fn fetch(self, v: &ParamView<'_>) -> f32 {
        FloatParamKey { name: self.0, index: 0 }.fetch(v)
    }
}
// ... same pattern for IntParamName, BoolParamName, EnumParamName,
// BufferParamName ...

impl<'a> ParamView<'a> {
    pub fn get<K: ParamKey>(&self, k: K) -> K::Value { k.fetch(self) }
}
```

Use site:

```rust
self.dry_wet = p.get(params::DRY_WET);              // f32
self.gains[i] = p.get(params::GAIN.at(i));          // f32
self.mode     = p.get(params::MODE);                // FilterMode
self.ir_id    = p.get(params::IR);                  // Option<FloatBufferId>
```

Return type is driven entirely by the key's `ParamKey::Value`; no
turbofish is needed because inference flows from the key type.
Kind mismatch (e.g. `let x: i64 = p.get(params::GAIN.at(i));`) is
a compile error at the assignment site.

Array access is always via `.at(i)`. There is no `p.get(GAIN)` for
a bare array name returning an iterator — that form would need a
GAT on `ParamKey::Value` to let the iterator borrow the view, and
for zero real ergonomic gain. If batch iteration is wanted later,
add a separate `p.array(GAIN)` method alongside `get`; don't force
it through `ParamKey`.

`*ParamKey` is `{ name: &'static str, index: u16 }` — one struct
per kind, not interchangeable.

Index bounds check: `p.get(GAIN.at(i))` for `i >= channels` misses
the perfect hash. Debug: assert-panic. Release: the
`ParamViewIndex` guarantees the hash is total over valid keys and
returns a sentinel (NaN / 0 / `None` depending on kind) for misses,
with a per-runtime counter incremented. Misses are a module bug and
should surface in CI via the fuzzing in ADR 0045 Spike 9.

### 3. Descriptor builder is generic over typed names

```rust
impl ModuleDescriptor {
    pub fn float_param(
        self, n: FloatParamName, min: f32, max: f32, d: f32,
    ) -> Self;
    pub fn float_param_multi(
        self, n: FloatParamArray, count: usize,
        min: f32, max: f32, d: f32,
    ) -> Self;
    pub fn int_param(
        self, n: IntParamName, min: i64, max: i64, d: i64,
    ) -> Self;
    pub fn int_param_multi(
        self, n: IntParamArray, count: usize,
        min: i64, max: i64, d: i64,
    ) -> Self;
    pub fn bool_param(self, n: BoolParamName, d: bool) -> Self;
    pub fn enum_param<E: ParamEnum>(
        self, n: EnumParamName<E>, d: E,
    ) -> Self;
    pub fn buffer_param(self, n: BufferParamName) -> Self;
    // ... array variants for the others ...
}
```

Calling `float_param_multi(params::GAIN, ...)` with `GAIN: IntParamArray`
is a type error. Descriptor construction and access site now cannot
disagree on `(name, kind, scalar-vs-array)`.

### 4. The `module_params!` macro

Single declaration site emits the typed consts:

```rust
module_params! {
    Delay {
        dry_wet:   Float,
        delay_ms:  IntArray,
        gain:      FloatArray,
        feedback:  FloatArray,
        tone:      FloatArray,
        drive:     FloatArray,
    }
}
```

Expands to:

```rust
pub mod params {
    use super::*;
    use patches_core::params::*;

    pub const DRY_WET:  FloatParamName = FloatParamName::new("dry_wet");
    pub const DELAY_MS: IntArray       = IntParamArray::new("delay_ms");
    pub const GAIN:     FloatParamArray = FloatParamArray::new("gain");
    pub const FEEDBACK: FloatParamArray = FloatParamArray::new("feedback");
    pub const TONE:     FloatParamArray = FloatParamArray::new("tone");
    pub const DRIVE:    FloatParamArray = FloatParamArray::new("drive");
}
```

For enum parameters, the sibling `#[derive(ParamEnum)]` attaches the
variant type:

```rust
module_params! {
    Filter {
        cutoff: Float,
        mode:   Enum<FilterMode>,
    }
}
// → pub const MODE: EnumParamName<FilterMode> = EnumParamName::new("mode");
```

The macro does **not** own `describe`. `describe(shape)` remains
hand-written — it must consult `shape.channels` and `shape.poly_voices`
at runtime, which the macro cannot see. The macro's sole job is to
emit the typed name consts and (for arrays) keep kind + string
aligned with the descriptor builder's required argument types. See
`Delay::describe` ([patches-modules/src/delay.rs:93](../patches-modules/src/delay.rs#L93))
for the reference shape of a runtime-channel-count descriptor.

### 5. Use site (Delay worked example)

```rust
module_params! {
    Delay {
        dry_wet:  Float,
        delay_ms: IntArray,
        gain:     FloatArray,
        feedback: FloatArray,
        tone:     FloatArray,
        drive:    FloatArray,
    }
}

impl Module for Delay {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("Delay", shape.clone())
            .mono_in("in")
            .mono_in("drywet_cv")
            .mono_in_multi("sync_ms",  n)
            // ... ports unchanged; port names not yet typed ...
            .float_param      (params::DRY_WET,            0.0,  1.0, 1.0)
            .int_param_multi  (params::DELAY_MS, n,        0,    2000, 500)
            .float_param_multi(params::GAIN,     n,        0.0,  1.0,  1.0)
            .float_param_multi(params::FEEDBACK, n,        0.0,  1.0,  0.0)
            .float_param_multi(params::TONE,     n,        0.0,  1.0,  1.0)
            .float_param_multi(params::DRIVE,    n,        0.1, 10.0,  1.0)
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.dry_wet = p.get(params::DRY_WET);
        for i in 0..self.taps as u16 {
            self.delay_ms[i as usize]  = p.get(params::DELAY_MS.at(i)) as f32;
            self.gains[i as usize]     = p.get(params::GAIN.at(i));
            self.feedbacks[i as usize] = p.get(params::FEEDBACK.at(i));
            let tone                   = p.get(params::TONE.at(i));
            if (tone - self.tones[i as usize]).abs() > f32::EPSILON {
                self.tones[i as usize] = tone;
                self.tone_filters[i as usize].set_tone(tone);
            }
            self.drives[i as usize] = p.get(params::DRIVE.at(i));
        }
    }
}
```

Kind errors, scalar/array errors, and typo errors now all surface at
compile time. Only per-instance index bounds remain a runtime
concern, and they are unavoidable given runtime channel counts.

## Consequences

### Positive

- Three classes of bug (wrong kind, missing index, typo) move from
  runtime to compile time.
- Descriptor and access site share a single source of truth per
  parameter. Renames happen once, at the `module_params!` block.
- Enum parameters stop being a special-cased ergonomic add-on; they
  sit on the same typed-key ladder as every other kind.
- The API is a single method. `p.get(params::GAIN.at(i))` works
  for every kind; return type is driven by the key's associated
  `ParamKey::Value`, so readers see one shape everywhere.

### Negative

- One more macro (`module_params!`) and ten typed name types.
  Offset: retires the ad-hoc `params_enum!` from ADR 0045 Spike 0.
- Port names remain untyped in this ADR (still `&str` strings on
  `mono_in`, `mono_in_multi`, etc.). A follow-up ADR could extend
  the same pattern to ports, but their kind surface (mono/poly,
  in/out, stereo suffixes — see CLAUDE.md port naming conventions)
  is richer and worth treating separately.
- FFI plugins cross the ABI with untyped `(name, index)` pairs on
  the wire (ADR 0045 §3 is offset-indexed, so names are not on the
  wire at all past load). Typed keys are a host-Rust-side
  ergonomic layer; the ABI itself is unaffected.

### Alternatives considered

- **Leave `ParamView` untyped and rely on runtime asserts.**
  Rejected: runtime-only enforcement is the problem this ADR
  exists to solve. Asserts catch the bug on a developer's machine;
  typed keys prevent it from being written.
- **Generate `describe` from the macro too.** Rejected: modules
  like Delay drive port counts from `shape.channels` and need the
  live `n`. A declarative `describe` would either lose that
  flexibility or need a sub-DSL rich enough to express it, at
  which point the macro is fighting the language. Handwritten
  `describe` + typed name consts splits the concerns cleanly.
- **Store array length in the `*ParamArray` type via const
  generics.** Rejected: channel counts are runtime per instance
  (`shape.channels` in `Module::describe`). A const generic would
  force compile-time-known counts, excluding every poly and
  multichannel module in the tree.

## Relationship to ADR 0045

This ADR refines the `ParamView` access surface defined in ADR 0045
§4 and the `ModuleDescriptor` builder implied by §1–§3. It does not
change any wire format, ABI function, or threading contract. It is
purely a type-system sharpening of the Rust-side API.

The `params_enum!` macro sketched in ADR 0045 Spike 0 is subsumed
by `module_params!` + `#[derive(ParamEnum)]` here. When ADR 0045
Spike 0 lands, it should emit typed `EnumParamName<E>` consts
directly rather than a parallel string-based lookup.

## Implementation placement (relative to ADR 0045's spike sequence)

Spikes 0, 1, 2, 3 are complete and Spike 5 is in progress with the
untyped `impl Into<ParameterKey>` API. Land this ADR as an
**interlude between Spike 5 and Spike 6**: after Spike 5 finishes
moving every in-process module onto `ParamView`, tighten the
`ParamView` getters and `ModuleDescriptor` builder to typed keys
and migrate the same call sites in one pass.

Rationale for placement:

- Retrofitting after Spike 5 is one migration (rewrite module
  access sites once: string → typed), not two. Had this ADR existed
  before Spike 3 it would have ridden in on that spike's getter
  definitions; that ship has sailed, but the cost of catching up
  now is bounded and mechanical.
- Spike 6 (table growth via atomic pointer swap) has no interaction
  with key typing. The interlude can complete before Spike 6 starts
  without blocking anything or competing for the same files.
- Spike 7 (FFI ABI redesign, first external plugin) benefits from
  having typed keys in place so the example plugin's Rust-side
  descriptor and access code demonstrate the final shape.
- The `params_enum!` sketch from Spike 0 retires during the
  interlude as its consumers are rewritten to `EnumParamName<E>`.

Interlude work items:

1. Declare typed name types (`FloatParamName`, `FloatParamArray`,
   … `EnumParamName<E>`, `ParamEnum` trait + derive) in
   `patches-core`.
2. Add `module_params!` macro emitting the typed consts.
3. Genericise `ModuleDescriptor::{float_param, float_param_multi,
   int_param, …}` to take typed names; update all call sites.
4. Change `ParamView` getters to take typed keys.
5. Migrate every in-process module's `describe` and
   `update_validated_parameters` to `module_params!` + typed access.
6. Replace `params_enum!` usages with `EnumParamName<E>`; remove
   the macro.
7. Add compile-fail tests for the three error classes (wrong kind,
   scalar-vs-array mismatch, typo).

Once the interlude lands, Spikes 6–9 proceed as originally
described in ADR 0045.
