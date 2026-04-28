# ADR 0059 — Stereo cable kind and unified `Tap` module

**Date:** 2026-04-27
**Status:** Proposed
**Related:**
[ADR 0015 — Polyphonic cables](0015-polyphonic-cables.md),
[ADR 0030 — Trigger and gate input types](0030-trigger-and-gate-input-types.md),
[ADR 0047 — Sub-sample trigger cables](0047-sub-sample-trigger-cables.md),
[ADR 0054 — Tap DSL and modules](0054-tap-dsl-and-modules.md)

## Context

Stereo modules (`stereo_delay`, `convolution_reverb`, `fdn_reverb`,
`stereo_limiter`, `audio_in`, `audio_out`, mixer stereo variants) expose
paired `*_left` / `*_right` mono ports. Every stereo connection in patch
source costs two cables. The pair carries the same kind, layout, and
provenance — there is no case where the two halves diverge — so the
duplication is pure authoring tax. It is also easy to wire one half
correctly and the other half wrong (swapped, missing, or routed from a
different source) with no parser-level diagnostic available because each
half is well-typed in isolation.

ADR 0054 left taps with two underlying modules (`AudioTap`,
`TriggerTap`) and global alphabetical slot ordering keyed on a unique
tap name. With a stereo cable kind in hand, both decisions become
revisable: `meter` taps want to display stereo bars as a single grouped
widget, and a single `Tap` module with mono / stereo / trigger input
ports per channel can replace the two-module split.

## Decision

### 1. New cable kind: `Stereo`

`CableKind` gains a `Stereo` variant. A stereo cable carries `[f32; 2]`
per sample (`L`, `R`). Implementation reuses the existing
`CableValue::Poly([f32; 16])` storage — stereo is a width-2 occupancy
of the same slot — so the cable pool, RTRB transfers, and existing
backplane code do not change shape. Only the type checker, coercion
rules, and module descriptor surface treat `Stereo` distinctly.

`MonoLayout` does not apply to stereo cables. There is no
`Stereo + Trigger` cable: stereo is exclusively audio/CV.

### 2. Coercion rules

| From   | To     | Rule                                         |
| ------ | ------ | -------------------------------------------- |
| Mono   | Mono   | direct (existing)                            |
| Stereo | Stereo | direct                                       |
| Mono   | Stereo | **broadcast**: `L = R = source`              |
| Stereo | Mono   | **rejected** with `CableKindMismatch`        |
| Poly   | Stereo | rejected                                     |
| Stereo | Poly   | rejected                                     |

Mono→stereo broadcast is implemented at the cable-builder level: when
the planner observes a mono source feeding a stereo input it tags the
cable as broadcast and the consumer reads through a thin
`StereoInput::read_broadcast()` helper that returns `(s, s)` from the
underlying mono slot, with no extra audio-thread work and no synthetic
broadcast module. Authoring-side this is silent — no warning, no
conversion node in diagrams.

### 3. New port kind: `StereoInput` / `StereoOutput`

Module descriptors gain `.stereo_in(name)` and `.stereo_out(name)`
builder methods. The descriptor records `CableKind::Stereo` for those
ports. Existing stereo modules migrate from paired mono ports to a
single stereo port; the DSL surface for those modules collapses
accordingly:

```text
# before
osc.out -> delay.in_left
osc.out -> delay.in_right
delay.out_left  -> mix.in_left
delay.out_right -> mix.in_right

# after (broadcast on input, single stereo cable on output)
osc.out      -> delay.in
delay.out    -> mix.in
```

The `_left` / `_right` port-naming convention (CLAUDE.md) is retired
for modules that take a complete stereo signal. It is kept only where
the two halves are *semantically distinct* (e.g. mid/side processors
declared as two mono ports, not as a stereo pair).

### 3a. Stereo↔mono utility modules

Stereo→mono coercion is rejected at the type level (§2). When the
user genuinely wants the two halves as separate mono signals, they
write an explicit utility module:

- `StereoSplitter` — one stereo input (`in`), two mono outputs
  (`out_left`, `out_right`).
