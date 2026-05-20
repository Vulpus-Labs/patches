# DSL reference

This is the complete syntax reference for `.patches` files. It
assumes familiarity with the concepts in
[Mental model](mental-model.md) and the narrative introductions in
[DSL basics](dsl-basics.md),
[Signal kinds and cable rules](dsl-signal-kinds.md),
[Templates and abstraction](dsl-templates.md), and
[Poly and voice allocation](dsl-poly.md). This page is the lookup;
those are the on-ramp.

## File structure

A patch file contains zero or more top-level blocks followed by exactly one `patch` block. Comments run from `#` to end of line. Whitespace is insignificant.

```patches
# optional: include another .patches file (templates / patterns / sections)
include "shared/voices.patches"

# optional: pattern definitions for the tracker sequencer
pattern kick_basic {
    hit: x:1.0 . . . x:1.0 . . .
}

# optional: song definitions referencing patterns
song my_song(kick, bass) {
    play {
        kick_basic, bass_verse
    }
}

# optional: top-level reusable section (visible to all songs)
section intro {
    kick_basic, bass_verse
}

# optional: templates
template voice(...) {
    ...
}

# required: exactly one patch block
patch {
    ...
}
```

### `include` directive

`include "path"` inlines another `.patches` file at the point of the
directive. Includes are resolved at parse time, relative to the including
file. The included file may contribute templates, patterns, songs, and
top-level sections — but not a `patch` block (only the root file declares
one).

## Module declarations

```
module <name> : <Type>
module <name> : <Type> { <params> }
module <name> : <Type>(<shape-args>) { <params> }
stereo module <name> : <Type>(<shape-args>) { <params> }
```

- **name** — an identifier (`[a-zA-Z_][a-zA-Z0-9_-]*`). Used for connections and for identity matching across hot-reloads.
- **Type** — the module type name as registered in the module registry (e.g. `Osc`, `PolyAdsr`, `Sum`).
- **shape-args** — key-value pairs that control port arity. Written in parentheses before the parameter braces: `Sum(channels: 3)`.
- **params** — key-value pairs configuring the module's parameters.
- **`stereo`** — prefix keyword requesting two parallel
  instances of the module sharing connections and parameters. Per-side
  overrides use `@l:` / `@r:` at-blocks; per-side ports are addressed via
  `port[l]` / `port[r]`.

### Parameter syntax

Parameters are comma-separated `key: value` pairs:

```patches
module env : Adsr { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
```

Indexed parameters use bracket notation:

```patches
module mix : StereoMixer(channels: 4) { level[0]: 0.8, pan[0]: -0.5 }
```

At-block syntax groups parameters by index:

```patches
module dly : Delay(channels: 2) {
    @0: { delay_ms: 250, feedback: 0.4 },
    @1: { delay_ms: 375, feedback: 0.3 }
}
```

When the module shape defines named aliases, those can be used instead of numeric indices:

```patches
module mix : Mixer(channels: [drums, bass, lead]) {
    @drums { level: 0.8 },
    @bass  { level: 0.6 }
}
```

On a `stereo` module, the special `@l:` / `@r:` blocks override per side:

```patches
stereo module dly : Delay(channels: 2) {
    @0: { delay_ms: 250 },
    @l: { feedback: 0.45 },   # left-side-only override
    @r: { feedback: 0.55 }    # right-side-only override
}
```

### Parameter value types

| Syntax              | Type      | Notes                                                |
| ------------------- | --------- | ---------------------------------------------------- |
| `440.0`             | float     | bare decimal                                         |
| `2`                 | int       | bare integer                                         |
| `440Hz`             | frequency | converted to V/oct; also `2.5kHz`                    |
| `-6dB`              | amplitude | converted to linear (0 dB = 1.0, -6 dB ≈ 0.5)        |
| `7s`                | interval  | semitones, converted to V/oct (`s` = semitone)       |
| `50c`               | interval  | cents, converted to V/oct (`c` = cent)               |
| `C4`, `A#3`, `Bb2`  | note name | converted to V/oct                                   |
| `true` / `false`    | boolean   |                                                      |
| `linear`            | string    | unquoted; `"linear"` also works                      |
| `"hello world"`     | string    | quotes required if value contains spaces             |
| `file("ir.wav")`    | file path | resolves to a WAV file path (used by convolution)    |

