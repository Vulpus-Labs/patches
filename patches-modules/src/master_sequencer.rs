use std::sync::Arc;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, PolyInput, PolyOutput, ModuleShape, OutputPort,
    Song, TrackerData, ReceivesTrackerData, TransportFrame,
    GLOBAL_TRANSPORT,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Drives song playback with transport controls, swing, and a poly clock bus
/// per song channel.
///
/// The MasterSequencer reads a named song from `TrackerData` and outputs a poly
/// clock bus per song channel. Each clock bus carries four voices encoding
/// timing and pattern-selection data for downstream `PatternPlayer` modules.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `start` | mono | Rising edge resets and begins playback |
/// | `stop` | mono | Rising edge halts and resets playback |
/// | `pause` | mono | Rising edge halts playback in place |
/// | `resume` | mono | Rising edge continues from current position |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `clock[i]` | poly | Clock bus per channel (i in 0..N−1, N = channels) |
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
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `bpm` | float | 1.0–999.0 | `120.0` | Tempo in beats per minute |
/// | `rows_per_beat` | int | 1–64 | `4` | Steps per beat |
/// | `song` | song_name | — | none | Name of the song to play (resolved to index) |
/// | `loop` | bool | — | `true` | Loop at end of song |
/// | `autostart` | bool | — | `true` | Begin playback on activation |
/// | `swing` | float | 0.0–1.0 | `0.5` | Swing ratio for alternating steps |
/// | `sync` | enum | auto/free/host | `auto` | Clock source: auto selects based on hosted flag |
///
/// In `auto` mode the sequencer checks `AudioEnvironment::hosted` at
/// prepare time to select its clock source — host transport if hosted,
/// internal BPM otherwise. `free` forces the internal clock regardless;
/// `host` forces host transport regardless. In host mode the sequencer
/// reads the `GLOBAL_TRANSPORT` backplane slot directly; `bpm`,
/// `autostart`, and `swing` are ignored. When the host stops, playback
/// freezes rather than resetting.
pub struct MasterSequencer {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    channels: usize,

    // Tracker data
    tracker_data: Option<Arc<TrackerData>>,
    song_index: Option<usize>,

    // Cached parameters
    bpm: f32,
    rows_per_beat: i64,
    do_loop: bool,
    autostart: bool,
    swing: f32,

    // Host sync
    /// Cached `AudioEnvironment::hosted` flag for resolving `sync: auto`.
    hosted: bool,
    /// Whether to use host transport (resolved from `sync` param + `hosted` flag).
    use_host_transport: bool,
    /// Fixed input pointing at the GLOBAL_TRANSPORT backplane slot.
    transport_in: PolyInput,
    /// Previous host playing state for edge detection.
    prev_host_playing: f32,

    // Transport state
    state: TransportState,
    /// Current row in the song order.
    song_row: usize,
    /// Current step within the pattern at the current song row.
    pattern_step: usize,
    /// Samples remaining until the next tick.
    samples_until_tick: f32,
    /// Whether this is the very first tick after starting/restarting.
    first_tick: bool,
    /// Whether we just entered a new pattern (first tick of a new song row).
    pattern_just_reset: bool,
    /// Global step counter for swing (even/odd alternation).
    global_step: usize,
    /// Whether the song has ended (non-looping mode).
    song_ended: bool,
    /// Whether to emit the stop sentinel on this sample.
    emit_stop_sentinel: bool,
    /// Pre-allocated bank index buffer (one entry per song channel).
    bank_indices: Vec<f32>,
    /// Fractional position within the current step (0.0..1.0).
    /// Non-zero only in host sync mode when the DAW is mid-step.
    step_fraction: f32,

    // Rising-edge detection
    prev_start: f32,
    prev_stop: f32,
    prev_pause: f32,
    prev_resume: f32,

    // Ports
    in_start: MonoInput,
    in_stop: MonoInput,
    in_pause: MonoInput,
    in_resume: MonoInput,
    clock_out: Vec<PolyOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TransportState {
    Stopped,
    Playing,
    Paused,
}

impl MasterSequencer {
    fn base_tick_seconds(&self) -> f32 {
        60.0 / (self.bpm * self.rows_per_beat as f32)
    }

