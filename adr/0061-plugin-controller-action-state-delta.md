# ADR 0061 — Plugin controller, actions, and state deltas

**Date:** 2026-04-29
**Status:** Proposed
**Related:**
[ADR 0044 — Dynamic module loading and reload](0044-dynamic-module-loading-reload.md),
[ADR 0051 — Module panic halt policy](0051-module-panic-halt-policy.md)

## Context

`patches-clap` (and to a lesser extent any future plugin host shell) keeps
its persistable model spread across two structs:

- `PatchesClapPlugin` owns `dsl_source`, `module_paths`, `registry`,
  audio endpoints, and the host pointer.
- `GuiState` (in `patches-plugin-common`) owns `file_path`,
  `module_paths` (mirrored), `tap_opts`, status log, halt info,
  request flags (`browse_requested`, `reload_requested`, …), and the
  current diagnostic view.

Mutation happens in roughly four places:

1. CLAP lifecycle entry points (`activate`, `state_load`,
   `state_save`, `gui_create`, …).
2. `on_main_thread`, which polls `*_requested` flags and runs handlers
   inline.
3. `Intent::apply` on the webview thread, which mutates `GuiState` for
   intents that don't need main-thread work.
4. The audio thread, indirectly, through halt observation and plan
   adoption.

Each handler is responsible for keeping the two stores in sync, calling
`mark_state_dirty` when something persistable changed, calling
`request_restart` when it must, and ordering the side-effects correctly
(scan registry *before* compile, push diagnostics on failure, …). The
discipline is by convention. Symptoms of the drift visible today:

- `mark_state_dirty` was missing entirely until 0754 added it ad-hoc to
  four call sites; tap-opts and window-size changes still don't dirty
  state because they happen on the webview thread with no host pointer.
- `module_paths` lives on both `p` and `g`; every handler that touches
  it has to write both, and the ordering matters for the post-snapshot
  rendering.
- "Reload" reads the file, compiles it, and only later (via
  `request_restart` → `activate`) rescans module paths. If a patch
  references a module that lives in a scan path, reload fails before
  the registry would have learned about it.
- Tests of state-transition logic require standing up a fake CLAP host
  and FFI plugin scanner because the logic is wired directly to host
  callbacks.

The `Intent` enum is already MVC-shaped; the rest of the code hasn't
caught up.

## Decision

Introduce a controller layer in `patches-plugin-common` that owns the
persistable model and exposes a single entry point for state
transitions. Rename the existing `Intent` to `Action` and broaden it to
cover host events as well as UI gestures. Every handler returns a
`StateDelta` describing what changed; the shell (CLAP plugin) reacts to
the delta by calling host callbacks and pushing a fresh snapshot.

### Controller

```rust
pub struct Controller {
    // Persistable model — what state.save writes.
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
    pub module_paths: Vec<PathBuf>,
    pub tap_opts: HashMap<usize, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,

    // Derived / live state.
    pub registry: Registry,
    pub status_log: VecDeque<String>,
    pub diagnostic_view: DiagnosticView,
    pub halt: Option<HaltInfoSnapshot>,
    pub taps: Vec<TapSummary>,
    pub module_names: Vec<String>,
}

impl Controller {
    pub fn apply(&mut self, action: Action, env: &mut dyn Env) -> StateDelta { … }
    pub fn snapshot(&self) -> GuiSnapshot { … }
}
```

`Env` is a trait the controller calls into for side-effects it cannot
perform itself: `read_file`, `pick_file`, `pick_folder`,
`compile_and_push_plan`, `scan_paths`. Host shells supply concrete
impls; tests supply fakes.

### Action

`Action` is the closed set of state transitions:

```rust
pub enum Action {
    // UI gestures
    Browse,
    Reload,
    LoadPath(PathBuf),
    AddModulePath,
    AddModulePathDirect(PathBuf),
    RemoveModulePath(usize),
    Rescan,
    SetTapOpts { slot: usize, … },
    SetWindowSize(u32, u32),

    // Host events
    Activate,
    Deactivate,
    StateLoad(SerializedState),
    HaltObserved(HaltInfoSnapshot),
    PlanAdopted,
    DiagnosticsDrained(Vec<RenderedDiagnostic>),
}
```

Webview JSON intents and `state_load` both deserialise into `Action`.
There is exactly one place in the code that mutates persistable state.

### StateDelta

```rust
pub struct StateDelta {
    pub persistable_changed: bool,   // call mark_state_dirty
    pub requires_restart: bool,      // call request_restart
    pub snapshot_changed: bool,      // republish GuiSnapshot
    pub plan_recompile: bool,        // env.compile_and_push_plan
}
```

The shell's main-thread pump reduces to:

```rust
for action in drained_actions {
    let delta = controller.apply(action, &mut env);
    if delta.persistable_changed { plugin.mark_state_dirty(); }
    if delta.requires_restart    { plugin.request_restart(); }
    if delta.snapshot_changed    { gui.push_snapshot(controller.snapshot()); }
}
```

