//! Per-slot observation processors (ADR 0056).
//!
//! A `Processor` consumes one block of lane samples and emits zero or
//! more [`Observation`]s. The observer thread loop tags each
//! observation with the lane and processor id before publishing it to
//! the subscriber surface.
//!
//! Identity is `(tap_name, kind, params)`; on replan, processors with
//! matching identity are reused, the rest are rebuilt.

use patches_core::TAP_BLOCK;
use patches_dsl::ast::{Scalar, Value};
use patches_dsl::manifest::{TapDescriptor, TapParamMap, TapType};

/// One observation produced by a processor on a block.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// Scalar level: meter peak, meter rms, gate / trigger LED.
    Level(f32),
    /// FFT bin magnitudes (reserved for future spectrum pipeline).
    Spectrum(Vec<f32>),
    /// Oscilloscope buffer (reserved for future osc pipeline).
    Scope(Vec<f32>),
}

/// Stable identifier for a processor's output stream within a slot.
///
/// `meter` produces two streams: `MeterPeak` and `MeterRms`. Other
/// component types currently produce no streams (stubbed) but their
/// tags are reserved so subscribers can begin polling slots and get
/// zeros until the pipeline is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessorId {
    MeterPeak,
    MeterRms,
}

impl ProcessorId {
    pub const ALL: [ProcessorId; 2] = [ProcessorId::MeterPeak, ProcessorId::MeterRms];

    pub fn index(self) -> usize {
        match self {
            ProcessorId::MeterPeak => 0,
            ProcessorId::MeterRms => 1,
        }
    }

    pub const COUNT: usize = 2;
}

/// Stateless-to-test processor: pure function of (lane block, internal
/// state) → 0+ observations.
pub trait Processor: Send {
    /// The id of the observation stream this processor publishes.
    fn id(&self) -> ProcessorId;

    /// Identity key — `(tap_name, kind, params)` — used by replan to
    /// decide reuse vs rebuild. Two processors with equal keys are
    /// interchangeable (param-driven state may differ in capacity but
    /// will produce the same outputs for the same inputs).
    fn identity(&self) -> &ProcessorIdentity;

    /// Consume one block of lane samples; emit at most one
    /// observation. (Multi-output components decompose into multiple
    /// `Processor`s, one per stream.)
    fn process(&mut self, lane: &[f32; TAP_BLOCK]) -> Option<Observation>;
}

/// Identity key for processor reuse on replan.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessorIdentity {
    pub tap_name: String,
    pub kind: ProcessorId,
    /// Canonical (sorted, stringified) params. Float keys are formatted
    /// to a fixed precision so manifest round-trips compare equal.
    pub params: Vec<(String, String)>,
}

impl ProcessorIdentity {
    pub fn new(tap_name: &str, kind: ProcessorId, params: &TapParamMap) -> Self {
        let mut canon: Vec<(String, String)> = params
            .iter()
            .map(|((q, k), v)| (format!("{q}.{k}"), value_to_string(v)))
            .collect();
        canon.sort();
        Self {
            tap_name: tap_name.to_string(),
            kind,
            params: canon,
        }
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Scalar(Scalar::Int(i)) => i.to_string(),
        Value::Scalar(Scalar::Float(f)) => format!("{f:.6}"),
        Value::Scalar(Scalar::Bool(b)) => b.to_string(),
        Value::Scalar(Scalar::Str(s)) => format!("{s:?}"),
        Value::Scalar(Scalar::ParamRef(s)) => format!("<{s}>"),
        Value::File(p) => format!("file({p:?})"),
    }
}

// ─── Meter pipeline ──────────────────────────────────────────────────────────

/// Meter peak processor: running max-abs with ballistic exponential
/// decay. Per sample: `peak = max(peak * decay_per_sample, |x|)`.
pub struct MeterPeak {
    identity: ProcessorIdentity,
    decay_per_sample: f32,
    state: f32,
}

impl MeterPeak {
    pub fn new(identity: ProcessorIdentity, decay_per_sample: f32) -> Self {
        Self {
            identity,
            decay_per_sample,
            state: 0.0,
        }
    }
}

