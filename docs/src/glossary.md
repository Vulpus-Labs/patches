# Glossary

Terms used across the manual, with one or two sentences each.

**Audio.** Signal kind for sound. Float samples in roughly
`[-1, +1]` produced at the host's sample rate.

**Cable.** A directed connection from one module's output port to
one module's input port. Carries one signal stream of one kind.

**CLAP plugin.** The plugin packaging that lets a DAW host load
Patches as an instrument or audio effect. Built from `patches-clap`,
shipped as a single `.dylib` / `.so` / `.dll` bundle. Distinct from
*native plugin* — see below.

**Control voltage (CV).** Signal kind for modulation and pitch.
Pitch follows the **V/oct** convention: one unit per octave.

**CV.** See *Control voltage*.

**DSL.** The text format Patches files are written in
(`.patches`). Specified in `patches-dsl/src/grammar.pest`; described
in [DSL basics](dsl-basics.md) and the
[DSL reference](dsl-reference.md).

**Edit-reload cycle.** The development loop where saving a
`.patches` file triggers an in-place engine rebuild while audio
keeps playing. Covered in
[The edit-reload cycle](authoring-edit-reload.md).

**Engine.** The real-time audio component. Owns the execution plan,
the module instances, and the cable pool. Sources: `patches-core`,
`patches-engine`.

**Execution plan.** The flat, ordered list of module slots and
buffer assignments the audio thread runs. Built by the planner from
a `ModuleGraph`.

**Fan-in.** Multiple cables converging on one input. Not directly
allowed — each input takes one cable. Combine signals through a
mixer module.

**Fan-out.** One output driving multiple inputs. Free: draw as many
cables from an output as needed, no splitter required.

**FlatPatch.** The intermediate representation produced by the DSL
expander after template inlining and parameter substitution. A flat
list of module declarations and connections — the input the
interpreter validates.

**Fusion.** Planner optimisation that routes cross-SCC cables in
acyclic (feedback-free) regions through the *scratch* pool region so
consumers read producers' *current-tick* output instead of going
through the one-sample cable delay. Cycle-bearing cables stay in the
*cycle* region and keep the one-sample delay (which makes feedback
well-defined). Removes lag from long signal chains while preserving
feedback-loop semantics.

**Gate.** Signal kind held `1.0` while a note is active, `0.0`
when released. Drives an envelope's hold-and-release behaviour.

**Global config.** Host-scoped persisted settings (currently:
bundle directory list). Stored in a cross-platform `settings.toml`.
Distinct from per-patch sidecar state.

**Hot-reload.** Same as *edit-reload* — the engine swaps in a new
graph without stopping audio.

**LSP.** Language server (`patches-lsp`) backing the VS Code
extension's diagnostics, hover, completions, and SVG rendering. See
[LSP architecture](internals-lsp.md).

**Mono.** A cable that carries one sample per audio tick.

**Module.** A unit of computation with a fixed set of named input
and output ports. Examples: `Osc`, `PolyAdsr`, `StereoMixer`,
`AudioOut`. Implementations live in `patches-modules` or in
externally-built native plugin bundles.

**Module graph.** The validated graph of module instances and
typed cables produced by the interpreter. Input to the planner.

**Module type.** The class of a module instance. Defines the
instance's ports and parameters. An instance has a *name* (chosen
in the patch) and a *type* (chosen from the registry):
`module osc : Osc` declares an instance named `osc` of type `Osc`.

**Native plugin.** A module bundle built as a separate `cdylib` and
loaded at host startup via the [Native plugin ABI](extending-abi.md).
Distinct from *CLAP plugin*: native plugins add module types to
Patches; the CLAP plugin embeds Patches into a DAW.

**Patch.** A complete `.patches` file: optional templates, songs,
patterns, host controls, then exactly one `patch { ... }` block
declaring modules and their connections.

**Per-patch sidecar.** Persisted state attached to a specific
patch file. In the player, written next to the `.patches` file as
`<name>.patches.state` (or under `$XDG_STATE_HOME`); in the CLAP
plugin, written via the host's CLAP-state mechanism. Patch knobs,
monitor view toggles. Distinct from *global config*.

**Plan.** Shorthand for *execution plan*.

**Planner.** The component that turns a `ModuleGraph` into an
`ExecutionPlan`, reusing module instances across reloads by
name/type match. Lives in `patches-planner`.

**Poly.** A cable that carries one sample per voice per audio
tick. The voice pool is 16 wide in every shipping host.

**Polyphony.** The ability to play multiple notes simultaneously,
each on its own voice slot. Driven by `PolyMidiToCv` (or a similar
poly source) into a poly module chain.

**Port.** A named input or output on a module. Each port has a
*kind* (mono / poly / stereo) and, for mono and poly, a *layout*
(audio / trigger / transport / midi) that connection-time
validation checks.

**REPL.** The interactive loop the edit-reload cycle creates for
patch authoring: save, listen, save again. Patches' development
workflow is REPL-shaped, not compile-run-listen-shaped.

**SCC.** Strongly connected component of the module graph —
maximal subgraph in which every module reaches every other module.
Cycle-bearing SCCs retain the one-sample cable delay; acyclic SCCs
are fused.

**Sidecar.** See *per-patch sidecar*.

**Signal kind.** The type of signal a port carries: audio, CV,
gate, trigger, or stereo. The planner uses kinds to validate
connections at compile time. See
[Signal kinds and cable rules](dsl-signal-kinds.md).

**Stereo.** Signal kind carrying `(L, R)` audio samples on one
cable. A symmetric stereo module uses a single stereo port rather than paired `_left`/`_right` mono ports.

**Template.** A named, parameterised subgraph in a `.patches` file.
Expanded at compile time into concrete module instances. Templates
are a DSL abstraction, not a runtime construct. See
[Templates and abstraction](dsl-templates.md).

**Trigger.** Signal kind: sub-sample-accurate event marker. On each sample a trigger cable carries `0.0` for "no
event" or a value in `(0.0, 1.0]` giving the event's *fractional
position* within the sample (used by oscillator hard-sync, BLEP
correction, etc.). Tells an envelope when to restart from its attack
phase, regardless of whether the gate was already high.

**V/oct.** The pitch encoding convention: one unit = one octave;
one semitone = 1/12 unit. Used by oscillator `voct` inputs and by
the V/oct CV emitted by MIDI-to-CV modules.

**Voice.** One slot of a poly cable. A `PolyAdsr` or `PolyOsc`
runs 16 voices in parallel; voice allocation is handled by
`PolyMidiToCv` (or a sequencer with multiple voice outputs).

**Voice allocation.** Mapping incoming MIDI notes onto voice slots.
`PolyMidiToCv` uses LIFO stealing: when all voices are busy, the
most-recently-allocated voice is reused for the next note.

**Webview GUI.** The CLAP plugin's UI, an embedded `wry` webview
rendering a small JS app. The engine and controller live in Rust;
the webview is a thin shell over the controller's `GuiSnapshot` /
`Intent` types.
