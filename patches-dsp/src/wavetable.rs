use std::sync::LazyLock;

const TABLE_SIZE: usize = 2048;
const TABLE_SIZE_F: f32 = TABLE_SIZE as f32;
const TABLE_SIZE_MASK: usize = TABLE_SIZE - 1;

/// 2048-point linearly-interpolated sine wavetable.
///
/// Provides `mono_lookup` (single voice) and `poly_lookup` (16-voice SIMD-friendly)
/// variants.  Phase is normalised to [0, 1) for both.
pub struct SineTable {
    table: [f32; TABLE_SIZE],
}

impl Default for SineTable {
    fn default() -> Self {
        let mut table = [0.0; TABLE_SIZE];
        for (i, entry) in table.iter_mut().enumerate() {
            *entry = (i as f32 / TABLE_SIZE_F * std::f32::consts::TAU).sin();
        }
        Self { table }
    }
}

impl SineTable {
    /// Look up sin(2π·phase) with linear interpolation.  `phase` must be in [0, 1).
    pub fn mono_lookup(&self, phase: f32) -> f32 {
        let idx_f = phase * TABLE_SIZE_F;
        let idx_int = idx_f as usize;
        let frac = idx_f - (idx_int as f32);

        let a = self.table[idx_int & TABLE_SIZE_MASK];
        let b = self.table[(idx_int + 1) & TABLE_SIZE_MASK];
        a + frac * (b - a)
    }

    /// Look up sin(2π·phase) for each of 16 voices.  Each `phase` must be in [0, 1).
    pub fn poly_lookup(&self, phases: [f32; 16]) -> [f32; 16] {
        let mut out = [0.0; 16];
        for i in 0..16 {
            out[i] = self.mono_lookup(phases[i]);
        }
        out
    }
}

/// Process-lifetime singleton for the 2048-point sine wavetable.
pub static SINE_TABLE: LazyLock<SineTable> = LazyLock::new(SineTable::default);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;

    fn table() -> SineTable {
        SineTable::default()
    }

    // --- mono_lookup tests ---

    #[test]
    fn mono_lookup_key_points() {
        let t = table();
        let cases: &[(f32, f32, f32, &str)] = &[
            (0.0,  0.0,  1e-4, "sin(0) = 0"),
            (0.25, 1.0,  1e-3, "sin(pi/2) = 1"),
            (0.5,  0.0,  1e-3, "sin(pi) = 0"),
            (0.75, -1.0, 1e-3, "sin(3pi/2) = -1"),
        ];
        for &(phase, expected, tol, label) in cases {
            let v = t.mono_lookup(phase);
            assert_within!(expected, v, tol, "phase={phase} ({label}): expected {expected}, got {v}");
        }
    }

    #[test]
    fn mono_interpolates_smoothly() {
        // The lookup is linearly interpolated; compare against f32::sin directly.
        let t = table();
        for i in 0..100u32 {
            let phase = i as f32 / 100.0;
            let expected = (phase * std::f32::consts::TAU).sin();
            let got = t.mono_lookup(phase);
            assert_within!(expected, got, 2e-4, "phase={phase}: expected {expected}, got {got}");
        }
    }

    // --- poly_lookup tests ---

    #[test]
    fn poly_matches_mono_for_each_lane() {
        let t = table();
        // 16 evenly-spaced phases covering [0, 1)
        let phases: [f32; 16] = std::array::from_fn(|i| i as f32 / 16.0);
        let poly_out = t.poly_lookup(phases);
        for (&ph, &pv) in phases.iter().zip(poly_out.iter()) {
            let mono = t.mono_lookup(ph);
            assert_within!(mono, pv, 1e-5, "poly lane mismatch: mono({ph})={mono}, poly={pv}");
        }
    }

    #[test]
    fn poly_known_phases() {
        let t = table();
        let mut phases = [0.0_f32; 16];
        phases[0] = 0.0;   // sin ≈ 0
        phases[4] = 0.25;  // sin ≈ +1
        phases[8] = 0.5;   // sin ≈ 0
        phases[12] = 0.75; // sin ≈ -1
        let out = t.poly_lookup(phases);

        assert_within!(0.0, out[0], 1e-4);
        assert_within!(1.0, out[4], 1e-3);
        assert_within!(0.0, out[8], 1e-3);
        assert_within!(-1.0, out[12], 1e-3);
    }

    #[test]
    fn poly_wrap_at_table_boundary() {
        // Phase very close to 1.0 — the next-index wrap must not go out of bounds.
        let t = table();
        let phase = 1.0_f32 - 1.0 / TABLE_SIZE as f32;
        let mut phases = [0.0_f32; 16];
        phases[0] = phase;
        let out = t.poly_lookup(phases);
        // Just confirm it doesn't panic and is a reasonable sine value.
        assert!(out[0].abs() <= 1.0, "sine must be in [-1,1], got {}", out[0]);
    }

    // --- SNR test ---

    #[test]
    fn wavetable_snr() {
        // T6 — Wavetable SNR: assert RMS error of SineTable::mono_lookup vs
        // f64::sin() over a dense phase sweep is within tolerance.
        //
        // Documented tolerance: RMS < 1e-4 across [0, 1).
        // A 2048-point linearly-interpolated table achieves ~5e-8 RMS in practice.
        let t = table();
        let steps = 100_000u32;
        let mut sum_sq_err = 0.0f64;

        for i in 0..steps {
            let phase = i as f32 / steps as f32;
            let approx = t.mono_lookup(phase) as f64;
            let exact = (phase as f64 * std::f64::consts::TAU).sin();
            let err = approx - exact;
            sum_sq_err += err * err;
        }

        let rms = (sum_sq_err / steps as f64).sqrt();
        println!("SineTable RMS error: {rms:.2e}");
        // Documented tolerance: RMS < 1e-4 over full period.
        assert!(rms < 1e-4, "SineTable RMS error {rms:.2e} exceeds 1e-4");
    }
}
