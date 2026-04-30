---
id: "0763"
title: Poll-and-synthesise audio-thread events as actions
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061", "0051"]
---

## Summary

The shell pump polls existing audio/observer channels each tick and
synthesises actions on diff: `HaltObserved`, `DiagnosticsDrained`,
`PlanAdopted`. Replaces the inline halt-sync and diagnostic-drain
blocks in `on_main_thread` with controller-driven mutation.

## Acceptance criteria

- [ ] `HaltHandle` snapshot diffed against last-seen; on change emits
      `Action::HaltObserved(snapshot)`.
- [ ] `DiagnosticReader::drain()` empties into
      `Action::DiagnosticsDrained(Vec<RenderedDiagnostic>)`.
- [ ] Plan adoption signal: smallest viable mechanism (an
      `AtomicU64` adopted-plan-id bumped by `adopt_plan`, polled by
      the shell). On change emits `Action::PlanAdopted`.
- [ ] No SPSC ring from the audio thread; audio thread keeps its
      existing real-time discipline.
- [ ] Halt banner + status-log behaviour identical to today.

## Notes

`PlanAdopted` may end up unused in the controller; ship it only if a
handler actually needs it. If not, drop it from the action set and
record that decision in the ticket close-out.
