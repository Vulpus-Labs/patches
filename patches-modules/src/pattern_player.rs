use std::sync::Arc;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoOutput, PolyInput, ModuleShape, OutputPort,
    TrackerData, ReceivesTrackerData,
};
use patches_core::parameter_map::ParameterMap;

/// A generic multi-channel step sequencer that reads a poly clock bus, steps
/// through pattern data from `TrackerData`, and outputs cv1/cv2/trigger/gate
/// signals per channel.
///
/// The PatternPlayer does not know whether its channels are notes, drums, or
/// automation. All channels produce the same four output types. The wiring in
/// the patch block determines how outputs are used.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `clock` | poly | Clock bus from MasterSequencer |
///
/// Clock bus voices:
///
/// | Voice | Signal | Description |
/// |-------|--------|-------------|
/// | 0 | pattern reset | 1.0 on first tick of a new pattern |
/// | 1 | pattern bank index | float-encoded integer (−1 = stop sentinel) |
/// | 2 | tick trigger | 1.0 on each step |
/// | 3 | tick duration | seconds per tick |
/// | 4 | step index | absolute step within pattern (0-based) |
/// | 5 | step fraction | fractional position within step (0.0..1.0) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `cv1[i]` | mono | Control voltage 1 per channel (i in 0..N−1, N = channels) |
/// | `cv2[i]` | mono | Control voltage 2 per channel (i in 0..N−1, N = channels) |
/// | `trigger[i]` | mono | Trigger per channel (i in 0..N−1, N = channels) |
/// | `gate[i]` | mono | Gate per channel (i in 0..N−1, N = channels) |
pub struct PatternPlayer {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    channels: usize,

    // Tracker data
    tracker_data: Option<Arc<TrackerData>>,

    // Per-channel state
    step_index: Vec<usize>,
    /// Current cv1 value per channel (for slides, holds current interpolated value).
    cv1: Vec<f32>,
    /// Current cv2 value per channel.
    cv2: Vec<f32>,
    /// Current gate state per channel.
    gate: Vec<bool>,
    /// Whether trigger should fire this sample.
    trigger_pending: Vec<bool>,
    /// Whether this channel is in a stopped state.
    stopped: bool,

    // Slide state per channel
    slide_active: Vec<bool>,
    slide_cv1_start: Vec<f32>,
    slide_cv1_end: Vec<f32>,
    slide_cv2_start: Vec<f32>,
    slide_cv2_end: Vec<f32>,
    slide_samples_total: Vec<f32>,
    slide_samples_elapsed: Vec<f32>,

    // Repeat state per channel
    repeat_active: Vec<bool>,
    repeat_count: Vec<u8>,
    repeat_index: Vec<u8>,
    repeat_interval_samples: Vec<f32>,
    repeat_samples_elapsed: Vec<f32>,
    /// Sample count at which the gate should drop before the next sub-trigger.
    /// Each sub-note has ~80% gate-on duty cycle so the ADSR gets a clear
    /// release-then-attack transient on each retrigger.
    repeat_gate_off_at: Vec<f32>,

    // Current tick duration in samples (for slides and repeats)
    current_tick_duration_samples: f32,

    // Previous clock trigger state for edge detection
    prev_tick_trigger: f32,

    // Current pattern bank index
    current_bank_index: Option<usize>,

    // Ports
    clock_in: PolyInput,
    cv1_out: Vec<MonoOutput>,
    cv2_out: Vec<MonoOutput>,
    trigger_out: Vec<MonoOutput>,
    gate_out: Vec<MonoOutput>,
}

impl PatternPlayer {
    fn clear_all(&mut self) {
        for i in 0..self.channels {
            self.cv1[i] = 0.0;
            self.cv2[i] = 0.0;
            self.gate[i] = false;
            self.trigger_pending[i] = false;
            self.slide_active[i] = false;
            self.repeat_active[i] = false;
            self.repeat_gate_off_at[i] = f32::MAX;
        }
        self.stopped = true;
    }

