use std::collections::HashMap;
use crate::audio_environment::AudioEnvironment;
use crate::cable_pool::CablePool;
use crate::cables::{
    CableKind, CableValue, InputPort, MonoInput, MonoOutput, OutputPort, PolyInput, PolyOutput,
    POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
};
use crate::modules::{InstanceId, Module, ModuleShape, ParameterValue};
use crate::COEFF_UPDATE_INTERVAL;
use crate::modules::parameter_map::ParameterMap;

/// A single-module test fixture that owns the module and its cable pool, derives
/// port-to-cable assignments from the descriptor, and exposes a named-port interface.
///
/// Eliminates pool sizing, `set_ports` setup, ping-pong management, and `CableValue`
/// unwrapping from test bodies.
///
/// # Construction
///
/// ```ignore
/// let mut h = ModuleHarness::build::<Vca>(&[]);
/// let mut h = ModuleHarness::build::<Oscillator>(&params!["frequency" => 440.0_f32]);
/// let mut h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 3, length: 0, ..Default::default() });
/// let mut h = ModuleHarness::build_with_env::<Glide>(&params!["glide_ms" => 100.0_f32],
///     AudioEnvironment { sample_rate: 22050.0, poly_voices: 16, periodic_update_interval: 32, hosted: false });
/// ```
///
/// All ports are marked `connected: true` by default. Use the `disconnect_*` methods
/// to mark specific ports as disconnected before the first tick.
pub struct ModuleHarness {
    module: Box<dyn Module>,
    pool: Vec<[CableValue; 2]>,
    /// (name, index) → descriptor position for input ports.
    /// Cable slot for input at position i = i.
    input_idx: HashMap<(String, usize), usize>,
    /// (name, index) → descriptor position for output ports.
    /// Cable slot for output at position j = n_inputs + j.
    output_idx: HashMap<(String, usize), usize>,
    n_inputs: usize,
    input_connected: Vec<bool>,
    output_connected: Vec<bool>,
    input_kinds: Vec<CableKind>,
    output_kinds: Vec<CableKind>,
    pub(crate) wi: usize,
    sample_counter: u32,
}

impl ModuleHarness {
    // ── Construction ─────────────────────────────────────────────────────────

