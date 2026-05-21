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

| Cell shape         | Trigger? | Slide on this tick?                    | cv1 effect at start of tick | Cv1 effect at end of tick     |
| ------------------ | -------- | -------------------------------------- | --------------------------- | ----------------------------- |
| `value`            | yes      | depends on prior                       | snap to `value`             | unchanged                     |
| `_`                | no       | depends on prior                       | unchanged                   | depends on slide-open state   |
| `/value`           | no       | no (closes any open slide at boundary) | snap to `value`             | unchanged                     |
| `value>`           | yes      | opens                                  | snap to `value`             | (ramping; resolved at close)  |
| `>_`               | no       | yes (opens if not open)                | unchanged                   | (ramping; resolved at close)  |
| `>value`           | no       | yes (closes within tick)               | unchanged                   | `value`                       |
| `value>value`      | yes      | yes (one-tick slide)                   | snap to first value         | second value                  |
| `/value>cv1_end`   | no       | yes (one-tick slide)                   | snap to `value`             | `cv1_end`                     |

The `value>value` form is kept as sugar for the common single-tick
slide-into-hold case; it is exactly equivalent to `value> /value`
under the unified semantics. The `/value>cv1_end` shape is the
no-retrigger counterpart of `value>value`: snap cv1 without trigger
and ramp to `cv1_end` within the same tick. It is equivalent to
`/value> />cv1_end` if such a no-retrig open form existed; lacking
that, the row-build pass resolves the single-cell sugar to a
dedicated `StepCvSlide` effect.

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
    // /value>cv1_end: snap + in-tick ramp, no trigger
    StepCvSlide {
        cv1: f32,
        cv1_end: f32,
        cv2: Option<f32>,
    },
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

## Amendment 2026-05-20 — abolish `slide()`; cv2 on multi-cell slides

Drop the `slide(n, A, B)` macro generator and extend the multi-cell
slide shapes (`value>`, `/value`, `>value`) with an optional `:cv2`
modifier so velocity / volume can ramp across a slide alongside cv1.

### Context (amendment)

Ticket 0946 retained two shapes only as sugar:

1. The `slide(n, A, B)` macro generator, lowered at expand time to
   `A>` + (n−2)·`>_` + `>B` cells (n ≥ 2) or to `A>B` (n = 1).
2. The `value>value` one-cell cv1 slide (`StepKind::SlideSugar`).

Both forms predate the unified row-build pass and only ramp **cv1**.
A user writing a velocity slide alongside a pitch slide
(`C4:0.5> _ >C4:1.0`) gets a parse error: the multi-cell `value>`
shape carries no cv2 endpoint, and the cv2 sugar (`:cv2>cv2_end`)
only attaches to single-cell `step_valued` cells.

### Decision (amendment)

Drop the `slide()` macro. Keep the `value>value` cell sugar — it
packs an open+close into one tick and has no drop-in unsugared
replacement (see "Trade-offs declined"). Extend the three multi-cell
slide shapes to accept `:cv2` so cv2 can ramp through them.

#### Surface grammar (amendment)

- **Remove** `slide(n, A, B)` generator. Authors write the cells
  inline. The migration is mechanical:
  - `slide(1, A, B)` → `A>B` (the existing one-cell sugar; same
    semantics: ramp A→B over tick 1, land at B at the boundary).
  - `slide(2, A, B)` → `A> >B` (close-in-tick; ramp covers both
    ticks landing at B inside tick 2).
  - `slide(n, A, B)` for n ≥ 3 → `A>` + (n−2)·`>_` + `>B`.

- **Keep** the `value>value` cell form. The sugar is the only shape
  that puts a triggered open + close in a single tick, which is
  semantically distinct from `value> /value` (latter takes two
  cells and holds the close value through tick 2).

- **Extend** the three multi-cell slide shapes to accept an optional
  `:cv2` tail. Updated forms:

