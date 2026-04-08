
/// Middle C0 frequency in Hz (MIDI note 0), used as the V/OCT reference pitch.
/// V/OCT oscillators add the user-supplied `frequency_offset` to this value,
/// so a `frequency_offset` of `0.0` places the oscillator at C0 (≈ 16.35 Hz)
/// before any V/OCT input is applied.
pub const C0_FREQ: f32 = 16.351_598;

/// Sentinel value used to force the exp2 cache to recompute on first use.
///
/// Chosen to be a finite value far outside any plausible modulation range,
/// avoiding NaN (which breaks `!=` comparisons if upstream ever produces NaN).
const EXP2_CACHE_SENTINEL: f32 = f32::MIN;

use crate::common::approximate::fast_exp2;

#[derive(PartialEq)]
pub enum FMMode {
    Linear,
    Exponential,
}

// ── Frequency converters ─────────────────────────────────────────────────────

/// Converts a frequency (Hz) to a normalised phase increment by multiplying
/// by the reciprocal of the sample rate.
pub struct MonoFrequencyConverter {
    sample_rate_reciprocal: f32,
}

impl MonoFrequencyConverter {
    pub fn new(sample_rate: f32) -> Self {
        Self { sample_rate_reciprocal: 1.0 / sample_rate }
    }

    pub fn to_increment(&self, frequency: f32) -> f32 {
        frequency * self.sample_rate_reciprocal
    }
}

/// Polyphonic frequency-to-increment converter.
///
/// Same maths as [`MonoFrequencyConverter`] but provides a convenience method
/// that operates on a full 16-voice array.
pub struct PolyFrequencyConverter {
    sample_rate_reciprocal: f32,
}

impl PolyFrequencyConverter {
    pub fn new(sample_rate: f32) -> Self {
        Self { sample_rate_reciprocal: f32::recip(sample_rate) }
    }

    pub fn to_increment(&self, frequency: f32) -> f32 {
        frequency * self.sample_rate_reciprocal
    }

    /// Convert a per-voice frequency array to a per-voice increment array.
    pub fn all_to_increments(&self, frequencies: &[f32; 16]) -> [f32; 16] {
        let r = self.sample_rate_reciprocal;
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            out[i] = frequencies[i] * r;
        }
        out
    }
}

// ── Frequency change trackers ────────────────────────────────────────────────

/// Caches previous modulation inputs and computes modulated frequencies for a
/// single mono channel.
///
/// Stores the reference frequency, V/OCT offset, FM mode, and connectivity
/// flags. Caches the `exp2` result for exponential modulation to avoid
/// recomputing it when the combined exponent has not changed.
///
/// The `voct_offset` is expressed in octaves relative to the reference pitch:
/// `0.0` leaves the reference unchanged, `1.0` raises it by one octave, etc.
pub struct MonoFrequencyChangeTracker {
    reference_frequency: f32,
    voct_offset: f32,
    /// Cached result of `reference_frequency * 2^voct_offset`; updated in `set_voct_offset`.
    cached_base_frequency: f32,
    pub voct_modulating: bool,
    pub fm_modulating: bool,
    pub fm_mode: FMMode,
    last_exp_mod: f32,
    cached_exp2: f32,
}

impl MonoFrequencyChangeTracker {
    pub fn new(reference_frequency: f32) -> Self {
        Self {
            reference_frequency,
            voct_offset: 0.0,
            cached_base_frequency: reference_frequency,
            voct_modulating: false,
            fm_modulating: false,
            fm_mode: FMMode::Linear,
            last_exp_mod: EXP2_CACHE_SENTINEL,
            cached_exp2: 1.0,
        }
    }

    /// Set the static V/OCT offset (in octaves) applied before any CV modulation.
    pub fn set_voct_offset(&mut self, offset: f32) {
        self.voct_offset = offset;
        self.cached_base_frequency = self.reference_frequency * fast_exp2(offset);
    }

    pub fn set_fm_mode(&mut self, mode: FMMode) {
        self.fm_mode = mode;
    }

    pub fn is_modulating(&self) -> bool {
        self.voct_modulating || self.fm_modulating
    }

