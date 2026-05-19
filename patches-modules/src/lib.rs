pub mod host;
pub mod console;
pub mod filter;
pub mod midi;
pub mod modulators;
pub mod osc;
pub mod poly_filter;
pub mod stereo;
pub mod common;

pub mod svf;
pub mod poly_svf;
pub mod reverb;
pub mod delay;
pub mod detectors;
pub mod dynamics;
pub mod sequencer;
pub mod effects;
pub mod utils;
pub mod primitives;

pub use console::Console;
pub use host::{AudioIn, AudioOut, Clock, HostControl, HostControlKind, HostTransport, MsTicker, TempoSync};
pub use filter::ResonantLowpass;
pub use filter::ResonantHighpass;
pub use filter::ResonantBandpass;
pub use poly_filter::PolyResonantLowpass;
pub use poly_filter::PolyResonantHighpass;
pub use poly_filter::PolyResonantBandpass;
pub use midi::{
    MidiArp, MidiCc, MidiDelay, MidiDrumset, MidiIn, MidiSplit, MidiToCv, MidiTranspose,
    PolyMidiToCv,
};
pub use modulators::{
    Adsr, Glide, Lfo, Op, PolyAdsr, PolyLfo, PolyOp, PolyQuant, PolySah, PolyTuner, Quant, RingMod,
    Sah, Tuner,
};
pub use osc::{Noise, Oscillator, PolyNoise, PolyOsc};
pub use stereo::{
    Balance, MidSide, MonoBass, Pan, StereoJoiner, StereoSplitter, StereoSum, StereoWidth,
};
pub use utils::{MonoToPoly, PolySum, PolyToMono, PolyVca, Sum, Tap, TapKind, Vca};
pub use svf::Svf;
pub use poly_svf::PolySvf;
pub use reverb::FdnReverb;
pub use delay::{Delay, StereoDelay};
pub use detectors::{
    AudioToGate, AudioToTrigger, EdgeDirectionParam, PolyAudioToGate, PolyAudioToTrigger,
    SyncToTrigger, TriggerToSync,
};
pub use dynamics::{
    CompDetectMode, Compressor, Gate, Limiter, StereoCompressor, StereoGate, StereoLimiter,
    TransientShaper,
};
pub use sequencer::{MasterSequencer, PatternPlayer};
pub use effects::{Bitcrusher, Drive};
pub use primitives::{Comb, CombMode, DcBlocker};

