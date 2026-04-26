# ADR 0057 — Host control as boundary-crossing cables

**Date:** 2026-04-26
**Status:** Proposed
**Related:**
[ADR 0054 — Tap DSL syntax and module decomposition](0054-tap-dsl-and-modules.md),
[ADR 0048 — MIDI source and routing modules](0048-midi-source-and-routing-modules.md),
[ADR 0046 — Typed parameter keys](0046-typed-parameter-keys.md)

## Context

The CLAP plugin embeds Patches in a DAW. The DAW exposes automatable
parameters: knobs, sliders, toggles. A patch author wants those host-
side controls to drive cables inside the patch (filter cutoff, envelope
attack, mute switches) and the host to publish them as automatable CLAP
parameters with stable names.

Three lanes of input are now in play and must not be conflated:

1. **Patch parameters** — set at patch-definition time, immutable at
   runtime. Typed-key access (ADR 0046). Not host-automatable.
2. **MIDI** — performance input from keyboards, controllers, sequencers.
   Routed through MIDI source modules (ADR 0048). Per-CC, per-channel.
3. **Host control** — DAW-side automation knobs/sliders/toggles bound
   to a CLAP parameter list, varying at audio-block rate.

Routing host control through MIDI CC was considered. Rejected: it
contends with external MIDI sources for CC numbers, smuggles host
automation through a protocol designed for performance input, and
forces patch authors to pick CC numbers instead of meaningful names.

A separate lane is the right factoring. The remaining design question
is how host control surfaces in the DSL and how it crosses the
host→audio boundary. ADR 0054 already established a pattern for
boundary-crossing cables: the `~` sigil with synthesised modules
reading/writing the backplane. Host control inverts the direction.

## Decision

### 1. Surface syntax — inverse `~` form

Host controls are declared in cable expressions as **sources**, the
inverse direction of taps:

```text
~knob(filter_cutoff, range: 20..20000, default: 1000) -> filter.voct
~slider(vca_attack, range: 0.001..2.0, default: 0.01) -> vca.attack_cv
~toggle(reverb_bypass, default: false) -> reverb.bypass
```

- `~kind(name, k: v, ...)` is a **host control source**. The `~`
  prefix is the same reserved sigil as taps; `~` marks any boundary-
  crossing cable, with the arrow direction distinguishing source from
  sink.
- Kinds: `knob | slider | toggle`. The kind is a **UI rendering hint**
  for the host-side surface; semantically `knob` and `slider` are both
  ranged scalar control floats and `toggle` is a boolean. The audio
  side does not branch on kind.
- Parameters following the name configure the host-side surface
  (range, default, label, taper, units). Parsed as typed literals, k/v
  pairs. Forwarded verbatim to the host via the manifest (§5); the
  CLAP plugin interprets them when publishing parameters.
- `name` is the host control's identifier within the patch. Names must
  be unique across all host control sources in the patch. Names form
  a separate namespace from tap names — `~knob(cutoff)` and
  `~tap(cutoff)` may coexist.
- Host control sources are valid **only at top-level patch scope**,
  same rule and reasoning as taps (ADR 0054 §1): exposure is a
  top-down concern decided by the patch author, not the template
  author.

The cable type is `Mono` + `Audio` (CV-shaped) for `knob` / `slider`
and `Mono` + `Trigger` is **not** used — `toggle` produces a
sample-and-hold audio signal at 0.0 or 1.0, not a sub-sample trigger.
A patch author wanting an edge-triggered host control wires a `toggle`
into a derivative module.

### 2. Desugaring — implicit host-control modules

The expander collects all host control sources, groups them by output
cable type, and synthesises one module instance per group:

```text
# input
~knob(cutoff, range: 20..20000) -> filter.voct
~slider(attack, range: 0..2)    -> vca.attack_cv
~toggle(bypass, default: false) -> reverb.bypass

# desugared
module ~host_control : HostControl(channels: 3) {
  @cutoff { slot_offset: 0 }
  @attack { slot_offset: 1 }
  @bypass { slot_offset: 2 }
}
~host_control.out[cutoff] -> filter.voct
~host_control.out[attack] -> vca.attack_cv
~host_control.out[bypass] -> reverb.bypass
```

