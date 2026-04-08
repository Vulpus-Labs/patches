# patches-dsl test report

**Test run date:** 2026-04-02
**Result:** 84 tests passed, 0 failed, 0 ignored

The `patches-dsl` crate has two pipeline stages — a PEG parser (`parser.rs`) and a template expander (`expand.rs`) — and tests are organised into three test files covering these stages.

---

## 1. Parser (`parser_tests.rs`) — 7 tests

The parser converts `.patches` source text into an AST. It handles module declarations, connections, parameters, unit literals (Hz, kHz, dB, note names), and structural syntax.

### 1.1 Positive fixture parsing

**Test:** `positive_fixtures_parse_ok`
**What it does:** Parses five fixture files (`simple`, `scaled_and_indexed`, `array_params`, `voice_template`, `nested_templates`) and asserts each returns `Ok`.
**Expected:** All five fixtures parse without error.
**Result:** PASS — all five parse successfully.

### 1.2 Literal parse-error propagation

**Test:** `int_literal_overflow_returns_parse_error`
**What it does:** Parses a module with an overflowing integer literal (`99999999999999999999`) and verifies a clean `ParseError` is returned rather than a panic.
**Expected:** `Err(ParseError)` with message containing "invalid integer literal".
**Result:** PASS — error returned with expected message.

### 1.3 Unit-literal parsing

**Test:** `positive_unit_literals`
**What it does:** Parses the `unit_literals.patches` fixture file and asserts it succeeds.
**Expected:** `Ok`.
**Result:** PASS.

### 1.4 Unit-literal conversions

**Test:** `unit_literal_conversions`
**What it does:** Verifies that dB, Hz, kHz, and note-name literals are correctly converted to their internal representation (linear amplitude for dB, V/OCT offset from C0 for frequency/note values). Tests 14 cases:

| Literal | Expected value | Conversion rule |
|---------|---------------|-----------------|
| `0dB` | 1.0 | 10^(0/20) = 1.0 |
| `-6dB` | ~0.501187 | 10^(-6/20) |
| `0DB`, `0Db` | 1.0 | Case-insensitive dB |
| `440Hz` | ~4.7539 | log2(440 / C0_HZ) V/OCT |
| `440hz`, `440HZ` | ~4.7539 | Case-insensitive Hz |
| `0.44kHz` | ~4.7539 | kHz to Hz then V/OCT |
| `C0` | 0.0 | Base octave |
| `C4` | 4.0 | 4 octaves above C0 |
| `c4` | 4.0 | Case-insensitive note |
| `A4` | 4.75 | (4*12 + 9) / 12 = 57/12 |
| `Bb2` | ~2.833 | (2*12 + 10) / 12 = 34/12 |
| `A#-1` | ~-0.167 | (-1*12 + 10) / 12 = -2/12 |

**Expected:** Each parsed scalar matches expected float to within 1e-9.
**Result:** PASS — all 14 conversions match.

### 1.5 Unit-literal error cases

**Test:** `unit_literal_errors`
**What it does:** Verifies that negative Hz (`-440Hz`), zero Hz (`0Hz`), and zero kHz (`0.0kHz`) all produce parse errors.
**Expected:** `Err` for all three.
**Result:** PASS.

### 1.6 Note-like identifier fallthrough

**Test:** `note_like_ident_is_string`
**What it does:** Parses `C4foo` and verifies it falls through from note-literal matching (due to the word-boundary check) to an unquoted string identifier.
**Expected:** `Scalar::Str("C4foo")`.
**Result:** PASS.

### 1.7 Negative fixture parsing

**Test:** `negative_fixtures_parse_err`
**What it does:** Parses six deliberately malformed fixture files (`missing_arrow`, `malformed_index`, `malformed_scale`, `unknown_arrow`, `bare_module`, `unclosed_param_block`) and asserts each returns `Err`.
**Expected:** All six produce parse errors.
**Result:** PASS.

