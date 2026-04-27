//! Per-slot observation processors (ADR 0056).
//!
//! A `Processor` is a per-slot, per-component buffering and analysis unit.
//! The observer thread feeds it one lane block at a time via
//! [`Processor::write_block`] (cheap — typically a ring update). UI
//! readers pull observations from it lazily:
//!
//! - scalar streams (meter peak/rms, gate/trigger) via [`Processor::scalar`];
//! - vector streams (spectrum bins, scope waveform) via
//!   [`Processor::read_into`].
//!
//! The reader-pull split means heavy analysis (FFT, scope linearisation)
//! runs at reader cadence (~30 Hz), not block cadence (~hundreds of Hz),
//! so audio-rate writes are cheap and analysis cost amortises to display
//! rate.
//!
//! Identity is `(tap_name, kind)`; on replan, processors with matching
//! identity are reused, the rest are rebuilt.

use patches_core::TAP_BLOCK;
use patches_dsl::manifest::{TapDescriptor, TapType};

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
    /// Gate LED — scalar 0..1, "on" while the gate signal is held high
    /// with a brief release tail.
    GateLed,
    /// Trigger LED — scalar 0..1, flashes on impulses then decays.
    TriggerLed,
    /// Magnitude spectrum (vector observation). Routes through
    /// [`Processor::read_into`]; not stored in the scalar cell array.
    /// Excluded from [`ProcessorId::ALL`] / [`ProcessorId::COUNT`].
    Spectrum,
    /// Oscilloscope waveform (vector observation). Same surface
    /// pattern as Spectrum.
    Scope,
}

impl ProcessorId {
    /// Scalar processors only — vector streams (`Spectrum`, `Scope`)
    /// live behind [`Processor::read_into`] and are excluded.
    pub const ALL: [ProcessorId; 4] = [
        ProcessorId::MeterPeak,
        ProcessorId::MeterRms,
        ProcessorId::GateLed,
        ProcessorId::TriggerLed,
    ];

    /// Index into the scalar cell array. Panics for non-scalar variants
    /// (e.g. `Spectrum`); callers must route by stream variant before
    /// reaching this.
    pub fn index(self) -> usize {
        match self {
            ProcessorId::MeterPeak => 0,
            ProcessorId::MeterRms => 1,
            ProcessorId::GateLed => 2,
            ProcessorId::TriggerLed => 3,
            ProcessorId::Spectrum => {
                panic!("ProcessorId::Spectrum is a vector stream, not a scalar cell")
            }
            ProcessorId::Scope => {
                panic!("ProcessorId::Scope is a vector stream, not a scalar cell")
            }
        }
    }

    pub const COUNT: usize = 4;
}

/// Buffer + transform contract for a single observation stream.
///
/// `write_block` is called once per audio block on the observer thread
/// and must be cheap (no FFT, no allocation). Analysis runs on the
/// reader's request via `scalar()` (for one-shot scalar streams like
/// meters) or `read_into()` (for vector streams like spectrum/scope).
pub trait Processor: Send {
    fn id(&self) -> ProcessorId;
    fn identity(&self) -> &ProcessorIdentity;

    /// Consume one block of lane samples. Must not allocate or run any
    /// analysis pass — keep this to ring updates and per-sample stats.
    ///
    /// `block_sample_time` is the monotonic sample index of the first
    /// sample in `lane`. Time-base-sensitive processors (e.g. the
    /// oscilloscope, which decimates on a sample_time grid to keep
    /// multiple scope taps phase-locked) use this; stateless processors
    /// ignore it.
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], block_sample_time: u64);

    /// Latest scalar observation. `None` for non-scalar processors.
    /// Default impl returns `None`.
    fn scalar(&self) -> Option<f32> {
        None
    }

    /// Run the vector analysis on the buffered input and copy the
    /// result into `dst`. Returns `true` if a result is available;
    /// `false` if the processor has not yet accumulated enough input
    /// or if it has no vector output. Default impl returns `false`.
    ///
    /// Takes `&mut self` so processors with FFT scratch can run their
    /// transform in-place without internal locking.
    fn read_into(&mut self, dst: &mut Vec<f32>) -> bool {
        let _ = dst;
        false
    }

    /// Downcast hook for typed reader paths
    /// (`SubscribersHandle::read_spectrum_into_with` and friends).
    /// Vector processors override; scalar processors don't need it.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Identity key for processor reuse on replan.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessorIdentity {
    pub tap_name: String,
    pub kind: ProcessorId,
}

