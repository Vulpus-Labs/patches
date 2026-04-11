# E059 — Interpreter and TrackerData planning

## Goal

Define the `TrackerData` types in patches-core, add the
`ReceivesTrackerData` trait, and wire the interpreter and planner to
collect pattern/song blocks into `Arc<TrackerData>` and distribute it to
modules on plan activation.

After this epic:

- `TrackerData`, `PatternBank`, `SongBank`, `Pattern`, `Song`, and `Step`
  are defined in patches-core.
- `ReceivesTrackerData` is an opt-in trait on `Module`, following the
  `ReceivesMidi` precedent.
- The interpreter collects parsed pattern/song blocks, builds
  `TrackerData`, and validates all cross-references (pattern existence,
  step count consistency, channel count consistency, song-sequencer
  channel matching).
- `ExecutionPlan` carries `Option<Arc<TrackerData>>` and a list of
  receiver indices.
- The planner broadcasts the `Arc` to all implementing modules on plan
  activation.

## Background

ADR 0029 describes the full design. The distribution model follows the
`ReceivesMidi` precedent: an opt-in trait, planner scans during build,
`Arc` clone on activation (ref-count bump only), old data freed on the
cleanup thread. The audio thread never allocates or frees tracker data.

## Tickets

| ID   | Title                                                        | Dependencies  |
| ---- | ------------------------------------------------------------ | ------------- |
| 0318 | patches-core: TrackerData, Pattern, Song, Step structs        | —             |
| 0319 | patches-core: ReceivesTrackerData trait                       | 0318          |
| 0320 | Interpreter: collect pattern/song blocks, build TrackerData   | 0318, E057    |
| 0321 | Interpreter: tracker validation rules                         | 0320          |
| 0322 | ExecutionPlan: tracker_data field and receiver indices         | 0319          |
| 0323 | Planner: broadcast Arc<TrackerData> on plan activation         | 0322          |

Epic: E059
ADR: 0029
