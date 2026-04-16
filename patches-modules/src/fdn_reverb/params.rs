//! Character archetype data, parameter derivation, and buffer sizing.

use super::line::BASE_MS;
use super::matrix::LINES;

#[derive(Copy, Clone)]
pub(super) struct CharData {
    pub(super) min_scale:        f32,
    pub(super) max_scale:        f32,
    pub(super) lfo_rate_hz:      f32,
    pub(super) lfo_depth_ms:     f32,
    pub(super) max_pre_delay_ms: f32,
    pub(super) crossover_min:    f32,  // crossover Hz at brightness = 0
    pub(super) crossover_max:    f32,  // crossover Hz at brightness = 1
    pub(super) lf_hf_ratio_min:  f32,  // lf/hf RT60 ratio at brightness = 0
    pub(super) lf_hf_ratio_max:  f32,  // lf/hf RT60 ratio at brightness = 1
    pub(super) rt60_lf_min:      f32,  // RT60 (LF) at size = 0
    pub(super) rt60_lf_max:      f32,  // RT60 (LF) at size = 1
}

pub(super) const CHARS: [CharData; 5] = [
    // 0: plate
    CharData { min_scale: 0.08, max_scale: 0.40, lfo_rate_hz: 0.27, lfo_depth_ms: 0.3,
               max_pre_delay_ms: 10.0, crossover_min: 2000.0, crossover_max: 8000.0,
               lf_hf_ratio_min: 1.5, lf_hf_ratio_max: 1.1,
               rt60_lf_min: 0.3, rt60_lf_max: 1.5 },
    // 1: room
    CharData { min_scale: 0.10, max_scale: 0.80, lfo_rate_hz: 0.15, lfo_depth_ms: 0.8,
               max_pre_delay_ms: 25.0, crossover_min: 500.0, crossover_max: 2500.0,
               lf_hf_ratio_min: 3.0, lf_hf_ratio_max: 1.5,
               rt60_lf_min: 0.4, rt60_lf_max: 2.5 },
    // 2: chamber
    CharData { min_scale: 0.15, max_scale: 0.60, lfo_rate_hz: 0.20, lfo_depth_ms: 0.5,
               max_pre_delay_ms: 20.0, crossover_min: 800.0, crossover_max: 6000.0,
               lf_hf_ratio_min: 2.5, lf_hf_ratio_max: 1.2,
               rt60_lf_min: 0.3, rt60_lf_max: 2.0 },
    // 3: hall (default)
    CharData { min_scale: 0.20, max_scale: 1.20, lfo_rate_hz: 0.10, lfo_depth_ms: 1.2,
               max_pre_delay_ms: 50.0, crossover_min: 300.0, crossover_max: 2000.0,
               lf_hf_ratio_min: 5.0, lf_hf_ratio_max: 2.0,
               rt60_lf_min: 0.8, rt60_lf_max: 5.0 },
    // 4: cathedral
    CharData { min_scale: 0.40, max_scale: 2.50, lfo_rate_hz: 0.06, lfo_depth_ms: 2.0,
               max_pre_delay_ms: 80.0, crossover_min: 200.0, crossover_max: 1500.0,
               lf_hf_ratio_min: 8.0, lf_hf_ratio_max: 3.0,
               rt60_lf_min: 1.5, rt60_lf_max: 8.0 },
];

pub(super) fn char_index(name: &str) -> usize {
    match name {
        "plate"     => 0,
        "room"      => 1,
        "chamber"   => 2,
        "hall"      => 3,
        "cathedral" => 4,
        _           => 3,
    }
}

/// Returns `(delay_scale, rt60_lf, rt60_hf, crossover_hz)` from user-facing knobs.
pub(super) fn derive_params(size: f32, brightness: f32, char_idx: usize) -> (f32, f32, f32, f32) {
    let c = CHARS[char_idx];
    let scale     = c.min_scale * (c.max_scale / c.min_scale).powf(size);
    let rt60_lf   = c.rt60_lf_min + (c.rt60_lf_max - c.rt60_lf_min) * size;
    let lf_hf     = c.lf_hf_ratio_min + (c.lf_hf_ratio_max - c.lf_hf_ratio_min) * brightness;
    let rt60_hf   = rt60_lf / lf_hf.max(1.0);
    let crossover = c.crossover_min + (c.crossover_max - c.crossover_min) * brightness;
    (scale, rt60_lf, rt60_hf, crossover)
}

/// Character data pre-scaled by the sample rate.
///
/// Rebuilt in `prepare` and whenever the character parameter changes.
/// `sr_ms` (`sample_rate * 0.001`) is not stored; it is computed inline at
/// the two call sites that construct this struct.
#[derive(Copy, Clone)]
pub(super) struct ScaledCharacter {
    /// `c.lfo_depth_ms * sr_ms` — LFO modulation depth in samples.
    pub(super) lfo_depth_samp: f32,
    /// `c.max_pre_delay_ms * sr_ms` — pre-delay capacity in samples.
    pub(super) max_pre_delay_samp: f32,
    /// `BASE_MS[i] * sr_ms` — nominal delay length in samples before scale.
    pub(super) base_samps: [f32; LINES],
}

impl ScaledCharacter {
    pub(super) fn new(char_idx: usize, sample_rate: f32) -> Self {
        let sr_ms = sample_rate * 0.001;
        let c = CHARS[char_idx];
        Self {
            lfo_depth_samp:    c.lfo_depth_ms     * sr_ms,
            max_pre_delay_samp: c.max_pre_delay_ms * sr_ms,
            base_samps:        BASE_MS.map(|ms| ms * sr_ms),
        }
    }
}

/// Maximum per-line delay duration (cathedral archetype, size=1, plus LFO depth).
///
/// All archetypes' delay lines fit within this budget, so buffers allocated at
/// `prepare` time are always large enough regardless of later character changes.
// 79.3 ms * 2.50 scale + 2.0 ms LFO depth = 200.25 ms (cathedral worst case)
pub(super) const MAX_LINE_SECS: f32 = (BASE_MS[LINES - 1] * 2.50 + 2.0) / 1000.0;

/// Maximum pre-delay duration (cathedral archetype).
pub(super) const MAX_PRE_DELAY_SECS: f32 = 0.080; // 80 ms
