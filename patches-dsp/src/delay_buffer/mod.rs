//! Circular delay buffer with power-of-two capacity and multiple interpolation modes.
//!
//! # Buffer sizing
//!
//! The capacity is always the smallest power of two that accommodates the requested
//! number of samples.  Use [`DelayBuffer::for_duration`] when you know the maximum
//! delay in seconds and the sample rate (available from `AudioEnvironment`); use
//! [`DelayBuffer::new`] when you already have a sample count.
//!
//! # Poly layout rationale
//!
//! [`PolyDelayBuffer`] stores samples interleaved as `[f32; 16]` per time step
//! (one cache line) rather than 16 separate buffers.  This wins for the typical
//! usage pattern where all voices are written and read together on every tick:
//!
//! * **Write**: one cache-line write covers all 16 voices.
//! * **Single-tap read**: one cache-line read returns all 16 voices.
//! * **Multi-tap read** (linear = 2, cubic = 4, Thiran = 2): each extra tap
//!   costs one additional cache line; taps are in adjacent memory.
//!
//! Separate per-voice buffers would scatter every read/write across 16 cache lines.

// ─── Mono ────────────────────────────────────────────────────────────────────

/// Mono circular sample buffer with power-of-two capacity.
///
/// After [`push`](DelayBuffer::push), `write` points to the most recently written
/// sample.  [`read_nearest(0)`](DelayBuffer::read_nearest) returns it; larger
/// offsets go further back in time.
pub struct DelayBuffer {
    data: Box<[f32]>,
    mask: usize,
    /// Index of the most recently written sample.
    write: usize,
}

impl DelayBuffer {
    /// Allocate a zero-initialised buffer large enough to hold at least
    /// `min_samples` samples.  The actual capacity is rounded up to the next
    /// power of two.
    ///
    /// # Panics
    /// Panics if `min_samples` is zero or would overflow when rounded up.
    pub fn new(min_samples: usize) -> Self {
        assert!(min_samples > 0, "DelayBuffer requires at least 1 sample");
        let size = min_samples.next_power_of_two();
        Self {
            data: vec![0.0_f32; size].into_boxed_slice(),
            mask: size - 1,
            write: 0,
        }
    }

    /// Allocate a buffer large enough to hold `max_delay_secs` seconds at
    /// `sample_rate` Hz, rounding up to the next power of two.
    pub fn for_duration(max_delay_secs: f32, sample_rate: f32) -> Self {
        let min_samples = (max_delay_secs * sample_rate).ceil() as usize;
        Self::new(min_samples.max(1))
    }

    /// Actual capacity in samples (always a power of two, ≥ the requested size).
    #[inline]
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Write one sample, advancing the write position.
    #[inline]
    pub fn push(&mut self, sample: f32) {
        self.write = self.write.wrapping_add(1) & self.mask;
        self.data[self.write] = sample;
    }

    /// Read the sample `offset` positions back from the write head.
    ///
    /// `offset = 0` → most recently written sample.
    /// `offset = capacity() - 1` → oldest sample.
    /// Wraps with the bitmask; out-of-range offsets return stale or
    /// zero-initialised data (well-defined, never UB).
    #[inline]
    fn read_at(&self, offset: usize) -> f32 {
        self.data[self.write.wrapping_sub(offset) & self.mask]
    }

    /// Integer (nearest-sample) read.
    #[inline]
    pub fn read_nearest(&self, offset: usize) -> f32 {
        self.read_at(offset)
    }

    /// Linear interpolation between the floor and ceiling sample.
    ///
    /// `offset` must be in `[0.0, capacity() as f32)`.
    #[inline]
    pub fn read_linear(&self, offset: f32) -> f32 {
        let i = offset as usize;
        let f = offset - i as f32;
        let x0 = self.read_at(i);
        let x1 = self.read_at(i + 1);
        x0 + f * (x1 - x0)
    }

