use std::sync::Arc;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoOutput, PolyInput, ModuleShape, OutputPort,
    TrackerData, ReceivesTrackerData,
};
use patches_core::parameter_map::ParameterMap;
use patches_tracker_core::{ClockBusFrame, PatternPlayerCore};

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
    tracker_data: Option<Arc<TrackerData>>,
    pub(crate) core: PatternPlayerCore,

    clock_in: PolyInput,
    cv1_out: Vec<MonoOutput>,
    cv2_out: Vec<MonoOutput>,
    trigger_out: Vec<MonoOutput>,
    gate_out: Vec<MonoOutput>,
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
            tracker_data: None,
            core: PatternPlayerCore::new(env.sample_rate, channels),
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
        let n = self.core.channels;
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
        let frame = ClockBusFrame::from_poly(&clock);

        if let Some(ref data) = self.tracker_data {
            self.core.tick(&frame, data);
        } else {
            // No tracker data yet: still consume the edge so we do not
            // re-fire on the next sample, and emit nothing.
            let _ = self.core.prev_tick_trigger;
            self.core.prev_tick_trigger = frame.tick_trigger;
        }

        let n = self.core.channels;
        for ch in 0..n {
            if self.cv1_out[ch].connected {
                pool.write_mono(&self.cv1_out[ch], self.core.cv1[ch]);
            }
            if self.cv2_out[ch].connected {
                pool.write_mono(&self.cv2_out[ch], self.core.cv2[ch]);
            }
            pool.write_mono(
                &self.trigger_out[ch],
                if self.core.trigger_pending[ch] { 1.0 } else { 0.0 },
            );
            pool.write_mono(
                &self.gate_out[ch],
                if self.core.gate[ch] { 1.0 } else { 0.0 },
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
mod tests;
