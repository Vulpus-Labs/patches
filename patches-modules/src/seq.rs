use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoOutput, ModuleShape, OutputPort, TriggerInput,
};
use patches_core::build_error::BuildError;
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// A pre-parsed step in the sequencer pattern.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    /// A named note with a V/OCT pitch value (relative to C0=0.0).
    Note { voct: f32 },
    /// A rest: gate=0, trigger=0; pitch holds previous value.
    Rest,
    /// A tie: gate=1, trigger=0; pitch holds the current tied note's value.
    Tie,
}

/// Error returned when a step string cannot be parsed.
#[derive(Debug, PartialEq)]
struct ParseStepError {
    step: String,
}

impl std::fmt::Display for ParseStepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unrecognised step string: {:?}", self.step)
    }
}

impl std::error::Error for ParseStepError {}

/// Parse a step string into a `Step`.
fn parse_step(s: &str) -> Result<Step, ParseStepError> {
    match s {
        "-" => return Ok(Step::Rest),
        "_" => return Ok(Step::Tie),
        _ => {}
    }

    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(ParseStepError { step: s.to_string() });
    }

    // Letter
    let letter = bytes[0] as char;
    let semitone_base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(ParseStepError { step: s.to_string() }),
    };

    let mut pos = 1;

    // Optional accidental
    let accidental: i32 = if pos < bytes.len() {
        match bytes[pos] as char {
            '#' => { pos += 1; 1 }
            'b' => { pos += 1; -1 }
            _ => 0,
        }
    } else {
        0
    };

    // Octave digit
    if pos >= bytes.len() {
        return Err(ParseStepError { step: s.to_string() });
    }
    let octave_char = bytes[pos] as char;
    let octave: i32 = octave_char
        .to_digit(10)
        .map(|d| d as i32)
        .ok_or_else(|| ParseStepError { step: s.to_string() })?;
    pos += 1;

    // No trailing characters allowed
    if pos != bytes.len() {
        return Err(ParseStepError { step: s.to_string() });
    }

    let semitone_index = semitone_base + accidental;
    let voct = octave as f32 + semitone_index as f32 / 12.0;
    Ok(Step::Note { voct })
}

/// A step sequencer that advances one step per rising edge on the `clock` input.
///
/// Steps are specified as an array of note strings. Note format: letter + optional
/// accidental (`#`/`b`) + octave digit (e.g. `C3`, `D#1`, `Bb2`). Use `-` for a
/// rest (gate low, pitch holds) and `_` for a tie (gate stays high, no retrigger).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `clock` | mono | Rising edge advances to the next step |
/// | `start` | mono | Rising edge starts playback |
/// | `stop` | mono | Rising edge stops playback (gate drops) |
/// | `reset` | mono | Rising edge resets the step index to 0 |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `pitch` | mono | V/oct pitch (C0 = 0.0) |
/// | `trigger` | mono | 1.0 on the clock-advance sample, then 0.0 |
/// | `gate` | mono | 1.0 while a note or tie is active |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `steps` | str\[\] | — | `[]` | Array of step strings (e.g. `C3`, `D#1`, `-`, `_`) |
pub struct Seq {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    steps: Vec<Step>,
    step_index: usize,
    playing: bool,
    /// Pitch value held until next note step.
    current_pitch: f32,
    /// Whether to emit trigger=1 on this sample.
    trigger_pending: bool,
    // Port fields
    in_clock: TriggerInput,
    in_start: TriggerInput,
    in_stop: TriggerInput,
    in_reset: TriggerInput,
    out_pitch: MonoOutput,
    out_trigger: MonoOutput,
    out_gate: MonoOutput,
}

impl Seq {
    /// Apply the step at `self.step_index` to the internal pitch/trigger state.
    fn apply_current_step(&mut self) {
        match &self.steps[self.step_index] {
            Step::Note { voct } => {
                self.current_pitch = *voct;
                self.trigger_pending = true;
            }
            Step::Rest | Step::Tie => {
                // pitch holds; trigger stays false
            }
        }
    }
}