    /// Catmull-Rom cubic interpolation using four surrounding samples.
    ///
    /// Interpolates between `read_at(floor)` and `read_at(floor + 1)`.  The two
    /// guard taps (`floor - 1` and `floor + 2`) provide curvature.  When
    /// `floor == 0` the lower guard wraps to the oldest slot in the buffer —
    /// harmless once the buffer is fully written, and zero during the initial fill.
    ///
    /// `offset` must be in `[0.0, capacity() as f32 - 2.0]`.
    #[inline]
    pub fn read_cubic(&self, offset: f32) -> f32 {
        let i = offset as usize;
        let f = offset - i as f32;
        let x0 = self.read_at(i.wrapping_sub(1));
        let x1 = self.read_at(i);
        let x2 = self.read_at(i + 1);
        let x3 = self.read_at(i + 2);
        // Catmull-Rom in Horner form: evaluates to x1 at f=0, x2 at f=1.
        let a0 = -0.5 * x0 + 1.5 * x1 - 1.5 * x2 + 0.5 * x3;
        let a1 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
        let a2 = -0.5 * x0 + 0.5 * x2;
        let a3 = x1;
        ((a0 * f + a1) * f + a2) * f + a3
    }
}

// ─── Mono Thiran ─────────────────────────────────────────────────────────────

/// First-order Thiran all-pass interpolation state for a [`DelayBuffer`].
///
/// Thiran all-pass achieves flat group delay across the audio band, making it
/// preferable to polynomial interpolation for modulated delay lines (chorus,
/// vibrato) where phase consistency matters more than time-domain accuracy.
///
/// The coefficient is `a = (1 − η) / (1 + η)` for fractional delay `η ∈ (0, 1)`.
/// Difference equation: `y[n] = a·(x[n] − y[n−1]) + x[n−1]`
/// where `x[n]` = tap at floor offset and `x[n−1]` = tap at floor + 1.
///
/// Keep one `ThiranInterp` instance **per read-head**.  If the fractional part
/// changes abruptly, call [`reset`](ThiranInterp::reset) to avoid a click.
///
/// # Stability
/// The fractional part is clamped to `[FRAC_EPSILON, 1 − FRAC_EPSILON]`
/// to keep the pole away from `z = 1`.
pub struct ThiranInterp {
    y_prev: f32,
}

impl ThiranInterp {
    /// Fractional part is clamped to this range to avoid the pole at `z = 1`.
    pub const FRAC_EPSILON: f32 = 1.0e-3;

    pub fn new() -> Self {
        Self { y_prev: 0.0 }
    }

    /// Zero the internal state.  Call on a discontinuous jump in delay offset.
    pub fn reset(&mut self) {
        self.y_prev = 0.0;
    }

    /// Read from `buf` at fractional `offset`, advancing internal state.
    ///
    /// `offset` must satisfy `0.0 ≤ offset < buf.capacity() as f32 − 1.0`.
    #[inline]
    pub fn read(&mut self, buf: &DelayBuffer, offset: f32) -> f32 {
        let i = offset as usize;
        let frac =
            (offset - i as f32).clamp(Self::FRAC_EPSILON, 1.0 - Self::FRAC_EPSILON);
        let a = (1.0 - frac) / (1.0 + frac);
        let x0 = buf.read_at(i);
        let x1 = buf.read_at(i + 1);
        let y = a * (x0 - self.y_prev) + x1;
        self.y_prev = y;
        y
    }
}

impl Default for ThiranInterp {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Poly ────────────────────────────────────────────────────────────────────

/// 16-voice polyphonic circular delay buffer with power-of-two capacity.
///
/// Samples are stored interleaved as `[f32; 16]` per time step (one cache line).
/// See the [module-level documentation](self) for the layout rationale.
pub struct PolyDelayBuffer {
    data: Box<[[f32; 16]]>,
    mask: usize,
    /// Index of the most recently written frame.
    write: usize,
}

impl PolyDelayBuffer {
    /// Allocate a zero-initialised buffer large enough to hold at least
    /// `min_samples` frames per voice, rounded up to the next power of two.
    ///
    /// # Panics
    /// Panics if `min_samples` is zero or would overflow when rounded up.
    pub fn new(min_samples: usize) -> Self {
        assert!(min_samples > 0, "PolyDelayBuffer requires at least 1 sample");
        let size = min_samples.next_power_of_two();
        Self {
            data: vec![[0.0_f32; 16]; size].into_boxed_slice(),
            mask: size - 1,
            write: 0,
        }
    }