    fn apply_step(&mut self, channel: usize, bank_index: usize, step_fraction: f32) {
        let Some(ref data) = self.tracker_data else { return };
        let Some(pattern) = data.patterns.patterns.get(bank_index) else { return };

        if channel >= pattern.channels || channel >= pattern.data.len() {
            // Surplus channel: silence
            self.gate[channel] = false;
            self.trigger_pending[channel] = false;
            self.slide_active[channel] = false;
            self.repeat_active[channel] = false;
            return;
        }

        let step_idx = self.step_index[channel] % pattern.steps;
        let step = &pattern.data[channel][step_idx];

        if !step.gate {
            // Rest: gate off, no trigger
            self.gate[channel] = false;
            self.trigger_pending[channel] = false;
            self.slide_active[channel] = false;
            self.repeat_active[channel] = false;
            return;
        }

        if !step.trigger {
            // Tie: gate stays high, no trigger. If the tie carries slide
            // targets (cv1_end / cv2_end) the slide continues through the
            // tie — otherwise cv carries over unchanged. This supports
            // multi-step slides (e.g. `slide(N, a, b)` expands to N chained
            // subdivisions where only the first triggers).
            self.gate[channel] = true;
            self.trigger_pending[channel] = false;
            self.repeat_active[channel] = false;
            if step.cv1_end.is_some() || step.cv2_end.is_some() {
                let elapsed_samples = step_fraction * self.current_tick_duration_samples;
                self.slide_active[channel] = true;
                self.slide_cv1_start[channel] = step.cv1;
                self.slide_cv1_end[channel] = step.cv1_end.unwrap_or(step.cv1);
                self.slide_cv2_start[channel] = step.cv2;
                self.slide_cv2_end[channel] = step.cv2_end.unwrap_or(step.cv2);
                self.slide_samples_total[channel] = self.current_tick_duration_samples;
                self.slide_samples_elapsed[channel] = elapsed_samples;
                let t = if self.current_tick_duration_samples > 0.0 {
                    (elapsed_samples / self.current_tick_duration_samples).min(1.0)
                } else {
                    0.0
                };
                self.cv1[channel] = step.cv1 + t * (step.cv1_end.unwrap_or(step.cv1) - step.cv1);
                self.cv2[channel] = step.cv2 + t * (step.cv2_end.unwrap_or(step.cv2) - step.cv2);
            } else {
                self.slide_active[channel] = false;
            }
            return;
        }

        // Normal step with trigger
        self.gate[channel] = true;
        self.trigger_pending[channel] = true;

        // Pre-advance amount in samples for mid-step seeks.
        let elapsed_samples = step_fraction * self.current_tick_duration_samples;

        // Check for slides
        if step.cv1_end.is_some() || step.cv2_end.is_some() {
            self.slide_active[channel] = true;
            self.slide_cv1_start[channel] = step.cv1;
            self.slide_cv1_end[channel] = step.cv1_end.unwrap_or(step.cv1);
            self.slide_cv2_start[channel] = step.cv2;
            self.slide_cv2_end[channel] = step.cv2_end.unwrap_or(step.cv2);
            self.slide_samples_total[channel] = self.current_tick_duration_samples;
            self.slide_samples_elapsed[channel] = elapsed_samples;
            // Set cv to the interpolated position at step_fraction.
            let t = if self.current_tick_duration_samples > 0.0 {
                (elapsed_samples / self.current_tick_duration_samples).min(1.0)
            } else {
                0.0
            };
            self.cv1[channel] = step.cv1 + t * (step.cv1_end.unwrap_or(step.cv1) - step.cv1);
            self.cv2[channel] = step.cv2 + t * (step.cv2_end.unwrap_or(step.cv2) - step.cv2);
        } else {
            self.slide_active[channel] = false;
            self.cv1[channel] = step.cv1;
            self.cv2[channel] = step.cv2;
        }

        // Check for repeats
        if step.repeat > 1 {
            self.repeat_active[channel] = true;
            self.repeat_count[channel] = step.repeat;
            let interval =
                self.current_tick_duration_samples / step.repeat as f32;
            self.repeat_interval_samples[channel] = interval;
            self.repeat_samples_elapsed[channel] = elapsed_samples;
            // Skip past repeat pulses that have already elapsed.
            let repeat_idx = if interval > 0.0 {
                ((elapsed_samples / interval).floor() as u8 + 1).min(step.repeat)
            } else {
                1
            };
            self.repeat_index[channel] = repeat_idx;
            // Schedule gate-off relative to the most recent sub-trigger.
            let last_trigger_at = (repeat_idx.saturating_sub(1)) as f32 * interval;
            self.repeat_gate_off_at[channel] = last_trigger_at + interval * 0.8;
        } else {
            self.repeat_active[channel] = false;
            self.repeat_gate_off_at[channel] = f32::MAX;
        }
    }
}