Frequency, note, and dB literals are case-insensitive. There is no
time-unit suffix; durations are bare floats in seconds. `s` after a number
means **semitones**, not seconds.

## Connections

### Forward connection

```patches
osc.sine -> out.in
```

`out` here is `AudioOut`, whose `in` is a stereo port. The mono `osc.sine`
broadcasts onto both channels automatically (see *Cable kinds* below).

### Scaled forward connection

```patches
osc.sine -[0.5]-> out.in
```

The scale factor is a float multiplied onto the signal at the cable level.

### Range-mapped connections

A scalar `-[k]->` only multiplies the signal. To map a known source
range affinely onto a destination range, use a **range expression**
on the arrow:

```patches
ctrl.knob -[uni(20Hz, 8kHz)]-> filter.cutoff
lfo.out   -[bi(C2, C5)]->      osc.voct
```

Both forms hard-clip at the destination endpoints; out-of-source-range
inputs saturate.

#### `uni(lo, hi)` — `[0, 1]` → `[lo, hi]`

For knob and host-control sources delivering a normalized value:

```patches
patch {
    knob   cutoff { displayName: "Cutoff" }
    module filt : Svf

    ~cutoff -[uni(20Hz, 8kHz)]-> filt.cutoff
}
```

#### `bi(lo, hi)` — `[-1, 1]` → `[lo, hi]`

For bipolar modulation sources (LFOs, audio-rate modulators):

```patches
patch {
    module lfo : Lfo
    module osc : Osc

    # vibrato around C4, ±1 octave; note literals lower to v/oct.
    lfo.sine -[bi(C3, C5)]-> osc.voct
}
```

Note literals and `Hz` / `kHz` literals mix freely inside a single
range — both belong to the **pitch family** and lower to v/oct over
`C0`. Other unit families (`s`, `dB`) stay in their native unit.
Cross-family pairs are rejected at expansion time:

```patches
# error: cable range endpoints have incompatible unit families:
#        pitch vs level
osc.sine -[bi(440Hz, -12dB)]-> dst.in
```

Endpoints accept the same forms as scalar scales: numeric and
unit-suffixed literals, note literals, and `<param>` references. A
template can therefore parameterise the destination range:

```patches
template knob_to_cutoff(lo: float, hi: float) {
    in: knob
    out: cutoff
    $.knob -[uni(<lo>, <hi>)]-> $.cutoff
}
```

Range and scalar segments compose freely across template boundaries
(see *Scale composition* below); the result is a single affine plus
clip applied once at the destination port.

### Backward connection

```patches
$.audio <- vca.out
$.audio <-[0.5]- vca.out
```

`<-` connects from right to left. Primarily useful inside templates for wiring boundary ports. The scaled form uses `<-[scale]-`.

### Indexed ports

The index in brackets selects which slot of a multi-port to connect.
Aliases declared on the module's shape are the idiomatic form:

```patches
module mix : Mixer([drums, bass, lead])

kick.out -> mix.in[drums]
bass.out -> mix.in[bass]
lead.out -> mix.in[lead]
```

Numeric indices work when the shape is declared as a bare count:

```patches
module sum : Sum(2)

osc_a.sine -> sum.in[0]
osc_b.sine -> sum.in[1]
```

On `stereo` modules, the channel selectors `[l]` and `[r]` address the
per-side instance instead of a numeric index:

```patches
stereo module dly : Delay
src.left  -> dly.in[l]
src.right -> dly.in[r]
```

### Fan-in and fan-out

A comma-separated list of endpoints on either side of an arrow connects
multiple sources or destinations in one statement:

```patches
osc.sine -> filter.in, scope.in            # fan-out: one source → many sinks
voice_a.out, voice_b.out -> mix.in         # fan-in: many sources → one sink
```

