# ADR 0077 — Tracker step event grammar

## Status

Proposed (2026-05-20)

## Context

The tracker step grammar has accreted incrementally. Three operations
on a held note exist today, with three independent surface forms and
overlapping runtime paths:

| Operation                                  | Surface today                 | Runtime path                                |
| ------------------------------------------ | ----------------------------- | ------------------------------------------- |
| Sustain a held note                        | `~`                           | tie branch in `apply_step` (no cv change)   |
| Slide pitch within one tick                | `value>value`                 | trigger + slide arm in `apply_step`         |
| Slide pitch over multiple ticks            | `slide(n, start, end)` macro  | expands to triggered head + tied-slide tail |
| Roll N triggers in one tick                | `value*N`                     | repeat arm in `apply_step`                  |
| Roll N triggers over multiple ticks (E152) | `value*N ~ ~`                 | row-build span annotation + spread schedule |

Two gaps in this surface:

1. **No way to change pitch without retriggering.** A bare `~` tie
   cannot carry a new cv1 — the parser sets `cv1 = 0.0` on tie tokens
   and the runtime's tie branch reads `step.cv1` only when it sees a
   slide target. So "step from C4 to Eb4 with the gate staying high,
   no trigger" has no expression. The author who wants this
   musical idea must invoke `slide(2, C4, Eb4)` and accept a one-tick
   ramp instead of an instant change.
2. **Slide-over-N-ticks requires a macro.** The `slide(n, start, end)`
   generator expands at parse time into a fixed-shape sequence of
   tied sub-steps. There is no in-row syntax for "ramp from this note
   to that note over the next N ticks," so authors cannot interleave
   slides with arbitrary other tracker decoration on the same row.

A third concern is **token ambiguity**. The tie token `~` collides
visually (and in the LSP cursor classifier) with the `~tap(...)` and
`~host_control` cable-endpoint forms. The classifier disambiguates by
ancestor walk; the underlying confusion still costs LSP authoring
clarity.

E152 ("tie-span repeats") landed the row-build annotation pass that
turns `value*N ~ ~` into a `repeat_span = 3` anchor with absorbed-tie
neighbours. That work introduced the *concept* of channel-stateful
row-build — a continuation cell's meaning depends on the anchor it
follows. The same lever is what we need for slides over N ticks, and
the row-build pass is the natural place to land it.

PatternPlayer's `apply_step` itself is an if-else cascade over
`(absorbed_by_roll, gate, trigger, repeat>1, cv1_end.is_some(),
cv2_end.is_some())` flags, with state-reset boilerplate duplicated
across arms. The implicit cell taxonomy is never named in the code.
Generalising slides requires either widening the cascade or naming
the taxonomy explicitly.

### Existing tie semantics under E152

A tie cell after a `*N` anchor is *absorbed* into the roll's spread
schedule (`absorbed_by_roll = true`); a tie cell after a non-roll
step is the old *sustain* (gate stays, cv unchanged). The two
interpretations are disambiguated by the row-build pass annotating
each cell based on the preceding anchor's `repeat > 1`. E152 made
implicit absorption a settled idea for one modifier (`*N`); this ADR
generalises that idea to slides.

### Why not just extend `slide(n, ...)`

The macro form imposes a single-cell representation that hides the
intermediate ticks. Authors cannot tap a single midpoint tick (e.g.
to retrigger envelopes, drop the gate briefly, or sidechain another
event off it). Row-level cell-by-cell notation is necessary to
expose the intermediate ticks as authoring targets. The macro
remains useful as a shorthand for the common case of "uniform ramp,
N ticks, no row decoration"; it does not preclude an in-row form.

## Decision

### Surface grammar

Replace `~` with `_` for tie cells. Drop the `~`-as-tie token from
the step grammar entirely. The `~tap(...)` and `~name` cable-endpoint
uses are unaffected.

Introduce three new step cell shapes alongside the existing
`value`, `value>value` (one-tick slide), `value:cv2`, `value*N`, `.`
(rest):

| Cell shape    | Trigger? | Slide on this tick?                    | cv1 effect at start of tick | Cv1 effect at end of tick     |
| ------------- | -------- | -------------------------------------- | --------------------------- | ----------------------------- |
| `value`       | yes      | depends on prior                       | snap to `value`             | unchanged                     |
| `_`           | no       | depends on prior                       | unchanged                   | depends on slide-open state   |
| `/value`      | no       | no (closes any open slide at boundary) | snap to `value`             | unchanged                     |
| `value>`      | yes      | opens                                  | snap to `value`             | (ramping; resolved at close)  |
| `>_`          | no       | yes (opens if not open)                | unchanged                   | (ramping; resolved at close)  |
| `>value`      | no       | yes (closes within tick)               | unchanged                   | `value`                       |
| `value>value` | yes      | yes (one-tick slide)                   | snap to first value         | second value                  |

