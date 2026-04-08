# Hot-reloading patches

Hot-reload is the defining feature of Patches. The running audio stream is never
interrupted; the graph is rebuilt around the existing module instances.

## What survives a reload

A module instance survives if the new patch contains a module of the same type at
the same structural position in the graph. Its internal state — oscillator phase,
filter history, envelope stage — is carried forward.

Modules that are new in the new patch are freshly initialised. Modules removed from
the patch are tombstoned and deallocated on a background thread.

## What resets

- Parameters changed inline (`frequency: 440Hz`) are applied to the surviving
  module as parameter updates, without reinitialising it.
- Cable connectivity changes (connections added or removed) notify affected modules
  via `set_connectivity`, so they can adapt (e.g. skip processing an unconnected
  output).

## Practical tips

- You can add, remove, or rewire modules freely while the patch is running.
- Changing a module's *type* (e.g. replacing `Osc` with `PolyOsc`) will reset
  that module, since the types do not match.
- If the new patch file contains a parse or validation error, the old patch
  continues running and the error is printed to stderr.