The synthesised module is a first-class module with the `~` reserved
prefix (same rule as ADR 0054 §2). User modules may not use `~`. SVG
renderers see an ordinary module and may render or suppress it by
flag.

Host-side parameters (range, default, label) are **not** visible on
the audio-side module; they ride the manifest (§5) to the host.

### 3. Slot ordering — alphabetical by control name

The host-control slot index of each source is its position in the
alphabetical sort of all host control source names in the patch.
Same scheme as ADR 0054 §3, same rationale: deterministic mapping
both sides compute independently from the same input list.

When host controls are added or removed the implicit module's
`channels` shape changes, triggering the standard size-change → drop
+ replace path. The host-side CLAP parameter list is republished via
the new manifest. Renames and parameter changes (range, default) on
an unchanged set are pure parameter updates on the existing instance.

### 4. The `HostControl` module

Single module covering all host control sources, sized by the number
of declared sources:

- Input: none (it is a source in patch-graph terms).
- Output: `out[i]`, `i ∈ 0..channels`, `MonoLayout::Audio`.
- Per-channel parameters:
  - `name: String` — manifest correlation, never read after planning.
  - `slot_offset: usize` — first backplane slot owned by this module.
- Per-channel state on the audio side: nothing beyond the cached
  current value of the slot.

The audio thread's per-tick action per channel:
`out[i] = backplane[slot_offset + i]`. Nothing else.

The backplane region for host control is distinct from the tap
region; ADR 0053 already accommodates multiple regions in the
audio↔control plumbing.

The control thread writes the backplane in response to host parameter
updates (CLAP `process()` parameter event queue, sample-accurate or
block-rate as the host provides). Writes are plain stores; the audio
thread reads with `Acquire`, the control thread writes with `Release`,
matching the parameter-update channel established for ADR 0045/0046.
Tearing on individual `f32` is acceptable for control signals at
audio-block boundaries; a brief mid-ramp readback during a parameter
update produces no audible artefact at typical knob rates.

Sub-block automation accuracy is a future concern (sample-accurate
host automation requires per-sample backplane updates). The current
design publishes one value per block per host control, which matches
the resolution most hosts deliver and which the existing parameter
ramp primitive (ADR 0050) can smooth where needed.

### 5. Manifest construction

Out of the desugaring step the planner emits a host-control manifest:

```rust
pub struct HostControlDescriptor {
    pub slot: usize,
    pub name: String,
    pub kind: HostControlKind,    // Knob | Slider | Toggle
    pub params: HostControlParamMap,  // untyped k/v: range, default, label, taper, units
    pub source: ProvenanceTag,
}
pub type HostControlManifest = Vec<HostControlDescriptor>;  // sorted by slot
```

`HostControlParamMap` is an untyped k/v map of literals collected from
the source declaration. The CLAP plugin interprets the map when
publishing the parameter list to the host: `range` becomes the CLAP
parameter range, `default` the default value, `label` the display
name, `taper` the curve mapping, `units` the unit string.

Per-kind parameter validation is deferred — the CLAP plugin rejects
malformed maps with a diagnostic at parameter publication time, not
at parse time. Kinds and their parameter vocabularies are not yet
stable enough to justify DSL-level schema enforcement.

The manifest travels to the CLAP plugin (the host-side observer of
this lane) over the planner→host control ring, parallel to the
tap manifest's planner→observer ring (ADR 0053 §6). The audio side
does not see the manifest; it operates entirely from
`(slot_offset, channel_count)` baked into the `HostControl`
instance.

### 6. CLAP parameter mapping

The CLAP plugin maintains a stable mapping from host control name to
CLAP parameter ID across patch reloads. On manifest update:

- Names already known retain their parameter ID and current value.
- New names get fresh parameter IDs.
- Removed names retain their parameter ID in a tombstone table for
  the session, so DAW automation lanes referencing them are not
  silently rebound to a different control.

