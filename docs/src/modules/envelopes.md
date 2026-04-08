# Envelopes

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Adsr` — Mono ADSR envelope

A standard Attack-Decay-Sustain-Release envelope generator.

**Parameters**

| Parameter | Type | Default | Description |
|---|---|---|---|
| `attack` | float (s) | `0.01` | Attack time in seconds |
| `decay` | float (s) | `0.1` | Decay time in seconds |
| `sustain` | float | `0.7` | Sustain level (0–1) |
| `release` | float (s) | `0.3` | Release time in seconds |

**Inputs**

| Port | Description |
|---|---|
| `trigger` | Rising edge starts the attack |
| `gate` | High = sustain; falling edge starts release |

**Outputs**

| Port | Description |
|---|---|
| `out` | Envelope value (0–1) |

---

## `PolyAdsr` — Per-voice ADSR

Same as `Adsr` but operates independently on each polyphonic voice.

**Parameters** — same as `Adsr`.

**Inputs**

| Port | Description |
|---|---|
| `trigger` | Per-voice trigger (poly) |
| `gate` | Per-voice gate (poly) |

**Outputs**

| Port | Description |
|---|---|
| `out` | Per-voice envelope (poly) |
