---
id: "E070"
title: Poly layout validation
created: 2026-04-12
tickets: ["0367", "0368"]
adr: "0033"
---

## Summary

Add interpreter-level validation for structured poly connections
(ADR 0033, Phase 2). A `PolyLayout` enum tags poly ports with
their expected frame format (`Audio`, `Transport`, `Midi`). The
interpreter rejects connections between incompatible non-`Audio`
layouts at patch load time, catching wiring errors that would
otherwise cause silent data corruption.

## Tickets

| Ticket | Title                                       |
| ------ | ------------------------------------------- |
| 0367   | PolyLayout enum and descriptor integration  |
| 0368   | Interpreter validates poly layout compatibility |

0367 must land first; 0368 depends on it.