| Cell                | New shape      | Effect                                                                                                                              |
| ------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| slide open          | `value[:cv2]>` | trigger; cv1 ← value; cv2 ← `:cv2` (or 0); records open cv2 as ramp start                                                           |
| step-to             | `/value[:cv2]` | no trigger; cv1 ← value at boundary; cv2 ← `:cv2` at boundary (or unchanged); closes any open slide at boundary                     |
| slide close in tick | `>value[:cv2]` | no trigger; cv1 ramps to value across this tick; cv2 ramps to `:cv2` across this tick (or holds open's cv2); closes inside the tick |

  `>_` (TieFlow) carries no values — it inherits whatever the open
  cell set. The existing `value>value` sugar already supports cv2
  (`A:0.5>B:0.8`) via `step_cv2`'s nested `step_slide_target`; no
  change there.

#### Row-build resolution (amendment)

`StepKind` keeps `SlideSugar` and adds optional `cv2` to `StepTo`
and `SlideCloseInTick`:

```rust
pub enum StepKind {
    Rest,
    Tie,
    Note { repeat: u8 },
    SlideSugar { cv1_end: f32, cv2_end: Option<f32> },  // unchanged
    SlideOpen,                                          // cv1, cv2 from Step.cv1/cv2
    StepTo { cv2: Option<f32> },                        // unchanged shape
    // /value>cv1_end[:cv2] (no-retrig counterpart to SlideSugar)
    StepToSlide { cv1_end: f32, cv2: Option<f32> },
    TieFlow,
    SlideCloseInTick { cv2: Option<f32> },              // gains cv2
}
```

`resolve_step_effects` patches both `close_cv1` and `close_cv2` on
the open `SlideOpen` struct when the close cell arrives. If the
close cell's `:cv2` is `None`, `close_cv2` stays at `None` and the
runtime falls back to the open's cv2 (constant cv2 through the
slide). If the open had no `:cv2` modifier either, `Step.cv2` is
`0.0` and that's the ramp.

#### Pattern player (amendment)

`apply_step` already reads `SlideOpen.close_cv2` (its `Option<f32>`
lets the runtime fall back to the open's cv2 when `None`). The
`SlideCloseInTick` arm needs a `cv2` read alongside the existing
`cv1` to drive the cv2 ramp inside the close tick.

#### LSP / hover (amendment)

- `decode_step` drops the `slide_generator` branch; adds parsing
  for the new `:cv2` tail on the three slide cells.
- Hover text on a slide cell can mention the cv2 endpoint when one
  is set (polish, not required).

### Consequences (amendment)

#### Authoring (amendment)

- Velocity / volume slides become first-class: `C4:0.5> _ >C4:1.0`
  ramps both pitch and velocity across three ticks.
- `slide(n, …)` is removed. The in-row form is the only way to
  spread a slide across many ticks; the `value>value` sugar
  remains for the common one-tick case.

#### Runtime (amendment)

- `SlideOpen.close_cv2` was already `Option<f32>`; only the row-
  build pass needs to populate it from the close cell's `:cv2`,
  and the `SlideCloseInTick` runtime arm needs to read the
  optional cv2 endpoint.
- No audio-thread changes that would affect existing patterns.
  No golden regeneration required; cv2-sliding patterns are new,
  no prior golden exists.

#### Compatibility (amendment)

- Every in-tree `slide(n, …)` invocation must be rewritten by hand
  to the equivalent in-row form. Survey shows: 4 sites (1 fixture,
  3 inline test strings).
- `value>value` sugar is unchanged.

#### Trade-offs declined (amendment)

- **Abolish `value>value` sugar too (full uniformity).** Declined.
  The sugar's "open+close in one tick" cannot be reproduced
  unsugared without consuming an extra cell (`A> /B` is two cells;
  `A>B` is one), so every existing use would shift its row's
  subsequent cells by +1 tick. The grammar irregularity is the
  lesser cost vs. forcing content reshuffles in every patch using
  one-tick slides.
- **Keep `slide()` for very long ramps.** Declined — `A>` +
  many `>_` + `>B` is verbose but parses identically to other in-
  row content and lets the author tap intermediate ticks. A user
  who really wants the one-liner can define a template that emits
  the expanded cells.
- **Add `:cv2` to `>_` (TieFlow).** Declined — `>_` is the "flow
  through, no new info" cell. Adding cv2 there means two ways to
  set the same value (cv2 on the open + cv2 on the flow), with
  unclear precedence. The close cell carries the close cv2; that's
  enough.
- **Allow cv1-less close (`/:cv2`, `>:cv2`) for cv2-only slides.**
  Declined — the close cell's cv1 always closes the cv1 slide. To
  hold cv1 steady through the slide, write `cv1> _ >cv1:cv2_end`
  with the same cv1 on both endpoints. Redundant but explicit.

## References

- E152 closed epic: tracker tie-span repeats, ADR 0072 fusion
  context, ticket 0939's `annotate_repeat_spans`.
- ADR 0042: tracker scope vs DSP (this ADR sits squarely in the
  tracker-state layer).
- ADR 0047: sub-sample sync events (orthogonal; ticks here are
  ADR 0030 sample-accurate gate boundaries).
- ADR 0072: cycle-free subgraph fusion (PatternPlayer is one node
  in the fused graph; this ADR does not change the fusion shape).