    fn tick_duration_seconds(&self, step: usize) -> f32 {
        let base = self.base_tick_seconds();
        if (self.swing - 0.5).abs() < f32::EPSILON {
            base
        } else if step.is_multiple_of(2) {
            2.0 * base * self.swing
        } else {
            2.0 * base * (1.0 - self.swing)
        }
    }

    fn tick_duration_samples(&self, step: usize) -> f32 {
        self.tick_duration_seconds(step) * self.sample_rate
    }

    /// Reset transport to the beginning of the song.
    fn reset_position(&mut self) {
        self.song_row = 0;
        self.pattern_step = 0;
        self.global_step = 0;
        self.first_tick = true;
        self.pattern_just_reset = true;
        self.song_ended = false;
        self.emit_stop_sentinel = false;
        self.samples_until_tick = 0.0;
    }

    /// Get the current song, if tracker data and a valid song index are set.
    fn current_song(&self) -> Option<&Song> {
        let data = self.tracker_data.as_ref()?;
        let idx = self.song_index?;
        data.songs.songs.get(idx)
    }

    /// Advance to the next step. Returns false if song has ended.
    fn advance_step(&mut self) -> bool {
        let Some(ref data) = self.tracker_data else { return false };
        let Some(idx) = self.song_index else { return false };
        let Some(song) = data.songs.songs.get(idx) else { return false };

        if song.order.is_empty() {
            return false;
        }

        // Determine the pattern length for the current row.
        let pattern_len = self.current_pattern_length();

        self.pattern_step += 1;
        self.global_step += 1;
        self.pattern_just_reset = false;

        // Check if we've finished the current pattern.
        if self.pattern_step >= pattern_len {
            self.pattern_step = 0;
            self.song_row += 1;
            self.pattern_just_reset = true;

            // Check if we've reached the end of the song.
            if self.song_row >= song.order.len() {
                if self.do_loop {
                    self.song_row = song.loop_point;
                } else {
                    self.song_ended = true;
                    self.emit_stop_sentinel = true;
                    return false;
                }
            }
        }

        true
    }

    /// Get the step count of the pattern(s) at the given song row.
    fn pattern_length_at_row(&self, row: usize) -> usize {
        let Some(ref data) = self.tracker_data else { return 0 };
        let Some(idx) = self.song_index else { return 0 };
        let Some(song) = data.songs.songs.get(idx) else { return 0 };

        if row >= song.order.len() {
            return 0;
        }

        // Find the first non-None pattern in this row and use its step count.
        for idx in song.order[row].iter().flatten() {
            if let Some(pattern) = data.patterns.patterns.get(*idx) {
                return pattern.steps;
            }
        }

        // All channels are silent — use a default of 1 to advance.
        1
    }

    /// Get the step count of the pattern(s) at the current song row.
    fn current_pattern_length(&self) -> usize {
        self.pattern_length_at_row(self.song_row)
    }