impl Module for Seq {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Seq", shape.clone())
            .mono_in("clock")
            .mono_in("start")
            .mono_in("stop")
            .mono_in("reset")
            .mono_out("pitch")
            .mono_out("trigger")
            .mono_out("gate")
            .array_param("steps", &[], shape.length)
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let capacity = descriptor.shape.length;
        Self {
            instance_id,
            descriptor,
            steps: Vec::with_capacity(capacity),
            step_index: 0,
            playing: true,
            current_pitch: 0.0,
            trigger_pending: false,
            in_clock: TriggerInput::default(),
            in_start: TriggerInput::default(),
            in_stop: TriggerInput::default(),
            in_reset: TriggerInput::default(),
            out_pitch: MonoOutput::default(),
            out_trigger: MonoOutput::default(),
            out_gate: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Array(step_strs)) = params.get_scalar("steps") {
            // Steps have already been validated by update_parameters; parse is infallible here.
            let parsed: Vec<Step> = step_strs
                .iter()
                .filter_map(|s| parse_step(s).ok())
                .collect();
            self.steps = parsed;
            // Do not reset step_index: preserve position so that hot-reloading a pattern
            // during playback does not cause an audible jump to step 0.
            // process() uses steps.get(step_index), which returns None (treated as rest)
            // for any out-of-range index until the next clock edge wraps it back in bounds.
        }
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        patches_core::validate_parameters(params, self.descriptor())?;
        // Validate step patterns before applying — array content is not checked by
        // validate_parameters, so we do it here in the fallible layer.
        if let Some(ParameterValue::Array(step_strs)) = params.get_scalar("steps") {
            let _: Vec<Step> = step_strs
                .iter()
                .map(|s| parse_step(s))
                .collect::<Result<Vec<Step>, ParseStepError>>()
                .map_err(|e| BuildError::Custom {
                    module: "StepSequencer",
                    message: format!("invalid step pattern: {e}"),
                })?;
        }
        self.update_validated_parameters(&mut params.clone());
        Ok(())
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_clock = TriggerInput::from_ports(inputs, 0);
        self.in_start = TriggerInput::from_ports(inputs, 1);
        self.in_stop = TriggerInput::from_ports(inputs, 2);
        self.in_reset = TriggerInput::from_ports(inputs, 3);
        self.out_pitch = MonoOutput::from_ports(outputs, 0);
        self.out_trigger = MonoOutput::from_ports(outputs, 1);
        self.out_gate = MonoOutput::from_ports(outputs, 2);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Guard: when the pattern is empty all outputs hold at rest values.
        if self.steps.is_empty() {
            pool.write_mono(&self.out_pitch, 0.0);
            pool.write_mono(&self.out_trigger, 0.0);
            pool.write_mono(&self.out_gate, 0.0);
            return;
        }

        let clock_rose = self.in_clock.tick(pool);
        let start_rose = self.in_start.tick(pool);
        let stop_rose  = self.in_stop.tick(pool);
        let reset_rose = self.in_reset.tick(pool);

        if reset_rose {
            self.step_index = 0;
            self.trigger_pending = false;
        }

        if stop_rose {
            self.playing = false;
            self.trigger_pending = false;
        }

        if start_rose {
            self.playing = true;
        }

        if clock_rose && self.playing && !self.steps.is_empty() {
            self.step_index = (self.step_index + 1) % self.steps.len();
            self.apply_current_step();
        }

        // Determine outputs from current step
        let (gate, trigger) = if !self.playing {
            (0.0, 0.0)
        } else {
            match self.steps.get(self.step_index) {
                Some(Step::Note { .. }) => {
                    let t = if self.trigger_pending { 1.0 } else { 0.0 };
                    (1.0, t)
                }
                Some(Step::Tie) => (1.0, 0.0),
                Some(Step::Rest) | None => (0.0, 0.0),
            }
        };