    /// The unmodulated base frequency: `reference × 2^voct_offset`.
    pub fn base_frequency(&self) -> f32 {
        self.cached_base_frequency
    }

    /// Compute the modulated frequency for this sample.
    pub fn compute_modulated(&mut self, voct: f32, fm: f32) -> f32 {
        let mut frequency = self.cached_base_frequency;
        let mut exp_mod = 0.0f32;
        if self.voct_modulating {
            exp_mod = voct;
        }
        if self.fm_modulating && self.fm_mode == FMMode::Exponential {
            exp_mod += fm;
        }
        if exp_mod != 0.0 {
            if exp_mod != self.last_exp_mod {
                self.cached_exp2 = fast_exp2(exp_mod);
                self.last_exp_mod = exp_mod;
            }
            frequency *= self.cached_exp2;
        }
        if self.fm_modulating && self.fm_mode == FMMode::Linear {
            frequency += fm * 10.0;
        }
        frequency
    }
}

/// Caches previous modulation inputs and computes modulated frequencies for
/// up to 16 polyphonic voices independently.
///
/// Each voice has its own `exp2` cache; the reference frequency and FM flags
/// are shared across all voices.
///
/// The `voct_offset` is expressed in octaves relative to the reference pitch:
/// `0.0` leaves the reference unchanged, `1.0` raises it by one octave, etc.
pub struct PolyFrequencyChangeTracker {
    reference_frequency: f32,
    voct_offset: f32,
    /// Cached result of `reference_frequency * 2^voct_offset`; updated in `set_voct_offset`.
    cached_base_frequency: f32,
    pub voct_modulating: bool,
    pub fm_modulating: bool,
    pub fm_mode: FMMode,
    last_exp_mod: [f32; 16],
    cached_exp2: [f32; 16],
}

impl PolyFrequencyChangeTracker {
    pub fn new(reference_frequency: f32) -> Self {
        Self {
            reference_frequency,
            voct_offset: 0.0,
            cached_base_frequency: reference_frequency,
            voct_modulating: false,
            fm_modulating: false,
            fm_mode: FMMode::Linear,
            last_exp_mod: [EXP2_CACHE_SENTINEL; 16],
            cached_exp2: [1.0; 16],
        }
    }

    /// Set the static V/OCT offset (in octaves) applied before any CV modulation.
    pub fn set_voct_offset(&mut self, offset: f32) {
        self.voct_offset = offset;
        self.cached_base_frequency = self.reference_frequency * fast_exp2(offset);
    }

    pub fn set_fm_mode(&mut self, mode: FMMode) {
        self.fm_mode = mode;
    }

    pub fn is_modulating(&self) -> bool {
        self.voct_modulating || self.fm_modulating
    }

    /// The unmodulated base frequency shared by all voices: `reference × 2^voct_offset`.
    pub fn base_frequency(&self) -> f32 {
        self.cached_base_frequency
    }

    /// Compute the modulated frequency for a single voice.
    pub fn compute_modulated(&mut self, voice: usize, voct: f32, fm: f32) -> f32 {
        let mut frequency = self.cached_base_frequency;
        let mut exp_mod = 0.0f32;
        if self.voct_modulating {
            exp_mod = voct;
        }
        if self.fm_modulating && self.fm_mode == FMMode::Exponential {
            exp_mod += fm;
        }
        if exp_mod != 0.0 {
            if exp_mod != self.last_exp_mod[voice] {
                self.cached_exp2[voice] = fast_exp2(exp_mod);
                self.last_exp_mod[voice] = exp_mod;
            }
            frequency *= self.cached_exp2[voice];
        }
        if self.fm_modulating && self.fm_mode == FMMode::Linear {
            frequency += fm * 10.0;
        }
        frequency
    }

    /// Compute modulated frequencies for all 16 voices.
    pub fn compute_all(&mut self, voct: &[f32; 16], fm: &[f32; 16]) -> [f32; 16] {
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            out[i] = self.compute_modulated(i, voct[i], fm[i]);
        }
        out

    }
}
