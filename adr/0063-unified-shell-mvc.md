# ADR 0063 — Unified shell MVC across CLAP and Ratatui

**Date:** 2026-04-30
**Status:** Proposed
**Related:**
[ADR 0053 — Observation three-thread split](0053-observation-three-thread-split.md),
[ADR 0055 — Observation bringup via Ratatui player](0055-observation-bringup-via-ratatui-player.md),
[ADR 0057 — Host control as boundary-crossing cables](0057-host-control-cables.md),
[ADR 0058 — Subscriber surface and UI decoupling](0058-subscriber-surface-and-ui-decoupling.md),
[ADR 0061 — Plugin controller, actions, and state deltas](0061-plugin-controller-action-state-delta.md),
[ADR 0062 — Cable range expressions](0062-cable-range-expressions.md)

## Context

Patches has two interactive shells in flight:

- **`patches-clap`** — CLAP plugin, webview GUI (wry), embedded in a DAW.
  Host owns persistence; webview JSON intents drive state changes.
- **`patches-player`** — Ratatui terminal app, current home of observation
  bringup (ADR 0055). No plugin host, no persistence, no controls.

Both shells need to ship the same functional surface:

1. Load / reload a `.patches` file, manage module-scan paths, surface
   diagnostics.
2. Render observation frames (meters, scopes, spectra) from the
   subscriber surface (ADR 0058).
3. Drive host-control values (ADR 0057) into the audio thread and
   persist them across sessions.
4. Eventually expose presets — named bundles of host-control values
   bound to a patch.

Today these are diverging. `patches-clap` is mid-migration to the
Controller / Action / StateDelta model (ADR 0061) but the `Controller`
type does not yet exist. `patches-player` has its own ad-hoc state for
tap rendering and never went through `patches-plugin-common` at all.
Host controls (0057) and range expressions (0062) are unimplemented in
either shell. CLAP has `state_load` / `state_save`; Ratatui has nothing.

If we land 0057, 0062, persistence, and presets shell-by-shell, we
double the surface area and guarantee divergence. The prerequisite is a
common MVC core that both shells consume. This ADR enumerates the
pieces that have to come together and the order in which they land.

The scope is *what must unify*, not the bytes-on-the-wire of each
piece — those live in their respective ADRs. This ADR is the
integration map.

## Decision

### 1. `patches-plugin-common` is the shared MVC crate

Both shells depend on it. It owns:

- `Controller` (ADR 0061): persistable model + derived state + `apply`
  entry point.
- `Action` enum: closed set of state transitions, source-agnostic.
- `StateDelta`: post-condition flags the shell reacts to.
- `Env` trait: side-effects the controller cannot perform itself
  (file I/O, dialogs, plan dispatch, sidecar read/write — see §5).
- `GuiSnapshot`: the read-model both UIs render from.
- `SerializedState`: the persistable subset, transport-agnostic.

**Constraint:** `patches-plugin-common` must not depend on `clap-sys`,
`wry`, `ratatui`, or any host SDK. Today it has CLAP-flavoured types
(`HaltInfoSnapshot` is fine; anything `clap_*` is not). Audit and hoist
CLAP-specific surfaces into `patches-clap` before step 2 of the 0061
migration.

### 2. Shared action vocabulary

`Action` is the union of every state transition either shell can
trigger. Sources (keystroke, JSON intent, host callback) are erased at
the controller boundary.

Inherits from 0061:

- `Browse`, `Reload`, `LoadPath`, `AddModulePath{,Direct}`,
  `RemoveModulePath`, `Rescan`
- `SetTapOpts`, `SetWindowSize`
- `Activate`, `Deactivate`, `StateLoad`, `HaltObserved`,
  `PlanAdopted`, `DiagnosticsDrained`

Adds for host controls (this ADR):

- `SetHostControl { name: String, value: f32 }` — both UIs lower knob
  drags and CLAP parameter events to the same action. Name-keyed, not
  slot-keyed (ADR 0057 §3 slots churn on add/remove).