---

## 2. Expander (`expand_tests.rs`) — 41 tests

The expander takes the AST and inlines templates, substitutes parameters, composes cable scales at template boundaries, and produces a `FlatPatch` of concrete modules and connections.

### 2.1 Flat patch passthrough (no templates)

**Test:** `flat_passthrough_simple`
**What it does:** Parses and expands the `simple.patches` fixture. Verifies modules `osc` and `out` exist and exactly 2 connections are present.
**Expected:** 2 modules, 2 connections.
**Result:** PASS.

**Test:** `flat_passthrough_module_types`
**What it does:** Verifies `osc` has type `"Osc"` and `out` has type `"AudioOut"`.
**Expected:** Correct type names.
**Result:** PASS.

**Test:** `flat_passthrough_params_preserved`
**What it does:** Verifies the `frequency` parameter on `osc` is converted from `440Hz` to V/OCT (log2(440/C0_HZ)).
**Expected:** Float value ~4.7539 (within 1e-9).
**Result:** PASS.

**Test:** `flat_arrow_normalisation`
**What it does:** Verifies that both `->` and `<-` arrows produce normalised from/to direction (from=osc, to=out).
**Expected:** All connections have from_module="osc", to_module="out".
**Result:** PASS.

### 2.2 Single template expansion

**Test:** `single_template_modules_namespaced`
**What it does:** Expands `voice_template.patches` and verifies that inner modules are namespaced (e.g. `v1/osc`, `v2/env`) while intermediate template instances (`v1`, `v2`) do not appear as FlatModules.
**Expected:** 9 modules present, 2 template-instance IDs absent.
**Result:** PASS.

**Test:** `single_template_internal_connections`
**What it does:** Verifies internal template connections are preserved with namespacing: `v1/osc.sine -> v1/vca.in` and `v1/env.out -> v1/vca.cv`.
**Expected:** Both connections present.
**Result:** PASS.

**Test:** `single_template_boundary_rewired`
**What it does:** Verifies boundary connections are rewired through templates: `seq.pitch -> v1.voct` becomes `seq.pitch -> v1/osc.voct`; `out.in_left <- v1.audio` becomes `v1/vca.out -> out.in_left`.
**Expected:** Both rewired connections present.
**Result:** PASS.

**Test:** `single_template_scale_composed`
**What it does:** Verifies cable scales are preserved through boundary rewiring: `seq.pitch -[0.5]-> v2.voct` results in scale 0.5 on the final connection; `out.in_right <-[0.8]- v2.audio` results in scale 0.8.
**Expected:** Scales 0.5 and 0.8 (within 1e-12).
**Result:** PASS.

### 2.3 Parameter substitution

**Test:** `param_substitution_supplied`
**What it does:** Verifies that supplied template parameters override defaults: `v1` instantiated with `attack: 0.005, sustain: 0.6`, `decay` uses default `0.1`.
**Expected:** attack=0.005, decay=0.1, sustain=0.6.
**Result:** PASS.

**Test:** `param_default_used_for_v2`
**What it does:** Verifies `v2` uses all default values: attack=0.01, sustain=0.7.
**Expected:** Defaults applied.
**Result:** PASS.

### 2.4 Nested template expansion

**Test:** `nested_template_modules_namespaced`
**What it does:** Expands `nested_templates.patches` with a `filtered_voice` template containing an inner `voice` template. Verifies double-namespaced modules like `fv/v/osc` and that intermediates (`fv`, `fv/v`) are absent.
**Expected:** 4 modules present, 2 template IDs absent.
**Result:** PASS.

**Test:** `nested_template_boundary_rewired`
**What it does:** Verifies multi-level boundary rewiring: `seq.pitch -> fv.voct` reaches `fv/v/osc.voct`; `out.in_left <- fv.audio` comes from `fv/filt.out`.
**Expected:** Both rewired connections present.
**Result:** PASS.