impl ProcessorIdentity {
    pub fn new(tap_name: &str, kind: ProcessorId) -> Self {
        Self {
            tap_name: tap_name.to_string(),
            kind,
        }
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
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], _block_sample_time: u64) {
        let mut p = self.state;
        for &x in lane.iter() {
            p *= self.decay_per_sample;
            let mag = x.abs();
            if mag > p {
                p = mag;
            }
        }
        self.state = p;
    }
    fn scalar(&self) -> Option<f32> {
        Some(self.state)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Meter RMS processor: rolling-window mean square (sqrt'd at read).
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
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], _block_sample_time: u64) {
        let n = self.window.len();
        for &x in lane.iter() {
            let sq = (x as f64) * (x as f64);
            let old = self.window[self.head] as f64;
            self.sum_sq += sq - old;
            self.window[self.head] = sq as f32;
            self.head = (self.head + 1) % n;
        }
    }
    fn scalar(&self) -> Option<f32> {
        let n = self.window.len();
        let mean = self.sum_sq.max(0.0) / (n as f64);
        Some(mean.sqrt() as f32)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ─── LED pipelines ───────────────────────────────────────────────────────────

/// Gate-detection threshold: any sample with `|x| >= GATE_ON_THRESHOLD`
/// is treated as the gate being asserted.
pub const GATE_ON_THRESHOLD: f32 = 0.5;

/// Default release time (ms) for the gate LED's hold tail. The LED
/// shows the raw gate level when held, plus an exponential release so
/// brief pulses stay visible at UI refresh cadence.
pub const DEFAULT_GATE_RELEASE_MS: f32 = 80.0;

// (Trigger LED has no audio-side decay — it latches a fired flag and
// the consumer clears it on read; visual fade lives in the UI.)

/// Gate LED processor. Output scalar is 1.0 while any sample in the
/// block is at or above the gate-on threshold; otherwise the previous
/// state decays exponentially with `decay_per_sample`.
pub struct GateLed {
    identity: ProcessorIdentity,
    decay_per_sample: f32,
    state: f32,
}

impl GateLed {
    pub fn new(identity: ProcessorIdentity, decay_per_sample: f32) -> Self {
        Self { identity, decay_per_sample, state: 0.0 }
    }
}

impl Processor for GateLed {
    fn id(&self) -> ProcessorId { ProcessorId::GateLed }
    fn identity(&self) -> &ProcessorIdentity { &self.identity }
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], _block_sample_time: u64) {
        let mut s = self.state;
        for &x in lane.iter() {
            s *= self.decay_per_sample;
            if x.abs() >= GATE_ON_THRESHOLD && s < 1.0 {
                s = 1.0;
            }
        }
        self.state = s;
    }
    fn scalar(&self) -> Option<f32> { Some(self.state) }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

/// Trigger LED processor. Latches a "fired since last publish" flag
/// whenever any sample is non-zero (trigger-cable convention: 0.0 = no
/// event, fractional sample index in (0, 1] = event). The flag is
/// reported once via `scalar()` then cleared, so the published cell
/// holds 1.0 until the consumer swaps it back to 0 — no audio-side
/// decay. Visual fade is the UI's job.
pub struct TriggerLed {
    identity: ProcessorIdentity,
    fired: std::cell::Cell<bool>,
}

impl TriggerLed {
    pub fn new(identity: ProcessorIdentity) -> Self {
        Self { identity, fired: std::cell::Cell::new(false) }
    }
}

