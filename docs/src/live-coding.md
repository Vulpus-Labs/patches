# Live coding and hot-reload

The player watches your `.patches` file for changes. When you save, it parses the new file, builds a new execution plan, and hands it to the audio thread — all without stopping the audio stream. This chapter explains what happens during that handoff, what state is preserved, and how to think about it while performing.

## The reload cycle

1. You edit the `.patches` file and save.
2. The file watcher detects the change and hands the new source to the DSL parser.
3. The parser produces a flat list of module declarations and connections.
4. The interpreter validates this against the module registry — checking that module types exist, ports are real, parameter types and ranges are correct, and cable kinds (mono/poly) match.
5. If validation fails, the error is printed to stderr and the running patch continues unchanged.
6. If validation succeeds, the planner builds a new execution plan.
7. The plan is sent to the audio thread via a lock-free ring buffer. The audio thread picks it up at the start of its next processing block and swaps it in.

The audio stream never stops. There is no gap, no fade-out-and-fade-in. The new plan takes effect between two consecutive samples.

## What survives

The planner matches modules in the new patch against those in the running patch by **name and type**. A module named `osc` of type `PolyOsc` in the old patch will be matched to a module named `osc` of type `PolyOsc` in the new patch. When a match is found, the running module instance is carried forward — its internal state is untouched.

This means:

- **Oscillator phase** carries over. A 440 Hz oscillator that has been running for ten seconds will continue from its current phase position, not restart from zero.
- **Filter state** carries over. The filter's internal delay buffers (which determine its current resonant behaviour) are preserved.
- **Envelope position** carries over. An envelope in its sustain phase stays in sustain.
- **Delay buffers** carry over. A delay line full of audio retains its contents.

The practical effect is that parameter changes sound smooth. Changing a filter cutoff mid-note produces a continuous sweep, not an abrupt reset.

## What resets

A module is **not** matched — and is freshly instantiated — when:

- It is new in the patch (did not exist before).
- Its type changed (e.g. you replaced `Osc` with `PolyOsc`). The names match but the types do not, so the old instance cannot be reused.
- Its name changed (e.g. you renamed `osc` to `oscillator`). From the planner's perspective this is a different module.

Freshly instantiated modules start from their initial state: oscillator phase at zero, envelopes idle, delay buffers empty.

Modules that existed in the old patch but are absent from the new patch are removed. Their memory is reclaimed on a background thread so the audio thread does not have to deallocate anything.

## Parameter updates

When a surviving module's parameters change (e.g. you edited `cutoff: 600Hz` to `cutoff: 800Hz`), the new values are applied to the existing instance. The module is not reinstantiated — only its parameter fields are updated. Most modules respond to parameter changes immediately on the next sample. Some (like filters) recompute their coefficients periodically, so a parameter change may take effect over a few samples rather than instantaneously, which actually sounds more natural.

## Connectivity changes

When you add, remove, or reroute cables, the affected modules are notified of their new port assignments. Modules that care about connectivity (e.g. an oscillator that skips computing an output waveform if nothing is connected to it) can adapt accordingly. The cable pool itself is stable — unchanged cables keep their buffer slots, so signals on cables that were not rewired continue without interruption.

## Error recovery

If the new patch file has a syntax error or fails validation, the running patch is unaffected. The error is printed to stderr with file position information (line and column). You can fix the error and save again; the next successful parse will trigger a reload as normal.

This means you can make speculative edits freely. A half-finished connection or a misspelled module type will not crash the running audio — it will just fail to reload, and you can correct it.

## Thinking about identity

The matching rule — same name, same type — is the key mental model. If you want a module to survive a reload, keep its name and type the same. If you want it to reset, change one of them. A common pattern is renaming a module temporarily to force a reset (clearing a delay buffer, for example), then renaming it back.

Structural changes — adding new modules, removing old ones, rewiring — are all fine and happen smoothly. The only thing the planner cannot carry across is a change in a module's fundamental identity.