**Test:** `nested_template_internal_connection`
**What it does:** Verifies cross-level internal connection: `v.audio -> filt.in` inside `filtered_voice` becomes `fv/v/vca.out -> fv/filt.in`.
**Expected:** Connection present.
**Result:** PASS.

### 2.5 Error cases

**Test:** `error_missing_required_param`
**What it does:** Template declares `freq: float` (no default); caller omits it.
**Expected:** ExpandError containing "missing required parameter".
**Result:** PASS.

**Test:** `error_unknown_param`
**What it does:** Caller supplies `unknown_param: 42.0` not declared in template.
**Expected:** ExpandError containing "unknown parameter".
**Result:** PASS.

**Test:** `error_recursive_template`
**What it does:** A directly self-recursive template definition.
**Expected:** ExpandError containing "recursive".
**Result:** PASS.

### 2.6 Warning diagnostics

**Test:** `no_warnings_for_implicit_scale_or_index`
**What it does:** A simple connection `osc.sine -> out.in_left` with no explicit scale or index.
**Expected:** No warnings emitted.
**Result:** PASS.

**Test:** `no_warnings_when_scale_and_indices_explicit`
**What it does:** Connection `osc.sine[0] -[1.0]-> out.in_left[0]` with everything explicit.
**Expected:** No warnings.
**Result:** PASS.

### 2.7 `<param>` syntax, unquoted strings, shorthand, structural interpolation

**Test:** `unquoted_string_literal_eq_quoted`
**What it does:** Verifies `waveform: sine` (unquoted) produces the same `Scalar::Str("sine")` as `waveform: "sine"` (quoted).
**Expected:** Both produce identical `Value::Scalar(Scalar::Str("sine"))`.
**Result:** PASS.

**Test:** `param_ref_in_param_block_substituted`
**What it does:** Template with `{ frequency: <freq> }` instantiated with `freq: 880.0` — verifies the inner module gets `frequency: 880.0`.
**Expected:** frequency=880.0.
**Result:** PASS.

**Test:** `shorthand_param_entry_expands_like_key_value`
**What it does:** Verifies `{ <attack>, <decay>, release: 0.3 }` (shorthand) expands identically to `{ attack: <attack>, decay: <decay>, release: 0.3 }` (explicit).
**Expected:** Identical parameter values on both modules.
**Result:** PASS.

**Test:** `port_label_interpolation`
**What it does:** Verifies that a template with port labels parses and expands without error.
**Expected:** No error.
**Result:** PASS.

**Test:** `port_label_param_interpolation_resolves_correctly`
**What it does:** Verifies that `osc.<port_name>` syntax parses and expands correctly.
**Expected:** No error.
**Result:** PASS.

**Test:** `scale_interpolation_param_ref`
**What it does:** `-[<gain>]->` with `gain: 0.5` — verifies the connection has scale 0.5.
**Expected:** scale=0.5 (within 1e-12).
**Result:** PASS.

**Test:** `error_unknown_param_ref_in_port_label`
**What it does:** `osc.<nonexistent>` references an undeclared param.
**Expected:** ExpandError containing "nonexistent".
**Result:** PASS.

**Test:** `error_non_numeric_param_ref_in_scale`
**What it does:** `-[<waveform>]->` where `waveform=0.5` (numeric) — verifies this succeeds and produces scale 0.5.
**Expected:** scale=0.5.
**Result:** PASS.

**Test:** `ast_port_label_literal_and_param_variants_parse`
**What it does:** Verifies that `osc.sine` parses as `PortLabel::Literal("sine")` at the AST level.
**Expected:** Correct AST variant.
**Result:** PASS.

### 2.8 Variable arity expansion (`[*n]`, `[k]`, group params)

**Test:** `arity_expansion_basic_three_connections`
**What it does:** Template with `in: in[size]` and `mixer.in[*size] <- $.in[*size]`, instantiated with `size: 3`. Verifies 3 connections into `b/mixer.in` at indices 0, 1, 2.
**Expected:** 3 connections, indices [0, 1, 2].
**Result:** PASS.