impl Module for PatternPlayer {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("PatternPlayer", shape.clone())
            .poly_in("clock")
            .mono_out_multi("cv1", n)
            .mono_out_multi("cv2", n)
            .mono_out_multi("trigger", n)
            .mono_out_multi("gate", n)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            sample_rate: env.sample_rate,
            channels,
            tracker_data: None,
            step_index: vec![0; channels],
            cv1: vec![0.0; channels],
            cv2: vec![0.0; channels],
            gate: vec![false; channels],
            trigger_pending: vec![false; channels],
            stopped: false,
            slide_active: vec![false; channels],
            slide_cv1_start: vec![0.0; channels],
            slide_cv1_end: vec![0.0; channels],
            slide_cv2_start: vec![0.0; channels],
            slide_cv2_end: vec![0.0; channels],
            slide_samples_total: vec![0.0; channels],
            slide_samples_elapsed: vec![0.0; channels],
            repeat_active: vec![false; channels],
            repeat_count: vec![1; channels],
            repeat_index: vec![0; channels],
            repeat_interval_samples: vec![0.0; channels],
            repeat_samples_elapsed: vec![0.0; channels],
            repeat_gate_off_at: vec![f32::MAX; channels],
            current_tick_duration_samples: 0.0,
            prev_tick_trigger: 0.0,
            current_bank_index: None,
            clock_in: PolyInput::default(),
            cv1_out: vec![MonoOutput::default(); channels],
            cv2_out: vec![MonoOutput::default(); channels],
            trigger_out: vec![MonoOutput::default(); channels],
            gate_out: vec![MonoOutput::default(); channels],
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {
        // PatternPlayer has no parameters — all data comes from tracker data
        // and the clock bus.
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.channels;
        self.clock_in = PolyInput::from_ports(inputs, 0);
        for i in 0..n {
            self.cv1_out[i] = MonoOutput::from_ports(outputs, i);
            self.cv2_out[i] = MonoOutput::from_ports(outputs, n + i);
            self.trigger_out[i] = MonoOutput::from_ports(outputs, 2 * n + i);
            self.gate_out[i] = MonoOutput::from_ports(outputs, 3 * n + i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let clock = pool.read_poly(&self.clock_in);
        let bank_index_f = clock[1];
        let tick_trigger = clock[2];
        let tick_duration_secs = clock[3];

        let tick_rose = tick_trigger >= 0.5 && self.prev_tick_trigger < 0.5;
        self.prev_tick_trigger = tick_trigger;

        if tick_rose {
            // Check for stop sentinel
            if bank_index_f < 0.0 {
                self.clear_all();
            } else {
                self.stopped = false;
                let bank_index = bank_index_f.round() as usize;
                self.current_bank_index = Some(bank_index);
                self.current_tick_duration_samples = tick_duration_secs * self.sample_rate;

                // Use absolute step index from bus[4].
                let step_index = clock[4].round() as usize;
                let step_fraction = clock[5];
                for i in 0..self.channels {
                    self.step_index[i] = step_index;
                }

                // Apply step data for each channel
                for ch in 0..self.channels {
                    self.apply_step(ch, bank_index, step_fraction);
                }
            }
        } else if !self.stopped {
            // Between ticks: process slides and repeats
            for ch in 0..self.channels {
                // Clear single-sample trigger pulse
                self.trigger_pending[ch] = false;

                // Slide interpolation
                if self.slide_active[ch] {
                    self.slide_samples_elapsed[ch] += 1.0;
                    let t = if self.slide_samples_total[ch] > 0.0 {
                        (self.slide_samples_elapsed[ch] / self.slide_samples_total[ch]).min(1.0)
                    } else {
                        1.0
                    };
                    self.cv1[ch] = self.slide_cv1_start[ch]
                        + t * (self.slide_cv1_end[ch] - self.slide_cv1_start[ch]);
                    self.cv2[ch] = self.slide_cv2_start[ch]
                        + t * (self.slide_cv2_end[ch] - self.slide_cv2_start[ch]);
                }

                // Repeat sub-triggers
                if self.repeat_active[ch] && self.repeat_index[ch] < self.repeat_count[ch] {
                    self.repeat_samples_elapsed[ch] += 1.0;
                    let elapsed = self.repeat_samples_elapsed[ch];

                    // Drop gate before next sub-trigger so ADSR gets a clear
                    // release-then-attack transient.
                    if elapsed >= self.repeat_gate_off_at[ch] {
                        self.gate[ch] = false;
                    }

                    let next_trigger_at =
                        self.repeat_interval_samples[ch] * self.repeat_index[ch] as f32;
                    if elapsed >= next_trigger_at {
                        self.trigger_pending[ch] = true;
                        self.gate[ch] = true;
                        self.repeat_index[ch] += 1;
                        // Schedule gate-off for this sub-note.
                        let interval = self.repeat_interval_samples[ch];
                        self.repeat_gate_off_at[ch] = next_trigger_at + interval * 0.8;
                    }
                }
            }
        }

        // Write outputs
        for ch in 0..self.channels {
            if self.cv1_out[ch].connected {
                pool.write_mono(&self.cv1_out[ch], self.cv1[ch]);
            }
            if self.cv2_out[ch].connected {
                pool.write_mono(&self.cv2_out[ch], self.cv2[ch]);
            }
            pool.write_mono(
                &self.trigger_out[ch],
                if self.trigger_pending[ch] { 1.0 } else { 0.0 },
            );
            pool.write_mono(
                &self.gate_out[ch],
                if self.gate[ch] { 1.0 } else { 0.0 },
            );
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_tracker_data_receiver(&mut self) -> Option<&mut dyn ReceivesTrackerData> {
        Some(self)
    }
}

impl ReceivesTrackerData for PatternPlayer {
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>) {
        self.tracker_data = Some(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::{
        PatternBank, SongBank, Pattern, TrackerStep,
    };

    const SR: f32 = 44100.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn shape(channels: usize) -> ModuleShape {
        ModuleShape { channels, length: 0, ..Default::default() }
    }

    fn step(cv1: f32, cv2: f32, trigger: bool, gate: bool) -> TrackerStep {
        TrackerStep {
            cv1,
            cv2,
            trigger,
            gate,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
        }
    }

    fn slide_step(cv1: f32, cv1_end: f32, cv2: f32, cv2_end: f32) -> TrackerStep {
        TrackerStep {
            cv1,
            cv2,
            trigger: true,
            gate: true,
            cv1_end: Some(cv1_end),
            cv2_end: Some(cv2_end),
            repeat: 1,
        }
    }

    fn repeat_step(cv1: f32, repeat: u8) -> TrackerStep {
        TrackerStep {
            cv1,
            cv2: 0.0,
            trigger: true,
            gate: true,
            cv1_end: None,
            cv2_end: None,
            repeat,
        }
    }

    fn rest() -> TrackerStep {
        TrackerStep {
            cv1: 0.0,
            cv2: 0.0,
            trigger: false,
            gate: false,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
        }
    }

    fn tie() -> TrackerStep {
        TrackerStep {
            cv1: 0.0,
            cv2: 0.0,
            trigger: false,
            gate: true,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
        }
    }

    #[test]
    fn basic_step_playback() {
        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 1,
                    steps: 3,
                    data: vec![vec![
                        step(1.0, 0.5, true, true),
                        step(2.0, 0.8, true, true),
                        rest(),
                    ]],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(1);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);

        player.prev_tick_trigger = 0.0;
        player.current_tick_duration_samples = 0.125 * SR;
        player.step_index[0] = 0;
        player.apply_step(0, 0, 0.0);

        assert_eq!(player.cv1[0], 1.0, "cv1 should be 1.0 at step 0");
        assert_eq!(player.cv2[0], 0.5, "cv2 should be 0.5 at step 0");
        assert!(player.gate[0], "gate should be high at step 0");
        assert!(player.trigger_pending[0], "trigger should fire at step 0");

        // Step 1
        player.step_index[0] = 1;
        player.apply_step(0, 0, 0.0);
        assert_eq!(player.cv1[0], 2.0);
        assert_eq!(player.cv2[0], 0.8);
        assert!(player.gate[0]);
        assert!(player.trigger_pending[0]);

        // Step 2 (rest)
        player.step_index[0] = 2;
        player.apply_step(0, 0, 0.0);
        assert!(!player.gate[0], "gate should be low at rest");
        assert!(!player.trigger_pending[0], "no trigger at rest");
    }

    #[test]
    fn tie_holds_gate() {
        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 1,
                    steps: 3,
                    data: vec![vec![
                        step(3.0, 0.0, true, true),
                        tie(),
                        rest(),
                    ]],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(1);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);
        player.current_tick_duration_samples = 0.125 * SR;

        // Step 0: note
        player.step_index[0] = 0;
        player.apply_step(0, 0, 0.0);
        assert_eq!(player.cv1[0], 3.0);
        assert!(player.gate[0]);
        assert!(player.trigger_pending[0]);

        // Step 1: tie — gate stays high, no trigger, cv carries over
        player.step_index[0] = 1;
        player.apply_step(0, 0, 0.0);
        assert!(player.gate[0], "tie should keep gate high");
        assert!(!player.trigger_pending[0], "tie should not trigger");
        assert_eq!(player.cv1[0], 3.0, "cv should carry over on tie");
    }

    #[test]
    fn slide_interpolation() {
        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 1,
                    steps: 1,
                    data: vec![vec![
                        slide_step(0.0, 1.0, 0.0, 0.5),
                    ]],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(1);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);

        // Set tick duration to 100 samples for easy testing
        player.current_tick_duration_samples = 100.0;
        player.step_index[0] = 0;
        player.apply_step(0, 0, 0.0);

        assert!(player.slide_active[0], "slide should be active");
        assert_eq!(player.slide_cv1_start[0], 0.0);
        assert_eq!(player.slide_cv1_end[0], 1.0);

        // Simulate 50 samples of slide
        player.slide_samples_elapsed[0] = 50.0;
        let t = 50.0 / 100.0;
        let expected_cv1 = 0.0 + t * (1.0 - 0.0);
        let interp_cv1 = player.slide_cv1_start[0]
            + t * (player.slide_cv1_end[0] - player.slide_cv1_start[0]);
        assert!((interp_cv1 - expected_cv1).abs() < 1e-6);
        assert!((interp_cv1 - 0.5).abs() < 1e-6, "halfway through slide cv1 should be 0.5");
    }

    #[test]
    fn repeat_subdivision() {
        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 1,
                    steps: 1,
                    data: vec![vec![
                        repeat_step(1.0, 3),
                    ]],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(1);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);

        player.current_tick_duration_samples = 300.0;
        player.step_index[0] = 0;
        player.apply_step(0, 0, 0.0);

        assert!(player.repeat_active[0]);
        assert_eq!(player.repeat_count[0], 3);
        assert_eq!(player.repeat_index[0], 1); // first trigger already fired
        assert!((player.repeat_interval_samples[0] - 100.0).abs() < 1e-6);
    }

    #[test]
    fn stop_sentinel_clears_all() {
        let s = shape(2);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());

        // Set some state
        player.cv1[0] = 5.0;
        player.gate[0] = true;
        player.cv1[1] = 3.0;
        player.gate[1] = true;

        // Clear all (simulating stop sentinel)
        player.clear_all();

        assert_eq!(player.cv1[0], 0.0);
        assert_eq!(player.cv1[1], 0.0);
        assert!(!player.gate[0]);
        assert!(!player.gate[1]);
        assert!(player.stopped);
    }

