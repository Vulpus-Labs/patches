# The edit-reload cycle

When you save a `.patches` file, the engine builds the new graph and
swaps it in without stopping the audio. Module instances whose name
and type are unchanged carry their state across the swap; the rest
are freshly instantiated.

This makes Patches' development loop closer to a REPL than to a
compile-run-listen workflow. Save → hear the difference → save
again. The audio never stops; small edits sound like parameter
tweaks. That's the *edit-reload cycle*.

This is a development feature, not a performance one. (Live coding
on top of it is possible — the cycle is robust enough — but the
design centres on iteration speed during patch authoring, not on
on-stage editing.)

## The reload cycle

1. You edit the `.patches` file and save.
2. The watcher detects the change and feeds the new source to the
   DSL parser.
3. The parser produces a `FlatPatch` — a flat list of module
   declarations and connections after template expansion.
4. The interpreter validates the FlatPatch against the module
   registry: module types exist, port names are real, parameter
   types are correct, cable kinds match.
5. **If validation fails**, diagnostics are surfaced (TUI log,
   plugin log pane) and the running patch continues unchanged.
6. **If validation succeeds**, the planner builds a new
   `ExecutionPlan`.
7. The plan is sent to the audio thread via a lock-free ring
   buffer. The audio thread picks it up at the start of its next
   processing block and swaps it in.

The audio stream never stops. There is no gap, no fade. The new
plan takes effect between two consecutive samples.

## What survives a reload

The planner matches modules in the new patch against those in the
running patch by **name and type**. A module named `osc` of type
`PolyOsc` in the old patch matches a module named `osc` of type
`PolyOsc` in the new patch — its instance is carried forward, its
internal state untouched.

State that survives includes:

- **Oscillator phase.** A 440 Hz oscillator running for ten
  seconds continues from its current phase, not from zero.
- **Filter state.** The filter's internal delay buffers (which
  determine its current resonant behaviour) are preserved.
- **Envelope position.** An envelope in its sustain phase stays
  in sustain.
- **Delay buffers.** A delay line full of audio retains its
  contents.
- **VCA and mixer state.** Per-channel summation accumulators.

The practical effect is that parameter edits sound smooth. Changing
a filter cutoff mid-note produces a continuous sweep, not an
abrupt reset.

## What resets

A module is *not* matched, and is freshly instantiated, when:

- **It is new** in the patch (didn't exist before).
- **Its type changed.** Replacing `Osc` with `PolyOsc` keeps the
  name but the type is different, so the old instance can't be
  reused.
- **Its name changed.** Renaming `osc` to `oscillator` looks like a
  different module to the planner.

Freshly instantiated modules start from initial state: oscillator
phase at zero, envelopes idle, delay buffers empty.

Modules that existed before and are absent from the new patch are
removed. Their memory is reclaimed on a background thread — the
audio thread doesn't deallocate.

## Parameter updates

When a surviving module's parameters change (`cutoff: 600Hz` →
`cutoff: 800Hz`), the new values are applied to the existing
instance. The module isn't reinstantiated — only its parameter
fields are updated.

Most modules respond on the next sample. Some (filters with
biquad coefficients, oversampling stages) recompute periodically,
so a change may take effect over a few samples rather than
instantaneously. This usually sounds more natural than a hard
parameter snap.

## Connectivity changes

When you add, remove, or reroute cables, the planner rewires the
buffer pool and notifies affected modules of their new port
assignments. Modules that care about connectivity (an oscillator
that skips computing an output waveform if nothing's connected) can
adapt.

The buffer pool itself is stable across reloads — unchanged cables
keep their buffer slots, so signals on un-touched cables continue
without interruption. Only the rewired cables see a fresh buffer.

## Error recovery

If the new file has a syntax error or fails validation, the running
patch is unaffected. Diagnostics name the file position (line,
column) and the rule that failed. Fix and save again — the next
successful parse triggers a reload.

This means you can save *speculatively* without fear. A
half-finished connection, a misspelled module type, an unclosed
brace — none of these crash the running audio. They just fail to
reload until you fix them.

## Thinking about identity

The matching rule — same name, same type — is the mental model.

- Want a module to *survive* a reload? Keep its name and type the
  same.
- Want it to *reset*? Change one of them.

A common pattern: rename a module temporarily to force a reset (a
delay line full of stale audio, for example, can be cleared by
renaming it, saving, then renaming back). Less brutally, change
its type to a no-op variant and back.

Structural changes — adding new modules, removing old ones,
rewiring — happen smoothly. The only thing the planner cannot
carry across is a change in a module's fundamental identity.

## Working flow

A productive iteration loop:

1. Start `patch_player your.patches` (or load the patch as a CLAP
   plugin).
2. Make a small edit — change one parameter, one connection, add
   one module.
3. Save.
4. Listen.
5. Repeat.

If you're tempted to make a big edit ("rewrite the filter section
entirely"), save halfway. The patch keeps running with the partial
edit — you'll either hear the half-rewrite sound interesting, or
the reload will fail with a diagnostic that tells you exactly what
to fix.

The cost of saving is small enough that "save to check" is a valid
debugging step. Use it.

## In the plugin

Inside a DAW, the CLAP plugin watches the file the same way the
player does. Edit the patch file in any editor, save, and the
plugin reloads in place. State survival rules are identical.

If you edit through the plugin's webview (no in-place editor yet —
this is via your external editor, watched), the cycle is the same.
The plugin's *Reload* button forces a recompile from the current
file contents (useful when the file watcher missed an event, or to
retry after fixing a previously-failing edit).

## What about live performance?

The cycle is robust enough to perform with — the audio doesn't drop
and structural edits stay smooth. People do live-code with Patches.

But the cycle is designed around iteration speed during authoring,
not around the constraints of a stage: there's no syntax-aware
editor mode, no commit/discard distinction, no "stage this change
for the next downbeat" mechanism. If those things matter for your
performance, you'll either work around them or ask for them as
features.

Authoring is the headline use; performing is a happy accident of
the cycle being fast and crash-safe.