The `value>value` form is kept as sugar for the common single-tick
slide-into-hold case; it is exactly equivalent to `value> /value`
under the unified semantics.

### Unified close rule

Every slide closes when the row-build pass encounters a cell that is
not `_`, `>_`, or another slide-open. The close cell's value is the
slide's endpoint:

- A `value` close cell ends the slide at the tick boundary leading
  into the cell and *also* fires a fresh trigger on this tick. The
  previous ticks rendered as a ramp landing exactly at `value` at
  start of this tick; this tick is a fresh note onset at `value`.
- A `/value` close cell ends the slide at the tick boundary, with
  no trigger. This tick is a sustained hold at `value`. The
  musical idea is "the slide landed; hold without retrigger."
- A `>value` close cell ramps within this tick, with no trigger.
  This tick is itself a slide tick, ending at `value`.

Bare `value` cells are therefore **always locally readable as fresh
triggers**. The lead-in shape (flat hold vs. ramp) is determined by
prior cells, not by anything on the close cell itself.

### Continuation absorption

A `_` cell flows through whatever modifier was last open on the
channel:

- After a non-roll, non-slide anchor: sustain (gate stays, cv held).
- After a `value*N` anchor: absorbed into the roll's spread schedule
  (E152, unchanged).
- After a `value>` or `>_`: absorbed into the slide's ramp.