### Lifecycle ordering

The controller fixes one concrete bug: `Action::Reload` and
`Action::LoadPath` run *scan-then-compile* by construction, so a
patch that depends on an FFI module loads correctly the first time.
`Action::Rescan` becomes a controller-internal op that rebuilds the
registry and recompiles the current source in one step;
`request_restart` becomes optional rather than load-bearing.

### Two-pass rescan

Per [ADR 0044 §3](0044-dynamic-module-loading-reload.md), reload is
always hard-stop: we do not keep older and newer module versions
simultaneously resident, and there is no in-place hot-swap. The
question is therefore not *how* to swap but *whether* a restart is
needed at all. `Rescan` splits into two passes:

1. **Probe.** Walk the configured `module_paths`, read each candidate
   bundle's manifest (name + version + ABI), and diff against the
   currently-registered builders. Cheap — does not keep the library
   loaded, does not construct instances, runs on the main thread.
   Output is a `RescanProbe { added, replaced, removed, unchanged,
   errors }`.
2. **Apply.** If the probe shows any `added` / `replaced` / `removed`
   entries, the controller flips `requires_restart` and the shell
   calls `request_restart`; `activate` then performs the full load
   per ADR 0044. If the probe is empty (only `unchanged`), the
   controller updates the status log and the `module_names` mirror
   from probe results and returns without restarting.

The probe also surfaces ABI-mismatch / dlopen errors immediately to
the GUI, before the engine stops, so the user sees the cause without
losing audio when nothing actionable was found.
`Action::AddModulePath` runs the probe automatically as a preview;
the user still presses Rescan to apply.

## Consequences

**Good:**

- One place to reason about persistable state. `mark_state_dirty`
  cannot be forgotten — it's a property of the action, not a sprinkle.
- Controller is testable without CLAP, FFI, or webview. Snapshot diffs
  become unit tests.
- Reload is no longer racy w.r.t. external module paths.
- Future plugin shells (VST3, standalone) reuse the controller; they
  only need to write a CLAP-equivalent shell + `Env` impl.
- Host-thread/webview-thread split becomes explicit: webview posts
  `Action` to a queue, main thread drains. No more shared `*_requested`
  flags.

**Bad:**

- Real refactor. Not a one-PR job; touches `patches-plugin-common`,
  `patches-clap`, the webview JS bridge (intents already JSON, but
  shapes shift), and every test that currently mocks `GuiState` directly.
- Some currently-cheap operations (e.g. setting a single tap opt) round
  through the controller and emit a snapshot diff. Throughput is fine
  at GUI rates; we should still benchmark the snapshot path before
  removing the existing dedupe cache.
- `Env` is another trait to maintain. Worth it only if we keep the
  trait small (file I/O, dialogs, plan dispatch) and resist the urge to
  push every host callback through it.

### Audio-thread events as actions

The audio thread does not push into the action queue. Existing channels
are all poll-shaped:

- `HaltHandle` — audio thread stores halt state in atomics
  (ADR 0051); main thread reads a snapshot.
- `DiagnosticReader` — observer thread (not audio) writes to a
  queue; main thread drains.
- `plan_rx` (rtrb ring) — main → audio only. Adoption is currently
  silent; if the controller needs to react, we add either a small
  return ring or a "last-adopted plan id" atomic.

The controller pump therefore *polls and synthesises* each tick: it
reads each channel, diffs against last-seen state, and constructs
`Action::HaltObserved` / `Action::DiagnosticsDrained` /
`Action::PlanAdopted` when something changed. No SPSC-from-audio is
required and the audio thread keeps its existing real-time discipline.

Tap-frame pushes (per-tick scope/spectrum data) stay outside this
model. They are a high-frequency channel, not a state transition: the
controller owns the *manifest* (which slots exist, what kind they
are); the observer pipeline owns the *samples* and pushes them to the
webview directly via `applyTaps`. Snapshot dedupe and tap-frame
dedupe remain independent.

## Migration

Suggested epic ordering (each step keeps the build green):

1. Land `Controller`, `Action`, `StateDelta`, `Env` in
   `patches-plugin-common`. No callers yet; pure addition.
2. Move `module_paths`, `dsl_source` into `Controller`. CLAP plugin
   delegates accessors. `GuiState` mirror retained for now.
3. Migrate handlers one at a time: `Browse` → `LoadPath`,
   `Reload`, `Add/RemoveModulePath`, `Rescan`. Delete the
   `*_requested` flags as each one moves.
4. Migrate `Activate`, `StateLoad` into `Action::*`.
5. Move `tap_opts` and window size into the controller; webview's
   direct-mutation path becomes `Action::SetTapOpts` / `SetWindowSize`.
6. Delete the duplicated fields from `GuiState`; `GuiSnapshot::from_state`
   becomes `Controller::snapshot`.

Each step ships independently and is reversible.
