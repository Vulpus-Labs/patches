pub mod adsr;
pub mod audio_in;
pub mod mixer;
pub mod mono_to_poly;
pub mod audio_out;
pub mod clock;
pub mod filter;
pub mod midi_cc;
pub mod midi_in;
pub mod oscillator;
pub mod poly_adsr;
pub mod poly_filter;
pub mod poly_midi_in;
pub mod poly_sum;
pub mod poly_osc;
pub mod poly_to_mono;
pub mod poly_vca;
pub mod sum;
pub mod vca;
pub mod glide;
pub mod lfo;
pub mod tuner;
pub mod common;
pub mod noise;
pub mod ring_mod;
pub mod svf;
pub mod poly_svf;
pub mod fdn_reverb;
pub mod delay;
pub mod stereo_delay;
pub mod sah;
pub mod poly_sah;
pub mod poly_tuner;
pub mod quant_util;
pub mod quant;
pub mod poly_quant;
pub mod limiter;
pub mod stereo_limiter;
pub mod pitch_shift;
pub mod convolution_reverb;
pub mod master_sequencer;
pub mod pattern_player;
pub mod kick;
pub mod snare;
pub mod clap_drum;
pub mod hihat;
pub mod tom;
pub mod claves;
pub mod cymbal;
pub mod midi_drumset;
pub mod bitcrusher;
pub mod drive;
pub mod transient_shaper;
pub mod host_transport;
pub mod tempo_sync;
pub mod ms_ticker;

pub use adsr::Adsr;
pub use mixer::{Mixer, StereoMixer, PolyMixer, StereoPolyMixer};
pub use audio_in::AudioIn;
pub use audio_out::AudioOut;
pub use clock::Clock;
pub use filter::ResonantLowpass;
pub use filter::ResonantHighpass;
pub use filter::ResonantBandpass;
pub use poly_filter::PolyResonantLowpass;
pub use poly_filter::PolyResonantHighpass;
pub use poly_filter::PolyResonantBandpass;
pub use mono_to_poly::MonoToPoly;
pub use midi_cc::MidiCc;
pub use midi_in::MonoMidiIn;
pub use oscillator::Oscillator;
pub use poly_adsr::PolyAdsr;
pub use poly_midi_in::PolyMidiIn;
pub use poly_sum::PolySum;
pub use poly_osc::PolyOsc;
pub use poly_to_mono::PolyToMono;
pub use poly_vca::PolyVca;
pub use sum::Sum;
pub use vca::Vca;
pub use glide::Glide;
pub use lfo::Lfo;
pub use tuner::Tuner;
pub use poly_tuner::PolyTuner;
pub use noise::{Noise, PolyNoise};
pub use ring_mod::RingMod;
pub use svf::Svf;
pub use poly_svf::PolySvf;
pub use fdn_reverb::FdnReverb;
pub use delay::Delay;
pub use stereo_delay::StereoDelay;
pub use sah::Sah;
pub use poly_sah::PolySah;
pub use quant::Quant;
pub use poly_quant::PolyQuant;
pub use limiter::Limiter;
pub use stereo_limiter::StereoLimiter;
pub use pitch_shift::PitchShift;
pub use convolution_reverb::ConvolutionReverb;
pub use convolution_reverb::StereoConvReverb;
pub use master_sequencer::MasterSequencer;
pub use pattern_player::PatternPlayer;
pub use kick::Kick;
pub use snare::Snare;
pub use clap_drum::ClapDrum;
pub use hihat::{ClosedHiHat, OpenHiHat};
pub use tom::Tom;
pub use claves::Claves;
pub use cymbal::Cymbal;
pub use midi_drumset::MidiDrumset;
pub use bitcrusher::Bitcrusher;
pub use drive::Drive;
pub use transient_shaper::TransientShaper;
pub use host_transport::HostTransport;
pub use tempo_sync::TempoSync;
pub use ms_ticker::MsTicker;