    #[test]
    fn channel_count_mismatch_excess_ignored() {
        // Pattern has 2 channels, player has 1 — excess pattern channels ignored
        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 2,
                    steps: 1,
                    data: vec![
                        vec![step(1.0, 0.0, true, true)],
                        vec![step(2.0, 0.0, true, true)],
                    ],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(1); // only 1 channel
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);
        player.current_tick_duration_samples = 100.0;

        player.step_index[0] = 0;
        player.apply_step(0, 0, 0.0);
        assert_eq!(player.cv1[0], 1.0, "channel 0 should get its data");
    }

    #[test]
    fn channel_count_mismatch_surplus_silent() {
        // Pattern has 1 channel, player has 2 — surplus player channels silent
        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 1,
                    steps: 1,
                    data: vec![
                        vec![step(1.0, 0.0, true, true)],
                    ],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(2);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);
        player.current_tick_duration_samples = 100.0;

        player.step_index[0] = 0;
        player.step_index[1] = 0;
        player.apply_step(0, 0, 0.0);
        player.apply_step(1, 0, 0.0);

        assert_eq!(player.cv1[0], 1.0, "channel 0 should have data");
        assert!(!player.gate[1], "surplus channel should be silent");
    }

    #[test]
    fn repeat_via_process_produces_triggers_and_gate_cycles() {
        use patches_core::cables::{
            CableValue, InputPort, OutputPort, PolyInput, MonoOutput,
            POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
        };
        use patches_core::cable_pool::CablePool;

        let data = Arc::new(TrackerData {
            patterns: PatternBank {
                patterns: vec![Pattern {
                    channels: 1,
                    steps: 1,
                    data: vec![vec![repeat_step(1.0, 3)]],
                }],
            },
            songs: SongBank { songs: vec![] },
        });

        let s = shape(1);
        let desc = PatternPlayer::describe(&s);
        let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
        player.update_validated_parameters(&mut ParameterMap::new());
        player.receive_tracker_data(data);

        // Pool layout: reserved(16) + 1 poly input (clock) + 4 mono outputs
        // Input: slot 16 (poly clock)
        // Outputs: slot 17 (cv1), 18 (cv2), 19 (trigger), 20 (gate)
        let clock_slot = RESERVED_SLOTS;
        let trigger_slot = RESERVED_SLOTS + 1 + 2; // after 1 input + cv1 + cv2
        let gate_slot = RESERVED_SLOTS + 1 + 3;
        let pool_size = RESERVED_SLOTS + 1 + 4;

        let mut pool_buf = vec![[CableValue::Mono(0.0); 2]; pool_size];
        pool_buf[POLY_READ_SINK] = [CableValue::Poly([0.0; 16]); 2];
        pool_buf[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]); 2];
        pool_buf[clock_slot] = [CableValue::Poly([0.0; 16]); 2];

        // Wire ports
        let inputs = vec![InputPort::Poly(PolyInput {
            cable_idx: clock_slot,
            scale: 1.0,
            connected: true,
        })];
        let outputs = vec![
            OutputPort::Mono(MonoOutput { cable_idx: RESERVED_SLOTS + 1, connected: true }),
            OutputPort::Mono(MonoOutput { cable_idx: RESERVED_SLOTS + 2, connected: true }),
            OutputPort::Mono(MonoOutput { cable_idx: trigger_slot, connected: true }),
            OutputPort::Mono(MonoOutput { cable_idx: gate_slot, connected: true }),
        ];
        player.set_ports(&inputs, &outputs);

        let tick_duration_secs = 300.0 / SR; // 300 samples
        let tick_samples = 300_usize;

        // ── Sample 0: write clock with tick trigger ──────────────────────────
        let mut clock_bus = [0.0_f32; 16];
        clock_bus[0] = 1.0; // pattern reset
        clock_bus[1] = 0.0; // bank index
        clock_bus[2] = 1.0; // tick trigger
        clock_bus[3] = tick_duration_secs;

        let mut wi = 0;
        // Write clock to the READ side (wi=0 writes to slot 0, read from slot 0 when wi=1...
        // actually CablePool reads from 1-wi). Let's just write to both slots.
        pool_buf[clock_slot] = [CableValue::Poly(clock_bus); 2];

        {
            let mut cp = CablePool::new(&mut pool_buf, wi);
            player.process(&mut cp);
        }
        wi = 1 - wi;

        // Read trigger and gate from the write slot
        let read_mono = |buf: &Vec<[CableValue; 2]>, slot: usize, write_idx: usize| -> f32 {
            match buf[slot][write_idx] {
                CableValue::Mono(v) => v,
                _ => panic!("expected Mono"),
            }
        };

        let t0_trigger = read_mono(&pool_buf, trigger_slot, 1 - wi);
        let t0_gate = read_mono(&pool_buf, gate_slot, 1 - wi);
        assert_eq!(t0_trigger, 1.0, "tick_rose should fire trigger");
        assert_eq!(t0_gate, 1.0, "tick_rose should set gate high");

        // ── Run for tick_samples, counting trigger edges and gate drops ──────
        // Set clock to no trigger for remaining samples
        let mut silent_clock = [0.0_f32; 16];
        silent_clock[1] = 0.0;
        silent_clock[3] = tick_duration_secs;

        let mut trigger_rising_edges = 1; // already got one
        let mut gate_drops = 0;
        let mut prev_trigger = t0_trigger;
        let mut prev_gate = t0_gate;

        for _sample in 1..tick_samples {
            pool_buf[clock_slot] = [CableValue::Poly(silent_clock); 2];
            {
                let mut cp = CablePool::new(&mut pool_buf, wi);
                player.process(&mut cp);
            }
            wi = 1 - wi;

            let trigger = read_mono(&pool_buf, trigger_slot, 1 - wi);
            let gate = read_mono(&pool_buf, gate_slot, 1 - wi);

            if trigger >= 0.5 && prev_trigger < 0.5 {
                trigger_rising_edges += 1;
            }
            if gate < 0.5 && prev_gate >= 0.5 {
                gate_drops += 1;
            }

            prev_trigger = trigger;
            prev_gate = gate;
        }

        assert_eq!(
            trigger_rising_edges, 3,
            "expected 3 trigger edges for repeat=3, got {trigger_rising_edges}"
        );
        assert_eq!(
            gate_drops, 2,
            "expected 2 gate drops between 3 sub-notes, got {gate_drops}"
        );
    }
}