**Test:** `arity_expansion_boundary_template_fan`
**What it does:** Same pattern with `size: 4` — verifies 4 connections into `fan/m` at indices 0..3.
**Expected:** 4 connections, indices [0, 1, 2, 3].
**Result:** PASS.

**Test:** `param_index_single_connection`
**What it does:** `m.in[channel]` with `channel: 2` produces one connection with `to_index == 2`.
**Expected:** 1 connection, to_index=2.
**Result:** PASS.

**Test:** `arity_expansion_scale_composed`
**What it does:** Each arity-expanded connection carries the caller's scale: `osc.sine -[0.5]-> fan.ch[N]` produces scale 0.5 on each.
**Expected:** 2 connections, each with scale 0.5 (within 1e-12).
**Result:** PASS.

### 2.9 Group parameters

**Test:** `group_param_broadcast`
**What it does:** `level: 0.8` on a `level[size]: float` group with `size: 3` produces `level/0`, `level/1`, `level/2` all equal to 0.8.
**Expected:** All three slots = 0.8.
**Result:** PASS.

**Test:** `group_param_explicit_array`
**What it does:** `level: [0.1, 0.2, 0.3]` produces `level/0=0.1`, `level/1=0.2`, `level/2=0.3`.
**Expected:** Distinct per-slot values.
**Result:** PASS.

**Test:** `group_param_explicit_array_length_mismatch_error`
**What it does:** 3-element array supplied to a 2-slot group.
**Expected:** ExpandError containing "length" or "arity".
**Result:** PASS.

**Test:** `group_param_per_index`
**What it does:** `level[0]: 0.8, level[1]: 0.3` with 3-slot group — slot 2 uses default 1.0.
**Expected:** level/0=0.8, level/1=0.3, level/2=1.0.
**Result:** PASS.

**Test:** `limited_mixer_example_end_to_end`
**What it does:** Full `LimitedMixer` example from ADR 0019 — verifies inner module `lm/m` has type `Sum`, 3 in-connections, 1 out-connection.
**Expected:** Correct structure.
**Result:** PASS.

### 2.10 Arity / group param error cases

**Test:** `error_arity_param_missing`
**What it does:** `[*nonexistent]` references an undeclared param.
**Expected:** ExpandError containing "nonexistent".
**Result:** PASS.

**Test:** `error_arity_mismatch`
**What it does:** `[*n]` on both sides of a connection with different values (n=2, m=3).
**Expected:** ExpandError containing "arity" or "mismatch".
**Result:** PASS.

### 2.11 AST structure verification

**Test:** `ast_port_index_variants`
**What it does:** Verifies `[0]`, `[k]`, and `[*n]` parse to `PortIndex::Literal(0)`, `PortIndex::Alias("k")`, and `PortIndex::Arity("n")` respectively.
**Expected:** Correct AST variants.
**Result:** PASS.

**Test:** `ast_port_group_decl_arity`
**What it does:** Verifies `in: freq, audio[n]` parses to two `PortGroupDecl` structs with correct names and arity.
**Expected:** `freq` (arity=None), `audio` (arity=Some("n")).
**Result:** PASS.

**Test:** `ast_param_decl_arity`
**What it does:** Verifies `level[size]: float = 1.0` parses to a `ParamDecl` with `arity: Some("size")`.
**Expected:** Correct arity.
**Result:** PASS.

### 2.12 Scale composition — both factors non-trivial

These tests ensure scale multiplication works when neither factor is 1.0, guarding against bugs that would silently ignore one side.

**Test:** `scale_in_port_both_outer_and_inner_nontrivial`
**What it does:** Inner boundary scale 0.4, outer connection scale 0.5.
**Expected:** Composed scale = 0.5 * 0.4 = 0.2 (within 1e-12).
**Result:** PASS.