Parameter IDs are stable within a CLAP plugin session. Cross-session
stability (saving a DAW project, reopening) is provided by the host
control name embedded in CLAP parameter cookie data; a future ADR
will detail the persistence format if it grows beyond name-based
matching.

### 7. Relationship to MIDI

Host control and MIDI are independent lanes. A patch may use both:

```text
~knob(filter_cutoff) -> filter.voct
midi.cc[74]          -> filter.cv_mod
```

External CC mapping (DAW hardware controller → CLAP host control) is
handled by the host, not Patches. The DAW maps a hardware CC to a
CLAP parameter; that parameter then drives the host control source.
This is the standard CLAP/DAW workflow and Patches stays out of it.

### 8. Relationship to patch parameters

Host control sources do not write patch parameters and cannot be
written from patch parameters. The two are disjoint.

A pattern that previously required a runtime-mutable parameter (e.g.
"filter cutoff that the user can sweep") becomes a host control:
declare a `~knob` and wire it to the cable. The patch parameter
system stays definition-time and immutable as ADR 0046 specifies.

## Consequences

**Positive**

- One sigil, two directions: `~` cables cross the audio boundary in
  whichever direction the arrow points. Symmetric with taps; same
  reserved-prefix rule, same manifest pattern, same drop+replace path
  on set change.
- Audio side stays dumb: a single `HostControl` module that copies
  backplane slots to outputs. No per-kind branching, no host-side
  state, no allocation.
- Three input lanes (parameters, MIDI, host control) are
  syntactically and semantically distinct. No collision, no implicit
  routing.
- DAW automation surfaces use stable, meaningful names instead of CC
  numbers.
- New host control kinds (e.g. `xy_pad`) cost one entry in the kind
  enum and one CLAP-side rendering branch. The audio side and the
  DSL grammar are unchanged.

**Negative**

- Another reserved keyword family on `~` (`~knob`, `~slider`,
  `~toggle`). Documented alongside taps; minor parser surface
  growth.
- Two manifests (tap, host control) flow planner→observer; the
  control plumbing carries both. ADR 0053 already tolerates this but
  the surface area is incremental.
- Sub-block automation accuracy is not addressed. Future work if
  hosts demand it.

**Neutral**

- The CLAP parameter ID stability rule (§6) is the first place
  Patches commits to cross-publication identity for an externally-
  visible name. Equivalent considerations may apply to tap names if
  observer state is ever persisted; not yet.

## Alternatives considered

### Route host control through MIDI CC

Cheap, reuses existing MIDI plumbing. Rejected: contention with
external MIDI sources, semantic mismatch (host automation is not
performance input), forces patch authors to pick CC numbers instead
of names.

### Make host control a kind of patch parameter

Conflates definition-time configuration with runtime control. Breaks
the parameter-immutability invariant established by ADR 0046. The
disjoint-lanes design (§8) preserves it.

### One module per kind (`KnobSource`, `SliderSource`, `ToggleSource`)

Symmetric with ADR 0054's per-cable-type tap modules. Rejected: tap
modules split because cable input types differ (audio vs trigger),
which the type checker enforces. Host control kinds all produce
mono-audio outputs and differ only in host-side rendering, which is
manifest data. One module is enough.

### Per-source CLAP parameter declarations in the patch file

`@clap_param cutoff { range: ..., default: ... }` as a top-level
declaration disjoint from the cable graph. Rejected: it duplicates
the cable wiring (the user has to declare the parameter and then
wire it) and obscures the connection between the host control and
the cable it drives. The inline `~knob(...) -> ...` form keeps
declaration and wiring together.

## Cross-references

- ADR 0054 — tap DSL; this ADR mirrors its sigil, manifest, and
  drop+replace conventions in the source direction.
- ADR 0053 — three-thread observation architecture; the host control
  manifest rides parallel control-ring infrastructure.
- ADR 0048 — MIDI source/routing modules; explicitly disjoint from
  host control.
- ADR 0046 — typed parameter keys; host control is *not* a
  parameter, this ADR preserves that distinction.
- ADR 0050 — coefficient ramp primitive; available for smoothing
  block-rate host control values into per-sample cables where the
  module needs it.