- `HostControlManifestUpdated(HostControlManifest)` — emitted by the
  planner after a successful compile; controller stores the manifest
  for snapshot delivery and reconciles current values (drop entries
  for removed names, populate defaults for new names from manifest
  metadata).

Adds for persistence (§5):

- `SaveSidecar` / `LoadSidecar(path)` — Ratatui-only by use, but
  expressible in the shared vocabulary so CLAP can opt in for export.

### 3. Snapshot is the read-model for both UIs

`GuiSnapshot` extends with:

```rust
pub struct GuiSnapshot {
    // existing 0061 fields …
    pub host_controls: Vec<HostControlSnapshot>,
}

pub struct HostControlSnapshot {
    pub name: String,
    pub kind: HostControlKind,        // Knob | Slider | Toggle
    pub value: f32,                   // current value
    pub params: HostControlParamMap,  // range, default, label, taper, units
}
```

Webview JS and Ratatui widgets render from the same struct. The
webview already has `applyTaps` (per ADR 0061's note that tap-frame
pushes stay outside the snapshot model); host-control updates can ride
the snapshot path because they are state, not high-frequency frames.

### 4. Two interchange channels with audio, both already specified

The controller mediates both directions:

- **Control → audio** (host controls): backplane region from ADR 0057
  §4. The handle to write the backplane lives on the controller (or
  on `Env`, TBD during impl — the controller-owned option keeps the
  audit trail tighter, but requires the controller to outlive the
  audio plan, which it does). `Action::SetHostControl` writes one
  slot, marks `persistable_changed`.
- **Audio → observation** (taps): the subscriber surface from
  ADR 0058. `LatestValues` for state-shaped observations,
  `Diagnostic` ring for events. Both shells subscribe identically;
  the controller owns the *manifest* (which slots, what kind), the
  observer pipeline owns the *samples*.

Range-expression lowering (ADR 0062) happens at builder time and is
invisible to the controller. The controller writes `[0, 1]` for knobs
and `[-1, 1]` for any future bipolar control source; the cable's
range expression maps to the destination. This keeps the controller
free of per-destination unit knowledge.

### 5. Persistence: `SerializedState` over two transports

```rust
pub struct SerializedState {
    pub host_controls: HashMap<String, f32>,
    pub tap_opts: HashMap<String, TapDisplayOpts>,  // name-keyed, see below
    pub window_size: Option<(u32, u32)>,
    pub module_paths: Vec<PathBuf>,
}
```

Two transports:

- **CLAP** uses `state_save` / `state_load` callbacks. Bytes go into
  the DAW project file. CLAP shell serialises `SerializedState` to
  bytes and back.
- **Ratatui** uses a sidecar file `<patch>.patches.state` adjacent to
  the loaded `.patches`. `Env::load_sidecar(path)` /
  `Env::save_sidecar(path, state)` are the only new `Env` methods.
  The CLAP `Env` impl no-ops them; the Ratatui `Env` impl reads/writes
  JSON.

Sidecar lifecycle:

- On `LoadPath`, after compile succeeds, controller emits
  `Action::LoadSidecar` if the env reports one exists. Missing sidecar
  → defaults from manifest.
- On any `persistable_changed` delta, Ratatui shell schedules a
  debounced `SaveSidecar`. CLAP shell calls `mark_state_dirty`
  instead — the host writes when it chooses.
- Stale entries (renamed/removed knobs) are dropped on the next save
  after manifest reconciliation. Mirrors CLAP's tombstone behaviour
  (ADR 0057 §6) but is simpler: the sidecar is owned by the
  controller, the CLAP tombstone table is owned by the host's
  parameter ID space. The two coexist; neither subsumes the other.

`tap_opts` keys move from slot index to tap name for the same reason
host controls use names: slot churn on patch edits.