    /// Allocate a buffer large enough for `max_delay_secs` at `sample_rate` Hz,
    /// rounded up to the next power of two.
    pub fn for_duration(max_delay_secs: f32, sample_rate: f32) -> Self {
        let min_samples = (max_delay_secs * sample_rate).ceil() as usize;
        Self::new(min_samples.max(1))
    }

    /// Actual capacity in frames (always a power of two, ≥ the requested size).
    #[inline]
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Write one frame of 16 voice samples, advancing the write position.
    #[inline]
    pub fn push(&mut self, samples: [f32; 16]) {
        self.write = self.write.wrapping_add(1) & self.mask;
        self.data[self.write] = samples;
    }

    #[inline]
    fn read_at(&self, offset: usize) -> [f32; 16] {
        self.data[self.write.wrapping_sub(offset) & self.mask]
    }

    /// Integer (nearest-sample) read for all 16 voices.
    #[inline]
    pub fn read_nearest(&self, offset: usize) -> [f32; 16] {
        self.read_at(offset)
    }

    /// Linear interpolation for all 16 voices.
    #[inline]
    pub fn read_linear(&self, offset: f32) -> [f32; 16] {
        let i = offset as usize;
        let f = offset - i as f32;
        let x0 = self.read_at(i);
        let x1 = self.read_at(i + 1);
        std::array::from_fn(|v| x0[v] + f * (x1[v] - x0[v]))
    }

    /// Catmull-Rom cubic interpolation for all 16 voices.
    ///
    /// The four scalar Catmull-Rom weights are computed once from the fractional
    /// part and applied across all voices, which the compiler can auto-vectorise.
    ///
    /// The same guard-tap wrapping behaviour as [`DelayBuffer::read_cubic`] applies.
    #[inline]
    pub fn read_cubic(&self, offset: f32) -> [f32; 16] {
        let i = offset as usize;
        let f = offset - i as f32;
        let x0 = self.read_at(i.wrapping_sub(1));
        let x1 = self.read_at(i);
        let x2 = self.read_at(i + 1);
        let x3 = self.read_at(i + 2);
        // Weights are scalar (functions of f only), computed once for all voices.
        // Evaluates to x1 at f=0, x2 at f=1 (partition of unity verified in tests).
        let f2 = f * f;
        let f3 = f2 * f;
        let w0 = 0.5 * (-f3 + 2.0 * f2 - f);
        let w1 = 0.5 * (3.0 * f3 - 5.0 * f2 + 2.0);
        let w2 = 0.5 * (-3.0 * f3 + 4.0 * f2 + f);
        let w3 = 0.5 * (f3 - f2);
        std::array::from_fn(|v| w0 * x0[v] + w1 * x1[v] + w2 * x2[v] + w3 * x3[v])
    }
}

// ─── Poly Thiran ─────────────────────────────────────────────────────────────

/// First-order Thiran all-pass interpolation state for a [`PolyDelayBuffer`].
///
/// Maintains independent `y_prev` per voice.  The coefficient `a` is shared
/// (all voices receive the same fractional delay) and computed once per call.
/// See [`ThiranInterp`] for the full description and stability notes.
pub struct PolyThiranInterp {
    y_prev: [f32; 16],
}

impl PolyThiranInterp {
    pub fn new() -> Self {
        Self { y_prev: [0.0; 16] }
    }

    /// Zero all 16 voice states.
    pub fn reset(&mut self) {
        self.y_prev = [0.0; 16];
    }

    /// Read from `buf` at fractional `offset` for all 16 voices, advancing state.
    ///
    /// `offset` must satisfy `0.0 ≤ offset < buf.capacity() as f32 − 1.0`.
    #[inline]
    pub fn read(&mut self, buf: &PolyDelayBuffer, offset: f32) -> [f32; 16] {
        let i = offset as usize;
        let frac = (offset - i as f32)
            .clamp(ThiranInterp::FRAC_EPSILON, 1.0 - ThiranInterp::FRAC_EPSILON);
        let a = (1.0 - frac) / (1.0 + frac);
        let x0 = buf.read_at(i);
        let x1 = buf.read_at(i + 1);
        let mut result = [0.0_f32; 16];
        for v in 0..16 {
            let y = a * (x0[v] - self.y_prev[v]) + x1[v];
            self.y_prev[v] = y;
            result[v] = y;
        }
        result
    }
}

impl Default for PolyThiranInterp {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
