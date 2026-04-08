# Amplifiers & VCAs

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Vca` — Mono voltage-controlled amplifier

Multiplies an audio signal by a control signal.

**Inputs**

| Port | Description |
|---|---|
| `in` | Audio input |
| `cv` | Control voltage (0–1 typical) |

**Outputs**

| Port | Description |
|---|---|
| `out` | `in × cv` |

---

## `PolyVca` — Per-voice VCA

Same as `Vca` but operates on poly cables, multiplying each voice independently.

**Inputs**

| Port | Description |
|---|---|
| `in` | Per-voice audio (poly) |
| `cv` | Per-voice control voltage (poly) |

**Outputs**

| Port | Description |
|---|---|
| `out` | Per-voice `in × cv` (poly) |
