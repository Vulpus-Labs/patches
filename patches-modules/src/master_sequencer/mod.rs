use std::sync::Arc;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, PolyInput, PolyOutput, ModuleShape, OutputPort,
    TrackerData, ReceivesTrackerData, TransportFrame,
    GLOBAL_TRANSPORT,
};
use patches_core::parameter_map::ParameterMap;
use patches_tracker_core::{HostTransport, SequencerCore, TickResult, TransportEdges};

mod params;

/// Drives song playback with transport controls, swing, and a poly clock bus
/// per song channel.
///
/// The MasterSequencer reads a named song from `TrackerData` and outputs a poly
/// clock bus per song channel. Each clock bus carries six voices encoding
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

    tracker_data: Option<Arc<TrackerData>>,

    // Module-shell settings resolved from parameters.
    autostart: bool,
    pub(crate) hosted: bool,
    pub(crate) use_host_transport: bool,

    pub(crate) core: SequencerCore,

    /// Fixed input pointing at the GLOBAL_TRANSPORT backplane slot.
    transport_in: PolyInput,

    in_start: MonoInput,
    in_stop: MonoInput,
    in_pause: MonoInput,
    in_resume: MonoInput,
    clock_out: Vec<PolyOutput>,
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
            .enum_param("sync", self::params::SyncMode::VARIANTS, "auto")
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            tracker_data: None,
            autostart: true,
            hosted: env.hosted,
            use_host_transport: env.hosted,
            core: SequencerCore::new(env.sample_rate, channels),
            transport_in: PolyInput {
                cable_idx: GLOBAL_TRANSPORT,
                scale: 1.0,
                connected: true,
            },
            in_start: MonoInput::default(),
            in_stop: MonoInput::default(),
            in_pause: MonoInput::default(),
            in_resume: MonoInput::default(),
            clock_out: vec![PolyOutput::default(); channels],
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
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
        for i in 0..self.core.channels {
            self.clock_out[i] = PolyOutput::from_ports(outputs, i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let result = if self.use_host_transport {
            let transport = pool.read_poly(&self.transport_in);
            let host = HostTransport {
                playing: TransportFrame::playing_raw(&transport),
                tempo: TransportFrame::tempo(&transport),
                beat: TransportFrame::beat(&transport) as f64,
                tsig_num: TransportFrame::tsig_num(&transport) as f64,
                tsig_denom: TransportFrame::tsig_denom(&transport) as f64,
            };
            self.tick_host(&host)
        } else {
            let edges = TransportEdges {
                start: pool.read_mono(&self.in_start),
                stop: pool.read_mono(&self.in_stop),
                pause: pool.read_mono(&self.in_pause),
                resume: pool.read_mono(&self.in_resume),
            };
            self.tick_free(&edges)
        };

        if self.core.emit_stop_sentinel {
            for i in 0..self.core.channels {
                let mut bus = [0.0_f32; 16];
                bus[1] = -1.0;
                bus[2] = 1.0;
                pool.write_poly(&self.clock_out[i], bus);
            }
            self.core.emit_stop_sentinel = false;
        } else {
            for i in 0..self.core.channels {
                let mut bus = [0.0_f32; 16];
                bus[0] = if result.reset_fired { 1.0 } else { 0.0 };
                bus[1] = self.core.bank_indices.get(i).copied().unwrap_or(0.0);
                bus[2] = if result.tick_fired { 1.0 } else { 0.0 };
                bus[3] = result.tick_duration_seconds;
                bus[4] = self.core.pattern_step as f32;
                bus[5] = self.core.step_fraction;
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
    fn tick_host(&mut self, host: &HostTransport) -> TickResult {
        match self.tracker_data.as_ref() {
            Some(data) => self.core.tick_host(host, data),
            None => TickResult {
                tick_fired: false,
                reset_fired: false,
                tick_duration_seconds: self.core.base_tick_seconds(),
            },
        }
    }

    fn tick_free(&mut self, edges: &TransportEdges) -> TickResult {
        match self.tracker_data.as_ref() {
            Some(data) => self.core.tick_free(edges, data),
            None => TickResult {
                tick_fired: false,
                reset_fired: false,
                tick_duration_seconds: self.core.base_tick_seconds(),
            },
        }
    }

}

impl ReceivesTrackerData for MasterSequencer {
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>) {
        self.tracker_data = Some(data);
    }
}

#[cfg(test)]
mod tests;