A `>_` cell explicitly opens or continues a slide from the channel's
current cv. It is required when the slide starts on a non-value cell
(i.e. when the author wants "hold this note for a bit, then start
ramping somewhere in the row without retriggering"). Examples:

```
E4 _ >_ /G4    # hold E4 tick 1, hold E4 tick 2, slide tick 3, hold G4 tick 4
E4> _ /G4      # slide E4 to G4 over ticks 1+2, hold G4 tick 3
E4> _ >G4      # slide E4 to G4 over ticks 1+2+3
E4> _ G4       # slide E4 to G4 over ticks 1+2, fresh G4 trigger on tick 3
E4 /G4         # change cv to G4 on tick 2, no retrigger
```

### Row-build pass: StepEffect

Row-build resolves each cell into a `StepEffect`, a tagged effect
the pattern player applies directly:

```rust
pub enum StepEffect {
    Silent,                                     // rest `.`
    StartNote {
        cv1: f32,
        cv2: f32,
        slide: Option<SlideOpen>,               // value>  or  value>value
        roll: Option<RollSpec>,                 // value*N
    },
    StepCv { cv1: f32, cv2: Option<f32> },      // /value: snap cv, no trigger
    Hold,                                       // bare `_` with no active modifier
    SlideFlow,                                  // `_` absorbed by a slide; or `>_`
    SlideCloseInTick { cv1: f32 },              // >value
    AbsorbedRoll,                               // `_` absorbed by E152 roll spread
}

pub struct SlideOpen {
    pub close_cv1: f32,           // resolved by the row-build close cell
    pub closes_at_boundary: bool, // true for `/value` / `value` close
}

pub struct RollSpec {
    pub count: u8,                // N from `*N`
    pub span: u8,                 // E152 repeat_span
}
```

Row-build is **channel-stateful**: it walks each channel's step run
left-to-right tracking `(slide_open: bool, roll_active: bool)` and
emitting one `StepEffect` per cell. The pass is the single
authoritative resolution of the surface grammar into runtime
semantics; the runtime sees only `StepEffect`s and never reasons
about cell shapes.

### Pattern player rewrite

`PatternPlayerCore::apply_step` dispatches on `StepEffect` instead
of inspecting `step.trigger`, `step.gate`, `step.cv1_end`,
`step.repeat`, and `step.absorbed_by_roll`. The state-reset
boilerplate (`slide_active = false`, `repeat_active = false`, etc.)
collapses to one place per effect kind. The inter-tick advance loop
(`tick`'s non-rising-edge branch) is unchanged in shape but now
asks a higher-level question per channel ("does this channel have an
open slide schedule? a repeat schedule?") rather than reading a flag
soup.

### Sub-event scheduling: respect per-tick swung durations

Today, E152's spread roll captures the anchor tick's duration once
and divides the span uniformly. When the span straddles a swing
boundary (anchor tick longer / shorter than the next), the schedule
ignores per-tick variation and the audible result drifts.

The unified row-build emits a per-channel schedule of *sub-events*:
each sub-event is a `(tick_index_in_span, fraction_within_tick)`
pair. For a `value*N` with `repeat_span = S`: N pairs at
`t_k = k/N · S`. For a slide over S ticks: one continuous ramp whose
sample-time placement is resolved tick-by-tick.

The pattern player consumes one sub-event per inter-tick advance
when the schedule says "this sub-event lands inside the current
tick at fraction f"; the sample offset is `f * current_tick_dur`
where `current_tick_dur` is the actually-swung duration of the
*current* tick (not the anchor tick). This resolves the E152 v1
swing-within-span limitation as a side effect of the rewrite.

### Token ambiguity

The grammar change `~` → `_` removes the cursor-classification
clash with `~tap` / `~name`. The LSP's `tree_nav::classify_step_node`
helper updates to recognise `_` tokens; the existing cable-endpoint
classifiers continue to recognise `~`.

## Consequences

### Authoring

- Bare `value` is always a fresh trigger — no row context needed to
  read a single cell.
- `/value` is a new, locally-readable form for "change pitch without
  retrigger." The musical idea that was previously inexpressible has
  syntax.
- Slides over N ticks are written cell-by-cell without invoking the
  `slide(...)` macro. The macro is retained as shorthand; in-row
  notation is the new primary form.
- Tie token visual ambiguity with `~tap` is removed.

### Runtime

- `apply_step`'s flag cascade collapses to a dispatch on
  `StepEffect`.
- Per-channel sub-event schedules replace the single `interval`
  capture used in E152. The change is local to the pattern player;
  the cable layout, descriptor, and tracker data types are
  unchanged.
- Swing-within-span behaviour now respects per-tick durations.
  Existing audio goldens for E152 `value*N _` spread patterns will
  shift by sub-sample amounts on swung tempos; non-swung patterns
  stay bit-identical. Goldens for unswung patterns must remain
  bit-identical; swung goldens regenerate.

### Compatibility

- The `~` token is **removed** from the step grammar. Patches that
  use `~` in step rows stop parsing. Migration: mechanical
  substitution `~` → `_` in pattern bodies. The patches-fmt tool
  (or a one-shot migration script) handles this.
- The `slide(n, start, end)` macro continues to work and lowers to
  the new in-row cell shapes.
- `value*N _` (E152) continues to work; the row-build annotation
  pass is generalised but the absorption behaviour for `*N` is
  preserved exactly.

### Implementation cost

- Grammar: ~5 new productions in pest + tree-sitter, mirroring
  shapes.
- Row-build: pass is rewritten as a channel-stateful walk emitting
  `StepEffect`s. ~200 LOC plus tests.
- Pattern player: `apply_step` and `advance_roll_one_sample`
  rewritten against `StepEffect`. Drop `cv1_end`, `cv2_end`,
  `repeat_span`, `absorbed_by_roll` from `TrackerStep` in favour of
  carrying the resolved effect.
- LSP: hover updates to recognise the six cell forms; slide-open
  overlay across the row optional polish.
- Manual: the tracker section rewrites against the unified model.
- Goldens: extensive integration coverage for the new combinations.

### Trade-offs declined

- **Always-explicit slide continuation `>_`.** Requiring `>_` on
  every flow tick (rejecting bare `_` absorption into slides) gives
  full cell-by-cell local readability — no channel-state lookup at
  all. Declined because it breaks E152's `*N _ _` and because long
  slides become verbose. The unified close rule keeps bare `value`
  locally readable; the *lead-in* shape requires reading left, but
  that's intrinsic to "this cell continues something" anywhere in
  the system.
- **Doubled-arrow `>>` for slide span.** Earlier proposal: `>>` to
  spread the slide. Declined because the per-tick `>` / `_` / `>_`
  / `/` system gives finer control (per-tick on/off of "is this a
  slide tick?") for the same author cost on the common case.
- **Keep `~` for sustain, add separate `_` for "change cv".** Two
  continuation tokens — declined. The single-token rule (`_`
  flows through whatever the prior modifier said) is cleaner and
  removes the visual ambiguity of `~` with cable-endpoint
  decoration.

## References

- E152 closed epic: tracker tie-span repeats, ADR 0072 fusion
  context, ticket 0939's `annotate_repeat_spans`.
- ADR 0042: tracker scope vs DSP (this ADR sits squarely in the
  tracker-state layer).
- ADR 0047: sub-sample sync events (orthogonal; ticks here are
  ADR 0030 sample-accurate gate boundaries).
- ADR 0072: cycle-free subgraph fusion (PatternPlayer is one node
  in the fused graph; this ADR does not change the fusion shape).