    /// Map an absolute bar number to a song row index, respecting `loop_point`.
    ///
    /// Returns `None` if the bar is past the end of a non-looping song.
    fn resolve_song_row(&self, bar: usize) -> Option<usize> {
        let song = self.current_song()?;
        let song_len = song.order.len();
        if song_len == 0 {
            return None;
        }
        if bar < song_len {
            Some(bar)
        } else if self.do_loop {
            let loop_point = song.loop_point;
            let loop_len = song_len - loop_point;
            if loop_len == 0 {
                return None;
            }
            Some(loop_point + (bar - song_len) % loop_len)
        } else {
            None
        }
    }
}

impl Module for MasterSequencer {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("MasterSequencer", shape.clone())
            .mono_in("start")
            .mono_in("stop")
            .mono_in("pause")
            .mono_in("resume")
            .poly_out_multi("clock", n)
            .float_param("bpm", 1.0, 999.0, 120.0)
            .int_param("rows_per_beat", 1, 64, 4)
            .song_name_param("song")
            .bool_param("loop", true)
            .bool_param("autostart", true)
            .float_param("swing", 0.0, 1.0, 0.5)
            .enum_param("sync", &["auto", "free", "host"], "auto")
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            sample_rate: env.sample_rate,
            channels,
            tracker_data: None,
            song_index: None,
            bpm: 120.0,
            rows_per_beat: 4,
            do_loop: true,
            autostart: true,
            swing: 0.5,
            // Default sync=auto: use host transport if hosted.
            hosted: env.hosted,
            use_host_transport: env.hosted,
            transport_in: PolyInput {
                cable_idx: GLOBAL_TRANSPORT,
                scale: 1.0,
                connected: true,
            },
            prev_host_playing: 0.0,
            state: TransportState::Stopped,
            song_row: 0,
            pattern_step: 0,
            samples_until_tick: 0.0,
            first_tick: true,
            pattern_just_reset: true,
            global_step: 0,
            song_ended: false,
            emit_stop_sentinel: false,
            bank_indices: vec![0.0; channels],
            step_fraction: 0.0,
            prev_start: 0.0,
            prev_stop: 0.0,
            prev_pause: 0.0,
            prev_resume: 0.0,
            in_start: MonoInput::default(),
            in_stop: MonoInput::default(),
            in_pause: MonoInput::default(),
            in_resume: MonoInput::default(),
            clock_out: vec![PolyOutput::default(); channels],
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("bpm") {
            self.bpm = *v;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("rows_per_beat") {
            self.rows_per_beat = *v;
        }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("song") {
            self.song_index = if *v < 0 { None } else { Some(*v as usize) };
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("loop") {
            self.do_loop = *v;
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("autostart") {
            self.autostart = *v;
            if self.autostart && !self.use_host_transport {
                self.state = TransportState::Playing;
                self.reset_position();
            }
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("swing") {
            self.swing = *v;
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("sync") {
            self.use_host_transport = match *v {
                "free" => false,
                "host" => true,
                _ /* auto */ => self.hosted,
            };
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_start = MonoInput::from_ports(inputs, 0);
        self.in_stop = MonoInput::from_ports(inputs, 1);
        self.in_pause = MonoInput::from_ports(inputs, 2);
        self.in_resume = MonoInput::from_ports(inputs, 3);
        for i in 0..self.channels {
            self.clock_out[i] = PolyOutput::from_ports(outputs, i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Default: silence on all clock buses
        let mut tick_fired = false;
        let mut reset_fired = false;
        let mut current_tick_duration = self.base_tick_seconds();
        for v in &mut self.bank_indices { *v = 0.0; }
        self.step_fraction = 0.0;

        if self.use_host_transport {
            self.process_host_sync(pool, &mut tick_fired, &mut reset_fired, &mut current_tick_duration);
        } else {
            self.process_free_sync(pool, &mut tick_fired, &mut reset_fired, &mut current_tick_duration);
        }

        // Write clock bus outputs
        if self.emit_stop_sentinel {
            // Send stop sentinel: bank index -1
            for i in 0..self.channels {
                let mut bus = [0.0_f32; 16];
                bus[1] = -1.0; // stop sentinel
                bus[2] = 1.0;  // tick trigger (so PatternPlayer processes this)
                pool.write_poly(&self.clock_out[i], bus);
            }
            self.emit_stop_sentinel = false;
        } else {
            for i in 0..self.channels {
                let mut bus = [0.0_f32; 16];
                bus[0] = if reset_fired { 1.0 } else { 0.0 };
                bus[1] = self.bank_indices.get(i).copied().unwrap_or(0.0);
                bus[2] = if tick_fired { 1.0 } else { 0.0 };
                bus[3] = current_tick_duration;
                bus[4] = self.pattern_step as f32;
                bus[5] = self.step_fraction;
                pool.write_poly(&self.clock_out[i], bus);
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_tracker_data_receiver(&mut self) -> Option<&mut dyn ReceivesTrackerData> {
        Some(self)
    }
}

impl MasterSequencer {
    /// Free-running clock logic (original behaviour).
    fn process_free_sync(
        &mut self,
        pool: &mut CablePool<'_>,
        tick_fired: &mut bool,
        reset_fired: &mut bool,
        current_tick_duration: &mut f32,
    ) {
        // Transport input edge detection
        let start = pool.read_mono(&self.in_start);
        let stop = pool.read_mono(&self.in_stop);
        let pause = pool.read_mono(&self.in_pause);
        let resume = pool.read_mono(&self.in_resume);

        let start_rose = start >= 0.5 && self.prev_start < 0.5;
        let stop_rose = stop >= 0.5 && self.prev_stop < 0.5;
        let pause_rose = pause >= 0.5 && self.prev_pause < 0.5;
        let resume_rose = resume >= 0.5 && self.prev_resume < 0.5;

        self.prev_start = start;
        self.prev_stop = stop;
        self.prev_pause = pause;
        self.prev_resume = resume;

        // Transport state machine
        if stop_rose {
            self.state = TransportState::Stopped;
            self.reset_position();
        }
        if start_rose {
            self.state = TransportState::Playing;
            self.reset_position();
        }
        if pause_rose && self.state == TransportState::Playing {
            self.state = TransportState::Paused;
        }
        if resume_rose && self.state == TransportState::Paused {
            self.state = TransportState::Playing;
        }

        if self.state == TransportState::Playing && !self.song_ended {
            let has_song = self.current_song().is_some();

            if has_song {
                if self.first_tick {
                    *tick_fired = true;
                    *reset_fired = self.pattern_just_reset;
                    self.pattern_just_reset = false;
                    self.first_tick = false;
                    *current_tick_duration = self.tick_duration_seconds(self.global_step);
                    self.samples_until_tick = self.tick_duration_samples(self.global_step);
                    self.fill_bank_indices();
                } else {
                    self.samples_until_tick -= 1.0;

                    if self.samples_until_tick <= 0.0 {
                        if self.advance_step() {
                            *tick_fired = true;
                            *reset_fired = self.pattern_just_reset;
                            self.pattern_just_reset = false;
                            *current_tick_duration = self.tick_duration_seconds(self.global_step);
                            self.samples_until_tick += self.tick_duration_samples(self.global_step);
                            self.fill_bank_indices();
                        }
                    } else {
                        self.fill_bank_indices();
                    }
                }
            }
        }
    }

    /// Host-synced clock logic: uses absolute bar position from host transport
    /// to set the sequencer's position directly. One DAW bar = one sequencer
    /// pattern; the pattern's steps spread across the bar regardless of time
    /// signature.
    fn process_host_sync(
        &mut self,
        pool: &mut CablePool<'_>,
        tick_fired: &mut bool,
        reset_fired: &mut bool,
        current_tick_duration: &mut f32,
    ) {
        let transport = pool.read_poly(&self.transport_in);
        let playing = TransportFrame::playing_raw(&transport);
        let tempo = TransportFrame::tempo(&transport);
        let beat = TransportFrame::beat(&transport) as f64;
        let tsig_num = TransportFrame::tsig_num(&transport) as f64;
        let tsig_denom = TransportFrame::tsig_denom(&transport) as f64;

        // Detect playing edge: host started
        let host_started = playing >= 0.5 && self.prev_host_playing < 0.5;
        // Detect playing edge: host stopped
        let host_stopped = playing < 0.5 && self.prev_host_playing >= 0.5;
        self.prev_host_playing = playing;

        if host_started {
            self.state = TransportState::Playing;
            self.first_tick = true;
            self.song_ended = false;
            self.emit_stop_sentinel = false;
        }
        if host_stopped {
            // Freeze position — don't reset (matches DAW pause/resume).
            self.state = TransportState::Paused;
        }

        if self.state == TransportState::Playing && !self.song_ended && self.current_song().is_some() {
            // Compute beats per bar from time signature. Default to 4/4 if
            // the host doesn't provide time signature data.
            let beats_per_bar = if tsig_num > 0.0 && tsig_denom > 0.0 {
                tsig_num * (4.0 / tsig_denom)
            } else {
                4.0
            };

            // Derive bar number and fractional position within the bar from
            // the continuous beat position.
            let beat_clamped = beat.max(0.0);
            let bar_pos = beat_clamped / beats_per_bar;
            let bar_number = bar_pos.floor() as usize;
            let bar_fraction = bar_pos - bar_number as f64;

            // Map bar to song row, respecting loop point.
            let Some(target_row) = self.resolve_song_row(bar_number) else {
                self.song_ended = true;
                self.emit_stop_sentinel = true;
                self.fill_bank_indices();
                return;
            };

            // Map bar fraction to step within the pattern at the target row.
            let pattern_len = self.pattern_length_at_row(target_row);
            let target_step = if pattern_len > 0 {
                ((bar_fraction * pattern_len as f64).floor() as usize).min(pattern_len - 1)
            } else {
                0
            };

            // Fractional position within the step (for mid-step seeks).
            self.step_fraction = if pattern_len > 0 {
                ((bar_fraction * pattern_len as f64) - target_step as f64) as f32
            } else {
                0.0
            };

            // Detect position changes.
            let row_changed = target_row != self.song_row;
            let step_changed = target_step != self.pattern_step || row_changed;

            if self.first_tick {
                // First tick after host start — set position absolutely.
                self.song_row = target_row;
                self.pattern_step = target_step;
                *tick_fired = true;
                *reset_fired = true;
                self.first_tick = false;
                self.fill_bank_indices();
            } else if step_changed {
                self.song_row = target_row;
                self.pattern_step = target_step;
                *tick_fired = true;
                *reset_fired = row_changed;
                self.fill_bank_indices();
            } else {
                self.fill_bank_indices();
            }

            // Tick duration: one pattern step in seconds.
            if tempo > 0.0 && pattern_len > 0 {
                let bar_duration_secs = (beats_per_bar as f32 / tempo) * 60.0;
                *current_tick_duration = bar_duration_secs / pattern_len as f32;
            }
        }
    }

    fn fill_bank_indices(&mut self) {
        let Some(ref data) = self.tracker_data else { return };
        let Some(idx) = self.song_index else { return };
        let Some(song) = data.songs.songs.get(idx) else { return };

        if self.song_row >= song.order.len() {
            return;
        }

        let row = &song.order[self.song_row];
        for (i, idx) in self.bank_indices.iter_mut().enumerate() {
            if let Some(Some(bank_idx)) = row.get(i) {
                *idx = *bank_idx as f32;
            } else {
                *idx = -1.0; // silence
            }
        }
    }
}

impl ReceivesTrackerData for MasterSequencer {
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>) {
        self.tracker_data = Some(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::{
        PatternBank, SongBank, Song, Pattern, TrackerStep,
    };
    use std::collections::HashMap;

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

    fn simple_step(cv1: f32, trigger: bool, gate: bool) -> TrackerStep {
        TrackerStep { cv1, cv2: 0.0, trigger, gate, cv1_end: None, cv2_end: None, repeat: 1 }
    }

    fn make_sequencer(song_index: i64, bpm: f32, rows_per_beat: i64, do_loop: bool, autostart: bool, swing: f32) -> MasterSequencer {
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("bpm".into(), ParameterValue::Float(bpm));
        params.insert("rows_per_beat".into(), ParameterValue::Int(rows_per_beat));
        params.insert("song".into(), ParameterValue::Int(song_index));
        params.insert("loop".into(), ParameterValue::Bool(do_loop));
        params.insert("autostart".into(), ParameterValue::Bool(autostart));
        params.insert("swing".into(), ParameterValue::Float(swing));
        seq.update_validated_parameters(&mut params);
        seq
    }

    #[test]
    fn tick_timing_at_120_bpm_4_rows() {
        // 120 BPM, 4 rows/beat → tick every 60/(120*4) = 0.125s = 5512.5 samples
        let seq = make_sequencer(0, 120.0, 4, true, true, 0.5);

        let expected_samples = (SR * 60.0 / (120.0 * 4.0)) as usize;
        assert_eq!(expected_samples, 5512);

        let tick_dur = seq.tick_duration_seconds(0);
        assert!((tick_dur - 0.125_f32).abs() < 1e-6);

        let tick_samples = seq.tick_duration_samples(0);
        assert!((tick_samples - 5512.5).abs() < 1.0);
    }

    #[test]
    fn swing_tick_durations() {
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("bpm".into(), ParameterValue::Float(120.0));
        params.insert("rows_per_beat".into(), ParameterValue::Int(4));
        params.insert("song".into(), ParameterValue::Int(0));
        params.insert("loop".into(), ParameterValue::Bool(true));
        params.insert("autostart".into(), ParameterValue::Bool(true));
        params.insert("swing".into(), ParameterValue::Float(0.67));
        seq.update_validated_parameters(&mut params);

        let base = 60.0 / (120.0 * 4.0); // 0.125s

        // Even step: 2 * 0.125 * 0.67 = 0.1675
        let even = seq.tick_duration_seconds(0);
        assert!((even - 2.0 * base * 0.67).abs() < 1e-6, "even step duration: {even}");

        // Odd step: 2 * 0.125 * 0.33 = 0.0825
        let odd = seq.tick_duration_seconds(1);
        assert!((odd - 2.0 * base * 0.33).abs() < 1e-6, "odd step duration: {odd}");

        // Sum of even+odd = 2*base
        assert!((even + odd - 2.0 * base).abs() < 1e-6, "even+odd should equal 2*base");
    }

    #[test]
    fn transport_state_machine() {
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("bpm".into(), ParameterValue::Float(120.0));
        params.insert("rows_per_beat".into(), ParameterValue::Int(4));
        params.insert("song".into(), ParameterValue::Int(0));
        params.insert("loop".into(), ParameterValue::Bool(true));
        params.insert("autostart".into(), ParameterValue::Bool(false));
        params.insert("swing".into(), ParameterValue::Float(0.5));
        seq.update_validated_parameters(&mut params);

        assert_eq!(seq.state, TransportState::Stopped);

        // Simulate start
        seq.state = TransportState::Playing;
        seq.reset_position();
        assert_eq!(seq.state, TransportState::Playing);

        // Simulate pause
        seq.state = TransportState::Paused;
        assert_eq!(seq.state, TransportState::Paused);

        // Resume only works from Paused
        seq.state = TransportState::Playing;
        assert_eq!(seq.state, TransportState::Playing);

        // Stop resets
        seq.state = TransportState::Stopped;
        seq.reset_position();
        assert_eq!(seq.song_row, 0);
        assert_eq!(seq.pattern_step, 0);
    }

    #[test]
    fn loop_point_behaviour() {
        let song = Song {
            channels: 1,
            order: vec![
                vec![Some(0)], // row 0 (intro)
                vec![Some(0)], // row 1 — loop point
                vec![Some(1)], // row 2
            ],
            loop_point: 1,
        };

        let pattern = Pattern {
            channels: 1,
            steps: 2,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
            ]],
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern.clone(), pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("loop_song".to_string(), 0)]),
            },
        });

        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("bpm".into(), ParameterValue::Float(120.0));
        params.insert("rows_per_beat".into(), ParameterValue::Int(4));
        params.insert("song".into(), ParameterValue::Int(0));
        params.insert("loop".into(), ParameterValue::Bool(true));
        params.insert("autostart".into(), ParameterValue::Bool(true));
        params.insert("swing".into(), ParameterValue::Float(0.5));
        seq.update_validated_parameters(&mut params);
        seq.receive_tracker_data(data);

        // 3 rows × 2 steps = 6 advances total before looping.
        // Start at row=0 step=0. After 6 advances: past row 2, loops to row 1.
        for i in 0..6 {
            let ok = seq.advance_step();
            assert!(ok, "advance {i} should succeed");
        }
        // Should have looped back to row 1, step 0
        assert_eq!(seq.song_row, 1, "should loop to row 1");
        assert_eq!(seq.pattern_step, 0, "should be at start of pattern");
    }

    #[test]
    fn end_of_song_no_loop() {
        let song = Song {
            channels: 1,
            order: vec![vec![Some(0)]],
            loop_point: 0,
        };

        let pattern = Pattern {
            channels: 1,
            steps: 2,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
            ]],
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("finite_song".to_string(), 0)]),
            },
        });

        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("bpm".into(), ParameterValue::Float(120.0));
        params.insert("rows_per_beat".into(), ParameterValue::Int(4));
        params.insert("song".into(), ParameterValue::Int(0));
        params.insert("loop".into(), ParameterValue::Bool(false));
        params.insert("autostart".into(), ParameterValue::Bool(true));
        params.insert("swing".into(), ParameterValue::Float(0.5));
        seq.update_validated_parameters(&mut params);
        seq.receive_tracker_data(data);

