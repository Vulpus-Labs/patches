# Module reference

Each module type is documented with its ports, parameters, and behaviour. The canonical source of truth for port names and parameter ranges is the doc comment on each module struct in `patches-modules/src/`.

| Category | Modules | Purpose |
|----------|---------|---------|
| [Oscillators](oscillators.md) | `Osc`, `PolyOsc`, `Lfo` | Waveform generation |
| [Envelopes](envelopes.md) | `Adsr`, `PolyAdsr` | Amplitude and modulation shaping |
| [Filters](filters.md) | `Lowpass`, `Highpass`, `PolyLowpass`, `PolyHighpass` | Frequency-dependent attenuation |
| [Amplifiers & VCAs](vcas.md) | `Vca`, `PolyVca`, `StereoVca` | Voltage-controlled amplitude |
| [Mixers](mixers.md) | `Sum`, `PolySum`, `StereoMixer`, `MonoToMono` | Signal summing and routing |
| [Noise](noise.md) | `Noise` | White, pink, brown, and red noise |
| [Sequencers & clocks](sequencers.md) | `Clock` | Tempo-locked trigger generation |
| [MIDI](midi.md) | `MidiIn`, `PolyMidiIn` | MIDI keyboard and controller input |
| [Delays & reverb](delays.md) | `Delay` | Multi-tap delay with feedback |
| [Dynamics](dynamics.md) | `Limiter` | Lookahead peak limiting |
| [Utilities](utilities.md) | `Glide`, `MonoToPoly`, `PolyToMono` | Signal conversion and smoothing |
| [Output](output.md) | `AudioOut` | Stereo audio output (required) |