Fan-out drives every listed sink from the same source. Fan-in to a single
input port of uniform cable kind is **auto-summed**: the expander
synthesises a `Sum` (or `PolySum` / `StereoSum`) module under the hood.
Mixed-kind fan-in is rejected at expansion time.

### Arity expansion

```patches
$.audio[*size] -> sum.in[*size]
```

The `[*name]` wildcard expands into one connection per index from 0 to `name - 1`. Only valid inside templates where `name` is an int parameter.

### Rules

- An input port may receive multiple connections of uniform cable kind;
  the expander synthesises a `Sum` / `PolySum` / `StereoSum` to combine
  them (see *Fan-in and fan-out*). Mixed-kind fan-in is rejected.
- An output port can drive any number of inputs.
- Cable kinds must match: mono ↔ mono, poly ↔ poly, stereo ↔ stereo.
  Use `MonoToPoly` / `PolyToMono` to bridge mono and poly.

### Cable kinds (mono / poly / stereo)

- **Mono** carries one `f32` per tick. The default for control and audio cables.
- **Poly** carries 16 voices (`[f32; 16]`). Used by `Poly*` modules.
- **Stereo** carries an `(L, R)` pair. Stereo modules expose a single
  port — `stereo_delay.in`, `lim.out` — instead of paired `*_left` /
  `*_right` ports.

A mono Audio source feeding a stereo input is silently broadcast
(`L = R = source`); no extra wiring is needed for the common
"mono-into-effect" case. Stereo→mono is rejected — when the user
genuinely wants the two halves apart they go through `StereoSplitter`,
and `StereoJoiner` assembles two monos back into one stereo cable.

## Host controls

A **host control** is a named source surfaced to the plugin host
(CLAP parameter, knob row in the CLI). Four kinds, declared at the
top of `patch { ... }`:

```patches
patch {
    knob    cutoff   { displayName: "Cutoff" }
    slider  mix      { default: 0.5 }
    toggle  bypass   { default: false }
    trigger fire     { }
}
```

- `knob` / `slider` produce a smoothed audio-rate signal in
  `[-1, 1]`.
- `toggle` produces a stepped 0 / 1 signal; `default` is required.
- `trigger` produces one-sample pulses fired by the host; no
  required fields.

### Fields

| Field          | Kinds                  | Optional           | Notes                                                  |
| -------------- | ---------------------- | ------------------ | ------------------------------------------------------ |
| `displayName`  | all                    | yes                | Surface label for the host UI.                         |
| `unit`         | knob / slider          | yes                | Hint for value formatting.                             |
| `default`      | knob / slider / toggle | knob / slider only | Toggle requires `default`; others optional.            |
| `low` / `high` | knob / slider          | yes                | Display-only metadata. Runtime mapping is the cable    |
|                |                        |                    | range operator's job (see *Range-mapped connections*). |
| `taper`        | knob / slider          | yes                | UI taper hint (`linear`, `exp`).                       |

### References

A `~name` reference on a cable endpoint connects the host control to
a port:

```patches
patch {
    knob cutoff { }
    module filt : Svf
    ~cutoff -[uni(20Hz, 8kHz)]-> filt.cutoff
}
```

The cable carries the normalized `[-1, 1]` value; the range operator
remaps it to the destination's musical units.

### Implicit knob

If a `~name` cable endpoint has no matching declaration, the
desugarer synthesises an empty `knob name {}` block:

```patches
patch {
    module filt : Svf
    ~cutoff -[uni(20Hz, 8kHz)]-> filt.cutoff   # implicit knob `cutoff`
}
```

Implicit and explicit empty-knob forms lower to the same patch. To
surface a `slider`, `toggle`, or `trigger` instead, declare it
explicitly — only `knob` is implicit.

## Templates

Templates define reusable sub-graphs that are expanded at compile time with no runtime cost.

### Definition

```patches
template voice(attack: float = 0.01, decay: float = 0.1, sustain: float = 0.7) {
    in:  voct, gate
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack>, decay: <decay>, sustain: <sustain>, release: 0.3 }
    module vca : Vca

    $.voct    -> osc.voct
    $.gate    -> env.gate
    osc.sine  -> vca.in
    env.out   -> vca.cv
    $.audio   <- vca.out
}
```

