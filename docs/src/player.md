# Running the player

`patches-player` is the main binary. It loads a patch file, starts audio, and
watches the file for changes.

```bash
cargo run -p patches-player -- <path-to-patch>
```

## Command-line usage

```
patch_player <FILE>
```

`FILE` must be a `.patches` file. The player uses the first available audio
output device at its default sample rate.

## What the player does on startup

1. Parses and validates the patch file.
2. Builds an execution plan from the module graph.
3. Opens the audio stream via CPAL and begins ticking the plan.
4. Starts a file-watcher thread that monitors `FILE` for writes.

## On file change

1. The new file is parsed and validated.
2. A new execution plan is built, reusing module instances where possible
   (matched by type and structural position in the graph).
3. The new plan is sent to the audio thread via a lock-free ring buffer.
4. The audio thread swaps the plan in at a safe point between samples.

Module state (oscillator phase, envelope position, filter history) is preserved
for any module that survives the reload.