impl Processor for TriggerLed {
    fn id(&self) -> ProcessorId { ProcessorId::TriggerLed }
    fn identity(&self) -> &ProcessorIdentity { &self.identity }
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], _block_sample_time: u64) {
        if lane.iter().any(|&x| x != 0.0) {
            self.fired.set(true);
        }
    }
    /// One-shot: returns `Some(1.0)` exactly once after a fire and clears
    /// the flag, `None` otherwise. The observer publishes only on `Some`,
    /// so the cell latches at 1.0 until the consumer takes it.
    fn scalar(&self) -> Option<f32> {
        if self.fired.replace(false) { Some(1.0) } else { None }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

// ─── Spectrum pipeline ───────────────────────────────────────────────────────

/// Maximum FFT size the spectrum processor will compute. Buffer is
/// sized to hold this many raw samples; client picks the actual FFT
/// size (any power of two ≤ this) at read time.
pub const SPECTRUM_FFT_SIZE_MAX: usize = 4096;

/// Allowed FFT sizes for client requests. Other sizes round down to
/// the nearest entry.
pub const SPECTRUM_FFT_SIZES: [usize; 3] = [1024, 2048, 4096];

/// Default FFT size used when the client doesn't specify one.
pub const SPECTRUM_FFT_SIZE_DEFAULT: usize = 1024;

/// Bin count for an FFT of size `n`: `n / 2 + 1` (DC through Nyquist).
pub const fn spectrum_bin_count(n: usize) -> usize {
    n / 2 + 1
}

/// Bin count at the maximum supported FFT size.
pub const SPECTRUM_BIN_COUNT: usize = spectrum_bin_count(SPECTRUM_FFT_SIZE_MAX);

/// Read-time options for the spectrum processor.
#[derive(Clone, Copy, Debug)]
pub struct SpectrumReadOpts {
    /// Requested FFT size. Rounded down to the nearest member of
    /// [`SPECTRUM_FFT_SIZES`]; out-of-range values clamp to bounds.
    pub fft_size: usize,
}

impl Default for SpectrumReadOpts {
    fn default() -> Self {
        Self { fft_size: SPECTRUM_FFT_SIZE_DEFAULT }
    }
}

impl SpectrumReadOpts {
    /// Round `fft_size` down to a supported size. Out-of-range values
    /// clamp to the nearest endpoint.
    pub fn resolve_fft_size(&self) -> usize {
        let req = self.fft_size;
        let mut chosen = SPECTRUM_FFT_SIZES[0];
        for &n in SPECTRUM_FFT_SIZES.iter() {
            if req >= n {
                chosen = n;
            }
        }
        chosen
    }
}

/// Magnitude-spectrum processor with a Hann-windowed FFT computed at
/// reader cadence.
///
/// `write_block` is a memcpy into a fixed-size raw sample ring sized
/// for the largest supported FFT. `read_into` picks a window of
/// `opts.fft_size` samples from the tail of the ring (oldest → newest),
/// applies a Hann window of matching length, runs a real-FFT, and
/// copies per-bin magnitudes into `dst`. Phase is discarded.
///
/// Hann windows are precomputed for each supported FFT size and held
/// in an array indexed by [`SPECTRUM_FFT_SIZES`] order. FFT instances
/// likewise — initialising a `RealPackedFft` is the priciest part of
/// construction but happens once.
pub struct Spectrum {
    identity: ProcessorIdentity,
    /// FFT instance per supported size.
    ffts: [patches_dsp::fft::RealPackedFft; SPECTRUM_FFT_SIZES.len()],
    /// Hann windows per supported size.
    windows: [Box<[f32]>; SPECTRUM_FFT_SIZES.len()],
    /// Raw sample ring (single-sample granularity), sized for the
    /// max FFT. Newest sample at `write_idx - 1` (mod len).
    ring: Box<[f32; SPECTRUM_FFT_SIZE_MAX]>,
    write_idx: usize,
    /// Total samples ever written; used to gate the first read until
    /// the requested window has been filled.
    sample_count: u64,
    /// FFT scratch (windowed time-domain → packed frequency-domain).
    /// Sized for max FFT; the active prefix is `opts.fft_size`.
    scratch: Box<[f32]>,
    /// Reusable magnitude buffer (sized for max bin count).
    mags: Vec<f32>,
}

fn hann_window(n: usize) -> Box<[f32]> {
    (0..n)
        .map(|i| {
            let t = (i as f64) * std::f64::consts::TAU / (n as f64);
            (0.5 * (1.0 - t.cos())) as f32
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

impl Spectrum {
    pub fn new(identity: ProcessorIdentity) -> Self {
        let ffts = [
            patches_dsp::fft::RealPackedFft::new(SPECTRUM_FFT_SIZES[0]),
            patches_dsp::fft::RealPackedFft::new(SPECTRUM_FFT_SIZES[1]),
            patches_dsp::fft::RealPackedFft::new(SPECTRUM_FFT_SIZES[2]),
        ];
        let windows = [
            hann_window(SPECTRUM_FFT_SIZES[0]),
            hann_window(SPECTRUM_FFT_SIZES[1]),
            hann_window(SPECTRUM_FFT_SIZES[2]),
        ];
        Self {
            identity,
            ffts,
            windows,
            ring: Box::new([0.0; SPECTRUM_FFT_SIZE_MAX]),
            write_idx: 0,
            sample_count: 0,
            scratch: vec![0.0; SPECTRUM_FFT_SIZE_MAX].into_boxed_slice(),
            mags: vec![0.0; SPECTRUM_BIN_COUNT],
        }
    }

    fn fft_index(n: usize) -> Option<usize> {
        SPECTRUM_FFT_SIZES.iter().position(|&s| s == n)
    }

    /// Compute magnitudes for an FFT of size `n` over the latest `n`
    /// samples of the ring, applying the precomputed Hann window. The
    /// result lands in `self.mags[..n/2 + 1]`.
    fn compute(&mut self, n: usize) {
        let idx = Self::fft_index(n).expect("validated fft_size");
        // Assemble windowed input: oldest of the latest n samples first.
        let ring_len = self.ring.len();
        // The next-write slot points to the oldest sample in a fully-
        // filled ring. For the last n samples, the start is
        // `write_idx + (ring_len - n)` mod ring_len.
        let start = (self.write_idx + ring_len - n) % ring_len;
        let win = &self.windows[idx];
        for i in 0..n {
            let src = (start + i) % ring_len;
            self.scratch[i] = self.ring[src] * win[i];
        }
        self.ffts[idx].forward(&mut self.scratch[..n]);
        // Packed CMSIS layout: [0]=DC re, [1]=Nyquist re,
        // [2k]=re, [2k+1]=im for k=1..n/2-1.
        let half = n / 2;
        let norm = 2.0 / n as f32;
        self.mags[0] = self.scratch[0].abs() * norm;
        self.mags[half] = self.scratch[1].abs() * norm;
        for k in 1..half {
            let re = self.scratch[2 * k];
            let im = self.scratch[2 * k + 1];
            self.mags[k] = (re * re + im * im).sqrt() * norm;
        }
    }

    /// Run the spectrum analysis with `opts` and copy the result into
    /// `dst`. Returns `false` if the ring hasn't yet accumulated
    /// `opts.fft_size` samples.
    pub fn read_with(&mut self, opts: SpectrumReadOpts, dst: &mut Vec<f32>) -> bool {
        let n = opts.resolve_fft_size();
        if (self.sample_count as usize) < n {
            return false;
        }
        self.compute(n);
        let bins = spectrum_bin_count(n);
        dst.clear();
        dst.extend_from_slice(&self.mags[..bins]);
        true
    }
}

impl Processor for Spectrum {
    fn id(&self) -> ProcessorId {
        ProcessorId::Spectrum
    }
    fn identity(&self) -> &ProcessorIdentity {
        &self.identity
    }
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], _block_sample_time: u64) {
        let n = self.ring.len();
        for &x in lane.iter() {
            self.ring[self.write_idx] = x;
            self.write_idx = (self.write_idx + 1) % n;
        }
        self.sample_count = self.sample_count.saturating_add(TAP_BLOCK as u64);
    }
    fn read_into(&mut self, dst: &mut Vec<f32>) -> bool {
        self.read_with(SpectrumReadOpts::default(), dst)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ─── Oscilloscope pipeline ───────────────────────────────────────────────────

/// Raw-sample ring length. Sized to ~680 ms at 48 kHz; the full ring
/// is sample-time-aligned across taps (sample 0 is divisible by every
/// decimation factor a client could plausibly request).
pub const SCOPE_RING_SAMPLES: usize = 32_768;

/// Allowed display-window decimation factors. Display sample stride =
/// `SCOPE_DECIMATION_*`, so a window of `n` display samples covers
/// `n * decimation` raw samples.
pub const SCOPE_DECIMATION_DEFAULT: usize = 16;

/// Default display-window length in samples.
pub const SCOPE_WINDOW_DEFAULT: usize = 512;

/// Lower / upper clamp on requested display-window length.
pub const SCOPE_WINDOW_MIN: usize = 32;
pub const SCOPE_WINDOW_MAX: usize = SCOPE_RING_SAMPLES;

/// Read-time options for the oscilloscope.
#[derive(Clone, Copy, Debug)]
pub struct ScopeReadOpts {
    /// Display-sample stride. `1` for raw, `16` for the historical
    /// default. Must be ≥ 1; clamped to ring capacity.
    pub decimation: usize,
    /// Number of display samples to return. Clamped so
    /// `decimation * window_samples <= SCOPE_RING_SAMPLES`.
    pub window_samples: usize,
}

impl Default for ScopeReadOpts {
    fn default() -> Self {
        Self {
            decimation: SCOPE_DECIMATION_DEFAULT,
            window_samples: SCOPE_WINDOW_DEFAULT,
        }
    }
}

impl ScopeReadOpts {
    /// Resolve to in-bounds (decimation, window_samples).
    fn resolve(&self) -> (usize, usize) {
        let dec = self.decimation.max(1);
        let max_window = SCOPE_RING_SAMPLES / dec;
        let win = self
            .window_samples
            .clamp(SCOPE_WINDOW_MIN.min(max_window), max_window);
        (dec, win)
    }
}

/// Oscilloscope processor. Holds a fixed-size ring of raw (un-decimated)
/// audio samples; the client picks decimation and window length at
/// read time, and the processor emits the latest `window_samples`
/// decimated samples in order. Sample-time alignment is preserved
/// across taps because `write_idx` advances monotonically with the
/// global sample stream.
///
/// Display-side transforms (zero-cross snap, triggering, etc.) are
/// the client's responsibility — the server surface stays raw so they
/// can be toggled live.
pub struct Oscilloscope {
    identity: ProcessorIdentity,
    ring: Box<[f32; SCOPE_RING_SAMPLES]>,
    write_idx: usize,
    sample_count: u64,
}

impl Oscilloscope {
    pub fn new(identity: ProcessorIdentity) -> Self {
        Self {
            identity,
            ring: Box::new([0.0; SCOPE_RING_SAMPLES]),
            write_idx: 0,
            sample_count: 0,
        }
    }

    pub fn with_params(identity: ProcessorIdentity, _ring_len: usize) -> Self {
        // Ring length is now fixed; legacy parameter kept for callers
        // that pre-date the read-time decimation API.
        Self::new(identity)
    }

    /// Pull the latest `window_samples` decimated samples (stride
    /// `decimation`) into `dst` in oldest → newest order. Returns
    /// `false` until enough raw samples have been buffered.
    pub fn read_with(&mut self, opts: ScopeReadOpts, dst: &mut Vec<f32>) -> bool {
        let (dec, win) = opts.resolve();
        let span = dec.saturating_mul(win);
        if span == 0 || (self.sample_count as usize) < span {
            return false;
        }
        let ring_len = self.ring.len();
        // Latest sample at `(write_idx + ring_len - 1) % ring_len`.
        // Anchor the decimation grid on the global sample stream so
        // multiple scope taps stay phase-locked: pick samples whose
        // absolute sample index satisfies `i % dec == (latest_i) %
        // dec`. With `latest_i = sample_count - 1`, walk backwards.
        let latest_i = (self.sample_count - 1) as usize;
        // Build oldest → newest:
        dst.clear();
        dst.reserve(win);
        for k in 0..win {
            // Position from latest: (win - 1 - k) decimation steps back.
            let steps_back = win - 1 - k;
            let i = latest_i.saturating_sub(steps_back * dec);
            // Map absolute index `i` to ring slot via wrap.
            // `write_idx` points at the *next* slot to write, so the
            // sample at absolute index `i` lives at
            // `(write_idx - (latest_i + 1 - i)) mod ring_len`
            //   = `(write_idx + ring_len - (latest_i + 1 - i)) % ring_len`.
            let offset_back = latest_i + 1 - i;
            let slot = (self.write_idx + ring_len - offset_back) % ring_len;
            dst.push(self.ring[slot]);
        }
        true
    }
}

impl Processor for Oscilloscope {
    fn id(&self) -> ProcessorId {
        ProcessorId::Scope
    }
    fn identity(&self) -> &ProcessorIdentity {
        &self.identity
    }
    fn write_block(&mut self, lane: &[f32; TAP_BLOCK], _block_sample_time: u64) {
        let n = self.ring.len();
        for &x in lane.iter() {
            self.ring[self.write_idx] = x;
            self.write_idx = (self.write_idx + 1) % n;
        }
        self.sample_count = self.sample_count.saturating_add(TAP_BLOCK as u64);
    }
    fn read_into(&mut self, dst: &mut Vec<f32>) -> bool {
        self.read_with(ScopeReadOpts::default(), dst)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
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

/// Build the per-lane processor list for one [`TapDescriptor`].
///
/// All per-tap configuration (FFT size, decimation, RMS window, peak
/// decay, etc.) is resolved at read time by the client via
/// `SubscribersHandle` typed read options; the observer constructs
/// processors with built-in defaults.
pub fn build_pipeline(
    desc: &TapDescriptor,
    sample_rate: f32,
) -> (Vec<Box<dyn Processor>>, Vec<TapType>) {
    let mut out: Vec<Box<dyn Processor>> = Vec::new();
    let unimplemented: Vec<TapType> = Vec::new();

    for comp in desc.components.iter().copied() {
        match comp {
            TapType::Meter => {
                let dps = decay_per_sample(DEFAULT_METER_DECAY_MS, sample_rate);
                let win = window_samples(DEFAULT_METER_WINDOW_MS, sample_rate);
                let id_peak = ProcessorIdentity::new(&desc.name, ProcessorId::MeterPeak);
                let id_rms = ProcessorIdentity::new(&desc.name, ProcessorId::MeterRms);
                out.push(Box::new(MeterPeak::new(id_peak, dps)));
                out.push(Box::new(MeterRms::new(id_rms, win)));
            }
            TapType::Spectrum => {
                let id = ProcessorIdentity::new(&desc.name, ProcessorId::Spectrum);
                out.push(Box::new(Spectrum::new(id)));
            }
            TapType::Osc => {
                let id = ProcessorIdentity::new(&desc.name, ProcessorId::Scope);
                out.push(Box::new(Oscilloscope::new(id)));
            }
            TapType::GateLed => {
                let dps = decay_per_sample(DEFAULT_GATE_RELEASE_MS, sample_rate);
                let id = ProcessorIdentity::new(&desc.name, ProcessorId::GateLed);
                out.push(Box::new(GateLed::new(id, dps)));
            }
            TapType::TriggerLed => {
                let id = ProcessorIdentity::new(&desc.name, ProcessorId::TriggerLed);
                out.push(Box::new(TriggerLed::new(id)));
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

    fn meter_desc(name: &str) -> TapDescriptor {
        TapDescriptor {
            slot: 0,
            name: name.to_string(),
            components: vec![TapType::Meter],
            source: empty_provenance(),
        }
    }

    fn dc_block(value: f32) -> [f32; TAP_BLOCK] {
        [value; TAP_BLOCK]
    }

    #[test]
    fn meter_peak_dc_unity_yields_unity() {
        let id = ProcessorIdentity::new("t", ProcessorId::MeterPeak);
        let mut p = MeterPeak::new(id, decay_per_sample(300.0, 48_000.0));
        p.write_block(&dc_block(1.0), 0);
        assert_eq!(p.scalar(), Some(1.0));
    }

    #[test]
    fn meter_peak_decays_after_silence() {
        let id = ProcessorIdentity::new("t", ProcessorId::MeterPeak);
        let mut p = MeterPeak::new(id, decay_per_sample(10.0, 48_000.0));
        p.write_block(&dc_block(1.0), 0);
        for _ in 0..64 {
            p.write_block(&dc_block(0.0), 0);
        }
        let v = p.scalar().expect("scalar");
        assert!(v < 0.05, "expected decayed peak << 1, got {v}");
    }

    #[test]
    fn meter_rms_dc_unity_settles_to_unity() {
        let id = ProcessorIdentity::new("t", ProcessorId::MeterRms);
        let mut p = MeterRms::new(id, TAP_BLOCK);
        p.write_block(&dc_block(1.0), 0);
        let v = p.scalar().expect("scalar");
        assert!((v - 1.0).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn build_pipeline_meter_emits_two_processors() {
        let desc = meter_desc("foo");
        let (procs, unimpl) = build_pipeline(&desc, 48_000.0);
        assert_eq!(procs.len(), 2);
        assert!(unimpl.is_empty());
        assert_eq!(procs[0].id(), ProcessorId::MeterPeak);
        assert_eq!(procs[1].id(), ProcessorId::MeterRms);
    }

    #[test]
    fn build_pipeline_emits_led_processors() {
        let desc = TapDescriptor {
            slot: 0,
            name: "bar".into(),
            components: vec![TapType::GateLed, TapType::TriggerLed],
            source: empty_provenance(),
        };
        let (procs, unimpl) = build_pipeline(&desc, 48_000.0);
        assert!(unimpl.is_empty());
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[0].id(), ProcessorId::GateLed);
        assert_eq!(procs[1].id(), ProcessorId::TriggerLed);
    }

    #[test]
    fn gate_led_holds_then_releases() {
        let id = ProcessorIdentity::new("g", ProcessorId::GateLed);
        let mut p = GateLed::new(id, decay_per_sample(80.0, 48_000.0));
        p.write_block(&dc_block(1.0), 0);
        assert!((p.scalar().unwrap() - 1.0).abs() < 1e-6);
        for _ in 0..256 {
            p.write_block(&dc_block(0.0), 0);
        }
        let v = p.scalar().unwrap();
        assert!(v < 1.0 && v >= 0.0, "expected partial decay, got {v}");
    }

    #[test]
    fn trigger_led_latches_one_shot_then_clears() {
        let id = ProcessorIdentity::new("t", ProcessorId::TriggerLed);
        let mut p = TriggerLed::new(id);
        let mut block = [0.0f32; TAP_BLOCK];
        block[3] = 0.42;
        p.write_block(&block, 0);
        assert_eq!(p.scalar(), Some(1.0));
        // Second read in the same tick yields nothing — flag was cleared.
        assert_eq!(p.scalar(), None);
        // No fire on the next block: still nothing.
        p.write_block(&[0.0; TAP_BLOCK], 0);
        assert_eq!(p.scalar(), None);
    }

    #[test]
    fn rms_window_is_sample_rate_aware() {
        let n44 = window_samples(50.0, 44_100.0);
        let n96 = window_samples(50.0, 96_000.0);
        assert_eq!(n44, 2205);
        assert_eq!(n96, 4800);
    }

    fn blocks_for_samples(n: usize) -> usize {
        n.div_ceil(TAP_BLOCK)
    }

    #[test]
    fn spectrum_read_before_full_window_returns_false() {
        let id = ProcessorIdentity::new("s", ProcessorId::Spectrum);
        let mut p = Spectrum::new(id);
        // Push fewer than the default FFT size of samples.
        for _ in 0..(blocks_for_samples(SPECTRUM_FFT_SIZE_DEFAULT) - 1) {
            p.write_block(&dc_block(0.0), 0);
        }
        let mut out = Vec::new();
        assert!(!p.read_into(&mut out));
    }

    #[test]
    fn spectrum_read_after_full_window_returns_bins() {
        let id = ProcessorIdentity::new("s", ProcessorId::Spectrum);
        let mut p = Spectrum::new(id);
        for _ in 0..blocks_for_samples(SPECTRUM_FFT_SIZE_DEFAULT) {
            p.write_block(&dc_block(0.0), 0);
        }
        let mut out = Vec::new();
        assert!(p.read_into(&mut out));
        assert_eq!(out.len(), spectrum_bin_count(SPECTRUM_FFT_SIZE_DEFAULT));
    }

    #[test]
    fn spectrum_pure_tone_peaks_at_expected_bin() {
        let id = ProcessorIdentity::new("s", ProcessorId::Spectrum);
        let mut p = Spectrum::new(id);
        let sr = 48_000.0_f32;
        let freq = 1_000.0_f32;
        let n = SPECTRUM_FFT_SIZE_DEFAULT;
        let mut sample_idx = 0usize;
        while sample_idx < n {
            let mut block = [0.0f32; TAP_BLOCK];
            for s in block.iter_mut() {
                let t = sample_idx as f32 / sr;
                *s = (std::f32::consts::TAU * freq * t).sin();
                sample_idx += 1;
            }
            p.write_block(&block, 0);
        }
        let mut v = Vec::new();
        assert!(p.read_into(&mut v));
        let (peak_bin, _) = v
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        let expected_bin = (freq / (sr / n as f32)).round() as usize;
        let diff = peak_bin.abs_diff(expected_bin);
        assert!(diff <= 2, "peak bin {peak_bin} too far from expected {expected_bin}");
    }

    #[test]
    fn spectrum_dc_input_concentrates_in_bin_zero() {
        let id = ProcessorIdentity::new("s", ProcessorId::Spectrum);
        let mut p = Spectrum::new(id);
        for _ in 0..blocks_for_samples(SPECTRUM_FFT_SIZE_DEFAULT) {
            p.write_block(&dc_block(1.0), 0);
        }
        let mut v = Vec::new();
        assert!(p.read_into(&mut v));
        let (peak_bin, _) = v
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!(peak_bin <= 1, "DC should peak at bin 0 (or 1), got {peak_bin}");
    }

    #[test]
    fn spectrum_supports_2048_and_4096_fft_sizes() {
        let id = ProcessorIdentity::new("s", ProcessorId::Spectrum);
        let mut p = Spectrum::new(id);
        // Fill enough for 4096-size FFT.
        for _ in 0..blocks_for_samples(4096) {
            p.write_block(&dc_block(0.5), 0);
        }
        for &n in &[1024usize, 2048, 4096] {
            let mut v = Vec::new();
            let opts = SpectrumReadOpts { fft_size: n };
            assert!(p.read_with(opts, &mut v), "fft_size {n} read failed");
            assert_eq!(v.len(), spectrum_bin_count(n), "wrong bin count for {n}");
        }
    }

    #[test]
    fn scope_decimation_emits_strided_samples() {
        // Drive the processor with samples whose value equals their
        // absolute sample index. With the default decimation of 16
        // and a 512-sample window, the read should return the latest
        // 512 samples whose absolute index is on a 16-sample stride.
        let id = ProcessorIdentity::new("o", ProcessorId::Scope);
        let mut p = Oscilloscope::new(id);
        let opts = ScopeReadOpts::default();
        let span = opts.decimation * opts.window_samples;
        let blocks = span.div_ceil(TAP_BLOCK);
        for b in 0..blocks {
            let t0 = (b * TAP_BLOCK) as u64;
            let mut block = [0.0f32; TAP_BLOCK];
            for (i, s) in block.iter_mut().enumerate() {
                *s = (t0 + i as u64) as f32;
            }
            p.write_block(&block, t0);
        }
        let mut v = Vec::new();
        assert!(p.read_with(opts, &mut v));
        assert_eq!(v.len(), opts.window_samples);
        for w in v.windows(2) {
            assert_eq!(w[1] as u64 - w[0] as u64, opts.decimation as u64);
        }
    }

    #[test]
    fn scope_supports_client_chosen_decimation_and_window() {
        let id = ProcessorIdentity::new("o", ProcessorId::Scope);
        let mut p = Oscilloscope::new(id);
        // Push enough samples for any reasonable request.
        for b in 0..(SCOPE_RING_SAMPLES.div_ceil(TAP_BLOCK)) {
            let t0 = (b * TAP_BLOCK) as u64;
            let mut block = [0.0f32; TAP_BLOCK];
            for (i, s) in block.iter_mut().enumerate() {
                *s = (t0 + i as u64) as f32;
            }
            p.write_block(&block, t0);
        }
        for &(dec, win) in &[(1usize, 256usize), (4, 1024), (32, 256)] {
            let mut v = Vec::new();
            let opts = ScopeReadOpts { decimation: dec, window_samples: win };
            assert!(p.read_with(opts, &mut v));
            assert_eq!(v.len(), win);
            for w in v.windows(2) {
                assert_eq!(w[1] as u64 - w[0] as u64, dec as u64);
            }
        }
    }

    #[test]
    fn build_pipeline_osc_emits_scope_processor() {
        let desc = TapDescriptor {
            slot: 0,
            name: "o".into(),
            components: vec![TapType::Osc],
            source: empty_provenance(),
        };
        let (procs, unimpl) = build_pipeline(&desc, 48_000.0);
        assert_eq!(procs.len(), 1);
        assert!(unimpl.is_empty());
        assert_eq!(procs[0].id(), ProcessorId::Scope);
    }

    #[test]
    fn build_pipeline_spectrum_emits_one_processor() {
        let desc = TapDescriptor {
            slot: 0,
            name: "s".into(),
            components: vec![TapType::Spectrum],
            source: empty_provenance(),
        };
        let (procs, unimpl) = build_pipeline(&desc, 48_000.0);
        assert_eq!(procs.len(), 1);
        assert!(unimpl.is_empty());
        assert_eq!(procs[0].id(), ProcessorId::Spectrum);
    }
}