- `StereoJoiner` — two mono inputs (`in_left`, `in_right`), one
  stereo output (`out`).

Both are zero-DSP pass-through plumbing. They make the conversion
visible in source and in diagrams, and they are the *only* way to
break or assemble a stereo cable. There is no implicit `.left` /
`.right` accessor on stereo ports; if the user wants the channels
apart they instantiate a splitter.

These are the one place `_left`/`_right` survives in the new
convention, because the splitter and joiner are *defined* by the
asymmetry of their two halves.

### 4. Unified `Tap` module — `TriggerTap` retired

`TriggerTap` is removed. A single `Tap` module replaces both. Per
channel it exposes three input ports — `mono_in`, `stereo_in`,
`trigger_in` — and the desugarer wires exactly one based on the tap
type:

| Tap type        | Port wired to | Input cable kind   |
| --------------- | ------------- | ------------------ |
| `meter`         | `mono_in`     | Mono + Audio       |
| `stereo_meter`  | `stereo_in`   | Stereo             |
| `osc`           | `mono_in`     | Mono + Audio       |
| `spectrum`      | `mono_in`     | Mono + Audio       |
| `gate_led`      | `mono_in`     | Mono + Audio       |
| `trigger_led`   | `trigger_in`  | Mono + Trigger     |

Only `meter` gains a stereo variant in this ADR; `osc` and `spectrum`
remain mono. Stereo metering is the common authoring case and the only
one where pairing produces a meaningful UI win in the current
observation surface.

### 5. Slot allocation

Each tap channel claims slots in declaration order:

- mono / trigger channels: 1 slot
- stereo channels: 2 consecutive slots (`L` at `slot_offset`, `R` at
  `slot_offset + 1`)

The next channel's `slot_offset` is the previous channel's
`slot_offset + width`. The total backplane width per `Tap` instance is
the sum of channel widths.

`MAX_TAPS` (ADR 0053) is reinterpreted as a slot count, not a channel
count, and raised from 32 to **64** (four backplane poly slots) so
stereo-heavy patches keep usable headroom — sixteen stereo meters at 32
slots was the obvious forcing case, and the cost of the bump (~512 B
working set, four-slot zeroing per tick) is below noise. Raising later
would require manifest-format awareness in every observer, so the
move happens alongside the slot-allocation rewrite (ticket 0740) rather
than as a follow-up migration.

### 6. Tap identity, ordering, and slot coalescing

Identity is `(cable_kind, name)`, where `cable_kind` is the underlying
input-port kind (`Mono` / `Stereo` / `Trigger`) — not the user-facing
component label. Two taps of incompatible kind with the same name
coexist as separate channels:

```text
clock.tick     -> ~trigger_led(kick)   # (Trigger, kick) — its own slot
kick_drum.out  -> ~meter(kick)         # (Mono, kick)    — its own slot
```

Two taps of *the same* cable kind with the same name **coalesce onto
one slot**. Their components union, and the cables collapse into a
single edge when the producer matches:

```text
kick_drum.out -> ~meter(kick)
kick_drum.out -> ~osc(kick)
kick_drum.out -> ~spectrum(kick)
# → one slot, components = [meter, osc, spectrum], one cable.
# Equivalent to: kick_drum.out -> ~meter+osc+spectrum(kick)
```

Coalescing makes the separate-declaration form a sugar for the
compound form, and recovers backplane real estate (one slot for three
mono components on the same producer) without forcing the user to
write the compound up front. Where the producers differ
(`a.out -> ~meter(bus)` and `b.out -> ~meter(bus)`), the channels
still coalesce but the resulting two cables target the same input
port; the connectivity validator surfaces that as
`InputAlreadyConnected`, which is the user's bug to fix (the desugarer
does not silently sum).

Ordering across the patch is **source location**, not alphabetical.
The planner walks tap targets in source order and assigns slot offsets
sequentially; the observer keys per-slot state by `(cable_kind, name)`.

### 7. Stereo pair naming convention for `meter`

