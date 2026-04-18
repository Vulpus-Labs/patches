---
id: "0516"
title: New patches-host crate with shared player/CLAP composition
priority: medium
created: 2026-04-17
---

## Summary

Create `patches-host`, a crate that bundles the composition currently
duplicated between `patches-player` and `patches-clap`: registry
init, DSL pipeline driving, planner construction, plan-channel
wiring, processor spawn. Expose traits for the divergent bits so each
binary plugs in its own file source, audio callback structure, and
event handling.

Part of epic E089 (see ADR 0040). Depends on 0513 and 0515.

## Acceptance criteria

- [x] New `patches-host/` crate exists with `publish = false`.
- [x] Depends on `patches-core`, `patches-dsl`, `patches-interpreter`,
  `patches-registry`, `patches-planner`, `patches-engine`,
  `patches-diagnostics` (plus `rtrb` for the plan/cleanup channels).
- [x] Trait surface:
  - `HostFileSource` with `PathSource` (player-style file load with
    include resolution) and `InMemorySource` (CLAP-style string,
    optional master path for include resolution, optional base dir
    for asset resolution). Exposes a `LoadedSource` value type.
  - `HostAudioCallback` — minimal install-once trait. Concrete
    `process` shape deliberately deferred to wave 5; ticket flags
    this surface as expected to bend under the first two consumers.
  - `CompileError` (lifted from `patches-clap`) is the shared
    stage-tagged error / diagnostic surface — supersedes a separate
    `HostPatchSource` trait, which would just re-wrap it.
- [x] `HostBuilder::build(env)` returns a `HostRuntime` carrying the
  `Planner`, `PatchProcessor`, plan channel, and cleanup-thread join
  handle.
- [x] `load_patch(source, registry, env)` runs expand → bind → build;
  `HostRuntime::compile_and_push` adds the planner stage and pushes the
  resulting `ExecutionPlan` onto the plan channel.
- [x] Tests: `tests/host.rs` covers builder construction, end-to-end
  compile-and-push, parse-stage error surfacing, and a third-party
  `HostFileSource` impl (object-safety check).
- [x] `cargo build`, `cargo test -p patches-host` (4 passed),
  `cargo clippy -p patches-host` clean (no host-crate warnings;
  workspace warnings are pre-existing in `patches-engine`).

## Notes

The trait shape will bend under the first two real consumers.
Expect to iterate after 0517 and 0518 land; the point of this ticket
is to give them a starting surface, not to freeze the API.

Research for the epic identified ~250+ lines of duplicated wiring
between `patches-player/src/main.rs` and
`patches-clap/src/plugin.rs` + `patches-clap/src/factory.rs`. Consult
those files when shaping the traits.

The CLAP path also has a cleanup thread and GUI state management
that are not shared with player; leave those in `patches-clap`.

## Outcome

`patches-host` lands as a 5-module crate (`error`, `source`, `load`,
`builder`, `callback`) totaling ~330 LoC + ~110 LoC of tests. No code in
`patches-player` or `patches-clap` is touched yet — that's wave 5
(0517 / 0518), where the trait shape will iterate.

Design notes for the consumer ports:

- `CompileError` here is the same shape as `patches-clap`'s, plus a
  `Parse` arm and a `NotActivated` arm. The CLAP error type can be
  replaced with a re-export. `patches-player`'s `LoadPatchError` can
  collapse into it (the `Bind` variant already carries
  `Vec<BindError>`); the player's `render_to_stderr` becomes
  diagnostic-render glue around `to_rendered_diagnostics`.
- `HostRuntime::compile_and_push` does best-effort plan-channel push
  (drops on full). The player's `push_build_result` retry-with-sleep
  loop can stay in the player layer or move behind a `compile_and_push_blocking`
  helper if both consumers want it — defer.
- `HostAudioCallback` is intentionally just `install`. The player's
  callback lives inside `patches-cpal::PatchEngine` and the CLAP
  callback is a sample-accurate loop with transport extraction; a
  unified `process` signature would freeze the wrong shape.
- MIDI: not part of the host surface. Player builds its own MIDI
  connector around `patches-engine`'s scheduler; CLAP feeds events
  inline from the host's event list. Crossing that boundary in host
  would require committing to one of the two extraction styles.
