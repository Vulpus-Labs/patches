---
id: "0711"
title: LSP tap-component completions, validation, and param hinting
priority: medium
created: 2026-04-26
---

## Summary

Make the LSP authoritative for the five tap component types and
their parameter schemas:

- `meter`, `spectrum`, `osc`, `gate_led`, `trigger_led`.

Surface area:

1. **Validation.** Reject unknown component names at parse/expand
   time (today they slip through into `TapType::from_ast_name` which
   returns `None`). Emit a structural diagnostic with span on the
   bad token, suggesting valid alternatives.
2. **Completion after `~`** (in tap-target context). Offer all five
   component names. Trigger when the cursor sits immediately after
   `~` at a top-level cable endpoint position.
3. **Completion after `+`** (compound-tap context). Offer the four
   not-yet-listed components, filtered for cable-kind compatibility:
   `trigger_led` cannot mix with audio components; if the existing
   list contains any of `meter` / `spectrum` / `osc` / `gate_led`,
   omit `trigger_led` (and vice versa).
4. **Param-key validation and completion** *inside* the parens of a
   tap target. Param keys may be:
   - Unqualified, when the tap declares exactly one component
     (e.g. `~meter(level, window: 25)` resolves `window` →
     `meter.window`).
   - Qualified by a listed component name (e.g.
     `~meter+spectrum(level, meter.window: 25)`).

   Validation rejects:
   - Unqualified keys on compound taps (already in place per
     `validate_tap_params`; verify still hits).
   - Keys whose qualifier is not one of the listed components.
   - Keys that are not in the qualifier's known schema.

   Completion offers:
   - On the current tap, the union of valid keys (qualified for
     compound taps, unqualified for simple).

## Param schemas

Authoritative, mirrored from `patches-observation`:

| Component      | Param              | Type   | Default                       |
|----------------|--------------------|--------|-------------------------------|
| `meter`        | `decay`            | float  | 300 (ms)                      |
| `meter`        | `window`           | float  | 50 (ms)                       |
| `osc`          | `window_ms`        | float  | derived from current consts   |
| `osc`          | `snap_zero_cross`  | bool   | `false`                       |
| `spectrum`     | (none)             | —      | —                             |
| `gate_led`     | (none)             | —      | —                             |
| `trigger_led`  | (none)             | —      | —                             |

The schema lives in *one* place — likely a const map in
`patches-observation` (or a shared crate) that both the runtime
parameter lookup and the LSP completer/validator read. Avoid forking
the source of truth.

## Acceptance criteria

- [ ] Single source-of-truth schema (`pub const TAP_PARAM_SCHEMA: &[
  (TapType, &[(name, ParamKind)])]` or similar) in
  `patches-observation` or a new shared crate; both runtime
  `lookup_*` and LSP completer/validator import from it.
- [ ] Validation: unknown component name → structural error
  (`Code::TapUnknownComponent` or similar new variant). Span on the
  component identifier.
- [ ] Validation: unknown param key for a given component →
  structural error (`Code::TapUnknownParam`).
- [ ] Validation: param qualifier doesn't match any listed component
  → already covered by `Code::TapUnknownQualifier`; verify still
  fires for the new components.
- [ ] LSP completion: trigger characters `~` and `+` registered;
  completion provider returns component names in tap-target
  context. Filtered against already-listed components and cable-kind
  compatibility.
- [ ] LSP completion: inside `(...)` of a tap target, offer param
  keys. Unqualified for simple taps; qualified (`meter.window`,
  `osc.window_ms`, …) for compound taps.
- [ ] LSP hover (extend existing tap hover): show param schema for
  the component under cursor.
- [ ] Tree-sitter grammar / queries updated if needed for the
  context-detection (verify what `tree_nav.rs` exposes today).
- [ ] Tests:
  - DSL: `~unknown(x)` rejected with new code.
  - DSL: `~meter(x, bogus: 1)` rejected.
  - DSL: `~osc(x, snap_zero_cross: true)` accepts (after 0710
    lands).
  - LSP: completion at `~|` (cursor at `|`) returns five entries.
  - LSP: completion at `~meter+|` returns three (audio types minus
    `meter`, excluding `trigger_led`).
  - LSP: completion inside `~osc(|)` returns `window_ms`,
    `snap_zero_cross`.

## Dependencies / sequencing

- Blocked on 0710 for the osc params to validate against.
- Should land alongside or just after 0710 so the tests cover the
  full schema.

## Notes

- Cable-kind compatibility lives in `validate_tap_params` already
  (`TapMixedCableKinds`); the LSP filter mirrors that rule on the
  completion side so users can't construct an invalid compound by
  accepting a suggestion.
- Cross-references: ADR 0054 (DSL surface), ticket 0698 (existing
  LSP tap diagnostics + hover), ticket 0709 (scope), ticket 0710
  (osc params).