**Test:** `scale_out_port_both_outer_and_inner_nontrivial`
**What it does:** Inner out-boundary scale 0.4, outer connection scale 0.5.
**Expected:** Composed scale = 0.4 * 0.5 = 0.2 (within 1e-12).
**Result:** PASS.

**Test:** `scale_three_level_with_nontrivial_outer`
**What it does:** Three nested boundary levels each with scale 0.5, outer connection 0.8.
**Expected:** Composed scale = 0.8 * 0.5^3 = 0.1 (within 1e-12).
**Result:** PASS.

**Test:** `scale_param_ref_boundary_with_nontrivial_outer`
**What it does:** Inner `<boost>` param resolves to 0.4, outer scale 0.5.
**Expected:** Composed scale = 0.4 * 0.5 = 0.2 (within 1e-12).
**Result:** PASS.

**Test:** `scale_negative_survives_boundary_composition`
**What it does:** Inner boundary scale -0.5 (phase inversion), outer scale 0.8.
**Expected:** Composed scale = 0.8 * (-0.5) = -0.4 (within 1e-12).
**Result:** PASS.

---

## 3. Torture tests (`torture_tests.rs`) — 36 tests

Stress tests for edge cases in deeply nested templates, variable arity, and error detection.

### 3.1 Three-level nested templates with name aliasing

**Test:** `deep_alias_module_ids_namespaced`
**What it does:** Three-level nesting (`outer -> middle -> inner`) with parameter name aliasing at each level (`tempo->speed->rate`). Verifies fully-namespaced module IDs like `top/mid/i/lfo`.
**Expected:** 7 concrete modules present, 3 intermediate template IDs absent.
**Result:** PASS.

**Test:** `deep_alias_params_propagate_through_aliases`
**What it does:** `outer(tempo=3.0)` aliased through two levels to `inner(rate=3.0)`. Verifies `lfo.rate` and `env.decay` both equal 3.0.
**Expected:** Both params = 3.0.
**Result:** PASS.

**Test:** `deep_alias_connections_rewired_through_three_boundaries`
**What it does:** `clk.semiquaver -> top.clock` rewired through three template boundaries to reach `top/mid/i/lfo.sync` and `top/mid/i/env.gate`.
**Expected:** Both connections present.
**Result:** PASS.

**Test:** `deep_alias_out_port_chain_rewired`
**What it does:** `out.in_left <- top.mix` rewired through three out-port levels to `top/amp.out`.
**Expected:** Connections to both `in_left` and `in_right` from `top/amp.out`.
**Result:** PASS.

**Test:** `deep_alias_internal_connections_within_inner`
**What it does:** Connections declared inside `inner` template body appear as fully-prefixed flat connections (`top/mid/i/lfo.sine -> top/mid/i/vca.in`, `top/mid/i/env.out -> top/mid/i/vca.cv`).
**Expected:** Both connections present.
**Result:** PASS.

**Test:** `deep_alias_cross_boundary_internal_connections`
**What it does:** Cross-boundary connections within the template hierarchy: `inner.audio -> filt.in` (middle body) and `filt.out -> amp.in` (outer body).
**Expected:** `top/mid/i/vca.out -> top/mid/filt.in` and `top/mid/filt.out -> top/amp.in` both present.
**Result:** PASS.

**Test:** `deep_alias_scale_composed_across_three_boundaries`
**What it does:** Each boundary carries scale 0.5, external connection scale 1.0.
**Expected:** Composed scale = 1.0 * 0.5^3 = 0.125 (within 1e-12) on both `lfo.sync` and `env.gate` connections.
**Result:** PASS — both connections have scale 0.125.

### 3.2 All arity / index forms in one template

Tests use the `arity_everything.patches` fixture which combines `[*n]`, `[k]`, group params (broadcast/array/per-index), and `<param>` scale interpolation in a single template.