pub fn default_registry() -> patches_core::registry::Registry {
    let mut r = patches_core::registry::Registry::new();
    r.register::<Oscillator>();
    r.register::<Sum>();
    r.register::<Vca>();
    r.register::<AudioIn>();
    r.register::<AudioOut>();
    r.register::<Adsr>();
    r.register::<Clock>();
    r.register::<Glide>();
    r.register::<Lfo>();
    r.register::<PolyLfo>();
    r.register::<ResonantLowpass>();
    r.register::<ResonantHighpass>();
    r.register::<ResonantBandpass>();
    r.register::<PolyResonantLowpass>();
    r.register::<PolyResonantHighpass>();
    r.register::<PolyResonantBandpass>();
    r.register::<Tuner>();
    r.register::<PolyTuner>();
    r.register::<MidiCc>();
    r.register::<MidiToCv>();
    r.register::<PolyMidiToCv>();
    r.register::<MidiIn>();
    r.register::<MidiSplit>();
    r.register::<MidiTranspose>();
    r.register::<MidiArp>();
    r.register::<MidiDelay>();
    r.register::<Op>();
    r.register::<PolyOp>();
    r.register::<PolyOsc>();
    r.register::<PolyAdsr>();
    r.register::<PolyVca>();
    r.register::<PolySum>();
    r.register::<StereoSum>();
    r.register::<Pan>();
    r.register::<Balance>();
    r.register::<StereoWidth>();
    r.register::<MidSide>();
    r.register::<MonoBass>();
    r.register::<PolyToMono>();
    r.register::<MonoToPoly>();
    r.register::<Console>();
    r.register::<Noise>();
    r.register::<PolyNoise>();
    r.register::<RingMod>();
    r.register::<Svf>();
    r.register::<PolySvf>();
    r.register::<FdnReverb>();
    r.register::<Delay>();
    r.register::<StereoDelay>();
    r.register::<StereoSplitter>();
    r.register::<StereoJoiner>();
    r.register::<Sah>();
    r.register::<PolySah>();
    r.register::<Quant>();
    r.register::<PolyQuant>();
    r.register::<Limiter>();
    r.register::<StereoLimiter>();
    r.register::<Compressor>();
    r.register::<StereoCompressor>();
    r.register::<Gate>();
    r.register::<StereoGate>();
    r.register::<AudioToTrigger>();
    r.register::<PolyAudioToTrigger>();
    r.register::<AudioToGate>();
    r.register::<PolyAudioToGate>();
    r.register::<MasterSequencer>();
    r.register::<PatternPlayer>();
    r.register::<MidiDrumset>();
    r.register::<Bitcrusher>();
    r.register::<Drive>();
    r.register::<TransientShaper>();
    r.register::<HostTransport>();
    r.register::<TempoSync>();
    r.register::<MsTicker>();
    r.register::<TriggerToSync>();
    r.register::<SyncToTrigger>();
    r.register::<Tap>();
    r.register::<HostControl>();
    r.register::<DcBlocker>();
    r.register::<Comb>();
    // The stdlib bundles (patches-vintage, patches-drums,
    // patches-fft-bundle — all shipped from the patches-bundles
    // repo) are not in the default registry. Hosts load their
    // cdylibs via `PluginScanner` — see
    // `patches_ffi::scanner::stdlib_scanner` (ticket 0876 /
    // ADR 0073) for the default search-path policy.
    r
}

#[cfg(test)]
mod tests {
    use patches_core::{AudioEnvironment, InstanceId, ModuleShape};
    use patches_core::parameter_map::ParameterMap;

    #[test]
    fn default_registry_contains_all_modules() {
        let r = super::default_registry();
        let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let shape = ModuleShape { channels: 2 };
        let params = ParameterMap::new();

        for name in &[
            "Osc",
            "Sum",
            "Vca",
            "AudioIn",
            "AudioOut",
            "Adsr",
            "Clock",
            "Glide",
            "Lfo",
            "PolyLfo",
            "Lowpass",
            "Highpass",
            "Bandpass",
            "Tuner",
            "PolyTuner",
            "MidiToCv",
            "PolyMidiToCv",
            "Op",
            "PolyOp",
            "PolyOsc",
            "PolyAdsr",
            "PolyVca",
            "PolySum",
            "StereoSum",
            "Pan",
            "Balance",
            "StereoWidth",
            "MidSide",
            "MonoBass",
            "PolyToMono",
            "MonoToPoly",
            "PolyLowpass",
            "PolyHighpass",
            "PolyBandpass",
            "Console",
            "RingMod",
            "Noise",
            "PolyNoise",
            "Svf",
            "PolySvf",
            "FdnReverb",
            "Delay",
            "StereoDelay",
            "Sah",
            "PolySah",
            "Quant",
            "PolyQuant",
            "Limiter",
            "Compressor",
            "StereoCompressor",
            "Gate",
            "StereoGate",
            "AudioToTrigger",
            "PolyAudioToTrigger",
            "AudioToGate",
            "PolyAudioToGate",
            "MasterSequencer",
            "PatternPlayer",
            "MidiDrumset",
            "MidiSplit",
            "MidiTranspose",
            "MidiArp",
            "MidiDelay",
            "Bitcrusher",
            "Drive",
            "TransientShaper",
            "TempoSync",
            "MsTicker",
            "DcBlocker",
            "Comb",
        ] {
            assert!(
                r.create(name, &env, &shape, &params, &patches_core::StructuralParams::new(), InstanceId::next()).is_ok(),
                "default_registry() missing module: {name}",
            );
        }
    }
}
