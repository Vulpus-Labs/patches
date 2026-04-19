use patches_core::{
    AudioEnvironment, CablePool, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoOutput, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Generates bar, beat, quaver, and semiquaver trigger pulses from a configurable BPM.
///
/// All four outputs are derived from a single beat-phase accumulator, keeping them
/// perfectly phase-locked. Outputs are 1.0 on the one sample at each boundary and
/// 0.0 on all other samples. Supports both simple time signatures
/// (quavers_per_beat=2) and compound (quavers_per_beat=3).
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `bar` | mono | 1.0 trigger pulse at each bar boundary |
/// | `beat` | mono | 1.0 trigger pulse at each beat boundary |
/// | `quaver` | mono | 1.0 trigger pulse at each quaver (eighth-note) boundary |
/// | `semiquaver` | mono | 1.0 trigger pulse at each semiquaver (sixteenth-note) boundary |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `bpm` | float | 1.0–300.0 | `120.0` | Tempo in beats per minute |
/// | `beats_per_bar` | int | 1–16 | `4` | Number of beats per bar |
/// | `quavers_per_beat` | int | 1–4 | `2` | Quavers per beat (2 = simple, 3 = compound) |
pub struct Clock {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    bpm: f32,
    beats_per_bar: u32,
    quavers_per_beat: u32,
    /// beat_phase increment per sample: bpm / (60.0 * sample_rate)
    beat_phase_delta: f32,
    /// Beat phase in [0.0, 1.0); incremented each sample
    beat_phase: f32,
    /// Number of beats that have completed (for bar boundary detection)
    beat_count: u32,
    // Output port fields
    out_bar: MonoOutput,
    out_beat: MonoOutput,
    out_quaver: MonoOutput,
    out_semiquaver: MonoOutput,
}

impl Module for Clock {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Clock", shape.clone())
            .mono_out("bar")
            .mono_out("beat")
            .mono_out("quaver")
            .mono_out("semiquaver")
            .float_param("bpm", 1.0, 300.0, 120.0)
            .int_param("beats_per_bar", 1, 16, 4)
            .int_param("quavers_per_beat", 1, 4, 2)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            bpm: 0.0,
            beats_per_bar: 0,
            quavers_per_beat: 0,
            beat_phase_delta: 0.0,
            beat_phase: 0.0,
            beat_count: 0,
            out_bar: MonoOutput::default(),
            out_beat: MonoOutput::default(),
            out_quaver: MonoOutput::default(),
            out_semiquaver: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("bpm") {
            self.bpm = *v;
            self.beat_phase_delta = self.bpm / (60.0 * self.sample_rate);
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("beats_per_bar") {
            self.beats_per_bar = *v as u32;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("quavers_per_beat") {
            self.quavers_per_beat = *v as u32;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[patches_core::InputPort], outputs: &[OutputPort]) {
        self.out_bar = MonoOutput::from_ports(outputs, 0);
        self.out_beat = MonoOutput::from_ports(outputs, 1);
        self.out_quaver = MonoOutput::from_ports(outputs, 2);
        self.out_semiquaver = MonoOutput::from_ports(outputs, 3);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Record old phase before increment
        let old_phase = self.beat_phase;

        // Increment beat phase
        self.beat_phase += self.beat_phase_delta;

        let mut bar_fired = false;
        let beat_fired = if self.beat_phase >= 1.0 {
            self.beat_phase -= 1.0;
            self.beat_count = self.beat_count.wrapping_add(1);

            // Check for bar boundary
            if self.beat_count.is_multiple_of(self.beats_per_bar) {
                bar_fired = true;
            }
            true
        } else {
            false
        };

        let new_phase = self.beat_phase;

        // Phase-to-bucket helper: clamp to [0, buckets) before casting to
        // avoid NaN/infinity producing bogus u64 values.
        let bucket = |phase: f32, buckets: u32| -> u64 {
            let raw = phase * buckets as f32;
            if raw > 0.0 { (raw as u64).min(buckets as u64 - 1) } else { 0 }
        };

        // Check for quaver boundary (1/quavers_per_beat of a beat)
        let quaver_buckets = self.quavers_per_beat;
        let old_quaver_bucket = bucket(old_phase, quaver_buckets);
        let new_quaver_bucket = bucket(new_phase, quaver_buckets);
        let quaver_fired = new_quaver_bucket > old_quaver_bucket || beat_fired;

        // Check for semiquaver boundary (half of a quaver)
        let semiquaver_buckets = self.quavers_per_beat * 2;
        let old_semiquaver_bucket = bucket(old_phase, semiquaver_buckets);
        let new_semiquaver_bucket = bucket(new_phase, semiquaver_buckets);
        let semiquaver_fired = new_semiquaver_bucket > old_semiquaver_bucket || beat_fired;

        pool.write_mono(&self.out_bar, if bar_fired { 1.0 } else { 0.0 });
        pool.write_mono(&self.out_beat, if beat_fired { 1.0 } else { 0.0 });
        pool.write_mono(&self.out_quaver, if quaver_fired { 1.0 } else { 0.0 });
        pool.write_mono(&self.out_semiquaver, if semiquaver_fired { 1.0 } else { 0.0 });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{ModuleHarness, params};

    fn make_clock_sr(bpm: f32, beats_per_bar: i64, quavers_per_beat: i64, sample_rate: f32) -> ModuleHarness {
        ModuleHarness::build_with_env::<Clock>(
            params!["bpm" => bpm, "beats_per_bar" => beats_per_bar, "quavers_per_beat" => quavers_per_beat],
            AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        )
    }

    fn make_clock(bpm: f32, beats_per_bar: i64, quavers_per_beat: i64) -> ModuleHarness {
        make_clock_sr(bpm, beats_per_bar, quavers_per_beat, 44100.0)
    }

    #[test]
    fn four_four_time_4bpm_sample_rate_1() {
        // 4/4 time at 4 BPM with sample rate 1 Hz.
        // At 4 BPM, a beat occurs every 60/4 = 15 seconds.
        // With sample_rate = 1, that's every 15 samples.
        // In 4/4, a bar has 4 beats, so bar fires every 60 samples.
        let mut h = make_clock_sr(4.0, 4, 2, 1.0);
        let mut beat_count = 0;
        let mut bar_count = 0;

        // Process 64 samples and count pulses
        for _ in 0..64 {
            h.tick();
            if h.read_mono("beat") > 0.5 { beat_count += 1; }
            if h.read_mono("bar")  > 0.5 { bar_count += 1; }
        }

        // In 64 samples at 4 BPM / 1 Hz:
        // beat_phase increments by 4/60 per sample
        // Beat fires when beat_phase wraps (every 15 samples)
        // 64 / 15 ≈ 4.26, so 4 beats and 0 bars (bar fires on 4th beat, which is at 60 samples)
        assert_eq!(beat_count, 4, "expected 4 beats in 64 samples at 4 BPM");
        assert_eq!(bar_count, 1, "expected 1 bar (fires with 4th beat) in 64 samples");
    }

    #[test]
    fn six_eight_time_120bpm() {
        // 6/8 time (6 beats per bar, compound with quavers_per_beat=3)
        // At 120 BPM with sample_rate 44100:
        // A beat completes every 22050 samples; 6 beats per bar.
        let mut h = make_clock(120.0, 6, 3);
        let mut beat_count = 0;
        let mut bar_count = 0;
        let mut quaver_count = 0;
        let mut semiquaver_count = 0;

        for _ in 0..150000usize {
            h.tick();
            if h.read_mono("bar")       > 0.5 { bar_count += 1; }
            if h.read_mono("beat")      > 0.5 { beat_count += 1; }
            if h.read_mono("quaver")    > 0.5 { quaver_count += 1; }
            if h.read_mono("semiquaver") > 0.5 { semiquaver_count += 1; }
        }

        // 150000 / 22050 ≈ 6.8 beats, so ~6 beats complete within the window
        assert_eq!(beat_count, 6, "expected 6 beats in 150000 samples at 120 BPM");
        assert!(bar_count > 0, "expected at least 1 bar");
        assert!(quaver_count > beat_count, "expected more quavers than beats");
        assert!(semiquaver_count > quaver_count, "expected more semiquavers than quavers");
    }

    #[test]
    fn all_outputs_initialized_to_zero() {
        let mut h = make_clock(120.0, 4, 2);
        // First few samples should not fire anything unless we're at a boundary
        for i in 0..5usize {
            h.tick();
            if i > 0 {
                assert_eq!(h.read_mono("bar"),       0.0, "bar should be 0 at sample {i}");
                assert_eq!(h.read_mono("beat"),      0.0, "beat should be 0 at sample {i}");
                assert_eq!(h.read_mono("quaver"),    0.0, "quaver should be 0 at sample {i}");
                assert_eq!(h.read_mono("semiquaver"),0.0, "semiquaver should be 0 at sample {i}");
            }
        }
    }
}