`stereo_meter` taps publish `L` and `R` to consecutive slots. The
manifest names them `foo/left` and `foo/right` (where `foo` is the
declared tap name). UI subscribers group by stem:

```text
master_bus.out -> ~stereo_meter(master)
# manifest emits two scalar tracks: master/left, master/right
# UI groups them as a single stereo widget labelled "master"
```

`/` is reserved in tap names — user-supplied tap names may not contain
it; the parser rejects names containing `/`. The convention is purely
observer-side; the audio path stays unaware.

### 8. Compound taps

Compound forms (`~a+b+c(name, ...)`) follow the cable-kind rule of
their components. Mixing stereo and mono components is a parse error
(the underlying `Tap` channel is one shape). `meter+spectrum` etc.
remain mono-only as before.

## Consequences

**Positive**

- One cable per stereo connection; halves the line count for typical
  effect-chain wiring and removes a class of swap/miswire bugs.
- Unified `Tap` module: one registration, one descriptor, one set of
  tests. Trigger handling stops being a special module.
- Tap identity by `(tap_type, name)` matches user intent (a "kick" LED
  and a "kick" meter are obviously different things).
- Source-order slot mapping is the order users already read.
- Stereo metering pairs naturally without per-tap UI configuration.

**Negative**

- `CableKind` grows a third variant; every match arm in the type
  checker, planner, and graph validator needs a new branch. Most are
  trivial (`Stereo` rejects on `MonoLayout`-tagged operations).
- Mono→stereo broadcast is a new coercion path. Implementation is small
  but it is the first case where a cable's *consumer-side* shape
  differs from the producer-side shape.
- Stereo modules' port names change; existing patches and tests need
  migration.
- Loss of alphabetical slot ordering changes how slot indices appear in
  hover / diagnostic output. Source-order is at least as obvious but
  IDE consumers that cached "slot 3 = third in alphabetical order" need
  refresh.
- ADR 0054 §3, §4, §5 are partially superseded (single module, source
  order, identity tuple). This ADR is the authority where they
  conflict.

**Neutral**

- `[f32; 16]` poly storage is reused for stereo slots; no new pool
  shape. Stereo cables read 2 lanes and ignore 14, paying poly's
  memory cost for the simpler implementation. Acceptable: stereo
  cables are a small fraction of any real patch's wiring and the
  saving from collapsing pairs of mono cables more than offsets it.
- `_left` / `_right` naming convention shrinks to mid/side and other
  semantically-asymmetric pair cases.

## Alternatives considered

### Stereo as a pair of mono ports with a "linked" tag

Keep current port surface; mark pairs in the descriptor so the planner
and UI can group them. Rejected: doesn't reduce authoring cost (still
two cables in source) and adds a parallel notion of "stereo-ness" that
the type checker has to understand without giving the user a single
stereo cable to manipulate.

### Stereo via `PolyLayout::Stereo`

Reuse `CableKind::Poly` with a layout tag pinning it to channels 0–1.
Rejected: poly cables flow through poly ports (`PolyInput` /
`PolyOutput`); making *some* poly cables connect to a hypothetical
`StereoInput` blurs the kind/layout split. A third top-level kind is
clearer than an exception inside `Poly`.

### Keep `TriggerTap`, only collapse stereo on `AudioTap`

Smaller change, but leaves the two-module asymmetry as a permanent
oddity for one tap type. Single `Tap` is the right shape now that
stereo is forcing the descriptor into per-channel multi-port territory
anyway.

### Alphabetical ordering with `(tap_type, name)` tuple key

Preserves ADR 0054's stability story. Rejected: source order is what
users see and is invariant under name changes within a tap type. A
rename no longer shifts every later slot; only the renamed slot moves
in the manifest's name index, not in the slot table.

## Cross-references

- ADR 0054 — superseded in part (single `Tap` module, source-order
  slot mapping, identity tuple). Its compound-tap and observer-side
  derivation rules are unchanged.
- ADR 0053 — three-thread split; backplane width interpretation
  changes (slots, not channels) but no architectural shift.
- CLAUDE.md "Port naming conventions" — `_left`/`_right` retired for
  symmetric stereo modules.
