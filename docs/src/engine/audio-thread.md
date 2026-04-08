# Audio thread guarantees

The audio callback runs under hard real-time constraints on most platforms.
Violating these constraints causes audible glitches (dropouts, clicks, or pops).

## Rules enforced in Patches

**No allocations.** All buffers and module state are pre-allocated when the plan
is built. The audio callback never calls `Box::new`, `Vec::push` (with growth),
`Arc::clone` (may allocate), or any other heap-allocating operation.

**No blocking.** The audio callback never:
- acquires a mutex or other blocking lock
- performs file or network I/O
- calls any syscall that may sleep

**No deallocation.** Dropping a `Box<dyn Module>` or an `ExecutionPlan` may call
arbitrary destructor code, which can allocate, block, or take locks. Patches
routes all deallocation to the `"patches-cleanup"` thread — see
[Off-thread deallocation](deallocation.md).

## How communication works

The control thread (hot-reload, parameter changes) communicates with the audio
thread via an **rtrb** lock-free single-producer / single-consumer ring buffer.
The audio callback polls the ring buffer at the start of each tick; if a new plan
is available it is swapped in with no blocking.

The same ring buffer carries `CleanupAction` values *out* of the audio thread
(dropped modules, evicted plans) to the cleanup thread.

## `CablePool` and `Module::process`

Each module processes one sample per call to `process`. It receives a
`&mut CablePool` and reads its inputs / writes its outputs through pool accessor
methods. The pool uses a ping-pong double-buffer so reads from the previous tick
and writes to the current tick are always distinct memory regions.