impl Processor for MeterPeak {
    fn id(&self) -> ProcessorId {
        ProcessorId::MeterPeak
    }
    fn identity(&self) -> &ProcessorIdentity {
        &self.identity
    }
    fn process(&mut self, lane: &[f32; TAP_BLOCK]) -> Option<Observation> {
        let mut p = self.state;
        for &x in lane.iter() {
            p *= self.decay_per_sample;
            let mag = x.abs();
            if mag > p {
                p = mag;
            }
        }
        self.state = p;
        Some(Observation::Level(p))
    }
}

/// Meter RMS processor: rolling-window mean square (sqrt'd at emit).
/// The window is a ring of squared samples. `sum_sq` tracks the running
/// sum so each sample is one add and one subtract.
pub struct MeterRms {
    identity: ProcessorIdentity,
    window: Box<[f32]>,
    head: usize,
    sum_sq: f64,
}

impl MeterRms {
    pub fn new(identity: ProcessorIdentity, window_samples: usize) -> Self {
        let n = window_samples.max(1);
        Self {
            identity,
            window: vec![0.0; n].into_boxed_slice(),
            head: 0,
            sum_sq: 0.0,
        }
    }
}

impl Processor for MeterRms {
    fn id(&self) -> ProcessorId {
        ProcessorId::MeterRms
    }
    fn identity(&self) -> &ProcessorIdentity {
        &self.identity
    }
    fn process(&mut self, lane: &[f32; TAP_BLOCK]) -> Option<Observation> {
        let n = self.window.len();
        for &x in lane.iter() {
            let sq = (x as f64) * (x as f64);
            let old = self.window[self.head] as f64;
            self.sum_sq += sq - old;
            self.window[self.head] = sq as f32;
            self.head = (self.head + 1) % n;
        }
        let mean = (self.sum_sq.max(0.0)) / (n as f64);
        Some(Observation::Level(mean.sqrt() as f32))
    }
}

// ─── Defaults & manifest decoding ────────────────────────────────────────────

/// Default release time for the meter peak ballistic decay (ms).
pub const DEFAULT_METER_DECAY_MS: f32 = 300.0;
/// Default rolling RMS window length (ms).
pub const DEFAULT_METER_WINDOW_MS: f32 = 50.0;

fn decay_per_sample(release_ms: f32, sample_rate: f32) -> f32 {
    if release_ms <= 0.0 || sample_rate <= 0.0 {
        return 0.0;
    }
    (-1.0_f32 / (release_ms * 0.001 * sample_rate)).exp()
}

fn window_samples(window_ms: f32, sample_rate: f32) -> usize {
    let n = (window_ms * 0.001 * sample_rate).round() as i64;
    n.max(1) as usize
}

fn lookup_f32(params: &TapParamMap, qualifier: &str, key: &str) -> Option<f32> {
    for ((q, k), v) in params.iter() {
        if q == qualifier && k == key {
            return match v {
                Value::Scalar(Scalar::Float(f)) => Some(*f as f32),
                Value::Scalar(Scalar::Int(i)) => Some(*i as f32),
                _ => None,
            };
        }
    }
    None
}