**Test:** `arity_everything_module_ids`
**What it does:** Verifies all expected module IDs exist across three bus instances (`bus_b`, `bus_a`, `bus_p`) and two redirectors (`red0`, `red2`).
**Expected:** 11 modules present.
**Result:** PASS.

**Test:** `arity_everything_broadcast_gains`
**What it does:** `bus_b` with `{ gains: 0.7 }` broadcast to 3-slot group.
**Expected:** `gains/0`, `gains/1`, `gains/2` all = 0.7.
**Result:** PASS.

**Test:** `arity_everything_array_gains`
**What it does:** `bus_a` with `{ gains: [0.8, 0.6, 0.4] }`.
**Expected:** `gains/0=0.8`, `gains/1=0.6`, `gains/2=0.4`.
**Result:** PASS.

**Test:** `arity_everything_per_index_gains_with_default_fallback`
**What it does:** `bus_p` with `{ gains[0]: 1.0, gains[1]: 0.3 }` — slot 2 falls back to default 0.5.
**Expected:** `gains/0=1.0`, `gains/1=0.3`, `gains/2=0.5` (default).
**Result:** PASS.

**Test:** `arity_everything_expansion_produces_n_connections_into_mixer`
**What it does:** `[*n]` with n=3 produces exactly 3 connections into each bus's mixer at indices 0..2.
**Expected:** 3 connections per bus, indices [0, 1, 2].
**Result:** PASS.

**Test:** `arity_everything_param_index_on_concrete_destination`
**What it does:** Redirector template with `[ch]` param index: `red0` (ch=0) routes to `in[0]`, `red2` (ch=2) routes to `in[2]`.
**Expected:** Correct to_index values.
**Result:** PASS.

**Test:** `arity_everything_boost_scale_applied_via_param_ref`
**What it does:** `<-[<boost>]-` with `bus_b boost=0.9` and `bus_a boost=0.5`.
**Expected:** `bus_b/m` output scale = 0.9, `bus_a/m` output scale = 0.5 (within 1e-12).
**Result:** PASS.

**Test:** `arity_everything_param_index_on_dollar_boundary_source`
**What it does:** `sel.in <- $.ch[solo]` with different `solo` values per bus instance: bus_b solo=0 (receives from osc0), bus_a solo=1 (osc1), bus_p solo=2 (osc2).
**Expected:** Correct source module per bus.
**Result:** PASS.

**Test:** `arity_everything_solo_channel_fans_to_both_mixer_and_sel`
**What it does:** The solo channel's external connection fans out to both `m.in[solo]` (via arity expansion) and `sel.in` (via param index).
**Expected:** Both connections present for bus_b (osc0 -> bus_b/m.in[0] AND osc0 -> bus_b/sel.in).
**Result:** PASS.

### 3.3 Circular reference detection

**Test:** `mutual_recursion_detected`
**What it does:** Templates A and B reference each other (A -> B -> A).
**Expected:** ExpandError containing "recursive".
**Result:** PASS.

**Test:** `three_cycle_detected`
**What it does:** Templates A -> B -> C -> A form a three-way cycle.
**Expected:** ExpandError containing "recursive".
**Result:** PASS.

### 3.4 Mistyped call-site values

**Test:** `error_float_param_used_as_port_label`
**What it does:** Float param referenced as `osc.<gain>` — port labels must resolve to strings.
**Expected:** ExpandError containing "string" or "gain".
**Result:** PASS.

**Test:** `error_float_value_as_arity_param`
**What it does:** `[*n]` where n resolves to float 2.5 — arity must be a non-negative integer.
**Expected:** ExpandError containing "integer" or "arity".
**Result:** PASS.

**Test:** `error_negative_arity_param`
**What it does:** `[*n]` where n resolves to -2 — arity must be non-negative.
**Expected:** ExpandError containing "non-negative", "negative", or "arity".
**Result:** PASS.