    /// Build a harness for module type `M` with the given parameters.
    ///
    /// Uses `AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }` and
    /// `ModuleShape { channels: 0, length: 0, ..Default::default() }` as defaults. All ports connected.
    pub fn build<M: Module + 'static>(params: &[(&str, ParameterValue)]) -> Self {
        Self::build_full::<M>(
            params,
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false },
            ModuleShape { channels: 0, length: 0, ..Default::default() },
        )
    }

    /// Build a harness with a custom `ModuleShape` (needed for shape-dependent modules
    /// such as `Sum` where `channels` controls the number of input ports).
    pub fn build_with_shape<M: Module + 'static>(
        params: &[(&str, ParameterValue)],
        shape: ModuleShape,
    ) -> Self {
        Self::build_full::<M>(
            params,
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false },
            shape,
        )
    }

    /// Build a harness with a custom `AudioEnvironment` (needed for modules that use
    /// `sample_rate`, such as `Glide` and `Oscillator`).
    pub fn build_with_env<M: Module + 'static>(
        params: &[(&str, ParameterValue)],
        env: AudioEnvironment,
    ) -> Self {
        Self::build_full::<M>(params, env, ModuleShape { channels: 0, length: 0, ..Default::default() })
    }

    /// Build a harness with both a custom environment and shape.
    pub fn build_full<M: Module + 'static>(
        params: &[(&str, ParameterValue)],
        env: AudioEnvironment,
        shape: ModuleShape,
    ) -> Self {
        let param_map: ParameterMap = params
            .iter()
            .map(|(name, value)| (name.to_string(), 0, value.clone()))
            .collect();

        let module: Box<dyn Module> = Box::new(
            M::build(&env, &shape, &param_map, InstanceId::next())
                .expect("ModuleHarness::build: module creation failed")
        );

        let descriptor = M::describe(&shape);
        let n_inputs = descriptor.inputs.len();
        let n_outputs = descriptor.outputs.len();

        let mut input_idx = HashMap::new();
        let mut input_kinds = Vec::with_capacity(n_inputs);
        let mut input_connected = Vec::with_capacity(n_inputs);

        for (i, port) in descriptor.inputs.iter().enumerate() {
            input_idx.insert((port.name.to_string(), port.index), i);
            input_kinds.push(port.kind.clone());
            input_connected.push(true);
        }

        let mut output_idx = HashMap::new();
        let mut output_kinds = Vec::with_capacity(n_outputs);
        let mut output_connected = Vec::with_capacity(n_outputs);

        for (j, port) in descriptor.outputs.iter().enumerate() {
            output_idx.insert((port.name.to_string(), port.index), j);
            output_kinds.push(port.kind.clone());
            output_connected.push(true);
        }

        // Pool: reserved backplane slots occupy 0..RESERVED_SLOTS; user cables follow.
        // User inputs occupy RESERVED_SLOTS..RESERVED_SLOTS+n_inputs;
        // user outputs occupy RESERVED_SLOTS+n_inputs..RESERVED_SLOTS+n_inputs+n_outputs.
        // This mirrors the real planner's slot assignment so modules that write to
        // backplane slots (e.g. AudioOut → AUDIO_OUT_L/R) stay in-bounds.
        let pool_size = RESERVED_SLOTS + n_inputs + n_outputs;
        let mut pool = vec![[CableValue::Mono(0.0); 2]; pool_size];

        // Poly reserved slots must hold Poly values to avoid kind-mismatch panics.
        pool[POLY_READ_SINK]  = [CableValue::Poly([0.0; 16]); 2];
        pool[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]); 2];
        pool[crate::cables::GLOBAL_TRANSPORT] = [CableValue::Poly([0.0; 16]); 2];
        pool[crate::cables::GLOBAL_MIDI] = [CableValue::Poly([0.0; 16]); 2];

        // Initialise user poly slots to Poly([0.0; 16]) rather than Mono(0.0).
        for (i, kind) in input_kinds.iter().enumerate() {
            if kind.is_poly() {
                pool[RESERVED_SLOTS + i] = [CableValue::Poly([0.0; 16]); 2];
            }
        }
        for (j, kind) in output_kinds.iter().enumerate() {
            if kind.is_poly() {
                pool[RESERVED_SLOTS + n_inputs + j] = [CableValue::Poly([0.0; 16]); 2];
            }
        }

        let mut harness = Self {
            module,
            pool,
            input_idx,
            output_idx,
            n_inputs,
            input_connected,
            output_connected,
            input_kinds,
            output_kinds,
            wi: 0,
            sample_counter: 0,
        };

        harness.rebuild_ports();
        harness
    }

    // ── Accessors ────────────────────────────────────────────────────────────

    /// Return a reference to the module's descriptor.
    pub fn descriptor(&self) -> &crate::modules::ModuleDescriptor {
        self.module.descriptor()
    }

    /// Return the module's tracker-data receiver if it implements `ReceivesTrackerData`.
    pub fn as_tracker_data_receiver(&mut self) -> Option<&mut dyn crate::tracker::ReceivesTrackerData> {
        self.module.as_tracker_data_receiver()
    }

    /// Downcast the module to a concrete type via `as_any`.
    pub fn as_any(&self) -> &dyn std::any::Any {
        self.module.as_any()
    }

    /// Apply a partial parameter update, equivalent to calling
    /// `module.update_validated_parameters` with a `ParameterMap` built from
    /// the given slice. Used to test hot-reload / partial-update behaviour.
    pub fn update_validated_parameters(&mut self, params: &[(&str, ParameterValue)]) {
        let map: ParameterMap = params
            .iter()
            .map(|(k, v)| (k.to_string(), 0, v.clone()))
            .collect();
        self.update_params_map(&map);
    }

    /// Apply a `ParameterMap` (which may contain indexed parameters) directly to the module.
    ///
    /// Useful for modules whose parameters are indexed (e.g. `level/0`, `solo/1`)
    /// where `update_validated_parameters` cannot express multi-index entries.
    pub fn update_params_map(&mut self, params: &ParameterMap) {
        use crate::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
        use crate::param_layout::{compute_layout, defaults_from_descriptor};
        let desc = self.module.descriptor();
        let layout = compute_layout(desc);
        let index = ParamViewIndex::from_layout(&layout);
        let mut frame = ParamFrame::with_layout(&layout);
        let defaults = defaults_from_descriptor(desc);
        pack_into(&layout, &defaults, params, &mut frame)
            .expect("test harness pack_into failed");
        let view = ParamView::new(&index, &frame);
        self.module.update_validated_parameters(&view);
    }

    // ── Connectivity ─────────────────────────────────────────────────────────

    /// Disconnect the input port `(name, 0)`.
    ///
    /// Calls `set_ports` immediately so the module observes the change before
    /// the next `tick()`.
    pub fn disconnect_input(&mut self, name: &str) {
        self.disconnect_input_at(name, 0);
    }

    /// Disconnect the input port `(name, index)`.
    pub fn disconnect_input_at(&mut self, name: &str, index: usize) {
        let pos = self.input_pos(name, index);
        self.input_connected[pos] = false;
        self.rebuild_ports();
    }

    /// Disconnect the output port `(name, 0)`.
    pub fn disconnect_output(&mut self, name: &str) {
        self.disconnect_output_at(name, 0);
    }

    /// Disconnect the output port `(name, index)`.
    pub fn disconnect_output_at(&mut self, name: &str, index: usize) {
        let pos = self.output_pos(name, index);
        self.output_connected[pos] = false;
        self.rebuild_ports();
    }

    /// Disconnect all input ports.
    pub fn disconnect_all_inputs(&mut self) {
        for c in &mut self.input_connected { *c = false; }
        self.rebuild_ports();
    }

    /// Disconnect all output ports.
    pub fn disconnect_all_outputs(&mut self) {
        for c in &mut self.output_connected { *c = false; }
        self.rebuild_ports();
    }

    // ── Pool initialisation ───────────────────────────────────────────────────

    /// Fill all pool slots with `value`.
    ///
    /// Useful for sentinel-value tests (e.g. verifying that a disconnected output
    /// is never written by seeding it with a known non-zero value).
    pub fn init_pool(&mut self, value: CableValue) {
        for slot in &mut self.pool {
            *slot = [value; 2];
        }
    }

    // ── Mono inputs ──────────────────────────────────────────────────────────

    /// Write `value` to the read slot for input `(name, 0)`.
    ///
    /// The module reads this value on the next `tick()`.
    pub fn set_mono(&mut self, name: &str, value: f32) {
        self.set_mono_at(name, 0, value);
    }

    /// Write `value` to the read slot for input `(name, index)`.
    ///
    /// Writes to both ping-pong slots so the value persists across multiple ticks
    /// until `set_mono_at` is called again.
    pub fn set_mono_at(&mut self, name: &str, index: usize, value: f32) {
        let cable = self.input_cable(name, index);
        self.pool[cable] = [CableValue::Mono(value); 2];
    }

    // ── Poly inputs ──────────────────────────────────────────────────────────

    /// Write `value` to the read slot for poly input `(name, 0)`.
    pub fn set_poly(&mut self, name: &str, value: [f32; 16]) {
        self.set_poly_at(name, 0, value);
    }

    /// Write `value` to the read slot for poly input `(name, index)`.
    ///
    /// Writes to both ping-pong slots so the value persists across multiple ticks.
    pub fn set_poly_at(&mut self, name: &str, index: usize, value: [f32; 16]) {
        let cable = self.input_cable(name, index);
        self.pool[cable] = [CableValue::Poly(value); 2];
    }

    // ── Tick ─────────────────────────────────────────────────────────────────

    /// Process one sample. Returns `&mut Self` for chaining.
    ///
    /// Mirrors [`ExecutionPlan::tick`]: calls [`Module::periodic_update`] every
    /// [`COEFF_UPDATE_INTERVAL`] samples before the main process call.
    pub fn tick(&mut self) -> &mut Self {
        if self.sample_counter == 0 && self.module.wants_periodic() {
            let pool = CablePool::new(&mut self.pool, self.wi);
            self.module.periodic_update(&pool);
        }
        self.sample_counter += 1;
        if self.sample_counter >= COEFF_UPDATE_INTERVAL {
            self.sample_counter = 0;
        }
        self.module.process(&mut CablePool::new(&mut self.pool, self.wi));
        self.wi = 1 - self.wi;
        self
    }

    // ── Mono outputs ─────────────────────────────────────────────────────────

    /// Read the mono output `(name, 0)` from the most recently completed tick.
    ///
    /// # Panics
    /// Panics if the cable value is not `CableValue::Mono`.
    pub fn read_mono(&self, name: &str) -> f32 {
        self.read_mono_at(name, 0)
    }

    /// Read the mono output `(name, index)` from the most recently completed tick.
    pub fn read_mono_at(&self, name: &str, index: usize) -> f32 {
        let cable = self.output_cable(name, index);
        match self.pool[cable][1 - self.wi] {
            CableValue::Mono(v) => v,
            CableValue::Poly(_) => panic!(
                "ModuleHarness::read_mono: output '{}'/{}  is Poly, not Mono",
                name, index
            ),
        }
    }

    /// Run `ticks` samples and collect the named mono output into a `Vec<f32>`.
    pub fn run_mono(&mut self, ticks: usize, output: &str) -> Vec<f32> {
        (0..ticks).map(|_| { self.tick(); self.read_mono(output) }).collect()
    }

    /// Run `ticks` samples, feeding values from `inputs` (cycled if shorter)
    /// into the named mono input each tick, and collect the named mono output.
    pub fn run_mono_mapped(
        &mut self,
        ticks: usize,
        input: &str,
        inputs: &[f32],
        output: &str,
    ) -> Vec<f32> {
        (0..ticks).map(|i| {
            self.set_mono(input, inputs[i % inputs.len()]);
            self.tick();
            self.read_mono(output)
        }).collect()
    }

    // ── Poly outputs ─────────────────────────────────────────────────────────

    /// Read the poly output `(name, 0)` from the most recently completed tick.
    ///
    /// # Panics
    /// Panics if the cable value is not `CableValue::Poly`.
    pub fn read_poly(&self, name: &str) -> [f32; 16] {
        self.read_poly_at(name, 0)
    }

    /// Read the poly output `(name, index)` from the most recently completed tick.
    pub fn read_poly_at(&self, name: &str, index: usize) -> [f32; 16] {
        let cable = self.output_cable(name, index);
        match self.pool[cable][1 - self.wi] {
            CableValue::Poly(v) => v,
            CableValue::Mono(_) => panic!(
                "ModuleHarness::read_poly: output '{}'/{}  is Mono, not Poly",
                name, index
            ),
        }
    }

    /// Read a single voice from the poly output `(name, 0)`.
    ///
    /// Convenience wrapper around [`read_poly`] for tests that only care about
    /// one voice.
    ///
    /// # Panics
    /// Panics if `voice >= 16` or the cable value is not `CableValue::Poly`.
    pub fn read_poly_voice(&self, name: &str, voice: usize) -> f32 {
        self.read_poly_voice_at(name, 0, voice)
    }

    /// Read a single voice from the poly output `(name, index)`.
    pub fn read_poly_voice_at(&self, name: &str, index: usize, voice: usize) -> f32 {
        assert!(voice < 16, "read_poly_voice: voice {voice} out of range (max 15)");
        self.read_poly_at(name, index)[voice]
    }

    /// Run `ticks` samples and collect the named poly output.
    pub fn run_poly(&mut self, ticks: usize, output: &str) -> Vec<[f32; 16]> {
        (0..ticks).map(|_| { self.tick(); self.read_poly(output) }).collect()
    }

    /// Run `ticks` samples, feeding `inputs` (cycled if shorter) into the named
    /// poly input each tick, and collect the named poly output.
    pub fn run_poly_mapped(
        &mut self,
        ticks: usize,
        input: &str,
        inputs: &[[f32; 16]],
        output: &str,
    ) -> Vec<[f32; 16]> {
        (0..ticks).map(|i| {
            self.set_poly(input, inputs[i % inputs.len()]);
            self.tick();
            self.read_poly(output)
        }).collect()
    }

    // ── Measurement helpers ────────────────────────────────────────────────────

    /// Run `ticks` samples collecting mono output, then return the RMS.
    pub fn measure_rms(&mut self, ticks: usize, output: &str) -> f32 {
        let samples = self.run_mono(ticks, output);
        let sum_sq: f32 = samples.iter().map(|&x| x * x).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    /// Run `ticks` samples collecting mono output, then return the peak
    /// absolute value.
    pub fn measure_peak(&mut self, ticks: usize, output: &str) -> f32 {
        let samples = self.run_mono(ticks, output);
        samples.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
    }

    /// Run `ticks` samples and assert every sample of the named mono output
    /// falls within `[min, max]`.
    ///
    /// # Panics
    /// Panics with the first out-of-range sample.
    pub fn assert_output_bounded(
        &mut self,
        ticks: usize,
        output: &str,
        min: f32,
        max: f32,
    ) {
        for i in 0..ticks {
            self.tick();
            let v = self.read_mono(output);
            assert!(
                v >= min && v <= max,
                "sample {i}: output '{output}' = {v}, expected [{min}, {max}]"
            );
        }
    }

    /// Send a unit impulse on `input` and collect `n` samples of `output`.
    ///
    /// Drives `input` to 1.0 for the first tick, then 0.0 for the rest. The
    /// returned vector has length `n` starting at the first sample after the
    /// impulse is presented (i.e. `out[0]` is the module's response to the
    /// impulse on the same tick).
    pub fn impulse_response(&mut self, input: &str, output: &str, n: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(n);
        self.set_mono(input, 1.0);
        self.tick();
        out.push(self.read_mono(output));
        self.set_mono(input, 0.0);
        for _ in 1..n {
            self.tick();
            out.push(self.read_mono(output));
        }
        out
    }

    /// Hold `input` at 1.0 and collect `n` samples of `output` (step response).
    pub fn step_response(&mut self, input: &str, output: &str, n: usize) -> Vec<f32> {
        self.set_mono(input, 1.0);
        self.run_mono(n, output)
    }

    /// For each value in `values`, update parameter `name`, run `settle` ticks
    /// to let the module respond, then run `measure` more ticks collecting
    /// `output`, and pass the collected samples to `measure_fn`. Returns one
    /// scalar per input value.
    ///
    /// Useful for sweeping cutoff/gain/depth and asserting a monotonic or
    /// bounded relationship between parameter and measured output.
    pub fn sweep_parameter<F>(
        &mut self,
        name: &str,
        values: &[f32],
        settle: usize,
        measure: usize,
        output: &str,
        measure_fn: F,
    ) -> Vec<f32>
    where
        F: Fn(&[f32]) -> f32,
    {
        let mut results = Vec::with_capacity(values.len());
        for &v in values {
            self.update_validated_parameters(&[(name, ParameterValue::Float(v))]);
            for _ in 0..settle { self.tick(); }
            let samples = self.run_mono(measure, output);
            results.push(measure_fn(&samples));
        }
        results
    }

    /// Run `warmup` ticks then `measure` ticks; assert the variance of the
    /// measurement window stays below `max_variance`. Catches systems that
    /// have not settled (oscillating, drifting) but pass naive bound checks.
    pub fn assert_steady_state_bounded(
        &mut self,
        warmup: usize,
        measure: usize,
        output: &str,
        max_variance: f32,
    ) {
        for _ in 0..warmup { self.tick(); }
        let samples = self.run_mono(measure, output);
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        let var = samples.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / samples.len() as f32;
        assert!(
            var <= max_variance,
            "steady-state variance for '{output}' = {var:.6e}, exceeds bound {max_variance:.6e} (mean={mean})"
        );
    }

    /// Disconnect a list of input ports by name (all at index 0).
    ///
    /// Convenience for tests that need to isolate a module from CV inputs:
    /// ```ignore
    /// h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
    /// ```
    pub fn disconnect_inputs(&mut self, names: &[&str]) {
        for name in names {
            let pos = self.input_pos(name, 0);
            self.input_connected[pos] = false;
        }
        self.rebuild_ports();
    }

    // ── Internals ─────────────────────────────────────────────────────────────

    fn input_pos(&self, name: &str, index: usize) -> usize {
        *self.input_idx.get(&(name.to_string(), index)).unwrap_or_else(|| {
            panic!(
                "ModuleHarness: input port '{}/{}' not found in descriptor. \
                 Available inputs: {:?}",
                name, index,
                self.input_idx.keys().collect::<Vec<_>>()
            )
        })
    }

    fn output_pos(&self, name: &str, index: usize) -> usize {
        *self.output_idx.get(&(name.to_string(), index)).unwrap_or_else(|| {
            panic!(
                "ModuleHarness: output port '{}/{}' not found in descriptor. \
                 Available outputs: {:?}",
                name, index,
                self.output_idx.keys().collect::<Vec<_>>()
            )
        })
    }

    fn input_cable(&self, name: &str, index: usize) -> usize {
        RESERVED_SLOTS + self.input_pos(name, index)
    }

    fn output_cable(&self, name: &str, index: usize) -> usize {
        RESERVED_SLOTS + self.n_inputs + self.output_pos(name, index)
    }

    /// Read both ping-pong slots for a raw pool index.
    ///
    /// Useful for verifying writes to backplane slots (e.g. `AUDIO_OUT_L`)
    /// that are not exposed as named output ports.
    pub fn pool_slot(&self, idx: usize) -> CableValue {
        self.pool[idx][1 - self.wi]
    }

    /// Write `value` to both ping-pong slots at a raw pool index.
    ///
    /// Useful for seeding backplane slots (e.g. `AUDIO_IN_L`) that are not
    /// exposed as named input ports.
    pub fn set_pool_slot(&mut self, idx: usize, value: CableValue) {
        self.pool[idx] = [value; 2];
    }

    fn rebuild_ports(&mut self) {
        let inputs: Vec<InputPort> = self.input_kinds
            .iter()
            .enumerate()
            .map(|(i, kind)| {
                let mono = MonoInput {
                    cable_idx: RESERVED_SLOTS + i,
                    scale: 1.0,
                    connected: self.input_connected[i],
                };
                let poly = PolyInput {
                    cable_idx: RESERVED_SLOTS + i,
                    scale: 1.0,
                    connected: self.input_connected[i],
                };
                match kind {
                    CableKind::Mono => InputPort::Mono(mono),
                    CableKind::Poly => InputPort::Poly(poly),
                }
            })
            .collect();

        let outputs: Vec<OutputPort> = self.output_kinds
            .iter()
            .enumerate()
            .map(|(j, kind)| {
                let mono = MonoOutput {
                    cable_idx: RESERVED_SLOTS + self.n_inputs + j,
                    connected: self.output_connected[j],
                };
                let poly = PolyOutput {
                    cable_idx: RESERVED_SLOTS + self.n_inputs + j,
                    connected: self.output_connected[j],
                };
                match kind {
                    CableKind::Mono => OutputPort::Mono(mono),
                    CableKind::Poly => OutputPort::Poly(poly),
                }
            })
            .collect();

        self.module.set_ports(&inputs, &outputs);
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
