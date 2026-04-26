# ADR 0055 — Observation bringup via ratatui patches-player

**Date:** 2026-04-26
**Status:** Accepted
**Related:**
[ADR 0043 — Cable tap observation](0043-cable-tap-observation.md),
[ADR 0053 — Observation three-thread split](0053-observation-three-thread-split.md),
[ADR 0054 — Tap DSL and modules](0054-tap-dsl-and-modules.md)

## Context

ADRs 0043, 0053, and 0054 specify the observation architecture: tap
DSL syntax, tap modules writing into a fixed backplane, an SPSC frame
ring to an observer thread, and an observer-side analysis pipeline
producing telemetry for a UI subscriber. None of it is implemented.

In parallel the existing CLAP plugin shell (`patches-clap`) has grown
strongly-coupled control + view + ad-hoc meter code that doesn't
match this architecture and isn't worth refactoring in place. The
plan is to nuke and rewrite the plugin shell once the observation
layer and a clean control-plane API exist.

A reference frontend is needed to drive the observation API into
shape before the plugin rewrite. `patches-player` (the existing
headless CLI) is the natural candidate: no FFI, no host main-thread
dance, no parented windows, fast iteration. A ratatui TUI gives us
real interactive views (meters, status, event log) without the
toolkit-binding cost a GUI would impose.

This ADR records the bringup sequence and scope. Each step lands
behind its own ticket; this ADR is the through-line.

## Decision

Build the observation machinery end-to-end against a ratatui
`patches-player` frontend before touching the CLAP shell. Scope of
this bringup is **`meter` taps (fused peak + RMS) only**.
Oscilloscopes, spectra, LEDs, SVG graph rendering, and the
controller/command surface for live patch manipulation are deferred
to follow-up ADRs once the underlying machinery is exercised.

### Sequence

1. **DSL changes.** Implement ADR 0054 syntax for `~taptype(name, …)`
   tap targets in cable expressions. Parser, expander desugaring into
   implicit `AudioTap` / `TriggerTap` modules, alphabetical slot
   assignment, manifest emission. Top-level-only enforcement. For
   bringup: only the `meter` tap type needs wiring through the
   parser; the rest of the grammar (including compound tap types per
   ADR 0054 §1) lands but observer support is stubbed.

2. **Engine changes.** Add the backplane (`[f32; MAX_TAPS]` per ADR
   0053 §4 with `MAX_TAPS = 32`). Implement `AudioTap::tick` writing
   `backplane[slot_offset + i] = inputs[i]`. Allocate the SPSC frame
   ring; implement the per-block write-chunk + commit + drop-on-full
   path. No observer thread yet — the audio side just writes into the
   ring.

3. **Observer module.** New `patches-observation` crate. Owns: the
   consumer end of the ring, the manifest, per-slot pipeline state,
   and a subscriber interface. No engine, no audio, no UI
   dependencies. Runs on its own thread. Initial pipeline: `meter`
   (fused running max-abs with ballistic decay + rolling-window RMS,
   per ADR 0054 §7). Sample-rate-aware.

4. **Wire-up.** Planner emits the manifest (ADR 0054 §6) and ships it
   to the observer over the planner→observer control ring (ADR 0053
   §6). Engine pushes frames; observer drains, runs pipelines,
   exposes latest values per slot to subscribers via an atomic-scalar
   surface (ADR 0053 §7 "latest scalars").

5. **patches-player ratatui skin.** Replace the current CLI status
   output with a ratatui frontend. Initial views:
   - Header: patch path, sample rate, oversampling, engine state.
   - Meter pane: one bar pair (peak + RMS) per declared meter tap,
     labelled by tap name, colour-banded by dB level.
   - Event log pane: scrolling status messages (load, reload,
     compile errors, recording start/stop, observer drop notices).
   - Footer: keybindings.
   Keybindings for this bringup: `r` start/stop recording (uses the
   existing `wav_recorder`), `q` quit. No live patch reload, no
   module rescan, no oversampling change yet — those come with the
   controller ADR.

6. **Observer → UI dispatch.** TUI subscribes to the observer's
   latest-scalar surface, polls at frame rate (~30 Hz). Per-tap drop
   counters surface in the event log so observation gaps are
   diagnosable.

### Out of scope for this bringup

- Oscilloscope and spectrum pipelines (require triple-buffer surface
  and per-slot state; defer until peak/rms is stable end-to-end).
- LED tap types (`gate_led`, `trigger_led`).
- Live controller surface (load patch, rescan modules, change
  oversampling). The reload mechanism in the existing `patches-player`
  remains as-is for this bringup.
- CLAP plugin rewrite. Tracked separately; gated on the observation
  API and controller surface stabilising in `patches-player`.
- SVG patch-graph rendering in the TUI (explicitly not pursued).

### Reference frontend principle

`patches-player` is the **reference frontend** for the observation
layer and (later) the control plane. The CLAP shell, when rewritten,
conforms to the API shape `patches-player` validates. New observation
or control features land in `patches-player` first; the plugin shell
adopts them without inventing parallel mechanisms. This avoids the
historical pattern where the plugin grew shell-specific shortcuts
that rotted the abstraction.

## Consequences

**Positive**

- End-to-end exercise of the observation pipeline against a real
  interactive consumer before the plugin rewrite begins.
- TUI's redraw-and-poll model surfaces allocation, blocking, and
  latency problems closer to the plugin's real constraints than a
  webview would.
- Existing `patches-player` and `wav_recorder` keep working
  throughout; recording stays as the post-decimation stereo special
  case (not a tap).
- Scope is narrow enough to fit a single delivery cycle: DSL +
  engine + observer + TUI + two pipelines.

**Negative**

- TUI work (ratatui layouts, input handling, redraw cadence) is real
  effort that produces no plugin code directly. Justified by the
  reference-frontend principle but worth naming.
- One pipeline (`meter`) is a small surface to design the observer
  API against; the API may need revision when scope/spectrum
  pipelines and compound tap fan-out land. Acceptable — better to
  revise once than to speculate.
- The DSL grammar additions land before all tap types have observer
  support. The parser accepts what the observer can't yet render;
  unsupported tap types should produce a "not yet implemented"
  diagnostic rather than silently no-oping.

**Neutral**

- No CLAP work in this sequence. Plugin shell stays on its current
  ad-hoc meter path until the rewrite ADR.

## Cross-references

- ADR 0043 — original cable tap observation framing (superseded in
  detail by 0053/0054, retained for context).
- ADR 0053 — three-thread split, backplane, frame ring, MAX_TAPS=32.
- ADR 0054 — tap DSL syntax, implicit tap modules, manifest shape,
  observer-side pipelines.
