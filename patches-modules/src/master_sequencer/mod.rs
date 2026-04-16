use std::sync::Arc;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, PolyInput, PolyOutput, ModuleShape, OutputPort,
    TrackerData, ReceivesTrackerData,
    GLOBAL_TRANSPORT,
};
use patches_core::parameter_map::ParameterMap;

mod lookup;
mod params;
mod playback;

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
/// # Notes
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
        self.apply_params(params);
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

impl ReceivesTrackerData for MasterSequencer {
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>) {
        self.tracker_data = Some(data);
    }
}

#[cfg(test)]
mod tests;