/// Build the per-lane processor list for one [`TapDescriptor`].
///
/// Returns `(processors, unimplemented_components)` — the second
/// element is the list of component names that have no real pipeline
/// yet, so the observer can emit a one-shot diagnostic.
pub fn build_pipeline(
    desc: &TapDescriptor,
    sample_rate: f32,
) -> (Vec<Box<dyn Processor>>, Vec<TapType>) {
    let mut out: Vec<Box<dyn Processor>> = Vec::new();
    let mut unimplemented: Vec<TapType> = Vec::new();

    for comp in desc.components.iter().copied() {
        match comp {
            TapType::Meter => {
                let decay_ms = lookup_f32(&desc.params, "meter", "decay")
                    .unwrap_or(DEFAULT_METER_DECAY_MS);
                let window_ms = lookup_f32(&desc.params, "meter", "window")
                    .unwrap_or(DEFAULT_METER_WINDOW_MS);
                let dps = decay_per_sample(decay_ms, sample_rate);
                let win = window_samples(window_ms, sample_rate);
                let id_peak =
                    ProcessorIdentity::new(&desc.name, ProcessorId::MeterPeak, &desc.params);
                let id_rms =
                    ProcessorIdentity::new(&desc.name, ProcessorId::MeterRms, &desc.params);
                out.push(Box::new(MeterPeak::new(id_peak, dps)));
                out.push(Box::new(MeterRms::new(id_rms, win)));
            }
            TapType::Osc | TapType::Spectrum | TapType::GateLed | TapType::TriggerLed => {
                unimplemented.push(comp);
            }
        }
    }

    (out, unimplemented)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::Span;
    use patches_dsl::provenance::Provenance;

    fn empty_provenance() -> Provenance {
        Provenance::root(Span::synthetic())
    }

    fn meter_desc(name: &str, params: TapParamMap) -> TapDescriptor {
        TapDescriptor {
            slot: 0,
            name: name.to_string(),
            components: vec![TapType::Meter],
            params,
            source: empty_provenance(),
        }
    }

    fn dc_block(value: f32) -> [f32; TAP_BLOCK] {
        [value; TAP_BLOCK]
    }

    #[test]
    fn meter_peak_dc_unity_yields_unity() {
        let id = ProcessorIdentity::new("t", ProcessorId::MeterPeak, &vec![]);
        let mut p = MeterPeak::new(id, decay_per_sample(300.0, 48_000.0));
        let obs = p.process(&dc_block(1.0)).unwrap();
        assert_eq!(obs, Observation::Level(1.0));
    }

    #[test]
    fn meter_peak_decays_after_silence() {
        let id = ProcessorIdentity::new("t", ProcessorId::MeterPeak, &vec![]);
        let mut p = MeterPeak::new(id, decay_per_sample(10.0, 48_000.0));
        p.process(&dc_block(1.0));
        // Push silence for several release times (10 ms ≈ 480 samples).
        for _ in 0..64 {
            p.process(&dc_block(0.0));
        }
        let Observation::Level(v) = p.process(&dc_block(0.0)).unwrap() else {
            panic!("expected Level");
        };
        assert!(v < 0.05, "expected decayed peak << 1, got {v}");
    }

    #[test]
    fn meter_rms_dc_unity_settles_to_unity() {
        let id = ProcessorIdentity::new("t", ProcessorId::MeterRms, &vec![]);
        // Window = 1 block exactly so it settles after one push.
        let mut p = MeterRms::new(id, TAP_BLOCK);
        let Observation::Level(v) = p.process(&dc_block(1.0)).unwrap() else {
            panic!("expected Level");
        };
        assert!((v - 1.0).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn build_pipeline_meter_emits_two_processors() {
        let desc = meter_desc("foo", vec![]);
        let (procs, unimpl) = build_pipeline(&desc, 48_000.0);
        assert_eq!(procs.len(), 2);
        assert!(unimpl.is_empty());
        assert_eq!(procs[0].id(), ProcessorId::MeterPeak);
        assert_eq!(procs[1].id(), ProcessorId::MeterRms);
    }

    #[test]
    fn build_pipeline_unimplemented_components_recorded() {
        let desc = TapDescriptor {
            slot: 0,
            name: "bar".into(),
            components: vec![TapType::Osc, TapType::Spectrum],
            params: vec![],
            source: empty_provenance(),
        };
        let (procs, unimpl) = build_pipeline(&desc, 48_000.0);
        assert!(procs.is_empty());
        assert_eq!(unimpl, vec![TapType::Osc, TapType::Spectrum]);
    }

    #[test]
    fn rms_window_is_sample_rate_aware() {
        // 50 ms at 44.1k vs 96k — same wall-clock window length.
        let n44 = window_samples(50.0, 44_100.0);
        let n96 = window_samples(50.0, 96_000.0);
        assert_eq!(n44, 2205);
        assert_eq!(n96, 4800);
    }

    #[test]
    fn identity_canonicalises_param_order() {
        let a: TapParamMap = vec![
            (("meter".into(), "window".into()), Value::Scalar(Scalar::Int(25))),
            (("meter".into(), "decay".into()), Value::Scalar(Scalar::Int(300))),
        ];
        let b: TapParamMap = vec![
            (("meter".into(), "decay".into()), Value::Scalar(Scalar::Int(300))),
            (("meter".into(), "window".into()), Value::Scalar(Scalar::Int(25))),
        ];
        let ia = ProcessorIdentity::new("t", ProcessorId::MeterPeak, &a);
        let ib = ProcessorIdentity::new("t", ProcessorId::MeterPeak, &b);
        assert_eq!(ia, ib);
    }
}