pub fn default_registry() -> patches_registry::Registry {
    let mut r = patches_registry::Registry::new();
    r.register::<Oscillator>();
    r.register::<Sum>();
    r.register::<Vca>();
    r.register::<AudioIn>();
    r.register::<AudioOut>();
    r.register::<Adsr>();
    r.register::<Clock>();
    r.register::<Glide>();
    r.register::<Lfo>();
    r.register::<ResonantLowpass>();
    r.register::<ResonantHighpass>();
    r.register::<ResonantBandpass>();
    r.register::<PolyResonantLowpass>();
    r.register::<PolyResonantHighpass>();
    r.register::<PolyResonantBandpass>();
    r.register::<Tuner>();
    r.register::<PolyTuner>();
    r.register::<MidiCc>();
    r.register::<MonoMidiIn>();
    r.register::<PolyMidiIn>();
    r.register::<PolyOsc>();
    r.register::<PolyAdsr>();
    r.register::<PolyVca>();
    r.register::<PolySum>();
    r.register::<PolyToMono>();
    r.register::<MonoToPoly>();
    r.register::<Mixer>();
    r.register::<StereoMixer>();
    r.register::<PolyMixer>();
    r.register::<StereoPolyMixer>();
    r.register::<Noise>();
    r.register::<PolyNoise>();
    r.register::<RingMod>();
    r.register::<Svf>();
    r.register::<PolySvf>();
    r.register::<FdnReverb>();
    r.register::<Delay>();
    r.register::<StereoDelay>();
    r.register::<Sah>();
    r.register::<PolySah>();
    r.register::<Quant>();
    r.register::<PolyQuant>();
    r.register::<Limiter>();
    r.register::<StereoLimiter>();
    r.register::<PitchShift>();
    r.register::<ConvolutionReverb>();
    r.register_file_processor::<ConvolutionReverb>();
    r.register::<StereoConvReverb>();
    r.register_file_processor::<StereoConvReverb>();
    r.register::<MasterSequencer>();
    r.register::<PatternPlayer>();
    r.register::<Kick>();
    r.register::<Snare>();
    r.register::<ClapDrum>();
    r.register::<ClosedHiHat>();
    r.register::<OpenHiHat>();
    r.register::<Tom>();
    r.register::<Claves>();
    r.register::<Cymbal>();
    r.register::<MidiDrumset>();
    r.register::<Bitcrusher>();
    r.register::<Drive>();
    r.register::<TransientShaper>();
    r.register::<HostTransport>();
    r.register::<TempoSync>();
    r.register::<MsTicker>();
    // `patches-vintage` is no longer in the default registry (ADR 0045 Spike 8
    // Phase C / ticket 0570). Load its cdylib via `PluginScanner`.
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
        let shape = ModuleShape { channels: 2, length: 0, ..Default::default() };
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
            "Lowpass",
            "Highpass",
            "Bandpass",
            "Tuner",
            "PolyTuner",
            "MidiIn",
            "PolyMidiIn",
            "PolyOsc",
            "PolyAdsr",
            "PolyVca",
            "PolySum",
            "PolyToMono",
            "MonoToPoly",
            "PolyLowpass",
            "PolyHighpass",
            "PolyBandpass",
            "Mixer",
            "StereoMixer",
            "PolyMixer",
            "StereoPolyMixer",
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
            "PitchShift",
            "ConvReverb",
            "StereoConvReverb",
            "MasterSequencer",
            "PatternPlayer",
            "Kick",
            "Snare",
            "Clap",
            "ClosedHiHat",
            "OpenHiHat",
            "Tom",
            "Claves",
            "Cymbal",
            "MidiDrumset",
            "Bitcrusher",
            "Drive",
            "TransientShaper",
            "TempoSync",
            "MsTicker",
        ] {
            assert!(
                r.create(name, &env, &shape, &params, InstanceId::next()).is_ok(),
                "default_registry() missing module: {name}",
            );
        }
    }
}
