# Summary

[Introduction](introduction.md)

---

# Getting started

- [Installation & quickstart](getting-started.md)
- [Running the player](player.md)
- [Hot-reloading patches](hot-reload.md)

---

# DSL reference

- [Patch syntax overview](dsl/syntax.md)
- [Modules & parameters](dsl/modules.md)
- [Connections & scaling](dsl/connections.md)
- [Indexed ports](dsl/indexed-ports.md)
- [Templates](dsl/templates.md)

---

# Module reference

- [Oscillators](modules/oscillators.md)
- [Envelopes](modules/envelopes.md)
- [Filters](modules/filters.md)
- [Amplifiers & VCAs](modules/vcas.md)
- [Mixers](modules/mixers.md)
- [Noise generators](modules/noise.md)
- [Sequencers & clocks](modules/sequencers.md)
- [MIDI input](modules/midi.md)
- [Delays & reverb](modules/delays.md)
- [Dynamics](modules/dynamics.md)
- [Utilities](modules/utilities.md)
- [Audio output](modules/output.md)

---

# Engine internals

- [Architecture overview](engine/architecture.md)
- [Audio thread guarantees](engine/audio-thread.md)
- [Plan handoff & hot-reload](engine/plan-handoff.md)
- [Off-thread deallocation](engine/deallocation.md)
- [Polyphonic cables](engine/polyphony.md)

---

# Technical notes

- [Cable pool & ping-pong buffer](technical/cable-pool.md)
- [Module lifecycle & identity](technical/module-lifecycle.md)
- [DSP test audit (ADR 0022)](technical/dsp-test-audit.md)

---

# Implementing modules

- [The Module trait](implementing-modules/trait.md)
- [Worked example](implementing-modules/walkthrough.md)
- [Testing with ModuleHarness](implementing-modules/testing.md)

---

# Examples

- [Drone (Radigue-inspired)](examples/drone.md)
- [FM synthesiser](examples/fm-synth.md)
- [Polyphonic synth](examples/poly-synth.md)
- [Layered poly synth](examples/poly-synth-layered.md)