**Test:** `error_string_value_as_scale`
**What it does:** `-[<label>]->` where label resolves to string `"loud"` — scale must be numeric.
**Expected:** ExpandError containing "number", "scale", or "string".
**Result:** PASS.

**Test:** `error_table_element_in_group_param_array`
**What it does:** `{ vals: [{a: 1.0}, {b: 2.0}] }` — array elements must be scalars, not tables.
**Expected:** ExpandError containing "scalar", "element", or "array".
**Result:** PASS.

**Test:** `error_scalar_param_in_param_block`
**What it does:** Scalar param placed in `{ }` param block instead of `( )` shape block.
**Expected:** ExpandError containing "shape", "scalar", or "group".
**Result:** PASS.

**Test:** `error_group_param_in_shape_block`
**What it does:** Group param placed in `( )` shape block instead of `{ }` param block.
**Expected:** ExpandError containing "param block", "group", or "gains".
**Result:** PASS.

### 3.5 Additional edge cases

**Test:** `arity_one_produces_exactly_one_connection`
**What it does:** `[*n]` with n=1 produces exactly one connection at index 0.
**Expected:** 1 connection, to_index=0.
**Result:** PASS.

**Test:** `param_index_zero_is_valid`
**What it does:** `[ch]` with ch=0 — validates zero is not an off-by-one error.
**Expected:** to_index=0.
**Result:** PASS.

**Test:** `two_instances_of_same_template_within_another_have_distinct_namespaces`
**What it does:** Two instances (`va`, `vb`) of a `Voice` template inside a `Duo` template produce distinct namespaces (`duo/va/osc`, `duo/vb/osc`) with independent parameter values (261.6 Hz vs 523.2 Hz) and internal connections.
**Expected:** Distinct modules, correct per-instance params, independent wiring.
**Result:** PASS.

**Test:** `group_param_array_length_mismatch_exact_error`
**What it does:** 4-element array supplied to a 3-slot group.
**Expected:** ExpandError containing "length", "arity", or "mismatch".
**Result:** PASS.

**Test:** `group_param_per_index_out_of_bounds_error`
**What it does:** `gains[5]` on a group with arity 3 — index out of range.
**Expected:** ExpandError containing "range", "bounds", or "arity".
**Result:** PASS.

**Test:** `group_param_no_default_and_no_value_error`
**What it does:** Group param declared without default and not supplied at call site.
**Expected:** ExpandError containing "default", "gains", or "supplied".
**Result:** PASS.

---

## Summary

| Component | Tests | Pass | Fail |
|-----------|-------|------|------|
| Parser — positive fixtures | 2 | 2 | 0 |
| Parser — error propagation | 1 | 1 | 0 |
| Parser — unit/note literals | 4 | 4 | 0 |
| Expander — flat passthrough | 4 | 4 | 0 |
| Expander — single template | 4 | 4 | 0 |
| Expander — parameter substitution | 2 | 2 | 0 |
| Expander — nested templates | 3 | 3 | 0 |
| Expander — error cases | 3 | 3 | 0 |
| Expander — warnings | 2 | 2 | 0 |
| Expander — param/port/scale syntax | 8 | 8 | 0 |
| Expander — variable arity | 4 | 4 | 0 |
| Expander — group params | 5 | 5 | 0 |
| Expander — arity/group errors | 2 | 2 | 0 |
| Expander — AST structure | 3 | 3 | 0 |
| Expander — scale composition (nontrivial) | 5 | 5 | 0 |
| Torture — deep alias (3-level) | 7 | 7 | 0 |
| Torture — arity everything | 9 | 9 | 0 |
| Torture — circular references | 2 | 2 | 0 |
| Torture — mistyped values | 7 | 7 | 0 |
| Torture — edge cases | 5 | 5 | 0 |
| **Total** | **84** | **84** | **0** |

No tests in this crate use bounded numeric expectations (e.g. dB thresholds); all assertions are exact-match or within floating-point epsilon (1e-9 for unit conversions, 1e-12 for scale composition).
