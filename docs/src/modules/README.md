# Module reference

Each module type is documented with its ports, parameters, and behaviour. The canonical source of truth for port names and parameter ranges is the doc comment on each module struct in `patches-modules/src/`.

| Category                             | Modules                                                                                                   | Purpose                                  |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| [Oscillators](oscillators.md)        | `Osc`, `PolyOsc`, `Lfo`, `PolyLfo`                                                                        | Waveform generation                      |
| [Envelopes](envelopes.md)            | `Adsr`, `PolyAdsr`                                                                                        | Amplitude and modulation shaping         |
| [Filters](filters.md)                | `Lowpass`, `Highpass`, `Bandpass`, `Svf`, and poly variants                                               | Frequency-dependent attenuation          |
| [Amplifiers & VCAs](vcas.md)         | `Vca`, `PolyVca`                                                                                          | Voltage-controlled amplitude             |
| [Mixers](mixers.md)                  | `Sum`, `PolySum`, `Mixer`, `StereoMixer`, `PolyMixer`, `StereoPolyMixer`                                  | Signal summing and routing               |
| [Noise](noise.md)                    | `Noise`, `PolyNoise`                                                                                      | White, pink, brown, and red noise        |
| [Sequencers & clocks](sequencers.md) | `Clock`, `TempoSync`, `MsTicker`, `TriggerToSync`, `SyncToTrigger`                                        | Tempo-locked trigger generation          |
| [Tracker sequencer](tracker.md)      | `MasterSequencer`, `PatternPlayer`                                                                        | Song-driven pattern sequencing           |
| [Drum synthesis](drum-synthesis.md)  | `Kick`, `Snare`, `ClosedHiHat`, `OpenHiHat`, `Tom`, `Cymbal`, `Clap`, `Claves`                            | 808-style electronic drum synthesis      |
| [MIDI](midi.md)                      | `MidiToCv`, `PolyMidiToCv`, `MidiCC`, `MidiArp`, `MidiDelay`, `MidiSplit`, `MidiTranspose`, `MidiDrumset` | MIDI input and processing                |
| [Delays & reverb](delays.md)         | `Delay`, `StereoDelay`, `FdnReverb`, `ConvReverb`, `StereoConvReverb`                                     | Delay and reverb                         |
| [Dynamics](dynamics.md)              | `Limiter`, `StereoLimiter`, `Bitcrusher`, `Drive`, `TransientShaper`, `RingMod`, `PitchShift`             | Dynamics and nonlinear effects           |
| [Utilities](utilities.md)            | `Glide`, `Tuner`, `PolyTuner`, `Quant`, `PolyQuant`, `Sah`, `PolySah`, `MonoToPoly`, `PolyToMono`         | Conversion, smoothing, quantisation      |
| [Output](output.md)                  | `AudioOut`, `AudioIn`, `HostTransport`                                                                    | Audio I/O and host transport             |
