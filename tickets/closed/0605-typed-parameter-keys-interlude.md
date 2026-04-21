---
id: "0605"
title: ADR 0046 interlude — typed parameter keys
priority: high
created: 2026-04-21
---

## Summary

Interlude between ADR 0045 Spike 5 and Spike 6. Retrofit `ParamView` and
`ModuleDescriptor` with kind-typed parameter names. See
[adr/0046-typed-parameter-keys.md](../../adr/0046-typed-parameter-keys.md).

Moves three runtime-bug classes (wrong kind, missing index, typo) to
compile errors. Single source of truth for `(name, kind, shape)` per
module via `module_params!`.

## Acceptance criteria

- [x] Typed name / key types in `patches-core` (`FloatParamName`, `FloatParamArray`, `FloatParamKey`, …, `EnumParamName<E>`, `ParamEnum` trait).
- [x] `ParamKey` trait; `ParamView::get<K>` added.
- [x] `module_params!` macro emitting typed consts.
- [x] `ModuleDescriptor` builder takes typed names (hard switch).
- [x] Every in-process module migrated to `module_params!` + typed
      access.
- [x] `params_enum!` consumers carry `ParamEnum` (done in Phase A) and
      are used via `EnumParamName<E>`.
- [x] Legacy string-based `ParamView` getters (`float`, `int`, `bool`,
      `enum_variant`, `buffer`) removed.
- [x] Compile-fail tests: wrong kind, scalar-vs-array mismatch, typo
      (ticket 0607).
- [x] `cargo test` + `cargo clippy` clean.

## Notes

### Phase A — landed

- [patches-core/src/params.rs](../../patches-core/src/params.rs): typed
  name and key types, `ParamKey` trait, `ParamEnum` trait.
- [patches-core/src/module_params.rs](../../patches-core/src/module_params.rs):
  `module_params!` macro emitting typed consts under a sibling
  `pub mod params`.
- [patches-core/src/modules/params_enum.rs](../../patches-core/src/modules/params_enum.rs):
  `params_enum!` now also emits a `ParamEnum` impl — no separate
  derive needed in the migration phase.
- [patches-core/src/param_frame/view.rs](../../patches-core/src/param_frame/view.rs):
  `ParamView::get<K: ParamKey>` added alongside the existing
  string-based getters. Uses a new `lookup_static` internal that
  hashes `(&'static str, u16)` without constructing an owned
  `ParameterKey`. Workspace builds; `cargo test -p patches-core` green.

### Phase B — pending, mechanical but wide

Steps in order:

1. **Switch `ModuleDescriptor` builder signatures**: change
   `float_param(name: &'static str, …)` → `float_param(name:
   FloatParamName, …)`, and same for `int_param`, `bool_param`,
   `enum_param`, `buffer_param`, plus each `_multi` variant taking the
   corresponding `*ParamArray`. Drop `file_param` / `song_name_param`
   (ADR 0045 § 1 already removed them from the update path; keep
   `ParameterKind::File` / `SongName` only if descriptor consumers
   outside `Module::describe` still need them).
   - `enum_param` becomes generic: `fn enum_param<E: ParamEnum>(self,
     name: EnumParamName<E>, default: E) -> Self` — variant list pulled
     from `E::VARIANTS`, wire default is `default.to_variant()`.
2. **For every module file under `patches-modules/src/`** (see list
   below), add a `module_params!` block near the top and rewrite:
   - `describe` — every `.float_param("x", …)` → `.float_param(params::x, …)`;
     `.float_param_multi("x", n, …)` → `.float_param_multi(params::x, n, …)`,
     etc.
   - `update_validated_parameters` — every `params.float("x")` →
     `p.get(params::x)`; `.float("x", i)` (if any) → `p.get(params::x.at(i as u16))`.
3. **Ripple into non-module consumers** of the builder:
   `patches-vintage`, `patches-ffi`, any `Module::describe` outside
   `patches-modules` (e.g. the `test-plugins/*` crates). Grep for
   `\.float_param\(` / `\.int_param\(` / `\.enum_param\(` /
   `\.bool_param\(` / `\.buffer_param\(`.
4. **Retire the legacy `ParamView` string getters** (`float`, `int`,
   `bool`, `enum_variant`, `buffer` taking `impl Into<ParameterKey>`)
   once every call site is migrated. Delete `fetch_*_static` doc-hidden
   wrappers only if they prove redundant with direct inlining; they are
   cheap to keep.
5. **Retire `params_enum!`** in favour of `#[derive(ParamEnum)]` *or*
   keep `params_enum!` as-is and rename the ticket promise: the macro
   already emits `ParamEnum` (Phase A). ADR 0046 § 4 cites
   `#[derive(ParamEnum)]` as the ergonomic wrapper, but a proc-macro
   crate is a fresh dep and `params_enum!` reads cleanly. Recommend
   keeping `params_enum!` and updating ADR 0046 prose to match.
6. **Compile-fail tests** under `patches-core/tests/compile_fail/`
   using `trybuild` (already a workspace dev-dep? check) covering:
   - `let _: i64 = view.get(params::dry_wet);` (wrong kind assign)
   - `view.get(float_array_name)` without `.at(i)` (scalar-vs-array
     via `ParamKey` not impl'd for `FloatParamArray`)
   - `p.get(params::undefined_name)` (undefined ident)
7. **`cargo clippy --all-targets -- -D warnings`** clean; full
   `cargo test` clean under the allocator trap feature where it is
   already enabled.

### Module list (patches-modules/src/)

```text
adsr.rs           audio_in.rs        audio_out.rs        bitcrusher.rs
clap_drum.rs      claves.rs          clock.rs            cymbal.rs
delay.rs          drive.rs           glide.rs            hihat.rs
host_transport.rs kick.rs            lfo.rs              limiter.rs
midi_cc.rs        midi_drumset.rs    midi_in.rs          mono_to_poly.rs
ms_ticker.rs      noise.rs           oscillator.rs       pitch_shift.rs
poly_adsr.rs      poly_midi_in.rs    poly_osc.rs         poly_quant.rs
poly_sah.rs       poly_sum.rs        poly_svf.rs         poly_to_mono.rs
poly_tuner.rs     poly_vca.rs        quant.rs            ring_mod.rs
sah.rs            snare.rs           stereo_delay.rs     stereo_limiter.rs
```

Plus subdir modules under `convolution_reverb/`, `fdn_reverb/`,
`filter/`, `master_sequencer/`, `mixer/`, `pattern_player/`,
`poly_filter/`.

### Open design points

- `file_param` / `song_name_param`: post-ADR 0045 § 1 these no longer
  reach the audio thread. Decision for Phase B: leave them as
  string-typed on the builder (they are off-thread resolution sugar and
  don't participate in `ParamView`), or promote to
  `FileParamName` / `SongNameParamName` for uniformity. Recommend
  leaving them string-typed — they have no `ParamView::get` site.
- `buffer_param` on the builder doesn't exist today; `FloatBuffer`
  parameters are produced by planner resolution from `File` descriptors,
  not declared directly. Phase B may not need a `buffer_param` at all.
