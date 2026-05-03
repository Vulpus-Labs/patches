# ADR 0067 — Blast-radius cuts within the monorepo

**Date:** 2026-05-03
**Status:** Proposed
**Related:**
[ADR 0066 — Static descriptor templates and manifest externalization](0066-static-descriptor-templates-and-manifest.md),
[ADR 0064 — Non-Rust FFI plugins](0064-non-rust-ffi-plugins.md)

## Context

The workspace has grown to a point where unrelated changes trigger
unrelated validation work. A change in `patches-modules` retests
`patches-svg` even though svg's job — turning a manifest into an SVG —
has nothing to do with module implementations. A change in a leaf
application (`patches-clap`, `patches-player`) retests nothing else,
but a change in a leaf-only dep can still wash through the workspace
because `cargo test` defaults to `--workspace`.

Development is single-laptop with occasional pushes to GitHub. Agent
iterations currently run the full clippy + workspace test pipeline
per ticket, which is too slow for the inner loop and conflates
fast-feedback signals (does it compile and pass unit tests?) with
slower ones (clippy, integration, plugin scanning).

We are not ready to split the repository. The ensemble is still
evolving fast (grammar iterating on control signals and audio input;
host/registry/observation crates settling). A premature repo split
locks dep direction and version bumps before the shape is stable.

What we *can* do now is make cuts within the monorepo that:

- reduce the blast radius of any single change,
- harden the dep graph against accidental regression,
- separate fast feedback (per agent iteration) from slow checks
  (per push, per epic), so neither is paid when the other is wanted,
- prepare ground for a future repo split without committing to it.

## Decision

Adopt four cuts, in order:

### 1. Lib/bin splits where the binary pulls heavier deps than the lib

`patches-svg` is the immediate offender: the lib produces SVG from a
manifest and has no need for `patches-modules`, but the binary scans
modules to *build* a manifest, and the dep is declared crate-wide. The
result is that any module change retests the renderer.

The cut: separate the renderer (lib, manifest-only) from the discovery
binary (`patches-svg-cli`, links `patches-modules` + `patches-registry`).
Same pattern applies to `patches-tools` if it grows binaries with
heterogeneous deps.

### 2. Tiered validation profiles

Validation runs at four tiers, each with a named target (a `Justfile`
recipe or cargo alias). Each tier is a strict superset of the one
above it.

| Tier     | When                                    | Scope                                                                                                                                         |
| -------- | --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `inner`  | every agent iteration / fast inner loop | unit tests on the inner-loop crate set (`patches-core`, `patches-modules`, `patches-dsp`, `patches-engine`) plus the touched crate; no clippy |
| `commit` | before a commit closing a ticket        | `inner` + `cargo clippy` on touched crates                                                                                                    |
| `push`   | git push (GitHub Actions)               | full workspace `cargo build`, `cargo test`, `cargo clippy`, forbidden-edge lint                                                               |
| `smoke`  | manual, epic close, or scheduled        | `push` + integration tests, plugin scanner, LSP smoke, CLAP smoke, slow/expensive suites                                                      |

The split lets the agent commit on `inner` confidence and rely on
`push` to catch wider regressions on a slower cadence. Static tiers
are deliberately preferred over a dynamic reverse-dep walk: at single-
laptop scale the named-target approach is simpler and sufficient.

### 3. Forbidden-edge lint

Cuts rot if no one watches the dep graph. We codify the rules as a
`cargo deny` configuration (or a small `cargo metadata` walk in CI)
that fails the build when a forbidden edge appears — for example,
`patches-svg` (lib) depending on `patches-modules`, or any "leaf"
binary appearing in another crate's deps.

The forbidden-edge set is the executable form of this ADR.

### 4. `patches-tools` lib/bin split (deferred)

Track separately because the pressure is lower today. When tools grows
a binary with deps the others don't share, apply the same cut as svg.

## Consequences

### Positive

- Per-change retest scope shrinks toward what's actually affected.
- The dep graph becomes a checked artifact, not folklore.
- The workspace stays in one repo, one lockfile, one tooling stack —
  cross-crate refactors remain atomic.
- When we *do* split repos, the cuts already match the seams.

### Negative

- One-time work to split `patches-svg` into lib + cli; downstream
  consumers (`patches-lsp`, future `patches-clap`) update their dep.
- Tier targets (Justfile / cargo aliases) become artefacts to keep in
  sync as the workspace shape changes.
- Forbidden-edge config is another rule contributors must learn.

### Neutral

- The shared `Cargo.lock` still ripples version bumps across the
  workspace. This ADR does not address that; it is the price of
  monorepo and out of scope.

## Non-goals

- Repo split. Out of scope. This ADR explicitly assumes the monorepo
  stays.
- Static-vs-dynamic linkage decisions for module packs (whether
  first-party modules ship statically linked or runtime-loaded
  through the existing FFI vtable). The per-sample path through the
  current FFI is a hand-rolled `#[repr(C)]` vtable taking raw pool
  pointers — cheap on the call itself; the residual cost is lost
  cross-module inlining, which is empirical. That decision is out of
  scope here and depends on measurement, not on the cuts in this ADR.

## Alternatives considered

- **Multi-workspace single repo.** Each subdirectory its own
  workspace. Loses shared lockfile and tooling cohesion; gains little
  the tiered profiles don't already give.
- **Feature-gated inter-crate deps.** Heavy hammer. Cargo's feature
  unification leaks across the workspace and confuses IDE tooling.
- **`cargo-hakari` workspace-hack crate.** Documents feature
  unification but does not reduce blast radius. May be worth adopting
  later for a different reason; not part of this ADR.