- **Parameters** are typed: `float`, `int`, `str`, `bool`, `pattern`,
  `song`. Parameters with `= default` are optional at the call site;
  those without are required.
- **Boundary ports** are declared with `in:` and `out:` lists. Inside the template body, `$.name` refers to a boundary port.
- **Parameter references** `<name>` substitute the parameter's value anywhere in the template body: in parameter values (`{ attack: <attack> }`), port names (`osc.<wave>`), and cable scales (`-[<gain>]->`).

### Instantiation

```patches
patch {
    module v1 : voice(attack: 0.005, sustain: 0.6)
    module v2 : voice    # all defaults

    v1.audio -> out.in
    v2.audio -> out.in
}
```

Each instantiation expands to a separate copy of the template's modules with mangled names to avoid collisions.

### Parameter types

| Type      | Accepts                                              |
| --------- | ---------------------------------------------------- |
| `float`   | numeric values (int-to-float coercion allowed)       |
| `int`     | integer values                                       |
| `str`     | string values                                        |
| `bool`    | `true` / `false`                                     |
| `pattern` | pattern names — passed to a `PatternPlayer` cell     |
| `song`    | song names — passed to a `MasterSequencer` cell      |

Type checking is strict. A `float` parameter rejects `true`; a `bool` rejects `0`.

### Arity parameters

A template parameter can control the number of ports:

```patches
template mixer(size: int, level[size]: float = 1.0) {
    in: audio[size]
    out: mixed

    module sum : Sum(channels: <size>)

    $.audio[*size] -> sum.in[*size]
    $.mixed        <- sum.out
}
```

The `[*size]` expansion generates one connection per index, scaling automatically with the arity.

### Scale composition

When a scaled connection crosses a template boundary, the scales are multiplied. A connection `-[0.5]->` into a template that internally has `-[0.3]->` produces an effective scale of 0.15 at the underlying cable.

### Nesting

Templates can instantiate other templates. A `filtered_voice` template can contain `module v : voice(...)` and add further processing. Expansion is recursive — the outermost template is fully flattened before the patch is built.

## Patterns

Pattern blocks define step data for the tracker sequencer. Each pattern has
one or more named channels, and each channel has a row of step values.

```patches
pattern bass_line {
    note: C2  .  Eb2 .  G1  .  Ab1 .
    vel:  1.0 .  0.8 .  0.9 .  0.7 .
}
```

- **Channel names** (`note`, `vel`, `hit`, etc.) are identifiers. They must
  match the `channels` list on the `PatternPlayer` that reads this pattern.
- All channels in a pattern must have the **same number of steps**.
- Steps are separated by whitespace.

### Step events

The step grammar is a unified taxonomy of cell shapes (ADR 0077). Each
cell of a channel row parses into one shape; the row-build pass walks
each channel left-to-right tracking `(slide_open, roll_active)` channel
state and emits a resolved effect per cell. The pattern player
dispatches on the resolved effect.

#### Cell taxonomy

A cell produces up to four signals: `cv1`, `cv2`, `trigger`, `gate`.

| Cell           | Trigger? | Lead-in effect on this tick    | Outgoing slide / roll state              |
| -------------- | -------- | ------------------------------ | ---------------------------------------- |
| `value`        | yes      | snap cv1 (and cv2) at start    | closes any open slide at the boundary    |
| `value:cv2`    | yes      | snap cv1 + cv2 at start        | closes any open slide at the boundary    |
| `value*N`      | yes      | snap cv1; fires N sub-triggers | opens a roll                             |
| `value>value`  | yes      | snap cv1 at start              | one-tick slide that lands at end value   |
| `value>`       | yes      | snap cv1 at start              | opens a multi-tick slide                 |
| `/value`       | no       | snap cv1 at the tick boundary  | closes any open slide at the boundary    |
| `>_`           | no       | continues / opens a slide      | extends or opens slide flow              |
| `>value`       | no       | ramps to value within the tick | closes the slide inside this tick        |
| `_`            | no       | depends on the channel's state | absorbed by roll / slide, else sustains  |
| `.`            | no       | rest — gate drops              | resets channel state                     |