        // Advance past the last step
        let result = seq.advance_step(); // step 0 → step 1
        assert!(result, "first advance should succeed");
        let result = seq.advance_step(); // step 1 → end of song
        assert!(!result, "should hit end of song");
        assert!(seq.song_ended, "song_ended should be set");
        assert!(seq.emit_stop_sentinel, "should emit stop sentinel");
    }

    #[test]
    fn sync_auto_selects_host_when_hosted() {
        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let seq = MasterSequencer::prepare(&hosted_env, desc, InstanceId::next());
        assert!(seq.use_host_transport, "auto mode should use host transport when hosted");
    }

    #[test]
    fn sync_auto_selects_free_when_standalone() {
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        assert!(!seq.use_host_transport, "auto mode should not use host transport when standalone");
    }

    #[test]
    fn sync_free_overrides_hosted() {
        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&hosted_env, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("sync".into(), ParameterValue::Enum("free"));
        seq.update_validated_parameters(&mut params);
        assert!(!seq.use_host_transport, "sync=free should override hosted");
    }

    #[test]
    fn sync_host_overrides_standalone() {
        let s = shape(1);
        let desc = MasterSequencer::describe(&s);
        let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
        let mut params = ParameterMap::new();
        params.insert("sync".into(), ParameterValue::Enum("host"));
        seq.update_validated_parameters(&mut params);
        assert!(seq.use_host_transport, "sync=host should override standalone");
    }