        pool.write_mono(&self.out_pitch, self.current_pitch);
        pool.write_mono(&self.out_trigger, trigger);
        pool.write_mono(&self.out_gate, gate);

        // trigger is a one-sample pulse
        self.trigger_pending = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape, Registry, InstanceId};
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::test_support::{assert_within, ModuleHarness};

    fn make_sequencer(steps: &[&str]) -> ModuleHarness {
        let step_strs: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
        let pv = vec![("steps", ParameterValue::Array(step_strs.into()))];
        ModuleHarness::build_full::<Seq>(
            &pv,
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 },
            ModuleShape { channels: 0, length: 32, ..Default::default() },
        )
    }

    #[test]
    fn parse_note_c2() {
        assert_eq!(parse_step("C2"), Ok(Step::Note { voct: 2.0 }));
    }

    #[test]
    fn parse_note_c3() {
        assert_eq!(parse_step("C3"), Ok(Step::Note { voct: 3.0 }));
    }

    #[test]
    fn parse_note_sharp() {
        let expected = 2.0 + 1.0 / 12.0;
        match parse_step("C#2").unwrap() {
            Step::Note { voct } => assert_within!(expected, voct, 1e-12_f32),
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn parse_note_flat() {
        let expected = 3.0 + 10.0 / 12.0;
        match parse_step("Bb3").unwrap() {
            Step::Note { voct } => assert_within!(expected, voct, 1e-12_f32),
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn parse_rest() {
        assert_eq!(parse_step("-"), Ok(Step::Rest));
    }

    #[test]
    fn parse_tie() {
        assert_eq!(parse_step("_"), Ok(Step::Tie));
    }

    #[test]
    fn parse_invalid_returns_error() {
        assert!(parse_step("X9").is_err());
        assert!(parse_step("C").is_err());
        assert!(parse_step("").is_err());
        assert!(parse_step("C##3").is_err());
    }

    #[test]
    fn empty_pattern_succeeds_and_process_does_not_panic() {
        let mut h = make_sequencer(&[]);
        h.set_mono("clock", 0.0);
        h.set_mono("start", 0.0);
        h.set_mono("stop",  0.0);
        h.set_mono("reset", 0.0);
        h.tick();
        assert_eq!(h.read_mono("pitch"),   0.0, "pitch should be 0.0 for empty pattern");
        assert_eq!(h.read_mono("trigger"), 0.0, "trigger should be 0.0 for empty pattern");
        assert_eq!(h.read_mono("gate"),    0.0, "gate should be 0.0 for empty pattern");
        h.set_mono("clock", 1.0);
        h.tick();
        assert_eq!(h.read_mono("pitch"),   0.0);
        assert_eq!(h.read_mono("trigger"), 0.0);
        assert_eq!(h.read_mono("gate"),    0.0);
    }

    #[test]
    fn invalid_step_string_returns_err_from_create() {
        let mut params = ParameterMap::new();
        params.insert(
            "steps".into(),
            ParameterValue::Array(vec!["Z9".to_string()].into()),
        );
        let mut r = Registry::new();
        r.register::<Seq>();
        let result = r.create(
            "StepSequencer",
            &AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 },
            &ModuleShape { channels: 0, length: 32, ..Default::default() },
            &params,
            InstanceId::next(),
        );
        assert!(result.is_err(), "expected Err for invalid step string");
    }

    #[test]
    fn basic_sequence_pitch_trigger_gate() {
        let mut h = make_sequencer(&["C3", "D3", "-", "_"]);

        // Start playing
        h.set_mono("start", 1.0);
        h.set_mono("clock", 0.0);
        h.set_mono("stop",  0.0);
        h.set_mono("reset", 0.0);
        h.tick();
        h.set_mono("start", 0.0);
        h.tick();

        h.set_mono("clock", 0.0);
        h.tick();
        assert_eq!(h.read_mono("gate"),    1.0, "gate at step 0 (C3)");
        assert_eq!(h.read_mono("trigger"), 0.0, "no trigger before first clock");
        assert_eq!(h.read_mono("pitch"),   0.0, "pitch is initial current_pitch C0");

        let d3 = 3.0 + 2.0 / 12.0;
        h.set_mono("clock", 1.0);
        h.tick();
        assert_eq!(h.read_mono("gate"),    1.0, "gate on D3");
        assert_eq!(h.read_mono("trigger"), 1.0, "trigger on D3 advance");
        assert_within!(d3, h.read_mono("pitch"), 1e-12_f32);

        // clock stays high: no retrigger
        h.tick();
        assert_eq!(h.read_mono("trigger"), 0.0);
        assert_eq!(h.read_mono("gate"),    1.0);

        h.set_mono("clock", 0.0);
        h.tick();

        h.set_mono("clock", 1.0);
        h.tick();
        assert_eq!(h.read_mono("gate"),    0.0, "gate on rest");
        assert_eq!(h.read_mono("trigger"), 0.0, "no trigger on rest");
        assert_within!(d3, h.read_mono("pitch"), 1e-12_f32);

        h.set_mono("clock", 0.0);
        h.tick();
        h.set_mono("clock", 1.0);
        h.tick();
        assert_eq!(h.read_mono("gate"),    1.0, "gate on tie");
        assert_eq!(h.read_mono("trigger"), 0.0, "no trigger on tie");

        h.set_mono("clock", 0.0);
        h.tick();
        h.set_mono("clock", 1.0);
        h.tick();
        assert_eq!(h.read_mono("gate"),    1.0, "gate on C3 re-entry");
        assert_eq!(h.read_mono("trigger"), 1.0, "trigger on C3 re-entry");
        assert_within!(3.0, h.read_mono("pitch"), 1e-12_f32);
    }

    #[test]
    fn stop_suppresses_gate_and_blocks_clock() {
        let mut h = make_sequencer(&["C3", "D3"]);

        h.set_mono("start", 1.0);
        h.set_mono("clock", 0.0);
        h.set_mono("stop",  0.0);
        h.set_mono("reset", 0.0);
        h.tick();
        h.set_mono("start", 0.0);
        h.tick();

        h.set_mono("clock", 1.0);
        h.tick();
        h.set_mono("clock", 0.0);
        h.tick();

        h.set_mono("stop", 1.0);
        h.tick();
        assert_eq!(h.read_mono("gate"),    0.0, "gate suppressed on stop");
        assert_eq!(h.read_mono("trigger"), 0.0);

        h.set_mono("stop", 0.0);
        h.tick();
        h.set_mono("clock", 1.0);
        h.tick();
        assert_eq!(h.read_mono("gate"), 0.0, "gate stays 0 while stopped");

        h.set_mono("clock", 0.0);
        h.tick();
        h.set_mono("start", 1.0);
        h.tick();
        h.set_mono("start", 0.0);
        h.tick();
        assert_eq!(h.read_mono("gate"), 1.0, "gate restored after start");
    }

    #[test]
    fn reset_returns_to_step_zero_then_advance() {
        let mut h = make_sequencer(&["C3", "D3", "E3"]);

        h.set_mono("start", 1.0);
        h.set_mono("clock", 0.0);
        h.set_mono("stop",  0.0);
        h.set_mono("reset", 0.0);
        h.tick();
        h.set_mono("start", 0.0);
        h.tick();

        h.set_mono("clock", 1.0); h.tick();
        h.set_mono("clock", 0.0); h.tick();
        h.set_mono("clock", 1.0); h.tick();
        h.set_mono("clock", 0.0); h.tick();

        h.set_mono("reset", 1.0); h.tick();
        h.set_mono("reset", 0.0); h.tick();

        let d3 = 3.0 + 2.0 / 12.0;
        h.set_mono("clock", 1.0);
        h.tick();
        assert_within!(d3, h.read_mono("pitch"), 1e-12_f32);
        assert_eq!(h.read_mono("trigger"), 1.0);
        assert_eq!(h.read_mono("gate"),    1.0);
    }
}