`value` is any of: note name (`C4`, `A#3`), float (`0.5`), int (`2`),
unit literal (`440Hz`, `-6dB`). The optional `:cv2` tail attaches to
`value`, `value>`, `/value`, and `>value` (cv2 endpoints on close
cells ramp through the slide). Cv2 on `value>value` sugar is
`A:0.5>B:0.8`.

#### Unified close rule

A slide opened by `value>` or `>_` closes when the row-build pass
encounters a non-`_`, non-`>_`, non-`value>` cell:

- `value` close — slide lands at `value` at the tick boundary entering
  the cell; cell then fires a fresh trigger on its own tick.
- `/value` close — slide lands at `value` at the tick boundary; cell
  holds without retrigger.
- `>value` close — slide ramps to `value` *inside* this tick (no
  trigger); the close-tick is itself a slide-flow tick.

The lead-in shape (flat hold vs ramp) is determined by prior cells,
not by anything on the close cell. Bare `value` is therefore always
locally readable as a fresh trigger.

#### Continuation absorption

A `_` cell takes its meaning from whichever modifier was last open on
the channel:

- After a non-roll, non-slide anchor: sustain (gate held, cv unchanged).
- After a `value*N` anchor: absorbed into the roll's spread schedule
  (the N sub-triggers spread across the anchor tick + the absorbed-tie
  run). Span `S = 1 + consecutive_underscores`; sub-triggers fall at
  `k * S / N` of the span for `k = 0..N-1`; gate articulates at ~80%
  of each interval. Swing within the span is respected per-tick.
- After a `value>` or `>_`: absorbed into the slide's ramp.

`>_` is the explicit-flow form for slides that start on a non-value
cell ("hold this note for a bit, then start ramping").

#### Worked-example timelines

Each five-tick figure below shows trigger placement (`!`), gate
articulation (`-` = high, `.` = low), and cv shape across ticks.

```text
Pattern:  E4 . . . .
Tick:     1  2  3  4  5
Trigger:  !  .  .  .  .
Gate:     -- .  .  .  .
cv1:      E4 E4 E4 E4 E4   (held after gate drops; analog convention)

Pattern:  E4 /G4 . . .
Tick:     1   2  3  4  5
Trigger:  !   .  .  .  .   (only the E4 fires)
Gate:     --- -- .  .  .
cv1:      E4  G4 G4 G4 G4  (snap at the 1→2 boundary)

Pattern:  E4 _ /G4 . .
Tick:     1  2  3  4  5
Trigger:  !  .  .  .  .
Gate:     -- -- -- .  .
cv1:      E4 E4 G4 G4 G4  (snap at the 2→3 boundary)

Pattern:  E4> _ G4 . .
Tick:     1   2  3  4  5
Trigger:  !   .  !  .  .   (fresh G4 trigger at start of tick 3)
Gate:     --- -- -- .  .
cv1:      E4→ →G4 G4 G4 G4 (ramp through ticks 1+2; lands at boundary)

Pattern:  E4 _ >_ /G4 .
Tick:     1  2  3  4  5
Trigger:  !  .  .  .  .
Gate:     -- -- -- -- .
cv1:      E4 E4 E4→ G4 G4   (slide opens at tick 3 via >_; closes at
                              tick-3→tick-4 boundary)

Pattern:  E4> _ >G4 . .
Tick:     1   2  3   4  5
Trigger:  !   .  .   .  .
Gate:     --- -- --- .  .
cv1:      E4→ →  →G4 G4 G4  (continuous ramp across ticks 1-3; lands
                              at end of tick 3)

Pattern:  x*3 _ . . .
Tick:     1   2 3 4 5
Trigger:  !!!!! . . . .     (3 sub-triggers spread across ticks 1+2,
                              `*0/3`, `*2/3`, `*4/3` of T each)
Gate:     -..-..-.. . . .   (~80% duty per sub-interval)
cv1:      anchor cv1 held across the span
```