    #[test]
    fn host_sync_starts_on_playing_edge() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("bpm", ParameterValue::Float(120.0)),
                ("rows_per_beat", ParameterValue::Int(4)),
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        // Provide tracker data.
        let pattern = Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        let song = Song { channels: 1, order: vec![vec![Some(0)]], loop_point: 0 };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Tick 1: not playing yet — clock should be silent.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();
        let bus = h.read_poly("clock");
        assert!(bus[2] < 0.5, "no tick trigger when not playing");

        // Tick 2: playing starts (edge 0→1), beat at 0.0.
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::BEAT] = 0.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();
        let bus = h.read_poly("clock");
        assert!(bus[2] >= 0.5, "tick trigger should fire on first playing edge");
        assert!(bus[0] >= 0.5, "pattern reset should fire on first tick");
    }

    #[test]
    fn host_sync_freezes_on_stop() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("bpm", ParameterValue::Float(120.0)),
                ("rows_per_beat", ParameterValue::Int(4)),
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        let pattern = Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        let song = Song { channels: 1, order: vec![vec![Some(0)]], loop_point: 0 };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Start playing at beat 0.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 0.0;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        // Advance to beat 1.0 in 4/4 → bar fraction 0.25 → step 1 of 4.
        lanes[TransportFrame::BEAT] = 1.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        // Now stop playing — position should freeze (Paused), not reset.
        lanes[TransportFrame::PLAYING] = 0.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.state, TransportState::Paused, "should freeze (Paused), not Stop/reset");
        // Position should not have been reset to 0.
        assert_eq!(seq.pattern_step, 1, "position should be preserved at step 1");
    }

    #[test]
    fn host_sync_mid_song_start() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        // 4 rows of 4-step patterns.
        let make_pattern = || Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        let song = Song {
            channels: 1,
            order: vec![vec![Some(0)], vec![Some(1)], vec![Some(2)], vec![Some(3)]],
            loop_point: 0,
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![make_pattern(), make_pattern(), make_pattern(), make_pattern()] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Start playing at beat 8.0 in 4/4 → bar 2, step 0.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 8.0;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.song_row, 2, "should start at song row 2");
        assert_eq!(seq.pattern_step, 0, "should start at step 0 of the pattern");

        let bus = h.read_poly("clock");
        assert!(bus[2] >= 0.5, "tick trigger should fire");
        assert!(bus[0] >= 0.5, "pattern reset should fire on first tick");
        assert_eq!(bus[1].round() as usize, 2, "bank index should be 2");
    }

    #[test]
    fn host_sync_mid_bar_start() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        // 8-step pattern.
        let pattern = Pattern {
            channels: 1,
            steps: 8,
            data: vec![vec![
                simple_step(1.0, true, true), simple_step(2.0, true, true),
                simple_step(3.0, true, true), simple_step(4.0, true, true),
                simple_step(5.0, true, true), simple_step(6.0, true, true),
                simple_step(7.0, true, true), simple_step(8.0, true, true),
            ]],
        };
        let song = Song {
            channels: 1,
            order: vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
            loop_point: 0,
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Beat 9.7 in 4/4: bar 2, fraction 1.7/4 = 0.425, step = floor(0.425*8) = 3.
        // step_fraction = (0.425*8) - 3 = 0.4.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 9.7;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.song_row, 2, "should be at song row 2");
        assert_eq!(seq.pattern_step, 3, "should be at step 3 of 8");

        let bus = h.read_poly("clock");
        assert_eq!(bus[4].round() as usize, 3, "bus[4] should carry step index 3");
        assert!(bus[5] > 0.3, "bus[5] should carry step fraction ~0.4");
    }

    #[test]
    fn host_sync_three_four_time() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        let pattern = Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        let song = Song {
            channels: 1,
            order: vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
            loop_point: 0,
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // 3/4 time: beats_per_bar = 3. Beat 7.5 → bar 2 (7.5/3=2.5),
        // bar_fraction = 0.5, step = floor(0.5*4) = 2.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 7.5;
        lanes[TransportFrame::TSIG_NUM] = 3.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.song_row, 2, "should be at song row 2 in 3/4");
        assert_eq!(seq.pattern_step, 2, "should be at step 2 of 4");
    }

    #[test]
    fn host_sync_loop_wrapping() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        let pattern = Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        // 3 rows, loop_point=1. Bars 0,1,2 → rows 0,1,2.
        // Bar 3 → loops: loop_point + (3-3)%2 = 1.
        // Bar 4 → loop_point + (4-3)%2 = 2.
        // Bar 5 → loop_point + (5-3)%2 = 1.
        let song = Song {
            channels: 1,
            order: vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
            loop_point: 1,
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Bar 5 in 4/4: beat = 20.0.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 20.0;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.song_row, 1, "bar 5 should wrap to row 1 (loop_point=1)");
    }

    #[test]
    fn host_sync_non_looping_end() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(false)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        let pattern = Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        let song = Song {
            channels: 1,
            order: vec![vec![Some(0)], vec![Some(0)]],
            loop_point: 0,
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![pattern] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Bar 3 in 4/4 (beat=12.0): past the 2-row song, not looping.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 12.0;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert!(seq.song_ended, "song should have ended");
    }

    #[test]
    fn host_sync_daw_seek() {
        use patches_core::test_support::ModuleHarness;
        use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

        let hosted_env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let mut h = ModuleHarness::build_full::<MasterSequencer>(
            &[
                ("song", ParameterValue::Int(0)),
                ("loop", ParameterValue::Bool(true)),
                ("autostart", ParameterValue::Bool(false)),
            ],
            hosted_env,
            shape(1),
        );

        let make_pattern = || Pattern {
            channels: 1,
            steps: 4,
            data: vec![vec![
                simple_step(1.0, true, true),
                simple_step(2.0, true, true),
                simple_step(3.0, true, true),
                simple_step(4.0, true, true),
            ]],
        };
        let song = Song {
            channels: 1,
            order: (0..12).map(|i| vec![Some(i % 4)]).collect(),
            loop_point: 0,
        };
        let data = Arc::new(TrackerData {
            patterns: PatternBank { patterns: vec![make_pattern(), make_pattern(), make_pattern(), make_pattern()] },
            songs: SongBank {
                songs: vec![song],
                name_to_index: HashMap::from([("test".to_string(), 0)]),
            },
        });
        h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

        // Start at bar 1.
        let mut lanes = [0.0f32; 16];
        lanes[TransportFrame::PLAYING] = 1.0;
        lanes[TransportFrame::TEMPO] = 120.0;
        lanes[TransportFrame::BEAT] = 4.0;
        lanes[TransportFrame::TSIG_NUM] = 4.0;
        lanes[TransportFrame::TSIG_DENOM] = 4.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.song_row, 1);

        // Seek to bar 10 in a single tick.
        lanes[TransportFrame::BEAT] = 40.0;
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
        assert_eq!(seq.song_row, 10, "should jump directly to row 10");
        assert_eq!(seq.pattern_step, 0, "should be at step 0 of the bar");

        let bus = h.read_poly("clock");
        assert!(bus[2] >= 0.5, "tick trigger should fire on seek");
        assert!(bus[0] >= 0.5, "pattern reset should fire on row change");
    }
}