The CLAP plugin reconciles host-driven CLAP parameter values with
controller values on `state_load` and `Activate`. Conflict policy:
host wins (the DAW just loaded a project; its automation lane is
authoritative). Documented in the CLAP-specific impl, not in this ADR.

### 6. Presets fall out of `SerializedState`

A preset is `SerializedState` plus a patch identity (file path or
content hash). Save: write to a preset library directory. Load: apply
via `Action::StateLoad`. Cross-patch presets work because bindings are
name-keyed; presets degrade gracefully when the target patch doesn't
have all the named knobs.

CLAP host preset browser integration (`clap_plugin_preset_load`) is a
later concern. The internal preset format is fixed by
`SerializedState` and is the same for both shells.

### 7. Migration ordering

This ADR does not block on its own completion to be useful — each
piece is independently shippable. Recommended order:

1. **Audit `patches-plugin-common`** for CLAP leakage; hoist as needed.
2. **Land Controller / Action / StateDelta / Env** (ADR 0061 step 1).
3. **Migrate `patches-clap` handlers** to Controller (ADR 0061
   steps 2–6).
4. **Migrate `patches-player`** to Controller + Ratatui `Env` impl.
   Tap rendering already goes through the subscriber surface; this is
   mainly a state-management migration.
5. **Implement ADR 0057** against the unified Controller. Host-control
   manifest + backplane writer live on the controller; both shells
   call `SetHostControl`.
6. **Implement ADR 0062** in the DSL pipeline. Independent of the
   shell migration but unblocks meaningful knob wiring.
7. **Land sidecar persistence** (this ADR §5). Both shells gain
   `SerializedState` round-trip.
8. **Preset library** (this ADR §6). Pure addition over §7.

Steps 5 and 6 can run in parallel with 4. Step 7 depends on 3 and 4.

## Consequences

**Good:**

- Both shells share one model, one action vocabulary, one read-model,
  one persistence schema. New features land once.
- Presets, sidecars, and CLAP state all serialise the same struct.
  No second schema to maintain.
- Host-control plumbing (0057) and range expressions (0062) target a
  single integration surface.
- Ratatui shell becomes a first-class consumer of
  `patches-plugin-common`, not a parallel implementation. Reduces the
  observability bringup tax.

**Bad:**

- Migration is multi-crate and touches in-flight work. Steps 3 and 4
  must coordinate with whoever holds 0061 migration tickets.
- `patches-plugin-common` grows surface (host-control writer handle,
  sidecar `Env` methods, snapshot extensions). Worth it; documented
  here so the growth is intentional.
- Conflict policy on CLAP `state_load` (host wins vs controller wins)
  is a real decision deferred to impl. This ADR notes the choice but
  does not pin it.

**Neutral:**

- Name-keying for `tap_opts` is a schema change to in-tree state. No
  external compatibility burden because no CLAP project files
  reference Patches state yet in the wild.

## Cross-references

- ADR 0053 / 0056 / 0058 — observation pipeline, unchanged by this
  ADR. The unified controller subscribes through the existing
  surface.
- ADR 0055 — Ratatui as observation bringup vehicle. This ADR
  promotes the Ratatui shell from "test harness for taps" to "peer
  shell to CLAP."
- ADR 0057 — host-control cables. This ADR specifies where the
  manifest lands (controller) and how values reach the audio thread
  (controller-owned backplane writer).
- ADR 0061 — Controller / Action / StateDelta. This ADR extends the
  action vocabulary and snapshot for host controls and persistence.
- ADR 0062 — range expressions. Independent feature; this ADR
  notes that range mapping happens at the cable, not the controller,
  and the controller deals only in normalized values.

## Out of scope

- Sub-block automation accuracy (deferred by ADR 0057 §4).
- CLAP preset browser integration.
- Cross-shell live state sharing (e.g. running CLAP and Ratatui
  against the same patch simultaneously). Not a use case today.
- Audio-thread-side changes. The audio thread keeps its existing
  real-time discipline; all unification happens above the
  control-thread / observer-thread boundary.