The `value>value` sugar is the common one-tick case: it packs an
open-and-close into a single cell. It is exactly equivalent in effect
to `value> /value` *except* that the unsugared form takes two ticks
(slide tick 1, hold tick 2). Use the sugar when the timing fits.

#### Row-continuation across `|`

Channel state and slide / roll spans flow through the `|`
row-continuation marker — the parser merges continuations into one
logical row before the row-build pass, so a span can cross the join:

```patches
hit: x*3
   | _ _
vox: A3>
   | _ /B3
```

#### Migration from `~`

The `~` tie token was retired in favour of `_` (ADR 0077). Old
patches using `~` in step rows do not parse; mechanically replace
`~` → `_` inside pattern bodies. The cable-endpoint forms (`~tap(…)`,
`~host_control`) are unaffected.

#### `slide(…)` macro retired

The `slide(n, A, B)` macro generator was removed in ticket 0948.
Authors write slides inline using the cell shapes above. Mechanical
equivalences:

- `slide(1, A, B)` → `A>B`
- `slide(2, A, B)` → `A> >B`
- `slide(n, A, B)` (n ≥ 3) → `A>` + (n−2)×`>_` + `>B`

#### Known limitations

- **Row-end truncation**: a slide or roll never extends past the end
  of a channel row. `x*5 _ _` at the end of a 3-step row still fires
  5 sub-triggers across those 3 ticks. A `value>` with no close cell
  in the row is degraded to a no-op (close target = open value); the
  row-build pass emits a diagnostic.
- **Cross-loop / cross-bank**: spans don't wrap past a pattern loop
  or bank boundary; trailing sub-events are dropped rather than
  wrapping.
- **Mid-span live edit**: if a `_` cell is edited to a non-tie
  mid-roll or mid-slide, the new content takes over on its own tick
  rise and the prior span is replaced (no overrun).

## Songs

A song block defines a playback order built from named `section` blocks and
`play` statements.

```patches
song my_song(bass, lead, drums) {
    section verse {
        bass_verse, lead_verse, drums_a
        bass_verse, lead_verse, drums_b
    }
    section chorus {
        bass_chor, lead_chor, drums_a
    }

    play verse
    @loop
    play (verse, chorus) * 2
}
```

### Lanes

The **lanes** — the identifiers in parentheses after the song name — declare
one column per pattern assignment. Every row must supply exactly this many
cells. Lane names must match the `channels` shape argument on the
`MasterSequencer`.

### Cells

A row is a comma-separated list of cells. A cell is one of:

- a pattern name — the pattern played in that lane for that row;
- `_` — silence on that lane;
- `<param>` — a template-parameter reference, resolved at expansion time.

Rows are separated by newlines (newlines are significant inside row
sequences). A parenthesised sub-sequence can be repeated: `(a, b
                                                               c, d) * 2`.
A bare `cell * N` (without parentheses) is rejected at parse time.

### Sections

A `section` block is a named, reusable row sequence. Sections may be defined
inside a song (song-local scope) or at file top level (visible to all songs).
A section has no lane count of its own; it is validated against the invoking
song's lanes when `play` references it.

### Play statements

`play` composes sections into the song's running row list:

- `play verse` — append `verse`.
- `play verse, chorus` — append `verse`, then `chorus`.
- `play chorus * 2` — append `chorus` twice (`*` binds tighter than `,`).
- `play (verse, chorus) * 4` — append the group four times.
- `play { row, ... }` — an anonymous inline body (no section name).
- `play chorus { row, ... }` — defines `chorus` as a song-local section and
  plays it once. Later `play chorus` / `chorus` references replay it.

Inline bodies (`play { ... }` and `play name { ... }`) appear only as the
entire body of a single `play` statement. They cannot be mixed into a
composition expression such as `play a, { row, ... }, b`.

### Loop marker

`@loop` is a standalone song item between `play` statements. It sets the
loop-back row index in the emitted song. A song may contain at most one
`@loop`; omitting it loops the song from the beginning.

### Inline patterns

`pattern` blocks may appear as song items alongside `section` and `play`.
Inline patterns are song-local: their names are mangled under the enclosing
song so two songs may each define a `fill` pattern without collision.
